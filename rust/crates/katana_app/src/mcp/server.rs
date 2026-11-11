//! A Model Context Protocol server over a Session (session.rs): what lets
//! Claude - Claude Desktop, Claude Code, any MCP client - open a Katana
//! project, draw and edit it, run surveys' calculations, import and export,
//! and save it, with the very verbs a person types at katana_cli. The port of
//! src/katana_app/mcp_server.hpp/.cpp.
//!
//! The transport is MCP's stdio one: newline-delimited JSON-RPC 2.0 messages,
//! one per line, in on stdin and out on stdout (katana_mcp's main owns the
//! streams). This module only turns a message into its reply, so the tests
//! can drive it without a process. docs/mcp.md lists the tools and how to
//! connect a client.
//!
//! Every tool is a thin shape over Session::run: a tool builds command lines,
//! runs them, and hands back what the session printed. So there is one
//! implementation of every verb, the one katana_cli and its tests exercise,
//! and a tool can never do something the command line cannot. (Five tools run
//! no line and change nothing: katana_status, katana_customisation,
//! katana_road_objects and katana_leaders read the Document through cad, for
//! what no reply holds as structure, and katana_check_update runs the UPDATE
//! verbs' own CHECK and STATUS for an agent's surface - the session refuses
//! every UPDATE line here.)
//!
//! THE RUST SHAPE (docs/rust.md, step 6). The server holds no session: each
//! message is handled with the session lent for it (`Server::handle`), as the
//! interpreter is lent its drawing, so katana_mcp and the tests read the
//! drawing between messages. The C++ constructor told the session it was an
//! agent's (UpdateSurface::Mcp); here every message does, at the head of
//! `handle` - a session lent to the server is an agent's for that message,
//! whatever was done with it before, and nobody has a call to forget. What
//! the C++ catches as a std::exception - a nlohmann exception, or anything a
//! verb or a tool lets out - is a panic caught here: in a tool it is the
//! tool's error (`error: <what>`), and in a method JSON-RPC's -32603 with its
//! text.

use std::cmp::Ordering;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::sync::OnceLock;

use katana_cad::CommandInterpreter;
use katana_cad::annotation::leader_presets::{
    LeadersScopeReport, leaders_json_of, leaders_report, match_leaders,
};
use katana_cad::bim::bim_object::bim_model;
use katana_cad::customisation_report::customisation_json;
use katana_cad::document_status::{document_status, format_status, status_json};
use katana_cad::import_placement::{ImportPlacement, ImportPlacementMode, placement_word};
use katana_cad::road::road_object_verbs::{road_objects_report, road_objects_report_for};
use katana_cad::road::road_verbs::road_model_json;
use katana_cad::scope_verbs::{parse_scope_words, scope_record};
use katana_cad::terrain::surface_verbs::surface_styles_report;
use katana_core::json::{Json, JsonErrorKind};
use katana_core::text::format_exact_real;
use katana_entity::EntityId;
use katana_geometry::Vec2;

use super::customisation::customisation_tool;
use super::tools::{
    RunOptions, TOOL_STOPPED, Tool, ToolRefusal, ToolReply, argument, batch_reply,
    check_update_reply, hints, object_schema, one_line, optional_bool, property, quoted,
    required_string, run_captured, scope_argument_words, scope_words_of, status_of, thrown, words,
};
use crate::console::Terminal;
use crate::geo::gis_records::records_json;
use crate::geo::mcp_geo_tools::geo_tools;
use crate::session::Session;
use crate::session::panic_text;
use crate::update_verbs::{UpdateSurface, make_update_context};

/// The MCP revision this server is written to. A client asking for an older
/// one it also speaks (SUPPORTED_PROTOCOL_VERSIONS) gets it.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

/// Oldest first; LATEST_PROTOCOL_VERSION is the last.
const SUPPORTED_PROTOCOL_VERSIONS: [&str; 3] =
    ["2024-11-05", "2025-03-26", LATEST_PROTOCOL_VERSION];

// JSON-RPC 2.0's own error codes.
const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;
const INTERNAL_ERROR: i32 = -32603;
// MCP's "resource not found".
const RESOURCE_NOT_FOUND: i32 = -32002;

// A script or a batch longer than this is refused rather than run: a client
// that sends a million lines has a bug, and the reply would be unreadable.
const MOST_LINES: usize = 10000;

const INSTRUCTIONS: &str = concat!(
    "Katana is a civil engineering and surveying CAD engine. These tools drive one live ",
    "Katana session: a drawing (the Document) that is empty until you open a project ",
    "directory or draw into it. Every change is an undoable command, exactly as typed at ",
    "Katana's command line. Start with katana_status; katana_help lists every command and ",
    "its syntax. Points are x,y (easting,northing), @dx,dy relative or @dist<angle polar; ",
    "angles in degrees. Most edits act on the selection, so SELECT first - by id, or by ",
    "kind and filter across a scope: SELECT DRAWING WHERE TYPE=leader. Nothing is ",
    "written to disk until katana_save_project (or SAVE): open a COPY of a project you ",
    "care about, since opening can back up or migrate it in place."
);

// ---- tool arguments ------------------------------------------------------------------------

// One line per command: a command with a line break in it would be two, and
// the second would run unseen by whoever built the first.
fn single_line(text: &str, what: &str) -> Result<String, ToolRefusal> {
    if text.contains('\n') {
        return Err(ToolRefusal::new(format!(
            "{what} must be one line; send several as a list"
        )));
    }
    Ok(String::from(text))
}

// katana_import's placement, local and offsets as the word its IMPORT line
// ends in ("" to keep the data where it is), by cad's placement_word, so the
// line is the one a person would type.
fn import_placement_word(arguments: &Json) -> Result<String, ToolRefusal> {
    let local = optional_bool(arguments, "local", false)?;
    let chosen = argument(arguments, "placement");
    if chosen.is_null() {
        return Ok(String::from(if local { "LOCAL" } else { "" }));
    }
    static MODES: [(&str, ImportPlacementMode); 4] = [
        ("keep", ImportPlacementMode::Keep),
        ("local", ImportPlacementMode::Local),
        ("alongside", ImportPlacementMode::Alongside),
        ("offset", ImportPlacementMode::Offset),
    ];
    let found = chosen
        .as_str()
        .and_then(|name| MODES.iter().find(|(word, _)| *word == name));
    let Some(&(word, mode)) = found else {
        return Err(ToolRefusal::new(
            "\"placement\" must be keep, local, alongside or offset",
        ));
    };
    if local && mode != ImportPlacementMode::Local {
        return Err(ToolRefusal::new(format!(
            "\"local\": true and \"placement\": \"{word}\" say different things; give one"
        )));
    }
    let number = |name: &str| -> Result<f64, ToolRefusal> {
        let value = argument(arguments, name);
        if !value.is_number() || !value.get::<f64>()?.is_finite() {
            return Err(ToolRefusal::new(format!(
                "placement offset needs \"{name}\", a number"
            )));
        }
        Ok(value.get::<f64>()?)
    };
    let offset = if mode == ImportPlacementMode::Offset {
        Vec2::new(number("offset_east")?, number("offset_north")?)
    } else {
        Vec2::default()
    };
    Ok(placement_word(&ImportPlacement { mode, offset }))
}

// One key=value word of a line, quoted when the value holds a blank: refused
// for a double quote or a line break, which no line can carry.
fn option_word(key: &str, value: &str) -> Result<String, ToolRefusal> {
    if value.contains(['"', '\n']) {
        return Err(ToolRefusal::new(format!(
            "\"{key}\" may not hold a double quote or a line break (in SQLite, write an \
             identifier as [name])"
        )));
    }
    let blank = value.is_empty() || value.contains([' ', '\t']);
    Ok(if blank {
        format!(" {key}=\"{value}\"")
    } else {
        format!(" {key}={value}")
    })
}

// A list argument - ["a", "b"] or "a,b" - as the comma-joined value a line
// takes.
fn list_argument(value: &Json, name: &str) -> Result<String, ToolRefusal> {
    if let Some(text) = value.as_str() {
        return Ok(String::from(text));
    }
    if !value.is_array() || value.empty() || !value.iter().all(|each| each.is_string()) {
        return Err(ToolRefusal::new(format!("\"{name}\" is a list of names")));
    }
    let mut joined = String::new();
    for each in value.iter() {
        let item = each.get::<String>()?;
        if item.contains(',') {
            return Err(ToolRefusal::new(format!(
                "a name in \"{name}\" may not hold a comma"
            )));
        }
        if !joined.is_empty() {
            joined.push(',');
        }
        joined.push_str(&item);
    }
    Ok(joined)
}

// A whole number argument at least `low`, as text.
fn whole_argument(value: &Json, name: &str, low: i64) -> Result<String, ToolRefusal> {
    if !value.is_number_integer() || value.get::<i64>()? < low {
        return Err(ToolRefusal::new(format!(
            "\"{name}\" is a whole number from {low}"
        )));
    }
    Ok(format!("{}", value.get::<i64>()?))
}

// katana_import's options as the words of its IMPORT line (docs/mcp.md, "I3
// and I4"), each the word a person would type, so the line in the reply is
// one they could type again. The C++'s three lambdas, `text`, `list` and
// `whole`, are the three functions after it.
fn import_option_words(arguments: &Json) -> Result<String, ToolRefusal> {
    let mut words = String::new();
    list_option(&mut words, arguments, "layers", "layers")?;
    text_option(&mut words, arguments, "where", "where")?;
    text_option(&mut words, arguments, "sql", "sql")?;
    text_option(&mut words, arguments, "dialect", "dialect")?;
    list_option(&mut words, arguments, "fields", "fields")?;
    text_option(&mut words, arguments, "target_layer", "target")?;
    whole_option(&mut words, arguments, "max_features", "max", 1)?;
    let value = argument(arguments, "open_options");
    if !value.is_null() {
        if !value.is_array() || !value.iter().all(|each| each.is_string()) {
            return Err(ToolRefusal::new(
                "\"open_options\" is a list of \"KEY=VALUE\"",
            ));
        }
        for each in value.iter() {
            words += &option_word("oo", &each.get::<String>()?)?;
        }
    }
    let value = argument(arguments, "attributes");
    if !value.is_null() {
        let Some(attributes) = value.as_bool() else {
            return Err(ToolRefusal::new("\"attributes\" must be true or false"));
        };
        words += if attributes { "" } else { " attributes=no" };
    }
    text_option(&mut words, arguments, "crs", "crs")?;
    text_option(&mut words, arguments, "source_crs", "srs")?;
    whole_option(&mut words, arguments, "band", "band", 1)?;
    let value = argument(arguments, "subdataset");
    if !value.is_null() {
        let subdataset = if value.is_number_integer() {
            whole_argument(value, "subdataset", 1)?
        } else {
            String::from(value.as_str().unwrap_or(""))
        };
        words += &option_word("subdataset", &subdataset)?;
    }
    whole_option(&mut words, arguments, "max_pixels", "maxpixels", 1)?;
    text_option(&mut words, arguments, "name", "name")?;
    whole_option(&mut words, arguments, "budget", "budget", 1)?;
    let value = argument(arguments, "classes");
    if !value.is_null() {
        let classes = if value.is_number_integer() {
            whole_argument(value, "classes", 0)?
        } else {
            list_argument(value, "classes")?
        };
        words += &option_word("class", &classes)?;
    }
    let value = argument(arguments, "resolution");
    if !value.is_null() {
        if !value.is_number() || value.get::<f64>()?.partial_cmp(&0.0) != Some(Ordering::Greater) {
            return Err(ToolRefusal::new(
                "\"resolution\" is a point spacing above 0",
            ));
        }
        words += " resolution=";
        words += &format_exact_real(value.get::<f64>()?);
    }
    // The scope: `scope`, or an `area` alone.
    let scope = argument(arguments, "scope");
    let area = argument(arguments, "area");
    if !scope.is_null() || !area.is_null() {
        if !scope.is_null() && !scope.is_string() {
            return Err(ToolRefusal::new(
                "\"scope\" is selection, drawing, area or layers",
            ));
        }
        words.push(' ');
        words += &scope_words_of(
            scope.as_str().unwrap_or("area"),
            area,
            argument(arguments, "scope_layers"),
            optional_bool(arguments, "only", false)?,
            argument(arguments, "scope_where"),
        )?;
    }
    if optional_bool(arguments, "clip", false)? {
        words += " clip";
    }
    if optional_bool(arguments, "preview", false)? {
        words += " PREVIEW";
    }
    Ok(words)
}

// The C++'s `text`: a non-empty string option.
fn text_option(
    words: &mut String,
    arguments: &Json,
    name: &str,
    key: &str,
) -> Result<(), ToolRefusal> {
    let value = argument(arguments, name);
    if value.is_null() {
        return Ok(());
    }
    match value.as_str() {
        Some(text) if !text.is_empty() => {
            *words += &option_word(key, text)?;
            Ok(())
        }
        _ => Err(ToolRefusal::new(format!(
            "\"{name}\" must be a non-empty string"
        ))),
    }
}

// The C++'s `list`: a list of names, or the names comma-joined.
fn list_option(
    words: &mut String,
    arguments: &Json,
    name: &str,
    key: &str,
) -> Result<(), ToolRefusal> {
    let value = argument(arguments, name);
    if !value.is_null() {
        *words += &option_word(key, &list_argument(value, name)?)?;
    }
    Ok(())
}

// The C++'s `whole`: a whole number from `low`.
fn whole_option(
    words: &mut String,
    arguments: &Json,
    name: &str,
    key: &str,
    low: i64,
) -> Result<(), ToolRefusal> {
    let value = argument(arguments, name);
    if !value.is_null() {
        *words += &option_word(key, &whole_argument(value, name, low)?)?;
    }
    Ok(())
}

// katana_export's options as the words of its EXPORT line (docs/mcp.md, "I3
// and I4"): the scope first, as a person types it, then the options.
fn export_option_words(arguments: &Json) -> Result<String, ToolRefusal> {
    let mut words = String::new();
    let cloud = argument(arguments, "cloud");
    if !cloud.is_null() {
        if cloud.is_number_integer() {
            words += &format!(" CLOUD {}", cloud.get::<i64>()?);
        } else if let Some(name) = cloud.as_str()
            && !name.is_empty()
        {
            words += " CLOUD ";
            words += &quoted(name)?;
        } else {
            return Err(ToolRefusal::new(
                "\"cloud\" is a reference point cloud's id or name",
            ));
        }
    }
    if let Some(given) = scope_argument_words(arguments)? {
        words.push(' ');
        words += &given;
    }
    let name = argument(arguments, "layer_name");
    if !name.is_null() {
        match name.as_str() {
            Some(text) if !text.is_empty() => words += &option_word("layername", text)?,
            _ => {
                return Err(ToolRefusal::new(
                    "\"layer_name\" must be a non-empty string",
                ));
            }
        }
    }
    if optional_bool(arguments, "split_by_layer", false)? {
        words += " split=layer";
    }
    if optional_bool(arguments, "append", false)? {
        words += " append";
    }
    let crs = argument(arguments, "crs");
    if !crs.is_null() {
        match crs.as_str() {
            Some(text) if !text.is_empty() => words += &option_word("crs", text)?,
            _ => {
                return Err(ToolRefusal::new(
                    "\"crs\" is project, native or a code such as EPSG:4326",
                ));
            }
        }
    }
    for (name, key) in [
        ("creation_options", "co"),
        ("layer_creation_options", "lco"),
    ] {
        let value = argument(arguments, name);
        if !value.is_null() {
            if !value.is_array() || !value.iter().all(|each| each.is_string()) {
                return Err(ToolRefusal::new(format!(
                    "\"{name}\" is a list of \"KEY=VALUE\""
                )));
            }
            for each in value.iter() {
                words += &option_word(key, &each.get::<String>()?)?;
            }
        }
    }
    let text = argument(arguments, "text");
    if !text.is_null() {
        match text.as_str() {
            Some(choice @ ("points" | "skip")) => {
                words += " text=";
                words += choice;
            }
            _ => return Err(ToolRefusal::new("\"text\" is points or skip")),
        }
    }
    let curve = argument(arguments, "curve");
    if !curve.is_null() {
        if !curve.is_number() || curve.get::<f64>()?.partial_cmp(&0.0) != Some(Ordering::Greater) {
            return Err(ToolRefusal::new("\"curve\" is a distance above 0"));
        }
        words += " curve=";
        words += &format_exact_real(curve.get::<f64>()?);
    }
    let properties = argument(arguments, "properties");
    if !properties.is_null() {
        let Some(written) = properties.as_bool() else {
            return Err(ToolRefusal::new("\"properties\" must be true or false"));
        };
        words += if written {
            " properties=yes"
        } else {
            " properties=no"
        };
    }
    if optional_bool(arguments, "preview", false)? {
        words += " PREVIEW";
    }
    Ok(words)
}

fn string_list(value: &Json, name: &str) -> Result<Vec<String>, ToolRefusal> {
    if !value.is_array() || value.empty() {
        return Err(ToolRefusal::new(format!(
            "\"{name}\" must be a non-empty list of strings"
        )));
    }
    if value.size() > MOST_LINES {
        return Err(ToolRefusal::new(format!(
            "\"{name}\" has more than {MOST_LINES} entries; send them in parts"
        )));
    }
    let mut lines = Vec::new();
    for item in value.iter() {
        let Some(text) = item.as_str() else {
            return Err(ToolRefusal::new(format!(
                "every entry of \"{name}\" must be a string"
            )));
        };
        lines.push(single_line(text, "each command")?);
    }
    Ok(lines)
}

// ---- the replies of the tools that run lines ------------------------------------------------

// The reply of a tool whose line answers in records (IMPORT, EXPORT): the
// batch reply, and the records again as structured content, each an object
// of its fields (geo::gis_records), so an agent reads what came in - the
// entities, the reference layer's id, the move a placement made, each
// warning - without reading the text. (The Rust app always has GDAL: the
// C++'s arm without it, whose records are the text's alone, is not ported.)
fn records_reply(session: &mut Session, line: &str) -> Result<ToolReply, ToolRefusal> {
    let mut reply = one_line(session, line, &RunOptions::default());
    let records = match reply.structured["commands"]
        .as_array()
        .and_then(|commands| commands.first())
    {
        Some(first) if first.contains("output") => records_json(&first["output"].get::<String>()?),
        _ => Json::array(),
    };
    reply.structured["records"] = records;
    Ok(reply)
}

// ---- the tools ------------------------------------------------------------------------------

fn discard_property() -> Json {
    property(
        Json::from("boolean"),
        "Allow NEW or OPEN to throw away unsaved changes to the current drawing. Default false: \
         they are refused while there are unsaved changes.",
        [],
    )
}

// The schema of a list of strings: {"type": "array", "items": {"type": "string"}}.
fn strings_property(description: &str) -> Json {
    property(
        Json::from("array"),
        description,
        [("items", Json::object_of([("type", Json::from("string"))]))],
    )
}

// The schema of an area: four numbers.
fn area_property(description: &str) -> Json {
    property(
        Json::from("array"),
        description,
        [
            ("items", Json::object_of([("type", Json::from("number"))])),
            ("minItems", Json::from(4)),
            ("maxItems", Json::from(4)),
        ],
    )
}

fn run_options(arguments: &Json) -> Result<RunOptions, ToolRefusal> {
    Ok(RunOptions {
        stop_on_error: optional_bool(arguments, "stop_on_error", true)?,
        discard_unsaved_changes: optional_bool(arguments, "discard_unsaved_changes", false)?,
    })
}

fn discard_only(arguments: &Json) -> Result<RunOptions, ToolRefusal> {
    Ok(RunOptions {
        discard_unsaved_changes: optional_bool(arguments, "discard_unsaved_changes", false)?,
        ..RunOptions::default()
    })
}

fn run_commands(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let lines = string_list(argument(arguments, "commands"), "commands")?;
    let options = run_options(arguments)?;
    Ok(batch_reply(session, &lines, &options))
}

fn run_script(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let path = required_string(arguments, "path")?;
    // Read as a text file is everywhere (Session::read_script): the path is
    // UTF-8 from the JSON and was opened as an ANSI name, so a folder with an
    // accented letter was another folder, and the bytes of a script saved in
    // the ANSI code page reached the verbs as they were. A guess at the
    // encoding is said on the process's stderr, the server's log, as the
    // C++'s std::cerr says it outside the capture of any line.
    let script = Session::read_script(&path, &mut Terminal).map_err(|error| {
        ToolRefusal::new(format!(
            "cannot read the script {path}: {}",
            error.describe()
        ))
    })?;
    if script.len() > MOST_LINES {
        return Err(ToolRefusal::new(format!(
            "the script has more than {MOST_LINES} lines"
        )));
    }
    let options = run_options(arguments)?;
    Ok(batch_reply(session, &script, &options))
}

fn help(_session: &mut Session, _arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    Ok(ToolReply {
        text: Session::help_text(),
        structured: Json::Null,
        is_error: false,
    })
}

fn status(session: &mut Session, _arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let status = document_status(session.document());
    Ok(ToolReply {
        text: format_status(&status),
        structured: Json::parse(status_json(&status))?,
        is_error: false,
    })
}

fn new_project(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let options = discard_only(arguments)?;
    Ok(one_line(session, "NEW", &options))
}

fn open_project(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let line = format!("OPEN {}", quoted(&required_string(arguments, "path")?)?);
    let options = discard_only(arguments)?;
    Ok(one_line(session, &line, &options))
}

fn save_project(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    if argument(arguments, "path").is_null() {
        return Ok(one_line(session, "SAVE", &RunOptions::default()));
    }
    let line = format!("SAVE {}", quoted(&required_string(arguments, "path")?)?);
    Ok(one_line(session, &line, &RunOptions::default()))
}

fn list_entities(session: &mut Session, _arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    Ok(one_line(session, "LIST", &RunOptions::default()))
}

fn leaders(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let mut ids: Vec<EntityId> = Vec::new();
    if let Some(given) = arguments.find("ids") {
        if !given.is_array() {
            return Err(ToolRefusal::new("\"ids\" must be a list of entity ids"));
        }
        for id in given.iter() {
            if !id.is_number_integer() || id.get::<i64>()? < 1 {
                return Err(ToolRefusal::new(
                    "every entry of \"ids\" must be a positive whole number",
                ));
            }
            ids.push(id.get::<i64>()? as EntityId);
        }
    }
    let mut target: Option<EntityId> = None;
    if let Some(id) = arguments.find("target") {
        if !id.is_number_integer() || id.get::<i64>()? < 1 {
            return Err(ToolRefusal::new(
                "\"target\" must be a positive whole number",
            ));
        }
        target = Some(id.get::<i64>()? as EntityId);
    }
    // Which leaders: the ids, or what the scope and filter take. The scope is
    // read by the one reading of a scope in JSON (scope_words_of) and the one
    // grammar (parse_scope_words), and resolved by the one matcher; VIEW is
    // the window's, so none.
    let mut only: Option<Vec<EntityId>> = None;
    let mut scope_report: Option<LeadersScopeReport> = None;
    let scope_kind = argument(arguments, "scope");
    let area = argument(arguments, "area");
    let conditions = argument(arguments, "scope_where");
    if !scope_kind.is_null() || !area.is_null() || !conditions.is_null() {
        if !ids.is_empty() {
            return Err(ToolRefusal::new("give \"ids\" or a scope, not both"));
        }
        if !scope_kind.is_null() && !scope_kind.is_string() {
            return Err(ToolRefusal::new(
                "\"scope\" is selection, drawing, area or layers",
            ));
        }
        let kind = match scope_kind.as_str() {
            Some(kind) => kind,
            None if !area.is_null() => "area",
            None => "selection",
        };
        let scope_words = scope_words_of(
            kind,
            area,
            argument(arguments, "scope_layers"),
            optional_bool(arguments, "only", false)?,
            conditions,
        )?;
        let tokens = CommandInterpreter::tokenize(&scope_words)
            .map_err(|error| ToolRefusal::new(error.describe()))?;
        let mut at = 0;
        let scope = parse_scope_words(&tokens, &mut at)
            .map_err(|error| ToolRefusal::new(error.describe()))?;
        let found = match_leaders(session.document(), &scope, None)
            .map_err(|error| ToolRefusal::new(error.describe()))?;
        scope_report = Some(LeadersScopeReport {
            record: scope_record(&found),
            matched: found.matched.len(),
        });
        only = Some(found.matched);
    } else if !ids.is_empty() {
        only = Some(ids);
    }
    // From the Document, as katana_status is: a read leaves no line in the
    // session's history.
    let mut report = leaders_report(session.document(), only.as_deref(), target);
    report.scope = scope_report;
    let json = leaders_json_of(&report);
    Ok(ToolReply {
        structured: Json::parse(&json)?,
        text: json,
        is_error: false,
    })
}

fn describe_entity(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let id = argument(arguments, "id");
    if !id.is_number_integer() || id.get::<i64>()? < 1 {
        return Err(ToolRefusal::new("\"id\" must be a positive whole number"));
    }
    // #id: always the entity, whatever files the server's working directory
    // holds (session.rs, INFO).
    let line = format!("INFO #{}", id.get::<i64>()?);
    Ok(one_line(session, &line, &RunOptions::default()))
}

fn import(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let word = import_placement_word(arguments)?;
    let mut line = format!("IMPORT {}", quoted(&required_string(arguments, "path")?)?);
    if !word.is_empty() {
        line.push(' ');
        line += &word;
    }
    line += &import_option_words(arguments)?;
    records_reply(session, &line)
}

fn export(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let line = format!(
        "EXPORT {}{}",
        quoted(&required_string(arguments, "path")?)?,
        export_option_words(arguments)?
    );
    records_reply(session, &line)
}

fn undo(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let mut steps: i64 = 1;
    let value = argument(arguments, "steps");
    if !value.is_null() {
        if !value.is_number_integer() || value.get::<i64>()? < 1 {
            return Err(ToolRefusal::new(
                "\"steps\" must be a positive whole number",
            ));
        }
        steps = value.get::<i64>()?;
    }
    let redo = optional_bool(arguments, "redo", false)?;
    let line = format!("{} {steps}", if redo { "REDO" } else { "UNDO" });
    Ok(one_line(session, &line, &RunOptions::default()))
}

fn check_update(_session: &mut Session, _arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    Ok(check_update_reply(&make_update_context(UpdateSurface::Mcp)))
}

fn road_objects(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let Some(given) = scope_argument_words(arguments)? else {
        let report = road_objects_report(session.document());
        return Ok(ToolReply {
            structured: Json::parse(&report)?,
            text: report,
            is_error: false,
        });
    };
    let mut at = 0;
    let scope = CommandInterpreter::tokenize(&given)
        .and_then(|tokens| parse_scope_words(&tokens, &mut at))
        .map_err(|error| ToolRefusal::new(error.describe()))?;
    let report = road_objects_report_for(session.document(), &scope, None)
        .map_err(|error| ToolRefusal::new(error.describe()))?;
    Ok(ToolReply {
        structured: Json::parse(&report)?,
        text: report,
        is_error: false,
    })
}

// Every tool, in the order tools/list gives them: made once, the first time a
// message asks, and kept for the process.
fn tools() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        let mut list = vec![
            Tool {
                name: "katana_run_commands",
                title: "Run Katana commands",
                description: concat!(
                    "Run one or more Katana command-line commands, in order, against the live drawing - ",
                    "the same commands a person types at Katana's command line (katana_help lists them): ",
                    "draw (LINE, PLINE, RECT, CIRCLE, ARC, POINT, TEXT, DIM), select and modify (SELECT, ",
                    "MOVE, COPY, ROTATE, OFFSET, TRIM, FILLET...), layers and styles, alignments and ",
                    "design profiles (ALIGN), parcels (PARCEL), survey calculations (INVERSE, FORWARD, ",
                    "AREA), survey field files (SURVEY READ <file>; SURVEY IMPORT <file> [SETTINGS ",
                    "<file>] [SET key=value ...] [CODES on|off] [LINEWORK on|off], where SET ",
                    "control=<id>;drawing;fixed;0;fixed;0;fixed;0 ",
                    "holds a point of the drawing, as FORWARD puts one there, and an import codes ",
                    "and strings the points it draws by the loaded survey codes unless told off), ",
                    "survey codes (CODE, CODE LIST, CODE CHECK; LINEWORK [<scope>] [WHERE key=value ",
                    "...] [PROPERTY <name>] [ORDER number|entity] [CHORD <length>] [PREVIEW] joins ",
                    "coded points into lines, HELP ",
                    "LINEWORK) and the customisation they come from (CUSTOMISE <file> ",
                    "loads a Katana customisation file, CUSTOMISE EXPORT <file> writes the session as ",
                    "one, CUSTOMISE alone reports what is loaded; HELP CUSTOMISE), AS 5488 subsurface ",
                    "utilities ",
                    "(UTILITY REPORT, VERIFY, CLEARANCE, CHECK, DRAW, REGRADE, SCHEDULE - on a schedule ",
                    "file, or on what is drawn by the scope words DRAWING | SELECTION | ",
                    "AREA x0,y0,x1,y1 | LAYERS a,b [ONLY], then [WHERE key=value ...]; UTILITY DRAW ",
                    "<scope> METHOD <method> [TYPE <type>] [H_UNC <m>] [FIELDS column=property,...] ",
                    "draws the lines, polylines and points a survey or an import left in the drawing ",
                    "as services; HELP UTILITY), ",
                    "global modify (MODIFY <scope> [WHERE key=value ...] SET key=value ...), ",
                    "inspection (LIST, INFO), UNDO/REDO, ",
                    "and files (NEW, OPEN, SAVE, IMPORT, EXPORT). Each command is one undoable step. By ",
                    "default the batch stops at the first command that fails. Returns each command's ",
                    "output and the drawing's status afterwards. PLOT, PLOTSHEETS, SNAPSHOT, ONLINE, ",
                    "SCRIPT, GRID, EXAGGERATION and ZOOM, and VIEWS except SCALE, are the desktop window's verbs and are ",
                    "refused here, saying so: plotting, view pictures, online data and the views need ",
                    "the window (or katana run headless with --command); a script file is ",
                    "katana_run_script's. VIEWS SCALE id AUTO|1:N edits a saved plan view's paper ",
                    "linestyle and symbol scale independently of zoom and ANNOSCALE. ",
                    "UPDATE lines are refused here too, so that this tool reaches ",
                    "no network: katana_check_update is the tool that checks for a newer Katana."
                ),
                input_schema: object_schema(
                    Json::object_of([
                        (
                            "commands",
                            property(
                                Json::from("array"),
                                "Commands, one per entry, e.g. [\"LAYER NEW Kerb #FF0000\", \
                                 \"LAYER SET Kerb\", \"PLINE 0,0 30,0 30,20 CLOSE\"]",
                                [
                                    ("items", Json::object_of([("type", Json::from("string"))])),
                                    ("minItems", Json::from(1)),
                                ],
                            ),
                        ),
                        (
                            "stop_on_error",
                            property(
                                Json::from("boolean"),
                                "Stop at the first failing command (default true).",
                                [],
                            ),
                        ),
                        ("discard_unsaved_changes", discard_property()),
                    ]),
                    &["commands"],
                ),
                annotations: hints(false, true, false, false),
                call: run_commands,
            },
            Tool {
                name: "katana_run_script",
                title: "Run a Katana script file",
                description: concat!(
                    "Run a Katana command script (.kcs): one command per line, a line whose first ",
                    "non-blank is '#' a comment - the file katana_cli runs. Stops at the first failing ",
                    "line unless stop_on_error is false."
                ),
                input_schema: object_schema(
                    Json::object_of([
                        (
                            "path",
                            property(Json::from("string"), "The script file to run.", []),
                        ),
                        (
                            "stop_on_error",
                            property(
                                Json::from("boolean"),
                                "Stop at the first failing line (default true).",
                                [],
                            ),
                        ),
                        ("discard_unsaved_changes", discard_property()),
                    ]),
                    &["path"],
                ),
                annotations: hints(false, true, false, false),
                call: run_script,
            },
            Tool {
                name: "katana_help",
                title: "Katana command reference",
                description: concat!(
                    "The reference of every Katana command and its syntax: points, drawing, editing, ",
                    "selection, layers, linetypes, hatches, styles, alignments, parcels, survey ",
                    "calculations and codes, properties, history, files and import/export."
                ),
                input_schema: object_schema(Json::object(), &[]),
                annotations: hints(true, false, true, false),
                call: help,
            },
            Tool {
                name: "katana_status",
                title: "Drawing status",
                description: concat!(
                    "The live drawing's state: its project directory (if saved to one), whether it has ",
                    "unsaved changes, how many entities, layers and alignments it holds, the current ",
                    "layer and style, the selection and the undo history."
                ),
                input_schema: object_schema(Json::object(), &[]),
                annotations: hints(true, false, true, false),
                call: status,
            },
            Tool {
                name: "katana_new_project",
                title: "New drawing",
                description: concat!(
                    "Start a new, empty drawing. Refused while the current one has unsaved changes unless ",
                    "discard_unsaved_changes is true."
                ),
                input_schema: object_schema(
                    Json::object_of([("discard_unsaved_changes", discard_property())]),
                    &[],
                ),
                annotations: hints(false, true, false, false),
                call: new_project,
            },
            Tool {
                name: "katana_open_project",
                title: "Open a Katana project",
                description: concat!(
                    "Open a Katana project directory (the folder holding project.db) as the live ",
                    "drawing. Opening may back up or migrate the project in place, so open a copy of one ",
                    "that matters. Refused while the current drawing has unsaved changes unless ",
                    "discard_unsaved_changes is true."
                ),
                input_schema: object_schema(
                    Json::object_of([
                        (
                            "path",
                            property(Json::from("string"), "The project directory to open.", []),
                        ),
                        ("discard_unsaved_changes", discard_property()),
                    ]),
                    &["path"],
                ),
                annotations: hints(false, true, false, false),
                call: open_project,
            },
            Tool {
                name: "katana_save_project",
                title: "Save the drawing",
                description: concat!(
                    "Save the live drawing. With a path, save it as a project in that directory (created ",
                    "if need be) and carry on in it; without, save it where it was opened or last saved."
                ),
                input_schema: object_schema(
                    Json::object_of([(
                        "path",
                        property(
                            Json::from("string"),
                            "The project directory to save to. Omit to save in place.",
                            [],
                        ),
                    )]),
                    &[],
                ),
                annotations: hints(false, false, true, false),
                call: save_project,
            },
            Tool {
                name: "katana_list_entities",
                title: "List entities",
                description: concat!(
                    "List every entity in the drawing with its id, type, layer and key measurements ",
                    "(length, area, radius...). Ids are what SELECT, INFO and the edit commands take."
                ),
                input_schema: object_schema(Json::object(), &[]),
                annotations: hints(true, false, true, false),
                call: list_entities,
            },
            // ---- leaders (docs/annotation.md, "Presets"; docs/mcp.md, "Leaders") ----
            Tool {
                name: "katana_leaders",
                title: "Leaders and what they say",
                description: concat!(
                    "The drawing's leaders as structured data, each with what its note says NOW - read ",
                    "live off the entity its tip is on and, for a preset or a template naming them, off ",
                    "the drawing's surfaces and alignments: leaders [{id, layer, tip [x, y], vertices, ",
                    "target (the entity id the tip is on, or null), anchor, smart, template, labelstyle, ",
                    "preset (level, surface, cutfill, name, code, details or station when the template ",
                    "is that preset's), reads (the surface or alignment it reads), arrow, callout, text, ",
                    "lines, points [[x,y],...], text_style, paper_height, arrow_size, landing, ",
                    "optional landing_angle (radians)}], presets [{name, title, reads (target, surface or alignment), template, ",
                    "tool, says}], surfaces [...], alignments [...]. What LEADER LIST JSON prints. It ",
                    "reads only. To EDIT one use katana_run_commands: LEADER SET id vertex=N at=x,y ",
                    "moves zero-based point N (at alone moves the note end); callout=box|circle|none ",
                    "frames the note, and landingangle=auto|degrees chooses the landing direction. ",
                    "To MAKE one use katana_run_commands: LEADER #<id> <x,y> ",
                    "preset=level makes a leader saying the point's level (RL); preset=surface ",
                    "[surface=<name>] its height above (+) or below (-) a surface; preset=cutfill the ",
                    "cut or the fill to a surface; preset=name its name (point number); preset=code its ",
                    "survey code; preset=details all three; preset=station [alignment=<name>] its ",
                    "station and offset (left positive). A surface or alignment not named is the first ",
                    "visible surface or the first alignment, said as chosen= in the reply. LEADER FOR ",
                    "<ids>|SELECTION preset=... makes one to each entity in one undo step. A template ",
                    "may name {surface.NAME} {dsurface.NAME} {cut.NAME} {fill.NAME} {station.NAME} ",
                    "{offset.NAME} {profile.NAME} {dprofile.NAME} itself (katana_help, under Leader). ",
                    "Which leaders: all of them; or \"ids\"; or the ones a scope and filter take - the ",
                    "same grammar as Global Modify, MODIFY and the UTILITY verbs: \"scope\" selection, ",
                    "drawing, area (with \"area\") or layers (with \"scope_layers\" and \"only\"), and ",
                    "\"scope_where\" conditions - and then the reply also carries scope {record ",
                    "(scope=layers layers=... matched=N), matched}, N counting the leaders the scope ",
                    "took; a scope that takes none gives leaders [] and matched 0, not an error. ",
                    "\"target\" narrows either."
                ),
                input_schema: object_schema(
                    Json::object_of([
                        (
                            "ids",
                            property(
                                Json::from("array"),
                                "Only these leaders (by entity id); all when left out. Not with a \
                                 scope.",
                                [(
                                    "items",
                                    Json::object_of([
                                        ("type", Json::from("integer")),
                                        ("minimum", Json::from(1)),
                                    ]),
                                )],
                            ),
                        ),
                        (
                            "target",
                            property(
                                Json::from("integer"),
                                "Only the leaders whose tip is on this entity.",
                                [("minimum", Json::from(1))],
                            ),
                        ),
                        (
                            "scope",
                            property(
                                Json::from("string"),
                                "Only the leaders this takes of the drawing (the shared scope \
                                 grammar); area with \"area\", layers with \"scope_layers\" and \
                                 \"only\". VIEW is the window's. \"scope_where\" alone filters the \
                                 selection.",
                                [("enum", words(["selection", "drawing", "area", "layers"]))],
                            ),
                        ),
                        (
                            "area",
                            area_property(
                                "[x0, y0, x1, y1] in the drawing's coordinates: scope area (given \
                                 alone, it is the scope).",
                            ),
                        ),
                        (
                            "scope_layers",
                            strings_property("With scope layers: the drawing's layers."),
                        ),
                        (
                            "only",
                            property(
                                Json::from("boolean"),
                                "With scope layers: without their sublayers.",
                                [],
                            ),
                        ),
                        (
                            "scope_where",
                            strings_property(
                                "Conditions on what the scope takes: [\"LAYER=notes\", \
                                 \"PROP=owner:Smith*\"].",
                            ),
                        ),
                    ]),
                    &[],
                ),
                annotations: hints(true, false, true, false),
                call: leaders,
            },
            Tool {
                name: "katana_describe_entity",
                title: "Describe an entity",
                description: "Everything about one entity: its geometry, layer, attributes and properties.",
                input_schema: object_schema(
                    Json::object_of([(
                        "id",
                        property(
                            Json::from("integer"),
                            "The entity id, as katana_list_entities shows.",
                            [("minimum", Json::from(1))],
                        ),
                    )]),
                    &["id"],
                ),
                annotations: hints(true, false, true, false),
                call: describe_entity,
            },
            Tool {
                name: "katana_import",
                title: "Import a file",
                description: concat!(
                    "Import a file into the drawing: DXF always; with the GIS module also shapefiles, ",
                    "GeoJSON, GeoPackage, .12da archives and other vector formats (as entities), and ",
                    "rasters and point clouds (as reference layers). placement says where what a DXF, ",
                    "vector file or .12da archive holds lands: at its own coordinates (keep, the ",
                    "default), moved as one piece so its lower-left corner sits at 0,0 (local), onto ",
                    "the drawing's lower-left corner (alongside), or by offset_east and offset_north ",
                    "(offset). The reply is records: imported (what came in, its extent and CRS), placed ",
                    "(the move made), reference or surface (a layer or surface added, with its id), ",
                    "tally and warning - in structuredContent.records as objects."
                ),
                input_schema: object_schema(import_properties(), &["path"]),
                annotations: hints(false, false, false, false),
                call: import,
            },
            Tool {
                name: "katana_export",
                title: "Export the drawing",
                description: concat!(
                    "Export the drawing to a file, its format chosen by the extension: .dxf always; with ",
                    "the GIS module also GIS vector formats and .12da archives (with the session's ",
                    "surfaces). The reply is an exported record - the driver, how many features or ",
                    "entities were written and how many skipped - and any warnings, in ",
                    "structuredContent.records as objects."
                ),
                input_schema: object_schema(export_properties(), &["path"]),
                annotations: hints(false, true, true, false),
                call: export,
            },
            Tool {
                name: "katana_undo",
                title: "Undo",
                description: "Undo the last steps (default one); redo: true redoes them instead.",
                input_schema: object_schema(
                    Json::object_of([
                        (
                            "steps",
                            property(
                                Json::from("integer"),
                                "How many steps (default 1).",
                                [("minimum", Json::from(1))],
                            ),
                        ),
                        (
                            "redo",
                            property(
                                Json::from("boolean"),
                                "Redo instead of undo (default false).",
                                [],
                            ),
                        ),
                    ]),
                    &[],
                ),
                annotations: hints(false, false, false, false),
                call: undo,
            },
            // The one thing of the update system an agent has (update_verbs,
            // UpdateSurface::Mcp): asking, and reading this copy's status. It
            // runs no session line - an UPDATE line is refused in every tool
            // that runs lines - so that katana_run_commands can go on telling
            // its client it reaches no network, while this tool says that it
            // does. It writes nothing: the Mcp surface keeps no per-user state.
            Tool {
                name: "katana_check_update",
                title: "Check for a Katana update",
                description: concat!(
                    "Ask the release server whether a newer Katana than this copy has been released, ",
                    "and report this copy's update status. Read-only: it downloads nothing, installs ",
                    "nothing and writes no file. An update is downloaded and installed by a person, in ",
                    "the Katana window (Help > Check for Updates) or with katana_cli (UPDATE DOWNLOAD ",
                    "CONFIRM, then UPDATE APPLY CONFIRM as the last line of a run) - never by an agent, ",
                    "so the program this client holds open is never replaced under it, and UPDATE lines ",
                    "sent to katana_run_commands are refused. It reaches the network: the release feed ",
                    "is asked, and what it is sent of this copy is the program's version and platform ",
                    "as the user agent. One thing more is sent in one case: when the environment ",
                    "variable KATANA_UPDATE_TOKEN of this server's run holds an access token and the ",
                    "public address answers that there is no such release (a private release ",
                    "repository), that token is sent to api.github.com, and to no other host. A ",
                    "development copy (a build tree) asks nobody and answers ",
                    "update=unsupported reason=development_tree. The reply is two records, in ",
                    "structuredContent as the objects check and status. check: update=available, ",
                    "current, unreachable or unsupported, with installed, latest, files ",
                    "(changed/total), download_bytes, notes_url, and reason and detail when there is ",
                    "something to say. status: installed, target, kind=installer, archive or ",
                    "development, staged, skipped, auto, last_check, token=present or absent (never ",
                    "the token) and last. No per-user setting is read here, so staged, skipped, auto, ",
                    "last_check and last are what a copy with none says, and what a person decided ",
                    "about a release is not applied: a version they skipped, and a release that was ",
                    "installed on this machine, did not start and was put back - which the window and ",
                    "katana_cli answer as update=blocked and no longer offer - are both reported ",
                    "available here. update=blocked is never answered by this tool."
                ),
                input_schema: object_schema(Json::object(), &[]),
                annotations: hints(true, false, true, true),
                call: check_update,
            },
            // The customisation as structured data, from its own file
            // (mcp::customisation).
            customisation_tool(),
            // ---- road objects (docs/road_objects.md) ----
            // Read from the Document, as the customisation is, so asking
            // leaves no line in the history of a session nobody typed into;
            // placing and editing are katana_run_commands' ROADOBJECT lines.
            Tool {
                name: "katana_road_objects",
                title: "Road objects",
                description: concat!(
                    "The drawing's road objects - safety barriers, guide posts, signs, light poles, ",
                    "culverts, walls, fences and line markings placed along an alignment, along lines or ",
                    "at points - as structured data: each definition as the drawing keeps it (id, kind, ",
                    "placement, the parameters given), the 3D objects made of it (id, IFC class and ",
                    "predefined type, layer, material, triangles, whether closed, and quantities such as ",
                    "Length, Height, Count, GrossArea and GrossVolume), and what could not be made and ",
                    "why. Place and edit them with katana_run_commands: ROADOBJECT ADD barrier ",
                    "ALIGNMENT Main station=0 to=120 offset=4, ROADOBJECT ADD sign AT x,y,z code=R1-1, ",
                    "ROADOBJECT ADD fence ALONG FROM LAYERS boundary, ROADOBJECT SET id key=value, ",
                    "ROADOBJECT DELETE id; ROADOBJECT KINDS <kind> lists a kind's keys, units, ranges ",
                    "and defaults (HELP ROADOBJECT). With a scope it reports only the objects the scope ",
                    "and filter take, as ROADOBJECT LIST <scope> does, and says what it took in ",
                    "\"scope\": its record, how many it matched, and how many it would have taken but ",
                    "for not being made (they have no plan extent, which an area asks for)."
                ),
                input_schema: object_schema(
                    Json::object_of([
                        (
                            "scope",
                            property(
                                Json::from("string"),
                                "Which road objects (the shared scope grammar); all of them when \
                                 neither this nor \"area\" nor \"where\" is given. selection takes \
                                 the objects placed along a selected line; area those whose plan \
                                 extent meets the box; layers those on the layers (and beneath them \
                                 unless \"only\").",
                                [("enum", words(["selection", "drawing", "area", "layers"]))],
                            ),
                        ),
                        (
                            "area",
                            area_property("[x0, y0, x1, y1]: scope area (given alone, the scope)."),
                        ),
                        (
                            "layers",
                            strings_property("With scope layers: the drawing's layers."),
                        ),
                        (
                            "only",
                            property(
                                Json::from("boolean"),
                                "With scope layers: without their sublayers.",
                                [],
                            ),
                        ),
                        (
                            "where",
                            strings_property(
                                "Conditions on the objects the scope takes: \
                                 [\"PROP=kind:barrier\", \"LAYER=road/objects/*\"]. \
                                 PROP=key:pattern reads the keys SET writes (kind, id, type, \
                                 offset, station ...); COLOUR=#RRGGBB the colour drawn; DRAWN a \
                                 layer that is shown. TYPE, STYLE and TEXT are refused. Without \
                                 \"scope\" they filter the selection, as WHERE alone does on the \
                                 command line; \"scope\": \"drawing\" filters all.",
                            ),
                        ),
                    ]),
                    &[],
                ),
                annotations: hints(true, false, true, false),
                call: road_objects,
            },
            // ---- end road objects ----
        ];
        // The geoprocessing tools, from their own file (geo::mcp_geo_tools).
        list.extend(geo_tools());
        list
    })
}

fn import_properties() -> Json {
    Json::object_of([
        (
            "path",
            property(Json::from("string"), "The file to import.", []),
        ),
        (
            "placement",
            property(
                Json::from("string"),
                "Where the data lands, moved as one piece with its shape and dimensions \
                 unchanged: keep - its own coordinates; local - its lower-left corner at 0,0 \
                 (IMPORT ... LOCAL); alongside - its lower-left corner on the drawing's \
                 (ALONGSIDE; into an empty drawing it keeps its own); offset - moved by \
                 offset_east and offset_north (OFFSET=dE,dN). Anything but keep is refused for \
                 rasters and point clouds, which are drawn at their own coordinates.",
                [("enum", words(["keep", "local", "alongside", "offset"]))],
            ),
        ),
        (
            "offset_east",
            property(
                Json::from("number"),
                "With placement offset: added to every easting.",
                [],
            ),
        ),
        (
            "offset_north",
            property(
                Json::from("number"),
                "With placement offset: added to every northing.",
                [],
            ),
        ),
        (
            "local",
            property(
                Json::from("boolean"),
                "The same as placement local: move the imported data as one piece so its \
                 lower-left corner sits at 0,0 (IMPORT ... LOCAL). Refused for rasters and point \
                 clouds, which are drawn at their own coordinates.",
                [],
            ),
        ),
        (
            "layers",
            strings_property("Only these of the file's layers, by name (layers=)."),
        ),
        (
            "where",
            property(
                Json::from("string"),
                "An OGR SQL WHERE clause on the file's features, applied by the driver: kind = \
                 'lot' (where=).",
                [],
            ),
        ),
        (
            "sql",
            property(
                Json::from("string"),
                "A SELECT run on the file, whose rows are imported in place of its layers, as \
                 one layer (sql=). No double quotes: SQLite reads [identifier] alike.",
                [],
            ),
        ),
        (
            "dialect",
            property(
                Json::from("string"),
                "The dialect of sql; the driver's own by default.",
                [("enum", words(["ogrsql", "sqlite"]))],
            ),
        ),
        (
            "scope",
            property(
                Json::from("string"),
                "Only the file's features in the box of what this takes of the drawing (the \
                 shared scope grammar); area with \"area\", layers with \"scope_layers\" and \
                 \"only\"; \"scope_where\" filters what it takes. The box goes to the driver as \
                 its spatial filter.",
                [("enum", words(["selection", "drawing", "area", "layers"]))],
            ),
        ),
        (
            "area",
            area_property(
                "[x0, y0, x1, y1] in the drawing's coordinates: scope area (given alone, it is \
                 the scope).",
            ),
        ),
        (
            "scope_layers",
            strings_property("With scope layers: the drawing's layers."),
        ),
        (
            "only",
            property(
                Json::from("boolean"),
                "With scope layers: without their sublayers.",
                [],
            ),
        ),
        (
            "scope_where",
            strings_property("Conditions on what the scope takes: [\"TYPE=polyline\"]."),
        ),
        (
            "clip",
            property(
                Json::from("boolean"),
                "Cut features at the scope's edge (an area) or by the closed shapes it takes \
                 (clip).",
                [],
            ),
        ),
        (
            "fields",
            strings_property("Only these attributes become properties (fields=)."),
        ),
        (
            "attributes",
            property(
                Json::from("boolean"),
                "false: no attributes become properties (attributes=no).",
                [],
            ),
        ),
        (
            "target_layer",
            property(
                Json::from("string"),
                "Every entity on this layer (target=).",
                [],
            ),
        ),
        (
            "max_features",
            property(
                Json::from("integer"),
                "At most this many features (max=).",
                [("minimum", Json::from(1))],
            ),
        ),
        (
            "open_options",
            strings_property(
                "The driver's open options, [\"KEY=VALUE\"], each checked against the driver's \
                 own list (oo=).",
            ),
        ),
        (
            "crs",
            property(
                Json::from("string"),
                "project: vector data moved into the project's coordinate system; adopt: the \
                 project takes the file's when it has none (crs=).",
                [("enum", words(["project", "adopt"]))],
            ),
        ),
        (
            "source_crs",
            property(
                Json::from("string"),
                "The coordinate system of a file that declares none, EPSG:28356 (srs=).",
                [],
            ),
        ),
        (
            "band",
            property(
                Json::from("integer"),
                "A raster: this band alone, as grey (band=).",
                [("minimum", Json::from(1))],
            ),
        ),
        (
            "subdataset",
            property(
                words(["integer", "string"]),
                "A raster: the dataset inside a container, by number or name (subdataset=).",
                [],
            ),
        ),
        (
            "max_pixels",
            property(
                Json::from("integer"),
                "A raster: the display copy's longest side (maxpixels=).",
                [("minimum", Json::from(1))],
            ),
        ),
        (
            "name",
            property(
                Json::from("string"),
                "A raster or a point cloud: the reference layer's name.",
                [],
            ),
        ),
        (
            "budget",
            property(
                Json::from("integer"),
                "A point cloud: the points kept (budget=).",
                [("minimum", Json::from(1))],
            ),
        ),
        (
            "classes",
            property(
                Json::from("integer"),
                "A point cloud: only this ASPRS class (class=).",
                [("minimum", Json::from(0)), ("maximum", Json::from(255))],
            ),
        ),
        (
            "resolution",
            property(
                Json::from("number"),
                "A COPC point cloud: read its octree to this point spacing.",
                [],
            ),
        ),
        (
            "preview",
            property(
                Json::from("boolean"),
                "Read with the filters and say how many features matched of how many, importing \
                 nothing (PREVIEW).",
                [],
            ),
        ),
    ])
}

fn export_properties() -> Json {
    Json::object_of([
        (
            "path",
            property(
                Json::from("string"),
                "The file to write, e.g. site.dxf.",
                [],
            ),
        ),
        (
            "scope",
            property(
                Json::from("string"),
                "What of the drawing is written (the shared scope grammar); the whole drawing \
                 when neither this nor \"area\" is given. area with \"area\", layers with \
                 \"layers\" and \"only\"; \"where\" filters it.",
                [("enum", words(["selection", "drawing", "area", "layers"]))],
            ),
        ),
        (
            "area",
            area_property("[x0, y0, x1, y1]: scope area (given alone, the scope)."),
        ),
        (
            "layers",
            strings_property("With scope layers: the drawing's layers."),
        ),
        (
            "only",
            property(
                Json::from("boolean"),
                "With scope layers: without their sublayers.",
                [],
            ),
        ),
        (
            "where",
            strings_property(
                "Conditions on what the scope takes: [\"TYPE=polyline\", \
                 \"PROP=owner:Smith*\"]. Without \"scope\" they filter the selection, as WHERE \
                 alone does on the command line; \"scope\": \"drawing\" filters the whole \
                 drawing.",
            ),
        ),
        (
            "layer_name",
            property(
                Json::from("string"),
                "The layer's name in the file (layername=).",
                [],
            ),
        ),
        (
            "split_by_layer",
            property(
                Json::from("boolean"),
                "One file layer per drawing layer, named after it (split=layer).",
                [],
            ),
        ),
        (
            "append",
            property(
                Json::from("boolean"),
                "Add the layer to the file (a GeoPackage) rather than replace it; a layer name \
                 the file has is refused (append).",
                [],
            ),
        ),
        (
            "crs",
            property(
                Json::from("string"),
                "project (the default; a GeoJSON is then longitude and latitude, as RFC 7946 \
                 says), native (the project's, unconverted, where the format allows), or a code \
                 the coordinates are moved into (crs=).",
                [],
            ),
        ),
        (
            "creation_options",
            strings_property(
                "The driver's creation options, [\"KEY=VALUE\"], each checked against its list \
                 (co=).",
            ),
        ),
        (
            "layer_creation_options",
            strings_property("The driver's layer creation options, checked likewise (lco=)."),
        ),
        (
            "text",
            property(
                Json::from("string"),
                "points: text entities as points with text, text_height and text_rotation \
                 fields and an OGR_STYLE label; skip (the default) leaves them out, counted.",
                [("enum", words(["points", "skip"]))],
            ),
        ),
        (
            "curve",
            property(
                Json::from("number"),
                "The largest distance a chord may stray from an arc (curve=).",
                [],
            ),
        ),
        (
            "properties",
            property(
                Json::from("boolean"),
                "false: entity properties are not written.",
                [],
            ),
        ),
        (
            "cloud",
            property(
                words(["integer", "string"]),
                "With a .las or .laz path: the reference point cloud to write, by id or name \
                 (CLOUD); the one there is when absent. The points held are written, a budgeted \
                 import's sample included, which the reply says.",
                [],
            ),
        ),
        (
            "preview",
            property(
                Json::from("boolean"),
                "Say what would be written and what the scope takes, writing nothing (PREVIEW).",
                [],
            ),
        ),
    ])
}

// ---- resources -----------------------------------------------------------------------------

struct Resource {
    uri: &'static str,
    name: &'static str,
    description: &'static str,
    mime_type: &'static str,
}

const RESOURCES: [Resource; 6] = [
    Resource {
        uri: "katana://help",
        name: "Katana command reference",
        description: "Every command and its syntax.",
        mime_type: "text/plain",
    },
    Resource {
        uri: "katana://status",
        name: "Drawing status",
        description: "The live drawing's state, as katana_status reports it.",
        mime_type: "application/json",
    },
    // CUSTOMISE JSON (docs/customisation.md, "The replies"), as
    // katana_customisation's summary gives it.
    Resource {
        uri: "katana://customisation",
        name: "Customisation",
        description: concat!(
            "The session's customisation: its name and where it came from, its sources and ",
            "their notices, counts, linework codes, automation, colours, what is wrong with ",
            "its rules, what this drawing uses of it, and what went wrong when the session ",
            "started."
        ),
        mime_type: "application/json",
    },
    // ---- the drawing's surface display styles (docs/terrain.md, docs/mcp.md) ----
    // SURFACESTYLE LIST JSON: the styles a surface is drawn in, read from the
    // Document as the customisation is.
    Resource {
        uri: "katana://surface-styles",
        name: "Surface display styles",
        description: concat!(
            "The drawing's surface display styles - what each shows in plan and in 3D, its ",
            "colours, contours, ramp, bands and stops - and which is the default: what ",
            "SURFACESTYLE LIST JSON gives, in the form SURFACESTYLE NEW and SET change."
        ),
        mime_type: "application/json",
    },
    // ---- road model (docs/road_design.md) ----
    // ROADMODEL JSON: every 3D object of the road model - id, class, layer,
    // quantities, properties - and what could not be built.
    Resource {
        uri: "katana://road-model",
        name: "Road model",
        description: concat!(
            "The road model's 3D objects - each course, kerb and earthworks solid of every ",
            "corridor and every placed road object - with their IFC class, layer, quantities ",
            "and properties, and the problems that kept anything from being built."
        ),
        mime_type: "application/json",
    },
    // FORMATS JSON (docs/interop.md, "Formats"), as katana_formats lists it.
    Resource {
        uri: "katana://formats",
        name: "GDAL formats",
        description: "The formats this build of GDAL reads and writes: driver, kinds, extensions, /vsi.",
        mime_type: "application/json",
    },
];

// ---- JSON-RPC ------------------------------------------------------------------------------

fn reply(id: &Json, result: Json) -> String {
    Json::object_of([
        ("jsonrpc", Json::from("2.0")),
        ("id", id.clone()),
        ("result", result),
    ])
    .dump()
}

fn error_reply(id: &Json, code: i32, message: &str) -> String {
    Json::object_of([
        ("jsonrpc", Json::from("2.0")),
        ("id", id.clone()),
        (
            "error",
            Json::object_of([("code", Json::from(code)), ("message", Json::from(message))]),
        ),
    ])
    .dump()
}

// Every schema says additionalProperties: false, but a client need not check
// it: an argument the tool does not take (a misspelt "split" for
// "split_by_layer") was ignored, and the call ran without the option asked
// for. Refused here, naming it and the arguments there are.
fn refuse_undeclared(tool: &Tool, arguments: &Json) -> Result<(), ToolRefusal> {
    let none = Json::Null;
    let properties = tool.input_schema.find("properties").unwrap_or(&none);
    for (given, _) in arguments.items() {
        if properties.is_object() && properties.contains(&given) {
            continue;
        }
        let mut known = String::new();
        if properties.is_object() {
            for (declared, _) in properties.items() {
                if !known.is_empty() {
                    known += ", ";
                }
                known += &declared;
            }
        }
        return Err(ToolRefusal::new(format!(
            "{} takes no argument \"{given}\"{}",
            tool.name,
            if known.is_empty() {
                String::from(": it takes none")
            } else {
                format!("; it takes {known}")
            }
        )));
    }
    Ok(())
}

/// The MCP server: what a message is answered with. It holds no session; one
/// is lent for each message.
#[derive(Clone, Debug)]
pub struct Server {
    version: String,
    protocol_version: String,
}

impl Server {
    /// `version` is what serverInfo reports, the program's own version.
    pub fn new(version: impl Into<String>) -> Server {
        Server {
            version: version.into(),
            protocol_version: String::from(LATEST_PROTOCOL_VERSION),
        }
    }

    /// The protocol revision agreed at initialize, or the latest before it.
    pub fn protocol_version(&self) -> &str {
        &self.protocol_version
    }

    /// One message in, its reply out: None for a notification, which MCP
    /// never answers. A message that is not JSON-RPC is answered with a
    /// JSON-RPC error, as the specification requires. The message is bytes,
    /// read as JSON is (so one that is not UTF-8 is nlohmann's parse error);
    /// `session` is the drawing it acts on, an agent's: every UPDATE line is
    /// refused in it, naming katana_check_update (session.rs).
    pub fn handle(&mut self, session: &mut Session, message: impl AsRef<[u8]>) -> Option<String> {
        session.set_update_surface(UpdateSurface::Mcp);
        self.handle_bytes(session, message.as_ref())
    }

    fn handle_bytes(&mut self, session: &mut Session, message: &[u8]) -> Option<String> {
        let request = match Json::parse(message) {
            Ok(request) => request,
            Err(error) if error.kind == JsonErrorKind::ParseError => {
                return Some(error_reply(
                    &Json::Null,
                    PARSE_ERROR,
                    &format!("not JSON: {}", error.what),
                ));
            }
            // Only a parse_error is caught there: nlohmann's out_of_range of a
            // number past the largest double leaves the C++'s handle, as a
            // panic leaves this one.
            Err(error) => panic_any(error.what),
        };

        // A batch (MCP 2025-03-26 allowed them): each member answered in turn,
        // notifications not at all, and nothing sent when only they were in it.
        if request.is_array() {
            if request.empty() {
                return Some(error_reply(&Json::Null, INVALID_REQUEST, "an empty batch"));
            }
            let mut replies = String::new();
            for member in request.iter() {
                if let Some(one) = self.handle_bytes(session, member.dump().as_bytes()) {
                    replies.push(if replies.is_empty() { '[' } else { ',' });
                    replies += &one;
                }
            }
            if replies.is_empty() {
                return None;
            }
            replies.push(']');
            return Some(replies);
        }

        // nlohmann's value() throws for a "jsonrpc" that is not a string, and
        // here nothing catches it: it leaves handle in the C++ too.
        if !request.is_object()
            || thrown(request.value("jsonrpc", String::new())) != "2.0"
            || !request.contains("method")
            || !request["method"].is_string()
        {
            let id = match request.find("id") {
                Some(id) if request.is_object() => id.clone(),
                _ => Json::Null,
            };
            return Some(error_reply(
                &id,
                INVALID_REQUEST,
                "not a JSON-RPC 2.0 request",
            ));
        }
        let method = thrown(request["method"].get::<String>());
        let is_notification = !request.contains("id");
        let id = if is_notification {
            Json::Null
        } else {
            request["id"].clone()
        };
        let params = match request.find("params") {
            Some(params) if params.is_object() => params.clone(),
            _ => Json::object(),
        };

        if is_notification {
            // notifications/initialized, notifications/cancelled: nothing to
            // do, since every request is answered before the next is read.
            return None;
        }

        match catch_unwind(AssertUnwindSafe(|| {
            self.answer(session, &method, &id, &params)
        })) {
            Ok(answer) => Some(answer),
            Err(payload) => Some(error_reply(
                &id,
                INTERNAL_ERROR,
                &panic_text(&*payload, TOOL_STOPPED),
            )),
        }
    }

    // A request's reply. What the C++ throws out of it (and catches as a
    // std::exception) is a panic here, caught by handle.
    fn answer(&mut self, session: &mut Session, method: &str, id: &Json, params: &Json) -> String {
        match method {
            "initialize" => {
                // The revision asked for when this server speaks it, else the
                // latest.
                let asked = thrown(params.value("protocolVersion", String::new()));
                self.protocol_version = if SUPPORTED_PROTOCOL_VERSIONS.contains(&asked.as_str()) {
                    asked
                } else {
                    String::from(LATEST_PROTOCOL_VERSION)
                };
                reply(
                    id,
                    Json::object_of([
                        ("protocolVersion", Json::from(&self.protocol_version)),
                        (
                            "capabilities",
                            Json::object_of([
                                (
                                    "tools",
                                    Json::object_of([("listChanged", Json::from(false))]),
                                ),
                                (
                                    "resources",
                                    Json::object_of([("listChanged", Json::from(false))]),
                                ),
                            ]),
                        ),
                        (
                            "serverInfo",
                            Json::object_of([
                                ("name", Json::from("katana")),
                                ("title", Json::from("Katana")),
                                ("version", Json::from(&self.version)),
                            ]),
                        ),
                        ("instructions", Json::from(INSTRUCTIONS)),
                    ]),
                )
            }
            "ping" => reply(id, Json::object()),
            "tools/list" => {
                let list = tools()
                    .iter()
                    .map(|tool| {
                        Json::object_of([
                            ("name", Json::from(tool.name)),
                            ("title", Json::from(tool.title)),
                            ("description", Json::from(tool.description)),
                            ("inputSchema", tool.input_schema.clone()),
                            ("annotations", tool.annotations.clone()),
                        ])
                    })
                    .collect::<Vec<Json>>();
                reply(id, Json::object_of([("tools", Json::from(list))]))
            }
            "tools/call" => self.call_tool(session, id, params),
            "resources/list" => {
                let list = RESOURCES
                    .iter()
                    .map(|resource| {
                        Json::object_of([
                            ("uri", Json::from(resource.uri)),
                            ("name", Json::from(resource.name)),
                            ("description", Json::from(resource.description)),
                            ("mimeType", Json::from(resource.mime_type)),
                        ])
                    })
                    .collect::<Vec<Json>>();
                reply(id, Json::object_of([("resources", Json::from(list))]))
            }
            "resources/templates/list" => {
                reply(id, Json::object_of([("resourceTemplates", Json::array())]))
            }
            "resources/read" => read_resource(session, id, params),
            "prompts/list" => reply(id, Json::object_of([("prompts", Json::array())])),
            _ => error_reply(id, METHOD_NOT_FOUND, &format!("no method {method}")),
        }
    }

    fn call_tool(&self, session: &mut Session, id: &Json, params: &Json) -> String {
        let Some(name) = params.find("name").and_then(Json::as_str) else {
            return error_reply(id, INVALID_PARAMS, "tools/call needs a tool name");
        };
        let Some(tool) = tools().iter().find(|tool| tool.name == name) else {
            return error_reply(id, INVALID_PARAMS, &format!("no tool is called {name}"));
        };
        let arguments = match params.find("arguments") {
            Some(arguments) if arguments.is_object() => arguments.clone(),
            _ => Json::object(),
        };
        let called = catch_unwind(AssertUnwindSafe(|| -> Result<ToolReply, ToolRefusal> {
            refuse_undeclared(tool, &arguments)?;
            (tool.call)(session, &arguments)
        }));
        let result = match called {
            Ok(Ok(reply)) => reply,
            Ok(Err(refusal)) => ToolReply {
                text: format!("error: {}", refusal.message),
                structured: Json::Null,
                is_error: true,
            },
            Err(payload) => ToolReply {
                text: format!("error: {}", panic_text(&*payload, TOOL_STOPPED)),
                structured: Json::Null,
                is_error: true,
            },
        };
        let mut content = Json::object_of([
            (
                "content",
                Json::array_of([Json::object_of([
                    ("type", Json::from("text")),
                    ("text", Json::from(&result.text)),
                ])]),
            ),
            ("isError", Json::from(result.is_error)),
        ]);
        // Structured content arrived in 2025-06-18; an older client has the
        // same facts in the text.
        if !result.structured.is_null() && self.protocol_version.as_str() >= "2025-06-18" {
            content["structuredContent"] = result.structured;
        }
        reply(id, content)
    }
}

fn read_resource(session: &mut Session, id: &Json, params: &Json) -> String {
    let uri = thrown(params.value("uri", String::new()));
    let (text, mime_type) = match uri.as_str() {
        "katana://help" => (Session::help_text(), "text/plain"),
        "katana://status" => (
            status_of(session).dump_with(2, ' ', false),
            "application/json",
        ),
        // cad's own report, read from the Document as the status is: a line
        // would answer the same and leave a CUSTOMISE JSON in the history of a
        // session nobody had typed into.
        "katana://customisation" => (customisation_json(session.document()), "application/json"),
        // SURFACESTYLE LIST JSON's own form, standing_in note and all (cad's
        // terrain::surface_verbs): a project whose styles could not be read
        // shows the built-ins as the stand-ins they are.
        "katana://surface-styles" => (
            surface_styles_report(session.document()),
            "application/json",
        ),
        // ROADMODEL JSON, read from the Document as the status is: the road
        // model is made from the drawing's definitions, and a line would leave
        // a ROADMODEL in a history nobody typed.
        "katana://road-model" => (
            road_model_json(&bim_model(session.document())),
            "application/json",
        ),
        // The verb's own JSON, so the resource is what FORMATS says.
        "katana://formats" => {
            let formats = run_captured(session, "FORMATS JSON");
            if !formats.ok {
                return error_reply(id, INTERNAL_ERROR, &formats.messages);
            }
            (formats.output, "application/json")
        }
        _ => {
            return error_reply(id, RESOURCE_NOT_FOUND, &format!("no resource {uri}"));
        }
    };
    reply(
        id,
        Json::object_of([(
            "contents",
            Json::array_of([Json::object_of([
                ("uri", Json::from(&uri)),
                ("mimeType", Json::from(mime_type)),
                ("text", Json::from(text)),
            ])]),
        )]),
    )
}
