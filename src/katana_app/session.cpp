// The command-line session that katana_cli and katana_mcp both drive: one
// Document, the CommandInterpreter over it, and the verbs the front ends add
// above katana_cad (IMPORT, EXPORT, INFO <file>, SURVEY, COPC...). Each line is
// reported on std::cout (what it did) and std::cerr (errors and warnings), as
// katana_cli always has; katana_mcp captures both around each line.

#include <cctype>
#include <iostream>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/customisation_host.hpp"
#include "katana/cad/customisation_record.hpp"
#include "katana/cad/customisation_state.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "dxf_verbs.hpp"
#include "ifc_verbs.hpp"
#include "survey_verbs.hpp"
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
    // They were unknown commands here, although the help said the window's
    // verbs are refused by name. VIEWS and ZOOM are the interpreter's, which
    // refuses them by name itself when no window answers for the views.
    if (verb == "GRID" || verb == "EXAGGERATION") {
        return "is the desktop window's: it sets how the window's views draw, and katana_cli and "
               "katana_mcp have no views; run katana headless with --command";
    }
    return nullptr;
}

// `names`, each in quotes, with commas between.
std::string quotedNames(const std::vector<std::string>& names)
{
    std::string text;
    for (const std::string& name : names) {
        text += (text.empty() ? "\"" : ", \"") + name + "\"";
    }
    return text;
}

// What a program's session is handed for its customisation
// (cad/customisation_host.hpp): the built-in of this run - the one compiled
// in, or what the seam KATANA_BUILTIN_CUSTOMISATION says in its place - and
// the kept file, which here is the one KATANA_CUSTOMISATION names and nothing
// else. No per-user place is read: a command line, and a server an agent
// drives, must do the same thing on every machine and for every user.
//
// The variable is read through core, never getenv: on Windows that gives the
// ANSI code page's bytes, in which a file named outside the code page is a
// file named with '?'. Such a kept file was not found, so not read, with
// nothing said, and CUSTOMISE KEEP then failed to write it
// (core/path_text.hpp).
katana::cad::CustomisationHost hostOfThisRun()
{
    katana::cad::CustomisationHost host;
    host.builtIn = katana::cad::builtInCustomisation();
    if (const std::string kept =
            katana::core::environmentVariable(katana::cad::kKeptCustomisationVariable);
        !kept.empty()) {
        host.keptFile = katana::core::pathFromUtf8(kept);
    }
    return host;
}

// The customisation a program's session starts with, so that it can draw a
// survey rather than wait to be told where its linestyles are: the kept one
// when there is one and it reads, else the built-in, else none - cad's choice
// (startCustomisation), said here in this front end's words. The interpreter
// is handed the host FIRST: it notes the kept file as it is at that moment,
// and CUSTOMISE KEEP refuses to write over one that is another later.
void startWithTheHostsCustomisation(katana::cad::Document& document,
                                    katana::cad::CommandInterpreter& interpreter)
{
    const katana::cad::CustomisationHost host = hostOfThisRun();
    interpreter.setCustomisationHost(host);
    const katana::cad::CustomisationStart start = katana::cad::startCustomisation(document, host);
    // What went wrong is said, and the session starts all the same (audit
    // A12-06): unsaid, a built-in or a kept file that did not read drew every
    // drawing's linestyles as plain lines without a word. Said HERE it reaches
    // whoever reads this program's standard error, which a client of
    // katana_mcp does not: for that reader startCustomisation has left the
    // same sentences on the Document, and CUSTOMISE JSON, katana_customisation
    // and katana://customisation give them as `start`.
    for (const std::string& problem : start.problems) {
        std::cerr << "error: " << problem << "\n";
    }
    if (start.installed == katana::cad::CustomisationOrigin::None) {
        return;
    }
    const bool kept = start.installed == katana::cad::CustomisationOrigin::Kept;
    std::cout << "Customisation: " << start.name << ", " << start.definitions
              << " linestyles and symbols and " << start.rules << " survey code rules, "
              << (kept ? "kept" : "built in") << "\n";
    if (start.keptFromAnotherBuiltIn) {
        // It is the user's, so it is what starts; they may still want to know
        // that the program's own has moved on since they kept theirs.
        std::cerr << "warning: the kept customisation was made from another built-in "
                     "customisation than this program has; CUSTOMISE RESET gives this "
                     "program's\n";
    }
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
    // HELP (or ?) alone is the whole session's help - IFC, SURVEY and the geo
    // executor's families too, which the interpreter cannot know of - so
    // katana_cli -c HELP and katana_run_commands say what katana_help and
    // --help say. With a word (HELP SHEETS, HELP UTILITY, HELP CUSTOMISE) it
    // stays the interpreter's.
    if (const std::string_view body = katana::core::trimmed(line);
        body.find_first_of(" \t") == std::string_view::npos &&
        (upperVerb(line) == "HELP" || upperVerb(line) == "?")) {
        std::cout << katana::app::Session::helpText() << '\n';
        return true;
    }
    if (const char* why = windowOnlyVerb(upperVerb(line))) {
        std::cerr << "error: Unsupported: " << upperVerb(line) << ' ' << why << '\n';
        return false;
    }
    // SURVEY READ and SURVEY IMPORT: a survey field file, through surveyio,
    // which the interpreter (cad) may not see (survey_verbs.hpp).
    if (katana::app::isSurveyLine(line)) {
        const auto reply = katana::app::runSurveyLine(session.document, line);
        if (!reply) {
            std::cerr << "error: " << reply.error().describe() << '\n';
            return false;
        }
        std::cout << *reply << '\n';
        return true;
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
    // CUSTOMISE is the interpreter's, like any other verb below: it was taken
    // here, with a parser of this session's own, while the readers of a
    // customisation lived where katana_cad could not see them. The project's
    // record of what it was drawn with - names, never definitions, which are
    // session data (D1) - is written by the save itself (Document::save), so
    // nothing is done for that here either.
    const std::string verb = upperVerb(line);
#if defined(KATANA_WITH_INTEROP)
    // The reference layers the drawing is worked on top of are recorded here:
    // their sources and display, read again when it opens (docs/interop.md,
    // "Reference layers"). Only for a SAVE that has somewhere to go, as in the
    // window: writing them marks the drawing modified whether or not the save
    // then happens.
    if (verb == "SAVE" &&
        katana::cad::typedSaveHasDestination(line, session.document.hasProject()) &&
        session.geo != nullptr) {
        katana::app::geo::recordReferences(*session.geo);
    }
#endif
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
    if (verb == "OPEN" && katana::cad::CommandInterpreter::replacesDocument(line)) {
        // A warning, not a refusal: the drawing opens and draws, but what a
        // missing customisation defined draws as a plain line. The open
        // worked out which (Document::open); the words are this front end's.
        // A project records its customisations by NAME - the name a file
        // declares, not the file's - so they are not called files here.
        const std::vector<std::string>& missing =
            session.document.customisationState().missingAtOpen;
        if (!missing.empty()) {
            std::cerr << "warning: this project was drawn with customisations that are not "
                         "loaded: "
                      << quotedNames(missing) << "\n";
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
    SessionState session{document, interpreter, {}, nullptr};
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
    SessionState session{document, interpreter};
#endif
};

Session::Session(const char* executable) : state_(std::make_unique<State>())
{
#if defined(KATANA_WITH_INTEROP)
    state_->session.geo = &state_->geo;
#endif
    // No colour callback is handed over: the interpreter resolves a colour
    // name through the Document - the customisation's own table, then the
    // standard names - which is everything this session could have answered.
    if (executable != nullptr) {
        startWithTheHostsCustomisation(state_->document, state_->interpreter);
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
    // The interpreter's help, CUSTOMISE and CODE among it, then a blank line
    // and the verbs this session adds above katana_cad.
    std::string text = katana::cad::CommandInterpreter::helpText();
    text += "\n";
    text += katana::app::ifcHelpText();
    text += katana::app::surveyHelpText();
#if defined(KATANA_WITH_INTEROP)
    // IMPORT, EXPORT, INFO <file>, REFS and COPC are the executor's, so its
    // table says them, as it does in the window's HELP.
    text += katana::app::geo::helpText();
#else
    text += "Interop   IMPORT <file.dxf> [LOCAL | ALONGSIDE | OFFSET=dE,dN] | EXPORT <file.dxf>\n"
            "          (this build has no GDAL: DXF only)\n";
#endif
    text += "Window    PLOT, PLOTSHEETS, SNAPSHOT, ONLINE, SCRIPT, GRID and EXAGGERATION are the\n"
            "          desktop window's verbs, and VIEWS and ZOOM act on its views: all are\n"
            "          refused here. katana --plot, --plot-sheets and --command run them\n"
            "          headless; a script is katana_cli's argument, or katana_mcp's\n"
            "          katana_run_script\n";
    return text;
}

} // namespace katana::app
