#pragma once

// What an MCP tool is, and the helpers every tool is built from, shared by
// the server's own tools (mcp_server.cpp) and the geoprocessing tools
// (geo/mcp_geo_tools.cpp), so a tool added in another file is the same kind
// of thing and refuses the same way (docs/mcp.md).

#include <functional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "session.hpp"

namespace katana::app::mcp {

using Json = nlohmann::json;

struct ToolReply {
    std::string text;
    Json structured; // null when the tool has nothing but its text
    bool isError = false;
};

struct Tool {
    const char* name;
    const char* title;
    const char* description;
    Json inputSchema;
    Json annotations;
    std::function<ToolReply(Session&, const Json&)> call;
};

// A tool's failure that is the caller's to fix, reported as a tool result
// (isError) so that the model reads it, rather than as a protocol error.
struct ToolRefusal {
    std::string message;
};

struct RunOptions {
    bool stopOnError = true;
    bool discardUnsavedChanges = false;
};

[[nodiscard]] Json objectSchema(Json properties, std::vector<std::string> required = {});
[[nodiscard]] Json hints(bool readOnly, bool destructive, bool idempotent, bool openWorld = false);

// One line through the session: its transcript, and the drawing's state after it.
[[nodiscard]] ToolReply oneLine(Session& session, const std::string& line,
                                const RunOptions& options = {});

// One line through the session, and what it printed: what a tool that turns
// a line's records into structured content reads.
struct LineOutcome {
    bool ok = false;
    std::string output;   // std::cout
    std::string messages; // std::cerr
};
[[nodiscard]] LineOutcome runCaptured(Session& session, const std::string& line);

// A path as the interpreter reads it: one quoted word. ToolRefusal for a
// quote or a line break, which no line can say.
[[nodiscard]] std::string quoted(const std::string& path);

[[nodiscard]] const Json& argument(const Json& arguments, const char* name);
[[nodiscard]] std::string requiredString(const Json& arguments, const char* name);
[[nodiscard]] bool optionalBool(const Json& arguments, const char* name, bool fallback);

// A scope as the one grammar's words (cad::formatScopeWords), from a tool's
// JSON: `kind` selection, drawing, area (with `area` [x0, y0, x1, y1]) or
// layers (with `layers` ["a", "b"] and `only`), and `where` a list of
// conditions ["TYPE=polyline", "PROP=owner:Smith*"] (null for none). VIEW is
// the window's and is refused. ToolRefusal naming what is wrong. The ONE
// reading of a scope in JSON: katana_gdal_run's sources, katana_gis_query,
// katana_import and katana_export all say a scope through it.
[[nodiscard]] std::string scopeWordsOf(const std::string& kind, const Json& area,
                                       const Json& layers, bool only, const Json& where);

// The geoprocessing tools (geo/mcp_geo_tools.cpp): katana_gdal_catalogue,
// katana_gdal_describe, katana_gdal_run, and the lanes' after them.
[[nodiscard]] std::vector<Tool> geoTools();

} // namespace katana::app::mcp
