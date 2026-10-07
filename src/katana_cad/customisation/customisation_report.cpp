#include "katana/cad/customisation_report.hpp"

#include <algorithm>
#include <map>
#include <sstream>
#include <string>

#include <nlohmann/json.hpp>

#include "katana/cad/code_table.hpp"
#include "katana/cad/colour_lookup.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/tables.hpp"

namespace katana::cad {

namespace {

// Library definitions only: the pickers also offer the model's linetypes and
// the built-in shapes, which no customisation brought. This line once said
// "157 of them symbols", counting `mode vertex` alone - one of D3's four
// reasons, and a minority of the symbols the reference survey code files use.
std::size_t fromLibrary(const std::vector<CatalogueEntry>& entries)
{
    return static_cast<std::size_t>(
        std::ranges::count(entries, DefinitionSource::Library, &CatalogueEntry::source));
}

std::string counted(std::size_t count, const char* one, const char* many)
{
    return std::to_string(count) + " " + (count == 1 ? one : many);
}

// Nothing to count: no definition and no rule. (A session may still hold a
// name, colours and settings; it is the definitions and the rules that draw.)
bool nothingLoaded(const CustomisationSummary& summary)
{
    return summary.definitions == 0 && summary.rules == 0;
}

// The two lines the reply begins with when something is loaded.
void writeCounts(std::ostream& out, const CustomisationSummary& summary)
{
    out << counted(summary.definitions, "linestyle and symbol definition",
                   "linestyle and symbol definitions")
        << " in " << counted(summary.groups, "group", "groups") << ": " << summary.symbols
        << " offered as symbols, " << summary.linestyles
        << " as linestyles (one definition can be both)\n"
        << counted(summary.rules, "survey code rule", "survey code rules") << " over "
        << counted(summary.codes, "distinct code", "distinct codes") << "\n";
}

const char* yesNo(bool value) { return value ? "yes" : "no"; }
const char* onOff(bool value) { return value ? "on" : "off"; }

} // namespace

CustomisationSummary customisationSummary(const Document& document,
                                          const std::vector<CustomisationSource>& loaded,
                                          const std::vector<std::string>& missingAtOpen)
{
    CustomisationSummary summary;
    const katana::entity::StyleLibrary& library = document.styleLibrary();
    summary.definitions = library.size();
    summary.groups = katana::entity::styleGroups(library).size();
    summary.symbols = symbolDefinitionCount(document);
    summary.linestyles = fromLibrary(linetypeChoices(document, false));
    summary.rules = document.surveyMap().size();
    summary.codes = document.surveyMap().keys().size();
    summary.loaded = loaded;
    summary.notLoaded = missingAtOpen;
    summary.coverage = customisationCoverage(document);
    return summary;
}

// ---- the verb's reply -------------------------------------------------------------------

CustomisationSummary customisationSummary(const Document& document)
{
    const CustomisationState& state = document.customisationState();
    CustomisationSummary summary =
        customisationSummary(document, state.sources, state.missingAtOpen);
    summary.name = state.name;
    summary.origin = state.origin;
    summary.kept = state.kept;
    summary.colours = state.colours.size();
    summary.automation = state.automation;
    summary.linework = state.linework;
    return summary;
}

std::string customisationStateRecord(const CustomisationSummary& summary)
{
    // The counts again, although the two lines above them say them in words:
    // a script reads this line and should not have to read a sentence.
    return "customisation name=" + recordValue(summary.name) + " origin=" +
           toString(summary.origin) + " kept=" + yesNo(summary.kept) +
           " definitions=" + std::to_string(summary.definitions) +
           " codes=" + std::to_string(summary.codes) +
           " rules=" + std::to_string(summary.rules) +
           " colours=" + std::to_string(summary.colours);
}

std::string formatCustomisationReply(const CustomisationSummary& summary)
{
    std::ostringstream out;
    const bool nothing = nothingLoaded(summary);
    if (nothing) {
        // A session may hold a customisation that brought neither kind - a
        // name, a table of colours, the settings. "No customisation is
        // loaded." above the record that names it and gives its origin
        // contradicted the next line; what is true of it is that nothing in
        // it draws.
        out << (summary.origin == CustomisationOrigin::None
                    ? "No customisation is loaded.\n"
                    : "No linestyle or symbol definitions and no survey code rules are "
                      "loaded.\n")
            << "  CUSTOMISE <file> [<file>...]  loads Katana customisation files\n";
    } else {
        writeCounts(out, summary);
    }
    out << customisationStateRecord(summary) << "\n";
    for (const CustomisationSource& source : summary.loaded) {
        out << "source name=" << recordValue(source.name) << " definitions="
            << yesNo(source.definitions) << " rules=" << yesNo(source.rules) << "\n";
    }
    // The keys CUSTOMISE SET takes, so that either record can be typed back
    // after it. A control switched off has no spelling, written "".
    out << "automation auto.codes=" << onOff(summary.automation.codesOnSurveyImport)
        << " auto.linework=" << onOff(summary.automation.lineworkOnSurveyImport) << "\n";
    out << "linework";
    for (const katana::entity::LineworkCodeMember& member :
         katana::entity::lineworkCodeMembers()) {
        out << " linework." << katana::core::lowered(member.name) << "="
            << recordValue(summary.linework.*member.spelling);
    }
    out << "\n";
    // What the OPEN warned of, kept in view: said once at the open, it
    // scrolled away while what those names defined still drew as plain lines.
    for (const std::string& name : summary.notLoaded) {
        out << "missing name=" << recordValue(name) << "\n";
    }
    if (!nothing) {
        out << formatCoverage(summary.coverage);
    }
    return out.str();
}

std::vector<std::string> undefinedRuleNames(const Document& document)
{
    std::vector<std::string> undefined;
    for (const std::string& name : document.surveyMap().stylesReferenced()) {
        if (!isPlainLinestyle(name) && document.definitionFor(name) == nullptr &&
            !katana::entity::isBuiltInSymbolName(name)) {
            undefined.push_back(name);
        }
    }
    return undefined;
}

std::string customisationJson(const Document& document)
{
    using Json = nlohmann::json;
    const CustomisationState& state = document.customisationState();
    const CustomisationSummary summary = customisationSummary(document);

    Json sources = Json::array();
    for (const CustomisationSource& source : state.sources) {
        sources.push_back({{"name", source.name},
                           {"definitions", source.definitions},
                           {"rules", source.rules},
                           {"notice", source.notice}});
    }
    Json colours = Json::object();
    for (const katana::entity::ColourTable::Entry& entry : state.colours.entries()) {
        colours[entry.name] = entry.colour.toHex();
    }
    Json linework = Json::object();
    for (const katana::entity::LineworkCodeMember& member :
         katana::entity::lineworkCodeMembers()) {
        linework[std::string(member.name)] = state.linework.*member.spelling;
    }

    // The lint CODE CHECK prints, counted. A count of the rules that cannot
    // be applied is called that, and never by the lint's own word for them:
    // a reply that succeeded must not hold a word a script takes for failure.
    const std::vector<LintIssue> issues = lintSurveyMap(
        document.surveyMap(), document.styleLibrary(), colourLookup(document),
        [](std::string_view name) { return katana::entity::isBuiltInSymbolName(name); });
    std::size_t cannotApply = 0;
    std::map<std::string, std::size_t> byKind;
    for (const LintIssue& issue : issues) {
        cannotApply += issue.severity == LintSeverity::Error ? 1 : 0;
        ++byKind[toString(issue.kind)];
    }

    Json basedOn = nullptr;
    if (state.basedOn) {
        basedOn = Json{{"name", state.basedOn->name}, {"digest", state.basedOn->digest}};
    }
    const Json json{
        {"name", state.name},
        {"origin", toString(state.origin)},
        {"kept", state.kept},
        {"description", state.description},
        {"notice", state.notice},
        {"basedOn", basedOn},
        {"builtIn", state.builtIn},
        {"sources", sources},
        {"counts",
         {{"definitions", summary.definitions},
          {"groups", summary.groups},
          {"symbols", summary.symbols},
          {"linestyles", summary.linestyles},
          {"rules", summary.rules},
          {"codes", summary.codes},
          {"colours", summary.colours}}},
        {"automation",
         {{"codesOnSurveyImport", state.automation.codesOnSurveyImport},
          {"lineworkOnSurveyImport", state.automation.lineworkOnSurveyImport}}},
        {"linework", linework},
        {"colours", colours},
        {"problems",
         {{"rules", summary.rules},
          {"cannotApply", cannotApply},
          {"warnings", issues.size() - cannotApply},
          {"byKind", byKind},
          {"undefined", undefinedRuleNames(document)}}},
        {"coverage",
         {{"styles", summary.coverage.styles},
          {"named", summary.coverage.named},
          {"resolved", summary.coverage.resolved},
          {"builtIn", summary.coverage.builtIn},
          {"unresolved", summary.coverage.unresolved},
          {"notLinestyles", summary.coverage.notLinestyles}}},
        {"missing", state.missingAtOpen},
        // What the start found, always written - empty when it had nothing
        // to say - so that a reader can tell "nothing went wrong" from a
        // build that does not say.
        {"start",
         {{"problems", state.startProblems},
          {"keptFromAnotherBuiltIn", state.keptFromAnotherBuiltIn}}},
    };
    // A name that is not UTF-8 is shown with U+FFFD rather than thrown at a
    // client that only asked what is loaded (as STATUS JSON does).
    return json.dump(2, ' ', false, Json::error_handler_t::replace);
}

} // namespace katana::cad
