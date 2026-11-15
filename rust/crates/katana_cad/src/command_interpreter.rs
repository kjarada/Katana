//! Text command line for a Document (PLAN.MD Phase 08 "command line";
//! include/katana/cad/command_interpreter.hpp,
//! src/katana_cad/command_interpreter.cpp).
//!
//! One line in, one command out: the interpreter only parses and then builds
//! the same Command objects the GUI uses, so everything typed is validated,
//! atomic and undoable. It has no GUI dependency; the desktop application and
//! the headless `katana_cli` share it, and `katana_mcp` runs its lines.
//!
//! ```text
//! Syntax
//!   VERB arg arg ...                 verbs are case-insensitive, with aliases
//!   points      12.5,40   absolute
//!               @3,4      relative to the last point entered
//!               @5<30     polar: distance < angle (degrees, counter-clockwise from +x)
//!   angles      degrees
//!   text        "quoted strings may contain spaces"
//!   ids         entity ids as shown by LIST
//! ```
//!
//! Numbers are parsed locale-independently ('.' is always the decimal
//! point). HELP lists every command.
//!
//! The annotation verbs are in annotation/annotation_verbs.rs and the drawing
//! system's in drawing/drawing_verbs.rs, as `impl CommandInterpreter` blocks:
//! the C++ keeps those members beside the rest of their code.
//!
//! # The interpreter does not hold the Document (decided by the port, wave I)
//!
//! The C++ interpreter keeps a `Document&` for its whole life, and its
//! front ends read and change that Document between lines: the window paints
//! it, a test asserts on its model after each `run`, the session saves it. In
//! Rust an interpreter holding `&mut Document` would lock the Document away
//! for as long as the interpreter lives, so nothing could look at it between
//! two lines. So the Document is LENT for each line: `run(&mut self, &mut
//! Document, line)`. The interpreter keeps only what is its own - the history,
//! the last point, and the contexts a front end hands it (the sheet context,
//! the customisation host, the scope's view provider, the views' host).
//!
//! Rejected: `CommandInterpreter<'d> { document: &'d mut Document }`, the C++
//! shape, for the reason above; and the Document shared as
//! `Rc<RefCell<Document>>`, which would turn every reader into a run-time
//! borrow that can panic, in a crate whose Document is borrowed by reference
//! everywhere else.
//!
//! The contexts are owned and `'static`: a front end hands over a boxed
//! closure (the sheet context, the scope's view provider) and its views' host
//! as `Rc<dyn ViewVerbHost>`, which its own scope provider may share - the
//! host keeps its ViewSet in a RefCell that both read (view_verbs.rs). That
//! makes the interpreter single-threaded, as the C++'s std::function members
//! and the window's main thread are. A front end that replaces its host or
//! its views calls the setter again: the C++ asks its providers at each line,
//! here the front end hands over what it has whenever that changes.

use std::collections::BTreeSet;
use std::rc::Rc;

use katana_commands::{
    Transaction, array_entities, chamfer_entities, copy_entities, create_alignment, create_arc,
    create_circle, create_dimension, create_dimension_style, create_hatch_pattern, create_layer,
    create_line, create_linetype, create_point, create_polyline, create_style, create_text,
    delete_alignment, delete_dimension_style, delete_entities, delete_hatch_pattern, delete_layer,
    delete_linetype, delete_style, extend_entity, fillet_entities, merge_linetype, merge_style,
    mirror_entities, move_entities, offset_entity, remove_entity_property, rename_entity_property,
    rename_linetype, rename_style, rotate_entities, scale_entities, set_entity_color,
    set_entity_layer, set_entity_property, set_entity_style, trim_entity, update_alignment,
    update_dimension_style, update_hatch_pattern, update_layer, update_style_if_changed,
};
use katana_core::error::make_error;
use katana_core::path_text::path_from_utf8;
use katana_core::text::{equals_ignoring_case, format_exact_real, format_general, is_ascii_space};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::display::{BY_LAYER_LINETYPE, is_by_layer};
use katana_entity::entity::{entity_type_from_string, property_to_string, type_name};
use katana_entity::leader_values::leader_note;
use katana_entity::table_usage::{TableUsage, UsageOptions, table_usage};
use katana_entity::tables::is_built_in_symbol_name;
use katana_entity::{
    Alignment, CalloutShape, Color, DimensionGeometry, DimensionKind, DimensionStyle, Entity,
    EntityId, EnumName, Geometry, HatchLineFamily, HatchPattern, Layer, LeaderContext, Linetype,
    LinetypeElement, Model, PropertyValue, Style, TextGeometry, TextJustify,
};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::profile::solve_profile;
use katana_geometry::{AlignmentPi, Arc2, Point2, ProfilePvi, Rectangle2, Vec2, VerticalAlignment};
use katana_math::{DEG_TO_RAD, RAD_TO_DEG};

use crate::alignment_report::{format_profile_report, format_setting_out, setting_out_stations};
use crate::annotation::dimension_style_verbs::{
    describe_dimension_style, set_dimension_style_fields,
};
use crate::annotation::leader_draw::code_properties;
use crate::annotation::leader_edit::{ReadKind, leaders_reading};
use crate::annotation::leader_presets::leader_context;
use crate::customisation_host::CustomisationHost;
use crate::customisation_verbs::{
    CustomisationVerbContext, customisation_verb_context, customisation_verb_help,
    is_customisation_verb, run_customisation_verb,
};
use crate::document::Document;
use crate::document_status::{document_status, format_status, status_json};
use crate::global_modify::{GlobalModify, ModifyFilter, SymbolModify, plan_global_modify};
use crate::import_placement::{ImportPlacement, parse_placement_word};
use crate::layer_overrides::NO_LAYER_OVERRIDES;
use crate::linework_verbs::{is_linework_verb, linework_verb_help, run_linework_verb};
use crate::parcel::{format_parcel_report, legal_description, parcel_labels, parcel_report};
use crate::plotting::sheet_verbs::{
    SheetVerbContextFn, is_sheet_verb, run_sheet_verb, sheet_verb_help,
};
use crate::project_crs::{
    CrsChoice, describe_coordinate_system, find_coordinate_systems, suggest_coordinate_systems,
};
use crate::property_outline::property_tree_reply;
use crate::purge::{PurgeOptions, plan_purge, purge_command};
use crate::road::road_object_verbs::{
    is_road_object_verb, road_object_verb_help, run_road_object_verb,
};
use crate::road::road_verbs::{is_road_verb, road_verb_help, run_road_verb};
use crate::scope_verbs::{
    ScopeMatch, ScopeSource, ScopeViewFn, ScopeViewProvider, is_scope_word, match_scope,
    parse_colour_or_by_layer, parse_scope_words, parse_where_condition, resolve_scope,
    scope_record, type_tally,
};
use crate::selection::{SelectionFilter, is_selectable};
use crate::style_catalogue::{CatalogueEntry, check_linetype_name, filter_choices, symbol_choices};
use crate::survey_code_verbs::{is_survey_code_verb, run_survey_code_verb};
use crate::survey_tools::{
    ForwardInput, InverseResult, c_fixed, compute_area, compute_forward, compute_inverse,
    ends_of_line, format_area_report, format_forward_report, format_inverse_report,
    forward_point_command, parse_survey_direction, parse_survey_number, parse_survey_position,
};
use crate::terrain::surface_display_verbs::{
    is_surface_display_verb, run_surface_display_verb, surface_display_verb_help,
};
use crate::terrain::surface_edit_verbs::{
    is_surface_edit_verb, run_surface_edit_verb, surface_edit_help_summary, surface_edit_verb_help,
};
use crate::terrain::surface_verbs::{
    is_surface_verb, run_surface_verb, surface_verb_help, surface_verb_summary,
};
use crate::terrain::volume_verbs::{is_volume_verb, run_volume_verb, volume_verb_help};
use crate::utilities::utility_verbs::{is_utility_verb, run_utility_verb, utility_verb_help};
use crate::view_verbs::{ViewVerbHost, is_view_verb, run_view_verb};

/// The rest of a front end's IMPORT line, after the verb: the path, one layer
/// of surrounding quotes removed so that a path may hold blanks, and where the
/// data is to land (import_placement.rs). See
/// [`CommandInterpreter::import_argument`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportArgument {
    pub path: String,
    pub placement: ImportPlacement,
}

/// The text command line: see the module comment for why the Document is
/// lent to each [`run`](Self::run) rather than held.
#[derive(Default)]
pub struct CommandInterpreter {
    pub(crate) history: Vec<String>,
    pub(crate) last_point: Option<Point2>,
    pub(crate) sheet_context: Option<Box<SheetVerbContextFn<'static>>>,
    pub(crate) customisation: CustomisationVerbContext,
    pub(crate) scope_views: Option<Box<ScopeViewFn<'static>>>,
    pub(crate) view_host: Option<Rc<dyn ViewVerbHost>>,
}

// ---- words and numbers ------------------------------------------------------------------

/// The C++ `core::uppered` of a word, as text: ASCII letters only, so a word
/// that is UTF-8 stays UTF-8.
pub(crate) fn uppered(word: &str) -> String {
    word.to_ascii_uppercase()
}

// Splits on whitespace; double quotes group words and are removed. The
// blanks are core::is_ascii_space's, never the locale's: in a Latin-1 locale
// isspace(0xA0) is true, and 0xA0 is the second byte of a UTF-8 a-grave.
fn tokenize(line: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for ch in line.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            has_token = true; // "" is a valid empty token
        } else if !in_quotes && ch.is_ascii() && is_ascii_space(ch as u8) {
            if has_token {
                tokens.push(std::mem::take(&mut current));
                has_token = false;
            }
        } else {
            current.push(ch);
            has_token = true;
        }
    }
    if in_quotes {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "unterminated quoted string",
        ));
    }
    if has_token {
        tokens.push(current);
    }
    Ok(tokens)
}

// core::trimmed of text: the ASCII blanks is_ascii_space names.
fn trimmed_text(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_ascii() && is_ascii_space(c as u8))
}

// The text of `line` after its first `words` tokens, exactly as typed and
// trimmed: quotes, commas and runs of blanks kept. tokenize() removes every
// double quote, which is right for a name and ruinous for a WKT, whose names
// are all in quotes; a verb that takes a WKT reads it from here. The tokens
// are walked by tokenize()'s own rules, so "the first two words" means the
// same here as there.
fn text_after_tokens(line: &str, words: usize) -> &str {
    let bytes = line.as_bytes();
    let mut at = 0;
    for _ in 0..words {
        while at < bytes.len() && is_ascii_space(bytes[at]) {
            at += 1;
        }
        let mut in_quotes = false;
        while at < bytes.len() && (in_quotes || !is_ascii_space(bytes[at])) {
            if bytes[at] == b'"' {
                in_quotes = !in_quotes;
            }
            at += 1;
        }
    }
    // Every stop is at an ASCII byte or the end, so `at` is a character
    // boundary.
    trimmed_text(&line[at..])
}

// std::from_chars into an unsigned integer: digits only, no sign, no blank,
// and nothing that does not fit.
fn parse_unsigned<T: std::str::FromStr>(text: &str) -> Option<T> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

// std::from_chars into a signed integer: an optional '-' (never a '+') and
// digits, and nothing that does not fit.
pub(crate) fn parse_signed<T: std::str::FromStr>(text: &str) -> Option<T> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

// std::from_chars is locale independent: "1.5" never becomes "1,5". One plus
// sign is taken off first, which from_chars rejects; what follows is
// from_chars' grammar, a second sign included, and the value must be finite.
fn parse_number(text: &str) -> Result<f64> {
    let body = text.strip_prefix('+').unwrap_or(text);
    let value = if body.starts_with('+') {
        None
    } else {
        katana_core::text::parse_finite_double(body)
    };
    value.ok_or_else(|| make_error(ErrorCode::ParseFailure, "expected a number", text))
}

// "station,elevation" or "station,elevation,curveLength", as ALIGN DESIGN
// takes its PVIs - one token per PVI, so a profile of any length is one line.
fn parse_pvi(text: &str) -> Result<ProfilePvi> {
    let malformed = || {
        make_error(
            ErrorCode::InvalidArgument,
            "a PVI is station,elevation[,curveLength]",
            text,
        )
    };
    let mut numbers = Vec::new();
    for part in text.split(',') {
        numbers.push(parse_number(part).map_err(|_| malformed())?);
    }
    if numbers.len() < 2 || numbers.len() > 3 {
        return Err(malformed());
    }
    Ok(ProfilePvi {
        station: numbers[0],
        elevation: numbers[1],
        curve_length: if numbers.len() == 3 { numbers[2] } else { 0.0 },
    })
}

// "x,y[,radius[,spiralIn[,spiralOut]]]", as ALIGN PIS takes its PIs - one
// token per PI, the horizontal counterpart of parse_pvi.
fn parse_alignment_pi(text: &str) -> Result<AlignmentPi> {
    let malformed = || {
        make_error(
            ErrorCode::InvalidArgument,
            "a PI is x,y[,radius[,spiralIn[,spiralOut]]]",
            text,
        )
    };
    let mut numbers = Vec::new();
    for part in text.split(',') {
        numbers.push(parse_number(part).map_err(|_| malformed())?);
    }
    if numbers.len() < 2 || numbers.len() > 5 {
        return Err(malformed());
    }
    let mut pi = AlignmentPi {
        point: Point2::new(numbers[0], numbers[1]),
        ..AlignmentPi::default()
    };
    let fields = [&mut pi.radius, &mut pi.spiral_in, &mut pi.spiral_out];
    for (field, &value) in fields.into_iter().zip(&numbers[2..]) {
        *field = value;
    }
    Ok(pi)
}

fn parse_id(text: &str) -> Result<EntityId> {
    match parse_unsigned::<EntityId>(text) {
        Some(value) if value != 0 => Ok(value),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            "expected an entity id",
            text,
        )),
    }
}

fn parse_count(text: &str) -> Result<i32> {
    match parse_signed::<i32>(text) {
        Some(value) if value >= 1 => Ok(value),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            "expected a positive whole number",
            text,
        )),
    }
}

fn usage(text: &str) -> Error {
    Error::new(ErrorCode::InvalidArgument, format!("usage: {text}"))
}

// The C++ alias table, in its order: each short word and the verb it names.
const ALIASES: [(&str, &str); 46] = [
    ("L", "LINE"),
    ("PL", "PLINE"),
    ("POLYLINE", "PLINE"),
    ("C", "CIRCLE"),
    ("A", "ARC"),
    ("PO", "POINT"),
    ("REC", "RECT"),
    ("RECTANGLE", "RECT"),
    ("T", "TEXT"),
    ("DIMENSION", "DIM"),
    ("M", "MOVE"),
    ("CO", "COPY"),
    ("CP", "COPY"),
    ("RO", "ROTATE"),
    ("SC", "SCALE"),
    ("MI", "MIRROR"),
    ("AR", "ARRAY"),
    ("E", "ERASE"),
    ("DELETE", "ERASE"),
    ("DEL", "ERASE"),
    ("O", "OFFSET"),
    ("TR", "TRIM"),
    ("EX", "EXTEND"),
    ("F", "FILLET"),
    ("CHA", "CHAMFER"),
    ("U", "UNDO"),
    ("LA", "LAYER"),
    ("SEL", "SELECT"),
    ("LT", "LINETYPE"),
    ("LTYPE", "LINETYPE"),
    ("DS", "DIMSTYLE"),
    ("HA", "HATCH"),
    ("AL", "ALIGN"),
    ("PARC", "PARCEL"),
    ("ST", "STYLE"),
    ("RADIATE", "FORWARD"),
    ("?", "HELP"),
    ("LE", "LEADER"),
    ("MT", "MTEXT"),
    ("TS", "TEXTSTYLE"),
    ("LS", "LABELSTYLE"),
    ("GM", "MODIFY"),
    ("GMODIFY", "MODIFY"),
    ("GLOBALMODIFY", "MODIFY"),
    ("Z", "ZOOM"),
    ("CUSTOMIZE", "CUSTOMISE"),
];

// The verb a line's first word names, as run() takes it: in capitals, an
// alias resolved, and the apostrophe of AutoCAD's transparent form taken off
// a view verb ('ZOOM, 'Z). On any other word the apostrophe stays, so the
// word is no verb and run() refuses it. One reading for run(), for
// replaces_document and for the window's question (verb_of).
fn canonical_verb(word: &str) -> String {
    let verb = uppered(word);
    let apostrophe = verb.len() > 1 && verb.starts_with('\'');
    let mut bare = if apostrophe {
        String::from(&verb[1..])
    } else {
        verb.clone()
    };
    if let Some((_, target)) = ALIASES.iter().find(|(alias, _)| *alias == bare) {
        bare = String::from(*target);
    }
    if apostrophe && !is_view_verb(&bare) {
        verb
    } else {
        bare
    }
}

// A line break in a text is written \n, so one entity stays one line of the
// reply a script or an agent reads.
fn one_line(text: &str) -> String {
    text.replace('\n', "\\n")
}

// A stream at precision 12 in its default notation (%.12g).
fn g12(value: f64) -> String {
    format_general(value, 12)
}

// `leaders`: the drawing's surfaces and alignments, which a leader's note may
// read (annotation/leader_presets.rs).
fn describe(model: &Model, entity: &Entity, leaders: &LeaderContext) -> String {
    let mut out = format!(
        "{}  {}  layer={}",
        entity.id,
        entity.r#type().name(),
        entity.layer
    );
    match &entity.geometry {
        Geometry::Point(g) => {
            out += &format!("  at {},{}", g12(g.position.x), g12(g.position.y));
        }
        Geometry::Line(g) => {
            out += &format!(
                "  {},{} -> {},{}  length={}",
                g12(g.start.x),
                g12(g.start.y),
                g12(g.end.x),
                g12(g.end.y),
                g12(g.length())
            );
        }
        Geometry::Arc(g) => {
            out += &format!(
                "  centre {},{}  radius={}  length={}",
                g12(g.center.x),
                g12(g.center.y),
                g12(g.radius),
                g12(g.length())
            );
        }
        Geometry::Polyline(g) => {
            out += &format!(
                "  vertices={}{}  length={}",
                g.vertices.len(),
                if g.closed { "  closed" } else { "  open" },
                g12(g.length())
            );
            if g.closed {
                out += &format!("  area={}", g12(g.area()));
            }
        }
        Geometry::Circle(g) => {
            out += &format!(
                "  centre {},{}  radius={}  area={}",
                g12(g.center.x),
                g12(g.center.y),
                g12(g.radius),
                g12(g.area())
            );
        }
        Geometry::Text(g) => {
            out += &format!("  \"{}\"  height={}", one_line(&g.text), g12(g.height));
            if !g.style.is_empty() {
                out += &format!("  style={}", g.style);
            }
            if g.paper_height > 0.0 {
                out += &format!("  paper={}", g12(g.paper_height));
            }
            if g.justify != TextJustify::BottomLeft {
                out += &format!("  justify={}", g.justify.name());
            }
        }
        Geometry::Dimension(g) => {
            if g.kind != DimensionKind::Aligned {
                out += &format!("  {}", g.kind.name());
            }
            out += &format!("  measures {}", g12(g.measurement()));
            if g.start_ref.associated() || g.end_ref.associated() || g.vertex_ref.associated() {
                out += "  associative";
            }
        }
        Geometry::Label(g) => {
            out += &format!("  style={}", g.style);
            if g.target != 0 {
                out += &format!("  target={}", g.target);
            } else {
                out += &format!("  alignment={}", g.alignment);
            }
            if g.part >= 0 {
                out += &format!("  part={}", g.part);
            }
            if !g.rule.is_empty() {
                out += &format!("  rule={}", g.rule);
            }
            if let Some(position) = g.position {
                out += &format!("  at {},{}", g12(position.x), g12(position.y));
            }
        }
        Geometry::Leader(g) => {
            let tip = g.vertices.first().copied().unwrap_or_default();
            out += &format!(
                "  vertices={}  tip {},{}",
                g.vertices.len(),
                g12(tip.x),
                g12(tip.y)
            );
            // What the leader says as it is drawn: a smart leader's note is
            // read off its target now (docs/annotation.md, "Smart leaders").
            let note = leader_note(model, g, code_properties(), leaders);
            if !note.is_empty() {
                out += &format!("  \"{}\"", one_line(&note));
            }
            if g.tip_ref.associated() {
                out += &format!(
                    "  on {} {}",
                    g.tip_ref.entity,
                    katana_entity::anchor::describe(&g.tip_ref)
                );
            }
            if g.fields {
                out += &format!("  template=\"{}\"", one_line(&g.text));
            }
            if !g.label_style.is_empty() {
                out += &format!("  labelstyle={}", g.label_style);
            }
            if g.callout != CalloutShape::None {
                out += &format!("  callout={}", g.callout.name());
            }
        }
        Geometry::CurvePolyline(g) => {
            let arcs = (0..g.segment_count()).filter(|&i| g.is_arc(i)).count();
            let with_height = g
                .vertices
                .iter()
                .filter(|vertex| vertex.height.is_some())
                .count();
            out += &format!(
                "  vertices={}{}  arcs={arcs}  heights={with_height}  length={}",
                g.vertices.len(),
                if g.closed { "  closed" } else { "  open" },
                g12(g.length())
            );
            if g.closed {
                out += &format!("  area={}", g12(g.area()));
            }
        }
        Geometry::Ellipse(g) => {
            out += &format!(
                "  centre {},{}  major={}  minor={}  rotation={}",
                g12(g.center.x),
                g12(g.center.y),
                g12(g.major_radius()),
                g12(g.minor_radius()),
                g12(g.major_axis.angle() * RAD_TO_DEG)
            );
            if !g.is_full() {
                out += &format!(
                    "  start={}  sweep={}",
                    g12(g.start_parameter * RAD_TO_DEG),
                    g12(g.sweep * RAD_TO_DEG)
                );
            }
            out += &format!("  length={}", g12(g.length()));
        }
        Geometry::Spline(g) => {
            out += &format!("  degree={}  control={}", g.degree, g.control_points.len());
            if g.has_fit_points() {
                out += &format!("  fit={}", g.fit_points.len());
            }
            out += &format!("  length={}", g12(g.length()));
        }
    }
    if !entity.visible {
        out += "  hidden";
    }
    out
}

// A value of a STATED type, for the times the guess below would be wrong: a
// code that reads as a number ("2"), a level that must stay real ("31").
fn typed_property_value(r#type: &str, text: &str) -> Result<PropertyValue> {
    let wanted = uppered(r#type);
    if wanted == "TEXT" || wanted == "STRING" {
        return Ok(PropertyValue::Text(String::from(text)));
    }
    if wanted == "INTEGER" || wanted == "INT" {
        return match parse_signed::<i64>(text) {
            Some(integer) => Ok(PropertyValue::Integer(integer)),
            None => Err(make_error(ErrorCode::ParseFailure, "not an integer", text)),
        };
    }
    if wanted == "REAL" || wanted == "DOUBLE" {
        return Ok(PropertyValue::Real(parse_number(text)?));
    }
    if wanted == "BOOLEAN" || wanted == "BOOL" {
        let folded = uppered(text);
        if folded == "TRUE" || folded == "1" {
            return Ok(PropertyValue::Boolean(true));
        }
        if folded == "FALSE" || folded == "0" {
            return Ok(PropertyValue::Boolean(false));
        }
        return Err(make_error(ErrorCode::ParseFailure, "not a boolean", text));
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        "the type must be text, integer, real or boolean",
        r#type,
    ))
}

// "true"/"false" -> bool, whole numbers -> integer, other numbers -> real,
// else text.
fn parse_property_value(text: &str) -> PropertyValue {
    let folded = uppered(text);
    if folded == "TRUE" {
        return PropertyValue::Boolean(true);
    }
    if folded == "FALSE" {
        return PropertyValue::Boolean(false);
    }
    if let Some(integer) = parse_signed::<i64>(text) {
        return PropertyValue::Integer(integer);
    }
    if let Ok(number) = parse_number(text) {
        return PropertyValue::Real(number);
    }
    PropertyValue::Text(String::from(text))
}

// `status` is the command's; on success the reply is `message`.
pub(crate) fn finish(status: Status, message: String) -> Result<String> {
    status?;
    Ok(message)
}

// ---- the table managers' verbs ------------------------------------------------------

fn rest_of_line(args: &[String], from: usize) -> String {
    args.get(from..)
        .map(|rest| rest.join(" "))
        .unwrap_or_default()
}

fn counted_noun(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn listed(names: &[String]) -> String {
    names.join(", ")
}

// Text built line by line with a "\n" after each, the last one taken off.
fn without_last_newline(mut text: String) -> String {
    text.pop();
    text
}

// PURGE [STYLES|LINETYPES|HATCHES|ALL]. The current style is kept although
// nothing wears it yet: it is what the next thing drawn will be.
fn purge_tables(document: &mut Document, args: &[String]) -> Result<String> {
    const PURGE_USAGE: &str = "PURGE [STYLES|LINETYPES|HATCHES|ALL]";
    if args.len() > 1 {
        return Err(usage(PURGE_USAGE));
    }
    let what = args
        .first()
        .map_or_else(|| String::from("ALL"), |word| uppered(word));
    let mut options = PurgeOptions::default();
    match what.as_str() {
        "STYLES" => {
            options.linetypes = false;
            options.hatches = false;
        }
        "LINETYPES" => {
            options.styles = false;
            options.hatches = false;
        }
        "HATCHES" => {
            options.styles = false;
            options.linetypes = false;
        }
        "ALL" => {}
        _ => return Err(usage(PURGE_USAGE)),
    }
    if !document.current_style().is_empty() {
        options
            .keep_styles
            .push(String::from(document.current_style()));
    }
    let plan = plan_purge(document.model(), &options);
    let Some(command) = purge_command(document.model(), &options)? else {
        return Ok(String::from("nothing to purge: everything is used"));
    };
    document.execute(command)?;
    let mut reply = format!(
        "purged {}, {} and {} (one UNDO restores them)",
        counted_noun(plan.styles.len(), "style", "styles"),
        counted_noun(plan.linetypes.len(), "linetype", "linetypes"),
        counted_noun(plan.hatch_patterns.len(), "hatch pattern", "hatch patterns")
    );
    for (label, names) in [
        ("styles", &plan.styles),
        ("linetypes", &plan.linetypes),
        ("hatch patterns", &plan.hatch_patterns),
    ] {
        if !names.is_empty() {
            reply += &format!("\n  {label}: {}", listed(names));
        }
    }
    Ok(reply)
}

// ---- global modify ---------------------------------------------------------------------------

const MODIFY_USAGE: &str = "MODIFY [SELECTION|DRAWING|VIEW [id] [EXTENTS]|AREA x0,y0,x1,y1|LAYERS \
                            a,b [ONLY]] [WHERE key=value ...] SET key=value ... [PREVIEW]; HELP \
                            lists the keys";

fn parse_yes_no(text: &str) -> Result<bool> {
    let folded = uppered(text);
    match folded.as_str() {
        "YES" | "ON" | "TRUE" | "1" => Ok(true),
        "NO" | "OFF" | "FALSE" | "0" => Ok(false),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            "expected yes or no",
            text,
        )),
    }
}

fn parse_set_word(word: &str, change: &mut GlobalModify) -> Status {
    let Some(equals) = word.find('=') else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "a SET field is key=value",
            word,
        ));
    };
    let key = uppered(&word[..equals]);
    let value = &word[equals + 1..];
    let entities = &mut change.entities;
    let layers = &mut change.layers;
    let styles = &mut change.styles;
    match key.as_str() {
        "LAYER" => entities.layer = Some(String::from(value)),
        "COLOUR" | "COLOR" => entities.colour = Some(parse_colour_or_by_layer(value)?),
        "STYLE" => {
            entities.style = Some(if uppered(value) == "BYLAYER" {
                String::new()
            } else {
                String::from(value)
            });
        }
        "VISIBLE" => entities.visible = Some(parse_yes_no(value)?),
        "PROP" => {
            let colon = value.find(':');
            let Some(colon) = colon.filter(|&colon| colon != 0) else {
                return Err(make_error(ErrorCode::ParseFailure, "PROP=key:value", word));
            };
            entities.set_properties.push((
                String::from(&value[..colon]),
                parse_property_value(&value[colon + 1..]),
            ));
        }
        "UNPROP" => entities.remove_properties.push(String::from(value)),
        "HEIGHT" => entities.text_height = Some(parse_number(value)?),
        "SYMBOL" => {
            // A library symbol's name may hold spaces ("CULT Bollard"), so it
            // is quoted: SYMBOL="CULT Bollard@0.5".
            let mut symbol = SymbolModify::default();
            match value.rfind('@') {
                Some(at) => {
                    symbol.name = String::from(&value[..at]);
                    symbol.size = parse_number(&value[at + 1..])?;
                }
                None => symbol.name = String::from(value),
            }
            entities.symbol = Some(symbol);
        }
        "LAYER.COLOUR" | "LAYER.COLOR" => layers.colour = Some(Color::from_hex(value)?),
        "LAYER.LTYPE" | "LAYER.LINETYPE" => layers.linetype = Some(String::from(value)),
        "LAYER.WEIGHT" => layers.line_weight = Some(parse_number(value)?),
        "LAYER.HATCH" => layers.hatch_pattern = Some(String::from(value)),
        "LAYER.DIMSTYLE" => {
            layers.dimension_style = Some(if value == "-" {
                String::new()
            } else {
                String::from(value)
            });
        }
        "LAYER.VISIBLE" => layers.visible = Some(parse_yes_no(value)?),
        "LAYER.LOCKED" => layers.locked = Some(parse_yes_no(value)?),
        "STYLE.COLOUR" | "STYLE.COLOR" => styles.colour = Some(parse_colour_or_by_layer(value)?),
        "STYLE.LTYPE" | "STYLE.LINETYPE" => styles.linetype = Some(String::from(value)),
        "STYLE.WEIGHT" => styles.line_weight = Some(parse_number(value)?),
        "STYLE.HATCH" => {
            styles.hatch_pattern = Some(if value == "-" {
                String::new()
            } else {
                String::from(value)
            });
        }
        "STYLE.SYMBOL" => {
            styles.symbol = Some(if value == "-" {
                String::new()
            } else {
                String::from(value)
            });
        }
        "STYLE.SYMBOLSIZE" => styles.symbol_size = Some(parse_number(value)?),
        // A factor, checked above 0 with the rest of the request
        // (plan_global_modify): 2 doubles every symbol size, 0.5 halves it.
        "STYLE.SYMBOLSCALE" => styles.symbol_scale = Some(parse_number(value)?),
        _ => {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "not a SET key; HELP lists them",
                word,
            ));
        }
    }
    Ok(())
}

// ---- the interpreter ------------------------------------------------------------------

impl CommandInterpreter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs one line against `document`. On success returns a short
    /// human-readable message; empty lines succeed with an empty message.
    pub fn run(&mut self, document: &mut Document, line: &str) -> Result<String> {
        let tokens = tokenize(line)?;
        if tokens.is_empty() {
            return Ok(String::new());
        }
        self.history.push(String::from(line));

        // 'ZOOM and 'Z, AutoCAD's transparent form, typed with no tool
        // running: the verb itself, as AutoCAD takes it at its prompt (a
        // running tool is handed it first). Only the view verbs have one; any
        // other word after an apostrophe stays unknown.
        let verb = canonical_verb(&tokens[0]);
        if verb.len() > 1 && verb.starts_with('\'') {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "unknown command; type HELP",
                tokens[0].as_str(),
            ));
        }
        let args = &tokens[1..];

        if verb == "HELP" {
            return Ok(Self::help_for(args));
        }
        if Self::is_annotation_verb(&verb, args) {
            return self.annotation(document, &verb, args);
        }
        if Self::is_drawing_verb(&verb, args) {
            return self.drawing_verb(document, &verb, args);
        }
        if is_sheet_verb(&verb) {
            return run_sheet_verb(document, &tokens, self.sheet_context.as_deref());
        }
        // SURFACE EDIT, MERGE, PASTE, CLIP and OFFSET (docs/surface_editing.md);
        // the other SURFACE words are other modules'.
        if is_surface_edit_verb(&tokens) {
            return run_surface_edit_verb(document, &tokens, self.views());
        }
        // The drawing's surfaces and their styles (docs/terrain.md):
        // SURFACESTYLE, and the SURFACE words that are the drawing's - the
        // geoprocessing executor takes LIST, INFO, REMOVE, FROM and EXPORT
        // before a line gets here.
        if is_surface_verb(&tokens) {
            return run_surface_verb(document, &tokens);
        }
        if is_linework_verb(&verb) {
            return run_linework_verb(document, &tokens, self.views());
        }
        if is_utility_verb(&verb) {
            return run_utility_verb(document, &tokens, self.views());
        }
        // volumes: VOLUME ..., SURFACE DIFFERENCE and SURFACE ENVELOPE.
        if is_volume_verb(&verb, args) {
            return run_volume_verb(document, &tokens, self.views());
        }
        if is_road_object_verb(&verb) {
            return run_road_object_verb(document, &tokens, self.views());
        }
        // ---- road model (docs/road_design.md) ----
        if is_road_verb(&verb) {
            return run_road_verb(document, &tokens);
        }
        // Surface display (docs/surface_display.md): SURFACE LEGEND | PLAN.
        if is_surface_display_verb(&verb, args) {
            return run_surface_display_verb(document, args, self.views());
        }
        // Both families are handed the words AFTER the verb, which was read
        // as every verb is (canonical_verb): handed the line's own first word
        // they tested it again, and refused an alias the interpreter had
        // accepted.
        if is_survey_code_verb(&verb) {
            return run_survey_code_verb(document, args, self.views());
        }
        if is_customisation_verb(&verb) {
            return run_customisation_verb(document, args, &mut self.customisation);
        }
        match verb.as_str() {
            // PLINE is the drawing system's (is_drawing_verb, above), so each
            // replies with the id of what it made.
            "POINT" | "LINE" | "RECT" | "CIRCLE" | "ARC" | "TEXT" | "DIM" => {
                return self.draw(document, &verb, args);
            }
            "MOVE" | "COPY" | "ROTATE" | "SCALE" | "MIRROR" | "ARRAY" | "ERASE" => {
                return self.transform(document, &verb, args);
            }
            "OFFSET" | "TRIM" | "EXTEND" | "FILLET" | "CHAMFER" => {
                return self.edit(document, &verb, args);
            }
            "CHLAYER" | "COLOR" | "PROP" => return self.attributes(document, &verb, args),
            "MODIFY" => return self.modify(document, args),
            "SELECT" => return self.select(document, args),
            "LAYER" => return layer(document, args),
            "LINETYPE" => return linetype(document, args),
            "DIMSTYLE" => return dimension_style(document, args),
            "HATCH" => return hatch_pattern(document, args),
            "STYLE" => return style(document, args),
            "PURGE" => return purge_tables(document, args),
            "ALIGN" => return self.alignment(document, args),
            "CRS" => return coordinate_system(document, args, line),
            "PARCEL" => return parcel(document, args),
            "INVERSE" | "FORWARD" | "AREA" => return survey(document, &verb, args),
            "UNDO" | "REDO" => return undo_redo(document, &verb, args),
            "NEW" | "OPEN" | "SAVE" => return self.file(document, &verb, args),
            "LIST" | "INFO" | "STATUS" => return inspect(document, &verb, args),
            _ => {}
        }
        // SCALE also edits the document's saved view assignments without a
        // window.
        if is_view_verb(&verb) {
            return run_view_verb(
                document,
                &verb,
                args,
                self.view_host.as_deref(),
                self.views(),
            );
        }
        Err(make_error(
            ErrorCode::ParseFailure,
            "unknown command; type HELP",
            verb,
        ))
    }

    // HELP and its sub-helps: a family's when the first word after HELP is
    // one of its verbs, else the whole list.
    fn help_for(args: &[String]) -> String {
        let first = args.first().map(String::as_str);
        if first.is_some_and(is_linework_verb) {
            return linework_verb_help();
        }
        if first.is_some_and(is_sheet_verb) {
            return sheet_verb_help();
        }
        // HELP SURFACE EDIT (MERGE, PASTE, CLIP, OFFSET): the surface edits'.
        if is_surface_edit_verb(args) {
            return surface_edit_verb_help();
        }
        // The drawing's surfaces and their styles (docs/terrain.md).
        if first.is_some_and(|word| equals_ignoring_case(word.as_bytes(), b"SURFACESTYLE")) {
            return surface_verb_help();
        }
        if first.is_some_and(is_utility_verb) {
            return utility_verb_help();
        }
        // volumes: HELP VOLUME.
        if first.is_some_and(|word| equals_ignoring_case(word.as_bytes(), b"VOLUME")) {
            return volume_verb_help();
        }
        if first.is_some_and(is_road_object_verb) {
            return road_object_verb_help();
        }
        // ---- road model (docs/road_design.md) ----
        if first.is_some_and(is_road_verb) {
            return road_verb_help();
        }
        // Asked by either spelling, as the verb is run by either.
        if first.is_some_and(|word| is_customisation_verb(&canonical_verb(word))) {
            return customisation_verb_help();
        }
        Self::help_text()
    }

    /// Every non-empty line passed to run(), oldest first.
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Every command, as HELP prints it.
    pub fn help_text() -> String {
        let mut text = String::from(HELP_TEXT);
        text += &Self::annotation_help_text();
        text += &Self::drawing_help_text();
        text += &surface_edit_help_summary();
        text += &surface_verb_summary();
        text += &surface_display_verb_help();
        text
    }

    /// A line split into words as run() splits it: on whitespace, double
    /// quotes grouping words and removed ("" is an empty word). ParseFailure
    /// for a quote never closed. For a front end's own verbs (PLOTSHEETS), so
    /// they read a line exactly as the interpreter would.
    pub fn tokenize(line: &str) -> Result<Vec<String>> {
        tokenize(line)
    }

    /// What the sheet verbs (plotting/sheet_verbs.rs) cannot see in the
    /// Document - the surfaces a cross section samples, the extent the plan
    /// view draws - from the front end that has them. Headless there is none,
    /// and the verbs do without.
    pub fn set_sheet_context(&mut self, provider: Option<Box<SheetVerbContextFn<'static>>>) {
        self.sheet_context = provider;
    }

    /// What CUSTOMISE RESET, KEEP and REVERT need that the Document does not
    /// hold (customisation_verbs.rs): the program's built-in customisation
    /// and the path of the kept file. A front end hands it over where its
    /// session starts, BEFORE it calls start_customisation - the kept file is
    /// noted as it is at this moment, and KEEP refuses to write over one that
    /// is another later. A session given none - a test's bare Document, a
    /// tool - has the rest of CUSTOMISE, and those three refused by name.
    ///
    /// The colour names CODE needs are the Document's to resolve - its
    /// customisation's own table, then the standard names (colour_lookup.rs)
    /// - so no front end hands the interpreter a lookup.
    pub fn set_customisation_host(&mut self, host: CustomisationHost) {
        self.customisation = customisation_verb_context(host);
    }

    /// The rest of a front end's IMPORT line, after the verb: the path, one
    /// layer of surrounding quotes removed so that a path may hold blanks,
    /// and where the data is to land (import_placement.rs), from the line's
    /// last word when that is LOCAL, ALONGSIDE or OFFSET=dE,dN - a word of
    /// its own, in any case, never inside the quotes, so a quoted "site
    /// LOCAL" is a file, and never the only word, so a file called LOCAL can
    /// be imported. The window and the session both read the line with it:
    /// the session once took LOCAL off and left the quotes on, so every
    /// quoted IMPORT ... LOCAL - what katana_import sends - looked for a file
    /// named with its quotes. InvalidArgument for an OFFSET= that is not two
    /// numbers.
    pub fn import_argument(rest: &str) -> Result<ImportArgument> {
        let mut argument = ImportArgument::default();
        let mut text = trimmed_text(rest);
        // The last word, after the last blank - unless it holds a quote, when
        // it is the end of a quoted path ("yard OFFSET=1,2") and names no
        // placement.
        let bytes = text.as_bytes();
        let mut blank = bytes.len();
        while blank > 0 && !is_ascii_space(bytes[blank - 1]) {
            blank -= 1;
        }
        if blank > 0
            && !text[blank..].contains('"')
            && let Some(placement) = parse_placement_word(&text[blank..])?
        {
            argument.placement = placement;
            text = trimmed_text(&text[..blank]);
        }
        if text.len() >= 2
            && let Some(inner) = text
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
        {
            text = inner;
        }
        argument.path = String::from(text);
        Ok(argument)
    }

    /// Whether `word` names an entity as INFO takes one: a positive whole
    /// number, or one written #n as an anchored point names its entity. For
    /// a front end that reads INFO <file> first, so that INFO 12 still
    /// reaches the entity when no file of that name exists.
    pub fn is_entity_id(word: &str) -> bool {
        parse_id(word.strip_prefix('#').unwrap_or(word)).is_ok()
    }

    /// Whether `line` replaces the drawing: NEW, or an OPEN of a project
    /// directory - not OPEN with no target, SELECTION or #ids, which opens
    /// polylines (docs/drawing.md). The window asks it before its discard
    /// question and its reset of the backdrop and views, the session before
    /// its missing-customisation check, katana_mcp before its
    /// unsaved-changes guard, so an OPEN #12 neither asks to discard the
    /// drawing nor is refused for unsaved changes.
    pub fn replaces_document(line: &str) -> bool {
        let Ok(tokens) = tokenize(line) else {
            return false;
        };
        let Some(first) = tokens.first() else {
            return false;
        };
        let verb = canonical_verb(first);
        verb == "NEW" || (verb == "OPEN" && !Self::is_drawing_verb(&verb, &tokens[1..]))
    }

    /// The verb `line` runs, as run() reads it: its first word in capitals,
    /// the apostrophe of 'ZOOM taken off (only a view verb takes one; any
    /// other word keeps it, and run() refuses it), and an alias resolved - Z
    /// is ZOOM. "" for a line with no word or one that does not split. The
    /// window asks it whether a line typed inside a tool is a ZOOM, so which
    /// words are ZOOM is this table's alone.
    pub fn verb_of(line: &str) -> String {
        match tokenize(line) {
            Ok(tokens) if !tokens.is_empty() => canonical_verb(&tokens[0]),
            _ => String::new(),
        }
    }

    /// What the scope word VIEW means (scope_verbs.rs): the window's active
    /// plan view, or the view with the id given, with its own hidden layers
    /// and its visible area. Headless there is none, and VIEW is refused in
    /// favour of AREA.
    pub fn set_scope_context(&mut self, provider: Option<Box<ScopeViewFn<'static>>>) {
        self.scope_views = provider;
    }

    /// What VIEW is answered by, for a front end's own verbs that take the
    /// shared scope (the geoprocessing verbs above katana_cad): None
    /// headless, so VIEW is refused there exactly as MODIFY refuses it.
    pub fn scope_context(&self) -> ScopeViewProvider<'_> {
        self.views()
    }

    /// What VIEWS and ZOOM act on (view_verbs.rs): the window's views. The
    /// window supplies its workspace; headless there is none, and both verbs
    /// are refused by name.
    pub fn set_view_host(&mut self, host: Option<Rc<dyn ViewVerbHost>>) {
        self.view_host = host;
    }

    /// Forgets the "last point" that relative (@dx,dy) and polar (@d<a)
    /// points resolve against. The interpreter cannot see a document being
    /// replaced behind it, so a front end that swaps documents must say so -
    /// otherwise "@10,10" after File > New silently measures from the
    /// discarded drawing.
    pub fn reset_point_state(&mut self) {
        self.last_point = None;
    }

    /// A property value as PROP SET reads one: of the stated `type` (text,
    /// integer, real, boolean) when there is one, else guessed from the text -
    /// "true" and "false" are booleans, a whole number an integer, another
    /// number a real, anything else text. LEADER PROP and the window's
    /// Leaders manager read a value the same way.
    pub fn property_value(text: &str, r#type: Option<&str>) -> Result<PropertyValue> {
        match r#type {
            Some(r#type) => typed_property_value(r#type, text),
            None => Ok(parse_property_value(text)),
        }
    }

    // ---- shared by the verbs ------------------------------------------------------------

    /// The scope's view provider, lent for one verb.
    pub(crate) fn views(&self) -> ScopeViewProvider<'_> {
        self.scope_views.as_deref()
    }

    /// A point: x,y absolute, @dx,dy relative to the last point, @d<a polar;
    /// it becomes the last point.
    pub(crate) fn parse_point(&mut self, text: &str) -> Result<Point2> {
        let (relative, body) = match text.strip_prefix('@') {
            Some(body) => (true, body),
            None => (false, text),
        };
        let last = match (relative, self.last_point) {
            (true, None) => {
                return Err(make_error(
                    ErrorCode::InvalidState,
                    "a relative point needs a previous point in this session",
                    text,
                ));
            }
            (_, last) => last.unwrap_or_default(),
        };
        let value = if let Some(polar) = body.find('<') {
            if !relative {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "polar points must be relative: @dist<angle",
                    text,
                ));
            }
            let distance = parse_number(&body[..polar]);
            let degrees = parse_number(&body[polar + 1..]);
            match (distance, degrees) {
                (Ok(distance), Ok(degrees)) => {
                    Vec2::new(distance, 0.0).rotated(degrees * DEG_TO_RAD)
                }
                _ => {
                    return Err(make_error(
                        ErrorCode::ParseFailure,
                        "expected @distance<angle",
                        text,
                    ));
                }
            }
        } else {
            let Some(comma) = body.find(',') else {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "expected a point as x,y",
                    text,
                ));
            };
            match (
                parse_number(&body[..comma]),
                parse_number(&body[comma + 1..]),
            ) {
                (Ok(x), Ok(y)) => Vec2::new(x, y),
                _ => {
                    return Err(make_error(
                        ErrorCode::ParseFailure,
                        "expected a point as x,y",
                        text,
                    ));
                }
            }
        };
        let point = if relative { last + value } else { value };
        self.last_point = Some(point);
        Ok(point)
    }

    fn parse_points(&mut self, args: &[String], count: usize) -> Result<Vec<Point2>> {
        let mut points = Vec::new();
        for word in &args[..count] {
            points.push(self.parse_point(word)?);
        }
        Ok(points)
    }

    // ---- draw -------------------------------------------------------------------------

    fn draw(&mut self, document: &mut Document, verb: &str, args: &[String]) -> Result<String> {
        let attributes = document.current_attributes();
        // Leading arguments that are points. (PLINE is the drawing system's:
        // drawing/drawing_verbs.rs.)
        match verb {
            "POINT" => {
                if args.len() != 1 {
                    return Err(usage("POINT x,y"));
                }
                let points = self.parse_points(args, 1)?;
                finish(
                    document.execute(create_point(points[0], attributes)),
                    String::from("point created"),
                )
            }
            "LINE" => {
                if args.len() < 2 {
                    return Err(usage("LINE p p [p...]"));
                }
                let points = self.parse_points(args, args.len())?;
                if points.len() == 2 {
                    return finish(
                        document.execute(create_line(points[0], points[1], attributes)),
                        String::from("line created"),
                    );
                }
                let mut chain = Transaction::new("CREATE_LINES");
                for pair in points.windows(2) {
                    chain.add(create_line(pair[0], pair[1], attributes.clone()));
                }
                let count = chain.size();
                finish(
                    document.execute(Box::new(chain)),
                    format!("{count} lines created"),
                )
            }
            "RECT" => {
                if args.len() != 2 {
                    return Err(usage("RECT corner corner"));
                }
                let points = self.parse_points(args, 2)?;
                let rectangle = Rectangle2::from_corners(points[0], points[1]);
                finish(
                    document.execute(create_polyline(rectangle.to_polyline(), attributes)),
                    String::from("rectangle created"),
                )
            }
            "CIRCLE" => {
                if args.len() != 2 {
                    return Err(usage("CIRCLE centre radius"));
                }
                let points = self.parse_points(args, 1);
                let radius = parse_number(&args[1]);
                let points = points?;
                let radius = radius?;
                finish(
                    document.execute(create_circle(points[0], radius, attributes)),
                    String::from("circle created"),
                )
            }
            "ARC" => {
                if args.len() != 3 {
                    return Err(usage("ARC start point-on-arc end"));
                }
                let points = self.parse_points(args, 3)?;
                let Some(arc) = Arc2::through_points(points[0], points[1], points[2]) else {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        "the three points are collinear or coincident",
                    ));
                };
                finish(
                    document.execute(create_arc(arc, attributes)),
                    String::from("arc created"),
                )
            }
            "TEXT" => {
                if args.len() < 3 {
                    return Err(usage("TEXT x,y height \"text\""));
                }
                let points = self.parse_points(args, 1);
                let height = parse_number(&args[1]);
                let points = points?;
                let height = height?;
                let text = args[2..].join(" ");
                finish(
                    document.execute(create_text(
                        TextGeometry::new(points[0], text, height, 0.0),
                        attributes,
                    )),
                    String::from("text created"),
                )
            }
            _ => {
                // DIM
                if args.len() != 3 {
                    return Err(usage("DIM p p offset"));
                }
                let points = self.parse_points(args, 2);
                let offset = parse_number(&args[2]);
                let points = points?;
                let offset = offset?;
                finish(
                    document.execute(create_dimension(
                        DimensionGeometry::new(points[0], points[1], offset, ""),
                        attributes,
                    )),
                    String::from("dimension created"),
                )
            }
        }
    }

    // ---- transform --------------------------------------------------------------------

    fn transform(
        &mut self,
        document: &mut Document,
        verb: &str,
        args: &[String],
    ) -> Result<String> {
        require_selection(document)?;
        let ids = document.selection().ids();
        let count = format!("{}", ids.len());

        // A displacement is written like a point but is never relative to
        // anything.
        let parse_vector = |text: &str| -> Result<Vec2> {
            let refused = || {
                make_error(
                    ErrorCode::ParseFailure,
                    "expected a displacement as dx,dy",
                    text,
                )
            };
            let comma = text.find(',').ok_or_else(refused)?;
            match (
                parse_number(&text[..comma]),
                parse_number(&text[comma + 1..]),
            ) {
                (Ok(x), Ok(y)) => Ok(Vec2::new(x, y)),
                _ => Err(refused()),
            }
        };

        match verb {
            "ERASE" => finish(
                document.execute(delete_entities(ids)),
                format!("{count} erased"),
            ),
            "MOVE" | "COPY" => {
                if args.len() != 1 {
                    return Err(usage(&format!("{verb} dx,dy")));
                }
                let delta = parse_vector(&args[0])?;
                if verb == "MOVE" {
                    finish(
                        document.execute(move_entities(ids, delta)),
                        format!("{count} moved"),
                    )
                } else {
                    finish(
                        document.execute(copy_entities(ids, delta)),
                        format!("{count} copied"),
                    )
                }
            }
            "ROTATE" | "SCALE" => {
                if args.len() != 2 {
                    return Err(usage(if verb == "ROTATE" {
                        "ROTATE centre angle"
                    } else {
                        "SCALE centre factor"
                    }));
                }
                let center = self.parse_point(&args[0]);
                let value = parse_number(&args[1]);
                let center = center?;
                let value = value?;
                if verb == "ROTATE" {
                    finish(
                        document.execute(rotate_entities(ids, center, value * DEG_TO_RAD)),
                        format!("{count} rotated"),
                    )
                } else {
                    finish(
                        document.execute(scale_entities(ids, center, value)),
                        format!("{count} scaled"),
                    )
                }
            }
            "MIRROR" => {
                if args.len() < 2
                    || args.len() > 3
                    || (args.len() == 3 && uppered(&args[2]) != "KEEP")
                {
                    return Err(usage("MIRROR p p [KEEP]"));
                }
                let a = self.parse_point(&args[0])?;
                let b = self.parse_point(&args[1])?;
                finish(
                    document.execute(mirror_entities(ids, a, b, args.len() == 3)),
                    format!("{count} mirrored"),
                )
            }
            _ => {
                // ARRAY
                if args.len() != 3 {
                    return Err(usage("ARRAY rows columns dx,dy"));
                }
                let rows = parse_count(&args[0]);
                let columns = parse_count(&args[1]);
                let spacing = parse_vector(&args[2]);
                let rows = rows?;
                let columns = columns?;
                let spacing = spacing?;
                finish(
                    document.execute(array_entities(ids, rows, columns, spacing)),
                    String::from("array created"),
                )
            }
        }
    }

    // ---- edit -------------------------------------------------------------------------

    fn edit(&mut self, document: &mut Document, verb: &str, args: &[String]) -> Result<String> {
        let parse_ids = |from: usize| -> Result<Vec<EntityId>> {
            args[from..]
                .iter()
                .map(|word| parse_id(word.as_str()))
                .collect()
        };

        match verb {
            "OFFSET" => {
                if args.len() != 3 {
                    return Err(usage("OFFSET id distance side-point"));
                }
                let id = parse_id(&args[0]);
                let distance = parse_number(&args[1]);
                let id = id?;
                let distance = distance?;
                let side = self.parse_point(&args[2])?;
                finish(
                    document.execute(offset_entity(id, distance, side)),
                    String::from("offset created"),
                )
            }
            "TRIM" | "EXTEND" => {
                if args.len() < 3 {
                    return Err(usage(&format!("{verb} id pick-point other-id...")));
                }
                let id = parse_id(&args[0])?;
                let pick = self.parse_point(&args[1])?;
                let others = parse_ids(2)?;
                if verb == "TRIM" {
                    finish(
                        document.execute(trim_entity(id, others, pick)),
                        String::from("trimmed"),
                    )
                } else {
                    finish(
                        document.execute(extend_entity(id, others, pick)),
                        String::from("extended"),
                    )
                }
            }
            "FILLET" => {
                if args.len() != 3 {
                    return Err(usage("FILLET id id radius"));
                }
                let first = parse_id(&args[0]);
                let second = parse_id(&args[1]);
                let radius = parse_number(&args[2]);
                let first = first?;
                let second = second?;
                let radius = radius?;
                finish(
                    document.execute(fillet_entities(first, second, radius)),
                    String::from("filleted"),
                )
            }
            _ => {
                // CHAMFER
                if args.len() < 3 || args.len() > 4 {
                    return Err(usage("CHAMFER id id distance [distance2]"));
                }
                let first = parse_id(&args[0]);
                let second = parse_id(&args[1]);
                let distance_first = parse_number(&args[2]);
                let distance_second = if args.len() == 4 {
                    parse_number(&args[3])
                } else {
                    distance_first.clone()
                };
                let first = first?;
                let second = second?;
                let distance_first = distance_first?;
                let distance_second = distance_second?;
                finish(
                    document.execute(chamfer_entities(
                        first,
                        second,
                        distance_first,
                        distance_second,
                    )),
                    String::from("chamfered"),
                )
            }
        }
    }

    // ---- select -----------------------------------------------------------------------

    fn select(&mut self, document: &mut Document, args: &[String]) -> Result<String> {
        if args.is_empty() {
            return Err(usage(
                "SELECT ALL | NONE | id... | LAYER name | TYPE name | \
                 SELECTION|DRAWING|VIEW [id] [EXTENTS]|AREA x0,y0,x1,y1|LAYERS a,b [ONLY] \
                 [WHERE key=value ...] | WHERE key=value ...",
            ));
        }
        let mode = uppered(&args[0]);
        let mut ids: Vec<EntityId> = Vec::new();

        // The shared scope and filter (docs/cad.md, "Scope and filter"), so a
        // type, a style or a property is selected across the drawing, a view
        // or layers as Global Modify's Select Matches selects it: SELECT
        // DRAWING WHERE TYPE=leader. ALL alone and LAYER name keep the meaning
        // they had before, although ALL and LAYER are scope words too
        // (DRAWING, LAYERS): LAYER name is that one layer, where LAYERS takes
        // the layers beneath it, and a script's SELECT LAYER must not come to
        // take more.
        let own_form = (mode == "ALL" && args.len() == 1) || (mode == "LAYER" && args.len() == 2);
        if is_scope_word(&args[0]) && !own_form {
            let mut at = 0;
            let words = parse_scope_words(args, &mut at)?;
            if at < args.len() {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "SELECT takes nothing after its scope and WHERE conditions",
                    args[at].as_str(),
                ));
            }
            let matched = match_scope(document, &words, self.views())?;
            let model = document.model();
            // Selected by the rule every other SELECT keeps: what is hidden,
            // or on a hidden or locked layer, is matched and counted but not
            // selected, so the next verb never acts on what cannot be seen or
            // is locked.
            for &id in &matched.matched {
                if let Some(entity) = model.entities.find(id)
                    && is_selectable(model, entity, &NO_LAYER_OVERRIDES)
                {
                    ids.push(id);
                }
            }
            let mut reply = format!("{} selected={}", scope_record(&matched), ids.len());
            let left = matched.matched.len() - ids.len();
            if left > 0 {
                reply += &format!(" unselectable={left}");
            }
            // What was matched, by kind: what the window's type boxes count.
            let tally = type_tally(model, &matched.matched);
            if !tally.is_empty() {
                reply += &format!(" types={tally}");
            }
            document.selection_mut().set(ids);
            document.notify_selection_changed();
            return Ok(reply);
        }

        {
            let model = document.model();
            let mut collect = |filter: &SelectionFilter<'_>| {
                model.entities.for_each(|entity| {
                    // The document rule: the interpreter knows no view, and
                    // SELECT ALL meaning "what the last-clicked view shows"
                    // would make a typed command depend on a click the user
                    // may not remember.
                    if is_selectable(model, entity, &NO_LAYER_OVERRIDES) && filter.accepts(entity) {
                        ids.push(entity.id);
                    }
                });
            };

            if mode == "NONE" {
                // nothing to collect
            } else if mode == "ALL" {
                collect(&SelectionFilter::default());
            } else if mode == "LAYER" && args.len() == 2 {
                if !model.layers.contains(&args[1]) {
                    return Err(make_error(
                        ErrorCode::NotFound,
                        "layer does not exist",
                        args[1].as_str(),
                    ));
                }
                collect(&SelectionFilter {
                    layer: Some(args[1].clone()),
                    ..SelectionFilter::default()
                });
            } else if mode == "TYPE" && args.len() == 2 {
                let entity_type = entity_type_from_string(&args[1])?;
                collect(&SelectionFilter {
                    types: BTreeSet::from([entity_type]),
                    ..SelectionFilter::default()
                });
            } else {
                for text in args {
                    // #12 as well as 12, as INFO takes it: the id copied from a
                    // DIM line or a refusal is taken as it is.
                    let id = parse_id(text.strip_prefix('#').unwrap_or(text))?;
                    let Some(entity) = model.entities.find(id) else {
                        return Err(make_error(
                            ErrorCode::NotFound,
                            "entity does not exist",
                            text.as_str(),
                        ));
                    };
                    if !is_selectable(model, entity, &NO_LAYER_OVERRIDES) {
                        return Err(make_error(
                            ErrorCode::CommandRejected,
                            "entity is hidden or on a hidden or locked layer",
                            text.as_str(),
                        ));
                    }
                    ids.push(id);
                }
            }
        }
        document.selection_mut().set(ids);
        document.notify_selection_changed();
        Ok(format!("{} selected", document.selection().size()))
    }

    // ---- alignments -------------------------------------------------------------------

    fn alignment(&mut self, document: &mut Document, args: &[String]) -> Result<String> {
        const ALIGN_USAGE: &str = "ALIGN LIST | NEW name x,y x,y [x,y ...] | PI name x,y [radius [spiralIn [spiralOut]]]\
             \n      | SET name index radius [spiralIn [spiralOut]] | START name station\
             \n      | PIS name [x,y[,radius[,spiralIn[,spiralOut]]] ...]\
             \n      | STATIONS name interval | DELETE name     (PI indices count from 0)\
             \n      | DESIGN name station,elevation[,curveLength] ... (at least two PVIs)\
             \n      | PVI name station elevation [curveLength] | PROFILE name | CLEARPROFILE name";
        let action = args
            .first()
            .map_or_else(|| String::from("LIST"), |word| uppered(word));

        if action == "LIST" {
            let mut out = String::new();
            for alignment in document.model().alignments.all() {
                out += &format!(
                    "  {}  {} PIs",
                    alignment.name,
                    alignment.horizontal.pis.len()
                );
                // Stored alignments always solve - add() refused any that
                // could not - so a failure here is a defect worth showing,
                // not hiding.
                match solve_alignment(&alignment.horizontal) {
                    Ok(solved) => {
                        out += &format!(
                            "  {} to {}  length {}",
                            c_fixed(solved.start_station(), 3),
                            c_fixed(solved.end_station(), 3),
                            c_fixed(solved.length(), 3)
                        );
                    }
                    Err(error) => {
                        out += &format!("  (does not solve: {})", error.describe());
                    }
                }
                if !alignment.description.is_empty() {
                    out += &format!("  {}", alignment.description);
                }
                out.push('\n');
            }
            return Ok(without_last_newline(out));
        }
        if args.len() < 2 {
            return Err(usage(ALIGN_USAGE));
        }
        let name = &args[1];

        if action == "NEW" {
            if args.len() < 4 {
                return Err(usage("ALIGN NEW name x,y x,y [x,y ...]   at least two PIs"));
            }
            let mut created = Alignment {
                name: name.clone(),
                ..Alignment::default()
            };
            for word in &args[2..] {
                let point = self.parse_point(word)?;
                created.horizontal.pis.push(AlignmentPi {
                    point,
                    ..AlignmentPi::default()
                });
            }
            return finish(
                document.execute(create_alignment(created)),
                format!("alignment {name} created with {} PIs", args.len() - 2),
            );
        }
        if action == "DELETE" {
            // The leaders whose notes read it by name will read nothing: said,
            // not refused (docs/annotation.md, "A surface or an alignment is
            // read by name"); UNDO puts the alignment back with its readers.
            let readers = leaders_reading(document.model(), ReadKind::Alignment, name);
            let warning = if readers > 0 {
                format!(
                    "\nwarning leaders={readers} read alignment {name} and will read nothing now"
                )
            } else {
                String::new()
            };
            return finish(
                document.execute(delete_alignment(name.as_str())),
                format!("alignment {name} deleted{warning}"),
            );
        }

        let Some(existing) = document.model().alignments.find(name).cloned() else {
            return Err(make_error(
                ErrorCode::NotFound,
                "alignment does not exist",
                name.as_str(),
            ));
        };
        let mut changed = existing.clone();

        match action.as_str() {
            "PI" => {
                if args.len() < 3 {
                    return Err(usage(
                        "ALIGN PI name x,y [radius [spiralIn [spiralOut]]]   appends a PI",
                    ));
                }
                let point = self.parse_point(&args[2])?;
                let mut pi = AlignmentPi {
                    point,
                    ..AlignmentPi::default()
                };
                set_curve_fields(&mut pi, args)?;
                changed.horizontal.pis.push(pi);
                // Counted before the step: the C++ built this text as an
                // argument beside the execute, in an order C++ leaves open.
                let count = existing.horizontal.pis.len() + 1;
                finish(
                    document.execute(update_alignment(changed)),
                    format!("alignment {name} now has {count} PIs"),
                )
            }
            "SET" => {
                if args.len() < 4 {
                    return Err(usage(
                        "ALIGN SET name index radius [spiralIn [spiralOut]]   index from 0",
                    ));
                }
                let index = parse_number(&args[2])?;
                if index < 0.0 || index >= changed.horizontal.pis.len() as f64 {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "no such PI",
                        format!("index={}", args[2]),
                    ));
                }
                set_curve_fields(&mut changed.horizontal.pis[index as usize], args)?;
                finish(
                    document.execute(update_alignment(changed)),
                    format!("alignment {name} PI {} updated", args[2]),
                )
            }
            "PIS" => {
                // Alone, what an agent needs to edit an alignment it did not
                // draw: every PI, exactly, a record each.
                if args.len() == 2 {
                    let lines: Vec<String> = existing
                        .horizontal
                        .pis
                        .iter()
                        .enumerate()
                        .map(|(i, pi)| {
                            format!(
                                "pi index={i} x={} y={} radius={} spiral_in={} spiral_out={}",
                                format_exact_real(pi.point.x),
                                format_exact_real(pi.point.y),
                                format_exact_real(pi.radius),
                                format_exact_real(pi.spiral_in),
                                format_exact_real(pi.spiral_out)
                            )
                        })
                        .collect();
                    return Ok(lines.join("\n"));
                }
                // With PIs, the whole horizontal definition at once, as DESIGN
                // is the whole profile: an edit of several PIs - the Alignment
                // Manager's grid - is one undo step, and the model judges the
                // result whole.
                if args.len() < 4 {
                    return Err(usage(
                        "ALIGN PIS name x,y[,radius[,spiralIn[,spiralOut]]] ...   at least two; \
                         ALIGN PIS name alone prints them",
                    ));
                }
                let mut pis = Vec::new();
                for word in &args[2..] {
                    pis.push(parse_alignment_pi(word)?);
                }
                changed.horizontal.pis = pis;
                let count = changed.horizontal.pis.len();
                // Setting what is already there is not an edit: no undo step.
                // The Alignment Manager's Apply PIs writes the stored PIs back
                // exactly when its grid was not edited, and each press was
                // once a step and a drawing to save.
                if changed == existing {
                    return Ok(format!("alignment {name} unchanged"));
                }
                finish(
                    document.execute(update_alignment(changed)),
                    format!("alignment {name} now has {count} PIs"),
                )
            }
            "START" => {
                if args.len() < 3 {
                    return Err(usage("ALIGN START name station"));
                }
                changed.horizontal.start_station = parse_number(&args[2])?;
                finish(
                    document.execute(update_alignment(changed)),
                    format!("alignment {name} starts at {}", args[2]),
                )
            }
            "STATIONS" => {
                if args.len() < 3 {
                    return Err(usage("ALIGN STATIONS name interval"));
                }
                let interval = parse_number(&args[2])?;
                let solved = solve_alignment(&existing.horizontal)?;
                // The table Terrain > Alignment Manager shows, from the same
                // function.
                let stations = setting_out_stations(&solved, interval)?;
                Ok(format_setting_out(&stations))
            }
            "DESIGN" => {
                // The whole profile at once, because a profile with one PVI
                // cannot be built and the model refuses to hold one - so it
                // cannot be grown from nothing one PVI at a time. PVI appends
                // to a profile that exists.
                if args.len() < 4 {
                    return Err(usage(
                        "ALIGN DESIGN name station,elevation[,curveLength] ...   at least two; \
                         the first and last carry no curve",
                    ));
                }
                let mut profile = VerticalAlignment::default();
                for word in &args[2..] {
                    profile.pvis.push(parse_pvi(word)?);
                }
                changed.vertical = Some(profile);
                // As PIS: the manager's Apply Profile on an unedited grid.
                if changed == existing {
                    return Ok(format!("alignment {name} unchanged"));
                }
                finish(
                    document.execute(update_alignment(changed)),
                    format!("alignment {name} designed with {} PVIs", args.len() - 2),
                )
            }
            "PVI" => {
                if args.len() < 4 {
                    return Err(usage(
                        "ALIGN PVI name station elevation [curveLength]   appends a PVI to an \
                         existing design profile; use DESIGN to start one",
                    ));
                }
                if existing.vertical.is_none() {
                    return Err(make_error(
                        ErrorCode::InvalidState,
                        "the alignment has no design profile to append to; define one with \
                         ALIGN DESIGN",
                        name.as_str(),
                    ));
                }
                let station = parse_number(&args[2])?;
                let elevation = parse_number(&args[3])?;
                let curve_length = match args.get(4) {
                    Some(word) => parse_number(word)?,
                    None => 0.0,
                };
                let profile = changed.vertical.get_or_insert_default();
                profile.pvis.push(ProfilePvi {
                    station,
                    elevation,
                    curve_length,
                });
                // Counted before the step moves `changed`: the C++ built this
                // text beside the execute, in an order C++ leaves open.
                let count = profile.pvis.len();
                finish(
                    document.execute(update_alignment(changed)),
                    format!("alignment {name} profile now has {count} PVIs"),
                )
            }
            "PROFILE" => {
                let Some(vertical) = &existing.vertical else {
                    return Ok(format!("alignment {name} has no design profile"));
                };
                let solved = solve_profile(vertical)?;
                Ok(format_profile_report(vertical, &solved))
            }
            "CLEARPROFILE" => {
                changed.vertical = None;
                finish(
                    document.execute(update_alignment(changed)),
                    format!("alignment {name} profile removed"),
                )
            }
            _ => Err(usage(ALIGN_USAGE)),
        }
    }

    // ---- attributes -------------------------------------------------------------------

    fn attributes(
        &mut self,
        document: &mut Document,
        verb: &str,
        args: &[String],
    ) -> Result<String> {
        // PROP TREE reads, and takes a scope as every reading verb does
        // (property_outline.rs), so it is answered before the selection is
        // required: PROP TREE DRAWING needs none.
        if verb == "PROP" && args.first().is_some_and(|word| uppered(word) == "TREE") {
            return property_tree_reply(document, &args[1..], self.views());
        }
        require_selection(document)?;
        let ids = document.selection().ids();
        let count = format!("{}", ids.len());

        if verb == "CHLAYER" {
            if args.len() != 1 {
                return Err(usage("CHLAYER name"));
            }
            return finish(
                document.execute(set_entity_layer(ids, args[0].as_str())),
                format!("{count} moved to layer {}", args[0]),
            );
        }
        if verb == "COLOR" {
            if args.len() != 1 {
                return Err(usage("COLOR #RRGGBB | BYLAYER"));
            }
            let color = if uppered(&args[0]) != "BYLAYER" {
                Some(Color::from_hex(&args[0])?)
            } else {
                None
            };
            return finish(
                document.execute(set_entity_color(ids, color)),
                format!("{count} recoloured"),
            );
        }
        // PROP. Verb-first, as LAYER, STYLE, LINETYPE and HATCH are: the old
        // bare `PROP key value` could not be extended without becoming
        // ambiguous, since a property may be named DELETE.
        const PROP_USAGE: &str = "PROP LIST | SET key value [text|integer|real|boolean] | DELETE \
                                  key | RENAME old new | TREE [scope] [UNDER path] [FROM n] \
                                  [LIMIT n]";
        let action = args
            .first()
            .map_or_else(|| String::from("LIST"), |word| uppered(word));

        match action.as_str() {
            "LIST" => {
                let mut out = String::new();
                for &id in &ids {
                    let Some(entity) = document.model().entities.find(id) else {
                        continue;
                    };
                    out += &format!("  {id}:");
                    if entity.properties.is_empty() {
                        out += " (none)";
                    }
                    for (key, value) in &entity.properties {
                        out += &format!(
                            " {key}={} ({})",
                            property_to_string(value),
                            type_name(value)
                        );
                    }
                    out.push('\n');
                }
                Ok(without_last_newline(out))
            }
            "SET" => {
                if args.len() < 3 || args.len() > 4 {
                    return Err(usage(PROP_USAGE));
                }
                // The type is guessed from the value unless it is stated.
                // Survey code attributes are typed, and "2" as text is not 2
                // as an integer.
                let value = if args.len() == 4 {
                    typed_property_value(&args[3], &args[2])?
                } else {
                    parse_property_value(&args[2])
                };
                finish(
                    document.execute(set_entity_property(ids, args[1].as_str(), value)),
                    format!("property {} set on {count}", args[1]),
                )
            }
            "DELETE" => {
                if args.len() != 2 {
                    return Err(usage(PROP_USAGE));
                }
                finish(
                    document.execute(remove_entity_property(ids, args[1].as_str())),
                    format!("property {} removed from {count}", args[1]),
                )
            }
            "RENAME" => {
                if args.len() != 3 {
                    return Err(usage(PROP_USAGE));
                }
                finish(
                    document.execute(rename_entity_property(
                        ids,
                        args[1].as_str(),
                        args[2].as_str(),
                    )),
                    format!("property {} renamed to {} on {count}", args[1], args[2]),
                )
            }
            _ => Err(usage(PROP_USAGE)),
        }
    }

    // ---- global modify ----------------------------------------------------------------

    fn modify(&mut self, document: &mut Document, args: &[String]) -> Result<String> {
        // MODIFY's own words stand where they always could: PREVIEW anywhere,
        // SET before the fields, and a WHERE after them taking the filter up
        // again. The rest are the shared scope and filter words
        // (scope_verbs.rs), read by the one parser - a LAYERS list being the
        // word after LAYERS, whatever it says, so a layer may be called
        // "preview" or "set".
        let mut scope: Vec<String> = Vec::new();
        let mut request = GlobalModify::default();
        let mut preview = false;
        let mut saw_set = false;
        let mut in_where = false;
        // Where in `scope` the filter began.
        let mut first_where: Option<usize> = None;
        let mut i = 0;
        while i < args.len() {
            let word = &args[i];
            let folded = uppered(word);
            if folded == "PREVIEW" {
                preview = true;
                i += 1;
                continue;
            }
            if folded == "SET" {
                saw_set = true;
                in_where = false;
                i += 1;
                continue;
            }
            if folded == "WHERE" {
                first_where.get_or_insert(scope.len());
                in_where = true;
            } else if saw_set && !in_where {
                parse_set_word(word, &mut request)?;
                i += 1;
                continue;
            }
            scope.push(word.clone());
            if !in_where && (folded == "LAYERS" || folded == "LAYER") && i + 1 < args.len() {
                i += 1;
                scope.push(args[i].clone());
            }
            i += 1;
        }
        if !saw_set {
            return Err(usage(MODIFY_USAGE));
        }
        let mut at = 0;
        let scope_words = parse_scope_words(&scope, &mut at)?;
        if at < scope.len() {
            // In the filter, a word that is no condition is named as one.
            if first_where.is_some_and(|first| at > first) {
                let mut unused = ModifyFilter::default();
                parse_where_condition(&scope[at], &mut unused)?;
            }
            return Err(usage(MODIFY_USAGE));
        }
        if scope_words.source == ScopeSource::Selection && document.selection().empty() {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "nothing is selected; SELECT first, or MODIFY DRAWING or MODIFY LAYERS",
            ));
        }
        let resolved = resolve_scope(&scope_words, self.views())?;
        let mut plan = plan_global_modify(document, &resolved.scope, &resolved.filter, &request)?;
        // A view's scope and a typed window say which part of the drawing
        // they were - the view moves, and its id is not its title's number -
        // as the UTILITY replies do; the other scopes say it in the line
        // itself.
        let mut reply = format!(
            "{}{}",
            if preview { "preview: " } else { "" },
            plan.summary()
        );
        if scope_words.source == ScopeSource::View || scope_words.source == ScopeSource::Area {
            let found = ScopeMatch {
                resolved: resolved.clone(),
                matched: plan.matched.clone(),
            };
            reply = format!("{}\n{reply}", scope_record(&found));
        }
        if preview {
            return Ok(reply);
        }
        let Some(command) = plan.command.take() else {
            return Ok(reply);
        };
        finish(
            document.execute(command),
            format!("{reply} One UNDO restores it."),
        )
    }

    // ---- file ---------------------------------------------------------------------------

    fn file(&mut self, document: &mut Document, verb: &str, args: &[String]) -> Result<String> {
        if verb == "NEW" {
            document.new_document();
            self.reset_point_state();
            return Ok(String::from("new drawing"));
        }
        if verb == "OPEN" {
            if args.len() != 1 {
                return Err(usage("OPEN project-directory"));
            }
            self.reset_point_state();
            // Sequenced deliberately: the count is read after the open. The
            // C++ once built it as an argument beside the open, whose order
            // C++ leaves open, and every OPEN reported "(0 entities)".
            // Through path_from_utf8: the bytes of the word are UTF-8
            // (core/path_text.rs).
            let status = document.open(&path_from_utf8(&args[0]));
            let count = document.model().entities.size();
            return finish(status, format!("opened {} ({count} entities)", args[0]));
        }
        // SAVE
        if args.len() > 1 {
            return Err(usage("SAVE [project-directory]"));
        }
        if let Some(directory) = args.first() {
            return finish(
                document.save_as(&path_from_utf8(directory)),
                format!("saved to {directory}"),
            );
        }
        finish(document.save(), String::from("saved"))
    }
}

// ALIGN PI's and ALIGN SET's radius, spiral in and spiral out, from the words
// after the PI (the fourth to the sixth).
fn set_curve_fields(pi: &mut AlignmentPi, args: &[String]) -> Status {
    let fields = [&mut pi.radius, &mut pi.spiral_in, &mut pi.spiral_out];
    for (field, word) in fields.into_iter().zip(args.iter().skip(3)) {
        *field = parse_number(word)?;
    }
    Ok(())
}

fn require_selection(document: &Document) -> Status {
    if document.selection().empty() {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "nothing is selected; use SELECT first",
        ));
    }
    Ok(())
}

// ---- styles -------------------------------------------------------------------------------

const STYLE_USAGE: &str = "STYLE LIST | SYMBOLS [filter] | NEW name [field value] | SET name \
                           field value | RENAME old new | DELETE name | APPLY name | MERGE from \
                           into | USAGE [name] | CURRENT [name|-]\n  fields: linetype (a model \
                           linetype, a library linestyle or ByLayer), weight (mm),\n  colour \
                           (#RRGGBB or bylayer), hatch, symbol,\n  symbolsize (model units, 0 \
                           for the default mark), description\n  APPLY sets the style of the \
                           selection; APPLY - clears it (ByLayer)";

// A name with spaces that was not quoted: refused when there are more words
// than `count`.
fn wants(args: &[String], count: usize) -> Status {
    if args.len() <= count {
        return Ok(());
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        "too many arguments: put a name with spaces in double quotes",
        args[count].as_str(),
    ))
}

// SET's fields, applied to `changed`. Shared by SET and by NEW with a field,
// so a new style and an edited one read a field the same way.
fn set_style_field(document: &Document, args: &[String], changed: &mut Style) -> Status {
    let model = document.model();
    let field = uppered(&args[2]);
    let value = &args[3];
    // A field whose value is one word takes one: STYLE NEW a colour #F00
    // weight 0.5 made the style, said so, and dropped the weight. The names
    // (LINETYPE, SYMBOL, DESCRIPTION) take the rest of the line.
    if matches!(
        field.as_str(),
        "WEIGHT" | "COLOUR" | "COLOR" | "HATCH" | "SYMBOLSIZE"
    ) && args.len() > 4
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "too many arguments: one field and its value; set another with STYLE SET",
            args[4].as_str(),
        ));
    }
    match field.as_str() {
        "LINETYPE" => {
            // The rest of the line, as SYMBOL takes it: "WATR Main" unquoted
            // used to become "WATR" with "Main" dropped.
            let linetype_name = rest_of_line(args, 3);
            if is_by_layer(&linetype_name) {
                changed.linetype = String::from(BY_LAYER_LINETYPE);
            } else {
                check_linetype_name(document, &linetype_name)?;
                changed.linetype = linetype_name;
            }
        }
        "WEIGHT" => changed.line_weight = parse_number(value)?,
        "COLOUR" | "COLOR" => {
            changed.color = if uppered(value) == "BYLAYER" {
                None
            } else {
                Some(Color::from_hex(value)?)
            };
        }
        "HATCH" => {
            if value != "-" && !model.hatch_patterns.contains(value) {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "hatch pattern does not exist",
                    value.as_str(),
                ));
            }
            changed.hatch_pattern = if value == "-" {
                String::new()
            } else {
                value.clone()
            };
        }
        "SYMBOL" => {
            // A library symbol's name has spaces in it - "CULT Bollard" - so
            // the rest of the line is the name, as DESCRIPTION does.
            let symbol_name = rest_of_line(args, 3);
            if symbol_name == "-" {
                changed.symbol.clear();
            } else {
                // The model itself accepts any name, because a project can be
                // opened before its library is loaded (see entity::validate).
                // A person TYPING one should still be told about a typo,
                // which is the same courtesy HATCH above pays.
                if !is_built_in_symbol_name(&symbol_name)
                    && document.definition_for(&symbol_name).is_none()
                {
                    return Err(make_error(
                        ErrorCode::NotFound,
                        "no symbol of that name is built in or in the loaded library",
                        symbol_name,
                    ));
                }
                changed.symbol = symbol_name;
            }
        }
        "SYMBOLSIZE" => changed.symbol_size = parse_number(value)?,
        "DESCRIPTION" => changed.description = rest_of_line(args, 3),
        _ => return Err(usage(STYLE_USAGE)),
    }
    Ok(())
}

fn style(document: &mut Document, args: &[String]) -> Result<String> {
    let action = args
        .first()
        .map_or_else(|| String::from("LIST"), |word| uppered(word));

    if action == "LIST" {
        let mut out = String::new();
        for item in document.model().styles.all() {
            out += &format!(
                "  {}  linetype={}  weight={}",
                item.name,
                item.linetype,
                format_general(item.line_weight, 6)
            );
            if let Some(color) = item.color {
                out += &format!("  colour={}", color.to_hex());
            }
            if !item.hatch_pattern.is_empty() {
                out += &format!("  hatch={}", item.hatch_pattern);
            }
            if !item.symbol.is_empty() {
                out += &format!("  symbol={}", item.symbol);
                if item.symbol_size > 0.0 {
                    out += &format!("@{}", format_general(item.symbol_size, 6));
                }
            }
            if !item.description.is_empty() {
                out += &format!("  {}", item.description);
            }
            out.push('\n');
        }
        let text = without_last_newline(out);
        return Ok(if text.is_empty() {
            String::from("no styles")
        } else {
            text
        });
    }
    if action == "SYMBOLS" {
        // The catalogue's list, the one a symbol picker offers: the built-in
        // shapes and every library definition D3 calls a symbol - not only
        // the `mode vertex` ones, which are the minority of those in use.
        // Filtered by the rest of the line, case folded.
        let symbols: Vec<CatalogueEntry> =
            filter_choices(&symbol_choices(document), &rest_of_line(args, 1));
        let mut out = String::new();
        for entry in &symbols {
            out += &format!("  {}  ({}", entry.name, entry.source.name());
            if !entry.group.is_empty() {
                out += &format!(", {}", entry.group);
            }
            out.push(')');
            if entry.users.used() {
                out += &format!("  {}", entry.users.describe());
            }
            out.push('\n');
        }
        let text = without_last_newline(out);
        return Ok(if text.is_empty() {
            String::from("no symbol matches")
        } else {
            text
        });
    }
    if action == "USAGE" {
        let model = document.model();
        let usage_table = table_usage(model, UsageOptions::default());
        if args.len() >= 2 {
            let wanted = rest_of_line(args, 1);
            let users = TableUsage::of(&usage_table.styles, &wanted);
            if !model.styles.contains(&wanted) && !users.used() {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "style does not exist",
                    wanted,
                ));
            }
            return Ok(format!(
                "style {wanted}: {}{}",
                if users.used() {
                    users.describe()
                } else {
                    String::from("unused")
                },
                if model.styles.contains(&wanted) {
                    ""
                } else {
                    " (not in the style table)"
                }
            ));
        }
        let mut out = String::new();
        for (name, users) in &usage_table.styles {
            out += &format!(
                "  {name}  {}{}\n",
                counted_noun(users.entities, "entity", "entities"),
                if model.styles.contains(name) {
                    ""
                } else {
                    "  (not in the style table)"
                }
            );
        }
        let text = without_last_newline(out);
        return Ok(if text.is_empty() {
            String::from("no styles")
        } else {
            text
        });
    }
    if action == "CURRENT" {
        if args.len() >= 2 {
            let wanted = rest_of_line(args, 1);
            let by_layer = wanted == "-" || is_by_layer(&wanted);
            document.set_current_style(if by_layer { "" } else { &wanted })?;
        }
        return Ok(format!(
            "current style is {}",
            if document.current_style().is_empty() {
                "ByLayer"
            } else {
                document.current_style()
            }
        ));
    }

    if args.len() < 2 {
        return Err(usage(STYLE_USAGE));
    }
    let name = &args[1];
    // A library style name has spaces in it ("TOPO Natural Surface Point"),
    // and STYLE NEW TOPO Natural Surface Point used to make a style called
    // "TOPO" and drop the rest without a word. Everything but SET, whose
    // description takes the rest of the line, now says so.
    match action.as_str() {
        "NEW" => {
            let mut item = Style {
                name: name.clone(),
                ..Style::default()
            };
            // NEW name field value: a style made with one field set, as SET
            // would set it, in one step - the Hatch Patterns tab's "New Style
            // Using This". A second word that is no field is a name that
            // needed quotes.
            if args.len() > 2 {
                const FIELDS: [&str; 8] = [
                    "LINETYPE",
                    "WEIGHT",
                    "COLOUR",
                    "COLOR",
                    "HATCH",
                    "SYMBOL",
                    "SYMBOLSIZE",
                    "DESCRIPTION",
                ];
                let field = uppered(&args[2]);
                if !FIELDS.contains(&field.as_str()) {
                    wants(args, 2)?;
                }
                if args.len() < 4 {
                    return Err(usage(STYLE_USAGE));
                }
                set_style_field(document, args, &mut item)?;
            }
            finish(
                document.execute(create_style(item)),
                format!("style {name} created"),
            )
        }
        "DELETE" => {
            wants(args, 2)?;
            finish(
                document.execute(delete_style(name.as_str())),
                format!("style {name} deleted"),
            )
        }
        "RENAME" => {
            if args.len() < 3 {
                return Err(usage("STYLE RENAME old new"));
            }
            wants(args, 3)?;
            finish(
                document.execute(rename_style(name.as_str(), args[2].as_str())),
                format!(
                    "style {name} renamed to {} (every entity wearing it came too)",
                    args[2]
                ),
            )
        }
        "MERGE" => {
            if args.len() < 3 {
                return Err(usage("STYLE MERGE from into"));
            }
            wants(args, 3)?;
            let moved = TableUsage::of(
                &table_usage(document.model(), UsageOptions::default()).styles,
                name,
            )
            .entities;
            finish(
                document.execute(merge_style(name.as_str(), args[2].as_str())),
                format!(
                    "style {name} merged into {} ({} moved)",
                    args[2],
                    counted_noun(moved, "entity", "entities")
                ),
            )
        }
        "APPLY" => {
            wants(args, 2)?;
            let ids = document.selection().ids();
            if ids.is_empty() {
                return Err(Error::new(ErrorCode::InvalidState, "nothing is selected"));
            }
            let wanted = if name == "-" {
                String::new()
            } else {
                name.clone()
            };
            if !wanted.is_empty() && !document.model().styles.contains(&wanted) {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "style does not exist",
                    wanted,
                ));
            }
            let count = ids.len();
            let shown = if wanted.is_empty() {
                String::from("ByLayer")
            } else {
                wanted.clone()
            };
            finish(
                document.execute(set_entity_style(ids, wanted)),
                format!("{count} entities set to style {shown}"),
            )
        }
        "SET" => {
            if args.len() < 4 {
                return Err(usage(STYLE_USAGE));
            }
            let Some(existing) = document.model().styles.find(name) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "style does not exist",
                    name.as_str(),
                ));
            };
            let mut changed = existing.clone();
            set_style_field(document, args, &mut changed)?;
            // Setting what is already there is not an edit: no undo step.
            let Some(command) = update_style_if_changed(document.model(), changed) else {
                return Ok(format!("style {name} unchanged"));
            };
            finish(document.execute(command), format!("style {name} updated"))
        }
        _ => Err(usage(STYLE_USAGE)),
    }
}

// ---- layers -------------------------------------------------------------------------------

fn dimension_style(document: &mut Document, args: &[String]) -> Result<String> {
    let action = args
        .first()
        .map_or_else(|| String::from("LIST"), |word| uppered(word));

    if action == "LIST" {
        let mut out = String::new();
        for style in document.model().dimension_styles.all() {
            out += &format!(
                "  {}  text={}  arrow={} {}  decimals={}",
                style.name,
                format_general(style.text_height, 6),
                format_general(style.arrow_size, 6),
                style.arrow_head.name(),
                style.decimals
            );
            if style.unit_scale != 1.0 {
                out += &format!("  scale={}", format_general(style.unit_scale, 6));
            }
            if style.round_to > 0.0 {
                out += &format!("  round={}", format_general(style.round_to, 6));
            }
            if !style.suffix.is_empty() {
                out += &format!("  suffix='{}'", style.suffix);
            }
            if style.paper_sized {
                out += "  paper-sized";
            }
            // What a dimension of exactly ten units would read as. This is
            // the question anyone setting a style is actually asking, and
            // working it out from the fields is guesswork.
            out += &format!(
                "  (10 reads as {})",
                katana_entity::dimension_text::format_measurement(10.0, &style)
            );
            out.push('\n');
        }
        return Ok(without_last_newline(out));
    }

    const DIMSTYLE_USAGE: &str = "DIMSTYLE LIST | INFO name | NEW name [field value ...] | SET \
                                  name field value [field value ...] | DELETE name";
    if args.len() < 2 {
        return Err(usage(DIMSTYLE_USAGE));
    }
    let name = &args[1];
    // The field/value pairs after the name (dimension_style_verbs.rs), read
    // all or nothing: a line with one bad pair changes nothing.
    let pairs = &args[args.len().min(2)..];

    match action.as_str() {
        "INFO" => {
            let model = document.model();
            let Some(current) = model.dimension_styles.find(name) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "dimension style does not exist",
                    name.as_str(),
                ));
            };
            Ok(describe_dimension_style(model, current))
        }
        "NEW" => {
            // With fields, a copy of another style is ONE step: the window's
            // Duplicate would otherwise be a NEW and a SET, two undos for one
            // act.
            let mut created = DimensionStyle {
                name: name.clone(),
                ..DimensionStyle::default()
            };
            if !pairs.is_empty() {
                set_dimension_style_fields(&mut created, pairs)?;
            }
            finish(
                document.execute(create_dimension_style(created)),
                format!("dimension style {name} created"),
            )
        }
        "DELETE" => finish(
            document.execute(delete_dimension_style(name.as_str())),
            format!("dimension style {name} deleted"),
        ),
        "SET" => {
            if pairs.is_empty() {
                return Err(usage(
                    "DIMSTYLE SET name field value [field value ...]\n  fields: TEXT GAP EXTOFF \
                     EXTBEYOND ARROW HEAD SCALE DECIMALS ROUND PREFIX SUFFIX TRIM PAPER",
                ));
            }
            let Some(current) = document.model().dimension_styles.find(name) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "dimension style does not exist",
                    name.as_str(),
                ));
            };
            // Every pair onto one copy, then ONE update: one undo step however
            // many fields the line sets, as the window's Apply sets them.
            let mut changed = current.clone();
            set_dimension_style_fields(&mut changed, pairs)?;
            finish(
                document.execute(update_dimension_style(changed)),
                format!("dimension style {name} updated"),
            )
        }
        _ => Err(usage(DIMSTYLE_USAGE)),
    }
}

// "angle spacing [angle spacing ...]" from args[2] on. Angles are DEGREES
// here and radians in the model. Every other angle the interpreter takes is
// in degrees, because that is what a drafter types; converting at the edge
// keeps the model in one unit.
fn hatch_families(args: &[String]) -> Result<Vec<HatchLineFamily>> {
    let mut parsed = Vec::new();
    let mut i = 2;
    while i + 1 < args.len() {
        let angle = parse_number(&args[i])?;
        let spacing = parse_number(&args[i + 1])?;
        parsed.push(HatchLineFamily {
            angle: angle * DEG_TO_RAD,
            spacing,
            ..HatchLineFamily::default()
        });
        i += 2;
    }
    Ok(parsed)
}

fn hatch_pattern(document: &mut Document, args: &[String]) -> Result<String> {
    let action = args
        .first()
        .map_or_else(|| String::from("LIST"), |word| uppered(word));

    if action == "LIST" {
        let mut out = String::new();
        for pattern in document.model().hatch_patterns.all() {
            out += &format!("  {}", pattern.name);
            if pattern.solid {
                out += "  solid";
            } else if pattern.families.is_empty() {
                out += "  (draws nothing)";
            } else {
                for family in &pattern.families {
                    out += &format!(
                        "  [{} deg @ {}",
                        format_general(family.angle * RAD_TO_DEG, 6),
                        format_general(family.spacing, 6)
                    );
                    if family.offset != 0.0 {
                        out += &format!(" offset {}", format_general(family.offset, 6));
                    }
                    out.push(']');
                }
            }
            if !pattern.description.is_empty() {
                out += &format!("  {}", pattern.description);
            }
            out.push('\n');
        }
        return Ok(without_last_newline(out));
    }

    const HATCH_USAGE: &str = "HATCH LIST | SOLID name | NEW name angle spacing [angle spacing \
                               ...] | SET name angle spacing [angle spacing ...] | SET name \
                               SOLID | DELETE name";
    if args.len() < 2 {
        return Err(usage(HATCH_USAGE));
    }
    let name = &args[1];

    match action.as_str() {
        "DELETE" => finish(
            document.execute(delete_hatch_pattern(name.as_str())),
            format!("hatch pattern {name} deleted"),
        ),
        "SOLID" => {
            let pattern = HatchPattern {
                name: name.clone(),
                solid: true,
                description: String::from("Solid fill"),
                ..HatchPattern::default()
            };
            finish(
                document.execute(create_hatch_pattern(pattern)),
                format!("hatch pattern {name} created (solid)"),
            )
        }
        "NEW" => {
            if args.len() < 4 || !(args.len() - 2).is_multiple_of(2) {
                return Err(usage(
                    "HATCH NEW name angle spacing [angle spacing ...]\n  angle in DEGREES, \
                     spacing in MODEL units. e.g. HATCH NEW brick 45 0.25   (crosshatch: 45 \
                     0.25 135 0.25)",
                ));
            }
            let families = hatch_families(args)?;
            let count = families.len();
            let pattern = HatchPattern {
                name: name.clone(),
                families,
                ..HatchPattern::default()
            };
            finish(
                document.execute(create_hatch_pattern(pattern)),
                format!("hatch pattern {name} created ({count} families)"),
            )
        }
        "SET" => {
            // An existing pattern's families, or its fill, replaced as one
            // undo step - what the Hatch Patterns tab's Save runs. The name
            // and the description are the pattern's and are kept; so is each
            // family's offset where a family of that position remains, since
            // none can be typed and an imported pattern's would otherwise be
            // lost to an edit of its spacing.
            let to_solid = args.len() == 3 && uppered(&args[2]) == "SOLID";
            if !to_solid && (args.len() < 4 || !(args.len() - 2).is_multiple_of(2)) {
                return Err(usage(
                    "HATCH SET name angle spacing [angle spacing ...] | HATCH SET name SOLID\n  \
                     angle in DEGREES, spacing in MODEL units",
                ));
            }
            let Some(existing) = document.model().hatch_patterns.find(name).cloned() else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "hatch pattern does not exist",
                    name.as_str(),
                ));
            };
            let mut changed = existing.clone();
            if to_solid {
                changed.solid = true;
                changed.families.clear();
                // What HATCH SOLID calls one, when it has no words of its own.
                if changed.description.is_empty() {
                    changed.description = String::from("Solid fill");
                }
            } else {
                let mut parsed = hatch_families(args)?;
                for (family, kept) in parsed.iter_mut().zip(&existing.families) {
                    family.offset = kept.offset;
                }
                changed.solid = false;
                changed.families = parsed;
                // HATCH SOLID's own words, which would now be untrue.
                if existing.solid && changed.description == "Solid fill" {
                    changed.description.clear();
                }
            }
            // Setting what is already there is not an edit: no undo step.
            if changed == existing {
                return Ok(format!("hatch pattern {name} unchanged"));
            }
            let kind = if changed.solid {
                String::from("solid")
            } else {
                format!("{} families", changed.families.len())
            };
            finish(
                document.execute(update_hatch_pattern(changed)),
                format!("hatch pattern {name} updated ({kind})"),
            )
        }
        _ => Err(usage(HATCH_USAGE)),
    }
}

fn coordinate_system(document: &mut Document, args: &[String], line: &str) -> Result<String> {
    const CRS_USAGE: &str =
        "CRS | CRS SET code-or-WKT-or-PROJ | CRS CLEAR | CRS FIND words | CRS SUGGEST lon,lat";
    // One fact per line, key=value, as the other replies an agent reads.
    let quote = |text: &str| format!("\"{text}\"");
    let describe_line = |stored: &str| -> String {
        if stored.is_empty() {
            return String::from("crs id=none (local coordinates)");
        }
        match describe_coordinate_system(stored) {
            Ok(description) => format!(
                "crs id={} name={} kind={} units={}",
                description.id,
                quote(&description.name),
                quote(&description.kind),
                description.units
            ),
            Err(_) => format!("crs id={} recognised=no", quote(stored)),
        }
    };
    let choice_line = |entry: &CrsChoice| {
        format!(
            "choice id={} name={} group={}",
            entry.id,
            quote(&entry.name),
            quote(&entry.group)
        )
    };
    // Everything after the sub-verb, as one text: a WKT or PROJ string that
    // was not quoted still arrives whole.
    let rest = || rest_of_line(args, 1);
    let action = args
        .first()
        .map_or_else(|| String::from("SHOW"), |word| uppered(word));
    match action.as_str() {
        "SHOW" | "INFO" => Ok(describe_line(&document.metadata().coordinate_system)),
        "SET" => {
            if args.len() < 2 {
                return Err(usage(CRS_USAGE));
            }
            // The system as typed after CRS SET, not rebuilt from the tokens:
            // those have lost a WKT's quotes, and PROJ refused every pasted
            // WKT (TheCrsVerbTakesAWktWithItsQuotesAsTyped). A text that OPENS
            // with a quote is the grammar's quoted argument - CRS SET "GDA2020
            // / MGA zone 56" - and its quotes only group it.
            let typed = text_after_tokens(line, 2);
            let system = if typed.starts_with('"') {
                rest()
            } else {
                String::from(typed)
            };
            document.set_coordinate_system(&system, Document::SET_CRS)?;
            Ok(describe_line(&document.metadata().coordinate_system))
        }
        "CLEAR" | "NONE" => {
            document.set_coordinate_system("", Document::SET_CRS)?;
            Ok(describe_line(&document.metadata().coordinate_system))
        }
        "FIND" => {
            let found = find_coordinate_systems(&rest());
            let mut text = format!("found count={}", found.len());
            for entry in &found {
                text += &format!("\n{}", choice_line(entry));
            }
            Ok(text)
        }
        "SUGGEST" => {
            let place = rest().replace(',', " ");
            // As a stream reads two words: separated by blanks, the rest left.
            let mut words = place
                .split(|c: char| c.is_ascii() && is_ascii_space(c as u8))
                .filter(|word| !word.is_empty());
            let longitude = parse_number(words.next().unwrap_or(""));
            let latitude = parse_number(words.next().unwrap_or(""));
            let (Ok(longitude), Ok(latitude)) = (longitude, latitude) else {
                return Err(usage(
                    "CRS SUGGEST longitude,latitude   e.g. CRS SUGGEST 151.21,-33.87",
                ));
            };
            let suggested = suggest_coordinate_systems(longitude, latitude)?;
            let mut text = format!("suggested count={}", suggested.len());
            for entry in &suggested {
                text += &format!("\n{}", choice_line(entry));
            }
            Ok(text)
        }
        _ => Err(usage(CRS_USAGE)),
    }
}

fn parcel(document: &mut Document, args: &[String]) -> Result<String> {
    const PARCEL_USAGE: &str = "PARCEL id | PARCEL id LEGAL [name] | PARCEL id LABEL [height]";
    let Some(first) = args.first() else {
        return Err(usage(PARCEL_USAGE));
    };
    let id = parse_id(first)?;
    let report = {
        let Some(entity) = document.model().entities.find(id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "no entity with that id",
                first.as_str(),
            ));
        };
        match &entity.geometry {
            Geometry::CurvePolyline(_) => {
                // Not chorded behind the surveyor's back: a legal
                // description's arc course is radius, arc and chord, which
                // parcel_report does not write.
                return Err(make_error(
                    ErrorCode::Unsupported,
                    "a parcel with arc courses is not reported yet; its courses are bearings \
                     and distances of straight sides",
                    first.as_str(),
                ));
            }
            Geometry::Polyline(boundary) => parcel_report(boundary)?,
            _ => {
                return Err(make_error(
                    ErrorCode::InvalidGeometry,
                    "a parcel must be a closed polyline",
                    first.as_str(),
                ));
            }
        }
    };
    let action = args
        .get(1)
        .map_or_else(|| String::from("REPORT"), |word| uppered(word));

    if action == "LEGAL" {
        let name = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| format!("Parcel {first}"));
        return Ok(legal_description(&report, &name));
    }
    if action == "LABEL" {
        let height = match args.get(2) {
            Some(word) => parse_number(word)?,
            None => 2.5,
        };
        let labels = parcel_labels(&report, height)?;
        // One undo step for the lot: nobody wants to undo a parcel's labels
        // one bearing at a time.
        let attributes = document.current_attributes();
        let mut chain = Transaction::new("PARCEL_LABELS");
        let count = labels.len();
        for label in labels {
            chain.add(create_text(label, attributes.clone()));
        }
        // The layer by name: "the current layer" does not say which, and a
        // person who asked from a dialog has not been looking at it.
        return finish(
            document.execute(Box::new(chain)),
            format!(
                "{count} labels created on layer {}, the current layer",
                attributes.layer
            ),
        );
    }
    if action != "REPORT" {
        return Err(usage(PARCEL_USAGE));
    }
    // What Survey > Parcel Report shows, from the same formatter.
    Ok(format_parcel_report(&report))
}

// ---- survey tools ---------------------------------------------------------------------------
//
// Parsing only: every number comes from survey_tools.rs, and so does every
// word of the reply, which is the report the Survey menu's dialogs show.

fn survey(document: &mut Document, verb: &str, args: &[String]) -> Result<String> {
    if verb == "INVERSE" {
        const INVERSE_USAGE: &str =
            "INVERSE p p | INVERSE line-id   (p = E,N or E,N,Z or a point id)";
        let result: Result<InverseResult> = match args.len() {
            1 => {
                let Ok(id) = parse_id(&args[0]) else {
                    return Err(usage(INVERSE_USAGE));
                };
                let (from, to) = ends_of_line(document, id)?;
                compute_inverse(&from, &to)
            }
            2 => {
                let from = parse_survey_position(document, &args[0])?;
                let to = parse_survey_position(document, &args[1])?;
                compute_inverse(&from, &to)
            }
            _ => return Err(usage(INVERSE_USAGE)),
        };
        return Ok(format_inverse_report(&result?));
    }

    if verb == "FORWARD" {
        if args.len() < 3 || args.len() > 5 {
            return Err(usage(
                "FORWARD p direction distance [dZ [name]]   (dZ - for none)",
            ));
        }
        let from = parse_survey_position(document, &args[0])?;
        let azimuth = parse_survey_direction(&args[1])?;
        let distance = parse_survey_number(&args[2], "the horizontal distance")?;
        // The name only ever comes fifth, so a point numbered "102" is never
        // taken for a height difference of 102.
        let height_difference = if args.len() >= 4 && args[3] != "-" {
            Some(parse_survey_number(&args[3], "the height difference")?)
        } else {
            None
        };
        let input = ForwardInput {
            from,
            azimuth,
            distance,
            height_difference,
            name: args.get(4).cloned().unwrap_or_default(),
        };
        let result = compute_forward(&input)?;
        let command = forward_point_command(document, &result)?;
        let report = format_forward_report(&result);
        let status = document.execute(command);
        return finish(
            status,
            format!("{report}\n  Created on layer {}", document.current_layer()),
        );
    }

    // AREA
    let mut ids = Vec::new();
    if args.is_empty() {
        require_selection(document)?;
        ids = document.selection().ids();
    }
    for word in args {
        let Ok(id) = parse_id(word) else {
            return Err(usage("AREA [id...]   (the selection when no ids)"));
        };
        ids.push(id);
    }
    let result = compute_area(document, &ids)?;
    Ok(format_area_report(&result))
}

fn linetype(document: &mut Document, args: &[String]) -> Result<String> {
    const LINETYPE_USAGE: &str = "LINETYPE LIST | NEW name dash gap [dash gap ...] | RENAME old \
                                  new | DELETE name | MERGE from into";
    let action = args
        .first()
        .map_or_else(|| String::from("LIST"), |word| uppered(word));

    if action == "LIST" {
        let mut out = String::new();
        for linetype in document.model().linetypes.all() {
            out += &format!("  {}", linetype.name);
            if linetype.is_continuous() {
                out += "  (solid)";
            } else {
                let lengths: Vec<String> = linetype
                    .pattern
                    .iter()
                    .map(|element| format_general(element.length, 6))
                    .collect();
                out += &format!(
                    "  [{}]  period={}",
                    lengths.join(" "),
                    format_general(linetype.pattern_length(), 6)
                );
            }
            if !linetype.description.is_empty() {
                out += &format!("  {}", linetype.description);
            }
            out.push('\n');
        }
        return Ok(without_last_newline(out));
    }

    if args.len() < 2 {
        return Err(usage(LINETYPE_USAGE));
    }
    let name = &args[1];

    match action.as_str() {
        "NEW" => {
            if args.len() < 3 {
                return Err(usage(
                    "LINETYPE NEW name dash gap [dash gap ...]   (positive = dash, negative = \
                     gap, 0 = dot; model units)",
                ));
            }
            let mut created = Linetype {
                name: name.clone(),
                description: String::from("User defined"),
                ..Linetype::default()
            };
            for word in &args[2..] {
                created.pattern.push(LinetypeElement {
                    length: parse_number(word)?,
                });
            }
            // validate() has the real rules; this just gives the common
            // mistake a better sentence than "must end with a gap".
            if !created.pattern.len().is_multiple_of(2) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "give a gap after every dash: lengths come in pairs",
                    format!("{} given", created.pattern.len()),
                ));
            }
            finish(
                document.execute(create_linetype(created)),
                format!("linetype {name} created"),
            )
        }
        "DELETE" => finish(
            document.execute(delete_linetype(name.as_str())),
            format!("linetype {name} deleted"),
        ),
        "RENAME" => {
            if args.len() < 3 {
                return Err(usage("LINETYPE RENAME old new"));
            }
            finish(
                document.execute(rename_linetype(name.as_str(), args[2].as_str())),
                format!(
                    "linetype {name} renamed to {} (every layer and style naming it came too)",
                    args[2]
                ),
            )
        }
        "MERGE" => {
            if args.len() != 3 {
                return Err(usage(
                    "LINETYPE MERGE from into   (put a name with spaces in double quotes)",
                ));
            }
            finish(
                document.execute(merge_linetype(name.as_str(), args[2].as_str())),
                format!(
                    "linetype {name} merged into {} (every layer and style naming it now names \
                     {})",
                    args[2], args[2]
                ),
            )
        }
        _ => Err(usage(LINETYPE_USAGE)),
    }
}

fn existing_layer(document: &Document, name: &str) -> Result<Layer> {
    document
        .model()
        .layers
        .find(name)
        .cloned()
        .ok_or_else(|| make_error(ErrorCode::NotFound, "layer does not exist", name))
}

fn layer(document: &mut Document, args: &[String]) -> Result<String> {
    let action = args
        .first()
        .map_or_else(|| String::from("LIST"), |word| uppered(word));

    if action == "LIST" {
        let model = document.model();
        let mut out = String::new();
        for layer in model.layers.all() {
            out += &format!(
                "{}{}  {}{}{}  ({} entities)\n",
                if layer.name == document.current_layer() {
                    "* "
                } else {
                    "  "
                },
                layer.name,
                layer.color.to_hex(),
                if layer.visible { "" } else { "  hidden" },
                if layer.locked { "  locked" } else { "" },
                model.entities.count_on_layer(&layer.name)
            );
        }
        return Ok(without_last_newline(out));
    }
    if args.len() < 2 {
        return Err(usage(
            "LAYER LIST | NEW name [#RRGGBB] | SET|DELETE|SHOW|HIDE|LOCK|UNLOCK name | LTYPE \
             layer linetype",
        ));
    }
    let name = &args[1];

    match action.as_str() {
        "NEW" => {
            let mut created = Layer {
                name: name.clone(),
                ..Layer::default()
            };
            if let Some(colour) = args.get(2) {
                created.color = Color::from_hex(colour)?;
            }
            return finish(
                document.execute(create_layer(created)),
                format!("layer {name} created"),
            );
        }
        "SET" => {
            return finish(
                document.set_current_layer(name),
                format!("current layer is {name}"),
            );
        }
        "LTYPE" => {
            // LAYER LTYPE <layer> <linetype>. Without a way to say this, a
            // defined pattern can never be attached to anything and the whole
            // feature is unreachable from the application.
            if args.len() < 3 {
                return Err(usage("LAYER LTYPE layer linetype"));
            }
            let mut changed = existing_layer(document, name)?;
            // The same names STYLE SET linetype takes, bar ByLayer: a layer is
            // what ByLayer inherits FROM.
            let linetype_name = rest_of_line(args, 2);
            if is_by_layer(&linetype_name) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a layer cannot be ByLayer: it is what ByLayer inherits from",
                    linetype_name,
                ));
            }
            check_linetype_name(document, &linetype_name)?;
            changed.linetype = linetype_name.clone();
            return finish(
                document.execute(update_layer(changed)),
                format!("layer {name} uses linetype {linetype_name}"),
            );
        }
        "HATCH" => {
            if args.len() < 3 {
                return Err(usage("LAYER HATCH layer pattern"));
            }
            let mut changed = existing_layer(document, name)?;
            if !document.model().hatch_patterns.contains(&args[2]) {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "hatch pattern does not exist",
                    args[2].as_str(),
                ));
            }
            changed.hatch_pattern = args[2].clone();
            return finish(
                document.execute(update_layer(changed)),
                format!("layer {name} uses hatch pattern {}", args[2]),
            );
        }
        "DIMSTYLE" => {
            if args.len() < 3 {
                return Err(usage("LAYER DIMSTYLE layer style"));
            }
            let mut changed = existing_layer(document, name)?;
            if !document.model().dimension_styles.contains(&args[2]) {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "dimension style does not exist",
                    args[2].as_str(),
                ));
            }
            changed.dimension_style = args[2].clone();
            return finish(
                document.execute(update_layer(changed)),
                format!("layer {name} uses dimension style {}", args[2]),
            );
        }
        "DELETE" => {
            return finish(
                document.execute(delete_layer(name.as_str())),
                format!("layer {name} deleted"),
            );
        }
        _ => {}
    }

    let mut changed = existing_layer(document, name)?;
    if action == "SHOW" || action == "HIDE" {
        changed.visible = action == "SHOW";
    } else if action == "LOCK" || action == "UNLOCK" {
        changed.locked = action == "LOCK";
        if changed.locked && name == document.current_layer() {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "the current layer cannot be locked",
                name.as_str(),
            ));
        }
    } else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "unknown LAYER action",
            action,
        ));
    }
    finish(
        document.execute(update_layer(changed)),
        format!("layer {name} updated"),
    )
}

// ---- history / inspect -----------------------------------------------------------------

fn undo_redo(document: &mut Document, verb: &str, args: &[String]) -> Result<String> {
    let steps = match args.first() {
        Some(word) => parse_count(word)?,
        None => 1,
    };
    let mut done = 0;
    while done < steps {
        let status = if verb == "UNDO" {
            document.undo()
        } else {
            document.redo()
        };
        if let Err(error) = status {
            if done == 0 {
                return Err(error);
            }
            break; // ran out of history part way: report what was done
        }
        done += 1;
    }
    Ok(format!(
        "{done}{}",
        if verb == "UNDO" { " undone" } else { " redone" }
    ))
}

// A property as INFO prints it: a stream's default notation at precision 6
// for a real, true or false for a flag.
fn streamed(value: &PropertyValue) -> String {
    match value {
        PropertyValue::Boolean(flag) => String::from(if *flag { "true" } else { "false" }),
        PropertyValue::Integer(integer) => format!("{integer}"),
        PropertyValue::Real(real) => format_general(*real, 6),
        PropertyValue::Text(text) => text.clone(),
    }
}

fn inspect(document: &Document, verb: &str, args: &[String]) -> Result<String> {
    let model = document.model();
    // The drawing's state, as katana_mcp's katana_status gives it and File >
    // Drawing Summary copies it (document_status.rs): one definition, so an
    // agent reads the same record whichever front end it drives.
    if verb == "STATUS" {
        if args.is_empty() {
            return Ok(format_status(&document_status(document)));
        }
        if args.len() == 1 && uppered(&args[0]) == "JSON" {
            return Ok(status_json(&document_status(document)));
        }
        return Err(usage("STATUS [JSON]"));
    }
    if verb == "INFO" {
        if args.len() != 1 {
            return Err(usage("INFO id"));
        }
        // #12 as well as 12: how an anchored point names an entity, so an id
        // copied from a DIM line is taken as it is.
        let word = args[0].strip_prefix('#').unwrap_or(&args[0]);
        let id = parse_id(word)?;
        let Some(entity) = model.entities.find(id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not exist",
                args[0].as_str(),
            ));
        };
        let mut text = describe(model, entity, &leader_context(document));
        for (key, value) in &entity.properties {
            text += &format!("\n    {key} = {}", streamed(value));
        }
        return Ok(text);
    }
    // LIST takes no words. It once ignored them, so LIST LAYER x listed the
    // whole drawing and read as though it had filtered it.
    if !args.is_empty() {
        return Err(usage("LIST (every entity; INFO id describes one)"));
    }
    let mut text = format!("{} entities", model.entities.size());
    let leaders = leader_context(document);
    model.entities.for_each(|entity| {
        text += &format!("\n{}", describe(model, entity, &leaders));
    });
    Ok(text)
}

const HELP_TEXT: &str = r#"Points: x,y | @dx,dy (relative) | @dist<angle (polar, degrees). Angles in degrees.

Draw      POINT p | LINE p p [p...] | PLINE p p [p...] [CLOSE] | RECT p p
          CIRCLE centre radius | ARC p p p (start, on-arc, end)
          TEXT p height "text" | DIM p p offset
Modify    (act on the selection)
          MOVE dx,dy | COPY dx,dy | ROTATE centre angle | SCALE centre factor
          MIRROR p p [KEEP] | ARRAY rows columns dx,dy | ERASE
Edit      OFFSET id distance side-point | TRIM id pick-point cutter-id...
          EXTEND id pick-point boundary-id... | FILLET id id radius
          CHAMFER id id distance [distance2]
Select    SELECT ALL | NONE | id... (or #id) | LAYER name | TYPE name
Layers    LAYER LIST | NEW name [#RRGGBB] | SET name | DELETE name
          LAYER SHOW|HIDE|LOCK|UNLOCK name | LAYER LTYPE layer linetype
Linetype  LINETYPE LIST | NEW name dash gap [dash gap ...] | RENAME old new | DELETE name
          LINETYPE MERGE from into   (repoints every layer and style, then deletes from)
          lengths are MODEL units: + dash, - gap, 0 dot. e.g. LINETYPE NEW fence 1 -0.5
Hatch     HATCH LIST | SOLID name | NEW name angle spacing [angle spacing ...] | DELETE name
          HATCH SET name angle spacing [angle spacing ...] | SET name SOLID   edits one
          angle in DEGREES, spacing in MODEL units.  LAYER HATCH layer pattern attaches one
Style     STYLE LIST | SYMBOLS [filter] | NEW name [field value] | SET name field value
          STYLE RENAME old new
          STYLE DELETE name | APPLY name | MERGE from into | USAGE [name] | CURRENT [name|-]
          fields: linetype weight colour hatch symbol symbolsize description; APPLY - = ByLayer
          linetype takes a model linetype, a loaded library linestyle or ByLayer (the layer's)
Purge     PURGE [STYLES|LINETYPES|HATCHES|ALL]   deletes what nothing uses, as one undo step
Align     ALIGN LIST | NEW name x,y x,y [x,y ...] | PI name x,y [radius [spIn [spOut]]]
          SET name index radius [spIn [spOut]] | START name station
          PIS name prints the PIs; PIS name x,y[,r[,in[,out]]] ... replaces them all
          STATIONS name interval   the setting-out table with the key stations, key=value
          DELETE name.  PI indices count from 0; radius 0 is a kink; spirals in MODEL units
          DESIGN name s,z[,L] s,z[,L] ... defines the design profile (parabolic vertical
          curves, symmetric); PVI name s z [L] appends; PROFILE name prints it with its
          high and low points; CLEARPROFILE name removes it
CRS       CRS   the project's coordinate system  |  CRS SET EPSG:7856 (or a code, WKT, PROJ)
          CRS CLEAR (local coordinates)  |  CRS FIND words (the common list: CRS FIND mga 56)
          CRS SUGGEST lon,lat   the systems that suit a place, best first; SET is one undo step
Parcel    PARCEL id            bearings, distances, area and centroid of a closed polyline
          PARCEL id LEGAL [name]   the deed wording;  PARCEL id LABEL [height]   text labels
Survey    INVERSE p p | INVERSE line-id   distance, azimuth, bearing; height difference,
          slope distance and grade when both ends have an elevation
          FORWARD p direction distance [dZ [name]]   a new point on the current layer, one
          undo step (alias RADIATE); dZ - for none. p is E,N or E,N,Z (easting first) or a
          point's id. direction: an azimuth 36d52m11.63s | 36:52:11.63 | 36-52-11.63 |
          36.8699 (decimal degrees) or a bearing N36d52m11.63sE ("N 36 52 11.63 E" quoted)
          AREA [id...]   area and perimeter of closed polylines and circles, each and in
          total (the selection when no ids); hectares when the project unit is the metre
Codes     CODE [scope] [WHERE k=v ...] [PROPERTY name] [PREVIEW]   apply the loaded survey
          codes to the entities the scope takes that carry a field code (the property
          found when not named), one undo step; PREVIEW reports and changes nothing.
          With no scope word it is the whole DRAWING, under a bare WHERE too: CODE has
          always been the whole drawing, where MODIFY with none is the selection
          CODE CENSUS [scope] [WHERE k=v ...] [PROPERTY name] [PREVIEW]   the codes those
          entities carry (it changes nothing, so PREVIEW asks nothing more of it)
          CODE EXPLAIN code   why a code gets what it gets
          CODE LIST [filter]   the loaded survey codes, one per line  |  CODE CHECK   the
          same checked: it fails when a rule has an error
          CODE name and CODE CENSUS name still read name as the property; one called
          as a scope word or a subcommand, or of several words the first of which is
          one (Area m2), is PROPERTY "name"
Customise CUSTOMISE [REPLACE] <file> [<file>...]   load Katana customisation files, merged
          into what is loaded (REPLACE: in the place of each kind they bring); all or
          nothing. CUSTOMISE alone reports what is loaded, CUSTOMISE JSON as JSON
          CUSTOMISE DEFINITIONS <file> [<file>...]   merge their definitions and colours alone
          CUSTOMISE EXPORT <file> [CODES] [LINESTYLES] [SYMBOLS] [NAME name] [ONLY name...]
          CUSTOMISE BUILTIN [<name>]   list the built-ins, or make one the session
          CUSTOMISE RESET (the built-in one) | KEEP (this one, for the next start) | REVERT
          CUSTOMISE REMOVE name... [FORCE] | REMOVE CODE key...
          CUSTOMISE SET auto.codes=on|off auto.linework=on|off linework.start=ST ...
          (HELP CUSTOMISE: every word and every reply)
DimStyle  DIMSTYLE LIST | INFO name | NEW name [field value ...] | DELETE name
          DIMSTYLE SET name field value [field value ...]   every pair one undo step
          fields TEXT GAP EXTOFF EXTBEYOND ARROW HEAD SCALE DECIMALS ROUND PREFIX SUFFIX TRIM
          PAPER on|off (sizes in paper mm, drawn at the annotation scale)
          LAYER DIMSTYLE layer style   attaches one
Attribs   CHLAYER name | COLOR #RRGGBB|BYLAYER   (selection)
Global    MODIFY [scope] [WHERE k=v ...] SET k=v ... [PREVIEW]   global modify, one undo step
Scope     MODIFY, CODE and the UTILITY verbs: SELECTION | DRAWING | VIEW [id] [EXTENTS]
          (the window's plan view as on screen; EXTENTS: its layers anywhere; id is the
          view=N a reply gives, not its title's number) | AREA x0,y0,x1,y1 |
          LAYERS a,b [ONLY] (ONLY: not their sublayers). MODIFY with none takes the
          selection and CODE the drawing; a UTILITY verb takes a scope word or WHERE,
          else reads a file
          WHERE: TYPE=point,line LAYER=pat STYLE=pat|ByLayer COLOUR=#RRGGBB|ByLayer
          PROP=key[:pat] (PROP=:pat: any property) TEXT=pat DRAWN ('*' '?' wildcards)
          SET entities: LAYER= COLOUR= STYLE=name|ByLayer VISIBLE=yes|no PROP=key:value
          UNPROP=key HEIGHT=h SYMBOL=name[@size]   layers: LAYER.COLOUR= LAYER.LTYPE=
          LAYER.WEIGHT= LAYER.HATCH= LAYER.DIMSTYLE= LAYER.VISIBLE= LAYER.LOCKED=
          styles worn: STYLE.COLOUR= STYLE.LTYPE= STYLE.WEIGHT= STYLE.HATCH= STYLE.SYMBOL=
          STYLE.SYMBOLSIZE= STYLE.SYMBOLSCALE=factor (2 doubles each symbol size)
          PREVIEW reports what would change and changes nothing
Props     PROP LIST | SET key value [text|integer|real|boolean] | DELETE key
          PROP RENAME old new   (selection; the type is guessed unless stated)
          PROP TREE [scope] [WHERE k=v ...] [UNDER path] [FROM n] [LIMIT n]   the
          properties as the tree their '/' names make, one level a reply (UNDER vertex/3)
History   UNDO [n] | REDO [n]
File      NEW | OPEN directory | SAVE [directory]
Inspect   LIST | INFO id (or #id) | STATUS [JSON] (the drawing at a glance) | HELP
Sheets    SHEETS [LIST] | JSON [path] | SAVE path | LOAD path      (HELP SHEETS: every option)
          SHEET NEW|REMOVE|MOVE|COPY|RENAME|SET|FIELD | VIEW ADD|SET|REMOVE|LIST | TILE n preset
          GENERATE fit|grid|strips|profile|sections|frames | TITLEBLOCK [LIST] | field value
          TITLEBLOCK REVISION ADD|REMOVE | LOGO path
          PLOTSHEETS path.pdf [sheets=1,3-5] [dpi=300]   the desktop window's alone (it paints);
          katana_cli refuses it - headless, katana <project> --plot-sheets out.pdf
Utility   UTILITY REPORT|VERIFY|CLEARANCE|CHECK schedule.csv|scope ...  AS 5488 subsurface utilities:
          grade, verify, clear, check against a schema, on a schedule or what is drawn;
          DRAW schedule.csv, REGRADE scope, SCHEDULE out.csv scope (HELP UTILITY)
Volumes   VOLUME SURFACES|DATUM|STOCKPILE|SECTIONS|GRID ...   cut and fill between the
          drawing's surfaces, to a level, of a stockpile, along an alignment or on a grid,
          inside polygons; SURFACE DIFFERENCE|ENVELOPE ... a cut/fill map or envelope as a
          new surface (HELP VOLUME)
RoadObj   ROADOBJECT KINDS | LIST [JSON|scope] | INFO id | ADD kind <placement> [key=value ...]
          | SET id key=value ... | COPY id | DELETE id ...|ALL|scope | QUANTITIES [id ...|scope]
          | PROBLEMS   barriers, guide posts, signs, light poles, culverts, walls, fences, line
          markings along an alignment, along lines or at points, one undo step; LIST, QUANTITIES
          and DELETE take the shared scope and WHERE (HELP ROADOBJECT)
Road      ROADTEMPLATE [LIST] | INFO | DEFAULTS | NEW | ADD | SET | REMOVE | COURSE | EXPORT |
          IMPORT ...  road templates: lanes, shoulders, kerbs, footpaths, verges, medians,
          drains, batters and their pavement courses, kept in a file by EXPORT and IMPORT;
          CORRIDOR NEW c ALIGNMENT a TEMPLATE t [surface=s] [curvetol=e] | REGION |
          SUPERELEVATION | BUILD | SECTION | STRINGS | SURFACE | QUANTITIES [csv=]; ROADMODEL
          [LIST] | JSON | PROBLEMS | INFO id   the 3D road model (HELP CORRIDOR)
Linework  LINEWORK [scope] [WHERE k=v ...] [PROPERTY name] [ORDER number|entity]
          [CHORD length] [PREVIEW]   joins coded survey points into lines by the survey
          codes and their control codes, one undo step; no scope word: the selection, else
          the drawing. Points their survey job has strung, and lines already drawn, are left out
          and counted (HELP LINEWORK)
Views     the desktop window's views, except SCALE also edits saved plan views headlessly:
          VIEWS [LIST] | OPEN plan|3d|section|elevation | ACTIVATE id   a record per view;
          id is the view= a record gives, not its title's number
          VIEWS LINK id[,id...] [TO id]   linked plan views pan and zoom together: every one
          shows the centre and scale of the one moved last; TO is the one the others come to
          VIEWS UNLINK id[,id...] | ALL
          VIEWS HIDE id layer[,layer...] | SHOW id layer[,layer...]|ALL | ISOLATE id layer
          the layers that view hides of its own, as its Layers button sets them
          VIEWS HIDE|SHOW|ISOLATE id LAYER name...   each word after LAYER is one layer, commas
          and all (a layer's name may hold one)
          VIEWS HIDE|SHOW id REFERENCE ref[,ref...]   the imported rasters and point clouds
          that view hides of its own, by the id the Reference Data panel gives them
          VIEWS SET id ghosts=on|off   whether that view shows the selection faintly where it
          hides the layer (on when a view opens)
          VIEWS SCALE id AUTO|1:N   paper linestyles and symbols at a reference scale,
          independent of zoom and ANNOSCALE; AUTO keeps their screen size. Saved plan-view
          ids are available to katana_cli and katana_mcp; other view operations need the window
Zoom      ZOOM (Z)   the active view's extents, as 'ZOOM and 'Z are | ZOOM EXTENTS (E, ALL,
          A) | IN [f] | OUT [f] | f | fX | WINDOW x0,y0,x1,y1 | CENTRE x,y [SCALE
          pixels-per-unit]   [view=id]; IN and OUT are by 2 about the view's centre;
          linked views follow every ZOOM
          ZOOM SELECTION | DRAWING | VIEW [id] | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY]
          [WHERE k=v ...]   frames what the scope takes (matched=0: nothing moves)
Aliases   L PL C A PO REC T M CO RO SC MI AR E O TR EX F CHA U LA SEL RADIATE GM Z ?  LE MT TS LS
"#;
