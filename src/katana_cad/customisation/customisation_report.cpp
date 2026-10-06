#include "katana/cad/customisation_report.hpp"

#include <algorithm>
#include <sstream>
#include <string>

#include "katana/cad/code_table.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/entity/style_library.hpp"

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

} // namespace

CustomisationSummary customisationSummary(const Document& document,
                                          const std::vector<CustomisationSource>& loaded,
                                          const std::vector<std::string>& missingAtOpen)
{
    CustomisationSummary summary;
    const katana::entity::StyleLibrary& library = document.styleLibrary();
    summary.definitions = library.size();
    summary.groups = katana::entity::styleGroups(library).size();
    summary.symbols = fromLibrary(symbolChoices(document));
    summary.linestyles = fromLibrary(linetypeChoices(document, false));
    summary.rules = document.surveyMap().size();
    summary.codes = document.surveyMap().keys().size();
    summary.loaded = loaded;
    summary.notLoaded = missingAtOpen;
    summary.coverage = customisationCoverage(document);
    return summary;
}

std::string formatCustomisationSummary(const CustomisationSummary& summary)
{
    std::ostringstream out;
    const bool nothing = summary.definitions == 0 && summary.rules == 0;
    if (nothing) {
        out << "No customisation is loaded.\n"
            << "  CUSTOMISE <file> [<file>...]  loads style libraries (.4d) and survey code "
               "files (.mapfile)\n";
    } else {
        out << counted(summary.definitions, "linestyle and symbol definition",
                       "linestyle and symbol definitions")
            << " in " << counted(summary.groups, "group", "groups") << ": " << summary.symbols
            << " offered as symbols, " << summary.linestyles
            << " as linestyles (one definition can be both)\n"
            << counted(summary.rules, "survey code rule", "survey code rules") << " over "
            << counted(summary.codes, "distinct code", "distinct codes") << "\n";
    }
    // Which files, because "is my symbol file loaded?" is the question a
    // count cannot answer.
    if (!summary.loaded.empty()) {
        out << "Loaded files, in load order:\n";
        for (const CustomisationSource& file : summary.loaded) {
            // By what it brought: definitions alone are a style library's,
            // rules alone a survey code file's, and both one customisation's.
            // One that brought NEITHER is still a source - a table of
            // colours, a set of control codes - and is said to be that, not
            // passed off as the survey code file it would otherwise fall
            // through to.
            const char* kind = "a customisation with no definitions or survey code rules";
            if (file.definitions && file.rules) {
                kind = "a customisation";
            } else if (file.definitions) {
                kind = "a style library";
            } else if (file.rules) {
                kind = "a survey code file";
            }
            out << "  \"" << file.name << "\", " << kind << "\n";
        }
    }
    // What the OPEN warned of, kept in view: said once at the open, it
    // scrolled away while what those files defined still drew as plain lines.
    if (!summary.notLoaded.empty()) {
        out << "This project was drawn with customisation files that are not loaded:";
        for (std::size_t i = 0; i < summary.notLoaded.size(); ++i) {
            out << (i == 0 ? " \"" : ", \"") << summary.notLoaded[i] << "\"";
        }
        out << "\n";
    }
    if (!nothing) {
        out << formatCoverage(summary.coverage);
    }
    return out.str();
}

std::string customisationReport(const Document& document,
                                const std::vector<CustomisationSource>& loaded,
                                const std::vector<std::string>& missingAtOpen)
{
    return formatCustomisationSummary(customisationSummary(document, loaded, missingAtOpen));
}

} // namespace katana::cad
