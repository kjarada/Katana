//! Applying a survey code to a drawing (PLAN.MD 20.3, slice 4;
//! include/katana/cad/survey_coding.hpp, src/katana_cad/survey_coding.cpp).
//!
//! This is what the three files are FOR. An entity carries a field code -
//! `WM01` - and the mapfile says a water main goes in model SURVEY SERVICES,
//! coloured "sui water potable", drawn with the "WATR Main" linestyle. This
//! turns that into the layer, the style and the properties the entity should
//! have, as ONE undoable command.
//!
//! It plans rather than acts: everything it decides comes back as a
//! Transaction, so it is undoable in one step, and a report says what it did
//! and - just as important - which codes the mapfile had no rule for. A code
//! silently left alone looks exactly like a code that was handled.
//!
//! The colour is resolved through a callback because the standard colour
//! NAMES are known to the archive layer, which cad may not see. The front ends
//! pass its standard colour table. A name the callback does not know leaves
//! the entity's colour alone rather than guessing at one.
//!
//! WHICH STYLE A CODE GETS (the lead's decision D4). What a code looks like is
//! its APPEARANCE: the linestyle (or a plain line), the symbol, the symbol's
//! size and the colour. In the reference mapfile every symbol code also says
//! linestyle "0", and "0" comes with 18 different colours, so naming a style
//! after the linestyle alone put every one of those codes on ONE style "0",
//! with whichever entity came first deciding its symbol and colour for all.
//! Instead:
//!
//! ```text
//! 1. an existing style with exactly that appearance is REUSED, whatever it is
//!    called or says in its description - so renaming a coded style and
//!    applying codes again does not make a second one. A colour name the
//!    callback does not know has no RGB, so any style with no colour of its
//!    own draws it; one described by that name is preferred, and one
//!    described by another such name of the map is left to that name's
//!    codes;
//! 2. otherwise a style is created, named after the linestyle, else the
//!    symbol, else "Plain"; a name another appearance already holds gets
//!    " (<colour name>)" and then " 2", " 3" - decided over the appearances
//!    in a fixed order, so the names follow from the rules and never from
//!    which entity came first.
//! ```

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use katana_commands::{
    CommandPtr, Transaction, create_layer, create_style, set_entity_layer, set_entity_property,
    set_entity_style,
};
use katana_core::Result;
use katana_core::text::lowered;
use katana_entity::tables::{CONTINUOUS_LINETYPE, is_built_in_symbol_name};
use katana_entity::{
    Color, Entity, EntityId, EntityType, EnumName, Layer, Model, PropertyValue, Style,
    StyleLibrary, SurveyMap, SurveyMatch, SurveyMatchKind, validate,
};

use crate::document::Document;
use crate::linework::{LineworkCodes, parse_field_code};
use crate::style_catalogue::{NameStatus, linetype_status, symbol_status};

/// Standard colour name to RGB: the C++ `std::function<std::optional<Color>
/// (std::string_view)>`. Shared, so options holding one are copied as the
/// C++'s are, and a command that styles lines later can keep them.
pub type ColourOf = Arc<dyn Fn(&str) -> Option<Color> + Send + Sync>;

#[derive(Clone)]
pub struct SurveyCodingOptions {
    /// The entity property holding the field code. EMPTY means find it: the
    /// first of code_property_candidates() that any entity actually carries,
    /// which the report names. A drawing from an archive import carries the
    /// string name in "12d.name" and a survey file's points carry "code", and
    /// asking the caller which is asking them to know how the drawing got here.
    pub property: String,
    /// Only these entities, or every entity when empty.
    pub ids: Vec<EntityId>,
    /// Standard colour name to RGB. Without one, colours are left alone.
    pub colour_of: Option<ColourOf>,
    /// Off, an entity whose rule names a model the drawing has no layer for
    /// keeps its layer - its style and attributes are still applied - and is
    /// counted in `skipped_no_layer`.
    pub create_layers: bool,
    /// Off, an entity whose appearance no existing style has keeps its style
    /// and is counted in `skipped_no_style`.
    pub create_styles: bool,
    /// Attach the attributes the mapfile gives as entity properties. A value
    /// that names another attribute ("$PipeDiameter") is skipped: resolving it
    /// needs the survey data this drawing was made from, which is not here.
    pub set_attributes: bool,
}

impl Default for SurveyCodingOptions {
    fn default() -> Self {
        Self {
            property: String::new(),
            ids: Vec::new(),
            colour_of: None,
            create_layers: true,
            create_styles: true,
            set_attributes: true,
        }
    }
}

impl fmt::Debug for SurveyCodingOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SurveyCodingOptions")
            .field("property", &self.property)
            .field("ids", &self.ids)
            .field("colour_of", &self.colour_of.is_some())
            .field("create_layers", &self.create_layers)
            .field("create_styles", &self.create_styles)
            .field("set_attributes", &self.set_attributes)
            .finish()
    }
}

/// Where a field code is looked for, in order, when none is named.
pub fn code_property_candidates() -> &'static [&'static str] {
    &[
        "code",     // a survey file's own field code
        "12d.name", // an archive string's name, which is what a coded survey puts there
        "Code",
        "CODE",
        "feature_code",
    ]
}

// ---- string names --------------------------------------------------------------------
//
// A point's STRING NAME is its code followed by its string number: "KJ01" is
// string 01 of code KJ, and the rules are keyed on that name ("KJ*", and
// "B1*" for the strings 10 to 19 of a one-letter code B). A delimited list
// writes the name whole in its code column. A field file whose records keep
// the two apart (a code column and a string column) gives a point the code
// alone, and the number comes into the drawing beside it, in this property -
// as the file's own metadata key when it has one, else from the feature that
// strings the point (survey_finish, name_survey_strings).
//
// An entity WITHOUT the property is read exactly as before it existed.

/// The property a string number is carried in, beside the code.
pub const SURVEY_STRING_PROPERTY: &str = "string";

/// What core's is_ascii_space calls a blank, for trimming text.
const ASCII_SPACE: [char; 6] = [' ', '\t', '\n', '\x0b', '\x0c', '\r'];

/// The string number `entity` carries: the text under SURVEY_STRING_PROPERTY,
/// less blanks at either end; empty when it has none or holds something that
/// is not text (a number there is some other program's field, not a string
/// number this import wrote).
pub fn survey_string_of(entity: &Entity) -> &str {
    match entity.properties.get(SURVEY_STRING_PROPERTY) {
        Some(PropertyValue::Text(text)) => text.trim_matches(ASCII_SPACE),
        _ => "",
    }
}

/// The name `code` is looked up by when its entity carries the string number
/// `string`: the string name (`code` followed by `string`) when a rule MORE
/// SPECIFIC than the bare "*" answers it, else `code` alone.
///
/// The name first, because that is what the keys are written for: code "B" in
/// string "12" is "B12", which "B1*" answers and "B" never could. The code
/// alone second, so that numbering a string never LOSES a rule the code had:
/// an exact key "PTBB" still answers PTBB in string 3, whose name "PABB3" it
/// does not match, and a code nobody wrote a rule for is reported once, as
/// itself, rather than once per string. For the same reason the code is also
/// the answer when the name's best rule is a prefix SHORTER than the code
/// ("PA*", a rule for every code that begins PA) and the code has an exact key
/// of its own: that key is the more specific rule for it, numbered or not.
/// With no `string` the answer is `code`; with no `code` there is no name and
/// the answer is empty.
pub fn survey_lookup_name(map: &SurveyMap, code: &str, string: &str) -> String {
    if code.is_empty() || string.is_empty() {
        return String::from(code);
    }
    let name = format!("{code}{string}");
    // Most specific first, so when the first rule is the bare "*" nothing
    // else answers the name: a handful of hash probes, not a combined lookup.
    let rules = map.r#match(&name);
    let Some(best) = rules.first() else {
        return String::from(code);
    };
    if best.key == "*" {
        return String::from(code);
    }
    // The best rule for the name. An exact key, or a prefix as long as the
    // code or longer ("KJ*" for KJ, "B1*" for B), was written for the
    // numbered names of this code. A SHORTER prefix ("PA*" for PTBB) is a
    // rule for a family of codes: it answers the code itself too, and less
    // well than an exact key the code has of its own - which the number must
    // not take the code away from.
    if best.key.ends_with('*') && best.key.len() - 1 < code.len() {
        let own = map.r#match(code);
        if own.first().is_some_and(|rule| rule.key == code) {
            return String::from(code);
        }
    }
    name
}

/// The code an entity carries under `property`, or None. Only TEXT is a code:
/// a number there is a measurement someone named badly, and treating "1.5" as
/// a field code would put it in whatever model the rule for `1*` names. The
/// entity's properties are asked first and its METADATA after, because an
/// archive import records a string's name - its code, in a coded survey - as
/// provenance ("12d.name"), not as a property.
pub fn survey_code_of<'a>(entity: &'a Entity, property: &str) -> Option<&'a str> {
    for place in [&entity.properties, &entity.metadata] {
        let Some(found) = place.get(property) else {
            continue;
        };
        // A property of the right name that is not a code does not fall
        // through to the metadata: it IS the entity's value for that name.
        return match found {
            PropertyValue::Text(text) if !text.is_empty() => Some(text.as_str()),
            _ => None,
        };
    }
    None
}

/// Which of code_property_candidates() any of `subject` actually carries as
/// text; the first candidate when none does, so a report can still name what
/// was looked for.
pub fn find_code_property(model: &Model, subject: &[EntityId]) -> String {
    for &candidate in code_property_candidates() {
        for &id in subject {
            if let Some(entity) = model.entities.find(id)
                && survey_code_of(entity, candidate).is_some()
            {
                return String::from(candidate);
            }
        }
    }
    String::from(code_property_candidates()[0])
}

/// The plain continuous line: "0", "1", "continuous" in any case, or no name
/// at all. Such a linestyle names no definition and draws as a plain line.
pub fn is_plain_linestyle(name: &str) -> bool {
    // Folded because library names are compared without regard to case,
    // and "Continuous" in a hand-written mapfile means what "continuous" does.
    let word = lowered(name);
    word.is_empty() || word == b"0" || word == b"1" || word == CONTINUOUS_LINETYPE.as_bytes()
}

/// What happened to the style of one code's entities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SurveyStyleOutcome {
    /// The code's rules give no appearance, so its style is left alone.
    #[default]
    None,
    /// An existing style has exactly this appearance.
    Reused,
    /// A new style was planned for it.
    Created,
    /// No style has it and create_styles is off.
    Skipped,
}

/// The C++ `toString(SurveyStyleOutcome)`.
impl EnumName for SurveyStyleOutcome {
    fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Reused => "reused",
            Self::Created => "created",
            Self::Skipped => "skipped",
        }
    }
}

/// One distinct code: what the map made of it and what that did to the
/// entities carrying it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyCodeRow {
    pub code: String,
    /// Carrying this code.
    pub entities: usize,
    pub kind: SurveyMatchKind,
    /// SurveyMatch::matched(): decision D5.
    pub matched: bool,
    /// The layers those entities were on, distinct and in name order, and the
    /// one the rule gives (empty: it gives none, so they stay where they are).
    pub layers_from: Vec<String>,
    pub layer: String,
    /// Of `entities`, those that stayed where they were because `layer` does
    /// not exist and layers are not being created (they are also counted in
    /// SurveyCodingReport::skipped_no_layer). Kept per code so that a row never
    /// reads as a move that did not happen.
    pub layer_kept: usize,
    /// The style they get; empty with SurveyStyleOutcome::None.
    pub style: String,
    pub style_outcome: SurveyStyleOutcome,
    /// Attribute NAMES, in the order the combined rule lists them.
    pub attributes_set: Vec<String>,
    /// Their value names another.
    pub attributes_deferred: Vec<String>,
    /// Of `entities`, those this alters.
    pub changed: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyCodingReport {
    /// The property the codes were actually read from - what was asked for, or
    /// what was found. Reported because "0 entities carry a code" is a
    /// different problem from "they carry it under another name".
    pub property: String,
    /// Entities carrying a code at all.
    pub coded: usize,
    /// ... whose code the map has a rule for (D5).
    pub matched: usize,
    /// ... whose code only the bare `*` answers, with no model, linestyle or
    /// symbol: in practice a code nobody wrote a rule for (audit CAD-05). Its
    /// `*` attributes are still applied - that rule does say every code gets
    /// them - but it is not counted as matched.
    pub fallback_only: usize,
    /// Entities this alters in any way - layer, style or a property - each
    /// counted once.
    pub changed: usize,
    /// Kept their layer: create_layers is off.
    pub skipped_no_layer: usize,
    /// Kept their style: create_styles is off.
    pub skipped_no_style: usize,
    /// No rule at all; distinct, in name order.
    pub unmatched_codes: Vec<String>,
    /// Distinct, in name order.
    pub fallback_only_codes: Vec<String>,
    /// In name order.
    pub layers_created: Vec<String>,
    /// In name order.
    pub styles_created: Vec<String>,
    /// Existing styles given to codes, in name order.
    pub styles_reused: Vec<String>,
    /// Linestyles and symbols the rules of the codes present name that the
    /// loaded library does not define - checked for every code, not only for
    /// one that creates a style. They are still set on the style - the name is
    /// what the rules record - and they draw as a plain line or mark until a
    /// library defines them. Plain lines and Katana's built-in symbol shapes
    /// are never listed: they draw correctly without a library.
    pub missing_definitions: Vec<String>,
    /// Attributes skipped because their value names another attribute, one
    /// per entity and attribute.
    pub deferred_attributes: usize,
    /// One row per distinct code, matched or not, in code order.
    pub codes: Vec<SurveyCodeRow>,
}

/// How much of THIS drawing the loaded customisation actually answers for.
///
/// It exists because "the linestyles are not showing" has several causes that
/// look identical from the outside - no customisation loaded, a customisation
/// that does not define what this drawing names, or a drawing whose styles are
/// plain continuous lines - and a person cannot tell them apart by looking. This
/// says which.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomisationCoverage {
    /// Whether the session holds linestyle or symbol definitions: with none
    /// loaded, a drawing with no styles is told to import ones that carry
    /// definitions; with some, that its styles are made as data is drawn.
    pub definitions_loaded: bool,
    /// Styles the drawing has.
    pub styles: usize,
    /// ... that name a linestyle or a symbol.
    pub named: usize,
    /// ... that the loaded library defines.
    pub resolved: usize,
    /// ... whose only name is a symbol Katana draws itself ("cross",
    /// "manhole") or a linetype of the drawing's own, and no library defines:
    /// drawn correctly, so neither missing nor counted as named (audit CAD-17).
    pub built_in: usize,
    /// The names the styles give whose drawing is a fallback, by
    /// linetype_status and symbol_status - the rule missing_names reports by
    /// (style_catalogue.rs) - split by its two reasons, because each asks for
    /// a different fix. So never a built-in symbol, ByLayer, a plain line or a
    /// style's own symbol as its linetype (D8). Distinct and in name order;
    /// the styles' names only - a layer's missing linetype is in
    /// missing_names, not here.
    ///
    /// Undefined: in no loaded library, so the fix is to load one that
    /// defines it. The window and CUSTOMISE print these as exactly that.
    pub unresolved: Vec<String>,
    /// NotALinestyle: a linetype naming only a `mode vertex` definition, which
    /// the viewport draws solid (D2). The library IS loaded and does define
    /// the name, as a symbol, so the fix is to pick a linestyle instead; a
    /// name here is never in `unresolved`, since symbol_status finds any
    /// library definition.
    pub not_linestyles: Vec<String>,
}

pub fn customisation_coverage(document: &Document) -> CustomisationCoverage {
    let mut coverage = CustomisationCoverage {
        definitions_loaded: !document.style_library().empty(),
        ..CustomisationCoverage::default()
    };
    let mut undefined: BTreeSet<String> = BTreeSet::new();
    let mut not_linestyles: BTreeSet<String> = BTreeSet::new();
    // for_each rather than all(): all() returns the whole style table by
    // value, and this only reads it.
    document.model().styles.for_each(|style| {
        coverage.styles += 1;
        // A style names a definition through either field, judged by the one
        // rule missing_names reads (style_catalogue.rs), so this list and the
        // style manager's Diagnostics cannot disagree. A plain line, ByLayer
        // and a linetype that is the style's own symbol (D8) name nothing; a
        // model linetype and a built-in symbol shape are drawn by Katana
        // itself, unless a loaded library defines the name, which then wins
        // (decision D2).
        let mut names = false;
        let mut found = false;
        let mut katana_drawn = false;
        let fields = [
            (
                linetype_status(document, &style.linetype, &style.symbol),
                &style.linetype,
            ),
            (symbol_status(document, &style.symbol), &style.symbol),
        ];
        for (status, name) in fields {
            match status {
                NameStatus::Plain | NameStatus::OwnSymbol => {}
                NameStatus::Library => {
                    names = true;
                    found = true;
                }
                NameStatus::Katana => katana_drawn = true,
                // Kept apart, not merged into one "missing" list: a caller
                // printing "in no loaded library" for a name the loaded library
                // defines as a symbol sends people to load what is loaded.
                NameStatus::NotALinestyle => {
                    names = true;
                    not_linestyles.insert(name.clone());
                }
                NameStatus::Undefined => {
                    names = true;
                    undefined.insert(name.clone());
                }
            }
        }
        coverage.named += usize::from(names);
        coverage.resolved += usize::from(found);
        coverage.built_in += usize::from(katana_drawn && !names);
    });
    coverage.unresolved = undefined.into_iter().collect();
    coverage.not_linestyles = not_linestyles.into_iter().collect();
    coverage
}

// ---- applying the codes ------------------------------------------------------------------

/// What the survey coding reads of a document: its model, its survey map and
/// its style library. Borrowed from a Document (CodingSource::of), or - for a
/// step of the document's own, which runs while the document lends its state
/// (document.rs, StateLink) - from the model the step is handed and the state
/// it reaches. The C++ reads all three through `const Document&`; a Rust step
/// cannot reach the Document it runs in, so the three are named instead.
#[derive(Clone, Copy)]
pub struct CodingSource<'a> {
    pub model: &'a Model,
    pub survey_map: &'a SurveyMap,
    pub library: &'a StyleLibrary,
}

impl<'a> CodingSource<'a> {
    pub fn of(document: &'a Document) -> Self {
        Self {
            model: document.model(),
            survey_map: document.survey_map(),
            library: document.style_library(),
        }
    }

    /// Document::definition_for, over the library this source reads.
    fn definition_for(&self, name: &str) -> bool {
        !name.is_empty() && self.library.find(name).is_some()
    }
}

// The code a rule is looked up by. A POINT's code is written on a field
// controller, where the string name is followed by linework controls -
// "PABB ST" starts a string of PABB - and the mapfile's key is the name
// alone: the whole field, blank included, can never match one, because no
// key holds a blank (SurveyMap::add refuses it). So a point is coded by its
// string name, as process_linework reads the same field. Any other entity's
// code is taken as it is: the lines linework makes carry the name alone.
//
// Either is then followed by the string number the entity carries beside its
// code, when it carries one (survey_lookup_name says when that name is the one
// looked up). Without one this is the code, exactly as it was.
fn code_to_look_up(map: &SurveyMap, entity: &Entity, code: &str) -> String {
    let mut name = String::from(code);
    if entity.r#type() == EntityType::Point {
        let field = parse_field_code(code, &LineworkCodes::default());
        // Blanks alone have no first token; the field is then reported as it is.
        if !field.name.is_empty() {
            name = field.name;
        }
    }
    let string = survey_string_of(entity);
    if string.is_empty() {
        name
    } else {
        survey_lookup_name(map, &name, string)
    }
}

// What a code looks like on the page (decision D4): the linestyle or a plain
// line, the symbol and its size, and the colour. Two codes with the same
// appearance share a style; two with different ones never do.
#[derive(Clone, Debug, Default)]
struct Appearance {
    /// Empty: a plain line.
    linestyle: String,
    symbol: String,
    symbol_size: f64,
    /// What colour_of made of the rule's colour name, or nothing.
    colour: Option<Color>,
    /// The name, when there is one. A name the callback does not know is the
    /// only thing that tells "sui water potable" from "sui electricity", so
    /// it keeps two such codes apart when styles are CREATED for them - each
    /// gets its own, carrying its name. It does not change how either DRAWS:
    /// with no RGB both draw with no colour of their own, which is why reuse
    /// (existing_style_for) does not ask for the name. A known name is only a
    /// label for its RGB.
    colour_name: String,
}

impl Appearance {
    /// The colour's identity: its RGB when that is known (so "White" and
    /// "white" are one colour), otherwise its name, otherwise nothing.
    fn colour_identity(&self) -> String {
        match self.colour {
            Some(colour) => format!("rgb {}", colour.to_hex()),
            None if self.colour_name.is_empty() => String::new(),
            None => format!("name {}", self.colour_name),
        }
    }

    /// A fixed order over appearances, which is what makes the names given to
    /// new styles follow from the rules rather than from the entities.
    fn identity(&self) -> AppearanceIdentity {
        AppearanceIdentity {
            linestyle: self.linestyle.clone(),
            symbol: self.symbol.clone(),
            symbol_size: self.symbol_size,
            colour: self.colour_identity(),
        }
    }
}

/// The C++ `std::tuple<std::string, std::string, double, std::string>` as a
/// map key: compared member by member, the size with `<` as the tuple
/// compares it (so -0 and 0 are one size).
#[derive(Clone, Debug)]
struct AppearanceIdentity {
    linestyle: String,
    symbol: String,
    symbol_size: f64,
    colour: String,
}

impl Ord for AppearanceIdentity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.linestyle
            .cmp(&other.linestyle)
            .then_with(|| self.symbol.cmp(&other.symbol))
            .then_with(|| {
                self.symbol_size
                    .partial_cmp(&other.symbol_size)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| self.colour.cmp(&other.colour))
    }
}

impl PartialOrd for AppearanceIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for AppearanceIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for AppearanceIdentity {}

// Whether an existing style draws exactly this appearance. Only what is drawn
// is compared - a weight or a description someone changed on a coded style
// does not stop it being reused, and neither does a new name, which is the
// point.
fn draws_as(style: &Style, appearance: &Appearance) -> bool {
    let line_matches = if appearance.linestyle.is_empty() {
        is_plain_linestyle(&style.linetype)
    } else {
        style.linetype == appearance.linestyle
    };
    if !line_matches
        || style.symbol != appearance.symbol
        || style.symbol_size != appearance.symbol_size
    {
        return false;
    }
    // A colour name with no RGB draws as no colour at all, whatever the name:
    // an archive import's "WATR Main" (no colour, the import's description)
    // draws exactly as the style coding would make for "sui water potable".
    // Asking for the name here too made coding duplicate every such style
    // whose description was not that name.
    match appearance.colour {
        Some(colour) => style.color == Some(colour),
        None => style.color.is_none(),
    }
}

// The colour names in the map that colour_of has no RGB for. A style made by
// coding for one of these has no colour and carries the name as its
// description - the only mark of which colour it was made for.
fn unknown_colour_names(map: &SurveyMap, options: &SurveyCodingOptions) -> BTreeSet<String> {
    let mut asked: BTreeSet<&str> = BTreeSet::new();
    let mut unknown = BTreeSet::new();
    for rule in map.rules() {
        if rule.colour.is_empty() || !asked.insert(rule.colour.as_str()) {
            continue;
        }
        let known = options
            .colour_of
            .as_ref()
            .is_some_and(|colour_of| colour_of(&rule.colour).is_some());
        if !known {
            unknown.insert(rule.colour.clone());
        }
    }
    unknown
}

// The appearance a combined rule gives, or nothing when it says nothing
// about how its code is drawn - a code only attributes are attached to keeps
// its style.
fn appearance_of(found: &SurveyMatch, options: &SurveyCodingOptions) -> Option<Appearance> {
    let rule = &found.resolved;
    let symbol = rule
        .symbol
        .as_ref()
        .filter(|symbol| !symbol.style.is_empty());
    if rule.linestyle.is_empty() && symbol.is_none() && rule.colour.is_empty() {
        return None;
    }
    let mut appearance = Appearance {
        linestyle: if is_plain_linestyle(&rule.linestyle) {
            String::new()
        } else {
            rule.linestyle.clone()
        },
        colour_name: rule.colour.clone(),
        ..Appearance::default()
    };
    if let Some(symbol) = symbol {
        appearance.symbol.clone_from(&symbol.style);
        appearance.symbol_size = symbol.size;
    }
    if let Some(colour_of) = &options.colour_of
        && !rule.colour.is_empty()
    {
        appearance.colour = colour_of(&rule.colour);
    }
    Some(appearance)
}

// The style an existing drawing already has for an appearance. Of those that
// draw it, one whose description is the appearance's colour name comes first:
// that is the style an earlier pass made for it, so applying codes again
// keeps each code on its own style even when several draw alike. Otherwise the
// first in name order, so the choice never depends on how the table was
// filled - except that, for a colour with no RGB, a style described by ANOTHER
// unknown colour name of the map is that colour's style, not this one's. The
// map decides that set, not the entities being coded, so a code added later
// gets the style a fresh drawing would give it rather than borrowing its
// neighbour's.
fn existing_style_for<'a>(
    model: &'a Model,
    appearance: &Appearance,
    unknown_colours: &BTreeSet<String>,
) -> Option<&'a Style> {
    let mut own: Option<&'a Style> = None;
    let mut other: Option<&'a Style> = None;
    // In name order, as the table visits them.
    for name in model.styles.names() {
        let Some(style) = model.styles.find(&name) else {
            continue;
        };
        if !draws_as(style, appearance) {
            continue;
        }
        if style.description == appearance.colour_name {
            own.get_or_insert(style);
        } else if appearance.colour.is_some() || !unknown_colours.contains(&style.description) {
            other.get_or_insert(style);
        }
    }
    own.or(other)
}

// The name a new style for this appearance takes: the linestyle, else the
// symbol, else "Plain"; then with the colour; then numbered. `taken` says
// whether a name is already held - by an existing style (which, having
// failed draws_as, draws something else) or by a style planned earlier in the
// fixed order.
fn name_for(appearance: &Appearance, taken: impl Fn(&str) -> bool) -> String {
    let base = if !appearance.linestyle.is_empty() {
        appearance.linestyle.clone()
    } else if !appearance.symbol.is_empty() {
        appearance.symbol.clone()
    } else {
        String::from("Plain")
    };
    if !taken(&base) {
        return base;
    }
    let mut stem = base;
    if !appearance.colour_name.is_empty() {
        stem = format!("{stem} ({})", appearance.colour_name);
        if !taken(&stem) {
            return stem;
        }
    }
    let mut n: usize = 2;
    loop {
        let numbered = format!("{stem} {n}");
        if !taken(&numbered) {
            return numbered;
        }
        n += 1;
    }
}

// Whether the library lacks a name a rule gives. Never true of a plain line
// or of one of the shapes Katana draws itself: both draw correctly without a
// library, so neither is "missing".
fn missing_from_library(source: &CodingSource<'_>, name: &str, symbol: bool) -> bool {
    if is_plain_linestyle(name) {
        return false;
    }
    if symbol && is_built_in_symbol_name(name) {
        return false;
    }
    !source.definition_for(name)
}

fn property_holds(entity: &Entity, name: &str, value: &str) -> bool {
    matches!(entity.properties.get(name), Some(PropertyValue::Text(text)) if text == value)
}

/// None with no error when there is nothing to do - no entity carries a code
/// the map has a rule for, or every coded entity already has what its rule
/// gives - so a caller can tell "nothing to do" from "something went wrong".
/// Fails only on a rule that cannot be turned into a valid layer or style.
///
/// A POINT is coded by the string name of its field code, its linework
/// controls left out (parse_field_code in linework.rs): "PABB ST" is coded as
/// PABB, and reported under PABB. Any other entity's code is looked up whole.
/// Either is followed by the entity's string number when it carries one
/// (SURVEY_STRING_PROPERTY, survey_lookup_name): a point coded "B" in string
/// "12" is coded, and reported, as B12.
///
/// `report`, when given, is replaced by what was found, unless the planning
/// fails (the C++ `SurveyCodingReport*`, null by default).
pub fn apply_survey_codes(
    document: &Document,
    options: &SurveyCodingOptions,
    report: Option<&mut SurveyCodingReport>,
) -> Result<Option<CommandPtr>> {
    apply_survey_codes_to(CodingSource::of(document), options, report)
}

struct CodePlan<'p> {
    code: &'p str,
    ids: &'p [EntityId],
    found: SurveyMatch,
    appearance: Option<Appearance>,
}

struct StyleChoice {
    appearance: Appearance,
    name: String,
    outcome: SurveyStyleOutcome,
}

/// apply_survey_codes over the parts it reads: what a step of the document's
/// own command calls while the document's state is lent to it (linework's styling of
/// the lines it creates, the survey import's finish).
pub fn apply_survey_codes_to(
    source: CodingSource<'_>,
    options: &SurveyCodingOptions,
    report: Option<&mut SurveyCodingReport>,
) -> Result<Option<CommandPtr>> {
    let model = source.model;
    let map = source.survey_map;
    let mut tally = SurveyCodingReport::default();

    let subject = if options.ids.is_empty() {
        model.entities.ids()
    } else {
        options.ids.clone()
    };
    let property = if options.property.is_empty() {
        find_code_property(model, &subject)
    } else {
        options.property.clone()
    };
    tally.property.clone_from(&property);

    // The entities by code. Everything below is decided once per DISTINCT
    // code rather than once per entity: a survey of 30,000 points is a few
    // hundred codes, and each lookup combines every rule that matches.
    let mut entities_by_code: BTreeMap<String, Vec<EntityId>> = BTreeMap::new();
    for &id in &subject {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        if let Some(code) = survey_code_of(entity, &property) {
            tally.coded += 1;
            entities_by_code
                .entry(code_to_look_up(map, entity, code))
                .or_default()
                .push(id);
        }
    }

    let mut plans: Vec<CodePlan<'_>> = Vec::with_capacity(entities_by_code.len());
    let mut missing: BTreeSet<String> = BTreeSet::new();
    for (code, ids) in &entities_by_code {
        let mut plan = CodePlan {
            code,
            ids,
            found: map.lookup(code),
            appearance: None,
        };
        if !plan.found.empty() {
            // Checked for every code, not only when it creates a style: a
            // code given an existing style names the same definitions.
            let rule = &plan.found.resolved;
            if !rule.linestyle.is_empty() && missing_from_library(&source, &rule.linestyle, false) {
                missing.insert(rule.linestyle.clone());
            }
            if let Some(symbol) = &rule.symbol
                && !symbol.style.is_empty()
                && missing_from_library(&source, &symbol.style, true)
            {
                missing.insert(symbol.style.clone());
            }
            plan.appearance = appearance_of(&plan.found, options);
        }
        plans.push(plan);
    }

    // Each distinct appearance gets ONE style, found or made, in a fixed
    // order. Codes that share an appearance but spell its known colour
    // differently ("White", "white") share the style, labelled by the first
    // spelling in name order.
    let mut choices: BTreeMap<AppearanceIdentity, StyleChoice> = BTreeMap::new();
    for plan in &plans {
        let Some(appearance) = &plan.appearance else {
            continue;
        };
        let at = choices
            .entry(appearance.identity())
            .or_insert_with(|| StyleChoice {
                appearance: appearance.clone(),
                name: String::new(),
                outcome: SurveyStyleOutcome::None,
            });
        if appearance.colour_name < at.appearance.colour_name {
            at.appearance = appearance.clone();
        }
    }
    let mut planned: BTreeSet<String> = BTreeSet::new();
    let mut styles_needed: BTreeMap<String, Style> = BTreeMap::new();
    let mut reused: BTreeSet<String> = BTreeSet::new();
    let unknown_colours = if choices.is_empty() {
        BTreeSet::new()
    } else {
        unknown_colour_names(map, options)
    };
    for choice in choices.values_mut() {
        if let Some(existing) = existing_style_for(model, &choice.appearance, &unknown_colours) {
            choice.name.clone_from(&existing.name);
            choice.outcome = SurveyStyleOutcome::Reused;
            continue;
        }
        if !options.create_styles {
            choice.outcome = SurveyStyleOutcome::Skipped;
            continue;
        }
        choice.name = name_for(&choice.appearance, |name| {
            model.styles.contains(name) || planned.contains(name)
        });
        choice.outcome = SurveyStyleOutcome::Created;
        planned.insert(choice.name.clone());

        // The linestyle and the symbol are NAMES resolved when they are
        // drawn, against the loaded library. Recording a name the library
        // does not define is deliberate: it is what the rules say this is, and
        // it draws plainly until a library defines it.
        let style = Style {
            name: choice.name.clone(),
            linetype: if choice.appearance.linestyle.is_empty() {
                String::from(CONTINUOUS_LINETYPE)
            } else {
                choice.appearance.linestyle.clone()
            },
            symbol: choice.appearance.symbol.clone(),
            symbol_size: choice.appearance.symbol_size,
            // The colour name is kept whatever Katana makes of it, so a colour
            // this build has no RGB for is still known by name - the same
            // thing an archive import does.
            description: choice.appearance.colour_name.clone(),
            color: choice.appearance.colour,
            ..Style::default()
        };
        validate(&style)?;
        styles_needed.insert(choice.name.clone(), style);
    }

    // Grouped, so that one command covers every entity that resolved the same
    // way: a command per entity would make an undo stack nobody can use.
    let mut by_layer: BTreeMap<String, Vec<EntityId>> = BTreeMap::new();
    let mut by_style: BTreeMap<String, Vec<EntityId>> = BTreeMap::new();
    let mut by_property: BTreeMap<(String, String), Vec<EntityId>> = BTreeMap::new();
    let mut layers_needed: BTreeSet<String> = BTreeSet::new();
    let mut unmatched: BTreeSet<String> = BTreeSet::new();
    let mut fallback_only: BTreeSet<String> = BTreeSet::new();
    let mut changed: BTreeSet<EntityId> = BTreeSet::new();

    for plan in &plans {
        let found = &plan.found;
        let mut row = SurveyCodeRow {
            code: String::from(plan.code),
            entities: plan.ids.len(),
            kind: found.kind,
            matched: found.matched(),
            layer: found.resolved.model.clone(),
            ..SurveyCodeRow::default()
        };

        let mut layers_from: BTreeSet<String> = BTreeSet::new();
        for &id in plan.ids {
            if let Some(entity) = model.entities.find(id) {
                layers_from.insert(entity.layer.clone());
            }
        }
        row.layers_from = layers_from.into_iter().collect();

        if found.empty() {
            unmatched.insert(row.code.clone());
            tally.codes.push(row);
            continue;
        }
        if row.matched {
            tally.matched += row.entities;
        } else {
            tally.fallback_only += row.entities;
            fallback_only.insert(row.code.clone());
        }

        let choice = plan
            .appearance
            .as_ref()
            .and_then(|appearance| choices.get(&appearance.identity()));
        if let Some(choice) = choice {
            row.style.clone_from(&choice.name);
            row.style_outcome = choice.outcome;
            if choice.outcome == SurveyStyleOutcome::Reused {
                reused.insert(choice.name.clone());
            }
        }
        let layer_exists = row.layer.is_empty() || model.layers.contains(&row.layer);
        for attribute in &found.resolved.attributes {
            // "$PipeDiameter" names another attribute rather than being a
            // value. Resolving it needs the survey data this drawing came
            // from, which is not here, so it is counted and left.
            if attribute.value.starts_with('$') {
                row.attributes_deferred.push(attribute.name.clone());
            } else {
                row.attributes_set.push(attribute.name.clone());
            }
        }
        if !options.set_attributes {
            row.attributes_set.clear();
            row.attributes_deferred.clear();
        }

        for &id in plan.ids {
            let Some(entity) = model.entities.find(id) else {
                continue;
            };
            let mut alters = false;
            // Each part is decided on its own: a layer that cannot be made
            // does not stop the style and the attributes (audit CAD-18).
            if !row.layer.is_empty() && entity.layer != row.layer {
                if !layer_exists && !options.create_layers {
                    tally.skipped_no_layer += 1;
                    row.layer_kept += 1;
                } else {
                    if !layer_exists {
                        layers_needed.insert(row.layer.clone());
                    }
                    by_layer.entry(row.layer.clone()).or_default().push(id);
                    alters = true;
                }
            }
            if let Some(choice) = choice {
                if choice.outcome == SurveyStyleOutcome::Skipped {
                    tally.skipped_no_style += 1;
                } else if entity.style != choice.name {
                    by_style.entry(choice.name.clone()).or_default().push(id);
                    alters = true;
                }
            }
            if options.set_attributes {
                for attribute in &found.resolved.attributes {
                    if attribute.value.starts_with('$') {
                        tally.deferred_attributes += 1;
                        continue;
                    }
                    if !property_holds(entity, &attribute.name, &attribute.value) {
                        by_property
                            .entry((attribute.name.clone(), attribute.value.clone()))
                            .or_default()
                            .push(id);
                        alters = true;
                    }
                }
            }
            if alters {
                row.changed += 1;
                changed.insert(id);
            }
        }
        tally.codes.push(row);
    }

    tally.changed = changed.len();
    tally.unmatched_codes = unmatched.into_iter().collect();
    tally.fallback_only_codes = fallback_only.into_iter().collect();
    tally.missing_definitions = missing.into_iter().collect();
    tally.layers_created = layers_needed.into_iter().collect();
    tally.styles_reused = reused.into_iter().collect();
    tally.styles_created = styles_needed.keys().cloned().collect();

    let mut transaction = Transaction::new("APPLY SURVEY CODES");
    for name in &tally.layers_created {
        transaction.add(create_layer(Layer {
            name: name.clone(),
            ..Layer::default()
        }));
    }
    for style in styles_needed.into_values() {
        transaction.add(create_style(style));
    }
    for (layer, ids) in by_layer {
        transaction.add(set_entity_layer(ids, layer));
    }
    for (style, ids) in by_style {
        transaction.add(set_entity_style(ids, style));
    }
    for ((name, value), ids) in by_property {
        transaction.add(set_entity_property(ids, name, PropertyValue::Text(value)));
    }

    if let Some(report) = report {
        *report = tally;
    }
    if transaction.size() == 0 {
        return Ok(None); // nothing to do, and no error
    }
    Ok(Some(Box::new(transaction)))
}
