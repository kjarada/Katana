// katana_gis_query (docs/mcp.md, "V5"): an agent's question of the drawing
// in one call - "the easement area per owner" - answered as typed JSON rows.
//
// Like every tool that runs a verb, it builds the line a person would type -
// GIS SQL "<select>" <scope> dialect=<d> - and runs it through the Session,
// then reads the reply's records back: the column records give each column
// its type, so a 7 comes back as 7 and a "7" as "7", and a column a row has
// no cell for is null.

#include <algorithm>
#include <string>
#include <utility>
#include <vector>

#include "../mcp_tools.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/text.hpp"
#include "replies.hpp"
#include "vector_support.hpp"

namespace katana::app::mcp {

namespace {

namespace geo = katana::app::geo;

// The scope as the one grammar's words, as katana_gdal_run writes them.
std::string scopeWords(const Json& arguments)
{
    katana::cad::ScopeWords scope;
    const Json& kind = argument(arguments, "scope");
    const std::string name = kind.is_string() ? kind.get<std::string>() : std::string("drawing");
    if (!kind.is_null() && !kind.is_string()) {
        throw ToolRefusal{"\"scope\" is drawing, selection, area or layers"};
    }
    if (name == "drawing") {
        scope.source = katana::cad::ScopeSource::Drawing;
    } else if (name == "selection") {
        scope.source = katana::cad::ScopeSource::Selection;
    } else if (name == "area") {
        const Json& area = argument(arguments, "area");
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
    } else if (name == "layers") {
        const Json& layers = argument(arguments, "layers");
        if (!layers.is_array() || layers.empty() ||
            !std::ranges::all_of(layers, [](const Json& l) { return l.is_string(); })) {
            throw ToolRefusal{"scope layers needs \"layers\": [\"a\", \"b\"]"};
        }
        scope.source = katana::cad::ScopeSource::Layers;
        for (const Json& layer : layers) {
            scope.layers.push_back(layer.get<std::string>());
        }
        scope.sublayers = !optionalBool(arguments, "only", false);
    } else {
        throw ToolRefusal{"\"scope\" is drawing, selection, area or layers (VIEW is the window's)"};
    }
    if (const Json& where = argument(arguments, "where"); !where.is_null()) {
        if (!where.is_array()) {
            throw ToolRefusal{"\"where\" is a list of conditions: [\"TYPE=polyline\"]"};
        }
        for (const Json& condition : where) {
            if (!condition.is_string()) {
                throw ToolRefusal{"each \"where\" condition is a string"};
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

// A cell as JSON, by its column's type.
Json typed(const std::string& text, const std::string& type)
{
    if (type == "integer") {
        if (const auto whole = katana::core::parseInteger(text)) {
            return *whole;
        }
    } else if (type == "real") {
        if (const auto real = katana::core::parseFiniteDouble(text)) {
            return *real;
        }
    } else if (type == "boolean") {
        return text == "true" || text == "1";
    }
    return text;
}

ToolReply query(Session& session, const Json& arguments)
{
    const std::string asked = requiredString(arguments, "sql");
    const Json& dialectArgument = argument(arguments, "dialect");
    const std::string dialect =
        dialectArgument.is_string() ? katana::core::lowered(dialectArgument.get<std::string>())
                                    : std::string("sqlite");
    if (dialect != "sqlite" && dialect != "ogrsql") {
        throw ToolRefusal{"\"dialect\" is sqlite (with Spatialite's functions) or ogrsql"};
    }
    // One word of a line: SQLite's "identifiers" become [identifiers], which
    // it reads alike (vector::sqlForLine, the dialog's too).
    auto sql = geo::vector::sqlForLine(asked, dialect == "sqlite");
    if (!sql) {
        throw ToolRefusal{sql.error().message};
    }
    const std::string line = "GIS SQL \"" + *sql + "\" " + scopeWords(arguments) +
                             " dialect=" + dialect;
    const LineOutcome outcome = runCaptured(session, line);
    Json columns = Json::array();
    Json types = Json::array();
    std::vector<std::pair<std::string, std::string>> keys; // key -> column name
    std::vector<std::string> keyTypes;
    Json rows = Json::array();
    Json result{{"ok", outcome.ok}, {"line", line}, {"matched", nullptr}, {"used", nullptr}};
    for (const geo::Record& record : geo::parseRecords(outcome.output)) {
        if (record.kind == "column") {
            const std::string name = record.get("name").value_or("");
            columns.push_back(name);
            types.push_back(record.get("type").value_or("string"));
            keys.emplace_back(record.get("key").value_or(name), name);
            keyTypes.push_back(record.get("type").value_or("string"));
        } else if (record.kind == "row") {
            Json row = Json::object();
            for (std::size_t k = 0; k < keys.size(); ++k) {
                const auto cell = record.get(keys[k].first);
                row[keys[k].second] = cell ? typed(*cell, keyTypes[k]) : Json(nullptr);
            }
            rows.push_back(std::move(row));
        } else if (record.kind == "scope") {
            for (const char* key : {"matched", "used"}) {
                if (const auto count = katana::core::parseInteger(record.get(key).value_or(""))) {
                    result[key] = *count;
                }
            }
        }
    }
    result["columns"] = columns;
    result["column_types"] = types;
    result["rows"] = rows;
    if (!outcome.messages.empty()) {
        result["messages"] = outcome.messages;
    }
    std::string text = "> " + line;
    if (!outcome.output.empty()) {
        text += "\n" + outcome.output;
    }
    if (!outcome.messages.empty()) {
        text += "\n" + outcome.messages;
    }
    return ToolReply{text, result, !outcome.ok};
}

} // namespace

Tool gisQueryTool()
{
    return Tool{
        "katana_gis_query", "Query the drawing with SQL",
        "Ask the live drawing a question in SQL and get the answer as typed JSON rows: "
        "\"SELECT owner, SUM(ST_Area(geometry)) AS area FROM polygons GROUP BY owner\" in one "
        "call. The scope (default: the whole drawing) becomes three tables - points, lines, "
        "polygons - each with katana_id, layer, style, colour and type, then every property as "
        "a typed column; the geometry column is geometry. The dialect is sqlite (SQLite with "
        "Spatialite's ST_ functions; the default) or ogrsql. Only a SELECT runs, and it changes "
        "nothing. Write an identifier holding a dot in double quotes or [brackets]: "
        "\"gis.source\". Returns {columns, column_types, rows, matched, used}.",
        objectSchema(
            Json{{"sql",
                  {{"type", "string"},
                   {"description", "One SELECT statement over points, lines and polygons."}}},
                 {"scope",
                  {{"type", "string"},
                   {"enum", {"drawing", "selection", "area", "layers"}},
                   {"description", "What becomes the tables (default drawing)."}}},
                 {"area",
                  {{"type", "array"},
                   {"items", {{"type", "number"}}},
                   {"minItems", 4},
                   {"maxItems", 4},
                   {"description", "scope area: [x0, y0, x1, y1]."}}},
                 {"layers",
                  {{"type", "array"},
                   {"items", {{"type", "string"}}},
                   {"description", "scope layers: the layer paths."}}},
                 {"only",
                  {{"type", "boolean"},
                   {"description", "scope layers: without their sublayers."}}},
                 {"where",
                  {{"type", "array"},
                   {"items", {{"type", "string"}}},
                   {"description", "The shared filter: [\"TYPE=polyline\", \"PROP=owner:Smith*\"]."}}},
                 {"dialect",
                  {{"type", "string"},
                   {"enum", {"sqlite", "ogrsql"}},
                   {"description", "sqlite (default) or ogrsql."}}}},
            {"sql"}),
        hints(true, false, true), query};
}

} // namespace katana::app::mcp
