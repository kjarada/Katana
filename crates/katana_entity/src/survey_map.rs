//! What a surveyor's field code becomes (PLAN.MD 20.3, slice 3).
//!
//! A code is not a label, it is an instruction. `WM01` is a water main: put it
//! in model SURVEY SERVICES, colour it "sui water potable", join it into a
//! line rather than leaving it a point, draw it with the "WATR Main"
//! linestyle. `AC01` is a bollard and gets a symbol instead. A survey code
//! file (.mapfile) is the table that says so, and this is that table.
//!
//! A rule is keyed by a code pattern which is either EXACT ("PABB") or a
//! PREFIX ("WM*"). The two survey code files of the reference customisation
//! hold 1,624 rules over 632 distinct keys between them, and not one uses a
//! wildcard anywhere but at the end - so a key of any other shape is refused
//! rather than matched approximately.
//!
//! A survey code file is written in SECTIONS, and a rule carries only what its
//! section is about: where the code goes, or the symbol it gets, or the
//! attributes it carries, or how it is drawn as a pipe. So several rules can
//! match one code - 213 keys in the detail file do - and a code's treatment is
//! the UNION of them. `lookup` is what combines them; see
//! docs/survey_coding.md for how ties are settled and on what evidence.

use std::collections::{BTreeSet, HashMap};

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_core::text::is_ascii_space;

use crate::entity::EnumName;
use crate::entity_geometry::Validate;
use crate::layer_path::validate_layer_path;
use crate::validation::validate_name;

/// Whether a code's points are joined into a string or left as points. The
/// files write both cases ("Line" and "line"), so it is parsed without regard
/// to case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurveyBreakline {
    Line,
    Point,
}

impl EnumName for SurveyBreakline {
    fn name(self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::Point => "Point",
        }
    }
}

pub fn parse_survey_breakline(text: &str) -> Option<SurveyBreakline> {
    let word = text.to_ascii_lowercase();
    match word.as_str() {
        "line" => Some(SurveyBreakline::Line),
        "point" => Some(SurveyBreakline::Point),
        _ => None,
    }
}

/// One attribute a rule attaches. The value is kept VERBATIM because it may
/// name another attribute rather than be a value: the survey code files use
/// "$PipeDiameter" and "$CulvBankHeight", and resolving those needs the entity
/// the rule is being applied to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyAttribute {
    pub r#type: String, // "text" or "integer", the file's own word
    pub name: String,
    pub value: String,
}

/// `<symbol_data>`: the symbol a code gets, by name in the symbol library.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveySymbol {
    pub style: String,  // a name in the loaded StyleLibrary
    pub colour: String, // a standard colour name; empty means the string's own
    pub size: f64,      // 0 means the library definition's own size
    pub rotation: f64,
    pub offset: f64, // across the string
    pub raise: f64,  // up
}

/// `<textstyle_data>`: how a code's text is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct SurveyTextStyle {
    pub textstyle: String,
    pub colour: String,
    pub r#type: String, // "paper" or "world", as written
    pub size: f64,
    pub justify_x: String,
    pub justify_y: String,
    pub offset: f64,
    pub raise: f64,
    pub angle: f64,
    pub slant: f64,
    pub width_factor: f64, // `x_factor`
    pub underline: bool,
    pub strikeout: bool,
    pub italic: bool,
    pub weight: String, // "0" or "Normal" in these files: a pen, not a number
}

impl Default for SurveyTextStyle {
    fn default() -> Self {
        Self {
            textstyle: String::new(),
            colour: String::new(),
            r#type: String::new(),
            size: 0.0,
            justify_x: String::new(),
            justify_y: String::new(),
            offset: 0.0,
            raise: 0.0,
            angle: 0.0,
            slant: 0.0,
            width_factor: 1.0,
            underline: false,
            strikeout: false,
            italic: false,
            weight: String::new(),
        }
    }
}

/// How a coded string is drawn as a pipe or a culvert, from one of the three
/// `*pipe_data` sections. `size1` and `size2` are kept as TEXT because they
/// are "0.375" in one rule and "$PipeDiameter" in another, and which it is
/// cannot be decided without the entity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyPipe {
    pub justify: String, // "Obvert", "Invert", "Centre"
    pub shape: String,   // "diameter", "culvert"
    pub size1: String,
    pub size2: String,
    pub active: bool,
}

/// Which section of the survey code file a rule came from. Kept because it is
/// the section, not the fields, that says what a rule is ABOUT -
/// `<map_attributes>` means attributes on the string inside
/// `string_attribute_data` and attributes on each vertex inside
/// `vertex_attribute_data`. A section added here needs its word in
/// SURVEY_SECTION_WORDS below.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SurveySection {
    /// map_data: model, colour, breakline, linestyle, weight
    #[default]
    Map,
    /// vertex_symbol_data and vertex_symbol_data_v9
    VertexSymbol,
    /// vertex_textstyle_data
    VertexTextStyle,
    /// pipe_data
    Pipe,
    /// vertex_pipe_data
    VertexPipe,
    /// segment_pipe_data
    SegmentPipe,
    /// string_attribute_data
    StringAttribute,
    /// vertex_attribute_data
    VertexAttribute,
    /// tinable_data
    Tinable,
}

/// One section and the word for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurveySectionWord {
    pub section: SurveySection,
    pub word: &'static str,
}

/// The word for each section, in the enumeration's order: what a reply and a
/// label call it ("rule #2 KT* (feature)", the code manager's Section list)
/// and what a Katana customisation file holds as a rule's "sets". ONE table,
/// so that what a person reads in the window is what they write in the file.
/// A word changed or added here is a new version of that format
/// (docs/customisation.md).
///
/// The element names in the comments above (`map_data`) are a survey code
/// file's, and are not these: they are kept with that file's reader and
/// writer, which are the only code to use them.
pub const SURVEY_SECTION_WORDS: [SurveySectionWord; 9] = [
    SurveySectionWord {
        section: SurveySection::Map,
        word: "feature",
    },
    SurveySectionWord {
        section: SurveySection::VertexSymbol,
        word: "symbol",
    },
    SurveySectionWord {
        section: SurveySection::VertexTextStyle,
        word: "text",
    },
    SurveySectionWord {
        section: SurveySection::Pipe,
        word: "pipe",
    },
    SurveySectionWord {
        section: SurveySection::VertexPipe,
        word: "vertexPipe",
    },
    SurveySectionWord {
        section: SurveySection::SegmentPipe,
        word: "segmentPipe",
    },
    SurveySectionWord {
        section: SurveySection::StringAttribute,
        word: "attributes",
    },
    SurveySectionWord {
        section: SurveySection::VertexAttribute,
        word: "vertexAttributes",
    },
    SurveySectionWord {
        section: SurveySection::Tinable,
        word: "surface",
    },
];

/// The word SURVEY_SECTION_WORDS gives the section.
impl EnumName for SurveySection {
    fn name(self) -> &'static str {
        // A value outside the enumeration, which the C++ can hold and Rust
        // cannot, gave the first section's word.
        SURVEY_SECTION_WORDS
            .iter()
            .find(|entry| entry.section == self)
            .map_or(SURVEY_SECTION_WORDS[0].word, |entry| entry.word)
    }
}

/// Everything a rule can say. A rule sets only what its section is about; an
/// unset field is one it says nothing about, which is what lets rules combine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyRule {
    pub key: String, // as written: "WM*" or "PABB"
    pub section: SurveySection,

    pub model: String, // the rule's model, which becomes a Katana layer
    pub colour: String,
    pub linestyle: String,
    /// "0" and "Normal" both appear and both mean the default pen, so this is
    /// kept as text rather than forced into a number it is not.
    pub weight: String,
    pub group: String,
    pub comment: String,
    pub breakline: Option<SurveyBreakline>,
    pub tinable: Option<bool>,
    pub hide: Option<bool>,

    pub symbol: Option<SurveySymbol>,
    pub text_style: Option<SurveyTextStyle>,
    pub pipe: Option<SurveyPipe>,         // the string as a pipe
    pub vertex_pipe: Option<SurveyPipe>,  // each vertex
    pub segment_pipe: Option<SurveyPipe>, // each segment

    pub attributes: Vec<SurveyAttribute>,        // on the string
    pub vertex_attributes: Vec<SurveyAttribute>, // on each vertex
    pub segment_attributes: Vec<SurveyAttribute>, // on each segment
}

/// How a code met the map, by the most specific key that matched it. Kept
/// apart from "matched" because the reference survey code files carry `*`
/// rules - 16 in each pipe section alone - that every code meets, so "some
/// rule matched" is true of a typo too (audit CAD-05).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SurveyMatchKind {
    /// a key equal to the code
    Exact,
    /// a key such as "WM*", but not the bare `*`
    Prefix,
    /// nothing but the bare `*`
    FallbackOnly,
    /// no rule at all
    #[default]
    None,
}

impl EnumName for SurveyMatchKind {
    fn name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Prefix => "prefix",
            Self::FallbackOnly => "fallback only",
            Self::None => "none",
        }
    }
}

/// What a code resolves to: the rules that matched, combined.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyMatch {
    pub resolved: SurveyRule, // `key` is the most specific key that matched
    /// Every key that contributed, most specific first.
    pub keys: Vec<String>,
    pub kind: SurveyMatchKind,
}

impl SurveyMatch {
    pub fn empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Whether the map has something to say about THIS code rather than about
    /// every code: a rule more specific than `*` matched, or the combination
    /// names a model, a linestyle or a symbol (the lead's decision D5). A code
    /// only `*` answers, with nothing but its attributes, is fallback-only -
    /// in practice a code nobody wrote a rule for, most often a typo.
    pub fn matched(&self) -> bool {
        self.kind == SurveyMatchKind::Exact
            || self.kind == SurveyMatchKind::Prefix
            || !self.resolved.model.is_empty()
            || !self.resolved.linestyle.is_empty()
            || self
                .resolved
                .symbol
                .as_ref()
                .is_some_and(|symbol| !symbol.style.is_empty())
    }
}

fn is_prefix_key(key: &str) -> bool {
    key.ends_with('*')
}

/// The kind of match a key makes, by its shape alone.
fn kind_of_key(key: &str) -> SurveyMatchKind {
    if !is_prefix_key(key) {
        return SurveyMatchKind::Exact;
    }
    if key.len() == 1 {
        SurveyMatchKind::FallbackOnly
    } else {
        SurveyMatchKind::Prefix
    }
}

fn out_of_range(index: usize, size: usize) -> Error {
    make_error(
        ErrorCode::NotFound,
        "no survey rule at that index",
        format!("{index} of {size}"),
    )
}

/// A field is taken from the first rule that says anything about it.
fn take_text_if_unset(into: &mut String, from: &str) {
    if into.is_empty() && !from.is_empty() {
        *into = String::from(from);
    }
}

fn take_if_unset<T: Clone>(into: &mut Option<T>, from: &Option<T>) {
    if into.is_none() && from.is_some() {
        into.clone_from(from);
    }
}

/// Attributes ACCUMULATE rather than override: a general rule saying every
/// pipe has a DepthLocation and a specific one giving a diameter are both
/// meant, which is the whole reason the survey code files are written with a
/// `*` rule beside the coded ones.
fn append_attributes(into: &mut Vec<SurveyAttribute>, from: &[SurveyAttribute]) {
    for attribute in from {
        if !into.iter().any(|kept| kept.name == attribute.name) {
            into.push(attribute.clone());
        }
    }
}

/// Besides numbers being finite: a key has no surrounding whitespace (a field
/// code never has, so such a key can match nothing a surveyor types), a
/// model, when there is one, is a valid layer path (it becomes one when the
/// code is applied, and failing there would fail the whole application over
/// one rule), and the size of a symbol or of a text is not negative (0 is
/// "the definition's own" and "not said"). (The C++ also refused text that is
/// not UTF-8; a String cannot be.)
impl Validate for SurveyRule {
    fn validate(&self) -> Status {
        let rule = self;
        validate_name(&rule.key, "survey code")?;
        // A wildcard anywhere but at the end is a key this cannot match, and
        // matching it approximately would put the code in the wrong model.
        if let Some(star) = rule.key.find('*')
            && star + 1 != rule.key.len()
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a survey code key may only use `*` as its last character",
                rule.key.as_str(),
            ));
        }
        if rule.key.contains('?') {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "`?` is not a wildcard a survey code key may use",
                rule.key.as_str(),
            ));
        }
        // Matching is byte for byte, so " WM*" could only ever match a code
        // that itself starts with a blank - which no surveyor types. Refused
        // here rather than trimmed, because silently changing a key changes
        // which codes it catches. (std::isspace in the "C" locale.)
        let bytes = rule.key.as_bytes();
        if bytes.first().copied().is_some_and(is_ascii_space)
            || bytes.last().copied().is_some_and(is_ascii_space)
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a survey code key may not begin or end with whitespace",
                format!("\"{}\"", rule.key),
            ));
        }
        if !rule.model.is_empty()
            && let Err(error) = validate_layer_path(&rule.model)
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the layer of rule \"{}\" is not a valid layer path: {}",
                    rule.key, error.message
                ),
                rule.model.as_str(),
            ));
        }
        if let Some(symbol) = &rule.symbol {
            if !(symbol.size.is_finite()
                && symbol.rotation.is_finite()
                && symbol.offset.is_finite()
                && symbol.raise.is_finite())
            {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a symbol's numbers are not finite",
                    rule.key.as_str(),
                ));
            }
            // Said apart from the above: -1 is finite, and a refusal that
            // called it "not finite" sent its reader looking for the wrong
            // mistake. 0 is the definition's own size; below it there is no
            // size to draw at.
            if symbol.size < 0.0 {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a symbol's size cannot be negative",
                    rule.key.as_str(),
                ));
            }
        }
        if let Some(text) = &rule.text_style {
            if !(text.size.is_finite()
                && text.offset.is_finite()
                && text.raise.is_finite()
                && text.angle.is_finite()
                && text.slant.is_finite()
                && text.width_factor.is_finite())
            {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a text style's numbers are not finite",
                    rule.key.as_str(),
                ));
            }
            if text.size < 0.0 {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a text style's size cannot be negative",
                    rule.key.as_str(),
                ));
            }
        }
        for list in [
            &rule.attributes,
            &rule.vertex_attributes,
            &rule.segment_attributes,
        ] {
            if list.iter().any(|attribute| attribute.name.is_empty()) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "an attribute of this rule has no name",
                    rule.key.as_str(),
                ));
            }
        }
        Ok(())
    }
}

/// Rule indices by key: "PABB" to the rules keyed exactly so, or "WM" to the
/// rules keyed "WM*" ("" for the bare `*`).
type KeyIndex = HashMap<String, Vec<usize>>;

/// A rule's identity is its INDEX, and its index is also its precedence:
/// among rules of equal specificity the earlier wins. So the editing calls
/// below are by index, and every one of them - add included - INVALIDATES
/// what match and lookup handed out before it: an index from match_indices
/// may now name another rule. A caller holding one across an edit must ask
/// again. (A cache keys on cad::Document::surveyMapGeneration, never on a
/// rule's address.)
#[derive(Clone, Debug, Default)]
pub struct SurveyMap {
    rules: Vec<SurveyRule>,
    // Rule indices by key, so that a lookup is a handful of hash probes - one
    // for the exact key and one per prefix length of the code - instead of a
    // test of every rule: applying 1,624 rules to 20,000 points was 32 million
    // key comparisons. Derived from rules and rebuilt by every edit but add;
    // each list is ascending, which is read order, which is what breaks ties.
    exact: KeyIndex,
    prefixes: KeyIndex,
}

/// Equal when the rules are, in the same order: order is precedence, so two
/// maps holding the same rules in another order can resolve a code
/// differently and are not equal.
impl PartialEq for SurveyMap {
    fn eq(&self, other: &Self) -> bool {
        self.rules == other.rules
    }
}

impl SurveyMap {
    pub fn new() -> Self {
        Self::default()
    }

    fn index_rule(&mut self, at: usize) {
        let key = &self.rules[at].key;
        if is_prefix_key(key) {
            self.prefixes
                .entry(String::from(&key[..key.len() - 1]))
                .or_default()
                .push(at);
        } else {
            self.exact.entry(key.clone()).or_default().push(at);
        }
    }

    fn reindex(&mut self) {
        self.exact.clear();
        self.prefixes.clear();
        for at in 0..self.rules.len() {
            self.index_rule(at);
        }
    }

    /// Rules are kept in the order they were read. Several rules with one key
    /// is the normal case - that is how a survey code file is written, one
    /// section per aspect - and where two do set the same field the EARLIER
    /// wins, so that loading a second file adds to the first rather than
    /// overriding it.
    pub fn add(&mut self, rule: SurveyRule) -> Status {
        rule.validate()?;
        self.rules.push(rule);
        // Appending moves no other rule, so only the new one needs indexing -
        // which keeps loading a survey code file linear.
        self.index_rule(self.rules.len() - 1);
        Ok(())
    }

    // ---- editing, by index -----------------------------------------------------
    // Each fails with InvalidArgument (a rule validate refuses) or NotFound
    // (an index past the end), and then leaves the map exactly as it was.

    /// A copy rather than a reference: an editor reads a rule into a form,
    /// and a reference would dangle at the first edit.
    pub fn at(&self, index: usize) -> Result<SurveyRule> {
        self.rules
            .get(index)
            .cloned()
            .ok_or_else(|| out_of_range(index, self.rules.len()))
    }

    pub fn replace(&mut self, index: usize, rule: SurveyRule) -> Status {
        if index >= self.rules.len() {
            return Err(out_of_range(index, self.rules.len()));
        }
        rule.validate()?;
        self.rules[index] = rule;
        self.reindex(); // the key may have changed
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> Status {
        if index >= self.rules.len() {
            return Err(out_of_range(index, self.rules.len()));
        }
        self.rules.remove(index);
        self.reindex();
        Ok(())
    }

    /// Before the rule now at `index`; `index == size()` appends.
    pub fn insert(&mut self, index: usize, rule: SurveyRule) -> Status {
        // size() itself is allowed: inserting there is appending.
        if index > self.rules.len() {
            return Err(out_of_range(index, self.rules.len()));
        }
        rule.validate()?;
        self.rules.insert(index, rule);
        self.reindex();
        Ok(())
    }

    /// Afterwards the rule is AT `to`, the others keeping their relative
    /// order: [A B C D] move(0, 2) gives [B C A D]. Order is precedence, so
    /// this is how an editor says which of two equal keys wins.
    pub fn r#move(&mut self, from: usize, to: usize) -> Status {
        if from >= self.rules.len() {
            return Err(out_of_range(from, self.rules.len()));
        }
        if to >= self.rules.len() {
            return Err(out_of_range(to, self.rules.len()));
        }
        // A rotation of the span between the two, which is exactly "take it
        // out and put it back at `to`" without a second copy of the rule.
        if from < to {
            self.rules[from..=to].rotate_left(1);
        } else if to < from {
            self.rules[to..=from].rotate_right(1);
        }
        self.reindex();
        Ok(())
    }

    /// The same rules as `match` gives, as indices into rules(), in the same
    /// order - what an explanation cites, since a rule has no identity but
    /// its place.
    pub fn match_indices(&self, code: &str) -> Vec<usize> {
        // The order the old scan-and-stable-sort gave, built directly: the
        // exact key first, then each prefix of the code from the longest to
        // the empty one (the bare `*`). Two keys of one specificity that both
        // match a code must be the SAME key - an exact key equal to it, or a
        // prefix of that length - so they share one index list, and that list
        // is in read order, which is what lets an earlier file win over a
        // later one.
        let mut found = self.exact.get(code).cloned().unwrap_or_default();
        for length in (0..=code.len()).rev() {
            // A prefix that ends inside a character is no key's: every key
            // is whole UTF-8, and so is what is left of one without its `*`.
            if !code.is_char_boundary(length) {
                continue;
            }
            if let Some(prefix) = self.prefixes.get(&code[..length]) {
                found.extend_from_slice(prefix);
            }
        }
        found
    }

    /// Every rule whose key matches, most specific first: an exact key before
    /// a prefix, a longer prefix before a shorter, `*` last. Rules of equal
    /// specificity keep the order they were read.
    pub fn r#match(&self, code: &str) -> Vec<&SurveyRule> {
        self.match_indices(code)
            .into_iter()
            .map(|index| &self.rules[index])
            .collect()
    }

    /// Those rules combined: for each field, the most specific rule that says
    /// anything about it wins. Attributes accumulate instead of overriding.
    pub fn lookup(&self, code: &str) -> SurveyMatch {
        let mut found = SurveyMatch::default();
        for rule in self.r#match(code) {
            if found.keys.is_empty() {
                found.resolved.key.clone_from(&rule.key);
                found.resolved.section = rule.section;
                found.kind = kind_of_key(&rule.key);
            }
            found.keys.push(rule.key.clone());
            let resolved = &mut found.resolved;
            take_text_if_unset(&mut resolved.model, &rule.model);
            take_text_if_unset(&mut resolved.colour, &rule.colour);
            take_text_if_unset(&mut resolved.linestyle, &rule.linestyle);
            take_text_if_unset(&mut resolved.weight, &rule.weight);
            take_text_if_unset(&mut resolved.group, &rule.group);
            take_text_if_unset(&mut resolved.comment, &rule.comment);
            take_if_unset(&mut resolved.breakline, &rule.breakline);
            take_if_unset(&mut resolved.tinable, &rule.tinable);
            take_if_unset(&mut resolved.hide, &rule.hide);
            take_if_unset(&mut resolved.symbol, &rule.symbol);
            take_if_unset(&mut resolved.text_style, &rule.text_style);
            take_if_unset(&mut resolved.pipe, &rule.pipe);
            take_if_unset(&mut resolved.vertex_pipe, &rule.vertex_pipe);
            take_if_unset(&mut resolved.segment_pipe, &rule.segment_pipe);
            append_attributes(&mut resolved.attributes, &rule.attributes);
            append_attributes(&mut resolved.vertex_attributes, &rule.vertex_attributes);
            append_attributes(&mut resolved.segment_attributes, &rule.segment_attributes);
        }
        found
    }

    pub fn rules(&self) -> &[SurveyRule] {
        &self.rules
    }

    pub fn size(&self) -> usize {
        self.rules.len()
    }

    pub fn empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Every distinct key, in name order.
    pub fn keys(&self) -> Vec<String> {
        let distinct: BTreeSet<&str> = self.rules.iter().map(|rule| rule.key.as_str()).collect();
        distinct.into_iter().map(String::from).collect()
    }

    /// Every linestyle and symbol name the rules reference, in name order -
    /// what a library must provide for this map to draw.
    pub fn styles_referenced(&self) -> Vec<String> {
        let mut distinct = BTreeSet::new();
        for rule in &self.rules {
            if !rule.linestyle.is_empty() {
                distinct.insert(rule.linestyle.as_str());
            }
            if let Some(symbol) = &rule.symbol
                && !symbol.style.is_empty()
            {
                distinct.insert(symbol.style.as_str());
            }
        }
        distinct.into_iter().map(String::from).collect()
    }
}
