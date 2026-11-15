//! How an entity's drawn appearance is resolved.
//!
//! There must be exactly ONE answer to "what colour is this entity, how thick
//! is it, and what linetype does it use", because every renderer has to agree.
//! Before this existed there were three different answers, and nothing read
//! Style::color, Style::line_weight or Style::linetype at all.
//!
//! THE CHAIN, outermost override first. This is the DXF model (ByLayer /
//! ByBlock), which is what a CAD user expects and what an exchange format
//! assumes:
//!
//!   colour       entity.color, else the style's colour if it has one, else
//!                the layer's colour.
//!   line weight  the style's if the entity names one, else the layer's.
//!   linetype     the style's if the entity names one and it is not "ByLayer",
//!                else the layer's.
//!   hatch        the style's if it names one, else the layer's.
//!
//! A linetype has no empty value to say "ByLayer" with - "" is a name like any
//! other in the tables - so a Style says it with the word itself, which DXF
//! already reserves (group code 6) and validate(Linetype) already refuses as a
//! linetype NAME.
//!
//! A missing layer or style resolves to the defaults rather than failing. An
//! entity can reference a layer that has been removed - the model permits it,
//! and a renderer must not crash on it.

use katana_core::text::equals_ignoring_case;

use crate::entity::{Color, Entity};
use crate::model::Model;
use crate::tables::{CONTINUOUS_LINETYPE, Layer, NO_HATCH, NO_SYMBOL, Style};

/// What a Style::linetype says to inherit the layer's linetype.
pub const BY_LAYER_LINETYPE: &str = "ByLayer";

/// In any case, because the reservation it relies on is in any case: no
/// linetype can be called "BYLAYER" either, so the spelling cannot be taken
/// for a name.
pub fn is_by_layer(linetype: &str) -> bool {
    equals_ignoring_case(linetype.as_bytes(), BY_LAYER_LINETYPE.as_bytes())
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedDisplay {
    pub color: Color,
    /// Millimetres on paper.
    pub line_weight: f64,
    pub linetype: String,
    /// "none" when the entity is not hatched, never empty, so a renderer looks
    /// the name up rather than testing for emptiness first.
    pub hatch_pattern: String,
    /// For a point: the symbol its style draws it with, NO_SYMBOL for the
    /// viewport's plain mark; and the size, 0 for the viewport's own.
    pub symbol: String,
    pub symbol_size: f64,
}

impl Default for ResolvedDisplay {
    fn default() -> Self {
        Self {
            color: Color::default(),
            line_weight: 0.25,
            linetype: String::from(CONTINUOUS_LINETYPE),
            hatch_pattern: String::from(NO_HATCH),
            symbol: String::from(NO_SYMBOL),
            symbol_size: 0.0,
        }
    }
}

/// The linetype NAME of the chain, for a layer and a style already found
/// (either may be None: a missing layer, or no style or a missing one). What
/// resolve_display itself reads, so a caller that counts entities per
/// linetype - table_usage - cannot come to a different answer.
pub fn resolved_linetype<'a>(layer: Option<&'a Layer>, style: Option<&'a Style>) -> &'a str {
    if let Some(style) = style
        && !is_by_layer(&style.linetype)
    {
        return &style.linetype;
    }
    // A layer has no layer above it to inherit from, so a layer that says
    // ByLayer - which nothing here offers, but a hand-made table could hold -
    // draws what a missing layer does.
    if let Some(layer) = layer
        && !is_by_layer(&layer.linetype)
    {
        return &layer.linetype;
    }
    CONTINUOUS_LINETYPE
}

/// The hatch pattern NAME of the chain, as resolved_linetype.
pub fn resolved_hatch_pattern<'a>(layer: Option<&'a Layer>, style: Option<&'a Style>) -> &'a str {
    if let Some(style) = style
        && !style.hatch_pattern.is_empty()
    {
        return &style.hatch_pattern;
    }
    if let Some(layer) = layer
        && !layer.hatch_pattern.is_empty()
    {
        return &layer.hatch_pattern;
    }
    NO_HATCH
}

/// Never fails: every step has a defined fallback, ending at the built-in
/// defaults.
pub fn resolve_display(model: &Model, entity: &Entity) -> ResolvedDisplay {
    let mut resolved = ResolvedDisplay::default();

    let layer = model.layers.find(&entity.layer);
    let style = if entity.style.is_empty() {
        None
    } else {
        model.styles.find(&entity.style)
    };

    if let Some(layer) = layer {
        resolved.color = layer.color;
        resolved.line_weight = layer.line_weight;
    }
    // A named style overrides the layer. line_weight is not optional on a
    // Style, so naming a style takes it; the linetype is taken unless the
    // style says ByLayer; colour is optional and an empty one means "keep
    // using the layer's", which is what ByLayer means.
    if let Some(style) = style {
        resolved.line_weight = style.line_weight;
        if let Some(color) = style.color {
            resolved.color = color;
        }
        // A symbol is the style's alone: a layer has none, and a point with
        // no style draws the plain mark.
        resolved.symbol = style.symbol.clone();
        resolved.symbol_size = style.symbol_size;
    }
    resolved.linetype = resolved_linetype(layer, style).to_owned();
    resolved.hatch_pattern = resolved_hatch_pattern(layer, style).to_owned();

    // The entity's own colour overrides everything. It is the only per-entity
    // override the model has; there is deliberately no per-entity line weight
    // or linetype, because those belong to the layer or the style.
    if let Some(color) = entity.color {
        resolved.color = color;
    }
    resolved
}
