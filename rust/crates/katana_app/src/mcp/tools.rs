//! What an MCP tool is, and the helpers every tool is built from, shared by
//! the server's own tools (mcp::server) and the geoprocessing tools
//! (geo::mcp_geo_tools), so a tool added in another file is the same kind of
//! thing and refuses the same way (docs/mcp.md). The port of
//! src/katana_app/mcp_tools.hpp, with the bodies mcp_server.cpp gives its
//! functions.
//!
//! THE RUST SHAPE (docs/rust.md, step 6). A tool is a row: its name, title,
//! description, schemas and a plain `fn` that takes the session lent for the
//! call and the call's arguments. The C++'s `throw ToolRefusal` is the `Err`
//! of that fn; a nlohmann exception, which the C++ catches as a
//! std::exception and says the same way, is one too (`From<JsonError>`). A
//! tool's lines run through Session::run with a [`Captured`] console lent for
//! each: the C++'s CaptureStreams, with nothing process-wide swapped.

use std::panic::panic_any;

use katana_cad::CommandInterpreter;
use katana_cad::document_status::{document_status, status_json};
use katana_cad::scope_verbs::{ScopeSource, ScopeWords, format_scope_words, parse_where_condition};
use katana_core::json::{Json, JsonError, JsonResult};
use katana_geometry::{Box2, Point2};
use katana_math::cpp::{max, min};

use crate::console::Captured;
use crate::session::Session;
use crate::update_verbs::{UpdateContext, run_update_line};

/// What a tool answers: its text, the same facts as structured content (null
/// when the tool has nothing but its text), and whether it failed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolReply {
    pub text: String,
    pub structured: Json,
    pub is_error: bool,
}

/// What a tool runs: the session lent for the call and the call's arguments
/// (an object). `Err` is the C++'s `throw ToolRefusal`.
pub type ToolCall = fn(&mut Session, &Json) -> Result<ToolReply, ToolRefusal>;

/// One tool as tools/list says it, and what it runs.
#[derive(Clone, Debug)]
pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub input_schema: Json,
    pub annotations: Json,
    pub call: ToolCall,
}

/// A tool's failure that is the caller's to fix, reported as a tool result
/// (isError) so that the model reads it, rather than as a protocol error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolRefusal {
    pub message: String,
}

impl ToolRefusal {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// A nlohmann exception out of a tool: the C++ catches it beside the
/// refusals (as a std::exception) and says it the same way, `error: ` and its
/// what().
impl From<JsonError> for ToolRefusal {
    fn from(error: JsonError) -> Self {
        Self::new(error.what)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunOptions {
    pub stop_on_error: bool,
    pub discard_unsaved_changes: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            stop_on_error: true,
            discard_unsaved_changes: false,
        }
    }
}

/// One line through the session, and what it printed: what a tool that turns
/// a line's records into structured content reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineOutcome {
    pub ok: bool,
    /// std::cout
    pub output: String,
    /// std::cerr
    pub messages: String,
}

/// A nlohmann call that throws, where nothing between it and the server
/// returns a value to carry it: its exception's what() becomes the panic's
/// text, which the server catches as the C++ catches a std::exception (the
/// tool's call, `error: <what>`; a method, JSON-RPC -32603 with it).
pub(crate) fn thrown<T>(result: JsonResult<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic_any(error.what),
    }
}

/// What a tool's panic says when its payload holds no text.
pub(crate) const TOOL_STOPPED: &str = "a tool stopped without a message";

// ---- running lines -------------------------------------------------------------------------

// What a line printed, without the line ends and blanks after it.
fn trimmed(mut text: String) -> String {
    while text.ends_with(['\n', '\r', ' ']) {
        text.pop();
    }
    text
}

// The line's first word in capitals (ASCII only, as std::toupper in the
// classic locale does).
fn upper_first_word(line: &str) -> String {
    let mut word = String::new();
    for ch in line.chars() {
        if ch == ' ' || ch == '\t' {
            if !word.is_empty() {
                break;
            }
            continue;
        }
        word.push(ch.to_ascii_uppercase());
    }
    word
}

struct LineResult {
    command: String,
    ok: bool,
    skipped: bool,
    /// what the line printed on std::cout
    output: String,
    /// and on std::cerr: errors and warnings
    messages: String,
}

struct Batch {
    lines: Vec<LineResult>,
    ok: bool,
}

fn run_lines(session: &mut Session, lines: &[String], options: &RunOptions) -> Batch {
    let mut batch = Batch {
        lines: Vec::new(),
        ok: true,
    };
    let mut stopped = false;
    for raw in lines {
        let mut line = raw.clone();
        if line.ends_with('\r') {
            line.pop();
        }
        let mut result = LineResult {
            command: line.clone(),
            ok: true,
            skipped: false,
            output: String::new(),
            messages: String::new(),
        };
        let verb = upper_first_word(&line);
        if stopped {
            result.skipped = true;
        } else if Session::is_quit(&line) {
            // The client ends the session by closing it, not by a verb that
            // would leave the server running with nothing to serve.
            result.skipped = true;
            result.messages =
                String::from("QUIT is not a command here: the MCP client ends the session.");
        } else if CommandInterpreter::replaces_document(&line)
            && session.document().is_modified()
            && !options.discard_unsaved_changes
        {
            // The interpreter replaces the drawing without asking, as a typed
            // NEW always has. A model is not a person looking at the window,
            // so it is asked to say so.
            result.ok = false;
            result.messages = format!(
                "error: the drawing has unsaved changes that {verb} would discard. Save first \
                 (katana_save_project), or pass discard_unsaved_changes: true."
            );
        } else {
            // The session catches a panic out of a verb itself and says it on
            // the console it is lent (`error: Internal: ...`), as the C++'s
            // catch here said a std::exception.
            let mut capture = Captured::default();
            result.ok = session.run(&line, &mut capture);
            result.output = trimmed(capture.out);
            result.messages = trimmed(capture.err);
        }
        if !result.ok {
            batch.ok = false;
            stopped = options.stop_on_error;
        }
        batch.lines.push(result);
    }
    batch
}

fn transcript_of(batch: &Batch) -> String {
    let mut text = String::new();
    for line in &batch.lines {
        if line.command.is_empty() && line.output.is_empty() && line.messages.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("> ");
        text.push_str(&line.command);
        text.push('\n');
        if line.skipped && line.messages.is_empty() {
            text.push_str("(not run: an earlier command failed)\n");
            continue;
        }
        if !line.output.is_empty() {
            text.push_str(&line.output);
            text.push('\n');
        }
        if !line.messages.is_empty() {
            text.push_str(&line.messages);
            text.push('\n');
        }
    }
    trimmed(text)
}

fn lines_json(batch: &Batch) -> Json {
    let mut lines = Vec::new();
    for line in &batch.lines {
        let mut entry = Json::object_of([
            ("command", Json::from(&line.command)),
            ("ok", Json::from(line.ok && !line.skipped)),
        ]);
        if line.skipped {
            entry["skipped"] = Json::from(true);
        }
        if !line.output.is_empty() {
            entry["output"] = Json::from(&line.output);
        }
        if !line.messages.is_empty() {
            entry["messages"] = Json::from(&line.messages);
        }
        lines.push(entry);
    }
    Json::from(lines)
}

/// The drawing's state, as STATUS JSON gives it (cad's document_status): one
/// definition for this server, katana_cli and the window.
pub(crate) fn status_of(session: &Session) -> Json {
    thrown(Json::parse(status_json(&document_status(
        session.document(),
    ))))
}

/// The reply of a tool that is a batch of lines, with the state after it.
pub(crate) fn batch_reply(
    session: &mut Session,
    lines: &[String],
    options: &RunOptions,
) -> ToolReply {
    let batch = run_lines(session, lines, options);
    let status = status_of(session);
    let mut text = transcript_of(&batch);
    if text.is_empty() {
        text = String::from("(nothing to run)");
    }
    ToolReply {
        text,
        structured: Json::object_of([
            ("ok", Json::from(batch.ok)),
            ("commands", lines_json(&batch)),
            ("status", status),
        ]),
        is_error: !batch.ok,
    }
}

// ---- the helpers every tool is built from ---------------------------------------------------

/// An object schema of these properties: `required` named when there are
/// any, and additionalProperties false.
pub fn object_schema(properties: Json, required: &[&str]) -> Json {
    let mut schema = Json::object_of([("type", Json::from("object")), ("properties", properties)]);
    if !required.is_empty() {
        schema["required"] = Json::array_of(required.iter().map(|&name| Json::from(name)));
    }
    schema["additionalProperties"] = Json::from(false);
    schema
}

/// A tool's annotations. A tool that changes nothing is listed with two
/// hints only: whether it destroys, and whether asking twice is asking once,
/// mean something only for a tool that changes things.
pub fn hints(read_only: bool, destructive: bool, idempotent: bool, open_world: bool) -> Json {
    let mut annotations = Json::object_of([
        ("readOnlyHint", Json::from(read_only)),
        ("openWorldHint", Json::from(open_world)),
    ]);
    if !read_only {
        annotations["destructiveHint"] = Json::from(destructive);
        annotations["idempotentHint"] = Json::from(idempotent);
    }
    annotations
}

/// One line through the session: its transcript, and the drawing's state
/// after it.
pub fn one_line(session: &mut Session, line: &str, options: &RunOptions) -> ToolReply {
    batch_reply(session, &[String::from(line)], options)
}

/// One line through the session, and what it printed.
pub fn run_captured(session: &mut Session, line: &str) -> LineOutcome {
    let batch = run_lines(session, &[String::from(line)], &RunOptions::default());
    let result = &batch.lines[0];
    LineOutcome {
        ok: result.ok && !result.skipped,
        output: result.output.clone(),
        messages: result.messages.clone(),
    }
}

/// A path as the interpreter reads it: one quoted word. It has no escape, so
/// a path with a quote in it cannot be said at all - and is refused rather
/// than cut short at the quote; so is a line break, which no line can say.
pub fn quoted(path: &str) -> Result<String, ToolRefusal> {
    if path.contains(['"', '\n']) {
        return Err(ToolRefusal::new(format!(
            "a path may not contain a double quote or a line break: {path}"
        )));
    }
    Ok(format!("\"{path}\""))
}

// What a missing argument reads as: nlohmann's null.
static NULL: Json = Json::Null;

/// The argument `name`, or null when it is not given (or `arguments` is no
/// object).
pub fn argument<'a>(arguments: &'a Json, name: &str) -> &'a Json {
    arguments.find(name).unwrap_or(&NULL)
}

pub fn required_string(arguments: &Json, name: &str) -> Result<String, ToolRefusal> {
    match argument(arguments, name).as_str() {
        Some(value) if !value.is_empty() => Ok(String::from(value)),
        _ => Err(ToolRefusal::new(format!(
            "\"{name}\" must be a non-empty string"
        ))),
    }
}

pub fn optional_bool(arguments: &Json, name: &str, fallback: bool) -> Result<bool, ToolRefusal> {
    let value = argument(arguments, name);
    if value.is_null() {
        return Ok(fallback);
    }
    value
        .as_bool()
        .ok_or_else(|| ToolRefusal::new(format!("\"{name}\" must be true or false")))
}

/// A scope as the one grammar's words (cad's format_scope_words), from a
/// tool's JSON: `kind` selection, drawing, area (with `area` [x0, y0, x1, y1])
/// or layers (with `layers` ["a", "b"] and `only`), and `conditions` (the
/// C++'s `where`) a list of conditions ["TYPE=polyline", "PROP=owner:Smith*"]
/// (null for none). VIEW is the window's and is refused. A refusal naming what
/// is wrong. The ONE reading of a scope in JSON: katana_gdal_run's sources,
/// katana_gis_query, katana_import and katana_export all say a scope through
/// it.
pub fn scope_words_of(
    kind: &str,
    area: &Json,
    layers: &Json,
    only: bool,
    conditions: &Json,
) -> Result<String, ToolRefusal> {
    let mut scope = ScopeWords::default();
    match kind {
        "selection" => scope.source = ScopeSource::Selection,
        "drawing" => scope.source = ScopeSource::Drawing,
        "area" => {
            if !area.is_array() || area.size() != 4 || !area.iter().all(|n| n.is_number()) {
                return Err(ToolRefusal::new(
                    "scope area needs \"area\": [x0, y0, x1, y1]",
                ));
            }
            scope.source = ScopeSource::Area;
            let x0 = area[0].get::<f64>()?;
            let y0 = area[1].get::<f64>()?;
            let x1 = area[2].get::<f64>()?;
            let y1 = area[3].get::<f64>()?;
            scope.area = Box2::new(
                Point2::new(min(x0, x1), min(y0, y1)),
                Point2::new(max(x0, x1), max(y0, y1)),
            );
        }
        "layers" => {
            if !layers.is_array() || layers.empty() || !layers.iter().all(|l| l.is_string()) {
                return Err(ToolRefusal::new(
                    "scope layers needs \"layers\": [\"a\", \"b\"]",
                ));
            }
            scope.source = ScopeSource::Layers;
            for layer in layers.iter() {
                scope.layers.push(layer.get::<String>()?);
            }
            scope.sublayers = !only;
        }
        _ => {
            return Err(ToolRefusal::new(
                "\"scope\" is selection, drawing, area or layers (VIEW is the window's)",
            ));
        }
    }
    if !conditions.is_null() {
        if !conditions.is_array() {
            return Err(ToolRefusal::new(
                "the scope's conditions are a list: [\"TYPE=polyline\"]",
            ));
        }
        for condition in conditions.iter() {
            let Some(condition) = condition.as_str() else {
                return Err(ToolRefusal::new(
                    "each of the scope's conditions is a string",
                ));
            };
            parse_where_condition(condition, &mut scope.filter)
                .map_err(|error| ToolRefusal::new(error.describe()))?;
        }
    }
    format_scope_words(&scope).map_err(|error| ToolRefusal::new(error.describe()))
}

/// The scope a tool's arguments say, as the words of the same grammar, or
/// None when they say none: "scope" (selection, drawing, area or layers), an
/// "area" alone, "layers" with "only", and "where" - which alone filters the
/// selection, as WHERE alone does on the command line, written SELECTION
/// WHERE ... so the line says what it takes. "layers" without "scope" is
/// refused. The arguments katana_export and katana_road_objects share.
pub fn scope_argument_words(arguments: &Json) -> Result<Option<String>, ToolRefusal> {
    let scope = argument(arguments, "scope");
    let area = argument(arguments, "area");
    if !scope.is_null() || !area.is_null() {
        if !scope.is_null() && !scope.is_string() {
            return Err(ToolRefusal::new(
                "\"scope\" is selection, drawing, area or layers",
            ));
        }
        return scope_words_of(
            scope.as_str().unwrap_or("area"),
            area,
            argument(arguments, "layers"),
            optional_bool(arguments, "only", false)?,
            argument(arguments, "where"),
        )
        .map(Some);
    }
    if !argument(arguments, "where").is_null() && argument(arguments, "layers").is_null() {
        // "where" alone filters the selection, as WHERE alone does on the
        // command line (the one scope parser's rule, MODIFY's): written
        // SELECTION WHERE ..., so the line says what it takes.
        return scope_words_of(
            "selection",
            area,
            &Json::Null,
            false,
            argument(arguments, "where"),
        )
        .map(Some);
    }
    if !argument(arguments, "layers").is_null() {
        return Err(ToolRefusal::new(
            "\"layers\" says a scope: give \"scope\": \"layers\" with it",
        ));
    }
    Ok(None)
}

// ---- katana_check_update -------------------------------------------------------------------

/// `std::string::npos`, for the C++'s finds.
const NPOS: usize = usize::MAX;

/// The first `byte` in `text` at or after `from`; NPOS for none.
fn find_byte(text: &[u8], byte: u8, from: usize) -> usize {
    text.get(from..)
        .and_then(|rest| rest.iter().position(|&each| each == byte))
        .map_or(NPOS, |at| from + at)
}

// One reply record as an object: each key=value word a member, a value in
// quotes read back as cad's record_value wrote it (always text), and a plain
// value of digits alone a number. A word that is no key=value - the name a
// record begins with - is not a member.
fn record_object(record: &str) -> Json {
    let bytes = record.as_bytes();
    let mut object = Json::object();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b' ' {
            at += 1;
            continue;
        }
        let equals = find_byte(bytes, b'=', at);
        let blank = find_byte(bytes, b' ', at);
        if equals == NPOS || blank < equals {
            at = if blank == NPOS { bytes.len() } else { blank };
            continue;
        }
        let key = String::from_utf8_lossy(&bytes[at..equals]).into_owned();
        at = equals + 1;
        if at < bytes.len() && bytes[at] == b'"' {
            let mut value = Vec::new();
            at += 1;
            while at < bytes.len() && bytes[at] != b'"' {
                if bytes[at] == b'\\' && at + 1 < bytes.len() {
                    at += 1;
                }
                value.push(bytes[at]);
                at += 1;
            }
            // past the closing quote
            if at < bytes.len() {
                at += 1;
            }
            object[key.as_str()] = Json::from(String::from_utf8_lossy(&value).into_owned());
            continue;
        }
        let end = find_byte(bytes, b' ', at).min(bytes.len());
        let value = &bytes[at..end];
        at = end;
        // Eighteen digits fit a 64-bit number whatever they are.
        let number = !value.is_empty() && value.len() <= 18 && value.iter().all(u8::is_ascii_digit);
        object[key.as_str()] = if number {
            Json::from(
                value
                    .iter()
                    .fold(0u64, |whole, &digit| whole * 10 + u64::from(digit - b'0')),
            )
        } else {
            Json::from(String::from_utf8_lossy(value).into_owned())
        };
    }
    object
}

// An UPDATE reply as an object: its first line is the record, and the text of
// each `warning text="..."` after it an entry of `warnings`.
fn update_reply_object(text: &str) -> Json {
    let mut object = Json::object();
    let mut warnings = Vec::new();
    let mut first = true;
    let mut rest = text;
    while !rest.is_empty() {
        let end = rest.find('\n');
        let line = &rest[..end.unwrap_or(rest.len())];
        if first {
            object = record_object(line);
            first = false;
        } else {
            let extra = record_object(line);
            if let Some(warning) = extra.find("text") {
                warnings.push(warning.clone());
            }
        }
        match end {
            None => break,
            Some(end) => rest = &rest[end + 1..],
        }
    }
    if !warnings.is_empty() {
        object["warnings"] = Json::from(warnings);
    }
    object
}

/// katana_check_update's reply for `context` (mcp::server): UPDATE CHECK and
/// UPDATE STATUS as the verbs answer them (update_verbs), one line each in the
/// text, and in the structured content as the objects `check` and `status` -
/// a member for each key=value of the record, a number where the value is
/// one - with `warnings` in either when the reply had any. The tool passes the
/// running program's context for the Mcp surface, which keeps no per-user
/// state; a test passes one over a fake feed. An error when a line could not
/// be run at all: a feed that cannot be read is a reply (update=unreachable).
pub fn check_update_reply(context: &UpdateContext<'_>) -> ToolReply {
    // The verbs' own two lines, run for the context given: what the tool says
    // is what UPDATE CHECK and UPDATE STATUS say, word for word.
    let check = match run_update_line(context, "UPDATE CHECK") {
        Ok(check) => check,
        Err(error) => {
            return ToolReply {
                text: format!("error: {}", error.describe()),
                structured: Json::Null,
                is_error: true,
            };
        }
    };
    let status = match run_update_line(context, "UPDATE STATUS") {
        Ok(status) => status,
        Err(error) => {
            return ToolReply {
                text: format!("error: {}", error.describe()),
                structured: Json::Null,
                is_error: true,
            };
        }
    };
    ToolReply {
        text: format!("{}\n{}", check.text, status.text),
        structured: Json::object_of([
            ("check", update_reply_object(&check.text)),
            ("status", update_reply_object(&status.text)),
        ]),
        is_error: false,
    }
}

// ---- the schemas' JSON -----------------------------------------------------------------------
//
// The C++ writes a schema as nlohmann's initializer lists; these two say the
// shapes it repeats, so a tool's row reads as the C++'s does.

/// One argument's schema: its JSON type (a word, or a list of words) and
/// what it is for, with any other keywords (enum, items, bounds) beside them.
pub(crate) fn property<const N: usize>(
    kind: Json,
    description: &str,
    more: [(&str, Json); N],
) -> Json {
    let mut schema = Json::object_of([("type", kind), ("description", Json::from(description))]);
    for (key, value) in more {
        schema[key] = value;
    }
    schema
}

/// A list of words, as `{"a", "b"}` makes one in the C++.
pub(crate) fn words<const N: usize>(values: [&str; N]) -> Json {
    Json::array_of(values.map(Json::from))
}

/// core's lowered, as text: it changes the ASCII letters alone, so UTF-8 in
/// is UTF-8 out. What the tools' filters fold case with (searching folds
/// case; storing never does).
pub(crate) fn lowered_text(text: &str) -> String {
    String::from_utf8_lossy(&katana_core::text::lowered(text)).into_owned()
}
