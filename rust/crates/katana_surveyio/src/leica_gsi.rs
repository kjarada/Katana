//! Leica GSI-8 and GSI-16: the probe, the reader and the format's registration.
//! What an import produces is described in leica.rs.

// THE SPECIFICATION is Leica Geosystems, "GSI ONLINE for Leica TPS and DNA",
// November 2003: the data word and the word information table on pages 5-6,
// the TPS1000/1100/2000/5000 word indices in the PUT and GET tables on pages
// 35-37, the DNA (levelling) words on pages 40-42; its SET/CONF tables list
// the instrument settings no word records (171, the horizontal circle's
// direction). Where a code block is stored relative to its point is another
// such setting, from Leica's "TPS1200 Technical Reference Manual", version
// 5.0 (see codes_recorded_after_points). A word is
//
//     WI  info  sign  data  blank
//     positions 1-2   word index (a DNA level also writes three-digit ones)
//     position  3     no significance - except in words 11 and 41, where
//                     positions 3-6 are the block number
//     position  4     automatic index (the vertical compensator), angle words
//     position  5     input mode: 0 measured, 1 keyed in, 2 / 3 measured with
//                     the horizontal correction on / off, 4 computed on board
//     position  6     units: 0 m (last digit 1 mm), 1 ft (1/1000 ft), 2 gon,
//                     3 decimal degrees, 4 sexagesimal, 5 mil, 6 m (1/10 mm),
//                     7 ft (1/10000 ft), 8 m (1/100 mm)
//     position  7     sign
//     then 8 (GSI-8) or 16 (GSI-16) characters of data, right justified and
//     padded with '0'. A GSI-16 block begins with '*'.
//
// THE DECIMAL POINT IS NOT WRITTEN: the unit digit places it
// ("81..00+00005387" is 5.387 m, "21.102+17920860" is 179.20860 gon). Angles
// have five decimals in gon and degrees (GET 21/22 examples) and in the
// sexagesimal DDD.MMSSs; a mil angle has four, the only scale at which a full
// circle of 6400 mil fits eight digits as 400 gon and 360 degrees do at five.
// A file that writes a point anyway (the owner's "84..00+8085.844") is read as
// written, and the import says it met one.
//
// SIXTY SECONDS. A sexagesimal word's minutes and seconds run to 59: GSI
// ONLINE's GET examples ("21.104+12149400" is 121 49 40.0) show nothing else,
// and it says nothing of 60. A writer that rounds the seconds on their own,
// without carrying into the minutes, writes 60 for a value within its
// rounding of the next minute. 60.0 seconds IS the next minute, exactly, so a
// word whose seconds are 60 with nothing after them is read as the next
// minute, carried in whole numbers before the angle becomes a real, and the
// import says once how many there were and where. That holds only where the
// word itself writes both digits of the seconds in their place: at its
// block's full width, or four digits after a written point. A short or long
// word is aligned from the right and a point's missing digits are supplied as
// zeros, and a 60 that alignment or padding made is refused. A traverse the
// owner sent has 24 such words; 19 agree with the other face of their round,
// and the other 5 with the same face's other readings of their target, only
// when read so (docs/survey.md, "Leica GSI"). Seconds past 60.0 and minutes
// past 59 are still refused: no file has shown either.
//
// A circle reading (words 21 and 22) runs to one full circle - the unit
// digit's 400 gon, 360 degrees or 6400 mil - and a word past that is refused,
// not wrapped. A full circle itself is the circle's zero, reached by rounding
// up at the top of the circle.
//
// A SIGN comes with every measured value (page 6: "+: Positive value, -:
// Negative value"), and what a negative one means differs by circle. Word 22
// is read as a zenith angle, and no zenith angle is negative, so a negative
// word 22 is refused at its record; wrapped onto the circle, -005 00 00 would
// be a face-right zenith of 5 degrees, the shot drawn near the vertical and
// half a turn round from where it was measured. What wrote it is not known: a
// damaged word, or an instrument whose vertical angle setting reached the
// file. GSI ONLINE lists such a setting for the TPS100 series (page 9, SET 44,
// "V angle READING": 0 Zenith, 1 Horizontal, 2 Slope in percent), under which
// a sight below the horizon reads negative, but does not say that word 22
// follows it. No setting gives word 21 another meaning - 171 only turns the
// circle clockwise or counterclockwise, 178 and 179 switch the Hz compensator
// and collimation - so a negative one is the direction it names, counted the
// other way from the circle's zero: -090 00 00 is read as 270 00 00. -0 is
// zero in either word.
//
// Everything here streams over the file's bytes: no regex, and nothing
// allocated per record beyond the values it produces. Text reaches the model
// as validated UTF-8 (docs/rust.md, "Text in the model is validated UTF-8"):
// every comparison - of point ids, of codes - is made on the bytes as the C++
// makes it, and a text becomes a String only where it leaves for the model or
// a message, as UTF-8 when it is UTF-8 and as Windows-1252 when it is not.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++ oracle's negated comparisons refuse NaN precisions, as they must."
)]

use std::collections::{BTreeMap, HashMap};

use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result};
use katana_math::unit_ratio::{to_metres, units};
use katana_math::vec3::Vec3;
use katana_survey::data_model::distance_sigma;
use katana_survey::{
    AngularUnit, CoordinateSource, CorrectionState, DistanceKind, DistanceObservation, Face,
    HorizontalDirectionObservation, InstrumentSettings, LevelDifferenceObservation, LinearUnit,
    Observation, ObservationPrecision, Pointing, SourceRecord, Station, SurveyFeature, SurveyPoint,
    SurveyStation, SurveyTimestamp, TargetInfo, UnpositionedPoint, ZenithAngleObservation,
};

use crate::detect::without_byte_order_mark;
use crate::format::{
    FormatContent, FormatDescriptor, FormatRegistration, FormatSignature, Manufacturer, ProbeInput,
    ReadOptions, ReadResult, ReadWarning, ruled_out,
};
use crate::leica::LEICA_GSI_FORMAT_ID;

// The version of THIS code (format.rs). Bump it when a file would import
// differently, so the SourceRecords of old imports say which reading they had.
// 1.1 reads 60-second angle words as the next minute, an all-zero code or
// remark word as empty, a backsight the file positions later, and code
// information in a point's own block; it refuses a circle reading past a full
// circle, which 1.0 wrapped onto it. 1.2 refuses a negative vertical reading
// and a sexagesimal word with no digit, which 1.1 read as angles, and carries
// a block's own horizontal distance and height difference (32, 33) where no
// zenith angle beside its slope distance was read, or no distance beside its
// zenith angle, where 1.1 dropped them as values the reduction computes.
const PARSER_VERSION: &str = "1.2";

const PI: f64 = std::f64::consts::PI;
const TWO_PI: f64 = 2.0 * std::f64::consts::PI;

// Record-level warnings listed one by one before the rest are only counted. A
// damaged file can have a problem on every line, and a list of a million
// warnings is not one a person reads; the count still says how many there were.
const MAX_LISTED_WARNINGS: usize = 1000;

// Two coordinates for one point that agree to half the last digit of a
// millimetre word are the same coordinates written twice.
const SAME_POSITION_TOLERANCE: f64 = 0.0005;

// A note for the file names the records it is about, up to this many, and
// counts the rest: enough to find each in a job with a few dozen, without a
// message of thousands of numbers when a writer did something on every line.
const LISTED_RECORDS: usize = 100;

// Bytes from the file as text for the model or a message.
fn text(bytes: &[u8]) -> String {
    utf8_or_windows1252(bytes)
}

// ---- Words -----------------------------------------------------------------------

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

#[derive(Clone, Copy, Debug)]
struct Word<'a> {
    /// The word index: 11, 21, ... or 330 for a DNA word.
    index: i32,
    /// Positions 3-6 (4-6 after a three-digit index).
    info: &'a [u8],
    sign: u8,
    /// 8 or 16 characters in a well-formed file.
    data: &'a [u8],
}

// Positions 4, 5 and 6 of the word, '.' where the file leaves them empty.
fn info_at(word: &Word<'_>, position: usize) -> u8 {
    // `info` starts at position 3, or at position 4 after a three-digit index.
    let first = if word.info.len() == 4 { 3 } else { 4 };
    if position < first || position - first >= word.info.len() {
        return b'.';
    }
    word.info[position - first]
}

fn unit_digit(word: &Word<'_>) -> u8 {
    info_at(word, 6)
}

fn input_mode(word: &Word<'_>) -> u8 {
    info_at(word, 5)
}

// One blank-separated token as a GSI word, or None when it is not one.
fn parse_word(token: &[u8]) -> Option<Word<'_>> {
    if token.len() < 8 || !is_digit(token[0]) || !is_digit(token[1]) {
        return None;
    }
    let mut index = i32::from(token[0] - b'0') * 10 + i32::from(token[1] - b'0');
    let mut info_start = 2;
    // Words 11 and 41 carry the block number in positions 3-6, so a digit
    // there is theirs; anywhere else a third digit is a DNA three-digit index
    // (page 40: "a two or three character Word Index").
    if index != 11 && index != 41 && is_digit(token[2]) {
        index = index * 10 + i32::from(token[2] - b'0');
        info_start = 3;
    }
    let sign = token[6];
    if sign != b'+' && sign != b'-' {
        return None;
    }
    let info = &token[info_start..6];
    let data = &token[7..];
    // 16 is GSI-16's width; more than that is not a GSI word and could not be
    // held in the integer the number is read into.
    if data.is_empty() || data.len() > 16 {
        return None;
    }
    Some(Word {
        index,
        info,
        sign,
        data,
    })
}

// Text data without the '0' padding the specification right-justifies it
// with: "0000A110" is point A110, "00000013" is code 13 (GET 11 and 41).
fn text_of(data: &[u8]) -> &[u8] {
    match data.iter().position(|&c| c != b'0') {
        Some(first) => &data[first..],
        None => &data[data.len() - 1..],
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Number {
    value: f64,
    /// The file wrote a decimal point itself.
    explicit_point: bool,
}

// The digits of a numeric word, signed; the unit digit's scale is NOT applied
// unless the file wrote its own point.
fn number_of(data: &[u8], sign: u8) -> Option<Number> {
    let mut points = 0usize;
    for &c in data {
        if c == b'.' {
            points += 1;
        } else if !is_digit(c) {
            return None;
        }
    }
    if points > 1 || data == b"." {
        return None;
    }
    let mut number = Number::default();
    if points == 1 {
        // Digits and one point: std::from_chars' fixed format, correctly
        // rounded, as Rust's parse is.
        number.value = std::str::from_utf8(data).ok()?.parse().ok()?;
        number.explicit_point = true;
    } else {
        // At most 16 digits: parse_word refuses more.
        let mut digits: i64 = 0;
        for &c in data {
            digits = digits * 10 + i64::from(c - b'0');
        }
        number.value = digits as f64;
    }
    if sign == b'-' {
        number.value = -number.value;
    }
    Some(number)
}

// Division by an exact power of ten, so that an integer word becomes the double
// nearest its decimal value: 8085844 / 1000 is the same double as "8085.844".
const POWERS_OF_TEN: [f64; 6] = [1.0, 10.0, 100.0, 1000.0, 10000.0, 100000.0];

fn wrap_to_circle(radians: f64) -> f64 {
    let mut wrapped = radians % TWO_PI;
    if wrapped < 0.0 {
        wrapped += TWO_PI;
    }
    if wrapped >= TWO_PI { 0.0 } else { wrapped }
}

// What a word index is, for the messages about words this reader does not use.
fn word_name(index: usize) -> &'static str {
    match index {
        17 => "date",
        25 => "horizontal circle difference",
        26 => "offset",
        27 => "vertical correction",
        28 => "horizontal correction",
        52 => "number of measurements and standard deviation",
        53 => "average",
        95 => "instrument temperature",
        // DNA: staff readings, levelling results
        300.. => "a digital level's word",
        _ => "not defined for total stations",
    }
}

// ---- One block, as its words give it ---------------------------------------------

// A sexagesimal word whose seconds were 60, read as the next minute (SIXTY
// SECONDS, at the top): enough to show it as written and as read.
#[derive(Clone, Copy, Debug, Default)]
struct SixtySeconds {
    word: i32,
    negative: bool,
    /// As written.
    degrees: i64,
    /// As written; the seconds were 60.
    minutes: i32,
    /// The digits written after the seconds, all 0.
    zeros: usize,
}

fn two_digits(text: &mut String, part: i32) {
    // 0..60, so each digit is one character.
    text.push(char::from(b'0' + (part / 10) as u8));
    text.push(char::from(b'0' + (part % 10) as u8));
}

// "word 22, 086 27 60.0, read as 086 28 00.0"
fn sixty_text(sixty: &SixtySeconds) -> String {
    let angle = |degrees: i64, minutes: i32, seconds: i32| {
        let whole = format!("{degrees}");
        let mut text = String::from(if sixty.negative { "-" } else { "" });
        text.push_str(&"0".repeat(3usize.saturating_sub(whole.len())));
        text.push_str(&whole);
        for part in [minutes, seconds] {
            text.push(' ');
            two_digits(&mut text, part);
        }
        if sixty.zeros != 0 {
            text.push('.');
            text.push_str(&"0".repeat(sixty.zeros));
        }
        text
    };
    let next_degree = sixty.minutes == 59;
    format!(
        "word {}, {}, read as {}",
        sixty.word,
        angle(sixty.degrees, sixty.minutes, 60),
        angle(
            sixty.degrees + i64::from(next_degree),
            if next_degree { 0 } else { sixty.minutes + 1 },
            0
        )
    )
}

#[derive(Clone, Debug, Default)]
struct Block<'a> {
    record: usize,
    /// GSI-16.
    wide: bool,
    /// 11: a point's block; 41: a code block.
    kind: i32,
    /// The point id, or the code of a code block.
    name: &'a [u8],

    hz: Option<f64>,
    v: Option<f64>,
    // Set where hz or v was a 60-second word; counted once the value is kept.
    hz_sixty: Option<SixtySeconds>,
    v_sixty: Option<SixtySeconds>,
    slope: Option<f64>,
    horizontal: Option<f64>,
    height_difference: Option<f64>,
    hz_input_mode: u8,
    v_index: u8,

    /// 81-83: the target.
    e: Option<f64>,
    n: Option<f64>,
    h: Option<f64>,
    target_input_mode: u8,
    /// 84-86: the station.
    e0: Option<f64>,
    n0: Option<f64>,
    h0: Option<f64>,
    station_input_mode: u8,
    /// 87.
    reflector_height: Option<f64>,
    /// 88.
    instrument_height: Option<f64>,
    /// Letters in word 87 or 88.
    height_text: &'a [u8],
    height_text_word: i32,

    ppm: Option<f64>,
    /// Metres.
    prism_constant: Option<f64>,

    /// 71..79, read in a point's block.
    remarks: [&'a [u8]; 9],
    /// 42..49, in a code block or a point's.
    code_info: [&'a [u8]; 8],
    /// 12.
    serial: &'a [u8],
    /// 13.
    type_: &'a [u8],
    time18: &'a [u8],
    time19: &'a [u8],
    /// Any measurement word, for a code block's check.
    measurement_words: bool,
    /// Holds a digital level's three-digit word.
    levelling: bool,
}

impl Block<'_> {
    fn new(record: usize) -> Self {
        Self {
            record,
            hz_input_mode: b'.',
            v_index: b'.',
            target_input_mode: b'.',
            station_input_mode: b'.',
            ..Default::default()
        }
    }

    fn measures(&self) -> bool {
        self.hz.is_some()
            || self.v.is_some()
            || self.slope.is_some()
            || self.horizontal.is_some()
            || self.height_difference.is_some()
    }

    fn station_coordinates(&self) -> bool {
        self.e0.is_some() || self.n0.is_some() || self.h0.is_some()
    }

    fn target_coordinates(&self) -> bool {
        self.e.is_some() || self.n.is_some() || self.h.is_some()
    }
}

// A count of one kind of thing the file does many times, reported ONCE with
// where it first happened rather than once per record.
#[derive(Clone, Debug, Default)]
struct Tally {
    count: usize,
    first_record: usize,
    example: Vec<u8>,
}

impl Tally {
    fn add(&mut self, record: usize, text: &[u8]) {
        if self.count == 0 {
            self.first_record = record;
            self.example = text.to_vec();
        }
        self.count += 1;
    }

    // add(), with an example that has to be built: `make` runs for the first
    // occurrence only, so a thousand of them cost one string.
    fn add_made(&mut self, record: usize, make: impl FnOnce() -> String) {
        if self.count == 0 {
            let example = make();
            self.add(record, example.as_bytes());
        } else {
            self.add(record, b"");
        }
    }

    fn example_text(&self) -> String {
        text(&self.example)
    }
}

// The records a note for the file is about: the first LISTED_RECORDS named,
// the rest counted. A record added twice in a row is one record.
#[derive(Clone, Debug, Default)]
struct RecordList {
    listed: Vec<usize>,
    count: usize,
    last: usize,
}

impl RecordList {
    fn add(&mut self, record: usize) {
        if self.count != 0 && record == self.last {
            return;
        }
        self.last = record;
        self.count += 1;
        if self.listed.len() < LISTED_RECORDS {
            self.listed.push(record);
        }
    }

    // "record 4", "records 4, 5", "records 4, 5, ... and 20 more"
    fn text(&self) -> String {
        let mut out = String::from(if self.count == 1 {
            "record "
        } else {
            "records "
        });
        for (i, record) in self.listed.iter().enumerate() {
            if i != 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{record}"));
        }
        if self.count > self.listed.len() {
            out.push_str(&format!(" and {} more", self.count - self.listed.len()));
        }
        out
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

// std::to_chars(fixed, decimals) into a buffer of 64 characters: None when the
// text does not fit, as to_chars then fails.
fn chars_fixed(value: f64, decimals: usize) -> Option<String> {
    let text = if value.is_nan() {
        String::from(if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        })
    } else if value.is_infinite() {
        String::from(if value < 0.0 { "-inf" } else { "inf" })
    } else {
        // Rust's fixed digits are std::to_chars', the sign of a negative zero
        // or of a negative value rounded to zero included.
        format!("{value:.decimals$}")
    };
    (text.len() <= 64).then_some(text)
}

fn metres_text(metres: f64) -> String {
    let Some(mut text) = chars_fixed(metres, 4) else {
        return String::from("?");
    };
    while text.len() > 1 && text.ends_with('0') && text.as_bytes()[text.len() - 2] != b'.' {
        text.pop();
    }
    text
}

// The kind of block a line holds, judged by its first word as read_block judges
// it: 11 (a point's block), 41 (a code block), or 0 for anything else - a
// blank or unreadable line, or a digital level's special code block, which
// codes no point (GsiReader::code_block).
fn block_kind_of(line: &[u8]) -> i32 {
    let mut position = 0;
    loop {
        while position < line.len() && is_blank(line[position]) {
            position += 1;
        }
        if position >= line.len() {
            return 0;
        }
        let mut end = position;
        while end < line.len() && !is_blank(line[end]) {
            end += 1;
        }
        let mut token = &line[position..end];
        position = end;
        if token[0] == b'*' {
            token = &token[1..];
        }
        if token == b"\x1a" {
            continue; // a DOS end-of-file mark, as read_block skips it
        }
        let Some(word) = parse_word(token) else {
            return 0;
        };
        if word.index == 41 {
            return if text_of(word.data)[0] == b'?' { 0 } else { 41 };
        }
        return if word.index == 11 { 11 } else { 0 };
    }
}

fn is_line_end(c: u8) -> bool {
    c == b'\r' || c == b'\n'
}

// Whether the file's code blocks were recorded AFTER their points. GSI does
// not say: it is the instrument's <Rec Free Code: Before Point / After Point>
// setting (Leica TPS1200 Technical Reference Manual, version 5.0, 16.3 "Coding
// & Linework Settings" and 8.4 "Quick Coding"), and no word carries it. The
// one sign a file gives is at its ends. Recorded before its point, a code
// block comes first when the first point is coded and never comes last - a
// code with no point after it is an unfinished record. Recorded after, the
// file must begin with a point and ends with a code whenever its last point
// is coded. So a file that begins with a point block and ends with a code
// block is read as After Point; any other as Before Point. Only the first and
// the last blocks are looked at, so this costs a line or two, not a pass.
fn codes_recorded_after_points(bytes: &[u8]) -> bool {
    let mut first = 0;
    let mut rest = bytes;
    while !rest.is_empty() && first == 0 {
        let end = rest.iter().position(|&c| is_line_end(c));
        first = block_kind_of(&rest[..end.unwrap_or(rest.len())]);
        rest = match end {
            Some(end) => &rest[end + 1..],
            None => &[],
        };
    }
    if first != 11 {
        return false;
    }
    let mut last = 0;
    let mut rest = bytes;
    while !rest.is_empty() && last == 0 {
        let cut = rest.iter().rposition(|&c| is_line_end(c));
        last = block_kind_of(match cut {
            Some(cut) => &rest[cut + 1..],
            None => rest,
        });
        rest = match cut {
            Some(cut) => &rest[..cut],
            None => &[],
        };
    }
    last == 41
}

fn shown(text_bytes: &[u8]) -> String {
    let mut out = vec![b'\''];
    for &byte in &text_bytes[..text_bytes.len().min(40)] {
        out.push(if byte < 0x20 || byte == 0x7F {
            b'?'
        } else {
            byte
        });
    }
    out.extend_from_slice(if text_bytes.len() > 40 { b"...'" } else { b"'" });
    text(&out)
}

// A point as the file builds it up, before it is known whether it has a
// position (it may be named by a shot first and given coordinates later).
#[derive(Clone, Debug, Default)]
struct PointSlot {
    id: String,
    positioned: bool,
    northing: f64,
    easting: f64,
    elevation: Option<f64>,
    coordinate_source: CoordinateSource,
    /// The bytes the file wrote: codes are compared as the C++ compares them.
    code: Vec<u8>,
    metadata: BTreeMap<String, String>,
    /// Where the point was first named, or given its position.
    record: usize,
    /// ... and whether that block was GSI-16.
    wide: bool,
    /// Setups made on it so far, for their ids (station_id_for).
    setups: usize,
    // 1 + the setup whose shot gave the position (words 81-83, which the
    // instrument computed with that setup's orientation), else 0: such a
    // position cannot orient that setup (GsiReader::shot_block, the backsight).
    positioned_by_shot_of: usize,
}

// A setup whose first shot was to a point with no position yet: it is the
// setup's backsight if the file positions the point later (GsiReader::finish).
#[derive(Clone, Copy, Debug, Default)]
struct FirstShot {
    /// Index into project.stations.
    station: usize,
    slot: usize,
}

#[derive(Clone, Debug, Default)]
struct CodeBlock {
    code: Vec<u8>,
    info: [Vec<u8>; 8],
    record: usize,
}

// A point's place in the run of points that makes the features, settled once
// the point can get no more codes (GsiReader::point_code).
#[derive(Clone, Copy, Debug, Default)]
struct FeatureStep {
    slot: usize,
    record: usize,
    wide: bool,
}

// A feature being strung: its code as written and its points by slot, so that
// a point is compared with the one before it as the C++ compares their ids.
#[derive(Clone, Debug, Default)]
struct Run {
    code: Vec<u8>,
    slots: Vec<usize>,
    source: SourceRecord,
}

// A setup that is only a setup once a measurement follows it (see station_block).
#[derive(Clone, Debug, Default)]
struct PendingSetup {
    slot: usize,
    instrument_height: Option<f64>,
    record: usize,
    wide: bool,
    time: SurveyTimestamp,
    time_without_year: String,
}

// The warnings of the file, listed up to MAX_LISTED_WARNINGS and counted past it.
#[derive(Clone, Debug, Default)]
struct Warnings {
    file_name: String,
    listed: Vec<ReadWarning>,
    unlisted: usize,
}

impl Warnings {
    fn warn(&mut self, record: usize, message: String) {
        if self.listed.len() < MAX_LISTED_WARNINGS {
            self.listed.push(ReadWarning {
                file_name: self.file_name.clone(),
                record,
                message,
            });
        } else {
            self.unlisted += 1;
        }
    }
}

// What the file did many times, said once: the message is built only for a
// tally that has something to say, so a clean file costs nothing here.
fn once(warnings: &mut Warnings, tally: &Tally, message: impl FnOnce() -> String) {
    if tally.count != 0 {
        warnings.warn(tally.first_record, message());
    }
}

#[derive(Debug, Default)]
struct GsiReader {
    file_name: String,
    precision: ObservationPrecision,
    narrow_source: SourceRecord,
    wide_source: SourceRecord,
    result: ReadResult,
    warnings: Warnings,

    slots: Vec<PointSlot>,
    /// Point id, as the file's bytes, -> slot. The order of a hash map is
    /// never seen: the slots keep the file's order.
    slot_index: HashMap<Vec<u8>, usize>,
    pending_codes: Vec<CodeBlock>,
    run: Option<Run>,
    /// The last point, not yet in a feature.
    feature_step: Option<FeatureStep>,
    // The slot of the last block that named a point and took its codes
    // (point_code): the point BEFORE a code block. It is not the last slot
    // made once a round of shots names its points a second time.
    last_point_slot: Option<usize>,
    codes_since_point: usize,
    /// codes_recorded_after_points.
    codes_after_points: bool,
    after_points_said: bool,
    /// Code blocks read as point codes.
    code_blocks: usize,

    pending: Option<PendingSetup>,
    /// First shots to points not yet positioned.
    first_shots: Vec<FirstShot>,
    /// Index into project.stations.
    active: Option<usize>,
    active_slot: usize,
    active_height: Option<f64>,
    pointings: usize,
    last_setup_observations: usize,
    /// 0 none seen, 'x' varies, else the digit.
    hz_input_mode: u8,
    v_index: u8,

    // What the instrument is set to, from the last word that said so.
    reflector_height: Option<f64>,
    ppm: Option<f64>,
    prism: Option<f64>,
    /// The open setup's ppm came from an earlier one.
    ppm_carried: bool,
    /// ... and its prism constant.
    prism_carried: bool,
    station_block_ppm: bool,
    station_block_prism: bool,
    serial: String,
    type_: String,

    linear_unit: LinearUnit,
    angular_unit: AngularUnit,

    // By two-digit word index: a three-digit (digital level) word is counted
    // apart, as `levelling`, before a word reaches this table.
    unread: Vec<Tally>,
    explicit_points: Tally,
    // Sexagesimal words of 60.0 seconds read as the next minute, and their records.
    rounded_seconds: Tally,
    rounded_second_records: RecordList,
    /// Vertical readings (word 22) below zero, refused.
    negative_zeniths: Tally,
    /// Code information and remark words of nothing but zeros.
    zero_code_words: Tally,
    /// Remark words (71-79) in a code block, not read.
    remarks_in_code_blocks: Tally,
    /// Distances measured with another prism constant than their setup's.
    other_prism: Tally,
    /// Length words with a written decimal point.
    pointed_lengths: usize,
    /// ... and without one, as the specification has them.
    unpointed_lengths: Tally,
    odd_width: Tally,
    feet: Tally,
    mixed_linear_units: Tally,
    mixed_angular_units: Tally,
    height_text_as_code: Tally,
    height_text_kept: Tally,
    coordinate_records: Tally,
    zero_distances: Tally,
    derived_values: Tally,
    /// Slope distances passed over for the block's own 32.
    slope_without_zenith: Tally,
    time_words: Tally,
    height_only: Tally,
    partial_position: Tally,
    levelling: Tally,
    /// A digital level's '?' code blocks.
    special_codes: Tally,
    /// Setups given a backsight by their first shot.
    backsighted: usize,
    /// ... to a point positioned later in the file.
    backsighted_later: usize,
    setups_without_height: usize,
    setups_without_ppm: usize,
    setups_without_prism: usize,
    shots_without_reflector_height: usize,
    zenith_angles: usize,
    /// Horizontal circle readings read.
    directions: usize,
    wide_blocks: usize,
    narrow_blocks: usize,
}

// The input mode of a coordinate word says how the value came to be: keyed
// in (1), computed by an on-board program (4), or measured (0, 2, 3) - which
// for a target's coordinates means reduced from the shot. For a STATION's
// coordinates "measured" says nothing: they are not measured at the station,
// so it stays Unknown.
fn source_of(mode: u8, station: bool) -> CoordinateSource {
    match mode {
        b'1' => CoordinateSource::Entered,
        b'4' => CoordinateSource::Calculated,
        b'0' | b'2' | b'3' => {
            if station {
                CoordinateSource::Unknown
            } else {
                CoordinateSource::FieldObserved
            }
        }
        _ => CoordinateSource::Unknown,
    }
}

fn track(state: &mut u8, value: u8) {
    if value == b'.' || *state == b'x' {
        return;
    }
    *state = if *state == 0 {
        value
    } else if *state == value {
        *state
    } else {
        b'x'
    };
}

// The fields of a time word: `count` digits from `from`, or -1.
fn time_field(text_bytes: &[u8], from: usize, count: usize) -> i32 {
    let mut value = 0;
    for i in from..from + count {
        match text_bytes.get(i) {
            Some(&c) if is_digit(c) => value = value * 10 + i32::from(c - b'0'),
            _ => return -1,
        }
    }
    value
}

impl GsiReader {
    fn new(file_name: &str, options: &ReadOptions) -> Self {
        let narrow_source = SourceRecord {
            manufacturer: String::from("Leica"),
            format: String::from("Leica GSI"),
            format_version: String::from("GSI-8"),
            file_name: String::from(file_name),
            record_number: 0,
        };
        let wide_source = SourceRecord {
            format_version: String::from("GSI-16"),
            ..narrow_source.clone()
        };
        Self {
            file_name: String::from(file_name),
            precision: options.precision,
            narrow_source,
            wide_source,
            warnings: Warnings {
                file_name: String::from(file_name),
                ..Default::default()
            },
            unread: vec![Tally::default(); 100],
            ..Default::default()
        }
    }

    fn read(&mut self, bytes: &[u8]) {
        let bytes = without_byte_order_mark(bytes);
        // A block names at most one point, so the line count bounds the
        // points; reserving once spares the doubling copies of a vector that
        // is most of a coordinate list's memory. The C++ reports an allocation
        // that fails; Rust would abort, so a reservation that cannot be made
        // is not made and the vector grows as it goes.
        let line_feeds = bytes.iter().filter(|&&c| c == b'\n').count();
        let _ = self.slots.try_reserve(line_feeds + 1);
        self.codes_after_points = codes_recorded_after_points(bytes);
        let mut record = 0;
        let mut start = 0;
        while start < bytes.len() {
            let mut stop = start;
            while stop < bytes.len() && !is_line_end(bytes[stop]) {
                stop += 1;
            }
            record += 1;
            self.read_block(&bytes[start..stop], record);
            // "\r\n" ends one line, not two.
            if stop + 1 < bytes.len() && bytes[stop] == b'\r' && bytes[stop + 1] == b'\n' {
                stop += 1;
            }
            start = stop + 1;
        }
    }

    // ---- Diagnostics --------------------------------------------------------------

    fn warn(&mut self, record: usize, message: String) {
        self.warnings.warn(record, message);
    }

    fn warn_word(&mut self, block: &Block<'_>, word: &Word<'_>, problem: &str) {
        // A '-' is shown, being part of the value written; the '+' nearly
        // every word carries is not.
        let mut written = Vec::with_capacity(word.data.len() + 1);
        if word.sign == b'-' {
            written.push(b'-');
        }
        written.extend_from_slice(word.data);
        self.warn(
            block.record,
            format!(
                "word {} {} {problem}; that value was not read",
                word.index,
                shown(&written)
            ),
        );
    }

    fn template_for(&self, wide: bool) -> &SourceRecord {
        if wide {
            &self.wide_source
        } else {
            &self.narrow_source
        }
    }

    fn source_at(&self, record: usize, wide: bool) -> SourceRecord {
        SourceRecord {
            record_number: record,
            ..self.template_for(wide).clone()
        }
    }

    // ---- Values -------------------------------------------------------------------

    fn note_linear_unit(&mut self, unit: LinearUnit, record: usize) {
        if self.linear_unit == LinearUnit::Unknown {
            self.linear_unit = unit;
        } else if unit != self.linear_unit {
            self.mixed_linear_units.add(record, b"");
        }
    }

    fn note_angular_unit(&mut self, unit: AngularUnit, record: usize) {
        if self.angular_unit == AngularUnit::Unknown {
            self.angular_unit = unit;
        } else if unit != self.angular_unit {
            self.mixed_angular_units.add(record, b"");
        }
    }

    fn number_in(&mut self, block: &Block<'_>, word: &Word<'_>) -> Option<Number> {
        let value = number_of(word.data, word.sign);
        match value {
            None => self.warn_word(block, word, "is not a number"),
            Some(number) if number.explicit_point => {
                self.explicit_points.add(block.record, word.data);
            }
            Some(_) => {}
        }
        value
    }

    // A length word in metres. `unit_when_empty` is the unit a word may leave
    // unstated, where the specification's own example does (words 58 and 59);
    // 0 for none.
    fn length_of(
        &mut self,
        block: &Block<'_>,
        word: &Word<'_>,
        unit_when_empty: u8,
    ) -> Option<f64> {
        let mut unit = unit_digit(word);
        if unit == b'.' && unit_when_empty != 0 {
            unit = unit_when_empty;
        }
        let (decimals, feet) = match unit {
            b'0' => (3, false),
            b'1' => (3, true),
            b'6' => (4, false),
            b'7' => (4, true),
            b'8' => (5, false),
            _ => {
                self.warn_word(
                    block,
                    word,
                    &format!(
                        "has unit digit '{}', which is not a length unit",
                        text(&[unit])
                    ),
                );
                return None;
            }
        };
        let value = self.number_in(block, word)?;
        // Counted apart from angles: a file that writes its own points in
        // lengths and leaves a few to the unit digit has probably lost a
        // character in those few (finish() names them).
        if value.explicit_point {
            self.pointed_lengths += 1;
        } else {
            self.unpointed_lengths.add(block.record, word.data);
        }
        let mut metres = if value.explicit_point {
            value.value
        } else {
            value.value / POWERS_OF_TEN[decimals]
        };
        if feet {
            // GSI says "feet" and not which. The US survey foot: see finish().
            self.feet.add(block.record, b"");
            self.note_linear_unit(LinearUnit::UsSurveyFeet, block.record);
            metres = to_metres(metres, units::US_SURVEY_FOOT);
        } else {
            self.note_linear_unit(LinearUnit::Metres, block.record);
        }
        Some(metres)
    }

    // A circle reading (word 21 or 22) in radians, as read: not wrapped, not
    // turned into a zenith; with it, the 60-second word read as the next
    // minute it was (SIXTY SECONDS, at the top), or None. A negative word 22
    // is refused here; a negative word 21 is returned as it is, and shot_block
    // wraps it onto the circle (SIGN, at the top).
    fn angle_of(
        &mut self,
        block: &Block<'_>,
        word: &Word<'_>,
    ) -> (Option<f64>, Option<SixtySeconds>) {
        let unit = unit_digit(word);
        if unit == b'4' {
            return self.sexagesimal(block, word);
        }
        let (to_radians, decimals, full_circle, circle, declared) = match unit {
            b'2' => (PI / 200.0, 5, 400.0, "400 gon", AngularUnit::Gons),
            b'3' => (
                PI / 180.0,
                5,
                360.0,
                "360 degrees",
                AngularUnit::DecimalDegrees,
            ),
            b'5' => (TWO_PI / 6400.0, 4, 6400.0, "6400 mil", AngularUnit::Mils),
            _ => {
                self.warn_word(
                    block,
                    word,
                    &format!(
                        "has unit digit '{}', which is not an angle unit",
                        text(&[unit])
                    ),
                );
                return (None, None);
            }
        };
        let Some(value) = self.number_in(block, word) else {
            return (None, None);
        };
        let in_unit = if value.explicit_point {
            value.value
        } else {
            value.value / POWERS_OF_TEN[decimals]
        };
        // The unit digit names the circle (page 6: 400 gon, 360 degrees
        // decimal or sexagesimal, 6400 mil), and no circle reading is past
        // it: such a word is damaged, and wrapping it onto the circle would
        // hide that. The full circle itself is the zero, as a rounding up at
        // the top of the circle writes it.
        if in_unit.abs() > full_circle {
            self.warn_word(
                block,
                word,
                &format!("is past a full circle ({circle}), which no circle reading is"),
            );
            return (None, None);
        }
        if self.refused_as_negative_zenith(block, word, in_unit) {
            return (None, None);
        }
        self.note_angular_unit(declared, block.record);
        (Some(in_unit * to_radians), None)
    }

    // A vertical reading below zero is no zenith angle (SIGN, at the top):
    // refused at its record, and counted for the note on the vertical angle
    // setting in finish(). `value` is the reading in any unit; -0 is not below
    // zero, and is read. Counted wherever the word is, a code block's too, as
    // every refused word is warned about wherever it is: the note counts the
    // refusals, as a sign of what wrote the file's vertical readings. The
    // 60-second note is another matter - it says its words were READ as the
    // next minute, so it counts only the words whose values were kept
    // (kept_sixty).
    fn refused_as_negative_zenith(
        &mut self,
        block: &Block<'_>,
        word: &Word<'_>,
        value: f64,
    ) -> bool {
        if word.index != 22 || value >= 0.0 {
            return false;
        }
        self.warn_word(
            block,
            word,
            "is negative, which no zenith angle is (a damaged word, or an instrument whose \
             vertical angle setting - GSI ONLINE's SET 44: horizontal, or slope in percent - \
             reached the file)",
        );
        self.negative_zeniths.add(block.record, b"");
        true
    }

    // DDDMMSSs: degrees, minutes, seconds and tenths - DDD.MMSSs with the point
    // unwritten. A file that writes the point is read the same way around it.
    // Seconds of 60.0 are the next minute where the word writes them in their
    // place (SIXTY SECONDS, at the top), and the second value then says so.
    fn sexagesimal(
        &mut self,
        block: &Block<'_>,
        word: &Word<'_>,
    ) -> (Option<f64>, Option<SixtySeconds>) {
        // A word with no digit - "." - writes no angle. The missing digits
        // supplied below would make it 000 00 00.0, a reading like any other;
        // number_of refuses the same word in the other units.
        if !word.data.iter().any(|&c| is_digit(c)) {
            self.warn_word(block, word, "is not a sexagesimal angle: it has no digit");
            return (None, None);
        }
        let mut padded: Vec<u8> = Vec::new();
        let mut whole: &[u8] = word.data;
        let mut fraction: &[u8];
        let point = whole.iter().position(|&c| c == b'.');
        if let Some(point) = point {
            self.explicit_points.add(block.record, word.data);
            fraction = &whole[point + 1..];
            whole = &whole[..point];
        } else if whole.len() > 5 {
            fraction = &whole[whole.len() - 5..];
            whole = &whole[..whole.len() - 5];
        } else {
            fraction = whole;
            whole = &[];
        }
        let digits_of = |text_bytes: &[u8], from: usize, count: usize| -> i32 {
            let mut value = 0;
            for i in from..from + count {
                // Missing digits on the right are zeros: "123.45" is 123 45 00.
                let c = text_bytes.get(i).copied().unwrap_or(b'0');
                if !is_digit(c) {
                    return -1;
                }
                value = value * 10 + i32::from(c - b'0');
            }
            value
        };
        // Without a written point a short word is right-aligned: "12345" is
        // 0 degrees 12' 34.5", so the five places are filled from the left.
        if point.is_none() && fraction.len() < 5 {
            padded.resize(5 - fraction.len(), b'0');
            padded.extend_from_slice(fraction);
            fraction = &padded;
        }
        let mut degrees: i64 = 0;
        for &c in whole {
            if !is_digit(c) {
                self.warn_word(block, word, "is not a sexagesimal angle");
                return (None, None);
            }
            degrees = degrees * 10 + i64::from(c - b'0');
        }
        let mut minutes = digits_of(fraction, 0, 2);
        let mut seconds = digits_of(fraction, 2, 2);
        // Tenths and anything finer, as the decimal part of the seconds.
        let mut finer = 0.0;
        let mut scale = 0.1;
        // A digit other than 0 after the seconds.
        let mut past_whole_seconds = false;
        for &c in fraction.iter().skip(4) {
            if !is_digit(c) {
                self.warn_word(block, word, "is not a sexagesimal angle");
                return (None, None);
            }
            finer += f64::from(c - b'0') * scale;
            past_whole_seconds = past_whole_seconds || c != b'0';
            scale /= 10.0;
        }
        if minutes < 0 || seconds < 0 {
            self.warn_word(block, word, "is not a sexagesimal angle");
            return (None, None);
        }
        if minutes >= 60 {
            self.warn_word(
                block,
                word,
                "is not a sexagesimal angle: the minutes run to 59",
            );
            return (None, None);
        }
        if seconds > 60 || (seconds == 60 && past_whole_seconds) {
            self.warn_word(
                block,
                word,
                "is not a sexagesimal angle: the seconds run to 59, and to 60.0 only where a \
                 writer rounded up without carrying the minute",
            );
            return (None, None);
        }
        let mut carried = None;
        if seconds == 60 {
            // Only a 60 the word writes itself, both digits in their place. A
            // word not its block's width was aligned from the right above, and
            // a written point's missing digits were supplied as zeros: either
            // can make a 60 of digits that were never the seconds.
            let width: usize = if block.wide { 16 } else { 8 };
            if point.is_some() && fraction.len() < 4 {
                self.warn_word(
                    block,
                    word,
                    "is not a sexagesimal angle: it writes one digit of the seconds after its \
                     point, and 60 seconds are read as the next minute only where both are \
                     written",
                );
                return (None, None);
            }
            if point.is_none() && word.data.len() != width {
                self.warn_word(
                    block,
                    word,
                    &format!(
                        "is not a sexagesimal angle: 60 seconds are read as the next minute only \
                         in a word of its block's width, {width} characters, where each digit is \
                         in its place, and this one has {}",
                        word.data.len()
                    ),
                );
                return (None, None);
            }
            carried = Some(SixtySeconds {
                word: word.index,
                negative: word.sign == b'-',
                degrees,
                minutes,
                zeros: fraction.len() - 4,
            });
            // Carried in whole numbers, so that the angle is the same double
            // as the word written with its carry made; adding 60/3600 of a
            // degree as a real is not.
            seconds = 0;
            minutes += 1;
            if minutes == 60 {
                minutes = 0;
                degrees += 1;
            }
        }
        // One full circle at most (angle_of says why); 360 00 00.0, as written
        // or as 359 59 60.0 carried, is the circle's zero.
        let past_zero = minutes != 0 || seconds != 0 || past_whole_seconds;
        if degrees > 360 || (degrees == 360 && past_zero) {
            self.warn_word(
                block,
                word,
                "is past a full circle (360 degrees), which no circle reading is",
            );
            return (None, None);
        }
        let mut degrees_value =
            degrees as f64 + f64::from(minutes) / 60.0 + (f64::from(seconds) + finer) / 3600.0;
        if word.sign == b'-' {
            degrees_value = -degrees_value;
        }
        // Where this sits among the steps above changes nothing: the sign is
        // the same before a carry and after it (-000 00 60.0 is below zero
        // either way), and a refused word's sixty is never counted, since
        // the 60-second words are counted where their values are kept
        // (kept_sixty).
        if self.refused_as_negative_zenith(block, word, degrees_value) {
            return (None, None);
        }
        self.note_angular_unit(AngularUnit::DegreesMinutesSeconds, block.record);
        (Some(degrees_value * (PI / 180.0)), carried)
    }

    // Word 51: "+0220+002" is 220 ppm and a prism constant of 2 mm, each with
    // its own sign (GET 51, and page 6: "certain data blocks ... carry more than
    // 1 value ... with a sign before each single value").
    fn ppm_and_prism(&mut self, block: &mut Block<'_>, word: &Word<'_>) {
        let data = word.data;
        let cut = data
            .iter()
            .skip(1)
            .position(|&c| c == b'+' || c == b'-')
            .map(|at| at + 1);
        let Some(cut) = cut.filter(|&cut| cut + 1 < data.len()) else {
            self.warn_word(
                block,
                word,
                "does not hold a ppm and a prism constant, each signed",
            );
            return;
        };
        let ppm = number_of(&data[..cut], word.sign);
        let millimetres = number_of(&data[cut + 1..], data[cut]);
        match (ppm, millimetres) {
            (Some(ppm), Some(millimetres))
                if !ppm.explicit_point && !millimetres.explicit_point =>
            {
                block.ppm = Some(ppm.value);
                block.prism_constant = Some(millimetres.value / 1000.0);
            }
            _ => self.warn_word(
                block,
                word,
                "does not hold a ppm and a prism constant, each signed",
            ),
        }
    }

    // The text of a code information (42-49) or remark (71-79) word. One of
    // nothing but zeros is EMPTY, not the code "0": GSI right-justifies text
    // and pads it with '0', so that is how an empty value is written - Leica's
    // Format Manager writes an unset code information word "43....+00000000"
    // (Reference Guide V1.0, Annex 2) - and read as "0" it gives every point
    // of a job that records word 71 empty on each shot one code, and one
    // feature through all of them. The code "0" would be written the same,
    // so the import says how many it read as empty.
    fn code_text<'a>(&mut self, block: &Block<'a>, word: &Word<'a>) -> &'a [u8] {
        if word.data.iter().all(|&c| c == b'0') {
            self.zero_code_words
                .add_made(block.record, || format!("word {}", word.index));
            return &[];
        }
        text_of(word.data)
    }

    // A height word (87 or 88). Letters where a height belongs: some exporters
    // write the point code into a height word (see point_code), and then the
    // block's height text is set and None comes back; otherwise the length.
    fn height_word<'a>(&mut self, block: &mut Block<'a>, word: &Word<'a>) -> Option<Option<f64>> {
        let letters = word.data.iter().any(|&c| !is_digit(c) && c != b'.');
        if letters {
            block.height_text = text_of(word.data);
            block.height_text_word = word.index;
            return None;
        }
        Some(self.length_of(block, word, 0))
    }

    fn decode_word<'a>(&mut self, block: &mut Block<'a>, word: &Word<'a>, first: bool) {
        match word.index {
            11 | 41 => {
                if !first {
                    self.warn_word(block, word, "is a block's first word, found inside a block");
                    return;
                }
                block.name = text_of(word.data);
                return;
            }
            12 => {
                block.serial = text_of(word.data);
                return;
            }
            13 => {
                block.type_ = text_of(word.data);
                return;
            }
            18 => {
                block.time18 = word.data;
                return;
            }
            19 => {
                block.time19 = word.data;
                return;
            }
            21 => {
                block.measurement_words = true;
                let (value, sixty) = self.angle_of(block, word);
                block.hz = value;
                block.hz_sixty = sixty;
                block.hz_input_mode = input_mode(word);
                return;
            }
            22 => {
                block.measurement_words = true;
                let (value, sixty) = self.angle_of(block, word);
                block.v = value;
                block.v_sixty = sixty;
                block.v_index = info_at(word, 4);
                return;
            }
            31 => {
                block.measurement_words = true;
                block.slope = self.length_of(block, word, 0);
                return;
            }
            32 => {
                block.measurement_words = true;
                block.horizontal = self.length_of(block, word, 0);
                return;
            }
            33 => {
                block.measurement_words = true;
                block.height_difference = self.length_of(block, word, 0);
                return;
            }
            51 => {
                self.ppm_and_prism(block, word);
                return;
            }
            58 => {
                // GET 58: "58..16+00000020" is 2 mm; PUT 58 leaves the unit out
                // and means tenths of a millimetre.
                block.prism_constant = self.length_of(block, word, b'6');
                return;
            }
            59 => {
                // GET 59: "59..16+02200000" is 220 ppm - the unit digit's
                // decimals, four when it is left out (PUT 59).
                let unit = match unit_digit(word) {
                    b'.' => b'6',
                    unit => unit,
                };
                let decimals = if unit == b'6' || unit == b'7' {
                    4
                } else if unit == b'8' {
                    5
                } else {
                    3
                };
                if let Some(value) = self.number_in(block, word) {
                    block.ppm = Some(if value.explicit_point {
                        value.value
                    } else {
                        value.value / POWERS_OF_TEN[decimals]
                    });
                }
                return;
            }
            81 => {
                block.e = self.length_of(block, word, 0);
                block.target_input_mode = input_mode(word);
                return;
            }
            82 => {
                block.n = self.length_of(block, word, 0);
                block.target_input_mode = input_mode(word);
                return;
            }
            83 => {
                block.h = self.length_of(block, word, 0);
                block.target_input_mode = input_mode(word);
                return;
            }
            84 => {
                block.e0 = self.length_of(block, word, 0);
                block.station_input_mode = input_mode(word);
                return;
            }
            85 => {
                block.n0 = self.length_of(block, word, 0);
                block.station_input_mode = input_mode(word);
                return;
            }
            86 => {
                block.h0 = self.length_of(block, word, 0);
                block.station_input_mode = input_mode(word);
                return;
            }
            87 => {
                if let Some(height) = self.height_word(block, word) {
                    block.reflector_height = height;
                }
                return;
            }
            88 => {
                if let Some(height) = self.height_word(block, word) {
                    block.instrument_height = height;
                }
                return;
            }
            _ => {}
        }
        if (42..=49).contains(&word.index) {
            let value = self.code_text(block, word);
            block.code_info[(word.index - 42) as usize] = value;
            return;
        }
        if (71..=79).contains(&word.index) {
            // Words 71-79 are read in a point's block, as its code (71) and
            // remarks (72-79). In a code block they could be those or more
            // about the block's own code, and nothing tells which, so they
            // are counted as not read rather than guessed at.
            if block.kind == 41 {
                self.remarks_in_code_blocks
                    .add_made(block.record, || format!("word {}", word.index));
                return;
            }
            let value = self.code_text(block, word);
            block.remarks[(word.index - 71) as usize] = value;
            return;
        }
        if word.index >= 100 {
            block.levelling = true; // said once for the file, in finish()
            return;
        }
        // Counted and reported by index: a word this reader does not use is
        // said once for the file, never dropped without a word.
        if word.index >= 0 && (word.index as usize) < self.unread.len() {
            self.unread[word.index as usize].add(block.record, b"");
        }
    }

    // ---- Blocks -------------------------------------------------------------------

    fn read_block(&mut self, line: &[u8], record: usize) {
        let mut block = Block::new(record);
        let mut first = true;
        let mut position = 0;
        loop {
            while position < line.len() && is_blank(line[position]) {
                position += 1;
            }
            if position >= line.len() {
                break;
            }
            let mut end = position;
            while end < line.len() && !is_blank(line[end]) {
                end += 1;
            }
            let mut token = &line[position..end];
            position = end;
            if first && token[0] == b'*' {
                block.wide = true;
                token = &token[1..];
            }
            // A DOS end-of-file mark on the last line is not a word.
            if token == b"\x1a" {
                continue;
            }
            let Some(word) = parse_word(token) else {
                if first {
                    self.warn(
                        record,
                        format!(
                            "does not begin with a GSI word ({}); the line was not read",
                            shown(token)
                        ),
                    );
                    self.result.records_skipped += 1;
                    return;
                }
                self.warn(
                    record,
                    format!(
                        "{} is not a GSI word; the rest of the block was read",
                        shown(token)
                    ),
                );
                continue;
            };
            if first {
                if word.index != 11 && word.index != 41 {
                    self.warn(
                        record,
                        format!(
                            "begins with word {}, where a GSI block begins with word 11 (a \
                             point) or 41 (a code); the block was not read",
                            word.index
                        ),
                    );
                    self.result.records_skipped += 1;
                    return;
                }
                block.kind = word.index;
            }
            let width = if block.wide { 16 } else { 8 };
            if word.data.len() != width {
                self.odd_width.add(record, token);
            }
            self.decode_word(&mut block, &word, first);
            first = false;
        }
        if first {
            return; // a blank line is not a record
        }
        self.result.records_read += 1;
        if block.wide {
            self.wide_blocks += 1;
        } else {
            self.narrow_blocks += 1;
        }
        if block.kind == 41 {
            self.code_block(&block);
        } else if block.levelling {
            // A digital level's block: its point is named, its staff readings
            // and distances are not total station shots and are not read.
            self.levelling.add(block.record, block.name);
            block.horizontal = None;
            block.height_difference = None;
            block.slope = None;
            self.point_block(&block);
        } else if block.measures() {
            self.shot_block(&block);
        } else if block.station_coordinates() || block.instrument_height.is_some() {
            self.station_block(&block);
        } else {
            self.point_block(&block);
        }
        if block.reflector_height.is_some() {
            self.reflector_height = block.reflector_height;
        }
    }

    fn code_block(&mut self, block: &Block<'_>) {
        if block.measurement_words {
            self.warn(
                block.record,
                String::from("a code block holds measurement words; they were not read"),
            );
        }
        // A digital level's "special code block" names the levelling method
        // ("410000+?......1" is line levelling BF: GSI ONLINE's DNA section)
        // and starts a line; it is not a point's code.
        if block.name.first() == Some(&b'?') {
            self.special_codes.add(block.record, block.name);
            return;
        }
        let code = CodeBlock {
            code: block.name.to_vec(),
            info: block.code_info.map(<[u8]>::to_vec),
            record: block.record,
        };
        self.code_blocks += 1;
        // Recorded after its point (codes_recorded_after_points): the point
        // before is the one. Its place in the features waited for this.
        if self.codes_after_points
            && let Some(last) = self.last_point_slot
        {
            if !self.after_points_said {
                self.after_points_said = true;
                self.warn(
                    block.record,
                    String::from(
                        "the file begins with a point block and ends with a code block, as an \
                         instrument set to record codes after their point (<Rec Free Code: After \
                         Point>) writes a job, so each code block was attached to the point \
                         before it",
                    ),
                );
            }
            let ordinal = self.codes_since_point;
            self.codes_since_point += 1;
            self.attach_code(last, &code, ordinal);
            return;
        }
        self.pending_codes.push(code);
    }

    fn new_slot(&mut self, id: &[u8], record: usize, wide: bool) -> usize {
        self.slots.push(PointSlot {
            id: text(id),
            record,
            wide,
            ..Default::default()
        });
        self.slot_index.insert(id.to_vec(), self.slots.len() - 1);
        self.slots.len() - 1
    }

    fn slot_for(&mut self, id: &[u8], record: usize, wide: bool) -> usize {
        if let Some(&found) = self.slot_index.get(id) {
            return found;
        }
        self.new_slot(id, record, wide)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "The C++ placePoint takes these, one per word and setting of the block."
    )]
    fn place_point(
        &mut self,
        index: usize,
        e: Option<f64>,
        n: Option<f64>,
        h: Option<f64>,
        mode: u8,
        station: bool,
        block: &Block<'_>,
    ) {
        let (Some(e), Some(n)) = (e, n) else {
            // A height alone cannot place a point; it is kept, not dropped.
            let slot = &mut self.slots[index];
            if let Some(h) = h {
                slot.metadata
                    .insert(String::from("height without a position"), metres_text(h));
                self.height_only.add(block.record, slot.id.as_bytes());
            } else {
                self.partial_position.add(block.record, slot.id.as_bytes());
            }
            return;
        };
        let slot = &mut self.slots[index];
        if !slot.positioned {
            slot.positioned = true;
            slot.easting = e;
            slot.northing = n;
            slot.elevation = h;
            slot.coordinate_source = source_of(mode, station);
            slot.record = block.record;
            slot.wide = block.wide;
            return;
        }
        let horizontal = (e - slot.easting).hypot(n - slot.northing);
        let vertical = match (h, slot.elevation) {
            (Some(h), Some(elevation)) => (h - elevation).abs(),
            _ => 0.0,
        };
        if horizontal > SAME_POSITION_TOLERANCE || vertical > SAME_POSITION_TOLERANCE {
            let message = format!(
                "point {} is given coordinates again, {} m horizontally and {} m in height from \
                 those of record {}; the first are kept",
                slot.id,
                metres_text(horizontal),
                metres_text(vertical),
                slot.record
            );
            self.warn(block.record, message);
        }
        let slot = &mut self.slots[index];
        if slot.elevation.is_none() && h.is_some() {
            slot.elevation = h;
        }
    }

    fn add_code(&mut self, index: usize, code: &[u8], where_from: &str) {
        let slot = &mut self.slots[index];
        if slot.code.is_empty() {
            slot.code = code.to_vec();
        } else if slot.code != code {
            slot.metadata
                .entry(String::from(where_from))
                .or_insert_with(|| text(code));
        }
    }

    // A point's code: word 71 in its own block; else its code block - the one
    // before it, or after it in a file recorded After Point (see leica.rs);
    // else text the file wrote into a height word. Whichever is not the code
    // is kept in the point's metadata.
    fn point_code(&mut self, index: usize, block: &Block<'_>) {
        // The point before this one has every code it will get: in a file
        // recorded After Point its code blocks came between the two.
        self.flush_feature();
        if !block.remarks[0].is_empty() {
            self.add_code(index, block.remarks[0], "remark 1");
        }
        for (i, remark) in block.remarks.iter().enumerate().skip(1) {
            if !remark.is_empty() {
                self.slots[index]
                    .metadata
                    .insert(format!("remark {}", i + 1), text(remark));
            }
        }
        // Code information (42-49) in the point's own block rather than a
        // code block's: "one may create any kind of GSI formats" (Leica's
        // Format Manager Reference Guide V1.0, Annex 2), and here the point is
        // the one thing it can be about.
        for (i, info) in block.code_info.iter().enumerate() {
            if !info.is_empty() {
                self.slots[index]
                    .metadata
                    .insert(format!("code information {}", i + 1), text(info));
            }
        }
        let pending = std::mem::take(&mut self.pending_codes);
        for (c, code) in pending.iter().enumerate() {
            self.attach_code(index, code, c);
        }
        if !block.height_text.is_empty() {
            let key = format!("word {} text", block.height_text_word);
            if self.slots[index].code.is_empty() {
                self.slots[index].code = block.height_text.to_vec();
                self.height_text_as_code
                    .add(block.record, block.height_text);
            } else {
                self.slots[index]
                    .metadata
                    .insert(key, text(block.height_text));
                self.height_text_kept.add(block.record, block.height_text);
            }
        }
        self.feature_step = Some(FeatureStep {
            slot: index,
            record: block.record,
            wide: block.wide,
        });
        self.last_point_slot = Some(index);
        self.codes_since_point = 0;
    }

    fn attach_code(&mut self, index: usize, code: &CodeBlock, ordinal: usize) {
        let slot = &mut self.slots[index];
        let prefix = if ordinal == 0 {
            String::from("code block")
        } else {
            format!("code block {}", ordinal + 1)
        };
        if slot.code.is_empty() {
            slot.code = code.code.clone();
        } else if slot.code != code.code {
            slot.metadata.insert(prefix.clone(), text(&code.code));
        }
        for (i, info) in code.info.iter().enumerate() {
            if !info.is_empty() {
                slot.metadata
                    .insert(format!("{prefix} information {}", i + 1), text(info));
            }
        }
    }

    fn flush_feature(&mut self) {
        if let Some(step) = self.feature_step.take() {
            self.feature(step.slot, step.record, step.wide);
        }
    }

    // One feature per run of consecutive points with the same code: GSI has no
    // string numbers, so a change of code (or an uncoded point) ends a string.
    fn feature(&mut self, index: usize, record: usize, wide: bool) {
        if self.slots[index].code.is_empty() {
            self.close_feature();
            return;
        }
        if let Some(run) = self.run.as_mut()
            && run.code == self.slots[index].code
        {
            if run.slots.last() != Some(&index) {
                run.slots.push(index);
            }
            return;
        }
        self.close_feature();
        self.run = Some(Run {
            code: self.slots[index].code.clone(),
            slots: vec![index],
            source: self.source_at(record, wide),
        });
    }

    fn close_feature(&mut self) {
        if let Some(run) = self.run.take() {
            let point_ids: Vec<String> = run
                .slots
                .iter()
                .map(|&slot| self.slots[slot].id.clone())
                .collect();
            self.result.project.features.push(SurveyFeature {
                code: text(&run.code),
                point_ids,
                source: run.source,
                ..Default::default()
            });
        }
    }

    // ---- Setups ---------------------------------------------------------------------

    // A block that gives the station's coordinates or the instrument height and
    // measures nothing. It is a SETUP only if measurements follow it; one
    // followed by another such block (or by nothing) was a list of coordinates
    // - the shape of the owner's "11 + 84-86 + 88" files - and the point simply
    // keeps them.
    fn station_block(&mut self, block: &Block<'_>) {
        self.discard_pending_setup();
        let index = self.slot_for(block.name, block.record, block.wide);
        if block.station_coordinates() {
            self.place_point(
                index,
                block.e0,
                block.n0,
                block.h0,
                block.station_input_mode,
                true,
                block,
            );
        } else if block.target_coordinates() {
            self.place_point(
                index,
                block.e,
                block.n,
                block.h,
                block.target_input_mode,
                false,
                block,
            );
        }
        self.point_code(index, block);
        self.note_settings(block, false);
        self.station_block_ppm = block.ppm.is_some();
        self.station_block_prism = block.prism_constant.is_some();
        let mut setup = PendingSetup {
            slot: index,
            instrument_height: block.instrument_height,
            record: block.record,
            wide: block.wide,
            ..Default::default()
        };
        self.time_of(block, &mut setup.time, &mut setup.time_without_year);
        self.pending = Some(setup);
    }

    fn discard_pending_setup(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.coordinate_records
            .add(pending.record, self.slots[pending.slot].id.as_bytes());
        if let Some(height) = pending.instrument_height {
            self.slots[pending.slot]
                .metadata
                .insert(String::from("instrument height"), metres_text(height));
        }
    }

    // A setup's id: its point's id for the first setup on the point, "ID (n)"
    // for the n-th. A point id from the file holds no blank (a blank ends a
    // GSI word) and the one id made up here, "GSI station N", no bracket, so
    // an "ID (n)" is never a point's id nor another point's setup's: a count
    // per point makes the id with no search. A search from "(2)" upwards
    // costs the k-th setup on a point k lookups - a monitoring pillar set up
    // every half hour for a year is some 17,500 setups, 150 million lookups.
    fn station_id_for(&mut self, slot: usize) -> String {
        let point = &mut self.slots[slot];
        point.setups += 1;
        let n = point.setups;
        if n == 1 {
            point.id.clone()
        } else {
            format!("{} ({n})", point.id)
        }
    }

    fn open_setup(
        &mut self,
        slot: usize,
        instrument_height: Option<f64>,
        record: usize,
        wide: bool,
        time: &SurveyTimestamp,
        time_without_year: &str,
    ) {
        self.close_setup();
        let point_id = self.slots[slot].id.clone();
        let id = self.station_id_for(slot);
        let mut station = SurveyStation {
            setup: Station {
                id,
                point_id,
                instrument_height: instrument_height.unwrap_or(0.0),
            },
            source: self.source_at(record, wide),
            // What the instrument was last set to carries over - it keeps its
            // settings from one setup to the next - until this setup's own
            // blocks state theirs (note_settings).
            instrument: InstrumentSettings {
                model: self.type_.clone(),
                serial_number: self.serial.clone(),
                time: time.clone(),
                ..Default::default()
            },
            ..Default::default()
        };
        if self.ppm.is_some() {
            station.instrument.atmospheric_ppm = self.ppm;
            station.instrument.atmospheric_ppm_state = CorrectionState::Applied;
        }
        if self.prism.is_some() {
            station.instrument.prism_constant = self.prism;
            station.instrument.prism_constant_state = CorrectionState::Applied;
        }
        self.ppm_carried = self.ppm.is_some() && !self.station_block_ppm;
        self.prism_carried = self.prism.is_some() && !self.station_block_prism;
        if !time_without_year.is_empty() {
            station
                .metadata
                .entry(String::from("time (no year recorded)"))
                .or_insert_with(|| String::from(time_without_year));
        }
        // Setups in one job are alike in size; starting from the last one's
        // count saves the growth copies of this one's observations.
        station.observations.reserve(self.last_setup_observations);
        self.result.project.stations.push(station);
        self.active = Some(self.result.project.stations.len() - 1);
        self.active_slot = slot;
        self.active_height = instrument_height;
        self.pointings = 0;
        self.hz_input_mode = 0;
        self.v_index = 0;
    }

    // What the whole setup's shots said about the instrument's own corrections,
    // written once when the setup ends.
    fn close_setup(&mut self) {
        let Some(active) = self.active else {
            return;
        };
        let station = &mut self.result.project.stations[active];
        self.last_setup_observations = station.observations.len();
        // Input mode 2 / 3 of an angle word: the horizontal (collimation and
        // tilt) correction on / off. Position 4: the automatic vertical index
        // off (0) or operating (1, 3).
        let horizontal = match self.hz_input_mode {
            b'x' => Some("varies within the setup"),
            b'2' => Some("on"),
            b'3' => Some("off"),
            _ => None,
        };
        if let Some(value) = horizontal {
            station
                .metadata
                .entry(String::from("horizontal correction"))
                .or_insert_with(|| String::from(value));
        }
        let vertical = match self.v_index {
            b'x' => Some("varies within the setup"),
            b'1' | b'3' => Some("on"),
            b'0' => Some("off"),
            _ => None,
        };
        if let Some(value) = vertical {
            station
                .metadata
                .entry(String::from("automatic vertical index"))
                .or_insert_with(|| String::from(value));
        }
        if station.instrument.atmospheric_ppm.is_none() {
            self.setups_without_ppm += 1;
        }
        if station.instrument.prism_constant.is_none() {
            self.setups_without_prism += 1;
        }
        if self.active_height.is_none() {
            self.setups_without_height += 1;
        }
        self.active = None;
    }

    // Words 12, 13, 51, 58, 59 change what the instrument is set to from here
    // on; the setup that is open records them. A station block's words are for
    // the setup it opens (open_setup takes them from here), never for the one
    // before it, hence `to_open_setup`.
    fn note_settings(&mut self, block: &Block<'_>, to_open_setup: bool) {
        let station = if to_open_setup && self.pending.is_none() {
            self.active
        } else {
            None
        };
        if let Some(ppm) = block.ppm {
            if let Some(station) = station {
                let carried = self.ppm_carried;
                let setup = &mut self.result.project.stations[station];
                let recorded = setup.instrument.atmospheric_ppm;
                match recorded {
                    Some(old) if !carried => {
                        if old != ppm {
                            let message = format!(
                                "the ppm changes from {} to {} within setup {}; the setup \
                                 records the first, and the distances carry whichever the \
                                 instrument applied",
                                metres_text(old),
                                metres_text(ppm),
                                setup.setup.id
                            );
                            self.warn(block.record, message);
                        }
                    }
                    _ => {
                        setup.instrument.atmospheric_ppm = Some(ppm);
                        setup.instrument.atmospheric_ppm_state = CorrectionState::Applied;
                        self.ppm_carried = false;
                    }
                }
            }
            self.ppm = Some(ppm);
        }
        if let Some(prism) = block.prism_constant {
            if let Some(station) = station {
                let setup = &mut self.result.project.stations[station];
                if setup.instrument.prism_constant.is_none() || self.prism_carried {
                    self.prism_carried = false;
                    setup.instrument.prism_constant = Some(prism);
                    setup.instrument.prism_constant_state = CorrectionState::Applied;
                }
            }
            self.prism = Some(prism);
        }
        if !block.serial.is_empty() {
            self.serial = text(block.serial);
            if let Some(station) = station {
                let instrument = &mut self.result.project.stations[station].instrument;
                if instrument.serial_number.is_empty() {
                    instrument.serial_number = self.serial.clone();
                }
            }
        }
        if !block.type_.is_empty() {
            self.type_ = text(block.type_);
            if let Some(station) = station {
                let instrument = &mut self.result.project.stations[station].instrument;
                if instrument.model.is_empty() {
                    instrument.model = self.type_.clone();
                }
            }
        }
    }

    // Word 19 is MMDDhhmm; word 18 is YYSSmmm (year, seconds, milliseconds) -
    // GET 19 and the TPS time format [YY.SS.mSmSmS]. The two-digit year is read
    // as POSIX strptime's %y does: 69-99 are 1969-1999, 00-68 are 2000-2068.
    fn time_of(
        &mut self,
        block: &Block<'_>,
        time: &mut SurveyTimestamp,
        without_year: &mut String,
    ) {
        if block.time19.is_empty() {
            if !block.time18.is_empty() {
                self.time_words.add(block.record, b"");
            }
            return;
        }
        let t19: &[u8] = if block.time19.len() >= 8 {
            &block.time19[block.time19.len() - 8..]
        } else {
            b""
        };
        let month = time_field(t19, 0, 2);
        let day = time_field(t19, 2, 2);
        let hour = time_field(t19, 4, 2);
        let minute = time_field(t19, 6, 2);
        if !(1..=12).contains(&month)
            || !(1..=31).contains(&day)
            || !(0..=23).contains(&hour)
            || !(0..=59).contains(&minute)
        {
            self.warn(
                block.record,
                format!(
                    "word 19 {} is not a time (MMDDhhmm); the time was not read",
                    shown(block.time19)
                ),
            );
            return;
        }
        let t18: &[u8] = if block.time18.len() >= 7 {
            &block.time18[block.time18.len() - 7..]
        } else {
            b""
        };
        let year = time_field(t18, 0, 2);
        let second = time_field(t18, 2, 2);
        let millisecond = time_field(t18, 4, 3);
        if year < 0 || !(0..=59).contains(&second) || millisecond < 0 {
            let two = |value: i32| {
                let mut out = String::new();
                two_digits(&mut out, value);
                out
            };
            *without_year = format!("{}-{} {}:{}", two(month), two(day), two(hour), two(minute));
            if !block.time18.is_empty() {
                self.warn(
                    block.record,
                    format!(
                        "word 18 {} is not a year and seconds (YYSSmmm); the time was read \
                         without them",
                        shown(block.time18)
                    ),
                );
            }
            return;
        }
        time.year = if year >= 69 { 1900 + year } else { 2000 + year };
        time.month = month;
        time.day = day;
        time.hour = hour;
        time.minute = minute;
        time.second = f64::from(second) + f64::from(millisecond) / 1000.0;
    }

    fn implicit_setup(&mut self, block: &Block<'_>) {
        let mut id = String::from("GSI station");
        let mut n = 2;
        while self.slot_index.contains_key(id.as_bytes()) {
            id = format!("GSI station {n}");
            n += 1;
        }
        self.warn(
            block.record,
            format!(
                "measurements begin before any station block, so the instrument's position is \
                 not recorded; they are kept under a setup at a point named '{id}' with no \
                 coordinates"
            ),
        );
        let slot = self.slot_for(id.as_bytes(), block.record, block.wide);
        self.open_setup(
            slot,
            None,
            block.record,
            block.wide,
            &SurveyTimestamp::default(),
            "",
        );
    }

    // Whether a distance word is one to carry: not absent, not Leica's zero
    // for "no distance measured", not negative.
    fn usable(&mut self, value: Option<f64>, what: &str, record: usize) -> Option<f64> {
        let value = value?;
        if value == 0.0 {
            self.zero_distances.add(record, b""); // Leica's "no distance measured"
            return None;
        }
        if value < 0.0 {
            self.warn(
                record,
                format!(
                    "the {what} is negative ({} m); it was not read",
                    metres_text(value)
                ),
            );
            return None;
        }
        Some(value)
    }

    // Another constant than the setup began with: a changed prism or a wrong
    // setting, which the person has to tell apart.
    fn note_other_prism(&mut self, record: usize, began: Option<f64>, setup_id: &str) {
        if let (Some(prism), Some(began)) = (self.prism, began)
            && began != prism
        {
            self.other_prism.add_made(record, || {
                format!(
                    "{} m in setup {setup_id}, which began with {} m",
                    metres_text(prism),
                    metres_text(began)
                )
            });
        }
    }

    fn shot_block(&mut self, block: &Block<'_>) {
        if let Some(setup) = self.pending.take() {
            self.open_setup(
                setup.slot,
                setup.instrument_height,
                setup.record,
                setup.wide,
                &setup.time,
                &setup.time_without_year,
            );
        } else if self.active.is_none() {
            self.implicit_setup(block);
        }
        self.note_settings(block, true);
        let active = self
            .active
            .expect("a setup is open: the pending one or an implicit one was just opened");

        if !block.time19.is_empty() || !block.time18.is_empty() {
            let known = self.result.project.stations[active].instrument.time.known();
            if !known && self.pointings == 0 {
                let mut time = self.result.project.stations[active].instrument.time.clone();
                let mut without_year = String::new();
                self.time_of(block, &mut time, &mut without_year);
                let station = &mut self.result.project.stations[active];
                station.instrument.time = time;
                if !without_year.is_empty() {
                    station
                        .metadata
                        .entry(String::from("time (no year recorded)"))
                        .or_insert(without_year);
                }
            } else {
                self.time_words.add(block.record, b"");
            }
        }
        if block.station_coordinates() {
            self.place_point(
                self.active_slot,
                block.e0,
                block.n0,
                block.h0,
                block.station_input_mode,
                true,
                block,
            );
        }
        let mut instrument_height = self.active_height.unwrap_or(0.0);
        if let Some(height) = block.instrument_height {
            instrument_height = height;
            if self.active_height.is_none() {
                self.active_height = Some(height);
                self.result.project.stations[active].setup.instrument_height = height;
            }
        }

        // The name compared by its bytes, as the C++ compares it with the
        // occupied point's id: the index holds every slot by its bytes.
        let found = self.slot_index.get(block.name).copied();
        if found == Some(self.active_slot) {
            let message = format!(
                "measures point {}, the point the instrument stands on; the measurement was not \
                 read",
                self.slots[self.active_slot].id
            );
            self.warn(block.record, message);
            self.result.records_skipped += 1;
            self.result.records_read -= 1;
            return;
        }
        let target = match found {
            Some(found) => found,
            None => self.new_slot(block.name, block.record, block.wide),
        };
        // GSI has no backsight word. A setup is made on the instrument as a
        // station and then an orientation shot, and the file records them in
        // that order; so a setup's FIRST shot, when it is to a point the file
        // gives coordinates, is taken as its backsight. Orienting on a point
        // of known position is sound whatever the observer called the shot; a
        // first shot to an unknown point orients nothing and is not named.
        // The coordinates may come LATER in the file - a traverse is often
        // begun on a mark keyed in only when the instrument stands on it -
        // since a reduction has the whole file; finish() settles those. They
        // may not come from this setup's own shots, which the instrument
        // computed with the orientation the backsight is to give. The import
        // says how many setups this gave.
        if self.pointings == 0
            && self.result.project.stations[active]
                .backsight_point_id
                .is_empty()
        {
            if self.slots[target].positioned {
                let id = self.slots[target].id.clone();
                let station = &mut self.result.project.stations[active];
                station.backsight_point_id = id;
                station
                    .metadata
                    .entry(String::from("backsight"))
                    .or_insert_with(|| {
                        String::from(
                            "the setup's first shot, to a point with coordinates earlier in the \
                             file (GSI marks no backsight)",
                        )
                    });
                self.backsighted += 1;
            } else {
                self.first_shots.push(FirstShot {
                    station: active,
                    slot: target,
                });
            }
        }
        if block.target_coordinates() {
            let positioned = self.slots[target].positioned;
            self.place_point(
                target,
                block.e,
                block.n,
                block.h,
                block.target_input_mode,
                false,
                block,
            );
            if !positioned && self.slots[target].positioned {
                self.slots[target].positioned_by_shot_of = active + 1;
            }
        }
        self.point_code(target, block);
        let to = self.slots[target].id.clone();
        let from = self.slots[self.active_slot].id.clone();
        track(&mut self.hz_input_mode, block.hz_input_mode);
        track(&mut self.v_index, block.v_index);

        let target_height = block
            .reflector_height
            .unwrap_or(self.reflector_height.unwrap_or(0.0));
        if block.reflector_height.is_none() && self.reflector_height.is_none() {
            self.shots_without_reflector_height += 1;
        }
        self.pointings += 1;
        let mut pointing = Pointing {
            index: self.pointings,
            face: Face::Unknown,
        };
        let mut zenith = None;
        if let Some(v) = block.v {
            let reading = wrap_to_circle(v);
            if reading > 0.0 && reading < PI {
                pointing.face = Face::Left;
            } else if reading > PI {
                pointing.face = Face::Right;
            }
            zenith = Some(if reading > PI {
                TWO_PI - reading
            } else {
                reading
            });
            self.zenith_angles += 1;
        }
        // Each observation's source is the file's, at this record.
        let source = self.source_at(block.record, block.wide);
        let precision = self.precision;
        let prism = self.prism;
        let (began, setup_id) = {
            let station = &self.result.project.stations[active];
            (station.instrument.prism_constant, station.setup.id.clone())
        };
        let distance_of = |value: f64, kind: DistanceKind| {
            let mut target_info = TargetInfo::default();
            if prism.is_some() {
                target_info.prism_constant = prism;
                target_info.prism_constant_state = CorrectionState::Applied;
            }
            Observation::Distance(DistanceObservation {
                from: from.clone(),
                to: to.clone(),
                distance: value,
                sigma: distance_sigma(&precision, value),
                kind,
                instrument_height,
                target_height,
                source: source.clone(),
                pointing,
                target: target_info,
            })
        };
        let mut observations: Vec<Observation> = Vec::new();
        if let Some(hz) = block.hz {
            self.directions += 1;
            observations.push(Observation::HorizontalDirection(
                HorizontalDirectionObservation {
                    at: from.clone(),
                    to: to.clone(),
                    direction: wrap_to_circle(hz),
                    sigma: precision.direction,
                    source: source.clone(),
                    pointing,
                },
            ));
            self.kept_sixty(block.record, block.hz_sixty);
        }
        if let Some(angle) = zenith {
            observations.push(Observation::ZenithAngle(ZenithAngleObservation {
                from: from.clone(),
                to: to.clone(),
                angle,
                sigma: precision.zenith,
                instrument_height,
                target_height,
                source: source.clone(),
                pointing,
            }));
            self.kept_sixty(block.record, block.v_sixty);
        }
        // The instrument's own horizontal distance and height difference (32,
        // 33) are what it computed from the slope distance and its vertical
        // reading. Beside a zenith angle and a distance that were both read
        // they are DERIVED: the reduction computes its own from those, and
        // carrying both would count one measurement twice. Without either -
        // no zenith angle read beside the slope distance (none recorded, or
        // one refused at its record), or no distance beside the zenith angle
        // - the reduction can compute neither, and they are the shot's only
        // horizontal distance and height, so they are carried as the file
        // gives them. A pointing keeps one distance, the one it is radiated
        // by, and a slope distance with no zenith angle cannot be reduced to
        // horizontal: where the block has its own horizontal distance, that
        // is the shot's, and the slope distance is not carried (finish()
        // says how many).
        let slope = self.usable(block.slope, "slope distance", block.record);
        let mut derived = false;
        if let (Some(slope), Some(_)) = (slope, zenith) {
            observations.push(distance_of(slope, DistanceKind::Slope));
            self.note_other_prism(block.record, began, &setup_id);
            derived = block.horizontal.is_some() || block.height_difference.is_some();
        } else {
            // Beside a slope distance, a horizontal distance of zero -
            // Leica's "none measured" - is passed over for the slope
            // distance, not counted as a shot without one.
            let horizontal = if slope.is_some() && block.horizontal == Some(0.0) {
                None
            } else {
                self.usable(block.horizontal, "horizontal distance", block.record)
            };
            if let Some(horizontal) = horizontal {
                observations.push(distance_of(horizontal, DistanceKind::Horizontal));
                self.note_other_prism(block.record, began, &setup_id);
                if slope.is_some() {
                    self.slope_without_zenith.add(block.record, b"");
                }
            } else if let Some(slope) = slope {
                observations.push(distance_of(slope, DistanceKind::Slope));
                self.note_other_prism(block.record, began, &setup_id);
            }
            if let Some(height_difference) = block.height_difference {
                if zenith.is_some() && horizontal.is_some() {
                    derived = true;
                } else {
                    let length = match block.horizontal {
                        Some(horizontal) if horizontal > 0.0 => horizontal,
                        _ => 0.0,
                    };
                    // Both heights measured with a tape, and the angle the
                    // instrument reduced with over the length of the line.
                    let sigma = Vec3::new(
                        precision.height_measurement,
                        precision.height_measurement,
                        length * precision.zenith,
                    )
                    .length();
                    observations.push(Observation::LevelDifference(LevelDifferenceObservation {
                        from: from.clone(),
                        to: to.clone(),
                        height_difference,
                        sigma,
                        length,
                        source: source.clone(),
                    }));
                }
            }
        }
        if derived {
            self.derived_values.add(block.record, b"");
        }
        self.result.project.stations[active]
            .observations
            .extend(observations);
    }

    // A 60-second word whose value became an observation: counted, and its
    // record listed, for the note in finish(). Counted here, not where the
    // word is decoded, so that a word the block does not keep - in a code
    // block, or a shot at the occupied point - is not said to be read.
    fn kept_sixty(&mut self, record: usize, sixty: Option<SixtySeconds>) {
        if let Some(sixty) = sixty {
            self.rounded_seconds.add_made(record, || sixty_text(&sixty));
            self.rounded_second_records.add(record);
        }
    }

    fn point_block(&mut self, block: &Block<'_>) {
        let index = self.slot_for(block.name, block.record, block.wide);
        if block.target_coordinates() {
            self.place_point(
                index,
                block.e,
                block.n,
                block.h,
                block.target_input_mode,
                false,
                block,
            );
        }
        self.point_code(index, block);
        self.note_settings(block, true);
        if !block.time18.is_empty() || !block.time19.is_empty() {
            self.time_words.add(block.record, b"");
        }
    }

    fn finish(mut self) -> ReadResult {
        self.discard_pending_setup();
        self.close_setup();
        // A first shot to a point positioned since is its setup's backsight,
        // unless the position came from that setup's own shots (shot_block).
        let first_shots = std::mem::take(&mut self.first_shots);
        for first in &first_shots {
            let slot = &self.slots[first.slot];
            if !slot.positioned || slot.positioned_by_shot_of == first.station + 1 {
                continue;
            }
            let id = slot.id.clone();
            let station = &mut self.result.project.stations[first.station];
            station.backsight_point_id = id;
            station
                .metadata
                .entry(String::from("backsight"))
                .or_insert_with(|| {
                    String::from(
                        "the setup's first shot, to a point given coordinates later in the file \
                         (GSI marks no backsight)",
                    )
                });
            self.backsighted += 1;
            self.backsighted_later += 1;
        }
        if !self.pending_codes.is_empty() {
            let codes = std::mem::take(&mut self.pending_codes);
            // Nothing followed the last code block(s): the point before is the
            // only one they can belong to - the point of the last block that
            // named one, not the last point the file happened to name first.
            if let Some(last) = self.last_point_slot {
                let message = format!(
                    "no point follows this code block, so it is attached to the point before it, \
                     {}",
                    self.slots[last].id
                );
                self.warn(codes[0].record, message);
                let base = self.codes_since_point;
                for (c, code) in codes.iter().enumerate() {
                    self.attach_code(last, code, base + c);
                }
            } else {
                for code in &codes {
                    self.warn(
                        code.record,
                        format!(
                            "code block {} belongs to no point: no block before or after it names \
                             one",
                            shown(&code.code)
                        ),
                    );
                }
            }
        }
        self.flush_feature();
        self.close_feature();

        let mut project = std::mem::take(&mut self.result.project);
        let slots = std::mem::take(&mut self.slots);
        let positioned = slots.iter().filter(|slot| slot.positioned).count();
        project.points.reserve(positioned);
        project
            .unpositioned_points
            .reserve(slots.len() - positioned);
        for slot in slots {
            let source = self.source_at(slot.record, slot.wide);
            if slot.positioned {
                project.points.push(SurveyPoint {
                    id: slot.id,
                    northing: slot.northing,
                    easting: slot.easting,
                    elevation: slot.elevation,
                    code: text(&slot.code),
                    metadata: slot.metadata,
                    coordinate_source: slot.coordinate_source,
                    source,
                    ..Default::default()
                });
            } else {
                project.unpositioned_points.push(UnpositionedPoint {
                    id: slot.id,
                    code: text(&slot.code),
                    metadata: slot.metadata,
                    source,
                    ..Default::default()
                });
            }
        }
        project.name = self.file_name.clone();
        project.source = self.narrow_source.clone();
        if self.wide_blocks != 0 {
            project.source.format_version = String::from(if self.narrow_blocks == 0 {
                "GSI-16"
            } else {
                "GSI-8 and GSI-16"
            });
        }
        project.units.linear = self.linear_unit;
        project.units.angular = self.angular_unit;
        project
            .metadata
            .entry(String::from("parser version"))
            .or_insert_with(|| String::from(PARSER_VERSION));
        project
            .metadata
            .entry(String::from("code blocks belong to"))
            .or_insert_with(|| {
                String::from(if self.codes_after_points {
                    "the point before them"
                } else {
                    "the point after them"
                })
            });

        // ---- What the file did many times, said once each ----
        let warnings = &mut self.warnings;
        once(warnings, &self.explicit_points, || {
            format!(
                "{} a decimal point (the first {}), which the GSI specification places by the \
                 unit digit instead; the values were read as written",
                plural(
                    self.explicit_points.count,
                    "numeric word writes",
                    "numeric words write"
                ),
                shown(&self.explicit_points.example)
            )
        });
        once(warnings, &self.rounded_seconds, || {
            let one = self.rounded_seconds.count == 1;
            format!(
                "{} 60 seconds, where a sexagesimal angle's seconds run to 59 (at {}{}{}): a \
                 writer that rounds the seconds on their own, without carrying into the minutes, \
                 writes 60 for a value within its rounding of the next minute, and 60 seconds is \
                 exactly that minute, so {} read as the next minute, within the writer's \
                 rounding of what was measured",
                plural(
                    self.rounded_seconds.count,
                    "angle word writes",
                    "angle words write"
                ),
                self.rounded_second_records.text(),
                if one { ": " } else { "; the first, " },
                self.rounded_seconds.example_text(),
                if one { "it was" } else { "each was" }
            )
        });
        if self.unpointed_lengths.count < self.pointed_lengths {
            // Both readings of such a word are guesses but one: the unit digit's
            // is the specification's, and it is the one taken. A real export has
            // been seen with two such words among some 1,500 that write a point,
            // each far from its neighbours, so the person is told where to look.
            once(warnings, &self.unpointed_lengths, || {
                format!(
                    "{} the decimal point to the unit digit (the first {}) where the file's \
                     other {} write one: they were read by the unit digit, as the specification \
                     says, but a file that mixes the two forms may have lost a point or a digit \
                     there; check those values",
                    plural(
                        self.unpointed_lengths.count,
                        "length word leaves",
                        "length words leave"
                    ),
                    shown(&self.unpointed_lengths.example),
                    self.pointed_lengths
                )
            });
        }
        once(warnings, &self.height_text_as_code, || {
            format!(
                "{} text where a height belongs (the first {}, in word 87 or 88, which GSI \
                 defines as a reflector or instrument height); it was read as the point's code, \
                 as some exporters write it there",
                plural(self.height_text_as_code.count, "block has", "blocks have"),
                shown(&self.height_text_as_code.example)
            )
        });
        once(warnings, &self.height_text_kept, || {
            format!(
                "{} text in a height word beside a code of its own; the text is in the point's \
                 metadata",
                plural(self.height_text_kept.count, "block has", "blocks have")
            )
        });
        once(warnings, &self.zero_code_words, || {
            format!(
                "{} all zeros (the first, {}), which is how GSI writes an empty value, padding \
                 text with '0': they were read as empty, not as the code '0', which would be \
                 written the same",
                plural(
                    self.zero_code_words.count,
                    "remark or code information word is",
                    "remark or code information words are"
                ),
                self.zero_code_words.example_text()
            )
        });
        once(warnings, &self.remarks_in_code_blocks, || {
            format!(
                "{} in a code block (the first, {}), where nothing tells whether they are a \
                 point's code and remarks or more about the block's own code: they were not read",
                plural(
                    self.remarks_in_code_blocks.count,
                    "remark word (71-79) is",
                    "remark words (71-79) are"
                ),
                self.remarks_in_code_blocks.example_text()
            )
        });
        once(warnings, &self.coordinate_records, || {
            format!(
                "{} station coordinates (words 84-86) or an instrument height with no \
                 measurement after them (the first names point {}); they were read as the \
                 coordinates of the point they name, not as setups",
                plural(self.coordinate_records.count, "block gives", "blocks give"),
                self.coordinate_records.example_text()
            )
        });
        once(warnings, &self.zero_distances, || {
            format!(
                "{} a distance of zero, Leica's way of saying none was measured; those shots \
                 have no distance",
                plural(self.zero_distances.count, "block records", "blocks record")
            )
        });
        once(warnings, &self.other_prism, || {
            format!(
                "{} measured with a prism constant other than the one {} setup began with (the \
                 first, {}): each keeps the constant the instrument applied to it, so if the \
                 prism was not changed, those distances are long or short by the difference",
                plural(self.other_prism.count, "distance was", "distances were"),
                if self.other_prism.count == 1 {
                    "its"
                } else {
                    "their"
                },
                self.other_prism.example_text()
            )
        });
        once(warnings, &self.odd_width, || {
            format!(
                "{} not the width its block calls for (8 characters of data in GSI-8, 16 in \
                 GSI-16; the first {}); they were read as far as they go",
                plural(self.odd_width.count, "word is", "words are"),
                shown(&self.odd_width.example)
            )
        });
        once(warnings, &self.feet, || {
            String::from(
                "the file is in feet, and GSI does not say which foot: US survey feet were \
                 assumed (1200/3937 m); if the instrument was set to international feet every \
                 length is 2 ppm long",
            )
        });
        once(warnings, &self.mixed_linear_units, || {
            String::from(
                "the file mixes length units; each value was converted from its own, and the \
                 file is described by the first",
            )
        });
        once(warnings, &self.mixed_angular_units, || {
            String::from(
                "the file mixes angle units; each value was converted from its own, and the \
                 file is described by the first",
            )
        });
        once(warnings, &self.height_only, || {
            format!(
                "{} a height and no easting or northing (the first {}); the height is in the \
                 point's metadata and the point has no position",
                plural(self.height_only.count, "point has", "points have"),
                self.height_only.example_text()
            )
        });
        once(warnings, &self.partial_position, || {
            format!(
                "{} an easting without a northing or the reverse (the first {}); it has no \
                 position",
                plural(self.partial_position.count, "point has", "points have"),
                self.partial_position.example_text()
            )
        });
        once(warnings, &self.levelling, || {
            format!(
                "{} a digital level's words (three-digit word indices such as 330, the staff \
                 readings; the first names point {}): this parser reads total station data, so \
                 their points are listed and the levelling is not read",
                plural(self.levelling.count, "block holds", "blocks hold"),
                self.levelling.example_text()
            )
        });
        once(warnings, &self.special_codes, || {
            format!(
                "{} a digital level's special code block (a '?' in position 8, naming the \
                 levelling method; the first {}): the levelling is not read, and they were not \
                 taken as point codes",
                plural(self.special_codes.count, "code block is", "code blocks are"),
                shown(&self.special_codes.example)
            )
        });
        once(warnings, &self.time_words, || {
            format!(
                "{} a time the model has no place for (it keeps one per setup); those times were \
                 not carried",
                plural(self.time_words.count, "block records", "blocks record")
            )
        });
        for (index, tally) in self.unread.iter().enumerate() {
            once(warnings, tally, || {
                format!(
                    "word {index} ({}) appears in {} and is not read by this parser",
                    word_name(index),
                    plural(tally.count, "block", "blocks")
                )
            });
        }
        if warnings.unlisted != 0 {
            let message = format!(
                "{} not listed: the first {MAX_LISTED_WARNINGS} are",
                plural(
                    warnings.unlisted,
                    "further warning is",
                    "further warnings are"
                )
            );
            warnings.listed.push(ReadWarning {
                file_name: self.file_name.clone(),
                record: 0,
                message,
            });
        }

        // ---- What the file does not carry ----
        let mut lacking: Vec<String> = Vec::new();
        lacking.push(String::from(
            "GSI states no coordinate system: the coordinates are in whatever system the \
             instrument or controller was set to",
        ));
        let setups = project.stations.len();
        if setups != 0 {
            let found = if self.backsighted == 0 {
                String::from(
                    "no setup's first shot is to a point the file gives coordinates, other than \
                     by that setup's own shots, so no setup has one",
                )
            } else {
                format!(
                    "{} of {} took the point of their first shot as the backsight, because the \
                     file gives it coordinates{}",
                    self.backsighted,
                    plural(setups, "setup", "setups"),
                    if self.backsighted_later == 0 {
                        String::new()
                    } else {
                        format!(
                            " ({} of them from coordinates later in the file)",
                            self.backsighted_later
                        )
                    }
                )
            };
            lacking.push(format!(
                "GSI does not mark a backsight: {found}{}",
                if self.backsighted == setups {
                    ""
                } else {
                    "; a setup without one cannot be oriented until its backsight is known"
                }
            ));
        }
        if self.setups_without_height != 0 {
            lacking.push(format!(
                "no instrument height on {} of {} (read as 0)",
                self.setups_without_height,
                plural(setups, "setup", "setups")
            ));
        }
        if self.shots_without_reflector_height != 0 {
            lacking.push(format!(
                "no reflector height before {} (read as 0)",
                plural(self.shots_without_reflector_height, "shot", "shots")
            ));
        }
        if self.setups_without_ppm != 0 {
            lacking.push(format!(
                "no atmospheric ppm on {} of {}: whether their distances were corrected is not \
                 stated",
                self.setups_without_ppm,
                plural(setups, "setup", "setups")
            ));
        }
        if self.setups_without_prism != 0 {
            lacking.push(format!(
                "no prism constant on {} of {}: whether their distances include one is not stated",
                self.setups_without_prism,
                plural(setups, "setup", "setups")
            ));
        }
        if self.zenith_angles != 0 || self.negative_zeniths.count != 0 {
            let zeniths_read = self.zenith_angles != 0;
            let mut setting = String::from("GSI does not record the vertical angle setting: ");
            if zeniths_read {
                setting
                    .push_str("word 22 was read as a zenith angle, the instruments' usual setting");
            }
            if self.negative_zeniths.count != 0 {
                // Each was warned about at its record. What they may say of the
                // file's OTHER vertical readings, which were read, is said here
                // once: a positive elevation read as a zenith is no less wrong,
                // and nothing in its word shows it. Where every one was refused,
                // no word 22 was read, and there are no others to doubt.
                setting.push_str(&format!(
                    "{}{} refused as negative, which no zenith angle is (the first at record \
                     {}){}: such a reading is a damaged word, or an instrument's vertical angle \
                     setting reaching the file (GSI ONLINE's SET 44 reads V from the zenith, the \
                     horizon or in percent, and it does not say whether word 22 follows it){}",
                    if zeniths_read { "; " } else { "" },
                    plural(self.negative_zeniths.count, "reading was", "readings were"),
                    self.negative_zeniths.first_record,
                    if zeniths_read {
                        ""
                    } else {
                        ", and no word 22 was read as a zenith angle"
                    },
                    if zeniths_read {
                        "; if it was the setting, the file's other word 22 readings are not \
                         zenith angles either, and the shots read with them are wrong"
                    } else {
                        ""
                    }
                ));
            }
            lacking.push(setting);
        }
        if self.directions != 0 {
            // GSI ONLINE's SET/CONF 171, "Direction of horizontal circle reading
            // (Hz-Angle)": 0 clockwise, 1 counterclockwise - a setting, not a word.
            lacking.push(String::from(
                "GSI does not record the direction of the horizontal circle (an instrument \
                 setting): word 21 was read as increasing clockwise, as instruments are normally \
                 set; a job measured with the circle counterclockwise comes out mirrored about \
                 each setup's backsight",
            ));
        }
        if self.code_blocks != 0 {
            lacking.push(format!(
                "GSI does not record whether the instrument stored a code block before or after \
                 its point (its <Rec Free Code:> setting): {}",
                if self.codes_after_points {
                    "this file begins with a point block and ends with a code block, as a job \
                     recorded After Point does, so each code block was taken to code the point \
                     before it; had the instrument been set to Before Point, each code belongs \
                     to the point after its block"
                } else {
                    "each code block was taken to code the point after it, as a job recorded \
                     Before Point does; had the instrument been set to After Point, each code \
                     belongs to the point before its block"
                }
            ));
        }
        if self.derived_values.count != 0 {
            lacking.push(format!(
                "{} instrument-computed horizontal distance (32) or height difference (33) beside \
                 a zenith angle and a distance that were read: not carried, the reduction \
                 computes its own from those",
                plural(self.derived_values.count, "block's", "blocks'")
            ));
        }
        if self.slope_without_zenith.count != 0 {
            lacking.push(format!(
                "{} a slope distance (31) and no zenith angle that was read (none recorded, or \
                 one refused at its record; the first at record {}), so the slope distance \
                 cannot be reduced to horizontal: it was not carried, and the horizontal \
                 distance (32) the instrument computed for the same shot was carried in its \
                 place",
                plural(self.slope_without_zenith.count, "block has", "blocks have"),
                self.slope_without_zenith.first_record
            ));
        }
        self.result.not_carried.extend(lacking);
        self.result.project = project;
        self.result.warnings = std::mem::take(&mut self.warnings.listed);
        self.result
    }
}

// ---- The format --------------------------------------------------------------------

fn gsi_format() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(LEICA_GSI_FORMAT_ID),
        human_name: String::from("Leica GSI (GSI-8, GSI-16)"),
        manufacturer: Manufacturer::Leica,
        reads: FormatContent {
            points: true,
            observations: true,
            stations: true,
            features: true,
            coordinate_system: false,
            instrument_settings: true,
            gnss: false,
        },
        can_import: true,
        can_export: false,
        parser_version: String::from(PARSER_VERSION),
        extensions: vec![String::from("gsi")],
    }
}

// Does this line look like a GSI block: an optional '*', then words, the first
// of them word 11 or 41? Some((wide, every word the standard width)) when it
// does; wide when it began with '*'.
fn looks_like_block(line: &[u8]) -> Option<(bool, bool)> {
    let mut position = 0;
    let mut first = true;
    let mut wide = false;
    let mut standard_width = true;
    loop {
        while position < line.len() && is_blank(line[position]) {
            position += 1;
        }
        if position >= line.len() {
            return (!first).then_some((wide, standard_width));
        }
        let mut end = position;
        while end < line.len() && !is_blank(line[end]) {
            end += 1;
        }
        let mut token = &line[position..end];
        position = end;
        if first && token[0] == b'*' {
            wide = true;
            token = &token[1..];
        }
        let word = parse_word(token)?;
        if first && word.index != 11 && word.index != 41 {
            return None;
        }
        let width = if wide { 16 } else { 8 };
        standard_width = standard_width && word.data.len() == width;
        first = false;
    }
}

fn probe_gsi(input: &ProbeInput<'_>) -> FormatSignature {
    let mut rest = without_byte_order_mark(input.bytes);
    let mut lines = 0usize;
    let mut blocks = 0usize;
    let mut wide = 0usize;
    let mut standard = 0usize;
    while !rest.is_empty() && lines < 200 {
        let end = rest.iter().position(|&c| is_line_end(c));
        if end.is_none() && input.truncated {
            break; // cut by PROBE_BYTES, not by the file
        }
        let line = &rest[..end.unwrap_or(rest.len())];
        rest = match end {
            Some(end) => &rest[end + 1..],
            None => &[],
        };
        if line.iter().all(|&c| c == b' ' || c == b'\t' || c == 0x1a) {
            continue;
        }
        lines += 1;
        if let Some((is_wide, is_standard)) = looks_like_block(line) {
            blocks += 1;
            wide += usize::from(is_wide);
            standard += usize::from(is_standard);
        }
    }
    if blocks == 0 {
        return ruled_out();
    }
    let mut evidence =
        format!("{blocks} of {lines} lines are GSI blocks beginning with word 11 or 41");
    if blocks != lines {
        // A GSI file is blocks and nothing else; a few stray lines is a damaged
        // one, many is something else that happens to contain such lines.
        let share = blocks as f64 / lines as f64;
        evidence.push_str(", the rest are not");
        return FormatSignature {
            confidence: if share >= 0.9 { 0.6 } else { 0.2 * share },
            evidence,
        };
    }
    let mut confidence = if blocks >= 3 { 0.93 } else { 0.8 };
    evidence.push_str(if wide == blocks {
        ", GSI-16 (lines begin with '*')"
    } else if wide == 0 {
        ", GSI-8"
    } else {
        ", GSI-8 and GSI-16 mixed"
    });
    if standard == blocks {
        confidence += 0.03;
        evidence.push_str(", every word the standard width");
    }
    if input.extension == "gsi" {
        confidence += 0.03;
        evidence.push_str(", extension .gsi");
    }
    FormatSignature {
        confidence,
        evidence,
    }
}

fn read_gsi(bytes: &[u8], file_name: &str, options: &ReadOptions) -> Result<ReadResult> {
    let precision = &options.precision;
    if !(precision.direction > 0.0)
        || !(precision.zenith > 0.0)
        || !(precision.height_measurement > 0.0)
        || !(precision.distance_constant > 0.0 || precision.distance_ppm > 0.0)
        || !precision.direction.is_finite()
        || !precision.zenith.is_finite()
        || !precision.height_measurement.is_finite()
        || !precision.distance_constant.is_finite()
        || !precision.distance_ppm.is_finite()
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the default precision given for a GSI import must be positive: every observation \
             needs a standard deviation",
        )
        .with_context(file_name));
    }
    let mut reader = GsiReader::new(file_name, options);
    reader.read(bytes);
    let result = reader.finish();
    if result.records_read == 0 {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "no GSI block could be read: a GSI file is lines of words, each beginning with word \
             11 (a point) or 41 (a code)",
        )
        .with_context(file_name));
    }
    Ok(result)
}

pub(crate) fn registration() -> FormatRegistration {
    FormatRegistration::new(gsi_format(), probe_gsi, Some(read_gsi))
}
