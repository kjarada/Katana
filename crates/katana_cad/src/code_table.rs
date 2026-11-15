//! The survey code library as something a person can question: why a code
//! gets what it gets, the map as a table of codes, which codes a drawing
//! actually carries, and what is wrong with a map before it is applied
//! (include/katana/cad/code_table.hpp, src/katana_cad/customisation/code_table.cpp).
//!
//! These are the foundations of the survey-code manager, and of the verbs
//! CODE EXPLAIN, CODE CENSUS, CODE LIST and CODE CHECK. They are here, below
//! both front ends, so that the GUI and the CLI print the SAME thing: the two
//! used to disagree about what applying codes had done.
//!
//! Library definitions and colours come in through callbacks, as they do for
//! apply_survey_codes: the standard colour names are known to the archive
//! layer, which cad may not see, and a caller holding a Document passes
//! definition_for. The C++'s `std::function` parameters, which may be empty,
//! are `Option<&dyn Fn>` here.

use std::collections::BTreeMap;

use katana_core::text::{format_exact_real, is_ascii_space, lowered, trimmed};
use katana_entity::layer_path::validate_layer_path;
use katana_entity::tables::is_built_in_symbol_name;
use katana_entity::{
    Color, EntityId, EnumName, LineStyle, StyleLibrary, SurveyAttribute, SurveyMap,
    SurveyMatchKind, SurveyPipe, SurveyRule, SurveySection, SurveySymbol, SurveyTextStyle,
};

use crate::document::Document;
use crate::survey_coding::{
    CustomisationCoverage, SurveyCodingReport, SurveyStyleOutcome, find_code_property,
    is_plain_linestyle, survey_code_of,
};

/// The library definition of a name, or None (Document::definition_for).
pub type DefinitionLookup<'a> = dyn Fn(&str) -> Option<&'a LineStyle> + 'a;
/// A standard colour name's RGB, or None for a name it does not know.
pub type ColourLookup<'a> = dyn Fn(&str) -> Option<Color> + 'a;
/// Whether Katana draws a symbol of this name without a library
/// (entity's is_built_in_symbol_name).
pub type BuiltInSymbolTest<'a> = dyn Fn(&str) -> bool + 'a;

// ---- explaining one code ------------------------------------------------------

/// A later matching rule that sets a field to something ELSE and loses: the
/// C++ `CodeFieldSource::Overruled`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeFieldSourceOverruled {
    pub rule: usize,
    pub key: String,
    pub section: SurveySection,
    pub value: String,
}

/// Where one field of a code's combined result came from: the FIRST rule, most
/// specific first, that says anything about it - the rule SurveyMap::lookup
/// takes it from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeFieldSource {
    /// "layer", "colour", "linestyle", "weight", "group", "comment",
    /// "breakline", "surface", "hide", "symbol", "text style", "pipe",
    /// "vertex pipe" or "segment pipe". "layer" is SurveyRule::model and
    /// "surface" SurveyRule::tinable, by the words a customisation file has
    /// for them.
    pub field: String,
    /// As text, for a person to read.
    pub value: String,
    /// Its index in SurveyMap::rules().
    pub rule: usize,
    pub key: String,
    pub section: SurveySection,
    /// Later matching rules that set this field to something ELSE and lose -
    /// most often the second of two loaded mapfiles disagreeing (the built-in
    /// pair differ on `hide` for 190 keys). Most specific first.
    pub overruled: Vec<CodeFieldSourceOverruled>,
}

/// What a linestyle or symbol name resolves to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeDefinition {
    /// As the rules give it; empty when they give none.
    pub name: String,
    /// The plain continuous line: "0", "1", "continuous".
    pub plain: bool,
    /// The library has a definition of this name.
    pub defined: bool,
    /// ... and it is drawn at vertices (`at_vertices`).
    pub vertex_mode: bool,
    /// A symbol no library defines but Katana draws itself.
    pub built_in: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeColour {
    pub name: String,
    /// None with a name: a colour name nothing here knows, which leaves the
    /// entity's colour alone when codes are applied.
    pub rgb: Option<Color>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeAttribute {
    /// "string", "vertex" or "segment" - which list it is in.
    pub scope: String,
    pub attribute: SurveyAttribute,
    /// Its value names another attribute ("$PipeDiameter"), which needs the
    /// survey data the drawing came from: it is counted and left, not set.
    pub deferred: bool,
    pub rule: usize,
    pub key: String,
    pub section: SurveySection,
}

/// A key the code would have met but for letter case or surrounding blanks -
/// the reason "wm01" gets nothing from the rule for "WM*", which no rule will
/// say for itself: matching is byte for byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NearMissKind {
    #[default]
    Case,
    Whitespace,
}

/// The C++ `toString(NearMissKind)`.
impl EnumName for NearMissKind {
    fn name(self) -> &'static str {
        match self {
            NearMissKind::Case => "differs only in letter case",
            NearMissKind::Whitespace => "differs by blanks around the code",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeNearMiss {
    pub key: String,
    pub kind: NearMissKind,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodeExplanation {
    pub code: String,
    pub kind: SurveyMatchKind,
    /// SurveyMatch::matched(), decision D5.
    pub matched: bool,
    /// Exactly what SurveyMap::lookup gives.
    pub resolved: SurveyRule,
    /// Every rule that matched, most specific first.
    pub rules: Vec<usize>,
    /// The fields the combination sets, in the order above.
    pub fields: Vec<CodeFieldSource>,
    pub linestyle: CodeDefinition,
    pub symbol: CodeDefinition,
    pub colour: CodeColour,
    /// As combined: string, then vertex, then segment.
    pub attributes: Vec<CodeAttribute>,
    /// In key order.
    pub near_misses: Vec<CodeNearMiss>,
}

// A key's own shape, as SurveyMap reads it: a trailing `*` makes a prefix,
// and the bare `*` alone is the fallback every code meets.
fn kind_of_key(key: &[u8]) -> SurveyMatchKind {
    if key.last() != Some(&b'*') {
        return SurveyMatchKind::Exact;
    }
    if key.len() == 1 {
        SurveyMatchKind::FallbackOnly
    } else {
        SurveyMatchKind::Prefix
    }
}

// Whether `key` catches `code`, byte for byte - the same test SurveyMap
// makes, repeated here only to ask it of a code the map was not given (a
// folded or trimmed one).
fn key_catches(key: &[u8], code: &[u8]) -> bool {
    if kind_of_key(key) == SurveyMatchKind::Exact {
        return key == code;
    }
    code.starts_with(&key[..key.len() - 1])
}

fn real(value: f64) -> String {
    format_exact_real(value)
}

fn yes_no(value: bool) -> String {
    String::from(if value { "yes" } else { "no" })
}

fn symbol_text(symbol: &SurveySymbol) -> String {
    let mut text = if symbol.style.is_empty() {
        String::from("(no name)")
    } else {
        symbol.style.clone()
    };
    if symbol.size != 0.0 {
        text.push_str(&format!(", size {}", real(symbol.size)));
    }
    if !symbol.colour.is_empty() {
        text.push_str(&format!(", colour {}", symbol.colour));
    }
    if symbol.rotation != 0.0 {
        text.push_str(&format!(", rotation {}", real(symbol.rotation)));
    }
    if symbol.offset != 0.0 {
        text.push_str(&format!(", offset {}", real(symbol.offset)));
    }
    if symbol.raise != 0.0 {
        text.push_str(&format!(", raise {}", real(symbol.raise)));
    }
    text
}

fn text_style_text(style: &SurveyTextStyle) -> String {
    let mut text = if style.textstyle.is_empty() {
        String::from("(default)")
    } else {
        style.textstyle.clone()
    };
    if style.size != 0.0 {
        text.push_str(&format!(", size {}", real(style.size)));
        if !style.r#type.is_empty() {
            text.push_str(&format!(" {}", style.r#type));
        }
    }
    if !style.colour.is_empty() {
        text.push_str(&format!(", colour {}", style.colour));
    }
    text
}

fn pipe_text(pipe: &SurveyPipe) -> String {
    let mut text = if pipe.shape.is_empty() {
        String::from("(no shape)")
    } else {
        pipe.shape.clone()
    };
    if !pipe.size1.is_empty() {
        text.push_str(&format!(" {}", pipe.size1));
        if !pipe.size2.is_empty() {
            text.push_str(&format!(" x {}", pipe.size2));
        }
    }
    if !pipe.justify.is_empty() {
        text.push_str(&format!(", {}", pipe.justify));
    }
    if !pipe.active {
        text.push_str(", inactive");
    }
    text
}

// The fields a rule can set, in the order CodeExplanation::fields lists them,
// each with the test SurveyMap::lookup makes - an empty string or an absent
// optional says nothing - and the value as a person reads it. A value is only
// asked of a rule that sets the field.
struct FieldSpec {
    name: &'static str,
    sets: fn(&SurveyRule) -> bool,
    text: fn(&SurveyRule) -> String,
}

fn field_specs() -> [FieldSpec; 14] {
    [
        FieldSpec {
            name: "layer",
            sets: |r| !r.model.is_empty(),
            text: |r| r.model.clone(),
        },
        FieldSpec {
            name: "colour",
            sets: |r| !r.colour.is_empty(),
            text: |r| r.colour.clone(),
        },
        FieldSpec {
            name: "linestyle",
            sets: |r| !r.linestyle.is_empty(),
            text: |r| r.linestyle.clone(),
        },
        FieldSpec {
            name: "weight",
            sets: |r| !r.weight.is_empty(),
            text: |r| r.weight.clone(),
        },
        FieldSpec {
            name: "group",
            sets: |r| !r.group.is_empty(),
            text: |r| r.group.clone(),
        },
        FieldSpec {
            name: "comment",
            sets: |r| !r.comment.is_empty(),
            text: |r| r.comment.clone(),
        },
        FieldSpec {
            name: "breakline",
            sets: |r| r.breakline.is_some(),
            text: |r| {
                r.breakline
                    .map_or_else(String::new, |breakline| String::from(breakline.name()))
            },
        },
        FieldSpec {
            name: "surface",
            sets: |r| r.tinable.is_some(),
            text: |r| r.tinable.map_or_else(String::new, yes_no),
        },
        FieldSpec {
            name: "hide",
            sets: |r| r.hide.is_some(),
            text: |r| r.hide.map_or_else(String::new, yes_no),
        },
        FieldSpec {
            name: "symbol",
            sets: |r| r.symbol.is_some(),
            text: |r| r.symbol.as_ref().map_or_else(String::new, symbol_text),
        },
        FieldSpec {
            name: "text style",
            sets: |r| r.text_style.is_some(),
            text: |r| {
                r.text_style
                    .as_ref()
                    .map_or_else(String::new, text_style_text)
            },
        },
        FieldSpec {
            name: "pipe",
            sets: |r| r.pipe.is_some(),
            text: |r| r.pipe.as_ref().map_or_else(String::new, pipe_text),
        },
        FieldSpec {
            name: "vertex pipe",
            sets: |r| r.vertex_pipe.is_some(),
            text: |r| r.vertex_pipe.as_ref().map_or_else(String::new, pipe_text),
        },
        FieldSpec {
            name: "segment pipe",
            sets: |r| r.segment_pipe.is_some(),
            text: |r| r.segment_pipe.as_ref().map_or_else(String::new, pipe_text),
        },
    ]
}

// The three attribute lists, by the scope word CodeAttribute uses.
fn string_attributes(rule: &SurveyRule) -> &[SurveyAttribute] {
    &rule.attributes
}

fn vertex_attributes(rule: &SurveyRule) -> &[SurveyAttribute] {
    &rule.vertex_attributes
}

fn segment_attributes(rule: &SurveyRule) -> &[SurveyAttribute] {
    &rule.segment_attributes
}

type AttributeList = fn(&SurveyRule) -> &[SurveyAttribute];

const ATTRIBUTE_SCOPES: [(&str, AttributeList); 3] = [
    ("string", string_attributes),
    ("vertex", vertex_attributes),
    ("segment", segment_attributes),
];

fn names_attribute(list: &[SurveyAttribute], name: &str) -> bool {
    list.iter().any(|attribute| attribute.name == name)
}

fn is_deferred(attribute: &SurveyAttribute) -> bool {
    attribute.value.starts_with('$')
}

// Whether a definition may be drawn at a vertex (decision D3): it is `mode
// vertex`, or its customisation lists it as a symbol. There is no third case:
// every definition says which it is, so one that does not say "symbol" is a
// linestyle.
fn symbol_capable(definition: &LineStyle) -> bool {
    definition.at_vertices || definition.symbol
}

// What a rule holds that its section does not use, in the words a person
// knows the fields by; empty when everything it holds belongs. Each section
// is about one aspect of a code, and the table is what the survey code file
// writer enforced (it refused to write such a rule, having no element for the
// field): where the code goes in `Map`; its symbol in `VertexSymbol`; its
// text in `VertexTextStyle`; the three pipes each in their own; the string's
// attributes in `Pipe` and `StringAttribute`, each vertex's in `VertexPipe`
// and `VertexAttribute`, each segment's in `SegmentPipe`; `Tinable`'s flag in
// its own. A comment may sit on any rule.
fn fields_outside_section(rule: &SurveyRule) -> Vec<&'static str> {
    let s = rule.section;
    let mut outside = Vec::new();
    let mut unless = |belongs: bool, held: bool, field: &'static str| {
        if held && !belongs {
            outside.push(field);
        }
    };
    // By the words every other reply names a field by ("layer" and "surface"
    // for SurveyRule::model and ::tinable; docs/customisation.md, "The words a
    // rule is shown by"): CODE CHECK prints this beside CODE EXPLAIN's.
    let map = s == SurveySection::Map;
    unless(map, !rule.model.is_empty(), "layer");
    unless(map, !rule.colour.is_empty(), "colour");
    unless(map, !rule.linestyle.is_empty(), "linestyle");
    unless(map, !rule.weight.is_empty(), "weight");
    unless(map, !rule.group.is_empty(), "group");
    unless(map, rule.breakline.is_some(), "breakline");
    unless(
        s == SurveySection::Tinable,
        rule.tinable.is_some(),
        "surface",
    );
    unless(
        s == SurveySection::VertexSymbol,
        rule.hide.is_some(),
        "hide",
    );
    unless(
        s == SurveySection::VertexSymbol,
        rule.symbol.is_some(),
        "symbol",
    );
    unless(
        s == SurveySection::VertexTextStyle,
        rule.text_style.is_some(),
        "text style",
    );
    unless(s == SurveySection::Pipe, rule.pipe.is_some(), "pipe");
    unless(
        s == SurveySection::VertexPipe,
        rule.vertex_pipe.is_some(),
        "vertex pipe",
    );
    unless(
        s == SurveySection::SegmentPipe,
        rule.segment_pipe.is_some(),
        "segment pipe",
    );
    unless(
        s == SurveySection::Pipe || s == SurveySection::StringAttribute,
        !rule.attributes.is_empty(),
        "string attributes",
    );
    unless(
        s == SurveySection::VertexPipe || s == SurveySection::VertexAttribute,
        !rule.vertex_attributes.is_empty(),
        "vertex attributes",
    );
    unless(
        s == SurveySection::SegmentPipe,
        !rule.segment_attributes.is_empty(),
        "segment attributes",
    );
    outside
}

fn in_quotes(text: &str) -> String {
    format!("\"{text}\"")
}

fn joined<S: AsRef<str>>(items: &[S], with: &str) -> String {
    let mut text = String::new();
    for (i, item) in items.iter().enumerate() {
        if i != 0 {
            text.push_str(with);
        }
        text.push_str(item.as_ref());
    }
    text
}

fn rule_ref(index: usize, key: &str, section: SurveySection) -> String {
    format!("rule #{index} {key} ({})", section.name())
}

fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// The definition `definition_of` gives a name, or None when there is no
// lookup or no name.
fn definition_named<'a>(
    definition_of: Option<&DefinitionLookup<'a>>,
    name: &str,
) -> Option<&'a LineStyle> {
    match definition_of {
        Some(lookup) if !name.is_empty() => lookup(name),
        _ => None,
    }
}

pub fn explain_code(
    map: &SurveyMap,
    code: &str,
    definition_of: Option<&DefinitionLookup<'_>>,
    colour_of: Option<&ColourLookup<'_>>,
) -> CodeExplanation {
    let found = map.lookup(code);
    let mut out = CodeExplanation {
        code: String::from(code),
        kind: found.kind,
        matched: found.matched(),
        resolved: found.resolved.clone(),
        rules: map.match_indices(code),
        ..CodeExplanation::default()
    };
    let rules = map.rules();

    // The first rule, most specific first, that sets a field is the one
    // lookup took it from; any later one saying otherwise is overruled.
    for spec in field_specs() {
        let mut source: Option<usize> = None;
        for &index in &out.rules {
            let rule = &rules[index];
            if !(spec.sets)(rule) {
                continue;
            }
            let value = (spec.text)(rule);
            match source {
                None => {
                    out.fields.push(CodeFieldSource {
                        field: String::from(spec.name),
                        value,
                        rule: index,
                        key: rule.key.clone(),
                        section: rule.section,
                        overruled: Vec::new(),
                    });
                    source = Some(out.fields.len() - 1);
                }
                Some(at) => {
                    if value != out.fields[at].value {
                        out.fields[at].overruled.push(CodeFieldSourceOverruled {
                            rule: index,
                            key: rule.key.clone(),
                            section: rule.section,
                            value,
                        });
                    }
                }
            }
        }
    }

    out.linestyle.name.clone_from(&found.resolved.linestyle);
    out.linestyle.plain = !out.linestyle.name.is_empty() && is_plain_linestyle(&out.linestyle.name);
    if !out.linestyle.plain
        && let Some(definition) = definition_named(definition_of, &out.linestyle.name)
    {
        out.linestyle.defined = true;
        out.linestyle.vertex_mode = definition.at_vertices;
    }
    if let Some(symbol) = &found.resolved.symbol {
        out.symbol.name.clone_from(&symbol.style);
    }
    if let Some(definition) = definition_named(definition_of, &out.symbol.name) {
        out.symbol.defined = true;
        out.symbol.vertex_mode = definition.at_vertices;
    } else {
        out.symbol.built_in =
            !out.symbol.name.is_empty() && is_built_in_symbol_name(&out.symbol.name);
    }

    out.colour.name.clone_from(&found.resolved.colour);
    if let Some(lookup) = colour_of
        && !out.colour.name.is_empty()
    {
        out.colour.rgb = lookup(&out.colour.name);
    }

    // An attribute comes from the first matching rule that names it in the
    // same list - lookup keeps the first of each name.
    for (scope, of) in ATTRIBUTE_SCOPES {
        for attribute in of(&found.resolved) {
            let mut entry = CodeAttribute {
                scope: String::from(scope),
                attribute: attribute.clone(),
                deferred: is_deferred(attribute),
                ..CodeAttribute::default()
            };
            for &index in &out.rules {
                if names_attribute(of(&rules[index]), &attribute.name) {
                    entry.rule = index;
                    entry.key.clone_from(&rules[index].key);
                    entry.section = rules[index].section;
                    break;
                }
            }
            out.attributes.push(entry);
        }
    }

    // Keys this code misses only by case or by blanks. The bare `*` is never
    // one: it catches every code already.
    let code_bytes = code.as_bytes();
    let trimmed_code = trimmed(code_bytes);
    let padded = trimmed_code.len() != code_bytes.len();
    let folded_code = lowered(code_bytes);
    let folded_trimmed = lowered(trimmed_code);
    for key in map.keys() {
        if key == "*" || key_catches(key.as_bytes(), code_bytes) {
            continue;
        }
        let folded_key = lowered(&key);
        if padded
            && (key_catches(key.as_bytes(), trimmed_code)
                || key_catches(&folded_key, &folded_trimmed))
        {
            out.near_misses.push(CodeNearMiss {
                key,
                kind: NearMissKind::Whitespace,
            });
        } else if key_catches(&folded_key, &folded_code) {
            out.near_misses.push(CodeNearMiss {
                key,
                kind: NearMissKind::Case,
            });
        }
    }
    out
}

// ---- the map as a table of codes ------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodeTableRow {
    pub key: String,
    /// The key's own shape: Exact, Prefix, or FallbackOnly for the bare `*`.
    pub kind: SurveyMatchKind,
    /// The rules with exactly this key, in map order.
    pub rules: Vec<usize>,
    /// Theirs, distinct, in section order.
    pub sections: Vec<SurveySection>,
    /// What a code this key catches resolves to, the less specific keys that
    /// also catch it included: lookup(key) - for "WM*" that is WM*, then W*,
    /// then `*`.
    pub combined: SurveyRule,
}

/// One row per distinct key, in key order.
pub fn code_table(map: &SurveyMap) -> Vec<CodeTableRow> {
    let mut by_key: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let rules = map.rules();
    for (i, rule) in rules.iter().enumerate() {
        by_key.entry(rule.key.clone()).or_default().push(i);
    }
    let mut rows = Vec::with_capacity(by_key.len());
    for (key, indices) in by_key {
        // Distinct, in the order the enumeration declares them (the C++'s
        // std::set<SurveySection>).
        let mut sections: Vec<SurveySection> =
            indices.iter().map(|&index| rules[index].section).collect();
        sections.sort_by_key(|section| *section as u8);
        sections.dedup();
        // A key is itself a code its own rules catch: "WM*" read as a code
        // starts with "WM", so lookup gives WM*, then the shorter prefixes,
        // then `*` - and no exact key, since none ends in `*`.
        let combined = map.lookup(&key).resolved;
        rows.push(CodeTableRow {
            kind: kind_of_key(key.as_bytes()),
            key,
            rules: indices,
            sections,
            combined,
        });
    }
    rows
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Whether a row is one a person searching for `filter` means: a substring of
/// its key, comment, group, model, colour, linestyle or symbol, with case
/// ignored - searching folds case, storing never does (decision D3). An empty
/// filter matches every row.
pub fn code_table_row_matches(row: &CodeTableRow, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let needle = lowered(filter);
    let combined = &row.combined;
    for field in [
        &row.key,
        &combined.comment,
        &combined.group,
        &combined.model,
        &combined.colour,
        &combined.linestyle,
    ] {
        if contains_bytes(&lowered(field), &needle) {
            return true;
        }
    }
    combined
        .symbol
        .as_ref()
        .is_some_and(|symbol| contains_bytes(&lowered(&symbol.style), &needle))
}

// ---- the codes a drawing carries ------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeCensusRow {
    pub code: String,
    pub entities: usize,
    pub kind: SurveyMatchKind,
    pub matched: bool,
    /// What the combined rule gives, empty for none.
    pub model: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeCensus {
    /// What was asked for, or what was found.
    pub property: String,
    /// Entities carrying a code.
    pub coded: usize,
    /// One per distinct code, in code order.
    pub codes: Vec<CodeCensusRow>,
}

/// Every distinct code the drawing's entities carry under `property` (empty:
/// found as apply_survey_codes finds it), counted, and classed against the
/// loaded map. Each distinct code is looked up once. (The C++ defaults
/// `property` to empty.)
pub fn code_census(document: &Document, property: &str) -> CodeCensus {
    code_census_of(document, &document.model().entities.ids(), property)
}

/// The same over `ids` alone - what a scope took (CODE CENSUS <scope>). An
/// EMPTY list is no entity, and so a census of nothing: unlike
/// SurveyCodingOptions::ids, where empty means every entity, because a scope
/// that took nothing must count nothing. (The C++'s second overload of
/// codeCensus.)
pub fn code_census_of(document: &Document, ids: &[EntityId], property: &str) -> CodeCensus {
    let model = document.model();
    let mut census = CodeCensus {
        property: if property.is_empty() {
            find_code_property(model, ids)
        } else {
            String::from(property)
        },
        ..CodeCensus::default()
    };

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for &id in ids {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        if let Some(code) = survey_code_of(entity, &census.property) {
            census.coded += 1;
            *counts.entry(String::from(code)).or_default() += 1;
        }
    }
    let map = document.survey_map();
    census.codes.reserve(counts.len());
    for (code, entities) in counts {
        let found = map.lookup(&code);
        let matched = found.matched();
        census.codes.push(CodeCensusRow {
            code,
            entities,
            kind: found.kind,
            matched,
            model: found.resolved.model,
        });
    }
    census
}

// ---- what is wrong with a map -----------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LintSeverity {
    /// The rule cannot be applied as written.
    Error,
    /// It applies, but not as its author meant.
    #[default]
    Warning,
}

/// The kinds in the order the issues of one rule are listed in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LintKind {
    /// A linestyle no loaded library defines.
    #[default]
    UnresolvedLinestyle,
    /// A symbol no library defines and Katana cannot draw.
    UnresolvedSymbol,
    /// A symbol rule naming a definition known not to be a symbol.
    SymbolNotSymbolCapable,
    /// A linestyle naming a definition drawn at vertices.
    LinestyleIsVertex,
    /// A colour name the colour table does not know.
    UnknownColour,
    /// A layer (SurveyRule::model) that is not a layer path.
    InvalidLayerPath,
    /// A feature rule with no layer: its code stays where it is.
    NoModel,
    /// The same as an earlier rule, field for field.
    DuplicateRule,
    /// Earlier rules with its key already say all it says.
    ShadowedRule,
    /// A key with surrounding blanks, which matches no code typed.
    KeyWhitespace,
    /// A field its section does not use: a symbol rule that also names a
    /// layer. The model allows it and a lookup applies it; a rule's identity
    /// in a load is its key in its section, though, so such a field is
    /// replaced along with rules it does not belong to. (Last, so that the
    /// order the issues of one rule are listed in is unchanged for the kinds
    /// before it.)
    FieldOutsideSection,
}

/// The C++ `toString(LintSeverity)`.
impl EnumName for LintSeverity {
    fn name(self) -> &'static str {
        match self {
            LintSeverity::Error => "error",
            LintSeverity::Warning => "warning",
        }
    }
}

/// The C++ `toString(LintKind)`.
impl EnumName for LintKind {
    fn name(self) -> &'static str {
        match self {
            LintKind::UnresolvedLinestyle => "unresolved linestyle",
            LintKind::UnresolvedSymbol => "unresolved symbol",
            LintKind::SymbolNotSymbolCapable => "not a symbol",
            LintKind::LinestyleIsVertex => "linestyle is a symbol",
            LintKind::UnknownColour => "unknown colour",
            LintKind::InvalidLayerPath => "invalid layer path",
            LintKind::NoModel => "no layer",
            LintKind::DuplicateRule => "duplicate rule",
            LintKind::ShadowedRule => "shadowed rule",
            LintKind::KeyWhitespace => "key whitespace",
            LintKind::FieldOutsideSection => "field outside section",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LintIssue {
    /// Index in SurveyMap::rules().
    pub rule: usize,
    pub key: String,
    pub section: SurveySection,
    pub severity: LintSeverity,
    pub kind: LintKind,
    pub message: String,
}

fn issue_of(
    rule: &SurveyRule,
    index: usize,
    severity: LintSeverity,
    kind: LintKind,
    message: String,
) -> LintIssue {
    LintIssue {
        rule: index,
        key: rule.key.clone(),
        section: rule.section,
        severity,
        kind,
        message,
    }
}

/// What is wrong with ONE rule on its own - everything but DuplicateRule and
/// ShadowedRule, which need the rest of the map. For an editor's form before
/// the rule is committed: SurveyMap refuses a rule validate() refuses, so
/// KeyWhitespace and InvalidLayerPath can only be seen here, on a rule not
/// yet in a map. `index` is what the issues cite.
///
/// A symbol is taken to be symbol-capable when it is `mode vertex` or its
/// customisation lists it as a symbol (LineStyle::symbol; decision D3) - most
/// symbols the reference survey codes use are not `mode vertex`. A definition
/// says which it is, so none is of unknown kind: one not listed as a symbol is
/// a linestyle. `colour_of` and `is_built_in_symbol` may be None: then no
/// colour is checked, and no symbol name is excused as built in.
pub fn lint_survey_rule(
    rule: &SurveyRule,
    index: usize,
    library: &StyleLibrary,
    colour_of: Option<&ColourLookup<'_>>,
    is_built_in_symbol: Option<&BuiltInSymbolTest<'_>>,
) -> Vec<LintIssue> {
    let mut issues = Vec::new();
    let mut report = |severity: LintSeverity, kind: LintKind, message: String| {
        issues.push(issue_of(rule, index, severity, kind, message));
    };

    if !rule.linestyle.is_empty() && !is_plain_linestyle(&rule.linestyle) {
        match library.find(&rule.linestyle) {
            None => report(
                LintSeverity::Warning,
                LintKind::UnresolvedLinestyle,
                format!(
                    "linestyle {} is in no loaded library, so it draws as a plain line",
                    in_quotes(&rule.linestyle)
                ),
            ),
            // A vertex definition is a symbol: drawn at each vertex, never
            // along the line (decision D2).
            Some(found) if found.at_vertices => report(
                LintSeverity::Warning,
                LintKind::LinestyleIsVertex,
                format!(
                    "linestyle {} is a symbol (at vertices), drawn at each vertex rather than \
                     along the line",
                    in_quotes(&rule.linestyle)
                ),
            ),
            Some(_) => {}
        }
    }
    if let Some(symbol) = &rule.symbol
        && !symbol.style.is_empty()
    {
        let name = &symbol.style;
        match library.find(name) {
            None => {
                if !is_built_in_symbol.is_some_and(|test| test(name)) {
                    report(
                        LintSeverity::Warning,
                        LintKind::UnresolvedSymbol,
                        format!(
                            "symbol {} is in no loaded library and is not one Katana draws itself",
                            in_quotes(name)
                        ),
                    );
                }
            }
            // Where it is listed so, when the definition says where it is
            // from: one made in a session names no customisation.
            Some(found) if !symbol_capable(found) => report(
                LintSeverity::Warning,
                LintKind::SymbolNotSymbolCapable,
                format!(
                    "symbol {} is a linestyle{}, not a symbol",
                    in_quotes(name),
                    if found.source.is_empty() {
                        String::new()
                    } else {
                        format!(" from {}", in_quotes(&found.source))
                    }
                ),
            ),
            Some(_) => {}
        }
    }
    if let Some(lookup) = colour_of {
        let mut named: Vec<(&str, &str)> = vec![(rule.colour.as_str(), "colour")];
        if let Some(symbol) = &rule.symbol {
            named.push((symbol.colour.as_str(), "symbol colour"));
        }
        if let Some(text) = &rule.text_style {
            named.push((text.colour.as_str(), "text colour"));
        }
        for (name, what) in named {
            if !name.is_empty() && lookup(name).is_none() {
                report(
                    LintSeverity::Warning,
                    LintKind::UnknownColour,
                    format!(
                        "{what} {} is not a colour name Katana knows, so the entity keeps its own",
                        in_quotes(name)
                    ),
                );
            }
        }
    }
    if !rule.model.is_empty() {
        if let Err(error) = validate_layer_path(&rule.model) {
            report(
                LintSeverity::Error,
                LintKind::InvalidLayerPath,
                format!(
                    "layer {} is not a layer path: {}",
                    in_quotes(&rule.model),
                    error.message
                ),
            );
        }
    } else if rule.section == SurveySection::Map {
        report(
            LintSeverity::Warning,
            LintKind::NoModel,
            String::from(
                "a feature rule with no layer: its codes stay on whatever layer they are on",
            ),
        );
    }
    let key = rule.key.as_bytes();
    if key.first().is_some_and(|byte| is_ascii_space(*byte))
        || key.last().is_some_and(|byte| is_ascii_space(*byte))
    {
        report(
            LintSeverity::Error,
            LintKind::KeyWhitespace,
            format!(
                "key {} has blanks around it, so it matches no code a surveyor types",
                in_quotes(&rule.key)
            ),
        );
    }
    let outside = fields_outside_section(rule);
    if !outside.is_empty() {
        // A warning, not an error: a lookup takes a field from whichever rule
        // says it, so the rule applies. But a rule is known by its key IN ITS
        // SECTION, and that is what a load replaces - so a field outside its
        // section comes and goes with rules it has nothing to do with.
        let section = rule.section.name();
        report(
            LintSeverity::Warning,
            LintKind::FieldOutsideSection,
            format!(
                "holds {}, which a {section} rule does not use: a load that replaces this key's \
                 {section} rules replaces {} too",
                joined(&outside, ", "),
                if outside.len() == 1 { "it" } else { "them" }
            ),
        );
    }
    // Stable, as the C++'s stable_sort.
    issues.sort_by_key(|issue| issue.kind);
    issues
}

/// Every issue in the map, by rule index and then in the order of LintKind:
/// lint_survey_rule for each rule, and the rules that can never contribute.
pub fn lint_survey_map(
    map: &SurveyMap,
    library: &StyleLibrary,
    colour_of: Option<&ColourLookup<'_>>,
    is_built_in_symbol: Option<&BuiltInSymbolTest<'_>>,
) -> Vec<LintIssue> {
    let mut issues = Vec::new();
    let rules = map.rules();
    let specs = field_specs();
    // The earlier rules of each key. Only a rule of the SAME key always comes
    // before another in a lookup: a longer prefix or an exact key catches
    // only some of the codes a shorter one does, so it cannot shadow it.
    let mut earlier: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, rule) in rules.iter().enumerate() {
        let mut own = lint_survey_rule(rule, i, library, colour_of, is_built_in_symbol);
        let before = earlier.entry(rule.key.clone()).or_default();

        if let Some(&duplicate) = before.iter().find(|&&j| rules[j] == *rule) {
            own.push(issue_of(
                rule,
                i,
                LintSeverity::Warning,
                LintKind::DuplicateRule,
                format!("the same as rule #{duplicate}, field for field"),
            ));
        } else if !before.is_empty() {
            // Among rules of one key the earlier wins every field, and an
            // attribute is kept once per name - so a rule whose every field
            // and attribute an earlier one already sets never contributes.
            let set_earlier = |spec: &FieldSpec| before.iter().any(|&j| (spec.sets)(&rules[j]));
            let mut contributes = false;
            for spec in &specs {
                contributes = contributes || ((spec.sets)(rule) && !set_earlier(spec));
            }
            for (_, of) in ATTRIBUTE_SCOPES {
                for attribute in of(rule) {
                    contributes = contributes
                        || !before
                            .iter()
                            .any(|&j| names_attribute(of(&rules[j]), &attribute.name));
                }
            }
            if !contributes {
                let cited: Vec<String> = before.iter().map(|j| format!("#{j}")).collect();
                own.push(issue_of(
                    rule,
                    i,
                    LintSeverity::Warning,
                    LintKind::ShadowedRule,
                    format!(
                        "earlier rules for {} ({}) already set everything this one sets, so it \
                         never contributes",
                        in_quotes(&rule.key),
                        joined(&cited, ", ")
                    ),
                ));
            }
        }
        before.push(i);
        own.sort_by_key(|issue| issue.kind);
        issues.extend(own);
    }
    issues
}

// ---- one report text for every front end ------------------------------------------
//
// Plain text, lines ending in '\n'. The CLI prints them as they are; a dialog
// shows the same words.

pub fn format_coding_report(report: &SurveyCodingReport) -> String {
    let mut out = String::new();
    // Unsigned, as the C++'s size_t: a report whose counts disagree wraps
    // there rather than stopping the program here.
    let no_rule = report
        .coded
        .wrapping_sub(report.matched)
        .wrapping_sub(report.fallback_only);
    out.push_str(&format!(
        "{} a code in {}: {} matched, {} fallback-only (only the bare * rule answers), {} with \
         no rule\n",
        counted(report.coded, "entity carries", "entities carry"),
        in_quotes(&report.property),
        report.matched,
        report.fallback_only,
        no_rule
    ));
    out.push_str(&format!(
        "{} changed\n",
        counted(report.changed, "entity", "entities")
    ));
    if !report.layers_created.is_empty() {
        out.push_str(&format!(
            "Layers created: {}\n",
            joined(&report.layers_created, ", ")
        ));
    }
    if !report.styles_created.is_empty() {
        out.push_str(&format!(
            "Styles created: {}\n",
            joined(&report.styles_created, ", ")
        ));
    }
    if !report.styles_reused.is_empty() {
        out.push_str(&format!(
            "Styles reused: {}\n",
            joined(&report.styles_reused, ", ")
        ));
    }
    if report.skipped_no_layer != 0 {
        out.push_str(&format!(
            "{} kept their layer: it does not exist and layers are not being created\n",
            counted(report.skipped_no_layer, "entity", "entities")
        ));
    }
    if report.skipped_no_style != 0 {
        out.push_str(&format!(
            "{} kept their style: no style looks as the code says and styles are not being \
             created\n",
            counted(report.skipped_no_style, "entity", "entities")
        ));
    }
    if !report.unmatched_codes.is_empty() {
        out.push_str(&format!(
            "Codes with no rule: {}\n",
            joined(&report.unmatched_codes, ", ")
        ));
    }
    if !report.fallback_only_codes.is_empty() {
        out.push_str(&format!(
            "Fallback-only codes: {}\n",
            joined(&report.fallback_only_codes, ", ")
        ));
    }
    if !report.missing_definitions.is_empty() {
        out.push_str(&format!(
            "Named but in no loaded library: {}\n",
            joined(&report.missing_definitions, ", ")
        ));
    }
    if report.deferred_attributes != 0 {
        out.push_str(&format!(
            "{} left: the value names another attribute\n",
            counted(report.deferred_attributes, "attribute", "attributes")
        ));
    }
    if !report.codes.is_empty() {
        out.push_str("By code:\n");
    }
    for row in &report.codes {
        out.push_str(&format!(
            "  {}: {}, {}{}",
            row.code,
            counted(row.entities, "entity", "entities"),
            row.kind.name(),
            if row.matched {
                ", matched"
            } else {
                ", not matched"
            }
        ));
        if row.layer_kept != 0 {
            // Not an arrow: they did not move.
            out.push_str(&format!(
                "; layer {} kept: {} does not exist and layers are not being created",
                joined(&row.layers_from, ", "),
                row.layer
            ));
        } else if row.layers_from.len() == 1 && row.layers_from[0] == row.layer {
            out.push_str(&format!("; layer {} already", row.layer));
        } else if !row.layer.is_empty() {
            out.push_str(&format!(
                "; layer {} -> {}",
                joined(&row.layers_from, ", "),
                row.layer
            ));
        }
        if row.style_outcome != SurveyStyleOutcome::None {
            out.push_str("; style ");
            if !row.style.is_empty() {
                out.push_str(&row.style);
                out.push(' ');
            }
            out.push_str(&format!("({})", row.style_outcome.name()));
        }
        if !row.attributes_set.is_empty() {
            out.push_str(&format!("; set {}", joined(&row.attributes_set, ", ")));
        }
        if !row.attributes_deferred.is_empty() {
            out.push_str(&format!(
                "; deferred {}",
                joined(&row.attributes_deferred, ", ")
            ));
        }
        out.push_str(&format!("; {} changed\n", row.changed));
    }
    out
}

pub fn format_code_explanation(explanation: &CodeExplanation) -> String {
    let mut out = format!("Code {}: ", in_quotes(&explanation.code));
    match explanation.kind {
        SurveyMatchKind::None => out.push_str("no rule matches it\n"),
        SurveyMatchKind::FallbackOnly => out.push_str(if explanation.matched {
            "only the bare * rule answers it, and that names what it is\n"
        } else {
            "only the bare * rule answers it: fallback-only, not matched\n"
        }),
        _ => out.push_str(&format!("{} match, matched\n", explanation.kind.name())),
    }
    for field in &explanation.fields {
        out.push_str(&format!(
            "  {}: {}  <- {}\n",
            field.field,
            field.value,
            rule_ref(field.rule, &field.key, field.section)
        ));
        for other in &field.overruled {
            out.push_str(&format!(
                "    overruled: {}  <- {}\n",
                other.value,
                rule_ref(other.rule, &other.key, other.section)
            ));
        }
    }
    if !explanation.linestyle.name.is_empty() {
        out.push_str(&format!(
            "  linestyle {}: ",
            in_quotes(&explanation.linestyle.name)
        ));
        if explanation.linestyle.plain {
            out.push_str("a plain line\n");
        } else if !explanation.linestyle.defined {
            out.push_str("in no loaded library, so drawn as a plain line\n");
        } else {
            out.push_str(if explanation.linestyle.vertex_mode {
                "defined, but as a symbol (at vertices)\n"
            } else {
                "defined\n"
            });
        }
    }
    if !explanation.symbol.name.is_empty() {
        out.push_str(&format!(
            "  symbol {}: ",
            in_quotes(&explanation.symbol.name)
        ));
        if explanation.symbol.defined {
            out.push_str(if explanation.symbol.vertex_mode {
                "defined, at vertices\n"
            } else {
                "defined\n"
            });
        } else if explanation.symbol.built_in {
            out.push_str("drawn by Katana itself\n");
        } else {
            out.push_str("in no loaded library\n");
        }
    }
    if !explanation.colour.name.is_empty() {
        out.push_str(&format!(
            "  colour {}: {}\n",
            in_quotes(&explanation.colour.name),
            explanation.colour.rgb.map_or_else(
                || String::from("unknown name, so the entity keeps its own"),
                Color::to_hex
            )
        ));
    }
    for attribute in &explanation.attributes {
        out.push_str(&format!(
            "  {} attribute {} = {}",
            attribute.scope, attribute.attribute.name, attribute.attribute.value
        ));
        if attribute.deferred {
            out.push_str(" (deferred: the value names another attribute)");
        }
        out.push_str(&format!(
            "  <- {}\n",
            rule_ref(attribute.rule, &attribute.key, attribute.section)
        ));
    }
    for miss in &explanation.near_misses {
        out.push_str(&format!(
            "  near miss: key {} {}\n",
            in_quotes(&miss.key),
            miss.kind.name()
        ));
    }
    out
}

/// (The C++ defaults `filter` to empty.)
pub fn format_code_table(rows: &[CodeTableRow], filter: &str) -> String {
    let mut out = String::new();
    let mut shown = 0usize;
    for row in rows {
        if !code_table_row_matches(row, filter) {
            continue;
        }
        shown += 1;
        let sections: Vec<&str> = row.sections.iter().map(|section| section.name()).collect();
        let mut parts: Vec<String> = Vec::new();
        let combined = &row.combined;
        if !combined.model.is_empty() {
            parts.push(format!("layer {}", combined.model));
        }
        if !combined.linestyle.is_empty() {
            parts.push(format!("linestyle {}", combined.linestyle));
        }
        if let Some(symbol) = &combined.symbol
            && !symbol.style.is_empty()
        {
            parts.push(format!("symbol {}", symbol.style));
        }
        if !combined.colour.is_empty() {
            parts.push(format!("colour {}", combined.colour));
        }
        if !combined.group.is_empty() {
            parts.push(format!("group {}", combined.group));
        }
        if !combined.comment.is_empty() {
            parts.push(in_quotes(&combined.comment));
        }
        out.push_str(&format!(
            "{}  [{}; {}: {}]",
            row.key,
            row.kind.name(),
            counted(row.rules.len(), "rule", "rules"),
            joined(&sections, ", ")
        ));
        if !parts.is_empty() {
            out.push_str(&format!("  {}", joined(&parts, "; ")));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "{shown} of {}",
        counted(rows.len(), "code", "codes")
    ));
    if !filter.is_empty() {
        out.push_str(&format!(" match {}", in_quotes(filter)));
    }
    out.push('\n');
    out
}

pub fn format_code_census(census: &CodeCensus) -> String {
    let mut out = format!(
        "{} a code in {}, {}\n",
        counted(census.coded, "entity carries", "entities carry"),
        in_quotes(&census.property),
        counted(census.codes.len(), "distinct code", "distinct codes")
    );
    for row in &census.codes {
        out.push_str(&format!(
            "  {}: {}, {}{}",
            row.code,
            row.entities,
            row.kind.name(),
            if row.matched {
                ", matched"
            } else {
                ", not matched"
            }
        ));
        if !row.model.is_empty() {
            out.push_str(&format!(", layer {}", row.model));
        }
        out.push('\n');
    }
    out
}

pub fn format_lint(issues: &[LintIssue], rules: usize) -> String {
    let errors = issues
        .iter()
        .filter(|issue| issue.severity == LintSeverity::Error)
        .count();
    let mut out = format!("{} checked: ", counted(rules, "rule", "rules"));
    if issues.is_empty() {
        out.push_str("no problems found\n");
        return out;
    }
    out.push_str(&format!(
        "{}, {}\n",
        counted(errors, "error", "errors"),
        counted(issues.len() - errors, "warning", "warnings")
    ));
    // The reference customisation gives hundreds, most of a few kinds, so the
    // count of each kind comes before the list it summarises.
    let mut by_kind: BTreeMap<LintKind, usize> = BTreeMap::new();
    for issue in issues {
        *by_kind.entry(issue.kind).or_default() += 1;
    }
    let kinds: Vec<String> = by_kind
        .iter()
        .map(|(kind, count)| format!("{count} {}", kind.name()))
        .collect();
    out.push_str(&format!("  by kind: {}\n", joined(&kinds, ", ")));
    for issue in issues {
        out.push_str(&format!(
            "  {}: {}, {}: {}\n",
            rule_ref(issue.rule, &issue.key, issue.section),
            issue.severity.name(),
            issue.kind.name(),
            issue.message
        ));
    }
    out
}

pub fn format_coverage(coverage: &CustomisationCoverage) -> String {
    let mut out = String::new();
    if coverage.styles == 0 {
        if coverage.definitions_loaded {
            // The definitions are loaded: what is missing is something drawn
            // with them, and an import of survey data makes the styles itself.
            out.push_str(
                "This drawing has no styles yet; the customisation's styles are made when \
                 survey data is imported or when a rule draws.\n",
            );
        } else {
            out.push_str(
                "This drawing has no styles yet; import a drawing or survey that carries \
                 styles, or make one in Format > Styles and Linetypes or with STYLE NEW, to see \
                 the customisation take effect.\n",
            );
        }
        return out;
    }
    out.push_str(&format!(
        "{} of this drawing's {} styles are drawn with a loaded definition ({} name one",
        coverage.resolved, coverage.styles, coverage.named
    ));
    if coverage.built_in != 0 {
        out.push_str(&format!(
            ", {} use a shape Katana draws itself",
            coverage.built_in
        ));
    }
    out.push_str("; the rest are plain lines)\n");
    if !coverage.unresolved.is_empty() {
        let names: Vec<String> = coverage
            .unresolved
            .iter()
            .map(|name| in_quotes(name))
            .collect();
        out.push_str(&format!(
            "  {} in no loaded library: {}\n",
            counted(coverage.unresolved.len(), "name is", "names are"),
            joined(&names, ", ")
        ));
    }
    // Its own line, because the library that defines these IS loaded: the
    // fix is a linestyle in the style, not another library.
    if !coverage.not_linestyles.is_empty() {
        let names: Vec<String> = coverage
            .not_linestyles
            .iter()
            .map(|name| in_quotes(name))
            .collect();
        out.push_str(&format!(
            "  {} draws solid: {}\n",
            counted(
                coverage.not_linestyles.len(),
                "name is loaded as an `at vertices` symbol, not a linestyle, so a linetype \
                 naming it",
                "names are loaded as `at vertices` symbols, not linestyles, so a linetype \
                 naming one"
            ),
            joined(&names, ", ")
        ));
    }
    out
}
