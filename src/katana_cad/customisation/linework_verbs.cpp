// LINEWORK on the text command line, and what the drawing's customisation
// asks of a survey import (include/katana/cad/linework_verbs.hpp).
//
// In customisation/ beside linework.cpp, whose processLinework it runs, and
// because that directory is globbed into katana_cad.

#include "katana/cad/linework_verbs.hpp"

#include <algorithm>
#include <map>
#include <unordered_set>
#include <utility>

#include "katana/cad/colour_lookup.hpp"
#include "katana/cad/linework.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/entity.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
using Words = std::vector<std::string>;

namespace {

constexpr const char* kUsage =
    "LINEWORK [<scope>] [WHERE k=v ...] [ORDER number|entity] [PREVIEW]";

bool is(const std::string& word, std::string_view name)
{
    return katana::core::equalsIgnoringCase(word, name);
}

katana::core::Error notAWord(const std::string& word, const std::string& why)
{
    return makeError(ErrorCode::ParseFailure, why + "; " + kUsage, word);
}

// What the line asks for, read and not yet looked up.
struct LineworkLine {
    ScopeWords scope{};
    // No scope word was given: the selection when there is one, else the
    // drawing.
    bool scopeNamed = false;
    LineworkOrder order = LineworkOrder::PointNumber;
    bool preview = false;
};

Result<LineworkLine> parse(const Words& args)
{
    LineworkLine line;
    std::size_t at = 0;
    // WHERE alone names no scope: it filters whatever the default is.
    line.scopeNamed = !args.empty() && isScopeWord(args.front()) && !is(args.front(), "WHERE");
    auto scope = parseScopeWords(args, at);
    if (!scope) {
        return scope.error();
    }
    line.scope = std::move(scope).value();

    bool ordered = false;
    for (; at < args.size(); ++at) {
        const std::string& word = args[at];
        if (is(word, "PREVIEW")) {
            line.preview = true;
            continue;
        }
        if (is(word, "ORDER")) {
            if (ordered) {
                return notAWord(word, "ORDER is given twice");
            }
            if (at + 1 >= args.size()) {
                return notAWord(word, "ORDER needs number or entity after it");
            }
            const std::string& how = args[++at];
            if (is(how, "number")) {
                line.order = LineworkOrder::PointNumber;
            } else if (is(how, "entity")) {
                line.order = LineworkOrder::EntityOrder;
            } else {
                return notAWord(how, "ORDER takes number (by point number) or entity (the "
                                     "order the points were drawn in)");
            }
            ordered = true;
            continue;
        }
        // A scope after the verb's own words would be read by nobody: the
        // shared parser has finished, and guessing that it was meant is how a
        // line strings the whole drawing where it was to string one layer.
        if (isScopeWord(word)) {
            return notAWord(word, "the scope and its WHERE come first");
        }
        return notAWord(word, "not a LINEWORK word");
    }
    return line;
}

bool isPoint(const Entity* entity)
{
    return entity != nullptr &&
           std::holds_alternative<katana::entity::PointGeometry>(entity->geometry);
}

bool isLine(const Entity* entity)
{
    return entity != nullptr &&
           std::holds_alternative<katana::geometry::Polyline2>(entity->geometry);
}

// The string name `entity` carries under `property`: the first token of its
// code - how a point's code is read everywhere, its control codes left out -
// followed by its string number. Empty with no code. A line drawn by
// linework carries its points' code and string number, so a line and the
// points it was strung through have one name.
std::string stringNameOf(const Entity& entity, const std::string& property)
{
    const std::string* code = surveyCodeOf(entity, property);
    if (code == nullptr) {
        return {};
    }
    std::string name = parseFieldCode(*code, LineworkCodes{}).name;
    if (!name.empty()) {
        name += surveyStringOf(entity);
    }
    return name;
}

// The points of `subject` that the survey job which drew them has strung
// already (linework_verbs.hpp, at the top, has the rule and the reason).
Result<std::unordered_set<EntityId>> strungByTheirJob(const Document& document,
                                                      const std::vector<EntityId>& subject)
{
    std::unordered_set<EntityId> strung;
    if (subject.empty() || document.surveyJobs().empty()) {
        return strung;
    }
    const std::unordered_set<EntityId> asked(subject.begin(), subject.end());
    const auto& entities = document.model().entities;
    for (const SurveyJob& job : document.surveyJobs()) {
        // A job that owns nothing the scope took is not read at all: its
        // options, damaged or written by a newer Katana, are then no reason
        // to refuse a run that does not touch it.
        if (std::none_of(job.createdEntities.begin(), job.createdEntities.end(),
                         [&](EntityId id) { return asked.contains(id); })) {
            continue;
        }
        const auto stored = readSurveyJobOptions(job.importOptions, job.id);
        if (!stored) {
            return makeError(stored.error().code, stored.error().message,
                             "LINEWORK cannot tell whether survey job " + job.id +
                                 " strung its own points" +
                                 (stored.error().context.empty()
                                      ? std::string{}
                                      : "; " + stored.error().context));
        }
        if (!stored->drawLinework) {
            continue;
        }
        const std::string& property = stored->import.codeProperty.empty()
                                          ? codePropertyCandidates().front()
                                          : stored->import.codeProperty;
        std::unordered_set<std::string> drawn;
        for (const EntityId id : job.createdEntities) {
            const Entity* line = entities.find(id);
            if (isLine(line)) {
                if (std::string name = stringNameOf(*line, property); !name.empty()) {
                    drawn.insert(std::move(name));
                }
            }
        }
        if (drawn.empty()) {
            continue;
        }
        for (const EntityId id : job.createdEntities) {
            if (!asked.contains(id)) {
                continue;
            }
            const Entity* point = entities.find(id);
            if (isPoint(point) && drawn.contains(stringNameOf(*point, property))) {
                strung.insert(id);
            }
        }
    }
    return strung;
}

std::string count(std::size_t n)
{
    return std::to_string(n);
}

} // namespace

bool isLineworkVerb(std::string_view verb)
{
    return katana::core::equalsIgnoringCase(verb, "LINEWORK");
}

Result<std::string> runLineworkVerb(Document& document, const std::vector<std::string>& tokens,
                                    const ScopeViewProvider& views)
{
    if (tokens.empty() || !isLineworkVerb(tokens.front())) {
        return makeError(ErrorCode::ParseFailure, "not a LINEWORK command; type HELP LINEWORK",
                         tokens.empty() ? std::string{} : tokens.front());
    }
    auto line = parse(Words(tokens.begin() + 1, tokens.end()));
    if (!line) {
        return line.error();
    }
    if (!line->scopeNamed) {
        line->scope.source =
            document.selection().empty() ? ScopeSource::Drawing : ScopeSource::Selection;
    }
    const auto match = matchScope(document, line->scope, views);
    if (!match) {
        return match.error();
    }

    const auto strung = strungByTheirJob(document, match->matched);
    if (!strung) {
        return strung.error();
    }
    LineworkOptions options;
    options.order = line->order;
    options.keepPoints = true; // the verb never removes a point
    // The verb is run again and again over the same points - after a rule is
    // added, after more are surveyed - and a line it finds drawn is not drawn
    // on top of itself (linework_verbs.hpp, "A LINE THE DRAWING ALREADY HOLDS").
    options.skipLinesAlreadyDrawn = true;
    options.codes = document.customisationState().linework;
    options.coding.colourOf = colourLookup(document);
    for (const EntityId id : match->matched) {
        if (!strung->contains(id)) {
            options.ids.push_back(id);
        }
    }

    // Never with an empty list: to processLinework that is every point in the
    // drawing, and here it is a scope that took nothing, or only points their
    // job has strung.
    LineworkResult planned;
    if (!options.ids.empty()) {
        auto result = processLinework(document, options);
        if (!result) {
            return result.error();
        }
        planned = std::move(result).value();
    }
    const LineworkReport& report = planned.report;

    std::vector<EntityId> created;
    if (!line->preview && planned.command != nullptr) {
        if (auto status = document.execute(std::move(planned.command)); !status) {
            return status.error();
        }
        created = document.lastCreatedEntities();
    }

    std::string reply = "linework " + scopeRecord(*match) +
                        " considered=" + count(report.considered) +
                        " lines=" + count(report.strings.size()) +
                        " unplaced=" + count(report.unplaced.size()) +
                        " notes=" + count(report.notes.size());
    if (line->preview) {
        reply += " preview=yes";
    }
    if (!strung->empty()) {
        reply += "\nleft_out=" + count(strung->size()) + " reason=strung-by-their-job";
    }
    if (!report.alreadyDrawn.empty()) {
        // Points, as the record above counts: those the lines not drawn again
        // run through, each once - a join and a run may share one.
        std::unordered_set<EntityId> through;
        for (const LineworkString& drawn : report.alreadyDrawn) {
            through.insert(drawn.points.begin(), drawn.points.end());
        }
        reply += "\nleft_out=" + count(through.size()) + " reason=already-drawn lines=" +
                 count(report.alreadyDrawn.size());
    }
    // The lines are created in the order the report lists them, so the n-th
    // created entity is the n-th string; said only where the two lists are
    // the same length, which they are whenever every line is a new entity.
    const bool withIds = created.size() == report.strings.size();
    for (std::size_t i = 0; i < report.strings.size() && i < kLineworkStringsListed; ++i) {
        const LineworkString& string = report.strings[i];
        reply += "\nstring name=" + recordValue(string.name) + " key=" + recordValue(string.key) +
                 " number=" + recordValue(string.number) +
                 " points=" + count(string.pointNumbers.size()) +
                 " vertices=" + count(string.vertices) +
                 " closed=" + (string.closed ? "yes" : "no") +
                 " layer=" + recordValue(string.layer);
        if (string.join) {
            reply += " join=yes";
        }
        if (withIds) {
            reply += " entity=" + count(created[i]);
        }
    }
    if (report.strings.size() > kLineworkStringsListed) {
        reply += "\nstrings_more=" + count(report.strings.size() - kLineworkStringsListed);
    }
    // In the order the reasons and the kinds are declared, so two runs over
    // the same drawing answer alike.
    std::map<UnplacedReason, std::size_t> reasons;
    for (const UnplacedPoint& point : report.unplaced) {
        ++reasons[point.reason];
    }
    for (const auto& [reason, points] : reasons) {
        reply += "\nunplaced reason=" + recordValue(toString(reason)) + " points=" + count(points);
    }
    std::map<LineworkNoteKind, std::size_t> kinds;
    for (const LineworkNote& note : report.notes) {
        ++kinds[note.kind];
    }
    for (const auto& [kind, notes] : kinds) {
        reply += "\nnote kind=" + recordValue(toString(kind)) + " count=" + count(notes);
    }
    return reply;
}

std::string lineworkVerbHelp()
{
    return R"(LINEWORK [<scope>] [WHERE k=v ...] [ORDER number|entity] [PREVIEW]

Joins coded survey points into lines (docs/survey_coding.md, "Linework"): the points of
one string name - a point's code, its control codes left out, followed by its string
number - in order, where a survey code rule makes that code a line. The lines go on
their rule's layer and wear its style, with the customisation's colours. No point is
removed. The lines drawn are ONE undo step.

  <scope>   SELECTION | DRAWING | VIEW [id] [EXTENTS] | AREA x0,y0,x1,y1 | LAYERS a,b
            [ONLY], then [WHERE key=value ...] (HELP: "Scope"). It comes first. With no
            scope word: the selection when anything is selected, else the whole drawing.
  ORDER     number: by point number, 9 before 10 (the default; a point with none is not
            placed).  entity: the order the points were drawn in, which for an import is
            the order the file listed them.
  PREVIEW   plans and reports; nothing is drawn.

Control codes after the code: ST start, END end, CL close, BC and EC a curve, JPN <n> a
join to point n, RECT a rectangle - or as the drawing's customisation spells them.
A code that NO rule makes a line is strung only through the points that carry a
control code themselves: of "KB ST", "KB", "KB CL" with no rule for KB, the first and
the third are joined and the second is in no line, which the reply says.

Two things are left out, and counted:
  - points their survey job has already strung: a job imported with linework on drew
    and owns its lines, and a second line through the same points would lie on top of
    them;
  - a line the drawing already holds - the same code, string number, vertices and
    heights - so LINEWORK run again draws nothing twice. A string that has gained a
    point, or whose point was moved, is another line: it is drawn, and the line that
    was there stays until it is erased.
SURVEY IMPORT ... LINEWORK off imports a job that strings nothing, and LINEWORK then
strings its points BY THEIR CODES, not as the file numbered and closed its strings: a
string the file closed and began again under one number comes out as one open line.

The reply is key=value records:
  linework scope=... matched=<n> considered=<n> lines=<n> unplaced=<n> notes=<n>
            matched: what the scope took; considered: the points among it looked at;
            lines: those drawn. preview=yes after them for a PREVIEW
  left_out=<n> reason=strung-by-their-job      n points left to their job's lines
  left_out=<n> reason=already-drawn lines=<m>  m lines the drawing already holds, and
            the n points they run through (each record only when it has something)
  string name=<n> key=<k> number=<n> points=<n> vertices=<n> closed=yes|no layer=<l>
            [join=yes] entity=<id>      each line, the first 50, then strings_more=<n>
  unplaced reason="<why>" points=<n>           each reason a point is in no line
  note kind="<what>" count=<n>                 each kind of note
)";
}

SurveyImportFinish surveyImportFinish(const Document& document, std::optional<bool> codes,
                                      std::optional<bool> linework)
{
    const katana::entity::CustomisationAutomation& automation =
        document.customisationState().automation;
    SurveyImportFinish finish;
    finish.codesAsked = codes.value_or(automation.codesOnSurveyImport);
    finish.lineworkAsked = linework.value_or(automation.lineworkOnSurveyImport);
    finish.surveyCodesLoaded = !document.surveyMap().empty();
    finish.options.codes = finish.codesAsked && finish.surveyCodesLoaded;
    finish.options.linework = finish.lineworkAsked && finish.surveyCodesLoaded;
    finish.options.coding.colourOf = colourLookup(document);
    finish.options.controls = document.customisationState().linework;
    finish.options.order = LineworkOrder::PointNumber;
    return finish;
}

} // namespace katana::cad
