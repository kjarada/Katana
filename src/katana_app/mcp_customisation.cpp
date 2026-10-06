// katana_customisation (docs/mcp.md, "The customisation"): the session's
// customisation - its definitions, its survey code rules, what is wrong with
// them - as structured data for an agent.
//
// Every other tool is a shape over a command line. This one reads the
// Document instead, as katana_status does, for two reasons. The verbs that
// list a customisation (CUSTOMISE, CODE LIST, CODE CHECK) reply in text made
// for a person, and an agent that edits a rule needs the rule, not a line
// about the code it belongs to. And a read that went through a line would sit
// in the command history of a session it never changed.
//
// Nothing here has words of its own for what a customisation holds. A rule
// and a definition are written by the Katana customisation format's own
// writer (entity/customisation.hpp) and read back as JSON, so a member here
// is spelt exactly as a file spells it; the lint and the summary are cad's
// (code_table.hpp, customisation_report.hpp). It changes nothing: an edit is
// CUSTOMISE through katana_run_commands - export, edit the file, load it
// back.
//
// TWO members are the tool's own and no file's: a rule's `index` and a
// definition's `kind`. A file says the first by the rule's place in "codes"
// and the second by the list the definition is in, and its reader, which is
// strict, refuses both as members it does not know. So an entry read here
// goes into a file with that one member taken off - which the tool's
// description says, since an agent that copied a rule across whole was
// refused for it.

#include "mcp_tools.hpp"

#include <cstddef>
#include <limits>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/code_table.hpp"
#include "katana/cad/colour_lookup.hpp"
#include "katana/cad/customisation_report.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"
#include "katana/entity/tables.hpp"

namespace katana::app::mcp {

namespace {

// How many entries a reply holds when the call does not say, and the most it
// may ask for. A customisation of a survey authority's size has some 1,600
// rules and 800 definitions; a hundred is a screen of them, and a thousand
// keeps one reply to a size a client still reads.
constexpr std::size_t kDefaultLimit = 100;
constexpr std::size_t kLargestLimit = 1000;

struct Page {
    std::size_t offset = 0;
    std::size_t limit = kDefaultLimit;

    // Whether the entry that is number `match` among those the filter took
    // (counted from 0) is on this page.
    [[nodiscard]] bool holds(std::size_t match) const
    {
        return match >= offset && match - offset < limit;
    }
};

// A whole-number argument from `low` to `high`, or `fallback` when it is not
// given.
std::size_t wholeNumber(const Json& arguments, const char* name, std::size_t fallback,
                        long long low, long long high)
{
    const Json& value = argument(arguments, name);
    if (value.is_null()) {
        return fallback;
    }
    if (!value.is_number_integer() || value.get<long long>() < low ||
        value.get<long long>() > high) {
        throw ToolRefusal{std::string("\"") + name + "\" is a whole number from " +
                          std::to_string(low) +
                          (high == std::numeric_limits<long long>::max()
                               ? std::string()
                               : " to " + std::to_string(high))};
    }
    return static_cast<std::size_t>(value.get<long long>());
}

// An optional text argument; empty when it is not given.
std::string textArgument(const Json& arguments, const char* name)
{
    const Json& value = argument(arguments, name);
    if (value.is_null()) {
        return {};
    }
    if (!value.is_string()) {
        throw ToolRefusal{std::string("\"") + name + "\" must be a string"};
    }
    return value.get<std::string>();
}

// An argument that says nothing for the part asked for is refused, as one
// the tool does not declare is (mcp_server.cpp): ignored, a call that
// filtered the summary, or named a definition while asking for the codes,
// would answer something other than what was asked and say nothing of it.
void refuseArgument(const Json& arguments, const char* name, const std::string& why)
{
    if (!argument(arguments, name).is_null()) {
        throw ToolRefusal{std::string("\"") + name + "\" " + why};
    }
}

// Whether `text` holds `needle`, which is already in lower case. Searching
// folds case; storing never does (CODE LIST's filter keeps the same rule).
bool holds(std::string_view text, const std::string& needle)
{
    return needle.empty() || katana::core::lowered(text).find(needle) != std::string::npos;
}

// The reply every paged part gives: how many entries the filter took, the
// page asked for, and that page under the part's own name.
Json pageOf(const char* part, std::size_t total, const Page& page, Json entries)
{
    return Json{{"part", part},
                {"total", total},
                {"offset", page.offset},
                {"limit", page.limit},
                {part, std::move(entries)}};
}

// ---- summary --------------------------------------------------------------------------------

Json summaryOf(const katana::cad::Document& document)
{
    // cad's report, whole, and NOTHING beside it - not the lists' envelope,
    // not even `part`: CUSTOMISE JSON, katana://customisation and this are
    // then one object for one report, and no member cad adds later can meet
    // one of the tool's own under the same name. (A caller knows which part
    // it asked for.) What went wrong when the session started is in it, as
    // `start`: over this protocol there is nowhere else to read that.
    return Json::parse(katana::cad::customisationJson(document));
}

// ---- codes ----------------------------------------------------------------------------------

Json codesOf(const katana::cad::Document& document, const std::string& filter, const Page& page)
{
    const std::vector<katana::entity::SurveyRule>& rules = document.surveyMap().rules();
    // The rules alone, written as a customisation file writes them: the
    // "codes" list of that text is one object a rule, in map order, with the
    // members the format has and no others. The name is the file's own and is
    // not part of the reply.
    katana::entity::Customisation onlyRules;
    onlyRules.name = "codes";
    onlyRules.map = document.surveyMap();
    katana::entity::CustomisationWriteOptions options;
    options.linestyles = false;
    options.symbols = false;
    const auto text = katana::entity::customisationToJson(onlyRules, options);
    if (!text) {
        // What the writer refuses it could not read back - text that is not
        // UTF-8 in a rule made in a session, say. It names the rule.
        throw ToolRefusal{"the survey code rules cannot be written as a customisation: " +
                          text.error().describe()};
    }
    Json file = Json::parse(*text);
    // A file with no rules has no "codes" member at all.
    Json written = file.contains("codes") ? std::move(file["codes"]) : Json::array();
    if (written.size() != rules.size()) {
        throw ToolRefusal{"the survey code rules were not all written: " +
                          std::to_string(written.size()) + " of " + std::to_string(rules.size())};
    }

    const std::string needle = katana::core::lowered(filter);
    Json entries = Json::array();
    std::size_t matches = 0;
    for (std::size_t index = 0; index < rules.size(); ++index) {
        const katana::entity::SurveyRule& rule = rules[index];
        if (!holds(rule.key, needle) && !holds(rule.comment, needle) &&
            !holds(rule.model, needle)) {
            continue;
        }
        if (page.holds(matches)) {
            Json entry = std::move(written[index]);
            // The only identity a rule has: its place in the map, which is
            // what CODE EXPLAIN and CODE CHECK cite it by ("rule #57").
            entry["index"] = index;
            entries.push_back(std::move(entry));
        }
        ++matches;
    }
    return pageOf("codes", matches, page, std::move(entries));
}

// ---- definitions ----------------------------------------------------------------------------

// One definition: what a list shows of it, and with `whole` everything a
// customisation file holds for it, its strokes included.
Json definitionOf(const katana::entity::LineStyle& definition, bool whole)
{
    const auto text = katana::entity::definitionToJson(definition);
    if (!text) {
        throw ToolRefusal{"the definition cannot be written as a customisation's: " +
                          text.error().describe()};
    }
    Json written = Json::parse(*text);
    // The format leaves a member at its default out; a list says each of
    // these for every definition, so that a reader need not know the
    // defaults. "world" is the one the format gives `units` left out
    // (docs/customisation.md, "The members"); the others are the model's.
    Json entry{{"name", definition.name},
               {"kind", definition.symbol ? "symbol" : "linestyle"},
               {"group", definition.group},
               {"units", written.value("units", "world")},
               {"atVertices", definition.atVertices},
               {"from", definition.source}};
    if (!whole) {
        return entry;
    }
    if (!written.contains("strokes")) {
        // A definition with no strokes has no such member in a file; asked
        // for by name, it says that it has none.
        written["strokes"] = Json::array();
    }
    written.update(entry);
    return written;
}

Json definitionsOf(const katana::cad::Document& document, const std::string& filter,
                   const std::string& name, const Page& page)
{
    const katana::entity::StyleLibrary& library = document.styleLibrary();
    if (!name.empty()) {
        // Matched as written, as a rule's linestyle or symbol names it.
        const katana::entity::LineStyle* definition = library.find(name);
        if (definition == nullptr) {
            throw ToolRefusal{"no definition is called \"" + name +
                              "\"; names are matched as written, letter case included (part "
                              "definitions with a filter lists them)"};
        }
        Json entries = Json::array();
        entries.push_back(definitionOf(*definition, true));
        return pageOf("definitions", 1, Page{0, 1}, std::move(entries));
    }
    const std::string needle = katana::core::lowered(filter);
    Json entries = Json::array();
    std::size_t matches = 0;
    library.forEach([&](const katana::entity::LineStyle& definition) {
        if (!holds(definition.name, needle) && !holds(definition.group, needle)) {
            return;
        }
        if (page.holds(matches)) {
            entries.push_back(definitionOf(definition, false));
        }
        ++matches;
    });
    return pageOf("definitions", matches, page, std::move(entries));
}

// ---- problems -------------------------------------------------------------------------------

Json problemsOf(const katana::cad::Document& document, const std::string& filter,
                const Page& page)
{
    // The lint CODE CHECK prints, an object an issue.
    const std::vector<katana::cad::LintIssue> issues = katana::cad::lintSurveyMap(
        document.surveyMap(), document.styleLibrary(), katana::cad::colourLookup(document),
        [](std::string_view symbol) { return katana::entity::isBuiltInSymbolName(symbol); });
    const std::string needle = katana::core::lowered(filter);
    Json entries = Json::array();
    std::size_t matches = 0;
    std::size_t cannotApply = 0;
    for (const katana::cad::LintIssue& issue : issues) {
        cannotApply += issue.severity == katana::cad::LintSeverity::Error ? 1 : 0;
        const char* kind = katana::cad::toString(issue.kind);
        if (!holds(issue.key, needle) && !holds(kind, needle) && !holds(issue.message, needle)) {
            continue;
        }
        if (page.holds(matches)) {
            // `index` is the rule's, the `index` part codes gives it.
            entries.push_back(Json{{"index", issue.rule},
                                   {"key", issue.key},
                                   {"sets", katana::entity::toString(issue.section)},
                                   {"severity", katana::cad::toString(issue.severity)},
                                   {"kind", kind},
                                   {"message", issue.message}});
        }
        ++matches;
    }
    Json reply = pageOf("problems", matches, page, std::move(entries));
    // Of the whole lint, whatever the filter took: the three numbers the
    // summary's "problems" gives, by the same names.
    reply["rules"] = document.surveyMap().size();
    reply["cannotApply"] = cannotApply;
    reply["warnings"] = issues.size() - cannotApply;
    return reply;
}

ToolReply customisation(Session& session, const Json& arguments)
{
    const katana::cad::Document& document = session.document();
    const std::string part = requiredString(arguments, "part");
    Json structured;
    if (part == "summary") {
        for (const char* name : {"filter", "name", "offset", "limit"}) {
            refuseArgument(arguments, name, "does not go with part \"summary\", which is one "
                                            "object and not a list");
        }
        structured = summaryOf(document);
    } else if (part == "codes" || part == "definitions" || part == "problems") {
        const std::string name = textArgument(arguments, "name");
        if (part != "definitions") {
            refuseArgument(arguments, "name", "picks one definition: it goes with part "
                                              "\"definitions\"");
        } else if (!argument(arguments, "name").is_null()) {
            if (name.empty()) {
                throw ToolRefusal{"\"name\" must be a non-empty string"};
            }
            for (const char* other : {"filter", "offset", "limit"}) {
                refuseArgument(arguments, other, "does not go with \"name\", which picks "
                                                 "exactly one definition");
            }
        }
        const std::string filter = textArgument(arguments, "filter");
        Page page;
        page.offset = wholeNumber(arguments, "offset", 0, 0, std::numeric_limits<long long>::max());
        page.limit = wholeNumber(arguments, "limit", kDefaultLimit, 1,
                                 static_cast<long long>(kLargestLimit));
        structured = part == "codes"         ? codesOf(document, filter, page)
                     : part == "definitions" ? definitionsOf(document, filter, name, page)
                                             : problemsOf(document, filter, page);
    } else {
        throw ToolRefusal{"\"part\" is summary, codes, definitions or problems"};
    }
    // The text is the same object, for a client older than structured
    // content: it has every fact, as katana_status' text has. A name that is
    // not UTF-8 is shown with U+FFFD rather than thrown.
    return ToolReply{structured.dump(2, ' ', false, Json::error_handler_t::replace), structured};
}

} // namespace

Tool customisationTool()
{
    return Tool{
        "katana_customisation", "The customisation",
        "The session's customisation as structured data: the linestyle and symbol definitions, "
        "the survey code rules, the colours and the settings that turn a surveyor's field codes "
        "into a drawing. part chooses what: summary - what is loaded (name, origin, each source "
        "and its notice, counts, linework codes, automation, colours, what is wrong with the "
        "rules, what this drawing uses of it, and start: what went wrong when this session "
        "started, such as a kept customisation that did not read); it is ONE object, exactly "
        "what CUSTOMISE JSON prints and the katana://customisation resource holds, with no "
        "part, total, offset or limit beside its members; "
        "codes - one object a survey code rule: its index (its place, which is its precedence "
        "and what CODE EXPLAIN and CODE CHECK cite it by), key, sets and the members a Katana "
        "customisation file holds for it; definitions - one object a linestyle or symbol: name, "
        "kind, group, units, atVertices, from - and with name, that one definition whole, its "
        "strokes included; problems - the rules' lint: index, key, sets, severity, kind, "
        "message. codes, definitions and problems are lists, each {part, total, offset, limit, "
        "<part>: [...]}: total is how many entries filter "
        "took, and offset and limit (100 unless said, at most 1000) choose the page. It reads "
        "only. To CHANGE a customisation use katana_run_commands: CUSTOMISE EXPORT <file> "
        "writes the session as one Katana customisation file (JSON), which you edit, and "
        "CUSTOMISE <file> merges a file in - a definition replaces the one of its name, a "
        "key's rules in a section replace that key's there, so a file holding only what "
        "changes is an edit - or CUSTOMISE REPLACE <file> loads it in the place of what is "
        "loaded; CUSTOMISE REMOVE deletes, CUSTOMISE SET sets the switches and linework codes "
        "(HELP CUSTOMISE). A rule, or a definition read whole by name, goes into such a file "
        "once the member that is this tool's own is taken off it - index from a rule, kind "
        "from a definition: a file says a rule's place by its order in codes and a "
        "definition's kind by the list it is in, linestyles or symbols, and refuses a member "
        "it does not know. An entry of the definitions LIST is not the definition: it has no "
        "strokes.",
        objectSchema(
            Json{{"part",
                  {{"type", "string"},
                   {"enum", {"summary", "codes", "definitions", "problems"}},
                   {"description", "summary: one object, what is loaded and what went wrong at "
                                   "the start (no part or total beside it). codes: the survey "
                                   "code rules. definitions: the linestyles and symbols. "
                                   "problems: what the lint finds in the rules."}}},
                 {"filter",
                  {{"type", "string"},
                   {"description",
                    "Only the entries holding this text, letter case ignored: in a rule's key, "
                    "comment or layer (codes); a definition's name or group (definitions); an "
                    "issue's key, kind or message (problems)."}}},
                 {"name",
                  {{"type", "string"},
                   {"description",
                    "With part definitions: exactly one definition, by its name as written, "
                    "with everything a customisation file holds for it - its strokes too."}}},
                 {"offset",
                  {{"type", "integer"},
                   {"minimum", 0},
                   {"description", "How many matching entries to skip (default 0)."}}},
                 {"limit",
                  {{"type", "integer"},
                   {"minimum", 1},
                   {"maximum", kLargestLimit},
                   {"description", "The most entries to return (default 100)."}}}},
            {"part"}),
        hints(true, false, true), customisation};
}

} // namespace katana::app::mcp
