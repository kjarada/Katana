#include "katana/cad/linework.hpp"

#include <algorithm>
#include <cmath>
#include <map>
#include <set>
#include <utility>

#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace {

namespace cmd = katana::commands;
namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::PropertyValue;
using katana::geometry::Point2;
using katana::geometry::Vec2;

// ---- reading the points ----------------------------------------------------------

// The code a point carries, or nothing. Text only, as applySurveyCodes reads
// it: a number in the code property is a measurement named badly, not a code.
[[nodiscard]] const std::string* codeOf(const Entity& entity, const std::string& property)
{
    const auto found = entity.properties.find(property);
    if (found == entity.properties.end()) {
        return nullptr;
    }
    const auto* text = std::get_if<std::string>(&found->second);
    return text != nullptr && !text->empty() ? text : nullptr;
}

// The point number as text: survey import writes text ("102", "A12"), but a
// drawing from elsewhere may hold an integer, and both name the same point.
[[nodiscard]] std::string pointNumberOf(const Entity& entity, const std::string& property)
{
    const auto found = entity.properties.find(property);
    if (found == entity.properties.end()) {
        return {};
    }
    if (const auto* text = std::get_if<std::string>(&found->second)) {
        return std::string(katana::core::trimmed(*text));
    }
    if (std::holds_alternative<bool>(found->second)) {
        return {}; // a flag is not a number
    }
    return katana::entity::toString(found->second);
}

// How a point number sorts: numbers by value, so "9" comes before "10", and
// before every number that is not one, which sort by their text.
struct NumberKey {
    bool numeric = false;
    double value = 0.0;
    std::string text{};

    friend bool operator<(const NumberKey& a, const NumberKey& b)
    {
        if (a.numeric != b.numeric) {
            return a.numeric;
        }
        return a.numeric ? a.value < b.value : a.text < b.text;
    }
};

[[nodiscard]] NumberKey numberKey(const std::string& number)
{
    NumberKey key;
    key.text = number;
    if (const auto value = katana::core::parseFiniteDouble(number)) {
        key.numeric = true;
        key.value = *value;
    }
    return key;
}

// A point that is going into a line.
struct Candidate {
    EntityId id = katana::entity::kInvalidEntityId;
    Point2 at{};
    std::optional<double> height{};
    std::string number{};
    FieldCode code{};
    std::string layer{};
};

// A stretch of one string between its start and its end.
struct Run {
    std::vector<const Candidate*> points{};
    bool closed = false;
    bool rectangle = false;
};

// The polyline a run becomes, before it is an entity.
struct Shape {
    std::vector<Point2> vertices{};
    std::vector<std::optional<double>> heights{};
    bool closed = false;
    bool rectangle = false;
    std::size_t curves = 0;

    void add(const Point2& at, std::optional<double> height)
    {
        vertices.push_back(at);
        heights.push_back(height);
    }
};

// ---- curves ----------------------------------------------------------------------

// Appends the arc of `arc`'s circle from `from` to `to`, travelling in
// `arc`'s direction, as chords no further than `tolerance` from it. `from` is
// already in the shape; `to` is appended exactly, so a surveyed point stays a
// vertex where it was measured.
//
// The chord of angle t on radius r stands off the arc by its sagitta,
// r(1 - cos(t/2)); keeping that within the tolerance gives the largest step
// t = 2 acos(1 - tolerance / r). Heights between two surveyed heights are
// interpolated along the arc, which is what a kerb or a channel between two
// shots is assumed to do; with either end unsurveyed the chords have none.
void appendArc(Shape& shape, const katana::geometry::Arc2& arc, const Candidate& from,
               const Candidate& to, double tolerance)
{
    const double startAngle = (from.at - arc.center).angle();
    const double endAngle = (to.at - arc.center).angle();
    const double delta = arc.sweep >= 0.0 ? katana::math::normalizeAngle(endAngle - startAngle)
                                          : -katana::math::normalizeAngle(startAngle - endAngle);
    const double ratio = std::max(-1.0, 1.0 - tolerance / arc.radius);
    const double step = 2.0 * std::acos(ratio);
    const auto chords =
        static_cast<std::size_t>(std::max(1.0, std::ceil(std::abs(delta) / step)));
    for (std::size_t i = 1; i < chords; ++i) {
        const double t = static_cast<double>(i) / static_cast<double>(chords);
        std::optional<double> height{};
        if (from.height && to.height) {
            height = katana::math::lerp(*from.height, *to.height, t);
        }
        shape.add(arc.pointAtAngle(startAngle + delta * t), height);
    }
    shape.add(to.at, to.height);
}

struct NoteSink {
    std::vector<LineworkNote>& notes;

    void operator()(const Candidate& at, LineworkNoteKind kind, std::string detail = {}) const
    {
        notes.push_back(LineworkNote{at.id, at.number, kind, std::move(detail)});
    }
};

// Appends the curve through points[first..last] (both in the shape's run;
// points[first] already added).
void appendCurve(Shape& shape, const std::vector<const Candidate*>& points, std::size_t first,
                 std::size_t last, double tolerance, const NoteSink& note)
{
    const std::size_t count = last - first; // segments in the curve
    if (count < 2) {
        // Two points are on every circle through them: there is no curve to
        // draw, only the straight line the surveyor may not have meant.
        note(*points[first], LineworkNoteKind::CurveTooShort);
        shape.add(points[last]->at, points[last]->height);
        return;
    }
    ++shape.curves;
    // Each consecutive three points define an arc; a segment left over at the
    // end lies on the arc through the last three.
    auto piece = [&](std::size_t a, std::size_t b, std::size_t c, std::size_t segmentFrom) {
        const auto arc =
            katana::geometry::Arc2::throughPoints(points[a]->at, points[b]->at, points[c]->at);
        for (std::size_t i = segmentFrom; i < c; ++i) {
            if (!arc) {
                shape.add(points[i + 1]->at, points[i + 1]->height);
            } else {
                appendArc(shape, *arc, *points[i], *points[i + 1], tolerance);
            }
        }
        if (!arc) {
            note(*points[a], LineworkNoteKind::CurveCollinear);
        }
    };
    std::size_t k = first;
    while (k < last) {
        if (k + 2 <= last) {
            piece(k, k + 1, k + 2, k);
            k += 2;
        } else {
            piece(last - 2, last - 1, last, last - 1);
            k = last;
        }
    }
}

// The polyline for one run, with its curves, its closure or its rectangle.
[[nodiscard]] Shape shapeOf(const Run& run, double tolerance, const NoteSink& note)
{
    Shape shape;
    const auto& points = run.points;
    shape.closed = run.closed;

    if (run.rectangle) {
        if (points.size() == 3) {
            // Side p0-p1, reaching p2's side of it by p2's distance from it.
            const Vec2 side = points[1]->at - points[0]->at;
            const double length = side.length();
            const Vec2 normal =
                length > tol::kGeometric ? side.perpendicular() / length : Vec2(0.0, 0.0);
            const double width = normal.dot(points[2]->at - points[0]->at);
            if (std::abs(width) > tol::kGeometric) {
                shape.rectangle = true;
                shape.closed = true;
                shape.add(points[0]->at, points[0]->height);
                shape.add(points[1]->at, points[1]->height);
                // Constructed, not surveyed: no height rather than a guess.
                shape.add(points[1]->at + normal * width, std::nullopt);
                shape.add(points[0]->at + normal * width, std::nullopt);
                return shape;
            }
            // The third point is on the first side: a line, not an area.
            note(*points.back(), LineworkNoteKind::RectangleShape, "no width");
            shape.closed = false;
        } else {
            note(*points.back(), LineworkNoteKind::RectangleShape,
                 std::to_string(points.size()) + " points");
            shape.closed = points.size() >= 3;
        }
    }
    if (shape.closed && points.size() < 3) {
        // Two points closed back on themselves enclose nothing.
        note(*points.back(), LineworkNoteKind::CloseTooShort);
        shape.closed = false;
    }

    // Where the curves are: from a point carrying arcStart to the next
    // carrying arcEnd. A point carrying both ends one and begins the next.
    std::vector<std::pair<std::size_t, std::size_t>> curves;
    std::optional<std::size_t> open;
    for (std::size_t i = 0; i < points.size(); ++i) {
        const FieldCode& code = points[i]->code;
        if (code.has(LineworkControl::ArcEnd)) {
            if (open) {
                curves.emplace_back(*open, i);
                open.reset();
            } else {
                note(*points[i], LineworkNoteKind::CurveEndWithoutStart);
            }
        }
        if (code.has(LineworkControl::ArcStart)) {
            if (open) { // a new curve begins where the last one ends
                curves.emplace_back(*open, i);
            }
            open = i;
        }
    }
    if (open) {
        note(*points[*open], LineworkNoteKind::CurveUnterminated);
        if (*open + 1 < points.size()) {
            curves.emplace_back(*open, points.size() - 1);
        }
    }

    shape.add(points.front()->at, points.front()->height);
    std::size_t i = 0;
    auto curve = curves.begin();
    while (i + 1 < points.size()) {
        if (curve != curves.end() && curve->first == i) {
            appendCurve(shape, points, curve->first, curve->second, tolerance, note);
            i = curve->second;
            ++curve;
            continue;
        }
        shape.add(points[i + 1]->at, points[i + 1]->height);
        ++i;
    }
    return shape;
}

// Splits one string's points, in order, into runs at its start, end, close
// and rectangle codes.
[[nodiscard]] std::vector<Run> runsOf(const std::vector<Candidate>& ordered)
{
    std::vector<Run> runs;
    std::optional<Run> current;
    auto finish = [&] {
        if (current) {
            runs.push_back(std::move(*current));
            current.reset();
        }
    };
    for (const Candidate& point : ordered) {
        if (point.code.has(LineworkControl::Start)) {
            finish();
        }
        if (!current) {
            current.emplace();
        }
        current->points.push_back(&point);
        if (point.code.has(LineworkControl::Rectangle)) {
            current->rectangle = true;
            finish();
        } else if (point.code.has(LineworkControl::Close)) {
            current->closed = true;
            finish();
        } else if (point.code.has(LineworkControl::End)) {
            finish();
        }
    }
    finish();
    return runs;
}

// ---- styling, once the lines exist -----------------------------------------------

// Styles the lines another part of the same transaction has just created, by
// planning applySurveyCodes against the document at that moment - the only
// moment the lines exist to be planned for. Redo replays what was planned, so
// the ids and styles come back exactly.
class StyleLinesCommand final : public cmd::Command {
  public:
    StyleLinesCommand(const Document& document, const cmd::Command& lines,
                      SurveyCodingOptions options, std::shared_ptr<SurveyCodingReport> report)
        : document_(document), lines_(lines), options_(std::move(options)),
          report_(std::move(report))
    {
    }

    [[nodiscard]] std::string_view name() const override { return "STYLE_LINEWORK"; }

    [[nodiscard]] Status validate(const cmd::CommandContext& context) const override
    {
        // It plans against its Document; run on another model it would style
        // whatever that document's entities of the same ids are.
        if (&context.model != &document_.model()) {
            return makeError(ErrorCode::InvalidState,
                             "linework was planned for another document");
        }
        return {};
    }

    [[nodiscard]] Status execute(cmd::CommandContext& context) override
    {
        options_.ids = lines_.createdEntities();
        // An empty id list means EVERY entity to applySurveyCodes; with no
        // lines there is nothing to style, not everything.
        if (options_.ids.empty()) {
            return {};
        }
        auto planned = applySurveyCodes(document_, options_, report_.get());
        if (!planned) {
            return planned.error();
        }
        inner_ = std::move(*planned);
        if (!inner_) {
            return {};
        }
        if (auto status = inner_->validate(context); !status) {
            return status;
        }
        return inner_->execute(context);
    }

    [[nodiscard]] Status undo(cmd::CommandContext& context) override
    {
        return inner_ ? inner_->undo(context) : Status{};
    }

    [[nodiscard]] Status redo(cmd::CommandContext& context) override
    {
        return inner_ ? inner_->redo(context) : Status{};
    }

  private:
    const Document& document_;
    const cmd::Command& lines_; // owned by the same transaction, executed before this
    SurveyCodingOptions options_;
    std::shared_ptr<SurveyCodingReport> report_;
    cmd::CommandPtr inner_{};
};

// The code property to read when none is named: the first candidate any of
// these points carries, as applySurveyCodes chooses among all entities.
[[nodiscard]] std::string findCodeProperty(const std::vector<const Entity*>& points)
{
    for (const std::string& candidate : codePropertyCandidates()) {
        for (const Entity* point : points) {
            if (codeOf(*point, candidate) != nullptr) {
                return candidate;
            }
        }
    }
    return codePropertyCandidates().front();
}

} // namespace

// ---- string names ------------------------------------------------------------------

StringName splitStringName(const katana::entity::SurveyMap& map, std::string_view name)
{
    StringName split;
    const auto rules = map.match(name);
    if (rules.empty()) {
        return split;
    }
    split.matched = true;
    const std::string& key = rules.front()->key; // the most specific
    if (key == "*") {
        split.key = std::string(name);
        split.fallbackOnly = true;
        return split;
    }
    split.key = key;
    if (key.back() == '*') {
        split.number = std::string(name.substr(key.size() - 1));
    }
    return split;
}

// ---- control codes --------------------------------------------------------------

namespace {

[[nodiscard]] std::vector<std::pair<LineworkControl, const std::string*>>
spellings(const LineworkCodes& codes)
{
    return {{LineworkControl::Start, &codes.start},       {LineworkControl::End, &codes.end},
            {LineworkControl::Close, &codes.close},       {LineworkControl::ArcStart, &codes.arcStart},
            {LineworkControl::ArcEnd, &codes.arcEnd},     {LineworkControl::Join, &codes.join},
            {LineworkControl::Rectangle, &codes.rectangle}};
}

} // namespace

Status validate(const LineworkCodes& codes)
{
    const auto all = spellings(codes);
    for (std::size_t i = 0; i < all.size(); ++i) {
        const std::string& spelling = *all[i].second;
        if (std::any_of(spelling.begin(), spelling.end(), katana::core::isAsciiSpace)) {
            return makeError(ErrorCode::InvalidArgument,
                             "a linework control code cannot contain a blank: codes are split "
                             "on blanks, so it could never be matched",
                             std::string(toString(all[i].first)) + "=\"" + spelling + "\"");
        }
        for (std::size_t j = i + 1; j < all.size() && !spelling.empty(); ++j) {
            if (katana::core::equalsIgnoringCase(spelling, *all[j].second)) {
                return makeError(ErrorCode::InvalidArgument,
                                 "two linework controls are spelled alike, so the token would "
                                 "mean both",
                                 std::string(toString(all[i].first)) + " and " +
                                     std::string(toString(all[j].first)) + " are \"" +
                                     spelling + "\"");
            }
        }
    }
    return {};
}

std::string_view toString(LineworkControl control)
{
    switch (control) {
    case LineworkControl::Start:
        return "start";
    case LineworkControl::End:
        return "end";
    case LineworkControl::Close:
        return "close";
    case LineworkControl::ArcStart:
        return "arcStart";
    case LineworkControl::ArcEnd:
        return "arcEnd";
    case LineworkControl::Join:
        return "join";
    case LineworkControl::Rectangle:
        return "rectangle";
    }
    return "unknown";
}

bool FieldCode::has(LineworkControl control) const
{
    return std::find(controls.begin(), controls.end(), control) != controls.end();
}

FieldCode parseFieldCode(std::string_view code, const LineworkCodes& codes)
{
    std::vector<std::string_view> tokens;
    std::size_t at = 0;
    while (at < code.size()) {
        while (at < code.size() && katana::core::isAsciiSpace(code[at])) {
            ++at;
        }
        const std::size_t begin = at;
        while (at < code.size() && !katana::core::isAsciiSpace(code[at])) {
            ++at;
        }
        if (at > begin) {
            tokens.push_back(code.substr(begin, at - begin));
        }
    }

    FieldCode parsed;
    if (tokens.empty()) {
        return parsed;
    }
    parsed.name = std::string(tokens.front());
    const auto all = spellings(codes);
    for (std::size_t i = 1; i < tokens.size(); ++i) {
        const auto control =
            std::find_if(all.begin(), all.end(), [&](const auto& entry) {
                return !entry.second->empty() &&
                       katana::core::equalsIgnoringCase(tokens[i], *entry.second);
            });
        if (control == all.end()) {
            parsed.unknownTokens.emplace_back(tokens[i]);
            continue;
        }
        parsed.controls.push_back(control->first);
        if (control->first == LineworkControl::Join) {
            // The next token is the point to join to, unless it is itself a
            // control - "JPN CL" is a join with its number forgotten, not a
            // join to a point called CL.
            const bool next = i + 1 < tokens.size() &&
                              std::none_of(all.begin(), all.end(), [&](const auto& entry) {
                                  return !entry.second->empty() &&
                                         katana::core::equalsIgnoringCase(tokens[i + 1],
                                                                          *entry.second);
                              });
            if (next) {
                parsed.joinTo = std::string(tokens[++i]);
            } else {
                parsed.joinWithoutTarget = true;
            }
        }
    }
    return parsed;
}

// ---- report words ----------------------------------------------------------------

std::string_view toString(UnplacedReason reason)
{
    switch (reason) {
    case UnplacedReason::NoCode:
        return "no code";
    case UnplacedReason::NoRule:
        return "no rule for its code and no control code";
    case UnplacedReason::PointCode:
        return "its code is a point code";
    case UnplacedReason::NoPointNumber:
        return "no point number to order it by";
    case UnplacedReason::LonePoint:
        return "the only point of its string";
    case UnplacedReason::Coincident:
        return "every point of its string is in one place";
    }
    return "unknown";
}

std::string_view toString(LineworkNoteKind kind)
{
    switch (kind) {
    case LineworkNoteKind::UnknownToken:
        return "unknown token";
    case LineworkNoteKind::JoinWithoutTarget:
        return "join with no point number";
    case LineworkNoteKind::JoinTargetMissing:
        return "join target not found";
    case LineworkNoteKind::CurveTooShort:
        return "curve with no point between its ends, drawn straight";
    case LineworkNoteKind::CurveCollinear:
        return "curve points in a straight line, drawn straight";
    case LineworkNoteKind::CurveUnterminated:
        return "curve never ended, curved to the end of the string";
    case LineworkNoteKind::CurveEndWithoutStart:
        return "curve end with no curve begun";
    case LineworkNoteKind::CloseTooShort:
        return "closing a string of two points, drawn open";
    case LineworkNoteKind::RectangleShape:
        return "a rectangle needs three points with width, drawn as a line";
    case LineworkNoteKind::DuplicatePointNumber:
        return "two points of one string share a number";
    case LineworkNoteKind::NoRuleForName:
        return "a line by control code alone: no rule styles it";
    case LineworkNoteKind::FallbackOnlyName:
        return "a line only because the fallback rule \"*\" says so";
    }
    return "unknown";
}

// ---- processing ------------------------------------------------------------------

katana::core::Result<LineworkResult> processLinework(const Document& document,
                                                     const LineworkOptions& options)
{
    if (auto status = validate(options.codes); !status) {
        return status.error();
    }
    if (!(std::isfinite(options.chordTolerance) && options.chordTolerance > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the chord tolerance must be a positive distance",
                         "chordTolerance=" + std::to_string(options.chordTolerance));
    }

    const katana::entity::Model& model = document.model();
    const katana::entity::SurveyMap& map = document.surveyMap();
    LineworkResult result;
    LineworkReport& report = result.report;

    // In entity order whatever order they were given in: that IS the entity
    // order a string may be joined in, and it makes the report deterministic.
    std::vector<EntityId> subject = options.ids;
    if (subject.empty()) {
        subject = model.entities.ids();
    }
    std::sort(subject.begin(), subject.end());
    subject.erase(std::unique(subject.begin(), subject.end()), subject.end());

    std::vector<const Entity*> points;
    for (const EntityId id : subject) {
        const Entity* entity = model.entities.find(id);
        if (entity == nullptr ||
            !std::holds_alternative<katana::entity::PointGeometry>(entity->geometry)) {
            ++report.notPoints;
            continue;
        }
        points.push_back(entity);
    }
    const std::string property =
        options.property.empty() ? findCodeProperty(points) : options.property;
    report.property = property;
    NoteSink note{report.notes};

    // Resolved once per name rather than once per point: a survey of 30,000
    // points has a few hundred names, and lookup walks every rule.
    std::map<std::string, katana::entity::SurveyMatch> resolved;
    auto resolve = [&](const std::string& name) -> const katana::entity::SurveyMatch& {
        auto found = resolved.find(name);
        if (found == resolved.end()) {
            found = resolved.emplace(name, map.lookup(name)).first;
        }
        return found->second;
    };

    std::map<std::string, std::vector<Candidate>> strings; // by name
    for (const Entity* entity : points) {
        ++report.considered;
        Candidate candidate;
        candidate.id = entity->id;
        candidate.at = std::get<katana::entity::PointGeometry>(entity->geometry).position;
        candidate.height = katana::entity::heightsOf(entity->properties, 1).front();
        candidate.number = pointNumberOf(*entity, options.pointNumberProperty);
        candidate.layer = entity->layer;

        const std::string* code = codeOf(*entity, property);
        candidate.code = code != nullptr ? parseFieldCode(*code, options.codes) : FieldCode{};
        const std::string codeText = code != nullptr ? *code : std::string{};
        auto unplace = [&](UnplacedReason reason) {
            report.unplaced.push_back(UnplacedPoint{entity->id, candidate.number, codeText, reason});
        };
        if (candidate.code.name.empty()) {
            unplace(UnplacedReason::NoCode);
            continue;
        }
        for (const std::string& token : candidate.code.unknownTokens) {
            note(candidate, LineworkNoteKind::UnknownToken, token);
        }
        if (candidate.code.joinWithoutTarget) {
            note(candidate, LineworkNoteKind::JoinWithoutTarget);
        }

        const katana::entity::SurveyMatch& match = resolve(candidate.code.name);
        const bool line = match.resolved.breakline == katana::entity::SurveyBreakline::Line ||
                          !candidate.code.controls.empty();
        if (!line) {
            unplace(match.empty() ? UnplacedReason::NoRule : UnplacedReason::PointCode);
            continue;
        }
        if (options.order == LineworkOrder::PointNumber && candidate.number.empty()) {
            unplace(UnplacedReason::NoPointNumber);
            continue;
        }
        strings[candidate.code.name].push_back(std::move(candidate));
    }

    // Every point in the drawing by number, for joins: a join may reach a
    // point outside the selection, because the code says so.
    std::map<std::string, std::vector<const Entity*>> byNumber;
    const bool anyJoin = std::any_of(strings.begin(), strings.end(), [](const auto& entry) {
        return std::any_of(entry.second.begin(), entry.second.end(),
                           [](const Candidate& c) { return !c.code.joinTo.empty(); });
    });
    if (anyJoin) {
        model.entities.forEach([&](const Entity& entity) {
            if (std::holds_alternative<katana::entity::PointGeometry>(entity.geometry)) {
                const std::string number = pointNumberOf(entity, options.pointNumberProperty);
                if (!number.empty()) {
                    byNumber[number].push_back(&entity);
                }
            }
        });
    }

    std::vector<Entity> lines;
    std::set<EntityId> placed;
    std::set<std::string> layersNeeded;
    // Points whose runs made no line, with why. A later placing wins: a lone
    // point that a join reached IS in a line.
    std::map<EntityId, UnplacedPoint> lonely;

    for (auto& [name, candidates] : strings) {
        if (options.order == LineworkOrder::PointNumber) {
            // Stable, so equal numbers keep entity order.
            std::stable_sort(candidates.begin(), candidates.end(),
                             [](const Candidate& a, const Candidate& b) {
                                 return numberKey(a.number) < numberKey(b.number);
                             });
            for (std::size_t i = 1; i < candidates.size(); ++i) {
                if (candidates[i].number == candidates[i - 1].number) {
                    note(candidates[i], LineworkNoteKind::DuplicatePointNumber,
                         candidates[i].number);
                }
            }
        }

        const StringName split = splitStringName(map, name);
        if (!split.matched) {
            note(candidates.front(), LineworkNoteKind::NoRuleForName, name);
        } else if (split.fallbackOnly) {
            note(candidates.front(), LineworkNoteKind::FallbackOnlyName, name);
        }

        // 12d's model is Katana's layer; with none, the line stays with its
        // points.
        std::string layer = resolve(name).resolved.model;
        if (!layer.empty()) {
            if (auto status = katana::entity::validateLayerPath(layer); !status) {
                return makeError(ErrorCode::InvalidArgument,
                                 "the survey map gives a code a model that is not a valid "
                                 "layer name",
                                 "code=" + name + " model=" + layer + " " +
                                     status.error().context);
            }
            if (!model.layers.contains(layer)) {
                if (options.coding.createLayers) {
                    layersNeeded.insert(layer);
                } else {
                    layer.clear();
                }
            }
        }
        if (layer.empty()) {
            layer = candidates.front().layer;
        }

        auto emit = [&](const Shape& shape, const std::vector<const Candidate*>& members,
                        bool join) {
            Entity entity;
            entity.geometry = katana::geometry::Polyline2{shape.vertices, shape.closed};
            entity.layer = layer;
            // The line carries the name, so applySurveyCodes - now, and any
            // time it is applied again - codes it as its points were coded.
            entity.properties.insert_or_assign(property, PropertyValue(name));
            katana::entity::setHeights(entity.properties, shape.heights);
            lines.push_back(std::move(entity));

            LineworkString built;
            built.name = name;
            built.key = split.key;
            built.number = split.number;
            built.closed = shape.closed;
            built.rectangle = shape.rectangle;
            built.join = join;
            built.curves = shape.curves;
            built.vertices = shape.vertices.size();
            built.layer = layer;
            for (const Candidate* member : members) {
                built.points.push_back(member->id);
                built.pointNumbers.push_back(member->number);
                placed.insert(member->id);
                lonely.erase(member->id);
            }
            report.strings.push_back(std::move(built));
        };

        for (const Run& run : runsOf(candidates)) {
            if (run.points.size() < 2) {
                const Candidate& only = *run.points.front();
                if (!placed.contains(only.id)) {
                    lonely[only.id] =
                        UnplacedPoint{only.id, only.number, "", UnplacedReason::LonePoint};
                }
                continue;
            }
            const Shape shape = shapeOf(run, options.chordTolerance, note);
            const katana::geometry::Polyline2 polyline{shape.vertices, shape.closed};
            if (!(polyline.length() > tol::kGeometric)) {
                for (const Candidate* member : run.points) {
                    if (!placed.contains(member->id)) {
                        lonely[member->id] = UnplacedPoint{member->id, member->number, "",
                                                           UnplacedReason::Coincident};
                    }
                }
                continue;
            }
            emit(shape, run.points, false);
        }

        for (const Candidate& from : candidates) {
            if (from.code.joinTo.empty()) {
                continue;
            }
            const auto targets = byNumber.find(from.code.joinTo);
            if (targets == byNumber.end() || targets->second.size() != 1 ||
                targets->second.front()->id == from.id) {
                note(from, LineworkNoteKind::JoinTargetMissing, from.code.joinTo);
                continue;
            }
            const Entity& target = *targets->second.front();
            Candidate to;
            to.id = target.id;
            to.at = std::get<katana::entity::PointGeometry>(target.geometry).position;
            to.height = katana::entity::heightsOf(target.properties, 1).front();
            to.number = from.code.joinTo;
            if (!(from.at.distanceTo(to.at) > tol::kGeometric)) {
                note(from, LineworkNoteKind::JoinTargetMissing,
                     from.code.joinTo + " is in the same place");
                continue;
            }
            Shape shape;
            shape.add(from.at, from.height);
            shape.add(to.at, to.height);
            emit(shape, {&from, &to}, true);
        }
    }

    // The code each lonely point carried, for the report.
    for (auto& [id, entry] : lonely) {
        const Entity* entity = model.entities.find(id);
        const std::string* code = entity != nullptr ? codeOf(*entity, property) : nullptr;
        entry.code = code != nullptr ? *code : std::string{};
        report.unplaced.push_back(std::move(entry));
    }
    // A point code that a join reached is at the end of a line after all.
    std::erase_if(report.unplaced,
                  [&](const UnplacedPoint& point) { return placed.contains(point.id); });
    std::stable_sort(report.unplaced.begin(), report.unplaced.end(),
                     [](const UnplacedPoint& a, const UnplacedPoint& b) { return a.id < b.id; });
    report.layersCreated.assign(layersNeeded.begin(), layersNeeded.end());

    if (lines.empty()) {
        return result; // nothing to build, and no error
    }

    auto transaction = std::make_unique<cmd::Transaction>("PROCESS_LINEWORK");
    for (const std::string& name : report.layersCreated) {
        katana::entity::Layer layer;
        layer.name = name;
        transaction->add(cmd::createLayer(std::move(layer)));
    }
    cmd::CommandPtr create = cmd::createEntities(std::move(lines));
    const cmd::Command& created = *create;
    transaction->add(std::move(create));

    auto styling = std::make_shared<SurveyCodingReport>();
    report.styling = styling;
    SurveyCodingOptions coding = options.coding;
    coding.property = property;
    transaction->add(std::make_unique<StyleLinesCommand>(document, created, std::move(coding),
                                                         std::move(styling)));

    if (!options.keepPoints) {
        // Only what was asked about: a join may reach a point outside the
        // selection, and that point is not this command's to delete.
        std::vector<EntityId> remove;
        for (const EntityId id : placed) {
            if (std::binary_search(subject.begin(), subject.end(), id)) {
                remove.push_back(id);
            }
        }
        report.pointsRemoved = remove.size();
        if (!remove.empty()) {
            transaction->add(cmd::deleteEntities(std::move(remove)));
        }
    }

    result.command = std::move(transaction);
    return result;
}

} // namespace katana::cad
