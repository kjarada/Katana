// The command-line session that katana_cli and katana_mcp both drive: one
// Document, the CommandInterpreter over it, and the verbs the front ends add
// above katana_cad (IMPORT, EXPORT, INFO <file>, SURVEY, COPC...). Each line is
// reported on std::cout (what it did) and std::cerr (errors and warnings), as
// katana_cli always has; katana_mcp captures both around each line.

#include <cctype>
#include <cstddef>
#include <cstdint>
#include <exception>
#include <filesystem>
#include <functional>
#include <iostream>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/customisation_host.hpp"
#include "katana/cad/customisation_record.hpp"
#include "katana/cad/customisation_state.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/update/install.hpp"
#include "katana/update/recover.hpp"
#include "dxf_verbs.hpp"
#include "ifc_verbs.hpp"
#include "survey_verbs.hpp"
#include "import_records.hpp"
#include "session.hpp"
#include "update_verbs.hpp"

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

// The line's words in capitals with one blank between them, so that a line of
// two words is compared as one text however it was typed.
std::string upperWords(const std::string& line)
{
    std::string words;
    bool gap = false;
    for (const char ch : line) {
        if (ch == ' ' || ch == '\t') {
            gap = !words.empty();
            continue;
        }
        if (gap) {
            words += ' ';
            gap = false;
        }
        words += static_cast<char>(std::toupper(static_cast<unsigned char>(ch)));
    }
    return words;
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

// `count` and the noun for it. The start-up line gave every count the plural:
// a customisation of one definition and one rule had "1 linestyles and
// symbols and 1 survey code rules", where the window, for the same start,
// says "1 definition" and "1 survey code rule".
std::string counted(std::size_t count, const char* one, const char* many)
{
    return std::to_string(count) + ' ' + (count == 1 ? one : many);
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
    // One definition is a linestyle OR a symbol; several are linestyles and
    // symbols, whichever they are (CustomisationStart::definitions counts both).
    std::cout << "Customisation: " << start.name << ", "
              << counted(start.definitions, "linestyle or symbol", "linestyles and symbols")
              << " and " << counted(start.rules, "survey code rule", "survey code rules") << ", "
              << (kept ? "kept" : "built in") << "\n";
    if (start.keptFromAnotherBuiltIn) {
        // It is the user's, so it is what starts; they may still want to know
        // that the program's own has moved on since they kept theirs.
        std::cerr << "warning: the kept customisation was made from another built-in "
                     "customisation than this program has; CUSTOMISE RESET gives this "
                     "program's and CUSTOMISE BUILTIN lists the built-in customisations it "
                     "has\n";
    }
}

#if defined(KATANA_WITH_INTEROP)

// What the geoprocessing executor's verbs - IMPORT, EXPORT, INFO <file>, REFS,
// COPC, GDAL and the families after them (geo/geo_verbs.hpp) - keep beside
// the drawing. They live above katana_cad, which must not see GDAL or PDAL:
// that separation is what lets katana_cad build with -DKATANA_BUILD_IO=OFF.
struct InteropState {
    katana::interop::ReferenceData reference;
    // The surfaces are not here: they are the drawing's (Document::surfaces),
    // saved with it and undone with it (docs/terrain.md).
};

#endif // KATANA_WITH_INTEROP

// What the UPDATE verbs (update_verbs.hpp) need of a session. They act on the
// program that runs it, not on its drawing.
struct UpdatePart {
    // A program's session (session.hpp): there is a program for UPDATE to act on.
    bool program = false;
    // Whose lines these are: katana_cli's, unless katana_mcp's server said Mcp.
    katana::app::UpdateSurface surface = katana::app::UpdateSurface::Cli;
    // What an UPDATE APPLY CONFIRM left for the front end to launch once the
    // session has ended (Session::takePendingApply).
    std::optional<katana::update::ApplyRequest> pendingApply;
    // Where a line's context comes from when it is not the running program's
    // (Session::setUpdateContextSource). Empty: makeUpdateContext.
    std::function<katana::app::UpdateContext(katana::app::UpdateSurface)> source;
};

// One session's mutable state. Grouped so that adding a verb that needs more
// context does not mean changing every signature again.
struct SessionState {
    katana::cad::Document& document;
    katana::cad::CommandInterpreter& interpreter;
#if defined(KATANA_WITH_INTEROP)
    InteropState interop;
    // The geoprocessing executor's view of this session (geo/geo_verbs.hpp):
    // its drawing (with its surfaces), interpreter and reference rasters.
    katana::app::geo::Context* geo = nullptr;
#endif
    UpdatePart updates;
    // Whether Session::run has said that it read a line as Windows-1252: once a
    // session, not once a line, or a script saved in the ANSI code page would
    // say it on every line.
    bool saidWindows1252 = false;
};

// This process, which the update helper waits for before it swaps a file.
std::uint64_t ownProcessId()
{
#if defined(_WIN32)
    return static_cast<std::uint64_t>(_getpid());
#else
    return static_cast<std::uint64_t>(getpid());
#endif
}

// An UPDATE line: the verbs' reply on std::cout, a refusal on std::cerr.
// Returns false when the line was refused or failed.
bool runUpdate(UpdatePart& updates, const std::string& line)
{
    namespace app = katana::app;
    if (updates.surface == app::UpdateSurface::Mcp) {
        // Refused by name, as the window's own verbs are, and whatever the
        // line says: katana_run_commands promises its client that it reaches
        // no network (docs/mcp.md), and an agent that could download or apply
        // would replace the very program its client holds open.
        std::cerr << "error: Unsupported: UPDATE lines are not run by katana_mcp: its tool "
                     "katana_check_update checks for a newer Katana and reports this copy's "
                     "update status, and it downloads and installs nothing; the Katana window "
                     "or katana_cli does that\n";
        return false;
    }
    if (!updates.program) {
        std::cerr << "error: Unsupported: UPDATE acts on the program that runs it, and this "
                     "session is no program's (session.hpp)\n";
        return false;
    }
    // Made for each line: it reads the install's records and the stored token
    // as they are now, so a line sees what the line before it changed.
    app::UpdateContext context = updates.source ? updates.source(updates.surface)
                                                : app::makeUpdateContext(updates.surface);
    context.pid = ownProcessId();
    // `relaunch` is left as it came, which from makeUpdateContext is empty:
    // katana_cli starts nothing again after an update.
    // A download is watched, so its progress is printed as it happens; the
    // same lines are in the reply, and are not printed a second time.
    std::vector<std::string> shown;
    context.progressLine = [&shown](std::string_view progress) {
        std::cout << progress << std::endl;
        shown.emplace_back(progress);
    };
    const auto reply = app::runUpdateLine(context, line);
    if (!reply) {
        std::cerr << "error: " << reply.error().describe() << '\n';
        return false;
    }
    std::size_t matched = 0;
    for (std::string_view rest = reply->text; !rest.empty();) {
        const std::size_t end = rest.find('\n');
        const std::string_view one = rest.substr(0, end);
        if (matched < shown.size() && one == shown[matched]) {
            ++matched;
        } else {
            std::cout << one << '\n';
        }
        if (end == std::string_view::npos) {
            break;
        }
        rest.remove_prefix(end + 1);
    }
    if (reply->apply.has_value()) {
        // One apply is prepared at a time. An earlier one that nobody took is
        // nobody's now: its helper copy goes to the sweep (abandonApply).
        if (updates.pendingApply.has_value()) {
            (void)app::abandonApply(*updates.pendingApply);
        }
        updates.pendingApply = reply->apply;
    }
    return true;
}

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
    // HELP UPDATE is the session's as well: the update verbs are taken here,
    // so the interpreter, which answers HELP with a word, has no such word.
    if (const std::string words = upperWords(line);
        words == "HELP UPDATE" || words == "? UPDATE") {
        std::cout << katana::app::updateHelpText();
        return true;
    }
    if (const char* why = windowOnlyVerb(upperVerb(line))) {
        std::cerr << "error: Unsupported: " << upperVerb(line) << ' ' << why << '\n';
        return false;
    }
    // UPDATE CHECK, STATUS, DOWNLOAD, APPLY and the rest: the program's own
    // update, through the engine and the network, which the interpreter (cad)
    // sees neither of (update_verbs.hpp).
    if (katana::app::isUpdateLine(line)) {
        return runUpdate(session.updates, line);
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
                      << quotedNames(missing);
            // A name that is one of this program's built-ins is no file to
            // load: it is made the session by name.
            const std::string advice =
                katana::cad::builtInAdviceFor(missing, katana::cad::builtIns());
            if (!advice.empty()) {
                std::cerr << "; " << advice;
            }
            std::cerr << "\n";
        }
    }
#if defined(KATANA_WITH_INTEROP)
    if ((verb == "NEW" || verb == "OPEN") && session.geo != nullptr) {
        // The reference layers go with their drawing, as the window's do
        // (audit QT-17); an OPEN then reads again the layers the project
        // records. A layer whose file is gone is warned of by the restore,
        // and the drawing is open all the same. (The surfaces are the
        // drawing's: New cleared them, Open read the project's.)
        session.interop.reference.clear();
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
    SessionState session{document, interpreter, {}, nullptr, {}};
    // Derived rasters of a drawing with no project go to a folder of this
    // session's own, so two sessions never find each other's files.
    katana::app::geo::Context geo{document,
                                  interpreter,
                                  session.interop.reference,
                                  katana::app::geo::ownScratch(),
                                  {},
                                  {}};
#else
    SessionState session{document, interpreter, {}};
#endif
};

Session::Session(const char* executable) : state_(std::make_unique<State>())
{
#if defined(KATANA_WITH_INTEROP)
    state_->session.geo = &state_->geo;
#endif
    // A session of no program has nothing for UPDATE to act on (session.hpp).
    state_->session.updates.program = executable != nullptr;
    // No colour callback is handed over: the interpreter resolves a colour
    // name through the Document - the customisation's own table, then the
    // standard names - which is everything this session could have answered.
    if (executable != nullptr) {
        startWithTheHostsCustomisation(state_->document, state_->interpreter);
    }
}

Session::~Session()
{
    // A prepared apply that nobody took before the session ended will not be
    // launched: the helper copy it had made is listed for the sweep
    // (update_verbs.hpp, abandonApply), and the release stays downloaded.
    // Nothing is said if that fails, and nothing may leave a destructor: the
    // list's path is turned into text on the way, which the standard library
    // does by throwing when it cannot.
    if (const std::optional<katana::update::ApplyRequest>& pending =
            state_->session.updates.pendingApply) {
        try {
            (void)abandonApply(*pending);
        } catch (...) {
            // Nothing to say and nobody to say it to: the copy stays.
        }
    }
}

bool Session::run(const std::string& line)
{
    SessionState& session = state_->session;
    try {
#if defined(_WIN32)
        // Only here: the bytes a Windows program is handed are the ANSI code
        // page's, and Windows-1252 is the reading of them that decodeText also
        // falls back on. Elsewhere a line's bytes are what the person's
        // terminal and the file system say, and core::pathFromUtf8 takes a path
        // that is not UTF-8 as the narrow name it is - a file named in Latin-1
        // stays reachable - while a NAME that is not UTF-8 is refused by the
        // door of its own table, with a sentence.
        if (!katana::core::isValidUtf8(line)) {
            if (!session.saidWindows1252) {
                session.saidWindows1252 = true;
                std::cerr << "note: a line that is not UTF-8 text was read as Windows-1252 (the "
                             "ANSI code page); Katana's text is UTF-8, so save a script as UTF-8 "
                             "to say it plainly\n";
            }
            return runLine(session, katana::core::utf8OrWindows1252(line));
        }
#endif
        return runLine(session, line);
    } catch (const std::exception& error) {
        // Nothing up a verb's call chain catches: the standard library throws
        // for a path it cannot convert, nlohmann for text it cannot write, and
        // an exception out of main ends the process - in katana_cli, with the
        // drawing unsaved. The line fails, says why, and the session goes on.
        std::cerr << "error: Internal: " << error.what() << '\n';
        return false;
    }
}

katana::core::Result<std::vector<std::string>> Session::readScript(std::string_view path)
{
    auto bytes = katana::core::readFileBytes(katana::core::pathFromUtf8(path));
    if (!bytes) {
        return bytes.error();
    }
    auto decoded = katana::core::decodeText(*bytes);
    if (!decoded) {
        return katana::core::makeError(decoded.error().code, decoded.error().message,
                                       std::string(path));
    }
    // The fallback is said, never silent (text_encoding.hpp): a degree sign
    // read under the wrong code page is a wrong character, not an error.
    if (decoded->guessed) {
        std::cerr << "note: the script " << path << " is not UTF-8 text; read as "
                  << katana::core::toString(decoded->encoding) << '\n';
    }
    std::vector<std::string> lines;
    for (const std::string_view one : katana::core::splitLines(decoded->text)) {
        lines.emplace_back(one);
    }
    return lines;
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

void Session::setUpdateSurface(UpdateSurface surface)
{
    state_->session.updates.surface = surface;
}

void Session::setUpdateContextSource(std::function<UpdateContext(UpdateSurface)> source)
{
    state_->session.updates.source = std::move(source);
}

bool Session::isUpdateApply(const std::string& line)
{
    // The verbs' own splitter, not this file's: a line that the verbs would
    // read as UPDATE APPLY - its words parted by a carriage return, say, as a
    // -c argument can hold one - is the line that must be last.
    return isUpdateApplyLine(line);
}

std::optional<katana::update::ApplyRequest> Session::takePendingApply()
{
    return std::exchange(state_->session.updates.pendingApply, std::nullopt);
}

void recoverInterruptedUpdate()
{
    const katana::core::Result<std::filesystem::path> root =
        katana::update::installRootOfRunningProgram();
    if (!root) {
        return; // in no install: there is nothing an update could have left half done
    }
    if (const katana::core::Status recovered = katana::update::recover(*root); !recovered) {
        // Said, and the program goes on: the message names the command that
        // puts the install right by hand (recover.hpp).
        std::cerr << "warning: " << recovered.error().describe() << '\n';
    }
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
    // The program's own update, which the window's HELP lists too.
    text += katana::app::updateHelpText();
    text += "Window    PLOT, PLOTSHEETS, SNAPSHOT, ONLINE, SCRIPT, GRID and EXAGGERATION are the\n"
            "          desktop window's verbs, and VIEWS and ZOOM act on its views: all are\n"
            "          refused here. katana --plot, --plot-sheets and --command run them\n"
            "          headless; a script is katana_cli's argument, or katana_mcp's\n"
            "          katana_run_script\n";
    return text;
}

} // namespace katana::app
