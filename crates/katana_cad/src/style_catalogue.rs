//! What a linetype picker and a symbol picker offer, what each name is, and
//! which names the drawing uses that resolve to nothing - the logic of the
//! style, linetype and symbol managers, below the window so it can be tested
//! (include/katana/cad/style_catalogue.hpp,
//! src/katana_cad/customisation/style_catalogue.cpp).
//!
//! THE RULES (the lead's decisions D2 and D3, 2026-09-23):
//!
//! ```text
//! A line's linetype NAME resolves to a NON-vertex library definition of
//! that name first (drawn by its strokes, with no Katana dash applied), else
//! to a model Linetype (dashes), else to a solid line. A name that is both
//! is a COLLISION: reported, not refused. "ByLayer" on a Style inherits the
//! layer's linetype.
//!
//! A library definition is offered as a SYMBOL if any of: it is `mode
//! vertex`; a VertexSymbol rule of the survey map names it; a Style::symbol
//! names it; its customisation LISTS it as a symbol (LineStyle::symbol). It
//! is offered as a LINESTYLE if it is not `mode vertex`. It may be both: most
//! symbols the reference survey codes use are not `mode vertex`, which is
//! why at_vertices alone cannot decide.
//!
//! The fourth reason is what the definition's own customisation says of it
//! (LineStyle::symbol), not the NAME of the file it was read from: one
//! customisation holds both kinds under one name, so where a definition came
//! from cannot say which it is.
//!
//! A picker never relies on its list being complete: keep_current puts the
//! value being edited back, marked, when the list lacks it. That - not a
//! more complete list - is what stops an editor rewriting a name it could
//! not show (audit QT-02).
//!
//! Names are case-SENSITIVE, as they are everywhere in the model; a search
//! may fold case, storage never does.
//! ```

use std::collections::BTreeSet;

use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, lowered};
use katana_core::{ErrorCode, Status};
use katana_entity::display::{BY_LAYER_LINETYPE, is_by_layer, resolved_linetype};
use katana_entity::table_usage::table_usage;
use katana_entity::tables::{CONTINUOUS_LINETYPE, built_in_symbol_for, symbol_names};
use katana_entity::{
    EnumName, LineStyle, Model, StyleUnits, SurveySection, TableUsage, UsageOptions, Users,
};

use crate::document::Document;
use crate::style_resolver::{LinetypeKind, SymbolKind, resolve_line_pattern, resolve_symbol};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DefinitionSource {
    /// A dash pattern in the model's Linetype table.
    ModelLinetype,
    /// A library definition in the Document's StyleLibrary.
    #[default]
    Library,
    /// One of Katana's own: a built-in symbol shape, or ByLayer.
    BuiltIn,
    /// Nothing defines it: a current value keep_current put back.
    Undefined,
}

/// The C++ `toString(DefinitionSource)`.
impl EnumName for DefinitionSource {
    fn name(self) -> &'static str {
        match self {
            Self::ModelLinetype => "model linetype",
            Self::Library => "library",
            Self::BuiltIn => "built-in",
            Self::Undefined => "defined nowhere",
        }
    }
}

/// D3 for one library definition, with each reason kept so a browser can say
/// WHY a definition is listed where it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DefinitionKind {
    /// Offered as a linestyle: not `mode vertex`.
    pub linestyle: bool,
    /// Offered as a symbol: any of the four below.
    pub symbol: bool,
    pub at_vertices: bool,
    /// A VertexSymbol rule names it.
    pub named_by_survey_rule: bool,
    /// Some Style::symbol names it.
    pub named_by_style: bool,
    /// Its customisation lists it as one: LineStyle::symbol.
    pub listed_as_symbol: bool,
}

impl DefinitionKind {
    pub fn known(&self) -> bool {
        self.linestyle || self.symbol
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CatalogueEntry {
    pub name: String,
    pub source: DefinitionSource,
    /// LineStyle::source; empty unless source is Library.
    pub source_file: String,
    /// LineStyle::group; empty unless source is Library.
    pub group: String,
    /// Library only.
    pub units: Option<StyleUnits>,
    /// D3's flags for a library definition; a model linetype or ByLayer is
    /// just a linestyle, and a built-in shape just a symbol.
    pub kind: DefinitionKind,
    /// `mode vertex`; kind.at_vertices, said where a picker looks.
    pub at_vertices: bool,
    /// Linetypes: also a model Linetype, which the library definition wins
    /// over (D2). Reported so a person can rename one of the two.
    pub collision: bool,
    /// Defined nowhere: a current value keep_current put back.
    pub missing: bool,
    /// Who names it and the entities reaching it (entity's table_usage).
    pub users: Users,
}

// What D3 needs beyond the definition itself, gathered once for a whole
// list rather than once per definition - a list is eight hundred of them.
struct SymbolEvidence {
    style_symbols: BTreeSet<String>,
    rule_symbols: BTreeSet<String>,
}

impl SymbolEvidence {
    fn of(document: &Document) -> Self {
        let mut style_symbols = BTreeSet::new();
        document.model().styles.for_each(|style| {
            if !style.symbol.is_empty() {
                style_symbols.insert(style.symbol.clone());
            }
        });
        let mut rule_symbols = BTreeSet::new();
        for rule in document.survey_map().rules() {
            if rule.section == SurveySection::VertexSymbol
                && let Some(symbol) = &rule.symbol
                && !symbol.style.is_empty()
            {
                rule_symbols.insert(symbol.style.clone());
            }
        }
        Self {
            style_symbols,
            rule_symbols,
        }
    }
}

fn classify(definition: &LineStyle, evidence: &SymbolEvidence) -> DefinitionKind {
    let mut kind = DefinitionKind {
        at_vertices: definition.at_vertices,
        linestyle: !definition.at_vertices,
        named_by_survey_rule: evidence.rule_symbols.contains(&definition.name),
        named_by_style: evidence.style_symbols.contains(&definition.name),
        // What the definition's own customisation says of it. Not its source's
        // name: one customisation holds both kinds.
        listed_as_symbol: definition.symbol,
        symbol: false,
    };
    kind.symbol = kind.at_vertices
        || kind.named_by_survey_rule
        || kind.named_by_style
        || kind.listed_as_symbol;
    kind
}

fn from_definition(definition: &LineStyle, kind: DefinitionKind) -> CatalogueEntry {
    CatalogueEntry {
        name: definition.name.clone(),
        source: DefinitionSource::Library,
        source_file: definition.source.clone(),
        group: definition.group.clone(),
        units: Some(definition.units),
        kind,
        at_vertices: definition.at_vertices,
        ..CatalogueEntry::default()
    }
}

// ByLayer first, as every CAD package lists it; then by name with case
// folded, so "kerb" sits beside "KERB" rather than after "Z", and the exact
// name breaks the tie so the order never depends on insertion.
fn sort_for_picker(entries: &mut Vec<CatalogueEntry>) {
    let rank = |entry: &CatalogueEntry| -> i32 {
        if entry.source == DefinitionSource::BuiltIn && entry.name == BY_LAYER_LINETYPE {
            0
        } else {
            1
        }
    };
    let mut keyed: Vec<(Vec<u8>, CatalogueEntry)> = entries
        .drain(..)
        .map(|entry| (lowered(&entry.name), entry))
        .collect();
    keyed.sort_by(|a, b| {
        rank(&a.1)
            .cmp(&rank(&b.1))
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.name.cmp(&b.1.name))
    });
    entries.extend(keyed.into_iter().map(|(_, entry)| entry));
}

// A line with no pattern, whatever defines it or not: ByLayer and "" say
// nothing, and "continuous", "0" and "1" are the plain continuous line in a
// style library and DXF alike (D4). None of them is a name that is missing.
fn is_plain_linetype(name: &str) -> bool {
    name.is_empty()
        || is_by_layer(name)
        || equals_ignoring_case(name.as_bytes(), CONTINUOUS_LINETYPE.as_bytes())
        || name == "0"
        || name == "1"
}

fn contains_folded(text: &str, folded_needle: &[u8]) -> bool {
    let folded = lowered(text);
    folded_needle.is_empty()
        || folded
            .windows(folded_needle.len())
            .any(|window| window == folded_needle)
}

// The users of linetype `name` less the styles that name it as their own
// symbol's name: such a style draws a plain line under its symbol (D8), so
// it does not use the name as a linetype, and neither do the entities
// wearing it. Only when some other layer or style still names it are the
// entities counted again - one pass, and only in that rare mixed case - so
// that `entities` and `first_entity` stay those of the lines drawn solid.
fn without_own_symbol_styles(document: &Document, name: &str, users: &Users) -> Users {
    let model = document.model();
    let mut own_symbol: BTreeSet<String> = BTreeSet::new();
    for style_name in &users.styles {
        if let Some(style) = model.styles.find(style_name)
            && linetype_status(document, name, &style.symbol) == NameStatus::OwnSymbol
        {
            own_symbol.insert(style_name.clone());
        }
    }
    if own_symbol.is_empty() {
        return users.clone();
    }
    let mut kept = Users {
        layers: users.layers.clone(),
        ..Users::default()
    };
    for style_name in &users.styles {
        if !own_symbol.contains(style_name) {
            kept.styles.push(style_name.clone());
        }
    }
    if !kept.named() {
        return kept; // nothing else names it: not missing at all
    }
    // table_usage's own resolution (entity's resolved_linetype), less the
    // styles set aside above.
    model.entities.for_each(|entity| {
        let style = if entity.style.is_empty() {
            None
        } else {
            model.styles.find(&entity.style)
        };
        if let Some(style) = style
            && own_symbol.contains(&style.name)
        {
            return;
        }
        if resolved_linetype(model.layers.find(&entity.layer), style) != name {
            return;
        }
        if kept.entities == 0 || entity.id < kept.first_entity {
            kept.first_entity = entity.id;
        }
        kept.entities += 1;
    });
    kept
}

/// All false for a name the library does not define: a model linetype or a
/// built-in symbol is not a library definition, and is offered by its own
/// source.
pub fn classify_definition(document: &Document, name: &str) -> DefinitionKind {
    match document.definition_for(name) {
        None => DefinitionKind::default(),
        Some(definition) => classify(definition, &SymbolEvidence::of(document)),
    }
}

/// For a Style's linetype (`for_style`, the C++ default true) the list starts
/// with ByLayer; a layer's has no ByLayer. Then every model linetype and every
/// library definition offered as a linestyle, one entry per name (a collision
/// is one entry, the library's, marked), sorted by name with case folded.
pub fn linetype_choices(document: &Document, for_style: bool) -> Vec<CatalogueEntry> {
    let model = document.model();
    let usage = table_usage(model, UsageOptions::default());
    let evidence = SymbolEvidence::of(document);
    let mut entries = Vec::new();

    if for_style {
        let mut by_layer = CatalogueEntry {
            name: String::from(BY_LAYER_LINETYPE),
            source: DefinitionSource::BuiltIn,
            ..CatalogueEntry::default()
        };
        by_layer.kind.linestyle = true;
        // Only the styles: an entity reaching a linetype through ByLayer is
        // counted under the layer's linetype, which is the one it draws.
        model.styles.for_each(|style| {
            if is_by_layer(&style.linetype) {
                by_layer.users.styles.push(style.name.clone());
            }
        });
        entries.push(by_layer);
    }
    document.style_library().for_each(|definition| {
        let kind = classify(definition, &evidence);
        if !kind.linestyle {
            return;
        }
        let mut entry = from_definition(definition, kind);
        entry.collision = model.linetypes.contains(&definition.name);
        entry.users = TableUsage::of(&usage.linetypes, &definition.name).clone();
        entries.push(entry);
    });
    model.linetypes.for_each(|linetype| {
        // A collision is listed once, as the library's: that is what draws.
        if let Some(definition) = document.definition_for(&linetype.name)
            && !definition.at_vertices
        {
            return;
        }
        let mut entry = CatalogueEntry {
            name: linetype.name.clone(),
            source: DefinitionSource::ModelLinetype,
            users: TableUsage::of(&usage.linetypes, &linetype.name).clone(),
            ..CatalogueEntry::default()
        };
        entry.kind.linestyle = true;
        entries.push(entry);
    });
    sort_for_picker(&mut entries);
    entries
}

/// Every built-in symbol shape and every library definition offered as a
/// symbol, one entry per name (a library definition hides a built-in of the
/// same name, as it does when drawn), sorted by name with case folded.
pub fn symbol_choices(document: &Document) -> Vec<CatalogueEntry> {
    let usage = table_usage(document.model(), UsageOptions::default());
    let evidence = SymbolEvidence::of(document);
    let mut entries = Vec::new();

    document.style_library().for_each(|definition| {
        let kind = classify(definition, &evidence);
        if !kind.symbol {
            return;
        }
        let mut entry = from_definition(definition, kind);
        entry.users = TableUsage::of(&usage.symbols, &definition.name).clone();
        entries.push(entry);
    });
    for &name in symbol_names() {
        // Drawn from the library when it defines the name, so listed as it.
        if document.definition_for(name).is_some() {
            continue;
        }
        let mut entry = CatalogueEntry {
            name: String::from(name),
            source: DefinitionSource::BuiltIn,
            users: TableUsage::of(&usage.symbols, name).clone(),
            ..CatalogueEntry::default()
        };
        entry.kind.symbol = true;
        entries.push(entry);
    }
    sort_for_picker(&mut entries);
    entries
}

/// How many of the library's definitions symbol_choices offers as symbols -
/// the built-in shapes no customisation brought left out. THE count of
/// symbols that the start-up line, Settings and CUSTOMISE all say, so they
/// cannot differ.
pub fn symbol_definition_count(document: &Document) -> usize {
    symbol_choices(document)
        .iter()
        .filter(|entry| entry.source == DefinitionSource::Library)
        .count()
}

/// `choices` with `current` added, marked missing, when no entry has that
/// exact name; unchanged when one has, and for "" (nothing to keep).
pub fn keep_current(mut choices: Vec<CatalogueEntry>, current: &str) -> Vec<CatalogueEntry> {
    if current.is_empty() || choices.iter().any(|entry| entry.name == current) {
        return choices;
    }
    // First, where it is seen: the value being edited, which the editor is
    // about to write back, must be the one it shows.
    let kept = CatalogueEntry {
        name: String::from(current),
        source: DefinitionSource::Undefined,
        missing: true,
        ..CatalogueEntry::default()
    };
    choices.insert(0, kept);
    choices
}

/// The entries whose name, group or source file contains `text`, case
/// folded. Everything for an empty text.
pub fn filter_choices(choices: &[CatalogueEntry], text: &str) -> Vec<CatalogueEntry> {
    if text.is_empty() {
        return choices.to_vec();
    }
    let needle = lowered(text);
    choices
        .iter()
        .filter(|entry| {
            contains_folded(&entry.name, &needle)
                || contains_folded(&entry.group, &needle)
                || contains_folded(&entry.source_file, &needle)
        })
        .cloned()
        .collect()
}

// ---- diagnostics -----------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NameRole {
    #[default]
    Linetype,
    Symbol,
}

/// The C++ `toString(NameRole)`.
impl EnumName for NameRole {
    fn name(self) -> &'static str {
        match self {
            Self::Linetype => "linetype",
            Self::Symbol => "symbol",
        }
    }
}

/// What a name a Style or a Layer gives draws as - THE rule for "is this name
/// missing", read by missing_names here and by customisation_coverage
/// (survey_coding.rs), and so by the style manager's Diagnostics, the symbol
/// library, CUSTOMISE and the window's unresolved-names log. Each once had a
/// rule of its own, and they disagreed: a linetype naming only a `mode vertex`
/// definition was "defined" to one of them while the viewport drew it solid,
/// and built-in symbol names were "in no loaded library" to another (audit
/// CAD-17). Worked out through resolve_line_pattern and resolve_symbol, the
/// viewport's own answers, so a name is missing exactly when what is drawn is
/// a fallback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NameStatus {
    /// Names nothing: "", ByLayer, "continuous", "0", "1" in any case (D4).
    Plain,
    /// A style's linetype that is its own symbol's name - an archive import's
    /// pattern for a symbol string - drawn as a plain line under the symbol (D8).
    OwnSymbol,
    /// A loaded library definition draws it.
    Library,
    /// Katana draws it itself: a model linetype, or a built-in symbol shape.
    Katana,
    /// A linetype naming only a `mode vertex` definition: a symbol, not a
    /// linestyle, so the line is drawn solid (D2).
    NotALinestyle,
    /// Nothing defines it: drawn solid, or as the built-in shape its name
    /// suggests.
    #[default]
    Undefined,
}

/// The C++ `toString(NameStatus)`: "plain line", "own symbol", "library",
/// "built in", "not a linestyle", "defined nowhere".
impl EnumName for NameStatus {
    fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain line",
            Self::OwnSymbol => "own symbol",
            Self::Library => "library",
            Self::Katana => "built in",
            Self::NotALinestyle => "not a linestyle",
            Self::Undefined => "defined nowhere",
        }
    }
}

/// What missing_names reports: a name whose drawing is a fallback.
pub const fn is_missing(status: NameStatus) -> bool {
    matches!(status, NameStatus::NotALinestyle | NameStatus::Undefined)
}

/// A linetype as a Style or Layer names it. `own_symbol` is the Style's own
/// symbol, for D8's exception; a Layer has no symbol and passes "" (the C++
/// default).
pub fn linetype_status(document: &Document, linetype: &str, own_symbol: &str) -> NameStatus {
    if is_plain_linetype(linetype) {
        return NameStatus::Plain;
    }
    let resolved = resolve_line_pattern(
        document.model(),
        document.style_library(),
        linetype,
        own_symbol,
    );
    match resolved.kind {
        LinetypeKind::LibraryDefinition => return NameStatus::Library,
        LinetypeKind::ModelLinetype => return NameStatus::Katana,
        LinetypeKind::Solid => {}
    }
    // Solid: why, in the order resolve_line_pattern decides it.
    if !own_symbol.is_empty() && linetype == own_symbol {
        return NameStatus::OwnSymbol;
    }
    if document.definition_for(linetype).is_some() {
        NameStatus::NotALinestyle
    } else {
        NameStatus::Undefined
    }
}

/// A Style's symbol: Plain for "" (the viewport's plain point mark), Library,
/// Katana for a built-in shape, else Undefined. Never NotALinestyle: every
/// library definition, `mode vertex` or not, can be drawn as a symbol (D3).
pub fn symbol_status(document: &Document, symbol: &str) -> NameStatus {
    match resolve_symbol(document.style_library(), symbol).kind {
        SymbolKind::None => NameStatus::Plain,
        SymbolKind::LibraryDefinition => NameStatus::Library,
        SymbolKind::BuiltIn => NameStatus::Katana,
        SymbolKind::BuiltInFallback => NameStatus::Undefined,
    }
}

/// A name a Style or Layer uses whose drawing is a fallback.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MissingName {
    pub name: String,
    pub role: NameRole,
    /// Who names it, and the entities reaching it. For a linetype, not the
    /// styles that name it as their own symbol (D8): those draw no pattern.
    pub users: Users,
    /// What is drawn instead: "continuous" (a solid line) for a linetype, and
    /// for a symbol the built-in shape entity's built_in_symbol_for picks.
    pub fallback: String,
    /// Why: Undefined (nothing defines it), or, for a linetype only,
    /// NotALinestyle (a `mode vertex` symbol defines it, and no linestyle).
    pub status: NameStatus,
}

/// Every name a Style or Layer gives whose status is_missing - linetypes
/// first, then symbols, each ascending. So NOT missing: a model linetype, a
/// library linestyle, a built-in symbol name ("cross" draws a cross; audit
/// CAD-17), ByLayer, the plain-line names "continuous", "0" and "1" in any
/// case (D4), and a style's linetype that is its own symbol's name (D8),
/// which is what an archive import writes for every symbol string. Missing,
/// as "not a linestyle": a linetype naming only a `mode vertex` definition,
/// which the viewport draws solid and a linetype picker does not offer (D2,
/// D3).
pub fn missing_names(document: &Document) -> Vec<MissingName> {
    let model = document.model();
    let usage = table_usage(model, UsageOptions::default());
    let mut missing = Vec::new();

    for (name, users) in &usage.linetypes {
        // Only what a layer or style NAMES: a name reached by entities alone
        // is a default the chain fell back to, not something anyone chose.
        if !users.named() {
            continue;
        }
        // As a layer names it: a layer has no symbol of its own.
        let status = linetype_status(document, name, "");
        if !is_missing(status) {
            continue;
        }
        let naming = without_own_symbol_styles(document, name, users);
        if !naming.named() {
            continue;
        }
        missing.push(MissingName {
            name: name.clone(),
            role: NameRole::Linetype,
            users: naming,
            fallback: String::from(CONTINUOUS_LINETYPE),
            status,
        });
    }
    for (name, users) in &usage.symbols {
        let status = symbol_status(document, name);
        if !users.named() || !is_missing(status) {
            continue;
        }
        missing.push(MissingName {
            name: name.clone(),
            role: NameRole::Symbol,
            users: users.clone(),
            fallback: String::from(built_in_symbol_for(name)),
            status,
        });
    }
    missing
}

/// Names that are both a model Linetype and a non-vertex library definition
/// (D2's collisions), ascending.
pub fn linetype_collisions(document: &Document) -> Vec<String> {
    let mut names = Vec::new();
    document.model().linetypes.for_each(|linetype| {
        if let Some(definition) = document.definition_for(&linetype.name)
            && !definition.at_vertices
        {
            names.push(linetype.name.clone());
        }
    });
    names
}

/// `base` when the style table has no such name, else "base 2", "base 3" ...
/// the first free one. For the style manager's Duplicate and "New Style Using
/// This", which then offer the name to be changed, and for the symbol
/// library's Assign, which names a new style after its symbol.
pub fn free_style_name(model: &Model, base: &str) -> String {
    let mut name = String::from(base);
    let mut suffix = 2;
    while model.styles.contains(&name) {
        name = format!("{base} {suffix}");
        suffix += 1;
    }
    name
}

/// The one answer to "may a style or a layer be given this linetype name":
/// a model linetype, or a loaded library definition drawn along a line - the
/// same two places the viewport resolves it in (D2). A library linestyle used
/// to be refused here although the model and the viewport both take one
/// (audit CAD-06). A vertex symbol is refused BY NAME, because it is never
/// drawn as a line pattern (D8) and "does not exist" would be untrue. The
/// command line's STYLE SET and LAYER LTYPE ask it, and so does Global Modify
/// (global_modify.rs), so the two cannot disagree about a name.
pub fn check_linetype_name(document: &Document, name: &str) -> Status {
    if document.model().linetypes.contains(name) {
        return Ok(());
    }
    if let Some(definition) = document.definition_for(name) {
        if definition.at_vertices {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "that is an `at vertices` symbol, not a linestyle: give it as the symbol",
                name,
            ));
        }
        return Ok(());
    }
    Err(make_error(
        ErrorCode::NotFound,
        "no linetype of that name is in the drawing or the loaded library",
        name,
    ))
}
