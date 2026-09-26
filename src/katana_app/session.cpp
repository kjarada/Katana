// The command-line session that katana_cli and katana_mcp both drive: one
// Document, the CommandInterpreter over it, and the verbs the front ends add
// above katana_cad (CUSTOMISE, IMPORT, EXPORT, INFO <file>, COPC...). Each line is
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
#include "katana/cad/customisation_report.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/tables.hpp"
#include "dxf_verbs.hpp"
#include "ifc_verbs.hpp"
#include "import_records.hpp"
#include "session.hpp"

#if defined(KATANA_WITH_INTEROP)
#include "geo/geo_verbs.hpp"
#include "geo/references.hpp"
#include "katana/interop/reference_data.hpp"
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

// The desktop window's own verbs (MainWindow::dispatchLine), which this
// session cannot run: why, and what to use here instead. Answered by name,
// not as an unknown command, so a script written for File > Run Script that
// uses one fails in katana_cli and katana_mcp saying where it runs.
// Nullptr for any other verb.
const char* windowOnlyVerb(const std::string& verb)
{
    if (verb == "PLOT" || verb == "PLOTSHEETS" || verb == "SNAPSHOT") {
        return "is the desktop window's: its painter is Qt's, which katana_cli and katana_mcp "
               "do not have; a headless katana run has --plot and --plot-sheets";
    }
    if (verb == "ONLINE") {
        return "is the desktop window's (GIS > Online Data), which keeps the provider keys and "
               "runs the requests; run katana headless with --command \"ONLINE ...\"";
    }
    if (verb == "SCRIPT") {
        return "is the desktop window's: katana_cli runs a script given as its argument, and "
               "katana_mcp with katana_run_script";
    }
    return nullptr;
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
// see archive12d, which owns the table. The interpreter's CODE and MAPFILE
// CHECK are given it (CommandInterpreter::setColourLookup).
std::optional<katana::entity::Color> colourOf(std::string_view name)
{
    return katana::archive12d::standardColour(name);
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
    // Alone, it reports: what is loaded, from which files, what the project
    // was drawn with that is not, and what it covers here - the words the
    // window's CUSTOMISE says too (cad/customisation_report.hpp).
    if (paths.empty()) {
        std::cout << katana::cad::customisationReport(document, record, missingAtOpen);
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

#if defined(KATANA_WITH_INTEROP)

// What the geoprocessing executor's verbs - IMPORT, EXPORT, INFO <file>, REFS,
// COPC, GDAL and the families after them (geo/geo_verbs.hpp) - keep beside
// the drawing. They live above katana_cad, which must not see GDAL or PDAL:
// that separation is what lets katana_cad build with -DKATANA_BUILD_IO=OFF.
struct InteropState {
    katana::interop::ReferenceData reference;
    // The session's named surfaces (terrain/surface_store.hpp): what a
    // geoprocessing line's SURFACE <name> finds, and where an IMPORT puts a
    // 12d archive's.
    katana::terrain::SurfaceStore surfaces;
};

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
    // The geoprocessing executor's view of this session (geo/geo_verbs.hpp):
    // its drawing, interpreter, reference rasters and surfaces.
    katana::app::geo::Context* geo = nullptr;
#endif
};

// Returns false when the command failed.
bool runLine(SessionState& session, const std::string& line)
{
    // A note, whatever blanks come before its '#' - the rule the window's
    // scripts and command line keep (src/katana_qt/script_runner.hpp), so one
    // .kcs runs alike in katana_cli, katana_mcp and File > Run Script. An
    // indented "# note" was once a command here and stopped the script the
    // window ran through.
    if (const std::string_view body = katana::core::trimmed(line);
        !body.empty() && body.front() == '#') {
        return true;
    }
    if (const char* why = windowOnlyVerb(upperVerb(line))) {
        std::cerr << "error: Unsupported: " << upperVerb(line) << ' ' << why << '\n';
        return false;
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
    // A .ifc is read, written and described natively, with or without GDAL
    // (ifc_verbs.hpp). The rest of the line goes as it is, not through
    // argumentOf: the IFC grammar reads its own quotes, and a line may quote
    // more than one path.
    if (const std::string verb = upperVerb(line);
        verb == "IMPORT" || verb == "EXPORT" || verb == "INFO" || verb == "IFC") {
        const std::size_t space = line.find_first_of(" \t", line.find_first_not_of(" \t"));
        if (const std::optional<bool> handled = katana::app::runIfcVerb(
                session.document, verb,
                space == std::string::npos ? std::string_view{}
                                           : std::string_view(line).substr(space))) {
            return *handled;
        }
    }
#if defined(KATANA_WITH_INTEROP)
    // IMPORT, EXPORT, INFO <file>, REFS, COPC, GDAL and the families after
    // them run through the one executor the window runs them through, inline:
    // a session has nobody to wait for a job. A .dxf among them is read and
    // written natively (dxf_verbs.hpp).
    if (session.geo != nullptr && katana::app::geo::handles(line)) {
        const auto reply = katana::app::geo::runNow(*session.geo, line);
        if (!reply) {
            std::cerr << "error: " << reply.error().describe() << '\n';
            return false;
        }
        if (!reply->empty()) {
            std::cout << *reply << '\n';
        }
        return true;
    }
#else
    // Without GDAL, IMPORT and EXPORT are a .dxf's alone, read and written
    // natively by the same steps the executor runs (dxf_verbs.hpp).
    if (const std::string verb = upperVerb(line); verb == "IMPORT" || verb == "EXPORT") {
        const std::size_t space = line.find_first_of(" \t", line.find_first_not_of(" \t"));
        const std::string_view rest =
            space == std::string::npos ? std::string_view{} : std::string_view(line).substr(space);
        using Argument = katana::cad::CommandInterpreter::ImportArgument;
        const katana::core::Result<Argument> argument =
            verb == "IMPORT" ? katana::cad::CommandInterpreter::importArgument(rest)
                             : katana::core::Result<Argument>(
                                   Argument{katana::app::restOfLine(line), {}});
        if (!argument) {
            std::cerr << "error: " << argument.error().describe() << '\n';
            return false;
        }
        if (const std::optional<bool> handled = katana::app::runDxfVerb(
                session.document, verb, argument->path, argument->placement)) {
            return *handled;
        }
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
#if defined(KATANA_WITH_INTEROP)
        // And the reference layers it is worked on top of: their sources and
        // display, read again when it opens (docs/interop.md, "Reference
        // layers").
        if (session.geo != nullptr) {
            katana::app::geo::recordReferences(*session.geo);
        }
#endif
    }
    const auto reply = session.interpreter.run(line);
    if (!reply) {
        // A refusal's first line is what was refused and why. Lines after it
        // are the report behind it - UTILITY CHECK refuses a schedule with
        // errors and carries the whole check - and go where a report goes, so
        // a script that keeps stdout has the check exactly when it failed.
        katana::core::Error refusal = reply.error();
        const std::size_t lineEnd = refusal.message.find('\n');
        const std::string report =
            lineEnd == std::string::npos ? std::string() : refusal.message.substr(lineEnd + 1);
        if (lineEnd != std::string::npos) {
            refusal.message.resize(lineEnd);
        }
        std::cerr << "error: " << refusal.describe() << '\n';
        if (!report.empty()) {
            std::cout << report << '\n';
        }
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
#if defined(KATANA_WITH_INTEROP)
    if ((verb == "NEW" || verb == "OPEN") && session.geo != nullptr) {
        // The reference layers and the surfaces go with their drawing, as
        // the window's do (audit QT-17); an OPEN then reads again the layers
        // the project records. A layer whose file is gone is warned of by
        // the restore, and the drawing is open all the same.
        session.interop.reference.clear();
        session.interop.surfaces.clear();
        if (verb == "OPEN" && katana::app::geo::recordsReferences(*session.geo)) {
            if (const auto restored = katana::app::geo::runNow(*session.geo, "REFS RESTORE")) {
                std::cout << *restored << '\n';
            } else {
                std::cerr << "warning: the reference layers were not read again: "
                          << restored.error().describe() << '\n';
            }
        }
    }
#endif
    return true;
}

} // namespace

namespace katana::app {

struct Session::State {
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
#if defined(KATANA_WITH_INTEROP)
    SessionState session{document, interpreter, {}, {}, {}, nullptr};
    // Derived rasters of a drawing with no project go to a folder of this
    // session's own, so two sessions never find each other's files.
    katana::app::geo::Context geo{document,
                                  interpreter,
                                  session.interop.reference,
                                  session.interop.surfaces,
                                  katana::app::geo::ownScratch(),
                                  {},
                                  {}};
#else
    SessionState session{document, interpreter, {}, {}};
#endif
};

Session::Session(const char* executable) : state_(std::make_unique<State>())
{
#if defined(KATANA_WITH_INTEROP)
    state_->session.geo = &state_->geo;
#endif
    state_->interpreter.setColourLookup(colourOf);
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
            "Customise CUSTOMISE [REPLACE] <file> [<file>...]  load style\n"
            "          libraries (.4d) and survey code files (.mapfile), merged\n"
            "          into what is loaded; CUSTOMISE alone reports what is loaded\n";
    text += katana::app::ifcHelpText();
#if defined(KATANA_WITH_INTEROP)
    // IMPORT, EXPORT, INFO <file>, REFS and COPC are the executor's, so its
    // table says them, as it does in the window's HELP.
    text += katana::app::geo::helpText();
#else
    text += "Interop   IMPORT <file.dxf> [LOCAL | ALONGSIDE | OFFSET=dE,dN] | EXPORT <file.dxf>\n"
            "          (this build has no GDAL: DXF only)\n";
#endif
    text += "Window    PLOT, PLOTSHEETS, SNAPSHOT, ONLINE and SCRIPT are the desktop window's\n"
            "          verbs, refused here: katana --plot, --plot-sheets and --command run\n"
            "          them headless; a script is katana_cli's argument, or katana_mcp's\n"
            "          katana_run_script\n";
    return text;
}

} // namespace katana::app
