//! The geoprocessing tools of the MCP server (docs/mcp.md, "Geoprocessing
//! tools"): GDAL's catalogue and each algorithm's arguments as JSON, and a run
//! built from structured arguments. The port of
//! src/katana_app/geo/mcp_geo_tools.cpp (namespace katana::app::mcp, kept in
//! the geo folder beside the verbs it reads).
//!
//! katana_gdal_catalogue and katana_gdal_describe read the bridge directly:
//! they change nothing and a line would add nothing. katana_gdal_run builds
//! the GDAL line a person would type and runs it through the Session, as
//! every tool that changes something does, then reads the reply records back
//! into structured content - one implementation of the verb, the one
//! katana_cli runs.
//!
//! One reserved block per package that adds a tool, so the lanes edit only
//! their own lines.

use std::cmp::Ordering;

use katana_core::ErrorCode;
use katana_core::json::{Json, JsonError};
use katana_core::text::{format_exact_real, parse_finite_double, parse_integer};
use katana_gis::DataKind;
use katana_gis::formats::{find_format, format_options};
use katana_gis::processing::{self as gp, AlgorithmInfo, AlgorithmSpec, ArgType, Policy};

use super::formats_verbs::{
    FormatsQuery, FormatsQueryCapability, choose_formats, format_json, format_options_json,
    format_record, formats_summary, options_records,
};
use super::gis_records::records_json;
use super::replies::{Record, algorithm_record, arg_record, fixed3, line_word, parse_records};
use super::schema::{algorithm_json, arguments_schema, describe_json, inputs_schema};
use super::sql_mcp::gis_query_tool;
use crate::mcp::tools::{
    LineOutcome, Tool, ToolRefusal, ToolReply, argument, hints, lowered_text, object_schema,
    optional_bool, property, quoted, required_string, run_captured, scope_words_of, words,
};
use crate::session::Session;

// A word of a line by the one quoting rule (replies' line_word): quoted when
// it holds a blank, is empty or reads as a keyword. The line has no escape, so
// a quote or a line break cannot be said at all.
fn word(text: &str, what: &str) -> Result<String, ToolRefusal> {
    line_word(text).ok_or_else(|| {
        ToolRefusal::new(format!(
            "{what} may not contain a double quote or a line break"
        ))
    })
}

fn number(value: &Json) -> Result<String, ToolRefusal> {
    if value.is_number_integer() {
        return Ok(format!("{}", value.get::<i64>()?));
    }
    Ok(format_exact_real(value.get::<f64>()?))
}

// The algorithm an agent names: "raster hillshade", as GDAL LIST names it.
fn algorithm_words(name: &str) -> Vec<String> {
    name.split([' ', '\t'])
        .filter(|word| !word.is_empty())
        .map(String::from)
        .collect()
}

fn spec_of(arguments: &Json) -> Result<AlgorithmSpec, ToolRefusal> {
    let words = algorithm_words(&required_string(arguments, "algorithm")?);
    let mut consumed = 0;
    let path =
        gp::resolve(&words, &mut consumed).map_err(|error| ToolRefusal::new(error.describe()))?;
    if consumed != words.len() {
        return Err(ToolRefusal::new(format!(
            "\"algorithm\" names one algorithm; {} is not part of {}",
            words[consumed],
            gp::path_text(&path)
        )));
    }
    gp::describe(&path).map_err(|error| ToolRefusal::new(error.describe()))
}

// One source as FROM's words, the scope through the shared formatter so the
// words are exactly what the scope parser reads back.
fn source_words(source: &Json, arg: &str) -> Result<String, ToolRefusal> {
    if !source.is_object() {
        return Err(ToolRefusal::new(format!(
            "each input of \"{arg}\" is an object: {{scope ...}}, {{raster}}, {{surface}} or {{file}}"
        )));
    }
    // "where" alone is a scope of the drawing: the selection, filtered, as
    // WHERE alone reads on the command line.
    let scoped = source.contains("scope") || source.contains("where");
    let kinds = usize::from(scoped)
        + usize::from(source.contains("raster"))
        + usize::from(source.contains("surface"))
        + usize::from(source.contains("file"));
    if kinds != 1 {
        return Err(ToolRefusal::new(format!(
            "an input of \"{arg}\" gives exactly one of scope, raster, surface and file"
        )));
    }
    if let Some(raster) = source.find("raster") {
        if raster.is_number_integer() {
            return Ok(format!("RASTER {}", raster.get::<i64>()?));
        }
        let Some(name) = raster.as_str() else {
            return Err(ToolRefusal::new(
                "\"raster\" is a reference raster's id or name",
            ));
        };
        return Ok(format!("RASTER {}", word(name, "a raster's name")?));
    }
    if let Some(surface) = source.find("surface") {
        let Some(name) = surface.as_str() else {
            return Err(ToolRefusal::new("\"surface\" is a surface's name"));
        };
        let mut words = format!("SURFACE {}", word(name, "a surface's name")?);
        let cell = argument(source, "cell");
        if !cell.is_null() {
            if !cell.is_number() || cell.get::<f64>()?.partial_cmp(&0.0) != Some(Ordering::Greater)
            {
                return Err(ToolRefusal::new("\"cell\" is a positive length"));
            }
            words += " CELL ";
            words += &number(cell)?;
        }
        return Ok(words);
    }
    if let Some(file) = source.find("file") {
        let Some(path) = file.as_str() else {
            return Err(ToolRefusal::new("\"file\" is a path"));
        };
        let mut words = format!("FILE {}", word(path, "a path")?);
        let layer = argument(source, "layer");
        if !layer.is_null() {
            let Some(name) = layer.as_str() else {
                return Err(ToolRefusal::new("\"layer\" is the file's layer name"));
            };
            words += " LAYER ";
            words += &word(name, "a layer name")?;
        }
        return Ok(words);
    }
    let kind = match source.find("scope") {
        None => "selection",
        Some(scope) => scope.as_str().unwrap_or(""),
    };
    scope_words_of(
        kind,
        argument(source, "area"),
        argument(source, "layers"),
        optional_bool(source, "only", false)?,
        argument(source, "where"),
    )
}

// The arguments as GDAL's own --name=value words.
fn argument_words(spec: &AlgorithmSpec, arguments: &Json) -> Result<String, ToolRefusal> {
    if arguments.is_null() {
        return Ok(String::new());
    }
    if !arguments.is_object() {
        return Err(ToolRefusal::new(
            "\"arguments\" is an object of argument names and values",
        ));
    }
    let mut words = String::new();
    for (name, value) in arguments.items() {
        let found = spec
            .args
            .iter()
            .find(|arg| arg.name == *name || arg.aliases.iter().any(|alias| *alias == *name));
        let Some(found) = found else {
            return Err(ToolRefusal::new(format!(
                "no argument of {} is called {name}; katana_gdal_describe lists them",
                gp::path_text(&spec.info.path)
            )));
        };
        if found.is_dataset() {
            return Err(ToolRefusal::new(format!(
                "{name} is a dataset: give it in \"inputs\""
            )));
        }
        let option = format!("--{}", found.name);
        let scalar = |one: &Json| -> Result<String, ToolRefusal> {
            if let Some(flag) = one.as_bool() {
                return Ok(String::from(if flag { "true" } else { "false" }));
            }
            if one.is_number() {
                return number(one);
            }
            if let Some(text) = one.as_str() {
                return Ok(String::from(text));
            }
            Err(ToolRefusal::new(format!(
                "{name} takes a boolean, a number or a string"
            )))
        };
        if let Some(flag) = value.as_bool()
            && found.r#type == ArgType::Boolean
        {
            words.push(' ');
            words += &option;
            if !flag {
                words += "=false";
            }
        } else if value.is_array() {
            if found.packed_values || value.size() <= 1 {
                let mut packed = String::new();
                for one in value.iter() {
                    if !packed.is_empty() {
                        packed.push(',');
                    }
                    packed += &scalar(one)?;
                }
                words.push(' ');
                words += &word(&format!("{option}={packed}"), "a value")?;
            } else {
                for one in value.iter() {
                    words.push(' ');
                    words += &word(&format!("{option}={}", scalar(one)?), "a value")?;
                }
            }
        } else {
            words.push(' ');
            words += &word(&format!("{option}={}", scalar(value)?), "a value")?;
        }
    }
    Ok(words)
}

const OUTPUT_SHAPES: &str =
    "\"output\" is {layer}, {reference}, {file, format?, overwrite?} or {surface}";

fn target_words(output: &Json) -> Result<String, ToolRefusal> {
    if !output.is_object() {
        return Err(ToolRefusal::new(OUTPUT_SHAPES));
    }
    if output.contains("layer") {
        return Ok(format!(
            "TO LAYER {}",
            word(&required_string(output, "layer")?, "a layer")?
        ));
    }
    if output.contains("reference") {
        return Ok(format!(
            "TO REFERENCE {}",
            word(&required_string(output, "reference")?, "a name")?
        ));
    }
    if output.contains("surface") {
        return Ok(format!(
            "TO SURFACE {}",
            word(&required_string(output, "surface")?, "a name")?
        ));
    }
    if output.contains("file") {
        let mut words = format!(
            "TO FILE {}",
            word(&required_string(output, "file")?, "a path")?
        );
        if !argument(output, "format").is_null() {
            words += " FORMAT ";
            words += &word(&required_string(output, "format")?, "a driver")?;
        }
        if optional_bool(output, "overwrite", false)? {
            words += " OVERWRITE";
        }
        return Ok(words);
    }
    Err(ToolRefusal::new(OUTPUT_SHAPES))
}

fn integer_of(record: &Record, key: &str) -> Option<i64> {
    record.get(key).and_then(parse_integer)
}

fn fields_json(record: &Record) -> Json {
    let mut object = Json::object();
    for (key, text) in &record.fields {
        object[key.as_str()] = Json::from(text);
    }
    object
}

// The reply records as katana_gdal_run's structured content.
fn run_json(line: &str, outcome: &LineOutcome, algorithm: &str) -> Json {
    let mut inputs = Vec::new();
    let mut scopes = Vec::new();
    let mut outputs = Vec::new();
    let mut warnings = Vec::new();
    let mut result = Json::object_of([
        ("ok", Json::from(outcome.ok)),
        ("line", Json::from(line)),
        ("algorithm", Json::from(algorithm)),
        (
            "cancelled",
            Json::from(outcome.messages.contains("cancelled")),
        ),
        ("seconds", Json::Null),
    ]);
    for record in parse_records(&outcome.output) {
        match record.kind.as_str() {
            "gdal" => {
                if let Some(seconds) = record.get("seconds")
                    && let Some(value) = parse_finite_double(seconds)
                {
                    result["seconds"] = Json::from(value);
                }
                if record.get("preview").is_some() {
                    result["preview"] = Json::from(true);
                }
            }
            "input" => inputs.push(fields_json(&record)),
            "scope" => {
                let mut scope = fields_json(&record);
                let mut skipped = Json::object();
                for (key, text) in &record.fields {
                    if let Some(kind) = key.strip_prefix("skipped.") {
                        skipped[kind] = Json::from(parse_integer(text).unwrap_or(0));
                        // The object's own erase, which cannot fail on an object.
                        let _ = scope.erase(key);
                    }
                }
                for key in ["matched", "used", "points", "lines", "polygons"] {
                    if let Some(count) = integer_of(&record, key) {
                        scope[key] = Json::from(count);
                    }
                }
                scope["skipped"] = skipped;
                scopes.push(scope);
            }
            "output" => {
                let mut output = fields_json(&record);
                for key in ["created", "updated", "deleted", "skipped", "id"] {
                    if let Some(count) = integer_of(&record, key) {
                        output[key] = Json::from(count);
                    }
                }
                if let Some(raster) = record.get("raster") {
                    let x = raster.find('x');
                    let width = parse_integer(&raster[..x.unwrap_or(raster.len())]);
                    let height = x.and_then(|x| parse_integer(&raster[x + 1..]));
                    if let (Some(width), Some(height)) = (width, height) {
                        output["raster"] = Json::object_of([
                            ("width", Json::from(width)),
                            ("height", Json::from(height)),
                        ]);
                    }
                }
                outputs.push(output);
            }
            "text" => {
                result["text"] = Json::from(record.body.join("\n"));
            }
            "return" => {
                if let Some(code) = integer_of(&record, "code") {
                    result["return_code"] = Json::from(code);
                }
            }
            "warning" => warnings.push(Json::from(record.get("text").unwrap_or(""))),
            _ => {}
        }
    }
    result["inputs"] = Json::from(inputs);
    result["scope"] = Json::from(scopes);
    result["outputs"] = Json::from(outputs);
    result["warnings"] = Json::from(warnings);
    if !outcome.messages.is_empty() {
        result["messages"] = Json::from(&outcome.messages);
    }
    result
}

fn catalogue(_session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let filter = argument(arguments, "filter");
    if !filter.is_null() && !filter.is_string() {
        return Err(ToolRefusal::new(
            "\"filter\" is text: a group (raster, vector grid) or words to look for",
        ));
    }
    let text = filter.as_str().map(lowered_text).unwrap_or_default();
    let group = algorithm_words(&text);
    let mut is_group = false;
    if !group.is_empty() {
        let mut consumed = 0;
        let resolved = gp::resolve(&group, &mut consumed);
        is_group = matches!(&resolved, Err(error) if error.code == ErrorCode::InvalidArgument)
            && consumed == group.len();
    }
    let mut chosen: Vec<&AlgorithmInfo> = Vec::new();
    for info in gp::catalogue() {
        if info.container {
            continue;
        }
        let in_group = is_group && info.path.len() > group.len() && info.path.starts_with(&group);
        let matches = text.is_empty()
            || in_group
            || (!is_group
                && (lowered_text(&gp::path_text(&info.path)).contains(&text)
                    || lowered_text(&info.description).contains(&text)));
        if matches {
            chosen.push(info);
        }
    }
    // Schemas only for a short list: 121 schemas would be most of a model's
    // context for one question.
    let schemas = optional_bool(arguments, "schemas", false)? && chosen.len() <= 20;
    let mut algorithms = Vec::new();
    let mut lines = String::new();
    for info in &chosen {
        let mut entry = algorithm_json(info);
        if schemas && let Ok(spec) = gp::describe(&info.path) {
            entry["arguments_schema"] = arguments_schema(&spec);
            entry["inputs_schema"] = inputs_schema(&spec);
        }
        algorithms.push(entry);
        lines += &algorithm_record(info);
        lines.push('\n');
    }
    let versions = gp::versions();
    lines += &format!(
        "listed algorithms={} schemas={}",
        chosen.len(),
        if schemas { "yes" } else { "no" }
    );
    Ok(ToolReply {
        text: lines,
        structured: Json::object_of([
            ("gdal_version", Json::from(versions.gdal)),
            ("algorithms", Json::from(algorithms)),
        ]),
        is_error: false,
    })
}

fn describe(_session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let spec = spec_of(arguments)?;
    let mut text = algorithm_record(&spec.info);
    for arg in &spec.args {
        text.push('\n');
        text += &arg_record(arg);
    }
    Ok(ToolReply {
        text,
        structured: describe_json(&spec),
        is_error: false,
    })
}

fn run(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let spec = spec_of(arguments)?;
    let algorithm = gp::path_text(&spec.info.path);
    let confirm = optional_bool(arguments, "confirm", false)?;
    if spec.info.policy == Policy::Confirm && !confirm {
        return Err(ToolRefusal::new(format!(
            "{algorithm} changes or removes data that already exists; it runs only with \
             \"confirm\": true"
        )));
    }
    let mut line = format!("GDAL {algorithm}");
    let mut gdal_words: Vec<String> = Vec::new();
    let tokens = argument(arguments, "tokens");
    if !tokens.is_null() {
        if !tokens.is_array() {
            return Err(ToolRefusal::new("\"tokens\" is a list of GDAL's own words"));
        }
        for token in tokens.iter() {
            let Some(token) = token.as_str() else {
                return Err(ToolRefusal::new("each of \"tokens\" is a string"));
            };
            gdal_words.push(String::from(token));
            line.push(' ');
            line += &word(token, "a GDAL word")?;
        }
    }
    // A pipeline's `update` step is said here as a Confirm algorithm is; the
    // line refuses it too, and so does every other way a line is run.
    let step = gp::tail_effects(&spec.info.path, &gdal_words).confirm_step;
    if !step.is_empty() && !confirm {
        return Err(ToolRefusal::new(format!(
            "the pipeline step {step} changes data that already exists; it runs only with \
             \"confirm\": true"
        )));
    }
    line += &argument_words(&spec, argument(arguments, "arguments"))?;
    let inputs = argument(arguments, "inputs");
    if !inputs.is_null() {
        if !inputs.is_object() {
            return Err(ToolRefusal::new(
                "\"inputs\" is an object: {\"input\": {\"scope\": \"drawing\"}}",
            ));
        }
        for (arg, value) in inputs.items() {
            if value.is_array() {
                for source in value.iter() {
                    line += &format!(
                        " FROM {} {}",
                        word(&arg, "an argument name")?,
                        source_words(source, &arg)?
                    );
                }
            } else {
                line += &format!(
                    " FROM {} {}",
                    word(&arg, "an argument name")?,
                    source_words(value, &arg)?
                );
            }
        }
    }
    let output = argument(arguments, "output");
    if !output.is_null() {
        line.push(' ');
        line += &target_words(output)?;
        if optional_bool(arguments, "overwrite", false)?
            && !optional_bool(output, "overwrite", false)?
        {
            line += " OVERWRITE";
        }
    } else if optional_bool(arguments, "overwrite", false)? {
        line += " OVERWRITE";
    }
    if confirm {
        line += " CONFIRM";
    }
    if optional_bool(arguments, "preview", false)? {
        line += " PREVIEW";
    }
    let outcome = run_captured(session, &line);
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
        structured: run_json(&line, &outcome, &algorithm),
        text,
        is_error: !outcome.ok,
    })
}

// ---- T0: katana_terrain_list ----
// SURFACE LIST JSON through the Session, as a person's SURFACE LIST: the
// drawing's surfaces and the rasters a terrain verb reads.
fn terrain_list(session: &mut Session, _arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let line = "SURFACE LIST JSON";
    let outcome = run_captured(session, line);
    if !outcome.ok {
        return Ok(ToolReply {
            text: outcome.messages,
            structured: Json::Null,
            is_error: true,
        });
    }
    let Ok(mut listed) = Json::parse(&outcome.output) else {
        return Ok(ToolReply {
            text: format!("SURFACE LIST JSON did not answer JSON: {}", outcome.output),
            structured: Json::Null,
            is_error: true,
        });
    };
    let mut text = format!("> {line}");
    // The C++ reads each list through the operator[] of a value that is not
    // const: a list that is missing is put in as null and is then nothing to
    // walk, and a reply that is no object throws nlohmann's type_error 305 -
    // which IndexMut does (as a panic with that text, caught as the tool's
    // error).
    let _ = &mut listed["surfaces"];
    for surface in listed["surfaces"].iter() {
        text += &format!(
            "\nsurface {}: {} triangles, z {} to {}, style {}{}",
            surface["name"].get::<String>()?,
            surface["triangles"].get::<usize>()?,
            fixed3(surface["zmin"].get::<f64>()?),
            fixed3(surface["zmax"].get::<f64>()?),
            surface.value("style", String::new())?,
            if surface.value("visible", true)? {
                ""
            } else {
                ", hidden"
            }
        );
    }
    let _ = &mut listed["rasters"];
    for raster in listed["rasters"].iter() {
        text += &format!(
            "\nraster {} {}: {}x{} {}",
            raster["id"].get::<u64>()?,
            raster["name"].get::<String>()?,
            raster["width"].get::<i32>()?,
            raster["height"].get::<i32>()?,
            raster["role"].get::<String>()?
        );
    }
    Ok(ToolReply {
        text,
        structured: listed,
        is_error: false,
    })
}

// ---- D1: katana_dataset_info ----

// The INFO line of the arguments, and its reply's records as structured
// content; with json, GDAL's own info JSON too, from INFO ... JSON - the lines
// a person would type, run through the Session.
fn dataset_info(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let mut words = format!("INFO {}", quoted(&required_string(arguments, "path")?)?);
    let layer = argument(arguments, "layer");
    if !layer.is_null() {
        let Some(name) = layer.as_str() else {
            return Err(ToolRefusal::new("\"layer\" is a layer's name"));
        };
        words += " LAYER ";
        words += &word(name, "a layer name")?;
    }
    let mut line = words.clone();
    if optional_bool(arguments, "stats", false)? {
        line += " STATS";
    }
    if optional_bool(arguments, "check", false)? {
        line += " CHECK";
    }
    let outcome = run_captured(session, &line);
    let mut text = format!("> {line}");
    if !outcome.output.is_empty() {
        text.push('\n');
        text += &outcome.output;
    }
    if !outcome.messages.is_empty() {
        text.push('\n');
        text += &outcome.messages;
    }
    let mut result = Json::object_of([
        ("ok", Json::from(outcome.ok)),
        ("line", Json::from(&line)),
        ("records", records_json(&outcome.output)),
    ]);
    if !outcome.messages.is_empty() {
        result["messages"] = Json::from(&outcome.messages);
    }
    if outcome.ok && optional_bool(arguments, "json", false)? {
        let json_line = format!("{words} JSON");
        let json = run_captured(session, &json_line);
        let parsed = Json::parse(&json.output);
        let parsed = match parsed {
            Ok(parsed) if json.ok => parsed,
            _ => {
                return Err(ToolRefusal::new(format!(
                    "{json_line} gave no JSON: {}",
                    json.messages
                )));
            }
        };
        result["gdal"] = parsed;
        text += &format!("\n> {json_line}\n(GDAL's JSON is in structuredContent.gdal)");
    }
    Ok(ToolReply {
        text,
        structured: result,
        is_error: !outcome.ok,
    })
}

// ---- D2: katana_references ----

// nlohmann's `value(key, default)` for a default that is JSON: the member when
// there is one, else the default; type_error 306 for a value that is no
// object, as nlohmann throws it.
fn member_or(value: &Json, key: &str, default: Json) -> Result<Json, JsonError> {
    if !value.is_object() {
        return Err(JsonError::type_error(
            306,
            &format!("cannot use value() with {}", value.type_name()),
        ));
    }
    Ok(value.find(key).cloned().unwrap_or(default))
}

// A REFS line built from the action, run through the Session, and the
// reference layers after it as structured content: what an agent needs to act
// on a layer by its id, and to see that the action took.
fn references(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let chosen = argument(arguments, "action");
    let action = if chosen.is_null() {
        "list"
    } else {
        chosen.as_str().unwrap_or("?")
    };
    let reference = || -> Result<String, ToolRefusal> {
        let id = argument(arguments, "id");
        if id.is_number_integer() && id.get::<i64>()? > 0 {
            return Ok(format!("{}", id.get::<i64>()?));
        }
        if let Some(name) = id.as_str()
            && !name.is_empty()
        {
            return word(name, "a layer's name");
        }
        Err(ToolRefusal::new(
            "\"id\" is a reference layer's id, or its name",
        ))
    };
    let value_word = |what: &str| -> Result<String, ToolRefusal> {
        let given = argument(arguments, "value");
        if given.is_number() {
            return number(given);
        }
        if let Some(text) = given.as_str()
            && !text.is_empty()
        {
            return word(text, what);
        }
        Err(ToolRefusal::new(format!("\"value\" is {what}")))
    };
    let line = match action {
        "list" => String::from("REFS LIST"),
        // The verb's words are read in any case (Tokens::is).
        "show" | "hide" | "remove" | "info" => format!("REFS {action} {}", reference()?),
        "opacity" => format!(
            "REFS OPACITY {} {}",
            reference()?,
            value_word("an opacity from 0 to 1")?
        ),
        "color" => format!(
            "REFS COLOR {} {}",
            reference()?,
            value_word("elevation, intensity, classification, rgb or flat")?
        ),
        "rename" => format!(
            "REFS RENAME {} {}",
            reference()?,
            value_word("the new name")?
        ),
        "overviews" => {
            // Writes beside the raster's file: only when the agent says so.
            if !optional_bool(arguments, "confirm", false)? {
                return Err(ToolRefusal::new(
                    "overviews are written beside the raster's file (.ovr); they are built only \
                     with \"confirm\": true",
                ));
            }
            let mut line = format!("REFS OVERVIEWS {}", reference()?);
            if !argument(arguments, "value").is_null() {
                line += " levels=";
                line += &value_word("the levels, e.g. \"2,4,8\"")?;
            }
            line += " CONFIRM";
            line
        }
        "restore" => String::from("REFS RESTORE"),
        _ => {
            return Err(ToolRefusal::new(
                "\"action\" is list, show, hide, remove, info, opacity, color, rename, \
                 overviews or restore",
            ));
        }
    };
    let outcome = run_captured(session, &line);
    let mut text = format!("> {line}");
    if !outcome.output.is_empty() {
        text.push('\n');
        text += &outcome.output;
    }
    if !outcome.messages.is_empty() {
        text.push('\n');
        text += &outcome.messages;
    }
    let mut result = Json::object_of([
        ("ok", Json::from(outcome.ok)),
        ("line", Json::from(&line)),
        ("records", records_json(&outcome.output)),
    ]);
    if !outcome.messages.is_empty() {
        result["messages"] = Json::from(&outcome.messages);
    }
    // The layers as they are now, whatever the action was.
    let listed = run_captured(session, "REFS JSON");
    if listed.ok
        && let Ok(layers) = Json::parse(&listed.output)
    {
        result["references"] = member_or(&layers, "references", Json::array())?;
        result["missing"] = member_or(&layers, "missing", Json::array())?;
    }
    Ok(ToolReply {
        text,
        structured: result,
        is_error: !outcome.ok,
    })
}

// ---- I2: katana_formats ----
// Read-only, as katana_gdal_catalogue is: it reads the registry through the
// verb's own chooser and records (formats_verbs), so the tool, FORMATS and the
// katana://formats resource cannot come to differ.
fn formats(_session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let driver = argument(arguments, "driver");
    if !driver.is_null() {
        let Some(name) = driver.as_str() else {
            return Err(ToolRefusal::new(
                "\"driver\" is a driver's name, as FORMATS lists it",
            ));
        };
        let Some(format) = find_format(name) else {
            return Err(ToolRefusal::new(format!(
                "this GDAL build has no format named \"{name}\"; katana_formats lists them"
            )));
        };
        let options =
            format_options(&format.driver).map_err(|error| ToolRefusal::new(error.describe()))?;
        return Ok(ToolReply {
            text: options_records(format, &options),
            structured: format_options_json(format, &options),
            is_error: false,
        });
    }
    let choice = |name: &str, first: &str, second: &str| -> Result<u8, ToolRefusal> {
        let given = argument(arguments, name);
        if given.is_null() {
            return Ok(0);
        }
        match given.as_str() {
            Some(text) if text == first => Ok(1),
            Some(text) if text == second => Ok(2),
            _ => Err(ToolRefusal::new(format!(
                "\"{name}\" is \"{first}\" or \"{second}\""
            ))),
        }
    };
    let kind = match choice("kind", "raster", "vector")? {
        0 => None,
        1 => Some(DataKind::Raster),
        _ => Some(DataKind::Vector),
    };
    let capability = match choice("capability", "read", "write")? {
        0 => FormatsQueryCapability::Any,
        1 => FormatsQueryCapability::Read,
        _ => FormatsQueryCapability::Write,
    };
    let filter = argument(arguments, "filter");
    let text = if filter.is_null() {
        String::new()
    } else {
        match filter.as_str() {
            Some(text) => String::from(text),
            None => return Err(ToolRefusal::new("\"filter\" is text")),
        }
    };
    let query = FormatsQuery {
        kind,
        capability,
        text,
    };
    let mut formats = Vec::new();
    let mut text = String::new();
    let chosen = choose_formats(&query);
    for format in &chosen {
        formats.push(format_json(format));
        text += &format_record(format);
        text.push('\n');
    }
    text += &formats_summary(&query, chosen.len());
    Ok(ToolReply {
        text,
        structured: Json::object_of([
            ("gdal_version", Json::from(gp::versions().gdal)),
            ("formats", Json::from(formats)),
        ]),
        is_error: false,
    })
}

/// The geoprocessing tools: katana_gdal_catalogue, katana_gdal_describe,
/// katana_gdal_run, and the lanes' after them.
pub fn geo_tools() -> Vec<Tool> {
    vec![
        // ---- F0: GDAL algorithm bridge ----
        Tool {
            name: "katana_gdal_catalogue",
            title: "GDAL algorithms",
            description: concat!(
                "The catalogue of GDAL's algorithms Katana can run (GDAL 3.13's raster, vector, mdim, ",
                "dataset, vsi, driver and pipeline families): each one's path, description, aliases and ",
                "policy - safe, or confirm for those that change or remove existing data. filter narrows ",
                "it to a group (\"raster\", \"vector grid\") or to words in the names and descriptions; ",
                "schemas adds each one's argument and input JSON Schemas when 20 or fewer are listed."
            ),
            input_schema: object_schema(
                Json::object_of([
                    (
                        "filter",
                        property(
                            Json::from("string"),
                            "A group (raster, vector, vector grid ...) or words to look for, e.g. \
                         \"slope\".",
                            [],
                        ),
                    ),
                    (
                        "schemas",
                        property(
                            Json::from("boolean"),
                            "Add each algorithm's arguments_schema and inputs_schema; honoured when \
                         the filter leaves 20 or fewer.",
                            [],
                        ),
                    ),
                ]),
                &[],
            ),
            annotations: hints(true, false, true, false),
            call: catalogue,
        },
        Tool {
            name: "katana_gdal_describe",
            title: "Describe a GDAL algorithm",
            description: concat!(
                "One GDAL algorithm's arguments as data: name, short name, aliases, type, whether ",
                "required or positional, default, choices, bounds with whether each is inclusive, list ",
                "counts, and for a dataset argument its kinds and the Katana sources it can be bound ",
                "from. arguments_schema and inputs_schema are the JSON Schemas of katana_gdal_run's ",
                "arguments and inputs; gdal_usage is GDAL's own --json-usage."
            ),
            input_schema: object_schema(
                Json::object_of([(
                    "algorithm",
                    property(
                        Json::from("string"),
                        "The algorithm, e.g. \"raster hillshade\"; aliases are accepted (\"raster \
                     warp\").",
                        [],
                    ),
                )]),
                &["algorithm"],
            ),
            annotations: hints(true, false, true, false),
            call: describe,
        },
        Tool {
            name: "katana_gdal_run",
            title: "Run a GDAL algorithm",
            description: concat!(
                "Run one GDAL algorithm on the live drawing, as the GDAL verb does: arguments by name ",
                "(katana_gdal_describe's arguments_schema), GDAL's own words in tokens, datasets in ",
                "inputs bound from the drawing by scope and filter ({scope: \"drawing\"|\"selection\"|",
                "\"area\"|\"layers\", area, layers, only, where}), from a reference raster ({raster}), a ",
                "surface ({surface, cell}) or a file ({file, layer}), and the output to a layer ",
                "({layer}), a reference raster ({reference}) or a file ({file, format, overwrite}). ",
                "Without output, features go to layer gis/<algorithm> and rasters become a reference ",
                "raster. A result in the drawing is one undoable step. Algorithms whose policy is ",
                "confirm (they change or remove existing data) run only with confirm: true. preview ",
                "checks the arguments and reports what the scopes took, changing nothing. Returns the ",
                "records as structured data: what each scope matched and used, what was created."
            ),
            input_schema: object_schema(
                Json::object_of([
                    (
                        "algorithm",
                        property(
                            Json::from("string"),
                            "The algorithm, e.g. \"vector buffer\".",
                            [],
                        ),
                    ),
                    (
                        "arguments",
                        property(
                            Json::from("object"),
                            "Argument values by name, e.g. {\"distance\": 5, \"endcap-style\": \
                         \"flat\"}; see arguments_schema.",
                            [],
                        ),
                    ),
                    (
                        "tokens",
                        property(
                            Json::from("array"),
                            "GDAL's own command-line words, as `gdal <algorithm>` takes them, e.g. \
                         [\"--zfactor\", \"2\"].",
                            [("items", Json::object_of([("type", Json::from("string"))]))],
                        ),
                    ),
                    (
                        "inputs",
                        property(
                            Json::from("object"),
                            "Dataset arguments by name, each a source or a list of sources; see \
                         inputs_schema.",
                            [],
                        ),
                    ),
                    (
                        "output",
                        property(
                            Json::from("object"),
                            "{layer: \"gis/buffer\"} | {reference: \"shade\"} | {file: path, format?, \
                         overwrite?} | {surface: name}.",
                            [],
                        ),
                    ),
                    (
                        "confirm",
                        property(
                            Json::from("boolean"),
                            "Required true for an algorithm whose policy is confirm, and for a \
                         pipeline with an update step.",
                            [],
                        ),
                    ),
                    (
                        "overwrite",
                        property(
                            Json::from("boolean"),
                            "Required true when GDAL's own words change a dataset that already \
                         exists (--overwrite, --overwrite-layer, --append, --update, --upsert, \
                         --add, --resume), in a pipeline too; the same as output's overwrite for \
                         a file TO names.",
                            [],
                        ),
                    ),
                    (
                        "preview",
                        property(
                            Json::from("boolean"),
                            "Check and report the sources, run nothing (default false).",
                            [],
                        ),
                    ),
                ]),
                &["algorithm"],
            ),
            annotations: hints(false, true, false, true),
            call: run,
        },
        // ---- T0: katana_terrain_list ----
        Tool {
            name: "katana_terrain_list",
            title: "Surfaces and rasters",
            description: concat!(
                "The drawing's named surfaces (TIN, made by SURFACE FROM, TO SURFACE or an import; saved ",
                "with the project) and its reference rasters: what CONTOUR, RASTER SHADE, RASTER SLOPE ",
                "and a GDAL run's FROM SURFACE <name> or FROM RASTER <id|name> read. Each surface's ",
                "triangles, points, bounds [x0,y0,x1,y1], zmin, zmax, plan area, source, the display ",
                "style it is drawn in (katana://surface-styles lists them) and whether the views show ",
                "it; each raster's id, name, role (imagery, elevation or derived), size, square cell, ",
                "coordinate system, file and the line that derived it."
            ),
            input_schema: object_schema(Json::object(), &[]),
            annotations: hints(true, false, true, false),
            call: terrain_list,
        },
        // ---- V5: katana_gis_query ----
        gis_query_tool(),
        // ---- I2: katana_formats ----
        Tool {
            name: "katana_formats",
            title: "GDAL formats",
            description: concat!(
                "The formats this build of GDAL reads and writes, from its own registry: each driver's ",
                "name (what EXPORT's and GDAL's FORMAT take), description, the kinds of data it holds, ",
                "reads and writes (raster, vector), its extensions, and whether it opens /vsi paths ",
                "(inside a .zip, over https). kind and capability narrow the list (\"vector\", ",
                "\"write\"); filter keeps drivers whose name, description or extensions hold every word. ",
                "driver gives that one driver's open, creation and layer-creation options instead: ",
                "each option's name, type, default, choices, bounds and description, what IMPORT's and ",
                "EXPORT's driver options are checked against."
            ),
            input_schema: object_schema(
                Json::object_of([
                    (
                        "kind",
                        property(
                            Json::from("string"),
                            "Only drivers of this kind of data.",
                            [("enum", words(["raster", "vector"]))],
                        ),
                    ),
                    (
                        "capability",
                        property(
                            Json::from("string"),
                            "Only drivers that read, or write, that kind.",
                            [("enum", words(["read", "write"]))],
                        ),
                    ),
                    (
                        "filter",
                        property(
                            Json::from("string"),
                            "Words each of which the driver's name, description or an extension \
                         holds, e.g. \"parquet\".",
                            [],
                        ),
                    ),
                    (
                        "driver",
                        property(
                            Json::from("string"),
                            "One driver's options instead of the list, e.g. \"GPKG\".",
                            [],
                        ),
                    ),
                ]),
                &[],
            ),
            annotations: hints(true, false, true, false),
            call: formats,
        },
        // ---- D1: katana_dataset_info ----
        Tool {
            name: "katana_dataset_info",
            title: "Describe a data file",
            description: concat!(
                "What a GIS file, a folder or a point cloud holds, without importing it - the INFO ",
                "verb's records as structured data: dataset (kind, driver, CRS), raster (size, cell, ",
                "bounds), band (type, no-data, and with stats the minimum, maximum, mean and standard ",
                "deviation, computed without writing beside the file), layer (features, geometry, CRS, ",
                "bounds), field (name, type, width), subdataset, pointcloud; a folder gives a found ",
                "record per dataset. check reads every value and reports check and problem records. ",
                "json adds GDAL's own info JSON (raster info, vector info, mdim info) as gdal. path may ",
                "be a /vsi path or a URL."
            ),
            input_schema: object_schema(
                Json::object_of([
                    (
                        "path",
                        property(
                            Json::from("string"),
                            "The file, folder, /vsi path or URL.",
                            [],
                        ),
                    ),
                    (
                        "layer",
                        property(Json::from("string"), "Only this vector layer.", []),
                    ),
                    (
                        "stats",
                        property(
                            Json::from("boolean"),
                            "Compute every band's statistics from every pixel.",
                            [],
                        ),
                    ),
                    (
                        "check",
                        property(
                            Json::from("boolean"),
                            "Read every value, and report what could not be read.",
                            [],
                        ),
                    ),
                    (
                        "json",
                        property(
                            Json::from("boolean"),
                            "Add GDAL's own info JSON as \"gdal\" (default false).",
                            [],
                        ),
                    ),
                ]),
                &["path"],
            ),
            annotations: hints(true, false, true, true),
            call: dataset_info,
        },
        // ---- D2: katana_references ----
        Tool {
            name: "katana_references",
            title: "Reference layers",
            description: concat!(
                "The reference layers the drawing is worked on top of - rasters and point clouds - ",
                "listed, or one acted on by its id (or name): show, hide, remove, info, opacity (value ",
                "0 to 1, a raster's), color (value elevation, intensity, classification, rgb or flat, a ",
                "point cloud's), rename (value the new name), overviews (writes .ovr overviews beside ",
                "the raster's file; value the levels, e.g. \"2,4,8\"; only with confirm: true), or ",
                "restore (read again the layers the project records, as opening it does). The project ",
                "records each layer's source and display when it is saved. Returns the action's ",
                "records and the layers after it, each with its id; missing lists the layers the ",
                "project names whose files could not be read."
            ),
            input_schema: object_schema(
                Json::object_of([
                    (
                        "action",
                        property(
                            Json::from("string"),
                            "What to do; list by default.",
                            [(
                                "enum",
                                words([
                                    "list",
                                    "show",
                                    "hide",
                                    "remove",
                                    "info",
                                    "opacity",
                                    "color",
                                    "rename",
                                    "overviews",
                                    "restore",
                                ]),
                            )],
                        ),
                    ),
                    (
                        "id",
                        property(
                            words(["integer", "string"]),
                            "The layer's id, as the list gives it, or its name.",
                            [],
                        ),
                    ),
                    (
                        "value",
                        property(
                            words(["number", "string"]),
                            "The opacity, the colouring, the new name, or the overview levels.",
                            [],
                        ),
                    ),
                    (
                        "confirm",
                        property(
                            Json::from("boolean"),
                            "Required true for overviews, which write beside the raster's file.",
                            [],
                        ),
                    ),
                ]),
                &[],
            ),
            annotations: hints(false, true, false, false),
            call: references,
        },
    ]
}
