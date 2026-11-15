//! .12da text -> Archive.
//!
//! The manual describes the format as free-form: tokens separated by white
//! space, `//` to the end of the line a comment, text in double quotes with
//! `\"` and `\\` as escapes, keywords in any case. So this is a tokenizer and
//! a recursive-descent reader, NOT a line-oriented one: `chainage 0 interval 1
//! radius 15` on one line and the same three pairs on three lines are the
//! same file, and the manual's own examples use both.
//!
//! WHAT IS AN ERROR AND WHAT IS NOT. A file this reader cannot make structural
//! sense of is a ParseFailure naming the line: a brace that never closes, a
//! data block whose values do not divide into rows, a number that is not one,
//! a triangle naming a point that does not exist. Vocabulary it does not know
//! is NOT an error, because the writing program writes a great deal the
//! manual does not describe: an unknown block is skipped whole and counted in
//! `Archive::unrecognised`, an unknown scalar is kept in the element's
//! `extras`. The line between the two is whether carrying on could produce
//! WRONG geometry (stop) or merely less of it (continue, and say so).

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use katana_core::error::{Error, ErrorCode, Result};
use katana_core::text::{format_exact_real, parse_finite_double, parse_integer};
use katana_core::text_encoding::{decode_text, to_string as encoding_name};

use crate::archive::{
    AlignmentPart, AlignmentSource, ArcString, Archive, Attribute, AttributeList, AttributeValue,
    Breakline, CircleString, DraftingLeader, DraftingReference, DrainageRecord, DrainageString,
    Element, Field, FieldList, LasCloud, LasPoint, ModelRecord, PlotFrame, Segment, SegmentKind,
    SolvedGeometry, StringHeader, StringKind, SuperAlignment, SuperTin, TextString, Tin, Trimesh,
    TrimeshInfo, Vertex, VertexString, to_string,
};
use crate::lexer::{Lexer, Token, TokenKind};
use crate::text_utilities::{
    equals_ignoring_case, lower, parse_boolean, parse_hex_significand, trimmed,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadOptions {
    /// A file claiming more elements, or one data block claiming more values,
    /// than this is refused rather than allowed to exhaust memory. The
    /// defaults are an order of magnitude above the largest real archive seen
    /// (a 58 MB export with 2 400 strings and a 233 000-triangle tin).
    pub max_elements: usize,
    pub max_values_per_block: usize,
    /// Nesting beyond this is refused. Real exports reach 8 (attribute groups
    /// inside drawables inside a super alignment); the limit exists so that a
    /// file of a million `{` cannot overflow the stack.
    pub max_depth: usize,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            max_elements: 5_000_000,
            max_values_per_block: 200_000_000,
            max_depth: 64,
        }
    }
}

/// `text` must be UTF-8 (see katana_core::text_encoding for getting there
/// from a file); bytes that are not are refused here, about the file, rather
/// than by the entity model, which would refuse them for the whole import at
/// once.
pub fn read_archive(text: impl AsRef<[u8]>, options: &ReadOptions) -> Result<Archive> {
    let Ok(text) = std::str::from_utf8(text.as_ref()) else {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "the text is not valid UTF-8; decode the file with decodeText first",
        ));
    };
    read_checked(text, options)
}

/// Decodes, then reads. The encoding that was detected is added to the
/// archive's warnings when it had to be guessed.
pub fn read_archive_bytes(bytes: impl AsRef<[u8]>, options: &ReadOptions) -> Result<Archive> {
    let decoded = decode_text(bytes)?;
    // decode_text has already established what read_archive would check, so
    // going through it would walk all 58 MB a second time for nothing.
    let mut archive = read_checked(&decoded.text, options)?;
    if decoded.guessed {
        archive.warnings.insert(
            0,
            format!(
                "the file has no byte order mark; read as {}",
                encoding_name(decoded.encoding)
            ),
        );
    }
    Ok(archive)
}

/// A number as the format writes them: decimal, exponent, or a C99
/// hexadecimal float (`0x1.6d4014ac08318p+17`) - which is how the writing
/// program writes every tin by default, because it round-trips a double
/// exactly. None for anything else, including infinities and NaN, which no
/// coordinate may be.
pub fn parse_real(token: &str) -> Option<f64> {
    let mut token = token.as_bytes();
    // "502000.000, 6960000.000" is how a hand-made file separates a row.
    if let [rest @ .., b','] = token {
        token = rest;
    }
    let mut negative = false;
    if let [sign @ (b'+' | b'-'), rest @ ..] = token {
        negative = *sign == b'-';
        token = rest;
    }
    let first = *token.first()?;
    let value = if token.len() > 2 && token[0] == b'0' && (token[1] == b'x' || token[1] == b'X') {
        // from_chars takes hexadecimal floats WITHOUT the 0x that C99, and
        // the writing program, write.
        parse_hex_significand(&token[2..])?
    } else if !(first.is_ascii_digit() || first == b'.') {
        // from_chars would accept "inf" and "nan"; neither is a number here.
        return None;
    } else {
        parse_finite_double(token)?
    };
    if !value.is_finite() {
        return None;
    }
    Some(if negative { -value } else { value })
}

// The reader proper. `text` is already known to be UTF-8.
fn read_checked(text: &str, options: &ReadOptions) -> Result<Archive> {
    Reader::new(text, *options).run()
}

// A null inside a block of numbers, while it is still a block of numbers.
// parse_real never yields a NaN - it refuses anything not finite - so the
// value cannot collide with one that was read.
const NULL: f64 = f64::NAN;

// Warnings kept verbatim. A damaged 60 MB file can produce one per line, and
// a list nobody can read is not a report; past this the rest are counted.
const MAX_WARNINGS: usize = 100;

// The record layouts of manual 1.5.18, as the tags of the tagged form in the
// order the compact form lists the same values.
const LAS_P0: [&str; 12] = [
    "x", "y", "z", "i", "rn", "rc", "sd", "fe", "cl", "sr", "ud", "id",
];
const LAS_P1: [&str; 13] = [
    "x", "y", "z", "i", "rn", "rc", "sd", "fe", "cl", "sr", "ud", "id", "t",
];
const LAS_P2: [&str; 13] = [
    "x", "y", "z", "i", "rn", "rc", "sd", "fe", "cl", "sr", "ud", "id", "c",
];
const LAS_P3: [&str; 14] = [
    "x", "y", "z", "i", "rn", "rc", "sd", "fe", "cl", "sr", "ud", "id", "t", "c",
];
const LAS_P6: [&str; 15] = [
    "x", "y", "z", "i", "rn", "rc", "cf", "sc", "sd", "fe", "cl", "ud", "sr", "id", "t",
];
const LAS_P7: [&str; 16] = [
    "x", "y", "z", "i", "rn", "rc", "cf", "sc", "sd", "fe", "cl", "ud", "sr", "id", "t", "c",
];
const LAS_P8: [&str; 17] = [
    "x", "y", "z", "i", "rn", "rc", "cf", "sc", "sd", "fe", "cl", "ud", "sr", "id", "t", "c", "ir",
];

fn las_field_order(point_format: i32) -> &'static [&'static str] {
    match point_format {
        // 4 is 1 plus wave data, which the manual marks "not yet implemented".
        1 | 4 => &LAS_P1,
        2 => &LAS_P2,
        3 | 5 => &LAS_P3,
        6 | 9 => &LAS_P6,
        7 => &LAS_P7,
        8 | 10 => &LAS_P8,
        _ => &LAS_P0,
    }
}

// The integer fields of a LAS record, with the range each may hold.
trait LasInteger: Copy {
    const LOW: f64;
    const HIGH: f64;
    fn from_clamped(value: f64) -> Self;
}

macro_rules! las_integer {
    ($($kind:ty),+) => {
        $(impl LasInteger for $kind {
            const LOW: f64 = <$kind>::MIN as f64;
            const HIGH: f64 = <$kind>::MAX as f64;
            fn from_clamped(value: f64) -> Self {
                value as $kind
            }
        })+
    };
}

las_integer!(u8, u16, i16);

// True when `value` is a whole number within T; the field then takes it.
fn clamp_to<T: LasInteger>(value: f64, out: &mut T) -> bool {
    let exact = value == value.round() && (T::LOW..=T::HIGH).contains(&value);
    *out = T::from_clamped(value.round().clamp(T::LOW, T::HIGH));
    exact
}

// Returns false when the value was not one the field can hold, so that the
// caller can count it: a LAS field read as 0 because it was "abc" is a value
// lost, and the manual gives every one of them a range.
fn set_las_field(point: &mut LasPoint, tag: &str, value: &str) -> bool {
    if tag == "c" {
        // "64 bit integer" (manual 1.5.18): more than a double holds exactly,
        // and more than a SIGNED 64-bit integer holds - a LAS colour packs
        // three 16-bit channels, so its top bit is set whenever red is bright.
        // std::from_chars of an unsigned takes decimal digits and nothing
        // else: no sign.
        if !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && let Ok(colour) = value.parse::<u64>()
        {
            point.colour = colour;
            return true;
        }
        // A file that wrote the same bits as a signed number.
        if let Some(integer) = parse_integer(value) {
            point.colour = integer as u64;
            return true;
        }
        return false;
    }
    let Some(real) = parse_real(value) else {
        return false;
    };
    match tag {
        "x" => point.x = real,
        "y" => point.y = real,
        "z" => point.z = real,
        "t" => point.gps_time = real,
        "i" => return clamp_to(real, &mut point.intensity),
        "rn" => return clamp_to(real, &mut point.return_number),
        "rc" => return clamp_to(real, &mut point.return_count),
        "cf" => return clamp_to(real, &mut point.classification_flags),
        "sc" => return clamp_to(real, &mut point.scanner_channel),
        "sd" => return clamp_to(real, &mut point.scan_direction),
        "fe" => return clamp_to(real, &mut point.flight_line_edge),
        "cl" => return clamp_to(real, &mut point.classification),
        "sr" => return clamp_to(real, &mut point.scan_angle),
        "ud" => return clamp_to(real, &mut point.user_data),
        "id" => return clamp_to(real, &mut point.point_source_id),
        "ir" => return clamp_to(real, &mut point.near_infrared),
        _ => {} // a tag the format does not have: nothing to hold it
    }
    true
}

// The fields every string may carry, and the face string's hatching (manual
// 1.5.4): documented, so they take a value.
const VERTEX_STRING_DOCUMENTED: [&str; 11] = [
    "hatch_angle",
    "hatch_distance",
    "hatch_colour",
    "edge_colour",
    "fill_mode",
    "edge_mode",
    "null_value",
    "weight",
    "time_created",
    "time_updated",
    "interval",
];

const TEXT_ANNOTATION_KEYS: [&str; 14] = [
    "angle",
    "offset",
    "raise",
    "worldsize",
    "papersize",
    "screensize",
    "textstyle",
    "slant",
    "xfactor",
    "x_factor",
    "justify",
    "text_colour",
    "whiteout",
    "border_style",
];

const ARC_KEYS: [&str; 15] = [
    "interval",
    "radius",
    "xcentre",
    "ycentre",
    "zcentre",
    "xstart",
    "ystart",
    "zstart",
    "xend",
    "yend",
    "zend",
    "weight",
    "time_created",
    "time_updated",
    "null_value",
];

const TEXT_KEYS: [&str; 22] = [
    "x",
    "y",
    "z",
    "text",
    "angle",
    "offset",
    "raise",
    "textstyle",
    "slant",
    "xfactor",
    "x_factor",
    "worldsize",
    "papersize",
    "screensize",
    "justify",
    "text_colour",
    "whiteout",
    "border_style",
    "weight",
    "time_created",
    "time_updated",
    "null_value",
];

const PLOT_FRAME_KEYS: [&str; 28] = [
    "title_file",
    "border",
    "viewport",
    "user_title_file",
    "use_title_file",
    "title_1",
    "title_2",
    "plot_file",
    "text_size",
    "sheet_code",
    "width",
    "height",
    "scale",
    "rotation",
    "xorigin",
    "yorigin",
    "left_margin",
    "right_margin",
    "top_margin",
    "bottom_margin",
    "plotter",
    "textstyle",
    "plotter_type",
    "plotter_mode",
    "plotter_names",
    "weight",
    "time_created",
    "time_updated",
];

const VERTEX_KINDS: [(&str, StringKind); 8] = [
    ("super", StringKind::Super),
    ("2d", StringKind::TwoD),
    ("3d", StringKind::ThreeD),
    ("4d", StringKind::FourD),
    ("pipe", StringKind::Pipe),
    ("polyline", StringKind::Polyline),
    ("face", StringKind::Face),
    ("interface", StringKind::Interface),
];

// Every keyword some reader below has a case for. Used to tell a colour's
// second word from the next keyword, and an unknown flag from an unknown key
// and its value. A set, not a list: this is asked once per colour and once
// per undocumented key, and a linear scan walked all 140 entries for every
// miss - 14 million string comparisons on a 100 000-element archive.
static KNOWN_KEYWORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "name",
        "model",
        "colour",
        "color",
        "style",
        "chainage",
        "breakline",
        "attributes",
        "drafting_element",
        "leader",
        "overrides",
        "hook_angle",
        "justify_x",
        "display_text",
        "format_text",
        "style_name",
        "arrow_x",
        "arrow_y",
        "arrow_z",
        "hook_x",
        "hook_y",
        "hook_z",
        "arrow_size",
        "hook_length",
        "text_size",
        "closed",
        "z",
        "data",
        "data_2d",
        "data_3d",
        "radius_data",
        "major_data",
        "geometry_data",
        "colour_data",
        "point_data",
        "diameter",
        "diameter_value",
        "diameter_data",
        "pipe_value",
        "pipe_data",
        "culvert_value",
        "culvert_data",
        "justify",
        "pipe_justify",
        "interval",
        "weight",
        "time_created",
        "time_updated",
        "vertex_tinable_value",
        "vertex_tinable_data",
        "segment_tinable_value",
        "segment_tinable_data",
        "vertex_visible_value",
        "vertex_visible_data",
        "segment_visible_value",
        "segment_visible_data",
        "vertex_text_value",
        "vertex_text_data",
        "segment_text_value",
        "segment_text_data",
        "vertex_annotate_value",
        "vertex_annotate_data",
        "segment_annotate_value",
        "segment_annotate_data",
        "symbol_value",
        "symbol_data",
        "vertex_attribute_data",
        "segment_attribute_data",
        "radius",
        "xcentre",
        "ycentre",
        "zcentre",
        "xstart",
        "ystart",
        "zstart",
        "xend",
        "yend",
        "zend",
        "x",
        "y",
        "text",
        "angle",
        "offset",
        "raise",
        "textstyle",
        "slant",
        "xfactor",
        "x_factor",
        "worldsize",
        "papersize",
        "screensize",
        "outfall",
        "flow_direction",
        "pit",
        "pit_v2",
        "pipe",
        "property_control",
        "house_connection",
        "spiral_type",
        "valid_horizontal",
        "valid_vertical",
        "horizontal_parts",
        "horizontal_data",
        "vertical_parts",
        "vertical_data",
        "hipdata",
        "vipdata",
        "draw_mode",
        "length",
        "points",
        "triangles",
        "neighbours",
        "nulling",
        "colours",
        "input",
        "tins",
        "trimesh_3d",
        "vertices",
        "faces",
        "edges",
        "info",
        "blend",
        "size",
        "rotation",
        "width",
        "height",
        "scale",
        "xorigin",
        "yorigin",
        "border",
        "format",
        "range",
        "categories",
        "file_name",
        // and the words that begin an element or a command, so that
        // `colour dark` at the end of a header does not eat the next one
        "string",
        "tin",
        "full_tin",
        "super_tin",
        "primitive_3d",
        "null",
        "null_value",
        "project_attributes",
    ]
    .into_iter()
    .collect()
});

// `word` is the token AS WRITTEN; the set holds the keywords folded, and is
// asked with the token folded the same way.
fn is_known_keyword(word: &str) -> bool {
    KNOWN_KEYWORDS.contains(lower(word).as_str())
}

fn is_text_annotation_key(word: &str) -> bool {
    TEXT_ANNOTATION_KEYS.contains(&word)
}

// A height as read: only the `null` KEYWORD is no height yet. The null VALUE
// is applied afterwards, by apply_null, once the element is whole - because a
// string's own `null_value` may come after its data (string commands are
// order-free, manual 1.4.6), and until the block is closed the value in force
// is not known.
fn height_of(value: f64) -> Option<f64> {
    if value.is_nan() { None } else { Some(value) }
}

fn apply_null(z: &mut Option<f64>, null_value: f64) {
    if *z == Some(null_value) {
        *z = None;
    }
}

fn apply_null_all(vertices: &mut [Vertex], null_value: f64) {
    for vertex in vertices {
        apply_null(&mut vertex.z, null_value);
    }
}

// radius_data + major_data -> segments, once both have been read.
fn apply_radii(radii: &[f64], majors: &[bool], segments: &mut Vec<Segment>) {
    if !segments.is_empty() || radii.is_empty() {
        return; // geometry_data, when present, says more and wins
    }
    let mut any_arc = false;
    for (i, &value) in radii.iter().enumerate() {
        let mut segment = Segment::default();
        let radius = if value.is_nan() { 0.0 } else { value };
        if radius != 0.0 {
            segment.kind = SegmentKind::Arc;
            segment.radius = radius;
            segment.major = i < majors.len() && majors[i];
            any_arc = true;
        }
        segments.push(segment);
    }
    if !any_arc {
        segments.clear();
    }
}

// Moves what a small block held BESIDES its modelled members into extras, so
// that `pipe_value { diameter 0.3 thickness 0.02 }` does not lose the
// thickness for having been understood.
fn keep_others(fields: &FieldList, known: &[&str], prefix: &str, extras: &mut FieldList) {
    for field in fields.fields() {
        if !known.contains(&field.key.as_str()) {
            extras.add(
                format!("{prefix}{}", field.key),
                field.value.clone(),
                field.quoted,
            );
        }
    }
}

// A vertex from three named fields, or nothing when x or y is missing or not
// a number: a point placed at the origin for want of a coordinate is a point
// nobody wrote, and in a projected job it is 6 000 km away.
fn take_vertex(fields: &mut FieldList, x: &str, y: &str, z: &str) -> Option<Vertex> {
    let ex = fields.real(x)?;
    let ny = fields.real(y)?;
    let vertex = Vertex {
        x: ex,
        y: ny,
        z: fields.real(z).and_then(height_of),
    };
    fields.take(x);
    fields.take(y);
    fields.take(z);
    Some(vertex)
}

// "v12_p3" -> 3
fn point_format_of(format: &str) -> i32 {
    let Some(marker) = format.rfind("_p") else {
        return 0;
    };
    match parse_integer(&format[marker + 2..]) {
        Some(value) if (0..=10).contains(&value) => value as i32,
        _ => 0,
    }
}

struct Reader<'a> {
    lexer: Lexer<'a>,
    options: ReadOptions,
    archive: Archive,
    // Model names, folded, -> their place in archive.model_names: the map
    // compares WITHOUT case, as the manual does, and is asked rather than the
    // list scanned - every element asks, and the scan was O(elements x
    // models), 20 million comparisons on an archive of 100 000 elements over
    // 400 models.
    model_index: HashMap<String, usize>,
    // Failure is sticky: once fail() has been called every token is End,
    // every loop over a block's contents ends, and run() reports the FIRST
    // failure. That keeps a recursive-descent reader free of a Result on
    // every one of its sixty functions.
    error: Option<Error>,
    path: Vec<String>,
    suppressed_warnings: usize,
    null_attributes: usize,
    bad_las_fields: usize,

    // The commands in force (manual 1.4): defaults as the manual gives them.
    model: String,
    colour: String,
    style: String,
    breakline: Breakline,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str, options: ReadOptions) -> Self {
        Self {
            lexer: Lexer::new(text),
            options,
            archive: Archive::default(),
            model_index: HashMap::new(),
            error: None,
            path: Vec::new(),
            suppressed_warnings: 0,
            null_attributes: 0,
            bad_las_fields: 0,
            model: String::from("data"),
            colour: String::from("red"),
            style: String::from("1"),
            breakline: Breakline::Point,
        }
    }

    // ---- token plumbing ------------------------------------------------------

    fn failed(&self) -> bool {
        self.error.is_some()
    }

    fn fail(&mut self, line: u32, message: impl Into<String>) {
        if self.error.is_none() {
            self.error = Some(
                Error::new(ErrorCode::ParseFailure, message).with_context(format!("line {line}")),
            );
        }
    }

    fn warn(&mut self, message: String) {
        if self.archive.warnings.len() < MAX_WARNINGS {
            self.archive.warnings.push(message);
        } else {
            self.suppressed_warnings += 1;
        }
    }

    fn peek(&mut self) -> Token<'a> {
        if self.failed() {
            return Token::default();
        }
        self.lexer.peek()
    }

    fn next(&mut self) -> Token<'a> {
        if self.failed() {
            return Token::default();
        }
        let token = self.lexer.next();
        if token.kind == TokenKind::Open && self.lexer.depth() > self.options.max_depth {
            let depth = self.options.max_depth;
            self.fail(
                token.line,
                format!("blocks are nested more than {depth} deep"),
            );
            return Token::default();
        }
        token
    }

    // True at the `}` that ends the current block - and at the end of the
    // text and after a failure, so that `while !at_block_end()` always ends.
    fn at_block_end(&mut self) -> bool {
        let kind = self.peek().kind;
        kind == TokenKind::Close || kind == TokenKind::End
    }

    fn expect_open(&mut self, what: &str) -> bool {
        let token = self.next();
        if token.kind != TokenKind::Open {
            self.fail(token.line, format!("expected '{{' after {what}"));
            return false;
        }
        true
    }

    fn expect_close(&mut self, what: &str) {
        let token = self.next();
        if token.kind != TokenKind::Close && !self.failed() {
            self.fail(
                token.line,
                format!("the file ends inside {what}: a '}}' is missing"),
            );
        }
    }

    // The value after a keyword. A brace or the end of the text is a failure,
    // because "a value must always exist" (manual 1.4).
    fn next_value(&mut self, keyword: &str) -> String {
        let token = self.next();
        if !token.is_value() {
            self.fail(token.line, format!("'{keyword}' has no value"));
            return String::new();
        }
        token.text()
    }

    // A colour name. The standard colours include two-word names - "dark
    // green", "light grey" - which the writing program puts in quotes but
    // which hand-made files write bare, so `dark` or `light` takes the next
    // word with it unless that word is the next keyword.
    fn next_colour(&mut self, keyword: &str) -> String {
        let token = self.next();
        if !token.is_value() {
            self.fail(token.line, format!("'{keyword}' has no value"));
            return String::new();
        }
        let mut colour = token.text();
        if token.kind == TokenKind::Word {
            let qualifier =
                equals_ignoring_case(&colour, "dark") || equals_ignoring_case(&colour, "light");
            if qualifier {
                let following = self.peek();
                if following.kind == TokenKind::Word && !is_known_keyword(following.raw) {
                    colour.push(' ');
                    colour.push_str(&self.next().text());
                }
            }
        }
        colour
    }

    // None is a NULL, which is a value; a token that is not a number at all
    // is a failure.
    fn next_nullable_real(&mut self, keyword: &str) -> Option<f64> {
        let token = self.next();
        if !token.is_value() {
            self.fail(token.line, format!("'{keyword}' has no value"));
            return None;
        }
        if token.kind == TokenKind::Word && equals_ignoring_case(token.raw, "null") {
            return None;
        }
        let value = parse_real(token.raw);
        if value.is_none() {
            self.fail(
                token.line,
                format!("'{keyword}' needs a number, not '{}'", token.raw),
            );
        }
        value
    }

    fn next_real(&mut self, keyword: &str) -> f64 {
        let line = self.peek().line;
        let value = self.next_nullable_real(keyword);
        if value.is_none() && !self.failed() {
            self.fail(line, format!("'{keyword}' cannot be null"));
        }
        value.unwrap_or(0.0)
    }

    fn next_boolean(&mut self, keyword: &str) -> bool {
        let token = self.next();
        let value = if token.is_value() {
            parse_boolean(token.raw)
        } else {
            None
        };
        match value {
            Some(flag) => flag,
            None => {
                self.fail(token.line, format!("'{keyword}' needs true or false"));
                false
            }
        }
    }

    // The null value in force for an element: its own `null_value` when it
    // has one (kept in its extras, so that it is written back), else the
    // file's current one.
    fn null_value_for(&self, extras: &FieldList) -> f64 {
        extras.real("null_value").unwrap_or(self.archive.null_value)
    }

    // ---- blocks --------------------------------------------------------------

    fn path_with(&self, leaf: &str) -> String {
        let mut path = String::new();
        for part in &self.path {
            path.push_str(part);
            path.push('/');
        }
        path.push_str(leaf);
        path
    }

    // The next token is the `{` of a block this reader has no member for.
    fn skip_unknown_block(&mut self, keyword: &str) {
        let path = self.path_with(keyword);
        *self.archive.unrecognised.entry(path).or_insert(0) += 1;
        let open = self.next();
        if open.kind != TokenKind::Open {
            self.fail(open.line, format!("expected '{{' after {keyword}"));
            return;
        }
        let target = self.lexer.depth() - 1;
        while !self.failed() {
            let token = self.next();
            if token.kind == TokenKind::End {
                self.fail(open.line, format!("the block '{keyword}' is never closed"));
                return;
            }
            if token.kind == TokenKind::Close && self.lexer.depth() == target {
                return;
            }
        }
    }

    // `{ v v v ... }` of numbers, nulls as NULL.
    fn read_number_block(&mut self, what: &str, out: &mut Vec<f64>) -> bool {
        if !self.expect_open(what) {
            return false;
        }
        while !self.at_block_end() {
            let token = self.next();
            if token.kind == TokenKind::Word && equals_ignoring_case(token.raw, "null") {
                out.push(NULL);
            } else if let Some(value) = parse_real(token.raw)
                && token.kind == TokenKind::Word
            {
                out.push(value);
            } else {
                self.fail(
                    token.line,
                    format!("'{}' in {what} is not a number", token.raw),
                );
                return false;
            }
            if out.len() > self.options.max_values_per_block {
                let limit = self.options.max_values_per_block;
                self.fail(
                    token.line,
                    format!("{what} holds more values than the limit of {limit}"),
                );
                return false;
            }
        }
        self.expect_close(what);
        !self.failed()
    }

    fn read_integer_block(&mut self, what: &str, out: &mut Vec<i64>) -> bool {
        if !self.expect_open(what) {
            return false;
        }
        self.read_integers_until_close(what, out);
        self.expect_close(what);
        !self.failed()
    }

    fn read_integers_until_close(&mut self, what: &str, out: &mut Vec<i64>) {
        while !self.at_block_end() {
            let token = self.next();
            let value = if token.kind == TokenKind::Word {
                parse_integer(token.raw)
            } else {
                None
            };
            let Some(value) = value else {
                self.fail(
                    token.line,
                    format!("'{}' in {what} is not a whole number", token.raw),
                );
                return;
            };
            out.push(value);
            if out.len() > self.options.max_values_per_block {
                let limit = self.options.max_values_per_block;
                self.fail(
                    token.line,
                    format!("{what} holds more values than the limit of {limit}"),
                );
                return;
            }
        }
    }

    // `{ word "text" word ... }` as their values.
    fn read_text_block(&mut self, what: &str, out: &mut Vec<String>) -> bool {
        if !self.expect_open(what) {
            return false;
        }
        while !self.at_block_end() {
            out.push(self.next().text());
            if out.len() > self.options.max_values_per_block {
                let line = self.peek().line;
                self.fail(line, format!("{what} holds more values than the limit"));
                return false;
            }
        }
        self.expect_close(what);
        !self.failed()
    }

    fn read_boolean_block(&mut self, what: &str, out: &mut Vec<bool>) -> bool {
        let mut words = Vec::new();
        let line = self.peek().line;
        if !self.read_text_block(what, &mut words) {
            return false;
        }
        for word in &words {
            let Some(value) = parse_boolean(word) else {
                self.fail(line, format!("'{word}' in {what} is not a true/false flag"));
                return false;
            };
            out.push(value);
        }
        true
    }

    // Rows of `columns` numbers as vertices: x y [z]. A value count that does
    // not divide into rows means the rows cannot be told apart, and guessing
    // would shift every coordinate after the mistake.
    fn read_vertex_rows(
        &mut self,
        what: &str,
        columns: usize,
        vertices: &mut Vec<Vertex>,
        mut extra_columns: Option<&mut Vec<f64>>,
    ) -> bool {
        let line = self.peek().line;
        let mut values = Vec::new();
        if !self.read_number_block(what, &mut values) {
            return false;
        }
        if !values.len().is_multiple_of(columns) {
            self.fail(
                line,
                format!(
                    "{what} holds {} values, which is not a whole number of rows of {columns}",
                    values.len()
                ),
            );
            return false;
        }
        vertices.reserve(values.len() / columns);
        for row in values.chunks(columns) {
            if row[0].is_nan() || row[1].is_nan() {
                self.fail(
                    line,
                    format!("{what} has a null x or y; only a height can be null"),
                );
                return false;
            }
            vertices.push(Vertex {
                x: row[0],
                y: row[1],
                z: if columns >= 3 {
                    height_of(row[2])
                } else {
                    None
                },
            });
            if let Some(extras) = extra_columns.as_deref_mut() {
                extras.extend_from_slice(&row[3.min(columns)..]);
            }
        }
        true
    }

    // The superseded `x y z radius bulge` rows of a polyline, a drainage
    // string and an old super string: geometry of the segment LEAVING each
    // vertex rides on the vertex.
    fn read_polyline_rows(
        &mut self,
        what: &str,
        vertices: &mut Vec<Vertex>,
        segments: &mut Vec<Segment>,
    ) -> bool {
        let mut extras = Vec::new();
        if !self.read_vertex_rows(what, 5, vertices, Some(&mut extras)) {
            return false;
        }
        segments.clear();
        let mut any_arc = false;
        let mut i = 0;
        while i + 1 < extras.len() {
            let mut segment = Segment::default();
            let radius = if extras[i].is_nan() { 0.0 } else { extras[i] };
            if radius != 0.0 {
                segment.kind = SegmentKind::Arc;
                segment.radius = radius;
                segment.major = !extras[i + 1].is_nan() && extras[i + 1] != 0.0;
                any_arc = true;
            }
            segments.push(segment);
            i += 2;
        }
        if !any_arc {
            segments.clear(); // "empty means every segment is straight"
        }
        true
    }

    fn read_attributes(&mut self, out: &mut AttributeList) {
        if !self.expect_open("attributes") {
            return;
        }
        self.read_attribute_body(out);
        self.expect_close("attributes");
    }

    fn read_attribute_body(&mut self, out: &mut AttributeList) {
        while !self.at_block_end() {
            let type_token = self.next();
            let kind = lower(type_token.raw);
            if !type_token.is_word() {
                self.warn(format!(
                    "line {}: text where an attribute type was expected; ignored",
                    type_token.line
                ));
                continue;
            }
            if kind == "group" {
                let mut group = Attribute::default();
                let mut members = AttributeList::new();
                if self.peek().kind != TokenKind::Open {
                    group.name = self.next_value("group"); // `group "name" { ... }`
                }
                if !self.expect_open("group") {
                    return;
                }
                while !self.at_block_end() {
                    let key = self.next();
                    let word = lower(key.raw);
                    if key.is_word() && word == "name" {
                        group.name = self.next_value("name");
                    } else if key.is_word() && word == "attributes" {
                        self.read_attributes(&mut members);
                    } else if self.peek().kind == TokenKind::Open {
                        self.skip_unknown_block(&word);
                    } else if key.is_word() {
                        self.next(); // an undocumented scalar of the group itself
                    }
                }
                self.expect_close("group");
                group.value = AttributeValue::Group(members);
                out.push(group);
                continue;
            }
            if self.peek().kind == TokenKind::Open {
                self.skip_unknown_block(&kind);
                continue;
            }

            let mut attribute = Attribute {
                name: self.next_value(&kind),
                ..Attribute::default()
            };
            let value_token = self.next();
            if !value_token.is_value() {
                self.fail(
                    value_token.line,
                    format!("attribute '{}' has no value", attribute.name),
                );
                return;
            }
            let is_null = value_token.kind == TokenKind::Word
                && equals_ignoring_case(value_token.raw, "null");
            if kind == "integer" || kind == "uid" {
                let Some(value) = parse_integer(value_token.raw) else {
                    if !is_null {
                        self.warn(format!(
                            "line {}: integer attribute '{}' has the value '{}'; ignored",
                            value_token.line, attribute.name, value_token.raw
                        ));
                    }
                    self.null_attributes += 1;
                    continue;
                };
                attribute.value = AttributeValue::Integer(value);
            } else if kind == "real" {
                let Some(value) = parse_real(value_token.raw) else {
                    if !is_null {
                        self.warn(format!(
                            "line {}: real attribute '{}' has the value '{}'; ignored",
                            value_token.line, attribute.name, value_token.raw
                        ));
                    }
                    self.null_attributes += 1;
                    continue;
                };
                attribute.value = AttributeValue::Real(value);
            } else {
                attribute.value = AttributeValue::Text(value_token.text());
            }
            if kind != "integer" && kind != "real" && kind != "text" {
                attribute.declared_type = kind;
            }
            out.push(attribute);
        }
    }

    // A block of `key value` pairs, with an optional `attributes` block and -
    // for a property control - a `data` block of its own.
    fn read_field_block(
        &mut self,
        what: &str,
        fields: &mut FieldList,
        mut attributes: Option<&mut AttributeList>,
        mut data: Option<(&mut Vec<Vertex>, &mut Vec<Segment>)>,
    ) {
        if !self.expect_open(what) {
            return;
        }
        self.path.push(String::from(what));
        while !self.at_block_end() {
            let key = self.next();
            if !key.is_word() {
                self.warn(format!(
                    "line {}: text where a keyword was expected in {what}; ignored",
                    key.line
                ));
                continue;
            }
            let word = lower(key.raw);
            if self.peek().kind == TokenKind::Open {
                if word == "attributes"
                    && let Some(attributes) = attributes.as_deref_mut()
                {
                    self.read_attributes(attributes);
                } else if word == "data"
                    && let Some((vertices, segments)) = data.as_mut()
                {
                    self.read_polyline_rows("data", vertices, segments);
                } else {
                    self.skip_unknown_block(&word);
                }
                continue;
            }
            if self.at_block_end() {
                fields.add(word, "", false); // a keyword with nothing after it
                break;
            }
            if word.ends_with("colour") {
                let was_quoted = self.peek().kind == TokenKind::Quoted;
                let colour = self.next_colour(&word);
                fields.add(word, colour, was_quoted);
            } else {
                let value = self.next();
                fields.add(word, value.text(), value.kind == TokenKind::Quoted);
            }
        }
        self.expect_close(what);
        self.path.pop();
    }

    // `name_data { properties { ... } properties { ... } }`
    fn read_properties_list(&mut self, what: &str, out: &mut Vec<FieldList>) {
        if !self.expect_open(what) {
            return;
        }
        self.path.push(String::from(what));
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if self.peek().kind != TokenKind::Open {
                continue; // stray word between records
            }
            if word == "properties" {
                let mut properties = FieldList::new();
                self.read_field_block("properties", &mut properties, None, None);
                out.push(properties);
            } else {
                self.skip_unknown_block(&word);
            }
        }
        self.expect_close(what);
        self.path.pop();
    }

    fn read_attributes_list(&mut self, what: &str, out: &mut Vec<AttributeList>) {
        if !self.expect_open(what) {
            return;
        }
        self.path.push(String::from(what));
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if self.peek().kind != TokenKind::Open {
                continue;
            }
            if word == "attributes" {
                let mut attributes = AttributeList::new();
                self.read_attributes(&mut attributes);
                out.push(attributes);
            } else {
                self.skip_unknown_block(&word);
            }
        }
        self.expect_close(what);
        self.path.pop();
    }

    fn read_geometry_data(&mut self, segments: &mut Vec<Segment>) {
        if !self.expect_open("geometry_data") {
            return;
        }
        self.path.push(String::from("geometry_data"));
        segments.clear();
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if self.peek().kind != TokenKind::Open {
                continue;
            }
            let mut segment = Segment::default();
            let mut fields = FieldList::new();
            if word == "straight" {
                self.read_field_block(&word, &mut fields, None, None);
            } else if word == "arc" {
                self.read_field_block(&word, &mut fields, None, None);
                segment.kind = SegmentKind::Arc;
                segment.radius = fields.real("radius").unwrap_or(0.0);
                segment.major = fields.boolean("major").unwrap_or(false);
                if segment.radius == 0.0 {
                    segment.kind = SegmentKind::Straight; // an arc of no radius is a line
                }
            } else if word == "spiral" || word == "curve" || word == "parabola" {
                self.read_field_block(&word, &mut fields, None, None);
                segment.kind = if word == "spiral" {
                    SegmentKind::Spiral
                } else if word == "curve" {
                    SegmentKind::Curve
                } else {
                    SegmentKind::Parabola
                };
                segment.parameters = fields;
            } else {
                // A kind of segment the manual does not have. It still
                // occupies a place in the list - dropping it would shift every
                // later segment onto the wrong pair of vertices - so it is
                // kept as a straight and said to be one.
                self.warn(format!(
                    "line {}: segment of unknown kind '{word}' is drawn as a straight",
                    key.line
                ));
                self.skip_unknown_block(&word);
            }
            segments.push(segment);
        }
        self.expect_close("geometry_data");
        self.path.pop();
    }

    // ---- strings -------------------------------------------------------------

    fn new_header(&self) -> StringHeader {
        StringHeader {
            model: self.model.clone(),
            colour: self.colour.clone(),
            style: self.style.clone(),
            breakline: self.breakline,
            ..StringHeader::default()
        }
    }

    // The fields every string has. True when `key` was one and was consumed.
    fn read_header_field(&mut self, header: &mut StringHeader, key: &str) -> bool {
        match key {
            "name" => header.name = self.next_value(key),
            "model" => header.model = self.next_value(key), // registered by add_element
            "colour" | "color" => header.colour = self.next_colour(key),
            "style" => header.style = self.next_value(key),
            "chainage" => header.chainage = self.next_nullable_real(key).unwrap_or(0.0),
            "breakline" => header.breakline = self.read_breakline(),
            "attributes" if self.peek().kind == TokenKind::Open => {
                self.read_attributes(&mut header.attributes);
            }
            _ => return false,
        }
        true
    }

    fn read_breakline(&mut self) -> Breakline {
        let token = self.next();
        if equals_ignoring_case(token.raw, "point") {
            return Breakline::Point;
        }
        if !equals_ignoring_case(token.raw, "line") {
            self.warn(format!(
                "line {}: breakline '{}' is neither point nor line; read as line",
                token.line, token.raw
            ));
        }
        Breakline::Line
    }

    // Anything a string reader has no case for: a block is counted, a scalar
    // is kept. `documented` says the key is one the manual gives this element,
    // and so ALWAYS has a value ("a value must always exist", 1.4): the guard
    // below, which stops an undocumented flag from swallowing the next
    // keyword, must not fire on `text Pipe` or `title_1 Scale`, which are
    // legal - the manual only requires quotes around text that is not
    // alphanumeric.
    fn read_unknown(
        &mut self,
        key: &Token<'a>,
        word: &str,
        extras: &mut FieldList,
        documented: bool,
    ) {
        if !key.is_word() {
            let path = self.path_with("");
            self.warn(format!(
                "line {}: stray text \"{}\" in {path} ignored",
                key.line,
                key.text()
            ));
            return;
        }
        if self.peek().kind == TokenKind::Open {
            self.skip_unknown_block(word);
            return;
        }
        if self.at_block_end() {
            extras.add(word, "", false);
            return;
        }
        // A keyword this reader knows cannot be the VALUE of one it does not:
        // the unknown word is a flag with no value, and swallowing the next
        // keyword as its value would lose a real field.
        if !documented {
            let following = self.peek();
            if following.kind == TokenKind::Word && is_known_keyword(following.raw) {
                extras.add(word, "", false);
                return;
            }
        }
        if word.ends_with("colour") {
            let was_quoted = self.peek().kind == TokenKind::Quoted;
            let colour = self.next_colour(word);
            extras.add(word, colour, was_quoted);
            return;
        }
        let value = self.next();
        extras.add(word, value.text(), value.kind == TokenKind::Quoted);
    }

    fn read_vertex_string(&mut self, kind: StringKind) {
        let mut string = VertexString {
            kind,
            header: self.new_header(),
            ..VertexString::default()
        };
        let label = format!("string {}", to_string(kind));
        self.path.push(label.clone());

        let mut radii = Vec::new();
        let mut majors = Vec::new();
        let mut four_d_annotation = FieldList::new();

        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if key.is_word() && self.read_header_field(&mut string.header, &word) {
                continue;
            }
            let block = self.peek().kind == TokenKind::Open;
            if !key.is_word() {
                self.read_unknown(&key, &word, &mut string.header.extras, false);
            } else if word == "closed" {
                string.closed = self.next_boolean(&word);
            } else if word == "z" && !block {
                string.constant_z = height_of(self.next_nullable_real(&word).unwrap_or(NULL));
            } else if word == "data_2d" && block {
                self.read_vertex_rows(&word, 2, &mut string.vertices, None);
            } else if word == "data_3d" && block {
                self.read_vertex_rows(&word, 3, &mut string.vertices, None);
            } else if word == "data" && block {
                self.read_legacy_data(&mut string);
            } else if word == "radius_data" && block {
                self.read_number_block(&word, &mut radii);
            } else if word == "major_data" && block {
                self.read_boolean_block(&word, &mut majors);
            } else if word == "geometry_data" && block {
                self.read_geometry_data(&mut string.segments);
            } else if word == "colour_data" && block {
                self.read_text_block(&word, &mut string.segment_colours);
            } else if word == "point_data" && block {
                self.read_text_block(&word, &mut string.point_ids);
            } else if (word == "diameter_value" || word == "diameter") && !block {
                string.diameter = self.next_nullable_real(&word);
            } else if word == "diameter_data" && block {
                self.read_number_block(&word, &mut string.diameters);
            } else if word == "pipe_value" && block {
                // What the writing program's version 15 writes where the
                // manual says diameter_value.
                let mut fields = FieldList::new();
                self.read_field_block(&word, &mut fields, None, None);
                string.diameter = fields.real("diameter");
                keep_others(
                    &fields,
                    &["diameter"],
                    "pipe_value.",
                    &mut string.header.extras,
                );
            } else if word == "pipe_data" && block {
                let mut records = Vec::new();
                self.read_properties_list(&word, &mut records);
                for record in &records {
                    string
                        .diameters
                        .push(record.real("diameter").unwrap_or(0.0));
                }
            } else if word == "culvert_value" && block {
                let mut fields = FieldList::new();
                self.read_field_block(&word, &mut fields, None, None);
                string.culvert = Some([
                    fields.real("width").unwrap_or(0.0),
                    fields.real("height").unwrap_or(0.0),
                ]);
            } else if word == "culvert_data" && block {
                let mut records = Vec::new();
                self.read_properties_list(&word, &mut records);
                for record in &records {
                    string.culverts.push([
                        record.real("width").unwrap_or(0.0),
                        record.real("height").unwrap_or(0.0),
                    ]);
                }
            } else if (word == "justify" || word == "pipe_justify")
                && !block
                && kind != StringKind::FourD
            {
                string.justify = lower(&self.next_value(&word));
            } else if word == "vertex_tinable_value" && !block {
                string.vertex_tinable_value = Some(self.next_boolean(&word));
            } else if word == "segment_tinable_value" && !block {
                string.segment_tinable_value = Some(self.next_boolean(&word));
            } else if word == "vertex_visible_value" && !block {
                string.vertex_visible_value = Some(self.next_boolean(&word));
            } else if word == "segment_visible_value" && !block {
                string.segment_visible_value = Some(self.next_boolean(&word));
            } else if word == "vertex_tinable_data" && block {
                self.read_boolean_block(&word, &mut string.vertex_tinable);
            } else if word == "segment_tinable_data" && block {
                self.read_boolean_block(&word, &mut string.segment_tinable);
            } else if word == "vertex_visible_data" && block {
                self.read_boolean_block(&word, &mut string.vertex_visible);
            } else if word == "segment_visible_data" && block {
                self.read_boolean_block(&word, &mut string.segment_visible);
            } else if word == "vertex_text_value" && !block {
                string.vertex_text_value = Some(self.next_value(&word));
            } else if word == "segment_text_value" && !block {
                string.segment_text_value = Some(self.next_value(&word));
            } else if word == "vertex_text_data" && block {
                self.read_text_block(&word, &mut string.vertex_text);
            } else if word == "segment_text_data" && block {
                self.read_text_block(&word, &mut string.segment_text);
            } else if word == "vertex_annotate_value" && block {
                let mut annotation = FieldList::new();
                self.read_field_block(&word, &mut annotation, None, None);
                string.vertex_annotation = Some(annotation);
            } else if word == "segment_annotate_value" && block {
                let mut annotation = FieldList::new();
                self.read_field_block(&word, &mut annotation, None, None);
                string.segment_annotation = Some(annotation);
            } else if word == "vertex_annotate_data" && block {
                self.read_properties_list(&word, &mut string.vertex_annotations);
            } else if word == "segment_annotate_data" && block {
                self.read_properties_list(&word, &mut string.segment_annotations);
            } else if word == "symbol_value" && block {
                let mut symbol = FieldList::new();
                self.read_field_block(&word, &mut symbol, None, None);
                string.symbol = Some(symbol);
            } else if word == "symbol_data" && block {
                self.read_properties_list(&word, &mut string.symbols);
            } else if word == "vertex_attribute_data" && block {
                self.read_attributes_list(&word, &mut string.vertex_attributes);
            } else if word == "segment_attribute_data" && block {
                self.read_attributes_list(&word, &mut string.segment_attributes);
            } else if word == "interval" && block {
                let mut interval = FieldList::new();
                self.read_field_block(&word, &mut interval, None, None);
                string.interval = Some(interval);
            } else if kind == StringKind::FourD && !block && is_text_annotation_key(&word) {
                let value = self.next();
                four_d_annotation.add(word, value.text(), value.kind == TokenKind::Quoted);
            } else {
                let documented = VERTEX_STRING_DOCUMENTED.contains(&word.as_str());
                self.read_unknown(&key, &word, &mut string.header.extras, documented);
            }
        }
        self.expect_close(&label);
        self.path.pop();
        if self.failed() {
            return;
        }

        let null_value = self.null_value_for(&string.header.extras);
        apply_null_all(&mut string.vertices, null_value);
        apply_null(&mut string.constant_z, null_value);
        apply_radii(&radii, &majors, &mut string.segments);
        if !four_d_annotation.empty() {
            string.vertex_annotation = Some(four_d_annotation);
        }
        self.check_counts(&string);
        self.add_element(Element::VertexString(string));
    }

    // The `data` block of the string types that have one, whose columns
    // depend on the type (manual 1.5.11-1.5.15, 1.5.2, 1.5.5, 1.5.8.3).
    fn read_legacy_data(&mut self, string: &mut VertexString) {
        match string.kind {
            StringKind::TwoD => {
                self.read_vertex_rows("data", 2, &mut string.vertices, None);
            }
            StringKind::ThreeD | StringKind::Pipe | StringKind::Face => {
                self.read_vertex_rows("data", 3, &mut string.vertices, None);
            }
            StringKind::Interface => {
                let mut modes = Vec::new();
                if self.read_vertex_rows("data", 4, &mut string.vertices, Some(&mut modes)) {
                    let mut bad = 0_usize;
                    for &mode in &modes {
                        // -1 cut, 0 on the surface, 1 fill (manual 1.5.6).
                        // Anything else is not a mode, and a cast would make
                        // one up.
                        let valid = mode == -1.0 || mode == 0.0 || mode == 1.0;
                        if !valid {
                            bad += 1;
                        }
                        string
                            .interface_modes
                            .push(if valid { mode as i32 } else { 0 });
                    }
                    if bad != 0 {
                        self.warn(format!(
                            "string \"{}\": {bad} interface modes are not -1, 0 or 1 and were read as 0",
                            string.header.name
                        ));
                    }
                }
            }
            StringKind::FourD => {
                // x y z text - the one data block that is not all numbers.
                let line = self.peek().line;
                let mut tokens = Vec::new();
                if !self.read_text_block("data", &mut tokens) {
                    return;
                }
                if !tokens.len().is_multiple_of(4) {
                    self.fail(
                        line,
                        format!(
                            "the data of a 4d string holds {} values, which is not a whole number of x y z text rows",
                            tokens.len()
                        ),
                    );
                    return;
                }
                let (rows, _) = tokens.as_chunks::<4>();
                for row in rows {
                    let x = parse_real(&row[0]);
                    let y = parse_real(&row[1]);
                    let null_z = equals_ignoring_case(&row[2], "null");
                    let z = parse_real(&row[2]);
                    let (Some(x), Some(y)) = (x, y) else {
                        self.fail(
                            line,
                            "a row of the 4d string's data does not start with x y z",
                        );
                        return;
                    };
                    if z.is_none() && !null_z {
                        self.fail(
                            line,
                            "a row of the 4d string's data does not start with x y z",
                        );
                        return;
                    }
                    string.vertices.push(Vertex {
                        x,
                        y,
                        z: height_of(z.unwrap_or(NULL)),
                    });
                    string.vertex_text.push(row[3].clone());
                }
            }
            StringKind::Super | StringKind::Polyline => {
                self.read_polyline_rows("data", &mut string.vertices, &mut string.segments);
            }
        }
    }

    fn check_counts(&mut self, string: &VertexString) {
        let vertices = string.vertices.len();
        let segments = string.segment_count();
        let name = &string.header.name;
        let per_vertex = [
            (string.point_ids.len(), "point_data"),
            (string.vertex_text.len(), "vertex_text_data"),
            (string.vertex_tinable.len(), "vertex_tinable_data"),
            (string.vertex_visible.len(), "vertex_visible_data"),
            (string.vertex_attributes.len(), "vertex_attribute_data"),
        ];
        for (count, what) in per_vertex {
            if count != 0 && count != vertices {
                self.warn(format!(
                    "string \"{name}\": {what} has {count} entries for {vertices} vertices"
                ));
            }
        }
        // n entries are allowed for the n-1 segments of an open string.
        let per_segment = [
            (string.segments.len(), "the segment geometry"),
            (string.segment_colours.len(), "colour_data"),
            (string.segment_text.len(), "segment_text_data"),
            (string.diameters.len(), "the pipe diameters"),
            (string.culverts.len(), "culvert_data"),
        ];
        for (count, what) in per_segment {
            if count != 0 && count != segments && count != vertices {
                self.warn(format!(
                    "string \"{name}\": {what} has {count} entries for {segments} segments"
                ));
            }
        }
        // The manual (1.5.8.4.4) says a string cannot have both pipe diameters
        // and culvert dimensions. Version 15 of the writing program writes
        // both on every culvert string it exports, so that is not worth a
        // warning; both are kept.
    }

    // Scalars of a string whose geometry is all keyword fields. `documented`
    // lists the keys the manual gives the element, every one of which takes a
    // value; anything else is an unknown and gets the flag-or-value guess.
    fn read_scalar_string(
        &mut self,
        what: &str,
        header: &mut StringHeader,
        fields: &mut FieldList,
        documented: &[&str],
    ) {
        self.path.push(String::from(what));
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if key.is_word() && self.read_header_field(header, &word) {
                continue;
            }
            let known = documented.contains(&word.as_str());
            self.read_unknown(&key, &word, fields, known);
        }
        self.expect_close(what);
        self.path.pop();
    }

    // Manual 1.4.6: "if there is not enough recognised information to define
    // the string, the string is ignored" - said, not silent.
    fn incomplete(&mut self, header: &StringHeader, what: &str, missing: &str) {
        self.warn(format!(
            "{what} \"{}\" in model \"{}\" ignored: {missing} is missing or not a number",
            header.name, header.model
        ));
    }

    fn read_arc_string(&mut self) {
        let mut arc = ArcString {
            header: self.new_header(),
            ..ArcString::default()
        };
        let mut fields = FieldList::new();
        self.read_scalar_string("string arc", &mut arc.header, &mut fields, &ARC_KEYS);
        if self.failed() {
            return;
        }
        arc.radius = fields.real("radius").unwrap_or(0.0);
        fields.take("radius");
        let centre = take_vertex(&mut fields, "xcentre", "ycentre", "zcentre");
        let start = take_vertex(&mut fields, "xstart", "ystart", "zstart");
        let end = take_vertex(&mut fields, "xend", "yend", "zend");
        let (Some(centre), Some(start), Some(end)) = (centre, start, end) else {
            let missing = if centre.is_none() {
                "its centre"
            } else if start.is_none() {
                "its start"
            } else {
                "its end"
            };
            self.incomplete(&arc.header, "string arc", missing);
            return;
        };
        arc.centre = centre;
        arc.start = start;
        arc.end = end;
        arc.header.extras = fields;
        let null_value = self.null_value_for(&arc.header.extras);
        apply_null(&mut arc.centre.z, null_value);
        apply_null(&mut arc.start.z, null_value);
        apply_null(&mut arc.end.z, null_value);
        self.add_element(Element::ArcString(arc));
    }

    fn read_circle_string(&mut self, feature: bool) {
        let mut circle = CircleString {
            feature,
            header: self.new_header(),
            ..CircleString::default()
        };
        let mut fields = FieldList::new();
        let what = if feature {
            "string feature"
        } else {
            "string circle"
        };
        self.read_scalar_string(what, &mut circle.header, &mut fields, &ARC_KEYS);
        if self.failed() {
            return;
        }
        let radius = fields.real("radius");
        fields.take("radius");
        let centre = take_vertex(&mut fields, "xcentre", "ycentre", "zcentre");
        let (Some(centre), Some(radius)) = (centre, radius) else {
            let missing = if centre.is_none() {
                "its centre"
            } else {
                "its radius"
            };
            self.incomplete(&circle.header, what, missing);
            return;
        };
        circle.radius = radius;
        circle.centre = centre;
        circle.header.extras = fields;
        let null_value = self.null_value_for(&circle.header.extras);
        apply_null(&mut circle.centre.z, null_value);
        self.add_element(Element::CircleString(circle));
    }

    fn read_text_string(&mut self) {
        let mut text = TextString {
            header: self.new_header(),
            ..TextString::default()
        };
        let mut fields = FieldList::new();
        self.read_scalar_string("string text", &mut text.header, &mut fields, &TEXT_KEYS);
        if self.failed() {
            return;
        }
        if let Some(value) = fields.take("text") {
            text.text = value.value;
        }
        let Some(position) = take_vertex(&mut fields, "x", "y", "z") else {
            self.incomplete(&text.header, "string text", "its position");
            return;
        };
        text.position = position;
        for key in ["weight", "time_created", "time_updated", "null_value"] {
            if let Some(field) = fields.take(key) {
                text.header.extras.add(field.key, field.value, field.quoted);
            }
        }
        text.annotation = fields;
        let null_value = self.null_value_for(&text.header.extras);
        apply_null(&mut text.position.z, null_value);
        self.add_element(Element::TextString(text));
    }

    fn read_drafting_reference(&mut self, reference: &mut DraftingReference) {
        if !self.expect_open(&reference.name) {
            return;
        }
        self.path.push(reference.name.clone());
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if key.is_word() && self.peek().kind == TokenKind::Open {
                let mut child = DraftingReference {
                    name: word,
                    ..DraftingReference::default()
                };
                self.read_drafting_reference(&mut child);
                reference.blocks.push(child);
            } else {
                self.read_unknown(&key, &word, &mut reference.fields, true);
            }
        }
        self.expect_close(&reference.name);
        self.path.pop();
    }

    fn read_drafting_element(&mut self) {
        if !self.expect_open("drafting_element") {
            return;
        }
        self.path.push(String::from("drafting_element"));
        self.read_drafting_element_body();
        self.path.pop();
    }

    fn read_drafting_element_body(&mut self) {
        let mut leader = DraftingLeader {
            header: self.new_header(),
            ..DraftingLeader::default()
        };
        let mut found = false;
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if key.is_word() && word == "leader" && self.peek().kind == TokenKind::Open {
                if found {
                    self.fail(key.line, "a drafting_element contains more than one leader");
                    return;
                }
                found = true;
                self.next();
                self.path.push(String::from("leader"));
                let read = self.read_leader_body(&mut leader);
                self.path.pop();
                if !read {
                    return;
                }
            } else if !key.is_word() || !self.read_header_field(&mut leader.header, &word) {
                self.read_unknown(&key, &word, &mut leader.header.extras, false);
            }
        }
        self.expect_close("drafting_element");
        if self.failed() {
            return;
        }
        if !found {
            self.incomplete(&leader.header, "drafting_element", "its leader");
            return;
        }
        let arrow = take_vertex(&mut leader.fields, "arrow_x", "arrow_y", "arrow_z");
        let hook = take_vertex(&mut leader.fields, "hook_x", "hook_y", "hook_z");
        let (Some(arrow), Some(hook)) = (arrow, hook) else {
            let missing = if arrow.is_none() {
                "its arrow"
            } else {
                "its hook"
            };
            self.incomplete(&leader.header, "drafting_element/leader", missing);
            return;
        };
        leader.arrow = arrow;
        leader.hook = hook;
        let null_value = self.null_value_for(&leader.header.extras);
        apply_null(&mut leader.arrow.z, null_value);
        apply_null(&mut leader.hook.z, null_value);
        self.add_element(Element::DraftingLeader(leader));
    }

    // The fields of a leader block, after its `{`. False when the reading
    // failed and the element is to be left.
    fn read_leader_body(&mut self, leader: &mut DraftingLeader) -> bool {
        while !self.at_block_end() {
            let field = self.next();
            let name = lower(field.raw);
            if field.is_word() && name == "overrides" && self.peek().kind == TokenKind::Open {
                self.read_field_block(&name, &mut leader.overrides, None, None);
            } else if field.is_word() && name == "arrow" && self.peek().kind == TokenKind::Open {
                if leader.arrow_reference.is_some() {
                    self.fail(
                        field.line,
                        "a drafting leader contains more than one arrow reference",
                    );
                    return false;
                }
                let mut reference = DraftingReference {
                    name,
                    ..DraftingReference::default()
                };
                self.read_drafting_reference(&mut reference);
                leader.arrow_reference = Some(reference);
            } else {
                self.read_unknown(&field, &name, &mut leader.fields, true);
            }
        }
        self.expect_close("leader");
        true
    }

    // `text "label" x y [z]` on one line: not in the manual, but written by
    // other tools and read by viewers of the format.
    fn read_short_text(&mut self, line: u32) {
        let mut text = TextString {
            header: self.new_header(),
            text: self.next_value("text"),
            ..TextString::default()
        };
        let mut numbers = Vec::new();
        while numbers.len() < 3 && self.peek().kind == TokenKind::Word {
            let following = self.peek();
            let is_null = equals_ignoring_case(following.raw, "null");
            let value = parse_real(following.raw);
            if value.is_none() && !is_null {
                break;
            }
            numbers.push(value.unwrap_or(NULL));
            self.next();
        }
        if numbers.len() < 2 || numbers[0].is_nan() || numbers[1].is_nan() {
            self.fail(
                line,
                format!("'text \"{}\"' must be followed by x and y", text.text),
            );
            return;
        }
        text.position = Vertex {
            x: numbers[0],
            y: numbers[1],
            z: if numbers.len() > 2 {
                height_of(numbers[2])
            } else {
                None
            },
        };
        apply_null(&mut text.position.z, self.archive.null_value);
        self.add_element(Element::TextString(text));
    }

    fn read_plot_frame(&mut self) {
        let mut frame = PlotFrame {
            header: self.new_header(),
            ..PlotFrame::default()
        };
        self.read_scalar_string(
            "string plot_frame",
            &mut frame.header,
            &mut frame.fields,
            &PLOT_FRAME_KEYS,
        );
        if !self.failed() {
            self.add_element(Element::PlotFrame(frame));
        }
    }

    fn read_drainage_string(&mut self) {
        let mut drainage = DrainageString {
            header: self.new_header(),
            ..DrainageString::default()
        };
        self.path.push(String::from("string drainage"));
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if key.is_word() && self.read_header_field(&mut drainage.header, &word) {
                continue;
            }
            let block = self.peek().kind == TokenKind::Open;
            if !key.is_word() {
                self.read_unknown(&key, &word, &mut drainage.header.extras, false);
            } else if word == "outfall" && !block {
                drainage.outfall = self.next_nullable_real(&word);
            } else if word == "flow_direction" && !block {
                let line = key.line;
                let direction = self.next_real(&word);
                if direction == 0.0 || direction == 1.0 {
                    drainage.flow_direction = Some(direction as i32);
                } else if !self.failed() {
                    self.warn(format!(
                        "line {line}: flow_direction must be 0 or 1; '{}' ignored",
                        format_exact_real(direction)
                    ));
                }
            } else if word == "data" && block {
                self.read_polyline_rows("data", &mut drainage.vertices, &mut drainage.segments);
            } else if (word == "pit" || word == "pit_v2") && block {
                drainage.pits_are_version2 = drainage.pits_are_version2 || word == "pit_v2";
                let mut record = DrainageRecord::default();
                self.read_field_block(
                    &word,
                    &mut record.fields,
                    Some(&mut record.attributes),
                    None,
                );
                drainage.pits.push(record);
            } else if word == "pipe" && block {
                let mut record = DrainageRecord::default();
                self.read_field_block(
                    &word,
                    &mut record.fields,
                    Some(&mut record.attributes),
                    None,
                );
                drainage.pipes.push(record);
            } else if word == "property_control" && block {
                let mut record = DrainageRecord::default();
                let DrainageRecord {
                    fields,
                    attributes,
                    vertices,
                    segments,
                } = &mut record;
                self.read_field_block(&word, fields, Some(attributes), Some((vertices, segments)));
                drainage.property_controls.push(record);
            } else if word == "house_connection" && block {
                let mut record = DrainageRecord::default();
                self.read_field_block(
                    &word,
                    &mut record.fields,
                    Some(&mut record.attributes),
                    None,
                );
                drainage.house_connections.push(record);
            } else {
                self.read_unknown(&key, &word, &mut drainage.header.extras, false);
            }
        }
        self.expect_close("string drainage");
        self.path.pop();
        if !self.failed() {
            let null_value = self.null_value_for(&drainage.header.extras);
            apply_null_all(&mut drainage.vertices, null_value);
            for control in &mut drainage.property_controls {
                apply_null_all(&mut control.vertices, null_value);
            }
            self.add_element(Element::DrainageString(drainage));
        }
    }

    // ---- alignments ----------------------------------------------------------

    fn read_parts(&mut self, what: &str, parts: &mut Vec<AlignmentPart>) {
        if !self.expect_open(what) {
            return;
        }
        self.path.push(String::from(what));
        while !self.at_block_end() {
            let key = self.next();
            if self.peek().kind != TokenKind::Open {
                continue; // nothing in a parts block is a bare scalar
            }
            let kind = lower(key.raw);
            let mut part = AlignmentPart::default();
            self.read_field_block(&kind, &mut part.fields, Some(&mut part.attributes), None);
            part.kind = kind;
            parts.push(part);
        }
        self.expect_close(what);
        self.path.pop();
    }

    fn read_solved_geometry(&mut self, what: &str, geometry: &mut SolvedGeometry) {
        let vertical = what == "vertical_data";
        if !self.expect_open(what) {
            return;
        }
        self.path.push(String::from(what));
        let mut radii = Vec::new();
        let mut majors = Vec::new();
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            let block = self.peek().kind == TokenKind::Open;
            if !key.is_word() {
                continue;
            }
            if word == "data_2d" && block {
                self.read_vertex_rows(&word, 2, &mut geometry.vertices, None);
            } else if word == "geometry_data" && block {
                self.read_geometry_data(&mut geometry.segments);
            } else if word == "radius_data" && block {
                self.read_number_block(&word, &mut radii);
            } else if word == "major_data" && block {
                self.read_boolean_block(&word, &mut majors);
            } else if word == "closed" && !block {
                geometry.closed = self.next_boolean(&word);
            } else if word == "interval" && block {
                let mut interval = FieldList::new();
                self.read_field_block(&word, &mut interval, None, None);
                geometry.interval = Some(interval);
            } else if word == "colour_data" && block {
                self.read_text_block(&word, &mut geometry.segment_colours);
            } else {
                self.read_unknown(&key, &word, &mut geometry.header, false);
            }
        }
        self.expect_close(what);
        self.path.pop();
        apply_radii(&radii, &majors, &mut geometry.segments);
        if vertical {
            // "major ... is ignored since only minor arcs are used" (manual
            // 1.5.9.2.3b): a grade line cannot double back in chainage.
            for segment in &mut geometry.segments {
                segment.major = false;
            }
        }
    }

    fn read_super_alignment(&mut self) {
        let mut alignment = SuperAlignment {
            header: self.new_header(),
            ..SuperAlignment::default()
        };
        self.path.push(String::from("string super_alignment"));
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if key.is_word() && self.read_header_field(&mut alignment.header, &word) {
                continue;
            }
            let block = self.peek().kind == TokenKind::Open;
            if !key.is_word() {
                self.read_unknown(&key, &word, &mut alignment.header.extras, false);
            } else if word == "closed" && !block {
                alignment.closed = self.next_boolean(&word);
            } else if word == "spiral_type" && !block {
                alignment.spiral_type = self.next_value(&word);
            } else if word == "valid_horizontal" && !block {
                alignment.valid_horizontal = Some(self.next_boolean(&word));
            } else if word == "valid_vertical" && !block {
                alignment.valid_vertical = Some(self.next_boolean(&word));
            } else if word == "pipe_value" && block {
                // Where a super alignment that is a pipeline keeps its size.
                let mut fields = FieldList::new();
                self.read_field_block(&word, &mut fields, None, None);
                alignment.diameter = fields.real("diameter");
            } else if word == "length" && !block {
                alignment.pipe_length = self.next_nullable_real(&word);
            } else if word == "horizontal_parts" && block {
                self.read_parts(&word, &mut alignment.horizontal_parts);
            } else if word == "vertical_parts" && block {
                self.read_parts(&word, &mut alignment.vertical_parts);
            } else if word == "horizontal_data" && block {
                let mut data = SolvedGeometry::default();
                self.read_solved_geometry(&word, &mut data);
                alignment.horizontal_data = Some(data);
            } else if word == "vertical_data" && block {
                let mut data = SolvedGeometry::default();
                self.read_solved_geometry(&word, &mut data);
                alignment.vertical_data = Some(data);
            } else {
                self.read_unknown(&key, &word, &mut alignment.header.extras, false);
            }
        }
        self.expect_close("string super_alignment");
        self.path.pop();
        if !self.failed() {
            self.add_element(Element::SuperAlignment(alignment));
        }
    }

    // The superseded alignment and pipeline strings. Their hipdata and vipdata
    // are the IP method written as rows instead of blocks, and are read into
    // the same parts, numbered in hundreds as the manual numbers them.
    fn read_legacy_alignment(&mut self, source: AlignmentSource) {
        let mut alignment = SuperAlignment {
            source,
            header: self.new_header(),
            ..SuperAlignment::default()
        };
        let what = if source == AlignmentSource::Pipeline {
            "string pipeline"
        } else {
            "string alignment"
        };
        self.path.push(String::from(what));
        let mut part_id: i64 = 0;

        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if key.is_word() && self.read_header_field(&mut alignment.header, &word) {
                continue;
            }
            let block = self.peek().kind == TokenKind::Open;
            if !key.is_word() {
                self.read_unknown(&key, &word, &mut alignment.header.extras, false);
            } else if word == "spiral_type" && !block {
                alignment.spiral_type = self.next_value(&word);
            } else if word == "diameter" && !block && source == AlignmentSource::Pipeline {
                alignment.diameter = self.next_nullable_real(&word);
            } else if word == "length" && !block && source == AlignmentSource::Pipeline {
                alignment.pipe_length = self.next_nullable_real(&word);
            } else if word == "hipdata" && block {
                self.read_hip_data(&mut alignment.horizontal_parts, &mut part_id);
            } else if word == "vipdata" && block {
                self.read_vip_data(&mut alignment.vertical_parts, &mut part_id);
            } else {
                self.read_unknown(&key, &word, &mut alignment.header.extras, false);
            }
        }
        self.expect_close(what);
        self.path.pop();
        if !self.failed() {
            self.add_element(Element::SuperAlignment(alignment));
        }
    }

    // x y radius [spil1 length] [spil2 length]
    fn read_hip_data(&mut self, parts: &mut Vec<AlignmentPart>, part_id: &mut i64) {
        let line = self.peek().line;
        let mut tokens = Vec::new();
        if !self.read_text_block("hipdata", &mut tokens) {
            return;
        }
        let mut i = 0;
        while i < tokens.len() {
            let x = parse_real(&tokens[i]);
            let y = tokens.get(i + 1).and_then(|token| parse_real(token));
            let radius = tokens.get(i + 2).and_then(|token| parse_real(token));
            let (Some(x), Some(y), Some(radius)) = (x, y, radius) else {
                self.fail(line, "a row of hipdata is not 'x y radius'");
                return;
            };
            i += 3;
            let mut spiral_in = 0.0;
            let mut spiral_out = 0.0;
            while i + 1 < tokens.len() {
                let tag = lower(&tokens[i]);
                if tag != "spil1" && tag != "spil2" {
                    break;
                }
                let Some(length) = parse_real(&tokens[i + 1]) else {
                    self.fail(line, format!("'{tag}' in hipdata needs a length"));
                    return;
                };
                if tag == "spil1" {
                    spiral_in = length;
                } else {
                    spiral_out = length;
                }
                i += 2;
            }
            let kind = if spiral_in != 0.0 || spiral_out != 0.0 {
                "spiral"
            } else if radius != 0.0 {
                "arc"
            } else {
                "ip"
            };
            let mut part = AlignmentPart {
                kind: String::from(kind),
                ..AlignmentPart::default()
            };
            *part_id += 100;
            part.fields.set_integer("id", *part_id);
            if kind != "ip" {
                part.fields.set_real("r", radius);
            }
            if kind == "spiral" {
                part.fields.set_real("l1", spiral_in);
                part.fields.set_real("l2", spiral_out);
            }
            part.fields.set_real("x", x);
            part.fields.set_real("y", y);
            parts.push(part);
        }
    }

    // chainage height length [parabola]   |   chainage height radius circle
    fn read_vip_data(&mut self, parts: &mut Vec<AlignmentPart>, part_id: &mut i64) {
        let line = self.peek().line;
        let mut tokens = Vec::new();
        if !self.read_text_block("vipdata", &mut tokens) {
            return;
        }
        let mut i = 0;
        while i < tokens.len() {
            let chainage = parse_real(&tokens[i]);
            let height = tokens.get(i + 1).and_then(|token| parse_real(token));
            let size = tokens.get(i + 2).and_then(|token| parse_real(token));
            let (Some(chainage), Some(height), Some(size)) = (chainage, height, size) else {
                self.fail(line, "a row of vipdata is not 'chainage height length'");
                return;
            };
            i += 3;
            let mut circle = false;
            if i < tokens.len() {
                let tag = lower(&tokens[i]);
                if tag == "parabola" || tag == "circle" {
                    circle = tag == "circle";
                    i += 1;
                }
            }
            let kind = if size == 0.0 {
                "ip"
            } else if circle {
                "arc"
            } else {
                "length"
            };
            let mut part = AlignmentPart {
                kind: String::from(kind),
                ..AlignmentPart::default()
            };
            *part_id += 100;
            part.fields.set_integer("id", *part_id);
            if kind == "arc" {
                part.fields.set_real("r", size);
            } else if kind == "length" {
                part.fields.set_real("l", size);
            }
            part.fields.set_real("x", chainage);
            part.fields.set_real("y", height);
            parts.push(part);
        }
    }

    // ---- point clouds ---------------------------------------------------------

    fn read_las_cloud(&mut self) {
        let mut cloud = LasCloud {
            header: self.new_header(),
            ..LasCloud::default()
        };
        self.path.push(String::from("string las_cloud_data"));
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if key.is_word() && self.read_header_field(&mut cloud.header, &word) {
                continue;
            }
            let block = self.peek().kind == TokenKind::Open;
            if key.is_word() && (word == "data" || word == "ref_data") && block {
                self.read_las_data(&word, &mut cloud);
            } else {
                self.read_unknown(&key, &word, &mut cloud.header.extras, false);
            }
        }
        self.expect_close("string las_cloud_data");
        self.path.pop();
        if !self.failed() {
            if self.bad_las_fields != 0 {
                self.warn(format!(
                    "point cloud \"{}\": {} point fields were not numbers or were outside the range the manual gives them, and were read as 0 or clamped",
                    cloud.header.name, self.bad_las_fields
                ));
                self.bad_las_fields = 0;
            }
            self.add_element(Element::LasCloud(cloud));
        }
    }

    fn read_las_data(&mut self, what: &str, cloud: &mut LasCloud) {
        if !self.expect_open(what) {
            return;
        }
        self.path.push(String::from(what));
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            let block = self.peek().kind == TokenKind::Open;
            if !key.is_word() {
                continue;
            }
            if word == "categories" && block {
                self.read_boolean_block(&word, &mut cloud.categories);
            } else if word == "range" && block {
                self.read_field_block(&word, &mut cloud.range, None, None);
            } else if word == "format" && !block {
                cloud.format = lower(&self.next_value(&word));
                cloud.point_format = point_format_of(&cloud.format);
            } else if word == "file_name" && !block {
                cloud.reference_file = self.next_value(&word);
            } else if block
                && (word.starts_with("points_v") || word.starts_with("compact_points_v"))
            {
                let compact = word.starts_with("compact_");
                let format = String::from(&word[if compact { 15 } else { 7 }..]);
                if cloud.format.is_empty() {
                    cloud.format = format.clone();
                    cloud.point_format = point_format_of(&format);
                } else if cloud.format != format {
                    self.warn(format!(
                        "line {}: point cloud declares format {} but holds a {word} block",
                        key.line, cloud.format
                    ));
                }
                self.read_las_points(&word, compact, point_format_of(&format), &mut cloud.points);
            } else {
                self.read_unknown(&key, &word, &mut cloud.header.extras, false);
            }
        }
        self.expect_close(what);
        self.path.pop();
    }

    fn read_las_points(
        &mut self,
        what: &str,
        compact: bool,
        point_format: i32,
        points: &mut Vec<LasPoint>,
    ) {
        if !self.expect_open(what) {
            return;
        }
        self.path.push(String::from(what));
        if self.read_las_point_records(compact, point_format, points) {
            self.expect_close(what);
        }
        self.path.pop();
    }

    // The records of a points block, after its `{`. False when the reading
    // failed and the block is to be left.
    fn read_las_point_records(
        &mut self,
        compact: bool,
        point_format: i32,
        points: &mut Vec<LasPoint>,
    ) -> bool {
        let order = las_field_order(point_format);
        while !self.at_block_end() {
            let key = self.next();
            if self.peek().kind != TokenKind::Open {
                continue;
            }
            if !equals_ignoring_case(key.raw, "p") {
                self.skip_unknown_block(&lower(key.raw));
                continue;
            }
            let line = key.line;
            let mut tokens = Vec::new();
            if !self.read_text_block("p", &mut tokens) {
                return false;
            }
            let mut point = LasPoint::default();
            if compact {
                if tokens.len() < 3 {
                    self.fail(line, "a compact point record needs at least x y z");
                    return false;
                }
                for (tag, value) in order.iter().zip(&tokens) {
                    if !set_las_field(&mut point, tag, value) {
                        self.bad_las_fields += 1;
                    }
                }
            } else {
                let (pairs, _) = tokens.as_chunks::<2>();
                for pair in pairs {
                    if !set_las_field(&mut point, &lower(&pair[0]), &pair[1]) {
                        self.bad_las_fields += 1;
                    }
                }
            }
            points.push(point);
            if points.len() > self.options.max_values_per_block {
                self.fail(line, "the point cloud holds more points than the limit");
                return false;
            }
        }
        true
    }

    // ---- tins ------------------------------------------------------------------

    fn read_tin(&mut self, full: bool) {
        let mut tin = Tin {
            full,
            colour: self.colour.clone(),
            ..Tin::default()
        };
        let what = if full { "full_tin" } else { "tin" };
        let start_line = self.peek().line;
        if !self.expect_open(what) {
            return;
        }
        self.path.push(String::from(what));
        let mut triangles = Vec::new();
        let mut neighbours = Vec::new();
        let mut nulling = Vec::new();

        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            let block = self.peek().kind == TokenKind::Open;
            if !key.is_word() {
                self.read_unknown(&key, &word, &mut tin.extras, false);
            } else if word == "name" && !block {
                tin.name = self.next_value(&word);
            } else if (word == "colour" || word == "color") && !block {
                tin.colour = self.next_colour(&word);
            } else if word == "attributes" && block {
                self.read_attributes(&mut tin.attributes);
            } else if word == "points" && block {
                self.read_vertex_rows(&word, 3, &mut tin.points, None);
            } else if word == "triangles" && block {
                self.read_triangles(&mut triangles, &mut neighbours);
            } else if word == "neighbours" && block {
                self.read_integer_block(&word, &mut neighbours);
            } else if word == "nulling" && block {
                self.read_integer_block(&word, &mut nulling);
            } else if word == "colours" && block {
                self.read_text_block(&word, &mut tin.colours);
            } else if word == "input" && block {
                self.read_tin_input(&mut tin);
            } else {
                if word == "transformation" && block {
                    self.warn(format!(
                        "tin \"{}\" carries a transformation block, which the manual does not define; its points are read as written",
                        tin.name
                    ));
                }
                self.read_unknown(&key, &word, &mut tin.extras, false);
            }
        }
        self.expect_close(what);
        self.path.pop();
        if self.failed() {
            return;
        }

        // Manual 1.4.7: points and triangles are MANDATORY in both forms;
        // neighbours and nulling in a full_tin. Absent, they default - to no
        // surface, or to every triangle visible - and a default that quiet
        // would be a mistake hidden (PLAN.MD section 36).
        self.mandatory(!tin.points.is_empty(), what, &tin.name, "points");
        self.mandatory(!triangles.is_empty(), what, &tin.name, "triangles");
        if full {
            self.mandatory(!neighbours.is_empty(), what, &tin.name, "neighbours");
            self.mandatory(!nulling.is_empty(), what, &tin.name, "nulling");
        }
        let null_value = self.archive.null_value;
        apply_null_all(&mut tin.points, null_value);
        if !self.build_triangles(
            what,
            start_line,
            tin.points.len(),
            &triangles,
            &mut tin.triangles,
        ) {
            return;
        }
        if !neighbours.is_empty() {
            if neighbours.len() != triangles.len() {
                self.warn(format!(
                    "{what} \"{}\": neighbours holds {} rows for {} triangles; ignored",
                    tin.name,
                    neighbours.len() / 3,
                    tin.triangles.len()
                ));
            } else {
                let count = tin.triangles.len();
                let (rows, _) = neighbours.as_chunks::<3>();
                for values in rows {
                    tin.neighbours.push(values.map(|value| {
                        if value <= 0 || value as usize > count {
                            Tin::NO_NEIGHBOUR
                        } else {
                            (value - 1) as u32
                        }
                    }));
                }
            }
        }
        if !nulling.is_empty() {
            if nulling.len() != tin.triangles.len() {
                self.warn(format!(
                    "{what} \"{}\": nulling holds {} flags for {} triangles; triangles without a flag are taken as visible",
                    tin.name,
                    nulling.len(),
                    tin.triangles.len()
                ));
            }
            tin.visible.reserve(nulling.len());
            for &flag in &nulling {
                tin.visible.push(flag == 2);
            }
            tin.visible.resize(tin.triangles.len(), true);
        }
        if tin.name.is_empty() {
            self.warn(format!(
                "{what} at line {start_line} has no name, which the manual makes mandatory"
            ));
        }
        if !tin.colours.is_empty() && tin.colours.len() != tin.triangles.len() {
            self.warn(format!(
                "{what} \"{}\": colours holds {} entries for {} triangles",
                tin.name,
                tin.colours.len(),
                tin.triangles.len()
            ));
        }
        self.add_element(Element::Tin(tin));
    }

    fn mandatory(&mut self, present: bool, what: &str, name: &str, block: &str) {
        if !present {
            self.warn(format!(
                "{what} \"{name}\" has no {block} block, which the manual makes mandatory"
            ));
        }
    }

    // `triangles { 1 2 3 ... }` as the manual has it, or with the indices in a
    // `vertices { }` block of their own as some writers produce.
    fn read_triangles(&mut self, triangles: &mut Vec<i64>, neighbours: &mut Vec<i64>) {
        if !self.expect_open("triangles") {
            return;
        }
        self.path.push(String::from("triangles"));
        while !self.at_block_end() {
            let peeked = self.peek();
            if peeked.kind == TokenKind::Word && parse_integer(peeked.raw).is_some() {
                self.read_integers_until_close("triangles", triangles);
                continue;
            }
            let key = self.next();
            let word = lower(key.raw);
            if self.peek().kind != TokenKind::Open {
                self.fail(
                    key.line,
                    format!("'{}' in triangles is not a point number", key.raw),
                );
                self.path.pop();
                return;
            }
            if word == "vertices" {
                self.read_integer_block(&word, triangles);
            } else if word == "neighbours" {
                self.read_integer_block(&word, neighbours);
            } else {
                self.skip_unknown_block(&word);
            }
        }
        self.expect_close("triangles");
        self.path.pop();
    }

    // One-based indices in rows of three -> zero-based triangles. An index
    // outside the points is a failure: the triangle cannot be drawn, and one
    // bad index usually means every later one is wrong too.
    fn build_triangles(
        &mut self,
        what: &str,
        line: u32,
        point_count: usize,
        indices: &[i64],
        out: &mut Vec<[u32; 3]>,
    ) -> bool {
        if !indices.len().is_multiple_of(3) {
            self.fail(
                line,
                format!(
                    "{what} lists {} point numbers, which is not a whole number of triangles",
                    indices.len()
                ),
            );
            return false;
        }
        // The manual numbers points from 1. A file with a 0 in it was written
        // by something that counts from 0, and says so by containing one.
        let zero_based = indices.contains(&0);
        if zero_based {
            self.warn(format!(
                "{what} at line {line} numbers its points from 0, not from 1 as the manual requires; read as such"
            ));
        }
        out.reserve(indices.len() / 3);
        let (rows, _) = indices.as_chunks::<3>();
        for (number, row) in rows.iter().enumerate() {
            let mut triangle = [0_u32; 3];
            for (slot, &written) in triangle.iter_mut().zip(row) {
                // A minimum integer less one is undefined in C++; wrapped here,
                // it is past the points and refused as the C++ would refuse it.
                let index = written.wrapping_sub(if zero_based { 0 } else { 1 });
                if index < 0 || index as usize >= point_count {
                    self.fail(
                        line,
                        format!(
                            "{what}: triangle {} names point {written} but there are {point_count} points",
                            number + 1
                        ),
                    );
                    return false;
                }
                *slot = index as u32;
            }
            out.push(triangle);
        }
        true
    }

    fn read_tin_input(&mut self, tin: &mut Tin) {
        if !self.expect_open("input") {
            return;
        }
        self.path.push(String::from("input"));
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            if word == "models" && self.peek().kind == TokenKind::Open {
                self.read_text_block(&word, &mut tin.input_models);
            } else {
                self.read_unknown(&key, &word, &mut tin.input, false);
            }
        }
        self.expect_close("input");
        self.path.pop();
    }

    fn read_super_tin(&mut self) {
        let mut super_tin = SuperTin {
            colour: self.colour.clone(),
            ..SuperTin::default()
        };
        if !self.expect_open("super_tin") {
            return;
        }
        self.path.push(String::from("super_tin"));
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            let block = self.peek().kind == TokenKind::Open;
            if key.is_word() && word == "name" && !block {
                super_tin.name = self.next_value(&word);
            } else if key.is_word() && (word == "colour" || word == "color") && !block {
                super_tin.colour = self.next_colour(&word);
            } else if key.is_word() && word == "attributes" && block {
                self.read_attributes(&mut super_tin.attributes);
            } else if key.is_word() && word == "tins" && block {
                self.read_text_block(&word, &mut super_tin.tins);
            } else {
                self.read_unknown(&key, &word, &mut super_tin.extras, false);
            }
        }
        self.expect_close("super_tin");
        self.path.pop();
        if !self.failed() {
            // Manual 1.4.8: the name and the tins block are mandatory.
            if super_tin.name.is_empty() {
                self.warn(String::from(
                    "super_tin has no name, which the manual makes mandatory",
                ));
            }
            if super_tin.tins.is_empty() {
                self.warn(format!(
                    "super_tin \"{}\" has no tins block, which the manual makes mandatory",
                    super_tin.name
                ));
            }
            self.add_element(Element::SuperTin(super_tin));
        }
    }

    // ---- trimeshes -------------------------------------------------------------

    fn read_primitive_3d(&mut self) {
        let mut mesh = Trimesh {
            model: self.model.clone(),
            colour: self.colour.clone(),
            ..Trimesh::default()
        };
        let start_line = self.peek().line;
        if !self.expect_open("primitive_3d") {
            return;
        }
        self.path.push(String::from("primitive_3d"));
        let mut saw_mesh = false;
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            let block = self.peek().kind == TokenKind::Open;
            if key.is_word() && word == "name" && !block {
                mesh.name = self.next_value(&word);
            } else if key.is_word() && word == "model" && !block {
                mesh.model = self.next_value(&word); // registered by add_element
            } else if key.is_word() && (word == "colour" || word == "color") && !block {
                mesh.colour = self.next_colour(&word);
            } else if key.is_word() && word == "attributes" && block {
                self.read_attributes(&mut mesh.attributes);
            } else if key.is_word() && word == "trimesh_3d" && block {
                saw_mesh = true;
                self.read_trimesh(&mut mesh, start_line);
            } else {
                if word == "transformation" && block {
                    self.warn(format!(
                        "trimesh \"{}\" carries a transformation block, which the manual does not define; its vertices are read as written",
                        mesh.name
                    ));
                }
                self.read_unknown(&key, &word, &mut mesh.extras, false);
            }
        }
        self.expect_close("primitive_3d");
        self.path.pop();
        if self.failed() {
            return;
        }
        if !saw_mesh {
            self.warn(format!(
                "primitive_3d at line {start_line} holds no trimesh_3d"
            ));
        }
        let null_value = self.archive.null_value;
        apply_null_all(&mut mesh.vertices, null_value);
        self.add_element(Element::Trimesh(mesh));
    }

    fn read_trimesh(&mut self, mesh: &mut Trimesh, start_line: u32) {
        if !self.expect_open("trimesh_3d") {
            return;
        }
        self.path.push(String::from("trimesh_3d"));
        let mut faces = Vec::new();
        let mut edges = Vec::new();
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            let block = self.peek().kind == TokenKind::Open;
            if !key.is_word() {
                continue;
            }
            if word == "vertices" && block {
                self.read_vertex_rows(&word, 3, &mut mesh.vertices, None);
            } else if word == "faces" && block {
                self.read_integer_block(&word, &mut faces);
            } else if word == "edges" && block {
                self.read_integer_block(&word, &mut edges);
            } else if word == "info" && block {
                self.read_field_block(&word, &mut mesh.info, None, None);
            } else if word == "blend" && !block {
                mesh.blend = self.next_nullable_real(&word);
            } else if word == "vertex_infos" && block {
                self.read_infos(&word, &mut mesh.vertex_infos);
            } else if word == "edge_infos" && block {
                self.read_infos(&word, &mut mesh.edge_infos);
            } else if word == "face_infos" && block {
                self.read_infos(&word, &mut mesh.face_infos);
            } else if word == "vertex_flags" && block {
                self.read_index_block(&word, &mut mesh.vertex_flags);
            } else if word == "edge_flags" && block {
                self.read_index_block(&word, &mut mesh.edge_flags);
            } else if word == "face_flags" && block {
                self.read_index_block(&word, &mut mesh.face_flags);
            } else {
                self.read_unknown(&key, &word, &mut mesh.extras, false);
            }
        }
        self.expect_close("trimesh_3d");
        self.path.pop();
        if self.failed() {
            return;
        }
        // Manual 1.4.9: vertices and faces are MANDATORY.
        if mesh.vertices.is_empty() {
            self.warn(format!(
                "trimesh \"{}\" has no vertices block, which the manual makes mandatory",
                mesh.name
            ));
        }
        if faces.is_empty() {
            self.warn(format!(
                "trimesh \"{}\" has no faces block, which the manual makes mandatory",
                mesh.name
            ));
        }
        self.build_triangles(
            "trimesh_3d",
            start_line,
            mesh.vertices.len(),
            &faces,
            &mut mesh.faces,
        );
        // A flag is a ONE-based index into its info table, 0 meaning no info:
        // the manual's example (1.4.9) has two infos and the flags 2 0 1 2 0,
        // which reads no other way, and the writing program's own exports
        // agree.
        let checks = [
            (&mesh.vertex_flags, mesh.vertex_infos.len(), "vertex_flags"),
            (&mesh.edge_flags, mesh.edge_infos.len(), "edge_flags"),
            (&mesh.face_flags, mesh.face_infos.len(), "face_flags"),
        ];
        for (flags, infos, what) in checks {
            if flags.iter().any(|&flag| flag as usize > infos) {
                self.warn(format!(
                    "trimesh \"{}\": {what} names an info that does not exist",
                    mesh.name
                ));
            }
        }
        if edges.len().is_multiple_of(2) {
            let count = mesh.vertices.len() as i64;
            let (pairs, _) = edges.as_chunks::<2>();
            for pair in pairs {
                // Edges are "for checking only" (manual 1.4.9): a bad one is
                // dropped, not fatal.
                let a = pair[0].wrapping_sub(1);
                let b = pair[1].wrapping_sub(1);
                if a >= 0 && b >= 0 && a < count && b < count {
                    mesh.edges.push([a as u32, b as u32]);
                }
            }
        }
    }

    // flag key colour name, four to a row
    fn read_infos(&mut self, what: &str, out: &mut Vec<TrimeshInfo>) {
        let line = self.peek().line;
        let mut tokens = Vec::new();
        if !self.read_text_block(what, &mut tokens) {
            return;
        }
        if !tokens.len().is_multiple_of(4) {
            self.warn(format!(
                "line {line}: {what} is not rows of 'flag key colour name'; ignored"
            ));
            return;
        }
        let (rows, _) = tokens.as_chunks::<4>();
        for row in rows {
            out.push(TrimeshInfo {
                flag: parse_integer(&row[0]).unwrap_or(0),
                key: parse_integer(&row[1]).unwrap_or(0),
                colour: row[2].clone(),
                name: row[3].clone(),
            });
        }
    }

    fn read_index_block(&mut self, what: &str, out: &mut Vec<u32>) {
        let mut values = Vec::new();
        if !self.read_integer_block(what, &mut values) {
            return;
        }
        out.reserve(values.len());
        for &value in &values {
            // A value past 32 bits keeps its low bits, as the C++ cast does.
            out.push(if value < 0 { 0 } else { value as u32 });
        }
    }

    // ---- top level ---------------------------------------------------------------

    // Registers a model and returns the ONE spelling it is known by. Manual
    // 1.1: leading and trailing spaces are ignored and case is stored but not
    // compared, so " Fred " and "FRED" are the model "Fred" if that came
    // first - and the importer, which keys layers on the string, must see one
    // name.
    fn note_model(&mut self, name: &str) -> String {
        let trimmed = trimmed(name);
        let key = lower(trimmed);
        if let Some(&known) = self.model_index.get(&key) {
            return self.archive.model_names[known].clone();
        }
        self.archive.model_names.push(String::from(trimmed));
        self.model_index
            .insert(key, self.archive.model_names.len() - 1);
        String::from(trimmed)
    }

    // Every element goes in through here, so that the model it lands in is
    // registered when it is USED - not before its own `model` field, which the
    // manual puts inside the string, has been read. Otherwise a file whose
    // strings all name their model would also gain the default "data".
    fn add_element(&mut self, mut element: Element) {
        let model = match &mut element {
            Element::VertexString(string) => Some(&mut string.header.model),
            Element::ArcString(arc) => Some(&mut arc.header.model),
            Element::CircleString(circle) => Some(&mut circle.header.model),
            Element::TextString(text) => Some(&mut text.header.model),
            Element::PlotFrame(frame) => Some(&mut frame.header.model),
            Element::DrainageString(drainage) => Some(&mut drainage.header.model),
            Element::SuperAlignment(alignment) => Some(&mut alignment.header.model),
            Element::LasCloud(cloud) => Some(&mut cloud.header.model),
            Element::DraftingLeader(leader) => Some(&mut leader.header.model),
            Element::Trimesh(mesh) => Some(&mut mesh.model),
            Element::Tin(_) | Element::SuperTin(_) => None,
        };
        if let Some(model) = model {
            *model = self.note_model(model);
        }
        self.archive.elements.push(element);
    }

    fn read_model_block(&mut self) {
        let mut record = ModelRecord::default();
        if !self.expect_open("model") {
            return;
        }
        self.path.push(String::from("model"));
        while !self.at_block_end() {
            let key = self.next();
            let word = lower(key.raw);
            let block = self.peek().kind == TokenKind::Open;
            if key.is_word() && word == "name" && !block {
                record.name = self.next_value(&word);
            } else if key.is_word() && word == "attributes" && block {
                self.read_attributes(&mut record.attributes);
            } else {
                self.read_unknown(&key, &word, &mut record.extras, false);
            }
        }
        self.expect_close("model");
        self.path.pop();
        if self.failed() {
            return;
        }
        if !record.name.is_empty() {
            self.model = self.note_model(&record.name);
            record.name = self.model.clone();
        }
        self.archive.models.push(record);
    }

    fn read_string(&mut self, line: u32) {
        let kind = lower(&self.next_value("string"));
        if self.failed() {
            return;
        }
        if self.peek().kind != TokenKind::Open {
            self.fail(line, format!("expected '{{' after 'string {kind}'"));
            return;
        }
        self.lexer.collect_comments = false;

        for (name, string_kind) in VERTEX_KINDS {
            if kind == name {
                self.next(); // the '{'
                self.read_vertex_string(string_kind);
                return;
            }
        }
        if matches!(
            kind.as_str(),
            "arc"
                | "circle"
                | "feature"
                | "text"
                | "plot_frame"
                | "drainage"
                | "super_alignment"
                | "alignment"
                | "pipeline"
                | "las_cloud_data"
        ) {
            self.next(); // the '{'
        }
        match kind.as_str() {
            "arc" => self.read_arc_string(),
            "circle" | "feature" => self.read_circle_string(kind == "feature"),
            "text" => self.read_text_string(),
            "plot_frame" => self.read_plot_frame(),
            "drainage" => self.read_drainage_string(),
            "super_alignment" => self.read_super_alignment(),
            "alignment" => self.read_legacy_alignment(AlignmentSource::Alignment),
            "pipeline" => self.read_legacy_alignment(AlignmentSource::Pipeline),
            "las_cloud_data" => self.read_las_cloud(),
            _ => self.skip_unknown_block(&format!("string {kind}")),
        }
    }

    fn read_top_level(&mut self) {
        while !self.failed() {
            let token = self.next();
            if token.kind == TokenKind::End {
                break;
            }
            if token.kind == TokenKind::Close {
                self.fail(token.line, "a '}' closes nothing");
                break;
            }
            if token.kind == TokenKind::Open {
                // Put back in spirit: count it as a block with no keyword.
                *self
                    .archive
                    .unrecognised
                    .entry(String::from("(no keyword)"))
                    .or_insert(0) += 1;
                self.skip_rest_of_block(token.line);
                continue;
            }
            if token.kind == TokenKind::Quoted {
                self.warn(format!(
                    "line {}: stray text \"{}\" between elements ignored",
                    token.line,
                    token.text()
                ));
                continue;
            }

            let word = lower(token.raw);
            let block = self.peek().kind == TokenKind::Open;
            if word == "model" {
                if block {
                    self.read_model_block();
                } else {
                    let name = self.next_value(&word);
                    self.model = self.note_model(&name);
                }
            } else if (word == "colour" || word == "color") && !block {
                self.colour = self.next_colour(&word);
            } else if word == "style" && !block {
                self.style = self.next_value(&word);
            } else if word == "breakline" && !block {
                self.breakline = self.read_breakline();
            } else if word == "null" && !block {
                // `null -999` sets the value; `null null`, which the writing
                // program puts when it is using the keyword, changes nothing.
                let value = self.next();
                if let Some(number) = parse_real(value.raw)
                    && value.is_word()
                {
                    self.archive.null_value = number;
                }
            } else if word == "string" {
                self.read_string(token.line);
            } else if word == "tin" && block {
                self.lexer.collect_comments = false;
                self.read_tin(false);
            } else if word == "full_tin" && block {
                self.lexer.collect_comments = false;
                self.read_tin(true);
            } else if word == "super_tin" && block {
                self.lexer.collect_comments = false;
                self.read_super_tin();
            } else if word == "primitive_3d" && block {
                self.lexer.collect_comments = false;
                self.read_primitive_3d();
            } else if word == "drafting_element" && block {
                self.lexer.collect_comments = false;
                self.read_drafting_element();
            } else if word == "project_attributes" && block {
                let mut attributes = std::mem::take(&mut self.archive.project_attributes);
                self.read_attributes(&mut attributes);
                self.archive.project_attributes = attributes;
            } else if word == "text" && self.peek().kind == TokenKind::Quoted {
                self.read_short_text(token.line);
            } else if block {
                self.skip_unknown_block(&word);
            } else {
                self.warn(format!(
                    "line {}: '{}' is not a 12da command; ignored",
                    token.line, token.raw
                ));
            }

            if self.archive.elements.len() > self.options.max_elements {
                let limit = self.options.max_elements;
                self.fail(
                    token.line,
                    format!("the file holds more elements than the limit of {limit}"),
                );
            }
        }
    }

    // After a `{` has already been consumed.
    fn skip_rest_of_block(&mut self, open_line: u32) {
        let target = self.lexer.depth() - 1;
        while !self.failed() {
            let token = self.next();
            if token.kind == TokenKind::End {
                self.fail(open_line, "a '{' is never closed");
                return;
            }
            if token.kind == TokenKind::Close && self.lexer.depth() == target {
                return;
            }
        }
    }

    // The writing program's export settings, written as `// key value`
    // comments.
    fn read_header_settings(&mut self) {
        let comments = std::mem::take(&mut self.lexer.comments);
        for comment in comments {
            let body = trimmed(comment);
            let Some(space) = body.find([' ', '\t']) else {
                continue;
            };
            let key = &body[..space];
            let is_setting = matches!(
                key,
                "archive_version"
                    | "decimal_places"
                    | "null"
                    | "null_keyword"
                    | "do_null"
                    | "dereference"
                    | "project"
                    | "model_attributes_mode"
            ) || key.starts_with("output_");
            if !is_setting {
                continue;
            }
            let mut value = trimmed(&body[space..]);
            let is_quoted = value.len() >= 2 && value.starts_with('"') && value.ends_with('"');
            if is_quoted {
                value = &value[1..value.len() - 1];
            }
            self.archive.header_settings.push(Field {
                key: String::from(key),
                value: String::from(value),
                quoted: is_quoted,
            });
        }
    }

    fn run(mut self) -> Result<Archive> {
        self.read_top_level();
        if self.lexer.unterminated_quote() {
            // Whatever else was reported - usually a missing '}' - is a
            // symptom: the text swallowed the rest of the file, braces
            // included.
            self.error = None;
            let line = self.lexer.unterminated_quote_line();
            self.fail(line, "a text in double quotes is never closed");
        }
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        self.read_header_settings();
        if self.null_attributes != 0 {
            self.warn(format!(
                "{} attributes had a null or unreadable value and were not read",
                self.null_attributes
            ));
        }
        if self.suppressed_warnings != 0 {
            self.archive.warnings.push(format!(
                "{} further warnings were not listed",
                self.suppressed_warnings
            ));
        }
        Ok(self.archive)
    }
}
