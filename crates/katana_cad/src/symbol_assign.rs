//! The symbol library's logic, below the window so it can be tested: what the
//! library lists, who uses a symbol, how big one prints, putting a symbol on
//! points and swapping one symbol for another in the styles
//! (include/katana/cad/symbol_assign.hpp,
//! src/katana_cad/customisation/symbol_assign.cpp).
//!
//! A POINT CARRIES NO SYMBOL OF ITS OWN. Style::symbol does (tables.rs says
//! why: "the style of a point" and "the style of a line" are one thing), so
//! "give these points the TEST Tree symbol" means "put them in a style that
//! draws TEST Tree". assign_symbol_to_points finds such a style, or makes one,
//! and moves the points into it - in ONE undo step, so a person who undoes it
//! gets back both the points' old styles and a table without the new style.
//!
//! THE STYLE IT REUSES says nothing of its own beyond the symbol: colour,
//! linetype and hatch ByLayer, the default line weight, the exact symbol name
//! (names are case-sensitive, D3) and the exact size. A style that also sets a
//! colour is someone's deliberate choice for other points, and reusing it
//! would recolour these; one that differs only in its description does not
//! draw differently, so the description is not compared. The line weight has
//! no ByLayer in the model (a style's always wins, entity's resolve_display),
//! so "says nothing" there means the default a new Style gets.
//!
//! THE LIBRARY LIST is D3's: every definition symbol_choices offers as a
//! symbol (and the built-in shapes it does not hide), plus every symbol name a
//! Style or a survey code gives that nothing defines - listed, marked missing,
//! with the built-in shape it is drawn as - because that is exactly what a
//! person opening a symbol library to fix a drawing needs to see.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use katana_commands::{CommandPtr, Transaction, create_style, set_entity_style, update_style};
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, lowered};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::display::{BY_LAYER_LINETYPE, is_by_layer};
use katana_entity::table_usage::table_usage;
use katana_entity::tables::{built_in_symbol_for, is_built_in_symbol_name};
use katana_entity::{
    EntityId, EntityType, LineStyle, Model, Style, SurveyMap, SurveyRule, SurveySection,
    TableUsage, UsageOptions, validate,
};
use katana_geometry::{Box2, Point2};

use crate::document::Document;
use crate::style_catalogue::{
    CatalogueEntry, DefinitionSource, NameRole, free_style_name, missing_names, symbol_choices,
};
use crate::style_drawing::{StyleTextMark, symbol_drawing_of_style};

// ---- who uses a symbol --------------------------------------------------------------------

/// A survey code that draws a symbol at its vertices (a VertexSymbol rule).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SymbolCode {
    /// As written: "AC*".
    pub key: String,
    /// SurveySymbol::size; 0 is the definition's own.
    pub size: f64,
    /// A standard colour name; empty is the string's own.
    pub colour: String,
    pub comment: String,
}

fn is_symbol_rule(rule: &SurveyRule) -> bool {
    rule.section == SurveySection::VertexSymbol
        && rule
            .symbol
            .as_ref()
            .is_some_and(|symbol| !symbol.style.is_empty())
}

fn code_of(rule: &SurveyRule) -> SymbolCode {
    let (size, colour) = rule.symbol.as_ref().map_or((0.0, String::new()), |symbol| {
        (symbol.size, symbol.colour.clone())
    });
    SymbolCode {
        key: rule.key.clone(),
        size,
        colour,
        comment: rule.comment.clone(),
    }
}

fn symbol_name_of(rule: &SurveyRule) -> &str {
    rule.symbol
        .as_ref()
        .map_or("", |symbol| symbol.style.as_str())
}

// "says nothing else" - see the module comment for why each field is here
// and why the description is not.
fn says_only_its_symbol(style: &Style) -> bool {
    let fresh = Style::default();
    style.color.is_none()
        && is_by_layer(&style.linetype)
        && style.hatch_pattern.is_empty()
        && style.line_weight == fresh.line_weight
}

fn count_of(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The VertexSymbol rules naming `name` exactly, in the map's order.
pub fn symbol_codes(map: &SurveyMap, name: &str) -> Vec<SymbolCode> {
    if name.is_empty() {
        return Vec::new();
    }
    map.rules()
        .iter()
        .filter(|rule| is_symbol_rule(rule) && symbol_name_of(rule) == name)
        .map(code_of)
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SymbolUsers {
    /// Naming it, ascending.
    pub styles: Vec<String>,
    /// Points wearing them, ascending.
    pub points: Vec<EntityId>,
    /// Other entities wearing those styles - lines, which draw the symbol at
    /// every vertex (D8). Counted, not listed: "select the points" is the
    /// action a symbol library offers.
    pub other_entities: usize,
}

/// Read through entity's table_usage, the one answer the managers, the delete
/// guards and purge share. Empty for "" (no symbol is not a name).
pub fn symbol_users(document: &Document, name: &str) -> SymbolUsers {
    let mut users = SymbolUsers::default();
    if name.is_empty() {
        return users;
    }
    let model = document.model();
    let usage = table_usage(model, UsageOptions { entity_ids: true });
    let of = TableUsage::of(&usage.symbols, name);
    users.styles = of.styles.clone();
    for &id in &of.entity_ids {
        match model.entities.find(id) {
            Some(entity) if entity.r#type() == EntityType::Point => users.points.push(id),
            _ => users.other_entities += 1,
        }
    }
    users
}

// ---- what the library lists --------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SymbolLibraryEntry {
    /// From symbol_choices, or for a name nothing defines one with source
    /// Undefined and `missing` set; `users` is table_usage's either way.
    pub entry: CatalogueEntry,
    /// Missing only: the built-in shape it is drawn as instead (entity's
    /// built_in_symbol_for).
    pub fallback: String,
    pub codes: Vec<SymbolCode>,
}

/// symbol_choices, then every symbol name a Style or a VertexSymbol rule gives
/// that no library defines and that is not built in, all sorted by name with
/// case folded as a picker sorts.
pub fn symbol_library(document: &Document) -> Vec<SymbolLibraryEntry> {
    let map = document.survey_map();
    // Every code by the symbol it draws, in one pass over the rules: asking
    // symbol_codes per entry is a pass per entry - 800 definitions over
    // 1,600 rules on every reload.
    let mut codes_of: BTreeMap<String, Vec<SymbolCode>> = BTreeMap::new();
    for rule in map.rules() {
        if is_symbol_rule(rule) {
            codes_of
                .entry(String::from(symbol_name_of(rule)))
                .or_default()
                .push(code_of(rule));
        }
    }
    let codes_for = |name: &str| codes_of.get(name).cloned().unwrap_or_default();

    let mut entries = Vec::new();
    for choice in symbol_choices(document) {
        entries.push(SymbolLibraryEntry {
            codes: codes_for(&choice.name),
            entry: choice,
            fallback: String::new(),
        });
    }

    // The names nothing defines: a Style's (missing_names, which already
    // leaves out built-in names and carries the users) and a survey code's,
    // which missing_names does not look at because no Style holds it yet -
    // but the next survey import will draw it as a stand-in all the same.
    let mut missing: BTreeMap<String, SymbolLibraryEntry> = BTreeMap::new();
    for name in missing_names(document) {
        if name.role != NameRole::Symbol {
            continue;
        }
        let entry = SymbolLibraryEntry {
            entry: CatalogueEntry {
                name: name.name.clone(),
                source: DefinitionSource::Undefined,
                missing: true,
                users: name.users,
                ..CatalogueEntry::default()
            },
            fallback: name.fallback,
            codes: Vec::new(),
        };
        missing.entry(name.name).or_insert(entry);
    }
    for rule in map.rules() {
        if !is_symbol_rule(rule) {
            continue;
        }
        let name = symbol_name_of(rule);
        if missing.contains_key(name)
            || document.definition_for(name).is_some()
            || is_built_in_symbol_name(name)
        {
            continue;
        }
        let entry = SymbolLibraryEntry {
            entry: CatalogueEntry {
                name: String::from(name),
                source: DefinitionSource::Undefined,
                missing: true,
                ..CatalogueEntry::default()
            },
            fallback: String::from(built_in_symbol_for(name)),
            codes: Vec::new(),
        };
        missing.insert(String::from(name), entry);
    }
    for (name, mut entry) in missing {
        entry.codes = codes_for(&name);
        entries.push(entry);
    }

    // symbol_choices' order - case folded, the exact name breaking a tie -
    // over the whole list, so a missing name sits where a person looks for
    // it rather than at the end. Stable, as the C++'s stable_sort.
    entries.sort_by(|a, b| {
        lowered(&a.entry.name)
            .cmp(&lowered(&b.entry.name))
            .then_with(|| a.entry.name.cmp(&b.entry.name))
    });
    entries
}

// ---- how big it prints -----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SymbolPrintSize {
    /// Metres on the ground.
    pub ground_width: f64,
    pub ground_height: f64,
    /// Millimetres on the plot.
    pub paper_width: f64,
    pub paper_height: f64,
    /// The point the symbol is put on lies within what it draws. False for a
    /// definition drawn away from its insertion point: every point wearing it
    /// shows its mark somewhere else.
    pub insertion_inside: bool,
}

impl Default for SymbolPrintSize {
    fn default() -> Self {
        Self {
            ground_width: 0.0,
            ground_height: 0.0,
            paper_width: 0.0,
            paper_height: 0.0,
            insertion_inside: true,
        }
    }
}

/// 0.6 is about an average sans-serif character: Arial's digits are 0.556 of
/// the size they are set at, its capital V 0.667, most of its lower case 0.5.
pub const ESTIMATED_CHARACTER_WIDTH: f64 = 0.6;

/// The box a symbol's text covers, in the model units of the mark, ESTIMATED:
/// the face a text is painted in is the window's question, and cad has no
/// font. The text is taken to fill its `height` - the size it is painted at,
/// which its capitals and ascenders stay within - standing on its anchor,
/// hanging from it ("top") or centred on it ("middle"); and to be
/// ESTIMATED_CHARACTER_WIDTH of that height, times its width_factor, wide per
/// character, starting at the anchor, centred on it ("centre") or ending at it
/// ("right"). The box is then turned about the anchor by the text's angle. The
/// justification is read as the painter reads it (style_painter.cpp's
/// justifiedOrigin), so an unknown spelling stands on the anchor from its
/// left, as an unjustified text does. Descenders are not counted. Empty for an
/// empty text or a height that is not a positive number.
pub fn estimated_text_extent(text: &StyleTextMark) -> Box2 {
    let mut bounds = Box2::default();
    if text.text.is_empty() || !(text.height.is_finite() && text.height > 0.0) {
        return bounds;
    }
    // Characters, not bytes: "Ø" is one letter on the plot and two bytes here.
    let characters = text
        .text
        .bytes()
        .filter(|byte| (byte & 0xC0) != 0x80)
        .count() as f64;
    let factor = if text.width_factor.is_finite() && text.width_factor > 0.0 {
        text.width_factor
    } else {
        1.0
    };
    let width = ESTIMATED_CHARACTER_WIDTH * text.height * factor * characters;
    let justify = &text.justify;
    // In the text's own frame: x along it from the anchor, y up from it.
    let mut left = 0.0;
    if justify.contains("centre") || justify.contains("center") {
        left = -0.5 * width;
    } else if justify.contains("right") {
        left = -width;
    }
    let mut bottom = 0.0;
    if justify.contains("middle") {
        bottom = -0.5 * text.height;
    } else if justify.contains("top") {
        bottom = -text.height;
    }
    let cosine = text.angle.cos();
    let sine = text.angle.sin();
    for x in [left, left + width] {
        for y in [bottom, bottom + text.height] {
            bounds.expand(Point2::new(
                text.at.x + x * cosine - y * sine,
                text.at.y + x * sine + y * cosine,
            ));
        }
    }
    bounds
}

/// Where a text lands, as a box in the text's own model units. A caller that
/// has the font the text is painted in - a window dialog, through its painter's
/// text extent - measures it, so its answer agrees with the picture beside
/// it; one that does not passes none, and estimated_text_extent is used.
pub type TextExtent = dyn Fn(&StyleTextMark) -> Box2;

/// What a point wearing `definition` at `size` (Style::symbol_size: a width in
/// model units, 0 for the definition's own) draws, plotted at 1:N: the box of
/// symbol_drawing's strokes and of the space each of its texts covers, so it
/// is what prints - a worldstyle's units are ground metres, a paperstyle's
/// plot millimetres, and a size scales the definition so the larger side of
/// LineStyle::bounds() spans it. A text's space is `text_extent`'s answer, or
/// estimated_text_extent's when none is given (the C++ default): a text's
/// anchor alone has no size, and a symbol that is a letter, or carries one
/// above its mark, prints the letter too. None for a definition that draws
/// nothing, a denominator that is not positive or a size that is not a finite
/// number of at least 0.
pub fn symbol_print_size(
    definition: &LineStyle,
    size: f64,
    scale_denominator: f64,
    text_extent: Option<&TextExtent>,
) -> Option<SymbolPrintSize> {
    if !(scale_denominator > 0.0) || !(size.is_finite() && size >= 0.0) {
        return None;
    }
    // Model units (metres) per plot millimetre at 1:N.
    let paper_scale = scale_denominator / 1000.0;
    // Measured on the drawing a point gets, not recomputed: symbol_drawing
    // is where paper units, `factor`, the origin and a size are applied,
    // and a second copy of those rules here would be a second answer.
    let drawing =
        symbol_drawing_of_style(definition, Point2::new(0.0, 0.0), size, 0.0, paper_scale);
    // Not StyleDrawing::bounds(), which takes a text's anchor for all of it:
    // a letter set 0.5 m high above a valve's box then printed as nothing,
    // and a symbol that is only a letter as 0 x 0 mm.
    let mut bounds = Box2::default();
    for stroke in &drawing.strokes {
        for &point in &stroke.path.vertices {
            bounds.expand(point);
        }
    }
    for text in &drawing.texts {
        bounds.expand_box(match text_extent {
            Some(measure) => measure(text),
            None => estimated_text_extent(text),
        });
    }
    if bounds.empty() {
        return None;
    }
    let ground_width = bounds.width();
    let ground_height = bounds.height();
    Some(SymbolPrintSize {
        ground_width,
        ground_height,
        paper_width: ground_width / paper_scale,
        paper_height: ground_height / paper_scale,
        // The point the symbol is put on is the drawing's (0, 0): the C++'s
        // `min.x <= 0 && 0 <= max.x && min.y <= 0 && 0 <= max.y`.
        insertion_inside: (bounds.min.x..=bounds.max.x).contains(&0.0)
            && (bounds.min.y..=bounds.max.y).contains(&0.0),
    })
}

// ---- changing which symbol is drawn ----------------------------------------------------------

/// A style that draws exactly `symbol_name` at exactly `size` and says nothing
/// else (see the module comment), or None. When several do, the one named
/// after the symbol, else the first by name.
pub fn find_symbol_style<'a>(model: &'a Model, symbol_name: &str, size: f64) -> Option<&'a Style> {
    if symbol_name.is_empty() {
        return None;
    }
    let draws_only = |style: &Style| {
        style.symbol == symbol_name && style.symbol_size == size && says_only_its_symbol(style)
    };
    // The one a person would look for first: named after the symbol.
    if let Some(named) = model.styles.find(symbol_name)
        && draws_only(named)
    {
        return Some(named);
    }
    // In name order, as the table visits them.
    model
        .styles
        .names()
        .iter()
        .filter_map(|name| model.styles.find(name))
        .find(|&style| draws_only(style))
}

#[derive(Default)]
pub struct SymbolAssignment {
    /// ONE undo step: the new style, when one is made, and the points moved
    /// into it. None when there is nothing to change - every point among the
    /// ids already wears the style - so no empty step is ever pushed.
    pub command: Option<CommandPtr>,
    /// The style the points wear afterwards.
    pub style: String,
    /// `style` is new, and `command` makes it.
    pub creates_style: bool,
    /// Points that move into `style`.
    pub points: usize,
    /// Points already wearing it.
    pub already_in_style: usize,
    /// Other entities among the ids: left as they are.
    pub not_points: usize,
    /// Ids no entity has.
    pub not_found: usize,
}

impl fmt::Debug for SymbolAssignment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolAssignment")
            .field(
                "command",
                &self.command.as_ref().map(|command| command.name()),
            )
            .field("style", &self.style)
            .field("creates_style", &self.creates_style)
            .field("points", &self.points)
            .field("already_in_style", &self.already_in_style)
            .field("not_points", &self.not_points)
            .field("not_found", &self.not_found)
            .finish()
    }
}

/// Fails with InvalidArgument for an empty symbol name, a size that is not a
/// finite number of at least 0, or ids among which there is no point at all
/// (the message counts what there was instead). A locked layer is refused
/// when the command executes, and the whole step with it.
pub fn assign_symbol_to_points(
    document: &Document,
    ids: &[EntityId],
    symbol_name: &str,
    size: f64,
) -> Result<SymbolAssignment> {
    if symbol_name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no symbol to assign",
        ));
    }
    if !(size.is_finite() && size >= 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a symbol size must be a finite number of at least 0",
            format_exact_real(size),
        ));
    }
    if ids.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no entities to assign the symbol to",
        ));
    }

    let model = document.model();
    let mut assignment = SymbolAssignment::default();
    let mut points = Vec::new();
    let mut seen = BTreeSet::new();
    for &id in ids {
        if !seen.insert(id) {
            continue; // a selection lists an id once; a caller's list might not
        }
        match model.entities.find(id) {
            None => assignment.not_found += 1,
            Some(entity) if entity.r#type() != EntityType::Point => assignment.not_points += 1,
            Some(_) => points.push(id),
        }
    }
    if points.is_empty() {
        let mut what = count_of(
            assignment.not_points,
            "entity that is not a point",
            "entities that are not points",
        );
        if assignment.not_found > 0 {
            what += ", ";
            what += &count_of(
                assignment.not_found,
                "id no entity has",
                "ids no entity has",
            );
        }
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "none of the entities is a point",
            what,
        ));
    }

    let style = match find_symbol_style(model, symbol_name, size) {
        Some(found) => found.clone(),
        None => {
            let made = Style {
                name: free_style_name(model, symbol_name),
                linetype: String::from(BY_LAYER_LINETYPE),
                symbol: String::from(symbol_name),
                symbol_size: size,
                ..Style::default()
            };
            validate(&made)?;
            assignment.creates_style = true;
            made
        }
    };
    assignment.style = style.name.clone();

    let mut moving = Vec::new();
    for &id in &points {
        if model
            .entities
            .find(id)
            .is_some_and(|entity| entity.style == style.name)
        {
            assignment.already_in_style += 1;
        } else {
            moving.push(id);
        }
    }
    assignment.points = moving.len();
    if moving.is_empty() {
        // Every point already wears a style that exists: nothing to undo,
        // so no step. (A style that would be created is never worn yet.)
        return Ok(assignment);
    }

    let mut transaction = Transaction::new("ASSIGN_SYMBOL");
    let name = style.name.clone();
    if assignment.creates_style {
        transaction.add(create_style(style));
    }
    transaction.add(set_entity_style(moving, name));
    assignment.command = Some(Box::new(transaction));
    Ok(assignment)
}

/// Every style whose symbol is exactly `from` set to `to`, in one undo step;
/// None when no style names `from`, or `from` is "" or equal to `to`. `to`
/// may be any name, "" included (the plain point mark): it is a name resolved
/// when drawn, as every Style::symbol is.
pub fn replace_symbol_in_styles(model: &Model, from: &str, to: &str) -> Option<CommandPtr> {
    if from.is_empty() || from == to {
        return None;
    }
    let mut transaction = Transaction::new("REPLACE_SYMBOL");
    model.styles.for_each(|style| {
        if style.symbol == from {
            let mut changed = style.clone();
            changed.symbol = String::from(to);
            transaction.add(update_style(changed));
        }
    });
    if transaction.size() == 0 {
        return None;
    }
    Some(Box::new(transaction))
}
