//! What a reduction and adjustment did to the data, as structured values.
//!
//! The report is the answer to "what happened to my data?": every raw value,
//! every correction applied to it and how big it was, what came out, what was
//! thrown away and why, and how well it all fitted. It is data first and text
//! second, so that the same report renders as plain text (the job's log), as
//! HTML (the wizard's report page, a saved report) and is testable field by
//! field without parsing either.
//!
//! Units are the model's: metres and radians. The renderers convert to
//! millimetres and degrees-minutes-seconds for people.

use crate::data_model::{InstrumentSettings, Pointing, SourceRecord, SurveyText};
use crate::error_propagation::ErrorEllipse;
use crate::reduction_settings::ReductionSettings;

pub use crate::reduction_report_render::{render_html, render_text};

/// A sentence for a person, and the record it is about when there is one.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReportMessage {
    pub text: String,
    pub source: SourceRecord,
}

// ---- Input -----------------------------------------------------------------------

/// What was read. Filled by the CALLER (the survey layer cannot see the format
/// registry) and copied into the report unchanged: see ReductionContext::input.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReportInput {
    pub file_name: String,
    /// Other files the reader used, by name.
    pub sibling_files: Vec<String>,
    pub format_id: String,
    /// "Leica GSI-16"
    pub format_name: String,
    pub parser_version: String,
    pub records_read: usize,
    pub records_skipped: usize,
    /// The parser's.
    pub warnings: Vec<ReportMessage>,
    /// What the format did not carry.
    pub not_carried: Vec<String>,
}

// ---- Setups ----------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportSetup {
    pub station_id: String,
    pub point_id: String,
    pub instrument_height: f64,
    pub backsight_point_id: String,
    /// Grid azimuth to the backsight from the known coordinates.
    pub backsight_azimuth: Option<f64>,
    /// The mean circle reading on the backsight.
    pub backsight_reading: Option<f64>,
    /// Added to every circle reading of this setup to give a grid azimuth.
    pub orientation_correction: Option<f64>,
    /// Backsight check: measured minus computed, where both exist. Metres,
    /// horizontal.
    pub backsight_distance_difference: Option<f64>,
    /// Metres.
    pub backsight_height_difference: Option<f64>,
    pub instrument: InstrumentSettings,
    pub source: SourceRecord,
}

// ---- Observations ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CorrectionKind {
    PrismConstant,
    Atmospheric,
    /// The mean of a face-left / face-right pair.
    FaceMean,
    SlopeToHorizontal,
    CurvatureRefraction,
    /// To the ellipsoid or the geoid.
    HeightReduction,
    GridScale,
    CombinedFactor,
    /// Circle reading to grid azimuth.
    Orientation,
    InstrumentAndTargetHeight,
    #[default]
    Other,
}

impl SurveyText for CorrectionKind {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            CorrectionKind::PrismConstant => "prism constant",
            CorrectionKind::Atmospheric => "atmospheric",
            CorrectionKind::FaceMean => "face left / face right mean",
            CorrectionKind::SlopeToHorizontal => "slope to horizontal",
            CorrectionKind::CurvatureRefraction => "curvature and refraction",
            CorrectionKind::HeightReduction => "height reduction",
            CorrectionKind::GridScale => "grid scale factor",
            CorrectionKind::CombinedFactor => "combined scale factor",
            CorrectionKind::Orientation => "orientation",
            CorrectionKind::InstrumentAndTargetHeight => "instrument and target heights",
            CorrectionKind::Other => "other",
        }
    }
}

/// One correction applied to one value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppliedCorrection {
    pub kind: CorrectionKind,
    /// The change it made, in the unit of the value (metres or radians):
    /// value after = value before + amount.
    pub amount: f64,
    /// For a proportional correction, the factor applied (1 + ppm * 1e-6, a
    /// scale factor); absent for an additive one.
    pub factor: Option<f64>,
    /// Where the number came from, in words: "k = 0.13", "+12.4 ppm from
    /// 21.0 C and 1013.2 hPa", "instrument already applied -34.4 mm".
    pub note: String,
}

/// One raw value and what became of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportObservation {
    /// Empty for an observation belonging to no setup.
    pub station_id: String,
    /// observation_kind_name() of the raw observation.
    pub kind: String,
    pub from: String,
    pub to: String,
    pub pointing: Pointing,
    /// raw / reduced are radians when true, metres otherwise.
    pub angular: bool,
    pub raw: f64,
    /// In the order applied.
    pub corrections: Vec<AppliedCorrection>,
    /// Absent when rejected before reduction.
    pub reduced: Option<f64>,
    pub rejected: bool,
    /// "outside the face tolerance", "flagged by Baarda, w = 4.1"
    pub rejection_reason: String,
    pub source: SourceRecord,
}

/// One face-left / face-right pair and its check.
#[derive(Clone, Debug, PartialEq)]
pub struct FacePairCheck {
    pub station_id: String,
    pub target_id: String,
    pub left_pointing: usize,
    pub right_pointing: usize,
    /// Spreads as FaceTolerances defines them; absent for a quantity the pair
    /// did not observe on both faces.
    pub horizontal_spread: Option<f64>,
    pub zenith_spread: Option<f64>,
    pub distance_spread: Option<f64>,
    pub within_tolerance: bool,
    pub left: SourceRecord,
    pub right: SourceRecord,
}

impl Default for FacePairCheck {
    fn default() -> Self {
        Self {
            station_id: String::new(),
            target_id: String::new(),
            left_pointing: 0,
            right_pointing: 0,
            horizontal_spread: None,
            zenith_spread: None,
            distance_spread: None,
            within_tolerance: true,
            left: SourceRecord::default(),
            right: SourceRecord::default(),
        }
    }
}

// ---- Misclosures -----------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MisclosureReport {
    /// Traverse or loop name.
    pub name: String,
    /// Radians.
    pub angular: Option<f64>,
    pub angular_allowable: Option<f64>,
    /// Metres, computed - known.
    pub northing: Option<f64>,
    pub easting: Option<f64>,
    /// hypot(northing, easting)
    pub linear: Option<f64>,
    /// Metres, total traverse length.
    pub length: f64,
    /// N of "1 : N" = length / linear.
    pub precision_ratio: Option<f64>,
    /// Metres, levelling or trig height misclosure.
    pub height: Option<f64>,
    /// Absent when no tolerance applies.
    pub within_tolerance: Option<bool>,
}

// ---- Adjustment ------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportResidual {
    /// "slope distance 101 -> 102"
    pub observation: String,
    pub source: SourceRecord,
    pub angular: bool,
    /// v = adjusted - observed
    pub residual: f64,
    /// A-priori.
    pub sigma: f64,
    pub redundancy_number: f64,
    /// w (Baarda) or tau.
    pub standardised: Option<f64>,
    pub flagged: bool,
    pub rejected: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReportGlobalTest {
    pub statistic: f64,
    pub lower_critical: f64,
    pub upper_critical: f64,
    pub significance_level: f64,
    pub passed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportEllipse {
    pub point_id: String,
    /// One sigma.
    pub standard: ErrorEllipse,
    /// Multiply `standard` by this for the confidence level.
    pub confidence_scale: f64,
}

/// One adjustment run (horizontal and levels are two).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdjustmentReport {
    /// "network least squares (horizontal)", "traverse, Bowditch"
    pub method: String,
    pub unknowns: usize,
    pub observations: usize,
    pub redundancy: usize,
    /// A posteriori; absent at redundancy 0.
    pub variance_factor: Option<f64>,
    pub global_test: Option<ReportGlobalTest>,
    pub iterations: usize,
    pub residuals: Vec<ReportResidual>,
    /// Observation labels.
    pub flagged_outliers: Vec<String>,
    /// Auto-rejected, in rejection order.
    pub rejected_outliers: Vec<String>,
    pub ellipses: Vec<ReportEllipse>,
}

// ---- Resections ------------------------------------------------------------------

/// A resection whose geometry magnifies the errors of its observations more
/// than this many times is flagged as weak (ResectionReport::dilution): its
/// station is placed, with the precision it has, and the report, SURVEY IMPORT
/// and a warning say so. Three is the reduction's allowable multiplier - a
/// traverse's angular misclosure is allowed three standard deviations - here
/// against the least precise single observation the station was fixed from,
/// so only a geometry that clearly dominates the instrument is flagged: two
/// marks 30 degrees apart with distances give 2.7, three directions 60 degrees
/// apart from outside their triangle 2.5, a station 20 m inside a 100 m danger
/// circle 10.4, three marks within 11 degrees 123 (worked by a separate
/// script, docs/survey.md). A geometry that does not fix the station at all is
/// refused, not flagged.
pub const WEAK_RESECTION_DILUTION: f64 = 3.0;

/// A setup whose station nothing else positioned, positioned by resection (a
/// free station): the least squares of its reduced directions and distances to
/// points already placed, those points held, and of its trigonometric height
/// differences to the ones with heights. Each least squares is reported as an
/// adjustment is (a residual per direction, distance and height difference,
/// with its redundancy number and the outlier test the settings choose) and
/// the coordinates it gave, with their precision.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResectionReport {
    /// The setup.
    pub station_id: String,
    /// The point it stands on, which the resection positioned.
    pub point_id: String,
    /// The placed points it was computed from, in the order the setup observed them.
    pub targets: Vec<String>,
    pub northing: f64,
    pub easting: f64,
    /// Absent where no target with a height was observed with both a zenith
    /// angle and a distance, which a trigonometric height needs.
    pub elevation: Option<f64>,
    /// One sigma, of the point the setup stands on: the least squares'
    /// a-priori precision (the cofactor at the settings' weights), scaled by
    /// its variance factor only where its global test finds the residuals
    /// larger than those weights allow (on the few degrees of freedom of a
    /// resection, a variance factor the test accepts says no more than the
    /// weights) with, where the setup records an instrument height (a mark
    /// under the instrument), the a-priori centring over the mark and the
    /// measured instrument height added, which are common to every pointing and
    /// so are not in the least squares' weights.
    pub sigma_northing: f64,
    pub sigma_easting: f64,
    pub sigma_elevation: Option<f64>,
    /// Added to a circle reading to give a grid azimuth: the solution's, and
    /// its one sigma, taken as the coordinates' is.
    pub orientation: f64,
    pub sigma_orientation: f64,
    pub horizontal: AdjustmentReport,
    pub height: Option<AdjustmentReport>,
    /// The geometry's own strength, whatever the residuals say: the station's
    /// standard ellipse at the a-priori weights (sigma0 = 1), and its
    /// semi-major axis over the standard deviation of the least precise single
    /// observation's line of position (for a direction, its sight length times
    /// its standard deviation): how many times the geometry magnifies the
    /// observations' errors. Weak above WEAK_RESECTION_DILUTION.
    pub apriori_ellipse: ReportEllipse,
    pub dilution: f64,
    pub weak_geometry: bool,
    /// Pointings after the block the file says the field software resected
    /// from (RESECTION_END_METADATA) to the points the resection used: checks
    /// of the station, each a row of ReductionReport::misclosures.
    pub checks: usize,
    /// Where the file gives coordinates of its own for the station (a
    /// controller's, or keyed in), which the resection replaced: the resection
    /// less them. Also a misclosure row and a warning.
    pub file_northing_difference: Option<f64>,
    pub file_easting_difference: Option<f64>,
    /// Where another setup had radiated the station before its own block
    /// resected it (a setup that names no backsight, on a mark an earlier
    /// setup shot): that setup, and the resection less the radiation, which is
    /// a check of it. Also a misclosure row and a warning.
    pub radiated_from: String,
    pub radiated_northing_difference: Option<f64>,
    pub radiated_easting_difference: Option<f64>,
    /// The drawing projection's point scale factor at the station, where the
    /// settings reduce no distance to grid (no grid scale, no combined factor)
    /// and that factor would change its longest distance by more than the
    /// distance's standard deviation: ground distances were fitted to grid
    /// coordinates.
    pub unapplied_scale_factor: Option<f64>,
    pub source: SourceRecord,
}

// ---- Coordinates -----------------------------------------------------------------

/// How a coordinate came out of the reduction. The default is Radiation, what
/// a computed point starts as (ComputedPoint, CoordinateReport).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ComputationMethod {
    /// Held: it went in as control.
    Control,
    /// Computed from one setup, not adjusted.
    #[default]
    Radiation,
    TraverseBowditch,
    TraverseTransit,
    TraverseLeastSquares,
    NetworkLeastSquares,
    /// A GNSS position converted to grid.
    Gnss,
    /// A setup's station, from its pointings to placed points.
    Resection,
}

impl SurveyText for ComputationMethod {
    type Text = &'static str;
    fn text(self) -> &'static str {
        match self {
            ComputationMethod::Control => "control",
            ComputationMethod::Radiation => "radiation",
            ComputationMethod::TraverseBowditch => "traverse (Bowditch)",
            ComputationMethod::TraverseTransit => "traverse (Transit)",
            ComputationMethod::TraverseLeastSquares => "traverse (least squares)",
            ComputationMethod::NetworkLeastSquares => "network least squares",
            ComputationMethod::Gnss => "GNSS",
            ComputationMethod::Resection => "resection",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoordinateReport {
    pub point_id: String,
    pub northing: f64,
    pub easting: f64,
    pub elevation: Option<f64>,
    pub sigma_northing: Option<f64>,
    pub sigma_easting: Option<f64>,
    pub sigma_elevation: Option<f64>,
    pub method: ComputationMethod,
    /// Against the previous run of the same job (ReductionContext::previous);
    /// absent on the first run and for a point the previous run did not have.
    pub shift_northing: Option<f64>,
    pub shift_easting: Option<f64>,
    pub shift_elevation: Option<f64>,
}

// ---- The report ------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReductionReport {
    /// ISO 8601, supplied by the caller.
    pub created_utc: String,
    pub input: ReportInput,
    /// Exactly what was used.
    pub settings: ReductionSettings,
    pub setups: Vec<ReportSetup>,
    pub observations: Vec<ReportObservation>,
    pub face_pairs: Vec<FacePairCheck>,
    pub misclosures: Vec<MisclosureReport>,
    /// In the order they were computed.
    pub resections: Vec<ResectionReport>,
    pub adjustments: Vec<AdjustmentReport>,
    pub coordinates: Vec<CoordinateReport>,
    /// The reduction's own.
    pub warnings: Vec<ReportMessage>,
}
