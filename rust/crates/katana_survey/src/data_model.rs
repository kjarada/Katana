//! Survey data model.
//!
//! Surveying entities are plain values, kept apart from graphical CAD entities.
//!
//! Conventions
//!   * Lengths are metres, angles radians, standard deviations in the unit of the
//!     value they describe. Every observation carries an a-priori standard
//!     deviation `sigma` > 0.
//!   * Azimuths and horizontal angles are clockwise, in [0, 2*pi).
//!   * Points are referenced by their string id. Containers that are iterated for
//!     output are ordered (Vec / BTreeMap), never hash based, so results do not
//!     depend on hashing.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use std::collections::{BTreeMap, BTreeSet};

use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::numerics::{HALF_PI, PI, normalize_angle};
use katana_math::unit_ratio::{UnitRatio, units};

use crate::coordinate::Coordinate2;

/// The words a report uses for a value: the C++ overloads of `toString`, one
/// name over the types that have them (an enum gives a static text, a
/// timestamp an owned one).
pub trait SurveyText {
    type Text;
    fn text(self) -> Self::Text;
}

/// `toString(x)` of the C++: `to_string(Face::Left)`, `to_string(&timestamp)`.
pub fn to_string<T: SurveyText>(value: T) -> T::Text {
    value.text()
}

// ---- Provenance ----------------------------------------------------------------

/// The file name of `supplied`, with any directory the source itself gave
/// stripped off. "." and ".." are not names and come back empty.
///
/// SECURITY, and this is the reason the function exists. A field file is
/// untrusted text and it names other files - a coordinate list beside it, a raw
/// data file, a job. A name like "..\..\..\Windows\win.ini" must never become a
/// path this program opens and must never be recorded as though it were one: a
/// file must not be able to choose what gets read. Only the NAME survives; a
/// caller that wants the referenced file looks for it beside the file that named
/// it and nowhere else. That is the rule the archive reader already follows for
/// its ref_data references.
///
/// Separators are cut here rather than by Path::file_name() because what counts
/// as a separator THERE depends on the host: a POSIX build does not treat '\' as
/// one, so a Windows path inside a file read on Linux would pass through whole.
/// An untrusted name has to be cut the same way everywhere. ':' is cut as well,
/// so that a drive-relative "C:job.gsi" and an NTFS alternate data stream
/// "job.gsi:hidden" both come back as one plain name.
pub fn source_file_name(supplied: &str) -> String {
    // The separators are ASCII, so the byte after one starts a character.
    let name = match supplied.rfind(['/', '\\', ':']) {
        Some(cut) => &supplied[cut + 1..],
        None => supplied,
    };
    if name == "." || name == ".." {
        return String::new();
    }
    String::from(name)
}

/// Where an imported value came from: enough to point at the exact record of the
/// exact file, and no more.
///
/// Every imported point, observation, station and feature carries one. Two
/// things need it. An audit has to be able to answer "which file says this mark
/// is here?", and reprocessing has to be able to find the values a parser
/// produced when that parser is fixed - a value with no provenance cannot be
/// found, so it silently keeps whatever the old parser made of it.
///
/// `file_name` is a NAME, never a path: see source_file_name().
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceRecord {
    /// "Leica"; empty for a value this program computed.
    pub manufacturer: String,
    /// "GSI-16", as the format descriptor names it.
    pub format: String,
    /// The variant the parser ACTUALLY used, e.g. "16 byte words".
    pub format_version: String,
    /// File name only, no directory.
    pub file_name: String,
    /// 1-based record or line; 0 when the source has none.
    pub record_number: usize,
}

impl SourceRecord {
    /// True when this says anything at all. A default SourceRecord means "not
    /// imported", which is not the same as "imported from somewhere unnamed".
    pub fn known(&self) -> bool {
        !self.format.is_empty() || !self.file_name.is_empty()
    }
}

/// "site.gsi record 412 (Leica GSI-16)", or "unknown source" for a default
/// record. One phrasing, so five importers do not each invent their own for
/// their errors.
pub fn describe_source(source: &SourceRecord) -> String {
    if !source.known() {
        return String::from("unknown source");
    }
    let mut text = if source.file_name.is_empty() {
        String::from("<unnamed file>")
    } else {
        source.file_name.clone()
    };
    if source.record_number != 0 {
        text.push_str(&format!(" record {}", source.record_number));
    }
    let mut format = source.manufacturer.clone();
    for part in [&source.format, &source.format_version] {
        if part.is_empty() {
            continue;
        }
        if !format.is_empty() {
            format.push(' ');
        }
        format.push_str(part);
    }
    if !format.is_empty() {
        text.push_str(&format!(" ({format})"));
    }
    text
}

/// How a coordinate came to be.
///
/// Not a metadata string, because the distinction between a coordinate that was
/// MEASURED and one that was COMPUTED is load bearing twice over:
///   * an audit has to say which numbers in a report were observed and which
///     were derived from them - a derived number is evidence of nothing on its
///     own;
///   * reprocessing has to recompute the derived ones and leave the measured ones
///     exactly as they are. A traverse readjusted after an instrument height is
///     corrected must move the calculated marks and must not touch the control
///     it was hung on.
///
/// Neither is possible from a free-text field that some importers fill in and
/// others do not, so this is a field with a closed set of values.
///
/// One value per point rather than one per component (northing / easting /
/// elevation). The rejected alternative: no field format met so far states the
/// horizontal and the vertical provenance separately, and three fields that
/// every importer would have to fill identically are three chances to fill them
/// inconsistently. A point whose height was levelled onto observed horizontal
/// coordinates is Calculated, which is the safer of the two answers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoordinateSource {
    /// The source did not say.
    #[default]
    Unknown,
    /// Reduced from observations made at the instrument.
    FieldObserved,
    /// Computed here or by the instrument: traverse, resection, adjustment.
    Calculated,
    /// Keyed in or published: control off a certificate, a design value.
    Entered,
}

impl SurveyText for CoordinateSource {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            CoordinateSource::Unknown => "unknown",
            CoordinateSource::FieldObserved => "field observed",
            CoordinateSource::Calculated => "calculated",
            CoordinateSource::Entered => "entered",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyPoint {
    /// Unique within a network; doubles as the point name.
    pub id: String,
    pub northing: f64,
    pub easting: f64,
    /// ABSENT IS NOT ZERO. A coordinate list with no height column, a GSI block
    /// with no word 83, a LandXML CgPoint of two ordinates: each is a point whose
    /// height nobody measured, and 0.0 in its place is a real height at the
    /// datum. A surface built from it is pulled down to zero under every such
    /// point with nothing to say why. So the model carries "no height" as a
    /// value of its own and every consumer has to decide what it means for
    /// them: the drawing leaves the height property off, a level adjustment
    /// refuses a benchmark that has none.
    pub elevation: Option<f64>,
    /// Feature code, e.g. "IP", "EOP".
    pub code: String,
    /// Free text.
    pub description: String,
    pub metadata: BTreeMap<String, String>,
    pub coordinate_source: CoordinateSource,
    pub source: SourceRecord,
}

impl SurveyPoint {
    pub fn position(&self) -> Coordinate2 {
        Coordinate2 {
            northing: self.northing,
            easting: self.easting,
        }
    }
}

/// A point a source NAMES and gives no coordinates for: the target of a raw
/// observation before anything has reduced it, a backsight known only by its
/// number, a point a traverse record refers to and never lists.
///
/// A type of its own, and deliberately NOT a SurveyPoint with placeholder
/// ordinates. The first round of instrument parsers each needed this and each
/// improvised: one created the point at the origin with a marker in its
/// metadata, another at placeholder ordinates. Either reaches the drawing as a
/// real mark at (0, 0) the moment one consumer forgets to look for the marker -
/// and nothing about a point at the origin says it was never measured. Keeping
/// such a point OUT of SurveyProject::points means "every SurveyPoint has a
/// position" stays true by construction; the bridge cannot draw one because it
/// is not in the list it draws. The rejected alternative, an optional northing
/// and easting, would have let the two disagree and made every horizontal
/// consumer - the network, the COGO, the bridge - check for a case most of them
/// can never meet. Reducing observations to give these a position is the job of
/// a reduction step, which produces SurveyPoints with CoordinateSource::Calculated.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnpositionedPoint {
    /// Shares SurveyPoint's id space: unique across both lists.
    pub id: String,
    pub code: String,
    pub description: String,
    pub metadata: BTreeMap<String, String>,
    pub source: SourceRecord,
}

// ---- What the instrument did, and when ---------------------------------------------

/// Whether a correction is already in a recorded value.
///
/// Three states and not a bool, because "the file does not say" is the common
/// case and it is not the same as "not applied": an atmospheric correction
/// added a second time is as wrong as one left out, and the reduction has to
/// be able to tell the person which of the two it guessed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CorrectionState {
    /// The file does not say.
    #[default]
    Unknown,
    /// The instrument or controller already put it in the recorded value.
    Applied,
    /// The file states the value is raw in this respect.
    NotApplied,
}

impl SurveyText for CorrectionState {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            CorrectionState::Unknown => "not stated",
            CorrectionState::Applied => "applied by the instrument",
            CorrectionState::NotApplied => "not applied",
        }
    }
}

/// A calendar date and time exactly as a field file states it, with the time
/// system it names. Not a clock time point: a field file's clock is often local
/// and unlabelled, and converting it would invent a time zone the file never
/// gave. `year == 0` means "not stated".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyTimestamp {
    pub year: i32,
    /// 1..12
    pub month: i32,
    /// 1..31
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    pub second: f64,
    /// "UTC", "GPS", "local"; empty when the file does not say.
    pub time_system: String,
}

impl SurveyTimestamp {
    pub fn known(&self) -> bool {
        self.year != 0
    }
}

/// "2024-03-05T10:15:30.5 GPS", or "" for a timestamp that is not known. The
/// seconds are written with as many decimals as they need and no more.
impl SurveyText for &SurveyTimestamp {
    type Text = String;
    fn text(self) -> String {
        if !self.known() {
            return String::new();
        }
        // Shortest text that reads back as the same double, so a stored second
        // is not rounded to whatever a fixed precision thought was enough.
        let second_text = format_exact_real(self.second);
        let two_digits = |value: i32| {
            let text = format!("{value}");
            if text.len() < 2 {
                format!("0{text}")
            } else {
                text
            }
        };
        let mut text = format!(
            "{}-{}-{}T{}:{}:",
            self.year,
            two_digits(self.month),
            two_digits(self.day),
            two_digits(self.hour),
            two_digits(self.minute)
        );
        // Two integer digits, as ISO 8601 has them: "05.5", not "5.5".
        if self.second < 10.0 {
            text.push('0');
        }
        text.push_str(&second_text);
        if !self.time_system.is_empty() {
            text.push(' ');
            text.push_str(&self.time_system);
        }
        text
    }
}

/// The settings a total station recorded for one setup: which instrument, and
/// what it had already done to the numbers it wrote down.
///
/// Typed rather than left in SurveyStation::metadata, because the reduction
/// DECIDES from these: an atmospheric correction is applied automatically only
/// when the instrument did not apply one, and a prism constant only when the
/// shot says it is missing. A key/value bag would make every parser spell those
/// facts its own way and the reduction guess.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstrumentSettings {
    /// "Leica"
    pub make: String,
    /// "TS16"
    pub model: String,
    pub serial_number: String,
    /// Metres, as set on the instrument (e.g. -0.0344 for a Leica round prism
    /// against a zero-constant instrument, 0.0 for a Leica GPR1 on a Leica).
    pub prism_constant: Option<f64>,
    pub prism_constant_state: CorrectionState,
    /// Parts per million, as the instrument computed or was told it.
    pub atmospheric_ppm: Option<f64>,
    pub atmospheric_ppm_state: CorrectionState,
    pub temperature_celsius: Option<f64>,
    pub pressure_hectopascals: Option<f64>,
    pub relative_humidity_percent: Option<f64>,
    /// The coefficient of refraction the instrument used for its own reduced
    /// values (a recorded horizontal distance or height difference).
    pub refraction_coefficient: Option<f64>,
    pub curvature_refraction_state: CorrectionState,
    /// A grid or project scale factor the instrument or controller applied to
    /// the distances it recorded.
    pub scale_factor: Option<f64>,
    pub scale_factor_state: CorrectionState,
    pub time: SurveyTimestamp,
}

/// The a-priori precision of observations, used twice: a parser gives an
/// observation whose file states no standard deviation these values (every
/// observation must carry a sigma > 0, see validate_observation), and the
/// reduction replaces them with the ones the person set. Defaults are those of a
/// 3" total station with a 2 mm + 2 ppm EDM, which is a middling instrument: a
/// better one is under-weighted, which is the safe way round for an adjustment's
/// outlier test.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObservationPrecision {
    /// Radians.
    pub direction: f64,
    /// Radians.
    pub zenith: f64,
    /// Metres (the "a" of a mm + b ppm).
    pub distance_constant: f64,
    /// Parts per million (the "b").
    pub distance_ppm: f64,
    /// Metres.
    pub instrument_centring: f64,
    /// Metres.
    pub target_centring: f64,
    /// Metres, instrument and target heights.
    pub height_measurement: f64,
    /// Metres per sqrt(km) of level line.
    pub levelling_per_sqrt_kilometre: f64,
    /// Metres, where a file gives no covariance.
    pub gnss_horizontal: f64,
    /// Metres, where a file gives no covariance.
    pub gnss_vertical: f64,
}

impl Default for ObservationPrecision {
    #[allow(
        clippy::excessive_precision,
        reason = "The literal is the C++ default's, digit for digit."
    )]
    fn default() -> Self {
        Self {
            // 3" = 3 * pi / 648000
            direction: 1.4544410433286079e-05,
            // 3"
            zenith: 1.4544410433286079e-05,
            distance_constant: 0.002,
            distance_ppm: 2.0,
            instrument_centring: 0.001,
            target_centring: 0.001,
            height_measurement: 0.002,
            levelling_per_sqrt_kilometre: 0.001,
            gnss_horizontal: 0.010,
            gnss_vertical: 0.020,
        }
    }
}

/// sqrt(a^2 + (b * 1e-6 * d)^2): the EDM part only; centring is the
/// reduction's business because it depends on the geometry, not on the distance
/// alone.
pub fn distance_sigma(precision: &ObservationPrecision, distance: f64) -> f64 {
    precision
        .distance_constant
        .hypot(precision.distance_ppm * 1e-6 * distance)
}

// ---- Observations ------------------------------------------------------------
//
// Every observation carries the record it was read from, for the reasons given
// beside SourceRecord. It is part of the observation rather than a table beside
// it so that an observation moved between containers cannot lose its origin.

/// Which face of the telescope an observation was made on. Face I / face II in
/// the older books; Left is "vertical circle on the left of the observer".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Face {
    #[default]
    Unknown,
    Left,
    Right,
}

impl SurveyText for Face {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            Face::Unknown => "unknown face",
            Face::Left => "face left",
            Face::Right => "face right",
        }
    }
}

/// Which POINTING an observation belongs to: the horizontal direction, zenith
/// angle and slope distance of one shot share an index, so that the reduction
/// can pair a face-left shot with its face-right partner and reduce the three
/// together (a slope distance needs the zenith angle of the same pointing).
/// 0 means "the file does not group this observation with any other". Indices
/// are unique within one SurveyStation and carry no meaning beyond equality.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Pointing {
    pub index: usize,
    pub face: Face,
}

/// What a distance was measured to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TargetInfo {
    /// Metres, as recorded with the shot (which may differ from the setup's
    /// InstrumentSettings::prism_constant when the prism was changed mid setup).
    pub prism_constant: Option<f64>,
    pub prism_constant_state: CorrectionState,
    /// The file's own words: "prism", "reflectorless", "tape".
    pub target_type: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DistanceKind {
    #[default]
    Horizontal,
    Slope,
}

/// Distance from `from` to `to`. Slope distances carry the instrument and target
/// heights needed to reduce them; both are ignored for horizontal distances.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DistanceObservation {
    pub from: String,
    pub to: String,
    /// Metres, > 0.
    pub distance: f64,
    /// Metres.
    pub sigma: f64,
    pub kind: DistanceKind,
    pub instrument_height: f64,
    pub target_height: f64,
    pub source: SourceRecord,
    pub pointing: Pointing,
    pub target: TargetInfo,
}

/// Clockwise angle measured at `at`, turning from the backsight `from` to the
/// foresight `to`: azimuth(at->to) - azimuth(at->from), wrapped into [0, 2*pi).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HorizontalAngleObservation {
    pub at: String,
    pub from: String,
    pub to: String,
    pub angle: f64,
    /// Radians.
    pub sigma: f64,
    pub source: SourceRecord,
    pub pointing: Pointing,
}

/// Elevation angle above (+) or below (-) the horizon, in [-pi/2, pi/2].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VerticalAngleObservation {
    pub from: String,
    pub to: String,
    pub angle: f64,
    /// Radians.
    pub sigma: f64,
    pub instrument_height: f64,
    pub target_height: f64,
    pub source: SourceRecord,
    pub pointing: Pointing,
}

/// Angle from the zenith, in [0, pi]; pi/2 is horizontal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ZenithAngleObservation {
    pub from: String,
    pub to: String,
    pub angle: f64,
    /// Radians.
    pub sigma: f64,
    pub instrument_height: f64,
    pub target_height: f64,
    pub source: SourceRecord,
    pub pointing: Pointing,
}

/// A horizontal circle READING at `at` towards `to`, radians clockwise in
/// [0, 2*pi): what a total station actually records, before anything has
/// oriented it.
///
/// Its own kind, rather than a HorizontalAngleObservation from the backsight,
/// because a parser does not know the backsight reading of the same face when
/// it meets a shot (the backsight may be observed last, or on both faces), and
/// differencing early would throw away the face pairing the reduction needs.
/// The reduction turns directions into angles or azimuths for its network, and
/// gives a resection's reduced directions to the network adjustment as they
/// are, which takes the directions read at one point as a set with an
/// orientation unknown (network_adjustment.rs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HorizontalDirectionObservation {
    pub at: String,
    pub to: String,
    pub direction: f64,
    /// Radians.
    pub sigma: f64,
    pub source: SourceRecord,
    pub pointing: Pointing,
}

/// Grid azimuth of the line `from` -> `to` (gyro, astronomic or a known bearing).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AzimuthObservation {
    pub from: String,
    pub to: String,
    pub azimuth: f64,
    /// Radians.
    pub sigma: f64,
    pub source: SourceRecord,
}

/// GNSS baseline `from` -> `to`, already reduced to grid components.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GnssBaselineObservation {
    pub from: String,
    pub to: String,
    pub delta_northing: f64,
    pub delta_easting: f64,
    pub delta_up: f64,
    pub sigma_northing: f64,
    pub sigma_easting: f64,
    pub sigma_up: f64,
    pub source: SourceRecord,
}

/// GNSS position of `point` in the grid system of the network.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GnssPositionObservation {
    pub point: String,
    pub northing: f64,
    pub easting: f64,
    pub elevation: f64,
    pub sigma_northing: f64,
    pub sigma_easting: f64,
    pub sigma_elevation: f64,
    pub source: SourceRecord,
}

/// Levelled height difference H(to) - H(from). `length` is the length of the
/// level line in metres (0 when unknown); it is informational, the weight always
/// comes from `sigma`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelDifferenceObservation {
    pub from: String,
    pub to: String,
    pub height_difference: f64,
    /// Metres.
    pub sigma: f64,
    pub length: f64,
    pub source: SourceRecord,
}

// ---- GNSS as the receiver states it --------------------------------------------
//
// GnssBaselineObservation and GnssPositionObservation above are GRID values,
// already reduced into the network's system. What a receiver or a Trimble job
// actually records is earth-centred: ECEF coordinates, ECEF vectors, or
// latitude, longitude and ellipsoidal height. Turning those into grid needs a
// projection, which is geodesy, which the survey layer may not see; so a parser
// records them as they are and the reduction converts them through a function
// the caller supplies.

/// Earth-centred, earth-fixed, metres.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GeocentricCoordinate {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Latitude and longitude in radians (north and east positive), height above
/// the ellipsoid in metres.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GeodeticCoordinate {
    pub latitude: f64,
    pub longitude: f64,
    pub ellipsoidal_height: f64,
}

/// A symmetric 3x3 covariance, metres squared, in the frame of the value it
/// belongs to: X/Y/Z for a geocentric value; north/east/up (local, at the
/// point) for a geodetic one. All zero means the file gave none; otherwise the
/// diagonal must be positive.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GnssCovariance3 {
    pub xx: f64,
    pub yy: f64,
    pub zz: f64,
    pub xy: f64,
    pub xz: f64,
    pub yz: f64,
}

impl GnssCovariance3 {
    pub fn stated(&self) -> bool {
        self.xx != 0.0
            || self.yy != 0.0
            || self.zz != 0.0
            || self.xy != 0.0
            || self.xz != 0.0
            || self.yz != 0.0
    }
}

/// How an antenna height was measured. The reduction to the antenna reference
/// point depends on it, and a slant height reduced as a vertical one is a
/// centimetre-level error nobody sees.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AntennaHeightMethod {
    #[default]
    Unknown,
    /// Vertically to the antenna reference point (ARP).
    Vertical,
    /// To the edge of the antenna; needs the antenna's radius.
    Slant,
    /// Already reduced to the phase centre by the receiver.
    PhaseCentre,
    /// The file names a mark this list does not: see GnssAntenna::measured_to.
    Other,
}

impl SurveyText for AntennaHeightMethod {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            AntennaHeightMethod::Unknown => "not stated",
            AntennaHeightMethod::Vertical => "vertical to the antenna reference point",
            AntennaHeightMethod::Slant => "slant",
            AntennaHeightMethod::PhaseCentre => "to the phase centre",
            AntennaHeightMethod::Other => "other",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GnssAntenna {
    /// As the file gives it, e.g. the IGS name "LEIGS15     NONE". The C++
    /// member is `type`, a Rust keyword, hence the raw identifier.
    pub r#type: String,
    pub serial_number: String,
    /// Metres, as measured (RINEX "ANTENNA: DELTA H/E/N" H).
    pub height: f64,
    pub method: AntennaHeightMethod,
    /// The file's own words: "bottom of quick release".
    pub measured_to: String,
    /// Metres (RINEX DELTA E).
    pub east_offset: f64,
    /// Metres (RINEX DELTA N).
    pub north_offset: f64,
}

/// The kind of solution a receiver reports. Carried so that a report can say
/// "3 of these positions are float solutions" instead of weighting them all
/// alike without comment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GnssSolution {
    #[default]
    Unknown,
    Fixed,
    Float,
    Differential,
    Autonomous,
}

impl SurveyText for GnssSolution {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            GnssSolution::Unknown => "not stated",
            GnssSolution::Fixed => "fixed",
            GnssSolution::Float => "float",
            GnssSolution::Differential => "differential",
            GnssSolution::Autonomous => "autonomous",
        }
    }
}

/// A GNSS position of `point` in a global frame, as the file states it.
/// Exactly one of `geocentric` and `geodetic` is set (validate_observation).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GnssGlobalPositionObservation {
    pub point: String,
    pub geocentric: Option<GeocentricCoordinate>,
    pub geodetic: Option<GeodeticCoordinate>,
    /// See GnssCovariance3 for its frame.
    pub covariance: GnssCovariance3,
    /// As declared: "WGS 84", "ITRF2014"; empty when not.
    pub reference_frame: String,
    pub antenna: GnssAntenna,
    pub solution: GnssSolution,
    pub source: SourceRecord,
}

/// A GNSS vector `from` -> `to` in ECEF components, metres.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GnssGeocentricBaselineObservation {
    pub from: String,
    pub to: String,
    pub delta: GeocentricCoordinate,
    /// X/Y/Z
    pub covariance: GnssCovariance3,
    pub reference_frame: String,
    pub from_antenna: GnssAntenna,
    pub to_antenna: GnssAntenna,
    pub solution: GnssSolution,
    pub source: SourceRecord,
}

/// One observation of any kind: the C++ std::variant, whose alternatives become
/// the variants in the same order. New kinds are APPENDED, so that the index of
/// every older alternative stays what it was.
#[allow(
    clippy::large_enum_variant,
    reason = "The variants are the C++ variant's alternatives held by value, as observations are stored and compared there."
)]
#[derive(Clone, Debug, PartialEq)]
pub enum Observation {
    Distance(DistanceObservation),
    HorizontalAngle(HorizontalAngleObservation),
    VerticalAngle(VerticalAngleObservation),
    ZenithAngle(ZenithAngleObservation),
    Azimuth(AzimuthObservation),
    GnssBaseline(GnssBaselineObservation),
    GnssPosition(GnssPositionObservation),
    LevelDifference(LevelDifferenceObservation),
    HorizontalDirection(HorizontalDirectionObservation),
    GnssGlobalPosition(GnssGlobalPositionObservation),
    GnssGeocentricBaseline(GnssGeocentricBaselineObservation),
}

// The C++ converts each alternative to the variant implicitly; From does it here.
macro_rules! observation_from {
    ($($kind:ident => $variant:ident),* $(,)?) => {
        $(
            impl From<$kind> for Observation {
                fn from(observation: $kind) -> Self {
                    Observation::$variant(observation)
                }
            }
        )*
    };
}

observation_from! {
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

/// The pointing an observation belongs to, or None for a kind that has none
/// (azimuths, GNSS, level differences).
pub fn observation_pointing(observation: &Observation) -> Option<&Pointing> {
    match observation {
        Observation::Distance(o) => Some(&o.pointing),
        Observation::HorizontalAngle(o) => Some(&o.pointing),
        Observation::VerticalAngle(o) => Some(&o.pointing),
        Observation::ZenithAngle(o) => Some(&o.pointing),
        Observation::HorizontalDirection(o) => Some(&o.pointing),
        _ => None,
    }
}

/// Short name of the kind held by `observation`, for diagnostics.
pub fn observation_kind_name(observation: &Observation) -> String {
    String::from(match observation {
        Observation::Distance(o) => {
            if o.kind == DistanceKind::Horizontal {
                "horizontal distance"
            } else {
                "slope distance"
            }
        }
        Observation::HorizontalAngle(_) => "horizontal angle",
        Observation::VerticalAngle(_) => "vertical angle",
        Observation::ZenithAngle(_) => "zenith angle",
        Observation::Azimuth(_) => "azimuth",
        Observation::GnssBaseline(_) => "GNSS baseline",
        Observation::GnssPosition(_) => "GNSS position",
        Observation::LevelDifference(_) => "level difference",
        Observation::HorizontalDirection(_) => "horizontal direction",
        Observation::GnssGlobalPosition(o) => {
            if o.geocentric.is_some() {
                "GNSS geocentric position"
            } else {
                "GNSS geodetic position"
            }
        }
        Observation::GnssGeocentricBaseline(_) => "GNSS geocentric baseline",
    })
}

// The points an observation names, as views: never more than three.
fn referenced_point_views(observation: &Observation) -> Vec<&str> {
    match observation {
        Observation::HorizontalAngle(o) => vec![o.at.as_str(), o.from.as_str(), o.to.as_str()],
        Observation::HorizontalDirection(o) => vec![o.at.as_str(), o.to.as_str()],
        Observation::GnssPosition(o) => vec![o.point.as_str()],
        Observation::GnssGlobalPosition(o) => vec![o.point.as_str()],
        Observation::Distance(o) => vec![o.from.as_str(), o.to.as_str()],
        Observation::VerticalAngle(o) => vec![o.from.as_str(), o.to.as_str()],
        Observation::ZenithAngle(o) => vec![o.from.as_str(), o.to.as_str()],
        Observation::Azimuth(o) => vec![o.from.as_str(), o.to.as_str()],
        Observation::GnssBaseline(o) => vec![o.from.as_str(), o.to.as_str()],
        Observation::LevelDifference(o) => vec![o.from.as_str(), o.to.as_str()],
        Observation::GnssGeocentricBaseline(o) => vec![o.from.as_str(), o.to.as_str()],
    }
}

/// Ids of the points the observation refers to, in declaration order.
pub fn referenced_points(observation: &Observation) -> Vec<String> {
    referenced_point_views(observation)
        .into_iter()
        .map(String::from)
        .collect()
}

fn describe(observation: &Observation) -> String {
    let mut text = observation_kind_name(observation);
    let mut separator = " ";
    for id in referenced_point_views(observation) {
        text.push_str(separator);
        text.push_str(if id.is_empty() { "<empty>" } else { id });
        separator = " -> ";
    }
    text
}

fn invalid(observation: &Observation, message: String) -> Error {
    Error::new(ErrorCode::InvalidSurveyObservation, message).with_context(describe(observation))
}

// Empty string when every check passes, otherwise the reason.
fn check_finite(values: &[(&str, f64)]) -> String {
    for (name, value) in values {
        if !value.is_finite() {
            return format!("{name} is not finite");
        }
    }
    String::new()
}

fn check_sigmas(sigmas: &[(&str, f64)]) -> String {
    for (name, sigma) in sigmas {
        if !sigma.is_finite() || !(*sigma > 0.0) {
            return format!("{name} must be a positive finite standard deviation");
        }
    }
    String::new()
}

fn check_stations(ids: &[&str]) -> String {
    for (i, id) in ids.iter().enumerate() {
        if id.is_empty() {
            return String::from("station id is empty");
        }
        if ids[i + 1..].contains(id) {
            return String::from("stations of an observation must be distinct");
        }
    }
    String::new()
}

// A covariance the file did not state is allowed (the reduction weights the
// value from ObservationPrecision); a stated one needs a positive diagonal.
fn check_covariance(covariance: &GnssCovariance3) -> String {
    if !covariance.stated() {
        return String::new();
    }
    let reason = check_finite(&[
        ("covariance XY", covariance.xy),
        ("covariance XZ", covariance.xz),
        ("covariance YZ", covariance.yz),
    ]);
    if !reason.is_empty() {
        return reason;
    }
    for (name, variance) in [
        ("variance X", covariance.xx),
        ("variance Y", covariance.yy),
        ("variance Z", covariance.zz),
    ] {
        if !variance.is_finite() || !(variance > 0.0) {
            return format!("{name} must be a positive finite variance");
        }
    }
    String::new()
}

// First non-empty reason wins.
fn first_of(reasons: Vec<String>) -> String {
    reasons
        .into_iter()
        .find(|reason| !reason.is_empty())
        .unwrap_or_default()
}

fn when(condition: bool, reason: &str) -> String {
    if condition {
        String::new()
    } else {
        String::from(reason)
    }
}

fn value_problem(observation: &Observation) -> String {
    match observation {
        Observation::Distance(o) => first_of(vec![
            check_finite(&[
                ("distance", o.distance),
                ("instrument height", o.instrument_height),
                ("target height", o.target_height),
            ]),
            check_sigmas(&[("sigma", o.sigma)]),
            when(o.distance > 0.0, "distance must be positive"),
        ]),
        Observation::HorizontalAngle(o) => first_of(vec![
            check_finite(&[("angle", o.angle)]),
            check_sigmas(&[("sigma", o.sigma)]),
        ]),
        Observation::VerticalAngle(o) => first_of(vec![
            check_finite(&[
                ("angle", o.angle),
                ("instrument height", o.instrument_height),
                ("target height", o.target_height),
            ]),
            check_sigmas(&[("sigma", o.sigma)]),
            when(
                o.angle.abs() <= HALF_PI,
                "vertical angle must lie in [-pi/2, pi/2]",
            ),
        ]),
        Observation::ZenithAngle(o) => first_of(vec![
            check_finite(&[
                ("angle", o.angle),
                ("instrument height", o.instrument_height),
                ("target height", o.target_height),
            ]),
            check_sigmas(&[("sigma", o.sigma)]),
            when(
                (0.0..=PI).contains(&o.angle),
                "zenith angle must lie in [0, pi]",
            ),
        ]),
        Observation::Azimuth(o) => first_of(vec![
            check_finite(&[("azimuth", o.azimuth)]),
            check_sigmas(&[("sigma", o.sigma)]),
        ]),
        Observation::GnssBaseline(o) => first_of(vec![
            check_finite(&[
                ("delta northing", o.delta_northing),
                ("delta easting", o.delta_easting),
                ("delta up", o.delta_up),
            ]),
            check_sigmas(&[
                ("sigma northing", o.sigma_northing),
                ("sigma easting", o.sigma_easting),
                ("sigma up", o.sigma_up),
            ]),
        ]),
        Observation::GnssPosition(o) => first_of(vec![
            check_finite(&[
                ("northing", o.northing),
                ("easting", o.easting),
                ("elevation", o.elevation),
            ]),
            check_sigmas(&[
                ("sigma northing", o.sigma_northing),
                ("sigma easting", o.sigma_easting),
                ("sigma elevation", o.sigma_elevation),
            ]),
        ]),
        Observation::LevelDifference(o) => first_of(vec![
            check_finite(&[
                ("height difference", o.height_difference),
                ("length", o.length),
            ]),
            check_sigmas(&[("sigma", o.sigma)]),
            when(o.length >= 0.0, "length must not be negative"),
        ]),
        Observation::HorizontalDirection(o) => first_of(vec![
            check_finite(&[("direction", o.direction)]),
            check_sigmas(&[("sigma", o.sigma)]),
        ]),
        Observation::GnssGlobalPosition(o) => {
            let reason = match (&o.geocentric, &o.geodetic) {
                (Some(geocentric), None) => check_finite(&[
                    ("X", geocentric.x),
                    ("Y", geocentric.y),
                    ("Z", geocentric.z),
                ]),
                (None, Some(geodetic)) => first_of(vec![
                    check_finite(&[
                        ("latitude", geodetic.latitude),
                        ("longitude", geodetic.longitude),
                        ("ellipsoidal height", geodetic.ellipsoidal_height),
                    ]),
                    when(
                        geodetic.latitude.abs() <= HALF_PI,
                        "latitude must lie in [-pi/2, pi/2]",
                    ),
                ]),
                _ => {
                    return String::from(
                        "a GNSS position must be either geocentric or geodetic, not both or neither",
                    );
                }
            };
            first_of(vec![reason, check_covariance(&o.covariance)])
        }
        Observation::GnssGeocentricBaseline(o) => first_of(vec![
            check_finite(&[
                ("delta X", o.delta.x),
                ("delta Y", o.delta.y),
                ("delta Z", o.delta.z),
            ]),
            check_covariance(&o.covariance),
        ]),
    }
}

/// Checks the values of an observation on their own: finite numbers, sigma > 0,
/// distance > 0, angles in range, distinct non-empty station ids. Does not know
/// whether the stations exist; SurveyNetwork::add_observation checks that.
/// InvalidSurveyObservation on failure.
pub fn validate_observation(observation: &Observation) -> Status {
    let reason = check_stations(&referenced_point_views(observation));
    if !reason.is_empty() {
        return Err(invalid(observation, reason));
    }
    let reason = value_problem(observation);
    if !reason.is_empty() {
        return Err(invalid(observation, reason));
    }
    Ok(())
}

/// Copy with azimuths and horizontal angles wrapped into [0, 2*pi). Other
/// observations are returned unchanged.
pub fn normalized_observation(mut observation: Observation) -> Observation {
    match &mut observation {
        Observation::HorizontalAngle(angle) => angle.angle = normalize_angle(angle.angle),
        Observation::Azimuth(azimuth) => azimuth.azimuth = normalize_angle(azimuth.azimuth),
        Observation::HorizontalDirection(direction) => {
            direction.direction = normalize_angle(direction.direction);
        }
        _ => {}
    }
    observation
}

/// The record an observation was read from, whichever kind it is, so that a
/// caller reporting on a mixed list of observations does not have to match the
/// enum itself.
pub fn observation_source(observation: &Observation) -> &SourceRecord {
    match observation {
        Observation::Distance(o) => &o.source,
        Observation::HorizontalAngle(o) => &o.source,
        Observation::VerticalAngle(o) => &o.source,
        Observation::ZenithAngle(o) => &o.source,
        Observation::Azimuth(o) => &o.source,
        Observation::GnssBaseline(o) => &o.source,
        Observation::GnssPosition(o) => &o.source,
        Observation::LevelDifference(o) => &o.source,
        Observation::HorizontalDirection(o) => &o.source,
        Observation::GnssGlobalPosition(o) => &o.source,
        Observation::GnssGeocentricBaseline(o) => &o.source,
    }
}

pub fn set_observation_source(observation: &mut Observation, source: SourceRecord) {
    let target = match observation {
        Observation::Distance(o) => &mut o.source,
        Observation::HorizontalAngle(o) => &mut o.source,
        Observation::VerticalAngle(o) => &mut o.source,
        Observation::ZenithAngle(o) => &mut o.source,
        Observation::Azimuth(o) => &mut o.source,
        Observation::GnssBaseline(o) => &mut o.source,
        Observation::GnssPosition(o) => &mut o.source,
        Observation::LevelDifference(o) => &mut o.source,
        Observation::HorizontalDirection(o) => &mut o.source,
        Observation::GnssGlobalPosition(o) => &mut o.source,
        Observation::GnssGeocentricBaseline(o) => &mut o.source,
    };
    *target = source;
}

// ---- Setups and control --------------------------------------------------------

/// One instrument setup. Target heights belong to the individual observations.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Station {
    /// Setup identifier, unique within a network.
    pub id: String,
    /// Occupied point.
    pub point_id: String,
    pub instrument_height: f64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ControlConstraint {
    /// Adjusted like any other unknown.
    #[default]
    Free,
    /// Held at its published value, removed from the unknowns.
    Fixed,
    /// Published value enters the adjustment as an observation.
    Weighted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ControlComponent {
    pub constraint: ControlConstraint,
    /// Metres; required > 0 when Weighted, ignored otherwise.
    pub sigma: f64,
}

/// Declares which coordinate components of a point are control. The published
/// values are the coordinates of the SurveyPoint itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControlPoint {
    pub point_id: String,
    pub northing: ControlComponent,
    pub easting: ControlComponent,
    pub elevation: ControlComponent,
}

impl ControlPoint {
    pub fn fixed_horizontal(point_id: impl Into<String>) -> Self {
        let fixed = ControlComponent {
            constraint: ControlConstraint::Fixed,
            sigma: 0.0,
        };
        ControlPoint {
            point_id: point_id.into(),
            northing: fixed,
            easting: fixed,
            ..Default::default()
        }
    }

    pub fn fixed_vertical(point_id: impl Into<String>) -> Self {
        ControlPoint {
            point_id: point_id.into(),
            elevation: ControlComponent {
                constraint: ControlConstraint::Fixed,
                sigma: 0.0,
            },
            ..Default::default()
        }
    }

    pub fn fixed3d(point_id: impl Into<String>) -> Self {
        let mut control = Self::fixed_horizontal(point_id);
        control.elevation.constraint = ControlConstraint::Fixed;
        control
    }

    pub fn weighted_horizontal(
        point_id: impl Into<String>,
        sigma_northing: f64,
        sigma_easting: f64,
    ) -> Self {
        ControlPoint {
            point_id: point_id.into(),
            northing: ControlComponent {
                constraint: ControlConstraint::Weighted,
                sigma: sigma_northing,
            },
            easting: ControlComponent {
                constraint: ControlConstraint::Weighted,
                sigma: sigma_easting,
            },
            ..Default::default()
        }
    }

    pub fn weighted_vertical(point_id: impl Into<String>, sigma: f64) -> Self {
        ControlPoint {
            point_id: point_id.into(),
            elevation: ControlComponent {
                constraint: ControlConstraint::Weighted,
                sigma,
            },
            ..Default::default()
        }
    }
}

// ---- Traverse ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TraverseKind {
    /// Starts on control, ends on a new point: no closure check.
    #[default]
    Open,
    /// Returns to its first station; interior-angle procedure.
    ClosedLoop,
    /// Runs from one control point to another.
    Link,
}

/// One occupied traverse station.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraverseSetup {
    pub station_id: String,
    /// Clockwise, from the back station to the forward station.
    pub angle: f64,
    /// Horizontal, to the forward station; metres, > 0.
    pub distance: f64,
}

/// Angle turned at the end station of a link traverse, from the last traverse
/// station to a reference mark whose azimuth is known.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TraverseClosingAngle {
    /// Clockwise, last traverse station -> reference mark.
    pub angle: f64,
    /// Known azimuth end station -> reference mark.
    pub reference_azimuth: f64,
}

/// A traverse observed with angles and horizontal distances; self-contained.
///
///   Open / Link   `start_azimuth` is the known azimuth from the first station to
///                 its backsight reference; setups[0].angle is turned from that
///                 reference to the second station. The forward station of the
///                 last setup is `end_station_id`.
///   ClosedLoop    `start_azimuth` is the known (or assumed) azimuth of the first
///                 leg setups[0] -> setups[1]; setups[0].angle is turned from the
///                 last station to the second one; the last setup sights forward
///                 to setups[0]. `end_station_id`, `end` and `closing_angle` are
///                 unused. A loop oriented on an external reference mark is
///                 modelled as a Link traverse whose end equals its start.
///   Link          `end` holds the known coordinates of `end_station_id`;
///                 `closing_angle` is optional (without it there is no angular
///                 check, only a linear one).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Traverse {
    pub name: String,
    pub kind: TraverseKind,
    /// Known coordinates of setups[0].
    pub start: Coordinate2,
    pub start_azimuth: f64,
    pub setups: Vec<TraverseSetup>,
    pub end_station_id: String,
    pub end: Coordinate2,
    pub closing_angle: Option<TraverseClosingAngle>,
}

// ---- Field setups, features and the project one import produces -----------------

/// SurveyStation::metadata key a reader sets where the file marks the end of the
/// observations its field software computed a resection from (the opcode field
/// file's 129): the value is "record N", N the record number that ends them, in
/// the numbering of the observations' SourceRecord::record_number. The
/// reduction's resection then takes only the setup's observations read before
/// record N; those after it - a controller's checks, often taken at the end of
/// the setup - are checks of the resected station, as they were for the field
/// software. Without the key every observation of the setup may enter it.
pub const RESECTION_END_METADATA: &str = "resection end";

/// One instrument setup as a field file records it: the setup, what the
/// instrument was oriented on, and everything shot from there.
///
/// The setup is a `Station`, not a repetition of its fields. A second struct
/// carrying an id, an occupied point and an instrument height would be a second
/// way of saying the same thing, and the two would drift apart the first time
/// one of them gained a field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyStation {
    pub setup: Station,
    /// The point the instrument was oriented on. Empty when the file records no
    /// backsight - a free setup, or a resection stated only by its result.
    pub backsight_point_id: String,
    /// The horizontal circle reading set on the backsight, radians clockwise in
    /// [0, 2*pi). Absent when the file does not say, and absent is not zero: a
    /// setup oriented on 0 00 00 and a setup whose orientation was not recorded
    /// produce different bearings, so an importer must not invent one. With no
    /// backsight named, the reduction takes a setup that has one as a circle
    /// set to read azimuths - its readings are its bearings - whatever the
    /// value. (The name is older than `stated_backsight_azimuth`: this is the
    /// circle, never the azimuth.)
    pub backsight_azimuth: Option<f64>,
    /// The azimuth of the line to the backsight as the FILE states it - keyed
    /// in, or worked out by the field software - radians clockwise from north
    /// in [0, 2*pi). Absent when the file states none. Where the backsight's
    /// coordinates orient the setup this is not used; where nothing places the
    /// backsight, the reduction orients the setup on it less the setup's own
    /// reading on the backsight, or with no reading less the circle above -
    /// the field software's own orientation correction. Not the circle: a
    /// circle set to 0 00 00 on a backsight 123 degrees away is the usual
    /// case, and taking either for the other turns every bearing by the
    /// difference.
    pub stated_backsight_azimuth: Option<f64>,
    /// In the order the file gives them. The backsight's own observation, where
    /// the file records one, is one of these like any other.
    pub observations: Vec<Observation>,
    pub metadata: BTreeMap<String, String>,
    pub source: SourceRecord,
    /// What the instrument was and what it had already done to the numbers.
    pub instrument: InstrumentSettings,
}

/// One GNSS observing session, as a RINEX observation header (or a receiver's
/// own job) describes it. Metadata, not observations: raw phase and code are
/// not read into the model - a RINEX file is imported for what it says about
/// the occupation (marker, antenna, span), and processing it is out of scope.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GnssSession {
    pub marker_name: String,
    pub marker_number: String,
    pub receiver_type: String,
    pub receiver_serial: String,
    pub receiver_firmware: String,
    pub antenna: GnssAntenna,
    /// The header's approximate position, ECEF metres. Approximate is the word:
    /// it is typically a navigation solution, metres out.
    pub approximate_position: Option<GeocentricCoordinate>,
    pub first_epoch: SurveyTimestamp,
    pub last_epoch: SurveyTimestamp,
    pub interval_seconds: Option<f64>,
    pub epoch_count: usize,
    /// Distinct satellites seen, per system, keyed by the system's name
    /// ("GPS", "GLONASS", "Galileo", "BeiDou", "QZSS", "NavIC", "SBAS").
    pub satellites_per_system: BTreeMap<String, usize>,
    /// "3.04"
    pub format_version: String,
    pub observer: String,
    pub agency: String,
    pub source: SourceRecord,
}

/// A coded string: the points one field code strung together, in the order
/// they were observed. That is what a kerb line, a fence or a pipe run IS in
/// raw field data. The geometry is made from it later by the survey coding
/// rules, not here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyFeature {
    /// String name where the file gives one; may be empty.
    pub name: String,
    /// The field code, e.g. "KB", "FL".
    pub code: String,
    /// In observation order.
    pub point_ids: Vec<String>,
    pub closed: bool,
    pub description: String,
    pub metadata: BTreeMap<String, String>,
    pub source: SourceRecord,
}

/// The coordinate system a source file DECLARES, and nothing else.
///
/// Deliberately not a geodesy type and deliberately not a transformation. What
/// a field file states is a name, sometimes an EPSG code, and very often nothing
/// at all. Turning that into a transformation needs BOTH ends of it, and the far
/// end is the drawing the data is being imported into, which an importer does
/// not know. Transforming with one end guessed silently moves a survey - the
/// failure this type exists to make impossible. The declaration is recorded,
/// carried, and put in front of a person who can decide.
///
/// Built through the factories so that `unknown` cannot drift away from the
/// fields beside it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclaredCoordinateSystem {
    /// Exactly as the file spells it.
    pub name: String,
    /// 0 when the file gave none.
    pub epsg_code: i32,
    pub unknown: bool,
}

impl Default for DeclaredCoordinateSystem {
    fn default() -> Self {
        Self {
            name: String::new(),
            epsg_code: 0,
            unknown: true,
        }
    }
}

impl DeclaredCoordinateSystem {
    pub fn named(name: impl Into<String>) -> Self {
        let name = name.into();
        DeclaredCoordinateSystem {
            // a blank declaration declares nothing
            unknown: name.is_empty(),
            name,
            epsg_code: 0,
        }
    }

    /// The C++ default `name = {}` is an empty name here.
    pub fn epsg(code: i32, name: impl Into<String>) -> Self {
        let mut system = Self::named(name);
        // EPSG dataset codes are positive integers; 0 is the "none given" marker
        // of the field itself, so a file stating 0 has stated nothing usable.
        if code > 0 {
            system.epsg_code = code;
            system.unknown = false;
        }
        system
    }
}

/// The units a file declares ITS OWN numbers in.
///
/// The model is always metres and radians (see the conventions at the top of
/// this file): an importer converts on the way in, and this records what it
/// converted FROM, so a stored value can be checked against the figure printed
/// in the file. An importer that meets a unit it cannot convert must fail. It
/// must never store the raw number and note the unit here, because everything
/// downstream reads metres and would not look.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LinearUnit {
    #[default]
    Unknown,
    Metres,
    Feet,
    UsSurveyFeet,
    Links,
}

/// DegreesMinutesSeconds is a NOTATION whose unit is degrees, but instruments
/// and LandXML declare it as though it were a unit, so it is recorded as they
/// state it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AngularUnit {
    #[default]
    Unknown,
    Radians,
    DecimalDegrees,
    DegreesMinutesSeconds,
    Gons,
    Mils,
}

impl SurveyText for LinearUnit {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            LinearUnit::Unknown => "unknown",
            LinearUnit::Metres => "metres",
            LinearUnit::Feet => "feet",
            LinearUnit::UsSurveyFeet => "US survey feet",
            LinearUnit::Links => "links",
        }
    }
}

impl SurveyText for AngularUnit {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            AngularUnit::Unknown => "unknown",
            AngularUnit::Radians => "radians",
            AngularUnit::DecimalDegrees => "decimal degrees",
            AngularUnit::DegreesMinutesSeconds => "degrees, minutes and seconds",
            AngularUnit::Gons => "gons",
            AngularUnit::Mils => "mils",
        }
    }
}

/// Metres in one `unit`, as the exact ratio katana_math::unit_ratio defines:
/// Feet is the international foot (EPSG:9002), UsSurveyFeet the US survey foot
/// (EPSG:9003), Links the international link of 0.201168 m (EPSG:9098).
/// InvalidArgument for Unknown - there is no default unit, because a factor of
/// 1 silently applied to a file in feet puts a survey 3.28 times too far from
/// the origin and every mark on it looks fine.
pub fn metres_per(unit: LinearUnit) -> Result<UnitRatio> {
    match unit {
        LinearUnit::Unknown => Err(Error::new(
            ErrorCode::InvalidArgument,
            "the linear unit is unknown, so its numbers cannot be converted to metres",
        )
        .with_context("a unit must be declared; none is assumed")),
        LinearUnit::Metres => Ok(units::METRE),
        LinearUnit::Feet => Ok(units::INTERNATIONAL_FOOT),
        LinearUnit::UsSurveyFeet => Ok(units::US_SURVEY_FOOT),
        LinearUnit::Links => Ok(units::INTERNATIONAL_LINK),
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DeclaredUnits {
    pub linear: LinearUnit,
    pub angular: AngularUnit,
}

/// Everything one import produced, as the source stated it.
///
/// A plain aggregate rather than a type with invariants: an importer fills it
/// record by record, and a file that fails half way through is still worth
/// reporting on. What SurveyNetwork enforces at every insert, this checks once
/// at the end, in validate_project().
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyProject {
    pub name: String,
    pub coordinate_system: DeclaredCoordinateSystem,
    pub units: DeclaredUnits,
    pub points: Vec<SurveyPoint>,
    /// Named by the source with no coordinates - see UnpositionedPoint. An
    /// observation, station or feature may refer to one of these exactly as it
    /// may to a point; nothing may draw one.
    pub unpositioned_points: Vec<UnpositionedPoint>,
    pub stations: Vec<SurveyStation>,
    /// Observations belonging to no one setup: levelling runs, GNSS vectors.
    /// An observation taken FROM a setup lives on that SurveyStation instead, so
    /// that it is not recorded twice.
    pub observations: Vec<Observation>,
    pub traverses: Vec<Traverse>,
    pub features: Vec<SurveyFeature>,
    pub metadata: BTreeMap<String, String>,
    /// The file as a whole; its record_number is 0.
    pub source: SourceRecord,
    /// Points the FILE marks as control (a Leica fixed point, a Trimble
    /// "control" class). The published values are the points' own coordinates.
    /// What the reduction actually holds is the person's choice in its
    /// settings, which start from these.
    pub control_points: Vec<ControlPoint>,
    /// GNSS sessions the file describes (RINEX headers, receiver jobs).
    pub gnss_sessions: Vec<GnssSession>,
}

fn missing_point(point_id: &str, context: String) -> Error {
    Error::new(
        ErrorCode::NotFound,
        format!("point '{point_id}' is not in the project"),
    )
    .with_context(context)
}

// The ids of a project's points as views into the project, sorted once and
// searched by bisection.
//
// Every import is validated, so this is on the import path of every file. An
// earlier ordered set copied every id and referenced_points() copied every
// observation's ids again; on a synthetic job of 2,000 setups and 600,000 raw
// observations that was 2.5 s of a C++ Debug build before a single value had
// been looked at.
#[derive(Default)]
struct PointIds<'a> {
    ids: Vec<&'a str>,
}

impl<'a> PointIds<'a> {
    fn reserve(&mut self, count: usize) {
        self.ids.reserve(count);
    }

    fn add(&mut self, id: &'a str) {
        self.ids.push(id);
    }

    // Sorts; true when no id occurs twice.
    fn seal(&mut self) -> bool {
        self.ids.sort_unstable();
        !self.ids.windows(2).any(|pair| pair[0] == pair[1])
    }

    fn contains(&self, id: &str) -> bool {
        self.ids.binary_search(&id).is_ok()
    }
}

// validate_observation() plus "every point it names is in the project". A
// failure is worded by validate_observation() itself, so the two cannot
// disagree.
fn check_project_observation(
    observation: &Observation,
    points: &PointIds<'_>,
    context: &str,
) -> Status {
    let refs = referenced_point_views(observation);
    // Non-empty and distinct, the test check_stations words a failure of.
    let stations_usable = check_stations(&refs).is_empty();
    if !stations_usable || !value_problem(observation).is_empty() {
        return validate_observation(observation);
    }
    for id in refs {
        if !points.contains(id) {
            return Err(missing_point(
                id,
                format!("{context}: {}", describe(observation)),
            ));
        }
    }
    Ok(())
}

// The per-point checks. Duplicates are looked for only when the sorted ids
// have already shown there is one, and then in file order, so the error is
// the one the file's order makes first - the same one on every run.
fn check_points(project: &SurveyProject, look_for_duplicates: bool) -> Status {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for point in &project.points {
        if point.id.is_empty() {
            return Err(
                Error::new(ErrorCode::InvalidArgument, "a point has an empty id")
                    .with_context(describe_source(&point.source)),
            );
        }
        if look_for_duplicates && !seen.insert(&point.id) {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                format!("duplicate point id '{}'", point.id),
            )
            .with_context(describe_source(&point.source)));
        }
        let mut reason = check_finite(&[("northing", point.northing), ("easting", point.easting)]);
        if reason.is_empty()
            && let Some(elevation) = point.elevation
        {
            reason = check_finite(&[("elevation", elevation)]);
        }
        if !reason.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("point '{}': {reason}", point.id),
            )
            .with_context(describe_source(&point.source)));
        }
    }
    for point in &project.unpositioned_points {
        if point.id.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "an unpositioned point has an empty id",
            )
            .with_context(describe_source(&point.source)));
        }
        if look_for_duplicates && !seen.insert(&point.id) {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                format!(
                    "duplicate point id '{}' (a point may not be both positioned and unpositioned)",
                    point.id
                ),
            )
            .with_context(describe_source(&point.source)));
        }
    }
    Ok(())
}

/// Checks one project's internal consistency, so that importers do not each
/// invent their own check and disagree about what a valid project is:
///   * point ids are non-empty and unique across `points` and
///     `unpositioned_points` together, and coordinates are finite (an absent
///     elevation is valid; a present one must be finite);
///   * every reference below resolves to a point in EITHER list;
///   * station ids are non-empty and unique, and every occupied and backsighted
///     point is in the project;
///   * every feature names at least one point and every point it names is in
///     the project;
///   * every observation, on a station or loose, passes validate_observation()
///     and refers only to points that are in the project;
///   * every control point names a POSITIONED point (control with no published
///     value is a contradiction), and a Weighted component has a sigma > 0.
///
/// Traverses are NOT checked against the points: a Traverse is self-contained
/// by design (it carries its own start and end coordinates), so its station ids
/// are names of its own and need not be project points.
/// InvalidArgument / AlreadyExists / NotFound / InvalidSurveyObservation, with
/// the offender named in the error context.
pub fn validate_project(project: &SurveyProject) -> Status {
    let mut points = PointIds::default();
    points.reserve(project.points.len() + project.unpositioned_points.len());
    for point in &project.points {
        points.add(&point.id);
    }
    for point in &project.unpositioned_points {
        points.add(&point.id);
    }
    let unique = points.seal();
    check_points(project, !unique)?;

    let mut station_ids: BTreeSet<&str> = BTreeSet::new();
    for station in &project.stations {
        let setup = &station.setup;
        if setup.id.is_empty() {
            return Err(
                Error::new(ErrorCode::InvalidArgument, "a station has an empty id")
                    .with_context(describe_source(&station.source)),
            );
        }
        if !station_ids.insert(&setup.id) {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                format!("duplicate station id '{}'", setup.id),
            )
            .with_context(describe_source(&station.source)));
        }
        if !points.contains(&setup.point_id) {
            return Err(missing_point(
                &setup.point_id,
                format!("station {}", setup.id),
            ));
        }
        let reason = check_finite(&[("instrument height", setup.instrument_height)]);
        if !reason.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("station {}: {reason}", setup.id),
            )
            .with_context(describe_source(&station.source)));
        }
        if !station.backsight_point_id.is_empty() && !points.contains(&station.backsight_point_id) {
            return Err(missing_point(
                &station.backsight_point_id,
                format!("backsight of station {}", setup.id),
            ));
        }
        if let Some(azimuth) = station.backsight_azimuth
            && !azimuth.is_finite()
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("station {}: backsight azimuth is not finite", setup.id),
            )
            .with_context(describe_source(&station.source)));
        }
        if let Some(azimuth) = station.stated_backsight_azimuth
            && !azimuth.is_finite()
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "station {}: the azimuth the file states for the backsight is not finite",
                    setup.id
                ),
            )
            .with_context(describe_source(&station.source)));
        }
        let context = format!("station {}", setup.id);
        for observation in &station.observations {
            check_project_observation(observation, &points, &context)?;
        }
    }

    let loose_context = "project observation";
    for observation in &project.observations {
        check_project_observation(observation, &points, loose_context)?;
    }

    let mut positioned = PointIds::default();
    if !project.control_points.is_empty() {
        positioned.reserve(project.points.len());
        for point in &project.points {
            positioned.add(&point.id);
        }
        // duplicates were refused above
        let _ = positioned.seal();
    }
    for control in &project.control_points {
        if !positioned.contains(&control.point_id) {
            return Err(Error::new(
                ErrorCode::NotFound,
                format!(
                    "control point '{}' is not a positioned point of the project",
                    control.point_id
                ),
            )
            .with_context("control needs a published value to hold"));
        }
        for (name, component) in [
            ("northing", &control.northing),
            ("easting", &control.easting),
            ("elevation", &control.elevation),
        ] {
            if component.constraint == ControlConstraint::Weighted
                && (!component.sigma.is_finite() || !(component.sigma > 0.0))
            {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "control point '{}': the weighted {name} needs a positive standard deviation",
                        control.point_id
                    ),
                ));
            }
        }
    }

    for feature in &project.features {
        let label = format!(
            "'{}'",
            if feature.name.is_empty() {
                &feature.code
            } else {
                &feature.name
            }
        );
        if feature.point_ids.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("feature {label} names no points"),
            )
            .with_context(describe_source(&feature.source)));
        }
        for point_id in &feature.point_ids {
            if !points.contains(point_id) {
                return Err(missing_point(point_id, format!("feature {label}")));
            }
        }
    }

    Ok(())
}
