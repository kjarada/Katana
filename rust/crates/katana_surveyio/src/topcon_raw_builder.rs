//! What the RW5, GTS-7, opcode field file and Sokkia SDR readers share:
//! packed-angle parsing (RW5 and GTS-7 only) and the builder that turns a
//! stream of raw total-station records into one SurveyProject.
//!
//! Internal to surveyio: the readers of topcon_rw5, topcon_gts,
//! opcode_field_file and sokkia_sdr are its users - the last two read no Topcon
//! format but journals of the same kind, and the module keeps the name it was
//! given first. All four formats are journals written as the field work
//! happened - a setup, its backsight, its shots, a stored point - so they need
//! the same bookkeeping: a point is named long before (or without ever) being
//! given coordinates, a setup's observations share a numbering of pointings,
//! and a code on a shot strings that shot's point into a feature.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};

use katana_core::text::{format_exact_real, parse_finite_double, trimmed};
use katana_math::numerics::{PI, TWO_PI, tolerance};
use katana_survey::data_model::set_observation_source;
use katana_survey::{
    AzimuthObservation, CoordinateSource, DistanceObservation, Face, GnssBaselineObservation,
    GnssGeocentricBaselineObservation, GnssGlobalPositionObservation, GnssPositionObservation,
    HorizontalAngleObservation, HorizontalDirectionObservation, InstrumentSettings,
    LevelDifferenceObservation, Observation, SourceRecord, Station, SurveyFeature, SurveyPoint,
    SurveyProject, SurveyStation, UnpositionedPoint, VerticalAngleObservation,
    ZenithAngleObservation,
};

use crate::format::{ReadOptions, ReadResult, ReadWarning};

// ---- Angles -----------------------------------------------------------------

const RADIANS_PER_ARC_SECOND: f64 = PI / 648000.0;

// 10^n for the decimals of a second; exact in binary for n <= 22.
const POWERS_OF_TEN: [f64; 16] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15,
];

/// A real number occupying the whole (trimmed) token.
pub(crate) fn parse_real(text: impl AsRef<[u8]>) -> Option<f64> {
    parse_finite_double(trimmed(text.as_ref()))
}

/// "DDD.MMSSsss" - degrees, then two digits of minutes, two of seconds and any
/// decimals of a second, packed into one decimal number. Both the TDS RW5
/// specification ("Angle (See MO for units)", degrees) and the Topcon Link
/// manual's GTS-7 sample (322.33160 is 322 33' 16.0") write degrees this way.
///
/// Read from the TEXT, digit by digit, never as a double: 0.0044 is 0 00' 44",
/// and (0.0044 * 100 - 0) * 100 in binary floating point is 43.99999999999999.
/// Missing trailing digits are zeros ("90.3" is 90 30' 00"). A sign applies to
/// the whole angle ("-0.3000" is minus thirty minutes). None when the text is
/// not such a number or its minutes or seconds are 60 or more.
pub(crate) fn packed_degrees_to_radians(text: impl AsRef<[u8]>) -> Option<f64> {
    let mut text = trimmed(text.as_ref());
    let mut negative = false;
    if let Some(&sign) = text.first()
        && (sign == b'+' || sign == b'-')
    {
        negative = sign == b'-';
        text = &text[1..];
    }
    let (whole, fraction) = match text.iter().position(|&c| c == b'.') {
        Some(dot) => (&text[..dot], &text[dot + 1..]),
        None => (text, &text[text.len()..]),
    };
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    // Degrees beyond a few thousand are not an angle anyone recorded; the cap
    // keeps the integer arithmetic below far from overflow.
    if whole.len() > 6 {
        return None;
    }
    let mut degrees: i64 = 0;
    for &c in whole {
        if !c.is_ascii_digit() {
            return None;
        }
        degrees = degrees * 10 + i64::from(c - b'0');
    }
    if !fraction.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let digit = |i: usize| fraction.get(i).map_or(0, |&c| i32::from(c - b'0'));
    let minutes = digit(0) * 10 + digit(1);
    let seconds = digit(2) * 10 + digit(3);
    if minutes >= 60 || seconds >= 60 {
        return None;
    }
    // Decimals of a second: digits 5 onwards, read as one integer over a power
    // of ten. More than 15 of them is more precision than a double carries.
    let mut decimals = 0.0;
    if fraction.len() > 4 {
        let tail = &fraction[4..fraction.len().min(4 + 15)];
        let mut value: i64 = 0;
        for &c in tail {
            value = value * 10 + i64::from(c - b'0');
        }
        decimals = value as f64 / POWERS_OF_TEN[tail.len()];
    }
    let arc_seconds =
        (degrees as f64 * 60.0 + f64::from(minutes)) * 60.0 + f64::from(seconds) + decimals;
    let radians = arc_seconds * RADIANS_PER_ARC_SECOND;
    Some(if negative { -radians } else { radians })
}

/// Plain decimal gons (400 to the circle).
pub(crate) fn gons_to_radians(text: impl AsRef<[u8]>) -> Option<f64> {
    let gons = parse_real(text)?;
    Some(gons * (PI / 200.0))
}

/// Into [0, 2*pi).
pub(crate) fn wrap_to_circle(radians: f64) -> f64 {
    let mut wrapped = radians % TWO_PI;
    if wrapped < 0.0 {
        wrapped += TWO_PI;
    }
    // fmod of a value a hair below a whole turn can round up to exactly 2*pi.
    if wrapped >= TWO_PI { 0.0 } else { wrapped }
}

/// Metres per international foot, exact by definition (EPSG:9002), for the unit
/// switches both formats carry.
pub(crate) const METRES_PER_FOOT: f64 = 0.3048;
/// Metres per US survey foot, exact by definition (EPSG:9003).
pub(crate) const METRES_PER_US_SURVEY_FOOT: f64 = 1200.0 / 3937.0;

// ---- The builder ------------------------------------------------------------

/// Which face a zenith READING was taken on, from the reading itself: a circle
/// reading past 180 degrees exists only on face right (reverse); one below it
/// is face left. Exactly 0 or 180 says nothing.
pub(crate) fn face_of_zenith_reading(zenith_radians: f64) -> Face {
    if zenith_radians > PI {
        return Face::Right;
    }
    if zenith_radians > 0.0 && zenith_radians < PI {
        return Face::Left;
    }
    Face::Unknown
}

/// A zenith reading as the model holds it: in [0, pi]. A face-right reading
/// z > pi is the same line of sight as 2*pi - z on face left.
pub(crate) fn zenith_in_model_range(zenith_radians: f64) -> f64 {
    if zenith_radians > PI {
        TWO_PI - zenith_radians
    } else {
        zenith_radians
    }
}

// ---- Offsets ------------------------------------------------------------------

/// What a shot reads: a circle reading (radians, clockwise), a zenith in the
/// model's range [0, pi] and a slope distance (metres).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ShotReading {
    pub(crate) circle: f64,
    pub(crate) zenith: f64,
    pub(crate) slope: f64,
}

/// The reading of the point a shot's offsets put it at, from what was
/// measured: `radial` along the plan line from the instrument to the target,
/// positive away from it; `tangential` at right angles to that line, positive
/// to the right looking from the instrument (a clockwise circle reads it as
/// more); `height` added to the target's rise. With plan distance h = s sin z
/// and rise v = s cos z, the point is at plan distance h' = hypot(h + radial,
/// tangential) on the circle a + atan2(tangential, h + radial), rising
/// v' = v + height: the reading is that circle, the zenith atan2(h', v') and
/// the slope distance hypot(h', v'). None when that point is within
/// tolerance::COORDINATE of the instrument - its own mark, to which no shot has
/// a direction or a zenith.
///
/// One home for the arithmetic of every reader on this builder whose format
/// records offsets. The opcode field file applies its 42, 43 and 44 through it;
/// the GTS-7 reader keeps its OFFSET records unapplied.
pub(crate) fn offset_shot(
    measured: &ShotReading,
    radial: f64,
    tangential: f64,
    height: f64,
) -> Option<ShotReading> {
    let along = measured.slope * measured.zenith.sin() + radial;
    let plan = along.hypot(tangential);
    let rise = measured.slope * measured.zenith.cos() + height;
    let slope = plan.hypot(rise);
    if !(slope >= tolerance::COORDINATE) {
        return None;
    }
    Some(ShotReading {
        circle: wrap_to_circle(measured.circle + tangential.atan2(along)),
        zenith: plan.atan2(rise),
        slope,
    })
}

/// An observation kind a setup can hold, for
/// RawProjectBuilder::station_observation: the C++ builds the alternative in
/// place in the variant and hands back a reference to it.
pub(crate) trait StationObservation: Default + Into<Observation> {
    /// The alternative `observation` holds, when it is this kind.
    fn held(observation: &mut Observation) -> Option<&mut Self>;
}

macro_rules! station_observation {
    ($($kind:ident => $variant:ident),* $(,)?) => {
        $(
            impl StationObservation for $kind {
                fn held(observation: &mut Observation) -> Option<&mut Self> {
                    if let Observation::$variant(held) = observation {
                        Some(held)
                    } else {
                        None
                    }
                }
            }
        )*
    };
}

station_observation! {
    DistanceObservation => Distance,
    HorizontalAngleObservation => HorizontalAngle,
    VerticalAngleObservation => VerticalAngle,
    ZenithAngleObservation => ZenithAngle,
    AzimuthObservation => Azimuth,
    GnssBaselineObservation => GnssBaseline,
    GnssPositionObservation => GnssPosition,
    LevelDifferenceObservation => LevelDifference,
    HorizontalDirectionObservation => HorizontalDirection,
    GnssGlobalPositionObservation => GnssGlobalPosition,
    GnssGeocentricBaselineObservation => GnssGeocentricBaseline,
}

/// Which coordinates of a point a file states twice are its own, by the
/// format's rule.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum RestatedCoordinates {
    /// The default: control is stored before it is observed, and a later value
    /// for the same name is a check or a recomputation.
    #[default]
    FirstKept,
    /// The format's own rule is that the latest coordinates are the best (the
    /// Sokkia SDR's, whose traverse adjustment writes its results after the
    /// positions it adjusts).
    LatestKept,
}

const NO_FEATURE: u32 = u32::MAX;
const NOT_FOUND: u32 = u32::MAX;

#[derive(Clone, Debug, Default)]
struct PointEntry {
    id: String,
    /// Of id, kept so the table can grow without rehashing text.
    hash: u64,
    positioned: bool,
    northing: f64,
    easting: f64,
    elevation: Option<f64>,
    coordinate_source: CoordinateSource,
    position_record: usize,
    /// The SourceRecord is built once, in finish().
    source_record: usize,
    code: String,
    description: String,
    metadata: BTreeMap<String, String>,
    feature: u32,
}

// The table's hash of a point id. Not observable - the table is never walked -
// but fixed rather than seeded per run, so a run is the same each time.
fn hash_of(id: &str) -> u64 {
    BuildHasherDefault::<DefaultHasher>::default().hash_one(id)
}

// "N 5000000.25, E 500000.5, elevation 101.75": coordinates as a warning and
// a point's metadata quote them, in the shortest text that reads back.
fn coordinates_text(northing: f64, easting: f64, elevation: Option<f64>) -> String {
    let mut written = format!(
        "N {}, E {}",
        format_exact_real(northing),
        format_exact_real(easting)
    );
    if let Some(elevation) = elevation {
        written.push_str(&format!(", elevation {}", format_exact_real(elevation)));
    }
    written
}

/// Turns a stream of raw total-station records into one SurveyProject.
pub(crate) struct RawProjectBuilder {
    file_name: String,
    source_template: SourceRecord,
    options: ReadOptions,
    result: ReadResult,
    points: Vec<PointEntry>,
    // Point ids -> index in points, open addressing over points' own ids: one
    // table of 32-bit slots instead of a node and a second copy of the id per
    // point, which on a job of a million shots was a third of the read
    // (measured in the C++: 1.2 us per std::unordered_map insert). 0 is an
    // empty slot; otherwise index + 1. Kept at most half full.
    point_slots: Vec<u32>,
    /// The point entry() found or made last.
    last_point: u32,
    restated: RestatedCoordinates,
    unlisted_warnings: usize,
    first_unlisted_record: usize,
    station_index: HashMap<String, u32>,
    /// Setups begun on each point.
    occupations: HashMap<String, u32>,
    /// The OPEN feature of each code and string number.
    feature_index: HashMap<String, u32>,
    pointing_counters: Vec<usize>,
}

impl RawProjectBuilder {
    /// The first LISTED_WARNINGS warnings are listed; past them each is counted
    /// and finish() says how many more there were. A file of a hundred million
    /// unreadable lines (1 GiB is read_survey's cap) would otherwise become
    /// gigabytes of warnings - the program stops answering while it allocates,
    /// which the size cap exists to prevent.
    pub(crate) const LISTED_WARNINGS: usize = 10000;

    /// `source` is copied into every SourceRecord with the record number set.
    pub(crate) fn new(file_name: &str, source: SourceRecord, options: &ReadOptions) -> Self {
        let source_template = SourceRecord {
            file_name: String::from(file_name),
            record_number: 0,
            ..source
        };
        let result = ReadResult {
            project: SurveyProject {
                source: source_template.clone(),
                ..Default::default()
            },
            ..Default::default()
        };
        Self {
            file_name: String::from(file_name),
            source_template,
            options: options.clone(),
            result,
            points: Vec::new(),
            point_slots: Vec::new(),
            last_point: NOT_FOUND,
            restated: RestatedCoordinates::FirstKept,
            unlisted_warnings: 0,
            first_unlisted_record: 0,
            station_index: HashMap::new(),
            occupations: HashMap::new(),
            feature_index: HashMap::new(),
            pointing_counters: Vec::new(),
        }
    }

    pub(crate) fn options(&self) -> &ReadOptions {
        &self.options
    }

    pub(crate) fn project(&mut self) -> &mut SurveyProject {
        &mut self.result.project
    }

    pub(crate) fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Provenance of one record.
    pub(crate) fn source(&self, record: usize) -> SourceRecord {
        SourceRecord {
            record_number: record,
            ..self.source_template.clone()
        }
    }

    /// The same, assigned into an existing record.
    #[allow(
        dead_code,
        reason = "The C++ builder's stampSource, kept with its twin; station_observation builds its record with source(), and no reader calls this one."
    )]
    pub(crate) fn stamp_source(&self, target: &mut SourceRecord, record: usize) {
        target.clone_from(&self.source_template);
        target.record_number = record;
    }

    /// A warning about a record that WAS read (in part or in full); see
    /// LISTED_WARNINGS.
    pub(crate) fn warn(&mut self, record: usize, message: String) {
        if self.result.warnings.len() < Self::LISTED_WARNINGS {
            self.result.warnings.push(ReadWarning {
                file_name: self.file_name.clone(),
                record,
                message,
            });
            return;
        }
        if self.unlisted_warnings == 0 {
            self.first_unlisted_record = record;
        }
        self.unlisted_warnings += 1;
    }

    /// A record that was not imported: a warning and one more skipped record.
    pub(crate) fn skip(&mut self, record: usize, message: String) {
        self.warn(record, message);
        self.result.records_skipped += 1;
    }

    pub(crate) fn count_read(&mut self) {
        self.result.records_read += 1;
    }

    /// Records not imported that one warning already accounts for (the lines
    /// after a file's own end marker).
    pub(crate) fn count_skipped(&mut self, count: usize) {
        self.result.records_skipped += count;
    }

    pub(crate) fn records_skipped(&self) -> usize {
        self.result.records_skipped
    }

    pub(crate) fn not_carried(&mut self, text: String) {
        if !self.result.not_carried.contains(&text) {
            self.result.not_carried.push(text);
        }
    }

    // ---- Points
    //
    // Every id a record names is "mentioned": a point that is never given
    // coordinates ends up in SurveyProject::unpositioned_points, one that is in
    // SurveyProject::points, each in order of first mention.

    fn find_point(&self, id: &str, hash: u64) -> u32 {
        if self.point_slots.is_empty() {
            return NOT_FOUND;
        }
        let mask = self.point_slots.len() - 1;
        let mut slot = (hash as usize) & mask;
        loop {
            let occupant = self.point_slots[slot];
            if occupant == 0 {
                return NOT_FOUND;
            }
            let point = &self.points[occupant as usize - 1];
            if point.hash == hash && point.id == id {
                return occupant - 1;
            }
            slot = (slot + 1) & mask;
        }
    }

    fn place_point(&mut self, index: u32) {
        let mask = self.point_slots.len() - 1;
        let mut slot = (self.points[index as usize].hash as usize) & mask;
        while self.point_slots[slot] != 0 {
            slot = (slot + 1) & mask;
        }
        self.point_slots[slot] = index + 1;
    }

    // The index of the point called `id`, made (first mentioned at `record`)
    // when there is none.
    fn entry(&mut self, id: &str, record: usize) -> usize {
        // A shot names its target, then codes it, describes it or notes it: the
        // same point several times in a row, which needs no hashing.
        if self.last_point != NOT_FOUND && self.points[self.last_point as usize].id == id {
            return self.last_point as usize;
        }
        let hash = hash_of(id);
        let found = self.find_point(id, hash);
        if found != NOT_FOUND {
            self.last_point = found;
            return found as usize;
        }
        if (self.points.len() + 1) * 2 > self.point_slots.len() {
            self.point_slots = vec![0; (self.point_slots.len() * 2).max(1024)];
            for index in 0..self.points.len() as u32 {
                self.place_point(index);
            }
        }
        let index = self.points.len() as u32;
        self.points.push(PointEntry {
            id: String::from(id),
            hash,
            source_record: record,
            feature: NO_FEATURE,
            ..Default::default()
        });
        self.place_point(index);
        self.last_point = index;
        index as usize
    }

    pub(crate) fn mention_point(&mut self, id: &str, record: usize) {
        self.entry(id, record);
    }

    pub(crate) fn set_restated_coordinates(&mut self, rule: RestatedCoordinates) {
        self.restated = rule;
    }

    /// Coordinates for `id`, metres, kept as set_restated_coordinates says. A
    /// restatement that differs is a warning naming both records, and the
    /// coordinates not kept go to the point's metadata, never dropped; one that
    /// is the same is a repeat and needs no comment. `aside_because`, when not
    /// empty, is why these may place the point but never supersede its
    /// coordinates, whatever the rule (the Sokkia SDR's POS view of a shot kept
    /// in OBS view): the warning then ends with it.
    #[allow(
        clippy::too_many_arguments,
        reason = "The signature is the C++ builder's, argument for argument."
    )]
    pub(crate) fn position_point(
        &mut self,
        id: &str,
        northing: f64,
        easting: f64,
        elevation: Option<f64>,
        how: CoordinateSource,
        record: usize,
        aside_because: &str,
    ) {
        let index = self.entry(id, record);
        let rule = self.restated;
        let point = &mut self.points[index];
        if !point.positioned {
            point.positioned = true;
            point.northing = northing;
            point.easting = easting;
            point.elevation = elevation;
            point.coordinate_source = how;
            point.position_record = record;
            point.source_record = record;
            return;
        }
        if point.northing == northing
            && point.easting == easting
            && (elevation.is_none() || point.elevation == elevation)
        {
            return;
        }
        let restated = coordinates_text(northing, easting, elevation);
        let earlier = point.position_record;
        let message = if !aside_because.is_empty() {
            point.metadata.insert(
                format!("coordinates restated at record {record}"),
                restated.clone(),
            );
            format!(
                "point '{}' is given different coordinates here ({restated} m) from those of \
                 record {earlier}; those are kept and these are in the point's metadata, since \
                 {aside_because}",
                point.id
            )
        } else if rule == RestatedCoordinates::LatestKept {
            point.metadata.insert(
                format!("coordinates of record {earlier}, superseded"),
                coordinates_text(point.northing, point.easting, point.elevation),
            );
            point.northing = northing;
            point.easting = easting;
            point.elevation = elevation;
            point.coordinate_source = how;
            point.position_record = record;
            point.source_record = record;
            format!(
                "point '{}' is given different coordinates here ({restated} m) from those of \
                 record {earlier}; these, the latest, are kept, and those are in the point's \
                 metadata",
                point.id
            )
        } else {
            point.metadata.insert(
                format!("coordinates restated at record {record}"),
                restated.clone(),
            );
            format!(
                "point '{}' is given different coordinates here ({restated} m) from those of \
                 record {earlier}; the first are kept and these are in the point's metadata",
                point.id
            )
        };
        self.warn(record, message);
    }

    // feature_index's key: the code and the string number.
    fn feature_key(code: &str, string_number: &str) -> String {
        format!("{code}\x1f{string_number}")
    }

    /// The field code of `id` (the first word of `description` when `code` is
    /// empty is the caller's business). A non-empty code strings the point into
    /// the feature named by code and string number, in the order met.
    pub(crate) fn code_point(
        &mut self,
        id: &str,
        code: &str,
        description: &str,
        string_number: &str,
        record: usize,
    ) {
        let index = self.entry(id, record);
        {
            let point = &mut self.points[index];
            if point.code.is_empty() {
                point.code = String::from(code);
            }
            if point.description.is_empty() {
                point.description = String::from(description);
            }
        }
        if code.is_empty() {
            return;
        }
        // A feature is one code (and string number, where the format has one),
        // strung in the order its points were observed.
        let key = Self::feature_key(code, string_number);
        let feature_id = match self.feature_index.get(&key) {
            Some(&feature_id) => feature_id,
            None => {
                let feature_id = self.result.project.features.len() as u32;
                let source = self.source(record);
                self.result.project.features.push(SurveyFeature {
                    code: String::from(code),
                    name: String::from(string_number),
                    source,
                    ..Default::default()
                });
                self.feature_index.insert(key, feature_id);
                feature_id
            }
        };
        let point = &mut self.points[index];
        // A point shot again for the same string (both faces of a set, a
        // check) is still one vertex of it.
        if point.feature == feature_id {
            return;
        }
        if point.feature == NO_FEATURE {
            point.feature = feature_id;
        }
        self.result.project.features[feature_id as usize]
            .point_ids
            .push(point.id.clone());
    }

    /// Closes the open feature of `code` and `string_number`
    /// (SurveyFeature::closed) and takes it off the open list, so a later point
    /// of that code and string number starts a new feature rather than joining
    /// a closed one. The number of points in it; None when none is open.
    pub(crate) fn close_feature(&mut self, code: &str, string_number: &str) -> Option<usize> {
        let feature_id = self
            .feature_index
            .remove(&Self::feature_key(code, string_number))?;
        let feature = &mut self.result.project.features[feature_id as usize];
        feature.closed = true;
        Some(feature.point_ids.len())
    }

    /// The same for the feature `id` was first strung into, if still open.
    pub(crate) fn close_feature_of(&mut self, id: &str) -> Option<usize> {
        let index = self.find_point(id, hash_of(id));
        if index == NOT_FOUND || self.points[index as usize].feature == NO_FEATURE {
            return None;
        }
        // Open features are exactly those in feature_index: one that is not
        // closed is the one its code and string number find.
        let feature = &self.result.project.features[self.points[index as usize].feature as usize];
        if feature.closed {
            return None;
        }
        let code = feature.code.clone();
        let name = feature.name.clone();
        self.close_feature(&code, &name)
    }

    /// The last point of the open feature of `code` and `string_number`.
    pub(crate) fn last_point_of(&self, code: &str, string_number: &str) -> Option<String> {
        let feature_id = self
            .feature_index
            .get(&Self::feature_key(code, string_number))?;
        self.result.project.features[*feature_id as usize]
            .point_ids
            .last()
            .cloned()
    }

    pub(crate) fn add_point_metadata(&mut self, id: &str, key: String, value: String) {
        let index = self.entry(id, 0);
        self.points[index].metadata.insert(key, value);
    }

    pub(crate) fn has_point(&self, id: &str) -> bool {
        self.find_point(id, hash_of(id)) != NOT_FOUND
    }

    // ---- Setups

    /// Starts a setup on `point_id`. Its id is the point id, or "<point> (2)",
    /// "(3)" ... when the point is occupied again, so every station id is
    /// unique.
    pub(crate) fn begin_station(
        &mut self,
        point_id: &str,
        instrument_height: f64,
        settings: &InstrumentSettings,
        record: usize,
    ) -> &mut SurveyStation {
        self.mention_point(point_id, record);
        // The n-th occupation of a point is "<point> (n)". Counted per point,
        // so a file that occupies one point a hundred thousand times costs a
        // hundred thousand steps, not the square of it; the loop only runs on
        // when the file itself uses such a name for another setup.
        let occupations = self.occupations.entry(String::from(point_id)).or_insert(0);
        let mut id = String::from(point_id);
        if *occupations > 0 || self.station_index.contains_key(&id) {
            loop {
                *occupations += 1;
                id = format!("{point_id} ({})", *occupations);
                if !self.station_index.contains_key(&id) {
                    break;
                }
            }
        } else {
            *occupations += 1;
        }
        self.station_index
            .insert(id.clone(), self.result.project.stations.len() as u32);
        // Setups of one job tend to be alike: room for as many observations as
        // the last one had saves re-growing a vector of large variants.
        let expected = self
            .result
            .project
            .stations
            .last()
            .map_or(0, |station| station.observations.len());
        let source = self.source(record);
        self.result.project.stations.push(SurveyStation {
            setup: Station {
                id,
                point_id: String::from(point_id),
                instrument_height,
            },
            observations: Vec::with_capacity(expected),
            instrument: settings.clone(),
            source,
            ..Default::default()
        });
        self.pointing_counters.push(0);
        self.result
            .project
            .stations
            .last_mut()
            .expect("a station was just begun")
    }

    pub(crate) fn current_station(&mut self) -> Option<&mut SurveyStation> {
        self.result.project.stations.last_mut()
    }

    /// The next pointing number of the current setup (1, 2, ...).
    pub(crate) fn next_pointing(&mut self) -> usize {
        match self.pointing_counters.last_mut() {
            Some(counter) => {
                *counter += 1;
                *counter
            }
            None => 0,
        }
    }

    /// A new observation of kind T at the end of the current setup, with its
    /// provenance filled in and every other field default, for the caller to
    /// fill. Built where it will live, as the C++ builds it, rather than made
    /// and handed over. A setup must have been begun.
    pub(crate) fn station_observation<T: StationObservation>(&mut self, record: usize) -> &mut T {
        let source = self.source(record);
        let station = self
            .result
            .project
            .stations
            .last_mut()
            .expect("an observation of a setup needs a setup begun");
        station.observations.push(T::default().into());
        let observation = station
            .observations
            .last_mut()
            .expect("an observation was just added");
        set_observation_source(observation, source);
        T::held(observation).expect("the observation just added is of this kind")
    }

    pub(crate) fn add_loose_observation(&mut self, observation: Observation) {
        self.result.project.observations.push(observation);
    }

    pub(crate) fn add_station_note(&mut self, note: &str) {
        let metadata = match self.result.project.stations.last_mut() {
            Some(station) => &mut station.metadata,
            None => &mut self.result.project.metadata,
        };
        let notes = metadata.entry(String::from("notes")).or_default();
        if !notes.is_empty() {
            notes.push('\n');
        }
        notes.push_str(note);
    }

    /// The finished result. The builder is spent.
    pub(crate) fn finish(mut self) -> ReadResult {
        if self.unlisted_warnings > 0 {
            self.result.warnings.push(ReadWarning {
                file_name: self.file_name.clone(),
                record: 0,
                message: format!(
                    "{} more warnings, from record {} on, are not listed: only the first {} are. \
                     The records they are about are still counted as read or skipped",
                    self.unlisted_warnings,
                    self.first_unlisted_record,
                    Self::LISTED_WARNINGS
                ),
            });
        }
        let positioned = self.points.iter().filter(|point| point.positioned).count();
        let project = &mut self.result.project;
        project.points.reserve(positioned);
        project
            .unpositioned_points
            .reserve(self.points.len() - positioned);
        for point in std::mem::take(&mut self.points) {
            let source = SourceRecord {
                record_number: point.source_record,
                ..self.source_template.clone()
            };
            if point.positioned {
                project.points.push(SurveyPoint {
                    id: point.id,
                    northing: point.northing,
                    easting: point.easting,
                    elevation: point.elevation,
                    code: point.code,
                    description: point.description,
                    metadata: point.metadata,
                    coordinate_source: point.coordinate_source,
                    source,
                });
            } else {
                project.unpositioned_points.push(UnpositionedPoint {
                    id: point.id,
                    code: point.code,
                    description: point.description,
                    metadata: point.metadata,
                    source,
                });
            }
        }
        self.result
    }
}
