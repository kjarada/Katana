// The geoprocessing tools of the MCP server (docs/mcp.md, "Geoprocessing
// tools"): GDAL's catalogue and each algorithm's arguments as JSON, and a run
// built from structured arguments.
//
// katana_gdal_catalogue and katana_gdal_describe read the bridge directly:
// they change nothing and a line would add nothing. katana_gdal_run builds
// the GDAL line a person would type and runs it through the Session, as
// every tool that changes something does, then reads the reply records back
// into structured content - one implementation of the verb, the one
// katana_cli runs.
//
// One reserved block per package that adds a tool, so the lanes edit only
// their own lines.

#include <algorithm>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "../mcp_tools.hpp"
#include "formats_verbs.hpp"
#include "gis_records.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/processing.hpp"
#include "replies.hpp"
#include "schema.hpp"

namespace katana::app::mcp {

namespace {

namespace gp = katana::gis::processing;
namespace geo = katana::app::geo;

// A word of a line: as it is, or quoted when it holds a blank or is empty.
// The line has no escape, so a quote or a line break cannot be said at all.
std::string word(const std::string& text, const char* what)
{
    if (text.find_first_of("\"\r\n") != std::string::npos) {
        throw ToolRefusal{std::string(what) + " may not contain a double quote or a line break"};
    }
    const bool blank = text.empty() || text.find_first_of(" \t") != std::string::npos;
    return blank ? "\"" + text + "\"" : text;
}

std::string number(const Json& value)
{
    if (value.is_number_integer()) {
        return std::to_string(value.get<long long>());
    }
    return katana::core::formatExactReal(value.get<double>());
}

// The algorithm an agent names: "raster hillshade", as GDAL LIST names it.
std::vector<std::string> algorithmWords(const std::string& name)
{
    std::vector<std::string> words;
    std::string current;
    for (const char c : name) {
        if (c == ' ' || c == '\t') {
            if (!current.empty()) {
                words.push_back(std::move(current));
                current.clear();
            }
        } else {
            current += c;
        }
    }
    if (!current.empty()) {
        words.push_back(std::move(current));
    }
    return words;
}

gp::AlgorithmSpec specOf(const Json& arguments)
{
    const std::vector<std::string> words = algorithmWords(requiredString(arguments, "algorithm"));
    std::size_t consumed = 0;
    auto path = gp::resolve(words, consumed);
    if (!path) {
        throw ToolRefusal{path.error().describe()};
    }
    if (consumed != words.size()) {
        throw ToolRefusal{"\"algorithm\" names one algorithm; " + words[consumed] +
                          " is not part of " + gp::pathText(*path)};
    }
    auto spec = gp::describe(*path);
    if (!spec) {
        throw ToolRefusal{spec.error().describe()};
    }
    return std::move(spec).value();
}

// One source as FROM's words, the scope through the shared formatter so the
// words are exactly what the scope parser reads back.
std::string sourceWords(const Json& source, const std::string& arg)
{
    if (!source.is_object()) {
        throw ToolRefusal{"each input of \"" + arg + "\" is an object: {scope ...}, {raster}, "
                                                     "{surface} or {file}"};
    }
    const int kinds = (source.contains("scope") ? 1 : 0) + (source.contains("raster") ? 1 : 0) +
                      (source.contains("surface") ? 1 : 0) + (source.contains("file") ? 1 : 0);
    if (kinds != 1) {
        throw ToolRefusal{"an input of \"" + arg +
                          "\" gives exactly one of scope, raster, surface and file"};
    }
    if (source.contains("raster")) {
        const Json& raster = source["raster"];
        if (raster.is_number_integer()) {
            return "RASTER " + std::to_string(raster.get<long long>());
        }
        if (!raster.is_string()) {
            throw ToolRefusal{"\"raster\" is a reference raster's id or name"};
        }
        return "RASTER " + word(raster.get<std::string>(), "a raster's name");
    }
    if (source.contains("surface")) {
        if (!source["surface"].is_string()) {
            throw ToolRefusal{"\"surface\" is a surface's name"};
        }
        std::string words = "SURFACE " + word(source["surface"].get<std::string>(), "a surface's name");
        if (const Json& cell = argument(source, "cell"); !cell.is_null()) {
            if (!cell.is_number() || !(cell.get<double>() > 0.0)) {
                throw ToolRefusal{"\"cell\" is a positive length"};
            }
            words += " CELL " + number(cell);
        }
        return words;
    }
    if (source.contains("file")) {
        if (!source["file"].is_string()) {
            throw ToolRefusal{"\"file\" is a path"};
        }
        std::string words = "FILE " + word(source["file"].get<std::string>(), "a path");
        if (const Json& layer = argument(source, "layer"); !layer.is_null()) {
            if (!layer.is_string()) {
                throw ToolRefusal{"\"layer\" is the file's layer name"};
            }
            words += " LAYER " + word(layer.get<std::string>(), "a layer name");
        }
        return words;
    }
    const std::string kind = source["scope"].is_string() ? source["scope"].get<std::string>() : "";
    return scopeWordsOf(kind, argument(source, "area"), argument(source, "layers"),
                        optionalBool(source, "only", false), argument(source, "where"));
}

// The arguments as GDAL's own --name=value words.
std::string argumentWords(const gp::AlgorithmSpec& spec, const Json& arguments)
{
    if (arguments.is_null()) {
        return {};
    }
    if (!arguments.is_object()) {
        throw ToolRefusal{"\"arguments\" is an object of argument names and values"};
    }
    std::string words;
    for (const auto& [name, value] : arguments.items()) {
        const auto found = std::ranges::find_if(spec.args, [&](const gp::ArgSpec& arg) {
            return arg.name == name || std::ranges::find(arg.aliases, name) != arg.aliases.end();
        });
        if (found == spec.args.end()) {
            throw ToolRefusal{"no argument of " + gp::pathText(spec.info.path) + " is called " +
                              name + "; katana_gdal_describe lists them"};
        }
        if (found->isDataset()) {
            throw ToolRefusal{name + " is a dataset: give it in \"inputs\""};
        }
        const std::string option = "--" + found->name;
        const auto scalar = [&](const Json& one) -> std::string {
            if (one.is_boolean()) {
                return one.get<bool>() ? "true" : "false";
            }
            if (one.is_number()) {
                return number(one);
            }
            if (one.is_string()) {
                return one.get<std::string>();
            }
            throw ToolRefusal{name + " takes a boolean, a number or a string"};
        };
        if (value.is_boolean() && found->type == gp::ArgType::Boolean) {
            words += " " + (value.get<bool>() ? option : option + "=false");
        } else if (value.is_array()) {
            if (found->packedValues || value.size() <= 1) {
                std::string packed;
                for (const Json& one : value) {
                    packed += (packed.empty() ? "" : ",") + scalar(one);
                }
                words += " " + word(option + "=" + packed, "a value");
            } else {
                for (const Json& one : value) {
                    words += " " + word(option + "=" + scalar(one), "a value");
                }
            }
        } else {
            words += " " + word(option + "=" + scalar(value), "a value");
        }
    }
    return words;
}

std::string targetWords(const Json& output)
{
    if (!output.is_object()) {
        throw ToolRefusal{"\"output\" is {layer}, {reference}, {file, format?, overwrite?} or "
                          "{surface}"};
    }
    if (output.contains("layer")) {
        return "TO LAYER " + word(requiredString(output, "layer"), "a layer");
    }
    if (output.contains("reference")) {
        return "TO REFERENCE " + word(requiredString(output, "reference"), "a name");
    }
    if (output.contains("surface")) {
        return "TO SURFACE " + word(requiredString(output, "surface"), "a name");
    }
    if (output.contains("file")) {
        std::string words = "TO FILE " + word(requiredString(output, "file"), "a path");
        if (const Json& format = argument(output, "format"); !format.is_null()) {
            words += " FORMAT " + word(requiredString(output, "format"), "a driver");
        }
        if (optionalBool(output, "overwrite", false)) {
            words += " OVERWRITE";
        }
        return words;
    }
    throw ToolRefusal{"\"output\" is {layer}, {reference}, {file, format?, overwrite?} or "
                      "{surface}"};
}

std::optional<long long> integerOf(const geo::Record& record, const char* key)
{
    const auto text = record.get(key);
    const auto number = text ? katana::core::parseInteger(*text) : std::nullopt;
    return number ? std::optional<long long>(*number) : std::nullopt;
}

Json fieldsJson(const geo::Record& record)
{
    Json object = Json::object();
    for (const auto& [key, text] : record.fields) {
        object[key] = text;
    }
    return object;
}

// The reply records as katana_gdal_run's structured content.
Json runJson(const std::string& line, const LineOutcome& outcome, const std::string& algorithm)
{
    Json inputs = Json::array();
    Json scopes = Json::array();
    Json outputs = Json::array();
    Json warnings = Json::array();
    Json result{{"ok", outcome.ok},
                {"line", line},
                {"algorithm", algorithm},
                {"cancelled", outcome.messages.find("cancelled") != std::string::npos},
                {"seconds", nullptr}};
    for (const geo::Record& record : geo::parseRecords(outcome.output)) {
        if (record.kind == "gdal") {
            if (const auto seconds = record.get("seconds")) {
                if (const auto value = katana::core::parseFiniteDouble(*seconds)) {
                    result["seconds"] = *value;
                }
            }
            if (record.get("preview")) {
                result["preview"] = true;
            }
        } else if (record.kind == "input") {
            inputs.push_back(fieldsJson(record));
        } else if (record.kind == "scope") {
            Json scope = fieldsJson(record);
            Json skipped = Json::object();
            for (const auto& [key, text] : record.fields) {
                if (key.starts_with("skipped.")) {
                    skipped[key.substr(8)] = katana::core::parseInteger(text).value_or(0);
                    scope.erase(key);
                }
            }
            for (const char* key : {"matched", "used", "points", "lines", "polygons"}) {
                if (const auto count = integerOf(record, key)) {
                    scope[key] = *count;
                }
            }
            scope["skipped"] = skipped;
            scopes.push_back(std::move(scope));
        } else if (record.kind == "output") {
            Json output = fieldsJson(record);
            for (const char* key : {"created", "updated", "deleted", "skipped", "id"}) {
                if (const auto count = integerOf(record, key)) {
                    output[key] = *count;
                }
            }
            if (const auto raster = record.get("raster")) {
                const std::size_t x = raster->find('x');
                const auto width = katana::core::parseInteger(raster->substr(0, x));
                const auto height = x == std::string::npos
                                        ? std::nullopt
                                        : katana::core::parseInteger(raster->substr(x + 1));
                if (width && height) {
                    output["raster"] = Json{{"width", *width}, {"height", *height}};
                }
            }
            outputs.push_back(std::move(output));
        } else if (record.kind == "text") {
            std::string body;
            for (const std::string& text : record.body) {
                body += (body.empty() ? "" : "\n") + text;
            }
            result["text"] = body;
        } else if (record.kind == "return") {
            if (const auto code = integerOf(record, "code")) {
                result["return_code"] = *code;
            }
        } else if (record.kind == "warning") {
            warnings.push_back(record.get("text").value_or(""));
        }
    }
    result["inputs"] = inputs;
    result["scope"] = scopes;
    result["outputs"] = outputs;
    result["warnings"] = warnings;
    if (!outcome.messages.empty()) {
        result["messages"] = outcome.messages;
    }
    return result;
}

ToolReply catalogue(Session&, const Json& arguments)
{
    const Json& filter = argument(arguments, "filter");
    if (!filter.is_null() && !filter.is_string()) {
        throw ToolRefusal{"\"filter\" is text: a group (raster, vector grid) or words to look for"};
    }
    const std::string text = filter.is_string() ? katana::core::lowered(filter.get<std::string>()) : "";
    std::vector<std::string> group = algorithmWords(text);
    bool isGroup = false;
    if (!group.empty()) {
        std::size_t consumed = 0;
        const auto resolved = gp::resolve(group, consumed);
        isGroup = !resolved && resolved.error().code == katana::core::ErrorCode::InvalidArgument &&
                  consumed == group.size();
    }
    std::vector<const gp::AlgorithmInfo*> chosen;
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        if (info.container) {
            continue;
        }
        const bool inGroup = isGroup && info.path.size() > group.size() &&
                             std::equal(group.begin(), group.end(), info.path.begin());
        const bool matches =
            text.empty() || inGroup ||
            (!isGroup && (katana::core::lowered(gp::pathText(info.path)).find(text) != std::string::npos ||
                          katana::core::lowered(info.description).find(text) != std::string::npos));
        if (matches) {
            chosen.push_back(&info);
        }
    }
    // Schemas only for a short list: 121 schemas would be most of a model's
    // context for one question.
    const bool schemas = optionalBool(arguments, "schemas", false) && chosen.size() <= 20;
    Json algorithms = Json::array();
    std::string lines;
    for (const gp::AlgorithmInfo* info : chosen) {
        Json entry = geo::algorithmJson(*info);
        if (schemas) {
            if (auto spec = gp::describe(info->path)) {
                entry["arguments_schema"] = geo::argumentsSchema(*spec);
                entry["inputs_schema"] = geo::inputsSchema(*spec);
            }
        }
        algorithms.push_back(std::move(entry));
        lines += geo::algorithmRecord(*info) + "\n";
    }
    const gp::Versions versions = gp::versions();
    lines += "listed algorithms=" + std::to_string(chosen.size()) + " schemas=" +
             (schemas ? "yes" : "no");
    return ToolReply{lines, Json{{"gdal_version", versions.gdal}, {"algorithms", algorithms}}};
}

ToolReply describe(Session&, const Json& arguments)
{
    const gp::AlgorithmSpec spec = specOf(arguments);
    std::string text = geo::algorithmRecord(spec.info);
    for (const gp::ArgSpec& arg : spec.args) {
        text += "\n" + geo::argRecord(arg);
    }
    return ToolReply{text, geo::describeJson(spec)};
}

ToolReply run(Session& session, const Json& arguments)
{
    const gp::AlgorithmSpec spec = specOf(arguments);
    const std::string algorithm = gp::pathText(spec.info.path);
    const bool confirm = optionalBool(arguments, "confirm", false);
    if (spec.info.policy == gp::Policy::Confirm && !confirm) {
        throw ToolRefusal{algorithm + " changes or removes data that already exists; it runs only "
                                      "with \"confirm\": true"};
    }
    std::string line = "GDAL " + algorithm;
    if (const Json& tokens = argument(arguments, "tokens"); !tokens.is_null()) {
        if (!tokens.is_array()) {
            throw ToolRefusal{"\"tokens\" is a list of GDAL's own words"};
        }
        for (const Json& token : tokens) {
            if (!token.is_string()) {
                throw ToolRefusal{"each of \"tokens\" is a string"};
            }
            line += " " + word(token.get<std::string>(), "a GDAL word");
        }
    }
    line += argumentWords(spec, argument(arguments, "arguments"));
    if (const Json& inputs = argument(arguments, "inputs"); !inputs.is_null()) {
        if (!inputs.is_object()) {
            throw ToolRefusal{"\"inputs\" is an object: {\"input\": {\"scope\": \"drawing\"}}"};
        }
        for (const auto& [arg, value] : inputs.items()) {
            if (value.is_array()) {
                for (const Json& source : value) {
                    line += " FROM " + word(arg, "an argument name") + " " + sourceWords(source, arg);
                }
            } else {
                line += " FROM " + word(arg, "an argument name") + " " + sourceWords(value, arg);
            }
        }
    }
    if (const Json& output = argument(arguments, "output"); !output.is_null()) {
        line += " " + targetWords(output);
    }
    if (confirm) {
        line += " CONFIRM";
    }
    if (optionalBool(arguments, "preview", false)) {
        line += " PREVIEW";
    }
    const LineOutcome outcome = runCaptured(session, line);
    std::string text = "> " + line;
    if (!outcome.output.empty()) {
        text += "\n" + outcome.output;
    }
    if (!outcome.messages.empty()) {
        text += "\n" + outcome.messages;
    }
    return ToolReply{text, runJson(line, outcome, algorithm), !outcome.ok};
}

// ---- T0: katana_terrain_list ----
// SURFACE LIST JSON through the Session, as a person's SURFACE LIST: the
// session's surfaces and the rasters a terrain verb reads.
ToolReply terrainList(Session& session, const Json&)
{
    const std::string line = "SURFACE LIST JSON";
    const LineOutcome outcome = runCaptured(session, line);
    if (!outcome.ok) {
        return ToolReply{outcome.messages, Json(), true};
    }
    Json listed = Json::parse(outcome.output, nullptr, false);
    if (listed.is_discarded()) {
        return ToolReply{"SURFACE LIST JSON did not answer JSON: " + outcome.output, Json(), true};
    }
    std::string text = "> " + line;
    for (const Json& surface : listed["surfaces"]) {
        text += "\nsurface " + surface["name"].get<std::string>() + ": " +
                std::to_string(surface["triangles"].get<std::size_t>()) + " triangles, z " +
                geo::fixed3(surface["zmin"].get<double>()) + " to " +
                geo::fixed3(surface["zmax"].get<double>());
    }
    for (const Json& raster : listed["rasters"]) {
        text += "\nraster " + std::to_string(raster["id"].get<std::uint64_t>()) + " " +
                raster["name"].get<std::string>() + ": " +
                std::to_string(raster["width"].get<int>()) + "x" +
                std::to_string(raster["height"].get<int>()) + " " +
                raster["role"].get<std::string>();
    }
    return ToolReply{text, std::move(listed)};
}

// ---- D1: katana_dataset_info ----

// The INFO line of the arguments, and its reply's records as structured
// content; with json, GDAL's own info JSON too, from INFO ... JSON - the
// lines a person would type, run through the Session.
ToolReply datasetInfo(Session& session, const Json& arguments)
{
    std::string words = "INFO " + quoted(requiredString(arguments, "path"));
    if (const Json& layer = argument(arguments, "layer"); !layer.is_null()) {
        if (!layer.is_string()) {
            throw ToolRefusal{"\"layer\" is a layer's name"};
        }
        words += " LAYER " + word(layer.get<std::string>(), "a layer name");
    }
    std::string line = words;
    if (optionalBool(arguments, "stats", false)) {
        line += " STATS";
    }
    if (optionalBool(arguments, "check", false)) {
        line += " CHECK";
    }
    const LineOutcome outcome = runCaptured(session, line);
    std::string text = "> " + line;
    if (!outcome.output.empty()) {
        text += "\n" + outcome.output;
    }
    if (!outcome.messages.empty()) {
        text += "\n" + outcome.messages;
    }
    Json result{{"ok", outcome.ok}, {"line", line}, {"records", geo::recordsJson(outcome.output)}};
    if (!outcome.messages.empty()) {
        result["messages"] = outcome.messages;
    }
    if (outcome.ok && optionalBool(arguments, "json", false)) {
        const std::string jsonLine = words + " JSON";
        const LineOutcome json = runCaptured(session, jsonLine);
        const Json parsed = Json::parse(json.output, nullptr, false);
        if (!json.ok || parsed.is_discarded()) {
            throw ToolRefusal{jsonLine + " gave no JSON: " + json.messages};
        }
        result["gdal"] = parsed;
        text += "\n> " + jsonLine + "\n(GDAL's JSON is in structuredContent.gdal)";
    }
    return ToolReply{text, result, !outcome.ok};
}

// ---- D2: katana_references ----

// A REFS line built from the action, run through the Session, and the
// reference layers after it as structured content: what an agent needs to
// act on a layer by its id, and to see that the action took.
ToolReply references(Session& session, const Json& arguments)
{
    const Json& chosen = argument(arguments, "action");
    const std::string action =
        chosen.is_null() ? std::string("list")
                         : (chosen.is_string() ? chosen.get<std::string>() : std::string("?"));
    const auto reference = [&arguments]() -> std::string {
        const Json& id = argument(arguments, "id");
        if (id.is_number_integer() && id.get<long long>() > 0) {
            return std::to_string(id.get<long long>());
        }
        if (id.is_string() && !id.get<std::string>().empty()) {
            return word(id.get<std::string>(), "a layer's name");
        }
        throw ToolRefusal{"\"id\" is a reference layer's id, or its name"};
    };
    const auto valueWord = [&arguments](const char* what) -> std::string {
        const Json& given = argument(arguments, "value");
        if (given.is_number()) {
            return number(given);
        }
        if (given.is_string() && !given.get<std::string>().empty()) {
            return word(given.get<std::string>(), what);
        }
        throw ToolRefusal{std::string("\"value\" is ") + what};
    };
    std::string line;
    if (action == "list") {
        line = "REFS LIST";
    } else if (action == "show" || action == "hide" || action == "remove" || action == "info") {
        // The verb's words are read in any case (Tokens::is).
        line = "REFS " + action + " " + reference();
    } else if (action == "opacity") {
        line = "REFS OPACITY " + reference() + " " + valueWord("an opacity from 0 to 1");
    } else if (action == "color") {
        line = "REFS COLOR " + reference() + " " +
               valueWord("elevation, intensity, classification, rgb or flat");
    } else if (action == "rename") {
        line = "REFS RENAME " + reference() + " " + valueWord("the new name");
    } else if (action == "overviews") {
        // Writes beside the raster's file: only when the agent says so.
        if (!optionalBool(arguments, "confirm", false)) {
            throw ToolRefusal{"overviews are written beside the raster's file (.ovr); they are "
                              "built only with \"confirm\": true"};
        }
        line = "REFS OVERVIEWS " + reference();
        if (const Json& levels = argument(arguments, "value"); !levels.is_null()) {
            line += " levels=" + valueWord("the levels, e.g. \"2,4,8\"");
        }
        line += " CONFIRM";
    } else if (action == "restore") {
        line = "REFS RESTORE";
    } else {
        throw ToolRefusal{"\"action\" is list, show, hide, remove, info, opacity, color, rename, "
                          "overviews or restore"};
    }
    const LineOutcome outcome = runCaptured(session, line);
    std::string text = "> " + line;
    if (!outcome.output.empty()) {
        text += "\n" + outcome.output;
    }
    if (!outcome.messages.empty()) {
        text += "\n" + outcome.messages;
    }
    Json result{{"ok", outcome.ok}, {"line", line}, {"records", geo::recordsJson(outcome.output)}};
    if (!outcome.messages.empty()) {
        result["messages"] = outcome.messages;
    }
    // The layers as they are now, whatever the action was.
    const LineOutcome listed = runCaptured(session, "REFS JSON");
    const Json layers = Json::parse(listed.output, nullptr, false);
    if (listed.ok && !layers.is_discarded()) {
        result["references"] = layers.value("references", Json::array());
        result["missing"] = layers.value("missing", Json::array());
    }
    return ToolReply{text, result, !outcome.ok};
}

} // namespace

// ---- V5: katana_gis_query (sql_mcp.cpp) ----
[[nodiscard]] Tool gisQueryTool();

std::vector<Tool> geoTools()
{
    std::vector<Tool> tools;
    // ---- F0: GDAL algorithm bridge ----
    tools.push_back(Tool{
        "katana_gdal_catalogue", "GDAL algorithms",
        "The catalogue of GDAL's algorithms Katana can run (GDAL 3.13's raster, vector, mdim, "
        "dataset, vsi, driver and pipeline families): each one's path, description, aliases and "
        "policy - safe, or confirm for those that change or remove existing data. filter narrows "
        "it to a group (\"raster\", \"vector grid\") or to words in the names and descriptions; "
        "schemas adds each one's argument and input JSON Schemas when 20 or fewer are listed.",
        objectSchema(
            Json{{"filter",
                  {{"type", "string"},
                   {"description", "A group (raster, vector, vector grid ...) or words to look "
                                   "for, e.g. \"slope\"."}}},
                 {"schemas",
                  {{"type", "boolean"},
                   {"description", "Add each algorithm's arguments_schema and inputs_schema; "
                                   "honoured when the filter leaves 20 or fewer."}}}}),
        hints(true, false, true), catalogue});
    tools.push_back(Tool{
        "katana_gdal_describe", "Describe a GDAL algorithm",
        "One GDAL algorithm's arguments as data: name, short name, aliases, type, whether "
        "required or positional, default, choices, bounds with whether each is inclusive, list "
        "counts, and for a dataset argument its kinds and the Katana sources it can be bound "
        "from. arguments_schema and inputs_schema are the JSON Schemas of katana_gdal_run's "
        "arguments and inputs; gdal_usage is GDAL's own --json-usage.",
        objectSchema(Json{{"algorithm",
                           {{"type", "string"},
                            {"description", "The algorithm, e.g. \"raster hillshade\"; aliases "
                                            "are accepted (\"raster warp\")."}}}},
                     {"algorithm"}),
        hints(true, false, true), describe});
    tools.push_back(Tool{
        "katana_gdal_run", "Run a GDAL algorithm",
        "Run one GDAL algorithm on the live drawing, as the GDAL verb does: arguments by name "
        "(katana_gdal_describe's arguments_schema), GDAL's own words in tokens, datasets in "
        "inputs bound from the drawing by scope and filter ({scope: \"drawing\"|\"selection\"|"
        "\"area\"|\"layers\", area, layers, only, where}), from a reference raster ({raster}), a "
        "surface ({surface, cell}) or a file ({file, layer}), and the output to a layer "
        "({layer}), a reference raster ({reference}) or a file ({file, format, overwrite}). "
        "Without output, features go to layer gis/<algorithm> and rasters become a reference "
        "raster. A result in the drawing is one undoable step. Algorithms whose policy is "
        "confirm (they change or remove existing data) run only with confirm: true. preview "
        "checks the arguments and reports what the scopes took, changing nothing. Returns the "
        "records as structured data: what each scope matched and used, what was created.",
        objectSchema(
            Json{{"algorithm",
                  {{"type", "string"}, {"description", "The algorithm, e.g. \"vector buffer\"."}}},
                 {"arguments",
                  {{"type", "object"},
                   {"description", "Argument values by name, e.g. {\"distance\": 5, "
                                   "\"endcap-style\": \"flat\"}; see arguments_schema."}}},
                 {"tokens",
                  {{"type", "array"},
                   {"items", {{"type", "string"}}},
                   {"description", "GDAL's own command-line words, as `gdal <algorithm>` takes "
                                   "them, e.g. [\"--zfactor\", \"2\"]."}}},
                 {"inputs",
                  {{"type", "object"},
                   {"description", "Dataset arguments by name, each a source or a list of "
                                   "sources; see inputs_schema."}}},
                 {"output",
                  {{"type", "object"},
                   {"description", "{layer: \"gis/buffer\"} | {reference: \"shade\"} | {file: "
                                   "path, format?, overwrite?} | {surface: name}."}}},
                 {"confirm",
                  {{"type", "boolean"},
                   {"description", "Required true for an algorithm whose policy is confirm."}}},
                 {"preview",
                  {{"type", "boolean"},
                   {"description", "Check and report the sources, run nothing (default "
                                   "false)."}}}},
            {"algorithm"}),
        hints(false, true, false, true), run});
    // ---- T0: katana_terrain_list ----
    tools.push_back(Tool{
        "katana_terrain_list", "Surfaces and rasters",
        "The session's named surfaces (TIN, made by SURFACE FROM or TO SURFACE) and its reference "
        "rasters: what CONTOUR, RASTER SHADE, RASTER SLOPE and a GDAL run's FROM SURFACE <name> "
        "or FROM RASTER <id|name> read. Each surface's triangles, points, bounds [x0,y0,x1,y1], "
        "zmin, zmax, plan area and source; each raster's id, name, role (imagery, elevation or "
        "derived), size, square cell, coordinate system, file and the line that derived it.",
        objectSchema(Json::object()), hints(true, false, true), terrainList});
    // ---- V5: katana_gis_query ----
    tools.push_back(gisQueryTool());
    // ---- I2: katana_formats ----
    // Read-only, as katana_gdal_catalogue is: it reads the registry through
    // the verb's own chooser and records (formats_verbs.hpp), so the tool,
    // FORMATS and the katana://formats resource cannot come to differ.
    tools.push_back(Tool{
        "katana_formats", "GDAL formats",
        "The formats this build of GDAL reads and writes, from its own registry: each driver's "
        "name (what EXPORT's and GDAL's FORMAT take), description, the kinds of data it holds, "
        "reads and writes (raster, vector), its extensions, and whether it opens /vsi paths "
        "(inside a .zip, over https). kind and capability narrow the list (\"vector\", "
        "\"write\"); filter keeps drivers whose name, description or extensions hold every word. "
        "driver gives that one driver's open, creation and layer-creation options instead: "
        "each option's name, type, default, choices, bounds and description, what IMPORT's and "
        "EXPORT's driver options are checked against.",
        objectSchema(
            Json{{"kind",
                  {{"type", "string"},
                   {"enum", Json::array({"raster", "vector"})},
                   {"description", "Only drivers of this kind of data."}}},
                 {"capability",
                  {{"type", "string"},
                   {"enum", Json::array({"read", "write"})},
                   {"description", "Only drivers that read, or write, that kind."}}},
                 {"filter",
                  {{"type", "string"},
                   {"description", "Words each of which the driver's name, description or an "
                                   "extension holds, e.g. \"parquet\"."}}},
                 {"driver",
                  {{"type", "string"},
                   {"description", "One driver's options instead of the list, e.g. \"GPKG\"."}}}}),
        hints(true, false, true),
        [](Session&, const Json& arguments) -> ToolReply {
            if (const Json& driver = argument(arguments, "driver"); !driver.is_null()) {
                if (!driver.is_string()) {
                    throw ToolRefusal{"\"driver\" is a driver's name, as FORMATS lists it"};
                }
                const katana::gis::Format* format =
                    katana::gis::findFormat(driver.get<std::string>());
                if (format == nullptr) {
                    throw ToolRefusal{"this GDAL build has no format named \"" +
                                      driver.get<std::string>() + "\"; katana_formats lists them"};
                }
                auto options = katana::gis::formatOptions(format->driver);
                if (!options) {
                    throw ToolRefusal{options.error().describe()};
                }
                return ToolReply{geo::optionsRecords(*format, *options),
                                 geo::formatOptionsJson(*format, *options)};
            }
            geo::FormatsQuery query;
            const auto choice = [&arguments](const char* name, const char* first,
                                             const char* second) -> int {
                const Json& given = argument(arguments, name);
                if (given.is_null()) {
                    return 0;
                }
                if (given.is_string() && given.get<std::string>() == first) {
                    return 1;
                }
                if (given.is_string() && given.get<std::string>() == second) {
                    return 2;
                }
                throw ToolRefusal{std::string("\"") + name + "\" is \"" + first + "\" or \"" +
                                  second + "\""};
            };
            if (const int kind = choice("kind", "raster", "vector"); kind != 0) {
                query.kind = kind == 1 ? katana::gis::DataKind::Raster : katana::gis::DataKind::Vector;
            }
            if (const int capability = choice("capability", "read", "write"); capability != 0) {
                query.capability = capability == 1 ? geo::FormatsQuery::Capability::Read
                                                   : geo::FormatsQuery::Capability::Write;
            }
            if (const Json& filter = argument(arguments, "filter"); !filter.is_null()) {
                if (!filter.is_string()) {
                    throw ToolRefusal{"\"filter\" is text"};
                }
                query.text = filter.get<std::string>();
            }
            Json formats = Json::array();
            std::string text;
            const auto chosen = geo::chooseFormats(query);
            for (const katana::gis::Format* format : chosen) {
                formats.push_back(geo::formatJson(*format));
                text += geo::formatRecord(*format) + "\n";
            }
            text += geo::formatsSummary(query, chosen.size());
            return ToolReply{text, Json{{"gdal_version", gp::versions().gdal},
                                        {"formats", std::move(formats)}}};
        }});
    // ---- D1: katana_dataset_info ----
    tools.push_back(Tool{
        "katana_dataset_info", "Describe a data file",
        "What a GIS file, a folder or a point cloud holds, without importing it - the INFO "
        "verb's records as structured data: dataset (kind, driver, CRS), raster (size, cell, "
        "bounds), band (type, no-data, and with stats the minimum, maximum, mean and standard "
        "deviation, computed without writing beside the file), layer (features, geometry, CRS, "
        "bounds), field (name, type, width), subdataset, pointcloud; a folder gives a found "
        "record per dataset. check reads every value and reports check and problem records. "
        "json adds GDAL's own info JSON (raster info, vector info, mdim info) as gdal. path may "
        "be a /vsi path or a URL.",
        objectSchema(
            Json{{"path", {{"type", "string"}, {"description", "The file, folder, /vsi path or URL."}}},
                 {"layer",
                  {{"type", "string"}, {"description", "Only this vector layer."}}},
                 {"stats",
                  {{"type", "boolean"},
                   {"description", "Compute every band's statistics from every pixel."}}},
                 {"check",
                  {{"type", "boolean"},
                   {"description", "Read every value, and report what could not be read."}}},
                 {"json",
                  {{"type", "boolean"},
                   {"description", "Add GDAL's own info JSON as \"gdal\" (default false)."}}}},
            {"path"}),
        hints(true, false, true, true), datasetInfo});
    // ---- D2: katana_references ----
    tools.push_back(Tool{
        "katana_references", "Reference layers",
        "The reference layers the drawing is worked on top of - rasters and point clouds - "
        "listed, or one acted on by its id (or name): show, hide, remove, info, opacity (value "
        "0 to 1, a raster's), color (value elevation, intensity, classification, rgb or flat, a "
        "point cloud's), rename (value the new name), overviews (writes .ovr overviews beside "
        "the raster's file; value the levels, e.g. \"2,4,8\"; only with confirm: true), or "
        "restore (read again the layers the project records, as opening it does). The project "
        "records each layer's source and display when it is saved. Returns the action's "
        "records and the layers after it, each with its id; missing lists the layers the "
        "project names whose files could not be read.",
        objectSchema(
            Json{{"action",
                  {{"type", "string"},
                   {"enum", {"list", "show", "hide", "remove", "info", "opacity", "color",
                             "rename", "overviews", "restore"}},
                   {"description", "What to do; list by default."}}},
                 {"id",
                  {{"type", {"integer", "string"}},
                   {"description", "The layer's id, as the list gives it, or its name."}}},
                 {"value",
                  {{"type", {"number", "string"}},
                   {"description", "The opacity, the colouring, the new name, or the overview "
                                   "levels."}}},
                 {"confirm",
                  {{"type", "boolean"},
                   {"description", "Required true for overviews, which write beside the "
                                   "raster's file."}}}}),
        hints(false, true, false), references});
    return tools;
}

} // namespace katana::app::mcp
