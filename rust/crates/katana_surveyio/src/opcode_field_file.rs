//! The opcode field file (.fld): a survey job as tab-separated records, each
//! opening with a numeric operation code - total-station setups, resections
//! and shots, entered coordinates and RTK positions, coded strings and their
//! attributes.
//!
//! The port of src/katana_surveyio/opcode_field_file.cpp. The format has no
//! public interface of its own: it is reached through the registry
//! (`registration`), as the C++ reaches it through its static registration.

// Specification this reader implements:
//   [FLD] "Field File Format", the format publisher's reference manual
//         chapter, in the edition the owner supplied: sections 44.2
//         "Structure of the .fld File", 44.3 "Point Description", 44.4
//         "Measurements and Named Measurements", 44.5 "Searching for Special
//         Coordinates", 44.6 (time_text), 44.7 and 44.8, one entry per opcode.
//         The June 2025 edition numbers the same sections 1.2 to 1.8; it has
//         the entries and .fld syntax of 128, 129, 138 and 139, but not their
//         descriptions, nor 44.7's resection_measurement block.
//
// What [FLD] says, and this reader relies on:
//   * 44.2: a record is "a numeric operation code ... followed by zero or
//     more tabs and pieces of information", so an opcode alone on its line
//     (20, 99, 129) is a record.
//   * 44.3: a POINT DESCRIPTION is five tab-separated values - feature code,
//     string number, point ID, point name, point comment - in that order, a
//     missing one still taking its tab.
//   * 44.4: a measurement (02, 07 among the six kinds) makes a point that "is
//     appended to previous points with the same feature code and string
//     number": a feature is a code and a string number, strung in the order
//     measured. The CURRENT MEASUREMENT POINT is the last one made, and the
//     CURRENT STRING the string it was appended to.
//   * 44.5: a setup, a backsight and a check find their point by a search
//     that stops at the first found: the directly entered coordinates before
//     them, then the measurements before them, and in each the record's point
//     name among their point names, then its point name among their point
//     IDs, then its point ID among their point IDs (steps 4 to 9; steps 1 to
//     3 search the reducing program's own models, and step 10 asks the
//     person).
//   * 44.8, per opcode ("Optional information is enclosed in square
//     brackets"):
//       100 units, "only one choice for each": decimal degrees, metres, and
//           millimetres and celsius for pressure and temperature.
//       02  a "directly entered coordinate": description, X, Y, Z, of which
//           "No reduction is needed".
//       03  a setup on the point the description names, with its instrument
//           height. 128 a setup on an unknown point, whose coordinates "a
//           Least Squares Resection" computes from measurements to known
//           points; 129 ends its block, which "In some cases ... is not
//           necessary". 138 and 139 are the same for a Helmert resection.
//       04  the backsight: description, horizontal circle, vertical circle,
//           slope distance and an azimuth_value, which "may be specified when
//           no coordinate for the backsight point exists". 06 a check
//           measurement and 07 a measurement: description, horizontal and
//           vertical circle, slope distance; decimal degrees and metres.
//       05  the target height for 04, 06 and 07 after it; 09 a scale factor
//           "to apply to subsequent slope distances".
//       16  "Additional coding for the current measurement point": "A new
//           measurement point is created at the same position", with this
//           record's code and string number.
//       20  close string: with no description "the current string is
//           closed"; with one, the string its code and number name, else
//           "the string containing that point ID".
//       42, 43, 44  a radial, tangential or height offset of "the current
//           measured point", or of the last point of the string a
//           description's code and number name, else of "the point with that
//           point ID". Radial:
//           "along the plan line joining the current station to the specified
//           point", positive away from the station; tangential: "at rights
//           angles" to that line, "negative ... to the left (looking from the
//           station)" - each "from the specified points original position".
//           Height: it "adjusts the height of the point" where that height "is
//           not null".
//       47  ends the current string before the current measurement point,
//           which "becomes the first point of a new string with the same
//           feature code and string number"; 48 ends it after that point.
//       29  a memo; 41 text for the current measurement point, "any spaces
//           from column four onwards" being part of it; 71, 72, 73 an
//           integer, a real and a text attribute (name, value) of it, an
//           attribute whose name is blank being "unnamed"; -2 a comment.
//       99  "Stop processing ... at this line."
//       124 and 125 (an attribute group and its end), 140 (a GNSS coordinate)
//           and 145: "This opcode does not exist in the fld file."
//
// Readings that are this reader's own, stated so they can be checked:
//   * THE COLUMN AFTER THE OPCODE. The files this reader was written against
//     put one field more than [FLD]'s syntax at the front of every record
//     with values ("07<tab><tab>EB<tab>01..."): their 04 has as many fields as
//     [FLD]'s 04 with its azimuth, and their 128 a description and a value
//     [FLD]'s .fld syntax does not give it. Where the column is filled it
//     holds the date and time the record was made (the RTK job's, on each of
//     its coordinates, equal to the point's own Date and Time attributes) -
//     the counterpart of the XML form's time_surveyed, though [FLD] gives the
//     op_code_properties that carry it to the XML form only. A record whose
//     first field is a date or time has the column: only the column holds
//     one. A record whose first field is filled otherwise has not: that is a
//     feature code, and the column holds nothing else. A record whose first
//     field is blank cannot say - a record in the column's layout that has
//     lost a value has [FLD]'s count, and a feature code left empty makes the
//     first field empty - so the file decides it: the records whose opcode has
//     a fixed number of values (02, 03, 04, 06, 07) vote before any is read, a
//     count that fits only the column's layout, with the first field blank or
//     a date or time, for it, and a count that fits only [FLD]'s, with the
//     first field filled, against, and most votes win. Two kinds of record fit
//     both layouts: a 04 of ten fields (the column and no azimuth, or the
//     azimuth and no column) and a record one field longer than [FLD]'s whose
//     last field is blank (a trailing tab, or the column and a blank last
//     value); they vote only with a date or time first. Such a record with a
//     blank first field is read in the file's layout, unless that leaves its
//     first measured value (the horizontal circle, X, the instrument height)
//     blank where the other layout reads a whole record - a number there, and
//     a point named - the one sign which layout wrote it: it is then skipped,
//     saying so. Text there (a setup's comment, read without the column, is
//     where its instrument height would be) or a reading that names no point
//     is no such sign, and the record is read. A fixed record that does not
//     fit the layout it is read in is a warning and is skipped - a guess at
//     which value is the slope distance reads a plausible survey that is
//     wrong. One blank field past a layout's count is a trailing tab. A record
//     of free text takes the same rules: in a file with the column, a filled
//     first field that is not a date or time is read as a record written
//     without it; in a file without it (or one no fixed record decided, read
//     as [FLD] writes it), a record of fixed form (05, 09, 100, the offsets,
//     16, 20, 128) whose first value is blank and that fits only without that
//     value carries the column. A date or time is known by its shape
//     (is_date_or_time) and kept as text, "time stamp", on the point or setup
//     its record makes: which of the first two numbers of a date such as
//     03/04/26 is the day the file does not say.
//   * An opcode may have blanks or tabs before it (" 2") or lack its leading
//     zero ("7"): [FLD] calls it numeric and writes 1 to 9 as 01 to 09, and
//     nothing makes 07 differ from 7. The probe reads a line the same way.
//   * A LINE THAT IS NO RECORD (its first field is not an opcode) may have
//     been one, and what it made is not known: it ends the current point and
//     measurement, so what follows it that would describe them is skipped,
//     naming it. A number the reader does not take as an opcode ("0007",
//     "+3") is taken for the one it would be, for that bookkeeping only: one
//     that starts a setup leaves no setup, so the shots after it are not filed
//     under the setup before.
//   * The first line of the files is "{Version 6.0}", which [FLD] does not
//     mention: kept as "version". A line like it after the first record is no
//     record.
//   * X is the easting and Y the northing. [FLD] says only "(x, y, z)"; the
//     files put six-figure eastings in X and seven-figure MGA northings in Y,
//     and one labels a reference station's Easting and Northing so.
//   * A point is identified by its point name where it has one and by its
//     point ID otherwise. A setup, a backsight and a check (03, 04, 06) find
//     their point as 44.5 searches, among the coordinates (02) and then the
//     shots (07) before them, and stand on or measure a point of their own
//     name (else ID) only where the search finds none: a 03 that names the
//     ID a coordinate was given - alone, or with a name of its own - stands
//     on that coordinate. What such a record calls the point is kept, a
//     setup's with the setup ("point name", "point ID") and a backsight's or
//     a check's on the point under its record, rather than made a point of
//     its own. A point that only a setup, a backsight or a check made is in
//     neither of 44.5's lists, so a record whose name is such a point and
//     whose ID a coordinate or a shot gave finds that coordinate or shot; a
//     record whose name is another point than the one found is warned of. A
//     20 and an offset (42 to 44) find the point their description's point ID
//     names, as [FLD] 44.8 says: the shot's (07) where a shot and a coordinate
//     (02) were both given it - [FLD] calls a point ID "normally unique", the
//     ID a measurement is given, and an offset moves a shot - with a warning,
//     and for a 20 the coordinate's where only its string is open. Else, the
//     reader's own reading, they find the point of its name. A fixed record
//     naming neither is skipped.
//   * The vertical circle is a zenith reading: [FLD] never states its zero,
//     and the files' readings near 90 and 270 degrees are zeniths on the two
//     faces. One past 180 degrees is face right, held as 360 - z.
//   * AN RTK POSITION IS A GNSS POSITION. [FLD] gives a GNSS coordinate
//     opcode 140 and says 140 "does not exist in the fld file", so a .fld
//     writes an RTK position as an 02, its "directly entered coordinate", with
//     the receiver's attributes after it. An 02 whose own attributes - those
//     after it, before the next record that makes a point or a setup -
//     include one named "GNSS Solution" (what the receiver solved) is a
//     survey GnssPositionObservation of an unpositioned point, in the file's
//     grid, with the a-priori GNSS precision of ReadOptions: the receiver's
//     quality attributes are its own words, kept as attributes. Every other
//     02 is an entered coordinate. The reduction seeds a GNSS position before
//     any setup, as it seeds an entered coordinate, so a total-station setup
//     on an RTK mark orients on another; it reports the point as GNSS, not as
//     held control; and it checks a mark's second position against its
//     first. The Z is the mark's, as [FLD]'s 02 needs "No reduction": an
//     antenna height among the attributes is kept, not subtracted again. An
//     02 with no Z stays an entered coordinate (a GNSS position has a
//     height), with a warning.
//   * THE ATTRIBUTES OF A POINT MEASURED MORE THAN ONCE. [FLD] makes every
//     measurement a point of its own; Katana makes one point of a name and
//     keeps every measurement's attributes on it. The first record that makes
//     a point current (02, 04, 06, 07) owns the plain names; the time stamp,
//     the comment and the attributes after each later record about the same
//     point - a face pair, a mark measured again, a check, a backsight - are
//     kept under "record N/", N that record. 16 makes no second point here
//     (see below), so what follows it is still the current record's; a name
//     given twice there keeps its second value, where it differs, as "Name
//     (record M)", M the attribute's record, with a warning.
//   * ATTRIBUTE GROUPS (124 and 125). [FLD] defines them for the XML form
//     only: a name and a level, the attributes after it being in the group
//     until its end, 125. The files write 124 with an empty value, the name
//     and the level, and 125 with an empty name and the level. An attribute
//     inside a group is kept as "Group/Name" - a reference station's
//     "Easting" is not the point's - and nested groups as "Outer/Inner/Name",
//     "/" being how Katana's properties show a tree. A group still open when
//     a record makes another point or a setup, or when a 124 gives the level
//     of a group still open, is ended there, with a warning. A group nested
//     past GROUP_DEPTH named groups, or whose name would take the groups' part
//     of a name past GROUP_PATH_LENGTH characters, is read but not named in
//     its attributes' names, with a warning: named, K groups nested under one
//     point gave its K attributes names, and memory, that grew with K squared.
//   * 04 and 06 targets take the attributes after them as a shot's does.
//     [FLD] gives 41 and 71 to 73 to the current measurement point, which
//     neither makes, so this is the reader's reading: the files write a
//     check's date, time and target height after its 06, and a 04's are taken
//     the same way (the files write none). A check (06) does not string its
//     target ([FLD] makes a check a one-vertex string of its own): "check
//     measurement" on the point names the setup and the check's coding. A
//     backsight's (04) coding is kept in its setup's metadata, and a setup's
//     (03, 128): [FLD] strings a measurement point, which neither makes.
//   * THE BACKSIGHT. A setup's backsight (SurveyStation::backsight_point_id)
//     is the point of its first 04 that can orient it: one a 04 gives a
//     horizontal circle to, and which has a position the reduction will find -
//     coordinates an 02 gives it anywhere in the file (the reduction places
//     every one before any setup), or a direction and a distance measured to
//     it from an earlier setup - or which a 04 gives an azimuth. Where no 04's
//     point can, it is the first point a 04 gives a horizontal circle to, the
//     reduction then taking that circle as set to the grid and saying so;
//     where no 04 gives one, the first 04's point. It is settled when the file
//     ends (settle_backsights), so an 02 after the setup still counts. The
//     circle and the azimuth are that point's: SurveyStation::backsight_azimuth
//     is the circle reading on it, the horizontal circle of the first 04 to it
//     that gives one, as its face-left reading (a face-right one less 180
//     degrees, as the reduction means the pointings to the backsight), and a
//     later 04 to it changes it no more than a later pointing would; the 04's
//     azimuth_value is SurveyStation::stated_backsight_azimuth, the first a 04
//     to it gives, a later one that differs kept in the setup's metadata, with
//     a warning. The reduction orients the setup on that azimuth, less the
//     reading on the backsight, only where nothing places the backsight -
//     [FLD]'s "when no coordinate for the backsight point exists". A 04 to any
//     other point is read as a measurement of that point, with a warning, its
//     azimuth kept in the setup's metadata: the model orients a setup once, on
//     one point, and the circle or azimuth of one 04 less the reading on
//     another's point turns every direction of the setup by the difference.
//     Not simply the first 04 or the last: where that one is to a point
//     nothing places, with no azimuth, either rule orients the setup on its
//     circle taken as a grid azimuth although another 04 of the setup is to
//     control, and a first one with no horizontal circle leaves the setup
//     unoriented. [FLD]'s 04 entry says only that the reducing program shows
//     each 04's "bearing datum difference"; that it holds for the
//     measurements after that 04 is an inference from opcode 50's entry,
//     which says so of its own bearing, and re-orienting them so would split
//     the setup in two.
//   * A record that makes a point but is not read leaves no current point,
//     and a setup that is not read leaves no setup: what follows is skipped,
//     naming that record, rather than given to the point or setup before. So
//     does a record about a point this reader does not read (10, 11, 12, 14,
//     140) and an opcode [FLD] does not define.
//   * OFFSETS (42, 43, 44) are applied to the one shot (07) of their point
//     from the current setup, through the builder's offset_shot: the shot's
//     direction, zenith and slope distance become those of the offset
//     position, and what was measured is kept in the point's metadata ("shot
//     record N as measured"). A radial, a tangential and a height offset of
//     one shot combine, each from the measured position; two of one kind
//     apply to none of the shot, since whether the second adds to the first or
//     replaces it [FLD] does not say. They are applied when the setup ends.
//     [FLD] moves the measurement an offset follows, a point of its own there;
//     Katana makes one point of every measurement of a name, and moving one
//     of several pointings the reduction means would put the point between
//     them. So an offset of a point measured more than once from the setup is
//     kept in the point's metadata and not applied, with a warning, and so is
//     one of a point not shot from the current setup (a coordinate, a shot
//     from an earlier setup) and one that would put its point on the station.
//     The arithmetic's error is at apply_offsets.
//   * 128 (and 138) starts a setup on the point its description names:
//     [FLD]'s .fld syntax has the height only, and the files write the
//     description first, as the XML form carries one, and an empty value
//     after the height. With no point named the setup is on "resection at
//     record N". The file states no coordinates for it and the reduction
//     computes no resection, which not_carried says.
//   * 16 strings the current measurement point into the second string as
//     well, rather than making a second point at its position: one position,
//     as [FLD]'s two are. The second string is then the current string, as
//     [FLD]'s new point would make it, so a 20 after it closes that string. A
//     point ID or name the 16 gives is kept with the point, with a warning.
//     A 16 that is not read leaves the current string unknown, so a bare 20
//     after it is skipped, naming it, rather than closing the string before.
//     A 16, a 20 or an offset with one blank value past its count has a
//     trailing tab, as a fixed record may.
//   * 29 is a note on the setup (the project's before the first): [FLD] puts
//     a memo in its check-measurement model, which Katana does not have, and
//     a setup's notes are where a person reads a job. 41 is kept whole as
//     "additional text N" on the point: [FLD] appends it to the vertex's text,
//     and the point's description is already its comment.
//   * COMMENTS ("//", which [FLD] does not mention). Those before the first
//     record that makes a point or a setup are the header: a "Key : value"
//     one is kept in the project's metadata as "header: Key" (a key given
//     again, with another value, as "header: Key (record N)", with a
//     warning), any other in "header", and "Coordinate System" (with its
//     "Zone") is the system the file declares - by name, exactly as written,
//     never turned into an EPSG code here. A later comment (a resection's
//     residuals, an antenna height change) is a note on the setup it follows,
//     verbatim. A rule of dashes is decoration either way.
//   * 72 "Target height" and "Prism constant" are attributes of the point like
//     any other: the target height that applies is 05's, which the format
//     defines as such, and the prism constant has no unit in the file.
//   * 09 IS THE SCALE FACTOR. [FLD]'s entry for 15 (vertical circle
//     correction) gives its .fld syntax as "09 Vertical_circle_in_decimal_
//     degrees", 09 being entry 9's own opcode: every other entry's record
//     opens with the entry's number, so that line is read as a slip for 15,
//     and every 09 as a scale factor. The reader multiplies the slope
//     distances after it by the factor, so a setup whose every distance it
//     scales says so (InstrumentSettings: the factor, Applied).
//   * An opcode [FLD] defines only for its XML form (140, 145 ...) has no .fld
//     layout to read and is skipped, saying so. 124 and 125 are read because
//     the files show their layout, and one misread can only misname an
//     attribute, never move a point.
//   * 100: angle and distance units other than decimal degrees and metres are
//     refused rather than guessed at; the pressure and temperature units are
//     not read, since nothing here uses them (the files write "millibars").
//
// Text. The file is decoded once (decode_text: UTF-8, else UTF-16 by its mark
// or its NULs, else Windows-1252, said in a warning), so every value read from
// it is a view into validated UTF-8. A message that quotes the first bytes of
// a value cuts it as the C++ does, by bytes (`prefix`); a character cut in two
// is shown as U+FFFD, where the C++ message holds the half character's bytes.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use katana_core::text::{
    equals_ignoring_case, format_exact_real, is_ascii_space, lowered, parse_integer, split_lines,
    trimmed as trimmed_bytes,
};
use katana_core::text_encoding::{decode_text, to_string as encoding_name};
use katana_core::{Error, ErrorCode, Result};
use katana_math::numerics::PI;
use katana_survey::data_model::distance_sigma;
use katana_survey::{
    AngularUnit, CoordinateSource, CorrectionState, DeclaredCoordinateSystem, DistanceKind,
    DistanceObservation, Face, GnssPositionObservation, HorizontalDirectionObservation,
    InstrumentSettings, LinearUnit, Observation, Pointing, SourceRecord, ZenithAngleObservation,
};

use crate::detect::{PROBE_BYTES, looks_like_text, probe_lines, without_byte_order_mark};
use crate::format::{
    FormatContent, FormatDescriptor, FormatRegistration, FormatSignature, Manufacturer, ProbeInput,
    ReadOptions, ReadResult, describe, ruled_out,
};
use crate::topcon_raw_builder::{
    RawProjectBuilder, ShotReading, face_of_zenith_reading, offset_shot, parse_real,
    wrap_to_circle, zenith_in_model_range,
};

const FORMAT_ID: &str = "opcode-field-file";
// Where a refusal points a person: [FLD]'s entries, one per opcode.
const SPECIFICATION: &str = "Field File Format 44.8";
const RADIANS_PER_DEGREE: f64 = PI / 180.0;
// How many attribute groups an attribute's name nests, and how long the
// groups' part of it grows: the bounds Katana gives its other '/'-separated
// tree name, a layer path (MAXIMUM_LAYER_DEPTH and MAXIMUM_LAYER_NAME_LENGTH in
// katana_entity's layer_path, which surveyio may not see). Past them a group is
// read but not named, so names - and memory - grow no faster than the file.
const GROUP_DEPTH: usize = 16;
const GROUP_PATH_LENGTH: usize = 512;
// The attribute that says an 02 is a receiver's solution (see the top of this
// file): what the RTK job writes under each coordinate.
const GNSS_SOLUTION: &str = "GNSS Solution";

// core's trimmed, on text: the same ASCII blanks, so a view stays text.
fn trimmed(text: &str) -> &str {
    text.trim_matches(|c: char| u8::try_from(c).is_ok_and(is_ascii_space))
}

fn blank(text: &str) -> bool {
    trimmed(text).is_empty()
}

fn blank_bytes(text: &[u8]) -> bool {
    trimmed_bytes(text).is_empty()
}

// The first `bytes` bytes of `text`, for a message: the C++ cuts by bytes.
fn prefix(text: &str, bytes: usize) -> String {
    String::from_utf8_lossy(&text.as_bytes()[..text.len().min(bytes)]).into_owned()
}

// core's split_lines, on text: CR, LF and CRLF end a line, and a line of
// UTF-8 cut at those ASCII bytes is UTF-8.
fn lines_of(text: &str) -> Vec<&str> {
    split_lines(text.as_bytes())
        .into_iter()
        .map(|line| {
            let start = line.as_ptr() as usize - text.as_ptr() as usize;
            &text[start..start + line.len()]
        })
        .collect()
}

// The tab-separated fields of one line, the opcode first.
fn split_tabs(line: &str) -> Vec<&str> {
    line.split('\t').collect()
}

// A line from its first character that is not a blank or a tab: the opcode
// may have either before it (" 2"), and so may a comment.
fn without_indent(line: &str) -> &str {
    line.trim_start_matches([' ', '\t'])
}

// An opcode: an optional minus and one to three digits ("02", "100", "-2").
// The caller trims: " 2" is opcode 2.
fn opcode_of(text: &[u8]) -> Option<i32> {
    let negative = text.first() == Some(&b'-');
    let digits = if negative { &text[1..] } else { text };
    if digits.is_empty() || digits.len() > 3 {
        return None;
    }
    let mut value = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return None;
        }
        value = value * 10 + i32::from(c - b'0');
    }
    Some(if negative { -value } else { value })
}

// The opcode a first field that is not one would be, read loosely - a plus,
// or zeros before three digits ("+7", "0007") - for the bookkeeping of a line
// that is no record; None when it is no number at all.
fn loose_opcode(text: &str) -> Option<i32> {
    let mut text = text.as_bytes();
    let negative = text.first() == Some(&b'-');
    if negative || text.first() == Some(&b'+') {
        text = &text[1..];
    }
    while text.len() > 1 && text[0] == b'0' {
        text = &text[1..];
    }
    if !text.first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let value = opcode_of(text)?;
    Some(if negative { -value } else { value })
}

// "07", "42", "-2": an opcode as [FLD]'s syntax lines write it, whatever
// padding the file gave it.
fn opcode_text(opcode: i32) -> String {
    if (0..10).contains(&opcode) {
        format!("0{opcode}")
    } else {
        format!("{opcode}")
    }
}

fn is_letter(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

// Whether `text` reads as a date, a time or both, the way a controller writes
// one: "03/04/26 10:49:06.52", [FLD] 44.6's time_text "2015-09-28T06:42:45Z",
// "10:49:06", "03/04/2026", "3 Apr 2026 12:49 PM", "2026-04-03T10:49:06+1000",
// the basic ISO form "20260403T104906Z", a zone after the time ("10:49:06
// AEST", "UTC+10"). A time is two numbers joined by ':'; a date three joined
// by one of / - . , or a month's name standing apart from any digit and a
// number, or eight digits, T and four or six more. The words may only be a
// date's or a time's: T, Z, AM, PM, UTC, GMT, a month, and after the time a
// zone's two to five capitals. So a number alone ("12", "1.5", "1-2") and a
// feature code ("KB", "MAR1", "T1-2") are not one. Nothing is read out of it
// (see the top of this file).
#[allow(
    clippy::needless_range_loop,
    reason = "The token index is the point: each test looks at the tokens around it."
)]
fn is_date_or_time(text: &str) -> bool {
    const WORDS: [&[u8]; 6] = [b"t", b"z", b"am", b"pm", b"utc", b"gmt"];
    const MONTHS: [&[u8]; 24] = [
        b"jan",
        b"feb",
        b"mar",
        b"apr",
        b"may",
        b"jun",
        b"jul",
        b"aug",
        b"sep",
        b"sept",
        b"oct",
        b"nov",
        b"dec",
        b"january",
        b"february",
        b"march",
        b"april",
        b"june",
        b"july",
        b"august",
        b"september",
        b"october",
        b"november",
        b"december",
    ];
    const SEPARATORS: &[u8] = b"/-:.,+ ";
    // Numbers ('n'), words ('w') and the separators, each its own character.
    struct Token<'a> {
        kind: u8,
        text: &'a [u8],
    }
    let text = trimmed(text).as_bytes();
    let mut tokens: Vec<Token<'_>> = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let mut end = i + 1;
        let mut kind = text[i];
        if text[i].is_ascii_digit() {
            while end < text.len() && text[end].is_ascii_digit() {
                end += 1;
            }
            kind = b'n';
        } else if is_letter(text[i]) {
            while end < text.len() && is_letter(text[end]) {
                end += 1;
            }
            kind = b'w';
        } else if !SEPARATORS.contains(&text[i]) {
            return false;
        }
        tokens.push(Token {
            kind,
            text: &text[i..end],
        });
        i = end;
    }
    // Out of range reads as nothing, so i - 1 at the start is no token.
    let is = |i: usize, kind: u8| i < tokens.len() && tokens[i].kind == kind;
    let mut time = false;
    let mut date = false;
    let mut digits = false;
    let mut time_at = tokens.len();
    for i in 0..tokens.len() {
        digits = digits || is(i, b'n');
        if is(i, b'n') && is(i + 1, b':') && is(i + 2, b'n') {
            time_at = time_at.min(i);
            time = true;
        }
        for separator in *b"/-." {
            if is(i, b'n')
                && is(i + 1, separator)
                && is(i + 2, b'n')
                && is(i + 3, separator)
                && is(i + 4, b'n')
            {
                date = true;
            }
        }
        if is(i, b'n')
            && tokens[i].text.len() == 8
            && is(i + 1, b'w')
            && equals_ignoring_case(tokens[i + 1].text, b"t")
            && is(i + 2, b'n')
            && (tokens[i + 2].text.len() == 4 || tokens[i + 2].text.len() == 6)
        {
            date = true;
            time = true;
            time_at = time_at.min(i);
        }
    }
    let mut month = false;
    for i in 0..tokens.len() {
        if tokens[i].kind != b'w' {
            continue;
        }
        let word = lowered(tokens[i].text);
        let apart = !is(i.wrapping_sub(1), b'n') && !is(i + 1, b'n');
        if apart && MONTHS.contains(&word.as_slice()) {
            month = true;
        } else if WORDS.contains(&word.as_slice()) {
            continue;
        } else if time
            && i > time_at
            && apart
            && (2..=5).contains(&tokens[i].text.len())
            && tokens[i].text.iter().all(u8::is_ascii_uppercase)
        {
            continue; // a zone's abbreviation after the time
        } else {
            return false;
        }
    }
    time || date || (month && digits)
}

// A comment that is only a rule ("//------") or nothing.
fn is_decoration(text: &str) -> bool {
    text.bytes().all(|c| b"-=_*~ \t".contains(&c))
}

// The values after a point description that [FLD] gives each opcode with one;
// None for an opcode without a description.
fn values_after_description(opcode: i32) -> Option<usize> {
    match opcode {
        2 => Some(3), // X Y Z
        3 => Some(1), // instrument height
        4 => Some(3), // HA VA SD, and the azimuth count_fits allows
        6 => Some(3), // HA VA SD
        7 => Some(3), // HA VA SD
        _ => None,
    }
}

// What the first value after a fixed record's description is, for a message.
fn first_value_name(opcode: i32) -> &'static str {
    match opcode {
        2 => "X",
        3 => "the instrument height",
        _ => "the horizontal circle",
    }
}

// 1 (opcode) + `columns` + 5 (description) + the values: whether `count`
// fields are the fixed record [FLD] gives, with the column after the opcode
// (`columns` 1) or without it. 04's azimuth may be left off.
fn count_fits(opcode: i32, count: usize, columns: usize) -> bool {
    let Some(arity) = values_after_description(opcode) else {
        return false;
    };
    let plain = 1 + columns + 5 + arity;
    count == plain || (opcode == 4 && count == plain + 1)
}

// The layouts a fixed record's fields fit: with the column after the opcode,
// and without it. One blank field past a layout's count is a trailing tab.
#[derive(Clone, Copy, Debug, Default)]
struct LayoutFit {
    column: bool,
    plain: bool,
}

fn layout_fit<T: AsRef<[u8]>>(opcode: i32, fields: &[T]) -> LayoutFit {
    let count = fields.len();
    let trailing = count > 1
        && fields
            .last()
            .is_some_and(|field| blank_bytes(field.as_ref()));
    let fits = |columns: usize| {
        count_fits(opcode, count, columns) || (trailing && count_fits(opcode, count - 1, columns))
    };
    LayoutFit {
        column: fits(1),
        plain: fits(0),
    }
}

// The first value after a fixed record's description, read with the column
// (`columns` 1) or without it; empty when the record is too short for it.
fn first_value<'a>(fields: &[&'a str], columns: usize) -> &'a str {
    let at = 1 + columns + 5;
    if at < fields.len() { fields[at] } else { "" }
}

// For a record of free fields whose values nonetheless have a fixed form - a
// number, a list of units, the offsets' and 128's optional description, 16's
// and 20's - whether `values` fit that form; None for text, whose first value
// may be anything, blank included.
fn fixed_form_fits(opcode: i32, values: &[&str]) -> Option<bool> {
    let count = values.len();
    let first_filled = count > 0 && !blank(values[0]);
    match opcode {
        5 | 9 => Some(count == 1 && first_filled),
        100 => Some(count >= 2 && first_filled),
        42..=44 => Some(count == 1 || count == 6),
        128 | 138 => Some(count == 1 || count == 6 || (count == 7 && blank(values[6]))),
        16 => Some(count == 5),
        20 => Some(count == 5 || values.iter().all(|value| blank(value))),
        _ => None,
    }
}

// How many of `values` a record of free fields gives, one blank value past
// one of `counts` - its form's - being a trailing tab, as a fixed record's is
// (layout_fit).
fn given_values(values: &[&str], counts: &[usize]) -> usize {
    let count = values.len();
    if count > 0 && values.last().is_some_and(|value| blank(value)) && counts.contains(&(count - 1))
    {
        return count - 1;
    }
    count
}

// Opcodes that make a point or a setup, read or not: the header comments end
// at the first of them, and so does an attribute group left open.
fn makes_point_or_setup(opcode: i32) -> bool {
    matches!(
        opcode,
        2 | 3 | 4 | 6 | 7 | 10 | 11 | 12 | 14 | 128 | 138 | 140
    )
}

fn starts_setup(opcode: i32) -> bool {
    opcode == 3 || opcode == 128 || opcode == 138
}

// [FLD] 44.4's measurement points: what 16, 20 and the offsets act on.
fn makes_measurement(opcode: i32) -> bool {
    matches!(opcode, 2 | 7 | 10 | 11 | 12 | 140)
}

// [FLD] 44.8's name for each opcode this reader does not read, so a skipped
// record says what it was.
#[derive(Clone, Copy, Debug)]
struct UnreadOpcode {
    opcode: i32,
    name: &'static str,
    // "This opcode does not exist in the fld file": defined for the XML form.
    xml_only: bool,
    // A record about a point of its own - a measurement [FLD] 44.4 counts as
    // current, or a check to a named coordinate: what follows it describes
    // that point, so nothing after it may be given to the point before.
    about_a_point: bool,
    // A correction to the measurements after it.
    corrects_later: bool,
    // A field template, from which the measurements after it may take their
    // feature codes and string numbers.
    codes_later: bool,
    // Ends the current string, so the points after it of that code and string
    // number start another.
    ends_string: bool,
}

const fn unread_opcode_named(opcode: i32, name: &'static str) -> UnreadOpcode {
    UnreadOpcode {
        opcode,
        name,
        xml_only: false,
        about_a_point: false,
        corrects_later: false,
        codes_later: false,
        ends_string: false,
    }
}

const fn about_a_point(opcode: UnreadOpcode) -> UnreadOpcode {
    UnreadOpcode {
        about_a_point: true,
        ..opcode
    }
}

const fn xml_only(opcode: UnreadOpcode) -> UnreadOpcode {
    UnreadOpcode {
        xml_only: true,
        ..opcode
    }
}

const fn corrects_later(opcode: UnreadOpcode) -> UnreadOpcode {
    UnreadOpcode {
        corrects_later: true,
        ..opcode
    }
}

const fn codes_later(opcode: UnreadOpcode) -> UnreadOpcode {
    UnreadOpcode {
        codes_later: true,
        ..opcode
    }
}

const fn ends_string(opcode: UnreadOpcode) -> UnreadOpcode {
    UnreadOpcode {
        ends_string: true,
        ..opcode
    }
}

const UNREAD_OPCODES: &[UnreadOpcode] = &[
    unread_opcode_named(-3, "group"),
    unread_opcode_named(-1, "error text"),
    unread_opcode_named(1, "job data"),
    about_a_point(unread_opcode_named(10, "stadia measurement")),
    about_a_point(unread_opcode_named(
        11,
        "measurement by horizontal distance and height",
    )),
    about_a_point(unread_opcode_named(
        12,
        "measurement by horizontal distance and height difference",
    )),
    about_a_point(unread_opcode_named(14, "check coordinate")),
    corrects_later(unread_opcode_named(15, "vertical circle correction")),
    unread_opcode_named(17, "arc through the last three points"),
    unread_opcode_named(18, "circle feature"),
    unread_opcode_named(19, "reverse string"),
    unread_opcode_named(21, "join strings, last to last"),
    unread_opcode_named(22, "join strings, first to last"),
    unread_opcode_named(23, "join strings, first to first"),
    unread_opcode_named(24, "join strings, last to first"),
    unread_opcode_named(28, "height or depth text"),
    unread_opcode_named(30, "remove height"),
    unread_opcode_named(31, "remove point"),
    unread_opcode_named(37, "rectangle"),
    unread_opcode_named(38, "non-tinable next segment"),
    unread_opcode_named(39, "non-tinable previous segment"),
    unread_opcode_named(40, "non-tinable vertex"),
    unread_opcode_named(45, "parallelogram"),
    unread_opcode_named(46, "breakline"),
    ends_string(unread_opcode_named(47, "new string")),
    ends_string(unread_opcode_named(48, "end string")),
    unread_opcode_named(49, "distances"),
    // "used as the bearing datum difference for the current instrument set up
    // for all measurements ... that follow"
    corrects_later(unread_opcode_named(50, "backsight bearing")),
    // 51, 53, 54 and 56 to 59: "The next measurement takes the feature code and
    // string number from the next point of the field template". 52 ends a
    // template and 55 records one from the measurements' own codes.
    codes_later(unread_opcode_named(51, "template start")),
    unread_opcode_named(52, "template end"),
    codes_later(unread_opcode_named(53, "template pause")),
    codes_later(unread_opcode_named(54, "template continue")),
    unread_opcode_named(55, "template record"),
    codes_later(unread_opcode_named(56, "template skip")),
    codes_later(unread_opcode_named(57, "template delete")),
    codes_later(unread_opcode_named(58, "template insert")),
    codes_later(unread_opcode_named(59, "template change")),
    unread_opcode_named(60, "arc through the next three points"),
    unread_opcode_named(61, "arc start"),
    unread_opcode_named(62, "arc end"),
    unread_opcode_named(68, "integer attribute of the string"),
    unread_opcode_named(69, "real attribute of the string"),
    unread_opcode_named(70, "text attribute of the string"),
    unread_opcode_named(74, "integer attribute of the next segment"),
    unread_opcode_named(75, "real attribute of the next segment"),
    unread_opcode_named(76, "text attribute of the next segment"),
    unread_opcode_named(77, "integer attribute of the previous segment"),
    unread_opcode_named(78, "real attribute of the previous segment"),
    unread_opcode_named(79, "text attribute of the previous segment"),
    unread_opcode_named(80, "pipe justification, invert"),
    unread_opcode_named(81, "pipe justification, axial"),
    unread_opcode_named(82, "pipe justification, obvert"),
    unread_opcode_named(83, "shape record"),
    unread_opcode_named(84, "shape end"),
    unread_opcode_named(85, "shape parallel"),
    unread_opcode_named(86, "shape extrude"),
    unread_opcode_named(87, "shape parallel start"),
    unread_opcode_named(88, "shape extrude start"),
    unread_opcode_named(92, "2d string"),
    unread_opcode_named(93, "3d string"),
    unread_opcode_named(94, "4d string"),
    unread_opcode_named(95, "pipe diameter"),
    unread_opcode_named(96, "culvert"),
    unread_opcode_named(101, "automatic string order"),
    unread_opcode_named(107, "invisible previous segment"),
    unread_opcode_named(108, "invisible next segment"),
    unread_opcode_named(109, "invisible vertex"),
    unread_opcode_named(110, "building face"),
    unread_opcode_named(111, "building face end"),
    unread_opcode_named(112, "set collection"),
    unread_opcode_named(113, "set collection end"),
    unread_opcode_named(119, "code file"),
    xml_only(unread_opcode_named(120, "measured attribute of the string")),
    xml_only(unread_opcode_named(121, "measured attribute of the vertex")),
    xml_only(unread_opcode_named(
        122,
        "measured attribute of the next segment",
    )),
    xml_only(unread_opcode_named(
        123,
        "measured attribute of the previous segment",
    )),
    unread_opcode_named(126, "attachment"),
    corrects_later(unread_opcode_named(127, "distance correction")),
    xml_only(unread_opcode_named(130, "field file")),
    corrects_later(unread_opcode_named(131, "PPM correction")),
    about_a_point(xml_only(unread_opcode_named(140, "GNSS coordinate"))),
    xml_only(unread_opcode_named(141, "non-tinable string")),
    xml_only(unread_opcode_named(142, "extend the next segment")),
    xml_only(unread_opcode_named(143, "extend the previous segment")),
    xml_only(unread_opcode_named(144, "remove string")),
    xml_only(unread_opcode_named(145, "GNSS offset")),
    xml_only(unread_opcode_named(146, "midpoint of two points")),
    xml_only(unread_opcode_named(147, "midpoint of three points")),
    xml_only(unread_opcode_named(172, "extend the next segment in 3d")),
    xml_only(unread_opcode_named(
        173,
        "extend the previous segment in 3d",
    )),
];

fn unread_opcode(opcode: i32) -> Option<&'static UnreadOpcode> {
    UNREAD_OPCODES.iter().find(|known| known.opcode == opcode)
}

#[derive(Clone, Copy, Debug, Default)]
struct Description<'a> {
    feature_code: &'a str,
    string_number: &'a str,
    point_id: &'a str,
    point_name: &'a str,
    comment: &'a str,
}

impl<'a> Description<'a> {
    // The five values from `values[at]` on.
    fn of(values: &[&'a str], at: usize) -> Self {
        Self {
            feature_code: values[at],
            string_number: values[at + 1],
            point_id: values[at + 2],
            point_name: values[at + 3],
            comment: values[at + 4],
        }
    }

    // The point the record names: its name, else its ID.
    fn id(&self) -> &'a str {
        if self.point_name.is_empty() {
            self.point_id
        } else {
            self.point_name
        }
    }

    // "feature code KB, string number 01, comment kerb": what a record says
    // of a point that it does not string (a backsight, a check, a setup);
    // empty when it says none of that.
    fn coding(&self) -> String {
        let mut text = String::new();
        let mut add = |label: &str, value: &str| {
            if !blank(value) {
                if !text.is_empty() {
                    text.push_str(", ");
                }
                text.push_str(label);
                text.push(' ');
                text.push_str(trimmed(value));
            }
        };
        add("feature code", self.feature_code);
        add("string number", self.string_number);
        add("comment", self.comment);
        text
    }
}

// "KB 01", "PT04": a feature as a person names it.
fn feature_name(code: &str, string_number: &str) -> String {
    if string_number.is_empty() {
        String::from(code)
    } else {
        format!("{code} {string_number}")
    }
}

fn any_filled(values: &[&str], from: usize) -> bool {
    values.iter().skip(from).any(|value| !blank(value))
}

// "N 6200020, E 500010, elevation 12.5": coordinates as a restatement says them.
fn coordinates_text(northing: f64, easting: f64, elevation: Option<f64>) -> String {
    let mut text = format!(
        "N {}, E {}",
        format_exact_real(northing),
        format_exact_real(easting)
    );
    if let Some(elevation) = elevation {
        text.push_str(&format!(", elevation {}", format_exact_real(elevation)));
    }
    text
}

fn offset_kind(opcode: i32) -> &'static str {
    match opcode {
        42 => "radial",
        43 => "tangential",
        _ => "height",
    }
}

// Whether the file's records carry the column after the opcode (see the top
// of this file). Decided by a first pass over every line.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Layout {
    #[default]
    Undecided,
    Column,
    NoColumn,
}

// A record's values after its opcode and, where the record has it, the
// column after the opcode; `stamp` is that column, trimmed (empty, or a date
// or time).
#[derive(Clone, Debug, Default)]
struct Values<'a> {
    values: Vec<&'a str>,
    stamp: &'a str,
}

// One of 44.5's lists: the point names and the point IDs the directly
// entered coordinates (02), or the measurements (07), before a record gave,
// each ID with the point it made then (the first kept).
#[derive(Clone, Debug, Default)]
struct GivenPoints {
    names: HashSet<String>,
    ids: HashMap<String, String>,
}

// Every 04 read, in file order, for settle_backsights: the setup it is of (its
// index in SurveyProject::stations), its record and point, the circle it reads
// as its face-left reading, and the azimuth it states.
#[derive(Clone, Debug, Default)]
struct BacksightRecord {
    station: usize,
    record: usize,
    target: String,
    circle: Option<f64>,
    azimuth: Option<f64>,
    // As written.
    azimuth_text: String,
}

// The 02 whose record group is being read: its point is positioned, or given
// its GNSS position, when the next record that makes a point or a setup
// begins or the file ends, once its attributes have said which.
#[derive(Clone, Debug, Default)]
struct PendingCoordinate {
    id: String,
    easting: f64,
    northing: f64,
    elevation: Option<f64>,
    record: usize,
    gnss: bool,
}

// The first coordinates each point was given, for a later 02 of it.
#[derive(Clone, Copy, Debug, Default)]
struct GivenCoordinate {
    easting: f64,
    northing: f64,
    elevation: Option<f64>,
    record: usize,
    gnss: bool,
}

// What the current setup measured to each point, for the offsets.
#[derive(Clone, Debug, Default)]
struct Shot {
    // 04, 06 and 07 to the point from the setup.
    pointings: usize,
    // Of the first, when it is a 07.
    record: usize,
    // Index in SurveyProject::stations.
    station: usize,
    // Indices in the setup's observations.
    direction: Option<usize>,
    zenith: Option<usize>,
    distance: Option<usize>,
    // Radians and metres, as read and scaled.
    measured_reading: ShotReading,
    // The values as the file writes them.
    measured: String,
    // The offsets applied, metres.
    radial: f64,
    tangential: f64,
    height: f64,
}

#[derive(Clone, Debug, Default)]
struct PendingOffset {
    opcode: i32,
    target: String,
    amount: f64,
    record: usize,
}

struct FieldFileReader {
    builder: RawProjectBuilder,
    fatal: Option<Error>,
    layout: Layout,
    target_height: f64,
    target_height_seen: bool,
    distance_scale: f64,
    scale_warned: bool,
    // The factor the current setup's first slope distance was multiplied by,
    // and the first record of it whose factor was another (0 when none).
    setup_factor: Option<f64>,
    mixed_factor_record: usize,
    // Comments before the first record that makes a point or a setup are the
    // file's header.
    header: bool,
    header_text: String,
    // No record has been met yet: a "{Version ...}" line is the file's own.
    first_record: bool,

    // The point the last 02, 04, 06 or 07 was about: 41 and 71 to 73 describe
    // it. Empty after such a record that was not read, which is then
    // `unread_point_record`.
    current_point: String,
    unread_point_record: usize,
    // "record N/" on what the records after a point record give its point,
    // unless that record was the first about it; and what they gave, by key.
    attribute_prefix: String,
    record_values: HashMap<String, String>,
    // Every point a record has made current, and the point ID first given
    // with each point name.
    described_points: HashSet<String>,
    point_ids: HashMap<String, String>,
    // The names and point IDs the coordinates (02) and the shots (07) gave:
    // 44.5's two lists, searched in that order.
    coordinates_given: GivenPoints,
    shots_given: GivenPoints,
    // [FLD] 44.4's current measurement point - the point of the last 02 or 07
    // - with the record, the feature code and the string number that made
    // it: 16, 20 and the offsets act on it and its string.
    current_measurement: String,
    measurement_record: usize,
    // Made by a 07, not an 02.
    measurement_is_shot: bool,
    current_code: String,
    current_string: String,
    unread_measurement_record: usize,
    // A 16 that was not read: [FLD]'s 16 makes its own string the current
    // string, so which string is current is not known until a measurement or
    // a 16 is read. 0 when it is known.
    unread_string_record: usize,
    backsights: Vec<BacksightRecord>,
    // The first setup (its index) that measured each point a direction and a
    // distance - a 04, 06 or 07 the reduction radiates - so that a later
    // setup's backsight to it has a position.
    first_measured_from: HashMap<String, usize>,
    coordinate: Option<PendingCoordinate>,
    given_coordinates: HashMap<String, GivenCoordinate>,
    // Open attribute groups, outermost first, with the records that opened
    // them, whether each is named in its attributes' names, and the length of
    // group_prefix before it.
    groups: Vec<String>,
    group_records: Vec<usize>,
    group_named: Vec<bool>,
    group_prefix_lengths: Vec<usize>,
    // "Outer/Inner/": the names of the open groups that are named.
    group_prefix: String,
    named_groups: usize,
    // A setup record (03, 128, 138) that was not read: the measurements after
    // it are not filed under the setup before it.
    unread_setup_record: usize,
    // The resection (128 or 138) whose block is open, 0 when none.
    open_resection: usize,
    open_resection_opcode: i32,
    resections: usize,
    // A date and time on a record that makes no point or setup: said once.
    unkept_stamp_warned: bool,
    coordinate_system: String,
    zone: String,
    shots: HashMap<String, Shot>,
    pending_offsets: Vec<PendingOffset>,
}

impl FieldFileReader {
    fn new(file_name: &str, options: &ReadOptions) -> Self {
        let source = SourceRecord {
            manufacturer: String::from("Other"),
            format: String::from("Opcode field file"),
            format_version: String::from("fld"),
            file_name: String::from(file_name),
            record_number: 0,
        };
        Self {
            builder: RawProjectBuilder::new(file_name, source, options),
            fatal: None,
            layout: Layout::Undecided,
            target_height: 0.0,
            target_height_seen: false,
            distance_scale: 1.0,
            scale_warned: false,
            setup_factor: None,
            mixed_factor_record: 0,
            header: true,
            header_text: String::new(),
            first_record: true,
            current_point: String::new(),
            unread_point_record: 0,
            attribute_prefix: String::new(),
            record_values: HashMap::new(),
            described_points: HashSet::new(),
            point_ids: HashMap::new(),
            coordinates_given: GivenPoints::default(),
            shots_given: GivenPoints::default(),
            current_measurement: String::new(),
            measurement_record: 0,
            measurement_is_shot: false,
            current_code: String::new(),
            current_string: String::new(),
            unread_measurement_record: 0,
            unread_string_record: 0,
            backsights: Vec::new(),
            first_measured_from: HashMap::new(),
            coordinate: None,
            given_coordinates: HashMap::new(),
            groups: Vec::new(),
            group_records: Vec::new(),
            group_named: Vec::new(),
            group_prefix_lengths: Vec::new(),
            group_prefix: String::new(),
            named_groups: 0,
            unread_setup_record: 0,
            open_resection: 0,
            open_resection_opcode: 0,
            resections: 0,
            unkept_stamp_warned: false,
            coordinate_system: String::new(),
            zone: String::new(),
            shots: HashMap::new(),
            pending_offsets: Vec::new(),
        }
    }

    fn number(&mut self, text: &str, what: &str, n: usize) -> Option<f64> {
        if blank(text) {
            return None;
        }
        let value = parse_real(text);
        if value.is_none() {
            self.builder.warn(
                n,
                format!("{what} '{}' is not a number", prefix(trimmed(text), 40)),
            );
        }
        value
    }

    fn values_of<'a>(&self, opcode: i32, fields: &[&'a str]) -> Values<'a> {
        let mut result = Values::default();
        if fields.len() < 2 {
            return result; // the opcode alone
        }
        let first = fields[1];
        let mut column = false;
        if is_date_or_time(first) {
            column = true; // only the column holds a date or time
        } else if self.layout == Layout::Column {
            column = blank(first);
        } else if blank(first) && fields.len() > 2 {
            // Without the column (or undecided, read as [FLD] writes it): a
            // record of fixed form that fits only with its blank first value
            // taken as the column carries it - an empty target height, factor
            // or unit followed by one is no value.
            let whole = fixed_form_fits(opcode, &fields[1..]);
            column = whole == Some(false) && fixed_form_fits(opcode, &fields[2..]).unwrap_or(false);
        }
        if column {
            result.stamp = trimmed(first);
            result.values = fields[2..].to_vec();
        } else {
            result.values = fields[1..].to_vec();
        }
        result
    }

    fn comment(&mut self, line: &str, n: usize) {
        let text = trimmed(&line[2..]);
        if is_decoration(text) {
            return;
        }
        if !self.header {
            // A resection's residuals, an antenna height changed: kept, as
            // written, with the setup they follow.
            self.builder.add_station_note(text);
            return;
        }
        if let Some(colon) = text.find(':') {
            let key = trimmed(&text[..colon]);
            let value = trimmed(&text[colon + 1..]);
            if !key.is_empty() && key.len() <= 40 {
                if value.is_empty() {
                    return; // a heading the writer left blank ("Surveyor : ")
                }
                let name = format!("header: {key}");
                let given = self.builder.project().metadata.get(&name).cloned();
                match given {
                    None => {
                        self.builder
                            .project()
                            .metadata
                            .insert(name, String::from(value));
                    }
                    Some(given) => {
                        if given != value {
                            let again = format!("{name} (record {n})");
                            self.builder.warn(
                                n,
                                format!(
                                    "the header gives '{key}' again, as '{}' after '{}'; both are \
                                     kept, this one as '{again}'",
                                    prefix(value, 40),
                                    prefix(&given, 40)
                                ),
                            );
                            self.builder
                                .project()
                                .metadata
                                .insert(again, String::from(value));
                        }
                        return;
                    }
                }
                if key == "Coordinate System" {
                    self.coordinate_system = String::from(value);
                } else if key == "Zone" {
                    self.zone = String::from(value);
                }
                return;
            }
        }
        if !self.header_text.is_empty() {
            self.header_text.push('\n');
        }
        self.header_text.push_str(text);
    }

    fn units(&mut self, values: &[&str], n: usize) {
        let angle = values.first().map_or("", |value| trimmed(value));
        let distance = if values.len() < 2 {
            ""
        } else {
            trimmed(values[1])
        };
        if angle != "degrees" || distance != "metres" {
            self.fatal = Some(
                Error::new(
                    ErrorCode::FileImportFailure,
                    format!(
                        "record {n} of {} gives the units as angle '{angle}', distance \
                         '{distance}'; the format defines only degrees (decimal) and metres, and \
                         Katana will not guess what another unit means",
                        self.builder.file_name()
                    ),
                )
                .with_context(format!("{SPECIFICATION}, opcode 100")),
            );
            return;
        }
        self.builder.count_read();
    }

    fn scale_factor(&mut self, factor: f64, n: usize) {
        if factor != 1.0 && !self.scale_warned {
            self.scale_warned = true;
            self.builder.warn(
                n,
                format!(
                    "slope distances after this record are multiplied by the file's scale factor \
                     {}, as the format says to",
                    format_exact_real(factor)
                ),
            );
        }
        self.distance_scale = factor;
        self.builder.count_read();
    }

    // The setup that ends where another begins, or the file ends: its
    // offsets, and the scale factor its settings state.
    fn end_setup(&mut self) {
        self.apply_offsets();
        // The factor every slope distance of the setup was multiplied by is
        // what its settings say, Applied (see begin_setup); one that changed
        // within the setup leaves them saying none.
        let mut changed_within = None;
        if let Some(factor) = self.setup_factor
            && let Some(setup) = self.builder.current_station()
        {
            let settings = &mut setup.instrument;
            if self.mixed_factor_record != 0 {
                settings.scale_factor = None;
                settings.scale_factor_state = CorrectionState::Unknown;
                changed_within = Some(setup.setup.id.clone());
            } else if factor != 1.0 {
                settings.scale_factor = Some(factor);
                settings.scale_factor_state = CorrectionState::Applied;
            } else {
                settings.scale_factor = None;
                settings.scale_factor_state = CorrectionState::Unknown;
            }
        }
        if let Some(id) = changed_within {
            self.builder.warn(
                self.mixed_factor_record,
                format!(
                    "the scale factor changed within setup {id}: its slope distances from this \
                     record on are multiplied by another factor than those before, so its \
                     instrument settings state none"
                ),
            );
        }
        self.setup_factor = None;
        self.mixed_factor_record = 0;
    }

    // ---- The bookkeeping of the current point (see the fields above)

    fn point_record_starts(&mut self, n: usize, is_measurement: bool) {
        self.finish_coordinate();
        self.current_point.clear();
        self.unread_point_record = n;
        self.attribute_prefix.clear();
        self.record_values.clear();
        if is_measurement {
            self.current_measurement.clear();
            self.measurement_record = 0;
            self.measurement_is_shot = false;
            self.current_code.clear();
            self.current_string.clear();
            self.unread_measurement_record = n;
            self.unread_string_record = 0;
        }
    }

    fn point_record_read(&mut self, id: &str, d: &Description<'_>, n: usize, is_measurement: bool) {
        self.current_point = String::from(id);
        self.unread_point_record = 0;
        // The first record about a point owns its attributes' plain names; a
        // later one's go under its record (see the top of this file).
        self.attribute_prefix = if self.described_points.insert(String::from(id)) {
            String::new()
        } else {
            format!("record {n}/")
        };
        self.record_values.clear();
        if is_measurement {
            self.current_measurement = String::from(id);
            self.measurement_record = n;
            self.current_code = String::from(d.feature_code);
            self.current_string = String::from(d.string_number);
            self.unread_measurement_record = 0;
            self.unread_string_record = 0;
        }
    }

    // Whether a record about the current point has one; skips it otherwise.
    fn has_current_point(&mut self, what: &str, n: usize) -> bool {
        if !self.current_point.is_empty() {
            return true;
        }
        let message = if self.unread_point_record != 0 {
            format!(
                "{what} of the point of record {}, which was not read",
                self.unread_point_record
            )
        } else {
            format!("{what} with no point before it to describe")
        };
        self.builder.skip(n, message);
        false
    }

    // Whether a record about the current measurement point has one.
    fn has_current_measurement(&mut self, what: &str, n: usize) -> bool {
        if !self.current_measurement.is_empty() {
            return true;
        }
        let message = if self.unread_measurement_record != 0 {
            format!(
                "{what} of the measurement of record {}, which was not read",
                self.unread_measurement_record
            )
        } else {
            format!("{what} with no measurement (opcode 02 or 07) before it")
        };
        self.builder.skip(n, message);
        false
    }

    // `key` = `value` on the current point, under the current record's
    // prefix; a key the record has given already keeps a different value as
    // "key (record n)", with a warning.
    fn put_point_value(&mut self, key: &str, value: String, n: usize) {
        let full = format!("{}{key}", self.attribute_prefix);
        match self.record_values.entry(full) {
            Entry::Vacant(slot) => {
                let full = slot.key().clone();
                slot.insert(value.clone());
                self.builder
                    .add_point_metadata(&self.current_point, full, value);
            }
            Entry::Occupied(slot) => {
                if *slot.get() == value {
                    return; // given again, the same: nothing to add
                }
                let given = slot.get().clone();
                let again = format!("{} (record {n})", slot.key());
                self.builder.warn(
                    n,
                    format!(
                        "point '{}' is given '{key}' again, as '{}' after '{}'; both are kept, \
                         this one as '{again}'",
                        self.current_point,
                        prefix(&value, 40),
                        prefix(&given, 40)
                    ),
                );
                self.builder
                    .add_point_metadata(&self.current_point, again, value);
            }
        }
    }

    // What a record about `point` says besides its code: the point ID it
    // gives with the point's name - or, where 44.5 found the point by another
    // name or by its ID, the name and ID it gives - and a later record's
    // comment.
    fn keep_point_id(&mut self, d: &Description<'_>, point: &str, n: usize) {
        if !blank(d.point_name) && d.point_name != point {
            // 44.5 found the point by another name or by its ID: what this
            // record calls it is kept with the point, under the record, rather
            // than made a point of its own. The ID, unless it is one a
            // coordinate or a shot gave the point.
            self.put_point_value("point name", String::from(trimmed(d.point_name)), n);
            if !blank(d.point_id) && !self.gave_id(d.point_id, point) {
                self.put_point_value("point ID", String::from(trimmed(d.point_id)), n);
            }
            return;
        }
        if blank(d.point_name) || blank(d.point_id) {
            return;
        }
        // Named by its name, the point keeps its ID too: the first one given
        // it, and a different one a later record gives under that record.
        let name = String::from(d.point_name);
        match self.point_ids.entry(name.clone()) {
            Entry::Vacant(slot) => {
                let known = slot.insert(String::from(d.point_id)).clone();
                self.builder
                    .add_point_metadata(&name, String::from("point ID"), known);
            }
            Entry::Occupied(slot) => {
                if slot.get() != d.point_id {
                    self.builder.add_point_metadata(
                        &name,
                        format!("record {n}/point ID"),
                        String::from(d.point_id),
                    );
                }
            }
        }
    }

    fn keep_later_comment(&mut self, d: &Description<'_>, n: usize) {
        // The first record's comment is the point's description (code_point);
        // a later record's is its own, kept under it.
        if !self.attribute_prefix.is_empty() && !blank(d.comment) {
            self.put_point_value("comment", String::from(trimmed(d.comment)), n);
        }
    }

    // [FLD] 44.5: the point a setup, a backsight or a check names.
    fn known_point(&self, d: &Description<'_>) -> String {
        // 44.5, steps 4 to 9: the coordinates, then the shots; in each the
        // name among their names, the name among their IDs, the ID among their
        // IDs. A named point is its name, so the name found is the point.
        let named = !blank(d.point_name);
        let numbered = !blank(d.point_id);
        for given in [&self.coordinates_given, &self.shots_given] {
            if named {
                if given.names.contains(d.point_name) {
                    return String::from(d.point_name);
                }
                if let Some(owner) = given.ids.get(d.point_name) {
                    return owner.clone();
                }
            }
            if numbered && let Some(owner) = given.ids.get(d.point_id) {
                return owner.clone();
            }
        }
        // Step 10 asks the person; the file's own name for it stands instead -
        // a point no record gave coordinates, or one only a setup or a
        // backsight named before.
        String::from(d.id())
    }

    // Warns of a setup, backsight or check (`what`, which `acts` on the point
    // found) whose point name is a point other than the one 44.5 `found`.
    fn warn_of_another_named_point(
        &mut self,
        d: &Description<'_>,
        found: &str,
        what: &str,
        acts: &str,
        n: usize,
    ) {
        // 44.5 found the point by an ID, or by the name among the IDs, before
        // it reached the point of that name - or the point of that name is one
        // only a setup, a backsight or a check made, which its lists do not
        // hold.
        let name = d.point_name;
        if blank(name) || name == found || !self.builder.has_point(name) {
            return;
        }
        let with_id = if blank(d.point_id) {
            String::new()
        } else {
            format!(" with point ID {}", trimmed(d.point_id))
        };
        self.builder.warn(
            n,
            format!(
                "the {what} names point '{}'{with_id}, and the format's search for its point \
                 (44.5) finds point '{found}' first: the {what} {acts} '{found}', not the point \
                 of that name",
                trimmed(name)
            ),
        );
    }

    // [FLD] 44.8, 20 and 42 to 44: the points a description names by its
    // point ID - a shot's, then a coordinate's - else (the reader's own
    // reading) the point of its name; empty when no record before made one.
    fn described_points_of(&self, d: &Description<'_>) -> Vec<String> {
        let mut points: Vec<String> = Vec::new();
        // "the point with that point ID": the ID a measurement was given, so
        // the shot's before the coordinate's where both were given it.
        if !blank(d.point_id) {
            for given in [&self.shots_given, &self.coordinates_given] {
                if let Some(owner) = given.ids.get(d.point_id)
                    && !points.contains(owner)
                {
                    points.push(owner.clone());
                }
            }
        }
        if !points.is_empty() {
            return points;
        }
        // Not what [FLD] says, which names only the point ID here: a point of
        // the description's name (else of its ID, as a point that has no name
        // is known) that a record before made.
        let named = d.id();
        if !blank(named) && self.builder.has_point(named) {
            points.push(String::from(named));
        }
        points
    }

    // Warns of a 20 or an offset (`what`) whose point ID a shot and a
    // coordinate both gave (`points`, as described_points_of has them), saying
    // which it `took` and why.
    fn warn_of_shared_id(
        &mut self,
        d: &Description<'_>,
        points: &[String],
        what: &str,
        took: &str,
        why: &str,
        n: usize,
    ) {
        // Two points only when a shot and a coordinate were both given the ID:
        // [FLD] calls a point ID "normally unique", and says nothing of which.
        if points.len() < 2 {
            return;
        }
        self.builder.warn(
            n,
            format!(
                "{what} names point ID {}, which a shot gave point '{}' and a coordinate point \
                 '{}': it is taken for point '{took}', {why}",
                trimmed(d.point_id),
                points[0],
                points[1]
            ),
        );
    }

    // Whether a coordinate or a shot gave `point` the point ID `point_id`.
    fn gave_id(&self, point_id: &str, point: &str) -> bool {
        [&self.coordinates_given, &self.shots_given]
            .into_iter()
            .any(|given| given.ids.get(point_id).is_some_and(|owner| owner == point))
    }

    // The point name and the point ID a coordinate or a shot gives, with the
    // point it made.
    fn register_point(given: &mut GivenPoints, d: &Description<'_>, id: &str) {
        if !blank(d.point_name) {
            given.names.insert(String::from(id)); // a named point is its name
        }
        if !blank(d.point_id) {
            given
                .ids
                .entry(String::from(d.point_id))
                .or_insert_with(|| String::from(id));
        }
    }

    // Positions the point of the 02 whose record group ends here, or gives it
    // its GNSS position (see the top of this file).
    fn finish_coordinate(&mut self) {
        let Some(c) = self.coordinate.take() else {
            return;
        };
        if c.gnss && c.elevation.is_none() {
            self.builder.warn(
                c.record,
                format!(
                    "point '{}' is a receiver's solution (its '{GNSS_SOLUTION}' attribute) with no \
                     height, which a GNSS position has; it is kept as an entered coordinate",
                    c.id
                ),
            );
        }
        // The GNSS position's height: a solution with no height stays an
        // entered coordinate.
        let gnss_elevation = if c.gnss { c.elevation } else { None };
        let gnss = gnss_elevation.is_some();
        let earlier = match self.given_coordinates.entry(c.id.clone()) {
            Entry::Vacant(slot) => {
                slot.insert(GivenCoordinate {
                    easting: c.easting,
                    northing: c.northing,
                    elevation: c.elevation,
                    record: c.record,
                    gnss,
                });
                None
            }
            Entry::Occupied(slot) => Some(*slot.get()),
        };
        // The builder compares an entered coordinate with an earlier one; a
        // GNSS position it does not hold, so it is compared here.
        if let Some(given) = earlier {
            let differs = given.easting != c.easting
                || given.northing != c.northing
                || (c.elevation.is_some() && given.elevation != c.elevation);
            if differs && (gnss || given.gnss) {
                let restated = coordinates_text(c.northing, c.easting, c.elevation);
                // What goes beside the point is the position it does not hold:
                // this GNSS position, where an entered coordinate or the first
                // GNSS one is held; the earlier GNSS position, where this
                // entered coordinate is - not this one, which is the point's
                // own.
                if gnss {
                    self.builder.add_point_metadata(
                        &c.id,
                        format!("coordinates restated at record {}", c.record),
                        restated.clone(),
                    );
                } else {
                    self.builder.add_point_metadata(
                        &c.id,
                        format!("GNSS position of record {}", given.record),
                        coordinates_text(given.northing, given.easting, given.elevation),
                    );
                }
                let before = format!("record {}", given.record);
                self.builder.warn(
                    c.record,
                    format!(
                        "point '{}' is given {} here ({restated} m) that differs from the {} of \
                         {before}; {}",
                        c.id,
                        if gnss {
                            "a GNSS position"
                        } else {
                            "an entered coordinate"
                        },
                        if given.gnss {
                            "GNSS position"
                        } else {
                            "entered coordinate"
                        },
                        if gnss && given.gnss {
                            "both are kept as positions, and the reduction checks this one \
                             against the first"
                        } else {
                            "the reduction holds the entered one, and the other is kept beside it"
                        }
                    ),
                );
            }
        }
        if let Some(elevation) = gnss_elevation {
            let precision = self.builder.options().precision;
            let position = GnssPositionObservation {
                point: c.id,
                northing: c.northing,
                easting: c.easting,
                elevation,
                sigma_northing: precision.gnss_horizontal,
                sigma_easting: precision.gnss_horizontal,
                sigma_elevation: precision.gnss_vertical,
                source: self.builder.source(c.record),
            };
            self.builder.add_loose_observation(position.into());
            return;
        }
        self.builder.position_point(
            &c.id,
            c.northing,
            c.easting,
            c.elevation,
            CoordinateSource::Entered,
            c.record,
            "",
        );
    }

    // Begins a setup; the builder's current station is then that setup.
    fn begin_setup(&mut self, point_id: &str, height: Option<f64>, stamp: &str, n: usize) {
        if height.is_none() {
            self.builder.warn(
                n,
                String::from("the setup states no instrument height; 0 is used"),
            );
        }
        let mut settings = InstrumentSettings::default();
        if self.distance_scale != 1.0 {
            // Applied, not NotApplied: measurement() has already multiplied
            // every slope distance of this setup by it, and a reduction that
            // read it as missing would apply it twice.
            settings.scale_factor = Some(self.distance_scale);
            settings.scale_factor_state = CorrectionState::Applied;
        }
        let setup = self
            .builder
            .begin_station(point_id, height.unwrap_or(0.0), &settings, n);
        if !stamp.is_empty() {
            setup
                .metadata
                .insert(String::from("time stamp"), String::from(stamp));
        }
        self.unread_setup_record = 0;
        self.open_resection = 0;
        self.open_resection_opcode = 0;
    }

    fn setup_not_read(&mut self, n: usize, message: String) {
        self.unread_setup_record = n;
        self.open_resection = 0;
        self.open_resection_opcode = 0;
        self.builder.skip(
            n,
            format!("{message}; the measurements after it are not read until the next setup"),
        );
    }

    // The setup just begun, for its metadata.
    fn insert_setup_metadata(&mut self, key: &str, value: String) {
        if let Some(setup) = self.builder.current_station() {
            setup.metadata.insert(String::from(key), value);
        }
    }

    fn coordinate(&mut self, d: &Description<'_>, v: &[&str], stamp: &str, n: usize) {
        let x = self.number(v[0], "X", n);
        let y = self.number(v[1], "Y", n);
        let z = self.number(v[2], "Z", n);
        let (Some(x), Some(y)) = (x, y) else {
            self.builder
                .skip(n, String::from("coordinate record without both X and Y"));
            return;
        };
        let id = String::from(d.id());
        // Positioned when its record group ends (finish_coordinate), once the
        // attributes after it have said whether it is a GNSS position.
        self.builder
            .code_point(&id, d.feature_code, d.comment, d.string_number, n);
        self.coordinate = Some(PendingCoordinate {
            id: id.clone(),
            easting: x,
            northing: y,
            elevation: z,
            record: n,
            gnss: false,
        });
        self.point_record_read(&id, d, n, true);
        if !stamp.is_empty() {
            self.put_point_value("time stamp", String::from(stamp), n);
        }
        self.keep_later_comment(d, n);
        self.keep_point_id(d, &id, n);
        Self::register_point(&mut self.coordinates_given, d, &id);
        self.builder.count_read();
    }

    fn station(&mut self, d: &Description<'_>, v: &[&str], stamp: &str, n: usize) {
        let at = self.known_point(d);
        self.warn_of_another_named_point(d, &at, "setup", "stands on", n);
        let height = self.number(v[0], "instrument height", n);
        self.begin_setup(&at, height, stamp, n);
        // A setup makes no measurement point, so its code strings nothing
        // ([FLD] 44.4); its comment still describes the point it stands on.
        let coding = d.coding();
        if !coding.is_empty() {
            self.insert_setup_metadata("setup coding", coding);
        }
        if !blank(d.comment) {
            self.builder.code_point(&at, "", d.comment, "", n);
        }
        // What the record calls the point, where 44.5 found it by another name
        // or by its ID: kept with the setup, not made a point of its own.
        if !blank(d.point_name) && d.point_name != at {
            self.insert_setup_metadata("point name", String::from(trimmed(d.point_name)));
        }
        if !blank(d.point_id) && d.point_id != at {
            self.insert_setup_metadata("point ID", String::from(trimmed(d.point_id)));
        }
        self.builder.count_read();
    }

    // A 04 to `target` from the current setup, kept for settle_backsights: its
    // setup's backsight, or a measurement of another point (see the top of
    // this file).
    fn backsight(
        &mut self,
        target: &str,
        d: &Description<'_>,
        v: &[&str],
        horizontal: Option<f64>,
        face: Face,
        n: usize,
    ) {
        // A backsight's own code strings nothing ([FLD] finds the backsight by
        // its name): kept with the setup it orients rather than on the point,
        // whose drawn properties are its shots'.
        let coding = d.coding();
        if !coding.is_empty() {
            self.insert_setup_metadata(&format!("backsight record {n}"), coding);
        }
        let mut kept = BacksightRecord {
            station: self.builder.project().stations.len() - 1,
            record: n,
            target: String::from(target),
            ..Default::default()
        };
        // The circle reading on its point as its face-left reading, which the
        // reduction means the pointings to the backsight to.
        if let Some(horizontal) = horizontal {
            let circle = horizontal * RADIANS_PER_DEGREE;
            kept.circle = Some(wrap_to_circle(if face == Face::Right {
                circle - PI
            } else {
                circle
            }));
        }
        // The azimuth the file states for its point ("may be specified when no
        // coordinate for the backsight point exists").
        if v.len() > 3
            && let Some(azimuth) = self.number(v[3], "backsight azimuth", n)
        {
            kept.azimuth = Some(wrap_to_circle(azimuth * RADIANS_PER_DEGREE));
            kept.azimuth_text = String::from(trimmed(v[3]));
        }
        self.backsights.push(kept);
    }

    // Names each setup's backsight, with its circle and azimuth, once the
    // file has said which points have a position.
    fn settle_backsights(&mut self) {
        // backsights is in file order, so each setup's 04s are together.
        let mut begin = 0;
        while begin < self.backsights.len() {
            let station = self.backsights[begin].station;
            let mut end = begin;
            while end < self.backsights.len() && self.backsights[end].station == station {
                end += 1;
            }
            self.settle_backsight(station, begin, end);
            begin = end;
        }
    }

    fn settle_backsight(&mut self, station: usize, begin: usize, end: usize) {
        // Each point the setup's 04s are to, in the order of its first 04,
        // with its first 04 to give a circle and its first to give an azimuth.
        struct Candidate {
            first: usize,
            circle: Option<usize>,
            azimuth: Option<usize>,
        }
        let mut candidates: Vec<Candidate> = Vec::new();
        {
            // By point, so that a setup of many 04s is settled in linear time.
            let mut index: HashMap<&str, usize> = HashMap::new();
            for (offset, b) in self.backsights[begin..end].iter().enumerate() {
                let i = begin + offset;
                let next = candidates.len();
                let slot = *index.entry(b.target.as_str()).or_insert(next);
                if slot == next {
                    candidates.push(Candidate {
                        first: i,
                        circle: None,
                        azimuth: None,
                    });
                }
                let known = &mut candidates[slot];
                if b.circle.is_some() {
                    known.circle.get_or_insert(i);
                }
                if b.azimuth.is_some() {
                    known.azimuth.get_or_insert(i);
                }
            }
        }
        // What gives each candidate's point a position the reduction will
        // find: coordinates, or a setup before this one that measured it.
        // Empty when nothing does. Worked out for every candidate before the
        // choice, which only reads it.
        let mut placed: Vec<String> = Vec::with_capacity(candidates.len());
        for c in &candidates {
            let point = &self.backsights[c.first].target;
            let text = if self.given_coordinates.contains_key(point) {
                String::from("coordinates the file gives")
            } else if let Some(&measured) = self.first_measured_from.get(point)
                && measured < station
            {
                format!(
                    "measured from setup {}",
                    self.builder.project().stations[measured].setup.id
                )
            } else {
                String::new()
            };
            placed.push(text);
        }
        // The first that can orient the setup: a circle to it, and a position
        // or a stated azimuth. Else the first with a circle, which the
        // reduction takes as set to the grid; else the first.
        let mut chosen: Option<usize> = None;
        let mut why = String::new();
        for (k, c) in candidates.iter().enumerate() {
            if c.circle.is_none() {
                continue;
            }
            if !placed[k].is_empty() {
                why = format!(
                    "the first that can orient it, with a horizontal circle, and {}",
                    placed[k]
                );
            } else if c.azimuth.is_some() {
                why = String::from(
                    "the first that can orient it, with a horizontal circle and a stated azimuth",
                );
            } else {
                continue;
            }
            chosen = Some(k);
            break;
        }
        let chosen = match chosen {
            Some(k) => k,
            None => match candidates.iter().position(|c| c.circle.is_some()) {
                Some(k) => {
                    why = String::from(
                        "the first with a horizontal circle, as none with one is to a point with \
                         a position or states an azimuth",
                    );
                    k
                }
                None => {
                    why = String::from("its first, as none gives a horizontal circle");
                    0
                }
            },
        };
        let chosen = &candidates[chosen];
        let point = self.backsights[chosen.first].target.clone();
        let chosen_record = self.backsights[chosen.first].record;
        let (setup_id, stated) = {
            let circle = chosen.circle.and_then(|i| self.backsights[i].circle);
            let azimuth = chosen.azimuth.and_then(|i| self.backsights[i].azimuth);
            let setup = &mut self.builder.project().stations[station];
            setup.backsight_point_id = point.clone();
            if chosen.circle.is_some() {
                setup.backsight_azimuth = circle;
            }
            if chosen.azimuth.is_some() {
                setup.stated_backsight_azimuth = azimuth;
            }
            (setup.setup.id.clone(), setup.stated_backsight_azimuth)
        };
        for b in &self.backsights[begin..end] {
            let key = format!("backsight azimuth record {}", b.record);
            if b.target == point {
                // A later azimuth for the backsight that differs is kept
                // beside the first.
                if let Some(azimuth) = b.azimuth
                    && Some(azimuth) != stated
                {
                    self.builder.project().stations[station]
                        .metadata
                        .insert(key, format!("{} degrees", b.azimuth_text));
                    self.builder.warn(
                        b.record,
                        format!(
                            "the backsight gives the azimuth {} degrees, where an earlier \
                             backsight of setup {setup_id} gave another; the first is the \
                             setup's, and this one is kept in its metadata",
                            b.azimuth_text
                        ),
                    );
                }
                continue;
            }
            // Its observations make it a measurement of its point, which the
            // reduction radiates, or checks where the point has a position.
            let mut azimuth = String::new();
            if b.azimuth.is_some() {
                self.builder.project().stations[station]
                    .metadata
                    .insert(key, format!("{} degrees", b.azimuth_text));
                azimuth = format!(
                    "; its azimuth, {} degrees, is kept in the setup's metadata",
                    b.azimuth_text
                );
            }
            self.builder.warn(
                b.record,
                format!(
                    "a backsight to '{}', where setup {setup_id} backsights '{point}' (record \
                     {chosen_record}), {why}: this one is read as a measurement of its \
                     point{azimuth}",
                    b.target
                ),
            );
        }
    }

    fn measurement(&mut self, opcode: i32, d: &Description<'_>, v: &[&str], stamp: &str, n: usize) {
        let kind = match opcode {
            4 => "backsight",
            6 => "check measurement",
            _ => "measurement",
        };
        if self.unread_setup_record != 0 {
            let message = format!(
                "{kind} from the setup of record {}, which was not read",
                self.unread_setup_record
            );
            self.builder.skip(n, message);
            return;
        }
        let Some(station) = self.builder.project().stations.len().checked_sub(1) else {
            self.builder.skip(
                n,
                format!("{kind} with no setup (opcode 03 or 128) before it"),
            );
            return;
        };
        let (setup_point, setup_id, hi) = {
            let setup = &self.builder.project().stations[station];
            (
                setup.setup.point_id.clone(),
                setup.setup.id.clone(),
                setup.setup.instrument_height,
            )
        };
        // A shot names its own point; a backsight and a check search for theirs.
        let target = if opcode == 7 {
            String::from(d.id())
        } else {
            self.known_point(d)
        };
        if target == setup_point {
            self.builder
                .skip(n, format!("a {kind} from '{target}' to itself"));
            return;
        }
        let horizontal = self.number(v[0], "horizontal circle", n);
        let vertical = self.number(v[1], "vertical circle", n);
        let slope = self.number(v[2], "slope distance", n);

        let mut face = Face::Unknown;
        let mut zenith: Option<f64> = None;
        if let Some(vertical) = vertical {
            let radians = vertical * RADIANS_PER_DEGREE;
            // The reading is finite (parse_real refuses anything else), so the
            // range test is the C++'s `radians < 0 || radians >= 2 pi`.
            if !(0.0..2.0 * PI).contains(&radians) {
                self.builder.warn(
                    n,
                    String::from(
                        "vertical circle reading outside a full circle; the zenith is not imported",
                    ),
                );
            } else {
                face = face_of_zenith_reading(radians);
                zenith = Some(zenith_in_model_range(radians));
            }
        }
        let mut distance: Option<f64> = None;
        if let Some(slope) = slope {
            let scaled = slope * self.distance_scale;
            if scaled > 0.0 {
                distance = Some(scaled);
            } else {
                self.builder.warn(
                    n,
                    format!(
                        "slope distance of {} m read as no distance measured",
                        format_exact_real(slope)
                    ),
                );
            }
        }
        if horizontal.is_none() && zenith.is_none() && distance.is_none() {
            // Nothing of the record reaches the model: no point, no code.
            self.builder
                .skip(n, format!("{kind} with no readable value"));
            return;
        }
        if opcode == 4 {
            self.warn_of_another_named_point(d, &target, "backsight", "measures", n);
        } else if opcode == 6 {
            self.warn_of_another_named_point(d, &target, "check", "checks", n);
        }

        self.builder.mention_point(&target, n);
        if opcode == 7 {
            self.builder
                .code_point(&target, d.feature_code, d.comment, d.string_number, n);
        } else if opcode == 4 {
            self.backsight(&target, d, v, horizontal, face, n);
        }
        // A direction and a distance: what the reduction radiates the point
        // from, so a later setup's backsight to it has a position.
        if horizontal.is_some() && distance.is_some() {
            self.first_measured_from
                .entry(target.clone())
                .or_insert(station);
        }

        let precision = self.builder.options().precision;
        let pointing = Pointing {
            index: self.builder.next_pointing(),
            face,
        };
        let shot = self.shots.entry(target.clone()).or_default();
        if shot.pointings == 0 {
            shot.station = station;
        }
        shot.pointings += 1;
        // The first 07 to the point from this setup is what an offset moves.
        let first_shot = opcode == 7 && shot.record == 0;
        if first_shot {
            shot.record = n;
            shot.measured = format!(
                "horizontal circle {}, vertical circle {}, slope distance {}",
                trimmed(v[0]),
                trimmed(v[1]),
                trimmed(v[2])
            );
        }
        if let Some(horizontal) = horizontal {
            let observation = self
                .builder
                .station_observation::<HorizontalDirectionObservation>(n);
            observation.at = setup_point.clone();
            observation.to = target.clone();
            observation.direction = wrap_to_circle(horizontal * RADIANS_PER_DEGREE);
            observation.sigma = precision.direction;
            observation.pointing = pointing;
            let direction = observation.direction;
            if first_shot {
                shot.direction =
                    Some(self.builder.project().stations[station].observations.len() - 1);
                shot.measured_reading.circle = direction;
            }
        }
        if let Some(zenith) = zenith {
            let observation = self
                .builder
                .station_observation::<ZenithAngleObservation>(n);
            observation.from = setup_point.clone();
            observation.to = target.clone();
            observation.angle = zenith;
            observation.sigma = precision.zenith;
            observation.instrument_height = hi;
            observation.target_height = self.target_height;
            observation.pointing = pointing;
            if first_shot {
                shot.zenith = Some(self.builder.project().stations[station].observations.len() - 1);
                shot.measured_reading.zenith = zenith;
            }
        }
        if let Some(distance) = distance {
            let observation = self.builder.station_observation::<DistanceObservation>(n);
            observation.from = setup_point.clone();
            observation.to = target.clone();
            observation.distance = distance;
            observation.sigma = distance_sigma(&precision, distance);
            observation.kind = DistanceKind::Slope;
            observation.instrument_height = hi;
            observation.target_height = self.target_height;
            observation.pointing = pointing;
            match self.setup_factor {
                None => self.setup_factor = Some(self.distance_scale),
                Some(factor) if factor != self.distance_scale && self.mixed_factor_record == 0 => {
                    self.mixed_factor_record = n;
                }
                Some(_) => {}
            }
            if first_shot {
                shot.distance =
                    Some(self.builder.project().stations[station].observations.len() - 1);
                shot.measured_reading.slope = distance;
            }
        }
        self.point_record_read(&target, d, n, opcode == 7);
        if opcode == 7 {
            self.measurement_is_shot = true;
            Self::register_point(&mut self.shots_given, d, &target);
        }
        if !stamp.is_empty() {
            self.put_point_value("time stamp", String::from(stamp), n);
        }
        if opcode == 6 {
            let coding = d.coding();
            let value = if coding.is_empty() {
                format!("from setup {setup_id}")
            } else {
                format!("from setup {setup_id}, {coding}")
            };
            self.put_point_value("check measurement", value, n);
        } else if opcode == 7 {
            self.keep_later_comment(d, n);
        }
        self.keep_point_id(d, &target, n);
        self.builder.count_read();
    }

    fn resection(&mut self, opcode: i32, v: &Values<'_>, n: usize) {
        // [FLD]: "128 instrument_height_value"; the files write a point
        // description first, as the XML form has one, and an empty value after.
        let values = &v.values;
        let (d, height) = if values.len() == 1 {
            (Description::default(), values[0])
        } else if values.len() == 6 || (values.len() == 7 && blank(values[6])) {
            (Description::of(values, 0), values[5])
        } else {
            self.setup_not_read(
                n,
                format!(
                    "resection (opcode {}) with {} value(s) where the format gives the instrument \
                     height, or a point description and the height; which value is which cannot \
                     be told",
                    opcode_text(opcode),
                    values.len()
                ),
            );
            return;
        };
        let point_id = if blank(d.id()) {
            format!("resection at record {n}")
        } else {
            String::from(d.id())
        };
        let height = self.number(height, "instrument height", n);
        self.begin_setup(&point_id, height, v.stamp, n);
        self.insert_setup_metadata(
            "resection",
            format!(
                "{}, record {n}",
                if opcode == 128 {
                    "least squares"
                } else {
                    "Helmert"
                }
            ),
        );
        // The XML form calls this description the "created resection point"'s;
        // [FLD] says nothing strings it, so its code is the setup's, as a 03's.
        let coding = d.coding();
        if !coding.is_empty() {
            self.insert_setup_metadata("setup coding", coding);
        }
        if !blank(d.point_id) && d.point_id != point_id {
            self.insert_setup_metadata("point ID", String::from(d.point_id));
        }
        if !blank(d.comment) {
            self.builder.code_point(&point_id, "", d.comment, "", n);
        }
        self.open_resection = n;
        self.open_resection_opcode = opcode;
        self.resections += 1;
        self.builder.count_read();
    }

    fn resection_end(&mut self, opcode: i32, v: &Values<'_>, n: usize) {
        let what = format!("end of a resection (opcode {})", opcode_text(opcode));
        if self.open_resection == 0 {
            self.builder
                .skip(n, format!("{what} with no resection open"));
            return;
        }
        if any_filled(&v.values, 0) {
            self.builder.warn(
                n,
                format!("{what} carries values the format does not give it; they are not kept"),
            );
        }
        if self.open_resection_opcode + 1 != opcode {
            self.builder.warn(
                n,
                format!(
                    "{what} ends the resection of record {}, which is opcode {}",
                    self.open_resection,
                    opcode_text(self.open_resection_opcode)
                ),
            );
        }
        self.insert_setup_metadata("resection end", format!("record {n}"));
        self.open_resection = 0;
        self.open_resection_opcode = 0;
        self.builder.count_read();
    }

    fn multiple_coding(&mut self, v: &Values<'_>, n: usize) {
        let what = "multiple coding (opcode 16)";
        // One not read would have made its own string the current one, so
        // which string is current is not known after it (see the top of this
        // file).
        if given_values(&v.values, &[5]) != 5 {
            self.unread_string_record = n;
            self.builder.skip(
                n,
                format!(
                    "{what} with {} value(s) where the format gives a point description of 5",
                    v.values.len()
                ),
            );
            return;
        }
        if !self.has_current_measurement(what, n) {
            return;
        }
        let d = Description::of(&v.values, 0);
        if blank(d.feature_code) {
            self.unread_string_record = n;
            self.builder.skip(
                n,
                format!("{what} with no feature code: there is no string to add the point to"),
            );
            return;
        }
        // [FLD] makes a second point at the current point's position, strung
        // by this record's code and number. Katana strings the current point
        // itself into that string - one position, as [FLD]'s is - and keeps
        // what this record says of the second point with it.
        let current = self.current_measurement.clone();
        self.builder
            .code_point(&current, d.feature_code, "", d.string_number, n);
        // [FLD]'s second point is the current measurement point now, and its
        // string the current string: a 20 after this closes it.
        self.current_code = String::from(d.feature_code);
        self.current_string = String::from(d.string_number);
        self.unread_string_record = 0;
        if !blank(d.id()) || !blank(d.comment) {
            let mut said = feature_name(d.feature_code, d.string_number);
            if !blank(d.point_id) {
                said.push_str(", point ID ");
                said.push_str(trimmed(d.point_id));
            }
            if !blank(d.point_name) {
                said.push_str(", point name ");
                said.push_str(trimmed(d.point_name));
            }
            if !blank(d.comment) {
                said.push_str(", comment ");
                said.push_str(trimmed(d.comment));
            }
            self.builder
                .add_point_metadata(&current, format!("multiple coding record {n}"), said);
            if !blank(d.id()) {
                self.builder.warn(
                    n,
                    format!(
                        "{what} names point '{}': Katana strings point '{current}' into {} rather \
                         than making a second point at its position, and keeps the name in its \
                         metadata",
                        d.id(),
                        feature_name(d.feature_code, d.string_number)
                    ),
                );
            }
        }
        self.builder.count_read();
    }

    fn close_string(&mut self, v: &Values<'_>, n: usize) {
        let what = "close string (opcode 20)";
        let values = &v.values;
        let (which, points) = if !any_filled(values, 0) {
            // "If no point description is given, the current string is closed."
            if !self.has_current_measurement(what, n) {
                return;
            }
            if self.unread_string_record != 0 {
                let message = format!(
                    "{what}: the current string is the one the multiple coding of record {} made \
                     current, which was not read",
                    self.unread_string_record
                );
                self.builder.skip(n, message);
                return;
            }
            if self.current_code.is_empty() {
                let message = format!(
                    "{what}: the point of record {} has no feature code, so there is no current \
                     string",
                    self.measurement_record
                );
                self.builder.skip(n, message);
                return;
            }
            let which = feature_name(&self.current_code, &self.current_string);
            let points = self
                .builder
                .close_feature(&self.current_code, &self.current_string);
            (which, points)
        } else if given_values(values, &[5]) == 5 {
            let d = Description::of(values, 0);
            if !blank(d.feature_code) {
                let which = feature_name(d.feature_code, d.string_number);
                let points = self.builder.close_feature(d.feature_code, d.string_number);
                (which, points)
            } else if !blank(d.id()) {
                // "the string containing that point ID": of the points records
                // before gave the ID (or the name) the description gives, the
                // first on an open string.
                let candidates = self.described_points_of(&d);
                let mut point = candidates
                    .first()
                    .cloned()
                    .unwrap_or_else(|| String::from(d.id()));
                let mut points = None;
                for candidate in &candidates {
                    points = self.builder.close_feature_of(candidate);
                    if points.is_some() {
                        point = candidate.clone();
                        break;
                    }
                }
                let mut which = format!("of point '{point}'");
                if !candidates.is_empty() && !blank(d.point_id) && d.point_id != point {
                    which.push_str(&format!(" (point ID {})", trimmed(d.point_id)));
                }
                if points.is_some() {
                    let why = if point == candidates[0] {
                        "the shot's"
                    } else {
                        "the one on an open string"
                    };
                    self.warn_of_shared_id(&d, &candidates, what, &point, why, n);
                }
                (which, points)
            } else {
                self.builder.skip(
                    n,
                    format!("{what} whose point description names neither a string nor a point"),
                );
                return;
            }
        } else {
            self.builder.skip(
                n,
                format!(
                    "{what} with {} value(s) where the format gives none, or a point description \
                     of 5",
                    values.len()
                ),
            );
            return;
        };
        let Some(points) = points else {
            self.builder
                .skip(n, format!("{what}: there is no open string {which}"));
            return;
        };
        if points < 3 {
            self.builder.warn(
                n,
                format!("the string {which} is closed with {points} point(s)"),
            );
        }
        self.builder.count_read();
    }

    fn point_offset(&mut self, opcode: i32, v: &Values<'_>, n: usize) {
        let kind = offset_kind(opcode);
        let what = format!("{kind} offset (opcode {})", opcode_text(opcode));
        let values = &v.values;
        // Why the offset cannot move a shot, when that is already known.
        let mut not_applied = String::new();
        let given = given_values(values, &[1, 6]);
        let (target, amount_text) = if given == 1 {
            // "If no point description is given, the offset is used to adjust
            // the position of the current measured point."
            if !self.has_current_measurement(&what, n) {
                return;
            }
            let target = self.current_measurement.clone();
            if !self.measurement_is_shot {
                not_applied = format!(
                    "point '{target}' is the coordinate of record {}, which is kept as the file \
                     states it",
                    self.measurement_record
                );
            }
            (target, values[0])
        } else if given == 6 {
            let d = Description::of(values, 0);
            let amount_text = values[5];
            if !blank(d.feature_code) {
                // "the last point of the previous string with that feature
                // code and string number is adjusted"
                let Some(last) = self.builder.last_point_of(d.feature_code, d.string_number) else {
                    self.builder.skip(
                        n,
                        format!(
                            "{what}: there is no open string {}",
                            feature_name(d.feature_code, d.string_number)
                        ),
                    );
                    return;
                };
                (last, amount_text)
            } else {
                let candidates = self.described_points_of(&d);
                if candidates.is_empty() {
                    self.builder.skip(
                        n,
                        format!("{what} names no string and no point a record before it made"),
                    );
                    return;
                }
                // "If the point ID exists, then the point with that point ID
                // is adjusted": the shot's, where a coordinate was given it
                // too.
                let target = candidates[0].clone();
                self.warn_of_shared_id(&d, &candidates, &what, &target, "the shot's", n);
                (target, amount_text)
            }
        } else {
            self.builder.skip(
                n,
                format!(
                    "{what} with {} value(s) where the format gives the offset, or a point \
                     description and the offset; which value is which cannot be told",
                    values.len()
                ),
            );
            return;
        };
        let Some(amount) = self.number(amount_text, &what, n) else {
            self.builder.skip(n, format!("{what} with no offset"));
            return;
        };
        // shots holds the current setup's, and none after a setup not read.
        if not_applied.is_empty() && self.shots.get(&target).is_none_or(|shot| shot.record == 0) {
            not_applied =
                format!("point '{target}' was not measured (opcode 07) from the current setup");
        }
        if !not_applied.is_empty() {
            self.builder.add_point_metadata(
                &target,
                format!("offset record {n}"),
                format!("{kind} {} m, not applied", format_exact_real(amount)),
            );
            self.builder.warn(
                n,
                format!(
                    "the {what} of {} m to point '{target}' is kept in its metadata and not \
                     applied: {not_applied}",
                    format_exact_real(amount)
                ),
            );
        } else {
            self.pending_offsets.push(PendingOffset {
                opcode,
                target,
                amount,
                record: n,
            });
        }
        self.builder.count_read();
    }

    // The current setup's offsets: each shot moved once for all of its own
    // (see the top of this file for which are not applied, and why).
    //
    // The arithmetic is offset_shot's. The reduction then applies its grid
    // scale factor k and its curvature to the offset position's plan distance
    // h' rather than to the measured h, so the offset is scaled by k with the
    // rest: (k - 1) of it, under a millimetre for a metre's offset anywhere in
    // a six-degree transverse Mercator zone (k from 0.9996 on the central
    // meridian to about 1.001 at a zone's edge, k0 (1 + (dl cos phi)^2 / 2) for
    // dl up to 3 degrees); the change in curvature is h (h' - h) (1 - k_r) / R,
    // a few micrometres.
    fn apply_offsets(&mut self) {
        // How many offsets of each kind each shot has.
        let mut kinds: HashMap<String, [usize; 3]> = HashMap::new();
        for offset in &self.pending_offsets {
            kinds.entry(offset.target.clone()).or_default()[(offset.opcode - 42) as usize] += 1;
        }
        // Why each offset is not applied; empty for one that is.
        let mut not_applied = vec![String::new(); self.pending_offsets.len()];
        for (i, offset) in self.pending_offsets.iter().enumerate() {
            let count = kinds[&offset.target];
            let shot = self
                .shots
                .get_mut(&offset.target)
                .expect("an offset is kept only for a shot of the current setup");
            if shot.pointings > 1 {
                not_applied[i] = format!(
                    "point '{}' was measured {} times from setup {}: the format moves only the \
                     measurement an offset follows, a point of its own there, and Katana, which \
                     makes one point of them, would put it between the moved and the unmoved",
                    offset.target,
                    shot.pointings,
                    self.builder.project().stations[shot.station].setup.id
                );
            } else if shot.distance.is_none() || shot.zenith.is_none() {
                not_applied[i] = format!(
                    "the shot of record {} has no slope distance or no zenith, so the point has \
                     no plan distance or height to offset",
                    shot.record
                );
            } else if count[0] > 1 || count[1] > 1 || count[2] > 1 {
                not_applied[i] = format!(
                    "the shot of record {} has two offsets of one kind, and whether the second \
                     adds to the first or replaces it the format does not say",
                    shot.record
                );
            } else if offset.opcode == 42 {
                shot.radial += offset.amount;
            } else if offset.opcode == 43 {
                shot.tangential += offset.amount;
            } else {
                shot.height += offset.amount;
            }
        }
        let precision = self.builder.options().precision;
        let mut moved: HashSet<String> = HashSet::new();
        let mut on_station: HashSet<String> = HashSet::new();
        for (i, offset) in self.pending_offsets.iter().enumerate() {
            let target = &offset.target;
            if !not_applied[i].is_empty() || !moved.insert(target.clone()) {
                continue;
            }
            let shot = &self.shots[target];
            let Some(reading) = offset_shot(
                &shot.measured_reading,
                shot.radial,
                shot.tangential,
                shot.height,
            ) else {
                on_station.insert(target.clone());
                continue;
            };
            let observations = &mut self.builder.project().stations[shot.station].observations;
            if let Some(direction) = shot.direction
                && let Observation::HorizontalDirection(observation) = &mut observations[direction]
            {
                observation.direction = reading.circle;
            }
            if let Some(zenith) = shot.zenith
                && let Observation::ZenithAngle(observation) = &mut observations[zenith]
            {
                observation.angle = reading.zenith;
            }
            if let Some(distance) = shot.distance
                && let Observation::Distance(observation) = &mut observations[distance]
            {
                observation.distance = reading.slope;
                observation.sigma = distance_sigma(&precision, reading.slope);
            }
            self.builder.add_point_metadata(
                target,
                format!("shot record {} as measured", shot.record),
                shot.measured.clone(),
            );
        }
        for (i, offset) in self.pending_offsets.iter().enumerate() {
            let shot = &self.shots[&offset.target];
            if not_applied[i].is_empty() && on_station.contains(&offset.target) {
                not_applied[i] = format!(
                    "with the other offsets of its shot it puts point '{}' on the station",
                    offset.target
                );
            }
            let kind = offset_kind(offset.opcode);
            let amount = format_exact_real(offset.amount);
            let mut value = format!(
                "{kind} {amount} m from setup {}",
                self.builder.project().stations[shot.station].setup.id
            );
            if not_applied[i].is_empty() {
                value.push_str(&format!(", applied to the shot of record {}", shot.record));
            } else {
                value.push_str(", not applied");
                self.builder.warn(
                    offset.record,
                    format!(
                        "the {kind} offset (opcode {}) of {amount} m to point '{}' is kept in its \
                         metadata and not applied: {}",
                        opcode_text(offset.opcode),
                        offset.target,
                        not_applied[i]
                    ),
                );
            }
            self.builder.add_point_metadata(
                &offset.target,
                format!("offset record {}", offset.record),
                value,
            );
        }
        self.pending_offsets.clear();
        self.shots.clear();
    }

    fn attribute(&mut self, opcode: i32, values: &[&str], n: usize) {
        if !any_filled(values, 0) {
            self.builder.skip(
                n,
                format!(
                    "attribute record (opcode {}) with neither a name nor a value",
                    opcode_text(opcode)
                ),
            );
            return;
        }
        if !self.has_current_point("an attribute", n) {
            return;
        }
        let name = values.first().map_or("", |value| trimmed(value));
        let value = if opcode == 73 {
            // Text: a tab in it split it into more values.
            String::from(trimmed(&values[1..].join("\t")))
        } else {
            let value = if values.len() < 2 {
                String::new()
            } else {
                String::from(trimmed(values[1]))
            };
            if any_filled(values, 2) {
                self.builder.warn(
                    n,
                    format!("attribute '{name}' has values after its value; they are not kept"),
                );
            }
            value
        };
        let label = if name.is_empty() { "unnamed" } else { name };
        if opcode == 72 && !value.is_empty() && parse_real(&value).is_none() {
            self.builder.warn(
                n,
                format!(
                    "real attribute '{label}' holds '{}', which is not a number; it is kept as \
                     text",
                    prefix(&value, 40)
                ),
            );
        } else if opcode == 71 && !value.is_empty() && parse_integer(&value).is_none() {
            self.builder.warn(
                n,
                format!(
                    "integer attribute '{label}' holds '{}', which is not a whole number; it is \
                     kept as text",
                    prefix(&value, 40)
                ),
            );
        }
        // An 02 whose own attributes say what the receiver solved is a GNSS
        // position: coordinate is set only while the 02's record group lasts.
        if let Some(coordinate) = &mut self.coordinate
            && equals_ignoring_case(name.as_bytes(), GNSS_SOLUTION.as_bytes())
        {
            coordinate.gnss = true;
        }
        // [FLD] 44.8, opcodes 68 to 79: a blank name makes the attribute unnamed.
        let key = if name.is_empty() {
            format!("{}unnamed attribute (record {n})", self.group_prefix)
        } else {
            format!("{}{name}", self.group_prefix)
        };
        self.put_point_value(&key, value, n);
        self.builder.count_read();
    }

    fn additional_text(&mut self, values: &[&str], n: usize) {
        if !self.has_current_point("additional text", n) {
            return;
        }
        // Not trimmed: "any spaces from column four onwards will be part of
        // the text". A tab in it split it into more values.
        let text = values.join("\t");
        self.builder
            .add_point_metadata(&self.current_point, format!("additional text {n}"), text);
        self.builder.count_read();
    }

    fn pop_group(&mut self) {
        if self.group_named.last() == Some(&true) {
            self.named_groups -= 1;
        }
        if let Some(&length) = self.group_prefix_lengths.last() {
            self.group_prefix.truncate(length);
        }
        self.groups.pop();
        self.group_records.pop();
        self.group_named.pop();
        self.group_prefix_lengths.pop();
    }

    // Ends the open groups from depth `from` inward, each a warning that says
    // `why` (record n).
    fn end_groups(&mut self, from: usize, n: usize, why: &str) {
        while self.groups.len() > from {
            let record = self.group_records.last().copied().unwrap_or(0);
            let open = self.groups.last().cloned().unwrap_or_default();
            self.builder.warn(
                record,
                format!(
                    "attribute group '{open}' is not ended (opcode 125) before record {n}, which \
                     {why}; it is ended there"
                ),
            );
            self.pop_group();
        }
    }

    fn group_start(&mut self, values: &[&str], n: usize) {
        // XML: a name and a level. The files: an empty value, the name, the
        // level.
        let (before, name, level) = if values.len() == 3 {
            (trimmed(values[0]), trimmed(values[1]), trimmed(values[2]))
        } else if values.len() == 2
            && blank(values[0])
            && parse_integer(trimmed(values[1])).is_none()
        {
            // The files' form without its level: the empty value and the name.
            ("", trimmed(values[1]), "")
        } else if values.len() == 2 {
            ("", trimmed(values[0]), trimmed(values[1]))
        } else if values.len() == 1 {
            ("", trimmed(values[0]), "")
        } else {
            self.builder.skip(
                n,
                format!(
                    "attribute group (opcode 124) with {} value(s); which is its name cannot be \
                     told",
                    values.len()
                ),
            );
            return;
        };
        if !before.is_empty() {
            self.builder.warn(
                n,
                format!(
                    "attribute group '{name}' has '{}' before its name, which the files this \
                     reader knows leave empty; it is not kept",
                    prefix(before, 40)
                ),
            );
        }
        if name.is_empty() {
            self.builder.warn(
                n,
                String::from("attribute group with no name: its attributes are kept without one"),
            );
        }
        if !level.is_empty() {
            let depth = parse_integer(level);
            // A negative level converts to a size past any open count, as the
            // C++'s static_cast<std::size_t> does.
            if let Some(depth) = depth
                && depth >= 0
                && (depth as usize) < self.groups.len()
            {
                // A level a group still open has: that group, and any inside
                // it, were not ended.
                self.end_groups(
                    depth as usize,
                    n,
                    &format!("opens attribute group '{name}' at its level"),
                );
            } else if depth.is_none_or(|depth| depth as usize != self.groups.len()) {
                self.builder.warn(
                    n,
                    format!(
                        "attribute group '{name}' gives the level '{}' and opens inside {} \
                         group(s); it is nested as the records open and end it",
                        prefix(level, 20),
                        self.groups.len()
                    ),
                );
            }
        }
        // The name and its '/' fit in GROUP_PATH_LENGTH: the C++'s
        // `prefix + name + 1 <= length`, as Clippy would have it.
        let named = !name.is_empty()
            && self.named_groups < GROUP_DEPTH
            && self.group_prefix.len() + name.len() < GROUP_PATH_LENGTH;
        if !name.is_empty() && !named {
            self.builder.warn(
                n,
                format!(
                    "attribute group '{}' opens inside {} named group(s), past the {GROUP_DEPTH} \
                     levels or {GROUP_PATH_LENGTH} characters an attribute's name nests: its \
                     attributes are kept under the groups around it",
                    prefix(name, 40),
                    self.named_groups
                ),
            );
        }
        self.groups.push(String::from(name));
        self.group_records.push(n);
        self.group_named.push(named);
        self.group_prefix_lengths.push(self.group_prefix.len());
        if named {
            self.group_prefix.push_str(name);
            self.group_prefix.push('/');
            self.named_groups += 1;
        }
        self.builder.count_read();
    }

    fn group_end(&mut self, values: &[&str], n: usize) {
        let Some(open) = self.groups.last().cloned() else {
            self.builder.skip(
                n,
                String::from("end of an attribute group (opcode 125) with no group open"),
            );
            return;
        };
        // The files give the level of the group ended, last; a name, where one
        // is given, before it.
        let level = values.last().map_or("", |value| trimmed(value));
        let name = if values.len() < 2 {
            ""
        } else {
            trimmed(values[values.len() - 2])
        };
        if !level.is_empty() {
            // The C++ adds 1 to the level as a signed 64-bit number, which
            // for the largest level overflows (and wraps, in practice);
            // wrapping keeps that and does not panic.
            let depth = parse_integer(level);
            if depth.is_none_or(|depth| depth.wrapping_add(1) != self.groups.len() as i64) {
                self.builder.warn(
                    n,
                    format!(
                        "the end of attribute group '{open}' gives the level '{}', which is not \
                         the group's",
                        prefix(level, 20)
                    ),
                );
            }
        }
        if !name.is_empty() && name != open {
            self.builder.warn(
                n,
                format!(
                    "the end of attribute group '{open}' names '{}'; the open group is ended",
                    prefix(name, 40)
                ),
            );
        }
        self.pop_group();
        self.builder.count_read();
    }

    fn unread(&mut self, opcode: i32, n: usize) {
        let known = unread_opcode(opcode);
        // What follows a record about a point of its own describes that point,
        // and what follows an opcode the format does not define may: neither
        // is given to the point before.
        if known.is_none_or(|known| known.about_a_point) {
            self.point_record_starts(n, known.is_none() || makes_measurement(opcode));
        }
        let mut message = format!("opcode {}", opcode_text(opcode));
        match known {
            None => message.push_str(" is not one the format defines"),
            Some(known) if known.xml_only => message.push_str(&format!(
                " ({}) is defined only for the format's XML form, which says it does not exist in \
                 a .fld; its values are not guessed at",
                known.name
            )),
            Some(known) => {
                message.push_str(&format!(" ({}) is not one this reader imports", known.name))
            }
        }
        if let Some(known) = known {
            if known.corrects_later {
                message.push_str("; the measurements after it are read without it");
            } else if known.codes_later {
                message.push_str(
                    "; the measurements after it keep the codes they are written with, which a \
                     field template may have changed",
                );
            } else if known.ends_string {
                // 47 also moves the current point, the first of the new string.
                message.push_str(if opcode == 47 {
                    "; the current point is not moved into a new string of its own, and"
                } else {
                    ";"
                });
                message.push_str(
                    " the points after it are strung as if the string had not ended, a later \
                     point of its code and string number joining it",
                );
            }
        }
        self.builder.skip(n, message);
    }

    fn not_a_record(&mut self, first: &str, n: usize) {
        // `first` is never empty: a line is read from its first character
        // that is not a blank or a tab.
        let why = if first.bytes().all(|c| b"+-0123456789".contains(&c)) {
            format!(
                "'{}' is not an opcode: an opcode is a number of one to three digits, a minus \
                 before the negative ones",
                prefix(first, 20)
            )
        } else {
            format!(
                "'{}' is not an opcode: a record opens with a number",
                prefix(first, 20)
            )
        };
        // What the line made is not known. A number is taken for the opcode
        // it would be, for the bookkeeping alone; anything else may have made
        // a point, so what follows is not given to the point before.
        let loose = loose_opcode(first);
        if let Some(loose) = loose
            && makes_point_or_setup(loose)
        {
            self.header = false;
            self.end_groups(
                0,
                n,
                "is taken for the record that makes a point or a setup it would be",
            );
        }
        if let Some(loose) = loose
            && starts_setup(loose)
        {
            self.finish_coordinate();
            self.end_setup();
            self.setup_not_read(
                n,
                format!(
                    "{why}; it is taken for the setup (opcode {}) it would be",
                    opcode_text(loose)
                ),
            );
            return;
        }
        match loose {
            None => self.point_record_starts(n, true),
            Some(loose) if makes_point_or_setup(loose) => {
                self.point_record_starts(n, makes_measurement(loose));
            }
            Some(_) => {}
        }
        self.builder.skip(n, why);
    }

    fn decide_layout(&mut self, lines: &[&str]) {
        let mut column = 0usize;
        let mut no_column = 0usize;
        for line in lines {
            let lead = without_indent(line);
            let tab = lead.find('\t');
            let opcode = opcode_of(trimmed(&lead[..tab.unwrap_or(lead.len())]).as_bytes());
            if opcode == Some(99) {
                break; // "Stop processing ... at this line": what follows is not read, so no vote
            }
            let (Some(_), Some(opcode)) = (tab, opcode) else {
                continue;
            };
            if values_after_description(opcode).is_none() {
                continue;
            }
            let fields = split_tabs(lead);
            let fit = layout_fit(opcode, &fields);
            let dated = is_date_or_time(fields[1]);
            if fit.column && fit.plain {
                column += usize::from(dated); // only the column holds a date or time
            } else if fit.column && (blank(fields[1]) || dated) {
                column += 1;
            } else if fit.plain && !blank(fields[1]) {
                no_column += 1;
            }
        }
        if column > 0 || no_column > 0 {
            self.layout = if column >= no_column {
                Layout::Column
            } else {
                Layout::NoColumn
            };
        }
        if column > 0 && no_column > 0 {
            self.builder.warn(
                0,
                format!(
                    "{column} fixed record(s) have the column after the opcode (blank or a date \
                     or time) and {no_column} do not: each is read in the layout its fields show, \
                     and one whose first field is blank in the layout most have, {} it",
                    if self.layout == Layout::Column {
                        "with"
                    } else {
                        "without"
                    }
                ),
            );
        }
    }

    // A fixed record that is not read: a setup's leaves no setup.
    fn fail_fixed(&mut self, opcode: i32, n: usize, message: String) {
        if opcode == 3 {
            self.setup_not_read(n, message);
        } else {
            self.builder.skip(n, message);
        }
    }

    fn fixed_record(&mut self, opcode: i32, fields: &[&str], n: usize) {
        let arity = values_after_description(opcode).unwrap_or(0);
        if opcode != 3 {
            self.point_record_starts(n, opcode == 2 || opcode == 7);
        }
        let counted = format!(
            "opcode {} has {} values",
            opcode_text(opcode),
            fields.len() - 1
        );
        let fit = layout_fit(opcode, fields);
        let first = if fields.len() > 1 { fields[1] } else { "" };
        let mut columns = 0usize;
        if is_date_or_time(first) {
            // Only the column holds a date or time.
            if !fit.column {
                self.fail_fixed(
                    opcode,
                    n,
                    format!(
                        "{counted}, a date or time first, as the column after the opcode holds, \
                         where the format gives {} with it; which value is which cannot be told",
                        6 + arity
                    ),
                );
                return;
            }
            columns = 1;
        } else if !blank(first) {
            // A feature code: the column holds nothing else.
            if !fit.plain {
                let message = if fit.column {
                    format!(
                        "opcode {} holds '{}' in the column after the opcode, which is neither \
                         blank nor a date or time; which value is which cannot be told",
                        opcode_text(opcode),
                        prefix(trimmed(first), 40)
                    )
                } else {
                    format!(
                        "{counted} where the format gives {}; which value is which cannot be told",
                        5 + arity
                    )
                };
                self.fail_fixed(opcode, n, message);
                return;
            }
        } else {
            // Blank: the file's layout says which (one no fixed record decided
            // is read as [FLD] writes it), unless its reading leaves the first
            // value blank where the other reads a whole record - a number there
            // and a point named. Text there, or no point, is no other reading.
            columns = usize::from(self.layout == Layout::Column);
            let other = 1 - columns;
            if fit.column
                && fit.plain
                && blank(first_value(fields, columns))
                && parse_real(first_value(fields, other)).is_some()
                && !blank(Description::of(fields, 1 + other).id())
            {
                self.fail_fixed(
                    opcode,
                    n,
                    format!(
                        "{counted}, which fit the format's layout both with the column after the \
                         opcode and without it: read {} it, as this file's records are, {} is \
                         blank, and read {} it, it is not; which value is which cannot be told",
                        if columns == 1 { "with" } else { "without" },
                        first_value_name(opcode),
                        if columns == 1 { "without" } else { "with" }
                    ),
                );
                return;
            }
            let fits_this_layout = if columns == 1 { fit.column } else { fit.plain };
            if !fits_this_layout {
                let how = if columns == 1 {
                    " in this file (with the column after the opcode)"
                } else if self.layout == Layout::Undecided {
                    " (no record shows the column after the opcode as blank or a date or time, so \
                     the file is read without it)"
                } else {
                    ""
                };
                self.fail_fixed(
                    opcode,
                    n,
                    format!(
                        "{counted} where the format gives {}{how}; which value is which cannot be \
                         told",
                        5 + arity + columns
                    ),
                );
                return;
            }
        }
        // One blank field past the count is a trailing tab.
        let count = if count_fits(opcode, fields.len(), columns) {
            fields.len()
        } else {
            fields.len() - 1
        };
        let offset = 1 + columns;
        let d = Description::of(fields, offset);
        let values = &fields[offset + 5..count];
        let stamp = if columns == 1 { trimmed(first) } else { "" };
        if blank(d.id()) {
            self.fail_fixed(
                opcode,
                n,
                format!(
                    "opcode {} names its point by neither a point name nor a point ID",
                    opcode_text(opcode)
                ),
            );
            return;
        }
        match opcode {
            2 => self.coordinate(&d, values, stamp, n),
            3 => self.station(&d, values, stamp, n),
            _ => self.measurement(opcode, &d, values, stamp, n),
        }
    }

    fn record(&mut self, opcode: i32, fields: &[&str], n: usize) {
        if makes_point_or_setup(opcode) {
            self.header = false;
            self.finish_coordinate();
            self.end_groups(0, n, "makes another point or a setup");
        }
        if starts_setup(opcode) {
            self.end_setup();
        }
        if values_after_description(opcode).is_some() {
            self.fixed_record(opcode, fields, n);
            return;
        }
        let v = self.values_of(opcode, fields);
        if !v.stamp.is_empty() && opcode != 128 && opcode != 138 && !self.unkept_stamp_warned {
            self.unkept_stamp_warned = true;
            self.builder.warn(
                n,
                String::from(
                    "the date and time in the column after the opcode is kept on the points and \
                     setups records make; on this record, and any like it after, it is not kept",
                ),
            );
        }
        let first = v.values.first().map_or("", |value| trimmed(value));
        match opcode {
            100 => self.units(&v.values, n),
            5 => {
                if let Some(height) = self.number(first, "target height", n) {
                    self.target_height = height;
                    self.target_height_seen = true;
                    self.builder.count_read();
                } else {
                    self.builder.skip(
                        n,
                        String::from("target height record without a height the reader can read"),
                    );
                }
            }
            9 => match self.number(first, "scale factor", n) {
                Some(factor) if factor > 0.0 => self.scale_factor(factor, n),
                _ => self.builder.skip(
                    n,
                    String::from(
                        "scale factor record without a positive factor the reader can read",
                    ),
                ),
            },
            29 | -2 => {
                let mut text = String::new();
                for (i, value) in v.values.iter().enumerate() {
                    if i > 0 {
                        text.push(' ');
                    }
                    text.push_str(trimmed(value));
                }
                self.builder.add_station_note(trimmed(&text));
                self.builder.count_read();
            }
            41 => self.additional_text(&v.values, n),
            71..=73 => self.attribute(opcode, &v.values, n),
            124 => self.group_start(&v.values, n),
            125 => self.group_end(&v.values, n),
            16 => self.multiple_coding(&v, n),
            20 => self.close_string(&v, n),
            42..=44 => self.point_offset(opcode, &v, n),
            128 | 138 => self.resection(opcode, &v, n),
            129 | 139 => self.resection_end(opcode, &v, n),
            _ => self.unread(opcode, n),
        }
    }

    fn read(mut self, bytes: &[u8]) -> Result<ReadResult> {
        let decoded = decode_text(bytes)?;
        if decoded.guessed {
            self.builder.warn(
                0,
                format!(
                    "the file is not UTF-8; it was read as {}, so an accented name may be wrong",
                    encoding_name(decoded.encoding)
                ),
            );
        }
        // CR, LF and CRLF each end a line, so a record number is a line number
        // whichever system wrote the file.
        let lines = lines_of(&decoded.text);
        self.decide_layout(&lines);
        for (i, line) in lines.iter().enumerate() {
            if self.fatal.is_some() {
                break;
            }
            let n = i + 1;
            let lead = without_indent(line);
            if blank(lead) || lead == "\u{1A}" {
                continue; // nothing, or a DOS end-of-file byte
            }
            if lead.starts_with("//") {
                self.comment(lead, n);
                continue;
            }
            if self.first_record && lead.starts_with("{Version") {
                // "{Version 6.0}": the file's own version line.
                self.first_record = false;
                self.builder
                    .project()
                    .metadata
                    .insert(String::from("version"), prefix(trimmed(lead), 60));
                self.builder.count_read();
                continue;
            }
            self.first_record = false;
            let fields = split_tabs(lead);
            let first = trimmed(fields[0]);
            let Some(opcode) = opcode_of(first.as_bytes()) else {
                self.not_a_record(first, n);
                continue;
            };
            if opcode == 99 {
                // "Stop processing ... at this line": what follows is not
                // read, and is counted.
                self.builder.count_read();
                let after = lines[i + 1..]
                    .iter()
                    .filter(|rest| {
                        let rest = trimmed(rest);
                        !rest.is_empty() && !rest.starts_with("//") && rest != "\u{1A}"
                    })
                    .count();
                if after > 0 {
                    self.builder.count_skipped(after);
                    self.builder.warn(
                        n,
                        format!(
                            "{after} record(s) after the end of the file (opcode 99) are not \
                             read, as the format says"
                        ),
                    );
                }
                break;
            }
            self.record(opcode, &fields, n);
        }
        if let Some(fatal) = self.fatal.take() {
            return Err(fatal);
        }
        self.finish_coordinate();
        self.end_setup();
        // Every coordinate is read now, so each setup's backsight can be named.
        self.settle_backsights();
        for (group, &record) in self.groups.iter().zip(&self.group_records) {
            self.builder.warn(
                record,
                format!("attribute group '{group}' is not ended (opcode 125) before the file ends"),
            );
        }
        let project = self.builder.project();
        project.units.linear = LinearUnit::Metres;
        project.units.angular = AngularUnit::DecimalDegrees;
        if !self.coordinate_system.is_empty() {
            project.coordinate_system = DeclaredCoordinateSystem::named(if self.zone.is_empty() {
                self.coordinate_system.clone()
            } else {
                format!("{}, {}", self.coordinate_system, self.zone)
            });
        }
        if !self.header_text.is_empty() {
            project
                .metadata
                .insert(String::from("header"), self.header_text.clone());
        }
        if !project.stations.is_empty() {
            if !self.target_height_seen {
                self.builder.not_carried(String::from("no target heights"));
            }
            self.builder.not_carried(String::from(
                "no prism constant the reduction can use: the format carries none for the \
                 instrument, and a \"Prism constant\" attribute on a point states no unit",
            ));
            self.builder
                .not_carried(String::from("no atmospheric settings"));
        }
        if self.resections > 0 {
            self.builder.not_carried(format!(
                "no coordinates for the {} setup(s) made by resection (opcode 128 or 138): the \
                 file does not state them, so SURVEY IMPORT computes each by the reduction's \
                 resection from the points it measures; until then those setups and what was \
                 measured from them are not positioned",
                self.resections
            ));
        }
        let result = self.builder.finish();
        if result.project.stations.is_empty()
            && result.project.points.is_empty()
            && result.project.unpositioned_points.is_empty()
        {
            let mut context = format!("{} warning(s)", result.warnings.len());
            if let Some(first) = result.warnings.first() {
                context.push_str(", the first: ");
                context.push_str(&describe(first));
            }
            return Err(Error::new(
                ErrorCode::FileImportFailure,
                format!(
                    "{} holds no station, measurement or coordinate Katana could read",
                    result.project.source.file_name
                ),
            )
            .with_context(context));
        }
        Ok(result)
    }
}

// ---- Registration -----------------------------------------------------------------

fn probe_field_file(input: &ProbeInput<'_>) -> FormatSignature {
    let bytes = without_byte_order_mark(input.bytes);
    if !looks_like_text(bytes) {
        return ruled_out();
    }
    let mut version_line = false;
    let mut lines = 0usize;
    let mut records = 0usize;
    let mut measurements = 0usize;
    let mut first = true;
    // Every line PROBE_BYTES holds, comments passed over, so that a long
    // header of comments does not hide the records after it.
    for line in probe_lines(input, PROBE_BYTES) {
        let line = trimmed_bytes(line);
        if line.is_empty() || line.starts_with(b"//") {
            continue;
        }
        if first && line.starts_with(b"{Version ") {
            version_line = true;
            first = false;
            continue;
        }
        first = false;
        lines += 1;
        // The opcode as the reader takes it: trimmed (" 2" is opcode 2), and
        // a record even alone on its line ("20", "129").
        let tab = line.iter().position(|&c| c == b'\t');
        let Some(opcode) = opcode_of(trimmed_bytes(&line[..tab.unwrap_or(line.len())])) else {
            continue;
        };
        records += 1;
        // A coordinate, setup or shot of the field count the format gives it,
        // in either layout: a list of numbered rows ("   2<tab>E<tab>N<tab>Z")
        // opens with numbers too, but not with such records.
        if tab.is_some() && (opcode == 2 || opcode == 3 || opcode == 7) {
            let fields: Vec<&[u8]> = line.split(|&c| c == b'\t').collect();
            let fit = layout_fit(opcode, &fields);
            measurements += usize::from(fit.column || fit.plain);
        }
    }
    if records == 0 || measurements == 0 {
        return ruled_out();
    }
    let share = records as f64 / lines as f64;
    let evidence = format!("{records} of {lines} records open with a numeric opcode");
    let fld = input.extension == "fld";
    if share < 0.9 {
        return FormatSignature {
            confidence: if fld { 0.3 } else { 0.1 },
            evidence,
        };
    }
    if version_line {
        return FormatSignature {
            confidence: if fld { 0.98 } else { 0.9 },
            evidence: format!(
                "{}a {{Version ...}} line and {evidence}",
                if fld { "extension .fld, " } else { "" }
            ),
        };
    }
    FormatSignature {
        confidence: if fld { 0.85 } else { 0.5 },
        evidence: format!("{}{evidence}", if fld { "extension .fld and " } else { "" }),
    }
}

fn opcode_field_file_format() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(FORMAT_ID),
        human_name: String::from("Opcode field file (.fld)"),
        manufacturer: Manufacturer::Other,
        // gnss: an RTK position written as an 02 with its receiver's solution
        // is a GNSS position (see the top of this file).
        reads: FormatContent {
            points: true,
            observations: true,
            stations: true,
            features: true,
            coordinate_system: true,
            instrument_settings: false,
            gnss: true,
        },
        can_import: true,
        can_export: false,
        // 1.1: the column's date and time, blank-padded opcodes, resections,
        // multiple coding, strings closed, offsets applied, attribute groups,
        // every measurement's attributes kept, RTK positions as GNSS positions
        // and the backsight's stated azimuth. 1.2: the backsight is the setup's
        // first 04 that can orient it, 44.5's search by point ID and in its
        // order, 20 and the offsets by point ID, a record whose other layout
        // reads no whole record, 16, 20 and the offsets with a trailing tab, an
        // unread 16, the records after a 99 voting on no layout, a GNSS
        // position then an entered coordinate of one point - a job with any of
        // those imports differently.
        parser_version: String::from("1.2"),
        extensions: vec![String::from("fld")],
    }
}

fn read_field_file(bytes: &[u8], file_name: &str, options: &ReadOptions) -> Result<ReadResult> {
    FieldFileReader::new(file_name, options).read(bytes)
}

/// This format's line in the program's registry (format.rs).
pub(crate) fn registration() -> FormatRegistration {
    FormatRegistration::new(
        opcode_field_file_format(),
        probe_field_file,
        Some(read_field_file),
    )
}
