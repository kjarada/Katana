//! RW5 raw data: the comma-separated field journal of TDS Survey Pro, Carlson
//! SurvCE and Topcon Magnet Field.
//!
//! The port of src/katana_surveyio/topcon_rw5.cpp, which has no header of its
//! own: the format reaches the program through its registration alone.

// Specifications this reader implements, and nothing beyond them:
//   [TDS]  Tripod Data Systems, Inc., "Raw Data Record Specification, Survey
//          Pro Version 3.6", October 2, 2002 - every record type and field
//          tag, the enumerations (AD, UN, AU, EC, GM, ME ...) and units ("See
//          MO for units").
//   [SCE]  Carlson Software, "SurvCE Raw Data File Format (*.rw5) - Version
//          3.03", updated February 25, 2014 - the direct/reverse set records
//          BD, BR, FD, FR, the GPS record, the BP base position with its
//          antenna fields, and the G0 to G4 GNSS vector records.
//   [SCE250] Carlson Software, "SurvCE Version 2.50 Raw File records",
//          updated 5/4/2010 - the units and antenna points of the GNSS
//          vector: "The DX, DY and DZ values are phase center to phase
//          center. ALL THE VALUES ARE ALWAYS IN METERS"; "You will get the rod
//          height of the rover from the LS record prior to the vector records.
//          The LS,HR value is from phase center to the ground ... THE UNITS CAN
//          BE FEET OR METERS" (the MO unit); and the BP record's AG and PA,
//          always metres, added "to get the Phase Center to Ground value".
//   [CSP]  Carlson Survey 2019 help, "Edit-Process Raw Data File": "The Set
//          Azimuth is the circle reading of the instrument when sighting the
//          backsight", and that the MO scale factor "is multiplied by the
//          horizontal distance" and earth curvature "adjusts the calculated
//          points" when the file is PROCESSED - so neither is in a recorded
//          distance or angle.
//
// Readings of the specification that are this reader's own, stated here so
// that they can be checked:
//   * AR (angle right) is read as the horizontal CIRCLE reading, and BC (back
//     circle) as the circle reading on the backsight; each AR becomes a
//     HorizontalDirectionObservation and BC the setup's backsight_azimuth
//     (which the model defines as that reading). The evidence is [SCE]'s own
//     sample: the sideshot to the backsight point (SS,OP1,FP2,AR0.0044 after
//     BK,OP1,BP2,BS315.0000,BC0.0044) reads exactly the back circle, and its
//     reverse set records BR at 180 00' 37" - a circle reading, not an angle
//     from the backsight. Where BC is zero, the usual case, both readings
//     agree.
//   * BS, the backsight AZIMUTH, has no field in the model (backsight_azimuth
//     is the circle reading): it is kept in the setup's metadata as
//     "backsight azimuth (radians)". [TDS]'s field list gives BS the type
//     "Linear Distance", which its own sample (BS315.0000) contradicts.
//   * Zeniths above 180 degrees are face-right readings: the observation
//     holds 360 - z with Face::Right. BD/FD are face left and BR/FR face
//     right by their record type; a zenith that disagrees is a warning.
//   * CE (change in elevation) is a line-of-sight height difference, like
//     one computed from ZE and SD; it becomes a LevelDifferenceObservation of
//     CE + HI - HR, ground to ground ([CSP]: LS "sets the instrument and rod
//     heights used in elevation calculations").
//   * EO, the EDM offset in inches, is the setup's prism constant; the
//     specification does not say whether a recorded distance includes it, so
//     its state is Unknown.
//   * The description after "--" is the point's description; its first word
//     is the field code that strings the point into a feature.
//   * OF (off-centre shot) does not say which shot it corrects, so it is not
//     applied to one: every field it carries ([SCE]'s AR, ZE and SD, [TDS]'s
//     OL, HD, VD and LR) is kept as written in the setup's metadata, with a
//     warning. FE (foresight elevation), listed by [SCE] without a record, is
//     kept in the target point's metadata.
//   * A SurvCE vector (G1) is in metres whatever the MO unit, as [SCE250]
//     says; a Survey Pro baseline (BL) is in the MO unit, as [TDS] says ("See
//     MO for units"). The vector runs from phase centre to phase centre
//     ([SCE250]) unless a G4 record names another point for an end: its base
//     end is the last BP record's AG (to the reference point) plus its PA
//     (to the phase centre), its rover end the LS HR in force at the G1.
//   * Nothing in a record is dropped without a word: every field the
//     specifications define for a record is read into the model or into
//     metadata; a field they do not define is warned about at its first
//     record and counted at the end; a note on a record with no use for one
//     joins the setup's notes; and a shot giving two of a group [SCE] says to
//     give one of (AR/AL/AZ/BR/DR/DL, ZE/VA/CE, SD/HD) reads the first and
//     names the others.
//
// Records the specifications define that carry no survey observation or
// coordinate - stake-out, cut sheets, slope staking, calibration, projection
// and legacy levelling records - are warned about by number and counted as
// skipped, never dropped silently.
//
// Text: the file is read as bytes, undecoded, as the C++ reads it. A field
// goes into the model as UTF-8, any byte that is not replaced (text()).

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]
#![allow(
    clippy::manual_range_contains,
    reason = "The C++'s two comparisons, which let a NaN through where a range's contains would not."
)]

use std::cell::Cell;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, is_ascii_space, trimmed};
use katana_core::{Error, ErrorCode, Result};
use katana_math::numerics::{PI, TWO_PI};
use katana_survey::data_model::{self, distance_sigma};
use katana_survey::{
    AngularUnit, AntennaHeightMethod, AzimuthObservation, CoordinateSource, CorrectionState,
    DeclaredCoordinateSystem, DeclaredUnits, DistanceKind, DistanceObservation, Face,
    GeocentricCoordinate, GeodeticCoordinate, GnssAntenna, GnssCovariance3,
    GnssGeocentricBaselineObservation, GnssGlobalPositionObservation, GnssSolution,
    HorizontalAngleObservation, HorizontalDirectionObservation, InstrumentSettings,
    LevelDifferenceObservation, LinearUnit, Observation, ObservationPrecision, Pointing,
    SourceRecord, VerticalAngleObservation, ZenithAngleObservation,
};

use crate::detect::{looks_like_text, probe_lines, without_byte_order_mark};
use crate::format::{
    FormatContent, FormatDescriptor, FormatRegistration, FormatSignature, Manufacturer, ProbeInput,
    ReadOptions, ReadResult, describe, ruled_out,
};
use crate::topcon_raw_builder::{
    METRES_PER_FOOT, METRES_PER_US_SURVEY_FOOT, RawProjectBuilder, face_of_zenith_reading,
    gons_to_radians, packed_degrees_to_radians, parse_real, wrap_to_circle, zenith_in_model_range,
};

const FORMAT_ID: &str = "tds-rw5";
// The international inch, exact by definition (EPSG:9002's foot over 12).
const METRES_PER_INCH: f64 = 0.0254;

// A field as the model holds text: UTF-8, any byte that is not replaced. The
// C++ keeps the bytes as they are; the Rust model holds validated UTF-8.
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

// std::string_view::substr(pos, count): `pos` past the end is the C++'s
// out_of_range, here a panic, which read_survey reports as the C++'s does.
fn substr(bytes: &[u8], pos: usize, count: usize) -> &[u8] {
    let end = pos.saturating_add(count).min(bytes.len());
    &bytes[pos..end]
}

// The first `byte` at or after `from` (from <= bytes.len()).
fn find(bytes: &[u8], byte: u8, from: usize) -> Option<usize> {
    bytes[from..]
        .iter()
        .position(|&c| c == byte)
        .map(|at| at + from)
}

// An optional field compared with a value, as the C++ compares an optional
// string_view with a literal: absent is equal to nothing.
fn is(value: Option<&[u8]>, expected: &[u8]) -> bool {
    value == Some(expected)
}

fn signature(confidence: f64, evidence: String) -> FormatSignature {
    FormatSignature {
        confidence,
        evidence,
    }
}

// ---- Records ----------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct Field<'a> {
    tag: &'a [u8],
    value: &'a [u8],
}

// One record: its type and its tagged fields. A fixed array, so reading a
// record allocates nothing; no record in the specifications has more than 16
// fields, and a record with more is read up to the cap with a warning.
#[derive(Default)]
struct Record<'a> {
    r#type: &'a [u8],
    fields: [Field<'a>; 16],
    count: usize,
    overflow: bool,
    // the text after "--", commas and all
    note: &'a [u8],
    has_note: bool,
    // The fields a handler has looked at, one bit each. A field no handler
    // asked for is one this reader does not know, and Rw5Reader::record says
    // so rather than let it vanish. Cells, as the C++'s are mutable: the
    // handlers take the record as read-only.
    looked: Cell<u32>,
    note_looked: Cell<bool>,
}

impl<'a> Record<'a> {
    fn find(&self, tag: &[u8]) -> Option<&'a [u8]> {
        for (i, field) in self.fields.iter().enumerate().take(self.count) {
            if field.tag == tag {
                self.looked.set(self.looked.get() | (1u32 << i));
                return Some(field.value);
            }
        }
        None
    }
}

fn is_upper(c: u8) -> bool {
    c.is_ascii_uppercase()
}

fn starts_number(c: u8) -> bool {
    c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' || c == b' '
}

// [TDS] record 30, "BL,DC%s,PN%s,DX%s,DY%s,DZ%s,--%s,GM%s,CL%s,HP%s,VP%s", is
// the one record whose note is followed by fields. One of those is its tag
// written straight after the comma and a number (GM and CL are enumerations,
// HP and VP distances), so a note's own ", HP 2" is not taken for one.
fn is_baseline_tail_field(piece: &[u8]) -> bool {
    if piece.len() < 2 {
        return false;
    }
    let tag = &piece[..2];
    if tag != b"GM" && tag != b"CL" && tag != b"HP" && tag != b"VP" {
        return false;
    }
    let value = trimmed(&piece[2..]);
    value.is_empty() || (piece[2] != b' ' && starts_number(value[0]))
}

// Where a BL record's note, starting at `note_start`, ends: before the run of
// tail fields that closes the line. Found from the end, so that a comma in
// the note stays in it ("--CP fence, north,GM4,CL1,..."), and in one pass
// over the line.
fn baseline_note_end(line: &[u8], note_start: usize) -> usize {
    let mut end = line.len();
    while end > note_start {
        let Some(comma) = line[..end].iter().rposition(|&c| c == b',') else {
            break;
        };
        if comma < note_start {
            break;
        }
        let piece = &line[comma + 1..end];
        if !trimmed(piece).is_empty() && !is_baseline_tail_field(piece) {
            break;
        }
        end = comma;
    }
    end
}

// Splits `line` into a Record. [TDS]: "Raw data records are comma delimited";
// each field starts with its two-letter header, except northing and easting,
// whose header is "N " and "E " ("the header is N space"); a note field "--"
// is free text and runs to the end of the line, except on a BL record.
fn split_record(line: &[u8]) -> Record<'_> {
    let mut record = Record::default();
    let mut cut = find(line, b',', 0);
    record.r#type = trimmed(match cut {
        Some(cut) => &line[..cut],
        None => line,
    });
    // The G0 record ([SCE]) is free text after its type: date, time, base id.
    if record.r#type == b"G0" {
        record.has_note = true;
        record.note = match cut {
            Some(cut) => &line[cut + 1..],
            None => &[],
        };
        return record;
    }
    while let Some(previous) = cut {
        let start = previous + 1;
        let rest = &line[start..];
        let leading = trimmed(rest);
        if leading.starts_with(b"--") && !record.has_note {
            record.has_note = true;
            let blanks = rest.iter().take_while(|&&c| is_ascii_space(c)).count();
            let note_start = start + blanks + 2;
            if record.r#type != b"BL" {
                record.note = trimmed(&line[note_start..]);
                return record;
            }
            let note_end = baseline_note_end(line, note_start);
            record.note = trimmed(&line[note_start..note_end]);
            cut = if note_end == line.len() {
                None
            } else {
                Some(note_end)
            };
            continue;
        }
        cut = find(line, b',', start);
        let field_text = trimmed(match cut {
            Some(next) => &line[start..next],
            None => &line[start..],
        });
        if field_text.is_empty() {
            continue;
        }
        let field = if field_text.len() >= 2
            && (field_text[0] == b'N' || field_text[0] == b'E')
            && starts_number(field_text[1])
        {
            Field {
                tag: &field_text[..1],
                value: trimmed(&field_text[1..]),
            }
        } else {
            Field {
                tag: substr(field_text, 0, 2),
                value: if field_text.len() > 2 {
                    trimmed(&field_text[2..])
                } else {
                    &[]
                },
            }
        };
        if record.count == record.fields.len() {
            record.overflow = true;
            continue;
        }
        record.fields[record.count] = field;
        record.count += 1;
    }
    record
}

// ---- The record types the specifications define ------------------------------

// Records defined by [TDS] or [SCE] that hold no observation or coordinate
// Katana imports, with the reason given in the warning.
struct NotImported {
    r#type: &'static [u8],
    why: &'static str,
}

const NOT_IMPORTED: [NotImported; 40] = [
    NotImported {
        r#type: b"CF",
        why: "a cut sheet (stake-out result) is not survey data Katana imports",
    },
    NotImported {
        r#type: b"DE",
        why: "a design point is a stake-out target, not a surveyed point",
    },
    NotImported {
        r#type: b"DL",
        why: "a defined location is a computed design point",
    },
    NotImported {
        r#type: b"MD",
        why: "a multiple-distance record names no target point",
    },
    NotImported {
        r#type: b"OE",
        why: "an offset delta is a stake-out result",
    },
    NotImported {
        r#type: b"RD",
        why: "a repeat-directional record names no target point",
    },
    NotImported {
        r#type: b"RE",
        why: "a remote elevation names no target point",
    },
    NotImported {
        r#type: b"RS",
        why: "a resection observation is not imported: its setup is not stated until the \
              resection is solved",
    },
    NotImported {
        r#type: b"SD",
        why: "a deltas record is a stake-out result",
    },
    NotImported {
        r#type: b"SL",
        why: "a slope-staking record is a stake-out result",
    },
    NotImported {
        r#type: b"SR",
        why: "a slope-staking reference record is a stake-out result",
    },
    NotImported {
        r#type: b"SU",
        why: "a sun observation is not imported",
    },
    NotImported {
        r#type: b"CG",
        why: "COGO settings are not survey data",
    },
    NotImported {
        r#type: b"CT",
        why: "a calibration residual is not survey data",
    },
    NotImported {
        r#type: b"DG",
        why: "a datum grid file name is not survey data",
    },
    NotImported {
        r#type: b"DT",
        why: "a datum definition is geodesy the import does not apply",
    },
    NotImported {
        r#type: b"EE",
        why: "a GPS edit flag is not survey data",
    },
    NotImported {
        r#type: b"ES",
        why: "an ellipsoid definition is geodesy the import does not apply",
    },
    NotImported {
        r#type: b"GK",
        why: "a GPS stake-out record is a stake-out result",
    },
    NotImported {
        r#type: b"GO",
        why: "a GPS offset shot needs a reduction the model cannot hold",
    },
    NotImported {
        r#type: b"GP",
        why: "a GPS point type is not survey data",
    },
    NotImported {
        r#type: b"HA",
        why: "a horizontal calibration is a transformation the import does not apply",
    },
    NotImported {
        r#type: b"PE",
        why: "a projection definition is geodesy the import does not apply",
    },
    NotImported {
        r#type: b"PJ",
        why: "a projection definition is geodesy the import does not apply",
    },
    NotImported {
        r#type: b"RP",
        why: "a calibration point's local coordinates are not imported",
    },
    NotImported {
        r#type: b"RX",
        why: "a receiver setup is not survey data",
    },
    NotImported {
        r#type: b"ST",
        why: "local site settings are geodesy the import does not apply",
    },
    NotImported {
        r#type: b"VA",
        why: "a vertical calibration is a transformation the import does not apply",
    },
    NotImported {
        r#type: b"AA",
        why: "a legacy accumulating-angle record is not imported",
    },
    NotImported {
        r#type: b"BB",
        why: "a legacy bench-level record is not imported",
    },
    NotImported {
        r#type: b"BG",
        why: "a legacy geoid record is not imported",
    },
    NotImported {
        r#type: b"BS",
        why: "a legacy bench-level record is not imported",
    },
    NotImported {
        r#type: b"BT",
        why: "a legacy bench-level record is not imported",
    },
    NotImported {
        r#type: b"HC",
        why: "a legacy control record is not imported",
    },
    NotImported {
        r#type: b"LE",
        why: "a legacy vertical setup record is not imported",
    },
    NotImported {
        r#type: b"LG",
        why: "a legacy geoid record is not imported",
    },
    NotImported {
        r#type: b"LM",
        why: "a legacy mapping-plane record is not imported",
    },
    NotImported {
        r#type: b"LH",
        why: "a legacy transformation record is not imported",
    },
    NotImported {
        r#type: b"LV",
        why: "a legacy transformation record is not imported",
    },
    NotImported {
        r#type: b"VC",
        why: "a legacy control record is not imported",
    },
];

// Every record type the reader or NOT_IMPORTED knows, for the probe.
const IMPORTED: [&[u8]; 30] = [
    b"JB", b"MO", b"OC", b"BK", b"LS", b"SS", b"TR", b"OB", b"BD", b"BR", b"FD", b"FR", b"SK",
    b"RB", b"RF", b"SP", b"AP", b"GS", b"GR", b"FC", b"DP", b"AT", b"CS", b"GPS", b"BP", b"BL",
    b"CV", b"EP", b"AH", b"OF",
];

// A GNSS vector record's type: G0 to G4 ([SCE]).
fn is_vector_type(r#type: &[u8]) -> bool {
    r#type.len() == 2 && r#type[0] == b'G' && (b'0'..=b'4').contains(&r#type[1])
}

fn is_known_type(r#type: &[u8]) -> bool {
    if r#type == b"EQ" || is_vector_type(r#type) {
        return true;
    }
    if IMPORTED.contains(&r#type) {
        return true;
    }
    NOT_IMPORTED.iter().any(|known| known.r#type == r#type)
}

// The field tags [TDS] and [SCE] define for each record type the reader
// imports, space separated. Every handler reads all of its record's; the list
// is what a warning about a field no handler read consults, so that it says
// truly whether the specifications define the field.
const SHOT_FIELDS: &str = "OP FP AZ BR AR AL DR DL ZE VA CE SD HD HR FE";

const DEFINED_FIELDS: [(&[u8], &str); 35] = [
    (b"JB", "NM DT TM"),
    (b"MO", "AD UN SF EC EO AU"),
    (b"OC", "OP N E EL"),
    (b"BK", "OP BP BS BC"),
    (b"LS", "HI HR"),
    (b"SS", SHOT_FIELDS),
    (b"TR", SHOT_FIELDS),
    (b"OB", SHOT_FIELDS),
    (b"BD", SHOT_FIELDS),
    (b"BR", SHOT_FIELDS),
    (b"FD", SHOT_FIELDS),
    (b"FR", SHOT_FIELDS),
    (b"SK", "OP FP AR ZE SD"),
    (b"RB", "OP BP AR ZE SD HR"),
    (b"RF", "OP FP AR ZE SD HR"),
    (b"OF", "AR ZE SD OL HD VD LR"),
    (b"SP", "PN N E EL"),
    (b"AP", "PN N E EL"),
    (b"GS", "PN N E EL"),
    (b"GR", "PN N E EL"),
    (b"FC", "PN FN"),
    (b"DP", "PN"),
    (b"AT", "TN TV"),
    (b"CS", "CO ZG ZN DN"),
    (b"GPS", "PN LA LN EL"),
    (b"BP", "PN LA LN HT SG EL AG PA AT SR"),
    (b"BL", "DC PN DX DY DZ GM CL HP VP"),
    (b"CV", "DC SV SC XX XY XZ YY YZ ZZ"),
    (b"EP", "TM LA LN HT RH RV DH DV GM CL"),
    (b"AH", "DC MA ME RA"),
    (b"EQ", "DC RX RS AN AI AT TS TA HO VO"),
    (b"G1", "BP PN DX DY DZ"),
    (b"G2", "VX VY VZ"),
    (b"G3", "XY XZ YZ"),
    (b"G4", "BV RV"),
];

fn is_defined_field(r#type: &[u8], tag: &[u8]) -> bool {
    match DEFINED_FIELDS
        .iter()
        .find(|(defined, _)| *defined == r#type)
    {
        Some((_, tags)) => tags
            .as_bytes()
            .split(|&c| c == b' ')
            .any(|known| known == tag),
        None => false,
    }
}

// std::from_chars into an int: an optional minus, then digits only, the whole
// of the text, in range. No plus, no blanks.
fn integer_of(value: &[u8]) -> Option<i32> {
    if value.first() == Some(&b'+') {
        return None;
    }
    std::str::from_utf8(value).ok()?.parse::<i32>().ok()
}

// The name [TDS]'s "GPS Enumerated Fields List" gives a value, numbered from
// `first`; a value it does not list is kept as written, and said to be one.
fn enumerated(value: &[u8], names: &[&str], first: i32) -> String {
    if let Some(number) = integer_of(value)
        && number >= first
        && ((number - first) as usize) < names.len()
    {
        return String::from(names[(number - first) as usize]);
    }
    format!("'{}', a value the specification does not list", text(value))
}

// CL, numbered from 0; DC and CO from 1.
const CLASSIFICATIONS: [&str; 11] = [
    "unknown",
    "normal",
    "control",
    "as-built",
    "check",
    "backsight",
    "deleted normal",
    "deleted control",
    "deleted as-built",
    "deleted check",
    "deleted backsight",
];
const DERIVATIONS: [&str; 4] = ["base", "rover", "get base", "static"];
const COORDINATE_SYSTEM_OPTIONS: [&str; 4] =
    ["none", "scale only", "keyed in", "chosen from library"];

// Where a GNSS vector ends on an antenna: [SCE] G4's "APC Antenna Phase
// Center (L1)", "ARP Antenna Reference Point (Bottom of Antenna Mount)" and
// "UNK Unknown".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AntennaPoint {
    // [SCE250]: "phase center to phase center" unless G4 says
    #[default]
    PhaseCentre,
    ReferencePoint,
    NotStated,
}

// ---- Reading ---------------------------------------------------------------------

// The vector's ends and what was in force at its G1: an LS or BP record
// between the G1 and the next vector does not belong to this one.
#[derive(Clone, Debug, Default)]
struct VectorEnds {
    base: AntennaPoint,
    rover: AntennaPoint,
    // the G4 that named them, else the G1
    record: usize,
    base_antenna: GnssAntenna,
    base_reference_height: Option<f64>,
    base_phase_offset: Option<f64>,
    rover_antenna: GnssAntenna,
    rod_height: Option<f64>,
}

// Fields no handler reads, by "<record type> <tag>": the record of the first
// and how many records carried one. Warned once at the first and summed at
// the end, so a collector that writes an unknown field on every shot gives two
// warnings, not one per shot.
struct Unread {
    first: usize,
    records: usize,
}

// What one shot observes, for the observations it makes.
struct Shot<'a> {
    at: &'a str,
    to: &'a str,
    hi: f64,
    th: f64,
    pointing: Pointing,
    precision: ObservationPrecision,
}

const CURRENT: &str = "a setup is current: station_at found or began one";

struct Rw5Reader {
    builder: RawProjectBuilder,
    fatal: Option<Error>,

    // What the last MO record said.
    mode_seen: bool,
    metres_per_unit: f64,
    gons: bool,
    south_azimuths: bool,
    settings: InstrumentSettings,

    instrument_height: f64,
    instrument_height_stated: bool,
    rod_height: f64,
    rod_height_stated: bool,
    last_point: String,

    base_point: String,
    base_antenna: GnssAntenna,
    rover_antenna: GnssAntenna,
    // an AH record gave the rover's height
    rover_antenna_stated: bool,
    // The last BP record's AG (reference point to ground) and PA (reference
    // point to phase centre), metres, kept apart because a vector from that
    // base may start at either point (G4 BV), not only where the BP's own EL
    // was (its AT).
    base_reference_height: Option<f64>,
    base_phase_offset: Option<f64>,

    vector: Option<GnssGeocentricBaselineObservation>,
    vector_record: usize,
    vector_note: String,
    vector_ends: VectorEnds,
    // index in project().observations
    last_gnss: Option<usize>,

    unread: BTreeMap<String, Unread>,
}

impl Rw5Reader {
    fn new(file_name: &str, options: &ReadOptions) -> Self {
        Self {
            builder: RawProjectBuilder::new(
                file_name,
                SourceRecord {
                    manufacturer: String::from("Carlson"),
                    format: String::from("RW5"),
                    format_version: String::from("TDS3.6/SCE3.03"),
                    file_name: String::from(file_name),
                    record_number: 0,
                },
                options,
            ),
            fatal: None,
            mode_seen: false,
            metres_per_unit: 0.0,
            gons: false,
            south_azimuths: false,
            settings: InstrumentSettings::default(),
            instrument_height: 0.0,
            instrument_height_stated: false,
            rod_height: 0.0,
            rod_height_stated: false,
            last_point: String::new(),
            base_point: String::new(),
            base_antenna: GnssAntenna::default(),
            rover_antenna: GnssAntenna::default(),
            rover_antenna_stated: false,
            base_reference_height: None,
            base_phase_offset: None,
            vector: None,
            vector_record: 0,
            vector_note: String::new(),
            vector_ends: VectorEnds::default(),
            last_gnss: None,
            unread: BTreeMap::new(),
        }
    }

    // Values in the file's units, converted; a failure is a warning naming the
    // field, and None. Linear values need a stated unit: before the first MO
    // record there is none, and that fails the file (fatal).
    fn real(&mut self, value: Option<&[u8]>, tag: &str, n: usize) -> Option<f64> {
        let value = value?;
        let parsed = parse_real(value);
        if parsed.is_none() {
            self.builder
                .warn(n, format!("{tag} '{}' is not a number", text(value)));
        }
        parsed
    }

    fn linear(&mut self, value: Option<&[u8]>, tag: &str, n: usize) -> Option<f64> {
        value?;
        if !self.mode_seen {
            if self.fatal.is_none() {
                self.fatal = Some(make_error(
                    ErrorCode::FileImportFailure,
                    format!(
                        "record {n} of {} gives a distance or coordinate before any MO (mode) \
                         record has said whether the file is in feet, US survey feet or metres, \
                         and Katana will not guess",
                        self.builder.file_name()
                    ),
                    "RW5 [TDS] section 3: the mode setup record states the units",
                ));
            }
            return None;
        }
        let parsed = self.real(value, tag, n)?;
        Some(parsed * self.metres_per_unit)
    }

    fn angle(&mut self, value: Option<&[u8]>, tag: &str, n: usize) -> Option<f64> {
        let value = value?;
        if !self.mode_seen {
            if self.fatal.is_none() {
                self.fatal = Some(make_error(
                    ErrorCode::FileImportFailure,
                    format!(
                        "record {n} of {} gives an angle before any MO (mode) record has said \
                         whether angles are in degrees or grads, and Katana will not guess",
                        self.builder.file_name()
                    ),
                    "RW5 [TDS] section 3: the mode setup record states the units",
                ));
            }
            return None;
        }
        let parsed = if self.gons {
            gons_to_radians(value)
        } else {
            packed_degrees_to_radians(value)
        };
        if parsed.is_none() {
            self.builder.warn(
                n,
                format!(
                    "{tag} '{}' is not an angle in {}",
                    text(value),
                    if self.gons {
                        "grads"
                    } else {
                        "degrees (DDD.MMSS)"
                    }
                ),
            );
        }
        parsed
    }

    fn latitude_or_longitude(&mut self, value: Option<&[u8]>, tag: &str, n: usize) -> Option<f64> {
        let value = value?;
        // [TDS] field lists: "LA: Latitude ... Geodetic Angle (DMS)", whatever
        // the MO angle unit; [SCE]: "Lat(dd.mmss) Lon(dd.mmss - Negative for
        // West)".
        let parsed = packed_degrees_to_radians(value);
        if parsed.is_none() {
            self.builder.warn(
                n,
                format!(
                    "{tag} '{}' is not a latitude or longitude in DD.MMSS",
                    text(value)
                ),
            );
        }
        parsed
    }

    fn read(mut self, bytes: &[u8]) -> Result<ReadResult> {
        let mut n = 0;
        // UTF-8 byte order mark: a file saved by a Windows editor carries one.
        let mut start = if bytes.starts_with(b"\xEF\xBB\xBF") {
            3
        } else {
            0
        };
        while start < bytes.len() && self.fatal.is_none() {
            let end = find(bytes, b'\n', start).unwrap_or(bytes.len());
            let mut line = &bytes[start..end];
            start = end + 1;
            n += 1;
            if let Some(without) = line.strip_suffix(b"\r") {
                line = without;
            }
            self.record(line, n);
        }
        if let Some(fatal) = self.fatal.take() {
            return Err(fatal);
        }
        self.finish_vector();
        self.report_unread_fields();
        if !self.builder.project().stations.is_empty() {
            if !self.instrument_height_stated {
                self.builder.not_carried(String::from(
                    "no instrument heights (no LS record states HI)",
                ));
            }
            if !self.rod_height_stated {
                self.builder
                    .not_carried(String::from("no target heights (no LS record states HR)"));
            }
            self.builder.not_carried(String::from(
                "no atmospheric settings: the RW5 format has no field for temperature, pressure \
                 or ppm, so whether the distances are corrected is unknown",
            ));
            self.builder
                .not_carried(String::from("no instrument make, model or serial number"));
        }
        let result = self.builder.finish();
        let project = &result.project;
        if project.stations.is_empty()
            && project.observations.is_empty()
            && project.points.is_empty()
            && project.unpositioned_points.is_empty()
        {
            // Nothing but warnings: say so rather than import an empty job
            // quietly.
            return Err(make_error(
                ErrorCode::FileImportFailure,
                format!(
                    "{} holds no RW5 setup, observation or point Katana could read",
                    project.source.file_name
                ),
                format!(
                    "{} warning(s){}",
                    result.warnings.len(),
                    match result.warnings.first() {
                        Some(warning) => format!(", the first: {}", describe(warning)),
                        None => String::new(),
                    }
                ),
            ));
        }
        Ok(result)
    }

    fn record(&mut self, line: &[u8], n: usize) {
        let line = trimmed(line);
        if line.is_empty() {
            return;
        }
        // [TDS] 1: the note record, a line that is only a comment.
        if line.starts_with(b"--") {
            self.builder.add_station_note(&text(trimmed(&line[2..])));
            self.builder.count_read();
            return;
        }
        let r = split_record(line);
        if r.r#type.is_empty() || r.r#type.len() > 3 || !is_upper(r.r#type[0]) {
            self.builder.skip(
                n,
                format!("'{}' is not an RW5 record", text(substr(line, 0, 40))),
            );
            return;
        }
        if r.overflow {
            self.builder.warn(
                n,
                String::from(
                    "record has more fields than any RW5 record defines; the extra fields were \
                     not read",
                ),
            );
        }
        let skipped_before = self.builder.records_skipped();
        self.dispatch(&r, n);
        // A record skipped as a whole has already been warned about; one that
        // was read must not lose a field quietly.
        if self.fatal.is_none() && self.builder.records_skipped() == skipped_before {
            self.unread_fields(&r, n);
        }
    }

    fn dispatch(&mut self, r: &Record<'_>, n: usize) {
        let kind = r.r#type;
        if kind == b"MO" {
            self.mode(r, n);
        } else if kind == b"JB" {
            self.job(r, n);
        } else if kind == b"OC" {
            self.occupy(r, n);
        } else if kind == b"BK" {
            self.backsight(r, n);
        } else if kind == b"LS" {
            self.line_of_sight(r, n);
        } else if kind == b"SS"
            || kind == b"TR"
            || kind == b"OB"
            || kind == b"BD"
            || kind == b"BR"
            || kind == b"FD"
            || kind == b"FR"
            || kind == b"SK"
            || kind == b"RB"
            || kind == b"RF"
        {
            self.shot(r, n);
        } else if kind == b"SP" {
            self.store_point(r, n, CoordinateSource::Unknown);
        } else if kind == b"AP" || kind == b"GS" || kind == b"GR" {
            // Adjusted points and GPS-derived local coordinates: computed by
            // the data collector, not entered or observed.
            self.store_point(r, n, CoordinateSource::Calculated);
        } else if kind == b"FC" {
            let Some(pn) = r.find(b"PN").filter(|pn| !pn.is_empty()) else {
                self.builder
                    .skip(n, String::from("feature code record names no point"));
                return;
            };
            let code = r.find(b"FN").unwrap_or_default();
            self.builder.code_point(&text(pn), &text(code), "", "", n);
            self.builder.count_read();
        } else if kind == b"DP" {
            let Some(pn) = r.find(b"PN").filter(|pn| !pn.is_empty()) else {
                self.builder
                    .skip(n, String::from("deleted-point record names no point"));
                return;
            };
            let pn = text(pn);
            self.builder.add_point_metadata(
                &pn,
                String::from("deleted in the field"),
                format!("record {n}"),
            );
            self.builder.warn(
                n,
                format!(
                    "point '{pn}' was deleted in the field; it is imported with a note in its \
                     metadata, because its observations are still in the file"
                ),
            );
            self.builder.count_read();
        } else if kind == b"AT" {
            if self.last_point.is_empty() {
                self.builder
                    .skip(n, String::from("attribute record before any point"));
                return;
            }
            let name = text(r.find(b"TN").unwrap_or_default());
            let value = text(r.find(b"TV").unwrap_or_default());
            self.builder
                .add_point_metadata(&self.last_point, format!("attribute {name}"), value);
            self.builder.count_read();
        } else if kind == b"CS" {
            let mut name = text(r.find(b"ZG").unwrap_or_default());
            let zone = text(r.find(b"ZN").unwrap_or_default());
            let datum = text(r.find(b"DN").unwrap_or_default());
            if !zone.is_empty() {
                if name.is_empty() {
                    name = zone;
                } else {
                    name = format!("{name} / {zone}");
                }
            }
            if !datum.is_empty() {
                if name.is_empty() {
                    name = datum;
                } else {
                    name = format!("{name} ({datum})");
                }
            }
            if !name.is_empty() {
                self.builder.project().coordinate_system = DeclaredCoordinateSystem::named(name);
            }
            // [TDS] CO: how the system was set up (1 none ... 4 from the
            // library).
            if let Some(option) = r.find(b"CO") {
                self.builder.project().metadata.insert(
                    String::from("coordinate system option"),
                    enumerated(option, &COORDINATE_SYSTEM_OPTIONS, 1),
                );
            }
            self.builder.count_read();
        } else if kind == b"GPS" {
            self.gnss_position(r, n);
        } else if kind == b"BP" {
            self.base_position(r, n);
        } else if is_vector_type(kind) {
            self.vector_record(r, n);
        } else if kind == b"BL" {
            self.baseline(r, n);
        } else if kind == b"CV" {
            self.covariance(r, n);
        } else if kind == b"EP" {
            self.geodetic_position(r, n);
        } else if kind == b"AH" {
            self.antenna_height(r, n);
        } else if kind == b"EQ" {
            self.equipment(r, n);
        } else if kind == b"OF" {
            self.off_centre(r, n);
        } else if let Some(known) = NOT_IMPORTED.iter().find(|known| known.r#type == kind) {
            self.builder.skip(
                n,
                format!("{} record not imported: {}", text(kind), known.why),
            );
        } else {
            self.builder.skip(
                n,
                format!(
                    "'{}' is not a record type the RW5 specification defines",
                    text(kind)
                ),
            );
        }
    }

    fn mode(&mut self, r: &Record<'_>, n: usize) {
        // [TDS] enumerations: UN 0 feet, 1 metre, 2 US survey feet; AU 0
        // degrees, 1 grads; AD 0 north, 1 south; EC 0 off, 1 on.
        let un = r.find(b"UN");
        let unit = if is(un, b"0") {
            self.metres_per_unit = METRES_PER_FOOT;
            LinearUnit::Feet
        } else if is(un, b"1") {
            self.metres_per_unit = 1.0;
            LinearUnit::Metres
        } else if is(un, b"2") {
            self.metres_per_unit = METRES_PER_US_SURVEY_FOOT;
            LinearUnit::UsSurveyFeet
        } else {
            self.fatal = Some(make_error(
                ErrorCode::FileImportFailure,
                format!(
                    "record {n} of {} sets the distance unit to '{}', which is not one the RW5 \
                     specification defines (0 feet, 1 metres, 2 US survey feet)",
                    self.builder.file_name(),
                    un.map_or_else(|| String::from("(none)"), text)
                ),
                "RW5 [TDS] enumerated fields: UN",
            ));
            return;
        };
        let au = r.find(b"AU");
        let mut angular = AngularUnit::DegreesMinutesSeconds;
        if au.is_none() || is(au, b"0") {
            self.gons = false;
            if au.is_none() {
                // The mode record of [SCE] 2006 lists no AU; the angle unit
                // field came later and degrees is what a file without it was
                // written in.
                self.builder.warn(
                    n,
                    String::from(
                        "the mode record states no angle unit (AU); angles are read as degrees \
                         (DDD.MMSS)",
                    ),
                );
            }
        } else if is(au, b"1") {
            self.gons = true;
            angular = AngularUnit::Gons;
        } else {
            self.fatal = Some(make_error(
                ErrorCode::FileImportFailure,
                format!(
                    "record {n} of {} sets the angle unit to '{}', which is not one the RW5 \
                     specification defines (0 degrees, 1 grads)",
                    self.builder.file_name(),
                    au.map(text).unwrap_or_default()
                ),
                "RW5 [TDS] enumerated fields: AU",
            ));
            return;
        }
        let units = self.builder.project().units;
        if self.mode_seen && (units.linear != unit || units.angular != angular) {
            self.builder.warn(
                n,
                String::from(
                    "the units change here; values after this record are converted with the new \
                     units",
                ),
            );
        }
        if !self.mode_seen {
            self.builder.project().units = DeclaredUnits {
                linear: unit,
                angular,
            };
        }
        self.mode_seen = true;
        self.south_azimuths = is(r.find(b"AD"), b"1");
        if let Some(scale) = self.real(r.find(b"SF"), "SF", n) {
            self.settings.scale_factor = Some(scale);
            // [CSP]
            self.settings.scale_factor_state = CorrectionState::NotApplied;
        }
        if let Some(curvature) = r.find(b"EC") {
            // [CSP]: the collector's curvature setting adjusts the points it
            // CALCULATES; a raw zenith or slope distance never contains it, a
            // recorded HD or CE may when it was on.
            let off = curvature == b"0";
            self.settings.curvature_refraction_state = if off {
                CorrectionState::NotApplied
            } else {
                CorrectionState::Unknown
            };
            self.builder.project().metadata.insert(
                String::from("earth curvature (collector setting)"),
                String::from(if off { "off" } else { "on" }),
            );
        }
        if let Some(offset) = self.real(r.find(b"EO"), "EO", n) {
            self.settings.prism_constant = Some(offset * METRES_PER_INCH);
            self.settings.prism_constant_state = CorrectionState::Unknown;
        }
        self.builder.count_read();
    }

    fn job(&mut self, r: &Record<'_>, _: usize) {
        if let Some(name) = r.find(b"NM") {
            self.builder.project().name = text(name);
        }
        if let Some(date) = r.find(b"DT") {
            self.builder
                .project()
                .metadata
                .insert(String::from("job date"), text(date));
        }
        if let Some(time) = r.find(b"TM") {
            self.builder
                .project()
                .metadata
                .insert(String::from("job time"), text(time));
        }
        self.builder.count_read();
    }

    fn describe_point(&mut self, id: &str, r: &Record<'_>, n: usize) {
        self.last_point = String::from(id);
        r.note_looked.set(true);
        if !r.has_note || r.note.is_empty() {
            return;
        }
        let code = match r.note.iter().position(|&c| c == b' ' || c == b'\t') {
            Some(space) => &r.note[..space],
            None => r.note,
        };
        self.builder
            .code_point(id, &text(code), &text(r.note), "", n);
    }

    fn occupy(&mut self, r: &Record<'_>, n: usize) {
        let Some(op) = r.find(b"OP").filter(|op| !op.is_empty()) else {
            self.builder
                .skip(n, String::from("occupy record names no point"));
            return;
        };
        let northing = self.linear(r.find(b"N"), "N", n);
        let easting = self.linear(r.find(b"E"), "E", n);
        let elevation = self.linear(r.find(b"EL"), "EL", n);
        if self.fatal.is_some() {
            return;
        }
        let op = text(op);
        self.builder
            .begin_station(&op, self.instrument_height, &self.settings, n);
        match (northing, easting) {
            (Some(northing), Some(easting)) => self.builder.position_point(
                &op,
                northing,
                easting,
                elevation,
                CoordinateSource::Unknown,
                n,
                "",
            ),
            (None, None) => {}
            _ => self.builder.warn(
                n,
                format!(
                    "occupied point '{op}' has only one of northing and easting; it is imported \
                     without coordinates"
                ),
            ),
        }
        self.describe_point(&op, r, n);
        self.builder.count_read();
    }

    // Whether there is a setup for a record from `op` now: the current one
    // when it occupies `op` (or the record names none), or a new one there.
    fn station_at(&mut self, op: Option<&[u8]>, n: usize, kind: &str) -> bool {
        if let Some(occupied) = self
            .builder
            .current_station()
            .map(|station| station.setup.point_id.clone())
            && op.is_none_or(|op| op.is_empty() || text(op) == occupied)
        {
            return true;
        }
        let Some(op) = op.filter(|op| !op.is_empty()) else {
            return false;
        };
        let op = text(op);
        self.builder.warn(
            n,
            format!(
                "{kind} record from '{op}' with no occupy record for it; a setup there is started \
                 without coordinates"
            ),
        );
        self.builder
            .begin_station(&op, self.instrument_height, &self.settings, n);
        true
    }

    fn backsight(&mut self, r: &Record<'_>, n: usize) {
        if !self.station_at(r.find(b"OP"), n, "backsight") {
            self.builder.skip(
                n,
                String::from("backsight record before any setup and naming no occupied point"),
            );
            return;
        }
        let back_circle = self.angle(r.find(b"BC"), "BC", n);
        let back_azimuth = self.angle(r.find(b"BS"), "BS", n);
        if self.fatal.is_some() {
            return;
        }
        // A backsight taken again after shots re-orients the circle: the shots
        // after it are read against a different zero, so they are a new setup.
        let station = self.builder.current_station().expect(CURRENT);
        if !station.observations.is_empty() {
            let point = station.setup.point_id.clone();
            self.builder
                .begin_station(&point, self.instrument_height, &self.settings, n);
        }
        if let Some(bp) = r.find(b"BP").filter(|bp| !bp.is_empty()) {
            let bp = text(bp);
            let occupied = self
                .builder
                .current_station()
                .expect(CURRENT)
                .setup
                .point_id
                .clone();
            if bp == occupied {
                self.builder.warn(
                    n,
                    String::from(
                        "the backsight point is the occupied point; it is not recorded as the \
                         backsight",
                    ),
                );
            } else {
                self.builder.mention_point(&bp, n);
                self.builder
                    .current_station()
                    .expect(CURRENT)
                    .backsight_point_id = bp;
            }
        }
        let south = self.south_azimuths;
        let station = self.builder.current_station().expect(CURRENT);
        if let Some(circle) = back_circle {
            station.backsight_azimuth = Some(wrap_to_circle(circle));
        }
        if let Some(azimuth) = back_azimuth {
            let azimuth = wrap_to_circle(azimuth + if south { PI } else { 0.0 });
            station.metadata.insert(
                String::from("backsight azimuth (radians)"),
                format_exact_real(azimuth),
            );
        }
        self.builder.count_read();
    }

    fn line_of_sight(&mut self, r: &Record<'_>, n: usize) {
        let hi = self.linear(r.find(b"HI"), "HI", n);
        let hr = self.linear(r.find(b"HR"), "HR", n);
        if let Some(hi) = hi {
            self.instrument_height = hi;
            self.instrument_height_stated = true;
            let mut changed_in = None;
            if let Some(station) = self.builder.current_station() {
                if station.observations.is_empty() {
                    station.setup.instrument_height = hi;
                } else if station.setup.instrument_height != hi {
                    changed_in = Some(station.setup.id.clone());
                }
            }
            if let Some(setup) = changed_in {
                self.builder.warn(
                    n,
                    format!(
                        "the instrument height changes in the middle of setup '{setup}'; each \
                         observation carries the height it was taken with"
                    ),
                );
            }
        }
        if let Some(hr) = hr {
            self.rod_height = hr;
            self.rod_height_stated = true;
        }
        if self.fatal.is_none() {
            self.builder.count_read();
        }
    }

    // [SCE]: a shot records ONE of each group. A record with two says two
    // things; the first in the specification's order is read and the rest
    // named, never dropped without a word.
    fn one_of(&mut self, n: usize, group: &[(&str, bool)]) {
        let mut used = "";
        let mut extra = String::new();
        for &(tag, present) in group {
            if !present {
                continue;
            }
            if used.is_empty() {
                used = tag;
            } else {
                if !extra.is_empty() {
                    extra.push_str(", ");
                }
                extra.push_str(tag);
            }
        }
        if !extra.is_empty() {
            self.builder.warn(
                n,
                format!(
                    "the record gives {used} and also {extra}, where the specification allows \
                     one; {used} is read and {extra} is not imported"
                ),
            );
        }
    }

    fn shot(&mut self, r: &Record<'_>, n: usize) {
        let kind = r.r#type;
        let repeat_backsight = kind == b"RB";
        let target = r.find(if repeat_backsight { b"BP" } else { b"FP" });
        let Some(target) = target.filter(|target| !target.is_empty()) else {
            self.builder.skip(
                n,
                format!(
                    "{} record names no {} point",
                    text(kind),
                    if repeat_backsight {
                        "backsight"
                    } else {
                        "foresight"
                    }
                ),
            );
            return;
        };
        if !self.station_at(r.find(b"OP"), n, &text(kind)) {
            self.builder
                .skip(n, format!("{} record before any setup", text(kind)));
            return;
        }
        let to = text(target);
        let at = self
            .builder
            .current_station()
            .expect(CURRENT)
            .setup
            .point_id
            .clone();
        if to == at {
            self.builder
                .skip(n, format!("a shot from '{to}' to itself"));
            return;
        }
        // Every value first, so a unit failure leaves nothing half recorded.
        let target_height = self.linear(r.find(b"HR"), "HR", n);
        let ar = self.angle(r.find(b"AR"), "AR", n);
        let al = self.angle(r.find(b"AL"), "AL", n);
        let az = self.angle(r.find(b"AZ"), "AZ", n);
        let dr = self.angle(r.find(b"DR"), "DR", n);
        let dl = self.angle(r.find(b"DL"), "DL", n);
        let ze = self.angle(r.find(b"ZE"), "ZE", n);
        let va = self.angle(r.find(b"VA"), "VA", n);
        let ce = self.linear(r.find(b"CE"), "CE", n);
        let sd = self.linear(r.find(b"SD"), "SD", n);
        let hd = self.linear(r.find(b"HD"), "HD", n);
        let bearing = r.find(b"BR");
        // [SCE] lists FE, foresight elevation, among the field headers without
        // saying which record carries it: the elevation the collector computed
        // for the target, kept with the point.
        let fe = self.linear(r.find(b"FE"), "FE", n);
        if self.fatal.is_some() {
            return;
        }
        self.one_of(
            n,
            &[
                ("AR", ar.is_some()),
                ("AL", al.is_some()),
                ("AZ", az.is_some()),
                ("BR", bearing.is_some()),
                ("DR", dr.is_some()),
                ("DL", dl.is_some()),
            ],
        );
        self.one_of(
            n,
            &[
                ("ZE", ze.is_some()),
                ("VA", va.is_some()),
                ("CE", ce.is_some()),
            ],
        );
        // CE is a height difference and HD its horizontal partner: the pair is
        // one choice of the vertical group, so only SD with HD is a conflict.
        self.one_of(
            n,
            &[("SD", sd.is_some()), ("HD", hd.is_some() && ce.is_none())],
        );
        let hi = self.instrument_height;
        let th = target_height.unwrap_or(self.rod_height);
        let precision = self.builder.options().precision;
        // Nothing below begins a setup, so the station and its id stay put.
        self.builder.mention_point(&to, n);

        // The face of this pointing: the record type says so for a set, the
        // zenith reading does for a single shot.
        let mut face = Face::Unknown;
        if kind == b"BD" || kind == b"FD" {
            face = Face::Left;
        } else if kind == b"BR" || kind == b"FR" {
            face = Face::Right;
        }
        let mut zenith = None;
        if let Some(reading) = ze {
            if reading < 0.0 || reading >= TWO_PI {
                self.builder.warn(
                    n,
                    String::from(
                        "zenith reading outside a full circle; the zenith is not imported",
                    ),
                );
            } else {
                let read = face_of_zenith_reading(reading);
                if face == Face::Unknown {
                    face = read;
                } else if read != Face::Unknown && read != face {
                    self.builder.warn(
                        n,
                        format!(
                            "{} is a {}-face record but its zenith reading is a {}-face one",
                            text(kind),
                            data_model::to_string(face),
                            data_model::to_string(read)
                        ),
                    );
                }
                zenith = Some(zenith_in_model_range(reading));
            }
        }
        let pointing = Pointing {
            index: self.builder.next_pointing(),
            face,
        };
        let shot = Shot {
            at: &at,
            to: &to,
            hi,
            th,
            pointing,
            precision,
        };

        let any_horizontal = ar.is_some()
            || al.is_some()
            || az.is_some()
            || bearing.is_some()
            || dr.is_some()
            || dl.is_some();
        let mut horizontal = false;
        if ar.is_some() || al.is_some() {
            // Angle right is the clockwise circle reading (see the top of this
            // file); angle left the anticlockwise one.
            let direction = match (ar, al) {
                (Some(right), _) => wrap_to_circle(right),
                (None, Some(left)) => wrap_to_circle(-left),
                (None, None) => 0.0,
            };
            let observation = self
                .builder
                .station_observation::<HorizontalDirectionObservation>(n);
            observation.at = at.clone();
            observation.to = to.clone();
            observation.direction = direction;
            observation.sigma = precision.direction;
            observation.pointing = pointing;
            horizontal = true;
        } else if az.is_some() || bearing.is_some() {
            let mut azimuth = None;
            if let Some(az) = az {
                azimuth = Some(wrap_to_circle(
                    az + if self.south_azimuths { PI } else { 0.0 },
                ));
            } else if let Some(bearing) = bearing {
                // [SCE]: "BR Bearing (this field will be recorded as
                // N123.4500W)": a quadrant letter, the angle, a quadrant letter.
                if bearing.len() >= 3
                    && (bearing[0] == b'N' || bearing[0] == b'S')
                    && (bearing[bearing.len() - 1] == b'E' || bearing[bearing.len() - 1] == b'W')
                {
                    let angle_text = &bearing[1..bearing.len() - 1];
                    let angle = if self.gons {
                        gons_to_radians(angle_text)
                    } else {
                        packed_degrees_to_radians(angle_text)
                    };
                    if let Some(angle) = angle
                        && angle <= PI / 2.0
                    {
                        let north = bearing[0] == b'N';
                        let east = bearing[bearing.len() - 1] == b'E';
                        azimuth = Some(wrap_to_circle(if north {
                            if east { angle } else { -angle }
                        } else if east {
                            PI - angle
                        } else {
                            PI + angle
                        }));
                    }
                }
                if azimuth.is_none() {
                    self.builder.warn(
                        n,
                        format!(
                            "bearing '{}' is not a quadrant bearing; the horizontal angle is not \
                             imported",
                            text(bearing)
                        ),
                    );
                }
            }
            if let Some(azimuth) = azimuth {
                let observation = self.builder.station_observation::<AzimuthObservation>(n);
                observation.from = at.clone();
                observation.to = to.clone();
                observation.azimuth = azimuth;
                observation.sigma = precision.direction;
                horizontal = true;
            }
        } else if dr.is_some() || dl.is_some() {
            // A deflection is turned from the prolongation of the backsight line.
            let backsight = self
                .builder
                .current_station()
                .expect(CURRENT)
                .backsight_point_id
                .clone();
            if backsight.is_empty() || backsight == to {
                self.builder.warn(
                    n,
                    String::from(
                        "a deflection angle needs the setup's backsight point, which is not \
                         stated; the horizontal angle is not imported",
                    ),
                );
            } else {
                let angle = match (dr, dl) {
                    (Some(right), _) => wrap_to_circle(PI + right),
                    (None, Some(left)) => wrap_to_circle(PI - left),
                    (None, None) => 0.0,
                };
                let observation = self
                    .builder
                    .station_observation::<HorizontalAngleObservation>(n);
                observation.at = at.clone();
                observation.from = backsight;
                observation.to = to.clone();
                observation.angle = angle;
                observation.sigma = precision.direction;
                observation.pointing = pointing;
                horizontal = true;
            }
        }
        if !horizontal && !any_horizontal {
            self.builder
                .warn(n, String::from("the shot states no horizontal angle"));
        }

        if let Some(zenith) = zenith {
            let observation = self
                .builder
                .station_observation::<ZenithAngleObservation>(n);
            observation.from = at.clone();
            observation.to = to.clone();
            observation.angle = zenith;
            observation.sigma = precision.zenith;
            observation.instrument_height = hi;
            observation.target_height = th;
            observation.pointing = pointing;
        } else if let Some(vertical) = va {
            if vertical.abs() > PI / 2.0 {
                self.builder.warn(
                    n,
                    String::from("vertical angle beyond 90 degrees; it is not imported"),
                );
            } else {
                let observation = self
                    .builder
                    .station_observation::<VerticalAngleObservation>(n);
                observation.from = at.clone();
                observation.to = to.clone();
                observation.angle = vertical;
                observation.sigma = precision.zenith;
                observation.instrument_height = hi;
                observation.target_height = th;
                observation.pointing = pointing;
            }
        } else if let Some(change) = ce {
            // Line of sight to ground: the instrument above its mark, the
            // target above its own.
            let height_difference = change + hi - th;
            if height_difference.is_finite() {
                let difference = self
                    .builder
                    .station_observation::<LevelDifferenceObservation>(n);
                difference.from = at.clone();
                difference.to = to.clone();
                difference.height_difference = height_difference;
                difference.sigma = precision
                    .height_measurement
                    .hypot(precision.height_measurement);
                difference.length = match hd {
                    Some(flat) if flat > 0.0 => flat,
                    _ => 0.0,
                };
            } else {
                self.builder.warn(
                    n,
                    String::from(
                        "the change in elevation and heights do not add up to a finite height \
                         difference; it is not imported",
                    ),
                );
            }
        }

        if let Some(slope) = sd {
            self.distance(n, &shot, slope, DistanceKind::Slope, "SD");
        } else if let Some(flat) = hd {
            self.distance(n, &shot, flat, DistanceKind::Horizontal, "HD");
        }
        if kind == b"TR" {
            self.builder.add_point_metadata(
                &to,
                String::from("traverse foresight"),
                format!("record {n}"),
            );
        }
        if let Some(elevation) = fe {
            self.builder.add_point_metadata(
                &to,
                String::from("foresight elevation (collector, m)"),
                format_exact_real(elevation),
            );
        }
        self.describe_point(&to, r, n);
        self.builder.count_read();
    }

    fn distance(&mut self, n: usize, shot: &Shot<'_>, value: f64, kind: DistanceKind, tag: &str) {
        if !(value > 0.0) {
            self.builder.warn(
                n,
                format!(
                    "{tag} of {} m read as no distance measured",
                    format_exact_real(value)
                ),
            );
            return;
        }
        let observation = self.builder.station_observation::<DistanceObservation>(n);
        observation.from = String::from(shot.at);
        observation.to = String::from(shot.to);
        observation.distance = value;
        observation.sigma = distance_sigma(&shot.precision, value);
        observation.kind = kind;
        observation.instrument_height = shot.hi;
        observation.target_height = shot.th;
        observation.pointing = shot.pointing;
        // The EDM offset in force for THIS shot: an MO record in the middle of
        // a setup changes it for the shots after it, which the setup's
        // InstrumentSettings (taken when the setup began) cannot say.
        observation.target.prism_constant = self.settings.prism_constant;
        observation.target.prism_constant_state = self.settings.prism_constant_state;
    }

    fn store_point(&mut self, r: &Record<'_>, n: usize, how: CoordinateSource) {
        let Some(pn) = r.find(b"PN").filter(|pn| !pn.is_empty()) else {
            self.builder
                .skip(n, format!("{} record names no point", text(r.r#type)));
            return;
        };
        let northing = self.linear(r.find(b"N"), "N", n);
        let easting = self.linear(r.find(b"E"), "E", n);
        let elevation = self.linear(r.find(b"EL"), "EL", n);
        if self.fatal.is_some() {
            return;
        }
        let pn = text(pn);
        if let (Some(northing), Some(easting)) = (northing, easting) {
            self.builder
                .position_point(&pn, northing, easting, elevation, how, n, "");
        } else {
            self.builder.mention_point(&pn, n);
            self.builder.warn(
                n,
                format!(
                    "point '{pn}' is stored without both northing and easting; it is imported \
                     without coordinates"
                ),
            );
        }
        self.describe_point(&pn, r, n);
        self.builder.count_read();
    }

    fn gnss_position(&mut self, r: &Record<'_>, n: usize) {
        let Some(pn) = r.find(b"PN").filter(|pn| !pn.is_empty()) else {
            self.builder
                .skip(n, String::from("GPS record names no point"));
            return;
        };
        let latitude = self.latitude_or_longitude(r.find(b"LA"), "LA", n);
        let longitude = self.latitude_or_longitude(r.find(b"LN"), "LN", n);
        // [SCE]: "EL Ellipsoid Height (meters)" - metres whatever the MO unit.
        let height = self.real(r.find(b"EL"), "EL", n);
        let Some((latitude, longitude, height)) = readable(latitude, longitude, height) else {
            self.builder.skip(
                n,
                String::from(
                    "GPS record without a readable latitude, longitude and ellipsoid height",
                ),
            );
            return;
        };
        let pn = text(pn);
        self.builder.mention_point(&pn, n);
        let position = GnssGlobalPositionObservation {
            point: pn.clone(),
            geodetic: Some(GeodeticCoordinate {
                latitude,
                longitude,
                ellipsoidal_height: height,
            }),
            reference_frame: String::from("WGS 84"),
            // [SCE] 3.03: "GPS heights always to be recorded to phase center";
            // the rod height in force is that antenna height.
            antenna: GnssAntenna {
                height: self.rod_height,
                method: AntennaHeightMethod::PhaseCentre,
                measured_to: String::from("phase centre (RW5 GPS record)"),
                ..Default::default()
            },
            source: self.builder.source(n),
            ..Default::default()
        };
        self.builder.add_loose_observation(position.into());
        self.last_gnss = Some(self.builder.project().observations.len() - 1);
        self.builder.not_carried(String::from(
            "whether a GPS record's ellipsoid height is of the mark or of the antenna is not \
             stated; the rod height in force is recorded as the antenna height to the phase \
             centre",
        ));
        self.describe_point(&pn, r, n);
        self.builder.count_read();
    }

    fn base_position(&mut self, r: &Record<'_>, n: usize) {
        let Some(pn) = r.find(b"PN").filter(|pn| !pn.is_empty()) else {
            self.builder
                .skip(n, String::from("base position record names no point"));
            return;
        };
        let latitude = self.latitude_or_longitude(r.find(b"LA"), "LA", n);
        let longitude = self.latitude_or_longitude(r.find(b"LN"), "LN", n);
        // [SCE] writes EL in metres; [TDS] writes HT "(See MO for units)".
        let ht = r.find(b"HT");
        let height = if r.find(b"EL").is_some() {
            let height = self.real(r.find(b"EL"), "EL", n);
            if let Some(ht) = ht {
                self.builder.warn(
                    n,
                    format!(
                        "the base record gives both EL and HT; EL is read and HT ('{}') is not \
                         imported",
                        text(ht)
                    ),
                );
            }
            height
        } else {
            self.linear(ht, "HT", n)
        };
        if self.fatal.is_some() {
            return;
        }
        let pn = text(pn);
        self.base_point = pn.clone();
        self.builder.mention_point(&pn, n);
        // [TDS] SG: the setup group, an identifier with no survey meaning here.
        if let Some(group) = r.find(b"SG") {
            self.builder
                .add_point_metadata(&pn, String::from("setup group"), text(group));
        }
        // [SCE] BP: AG "Antenna distance from ARP (bottom of antenna) to
        // Ground", PA "Phase Center to ARP", both metres ([SCE250]). A new base
        // replaces both, stated or not.
        let ag = self.real(r.find(b"AG"), "AG", n);
        let pa = self.real(r.find(b"PA"), "PA", n);
        self.base_reference_height = ag;
        self.base_phase_offset = pa;
        // The antenna where the BP's own EL is: [SCE] BP AT, APC, ARP or UNK.
        let mut antenna = self.base_antenna.clone();
        let at = r.find(b"AT");
        if let Some(ag) = ag {
            if is(at, b"APC") {
                antenna.height = ag + pa.unwrap_or(0.0);
                antenna.method = AntennaHeightMethod::PhaseCentre;
                antenna.measured_to = String::from("L1 phase centre");
            } else if is(at, b"ARP") {
                antenna.height = ag;
                antenna.method = AntennaHeightMethod::Vertical;
                antenna.measured_to = String::from("antenna reference point");
            } else {
                antenna.height = ag;
                antenna.method = AntennaHeightMethod::Unknown;
                self.builder.warn(
                    n,
                    String::from(
                        "the base record does not say whether its height is at the phase centre \
                         or the antenna reference point",
                    ),
                );
            }
        }
        if let Some(source) = r.find(b"SR") {
            self.builder.add_point_metadata(
                &pn,
                String::from("base position source"),
                text(source),
            );
        }
        if let Some(pa) = pa {
            self.builder.add_point_metadata(
                &pn,
                String::from("antenna phase centre offset (m)"),
                format_exact_real(pa),
            );
        }
        // A Survey Pro baseline (BL) from this base starts at the same
        // antenna; a SurvCE vector (G1) at the point its G4 names (base_end).
        self.base_antenna = antenna.clone();
        let Some((latitude, longitude, height)) = readable(latitude, longitude, height) else {
            self.builder.warn(
                n,
                String::from(
                    "base position without a readable latitude, longitude and height; the base \
                     point is imported without a position",
                ),
            );
            self.describe_point(&pn, r, n);
            self.builder.count_read();
            return;
        };
        let position = GnssGlobalPositionObservation {
            point: pn.clone(),
            geodetic: Some(GeodeticCoordinate {
                latitude,
                longitude,
                ellipsoidal_height: height,
            }),
            reference_frame: String::from("WGS 84"),
            antenna,
            source: self.builder.source(n),
            ..Default::default()
        };
        self.builder.add_loose_observation(position.into());
        self.last_gnss = Some(self.builder.project().observations.len() - 1);
        self.describe_point(&pn, r, n);
        self.builder.count_read();
    }

    fn finish_vector(&mut self) {
        let Some(mut vector) = self.vector.take() else {
            return;
        };
        if !self.vector_note.is_empty() {
            self.builder.add_point_metadata(
                &vector.to,
                String::from("GNSS vector note"),
                self.vector_note.clone(),
            );
        }
        let c = vector.covariance;
        if c.stated() && (!(c.xx > 0.0) || !(c.yy > 0.0) || !(c.zz > 0.0)) {
            self.builder.warn(
                self.vector_record,
                String::from(
                    "the vector's variances are not all positive; its covariance is not imported",
                ),
            );
            vector.covariance = GnssCovariance3::default();
        }
        // Only now: the G4 that names the ends comes after the G1.
        vector.from_antenna = self.base_end();
        vector.to_antenna = self.rover_end();
        self.builder.add_loose_observation(vector.into());
        self.last_gnss = Some(self.builder.project().observations.len() - 1);
        self.vector_note.clear();
    }

    // The antenna at the base end of the vector being assembled.
    fn base_end(&mut self) -> GnssAntenna {
        let ends = &self.vector_ends;
        let mut antenna = ends.base_antenna.clone();
        let Some(ag) = ends.base_reference_height else {
            // No BP record gave AG: the base antenna is whatever an AH record
            // said, as it said it.
            return antenna;
        };
        antenna.height = ag;
        match ends.base {
            AntennaPoint::PhaseCentre => {
                if let Some(pa) = ends.base_phase_offset {
                    // [SCE250]: AG and PA "add ... together to get the Phase
                    // Center to Ground value".
                    antenna.height = ag + pa;
                    antenna.method = AntennaHeightMethod::PhaseCentre;
                    antenna.measured_to = String::from("L1: AG + PA of the base record");
                } else {
                    antenna.method = AntennaHeightMethod::Other;
                    antenna.measured_to = String::from(
                        "antenna reference point (AG of the base record); the vector starts at \
                         the phase centre, and no PA gives the offset",
                    );
                    self.builder.warn(
                        ends.record,
                        String::from(
                            "the vector starts at the base's phase centre, but the base record \
                             gives no PA (phase centre to reference point); its height is to the \
                             reference point and the vector's height difference cannot be taken \
                             to the marks",
                        ),
                    );
                }
            }
            AntennaPoint::ReferencePoint => {
                antenna.method = AntennaHeightMethod::Vertical;
                antenna.measured_to = String::from("AG of the base record");
            }
            AntennaPoint::NotStated => {
                antenna.method = AntennaHeightMethod::Unknown;
                antenna.measured_to = String::from(
                    "antenna reference point (AG of the base record); the G4 record does not say \
                     where the vector starts (BV UNK)",
                );
            }
        }
        antenna
    }

    // The antenna at the rover end of the vector being assembled.
    fn rover_end(&mut self) -> GnssAntenna {
        let ends = &self.vector_ends;
        let mut antenna = ends.rover_antenna.clone();
        let Some(rod_height) = ends.rod_height else {
            // an AH record's, or nothing (warned at the G1)
            return antenna;
        };
        // [SCE250]: "The LS,HR value is from phase center to the ground".
        antenna.height = rod_height;
        match ends.rover {
            AntennaPoint::PhaseCentre => {
                antenna.method = AntennaHeightMethod::PhaseCentre;
                antenna.measured_to = String::from("LS HR, phase centre to ground");
            }
            AntennaPoint::ReferencePoint => {
                // The height is not the one this end needs, and no record gives
                // the rover's phase centre offset to make it so: not passed off
                // as one.
                antenna.method = AntennaHeightMethod::Other;
                antenna.measured_to = String::from(
                    "phase centre (LS HR); the vector ends at the antenna reference point (G4 RV \
                     ARP), an offset from it no record gives",
                );
                self.builder.warn(
                    ends.record,
                    String::from(
                        "the vector ends at the rover's antenna reference point (RV ARP), but \
                         SurvCE records the rover's rod height (LS HR) to the phase centre, and \
                         no record gives the offset between them; the vector's height difference \
                         cannot be taken to the marks",
                    ),
                );
            }
            AntennaPoint::NotStated => {
                antenna.method = AntennaHeightMethod::Unknown;
                antenna.measured_to = String::from(
                    "phase centre (LS HR); the G4 record does not say where the vector ends (RV \
                     UNK)",
                );
            }
        }
        antenna
    }

    // [SCE] G4: one end's antenna point; None where the record does not name
    // one, or names one that is not APC, ARP or UNK (said).
    fn antenna_point(&mut self, point: Option<&[u8]>, tag: &str, n: usize) -> Option<AntennaPoint> {
        let point = point?;
        if point == b"APC" {
            return Some(AntennaPoint::PhaseCentre);
        }
        if point == b"ARP" {
            return Some(AntennaPoint::ReferencePoint);
        }
        if point == b"UNK" {
            return Some(AntennaPoint::NotStated);
        }
        self.builder.warn(
            n,
            format!(
                "{tag} '{}' is not APC, ARP or UNK; that end of the vector is taken to be the \
                 phase centre",
                text(point)
            ),
        );
        None
    }

    fn vector_record(&mut self, r: &Record<'_>, n: usize) {
        // [SCE] G0 date, time and base id; G1 base, rover and the vector; G2
        // the variances; G3 the covariances; G4 the antenna points.
        let which = r.r#type[1];
        if which == b'0' {
            self.finish_vector();
            r.note_looked.set(true);
            self.vector_note = text(trimmed(r.note));
            self.builder.count_read();
            return;
        }
        if which == b'1' {
            let note = self.vector_note.clone();
            self.finish_vector();
            self.vector_note = note;
            let from = r.find(b"BP");
            let to = r.find(b"PN");
            // [SCE250]: "ALL THE VALUES ARE ALWAYS IN METERS", whatever MO
            // says - unlike a Survey Pro BL, whose components are in the MO
            // unit.
            let dx = self.real(r.find(b"DX"), "DX", n);
            let dy = self.real(r.find(b"DY"), "DY", n);
            let dz = self.real(r.find(b"DZ"), "DZ", n);
            let vector = match (from, to, dx, dy, dz) {
                (Some(from), Some(to), Some(x), Some(y), Some(z))
                    if !from.is_empty() && !to.is_empty() && text(from) != text(to) =>
                {
                    Some((text(from), text(to), GeocentricCoordinate { x, y, z }))
                }
                _ => None,
            };
            let Some((from, to, delta)) = vector else {
                self.builder.skip(
                    n,
                    String::from(
                        "GNSS vector without two distinct points and all three components",
                    ),
                );
                self.vector_note.clear();
                return;
            };
            self.builder.mention_point(&from, n);
            self.builder.mention_point(&to, n);
            self.vector = Some(GnssGeocentricBaselineObservation {
                from,
                to,
                delta,
                reference_frame: String::from("WGS 84"),
                source: self.builder.source(n),
                ..Default::default()
            });
            self.vector_record = n;
            self.vector_ends = VectorEnds {
                record: n,
                base_antenna: self.base_antenna.clone(),
                base_reference_height: self.base_reference_height,
                base_phase_offset: self.base_phase_offset,
                rover_antenna: self.rover_antenna.clone(),
                ..Default::default()
            };
            if self.rod_height_stated {
                self.vector_ends.rod_height = Some(self.rod_height);
            } else if !self.rover_antenna_stated {
                // [SCE250]: the rover's height is "the LS record prior to the
                // vector records"; with none, the rover end's height is
                // unknown, not zero.
                self.vector_ends.rover_antenna.method = AntennaHeightMethod::Other;
                self.vector_ends.rover_antenna.measured_to = String::from(
                    "not stated: no LS record before the vector gives the rover's rod height",
                );
                self.builder.warn(
                    n,
                    String::from(
                        "no LS record before this vector gives the rover's rod height, so the \
                         vector's height difference cannot be taken to the marks",
                    ),
                );
            }
            self.builder.count_read();
            return;
        }
        if self.vector.is_none() {
            self.builder.skip(
                n,
                format!("{} record with no G1 vector before it", text(r.r#type)),
            );
            return;
        }
        if which == b'2' {
            // Variances in square metres ([TDS] field list: "for GPS position
            // (m2)").
            let xx = self.real(r.find(b"VX"), "VX", n).unwrap_or(0.0);
            let yy = self.real(r.find(b"VY"), "VY", n).unwrap_or(0.0);
            let zz = self.real(r.find(b"VZ"), "VZ", n).unwrap_or(0.0);
            if let Some(vector) = self.vector.as_mut() {
                vector.covariance.xx = xx;
                vector.covariance.yy = yy;
                vector.covariance.zz = zz;
            }
        } else if which == b'3' {
            let xy = self.real(r.find(b"XY"), "XY", n).unwrap_or(0.0);
            let xz = self.real(r.find(b"XZ"), "XZ", n).unwrap_or(0.0);
            let yz = self.real(r.find(b"YZ"), "YZ", n).unwrap_or(0.0);
            if let Some(vector) = self.vector.as_mut() {
                vector.covariance.xy = xy;
                vector.covariance.xz = xz;
                vector.covariance.yz = yz;
            }
        } else {
            // [SCE] G4: "Antenna point in Base, Antenna point in Rover"; the
            // heights follow in finish_vector.
            if let Some(base) = self.antenna_point(r.find(b"BV"), "BV", n) {
                self.vector_ends.base = base;
            }
            if let Some(rover) = self.antenna_point(r.find(b"RV"), "RV", n) {
                self.vector_ends.rover = rover;
            }
            self.vector_ends.record = n;
        }
        self.builder.count_read();
    }

    fn baseline(&mut self, r: &Record<'_>, n: usize) {
        let to = r.find(b"PN");
        let dx = self.linear(r.find(b"DX"), "DX", n);
        let dy = self.linear(r.find(b"DY"), "DY", n);
        let dz = self.linear(r.find(b"DZ"), "DZ", n);
        if self.fatal.is_some() {
            return;
        }
        let baseline = match (to, dx, dy, dz) {
            (Some(to), Some(x), Some(y), Some(z))
                if !self.base_point.is_empty() && !to.is_empty() && text(to) != self.base_point =>
            {
                Some((text(to), GeocentricCoordinate { x, y, z }))
            }
            _ => None,
        };
        let Some((to, delta)) = baseline else {
            self.builder.skip(
                n,
                String::from(
                    "GPS baseline without a base position before it, a rover point and all three \
                     components",
                ),
            );
            return;
        };
        self.builder.mention_point(&to, n);
        let vector = GnssGeocentricBaselineObservation {
            from: self.base_point.clone(),
            to: to.clone(),
            delta,
            reference_frame: String::from("WGS 84"),
            from_antenna: self.base_antenna.clone(),
            to_antenna: self.rover_antenna.clone(),
            solution: solution_of(r.find(b"GM")),
            source: self.builder.source(n),
            ..Default::default()
        };
        self.builder.add_loose_observation(vector.into());
        self.last_gnss = Some(self.builder.project().observations.len() - 1);
        if let Some(hp) = self.linear(r.find(b"HP"), "HP", n) {
            self.builder.add_point_metadata(
                &to,
                String::from("horizontal precision (m)"),
                format_exact_real(hp),
            );
        }
        if let Some(vp) = self.linear(r.find(b"VP"), "VP", n) {
            self.builder.add_point_metadata(
                &to,
                String::from("vertical precision (m)"),
                format_exact_real(vp),
            );
        }
        // [TDS] DC: how the baseline was derived (1 base, 2 rover, 3 get base,
        // 4 static); CL: the point's classification (1 normal, 2 control ...).
        if let Some(dc) = r.find(b"DC") {
            self.builder.add_point_metadata(
                &to,
                String::from("GNSS derivation"),
                enumerated(dc, &DERIVATIONS, 1),
            );
        }
        if let Some(cl) = r.find(b"CL") {
            self.builder.add_point_metadata(
                &to,
                String::from("classification"),
                enumerated(cl, &CLASSIFICATIONS, 0),
            );
        }
        self.describe_point(&to, r, n);
        self.builder.count_read();
    }

    fn covariance(&mut self, r: &Record<'_>, n: usize) {
        let Some(last_gnss) = self.last_gnss else {
            self.builder.skip(
                n,
                String::from("covariance record with no GPS position or baseline before it"),
            );
            return;
        };
        let c = GnssCovariance3 {
            xx: self.real(r.find(b"XX"), "XX", n).unwrap_or(0.0),
            yy: self.real(r.find(b"YY"), "YY", n).unwrap_or(0.0),
            zz: self.real(r.find(b"ZZ"), "ZZ", n).unwrap_or(0.0),
            xy: self.real(r.find(b"XY"), "XY", n).unwrap_or(0.0),
            xz: self.real(r.find(b"XZ"), "XZ", n).unwrap_or(0.0),
            yz: self.real(r.find(b"YZ"), "YZ", n).unwrap_or(0.0),
        };
        if !(c.xx > 0.0) || !(c.yy > 0.0) || !(c.zz > 0.0) {
            self.builder.skip(
                n,
                String::from("covariance record whose variances are not all positive"),
            );
            return;
        }
        let (point, of_position) = match &mut self.builder.project().observations[last_gnss] {
            Observation::GnssGeocentricBaseline(vector) => {
                vector.covariance = c;
                (vector.to.clone(), false)
            }
            Observation::GnssGlobalPosition(position) => (position.point.clone(), true),
            _ => (String::new(), false),
        };
        if of_position {
            // An X/Y/Z covariance cannot be the north/east/up one a geodetic
            // position carries (data_model.rs); it is kept as text.
            self.builder.add_point_metadata(
                &point,
                String::from("covariance XX YY ZZ XY XZ YZ (m2)"),
                format!(
                    "{} {} {} {} {} {}",
                    format_exact_real(c.xx),
                    format_exact_real(c.yy),
                    format_exact_real(c.zz),
                    format_exact_real(c.xy),
                    format_exact_real(c.xz),
                    format_exact_real(c.yz)
                ),
            );
        }
        // [TDS] CV: DC the derivation, SV "Minimum number of SV during
        // observation", SC "Error Scale" - kept with the point the covariance
        // belongs to (last_gnss is always a vector or a position, so one
        // exists).
        if let Some(dc) = r.find(b"DC")
            && !point.is_empty()
        {
            self.builder.add_point_metadata(
                &point,
                String::from("covariance derivation"),
                enumerated(dc, &DERIVATIONS, 1),
            );
        }
        if let Some(sv) = r.find(b"SV")
            && !point.is_empty()
        {
            self.builder
                .add_point_metadata(&point, String::from("minimum satellites"), text(sv));
        }
        if let Some(sc) = r.find(b"SC")
            && !point.is_empty()
        {
            self.builder.add_point_metadata(
                &point,
                String::from("covariance error scale"),
                text(sc),
            );
            // The specification does not say whether the variances already
            // hold the scale, so they are imported as written; a scale other
            // than one is worth a surveyor's look.
            if let Some(scale) = self.real(Some(sc), "SC", n)
                && scale != 1.0
            {
                self.builder.warn(
                    n,
                    format!(
                        "the covariance's error scale SC is {}; the variances are imported as \
                         written, not scaled, because the specification does not say whether \
                         they include it",
                        text(sc)
                    ),
                );
            }
        }
        self.builder.count_read();
    }

    fn geodetic_position(&mut self, r: &Record<'_>, n: usize) {
        // [TDS] 39: "When a point is stored, its geodetic position is
        // recorded" - the record names no point; it belongs to the one stored
        // before it.
        if self.last_point.is_empty() {
            self.builder.skip(
                n,
                String::from("geodetic position with no stored point before it"),
            );
            return;
        }
        let latitude = self.latitude_or_longitude(r.find(b"LA"), "LA", n);
        let longitude = self.latitude_or_longitude(r.find(b"LN"), "LN", n);
        let height = self.linear(r.find(b"HT"), "HT", n);
        if self.fatal.is_some() {
            return;
        }
        let Some((latitude, longitude, height)) = readable(latitude, longitude, height) else {
            self.builder.skip(
                n,
                String::from("geodetic position without a readable latitude, longitude and height"),
            );
            return;
        };
        let position = GnssGlobalPositionObservation {
            point: self.last_point.clone(),
            geodetic: Some(GeodeticCoordinate {
                latitude,
                longitude,
                ellipsoidal_height: height,
            }),
            antenna: self.rover_antenna.clone(),
            solution: solution_of(r.find(b"GM")),
            source: self.builder.source(n),
            ..Default::default()
        };
        self.builder.add_loose_observation(position.into());
        self.last_gnss = Some(self.builder.project().observations.len() - 1);
        for tag in ["RH", "RV", "DH", "DV"] {
            if let Some(value) = r.find(tag.as_bytes()) {
                self.builder.add_point_metadata(
                    &self.last_point,
                    format!("receiver {tag}"),
                    text(value),
                );
            }
        }
        // [TDS] EP TM "System Time" (HH:MM:SS) and CL the classification.
        if let Some(time) = r.find(b"TM") {
            self.builder.add_point_metadata(
                &self.last_point,
                String::from("GNSS time"),
                text(time),
            );
        }
        if let Some(cl) = r.find(b"CL") {
            self.builder.add_point_metadata(
                &self.last_point,
                String::from("classification"),
                enumerated(cl, &CLASSIFICATIONS, 0),
            );
        }
        self.builder.count_read();
    }

    fn antenna_height(&mut self, r: &Record<'_>, n: usize) {
        // [TDS] AH: DC derivation (1 base, 2 rover ...), MA measured height,
        // ME method (0 unknown, 1 true, 2 uncorrected), RA reduced to phase
        // centre.
        let measured = self.linear(r.find(b"MA"), "MA", n);
        let reduced = self.linear(r.find(b"RA"), "RA", n);
        if self.fatal.is_some() {
            return;
        }
        let base = is(r.find(b"DC"), b"1");
        if !base && (measured.is_some() || reduced.is_some()) {
            self.rover_antenna_stated = true;
        }
        let antenna = if base {
            &mut self.base_antenna
        } else {
            &mut self.rover_antenna
        };
        if let Some(measured) = measured {
            antenna.height = measured;
            let method = r.find(b"ME");
            antenna.method = if is(method, b"1") {
                AntennaHeightMethod::Vertical
            } else if is(method, b"2") {
                AntennaHeightMethod::Slant
            } else {
                AntennaHeightMethod::Unknown
            };
            antenna.measured_to = String::from(if is(method, b"2") {
                "uncorrected (slant) measurement"
            } else {
                ""
            });
        } else if let Some(reduced) = reduced {
            antenna.height = reduced;
            antenna.method = AntennaHeightMethod::PhaseCentre;
            antenna.measured_to = String::from("reduced to the phase centre by the data collector");
        }
        if let (Some(_), Some(reduced)) = (measured, reduced) {
            self.builder.project().metadata.insert(
                format!("antenna height to phase centre, record {n}"),
                format_exact_real(reduced),
            );
        }
        self.builder.count_read();
    }

    fn equipment(&mut self, r: &Record<'_>, n: usize) {
        // [TDS] EQ: TA "Tape Adjustment", HO "Horizontal Offset" and VO
        // "Vertical Offset" are the antenna's measuring geometry, in the MO
        // unit. An AH record's RA already has them applied; the raw values are
        // kept.
        let tape = self.linear(r.find(b"TA"), "TA", n);
        let horizontal = self.linear(r.find(b"HO"), "HO", n);
        let vertical = self.linear(r.find(b"VO"), "VO", n);
        if self.fatal.is_some() {
            return;
        }
        let antenna = if is(r.find(b"DC"), b"1") {
            &mut self.base_antenna
        } else {
            &mut self.rover_antenna
        };
        if let Some(kind) = r.find(b"AT") {
            antenna.r#type = text(kind);
        }
        if let Some(serial) = r.find(b"TS") {
            antenna.serial_number = text(serial);
        }
        let role = if is(r.find(b"DC"), b"1") {
            "base"
        } else {
            "rover"
        };
        let metadata = &mut self.builder.project().metadata;
        if let Some(receiver) = r.find(b"RX") {
            metadata.insert(format!("{role} receiver"), text(receiver));
        }
        if let Some(serial) = r.find(b"RS") {
            metadata.insert(format!("{role} receiver serial"), text(serial));
        }
        // AN and AI index the collector's own antenna table (Antenna.ini).
        if let Some(number) = r.find(b"AN") {
            metadata.insert(format!("{role} antenna number"), text(number));
        }
        if let Some(index) = r.find(b"AI") {
            metadata.insert(format!("{role} antenna index"), text(index));
        }
        if let Some(tape) = tape {
            metadata.insert(
                format!("{role} antenna tape adjustment (m)"),
                format_exact_real(tape),
            );
        }
        if let Some(horizontal) = horizontal {
            metadata.insert(
                format!("{role} antenna horizontal offset (m)"),
                format_exact_real(horizontal),
            );
        }
        if let Some(vertical) = vertical {
            metadata.insert(
                format!("{role} antenna vertical offset (m)"),
                format_exact_real(vertical),
            );
        }
        self.builder.count_read();
    }

    fn off_centre(&mut self, r: &Record<'_>, n: usize) {
        // [SCE] OF: the angle right, the actual zenith and the slope distance
        // of an off-centre shot (a tree, a pole); [TDS] 16 adds the offset
        // length OL, the horizontal and vertical distances HD and VD and the
        // left/right offset LR. The specifications do not say which shot it
        // corrects, so it is not tied to one: every field is kept, as written
        // and in the order written, with the setup it was observed from.
        let Some(station) = self.builder.current_station() else {
            self.builder
                .skip(n, String::from("off-centre shot before any setup"));
            return;
        };
        let mut values = String::new();
        for field in r.fields.iter().take(r.count) {
            if !values.is_empty() {
                values.push_str(", ");
            }
            values.push_str(&text(field.tag));
            values.push(' ');
            values.push_str(&text(field.value));
        }
        r.looked.set(u32::MAX);
        r.note_looked.set(true);
        if r.has_note && !r.note.is_empty() {
            values.push_str(if values.is_empty() {
                "note "
            } else {
                ", note "
            });
            values.push_str(&text(r.note));
        }
        station
            .metadata
            .insert(format!("off-centre shot, record {n} (file units)"), values);
        let setup = station.setup.id.clone();
        self.builder.warn(
            n,
            format!(
                "the off-centre shot is kept in setup '{setup}' metadata but not applied: the \
                 specification does not say which shot it corrects"
            ),
        );
        self.builder.count_read();
    }

    fn unread_fields(&mut self, r: &Record<'_>, n: usize) {
        if r.has_note && !r.note_looked.get() && !r.note.is_empty() {
            // A note on a record whose handler has no use for one (a
            // backsight, a mode record): kept with the setup like a note line.
            self.builder
                .add_station_note(&format!("{} note: {}", text(r.r#type), text(r.note)));
        }
        let all = if r.count >= 32 {
            u32::MAX
        } else {
            (1u32 << r.count) - 1
        };
        if (r.looked.get() & all) == all {
            return;
        }
        for (i, field) in r.fields.iter().enumerate().take(r.count) {
            if (r.looked.get() & (1u32 << i)) != 0 {
                continue;
            }
            let tag = field.tag;
            let value = text(substr(field.value, 0, 40));
            // Record::find reads the first of a tag given twice.
            let repeated = r.fields[..i].iter().any(|earlier| earlier.tag == tag);
            if repeated {
                self.builder.warn(
                    n,
                    format!(
                        "field '{}' is given more than once; the first is read and this one \
                         (value '{value}') is not imported",
                        text(tag)
                    ),
                );
                continue;
            }
            if is_defined_field(r.r#type, tag) {
                // Every handler reads its record's fields, so this is a field
                // it passed over here; said on every record, with its value.
                self.builder.warn(
                    n,
                    format!(
                        "field '{}' (value '{value}') is one the RW5 specifications define for a \
                         {} record, but it is not imported here",
                        text(tag),
                        text(r.r#type)
                    ),
                );
                continue;
            }
            let kind = text(r.r#type);
            match self.unread.entry(format!("{kind} {}", text(tag))) {
                Entry::Occupied(mut entry) => entry.get_mut().records += 1,
                Entry::Vacant(entry) => {
                    entry.insert(Unread {
                        first: n,
                        records: 1,
                    });
                    self.builder.warn(
                        n,
                        format!(
                            "field '{}' (value '{value}') is not one the RW5 specifications \
                             define for a {kind} record; it is not imported, here or on any \
                             later {kind} record",
                            text(tag)
                        ),
                    );
                }
            }
        }
    }

    fn report_unread_fields(&mut self) {
        for (key, unread) in &self.unread {
            if unread.records > 1 {
                let space = key.find(' ').unwrap_or(key.len());
                self.builder.warn(
                    0,
                    format!(
                        "the '{}' field of {} records was not imported on {} records, the first \
                         record {}",
                        key.get(space + 1..).unwrap_or_default(),
                        &key[..space],
                        unread.records,
                        unread.first
                    ),
                );
            }
        }
    }
}

// A latitude, longitude and height that can be a position: all three read,
// and the latitude within the poles.
fn readable(
    latitude: Option<f64>,
    longitude: Option<f64>,
    height: Option<f64>,
) -> Option<(f64, f64, f64)> {
    match (latitude, longitude, height) {
        (Some(latitude), Some(longitude), Some(height)) if !(latitude.abs() > PI / 2.0) => {
            Some((latitude, longitude, height))
        }
        _ => None,
    }
}

fn solution_of(gm: Option<&[u8]>) -> GnssSolution {
    // [TDS] GM: 2 Autonomous, 3 RTKFloat, 4 RTKFixed, 6 RTCMCode, 7 WAAS.
    if is(gm, b"2") {
        return GnssSolution::Autonomous;
    }
    if is(gm, b"3") {
        return GnssSolution::Float;
    }
    if is(gm, b"4") {
        return GnssSolution::Fixed;
    }
    if is(gm, b"6") || is(gm, b"7") {
        return GnssSolution::Differential;
    }
    GnssSolution::Unknown
}

// ---- Registration ---------------------------------------------------------------

fn probe_rw5(input: &ProbeInput<'_>) -> FormatSignature {
    let bytes = without_byte_order_mark(input.bytes);
    if !looks_like_text(bytes) {
        return ruled_out();
    }
    let mut lines = 0usize;
    let mut records = 0usize;
    let mut tps = 0usize;
    for line in probe_lines(input, 200) {
        let line = trimmed(line);
        if line.is_empty() {
            continue;
        }
        lines += 1;
        if line.starts_with(b"--") {
            records += 1;
            continue;
        }
        let Some(comma) = find(line, b',', 0) else {
            continue;
        };
        let kind = &line[..comma];
        if is_known_type(kind) {
            records += 1;
            if kind == b"MO"
                || kind == b"OC"
                || kind == b"BK"
                || kind == b"SS"
                || kind == b"LS"
                || kind == b"TR"
                || kind == b"SP"
                || kind == b"GPS"
            {
                tps += 1;
            }
        }
    }
    if lines == 0 || records == 0 {
        return ruled_out();
    }
    let share = records as f64 / lines as f64;
    let extension = input.extension == "rw5";
    let evidence = format!("{records} of {lines} lines are RW5 records");
    if share < 0.8 || tps == 0 {
        return if extension {
            signature(0.4, format!("extension .rw5 but {evidence}"))
        } else {
            ruled_out()
        };
    }
    if extension {
        return signature(0.97, format!("extension .rw5 and {evidence}"));
    }
    signature(0.85, format!("{evidence} (MO, OC, BK, LS, SS ...)"))
}

fn descriptor() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(FORMAT_ID),
        human_name: String::from("RW5 raw data (TDS Survey Pro, Carlson SurvCE, Topcon Magnet)"),
        manufacturer: Manufacturer::Carlson,
        reads: FormatContent {
            points: true,
            observations: true,
            stations: true,
            features: true,
            coordinate_system: true,
            instrument_settings: true,
            gnss: true,
        },
        can_import: true,
        can_export: false,
        parser_version: String::from("1.0"),
        extensions: vec![String::from("rw5")],
    }
}

fn read_rw5(bytes: &[u8], file_name: &str, options: &ReadOptions) -> Result<ReadResult> {
    Rw5Reader::new(file_name, options).read(bytes)
}

pub(crate) fn registration() -> FormatRegistration {
    FormatRegistration::new(descriptor(), probe_rw5, Some(read_rw5))
}
