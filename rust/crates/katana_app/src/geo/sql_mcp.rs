//! katana_gis_query (docs/mcp.md, "V5"): an agent's question of the drawing
//! in one call - "the easement area per owner" - answered as typed JSON rows.
//! The port of src/katana_app/geo/sql_mcp.cpp.
//!
//! Like every tool that runs a verb, it builds the line a person would type -
//! GIS SQL "<select>" <scope> dialect=<d> - and runs it through the Session,
//! then reads the reply's records back: the column records give each column
//! its type, so a 7 comes back as 7 and a "7" as "7", and a column a row has
//! no cell for is null.

use katana_core::json::Json;
use katana_core::text::{parse_finite_double, parse_integer};

use super::replies::parse_records;
use super::vector_support::sql_for_line;
use crate::mcp::tools::{
    Tool, ToolRefusal, ToolReply, argument, hints, lowered_text, object_schema, optional_bool,
    property, required_string, run_captured, scope_words_of, words,
};
use crate::session::Session;

// The scope as the one grammar's words, as katana_gdal_run writes them.
fn scope_words(arguments: &Json) -> Result<String, ToolRefusal> {
    let kind = argument(arguments, "scope");
    if !kind.is_null() && !kind.is_string() {
        return Err(ToolRefusal::new(
            "\"scope\" is drawing, selection, area or layers",
        ));
    }
    scope_words_of(
        kind.as_str().unwrap_or("drawing"),
        argument(arguments, "area"),
        argument(arguments, "layers"),
        optional_bool(arguments, "only", false)?,
        argument(arguments, "where"),
    )
}

// A cell as JSON, by its column's type.
fn typed(text: &str, kind: &str) -> Json {
    match kind {
        "integer" => {
            if let Some(whole) = parse_integer(text) {
                return Json::from(whole);
            }
        }
        "real" => {
            if let Some(real) = parse_finite_double(text) {
                return Json::from(real);
            }
        }
        "boolean" => return Json::from(text == "true" || text == "1"),
        _ => {}
    }
    Json::from(text)
}

fn query(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let asked = required_string(arguments, "sql")?;
    let dialect = argument(arguments, "dialect")
        .as_str()
        .map_or_else(|| String::from("sqlite"), lowered_text);
    if dialect != "sqlite" && dialect != "ogrsql" {
        return Err(ToolRefusal::new(
            "\"dialect\" is sqlite (with Spatialite's functions) or ogrsql",
        ));
    }
    // One word of a line: SQLite's "identifiers" become [identifiers], which
    // it reads alike (vector_support's sql_for_line, the dialog's too).
    let sql = sql_for_line(&asked, dialect == "sqlite")
        .map_err(|error| ToolRefusal::new(error.message))?;
    let line = format!(
        "GIS SQL \"{sql}\" {} dialect={dialect}",
        scope_words(arguments)?
    );
    let outcome = run_captured(session, &line);
    let mut columns = Vec::new();
    let mut types = Vec::new();
    // key -> column name
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut key_types: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    let mut result = Json::object_of([
        ("ok", Json::from(outcome.ok)),
        ("line", Json::from(&line)),
        ("matched", Json::Null),
        ("used", Json::Null),
    ]);
    for record in parse_records(&outcome.output) {
        match record.kind.as_str() {
            "column" => {
                let name = String::from(record.get("name").unwrap_or(""));
                columns.push(Json::from(&name));
                types.push(Json::from(record.get("type").unwrap_or("string")));
                keys.push((String::from(record.get("key").unwrap_or(&name)), name));
                key_types.push(String::from(record.get("type").unwrap_or("string")));
            }
            "row" => {
                let mut row = Json::object();
                for ((key, column), kind) in keys.iter().zip(&key_types) {
                    row[column.as_str()] = match record.get(key) {
                        Some(cell) => typed(cell, kind),
                        None => Json::Null,
                    };
                }
                rows.push(row);
            }
            "scope" => {
                for key in ["matched", "used"] {
                    if let Some(count) = parse_integer(record.get(key).unwrap_or("")) {
                        result[key] = Json::from(count);
                    }
                }
            }
            _ => {}
        }
    }
    result["columns"] = Json::from(columns);
    result["column_types"] = Json::from(types);
    result["rows"] = Json::from(rows);
    if !outcome.messages.is_empty() {
        result["messages"] = Json::from(&outcome.messages);
    }
    let mut text = format!("> {line}");
    if !outcome.output.is_empty() {
        text.push('\n');
        text += &outcome.output;
    }
    if !outcome.messages.is_empty() {
        text.push('\n');
        text += &outcome.messages;
    }
    Ok(ToolReply {
        text,
        structured: result,
        is_error: !outcome.ok,
    })
}

/// katana_gis_query: the drawing's question in SQL, answered as typed rows.
pub(super) fn gis_query_tool() -> Tool {
    Tool {
        name: "katana_gis_query",
        title: "Query the drawing with SQL",
        description: concat!(
            "Ask the live drawing a question in SQL and get the answer as typed JSON rows: ",
            "\"SELECT owner, SUM(ST_Area(geometry)) AS area FROM polygons GROUP BY owner\" in one ",
            "call. The scope (default: the whole drawing) becomes three tables - points, lines, ",
            "polygons - each with katana_id, layer, style, colour and type, then every property as ",
            "a typed column; the geometry column is geometry. The dialect is sqlite (SQLite with ",
            "Spatialite's ST_ functions; the default) or ogrsql. Only a SELECT runs, and it changes ",
            "nothing. Write an identifier holding a dot in double quotes or [brackets]: ",
            "\"gis.source\". Returns {columns, column_types, rows, matched, used}."
        ),
        input_schema: object_schema(
            Json::object_of([
                (
                    "sql",
                    property(
                        Json::from("string"),
                        "One SELECT statement over points, lines and polygons.",
                        [],
                    ),
                ),
                (
                    "scope",
                    property(
                        Json::from("string"),
                        "What becomes the tables (default drawing).",
                        [("enum", words(["drawing", "selection", "area", "layers"]))],
                    ),
                ),
                (
                    "area",
                    property(
                        Json::from("array"),
                        "scope area: [x0, y0, x1, y1].",
                        [
                            ("items", Json::object_of([("type", Json::from("number"))])),
                            ("minItems", Json::from(4)),
                            ("maxItems", Json::from(4)),
                        ],
                    ),
                ),
                (
                    "layers",
                    property(
                        Json::from("array"),
                        "scope layers: the layer paths.",
                        [("items", Json::object_of([("type", Json::from("string"))]))],
                    ),
                ),
                (
                    "only",
                    property(
                        Json::from("boolean"),
                        "scope layers: without their sublayers.",
                        [],
                    ),
                ),
                (
                    "where",
                    property(
                        Json::from("array"),
                        "The shared filter: [\"TYPE=polyline\", \"PROP=owner:Smith*\"].",
                        [("items", Json::object_of([("type", Json::from("string"))]))],
                    ),
                ),
                (
                    "dialect",
                    property(
                        Json::from("string"),
                        "sqlite (default) or ogrsql.",
                        [("enum", words(["sqlite", "ogrsql"]))],
                    ),
                ),
            ]),
            &["sql"],
        ),
        annotations: hints(true, false, true, false),
        call: query,
    }
}
