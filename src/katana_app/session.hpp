#pragma once

// One command-line session: a Document, the CommandInterpreter over it, and
// the verbs the front ends add above katana_cad - the native DXF and IFC
// IMPORT/EXPORT, SURVEY and, with KATANA_BUILD_IO, the GDAL/PDAL ones. (CODE
// and CUSTOMISE are the interpreter's own; a session only hands it the host
// CUSTOMISE RESET, KEEP and REVERT need.)
//
// katana_cli and katana_mcp both drive it, so a line typed at the command line
// and a line sent by Claude over MCP (docs/mcp.md) do exactly the same thing.
//
// Each line is reported on std::cout (what it did) and std::cerr (errors and
// warnings). katana_cli lets both through; katana_mcp captures them around
// each line and hands them back to the client.
//
// The UPDATE verbs (update_verbs.hpp) are taken here too, and act on the
// PROGRAM that runs the session, not on its drawing: katana_cli runs them
// all, with UPDATE APPLY CONFIRM as the last line of its run
// (takePendingApply); katana_mcp runs none, and says which of its tools
// checks for an update (setUpdateSurface).

#include <functional>
#include <memory>
#include <optional>
#include <string>

#include "katana/cad/document.hpp"
#include "katana/update/apply.hpp"

namespace katana::app {

// update_verbs.hpp, which a front end that only runs lines need not read.
enum class UpdateSurface;
struct UpdateContext;

// What katana_cli's and katana_mcp's main() do before anything else:
// update::recover for the install the running program is in, so that an
// update a crash or a power cut interrupted is put back before the program
// reads a file of a half-swapped install (include/katana/update/recover.hpp).
// With nothing interrupted - every start but that one - it is the listing of
// one folder, and nothing at all for a program that is in no install (a build
// tree has no .update folder). A headless program writes nothing per user
// here: the recovery's only record is the install's own log. A recovery that
// was needed and did not complete is said on std::cerr with the command that
// puts the install right by hand, and the program goes on.
void recoverInterruptedUpdate();

class Session {
  public:
    // A PROGRAM's session (`executable` is its argv[0]) starts with a
    // customisation, and says which on std::cout: the kept one when the
    // environment variable KATANA_CUSTOMISATION names a file that reads, else
    // the program's built-in one - which KATANA_BUILTIN_CUSTOMISATION can
    // replace for one run, or switch off with `none` - else none
    // (cad/customisation_host.hpp). What could not be read is said on
    // std::cerr and the session starts all the same; it also stays in the
    // session's report (CUSTOMISE JSON, `start`), for a reader that is never
    // shown std::cerr. Both variables are read as Unicode on every platform
    // (core::environmentVariable), so a file may sit under a folder named in
    // any script.
    //
    // nullptr is a session of no program - a test's, a tool's: it is handed
    // no host, reads neither variable and starts EMPTY, whatever this build
    // compiled in, so what it does is the same on every machine. CUSTOMISE
    // RESET, KEEP and REVERT are refused there by name.
    //
    // Only whether `executable` is null is read: nothing is looked for beside
    // the program any more. That search was a third way a session could
    // start, decided by what happened to lie in a folder.
    explicit Session(const char* executable);
    ~Session();
    Session(const Session&) = delete;
    Session& operator=(const Session&) = delete;

    // Runs one line. False when it failed; either way what happened has been
    // written to std::cout and std::cerr. '#' comments and empty lines succeed.
    bool run(const std::string& line);

    [[nodiscard]] katana::cad::Document& document();
    [[nodiscard]] const katana::cad::Document& document() const;

    // QUIT and EXIT, in any case: a front end's cue to stop, never run.
    [[nodiscard]] static bool isQuit(const std::string& line);

    // The interpreter's HELP and the verbs a session adds to it.
    [[nodiscard]] static std::string helpText();

    // Which front end runs this session's UPDATE lines (update_verbs.hpp):
    // katana_cli's unless told, which runs them all. katana_mcp's server says
    // Mcp, and every UPDATE line is then refused naming its tool,
    // katana_check_update - as the window's own verbs are refused, by name and
    // saying where they run - so that the tool that runs lines reaches no
    // network and an agent never downloads or applies. A session of no
    // program (the constructor's nullptr) refuses them whatever it is told:
    // there is no program for them to act on.
    void setUpdateSurface(UpdateSurface surface);

    // Where this session's UPDATE lines get their context: makeUpdateContext
    // (update_verbs.hpp) of the running program, unless one is handed over
    // here. It is called for every line with the session's surface, as
    // makeUpdateContext is, so a line sees what the line before it changed.
    // To what it returns the session adds what is its own to say: the
    // process the helper waits for (`pid`, this one) and the printing of a
    // download's progress; `relaunch` it leaves as it comes, which from
    // makeUpdateContext is empty - katana_cli starts nothing again. A test
    // hands over a scratch install and a fake feed, and so runs what
    // katana_cli runs. An empty function puts the running program's back.
    void setUpdateContextSource(std::function<UpdateContext(UpdateSurface)> source);

    // True for an UPDATE APPLY line, CONFIRM or not: the line a front end
    // must run LAST, since a session that has run it is over. The verbs' own
    // reading of the line (update_verbs.hpp, isUpdateApplyLine).
    [[nodiscard]] static bool isUpdateApply(const std::string& line);

    // What an UPDATE APPLY CONFIRM line that succeeded left to be done, once:
    // the staged release is verified and its helper has proved it starts, and
    // NOTHING in the install has changed. The front end ends the session
    // first - this object destroyed, its drawing closed - and only then
    // calls update::launchApply with it; the helper waits for the process to
    // exit. nullopt when no such line has succeeded since the last call. A
    // front end that does not take it, or does not launch, leaves the release
    // downloaded and the install as it was. The helper's copy that the line
    // made is then nobody's: a front end that took the request and does not
    // launch it calls abandonApply (update_verbs.hpp), and a request nobody
    // took is abandoned by the session itself, when a later one takes its
    // place or when the session ends.
    [[nodiscard]] std::optional<katana::update::ApplyRequest> takePendingApply();

  private:
    struct State;
    std::unique_ptr<State> state_;
};

} // namespace katana::app
