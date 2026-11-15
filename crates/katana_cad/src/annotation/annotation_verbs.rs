//! The annotation verbs of the command line (docs/annotation.md, "The
//! command line"; src/katana_cad/annotation/annotation_verbs.cpp):
//! CommandInterpreter's members for ANNOSCALE, TEXTSTYLE, TEXT and MTEXT with
//! options, TEXTEDIT, LABELSTYLE, LABEL, AUTOLABEL, the DIM kinds, LEADER and
//! BALLOON. Kept here, with the rest of the annotation code, rather than in
//! command_interpreter.rs.
//!
//! Written for an agent as much as for a person, which decides three things:
//!
//! * OPTIONS ARE key=value, in any order after the positional arguments, keys
//!   case-insensitive; an unknown key is refused naming the keys that are
//!   known, so a typo is an error and not a silently ignored setting.
//! * REPLIES ARE RECORDS: `key=value` pairs separated by spaces, one record
//!   per line, values quoted when they hold a space or a quote. What was made
//!   says its id; a LIST says every field a SET takes.
//! * EVERY EDIT IS ONE STEP through Document::execute: one UNDO takes back a
//!   LABEL of forty entities or an AUTOLABEL RUN of four thousand labels.
//!
//! "\n" (a backslash and an n) in a text, template or note value is a line
//! break, since a command is one line.

use std::collections::{BTreeMap, BTreeSet};

use katana_commands::{
    ChangeSet, ChangeSetCommand, CommandPtr, Transaction, create_dimension, create_label_rule,
    create_label_style, create_text, create_text_style, delete_entities, delete_label_rule,
    delete_label_style, delete_text_style, set_entity_geometry, update_label_rule,
    update_label_style, update_text_style,
};
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double, unescape_typed};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::annotation::{
    DEFAULT_TEXT_STYLE_NAME, annotation_model_size, label_kind_from_string,
    label_marker_from_string, label_orientation_from_string, label_placement_from_string,
};
use katana_entity::entity::{
    anchor_point_from_string, callout_shape_from_string, text_justify_from_string,
};
use katana_entity::label_text::{check_label_template, label_value_names};
use katana_entity::leader_values::{anchor_values, is_smart};
use katana_entity::tables::arrow_head_from_string;
use katana_entity::{
    AnchorPoint, AnchorRef, ArrowHead, Color, DimensionGeometry, Entity, EntityId, EnumName,
    Geometry, LabelKind, LabelPlacement, LabelRule, LabelStyle, LeaderContext, LeaderGeometry,
    Model, PropertyValue, TextGeometry, TextStyle,
};
use katana_geometry::{Box2, Point2, Vec2};
use katana_math::{DEG_TO_RAD, PI, RAD_TO_DEG};

use crate::annotation::auto_label::{
    AutoLabelReport, LabelRequest, auto_label, clear_auto_labels, create_labels,
    default_label_styles,
};
use crate::annotation::command_words::record_value;
use crate::annotation::dimension_build::{
    AnchoredPoint, DimensionChain, aligned_dimension, anchored_point, angular_between_lines,
    angular_dimension, default_anchor, dimension_chain, linear_dimension, ordinate_dimension,
    radial_dimension,
};
use crate::annotation::label_layout::{
    LabelLayoutOptions, label_entities, label_keep_out, layout_labels, shown_label_text,
};
use crate::annotation::leader_draw::code_properties;
use crate::annotation::leader_edit::{
    BalloonOrder, LeaderChange, LeaderNote, LeaderNoteKind, LeaderVertexMove, LeadersForOptions,
    align_leaders, attach_leader, change_leaders, leader_says, leader_target_values,
    leader_value_rows, leaders_for, new_leader, parse_leader_landing_angle, release_leaders,
    renumber_balloons, require_leaders, set_leaders_target_property,
};
use crate::annotation::leader_presets::{
    LeadersScopeReport, PresetReads, ResolvedPreset, leader_context, leader_preset_named,
    leader_presets, leaders_json_of, leaders_report, match_leaders, preset_info, recognise_preset,
    resolve_leader_preset,
};
use crate::annotation::text_layout::fit_model_height;
use crate::command_interpreter::{CommandInterpreter, finish, parse_signed};
use crate::dimension_draw::{dimension_text, resolve_dimension_style};
use crate::document::Document;
use crate::scope_verbs::{
    ScopeSource, ScopeViewProvider, ScopeWords, is_scope_word, is_where_key, match_scope,
    parse_scope_words, scope_record,
};

// LABEL LAYOUT's keep-out at most: the plan painter's own bound, so a drawing
// too large for it is judged the same way on both.
const LAYOUT_LINEWORK_LIMIT: usize = 200_000;

fn upper_case(text: &str) -> String {
    text.to_ascii_uppercase()
}

fn lower_case(text: &str) -> String {
    text.to_ascii_lowercase()
}

fn usage_of(text: &str) -> Error {
    Error::new(ErrorCode::InvalidArgument, format!("usage: {text}"))
}

// ---- options ------------------------------------------------------------------------

// The positional arguments and the key=value options of a verb. A token is an
// option when what precedes its first '=' is a word (letters, digits, '-'),
// so a template or a note that holds an '=' is still an option's value.
#[derive(Clone, Debug, Default)]
struct Arguments {
    positional: Vec<String>,
    options: BTreeMap<String, String>,
}

impl Arguments {
    fn has(&self, key: &str) -> bool {
        self.options.contains_key(key)
    }

    fn find(&self, key: &str) -> Option<&String> {
        self.options.get(key)
    }
}

fn is_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

// The key of `token` when it is an option (lower case), and where its '=' is.
fn option_key(token: &str) -> Option<(String, usize)> {
    let equals = token.find('=')?;
    is_key(&token[..equals]).then(|| (lower_case(&token[..equals]), equals))
}

// A typed text read back, as field() writes it: "\n" a line break, "\\" a
// backslash (core's reader, shared with the sheet verbs).
fn unescape(text: &str) -> String {
    unescape_typed(text)
}

fn unknown_option(key: &str, known: &[&str]) -> Error {
    make_error(
        ErrorCode::InvalidArgument,
        format!("unknown option '{key}'"),
        format!("known: {}", known.join(" ")),
    )
}

fn split_arguments(args: &[String], from: usize, known: &[&str]) -> Result<Arguments> {
    let mut result = Arguments::default();
    for token in args.get(from..).unwrap_or_default() {
        if let Some((key, equals)) = option_key(token) {
            if !known.contains(&key.as_str()) {
                return Err(unknown_option(&key, known));
            }
            result
                .options
                .insert(key, String::from(&token[equals + 1..]));
        } else {
            result.positional.push(token.clone());
        }
    }
    Ok(result)
}

fn number_of(text: &str, what: &str) -> Result<f64> {
    parse_finite_double(text).ok_or_else(|| {
        make_error(
            ErrorCode::ParseFailure,
            format!("expected a number for {what}"),
            text,
        )
    })
}

fn switch_of(text: &str, what: &str) -> Result<bool> {
    match lower_case(text).as_str() {
        "on" | "yes" | "true" | "1" => Ok(true),
        "off" | "no" | "false" | "0" => Ok(false),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            format!("expected on or off for {what}"),
            text,
        )),
    }
}

fn integer_of(text: &str, what: &str) -> Result<i32> {
    parse_signed::<i32>(text).ok_or_else(|| {
        make_error(
            ErrorCode::ParseFailure,
            format!("expected a whole number for {what}"),
            text,
        )
    })
}

// A colour, or ByLayer (none).
fn colour_of(text: &str) -> Result<Option<Color>> {
    let value = lower_case(text);
    if value == "bylayer" || value == "-" || value.is_empty() {
        return Ok(None);
    }
    Ok(Some(Color::from_hex(text)?))
}

fn id_of(text: &str) -> Result<EntityId> {
    let text = text.strip_prefix('#').unwrap_or(text);
    let digits = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    match digits.then(|| text.parse::<EntityId>().ok()).flatten() {
        Some(value) if value != 0 => Ok(value),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            "expected an entity id",
            text,
        )),
    }
}

// ---- replies --------------------------------------------------------------------------

// A value as a record field (command_words.rs, record_value: the one rule,
// shared with DIMSTYLE INFO's record).
fn field(value: &str) -> String {
    record_value(value)
}

// Shortest text that reads back as the same double, as every number in a
// reply is: an agent that reads 2.5 back must get 2.5.
fn real(value: f64) -> String {
    format_exact_real(value)
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn colour_text(colour: Option<Color>) -> String {
    colour.map_or_else(|| String::from("bylayer"), Color::to_hex)
}

fn degrees(radians: f64) -> String {
    real(radians * RAD_TO_DEG)
}

// ---- text styles ----------------------------------------------------------------------

const TEXT_STYLE_KEYS: &[&str] = &[
    "font", "paper", "width", "oblique", "bold", "italic", "colour", "color", "mask", "margin",
    "readable", "spacing",
];

fn apply_text_style_options(args: &Arguments, style: &mut TextStyle) -> Status {
    for (key, value) in &args.options {
        match key.as_str() {
            "font" => style.font_family = value.clone(),
            "paper" => style.paper_height = number_of(value, "paper")?,
            "width" => style.width_factor = number_of(value, "width")?,
            "oblique" => style.oblique = number_of(value, "oblique")? * DEG_TO_RAD,
            "bold" => style.bold = switch_of(value, key)?,
            "italic" => style.italic = switch_of(value, key)?,
            "mask" => style.mask = switch_of(value, key)?,
            "readable" => style.readable = switch_of(value, key)?,
            "colour" | "color" => style.color = colour_of(value)?,
            "margin" => style.mask_margin = number_of(value, "margin")?,
            "spacing" => style.line_spacing = number_of(value, "spacing")?,
            _ => {}
        }
    }
    Ok(())
}

fn describe_text_style(style: &TextStyle) -> String {
    format!(
        "name={} font={} paper={} width={} oblique={} bold={} italic={} colour={} mask={} \
         margin={} readable={} spacing={}",
        field(&style.name),
        field(&style.font_family),
        real(style.paper_height),
        real(style.width_factor),
        degrees(style.oblique),
        on_off(style.bold),
        on_off(style.italic),
        colour_text(style.color),
        on_off(style.mask),
        real(style.mask_margin),
        on_off(style.readable),
        real(style.line_spacing)
    )
}

// ---- label styles ---------------------------------------------------------------------

const LABEL_STYLE_KEYS: &[&str] = &[
    "kind",
    "text",
    "textstyle",
    "paper",
    "placement",
    "orientation",
    "offset",
    "leader",
    "displace",
    "priority",
    "colour",
    "color",
    "marker",
    "markersize",
    "minlength",
    "interval",
    "tick",
    "ticklength",
];

// The template a new style of each kind starts with.
fn default_template(kind: LabelKind) -> String {
    String::from(match kind {
        LabelKind::Point => "{point}",
        LabelKind::Segment => "{bearing:dms} {distance:.3f}",
        LabelKind::Arc => "R {radius:.3f}",
        LabelKind::Area => "{area:m2:.1f} m\u{b2}",
        LabelKind::Chainage => "{chainage:ch}",
    })
}

fn apply_label_style_options(args: &Arguments, style: &mut LabelStyle) -> Status {
    for (key, value) in &args.options {
        let number = || number_of(value, key);
        match key.as_str() {
            "kind" => {} // fixed at NEW
            "text" => style.text = unescape(value),
            "textstyle" => style.text_style = value.clone(),
            "paper" => style.paper_height = number()?,
            "offset" => style.offset = number()?,
            "markersize" => style.marker_size = number()?,
            "minlength" => style.minimum_length = number()?,
            "interval" => style.interval = number()?,
            "tick" => style.tick_interval = number()?,
            "ticklength" => style.tick_length = number()?,
            "placement" => style.placement = label_placement_from_string(value)?,
            "orientation" => style.orientation = label_orientation_from_string(value)?,
            "marker" => style.marker = label_marker_from_string(value)?,
            "leader" => style.leader = switch_of(value, key)?,
            "displace" => style.displace = switch_of(value, key)?,
            "priority" => style.priority = integer_of(value, "priority")?,
            "colour" | "color" => style.color = colour_of(value)?,
            _ => {}
        }
    }
    Ok(())
}

fn describe_label_style(style: &LabelStyle) -> String {
    let text_style = if style.text_style.is_empty() {
        DEFAULT_TEXT_STYLE_NAME
    } else {
        style.text_style.as_str()
    };
    let mut out = format!(
        "name={} kind={} text={} textstyle={} paper={} placement={} orientation={} offset={} \
         leader={} displace={} priority={} colour={} marker={} markersize={} minlength={}",
        field(&style.name),
        style.kind.name(),
        field(&style.text),
        field(text_style),
        real(style.paper_height),
        style.placement.name(),
        style.orientation.name(),
        real(style.offset),
        on_off(style.leader),
        on_off(style.displace),
        style.priority,
        colour_text(style.color),
        style.marker.name(),
        real(style.marker_size),
        real(style.minimum_length)
    );
    if style.kind == LabelKind::Chainage {
        out += &format!(
            " interval={} tick={} ticklength={}",
            real(style.interval),
            real(style.tick_interval),
            real(style.tick_length)
        );
    }
    out
}

// ---- rules ------------------------------------------------------------------------

const RULE_KEYS: &[&str] = &["style", "layer", "code", "type", "labellayer", "enabled"];

fn apply_rule_options(args: &Arguments, rule: &mut LabelRule) -> Status {
    for (key, value) in &args.options {
        match key.as_str() {
            "style" => rule.label_style = value.clone(),
            "layer" => rule.layer = value.clone(),
            "code" => rule.code = value.clone(),
            "type" => rule.entity_type = value.clone(),
            "labellayer" => rule.label_layer = value.clone(),
            "enabled" => rule.enabled = switch_of(value, "enabled")?,
            _ => {}
        }
    }
    Ok(())
}

fn describe_rule(rule: &LabelRule) -> String {
    format!(
        "name={} style={} layer={} code={} type={} labellayer={} enabled={}",
        field(&rule.name),
        field(&rule.label_style),
        field(&rule.layer),
        field(&rule.code),
        field(&rule.entity_type),
        field(&rule.label_layer),
        on_off(rule.enabled)
    )
}

// Runs a command built elsewhere; None is "nothing to do", said.
fn run_built(
    document: &mut Document,
    built: Result<Option<CommandPtr>>,
    done: String,
    nothing: &str,
) -> Result<String> {
    let Some(command) = built? else {
        return Ok(String::from(nothing));
    };
    document.execute(command)?;
    Ok(done)
}

fn id_list(ids: &[EntityId]) -> String {
    let words: Vec<String> = ids.iter().map(|id| format!("{id}")).collect();
    words.join(",")
}

// ---- text, MTEXT, TEXTEDIT ------------------------------------------------------------

const TEXT_KEYS: &[&str] = &[
    "style", "paper", "justify", "rotation", "height", "text", "at",
];

// The options a styled text takes, onto `text`. The model height of a
// paper-sized text is set for the document's annotation scale (what its box
// and picking go by; TextGeometry::height).
fn apply_text_options(
    args: &Arguments,
    model: &Model,
    scale: f64,
    text: &mut TextGeometry,
) -> Status {
    for (key, value) in &args.options {
        match key.as_str() {
            "style" => {
                if !value.is_empty() && !model.text_styles.contains(value) {
                    return Err(make_error(
                        ErrorCode::NotFound,
                        "text style does not exist",
                        value.as_str(),
                    ));
                }
                text.style = value.clone();
            }
            "paper" => text.paper_height = number_of(value, "paper")?,
            "justify" => text.justify = text_justify_from_string(value)?,
            "rotation" => text.rotation = number_of(value, "rotation")? * DEG_TO_RAD,
            "height" => text.height = number_of(value, "height")?,
            "text" => text.text = unescape(value),
            _ => {}
        }
    }
    // The Text and Multiline Text tools make theirs by the same rule.
    fit_model_height(model, scale, text);
    Ok(())
}

// ---- LEADER, BALLOON ---------------------------------------------------------------------
//
// Thin: each subcommand reads its words into the edit leader_edit.rs makes,
// which the window's Leaders manager makes too, and replies with what
// happened.

const LEADER_USAGE: &str = "LEADER p p [p...] [text= | template= | labelstyle= | preset= \
                            [surface= | alignment=]] [arrow= callout= style= paper= arrowsize= \
                            landing=] | LEADER LIST | PRESETS | VALUES | SET | ATTACH | DETACH | \
                            FREEZE | PROP | FOR | ALIGN (HELP says each)";

// The long names and the short ones a person types.
fn arrow_of(value: &str) -> Result<ArrowHead> {
    let head = arrow_head_from_string(value);
    if let Ok(head) = head {
        return Ok(head);
    }
    match lower_case(value).as_str() {
        "closed" | "filled" => Ok(ArrowHead::ClosedFilled),
        "open" => Ok(ArrowHead::Open),
        "tick" => Ok(ArrowHead::Tick),
        "dot" => Ok(ArrowHead::Dot),
        "none" => Ok(ArrowHead::None),
        _ => head,
    }
}

// The note and look options as a change: at most one of text= (literal),
// template= (read off the tip's entity), labelstyle= (that style's template)
// and, for a balloon, n=; arrow=, callout=, style=, paper=, arrowsize=,
// landing=.
fn leader_change_of(args: &Arguments) -> Result<LeaderChange> {
    let mut change = LeaderChange::default();
    let given = ["text", "template", "labelstyle", "n", "preset"]
        .iter()
        .filter(|key| args.has(key))
        .count();
    if given > 1 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a leader's note is one of text=, template=, labelstyle= or preset=",
        ));
    }
    for key in ["text", "n"] {
        if let Some(text) = args.find(key) {
            change.note = Some(LeaderNote {
                kind: LeaderNoteKind::Text,
                text: unescape(text),
            });
        }
    }
    if let Some(text) = args.find("template") {
        change.note = Some(LeaderNote {
            kind: LeaderNoteKind::Template,
            text: unescape(text),
        });
    }
    if let Some(name) = args.find("labelstyle") {
        change.note = Some(LeaderNote {
            kind: LeaderNoteKind::LabelStyle,
            text: name.clone(),
        });
    }
    if let Some(value) = args.find("arrow") {
        change.arrow = Some(arrow_of(value)?);
    }
    if let Some(value) = args.find("callout") {
        change.callout = Some(callout_shape_from_string(value)?);
    }
    if let Some(value) = args.find("style") {
        change.text_style = Some(value.clone());
    }
    for (key, target) in [
        ("paper", &mut change.paper_height),
        ("arrowsize", &mut change.arrow_size),
        ("landing", &mut change.landing),
    ] {
        if let Some(value) = args.find(key) {
            *target = Some(number_of(value, key)?);
        }
    }
    if let Some(value) = args.find("landingangle") {
        change.landing_angle = Some(parse_leader_landing_angle(value)?);
    }
    Ok(change)
}

// preset= - with the surface= or alignment= it reads - made the note of
// `change`: the preset's template for this drawing (leader_presets.rs), so a
// typed line and a preset tool make one leader. None without preset=;
// refused for surface= or alignment= without one.
fn preset_of(
    args: &Arguments,
    document: &Document,
    change: &mut LeaderChange,
) -> Result<Option<ResolvedPreset>> {
    let surface = args.find("surface");
    let alignment = args.find("alignment");
    let Some(word) = args.find("preset") else {
        if surface.is_some() || alignment.is_some() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "surface= and alignment= say what a preset= reads: give the preset",
            ));
        }
        return Ok(None);
    };
    let preset = leader_preset_named(word)?;
    let resolved = resolve_leader_preset(
        document,
        preset,
        surface.map(String::as_str),
        alignment.map(String::as_str),
    )?;
    change.note = Some(LeaderNote {
        kind: LeaderNoteKind::Template,
        text: resolved.template_text.clone(),
    });
    Ok(Some(resolved))
}

// What a reply adds for a preset made: which, what it reads, and - when
// nobody named it - that it was chosen.
fn preset_reply(preset: Option<&ResolvedPreset>) -> String {
    let Some(preset) = preset else {
        return String::new();
    };
    let info = preset_info(preset.preset);
    let mut reply = format!(" preset={}", info.word);
    if !preset.reads.is_empty() {
        reply += if info.reads == PresetReads::Surface {
            " surface="
        } else {
            " alignment="
        };
        reply += &field(&preset.reads);
    }
    if !preset.chosen.is_empty() {
        reply += &format!(" chosen={}", field(&preset.chosen));
    }
    reply
}

fn leader_of(entity: &Entity) -> Option<&LeaderGeometry> {
    match &entity.geometry {
        Geometry::Leader(leader) => Some(leader),
        _ => None,
    }
}

// One leader as a record: where it is, what it is on, what it says.
fn describe_leader(model: &Model, entity: &Entity, context: &LeaderContext) -> String {
    let Some(leader) = leader_of(entity) else {
        return String::new();
    };
    let tip = leader.vertices.first().copied().unwrap_or_default();
    let mut line = format!(
        "id={} layer={} vertices={} tip={},{}",
        entity.id,
        field(&entity.layer),
        leader.vertices.len(),
        real(tip.x),
        real(tip.y)
    );
    if leader.tip_ref.associated() {
        line += &format!(
            " target={} anchor={}",
            leader.tip_ref.entity,
            field(&katana_entity::anchor::describe(&leader.tip_ref))
        );
    }
    line += &format!(" smart={}", if is_smart(leader) { "yes" } else { "no" });
    if leader.fields {
        line += &format!(" template={}", field(&leader.text));
        // A preset's template says which: what the window shows it as.
        if let Some(preset) = recognise_preset(&leader.text) {
            let info = preset_info(preset.preset);
            line += &format!(" preset={}", info.word);
            if !preset.reads.is_empty() {
                line += if info.reads == PresetReads::Surface {
                    " surface="
                } else {
                    " alignment="
                };
                line += &field(&preset.reads);
            }
        }
    }
    if !leader.label_style.is_empty() {
        line += &format!(" labelstyle={}", field(&leader.label_style));
    }
    line += &format!(
        " arrow={} callout={} text={}",
        leader.arrow.name(),
        leader.callout.name(),
        field(&leader_says(model, leader, context))
    );
    if let Some(angle) = leader.landing_angle {
        line += &format!(" landing_angle={}", real(angle));
    }
    line
}

// The ids `tokens` name, in id order and each once, restricted to leaders
// when `leaders_only` (the selection and the rest of the shared scope are
// choose() below).
fn ids_of(tokens: &[String], document: &Document, leaders_only: bool) -> Result<Vec<EntityId>> {
    let mut ids = Vec::new();
    for token in tokens {
        let id = id_of(token)?;
        if !document.model().entities.contains(id) {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not exist",
                format!("id={id}"),
            ));
        }
        ids.push(id);
    }
    if leaders_only {
        require_leaders(document.model(), &ids)?;
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

// ---- what a leader verb acts on: ids, or the shared scope -----------------------------------
//
// Every verb that reads or changes a set of drawing data takes the one scope
// and filter grammar (docs/cad.md; scope_verbs.rs):
//
//   [SELECTION | VIEW | DRAWING | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY]] [WHERE k=v ...]
//
// read by parse_scope_words and resolved by match_scope, beside ids, which
// stay the other way to name them. The verb's own key=value options stand
// where they like BEFORE a WHERE; after it a key that is a WHERE key (TYPE
// LAYER STYLE COLOUR PROP TEXT) is the condition and not the option - so
// style= and text= go before WHERE.

#[derive(Clone, Debug, Default)]
struct Subject {
    // The words before any scope word (ids; JSON for LIST) and the options.
    args: Arguments,
    // Set when a scope word or WHERE was given.
    scope: Option<ScopeWords>,
    // The words after the scope that the grammar did not take: a trailing
    // JSON, for the verb to read or refuse.
    after: Vec<String>,
}

// `tokens[from..to]` as a Subject; the options are those in `known`.
fn subject_of(tokens: &[String], from: usize, to: usize, known: &[&str]) -> Result<Subject> {
    let mut subject = Subject::default();
    let mut words: Vec<String> = Vec::new();
    let mut in_scope = false;
    let mut after_where = false;
    for token in &tokens[from..to] {
        let keyed = option_key(token);
        if upper_case(token) == "WHERE" {
            in_scope = true;
            after_where = true;
            words.push(token.clone());
        } else if after_where {
            // A condition is the grammar's - its key may be one of the verb's
            // options too - and a key it does not know is named by the
            // grammar as one that is no condition. Only an option that is no
            // WHERE key stays the verb's.
            match keyed {
                Some((key, equals)) if !is_where_key(&key) && known.contains(&key.as_str()) => {
                    subject
                        .args
                        .options
                        .insert(key, String::from(&token[equals + 1..]));
                }
                _ => words.push(token.clone()),
            }
        } else if let Some((key, equals)) = keyed {
            if !known.contains(&key.as_str()) {
                return Err(unknown_option(&key, known));
            }
            subject
                .args
                .options
                .insert(key, String::from(&token[equals + 1..]));
        } else if in_scope || is_scope_word(token) {
            in_scope = true;
            words.push(token.clone());
        } else {
            subject.args.positional.push(token.clone());
        }
    }
    if !words.is_empty() {
        let mut at = 0;
        subject.scope = Some(parse_scope_words(&words, &mut at)?);
        subject.after = words[at..].to_vec();
    }
    Ok(subject)
}

// What a verb acts on once its words are read.
#[derive(Clone, Debug, Default)]
struct Chosen {
    // Ascending, each once.
    ids: Vec<EntityId>,
    // The scope's record (scope_record) - how many it took, said with the
    // result - or empty when ids were named.
    record: String,
    scoped: bool,
}

// The ids `named` name or the scope of `subject` takes, as leaders when
// `leaders_only` (what a scope matched is narrowed to the leaders among it,
// so `matched=` counts them). Both is refused, and neither is the caller's to
// refuse with its own usage. A scope that takes nothing is no failure - the
// record says matched=0 - except for an EDIT of the selection with nothing
// suitable selected, which is the mistake MODIFY refuses too: it forgot to
// select.
fn choose(
    document: &Document,
    views: ScopeViewProvider<'_>,
    subject: &Subject,
    named: &[String],
    leaders_only: bool,
    edit: bool,
) -> Result<Chosen> {
    let Some(scope) = &subject.scope else {
        return Ok(Chosen {
            ids: ids_of(named, document, leaders_only)?,
            ..Chosen::default()
        });
    };
    if let Some(first) = named.first() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "name the entities by id or by a scope (SELECTION, VIEW, DRAWING, AREA, LAYERS ... \
             WHERE ...), not both",
            first.as_str(),
        ));
    }
    if let Some(first) = subject.after.first() {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "not a word that follows a scope and its WHERE conditions here",
            first.as_str(),
        ));
    }
    if edit && scope.source == ScopeSource::Selection {
        let model = document.model();
        let any = document.selection().ids().into_iter().any(|id| {
            model
                .entities
                .find(id)
                .is_some_and(|entity| !leaders_only || leader_of(entity).is_some())
        });
        if !any {
            return Err(Error::new(
                ErrorCode::InvalidState,
                if leaders_only {
                    "no leader is selected; use SELECT first"
                } else {
                    "nothing is selected; use SELECT first"
                },
            ));
        }
    }
    let found = if leaders_only {
        match_leaders(document, scope, views)?
    } else {
        match_scope(document, scope, views)?
    };
    Ok(Chosen {
        record: scope_record(&found),
        ids: found.matched,
        scoped: true,
    })
}

// `reply` led by the scope's record, when a scope chose what it is about.
fn with_scope(chosen: &Chosen, reply: &str) -> String {
    if !chosen.scoped {
        return String::from(reply);
    }
    if reply.is_empty() {
        chosen.record.clone()
    } else {
        format!("{}\n{reply}", chosen.record)
    }
}

// Runs `command` when there is one: an edit that changes nothing is no step.
fn run_optional(document: &mut Document, command: Option<CommandPtr>) -> Status {
    match command {
        Some(command) => document.execute(command),
        None => Ok(()),
    }
}

// ---- the verbs ------------------------------------------------------------------------

impl CommandInterpreter {
    pub(crate) fn is_annotation_verb(verb: &str, args: &[String]) -> bool {
        const VERBS: [&str; 9] = [
            "ANNOSCALE",
            "TEXTSTYLE",
            "MTEXT",
            "TEXTEDIT",
            "LABELSTYLE",
            "LABEL",
            "AUTOLABEL",
            "LEADER",
            "BALLOON",
        ];
        if VERBS.contains(&verb) {
            return true;
        }
        if verb == "TEXT" {
            // The plain TEXT p height "text" is the drawing verb's; with an
            // option it is a styled text.
            return args.iter().any(|token| option_key(token).is_some());
        }
        if verb == "DIM"
            && let Some(first) = args.first()
        {
            const KINDS: [&str; 11] = [
                "LINEAR",
                "HORIZONTAL",
                "VERTICAL",
                "ALIGNED",
                "ANGULAR",
                "RADIUS",
                "DIAMETER",
                "ORDINATE",
                "BASELINE",
                "CONTINUE",
                "LIST",
            ];
            return KINDS.contains(&upper_case(first).as_str());
        }
        false
    }

    pub(crate) fn annotation_help_text() -> String {
        String::from(ANNOTATION_HELP_TEXT)
    }

    pub(crate) fn annotation(
        &mut self,
        document: &mut Document,
        verb: &str,
        args: &[String],
    ) -> Result<String> {
        match verb {
            "ANNOSCALE" => annotation_scale(document, args),
            "TEXTSTYLE" => text_style(document, args),
            "TEXT" | "MTEXT" => self.styled_text(document, verb, args),
            "TEXTEDIT" => self.text_edit(document, args),
            "LABELSTYLE" => label_style(document, args),
            "LABEL" => self.label(document, args),
            "AUTOLABEL" => auto_label_verb(document, args),
            "DIM" => self.dimension(document, args),
            _ => self.leader(document, verb, args),
        }
    }

    // ---- points -------------------------------------------------------------------------

    /// A point that may name another entity's point - "#12", "#12.end",
    /// "#12.v3", "#12.s2", "#12.mid", "#12.centre", "#12.inside" - or the
    /// point of it nearest a point, "#12@x,y" (on it, Along) - or any point
    /// parse_point takes. A named point carries its reference, so what is
    /// made from it follows the entity (annotation/associative.rs).
    pub(crate) fn parse_anchored_point(
        &mut self,
        document: &Document,
        text: &str,
    ) -> Result<AnchoredPoint> {
        if !text.starts_with('#') {
            let point = self.parse_point(text)?;
            return Ok(AnchoredPoint {
                point,
                r#ref: AnchorRef::default(),
            });
        }
        let model = document.model();
        // "#12@x,y": the place on #12 nearest x,y, named so that it stays
        // there as #12 moves and stretches (entity's nearest_anchor). Split
        // before the '.' of "#12.end", since the point may have decimals.
        if let Some(at) = text.find('@') {
            let id = id_of(&text[..at])?;
            let Some(entity) = model.entities.find(id) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "entity does not exist",
                    text,
                ));
            };
            let near = self.parse_point(&text[at + 1..])?;
            let Some(reference) = katana_entity::anchor::nearest_anchor(entity, near) else {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    if matches!(entity.geometry, Geometry::Spline(_)) {
                        "a spline has no place along it to attach to; use #id.start or #id.end"
                    } else {
                        "that entity has no place on it to attach to"
                    },
                    text,
                ));
            };
            let anchored = anchored_point(model, &reference);
            if let Ok(anchored) = &anchored {
                self.last_point = Some(anchored.point);
            }
            return anchored;
        }
        let dot = text.find('.');
        let id = id_of(&text[..dot.unwrap_or(text.len())])?;
        let Some(entity) = model.entities.find(id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not exist",
                text,
            ));
        };
        let mut reference = AnchorRef {
            entity: id,
            point: default_anchor(&entity.geometry),
            index: 0,
            ..AnchorRef::default()
        };
        if let Some(dot) = dot {
            let part = lower_case(&text[dot + 1..]);
            if let Some(spec) = part.strip_prefix("along") {
                // "#12.along1:0.25": a quarter of the way along segment 1 of
                // #12 (0 on a line, an arc or a circle), exactly - what LIST
                // prints as "along 1 0.25", and what the window's Along sets;
                // "#12@x,y" names only the place nearest a point.
                let mut index: u32 = 0;
                let mut fraction: Option<f64> = None;
                if let Some(colon) = spec.find(':')
                    && let Some(segment) = unsigned_of(&spec[..colon])
                {
                    index = segment;
                    fraction = parse_finite_double(&spec[colon + 1..]);
                }
                let Some(fraction) = fraction.filter(|&f| !(f < 0.0) && !(f > 1.0)) else {
                    return Err(make_error(
                        ErrorCode::ParseFailure,
                        "along is #id.along<segment>:<fraction>, the fraction from 0 to 1 \
                         (#12.along0:0.5 is the middle of a line)",
                        text,
                    ));
                };
                if index != 0
                    && !matches!(
                        entity.geometry,
                        Geometry::Polyline(_) | Geometry::CurvePolyline(_)
                    )
                {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "only a polyline has segments to number: along0:t on a line, an arc, a \
                         circle or an ellipse",
                        text,
                    ));
                }
                if matches!(entity.geometry, Geometry::Spline(_)) {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "a spline has no place along it to name; use #id.start, #id.end or \
                         #id@x,y",
                        text,
                    ));
                }
                reference.point = AnchorPoint::Along;
                reference.index = index;
                // A circle's turn is [0, 1), as nearest_anchor names it: a
                // whole turn is where it starts - and a whole ellipse's.
                let whole_turn = match &entity.geometry {
                    Geometry::Circle(_) => true,
                    Geometry::Ellipse(ellipse) => ellipse.is_full(),
                    _ => false,
                };
                reference.parameter = if whole_turn && fraction == 1.0 {
                    0.0
                } else {
                    fraction
                };
            } else if let Some(index) = indexed(&part, "vertex").or_else(|| indexed(&part, "v")) {
                reference.point = AnchorPoint::Vertex;
                reference.index = index;
            } else if let Some(index) = indexed(&part, "segment").or_else(|| indexed(&part, "s")) {
                reference.point = AnchorPoint::SegmentMid;
                reference.index = index;
            } else {
                let Ok(point) = anchor_point_from_string(&part) else {
                    return Err(make_error(
                        ErrorCode::ParseFailure,
                        "unknown point of an entity (start end mid centre position inside vN \
                         sN alongN:T, or #id@x,y for the nearest point on it)",
                        text,
                    ));
                };
                reference.point = point;
            }
        }
        let anchored = anchored_point(model, &reference);
        if let Ok(anchored) = &anchored {
            self.last_point = Some(anchored.point);
        }
        anchored
    }

    // ---- TEXT, MTEXT, TEXTEDIT ---------------------------------------------------------

    fn styled_text(
        &mut self,
        document: &mut Document,
        verb: &str,
        args: &[String],
    ) -> Result<String> {
        let parsed = split_arguments(args, 0, TEXT_KEYS)?;
        let mtext = verb == "MTEXT";
        let positional = &parsed.positional;
        if positional.len() < 2 || positional.len() > 3 || (mtext && positional.len() != 2) {
            return Err(usage_of(if mtext {
                "MTEXT p \"text\" [style= paper= justify= rotation=]"
            } else {
                "TEXT p [height] \"text\" [style= paper= justify= rotation=]"
            }));
        }
        let at = self.parse_point(&positional[0])?;
        let mut text = TextGeometry {
            position: at,
            text: unescape(&positional[positional.len() - 1]),
            ..TextGeometry::default()
        };
        if positional.len() == 3 {
            text.height = number_of(&positional[1], "the height")?;
        } else {
            // No height given: a paper-sized text in the Standard style,
            // unless the options say otherwise.
            text.style = String::from(DEFAULT_TEXT_STYLE_NAME);
        }
        apply_text_options(
            &parsed,
            document.model(),
            document.annotation_scale(),
            &mut text,
        )?;
        let attributes = document.current_attributes();
        document.execute(create_text(text.clone(), attributes))?;
        let created = document.last_created_entities();
        Ok(format!(
            "created text id={} style={} paper={} justify={} height={}",
            created
                .first()
                .map_or_else(|| String::from("?"), |id| format!("{id}")),
            field(&text.style),
            real(text.paper_height),
            text.justify.name(),
            real(text.height)
        ))
    }

    fn text_edit(&mut self, document: &mut Document, args: &[String]) -> Result<String> {
        let parsed = split_arguments(args, 0, TEXT_KEYS)?;
        if parsed.positional.len() != 1 || parsed.options.is_empty() {
            return Err(usage_of(
                "TEXTEDIT id [text= style= paper= justify= rotation= height= at=x,y]",
            ));
        }
        let id = id_of(&parsed.positional[0])?;
        let current = match document
            .model()
            .entities
            .find(id)
            .map(|entity| &entity.geometry)
        {
            Some(Geometry::Text(text)) => text.clone(),
            _ => {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "that entity is not a text",
                    format!("id={id}"),
                ));
            }
        };
        let mut text = current;
        if let Some(at) = parsed.find("at") {
            text.position = self.parse_point(at)?;
        }
        apply_text_options(
            &parsed,
            document.model(),
            document.annotation_scale(),
            &mut text,
        )?;
        finish(
            document.execute(set_entity_geometry(id, Geometry::Text(text))),
            format!("updated text id={id}"),
        )
    }

    // ---- LABEL ------------------------------------------------------------------------

    fn label(&mut self, document: &mut Document, args: &[String]) -> Result<String> {
        let action = args
            .first()
            .map(|word| upper_case(word))
            .unwrap_or_default();
        if action == "LIST" {
            let mut only = BTreeSet::new();
            for word in &args[1..] {
                only.insert(id_of(word)?);
            }
            let model = document.model();
            let mut lines: Vec<String> = Vec::new();
            model.entities.for_each(|entity| {
                let Geometry::Label(label) = &entity.geometry else {
                    return;
                };
                if !only.is_empty() && !only.contains(&entity.id) {
                    return;
                }
                let mut line = format!("id={} style={}", entity.id, field(&label.style));
                if label.target != 0 {
                    line += &format!(" target={}", label.target);
                } else {
                    line += &format!(" alignment={}", field(&label.alignment));
                }
                line += &format!(
                    " part={} layer={} rule={}",
                    label.part,
                    field(&entity.layer),
                    field(&label.rule)
                );
                if let Some(position) = label.position {
                    line += &format!(" at={},{}", real(position.x), real(position.y));
                }
                line += &format!(" text={}", field(&shown_label_text(model, label)));
                lines.push(line);
            });
            return Ok(lines.join("\n"));
        }
        if action == "DELETE" {
            let mut ids = Vec::new();
            for word in &args[1..] {
                let id = id_of(word)?;
                let is_label = document
                    .model()
                    .entities
                    .find(id)
                    .is_some_and(|entity| matches!(entity.geometry, Geometry::Label(_)));
                if !is_label {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "that entity is not a label",
                        format!("id={id}"),
                    ));
                }
                ids.push(id);
            }
            if ids.is_empty() {
                return Err(usage_of("LABEL DELETE id [id...]"));
            }
            let count = ids.len();
            return finish(
                document.execute(delete_entities(ids)),
                format!("deleted labels={count}"),
            );
        }
        if action == "LAYOUT" {
            let parsed = split_arguments(args, 1, &["scale", "collisions"])?;
            let mut options = LabelLayoutOptions {
                scale: document.annotation_scale(),
                ..LabelLayoutOptions::default()
            };
            if let Some(scale) = parsed.find("scale") {
                options.scale = number_of(scale, "scale")?;
            }
            if let Some(collisions) = parsed.find("collisions") {
                options.avoid_collisions = switch_of(collisions, "collisions")?;
            }
            let model = document.model();
            // What the plan view keeps labels out of, over the whole drawing,
            // so this reply is where a view at this scale puts them.
            if options.avoid_collisions {
                let linework = label_keep_out(
                    model,
                    options.scale,
                    &options.measure,
                    LAYOUT_LINEWORK_LIMIT,
                    &leader_context(document),
                );
                options.linework = linework;
            }
            let layout = layout_labels(model, &label_entities(model), &options);
            let mut out = format!(
                "scale={} considered={} placed={} displaced={} suppressed={} orphaned={}",
                real(options.scale),
                layout.considered,
                layout.placed.len(),
                layout.displaced,
                layout.suppressed,
                layout.orphaned
            );
            for placed in &layout.placed {
                let mut extent = Box2::default();
                for corners in &placed.drawing.text_boxes {
                    for &p in corners {
                        extent.expand(p);
                    }
                }
                let runs: Vec<&str> = placed
                    .drawing
                    .texts
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect();
                let centre = if extent.empty() {
                    Point2::default()
                } else {
                    extent.center()
                };
                out += &format!(
                    "\nlabel={} piece={} x={} y={} candidate={} displaced={} text={}",
                    placed.label,
                    placed.piece,
                    real(centre.x),
                    real(centre.y),
                    placed.candidate,
                    if placed.displaced { "yes" } else { "no" },
                    field(&runs.join("\n"))
                );
            }
            // Then each piece that found no room, so it can be selected and
            // given some.
            for piece in &layout.suppressed_pieces {
                out += &format!(
                    "\nlabel={} piece={} suppressed=yes",
                    piece.label, piece.piece
                );
            }
            return Ok(out);
        }
        if action == "SET" {
            let parsed = split_arguments(args, 2, &["style", "at", "text", "layer"])?;
            if args.len() < 3 {
                return Err(usage_of(
                    "LABEL SET id [style= at=x,y|none text=|none layer=]",
                ));
            }
            let id = id_of(&args[1])?;
            let found = document
                .model()
                .entities
                .find(id)
                .filter(|entity| matches!(entity.geometry, Geometry::Label(_)))
                .cloned();
            let Some(mut changed) = found else {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "that entity is not a label",
                    format!("id={id}"),
                ));
            };
            if let Geometry::Label(label) = &mut changed.geometry {
                if let Some(style) = parsed.find("style") {
                    label.style = style.clone();
                }
                if let Some(text) = parsed.find("text") {
                    label.text_override = if lower_case(text) == "none" {
                        String::new()
                    } else {
                        unescape(text)
                    };
                }
                if let Some(at) = parsed.find("at") {
                    label.position = if lower_case(at) == "none" {
                        None
                    } else {
                        Some(self.parse_point(at)?)
                    };
                }
            }
            if let Some(layer) = parsed.find("layer") {
                changed.layer = layer.clone();
            }
            let changes = ChangeSet {
                modify: vec![changed],
                ..ChangeSet::default()
            };
            return finish(
                document.execute(Box::new(ChangeSetCommand::new(
                    "SET_LABEL",
                    move |_context| Ok(changes.clone()),
                    false,
                ))),
                format!("updated label id={id}"),
            );
        }

        // Making labels: LABEL id... | LABEL SELECTION | LABEL ALIGN name.
        let parsed = split_arguments(args, 0, &["style", "part", "layer", "at", "text"])?;
        let Some(style) = parsed
            .find("style")
            .filter(|_| !parsed.positional.is_empty())
        else {
            return Err(usage_of(
                "LABEL id [id...] style= [part=N layer= at=x,y text=] | LABEL SELECTION style= | \
                 LABEL ALIGN name style=",
            ));
        };
        let mut request = LabelRequest {
            style: style.clone(),
            ..LabelRequest::default()
        };
        if let Some(layer) = parsed.find("layer") {
            request.layer = layer.clone();
        }
        if let Some(text) = parsed.find("text") {
            request.text_override = unescape(text);
        }
        if let Some(part) = parsed.find("part") {
            request.part = integer_of(part, "part")?;
        }
        if let Some(at) = parsed.find("at") {
            request.position = Some(self.parse_point(at)?);
        }
        let mut requests = Vec::new();
        let first = upper_case(&parsed.positional[0]);
        if first == "ALIGN" {
            if parsed.positional.len() != 2 {
                return Err(usage_of("LABEL ALIGN name style="));
            }
            request.alignment = parsed.positional[1].clone();
            requests.push(request);
        } else if first == "SELECTION" {
            for id in document.selection().ids() {
                requests.push(LabelRequest {
                    target: id,
                    ..request.clone()
                });
            }
            if requests.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidState,
                    "nothing is selected; use SELECT first",
                ));
            }
        } else {
            for token in &parsed.positional {
                requests.push(LabelRequest {
                    target: id_of(token)?,
                    ..request.clone()
                });
            }
        }
        // Every label as one step: one UNDO takes back the lot. The Label
        // Objects tool makes its labels through the same door.
        let built = create_labels(document.model(), &requests)?;
        document.execute(built)?;
        let created = document.last_created_entities();
        Ok(format!(
            "created labels={} ids={}",
            created.len(),
            id_list(&created)
        ))
    }

    // ---- DIM ----------------------------------------------------------------------------

    fn dimension(&mut self, document: &mut Document, args: &[String]) -> Result<String> {
        let kind = upper_case(&args[0]);
        let parsed = split_arguments(args, 1, &["at", "angle", "datum", "axis", "spacing"])?;
        let positional = &parsed.positional;

        let at = match parsed.find("at") {
            Some(text) => Some(self.parse_anchored_point(document, text)?.point),
            None => None,
        };
        let points = |from: usize, interpreter: &mut Self| -> Result<Vec<AnchoredPoint>> {
            let mut list = Vec::new();
            for word in &positional[from..] {
                list.push(interpreter.parse_anchored_point(document, word)?);
            }
            Ok(list)
        };

        match kind.as_str() {
            "LINEAR" | "HORIZONTAL" | "VERTICAL" | "ALIGNED" => {
                let list = points(0, self)?;
                let Some(at) = at.filter(|_| list.len() == 2) else {
                    return Err(usage_of(&format!(
                        "DIM {kind} p p at=p{}",
                        if kind == "LINEAR" { " [angle=deg]" } else { "" }
                    )));
                };
                let angle = match kind.as_str() {
                    "HORIZONTAL" => Some(0.0),
                    "VERTICAL" => Some(0.5 * PI),
                    _ => match parsed.find("angle") {
                        Some(text) => Some(number_of(text, "angle")? * DEG_TO_RAD),
                        None => None,
                    },
                };
                let made = if kind == "ALIGNED" {
                    aligned_dimension(&list[0], &list[1], at)?
                } else {
                    linear_dimension(&list[0], &list[1], at, angle)?
                };
                created_dimension(document, &made)
            }
            "ANGULAR" => {
                // Two ids: the angle between two lines.
                if positional.len() == 2 {
                    let first = id_of(&positional[0]);
                    let second = id_of(&positional[1]);
                    let (Ok(first), Ok(second), Some(at)) = (first, second, at) else {
                        return Err(usage_of("DIM ANGULAR line-id line-id at=p"));
                    };
                    let made = angular_between_lines(document.model(), first, second, at)?;
                    return created_dimension(document, &made);
                }
                let list = points(0, self)?;
                if list.len() != 3 {
                    return Err(usage_of("DIM ANGULAR vertex p p [at=p]"));
                }
                let made = angular_dimension(&list[0], &list[1], &list[2], at)?;
                created_dimension(document, &made)
            }
            "RADIUS" | "DIAMETER" => {
                if positional.len() != 1 {
                    return Err(usage_of(&format!("DIM {kind} circle-id [at=p]")));
                }
                let id = id_of(&positional[0])?;
                let through = match at {
                    Some(at) => at,
                    None => {
                        // Up and to the right of the curve, a third of its
                        // radius out.
                        let Some(entity) = document.model().entities.find(id) else {
                            return Err(make_error(
                                ErrorCode::NotFound,
                                "entity does not exist",
                                format!("id={id}"),
                            ));
                        };
                        match &entity.geometry {
                            Geometry::Circle(circle) => {
                                circle.center
                                    + Vec2::new(1.0, 1.0).normalized() * (circle.radius * 1.33)
                            }
                            Geometry::Arc(arc) => {
                                arc.center + (arc.point_at(0.5) - arc.center) * 1.33
                            }
                            _ => Point2::default(),
                        }
                    }
                };
                let made = radial_dimension(document.model(), id, through, kind == "DIAMETER")?;
                created_dimension(document, &made)
            }
            "ORDINATE" => {
                let list = points(0, self)?;
                let Some(at) = at.filter(|_| list.len() == 1) else {
                    return Err(usage_of("DIM ORDINATE p at=p [datum=p axis=x|y]"));
                };
                let datum = match parsed.find("datum") {
                    Some(text) => self.parse_anchored_point(document, text)?,
                    None => AnchoredPoint {
                        point: Point2::new(0.0, 0.0),
                        r#ref: AnchorRef::default(),
                    },
                };
                let x_axis = match parsed.find("axis") {
                    Some(axis) => {
                        let a = lower_case(axis);
                        if a != "x" && a != "y" {
                            return Err(usage_of("DIM ORDINATE ... axis=x|y"));
                        }
                        Some(a == "x")
                    }
                    None => None,
                };
                let made = ordinate_dimension(&datum, &list[0], at, x_axis)?;
                created_dimension(document, &made)
            }
            "BASELINE" | "CONTINUE" => {
                if positional.len() < 2 {
                    return Err(usage_of(&format!(
                        "DIM {kind} dim-id p [p...]{}",
                        if kind == "BASELINE" {
                            " [spacing=]"
                        } else {
                            ""
                        }
                    )));
                }
                let id = id_of(&positional[0])?;
                let list = points(1, self)?;
                // A text height and a half apart unless told (baseline_spacing).
                let spacing = match parsed.find("spacing") {
                    Some(text) if kind == "BASELINE" => Some(number_of(text, "spacing")?),
                    _ => None,
                };
                // The whole chain as one step, on the base's layer and style -
                // the Baseline and Continue Dimension tools' door too.
                let built = dimension_chain(
                    document.model(),
                    id,
                    &list,
                    if kind == "BASELINE" {
                        DimensionChain::Baseline
                    } else {
                        DimensionChain::Continued
                    },
                    spacing,
                    document.annotation_scale(),
                )?;
                document.execute(built)?;
                let ids = document.last_created_entities();
                Ok(format!(
                    "created dimensions={} ids={}",
                    ids.len(),
                    id_list(&ids)
                ))
            }
            _ => Err(usage_of(
                "DIM LINEAR|HORIZONTAL|VERTICAL|ALIGNED|ANGULAR|RADIUS|DIAMETER|ORDINATE|\
                 BASELINE|CONTINUE ...",
            )),
        }
    }

    // ---- LEADER, BALLOON ----------------------------------------------------------------

    fn leader(&mut self, document: &mut Document, verb: &str, args: &[String]) -> Result<String> {
        let balloon = verb == "BALLOON";
        let action = args
            .first()
            .map(|word| upper_case(word))
            .unwrap_or_default();
        // The drawing's surfaces and alignments, which a note may read: no
        // verb here changes them, so one made now serves the whole line.
        let context = leader_context(document);

        if !balloon && action == "PRESETS" {
            if args.len() != 1 {
                return Err(usage_of("LEADER PRESETS"));
            }
            let lines: Vec<String> = leader_presets()
                .iter()
                .map(|info| {
                    format!(
                        "preset={} title={} reads={} template={} tool={}",
                        info.word,
                        field(info.title),
                        match info.reads {
                            PresetReads::Target => "target",
                            PresetReads::Surface => "surface",
                            PresetReads::Alignment => "alignment",
                        },
                        field(info.pattern),
                        if info.tool.is_empty() {
                            "none"
                        } else {
                            info.tool
                        }
                    )
                })
                .collect();
            return Ok(lines.join("\n"));
        }

        if !balloon && action == "LIST" {
            let mut subject = subject_of(args, 1, args.len(), &["target"])?;
            let mut json = false;
            let mut named = Vec::new();
            for token in &subject.args.positional {
                if upper_case(token) == "JSON" {
                    json = true;
                } else {
                    named.push(token.clone());
                }
            }
            // JSON may stand after the scope too: LEADER LIST LAYERS notes
            // JSON.
            if subject.scope.is_some()
                && subject.after.len() == 1
                && upper_case(&subject.after[0]) == "JSON"
            {
                json = true;
                subject.after.clear();
            }
            let target = match subject.args.find("target") {
                Some(value) => Some(id_of(value)?),
                None => None,
            };
            // Not every leader but those named or those the scope took - and
            // none, not all, when the scope took none. Ids are only looked at
            // as ids: one that is no leader lists nothing, as it always did.
            let mut limit: Option<Vec<EntityId>> = None;
            let mut picked = Chosen::default();
            if subject.scope.is_some() {
                picked = choose(document, self.views(), &subject, &named, true, false)?;
                limit = Some(picked.ids.clone());
            } else if !named.is_empty() {
                let mut ids = Vec::new();
                for token in &named {
                    ids.push(id_of(token)?);
                }
                ids.sort_unstable();
                ids.dedup();
                limit = Some(ids);
            }
            if json {
                let mut report = leaders_report(document, limit.as_deref(), target);
                if picked.scoped {
                    report.scope = Some(LeadersScopeReport {
                        record: picked.record.clone(),
                        matched: picked.ids.len(),
                    });
                }
                return Ok(leaders_json_of(&report));
            }
            let model = document.model();
            let mut lines: Vec<String> = Vec::new();
            model.entities.for_each(|entity| {
                let Some(leader) = leader_of(entity) else {
                    return;
                };
                if limit
                    .as_ref()
                    .is_some_and(|limit| limit.binary_search(&entity.id).is_err())
                    || target.is_some_and(|target| leader.tip_ref.entity != target)
                {
                    return;
                }
                lines.push(describe_leader(model, entity, &context));
            });
            return Ok(with_scope(&picked, &lines.join("\n")));
        }

        if !balloon && action == "VALUES" && args.len() >= 2 && is_scope_word(&args[1]) {
            // Every value a note could name, for each leader a scope takes
            // whose tip is on an entity: the block LEADER VALUES id prints,
            // for each.
            let subject = subject_of(args, 1, args.len(), &[])?;
            let mut picked = choose(
                document,
                self.views(),
                &subject,
                &subject.args.positional,
                true,
                false,
            )?;
            let model = document.model();
            let mut blocks = String::new();
            let mut skipped = 0;
            for &id in &picked.ids {
                let Some(leader) = model.entities.find(id).and_then(leader_of) else {
                    continue;
                };
                if !leader.tip_ref.associated() {
                    skipped += 1;
                    continue;
                }
                let Some(read) = leader_target_values(model, leader, &context) else {
                    skipped += 1;
                    continue;
                };
                if !blocks.is_empty() {
                    blocks.push('\n');
                }
                blocks += &format!(
                    "leader={id} target={} type={}",
                    leader.tip_ref.entity,
                    read.get("type")
                        .map(|value| value.text.as_str())
                        .unwrap_or("")
                );
                for row in leader_value_rows(&read) {
                    blocks += &format!("\nvalue={} text={}", field(&row.name), field(&row.text));
                }
            }
            // Those with a tip on nothing have no values to list: counted,
            // said.
            picked.record += &format!(" skipped={skipped}");
            return Ok(with_scope(&picked, &blocks));
        }

        if !balloon && action == "VALUES" {
            if args.len() != 2 || args[1].is_empty() {
                return Err(usage_of(
                    "LEADER VALUES id | #id[.point] | #id@x,y | <scope>",
                ));
            }
            let named_leader =
                id_of(&args[1]).ok().and_then(|id| {
                    document.model().entities.find(id).and_then(|entity| {
                        leader_of(entity).map(|leader| (entity.id, leader.clone()))
                    })
                });
            let (values, mut out) = if let Some((id, leader)) = named_leader {
                if !leader.tip_ref.associated() {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "that leader's tip is on no entity (LEADER ATTACH puts it on one)",
                        format!("id={id}"),
                    ));
                }
                let Some(read) = leader_target_values(document.model(), &leader, &context) else {
                    return Err(make_error(
                        ErrorCode::NotFound,
                        "the entity the leader's tip is on is gone",
                        format!("id={}", leader.tip_ref.entity),
                    ));
                };
                (
                    read,
                    format!("leader={id} target={}", leader.tip_ref.entity),
                )
            } else {
                // A place on an entity: what a leader there could say.
                let word = if args[1].starts_with('#') {
                    args[1].clone()
                } else {
                    format!("#{}", args[1])
                };
                let place = self.parse_anchored_point(document, &word)?;
                let Some(target) = document.model().entities.find(place.r#ref.entity) else {
                    return Err(make_error(
                        ErrorCode::NotFound,
                        "entity does not exist",
                        word,
                    ));
                };
                (
                    anchor_values(
                        target,
                        &place.r#ref,
                        place.point,
                        code_properties(),
                        &context,
                    ),
                    format!("target={}", target.id),
                )
            };
            out += &format!(
                " type={}",
                values
                    .get("type")
                    .map(|value| value.text.as_str())
                    .unwrap_or("")
            );
            for row in leader_value_rows(&values) {
                out += &format!("\nvalue={} text={}", field(&row.name), field(&row.text));
            }
            return Ok(out);
        }

        if !balloon && action == "SET" {
            let subject = subject_of(
                args,
                1,
                args.len(),
                &[
                    "text",
                    "template",
                    "labelstyle",
                    "preset",
                    "surface",
                    "alignment",
                    "arrow",
                    "callout",
                    "style",
                    "paper",
                    "arrowsize",
                    "landing",
                    "landingangle",
                    "tip",
                    "at",
                    "vertex",
                ],
            )?;
            let parsed = &subject.args;
            if (parsed.positional.is_empty() && subject.scope.is_none())
                || parsed.options.is_empty()
            {
                return Err(usage_of(
                    "LEADER SET id [id...] | <scope> [text= template= labelstyle= preset= \
                     surface=name alignment=name arrow= callout= style= paper= arrowsize= \
                     landing= landingangle=auto|deg tip=p at=p [vertex=N]] (zero-based vertex; \
                     options before WHERE)",
                ));
            }
            let picked = choose(
                document,
                self.views(),
                &subject,
                &parsed.positional,
                true,
                true,
            )?;
            let mut change = leader_change_of(parsed)?;
            let preset = preset_of(parsed, document, &mut change)?;
            if let Some(value) = parsed.find("tip") {
                change.tip = Some(self.parse_anchored_point(document, value)?);
            }
            if let Some(value) = parsed.find("at") {
                change.hang = Some(self.parse_point(value)?);
            }
            if let Some(value) = parsed.find("vertex") {
                let Some(hang) = change.hang else {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        "vertex= needs at=x,y",
                    ));
                };
                let index = integer_of(value, "vertex")?;
                if index < 0 {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        "vertex= is a zero-based index",
                    ));
                }
                change.vertex = Some(LeaderVertexMove {
                    index: index as usize,
                    position: hang,
                });
                change.hang = None;
            }
            if picked.ids.is_empty() {
                // A scope that took no leader: said, not refused.
                return Ok(with_scope(
                    &picked,
                    &format!("updated leaders=0{}", preset_reply(preset.as_ref())),
                ));
            }
            let command = change_leaders(document.model(), picked.ids.clone(), &change, &context)?;
            run_optional(document, command)?;
            let model = document.model();
            let mut out = format!(
                "updated leaders={}{}",
                picked.ids.len(),
                preset_reply(preset.as_ref())
            );
            for &id in &picked.ids {
                if let Some(entity) = model.entities.find(id) {
                    out += &format!("\n{}", describe_leader(model, entity, &context));
                }
            }
            return Ok(with_scope(&picked, &out));
        }

        if !balloon && action == "ATTACH" {
            if args.len() != 3 {
                return Err(usage_of("LEADER ATTACH id #id[.point] | #id@x,y"));
            }
            let ids = ids_of(&args[1..2], document, true)?;
            let place = self.parse_anchored_point(document, &args[2])?;
            let command =
                attach_leader(document.model(), ids[0], &place, &context).map_err(|error| {
                    let context = if error.context.is_empty() {
                        args[2].clone()
                    } else {
                        error.context.clone()
                    };
                    make_error(error.code, error.message, context)
                })?;
            run_optional(document, command)?;
            let model = document.model();
            let described = model
                .entities
                .find(ids[0])
                .map(|entity| describe_leader(model, entity, &context))
                .unwrap_or_default();
            return Ok(format!("attached leader {described}"));
        }

        if !balloon && (action == "DETACH" || action == "FREEZE") {
            let detach = action == "DETACH";
            let subject = subject_of(args, 1, args.len(), &[])?;
            if subject.args.positional.is_empty() && subject.scope.is_none() {
                return Err(usage_of(if detach {
                    "LEADER DETACH id [id...] | <scope>"
                } else {
                    "LEADER FREEZE id [id...] | <scope>"
                }));
            }
            let picked = choose(
                document,
                self.views(),
                &subject,
                &subject.args.positional,
                true,
                true,
            )?;
            if picked.ids.is_empty() {
                return Ok(with_scope(
                    &picked,
                    if detach {
                        "detached leaders=0 frozen=0"
                    } else {
                        "frozen leaders=0"
                    },
                ));
            }
            let release = release_leaders(document.model(), picked.ids.clone(), detach, &context)?;
            let (changed, frozen) = (release.changed, release.frozen);
            run_optional(document, release.command)?;
            return Ok(with_scope(
                &picked,
                &if detach {
                    format!("detached leaders={changed} frozen={frozen}")
                } else {
                    format!("frozen leaders={frozen}")
                },
            ));
        }

        if !balloon && action == "PROP" {
            const PROP_USAGE: &str = "LEADER PROP id | <scope> SET key value \
                                      [text|integer|real|boolean] | LEADER PROP id | <scope> \
                                      DELETE key";
            // SET or DELETE ends the subject: the scope's words, which may
            // hold WHERE conditions, stand before it. The word after LAYERS is
            // a layer list, even if a layer is called set.
            let mut at = 0;
            for i in 2..args.len() {
                let before = upper_case(&args[i - 1]);
                let word = upper_case(&args[i]);
                if (word == "SET" || word == "DELETE") && before != "LAYERS" && before != "LAYER" {
                    at = i;
                    break;
                }
            }
            if at == 0 || args.len() < at + 2 {
                return Err(usage_of(PROP_USAGE));
            }
            let subject = subject_of(args, 1, at, &[])?;
            if subject.args.positional.is_empty() && subject.scope.is_none() {
                return Err(usage_of(PROP_USAGE));
            }
            let mut picked = choose(
                document,
                self.views(),
                &subject,
                &subject.args.positional,
                true,
                true,
            )?;
            let sub = upper_case(&args[at]);
            let key = &args[at + 1];
            let mut value: Option<PropertyValue> = None;
            if sub == "SET" {
                // key value [type]
                if args.len() < at + 3 || args.len() > at + 4 {
                    return Err(usage_of(PROP_USAGE));
                }
                value = Some(Self::property_value(
                    &args[at + 2],
                    args.get(at + 3).map(String::as_str),
                )?);
            } else if args.len() != at + 2 {
                return Err(usage_of(PROP_USAGE));
            }
            if picked.ids.is_empty() {
                return Ok(with_scope(
                    &picked,
                    &format!("property={} leaders=0", field(key)),
                ));
            }
            let set = set_leaders_target_property(
                document.model(),
                picked.ids.clone(),
                key,
                value.as_ref(),
            )?;
            let (entities, skipped) = (set.entities, set.skipped);
            document.execute(set.command)?;
            let model = document.model();
            let mut lines: Vec<String> = Vec::new();
            for &id in &picked.ids {
                let Some(leader) = model.entities.find(id).and_then(leader_of) else {
                    continue;
                };
                if !leader.tip_ref.associated() || !model.entities.contains(leader.tip_ref.entity) {
                    continue; // skipped: it had no entity to set the attribute on
                }
                lines.push(format!(
                    "leader={id} target={} property={} text={}",
                    leader.tip_ref.entity,
                    field(key),
                    field(&leader_says(model, leader, &context))
                ));
            }
            if picked.scoped {
                picked.record += &format!(" entities={entities} skipped={skipped}");
            }
            return Ok(with_scope(&picked, &lines.join("\n")));
        }

        if !balloon && action == "ALIGN" {
            let subject = subject_of(args, 1, args.len(), &["x", "spacing"])?;
            let parsed = &subject.args;
            if parsed.positional.is_empty() && subject.scope.is_none() {
                return Err(usage_of(
                    "LEADER ALIGN id id [id...] | <scope> [x=] [spacing=mm]",
                ));
            }
            let picked = choose(
                document,
                self.views(),
                &subject,
                &parsed.positional,
                true,
                true,
            )?;
            let x = match parsed.find("x") {
                Some(value) => Some(number_of(value, "x")?),
                None => None,
            };
            let spacing = match parsed.find("spacing") {
                Some(value) => {
                    let number = number_of(value, "spacing")?;
                    if !(number > 0.0) {
                        return Err(make_error(
                            ErrorCode::InvalidArgument,
                            "spacing= must be more than zero",
                            value.as_str(),
                        ));
                    }
                    Some(annotation_model_size(number, document.annotation_scale()))
                }
                None => None,
            };
            if picked.ids.is_empty() {
                return Ok(with_scope(&picked, "aligned leaders=0"));
            }
            let alignment = align_leaders(document.model(), picked.ids.clone(), x, spacing)?;
            let (count, aligned_x) = (alignment.count, alignment.x);
            run_optional(document, alignment.command)?;
            return Ok(with_scope(
                &picked,
                &format!(
                    "aligned leaders={count} x={}{}",
                    real(aligned_x),
                    spacing
                        .map_or_else(String::new, |spacing| format!(" spacing={}", real(spacing)))
                ),
            ));
        }

        if balloon && action == "RENUMBER" {
            let subject = subject_of(args, 1, args.len(), &["start", "order"])?;
            let parsed = &subject.args;
            if !parsed.positional.is_empty() {
                return Err(usage_of(
                    "BALLOON RENUMBER [<scope>] [start=1] [order=id|x|y]",
                ));
            }
            let start = match parsed.find("start") {
                Some(value) => integer_of(value, "start")?,
                None => 1,
            };
            let order = parsed
                .find("order")
                .map_or_else(|| String::from("id"), |order| lower_case(order));
            if order != "id" && order != "x" && order != "y" {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "order= is id, x or y",
                    order,
                ));
            }
            // Every numbered balloon in the drawing, as it always was, unless
            // a scope limits it to some: those it takes are numbered from
            // `start` and the others keep their numbers.
            let mut picked = Chosen::default();
            let mut within: Option<Vec<EntityId>> = None;
            if subject.scope.is_some() {
                picked = choose(document, self.views(), &subject, &[], true, true)?;
                within = Some(picked.ids.clone());
            }
            let renumbering = renumber_balloons(
                document.model(),
                start,
                match order.as_str() {
                    "x" => BalloonOrder::X,
                    "y" => BalloonOrder::Y,
                    _ => BalloonOrder::Id,
                },
                within.as_deref(),
            );
            let (balloons, renumbered) = (renumbering.balloons, renumbering.renumbered);
            run_optional(document, renumbering.command)?;
            return Ok(with_scope(
                &picked,
                &format!("balloons={balloons} renumbered={renumbered}"),
            ));
        }

        if action == "FOR" {
            let subject = subject_of(
                args,
                1,
                args.len(),
                &[
                    "text",
                    "template",
                    "labelstyle",
                    "n",
                    "preset",
                    "surface",
                    "alignment",
                    "arrow",
                    "callout",
                    "style",
                    "paper",
                    "arrowsize",
                    "landing",
                    "landingangle",
                    "angle",
                    "length",
                ],
            )?;
            let parsed = &subject.args;
            if parsed.positional.is_empty() && subject.scope.is_none() {
                return Err(usage_of(if balloon {
                    "BALLOON FOR id [id...] | <scope> [n= template= labelstyle= angle=deg \
                     length=mm ...]   (options before WHERE)"
                } else {
                    "LEADER FOR id [id...] | <scope> [text= template= labelstyle= preset= \
                     angle=deg length=mm ...]   (options before WHERE)"
                }));
            }
            // The entities the leaders are made for: named, or what the scope
            // and filter take - of any kind, those with no place for a leader
            // counted in skipped=.
            let picked = choose(
                document,
                self.views(),
                &subject,
                &parsed.positional,
                false,
                true,
            )?;
            let mut change = leader_change_of(parsed)?;
            let preset = preset_of(parsed, document, &mut change)?;
            let mut options = LeadersForOptions {
                change,
                balloon,
                ..LeadersForOptions::default()
            };
            // Degrees typed; left out, LeadersForOptions' own (its one
            // source).
            if let Some(value) = parsed.find("angle") {
                options.angle = number_of(value, "angle")? * DEG_TO_RAD;
            }
            if let Some(value) = parsed.find("length") {
                options.length = number_of(value, "length")?;
            }
            let noun = if balloon { "balloons=" } else { "leaders=" };
            if picked.ids.is_empty() {
                // A scope that took nothing to put a leader on: said, not
                // refused.
                return Ok(with_scope(
                    &picked,
                    &format!(
                        "created {noun}0 ids= skipped=0{}",
                        preset_reply(preset.as_ref())
                    ),
                ));
            }
            let made = leaders_for(
                document.model(),
                picked.ids.clone(),
                &options,
                document.annotation_scale(),
                &document.current_attributes(),
                &context,
            )?;
            let (skipped, first_skip) = (made.skipped, made.first_skip.clone());
            document.execute(made.command)?;
            let ids = document.last_created_entities();
            // Why the first was left out, said when any was (the window shows
            // it beside the count, and an agent reads it without a second
            // call).
            let why = if skipped > 0 && !first_skip.is_empty() {
                format!(" firstskip={}", field(&first_skip))
            } else {
                String::new()
            };
            return Ok(with_scope(
                &picked,
                &format!(
                    "created {noun}{} ids={} skipped={skipped}{why}{}",
                    ids.len(),
                    id_list(&ids),
                    preset_reply(preset.as_ref())
                ),
            ));
        }

        // Making one: LEADER p p [p...] / BALLOON p p [p...].
        let parsed = split_arguments(
            args,
            0,
            &[
                "text",
                "template",
                "labelstyle",
                "preset",
                "surface",
                "alignment",
                "arrow",
                "callout",
                "style",
                "paper",
                "arrowsize",
                "landing",
                "landingangle",
                "n",
            ],
        )?;
        if parsed.positional.len() < 2 {
            return Err(usage_of(if balloon {
                "BALLOON p p [p...] [n= | template= | labelstyle=] [style= paper=] | BALLOON FOR \
                 id... | SELECTION | BALLOON RENUMBER"
            } else {
                LEADER_USAGE
            }));
        }
        let mut points = Vec::new();
        for token in &parsed.positional {
            points.push(self.parse_anchored_point(document, token)?);
        }
        let mut change = leader_change_of(&parsed)?;
        let preset = preset_of(&parsed, document, &mut change)?;
        let leader = new_leader(document.model(), &points, &change, balloon, &context)?;
        let attributes = document.current_attributes();
        let made = leader.clone();
        document.execute(Box::new(ChangeSetCommand::new(
            if balloon {
                "CREATE_BALLOON"
            } else {
                "CREATE_LEADER"
            },
            move |_context| {
                let entity = Entity {
                    geometry: Geometry::Leader(made.clone()),
                    layer: attributes.layer.clone(),
                    style: attributes.style.clone(),
                    color: attributes.color,
                    ..Entity::default()
                };
                Ok(ChangeSet {
                    add: vec![entity],
                    ..ChangeSet::default()
                })
            },
            false,
        )))?;
        let ids = document.last_created_entities();
        let mut reply = format!(
            "{}{} text={} associative={}",
            if balloon {
                "created balloon id="
            } else {
                "created leader id="
            },
            id_list(&ids),
            field(&leader_says(document.model(), &leader, &context)),
            if leader.tip_ref.associated() {
                "yes"
            } else {
                "no"
            }
        );
        if leader.tip_ref.associated() {
            reply += &format!(
                " target={} anchor={}",
                leader.tip_ref.entity,
                field(&katana_entity::anchor::describe(&leader.tip_ref))
            );
        }
        if leader.fields {
            reply += &format!(" template={}", field(&leader.text));
        }
        if !leader.label_style.is_empty() {
            reply += &format!(" labelstyle={}", field(&leader.label_style));
        }
        Ok(reply + &preset_reply(preset.as_ref()))
    }
}

// std::from_chars into a u32: digits only, nothing that does not fit.
fn unsigned_of(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

// "v3" or "vertex3" read as the index after `prefix`, the whole rest digits.
fn indexed(part: &str, prefix: &str) -> Option<u32> {
    if part.len() > prefix.len() {
        part.strip_prefix(prefix).and_then(unsigned_of)
    } else {
        None
    }
}

// Runs the dimension's creation and replies with what it made.
fn created_dimension(document: &mut Document, dimension: &DimensionGeometry) -> Result<String> {
    document.execute(create_dimension(
        dimension.clone(),
        document.current_attributes(),
    ))?;
    let ids = document.last_created_entities();
    let model = document.model();
    let text = ids
        .first()
        .and_then(|&id| model.entities.find(id))
        .map(|entity| dimension_text(dimension, resolve_dimension_style(model, entity)))
        .unwrap_or_default();
    Ok(format!(
        "created dimension id={} kind={} measures={} text={} associative={}",
        id_list(&ids),
        dimension.kind.name(),
        real(dimension.measurement()),
        field(&text),
        if dimension.start_ref.associated()
            || dimension.end_ref.associated()
            || dimension.vertex_ref.associated()
        {
            "yes"
        } else {
            "no"
        }
    ))
}

// ---- ANNOSCALE ------------------------------------------------------------------------

fn annotation_scale(document: &mut Document, args: &[String]) -> Result<String> {
    let Some(first) = args.first() else {
        return Ok(format!("annoscale={}", real(document.annotation_scale())));
    };
    let mut text = first.as_str();
    if let Some(colon) = text.find(':') {
        if &text[..colon] != "1" {
            return Err(usage_of("ANNOSCALE N | 1:N"));
        }
        text = &text[colon + 1..];
    }
    let scale = number_of(text, "the scale")?;
    document.set_annotation_scale(scale)?;
    Ok(format!("annoscale={}", real(document.annotation_scale())))
}

// ---- TEXTSTYLE ------------------------------------------------------------------------------

fn text_style(document: &mut Document, args: &[String]) -> Result<String> {
    let action = args
        .first()
        .map_or_else(|| String::from("LIST"), |word| upper_case(word));
    if action == "LIST" {
        let mut lines: Vec<String> = Vec::new();
        document
            .model()
            .text_styles
            .for_each(|style| lines.push(describe_text_style(style)));
        return Ok(lines.join("\n"));
    }
    if args.len() < 2 {
        return Err(usage_of(
            "TEXTSTYLE LIST | NEW name [opts] | SET name opts | DELETE name",
        ));
    }
    let name = &args[1];
    if action == "DELETE" {
        return finish(
            document.execute(delete_text_style(name.as_str())),
            format!("deleted text style name={}", field(name)),
        );
    }
    let parsed = split_arguments(args, 2, TEXT_STYLE_KEYS)?;
    match action.as_str() {
        "NEW" => {
            let mut style = TextStyle {
                name: name.clone(),
                ..TextStyle::default()
            };
            apply_text_style_options(&parsed, &mut style)?;
            let described = describe_text_style(&style);
            finish(
                document.execute(create_text_style(style)),
                format!("created text style {described}"),
            )
        }
        "SET" => {
            let Some(current) = document.model().text_styles.find(name) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "text style does not exist",
                    name.as_str(),
                ));
            };
            let mut style = current.clone();
            apply_text_style_options(&parsed, &mut style)?;
            let described = describe_text_style(&style);
            finish(
                document.execute(update_text_style(style)),
                format!("updated text style {described}"),
            )
        }
        "INFO" => match document.model().text_styles.find(name) {
            Some(current) => Ok(describe_text_style(current)),
            None => Err(make_error(
                ErrorCode::NotFound,
                "text style does not exist",
                name.as_str(),
            )),
        },
        _ => Err(usage_of(
            "TEXTSTYLE LIST | NEW name [opts] | SET name opts | DELETE name | INFO name",
        )),
    }
}

// ---- LABELSTYLE -------------------------------------------------------------------------------

fn label_style(document: &mut Document, args: &[String]) -> Result<String> {
    let action = args
        .first()
        .map_or_else(|| String::from("LIST"), |word| upper_case(word));
    if action == "LIST" {
        let mut lines: Vec<String> = Vec::new();
        document
            .model()
            .label_styles
            .for_each(|style| lines.push(describe_label_style(style)));
        return Ok(lines.join("\n"));
    }
    if action == "DEFAULTS" {
        // The standard set, those not there yet, as one step.
        let mut transaction = Transaction::new("LABELSTYLE_DEFAULTS");
        let mut added: Vec<String> = Vec::new();
        for style in default_label_styles() {
            if document.model().label_styles.contains(&style.name) {
                continue;
            }
            added.push(field(&style.name));
            transaction.add(create_label_style(style));
        }
        if transaction.size() == 0 {
            return Ok(String::from("added=0"));
        }
        let count = transaction.size();
        return finish(
            document.execute(Box::new(transaction)),
            format!("added={count} styles={}", added.join(",")),
        );
    }
    if args.len() < 2 {
        return Err(usage_of(
            "LABELSTYLE LIST | NEW name kind= [opts] | SET name opts | DELETE name | DEFAULTS | \
             VALUES kind | CHECK kind template",
        ));
    }
    let name = &args[1];
    if action == "VALUES" {
        let kind = label_kind_from_string(name)?;
        let mut out = format!(
            "kind={} values=id,layer,code,point,description",
            kind.name()
        );
        for value in label_value_names(kind) {
            out.push(',');
            out += value;
        }
        return Ok(out + ",prop.NAME");
    }
    if action == "CHECK" {
        let kind = label_kind_from_string(name)?;
        if args.len() < 3 {
            return Err(usage_of("LABELSTYLE CHECK kind \"template\""));
        }
        check_label_template(&unescape(&args[2]), kind)?;
        return Ok(String::from("valid=yes"));
    }
    if action == "DELETE" {
        return finish(
            document.execute(delete_label_style(name.as_str())),
            format!("deleted label style name={}", field(name)),
        );
    }
    let parsed = split_arguments(args, 2, LABEL_STYLE_KEYS)?;
    match action.as_str() {
        "NEW" => {
            let mut style = LabelStyle {
                name: name.clone(),
                ..LabelStyle::default()
            };
            if let Some(kind) = parsed.find("kind") {
                style.kind = label_kind_from_string(kind)?;
            }
            style.text = default_template(style.kind);
            if style.kind == LabelKind::Area {
                style.placement = LabelPlacement::Centroid;
            }
            apply_label_style_options(&parsed, &mut style)?;
            let described = describe_label_style(&style);
            finish(
                document.execute(create_label_style(style)),
                format!("created label style {described}"),
            )
        }
        "SET" => {
            let Some(current) = document.model().label_styles.find(name) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "label style does not exist",
                    name.as_str(),
                ));
            };
            if parsed.has("kind") {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a label style's kind is fixed; make a new style for another kind",
                    name.as_str(),
                ));
            }
            let mut style = current.clone();
            apply_label_style_options(&parsed, &mut style)?;
            let described = describe_label_style(&style);
            finish(
                document.execute(update_label_style(style)),
                format!("updated label style {described}"),
            )
        }
        "INFO" => match document.model().label_styles.find(name) {
            Some(current) => Ok(describe_label_style(current)),
            None => Err(make_error(
                ErrorCode::NotFound,
                "label style does not exist",
                name.as_str(),
            )),
        },
        _ => Err(usage_of(
            "LABELSTYLE LIST | NEW | SET | DELETE | INFO | DEFAULTS | VALUES | CHECK",
        )),
    }
}

// ---- AUTOLABEL -------------------------------------------------------------------------------

fn auto_label_verb(document: &mut Document, args: &[String]) -> Result<String> {
    let action = args
        .first()
        .map(|word| upper_case(word))
        .unwrap_or_default();
    if action == "RULE" {
        let sub = args
            .get(1)
            .map_or_else(|| String::from("LIST"), |word| upper_case(word));
        if sub == "LIST" {
            let mut lines: Vec<String> = Vec::new();
            document
                .model()
                .label_rules
                .for_each(|rule| lines.push(describe_rule(rule)));
            return Ok(lines.join("\n"));
        }
        if args.len() < 3 {
            return Err(usage_of(
                "AUTOLABEL RULE ADD name style= [opts] | SET name opts | DELETE name | LIST",
            ));
        }
        let name = &args[2];
        if sub == "DELETE" {
            return finish(
                document.execute(delete_label_rule(name.as_str())),
                format!("deleted rule name={}", field(name)),
            );
        }
        let parsed = split_arguments(args, 3, RULE_KEYS)?;
        if sub == "ADD" || sub == "NEW" {
            let mut rule = LabelRule {
                name: name.clone(),
                ..LabelRule::default()
            };
            apply_rule_options(&parsed, &mut rule)?;
            if !document.model().label_styles.contains(&rule.label_style) {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "label style does not exist",
                    rule.label_style.as_str(),
                ));
            }
            let described = describe_rule(&rule);
            return finish(
                document.execute(create_label_rule(rule)),
                format!("created rule {described}"),
            );
        }
        if sub == "SET" {
            let Some(current) = document.model().label_rules.find(name) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "label rule does not exist",
                    name.as_str(),
                ));
            };
            let mut rule = current.clone();
            apply_rule_options(&parsed, &mut rule)?;
            if !document.model().label_styles.contains(&rule.label_style) {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "label style does not exist",
                    rule.label_style.as_str(),
                ));
            }
            let described = describe_rule(&rule);
            return finish(
                document.execute(update_label_rule(rule)),
                format!("updated rule {described}"),
            );
        }
        return Err(usage_of("AUTOLABEL RULE ADD | SET | DELETE | LIST"));
    }
    let rules: Vec<String> = args.get(1..).unwrap_or_default().to_vec();
    if action == "RUN" || action == "PREVIEW" {
        let mut report = AutoLabelReport::default();
        let built = auto_label(document.model(), &rules, Some(&mut report))?;
        let mut out = format!(
            "{} created={} kept={} removed={} skipped={}",
            if action == "RUN" {
                "autolabel"
            } else {
                "preview"
            },
            report.created,
            report.kept,
            report.removed,
            report.skipped
        );
        for (rule, count) in &report.per_rule {
            out += &format!("\nrule={} labels={count}", field(rule));
        }
        if action == "RUN"
            && let Some(command) = built
        {
            document.execute(command)?;
        }
        return Ok(out);
    }
    if action == "CLEAR" {
        let mut removed = 0;
        let built = clear_auto_labels(document.model(), &rules, Some(&mut removed));
        return run_built(document, built, format!("removed={removed}"), "removed=0");
    }
    Err(usage_of(
        "AUTOLABEL RULE ... | RUN [rule...] | PREVIEW [rule...] | CLEAR [rule...]",
    ))
}

const ANNOTATION_HELP_TEXT: &str = r#"Annotation (docs/annotation.md): sizes in PAPER mm, drawn at mm x scale / 1000
          options are key=value; \n in a text or template is a line break; replies are
          key=value records.  A point may be #id[.start|.end|.mid|.centre|.inside|.vN|.sN],
          or #id@x,y (the point of #id nearest x,y): the annotation made from it follows
          that entity (associative).
AnnoScale ANNOSCALE [N | 1:N]   the plan view's annotation scale (one undo step)
TextStyle TEXTSTYLE LIST | NEW name [opts] | SET name opts | DELETE name
          opts: font= paper=mm width= oblique=deg bold= italic= colour=#RRGGBB|bylayer
          mask=on|off margin=mm readable=on|off spacing=
Text      TEXT p [height] "text" style= paper=mm justify=TL..BR rotation=deg
          MTEXT p "text" [style= paper= justify= rotation=]   paper-sized, multi-line
          TEXTEDIT id [text= style= paper= justify= rotation= height= at=x,y]
LabelStyle LABELSTYLE LIST | NEW name kind=point|segment|arc|area|chainage [text=template]
          [opts] | SET name opts | DELETE name | DEFAULTS | VALUES kind | CHECK kind template
          opts: text= textstyle= paper= placement=auto|above|below|along|centroid|right|left
          orientation=aligned|horizontal offset=mm leader= displace= priority= colour=
          marker=none|cross|dot|circle markersize=mm minlength=mm interval= tick= ticklength=mm
          template: {value:step:step} e.g. {bearing:dms} {distance:.3f}  {area:ha:.4f}
          RL {z:.3f}  {chainage:ch}; steps m mm km ft m2 ha km2 ac deg rad gon dms dms.N dm qb
          ch .Nf upper lower; a line with a value the target lacks is dropped
Label     LABEL id [id...] style= [part=N] [layer=] [at=x,y] [text=]  (SELECTION for the
          selection) | LABEL ALIGN name style= [layer=] | LIST [id...] | SET id [style=
          at=x,y|none text=|none layer=] | DELETE id... | LAYOUT [scale=N] [collisions=on|off]
          (LAYOUT adds a label= piece= suppressed=yes line for each piece with no room)
AutoLabel AUTOLABEL RULE ADD name style= [layer=glob code=glob type= labellayer= enabled=]
          AUTOLABEL RULE SET name opts | RULE DELETE name | RULE LIST
          AUTOLABEL RUN [rule...] | PREVIEW [rule...] | CLEAR [rule...]
Dim       DIM LINEAR|HORIZONTAL|VERTICAL p p at=p [angle=deg] | DIM ALIGNED p p at=p
          DIM ANGULAR vertex p p [at=p] | DIM ANGULAR line-id line-id at=p
          DIM RADIUS|DIAMETER circle-id [at=p] | DIM ORDINATE p at=p [datum=p axis=x|y]
          DIM BASELINE dim-id p [p...] [spacing=] | DIM CONTINUE dim-id p [p...]
Leader    LEADER p p [p...] [text= | template= | labelstyle=] [arrow=closed|open|tick|dot|none
          callout=none|box|circle style= paper=mm arrowsize=mm landing=mm landingangle=auto|deg]
          a SMART leader's tip is on an entity (#id...) and its note is read off it every
          time it is drawn: template="IL {prop.invert:.3f}\n{length:.2f} m" (the label
          template language), or labelstyle=name (that style's template); a line naming a
          value the entity lacks is dropped. A tip inside an outline (#id.inside) ends in a dot.
          A note may also read the drawing's surfaces and alignments at the tip: {surface.NAME}
          the level, {dsurface.NAME} the target's z above it, {cut.NAME} {fill.NAME} (one is
          present), {station.NAME} {offset.NAME} (left +), {profile.NAME} {dprofile.NAME}; NAME
          as the drawing has it, blanks and all; +.Nf writes a + before a value above nothing
          preset=level|surface|cutfill|name|code|details|station [surface=name|alignment=name]
          a ready template (LEADER PRESETS lists them); with none named, the first visible
          surface or the first alignment, said as chosen= in the reply
          LEADER VALUES id | #id[.point|@x,y] | <scope>   every value a note there can name
          LEADER LIST [id...] [target=id] [JSON] | PRESETS | SET id...|<scope> [text= template=
          labelstyle= preset= arrow= callout= style= paper= arrowsize= landing= landingangle=auto|deg
          tip=p at=p [vertex=N]]  vertex is zero-based (0 is the tip); at alone moves the note end
          LEADER ATTACH id #id[.point|@x,y] | DETACH id...|<scope> | FREEZE id...|<scope>
          LEADER PROP id|<scope> SET key value [type] | PROP id|<scope> DELETE key   the tip
          entity's attribute
          LEADER FOR id...|<scope> [text=|template=|labelstyle=|preset=] [angle=deg length=mm]
          a leader to each entity, one undo step (a point, mid-line, mid-arc, inside a lot)
          LEADER ALIGN id id...|<scope> [x=] [spacing=mm]   the notes in a column
          BALLOON p p [p...] [n=number | template= | labelstyle=] [style= paper=]   a numbered
          circle callout; BALLOON FOR id...|<scope> numbers one to each entity;
          BALLOON RENUMBER [<scope>] [start=1] [order=id|x|y]   the numbered balloons, in order
          <scope>   SELECTION | VIEW [id] [EXTENTS] | DRAWING | AREA x0,y0,x1,y1 | LAYERS a,b
          [ONLY], then WHERE TYPE=.. LAYER=.. STYLE=.. COLOUR=.. PROP=.. TEXT=.. DRAWN - the grammar
          of MODIFY and the UTILITY verbs, in place of ids. A reply leads with scope=... matched=N
          (the leaders, or for FOR the entities, it took); one that took none is said, not refused.
          The verb's own key=value options go BEFORE WHERE: after it style= and text= are
          conditions."#;
