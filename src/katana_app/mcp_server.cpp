// The MCP server's messages and tools (mcp_server.hpp, docs/mcp.md).
//
// Every tool is a thin shape over Session::run: a tool builds command lines,
// runs them, and hands back what the session printed. So there is one
// implementation of every verb, the one katana_cli and its tests exercise, and
// a tool can never do something the command line cannot. (Two tools run no
// line: katana_status and katana_customisation read the Document through cad,
// for what no reply holds as structure, and change nothing.)

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

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/customisation_report.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/document_status.hpp"
#include "katana/cad/import_placement.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/model.hpp"

#if defined(KATANA_WITH_INTEROP)
#include "geo/gis_records.hpp"
#endif

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
        } else if (katana::cad::CommandInterpreter::replacesDocument(line) &&
                   session.document().isModified() &&
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

// One key=value word of a line, quoted when the value holds a blank:
// ToolRefusal for a double quote or a line break, which no line can carry.
std::string optionWord(const std::string& key, const std::string& value)
{
    if (value.find('"') != std::string::npos || value.find('\n') != std::string::npos) {
        throw ToolRefusal{"\"" + key + "\" may not hold a double quote or a line break (in "
                          "SQLite, write an identifier as [name])"};
    }
    const bool blank = value.empty() || value.find_first_of(" \t") != std::string::npos;
    return " " + key + "=" + (blank ? "\"" + value + "\"" : value);
}

// A list argument - ["a", "b"] or "a,b" - as the comma-joined value a line
// takes.
std::string listArgument(const Json& value, const char* name)
{
    if (value.is_string()) {
        return value.get<std::string>();
    }
    if (!value.is_array() || value.empty() ||
        !std::ranges::all_of(value, [](const Json& each) { return each.is_string(); })) {
        throw ToolRefusal{std::string("\"") + name + "\" is a list of names"};
    }
    std::string joined;
    for (const Json& each : value) {
        const std::string item = each.get<std::string>();
        if (item.find(',') != std::string::npos) {
            throw ToolRefusal{std::string("a name in \"") + name + "\" may not hold a comma"};
        }
        joined += (joined.empty() ? "" : ",") + item;
    }
    return joined;
}

// A whole number argument at least `low`, as text.
std::string wholeArgument(const Json& value, const char* name, long long low)
{
    if (!value.is_number_integer() || value.get<long long>() < low) {
        throw ToolRefusal{std::string("\"") + name + "\" is a whole number from " +
                          std::to_string(low)};
    }
    return std::to_string(value.get<long long>());
}

// katana_import's options as the words of its IMPORT line (docs/mcp.md, "I3
// and I4"), each the word a person would type, so the line in the reply is
// one they could type again.
std::string importOptionWords(const Json& arguments)
{
    std::string words;
    const auto text = [&](const char* name, const char* key) {
        if (const Json& value = argument(arguments, name); !value.is_null()) {
            if (!value.is_string() || value.get<std::string>().empty()) {
                throw ToolRefusal{std::string("\"") + name + "\" must be a non-empty string"};
            }
            words += optionWord(key, value.get<std::string>());
        }
    };
    const auto list = [&](const char* name, const char* key) {
        if (const Json& value = argument(arguments, name); !value.is_null()) {
            words += optionWord(key, listArgument(value, name));
        }
    };
    const auto whole = [&](const char* name, const char* key, long long low) {
        if (const Json& value = argument(arguments, name); !value.is_null()) {
            words += optionWord(key, wholeArgument(value, name, low));
        }
    };
    list("layers", "layers");
    text("where", "where");
    text("sql", "sql");
    text("dialect", "dialect");
    list("fields", "fields");
    text("target_layer", "target");
    whole("max_features", "max", 1);
    if (const Json& value = argument(arguments, "open_options"); !value.is_null()) {
        if (!value.is_array() ||
            !std::ranges::all_of(value, [](const Json& each) { return each.is_string(); })) {
            throw ToolRefusal{"\"open_options\" is a list of \"KEY=VALUE\""};
        }
        for (const Json& each : value) {
            words += optionWord("oo", each.get<std::string>());
        }
    }
    if (const Json& value = argument(arguments, "attributes"); !value.is_null()) {
        if (!value.is_boolean()) {
            throw ToolRefusal{"\"attributes\" must be true or false"};
        }
        words += value.get<bool>() ? "" : " attributes=no";
    }
    text("crs", "crs");
    text("source_crs", "srs");
    whole("band", "band", 1);
    if (const Json& value = argument(arguments, "subdataset"); !value.is_null()) {
        words += optionWord("subdataset", value.is_number_integer()
                                              ? wholeArgument(value, "subdataset", 1)
                                              : (value.is_string() ? value.get<std::string>()
                                                                   : std::string()));
    }
    whole("max_pixels", "maxpixels", 1);
    text("name", "name");
    whole("budget", "budget", 1);
    if (const Json& value = argument(arguments, "classes"); !value.is_null()) {
        words += optionWord("class", value.is_number_integer() ? wholeArgument(value, "classes", 0)
                                                               : listArgument(value, "classes"));
    }
    if (const Json& value = argument(arguments, "resolution"); !value.is_null()) {
        if (!value.is_number() || !(value.get<double>() > 0.0)) {
            throw ToolRefusal{"\"resolution\" is a point spacing above 0"};
        }
        words += " resolution=" + katana::core::formatExactReal(value.get<double>());
    }
    // The scope: `scope`, or an `area` alone.
    const Json& scope = argument(arguments, "scope");
    const Json& area = argument(arguments, "area");
    if (!scope.is_null() || !area.is_null()) {
        if (!scope.is_null() && !scope.is_string()) {
            throw ToolRefusal{"\"scope\" is selection, drawing, area or layers"};
        }
        words += " " + scopeWordsOf(scope.is_string() ? scope.get<std::string>() : "area", area,
                                    argument(arguments, "scope_layers"),
                                    optionalBool(arguments, "only", false),
                                    argument(arguments, "scope_where"));
    }
    if (optionalBool(arguments, "clip", false)) {
        words += " clip";
    }
    if (optionalBool(arguments, "preview", false)) {
        words += " PREVIEW";
    }
    return words;
}

// katana_export's options as the words of its EXPORT line (docs/mcp.md, "I3
// and I4"): the scope first, as a person types it, then the options.
std::string exportOptionWords(const Json& arguments)
{
    std::string words;
    if (const Json& cloud = argument(arguments, "cloud"); !cloud.is_null()) {
        if (cloud.is_number_integer()) {
            words += " CLOUD " + std::to_string(cloud.get<long long>());
        } else if (cloud.is_string() && !cloud.get<std::string>().empty()) {
            words += " CLOUD " + quoted(cloud.get<std::string>());
        } else {
            throw ToolRefusal{"\"cloud\" is a reference point cloud's id or name"};
        }
    }
    const Json& scope = argument(arguments, "scope");
    const Json& area = argument(arguments, "area");
    if (!scope.is_null() || !area.is_null()) {
        if (!scope.is_null() && !scope.is_string()) {
            throw ToolRefusal{"\"scope\" is selection, drawing, area or layers"};
        }
        words += " " + scopeWordsOf(scope.is_string() ? scope.get<std::string>() : "area", area,
                                    argument(arguments, "layers"),
                                    optionalBool(arguments, "only", false),
                                    argument(arguments, "where"));
    } else if (!argument(arguments, "where").is_null() && argument(arguments, "layers").is_null()) {
        // "where" alone filters the selection, as WHERE alone does on the
        // command line (the one scope parser's rule, MODIFY's): written
        // SELECTION WHERE ..., so the line says what it takes.
        words += " " + scopeWordsOf("selection", area, Json(), false, argument(arguments, "where"));
    } else if (!argument(arguments, "layers").is_null()) {
        throw ToolRefusal{"\"layers\" says a scope: give \"scope\": \"layers\" with it"};
    }
    if (const Json& name = argument(arguments, "layer_name"); !name.is_null()) {
        if (!name.is_string() || name.get<std::string>().empty()) {
            throw ToolRefusal{"\"layer_name\" must be a non-empty string"};
        }
        words += optionWord("layername", name.get<std::string>());
    }
    if (optionalBool(arguments, "split_by_layer", false)) {
        words += " split=layer";
    }
    if (optionalBool(arguments, "append", false)) {
        words += " append";
    }
    if (const Json& crs = argument(arguments, "crs"); !crs.is_null()) {
        if (!crs.is_string() || crs.get<std::string>().empty()) {
            throw ToolRefusal{"\"crs\" is project, native or a code such as EPSG:4326"};
        }
        words += optionWord("crs", crs.get<std::string>());
    }
    const auto options = [&](const char* name, const char* key) {
        if (const Json& value = argument(arguments, name); !value.is_null()) {
            if (!value.is_array() ||
                !std::ranges::all_of(value, [](const Json& each) { return each.is_string(); })) {
                throw ToolRefusal{std::string("\"") + name + "\" is a list of \"KEY=VALUE\""};
            }
            for (const Json& each : value) {
                words += optionWord(key, each.get<std::string>());
            }
        }
    };
    options("creation_options", "co");
    options("layer_creation_options", "lco");
    if (const Json& text = argument(arguments, "text"); !text.is_null()) {
        if (!text.is_string() || (text != "points" && text != "skip")) {
            throw ToolRefusal{"\"text\" is points or skip"};
        }
        words += " text=" + text.get<std::string>();
    }
    if (const Json& curve = argument(arguments, "curve"); !curve.is_null()) {
        if (!curve.is_number() || !(curve.get<double>() > 0.0)) {
            throw ToolRefusal{"\"curve\" is a distance above 0"};
        }
        words += " curve=" + katana::core::formatExactReal(curve.get<double>());
    }
    if (const Json& properties = argument(arguments, "properties"); !properties.is_null()) {
        if (!properties.is_boolean()) {
            throw ToolRefusal{"\"properties\" must be true or false"};
        }
        words += properties.get<bool>() ? " properties=yes" : " properties=no";
    }
    if (optionalBool(arguments, "preview", false)) {
        words += " PREVIEW";
    }
    return words;
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

// The reply of a tool whose line answers in records (IMPORT, EXPORT): the
// batch reply, and the records again as structured content, each an object
// of its fields (geo/gis_records.hpp), so an agent reads what came in - the
// entities, the reference layer's id, the move a placement made, each
// warning - without reading the text. Without GDAL the records are the text's
// alone: the DXF verbs write the same words, but the reader of them is the
// executor's.
ToolReply recordsReply(Session& session, const std::string& line)
{
    ToolReply reply = oneLine(session, line);
#if defined(KATANA_WITH_INTEROP)
    const Json& commands = reply.structured["commands"];
    Json records = Json::array();
    if (!commands.empty() && commands.front().contains("output")) {
        records = katana::app::geo::recordsJson(commands.front()["output"].get<std::string>());
    }
    reply.structured["records"] = std::move(records);
#endif
    return reply;
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
            "AREA), survey field files (SURVEY READ <file>; SURVEY IMPORT <file> [SETTINGS "
            "<file>] [SET key=value ...] [CODES on|off] [LINEWORK on|off], where SET "
            "control=<id>;drawing;fixed;0;fixed;0;fixed;0 "
            "holds a point of the drawing, as FORWARD puts one there, and an import codes "
            "and strings the points it draws by the loaded survey codes unless told off), "
            "survey codes (CODE, CODE LIST, CODE CHECK; LINEWORK [<scope>] [WHERE key=value "
            "...] [ORDER number|entity] [PREVIEW] joins coded points into lines, HELP "
            "LINEWORK) and the customisation they come from (CUSTOMISE <file> "
            "loads a Katana customisation file, CUSTOMISE EXPORT <file> writes the session as "
            "one, CUSTOMISE alone reports what is loaded; HELP CUSTOMISE), AS 5488 subsurface "
            "utilities "
            "(UTILITY REPORT, VERIFY, CLEARANCE, CHECK, DRAW, REGRADE, SCHEDULE - on a schedule "
            "file, or on what is drawn by the scope words DRAWING | SELECTION | "
            "AREA x0,y0,x1,y1 | LAYERS a,b [ONLY], then [WHERE key=value ...]; UTILITY DRAW "
            "<scope> METHOD <method> [TYPE <type>] [H_UNC <m>] [FIELDS column=property,...] "
            "draws the lines, polylines and points a survey or an import left in the drawing "
            "as services; HELP UTILITY), "
            "global modify (MODIFY <scope> [WHERE key=value ...] SET key=value ...), "
            "inspection (LIST, INFO), UNDO/REDO, "
            "and files (NEW, OPEN, SAVE, IMPORT, EXPORT). Each command is one undoable step. By "
            "default the batch stops at the first command that fails. Returns each command's "
            "output and the drawing's status afterwards. PLOT, PLOTSHEETS, SNAPSHOT, ONLINE, "
            "SCRIPT, GRID, EXAGGERATION, VIEWS and ZOOM are the desktop window's verbs and are "
            "refused here, saying so: plotting, view pictures, online data and the views need "
            "the window (or katana run headless with --command); a script file is "
            "katana_run_script's.",
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
            "(offset). The reply is records: imported (what came in, its extent and CRS), placed "
            "(the move made), reference or surface (a layer or surface added, with its id), "
            "tally and warning - in structuredContent.records as objects.",
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
                        "rasters and point clouds, which are drawn at their own coordinates."}}},
                     {"layers",
                      {{"type", "array"},
                       {"items", {{"type", "string"}}},
                       {"description", "Only these of the file's layers, by name (layers=)."}}},
                     {"where",
                      {{"type", "string"},
                       {"description",
                        "An OGR SQL WHERE clause on the file's features, applied by the driver: "
                        "kind = 'lot' (where=)."}}},
                     {"sql",
                      {{"type", "string"},
                       {"description",
                        "A SELECT run on the file, whose rows are imported in place of its "
                        "layers, as one layer (sql=). No double quotes: SQLite reads "
                        "[identifier] alike."}}},
                     {"dialect",
                      {{"type", "string"},
                       {"enum", {"ogrsql", "sqlite"}},
                       {"description", "The dialect of sql; the driver's own by default."}}},
                     {"scope",
                      {{"type", "string"},
                       {"enum", {"selection", "drawing", "area", "layers"}},
                       {"description",
                        "Only the file's features in the box of what this takes of the drawing "
                        "(the shared scope grammar); area with \"area\", layers with "
                        "\"scope_layers\" and \"only\"; \"scope_where\" filters what it "
                        "takes. The box goes to the driver as its spatial filter."}}},
                     {"area",
                      {{"type", "array"},
                       {"items", {{"type", "number"}}},
                       {"minItems", 4},
                       {"maxItems", 4},
                       {"description",
                        "[x0, y0, x1, y1] in the drawing's coordinates: scope area (given alone, "
                        "it is the scope)."}}},
                     {"scope_layers",
                      {{"type", "array"},
                       {"items", {{"type", "string"}}},
                       {"description", "With scope layers: the drawing's layers."}}},
                     {"only",
                      {{"type", "boolean"},
                       {"description", "With scope layers: without their sublayers."}}},
                     {"scope_where",
                      {{"type", "array"},
                       {"items", {{"type", "string"}}},
                       {"description",
                        "Conditions on what the scope takes: [\"TYPE=polyline\"]."}}},
                     {"clip",
                      {{"type", "boolean"},
                       {"description",
                        "Cut features at the scope's edge (an area) or by the closed shapes it "
                        "takes (clip)."}}},
                     {"fields",
                      {{"type", "array"},
                       {"items", {{"type", "string"}}},
                       {"description", "Only these attributes become properties (fields=)."}}},
                     {"attributes",
                      {{"type", "boolean"},
                       {"description",
                        "false: no attributes become properties (attributes=no)."}}},
                     {"target_layer",
                      {{"type", "string"},
                       {"description", "Every entity on this layer (target=)."}}},
                     {"max_features",
                      {{"type", "integer"},
                       {"minimum", 1},
                       {"description", "At most this many features (max=)."}}},
                     {"open_options",
                      {{"type", "array"},
                       {"items", {{"type", "string"}}},
                       {"description",
                        "The driver's open options, [\"KEY=VALUE\"], each checked against the "
                        "driver's own list (oo=)."}}},
                     {"crs",
                      {{"type", "string"},
                       {"enum", {"project", "adopt"}},
                       {"description",
                        "project: vector data moved into the project's coordinate system; adopt: "
                        "the project takes the file's when it has none (crs=)."}}},
                     {"source_crs",
                      {{"type", "string"},
                       {"description",
                        "The coordinate system of a file that declares none, EPSG:28356 (srs=)."}}},
                     {"band",
                      {{"type", "integer"},
                       {"minimum", 1},
                       {"description", "A raster: this band alone, as grey (band=)."}}},
                     {"subdataset",
                      {{"type", Json::array({"integer", "string"})},
                       {"description",
                        "A raster: the dataset inside a container, by number or name "
                        "(subdataset=)."}}},
                     {"max_pixels",
                      {{"type", "integer"},
                       {"minimum", 1},
                       {"description", "A raster: the display copy's longest side (maxpixels=)."}}},
                     {"name",
                      {{"type", "string"},
                       {"description", "A raster or a point cloud: the reference layer's name."}}},
                     {"budget",
                      {{"type", "integer"},
                       {"minimum", 1},
                       {"description", "A point cloud: the points kept (budget=)."}}},
                     {"classes",
                      {{"type", "integer"},
                       {"minimum", 0},
                       {"maximum", 255},
                       {"description", "A point cloud: only this ASPRS class (class=)."}}},
                     {"resolution",
                      {{"type", "number"},
                       {"description",
                        "A COPC point cloud: read its octree to this point spacing."}}},
                     {"preview",
                      {{"type", "boolean"},
                       {"description",
                        "Read with the filters and say how many features matched of how many, "
                        "importing nothing (PREVIEW)."}}}},
                {"path"}),
            hints(false, false, false), [](Session& session, const Json& arguments) {
                const std::string word = importPlacementWord(arguments);
                return recordsReply(session, "IMPORT " +
                                                 quoted(requiredString(arguments, "path")) +
                                                 (word.empty() ? "" : " " + word) +
                                                 importOptionWords(arguments));
            }});

        list.push_back(Tool{
            "katana_export", "Export the drawing",
            "Export the drawing to a file, its format chosen by the extension: .dxf always; with "
            "the GIS module also GIS vector formats and .12da archives (with the session's "
            "surfaces). The reply is an exported record - the driver, how many features or "
            "entities were written and how many skipped - and any warnings, in "
            "structuredContent.records as objects.",
            objectSchema(
                Json{{"path",
                      {{"type", "string"}, {"description", "The file to write, e.g. site.dxf."}}},
                     {"scope",
                      {{"type", "string"},
                       {"enum", {"selection", "drawing", "area", "layers"}},
                       {"description",
                        "What of the drawing is written (the shared scope grammar); the whole "
                        "drawing when neither this nor \"area\" is given. area with \"area\", "
                        "layers with \"layers\" and \"only\"; \"where\" filters it."}}},
                     {"area",
                      {{"type", "array"},
                       {"items", {{"type", "number"}}},
                       {"minItems", 4},
                       {"maxItems", 4},
                       {"description", "[x0, y0, x1, y1]: scope area (given alone, the scope)."}}},
                     {"layers",
                      {{"type", "array"},
                       {"items", {{"type", "string"}}},
                       {"description", "With scope layers: the drawing's layers."}}},
                     {"only",
                      {{"type", "boolean"},
                       {"description", "With scope layers: without their sublayers."}}},
                     {"where",
                      {{"type", "array"},
                       {"items", {{"type", "string"}}},
                       {"description", "Conditions on what the scope takes: [\"TYPE=polyline\", "
                                       "\"PROP=owner:Smith*\"]. Without \"scope\" they filter "
                                       "the selection, as WHERE alone does on the command line; "
                                       "\"scope\": \"drawing\" filters the whole drawing."}}},
                     {"layer_name",
                      {{"type", "string"},
                       {"description", "The layer's name in the file (layername=)."}}},
                     {"split_by_layer",
                      {{"type", "boolean"},
                       {"description",
                        "One file layer per drawing layer, named after it (split=layer)."}}},
                     {"append",
                      {{"type", "boolean"},
                       {"description",
                        "Add the layer to the file (a GeoPackage) rather than replace it; a "
                        "layer name the file has is refused (append)."}}},
                     {"crs",
                      {{"type", "string"},
                       {"description",
                        "project (the default; a GeoJSON is then longitude and latitude, as "
                        "RFC 7946 says), native (the project's, unconverted, where the format "
                        "allows), or a code the coordinates are moved into (crs=)."}}},
                     {"creation_options",
                      {{"type", "array"},
                       {"items", {{"type", "string"}}},
                       {"description",
                        "The driver's creation options, [\"KEY=VALUE\"], each checked against "
                        "its list (co=)."}}},
                     {"layer_creation_options",
                      {{"type", "array"},
                       {"items", {{"type", "string"}}},
                       {"description",
                        "The driver's layer creation options, checked likewise (lco=)."}}},
                     {"text",
                      {{"type", "string"},
                       {"enum", {"points", "skip"}},
                       {"description",
                        "points: text entities as points with text, text_height and "
                        "text_rotation fields and an OGR_STYLE label; skip (the default) leaves "
                        "them out, counted."}}},
                     {"curve",
                      {{"type", "number"},
                       {"description",
                        "The largest distance a chord may stray from an arc (curve=)."}}},
                     {"properties",
                      {{"type", "boolean"},
                       {"description", "false: entity properties are not written."}}},
                     {"cloud",
                      {{"type", {"integer", "string"}},
                       {"description",
                        "With a .las or .laz path: the reference point cloud to write, by id or "
                        "name (CLOUD); the one there is when absent. The points held are "
                        "written, a budgeted import's sample included, which the reply says."}}},
                     {"preview",
                      {{"type", "boolean"},
                       {"description",
                        "Say what would be written and what the scope takes, writing nothing "
                        "(PREVIEW)."}}}},
                {"path"}),
            hints(false, true, true), [](Session& session, const Json& arguments) {
                return recordsReply(session, "EXPORT " +
                                                 quoted(requiredString(arguments, "path")) +
                                                 exportOptionWords(arguments));
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

        // The customisation as structured data, from its own file
        // (mcp_customisation.cpp).
        list.push_back(customisationTool());

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

constexpr std::array kResources{
    Resource{"katana://help", "Katana command reference", "Every command and its syntax.",
             "text/plain"},
    Resource{"katana://status", "Drawing status",
             "The live drawing's state, as katana_status reports it.", "application/json"},
    // CUSTOMISE JSON (docs/customisation.md, "The replies"), as
    // katana_customisation's summary gives it.
    Resource{"katana://customisation", "Customisation",
             "The session's customisation: its name and where it came from, its sources and "
             "their notices, counts, linework codes, automation, colours, what is wrong with "
             "its rules, what this drawing uses of it, and what went wrong when the session "
             "started.",
             "application/json"},
#if defined(KATANA_WITH_INTEROP)
    // FORMATS JSON (docs/interop.md, "Formats"), as katana_formats lists it.
    Resource{"katana://formats", "GDAL formats",
             "The formats this build of GDAL reads and writes: driver, kinds, extensions, /vsi.",
             "application/json"},
#endif
};

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

std::string scopeWordsOf(const std::string& kind, const Json& area, const Json& layers, bool only,
                         const Json& where)
{
    katana::cad::ScopeWords scope;
    if (kind == "selection") {
        scope.source = katana::cad::ScopeSource::Selection;
    } else if (kind == "drawing") {
        scope.source = katana::cad::ScopeSource::Drawing;
    } else if (kind == "area") {
        if (!area.is_array() || area.size() != 4 ||
            !std::ranges::all_of(area, [](const Json& n) { return n.is_number(); })) {
            throw ToolRefusal{"scope area needs \"area\": [x0, y0, x1, y1]"};
        }
        scope.source = katana::cad::ScopeSource::Area;
        const double x0 = area[0].get<double>(), y0 = area[1].get<double>();
        const double x1 = area[2].get<double>(), y1 = area[3].get<double>();
        scope.area = katana::geometry::Box2(
            katana::geometry::Point2(std::min(x0, x1), std::min(y0, y1)),
            katana::geometry::Point2(std::max(x0, x1), std::max(y0, y1)));
    } else if (kind == "layers") {
        if (!layers.is_array() || layers.empty() ||
            !std::ranges::all_of(layers, [](const Json& l) { return l.is_string(); })) {
            throw ToolRefusal{"scope layers needs \"layers\": [\"a\", \"b\"]"};
        }
        scope.source = katana::cad::ScopeSource::Layers;
        for (const Json& layer : layers) {
            scope.layers.push_back(layer.get<std::string>());
        }
        scope.sublayers = !only;
    } else {
        throw ToolRefusal{"\"scope\" is selection, drawing, area or layers (VIEW is the window's)"};
    }
    if (!where.is_null()) {
        if (!where.is_array()) {
            throw ToolRefusal{"the scope's conditions are a list: [\"TYPE=polyline\"]"};
        }
        for (const Json& condition : where) {
            if (!condition.is_string()) {
                throw ToolRefusal{"each of the scope's conditions is a string"};
            }
            if (auto status =
                    katana::cad::parseWhereCondition(condition.get<std::string>(), scope.filter);
                !status) {
                throw ToolRefusal{status.error().describe()};
            }
        }
    }
    auto words = katana::cad::formatScopeWords(scope);
    if (!words) {
        throw ToolRefusal{words.error().describe()};
    }
    return *words;
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

namespace {

// Every schema says additionalProperties: false, but a client need not check
// it: an argument the tool does not take (a misspelt "split" for
// "split_by_layer") was ignored, and the call ran without the option asked
// for. Refused here, naming it and the arguments there are.
void refuseUndeclared(const Tool& tool, const Json& arguments)
{
    const Json& properties = tool.inputSchema["properties"];
    for (const auto& given : arguments.items()) {
        if (properties.is_object() && properties.contains(given.key())) {
            continue;
        }
        std::string known;
        if (properties.is_object()) {
            for (const auto& declared : properties.items()) {
                known += (known.empty() ? "" : ", ") + declared.key();
            }
        }
        throw ToolRefusal{std::string(tool.name) + " takes no argument \"" + given.key() + "\"" +
                          (known.empty() ? std::string(": it takes none") : "; it takes " + known)};
    }
}

} // namespace

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
                refuseUndeclared(*tool, arguments);
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
            } else if (uri == "katana://customisation") {
                // cad's own report, read from the Document as the status is:
                // a line would answer the same and leave a CUSTOMISE JSON in
                // the history of a session nobody had typed into.
                text = katana::cad::customisationJson(session_.document());
                mimeType = "application/json";
#if defined(KATANA_WITH_INTEROP)
            } else if (uri == "katana://formats") {
                // The verb's own JSON, so the resource is what FORMATS says.
                const LineOutcome formats = runCaptured(session_, "FORMATS JSON");
                if (!formats.ok) {
                    return errorReply(id, {kInternalError, formats.messages});
                }
                text = formats.output;
                mimeType = "application/json";
#endif
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
