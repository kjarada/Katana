// The MCP server's messages and tools (mcp_server.hpp, docs/mcp.md).
//
// Every tool is a thin shape over Session::run: a tool builds command lines,
// runs them, and hands back what the session printed. So there is one
// implementation of every verb, the one katana_cli and its tests exercise, and
// a tool can never do something the command line cannot.

#include "mcp_server.hpp"
#include "mcp_tools.hpp"

#include <algorithm>
#include <array>
#include <cctype>
#include <cmath>
#include <exception>
#include <fstream>
#include <functional>
#include <iostream>
#include <map>
#include <sstream>
#include <string>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "katana/cad/document.hpp"
#include "katana/cad/document_status.hpp"
#include "katana/cad/import_placement.hpp"
#include "katana/entity/model.hpp"

namespace katana::app::mcp {

namespace {

// Oldest first; kLatestProtocolVersion is the last.
constexpr std::array<std::string_view, 3> kSupportedProtocolVersions{"2024-11-05", "2025-03-26",
                                                                     kLatestProtocolVersion};

// JSON-RPC 2.0's own error codes.
constexpr int kParseError = -32700;
constexpr int kInvalidRequest = -32600;
constexpr int kMethodNotFound = -32601;
constexpr int kInvalidParams = -32602;
constexpr int kInternalError = -32603;

// A script or a batch longer than this is refused rather than run: a client
// that sends a million lines has a bug, and the reply would be unreadable.
constexpr std::size_t kMostLines = 10000;

const char* const kInstructions =
    "Katana is a civil engineering and surveying CAD engine. These tools drive one live "
    "Katana session: a drawing (the Document) that is empty until you open a project "
    "directory or draw into it. Every change is an undoable command, exactly as typed at "
    "Katana's command line. Start with katana_status; katana_help lists every command and "
    "its syntax. Points are x,y (easting,northing), @dx,dy relative or @dist<angle polar; "
    "angles in degrees. Most edits act on the selection, so SELECT first. Nothing is "
    "written to disk until katana_save_project (or SAVE): open a COPY of a project you "
    "care about, since opening can back up or migrate it in place.";

// ---- capturing what a line prints ---------------------------------------------------

// Swaps std::cout and std::cerr (and std::clog, which shares cerr's device) for
// strings while it lives. The session reports on them; the protocol has its
// own stream (mcp_main.cpp), so nothing a verb prints can corrupt a message.
class CaptureStreams {
  public:
    CaptureStreams()
        : cout_(std::cout.rdbuf(out_.rdbuf())), cerr_(std::cerr.rdbuf(err_.rdbuf())),
          clog_(std::clog.rdbuf(err_.rdbuf()))
    {
    }
    ~CaptureStreams()
    {
        std::cout.flush();
        std::cerr.flush();
        std::cout.rdbuf(cout_);
        std::cerr.rdbuf(cerr_);
        std::clog.rdbuf(clog_);
    }
    CaptureStreams(const CaptureStreams&) = delete;
    CaptureStreams& operator=(const CaptureStreams&) = delete;

    [[nodiscard]] std::string out() const { return out_.str(); }
    [[nodiscard]] std::string err() const { return err_.str(); }

  private:
    std::ostringstream out_;
    std::ostringstream err_;
    std::streambuf* cout_;
    std::streambuf* cerr_;
    std::streambuf* clog_;
};

std::string trimmed(std::string text)
{
    while (!text.empty() && (text.back() == '\n' || text.back() == '\r' || text.back() == ' ')) {
        text.pop_back();
    }
    return text;
}

std::string upperFirstWord(std::string_view line)
{
    std::string word;
    for (const char ch : line) {
        if (ch == ' ' || ch == '\t') {
            if (!word.empty()) {
                break;
            }
            continue;
        }
        word += static_cast<char>(std::toupper(static_cast<unsigned char>(ch)));
    }
    return word;
}

// ---- running lines ------------------------------------------------------------------

struct LineResult {
    std::string command;
    bool ok = true;
    bool skipped = false;
    std::string output;   // what the line printed on std::cout
    std::string messages; // and on std::cerr: errors and warnings
};

struct Batch {
    std::vector<LineResult> lines;
    bool ok = true;
};

Batch runLines(Session& session, const std::vector<std::string>& lines, const RunOptions& options)
{
    Batch batch;
    bool stopped = false;
    for (const std::string& raw : lines) {
        std::string line = raw;
        if (!line.empty() && line.back() == '\r') {
            line.pop_back();
        }
        LineResult result;
        result.command = line;
        const std::string verb = upperFirstWord(line);
        if (stopped) {
            result.skipped = true;
        } else if (Session::isQuit(line)) {
            // The client ends the session by closing it, not by a verb that
            // would leave the server running with nothing to serve.
            result.skipped = true;
            result.messages = "QUIT is not a command here: the MCP client ends the session.";
        } else if ((verb == "NEW" || verb == "OPEN") && session.document().isModified() &&
                   !options.discardUnsavedChanges) {
            // The interpreter replaces the drawing without asking, as a typed
            // NEW always has. A model is not a person looking at the window,
            // so it is asked to say so.
            result.ok = false;
            result.messages = "error: the drawing has unsaved changes that " + verb +
                              " would discard. Save first (katana_save_project), or pass "
                              "discard_unsaved_changes: true.";
        } else {
            CaptureStreams capture;
            try {
                result.ok = session.run(line);
            } catch (const std::exception& error) {
                result.ok = false;
                std::cerr << "error: " << error.what() << '\n';
            }
            result.output = trimmed(capture.out());
            result.messages = trimmed(capture.err());
        }
        if (!result.ok) {
            batch.ok = false;
            stopped = options.stopOnError;
        }
        batch.lines.push_back(std::move(result));
    }
    return batch;
}

std::string transcriptOf(const Batch& batch)
{
    std::string text;
    for (const LineResult& line : batch.lines) {
        if (line.command.empty() && line.output.empty() && line.messages.empty()) {
            continue;
        }
        if (!text.empty()) {
            text += '\n';
        }
        text += "> " + line.command + '\n';
        if (line.skipped && line.messages.empty()) {
            text += "(not run: an earlier command failed)\n";
            continue;
        }
        if (!line.output.empty()) {
            text += line.output + '\n';
        }
        if (!line.messages.empty()) {
            text += line.messages + '\n';
        }
    }
    return trimmed(text);
}

Json linesJson(const Batch& batch)
{
    Json lines = Json::array();
    for (const LineResult& line : batch.lines) {
        Json entry{{"command", line.command}, {"ok", line.ok && !line.skipped}};
        if (line.skipped) {
            entry["skipped"] = true;
        }
        if (!line.output.empty()) {
            entry["output"] = line.output;
        }
        if (!line.messages.empty()) {
            entry["messages"] = line.messages;
        }
        lines.push_back(std::move(entry));
    }
    return lines;
}

// ---- tool arguments -----------------------------------------------------------------

// One line per command: a command with a line break in it would be two, and
// the second would run unseen by whoever built the first.
std::string singleLine(const std::string& text, const char* what)
{
    if (text.find('\n') != std::string::npos) {
        throw ToolRefusal{std::string(what) + " must be one line; send several as a list"};
    }
    return text;
}

// katana_import's placement, local and offsets as the word its IMPORT line
// ends in ("" to keep the data where it is), by cad::placementWord, so the
// line is the one a person would type.
std::string importPlacementWord(const Json& arguments)
{
    using katana::cad::ImportPlacementMode;
    const bool local = optionalBool(arguments, "local", false);
    const Json& chosen = argument(arguments, "placement");
    if (chosen.is_null()) {
        return local ? "LOCAL" : "";
    }
    static const std::map<std::string, ImportPlacementMode> kModes{
        {"keep", ImportPlacementMode::Keep},
        {"local", ImportPlacementMode::Local},
        {"alongside", ImportPlacementMode::Alongside},
        {"offset", ImportPlacementMode::Offset}};
    const auto mode = chosen.is_string() ? kModes.find(chosen.get<std::string>()) : kModes.end();
    if (mode == kModes.end()) {
        throw ToolRefusal{"\"placement\" must be keep, local, alongside or offset"};
    }
    if (local && mode->second != ImportPlacementMode::Local) {
        throw ToolRefusal{"\"local\": true and \"placement\": \"" + mode->first +
                          "\" say different things; give one"};
    }
    katana::cad::ImportPlacement placement{mode->second, {}};
    if (mode->second == ImportPlacementMode::Offset) {
        const auto number = [&](const char* name) {
            const Json& value = argument(arguments, name);
            if (!value.is_number() || !std::isfinite(value.get<double>())) {
                throw ToolRefusal{std::string("placement offset needs \"") + name +
                                  "\", a number"};
            }
            return value.get<double>();
        };
        placement.offset = katana::geometry::Vec2(number("offset_east"), number("offset_north"));
    }
    return katana::cad::placementWord(placement);
}

// ---- the session's state ------------------------------------------------------------

// The drawing's state, as STATUS JSON gives it (cad/document_status.hpp): one
// definition for this server, katana_cli and the window.
Json statusOf(const Session& session)
{
    return Json::parse(
        katana::cad::statusJson(katana::cad::documentStatus(session.document())));
}

// ---- tools --------------------------------------------------------------------------

const Json kDiscardProperty{
    {"type", "boolean"},
    {"description", "Allow NEW or OPEN to throw away unsaved changes to the current drawing. "
                    "Default false: they are refused while there are unsaved changes."}};

// The reply of a tool that is a batch of lines, with the state after it.
ToolReply batchReply(Session& session, const std::vector<std::string>& lines,
                     const RunOptions& options)
{
    const Batch batch = runLines(session, lines, options);
    const Json status = statusOf(session);
    std::string text = transcriptOf(batch);
    if (text.empty()) {
        text = "(nothing to run)";
    }
    return {text, Json{{"ok", batch.ok}, {"commands", linesJson(batch)}, {"status", status}},
            !batch.ok};
}

std::vector<std::string> stringList(const Json& value, const char* name)
{
    if (!value.is_array() || value.empty()) {
        throw ToolRefusal{std::string("\"") + name + "\" must be a non-empty list of strings"};
    }
    if (value.size() > kMostLines) {
        throw ToolRefusal{std::string("\"") + name + "\" has more than " +
                          std::to_string(kMostLines) + " entries; send them in parts"};
    }
    std::vector<std::string> lines;
    for (const Json& item : value) {
        if (!item.is_string()) {
            throw ToolRefusal{std::string("every entry of \"") + name + "\" must be a string"};
        }
        lines.push_back(singleLine(item.get<std::string>(), "each command"));
    }
    return lines;
}

const std::vector<Tool>& tools()
{
    static const std::vector<Tool> kTools = [] {
        std::vector<Tool> list;

        list.push_back(Tool{
            "katana_run_commands", "Run Katana commands",
            "Run one or more Katana command-line commands, in order, against the live drawing - "
            "the same commands a person types at Katana's command line (katana_help lists them): "
            "draw (LINE, PLINE, RECT, CIRCLE, ARC, POINT, TEXT, DIM), select and modify (SELECT, "
            "MOVE, COPY, ROTATE, OFFSET, TRIM, FILLET...), layers and styles, alignments and "
            "design profiles (ALIGN), parcels (PARCEL), survey calculations (INVERSE, FORWARD, "
            "AREA), survey codes (CODE, MAPFILE, CUSTOMISE), AS 5488 subsurface utilities "
            "(UTILITY REPORT, VERIFY, CLEARANCE, CHECK, DRAW, REGRADE, SCHEDULE - on a schedule "
            "file, or on what is drawn by the scope words DRAWING | SELECTION | "
            "AREA x0,y0,x1,y1 | LAYERS a,b [ONLY], then [WHERE key=value ...]; HELP UTILITY), "
            "global modify (MODIFY <scope> [WHERE key=value ...] SET key=value ...), "
            "inspection (LIST, INFO), UNDO/REDO, "
            "and files (NEW, OPEN, SAVE, IMPORT, EXPORT). Each command is one undoable step. By "
            "default the batch stops at the first command that fails. Returns each command's "
            "output and the drawing's status afterwards. PLOT, PLOTSHEETS, SNAPSHOT, ONLINE and "
            "SCRIPT are the desktop window's verbs and are refused here, saying so: plotting, "
            "view pictures and online data need the window (or katana run headless); a script "
            "file is katana_run_script's.",
            objectSchema(
                Json{{"commands",
                      {{"type", "array"},
                       {"items", {{"type", "string"}}},
                       {"minItems", 1},
                       {"description", "Commands, one per entry, e.g. [\"LAYER NEW Kerb #FF0000\", "
                                       "\"LAYER SET Kerb\", \"PLINE 0,0 30,0 30,20 CLOSE\"]"}}},
                     {"stop_on_error",
                      {{"type", "boolean"},
                       {"description", "Stop at the first failing command (default true)."}}},
                     {"discard_unsaved_changes", kDiscardProperty}},
                {"commands"}),
            hints(false, true, false), [](Session& session, const Json& arguments) {
                return batchReply(session, stringList(argument(arguments, "commands"), "commands"),
                                  {.stopOnError = optionalBool(arguments, "stop_on_error", true),
                                   .discardUnsavedChanges =
                                       optionalBool(arguments, "discard_unsaved_changes", false)});
            }});

        list.push_back(Tool{
            "katana_run_script", "Run a Katana script file",
            "Run a Katana command script (.kcs): one command per line, a line whose first "
            "non-blank is '#' a comment - the file katana_cli runs. Stops at the first failing "
            "line unless stop_on_error is false.",
            objectSchema(
                Json{{"path", {{"type", "string"}, {"description", "The script file to run."}}},
                     {"stop_on_error",
                      {{"type", "boolean"},
                       {"description", "Stop at the first failing line (default true)."}}},
                     {"discard_unsaved_changes", kDiscardProperty}},
                {"path"}),
            hints(false, true, false), [](Session& session, const Json& arguments) {
                const std::string path = requiredString(arguments, "path");
                std::ifstream script(path);
                if (!script) {
                    throw ToolRefusal{"cannot read the script " + path};
                }
                std::vector<std::string> lines;
                for (std::string line; std::getline(script, line);) {
                    if (lines.size() == kMostLines) {
                        throw ToolRefusal{"the script has more than " + std::to_string(kMostLines) +
                                          " lines"};
                    }
                    lines.push_back(std::move(line));
                }
                return batchReply(session, lines,
                                  {.stopOnError = optionalBool(arguments, "stop_on_error", true),
                                   .discardUnsavedChanges =
                                       optionalBool(arguments, "discard_unsaved_changes", false)});
            }});

        list.push_back(
            Tool{"katana_help", "Katana command reference",
                 "The reference of every Katana command and its syntax: points, drawing, editing, "
                 "selection, layers, linetypes, hatches, styles, alignments, parcels, survey "
                 "calculations and codes, properties, history, files and import/export.",
                 objectSchema(Json::object()), hints(true, false, true),
                 [](Session&, const Json&) { return ToolReply{Session::helpText(), nullptr}; }});

        list.push_back(Tool{
            "katana_status", "Drawing status",
            "The live drawing's state: its project directory (if saved to one), whether it has "
            "unsaved changes, how many entities, layers and alignments it holds, the current "
            "layer and style, the selection and the undo history.",
            objectSchema(Json::object()), hints(true, false, true),
            [](Session& session, const Json&) {
                const katana::cad::DocumentStatus status =
                    katana::cad::documentStatus(session.document());
                return ToolReply{katana::cad::formatStatus(status),
                                 Json::parse(katana::cad::statusJson(status))};
            }});

        list.push_back(Tool{
            "katana_new_project", "New drawing",
            "Start a new, empty drawing. Refused while the current one has unsaved changes unless "
            "discard_unsaved_changes is true.",
            objectSchema(Json{{"discard_unsaved_changes", kDiscardProperty}}),
            hints(false, true, false), [](Session& session, const Json& arguments) {
                return oneLine(session, "NEW",
                               {.discardUnsavedChanges =
                                    optionalBool(arguments, "discard_unsaved_changes", false)});
            }});

        list.push_back(Tool{
            "katana_open_project", "Open a Katana project",
            "Open a Katana project directory (the folder holding project.db) as the live "
            "drawing. Opening may back up or migrate the project in place, so open a copy of one "
            "that matters. Refused while the current drawing has unsaved changes unless "
            "discard_unsaved_changes is true.",
            objectSchema(
                Json{{"path",
                      {{"type", "string"}, {"description", "The project directory to open."}}},
                     {"discard_unsaved_changes", kDiscardProperty}},
                {"path"}),
            hints(false, true, false), [](Session& session, const Json& arguments) {
                return oneLine(session, "OPEN " + quoted(requiredString(arguments, "path")),
                               {.discardUnsavedChanges =
                                    optionalBool(arguments, "discard_unsaved_changes", false)});
            }});

        list.push_back(Tool{
            "katana_save_project", "Save the drawing",
            "Save the live drawing. With a path, save it as a project in that directory (created "
            "if need be) and carry on in it; without, save it where it was opened or last saved.",
            objectSchema(Json{{"path",
                               {{"type", "string"},
                                {"description", "The project directory to save to. Omit to save "
                                                "in place."}}}}),
            hints(false, false, true), [](Session& session, const Json& arguments) {
                const Json& path = argument(arguments, "path");
                if (path.is_null()) {
                    return oneLine(session, "SAVE");
                }
                return oneLine(session, "SAVE " + quoted(requiredString(arguments, "path")));
            }});

        list.push_back(
            Tool{"katana_list_entities", "List entities",
                 "List every entity in the drawing with its id, type, layer and key measurements "
                 "(length, area, radius...). Ids are what SELECT, INFO and the edit commands take.",
                 objectSchema(Json::object()), hints(true, false, true),
                 [](Session& session, const Json&) { return oneLine(session, "LIST"); }});

        list.push_back(Tool{
            "katana_describe_entity", "Describe an entity",
            "Everything about one entity: its geometry, layer, attributes and properties.",
            objectSchema(Json{{"id",
                               {{"type", "integer"},
                                {"minimum", 1},
                                {"description", "The entity id, as katana_list_entities shows."}}}},
                         {"id"}),
            hints(true, false, true), [](Session& session, const Json& arguments) {
                const Json& id = argument(arguments, "id");
                if (!id.is_number_integer() || id.get<long long>() < 1) {
                    throw ToolRefusal{"\"id\" must be a positive whole number"};
                }
                // #id: always the entity, whatever files the server's
                // working directory holds (session.cpp, INFO).
                return oneLine(session, "INFO #" + std::to_string(id.get<long long>()));
            }});

        list.push_back(Tool{
            "katana_import", "Import a file",
            "Import a file into the drawing: DXF always; with the GIS module also shapefiles, "
            "GeoJSON, GeoPackage, .12da archives and other vector formats (as entities), and "
            "rasters and point clouds (as reference layers). placement says where what a DXF, "
            "vector file or .12da archive holds lands: at its own coordinates (keep, the "
            "default), moved as one piece so its lower-left corner sits at 0,0 (local), onto "
            "the drawing's lower-left corner (alongside), or by offset_east and offset_north "
            "(offset). The reply says the move made.",
            objectSchema(
                Json{{"path", {{"type", "string"}, {"description", "The file to import."}}},
                     {"placement",
                      {{"type", "string"},
                       {"enum", {"keep", "local", "alongside", "offset"}},
                       {"description",
                        "Where the data lands, moved as one piece with its shape and dimensions "
                        "unchanged: keep - its own coordinates; local - its lower-left corner at "
                        "0,0 (IMPORT ... LOCAL); alongside - its lower-left corner on the "
                        "drawing's (ALONGSIDE; into an empty drawing it keeps its own); offset - "
                        "moved by offset_east and offset_north (OFFSET=dE,dN). Anything but keep "
                        "is refused for rasters and point clouds, which are drawn at their own "
                        "coordinates."}}},
                     {"offset_east",
                      {{"type", "number"},
                       {"description", "With placement offset: added to every easting."}}},
                     {"offset_north",
                      {{"type", "number"},
                       {"description", "With placement offset: added to every northing."}}},
                     {"local",
                      {{"type", "boolean"},
                       {"description",
                        "The same as placement local: move the imported data as one piece so "
                        "its lower-left corner sits at 0,0 (IMPORT ... LOCAL). Refused for "
                        "rasters and point clouds, which are drawn at their own coordinates."}}}},
                {"path"}),
            hints(false, false, false), [](Session& session, const Json& arguments) {
                const std::string word = importPlacementWord(arguments);
                return oneLine(session, "IMPORT " + quoted(requiredString(arguments, "path")) +
                                            (word.empty() ? "" : " " + word));
            }});

        list.push_back(Tool{
            "katana_export", "Export the drawing",
            "Export the drawing to a file, its format chosen by the extension: .dxf always; with "
            "the GIS module also GIS vector formats.",
            objectSchema(
                Json{{"path",
                      {{"type", "string"}, {"description", "The file to write, e.g. site.dxf."}}}},
                {"path"}),
            hints(false, true, true), [](Session& session, const Json& arguments) {
                return oneLine(session, "EXPORT " + quoted(requiredString(arguments, "path")));
            }});

        list.push_back(Tool{
            "katana_undo", "Undo",
            "Undo the last steps (default one); redo: true redoes them instead.",
            objectSchema(Json{
                {"steps",
                 {{"type", "integer"},
                  {"minimum", 1},
                  {"description", "How many steps (default 1)."}}},
                {"redo",
                 {{"type", "boolean"}, {"description", "Redo instead of undo (default false)."}}}}),
            hints(false, false, false), [](Session& session, const Json& arguments) {
                long long steps = 1;
                if (const Json& value = argument(arguments, "steps"); !value.is_null()) {
                    if (!value.is_number_integer() || value.get<long long>() < 1) {
                        throw ToolRefusal{"\"steps\" must be a positive whole number"};
                    }
                    steps = value.get<long long>();
                }
                const bool redo = optionalBool(arguments, "redo", false);
                return oneLine(session,
                               std::string(redo ? "REDO " : "UNDO ") + std::to_string(steps));
            }});

#if defined(KATANA_WITH_INTEROP)
        // The geoprocessing tools, from their own file (geo/mcp_geo_tools.cpp).
        for (Tool& tool : geoTools()) {
            list.push_back(std::move(tool));
        }
#endif
        return list;
    }();
    return kTools;
}

// ---- resources ----------------------------------------------------------------------

struct Resource {
    const char* uri;
    const char* name;
    const char* description;
    const char* mimeType;
};

constexpr std::array<Resource, 2> kResources{{
    {"katana://help", "Katana command reference", "Every command and its syntax.", "text/plain"},
    {"katana://status", "Drawing status", "The live drawing's state, as katana_status reports it.",
     "application/json"},
}};

// ---- JSON-RPC -----------------------------------------------------------------------

struct RpcError {
    int code;
    std::string message;
};

std::string reply(const Json& id, Json result)
{
    return Json{{"jsonrpc", "2.0"}, {"id", id}, {"result", std::move(result)}}.dump(
        -1, ' ', false, Json::error_handler_t::replace);
}

std::string errorReply(const Json& id, const RpcError& error)
{
    return Json{{"jsonrpc", "2.0"},
                {"id", id},
                {"error", {{"code", error.code}, {"message", error.message}}}}
        .dump(-1, ' ', false, Json::error_handler_t::replace);
}

} // namespace

// ---- the helpers every tool is built from (mcp_tools.hpp) -------------------------------

const Json& argument(const Json& arguments, const char* name)
{
    static const Json kNull;
    const auto found = arguments.find(name);
    return found == arguments.end() ? kNull : *found;
}

std::string requiredString(const Json& arguments, const char* name)
{
    const Json& value = argument(arguments, name);
    if (!value.is_string() || value.get<std::string>().empty()) {
        throw ToolRefusal{std::string("\"") + name + "\" must be a non-empty string"};
    }
    return value.get<std::string>();
}

bool optionalBool(const Json& arguments, const char* name, bool fallback)
{
    const Json& value = argument(arguments, name);
    if (value.is_null()) {
        return fallback;
    }
    if (!value.is_boolean()) {
        throw ToolRefusal{std::string("\"") + name + "\" must be true or false"};
    }
    return value.get<bool>();
}

// A path as the interpreter reads it: one quoted word. It has no escape, so a
// path with a quote in it cannot be said at all - and is refused rather than
// cut short at the quote.
std::string quoted(const std::string& path)
{
    if (path.find('"') != std::string::npos || path.find('\n') != std::string::npos) {
        throw ToolRefusal{"a path may not contain a double quote or a line break: " + path};
    }
    return '"' + path + '"';
}

Json objectSchema(Json properties, std::vector<std::string> required)
{
    Json schema{{"type", "object"}, {"properties", std::move(properties)}};
    if (!required.empty()) {
        schema["required"] = std::move(required);
    }
    schema["additionalProperties"] = false;
    return schema;
}

Json hints(bool readOnly, bool destructive, bool idempotent, bool openWorld)
{
    Json annotations{{"readOnlyHint", readOnly}, {"openWorldHint", openWorld}};
    if (!readOnly) {
        annotations["destructiveHint"] = destructive;
        annotations["idempotentHint"] = idempotent;
    }
    return annotations;
}

ToolReply oneLine(Session& session, const std::string& line, const RunOptions& options)
{
    return batchReply(session, {line}, options);
}

LineOutcome runCaptured(Session& session, const std::string& line)
{
    const Batch batch = runLines(session, {line}, {});
    const LineResult& result = batch.lines.front();
    return LineOutcome{result.ok && !result.skipped, result.output, result.messages};
}

Server::Server(Session& session, std::string version)
    : session_(session), version_(std::move(version))
{
}

std::optional<std::string> Server::handle(std::string_view message)
{
    Json request;
    try {
        request = Json::parse(message);
    } catch (const Json::parse_error& error) {
        return errorReply(nullptr, {kParseError, std::string("not JSON: ") + error.what()});
    }

    // A batch (MCP 2025-03-26 allowed them): each member answered in turn,
    // notifications not at all, and nothing sent when only they were in it.
    if (request.is_array()) {
        if (request.empty()) {
            return errorReply(nullptr, {kInvalidRequest, "an empty batch"});
        }
        std::string replies;
        for (const Json& member : request) {
            if (auto one = handle(member.dump())) {
                replies += (replies.empty() ? "[" : ",") + *one;
            }
        }
        if (replies.empty()) {
            return std::nullopt;
        }
        return replies + "]";
    }

    if (!request.is_object() || request.value("jsonrpc", "") != "2.0" ||
        !request.contains("method") || !request["method"].is_string()) {
        const Json id = request.is_object() && request.contains("id") ? request["id"] : nullptr;
        return errorReply(id, {kInvalidRequest, "not a JSON-RPC 2.0 request"});
    }
    const std::string method = request["method"].get<std::string>();
    const bool isNotification = !request.contains("id");
    const Json id = isNotification ? Json(nullptr) : request["id"];
    const Json params = request.contains("params") && request["params"].is_object()
                            ? request["params"]
                            : Json::object();

    if (isNotification) {
        // notifications/initialized, notifications/cancelled: nothing to do,
        // since every request is answered before the next is read.
        return std::nullopt;
    }

    try {
        if (method == "initialize") {
            const std::string asked = params.value("protocolVersion", "");
            protocolVersion_ = std::string(kLatestProtocolVersion);
            for (const std::string_view supported : kSupportedProtocolVersions) {
                if (asked == supported) {
                    protocolVersion_ = asked;
                }
            }
            return reply(id,
                         Json{{"protocolVersion", protocolVersion_},
                              {"capabilities",
                               {{"tools", {{"listChanged", false}}},
                                {"resources", {{"listChanged", false}}}}},
                              {"serverInfo",
                               {{"name", "katana"}, {"title", "Katana"}, {"version", version_}}},
                              {"instructions", kInstructions}});
        }
        if (method == "ping") {
            return reply(id, Json::object());
        }
        if (method == "tools/list") {
            Json list = Json::array();
            for (const Tool& tool : tools()) {
                list.push_back(Json{{"name", tool.name},
                                    {"title", tool.title},
                                    {"description", tool.description},
                                    {"inputSchema", tool.inputSchema},
                                    {"annotations", tool.annotations}});
            }
            return reply(id, Json{{"tools", std::move(list)}});
        }
        if (method == "tools/call") {
            if (!params.contains("name") || !params["name"].is_string()) {
                return errorReply(id, {kInvalidParams, "tools/call needs a tool name"});
            }
            const std::string name = params["name"].get<std::string>();
            const auto tool = std::find_if(tools().begin(), tools().end(),
                                           [&](const Tool& t) { return name == t.name; });
            if (tool == tools().end()) {
                return errorReply(id, {kInvalidParams, "no tool is called " + name});
            }
            const Json arguments = params.contains("arguments") && params["arguments"].is_object()
                                       ? params["arguments"]
                                       : Json::object();
            ToolReply result;
            try {
                result = tool->call(session_, arguments);
            } catch (const ToolRefusal& refusal) {
                result = {"error: " + refusal.message, nullptr, true};
            } catch (const std::exception& error) {
                result = {std::string("error: ") + error.what(), nullptr, true};
            }
            Json content{{"content", Json::array({Json{{"type", "text"}, {"text", result.text}}})},
                         {"isError", result.isError}};
            // Structured content arrived in 2025-06-18; an older client has
            // the same facts in the text.
            if (!result.structured.is_null() && protocolVersion_ >= "2025-06-18") {
                content["structuredContent"] = result.structured;
            }
            return reply(id, std::move(content));
        }
        if (method == "resources/list") {
            Json list = Json::array();
            for (const Resource& resource : kResources) {
                list.push_back(Json{{"uri", resource.uri},
                                    {"name", resource.name},
                                    {"description", resource.description},
                                    {"mimeType", resource.mimeType}});
            }
            return reply(id, Json{{"resources", std::move(list)}});
        }
        if (method == "resources/templates/list") {
            return reply(id, Json{{"resourceTemplates", Json::array()}});
        }
        if (method == "resources/read") {
            const std::string uri = params.value("uri", "");
            std::string text;
            std::string mimeType;
            if (uri == "katana://help") {
                text = Session::helpText();
                mimeType = "text/plain";
            } else if (uri == "katana://status") {
                text = statusOf(session_).dump(2);
                mimeType = "application/json";
            } else {
                // -32002 is MCP's "resource not found".
                return errorReply(id, {-32002, "no resource " + uri});
            }
            return reply(
                id,
                Json{{"contents",
                      Json::array({Json{{"uri", uri}, {"mimeType", mimeType}, {"text", text}}})}});
        }
        if (method == "prompts/list") {
            return reply(id, Json{{"prompts", Json::array()}});
        }
        return errorReply(id, {kMethodNotFound, "no method " + method});
    } catch (const std::exception& error) {
        return errorReply(id, {kInternalError, error.what()});
    }
}

} // namespace katana::app::mcp
