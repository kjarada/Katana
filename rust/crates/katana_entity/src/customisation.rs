//! A customisation, and the Katana customisation format that keeps one.
//!
//! A customisation is what turns a surveyor's codes into a drawing: the
//! linestyle and symbol definitions (StyleLibrary), the survey code rules
//! (SurveyMap), the colours its names mean (ColourTable) and how linework is
//! processed. It was three files in another program's formats; this is one
//! value and one file of Katana's own:
//!
//! ```text
//!   {"format": "katana-customisation", "version": 1, "name": "NSW",
//!    "colours": {"sui test power": "#FF7F00"},
//!    "linestyles": [{"name": "TEST Water Main", "strokes": [["move", 0, 0], ...]}],
//!    "symbols": [...],
//!    "codes": [{"key": "WM*", "sets": "feature", "layer": "TEST SERVICES"}]}
//! ```
//!
//! docs/customisation.md is the format's specification: every member, why the
//! reader is strict, what may never change without a new version, and the
//! alternatives that were rejected. Three things about it shape this module:
//!
//! * It is STRICT. A member the format does not know, a member given twice and
//!   a value of the wrong type are refused, naming the entry, where a lenient
//!   reader would skip them: a customisation is edited by hand, and a typo
//!   that vanished silently would also be gone from the copy Katana writes
//!   back.
//! * It is LOSSLESS against this model. A customisation the writer accepts
//!   reads back equal (PartialEq below), rule order and absent optionals
//!   included; what it could not write back as itself it refuses to write.
//! * It lives in the entity layer because both `cad`, which loads and edits a
//!   customisation, and the converter of the older formats, which may not see
//!   cad, must reach it (tools/check_layering.cmake). The JSON is private to
//!   this module; everything public is text and katana_core::Result.
//!
//! The reader is nlohmann's parse events (katana_core::json's), as the C++
//! is, because the format reads three things a tree does not keep: a member
//! given twice, a definition's strokes as they go by, and where a number too
//! large to hold stands. Text in the model is validated UTF-8 (docs/rust.md,
//! "Step 2"), so the C++'s refusals of text that is not UTF-8, in the writer
//! and in the checks of a name, have no twin: a String cannot hold such text.

use std::collections::{BTreeMap, BTreeSet};

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_core::text::format_exact_real;
use katana_core::text_encoding::{TextEncoding, decode_text};
use katana_geometry::Point2;

use crate::colour_names::ColourTable;
use crate::entity::Color;
use crate::entity_geometry::Validate;
use crate::layer_path::validate_layer_path;
use crate::linework_codes::{LineworkCodes, linework_code_members};
use crate::style_library::{LineStyle, Stroke, StrokeOp, StrokeText, StyleLibrary, StyleUnits};
use crate::survey_map::{
    SURVEY_SECTION_WORDS, SurveyAttribute, SurveyBreakline, SurveyMap, SurveyPipe, SurveyRule,
    SurveySection, SurveySymbol, SurveyTextStyle,
};
use katana_core::json::{self, JsonError, Sax, sax_parse};

/// The value of a file's "format" member.
pub const CUSTOMISATION_FORMAT: &str = "katana-customisation";
/// The newest "version" this build reads, and the one it writes.
pub const CUSTOMISATION_VERSION: i32 = 1;

// The bounds a file is held to by the reader (and the writer, so that what is
// written reads back). Each is far above anything a drawing needs and far
// below what exhausts a machine: a customisation is read from a file somebody
// else wrote, and a number or a length in it is drawn, looped over and copied.
// The built-in customisation is read by the reader, so it is within every
// bound.

/// No coordinate, radius, length, size or angle is larger in size than a
/// billion: a world coordinate in metres is 25 times the Earth's
/// circumference at that size.
pub const CUSTOMISATION_MOST_MAGNITUDE: f64 = 1.0e9;
/// Two turns; an arc sweeping more is drawn as one that goes round and round,
/// and the number of points it is drawn with follows its sweep.
pub const CUSTOMISATION_MOST_SWEEP_DEGREES: f64 = 720.0;
/// A definition is drawn stroke by stroke at every place it is laid.
pub const CUSTOMISATION_MOST_STROKES: usize = 100_000;
/// Every text member, a notice's line and a colour's name.
pub const CUSTOMISATION_MOST_TEXT_BYTES: usize = 1000;
/// The codes list of one file.
pub const CUSTOMISATION_MOST_RULES: usize = 1_000_000;

/// A customisation's name: not empty, with no line break, '/' or '\', no
/// control character (U+0000 to U+001F, U+007F) and no blank at either end.
/// InvalidArgument otherwise.
///
/// The first three are what the project store refuses
/// (storage::ProjectMetadata): the name is written into a project as the
/// record of what the drawing was drawn with, one name a line and never a
/// path, so a name that passed here and failed there would make every SAVE of
/// the session fail. The last two the store would take. They are refused
/// because the name is an IDENTITY, which a project is matched to its
/// customisation by: "NSW " and "NSW" would be two customisations that look
/// like one, and a tab or an escape cannot be seen where the name is shown. A
/// layer's name is held to the same (validate_layer_path). (The C++ also
/// refused a name that is not UTF-8; a `&str` cannot be one.)
pub fn validate_customisation_name(name: &str) -> Status {
    if name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a customisation name cannot be empty",
        ));
    }
    if name.contains(['\n', '\r', '/', '\\']) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a customisation name cannot hold a line break, '/' or '\\': a project records it as one line, and it is not a path",
            in_quotes(name),
        ));
    }
    // Those three are what the project's store refuses. These two it would
    // take, and are refused because a name is an IDENTITY - a project is
    // matched to its customisation by it - and must be what a person sees:
    // the same rule a layer's name is held to (validate_layer_path).
    if name.bytes().any(|byte| byte < 0x20 || byte == 0x7F) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a customisation name cannot hold a control character: a tab, an escape and the like cannot be seen where the name is shown",
            in_quotes(name),
        ));
    }
    if name.starts_with(' ') || name.ends_with(' ') {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a customisation name cannot begin or end with a blank: it would look like the name without it, and be another",
            in_quotes(name),
        ));
    }
    Ok(())
}

/// One customisation that went into this one, as the session's load record
/// holds it: a customisation merged from two keeps both names, what each
/// brought and each one's own notice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomisationSourceNote {
    pub name: String,
    pub definitions: bool, // it brought linestyle or symbol definitions
    pub rules: bool,       // it brought survey code rules
    pub notice: Vec<String>,
}

/// The customisation an edited copy started from: its name, and the digest
/// (customisation_digest) of the file it was read from, so that a copy kept by
/// a user can be told from one made against an earlier edition of the same
/// name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomisationBase {
    pub name: String,
    pub digest: String,
}

/// What is applied to survey data without being asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomisationAutomation {
    pub codes_on_survey_import: bool, // code the points an import created
    pub linework_on_survey_import: bool, // then join the coded points into lines
}

impl Default for CustomisationAutomation {
    fn default() -> Self {
        Self {
            codes_on_survey_import: true,
            linework_on_survey_import: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Customisation {
    pub name: String,
    pub description: String,
    /// The author's notice, a line an entry: a licence, a disclaimer. Carried
    /// with the data and shown to whoever uses it.
    pub notice: Vec<String>,
    pub sources: Vec<CustomisationSourceNote>,
    pub based_on: Option<CustomisationBase>,
    pub colours: ColourTable,
    /// Absent means the customisation SAYS NOTHING about them, which is not
    /// the same as giving the defaults: merged into a session, it leaves the
    /// session's own alone. A file of symbols for a colleague must not reset
    /// their control codes.
    pub linework: Option<LineworkCodes>,
    pub automation: Option<CustomisationAutomation>,
    /// Each definition's `symbol` says which of the file's two arrays holds
    /// it, and its `source` the customisation it came from.
    pub library: StyleLibrary,
    pub map: SurveyMap,
}

/// Equal when every member is, the library definition for definition and the
/// rules in the same order. (NamedTable has no PartialEq of its own: a table
/// also counts its revisions, which are not part of its value.)
impl PartialEq for Customisation {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.description == other.description
            && self.notice == other.notice
            && self.sources == other.sources
            && self.based_on == other.based_on
            && self.colours == other.colours
            && self.linework == other.linework
            && self.automation == other.automation
            && same_definitions(&self.library, &other.library)
            && self.map == other.map
    }
}

/// What a write holds of a customisation's definitions and rules. Its name,
/// notice, sources, colours, linework and automation are written as the value
/// has them - a caller that wants one left out clears it in a copy first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomisationWriteOptions {
    pub linestyles: bool, // the definitions not listed as symbols
    pub symbols: bool,    // the definitions listed as symbols
    pub codes: bool,      // the survey code rules
    /// Only these definitions, by name; every definition when empty. A name
    /// the library lacks fails the write (NotFound), and so does one whose
    /// kind the two flags above leave out (InvalidArgument): a definition
    /// asked for by name is never dropped silently.
    pub only: Vec<String>,
}

impl Default for CustomisationWriteOptions {
    fn default() -> Self {
        Self {
            linestyles: true,
            symbols: true,
            codes: true,
            only: Vec::new(),
        }
    }
}

// ---- the format's words -------------------------------------------------------------
//
// Every word here is part of version 1: a word changed, and a word ADDED, is a
// new version (an older Katana would refuse the file for it). The reader and
// the writer share every table of words, so the two cannot come to disagree
// about a spelling.

#[derive(Clone, Copy)]
struct Word<E> {
    value: E,
    word: &'static str,
}

static UNITS_WORDS: [Word<StyleUnits>; 3] = [
    Word {
        value: StyleUnits::World,
        word: "world",
    },
    Word {
        value: StyleUnits::Paper,
        word: "paper",
    },
    Word {
        value: StyleUnits::TwoPoint,
        word: "twoPoint",
    },
];

static DRAW_WORDS: [Word<SurveyBreakline>; 2] = [
    Word {
        value: SurveyBreakline::Line,
        word: "line",
    },
    Word {
        value: SurveyBreakline::Point,
        word: "point",
    },
];

const SECTION_COUNT: usize = SURVEY_SECTION_WORDS.len();

/// What a rule SETS, in the words of the entity layer's one table of them
/// (SURVEY_SECTION_WORDS, survey_map.rs). The EnumName of a SurveySection
/// reads the same table for the survey code tools' text, so a file holds the
/// word the window shows for the same thing.
static SECTION_WORDS: [Word<SurveySection>; SECTION_COUNT] = section_words();

const fn section_words() -> [Word<SurveySection>; SECTION_COUNT] {
    let mut table = [Word {
        value: SurveySection::Map,
        word: "",
    }; SECTION_COUNT];
    let mut i = 0;
    while i < SECTION_COUNT {
        table[i] = Word {
            value: SURVEY_SECTION_WORDS[i].section,
            word: SURVEY_SECTION_WORDS[i].word,
        };
        i += 1;
    }
    table
}

/// The model keeps an attribute's type as text; the format holds only these.
const ATTRIBUTE_TYPES: [&str; 2] = ["text", "integer"];

/// A stroke is an array: its word, then `values` more. Move and draw first,
/// which is what a library is made of.
struct StrokeWord {
    op: StrokeOp,
    word: &'static str,
    values: usize,
    takes: &'static str, // the values in words, for a refusal
}

static STROKE_WORDS: [StrokeWord; 7] = [
    StrokeWord {
        op: StrokeOp::Move,
        word: "move",
        values: 2,
        takes: "x and y",
    },
    StrokeWord {
        op: StrokeOp::Draw,
        word: "draw",
        values: 2,
        takes: "x and y",
    },
    StrokeWord {
        op: StrokeOp::Arc,
        word: "arc",
        values: 3,
        takes: "a radius, a start angle and an end angle",
    },
    StrokeWord {
        op: StrokeOp::Circle,
        word: "circle",
        values: 1,
        takes: "a radius",
    },
    StrokeWord {
        op: StrokeOp::Dot,
        word: "dot",
        values: 1,
        takes: "a radius",
    },
    StrokeWord {
        op: StrokeOp::Pen,
        word: "pen",
        values: 1,
        takes: "a colour name",
    },
    StrokeWord {
        op: StrokeOp::Text,
        word: "text",
        values: 1,
        takes: "one object",
    },
];

const TOP_LEVEL: &str = "top level";
const NOT_A_CUSTOMISATION: &str = "not a Katana customisation file";
const NOT_A_DEFINITION: &str = "not a definition in the Katana customisation format";
const NOT_A_STROKE: &str = "a stroke is a list that begins with its kind, such as [\"move\", 0, 0]";

/// The word of a value; empty for a value outside the enumeration, which the
/// writer refuses (and which a Rust enumeration cannot hold).
fn word_of<E: Copy + PartialEq>(words: &[Word<E>], value: E) -> &'static str {
    words
        .iter()
        .find(|entry| entry.value == value)
        .map_or("", |entry| entry.word)
}

fn stroke_word_of_op(op: StrokeOp) -> Option<&'static StrokeWord> {
    STROKE_WORDS.iter().find(|entry| entry.op == op)
}

fn stroke_word_of_text(word: &str) -> Option<&'static StrokeWord> {
    STROKE_WORDS.iter().find(|entry| entry.word == word)
}

fn listed<E>(words: &[Word<E>]) -> String {
    words
        .iter()
        .map(|entry| entry.word)
        .collect::<Vec<_>>()
        .join(", ")
}

// ---- refusals -----------------------------------------------------------------------

/// "<where>: <what>" - every refusal names the entry it is about first.
fn refuse(code: ErrorCode, place: &str, what: &str, context: impl Into<String>) -> Error {
    make_error(code, format!("{place}: {what}"), context)
}

/// A name as a message shows it: in double quotes, with what would break the
/// line or the quoting escaped the way JSON escapes it.
///
/// Cut short past 240 bytes: a refusal names the entry it is about, and an
/// entry's name may be as long as the file is (a name over the bound is
/// refused, but is named in that refusal too, and in every other about it).
/// A cut inside a character ends in U+FFFD, as nlohmann's replacing dump of
/// the cut bytes does.
fn in_quotes(text: &str) -> String {
    const LONGEST_QUOTED: usize = 240;
    let mut quoted = String::new();
    if text.len() <= LONGEST_QUOTED {
        json::write_string(&mut quoted, text, false);
        return quoted;
    }
    let mut end = LONGEST_QUOTED;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut piece = String::from(&text[..end]);
    if end < LONGEST_QUOTED {
        piece.push('\u{FFFD}');
    }
    json::write_string(&mut quoted, &piece, false);
    quoted.insert_str(quoted.len() - 1, "...");
    quoted
}

const TOO_LARGE: &str = "has a number too large to hold";
const TOO_SMALL: &str = "has a number too small to hold: it is not zero, and would be read as 0";

// The format's bounds, in the words a refusal says them in.
const _: () = assert!(
    CUSTOMISATION_MOST_MAGNITUDE == 1.0e9
        && CUSTOMISATION_MOST_SWEEP_DEGREES == 720.0
        && CUSTOMISATION_MOST_STROKES == 100_000
        && CUSTOMISATION_MOST_TEXT_BYTES == 1000
        && CUSTOMISATION_MOST_RULES == 1_000_000,
    "the refusals below say these numbers"
);
const BEYOND_MAGNITUDE: &str =
    "larger than 1000000000 in size, the most any number of a customisation holds";
const TOO_LONG: &str = "longer than 1000 bytes, the most a name or text of a customisation holds";
const SWEEPS_TOO_FAR: &str = "sweeps more than 720 degrees (two turns), the most an arc holds";
const TOO_MANY_STROKES: &str = "a definition holds at most 100000 strokes, and this is one more";
const TOO_MANY_RULES: &str =
    "\"codes\" holds more than 1000000 rules, the most a customisation holds";

/// A NaN is not within it, and is no number the file can hold either.
fn within_magnitude(value: f64) -> bool {
    value.abs() <= CUSTOMISATION_MOST_MAGNITUDE
}

fn within_sweep(start_angle: f64, end_angle: f64) -> bool {
    (end_angle - start_angle).abs() <= CUSTOMISATION_MOST_SWEEP_DEGREES
}

/// Whether a number's text, which the JSON reader read as zero, says some
/// other number: a digit that is not 0 before any exponent. ("0e-400" and
/// "-0.000" are zero; "1e-400" is below the smallest double and is not.)
fn says_more_than_zero(written: &str) -> bool {
    for character in written.bytes() {
        if character == b'e' || character == b'E' {
            break;
        }
        if (b'1'..=b'9').contains(&character) {
            return true;
        }
    }
    false
}

/// The most a refusal shows of a value.
const LONGEST_SHOWN: usize = 60;

// ---- what the tree holds ------------------------------------------------------------
//
// The tree the reader builds is nlohmann's json as the C++ uses it: objects
// are std::maps, so their members are walked in the order of their names. Two
// kinds of node stand for something the text held that a tree cannot hold as
// itself; in the C++ each is a BINARY value, because JSON text has none, so
// nothing a file can say is mistaken for either:
//
//   * Strokes: where a definition's strokes were, which entry of
//     Parsed::strokes they are (they are read as the text is parsed and never
//     become a tree).
//   * Lost: a number the reader cannot hold, as the text wrote it. Whatever
//     reads a number refuses it by the member it is in; anything else refuses
//     it as it would any value of the wrong kind, and shows it as written.
//
// The nodes live in one arena and a container holds its members' places in
// it, so nothing recursive is ever built or dropped: a value may nest as deep
// as its text is long, and nlohmann's own destructor is iterative for that
// reason.

enum Node {
    Null,
    Boolean(bool),
    Integer(i64),
    Unsigned(u64),
    Float(f64),
    Text(String),
    Lost(String),
    Strokes(usize),
    Array(Vec<usize>),
    Object(BTreeMap<String, usize>),
}

struct Tree {
    nodes: Vec<Node>,
    root: usize,
}

impl Tree {
    fn new() -> Self {
        // The root is null until a value is read.
        Self {
            nodes: vec![Node::Null],
            root: 0,
        }
    }

    fn push(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn at(&self, at: usize) -> Json<'_> {
        Json { tree: self, at }
    }

    fn root(&self) -> Json<'_> {
        self.at(self.root)
    }
}

/// A value of the tree, as a `const Json&` of the C++ is.
#[derive(Clone, Copy)]
struct Json<'a> {
    tree: &'a Tree,
    at: usize,
}

impl<'a> Json<'a> {
    fn node(self) -> &'a Node {
        &self.tree.nodes[self.at]
    }

    fn is_object(&self) -> bool {
        matches!(self.node(), Node::Object(_))
    }

    fn is_array(&self) -> bool {
        matches!(self.node(), Node::Array(_))
    }

    fn is_string(&self) -> bool {
        matches!(self.node(), Node::Text(_))
    }

    fn is_number(&self) -> bool {
        matches!(
            self.node(),
            Node::Integer(_) | Node::Unsigned(_) | Node::Float(_)
        )
    }

    fn is_number_integer(&self) -> bool {
        matches!(self.node(), Node::Integer(_) | Node::Unsigned(_))
    }

    fn is_number_unsigned(&self) -> bool {
        matches!(self.node(), Node::Unsigned(_))
    }

    fn is_boolean(&self) -> bool {
        matches!(self.node(), Node::Boolean(_))
    }

    fn is_lost(&self) -> bool {
        matches!(self.node(), Node::Lost(_))
    }

    /// nlohmann's size(): 0 for null, the members of a list or an object, 1
    /// for anything else.
    fn size(self) -> usize {
        match self.node() {
            Node::Null => 0,
            Node::Array(items) => items.len(),
            Node::Object(members) => members.len(),
            _ => 1,
        }
    }

    fn find(self, key: &str) -> Option<Json<'a>> {
        match self.node() {
            Node::Object(members) => members.get(key).map(|&at| self.tree.at(at)),
            _ => None,
        }
    }

    /// A list's values; nothing for anything else.
    fn elements(self) -> impl Iterator<Item = Json<'a>> + 'a {
        let tree = self.tree;
        let items: &'a [usize] = match self.node() {
            Node::Array(items) => items.as_slice(),
            _ => &[],
        };
        items.iter().map(move |&at| tree.at(at))
    }

    /// An object's members in the order of their names; nothing for anything
    /// else.
    fn members(self) -> impl Iterator<Item = (&'a str, Json<'a>)> + 'a {
        let tree = self.tree;
        let members = match self.node() {
            Node::Object(members) => Some(members),
            _ => None,
        };
        members.into_iter().flat_map(move |members| {
            members
                .iter()
                .map(move |(key, &at)| (key.as_str(), tree.at(at)))
        })
    }

    /// get_ref<const std::string&>, asked of text only.
    fn as_str(&self) -> &'a str {
        match self.node() {
            Node::Text(text) => text.as_str(),
            _ => "",
        }
    }

    /// get<double>: nlohmann's static_cast of whichever number it holds.
    fn as_f64(&self) -> f64 {
        match *self.node() {
            Node::Integer(value) => value as f64,
            Node::Unsigned(value) => value as f64,
            Node::Float(value) => value,
            _ => 0.0,
        }
    }

    fn as_u64(&self) -> u64 {
        match *self.node() {
            Node::Integer(value) => value as u64,
            Node::Unsigned(value) => value,
            Node::Float(value) => value as u64,
            _ => 0,
        }
    }

    fn as_i64(&self) -> i64 {
        match *self.node() {
            Node::Integer(value) => value,
            Node::Unsigned(value) => value as i64,
            Node::Float(value) => value as i64,
            _ => 0,
        }
    }

    fn as_bool(&self) -> bool {
        matches!(self.node(), Node::Boolean(true))
    }
}

/// Appends `value` as a file would write it on one line, and stops once enough
/// is written to be cut off. NOT a whole dump: that writes the whole value
/// before any of it is cut, going down a level of the program's stack for
/// every level of the value - and a value may nest as deep as its text is
/// long. 100,000 "[" are a file of 200 KB, and were a stack overflow. This goes
/// down a level only after writing a character, so never deeper than
/// LONGEST_SHOWN.
fn append_shown(text: &mut String, value: Json<'_>) {
    if text.len() > LONGEST_SHOWN {
        return;
    }
    match value.node() {
        Node::Array(items) => {
            text.push('[');
            for (i, &item) in items.iter().enumerate() {
                if text.len() > LONGEST_SHOWN {
                    return;
                }
                if i > 0 {
                    text.push(',');
                }
                append_shown(text, value.tree.at(item));
            }
            text.push(']');
        }
        Node::Object(members) => {
            text.push('{');
            for (i, (key, &item)) in members.iter().enumerate() {
                if text.len() > LONGEST_SHOWN {
                    return;
                }
                if i > 0 {
                    text.push(',');
                }
                json::write_string(text, key, false);
                text.push(':');
                append_shown(text, value.tree.at(item));
            }
            text.push('}');
        }
        Node::Lost(written) => text.push_str(written),
        // a list of strokes, which is not kept as it was written
        Node::Strokes(_) => text.push_str("[...]"),
        // text, a number, true, false or null: nothing below it to go down to
        Node::Null => text.push_str("null"),
        Node::Boolean(flag) => text.push_str(if *flag { "true" } else { "false" }),
        Node::Integer(number) => text.push_str(&format!("{number}")),
        Node::Unsigned(number) => text.push_str(&format!("{number}")),
        Node::Float(number) => text.push_str(&json::dump_float(*number)),
        Node::Text(string) => json::write_string(text, string, false),
    }
}

/// What a refusal shows, cut at LONGEST_SHOWN bytes.
fn cut_short(mut text: String) -> String {
    if text.len() > LONGEST_SHOWN {
        // Never in the middle of a character: step back over continuation
        // bytes.
        let mut cut = LONGEST_SHOWN;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("...");
    }
    text
}

/// A value as the file wrote it, cut short: what a refusal shows of a value of
/// the wrong type.
fn shown(value: Json<'_>) -> String {
    let mut text = String::new();
    append_shown(&mut text, value);
    cut_short(text)
}

/// The same for text that is not in the tree (an object's member name).
fn shown_text(value: &str) -> String {
    let mut text = String::new();
    json::write_string(&mut text, value, false);
    cut_short(text)
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// "strokes[9]", or "<entry> strokes[9]": a place in a list, counted from 0 as
/// a JSON path counts.
fn placed(entry: &str, list: &str, index: usize) -> String {
    let mut label = String::from(entry);
    if !label.is_empty() {
        label.push(' ');
    }
    label.push_str(list);
    label.push_str(&format!("[{index}]"));
    label
}

/// "codes[57] "WM*"": the array, the place in it and the entry's own name. All
/// three, because a rule's key alone is ambiguous - several rules share one -
/// and a place alone says nothing to the person who wrote the file.
fn labelled(array: &str, index: usize, name: &str) -> String {
    let mut label = placed("", array, index);
    label.push(' ');
    label.push_str(&in_quotes(name));
    label
}

/// The same from the file, where an entry may not be an object or may have no
/// name to show.
fn entry_label(array: &str, index: usize, entry: Json<'_>, name_member: &str) -> String {
    if let Some(name) = entry.find(name_member)
        && name.is_string()
    {
        return labelled(array, index, name.as_str());
    }
    placed("", array, index)
}

/// The member that names an entry of a top-level array, or nothing.
fn name_member_of(array: &str) -> &'static str {
    match array {
        "codes" => "key",
        "linestyles" | "symbols" | "sources" => "name",
        _ => "",
    }
}

fn is_digest(text: &str) -> bool {
    text.len() == 16
        && text
            .bytes()
            .all(|character| character.is_ascii_digit() || (b'a'..=b'f').contains(&character))
}

/// What both the reader and the writer require of a customisation outside its
/// definitions and rules. One function, so that the writer cannot write a file
/// the reader would then refuse - a kept customisation nobody can open again.
fn check_header(customisation: &Customisation) -> Status {
    if let Err(error) = validate_customisation_name(&customisation.name) {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            TOP_LEVEL,
            &format!("\"name\": {}", error.message),
            error.context,
        ));
    }
    for (i, source) in customisation.sources.iter().enumerate() {
        if let Err(error) = validate_customisation_name(&source.name) {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                &labelled("sources", i, &source.name),
                &format!("\"name\": {}", error.message),
                error.context,
            ));
        }
    }
    if let Some(base) = &customisation.based_on {
        if let Err(error) = validate_customisation_name(&base.name) {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                "basedOn",
                &format!("\"name\": {}", error.message),
                error.context,
            ));
        }
        if !is_digest(&base.digest) {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                "basedOn",
                "\"digest\" must be 16 hexadecimal digits in lower case",
                in_quotes(&base.digest),
            ));
        }
    }
    if let Some(linework) = &customisation.linework
        && let Err(error) = linework.validate()
    {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            "linework",
            &error.message,
            error.context,
        ));
    }
    Ok(())
}

/// A definition's source is a customisation's name, or empty for one made in
/// a session; it reaches a project's record like the customisation's own
/// name.
fn check_source(definition: &LineStyle, place: &str) -> Status {
    if definition.source.is_empty() {
        return Ok(());
    }
    if let Err(error) = validate_customisation_name(&definition.source) {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            place,
            &format!("\"from\": {}", error.message),
            error.context,
        ));
    }
    Ok(())
}

/// Doubles are compared bit for bit when deciding whether a member is at its
/// default, so -0.0 is not "the default 0" and is written, and reads back as
/// -0.0. (== would call the two equal and the sign would be lost.)
fn same_bits(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

fn same_point(a: Point2, b: Point2) -> bool {
    same_bits(a.x, b.x) && same_bits(a.y, b.y)
}

// ---- writing ------------------------------------------------------------------------
//
// A hand-written emitter rather than a JSON library's dump(): that gives either
// one line for the whole file or one line for every number, and a library of
// 35,000 strokes is read, compared and merged by people as text. One stroke a
// line is the unit a diff shows and a person edits.

const NO_STROKE: usize = usize::MAX;

struct Out {
    /// Where a refusal says the trouble is: the entry being written, the
    /// member of it and, inside a definition, the stroke.
    entry: String,
    member: &'static str,
    stroke: usize,
    written: String,
}

impl Out {
    fn new(entry: &str) -> Self {
        Self {
            entry: String::from(entry),
            member: "",
            stroke: NO_STROKE,
            written: String::new(),
        }
    }

    fn reserve(&mut self, bytes: usize) {
        self.written.reserve(bytes);
    }

    fn raw(&mut self, piece: &str) {
        self.written.push_str(piece);
    }

    /// Text is written as JSON escapes it; text past the bound is refused
    /// here, where the entry can be named.
    fn text(&mut self, value: &str) -> Status {
        if value.len() > CUSTOMISATION_MOST_TEXT_BYTES {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                &self.place(),
                &format!("{} is {TOO_LONG}", self.subject()),
                "",
            ));
        }
        json::write_string(&mut self.written, value, false);
        Ok(())
    }

    fn real(&mut self, value: f64) -> Status {
        if !value.is_finite() {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                &self.place(),
                &format!(
                    "{} is not a finite number (an infinity or NaN), which the format cannot hold",
                    self.subject()
                ),
                "",
            ));
        }
        if !within_magnitude(value) {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                &self.place(),
                &format!("{} is {BEYOND_MAGNITUDE}", self.subject()),
                "",
            ));
        }
        // The shortest text that reads back as the same double is "-0" for
        // negative zero - which a JSON reader takes for the INTEGER 0, losing
        // the sign. "-0.0" is a real and keeps it.
        if value == 0.0 && value.is_sign_negative() {
            self.written.push_str("-0.0");
            return Ok(());
        }
        self.written.push_str(&format_exact_real(value));
        Ok(())
    }

    fn whole(&mut self, value: i64) {
        self.written.push_str(&format!("{value}"));
    }

    fn flag(&mut self, value: bool) {
        self.written.push_str(if value { "true" } else { "false" });
    }

    /// A word of the format: written in quotes as it stands, since every one
    /// is plain ASCII letters. An empty word is a value outside its
    /// enumeration.
    fn word(&mut self, value: &str) -> Status {
        if value.is_empty() {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                &self.place(),
                &format!("{} holds a value outside its enumeration", self.subject()),
                "",
            ));
        }
        self.written.push('"');
        self.written.push_str(value);
        self.written.push('"');
        Ok(())
    }

    fn place(&self) -> String {
        if self.stroke == NO_STROKE {
            self.entry.clone()
        } else {
            placed(&self.entry, "strokes", self.stroke)
        }
    }

    fn subject(&self) -> String {
        if self.member.is_empty() {
            String::from("a value")
        } else {
            in_quotes(self.member)
        }
    }

    /// Names a part of an entry while it is being written (`codes[3] "AC*"
    /// symbol`); `leave` with what this returns puts the entry's own name
    /// back.
    fn within(&mut self, part: &str) -> usize {
        let size = self.entry.len();
        self.entry.push(' ');
        self.entry.push_str(part);
        size
    }

    fn leave(&mut self, size: usize) {
        self.entry.truncate(size);
    }

    fn take(self) -> String {
        self.written
    }
}

/// One object written on one line: {"a": 1, "b": "x"}. A member at its
/// default is left out by the helpers that take a fallback; key() is for the
/// members that are always written and for values the caller writes itself.
struct Object {
    first: bool,
}

impl Object {
    fn open(out: &mut Out) -> Self {
        out.raw("{");
        Self { first: true }
    }

    fn key(&mut self, out: &mut Out, name: &'static str) {
        out.raw(if self.first { "\"" } else { ", \"" });
        self.first = false;
        out.raw(name);
        out.raw("\": ");
        out.member = name;
    }

    fn text(&mut self, out: &mut Out, name: &'static str, value: &str, fallback: &str) -> Status {
        if value != fallback {
            self.key(out, name);
            out.text(value)?;
        }
        Ok(())
    }

    fn real(&mut self, out: &mut Out, name: &'static str, value: f64, fallback: f64) -> Status {
        if !same_bits(value, fallback) {
            self.key(out, name);
            out.real(value)?;
        }
        Ok(())
    }

    fn whole(&mut self, out: &mut Out, name: &'static str, value: i32, fallback: i32) {
        if value != fallback {
            self.key(out, name);
            out.whole(i64::from(value));
        }
    }

    fn flag(&mut self, out: &mut Out, name: &'static str, value: bool, fallback: bool) {
        if value != fallback {
            self.key(out, name);
            out.flag(value);
        }
    }

    fn close(self, out: &mut Out) {
        out.raw("}");
    }
}

fn write_point(out: &mut Out, point: Point2) -> Status {
    out.raw("[");
    out.real(point.x)?;
    out.raw(", ");
    out.real(point.y)?;
    out.raw("]");
    Ok(())
}

fn write_lines(out: &mut Out, lines: &[String]) -> Status {
    out.raw("[");
    for (i, line) in lines.iter().enumerate() {
        out.raw(if i == 0 { "" } else { ", " });
        out.text(line)?;
    }
    out.raw("]");
    Ok(())
}

/// What the writer refuses of a definition's strokes, so that "lossless" is
/// literally true: a stroke carrying a member its kind does not use (the file
/// has nowhere to put it), and texts that are not exactly the text strokes in
/// order (a text no stroke places, or one two strokes share, has no form
/// either). validate allows both; they are not errors in a model, only things
/// this file cannot say.
fn check_strokes(definition: &LineStyle, place: &str) -> Status {
    let d = Stroke::default();
    if definition.strokes.len() > CUSTOMISATION_MOST_STROKES {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            place,
            &format!(
                "the definition holds {}, and a definition holds at most 100000",
                counted(definition.strokes.len(), "stroke", "strokes")
            ),
            "",
        ));
    }
    let mut texts = 0;
    for (i, stroke) in definition.strokes.iter().enumerate() {
        let here = || placed(place, "strokes", i);
        let Some(kind) = stroke_word_of_op(stroke.op) else {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                &here(),
                "the stroke is of no kind the format knows",
                "",
            ));
        };
        let uses_point = stroke.op == StrokeOp::Move || stroke.op == StrokeOp::Draw;
        let uses_radius = stroke.op == StrokeOp::Arc
            || stroke.op == StrokeOp::Circle
            || stroke.op == StrokeOp::Dot;
        let uses_angles = stroke.op == StrokeOp::Arc;
        let stray = if !uses_point && !same_point(stroke.point, d.point) {
            "a point"
        } else if !uses_radius && !same_bits(stroke.radius, d.radius) {
            "a radius"
        } else if !uses_angles
            && (!same_bits(stroke.start_angle, d.start_angle)
                || !same_bits(stroke.end_angle, d.end_angle))
        {
            "an angle"
        } else if stroke.op != StrokeOp::Text && stroke.text != d.text {
            "a text"
        } else if stroke.op != StrokeOp::Pen && !stroke.pen.is_empty() {
            "a pen"
        } else {
            ""
        };
        if !stray.is_empty() {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                &here(),
                &format!(
                    "this {} stroke carries {stray}, which it does not use and the format cannot hold",
                    kind.word
                ),
                "",
            ));
        }
        if stroke.op == StrokeOp::Arc && !within_sweep(stroke.start_angle, stroke.end_angle) {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                &here(),
                &format!("this arc {SWEEPS_TOO_FAR}"),
                "",
            ));
        }
        if stroke.op == StrokeOp::Text {
            if stroke.text != texts {
                let named = if stroke.text == Stroke::NO_TEXT {
                    String::from("none")
                } else {
                    format!("{}", stroke.text)
                };
                return Err(refuse(
                    ErrorCode::InvalidArgument,
                    &here(),
                    &format!(
                        "the texts of this definition are not its text strokes in order: this is text stroke {texts} and names text {named}"
                    ),
                    "",
                ));
            }
            texts += 1;
        }
    }
    if texts != definition.texts.len() {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            place,
            &format!(
                "the definition holds {} and {}: a text no stroke places cannot be written",
                counted(definition.texts.len(), "text", "texts"),
                counted(texts, "text stroke", "text strokes")
            ),
            "",
        ));
    }
    Ok(())
}

fn write_stroke(out: &mut Out, definition: &LineStyle, stroke: &Stroke) -> Status {
    let d = StrokeText::default();
    // check_strokes found its kind
    let Some(kind) = stroke_word_of_op(stroke.op) else {
        return Ok(());
    };
    out.member = kind.word;
    out.raw("[\"");
    out.raw(kind.word);
    out.raw("\", ");
    match stroke.op {
        StrokeOp::Move | StrokeOp::Draw => {
            out.real(stroke.point.x)?;
            out.raw(", ");
            out.real(stroke.point.y)?;
        }
        StrokeOp::Arc => {
            out.real(stroke.radius)?;
            out.raw(", ");
            out.real(stroke.start_angle)?;
            out.raw(", ");
            out.real(stroke.end_angle)?;
        }
        StrokeOp::Circle | StrokeOp::Dot => {
            out.real(stroke.radius)?;
        }
        StrokeOp::Pen => {
            out.text(&stroke.pen)?;
        }
        StrokeOp::Text => {
            // in step with the strokes: check_strokes
            let text = &definition.texts[stroke.text];
            let mut object = Object::open(out);
            object.text(out, "text", &text.text, &d.text)?;
            object.real(out, "angle", text.angle, d.angle)?;
            object.real(out, "height", text.height, d.height)?;
            object.text(out, "justify", &text.justify, &d.justify)?;
            object.text(out, "font", &text.font, &d.font)?;
            object.real(out, "widthFactor", text.width_factor, d.width_factor)?;
            if !same_bits(text.unnamed[0], d.unnamed[0])
                || !same_bits(text.unnamed[1], d.unnamed[1])
                || !same_bits(text.unnamed[2], d.unnamed[2])
            {
                object.key(out, "extra");
                out.raw("[");
                out.real(text.unnamed[0])?;
                out.raw(", ");
                out.real(text.unnamed[1])?;
                out.raw(", ");
                out.real(text.unnamed[2])?;
                out.raw("]");
            }
            object.close(out);
        }
    }
    out.raw("]");
    Ok(())
}

/// One definition: its members on one line and then its strokes one a line,
/// each indented two further than `indent`. `out.entry` names it already.
fn write_definition(
    out: &mut Out,
    definition: &LineStyle,
    customisation: &str,
    indent: &str,
) -> Status {
    let d = LineStyle::default();
    // A library holds only what validate accepted; a definition handed over
    // on its own (definition_to_json) may not have been asked.
    if let Err(error) = definition.validate() {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            &out.entry,
            &error.message,
            "",
        ));
    }
    check_source(definition, &out.entry)?;
    check_strokes(definition, &out.entry)?;

    let mut object = Object::open(out);
    object.key(out, "name");
    out.text(&definition.name)?;
    object.text(out, "group", &definition.group, &d.group)?;
    if definition.units != d.units {
        object.key(out, "units");
        out.word(word_of(&UNITS_WORDS, definition.units))?;
    }
    object.flag(out, "atVertices", definition.at_vertices, d.at_vertices);
    object.real(out, "length", definition.length, d.length)?;
    object.real(out, "factor", definition.factor, d.factor)?;
    if !same_point(definition.origin, d.origin) {
        object.key(out, "origin");
        write_point(out, definition.origin)?;
    }
    if !same_point(definition.anchor1, d.anchor1) || !same_point(definition.anchor2, d.anchor2) {
        object.key(out, "anchors");
        out.raw("[");
        write_point(out, definition.anchor1)?;
        out.raw(", ");
        write_point(out, definition.anchor2)?;
        out.raw("]");
    }
    object.whole(out, "stretchMode", definition.stretch_mode, d.stretch_mode);
    object.whole(out, "cycleMode", definition.cycle_mode, d.cycle_mode);
    // Written only when it differs from the name it sits under; an absent
    // "from" reads as that name. So a source of "" under a named customisation
    // IS written, as "", and stays "made in a session".
    if definition.source != customisation {
        object.key(out, "from");
        out.text(&definition.source)?;
    }
    if !definition.strokes.is_empty() {
        object.key(out, "strokes");
        out.raw("[");
        for (i, stroke) in definition.strokes.iter().enumerate() {
            out.raw(if i == 0 { "\n" } else { ",\n" });
            out.raw(indent);
            out.raw("  ");
            out.stroke = i;
            write_stroke(out, definition, stroke)?;
        }
        out.stroke = NO_STROKE;
        out.raw("\n");
        out.raw(indent);
        out.raw("]");
    }
    object.close(out);
    Ok(())
}

fn write_attributes(out: &mut Out, name: &str, attributes: &[SurveyAttribute]) -> Status {
    out.raw("[");
    for (i, attribute) in attributes.iter().enumerate() {
        let size = out.within(&placed("", name, i));
        out.raw(if i == 0 { "" } else { ", " });
        if !ATTRIBUTE_TYPES.contains(&attribute.r#type.as_str()) {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                &out.entry,
                &format!(
                    "\"type\" is {}, and an attribute is \"text\" or \"integer\"",
                    in_quotes(&attribute.r#type)
                ),
                "",
            ));
        }
        let mut object = Object::open(out);
        object.key(out, "type");
        out.word(&attribute.r#type)?;
        object.key(out, "name");
        out.text(&attribute.name)?;
        object.text(out, "value", &attribute.value, "")?;
        object.close(out);
        out.leave(size);
    }
    out.raw("]");
    Ok(())
}

fn write_pipe(out: &mut Out, name: &str, pipe: &SurveyPipe) -> Status {
    let d = SurveyPipe::default();
    let size = out.within(name);
    let mut object = Object::open(out);
    object.text(out, "justify", &pipe.justify, &d.justify)?;
    object.text(out, "shape", &pipe.shape, &d.shape)?;
    object.text(out, "size1", &pipe.size1, &d.size1)?;
    object.text(out, "size2", &pipe.size2, &d.size2)?;
    object.flag(out, "active", pipe.active, d.active);
    object.close(out);
    out.leave(size);
    Ok(())
}

/// One rule on one line. An optional that is PRESENT is written even when
/// everything in it is at its default ("text": {}): present and absent are two
/// states of the model - SurveyMap::lookup takes a present sub-object whole,
/// so an empty one still stops a less specific rule's from showing through.
fn write_rule(out: &mut Out, rule: &SurveyRule) -> Status {
    let d = SurveyRule::default();
    let mut object = Object::open(out);
    object.key(out, "key");
    out.text(&rule.key)?;
    object.key(out, "sets");
    out.word(word_of(&SECTION_WORDS, rule.section))?;
    object.text(out, "layer", &rule.model, &d.model)?;
    object.text(out, "colour", &rule.colour, &d.colour)?;
    if let Some(breakline) = rule.breakline {
        object.key(out, "draw");
        out.word(word_of(&DRAW_WORDS, breakline))?;
    }
    object.text(out, "linestyle", &rule.linestyle, &d.linestyle)?;
    object.text(out, "weight", &rule.weight, &d.weight)?;
    object.text(out, "group", &rule.group, &d.group)?;
    object.text(out, "comment", &rule.comment, &d.comment)?;
    if let Some(tinable) = rule.tinable {
        object.key(out, "surface");
        out.flag(tinable);
    }
    if let Some(hide) = rule.hide {
        object.key(out, "hide");
        out.flag(hide);
    }
    if let Some(symbol) = &rule.symbol {
        let s = SurveySymbol::default();
        object.key(out, "symbol");
        let size = out.within("symbol");
        let mut part = Object::open(out);
        part.text(out, "name", &symbol.style, &s.style)?;
        part.text(out, "colour", &symbol.colour, &s.colour)?;
        part.real(out, "size", symbol.size, s.size)?;
        part.real(out, "rotation", symbol.rotation, s.rotation)?;
        part.real(out, "offset", symbol.offset, s.offset)?;
        part.real(out, "raise", symbol.raise, s.raise)?;
        part.close(out);
        out.leave(size);
    }
    if let Some(style) = &rule.text_style {
        let t = SurveyTextStyle::default();
        object.key(out, "text");
        let size = out.within("text");
        let mut part = Object::open(out);
        part.text(out, "style", &style.textstyle, &t.textstyle)?;
        part.text(out, "colour", &style.colour, &t.colour)?;
        part.text(out, "units", &style.r#type, &t.r#type)?;
        part.real(out, "size", style.size, t.size)?;
        part.text(out, "justifyX", &style.justify_x, &t.justify_x)?;
        part.text(out, "justifyY", &style.justify_y, &t.justify_y)?;
        part.real(out, "offset", style.offset, t.offset)?;
        part.real(out, "raise", style.raise, t.raise)?;
        part.real(out, "angle", style.angle, t.angle)?;
        part.real(out, "slant", style.slant, t.slant)?;
        part.real(out, "widthFactor", style.width_factor, t.width_factor)?;
        part.flag(out, "underline", style.underline, t.underline);
        part.flag(out, "strikeout", style.strikeout, t.strikeout);
        part.flag(out, "italic", style.italic, t.italic);
        part.text(out, "weight", &style.weight, &t.weight)?;
        part.close(out);
        out.leave(size);
    }
    if let Some(pipe) = &rule.pipe {
        object.key(out, "pipe");
        write_pipe(out, "pipe", pipe)?;
    }
    if let Some(pipe) = &rule.vertex_pipe {
        object.key(out, "vertexPipe");
        write_pipe(out, "vertexPipe", pipe)?;
    }
    if let Some(pipe) = &rule.segment_pipe {
        object.key(out, "segmentPipe");
        write_pipe(out, "segmentPipe", pipe)?;
    }
    if !rule.attributes.is_empty() {
        object.key(out, "attributes");
        write_attributes(out, "attributes", &rule.attributes)?;
    }
    if !rule.vertex_attributes.is_empty() {
        object.key(out, "vertexAttributes");
        write_attributes(out, "vertexAttributes", &rule.vertex_attributes)?;
    }
    if !rule.segment_attributes.is_empty() {
        object.key(out, "segmentAttributes");
        write_attributes(out, "segmentAttributes", &rule.segment_attributes)?;
    }
    object.close(out);
    Ok(())
}

/// The definitions a write holds, by the array each goes in, in name order.
struct Selection<'a> {
    linestyles: Vec<&'a LineStyle>,
    symbols: Vec<&'a LineStyle>,
}

fn selected<'a>(
    library: &'a StyleLibrary,
    options: &CustomisationWriteOptions,
) -> Result<Selection<'a>> {
    let only: BTreeSet<&str> = options.only.iter().map(String::as_str).collect();
    for &name in &only {
        let Some(definition) = library.find(name) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "the customisation has no definition of this name",
                in_quotes(name),
            ));
        };
        let wanted = if definition.symbol {
            options.symbols
        } else {
            options.linestyles
        };
        if !wanted {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "a definition asked for by name is a {} are not being written",
                    if definition.symbol {
                        "symbol, and symbols"
                    } else {
                        "linestyle, and linestyles"
                    }
                ),
                in_quotes(name),
            ));
        }
    }
    let mut selection = Selection {
        linestyles: Vec::new(),
        symbols: Vec::new(),
    };
    for name in library.names() {
        // in name order
        let Some(definition) = library.find(&name) else {
            continue;
        };
        if !only.is_empty() && !only.contains(name.as_str()) {
            continue;
        }
        if definition.symbol {
            if options.symbols {
                selection.symbols.push(definition);
            }
        } else if options.linestyles {
            selection.linestyles.push(definition);
        }
    }
    Ok(selection)
}

/// A top-level member: on a line of its own, two blanks in.
fn member(out: &mut Out, first: &mut bool, name: &'static str) {
    out.raw(if *first { "\n  \"" } else { ",\n  \"" });
    *first = false;
    out.raw(name);
    out.raw("\": ");
    out.member = name;
}

/// The entries of a list, one a line.
fn entry(out: &mut Out, index: usize) {
    out.raw(if index == 0 { "\n    " } else { ",\n    " });
}

fn write_definitions(
    out: &mut Out,
    first: &mut bool,
    array: &'static str,
    list: &[&LineStyle],
    customisation: &str,
) -> Status {
    if list.is_empty() {
        return Ok(());
    }
    member(out, first, array);
    out.raw("[");
    for (i, definition) in list.iter().enumerate() {
        out.entry = labelled(array, i, &definition.name);
        entry(out, i);
        write_definition(out, definition, customisation, "    ")?;
    }
    out.raw("\n  ]");
    out.entry = String::from(TOP_LEVEL);
    Ok(())
}

/// The layout, as docs/customisation.md states it: two blanks a level; the
/// top-level members one a line; the entries of the six lists one a line; a
/// definition's strokes one a line; everything else on its entry's line.
fn written(customisation: &Customisation, options: &CustomisationWriteOptions) -> Result<String> {
    check_header(customisation)?;
    let selection = selected(&customisation.library, options)?;

    let mut out = Out::new(TOP_LEVEL);
    // About what a line of each kind takes; only to save regrowing the text.
    let mut strokes = 0;
    customisation
        .library
        .for_each(|definition| strokes += definition.strokes.len());
    out.reserve(1024 + 32 * strokes + 160 * customisation.map.size());

    let mut first = true;
    out.raw("{");
    member(&mut out, &mut first, "format");
    out.text(CUSTOMISATION_FORMAT)?;
    member(&mut out, &mut first, "version");
    out.whole(i64::from(CUSTOMISATION_VERSION));
    member(&mut out, &mut first, "name");
    out.text(&customisation.name)?;
    if !customisation.description.is_empty() {
        member(&mut out, &mut first, "description");
        out.text(&customisation.description)?;
    }
    if !customisation.notice.is_empty() {
        member(&mut out, &mut first, "notice");
        out.raw("[");
        for (i, line) in customisation.notice.iter().enumerate() {
            entry(&mut out, i);
            out.text(line)?;
        }
        out.raw("\n  ]");
    }
    if !customisation.sources.is_empty() {
        let d = CustomisationSourceNote::default();
        member(&mut out, &mut first, "sources");
        out.raw("[");
        for (i, source) in customisation.sources.iter().enumerate() {
            out.entry = labelled("sources", i, &source.name);
            entry(&mut out, i);
            let mut object = Object::open(&mut out);
            object.key(&mut out, "name");
            out.text(&source.name)?;
            object.flag(&mut out, "definitions", source.definitions, d.definitions);
            object.flag(&mut out, "rules", source.rules, d.rules);
            if !source.notice.is_empty() {
                object.key(&mut out, "notice");
                write_lines(&mut out, &source.notice)?;
            }
            object.close(&mut out);
        }
        out.raw("\n  ]");
        out.entry = String::from(TOP_LEVEL);
    }
    if let Some(base) = &customisation.based_on {
        member(&mut out, &mut first, "basedOn");
        out.entry = String::from("basedOn");
        let mut object = Object::open(&mut out);
        object.key(&mut out, "name");
        out.text(&base.name)?;
        object.key(&mut out, "digest");
        out.text(&base.digest)?;
        object.close(&mut out);
        out.entry = String::from(TOP_LEVEL);
    }
    if !customisation.colours.empty() {
        member(&mut out, &mut first, "colours");
        out.raw("{");
        for (index, colour) in customisation.colours.entries().iter().enumerate() {
            entry(&mut out, index);
            out.text(&colour.name)?;
            out.raw(": ");
            out.text(&colour.colour.to_hex())?;
        }
        out.raw("\n  }");
    }
    // Every member of these two is written, default or not: they are
    // settings, and a file Katana wrote should not come to mean something
    // else because a default changed under it.
    if let Some(linework) = &customisation.linework {
        member(&mut out, &mut first, "linework");
        out.entry = String::from("linework");
        let mut object = Object::open(&mut out);
        for code in linework_code_members() {
            object.key(&mut out, code.name);
            out.text(code.spelling(linework))?;
        }
        object.close(&mut out);
        out.entry = String::from(TOP_LEVEL);
    }
    if let Some(automation) = &customisation.automation {
        member(&mut out, &mut first, "automation");
        let mut object = Object::open(&mut out);
        object.key(&mut out, "codesOnSurveyImport");
        out.flag(automation.codes_on_survey_import);
        object.key(&mut out, "lineworkOnSurveyImport");
        out.flag(automation.linework_on_survey_import);
        object.close(&mut out);
    }
    write_definitions(
        &mut out,
        &mut first,
        "linestyles",
        &selection.linestyles,
        &customisation.name,
    )?;
    write_definitions(
        &mut out,
        &mut first,
        "symbols",
        &selection.symbols,
        &customisation.name,
    )?;
    if options.codes && !customisation.map.empty() {
        member(&mut out, &mut first, "codes");
        out.raw("[");
        for (i, rule) in customisation.map.rules().iter().enumerate() {
            out.entry = labelled("codes", i, &rule.key);
            entry(&mut out, i);
            write_rule(&mut out, rule)?;
        }
        out.raw("\n  ]");
        out.entry = String::from(TOP_LEVEL);
    }
    out.raw("\n}\n");
    Ok(out.take())
}

// ---- reading ------------------------------------------------------------------------

/// One step from the root of a text to a place in it: a member, or a place in
/// a list.
#[derive(Clone, Debug, Default)]
struct Step {
    key: String,
    index: usize,
    indexed: bool,
}

/// A member given twice in one object, and where the object is.
struct Duplicate {
    path: Vec<Step>,
    member: String,
}

/// A number past the largest a double holds, at which the JSON reader stops:
/// where it is, and what the text wrote.
#[derive(Default)]
struct TooLarge {
    /// To the object the number is in - itself a member, or in a list under
    /// one - or, inside a stroke, to the stroke.
    path: Vec<Step>,
    member: String, // of that object; empty in a stroke
    stroke: bool,
    written: String, // "1e400"
    at: String,      // "line 2, column 45": of its last character
}

/// The first stroke that is not one: its place, and what is wrong with it.
struct Problem {
    index: usize,
    what: String,
    context: String,
}

/// The strokes of one definition, read while the text is parsed rather than
/// from a tree of it. A library is 35,000 strokes, and a tree holds each as a
/// list of its own with its word as text of its own - most of what a tree of
/// the whole file weighs, built only to be read once and thrown away.
#[derive(Default)]
struct StrokesRead {
    strokes: Vec<Stroke>,
    /// What each text stroke holds (a node of the tree, outside it), with the
    /// stroke's place. Read when the definition is (read_definition), where
    /// the entry it is in can be named.
    texts: Vec<(usize, usize)>,
    problem: Option<Problem>,
}

struct Parsed {
    tree: Tree,
    duplicate: Option<Duplicate>,
    /// By the number the tree holds where a definition's "strokes" would be.
    strokes: Vec<StrokesRead>,
    /// Set when the reading stopped at such a number: the tree is then only
    /// what the text held BEFORE it.
    too_large: Option<TooLarge>,
    /// Set when the reading stopped at the rule past the most a file holds
    /// (CUSTOMISATION_MOST_RULES): the tree is then only what the text held
    /// before it, as above.
    too_many_rules: bool,
}

/// Which text is being read: it decides where a list of strokes can be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextKind {
    File,
    Definition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// a list or an object being built into a tree
    Tree,
    /// a definition's list of strokes
    Strokes,
    /// one stroke of it
    Stroke,
}

struct Frame {
    role: Role,
    /// Tree: where it is being built (0, unused, for the other two roles).
    node: usize,
    object: bool,    // an object rather than a list
    aside: bool,     // Tree: the root of a tree built aside from the text's own
    key: String,     // an object: the member whose value comes next
    children: usize, // a list: its values so far
}

impl Frame {
    fn new(role: Role, node: usize, object: bool, aside: bool) -> Self {
        Self {
            role,
            node,
            object,
            aside,
            key: String::new(),
            children: 0,
        }
    }
}

/// One value of the stroke being read: a known kind word, kept as the word
/// alone, or a value as the text gave it (a node of the tree, outside it).
#[derive(Clone, Copy)]
enum StrokeValue {
    Word(&'static StrokeWord),
    Number { number: f64, written: usize },
    Other { written: usize },
}

/// Builds the tree of a text from the parse events, for two things a tree of
/// its own would not give:
///
/// * A member given twice is SEEN. nlohmann keeps the last of two equal keys
///   and says nothing, so a second "codes" list pasted at the end of a file
///   would replace 1,624 rules silently. Here the second is noticed as it is
///   put in, which is the only moment the first can still be seen.
/// * A definition's strokes never become a tree (StrokesRead).
struct TreeBuilder {
    kind: TextKind,
    tree: Tree,
    open: Vec<Frame>,
    values: Vec<StrokeValue>, // of the stroke being read
    strokes: Vec<StrokesRead>,
    duplicate: Option<Duplicate>,
    too_large: Option<TooLarge>,
    stopped_at: usize, // bytes of the text read when it stopped
    rules: usize,      // the objects of the file's "codes" list so far
    too_many_rules: bool,
    reason: String,
}

impl TreeBuilder {
    fn new(kind: TextKind) -> Self {
        Self {
            kind,
            tree: Tree::new(),
            open: Vec::new(),
            values: Vec::new(),
            strokes: Vec::new(),
            duplicate: None,
            too_large: None,
            stopped_at: 0,
            rules: 0,
            too_many_rules: false,
            reason: String::new(),
        }
    }

    fn take(self) -> Parsed {
        Parsed {
            tree: self.tree,
            duplicate: self.duplicate,
            strokes: self.strokes,
            too_large: self.too_large,
            too_many_rules: self.too_many_rules,
        }
    }

    fn in_stroke(&self) -> bool {
        self.open
            .last()
            .is_some_and(|frame| frame.role == Role::Stroke)
    }

    /// A value begins: in a list, it is the next one.
    fn child(&mut self) {
        if let Some(frame) = self.open.last_mut()
            && !frame.object
        {
            frame.children += 1;
        }
    }

    /// Where the innermost of the first `count` open containers is: each of
    /// them but the root, by where it sits in its parent - and no further than
    /// a stroke. A stroke is the smallest place a refusal names (`symbols[0]
    /// "TEST Valve" strokes[9]`); what is wrong inside one, in a text stroke's
    /// object, is said of that stroke.
    fn path(&self, count: usize) -> Vec<Step> {
        let mut steps = Vec::new();
        for pair in self.open[..count].windows(2) {
            let (parent, child) = (&pair[0], &pair[1]);
            steps.push(if parent.object {
                Step {
                    key: parent.key.clone(),
                    index: 0,
                    indexed: false,
                }
            } else {
                Step {
                    key: String::new(),
                    index: parent.children.saturating_sub(1),
                    indexed: true,
                }
            });
            if child.role == Role::Stroke {
                break;
            }
        }
        steps
    }

    /// Where the value now being read is, as a refusal names a place: the
    /// object it is a member of - or, when it is in a list, the object that
    /// list is a member of - with that member; or the stroke it is a value
    /// of.
    fn here(&self) -> TooLarge {
        let mut place = TooLarge::default();
        for (i, frame) in self.open.iter().enumerate().rev() {
            if frame.object {
                place.member.clone_from(&frame.key);
                place.path = self.path(i + 1);
                return place;
            }
            if frame.role == Role::Stroke {
                place.stroke = true;
                place.path = self.path(i + 1);
                return place;
            }
        }
        // In no object at all: lists alone, or the text is the value itself.
        place.path = self.path(self.open.len());
        place
    }

    /// Puts a value where the innermost open list or object wants it, and
    /// says where it now is.
    fn place(&mut self, value: Node) -> usize {
        let at = self.tree.push(value);
        let Some(parent) = self.open.last() else {
            self.tree.root = at;
            return at;
        };
        let node = parent.node;
        if !parent.object {
            if let Node::Array(items) = &mut self.tree.nodes[node] {
                items.push(at);
            }
            return at;
        }
        let key = parent.key.clone();
        let given =
            matches!(&self.tree.nodes[node], Node::Object(members) if members.contains_key(&key));
        if given && self.duplicate.is_none() {
            self.duplicate = Some(Duplicate {
                path: self.path(self.open.len()),
                member: key.clone(),
            });
        }
        // a second value replaces the first: refused later, once the text has
        // been read to its end
        if let Node::Object(members) = &mut self.tree.nodes[node] {
            members.insert(key, at);
        }
        at
    }

    fn scalar(&mut self, value: Node) -> bool {
        self.child();
        match self.open.last().map(|frame| frame.role) {
            None | Some(Role::Tree) => {
                self.place(value);
            }
            Some(Role::Stroke) => {
                let written = self.tree.push(value);
                self.values.push(StrokeValue::Other { written });
            }
            Some(Role::Strokes) => {
                // a bare value where a stroke should be
                let written = self.tree.push(value);
                self.not_a_stroke(written);
            }
        }
        true
    }

    fn number(&mut self, written: Node, value: f64) -> bool {
        if !self.in_stroke() {
            return self.scalar(written);
        }
        self.child();
        let written = self.tree.push(written);
        self.values.push(StrokeValue::Number {
            number: value,
            written,
        });
        true
    }

    fn begin(&mut self, object: bool) -> bool {
        self.child();
        let role = self.open.last().map(|frame| frame.role);
        if let Some(role) = role
            && role != Role::Tree
        {
            if role == Role::Strokes && !object {
                self.open.push(Frame::new(Role::Stroke, 0, false, false));
                self.values.clear();
                return true;
            }
            // An object where a stroke should be, or an object or a list
            // inside a stroke (a text's object is one): built aside, and
            // handed over whole when it closes.
            let aside = self.tree.push(if object {
                Node::Object(BTreeMap::new())
            } else {
                Node::Array(Vec::new())
            });
            self.open.push(Frame::new(Role::Tree, aside, object, true));
            return true;
        }
        if !object && self.begins_strokes() {
            // The tree holds, where the strokes would be, which entry of
            // `strokes` they are.
            let which = self.strokes.len();
            self.place(Node::Strokes(which));
            self.strokes.push(StrokesRead::default());
            self.open.push(Frame::new(Role::Strokes, 0, false, false));
            return true;
        }
        if object && self.is_rule_of_a_file() {
            self.rules += 1;
            if self.rules > CUSTOMISATION_MOST_RULES {
                // Stops the parse here: the rules past the bound are never
                // held, so a file of millions costs no more than the bound
                // does.
                self.too_many_rules = true;
                return false;
            }
        }
        let node = self.place(if object {
            Node::Object(BTreeMap::new())
        } else {
            Node::Array(Vec::new())
        });
        self.open.push(Frame::new(Role::Tree, node, object, false));
        true
    }

    fn end(&mut self) -> bool {
        let Some(frame) = self.open.pop() else {
            return true;
        };
        if frame.role == Role::Stroke {
            self.finish_stroke();
        } else if frame.role == Role::Tree && frame.aside {
            if self.in_stroke() {
                self.values.push(StrokeValue::Other {
                    written: frame.node,
                });
            } else {
                self.not_a_stroke(frame.node);
            }
        }
        true
    }

    /// An object that is an entry of a file's "codes" list: a rule.
    fn is_rule_of_a_file(&self) -> bool {
        self.kind == TextKind::File
            && self.open.len() == 2
            && self.open[0].object
            && self.open[0].key == "codes"
            && !self.open[1].object
    }

    /// In a file, the "strokes" of an entry of "linestyles" or "symbols"; in a
    /// definition's own text, its "strokes". Anywhere else a member of that
    /// name is built into the tree like any other, and refused there.
    fn begins_strokes(&self) -> bool {
        if self.kind == TextKind::Definition {
            return self.open.len() == 1 && self.open[0].object && self.open[0].key == "strokes";
        }
        self.open.len() == 3
            && self.open[0].object
            && (self.open[0].key == "linestyles" || self.open[0].key == "symbols")
            && !self.open[1].object
            && self.open[2].object
            && self.open[2].key == "strokes"
    }

    /// The stroke being read as the text wrote it, for a refusal to show:
    /// written straight from its values, never deeper than it shows.
    fn shown_stroke(&self) -> String {
        let mut text = String::from("[");
        for (i, value) in self.values.iter().enumerate() {
            if text.len() > LONGEST_SHOWN {
                break;
            }
            if i > 0 {
                text.push(',');
            }
            match *value {
                StrokeValue::Word(word) => text.push_str(&in_quotes(word.word)),
                StrokeValue::Number { written, .. } | StrokeValue::Other { written } => {
                    append_shown(&mut text, self.tree.at(written));
                }
            }
        }
        text.push(']');
        cut_short(text)
    }

    /// The first problem of a definition's strokes is the one a refusal names;
    /// the list frame is innermost when this is called.
    fn problem(&mut self, what: String, context: String) {
        let index = self
            .open
            .last()
            .map_or(0, |frame| frame.children)
            .saturating_sub(1);
        if let Some(read) = self.strokes.last_mut()
            && read.problem.is_none()
        {
            read.problem = Some(Problem {
                index,
                what,
                context,
            });
        }
    }

    fn not_a_stroke(&mut self, written: usize) {
        let context = shown(self.tree.at(written));
        self.problem(String::from(NOT_A_STROKE), context);
    }

    /// One stroke's values are in: the stroke they make, or what is wrong.
    fn finish_stroke(&mut self) {
        let Some(read) = self.strokes.last() else {
            return;
        };
        if read.problem.is_some() {
            return; // nothing after the first problem is kept
        }
        if read.strokes.len() >= CUSTOMISATION_MOST_STROKES {
            self.problem(String::from(TOO_MANY_STROKES), String::new());
            return;
        }
        let kind = match self.values.first().copied() {
            Some(StrokeValue::Word(word)) => word,
            Some(StrokeValue::Other { written }) if self.tree.at(written).is_string() => {
                let known = STROKE_WORDS
                    .iter()
                    .map(|entry| entry.word)
                    .collect::<Vec<_>>()
                    .join(", ");
                let what = format!(
                    "{} is not a kind of stroke, which is one of {known}",
                    in_quotes(self.tree.at(written).as_str())
                );
                self.problem(what, String::new());
                return;
            }
            _ => {
                let context = self.shown_stroke();
                self.problem(String::from(NOT_A_STROKE), context);
                return;
            }
        };
        // Built only for a refusal: there are tens of thousands of strokes.
        let takes = || format!("{} takes {}", in_quotes(kind.word), kind.takes);
        if self.values.len() != kind.values + 1 {
            let what = format!(
                "{} ({} after the word) and has {}",
                takes(),
                kind.values,
                self.values.len() - 1
            );
            let context = self.shown_stroke();
            self.problem(what, context);
            return;
        }

        let mut stroke = Stroke {
            op: kind.op,
            ..Stroke::default()
        };
        match kind.op {
            StrokeOp::Move | StrokeOp::Draw | StrokeOp::Arc | StrokeOp::Circle | StrokeOp::Dot => {
                let numbers: Option<Vec<f64>> = self.values[1..]
                    .iter()
                    .map(|value| match *value {
                        StrokeValue::Number { number, .. } => Some(number),
                        _ => None,
                    })
                    .collect();
                let Some(numbers) = numbers else {
                    // A number too small to hold is among the values as what
                    // it is, and is said to be that.
                    let lost = self.values[1..].iter().any(|value| {
                        matches!(*value, StrokeValue::Other { written } if self.tree.at(written).is_lost())
                    });
                    let what = if lost {
                        format!("this stroke {TOO_SMALL}")
                    } else {
                        format!("{}, each a number", takes())
                    };
                    let context = self.shown_stroke();
                    self.problem(what, context);
                    return;
                };
                if numbers.iter().any(|&number| !within_magnitude(number)) {
                    let what = format!("{} has a number {BEYOND_MAGNITUDE}", in_quotes(kind.word));
                    let context = self.shown_stroke();
                    self.problem(what, context);
                    return;
                }
                if kind.op == StrokeOp::Arc && !within_sweep(numbers[1], numbers[2]) {
                    let what = format!("{} {SWEEPS_TOO_FAR}", in_quotes(kind.word));
                    let context = self.shown_stroke();
                    self.problem(what, context);
                    return;
                }
                if kind.op == StrokeOp::Move || kind.op == StrokeOp::Draw {
                    stroke.point = Point2::new(numbers[0], numbers[1]);
                } else {
                    stroke.radius = numbers[0];
                    if kind.op == StrokeOp::Arc {
                        stroke.start_angle = numbers[1];
                        stroke.end_angle = numbers[2];
                    }
                }
            }
            StrokeOp::Pen => {
                let pen = match self.values[1] {
                    StrokeValue::Other { written } => match self.tree.at(written).node() {
                        Node::Text(text) => Some(text.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                let Some(pen) = pen else {
                    let what = format!("{}, as text in double quotes", takes());
                    let context = self.shown_stroke();
                    self.problem(what, context);
                    return;
                };
                if pen.len() > CUSTOMISATION_MOST_TEXT_BYTES {
                    let what = format!("{} is {TOO_LONG}", in_quotes(kind.word));
                    let context = self.shown_stroke();
                    self.problem(what, context);
                    return;
                }
                stroke.pen = pen;
            }
            StrokeOp::Text => {
                // Kept beside the strokes in the model and inside the stroke
                // in the file, where a person looks for it: so the i-th text
                // stroke names text i, the only arrangement the writer accepts
                // back.
                let index = self
                    .open
                    .last()
                    .map_or(0, |frame| frame.children)
                    .saturating_sub(1);
                let second = self.values[1];
                let written = match second {
                    StrokeValue::Number { written, .. } | StrokeValue::Other { written } => written,
                    StrokeValue::Word(_) => self.tree.push(Node::Null),
                };
                if let Some(read) = self.strokes.last_mut() {
                    stroke.text = read.texts.len();
                    read.texts.push((index, written));
                }
            }
        }
        if let Some(read) = self.strokes.last_mut() {
            read.strokes.push(stroke);
        }
    }
}

impl Sax for TreeBuilder {
    fn null(&mut self) -> bool {
        self.scalar(Node::Null)
    }

    fn boolean(&mut self, value: bool) -> bool {
        self.scalar(Node::Boolean(value))
    }

    fn number_integer(&mut self, value: i64) -> bool {
        self.number(Node::Integer(value), value as f64)
    }

    fn number_unsigned(&mut self, value: u64) -> bool {
        self.number(Node::Unsigned(value), value as f64)
    }

    fn number_float(&mut self, value: f64, written: &str) -> bool {
        // A number below the smallest a double holds comes from the reader as
        // 0, with nothing said. But 0 is not what the file wrote, and here 0
        // has meanings of its own - a length "not said", a size "the
        // definition's own" - so it is kept as what it is, a number this
        // cannot hold, and refused where it is read.
        if value == 0.0 && says_more_than_zero(written) {
            return self.scalar(Node::Lost(String::from(written)));
        }
        self.number(Node::Float(value), value)
    }

    fn string(&mut self, value: &mut String) -> bool {
        // A stroke's first value is its kind, one of seven words: known by
        // looking at it, with no copy made.
        if self.in_stroke()
            && self.values.is_empty()
            && let Some(word) = stroke_word_of_text(value)
        {
            self.child();
            self.values.push(StrokeValue::Word(word));
            return true;
        }
        self.scalar(Node::Text(std::mem::take(value)))
    }

    fn start_object(&mut self) -> bool {
        self.begin(true)
    }

    fn key(&mut self, value: &mut String) -> bool {
        if let Some(frame) = self.open.last_mut() {
            frame.key = std::mem::take(value);
        }
        true
    }

    fn end_object(&mut self) -> bool {
        self.end()
    }

    fn start_array(&mut self) -> bool {
        self.begin(false)
    }

    fn end_array(&mut self) -> bool {
        self.end()
    }

    fn parse_error(&mut self, position: usize, token: &str, error: &JsonError) -> bool {
        self.reason.clone_from(&error.what);
        // The one thing the reader refuses that IS JSON: a number past the
        // largest double ("number overflow", nlohmann's error 406). Its place
        // is kept, because what follows it will never be read.
        const NUMBER_OVERFLOW: u32 = 406;
        if error.id == NUMBER_OVERFLOW {
            let mut place = self.here();
            place.written = String::from(token);
            self.too_large = Some(place);
            self.stopped_at = position;
        }
        false
    }
}

/// "line 2, column 45" for the last of the first `read` bytes of `text`: where
/// the JSON reader stands when it has read that far, counted as it counts for
/// its own messages - lines from 1, and the column of the last byte read.
fn line_and_column(text: &str, read: usize) -> String {
    let before = &text.as_bytes()[..read.min(text.len())];
    let line = 1 + before.iter().filter(|&&byte| byte == b'\n').count();
    let column = before.len()
        - before
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map_or(0, |line_break| line_break + 1);
    format!("line {line}, column {column}")
}

/// The bytes of a text as JSON. Anything that is not JSON is `not_this` - the
/// one message a caller's own kind of text gets - with the reason, and for
/// malformed JSON the line and column, as the context. The one exception comes
/// back in what is returned, for the caller to refuse: a number too large to
/// hold (Parsed::too_large), where what the text is depends on how far it had
/// been read.
fn parse_json(bytes: &[u8], not_this: &str, kind: TextKind) -> Result<Parsed> {
    // Decoded first: the JSON reader steps over a UTF-8 byte order mark and
    // nothing else, and an editor on Windows saves UTF-16 readily.
    //
    // A byte order mark over bytes that are not what it promises - a file
    // cut short, or damaged - is still bytes that are neither UTF-8 nor
    // UTF-16, and so the same message as those below, with the decoder's
    // account of what is wrong beside it.
    let decoded = decode_text(bytes)
        .map_err(|error| make_error(ErrorCode::ParseFailure, not_this, error.message))?;
    // Bytes that are neither UTF-8 nor UTF-16 are decoded as Windows-1252 for
    // the formats that never said what they are. This one does say: it is
    // JSON, and JSON is UTF-8. A guess at a code page would put a wrong
    // character into a name silently, and a name here is an identity.
    if decoded.encoding == TextEncoding::Windows1252 {
        return Err(make_error(
            ErrorCode::ParseFailure,
            not_this,
            "the bytes are not UTF-8 text",
        ));
    }
    // A file of another program's format begins with '<' (markup) or "//" (a
    // comment), neither of which JSON can begin with. Told apart by those two
    // characters and nothing more - the readers of those formats are the
    // converter's - so that the person is sent there and is not shown the
    // parser's account of a character it did not expect.
    if kind == TextKind::File {
        let first = decoded.text.trim_start_matches([' ', '\t', '\n', '\r']);
        if first.starts_with('<') || first.starts_with("//") {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                format!(
                    "{not_this}; the older formats are converted with katana_customisation_convert (a developer's tool, not installed; see docs/customisation.md)"
                ),
            ));
        }
    }
    let mut builder = TreeBuilder::new(kind);
    if !sax_parse(decoded.text.as_bytes(), &mut builder) {
        if builder.too_many_rules {
            return Ok(builder.take());
        }
        if builder.too_large.is_some() {
            let at = line_and_column(&decoded.text, builder.stopped_at);
            let mut text = builder.take();
            if let Some(number) = &mut text.too_large {
                number.at = at;
            }
            return Ok(text);
        }
        // "[json.exception.parse_error.101] parse error at line 3, column 7:
        // ...": the bracketed id is the library's and says nothing to a
        // person.
        let mut reason = builder.reason;
        if reason.starts_with('[')
            && let Some(id) = reason.find("] ")
        {
            reason.replace_range(..id + 2, "");
        }
        const LONGEST: usize = 240;
        if reason.len() > LONGEST {
            // The C++ cuts the bytes; a cut inside a character is shown as
            // U+FFFD here, since the context is text.
            let mut cut = String::from_utf8_lossy(&reason.as_bytes()[..LONGEST]).into_owned();
            cut.push_str("...");
            reason = cut;
        }
        return Err(make_error(ErrorCode::ParseFailure, not_this, reason));
    }
    Ok(builder.take())
}

/// Where something noticed while the text was parsed is - a member given
/// twice, a number too large to hold - in the words every other refusal uses.
/// `base` names the root when it is an entry itself (a definition read on its
/// own).
fn place_of(tree: &Tree, path: &[Step], base: &str) -> String {
    let mut label = String::from(base);
    if path.is_empty() {
        return if label.is_empty() {
            String::from(TOP_LEVEL)
        } else {
            label
        };
    }
    let mut at = Some(tree.root());
    for (i, step) in path.iter().enumerate() {
        if !step.indexed {
            if !label.is_empty() {
                label.push(' ');
            }
            label.push_str(&step.key);
            at = at.and_then(|value| value.find(&step.key));
            continue;
        }
        label.push_str(&format!("[{}]", step.index));
        at = at.and_then(|value| value.elements().nth(step.index));
        // An entry of one of the file's lists is shown with its own name.
        if base.is_empty()
            && i == 1
            && !path[0].indexed
            && let Some(entry) = at
            && entry.is_object()
        {
            let name_member = name_member_of(&path[0].key);
            if !name_member.is_empty()
                && let Some(name) = entry.find(name_member)
                && name.is_string()
            {
                label.push(' ');
                label.push_str(&in_quotes(name.as_str()));
            }
        }
    }
    label
}

fn refuse_duplicate(tree: &Tree, duplicate: Option<&Duplicate>, base: &str) -> Status {
    if let Some(duplicate) = duplicate {
        return Err(refuse(
            ErrorCode::ParseFailure,
            &place_of(tree, &duplicate.path, base),
            &format!(
                "member {} is given twice, and only one of them could be kept",
                in_quotes(&duplicate.member)
            ),
            "",
        ));
    }
    Ok(())
}

/// What a refusal shows beside a number too large to hold: the number, and
/// where in the text it is.
fn written_at(number: &TooLarge) -> String {
    format!("{} at {}", number.written, number.at)
}

/// Refuses the number too large to hold that the reading of a text stopped
/// at, by the entry and the member it is in, as a value of the wrong kind is
/// refused.
fn refuse_too_large(tree: &Tree, number: &TooLarge, base: &str) -> Error {
    let subject = if number.stroke {
        String::from("this stroke")
    } else {
        in_quotes(&number.member)
    };
    refuse(
        ErrorCode::ParseFailure,
        &place_of(tree, &number.path, base),
        &format!("{subject} {TOO_LARGE}"),
        written_at(number),
    )
}

/// One object of the file being read. Every member is taken through here by
/// name, and that is what makes a member KNOWN: finish() refuses whatever the
/// object holds that nothing asked for. There is no separate list of allowed
/// names to fall out of step with the code that reads them.
struct Members<'a> {
    json: Json<'a>,
    label: String,
    asked: Vec<&'static str>,
    found: usize,
}

impl<'a> Members<'a> {
    // A rule has the most members, nineteen; room for them at once saves the
    // list growing five times for every rule read.
    const MOST_MEMBERS: usize = 24;

    fn new(json: Json<'a>, label: String) -> Result<Self> {
        if !json.is_object() {
            return Err(refuse(
                ErrorCode::ParseFailure,
                &label,
                "must be an object, written {...}",
                shown(json),
            ));
        }
        Ok(Self {
            json,
            label,
            asked: Vec::with_capacity(Self::MOST_MEMBERS),
            found: 0,
        })
    }

    /// The entry this object is, as a refusal names it.
    fn label(&self) -> &str {
        &self.label
    }

    /// The member, or None when the object does not have it.
    fn find(&mut self, name: &'static str) -> Option<Json<'a>> {
        self.asked.push(name);
        let found = self.json.find(name)?;
        self.found += 1;
        Some(found)
    }

    fn required(&mut self, name: &'static str) -> Result<Json<'a>> {
        match self.find(name) {
            Some(value) => Ok(value),
            None => Err(refuse(
                ErrorCode::ParseFailure,
                &self.label,
                &format!("has no {}, which it must", in_quotes(name)),
                "",
            )),
        }
    }

    fn text_of(&self, name: &str, value: Json<'a>) -> Result<String> {
        let Node::Text(text) = value.node() else {
            return Err(self.wrong(name, "must be text, in double quotes", value));
        };
        if text.len() > CUSTOMISATION_MOST_TEXT_BYTES {
            return Err(self.wrong(name, &format!("is {TOO_LONG}"), value));
        }
        Ok(text.clone())
    }

    fn text(&mut self, name: &'static str, fallback: &str) -> Result<String> {
        match self.find(name) {
            Some(value) => self.text_of(name, value),
            None => Ok(String::from(fallback)),
        }
    }

    fn required_text(&mut self, name: &'static str) -> Result<String> {
        let value = self.required(name)?;
        self.text_of(name, value)
    }

    fn real(&mut self, name: &'static str, fallback: f64) -> Result<f64> {
        let Some(value) = self.find(name) else {
            return Ok(fallback);
        };
        if value.is_lost() {
            return Err(self.wrong(name, TOO_SMALL, value));
        }
        if !value.is_number() {
            return Err(self.wrong(name, "must be a number", value));
        }
        if !within_magnitude(value.as_f64()) {
            return Err(self.wrong(name, &format!("is {BEYOND_MAGNITUDE}"), value));
        }
        Ok(value.as_f64())
    }

    /// A whole number: 2.7 would otherwise be cut to 2 and `true` read as 1,
    /// each some other value than the file gave.
    fn whole(&mut self, name: &'static str, fallback: i32) -> Result<i32> {
        let Some(value) = self.find(name) else {
            return Ok(fallback);
        };
        if !value.is_number_integer() {
            return Err(self.wrong(name, "must be a whole number", value));
        }
        let fits = if value.is_number_unsigned() {
            value.as_u64() <= i32::MAX as u64
        } else {
            value.as_i64() >= i64::from(i32::MIN)
        };
        if !fits {
            return Err(self.wrong(name, "is a whole number too large to hold", value));
        }
        Ok(value.as_i64() as i32)
    }

    fn optional_flag(&mut self, name: &'static str) -> Result<Option<bool>> {
        let Some(value) = self.find(name) else {
            return Ok(None);
        };
        if !value.is_boolean() {
            return Err(self.wrong(name, "must be true or false", value));
        }
        Ok(Some(value.as_bool()))
    }

    fn flag(&mut self, name: &'static str, fallback: bool) -> Result<bool> {
        Ok(self.optional_flag(name)?.unwrap_or(fallback))
    }

    fn optional_word<E: Copy>(
        &mut self,
        name: &'static str,
        words: &[Word<E>],
    ) -> Result<Option<E>> {
        let Some(value) = self.find(name) else {
            return Ok(None);
        };
        if let Node::Text(given) = value.node()
            && let Some(entry) = words.iter().find(|entry| entry.word == given.as_str())
        {
            return Ok(Some(entry.value));
        }
        Err(refuse(
            ErrorCode::ParseFailure,
            &self.label,
            &format!(
                "{} is {}, which is not one of {}",
                in_quotes(name),
                shown(value),
                listed(words)
            ),
            "",
        ))
    }

    fn word<E: Copy>(&mut self, name: &'static str, words: &[Word<E>], fallback: E) -> Result<E> {
        Ok(self.optional_word(name, words)?.unwrap_or(fallback))
    }

    fn wrong(&self, name: &str, rule: &str, value: Json<'_>) -> Error {
        refuse(
            ErrorCode::ParseFailure,
            &self.label,
            &format!("{} {rule}", in_quotes(name)),
            shown(value),
        )
    }

    /// After every member has been asked for: anything left is unknown. It is
    /// refused rather than skipped because a customisation is edited by hand,
    /// and "linesytle" skipped is a rule that silently draws nothing - and is
    /// no longer in the file once Katana has written it back.
    fn finish(&self) -> Status {
        if self.found == self.json.size() {
            return Ok(());
        }
        for (key, _) in self.json.members() {
            if !self.asked.contains(&key) {
                return Err(refuse(
                    ErrorCode::ParseFailure,
                    &self.label,
                    &format!("unknown member {}", in_quotes(key)),
                    format!("the members here are {}", self.asked.join(", ")),
                ));
            }
        }
        Ok(())
    }
}

fn point_of(json: Json<'_>, place: &str, member: &str) -> Result<Point2> {
    let items: Vec<Json<'_>> = json.elements().collect();
    if json.is_array() && items.len() == 2 && (items[0].is_lost() || items[1].is_lost()) {
        return Err(refuse(
            ErrorCode::ParseFailure,
            place,
            &format!("{} {TOO_SMALL}", in_quotes(member)),
            shown(json),
        ));
    }
    if !json.is_array() || items.len() != 2 || !items[0].is_number() || !items[1].is_number() {
        return Err(refuse(
            ErrorCode::ParseFailure,
            place,
            &format!("{} must be a point, written [x, y]", in_quotes(member)),
            shown(json),
        ));
    }
    if !within_magnitude(items[0].as_f64()) || !within_magnitude(items[1].as_f64()) {
        return Err(refuse(
            ErrorCode::ParseFailure,
            place,
            &format!("{} is {BEYOND_MAGNITUDE}", in_quotes(member)),
            shown(json),
        ));
    }
    Ok(Point2::new(items[0].as_f64(), items[1].as_f64()))
}

fn lines_of(json: Json<'_>, place: &str, member: &str) -> Result<Vec<String>> {
    let bad = || {
        refuse(
            ErrorCode::ParseFailure,
            place,
            &format!(
                "{} must be lines of text, written [\"...\", \"...\"]",
                in_quotes(member)
            ),
            shown(json),
        )
    };
    if !json.is_array() {
        return Err(bad());
    }
    let mut lines = Vec::new();
    for line in json.elements() {
        let Node::Text(text) = line.node() else {
            return Err(bad());
        };
        if text.len() > CUSTOMISATION_MOST_TEXT_BYTES {
            return Err(refuse(
                ErrorCode::ParseFailure,
                place,
                &format!("{} holds a line that is {TOO_LONG}", in_quotes(member)),
                shown(line),
            ));
        }
        lines.push(text.clone());
    }
    Ok(lines)
}

/// A list of the file: an array, or the refusal that says so.
fn list_of<'a>(json: Json<'a>, place: &str, member: &str) -> Result<Json<'a>> {
    if !json.is_array() {
        return Err(refuse(
            ErrorCode::ParseFailure,
            place,
            &format!("{} must be a list, written [...]", in_quotes(member)),
            shown(json),
        ));
    }
    Ok(json)
}

fn read_stroke_text(json: Json<'_>, place: &str) -> Result<StrokeText> {
    let d = StrokeText::default();
    let mut members = Members::new(json, String::from(place))?;
    let mut text = StrokeText {
        text: members.text("text", &d.text)?,
        angle: members.real("angle", d.angle)?,
        height: members.real("height", d.height)?,
        justify: members.text("justify", &d.justify)?,
        font: members.text("font", &d.font)?,
        width_factor: members.real("widthFactor", d.width_factor)?,
        unnamed: d.unnamed,
    };
    if let Some(extra) = members.find("extra") {
        let items: Vec<Json<'_>> = extra.elements().collect();
        let three = extra.is_array() && items.len() == text.unnamed.len();
        if three && items.iter().any(|item| item.is_lost()) {
            return Err(members.wrong("extra", TOO_SMALL, extra));
        }
        if !three || !items.iter().all(|item| item.is_number()) {
            return Err(members.wrong("extra", "must be three numbers, written [a, b, c]", extra));
        }
        for (slot, item) in text.unnamed.iter_mut().zip(&items) {
            *slot = item.as_f64();
            if !within_magnitude(*slot) {
                return Err(members.wrong(
                    "extra",
                    &format!("has a number {BEYOND_MAGNITUDE}"),
                    extra,
                ));
            }
        }
    }
    members.finish()?;
    Ok(text)
}

/// One definition. `symbol` is the array it sits in and `customisation` the
/// name it sits under, which is its source unless it says "from";
/// `strokes_read` is where the parse left every definition's strokes.
fn read_definition(
    json: Json<'_>,
    place: &str,
    symbol: bool,
    customisation: &str,
    strokes_read: &mut [StrokesRead],
) -> Result<LineStyle> {
    let d = LineStyle::default();
    let mut members = Members::new(json, String::from(place))?;
    let name = members.required_text("name")?;
    let group = members.text("group", &d.group)?;
    let units = members.word("units", &UNITS_WORDS, d.units)?;
    let at_vertices = members.flag("atVertices", d.at_vertices)?;
    let length = members.real("length", d.length)?;
    let factor = members.real("factor", d.factor)?;
    let origin = members
        .find("origin")
        .map(|origin| point_of(origin, place, "origin"))
        .transpose()?
        .unwrap_or(d.origin);
    let (anchor1, anchor2) = match members.find("anchors") {
        Some(anchors) => {
            let items: Vec<Json<'_>> = anchors.elements().collect();
            if !anchors.is_array() || items.len() != 2 {
                return Err(members.wrong(
                    "anchors",
                    "must be two points, written [[x, y], [x, y]]",
                    anchors,
                ));
            }
            (
                point_of(items[0], place, "anchors")?,
                point_of(items[1], place, "anchors")?,
            )
        }
        None => (d.anchor1, d.anchor2),
    };
    let stretch_mode = members.whole("stretchMode", d.stretch_mode)?;
    let cycle_mode = members.whole("cycleMode", d.cycle_mode)?;
    let source = match members.find("from") {
        Some(from) => members.text_of("from", from)?,
        // an absent "from" is the name the definition sits under
        None => String::from(customisation),
    };
    let strokes = members.find("strokes");
    members.finish()?;
    let mut definition = LineStyle {
        name,
        group,
        units,
        at_vertices,
        length,
        factor,
        origin,
        anchor1,
        anchor2,
        stretch_mode,
        cycle_mode,
        strokes: Vec::new(),
        texts: Vec::new(),
        source,
        symbol,
    };
    if let Some(strokes) = strokes {
        // A list of strokes was read as the parse went by, and the tree holds
        // only which one it was; anything else here is not a list.
        let Node::Strokes(which) = *strokes.node() else {
            return Err(members.wrong("strokes", "must be a list, written [...]", strokes));
        };
        let read = &mut strokes_read[which];
        // In the order of the strokes, so that the first thing wrong is the
        // one reported: the texts before the first stroke that is not one,
        // then that stroke.
        definition.texts.reserve(read.texts.len());
        for &(index, object) in &read.texts {
            if read
                .problem
                .as_ref()
                .is_some_and(|problem| index > problem.index)
            {
                break;
            }
            definition.texts.push(read_stroke_text(
                json.tree.at(object),
                &placed(place, "strokes", index),
            )?);
        }
        if let Some(problem) = &read.problem {
            return Err(refuse(
                ErrorCode::ParseFailure,
                &placed(place, "strokes", problem.index),
                &problem.what,
                problem.context.as_str(),
            ));
        }
        definition.strokes = std::mem::take(&mut read.strokes);
    }
    check_source(&definition, place)?;
    Ok(definition)
}

fn read_attributes(json: Json<'_>, place: &str, member: &str) -> Result<Vec<SurveyAttribute>> {
    let mut attributes = Vec::new();
    for (index, item) in list_of(json, place, member)?.elements().enumerate() {
        let mut members = Members::new(item, placed(place, member, index))?;
        // Text in the model, and only these two words here: a type this did
        // not know would be an attribute nothing can set.
        let given = members.required("type")?;
        let kind = match given.node() {
            Node::Text(text) if ATTRIBUTE_TYPES.contains(&text.as_str()) => text.clone(),
            _ => {
                return Err(refuse(
                    ErrorCode::ParseFailure,
                    members.label(),
                    &format!(
                        "\"type\" is {}, which is not one of text, integer",
                        shown(given)
                    ),
                    "",
                ));
            }
        };
        let name = members.required_text("name")?;
        let value = members.text("value", "")?;
        members.finish()?;
        attributes.push(SurveyAttribute {
            r#type: kind,
            name,
            value,
        });
    }
    Ok(attributes)
}

fn read_pipe(json: Json<'_>, place: String) -> Result<SurveyPipe> {
    let d = SurveyPipe::default();
    let mut members = Members::new(json, place)?;
    let pipe = SurveyPipe {
        justify: members.text("justify", &d.justify)?,
        shape: members.text("shape", &d.shape)?,
        size1: members.text("size1", &d.size1)?,
        size2: members.text("size2", &d.size2)?,
        active: members.flag("active", d.active)?,
    };
    members.finish()?;
    Ok(pipe)
}

fn read_symbol(json: Json<'_>, place: String) -> Result<SurveySymbol> {
    let s = SurveySymbol::default();
    let mut part = Members::new(json, place)?;
    let symbol = SurveySymbol {
        style: part.text("name", &s.style)?,
        colour: part.text("colour", &s.colour)?,
        size: part.real("size", s.size)?,
        rotation: part.real("rotation", s.rotation)?,
        offset: part.real("offset", s.offset)?,
        raise: part.real("raise", s.raise)?,
    };
    part.finish()?;
    Ok(symbol)
}

fn read_text_style(json: Json<'_>, place: String) -> Result<SurveyTextStyle> {
    let t = SurveyTextStyle::default();
    let mut part = Members::new(json, place)?;
    let style = SurveyTextStyle {
        textstyle: part.text("style", &t.textstyle)?,
        colour: part.text("colour", &t.colour)?,
        r#type: part.text("units", &t.r#type)?,
        size: part.real("size", t.size)?,
        justify_x: part.text("justifyX", &t.justify_x)?,
        justify_y: part.text("justifyY", &t.justify_y)?,
        offset: part.real("offset", t.offset)?,
        raise: part.real("raise", t.raise)?,
        angle: part.real("angle", t.angle)?,
        slant: part.real("slant", t.slant)?,
        width_factor: part.real("widthFactor", t.width_factor)?,
        underline: part.flag("underline", t.underline)?,
        strikeout: part.flag("strikeout", t.strikeout)?,
        italic: part.flag("italic", t.italic)?,
        weight: part.text("weight", &t.weight)?,
    };
    part.finish()?;
    Ok(style)
}

fn read_rule(json: Json<'_>, place: &str) -> Result<SurveyRule> {
    let d = SurveyRule::default();
    let mut members = Members::new(json, String::from(place))?;
    let key = members.required_text("key")?;
    // Required, with no default: a rule's identity in a merge is what it sets
    // and its key, so a symbol rule read as the default would replace that
    // key's layer and colour rule.
    let Some(section) = members.optional_word("sets", &SECTION_WORDS)? else {
        return Err(refuse(
            ErrorCode::ParseFailure,
            place,
            "has no \"sets\", which it must",
            format!("it is one of {}", listed(&SECTION_WORDS)),
        ));
    };
    let model = members.text("layer", &d.model)?;
    let colour = members.text("colour", &d.colour)?;
    let breakline = members.optional_word("draw", &DRAW_WORDS)?;
    let linestyle = members.text("linestyle", &d.linestyle)?;
    let weight = members.text("weight", &d.weight)?;
    let group = members.text("group", &d.group)?;
    let comment = members.text("comment", &d.comment)?;
    let tinable = members.optional_flag("surface")?;
    let hide = members.optional_flag("hide")?;
    let symbol = members
        .find("symbol")
        .map(|symbol| read_symbol(symbol, format!("{place} symbol")))
        .transpose()?;
    let text_style = members
        .find("text")
        .map(|text| read_text_style(text, format!("{place} text")))
        .transpose()?;
    let pipe = members
        .find("pipe")
        .map(|pipe| read_pipe(pipe, format!("{place} pipe")))
        .transpose()?;
    let vertex_pipe = members
        .find("vertexPipe")
        .map(|pipe| read_pipe(pipe, format!("{place} vertexPipe")))
        .transpose()?;
    let segment_pipe = members
        .find("segmentPipe")
        .map(|pipe| read_pipe(pipe, format!("{place} segmentPipe")))
        .transpose()?;
    let attributes = members
        .find("attributes")
        .map(|list| read_attributes(list, place, "attributes"))
        .transpose()?
        .unwrap_or_default();
    let vertex_attributes = members
        .find("vertexAttributes")
        .map(|list| read_attributes(list, place, "vertexAttributes"))
        .transpose()?
        .unwrap_or_default();
    let segment_attributes = members
        .find("segmentAttributes")
        .map(|list| read_attributes(list, place, "segmentAttributes"))
        .transpose()?
        .unwrap_or_default();
    members.finish()?;
    // validate would refuse this too, in the model's word for it; the file's
    // word is "layer".
    if !model.is_empty()
        && let Err(error) = validate_layer_path(&model)
    {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            place,
            &format!("\"layer\" is not a layer path: {}", error.message),
            in_quotes(&model),
        ));
    }
    Ok(SurveyRule {
        key,
        section,
        model,
        colour,
        linestyle,
        weight,
        group,
        comment,
        breakline,
        tinable,
        hide,
        symbol,
        text_style,
        pipe,
        vertex_pipe,
        segment_pipe,
        attributes,
        vertex_attributes,
        segment_attributes,
    })
}

fn says_this_format(format: Json<'_>) -> bool {
    matches!(format.node(), Node::Text(text) if text == CUSTOMISATION_FORMAT)
}

/// A version past the newest this build reads.
fn is_newer(version: Json<'_>) -> bool {
    matches!(*version.node(), Node::Unsigned(number) if number > CUSTOMISATION_VERSION as u64)
}

fn refuse_newer(version: Json<'_>) -> Error {
    make_error(
        ErrorCode::Unsupported,
        "the customisation was written by a newer version of Katana",
        format!(
            "version {}; this build reads version {CUSTOMISATION_VERSION}",
            shown(version)
        ),
    )
}

/// The root is a customisation of a version this build reads, or the refusal
/// that says which it is not. Before anything else is looked at: a newer file
/// may hold members this build would call unknown, and what it must be told is
/// that the file is newer.
fn require_customisation(root: Json<'_>) -> Status {
    if !root.is_object() {
        return Err(make_error(
            ErrorCode::ParseFailure,
            NOT_A_CUSTOMISATION,
            "the text is JSON, but not an object",
        ));
    }
    let Some(format) = root.find("format") else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            NOT_A_CUSTOMISATION,
            "it has no \"format\"",
        ));
    };
    if !says_this_format(format) {
        return Err(make_error(
            ErrorCode::ParseFailure,
            NOT_A_CUSTOMISATION,
            format!("its \"format\" is {}", shown(format)),
        ));
    }
    let Some(version) = root.find("version") else {
        return Err(refuse(
            ErrorCode::ParseFailure,
            TOP_LEVEL,
            "has no \"version\", which it must",
            "",
        ));
    };
    if !version.is_number_integer() {
        return Err(refuse(
            ErrorCode::ParseFailure,
            TOP_LEVEL,
            "\"version\" must be a whole number",
            shown(version),
        ));
    }
    if !version.is_number_unsigned() || version.as_u64() == 0 {
        return Err(refuse(
            ErrorCode::ParseFailure,
            TOP_LEVEL,
            &format!(
                "\"version\" is {}, and the first version is 1",
                shown(version)
            ),
            "",
        ));
    }
    if is_newer(version) {
        return Err(refuse_newer(version));
    }
    Ok(())
}

/// What a file is told whose reading stopped at a number too large to hold.
/// Everything else the reader refuses is refused with the whole text read, in
/// the order docs/customisation.md gives: is it this format, is it newer, then
/// what is wrong with it. Here there is only what the text held BEFORE the
/// number - the reader cannot go on past one - so that order is kept as far
/// as the text had got. A file that had said its "format" is refused by the
/// entry and the member, as for any value of the wrong kind (and one that had
/// said it is newer is told that); a file that had not is not known to be a
/// customisation at all, and gets the message every other text gets, with the
/// number and its place beside it. The writer puts "format" and "version"
/// first, so a file it wrote, or one edited from such a file, is the first
/// case.
fn refuse_too_large_in_file(tree: &Tree, number: &TooLarge) -> Error {
    let root = tree.root();
    let format = root.find("format");
    if !format.is_some_and(says_this_format) {
        return make_error(
            ErrorCode::ParseFailure,
            NOT_A_CUSTOMISATION,
            format!("it {TOO_LARGE}, {}", written_at(number)),
        );
    }
    if let Some(version) = root.find("version")
        && is_newer(version)
    {
        return refuse_newer(version);
    }
    refuse_too_large(tree, number, "")
}

fn read_definitions(
    json: Option<Json<'_>>,
    array: &str,
    symbol: bool,
    customisation: &mut Customisation,
    strokes: &mut [StrokesRead],
) -> Status {
    let Some(json) = json else {
        return Ok(());
    };
    for (index, item) in list_of(json, TOP_LEVEL, array)?.elements().enumerate() {
        let place = entry_label(array, index, item, "name");
        let definition = read_definition(item, &place, symbol, &customisation.name, strokes)?;
        // One library holds both lists, so a name is taken once across the
        // two - said here because NamedTable would call it AlreadyExists and
        // not say where the other one is.
        if let Some(other) = customisation.library.find(&definition.name) {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                &place,
                &format!(
                    "the file holds two definitions of this name; the other is in \"{}\"",
                    if other.symbol {
                        "symbols"
                    } else {
                        "linestyles"
                    }
                ),
                "",
            ));
        }
        if let Err(error) = customisation.library.add(definition) {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                &place,
                &error.message,
                "",
            ));
        }
    }
    Ok(())
}

fn read_customisation(tree: &Tree, strokes: &mut [StrokesRead]) -> Result<Customisation> {
    let root = tree.root();
    let mut members = Members::new(root, String::from(TOP_LEVEL))?;
    // read by require_customisation
    members.find("format");
    members.find("version");

    let mut customisation = Customisation {
        name: members.required_text("name")?,
        ..Customisation::default()
    };
    customisation.description = members.text("description", "")?;
    if let Some(notice) = members.find("notice") {
        customisation.notice = lines_of(notice, TOP_LEVEL, "notice")?;
    }
    if let Some(sources) = members.find("sources") {
        let d = CustomisationSourceNote::default();
        for (index, item) in list_of(sources, TOP_LEVEL, "sources")?
            .elements()
            .enumerate()
        {
            let mut part = Members::new(item, entry_label("sources", index, item, "name"))?;
            let name = part.required_text("name")?;
            let definitions = part.flag("definitions", d.definitions)?;
            let rules = part.flag("rules", d.rules)?;
            let notice = part
                .find("notice")
                .map(|notice| lines_of(notice, part.label(), "notice"))
                .transpose()?
                .unwrap_or_default();
            part.finish()?;
            customisation.sources.push(CustomisationSourceNote {
                name,
                definitions,
                rules,
                notice,
            });
        }
    }
    if let Some(based) = members.find("basedOn") {
        let mut part = Members::new(based, String::from("basedOn"))?;
        let name = part.required_text("name")?;
        let digest = part.required_text("digest")?;
        part.finish()?;
        customisation.based_on = Some(CustomisationBase { name, digest });
    }
    let colours = members.find("colours");
    if let Some(linework) = members.find("linework") {
        let mut part = Members::new(linework, String::from("linework"))?;
        let mut codes = LineworkCodes::default();
        for code in linework_code_members() {
            let fallback = String::from(code.spelling(&codes));
            *code.spelling_mut(&mut codes) = part.text(code.name, &fallback)?;
        }
        part.finish()?;
        customisation.linework = Some(codes);
    }
    if let Some(automation) = members.find("automation") {
        let d = CustomisationAutomation::default();
        let mut part = Members::new(automation, String::from("automation"))?;
        let codes_on_survey_import = part.flag("codesOnSurveyImport", d.codes_on_survey_import)?;
        let linework_on_survey_import =
            part.flag("lineworkOnSurveyImport", d.linework_on_survey_import)?;
        part.finish()?;
        customisation.automation = Some(CustomisationAutomation {
            codes_on_survey_import,
            linework_on_survey_import,
        });
    }
    let linestyles = members.find("linestyles");
    let symbols = members.find("symbols");
    let codes = members.find("codes");
    // Before the lists are gone through: a top-level member spelled wrongly
    // ("symbol" for "symbols") is the mistake to report, not whatever its
    // absence makes of the rest.
    members.finish()?;
    check_header(&customisation)?;

    if let Some(colours) = colours {
        if !colours.is_object() {
            return Err(refuse(
                ErrorCode::ParseFailure,
                TOP_LEVEL,
                "\"colours\" must be an object of names and colours, written {\"name\": \"#RRGGBB\"}",
                shown(colours),
            ));
        }
        for (key, item) in colours.members() {
            if key.len() > CUSTOMISATION_MOST_TEXT_BYTES {
                return Err(refuse(
                    ErrorCode::ParseFailure,
                    "colours",
                    &format!("a colour's name is {TOO_LONG}"),
                    shown_text(key),
                ));
            }
            let place = format!("colours {}", in_quotes(key));
            if !item.is_string() {
                return Err(refuse(
                    ErrorCode::ParseFailure,
                    &place,
                    "the colour must be text, written \"#RRGGBB\"",
                    shown(item),
                ));
            }
            let Ok(colour) = Color::from_hex(item.as_str()) else {
                return Err(refuse(
                    ErrorCode::ParseFailure,
                    &place,
                    &format!(
                        "the colour is {}, which is not #RRGGBB or #RRGGBBAA in hexadecimal",
                        shown(item)
                    ),
                    "",
                ));
            };
            if let Err(error) = customisation.colours.add(key, colour) {
                return Err(refuse(
                    ErrorCode::InvalidArgument,
                    &place,
                    &error.message,
                    error.context,
                ));
            }
        }
    }

    read_definitions(linestyles, "linestyles", false, &mut customisation, strokes)?;
    read_definitions(symbols, "symbols", true, &mut customisation, strokes)?;

    if let Some(codes) = codes {
        for (index, item) in list_of(codes, TOP_LEVEL, "codes")?.elements().enumerate() {
            let place = entry_label("codes", index, item, "key");
            let rule = read_rule(item, &place)?;
            if let Err(error) = customisation.map.add(rule) {
                return Err(refuse(
                    ErrorCode::InvalidArgument,
                    &place,
                    &error.message,
                    "",
                ));
            }
        }
    }
    Ok(customisation)
}

fn same_definitions(a: &StyleLibrary, b: &StyleLibrary) -> bool {
    if a.size() != b.size() {
        return false;
    }
    let mut same = true;
    a.for_each(|definition| {
        if same {
            same = b.find(&definition.name) == Some(definition);
        }
    });
    same
}

/// Reads the bytes of a Katana customisation file: UTF-8 with or without a
/// byte order mark, or UTF-16 (katana_core::text_encoding::decode_text).
///
/// ParseFailure "not a Katana customisation file" for text that is not JSON
/// or whose "format" is not CUSTOMISATION_FORMAT, and for bytes that are
/// neither UTF-8 nor UTF-16 (no code page is guessed at), a byte order mark
/// over bytes that break it included. Text that begins, after blanks, with
/// '<' or "//", which is how a survey code file (.mapfile) and a style library
/// (.4d) begin, is told so and where to take it, "...; the older formats are
/// converted with katana_customisation_convert (a developer's tool, not
/// installed; see docs/customisation.md)", with none of the parser's account
/// of the character; the program knows those formats by those two characters
/// and nothing more. Unsupported for a "version" newer than
/// CUSTOMISATION_VERSION. ParseFailure, naming the entry and the member
/// (`codes[57] "WM*": unknown member "linesytle"`), for a member the format
/// does not know, a member given twice, a required member missing, a value of
/// the wrong type, a word outside an enumeration, a stroke of the wrong length
/// and a number a double cannot hold (one too large, or one so small it would
/// be read as 0), and what is past a bound above. InvalidArgument, naming the
/// entry, for what is well-formed and still not a customisation: a bad name,
/// two definitions of one name, a colour name the table refuses, and
/// everything validate refuses of a definition, a rule or the linework codes.
/// Indices count from 0, as a JSON path does.
///
/// A Result whatever the text: one nested a hundred thousand lists deep is
/// refused like any other.
pub fn customisation_from_json(text: &[u8]) -> Result<Customisation> {
    let mut file = parse_json(text, NOT_A_CUSTOMISATION, TextKind::File)?;
    if let Some(number) = &file.too_large {
        return Err(refuse_too_large_in_file(&file.tree, number));
    }
    require_customisation(file.tree.root())?;
    if file.too_many_rules {
        // What the text said before the rule that was one too many: its
        // "format" and "version" came first in any file Katana wrote, and are
        // asked as for any other, so a file of another kind or a newer one is
        // told that.
        return Err(refuse(
            ErrorCode::ParseFailure,
            TOP_LEVEL,
            TOO_MANY_RULES,
            "",
        ));
    }
    refuse_duplicate(&file.tree, file.duplicate.as_ref(), "")?;
    read_customisation(&file.tree, &mut file.strokes)
}

/// Writes `customisation` as the text of a file: UTF-8, deterministic (the
/// same value gives the same bytes), a definition's strokes one a line.
/// Definitions are written in name order and rules in map order;
/// docs/customisation.md, "Layout", says where every byte goes.
///
/// InvalidArgument, naming the entry, for anything that would not read back
/// as itself: a number that is not finite or is past the bound above, text
/// past it, a definition of too many strokes, an arc sweeping too far, a
/// stroke carrying a member its kind does not use, a definition whose `texts`
/// are not exactly its text strokes in order, an attribute whose type is
/// neither "text" nor "integer", and whatever the reader would refuse (a bad
/// name, linework codes validate refuses). NotFound or InvalidArgument for
/// `options.only`, as said there.
pub fn customisation_to_json(
    customisation: &Customisation,
    options: &CustomisationWriteOptions,
) -> Result<String> {
    written(customisation, options)
}

/// One definition as the format's text - the very object a file holds in its
/// "linestyles" or "symbols" array, strokes one a line - for an editor to show
/// and take back. What a file says of a definition from outside it is given
/// here instead: `customisation` is the name it would sit under, so its source
/// is written (as "from") only when it differs; it fails as
/// customisation_to_json does, with the entry named `definition "<name>"`.
pub fn definition_to_json(definition: &LineStyle, customisation: &str) -> Result<String> {
    let mut out = Out::new(&format!("definition {}", in_quotes(&definition.name)));
    write_definition(&mut out, definition, customisation, "")?;
    Ok(out.take())
}

/// The other way: `customisation` is read as the definition's source when the
/// text gives none, and `symbol` is the array it would sit in. It fails as
/// customisation_from_json does, with the entry named `definition "<name>"`;
/// reading also runs validate, since no library is there to.
pub fn definition_from_json(text: &[u8], symbol: bool, customisation: &str) -> Result<LineStyle> {
    let mut file = parse_json(text, NOT_A_DEFINITION, TextKind::Definition)?;
    let root = file.tree.root();
    if !root.is_object() {
        let context = match &file.too_large {
            Some(number) => format!("it {TOO_LARGE}, {}", written_at(number)),
            None => String::from("the text is JSON, but not an object"),
        };
        return Err(make_error(
            ErrorCode::ParseFailure,
            NOT_A_DEFINITION,
            context,
        ));
    }
    let mut place = String::from("definition");
    if let Some(name) = root.find("name")
        && name.is_string()
    {
        place.push(' ');
        place.push_str(&in_quotes(name.as_str()));
    }
    if let Some(number) = &file.too_large {
        return Err(refuse_too_large(&file.tree, number, &place));
    }
    refuse_duplicate(&file.tree, file.duplicate.as_ref(), &place)?;
    let definition = read_definition(root, &place, symbol, customisation, &mut file.strokes)?;
    // In a file the library does this as the definition is added to it.
    if let Err(error) = definition.validate() {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            &place,
            &error.message,
            "",
        ));
    }
    Ok(definition)
}

/// The digest of a customisation file's bytes: FNV-1a, 64-bit, as 16
/// lower-case hexadecimal digits ("cbf29ce484222325" for no bytes). It tells
/// two editions of one name apart (CustomisationBase); it is not a security
/// hash and guards against nothing deliberate.
pub fn customisation_digest(bytes: &[u8]) -> String {
    // FNV-1a, 64-bit (Fowler, Noll and Vo): the hash a customisation's earlier
    // names are already known by in the cad layer, restated because entity may
    // not see cad. Checked against the algorithm's published vectors.
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}
