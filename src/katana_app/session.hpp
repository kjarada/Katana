#pragma once

// One command-line session: a Document, the CommandInterpreter over it, and
// the verbs the front ends add above katana_cad - CUSTOMISE, CODE, MAPFILE,
// the native DXF IMPORT/EXPORT and, with KATANA_BUILD_IO, the GDAL/PDAL ones.
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
    // Loads the customisation that ships with the program, as a session has
    // always started. `executable` is argv[0], from which customisation kept
    // beside the program is found; nullptr loads none.
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
