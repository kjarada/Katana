//! Trimble JobXML (.jxl) reader.
//!
//! The port of src/katana_surveyio/trimble_jobxml.cpp. The format has no public
//! interface of its own: it is reached through the registry (`registration`),
//! as the C++ reaches it through its static registration.

// SPECIFICATION: Trimble, "Job XML Schema Definition Version 5.72"
// (JobXMLSchema-5.72.xsd, published at
// https://ww2.trimble.com/schema/JobXML/5_7/JobXMLSchema-5.72.xsd). Element
// names, units and the meaning of every flag below are taken from that
// document; where a comment says "the schema says", it is quoting it.
//
// What a JobXML file is. A <JOBFile> root holding three sections:
//   <FieldBook>   every record the controller wrote, in the order it wrote
//                 them: instrument, atmosphere, station, backsight, target and
//                 point records, each with an ID that later records refer to.
//                 The schema says the raw terrestrial observations in it
//                 (circleType) have had NO corrections applied: no atmospheric
//                 ppm (unless the AtmosphereRecord's ApplyPPMToRawDistances is
//                 false), no prism constant, no curvature or refraction, no
//                 orientation. That is what the correction states below say.
//   <Reductions>  the controller's own "best" grid coordinates for each point.
//                 This reader does NOT use them as positions - the reduction
//                 in katana_survey computes positions from the raw
//                 observations, and a point that already had the controller's
//                 answer would be held rather than computed. They are kept in
//                 each point's metadata ("trimble.reductions.*", the file's
//                 own digits) so a report can compare the two.
//   <Environment> the units and coordinate system current at export. Kept as
//                 project metadata; the coordinate system names become the
//                 DeclaredCoordinateSystem (declared, never transformed).
//
// UNITS. The schema says every angle, latitude and longitude is in decimal
// degrees and every distance and height in metres, whatever the job's display
// units were; temperatures are degrees Celsius and pressures hPa. So nothing
// here depends on the UnitsRecord, and DeclaredUnits says metres and decimal
// degrees. Degrees become radians on the way in.
//
// FACES. Face1 is face left, Face2 face right. A VerticalCircle reading is
// the zenith-referenced circle reading Trimble instruments record: about 90
// degrees on face 1 and about 270 on face 2. ZenithAngleObservation holds a
// zenith ANGLE in [0, pi], so a reading r above 180 degrees is stored as
// 360 - r, the zenith angle it measures; the Pointing keeps the face, and the
// difference between that and the face-1 reading is the index error the
// reduction's face averaging removes. Horizontal circle readings are stored
// as read (HorizontalDirectionObservation), face 2 included.
//
// DELETED RECORDS. JobXML keeps records the surveyor deleted, flagged
// <Deleted>true</Deleted>. They are not imported, and one warning names how
// many there were and which.
//
// WHAT IS NOT READ is said, never dropped quietly: record kinds this reader
// does not interpret are counted per kind in a warning; elements inside a
// record it does interpret, but that the survey model has no place for, are
// listed per element group in a warning with the number of records carrying
// them; configuration records and the Environment go to project metadata.
//
// The file is read as a stream (xml_pull.rs): a 200 MB job is never held as a
// tree. Each FieldBook record is gathered into a small reusable buffer of
// (path, value) pairs, interpreted, and the buffer reused.
//
// TEXT. The C++ reads the document's bytes as they are; the survey model here
// holds validated UTF-8, so the document - JobXML is UTF-8, as its declaration
// says - is checked to be UTF-8 once, before it is read, and one that is not
// is refused, naming its line, rather than read into text it cannot be (a
// C++ defect: it puts such bytes in the model as they are). Every view the
// reader then takes is cut at ASCII markup, so every view is UTF-8 (`text`).

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use katana_core::text::{format_exact_real, parse_finite_double};
use katana_core::xml::XML_MAX_DEPTH;
use katana_core::{Error, ErrorCode, Result};
use katana_math::numerics::{PI, TWO_PI};
use katana_survey::data_model::{
    distance_sigma, observation_kind_name, source_file_name, validate_observation,
};
use katana_survey::{
    AngularUnit, AntennaHeightMethod, ControlPoint, CoordinateSource, CorrectionState,
    DeclaredCoordinateSystem, DeclaredUnits, DistanceKind, DistanceObservation, Face,
    GeocentricCoordinate, GeodeticCoordinate, GnssAntenna, GnssCovariance3,
    GnssGeocentricBaselineObservation, GnssGlobalPositionObservation, GnssSolution,
    HorizontalDirectionObservation, LinearUnit, Observation, Pointing, SourceRecord, Station,
    SurveyFeature, SurveyPoint, SurveyStation, SurveyTimestamp, UnpositionedPoint,
    ZenithAngleObservation,
};

use crate::detect::without_byte_order_mark;
use crate::format::{
    FormatContent, FormatDescriptor, FormatRegistration, FormatSignature, Manufacturer, ProbeInput,
    ReadOptions, ReadResult, ReadWarning, ruled_out,
};
use crate::xml_pull::{Event, LineCounter, XmlPullReader, append_xml_text, same_text};

const FORMAT_ID: &str = "trimble-jobxml";
const HUMAN_NAME: &str = "Trimble JobXML";
const RADIANS_PER_DEGREE: f64 = PI / 180.0;

// ---- Small text helpers -------------------------------------------------------

fn is_space(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\r' || c == b'\n'
}

fn trimmed(mut text: &[u8]) -> &[u8] {
    while let Some((&first, rest)) = text.split_first()
        && is_space(first)
    {
        text = rest;
    }
    while let Some((&last, rest)) = text.split_last()
        && is_space(last)
    {
        text = rest;
    }
    text
}

// A view of the document as text. The document was found to be UTF-8 before
// it was read (read_job_xml), and every view is cut at ASCII markup or is
// text decoded from such views, so the conversion cannot fail; were it ever
// to, the view would read as empty rather than as bytes the model cannot
// hold.
fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap_or_default()
}

// The first `count` bytes of a view, for a message: the C++ cuts by bytes,
// and a character cut in two shows as U+FFFD.
fn shown(bytes: &[u8], count: usize) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(count)]).into_owned()
}

// A decimal number as XML Schema writes a double: optional sign, digits,
// optional fraction and exponent. std::from_chars does not take a leading
// '+', which xsd:double allows, so the C++ steps over one; from_chars then
// takes no second sign but '-', as core's parse_finite_double would.
fn parse_number(text: &[u8]) -> Option<f64> {
    let mut text = trimmed(text);
    if let Some(rest) = text.strip_prefix(b"+") {
        text = rest;
    }
    if text.is_empty() || text[0] == b'+' {
        return None;
    }
    parse_finite_double(text)
}

fn positive_finite(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

// An angle in degrees as a circle reading in [0, 360). Almost every reading
// already is one, and fmod is the slowest step of a shot, so it is skipped
// for those.
fn circle_degrees(degrees: f64) -> f64 {
    if (0.0..360.0).contains(&degrees) {
        return degrees;
    }
    let mut degrees = degrees % 360.0;
    if degrees < 0.0 {
        degrees += 360.0;
    }
    if degrees >= 360.0 { 0.0 } else { degrees }
}

fn wrap_two_pi(angle: f64) -> f64 {
    if (0.0..TWO_PI).contains(&angle) {
        return angle;
    }
    let mut angle = angle % TWO_PI;
    if angle < 0.0 {
        angle += TWO_PI;
    }
    // fmod of a value a hair under 2*pi can round back up to it.
    if angle >= TWO_PI { 0.0 } else { angle }
}

// std::from_chars into an int: an optional '-', digits, nothing left over, no
// overflow. Rust's parse also takes a '+', which from_chars does not.
fn integer(digits: &[u8]) -> Option<i32> {
    if digits.first() == Some(&b'+') {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

// "2024-03-05T10:15:30.5", optionally followed by 'Z' or a +hh:mm offset.
// A timestamp with no zone is the controller's clock; which clock the file
// does not say, so the time system is left empty rather than guessed.
fn parse_timestamp(stamp: &[u8]) -> SurveyTimestamp {
    let mut time = SurveyTimestamp::default();
    let stamp = trimmed(stamp);
    if stamp.len() < 19
        || stamp[4] != b'-'
        || stamp[7] != b'-'
        || stamp[10] != b'T'
        || stamp[13] != b':'
        || stamp[16] != b':'
    {
        return time;
    }
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute)) = (
        integer(&stamp[0..4]),
        integer(&stamp[5..7]),
        integer(&stamp[8..10]),
        integer(&stamp[11..13]),
        integer(&stamp[14..16]),
    ) else {
        return time;
    };
    let mut seconds_end = 17;
    while seconds_end < stamp.len()
        && (stamp[seconds_end].is_ascii_digit() || stamp[seconds_end] == b'.')
    {
        seconds_end += 1;
    }
    let second = parse_number(&stamp[17..seconds_end]);
    let Some(second) = second else {
        return time;
    };
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second >= 61.0
    {
        return time;
    }
    time.year = year;
    time.month = month;
    time.day = day;
    time.hour = hour;
    time.minute = minute;
    time.second = second;
    let zone = &stamp[seconds_end..];
    if zone == b"Z" {
        time.time_system = String::from("UTC");
    } else if !zone.is_empty() {
        time.time_system = format!("UTC{}", text(zone));
    }
    time
}

// ---- One record, gathered ---------------------------------------------------------

// Where a path or value lives: a view of the document, or a run of the
// arena - an offset, not a view, because the arena moves as it grows.
#[derive(Clone, Copy, Debug)]
enum Span<'a> {
    Document(&'a [u8]),
    Arena { offset: u32, size: u32 },
}

impl Span<'_> {
    fn size(&self) -> usize {
        match self {
            Span::Document(view) => view.len(),
            Span::Arena { size, .. } => *size as usize,
        }
    }
}

const EMPTY_SPAN: Span<'static> = Span::Arena { offset: 0, size: 0 };
const NO_NAME: &[u8] = &[];

// `used` is a Cell so that taking a value (which marks it read) leaves the
// buffer borrowed only for reading: a record's interpreter holds several of
// its values at once, as the C++ holds several views.
#[derive(Clone, Debug)]
struct Field<'a> {
    path: Span<'a>,
    value: Span<'a>,
    used: Cell<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pending {
    None,
    View,
    Copied,
}

// The leaf values of one element (a FieldBook record, a Reductions point, the
// Environment), as (path relative to the element, decoded text) pairs in
// document order. Attributes of nested elements are kept as "path@name".
// Reused from record to record, so after the first few records it allocates
// nothing - and most values are not copied at all: a value that is one run of
// text with no entity in it is a view into the document, which outlives every
// record, and so is the path of a direct child, which is its element name.
// Decoded values and the paths of nested elements are copied into an arena.
struct RecordBuffer<'a> {
    arena: Vec<u8>,
    fields: Vec<Field<'a>>,
    path: Vec<u8>,
    depth_starts: [u32; XML_MAX_DEPTH],
    had_child: [bool; XML_MAX_DEPTH],
    names: [&'a [u8]; XML_MAX_DEPTH],
    depth: usize,
    // Elements opened past the arrays' end, never expected.
    overflow: usize,
    pending_view: &'a [u8],
    pending: Vec<u8>,
    pending_mode: Pending,
    decode_error: Error,
    line: usize,
    // Bytes of the values held as views into the document.
    viewed: usize,
    over_budget: bool,
}

impl<'a> RecordBuffer<'a> {
    // The most one record may make the reader hold, in bytes: its paths and
    // values, copied or viewed, and its list of fields. Why a limit: the path
    // of every nested value repeats the names of the elements around it, so a
    // crafted record - one element with a long name holding many empty ones
    // - asks for memory growing with the SQUARE of its size (a 128 KB file,
    // 512 MB; a 1 MB file, some 32 GB), and past 4 GiB the 32-bit offsets of
    // a Span would wrap and resolve to the wrong bytes. The largest record in
    // a public 918 kB Survey Controller job is under 3 kB and its Environment
    // under 4 kB: 16 MiB is thousands of times what a controller writes, and
    // keeps every offset far below 4 GiB. A record past it is dropped from the
    // buffer (and the memory given back) while the rest of it is read through,
    // and the caller skips it with a warning.
    const BUDGET_BYTES: usize = 16 << 20;
    const BUDGET_MEBIBYTES: usize = Self::BUDGET_BYTES >> 20;
    // What the C++ counts for each field held: sizeof(Field), two 16-byte
    // spans (a pointer and two 32-bit numbers) and a flag, padded to 40. The
    // C++'s figure, so that a record goes over the budget where the C++'s
    // does.
    const FIELD_BYTES: usize = 40;

    fn new() -> Self {
        Self {
            arena: Vec::new(),
            fields: Vec::new(),
            path: Vec::new(),
            depth_starts: [0; XML_MAX_DEPTH],
            had_child: [false; XML_MAX_DEPTH],
            names: [NO_NAME; XML_MAX_DEPTH],
            depth: 0,
            overflow: 0,
            pending_view: NO_NAME,
            pending: Vec::new(),
            pending_mode: Pending::None,
            decode_error: Error::default(),
            line: 0,
            viewed: 0,
            over_budget: false,
        }
    }

    fn reset(&mut self, line: usize) {
        self.arena.clear();
        self.fields.clear();
        self.path.clear();
        self.depth = 0;
        self.overflow = 0;
        self.pending.clear();
        self.pending_mode = Pending::None;
        self.line = line;
        self.viewed = 0;
        self.over_budget = false;
    }

    // True when the record went past BUDGET_BYTES: nothing of it is held.
    fn over_budget(&self) -> bool {
        self.over_budget
    }

    // `element` is a view into the document (XmlPullReader::name()).
    fn open(&mut self, element: &'a [u8]) {
        if self.over_budget {
            return;
        }
        if self.depth > 0 {
            self.had_child[self.depth - 1] = true;
        }
        // The pull reader refuses nesting deeper than XML_MAX_DEPTH from the
        // root, so a record (which starts below the root) never reaches the
        // end of these arrays; the check keeps that true if the reader's limit
        // and this one ever part.
        if self.depth >= self.depth_starts.len() {
            self.overflow += 1;
            return;
        }
        self.depth_starts[self.depth] = self.path.len() as u32;
        self.had_child[self.depth] = false;
        self.names[self.depth] = element;
        self.depth += 1;
        if !self.path.is_empty() {
            self.path.push(b'/');
        }
        self.path.extend_from_slice(element);
        self.pending.clear();
        self.pending_mode = Pending::None;
        self.check_budget();
    }

    // Attribute of the element just opened.
    fn attribute(&mut self, name: &[u8], decoded: &[u8]) {
        if self.over_budget {
            return;
        }
        let path_begin = self.arena.len() as u32;
        self.arena.extend_from_slice(&self.path);
        self.arena.push(b'@');
        self.arena.extend_from_slice(name);
        let value_begin = self.arena.len() as u32;
        self.arena.extend_from_slice(decoded);
        self.fields.push(Field {
            path: Span::Arena {
                offset: path_begin,
                size: value_begin - path_begin,
            },
            value: Span::Arena {
                offset: value_begin,
                size: self.arena.len() as u32 - value_begin,
            },
            used: Cell::new(false),
        });
        self.check_budget();
    }

    // A run of character data in the element open now; `raw` is a view into
    // the document. Entities are decoded (and refused, see xml_pull.rs) only
    // when the reader saw a reference in it. False when one cannot be
    // decoded, and decode_error() says why - a bool, not a Status, because
    // this runs for every piece of text in the file.
    fn add_text(&mut self, raw: &'a [u8], has_reference: bool) -> bool {
        if self.over_budget {
            return true;
        }
        let plain = !has_reference;
        if self.pending_mode == Pending::None && plain {
            self.pending_view = raw;
            self.pending_mode = Pending::View;
            return true;
        }
        if self.pending_mode == Pending::View {
            self.pending.clear();
            self.pending.extend_from_slice(self.pending_view);
        }
        self.pending_mode = Pending::Copied;
        if plain {
            self.pending.extend_from_slice(raw);
            self.check_budget();
            return true;
        }
        if let Err(error) = append_xml_text(raw, &mut self.pending) {
            self.decode_error = error;
            return false;
        }
        self.check_budget();
        true
    }

    fn decode_error(&self) -> &Error {
        &self.decode_error
    }

    fn close(&mut self) {
        if self.over_budget {
            return;
        }
        if self.overflow > 0 {
            self.overflow -= 1;
            return;
        }
        if self.depth == 0 {
            return;
        }
        self.depth -= 1;
        if !self.had_child[self.depth] {
            let path = if self.depth == 0 {
                Span::Document(self.names[0])
            } else {
                let offset = self.arena.len() as u32;
                self.arena.extend_from_slice(&self.path);
                Span::Arena {
                    offset,
                    size: self.path.len() as u32,
                }
            };
            let value = match self.pending_mode {
                Pending::View => {
                    let value = trimmed(self.pending_view);
                    // Counted, so no view's size can wrap its Span either.
                    self.viewed += value.len();
                    Span::Document(value)
                }
                Pending::Copied => {
                    let value = trimmed(&self.pending);
                    let offset = self.arena.len() as u32;
                    let size = value.len() as u32;
                    self.arena.extend_from_slice(value);
                    Span::Arena { offset, size }
                }
                Pending::None => EMPTY_SPAN,
            };
            self.fields.push(Field {
                path,
                value,
                used: Cell::new(false),
            });
        }
        self.path.truncate(self.depth_starts[self.depth] as usize);
        self.pending.clear();
        self.pending_mode = Pending::None;
        self.check_budget();
    }

    fn line(&self) -> usize {
        self.line
    }

    fn size(&self) -> usize {
        self.fields.len()
    }

    fn path(&self, i: usize) -> &[u8] {
        self.resolve(self.fields[i].path)
    }

    fn value(&self, i: usize) -> &[u8] {
        self.resolve(self.fields[i].value)
    }

    fn used(&self, i: usize) -> bool {
        self.fields[i].used.get()
    }

    fn mark_used(&self, i: usize) {
        self.fields[i].used.set(true);
    }

    fn mark_all_used(&self) {
        for field in &self.fields {
            field.used.set(true);
        }
    }

    // The first field at `wanted`, marked as read; None when there is none.
    fn take(&self, wanted: &[u8]) -> Option<&[u8]> {
        for field in &self.fields {
            if field.path.size() == wanted.len() && same_text(self.resolve(field.path), wanted) {
                field.used.set(true);
                return Some(self.resolve(field.value));
            }
        }
        None
    }

    // True when any field lies under `prefix` ("Circle" finds "Circle/...").
    fn has(&self, prefix: &[u8]) -> bool {
        self.fields.iter().any(|field| {
            if field.path.size() < prefix.len() {
                return false;
            }
            let path = self.resolve(field.path);
            path.starts_with(prefix) && (path.len() == prefix.len() || path[prefix.len()] == b'/')
        })
    }

    // A few additions and a comparison, after each change that can grow what
    // the record holds. Checked, not reserved against: a record is almost
    // always a few hundred bytes.
    fn check_budget(&mut self) {
        if self.arena.len()
            + self.path.len()
            + self.pending.len()
            + self.viewed
            + self.fields.len() * Self::FIELD_BYTES
            <= Self::BUDGET_BYTES
        {
            return;
        }
        self.over_budget = true;
        self.arena = Vec::new();
        self.fields = Vec::new();
        self.path = Vec::new();
        self.pending = Vec::new();
        self.pending_mode = Pending::None;
    }

    fn resolve(&self, span: Span<'a>) -> &[u8] {
        match span {
            Span::Document(view) => view,
            Span::Arena { offset, size } => {
                &self.arena[offset as usize..offset as usize + size as usize]
            }
        }
    }
}

// ---- What earlier records said, by record ID ----------------------------------------

#[derive(Clone, Debug, Default)]
struct InstrumentRecord {
    r#type: String,
    model: String,
    serial: String,
    firmware: String,
}

#[derive(Clone, Copy, Debug, Default)]
struct AtmosphereRecord {
    pressure: Option<f64>,
    temperature: Option<f64>,
    ppm: Option<f64>,
    refraction_coefficient: Option<f64>,
    apply_curvature: Option<bool>,
    apply_refraction: Option<bool>,
    apply_ppm_to_raw_distances: Option<bool>,
}

#[derive(Clone, Debug, Default)]
struct TargetRecord {
    prism_constant: Option<f64>,
    target_height: Option<f64>,
    prism_type: String,
}

#[derive(Clone, Debug, Default)]
struct AntennaRecord {
    measured_height: Option<f64>,
    reduced_height: Option<f64>,
    // Read, so that the element counts as read, and kept as the C++ keeps it;
    // no value of the model takes it.
    #[allow(
        dead_code,
        reason = "The C++ record holds it and uses it nowhere; reading it marks the element read."
    )]
    measurement_type: String,
    equipment_id: String,
}

#[derive(Clone, Debug, Default)]
struct EquipmentRecord {
    antenna_type: String,
    antenna_serial: String,
    measurement_method: String,
}

#[derive(Clone, Debug, Default)]
struct ReferenceRecord {
    point_name: String,
    antenna_id: String,
}

#[derive(Clone, Copy, Debug)]
struct StationState {
    // Into project.stations.
    index: usize,
    next_pointing: usize,
    height_known: bool,
    atmosphere_difference_warned: bool,
    reorientation_warned: bool,
    backsight_set: bool,
}

const NONE: usize = usize::MAX;

// Where a point name stands: its coordinate record (if any), its named-only
// entry (if any - made for an observation before or without coordinates),
// and whether a point record has described it yet.
#[derive(Clone, Copy, Debug)]
struct PointSlot {
    positioned: usize,
    named: usize,
    // The kept coordinate record is a control one.
    control: bool,
    // Its method, survey method and class are in its metadata.
    described: bool,
    // In SurveyProject::control_points already. A flag, not a search of that
    // list: a control list imported into a job is thousands of marks, and a
    // search per mark made reading it grow with the square of its length.
    control_listed: bool,
}

impl Default for PointSlot {
    fn default() -> Self {
        Self {
            positioned: NONE,
            named: NONE,
            control: false,
            described: false,
            control_listed: false,
        }
    }
}

// What was not read, for the warnings at the end.
#[derive(Clone, Debug, Default)]
struct Unread {
    records: usize,
    last_serial: usize,
    leaves: BTreeSet<String>,
}

// What a point record names and says of its point, for the named entry
// describe_named makes or completes.
struct Described<'r> {
    name: String,
    code: &'r str,
    description1: &'r str,
    extra: BTreeMap<String, String>,
    line: usize,
}

// ---- The reader ------------------------------------------------------------------
//
// The C++ keeps the setup and the target the previous shot named, as pointers
// into node-based maps, so that a run of shots skips their look-up. A Rust map
// moves its values as it grows, so the port looks them up for every shot: one
// hash of a short ID, and the same answer, since a later record with the same
// ID replaces the value the pointer saw. Likewise the C++'s pointer to the
// slot a point record touched is the slot of the record's name, looked up
// here.
struct JobXmlReader<'a> {
    file_name: String,
    options: &'a ReadOptions,
    xml: XmlPullReader<'a>,
    lines: LineCounter<'a>,
    result: ReadResult,
    base_source: SourceRecord,

    instruments: HashMap<String, InstrumentRecord>,
    atmospheres: HashMap<String, AtmosphereRecord>,
    targets: HashMap<String, TargetRecord>,
    antennas: HashMap<String, AntennaRecord>,
    equipment: HashMap<String, EquipmentRecord>,
    references: HashMap<String, ReferenceRecord>,
    stations: HashMap<String, StationState>,
    // BackBearingRecord ID -> StationRecord ID.
    back_bearing_stations: HashMap<String, String>,
    // Station name -> setups on it.
    occupations: HashMap<String, usize>,

    positioned: Vec<SurveyPoint>,
    named: Vec<UnpositionedPoint>,
    points: HashMap<String, PointSlot>,
    // Observations on the previous setup, a capacity hint.
    last_setup_shots: usize,

    // What was not read, for the warnings at the end.
    skipped_kinds: BTreeMap<String, usize>,
    unread: BTreeMap<String, Unread>,
    record_serial: usize,
    deleted_ids: Vec<String>,
    deleted_count: usize,
    mean_turned_angles: usize,
    keyed_global: usize,
    reductions_unknown: usize,
    setups_without_atmosphere: usize,
    shots_without_target_height: usize,
}

impl<'a> JobXmlReader<'a> {
    fn new(bytes: &'a [u8], file_name: &str, options: &'a ReadOptions) -> Self {
        Self {
            file_name: String::from(file_name),
            options,
            xml: XmlPullReader::new(bytes),
            lines: LineCounter::new(bytes),
            result: ReadResult::default(),
            base_source: SourceRecord::default(),
            instruments: HashMap::new(),
            atmospheres: HashMap::new(),
            targets: HashMap::new(),
            antennas: HashMap::new(),
            equipment: HashMap::new(),
            references: HashMap::new(),
            stations: HashMap::new(),
            back_bearing_stations: HashMap::new(),
            occupations: HashMap::new(),
            positioned: Vec::new(),
            named: Vec::new(),
            points: HashMap::new(),
            last_setup_shots: 0,
            skipped_kinds: BTreeMap::new(),
            unread: BTreeMap::new(),
            record_serial: 0,
            deleted_ids: Vec::new(),
            deleted_count: 0,
            mean_turned_angles: 0,
            keyed_global: 0,
            reductions_unknown: 0,
            setups_without_atmosphere: 0,
            shots_without_target_height: 0,
        }
    }

    // ---- plumbing

    fn warn(&mut self, line: usize, message: String) {
        self.result.warnings.push(ReadWarning {
            file_name: self.file_name.clone(),
            record: line,
            message,
        });
    }

    fn source_at(&self, line: usize) -> SourceRecord {
        SourceRecord {
            record_number: line,
            ..self.base_source.clone()
        }
    }

    fn number(&mut self, record: &RecordBuffer<'_>, path: &str) -> Option<f64> {
        let found = record.take(path.as_bytes())?;
        if trimmed(found).is_empty() {
            return None; // absent, or the schema's "null" (an empty element)
        }
        let value = parse_number(found);
        if value.is_none() {
            self.warn(
                record.line(),
                format!(
                    "{path} '{}' is not a number and was read as absent",
                    shown(found, 40)
                ),
            );
        }
        value
    }

    fn flag(&mut self, record: &RecordBuffer<'_>, path: &str) -> Option<bool> {
        let value = trimmed(record.take(path.as_bytes())?);
        if value == b"true" {
            return Some(true);
        }
        if value == b"false" {
            return Some(false);
        }
        if !value.is_empty() {
            self.warn(
                record.line(),
                format!(
                    "{path} '{}' is neither true nor false and was read as absent",
                    shown(value, 20)
                ),
            );
        }
        None
    }

    fn text_of(record: &RecordBuffer<'_>, path: &str) -> String {
        String::from(text(trimmed(
            record.take(path.as_bytes()).unwrap_or_default(),
        )))
    }

    fn note_unread(&mut self, record_name: &str, record: &RecordBuffer<'_>) {
        self.record_serial += 1;
        for i in 0..record.size() {
            if record.used(i) {
                continue;
            }
            let path = record.path(i);
            let slash = path.iter().position(|&c| c == b'/');
            let key = format!(
                "{record_name} {}",
                text(&path[..slash.unwrap_or(path.len())])
            );
            let entry = self.unread.entry(key).or_default();
            if entry.last_serial != self.record_serial {
                entry.last_serial = self.record_serial;
                entry.records += 1;
            }
            if slash.is_some() && entry.leaves.len() < 12 {
                let leaf_start = path.iter().rposition(|&c| c == b'/').map_or(0, |at| at + 1);
                let leaf = text(&path[leaf_start..]);
                if !entry.leaves.contains(leaf) {
                    entry.leaves.insert(String::from(leaf));
                }
            }
        }
    }

    fn decode_failure(&mut self, error: &Error) -> Error {
        Error::new(
            ErrorCode::FileImportFailure,
            format!(
                "{} line {}: {}",
                self.file_name,
                self.lines.line_at(self.xml.offset()),
                error.message
            ),
        )
        .with_context(error.context.clone())
    }

    // Reads the element just started (at `element_depth`) to its end into
    // `record`. An error is the XML's.
    fn gather(&mut self, record: &mut RecordBuffer<'a>, element_depth: usize) -> Result<()> {
        loop {
            match self.xml.next() {
                Event::StartElement => {
                    record.open(self.xml.name());
                    if !self.xml.has_attributes() || record.over_budget() {
                        continue;
                    }
                    // The schema puts attributes below record level on two
                    // elements only - Feature@Name and InformationGroup@name -
                    // and both are gathered, so a feature's name is read like
                    // any other value.
                    for attribute_name in [b"Name".as_slice(), b"name".as_slice()] {
                        let Some(raw) = self.xml.attribute(attribute_name) else {
                            continue;
                        };
                        let mut decoded = Vec::new();
                        if let Err(error) = append_xml_text(raw, &mut decoded) {
                            return Err(self.decode_failure(&error));
                        }
                        record.attribute(attribute_name, &decoded);
                    }
                }
                Event::Text => {
                    if !record.add_text(self.xml.text(), self.xml.text_has_reference()) {
                        let error = record.decode_error().clone();
                        return Err(self.decode_failure(&error));
                    }
                }
                Event::EndElement => {
                    if self.xml.depth() == element_depth {
                        return Ok(());
                    }
                    record.close();
                }
                Event::EndOfDocument => {
                    return Err(Error::new(
                        ErrorCode::FileImportFailure,
                        format!(
                            "{} ends inside a record, so it is incomplete",
                            self.file_name
                        ),
                    ));
                }
                Event::Error => {
                    return Err(Error::new(
                        ErrorCode::FileImportFailure,
                        format!("{}: {}", self.file_name, self.xml.error().message),
                    )
                    .with_context(self.xml.error().context.clone()));
                }
            }
        }
    }

    // A record past RecordBuffer::BUDGET_BYTES was read through but not kept.
    // Its element `name` comes from the file, so a crafted one is shortened.
    fn warn_over_budget(&mut self, name: &[u8], kind: &str, line: usize) {
        let mut name_shown = shown(name, 64);
        if name.len() > 64 {
            name_shown.push_str("...");
        }
        self.warn(
            line,
            format!(
                "the {name_shown}{kind} holds more than {} MiB of element names and values, \
                 thousands of times what a controller writes, and was not read; the file may be \
                 damaged",
                RecordBuffer::BUDGET_MEBIBYTES
            ),
        );
    }

    // ---- Points

    fn slot_for(&mut self, name: &str) -> &mut PointSlot {
        self.points.entry(String::from(name)).or_default()
    }

    fn positioned_index(&self, name: &str) -> Option<usize> {
        self.points
            .get(name)
            .map(|slot| slot.positioned)
            .filter(|&index| index != NONE)
    }

    fn named_index(&self, name: &str) -> Option<usize> {
        self.points
            .get(name)
            .map(|slot| slot.named)
            .filter(|&index| index != NONE)
    }

    fn metadata_of(&mut self, name: &str) -> Option<&mut BTreeMap<String, String>> {
        if let Some(index) = self.positioned_index(name) {
            return Some(&mut self.positioned[index].metadata);
        }
        if let Some(index) = self.named_index(name) {
            return Some(&mut self.named[index].metadata);
        }
        None
    }

    fn ensure_named(&mut self, name: &str, line: usize) {
        if name.is_empty() {
            return;
        }
        let next = self.named.len();
        let slot = self.slot_for(name);
        if slot.positioned != NONE || slot.named != NONE {
            return;
        }
        slot.named = next;
        let source = self.source_at(line);
        self.named.push(UnpositionedPoint {
            id: String::from(name),
            source,
            ..Default::default()
        });
    }

    // True when `point` is kept, false when an earlier record for its name is.
    fn add_positioned(&mut self, point: SurveyPoint, control: bool, record_id: &str) -> bool {
        let next = self.positioned.len();
        let slot = self.slot_for(&point.id);
        if slot.positioned == NONE {
            slot.positioned = next;
            slot.control = control;
            self.positioned.push(point);
            return true;
        }
        // Two coordinate records for one name. Trimble's own search rules
        // prefer a control point to any other and otherwise the first stored;
        // the one not kept is named in the warning, coordinates and all, so it
        // is not lost.
        let replace = control && !slot.control;
        let kept_index = slot.positioned;
        if replace {
            slot.control = true;
        }
        let kept = &self.positioned[kept_index];
        let dropped = if replace { kept } else { &point };
        let mut message = format!(
            "point {} has coordinates in more than one record; kept {} and not the one at N {} \
             E {}",
            point.id,
            if replace {
                format!("the control record {record_id}")
            } else {
                String::from("the first")
            },
            format_exact_real(dropped.northing),
            format_exact_real(dropped.easting)
        );
        if let Some(elevation) = dropped.elevation {
            message.push_str(&format!(" H {}", format_exact_real(elevation)));
        }
        let line = point.source.record_number;
        self.warn(line, message);
        if replace {
            self.positioned[kept_index] = point;
        }
        replace
    }

    // A value the survey model would refuse (validate_project) is a warning
    // about this record, not a reason to lose the whole file.
    fn add_observation(&mut self, observation: Observation, line: usize) -> bool {
        if let Err(error) = validate_observation(&observation) {
            self.warn(
                line,
                format!(
                    "{} not imported: {}",
                    observation_kind_name(&observation),
                    error.message
                ),
            );
            return false;
        }
        self.result.project.observations.push(observation);
        true
    }

    fn antenna_for(&self, antenna_id: &str) -> GnssAntenna {
        let mut antenna = GnssAntenna::default();
        let Some(record) = self.antennas.get(antenna_id) else {
            return antenna;
        };
        antenna.height = record.measured_height.unwrap_or(0.0);
        let mut method = String::new();
        if let Some(gear) = self.equipment.get(&record.equipment_id) {
            antenna.r#type = gear.antenna_type.clone();
            antenna.serial_number = gear.antenna_serial.clone();
            method = gear.measurement_method.clone();
        }
        antenna.measured_to = method.clone();
        if let Some(reduced) = record.reduced_height {
            // ReducedHeight is the controller's vertical height from the mark
            // to the antenna phase centre - measured against a real export: the
            // APC of an ECEF position lies exactly ReducedHeight above the mark
            // the controller reduced it to (0.0000 m horizontally, 0.0000 m in
            // height). It is what a reduction needs, whatever the words for the
            // measuring point were, so it is carried as a phase-centre height
            // and the measured one is kept in the words.
            antenna.height = reduced;
            antenna.method = AntennaHeightMethod::PhaseCentre;
            let mut words = if method.is_empty() {
                String::from("measured")
            } else {
                format!("{method}, measured")
            };
            if let Some(measured) = record.measured_height {
                words.push_str(&format!(" {} m", format_exact_real(measured)));
            }
            words.push_str("; height reduced to the phase centre by the controller");
            antenna.measured_to = words;
            return antenna;
        }
        // Without it, the words decide, and they are Trimble's own - and
        // translated: a Polish controller writes "Spod mocowania anteny" for
        // the bottom of the antenna mount. Only unambiguous English names are
        // mapped; anything else is Other with the words kept in measured_to,
        // because a slant height read as a vertical one is an error nobody
        // sees.
        let squeezed: String = method
            .chars()
            .filter(|&c| c != ' ')
            .map(|c| c.to_ascii_lowercase())
            .collect();
        antenna.method = if method.is_empty() {
            AntennaHeightMethod::Unknown
        } else if squeezed.contains("phasecent") {
            AntennaHeightMethod::PhaseCentre
        } else if squeezed.contains("bumper")
            || squeezed.contains("slant")
            || squeezed.contains("notch")
        {
            AntennaHeightMethod::Slant
        } else if squeezed == "bottomofantennamount" {
            AntennaHeightMethod::Vertical
        } else {
            AntennaHeightMethod::Other
        };
        antenna
    }

    // ---- FieldBook records

    fn keep_as_metadata(&mut self, prefix: &str, record: &RecordBuffer<'_>) {
        // Configuration a later record may override: the latest wins, which
        // is what the job's own state was at export.
        for i in 0..record.size() {
            record.mark_used(i);
            let key = format!("{prefix}.{}", text(record.path(i)));
            self.result
                .project
                .metadata
                .insert(key, String::from(text(record.value(i))));
        }
    }

    // Each record counts once in ReadResult: read when anything of it went
    // into the project (a setup, an observation, a point, a feature, a
    // definition later records use, metadata), skipped otherwise. So the
    // record readers below return whether they imported the record, and this
    // does the counting; a record they refuse has had its warning already.
    fn read_field_book_record(
        &mut self,
        name: &str,
        id: &str,
        time_stamp: &[u8],
        record: &RecordBuffer<'_>,
    ) {
        let mut imported = true;
        match name {
            "PointRecord" => imported = self.read_point_record(id, record),
            "StationRecord" => imported = self.read_station_record(id, time_stamp, record),
            "BackBearingRecord" => imported = self.read_back_bearing_record(id, record),
            "TargetRecord" => {
                let prism_type = Self::text_of(record, "PrismType");
                let prism_constant = self.number(record, "PrismConstant");
                let target_height = self.number(record, "TargetHeight");
                self.targets.insert(
                    String::from(id),
                    TargetRecord {
                        prism_constant,
                        target_height,
                        prism_type,
                    },
                );
            }
            "InstrumentRecord" => {
                let instrument = InstrumentRecord {
                    r#type: Self::text_of(record, "Type"),
                    model: Self::text_of(record, "Model"),
                    serial: Self::text_of(record, "Serial"),
                    firmware: Self::text_of(record, "FirmwareVersion"),
                };
                self.instruments.insert(String::from(id), instrument);
            }
            "AtmosphereRecord" => {
                let pressure = self.number(record, "Pressure");
                let temperature = self.number(record, "Temperature");
                let ppm = self.number(record, "PPM");
                let refraction_coefficient = self.number(record, "RefractionCoefficient");
                let apply_curvature = self.flag(record, "ApplyEarthCurvatureCorrection");
                let apply_refraction = self.flag(record, "ApplyRefractionCorrection");
                let apply_ppm_to_raw_distances = self.flag(record, "ApplyPPMToRawDistances");
                self.atmospheres.insert(
                    String::from(id),
                    AtmosphereRecord {
                        pressure,
                        temperature,
                        ppm,
                        refraction_coefficient,
                        apply_curvature,
                        apply_refraction,
                        apply_ppm_to_raw_distances,
                    },
                );
            }
            "AntennaRecord" => {
                let measured_height = self.number(record, "MeasuredHeight");
                let reduced_height = self.number(record, "ReducedHeight");
                let antenna = AntennaRecord {
                    measured_height,
                    reduced_height,
                    measurement_type: Self::text_of(record, "MeasurementType"),
                    equipment_id: Self::text_of(record, "GPSEquipmentID"),
                };
                self.antennas.insert(String::from(id), antenna);
            }
            "GPSEquipmentRecord" => {
                let gear = EquipmentRecord {
                    antenna_type: Self::text_of(record, "AntennaType"),
                    antenna_serial: Self::text_of(record, "AntennaSerialNumber"),
                    measurement_method: Self::text_of(record, "AntennaMeasurementMethod"),
                };
                self.equipment.insert(String::from(id), gear);
            }
            "ReferenceRecord" => {
                let reference = ReferenceRecord {
                    point_name: Self::text_of(record, "PointName"),
                    antenna_id: Self::text_of(record, "AntennaID"),
                };
                self.references.insert(String::from(id), reference);
            }
            "NoteRecord" => {
                let mut notes = String::new();
                for i in 0..record.size() {
                    if record.path(i) == b"Notes/Note" {
                        record.mark_used(i);
                        if !notes.is_empty() {
                            notes.push('\n');
                        }
                        notes.push_str(text(record.value(i)));
                    }
                }
                self.result
                    .project
                    .metadata
                    .insert(format!("jxl.note.{id}"), notes);
            }
            "LineRecord" => imported = self.read_line_record(id, record),
            "UnitsRecord"
            | "CorrectionsRecord"
            | "CoordinateSystemRecord"
            | "JobPropertiesRecord"
            | "FeatureCodingRecord"
            | "TimeZoneRecord" => self.keep_as_metadata(&format!("jxl.{name}"), record),
            _ => {
                *self.skipped_kinds.entry(String::from(name)).or_default() += 1;
                self.result.records_skipped += 1;
                return;
            }
        }
        if !imported {
            self.result.records_skipped += 1;
            return;
        }
        self.result.records_read += 1;
        self.note_unread(name, record);
    }

    fn read_station_record(
        &mut self,
        id: &str,
        time_stamp: &[u8],
        record: &RecordBuffer<'_>,
    ) -> bool {
        let line = record.line();
        let station_name = Self::text_of(record, "StationName");
        if station_name.is_empty() {
            self.warn(
                line,
                String::from("a station record with no station name was not read"),
            );
            record.mark_all_used();
            return false;
        }
        self.ensure_named(&station_name, line);

        let occupation = {
            let count = self.occupations.entry(station_name.clone()).or_default();
            *count += 1;
            *count
        };
        let height = self.number(record, "TheodoliteHeight");
        let mut station = SurveyStation {
            setup: Station {
                id: format!("{station_name}#{occupation}"),
                point_id: station_name.clone(),
                instrument_height: height.unwrap_or(0.0),
            },
            source: self.source_at(line),
            ..Default::default()
        };
        station
            .metadata
            .insert(String::from("jxl.recordId"), String::from(id));

        let settings = &mut station.instrument;
        settings.time = parse_timestamp(time_stamp);
        let instrument_id = Self::text_of(record, "InstrumentID");
        if let Some(instrument) = self.instruments.get(&instrument_id) {
            settings.model = if instrument.model.is_empty() {
                instrument.r#type.clone()
            } else {
                instrument.model.clone()
            };
            settings.serial_number = instrument.serial.clone();
            if instrument.r#type.starts_with("Trimble") {
                settings.make = String::from("Trimble");
            }
            station.metadata.insert(
                String::from("jxl.instrumentType"),
                instrument.r#type.clone(),
            );
            if !instrument.firmware.is_empty() {
                station.metadata.insert(
                    String::from("jxl.firmwareVersion"),
                    instrument.firmware.clone(),
                );
            }
        } else if !instrument_id.is_empty() {
            self.warn(
                line,
                format!(
                    "setup on {station_name} names instrument record {instrument_id}, which is \
                     not in the file"
                ),
            );
        }
        let settings = &mut station.instrument;
        let atmosphere_id = Self::text_of(record, "AtmosphereID");
        if let Some(atmosphere) = self.atmospheres.get(&atmosphere_id).copied() {
            settings.pressure_hectopascals = atmosphere.pressure;
            settings.temperature_celsius = atmosphere.temperature;
            settings.atmospheric_ppm = atmosphere.ppm;
            // The schema: raw EDM distances carry no atmospheric correction
            // unless ApplyPPMToRawDistances is false, which says it is already
            // in them.
            settings.atmospheric_ppm_state =
                if atmosphere.apply_ppm_to_raw_distances.unwrap_or(true) {
                    CorrectionState::NotApplied
                } else {
                    CorrectionState::Applied
                };
            settings.refraction_coefficient = atmosphere.refraction_coefficient;
            let flag_text = |value: bool| String::from(if value { "true" } else { "false" });
            if let Some(apply) = atmosphere.apply_curvature {
                station.metadata.insert(
                    String::from("jxl.applyEarthCurvatureCorrection"),
                    flag_text(apply),
                );
            }
            if let Some(apply) = atmosphere.apply_refraction {
                station.metadata.insert(
                    String::from("jxl.applyRefractionCorrection"),
                    flag_text(apply),
                );
            }
        } else {
            self.setups_without_atmosphere += 1;
        }
        // The schema: raw vertical circle readings carry no curvature or
        // refraction correction and raw EDM distances no scale factor - both
        // are applied only in the controller's own reductions.
        station.instrument.curvature_refraction_state = CorrectionState::NotApplied;
        station.instrument.scale_factor = self.number(record, "ScaleFactor");
        station.instrument.scale_factor_state = CorrectionState::NotApplied;
        for key in [
            "StationType",
            "ScaleType",
            "NumberOfBacksightMeasurements",
            "NumberOfBacksightPoints",
            "ScaleFactorStandardError",
        ] {
            if let Some(value) = record.take(key.as_bytes()) {
                station
                    .metadata
                    .insert(format!("jxl.{key}"), String::from(text(trimmed(value))));
            }
        }
        if height.is_none() {
            self.warn(
                line,
                format!("setup on {station_name} has no instrument height; 0 was used"),
            );
        }

        // Setups in one job tend to have alike numbers of shots, so the last
        // one's count saves growing the list a dozen times over, each a copy
        // of every observation so far. Capped, so one very long setup followed
        // by many short ones reserves too much only once.
        if let Some(last) = self.result.project.stations.last() {
            self.last_setup_shots = last.observations.len();
        }
        station
            .observations
            .reserve(self.last_setup_shots.min(1024));

        let state = StationState {
            index: self.result.project.stations.len(),
            next_pointing: 1,
            height_known: height.is_some(),
            atmosphere_difference_warned: false,
            reorientation_warned: false,
            backsight_set: false,
        };
        self.stations.insert(String::from(id), state);
        self.result.project.stations.push(station);
        true
    }

    fn read_back_bearing_record(&mut self, id: &str, record: &RecordBuffer<'_>) -> bool {
        let line = record.line();
        let station_id = Self::text_of(record, "StationRecordID");
        let backsight = Self::text_of(record, "BackSight");
        let face1 = self.number(record, "Face1HorizontalCircle");
        let face2 = self.number(record, "Face2HorizontalCircle");
        let correction = record.take(b"OrientationCorrection");
        record.take(b"Station"); // repeats the StationRecord's name

        let Some(state) = self.stations.get(&station_id).copied() else {
            self.warn(
                line,
                format!(
                    "backsight record {id} names setup record {station_id}, which is not in the \
                     file; it was not read"
                ),
            );
            return false;
        };
        self.back_bearing_stations
            .insert(String::from(id), station_id.clone());
        if state.backsight_set {
            // One SurveyStation holds one orientation. A re-orientation within
            // a setup is kept in metadata for the reduction and said aloud.
            let station = &mut self.result.project.stations[state.index];
            station.metadata.insert(
                format!("jxl.reorientation.{id}"),
                format!(
                    "{backsight} {}",
                    face1.map_or_else(|| String::from("-"), format_exact_real)
                ),
            );
            let setup_id = station.setup.id.clone();
            if !state.reorientation_warned {
                if let Some(state) = self.stations.get_mut(&station_id) {
                    state.reorientation_warned = true;
                }
                self.warn(
                    line,
                    format!(
                        "setup {setup_id} was oriented more than once; the first orientation is \
                         used and the later ones are in the setup's metadata"
                    ),
                );
            }
            return true;
        }
        if let Some(state) = self.stations.get_mut(&station_id) {
            state.backsight_set = true;
        }
        let point_id = self.result.project.stations[state.index]
            .setup
            .point_id
            .clone();
        if !backsight.is_empty() && backsight != point_id {
            self.ensure_named(&backsight, line);
            self.result.project.stations[state.index].backsight_point_id = backsight;
        }
        let station = &mut self.result.project.stations[state.index];
        // The circle reading observed on the backsight (face 1), which is what
        // SurveyStation::backsight_azimuth holds: the reading, not the azimuth.
        if let Some(face1) = face1 {
            station.backsight_azimuth = Some(wrap_two_pi(face1 * RADIANS_PER_DEGREE));
        }
        if let Some(face2) = face2 {
            station.metadata.insert(
                String::from("jxl.backsightFace2Circle_deg"),
                format_exact_real(face2),
            );
        }
        if let Some(correction) = correction
            && !trimmed(correction).is_empty()
        {
            station.metadata.insert(
                String::from("jxl.orientationCorrection_deg"),
                String::from(text(trimmed(correction))),
            );
        }
        true
    }

    fn read_line_record(&mut self, id: &str, record: &RecordBuffer<'_>) -> bool {
        let line = record.line();
        if self.flag(record, "Deleted").unwrap_or(false) {
            self.deleted_count += 1;
            if self.deleted_ids.len() < 10 {
                self.deleted_ids.push(String::from(id));
            }
            record.mark_all_used();
            return false;
        }
        let mut feature = SurveyFeature {
            name: Self::text_of(record, "Name"),
            code: Self::text_of(record, "Code"),
            ..Default::default()
        };
        let method = Self::text_of(record, "Method");
        let start = Self::text_of(record, "StartPoint");
        let end = Self::text_of(record, "EndPoint");
        if method != "TwoPoints" {
            self.warn(
                line,
                format!(
                    "line {} is defined by {}, not by two points, and was not read",
                    feature.name,
                    if method.is_empty() {
                        "no method"
                    } else {
                        method.as_str()
                    }
                ),
            );
            record.mark_all_used();
            return false;
        }
        if start.is_empty() || end.is_empty() || start == end {
            self.warn(
                line,
                format!(
                    "line {} does not join two different points and was not read",
                    feature.name
                ),
            );
            record.mark_all_used();
            return false;
        }
        self.ensure_named(&start, line);
        self.ensure_named(&end, line);
        feature.point_ids = vec![start, end];
        feature.source = self.source_at(line);
        feature
            .metadata
            .insert(String::from("jxl.recordId"), String::from(id));
        self.result.project.features.push(feature);
        true
    }

    fn read_point_record(&mut self, id: &str, record: &RecordBuffer<'_>) -> bool {
        // Feature and attribute library values, notes and how the point was
        // measured go to point metadata, and are built only for a point no
        // point record has described yet (or a record that carries features or
        // notes): a later shot to the same point would only try to add keys
        // the point already has, and building them per shot to throw them away
        // was a fifth of the read time of a large job.
        let name = String::from(text(trimmed(record.take(b"Name").unwrap_or_default())));
        let fresh = self.points.get(&name).is_none_or(|slot| !slot.described);
        let imported = self.read_point_record_body(id, record, fresh);
        // The body leaves the slot it named or positioned; a record it refused
        // may leave none.
        if fresh
            && !name.is_empty()
            && let Some(slot) = self.points.get(&name).copied()
        {
            let metadata = if slot.positioned != NONE {
                Some(&self.positioned[slot.positioned].metadata)
            } else if slot.named != NONE {
                Some(&self.named[slot.named].metadata)
            } else {
                None
            };
            let described = metadata.is_some_and(|metadata| metadata.contains_key("jxl.method"));
            if let Some(slot) = self.points.get_mut(&name) {
                slot.described = described;
            }
        }
        imported
    }

    // Names the point (unless it has coordinates) and gives its named entry
    // the code, description and metadata of this record where it has none.
    fn describe_named(&mut self, described: &mut Described<'_>) {
        let next = self.named.len();
        let slot = self.slot_for(&described.name);
        if slot.positioned == NONE && slot.named == NONE {
            slot.named = next;
            let source = self.source_at(described.line);
            self.named.push(UnpositionedPoint {
                id: described.name.clone(),
                source,
                ..Default::default()
            });
        }
        let Some(index) = self.named_index(&described.name) else {
            return;
        };
        let point = &mut self.named[index];
        if point.code.is_empty() {
            point.code = String::from(described.code);
        }
        if point.description.is_empty() {
            point.description = String::from(described.description1);
        }
        if point.metadata.is_empty() {
            point.metadata = std::mem::take(&mut described.extra);
        } else {
            for (key, value) in &described.extra {
                point
                    .metadata
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
        }
    }

    // A value the survey model would refuse (validate_project) is a warning
    // about this record, not a reason to lose the whole file.
    fn keep_or_refuse(
        &mut self,
        station: usize,
        observation: Observation,
        plainly_valid: bool,
        line: usize,
    ) {
        if !plainly_valid && let Err(error) = validate_observation(&observation) {
            self.warn(
                line,
                format!(
                    "{} not imported: {}",
                    observation_kind_name(&observation),
                    error.message
                ),
            );
            return;
        }
        self.result.project.stations[station]
            .observations
            .push(observation);
    }

    // QualityControl2 is an X/Y/Z variance-covariance matrix (m^2, 1 sigma,
    // the schema says), which is the frame of an ECEF value; QualityControl3
    // is north/east/elevation, the frame of a latitude/longitude one.
    fn covariance_xyz(
        &mut self,
        record: &RecordBuffer<'_>,
        name: &str,
        line: usize,
    ) -> GnssCovariance3 {
        let mut covariance = GnssCovariance3::default();
        if !record.has(b"QualityControl2") {
            return covariance;
        }
        let xx = self.number(record, "QualityControl2/VCVxx");
        let xy = self.number(record, "QualityControl2/VCVxy");
        let xz = self.number(record, "QualityControl2/VCVxz");
        let yy = self.number(record, "QualityControl2/VCVyy");
        let yz = self.number(record, "QualityControl2/VCVyz");
        let zz = self.number(record, "QualityControl2/VCVzz");
        if let (Some(xx), Some(xy), Some(xz), Some(yy), Some(yz), Some(zz)) =
            (xx, xy, xz, yy, yz, zz)
            && xx > 0.0
            && yy > 0.0
            && zz > 0.0
        {
            covariance = GnssCovariance3 {
                xx,
                yy,
                zz,
                xy,
                xz,
                yz,
            };
        } else {
            self.warn(
                line,
                format!(
                    "the variance-covariance matrix of {name} is incomplete or not positive and \
                     was not used"
                ),
            );
        }
        covariance
    }

    // The C++'s lambda of the same name: what it captures is passed.
    fn global_position(
        &mut self,
        mut position: GnssGlobalPositionObservation,
        described: &mut Described<'_>,
        antenna_id: &str,
        solution: GnssSolution,
    ) -> bool {
        self.describe_named(described);
        position.point = described.name.clone();
        position.antenna = self.antenna_for(antenna_id);
        position.solution = solution;
        position.source = self.source_at(described.line);
        self.add_observation(position.into(), described.line)
    }

    fn read_point_record_body(&mut self, id: &str, record: &RecordBuffer<'_>, fresh: bool) -> bool {
        let line = record.line();
        if self.flag(record, "Deleted").unwrap_or(false) {
            self.deleted_count += 1;
            if self.deleted_ids.len() < 10 {
                self.deleted_ids.push(String::from(id));
            }
            record.mark_all_used();
            return false;
        }
        // Views into the record buffer, which holds still until the next
        // record.
        let view = |path: &str| text(trimmed(record.take(path.as_bytes()).unwrap_or_default()));
        let name = String::from(view("Name"));
        if name.is_empty() {
            self.warn(
                line,
                String::from("a point record with no name was not read"),
            );
            record.mark_all_used();
            return false;
        }
        let code = view("Code");
        let method = view("Method");
        let survey_method = view("SurveyMethod");
        let classification = view("Classification");
        let description1 = view("Description1");
        let description2 = view("Description2");

        let mut extra: BTreeMap<String, String> = BTreeMap::new();
        if fresh || record.has(b"Features") || record.has(b"Notes") {
            let mut attribute_name = String::new();
            for i in 0..record.size() {
                let path = record.path(i);
                if path == b"Features/Feature@Name" {
                    record.mark_used(i);
                    let features = extra.entry(String::from("feature")).or_default();
                    if !features.is_empty() {
                        features.push_str(", ");
                    }
                    features.push_str(text(record.value(i)));
                } else if path == b"Features/Feature/Attribute/Name" {
                    record.mark_used(i);
                    attribute_name = String::from(text(record.value(i)));
                } else if path == b"Features/Feature/Attribute/Value" {
                    record.mark_used(i);
                    extra.insert(
                        format!("attribute.{attribute_name}"),
                        String::from(text(record.value(i))),
                    );
                } else if path == b"Features/Feature/Attribute/Type"
                    || path == b"Features/Feature/Attribute/ItemIndex"
                {
                    record.mark_used(i);
                } else if path == b"Notes/Note" {
                    record.mark_used(i);
                    let notes = extra.entry(String::from("note")).or_default();
                    if !notes.is_empty() {
                        notes.push('\n');
                    }
                    notes.push_str(text(record.value(i)));
                }
            }
        }
        if !description2.is_empty() {
            extra.insert(String::from("description2"), String::from(description2));
        }
        // The file a keyed-in point was imported from, as the controller
        // recorded it: a name to show, never a path to open.
        if let Some(imported) = record.take(b"Source")
            && !trimmed(imported).is_empty()
        {
            extra.insert(
                String::from("jxl.source"),
                source_file_name(text(trimmed(imported))),
            );
        }
        if fresh && !method.is_empty() {
            extra.insert(String::from("jxl.method"), String::from(method));
        }
        if fresh && !survey_method.is_empty() {
            extra.insert(
                String::from("jxl.surveyMethod"),
                String::from(survey_method),
            );
        }
        if fresh && !classification.is_empty() {
            extra.insert(
                String::from("jxl.classification"),
                String::from(classification),
            );
        }

        // ---- Grid coordinates: keyed in, copied, or computed by a COGO routine.
        if record.has(b"Grid") {
            let north = self.number(record, "Grid/North");
            let east = self.number(record, "Grid/East");
            let elevation = self.number(record, "Grid/Elevation");
            if let (Some(north), Some(east)) = (north, east) {
                let mut metadata = extra;
                metadata.insert(String::from("jxl.recordId"), String::from(id));
                let point = SurveyPoint {
                    id: name.clone(),
                    northing: north,
                    easting: east,
                    elevation,
                    code: String::from(code),
                    description: String::from(description1),
                    metadata,
                    source: self.source_at(line),
                    coordinate_source: if survey_method == "KeyedIn" || method == "CopiedPoint" {
                        CoordinateSource::Entered
                    } else {
                        CoordinateSource::Calculated
                    },
                };
                let control = classification == "Control";
                // A record whose coordinates lose to an earlier one's is
                // skipped: its values are only in add_positioned's warning.
                let kept = self.add_positioned(point, control, id);
                let slot = self.points.entry(name.clone()).or_default();
                if control && slot.control && !slot.control_listed {
                    slot.control_listed = true;
                    self.result
                        .project
                        .control_points
                        .push(if elevation.is_some() {
                            ControlPoint::fixed3d(name)
                        } else {
                            ControlPoint::fixed_horizontal(name)
                        });
                }
                return kept;
            }
            self.warn(
                line,
                format!(
                    "point {name} has a grid position with no northing or easting; it was read as \
                     a named point without coordinates"
                ),
            );
        }
        let mut described = Described {
            name: name.clone(),
            code,
            description1,
            extra,
            line,
        };

        // ---- Terrestrial observation.
        if record.has(b"Circle") {
            self.describe_named(&mut described);
            let station_id = view("StationID");
            let Some(state) = self.stations.get(station_id).copied() else {
                self.warn(
                    line,
                    format!(
                        "observation to {name} names setup record {station_id}, which is not in \
                         the file; it was not read"
                    ),
                );
                record.mark_all_used();
                return false;
            };
            let at = self.result.project.stations[state.index]
                .setup
                .point_id
                .clone();
            if name == at {
                self.warn(
                    line,
                    format!("observation from {at} to itself was not read"),
                );
                record.mark_all_used();
                return false;
            }
            record.take(b"BackBearingID"); // the setup's orientation, already on the station

            let target_id = view("TargetID");
            let target = self.targets.get(target_id).cloned();
            if target.is_none() && !target_id.is_empty() {
                self.warn(
                    line,
                    format!(
                        "observation to {name} names target record {target_id}, which is not in \
                         the file; target height 0 was used"
                    ),
                );
            }
            let target_height = target
                .as_ref()
                .and_then(|target| target.target_height)
                .unwrap_or(0.0);
            if target
                .as_ref()
                .is_none_or(|target| target.target_height.is_none())
            {
                self.shots_without_target_height += 1;
            }

            // Atmosphere as recorded with the shot: a change within a setup is
            // something the setup-level settings cannot hold, so it is said.
            let pressure = self.number(record, "Pressure");
            let temperature = self.number(record, "Temperature");
            let (setup_pressure, setup_temperature, setup_id, instrument_height) = {
                let station = &self.result.project.stations[state.index];
                (
                    station.instrument.pressure_hectopascals,
                    station.instrument.temperature_celsius,
                    station.setup.id.clone(),
                    station.setup.instrument_height,
                )
            };
            let changed = |shot: Option<f64>, setup: Option<f64>| match (shot, setup) {
                (Some(shot), Some(setup)) => (shot - setup).abs() > 0.05,
                _ => false,
            };
            let pressure_changed = changed(pressure, setup_pressure);
            let temperature_changed = changed(temperature, setup_temperature);
            if (pressure_changed || temperature_changed) && !state.atmosphere_difference_warned {
                if let Some(state) = self.stations.get_mut(station_id) {
                    state.atmosphere_difference_warned = true;
                }
                self.warn(
                    line,
                    format!(
                        "the pressure or temperature recorded with shots from setup {setup_id} \
                         differs from the setup's atmosphere record; the setup's is used"
                    ),
                );
            }

            let face = view("Circle/Face");
            let mut pointing = Pointing::default();
            if let Some(state) = self.stations.get_mut(station_id) {
                pointing.index = state.next_pointing;
                state.next_pointing += 1;
            }
            pointing.face = match face {
                "Face1" => Face::Left,
                "Face2" => Face::Right,
                _ => Face::Unknown,
            };
            let horizontal = self.number(record, "Circle/HorizontalCircle");
            let vertical = self.number(record, "Circle/VerticalCircle");
            let distance = self.number(record, "Circle/EDMDistance");
            let horizontal_error = self.number(record, "Circle/HorizontalCircleStandardError");
            let vertical_error = self.number(record, "Circle/VerticalCircleStandardError");
            let distance_error = self.number(record, "Circle/EDMDistanceStandardError");
            let mode = view("Circle/EDMMeasurementMode");
            let source = self.source_at(line);
            let precision = self.options.precision;

            // Each is held to the survey model's rules for its kind
            // (validate_observation), which here come to a few comparisons -
            // the ids are already known to be distinct and not empty, and
            // every number came through parse_number - and the model's own
            // check is asked only for the wording of a refusal.
            let before = self.result.project.stations[state.index].observations.len();
            if let Some(horizontal) = horizontal {
                let direction = HorizontalDirectionObservation {
                    at: at.clone(),
                    to: name.clone(),
                    direction: wrap_two_pi(circle_degrees(horizontal) * RADIANS_PER_DEGREE),
                    sigma: match horizontal_error {
                        Some(error) if error > 0.0 => error * RADIANS_PER_DEGREE,
                        _ => precision.direction,
                    },
                    source: source.clone(),
                    pointing,
                };
                let plainly_valid =
                    direction.direction.is_finite() && positive_finite(direction.sigma);
                self.keep_or_refuse(state.index, direction.into(), plainly_valid, line);
            }
            if let Some(vertical) = vertical {
                let reading = circle_degrees(vertical);
                let zenith = ZenithAngleObservation {
                    from: at.clone(),
                    to: name.clone(),
                    // A face-2 reading (above 180 degrees) measures the zenith
                    // angle 360 - reading; see the note at the top of this file.
                    angle: (if reading > 180.0 {
                        360.0 - reading
                    } else {
                        reading
                    }) * RADIANS_PER_DEGREE,
                    sigma: match vertical_error {
                        Some(error) if error > 0.0 => error * RADIANS_PER_DEGREE,
                        _ => precision.zenith,
                    },
                    instrument_height,
                    target_height,
                    source: source.clone(),
                    pointing,
                };
                let plainly_valid = (0.0..=PI).contains(&zenith.angle)
                    && zenith.instrument_height.is_finite()
                    && zenith.target_height.is_finite()
                    && positive_finite(zenith.sigma);
                self.keep_or_refuse(state.index, zenith.into(), plainly_valid, line);
            }
            if let Some(distance) = distance {
                let mut slope = DistanceObservation {
                    from: at.clone(),
                    to: name.clone(),
                    distance,
                    kind: DistanceKind::Slope,
                    sigma: match distance_error {
                        Some(error) if error > 0.0 => error,
                        _ => distance_sigma(&precision, distance),
                    },
                    instrument_height,
                    target_height,
                    source: source.clone(),
                    pointing,
                    ..Default::default()
                };
                if let Some(target) = &target {
                    slope.target.prism_constant = target.prism_constant;
                    // The schema: no prism constant is in the raw EDM distance.
                    slope.target.prism_constant_state = CorrectionState::NotApplied;
                    slope.target.target_type = target.prism_type.clone();
                }
                if !mode.is_empty() && slope.target.target_type.is_empty() {
                    slope.target.target_type = String::from(mode);
                }
                let plainly_valid = positive_finite(slope.distance)
                    && slope.instrument_height.is_finite()
                    && slope.target_height.is_finite()
                    && positive_finite(slope.sigma);
                self.keep_or_refuse(state.index, slope.into(), plainly_valid, line);
            }
            if horizontal.is_none() && vertical.is_none() && distance.is_none() {
                self.warn(
                    line,
                    format!(
                        "observation to {name} records no circle reading and no distance and was \
                         not read"
                    ),
                );
            }
            // Read when any of its values was kept; each one refused has its
            // warning from keep_or_refuse.
            return self.result.project.stations[state.index].observations.len() > before;
        }

        if record.has(b"MTA") {
            // A mean turned angle is the controller's mean of shots that are
            // already in the file as their own records; reading it too would
            // count them twice.
            self.describe_named(&mut described);
            self.mean_turned_angles += 1;
            record.mark_all_used();
            return false;
        }

        // ---- GNSS.
        let solution = match survey_method {
            "Fix" | "NetworkFix" | "RTK" | "NetworkRTK" | "xFillFix" | "RTX" | "RTXFast"
            | "RTKxFill" | "xFillX" => GnssSolution::Fixed,
            "Float" | "NetworkFloat" | "xFillFloat" => GnssSolution::Float,
            "Code" | "WAAS" | "OmniSTARHP" | "OmniSTARVBS" => GnssSolution::Differential,
            "Autonomous" => GnssSolution::Autonomous,
            _ => GnssSolution::Unknown,
        };
        let antenna_id = Self::text_of(record, "AntennaID");

        if record.has(b"ECEFDeltas") {
            self.describe_named(&mut described);
            let mut base = Self::text_of(record, "RTK_Base");
            let reference_id = Self::text_of(record, "ReferenceRecordID");
            let mut base_antenna_id = String::new();
            if let Some(reference) = self.references.get(&reference_id) {
                if base.is_empty() {
                    base = reference.point_name.clone();
                }
                base_antenna_id = reference.antenna_id.clone();
            }
            let dx = self.number(record, "ECEFDeltas/DeltaX");
            let dy = self.number(record, "ECEFDeltas/DeltaY");
            let dz = self.number(record, "ECEFDeltas/DeltaZ");
            let no_base = base.is_empty() || base == name;
            let (false, Some(dx), Some(dy), Some(dz)) = (no_base, dx, dy, dz) else {
                self.warn(
                    line,
                    format!(
                        "the GNSS vector to {name} has no base point or an incomplete delta and \
                         was not read"
                    ),
                );
                return false;
            };
            self.ensure_named(&base, line);
            let covariance = self.covariance_xyz(record, &name, line);
            let vector = GnssGeocentricBaselineObservation {
                from: base,
                to: name.clone(),
                delta: GeocentricCoordinate {
                    x: dx,
                    y: dy,
                    z: dz,
                },
                covariance,
                // The schema: ECEF values are "with respect to the job's WGS-84
                // reference frame".
                reference_frame: String::from("WGS 84"),
                from_antenna: self.antenna_for(&base_antenna_id),
                to_antenna: self.antenna_for(&antenna_id),
                solution,
                source: self.source_at(line),
            };
            return self.add_observation(vector.into(), line);
        }

        if record.has(b"ECEF") || record.has(b"RTXECEF") {
            let rtx = !record.has(b"ECEF");
            let group = if rtx { "RTXECEF" } else { "ECEF" };
            let x = self.number(record, &format!("{group}/X"));
            let y = self.number(record, &format!("{group}/Y"));
            let z = self.number(record, &format!("{group}/Z"));
            let (Some(x), Some(y), Some(z)) = (x, y, z) else {
                self.describe_named(&mut described);
                self.warn(
                    line,
                    format!("the ECEF position of {name} is incomplete and was not read"),
                );
                return false;
            };
            let mut position = GnssGlobalPositionObservation {
                geocentric: Some(GeocentricCoordinate { x, y, z }),
                covariance: self.covariance_xyz(record, &name, line),
                reference_frame: String::from("WGS 84"),
                ..Default::default()
            };
            if rtx {
                let frame = Self::text_of(record, "RTXECEF/ReferenceFrame");
                let epoch = Self::text_of(record, "RTXECEF/ITRFEpoch");
                position.reference_frame = if frame.is_empty() {
                    String::from("ITRF2008")
                } else {
                    frame
                };
                if !epoch.is_empty() {
                    position.reference_frame.push_str(" epoch ");
                    position.reference_frame.push_str(&epoch);
                }
            }
            return self.global_position(position, &mut described, &antenna_id, solution);
        }

        if record.has(b"WGS84") || record.has(b"Local") {
            let local = !record.has(b"WGS84");
            let group = if local { "Local" } else { "WGS84" };
            let latitude = self.number(record, &format!("{group}/Latitude"));
            let longitude = self.number(record, &format!("{group}/Longitude"));
            let height = self.number(record, &format!("{group}/Height"));
            let measured = !local
                && survey_method != "KeyedIn"
                && survey_method != "Copied"
                && method != "Coordinates"
                && method != "CopiedPoint";
            let (Some(latitude_value), Some(longitude_value), Some(height_value), true) =
                (latitude, longitude, height, measured)
            else {
                // Keyed in, copied or on a local ellipsoid: a position, but
                // not a grid one and not a GNSS observation. Named, with the
                // values in metadata for whatever projects it.
                self.describe_named(&mut described);
                if let Some(metadata) = self.metadata_of(&name) {
                    let prefix = if local { "local" } else { "wgs84" };
                    if let Some(latitude) = latitude {
                        metadata.insert(
                            format!("{prefix}.latitude_deg"),
                            format_exact_real(latitude),
                        );
                    }
                    if let Some(longitude) = longitude {
                        metadata.insert(
                            format!("{prefix}.longitude_deg"),
                            format_exact_real(longitude),
                        );
                    }
                    if let Some(height) = height {
                        metadata.insert(format!("{prefix}.height"), format_exact_real(height));
                    }
                }
                // Read: the values are kept (a warning at the end says where).
                self.keyed_global += 1;
                return true;
            };
            let mut position = GnssGlobalPositionObservation {
                geodetic: Some(GeodeticCoordinate {
                    latitude: latitude_value * RADIANS_PER_DEGREE,
                    longitude: longitude_value * RADIANS_PER_DEGREE,
                    ellipsoidal_height: height_value,
                }),
                ..Default::default()
            };
            if record.has(b"QualityControl3") {
                let north = self.number(record, "QualityControl3/SigmaNorth");
                let east = self.number(record, "QualityControl3/SigmaEast");
                let up = self.number(record, "QualityControl3/SigmaElevation");
                let north_east = self.number(record, "QualityControl3/CovarianceNorthEast");
                if let (Some(north), Some(east), Some(up)) = (north, east, up)
                    && north > 0.0
                    && east > 0.0
                    && up > 0.0
                {
                    position.covariance = GnssCovariance3 {
                        xx: north * north,
                        yy: east * east,
                        zz: up * up,
                        xy: north_east.unwrap_or(0.0),
                        xz: 0.0,
                        yz: 0.0,
                    };
                }
            }
            position.reference_frame = String::from("WGS 84");
            return self.global_position(position, &mut described, &antenna_id, solution);
        }

        // Anything else - a polar COGO point, a laser offset, a level reading
        // - names the point and says what it did not read: a record this
        // reader does not handle, so it counts as skipped.
        self.describe_named(&mut described);
        self.warn(
            line,
            format!(
                "point {name} was recorded by the method {}, which this reader does not turn into \
                 coordinates or observations",
                if method.is_empty() {
                    "(none stated)"
                } else {
                    method
                }
            ),
        );
        false
    }

    // ---- Reductions and Environment

    fn read_reductions_point(&mut self, record: &RecordBuffer<'_>) {
        let name = Self::text_of(record, "Name");
        // The file's own digits, not a re-formatted double, so a comparison in
        // a report is against exactly what the controller wrote.
        const KEPT: [(&str, &str); 7] = [
            ("ID", "trimble.reductions.recordId"),
            ("Grid/North", "trimble.reductions.north"),
            ("Grid/East", "trimble.reductions.east"),
            ("Grid/Elevation", "trimble.reductions.elevation"),
            ("WGS84/Latitude", "trimble.reductions.latitude_deg"),
            ("WGS84/Longitude", "trimble.reductions.longitude_deg"),
            ("WGS84/Height", "trimble.reductions.height"),
        ];
        let Some(metadata) = self.metadata_of(&name) else {
            self.reductions_unknown += 1;
            record.mark_all_used();
            return;
        };
        for (path, key) in KEPT {
            if let Some(value) = record.take(path.as_bytes())
                && !trimmed(value).is_empty()
            {
                metadata.insert(String::from(key), String::from(text(trimmed(value))));
            }
        }
        // The rest (code, descriptions, features) is the schema's copy of the
        // FieldBook record the point came from, already read there.
        record.mark_all_used();
    }

    fn read_environment(&mut self, record: &RecordBuffer<'_>) {
        let system = Self::text_of(record, "CoordinateSystem/SystemName");
        let zone = Self::text_of(record, "CoordinateSystem/ZoneName");
        let datum = Self::text_of(record, "CoordinateSystem/DatumName");
        if !system.is_empty() || !zone.is_empty() {
            let mut declared = system.clone();
            if !zone.is_empty() {
                if declared.is_empty() {
                    declared.push_str(&zone);
                } else {
                    declared.push_str(" / ");
                    declared.push_str(&zone);
                }
            }
            self.result.project.coordinate_system = DeclaredCoordinateSystem::named(declared);
        }
        let metadata = &mut self.result.project.metadata;
        if !system.is_empty() {
            metadata.insert(
                String::from("jxl.environment.CoordinateSystem/SystemName"),
                system,
            );
        }
        if !zone.is_empty() {
            metadata.insert(
                String::from("jxl.environment.CoordinateSystem/ZoneName"),
                zone,
            );
        }
        if !datum.is_empty() {
            metadata.insert(
                String::from("jxl.environment.CoordinateSystem/DatumName"),
                datum,
            );
        }
        self.keep_as_metadata("jxl.environment", record);
    }

    // ---- The whole file

    fn finish(&mut self) {
        // A name both positioned and named is positioned: the named entry was
        // made for an observation before the coordinate record appeared, or
        // after.
        for point in std::mem::take(&mut self.named) {
            if let Some(index) = self.positioned_index(&point.id) {
                let found = &mut self.positioned[index];
                for (key, value) in point.metadata {
                    found.metadata.entry(key).or_insert(value);
                }
                if found.code.is_empty() {
                    found.code = point.code;
                }
                continue;
            }
            self.result.project.unpositioned_points.push(point);
        }
        self.result.project.points = std::mem::take(&mut self.positioned);

        if self.deleted_count > 0 {
            let mut ids = String::new();
            for id in &self.deleted_ids {
                if !ids.is_empty() {
                    ids.push_str(", ");
                }
                ids.push_str(id);
            }
            if self.deleted_count > self.deleted_ids.len() {
                ids.push_str(", ...");
            }
            let message = format!(
                "{} record(s) the surveyor deleted in the controller were not imported (record \
                 IDs {ids})",
                self.deleted_count
            );
            self.warn(0, message);
        }
        if self.mean_turned_angles > 0 {
            let message = format!(
                "{} mean turned angle record(s) were not read as observations: each is the \
                 controller's mean of shots that are read from their own records",
                self.mean_turned_angles
            );
            self.warn(0, message);
        }
        if self.keyed_global > 0 {
            let message = format!(
                "{} point(s) given as keyed-in or local latitude and longitude have no grid \
                 position; their values are in the point's metadata",
                self.keyed_global
            );
            self.warn(0, message);
        }
        if self.reductions_unknown > 0 {
            let message = format!(
                "{} point(s) in the controller's Reductions section have no record in the field \
                 book that was read; their reduced coordinates were not kept",
                self.reductions_unknown
            );
            self.warn(0, message);
        }
        let skipped: Vec<String> = self
            .skipped_kinds
            .iter()
            .map(|(kind, count)| {
                format!("{count} {kind} record(s) were not read: this reader does not import them")
            })
            .collect();
        for message in skipped {
            self.warn(0, message);
        }
        let unread: Vec<String> = self
            .unread
            .iter()
            .map(|(key, unread)| {
                let mut message = format!(
                    "{key} in {} record(s) was read but the survey model has no place for it",
                    unread.records
                );
                if !unread.leaves.is_empty() {
                    message.push_str(" (");
                    let leaves: Vec<&str> = unread.leaves.iter().map(String::as_str).collect();
                    message.push_str(&leaves.join(", "));
                    message.push(')');
                }
                message
            })
            .collect();
        for message in unread {
            self.warn(0, message);
        }

        if self.setups_without_atmosphere > 0 {
            self.result.not_carried.push(format!(
                "{} setup(s) name no atmosphere record: the atmospheric correction state is \
                 unknown for them",
                self.setups_without_atmosphere
            ));
        }
        let setups_without_height = self
            .stations
            .values()
            .filter(|state| !state.height_known)
            .count();
        if setups_without_height > 0 {
            self.result.not_carried.push(format!(
                "{setups_without_height} setup(s) without an instrument height"
            ));
        }
        if self.shots_without_target_height > 0 {
            self.result.not_carried.push(format!(
                "{} shot(s) without a target height",
                self.shots_without_target_height
            ));
        }
    }

    fn root_attribute(&self, attribute_name: &[u8]) -> String {
        let Some(raw) = self.xml.attribute(attribute_name) else {
            return String::new();
        };
        let mut decoded = Vec::new();
        if append_xml_text(raw, &mut decoded).is_err() {
            return String::new();
        }
        String::from(text(&decoded))
    }

    fn read(mut self) -> Result<ReadResult> {
        // The root.
        let event = self.xml.next();
        if event == Event::Error {
            return Err(Error::new(
                ErrorCode::FileImportFailure,
                format!(
                    "{} is not readable as XML: {}",
                    self.file_name,
                    self.xml.error().message
                ),
            ));
        }
        if event != Event::StartElement || self.xml.name() != b"JOBFile" {
            return Err(Error::new(
                ErrorCode::FileImportFailure,
                format!(
                    "{} is not a Trimble JobXML file: its first element is <{}>, not <JOBFile>",
                    self.file_name,
                    String::from_utf8_lossy(self.xml.name())
                ),
            ));
        }
        let version = self.root_attribute(b"version");
        self.result.project.name = self.root_attribute(b"jobName");
        self.result.project.units = DeclaredUnits {
            linear: LinearUnit::Metres,
            angular: AngularUnit::DecimalDegrees,
        };
        for key in [
            "version",
            "product",
            "productVersion",
            "productDBVersion",
            "controllerSerialNumber",
            "TimeStamp",
        ] {
            let value = self.root_attribute(key.as_bytes());
            if !value.is_empty() {
                self.result
                    .project
                    .metadata
                    .insert(format!("jxl.{key}"), value);
            }
        }
        self.base_source = SourceRecord {
            manufacturer: String::from("Trimble"),
            format: String::from(HUMAN_NAME),
            format_version: if version.is_empty() {
                String::from("JobXML")
            } else {
                format!("JobXML {version}")
            },
            file_name: self.file_name.clone(),
            record_number: 0,
        };
        self.result.project.source = self.base_source.clone();
        if !version.is_empty() && !version.starts_with("5.") && !version.starts_with("4.") {
            self.warn(
                1,
                format!(
                    "JobXML version {version} is not one this reader was written against (4.x and \
                     5.x, schema 5.72)"
                ),
            );
        }

        let mut record = RecordBuffer::new();
        // The depth-2 element being walked.
        let mut section: &[u8] = b"";
        loop {
            match self.xml.next() {
                Event::Error => {
                    return Err(Error::new(
                        ErrorCode::FileImportFailure,
                        format!("{}: {}", self.file_name, self.xml.error().message),
                    ));
                }
                Event::EndOfDocument => break,
                Event::EndElement => {
                    if self.xml.depth() == 2 {
                        section = b"";
                    }
                    continue;
                }
                Event::Text => continue, // white space between records
                Event::StartElement => {}
            }
            let depth = self.xml.depth();
            if depth == 2 {
                section = self.xml.name();
                if section == b"Environment" {
                    record.reset(self.lines.line_at(self.xml.offset()));
                    self.gather(&mut record, 2)?;
                    if record.over_budget() {
                        self.warn_over_budget(b"Environment", " section", record.line());
                    } else {
                        self.read_environment(&record);
                    }
                    section = b"";
                } else if section != b"FieldBook" && section != b"Reductions" {
                    *self
                        .skipped_kinds
                        .entry(format!("{} section", text(section)))
                        .or_default() += 1;
                }
                continue;
            }
            if depth != 3 || (section != b"FieldBook" && section != b"Reductions") {
                continue; // inside a section this reader does not read (counted above)
            }
            // Views into the document, which outlives the record: a record ID
            // and time stamp are read for every record, and a time stamp is too
            // long to copy without a heap allocation.
            let name = self.xml.name();
            let id = self.xml.attribute(b"ID").map(trimmed).unwrap_or_default();
            let time_stamp = self
                .xml
                .attribute(b"TimeStamp")
                .map(trimmed)
                .unwrap_or_default();
            record.reset(self.lines.line_at(self.xml.offset()));
            self.gather(&mut record, 3)?;
            if record.over_budget() {
                if section == b"Reductions" {
                    self.warn_over_budget(
                        name,
                        " element of the Reductions section",
                        record.line(),
                    );
                } else {
                    self.warn_over_budget(name, " record", record.line());
                    self.result.records_skipped += 1;
                }
                continue;
            }
            if section == b"Reductions" {
                if name == b"Point" {
                    self.read_reductions_point(&record);
                }
                continue;
            }
            self.read_field_book_record(text(name), text(id), time_stamp, &record);
        }
        self.finish();
        Ok(self.result)
    }
}

// ---- Registration ----------------------------------------------------------------------

fn read_job_xml(bytes: &[u8], file_name: &str, options: &ReadOptions) -> Result<ReadResult> {
    let bytes = without_byte_order_mark(bytes);
    // The model holds validated UTF-8, and a JobXML file is UTF-8: one that
    // is not is refused at its first byte that is not, where the C++ reads
    // such bytes into the model as they are (see the top of this file).
    if let Err(error) = std::str::from_utf8(bytes) {
        let line = 1 + bytes[..error.valid_up_to()]
            .iter()
            .filter(|&&c| c == b'\n')
            .count();
        return Err(Error::new(
            ErrorCode::FileImportFailure,
            format!(
                "{file_name} line {line} holds bytes that are not UTF-8, which a JobXML file is; \
                 Katana does not guess what they mean"
            ),
        ));
    }
    JobXmlReader::new(bytes, file_name, options).read()
}

// The root element of a JobXML file is <JOBFile>; nothing else uses that
// name. The probe steps over the XML declaration, comments and white space to
// the first element and looks at its name - it does not parse anything.
fn probe_job_xml(input: &ProbeInput<'_>) -> FormatSignature {
    let bytes = without_byte_order_mark(input.bytes);
    let find = |needle: &[u8], from: usize| {
        bytes
            .get(from..)
            .and_then(|rest| {
                rest.windows(needle.len())
                    .position(|window| window == needle)
            })
            .map(|at| at + from)
    };
    let mut p = 0;
    for _ in 0..64 {
        while p < bytes.len() && is_space(bytes[p]) {
            p += 1;
        }
        if p + 1 >= bytes.len() || bytes[p] != b'<' {
            break;
        }
        if bytes[p..].starts_with(b"<!--") {
            let Some(close) = find(b"-->", p + 4) else {
                break;
            };
            p = close + 3;
            continue;
        }
        if bytes[p + 1] == b'?' {
            let Some(close) = find(b"?>", p + 2) else {
                break;
            };
            p = close + 2;
            continue;
        }
        let rest = &bytes[p..];
        if rest.starts_with(b"<JOBFile")
            && (rest.len() == 8 || is_space(rest[8]) || rest[8] == b'>' || rest[8] == b'/')
        {
            let tag_end = rest.iter().position(|&c| c == b'>').unwrap_or(rest.len());
            let tag = &rest[..tag_end];
            let contains = |needle: &[u8]| tag.windows(needle.len()).any(|window| window == needle);
            let trimble = contains(b"product=\"Trimble") || contains(b"product='Trimble");
            if input.extension == "jxl" {
                return FormatSignature {
                    confidence: 0.98,
                    evidence: String::from(if trimble {
                        "extension .jxl, root element JOBFile, product Trimble"
                    } else {
                        "extension .jxl and root element JOBFile"
                    }),
                };
            }
            return FormatSignature {
                confidence: if trimble { 0.95 } else { 0.9 },
                evidence: String::from(if trimble {
                    "root element JOBFile, product Trimble"
                } else {
                    "root element JOBFile"
                }),
            };
        }
        break;
    }
    if input.extension == "jxl" {
        return FormatSignature {
            confidence: 0.2,
            evidence: String::from("extension .jxl, but the first element is not JOBFile"),
        };
    }
    ruled_out()
}

fn trimble_job_xml_format() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(FORMAT_ID),
        human_name: String::from(HUMAN_NAME),
        manufacturer: Manufacturer::Trimble,
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
        extensions: vec![String::from("jxl")],
    }
}

/// This format's line in the program's registry (format.rs).
pub(crate) fn registration() -> FormatRegistration {
    FormatRegistration::new(trimble_job_xml_format(), probe_job_xml, Some(read_job_xml))
}
