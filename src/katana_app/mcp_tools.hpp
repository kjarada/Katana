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

namespace katana::app {
// update_verbs.hpp: everything an UPDATE line needs from the program that runs it.
struct UpdateContext;
} // namespace katana::app

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

// katana_check_update's reply for `context` (mcp_server.cpp): UPDATE CHECK and
// UPDATE STATUS as the verbs answer them (update_verbs.hpp), one line each in
// the text, and in the structured content as the objects `check` and `status`
// - a member for each key=value of the record, a number where the value is
// one - with `warnings` in either when the reply had any. The tool passes the
// running program's context for the Mcp surface, which keeps no per-user
// state; a test passes one over a fake feed. isError when a line could not be
// run at all: a feed that cannot be read is a reply (update=unreachable).
[[nodiscard]] ToolReply checkUpdateReply(const katana::app::UpdateContext& context);

// katana_customisation (mcp_customisation.cpp): the session's customisation
// as structured data - what is loaded, its survey code rules, its definitions
// and the rules' lint. Read from the Document, in every build.
[[nodiscard]] Tool customisationTool();

// The geoprocessing tools (geo/mcp_geo_tools.cpp): katana_gdal_catalogue,
// katana_gdal_describe, katana_gdal_run, and the lanes' after them.
[[nodiscard]] std::vector<Tool> geoTools();

} // namespace katana::app::mcp
