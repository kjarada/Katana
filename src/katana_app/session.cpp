// The command-line session that katana_cli and katana_mcp both drive: one
// Document, the CommandInterpreter over it, and the verbs the front ends add
// above katana_cad (CUSTOMISE, CODE, MAPFILE, IMPORT, EXPORT...). Each line is
// reported on std::cout (what it did) and std::cerr (errors and warnings), as
// katana_cli always has; katana_mcp captures both around each line.

#include <algorithm>
#include <cctype>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/archive12d/customisation.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/cad/code_table.hpp"
#include "katana/cad/customisation_record.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/entity/tables.hpp"
#include "dxf_verbs.hpp"
#include "ifc_verbs.hpp"
#include "utility_verbs.hpp"
#include "session.hpp"

#if defined(KATANA_WITH_INTEROP)
#include "katana/commands/entity_commands.hpp"
#include "katana/interop/archive12d.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/dataset_info.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"
#endif

namespace {

bool isQuit(const std::string& line)
{
    std::string verb;
    for (const char ch : line) {
        if (ch == ' ' || ch == '\t') {
            if (!verb.empty()) {
                break;
            }
            continue;
        }
        verb += static_cast<char>(std::toupper(static_cast<unsigned char>(ch)));
    }
    return verb == "QUIT" || verb == "EXIT";
}

// The rest of the line after the verb: leading and trailing blanks removed, and
// one layer of surrounding quotes stripped, so a path with spaces works.
// Outside the interoperability guard: CODE, MAPFILE and CUSTOMISE use them too,
// and they are offered in a build with no GDAL.
std::string argumentOf(const std::string& line, std::size_t verbLength)
{
    std::string rest = line.substr(verbLength);
    const auto first = rest.find_first_not_of(" \t");
    if (first == std::string::npos) {
        return {};
    }
    const auto last = rest.find_last_not_of(" \t");
    rest = rest.substr(first, last - first + 1);
    if (rest.size() >= 2 && rest.front() == '"' && rest.back() == '"') {
        rest = rest.substr(1, rest.size() - 2);
    }
    return rest;
}

std::string upperVerb(const std::string& line)
{
    std::string verb;
    for (const char ch : line) {
        if (ch == ' ' || ch == '\t') {
            if (!verb.empty()) {
                break;
            }
            continue;
        }
        verb += static_cast<char>(std::toupper(static_cast<unsigned char>(ch)));
    }
    return verb;
}

// CUSTOMISE is here rather than in CommandInterpreter for the same reason
// IMPORT is: the interpreter belongs to katana_cad, which may not see
// archive12d, where the customisation readers live. Unlike IMPORT it needs
// no third-party library, so it is outside the interoperability guard and is
// offered even in a build with no GDAL.
// What the loaded customisation means for THIS drawing. "The linestyles are
// not showing" looks identical whether nothing is loaded, the library does
// not define what the drawing names, or the drawing's styles are plain
// continuous lines - so say which.
void reportCoverage(const katana::cad::Document& document)
{
    // The words are cad's, so the application's log says the same.
    std::cout << katana::cad::formatCoverage(katana::cad::customisationCoverage(document));
}

// The standard colour names reach cad only through a callback: cad may not
// see archive12d, which owns the table.
std::optional<katana::entity::Color> colourOf(std::string_view name)
{
    return katana::archive12d::standardColour(name);
}

// The first word of `text`, upper-cased, and the rest as argumentOf reads it.
std::pair<std::string, std::string> splitWord(const std::string& text)
{
    const std::size_t first = text.find_first_not_of(" \t");
    if (first == std::string::npos) {
        return {};
    }
    const std::size_t end = text.find_first_of(" \t", first);
    return {upperVerb(text), end == std::string::npos ? std::string{} : argumentOf(text, end)};
}

// The files of a load, by name and kind, for the record a project keeps of
// what it was drawn with (cad/customisation_record.hpp).
std::vector<katana::cad::CustomisationSource>
sourcesOf(const katana::archive12d::Customisation& loaded)
{
    std::vector<katana::cad::CustomisationSource> sources;
    for (const katana::archive12d::LoadedFile& file : loaded.files) {
        const std::u8string name = file.path.filename().u8string();
        sources.push_back({std::string(reinterpret_cast<const char*>(name.data()), name.size()),
                           file.kind == katana::archive12d::CustomisationFile::StyleLibrary});
    }
    return sources;
}

// Up to `most` names, quoted, and how many more: a load of the reference
// library replaces hundreds, and a line of them all says nothing.
std::string sampleOf(const std::vector<std::string>& names, std::size_t most = 8)
{
    std::string text;
    for (std::size_t i = 0; i < names.size() && i < most; ++i) {
        text += (i == 0 ? "\"" : ", \"") + names[i] + "\"";
    }
    if (names.size() > most) {
        text += " and " + std::to_string(names.size() - most) + " more";
    }
    return text;
}

// Whatever customisation ships with the application or sits beside it, so a
// session starts able to draw a survey rather than waiting to be told where
// its linestyles are. What it loaded is added to `loaded`.
void loadDefaultCustomisation(katana::cad::Document& document, const char* executable,
                              std::vector<katana::cad::CustomisationSource>& loaded)
{
    // Compiled in: nothing to find and nothing to load.
    const katana::archive12d::Customisation& built = katana::archive12d::builtinCustomisation();
    // A file of it that could not be read cost only itself, and is said
    // here (audit A12-06): unsaid, a damaged file drew every drawing's
    // linestyles as plain lines without a word.
    for (const std::string& error : built.errors) {
        std::cerr << "error: the built-in customisation: " << error << "\n";
    }
    for (const std::string& warning : built.warnings) {
        std::cerr << "warning: the built-in customisation: " << warning << "\n";
    }
    if (!built.empty()) {
        std::cout << "Customisation: " << built.library.size() << " linestyles and symbols and "
                  << built.map.size() << " survey code rules, built in\n";
        document.setStyleLibrary(built.library);
        document.setSurveyMap(built.map);
        katana::cad::recordCustomisationLoad(loaded, sourcesOf(built), false, false);
        return;
    }
    const auto paths = katana::archive12d::findCustomisation(std::filesystem::path(executable));
    if (paths.empty()) {
        return;
    }
    auto found = katana::archive12d::readCustomisation(paths);
    if (!found) {
        std::cerr << "warning: the customisation beside this program could not be read: "
                  << found.error().describe() << "\n";
        return;
    }
    for (const std::string& warning : found->warnings) {
        std::cerr << "warning: " << warning << "\n";
    }
    std::cout << "Customisation: " << found->library.size() << " definitions and "
              << found->map.size() << " survey code rules, from "
              << paths.front().parent_path().string() << "\n";
    document.setStyleLibrary(found->library);
    document.setSurveyMap(found->map);
    katana::cad::recordCustomisationLoad(loaded, sourcesOf(*found), false, false);
}

// CUSTOMISE [REPLACE] <file>...: a load MERGES into what is loaded (the lead's
// decision D1) through archive12d::mergeCustomisation - the merge the window's
// Format > Load Customisation makes, so the two cannot come to differ. A
// load's definitions replace those of the same name, and the rules it gives a
// key in a section replace that key's rules there; everything else loaded is
// kept. REPLACE swaps out what the load brought - and only that: a load with
// no survey code file never installs an empty map, nor one with no library an empty
// library (audit QT-21).
//
// Whether REPLACE was given is decided by the caller, which alone knows
// whether a word was quoted.
bool runCustomise(katana::cad::Document& document,
                  std::vector<katana::cad::CustomisationSource>& record,
                  std::vector<std::string>& missingAtOpen, const std::vector<std::string>& paths,
                  bool replace)
{
    if (replace && paths.empty()) {
        std::cerr << "error: InvalidArgument: CUSTOMISE REPLACE needs a file\n";
        return false;
    }
    if (paths.empty()) {
        const auto& library = document.styleLibrary();
        const auto& map = document.surveyMap();
        if (library.empty() && map.empty()) {
            std::cout << "No customisation is loaded.\n"
                      << "  CUSTOMISE <file> [<file>...]  loads style libraries (.4d) and "
                         "survey code files (.mapfile)\n";
            return true;
        }
        // Counted as the pickers offer them (decision D3, cad::symbolChoices
        // and cad::linetypeChoices), library definitions only. This line once
        // said "157 of them symbols", counting `mode vertex` alone - one of
        // D3's four reasons, and a minority of the symbols the reference
        // survey code files use - so here one definition can be counted in both.
        const auto fromLibrary = [](const std::vector<katana::cad::CatalogueEntry>& entries) {
            return std::ranges::count(entries, katana::cad::DefinitionSource::Library,
                                      &katana::cad::CatalogueEntry::source);
        };
        std::cout << library.size() << " linestyle and symbol definitions in "
                  << katana::entity::styleGroups(library).size() << " groups: "
                  << fromLibrary(katana::cad::symbolChoices(document))
                  << " offered as symbols, "
                  << fromLibrary(katana::cad::linetypeChoices(document, false))
                  << " as linestyles (one definition can be both)\n"
                  << map.size() << " survey code rules over " << map.keys().size()
                  << " distinct codes\n";
        reportCoverage(document);
        return true;
    }

    // A file named twice in one load is read once: read twice, each of its
    // rules would be in the map twice. The window's CUSTOMISE and Format > Load
    // go through the same rule.
    const katana::cad::DistinctFiles distinct = katana::cad::distinctCustomisationFiles(
        std::vector<std::filesystem::path>(paths.begin(), paths.end()));
    for (const std::filesystem::path& repeat : distinct.repeats) {
        std::cout << "  " << repeat.string() << " is named twice in this load; it is read once\n";
    }
    auto loaded = katana::archive12d::readCustomisation(distinct.files);
    if (!loaded) {
        std::cerr << "error: " << loaded.error().describe() << "\n";
        return false;
    }
    for (const std::string& warning : loaded->warnings) {
        std::cout << "  warning: " << warning << "\n";
    }
    const auto mode =
        replace ? katana::archive12d::LoadMode::Replace : katana::archive12d::LoadMode::Merge;
    katana::archive12d::CustomisationMerge merged = katana::archive12d::mergeCustomisation(
        document.styleLibrary(), document.surveyMap(), *loaded, mode);
    // What each file did to what was loaded before it: a person loading their
    // own symbol file wants to see it ADDED to the rest, not put in its place.
    // The kind is said in the words the help and the window use ("survey code
    // file", "style library"), not the reader's name for the format.
    for (const katana::archive12d::FileMerge& file : merged.files) {
        const bool map = file.kind == katana::archive12d::CustomisationFile::MapFile;
        std::cout << "  " << (file.name.empty() ? std::string("(no file)") : file.name) << ": "
                  << (map ? "survey code file" : "style library") << ", " << file.added.size()
                  << " added, " << file.replaced.size() << " replaced"
                  << (map ? " (codes, once for each section)" : "");
        if (!file.replaced.empty()) {
            std::cout << ": " << sampleOf(file.replaced);
        }
        std::cout << "\n";
    }
    for (const std::string& problem : merged.problems) {
        std::cerr << "error: not installed: " << problem << "\n";
    }
    if (!merged.removedDefinitions.empty()) {
        std::cout << "  " << merged.removedDefinitions.size()
                  << " definitions the load did not bring are gone: "
                  << sampleOf(merged.removedDefinitions) << "\n";
    }
    if (!merged.removedKeys.empty()) {
        std::cout << "  " << merged.removedKeys.size()
                  << " codes the load did not bring are gone: " << sampleOf(merged.removedKeys)
                  << "\n";
    }
    document.setStyleLibrary(std::move(merged.library));
    document.setSurveyMap(std::move(merged.map));
    const std::vector<katana::cad::CustomisationSource> sources = sourcesOf(*loaded);
    katana::cad::recordCustomisationLoad(record, sources, replace && merged.libraryLoaded,
                                         replace && merged.mapLoaded);
    katana::cad::noteCustomisationLoaded(missingAtOpen, sources);
    std::cout << "Loaded now: " << document.styleLibrary().size() << " definitions, "
              << document.surveyMap().size() << " survey code rules\n";

    // A customisation need not be self-contained. Saying what is missing is
    // the difference between a symbol that is plainly absent and one that is
    // silently drawn as a dot. Judged against everything now loaded, since a
    // survey code file may name what an earlier load defined.
    std::vector<std::string> missing;
    for (const std::string& name : document.surveyMap().stylesReferenced()) {
        if (!katana::cad::isPlainLinestyle(name) && document.definitionFor(name) == nullptr &&
            !katana::entity::isBuiltInSymbolName(name)) {
            missing.push_back(name);
        }
    }
    if (!missing.empty()) {
        std::cout << "  " << missing.size()
                  << " names the survey codes ask for that no loaded library defines:";
        for (std::size_t i = 0; i < missing.size() && i < 8; ++i) {
            std::cout << (i == 0 ? " " : ", ") << "\"" << missing[i] << "\"";
        }
        std::cout << (missing.size() > 8 ? ", ...\n" : "\n");
    }
    reportCoverage(document);
    return true;
}

bool requireMap(const katana::cad::Document& document)
{
    if (document.surveyMap().empty()) {
        std::cerr << "error: InvalidState: no survey codes are loaded; use CUSTOMISE <file> "
                     "first\n";
        return false;
    }
    return true;
}

// CODE [<property>] applies the loaded survey codes to the drawing: every entity
// carrying a field code gets the model, the style and the attributes the
// survey codes say it should have, as ONE undoable command. CODE EXPLAIN <code>
// says why a code gets what it gets, and CODE CENSUS [<property>] lists the
// codes the drawing carries - so a property cannot be called EXPLAIN or
// CENSUS here, which no survey format does.
bool runCode(katana::cad::Document& document, const std::string& rest)
{
    if (!requireMap(document)) {
        return false;
    }
    const auto [word, argument] = splitWord(rest);
    if (word == "EXPLAIN") {
        if (argument.empty()) {
            std::cerr << "error: InvalidArgument: CODE EXPLAIN <code>\n";
            return false;
        }
        const auto explanation = katana::cad::explainCode(
            document.surveyMap(), argument,
            [&document](std::string_view name) { return document.definitionFor(name); },
            colourOf);
        std::cout << katana::cad::formatCodeExplanation(explanation);
        return true;
    }
    if (word == "CENSUS") {
        std::cout << katana::cad::formatCodeCensus(katana::cad::codeCensus(document, argument));
        return true;
    }

    katana::cad::SurveyCodingOptions options;
    options.property = argumentOf(rest, 0);
    options.colourOf = colourOf;
    katana::cad::SurveyCodingReport report;
    auto command = katana::cad::applySurveyCodes(document, options, &report);
    if (!command) {
        std::cerr << "error: " << command.error().describe() << "\n";
        return false;
    }
    std::cout << katana::cad::formatCodingReport(report);
    if (*command == nullptr) {
        std::cout << "Nothing to change.\n";
        return true;
    }
    if (const auto status = document.execute(std::move(*command)); !status) {
        std::cerr << "error: " << status.error().describe() << "\n";
        return false;
    }
    std::cout << "Applied as one command. UNDO puts it all back.\n";
    return true;
}

// MAPFILE LIST [<filter>] lists the loaded map one code per line, filtered
// with case ignored; MAPFILE CHECK lints it, and fails - so a script stops -
// when a rule cannot be applied as written.
bool runMapfile(const katana::cad::Document& document, const std::string& rest)
{
    const auto [word, argument] = splitWord(rest);
    if (word != "LIST" && word != "CHECK") {
        std::cerr << "error: InvalidArgument: MAPFILE LIST [<filter>] | CHECK\n";
        return false;
    }
    if (!requireMap(document)) {
        return false;
    }
    const katana::entity::SurveyMap& map = document.surveyMap();
    if (word == "LIST") {
        std::cout << katana::cad::formatCodeTable(katana::cad::codeTable(map), argument);
        return true;
    }
    const auto issues = katana::cad::lintSurveyMap(
        map, document.styleLibrary(), colourOf,
        [](std::string_view name) { return katana::entity::isBuiltInSymbolName(name); });
    std::cout << katana::cad::formatLint(issues, map.size());
    return std::none_of(issues.begin(), issues.end(), [](const katana::cad::LintIssue& issue) {
        return issue.severity == katana::cad::LintSeverity::Error;
    });
}

#if defined(KATANA_WITH_INTEROP)

// IMPORT, EXPORT and REFS live here rather than in CommandInterpreter because
// the interpreter belongs to katana_cad, which must not see GDAL or PDAL - that
// separation is what lets katana_cad build with -DKATANA_BUILD_IO=OFF. The
// front ends own the interoperability, and both front ends offer the same verbs.
struct InteropState {
    katana::interop::ReferenceData reference;
};

bool importPath(katana::cad::Document& document, InteropState& state, const std::string& text)
{
    namespace interop = katana::interop;
    namespace cmd = katana::commands;
    const std::filesystem::path path(text);

    // IMPORT <file> LOCAL shifts the data to sit alongside the drawing instead
    // of at its own survey coordinates.
    std::filesystem::path file = path;
    bool shiftToLocal = false;
    {
        const std::string raw = path.string();
        const std::size_t space = raw.find_last_of(" 	");
        if (space != std::string::npos) {
            std::string tail = raw.substr(space + 1);
            std::transform(tail.begin(), tail.end(), tail.begin(),
                           [](unsigned char c) { return static_cast<char>(std::toupper(c)); });
            if (tail == "LOCAL") {
                shiftToLocal = true;
                file = std::filesystem::path(raw.substr(0, space));
            }
        }
    }

    switch (interop::kindForPath(file)) {
    case interop::SourceKind::Vector: {
        interop::VectorImportOptions options;
        auto probe = interop::importVector(file, options);
        if (probe && shiftToLocal && !probe->bounds.empty()) {
            options.originShift = katana::geometry::Vec2(probe->bounds.min.x, probe->bounds.min.y);
        }
        auto imported = shiftToLocal ? interop::importVector(file, options) : std::move(probe);
        if (!imported) {
            std::cerr << "error: " << imported.error().describe() << '\n';
            return false;
        }
        auto transaction = std::make_unique<cmd::Transaction>("IMPORT");
        for (const std::string& name : imported->layersNeeded) {
            if (!document.model().layers.contains(name)) {
                katana::entity::Layer layer;
                layer.name = name;
                transaction->add(cmd::createLayer(layer));
            }
        }
        const std::size_t count = imported->entities.size();
        const auto bounds = imported->bounds;
        const auto existingBounds = document.model().entities.bounds();
        transaction->add(cmd::createEntities(std::move(imported->entities)));
        const auto status = document.execute(std::move(transaction));
        if (!status) {
            std::cerr << "error: " << status.error().describe() << '\n';
            return false;
        }
        std::cout << "imported " << count << " entities from " << file.filename().string()
                  << '\n';
        for (const std::string& warning : imported->warnings) {
            std::cout << "  " << warning << '\n';
        }
        if (!bounds.empty()) {
            std::cout << "  extent " << bounds.min.x << "," << bounds.min.y << " to "
                      << bounds.max.x << "," << bounds.max.y << '\n';
        }
        // Said out loud rather than left for the user to discover by zooming to
        // extents and finding their drawing has become a dot.
        const auto advice = interop::advisePlacement(existingBounds, bounds);
        if (advice.farApart) {
            std::cout << "  WARNING: " << advice.message << '\n'
                      << "  undo, then re-import with  IMPORT <file> LOCAL  to shift it "
                         "alongside the drawing\n";
        }
        return true;
    }
    case interop::SourceKind::Archive12d: {
        interop::Archive12dImportOptions options;
        auto probe = interop::importArchive12d(file, options);
        if (probe && shiftToLocal && !probe->bounds.empty()) {
            options.originShift = katana::geometry::Vec2(probe->bounds.min.x, probe->bounds.min.y);
        }
        auto imported = shiftToLocal && probe ? interop::importArchive12d(file, options)
                                             : std::move(probe);
        if (!imported) {
            std::cerr << "error: " << imported.error().describe() << '\n';
            return false;
        }
        // Layers, entities and alignments are ONE undo step, like any import.
        auto transaction = std::make_unique<cmd::Transaction>("IMPORT");
        for (const katana::entity::Layer& layer : imported->layersNeeded) {
            if (!document.model().layers.contains(layer.name)) {
                transaction->add(cmd::createLayer(layer));
            }
        }
        for (const katana::entity::Style& style : imported->stylesNeeded) {
            if (!document.model().styles.contains(style.name)) {
                transaction->add(cmd::createStyle(style));
            }
        }
        const std::size_t count = imported->entities.size();
        const auto existingBounds = document.model().entities.bounds();
        if (count != 0) {
            transaction->add(cmd::createEntities(std::move(imported->entities)));
        }
        std::size_t alignments = 0;
        for (katana::entity::Alignment& alignment : imported->alignments) {
            // A name the drawing already has would fail the whole transaction.
            const std::string base = alignment.name;
            for (int copy = 2; document.model().alignments.contains(alignment.name); ++copy) {
                alignment.name = base + " (" + std::to_string(copy) + ")";
            }
            transaction->add(cmd::createAlignment(alignment));
            ++alignments;
        }
        const auto status = document.execute(std::move(transaction));
        if (!status) {
            std::cerr << "error: " << status.error().describe() << '\n';
            return false;
        }
        std::cout << "imported " << count << " entities and " << alignments
                  << " alignments from " << file.filename().string() << " ("
                  << imported->encoding;
        if (!imported->archiveVersion.empty()) {
            std::cout << ", 12d archive " << imported->archiveVersion;
        }
        std::cout << ")\n";
        for (const auto& tally : imported->tally) {
            std::cout << "  " << tally.keyword << ": " << tally.read << " read, " << tally.imported
                      << " imported\n";
        }
        for (const auto& surface : imported->surfaces) {
            // The CLI has nowhere to keep a surface; the desktop application
            // does. Said, so that "12 tins read" is not taken for "12 kept".
            std::cout << "  surface \"" << surface.name << "\": "
                      << surface.surface.triangleCount() << " triangles (read and checked; the "
                      << "CLI holds no surfaces - import in the desktop application to use it)\n";
        }
        if (!imported->meshes.empty()) {
            // Same as a surface: read, checked, and nowhere for the CLI to
            // keep it. Said in numbers so it is not taken for kept.
            std::size_t triangles = 0;
            for (const auto& mesh : imported->meshes) {
                triangles += mesh.mesh.triangleCount();
            }
            std::cout << "  " << imported->meshes.size() << " meshes, " << triangles
                      << " triangles (read and checked; the CLI holds no meshes - import in the "
                      << "desktop application to see them)\n";
        }
        for (auto& cloud : imported->clouds) {
            std::cout << "  point cloud \"" << cloud.name << "\": " << cloud.points.size()
                      << " points\n";
            state.reference.add(std::move(cloud));
        }
        for (const std::string& warning : imported->warnings) {
            std::cout << "  " << warning << '\n';
        }
        if (!imported->bounds.empty()) {
            std::cout << "  extent " << imported->bounds.min.x << "," << imported->bounds.min.y
                      << " to " << imported->bounds.max.x << "," << imported->bounds.max.y << '\n';
        }
        const auto advice = interop::advisePlacement(existingBounds, imported->bounds);
        if (advice.farApart) {
            std::cout << "  WARNING: " << advice.message << '\n'
                      << "  undo, then re-import with  IMPORT <file> LOCAL  to shift it "
                         "alongside the drawing\n";
        }
        return true;
    }
    case interop::SourceKind::Raster:
    case interop::SourceKind::PointCloud:
        // Reference data is drawn at its own coordinates and has no shift to
        // take, so LOCAL is refused by name rather than dropped - or, as it
        // was, left on the end of the path to fail as a missing file
        // "ortho.tif LOCAL" (audit QT-13, QT-14).
        if (shiftToLocal) {
            std::cerr << "error: InvalidArgument: LOCAL is not supported for rasters and point "
                         "clouds, which are reference data drawn at their own coordinates\n";
            return false;
        }
        break;
    default:
        break;
    }

    switch (interop::kindForPath(file)) {
    case interop::SourceKind::Raster: {
        auto raster = interop::importRaster(file);
        if (!raster) {
            std::cerr << "error: " << raster.error().describe() << '\n';
            return false;
        }
        const int w = raster->width;
        const int h = raster->height;
        const bool georeferenced = raster->hasGeotransform;
        const auto bounds = raster->worldBounds();
        state.reference.add(std::move(*raster));
        std::cout << "imported raster " << file.filename().string() << " (" << w << "x" << h
                  << " px)";
        if (georeferenced) {
            std::cout << " covering " << bounds.min.x << ',' << bounds.min.y << " to "
                      << bounds.max.x << ',' << bounds.max.y;
        } else {
            std::cout << " (not georeferenced; placed at the origin)";
        }
        std::cout << '\n';
        return true;
    }
    case interop::SourceKind::PointCloud: {
        auto cloud = interop::importPointCloud(file);
        if (!cloud) {
            std::cerr << "error: " << cloud.error().describe() << '\n';
            return false;
        }
        const std::size_t shown = cloud->points.size();
        const std::uint64_t total = cloud->sourcePointCount;
        const bool decimated = cloud->isDecimated();
        state.reference.add(std::move(*cloud));
        std::cout << "imported point cloud " << file.filename().string() << " (" << shown
                  << " points";
        if (decimated) {
            std::cout << " sampled from " << total;
        }
        std::cout << ")\n";
        return true;
    }
    case interop::SourceKind::Vector:
    case interop::SourceKind::Archive12d:
    case interop::SourceKind::Unknown:
        break;
    }
    std::cerr << "error: Unsupported: no importer for '" << file.extension().string() << "'\n";
    return false;
}

// INFO <file>: what a GIS file or point cloud holds, without importing it. The
// wording is interop::formatDescription's, which the desktop application's
// GIS > Dataset Information shows too.
bool describePath(const std::string& text)
{
    auto description = katana::interop::describeSource(std::filesystem::path(text));
    if (!description) {
        std::cerr << "error: " << description.error().describe() << '\n';
        return false;
    }
    std::cout << katana::interop::formatDescription(*description);
    return true;
}

// The two paths of COPC <source> <destination>: each either "quoted" or one
// word, so a path with spaces is quoted and one without need not be.
std::vector<std::string> pathArguments(const std::string& text)
{
    std::vector<std::string> paths;
    std::size_t at = 0;
    while (at < text.size()) {
        at = text.find_first_not_of(" \t", at);
        if (at == std::string::npos) {
            break;
        }
        std::size_t end = 0;
        if (text[at] == '"') {
            end = text.find('"', at + 1);
            if (end == std::string::npos) {
                end = text.size();
            }
            paths.push_back(text.substr(at + 1, end - at - 1));
            at = end + 1;
        } else {
            end = text.find_first_of(" \t", at);
            if (end == std::string::npos) {
                end = text.size();
            }
            paths.push_back(text.substr(at, end - at));
            at = end;
        }
    }
    return paths;
}

// COPC <source> <destination.copc.laz>: the whole file rewritten as a Cloud
// Optimised Point Cloud, every point kept, so that later reads can ask for a
// level of detail instead of decimating (PLAN.MD Phase 17).
bool convertToCopc(const std::string& text)
{
    const std::vector<std::string> paths = pathArguments(text);
    if (paths.size() != 2) {
        std::cerr << "error: InvalidArgument: usage: COPC <source> <destination.copc.laz>\n";
        return false;
    }
    const auto status = katana::pointcloud::PointCloudEngine{}.convertToCopc(paths[0], paths[1]);
    if (!status) {
        std::cerr << "error: " << status.error().describe() << '\n';
        return false;
    }
    std::cout << "converted " << std::filesystem::path(paths[0]).filename().string() << " to "
              << std::filesystem::path(paths[1]).filename().string() << " (COPC)\n";
    return true;
}

bool exportPath(katana::cad::Document& document, const std::string& text)
{
    namespace interop = katana::interop;
    const std::filesystem::path target(text);
    if (interop::kindForPath(target) == interop::SourceKind::Archive12d) {
        // No surfaces: the CLI holds none.
        auto archive = interop::exportArchive12d(document.model(), {}, target);
        if (!archive) {
            std::cerr << "error: " << archive.error().describe() << '\n';
            return false;
        }
        std::cout << "exported " << archive->entitiesWritten << " entities and "
                  << archive->alignmentsWritten << " alignments (12d archive, "
                  << archive->bytesWritten << " bytes)\n";
        for (const std::string& warning : archive->warnings) {
            std::cout << "  " << warning << '\n';
        }
        return true;
    }
    auto result = interop::exportVector(document.model(), target);
    if (!result) {
        std::cerr << "error: " << result.error().describe() << '\n';
        return false;
    }
    std::cout << "exported " << result->featuresWritten << " features (" << result->driver
              << ")\n";
    for (const std::string& warning : result->warnings) {
        std::cout << "  " << warning << '\n';
    }
    return true;
}

void listReferences(const InteropState& state)
{
    if (state.reference.empty()) {
        std::cout << "no reference layers\n";
        return;
    }
    for (const auto& raster : state.reference.rasters()) {
        std::cout << raster.id << "  Raster  " << raster.name << "  " << raster.width << "x"
                  << raster.height << " px\n";
    }
    for (const auto& cloud : state.reference.pointClouds()) {
        std::cout << cloud.id << "  PointCloud  " << cloud.name << "  " << cloud.points.size()
                  << " pts\n";
    }
}

// nullopt when the line is not one of the interoperability verbs.
std::optional<bool> runInterop(katana::cad::Document& document, InteropState& state,
                               const std::string& line)
{
    const std::string verb = upperVerb(line);
    if (verb == "IMPORT") {
        const std::string argument = argumentOf(line, line.find_first_of(" \t") == std::string::npos
                                                          ? line.size()
                                                          : line.find_first_of(" \t"));
        if (argument.empty()) {
            std::cerr << "error: InvalidArgument: usage: IMPORT <file>\n";
            return false;
        }
        return importPath(document, state, argument);
    }
    if (verb == "EXPORT") {
        const std::string argument = argumentOf(line, line.find_first_of(" \t") == std::string::npos
                                                          ? line.size()
                                                          : line.find_first_of(" \t"));
        if (argument.empty()) {
            std::cerr << "error: InvalidArgument: usage: EXPORT <file>\n";
            return false;
        }
        return exportPath(document, argument);
    }
    if (verb == "REFS") {
        listReferences(state);
        return true;
    }
    if (verb == "INFO" || verb == "COPC") {
        const std::size_t space = line.find_first_of(" \t");
        const std::string argument = argumentOf(line, space == std::string::npos ? line.size() : space);
        if (argument.empty()) {
            std::cerr << "error: InvalidArgument: usage: " << verb
                      << (verb == "INFO" ? " <file>\n" : " <source> <destination.copc.laz>\n");
            return false;
        }
        // COPC takes the raw rest of the line: argumentOf strips one pair of
        // surrounding quotes, which would run two quoted paths together.
        return verb == "INFO" ? describePath(argument) : convertToCopc(line.substr(space));
    }
    return std::nullopt;
}

#endif // KATANA_WITH_INTEROP

// One session's mutable state. Grouped so that adding a verb that needs more
// context does not mean changing every signature again.
struct SessionState {
    katana::cad::Document& document;
    katana::cad::CommandInterpreter& interpreter;
    // The customisation files loaded, in load order: what a SAVE records
    // in the project, and what an OPEN compares the project's record with.
    std::vector<katana::cad::CustomisationSource> customisation{};
    // What the last OPEN found the project recorded but not loaded, less
    // what was loaded since: a SAVE keeps them in the record, since this
    // session cannot judge a file it never had.
    std::vector<std::string> customisationMissingAtOpen{};
#if defined(KATANA_WITH_INTEROP)
    InteropState interop;
#endif
};

// Returns false when the command failed.
bool runLine(SessionState& session, const std::string& line)
{
    if (!line.empty() && line.front() == '#') {
        return true;
    }
    if (upperVerb(line) == "CODE" || upperVerb(line) == "MAPFILE") {
        const std::size_t first = line.find_first_not_of(" \t");
        const std::size_t space = line.find_first_of(" \t", first);
        const std::string rest = space == std::string::npos ? std::string{} : line.substr(space);
        return upperVerb(line) == "CODE" ? runCode(session.document, rest)
                                         : runMapfile(session.document, rest);
    }
    if (upperVerb(line) == "CUSTOMISE" || upperVerb(line) == "CUSTOMIZE") {
        // Paths may have spaces, so they are taken as quoted words where they
        // are quoted and as plain words where they are not.
        std::vector<std::string> paths;
        // REPLACE is the keyword only as the first word, UNQUOTED and whole:
        // a quoted "replace me.mapfile" is a file. Upper-casing the first
        // blank-delimited word of the first path took that file for the
        // keyword and dropped it - or, given twice, replaced the loaded map
        // with it instead of merging.
        bool replace = false;
        std::size_t at = line.find_first_of(" \t");
        while (at != std::string::npos && at < line.size()) {
            while (at < line.size() && (line[at] == ' ' || line[at] == '\t')) {
                ++at;
            }
            if (at >= line.size()) {
                break;
            }
            if (line[at] == '"') {
                const std::size_t end = line.find('"', at + 1);
                if (end == std::string::npos) {
                    std::cerr << "error: InvalidArgument: a quoted path is never closed\n";
                    return false;
                }
                paths.push_back(line.substr(at + 1, end - at - 1));
                at = end + 1;
            } else {
                const std::size_t end = line.find_first_of(" \t", at);
                std::string word = line.substr(at, end == std::string::npos ? end : end - at);
                at = end;
                // An unquoted word has no blanks, so upperVerb upper-cases
                // the whole of it.
                if (paths.empty() && !replace && upperVerb(word) == "REPLACE") {
                    replace = true;
                    continue;
                }
                paths.push_back(std::move(word));
            }
        }
        return runCustomise(session.document, session.customisation,
                            session.customisationMissingAtOpen, paths, replace);
    }
    // The AS 5488 subsurface utility tools read schedules and report; they
    // never touch the drawing (utility_verbs.hpp).
    if (upperVerb(line) == "UTILITY") {
        const std::size_t space = line.find_first_of(" \t", line.find_first_not_of(" \t"));
        return katana::app::runUtilityVerb(
            space == std::string::npos ? std::string_view{} : std::string_view(line).substr(space));
    }
    // A .ifc is read and written natively, with or without GDAL (ifc_verbs.hpp).
    if (const std::string verb = upperVerb(line); verb == "IMPORT" || verb == "EXPORT") {
        const std::size_t space = line.find_first_of(" \t", line.find_first_not_of(" \t"));
        if (const std::optional<bool> handled = katana::app::runIfcVerb(
                session.document, verb,
                space == std::string::npos ? std::string_view{}
                                           : std::string_view(line).substr(space))) {
            return *handled;
        }
    }
    // A .dxf is read and written natively, with or without GDAL (dxf_verbs.hpp).
    if (const std::string verb = upperVerb(line); verb == "IMPORT" || verb == "EXPORT") {
        const std::size_t space = line.find_first_of(" \t", line.find_first_not_of(" \t"));
        if (const std::optional<bool> handled = katana::app::runDxfVerb(
                session.document, verb,
                argumentOf(line, space == std::string::npos ? line.size() : space))) {
            return *handled;
        }
    }
#if defined(KATANA_WITH_INTEROP)
    // Interoperability verbs are handled before the interpreter sees the line,
    // because they live above katana_cad rather than inside it.
    if (const std::optional<bool> handled =
            runInterop(session.document, session.interop, line)) {
        return *handled;
    }
#endif
    // The project records the customisation it was drawn with - the files'
    // names, never their definitions, which are session data (D1) - so that
    // opening it where they are not loaded can say so. Only for a SAVE that
    // has somewhere to go, as in the window: writing the record marks the
    // drawing modified whether or not the save then happens.
    const std::string verb = upperVerb(line);
    if (verb == "SAVE" &&
        katana::cad::typedSaveHasDestination(line, session.document.hasProject())) {
        katana::storage::ProjectMetadata metadata = session.document.metadata();
        metadata.customisation = katana::cad::customisationRecordToSave(
            metadata.customisation, session.customisationMissingAtOpen,
            session.document.styleLibrary(), session.customisation);
        session.document.setMetadata(std::move(metadata));
    }
    const auto reply = session.interpreter.run(line);
    if (!reply) {
        std::cerr << "error: " << reply.error().describe() << '\n';
        return false;
    }
    if (!reply->empty()) {
        std::cout << *reply << '\n';
    }
    if (verb == "OPEN") {
        // A warning, not a refusal: the drawing opens and draws, but what a
        // missing file defined draws as a plain line.
        const std::vector<std::string> missing = katana::cad::customisationNotLoaded(
            session.document.metadata().customisation, session.document.styleLibrary(),
            session.customisation);
        session.customisationMissingAtOpen = missing;
        if (!missing.empty()) {
            std::cerr << "warning: this project was drawn with customisation files that are not "
                         "loaded: "
                      << sampleOf(missing, missing.size()) << "\n";
        }
    }
    return true;
}

} // namespace

namespace katana::app {

struct Session::State {
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
#if defined(KATANA_WITH_INTEROP)
    SessionState session{document, interpreter, {}, {}, {}};
#else
    SessionState session{document, interpreter, {}, {}};
#endif
};

Session::Session(const char* executable) : state_(std::make_unique<State>())
{
    if (executable != nullptr) {
        loadDefaultCustomisation(state_->document, executable, state_->session.customisation);
    }
}

Session::~Session() = default;

bool Session::run(const std::string& line)
{
    return runLine(state_->session, line);
}

katana::cad::Document& Session::document()
{
    return state_->document;
}

const katana::cad::Document& Session::document() const
{
    return state_->document;
}

bool Session::isQuit(const std::string& line)
{
    return ::isQuit(line);
}

std::string Session::helpText()
{
    std::string text = katana::cad::CommandInterpreter::helpText();
    text += "\n"
            "Survey    CODE [<property>]  apply the loaded survey codes to every\n"
            "          entity carrying a field code (found when not named)\n"
            "          CODE EXPLAIN <code>  why a code gets what it gets\n"
            "          CODE CENSUS [<property>]  the codes this drawing carries\n"
            "          MAPFILE LIST [<filter>] | CHECK  the loaded survey codes\n"
            "          CUSTOMISE [REPLACE] <file> [<file>...]  load style\n"
            "          libraries (.4d) and survey code files (.mapfile), merged\n"
            "          into what is loaded; CUSTOMISE alone reports what is loaded\n";
    text += katana::app::utilityHelpText();
    text += katana::app::ifcHelpText();
#if defined(KATANA_WITH_INTEROP)
    text += "Interop   IMPORT <file> [LOCAL] | EXPORT <file> | REFS\n"
            "          vector -> entities; raster and point cloud -> "
            "reference layers\n"
            "          INFO <file>  what a GIS file or point cloud holds, "
            "without importing it\n"
            "          COPC <source> <destination.copc.laz>  rewrite a point "
            "cloud as COPC\n";
#else
    text += "Interop   IMPORT <file.dxf> | EXPORT <file.dxf>  (this build has no GDAL: DXF only)\n";
#endif
    return text;
}

} // namespace katana::app
