//! Least-squares adjustment of survey networks.
//!
//!   adjust_horizontal_network  2D, non-linear, Gauss-Newton on observation equations
//!   adjust_level_network       1D, linear
//!   adjust_traverse            a traverse turned into a network and adjusted rigorously
//!
//! Functional model of the horizontal adjustment (i = instrument, j = target,
//! dN = N_j - N_i, dE = E_j - E_i, d^2 = dN^2 + dE^2):
//!   distance   d_ij                                    partials wrt (N_j, E_j): dN/d, dE/d
//!   azimuth    atan2(dE, dN)                           partials wrt (N_j, E_j): -dE/d^2, dN/d^2
//!   angle      azimuth(at->to) - azimuth(at->from)     difference of two azimuth rows
//!   direction  azimuth(at->to) - o_at                  an azimuth row, and -1 wrt o_at
//!   GNSS baseline (dN, dE), GNSS position (N, E), weighted control: linear
//! Partials with respect to the instrument station have the opposite sign.
//! Angular misclosures are wrapped into (-pi, pi], so azimuths near 0 / 2*pi and
//! angles near a full turn are handled correctly.
//!
//! A horizontal direction is a circle reading, which becomes an azimuth only
//! with the circle's orientation: every direction read at one point is one SET
//! with one orientation unknown o (azimuth = direction + o), started at the mean
//! of azimuth less reading over the set and adjusted with the coordinates. The
//! set is the point's, not a setup's: the network has no notion of a setup, so
//! two setups on one point, whose circles were set apart, must not both be given
//! to it as directions (the reduction's network gives them as angles; its
//! resection, one setup at a time, as directions). Rejected: the angles from a
//! reference pointing that the reduction's network uses. They share that
//! pointing's error, which uncorrelated weights ignore, so a short first sight,
//! its centring error in every angle, spoils all of them; a set weights each
//! direction on its own, as the textbooks' and the field software's
//! free-station adjustments do.
//!
//! Stochastic model: uncorrelated observations, weight 1 / sigma^2, a-priori
//! variance factor 1. Observations that carry no horizontal information (slope
//! distances, vertical and zenith angles, level differences) are skipped by the
//! horizontal adjustment and listed in `unused_observations`; the level
//! adjustment uses level differences only.
//!
//! Approximate coordinates are taken from the network's points; Gauss-Newton
//! needs them roughly right (a traverse computation or a sketch-quality fix
//! suffices).
//!
//! The functions do not modify the network, hold no state and are safe to call
//! concurrently on the same network. Identical input gives bitwise identical
//! output.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use std::fmt;

use katana_core::log::{LogField, Logger};
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::numerics::{normalize_angle, normalize_angle_signed, tolerance};
use nalgebra::DMatrix;

use crate::coordinate::Coordinate2;
use crate::data_model::{
    AzimuthObservation, ControlConstraint, ControlPoint, DistanceKind, DistanceObservation,
    HorizontalAngleObservation, Observation, SurveyPoint, Traverse, TraverseKind, describe_source,
    referenced_points,
};
use crate::ellipse_core::ellipse_of;
use crate::error_propagation::{Covariance2, ErrorEllipse};
use crate::least_squares::CovarianceScaling;
use crate::lsq_core::{RankDeficiency, Whitened, scaled_matrix, solve_whitened, to_matrix};
use crate::matrix::Matrix;
use crate::network::SurveyNetwork;
use crate::statistics::chi_square_quantile;
use crate::traverse::{TraverseOptions, compute_traverse};

/// Metres. Gauss-Newton stops when the largest coordinate correction of an
/// iteration falls below this. It is the project-wide geometric tolerance
/// (0.1 micrometre): positions that differ by less are identical by project
/// policy, and it still lies ~50 ulp above the resolution of UTM sized doubles.
pub const DEFAULT_CONVERGENCE_THRESHOLD: f64 = tolerance::GEOMETRIC;

/// Iterations. Gauss-Newton converges quadratically near the solution; from
/// sketch-quality approximate coordinates it needs 3 to 6 iterations. Reaching
/// this limit means divergence (bad approximate coordinates or gross errors).
pub const DEFAULT_MAX_ITERATIONS: usize = 25;

#[derive(Clone, Copy)]
pub struct AdjustmentOptions<'a> {
    pub max_iterations: usize,
    /// Metres, > 0.
    pub convergence_threshold: f64,
    pub covariance_scaling: CovarianceScaling,
    /// Significance level alpha of the two-sided global test, in (0, 1).
    pub significance_level: f64,
    /// Optional, not owned. Receives one "survey" record per adjustment with
    /// counts and statistics, never coordinates.
    pub logger: Option<&'a Logger>,
}

impl Default for AdjustmentOptions<'_> {
    fn default() -> Self {
        Self {
            max_iterations: DEFAULT_MAX_ITERATIONS,
            convergence_threshold: DEFAULT_CONVERGENCE_THRESHOLD,
            covariance_scaling: CovarianceScaling::APosteriori,
            significance_level: 0.05,
            logger: None,
        }
    }
}

// Logger is neither Debug nor comparable; the options say only whether one is set.
impl fmt::Debug for AdjustmentOptions<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdjustmentOptions")
            .field("max_iterations", &self.max_iterations)
            .field("convergence_threshold", &self.convergence_threshold)
            .field("covariance_scaling", &self.covariance_scaling)
            .field("significance_level", &self.significance_level)
            .field("logger", &self.logger.is_some())
            .finish()
    }
}

/// Orientation is not a coordinate: it is the orientation unknown of the
/// direction set read at the point (see the functional model above), named by
/// that point like its coordinates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoordinateComponent {
    #[default]
    Northing,
    Easting,
    Elevation,
    Orientation,
}

/// Identifies one unknown: row / column k of the cofactor matrix.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AdjustedParameter {
    pub point_id: String,
    pub component: CoordinateComponent,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ResidualSource {
    /// `index` refers to SurveyNetwork::observations().
    #[default]
    Observed,
    /// `index` refers to SurveyNetwork::control_points() (weighted components).
    Control,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ResidualComponent {
    #[default]
    Value,
    Northing,
    Easting,
    Elevation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResidualRecord {
    pub source: ResidualSource,
    pub index: usize,
    /// Value for scalar observations; GNSS and control rows name their component.
    pub component: ResidualComponent,
    /// v = adjusted - observed; metres or radians.
    pub residual: f64,
    /// A-priori standard deviation of the observation.
    pub sigma: f64,
    pub redundancy_number: f64,
    /// v / (sigma * sqrt(r_i)), i.e. with sigma0 = 1; None when r_i ~ 0.
    pub standardized_residual: Option<f64>,
}

/// Two-sided chi-square test of H0: sigma0^2 = 1 on the statistic v^T P v.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlobalTest {
    /// v^T P v
    pub statistic: f64,
    /// chi2(r, alpha/2)
    pub lower_critical: f64,
    /// chi2(r, 1 - alpha/2)
    pub upper_critical: f64,
    pub significance_level: f64,
    /// lower_critical <= statistic <= upper_critical
    pub passed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdjustmentStatistics {
    /// Equations, including weighted control.
    pub observation_count: usize,
    pub unknown_count: usize,
    pub degrees_of_freedom: usize,
    /// v^T P v
    pub weighted_squared_residuals: f64,
    /// None when degrees_of_freedom = 0.
    pub variance_factor: Option<f64>,
    /// Factor between cofactor and covariance.
    pub covariance_scale: f64,
    /// None when degrees_of_freedom = 0.
    pub global_test: Option<GlobalTest>,
}

impl Default for AdjustmentStatistics {
    fn default() -> Self {
        Self {
            observation_count: 0,
            unknown_count: 0,
            degrees_of_freedom: 0,
            weighted_squared_residuals: 0.0,
            variance_factor: None,
            covariance_scale: 1.0,
            global_test: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdjustedStation {
    pub point_id: String,
    pub position: Coordinate2,
    /// Scaled by covariance_scale; zero for fixed components.
    pub covariance: Covariance2,
    pub sigma_northing: f64,
    pub sigma_easting: f64,
    /// Standard ellipse of `covariance`.
    pub ellipse: ErrorEllipse,
}

/// The orientation of the direction set read at one point.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdjustedOrientation {
    pub point_id: String,
    /// Radians in [0, 2*pi): azimuth = direction + orientation.
    pub orientation: f64,
    /// Scaled by covariance_scale.
    pub sigma: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HorizontalAdjustmentResult {
    /// Every point that takes part in the adjustment (including fixed control),
    /// in network order.
    pub stations: Vec<AdjustedStation>,
    /// One per point directions were read at, in network order.
    pub orientations: Vec<AdjustedOrientation>,
    /// Observation order, then control.
    pub residuals: Vec<ResidualRecord>,
    pub unused_observations: Vec<usize>,
    /// Order of the cofactor matrix.
    pub parameters: Vec<AdjustedParameter>,
    /// Qxx
    pub cofactor: Matrix,
    /// covariance_scale * Qxx
    pub covariance: Matrix,
    pub statistics: AdjustmentStatistics,
    /// Coordinate updates applied.
    pub iterations: usize,
    /// Largest |correction| of the final update, metres.
    pub last_correction: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdjustedElevation {
    pub point_id: String,
    pub elevation: f64,
    /// Scaled by covariance_scale; zero for fixed benchmarks.
    pub sigma: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelAdjustmentResult {
    /// Participating points, network order.
    pub elevations: Vec<AdjustedElevation>,
    pub residuals: Vec<ResidualRecord>,
    pub unused_observations: Vec<usize>,
    pub parameters: Vec<AdjustedParameter>,
    pub cofactor: Matrix,
    pub covariance: Matrix,
    pub statistics: AdjustmentStatistics,
}

/// A-priori precision used to turn a Traverse into weighted observations.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TraverseStochasticModel {
    /// Metres.
    pub distance_sigma_constant: f64,
    /// Parts per million of the distance.
    pub distance_sigma_ppm: f64,
    /// Radians, per measured angle.
    pub angle_sigma: f64,
    /// Radians. Standard deviation of the known reference azimuths. For Open
    /// and Link traverses it may be 0 (errorless reference); a ClosedLoop needs
    /// a positive value because its first-leg azimuth enters as an observation
    /// (use a small value such as 0.01" to hold it practically fixed).
    pub azimuth_sigma: f64,
}

/// The C++ `applyAdjustment` overloads: a result that can write itself back
/// into the network's points.
pub trait AppliedAdjustment {
    fn apply_to(&self, network: &mut SurveyNetwork) -> Status;
}

/// Writes adjusted positions / elevations back into the network's points.
pub fn apply_adjustment<R: AppliedAdjustment + ?Sized>(
    network: &mut SurveyNetwork,
    result: &R,
) -> Status {
    result.apply_to(network)
}

// Unit conversion of the distance-proportional part of an EDM standard
// deviation (parts per million), not a tolerance.
const PARTS_PER_MILLION: f64 = 1e-6;

// One linearised observation equation: sum(coefficient * correction) =
// misclosure + v.
#[derive(Clone, Copy)]
struct Equation {
    source: ResidualSource,
    index: usize,
    component: ResidualComponent,
    sigma: f64,
    // observed - computed from the current coordinates
    misclosure: f64,
    // An angle is the difference of two azimuths, each with four partials (two
    // per end); the occupied station therefore appears twice per component.
    terms: [(usize, f64); 8],
    term_count: usize,
}

impl Default for Equation {
    fn default() -> Self {
        Self {
            source: ResidualSource::Observed,
            index: 0,
            component: ResidualComponent::Value,
            sigma: 1.0,
            misclosure: 0.0,
            terms: [(0, 0.0); 8],
            term_count: 0,
        }
    }
}

impl Equation {
    // Coefficients of fixed components (None) multiply a zero correction and
    // are dropped.
    fn add_term(&mut self, parameter: Option<usize>, coefficient: f64) {
        if let Some(parameter) = parameter {
            self.terms[self.term_count] = (parameter, coefficient);
            self.term_count += 1;
        }
    }
}

fn check_options(options: &AdjustmentOptions<'_>) -> Status {
    if options.max_iterations == 0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "maxIterations must be at least 1",
        ));
    }
    if !options.convergence_threshold.is_finite() || !(options.convergence_threshold > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "convergenceThreshold must be positive and finite",
        ));
    }
    if !(options.significance_level > 0.0 && options.significance_level < 1.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "significanceLevel must lie strictly between 0 and 1",
        ));
    }
    Ok(())
}

// "N(P)", "E(P)" and "H(P)" for a coordinate, and "orientation(P)" for the
// orientation unknown of the directions read at P, which is not one.
fn parameter_name(parameter: &AdjustedParameter) -> String {
    let component = match parameter.component {
        CoordinateComponent::Northing => "N",
        CoordinateComponent::Easting => "E",
        CoordinateComponent::Elevation => "H",
        CoordinateComponent::Orientation => "orientation",
    };
    format!("{component}({})", parameter.point_id)
}

fn rank_error(deficiency: &RankDeficiency, parameters: &[AdjustedParameter], hint: &str) -> Error {
    let names = deficiency
        .unresolved
        .iter()
        .map(|&index| parameter_name(&parameters[index]))
        .collect::<Vec<_>>()
        .join(", ");
    let mut message = format!(
        "the network is rank deficient (rank {} of {} unknowns); unknowns that cannot be resolved: {names}",
        deficiency.rank,
        parameters.len()
    );
    if !hint.is_empty() {
        message.push_str(". ");
        message.push_str(hint);
    }
    Error::new(ErrorCode::AdjustmentFailure, message)
}

// Whitens (divides each row by its sigma) and solves.
fn solve_equations(equations: &[Equation], unknown_count: usize) -> Whitened {
    let mut design = DMatrix::<f64>::zeros(equations.len(), unknown_count);
    let mut misclosures = Vec::with_capacity(equations.len());
    for (r, equation) in equations.iter().enumerate() {
        for &(parameter, coefficient) in &equation.terms[..equation.term_count] {
            design[(r, parameter)] += coefficient / equation.sigma;
        }
        misclosures.push(equation.misclosure / equation.sigma);
    }
    solve_whitened(&design, &misclosures)
}

fn make_statistics(
    equations: &[Equation],
    unknown_count: usize,
    options: &AdjustmentOptions<'_>,
) -> Result<AdjustmentStatistics> {
    let mut statistics = AdjustmentStatistics {
        observation_count: equations.len(),
        unknown_count,
        degrees_of_freedom: equations.len() - unknown_count,
        ..Default::default()
    };
    for equation in equations {
        let whitened = equation.misclosure / equation.sigma;
        statistics.weighted_squared_residuals += whitened * whitened;
    }
    if statistics.degrees_of_freedom == 0 {
        // variance factor and global test are undefined
        return Ok(statistics);
    }
    let variance_factor =
        statistics.weighted_squared_residuals / statistics.degrees_of_freedom as f64;
    statistics.variance_factor = Some(variance_factor);
    if options.covariance_scaling == CovarianceScaling::APosteriori {
        statistics.covariance_scale = variance_factor;
    }

    let lower = chi_square_quantile(
        0.5 * options.significance_level,
        statistics.degrees_of_freedom,
    );
    let upper = chi_square_quantile(
        1.0 - 0.5 * options.significance_level,
        statistics.degrees_of_freedom,
    );
    let lower_critical = lower?;
    let upper_critical = upper?;
    let statistic = statistics.weighted_squared_residuals;
    statistics.global_test = Some(GlobalTest {
        statistic,
        lower_critical,
        upper_critical,
        significance_level: options.significance_level,
        // lower <= statistic <= upper, NaN failing as it does there
        passed: (lower_critical..=upper_critical).contains(&statistic),
    });
    Ok(statistics)
}

// Residuals of the final linearisation: v = computed - observed = -misclosure.
fn make_residuals(equations: &[Equation], leverage: &[f64]) -> Vec<ResidualRecord> {
    equations
        .iter()
        .zip(leverage)
        .map(|(equation, hat)| {
            let residual = -equation.misclosure;
            let redundancy_number = 1.0 - hat;
            let standardized_residual = if redundancy_number > tolerance::RELATIVE {
                Some(residual / (equation.sigma * redundancy_number.sqrt()))
            } else {
                None
            };
            ResidualRecord {
                source: equation.source,
                index: equation.index,
                component: equation.component,
                residual,
                sigma: equation.sigma,
                redundancy_number,
                standardized_residual,
            }
        })
        .collect()
}

fn log_outcome(
    options: &AdjustmentOptions<'_>,
    what: &str,
    statistics: &AdjustmentStatistics,
    iterations: usize,
) {
    let Some(logger) = options.logger else {
        return;
    };
    logger.info(
        "survey",
        what,
        &[
            LogField::new("observations", format!("{}", statistics.observation_count)),
            LogField::new("unknowns", format!("{}", statistics.unknown_count)),
            LogField::new(
                "degrees_of_freedom",
                format!("{}", statistics.degrees_of_freedom),
            ),
            LogField::new("iterations", format!("{iterations}")),
            LogField::new(
                "variance_factor",
                statistics
                    .variance_factor
                    .map_or_else(|| String::from("undefined"), format_exact_real),
            ),
        ],
    );
}

fn log_failure(options: &AdjustmentOptions<'_>, what: &str, error: &Error) {
    if let Some(logger) = options.logger {
        logger.error("survey", what, &[LogField::new("error", error.describe())]);
    }
}

// ---- horizontal network ------------------------------------------------------

fn is_horizontal(observation: &Observation) -> bool {
    match observation {
        Observation::Distance(distance) => distance.kind == DistanceKind::Horizontal,
        Observation::HorizontalAngle(_)
        | Observation::HorizontalDirection(_)
        | Observation::Azimuth(_)
        | Observation::GnssBaseline(_)
        | Observation::GnssPosition(_) => true,
        _ => false,
    }
}

struct HorizontalModel<'a> {
    network: &'a SurveyNetwork,
    used_observations: Vec<usize>,
    unused_observations: Vec<usize>,
    // per network point
    involved: Vec<bool>,
    // None for a fixed component, or a point outside the adjustment
    northing_parameter: Vec<Option<usize>>,
    easting_parameter: Vec<Option<usize>>,
    // Per network point: the orientation unknown of the directions read there.
    orientation_parameter: Vec<Option<usize>>,
    parameters: Vec<AdjustedParameter>,
}

// The network guarantees referential integrity: every id an observation or a
// control record names is one of its points.
fn point_of(network: &SurveyNetwork, id: &str) -> usize {
    network
        .point_index(id)
        .expect("the network refers only to its own points")
}

fn build_horizontal_model(network: &SurveyNetwork) -> HorizontalModel<'_> {
    let point_count = network.points().len();
    let mut model = HorizontalModel {
        network,
        used_observations: Vec::new(),
        unused_observations: Vec::new(),
        involved: vec![false; point_count],
        northing_parameter: vec![None; point_count],
        easting_parameter: vec![None; point_count],
        orientation_parameter: vec![None; point_count],
        parameters: Vec::new(),
    };
    // a direction set is read there
    let mut read_at = vec![false; point_count];

    for (i, observation) in network.observations().iter().enumerate() {
        if !is_horizontal(observation) {
            model.unused_observations.push(i);
            continue;
        }
        model.used_observations.push(i);
        for id in referenced_points(observation) {
            model.involved[point_of(network, &id)] = true;
        }
        if let Observation::HorizontalDirection(direction) = observation {
            read_at[point_of(network, &direction.at)] = true;
        }
    }

    // Unknowns in network point order, northing before easting: the layout of
    // the cofactor matrix is deterministic and documented.
    for (p, point) in network.points().iter().enumerate() {
        if !model.involved[p] {
            continue;
        }
        let control = network.control_for(&point.id);
        if control
            .as_ref()
            .is_none_or(|control| control.northing.constraint != ControlConstraint::Fixed)
        {
            model.northing_parameter[p] = Some(model.parameters.len());
            model.parameters.push(AdjustedParameter {
                point_id: point.id.clone(),
                component: CoordinateComponent::Northing,
            });
        }
        if control
            .as_ref()
            .is_none_or(|control| control.easting.constraint != ControlConstraint::Fixed)
        {
            model.easting_parameter[p] = Some(model.parameters.len());
            model.parameters.push(AdjustedParameter {
                point_id: point.id.clone(),
                component: CoordinateComponent::Easting,
            });
        }
    }
    // The orientations after every coordinate, in point order: nothing holds
    // an orientation, so each set always has its unknown.
    for (p, point) in network.points().iter().enumerate() {
        if read_at[p] {
            model.orientation_parameter[p] = Some(model.parameters.len());
            model.parameters.push(AdjustedParameter {
                point_id: point.id.clone(),
                component: CoordinateComponent::Orientation,
            });
        }
    }
    model
}

#[derive(Clone, Copy)]
struct Line {
    delta_northing: f64,
    delta_easting: f64,
    distance: f64,
}

fn line_between(
    model: &HorizontalModel<'_>,
    coordinates: &[Coordinate2],
    from: usize,
    to: usize,
) -> Result<Line> {
    let delta_northing = coordinates[to].northing - coordinates[from].northing;
    let delta_easting = coordinates[to].easting - coordinates[from].easting;
    let distance = delta_northing.hypot(delta_easting);
    if !distance.is_finite() {
        return Err(Error::new(
            ErrorCode::AdjustmentFailure,
            "the adjustment diverged: coordinates are no longer finite",
        ));
    }
    if distance <= tolerance::COORDINATE {
        let points = model.network.points();
        return Err(Error::new(
            ErrorCode::AdjustmentFailure,
            "two observed stations coincide; direction and distance partials are undefined",
        )
        .with_context(format!(
            "stations {} and {}",
            points[from].id, points[to].id
        )));
    }
    Ok(Line {
        delta_northing,
        delta_easting,
        distance,
    })
}

// Adds the azimuth partials of the line from -> to, scaled by `sign`.
fn add_azimuth_terms(
    equation: &mut Equation,
    model: &HorizontalModel<'_>,
    line: Line,
    from: usize,
    to: usize,
    sign: f64,
) {
    let squared = line.distance * line.distance;
    // d azimuth / d N_to
    let d_n = sign * -line.delta_easting / squared;
    // d azimuth / d E_to
    let d_e = sign * line.delta_northing / squared;
    equation.add_term(model.northing_parameter[to], d_n);
    equation.add_term(model.easting_parameter[to], d_e);
    equation.add_term(model.northing_parameter[from], -d_n);
    equation.add_term(model.easting_parameter[from], -d_e);
}

// `orientations` is per network point: the current value of the orientation
// unknown of the directions read there (unused elsewhere).
fn linearize_horizontal(
    model: &HorizontalModel<'_>,
    coordinates: &[Coordinate2],
    orientations: &[f64],
) -> Result<Vec<Equation>> {
    let network = model.network;
    let mut equations = Vec::new();

    for &index in &model.used_observations {
        let base = Equation {
            source: ResidualSource::Observed,
            index,
            ..Default::default()
        };
        match &network.observations()[index] {
            Observation::Distance(distance) => {
                let from = point_of(network, &distance.from);
                let to = point_of(network, &distance.to);
                let line = line_between(model, coordinates, from, to)?;
                let mut equation = base;
                equation.sigma = distance.sigma;
                equation.misclosure = distance.distance - line.distance;
                let d_n = line.delta_northing / line.distance;
                let d_e = line.delta_easting / line.distance;
                equation.add_term(model.northing_parameter[to], d_n);
                equation.add_term(model.easting_parameter[to], d_e);
                equation.add_term(model.northing_parameter[from], -d_n);
                equation.add_term(model.easting_parameter[from], -d_e);
                equations.push(equation);
            }
            Observation::Azimuth(azimuth) => {
                let from = point_of(network, &azimuth.from);
                let to = point_of(network, &azimuth.to);
                let line = line_between(model, coordinates, from, to)?;
                let mut equation = base;
                equation.sigma = azimuth.sigma;
                // Wrapped: an azimuth of 359.999° against a computed 0.001° is a
                // misclosure of -0.002°, not of 359.998°.
                equation.misclosure = normalize_angle_signed(
                    azimuth.azimuth - line.delta_easting.atan2(line.delta_northing),
                );
                add_azimuth_terms(&mut equation, model, line, from, to, 1.0);
                equations.push(equation);
            }
            Observation::HorizontalDirection(direction) => {
                let at = point_of(network, &direction.at);
                let to = point_of(network, &direction.to);
                let line = line_between(model, coordinates, at, to)?;
                let mut equation = base;
                equation.sigma = direction.sigma;
                // reading = azimuth - o, wrapped as an azimuth is.
                equation.misclosure = normalize_angle_signed(
                    direction.direction
                        - (line.delta_easting.atan2(line.delta_northing) - orientations[at]),
                );
                add_azimuth_terms(&mut equation, model, line, at, to, 1.0);
                equation.add_term(model.orientation_parameter[at], -1.0);
                equations.push(equation);
            }
            Observation::HorizontalAngle(angle) => {
                let at = point_of(network, &angle.at);
                let back = point_of(network, &angle.from);
                let fore = point_of(network, &angle.to);
                let back_line = line_between(model, coordinates, at, back)?;
                let fore_line = line_between(model, coordinates, at, fore)?;
                let mut equation = base;
                equation.sigma = angle.sigma;
                let computed = fore_line.delta_easting.atan2(fore_line.delta_northing)
                    - back_line.delta_easting.atan2(back_line.delta_northing);
                equation.misclosure = normalize_angle_signed(angle.angle - computed);
                // angle = azimuth(at->fore) - azimuth(at->back). The occupied
                // station receives one term per line and component;
                // solve_equations sums them.
                add_azimuth_terms(&mut equation, model, fore_line, at, fore, 1.0);
                add_azimuth_terms(&mut equation, model, back_line, at, back, -1.0);
                equations.push(equation);
            }
            Observation::GnssBaseline(baseline) => {
                let from = point_of(network, &baseline.from);
                let to = point_of(network, &baseline.to);
                let mut north = base;
                north.component = ResidualComponent::Northing;
                north.sigma = baseline.sigma_northing;
                north.misclosure = baseline.delta_northing
                    - (coordinates[to].northing - coordinates[from].northing);
                north.add_term(model.northing_parameter[to], 1.0);
                north.add_term(model.northing_parameter[from], -1.0);
                equations.push(north);

                let mut east = base;
                east.component = ResidualComponent::Easting;
                east.sigma = baseline.sigma_easting;
                east.misclosure =
                    baseline.delta_easting - (coordinates[to].easting - coordinates[from].easting);
                east.add_term(model.easting_parameter[to], 1.0);
                east.add_term(model.easting_parameter[from], -1.0);
                equations.push(east);
            }
            Observation::GnssPosition(position) => {
                let point = point_of(network, &position.point);
                let mut north = base;
                north.component = ResidualComponent::Northing;
                north.sigma = position.sigma_northing;
                north.misclosure = position.northing - coordinates[point].northing;
                north.add_term(model.northing_parameter[point], 1.0);
                equations.push(north);

                let mut east = base;
                east.component = ResidualComponent::Easting;
                east.sigma = position.sigma_easting;
                east.misclosure = position.easting - coordinates[point].easting;
                east.add_term(model.easting_parameter[point], 1.0);
                equations.push(east);
            }
            _ => {}
        }
    }

    // Weighted control: the published coordinate is an observation of the unknown.
    for (c, control) in network.control_points().iter().enumerate() {
        let point = point_of(network, &control.point_id);
        if !model.involved[point] {
            continue;
        }
        let published = &network.points()[point];
        if control.northing.constraint == ControlConstraint::Weighted {
            let mut equation = Equation {
                source: ResidualSource::Control,
                index: c,
                component: ResidualComponent::Northing,
                sigma: control.northing.sigma,
                misclosure: published.northing - coordinates[point].northing,
                ..Default::default()
            };
            equation.add_term(model.northing_parameter[point], 1.0);
            equations.push(equation);
        }
        if control.easting.constraint == ControlConstraint::Weighted {
            let mut equation = Equation {
                source: ResidualSource::Control,
                index: c,
                component: ResidualComponent::Easting,
                sigma: control.easting.sigma,
                misclosure: published.easting - coordinates[point].easting,
                ..Default::default()
            };
            equation.add_term(model.easting_parameter[point], 1.0);
            equations.push(equation);
        }
    }
    Ok(equations)
}

// Plain-language reason for the most common datum defects. A hint only: the
// authoritative decision is the numerical rank of the design matrix.
fn horizontal_datum_hint(model: &HorizontalModel<'_>) -> &'static str {
    let network = model.network;
    let mut constrained_points = 0;
    for control in network.control_points() {
        let horizontal = control.northing.constraint != ControlConstraint::Free
            || control.easting.constraint != ControlConstraint::Free;
        if horizontal && model.involved[point_of(network, &control.point_id)] {
            constrained_points += 1;
        }
    }
    let mut has_orientation = false;
    for &index in &model.used_observations {
        match &network.observations()[index] {
            Observation::GnssPosition(_) => constrained_points += 1,
            Observation::Azimuth(_) | Observation::GnssBaseline(_) => has_orientation = true,
            _ => {}
        }
    }
    if constrained_points == 0 {
        return "No control: the position of the network is undefined; fix or weight at least one point";
    }
    if constrained_points == 1 && !has_orientation {
        return "The orientation of the network is undefined; add an azimuth or a second control point";
    }
    "Check for stations that are tied in by too few observations"
}

fn adjust_horizontal(
    network: &SurveyNetwork,
    options: &AdjustmentOptions<'_>,
) -> Result<HorizontalAdjustmentResult> {
    check_options(options)?;
    let model = build_horizontal_model(network);
    if model.used_observations.is_empty() {
        return Err(Error::new(
            ErrorCode::AdjustmentFailure,
            "the network has no horizontal observations to adjust",
        ));
    }
    let unknown_count = model.parameters.len();
    if unknown_count == 0 {
        return Err(Error::new(
            ErrorCode::AdjustmentFailure,
            "the network has no free coordinates: every observed point is fixed",
        ));
    }

    let mut coordinates: Vec<Coordinate2> =
        network.points().iter().map(SurveyPoint::position).collect();

    // Each set's orientation starts at the mean of azimuth less reading over
    // its directions, wrapped about the first, from the approximate
    // coordinates: as near the answer as they are.
    let mut orientations = vec![0.0; coordinates.len()];
    {
        let mut first = vec![0.0; coordinates.len()];
        let mut offsets = vec![0.0; coordinates.len()];
        let mut count = vec![0_usize; coordinates.len()];
        for &index in &model.used_observations {
            let Observation::HorizontalDirection(direction) = &network.observations()[index] else {
                continue;
            };
            let at = point_of(network, &direction.at);
            let to = point_of(network, &direction.to);
            let value = (coordinates[to].easting - coordinates[at].easting)
                .atan2(coordinates[to].northing - coordinates[at].northing)
                - direction.direction;
            if count[at] == 0 {
                first[at] = value;
            }
            offsets[at] += normalize_angle_signed(value - first[at]);
            count[at] += 1;
        }
        for (p, orientation) in orientations.iter_mut().enumerate() {
            if count[p] > 0 {
                *orientation = first[p] + offsets[p] / count[p] as f64;
            }
        }
    }

    let mut result = HorizontalAdjustmentResult::default();
    let mut converged = false;
    loop {
        let equations = linearize_horizontal(&model, &coordinates, &orientations)?;
        if equations.len() < unknown_count {
            return Err(Error::new(
                ErrorCode::AdjustmentFailure,
                "fewer observation equations than unknown coordinates",
            )
            .with_context(format!(
                "equations {}, unknowns {unknown_count}",
                equations.len()
            )));
        }
        let core = match solve_equations(&equations, unknown_count) {
            Whitened::Solved(core) => core,
            Whitened::Deficient(deficiency) => {
                return Err(rank_error(
                    &deficiency,
                    &model.parameters,
                    horizontal_datum_hint(&model),
                ));
            }
        };

        if converged {
            // Linearised at the final coordinates: their residuals, cofactors
            // and redundancy numbers are reported; the (sub-threshold)
            // correction of this pass is not applied, so coordinates and
            // residuals agree exactly.
            let statistics = make_statistics(&equations, unknown_count, options)?;
            result.statistics = statistics;
            result.residuals = make_residuals(&equations, &core.leverage);
            result.cofactor = to_matrix(&core.cofactor);
            result.covariance = scaled_matrix(&core.cofactor, statistics.covariance_scale);
            break;
        }

        let mut largest: f64 = 0.0;
        for (p, coordinate) in coordinates.iter_mut().enumerate() {
            // std::max(largest, |correction|) is (largest < |c|) ? |c| : largest.
            if let Some(n) = model.northing_parameter[p] {
                let correction = core.parameters[n];
                coordinate.northing += correction;
                if largest < correction.abs() {
                    largest = correction.abs();
                }
            }
            if let Some(e) = model.easting_parameter[p] {
                let correction = core.parameters[e];
                coordinate.easting += correction;
                if largest < correction.abs() {
                    largest = correction.abs();
                }
            }
            // Not in `largest`, which is metres: an orientation is linear in
            // the model, so it has converged once the coordinates have.
            if let Some(o) = model.orientation_parameter[p] {
                orientations[p] += core.parameters[o];
            }
        }
        result.iterations += 1;
        result.last_correction = largest;
        if !largest.is_finite() {
            return Err(Error::new(
                ErrorCode::AdjustmentFailure,
                "the adjustment diverged: a coordinate correction is not finite",
            )
            .with_context(format!("iteration {}", result.iterations)));
        }
        if largest < options.convergence_threshold {
            converged = true;
        } else if result.iterations >= options.max_iterations {
            return Err(Error::new(
                ErrorCode::AdjustmentFailure,
                "the adjustment did not converge; check the approximate coordinates and look for gross errors",
            )
            .with_context(format!(
                "iterations {}, last correction {} m",
                result.iterations,
                format_exact_real(largest)
            )));
        }
    }

    result.parameters = model.parameters.clone();
    result.unused_observations = model.unused_observations.clone();
    for (p, coordinate) in coordinates.iter().enumerate() {
        if !model.involved[p] {
            continue;
        }
        let n = model.northing_parameter[p];
        let e = model.easting_parameter[p];
        let mut covariance = Covariance2::default();
        if let Some(n) = n {
            covariance.northing = result.covariance[(n, n)];
        }
        if let Some(e) = e {
            covariance.easting = result.covariance[(e, e)];
        }
        if let (Some(n), Some(e)) = (n, e) {
            covariance.northing_easting = result.covariance[(n, e)];
        }
        result.stations.push(AdjustedStation {
            point_id: network.points()[p].id.clone(),
            position: *coordinate,
            covariance,
            sigma_northing: covariance.northing.sqrt(),
            sigma_easting: covariance.easting.sqrt(),
            ellipse: ellipse_of(covariance),
        });
    }
    for (p, point) in network.points().iter().enumerate() {
        let Some(o) = model.orientation_parameter[p] else {
            continue;
        };
        result.orientations.push(AdjustedOrientation {
            point_id: point.id.clone(),
            orientation: normalize_angle(orientations[p]),
            sigma: result.covariance[(o, o)].sqrt(),
        });
    }
    Ok(result)
}

// ---- level network -----------------------------------------------------------

struct LevelModel<'a> {
    network: &'a SurveyNetwork,
    used_observations: Vec<usize>,
    unused_observations: Vec<usize>,
    involved: Vec<bool>,
    // per network point
    parameter: Vec<Option<usize>>,
    parameters: Vec<AdjustedParameter>,
}

fn build_level_model(network: &SurveyNetwork) -> LevelModel<'_> {
    let point_count = network.points().len();
    let mut model = LevelModel {
        network,
        used_observations: Vec::new(),
        unused_observations: Vec::new(),
        involved: vec![false; point_count],
        parameter: vec![None; point_count],
        parameters: Vec::new(),
    };

    for (i, observation) in network.observations().iter().enumerate() {
        let Observation::LevelDifference(level) = observation else {
            model.unused_observations.push(i);
            continue;
        };
        model.used_observations.push(i);
        model.involved[point_of(network, &level.from)] = true;
        model.involved[point_of(network, &level.to)] = true;
    }
    for (p, point) in network.points().iter().enumerate() {
        if !model.involved[p] {
            continue;
        }
        let control = network.control_for(&point.id);
        if control
            .as_ref()
            .is_none_or(|control| control.elevation.constraint != ControlConstraint::Fixed)
        {
            model.parameter[p] = Some(model.parameters.len());
            model.parameters.push(AdjustedParameter {
                point_id: point.id.clone(),
                component: CoordinateComponent::Elevation,
            });
        }
    }
    model
}

fn level_equations(model: &LevelModel<'_>, elevations: &[f64]) -> Vec<Equation> {
    let network = model.network;
    let mut equations = Vec::new();
    for &index in &model.used_observations {
        let Observation::LevelDifference(level) = &network.observations()[index] else {
            continue;
        };
        let from = point_of(network, &level.from);
        let to = point_of(network, &level.to);
        let mut equation = Equation {
            source: ResidualSource::Observed,
            index,
            sigma: level.sigma,
            misclosure: level.height_difference - (elevations[to] - elevations[from]),
            ..Default::default()
        };
        equation.add_term(model.parameter[to], 1.0);
        equation.add_term(model.parameter[from], -1.0);
        equations.push(equation);
    }
    for (c, control) in network.control_points().iter().enumerate() {
        let point = point_of(network, &control.point_id);
        if !model.involved[point] || control.elevation.constraint != ControlConstraint::Weighted {
            continue;
        }
        // adjust_level has refused a weighted benchmark with no height, so the
        // published value is there to be compared with.
        let published = network.points()[point].elevation.unwrap_or_default();
        let mut equation = Equation {
            source: ResidualSource::Control,
            index: c,
            component: ResidualComponent::Elevation,
            sigma: control.elevation.sigma,
            misclosure: published - elevations[point],
            ..Default::default()
        };
        equation.add_term(model.parameter[point], 1.0);
        equations.push(equation);
    }
    equations
}

fn adjust_level(
    network: &SurveyNetwork,
    options: &AdjustmentOptions<'_>,
) -> Result<LevelAdjustmentResult> {
    check_options(options)?;
    let model = build_level_model(network);
    if model.used_observations.is_empty() {
        return Err(Error::new(
            ErrorCode::AdjustmentFailure,
            "the network has no level differences to adjust",
        ));
    }
    let unknown_count = model.parameters.len();
    if unknown_count == 0 {
        return Err(Error::new(
            ErrorCode::AdjustmentFailure,
            "the network has no free elevations: every levelled point is fixed",
        ));
    }

    // A held or weighted benchmark IS the datum the run hangs on, so one with no
    // published height cannot be adjusted against - refusing it is the answer,
    // since starting it at zero would level the whole run onto the wrong datum
    // and report a perfect fit. A free point with no height starts at zero,
    // which costs nothing: the model is linear (see below), so the solve is
    // exact from any starting value.
    let mut elevations = Vec::with_capacity(network.points().len());
    for (p, point) in network.points().iter().enumerate() {
        if let Some(elevation) = point.elevation {
            elevations.push(elevation);
            continue;
        }
        if model.involved[p]
            && let Some(control) = network.control_for(&point.id)
            && control.elevation.constraint != ControlConstraint::Free
        {
            return Err(Error::new(
                ErrorCode::AdjustmentFailure,
                format!(
                    "benchmark '{}' is held in height but has no published elevation",
                    point.id
                ),
            )
            .with_context(describe_source(&point.source)));
        }
        elevations.push(0.0);
    }

    // The model is linear, so one solve for corrections to the stored
    // elevations is exact whatever those elevations are. Solving for
    // corrections rather than for the elevations keeps the right-hand side
    // small.
    let equations = level_equations(&model, &elevations);
    if equations.len() < unknown_count {
        return Err(Error::new(
            ErrorCode::AdjustmentFailure,
            "fewer level differences than unknown elevations",
        )
        .with_context(format!(
            "equations {}, unknowns {unknown_count}",
            equations.len()
        )));
    }
    let core = match solve_equations(&equations, unknown_count) {
        Whitened::Solved(core) => core,
        Whitened::Deficient(deficiency) => {
            return Err(rank_error(
                &deficiency,
                &model.parameters,
                "Every connected part of a level network needs a fixed or weighted benchmark",
            ));
        }
    };
    for (elevation, parameter) in elevations.iter_mut().zip(&model.parameter) {
        if let Some(k) = parameter {
            *elevation += core.parameters[*k];
        }
    }

    // Residuals from the adjusted elevations themselves, not from A x - l.
    let equations = level_equations(&model, &elevations);
    let statistics = make_statistics(&equations, unknown_count, options)?;

    let mut result = LevelAdjustmentResult {
        statistics,
        residuals: make_residuals(&equations, &core.leverage),
        unused_observations: model.unused_observations.clone(),
        parameters: model.parameters.clone(),
        cofactor: to_matrix(&core.cofactor),
        covariance: scaled_matrix(&core.cofactor, statistics.covariance_scale),
        elevations: Vec::new(),
    };
    for (p, &elevation) in elevations.iter().enumerate() {
        if !model.involved[p] {
            continue;
        }
        let sigma = model.parameter[p].map_or(0.0, |k| result.covariance[(k, k)].sqrt());
        result.elevations.push(AdjustedElevation {
            point_id: network.points()[p].id.clone(),
            elevation,
            sigma,
        });
    }
    Ok(result)
}

fn check_stochastic_model(traverse: &Traverse, model: &TraverseStochasticModel) -> Status {
    let non_negative = |value: f64| value.is_finite() && value >= 0.0;
    if !non_negative(model.distance_sigma_constant)
        || !non_negative(model.distance_sigma_ppm)
        || !(model.distance_sigma_constant > 0.0 || model.distance_sigma_ppm > 0.0)
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "distance sigma needs a non-negative constant and ppm part, at least one of them positive",
        ));
    }
    if !model.angle_sigma.is_finite() || !(model.angle_sigma > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "angle sigma must be positive and finite",
        ));
    }
    if !non_negative(model.azimuth_sigma) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "azimuth sigma must be finite and not negative",
        ));
    }
    if traverse.kind == TraverseKind::ClosedLoop && !(model.azimuth_sigma > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a closed loop needs a positive azimuth sigma: its first-leg azimuth enters the adjustment as an observation",
        ));
    }
    Ok(())
}

/// Errors (both adjustments)
///   InvalidArgument           bad options
///   AdjustmentFailure         no usable observations, no free coordinates, fewer
///                             equations than unknowns, datum defect / rank
///                             deficiency (the message names the unresolved
///                             unknowns), coincident stations, divergence
pub fn adjust_horizontal_network(
    network: &SurveyNetwork,
    options: &AdjustmentOptions<'_>,
) -> Result<HorizontalAdjustmentResult> {
    let result = adjust_horizontal(network, options);
    match &result {
        Ok(adjusted) => log_outcome(
            options,
            "horizontal network adjusted",
            &adjusted.statistics,
            adjusted.iterations,
        ),
        Err(error) => log_failure(options, "horizontal network adjustment failed", error),
    }
    result
}

pub fn adjust_level_network(
    network: &SurveyNetwork,
    options: &AdjustmentOptions<'_>,
) -> Result<LevelAdjustmentResult> {
    let result = adjust_level(network, options);
    match &result {
        Ok(adjusted) => log_outcome(options, "level network adjusted", &adjusted.statistics, 1),
        Err(error) => log_failure(options, "level network adjustment failed", error),
    }
    result
}

impl AppliedAdjustment for HorizontalAdjustmentResult {
    fn apply_to(&self, network: &mut SurveyNetwork) -> Status {
        // Validate everything first so that a failure leaves the network untouched.
        let mut updated = Vec::with_capacity(self.stations.len());
        for station in &self.stations {
            let mut point = network.point(&station.point_id)?;
            point.northing = station.position.northing;
            point.easting = station.position.easting;
            updated.push(point);
        }
        for point in updated {
            network.update_point(point)?;
        }
        Ok(())
    }
}

impl AppliedAdjustment for LevelAdjustmentResult {
    fn apply_to(&self, network: &mut SurveyNetwork) -> Status {
        let mut updated = Vec::with_capacity(self.elevations.len());
        for adjusted in &self.elevations {
            let mut point = network.point(&adjusted.point_id)?;
            point.elevation = Some(adjusted.elevation);
            updated.push(point);
        }
        for point in updated {
            network.update_point(point)?;
        }
        Ok(())
    }
}

/// Network equivalent of a traverse: stations at their compass-rule coordinates
/// (the approximate values), start (and end) held fixed, one distance per leg
/// (sigma^2 = constant^2 + (ppm * 1e-6 * d)^2), one angle per interior station.
/// An angle turned from a reference azimuth becomes an azimuth observation of
/// the adjoining leg with sigma^2 = angleSigma^2 + azimuthSigma^2.
pub fn build_traverse_network(
    traverse: &Traverse,
    model: &TraverseStochasticModel,
) -> Result<SurveyNetwork> {
    check_stochastic_model(traverse, model)?;
    // Compass-rule coordinates are the approximate values: already within the
    // misclosure of the least-squares solution.
    let preliminary = compute_traverse(traverse, &TraverseOptions::default())?;
    let setups = &traverse.setups;
    let is_loop = traverse.kind == TraverseKind::ClosedLoop;
    // link back to its start
    let closes_on_start = !is_loop && traverse.end_station_id == setups[0].station_id;

    let mut network = SurveyNetwork::default();
    for (s, station) in preliminary.stations.iter().enumerate() {
        if closes_on_start && s + 1 == preliminary.stations.len() {
            // the end station is the start point, already present
            break;
        }
        network.add_point(SurveyPoint {
            id: station.id.clone(),
            northing: station.adjusted.northing,
            easting: station.adjusted.easting,
            ..Default::default()
        })?;
    }
    network.add_control_point(ControlPoint::fixed_horizontal(
        setups[0].station_id.as_str(),
    ))?;
    if traverse.kind == TraverseKind::Link && !closes_on_start {
        network.add_control_point(ControlPoint::fixed_horizontal(
            traverse.end_station_id.as_str(),
        ))?;
    }

    let forward_station = |k: usize| {
        if k + 1 < setups.len() {
            &setups[k + 1].station_id
        } else if is_loop {
            &setups[0].station_id
        } else {
            &traverse.end_station_id
        }
    };
    let referenced_angle_sigma = model.angle_sigma.hypot(model.azimuth_sigma);

    for (k, setup) in setups.iter().enumerate() {
        network.add_observation(DistanceObservation {
            from: setup.station_id.clone(),
            to: String::from(forward_station(k)),
            distance: setup.distance,
            sigma: model
                .distance_sigma_constant
                .hypot(model.distance_sigma_ppm * PARTS_PER_MILLION * setup.distance),
            ..Default::default()
        })?;

        if k == 0 && !is_loop {
            // Angle turned from a reference direction of known azimuth:
            // equivalent to observing the azimuth of the first leg.
            network.add_observation(AzimuthObservation {
                from: setups[0].station_id.clone(),
                to: String::from(forward_station(0)),
                azimuth: normalize_angle(traverse.start_azimuth + setups[0].angle),
                sigma: referenced_angle_sigma,
                ..Default::default()
            })?;
            continue;
        }
        if k == 0 {
            network.add_observation(AzimuthObservation {
                from: setups[0].station_id.clone(),
                to: String::from(forward_station(0)),
                azimuth: normalize_angle(traverse.start_azimuth),
                sigma: model.azimuth_sigma,
                ..Default::default()
            })?;
        }
        let from = if k == 0 {
            &setups[setups.len() - 1].station_id
        } else {
            &setups[k - 1].station_id
        };
        network.add_observation(HorizontalAngleObservation {
            at: setup.station_id.clone(),
            from: from.clone(),
            to: String::from(forward_station(k)),
            angle: setup.angle,
            sigma: model.angle_sigma,
            ..Default::default()
        })?;
    }

    if traverse.kind == TraverseKind::Link
        && let Some(closing_angle) = traverse.closing_angle
    {
        // closing angle = azimuth(end->reference) - azimuth(end->last station).
        network.add_observation(AzimuthObservation {
            from: traverse.end_station_id.clone(),
            to: setups[setups.len() - 1].station_id.clone(),
            azimuth: normalize_angle(closing_angle.reference_azimuth - closing_angle.angle),
            sigma: referenced_angle_sigma,
            ..Default::default()
        })?;
    }
    Ok(network)
}

/// build_traverse_network + adjust_horizontal_network. An open traverse is
/// uniquely determined (degrees of freedom 0): it reproduces compute_traverse
/// and reports no variance factor.
pub fn adjust_traverse(
    traverse: &Traverse,
    model: &TraverseStochasticModel,
    options: &AdjustmentOptions<'_>,
) -> Result<HorizontalAdjustmentResult> {
    let network = build_traverse_network(traverse, model)?;
    adjust_horizontal_network(&network, options)
}
