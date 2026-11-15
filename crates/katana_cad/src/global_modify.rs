//! Global Modify (the owner's request of 2026-09-25: "a global modification
//! tool that can act on data on a selected view, layers, selected features,
//! etc., that can change styles, symbols, layers, colour, etc.";
//! include/katana/cad/global_modify.hpp,
//! src/katana_cad/customisation/global_modify.cpp).
//!
//! One request says three things, and each is a plain value here so that the
//! window's dialog, the command line's MODIFY and a test all state it the same
//! way:
//!
//! ```text
//! WHERE   ModifyScope: the selection, what a view draws (its hidden layers
//!         and, optionally, only what lies in its visible area), a list of
//!         layers (with or without the layers beneath them), or the whole
//!         drawing.
//! WHICH   ModifyFilter: narrows what the scope holds by type, layer, style,
//!         colour, property and text, so "every point coded TREE on the
//!         survey layers" is one request.
//! WHAT    GlobalModify: the entities' own attributes (layer, colour, style,
//!         visibility, properties, text height, the symbol a point draws),
//!         the layers they sit on (colour, linetype, weight, hatch,
//!         dimension style, shown, locked) and the styles they wear
//!         (colour, linetype, weight, hatch, symbol, symbol size, and a scale
//!         on the symbol size).
//! ```
//!
//! plan_global_modify turns the three into ONE command, so one Undo puts every
//! entity, layer and style back (Rule 2): nothing here edits the model. What
//! the plan reports - how many matched, how many change, which layers and
//! styles are rewritten, what was left alone and why - is worked out before
//! anything runs, so the dialog can show it as a preview and the command line
//! can print it.
//!
//! FIELDS THAT ARE ABSENT ARE LEFT ALONE. Every field of a change is an
//! Option; None means "not asked", never "clear it". Where the value itself
//! may be ByLayer, the value is an Option too: a colour field of
//! `Option<Option<Color>>` holding `Some(None)` is "make it ByLayer", and
//! `None` is "do not touch the colour". (The same reading as Entity::color,
//! where None is ByLayer.)
//!
//! WHAT IS NOT CHANGED, AND IS SAID. An entity on a locked layer is counted and
//! left as it is, as Match Properties leaves one (modify_properties.cpp) -
//! unless the same request unlocks that layer, in which case it is changed,
//! because the layer is unlocked first. A request that locks a layer locks it
//! after its entities are changed. A symbol is drawn only by a point (a line
//! wearing a symbol style draws it at every vertex, D8, but "put a tree on
//! these" means the points), so the other entities are counted, not moved.
//!
//! STYLE DEFINITIONS REACH BEYOND THE SCOPE. A style is shared: changing the
//! symbol of the style the selected points wear changes it for every entity
//! wearing that style, selected or not. The plan counts those others
//! (`style_reaches_others`) so no front end can hide it, and the dialog says
//! it beside the button.
//!
//! This, with scope_verbs, is CLAUDE.md section 1.1's one scope and filter
//! mechanism: every tool that reads or changes drawing data resolves its scope
//! through match_entities.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use katana_commands::{
    ChangeSet, ChangeSetCommand, CommandContext, CommandPtr, Transaction, create_layer,
    set_entity_style, update_layer, update_style_if_changed,
};
use katana_core::error::make_error;
use katana_core::text::{ascii_lower, equals_ignoring_case, format_exact_real};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::display::{BY_LAYER_LINETYPE, is_by_layer};
use katana_entity::entity::property_to_string;
use katana_entity::entity_geometry::bounding_box;
use katana_entity::layer_path::validate_layer_path;
use katana_entity::tables::{NO_SYMBOL, is_built_in_symbol_name};
use katana_entity::{
    Color, Entity, EntityId, EntityType, EnumName, Geometry, Layer, Model, PropertyMap,
    PropertyValue, Style,
};
use katana_geometry::Box2;

use crate::document::Document;
use crate::layer_overrides::{LayerOverrides, NO_LAYER_OVERRIDES};
use crate::selection::is_drawn;
use crate::style_catalogue::check_linetype_name;
use crate::symbol_assign::assign_symbol_to_points;

// ---- where -------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScopeKind {
    /// The document's selection.
    #[default]
    Selection,
    /// What one view draws.
    View,
    /// Everything on the named layers.
    Layers,
    /// Every entity in the drawing.
    Drawing,
}

/// The C++ `toString(ScopeKind)`: "selection", "view", "layers", "drawing".
impl EnumName for ScopeKind {
    fn name(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::View => "view",
            Self::Layers => "layers",
            Self::Drawing => "drawing",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModifyScope {
    pub kind: ScopeKind,

    /// View: the layers that view hides of its own (ViewState::layers), on top
    /// of the document's visibility. None is a view that hides nothing of its
    /// own, so the document rule alone decides what it draws. The C++ holds a
    /// pointer to layers someone else owns (a view's, or a ResolvedScope's
    /// shared copy); here the scope shares them, so a copy of the scope is as
    /// good as the original.
    pub view: Option<Arc<LayerOverrides>>,
    /// View: when set, only entities whose extent overlaps this box - a plan
    /// view's visible area (ViewTransform::visible_world_bounds), so "what I
    /// can see" means what is on screen, not everything on its layers.
    pub area: Option<Box2>,

    /// Layers: the layer paths, as the layer panel names them. Each must exist.
    pub layers: Vec<String>,
    /// Layers: a layer also takes everything nested beneath it, as hiding
    /// "design" hides "design/surface/tin1" (layer_path.rs).
    pub sublayers: bool,
}

impl Default for ModifyScope {
    fn default() -> Self {
        Self {
            kind: ScopeKind::Selection,
            view: None,
            area: None,
            layers: Vec::new(),
            sublayers: true,
        }
    }
}

// ---- which -------------------------------------------------------------------------------

/// A text pattern: '*' is any run of characters, '?' any one; ASCII letters
/// match either case. What CAD and survey programs take in a "name" field, and
/// what the survey codes' keys already use ("AC*"). A pattern with neither is
/// an exact match, apart from case. Matched byte by byte, as the C++ matches.
pub fn matches_pattern(text: &str, pattern: &str) -> bool {
    // The classic two-pointer match with one back-track point: linear in
    // practice, and with no recursion for a hostile "*a*a*a*..." to exhaust.
    let text = text.as_bytes();
    let pattern = pattern.as_bytes();
    let mut t = 0;
    let mut p = 0;
    let mut star_at: Option<usize> = None;
    let mut resume_at = 0;
    while t < text.len() {
        if p < pattern.len()
            && (pattern[p] == b'?' || ascii_lower(pattern[p]) == ascii_lower(text[t]))
        {
            t += 1;
            p += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star_at = Some(p);
            p += 1;
            resume_at = t;
        } else if let Some(star) = star_at {
            p = star + 1;
            resume_at += 1;
            t = resume_at;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModifyFilter {
    /// Empty: every type.
    pub types: BTreeSet<EntityType>,
    /// Layer names or patterns; an entity passes on ANY of them. Empty: every
    /// layer. Unlike ModifyScope::layers these are patterns and match the
    /// layer's full path, so "survey/*" takes everything beneath "survey".
    pub layers: Vec<String>,
    /// The style an entity wears, as a pattern; "ByLayer" (any case) or ""
    /// is an entity with no style of its own.
    pub style: Option<String>,
    /// The entity's own colour; an inner None is ByLayer.
    pub colour: Option<Option<Color>>,
    /// An entity carrying this property (exact key). With `property_value`
    /// also, its value as entity's property_to_string prints it must match
    /// that pattern.
    pub property: Option<String>,
    pub property_value: Option<String>,
    /// A Text entity's text, or a Dimension's override, as a pattern. Other
    /// entities do not pass when it is set.
    pub text: Option<String>,
    /// Leave out entities that are not drawn: switched off themselves, or on
    /// a layer that is hidden. The View scope always leaves them out, since a
    /// view does not draw them.
    pub drawn_only: bool,
}

impl ModifyFilter {
    pub fn empty(&self) -> bool {
        self.types.is_empty()
            && self.layers.is_empty()
            && self.style.is_none()
            && self.colour.is_none()
            && self.property.is_none()
            && self.property_value.is_none()
            && self.text.is_none()
            && !self.drawn_only
    }
}

// ---- what --------------------------------------------------------------------------------

/// What a point draws, through the style it is moved into (symbol_assign.rs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SymbolModify {
    /// A library symbol or a built-in shape.
    pub name: String,
    /// Model units; 0 is the definition's own.
    pub size: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityModify {
    /// Moves them onto this layer, created (with default settings) when the
    /// drawing has none of that name.
    pub layer: Option<String>,
    pub colour: Option<Option<Color>>,
    /// A style the drawing has, or "" for ByLayer.
    pub style: Option<String>,
    pub visible: Option<bool>,
    /// Set on every entity in scope, replacing a value of another type.
    pub set_properties: Vec<(String, PropertyValue)>,
    pub remove_properties: Vec<String>,
    /// Text entities: the height of the text, in model units.
    pub text_height: Option<f64>,
    /// Points: the symbol drawn at them. Applied after `style`, so a point
    /// given both wears the symbol's style.
    pub symbol: Option<SymbolModify>,
}

impl EntityModify {
    pub fn empty(&self) -> bool {
        self.layer.is_none()
            && self.colour.is_none()
            && self.style.is_none()
            && self.visible.is_none()
            && self.set_properties.is_empty()
            && self.remove_properties.is_empty()
            && self.text_height.is_none()
            && self.symbol.is_none()
    }
}

/// Applied to the layers the matched entities sit on - or, for the Layers
/// scope, to the scope's layers themselves, entities or not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerModify {
    pub colour: Option<Color>,
    /// A model linetype or a loaded library linestyle (check_linetype_name).
    pub linetype: Option<String>,
    /// Millimetres on paper.
    pub line_weight: Option<f64>,
    pub hatch_pattern: Option<String>,
    /// A dimension style the drawing has, or "" for the document default.
    pub dimension_style: Option<String>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
}

impl LayerModify {
    pub fn empty(&self) -> bool {
        self.colour.is_none()
            && self.linetype.is_none()
            && self.line_weight.is_none()
            && self.hatch_pattern.is_none()
            && self.dimension_style.is_none()
            && self.visible.is_none()
            && self.locked.is_none()
    }
}

/// Applied to the styles the matched entities wear (ByLayer is no style).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleModify {
    pub colour: Option<Option<Color>>,
    /// A model linetype, a library linestyle or "ByLayer" (the layer's).
    pub linetype: Option<String>,
    pub line_weight: Option<f64>,
    /// A hatch pattern the drawing has, or "" for ByLayer.
    pub hatch_pattern: Option<String>,
    pub symbol: Option<String>,
    pub symbol_size: Option<f64>,
    /// Multiplies each style's symbol size: 2 doubles every symbol the scope's
    /// styles draw, 0.5 halves them (the owner's "change the symbols size",
    /// 2026-11-03). Above 0. A style drawing its symbol at its own size (0) has
    /// no number to multiply and is left as it is, counted in the plan
    /// (GlobalModifyPlan::symbols_at_own_size). Not with symbol_size: a size
    /// set and scaled in one request has no one meaning.
    pub symbol_scale: Option<f64>,
}

impl StyleModify {
    pub fn empty(&self) -> bool {
        self.colour.is_none()
            && self.linetype.is_none()
            && self.line_weight.is_none()
            && self.hatch_pattern.is_none()
            && self.symbol.is_none()
            && self.symbol_size.is_none()
            && self.symbol_scale.is_none()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlobalModify {
    pub entities: EntityModify,
    pub layers: LayerModify,
    pub styles: StyleModify,
}

impl GlobalModify {
    pub fn empty(&self) -> bool {
        self.entities.empty() && self.layers.empty() && self.styles.empty()
    }
}

// ---- the plan ----------------------------------------------------------------------------

#[derive(Default)]
pub struct GlobalModifyPlan {
    /// Everything the scope and the filter took, ascending.
    pub matched: Vec<EntityId>,
    /// Of those, the entities whose own attributes change, ascending.
    pub changed: Vec<EntityId>,
    /// Left alone: on a locked layer the request does not unlock.
    pub locked: usize,
    /// Left alone by the symbol: matched, but not points.
    pub not_points: usize,
    /// Left alone by the text height: matched, but not text.
    pub not_text: usize,
    /// Left alone by the symbol scale: styles the scope reaches that draw a
    /// symbol at its own size (StyleModify::symbol_scale).
    pub symbols_at_own_size: usize,
    /// The layers and styles whose definitions change, ascending.
    pub layers_changed: Vec<String>,
    pub styles_changed: Vec<String>,
    /// Entities OUTSIDE `matched` that wear a style in styles_changed, and so
    /// are redrawn by it too.
    pub style_reaches_others: usize,
    /// A layer or style the command makes, when it does.
    pub creates_layer: Option<String>,
    pub creates_style: Option<String>,

    /// ONE undo step doing all of it; None when nothing would change, so no
    /// empty step is ever pushed.
    pub command: Option<CommandPtr>,
}

impl fmt::Debug for GlobalModifyPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GlobalModifyPlan")
            .field("matched", &self.matched)
            .field("changed", &self.changed)
            .field("locked", &self.locked)
            .field("not_points", &self.not_points)
            .field("not_text", &self.not_text)
            .field("symbols_at_own_size", &self.symbols_at_own_size)
            .field("layers_changed", &self.layers_changed)
            .field("styles_changed", &self.styles_changed)
            .field("style_reaches_others", &self.style_reaches_others)
            .field("creates_layer", &self.creates_layer)
            .field("creates_style", &self.creates_style)
            .field(
                "command",
                &self.command.as_ref().map(|command| command.name()),
            )
            .finish()
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

impl GlobalModifyPlan {
    /// "12 matched: 10 entities, 2 layers and 1 style change; 2 on a locked
    /// layer left as they are". What the dialog shows as its preview and the
    /// command line prints.
    pub fn summary(&self) -> String {
        if self.matched.is_empty() {
            return String::from(
                "Nothing matched: no entity is in the scope and passes the filter.",
            );
        }
        let mut text = counted(self.matched.len(), "entity matched", "entities matched");
        // Read from what changes, not from `command`, which is gone once it has
        // been executed and the summary is still wanted for the log.
        if self.changed.is_empty()
            && self.layers_changed.is_empty()
            && self.styles_changed.is_empty()
            && self.creates_layer.is_none()
            && self.creates_style.is_none()
        {
            text += "; nothing to change, they are already as asked";
        } else {
            let mut parts = Vec::new();
            if !self.changed.is_empty() {
                parts.push(counted(self.changed.len(), "entity", "entities"));
            }
            if !self.layers_changed.is_empty() {
                parts.push(counted(self.layers_changed.len(), "layer", "layers"));
            }
            if !self.styles_changed.is_empty() {
                parts.push(counted(self.styles_changed.len(), "style", "styles"));
            }
            let mut list = String::new();
            for (i, part) in parts.iter().enumerate() {
                list += if i == 0 {
                    ""
                } else if i + 1 == parts.len() {
                    " and "
                } else {
                    ", "
                };
                list += part;
            }
            text += "; changing ";
            text += if list.is_empty() {
                "the drawing's tables"
            } else {
                &list
            };
        }
        if let Some(layer) = &self.creates_layer {
            text += "; makes layer ";
            text += layer;
        }
        if let Some(style) = &self.creates_style {
            text += "; makes style ";
            text += style;
        }
        if self.locked > 0 {
            text += &format!(
                "; {} on a locked layer left as {}",
                counted(self.locked, "entity", "entities"),
                if self.locked == 1 {
                    "it is"
                } else {
                    "they are"
                }
            );
        }
        if self.not_points > 0 {
            text += &format!(
                "; {} not a point and draws no symbol",
                counted(self.not_points, "entity is", "entities are")
            );
        }
        if self.not_text > 0 {
            text += &format!(
                "; {} not text and has no height",
                counted(self.not_text, "entity is", "entities are")
            );
        }
        if self.symbols_at_own_size > 0 {
            text += &format!(
                "; {} with a symbol at its own size left as {}",
                counted(self.symbols_at_own_size, "style", "styles"),
                if self.symbols_at_own_size == 1 {
                    "it is: give it a size to scale"
                } else {
                    "they are: give them a size to scale"
                }
            );
        }
        if self.style_reaches_others > 0 {
            text += &format!(
                "; the changed styles also redraw {} outside the scope",
                counted(self.style_reaches_others, "entity", "entities")
            );
        }
        text + "."
    }
}

// ---- the pieces of match_entities --------------------------------------------------------

// `path` is `layer` or lies beneath it.
fn is_within(path: &str, layer: &str) -> bool {
    path == layer
        || (path.len() > layer.len()
            && path.starts_with(layer)
            && path.as_bytes()[layer.len()] == b'/')
}

// `path` and each of its ancestors, deepest last: "a/b/c" -> "a", "a/b", "a/b/c".
fn self_and_ancestors(path: &str) -> Vec<&str> {
    let mut prefixes: Vec<&str> = path.match_indices('/').map(|(at, _)| &path[..at]).collect();
    prefixes.push(path);
    prefixes
}

fn is_by_layer_name(name: &str) -> bool {
    name.is_empty() || equals_ignoring_case(name.as_bytes(), b"ByLayer")
}

// What a Text or a Dimension says, for the text filter; None for the rest.
fn text_of(entity: &Entity) -> Option<&str> {
    match &entity.geometry {
        Geometry::Text(text) => Some(text.text.as_str()),
        Geometry::Dimension(dimension) => Some(dimension.text_override.as_str()),
        _ => None,
    }
}

fn passes(model: &Model, entity: &Entity, filter: &ModifyFilter) -> bool {
    if !filter.types.is_empty() && !filter.types.contains(&entity.r#type()) {
        return false;
    }
    if !layer_passes_filter(filter, &entity.layer) {
        return false;
    }
    if let Some(style) = &filter.style {
        let wants_by_layer = is_by_layer_name(style);
        let refused = if wants_by_layer {
            !entity.style.is_empty()
        } else {
            entity.style.is_empty() || !matches_pattern(&entity.style, style)
        };
        if refused {
            return false;
        }
    }
    if let Some(colour) = &filter.colour
        && entity.color != *colour
    {
        return false;
    }
    if !properties_pass_filter(filter, &entity.properties) {
        return false;
    }
    if let Some(pattern) = &filter.text {
        match text_of(entity) {
            Some(text) if matches_pattern(text, pattern) => {}
            _ => return false,
        }
    }
    if filter.drawn_only && !is_drawn(model, entity, &NO_LAYER_OVERRIDES) {
        return false;
    }
    true
}

/// A Layers scope's layers against the drawing: InvalidArgument when it names
/// none, NotFound naming the first the drawing lacks. Nothing to check, so ok,
/// for any other kind of scope.
///
/// Not every tool acts on entities: the road objects are definitions that sit
/// on a layer and have a plan extent, and the ROADOBJECT verbs take the same
/// scope and filter. They resolve with these pieces, so the layer rules, the
/// pattern match and the property conditions are written once; match_entities
/// calls the same.
pub fn check_scope_layers(model: &Model, scope: &ModifyScope) -> Status {
    if scope.kind != ScopeKind::Layers {
        return Ok(());
    }
    if scope.layers.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "name at least one layer to change",
        ));
    }
    for name in &scope.layers {
        if !model.layers.contains(name) {
            return Err(make_error(
                ErrorCode::NotFound,
                "layer does not exist",
                name.as_str(),
            ));
        }
    }
    Ok(())
}

/// Whether a Layers scope takes what is on `layer`: the layer is one of the
/// scope's, or lies beneath one when the scope takes sublayers. False for any
/// other kind of scope, which does not decide by layer.
pub fn layer_in_scope(scope: &ModifyScope, layer: &str) -> bool {
    scope.kind == ScopeKind::Layers
        && scope.layers.iter().any(|name| {
            if scope.sublayers {
                is_within(layer, name)
            } else {
                layer == name
            }
        })
}

/// The filter's layer patterns against the full path of `layer`: any of them
/// (matches_pattern), and every layer when there are none.
pub fn layer_passes_filter(filter: &ModifyFilter, layer: &str) -> bool {
    filter.layers.is_empty()
        || filter
            .layers
            .iter()
            .any(|pattern| matches_pattern(layer, pattern))
}

/// The filter's property conditions against `properties`: with a key, a
/// property of exactly that name (and, with a value, one whose value as
/// property_to_string prints it matches the pattern); with only a value, any
/// property that does; no conditions pass anything.
pub fn properties_pass_filter(filter: &ModifyFilter, properties: &PropertyMap) -> bool {
    if let Some(key) = &filter.property {
        let Some(found) = properties.get(key) else {
            return false;
        };
        return filter
            .property_value
            .as_ref()
            .is_none_or(|pattern| matches_pattern(&property_to_string(found), pattern));
    }
    if let Some(pattern) = &filter.property_value {
        // A value with no key: any property carrying it.
        return properties
            .values()
            .any(|value| matches_pattern(&property_to_string(value), pattern));
    }
    true
}

/// What `scope` and `filter` take, ascending by id. Fails with NotFound for a
/// Layers scope naming a layer the drawing lacks, and with InvalidArgument for
/// a Layers scope naming none.
pub fn match_entities(
    document: &Document,
    scope: &ModifyScope,
    filter: &ModifyFilter,
) -> Result<Vec<EntityId>> {
    let model = document.model();
    check_scope_layers(model, scope)?;

    let mut matched = Vec::new();
    let mut consider = |entity: &Entity| {
        match scope.kind {
            ScopeKind::Selection | ScopeKind::Drawing => {}
            ScopeKind::View => {
                let hidden: &LayerOverrides = scope.view.as_deref().unwrap_or(&NO_LAYER_OVERRIDES);
                if !is_drawn(model, entity, hidden) {
                    return;
                }
                if let Some(area) = scope.area
                    && !bounding_box(&entity.geometry).intersects(area)
                {
                    return;
                }
            }
            ScopeKind::Layers => {
                if !layer_in_scope(scope, &entity.layer) {
                    return;
                }
            }
        }
        if passes(model, entity, filter) {
            matched.push(entity.id);
        }
    };

    if scope.kind == ScopeKind::Selection {
        for id in document.selection().ids() {
            if let Some(entity) = model.entities.find(id) {
                consider(entity);
            }
        }
    } else {
        model.entities.for_each(&mut consider);
    }
    matched.sort_unstable();
    Ok(matched)
}

// ---- checking and applying the request ---------------------------------------------------

fn check_positive(field: &str, value: f64) -> Status {
    if !(value.is_finite() && value > 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{field} must be above 0"),
            format_exact_real(value),
        ));
    }
    Ok(())
}

fn check_not_negative(field: &str, value: f64) -> Status {
    if !(value.is_finite() && value >= 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{field} must be 0 or more"),
            format_exact_real(value),
        ));
    }
    Ok(())
}

fn check_symbol_name(document: &Document, name: &str) -> Status {
    // The model accepts any name, since a project may be opened before its
    // library is loaded; a person choosing one is still told about a typo,
    // as STYLE SET SYMBOL tells them. "" is the plain point mark.
    if name.is_empty() || is_built_in_symbol_name(name) || document.definition_for(name).is_some() {
        return Ok(());
    }
    Err(make_error(
        ErrorCode::NotFound,
        "no symbol of that name is built in or in the loaded library",
        name,
    ))
}

// Everything the request says that the drawing would refuse, before any of
// it is planned.
fn check_request(document: &Document, change: &GlobalModify) -> Status {
    let model = document.model();
    if change.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "nothing to change: choose at least one field to set",
        ));
    }
    let entities = &change.entities;
    if let Some(layer) = &entities.layer {
        validate_layer_path(layer)?;
    }
    if let Some(style) = &entities.style
        && !style.is_empty()
        && !model.styles.contains(style)
    {
        return Err(make_error(
            ErrorCode::NotFound,
            "style does not exist",
            style.as_str(),
        ));
    }
    for (key, value) in &entities.set_properties {
        if key.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a property needs a name",
            ));
        }
        model.properties.validate(key, value)?;
    }
    if let Some(height) = entities.text_height {
        check_positive("text height", height)?;
    }
    if let Some(symbol) = &entities.symbol {
        if symbol.name.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "name the symbol to draw at the points",
            ));
        }
        check_symbol_name(document, &symbol.name)?;
        check_not_negative("symbol size", symbol.size)?;
    }

    let layers = &change.layers;
    if let Some(linetype) = &layers.linetype {
        check_linetype_name(document, linetype)?;
    }
    if let Some(weight) = layers.line_weight {
        check_not_negative("line weight", weight)?;
    }
    if let Some(pattern) = &layers.hatch_pattern
        && !model.hatch_patterns.contains(pattern)
    {
        return Err(make_error(
            ErrorCode::NotFound,
            "hatch pattern does not exist",
            pattern.as_str(),
        ));
    }
    if let Some(style) = &layers.dimension_style
        && !style.is_empty()
        && !model.dimension_styles.contains(style)
    {
        return Err(make_error(
            ErrorCode::NotFound,
            "dimension style does not exist",
            style.as_str(),
        ));
    }

    let styles = &change.styles;
    if let Some(linetype) = &styles.linetype
        && !is_by_layer(linetype)
    {
        check_linetype_name(document, linetype)?;
    }
    if let Some(weight) = styles.line_weight {
        check_not_negative("line weight", weight)?;
    }
    if let Some(pattern) = &styles.hatch_pattern
        && !pattern.is_empty()
        && !model.hatch_patterns.contains(pattern)
    {
        return Err(make_error(
            ErrorCode::NotFound,
            "hatch pattern does not exist",
            pattern.as_str(),
        ));
    }
    if let Some(symbol) = &styles.symbol {
        check_symbol_name(document, symbol)?;
    }
    if let Some(size) = styles.symbol_size {
        check_not_negative("symbol size", size)?;
    }
    if let Some(scale) = styles.symbol_scale {
        if styles.symbol_size.is_some() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "set the symbols' size or scale it, not both in one change",
            ));
        }
        // Above 0: a scale of 0 would make every size "own size", which is
        // another request (a symbol size of 0).
        check_positive("symbol scale", scale)?;
    }
    Ok(())
}

fn changed_layer(mut layer: Layer, change: &LayerModify) -> Layer {
    if let Some(colour) = change.colour {
        layer.color = colour;
    }
    if let Some(linetype) = &change.linetype {
        layer.linetype.clone_from(linetype);
    }
    if let Some(weight) = change.line_weight {
        layer.line_weight = weight;
    }
    if let Some(pattern) = &change.hatch_pattern {
        layer.hatch_pattern.clone_from(pattern);
    }
    if let Some(style) = &change.dimension_style {
        layer.dimension_style.clone_from(style);
    }
    if let Some(visible) = change.visible {
        layer.visible = visible;
    }
    // A lock waits for the end, after the entities on the layer are changed
    // (see plan_global_modify); an unlock comes first, so they can be.
    if change.locked == Some(false) {
        layer.locked = false;
    }
    layer
}

fn changed_style(mut style: Style, change: &StyleModify) -> Style {
    if let Some(colour) = change.colour {
        style.color = colour;
    }
    if let Some(linetype) = &change.linetype {
        style.linetype = if is_by_layer(linetype) {
            String::from(BY_LAYER_LINETYPE)
        } else {
            linetype.clone()
        };
    }
    if let Some(weight) = change.line_weight {
        style.line_weight = weight;
    }
    if let Some(pattern) = &change.hatch_pattern {
        style.hatch_pattern.clone_from(pattern);
    }
    if let Some(symbol) = &change.symbol {
        style.symbol.clone_from(symbol);
    }
    if let Some(size) = change.symbol_size {
        style.symbol_size = size;
    }
    // A size of 0 is the symbol's own size, which has no number to multiply.
    if let Some(scale) = change.symbol_scale
        && style.symbol_size > 0.0
    {
        style.symbol_size *= scale;
    }
    style
}

// The entity with the request's own attributes, the symbol aside (that goes
// through a style, after), and how it stands against what the request asks.
struct EntityOutcome {
    entity: Entity,
    /// The request sets a text height and this is not text.
    text_asked: bool,
}

fn changed_entity(original: &Entity, change: &EntityModify) -> EntityOutcome {
    let mut entity = original.clone();
    let mut text_asked = false;
    if let Some(layer) = &change.layer {
        entity.layer.clone_from(layer);
    }
    if let Some(colour) = change.colour {
        entity.color = colour;
    }
    if let Some(style) = &change.style {
        entity.style.clone_from(style);
    }
    if let Some(visible) = change.visible {
        entity.visible = visible;
    }
    for key in &change.remove_properties {
        entity.properties.remove(key);
    }
    for (key, value) in &change.set_properties {
        entity.properties.insert(key.clone(), value.clone());
    }
    if let Some(height) = change.text_height {
        if let Geometry::Text(text) = &mut entity.geometry {
            text.height = height;
        } else {
            text_asked = true;
        }
    }
    EntityOutcome { entity, text_asked }
}

/// The command and its report. Fails, changing nothing, for a request the
/// drawing would refuse - a style, linetype, hatch pattern or dimension style
/// it does not have, a layer name that is not a valid path, a weight, height
/// or size that is not a positive finite number (a weight and a symbol size
/// may be 0) - naming the field. Asking for no change at all is refused too:
/// it is a mistake in the request, not a request to do nothing. Matching
/// nothing is not a failure: the plan says so and carries no command.
pub fn plan_global_modify(
    document: &Document,
    scope: &ModifyScope,
    filter: &ModifyFilter,
    change: &GlobalModify,
) -> Result<GlobalModifyPlan> {
    check_request(document, change)?;
    let matched = match_entities(document, scope, filter)?;
    let model = document.model();
    let mut plan = GlobalModifyPlan {
        matched,
        ..GlobalModifyPlan::default()
    };

    // ---- the layers and styles whose definitions change ------------------------------------
    let mut target_layers: BTreeSet<String> = BTreeSet::new();
    if !change.layers.empty() {
        if scope.kind == ScopeKind::Layers {
            for name in &scope.layers {
                if scope.sublayers {
                    target_layers.extend(model.layers.subtree(name));
                } else {
                    target_layers.insert(name.clone());
                }
            }
        } else {
            for &id in &plan.matched {
                if let Some(entity) = model.entities.find(id) {
                    target_layers.insert(entity.layer.clone());
                }
            }
        }
    }
    let mut target_styles: BTreeSet<String> = BTreeSet::new();
    if !change.styles.empty() {
        for &id in &plan.matched {
            if let Some(entity) = model.entities.find(id)
                && !entity.style.is_empty()
                && model.styles.contains(&entity.style)
            {
                target_styles.insert(entity.style.clone());
            }
        }
    }

    // Phase 1: the layers (an unlock among them) and the styles.
    let mut first: Vec<CommandPtr> = Vec::new();
    let mut layers_to_lock: Vec<String> = Vec::new();
    for name in &target_layers {
        let Some(layer) = model.layers.find(name) else {
            continue;
        };
        let changed = changed_layer(layer.clone(), &change.layers);
        let locks = change.layers.locked == Some(true) && !layer.locked;
        if changed != *layer || locks {
            plan.layers_changed.push(name.clone());
        }
        if changed != *layer {
            first.push(update_layer(changed));
        }
        if locks {
            layers_to_lock.push(name.clone());
        }
    }
    for name in &target_styles {
        let Some(style) = model.styles.find(name) else {
            continue;
        };
        if change.styles.symbol_scale.is_some()
            && style.symbol != NO_SYMBOL
            && style.symbol_size == 0.0
        {
            plan.symbols_at_own_size += 1;
        }
        let changed = changed_style(style.clone(), &change.styles);
        // A finite size times a finite scale can still overflow.
        if !changed.symbol_size.is_finite() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the scaled symbol size is too large to hold",
                name.as_str(),
            ));
        }
        if let Some(command) = update_style_if_changed(model, changed) {
            first.push(command);
            plan.styles_changed.push(name.clone());
        }
    }
    if !plan.styles_changed.is_empty() {
        let changed_styles: BTreeSet<&str> =
            plan.styles_changed.iter().map(String::as_str).collect();
        let matched = &plan.matched;
        let mut reaches = 0;
        model.entities.for_each(|entity| {
            if changed_styles.contains(entity.style.as_str())
                && matched.binary_search(&entity.id).is_err()
            {
                reaches += 1;
            }
        });
        plan.style_reaches_others = reaches;
    }

    // Whether a layer is locked once phase 1 has run: it or an ancestor is,
    // counting the request's unlock of the target layers.
    let unlocks = change.layers.locked == Some(false);
    let locked_after_phase_one = |path: &str| -> bool {
        for prefix in self_and_ancestors(path) {
            if unlocks && target_layers.contains(prefix) {
                continue;
            }
            if model.layers.find(prefix).is_some_and(|layer| layer.locked) {
                return true;
            }
        }
        false
    };

    // ---- the entities --------------------------------------------------------------------
    let wanted = &change.entities;
    // A new layer's ancestors are made with it, unlocked, so only an ancestor
    // that already exists can lock it.
    if let Some(layer) = &wanted.layer
        && locked_after_phase_one(layer)
    {
        return Err(make_error(
            ErrorCode::InvalidState,
            if model.layers.contains(layer) {
                "the layer to move them onto is locked"
            } else {
                "the layer to move them onto would sit under a locked layer"
            },
            layer.as_str(),
        ));
    }

    let mut changes = ChangeSet::default();
    let mut points: Vec<EntityId> = Vec::new();
    for &id in &plan.matched {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        if locked_after_phase_one(&entity.layer) {
            plan.locked += 1;
            continue;
        }
        let outcome = changed_entity(entity, wanted);
        if outcome.text_asked {
            plan.not_text += 1;
        }
        if wanted.symbol.is_some() {
            if entity.r#type() == EntityType::Point {
                points.push(id);
            } else {
                plan.not_points += 1;
            }
        }
        if outcome.entity != *entity {
            changes.modify.push(outcome.entity);
        }
    }
    // Made only when something moves onto it: a request whose every match
    // was left alone must not leave an empty layer behind.
    if let Some(layer) = &wanted.layer
        && !model.layers.contains(layer)
        && !changes.modify.is_empty()
    {
        plan.creates_layer = Some(layer.clone());
    }

    // The symbol goes through a style that draws it, found or made
    // (symbol_assign.rs), asked of the drawing as it is now. Of what the
    // entity step changes, only a style bears on it - and a point given a
    // style as well as a symbol ends in the symbol's style, as the module
    // comment says.
    let mut symbol_command: Option<CommandPtr> = None;
    let mut changed: Vec<EntityId> = changes.modify.iter().map(|entity| entity.id).collect();
    if let Some(symbol) = &wanted.symbol
        && !points.is_empty()
    {
        let assignment = assign_symbol_to_points(document, &points, &symbol.name, symbol.size)?;
        if assignment.creates_style {
            plan.creates_style = Some(assignment.style.clone());
        }
        symbol_command = assignment.command;
        // A request that also names a style has just moved every point into
        // it, the symbol's style included; all of them are moved back.
        let restyled = wanted
            .style
            .as_ref()
            .is_some_and(|style| *style != assignment.style);
        if restyled {
            let mut both = Transaction::new("ASSIGN_SYMBOL");
            if let Some(command) = symbol_command.take() {
                both.add(command);
            }
            both.add(set_entity_style(points.clone(), assignment.style.clone()));
            symbol_command = Some(Box::new(both));
        }
        for &id in &points {
            if restyled
                || model
                    .entities
                    .find(id)
                    .is_some_and(|entity| entity.style != assignment.style)
            {
                changed.push(id);
            }
        }
    }
    changed.sort_unstable();
    changed.dedup();
    plan.changed = changed;

    // ---- one command -----------------------------------------------------------------------
    let mut transaction = Transaction::new("GLOBAL_MODIFY");
    for command in first {
        transaction.add(command);
    }
    if let Some(name) = &plan.creates_layer {
        transaction.add(create_layer(Layer {
            name: name.clone(),
            ..Layer::default()
        }));
    }
    if !changes.modify.is_empty() {
        transaction.add(Box::new(ChangeSetCommand::new(
            "GLOBAL_MODIFY_ENTITIES",
            move |_: &CommandContext<'_>| Ok(changes.clone()),
            false,
        )));
    }
    if let Some(command) = symbol_command {
        transaction.add(command);
    }
    for name in &layers_to_lock {
        if let Some(layer) = model.layers.find(name) {
            let mut locked = changed_layer(layer.clone(), &change.layers);
            locked.locked = true;
            transaction.add(update_layer(locked));
        }
    }
    if transaction.size() > 0 {
        plan.command = Some(Box::new(transaction));
    }
    Ok(plan)
}
