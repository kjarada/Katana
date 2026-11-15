//! How raw field observations are reduced and adjusted: every choice a person
//! can make about it, the defaults they start from, and a stable text form.
//!
//! A type of its own, apart from reduction.rs, because three things hold one:
//! a survey job stores the settings it was last adjusted with (so the person can
//! go back and change them), the reduction report prints the settings it used,
//! and the reduction reads them.
//!
//! Units follow the survey layer: metres and radians throughout, including the
//! tolerances and the a-priori standard deviations. The text form stores the
//! same numbers, so it round-trips bit for bit; a person never reads it (the
//! wizard shows seconds of arc and millimetres).

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]
#![allow(
    clippy::manual_range_contains,
    reason = "The C++'s comparisons, which a range would turn true for NaN."
)]

use std::collections::BTreeSet;

use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};

use crate::data_model::{
    ControlComponent, ControlConstraint, ControlPoint, ObservationPrecision, SurveyProject,
    SurveyText,
};

// ---- Constants a person may override ---------------------------------------------

/// The coefficient of refraction. 0.13 is Gauss's value for daytime sights
/// over land, the one every total station defaults to and the one the owner's
/// own survey engine uses.
pub const DEFAULT_REFRACTION_COEFFICIENT: f64 = 0.13;

/// Metres. The mean radius of the earth, for curvature and refraction and for
/// the height reduction factor R / (R + h). Using a mean rather than the radius
/// of curvature at the site changes a 1 km sight's curvature term by well under
/// a millimetre, which is below what the other terms know.
pub const MEAN_EARTH_RADIUS: f64 = 6_371_000.0;

// ---- Basic: which corrections ----------------------------------------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AtmosphericCorrection {
    /// Never add one.
    None,
    /// Compute it from the setup's recorded temperature, pressure and humidity
    /// and apply it ONLY where the instrument states it did not apply one
    /// itself. Where the file does not say, nothing is applied and the report
    /// says so: applying it twice is as wrong as leaving it out, and a guess
    /// either way has to be visible.
    #[default]
    Auto,
    /// Take out whatever the instrument applied and apply the value computed
    /// from the recorded weather instead.
    Recompute,
    /// Apply ReductionSettings::fixed_ppm to every distance not already
    /// corrected by the instrument.
    Fixed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PrismConstantPolicy {
    /// Add the recorded constant only where the shot or setup says the
    /// instrument did not add it.
    #[default]
    Auto,
    /// Take out any constant the instrument applied and apply
    /// ReductionSettings::prism_constant to every prism distance instead.
    Override,
    /// Distances as recorded.
    None,
}

/// What face left and face right mean for the reduction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FaceHandling {
    /// The mean of each face-left / face-right pair of one target at one setup
    /// is the observation; a pointing on one face only is used as it is. The
    /// pair's spread is checked against FaceTolerances and reported.
    #[default]
    Average,
    /// Face-left only; face-right pointings are reported as unused, not dropped
    /// silently.
    FaceLeftOnly,
    /// Every pointing is an observation of its own (an adjustment then sees the
    /// two faces as two measurements).
    Separate,
}

/// The largest spread between the two faces of one pair that passes the check.
/// Horizontal: |FL - (FR - pi)|, wrapped. Zenith: |FL + FR - 2 pi|, which is
/// twice the index error. Distance: |FL - FR|. Defaults suit a 3" instrument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceTolerances {
    /// Radians: 10" = 10 * pi / 648000.
    pub horizontal: f64,
    /// Radians: 20".
    pub zenith: f64,
    /// Metres.
    pub distance: f64,
    /// Off: a pair outside a tolerance is used, flagged in the report and
    /// warned about. On: it is excluded, both faces, and the report says why.
    /// Off by default for the reason auto-reject is: throwing a measurement
    /// away is the person's decision.
    pub exclude_outside: bool,
}

impl Default for FaceTolerances {
    fn default() -> Self {
        Self {
            horizontal: 4.84813681109536e-05,
            zenith: 9.69627362219072e-05,
            distance: 0.005,
            exclude_outside: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeightReduction {
    #[default]
    None,
    /// Distances to the ellipsoid: factor R / (R + h) with h = H + N, the
    /// geoid separation N coming from ReductionContext::geoid_separation.
    Ellipsoid,
    /// Distances to the geoid ("sea level"): factor R / (R + H).
    Geoid,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GridScale {
    /// Ground distances.
    #[default]
    None,
    /// ReductionSettings::fixed_grid_scale_factor, everywhere.
    Fixed,
    /// ReductionContext::grid_scale_factor at each line's mid point.
    FromProjection,
}

// ---- Adjustment ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AdjustmentMethod {
    /// Radiation from the setups: every point computed from the setup it was
    /// shot from, nothing adjusted. Misclosures are still reported where the
    /// data closes.
    #[default]
    None,
    /// The setups form a traverse; see TraverseRule.
    Traverse,
    /// Least squares over every observation; see NetworkDimension.
    Network,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TraverseRule {
    /// Compass rule: corrections in proportion to leg length.
    #[default]
    Bowditch,
    /// In proportion to |latitude| and |departure|.
    Transit,
    /// The traverse as a network (network_adjustment::adjust_traverse).
    LeastSquares,
}

/// There is no 3D option: the survey layer adjusts horizontal networks and
/// level networks (network_adjustment.rs) and has no combined 3D model.
/// Horizontal AND levels runs the two, one after the other, which is what most
/// field software does anyway.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NetworkDimension {
    #[default]
    Horizontal,
    Levels,
    HorizontalAndLevels,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OutlierTest {
    None,
    /// Data snooping: w = v / sigma_v against the normal quantile.
    #[default]
    Baarda,
    /// Pope's tau test: the a-posteriori version, for an unknown variance factor.
    Tau,
}

/// Where the coordinates a control point is held at come from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ControlOrigin {
    /// The point's coordinates in the imported file.
    #[default]
    File,
    /// A point already on the drawing (ReductionContext::drawing_points).
    Drawing,
}

/// One control point as the person chose it: which components are held and
/// how (ControlPoint), and whose coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControlSelection {
    pub point: ControlPoint,
    pub origin: ControlOrigin,
}

// ---- The settings ----------------------------------------------------------------

/// Every member has the default a new import starts with. The defaults reduce a
/// total-station file the way its instrument would have, and adjust nothing:
/// adjusting is a decision that needs control, and control is the person's to
/// choose.
#[derive(Clone, Debug, PartialEq)]
pub struct ReductionSettings {
    // Basic
    pub atmospheric: AtmosphericCorrection,
    /// Parts per million; used by AtmosphericCorrection::Fixed.
    pub fixed_ppm: f64,
    pub prism_constant_policy: PrismConstantPolicy,
    /// Metres; used by PrismConstantPolicy::Override.
    pub prism_constant: f64,
    pub faces: FaceHandling,
    pub face_tolerances: FaceTolerances,
    pub curvature_and_refraction: bool,
    pub refraction_coefficient: f64,
    /// Metres.
    pub earth_radius: f64,
    pub slope_to_horizontal: bool,
    pub height_reduction: HeightReduction,
    pub grid_scale: GridScale,
    pub fixed_grid_scale_factor: f64,
    /// When on, `combined_factor` is applied to every horizontal distance IN
    /// PLACE OF the height and grid factors: the surveyor's "combined scale
    /// factor" off a control certificate.
    pub use_combined_factor: bool,
    pub combined_factor: f64,

    // Adjustment
    pub method: AdjustmentMethod,
    pub traverse_rule: TraverseRule,
    pub network_dimension: NetworkDimension,
    /// A-priori standard deviations. Total-station observations are always
    /// weighted from these; a GNSS value keeps the covariance its file states
    /// when `use_file_covariances` is on and falls back to these otherwise.
    pub apriori: ObservationPrecision,
    pub use_file_covariances: bool,
    pub control: Vec<ControlSelection>,
    /// Of the error ellipses and the global test (whose significance level is
    /// 1 - confidence_level).
    pub confidence_level: f64,
    pub outlier_test: OutlierTest,
    /// Per observation. 0.001 is Baarda's alpha0, critical value 3.29.
    pub outlier_significance: f64,
    /// Off by default: throwing an observation away is the person's decision.
    /// On, the worst flagged observation is rejected and the adjustment rerun,
    /// one at a time, and every rejection is listed in the report.
    pub auto_reject_outliers: bool,
    /// Gauss-Newton iterations.
    pub max_iterations: usize,
}

impl Default for ReductionSettings {
    fn default() -> Self {
        Self {
            atmospheric: AtmosphericCorrection::Auto,
            fixed_ppm: 0.0,
            prism_constant_policy: PrismConstantPolicy::Auto,
            prism_constant: 0.0,
            faces: FaceHandling::Average,
            face_tolerances: FaceTolerances::default(),
            curvature_and_refraction: true,
            refraction_coefficient: DEFAULT_REFRACTION_COEFFICIENT,
            earth_radius: MEAN_EARTH_RADIUS,
            slope_to_horizontal: true,
            height_reduction: HeightReduction::None,
            grid_scale: GridScale::None,
            fixed_grid_scale_factor: 1.0,
            use_combined_factor: false,
            combined_factor: 1.0,
            method: AdjustmentMethod::None,
            traverse_rule: TraverseRule::Bowditch,
            network_dimension: NetworkDimension::Horizontal,
            apriori: ObservationPrecision::default(),
            use_file_covariances: true,
            control: Vec::new(),
            confidence_level: 0.95,
            outlier_test: OutlierTest::Baarda,
            outlier_significance: 0.001,
            auto_reject_outliers: false,
            max_iterations: 25,
        }
    }
}

// ---- Stable text form ------------------------------------------------------------
//
// key=value lines, one setting per line, the first line naming the format and
// its version:
//
//     katana-reduction-settings=1
//     atmospheric=auto
//     refraction.k=0.13
//     control=CP1;file;fixed;0;fixed;0;free;0
//     ...
//
// Doubles are written as the shortest text that reads back as the same
// double, so a save and a load change nothing. `control` repeats, once per
// control point: the id (percent-encoded, since an id is user data and may
// hold ';'), the origin, then constraint;sigma for northing, easting and
// elevation. Lines starting '#' and blank lines are ignored.
//
// Versioning: REDUCTION_SETTINGS_VERSION is bumped when a key changes meaning.
// A new key does NOT need a bump: a key missing from older text keeps its
// default. Text from a NEWER version is refused, never half read.

pub const REDUCTION_SETTINGS_VERSION: i32 = 1;

const HEADER_KEY: &str = "katana-reduction-settings";
const CONTROL_KEY: &str = "control";
const MAX_ITERATIONS_KEY: &str = "iterations.max";

// ---- Enum names ------------------------------------------------------------------
//
// One list of values per enum, so that parsing is "the value whose name this
// is" and a value added to an enum and not to its list fails the round-trip
// test rather than being written and then refused.

const ATMOSPHERIC_VALUES: [AtmosphericCorrection; 4] = [
    AtmosphericCorrection::None,
    AtmosphericCorrection::Auto,
    AtmosphericCorrection::Recompute,
    AtmosphericCorrection::Fixed,
];
const PRISM_VALUES: [PrismConstantPolicy; 3] = [
    PrismConstantPolicy::Auto,
    PrismConstantPolicy::Override,
    PrismConstantPolicy::None,
];
const FACE_VALUES: [FaceHandling; 3] = [
    FaceHandling::Average,
    FaceHandling::FaceLeftOnly,
    FaceHandling::Separate,
];
const HEIGHT_VALUES: [HeightReduction; 3] = [
    HeightReduction::None,
    HeightReduction::Ellipsoid,
    HeightReduction::Geoid,
];
const GRID_VALUES: [GridScale; 3] = [GridScale::None, GridScale::Fixed, GridScale::FromProjection];
const METHOD_VALUES: [AdjustmentMethod; 3] = [
    AdjustmentMethod::None,
    AdjustmentMethod::Traverse,
    AdjustmentMethod::Network,
];
const TRAVERSE_VALUES: [TraverseRule; 3] = [
    TraverseRule::Bowditch,
    TraverseRule::Transit,
    TraverseRule::LeastSquares,
];
const NETWORK_VALUES: [NetworkDimension; 3] = [
    NetworkDimension::Horizontal,
    NetworkDimension::Levels,
    NetworkDimension::HorizontalAndLevels,
];
const OUTLIER_VALUES: [OutlierTest; 3] = [OutlierTest::None, OutlierTest::Baarda, OutlierTest::Tau];
const ORIGIN_VALUES: [ControlOrigin; 2] = [ControlOrigin::File, ControlOrigin::Drawing];
const CONSTRAINT_VALUES: [ControlConstraint; 3] = [
    ControlConstraint::Free,
    ControlConstraint::Fixed,
    ControlConstraint::Weighted,
];

fn constraint_name(constraint: ControlConstraint) -> &'static str {
    match constraint {
        ControlConstraint::Free => "free",
        ControlConstraint::Fixed => "fixed",
        ControlConstraint::Weighted => "weighted",
    }
}

fn from_name<E: Copy>(name: &str, values: &[E], namer: impl Fn(E) -> &'static str) -> Option<E> {
    values.iter().copied().find(|value| name == namer(*value))
}

fn named<E: Copy + SurveyText<Text = &'static str>>(name: &str, values: &[E]) -> Option<E> {
    from_name(name, values, |value| value.text())
}

fn assign_enum<E: Copy + SurveyText<Text = &'static str>>(
    target: &mut E,
    name: &str,
    values: &[E],
) -> bool {
    match named(name, values) {
        Some(value) => {
            *target = value;
            true
        }
        None => false,
    }
}

// ---- Numbers ---------------------------------------------------------------------

// Shortest text that reads back as the same double: the round trip is exact,
// and 0.13 is written "0.13", not "0.13000000000000000444".
fn format_double(value: f64) -> String {
    format_exact_real(value)
}

// std::from_chars of a double: the whole text, no leading '+' or white space,
// "inf", "infinity" and "nan" in any case, and a value outside the double's
// range (an overflow to infinity, an underflow to zero) refused. Rust's parse
// accepts a '+' and reads an out-of-range value as infinity or zero, so both
// are checked here.
fn parse_double(text: &str) -> Option<f64> {
    if text.is_empty() || text.starts_with('+') {
        return None;
    }
    let value: f64 = text.parse().ok()?;
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let literal = unsigned.eq_ignore_ascii_case("inf")
        || unsigned.eq_ignore_ascii_case("infinity")
        || unsigned.eq_ignore_ascii_case("nan");
    if value.is_infinite() && !literal {
        return None;
    }
    if value == 0.0 {
        let mantissa = unsigned.split(['e', 'E']).next().unwrap_or("");
        if mantissa.bytes().any(|byte| matches!(byte, b'1'..=b'9')) {
            return None;
        }
    }
    Some(value)
}

// std::from_chars of a size_t: digits only, no sign, and an overflow refused.
fn parse_size(text: &str) -> Option<usize> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn parse_bool(text: &str) -> Option<bool> {
    match text {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

// ---- Percent encoding for control ids --------------------------------------------

fn needs_escape(byte: u8) -> bool {
    byte < 0x20 || byte == 0x7F || byte == b'%' || byte == b';' || byte == b'='
}

fn percent_encode(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        // Every byte that needs an escape is ASCII, so a character is either
        // one such byte or copied whole.
        if character.is_ascii() && needs_escape(character as u8) {
            let byte = character as u8;
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0F)]));
        } else {
            out.push(character);
        }
    }
    out
}

// The C++ decodes into a byte string, which may then hold bytes that are not
// UTF-8 ("%FF"); an id the model holds is UTF-8 (docs/rust.md), so such an id
// is refused here as a malformed control line.
fn percent_decode(text: &str) -> Option<String> {
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        if i + 2 >= bytes.len() {
            return None; // a '%' needs two hex digits after it
        }
        let high = nibble(bytes[i + 1])?;
        let low = nibble(bytes[i + 2])?;
        out.push((high << 4) | low);
        i += 3;
    }
    String::from_utf8(out).ok()
}

// ---- The keys --------------------------------------------------------------------
//
// Tables rather than a hand-written writer and a hand-written reader, so that
// the two cannot disagree about a key's name: each key is spelled once.

struct DoubleKey {
    key: &'static str,
    field: fn(&mut ReductionSettings) -> &mut f64,
}

struct BoolKey {
    key: &'static str,
    field: fn(&mut ReductionSettings) -> &mut bool,
}

struct EnumKey {
    key: &'static str,
    get: fn(&ReductionSettings) -> &'static str,
    set: fn(&mut ReductionSettings, &str) -> bool,
}

macro_rules! enum_key {
    ($key:literal => $field:ident, $values:ident) => {
        EnumKey {
            key: $key,
            get: {
                fn get(settings: &ReductionSettings) -> &'static str {
                    settings.$field.text()
                }
                get
            },
            set: {
                fn set(settings: &mut ReductionSettings, name: &str) -> bool {
                    assign_enum(&mut settings.$field, name, &$values)
                }
                set
            },
        }
    };
}

macro_rules! double_key {
    ($key:literal => $($field:ident).+) => {
        DoubleKey {
            key: $key,
            field: {
                fn field(settings: &mut ReductionSettings) -> &mut f64 {
                    &mut settings.$($field).+
                }
                field
            },
        }
    };
}

macro_rules! bool_key {
    ($key:literal => $($field:ident).+) => {
        BoolKey {
            key: $key,
            field: {
                fn field(settings: &mut ReductionSettings) -> &mut bool {
                    &mut settings.$($field).+
                }
                field
            },
        }
    };
}

// The order of these tables is the order of the text, which is the order of
// the struct, so a diff of two saved settings reads like the dialog.
const ENUM_KEYS: [EnumKey; 9] = [
    enum_key!("atmospheric" => atmospheric, ATMOSPHERIC_VALUES),
    enum_key!("prism_constant.policy" => prism_constant_policy, PRISM_VALUES),
    enum_key!("faces" => faces, FACE_VALUES),
    enum_key!("height_reduction" => height_reduction, HEIGHT_VALUES),
    enum_key!("grid_scale" => grid_scale, GRID_VALUES),
    enum_key!("adjustment.method" => method, METHOD_VALUES),
    enum_key!("adjustment.traverse_rule" => traverse_rule, TRAVERSE_VALUES),
    enum_key!("adjustment.network" => network_dimension, NETWORK_VALUES),
    enum_key!("outliers.test" => outlier_test, OUTLIER_VALUES),
];

const DOUBLE_KEYS: [DoubleKey; 21] = [
    double_key!("atmospheric.fixed_ppm" => fixed_ppm),
    double_key!("prism_constant.m" => prism_constant),
    double_key!("faces.tolerance.horizontal_rad" => face_tolerances.horizontal),
    double_key!("faces.tolerance.zenith_rad" => face_tolerances.zenith),
    double_key!("faces.tolerance.distance_m" => face_tolerances.distance),
    double_key!("refraction.k" => refraction_coefficient),
    double_key!("earth_radius_m" => earth_radius),
    double_key!("grid_scale.fixed_factor" => fixed_grid_scale_factor),
    double_key!("combined_factor.value" => combined_factor),
    double_key!("apriori.direction_rad" => apriori.direction),
    double_key!("apriori.zenith_rad" => apriori.zenith),
    double_key!("apriori.distance_constant_m" => apriori.distance_constant),
    double_key!("apriori.distance_ppm" => apriori.distance_ppm),
    double_key!("apriori.instrument_centring_m" => apriori.instrument_centring),
    double_key!("apriori.target_centring_m" => apriori.target_centring),
    double_key!("apriori.height_m" => apriori.height_measurement),
    double_key!("apriori.levelling_per_sqrt_km_m" => apriori.levelling_per_sqrt_kilometre),
    double_key!("apriori.gnss_horizontal_m" => apriori.gnss_horizontal),
    double_key!("apriori.gnss_vertical_m" => apriori.gnss_vertical),
    double_key!("confidence_level" => confidence_level),
    double_key!("outliers.significance" => outlier_significance),
];

const BOOL_KEYS: [BoolKey; 6] = [
    bool_key!("curvature_refraction" => curvature_and_refraction),
    bool_key!("faces.tolerance.exclude" => face_tolerances.exclude_outside),
    bool_key!("slope_to_horizontal" => slope_to_horizontal),
    bool_key!("combined_factor.use" => use_combined_factor),
    bool_key!("apriori.use_file_covariances" => use_file_covariances),
    bool_key!("outliers.auto_reject" => auto_reject_outliers),
];

fn control_line(selection: &ControlSelection) -> String {
    let mut line = String::from(CONTROL_KEY);
    line.push('=');
    line.push_str(&percent_encode(&selection.point.point_id));
    line.push(';');
    line.push_str(selection.origin.text());
    for component in [
        &selection.point.northing,
        &selection.point.easting,
        &selection.point.elevation,
    ] {
        line.push(';');
        line.push_str(constraint_name(component.constraint));
        line.push(';');
        line.push_str(&format_double(component.sigma));
    }
    line
}

fn parse_control(value: &str) -> Option<ControlSelection> {
    let mut fields = Vec::new();
    let mut start = 0;
    loop {
        let cut = value[start..].find(';').map(|at| start + at);
        fields.push(match cut {
            Some(cut) => &value[start..cut],
            None => &value[start..],
        });
        let Some(cut) = cut else {
            break;
        };
        start = cut + 1;
        if fields.len() > 8 {
            return None;
        }
    }
    if fields.len() != 8 {
        return None;
    }
    let id = percent_decode(fields[0])?;
    let origin = named(fields[1], &ORIGIN_VALUES)?;
    let mut selection = ControlSelection {
        point: ControlPoint {
            point_id: id,
            ..Default::default()
        },
        origin,
    };
    let point = &mut selection.point;
    let components: [&mut ControlComponent; 3] = [
        &mut point.northing,
        &mut point.easting,
        &mut point.elevation,
    ];
    for (i, component) in components.into_iter().enumerate() {
        let constraint = from_name(fields[2 + 2 * i], &CONSTRAINT_VALUES, constraint_name)?;
        let sigma = parse_double(fields[3 + 2 * i])?;
        component.constraint = constraint;
        component.sigma = sigma;
    }
    Some(selection)
}

fn invalid_setting(message: impl Into<String>) -> Error {
    make_error(ErrorCode::InvalidArgument, message, "reduction settings")
}

fn parse_error(line: usize, message: impl AsRef<str>) -> Error {
    Error::new(
        ErrorCode::ParseFailure,
        format!(
            "line {line} of the reduction settings: {}",
            message.as_ref()
        ),
    )
}

// ---- Names -----------------------------------------------------------------------
//
// The names the text form uses, also what the report prints: the C++ toString
// overloads, through the crate's one SurveyText.

impl SurveyText for AtmosphericCorrection {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            AtmosphericCorrection::None => "none",
            AtmosphericCorrection::Auto => "auto",
            AtmosphericCorrection::Recompute => "recompute",
            AtmosphericCorrection::Fixed => "fixed",
        }
    }
}

impl SurveyText for PrismConstantPolicy {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            PrismConstantPolicy::Auto => "auto",
            PrismConstantPolicy::Override => "override",
            PrismConstantPolicy::None => "none",
        }
    }
}

impl SurveyText for FaceHandling {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            FaceHandling::Average => "average",
            FaceHandling::FaceLeftOnly => "face-left-only",
            FaceHandling::Separate => "separate",
        }
    }
}

impl SurveyText for HeightReduction {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            HeightReduction::None => "none",
            HeightReduction::Ellipsoid => "ellipsoid",
            HeightReduction::Geoid => "geoid",
        }
    }
}

impl SurveyText for GridScale {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            GridScale::None => "none",
            GridScale::Fixed => "fixed",
            GridScale::FromProjection => "projection",
        }
    }
}

impl SurveyText for AdjustmentMethod {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            AdjustmentMethod::None => "none",
            AdjustmentMethod::Traverse => "traverse",
            AdjustmentMethod::Network => "network",
        }
    }
}

impl SurveyText for TraverseRule {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            TraverseRule::Bowditch => "bowditch",
            TraverseRule::Transit => "transit",
            TraverseRule::LeastSquares => "least-squares",
        }
    }
}

impl SurveyText for NetworkDimension {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            NetworkDimension::Horizontal => "horizontal",
            NetworkDimension::Levels => "levels",
            NetworkDimension::HorizontalAndLevels => "horizontal-and-levels",
        }
    }
}

impl SurveyText for OutlierTest {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            OutlierTest::None => "none",
            OutlierTest::Baarda => "baarda",
            OutlierTest::Tau => "tau",
        }
    }
}

impl SurveyText for ControlOrigin {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            ControlOrigin::File => "file",
            ControlOrigin::Drawing => "drawing",
        }
    }
}

// ---- Validation ------------------------------------------------------------------

/// Checks the settings on their own: finite numbers, positive factors, radii,
/// sigmas and tolerances; a refraction coefficient in [-1, 1]; a confidence
/// level and outlier significance in (0, 1); at least one iteration; control
/// point ids non-empty and unique, and every Weighted component with a
/// sigma > 0. InvalidArgument naming the setting, in words a surveyor uses.
pub fn validate_reduction_settings(settings: &ReductionSettings) -> Status {
    let finite = |value: f64| value.is_finite();
    let positive = |value: f64| value.is_finite() && value > 0.0;

    if !finite(settings.fixed_ppm) {
        return Err(invalid_setting(
            "the fixed atmospheric correction is not a number",
        ));
    }
    if !finite(settings.prism_constant) {
        return Err(invalid_setting("the prism constant is not a number"));
    }
    if !positive(settings.face_tolerances.horizontal)
        || !positive(settings.face_tolerances.zenith)
        || !positive(settings.face_tolerances.distance)
    {
        return Err(invalid_setting(
            "the face left / face right tolerances must be positive",
        ));
    }
    if !finite(settings.refraction_coefficient)
        || settings.refraction_coefficient < -1.0
        || settings.refraction_coefficient > 1.0
    {
        return Err(invalid_setting(
            "the coefficient of refraction must lie between -1 and 1",
        ));
    }
    if !positive(settings.earth_radius) {
        return Err(invalid_setting("the earth radius must be positive"));
    }
    if !positive(settings.fixed_grid_scale_factor) {
        return Err(invalid_setting("the grid scale factor must be positive"));
    }
    if !positive(settings.combined_factor) {
        return Err(invalid_setting(
            "the combined scale factor must be positive",
        ));
    }
    let apriori = &settings.apriori;
    for (name, value) in [
        ("direction", apriori.direction),
        ("zenith angle", apriori.zenith),
        ("distance (constant part)", apriori.distance_constant),
        ("levelling", apriori.levelling_per_sqrt_kilometre),
        ("GNSS horizontal", apriori.gnss_horizontal),
        ("GNSS vertical", apriori.gnss_vertical),
    ] {
        if !positive(value) {
            return Err(invalid_setting(format!(
                "the a-priori standard deviation of a {name} must be positive"
            )));
        }
    }
    for (name, value) in [
        ("distance (ppm part)", apriori.distance_ppm),
        ("instrument centring", apriori.instrument_centring),
        ("target centring", apriori.target_centring),
        ("instrument and target heights", apriori.height_measurement),
    ] {
        if !finite(value) || value < 0.0 {
            return Err(invalid_setting(format!(
                "the a-priori standard deviation of the {name} must not be negative"
            )));
        }
    }
    if !finite(settings.confidence_level)
        || settings.confidence_level <= 0.0
        || settings.confidence_level >= 1.0
    {
        return Err(invalid_setting(
            "the confidence level must lie strictly between 0 and 1",
        ));
    }
    if !finite(settings.outlier_significance)
        || settings.outlier_significance <= 0.0
        || settings.outlier_significance >= 1.0
    {
        return Err(invalid_setting(
            "the outlier test significance must lie strictly between 0 and 1",
        ));
    }
    if settings.max_iterations == 0 {
        return Err(invalid_setting(
            "the adjustment needs at least one iteration",
        ));
    }
    let mut seen = BTreeSet::new();
    for selection in &settings.control {
        let point = &selection.point;
        if point.point_id.is_empty() {
            return Err(invalid_setting("a control point has no point id"));
        }
        if !seen.insert(point.point_id.as_str()) {
            return Err(invalid_setting(format!(
                "control point '{}' is listed twice",
                point.point_id
            )));
        }
        for (name, component) in [
            ("northing", &point.northing),
            ("easting", &point.easting),
            ("elevation", &point.elevation),
        ] {
            if !finite(component.sigma) {
                return Err(invalid_setting(format!(
                    "control point '{}': the {name} standard deviation is not a number",
                    point.point_id
                )));
            }
            if component.constraint == ControlConstraint::Weighted && !(component.sigma > 0.0) {
                return Err(invalid_setting(format!(
                    "control point '{}': a weighted {name} needs a positive standard deviation",
                    point.point_id
                )));
            }
        }
    }
    Ok(())
}

/// The control a file declares, as a starting selection: every
/// SurveyProject::control_points entry held as the file holds it, with its
/// coordinates from the file.
pub fn control_from_file(project: &SurveyProject) -> Vec<ControlSelection> {
    project
        .control_points
        .iter()
        .map(|point| ControlSelection {
            point: point.clone(),
            origin: ControlOrigin::File,
        })
        .collect()
}

// ---- Text form -------------------------------------------------------------------

pub fn serialise_reduction_settings(settings: &ReductionSettings) -> String {
    // The accessors in the tables hand out references into a settings value;
    // writing reads through a copy rather than through a second set of them.
    let mut copy = settings.clone();
    let mut text = String::with_capacity(2048);
    let mut line = |key: &str, value: &str| {
        text.push_str(key);
        text.push('=');
        text.push_str(value);
        text.push('\n');
    };
    line(HEADER_KEY, &format!("{REDUCTION_SETTINGS_VERSION}"));
    for key in &ENUM_KEYS {
        line(key.key, (key.get)(&copy));
    }
    for key in &DOUBLE_KEYS {
        let value = *(key.field)(&mut copy);
        line(key.key, &format_double(value));
    }
    for key in &BOOL_KEYS {
        let value = *(key.field)(&mut copy);
        line(key.key, if value { "true" } else { "false" });
    }
    line(MAX_ITERATIONS_KEY, &format!("{}", copy.max_iterations));
    for selection in &copy.control {
        text.push_str(&control_line(selection));
        text.push('\n');
    }
    text
}

/// ParseFailure naming the line for a malformed line or value; Unsupported for
/// text written by a newer version; InvalidArgument when the settings read fail
/// validate_reduction_settings. An unknown key is not an error (a later build
/// may have added it): it is skipped and, when `warnings` is given, reported
/// there.
pub fn parse_reduction_settings(
    text: &str,
    mut warnings: Option<&mut Vec<String>>,
) -> Result<ReductionSettings> {
    let mut settings = ReductionSettings::default();
    settings.control.clear();
    let mut saw_header = false;
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (index, raw_line) in text.split('\n').enumerate() {
        let line_number = index + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(parse_error(
                line_number,
                format!("expected key=value, found '{line}'"),
            ));
        };

        if !saw_header {
            if key != HEADER_KEY {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "this text is not a set of Katana reduction settings",
                    format!("it must begin with {HEADER_KEY}=<version>"),
                ));
            }
            let version = parse_size(value);
            let Some(version) = version.filter(|version| *version != 0) else {
                return Err(parse_error(
                    line_number,
                    format!("the settings version '{value}' is not a positive whole number"),
                ));
            };
            if version > REDUCTION_SETTINGS_VERSION as usize {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    format!(
                        "these reduction settings were written by a newer Katana (settings \
                         version {version}); this one reads up to version \
                         {REDUCTION_SETTINGS_VERSION}"
                    ),
                ));
            }
            saw_header = true;
            continue;
        }

        if key == CONTROL_KEY {
            let Some(selection) = parse_control(value) else {
                return Err(parse_error(
                    line_number,
                    "a control line needs id;origin;then constraint;sigma for northing, \
                     easting and elevation",
                ));
            };
            settings.control.push(selection);
            continue;
        }
        if !seen.insert(key) {
            return Err(parse_error(line_number, format!("'{key}' is given twice")));
        }

        let mut known = false;
        for entry in &ENUM_KEYS {
            if entry.key == key {
                known = true;
                if !(entry.set)(&mut settings, value) {
                    return Err(parse_error(
                        line_number,
                        format!("'{value}' is not a choice for {key}"),
                    ));
                }
            }
        }
        for entry in &DOUBLE_KEYS {
            if entry.key == key {
                known = true;
                let Some(number) = parse_double(value) else {
                    return Err(parse_error(
                        line_number,
                        format!("'{value}' is not a number ({key})"),
                    ));
                };
                *(entry.field)(&mut settings) = number;
            }
        }
        for entry in &BOOL_KEYS {
            if entry.key == key {
                known = true;
                let Some(flag) = parse_bool(value) else {
                    return Err(parse_error(
                        line_number,
                        format!("{key} must be true or false"),
                    ));
                };
                *(entry.field)(&mut settings) = flag;
            }
        }
        if key == MAX_ITERATIONS_KEY {
            known = true;
            let Some(count) = parse_size(value) else {
                return Err(parse_error(
                    line_number,
                    "the iteration limit must be a whole number",
                ));
            };
            settings.max_iterations = count;
        }
        if !known && let Some(warnings) = warnings.as_deref_mut() {
            warnings.push(format!(
                "line {line_number}: setting '{key}' is not known to this version and was \
                 ignored"
            ));
        }
    }
    if !saw_header {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "the reduction settings text is empty",
        ));
    }
    validate_reduction_settings(&settings)?;
    Ok(settings)
}
