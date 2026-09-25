#pragma once

// STATUS [JSON]: the drawing's state at a glance - its project, whether it has
// unsaved changes, what it holds, what is current, the selection, the undo
// history and the customisation loaded (docs/cad.md, "STATUS").
//
// One definition, three front ends. katana_mcp's katana_status tool and its
// katana://status resource were built in mcp_server.cpp alone; the window had
// only pieces of it (the title, the current layer), and nothing an agent
// driving the window or katana_cli could ask for. They all come from here
// now: the interpreter's STATUS verb, the MCP tool and resource, and File >
// Drawing Summary's Copy as JSON.

#include <cstddef>
#include <optional>
#include <string>

#include "katana/cad/document.hpp"

namespace katana::cad {

struct DocumentStatus {
    // The project directory, UTF-8; empty when the drawing has none yet.
    std::optional<std::string> project{};
    bool modified = false;
    std::size_t entities = 0;
    std::size_t layers = 0;
    std::size_t alignments = 0;
    std::string currentLayer{};
    std::string currentStyle{}; // empty: ByLayer
    std::size_t selected = 0;
    std::size_t undoSteps = 0;
    std::size_t redoSteps = 0;
    std::size_t styleLibraryDefinitions = 0;
    std::size_t surveyCodeRules = 0;
};

[[nodiscard]] DocumentStatus documentStatus(const Document& document);

// Five lines of plain text, the last without a line end: what STATUS and
// katana_status print.
[[nodiscard]] std::string formatStatus(const DocumentStatus& status);

// The status as a JSON object, keys in alphabetical order and indented by two,
// with no line end after the closing brace: what STATUS JSON prints and the
// katana://status resource holds. The keys are the fields above, spelt as they
// are there; `project` is null when there is none.
[[nodiscard]] std::string statusJson(const DocumentStatus& status);

} // namespace katana::cad
