//! The Annotate family's shared parts (src/katana_cad/tools/annotate_common.hpp
//! and .cpp): the few rules every annotation tool applies the same way - what
//! a typed option or number is, how wide text is taken to be, how far apart
//! its lines are, and which dimension style a new leader or dimension takes.
//! One place, so Text and Leader cannot space their lines differently and two
//! tools cannot disagree about what "U" means.
//!
//! The C++ header also declares each tool's factory; here each factory is in
//! its tool's own module (`tools::annotate_text::make_text_tool`, ...), and
//! annotate.rs lists them in the catalogue.
//!
//! Besides the C++'s, two things the Rust tools need that no C++ file had to
//! say: a factory built from a function or closure (`factory`), and the
//! trait's own default answers for an override that falls back to them (the
//! C++'s `InteractiveTool::point(at)` and the like, which an overriding Rust
//! method cannot call). The vertex tools (modify_vertex.rs) use both too.

use std::collections::BTreeSet;
use std::sync::Arc;

use katana_commands::{ChangeSet, ChangeSetCommand, CommandPtr, EntityAttributes};
use katana_core::text::{equals_ignoring_case, format_exact_real, parse_finite_double};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{DimensionStyle, Entity, EntityId, EntityType, Geometry};
use katana_geometry::Point2;
use katana_math::numerics::tolerance;

use crate::dimension_draw::resolve_dimension_style;
use crate::document::Document;
use crate::interactive_tool::{
    InteractiveTool, ToolContext, ToolFactory, ToolInput, ToolStart, ToolStep,
};

/// core's trimmed over text: it drops ASCII bytes only, so what is left is
/// still UTF-8.
pub(crate) fn trimmed(text: &str) -> &str {
    std::str::from_utf8(katana_core::text::trimmed(text.as_bytes())).unwrap_or_default()
}

/// True when `typed` names the option `keyword`: the keyword or any prefix of
/// it at least `shortest` letters long (the C++ default is 1), in any case -
/// "u", "UN" and "Undo" all name Undo - which is how a CAD command line
/// accepts an option's capital letters.
pub fn is_option(typed: &str, keyword: &str, shortest: usize) -> bool {
    let word = trimmed(typed);
    if word.len() < shortest || word.len() > keyword.len() {
        return false;
    }
    equals_ignoring_case(word.as_bytes(), &keyword.as_bytes()[..word.len()])
}

/// The text style `typed` names in `document`, as it is stored: by its name,
/// else ignoring case, as a CAD command line takes one. NotFound saying where
/// the styles are listed, or with no drawing to look in. Shared by the tools
/// that offer a note's Style (Leader, Balloon), so both find the same one.
pub fn text_style_named(document: Option<&Document>, typed: &str) -> Result<String> {
    let text = trimmed(typed);
    let Some(document) = document else {
        return Err(Error::new(
            ErrorCode::NotFound,
            "there is no drawing to take the style from",
        ));
    };
    let styles = &document.model().text_styles;
    if styles.contains(text) {
        return Ok(String::from(text));
    }
    for name in styles.names() {
        if equals_ignoring_case(name.as_bytes(), text.as_bytes()) {
            return Ok(name);
        }
    }
    Err(Error::new(
        ErrorCode::NotFound,
        format!("there is no text style \"{text}\"; Format > Text Styles lists them"),
    ))
}

/// A typed height, angle, offset or distance. Through core's text, so "2.5"
/// means two and a half on every machine and "2,5" is not a number.
pub fn typed_number(typed: &str) -> Option<f64> {
    parse_finite_double(trimmed(typed))
}

/// Two points closer than the geometric tolerance are one point: a height, a
/// direction or a length measured between them is rounding noise, not input.
pub fn coincident(a: Point2, b: Point2) -> bool {
    !(a.distance_to(b) > tolerance::GEOMETRIC)
}

/// A new entity with the current layer, style and colour - what every tool
/// here creates.
pub fn new_entity(geometry: Geometry, attributes: &EntityAttributes) -> Entity {
    Entity {
        geometry,
        layer: attributes.layer.clone(),
        style: attributes.style.clone(),
        color: attributes.color,
        ..Entity::default()
    }
}

/// ONE command adding every entity in `entities`, shown as `name` in the
/// history, so one undo removes a whole multi-line text or a leader with its
/// arrow and its text.
pub fn create_all(name: impl Into<String>, entities: Vec<Entity>) -> CommandPtr {
    // The builder copies rather than moves its entities out: validate() may
    // run it more than once (change_set.rs), and a moved-from set would be
    // an empty command the second time.
    Box::new(ChangeSetCommand::new(
        name,
        move |_context| {
            Ok(ChangeSet {
                add: entities.clone(),
                ..ChangeSet::default()
            })
        },
        false,
    ))
}

/// The dimension style an annotation made now on the current layer is drawn
/// with: the layer's named style, else the document's default - the rule
/// resolve_dimension_style applies when the dimension is drawn, so a leader's
/// arrow and text are sized as a dimension beside it is. The C++ reads the
/// layer from the tool's context; here it is the ToolStart's `attributes`.
pub fn style_for_new_annotation(
    cx: &ToolContext<'_>,
    attributes: &EntityAttributes,
) -> DimensionStyle {
    let Some(document) = cx.document else {
        return DimensionStyle::default();
    };
    // resolve_dimension_style reads only the entity's layer, so an entity
    // that is not in the model yet resolves exactly as the one about to be
    // made.
    let probe = Entity {
        layer: attributes.layer.clone(),
        ..Entity::default()
    };
    resolve_dimension_style(document.model(), &probe).clone()
}

/// The width `text` is taken to occupy at `height`. The model has no font
/// metrics, so this is the 0.6-of-the-height advance per character that
/// entity's bounding box and the dimension label's centring already use,
/// counted per character (UTF-8 code point) rather than per byte, so "é" is
/// one character wide and not two.
pub fn estimated_text_width(text: &str, height: f64) -> f64 {
    // A UTF-8 continuation byte is 10xxxxxx; every other byte starts a
    // character.
    let characters = text.bytes().filter(|&byte| (byte & 0xC0) != 0x80).count();
    const ADVANCE_PER_CHARACTER: f64 = 0.6;
    ADVANCE_PER_CHARACTER * height * characters as f64
}

/// Baseline to baseline between the lines of a multi-line text or a leader's
/// note: 5/3 of the height, the single line spacing of AutoCAD's text (MTEXT
/// documents it as about 1.66 times the height). Computed as height * 5 / 3
/// so that a height of 3 gives exactly 5.
pub fn line_spacing(height: f64) -> f64 {
    height * 5.0 / 3.0
}

/// A number as a prompt shows a default: "2.5", "90". Rounded to nine
/// decimals first, so an angle stored in radians reads back as 90 and not
/// 90.00000000000001.
pub fn format_number(value: f64) -> String {
    // Only where nine decimals are within a double's precision; a larger
    // value has no such noise to remove and would gain some from the scaling.
    const SCALE: f64 = 1e9;
    let rounded = if value.abs() < 1e6 {
        (value * SCALE).round() / SCALE
    } else {
        value
    };
    // -0 would print as "-0".
    format_exact_real(if rounded == 0.0 { 0.0 } else { rounded })
}

/// What a leader's tip can be put on: what offers a place along it or a
/// position (entity's nearest_anchor) - the drawing system's curve polyline
/// and ellipse too, not a spline, which has no place along it. The Leader
/// tool's and the preset leaders' (the C++ writes the set out in each file).
pub(crate) fn tip_targets() -> BTreeSet<EntityType> {
    BTreeSet::from([
        EntityType::Point,
        EntityType::Line,
        EntityType::Arc,
        EntityType::Polyline,
        EntityType::Circle,
        EntityType::Text,
        EntityType::CurvePolyline,
        EntityType::Ellipse,
    ])
}

// ---- what a Rust tool needs that the C++ did not have to say ------------------------

/// A catalogue factory from a function or a closure. The bound makes the
/// closure's signature the factory's own, over every lifetime of the context
/// it is lent (a closure handed to `Arc::new` alone would be given one).
pub(crate) fn factory<F>(make: F) -> ToolFactory
where
    F: Fn(&ToolContext<'_>, &ToolStart) -> Box<dyn InteractiveTool> + Send + Sync + 'static,
{
    Arc::new(make)
}

// A tool with nothing of its own, whose methods are therefore the trait's
// defaults, answering as a tool at `expects` would.
struct TraitDefaults(ToolInput);

impl InteractiveTool for TraitDefaults {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::new()
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        self.0
    }
}

/// The trait's own point(): what the C++ `InteractiveTool::point(at)` answers
/// for a tool whose step `expects` this.
pub(crate) fn default_point(cx: &ToolContext<'_>, expects: ToolInput, at: Point2) -> ToolStep {
    TraitDefaults(expects).point(cx, at)
}

/// The trait's own entity() (the C++ `InteractiveTool::entity(id, at)`).
pub(crate) fn default_entity(
    cx: &ToolContext<'_>,
    expects: ToolInput,
    id: EntityId,
    at: Point2,
) -> ToolStep {
    TraitDefaults(expects).entity(cx, id, at)
}

/// The trait's own enter() (the C++ `InteractiveTool::enter()`).
pub(crate) fn default_enter(cx: &ToolContext<'_>) -> ToolStep {
    TraitDefaults(ToolInput::Value).enter(cx)
}
