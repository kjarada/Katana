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

#include <memory>
#include <string>

#include "katana/cad/document.hpp"

namespace katana::app {

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

  private:
    struct State;
    std::unique_ptr<State> state_;
};

} // namespace katana::app
