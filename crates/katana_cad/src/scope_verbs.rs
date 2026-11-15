//! The scope and filter words every verb that reads or changes drawing data
//! takes (docs/cad.md, "Scope and filter: one grammar for every verb on
//! drawing data"; include/katana/cad/scope_verbs.hpp,
//! src/katana_cad/scope_verbs.cpp): the owner's request of 2026-09-26, "i want
//! the utility tools to act on data on view, layer/s, elements, filtered
//! elements, like global change ... all tools need to have the ability to act
//! that way".
//!
//! ONE grammar, read by ONE parser, resolved by the one matcher Global Modify
//! already has (match_entities, global_modify.rs), so MODIFY, the UTILITY
//! verbs and every verb after them take a scope the same way:
//!
//! ```text
//! SELECTION | DRAWING | VIEW [<view id>] [EXTENTS] | AREA x0,y0,x1,y1 | LAYERS a,b[,c] [ONLY]
//! then [WHERE key=value ...]
//! ```
//!
//! VIEW is what the view shows now: its own hidden layers and the area on
//! screen. VIEW ... EXTENTS drops the area - what the view draws anywhere, as
//! it would show it zoomed to its extents - which is Global Modify's "What a
//! view shows" without "Only what is on screen"; without the word, the window's
//! controls said something no line could.
//!
//! Words are case-insensitive; SEL, ALL and LAYER are accepted for SELECTION,
//! DRAWING and LAYERS, as MODIFY accepted them. No scope word is the selection.
//! The WHERE keys are MODIFY's: TYPE=point,line LAYER=pat[,pat] STYLE=pat|ByLayer
//! COLOUR=#RRGGBB|ByLayer PROP=key[:pat] TEXT=pat DRAWN, with '*' and '?'
//! wildcards (matches_pattern). PROP=:pat is a value any property may hold, as
//! Global Modify's controls say it with the property left empty.
//!
//! A view id is the window's ViewId, which is not the number in a view's
//! title ("Plan 2" is numbered among the plan views, the id among all views):
//! a reply on VIEW says it (view=), and the "Apply to" list shows it.
//!
//! It is read in two steps so each can be tested alone and a front end can
//! build the words without a document:
//!
//! ```text
//! parse_scope_words   words -> ScopeWords: plain data, nothing looked up.
//! resolve_scope       ScopeWords -> a ModifyScope and ModifyFilter, asking
//!                     the window for a VIEW.
//! ```
//!
//! VIEW needs the window: the Document holds no views, so the front end that
//! has them answers for the active plan view, or the view with that id, with
//! its own hidden layers and its visible area (ScopeViewProvider). Headless
//! there is no view, and VIEW is refused naming AREA, which is the same scope
//! with the window typed in: a view that hides nothing of its own, looking at
//! that box. That is why AREA is a View scope with an area and no view layers
//! rather than a kind of its own - match_entities already reads a View scope
//! exactly so, and a second reading would be a second implementation.
//!
//! # The Rust shape of ScopeViewProvider (decided by the port, wave G1)
//!
//! The C++ provider is a `std::function` the interpreter keeps (an empty one
//! is headless) and every verb borrows as `const ScopeViewProvider&`. Here a
//! provider is BORROWED for the call: `ScopeViewProvider<'a>` is
//! `Option<&'a ScopeViewFn<'a>>`, None being headless, and a ScopeViewFn is
//! any `Fn(Option<u32>) -> Result<ScopeView>`. So a test or a front end passes
//! a closure that reads what it has - a ViewSet it owns, a list of answers -
//! with no `'static`, `Send` or shared ownership forced on it, and a verb that
//! hands the provider on copies it (it is `Copy`). What keeps one for longer
//! (the interpreter's scope context) keeps a `Box<ScopeViewFn<'static>>` and
//! lends `Some(&*boxed)`; a window keeps nothing that borrows its views, and
//! answers through scope_view_of over its ViewSet at the moment of the call.
//! Rejected: `Arc<dyn Fn + Send + Sync>`, which made every caller's closure
//! own what it reads (a test's list of the ids asked, a window's ViewSet) in
//! shared, locked storage for the length of a call that only borrows it.

use std::sync::Arc;

use katana_core::error::make_error;
use katana_core::text::{
    format_exact_real, is_ascii_space, parse_finite_double, parse_integer, uppered,
};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::entity::{ENTITY_TYPES, GEOMETRY_KIND_COUNT, entity_type_from_string};
use katana_entity::{Color, EntityId, EntityType, EnumName, Model};
use katana_geometry::{Box2, Point2};
use katana_math::cpp;

use crate::document::Document;
use crate::global_modify::{ModifyFilter, ModifyScope, ScopeKind, match_entities};
use crate::layer_overrides::LayerOverrides;
use crate::view_set::{ViewId, ViewSet};
use crate::view_transform::ViewTransform;
use crate::viewport_layout::ViewKind;

// ScopeWords::view and ScopeView::id hold a ViewId: the C++ static_assert.
const _: () = assert!(size_of::<ViewId>() == size_of::<u32>());

/// Which scope word was given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScopeSource {
    #[default]
    Selection,
    Drawing,
    View,
    Area,
    Layers,
}

/// The scope and filter words as read: plain data, nothing looked up. What a
/// front end fills to say a scope (format_scope_words) and what the parser
/// gives.
#[derive(Clone, Debug, PartialEq)]
pub struct ScopeWords {
    pub source: ScopeSource,
    /// VIEW <id>: that view (a ViewId); None: the active plan view.
    pub view: Option<u32>,
    /// VIEW ... EXTENTS: the view's own hidden layers without its area.
    pub extents: bool,
    /// AREA: min before max, whichever corners were typed first.
    pub area: Box2,
    /// LAYERS: the layer paths, each of which must exist, and whether each
    /// takes the layers beneath it (no ONLY).
    pub layers: Vec<String>,
    pub sublayers: bool,
    /// WHERE.
    pub filter: ModifyFilter,
}

impl Default for ScopeWords {
    fn default() -> Self {
        Self {
            source: ScopeSource::Selection,
            view: None,
            extents: false,
            area: Box2::default(),
            layers: Vec::new(),
            sublayers: true,
            filter: ModifyFilter::default(),
        }
    }
}

// "a,b,,c" -> a b c: an empty item is a stray comma, not a name.
fn split_list(text: &str) -> Vec<String> {
    text.split(',')
        .filter(|part| !part.is_empty())
        .map(String::from)
        .collect()
}

// A WHERE condition: a word holding '=', or DRAWN.
fn is_condition_word(word: &str) -> bool {
    uppered(word) == b"DRAWN" || word.contains('=')
}

/// One WHERE condition - key=value or DRAWN - into `filter`: what
/// parse_scope_words reads after WHERE, for a verb whose own words may take the
/// filter up again, as MODIFY's WHERE after SET does. ParseFailure naming the
/// word for a word that is no condition ("a WHERE condition is key=value"), a
/// key that is no WHERE key, or a value that does not read.
pub fn parse_where_condition(word: &str, filter: &mut ModifyFilter) -> Status {
    if uppered(word) == b"DRAWN" {
        filter.drawn_only = true;
        return Ok(());
    }
    let Some(equals) = word.find('=') else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "a WHERE condition is key=value",
            word,
        ));
    };
    let key = uppered(&word[..equals]);
    let value = &word[equals + 1..];
    match key.as_slice() {
        b"TYPE" => {
            for name in split_list(value) {
                filter.types.insert(entity_type_from_string(&name)?);
            }
        }
        b"LAYER" => filter.layers = split_list(value),
        b"STYLE" => filter.style = Some(String::from(value)),
        b"COLOUR" | b"COLOR" => filter.colour = Some(parse_colour_or_by_layer(value)?),
        b"PROP" => {
            // PROP=key, PROP=key:pat, and PROP=:pat - a value any property of
            // the entity may hold, as Global Modify's "Property" left empty with
            // a value takes it. The key ends at the first ':'.
            let colon = value.find(':');
            if colon == Some(0) {
                filter.property = None;
            } else {
                filter.property = Some(String::from(&value[..colon.unwrap_or(value.len())]));
            }
            if let Some(colon) = colon {
                filter.property_value = Some(String::from(&value[colon + 1..]));
            }
        }
        b"TEXT" => filter.text = Some(String::from(value)),
        _ => {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "not a WHERE key: TYPE LAYER STYLE COLOUR PROP TEXT or DRAWN",
                word,
            ));
        }
    }
    Ok(())
}

// "x0,y0,x1,y1", corners in either order.
fn parse_area(text: &str) -> Result<Box2> {
    let refuse = || {
        make_error(
            ErrorCode::ParseFailure,
            "AREA is x0,y0,x1,y1: four numbers",
            text,
        )
    };
    let mut numbers = Vec::new();
    for part in text.split(',') {
        numbers.push(parse_finite_double(part).ok_or_else(refuse)?);
    }
    let [x0, y0, x1, y1] = numbers[..] else {
        return Err(refuse());
    };
    Ok(Box2::new(
        Point2::new(cpp::min(x0, x1), cpp::min(y0, y1)),
        Point2::new(cpp::max(x0, x1), cpp::max(y0, y1)),
    ))
}

fn all_digits(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit())
}

// The filter as the words WHERE takes, unquoted.
fn condition_words(filter: &ModifyFilter) -> Result<Vec<String>> {
    let mut words = Vec::new();
    if !filter.types.is_empty() {
        let names: Vec<String> = filter
            .types
            .iter()
            .map(|entity_type| entity_type.name().to_ascii_lowercase())
            .collect();
        words.push(format!("TYPE={}", names.join(",")));
    }
    if !filter.layers.is_empty() {
        for pattern in &filter.layers {
            if pattern.is_empty() || pattern.contains(',') {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a layer pattern with a comma, or none, cannot be written in a LAYER= list",
                    pattern.as_str(),
                ));
            }
        }
        words.push(format!("LAYER={}", filter.layers.join(",")));
    }
    if let Some(style) = &filter.style {
        words.push(format!("STYLE={style}"));
    }
    if let Some(colour) = &filter.colour {
        words.push(format!(
            "COLOUR={}",
            match colour {
                Some(colour) => colour.to_hex(),
                None => String::from("ByLayer"),
            }
        ));
    }
    if let Some(property) = &filter.property
        && property.contains(':')
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a property whose name has a ':' in it cannot be written as PROP=, which ends the \
             name at the first ':'",
            property.as_str(),
        ));
    }
    if filter
        .property
        .as_ref()
        .is_some_and(|property| property.is_empty())
        && filter.property_value.is_some()
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a value of a property with no name cannot be written: PROP=:pat is a value of any \
             property",
        ));
    }
    if filter.property.is_some() || filter.property_value.is_some() {
        // No property with a value is a value of any: PROP=:pat.
        words.push(format!(
            "PROP={}{}",
            filter.property.as_deref().unwrap_or_default(),
            match &filter.property_value {
                Some(value) => format!(":{value}"),
                None => String::new(),
            }
        ));
    }
    if let Some(text) = &filter.text {
        words.push(format!("TEXT={text}"));
    }
    if filter.drawn_only {
        words.push(String::from("DRAWN"));
    }
    Ok(words)
}

fn source_word(source: ScopeSource) -> &'static str {
    match source {
        ScopeSource::Selection => "selection",
        ScopeSource::Drawing => "drawing",
        ScopeSource::View => "view",
        ScopeSource::Area => "area",
        ScopeSource::Layers => "layers",
    }
}

fn area_text(area: &Box2) -> String {
    format!(
        "{},{},{},{}",
        format_exact_real(area.min.x),
        format_exact_real(area.min.y),
        format_exact_real(area.max.x),
        format_exact_real(area.max.y)
    )
}

/// True for a word that begins a scope - SELECTION, SEL, DRAWING, ALL, VIEW,
/// AREA, LAYERS, LAYER - or its filter, WHERE; any case. What a verb that also
/// takes a file asks of its first word: a file whose name is one of these is
/// given with its directory ("./drawing").
pub fn is_scope_word(word: &str) -> bool {
    let folded = uppered(word);
    [
        "SELECTION",
        "SEL",
        "DRAWING",
        "ALL",
        "VIEW",
        "AREA",
        "LAYERS",
        "LAYER",
        "WHERE",
    ]
    .iter()
    .any(|name| folded == name.as_bytes())
}

/// True for the key of a WHERE condition - TYPE, LAYER, STYLE, COLOUR (COLOR),
/// PROP, TEXT - in any case. After WHERE a verb's own key=value option of the
/// same name is the condition, not the option: a verb that reads its options
/// wherever they stand asks this.
pub fn is_where_key(key: &str) -> bool {
    // The keys parse_where_condition reads.
    let folded = uppered(key);
    ["TYPE", "LAYER", "STYLE", "COLOUR", "COLOR", "PROP", "TEXT"]
        .iter()
        .any(|name| folded == name.as_bytes())
}

/// "#RRGGBB" or ByLayer (any case), as an entity's or a style's colour says it:
/// the inner None is ByLayer. MODIFY's SET COLOUR= reads it too.
pub fn parse_colour_or_by_layer(text: &str) -> Result<Option<Color>> {
    if uppered(text) == b"BYLAYER" {
        return Ok(None);
    }
    Ok(Some(Color::from_hex(text)?))
}

/// Reads the scope and filter words of `words` from `at` and leaves `at` at
/// the first word that is neither, where the verb's own words begin. A scope
/// word is read only before WHERE; after WHERE, a word holding '=' or DRAWN is
/// a condition and anything else ends the filter.
///
/// Refused, naming the word: two scope words; LAYERS with no layer after it;
/// ONLY other than after a LAYERS list; EXTENTS other than after VIEW [<id>];
/// AREA without four finite numbers; VIEW with an id of 0; a WHERE word with
/// '=' whose key is not a WHERE key, or whose value does not read (TYPE=blob,
/// COLOUR=red).
pub fn parse_scope_words(words: &[String], at: &mut usize) -> Result<ScopeWords> {
    let mut result = ScopeWords::default();
    let mut named = false;
    let mut i = *at;
    while i < words.len() {
        let word = &words[i];
        let folded = uppered(word);
        match folded.as_slice() {
            b"WHERE" => break,
            b"ONLY" => {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "ONLY follows a LAYERS list",
                    word.as_str(),
                ));
            }
            b"EXTENTS" => {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "EXTENTS follows VIEW [<view id>]",
                    word.as_str(),
                ));
            }
            b"SELECTION" | b"SEL" | b"DRAWING" | b"ALL" | b"VIEW" | b"AREA" | b"LAYERS"
            | b"LAYER" => {}
            _ => break,
        }
        if named {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "give one scope: SELECTION, DRAWING, VIEW, AREA or LAYERS",
                word.as_str(),
            ));
        }
        named = true;
        i += 1;
        match folded.as_slice() {
            b"SELECTION" | b"SEL" => result.source = ScopeSource::Selection,
            b"DRAWING" | b"ALL" => result.source = ScopeSource::Drawing,
            b"VIEW" => {
                result.source = ScopeSource::View;
                if i < words.len() && all_digits(&words[i]) {
                    let id = parse_integer(&words[i]);
                    let Some(id) = id.filter(|id| (1..=i64::from(u32::MAX)).contains(id)) else {
                        return Err(make_error(
                            ErrorCode::ParseFailure,
                            "a view id is a whole number from 1: the id a reply's view= gives, \
                             and the Apply to list shows as VIEW <id> - not the number in the \
                             view's title",
                            words[i].as_str(),
                        ));
                    };
                    result.view = u32::try_from(id).ok();
                    i += 1;
                }
                if i < words.len() && uppered(&words[i]) == b"EXTENTS" {
                    result.extents = true;
                    i += 1;
                }
            }
            b"AREA" => {
                if i >= words.len() {
                    return Err(make_error(
                        ErrorCode::ParseFailure,
                        "AREA is x0,y0,x1,y1: four numbers",
                        word.as_str(),
                    ));
                }
                result.area = parse_area(&words[i])?;
                result.source = ScopeSource::Area;
                i += 1;
            }
            _ => {
                if i >= words.len() || split_list(&words[i]).is_empty() {
                    return Err(make_error(
                        ErrorCode::ParseFailure,
                        "LAYERS needs the layers: LAYERS a,b[,c] [ONLY]",
                        word.as_str(),
                    ));
                }
                result.source = ScopeSource::Layers;
                result.layers = split_list(&words[i]);
                i += 1;
                if i < words.len() && uppered(&words[i]) == b"ONLY" {
                    result.sublayers = false;
                    i += 1;
                }
            }
        }
    }
    // WHERE, and its conditions up to the first word that is not one. A
    // second WHERE reads on, so "WHERE TYPE=point WHERE DRAWN" is one filter.
    if i < words.len() && uppered(&words[i]) == b"WHERE" {
        i += 1;
        while i < words.len() {
            if uppered(&words[i]) == b"WHERE" {
                i += 1;
                continue;
            }
            if !is_condition_word(&words[i]) {
                break;
            }
            parse_where_condition(&words[i], &mut result.filter)?;
            i += 1;
        }
    }
    *at = i;
    Ok(result)
}

/// The words that say `words` back, as a line takes them: parse_scope_words
/// reads the result as `words`. The scope word is always written, so the
/// result can begin a UTILITY line; a word with a blank in it is quoted.
/// InvalidArgument for what a line cannot say: a double quote anywhere (the
/// command line has no way to type one inside a word), a comma in a layer name
/// or pattern (it separates the list), an empty layer list, a ':' in a
/// property's name (PROP= ends the name at the first one), a value of a
/// property whose name is empty (PROP=:pat is a value of any property).
pub fn format_scope_words(words: &ScopeWords) -> Result<String> {
    let mut parts: Vec<String> = Vec::new();
    match words.source {
        ScopeSource::Selection => parts.push(String::from("SELECTION")),
        ScopeSource::Drawing => parts.push(String::from("DRAWING")),
        ScopeSource::View => {
            parts.push(String::from("VIEW"));
            if let Some(view) = words.view {
                parts.push(format!("{view}"));
            }
            if words.extents {
                parts.push(String::from("EXTENTS"));
            }
        }
        ScopeSource::Area => {
            if words.area.empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "an AREA needs a box",
                ));
            }
            parts.push(String::from("AREA"));
            parts.push(area_text(&words.area));
        }
        ScopeSource::Layers => {
            if words.layers.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "a LAYERS scope needs a layer",
                ));
            }
            for layer in &words.layers {
                if layer.is_empty() || layer.contains(',') {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "a layer name with a comma, or none, cannot be written in a LAYERS list",
                        layer.as_str(),
                    ));
                }
            }
            parts.push(String::from("LAYERS"));
            parts.push(words.layers.join(","));
            if !words.sublayers {
                parts.push(String::from("ONLY"));
            }
        }
    }
    let conditions = condition_words(&words.filter)?;
    if !conditions.is_empty() {
        parts.push(String::from("WHERE"));
        parts.extend(conditions);
    }

    let mut line = String::new();
    for part in &parts {
        if part.contains(['"', '\r', '\n']) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a double quote or a line break cannot be typed inside a word",
                part.as_str(),
            ));
        }
        let blank = part.bytes().any(is_ascii_space);
        if !line.is_empty() {
            line.push(' ');
        }
        if blank || part.is_empty() {
            line.push('"');
            line.push_str(part);
            line.push('"');
        } else {
            line.push_str(part);
        }
    }
    Ok(line)
}

// ---- the window's side ---------------------------------------------------------------------

/// What the window knows of a view that the Document does not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScopeView {
    /// Its ViewId, for the reply.
    pub id: u32,
    /// The layers it hides of its own (ViewState::layers).
    pub layers: LayerOverrides,
    /// What it shows: ViewTransform::visible_world_bounds. None: everything it
    /// draws, wherever it lies - a view with no plan extent (3D, a section)
    /// answers so. VIEW ... EXTENTS leaves it out whatever the window says.
    pub area: Option<Box2>,
    /// A plan view's whole transform - its size in pixels and its scale - when
    /// it has one, with `area`. A verb that says what the view DRAWS (SURFACE
    /// PLAN VIEW) needs the pixels per unit the level of detail is judged at,
    /// which the box alone does not give: the same box is 400 pixels wide or
    /// 1600.
    pub frame: Option<ViewTransform>,
}

/// Answers VIEW: the active plan view when `id` is None, else the view with
/// that id, of any kind. A front end fails with NotFound naming an id no open
/// view has, and InvalidState when no id is given and no plan view is open.
pub type ScopeViewFn<'a> = dyn Fn(Option<u32>) -> Result<ScopeView> + 'a;

/// A provider borrowed for the call; None is headless (the C++'s empty
/// std::function). See the module comment for why it is borrowed.
pub type ScopeViewProvider<'a> = Option<&'a ScopeViewFn<'a>>;

/// The answer of a front end that keeps its views in a ViewSet - the window's
/// workspace - so the rule is here, tested, and not in a widget: no id is the
/// plan view the Plot and the Standard Views act on (ViewSet::most_recent); an
/// id is that view, of any kind. A plan view answers its own hidden layers and
/// ViewTransform::visible_world_bounds; a view with no plan extent (3D, a
/// section, an elevation) its hidden layers only. NotFound for an id no open
/// view has; InvalidState when no plan view is open and none is named - each
/// naming AREA, which needs no window.
pub fn scope_view_of(views: &ViewSet, id: Option<u32>) -> Result<ScopeView> {
    let state = match id {
        Some(id) => views.find(id).ok_or_else(|| {
            make_error(
                ErrorCode::NotFound,
                "no open view has that id; VIEW alone is the plan view in use, and AREA \
                 x0,y0,x1,y1 names a window without one",
                format!("{id}"),
            )
        })?,
        None => views.most_recent(ViewKind::Plan).ok_or_else(|| {
            Error::new(
                ErrorCode::InvalidState,
                "VIEW is the plan view in use, and no plan view is open; open one, or give the \
                 window instead: AREA x0,y0,x1,y1",
            )
        })?,
    };
    let mut answer = ScopeView {
        id: state.id,
        layers: state.layers.clone(),
        ..ScopeView::default()
    };
    if state.kind == ViewKind::Plan {
        answer.area = Some(state.plan.visible_world_bounds());
        answer.frame = Some(state.plan);
    }
    Ok(answer)
}

/// The words resolved to what match_entities takes. It holds the view's
/// layers, which `scope.view` shares, so a copy or a move keeps `scope` good.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResolvedScope {
    pub words: ScopeWords,
    pub scope: ModifyScope,
    pub filter: ModifyFilter,
    /// VIEW: the view it resolved to.
    pub view: Option<u32>,
    /// VIEW: its plan transform (ScopeView::frame), when it has one.
    pub frame: Option<ViewTransform>,
    pub view_layers: Option<Arc<LayerOverrides>>,
}

/// VIEW asks `views`; with no provider (headless) it is refused, naming AREA.
/// Nothing else is looked up here: a layer the drawing lacks is
/// match_entities' refusal.
pub fn resolve_scope(words: &ScopeWords, views: ScopeViewProvider<'_>) -> Result<ResolvedScope> {
    let mut resolved = ResolvedScope {
        words: words.clone(),
        filter: words.filter.clone(),
        ..ResolvedScope::default()
    };
    let scope = &mut resolved.scope;
    match words.source {
        ScopeSource::Selection => scope.kind = ScopeKind::Selection,
        ScopeSource::Drawing => scope.kind = ScopeKind::Drawing,
        ScopeSource::Layers => {
            scope.kind = ScopeKind::Layers;
            scope.layers.clone_from(&words.layers);
            scope.sublayers = words.sublayers;
        }
        ScopeSource::Area => {
            // What a view hiding nothing of its own shows in that window.
            scope.kind = ScopeKind::View;
            scope.area = Some(words.area);
        }
        ScopeSource::View => {
            let Some(views) = views else {
                return Err(Error::new(
                    ErrorCode::InvalidState,
                    "VIEW is the window's plan view, and there is none here; give the window \
                     instead: AREA x0,y0,x1,y1",
                ));
            };
            let view = views(words.view)?;
            let layers = Arc::new(view.layers);
            scope.kind = ScopeKind::View;
            scope.view = Some(Arc::clone(&layers));
            if !words.extents {
                scope.area = view.area;
            }
            resolved.view_layers = Some(layers);
            resolved.view = Some(view.id);
            resolved.frame = view.frame;
        }
    }
    Ok(resolved)
}

/// What a scope took, for a reply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScopeMatch {
    pub resolved: ResolvedScope,
    /// Ascending, as match_entities gives them.
    pub matched: Vec<EntityId>,
}

/// resolve_scope, then match_entities. Taking nothing is not a failure: a scope
/// that matches nothing is reported, as Global Modify's preview reports it.
pub fn match_scope(
    document: &Document,
    words: &ScopeWords,
    views: ScopeViewProvider<'_>,
) -> Result<ScopeMatch> {
    let resolved = resolve_scope(words, views)?;
    let matched = match_entities(document, &resolved.scope, &resolved.filter)?;
    Ok(ScopeMatch { resolved, matched })
}

/// The record a reply carries to say what the scope took, key=value:
///
/// ```text
/// scope=drawing matched=21
/// scope=layers layers=utilities/water sublayers=no where="PROP=utility.line:W1" matched=6
/// scope=view view=3 area=0,0,40,25 matched=40      scope=area area=0,0,10,10 matched=2
/// scope=view view=3 extents=yes matched=52
/// ```
///
/// A view's area is the one it had when the line ran, so a reply says which
/// part of the drawing "what I see" was. A verb adds its own keys after it.
pub fn scope_record(found: &ScopeMatch) -> String {
    scope_record_of(&found.resolved, found.matched.len())
}

/// The same record for a resolver whose matches are not entities - the road
/// objects - from the resolved scope and how many it took (the C++ overload
/// `scopeRecord(const ResolvedScope&, std::size_t)`).
pub fn scope_record_of(resolved: &ResolvedScope, matched: usize) -> String {
    let words = &resolved.words;
    let mut record = format!("scope={}", source_word(words.source));
    match words.source {
        ScopeSource::View => {
            if let Some(view) = resolved.view {
                record += &format!(" view={view}");
            }
            // Where "what I see" was when the line ran: the view moves.
            if let Some(area) = &resolved.scope.area {
                record += &format!(" area={}", area_text(area));
            }
            if words.extents {
                record += " extents=yes";
            }
        }
        ScopeSource::Area => record += &format!(" area={}", area_text(&words.area)),
        ScopeSource::Layers => {
            record += &format!(
                " layers={} sublayers={}",
                record_value(&words.layers.join(",")),
                if words.sublayers { "yes" } else { "no" }
            );
        }
        ScopeSource::Selection | ScopeSource::Drawing => {}
    }
    // A filter that cannot be typed back still says what it was.
    if let Ok(conditions) = condition_words(&words.filter)
        && !conditions.is_empty()
    {
        record += &format!(" where={}", record_value(&conditions.join(" ")));
    }
    record + &format!(" matched={matched}")
}

/// How many of `ids` are of each kind, in the geometry variant's order (the
/// kind byte's), kinds with none left out and an id the model lacks skipped.
/// What the window's type boxes show beside each kind - "Leader (774)" - and
/// SELECT's types= says, so the two cannot count differently.
pub fn count_by_type(model: &Model, ids: &[EntityId]) -> Vec<(EntityType, usize)> {
    // Indexed by the kind byte, so the order is the variant's whatever order
    // the ids come in, and a kind appended to the variant is counted too.
    let mut tally = [0_usize; GEOMETRY_KIND_COUNT];
    for &id in ids {
        if let Some(entity) = model.entities.find(id) {
            tally[entity.geometry.index()] += 1;
        }
    }
    ENTITY_TYPES
        .iter()
        .zip(tally)
        .filter(|(_, count)| *count > 0)
        .map(|(&entity_type, count)| (entity_type, count))
        .collect()
}

/// count_by_type as one record value, each kind as TYPE= spells it:
/// "leader:774,point:12". Empty when it counts nothing.
pub fn type_tally(model: &Model, ids: &[EntityId]) -> String {
    let parts: Vec<String> = count_by_type(model, ids)
        .into_iter()
        .map(|(entity_type, count)| format!("{}:{count}", entity_type.name().to_ascii_lowercase()))
        .collect();
    parts.join(",")
}

/// A value in a reply record: as it is when it is one plain word, else in
/// double quotes with '"' and '\' escaped and a line break made a blank, so a
/// record still splits on blanks. The UTILITY records use it too.
pub fn record_value(value: &str) -> String {
    let plain = !value.is_empty() && !value.contains([' ', '\t', '"', '=', '\\', '\n', '\r']);
    if plain {
        return String::from(value);
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(if c == '\n' || c == '\r' { ' ' } else { c });
    }
    out.push('"');
    out
}
