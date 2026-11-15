//! Reduction and adjustment of raw field observations: the step between what a
//! parser read (surveyio) and the coordinates that go on the drawing (cad).
//!
//!   raw SurveyProject + ReductionSettings + ReductionContext
//!       -> reduce_and_adjust
//!       -> ReductionOutcome { reduced project, computed points, report }
//!
//! The pipeline, in the order the corrections are applied to one pointing:
//!
//! ```text
//!   1. prism constant                (PrismConstantPolicy)
//!   2. atmospheric ppm               (AtmosphericCorrection: Auto only where the
//!                                     instrument did not apply it)
//!   3. face left / face right mean   (FaceHandling, with FaceTolerances checked)
//!   4. slope to horizontal, with curvature and refraction (k, R)
//!   5. height reduction              (to the ellipsoid or the geoid)
//!   6. grid scale or combined factor
//!   7. orientation of circle readings to grid azimuths from the backsight
//!   8. coordinates: radiation, a traverse, or a network adjustment
//!      (the existing adjust_horizontal_network / adjust_level_network /
//!      compute_traverse / adjust_traverse, not re-implemented)
//! ```
//!
//! Every step writes what it did into the report.
//!
//! The survey layer may not see geodesy (tools/check_layering.cmake), so
//! everything that needs a projection or a geoid comes in through
//! ReductionContext as functions, which cad builds from katana_geodesy.
//!
//! Pure: no state, the inputs are not modified, identical inputs give bitwise
//! identical outputs (the report's time stamp is supplied, not read).
//!
//! How it works. Phase A, per setup and independent of any coordinate: group
//! the observations of each pointing, correct each distance (prism constant,
//! then atmospheric ppm), pair face left with face right and mean them, reduce
//! slope to horizontal with curvature and refraction, and form the height
//! difference mark to mark. Phase B, setup by setup as their stations become
//! known: orient the circle on the backsight, apply the factors that need a
//! position (height reduction, grid scale at the line's mid-point, or the
//! combined factor), and radiate the targets. A target already positioned
//! becomes a check instead. Then the adjustment the settings ask for
//! (reduction_adjust.rs), and the outputs: the computed points, the reduced
//! project and the report.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt;

use katana_core::error::make_error;
use katana_core::text::{parse_integer, trimmed};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::cpp;
use katana_math::numerics::{
    HALF_PI, PI, TWO_PI, normalize_angle, normalize_angle_signed, tolerance,
};

use crate::data_model::{
    AntennaHeightMethod, AzimuthObservation, ControlComponent, ControlConstraint, CoordinateSource,
    CorrectionState, DistanceKind, DistanceObservation, Face, GeocentricCoordinate,
    GeodeticCoordinate, GnssSolution, HorizontalDirectionObservation, Observation, Pointing,
    RESECTION_END_METADATA, SourceRecord, SurveyPoint, SurveyProject, SurveyStation, SurveyText,
    TargetInfo, VerticalAngleObservation, ZenithAngleObservation, distance_sigma,
    observation_kind_name, observation_source,
};
use crate::reduction_adjust::{
    adjust_as_network, adjust_as_traverse, in_resection_block, resect_setup, resection_shortfall,
};
use crate::reduction_engine::{
    Engine, GlobalGridPosition, Position, PositionOrigin, RecordedAngle, ReducedPointing,
    SetupState,
};
use crate::reduction_formulas::{
    ASSUMED_HUMIDITY_PERCENT, atmospheric_ppm, geodetic_from_geocentric, height_reduction_factor,
    local_variances, reduce_slope,
};
use crate::reduction_gnss::{
    antenna_words, convert_gnss_vectors, grid_of_geocentric, grid_of_geodetic,
    radiate_gnss_vectors, reducible_antenna,
};
use crate::reduction_report::{
    AppliedCorrection, ComputationMethod, CoordinateReport, CorrectionKind, FacePairCheck,
    MisclosureReport, ReductionReport, ReportInput, ReportObservation, ReportSetup,
};
use crate::reduction_settings::{
    AdjustmentMethod, AtmosphericCorrection, ControlOrigin, FaceHandling, GridScale,
    HeightReduction, PrismConstantPolicy, ReductionSettings, validate_reduction_settings,
};

/// A position in the drawing's grid system.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GridPosition {
    pub northing: f64,
    pub easting: f64,
    /// Metres above the ellipsoid.
    pub ellipsoidal_height: f64,
}

/// One coordinate the reduction produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComputedPoint {
    pub id: String,
    pub northing: f64,
    pub easting: f64,
    pub elevation: Option<f64>,
    /// Absent where nothing gave the point a precision (radiation without an
    /// adjustment); zero for a held component of control.
    pub sigma_northing: Option<f64>,
    pub sigma_easting: Option<f64>,
    pub sigma_elevation: Option<f64>,
    pub method: ComputationMethod,
}

/// The point scale factor of the drawing's projection at a grid position
/// (northing, easting, ellipsoidal height).
pub type GridScaleFactorFn = Box<dyn Fn(f64, f64, f64) -> Option<f64>>;
/// Geoid separation N (ellipsoidal height = orthometric height + N), metres,
/// at (northing, easting).
pub type GeoidSeparationFn = Box<dyn Fn(f64, f64) -> Option<f64>>;
/// A GNSS global value into the drawing's grid.
pub type GeocentricToGridFn = Box<dyn Fn(&GeocentricCoordinate) -> Option<GridPosition>>;
/// A GNSS global value into the drawing's grid.
pub type GeodeticToGridFn = Box<dyn Fn(&GeodeticCoordinate) -> Option<GridPosition>>;

/// What the caller supplies besides the data and the settings.
///
/// Every function may be absent. An absent function is a statement that the
/// drawing cannot answer the question, and the reduction then refuses the
/// setting that needs it (GridScale::FromProjection with no grid_scale_factor
/// is an InvalidArgument naming the setting) rather than quietly using 1.0.
#[derive(Default)]
pub struct ReductionContext {
    /// Points already on the drawing, by id. The reduction holds the ones the
    /// settings name as ControlOrigin::Drawing; the rest are ignored.
    pub drawing_points: Vec<SurveyPoint>,
    pub grid_scale_factor: Option<GridScaleFactorFn>,
    pub geoid_separation: Option<GeoidSeparationFn>,
    pub geocentric_to_grid: Option<GeocentricToGridFn>,
    pub geodetic_to_grid: Option<GeodeticToGridFn>,
    /// Copied into ReductionReport::input unchanged.
    pub input: ReportInput,
    /// The coordinates of the previous run of the same job, for the report's
    /// shifts when re-adjusting. Empty on a first import.
    pub previous: Vec<ComputedPoint>,
    /// Stamped on the report: the caller's clock, so a test is deterministic.
    pub created_utc: String,
}

// A function is neither Debug nor comparable; the context says only which it has.
impl fmt::Debug for ReductionContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReductionContext")
            .field("drawing_points", &self.drawing_points)
            .field("grid_scale_factor", &self.grid_scale_factor.is_some())
            .field("geoid_separation", &self.geoid_separation.is_some())
            .field("geocentric_to_grid", &self.geocentric_to_grid.is_some())
            .field("geodetic_to_grid", &self.geodetic_to_grid.is_some())
            .field("input", &self.input)
            .field("previous", &self.previous)
            .field("created_utc", &self.created_utc)
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReductionOutcome {
    /// The raw project with its observations reduced (horizontal distances,
    /// oriented directions, face means) and its computed points added as
    /// SurveyPoints with CoordinateSource::Calculated. What cad draws.
    pub reduced: SurveyProject,
    pub points: Vec<ComputedPoint>,
    pub report: ReductionReport,
}

// ---- Text helpers shared with the adjustments -------------------------------------

/// printf's "%.*f": Rust's fixed digits for a finite value (both are exact, ties
/// to even), and the C library's words for the others.
pub(crate) fn format_number(value: f64, decimals: usize) -> String {
    if value.is_nan() {
        return String::from(if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        });
    }
    if value.is_infinite() {
        return String::from(if value < 0.0 { "-inf" } else { "inf" });
    }
    format!("{value:.decimals$}")
}

pub(crate) fn format_seconds(radians: f64, decimals: usize) -> String {
    format_number(radians * 648000.0 / PI, decimals) + "\""
}

pub(crate) fn format_millimetres(metres: f64, decimals: usize) -> String {
    format_number(metres * 1000.0, decimals) + " mm"
}

const NO_ROW: usize = usize::MAX;

// ---- What the instrument did to its distances ---------------------------------------

// The atmospheric correction one setup's distances get: a factor, and why.
#[derive(Default)]
struct AtmosphereDecision {
    apply: bool,
    // net ppm applied (after taking out the instrument's)
    ppm: f64,
    // short: it is copied onto every distance of the setup
    note: String,
}

fn ppm_text(ppm: f64) -> String {
    format!(
        "{}{} ppm",
        if ppm >= 0.0 { "+" } else { "" },
        format_number(ppm, 1)
    )
}

fn decide_atmosphere<'a>(
    engine: &mut Engine<'a>,
    station: &'a SurveyStation,
) -> AtmosphereDecision {
    let settings = engine.settings;
    let instrument = &station.instrument;
    let decision = AtmosphereDecision::default();

    let mut computed = None;
    if let (Some(temperature), Some(pressure)) = (
        instrument.temperature_celsius,
        instrument.pressure_hectopascals,
    ) {
        let mut humidity = ASSUMED_HUMIDITY_PERCENT;
        if let Some(stated) = instrument.relative_humidity_percent {
            humidity = stated;
        } else {
            engine.warn_setup(
                station,
                "no humidity recorded; 60 % was assumed for the atmospheric correction (under \
                 1 ppm below 25 C).",
            );
        }
        computed = Some(atmospheric_ppm(temperature, pressure, humidity));
    }

    match settings.atmospheric {
        AtmosphericCorrection::None => decision,
        AtmosphericCorrection::Auto => {
            if instrument.atmospheric_ppm_state == CorrectionState::Applied {
                return decision; // the instrument did it; the setup record says so
            }
            if instrument.atmospheric_ppm_state == CorrectionState::Unknown {
                engine.warn_setup(
                    station,
                    "the file does not say whether the instrument applied an atmospheric \
                     correction, so none was applied. Choose Recompute or Fixed if the \
                     distances are raw.",
                );
                return decision;
            }
            if let Some(computed) = computed {
                // The note names the value; the setup row names the weather.
                return AtmosphereDecision {
                    apply: true,
                    ppm: computed,
                    note: ppm_text(computed),
                };
            }
            if let Some(recorded) = instrument.atmospheric_ppm {
                return AtmosphereDecision {
                    apply: true,
                    ppm: recorded,
                    note: ppm_text(recorded) + " as recorded",
                };
            }
            engine.warn_setup(
                station,
                "the instrument did not apply an atmospheric correction and the file records \
                 no weather to compute one, so none was applied.",
            );
            decision
        }
        AtmosphericCorrection::Recompute => {
            let Some(computed) = computed else {
                engine.warn_setup(
                    station,
                    "no temperature and pressure recorded, so the atmospheric correction \
                     cannot be recomputed; distances used as recorded.",
                );
                return decision;
            };
            let mut net = computed;
            let mut note = ppm_text(computed);
            if instrument.atmospheric_ppm_state == CorrectionState::Applied {
                let Some(applied) = instrument.atmospheric_ppm else {
                    engine.warn_setup(
                        station,
                        "the instrument applied an atmospheric correction but the file does \
                         not say how much, so it cannot be taken out; distances used as \
                         recorded.",
                    );
                    return decision;
                };
                // (1 + new) / (1 + old), as ppm.
                net = ((1.0 + computed * 1e-6) / (1.0 + applied * 1e-6) - 1.0) * 1e6;
                note = format!("{} for {}", ppm_text(computed), ppm_text(applied));
            } else if instrument.atmospheric_ppm_state == CorrectionState::Unknown {
                engine.warn_setup(
                    station,
                    "the file does not say whether the instrument applied an atmospheric \
                     correction; the recomputed one was applied as if it had not.",
                );
            }
            AtmosphereDecision {
                apply: true,
                ppm: net,
                note,
            }
        }
        AtmosphericCorrection::Fixed => {
            if instrument.atmospheric_ppm_state == CorrectionState::Applied {
                return decision;
            }
            if instrument.atmospheric_ppm_state == CorrectionState::Unknown {
                engine.warn_setup(
                    station,
                    "the file does not say whether the instrument applied an atmospheric \
                     correction; the fixed value was applied as if it had not.",
                );
            }
            AtmosphereDecision {
                apply: true,
                ppm: settings.fixed_ppm,
                note: ppm_text(settings.fixed_ppm) + " fixed",
            }
        }
    }
}

fn is_prism_target(target: &TargetInfo) -> bool {
    let kind = target.target_type.to_ascii_lowercase();
    !kind.contains("reflectorless")
        && !kind.contains("non-prism")
        && !kind.contains("nonprism")
        && !kind.contains("tape")
}

// The prism constant correction of one distance, in metres, and its note.
// `warned` keeps the "not stated" warning to one per setup.
fn prism_correction<'a>(
    engine: &mut Engine<'a>,
    station: &'a SurveyStation,
    distance: &DistanceObservation,
    warned: &mut bool,
) -> Option<AppliedCorrection> {
    let settings = engine.settings;
    if settings.prism_constant_policy == PrismConstantPolicy::None
        || !is_prism_target(&distance.target)
    {
        return None;
    }
    // The shot's own record wins over the setup's: the prism may have been
    // changed mid setup.
    let shot_states = distance.target.prism_constant.is_some()
        || distance.target.prism_constant_state != CorrectionState::Unknown;
    let constant = if shot_states {
        distance.target.prism_constant
    } else {
        station.instrument.prism_constant
    };
    let state = if shot_states {
        distance.target.prism_constant_state
    } else {
        station.instrument.prism_constant_state
    };
    let mut not_stated = |engine: &mut Engine<'a>, what: &str| {
        if !*warned {
            *warned = true;
            engine.warn_setup(station, what);
        }
    };

    if settings.prism_constant_policy == PrismConstantPolicy::Auto {
        if state == CorrectionState::NotApplied {
            let Some(constant) = constant else {
                not_stated(
                    engine,
                    "the file says the prism constant is not in the distances but not what it \
                     is; distances used as recorded.",
                );
                return None;
            };
            return Some(AppliedCorrection {
                kind: CorrectionKind::PrismConstant,
                amount: constant,
                factor: None,
                note: String::from("recorded"),
            });
        }
        if state == CorrectionState::Unknown {
            not_stated(
                engine,
                "the file does not say whether the prism constant is in the distances; none \
                 was added.",
            );
        }
        return None;
    }
    // Override: take out what the instrument applied, add the chosen constant.
    let mut amount = settings.prism_constant;
    if state == CorrectionState::Applied {
        let Some(constant) = constant else {
            not_stated(
                engine,
                "the instrument applied a prism constant the file does not state, so it cannot \
                 be replaced; distances used as recorded.",
            );
            return None;
        };
        amount -= constant;
    } else if state == CorrectionState::Unknown {
        not_stated(
            engine,
            "the file does not say whether the prism constant is in the distances; the chosen \
             constant was added as if it were not.",
        );
    }
    Some(AppliedCorrection {
        kind: CorrectionKind::PrismConstant,
        amount,
        factor: None,
        note: String::from(if state == CorrectionState::Applied {
            "replaced"
        } else {
            "chosen"
        }),
    })
}

// ---- Phase A: one setup ---------------------------------------------------------------

// The observations of one pointing.
#[derive(Clone, Copy)]
struct Shot<'a> {
    target: &'a str,
    face: Face,
    pointing_index: usize,
    direction: Option<&'a HorizontalDirectionObservation>,
    zenith: Option<&'a ZenithAngleObservation>,
    vertical: Option<&'a VerticalAngleObservation>,
    distance: Option<&'a DistanceObservation>,
    // Report rows.
    direction_row: usize,
    zenith_row: usize,
    distance_row: usize,
    // After prism and atmosphere.
    corrected_distance: f64,
    // FL-equivalent zenith (vertical angle converted, face right folded).
    zenith_value: f64,
}

impl<'a> Shot<'a> {
    fn new(target: &'a str, pointing: Pointing) -> Self {
        Self {
            target,
            face: pointing.face,
            pointing_index: pointing.index,
            direction: None,
            zenith: None,
            vertical: None,
            distance: None,
            direction_row: NO_ROW,
            zenith_row: NO_ROW,
            distance_row: NO_ROW,
            corrected_distance: 0.0,
            zenith_value: 0.0,
        }
    }

    fn has_zenith(&self) -> bool {
        self.zenith.is_some() || self.vertical.is_some()
    }

    fn rows(&self) -> [usize; 3] {
        [self.direction_row, self.zenith_row, self.distance_row]
    }
}

fn row<'e>(engine: &'e mut Engine<'_>, index: usize) -> &'e mut ReportObservation {
    &mut engine.report.observations[index]
}

#[allow(
    clippy::too_many_arguments,
    reason = "Mirrors the C++ addRow, one row's fields."
)]
fn add_row(
    engine: &mut Engine<'_>,
    station: &SurveyStation,
    kind: &str,
    to: &str,
    pointing: Pointing,
    angular: bool,
    raw: f64,
    source: &SourceRecord,
) -> usize {
    // One allocation for the whole chain instead of one per doubling: a
    // distance collects up to six corrections, and at 100 000 rows the
    // reallocations were a measurable share of the reduction.
    let observation = ReportObservation {
        station_id: station.setup.id.clone(),
        kind: String::from(kind),
        from: station.setup.point_id.clone(),
        to: String::from(to),
        pointing,
        angular,
        raw,
        reduced: Some(raw),
        source: source.clone(),
        corrections: Vec::with_capacity(6),
        ..Default::default()
    };
    engine.report.observations.push(observation);
    engine.report.observations.len() - 1
}

fn correct(
    engine: &mut Engine<'_>,
    row_index: usize,
    kind: CorrectionKind,
    amount: f64,
    factor: Option<f64>,
    note: String,
) {
    let observation = row(engine, row_index);
    observation.corrections.push(AppliedCorrection {
        kind,
        amount,
        factor,
        note,
    });
    if let Some(reduced) = observation.reduced.as_mut() {
        *reduced += amount;
    }
}

fn set_reduced(engine: &mut Engine<'_>, row_index: usize, value: f64) {
    row(engine, row_index).reduced = Some(value);
}

fn reject(engine: &mut Engine<'_>, row_index: usize, reason: &str) {
    let observation = row(engine, row_index);
    observation.rejected = true;
    observation.rejection_reason = String::from(reason);
}

fn reject_shot(engine: &mut Engine<'_>, shot: &Shot<'_>, reason: &str) {
    for index in shot.rows() {
        if index != NO_ROW {
            reject(engine, index, reason);
        }
    }
}

// Adds the rows of one shot to a pointing's bookkeeping.
fn collect_rows(pointing: &mut ReducedPointing<'_>, shot: &Shot<'_>) {
    if shot.direction_row != NO_ROW {
        pointing.direction_rows[pointing.direction_row_count] = shot.direction_row;
        pointing.direction_row_count += 1;
    }
    if shot.distance_row != NO_ROW {
        pointing.distance_rows[pointing.distance_row_count] = shot.distance_row;
        pointing.distance_row_count += 1;
    }
}

fn shot_source<'a>(shot: &Shot<'a>) -> Option<&'a SourceRecord> {
    if let Some(direction) = shot.direction {
        return Some(&direction.source);
    }
    if let Some(distance) = shot.distance {
        return Some(&distance.source);
    }
    if let Some(zenith) = shot.zenith {
        return Some(&zenith.source);
    }
    shot.vertical.map(|vertical| &vertical.source)
}

// Face-right single: the reading less 180 degrees, the zenith folded.
fn reduce_single<'a>(engine: &mut Engine<'a>, pointing: &mut ReducedPointing<'a>, shot: &Shot<'a>) {
    let settings = engine.settings;
    collect_rows(pointing, shot);
    pointing.face = shot.face;
    pointing.left_index = shot.pointing_index;
    pointing.source = shot_source(shot);
    if let Some(observation) = shot.direction {
        let mut direction = observation.direction;
        if shot.face == Face::Right {
            direction = normalize_angle(direction - PI);
            correct(
                engine,
                shot.direction_row,
                CorrectionKind::FaceMean,
                -PI,
                None,
                String::from("face right only"),
            );
        }
        let direction = normalize_angle(direction);
        pointing.direction = Some(direction);
        set_reduced(engine, shot.direction_row, direction);
        pointing.sigma_direction = settings.apriori.direction;
    }
    if shot.has_zenith() {
        pointing.zenith = Some(shot.zenith_value);
        pointing.sigma_zenith = settings.apriori.zenith;
    }
    if let Some(distance) = shot.distance {
        pointing.slope = Some(shot.corrected_distance);
        pointing.sigma_distance = distance_sigma(&settings.apriori, shot.corrected_distance);
        pointing.recorded_horizontal = distance.kind == DistanceKind::Horizontal;
        pointing.target_height = distance.target_height;
    } else if let Some(zenith) = shot.zenith {
        pointing.target_height = zenith.target_height;
    } else if let Some(vertical) = shot.vertical {
        pointing.target_height = vertical.target_height;
    }
}

// The raw value of a shot's zenith row: the zenith angle or the vertical angle.
fn raw_zenith(shot: &Shot<'_>) -> f64 {
    match (shot.zenith, shot.vertical) {
        (Some(zenith), _) => zenith.angle,
        (None, Some(vertical)) => vertical.angle,
        (None, None) => 0.0,
    }
}

// Face left with face right: the means, the spreads, and the check.
fn reduce_pair<'a>(
    engine: &mut Engine<'a>,
    setup_index: usize,
    pointing: &mut ReducedPointing<'a>,
    left: &Shot<'a>,
    right: &Shot<'a>,
) {
    let settings = engine.settings;
    let raw = engine.raw;
    let station = &raw.stations[setup_index];
    collect_rows(pointing, left);
    collect_rows(pointing, right);
    pointing.face = Face::Left;
    pointing.left_index = left.pointing_index;
    pointing.source = shot_source(left);

    let mut check = FacePairCheck {
        station_id: station.setup.id.clone(),
        target_id: String::from(left.target),
        left_pointing: left.pointing_index,
        right_pointing: right.pointing_index,
        ..Default::default()
    };
    if let Some(source) = shot_source(left) {
        check.left = source.clone();
    }
    if let Some(source) = shot_source(right) {
        check.right = source.clone();
    }

    // Horizontal: FL and FR - 180, meaned by half the wrapped difference so
    // that readings either side of zero mean correctly.
    if let (Some(left_direction), Some(right_direction)) = (left.direction, right.direction) {
        let l = left_direction.direction;
        let mut r = right_direction.direction;
        // A reading half a turn from its partner is raw face right; one within
        // a quarter turn was already reduced by the file.
        let folded = normalize_angle_signed(r - l).abs() > HALF_PI;
        if folded {
            r -= PI;
        }
        let difference = normalize_angle_signed(r - l);
        let mean = normalize_angle(l + 0.5 * difference);
        check.horizontal_spread = Some(difference.abs());
        pointing.direction = Some(mean);
        pointing.sigma_direction = settings.apriori.direction / 2.0_f64.sqrt();
        correct(
            engine,
            left.direction_row,
            CorrectionKind::FaceMean,
            normalize_angle_signed(mean - l),
            None,
            String::new(),
        );
        // Written as -180 deg and the small part, the way it is worked in a
        // field book, rather than wrapped to +179 59 58.
        correct(
            engine,
            right.direction_row,
            CorrectionKind::FaceMean,
            (if folded { -PI } else { 0.0 }) + normalize_angle_signed(mean - r),
            None,
            String::from(if folded { "less 180 deg" } else { "" }),
        );
        set_reduced(engine, left.direction_row, mean);
        set_reduced(engine, right.direction_row, mean);
    } else if left.direction.is_some() || right.direction.is_some() {
        let one_is_right = left.direction.is_none();
        let one = if one_is_right { right } else { left };
        let mut direction = one
            .direction
            .map_or(0.0, |observation| observation.direction);
        if one_is_right {
            direction = normalize_angle(direction - PI);
            correct(
                engine,
                one.direction_row,
                CorrectionKind::FaceMean,
                -PI,
                None,
                String::from("face right only"),
            );
        }
        let direction = normalize_angle(direction);
        pointing.direction = Some(direction);
        pointing.sigma_direction = settings.apriori.direction;
        set_reduced(engine, one.direction_row, direction);
    }

    // Zenith: the mean of FL and the folded FR removes the index error
    // i = (FL - FR') / 2 (with FR' = 360 - FR).
    let left_zenith = left.has_zenith();
    let right_zenith = right.has_zenith();
    if left_zenith && right_zenith {
        let mean = 0.5 * (left.zenith_value + right.zenith_value);
        check.zenith_spread = Some((left.zenith_value - right.zenith_value).abs());
        pointing.zenith = Some(mean);
        pointing.sigma_zenith = settings.apriori.zenith / 2.0_f64.sqrt();
        let left_before = row(engine, left.zenith_row)
            .reduced
            .unwrap_or(raw_zenith(left));
        let right_before = row(engine, right.zenith_row)
            .reduced
            .unwrap_or(raw_zenith(right));
        // The face-left row's amount is minus the index error.
        correct(
            engine,
            left.zenith_row,
            CorrectionKind::FaceMean,
            mean - left_before,
            None,
            String::new(),
        );
        correct(
            engine,
            right.zenith_row,
            CorrectionKind::FaceMean,
            mean - right_before,
            None,
            String::new(),
        );
        set_reduced(engine, left.zenith_row, mean);
        set_reduced(engine, right.zenith_row, mean);
    } else if left_zenith || right_zenith {
        pointing.zenith = Some(if left_zenith {
            left.zenith_value
        } else {
            right.zenith_value
        });
        pointing.sigma_zenith = settings.apriori.zenith;
    }

    // Distance: the mean of the two corrected distances.
    if let (Some(left_distance), Some(right_distance)) = (left.distance, right.distance) {
        let mean = 0.5 * (left.corrected_distance + right.corrected_distance);
        check.distance_spread = Some((left.corrected_distance - right.corrected_distance).abs());
        pointing.slope = Some(mean);
        pointing.sigma_distance = distance_sigma(&settings.apriori, mean);
        pointing.recorded_horizontal = left_distance.kind == DistanceKind::Horizontal;
        pointing.target_height = 0.5 * (left_distance.target_height + right_distance.target_height);
        correct(
            engine,
            left.distance_row,
            CorrectionKind::FaceMean,
            mean - left.corrected_distance,
            None,
            String::new(),
        );
        correct(
            engine,
            right.distance_row,
            CorrectionKind::FaceMean,
            mean - right.corrected_distance,
            None,
            String::new(),
        );
    } else if let Some((one, distance)) = left
        .distance
        .map(|distance| (left, distance))
        .or_else(|| right.distance.map(|distance| (right, distance)))
    {
        pointing.slope = Some(one.corrected_distance);
        pointing.sigma_distance = distance_sigma(&settings.apriori, one.corrected_distance);
        pointing.recorded_horizontal = distance.kind == DistanceKind::Horizontal;
        pointing.target_height = distance.target_height;
    } else if let Some(zenith) = left.zenith {
        pointing.target_height = zenith.target_height;
    }

    let tolerance = &settings.face_tolerances;
    check.within_tolerance = check
        .horizontal_spread
        .is_none_or(|spread| spread <= tolerance.horizontal)
        && check
            .zenith_spread
            .is_none_or(|spread| spread <= tolerance.zenith)
        && check
            .distance_spread
            .is_none_or(|spread| spread <= tolerance.distance);
    if !check.within_tolerance {
        let mut what = String::new();
        let mut add = |part: String| {
            if !what.is_empty() {
                what.push_str(", ");
            }
            what.push_str(&part);
        };
        if let Some(spread) = check
            .horizontal_spread
            .filter(|spread| *spread > tolerance.horizontal)
        {
            add(format!("horizontal {}", format_seconds(spread, 1)));
        }
        if let Some(spread) = check
            .zenith_spread
            .filter(|spread| *spread > tolerance.zenith)
        {
            add(format!("zenith {}", format_seconds(spread, 1)));
        }
        if let Some(spread) = check
            .distance_spread
            .filter(|spread| *spread > tolerance.distance)
        {
            add(format!("distance {}", format_millimetres(spread, 1)));
        }
        let text = format!("face pair outside tolerance ({what})");
        if tolerance.exclude_outside {
            pointing.rejected = true;
            reject_shot(engine, left, &text);
            reject_shot(engine, right, &text);
            engine.warn(
                format!(
                    "Setup {}, {}: {text}; the pair was excluded.",
                    station.setup.id, left.target
                ),
                check.left.clone(),
            );
        } else {
            engine.warn(
                format!(
                    "Setup {}, {}: {text}; the pair was used.",
                    station.setup.id, left.target
                ),
                check.left.clone(),
            );
        }
    }
    engine.report.face_pairs.push(check);
}

// Slope to horizontal, curvature and refraction, and the height difference.
fn reduce_to_horizontal<'a>(
    engine: &mut Engine<'a>,
    setup_index: usize,
    pointing: &mut ReducedPointing<'a>,
    warned_slope: &mut bool,
) {
    let settings = engine.settings;
    let raw = engine.raw;
    let station = &raw.stations[setup_index];
    if pointing.rejected {
        return;
    }
    let Some(slope) = pointing.slope else {
        return;
    };
    let instrument_height = station.setup.instrument_height;
    if pointing.recorded_horizontal {
        pointing.horizontal = Some(slope);
        if let Some(zenith) = pointing.zenith.filter(|zenith| zenith.sin() > 0.0) {
            // A recorded horizontal distance with its zenith still gives a height.
            let vertical = slope / zenith.tan();
            pointing.measured_vertical = vertical;
            let curvature = reduce_slope(
                vertical.hypot(slope),
                zenith,
                settings.curvature_and_refraction,
                settings.refraction_coefficient,
                settings.earth_radius,
            );
            pointing.height_difference = Some(
                vertical + curvature.vertical_correction + instrument_height
                    - pointing.target_height,
            );
        }
        return;
    }
    let Some(zenith) = pointing.zenith else {
        for &index in &pointing.distance_rows[..pointing.distance_row_count] {
            reject(
                engine,
                index,
                "no zenith angle in the same pointing to reduce it to horizontal",
            );
        }
        pointing.slope = None;
        return;
    };
    if !settings.slope_to_horizontal {
        if !*warned_slope {
            *warned_slope = true;
            engine.warn(
                format!(
                    "Setup {}: slope to horizontal is off, so its slope distances stay slope \
                     distances and position nothing.",
                    station.setup.id
                ),
                station.source.clone(),
            );
        }
        return;
    }

    let reduction = reduce_slope(
        slope,
        zenith,
        settings.curvature_and_refraction,
        settings.refraction_coefficient,
        settings.earth_radius,
    );
    pointing.measured_vertical = reduction.measured_vertical;
    let horizontal = reduction.measured_horizontal + reduction.horizontal_correction;
    pointing.horizontal = Some(horizontal);
    for &index in &pointing.distance_rows[..pointing.distance_row_count] {
        correct(
            engine,
            index,
            CorrectionKind::SlopeToHorizontal,
            reduction.measured_horizontal - slope,
            None,
            String::new(),
        );
        if settings.curvature_and_refraction {
            correct(
                engine,
                index,
                CorrectionKind::CurvatureRefraction,
                reduction.horizontal_correction,
                None,
                String::new(),
            );
        }
        set_reduced(engine, index, horizontal);
    }

    // The height difference mark to mark, as a row of its own: it is derived
    // from the slope distance and the zenith of this pointing together.
    let height_difference =
        reduction.measured_vertical + reduction.vertical_correction + instrument_height
            - pointing.target_height;
    pointing.height_difference = Some(height_difference);
    let mut corrections = Vec::with_capacity(2);
    if settings.curvature_and_refraction {
        corrections.push(AppliedCorrection {
            kind: CorrectionKind::CurvatureRefraction,
            amount: reduction.vertical_correction,
            ..Default::default()
        });
    }
    corrections.push(AppliedCorrection {
        kind: CorrectionKind::InstrumentAndTargetHeight,
        amount: instrument_height - pointing.target_height,
        ..Default::default()
    });
    let height = ReportObservation {
        station_id: station.setup.id.clone(),
        kind: String::from("height difference"),
        from: station.setup.point_id.clone(),
        to: String::from(pointing.target),
        pointing: Pointing {
            index: pointing.left_index,
            face: pointing.face,
        },
        raw: reduction.measured_vertical,
        corrections,
        reduced: Some(height_difference),
        source: pointing.source.cloned().unwrap_or_default(),
        ..Default::default()
    };
    engine.report.observations.push(height);
    pointing.height_row = Some(engine.report.observations.len() - 1);
}

// Everything that is not a pointing: kept as recorded, with a row that says so.
fn record_loose<'a>(engine: &mut Engine<'a>, observation: &'a Observation, station_id: &str) {
    let mut report_row = ReportObservation {
        station_id: String::from(station_id),
        kind: observation_kind_name(observation),
        source: observation_source(observation).clone(),
        ..Default::default()
    };
    let row_index = engine.report.observations.len();
    match observation {
        Observation::Distance(o) => {
            report_row.from = o.from.clone();
            report_row.to = o.to.clone();
            report_row.raw = o.distance;
        }
        Observation::HorizontalAngle(o) => {
            report_row.from = o.at.clone();
            report_row.to = o.to.clone();
            report_row.angular = true;
            report_row.raw = o.angle;
            engine.angles.push(RecordedAngle {
                at: &o.at,
                from: &o.from,
                to: &o.to,
                value: o.angle,
                sigma: o.sigma,
                row: row_index,
            });
        }
        Observation::Azimuth(o) => {
            report_row.from = o.from.clone();
            report_row.to = o.to.clone();
            report_row.angular = true;
            report_row.raw = o.azimuth;
            engine.angles.push(RecordedAngle {
                at: &o.from,
                from: "",
                to: &o.to,
                value: o.azimuth,
                sigma: o.sigma,
                row: row_index,
            });
        }
        Observation::LevelDifference(o) => {
            report_row.from = o.from.clone();
            report_row.to = o.to.clone();
            report_row.raw = o.height_difference;
        }
        Observation::GnssBaseline(o) => {
            report_row.from = o.from.clone();
            report_row.to = o.to.clone();
        }
        Observation::GnssGeocentricBaseline(o) => {
            report_row.from = o.from.clone();
            report_row.to = o.to.clone();
        }
        Observation::GnssPosition(o) => {
            report_row.to = o.point.clone();
        }
        Observation::GnssGlobalPosition(o) => {
            report_row.to = o.point.clone();
        }
        Observation::HorizontalDirection(o) => {
            report_row.from = o.at.clone();
            report_row.to = o.to.clone();
            report_row.angular = true;
            report_row.raw = o.direction;
        }
        Observation::ZenithAngle(o) => {
            report_row.from = o.from.clone();
            report_row.to = o.to.clone();
            report_row.angular = true;
            report_row.raw = o.angle;
        }
        Observation::VerticalAngle(o) => {
            report_row.from = o.from.clone();
            report_row.to = o.to.clone();
            report_row.angular = true;
            report_row.raw = o.angle;
        }
    }
    report_row.reduced = Some(report_row.raw);
    engine.report.observations.push(report_row);
}

// The shot of a pointing, by the file's pointing index: the one already
// grouped under that index for the same target, else a new one. A pointing
// index of 0 groups nothing.
fn shot_for<'a>(
    engine: &mut Engine<'a>,
    station: &'a SurveyStation,
    shots: &mut Vec<Shot<'a>>,
    by_pointing: &mut HashMap<usize, usize>,
    pointing: Pointing,
    target: &'a str,
) -> usize {
    if pointing.index != 0 {
        match by_pointing.get(&pointing.index).copied() {
            None => {
                by_pointing.insert(pointing.index, shots.len());
            }
            Some(existing) if shots[existing].target == target => return existing,
            Some(_) => {
                // One pointing index naming two targets: the file is
                // inconsistent; the second is a shot of its own.
                engine.warn(
                    format!(
                        "Setup {}: pointing {} names two targets; they were reduced separately.",
                        station.setup.id, pointing.index
                    ),
                    station.source.clone(),
                );
            }
        }
    }
    shots.push(Shot::new(target, pointing));
    shots.len() - 1
}

fn reduce_setup<'a>(
    engine: &mut Engine<'a>,
    setup_index: usize,
    shots: &mut Vec<Shot<'a>>,
    by_pointing: &mut HashMap<usize, usize>,
) {
    let settings = engine.settings;
    let raw = engine.raw;
    let station = &raw.stations[setup_index];

    let setup = ReportSetup {
        station_id: station.setup.id.clone(),
        point_id: station.setup.point_id.clone(),
        instrument_height: station.setup.instrument_height,
        backsight_point_id: station.backsight_point_id.clone(),
        instrument: station.instrument.clone(),
        source: station.source.clone(),
        ..Default::default()
    };
    engine.report.setups.push(setup);
    engine.setups[setup_index].report_index = engine.report.setups.len() - 1;

    let atmosphere = decide_atmosphere(engine, station);
    let mut warned_prism = false;
    let mut warned_slope = false;

    // Group by pointing, in the order of the file.
    shots.clear();
    by_pointing.clear();
    let no_pointing = Pointing::default();

    let station_point: &str = &station.setup.point_id;
    for observation in &station.observations {
        match observation {
            Observation::HorizontalDirection(direction) => {
                let at = shot_for(
                    engine,
                    station,
                    shots,
                    by_pointing,
                    direction.pointing,
                    &direction.to,
                );
                let at = if shots[at].direction.is_some() {
                    let fresh = shot_for(
                        engine,
                        station,
                        shots,
                        by_pointing,
                        no_pointing,
                        &direction.to,
                    );
                    shots[fresh].face = direction.pointing.face;
                    shots[fresh].pointing_index = direction.pointing.index;
                    fresh
                } else {
                    at
                };
                shots[at].direction = Some(direction);
                shots[at].direction_row = add_row(
                    engine,
                    station,
                    "horizontal direction",
                    &direction.to,
                    direction.pointing,
                    true,
                    direction.direction,
                    &direction.source,
                );
            }
            Observation::ZenithAngle(zenith) => {
                let at = shot_for(
                    engine,
                    station,
                    shots,
                    by_pointing,
                    zenith.pointing,
                    &zenith.to,
                );
                let target = if shots[at].has_zenith() {
                    shot_for(engine, station, shots, by_pointing, no_pointing, &zenith.to)
                } else {
                    at
                };
                let row_index = add_row(
                    engine,
                    station,
                    "zenith angle",
                    &zenith.to,
                    zenith.pointing,
                    true,
                    zenith.angle,
                    &zenith.source,
                );
                let shot = &mut shots[target];
                shot.face = zenith.pointing.face;
                shot.zenith = Some(zenith);
                shot.zenith_row = row_index;
                // A face-right zenith is stored folded (360 - reading) because
                // the model keeps zeniths in [0, 180]; a reading above 180 is
                // folded here.
                shot.zenith_value = zenith.angle;
                if zenith.angle > PI {
                    shot.zenith_value = TWO_PI - zenith.angle;
                    let amount = shot.zenith_value - zenith.angle;
                    correct(
                        engine,
                        row_index,
                        CorrectionKind::Other,
                        amount,
                        None,
                        String::from("360 less reading"),
                    );
                }
            }
            Observation::VerticalAngle(vertical) => {
                let at = shot_for(
                    engine,
                    station,
                    shots,
                    by_pointing,
                    vertical.pointing,
                    &vertical.to,
                );
                let target = if shots[at].has_zenith() {
                    shot_for(
                        engine,
                        station,
                        shots,
                        by_pointing,
                        no_pointing,
                        &vertical.to,
                    )
                } else {
                    at
                };
                let row_index = add_row(
                    engine,
                    station,
                    "vertical angle",
                    &vertical.to,
                    vertical.pointing,
                    true,
                    vertical.angle,
                    &vertical.source,
                );
                let shot = &mut shots[target];
                shot.face = vertical.pointing.face;
                shot.vertical = Some(vertical);
                shot.zenith_row = row_index;
                shot.zenith_value = HALF_PI - vertical.angle;
                let amount = shot.zenith_value - vertical.angle;
                correct(
                    engine,
                    row_index,
                    CorrectionKind::Other,
                    amount,
                    None,
                    String::from("as zenith"),
                );
            }
            Observation::Distance(distance) => {
                let at = shot_for(
                    engine,
                    station,
                    shots,
                    by_pointing,
                    distance.pointing,
                    &distance.to,
                );
                let target = if shots[at].distance.is_some() {
                    shot_for(
                        engine,
                        station,
                        shots,
                        by_pointing,
                        no_pointing,
                        &distance.to,
                    )
                } else {
                    at
                };
                let row_index = add_row(
                    engine,
                    station,
                    if distance.kind == DistanceKind::Slope {
                        "slope distance"
                    } else {
                        "horizontal distance"
                    },
                    &distance.to,
                    distance.pointing,
                    false,
                    distance.distance,
                    &distance.source,
                );
                shots[target].face = distance.pointing.face;
                shots[target].distance = Some(distance);
                shots[target].distance_row = row_index;
                let mut value = distance.distance;
                if let Some(prism) = prism_correction(engine, station, distance, &mut warned_prism)
                {
                    correct(
                        engine,
                        row_index,
                        prism.kind,
                        prism.amount,
                        None,
                        prism.note,
                    );
                    value += prism.amount;
                }
                if atmosphere.apply {
                    let factor = 1.0 + atmosphere.ppm * 1e-6;
                    correct(
                        engine,
                        row_index,
                        CorrectionKind::Atmospheric,
                        value * (factor - 1.0),
                        Some(factor),
                        atmosphere.note.clone(),
                    );
                    value *= factor;
                }
                shots[target].corrected_distance = value;
            }
            _ => record_loose(engine, observation, &station.setup.id),
        }
    }

    // Pair by target: the i-th face-left pointing of a target with its i-th
    // face-right one, in the order observed.
    let mut by_target: Vec<(&'a str, Vec<usize>)> = Vec::new();
    let mut target_slot: HashMap<&'a str, usize> = HashMap::new();
    for (i, shot) in shots.iter().enumerate() {
        if shot.target == station_point {
            reject_shot(engine, shot, "the target is the occupied point");
            continue;
        }
        let slot = *target_slot.entry(shot.target).or_insert_with(|| {
            by_target.push((shot.target, Vec::new()));
            by_target.len() - 1
        });
        by_target[slot].1.push(i);
    }

    for &(target, ref indices) in &by_target {
        let mut lefts = Vec::new();
        let mut rights = Vec::new();
        let mut singles = Vec::new();
        let mut unknowns = Vec::new();
        for &i in indices {
            if settings.faces == FaceHandling::Separate {
                singles.push(i);
            } else if shots[i].face == Face::Left {
                lefts.push(i);
            } else if shots[i].face == Face::Right {
                rights.push(i);
            } else {
                unknowns.push(i);
            }
        }
        // A file that does not state the face: two readings of one target
        // half a turn apart can only be the two faces, and meaning them as
        // one face would put the target in the wrong direction. The first
        // reading's side is taken as face left.
        let mut first_reading: Option<f64> = None;
        let mut opposite = false;
        for &i in &unknowns {
            if let Some(direction) = shots[i].direction {
                match first_reading {
                    None => first_reading = Some(direction.direction),
                    Some(first) => {
                        if normalize_angle_signed(direction.direction - first).abs() > HALF_PI {
                            opposite = true;
                        }
                    }
                }
            }
        }
        for &i in &unknowns {
            let (Some(direction), Some(first), true) =
                (shots[i].direction, first_reading, opposite)
            else {
                singles.push(i);
                continue;
            };
            let right = normalize_angle_signed(direction.direction - first).abs() > HALF_PI;
            shots[i].face = if right { Face::Right } else { Face::Left };
            if right {
                rights.push(i);
            } else {
                lefts.push(i);
            }
        }
        if opposite {
            engine.warn_setup(
                station,
                "the file does not state the faces; readings of one target half a turn apart \
                 were taken as face left and face right.",
            );
        }
        let mut pairs = 0;
        if settings.faces == FaceHandling::Average {
            pairs = lefts.len().min(rights.len());
            for (&l, &r) in lefts.iter().zip(&rights) {
                let mut pointing = ReducedPointing {
                    target,
                    ..Default::default()
                };
                let left = shots[l];
                let right = shots[r];
                reduce_pair(engine, setup_index, &mut pointing, &left, &right);
                reduce_to_horizontal(engine, setup_index, &mut pointing, &mut warned_slope);
                engine.setups[setup_index]
                    .pointings
                    .push(engine.pointings.len());
                engine.pointings.push(pointing);
            }
        }
        singles.extend_from_slice(&lefts[pairs..]);
        for &right in &rights[pairs..] {
            if settings.faces == FaceHandling::FaceLeftOnly {
                let shot = shots[right];
                reject_shot(engine, &shot, "face right not used (face left only)");
                continue;
            }
            singles.push(right);
        }
        singles.sort_unstable();
        for &i in &singles {
            let mut pointing = ReducedPointing {
                target,
                ..Default::default()
            };
            let shot = shots[i];
            reduce_single(engine, &mut pointing, &shot);
            reduce_to_horizontal(engine, setup_index, &mut pointing, &mut warned_slope);
            engine.setups[setup_index]
                .pointings
                .push(engine.pointings.len());
            engine.pointings.push(pointing);
        }
    }
}

// ---- Phase B helpers -----------------------------------------------------------------

fn is_phase_b(kind: CorrectionKind) -> bool {
    matches!(
        kind,
        CorrectionKind::HeightReduction
            | CorrectionKind::GridScale
            | CorrectionKind::CombinedFactor
            | CorrectionKind::Orientation
    )
}

// Rewinds a row to its phase A value, so phase B can run again after an
// adjustment moved the stations.
fn rewind(engine: &mut Engine<'_>, row_index: usize) {
    let observation = row(engine, row_index);
    let mut removed = 0.0;
    observation.corrections.retain(|correction| {
        if is_phase_b(correction.kind) {
            removed += correction.amount;
            false
        } else {
            true
        }
    });
    if let Some(reduced) = observation.reduced.as_mut()
        && removed != 0.0
    {
        *reduced -= removed;
    }
}

// The geoid separation a setup at `here` needs for its distances: for a height
// reduction to the ellipsoid or a grid scale from the projection, and never
// with the combined factor, which stands in for both.
fn geoid_for(engine: &Engine<'_>, here: &Position) -> Option<f64> {
    let settings = engine.settings;
    if (settings.height_reduction == HeightReduction::Ellipsoid
        || settings.grid_scale == GridScale::FromProjection)
        && let Some(geoid_separation) = &engine.context.geoid_separation
        && !settings.use_combined_factor
    {
        return geoid_separation(here.northing, here.easting);
    }
    None
}

// A notice the distance factors of a setup give once: the C++'s notice lambda.
#[derive(Clone, Copy)]
#[allow(
    clippy::enum_variant_names,
    reason = "Each notice names what the setup lacks, as the C++'s messages do."
)]
enum DistanceNotice {
    NoHeight,
    NoGeoid,
    NoScale,
}

// Phase B's factors on one horizontal distance, worked out without being
// written: the factors in the order applied (kind, amount, factor, note) and
// the setup's notices, for orient_and_radiate to record.
struct GridReduction {
    distance: f64,
    factors: [Option<(CorrectionKind, f64, f64, String)>; 2],
    notices: [Option<DistanceNotice>; 2],
}

// Phase B's factors on one pointing's horizontal distance from a station at
// `here`: the combined factor, or the height reduction and the grid scale (at
// the line's mid-point, or at the setup while the pointing has no azimuth).
// Recording them on the rows and giving the setup's notices is the caller's
// (the C++'s `record` flag); without, it is what a resection solves with
// before the station it computes has a position (resect_setup).
fn grid_reduction(
    engine: &Engine<'_>,
    setup_index: usize,
    pointing: &ReducedPointing<'_>,
    here: &Position,
    geoid: Option<f64>,
) -> Option<GridReduction> {
    let mut distance = pointing.horizontal?;
    let settings = engine.settings;
    let context = engine.context;
    let station = &engine.raw.stations[setup_index];
    let instrument_height = station.setup.instrument_height;
    let mut reduction = GridReduction {
        distance,
        factors: [None, None],
        notices: [None, None],
    };
    let mut factor_count = 0;
    let mut notice_count = 0;
    let mut factor = |distance: &mut f64, kind: CorrectionKind, value: f64, note: String| {
        reduction.factors[factor_count] = Some((kind, *distance * (value - 1.0), value, note));
        factor_count += 1;
        *distance *= value;
    };
    let mut notices = [None, None];
    let mut notice = |which: DistanceNotice| {
        notices[notice_count] = Some(which);
        notice_count += 1;
    };
    if settings.use_combined_factor {
        factor(
            &mut distance,
            CorrectionKind::CombinedFactor,
            settings.combined_factor,
            String::new(),
        );
    } else {
        if settings.height_reduction != HeightReduction::None {
            match here.height {
                None => notice(DistanceNotice::NoHeight),
                Some(station_height) => {
                    let mut height =
                        station_height + instrument_height + 0.5 * pointing.measured_vertical;
                    let mut ok = true;
                    if settings.height_reduction == HeightReduction::Ellipsoid {
                        match geoid {
                            Some(geoid) => height += geoid,
                            None => {
                                ok = false;
                                notice(DistanceNotice::NoGeoid);
                            }
                        }
                    }
                    if ok {
                        factor(
                            &mut distance,
                            CorrectionKind::HeightReduction,
                            height_reduction_factor(height, settings.earth_radius),
                            format!("h {} m", format_number(height, 1)),
                        );
                    }
                }
            }
        }
        if settings.grid_scale == GridScale::Fixed {
            factor(
                &mut distance,
                CorrectionKind::GridScale,
                settings.fixed_grid_scale_factor,
                String::from("fixed"),
            );
        } else if settings.grid_scale == GridScale::FromProjection {
            let mut northing = here.northing;
            let mut easting = here.easting;
            let mut place = "at setup";
            if let Some(azimuth) = pointing.azimuth {
                northing += 0.5 * distance * azimuth.cos();
                easting += 0.5 * distance * azimuth.sin();
                place = "at mid-point";
            }
            let mut ellipsoidal = here.height.unwrap_or(0.0) + geoid.unwrap_or(0.0);
            if let Some(height_difference) = pointing.height_difference {
                ellipsoidal += 0.5 * height_difference;
            }
            // reduce_and_adjust refuses FromProjection without the function.
            let scale = context
                .grid_scale_factor
                .as_ref()
                .and_then(|scale_factor| scale_factor(northing, easting, ellipsoidal));
            match scale.filter(|scale| scale.is_finite() && *scale > 0.0) {
                Some(scale) => factor(
                    &mut distance,
                    CorrectionKind::GridScale,
                    scale,
                    String::from(place),
                ),
                None => notice(DistanceNotice::NoScale),
            }
        }
    }
    reduction.notices = notices;
    reduction.distance = distance;
    Some(reduction)
}

// grid_reduction recorded: each factor on the pointing's distance rows, the
// setup's notices given once, the rows' reduced values set.
fn record_grid_distance<'a>(
    engine: &mut Engine<'a>,
    setup_index: usize,
    pointing: &ReducedPointing<'a>,
    here: &Position,
    geoid: Option<f64>,
) -> Option<f64> {
    let reduction = grid_reduction(engine, setup_index, pointing, here, geoid)?;
    let rows = &pointing.distance_rows[..pointing.distance_row_count];
    for (kind, amount, value, note) in reduction.factors.iter().flatten() {
        for &index in rows {
            correct(engine, index, *kind, *amount, Some(*value), note.clone());
        }
    }
    let raw = engine.raw;
    let station = &raw.stations[setup_index];
    for notice in reduction.notices.iter().flatten() {
        let state = &mut engine.setups[setup_index];
        let given = match notice {
            DistanceNotice::NoHeight => &mut state.warned_height,
            DistanceNotice::NoGeoid => &mut state.warned_geoid,
            DistanceNotice::NoScale => &mut state.warned_scale,
        };
        if *given {
            continue;
        }
        *given = true;
        let text = match notice {
            DistanceNotice::NoHeight => format!(
                ": its point {} has no height, so its distances were not reduced for height.",
                station.setup.point_id
            ),
            DistanceNotice::NoGeoid => String::from(
                ": the drawing gives no geoid separation there, so its distances were not \
                 reduced to the ellipsoid.",
            ),
            DistanceNotice::NoScale => String::from(
                ": the drawing's projection gives no scale factor there, so its distances were \
                 not reduced to grid.",
            ),
        };
        engine.warn(
            format!("Setup {}{text}", station.setup.id),
            station.source.clone(),
        );
    }
    for &index in rows {
        set_reduced(engine, index, reduction.distance);
    }
    Some(reduction.distance)
}

// The orientation of a resected setup (SetupState::resected) standing at
// `here`: the weighted mean of azimuth less reading over the directions its
// resection used, wrapped about the first (at the resection's own station,
// its least-squares orientation).
fn resection_orientation(engine: &Engine<'_>, setup_index: usize, here: &Position) -> Option<f64> {
    let mut first: Option<f64> = None;
    let mut offsets = 0.0;
    let mut weights = 0.0;
    for &(p, sigma) in &engine.setups[setup_index].resection_directions {
        let pointing = &engine.pointings[p];
        let (Some(target), Some(direction)) = (engine.find(pointing.target), pointing.direction)
        else {
            continue;
        };
        let delta_northing = target.northing - here.northing;
        let delta_easting = target.easting - here.easting;
        if delta_northing.hypot(delta_easting) <= tolerance::COORDINATE {
            continue;
        }
        let value = normalize_angle(delta_easting.atan2(delta_northing) - direction);
        let reference = *first.get_or_insert(value);
        let weight = 1.0 / (sigma * sigma);
        offsets += weight * normalize_angle_signed(value - reference);
        weights += weight;
    }
    let first = first?;
    Some(normalize_angle_signed(first + offsets / weights))
}

/// A pointing's horizontal distance with phase B's factors as they would be
/// from a station at `here` (at the setup, the pointing having no azimuth
/// yet), written nowhere: what a resection solves with before its station
/// exists. Absent without a horizontal distance.
pub(crate) fn grid_distance_from(
    engine: &Engine<'_>,
    setup_index: usize,
    pointing: &ReducedPointing<'_>,
    here: &Position,
) -> Option<f64> {
    grid_reduction(engine, setup_index, pointing, here, geoid_for(engine, here))
        .map(|reduction| reduction.distance)
}

fn any_row_rejected(engine: &Engine<'_>, rows: &[usize]) -> bool {
    rows.iter()
        .any(|&index| engine.report.observations[index].rejected)
}

/// Whether an adjustment has rejected the direction, the distance or the
/// height of `pointing` (marked its report rows, as a resection's outlier test
/// does before the network runs). What a resection rejected, its setup's
/// orientation leaves out (resect_setup), a mean direction leaves out
/// (mean_direction), and the network takes no more of, nor as its reference
/// direction (adjust_as_network). (A pointing the face-pair test excluded is
/// `rejected` as a whole instead.)
pub(crate) fn direction_rejected(engine: &Engine<'_>, pointing: &ReducedPointing<'_>) -> bool {
    any_row_rejected(
        engine,
        &pointing.direction_rows[..pointing.direction_row_count],
    )
}

pub(crate) fn distance_rejected(engine: &Engine<'_>, pointing: &ReducedPointing<'_>) -> bool {
    any_row_rejected(
        engine,
        &pointing.distance_rows[..pointing.distance_row_count],
    )
}

pub(crate) fn height_rejected(engine: &Engine<'_>, pointing: &ReducedPointing<'_>) -> bool {
    pointing
        .height_row
        .is_some_and(|index| engine.report.observations[index].rejected)
}

/// The mean circle reading of a setup's reduced pointings to `target`, absent
/// when none has a direction that was not rejected.
pub(crate) fn mean_direction(engine: &Engine<'_>, setup_index: usize, target: &str) -> Option<f64> {
    let mut first: Option<f64> = None;
    let mut sum = 0.0;
    let mut count = 0_usize;
    for &p in &engine.setups[setup_index].pointings {
        let pointing = &engine.pointings[p];
        let Some(direction) = pointing.direction else {
            continue;
        };
        if pointing.rejected || pointing.target != target || direction_rejected(engine, pointing) {
            continue;
        }
        let reference = *first.get_or_insert(direction);
        sum += normalize_angle_signed(direction - reference);
        count += 1;
    }
    let first = first?;
    Some(normalize_angle(first + sum / count as f64))
}

// Per target, the means of what one setup measured to it. Directions and
// distances are meaned apart, so a file that does not group a shot's
// observations into one pointing still radiates.
struct Accumulator<'a> {
    target: &'a str,
    first_azimuth: f64,
    // sum of (azimuth - first), wrapped
    azimuth_offsets: f64,
    azimuth_count: usize,
    distance: f64,
    distance_count: usize,
    height_difference: f64,
    height_count: usize,
}

/// Phase B for one setup: orientation, the distance factors that need a
/// position, and radiation of its targets. With `reradiate` None (the first
/// pass) a target is placed only where no position exists yet, and a target
/// that already has one becomes a check. After an adjustment moved the
/// stations, `reradiate` names the side shots, and only those are placed
/// again (overwriting what the first pass gave them).
///
/// The orientation comes from the backsight's coordinates. Only where no
/// coordinates can come (no backsight is named, or
/// SetupState::accept_circle_as_set says the named one will not be placed)
/// does the azimuth the file states for the backsight give it, or failing
/// that the circle set on the backsight, standing in for its grid azimuth.
/// Otherwise a setup whose backsight has no position yet is left unoriented
/// and radiates nothing, to be tried again.
pub(crate) fn orient_and_radiate<'a>(
    engine: &mut Engine<'a>,
    setup_index: usize,
    reradiate: Option<&HashSet<&'a str>>,
) {
    let raw = engine.raw;
    let station = &raw.stations[setup_index];
    // A copy: placing targets may move it.
    let Some(here) = engine.find(&station.setup.point_id).copied() else {
        return;
    };
    let report_index = engine.setups[setup_index].report_index;
    engine.setups[setup_index].positioned = true;

    // ---- orientation ----
    let backsight: &'a str = &station.backsight_point_id;
    let named_backsight = !backsight.is_empty() && backsight != station.setup.point_id;
    let backsight_position = if named_backsight {
        engine.find(backsight).copied()
    } else {
        None
    };
    let mut backsight_azimuth = None;
    let mut orientation_assumed = false;
    let mut orientation_stated = false;
    // No backsight named, one placed on top of the setup, or one nothing
    // will place: its coordinates will never orient this setup.
    let coordinates_will_not_orient = !named_backsight
        || backsight_position.is_some()
        || engine.setups[setup_index].accept_circle_as_set;
    if let Some(position) = backsight_position.filter(|position| {
        (position.northing - here.northing).hypot(position.easting - here.easting) > 1e-4
    }) {
        backsight_azimuth = Some(normalize_angle(
            (position.easting - here.easting).atan2(position.northing - here.northing),
        ));
    } else if let Some(stated) = station
        .stated_backsight_azimuth
        .filter(|_| coordinates_will_not_orient)
    {
        // The azimuth the file states for the backsight: only where no
        // coordinates of the backsight can orient the setup (see
        // SetupState::accept_circle_as_set for why not sooner). Less the
        // reading on the backsight below, it is the field software's own
        // orientation correction.
        backsight_azimuth = Some(normalize_angle(stated));
        orientation_assumed = true;
        orientation_stated = true;
    } else if let Some(circle) = station
        .backsight_azimuth
        .filter(|_| coordinates_will_not_orient)
    {
        // The circle as set, taken as the grid azimuth, on the same terms.
        backsight_azimuth = Some(normalize_angle(circle));
        orientation_assumed = true;
    }
    let backsight_reading = if backsight.is_empty() {
        None
    } else {
        mean_direction(engine, setup_index, backsight)
    };
    {
        let setup = &mut engine.report.setups[report_index];
        setup.backsight_azimuth = backsight_azimuth;
        setup.backsight_reading = backsight_reading;
    }
    let mut orientation = None;
    if let Some(azimuth) = backsight_azimuth {
        if let Some(reading) = backsight_reading {
            orientation = Some(normalize_angle_signed(azimuth - reading));
        } else if let Some(circle) = station.backsight_azimuth {
            // The circle was set to this reading on the backsight.
            orientation = Some(normalize_angle_signed(azimuth - normalize_angle(circle)));
        }
    }
    if engine.setups[setup_index].resected {
        // Oriented by its resection, whatever its backsight: a named one is
        // a target like the others there.
        orientation = resection_orientation(engine, setup_index, &here);
        orientation_assumed = false;
        orientation_stated = false;
    }
    {
        let state = &mut engine.setups[setup_index];
        state.orientation = orientation;
        state.orientation_assumed = orientation_assumed;
        state.orientation_stated = orientation_stated;
    }
    engine.report.setups[report_index].orientation_correction = orientation;

    // ---- per pointing: factors, azimuth, position ----
    let mut radiated: Vec<Accumulator<'a>> = Vec::new();
    let mut radiated_slot: HashMap<&'a str, usize> = HashMap::new();
    let geoid = geoid_for(engine, &here);
    let orientation_note = if orientation_stated {
        "azimuth as the file states"
    } else if orientation_assumed {
        "circle as set"
    } else {
        ""
    };

    let pointing_count = engine.setups[setup_index].pointings.len();
    for k in 0..pointing_count {
        let p = engine.setups[setup_index].pointings[k];
        let mut pointing = engine.pointings[p];
        if pointing.rejected {
            continue;
        }
        for &index in &pointing.distance_rows[..pointing.distance_row_count] {
            rewind(engine, index);
        }
        for &index in &pointing.direction_rows[..pointing.direction_row_count] {
            rewind(engine, index);
        }

        pointing.azimuth = None;
        if let (Some(direction), Some(orientation)) = (pointing.direction, orientation) {
            let azimuth = normalize_angle(direction + orientation);
            pointing.azimuth = Some(azimuth);
            for &index in &pointing.direction_rows[..pointing.direction_row_count] {
                correct(
                    engine,
                    index,
                    CorrectionKind::Orientation,
                    orientation,
                    None,
                    String::from(orientation_note),
                );
                set_reduced(engine, index, azimuth);
            }
        }

        pointing.grid_distance = record_grid_distance(engine, setup_index, &pointing, &here, geoid);
        engine.pointings[p] = pointing;

        if let Some(reradiate) = reradiate
            && !reradiate.contains(pointing.target)
            && pointing.target != backsight
        {
            continue;
        }
        if engine.setups[setup_index].resection_pointings.contains(&p) {
            continue; // its resection used it: its residuals are reported there, not as a check
        }
        if pointing.azimuth.is_none()
            && pointing.grid_distance.is_none()
            && pointing.height_difference.is_none()
        {
            continue;
        }
        let slot = *radiated_slot.entry(pointing.target).or_insert_with(|| {
            radiated.push(Accumulator {
                target: pointing.target,
                first_azimuth: 0.0,
                azimuth_offsets: 0.0,
                azimuth_count: 0,
                distance: 0.0,
                distance_count: 0,
                height_difference: 0.0,
                height_count: 0,
            });
            radiated.len() - 1
        });
        let a = &mut radiated[slot];
        if let Some(azimuth) = pointing.azimuth {
            if a.azimuth_count == 0 {
                a.first_azimuth = azimuth;
            }
            a.azimuth_offsets += normalize_angle_signed(azimuth - a.first_azimuth);
            a.azimuth_count += 1;
        }
        if let Some(distance) = pointing.grid_distance {
            a.distance += distance;
            a.distance_count += 1;
        }
        if let Some(height_difference) = pointing.height_difference {
            a.height_difference += height_difference;
            a.height_count += 1;
        }
    }

    // ---- place or check ----
    for a in &radiated {
        if a.azimuth_count == 0 || a.distance_count == 0 {
            continue; // an angle or a distance alone positions nothing
        }
        let count = a.distance_count as f64;
        let azimuth = a.first_azimuth + a.azimuth_offsets / a.azimuth_count as f64;
        let mut computed = Position {
            northing: here.northing + a.distance / count * azimuth.cos(),
            easting: here.easting + a.distance / count * azimuth.sin(),
            origin: PositionOrigin::Computed,
            method: ComputationMethod::Radiation,
            from_setup: setup_index,
            ..Default::default()
        };
        if let Some(station_height) = here.height
            && a.height_count > 0
        {
            computed.height = Some(station_height + a.height_difference / a.height_count as f64);
        }

        let existing = engine.find(a.target).copied();
        if a.target == backsight
            && let Some(existing) = existing
        {
            // The backsight check: measured against the coordinates.
            let known = (existing.northing - here.northing).hypot(existing.easting - here.easting);
            let setup = &mut engine.report.setups[report_index];
            setup.backsight_distance_difference = Some(a.distance / count - known);
            if let (Some(_), Some(existing_height), Some(computed_height), true) = (
                here.height,
                existing.height,
                computed.height,
                a.height_count > 0,
            ) {
                setup.backsight_height_difference = Some(computed_height - existing_height);
            }
            continue;
        }
        if reradiate.is_some() {
            // A point its resection was computed from is held by that
            // resection: a later pointing to it checks the station, and
            // places nothing.
            if !engine.setups[setup_index]
                .resection_targets
                .contains(a.target)
            {
                engine.place(a.target, computed);
            }
            continue;
        }
        let mut existing = match existing {
            Some(existing) if existing.origin != PositionOrigin::FileOnly => existing,
            _ => {
                engine.place(a.target, computed);
                continue;
            }
        };
        if existing.origin == PositionOrigin::Computed
            && existing.height.is_none()
            && computed.height.is_some()
        {
            existing.height = computed.height;
            if let Some(stored) = engine.find_mut(a.target) {
                stored.height = computed.height;
            }
        }
        let northing = computed.northing - existing.northing;
        let easting = computed.easting - existing.easting;
        let linear = northing.hypot(easting);
        let length = a.distance / count;
        let check = MisclosureReport {
            name: format!("{} from setup {}", a.target, station.setup.id),
            northing: Some(northing),
            easting: Some(easting),
            linear: Some(linear),
            length,
            precision_ratio: (linear > 0.0).then_some(length / linear),
            height: match (computed.height, existing.height) {
                (Some(computed_height), Some(existing_height)) => {
                    Some(computed_height - existing_height)
                }
                _ => None,
            },
            ..Default::default()
        };
        engine.report.misclosures.push(check);
    }
}

// ---- Seeds: control, entered points, GNSS ---------------------------------------------

fn seed_control<'a>(engine: &mut Engine<'a>, drawing_only: &mut HashSet<&'a str>) -> Status {
    let settings = engine.settings;
    let context = engine.context;
    for selection in &settings.control {
        let id: &str = &selection.point.point_id;
        let point: Option<&'a SurveyPoint> = if selection.origin == ControlOrigin::File {
            engine.file_points.get(id).copied()
        } else {
            let found = context
                .drawing_points
                .iter()
                .find(|candidate| candidate.id == id);
            if let Some(found) = found
                && !engine.file_points.contains_key(id)
            {
                drawing_only.insert(&found.id);
            }
            found
        };
        let Some(point) = point else {
            return Err(Error::new(
                ErrorCode::NotFound,
                format!(
                    "Control point {id} is not {}, so it cannot be held.",
                    if selection.origin == ControlOrigin::File {
                        "a point with coordinates in the file"
                    } else {
                        "on the drawing"
                    }
                ),
            ));
        };
        let held_height = selection.point.elevation.constraint != ControlConstraint::Free;
        if held_height && point.elevation.is_none() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "Control point {id} is held in height but has no height; give it one or \
                     free its height."
                ),
            ));
        }
        let sigma_of = |component: &ControlComponent| match component.constraint {
            ControlConstraint::Fixed => Some(0.0),
            ControlConstraint::Weighted => Some(component.sigma),
            ControlConstraint::Free => None,
        };
        let position = Position {
            northing: point.northing,
            easting: point.easting,
            height: point.elevation,
            origin: PositionOrigin::Control,
            method: ComputationMethod::Control,
            held_horizontal: selection.point.northing.constraint != ControlConstraint::Free
                || selection.point.easting.constraint != ControlConstraint::Free,
            sigma_northing: sigma_of(&selection.point.northing),
            sigma_easting: sigma_of(&selection.point.easting),
            sigma_height: sigma_of(&selection.point.elevation),
            ..Default::default()
        };
        engine.place(&point.id, position);
    }
    Ok(())
}

// The record a file's resection block ends at, from the setup's
// RESECTION_END_METADATA ("record N"): absent where the setup has none, or its
// value does not end in a record number, and then every pointing of the
// setup may enter its resection, as for a file that marks no block.
fn resection_block_end_of(station: &SurveyStation) -> Option<usize> {
    let value = station.metadata.get(RESECTION_END_METADATA)?;
    let value = trimmed(value.as_bytes());
    let last = match value.iter().rposition(|&byte| byte == b' ') {
        Some(space) => &value[space + 1..],
        None => value,
    };
    let record = parse_integer(last)?;
    if record <= 0 {
        return None;
    }
    Some(record as usize)
}

fn seed_entered(engine: &mut Engine<'_>) {
    let raw = engine.raw;
    for point in &raw.points {
        if point.coordinate_source != CoordinateSource::Entered || engine.find(&point.id).is_some()
        {
            continue;
        }
        engine.place(
            &point.id,
            Position {
                northing: point.northing,
                easting: point.easting,
                height: point.elevation,
                origin: PositionOrigin::Entered,
                method: ComputationMethod::Control,
                ..Default::default()
            },
        );
    }
}

// What seed_gnss found worth one sentence each rather than one per position.
#[derive(Default)]
struct GnssSeedNotices {
    // antenna heights to the ARP, not the phase centre
    to_reference_point: usize,
}

// GNSS positions: the grid ones as they are, the global ones through the
// context's conversion. The first position of a point places it, unless
// control or an entered point already holds it; a later one of a point an
// earlier position placed is a check on it, with its misclosure. Each is
// kept in engine.global_positions, and the network takes every one of a
// point GNSS placed, each with its own value.
//
// A global position is where the receiver was: at its antenna. The file's
// antenna height takes it down to the mark, the way reduction_gnss.rs takes a
// vector's ends down, so a position and a vector from it agree on what height
// they mean. A reader whose position is already of the mark gives no antenna
// (height 0).
fn seed_gnss(
    engine: &mut Engine<'_>,
    raw_index: usize,
    row_index: usize,
    notices: &mut GnssSeedNotices,
) {
    let raw = engine.raw;
    let settings = engine.settings;
    let context = engine.context;
    let observation = &raw.observations[raw_index];
    let mut converted = GlobalGridPosition {
        row: row_index,
        sigma_horizontal: settings.apriori.gnss_horizontal,
        sigma_vertical: settings.apriori.gnss_vertical,
        ..Default::default()
    };
    match observation {
        Observation::GnssPosition(position) => {
            converted.point = position.point.as_str();
            converted.northing = position.northing;
            converted.easting = position.easting;
            converted.height = Some(position.elevation);
            converted.sigma_horizontal = cpp::max(position.sigma_northing, position.sigma_easting);
            converted.sigma_vertical = position.sigma_elevation;
            converted.source = Some(&position.source);
        }
        Observation::GnssGlobalPosition(global) => {
            converted.point = global.point.as_str();
            converted.source = Some(&global.source);
            // A valid observation has exactly one of the two; the C++ reads
            // the geodetic one unchecked where neither is set.
            let grid = match (&global.geocentric, &global.geodetic) {
                (Some(geocentric), _) => grid_of_geocentric(context, geocentric),
                (None, Some(geodetic)) => grid_of_geodetic(context, geodetic),
                (None, None) => None,
            };
            let Some(grid) = grid else {
                engine.warn(
                    format!(
                        "GNSS position of {} not used: the drawing's coordinate system cannot \
                         convert it to grid.",
                        global.point
                    ),
                    global.source.clone(),
                );
                reject(engine, row_index, "no conversion to the drawing's grid");
                return;
            };
            if settings.use_file_covariances && global.covariance.stated() {
                // A geodetic value's covariance is already local north / east /
                // up; a geocentric one's is X / Y / Z and is turned at the point.
                let mut north = global.covariance.xx;
                let mut east = global.covariance.yy;
                let mut up = global.covariance.zz;
                if let Some(geocentric) = &global.geocentric {
                    let local =
                        local_variances(&global.covariance, &geodetic_from_geocentric(geocentric));
                    north = local.north;
                    east = local.east;
                    up = local.up;
                }
                if north > 0.0 && east > 0.0 && up > 0.0 {
                    converted.sigma_horizontal = cpp::max(north, east).sqrt();
                    converted.sigma_vertical = up.sqrt();
                }
            }
            converted.northing = grid.northing;
            converted.easting = grid.easting;
            // The row follows the height, the one value the reduction changes:
            // from the ellipsoid at the antenna down to the mark.
            {
                let report_row = row(engine, row_index);
                report_row.raw = grid.ellipsoidal_height;
                report_row.reduced = Some(grid.ellipsoidal_height);
            }
            let mut height = grid.ellipsoidal_height;
            if let Some(geoid_separation) = &context.geoid_separation
                && let Some(separation) = geoid_separation(grid.northing, grid.easting)
            {
                correct(
                    engine,
                    row_index,
                    CorrectionKind::Other,
                    -separation,
                    None,
                    format!("geoid separation {} m", format_number(separation, 3)),
                );
                height -= separation;
            }
            // The antenna: the horizontal position needs nothing (the plumb line
            // through a 2 m pole moves 0.3 mm over the grid's convergence), the
            // height comes down by the antenna height.
            let antenna = &global.antenna;
            let mut grid_words = format!(
                "to grid N {} E {}",
                format_number(grid.northing, 3),
                format_number(grid.easting, 3)
            );
            if antenna.height != 0.0 && reducible_antenna(antenna) {
                correct(
                    engine,
                    row_index,
                    CorrectionKind::InstrumentAndTargetHeight,
                    -antenna.height,
                    None,
                    format!("antenna {}", antenna_words(antenna)),
                );
                height -= antenna.height;
                if antenna.method == AntennaHeightMethod::Vertical {
                    notices.to_reference_point += 1;
                }
                converted.height = Some(height);
            } else if antenna.height != 0.0 {
                engine.warn(
                    format!(
                        "GNSS position of {}: its antenna height ({}) is not a vertical one, so \
                         the point's height was left out; its horizontal position was used.",
                        global.point,
                        antenna_words(antenna)
                    ),
                    global.source.clone(),
                );
                grid_words.push_str(&format!(
                    "; height left out, antenna {}",
                    antenna_words(antenna)
                ));
            } else {
                converted.height = Some(height);
            }
            correct(
                engine,
                row_index,
                CorrectionKind::Other,
                0.0,
                None,
                grid_words,
            );
            if global.solution == GnssSolution::Float || global.solution == GnssSolution::Autonomous
            {
                engine.warn(
                    format!(
                        "GNSS position of {} is a {} solution.",
                        global.point,
                        global.solution.text()
                    ),
                    global.source.clone(),
                );
            }
        }
        _ => return,
    }
    engine.global_positions[raw_index] = Some(converted);

    if let Some(existing) = engine.find(converted.point).copied() {
        if existing.origin != PositionOrigin::Gnss {
            return; // control or an entered point wins; the position still serves a vector
        }
        // An earlier occupation placed it: this one is a check on it, never
        // silently unused.
        let record = converted.source.map_or(0, |source| source.record_number);
        let northing = converted.northing - existing.northing;
        let easting = converted.easting - existing.easting;
        let check = MisclosureReport {
            name: format!(
                "{} by a second GNSS position{}",
                converted.point,
                if record != 0 {
                    format!(" (record {record})")
                } else {
                    String::new()
                }
            ),
            northing: Some(northing),
            easting: Some(easting),
            linear: Some(northing.hypot(easting)),
            height: match (converted.height, existing.height) {
                (Some(converted_height), Some(existing_height)) => {
                    Some(converted_height - existing_height)
                }
                _ => None,
            },
            ..Default::default()
        };
        engine.report.misclosures.push(check);
        return;
    }
    engine.place(
        converted.point,
        Position {
            northing: converted.northing,
            easting: converted.easting,
            height: converted.height,
            origin: PositionOrigin::Gnss,
            method: ComputationMethod::Gnss,
            sigma_northing: Some(converted.sigma_horizontal),
            sigma_easting: Some(converted.sigma_horizontal),
            sigma_height: Some(converted.sigma_vertical),
            ..Default::default()
        },
    );
}

// ---- Phase B over the setups ----------------------------------------------------------

// Phase B for every setup: each in file order once its station has a
// position, with GNSS vectors radiated as their bases get one (a setup may
// stand on an RTK point, a vector may start at a point a setup radiated).
//
// A setup whose named backsight has no position yet waits for it: a setup on
// control that backsights a traverse point is oriented once a later setup
// has radiated that point. It is tried again when THAT point is placed, not
// whenever anything is: with a thousand setups placed one at a time, trying
// every waiting setup after every placement took 12 s, where waiting on the
// one point that matters costs nothing. Each setup is tried at most twice.
//
// When nothing more can be tried, in this order:
//   0. a setup whose station nothing positions, and which observes enough
//      points that are placed (two with a direction and a distance, or three
//      with a direction) is resected from them (resect_setup), one at a time
//      in file order: its station is then placed, and it orients and radiates
//      as any other setup;
//   1. a setup whose station nothing positions stands on the file's own
//      coordinates for it, one at a time (its radiation may position others);
//   2. a setup still waiting for its backsight, where the file records the
//      circle set on it, is oriented on that circle taken as a grid azimuth,
//      one at a time in file order (it may place another setup's backsight),
//      and the report says so;
//   3. the rest are given up, and the report says why.
//
// The resection is a fallback, not a first choice: a station another setup
// radiates is placed that way as it always was, and where its setup is
// oriented on a backsight, or the circle as set, that setup's pointings to
// placed points are checks of it. A setup that names no backsight has nothing
// but the points it observes to orient it. On a station another setup
// radiated (a free station whose mark an earlier setup shot) it is resected
// from its block when it is tried, as the field software resected it, and the
// radiation becomes a check of the resection (resect_setup reports it); with
// too few placed points by then, it waits for step 0 like a station nothing
// positions. A station control, an entered or a GNSS point gives is not
// resected (a resection must not move it), nor one another resection placed
// (docs/survey.md, "The reduction's resection", Not done).
//
// The resection comes before the file's own coordinates because it is
// computed from what was measured, which is what the reduction is for, where
// those coordinates are the field software's or a person's, unchecked: the
// same order as a radiated point, which replaces the file's coordinates of a
// point (PositionOrigin::FileOnly). So a setup that stood on the file's own
// coordinates before (step 1) and observes enough placed points is now
// resected instead: the file's coordinates are then reported as a check of the
// resection, with a warning, never dropped unseen (resect_setup). It comes
// before the circle as set, which is an assumption about the instrument. A
// resection that is refused (resect_setup says why) leaves the setup to the
// later steps, which name the refusal.

// What a waiting setup observes of one target, for the resection candidates.
#[derive(Clone, Copy)]
struct Watch {
    setup: usize,
    direction: bool,
    distance: bool,
}

type MinHeap = BinaryHeap<Reverse<usize>>;

const NONE_CURRENT: usize = usize::MAX;

struct Placer<'a> {
    count: usize,
    done: Vec<bool>,
    tried: Vec<bool>,
    queued: Vec<bool>,
    remaining: usize,
    // Setups by the point they wait for: their station, or, once tried, their
    // backsight.
    waiting_for: HashMap<&'a str, Vec<usize>>,
    // Waiting setups whose file records the circle set on the backsight.
    circle_candidates: MinHeap,
    // A pass tries setups in file order; one woken by a setup later in the
    // file than itself waits for the next pass, as a sweep through the file
    // would leave it.
    this_pass: MinHeap,
    next_pass: MinHeap,
    // Entries of engine.position_order already looked up in waiting_for.
    woken: usize,
    // Resection candidates (step 0): setups whose station has no position,
    // counted as the points they observe are placed, so that a placement
    // costs only the setups watching that point.
    watching: HashMap<&'a str, Vec<Watch>>,
    placed_with_direction: Vec<usize>,
    placed_with_both: Vec<usize>,
    resection_queued: Vec<bool>,
    resection_candidates: MinHeap,
    // Why, for the later steps' words.
    resection_refused: Vec<String>,
    // Setups that name no backsight, on a station another setup radiated, not
    // yet resected (see above): tried again at step 0 as their targets are
    // placed, their stations placed already.
    awaiting_resection: Vec<bool>,
}

impl<'a> Placer<'a> {
    fn new(count: usize) -> Self {
        Self {
            count,
            done: vec![false; count],
            tried: vec![false; count],
            queued: vec![false; count],
            remaining: count,
            waiting_for: HashMap::new(),
            circle_candidates: MinHeap::new(),
            this_pass: MinHeap::new(),
            next_pass: MinHeap::new(),
            woken: 0,
            watching: HashMap::new(),
            placed_with_direction: vec![0; count],
            placed_with_both: vec![0; count],
            resection_queued: vec![false; count],
            resection_candidates: MinHeap::new(),
            resection_refused: vec![String::new(); count],
            awaiting_resection: vec![false; count],
        }
    }

    fn on_a_radiated_station(&self, engine: &Engine<'a>, s: usize) -> bool {
        let state = &engine.setups[s];
        let station = &engine.raw.stations[s];
        if state.orientation.is_some() || state.resected || !station.backsight_point_id.is_empty() {
            return false;
        }
        engine
            .find(&station.setup.point_id)
            .is_some_and(|position| {
                position.origin == PositionOrigin::Computed
                    && position.method == ComputationMethod::Radiation
                    && position.from_setup != s
            })
    }

    fn count_placed(&mut self, watch: Watch) {
        let s = watch.setup;
        self.placed_with_direction[s] += usize::from(watch.direction);
        self.placed_with_both[s] += usize::from(watch.direction && watch.distance);
        if !self.resection_queued[s]
            && (self.placed_with_both[s] >= 2 || self.placed_with_direction[s] >= 3)
        {
            self.resection_queued[s] = true;
            self.resection_candidates.push(Reverse(s));
        }
    }

    fn watch_targets(&mut self, engine: &Engine<'a>, s: usize) {
        let station = &engine.raw.stations[s];
        // Looked up only: each target's watch is counted or filed once, and
        // the order of that makes no difference.
        let mut targets: HashMap<&'a str, Watch> = HashMap::new();
        for &p in &engine.setups[s].pointings {
            let pointing = &engine.pointings[p];
            if pointing.rejected
                || pointing.target == station.setup.point_id
                || !in_resection_block(engine, s, pointing)
            {
                continue;
            }
            let watch = targets.entry(pointing.target).or_insert(Watch {
                setup: s,
                direction: false,
                distance: false,
            });
            watch.direction = watch.direction || pointing.direction.is_some();
            watch.distance = watch.distance || pointing.horizontal.is_some();
        }
        for (target, watch) in targets {
            if engine.find(target).is_some() {
                self.count_placed(watch);
            } else {
                self.watching.entry(target).or_default().push(watch);
            }
        }
    }

    fn wake(&mut self, engine: &Engine<'a>, current: usize) {
        while self.woken < engine.position_order.len() {
            let placed = engine.position_order[self.woken];
            self.woken += 1;
            if let Some(watches) = self.watching.remove(placed) {
                for watch in watches {
                    if !self.done[watch.setup] {
                        self.count_placed(watch);
                    }
                }
            }
            let Some(waiters) = self.waiting_for.remove(placed) else {
                continue;
            };
            for s in waiters {
                if self.done[s] || self.queued[s] {
                    continue;
                }
                self.queued[s] = true;
                if current == NONE_CURRENT || s > current {
                    self.this_pass.push(Reverse(s));
                } else {
                    self.next_pass.push(Reverse(s));
                }
            }
        }
    }

    fn waits_for_backsight(&self, engine: &Engine<'a>, s: usize) -> bool {
        let station = &engine.raw.stations[s];
        let backsight = &station.backsight_point_id;
        engine.setups[s].orientation.is_none()
            && !backsight.is_empty()
            && *backsight != station.setup.point_id
            && engine.find(backsight).is_none()
    }

    fn warn_unoriented(&self, engine: &mut Engine<'a>, s: usize) {
        let raw = engine.raw;
        let station = &raw.stations[s];
        let backsight = &station.backsight_point_id;
        let why = if backsight.is_empty() {
            let mut why = String::from(" has no backsight");
            // A setup like a second resection block on a station already
            // placed observes placed points that could orient it, which the
            // reduction does not do (docs/survey.md, "The reduction's
            // resection", Not done): named, so the warning is not read as a
            // setup that observed nothing known.
            let mut placed: Vec<&str> = Vec::new();
            for &p in &engine.setups[s].pointings {
                let pointing = &engine.pointings[p];
                if !pointing.rejected
                    && pointing.direction.is_some()
                    && engine.find(pointing.target).is_some()
                    && !placed.contains(&pointing.target)
                {
                    placed.push(pointing.target);
                }
            }
            if !placed.is_empty() {
                why.push_str(&format!(
                    "; it observes {} placed point{} (",
                    placed.len(),
                    if placed.len() == 1 { "" } else { "s" }
                ));
                for (i, target) in placed.iter().take(5).enumerate() {
                    if i != 0 {
                        why.push_str(", ");
                    }
                    why.push_str(target);
                }
                why.push_str(if placed.len() > 5 { ", ...)" } else { ")" });
                why.push_str(
                    ", but a setup on a placed station is not oriented on the points it observes",
                );
            }
            why
        } else if *backsight == station.setup.point_id {
            String::from(
                " cannot be oriented: its backsight is the point it stands on and no circle \
                 setting was recorded",
            )
        } else if engine.find(backsight).is_none() {
            format!(
                " cannot be oriented: its backsight {backsight} has no position and no circle \
                 setting was recorded"
            )
        } else {
            format!(
                " cannot be oriented on its backsight {backsight}: it has no direction to it, or \
                 stands on the same position, and no circle setting was recorded"
            )
        };
        // One on a radiated station that its block could not resect: why not.
        let mut unresected = String::new();
        if self.awaiting_resection[s] {
            unresected = if self.resection_refused[s].is_empty() {
                resection_shortfall(engine, s)
            } else {
                self.resection_refused[s].clone()
            };
        }
        engine.warn(
            format!(
                "Setup {}{why}, so its directions are not oriented and its targets are not \
                 radiated.{}",
                station.setup.id,
                if unresected.is_empty() {
                    String::new()
                } else {
                    format!(" It was not resected: {unresected}.")
                }
            ),
            station.source.clone(),
        );
    }

    fn finish(&mut self, engine: &mut Engine<'a>, s: usize) {
        self.done[s] = true;
        self.remaining -= 1;
        let raw = engine.raw;
        let station = &raw.stations[s];
        let state = &engine.setups[s];
        let assumed = state.orientation_assumed;
        let oriented = state.orientation.is_some();
        let stated = state.orientation_stated;
        let accepted = state.accept_circle_as_set;
        if assumed && oriented && stated {
            let text = if accepted {
                "its backsight has no position, so its directions were oriented on the azimuth \
                 the file states for the backsight, less the reading on it: the bearings are the \
                 file's, not ones computed from coordinates."
            } else {
                "no backsight coordinates orient it, so its directions were oriented on the \
                 azimuth the file states for its backsight, less the reading on it: the bearings \
                 are the file's, not ones computed from coordinates."
            };
            engine.warn_setup(station, text);
        } else if assumed && oriented {
            let text = if accepted {
                "its backsight has no position, so its directions were oriented on the circle \
                 reading set on the backsight, taken as a grid azimuth: the bearings are right \
                 only if the circle was set to one."
            } else {
                "no backsight coordinates orient it, so its directions were oriented on the \
                 circle reading as set, taken as a grid azimuth: the bearings are right only if \
                 the circle was set to one."
            };
            engine.warn_setup(station, text);
        } else if !oriented {
            self.warn_unoriented(engine, s);
        }
    }

    fn attempt(&mut self, engine: &mut Engine<'a>, s: usize) {
        self.queued[s] = false;
        if self.done[s] {
            return;
        }
        self.tried[s] = true;
        orient_and_radiate(engine, s, None);
        if self.waits_for_backsight(engine, s) {
            let raw = engine.raw;
            let station = &raw.stations[s];
            self.waiting_for
                .entry(&station.backsight_point_id)
                .or_default()
                .push(s);
            if station.backsight_azimuth.is_some() || station.stated_backsight_azimuth.is_some() {
                self.circle_candidates.push(Reverse(s));
            }
            return;
        }
        if self.on_a_radiated_station(engine, s) {
            // Its block is what orients it: resected now where its targets
            // are placed, and oriented and radiated on that.
            if self.placed_with_both[s] >= 2 || self.placed_with_direction[s] >= 3 {
                let mut why = String::new();
                if resect_setup(engine, s, &mut why) {
                    self.awaiting_resection[s] = false;
                    orient_and_radiate(engine, s, None);
                    self.finish(engine, s);
                    return;
                }
                self.resection_refused[s] = why;
            }
            self.awaiting_resection[s] = true;
            return;
        }
        self.finish(engine, s);
    }

    fn run(&mut self, engine: &mut Engine<'a>) {
        let raw = engine.raw;
        let stations = &raw.stations;
        let count = self.count;

        radiate_gnss_vectors(engine, None);
        for (s, station) in stations.iter().enumerate() {
            if engine.find(&station.setup.point_id).is_some() {
                self.queued[s] = true;
                self.this_pass.push(Reverse(s));
            } else {
                self.waiting_for
                    .entry(&station.setup.point_id)
                    .or_default()
                    .push(s);
                self.watch_targets(engine, s);
            }
        }
        // Everything placed so far was placed before anything waited for it.
        self.woken = engine.position_order.len();

        let mut fallback_cursor = 0; // setups before it can never fall back
        while self.remaining > 0 {
            while let Some(s) = self.next_pass.pop() {
                self.this_pass.push(s);
            }
            radiate_gnss_vectors(engine, None);
            self.wake(engine, NONE_CURRENT);
            if !self.this_pass.is_empty() {
                while let Some(Reverse(s)) = self.this_pass.pop() {
                    self.attempt(engine, s);
                    self.wake(engine, s);
                }
                continue;
            }

            // 0. A resection, for the first setup in the file that can have
            // one. Its station, once placed, wakes it as any placement wakes a
            // setup; one on a radiated station, placed already, is tried again
            // directly.
            let mut resected = false;
            while !resected {
                let Some(Reverse(s)) = self.resection_candidates.pop() else {
                    break;
                };
                self.resection_queued[s] = false;
                if self.done[s]
                    || (engine.find(&stations[s].setup.point_id).is_some()
                        && !self.awaiting_resection[s])
                {
                    continue;
                }
                let mut why = String::new();
                resected = resect_setup(engine, s, &mut why);
                if !resected {
                    self.resection_refused[s] = why;
                } else if self.awaiting_resection[s] {
                    self.awaiting_resection[s] = false;
                    self.queued[s] = true;
                    self.this_pass.push(Reverse(s));
                }
            }
            if resected {
                continue;
            }

            // 1. The file's own coordinates for a station nothing positions. A
            // setup already standing on a position keeps it.
            let mut fell_back = false;
            while fallback_cursor < count && !fell_back {
                let s = fallback_cursor;
                fallback_cursor += 1;
                let point_id = &stations[s].setup.point_id;
                if self.done[s] || engine.find(point_id).is_some() {
                    continue;
                }
                let Some(&file) = engine.file_points.get(point_id.as_str()) else {
                    continue;
                };
                engine.place(
                    &file.id,
                    Position {
                        northing: file.northing,
                        easting: file.easting,
                        height: file.elevation,
                        origin: PositionOrigin::FileOnly,
                        ..Default::default()
                    },
                );
                engine.warn(
                    format!(
                        "Setup {} stands on {point_id}, which is not control and was not \
                         computed{}; the file's own coordinates for it were used.",
                        stations[s].setup.id,
                        if self.resection_refused[s].is_empty() {
                            String::new()
                        } else {
                            format!(
                                " (its resection was refused: {})",
                                self.resection_refused[s]
                            )
                        }
                    ),
                    stations[s].source.clone(),
                );
                fell_back = true;
            }
            if fell_back {
                continue;
            }

            // 2. The circle as set (or the azimuth the file states for the
            // backsight) for the first setup still waiting that has one.
            while self
                .circle_candidates
                .peek()
                .is_some_and(|Reverse(s)| self.done[*s])
            {
                self.circle_candidates.pop();
            }
            if let Some(Reverse(s)) = self.circle_candidates.pop() {
                engine.setups[s].accept_circle_as_set = true;
                orient_and_radiate(engine, s, None);
                self.finish(engine, s);
                continue;
            }

            // 3. Nothing more can be done.
            for s in 0..count {
                if !self.done[s] && self.tried[s] {
                    self.done[s] = true;
                    self.remaining -= 1;
                    self.warn_unoriented(engine, s);
                }
            }
            for (s, station) in stations.iter().enumerate() {
                if !self.done[s] {
                    self.done[s] = true;
                    self.remaining -= 1;
                    // Why it was not resected, where it observes any placed point.
                    let mut why = self.resection_refused[s].clone();
                    if why.is_empty() {
                        why = resection_shortfall(engine, s);
                    }
                    engine.warn(
                        format!(
                            "Setup {} stands on {}, which has no position: nothing was computed \
                             from it.{}",
                            station.setup.id,
                            station.setup.point_id,
                            if why.is_empty() {
                                String::new()
                            } else {
                                format!(" It was not resected: {why}.")
                            }
                        ),
                        station.source.clone(),
                    );
                }
            }
        }
    }
}

fn place_setups(engine: &mut Engine<'_>) {
    let mut placer = Placer::new(engine.raw.stations.len());
    placer.run(engine);
}

// ---- Outputs ----------------------------------------------------------------------------

fn is_pointing_kind(observation: &Observation) -> bool {
    matches!(
        observation,
        Observation::HorizontalDirection(_)
            | Observation::ZenithAngle(_)
            | Observation::VerticalAngle(_)
            | Observation::Distance(_)
    )
}

// The raw project with every station's pointings replaced by the reduced
// ones. Built field by field rather than copied and edited: copying 100 000
// raw observations only to throw them away cost as much as reducing them.
fn reduced_project(engine: &Engine<'_>) -> SurveyProject {
    let raw = engine.raw;
    let mut reduced = SurveyProject {
        name: raw.name.clone(),
        coordinate_system: raw.coordinate_system.clone(),
        units: raw.units,
        points: raw.points.clone(),
        unpositioned_points: raw.unpositioned_points.clone(),
        observations: raw.observations.clone(),
        traverses: raw.traverses.clone(),
        features: raw.features.clone(),
        metadata: raw.metadata.clone(),
        source: raw.source.clone(),
        control_points: raw.control_points.clone(),
        gnss_sessions: raw.gnss_sessions.clone(),
        stations: Vec::with_capacity(raw.stations.len()),
    };
    for (s, from) in raw.stations.iter().enumerate() {
        let mut kept = Vec::with_capacity(3 * engine.setups[s].pointings.len() + 4);
        for observation in &from.observations {
            if !is_pointing_kind(observation) {
                kept.push(observation.clone());
            }
        }
        let at = &from.setup.point_id;
        for &p in &engine.setups[s].pointings {
            let pointing = &engine.pointings[p];
            if pointing.rejected {
                continue;
            }
            let source = pointing.source.cloned().unwrap_or_default();
            let group = Pointing {
                index: pointing.left_index,
                face: Face::Unknown,
            };
            if let Some(azimuth) = pointing.azimuth {
                kept.push(Observation::Azimuth(AzimuthObservation {
                    from: at.clone(),
                    to: String::from(pointing.target),
                    azimuth,
                    sigma: pointing.sigma_direction,
                    source: source.clone(),
                }));
            } else if let Some(direction) = pointing.direction {
                kept.push(Observation::HorizontalDirection(
                    HorizontalDirectionObservation {
                        at: at.clone(),
                        to: String::from(pointing.target),
                        direction,
                        sigma: pointing.sigma_direction,
                        source: source.clone(),
                        pointing: group,
                    },
                ));
            }
            let distance = pointing.grid_distance.or(pointing.horizontal);
            if let Some(distance) = distance
                && distance > 0.0
                && pointing.sigma_distance > 0.0
            {
                kept.push(Observation::Distance(DistanceObservation {
                    from: at.clone(),
                    to: String::from(pointing.target),
                    distance,
                    sigma: pointing.sigma_distance,
                    kind: DistanceKind::Horizontal,
                    instrument_height: from.setup.instrument_height,
                    target_height: pointing.target_height,
                    source: source.clone(),
                    pointing: group,
                    ..Default::default()
                }));
            }
            if let Some(zenith) = pointing.zenith
                && (0.0..=PI).contains(&zenith)
                && pointing.sigma_zenith > 0.0
            {
                kept.push(Observation::ZenithAngle(ZenithAngleObservation {
                    from: at.clone(),
                    to: String::from(pointing.target),
                    angle: zenith,
                    sigma: pointing.sigma_zenith,
                    instrument_height: from.setup.instrument_height,
                    target_height: pointing.target_height,
                    source,
                    pointing: group,
                }));
            }
        }
        reduced.stations.push(SurveyStation {
            setup: from.setup.clone(),
            backsight_point_id: from.backsight_point_id.clone(),
            backsight_azimuth: from.backsight_azimuth,
            stated_backsight_azimuth: from.stated_backsight_azimuth,
            observations: kept,
            metadata: from.metadata.clone(),
            source: from.source.clone(),
            instrument: from.instrument.clone(),
        });
    }
    reduced
}

/// Reduces `raw` under `settings`.
///
/// Errors are for input that cannot be reduced at all, in a sentence a
/// surveyor can act on: InvalidArgument for settings that fail
/// validate_reduction_settings or need a context function that is absent;
/// NotFound for control naming a point neither the file nor the drawing has;
/// AdjustmentFailure from the adjustment (no control, a datum defect,
/// divergence), with its message. A single bad observation is NOT an error:
/// it is rejected, and the report says which and why.
pub fn reduce_and_adjust(
    raw: &SurveyProject,
    settings: &ReductionSettings,
    context: &ReductionContext,
) -> Result<ReductionOutcome> {
    validate_reduction_settings(settings)?;
    if !settings.use_combined_factor {
        if settings.height_reduction == HeightReduction::Ellipsoid
            && context.geoid_separation.is_none()
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "Height reduction to the ellipsoid needs the geoid separation, and the drawing's \
                 coordinate system cannot give one. Reduce to the geoid, or use a combined scale \
                 factor.",
                "height reduction",
            ));
        }
        if settings.grid_scale == GridScale::FromProjection && context.grid_scale_factor.is_none() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "Grid scale from the projection needs the drawing's coordinate system, and the \
                 drawing has none. Give a fixed scale factor or a combined factor instead.",
                "grid scale",
            ));
        }
    }

    let mut engine = Engine::new(raw, settings, context);
    engine.report.created_utc = context.created_utc.clone();
    engine.report.input = context.input.clone();
    engine.report.settings = settings.clone();
    engine.setups = raw
        .stations
        .iter()
        .map(|station| SetupState {
            resection_block_end: resection_block_end_of(station),
            ..Default::default()
        })
        .collect();
    engine.file_points.reserve(raw.points.len());
    for point in &raw.points {
        engine.file_points.entry(&point.id).or_insert(point);
    }
    let observation_count = raw.observations.len()
        + raw
            .stations
            .iter()
            .map(|station| station.observations.len())
            .sum::<usize>();
    engine
        .report
        .observations
        .reserve(observation_count + observation_count / 3 + 8);
    engine.report.face_pairs.reserve(observation_count / 6 + 4);
    engine.pointings.reserve(observation_count / 3 + 8);
    engine
        .positions
        .reserve(raw.points.len() + raw.unpositioned_points.len() + 8);

    let mut drawing_only: HashSet<&str> = HashSet::new();
    seed_control(&mut engine, &mut drawing_only)?;
    seed_entered(&mut engine);

    // Phase A.
    {
        let mut shots = Vec::new();
        let mut by_pointing = HashMap::new();
        for s in 0..raw.stations.len() {
            reduce_setup(&mut engine, s, &mut shots, &mut by_pointing);
        }
    }
    // Observations belonging to no setup, GNSS positions, and GNSS vectors in
    // grid terms.
    {
        let mut loose_rows = Vec::with_capacity(raw.observations.len());
        engine.global_positions = vec![None; raw.observations.len()];
        let mut notices = GnssSeedNotices::default();
        for (i, observation) in raw.observations.iter().enumerate() {
            record_loose(&mut engine, observation, "");
            let row_index = engine.report.observations.len() - 1;
            loose_rows.push(row_index);
            seed_gnss(&mut engine, i, row_index, &mut notices);
        }
        if notices.to_reference_point > 0 {
            engine.warn(
                format!(
                    "{} GNSS position(s) have antenna heights to the antenna's reference point: \
                     the offset from there to the phase centre, a few centimetres and not in the \
                     file, remains in their heights.",
                    notices.to_reference_point
                ),
                SourceRecord::default(),
            );
        }
        convert_gnss_vectors(&mut engine, &loose_rows);
    }

    place_setups(&mut engine);
    radiate_gnss_vectors(&mut engine, None);

    engine.flush_setup_notices();

    // The adjustment.
    match settings.method {
        AdjustmentMethod::Traverse => adjust_as_traverse(&mut engine)?,
        AdjustmentMethod::Network => adjust_as_network(&mut engine)?,
        AdjustmentMethod::None => {}
    }

    engine.flush_setup_notices();

    // ---- Outputs ----
    let mut previous: HashMap<&str, &ComputedPoint> = HashMap::new();
    for point in &context.previous {
        previous.entry(&point.id).or_insert(point);
    }
    let mut points = Vec::with_capacity(engine.position_order.len());
    engine
        .report
        .coordinates
        .reserve(engine.position_order.len());
    for &id in &engine.position_order {
        let position = engine.positions[id];
        if position.origin == PositionOrigin::FileOnly {
            continue;
        }
        let mut coordinate = CoordinateReport {
            point_id: String::from(id),
            northing: position.northing,
            easting: position.easting,
            elevation: position.height,
            sigma_northing: position.sigma_northing,
            sigma_easting: position.sigma_easting,
            sigma_elevation: position.sigma_height,
            method: position.method,
            ..Default::default()
        };
        if let Some(before) = previous.get(id) {
            coordinate.shift_northing = Some(position.northing - before.northing);
            coordinate.shift_easting = Some(position.easting - before.easting);
            if let (Some(height), Some(elevation)) = (position.height, before.elevation) {
                coordinate.shift_elevation = Some(height - elevation);
            }
        }
        if !drawing_only.contains(id) {
            points.push(ComputedPoint {
                id: String::from(id),
                northing: position.northing,
                easting: position.easting,
                elevation: position.height,
                sigma_northing: position.sigma_northing,
                sigma_easting: position.sigma_easting,
                sigma_elevation: position.sigma_height,
                method: position.method,
            });
        }
        engine.report.coordinates.push(coordinate);
    }

    let mut reduced = reduced_project(&engine);
    {
        let mut computed: HashMap<&str, &ComputedPoint> = HashMap::with_capacity(points.len());
        for point in &points {
            if point.method != ComputationMethod::Control {
                computed.entry(&point.id).or_insert(point);
            }
        }
        for point in &mut reduced.points {
            if let Some(found) = computed.get(point.id.as_str()) {
                point.northing = found.northing;
                point.easting = found.easting;
                if found.elevation.is_some() {
                    point.elevation = found.elevation;
                }
                point.coordinate_source = CoordinateSource::Calculated;
            }
        }
        let mut still_unpositioned = Vec::new();
        for unpositioned in std::mem::take(&mut reduced.unpositioned_points) {
            let Some(found) = computed.get(unpositioned.id.as_str()) else {
                still_unpositioned.push(unpositioned);
                continue;
            };
            reduced.points.push(SurveyPoint {
                id: unpositioned.id,
                northing: found.northing,
                easting: found.easting,
                elevation: found.elevation,
                code: unpositioned.code,
                description: unpositioned.description,
                metadata: unpositioned.metadata,
                coordinate_source: CoordinateSource::Calculated,
                source: unpositioned.source,
            });
        }
        reduced.unpositioned_points = still_unpositioned;
    }

    Ok(ReductionOutcome {
        reduced,
        points,
        report: engine.report,
    })
}
