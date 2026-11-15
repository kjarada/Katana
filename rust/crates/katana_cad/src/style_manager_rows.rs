//! What the styles and linetypes manager shows and what its form writes, below
//! the window so it can be tested: the rows of its three tables (styles;
//! drawing linetypes with library linestyles; diagnostics), its filters, the
//! fields a multi-selection agrees on, and the ONE command a bulk edit makes
//! (include/katana/cad/style_manager_rows.hpp,
//! src/katana_cad/customisation/style_manager_rows.cpp).
//!
//! The dialog only renders these. Every count comes from one table_usage pass
//! and every "missing" from missing_names, so the manager, the delete guards
//! and purge cannot disagree about what is used or what is defined.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use katana_commands::{CommandPtr, Transaction, update_style_if_changed};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};
use katana_entity::table_usage::table_usage;
use katana_entity::{
    Color, EntityId, EnumName, LineStyle, Model, Style, StyleUnits, TableUsage, UsageOptions,
    Users, validate,
};

use crate::document::Document;
use crate::style_catalogue::{NameRole, NameStatus, linetype_collisions, missing_names};
use crate::style_drawing::flatten_definition;

// ---- styles --------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleRow {
    pub style: Style,
    /// The entities wearing it (table_usage), which is what "Used" means for a
    /// style: a layer cannot name a style.
    pub entities: usize,
    /// Its linetype or symbol is a name missing_names reports: nothing defines
    /// it, and it is drawn by a fallback.
    pub missing_linetype: bool,
    pub missing_symbol: bool,
    /// Document::current_style: new work is drawn in it (decision D9).
    pub current: bool,
}

impl StyleRow {
    pub fn missing(&self) -> bool {
        self.missing_linetype || self.missing_symbol
    }
}

/// Every style of the model, ascending by name (the table's order).
pub fn style_rows(document: &Document) -> Vec<StyleRow> {
    let model = document.model();
    let usage = table_usage(model, UsageOptions::default());

    // A style is "missing" when it NAMES a missing linetype or symbol itself:
    // missing_names lists the styles naming each, so this is a lookup, not a
    // second resolution that could disagree with it.
    let mut missing_linetype: BTreeSet<String> = BTreeSet::new();
    let mut missing_symbol: BTreeSet<String> = BTreeSet::new();
    for missing in missing_names(document) {
        let set = if missing.role == NameRole::Linetype {
            &mut missing_linetype
        } else {
            &mut missing_symbol
        };
        set.extend(missing.users.styles);
    }

    let current = document.current_style();
    let mut rows = Vec::with_capacity(model.styles.size());
    model.styles.for_each(|style| {
        rows.push(StyleRow {
            style: style.clone(),
            entities: TableUsage::of(&usage.styles, &style.name).entities,
            missing_linetype: missing_linetype.contains(&style.name),
            missing_symbol: missing_symbol.contains(&style.name),
            current: !current.is_empty() && current == style.name,
        });
    });
    rows
}

/// The manager's chips: All / Used / Unused / Missing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StyleFilter {
    #[default]
    All,
    Used,
    Unused,
    Missing,
}

pub fn matches_filter(row: &StyleRow, filter: StyleFilter) -> bool {
    match filter {
        StyleFilter::All => true,
        StyleFilter::Used => row.entities > 0,
        StyleFilter::Unused => row.entities == 0,
        StyleFilter::Missing => row.missing(),
    }
}

// `folded_needle`, already folded, in `text` folded as core's lowered folds:
// ASCII letters only, so folded UTF-8 stays UTF-8 and a str search is a byte
// search.
fn contains_folded(text: &str, folded_needle: &str) -> bool {
    text.to_ascii_lowercase().contains(folded_needle)
}

/// The search box: `text` found in the name, linetype, symbol, hatch pattern or
/// description, ASCII case folded (decision D3: a search folds, storage never
/// does). Everything matches an empty text.
pub fn matches_search(row: &StyleRow, text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let needle = text.to_ascii_lowercase();
    let style = &row.style;
    contains_folded(&style.name, &needle)
        || contains_folded(&style.linetype, &needle)
        || contains_folded(&style.symbol, &needle)
        || contains_folded(&style.hatch_pattern, &needle)
        || contains_folded(&style.description, &needle)
}

// ---- the form over one style or several ------------------------------------------------

/// The editable fields of a Style, each either set or not. Read as what a
/// selection AGREES on (common_fields: unset = the form shows <varies>), and
/// as what a person EDITED (apply_edit: unset = leave every style's own
/// value). The name is not here: renaming is its own command, so the entities
/// wearing a style come with it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleFields {
    pub linetype: Option<String>,
    pub line_weight: Option<f64>,
    /// The outer Option says whether the field is set; the inner one is the
    /// Style's own (None = ByLayer).
    pub color: Option<Option<Color>>,
    pub hatch_pattern: Option<String>,
    pub symbol: Option<String>,
    pub symbol_size: Option<f64>,
    pub description: Option<String>,
}

impl StyleFields {
    pub fn empty(&self) -> bool {
        self.linetype.is_none()
            && self.line_weight.is_none()
            && self.color.is_none()
            && self.hatch_pattern.is_none()
            && self.symbol.is_none()
            && self.symbol_size.is_none()
            && self.description.is_none()
    }
}

// Every field of `style` set: the fields one style "agrees on".
fn fields_of(style: &Style) -> StyleFields {
    StyleFields {
        linetype: Some(style.linetype.clone()),
        line_weight: Some(style.line_weight),
        color: Some(style.color),
        hatch_pattern: Some(style.hatch_pattern.clone()),
        symbol: Some(style.symbol.clone()),
        symbol_size: Some(style.symbol_size),
        description: Some(style.description.clone()),
    }
}

// Clears `field` unless `value` equals it: what common_fields folds with.
fn keep_if_equal<T: PartialEq>(field: &mut Option<T>, value: &T) {
    if field.as_ref().is_some_and(|kept| kept != value) {
        *field = None;
    }
}

/// The fields every one of `styles` has the same value for, compared exactly
/// (a weight of 0.25 and one of 0.2500001 differ, and so show <varies>).
/// Every field set for one style; none for no styles.
pub fn common_fields(styles: &[Style]) -> StyleFields {
    let Some(front) = styles.first() else {
        return StyleFields::default();
    };
    let mut common = fields_of(front);
    for style in styles {
        keep_if_equal(&mut common.linetype, &style.linetype);
        keep_if_equal(&mut common.line_weight, &style.line_weight);
        keep_if_equal(&mut common.color, &style.color);
        keep_if_equal(&mut common.hatch_pattern, &style.hatch_pattern);
        keep_if_equal(&mut common.symbol, &style.symbol);
        keep_if_equal(&mut common.symbol_size, &style.symbol_size);
        keep_if_equal(&mut common.description, &style.description);
    }
    common
}

/// `style` with each SET field of `edit` written over it; the rest of it - and
/// its name - exactly as it was.
pub fn apply_edit(mut style: Style, edit: &StyleFields) -> Style {
    if let Some(linetype) = &edit.linetype {
        style.linetype.clone_from(linetype);
    }
    if let Some(line_weight) = edit.line_weight {
        style.line_weight = line_weight;
    }
    if let Some(color) = edit.color {
        style.color = color;
    }
    if let Some(hatch_pattern) = &edit.hatch_pattern {
        style.hatch_pattern.clone_from(hatch_pattern);
    }
    if let Some(symbol) = &edit.symbol {
        style.symbol.clone_from(symbol);
    }
    if let Some(symbol_size) = edit.symbol_size {
        style.symbol_size = symbol_size;
    }
    if let Some(description) = &edit.description {
        style.description.clone_from(description);
    }
    style
}

/// ONE command writing `edit` into every style named: one undo step however
/// many there are, holding only the styles it changes (update_style_if_changed
/// for each). None when it changes none (the C++'s null command) - an unedited
/// Save is not an edit and pushes no undo step (audit QT-02, QT-01's shape).
/// Fails with NotFound for a name the model has no style of, and with
/// validate(Style)'s error, naming the style, for an edit that would make one
/// invalid - before anything is built, so a refused bulk edit changes nothing.
pub fn edit_styles_command(
    model: &Model,
    names: &[String],
    edit: &StyleFields,
) -> Result<Option<CommandPtr>> {
    // Checked for every style before any command is built: a bulk edit that
    // one style refuses is refused whole, and says which style.
    let mut edited = Vec::with_capacity(names.len());
    for name in names {
        let Some(stored) = model.styles.find(name) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "there is no style of that name",
                name.as_str(),
            ));
        };
        let style = apply_edit(stored.clone(), edit);
        if let Err(mut error) = validate(&style) {
            error.context = if error.context.is_empty() {
                name.clone()
            } else {
                format!("{name}: {}", error.context)
            };
            return Err(error);
        }
        edited.push(style);
    }

    let mut parts: Vec<CommandPtr> = edited
        .into_iter()
        .filter_map(|style| update_style_if_changed(model, style))
        .collect();
    if parts.len() <= 1 {
        return Ok(parts.pop());
    }
    let mut transaction = Transaction::new("UpdateStyles");
    for part in parts {
        transaction.add(part);
    }
    Ok(Some(Box::new(transaction)))
}

// ---- linetypes and library linestyles ----------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LinetypeOrigin {
    /// A model Linetype: a dash pattern, saved with the project.
    #[default]
    Drawing,
    /// A non-vertex library definition of the session's StyleLibrary (D2).
    Library,
}

/// The C++ `toString(LinetypeOrigin)`.
impl EnumName for LinetypeOrigin {
    fn name(self) -> &'static str {
        match self {
            Self::Drawing => "drawing",
            Self::Library => "library",
        }
    }
}

/// What draws the line: a Katana dash pattern, or a library definition laid by
/// its units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LinetypeRowKind {
    /// Model Linetype; continuous when its pattern is empty.
    #[default]
    Dash,
    /// Paper linestyle (`paperstyle`): millimetres on the plot.
    Paper,
    /// World linestyle (`worldstyle`): model units.
    World,
    /// Two-point linestyle (`twoptstyle`): stretched between two anchors.
    TwoPoint,
}

/// The C++ `toString(LinetypeRowKind)`.
impl EnumName for LinetypeRowKind {
    fn name(self) -> &'static str {
        match self {
            Self::Dash => "dash",
            Self::Paper => "paper",
            Self::World => "world",
            Self::TwoPoint => "two-point",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinetypeRow {
    pub name: String,
    pub origin: LinetypeOrigin,
    pub kind: LinetypeRowKind,
    /// LineStyle::group; empty for a drawing linetype.
    pub group: String,
    /// LineStyle::source; empty for a drawing linetype.
    pub source_file: String,
    /// Linetype::description; empty for a library one.
    pub description: String,
    /// One repeat of the pattern: model units for a Dash or World row,
    /// millimetres on paper for a Paper row, as the drawing code lays it (the
    /// file's `length`, else the pen's travel). 0 for a continuous linetype and
    /// for a TwoPoint row, which is stretched and has no period.
    pub period: f64,
    /// Who names the NAME and the entities drawn with it. Shared by the two
    /// rows of a collision: both answer to the same name.
    pub users: Users,
    /// The name is both a drawing linetype and a library linestyle (D2): the
    /// library row is what is drawn, the drawing row is shadowed.
    pub collision: bool,
}

fn kind_of(units: StyleUnits) -> LinetypeRowKind {
    match units {
        StyleUnits::Paper => LinetypeRowKind::Paper,
        StyleUnits::TwoPoint => LinetypeRowKind::TwoPoint,
        StyleUnits::World => LinetypeRowKind::World,
    }
}

// One repeat as the drawing code lays it (style_drawing.rs, lay_linestyle):
// the file's `length`, else how far the pen travelled - before the scale a
// paperstyle is multiplied by, so a Paper period is in plot millimetres.
fn library_period(definition: &LineStyle) -> f64 {
    if definition.units == StyleUnits::TwoPoint {
        return 0.0;
    }
    let flat = flatten_definition(definition);
    if flat.length > 0.0 {
        flat.length
    } else {
        flat.natural_period()
    }
}

/// Every drawing linetype and every library definition offered as a linestyle
/// (not `mode vertex`), ascending by name with case folded and, for one name,
/// the drawing row first. A collision is TWO rows, so the shadowed drawing
/// linetype can still be edited, merged or renamed out of the way.
pub fn linetype_rows(document: &Document) -> Vec<LinetypeRow> {
    let model = document.model();
    let usage = table_usage(model, UsageOptions::default());
    let library = document.style_library();

    let mut rows = Vec::with_capacity(model.linetypes.size() + library.size());
    model.linetypes.for_each(|linetype| {
        rows.push(LinetypeRow {
            name: linetype.name.clone(),
            origin: LinetypeOrigin::Drawing,
            kind: LinetypeRowKind::Dash,
            description: linetype.description.clone(),
            period: linetype.pattern_length(),
            users: TableUsage::of(&usage.linetypes, &linetype.name).clone(),
            collision: library
                .find(&linetype.name)
                .is_some_and(|definition| !definition.at_vertices),
            ..LinetypeRow::default()
        });
    });
    library.for_each(|definition| {
        // D2: a `mode vertex` definition is a symbol, never a linestyle - a
        // line naming it is drawn solid - so it is not offered here.
        if definition.at_vertices {
            return;
        }
        rows.push(LinetypeRow {
            name: definition.name.clone(),
            origin: LinetypeOrigin::Library,
            kind: kind_of(definition.units),
            group: definition.group.clone(),
            source_file: definition.source.clone(),
            description: String::new(),
            period: library_period(definition),
            users: TableUsage::of(&usage.linetypes, &definition.name).clone(),
            collision: model.linetypes.contains(&definition.name),
        });
    });
    // Case folded, as a person reads a list; the exact name, then the drawing
    // row first, break ties so the order never depends on the input's.
    rows.sort_by(|a, b| {
        let folded_a = a.name.to_ascii_lowercase();
        let folded_b = b.name.to_ascii_lowercase();
        if folded_a != folded_b {
            return folded_a.cmp(&folded_b);
        }
        if a.name != b.name {
            return a.name.cmp(&b.name);
        }
        match (a.origin, b.origin) {
            (LinetypeOrigin::Drawing, LinetypeOrigin::Library) => Ordering::Less,
            (LinetypeOrigin::Library, LinetypeOrigin::Drawing) => Ordering::Greater,
            _ => Ordering::Equal,
        }
    });
    rows
}

// ---- diagnostics -----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StyleDiagnosticKind {
    /// A Style or Layer linetype nothing defines.
    #[default]
    MissingLinetype,
    /// A Style symbol nothing defines.
    MissingSymbol,
    /// A drawing linetype a library linestyle shadows (D2).
    Collision,
}

/// The C++ `toString(StyleDiagnosticKind)`.
impl EnumName for StyleDiagnosticKind {
    fn name(self) -> &'static str {
        match self {
            Self::MissingLinetype => "missing linetype",
            Self::MissingSymbol => "missing symbol",
            Self::Collision => "collision",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleDiagnostic {
    pub kind: StyleDiagnosticKind,
    pub name: String,
    pub users: Users,
    /// What is drawn meanwhile, in words: "a solid line (continuous)", "the
    /// built-in cross", "the library linestyle; the drawing's pattern is not
    /// drawn".
    pub drawn_as: String,
}

/// missing_names (linetypes, then symbols), then linetype_collisions, each
/// ascending - the manager's Diagnostics tab.
pub fn style_diagnostics(document: &Document) -> Vec<StyleDiagnostic> {
    let mut diagnostics = Vec::new();
    for missing in missing_names(document) {
        let (kind, drawn_as) = if missing.role == NameRole::Linetype {
            let mut drawn_as = format!("a solid line ({})", missing.fallback);
            // Defined, but as a symbol: say so, or a person looks for a
            // library to load when the fix is to pick a linestyle.
            if missing.status == NameStatus::NotALinestyle {
                drawn_as += ": an `at vertices` symbol, not a linestyle";
            }
            (StyleDiagnosticKind::MissingLinetype, drawn_as)
        } else {
            (
                StyleDiagnosticKind::MissingSymbol,
                format!("the built-in {}", missing.fallback),
            )
        };
        diagnostics.push(StyleDiagnostic {
            kind,
            name: missing.name,
            users: missing.users,
            drawn_as,
        });
    }
    let collisions = linetype_collisions(document);
    if !collisions.is_empty() {
        let usage = table_usage(document.model(), UsageOptions::default());
        for name in collisions {
            diagnostics.push(StyleDiagnostic {
                kind: StyleDiagnosticKind::Collision,
                users: TableUsage::of(&usage.linetypes, &name).clone(),
                name,
                drawn_as: String::from(
                    "the library linestyle; the drawing's dash pattern is not drawn",
                ),
            });
        }
    }
    diagnostics
}

// ---- select users ------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UsageTable {
    #[default]
    Style,
    Linetype,
    Symbol,
    HatchPattern,
}

/// The entities reaching any of `names` in `table` - wearing the style, drawn
/// with the linetype, the symbol or the hatch pattern - by table_usage's rule,
/// ascending and without repeats. What "Select Users" selects.
pub fn entities_using(model: &Model, table: UsageTable, names: &[String]) -> Vec<EntityId> {
    let usage = table_usage(model, UsageOptions { entity_ids: true });
    let map = match table {
        UsageTable::Style => &usage.styles,
        UsageTable::Linetype => &usage.linetypes,
        UsageTable::Symbol => &usage.symbols,
        UsageTable::HatchPattern => &usage.hatch_patterns,
    };
    let mut ids = Vec::new();
    for name in names {
        ids.extend_from_slice(&TableUsage::of(map, name).entity_ids);
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// `base`, else "base 2", "base 3" ... for a drawing linetype: free in the
/// model's table AND in the library, so a new linetype never starts life as a
/// D2 collision. Its twin for styles, free_style_name, is in
/// style_catalogue.rs, which the symbol library shares.
pub fn free_linetype_name(document: &Document, base: &str) -> String {
    let taken = |name: &str| {
        document.model().linetypes.contains(name) || document.style_library().contains(name)
    };
    let mut name = String::from(base);
    let mut suffix = 2;
    while taken(&name) {
        name = format!("{base} {suffix}");
        suffix += 1;
    }
    name
}
