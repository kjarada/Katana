//! Topcon GTS raw data: GTS-7 read, GTS-6 recognised and refused.
//!
//! The port of src/katana_surveyio/topcon_gts.cpp, which has no header of its
//! own: the two formats reach the program through their registrations alone.

// Specification this reader implements, and nothing beyond it:
//   [LINK] Topcon Positioning Systems, "Topcon Link Reference Manual", P/N
//          7010-0522, Appendix C "Sample File Formats": "GTS-7 Raw Format"
//          (pages C-6 to C-9) - the record list (JOB, DATE, NAME, INST, UNITS,
//          SCALE, ATMOS, STN, XYZ, BKB, BS, FS, SS, CTL, HV, SD, HD, OFFSET,
//          NOTE), their fields and their order rules, and a worked sample.
//
// GTS-6 is NOT read. [LINK] says of it only "See the GTS-6 interface manual
// for details" and prints a sample; that manual is not public, and reading a
// field layout off one sample would be inventing it. A GTS-6 file is
// recognised so that it can be refused by name, saying what to export instead.
//
// Readings of [LINK] that are this reader's own, stated so they can be checked:
//   * Each record is a control word, white space, then comma-separated fields.
//     The sample's first line ("TTools v1.0") is not a control word; a first
//     line that is not one is kept as the project's "header" metadata.
//   * XYZ is NORTHING, easting, elevation, although the record list labels it
//     "X(easting), Y(northing)". The manual's own sample settles it: from
//     station MARK (10, 10) the backsight ST1 is written "XYZ 13.85600,
//     7.04700" and observed at circle reading -37 26' 44" (= 322 33' 16", the
//     BKB angle) and 4.902 m at zenith 97 57' 06"; its bearing from MARK is
//     322 33' only if 13.856 is the northing (atan2(-2.953, 3.856)), and
//     127 27' if it were the easting. The sideshot to ST2 (7 56' 17", 4.956 m
//     at 97 13' 46", HI 1.52, HR 1.60) reproduces "XYZ 14.87000,10.67900,
//     -0.20400" to the millimetre the same way (tests/topcon_gts.rs).
//   * The sample writes the weather record as "TEMP", the list as "ATMOS";
//     both are read. Neither states its units, so temperature and pressure
//     are kept as text in metadata and no atmospheric state is claimed.
//   * VA is a zenith angle (the sample's 97 57' 06" looks slightly down at a
//     target 0.26 m below the instrument). Readings above 180 degrees are face
//     right and are held as 360 - z (Face::Right).
//   * Horizontal angles may be negative (the sample's -37.26440): they are
//     circle readings and are wrapped into [0, 360).
//   * BKB's third field ("backsight angle") is the circle reading on the
//     backsight - the sample's BS observation reads exactly it - and is the
//     setup's backsight_azimuth, which the model defines as that reading. The
//     second ("backsight bearing") is kept in the setup's metadata.
//   * HD's VD is the instrument-to-target height difference; the ground-to-
//     ground difference VD + HI - HT becomes a LevelDifferenceObservation.
//   * "F" (feet) does not say which foot. It is read as the international
//     foot, with a warning: a US survey foot file differs by 2 ppm.
//   * XYZ after STN or BKB is a known coordinate (CoordinateSource::Unknown);
//     after FS or SS it is one Topcon Link computed ([LINK]: GTS-7+ "saves
//     measured points SideShots coordinates after calculating coordinates"),
//     so Calculated.
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

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, is_ascii_space, trimmed};
use katana_core::{Error, ErrorCode, Result};
use katana_math::numerics::TWO_PI;
use katana_survey::data_model::{distance_sigma, source_file_name};
use katana_survey::{
    AngularUnit, CoordinateSource, DeclaredUnits, DistanceKind, DistanceObservation, Face,
    HorizontalDirectionObservation, InstrumentSettings, LevelDifferenceObservation, LinearUnit,
    ObservationPrecision, Pointing, SourceRecord, ZenithAngleObservation,
};

use crate::detect::{looks_like_text, probe_lines, without_byte_order_mark};
use crate::format::{
    FormatContent, FormatDescriptor, FormatRegistration, FormatSignature, Manufacturer, ProbeInput,
    ReadOptions, ReadResult, describe, ruled_out,
};
use crate::topcon_raw_builder::{
    METRES_PER_FOOT, RawProjectBuilder, face_of_zenith_reading, gons_to_radians,
    packed_degrees_to_radians, parse_real, wrap_to_circle, zenith_in_model_range,
};

const GTS7_ID: &str = "topcon-gts7";
const GTS6_ID: &str = "topcon-gts6";

const CONTROL_WORDS: [&[u8]; 20] = [
    b"JOB", b"DATE", b"NAME", b"INST", b"UNITS", b"SCALE", b"ATMOS", b"TEMP", b"STN", b"XYZ",
    b"BKB", b"BS", b"FS", b"SS", b"CTL", b"HV", b"SD", b"HD", b"NOTE", b"OFFSET",
];

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

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn signature(confidence: f64, evidence: String) -> FormatSignature {
    FormatSignature {
        confidence,
        evidence,
    }
}

fn is_control_word(word: &[u8]) -> bool {
    CONTROL_WORDS.contains(&word)
}

// How many fields each record has in [LINK]'s list (C-6 to C-8); a field past
// these is one this reader cannot place and says so. NOTE is free text.
fn fields_defined(word: &[u8]) -> usize {
    if word == b"NAME" || word == b"INST" {
        return 1;
    }
    if word == b"JOB"
        || word == b"DATE"
        || word == b"UNITS"
        || word == b"ATMOS"
        || word == b"TEMP"
        || word == b"BS"
        || word == b"HV"
    {
        return 2;
    }
    if word == b"FS" || word == b"SS" {
        return 4;
    }
    3 // SCALE, STN, XYZ, BKB, CTL, SD, HD, OFFSET
}

// Up to 8 comma-separated fields; no record in [LINK] has more than four.
#[derive(Default)]
struct Fields<'a> {
    values: [&'a [u8]; 8],
    count: usize,
    overflow: bool,
}

impl<'a> Fields<'a> {
    fn at(&self, i: usize) -> Option<&'a [u8]> {
        if i < self.count && !self.values[i].is_empty() {
            Some(self.values[i])
        } else {
            None
        }
    }
}

fn split_fields(text: &[u8]) -> Fields<'_> {
    let mut fields = Fields::default();
    let mut start = 0;
    loop {
        let cut = find(text, b',', start);
        let value = trimmed(match cut {
            Some(cut) => &text[start..cut],
            None => &text[start..],
        });
        if fields.count == fields.values.len() {
            fields.overflow = true;
        } else {
            fields.values[fields.count] = value;
            fields.count += 1;
        }
        match cut {
            Some(cut) => start = cut + 1,
            None => break,
        }
    }
    fields
}

// The point the next XYZ record gives coordinates to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Subject {
    None,
    Station,
    Backsight,
    Target,
}

// Records carrying fields past the defined ones, by control word: warned at
// the first, counted after, so a program that adds a field to every shot gives
// two warnings and not one per shot.
struct Extra {
    first: usize,
    records: usize,
}

// What one HV, SD or HD record observes, for the observations it makes.
struct Shot<'a> {
    at: &'a str,
    to: &'a str,
    hi: f64,
    th: f64,
    pointing: Pointing,
    precision: ObservationPrecision,
}

struct Gts7Reader {
    builder: RawProjectBuilder,
    fatal: Option<Error>,
    units_seen: bool,
    metres_per_unit: f64,
    gons: bool,
    settings: InstrumentSettings,
    // "temperature, pressure" as written, units unstated
    atmosphere: String,
    atmosphere_seen: bool,
    any_target_height: bool,

    subject: Subject,
    subject_point: String,
    // The shot header (BS / FS / SS) the next HV, SD or HD belongs to.
    header_open: bool,
    target: String,
    target_height: f64,
    last_observed: String,

    extra: BTreeMap<String, Extra>,
}

impl Gts7Reader {
    fn new(file_name: &str, options: &ReadOptions) -> Self {
        Self {
            builder: RawProjectBuilder::new(
                file_name,
                SourceRecord {
                    manufacturer: String::from("Topcon"),
                    format: String::from("GTS-7"),
                    format_version: String::from("Link 7010-0522"),
                    file_name: String::from(file_name),
                    record_number: 0,
                },
                options,
            ),
            fatal: None,
            units_seen: false,
            metres_per_unit: 1.0,
            gons: false,
            settings: InstrumentSettings::default(),
            atmosphere: String::new(),
            atmosphere_seen: false,
            any_target_height: false,
            subject: Subject::None,
            subject_point: String::new(),
            header_open: false,
            target: String::new(),
            target_height: 0.0,
            last_observed: String::new(),
            extra: BTreeMap::new(),
        }
    }

    fn extra_fields(&mut self, word: &[u8], f: &Fields<'_>, n: usize) {
        let defined = fields_defined(word);
        let mut values = String::new();
        for value in f.values.iter().take(f.count).skip(defined) {
            if !value.is_empty() {
                values.push_str(if values.is_empty() { "'" } else { ", '" });
                values.push_str(&text(substr(value, 0, 40)));
                values.push('\'');
            }
        }
        if values.is_empty() {
            return;
        }
        let word = text(word);
        match self.extra.entry(word.clone()) {
            Entry::Occupied(mut entry) => entry.get_mut().records += 1,
            Entry::Vacant(entry) => {
                entry.insert(Extra {
                    first: n,
                    records: 1,
                });
                self.builder.warn(
                    n,
                    format!(
                        "{word} record has fields past the {defined} the GTS-7 format defines \
                         ({values}); they are not imported, here or on any later {word} record"
                    ),
                );
            }
        }
    }

    fn units_known(&mut self, n: usize) -> bool {
        if !self.units_seen && self.fatal.is_none() {
            self.fatal = Some(make_error(
                ErrorCode::FileImportFailure,
                format!(
                    "record {n} of {} gives a measurement before any UNITS record has said \
                     whether the file is in metres or feet and degrees or gons, and Katana will \
                     not guess",
                    self.builder.file_name()
                ),
                "GTS-7 [LINK] C-6: UNITS Meter/Feet, Degree/Gon",
            ));
        }
        self.units_seen
    }

    fn linear(&mut self, value: Option<&[u8]>, what: &str, n: usize) -> Option<f64> {
        let value = value?;
        if !self.units_known(n) {
            return None;
        }
        let Some(parsed) = parse_real(value) else {
            self.builder
                .warn(n, format!("{what} '{}' is not a number", text(value)));
            return None;
        };
        Some(parsed * self.metres_per_unit)
    }

    fn angle(&mut self, value: Option<&[u8]>, what: &str, n: usize) -> Option<f64> {
        let value = value?;
        if !self.units_known(n) {
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
                    "{what} '{}' is not an angle in {}",
                    text(value),
                    if self.gons {
                        "gons"
                    } else {
                        "degrees (DDD.MMSS)"
                    }
                ),
            );
        }
        parsed
    }

    fn read(mut self, bytes: &[u8]) -> Result<ReadResult> {
        let mut n = 0;
        let mut start = if bytes.starts_with(b"\xEF\xBB\xBF") {
            3
        } else {
            0
        };
        let mut first = true;
        while start < bytes.len() && self.fatal.is_none() {
            let end = find(bytes, b'\n', start).unwrap_or(bytes.len());
            let line = &bytes[start..end];
            start = end + 1;
            n += 1;
            let line = trimmed(line);
            if line.is_empty() {
                continue;
            }
            let mut space = 0;
            while space < line.len() && !is_ascii_space(line[space]) {
                space += 1;
            }
            let word = &line[..space];
            if !is_control_word(word) {
                if first {
                    self.builder
                        .project()
                        .metadata
                        .insert(String::from("header"), text(substr(line, 0, 200)));
                    self.builder.count_read();
                } else {
                    self.builder.skip(
                        n,
                        format!(
                            "'{}' is not a GTS-7 record (the Topcon Link manual lists JOB, DATE, \
                             NAME, INST, UNITS, SCALE, ATMOS, STN, XYZ, BKB, BS, FS, SS, CTL, HV, \
                             SD, HD, OFFSET and NOTE)",
                            text(substr(word, 0, 20))
                        ),
                    );
                }
                first = false;
                continue;
            }
            first = false;
            if word == b"NOTE" {
                // A comment is free text: its commas are not field separators.
                self.builder
                    .add_station_note(&text(trimmed(&line[space..])));
                self.builder.count_read();
                continue;
            }
            let fields = split_fields(trimmed(&line[space..]));
            if fields.overflow {
                self.builder.warn(
                    n,
                    String::from(
                        "record has more fields than any GTS-7 record defines; the extra fields \
                         were not read",
                    ),
                );
            }
            let skipped_before = self.builder.records_skipped();
            self.record(word, &fields, n);
            if self.fatal.is_none() && self.builder.records_skipped() == skipped_before {
                self.extra_fields(word, &fields, n);
            }
        }
        if let Some(fatal) = self.fatal.take() {
            return Err(fatal);
        }
        for (word, extra) in &self.extra {
            if extra.records > 1 {
                self.builder.warn(
                    0,
                    format!(
                        "{} {word} records carry fields past the ones the GTS-7 format defines, \
                         none of them imported; the first is record {}",
                        extra.records, extra.first
                    ),
                );
            }
        }
        if !self.builder.project().stations.is_empty() {
            if !self.any_target_height {
                self.builder.not_carried(String::from("no target heights"));
            }
            self.builder.not_carried(String::from(
                "no prism constant: the GTS-7 format has no field for one",
            ));
            self.builder.not_carried(String::from(if self.atmosphere_seen {
                "atmospheric correction unknown: the file gives temperature and pressure without \
                 units and does not say whether the instrument applied a ppm correction"
            } else {
                "no atmospheric settings"
            }));
        }
        let result = self.builder.finish();
        let project = &result.project;
        if project.stations.is_empty()
            && project.observations.is_empty()
            && project.points.is_empty()
            && project.unpositioned_points.is_empty()
        {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                format!(
                    "{} holds no GTS-7 setup, observation or point Katana could read",
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

    fn record(&mut self, word: &[u8], f: &Fields<'_>, n: usize) {
        if word == b"JOB" {
            // The sample's job name is a path ("C:\Download\777.raw"): only the
            // name part is kept, for the reason given at source_file_name.
            if let Some(name) = f.at(0) {
                self.builder.project().name = source_file_name(&text(name));
            }
            if let Some(description) = f.at(1) {
                self.builder
                    .project()
                    .metadata
                    .insert(String::from("job description"), text(description));
            }
            self.builder.count_read();
        } else if word == b"DATE" {
            // Day and month order is not documented; kept as written.
            let mut date = text(f.at(0).unwrap_or_default());
            if let Some(time) = f.at(1) {
                date.push(' ');
                date.push_str(&text(time));
            }
            self.builder
                .project()
                .metadata
                .insert(String::from("date"), date);
            self.builder.count_read();
        } else if word == b"NAME" {
            self.builder
                .project()
                .metadata
                .insert(String::from("surveyor"), text(f.at(0).unwrap_or_default()));
            self.builder.count_read();
        } else if word == b"INST" {
            self.settings.model = text(f.at(0).unwrap_or_default());
            self.builder.count_read();
        } else if word == b"UNITS" {
            self.units(f, n);
        } else if word == b"SCALE" {
            // "grid factor, scale factor, elevation": what the collector would
            // apply when it computes coordinates, not what is in a raw reading.
            let metadata = &mut self.builder.project().metadata;
            metadata.insert(
                String::from("grid factor"),
                text(f.at(0).unwrap_or_default()),
            );
            metadata.insert(
                String::from("scale factor"),
                text(f.at(1).unwrap_or_default()),
            );
            metadata.insert(
                String::from("scale elevation"),
                text(f.at(2).unwrap_or_default()),
            );
            self.builder.count_read();
        } else if word == b"ATMOS" || word == b"TEMP" {
            self.atmosphere = format!(
                "{}, {}",
                text(f.at(0).unwrap_or_default()),
                text(f.at(1).unwrap_or_default())
            );
            self.atmosphere_seen = true;
            if let Some(station) = self.builder.current_station()
                && station.observations.is_empty()
            {
                station.metadata.insert(
                    String::from("temperature, pressure (units not stated)"),
                    self.atmosphere.clone(),
                );
            }
            self.builder.count_read();
        } else if word == b"STN" {
            self.station(f, n);
        } else if word == b"XYZ" {
            self.coordinates(f, n);
        } else if word == b"BKB" {
            self.backsight(f, n);
        } else if word == b"BS" || word == b"FS" || word == b"SS" {
            self.header(word, f, n);
        } else if word == b"CTL" {
            if !self.header_open {
                self.builder.skip(
                    n,
                    String::from("CTL record with no BS, FS or SS header before it"),
                );
                return;
            }
            self.builder.add_point_metadata(
                &self.target,
                String::from("control code"),
                text(f.at(0).unwrap_or_default()),
            );
            if let Some(code) = f.at(1) {
                self.builder.add_point_metadata(
                    &self.target,
                    String::from("second code"),
                    text(code),
                );
            }
            if let Some(string) = f.at(2) {
                self.builder.add_point_metadata(
                    &self.target,
                    String::from("second string number"),
                    text(string),
                );
            }
            self.builder.count_read();
        } else if word == b"HV" || word == b"SD" || word == b"HD" {
            self.measurement(word, f, n);
        } else if word == b"OFFSET" {
            self.offset(f, n);
        }
    }

    fn units(&mut self, f: &Fields<'_>, n: usize) {
        let length = f.at(0).unwrap_or_default();
        let angular = f.at(1).unwrap_or_default();
        let mut declared = DeclaredUnits::default();
        if length == b"M" {
            self.metres_per_unit = 1.0;
            declared.linear = LinearUnit::Metres;
        } else if length == b"F" {
            self.metres_per_unit = METRES_PER_FOOT;
            declared.linear = LinearUnit::Feet;
            self.builder.warn(
                n,
                String::from(
                    "the file is in feet and the GTS-7 format does not say which foot; it is read \
                     as the international foot (0.3048 m) - a file in US survey feet would be 2 \
                     ppm different, which matters for large coordinates",
                ),
            );
        } else {
            self.fatal = Some(make_error(
                ErrorCode::FileImportFailure,
                format!(
                    "record {n} of {} gives the length unit as '{}'; the GTS-7 format defines M \
                     (metres) and F (feet)",
                    self.builder.file_name(),
                    text(length)
                ),
                "GTS-7 [LINK] C-6: UNITS Meter/Feet, Degree/Gon",
            ));
            return;
        }
        if angular == b"D" {
            self.gons = false;
            declared.angular = AngularUnit::DegreesMinutesSeconds;
        } else if angular == b"G" {
            self.gons = true;
            declared.angular = AngularUnit::Gons;
        } else {
            self.fatal = Some(make_error(
                ErrorCode::FileImportFailure,
                format!(
                    "record {n} of {} gives the angle unit as '{}'; the GTS-7 format defines D \
                     (degrees) and G (gons)",
                    self.builder.file_name(),
                    text(angular)
                ),
                "GTS-7 [LINK] C-6: UNITS Meter/Feet, Degree/Gon",
            ));
            return;
        }
        if self.units_seen && self.builder.project().units != declared {
            self.builder.warn(
                n,
                String::from(
                    "the units change here; values after this record are converted with the new \
                     units",
                ),
            );
        }
        if !self.units_seen {
            self.builder.project().units = declared;
        }
        self.units_seen = true;
        self.builder.count_read();
    }

    fn station(&mut self, f: &Fields<'_>, n: usize) {
        let Some(point) = f.at(0) else {
            self.builder
                .skip(n, String::from("STN record names no point"));
            return;
        };
        let height = self.linear(f.at(1), "instrument height", n);
        if self.fatal.is_some() {
            return;
        }
        if height.is_none() {
            self.builder.warn(
                n,
                String::from("the setup states no instrument height; 0 is used"),
            );
        }
        let point = text(point);
        let setup = self
            .builder
            .begin_station(&point, height.unwrap_or(0.0), &self.settings, n);
        if self.atmosphere_seen {
            setup.metadata.insert(
                String::from("temperature, pressure (units not stated)"),
                self.atmosphere.clone(),
            );
        }
        if let Some(code) = f.at(2) {
            self.builder.code_point(&point, &text(code), "", "", n);
        }
        self.subject = Subject::Station;
        self.subject_point = point;
        self.header_open = false;
        self.builder.count_read();
    }

    fn coordinates(&mut self, f: &Fields<'_>, n: usize) {
        if self.subject == Subject::None {
            self.builder.skip(
                n,
                String::from("XYZ record with no STN, BKB, FS or SS before it to say whose it is"),
            );
            return;
        }
        let northing = self.linear(f.at(0), "northing", n);
        let easting = self.linear(f.at(1), "easting", n);
        let elevation = self.linear(f.at(2), "elevation", n);
        if self.fatal.is_some() {
            return;
        }
        let (Some(northing), Some(easting)) = (northing, easting) else {
            self.builder.skip(
                n,
                String::from("XYZ record without both northing and easting"),
            );
            return;
        };
        let how = if self.subject == Subject::Target {
            CoordinateSource::Calculated
        } else {
            CoordinateSource::Unknown
        };
        self.builder.position_point(
            &self.subject_point,
            northing,
            easting,
            elevation,
            how,
            n,
            "",
        );
        self.builder.count_read();
    }

    fn backsight(&mut self, f: &Fields<'_>, n: usize) {
        let occupied = self
            .builder
            .current_station()
            .map(|setup| setup.setup.point_id.clone());
        let (Some(occupied), Some(point)) = (occupied, f.at(0)) else {
            self.builder.skip(
                n,
                String::from("BKB record with no STN before it, or naming no point"),
            );
            return;
        };
        let bearing = self.angle(f.at(1), "backsight bearing", n);
        let circle = self.angle(f.at(2), "backsight angle", n);
        if self.fatal.is_some() {
            return;
        }
        let point = text(point);
        if point == occupied {
            self.builder
                .skip(n, String::from("the backsight is the occupied point"));
            return;
        }
        self.builder.mention_point(&point, n);
        if let Some(setup) = self.builder.current_station() {
            setup.backsight_point_id = point.clone();
            if let Some(circle) = circle {
                setup.backsight_azimuth = Some(wrap_to_circle(circle));
            }
            if let Some(bearing) = bearing {
                setup.metadata.insert(
                    String::from("backsight bearing (radians)"),
                    format_exact_real(wrap_to_circle(bearing)),
                );
            }
        }
        self.subject = Subject::Backsight;
        self.subject_point = point;
        self.builder.count_read();
    }

    fn header(&mut self, word: &[u8], f: &Fields<'_>, n: usize) {
        let occupied = self
            .builder
            .current_station()
            .map(|setup| setup.setup.point_id.clone());
        let (Some(point), Some(occupied)) = (f.at(0), occupied) else {
            self.header_open = false;
            self.builder.skip(
                n,
                format!(
                    "{} record with no STN before it, or naming no point",
                    text(word)
                ),
            );
            return;
        };
        let point = text(point);
        if point == occupied {
            self.header_open = false;
            self.builder
                .skip(n, format!("a shot from '{point}' to itself"));
            return;
        }
        // "BS ptno[,target height]": a backsight without one keeps the last.
        if let Some(height) = self.linear(f.at(1), "target height", n) {
            self.target_height = height;
            self.any_target_height = true;
        }
        if self.fatal.is_some() {
            return;
        }
        self.builder.mention_point(&point, n);
        self.target = point;
        self.header_open = true;
        if word != b"BS" {
            let code = f.at(2).unwrap_or_default();
            let string = f.at(3).unwrap_or_default();
            if !code.is_empty() {
                self.builder
                    .code_point(&self.target, &text(code), "", &text(string), n);
            }
            if word == b"FS" {
                self.builder.add_point_metadata(
                    &self.target,
                    String::from("traverse foresight"),
                    format!("record {n}"),
                );
            }
        } else if let Some(setup) = self.builder.current_station()
            && setup.backsight_point_id.is_empty()
        {
            setup.backsight_point_id = self.target.clone();
        }
        self.subject = Subject::Target;
        self.subject_point = self.target.clone();
        self.builder.count_read();
    }

    fn measurement(&mut self, word: &[u8], f: &Fields<'_>, n: usize) {
        if !self.header_open {
            self.builder.skip(
                n,
                format!(
                    "{} record with no BS, FS or SS header before it to say what was observed",
                    text(word)
                ),
            );
            return;
        }
        let horizontal = self.angle(f.at(0), "horizontal angle", n);
        let mut zenith = None;
        let mut slope = None;
        let mut flat = None;
        let mut rise = None;
        if word == b"HD" {
            flat = self.linear(f.at(1), "horizontal distance", n);
            rise = self.linear(f.at(2), "vertical distance", n);
        } else {
            zenith = self.angle(f.at(1), "vertical angle", n);
            if word == b"SD" {
                slope = self.linear(f.at(2), "slope distance", n);
            }
        }
        if self.fatal.is_some() {
            return;
        }
        let precision = self.builder.options().precision;
        // A shot header is opened only while a setup is current, and setups
        // are never taken away.
        let (at, hi) = match self.builder.current_station() {
            Some(setup) => (setup.setup.point_id.clone(), setup.setup.instrument_height),
            None => return,
        };
        let th = self.target_height;

        let mut face = Face::Unknown;
        if let Some(reading) = zenith {
            if reading < 0.0 || reading >= TWO_PI {
                self.builder.warn(
                    n,
                    String::from(
                        "zenith reading outside a full circle; the zenith is not imported",
                    ),
                );
                zenith = None;
            } else {
                face = face_of_zenith_reading(reading);
                zenith = Some(zenith_in_model_range(reading));
            }
        }
        let pointing = Pointing {
            index: self.builder.next_pointing(),
            face,
        };
        let target = self.target.clone();
        let shot = Shot {
            at: &at,
            to: &target,
            hi,
            th,
            pointing,
            precision,
        };
        let mut any = false;
        if let Some(horizontal) = horizontal {
            let observation = self
                .builder
                .station_observation::<HorizontalDirectionObservation>(n);
            observation.at = String::from(shot.at);
            observation.to = String::from(shot.to);
            observation.direction = wrap_to_circle(horizontal);
            observation.sigma = precision.direction;
            observation.pointing = pointing;
            any = true;
        }
        if let Some(zenith) = zenith {
            let observation = self
                .builder
                .station_observation::<ZenithAngleObservation>(n);
            observation.from = String::from(shot.at);
            observation.to = String::from(shot.to);
            observation.angle = zenith;
            observation.sigma = precision.zenith;
            observation.instrument_height = hi;
            observation.target_height = th;
            observation.pointing = pointing;
            any = true;
        }
        if let Some(slope) = slope {
            distance(&mut self.builder, n, &shot, slope, DistanceKind::Slope);
            any = true;
        }
        if let Some(flat) = flat {
            distance(&mut self.builder, n, &shot, flat, DistanceKind::Horizontal);
            any = true;
        }
        if let Some(rise) = rise {
            let height_difference = rise + hi - th;
            if height_difference.is_finite() {
                let difference = self
                    .builder
                    .station_observation::<LevelDifferenceObservation>(n);
                difference.from = at.clone();
                difference.to = target.clone();
                difference.height_difference = height_difference;
                difference.sigma = precision
                    .height_measurement
                    .hypot(precision.height_measurement);
                difference.length = match flat {
                    Some(flat) if flat > 0.0 => flat,
                    _ => 0.0,
                };
                any = true;
            } else {
                self.builder.warn(
                    n,
                    String::from(
                        "the vertical distance and heights do not add up to a finite height \
                         difference; it is not imported",
                    ),
                );
            }
        }
        if !any {
            self.builder
                .skip(n, format!("{} record with no readable value", text(word)));
            return;
        }
        self.last_observed = target;
        self.builder.count_read();
    }

    fn offset(&mut self, f: &Fields<'_>, n: usize) {
        if self.last_observed.is_empty() {
            self.builder.skip(
                n,
                String::from("OFFSET record with no SD or HD record before it"),
            );
            return;
        }
        let radial = self.linear(f.at(0), "radial offset", n);
        let tangential = self.linear(f.at(1), "tangential offset", n);
        let vertical = self.linear(f.at(2), "vertical offset", n);
        if self.fatal.is_some() {
            return;
        }
        let written =
            |value: Option<f64>| value.map_or_else(|| String::from("-"), format_exact_real);
        self.builder.add_point_metadata(
            &self.last_observed,
            String::from("offset radial, tangential, vertical (m)"),
            format!(
                "{}, {}, {}",
                written(radial),
                written(tangential),
                written(vertical)
            ),
        );
        self.builder.warn(
            n,
            format!(
                "the offsets to '{}' are kept in its metadata but not applied: the model holds \
                 the observation to the target, and moving the point is the reduction's work",
                self.last_observed
            ),
        );
        self.builder.count_read();
    }
}

// A distance of one HV, SD or HD record; one that is not positive is no
// distance measured, and said to be.
fn distance(
    builder: &mut RawProjectBuilder,
    n: usize,
    shot: &Shot<'_>,
    value: f64,
    kind: DistanceKind,
) {
    if !(value > 0.0) {
        builder.warn(
            n,
            format!(
                "distance of {} m read as no distance measured",
                format_exact_real(value)
            ),
        );
        return;
    }
    let observation = builder.station_observation::<DistanceObservation>(n);
    observation.from = String::from(shot.at);
    observation.to = String::from(shot.to);
    observation.distance = value;
    observation.sigma = distance_sigma(&shot.precision, value);
    observation.kind = kind;
    observation.instrument_height = shot.hi;
    observation.target_height = shot.th;
    observation.pointing = shot.pointing;
}

// ---- GTS-7 registration -------------------------------------------------------------

fn probe_gts7(input: &ProbeInput<'_>) -> FormatSignature {
    let bytes = without_byte_order_mark(input.bytes);
    if !looks_like_text(bytes) {
        return ruled_out();
    }
    let mut lines = 0usize;
    let mut records = 0usize;
    let mut measurements = 0usize;
    let mut first = true;
    for line in probe_lines(input, 200) {
        let line = trimmed(line);
        if line.is_empty() {
            continue;
        }
        let mut space = 0;
        while space < line.len() && !is_ascii_space(line[space]) {
            space += 1;
        }
        let word = &line[..space];
        // A control word is followed by white space ("STN           MARK,..."),
        // which is what tells "SS,OP1,..." (RW5) from "SS  1,1.6,TREE".
        let spaced = space < line.len();
        if is_control_word(word) && spaced {
            records += 1;
            lines += 1;
            if word == b"STN" || word == b"SD" || word == b"HV" || word == b"HD" {
                measurements += 1;
            }
        } else if !first {
            lines += 1;
        }
        first = false;
    }
    if records == 0 || measurements == 0 {
        return ruled_out();
    }
    let share = records as f64 / lines as f64;
    let evidence =
        format!("{records} of {lines} lines start with a GTS-7 control word (STN, BS, SS, SD ...)");
    if share < 0.8 {
        return signature(0.3, evidence);
    }
    let extension = input.extension.as_str();
    if extension == "gt7" || extension == "gts7" {
        return signature(0.97, format!("extension .{extension} and {evidence}"));
    }
    signature(0.9, evidence)
}

fn gts7_descriptor() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(GTS7_ID),
        human_name: String::from("Topcon GTS-7 raw"),
        manufacturer: Manufacturer::Topcon,
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
        parser_version: String::from("1.0"),
        // [LINK]: "GTS-7 Raw (*.raw; *.dat; *.gts; *.gts7; *.gt7)".
        extensions: vec![
            String::from("gt7"),
            String::from("gts7"),
            String::from("gts"),
            String::from("raw"),
            String::from("dat"),
        ],
    }
}

fn read_gts7(bytes: &[u8], file_name: &str, options: &ReadOptions) -> Result<ReadResult> {
    Gts7Reader::new(file_name, options).read(bytes)
}

pub(crate) fn gts7_registration() -> FormatRegistration {
    FormatRegistration::new(gts7_descriptor(), probe_gts7, Some(read_gts7))
}

// ---- GTS-6: recognised, refused ----------------------------------------------------

// The words of a GTS-6 file as [LINK]'s sample shows them: "_'" opens a
// station, "_+" a point, "_(" and "_*" a code, and "_ ?+" / "_ W+" a
// measurement - all joined by underscores. Enough to recognise the file; far
// from enough to read it.
fn probe_gts6(input: &ProbeInput<'_>) -> FormatSignature {
    let bytes = without_byte_order_mark(input.bytes);
    if !looks_like_text(bytes) {
        return ruled_out();
    }
    let start = trimmed(substr(bytes, 0, 64));
    if !(start.starts_with(b"_'") || start.starts_with(b"_+")) {
        return ruled_out();
    }
    let mut words = 0usize;
    let mut at = find(bytes, b'_', 0);
    while let Some(underscore) = at {
        if words >= 1000 {
            break;
        }
        words += 1;
        at = find(bytes, b'_', underscore + 1);
    }
    let measurement = contains(bytes, b"?+") || contains(bytes, b" W+");
    if !measurement || words < 4 {
        return signature(
            0.3,
            String::from("starts with a GTS-6 word but holds no GTS-6 measurement"),
        );
    }
    signature(
        0.9,
        String::from("underscore-separated GTS-6 words (_' station, _+ point, ?+ measurement)"),
    )
}

fn gts6_descriptor() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(GTS6_ID),
        human_name: String::from("Topcon GTS-6 raw (recognised, not readable)"),
        manufacturer: Manufacturer::Topcon,
        // nothing: see read_gts6
        reads: FormatContent::default(),
        can_import: true,
        can_export: false,
        parser_version: String::from("1.0"),
        extensions: vec![
            String::from("raw"),
            String::from("dat"),
            String::from("gts"),
        ],
    }
}

fn read_gts6(_: &[u8], file_name: &str, _: &ReadOptions) -> Result<ReadResult> {
    Err(make_error(
        ErrorCode::Unsupported,
        format!(
            "{file_name} is Topcon GTS-6 raw data, whose record layout Topcon publishes only in \
             the GTS-6 interface manual, which is not public; Katana does not read a layout it \
             would have to guess. Export the job as GTS-7 raw (Topcon Link or Topcon Tools: \
             Export, 'GTS-7 Raw') or as RW5, and import that"
        ),
        "Topcon Link Reference Manual 7010-0522, C-5: \"See the GTS-6 interface manual for \
         details\"",
    ))
}

pub(crate) fn gts6_registration() -> FormatRegistration {
    FormatRegistration::new(gts6_descriptor(), probe_gts6, Some(read_gts6))
}
