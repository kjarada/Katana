//! The adjustments of reduce_and_adjust: a traverse found in the setups, or a
//! least-squares network over every observation, and, before either, the
//! resection of a setup nothing else positions, which is a small network of
//! its own. All reuse the existing code (traverse.rs, network_adjustment.rs);
//! what is here is the translation from reduced pointings into their inputs
//! and from their results into the report. Nothing here solves a least
//! squares of its own: every one goes through adjust_horizontal_network,
//! adjust_level_network or adjust_traverse.
//!
//! Structure is used to keep the least squares small: a SIDE SHOT (a point
//! observed from one setup only, occupied by none, held by nothing, and no
//! setup's backsight) has no redundancy and cannot move any other point, so
//! it is left out of the adjustment and radiated afterwards from the adjusted
//! stations. A detail survey of 20 setups and 20 000 shots is then a network
//! of 20-odd points, which the dense QR solves in milliseconds; putting the
//! side shots in would give the same coordinates at a cost cubic in their
//! number.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ops::{Add, AddAssign, Div, Mul, Sub};

use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::cpp;
use katana_math::numerics::{
    DEG_TO_RAD, HALF_PI, PI, RAD_TO_DEG, normalize_angle, normalize_angle_signed, tolerance,
};
use katana_math::vec3::Vec3;

use crate::coordinate::Coordinate2;
use crate::data_model::{
    AzimuthObservation, ControlConstraint, ControlPoint, DistanceKind, DistanceObservation, Face,
    GnssBaselineObservation, GnssPositionObservation, HorizontalAngleObservation,
    HorizontalDirectionObservation, LevelDifferenceObservation, Observation, ObservationPrecision,
    Pointing, SourceRecord, SurveyPoint, SurveyStation, Traverse, TraverseClosingAngle,
    TraverseKind, TraverseSetup, observation_kind_name, observation_source, referenced_point_views,
};
use crate::error_propagation::{
    Covariance2, ErrorEllipse, ellipse_confidence_scale, error_ellipse,
};
use crate::network::SurveyNetwork;
use crate::network_adjustment::{
    AdjustmentOptions, AdjustmentStatistics, CoordinateComponent, GlobalTest,
    HorizontalAdjustmentResult, LevelAdjustmentResult, ResidualComponent, ResidualRecord,
    ResidualSource, TraverseStochasticModel, adjust_horizontal_network, adjust_level_network,
    adjust_traverse,
};
use crate::reduction::{
    direction_rejected, distance_rejected, format_millimetres, format_number, format_seconds,
    grid_distance_from, height_rejected, mean_direction, orient_and_radiate,
};
use crate::reduction_engine::{Engine, Position, PositionOrigin, ReducedPointing};
use crate::reduction_formulas::{baarda_critical, tau_critical};
use crate::reduction_gnss::radiate_gnss_vectors;
use crate::reduction_report::{
    AdjustmentReport, ComputationMethod, MisclosureReport, ReportEllipse, ReportGlobalTest,
    ReportResidual, ResectionReport, WEAK_RESECTION_DILUTION,
};
use crate::reduction_settings::{
    GridScale, NetworkDimension, OutlierTest, ReductionSettings, TraverseRule,
};
use crate::traverse::{TraverseAdjustment, TraverseOptions, compute_traverse};

// Dense least squares costs n u^2 per iteration, and the existing solver
// (adjust_horizontal_network) forms the full cofactor matrix and the thin Q on
// every one. Measured in a Release build of the C++ (ReductionPerformance, a
// chain of setups with 17 side shots each): 400 unknowns 0.44 s, 1 000
// unknowns 6.4 s, 2 000 unknowns 63 s, all of it in the solve. Beyond this many
// the reduction refuses rather than appear to hang. Side shots do not count
// (see above).
const MAX_NETWORK_UNKNOWNS: usize = 3000;

fn is_known(position: &Position) -> bool {
    matches!(
        position.origin,
        PositionOrigin::Control | PositionOrigin::Entered | PositionOrigin::Gnss
    )
}

fn azimuth_between(from: &Position, to: &Position) -> f64 {
    normalize_angle((to.easting - from.easting).atan2(to.northing - from.northing))
}

// The mean grid distance of a setup's pointings to `target`.
fn mean_distance(engine: &Engine<'_>, setup_index: usize, target: &str) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0_usize;
    for &p in &engine.setups[setup_index].pointings {
        let pointing = &engine.pointings[p];
        if pointing.rejected || pointing.target != target {
            continue;
        }
        if let Some(distance) = pointing.grid_distance {
            sum += distance;
            count += 1;
        }
    }
    if count == 0 {
        return None;
    }
    Some(sum / count as f64)
}

// Which of a pointing's reduced values a network observation was made from:
// a rejection marks those raw rows and no others (a rejected distance does
// not make the direction of the same pointing any less used).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum PointingPart {
    #[default]
    Direction,
    Distance,
    Height,
}

fn part_rows(pointing: &ReducedPointing<'_>, part: PointingPart) -> Vec<usize> {
    match part {
        PointingPart::Direction => pointing.direction_rows[..pointing.direction_row_count].to_vec(),
        PointingPart::Distance => pointing.distance_rows[..pointing.distance_row_count].to_vec(),
        PointingPart::Height => pointing.height_row.into_iter().collect(),
    }
}

fn mark_rows(engine: &mut Engine<'_>, rows: &[usize], rejected: bool, reason: &str) {
    for &index in rows {
        let row = &mut engine.report.observations[index];
        row.rejected = rejected;
        row.rejection_reason = if rejected {
            String::from(reason)
        } else {
            String::new()
        };
    }
}

// Every point positioned by the first pass that is not held and not a station
// of `stations`: what must follow the stations when they move. Not a resected
// station: its resection placed it, from points it holds, and radiating it
// again would put it where one other setup's pointing to it (a check) says.
fn movable_points<'a>(engine: &Engine<'a>, stations: &HashSet<&'a str>) -> HashSet<&'a str> {
    engine
        .positions
        .iter()
        .filter(|(id, position)| {
            position.origin == PositionOrigin::Computed
                && !stations.contains(*id)
                && position.method != ComputationMethod::Resection
        })
        .map(|(id, _)| *id)
        .collect()
}

fn reradiate<'a>(engine: &mut Engine<'a>, targets: &HashSet<&'a str>) {
    let raw = engine.raw;
    for (s, station) in raw.stations.iter().enumerate() {
        if engine.find(&station.setup.point_id).is_some() {
            orient_and_radiate(engine, s, Some(targets));
        }
    }
    radiate_gnss_vectors(engine, Some(targets));
}

// ---- Traverse ------------------------------------------------------------------------

struct Chain<'a> {
    setups: Vec<usize>,
    // empty: open
    end: &'a str,
    closing: Option<TraverseClosingAngle>,
}

fn find_traverse<'a>(engine: &Engine<'a>) -> Option<Chain<'a>> {
    let raw = engine.raw;
    let stations = &raw.stations;
    let known_point = |id: &str| engine.find(id).is_some_and(is_known);
    let observes = |setup: usize, target: &str| {
        mean_direction(engine, setup, target).is_some()
            && mean_distance(engine, setup, target).is_some()
    };

    for (start, first) in stations.iter().enumerate() {
        if !known_point(&first.setup.point_id)
            || first.backsight_point_id.is_empty()
            || !known_point(&first.backsight_point_id)
            || mean_direction(engine, start, &first.backsight_point_id).is_none()
        {
            continue;
        }
        let mut chain = Chain {
            setups: vec![start],
            end: "",
            closing: None,
        };
        let mut used = vec![false; stations.len()];
        used[start] = true;
        loop {
            let current = *chain.setups.last().expect("a chain has its first setup");
            let here = &stations[current];
            let next = stations.iter().enumerate().position(|(j, candidate)| {
                !(used[j]
                    || candidate.backsight_point_id != here.setup.point_id
                    || candidate.setup.point_id == here.backsight_point_id
                    || known_point(&candidate.setup.point_id)
                    || !observes(current, &candidate.setup.point_id)
                    || mean_direction(engine, j, &here.setup.point_id).is_none())
            });
            let Some(next) = next else {
                break;
            };
            used[next] = true;
            chain.setups.push(next);
        }

        // The end: a known point the last setup observed forward.
        let last = *chain.setups.last().expect("a chain has its first setup");
        let last_station = &stations[last];
        for &p in &engine.setups[last].pointings {
            let pointing = &engine.pointings[p];
            if pointing.rejected
                || pointing.target == last_station.backsight_point_id
                || !known_point(pointing.target)
                || !observes(last, pointing.target)
            {
                continue;
            }
            chain.end = pointing.target;
            break;
        }
        if chain.end.is_empty() && chain.setups.len() < 2 {
            continue; // one setup and no known end: not a traverse
        }
        if !chain.end.is_empty() {
            // Closing angle: at the end, from the last station to a known mark.
            let end_position = *engine.find(chain.end).expect("the end is a known point");
            if chain.end == first.setup.point_id {
                let to_last = mean_direction(engine, start, &last_station.setup.point_id);
                let to_back = mean_direction(engine, start, &first.backsight_point_id);
                if let (Some(to_last), Some(to_back)) = (to_last, to_back) {
                    let back = *engine
                        .find(&first.backsight_point_id)
                        .expect("the backsight is a known point");
                    chain.closing = Some(TraverseClosingAngle {
                        angle: normalize_angle(to_back - to_last),
                        reference_azimuth: azimuth_between(&end_position, &back),
                    });
                }
            } else {
                for (k, closing_station) in stations.iter().enumerate() {
                    if chain.closing.is_some() {
                        break;
                    }
                    if used[k] || closing_station.setup.point_id != chain.end {
                        continue;
                    }
                    let Some(to_last) = mean_direction(engine, k, &last_station.setup.point_id)
                    else {
                        continue;
                    };
                    for &p in &engine.setups[k].pointings {
                        let pointing = &engine.pointings[p];
                        if pointing.rejected
                            || pointing.direction.is_none()
                            || pointing.target == last_station.setup.point_id
                            || !known_point(pointing.target)
                        {
                            continue;
                        }
                        // The C++ dereferences this mean unchecked; a pointing
                        // with a direction that is not rejected gives one,
                        // unless an adjustment rejected its rows.
                        let Some(to_mark) = mean_direction(engine, k, pointing.target) else {
                            break;
                        };
                        let mark = *engine
                            .find(pointing.target)
                            .expect("the mark is a known point");
                        chain.closing = Some(TraverseClosingAngle {
                            angle: normalize_angle(to_mark - to_last),
                            reference_azimuth: azimuth_between(&end_position, &mark),
                        });
                        break;
                    }
                }
            }
        }
        return Some(chain);
    }
    None
}

/// AdjustmentMethod::Traverse: finds the traverse in the setups, adjusts it by
/// the chosen rule, and re-radiates the side shots from the adjusted stations.
pub(crate) fn adjust_as_traverse(engine: &mut Engine<'_>) -> Status {
    let settings = engine.settings;
    let raw = engine.raw;
    let stations = &raw.stations;
    let Some(chain) = find_traverse(engine) else {
        engine.warn(
            "Traverse adjustment chosen, but no traverse was found: it needs a first setup on a \
             known point observing a known backsight, and each next setup backsighting the one \
             before. The coordinates are by radiation.",
            SourceRecord::default(),
        );
        return Ok(());
    };

    let first = &stations[chain.setups[0]];
    let start_position = *engine
        .find(&first.setup.point_id)
        .expect("the traverse starts on a known point");
    let back_position = *engine
        .find(&first.backsight_point_id)
        .expect("the traverse starts on a known backsight");
    let mut traverse = Traverse {
        name: format!("traverse from {}", first.setup.point_id),
        start: Coordinate2::new(start_position.northing, start_position.easting),
        start_azimuth: azimuth_between(&start_position, &back_position),
        ..Default::default()
    };

    // Legs: the occupied stations in order, each with the angle back -> forward
    // and the distance forward, meaned with the distance measured back from
    // the next setup where there is one.
    let mut forwards: Vec<&str> = Vec::new();
    for (i, &s) in chain.setups.iter().enumerate() {
        let station = &stations[s];
        let forward: &str = if i + 1 < chain.setups.len() {
            &stations[chain.setups[i + 1]].setup.point_id
        } else if !chain.end.is_empty() {
            chain.end
        } else {
            break; // an open traverse ends at the last occupied station
        };
        let back = mean_direction(engine, s, &station.backsight_point_id);
        let ahead = mean_direction(engine, s, forward);
        let distance = mean_distance(engine, s, forward);
        let (Some(back), Some(ahead), Some(mut distance)) = (back, ahead, distance) else {
            return Err(Error::new(
                ErrorCode::AdjustmentFailure,
                format!(
                    "The traverse leg {} to {forward} lacks a direction or a distance, so the \
                     traverse cannot be computed.",
                    station.setup.point_id
                ),
            ));
        };
        if i + 1 < chain.setups.len()
            && let Some(returned) =
                mean_distance(engine, chain.setups[i + 1], &station.setup.point_id)
        {
            distance = 0.5 * (distance + returned);
        }
        traverse.setups.push(TraverseSetup {
            station_id: station.setup.point_id.clone(),
            angle: normalize_angle(ahead - back),
            distance,
        });
        forwards.push(forward);
    }
    let last_station = &stations[*chain.setups.last().expect("a chain has its first setup")];
    if chain.end.is_empty() {
        traverse.kind = TraverseKind::Open;
        traverse.end_station_id = last_station.setup.point_id.clone();
        engine.warn(
            format!(
                "The traverse from {} does not close on a known point: it has no misclosure and \
                 nothing to adjust.",
                first.setup.point_id
            ),
            SourceRecord::default(),
        );
    } else {
        traverse.kind = TraverseKind::Link;
        traverse.end_station_id = String::from(chain.end);
        let end_position = *engine.find(chain.end).expect("the end is a known point");
        traverse.end = Coordinate2::new(end_position.northing, end_position.easting);
        traverse.closing_angle = chain.closing;
    }

    let method = match settings.traverse_rule {
        TraverseRule::LeastSquares => ComputationMethod::TraverseLeastSquares,
        TraverseRule::Transit => ComputationMethod::TraverseTransit,
        TraverseRule::Bowditch => ComputationMethod::TraverseBowditch,
    };
    let mut adjustment = AdjustmentReport::default();
    let mut misclosure = MisclosureReport {
        name: traverse.name.clone(),
        ..Default::default()
    };
    let mut adjusted: Vec<(String, Coordinate2)> = Vec::new();
    let mut with_precision: Vec<(String, f64, f64)> = Vec::new();

    let computed = compute_traverse(
        &traverse,
        &TraverseOptions {
            balance_angles: true,
            adjustment: if settings.traverse_rule == TraverseRule::Transit {
                TraverseAdjustment::Transit
            } else {
                TraverseAdjustment::Compass
            },
        },
    )?;
    misclosure.angular = computed.angular_misclosure;
    misclosure.length = computed.total_length;
    if let Some(linear) = &computed.linear_misclosure {
        misclosure.northing = Some(linear.latitude);
        misclosure.easting = Some(linear.departure);
        misclosure.linear = Some(linear.length);
        misclosure.precision_ratio = linear.precision_denominator;
    }
    if let Some(angular) = computed.angular_misclosure {
        // Three standard deviations of the sum of the angles: each angle is
        // the difference of two face-meaned directions.
        let angle_sigma = settings.apriori.direction;
        let allowable = 3.0 * angle_sigma * (computed.angle_count as f64).sqrt();
        misclosure.angular_allowable = Some(allowable);
        misclosure.within_tolerance = Some(angular.abs() <= allowable);
    }
    // Height misclosure: the trigonometric heights carried along the legs
    // against the known height of the end.
    {
        let end_position = if chain.end.is_empty() {
            None
        } else {
            engine.find(chain.end).copied()
        };
        if let Some(end_height) = end_position.and_then(|position| position.height)
            && let Some(start_height) = start_position.height
        {
            let mut height = start_height;
            let mut complete = true;
            for (i, forward) in forwards.iter().enumerate().take(traverse.setups.len()) {
                if !complete {
                    break;
                }
                let s = chain.setups[i];
                let mut sum = 0.0;
                let mut count = 0_usize;
                for &p in &engine.setups[s].pointings {
                    let pointing = &engine.pointings[p];
                    if pointing.rejected || pointing.target != *forward {
                        continue;
                    }
                    if let Some(height_difference) = pointing.height_difference {
                        sum += height_difference;
                        count += 1;
                    }
                }
                complete = count > 0;
                height += if count > 0 { sum / count as f64 } else { 0.0 };
            }
            if complete {
                misclosure.height = Some(height - end_height);
            }
        }
    }

    if settings.traverse_rule == TraverseRule::LeastSquares {
        let model = TraverseStochasticModel {
            distance_sigma_constant: Vec3::new(
                settings.apriori.distance_constant,
                settings.apriori.instrument_centring,
                settings.apriori.target_centring,
            )
            .length(),
            distance_sigma_ppm: settings.apriori.distance_ppm,
            // two face-meaned directions
            angle_sigma: settings.apriori.direction,
            ..Default::default()
        };
        let options = AdjustmentOptions {
            max_iterations: settings.max_iterations,
            significance_level: 1.0 - settings.confidence_level,
            ..Default::default()
        };
        let result = adjust_traverse(&traverse, &model, &options)?;
        adjustment.method = String::from("traverse, least squares");
        adjustment.unknowns = result.statistics.unknown_count;
        adjustment.observations = result.statistics.observation_count;
        adjustment.redundancy = result.statistics.degrees_of_freedom;
        adjustment.variance_factor = result.statistics.variance_factor;
        adjustment.iterations = result.iterations;
        adjustment.global_test = result.statistics.global_test.map(report_global_test);
        let scale = ellipse_confidence_scale(settings.confidence_level).unwrap_or(0.0);
        for station in &result.stations {
            adjusted.push((station.point_id.clone(), station.position));
            with_precision.push((
                station.point_id.clone(),
                station.sigma_northing,
                station.sigma_easting,
            ));
            if station.sigma_northing > 0.0 || station.sigma_easting > 0.0 {
                adjustment.ellipses.push(ReportEllipse {
                    point_id: station.point_id.clone(),
                    standard: station.ellipse,
                    confidence_scale: scale,
                });
            }
        }
    } else {
        adjustment.method = String::from(if settings.traverse_rule == TraverseRule::Transit {
            "traverse, Transit rule"
        } else {
            "traverse, Bowditch (compass) rule"
        });
        let legs = traverse.setups.len();
        let link = traverse.kind == TraverseKind::Link;
        adjustment.unknowns = 2 * (legs - usize::from(link));
        adjustment.observations = 2 * legs + usize::from(traverse.closing_angle.is_some());
        adjustment.redundancy = if link {
            if traverse.closing_angle.is_some() {
                3
            } else {
                2
            }
        } else {
            0
        };
        for station in &computed.stations {
            adjusted.push((station.id.clone(), station.adjusted));
        }
    }
    engine.report.misclosures.push(misclosure);

    // Move the traverse stations, then everything radiated from them.
    for (id, position) in &adjusted {
        let Some(existing) = engine.find_mut(id) else {
            continue;
        };
        if is_known(existing) {
            continue; // held ends
        }
        existing.northing = position.northing;
        existing.easting = position.easting;
        existing.method = method;
        for (precision_id, sigma_northing, sigma_easting) in &with_precision {
            if precision_id == id {
                existing.sigma_northing = Some(*sigma_northing);
                existing.sigma_easting = Some(*sigma_easting);
            }
        }
    }
    let station_ids: HashSet<&str> = chain
        .setups
        .iter()
        .map(|&s| stations[s].setup.point_id.as_str())
        .collect();
    let movable = movable_points(engine, &station_ids);
    reradiate(engine, &movable);
    engine.report.adjustments.push(adjustment);
    Ok(())
}

fn report_global_test(test: GlobalTest) -> ReportGlobalTest {
    ReportGlobalTest {
        statistic: test.statistic,
        lower_critical: test.lower_critical,
        upper_critical: test.upper_critical,
        significance_level: test.significance_level,
        passed: test.passed,
    }
}

// ---- Network ---------------------------------------------------------------------------

// One observation of the network, with where it came from.
#[derive(Clone, Debug)]
struct NetworkObservation {
    observation: Observation,
    label: String,
    source: SourceRecord,
    // Index into Engine::pointings of the pointing it was made from.
    pointing: Option<usize>,
    part: PointingPart,
    // For an observation that is not part of a pointing (a GNSS vector): the
    // report row a rejection marks.
    row: Option<usize>,
}

impl NetworkObservation {
    fn new(observation: impl Into<Observation>, label: String, source: SourceRecord) -> Self {
        Self {
            observation: observation.into(),
            label,
            source,
            pointing: None,
            part: PointingPart::Direction,
            row: None,
        }
    }

    fn of_pointing(
        observation: impl Into<Observation>,
        label: String,
        source: SourceRecord,
        pointing: usize,
        part: PointingPart,
    ) -> Self {
        Self {
            pointing: Some(pointing),
            part,
            ..Self::new(observation, label, source)
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct OutlierVerdict {
    statistic: Option<f64>,
    flagged: bool,
}

// ---- The one weighting of a reduced pointing -------------------------------------------
//
// The network and the resection weight a pointing from the same a-priori
// precision (ReductionSettings::apriori, already meaned over the faces in
// phase A), with the centring and the instrument and target heights, which
// depend on the line and the setup and so are added here. They differ only
// in which of those errors belong to one pointing:
//
//   Occupied  the network positions the MARK a setup stands on: the
//             instrument's centring over it and its measured height are
//             errors of each pointing, as the target's are.
//   Resected  a resection positions the INSTRUMENT, from its pointings: its
//             centring and height are the same for every pointing, so they
//             move no pointing against another and are no part of any
//             pointing's weight (treating them as independent errors of
//             each would weight a short sight as if the instrument moved
//             between pointings). They are the mark's error against the
//             instrument, added to the station's precision afterwards where
//             there is a mark (resect_setup).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StationModel {
    Occupied,
    Resected,
}

fn centring_of(apriori: &ObservationPrecision, model: StationModel) -> f64 {
    if model == StationModel::Resected {
        apriori.target_centring
    } else {
        apriori.instrument_centring.hypot(apriori.target_centring)
    }
}

// An angular value of standard deviation `sigma` across a line of `distance`
// (0 when not known, and then no centring is added): a centring error e moves
// a direction by e / distance.
fn with_centring(
    apriori: &ObservationPrecision,
    sigma: f64,
    distance: f64,
    model: StationModel,
) -> f64 {
    if distance > 0.0 {
        sigma.hypot(centring_of(apriori, model) / distance)
    } else {
        sigma
    }
}

// A reduced horizontal distance: the EDM's precision and the centring, which
// can move it by its whole amount.
fn distance_sigma_of(
    apriori: &ObservationPrecision,
    pointing: &ReducedPointing<'_>,
    model: StationModel,
) -> f64 {
    pointing.sigma_distance.hypot(centring_of(apriori, model))
}

// A trigonometric height difference, dh = S cos z + HI - HT (curvature and
// refraction are exact enough not to count): sigma^2 = (cos z sigma_S)^2 +
// (S sin z sigma_z)^2 + the heights, each measured to sigma_h: the
// instrument's and the target's for an occupied mark, the target's alone for
// a resected instrument (see StationModel). Needs the pointing's slope
// distance and zenith.
fn height_difference_sigma_of(
    apriori: &ObservationPrecision,
    pointing: &ReducedPointing<'_>,
    model: StationModel,
) -> f64 {
    let zenith = pointing.zenith.unwrap_or(0.0);
    let slope = pointing.slope.unwrap_or(0.0);
    let s2 = zenith.cos() * pointing.sigma_distance;
    let z2 = slope * zenith.sin() * pointing.sigma_zenith;
    let heights = if model == StationModel::Resected {
        1.0
    } else {
        2.0
    };
    (s2 * s2 + z2 * z2 + heights * apriori.height_measurement * apriori.height_measurement).sqrt()
}

// Whether a network observation's residual is an angle (seconds in the
// report) rather than a length.
fn is_angular(observation: &Observation) -> bool {
    matches!(
        observation,
        Observation::HorizontalAngle(_)
            | Observation::HorizontalDirection(_)
            | Observation::Azimuth(_)
    )
}

// What adjust_with_outliers reads of either adjustment's result: the C++
// template over HorizontalAdjustmentResult and LevelAdjustmentResult.
trait AdjustmentOutcome {
    fn residuals(&self) -> &[ResidualRecord];
    fn statistics(&self) -> &AdjustmentStatistics;
}

impl AdjustmentOutcome for HorizontalAdjustmentResult {
    fn residuals(&self) -> &[ResidualRecord] {
        &self.residuals
    }
    fn statistics(&self) -> &AdjustmentStatistics {
        &self.statistics
    }
}

impl AdjustmentOutcome for LevelAdjustmentResult {
    fn residuals(&self) -> &[ResidualRecord] {
        &self.residuals
    }
    fn statistics(&self) -> &AdjustmentStatistics {
        &self.statistics
    }
}

// w or tau for each residual, and whether it exceeds the critical value.
fn test_residuals(
    settings: &ReductionSettings,
    result: &impl AdjustmentOutcome,
    critical: &mut String,
) -> Vec<OutlierVerdict> {
    let residuals = result.residuals();
    let mut verdicts = vec![OutlierVerdict::default(); residuals.len()];
    if settings.outlier_test == OutlierTest::None {
        return verdicts;
    }
    let statistics = result.statistics();
    let redundancy = statistics.degrees_of_freedom;
    let mut limit = None;
    let mut scale = 1.0;
    if settings.outlier_test == OutlierTest::Baarda {
        let baarda = baarda_critical(settings.outlier_significance);
        limit = Some(baarda);
        *critical = format!("Baarda w, critical {}", format_number(baarda, 2));
    } else if let Some(factor) = statistics.variance_factor.filter(|factor| *factor > 0.0) {
        limit = tau_critical(settings.outlier_significance, redundancy);
        scale = 1.0 / factor.sqrt();
        if let Some(tau) = limit {
            *critical = format!("tau, critical {}", format_number(tau, 2));
        }
    }
    for (verdict, residual) in verdicts.iter_mut().zip(residuals) {
        let Some(standardized) = residual.standardized_residual else {
            continue;
        };
        let statistic = standardized * scale;
        verdict.statistic = Some(statistic);
        verdict.flagged = limit.is_some_and(|limit| statistic.abs() > limit);
    }
    verdicts
}

fn residual_label(
    observations: &[NetworkObservation],
    included: &[usize],
    residual: &ResidualRecord,
    network: &SurveyNetwork,
    source: &mut SourceRecord,
) -> String {
    if residual.source == ResidualSource::Control {
        let component = match residual.component {
            ResidualComponent::Northing => "northing",
            ResidualComponent::Easting => "easting",
            _ => "height",
        };
        return format!(
            "control {component} {}",
            network.control_points()[residual.index].point_id
        );
    }
    let observation = &observations[included[residual.index]];
    *source = observation.source.clone();
    match residual.component {
        ResidualComponent::Northing => format!("{} (north)", observation.label),
        ResidualComponent::Easting => format!("{} (east)", observation.label),
        _ => observation.label.clone(),
    }
}

#[derive(Clone, Debug, Default)]
struct NetworkInputs {
    points: Vec<SurveyPoint>,
    control: Vec<ControlPoint>,
    observations: Vec<NetworkObservation>,
}

fn build_network(
    inputs: &NetworkInputs,
    excluded: &[bool],
    included: &mut Vec<usize>,
) -> Result<SurveyNetwork> {
    let mut network = SurveyNetwork::default();
    for point in &inputs.points {
        network.add_point(point.clone())?;
    }
    for control in &inputs.control {
        network.add_control_point(control.clone())?;
    }
    included.clear();
    for (i, observation) in inputs.observations.iter().enumerate() {
        if excluded[i] {
            continue;
        }
        network.add_observation(observation.observation.clone())?;
        included.push(i);
    }
    Ok(network)
}

// Runs one adjustment with the outlier test and, when the settings ask, the
// rejection loop; fills `report`. `adjust` is adjust_horizontal_network or
// adjust_level_network; `subject` is what the warnings call the adjustment
// ("the network", "the resection").
fn adjust_with_outliers<R: AdjustmentOutcome>(
    engine: &mut Engine<'_>,
    inputs: &NetworkInputs,
    method: &str,
    adjust: impl Fn(&SurveyNetwork, &AdjustmentOptions<'_>) -> Result<R>,
    report: &mut AdjustmentReport,
    subject: &str,
) -> Result<R> {
    let settings = engine.settings;
    let options = AdjustmentOptions {
        max_iterations: settings.max_iterations,
        significance_level: 1.0 - settings.confidence_level,
        ..Default::default()
    };
    let mut excluded = vec![false; inputs.observations.len()];
    let mut rejected_residuals: Vec<ReportResidual> = Vec::new();
    let mut included = Vec::new();
    // The last rejection, so that one which leaves the network unsolvable (the
    // only distance to a point) can be put back.
    let mut last_rejected: Option<usize> = None;
    let mut last_rows: Vec<usize> = Vec::new();
    let mut stop_rejecting = false;
    report.method = String::from(method);

    loop {
        let network = build_network(inputs, &excluded, &mut included)?;
        let result = adjust(&network, &options);
        let result = match (result, last_rejected) {
            (Err(error), Some(index)) => {
                let put_back = report
                    .rejected_outliers
                    .pop()
                    .expect("a rejection is listed");
                engine.warn(
                    format!(
                        "{put_back} was put back: without it {subject} cannot be adjusted ({}).",
                        error.message
                    ),
                    SourceRecord::default(),
                );
                excluded[index] = false;
                mark_rows(engine, &last_rows, false, "");
                rejected_residuals.pop();
                last_rejected = None;
                stop_rejecting = true;
                continue;
            }
            (result, _) => result?,
        };
        let mut critical = String::new();
        let verdicts = test_residuals(settings, &result, &mut critical);
        let residuals = result.residuals();

        let mut worst: Option<usize> = None;
        for (i, verdict) in verdicts.iter().enumerate() {
            if !verdict.flagged || residuals[i].source != ResidualSource::Observed {
                continue;
            }
            let statistic = verdict.statistic.map_or(0.0, f64::abs);
            if worst.is_none_or(|worst| statistic > verdicts[worst].statistic.map_or(0.0, f64::abs))
            {
                worst = Some(i);
            }
        }
        if settings.auto_reject_outliers
            && !stop_rejecting
            && let Some(worst) = worst
            && result.statistics().degrees_of_freedom > 1
        {
            let residual = &residuals[worst];
            let observation_index = included[residual.index];
            let mut source = SourceRecord::default();
            let label = residual_label(
                &inputs.observations,
                &included,
                residual,
                &network,
                &mut source,
            );
            let statistic = verdicts[worst].statistic.unwrap_or(0.0);
            let rejected = ReportResidual {
                observation: label.clone(),
                source: source.clone(),
                angular: is_angular(&inputs.observations[observation_index].observation),
                residual: residual.residual,
                sigma: residual.sigma,
                redundancy_number: residual.redundancy_number,
                standardised: verdicts[worst].statistic,
                flagged: true,
                rejected: true,
            };
            let reason = format!(
                "rejected by the {critical}, value {}",
                format_number(statistic, 2)
            );
            report.rejected_outliers.push(label.clone());
            let entry = &inputs.observations[observation_index];
            last_rows = match (entry.pointing, entry.row) {
                (Some(pointing), _) => part_rows(&engine.pointings[pointing], entry.part),
                (None, Some(row)) => vec![row],
                (None, None) => Vec::new(),
            };
            mark_rows(engine, &last_rows, true, &reason);
            last_rejected = Some(observation_index);
            engine.warn(
                format!("{label} {reason}; the adjustment was run again without it."),
                source,
            );
            rejected_residuals.push(rejected);
            excluded[observation_index] = true;
            continue;
        }

        // The final run.
        let statistics = result.statistics();
        report.unknowns = statistics.unknown_count;
        report.observations = statistics.observation_count;
        report.redundancy = statistics.degrees_of_freedom;
        report.variance_factor = statistics.variance_factor;
        if let Some(test) = statistics.global_test {
            report.global_test = Some(report_global_test(test));
        }
        report.residuals.clear();
        report
            .residuals
            .reserve(residuals.len() + rejected_residuals.len());
        for (residual, verdict) in residuals.iter().zip(&verdicts) {
            let mut source = SourceRecord::default();
            let observation = residual_label(
                &inputs.observations,
                &included,
                residual,
                &network,
                &mut source,
            );
            let entry = ReportResidual {
                observation,
                source,
                angular: residual.source == ResidualSource::Observed
                    && is_angular(&inputs.observations[included[residual.index]].observation),
                residual: residual.residual,
                sigma: residual.sigma,
                redundancy_number: residual.redundancy_number,
                standardised: verdict.statistic,
                flagged: verdict.flagged,
                rejected: false,
            };
            if entry.flagged {
                report.flagged_outliers.push(entry.observation.clone());
                engine.warn(
                    format!(
                        "{} is flagged by the {critical} (value {}); it was kept - turn on \
                         automatic rejection or remove it to see {subject} without it.",
                        entry.observation,
                        format_number(verdict.statistic.unwrap_or(0.0), 2)
                    ),
                    entry.source.clone(),
                );
            }
            report.residuals.push(entry);
        }
        report.residuals.append(&mut rejected_residuals);
        return Ok(result);
    }
}

/// AdjustmentMethod::Network: least squares over every observation that is
/// not a side shot, then the side shots radiated from the adjusted stations.
pub(crate) fn adjust_as_network<'a>(engine: &mut Engine<'a>) -> Status {
    let settings = engine.settings;
    let raw = engine.raw;
    let stations = &raw.stations;
    let horizontal = settings.network_dimension != NetworkDimension::Levels;
    let levels = settings.network_dimension != NetworkDimension::Horizontal;
    let apriori = &settings.apriori;

    // ---- which points are in the network ----
    // Sets and maps here are looked up, never iterated in an order the output
    // shows (`unplaced` is sorted).
    let mut observed_from: HashMap<&'a str, usize> = HashMap::new(); // first setup
    let mut multiple: HashSet<&'a str> = HashSet::new();
    let mut in_network: HashSet<&'a str> = HashSet::new();
    for (s, station) in stations.iter().enumerate() {
        in_network.insert(&station.setup.point_id);
        if !station.backsight_point_id.is_empty() {
            in_network.insert(&station.backsight_point_id);
        }
        for &p in &engine.setups[s].pointings {
            let pointing = &engine.pointings[p];
            if pointing.rejected {
                continue;
            }
            let first = *observed_from.entry(pointing.target).or_insert(s);
            if first != s {
                multiple.insert(pointing.target);
            }
        }
    }
    for (&id, position) in &engine.positions {
        if is_known(position) {
            in_network.insert(id);
        }
    }
    in_network.extend(multiple.iter().copied());
    for angle in &engine.angles {
        in_network.insert(angle.at);
        in_network.insert(angle.to);
        if !angle.from.is_empty() {
            in_network.insert(angle.from);
        }
    }
    for observation in &raw.observations {
        // GNSS vectors take part through engine.vectors: a rover reached by
        // one vector only is a side shot like a point shot from one setup.
        if matches!(
            observation,
            Observation::GnssBaseline(_) | Observation::GnssGeocentricBaseline(_)
        ) {
            continue;
        }
        in_network.extend(referenced_point_views(observation));
    }
    for (v, vector) in engine.vectors.iter().enumerate() {
        if engine.report.observations[vector.row].rejected {
            continue;
        }
        in_network.insert(vector.from);
        // A vector is a setup of its own for the side-shot rule.
        let own = stations.len() + v;
        let first = *observed_from.entry(vector.to).or_insert(own);
        if first != own {
            in_network.insert(vector.to);
        }
    }
    let side_shots: HashSet<&'a str> = observed_from
        .keys()
        .copied()
        .filter(|id| !in_network.contains(id))
        .collect();

    // ---- points and control ----
    let mut horizontal_inputs = NetworkInputs::default();
    let mut level_inputs = NetworkInputs::default();
    let mut present: HashSet<&'a str> = HashSet::new();
    for &id in &engine.position_order {
        if !in_network.contains(id) {
            continue;
        }
        let position = &engine.positions[id];
        let point = SurveyPoint {
            id: String::from(id),
            northing: position.northing,
            easting: position.easting,
            elevation: position.height,
            ..Default::default()
        };
        horizontal_inputs.points.push(point.clone());
        level_inputs.points.push(point);
        present.insert(id);
    }
    // The level network needs no horizontal position: a point the horizontal
    // network cannot take still takes part in the heights.
    let mut unplaced: Vec<&'a str> = in_network
        .iter()
        .copied()
        .filter(|id| !present.contains(id))
        .collect();
    unplaced.sort_unstable();
    let mut level_present = present.clone();
    for &id in &unplaced {
        let mut point = SurveyPoint {
            id: String::from(id),
            ..Default::default()
        };
        if let Some(file) = engine.file_points.get(id) {
            point.northing = file.northing;
            point.easting = file.easting;
            point.elevation = file.elevation;
        }
        level_inputs.points.push(point);
        level_present.insert(id);
        if horizontal && observed_from.contains_key(id) {
            engine.warn(
                format!(
                    "Point {id} has no approximate position (none of its setups could be \
                     oriented), so it was left out of the horizontal network."
                ),
                SourceRecord::default(),
            );
        }
    }
    if horizontal_inputs.points.len() * 2 > MAX_NETWORK_UNKNOWNS {
        return Err(Error::new(
            ErrorCode::AdjustmentFailure,
            format!(
                "The network has {} points besides its side shots; this build adjusts at most {} \
                 at once. Adjust the control network on its own, or use radiation or a \
                 traverse.",
                horizontal_inputs.points.len(),
                MAX_NETWORK_UNKNOWNS / 2
            ),
        ));
    }
    for selection in &settings.control {
        let control = &selection.point;
        let id = control.point_id.as_str();
        if present.contains(id)
            && (control.northing.constraint != ControlConstraint::Free
                || control.easting.constraint != ControlConstraint::Free)
        {
            horizontal_inputs.control.push(control.clone());
        }
        if level_present.contains(id) && control.elevation.constraint != ControlConstraint::Free {
            level_inputs.control.push(control.clone());
        }
    }

    // ---- observations ----
    let label = |s: usize, what: &str, from: &str, to: &str| {
        let mut text = format!("{what} at {}: ", stations[s].setup.id);
        if !from.is_empty() {
            text.push_str(from);
            text.push_str(" -> ");
        }
        text.push_str(to);
        text
    };
    for (s, station) in stations.iter().enumerate() {
        let state = &engine.setups[s];
        let at = &station.setup.point_id;
        let horizontal_here = horizontal && state.positioned && present.contains(at.as_str());

        // The reference direction: the backsight, else the first network
        // target. What an adjustment before this one rejected (a resection's
        // outlier test) the network takes no more than it did (the report
        // says rejected), and never as the reference, whose error would be in
        // every angle measured from it.
        let mut reference: &str = "";
        if !station.backsight_point_id.is_empty()
            && present.contains(station.backsight_point_id.as_str())
            && mean_direction(engine, s, &station.backsight_point_id).is_some()
        {
            reference = &station.backsight_point_id;
        } else {
            for &p in &state.pointings {
                let pointing = &engine.pointings[p];
                if !pointing.rejected
                    && pointing.direction.is_some()
                    && present.contains(pointing.target)
                    && !direction_rejected(engine, pointing)
                {
                    reference = pointing.target;
                    break;
                }
            }
        }
        let mut reference_direction = None;
        let mut reference_sigma = 0.0;
        if !reference.is_empty() {
            reference_direction = mean_direction(engine, s, reference);
            let mut count = 0_usize;
            let mut sigma = 0.0;
            for &p in &state.pointings {
                let pointing = &engine.pointings[p];
                if !pointing.rejected
                    && pointing.target == reference
                    && pointing.direction.is_some()
                    && !direction_rejected(engine, pointing)
                {
                    sigma = pointing.sigma_direction;
                    count += 1;
                }
            }
            reference_sigma = with_centring(
                apriori,
                sigma / (count.max(1) as f64).sqrt(),
                mean_distance(engine, s, reference).unwrap_or(0.0),
                StationModel::Occupied,
            );
            if state.orientation_assumed
                && let (Some(orientation), Some(direction), true) =
                    (state.orientation, reference_direction, horizontal_here)
            {
                // The circle was set, or the file states an azimuth, on a
                // backsight with no position: that orientation is the only
                // one this setup has.
                horizontal_inputs.observations.push(NetworkObservation::new(
                    AzimuthObservation {
                        from: at.clone(),
                        to: String::from(reference),
                        azimuth: normalize_angle(direction + orientation),
                        sigma: reference_sigma,
                        source: station.source.clone(),
                    },
                    label(
                        s,
                        if state.orientation_stated {
                            "azimuth (as the file states)"
                        } else {
                            "azimuth (circle as set)"
                        },
                        "",
                        reference,
                    ),
                    station.source.clone(),
                ));
            }
        }

        for &p in &state.pointings {
            let pointing = &engine.pointings[p];
            if pointing.rejected {
                continue;
            }
            let horizontal_target = horizontal_here && present.contains(pointing.target);
            let source = pointing
                .source
                .cloned()
                .unwrap_or_else(|| station.source.clone());
            if horizontal_target
                && let (Some(direction), Some(reference_direction)) =
                    (pointing.direction, reference_direction)
                && pointing.target != reference
                && !direction_rejected(engine, pointing)
            {
                let sigma = with_centring(
                    apriori,
                    pointing.sigma_direction.hypot(reference_sigma),
                    pointing.grid_distance.unwrap_or(0.0),
                    StationModel::Occupied,
                );
                horizontal_inputs
                    .observations
                    .push(NetworkObservation::of_pointing(
                        HorizontalAngleObservation {
                            at: at.clone(),
                            from: String::from(reference),
                            to: String::from(pointing.target),
                            angle: normalize_angle(direction - reference_direction),
                            sigma,
                            source: source.clone(),
                            ..Default::default()
                        },
                        label(s, "angle", reference, pointing.target),
                        source.clone(),
                        p,
                        PointingPart::Direction,
                    ));
            }
            if horizontal_target
                && let Some(grid_distance) = pointing.grid_distance
                && !distance_rejected(engine, pointing)
            {
                horizontal_inputs
                    .observations
                    .push(NetworkObservation::of_pointing(
                        DistanceObservation {
                            from: at.clone(),
                            to: String::from(pointing.target),
                            distance: grid_distance,
                            sigma: distance_sigma_of(apriori, pointing, StationModel::Occupied),
                            kind: DistanceKind::Horizontal,
                            source: source.clone(),
                            ..Default::default()
                        },
                        label(s, "distance", "", pointing.target),
                        source.clone(),
                        p,
                        PointingPart::Distance,
                    ));
            }
            if levels
                && let Some(height_difference) = pointing.height_difference
                && pointing.slope.is_some()
                && pointing.zenith.is_some()
                && level_present.contains(at.as_str())
                && level_present.contains(pointing.target)
                && !height_rejected(engine, pointing)
            {
                level_inputs
                    .observations
                    .push(NetworkObservation::of_pointing(
                        LevelDifferenceObservation {
                            from: at.clone(),
                            to: String::from(pointing.target),
                            height_difference,
                            sigma: height_difference_sigma_of(
                                apriori,
                                pointing,
                                StationModel::Occupied,
                            ),
                            length: pointing.grid_distance.unwrap_or(0.0),
                            source: source.clone(),
                        },
                        label(s, "height difference", "", pointing.target),
                        source,
                        p,
                        PointingPart::Height,
                    ));
            }
        }
    }
    for angle in &engine.angles {
        if !horizontal
            || !present.contains(angle.at)
            || !present.contains(angle.to)
            || (!angle.from.is_empty() && !present.contains(angle.from))
        {
            continue;
        }
        let source = engine.report.observations[angle.row].source.clone();
        if angle.from.is_empty() {
            horizontal_inputs.observations.push(NetworkObservation::new(
                AzimuthObservation {
                    from: String::from(angle.at),
                    to: String::from(angle.to),
                    azimuth: angle.value,
                    sigma: angle.sigma,
                    source: source.clone(),
                },
                format!("azimuth {} -> {}", angle.at, angle.to),
                source,
            ));
        } else {
            horizontal_inputs.observations.push(NetworkObservation::new(
                HorizontalAngleObservation {
                    at: String::from(angle.at),
                    from: String::from(angle.from),
                    to: String::from(angle.to),
                    angle: angle.value,
                    sigma: angle.sigma,
                    source: source.clone(),
                    ..Default::default()
                },
                format!("angle at {}: {} -> {}", angle.at, angle.from, angle.to),
                source,
            ));
        }
    }
    for (i, observation) in raw.observations.iter().enumerate() {
        let points = referenced_point_views(observation);
        let is_level = matches!(observation, Observation::LevelDifference(_));
        let among = if is_level { &level_present } else { &present };
        if !points.iter().all(|id| among.contains(id)) {
            continue;
        }
        let source = observation_source(observation).clone();
        match observation {
            Observation::LevelDifference(level) if levels => {
                level_inputs.observations.push(NetworkObservation::new(
                    level.clone(),
                    format!("levelled height difference {} -> {}", level.from, level.to),
                    source,
                ));
            }
            Observation::LevelDifference(_) => {}
            Observation::GnssPosition(_) if horizontal => {
                horizontal_inputs.observations.push(NetworkObservation::new(
                    observation.clone(),
                    loose_label(observation, &points),
                    source,
                ));
            }
            Observation::Distance(distance)
                if horizontal && distance.kind == DistanceKind::Horizontal =>
            {
                horizontal_inputs.observations.push(NetworkObservation::new(
                    observation.clone(),
                    loose_label(observation, &points),
                    source,
                ));
            }
            Observation::GnssGlobalPosition(global) if horizontal => {
                // Converted in the seeding pass; enters as a grid position,
                // each occupation with its own value and precision. A point
                // that is control or keyed in keeps what it went in with, as
                // in radiation.
                let position = engine.find(&global.point);
                let Some(converted) = engine.global_positions[i] else {
                    continue;
                };
                if position.is_none_or(|position| position.origin != PositionOrigin::Gnss) {
                    continue;
                }
                let mut entry = NetworkObservation::new(
                    GnssPositionObservation {
                        point: global.point.clone(),
                        northing: converted.northing,
                        easting: converted.easting,
                        elevation: converted.height.unwrap_or(0.0),
                        sigma_northing: converted.sigma_horizontal,
                        sigma_easting: converted.sigma_horizontal,
                        sigma_elevation: converted.sigma_vertical,
                        source: source.clone(),
                    },
                    format!(
                        "GNSS position {}{}",
                        global.point,
                        if source.record_number != 0 {
                            format!(" (record {})", source.record_number)
                        } else {
                            String::new()
                        }
                    ),
                    source,
                );
                entry.row = Some(converted.row);
                horizontal_inputs.observations.push(entry);
            }
            _ => {}
        }
    }

    for vector in &engine.vectors {
        if engine.report.observations[vector.row].rejected {
            continue;
        }
        let source = vector.source.cloned().unwrap_or_default();
        let vector_label = format!("GNSS vector {} -> {}", vector.from, vector.to);
        if horizontal && present.contains(vector.from) && present.contains(vector.to) {
            let mut entry = NetworkObservation::new(
                GnssBaselineObservation {
                    from: String::from(vector.from),
                    to: String::from(vector.to),
                    delta_northing: vector.delta_northing,
                    delta_easting: vector.delta_easting,
                    delta_up: vector.delta_height.unwrap_or(0.0),
                    sigma_northing: vector.sigma_northing,
                    sigma_easting: vector.sigma_easting,
                    sigma_up: vector.sigma_height,
                    source: source.clone(),
                },
                vector_label.clone(),
                source.clone(),
            );
            entry.row = Some(vector.row);
            horizontal_inputs.observations.push(entry);
        }
        if levels
            && let Some(delta_height) = vector.delta_height
            && level_present.contains(vector.from)
            && level_present.contains(vector.to)
        {
            let mut entry = NetworkObservation::new(
                LevelDifferenceObservation {
                    from: String::from(vector.from),
                    to: String::from(vector.to),
                    height_difference: delta_height,
                    sigma: vector.sigma_height,
                    length: vector.delta_northing.hypot(vector.delta_easting),
                    source: source.clone(),
                },
                format!("{vector_label} (height)"),
                source,
            );
            entry.row = vector.height_row;
            level_inputs.observations.push(entry);
        }
    }

    // ---- adjust ----
    // (The C++ also collects the ids each adjustment moved, and reads none.)
    let scale = ellipse_confidence_scale(settings.confidence_level).unwrap_or(0.0);
    let mut run_horizontal = horizontal;
    let mut run_levels = levels;
    if horizontal && levels {
        // Both asked for: a half with nothing to adjust is skipped, and said so.
        if horizontal_inputs.observations.is_empty() && !level_inputs.observations.is_empty() {
            run_horizontal = false;
            engine.warn(
                "No horizontal observations to adjust; only the heights were adjusted.",
                SourceRecord::default(),
            );
        } else if level_inputs.observations.is_empty() && !horizontal_inputs.observations.is_empty()
        {
            run_levels = false;
            engine.warn(
                "No height differences to adjust; only the horizontal network was adjusted.",
                SourceRecord::default(),
            );
        }
    }
    if run_horizontal {
        let mut report = AdjustmentReport::default();
        let result = adjust_with_outliers(
            engine,
            &horizontal_inputs,
            "network least squares (horizontal)",
            adjust_horizontal_network,
            &mut report,
            "the network",
        )?;
        report.iterations = result.iterations;
        for station in &result.stations {
            let Some(position) = engine.find_mut(&station.point_id) else {
                continue;
            };
            position.northing = station.position.northing;
            position.easting = station.position.easting;
            position.sigma_northing = Some(station.sigma_northing);
            position.sigma_easting = Some(station.sigma_easting);
            if !position.held_horizontal || position.origin != PositionOrigin::Control {
                position.method = ComputationMethod::NetworkLeastSquares;
            }
            if station.sigma_northing > 0.0 || station.sigma_easting > 0.0 {
                report.ellipses.push(ReportEllipse {
                    point_id: station.point_id.clone(),
                    standard: station.ellipse,
                    confidence_scale: scale,
                });
            }
        }
        engine.report.adjustments.push(report);
    }
    if run_levels {
        let mut report = AdjustmentReport::default();
        let result = adjust_with_outliers(
            engine,
            &level_inputs,
            "network least squares (levels)",
            adjust_level_network,
            &mut report,
            "the network",
        )?;
        report.iterations = 1;
        for elevation in &result.elevations {
            if engine.find(&elevation.point_id).is_none() {
                let Some((&key, &file)) = engine
                    .file_points
                    .get_key_value(elevation.point_id.as_str())
                else {
                    engine.warn(
                        format!(
                            "Point {} was levelled to {} m but has no horizontal position, so it \
                             cannot be drawn.",
                            elevation.point_id,
                            format_number(elevation.elevation, 4)
                        ),
                        SourceRecord::default(),
                    );
                    continue;
                };
                // A levelled height on the file's horizontal position.
                engine.place(
                    key,
                    Position {
                        northing: file.northing,
                        easting: file.easting,
                        origin: PositionOrigin::Computed,
                        method: ComputationMethod::NetworkLeastSquares,
                        ..Default::default()
                    },
                );
            }
            let Some(position) = engine.find_mut(&elevation.point_id) else {
                continue;
            };
            position.height = Some(elevation.elevation);
            position.sigma_height = Some(elevation.sigma);
            if position.origin == PositionOrigin::Computed {
                position.method = ComputationMethod::NetworkLeastSquares;
            }
        }
        engine.report.adjustments.push(report);
    }

    // ---- side shots from the adjusted stations ----
    if !side_shots.is_empty() {
        reradiate(engine, &side_shots);
    }
    Ok(())
}

// "horizontal distance A -> B", "GNSS position G1": a loose observation's
// label in the network.
fn loose_label(observation: &Observation, points: &[&str]) -> String {
    let mut text = observation_kind_name(observation);
    if let Some(first) = points.first() {
        text.push(' ');
        text.push_str(first);
    }
    if points.len() > 1
        && let Some(last) = points.last()
    {
        text.push_str(" -> ");
        text.push_str(last);
    }
    text
}

// ---- Resection -------------------------------------------------------------------------
//
// A free station: a setup on a point nothing else positions (or a setup that
// names no backsight on a point only another setup's radiation positions)
// computed from its reduced pointings to points already placed, which are
// held as they are. When it is tried, and why then, is place_setups'
// (reduction.rs); what it needs, how it is solved and what refuses it, here.
//
// Needed: two placed points each observed with a direction and a horizontal
// distance, or three observed with a direction: the fewest that fix a
// position and an orientation (two directions and two distances are four
// observations of the three unknowns, north, east and the orientation; three
// directions are three). Points closer together than their centring lets the
// pointings tell apart count once. Where the file marks the observations its
// field software resected from (RESECTION_END_METADATA), only those: the
// pointings after them are checks of the station, as they were in the field.
//
// Solved: by the network adjustment itself (adjust_horizontal_network), the
// targets held, each pointing's face-meaned direction an observation of one
// set whose orientation is the third unknown, and each horizontal distance,
// with phase B's factors taken at the approximate station; weighted by the
// reduction's one policy for an instrument that is itself the unknown
// (StationModel::Resected); its last run through adjust_with_outliers, so the
// settings' outlier test flags a residual here as in a network. Then the level
// adjustment of its trigonometric height differences to the targets with
// heights.
//
// Where it starts, and whether the station has one answer. Gauss-Newton finds
// the solution nearest its start, and a resection can have two: two marks with
// distances and a third mark by direction alone give one distance and one
// angle, two circles that meet twice. Started from one construction, it can
// place such a station tens of metres from where the readings were made, with
// a precision that says millimetres, and nothing flagged: both solutions fit.
// So it starts from every point the observations give: where two of the
// station's loci meet (the circle of a distance about its target, the arc
// from which two targets are seen under the angle between their readings, the
// line through them where that angle is 0 or 180 degrees) and the rigid fit
// of the station's own frame onto the targets with distances. Each start is
// judged by the weighted squares of the residuals it leaves, the orientation
// there the weighted mean of azimuth less reading (which minimises them over
// the orientation). The least squares runs from the best, then from each other
// start beyond the linear reach (below) of the solutions found whose squares
// are within the bound below of the least found; a run that fails moves on to
// the next start. The bound is chi-square with two degrees of freedom at the
// settings' confidence level (the square of the scale the station's ellipse is
// drawn at, 5.99 at 95 %), which is where the likelihood puts the edge of the
// station's confidence region: the positions whose weighted squares exceed the
// least by less than that are the ones its observations do not exclude. The
// ellipse stands in for that region about the solution. Where a second
// solution lies within the bound but outside the best one's ellipse, the
// region has a second part the ellipse does not show: the observations do not
// tell the two apart, and the resection is refused. And a start from the best
// point, not from the first three marks, keeps three marks on the danger
// circle, read first, from refusing a station a fourth one fixes.
//
// Refused, too, where the solution's own geometry does not fix it. Exactly
// degenerate geometry of directions alone (the station on one line with every
// three targets, or on the circle through them) the closed form finds.
// Readings carry their errors, though, and then it seldom is exact: a station
// on the danger circle read to an arc second lands anywhere on the circle. So
// the solution is also judged by its own a-priori precision: the cofactor at
// the settings' weights, whatever the residuals. The least squares takes each
// observation as linear about the solution; over an offset a from it, a
// distance of length D changes by up to a^2 / 2D more than that, and a
// direction by up to a^2 / 2D^2 radians (the second-order terms of hypot and
// atan2). Where the station's ellipse at the settings' confidence level
// reaches past the offset at which that exceeds an observation's own standard
// deviation (sqrt(2 D sigma) for a distance, D sqrt(2 sigma) for a direction)
// the model the least squares solved does not hold over the region the station
// may be in: its observations do not fix it, and it is refused. That criterion
// is the least squares' own and needs no tolerance from outside. A geometry it
// passes that still magnifies the observations' errors more than
// WEAK_RESECTION_DILUTION times is placed, and flagged weak.
//
// A geometric refusal names the shape of a geometry that fixes nothing which
// the station's comes nearest (two marks at one place, the station on one
// line with its marks, or on the circle through three of them) with its
// numbers at the solution: how far apart, within what angle of one line, how
// far from that circle. It says what the geometry is; whether that fixes the
// station is the precision's to say, so no tolerance decides what is near.
//
// Rejected: a resection formula (Tienstra's, Collins') for the answer. It
// takes three directions exactly, so it neither uses a fourth nor a distance,
// nor weights, nor says how well the station is fixed; the least squares does
// all of that and is the adjustment every other coordinate here comes from.
// Rejected too: a tolerance on each degenerate shape (how near the danger
// circle, how narrow the angles, how close two marks) which would need a
// number from outside for each, where the precision judges them all alike; and
// a relative one-position rule (marks closer than their pointings resolve at
// the range count once), which would need a number of standard deviations,
// and would still not see two solutions of marks that are well apart.

// A point in the plane: the real part the easting, the imaginary the northing.
// The C++'s std::complex<double>, with its arithmetic as GCC and libgcc do it:
// a product as (ac - bd, ad + bc) with __muldc3's recovery of infinities, a
// quotient by __divdc3's scaled Smith's method, a real factor component by
// component, abs as hypot, norm as re^2 + im^2, arg as atan2(im, re).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Plane {
    re: f64,
    im: f64,
}

impl Plane {
    const I: Plane = Plane { re: 0.0, im: 1.0 };

    const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    fn real(self) -> f64 {
        self.re
    }

    fn imag(self) -> f64 {
        self.im
    }

    fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    fn norm(self) -> f64 {
        self.re * self.re + self.im * self.im
    }

    fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }

    fn conj(self) -> Plane {
        Plane::new(self.re, -self.im)
    }

    fn polar(rho: f64, theta: f64) -> Plane {
        Plane::new(rho * theta.cos(), rho * theta.sin())
    }

    fn is_finite(self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }
}

impl Add for Plane {
    type Output = Plane;
    fn add(self, other: Plane) -> Plane {
        Plane::new(self.re + other.re, self.im + other.im)
    }
}

impl AddAssign for Plane {
    fn add_assign(&mut self, other: Plane) {
        *self = *self + other;
    }
}

impl Sub for Plane {
    type Output = Plane;
    fn sub(self, other: Plane) -> Plane {
        Plane::new(self.re - other.re, self.im - other.im)
    }
}

impl Mul<f64> for Plane {
    type Output = Plane;
    fn mul(self, factor: f64) -> Plane {
        Plane::new(self.re * factor, self.im * factor)
    }
}

impl Mul<Plane> for f64 {
    type Output = Plane;
    fn mul(self, plane: Plane) -> Plane {
        Plane::new(self * plane.re, self * plane.im)
    }
}

impl Div<f64> for Plane {
    type Output = Plane;
    fn div(self, divisor: f64) -> Plane {
        Plane::new(self.re / divisor, self.im / divisor)
    }
}

fn copy_sign_of_flag(flag: bool, sign: f64) -> f64 {
    (if flag { 1.0_f64 } else { 0.0_f64 }).copysign(sign)
}

impl Mul for Plane {
    type Output = Plane;
    // libgcc's __muldc3, which GCC's inline product falls back to when both
    // parts come out NaN.
    fn mul(self, other: Plane) -> Plane {
        let (mut a, mut b, mut c, mut d) = (self.re, self.im, other.re, other.im);
        let ac = a * c;
        let bd = b * d;
        let ad = a * d;
        let bc = b * c;
        let mut x = ac - bd;
        let mut y = ad + bc;
        if x.is_nan() && y.is_nan() {
            // Recover infinities that computed as NaN + iNaN.
            let mut recalc = false;
            if a.is_infinite() || b.is_infinite() {
                a = copy_sign_of_flag(a.is_infinite(), a);
                b = copy_sign_of_flag(b.is_infinite(), b);
                if c.is_nan() {
                    c = 0.0_f64.copysign(c);
                }
                if d.is_nan() {
                    d = 0.0_f64.copysign(d);
                }
                recalc = true;
            }
            if c.is_infinite() || d.is_infinite() {
                c = copy_sign_of_flag(c.is_infinite(), c);
                d = copy_sign_of_flag(d.is_infinite(), d);
                if a.is_nan() {
                    a = 0.0_f64.copysign(a);
                }
                if b.is_nan() {
                    b = 0.0_f64.copysign(b);
                }
                recalc = true;
            }
            if !recalc
                && (ac.is_infinite() || bd.is_infinite() || ad.is_infinite() || bc.is_infinite())
            {
                // Recover infinities from overflow by changing NaNs to 0.
                if a.is_nan() {
                    a = 0.0_f64.copysign(a);
                }
                if b.is_nan() {
                    b = 0.0_f64.copysign(b);
                }
                if c.is_nan() {
                    c = 0.0_f64.copysign(c);
                }
                if d.is_nan() {
                    d = 0.0_f64.copysign(d);
                }
                recalc = true;
            }
            if recalc {
                x = f64::INFINITY * (a * c - b * d);
                y = f64::INFINITY * (a * d + b * c);
            }
        }
        Plane::new(x, y)
    }
}

impl Div for Plane {
    type Output = Plane;
    // libgcc's __divdc3 (GCC 12 and later): Smith's method, scaled away from
    // overflow and underflow, then the C99 recovery of infinities and zeros.
    fn div(self, other: Plane) -> Plane {
        const RBIG: f64 = f64::MAX / 2.0;
        const RMIN: f64 = f64::MIN_POSITIVE;
        const RMIN2: f64 = f64::EPSILON;
        const RMINSCAL: f64 = 1.0 / f64::EPSILON;
        const RMAX2: f64 = RBIG * RMIN2;
        let (mut a, mut b, mut c, mut d) = (self.re, self.im, other.re, other.im);
        let x;
        let y;
        if c.abs() < d.abs() {
            // Prevent underflow when denominator is near max representable.
            if d.abs() >= RBIG {
                a /= 2.0;
                b /= 2.0;
                c /= 2.0;
                d /= 2.0;
            }
            // Avoid overflow/underflow issues when c and d are small.
            if d.abs() < RMIN2
                || (a.abs() < RMIN && b.abs() < RMAX2 && d.abs() < RMAX2)
                || (b.abs() < RMIN && a.abs() < RMAX2 && d.abs() < RMAX2)
            {
                a *= RMINSCAL;
                b *= RMINSCAL;
                c *= RMINSCAL;
                d *= RMINSCAL;
            }
            let ratio = c / d;
            let denom = (c * ratio) + d;
            // Choose alternate order of computation if ratio is subnormal.
            if ratio.abs() > RMIN {
                x = ((a * ratio) + b) / denom;
                y = ((b * ratio) - a) / denom;
            } else {
                x = ((c * (a / d)) + b) / denom;
                y = ((c * (b / d)) - a) / denom;
            }
        } else {
            if c.abs() >= RBIG {
                a /= 2.0;
                b /= 2.0;
                c /= 2.0;
                d /= 2.0;
            }
            if c.abs() < RMIN2
                || (a.abs() < RMIN && b.abs() < RMAX2 && c.abs() < RMAX2)
                || (b.abs() < RMIN && a.abs() < RMAX2 && c.abs() < RMAX2)
            {
                a *= RMINSCAL;
                b *= RMINSCAL;
                c *= RMINSCAL;
                d *= RMINSCAL;
            }
            let ratio = d / c;
            let denom = (d * ratio) + c;
            if ratio.abs() > RMIN {
                x = ((b * ratio) + a) / denom;
                y = (b - (a * ratio)) / denom;
            } else {
                x = (a + (d * (b / c))) / denom;
                y = (b - (d * (a / c))) / denom;
            }
        }
        let (mut x, mut y) = (x, y);
        // Recover infinities and zeros that computed as NaN+iNaN; the only
        // cases are nonzero/zero, infinite/finite, and finite/infinite.
        if x.is_nan() && y.is_nan() {
            if c == 0.0 && d == 0.0 && (!a.is_nan() || !b.is_nan()) {
                x = f64::INFINITY.copysign(c) * a;
                y = f64::INFINITY.copysign(c) * b;
            } else if (a.is_infinite() || b.is_infinite()) && c.is_finite() && d.is_finite() {
                let a = copy_sign_of_flag(a.is_infinite(), a);
                let b = copy_sign_of_flag(b.is_infinite(), b);
                x = f64::INFINITY * (a * c + b * d);
                y = f64::INFINITY * (b * c - a * d);
            } else if (c.is_infinite() || d.is_infinite()) && a.is_finite() && b.is_finite() {
                let c = copy_sign_of_flag(c.is_infinite(), c);
                let d = copy_sign_of_flag(d.is_infinite(), d);
                x = 0.0 * (a * c + b * d);
                y = 0.0 * (b * c - a * d);
            }
        }
        Plane::new(x, y)
    }
}

fn plane_of(position: &Position) -> Plane {
    Plane::new(position.easting, position.northing)
}

// "5.0 mm" below a metre, "12.345 m" from there.
fn length_words(metres: f64) -> String {
    if metres < 1.0 {
        format_millimetres(metres, 1)
    } else {
        format!("{} m", format_number(metres, 3))
    }
}

// "2.1\"" below a degree, "12.3 deg" from there.
fn angle_words(radians: f64) -> String {
    if radians < DEG_TO_RAD {
        format_seconds(radians, 1)
    } else {
        format!("{} deg", format_number(radians * RAD_TO_DEG, 1))
    }
}

// A setup's pointings to one placed point, meaned. The C++ points at the
// target's position; it is a copy here, which nothing changes while the
// resection runs.
#[derive(Clone, Copy, Debug)]
struct ResectionTarget<'a> {
    id: &'a str,
    position: Position,
    // mean circle reading
    direction: Option<f64>,
    // mean horizontal distance, on the ground (phase A)
    distance: Option<f64>,
}

// The placed points a setup observes in its resection block, in the order it
// first observes each, and the pointings to them it can use.
struct ResectionData<'a> {
    targets: Vec<ResectionTarget<'a>>,
    pointings: Vec<usize>,
    // Pointings after the block to one of the targets: checks of the station.
    checks: usize,
    // The first target whose pointing or position holds a value that is not finite.
    not_finite: String,
}

fn resection_data<'a>(engine: &Engine<'a>, setup_index: usize) -> ResectionData<'a> {
    let raw = engine.raw;
    let at = raw.stations[setup_index].setup.point_id.as_str();
    let finite = |value: Option<f64>| value.is_none_or(f64::is_finite);
    let mut data = ResectionData {
        targets: Vec::new(),
        pointings: Vec::new(),
        checks: 0,
        not_finite: String::new(),
    };
    let mut slot: HashMap<&'a str, usize> = HashMap::new();
    // Per target, the readings meaned as mean_direction means them (wrapped
    // about the first) and the distances; over the block's pointings only.
    let mut first_readings: Vec<f64> = Vec::new();
    let mut reading_offsets: Vec<f64> = Vec::new();
    let mut reading_counts: Vec<usize> = Vec::new();
    let mut distance_sums: Vec<f64> = Vec::new();
    let mut distance_counts: Vec<usize> = Vec::new();
    let mut later: Vec<usize> = Vec::new();
    for &p in &engine.setups[setup_index].pointings {
        let pointing = &engine.pointings[p];
        if pointing.rejected
            || pointing.target == at
            || (pointing.direction.is_none() && pointing.horizontal.is_none())
        {
            continue;
        }
        let Some(position) = engine.find(pointing.target).copied() else {
            continue;
        };
        if !in_resection_block(engine, setup_index, pointing) {
            later.push(p);
            continue;
        }
        if data.not_finite.is_empty()
            && (!finite(pointing.direction)
                || !finite(pointing.horizontal)
                || !finite(pointing.height_difference)
                || !position.northing.is_finite()
                || !position.easting.is_finite()
                || !finite(position.height))
        {
            data.not_finite = String::from(pointing.target);
        }
        let k = *slot.entry(pointing.target).or_insert_with(|| {
            data.targets.push(ResectionTarget {
                id: pointing.target,
                position,
                direction: None,
                distance: None,
            });
            first_readings.push(0.0);
            reading_offsets.push(0.0);
            reading_counts.push(0);
            distance_sums.push(0.0);
            distance_counts.push(0);
            data.targets.len() - 1
        });
        if let Some(direction) = pointing.direction {
            if reading_counts[k] == 0 {
                first_readings[k] = direction;
            }
            reading_offsets[k] += normalize_angle_signed(direction - first_readings[k]);
            reading_counts[k] += 1;
        }
        if let Some(horizontal) = pointing.horizontal {
            distance_sums[k] += horizontal;
            distance_counts[k] += 1;
        }
        data.pointings.push(p);
    }
    for (k, target) in data.targets.iter_mut().enumerate() {
        if reading_counts[k] > 0 {
            target.direction = Some(normalize_angle(
                first_readings[k] + reading_offsets[k] / reading_counts[k] as f64,
            ));
        }
        if distance_counts[k] > 0 {
            target.distance = Some(distance_sums[k] / distance_counts[k] as f64);
        }
    }
    for &p in &later {
        data.checks += usize::from(slot.contains_key(engine.pointings[p].target));
    }
    data
}

// Two marks closer than this are one position to a resection: the standard
// deviation of the difference of two targets, each centred to the settings'
// target centring, which no pointing to them can resolve, and never less than
// the project's coordinate tolerance.
fn one_position_of(apriori: &ObservationPrecision) -> f64 {
    cpp::max(
        tolerance::COORDINATE,
        2.0_f64.sqrt() * apriori.target_centring,
    )
}

// The targets at distinct positions: a point within `one_position` of an
// earlier one adds nothing to the geometry, so it is not counted again.
struct DistinctTargets<'a> {
    // a direction and a distance
    with_both: Vec<ResectionTarget<'a>>,
    // a direction, with or without
    with_direction: Vec<ResectionTarget<'a>>,
    // the first two that share a position, "A and B"
    coincident: String,
}

fn distinct_targets<'a>(targets: &[ResectionTarget<'a>], one_position: f64) -> DistinctTargets<'a> {
    struct Place {
        first: usize,
        both: bool,
        direction: bool,
    }
    let mut distinct = DistinctTargets {
        with_both: Vec::new(),
        with_direction: Vec::new(),
        coincident: String::new(),
    };
    let mut places: Vec<Place> = Vec::new();
    for (t, target) in targets.iter().enumerate() {
        let place = places
            .iter()
            .position(|place| {
                (plane_of(&targets[place.first].position) - plane_of(&target.position)).abs()
                    <= one_position
            })
            .unwrap_or(places.len());
        if place == places.len() {
            places.push(Place {
                first: t,
                both: false,
                direction: false,
            });
        } else if distinct.coincident.is_empty() {
            distinct.coincident = format!("{} and {}", targets[places[place].first].id, target.id);
        }
        if target.direction.is_some() && target.distance.is_some() && !places[place].both {
            places[place].both = true;
            distinct.with_both.push(*target);
        }
        if target.direction.is_some() && !places[place].direction {
            places[place].direction = true;
            distinct.with_direction.push(*target);
        }
    }
    distinct
}

fn names_of(targets: &[ResectionTarget<'_>]) -> String {
    let mut names = String::new();
    for (i, target) in targets.iter().enumerate() {
        names.push_str(if i == 0 {
            ""
        } else if i + 1 == targets.len() {
            " and "
        } else {
            ", "
        });
        names.push_str(target.id);
    }
    names
}

// Too few placed points, as a clause.
fn too_few(distinct: &DistinctTargets<'_>) -> String {
    let directions = distinct.with_direction.len();
    let mut clause = String::from("it observes ");
    if directions == 0 {
        clause.push_str("no placed point with a direction");
    } else {
        clause.push_str(&format!(
            "{directions} placed point{} with a direction ({}), {} of them with a distance as well",
            if directions == 1 { "" } else { "s" },
            names_of(&distinct.with_direction),
            distinct.with_both.len()
        ));
    }
    clause.push_str(
        ", where a resection needs two placed points observed with a direction and a distance, \
         or three observed with a direction",
    );
    if !distinct.coincident.is_empty() {
        clause.push_str(&format!(
            " ({} stand on one position, so count as one)",
            distinct.coincident
        ));
    }
    clause
}

// The station from targets observed with a direction and a distance: its own
// frame (the circle's zero as north, itself at the origin, each target at its
// reading and distance) turned and moved onto the targets by the rigid fit of
// least squares (a Helmert fit without scale). With local l and world w
// reduced to their centroids (primed), the turn that fits them is
// arg(sum conj(w') l'), a turn of the frame by e^(-i turn), and the station is
// the world centroid less the turned local one. Absent where the frame puts
// the targets at one place.
fn fit_station(targets: &[ResectionTarget<'_>]) -> Option<Plane> {
    let mut local = Vec::with_capacity(targets.len());
    let mut world = Vec::with_capacity(targets.len());
    let mut local_mean = Plane::default();
    let mut world_mean = Plane::default();
    for target in targets {
        // Reading d from north, clockwise: (D sin d, D cos d) = D e^(i(pi/2 - d)).
        let l = Plane::polar(
            target.distance.unwrap_or(0.0),
            HALF_PI - target.direction.unwrap_or(0.0),
        );
        let w = plane_of(&target.position);
        local.push(l);
        world.push(w);
        local_mean += l;
        world_mean += w;
    }
    let count = targets.len() as f64;
    local_mean = local_mean / count;
    world_mean = world_mean / count;
    let mut fit = Plane::default();
    let mut size = 0.0;
    for (l, w) in local.iter().zip(&world) {
        let l = *l - local_mean;
        let w = *w - world_mean;
        fit += w.conj() * l;
        size += l.norm() + w.norm();
    }
    if fit.abs() <= tolerance::RELATIVE * size {
        return None;
    }
    Some(world_mean - local_mean * Plane::polar(1.0, -fit.arg()))
}

// The centre of the circle on which a point sees `from` and `to` under the
// clockwise angle `angle` from the one to the other (the inscribed angle
// theorem): on the chord's perpendicular bisector, (chord / 2) cot(angle) from
// its middle.
fn inscribed_centre(from: Plane, to: Plane, angle: f64) -> Plane {
    0.5_f64 * (from + to) - Plane::I * (0.5_f64 * (to - from)) * (angle.cos() / angle.sin())
}

struct ThreeDirections {
    station: Option<Plane>,
    why: String,
}

// The station from three targets by direction alone. It is on each circle
// through two of them that sees them under the angle between their readings;
// two such circles share a target and meet again at the station, which is that
// target mirrored in the line through their centres. Refused where two of the
// three angles are exactly 0 or 180 degrees (the station on one line with the
// targets) and where the two circles are exactly one (the station on the
// circle through all three: the danger circle, where any point of it sees them
// alike). Used only to find directions alone exactly that degenerate: the
// least squares' starts come from the loci (resect_setup), and near those
// shapes its precision judges the solution.
fn three_directions(
    a: &ResectionTarget<'_>,
    b: &ResectionTarget<'_>,
    c: &ResectionTarget<'_>,
) -> ThreeDirections {
    struct Chord {
        from: usize,
        to: usize,
        angle: f64,
    }
    let targets = [a, b, c];
    let direction = |target: &ResectionTarget<'_>| target.direction.unwrap_or(0.0);
    let mut chords = [
        Chord {
            from: 0,
            to: 1,
            angle: direction(b) - direction(a),
        },
        Chord {
            from: 1,
            to: 2,
            angle: direction(c) - direction(b),
        },
        Chord {
            from: 2,
            to: 0,
            angle: direction(a) - direction(c),
        },
    ];
    // std::stable_sort by |sin| descending.
    chords.sort_by(|x, y| {
        y.angle
            .sin()
            .abs()
            .partial_cmp(&x.angle.sin().abs())
            .unwrap_or(Ordering::Equal)
    });
    let names = format!("{}, {} and {}", a.id, b.id, c.id);
    if chords[1].angle.sin().abs() <= tolerance::ANGULAR {
        return ThreeDirections {
            station: None,
            why: format!("it stands on one line with {names}, where directions cannot fix it"),
        };
    }
    let first = &chords[0];
    let second = &chords[1];
    let shared = if first.from == second.from || first.from == second.to {
        first.from
    } else {
        first.to
    };
    let centre1 = inscribed_centre(
        plane_of(&targets[first.from].position),
        plane_of(&targets[first.to].position),
        first.angle,
    );
    let centre2 = inscribed_centre(
        plane_of(&targets[second.from].position),
        plane_of(&targets[second.to].position),
        second.angle,
    );
    let axis = centre2 - centre1;
    let to_shared = plane_of(&targets[shared].position) - centre1;
    if axis.abs() <= tolerance::RELATIVE * to_shared.abs() {
        return ThreeDirections {
            station: None,
            why: format!(
                "it stands on the circle through {names} (the danger circle), where directions \
                 to them cannot fix it"
            ),
        };
    }
    ThreeDirections {
        station: Some(centre1 + axis / axis.conj() * to_shared.conj()),
        why: String::new(),
    }
}

// The circle through three points, absent where they are on one line.
fn circle_through(a: Plane, b: Plane, c: Plane) -> Option<(Plane, f64)> {
    // About a, so that grid-sized coordinates keep their digits.
    let u = b - a;
    let v = c - a;
    let twice = 2.0 * (u.real() * v.imag() - u.imag() * v.real());
    if !(twice.abs() > tolerance::RELATIVE * cpp::max(u.norm(), v.norm())) {
        return None;
    }
    let centre = Plane::new(
        (v.imag() * u.norm() - u.imag() * v.norm()) / twice,
        (u.real() * v.norm() - v.real() * u.norm()) / twice,
    );
    Some((a + centre, centre.abs()))
}

// Where the station is by one or two of its observations: a circle (of a
// distance about its target, or of the arc from which two targets are seen
// under the angle between their readings, inscribed_centre) or, where that
// angle is 0 or 180 degrees, the line through the two targets.
#[derive(Clone, Copy, Debug, Default)]
struct Locus {
    // a circle's centre, or a point of the line
    point: Plane,
    radius: f64,
    // the line's unit direction; zero for a circle
    along: Plane,
}

// The cross product of two vectors of the plane (its one component): zero for
// parallel ones.
fn cross(a: Plane, b: Plane) -> f64 {
    a.real() * b.imag() - a.imag() * b.real()
}

// Where two loci meet, added to `points`: none, one or two points. Two that
// miss each other (read with errors, where the true ones touch or nearly do)
// give one point instead, on the line of their centres (the foot of the
// radical line for two circles, of the centre for a circle and a line), which
// for two that nearly touch is where they would: a start near the station,
// which the least squares then solves.
fn meet(a: &Locus, b: &Locus, points: &mut Vec<Plane>) {
    let line_a = a.along != Plane::default();
    let line_b = b.along != Plane::default();
    if line_a && line_b {
        let turn = cross(a.along, b.along);
        if turn != 0.0 {
            points.push(a.point + a.along * (cross(b.point - a.point, b.along) / turn));
        }
        return;
    }
    if line_a || line_b {
        let (line, circle) = if line_a { (a, b) } else { (b, a) };
        let foot =
            line.point + line.along * (line.along.conj() * (circle.point - line.point)).real();
        let offset = (circle.point - foot).abs();
        if offset <= circle.radius {
            let half = line.along * (circle.radius * circle.radius - offset * offset).sqrt();
            points.push(foot + half);
            points.push(foot - half);
        } else {
            points.push(foot);
        }
        return;
    }
    let between = b.point - a.point;
    let apart = between.abs();
    if apart == 0.0 {
        return; // one centre: the circles meet everywhere or nowhere
    }
    let unit = between / apart;
    let along = (a.radius * a.radius - b.radius * b.radius + apart * apart) / (2.0 * apart);
    let across = a.radius * a.radius - along * along;
    let foot = a.point + unit * along;
    if across >= 0.0 {
        let half = Plane::I * unit * across.sqrt();
        points.push(foot + half);
        points.push(foot - half);
    } else {
        points.push(foot);
    }
}

// One horizontal observation of the resection as a start is judged by it, in
// the order its least squares takes them (resect_setup), so that one mask of
// the observations its outlier test rejected serves both.
#[derive(Clone, Copy, Debug)]
struct Reading {
    target: Plane,
    direction: bool,
    // the circle reading, or the grid distance
    value: f64,
    // a distance's; a direction's before its target's centring
    sigma: f64,
    // a direction's measured sight, over which that centring acts
    sight: Option<f64>,
}

// The weighted squares of the residuals a station at `station` leaves, at the
// orientation there that makes them least (the weighted mean of azimuth less
// reading) with the weights its least squares gives them. The readings
// `excluded` marks take no part.
fn squares_at(
    station: Plane,
    readings: &[Reading],
    excluded: &[bool],
    apriori: &ObservationPrecision,
) -> f64 {
    let mut sigmas = vec![0.0; readings.len()];
    let mut first: Option<f64> = None;
    let mut offsets = 0.0;
    let mut weights = 0.0;
    for (i, reading) in readings.iter().enumerate() {
        if excluded[i] || !reading.direction {
            continue;
        }
        let line = reading.target - station;
        sigmas[i] = with_centring(
            apriori,
            reading.sigma,
            reading.sight.unwrap_or(line.abs()),
            StationModel::Resected,
        );
        let value = normalize_angle(line.real().atan2(line.imag()) - reading.value);
        let reference = *first.get_or_insert(value);
        let weight = 1.0 / (sigmas[i] * sigmas[i]);
        offsets += weight * normalize_angle_signed(value - reference);
        weights += weight;
    }
    let orientation = first.map_or(0.0, |first| first + offsets / weights);
    let mut squares = 0.0;
    for (i, reading) in readings.iter().enumerate() {
        if excluded[i] {
            continue;
        }
        let line = reading.target - station;
        let standardised = if reading.direction {
            normalize_angle_signed(line.real().atan2(line.imag()) - reading.value - orientation)
                / sigmas[i]
        } else {
            (line.abs() - reading.value) / reading.sigma
        };
        squares += standardised * standardised;
    }
    squares
}

// The squared Mahalanobis distance of `offset` from a station whose
// coordinates have the cofactor `q`: above chi-square(2) at a confidence
// level, the offset lies outside the station's ellipse at that level.
fn mahalanobis2(offset: Plane, q: &Covariance2) -> f64 {
    let determinant = q.northing * q.easting - q.northing_easting * q.northing_easting;
    if !(determinant > 0.0) {
        return f64::INFINITY;
    }
    let north = offset.imag();
    let east = offset.real();
    (q.easting * north * north - 2.0 * q.northing_easting * north * east + q.northing * east * east)
        / determinant
}

// A shape of the geometries that fix no station, and how near one comes to it.
struct Shape {
    measure: f64,
    clause: String,
}

// Of the shapes that fix no station, the one a station at `station` comes
// nearest, as a clause with its numbers: two targets at one place (their
// separation over their distance from it), the station on one line with three
// or more targets (the largest sine of an angle between two sight lines, which
// one line through the station makes zero), or on the circle through three of
// them (its distance from that circle over the radius). Each measure is zero
// for its shape exactly; the least is named. What it names is a fact of the
// geometry; the refusal it goes with says why that does not fix the station.
#[allow(
    clippy::needless_range_loop,
    reason = "Every pair and triple of the targets, by index, as the C++ walks them."
)]
fn nearest_shape(targets: &[ResectionTarget<'_>], station: Plane) -> Shape {
    let mut nearest = Shape {
        measure: f64::INFINITY,
        clause: String::new(),
    };
    let mut consider = |measure: f64, clause: String| {
        if measure.is_finite() && measure < nearest.measure {
            nearest = Shape { measure, clause };
        }
    };
    let count = targets.len();
    for i in 0..count {
        for j in i + 1..count {
            let a = plane_of(&targets[i].position);
            let b = plane_of(&targets[j].position);
            let apart = (a - b).abs();
            // How far they are: as measured, else from the station.
            let reach = cpp::max(
                targets[i].distance.unwrap_or((a - station).abs()),
                targets[j].distance.unwrap_or((b - station).abs()),
            );
            if reach > 0.0 {
                consider(
                    apart / reach,
                    format!(
                        "{} and {} are {} apart, {} from it",
                        targets[i].id,
                        targets[j].id,
                        length_words(apart),
                        length_words(reach)
                    ),
                );
            }
        }
    }
    if count < 3 {
        return nearest;
    }
    let mut widest = 0.0;
    for i in 0..count {
        for j in i + 1..count {
            let sine = (targets[j].direction.unwrap_or(0.0) - targets[i].direction.unwrap_or(0.0))
                .sin()
                .abs();
            widest = cpp::max(widest, sine);
        }
    }
    consider(
        widest,
        format!(
            "its sight lines to {} are within {} of one line",
            names_of(targets),
            angle_words(cpp::min(widest, 1.0).asin())
        ),
    );
    for i in 0..count {
        for j in i + 1..count {
            for k in j + 1..count {
                let Some((centre, radius)) = circle_through(
                    plane_of(&targets[i].position),
                    plane_of(&targets[j].position),
                    plane_of(&targets[k].position),
                ) else {
                    continue;
                };
                let offset = ((station - centre).abs() - radius).abs();
                consider(
                    offset / radius,
                    format!(
                        "it stands {} from the circle through {} (the danger circle, radius {})",
                        length_words(offset),
                        names_of(&[targets[i], targets[j], targets[k]]),
                        length_words(radius)
                    ),
                );
            }
        }
    }
    nearest
}

// One observation of the resection, as its strength needs it.
#[derive(Clone, Copy, Debug)]
struct Sight {
    target: Plane,
    // radians for a direction, metres for a distance
    sigma: f64,
    direction: bool,
}

// How well the least squares fixed the station, from the geometry and the
// observations' a-priori precision alone (see the section's head).
#[derive(Clone, Copy, Debug, Default)]
struct StationStrength {
    // of the station's northing and easting, at the a-priori weights
    cofactor: Covariance2,
    // its standard ellipse
    apriori: ErrorEllipse,
    // its semi-major axis at the settings' confidence level
    confidence: f64,
    // the offset within which the linear model holds
    linear_within: f64,
    // the least precise single observation's line of position
    worst_line: f64,
}

impl StationStrength {
    fn fixes(&self) -> bool {
        self.confidence.is_finite() && self.confidence <= self.linear_within
    }

    fn dilution(&self) -> f64 {
        self.apriori.semi_major / self.worst_line
    }
}

fn station_strength(
    result: &HorizontalAdjustmentResult,
    at: &str,
    station: Plane,
    sights: &[Sight],
    confidence_scale: f64,
) -> Option<StationStrength> {
    let mut north = None;
    let mut east = None;
    for (k, parameter) in result.parameters.iter().enumerate() {
        if parameter.point_id != at {
            continue;
        }
        if parameter.component == CoordinateComponent::Northing {
            north = Some(k);
        } else if parameter.component == CoordinateComponent::Easting {
            east = Some(k);
        }
    }
    let (Some(north), Some(east)) = (north, east) else {
        return None;
    };
    if sights.is_empty() {
        return None;
    }
    let cofactor = Covariance2 {
        northing: result.cofactor[(north, north)],
        easting: result.cofactor[(east, east)],
        northing_easting: result.cofactor[(north, east)],
    };
    let ellipse = error_ellipse(cofactor).ok()?;
    let mut strength = StationStrength {
        cofactor,
        apriori: ellipse,
        confidence: ellipse.semi_major * confidence_scale,
        linear_within: f64::INFINITY,
        worst_line: 0.0,
    };
    for sight in sights {
        let length = (sight.target - station).abs();
        if sight.direction {
            strength.linear_within =
                cpp::min(strength.linear_within, length * (2.0 * sight.sigma).sqrt());
            strength.worst_line = cpp::max(strength.worst_line, length * sight.sigma);
        } else {
            strength.linear_within =
                cpp::min(strength.linear_within, (2.0 * length * sight.sigma).sqrt());
            strength.worst_line = cpp::max(strength.worst_line, sight.sigma);
        }
    }
    Some(strength)
}

// A solution of the resection's least squares: where it put the station, the
// weighted squares of its residuals, and how well it fixes the station.
#[derive(Clone, Copy, Debug)]
struct Solution {
    position: Plane,
    squares: f64,
    strength: StationStrength,
}

// The one of `found` with the least squares: std::min_element, the first of
// equals.
fn least_of(found: &[Solution]) -> usize {
    let mut least = 0;
    for (i, solution) in found.iter().enumerate().skip(1) {
        if solution.squares < found[least].squares {
            least = i;
        }
    }
    least
}

// Another solution of `found` its observations do not tell from `best`: its
// squares within `bound` of best's and it outside best's ellipse at the
// confidence level whose chi-square(2) is `bound`. The nearest in squares, or
// none.
fn rival_of(found: &[Solution], best: usize, bound: f64) -> Option<usize> {
    let best_solution = &found[best];
    let mut rival: Option<usize> = None;
    for (i, other) in found.iter().enumerate() {
        if i == best
            || other.squares > best_solution.squares + bound
            || mahalanobis2(
                other.position - best_solution.position,
                &best_solution.strength.cofactor,
            ) <= bound
        {
            continue;
        }
        if rival.is_none_or(|rival| other.squares < found[rival].squares) {
            rival = Some(i);
        }
    }
    rival
}

// "12 deg": the azimuth of an ellipse's semi-major axis, in whole degrees.
fn axis_words(ellipse: &ErrorEllipse) -> String {
    format!("{} deg", format_number(ellipse.orientation * 180.0 / PI, 0))
}

// "N 4935.721 E 5076.604": a position, to the millimetre.
fn position_words(position: Plane) -> String {
    format!(
        "N {} E {}",
        format_number(position.imag(), 3),
        format_number(position.real(), 3)
    )
}

// The precision a least squares gives, as a factor on its a-priori cofactor:
// the variance factor where its global test finds the residuals larger than
// the a-priori weights allow (above the test's upper bound), else 1. On the
// few degrees of freedom of a resection the variance factor is itself
// uncertain (a chi-square on r has a relative standard deviation of
// sqrt(2 / r), 141 % on one, 82 % on three) so one the test accepts says no
// more than the weights did, and scaling by it would shrink or swell the
// precision by chance (to nanometres where exact readings fit exactly); one
// above the bound says the weights were too optimistic.
fn precision_scale_of(statistics: &AdjustmentStatistics) -> f64 {
    match (statistics.global_test, statistics.variance_factor) {
        (Some(test), Some(factor)) if test.statistic > test.upper_critical => factor,
        _ => 1.0,
    }
}

/// Whether `pointing` of setup `setup_index` may enter the setup's resection:
/// every pointing, unless the file marks where the observations its field
/// software resected from end (SetupState::resection_block_end); then those
/// read before that record. The ones after are checks of the station.
pub(crate) fn in_resection_block(
    engine: &Engine<'_>,
    setup_index: usize,
    pointing: &ReducedPointing<'_>,
) -> bool {
    let Some(end) = engine.setups[setup_index].resection_block_end else {
        return true;
    };
    pointing
        .source
        .is_none_or(|source| source.record_number == 0 || source.record_number < end)
}

/// Why a setup whose station has no position was not a resection candidate,
/// as a clause (the placed points it observes against what a resection needs)
/// or empty when it observes none.
pub(crate) fn resection_shortfall(engine: &Engine<'_>, setup_index: usize) -> String {
    let data = resection_data(engine, setup_index);
    if data.targets.is_empty() {
        return String::new();
    }
    let distinct = distinct_targets(&data.targets, one_position_of(&engine.settings.apriori));
    if distinct.with_both.len() >= 2 || distinct.with_direction.len() >= 3 {
        return String::new(); // enough: a refusal would have said why not
    }
    too_few(&distinct)
}

// The network of a resection's least squares started at a station: the C++'s
// `Inputs`.
struct ResectionInputs {
    network: NetworkInputs,
    // one per observation
    sights: Vec<Sight>,
    // pointing, its sigma
    directions: Vec<(usize, f64)>,
}

// What a resection's searches share: the C++'s lambdas over resect_setup's
// locals, as one value, so that each takes the engine as it is when it runs.
struct Resection<'d, 'a> {
    setup_index: usize,
    at: &'a str,
    station: &'a SurveyStation,
    data: &'d ResectionData<'a>,
    apriori: &'a ObservationPrecision,
    approximate: Position,
    readings: Vec<Reading>,
    starts: Vec<Plane>,
    options: AdjustmentOptions<'static>,
    confidence_scale: f64,
    bound: f64,
    // Why the first least squares that failed did, and where it started.
    failure: String,
    failed_at: Plane,
}

impl<'a> Resection<'_, 'a> {
    fn label(&self, what: &str, to: &str) -> String {
        format!("{what} at {}: {to}", self.station.setup.id)
    }

    // The network of its least squares, started at `start`: the station free,
    // its targets held, a direction and a distance per pointing as `readings`
    // has them, with the distances' factors at the start.
    fn inputs_at(&self, engine: &Engine<'a>, start: Plane) -> ResectionInputs {
        let here = Position {
            northing: start.imag(),
            easting: start.real(),
            ..self.approximate
        };
        let mut inputs = ResectionInputs {
            network: NetworkInputs::default(),
            sights: Vec::new(),
            directions: Vec::new(),
        };
        inputs.network.points.push(SurveyPoint {
            id: String::from(self.at),
            northing: here.northing,
            easting: here.easting,
            ..Default::default()
        });
        for target in &self.data.targets {
            inputs.network.points.push(SurveyPoint {
                id: String::from(target.id),
                northing: target.position.northing,
                easting: target.position.easting,
                ..Default::default()
            });
            inputs
                .network
                .control
                .push(ControlPoint::fixed_horizontal(target.id));
        }
        for &p in &self.data.pointings {
            let pointing = &engine.pointings[p];
            let target = engine.positions[pointing.target];
            let source = pointing
                .source
                .cloned()
                .unwrap_or_else(|| self.station.source.clone());
            let grid_distance = grid_distance_from(engine, self.setup_index, pointing, &here);
            let group = Pointing {
                index: pointing.left_index,
                face: Face::Unknown,
            };
            if let Some(reading) = pointing.direction {
                // Across the measured line where there is one, else the line
                // the start gives: centring is millimetres, the start closer.
                let sigma = with_centring(
                    self.apriori,
                    pointing.sigma_direction,
                    grid_distance.unwrap_or((plane_of(&target) - start).abs()),
                    StationModel::Resected,
                );
                inputs.directions.push((p, sigma));
                inputs.sights.push(Sight {
                    target: plane_of(&target),
                    sigma,
                    direction: true,
                });
                inputs
                    .network
                    .observations
                    .push(NetworkObservation::of_pointing(
                        HorizontalDirectionObservation {
                            at: String::from(self.at),
                            to: String::from(pointing.target),
                            direction: reading,
                            sigma,
                            source: source.clone(),
                            pointing: group,
                        },
                        self.label("direction", pointing.target),
                        source.clone(),
                        p,
                        PointingPart::Direction,
                    ));
            }
            if let Some(grid_distance) = grid_distance {
                let sigma = distance_sigma_of(self.apriori, pointing, StationModel::Resected);
                inputs.sights.push(Sight {
                    target: plane_of(&target),
                    sigma,
                    direction: false,
                });
                inputs
                    .network
                    .observations
                    .push(NetworkObservation::of_pointing(
                        DistanceObservation {
                            from: String::from(self.at),
                            to: String::from(pointing.target),
                            distance: grid_distance,
                            sigma,
                            kind: DistanceKind::Horizontal,
                            source: source.clone(),
                            pointing: group,
                            ..Default::default()
                        },
                        self.label("distance", pointing.target),
                        source,
                        p,
                        PointingPart::Distance,
                    ));
            }
        }
        inputs
    }

    fn fail(&mut self, start: Plane, message: &str) {
        if self.failure.is_empty() {
            self.failure = String::from(message);
            self.failed_at = start;
        }
    }

    fn solve_from(
        &mut self,
        engine: &Engine<'a>,
        start: Plane,
        excluded: &[bool],
    ) -> Option<Solution> {
        let inputs = self.inputs_at(engine, start);
        let mut included = Vec::new();
        let network = match build_network(&inputs.network, excluded, &mut included) {
            Ok(network) => network,
            Err(error) => {
                self.fail(start, &error.message);
                return None;
            }
        };
        let result = match adjust_horizontal_network(&network, &self.options) {
            Ok(result) => result,
            Err(error) => {
                self.fail(start, &error.message);
                return None;
            }
        };
        let mut position = None;
        for adjusted in &result.stations {
            if adjusted.point_id == self.at {
                position = Some(Plane::new(
                    adjusted.position.easting,
                    adjusted.position.northing,
                ));
            }
        }
        let kept: Vec<Sight> = inputs
            .sights
            .iter()
            .zip(excluded)
            .filter(|(_, excluded)| !**excluded)
            .map(|(sight, _)| *sight)
            .collect();
        let strength = position.and_then(|position| {
            station_strength(&result, self.at, position, &kept, self.confidence_scale)
        });
        let (Some(position), Some(strength)) = (position, strength) else {
            self.fail(start, "it gives the station no precision");
            return None;
        };
        Some(Solution {
            position,
            squares: result.statistics.weighted_squared_residuals,
            strength,
        })
    }

    // The solutions from the starts, beside `found`, over the observations
    // `excluded` leaves (see the section's head).
    fn search(
        &mut self,
        engine: &Engine<'a>,
        excluded: &[bool],
        mut found: Vec<Solution>,
    ) -> Vec<Solution> {
        let mut order: Vec<(Plane, f64)> = Vec::new();
        for &start in &self.starts {
            let squares = squares_at(start, &self.readings, excluded, self.apriori);
            if squares.is_finite() {
                order.push((start, squares));
            }
        }
        // std::stable_sort by squares.
        order.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(Ordering::Equal));
        for (point, squares) in order {
            let mut least = f64::INFINITY;
            let mut reached = false;
            for solution in &found {
                least = cpp::min(least, solution.squares);
                reached =
                    reached || (point - solution.position).abs() <= solution.strength.linear_within;
            }
            if squares > least + self.bound {
                break; // it, and every start after it, fits worse than the region allows
            }
            if reached {
                continue; // within a solution's linear reach: that solution
            }
            let Some(solution) = self.solve_from(engine, point, excluded) else {
                continue;
            };
            let known = found.iter().any(|other| {
                mahalanobis2(solution.position - other.position, &other.strength.cofactor)
                    <= self.bound
            });
            if !known {
                found.push(solution);
            }
        }
        found
    }
}

// The shape a station at `position` comes nearest, as the head of a refusal.
fn shape_at(distinct: &DistinctTargets<'_>, position: Plane) -> String {
    let clause = nearest_shape(&distinct.with_direction, position).clause;
    if clause.is_empty() {
        clause
    } else {
        clause + ", and "
    }
}

fn two_positions(
    distinct: &DistinctTargets<'_>,
    one: &Solution,
    other: &Solution,
    confidence_words: &str,
    bound: f64,
) -> String {
    format!(
        "{}its observations fit two positions {} apart ({} and {}: weighted squares of residuals \
         {} and {}, where the station's {confidence_words} confidence region takes in all within \
         {} of the least), so they do not fix it",
        shape_at(distinct, one.position),
        length_words((other.position - one.position).abs()),
        position_words(one.position),
        position_words(other.position),
        format_number(one.squares, 3),
        format_number(other.squares, 3),
        format_number(bound, 2)
    )
}

// What a resection's least squares leaves on an observation row, so that a
// refusal can take it back.
struct RowState {
    row: usize,
    rejected: bool,
    reason: String,
}

fn take_back(engine: &mut Engine<'_>, warnings_before: usize, rows_before: &[RowState]) {
    engine.report.warnings.truncate(warnings_before);
    for previous in rows_before {
        let observation = &mut engine.report.observations[previous.row];
        observation.rejected = previous.rejected;
        observation.rejection_reason = previous.reason.clone();
    }
}

/// Positions the station of setup `setup_index` by resection from its reduced
/// pointings to points already placed, held as they are: the least squares of
/// its directions (one set, one orientation unknown) and horizontal distances,
/// run from every start its observations give, and then of its trigonometric
/// height differences to the targets with heights, weighted by the reduction's
/// a-priori precision for an instrument that is itself the unknown (its
/// centring and height go to the station's precision, not to each pointing).
/// The station has no position, or one another setup radiated, which it
/// replaces and reports as a check. Places the station
/// (ComputationMethod::Resection), marks the setup resected (SetupState),
/// reports it (ReductionReport::resections) and returns true; or returns false
/// and puts in `why`, as a clause, what refused it: too few placed points, a
/// value that is not finite, a distance that is not positive, directions alone
/// exactly on one line with the station or on the circle through it, a least
/// squares that fails from every start, two positions its observations fit
/// alike, or a station its observations' a-priori precision leaves uncertain
/// by more than its least squares' linear model holds over. A refusal leaves
/// nothing behind: no warning of its least squares, no rejected observation.
#[allow(
    clippy::needless_range_loop,
    reason = "Every triple of the targets and every pair of the loci, by index, as the C++ walks them."
)]
pub(crate) fn resect_setup<'a>(
    engine: &mut Engine<'a>,
    setup_index: usize,
    why: &mut String,
) -> bool {
    let settings = engine.settings;
    let context = engine.context;
    let apriori = &settings.apriori;
    let raw = engine.raw;
    let station = &raw.stations[setup_index];
    let at: &'a str = &station.setup.point_id;
    // Where another setup's radiation put the station, when place_setups
    // resects a setup that names no backsight on it: replaced, and kept as a
    // check of the resection.
    let radiated = engine
        .find(at)
        .copied()
        .filter(|before| before.method == ComputationMethod::Radiation);

    let data = resection_data(engine, setup_index);
    if !data.not_finite.is_empty() {
        *why = format!(
            "what it measured to {}, or where that point is, is not a finite number",
            data.not_finite
        );
        return false;
    }
    let one_position = one_position_of(apriori);
    let distinct = distinct_targets(&data.targets, one_position);
    if distinct.with_both.len() < 2 && distinct.with_direction.len() < 3 {
        *why = too_few(&distinct);
        return false;
    }

    // Directions alone, every three of them exactly on one line with the
    // station or on the circle through it: nothing fixes it, whatever the
    // start, and the closed form says which.
    let any_distance = data.targets.iter().any(|target| target.distance.is_some());
    if !any_distance {
        let t = &distinct.with_direction;
        let mut first = String::new();
        let mut fixes = false;
        'search: for i in 0..t.len() {
            for j in i + 1..t.len() {
                for k in j + 1..t.len() {
                    let three = three_directions(&t[i], &t[j], &t[k]);
                    fixes = three.station.is_some();
                    if fixes {
                        break 'search;
                    }
                    if first.is_empty() {
                        first = three.why;
                    }
                }
            }
        }
        if !fixes {
            *why = first;
            return false;
        }
    }

    // ---- its observations ----
    // The distances' factors are taken at a station not yet known: the rigid
    // fit where there is one, else amid the targets, and the height the
    // targets with heights give it. Only the grid scale and the geoid depend
    // on where, a few parts per million over the few hundred metres a start
    // can be off; the least squares takes them again at each start it runs
    // from.
    let fit = if distinct.with_both.len() >= 2 {
        fit_station(&distinct.with_both)
    } else {
        None
    };
    let reference = match fit.filter(|fit| fit.is_finite()) {
        Some(fit) => fit,
        None => {
            let mut sum = Plane::default();
            for target in &distinct.with_direction {
                sum += plane_of(&target.position);
            }
            sum / distinct.with_direction.len() as f64
        }
    };
    let mut approximate = Position {
        northing: reference.imag(),
        easting: reference.real(),
        ..Default::default()
    };
    {
        let mut sum = 0.0;
        let mut count = 0_usize;
        for &p in &data.pointings {
            let pointing = &engine.pointings[p];
            let target = &engine.positions[pointing.target];
            if let (Some(height_difference), Some(height)) =
                (pointing.height_difference, target.height)
            {
                sum += height - height_difference;
                count += 1;
            }
        }
        if count > 0 {
            approximate.height = Some(sum / count as f64);
        }
    }
    let mut readings: Vec<Reading> = Vec::new();
    for &p in &data.pointings {
        let pointing = &engine.pointings[p];
        let target = plane_of(&engine.positions[pointing.target]);
        let grid_distance = grid_distance_from(engine, setup_index, pointing, &approximate);
        if let Some(distance) = grid_distance
            && !(distance.is_finite() && distance > 0.0)
        {
            *why = format!(
                "its distance to {} is not a positive number",
                pointing.target
            );
            return false;
        }
        if let Some(direction) = pointing.direction {
            readings.push(Reading {
                target,
                direction: true,
                value: direction,
                sigma: pointing.sigma_direction,
                sight: grid_distance,
            });
        }
        if let Some(distance) = grid_distance {
            readings.push(Reading {
                target,
                direction: false,
                value: distance,
                sigma: distance_sigma_of(apriori, pointing, StationModel::Resected),
                sight: None,
            });
        }
    }

    // ---- where its least squares starts ----
    // Every point where two of its loci meet (the section's head), and the
    // rigid fit.
    let mut loci: Vec<Locus> = Vec::new();
    {
        // A circle per distinct place with a distance, its radius the mean.
        let mut centres: Vec<Plane> = Vec::new();
        for target in &data.targets {
            let centre = plane_of(&target.position);
            let mut sum = 0.0;
            let mut count = 0_usize;
            for reading in &readings {
                if !reading.direction && reading.target == centre {
                    sum += reading.value;
                    count += 1;
                }
            }
            let repeated = centres
                .iter()
                .any(|other| (*other - centre).abs() <= one_position);
            if count > 0 && !repeated {
                centres.push(centre);
                loci.push(Locus {
                    point: centre,
                    radius: sum / count as f64,
                    along: Plane::default(),
                });
            }
        }
        // An arc per pair of targets read with a direction: each with the
        // next round the circle and with the one it makes the widest angle
        // with: every pair of three targets, and of more a number of arcs
        // that grows with the targets, not with their square, while every
        // target is on two of them.
        let seen = &distinct.with_direction;
        let count = seen.len();
        let direction_of = |k: usize| seen[k].direction.unwrap_or(0.0);
        let mut by_reading: Vec<usize> = (0..count).collect();
        // std::stable_sort by reading.
        by_reading.sort_by(|&x, &y| {
            direction_of(x)
                .partial_cmp(&direction_of(y))
                .unwrap_or(Ordering::Equal)
        });
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        let mut pair = |i: usize, j: usize| {
            let both = (i.min(j), i.max(j));
            if i != j && !pairs.contains(&both) {
                pairs.push(both);
            }
        };
        for k in 0..count {
            pair(by_reading[k], by_reading[(k + 1) % count]);
        }
        for i in 0..count {
            let mut widest = i;
            let mut sine = -1.0;
            for j in 0..count {
                let s = (direction_of(j) - direction_of(i)).sin().abs();
                if j != i && s > sine {
                    sine = s;
                    widest = j;
                }
            }
            pair(i, widest);
        }
        for &(i, j) in &pairs {
            let from = plane_of(&seen[i].position);
            let to = plane_of(&seen[j].position);
            let angle = direction_of(j) - direction_of(i);
            if angle.sin().abs() <= tolerance::ANGULAR {
                loci.push(Locus {
                    point: from,
                    radius: 0.0,
                    along: (to - from) / (to - from).abs(),
                });
            } else {
                let centre = inscribed_centre(from, to, angle);
                loci.push(Locus {
                    point: centre,
                    radius: (from - centre).abs(),
                    along: Plane::default(),
                });
            }
        }
    }
    let mut starts: Vec<Plane> = Vec::new();
    if let Some(fit) = fit {
        starts.push(fit);
    }
    for a in 0..loci.len() {
        for b in a + 1..loci.len() {
            meet(&loci[a], &loci[b], &mut starts);
        }
    }
    // Not a start: a point that is not finite, or one on a target, where
    // every arc through that target meets another.
    starts.retain(|start| {
        start.is_finite()
            && !data
                .targets
                .iter()
                .any(|target| (plane_of(&target.position) - *start).abs() <= one_position)
    });

    // ---- its solutions ----
    let confidence_scale =
        ellipse_confidence_scale(settings.confidence_level).unwrap_or(f64::INFINITY);
    // chi-square(2) at the confidence level: the edge of the station's
    // confidence region, in weighted squares above the least.
    let bound = confidence_scale * confidence_scale;
    let confidence_words = format!("{} %", format_number(settings.confidence_level * 100.0, 0));
    let mut resection = Resection {
        setup_index,
        at,
        station,
        data: &data,
        apriori,
        approximate,
        readings,
        starts,
        options: AdjustmentOptions {
            max_iterations: settings.max_iterations,
            significance_level: 1.0 - settings.confidence_level,
            ..Default::default()
        },
        confidence_scale,
        bound,
        failure: String::new(),
        failed_at: Plane::default(),
    };

    let solutions = resection.search(engine, &vec![false; resection.readings.len()], Vec::new());
    if solutions.is_empty() {
        // A rank deficiency (network_adjustment.rs's rank error) is the
        // geometry's: its observations do not determine the station where the
        // best start put it, and the shape says how. A failure to converge is
        // not, and names none.
        let geometric = resection
            .failure
            .starts_with("the network is rank deficient");
        *why = format!(
            "{}its least squares cannot fix it ({})",
            if geometric {
                shape_at(&distinct, resection.failed_at)
            } else {
                String::new()
            },
            if resection.failure.is_empty() {
                "no start was found"
            } else {
                resection.failure.as_str()
            }
        );
        return false;
    }
    let best = least_of(&solutions);
    if let Some(rival) = rival_of(&solutions, best, bound) {
        *why = two_positions(
            &distinct,
            &solutions[best],
            &solutions[rival],
            &confidence_words,
            bound,
        );
        return false;
    }

    // ---- the least squares it is reported by, from the best solution ----
    let horizontal = resection.inputs_at(engine, solutions[best].position);

    // What its least squares leaves behind (warnings, observations marked
    // rejected) so that a refusal takes it back: a setup that was not
    // resected has no resection residuals.
    let warnings_before = engine.report.warnings.len();
    let mut rows_before: Vec<RowState> = Vec::new();
    for &p in &data.pointings {
        for part in [
            PointingPart::Direction,
            PointingPart::Distance,
            PointingPart::Height,
        ] {
            for row in part_rows(&engine.pointings[p], part) {
                let observation = &engine.report.observations[row];
                rows_before.push(RowState {
                    row,
                    rejected: observation.rejected,
                    reason: observation.rejection_reason.clone(),
                });
            }
        }
    }

    let name = format!("resection at {}", station.setup.id);
    let mut horizontal_report = AdjustmentReport::default();
    let solved = match adjust_with_outliers(
        engine,
        &horizontal.network,
        &format!("{name} (horizontal)"),
        adjust_horizontal_network,
        &mut horizontal_report,
        "the resection",
    ) {
        Ok(solved) => solved,
        Err(error) => {
            take_back(engine, warnings_before, &rows_before);
            *why = format!("its least squares cannot fix it ({})", error.message);
            return false;
        }
    };
    // The last of each that is the station's, as the C++ takes it.
    let fixed = solved
        .stations
        .iter()
        .rfind(|adjusted| adjusted.point_id == at);
    let orientation = solved
        .orientations
        .iter()
        .rfind(|adjusted| adjusted.point_id == at);
    let (Some(fixed), Some(orientation)) = (fixed, orientation) else {
        take_back(engine, warnings_before, &rows_before);
        *why = String::from("no direction is left to orient it");
        return false;
    };

    // ---- does it fix the station? ----
    let solution = Plane::new(fixed.position.easting, fixed.position.northing);
    let mut excluded = vec![false; horizontal.network.observations.len()];
    let mut kept: Vec<Sight> = Vec::new();
    for (i, observation) in horizontal.network.observations.iter().enumerate() {
        if let Some(pointing) = observation.pointing {
            for row in part_rows(&engine.pointings[pointing], observation.part) {
                excluded[i] = excluded[i] || engine.report.observations[row].rejected;
            }
        }
        if !excluded[i] {
            kept.push(horizontal.sights[i]);
        }
    }
    let strength = station_strength(&solved, at, solution, &kept, confidence_scale);
    if let Some(strength) = strength
        && excluded.contains(&true)
    {
        // What the outlier test rejected changes what the rest fix: searched
        // again without it, the solution reached the first found.
        let again = resection.search(
            engine,
            &excluded,
            vec![Solution {
                position: solution,
                squares: solved.statistics.weighted_squared_residuals,
                strength,
            }],
        );
        let least = least_of(&again);
        let rival = if least != 0 {
            Some(least)
        } else {
            rival_of(&again, least, bound)
        };
        if let Some(rival) = rival {
            take_back(engine, warnings_before, &rows_before);
            *why = two_positions(
                &distinct,
                &again[0],
                &again[rival],
                &confidence_words,
                bound,
            );
            return false;
        }
    }
    let Some(strength) = strength.filter(StationStrength::fixes) else {
        take_back(engine, warnings_before, &rows_before);
        *why = match strength {
            Some(strength) => format!(
                "{}at the precision of its observations it is uncertain by {} ({}) along azimuth \
                 {}, past the {} within which its least squares' linear model holds",
                shape_at(&distinct, solution),
                length_words(strength.confidence),
                confidence_words,
                axis_words(&strength.apriori),
                length_words(strength.linear_within)
            ),
            None => String::from("its least squares gives it no precision"),
        };
        return false;
    };

    // ---- its height ----
    let mut elevation = None;
    let mut sigma_elevation = None;
    let mut height_report = None;
    let mut levels = NetworkInputs::default();
    levels.points.push(SurveyPoint {
        id: String::from(at),
        northing: fixed.position.northing,
        easting: fixed.position.easting,
        elevation: approximate.height,
        ..Default::default()
    });
    let mut held_in_height: HashSet<&'a str> = HashSet::new();
    for &p in &data.pointings {
        let pointing = &engine.pointings[p];
        let target = &engine.positions[pointing.target];
        let (Some(height_difference), Some(_), Some(_), Some(target_height)) = (
            pointing.height_difference,
            pointing.slope,
            pointing.zenith,
            target.height,
        ) else {
            continue;
        };
        if held_in_height.insert(pointing.target) {
            levels.points.push(SurveyPoint {
                id: String::from(pointing.target),
                northing: target.northing,
                easting: target.easting,
                elevation: Some(target_height),
                ..Default::default()
            });
            levels
                .control
                .push(ControlPoint::fixed_vertical(pointing.target));
        }
        let source = pointing
            .source
            .cloned()
            .unwrap_or_else(|| station.source.clone());
        levels.observations.push(NetworkObservation::of_pointing(
            LevelDifferenceObservation {
                from: String::from(at),
                to: String::from(pointing.target),
                height_difference,
                sigma: height_difference_sigma_of(apriori, pointing, StationModel::Resected),
                length: pointing.horizontal.unwrap_or(0.0),
                source: source.clone(),
            },
            resection.label("height difference", pointing.target),
            source,
            p,
            PointingPart::Height,
        ));
    }
    if levels.observations.is_empty() {
        engine.warn_setup(
            station,
            "resected with no height: no point it was resected from has a height and was \
             observed with both a zenith angle and a distance, which a trigonometric height \
             needs, so nothing radiated from it has one.",
        );
    } else {
        let mut report = AdjustmentReport::default();
        match adjust_with_outliers(
            engine,
            &levels,
            &format!("{name} (heights)"),
            adjust_level_network,
            &mut report,
            "the resection",
        ) {
            Ok(levelled) => {
                let scale = precision_scale_of(&levelled.statistics);
                for (k, parameter) in levelled.parameters.iter().enumerate() {
                    if parameter.point_id == at {
                        sigma_elevation = Some((levelled.cofactor[(k, k)] * scale).sqrt());
                    }
                }
                for adjusted in &levelled.elevations {
                    if adjusted.point_id == at {
                        elevation = Some(adjusted.elevation);
                    }
                }
                report.iterations = 1;
                height_report = Some(report);
            }
            Err(error) => {
                engine.warn(
                    format!(
                        "Setup {}: the heights of its resection could not be adjusted ({}), so \
                         its station has no height.",
                        station.setup.id, error.message
                    ),
                    station.source.clone(),
                );
            }
        }
    }
    if horizontal_report.redundancy == 0 {
        engine.warn_setup(
            station,
            "its resection has no redundancy - as many observations as unknowns - so nothing \
             checks the position it gives.",
        );
    }

    // ---- the station ----
    // Its precision is the least squares' a-priori one, scaled only where the
    // global test finds its residuals too large (precision_scale_of). The
    // least squares placed the instrument. Where it stands over a mark (the
    // setup records an instrument height) the mark is off the instrument by
    // the centring and below it by the measured height, errors common to every
    // pointing and so in no pointing's weight (StationModel): they are the
    // station's, added here. An instrument height of zero is a free station
    // with no mark under it, the instrument itself the point.
    let plan_scale = precision_scale_of(&solved.statistics);
    let mut covariance = Covariance2 {
        northing: strength.cofactor.northing * plan_scale,
        easting: strength.cofactor.easting * plan_scale,
        northing_easting: strength.cofactor.northing_easting * plan_scale,
    };
    let mut sigma_orientation = 0.0;
    for (k, parameter) in solved.parameters.iter().enumerate() {
        if parameter.point_id == at && parameter.component == CoordinateComponent::Orientation {
            sigma_orientation = (solved.cofactor[(k, k)] * plan_scale).sqrt();
        }
    }
    let over_a_mark = station.setup.instrument_height != 0.0;
    if over_a_mark {
        let centring = apriori.instrument_centring * apriori.instrument_centring;
        covariance.northing += centring;
        covariance.easting += centring;
        if let Some(sigma) = sigma_elevation {
            sigma_elevation = Some(sigma.hypot(apriori.height_measurement));
        }
    }
    let position = Position {
        northing: fixed.position.northing,
        easting: fixed.position.easting,
        height: elevation,
        origin: PositionOrigin::Computed,
        method: ComputationMethod::Resection,
        sigma_northing: Some(covariance.northing.sqrt()),
        sigma_easting: Some(covariance.easting.sqrt()),
        sigma_height: sigma_elevation,
        ..Default::default()
    };
    engine.place(at, position);

    // One the outlier test rejected is not in the solution, so not in its
    // orientation either.
    let resection_directions: Vec<(usize, f64)> = horizontal
        .directions
        .iter()
        .copied()
        .filter(|&(p, _)| !direction_rejected(engine, &engine.pointings[p]))
        .collect();
    {
        let state = &mut engine.setups[setup_index];
        state.resected = true;
        state.resection_directions = resection_directions;
        state.resection_pointings = data.pointings.iter().copied().collect();
        state.resection_targets = data.targets.iter().map(|target| target.id).collect();
    }

    let fixed_ellipse = fixed.ellipse;
    horizontal_report.iterations = solved.iterations;
    horizontal_report.ellipses.push(ReportEllipse {
        point_id: String::from(at),
        standard: error_ellipse(covariance).unwrap_or(fixed_ellipse),
        confidence_scale,
    });
    let dilution = strength.dilution();
    let mut report = ResectionReport {
        station_id: station.setup.id.clone(),
        point_id: String::from(at),
        targets: data
            .targets
            .iter()
            .map(|target| String::from(target.id))
            .collect(),
        northing: position.northing,
        easting: position.easting,
        elevation,
        sigma_northing: covariance.northing.sqrt(),
        sigma_easting: covariance.easting.sqrt(),
        sigma_elevation,
        orientation: normalize_angle_signed(orientation.orientation),
        sigma_orientation,
        horizontal: horizontal_report,
        height: height_report,
        apriori_ellipse: ReportEllipse {
            point_id: String::from(at),
            standard: strength.apriori,
            confidence_scale,
        },
        dilution,
        weak_geometry: dilution > WEAK_RESECTION_DILUTION,
        checks: data.checks,
        source: station.source.clone(),
        ..Default::default()
    };
    let resected_from = names_of(&distinct.with_direction);

    // The position another setup radiated the station to, which the
    // resection replaced: a check of the one against the other, and a warning.
    if let Some(radiated) = radiated {
        let by = radiated.from_setup;
        let radiator = if by < raw.stations.len() {
            raw.stations[by].setup.id.clone()
        } else {
            String::from("another setup")
        };
        let northing = position.northing - radiated.northing;
        let easting = position.easting - radiated.easting;
        let linear = northing.hypot(easting);
        let check = MisclosureReport {
            name: format!(
                "{at} by resection at setup {}, against its radiation from setup {radiator}",
                station.setup.id
            ),
            northing: Some(northing),
            easting: Some(easting),
            linear: Some(linear),
            height: match (position.height, radiated.height) {
                (Some(resected_height), Some(radiated_height)) => {
                    Some(resected_height - radiated_height)
                }
                _ => None,
            },
            ..Default::default()
        };
        report.radiated_from = radiator.clone();
        report.radiated_northing_difference = Some(northing);
        report.radiated_easting_difference = Some(easting);
        engine.warn(
            format!(
                "Setup {} stands on {at}, which setup {radiator} radiated: its own resection from \
                 {resected_from} places it {} from there, and the radiation is a check of it (the \
                 misclosure \"{}\").",
                station.setup.id,
                length_words(linear),
                check.name
            ),
            station.source.clone(),
        );
        engine.report.misclosures.push(check);
    }

    // The file's own coordinates for the station, which the resection
    // replaced (place_setups says why it comes first): not dropped unseen, but
    // a check of the one against the other, and a warning.
    if let Some(&file) = engine.file_points.get(at) {
        let northing = position.northing - file.northing;
        let easting = position.easting - file.easting;
        let linear = northing.hypot(easting);
        let check = MisclosureReport {
            name: format!(
                "{at} by resection at setup {}, against the file's own coordinates",
                station.setup.id
            ),
            northing: Some(northing),
            easting: Some(easting),
            linear: Some(linear),
            height: match (position.height, file.elevation) {
                (Some(resected_height), Some(file_height)) => Some(resected_height - file_height),
                _ => None,
            },
            ..Default::default()
        };
        report.file_northing_difference = Some(northing);
        report.file_easting_difference = Some(easting);
        engine.warn(
            format!(
                "Setup {} stands on {at}: its resection from {resected_from} places it {} from \
                 the file's own coordinates for it, which were not used (the misclosure \"{}\").",
                station.setup.id,
                length_words(linear),
                check.name
            ),
            station.source.clone(),
        );
        engine.report.misclosures.push(check);
    }

    // Ground distances fitted to grid coordinates: where the settings reduce
    // no distance to the grid and the drawing's projection says the grid is
    // not the ground there, by more than the longest distance's own standard
    // deviation, the resection's distance residuals and its station carry the
    // difference, and nothing else would say why.
    if !settings.use_combined_factor
        && settings.grid_scale == GridScale::None
        && let Some(grid_scale_factor) = &context.grid_scale_factor
    {
        let mut longest = 0.0;
        let mut longest_sigma = 0.0;
        for sight in &kept {
            let length = (sight.target - solution).abs();
            if !sight.direction && length > longest {
                longest = length;
                longest_sigma = sight.sigma;
            }
        }
        let scale = grid_scale_factor(
            position.northing,
            position.easting,
            position.height.unwrap_or(0.0),
        );
        if let Some(scale) = scale
            && longest > 0.0
            && scale.is_finite()
            && (scale - 1.0).abs() * longest > longest_sigma
        {
            report.unapplied_scale_factor = Some(scale);
            engine.warn_setup(
                station,
                "its resection fitted distances measured on the ground to its targets' grid \
                 coordinates: the drawing's projection gives a point scale factor at the station \
                 that these settings do not apply (no grid scale), and it changes the longest \
                 distance by more than that distance's standard deviation, so the resection's \
                 residuals and its station carry the difference. Set the grid scale to the \
                 projection's to reduce the distances to the grid.",
            );
        }
    }

    if report.weak_geometry {
        engine.warn(
            format!(
                "Setup {}: its resection from {resected_from} has a weak geometry - at the \
                 precision of its observations its station is uncertain by {} ({}) along azimuth \
                 {}, {} times the standard deviation of its least precise observation - and \
                 everything radiated from it carries that.",
                station.setup.id,
                length_words(strength.confidence),
                confidence_words,
                axis_words(&strength.apriori),
                format_number(report.dilution, 1)
            ),
            station.source.clone(),
        );
    }
    engine.report.resections.push(report);
    true
}
