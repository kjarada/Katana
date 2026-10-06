// Field to finish in one step (include/katana/cad/survey_finish.hpp).
//
// In customisation/ beside linework.cpp, whose two entry points it runs, and
// because that directory is globbed into katana_cad.

#include "katana/cad/survey_finish.hpp"

#include <algorithm>
#include <map>
#include <set>
#include <unordered_map>
#include <unordered_set>
#include <utility>

#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

namespace {

namespace cmd = katana::commands;
namespace survey = katana::survey;
using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::EntityId;

// How many codes a sentence names before it says "and N more": enough to
// recognise the file, few enough to stay one line (survey_points.cpp names
// point ids by the same limit).
constexpr std::size_t kNamedCodes = 12;

[[nodiscard]] std::string namedCodes(const std::vector<std::string>& codes)
{
    std::string text;
    for (std::size_t i = 0; i < codes.size() && i < kNamedCodes; ++i) {
        text += (i == 0 ? "" : ", ") + codes[i];
    }
    if (codes.size() > kNamedCodes) {
        text += " and " + std::to_string(codes.size() - kNamedCodes) + " more";
    }
    return text;
}

[[nodiscard]] bool isPoint(const Entity* entity)
{
    return entity != nullptr &&
           std::holds_alternative<katana::entity::PointGeometry>(entity->geometry);
}

// A failure of one step, naming it: the message stays the step's own, which
// says what is wrong, and the context says where in the import it happened.
[[nodiscard]] Error failed(const Error& error, std::string_view step)
{
    return makeError(error.code, error.message,
                     "finishing the survey import: " + std::string(step) +
                         (error.context.empty() ? std::string{} : " " + error.context));
}

class SurveyFinishCommand final : public cmd::Command {
  public:
    SurveyFinishCommand(const Document& document, cmd::CommandPtr points,
                        survey::SurveyProject strings, SurveyImportOptions import,
                        SurveyFinishOptions options, std::shared_ptr<SurveyFinishReport> report,
                        SurveyFinishEarlier earlier)
        : document_(document), points_(std::move(points)), strings_(std::move(strings)),
          import_(std::move(import)), options_(std::move(options)), report_(std::move(report)),
          earlier_(std::move(earlier))
    {
    }

    // The import it is: one step in the history, under the import's name.
    [[nodiscard]] std::string_view name() const override { return points_->name(); }
    [[nodiscard]] bool isDestructive() const override { return points_->isDestructive(); }

    [[nodiscard]] Status validate(const cmd::CommandContext& context) const override
    {
        // Every step is planned against the Document; run on another model it
        // would code whatever that drawing's entities of the same ids are.
        if (&context.model != &document_.model()) {
            return makeError(ErrorCode::InvalidState,
                             "the survey import was planned for another document");
        }
        // Refused before anything is drawn rather than after the points are.
        if (options_.linework) {
            if (auto status = katana::entity::validate(options_.controls); !status) {
                return status;
            }
        }
        return points_->validate(context);
    }

    [[nodiscard]] Status execute(cmd::CommandContext& context) override
    {
        if (auto status = points_->execute(context); !status) {
            return status;
        }
        steps_.clear();
        SurveyFinishReport report;
        if (auto status = finish(context, report); !status) {
            // All or nothing: an import that asked for codes and got none
            // looks like one whose codes matched nothing.
            for (auto step = steps_.rbegin(); step != steps_.rend(); ++step) {
                (void)(*step)->undo(context);
            }
            steps_.clear();
            (void)points_->undo(context);
            return status;
        }
        if (report_ != nullptr) {
            *report_ = std::move(report);
        }
        // Redo replays the steps and never plans again, so what they were
        // planned from is let go: this command lives on in the undo history,
        // and the reduced survey there - every point with its metadata and
        // its source record, every feature - would be dead weight. (The job
        // commands let their raw project go for the same reason.)
        strings_ = {};
        earlier_ = {};
        return {};
    }

    [[nodiscard]] Status undo(cmd::CommandContext& context) override
    {
        for (auto step = steps_.rbegin(); step != steps_.rend(); ++step) {
            if (auto status = (*step)->undo(context); !status) {
                return status;
            }
        }
        return points_->undo(context);
    }

    // Replays what was planned when it ran, never plans again: the ids, the
    // layers and the styles come back exactly.
    [[nodiscard]] Status redo(cmd::CommandContext& context) override
    {
        if (auto status = points_->redo(context); !status) {
            return status;
        }
        for (const cmd::CommandPtr& step : steps_) {
            if (auto status = step->redo(context); !status) {
                return status;
            }
        }
        return {};
    }

    // The points, then the lines.
    [[nodiscard]] std::vector<EntityId> createdEntities() const override
    {
        std::vector<EntityId> created = points_->createdEntities();
        for (const cmd::CommandPtr& step : steps_) {
            const auto more = step->createdEntities();
            created.insert(created.end(), more.begin(), more.end());
        }
        return created;
    }

  private:
    [[nodiscard]] Status run(cmd::CommandContext& context, cmd::CommandPtr command,
                             std::string_view step)
    {
        Status status = command->validate(context);
        if (status) {
            status = command->execute(context);
        }
        if (!status) {
            return failed(status.error(), step);
        }
        steps_.push_back(std::move(command));
        return {};
    }

    // A step that draws lines: run, and the lines it created listed.
    [[nodiscard]] Status runLines(cmd::CommandContext& context, cmd::CommandPtr command,
                                  std::string_view step, SurveyFinishReport& report)
    {
        if (auto status = run(context, std::move(command), step); !status) {
            return status;
        }
        const auto drawn = steps_.back()->createdEntities();
        report.lines.insert(report.lines.end(), drawn.begin(), drawn.end());
        return {};
    }

    // The point number the import wrote on `entity`, as the job pairs its
    // points by it; empty when it wrote none.
    [[nodiscard]] std::string numberOf(const Entity& entity) const
    {
        if (import_.pointNumberProperty.empty()) {
            return {};
        }
        const auto found = entity.properties.find(import_.pointNumberProperty);
        return found == entity.properties.end() ? std::string{}
                                                : katana::entity::toString(found->second);
    }

    // The code `entity` is of: the first token of its code field, which is
    // how a point's code is read everywhere (parseFieldCode).
    [[nodiscard]] std::string codeOf(const Entity& entity) const
    {
        const std::string* text = import_.codeProperty.empty()
                                      ? nullptr
                                      : surveyCodeOf(entity, import_.codeProperty);
        return text != nullptr ? parseFieldCode(*text, options_.controls).name : std::string{};
    }

    [[nodiscard]] Status finish(cmd::CommandContext& context, SurveyFinishReport& report)
    {
        const katana::entity::Model& model = document_.model();
        // What the import drew, and only that. The lists handed on below are
        // checked for being empty where they are made: to applySurveyCodes an
        // empty list is every entity, to processLinework every point.
        std::vector<EntityId> created;
        for (const EntityId id : points_->createdEntities()) {
            if (isPoint(model.entities.find(id))) {
                created.push_back(id);
            }
        }
        report.points = created.size();
        const bool noCodes = document_.surveyMap().empty();

        if (options_.codes) {
            if (created.empty()) {
                report.whyNotCoded = SurveyFinishSkip::NoPoints;
            } else if (noCodes) {
                report.whyNotCoded = SurveyFinishSkip::NoSurveyCodes;
            } else if (auto status = applyCodes(context, created, report); !status) {
                return status;
            }
        }

        if (options_.linework) {
            // A re-adjusted job's earlier points that still stand, then the
            // new ones: all of them are the job's to string.
            std::vector<EntityId> subject;
            for (const EntityId id : earlier_.points) {
                if (isPoint(model.entities.find(id))) {
                    subject.push_back(id);
                }
            }
            subject.insert(subject.end(), created.begin(), created.end());
            // ... and the lines its earlier run drew that are still there.
            std::vector<EntityId> standing;
            for (const EntityId id : earlier_.lines) {
                if (model.entities.contains(id)) {
                    standing.push_back(id);
                }
            }
            if (subject.empty()) {
                report.whyNotStrung = SurveyFinishSkip::NoPoints;
            } else if (noCodes) {
                // With no map nothing says which codes are lines, and a line
                // by control code alone is not the customisation at work.
                report.whyNotStrung = SurveyFinishSkip::NoSurveyCodes;
            } else if (auto status = drawLines(context, subject, standing, report); !status) {
                return status;
            }

            if (!standing.empty()) {
                if (report.whyNotStrung != SurveyFinishSkip::None) {
                    // A step with nothing to do leaves the drawing as it
                    // found it, and the report says those lines are the
                    // earlier run's. (None was redrawn: no line was planned.)
                    report.earlierLinesKept = standing.size();
                } else {
                    // What this run strings again it has redrawn in place;
                    // what is left is a line of a string it no longer makes.
                    const std::unordered_set<EntityId> redrawn(
                        report.earlierLinesRedrawn.begin(), report.earlierLinesRedrawn.end());
                    std::erase_if(standing, [&](EntityId id) { return redrawn.contains(id); });
                    if (!standing.empty()) {
                        if (auto status =
                                run(context, cmd::deleteEntities(standing), "earlier lines");
                            !status) {
                            return status;
                        }
                        report.earlierLinesRemoved = std::move(standing);
                    }
                }
            }
        }
        return {};
    }

    [[nodiscard]] Status applyCodes(cmd::CommandContext& context,
                                    const std::vector<EntityId>& created,
                                    SurveyFinishReport& report)
    {
        SurveyCodingOptions coding = options_.coding;
        coding.property = import_.codeProperty;
        coding.ids = created; // not empty: finish() checked
        SurveyCodingReport tally;
        auto planned = applySurveyCodes(document_, coding, &tally);
        if (!planned) {
            return failed(planned.error(), "codes");
        }
        if (tally.coded == 0 || tally.matched == 0) {
            // Nothing the customisation knows is in this file, so the plan is
            // dropped whole - also the fallback rule's attributes, which a
            // CODE run would attach - and the points stay exactly as the
            // import drew them. Only what was SEEN is reported: what the plan
            // would have created was not.
            report.whyNotCoded = tally.coded == 0 ? SurveyFinishSkip::NoCodesInFile
                                                  : SurveyFinishSkip::NoRuleMatches;
            report.coding.property = std::move(tally.property);
            report.coding.coded = tally.coded;
            report.coding.fallbackOnly = tally.fallbackOnly;
            report.coding.unmatchedCodes = std::move(tally.unmatchedCodes);
            report.coding.fallbackOnlyCodes = std::move(tally.fallbackOnlyCodes);
            return {};
        }
        report.whyNotCoded = SurveyFinishSkip::None;
        report.coding = std::move(tally);
        // nullptr: every point already has what its rule gives.
        return *planned != nullptr ? run(context, std::move(*planned), "codes") : Status{};
    }

    // `subject`: the points to string, none of them missing from the drawing
    // and the list not empty. `earlierLines`: a re-adjusted job's lines that
    // are still there; the ones a line planned here is drawn AS are listed in
    // the report, and the caller deletes the rest.
    [[nodiscard]] Status drawLines(cmd::CommandContext& context,
                                   const std::vector<EntityId>& subject,
                                   std::vector<EntityId> earlierLines,
                                   SurveyFinishReport& report)
    {
        const katana::entity::Model& model = document_.model();
        const auto redrawn = [&](const LineworkReport& planned) {
            for (const LineworkString& line : planned.strings) {
                if (line.redrawn != katana::entity::kInvalidEntityId) {
                    report.earlierLinesRedrawn.push_back(line.redrawn);
                    std::erase(earlierLines, line.redrawn);
                }
            }
        };

        // The points by the number the import wrote on each: which features
        // name them, and where each stands.
        std::unordered_map<std::string, const Entity*> byNumber;
        byNumber.reserve(subject.size());
        for (const EntityId id : subject) {
            const Entity& entity = *model.entities.find(id);
            if (std::string number = numberOf(entity); !number.empty()) {
                byNumber.try_emplace(std::move(number), &entity);
            }
        }

        // 1. The strings the file numbered itself: the features that have a
        //    name. One with none says only what its points' codes say - from
        //    some readers in pieces, a feature per run of consecutive shots -
        //    and is left to step 2. One that names no point of this import is
        //    another import's string, and is not drawn a second time by this
        //    one; an import that wrote no point number cannot tell, and takes
        //    them all.
        const std::vector<survey::SurveyFeature>& features = strings_.features;
        std::vector<bool> ours(features.size(), false);
        // Point id -> the code of each such feature that holds it, and those
        // codes on their own (views into `features`, which nothing below
        // changes).
        std::unordered_multimap<std::string_view, std::string_view> heldBy;
        std::unordered_set<std::string_view> heldCodes;
        bool any = false;
        bool codes = false; // something here carries a code
        bool rules = false; // ... and a rule more specific than "*" answers one
        for (std::size_t i = 0; i < features.size(); ++i) {
            const survey::SurveyFeature& feature = features[i];
            if (katana::core::trimmed(feature.name).empty()) {
                continue;
            }
            if (!import_.pointNumberProperty.empty() &&
                std::none_of(feature.pointIds.begin(), feature.pointIds.end(),
                             [&](const std::string& id) { return byNumber.contains(id); })) {
                continue;
            }
            ours[i] = true;
            any = true;
            if (!feature.code.empty()) {
                codes = true;
                heldCodes.insert(feature.code);
                for (const std::string& id : feature.pointIds) {
                    heldBy.emplace(id, feature.code);
                }
            }
        }
        if (any) {
            // A line runs through its points where they STAND: a point the
            // person has moved, and a re-adjustment keeps where they put it,
            // is still on its line. A point with no entity among these - the
            // drawing's control, one a policy skipped, one deleted by hand -
            // is where the file puts it.
            for (survey::SurveyPoint& point : strings_.points) {
                const auto found = byNumber.find(point.id);
                if (found == byNumber.end()) {
                    continue;
                }
                const Entity& entity = *found->second;
                const auto& at =
                    std::get<katana::entity::PointGeometry>(entity.geometry).position;
                point.easting = at.x;
                point.northing = at.y;
                point.elevation = katana::entity::heightsOf(entity.properties, 1).front();
            }

            // Planned now: the coding step has run, so the layers it made are
            // in the model and are not created twice.
            SurveyFeatureOptions options;
            options.import = import_;
            options.coding = options_.coding;
            options.onlyRuledLines = true;
            options.consider = std::move(ours);
            options.earlierLines = earlierLines;
            auto planned = drawSurveyFeatures(document_, strings_, options);
            if (!planned) {
                return failed(planned.error(), "features");
            }
            rules = !planned->report.strings.empty() ||
                    std::any_of(planned->unplaced.begin(), planned->unplaced.end(),
                                [](const UnplacedFeature& feature) {
                                    return feature.reason != UnplacedFeatureReason::NoRule;
                                });
            redrawn(planned->report);
            // A point the person deleted, and the re-adjustment keeps deleted,
            // has no entity to stand anywhere: the file's string still runs
            // through where the run puts it, and that is said, because a
            // string strung by code simply passes such a point by.
            if (!earlier_.deletedByHand.empty()) {
                const std::unordered_set<std::string_view> deleted(
                    earlier_.deletedByHand.begin(), earlier_.deletedByHand.end());
                for (const LineworkString& line : planned->report.strings) {
                    report.verticesAtDeletedPoints += static_cast<std::size_t>(
                        std::count_if(line.pointNumbers.begin(), line.pointNumbers.end(),
                                      [&](const std::string& id) { return deleted.contains(id); }));
                }
            }
            report.features = std::move(planned->report);
            report.unplacedFeatures = std::move(planned->unplaced);
            if (planned->command != nullptr) {
                if (auto status =
                        runLines(context, std::move(planned->command), "features", report);
                    !status) {
                    return status;
                }
            }
        }

        // 2. The other points, by their codes - planned after the features
        //    ran, for the same reason. A point a numbered string OF ITS OWN
        //    CODE holds is that string's, and step 1 has answered for it with
        //    a line or with the reason there is none: strung again by its
        //    code it would be in two lines of one string, or be refused twice
        //    for one reason. A point only a string of ANOTHER code holds is
        //    still a point of its own code's string.
        std::vector<EntityId> rest;
        for (const EntityId id : subject) {
            bool held = false;
            if (any) {
                const Entity& entity = *model.entities.find(id);
                const std::string number = numberOf(entity);
                const std::string code = codeOf(entity);
                if (number.empty()) {
                    // No number to find it by, so it cannot be told from the
                    // points the file strung under its code: it is left to
                    // them when the file has a numbered string of that code.
                    held = heldCodes.contains(code);
                } else {
                    const auto [first, last] = heldBy.equal_range(number);
                    held = std::any_of(first, last,
                                       [&](const auto& entry) { return entry.second == code; });
                }
            }
            if (held) {
                ++report.pointsInFeatures;
            } else {
                rest.push_back(id);
            }
        }
        if (!rest.empty()) { // never an empty list: it means every point in the drawing
            LineworkOptions linework;
            linework.property = import_.codeProperty;
            linework.pointNumberProperty = import_.pointNumberProperty;
            linework.order = options_.order;
            linework.ids = std::move(rest);
            linework.keepPoints = true; // an import never deletes a point
            linework.codes = options_.controls;
            linework.coding = options_.coding;
            linework.onlyRuledLines = true;
            linework.earlierLines = earlierLines; // those step 1 did not redraw
            auto planned = processLinework(document_, linework);
            if (!planned) {
                return failed(planned.error(), "linework");
            }
            std::size_t noCode = 0;
            std::size_t noRule = 0;
            for (const UnplacedPoint& point : planned->report.unplaced) {
                noCode += point.reason == UnplacedReason::NoCode ? 1 : 0;
                noRule += point.reason == UnplacedReason::NoRule ? 1 : 0;
            }
            codes = codes || planned->report.considered > noCode;
            rules = rules || planned->report.considered > noCode + noRule;
            redrawn(planned->report);
            report.strung = std::move(planned->report);
            if (planned->command != nullptr) {
                if (auto status =
                        runLines(context, std::move(planned->command), "linework", report);
                    !status) {
                    return status;
                }
            }
        }

        // Under onlyRuledLines nothing is a line without a rule, so "no code"
        // and "no rule" both mean nothing was drawn and nothing changed.
        report.whyNotStrung = !codes   ? SurveyFinishSkip::NoCodesInFile
                              : !rules ? SurveyFinishSkip::NoRuleMatches
                                       : SurveyFinishSkip::None;
        return {};
    }

    const Document& document_;
    cmd::CommandPtr points_;
    // What the steps are planned from; emptied once they have run.
    survey::SurveyProject strings_;
    SurveyImportOptions import_;
    SurveyFinishOptions options_;
    std::shared_ptr<SurveyFinishReport> report_;
    SurveyFinishEarlier earlier_;
    // The steps that ran, in order - coding, features, linework, the removal
    // of a job's earlier lines - each as it was planned, for undo and redo.
    std::vector<cmd::CommandPtr> steps_{};
};

void addDistinct(std::set<std::string>& into, const std::vector<std::string>& names)
{
    into.insert(names.begin(), names.end());
}

[[nodiscard]] std::string_view because(SurveyFinishSkip skip)
{
    switch (skip) {
    case SurveyFinishSkip::None:
    case SurveyFinishSkip::NotAsked:
        break;
    case SurveyFinishSkip::NoPoints:
        return "there are no points to string";
    case SurveyFinishSkip::NoSurveyCodes:
        return "no survey codes are loaded";
    case SurveyFinishSkip::NoCodesInFile:
        return "none of the points carries a code";
    case SurveyFinishSkip::NoRuleMatches:
        return "no rule matches any of their codes";
    }
    return "it was not asked for";
}

// "no rule for its code: 1; its code is a point code: 2": how many were left
// out for each reason, in the order the reasons are declared.
template <class Reason>
[[nodiscard]] std::string byReason(const std::map<Reason, std::size_t>& counts)
{
    std::string text;
    for (const auto& [reason, count] : counts) {
        text += (text.empty() ? "" : "; ") + std::string(toString(reason)) + ": " +
                std::to_string(count);
    }
    return text;
}

// One sentence of the report, and whether it is something a person may have
// to act on rather than an account of what was done (finishWarnings).
struct Sentence {
    std::string text;
    bool warning = false;
};

[[nodiscard]] std::vector<Sentence> sentencesOf(const SurveyFinishReport& report)
{
    std::vector<Sentence> sentences;
    const auto count = [](std::size_t n) { return std::to_string(n); };

    if (report.whyNotCoded == SurveyFinishSkip::None) {
        sentences.push_back(
            {"Survey codes were applied to the " + count(report.points) + " point(s) drawn: " +
                 count(report.coding.coded) + " carry a code and " + count(report.coding.matched) +
                 " of those have a rule; " + count(report.coding.layersCreated.size()) +
                 " layer(s) and " + count(report.coding.stylesCreated.size()) +
                 " style(s) were created.",
             false});
        if (const auto unknown = report.codesWithNoRule(); !unknown.empty()) {
            sentences.push_back({"No rule for the code(s): " + namedCodes(unknown) + ".", true});
        }
    } else if (report.whyNotCoded == SurveyFinishSkip::NoPoints) {
        // Also every re-adjustment that draws no new point: the points it
        // moved keep the codes they have. Nothing to act on.
        sentences.push_back({"No new points were drawn, so no survey codes were applied.", false});
    } else if (report.whyNotCoded != SurveyFinishSkip::NotAsked) {
        std::string text =
            "Survey codes were not applied: " + std::string(because(report.whyNotCoded));
        if (const auto unknown = report.codesWithNoRule(); !unknown.empty()) {
            text += " (" + namedCodes(unknown) + ")";
        }
        sentences.push_back({text + ".", true});
    }

    if (report.whyNotStrung == SurveyFinishSkip::None) {
        std::string text = "Linework: " + count(report.lines.size()) + " line(s) drawn";
        if (!report.earlierLinesRedrawn.empty()) {
            text += " and " + count(report.earlierLinesRedrawn.size()) +
                    " of the job's line(s) redrawn where this adjustment puts them";
        }
        sentences.push_back({text + ".", false});
        if (!report.earlierLinesRemoved.empty()) {
            sentences.push_back({count(report.earlierLinesRemoved.size()) +
                                     " of the job's line(s) are no longer strung by this "
                                     "adjustment and were deleted, with anything attached to "
                                     "them.",
                                 true});
        }
        if (report.verticesAtDeletedPoints != 0) {
            sentences.push_back({count(report.verticesAtDeletedPoints) +
                                     " vertex(es) of the job's lines are at a point deleted "
                                     "from the drawing by hand: a string the file numbered "
                                     "still runs through where this adjustment puts it.",
                                 true});
        }
        if (!report.unplacedFeatures.empty()) {
            std::map<UnplacedFeatureReason, std::size_t> reasons;
            bool fault = false;
            for (const UnplacedFeature& feature : report.unplacedFeatures) {
                ++reasons[feature.reason];
                // A string of a point code is in no line because it is none.
                fault = fault || feature.reason != UnplacedFeatureReason::PointCode;
            }
            sentences.push_back({count(report.unplacedFeatures.size()) +
                                     " string(s) of the file are in no line (" +
                                     byReason(reasons) + ").",
                                 fault});
        }
        if (!report.strung.unplaced.empty()) {
            std::map<UnplacedReason, std::size_t> reasons;
            bool fault = false;
            for (const UnplacedPoint& point : report.strung.unplaced) {
                ++reasons[point.reason];
                // Likewise a point of a point code, and one with no code.
                fault = fault || (point.reason != UnplacedReason::PointCode &&
                                  point.reason != UnplacedReason::NoCode);
            }
            sentences.push_back({count(report.strung.unplaced.size()) +
                                     " point(s) are in no line (" + byReason(reasons) + ").",
                                 fault});
        }
    } else if (report.whyNotStrung != SurveyFinishSkip::NotAsked) {
        std::string text =
            "No linework was drawn: " + std::string(because(report.whyNotStrung)) + ".";
        if (report.earlierLinesKept != 0) {
            text += " The job's " + count(report.earlierLinesKept) +
                    " line(s) are as the earlier adjustment drew them.";
        }
        // No points to string is nothing to act on - unless lines were left
        // standing that this adjustment no longer accounts for.
        sentences.push_back({std::move(text), report.whyNotStrung != SurveyFinishSkip::NoPoints ||
                                                  report.earlierLinesKept != 0});
    }
    return sentences;
}

} // namespace

std::string_view toString(SurveyFinishSkip skip)
{
    switch (skip) {
    case SurveyFinishSkip::None:
        return "ran";
    case SurveyFinishSkip::NotAsked:
        return "not-asked";
    case SurveyFinishSkip::NoPoints:
        return "no-points";
    case SurveyFinishSkip::NoSurveyCodes:
        return "no-survey-codes";
    case SurveyFinishSkip::NoCodesInFile:
        return "no-codes-in-file";
    case SurveyFinishSkip::NoRuleMatches:
        return "no-rule-matches";
    }
    return "not-asked";
}

std::vector<std::string> SurveyFinishReport::codesWithNoRule() const
{
    std::set<std::string> codes;
    addDistinct(codes, coding.unmatchedCodes);
    addDistinct(codes, coding.fallbackOnlyCodes);
    return {codes.begin(), codes.end()};
}

std::vector<std::string> SurveyFinishReport::layersCreated() const
{
    std::set<std::string> layers;
    addDistinct(layers, coding.layersCreated);
    for (const LineworkReport* step : {&features, &strung}) {
        addDistinct(layers, step->layersCreated);
        if (step->styling != nullptr) {
            addDistinct(layers, step->styling->layersCreated);
        }
    }
    return {layers.begin(), layers.end()};
}

std::vector<std::string> SurveyFinishReport::stylesCreated() const
{
    std::set<std::string> styles;
    addDistinct(styles, coding.stylesCreated);
    for (const LineworkReport* step : {&features, &strung}) {
        if (step->styling != nullptr) {
            addDistinct(styles, step->styling->stylesCreated);
        }
    }
    return {styles.begin(), styles.end()};
}

std::vector<std::string> describe(const SurveyFinishReport& report)
{
    std::vector<std::string> sentences;
    for (Sentence& sentence : sentencesOf(report)) {
        sentences.push_back(std::move(sentence.text));
    }
    return sentences;
}

std::vector<std::string> finishWarnings(const SurveyFinishReport& report)
{
    std::vector<std::string> warnings;
    for (Sentence& sentence : sentencesOf(report)) {
        if (sentence.warning) {
            warnings.push_back(std::move(sentence.text));
        }
    }
    return warnings;
}

void nameSurveyStrings(std::vector<survey::SurveyPoint>& points,
                       const std::vector<survey::SurveyFeature>& features)
{
    if (features.empty()) {
        return;
    }
    // Views into the points' own ids: nothing below adds or removes a point.
    std::unordered_map<std::string_view, survey::SurveyPoint*> byId;
    byId.reserve(points.size());
    for (survey::SurveyPoint& point : points) {
        byId.emplace(point.id, &point);
    }
    for (const survey::SurveyFeature& feature : features) {
        // Blanks alone are no name, as the finish reads a feature and as the
        // number is read back off the point (surveyStringOf).
        if (katana::core::trimmed(feature.name).empty() || feature.code.empty()) {
            continue;
        }
        for (const std::string& id : feature.pointIds) {
            const auto found = byId.find(id);
            if (found == byId.end()) {
                continue; // not drawn: an unpositioned point, or the drawing's control
            }
            survey::SurveyPoint& point = *found->second;
            // Of its own code, read as the point is coded: by its first token.
            if (parseFieldCode(point.code, LineworkCodes{}).name != feature.code) {
                continue;
            }
            point.metadata.try_emplace(std::string(kSurveyStringProperty), feature.name);
        }
    }
}

cmd::CommandPtr withSurveyFinish(const Document& document, cmd::CommandPtr points,
                                 survey::SurveyProject strings, const SurveyImportOptions& import,
                                 SurveyFinishOptions options,
                                 std::shared_ptr<SurveyFinishReport> report,
                                 SurveyFinishEarlier earlier)
{
    if (points == nullptr || !options.any()) {
        if (report != nullptr) {
            *report = {};
            if (points == nullptr) {
                const auto why = [](bool asked) {
                    return asked ? SurveyFinishSkip::NoPoints : SurveyFinishSkip::NotAsked;
                };
                report->whyNotCoded = why(options.codes);
                report->whyNotStrung = why(options.linework);
            }
        }
        return points;
    }
    return std::make_unique<SurveyFinishCommand>(document, std::move(points), std::move(strings),
                                                 import, std::move(options), std::move(report),
                                                 std::move(earlier));
}

} // namespace katana::cad
