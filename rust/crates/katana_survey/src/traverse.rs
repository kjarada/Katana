//! Classical traverse computation: azimuth propagation, angular misclosure and
//! balancing, latitudes and departures, linear misclosure, relative precision
//! and the compass (Bowditch) or transit rule.
//!
//! See data_model.rs for the meaning of the Traverse fields per kind. Nothing is
//! printed; every intermediate quantity is reported in TraverseResult.
//!
//! Sign conventions
//!   * misclosure = computed - known (angular and linear); corrections have the
//!     opposite sign.
//!   * latitude = d * cos(azimuth) (north positive), departure = d * sin(azimuth)
//!     (east positive).

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use std::collections::BTreeSet;

use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::numerics::{PI, normalize_angle, normalize_angle_signed};

use crate::coordinate::Coordinate2;
use crate::data_model::{Traverse, TraverseKind};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TraverseAdjustment {
    /// Report misclosures only.
    #[default]
    None,
    /// Bowditch: corrections proportional to leg length.
    Compass,
    /// Corrections proportional to |latitude| and |departure|.
    Transit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraverseOptions {
    /// Distribute the angular misclosure equally over the measured angles before
    /// computing azimuths. Ignored where no angular check exists.
    pub balance_angles: bool,
    /// Ignored for open traverses, which have no linear misclosure.
    pub adjustment: TraverseAdjustment,
}

impl Default for TraverseOptions {
    fn default() -> Self {
        Self {
            balance_angles: true,
            adjustment: TraverseAdjustment::Compass,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraverseLegResult {
    pub from_id: String,
    pub to_id: String,
    /// From balanced angles when balancing is on.
    pub azimuth: f64,
    /// As observed.
    pub distance: f64,
    pub latitude: f64,
    pub departure: f64,
    pub latitude_correction: f64,
    pub departure_correction: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraverseStationResult {
    pub id: String,
    /// From (balanced) azimuths and observed distances.
    pub unadjusted: Coordinate2,
    /// After the linear adjustment; equals `unadjusted` for None / Open.
    pub adjusted: Coordinate2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LinearMisclosure {
    /// Computed - known closing northing.
    pub latitude: f64,
    /// Computed - known closing easting.
    pub departure: f64,
    /// hypot(latitude, departure)
    pub length: f64,
    /// length / total traverse length; 0 for a perfect closure.
    pub relative_precision: f64,
    /// The N of "1 : N"; None when the closure is exact.
    pub precision_denominator: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraverseResult {
    pub kind: TraverseKind,
    /// Angles taking part in the angular check.
    pub angle_count: usize,
    /// Computed - expected closing direction, in (-pi, pi]; None without an
    /// angular check (open traverse, link traverse without closing angle).
    pub angular_misclosure: Option<f64>,
    /// Added to every measured angle.
    pub angle_correction: f64,
    pub legs: Vec<TraverseLegResult>,
    /// One entry per station in traverse order, the start first. For a closed
    /// loop the start is not repeated at the end; for open and link traverses
    /// the last entry is the end station.
    pub stations: Vec<TraverseStationResult>,
    pub total_length: f64,
    /// None for open traverses.
    pub linear_misclosure: Option<LinearMisclosure>,
}

fn malformed(traverse: &Traverse, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::InvalidArgument, message)
        .with_context(format!("traverse '{}'", traverse.name))
}

fn is_finite(c: Coordinate2) -> bool {
    c.northing.is_finite() && c.easting.is_finite()
}

// Angular misclosure from the raw angles: azimuth carried through every
// station minus the direction it has to arrive at.
fn angular_misclosure(traverse: &Traverse) -> Option<f64> {
    let setups = &traverse.setups;
    if traverse.kind == TraverseKind::ClosedLoop {
        // Carrying the first-leg azimuth once around the loop adds every angle
        // plus pi per station and must reproduce it: sum + n*pi = 0 (mod 2*pi).
        // Holds for interior (counter-clockwise loop) and exterior angles alike.
        let mut carried = 0.0;
        for setup in setups {
            carried = normalize_angle(carried + setup.angle + PI);
        }
        return Some(normalize_angle_signed(carried));
    }
    if traverse.kind == TraverseKind::Link
        && let Some(closing_angle) = traverse.closing_angle
    {
        let mut azimuth = traverse.start_azimuth + setups[0].angle;
        for setup in &setups[1..] {
            azimuth = normalize_angle(azimuth + PI + setup.angle);
        }
        let closing = azimuth + PI + closing_angle.angle;
        return Some(normalize_angle_signed(
            closing - closing_angle.reference_azimuth,
        ));
    }
    None
}

fn leg_azimuths(traverse: &Traverse, angle_correction: f64) -> Vec<f64> {
    let setups = &traverse.setups;
    let mut azimuths = Vec::with_capacity(setups.len());
    azimuths.push(if traverse.kind == TraverseKind::ClosedLoop {
        normalize_angle(traverse.start_azimuth)
    } else {
        normalize_angle(traverse.start_azimuth + setups[0].angle + angle_correction)
    });
    for setup in &setups[1..] {
        let previous = azimuths[azimuths.len() - 1];
        azimuths.push(normalize_angle(
            previous + PI + setup.angle + angle_correction,
        ));
    }
    azimuths
}

/// InvalidArgument when the traverse is malformed: no setups (fewer than 3 for
/// a loop), empty or repeated station ids, non-finite values, non-positive
/// distances, or fields that the kind requires are missing.
pub fn validate_traverse(traverse: &Traverse) -> Status {
    let setups = &traverse.setups;
    let is_loop = traverse.kind == TraverseKind::ClosedLoop;
    if setups.is_empty() {
        return Err(malformed(traverse, "a traverse needs at least one setup"));
    }
    if is_loop && setups.len() < 3 {
        return Err(malformed(traverse, "a closed loop needs at least 3 setups"));
    }
    if !is_finite(traverse.start) || !traverse.start_azimuth.is_finite() {
        return Err(malformed(
            traverse,
            "start coordinates or start azimuth are not finite",
        ));
    }

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for setup in setups {
        if setup.station_id.is_empty() {
            return Err(malformed(traverse, "a setup has an empty station id"));
        }
        if !seen.insert(&setup.station_id) {
            return Err(malformed(
                traverse,
                format!("station id '{}' is repeated", setup.station_id),
            ));
        }
        if !setup.angle.is_finite() {
            return Err(malformed(
                traverse,
                format!("angle at '{}' is not finite", setup.station_id),
            ));
        }
        if !setup.distance.is_finite() || !(setup.distance > 0.0) {
            return Err(malformed(
                traverse,
                format!(
                    "distance from '{}' must be positive and finite",
                    setup.station_id
                ),
            ));
        }
    }
    if is_loop {
        return Ok(());
    }

    if traverse.end_station_id.is_empty() {
        return Err(malformed(
            traverse,
            "open and link traverses need an end station id",
        ));
    }
    // A link traverse may close on its own start (a loop oriented on an external
    // reference mark); any other repetition is an error.
    let closes_on_start = traverse.kind == TraverseKind::Link
        && setups.len() >= 2
        && traverse.end_station_id == setups[0].station_id;
    if seen.contains(traverse.end_station_id.as_str()) && !closes_on_start {
        return Err(malformed(
            traverse,
            format!("end station id '{}' is repeated", traverse.end_station_id),
        ));
    }
    if traverse.kind == TraverseKind::Link {
        if !is_finite(traverse.end) {
            return Err(malformed(traverse, "end coordinates are not finite"));
        }
        if closes_on_start && traverse.end != traverse.start {
            return Err(malformed(
                traverse,
                "the end station is the start station but their coordinates differ",
            ));
        }
        if let Some(closing_angle) = traverse.closing_angle
            && (!closing_angle.angle.is_finite() || !closing_angle.reference_azimuth.is_finite())
        {
            return Err(malformed(
                traverse,
                "closing angle or its reference azimuth is not finite",
            ));
        }
    }
    Ok(())
}

pub fn compute_traverse(traverse: &Traverse, options: &TraverseOptions) -> Result<TraverseResult> {
    validate_traverse(traverse)?;
    let setups = &traverse.setups;
    let leg_count = setups.len();
    let is_loop = traverse.kind == TraverseKind::ClosedLoop;

    let mut result = TraverseResult {
        kind: traverse.kind,
        ..Default::default()
    };

    // ---- angles ----
    result.angular_misclosure = angular_misclosure(traverse);
    if let Some(misclosure) = result.angular_misclosure {
        result.angle_count = if is_loop { leg_count } else { leg_count + 1 };
        if options.balance_angles {
            result.angle_correction = -misclosure / result.angle_count as f64;
        }
    }
    let azimuths = leg_azimuths(traverse, result.angle_correction);

    // ---- latitudes and departures ----
    result.legs = Vec::with_capacity(leg_count);
    let mut sum_latitude = 0.0;
    let mut sum_departure = 0.0;
    let mut sum_abs_latitude = 0.0;
    let mut sum_abs_departure = 0.0;
    for (k, (setup, &azimuth)) in setups.iter().zip(&azimuths).enumerate() {
        let to_id = if k + 1 < leg_count {
            setups[k + 1].station_id.clone()
        } else if is_loop {
            setups[0].station_id.clone()
        } else {
            traverse.end_station_id.clone()
        };
        let distance = setup.distance;
        let leg = TraverseLegResult {
            from_id: setup.station_id.clone(),
            to_id,
            azimuth,
            distance,
            latitude: distance * azimuth.cos(),
            departure: distance * azimuth.sin(),
            latitude_correction: 0.0,
            departure_correction: 0.0,
        };
        sum_latitude += leg.latitude;
        sum_departure += leg.departure;
        sum_abs_latitude += leg.latitude.abs();
        sum_abs_departure += leg.departure.abs();
        result.total_length += leg.distance;
        result.legs.push(leg);
    }

    // ---- linear misclosure and its distribution ----
    if traverse.kind != TraverseKind::Open {
        // The control difference is formed first: it is a small, (nearly) exact
        // number, whereas start + sum would round at the size of the coordinates.
        let known_latitude = if is_loop {
            0.0
        } else {
            traverse.end.northing - traverse.start.northing
        };
        let known_departure = if is_loop {
            0.0
        } else {
            traverse.end.easting - traverse.start.easting
        };
        let latitude = sum_latitude - known_latitude;
        let departure = sum_departure - known_departure;
        let length = latitude.hypot(departure);
        let misclosure = LinearMisclosure {
            latitude,
            departure,
            length,
            relative_precision: length / result.total_length,
            precision_denominator: if length > 0.0 {
                Some(result.total_length / length)
            } else {
                None
            },
        };
        result.linear_misclosure = Some(misclosure);

        let total_length = result.total_length;
        for leg in &mut result.legs {
            if options.adjustment == TraverseAdjustment::Compass {
                let share = leg.distance / total_length;
                leg.latitude_correction = -misclosure.latitude * share;
                leg.departure_correction = -misclosure.departure * share;
            } else if options.adjustment == TraverseAdjustment::Transit {
                // |sum| <= sum of magnitudes, so a zero denominator implies a zero
                // misclosure in that component and nothing to distribute.
                if sum_abs_latitude > 0.0 {
                    leg.latitude_correction =
                        -misclosure.latitude * leg.latitude.abs() / sum_abs_latitude;
                }
                if sum_abs_departure > 0.0 {
                    leg.departure_correction =
                        -misclosure.departure * leg.departure.abs() / sum_abs_departure;
                }
            }
        }
    }

    // ---- coordinates ----
    // Offsets from the start are accumulated first and added to the (possibly
    // UTM sized) start coordinates once per station, so rounding at coordinate
    // magnitude does not accumulate along the traverse.
    let station_count = if is_loop { leg_count } else { leg_count + 1 };
    result.stations = Vec::with_capacity(station_count);
    result.stations.push(TraverseStationResult {
        id: setups[0].station_id.clone(),
        unadjusted: traverse.start,
        adjusted: traverse.start,
    });
    let mut latitude = 0.0;
    let mut departure = 0.0;
    let mut adjusted_latitude = 0.0;
    let mut adjusted_departure = 0.0;
    for leg in &result.legs[..station_count - 1] {
        latitude += leg.latitude;
        departure += leg.departure;
        adjusted_latitude += leg.latitude + leg.latitude_correction;
        adjusted_departure += leg.departure + leg.departure_correction;
        result.stations.push(TraverseStationResult {
            id: leg.to_id.clone(),
            unadjusted: Coordinate2 {
                northing: traverse.start.northing + latitude,
                easting: traverse.start.easting + departure,
            },
            adjusted: Coordinate2 {
                northing: traverse.start.northing + adjusted_latitude,
                easting: traverse.start.easting + adjusted_departure,
            },
        });
    }
    // The adjusted end of a link traverse is its control point by construction;
    // state it exactly instead of to within the rounding of the distribution.
    if traverse.kind == TraverseKind::Link
        && options.adjustment != TraverseAdjustment::None
        && let Some(last) = result.stations.last_mut()
    {
        last.adjusted = traverse.end;
    }
    Ok(result)
}
