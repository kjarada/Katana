//! GNSS vectors in reduce_and_adjust: what an RTK controller records between a
//! base and a rover (a Trimble job's ECEF deltas, a Topcon RW5 vector) turned
//! into grid terms, then radiated from the base like a total-station shot, or
//! handed to the network as a baseline.
//!
//! A geocentric vector is converted at its base: the base's global position
//! (a GnssGlobalPositionObservation of it in the file, or the far end of an
//! earlier vector) and base + delta both go through the context's
//! geocentric_to_grid (or its geodetic_to_grid after GRS80, when that is the
//! one the drawing gives), and the vector in grid is their difference. The
//! difference is taken on the drawing's own projection, so the grid scale and
//! the convergence are in it; no scale factor is applied to it again.
//!
//! Heights: the difference of the two ellipsoidal heights, less the change in
//! geoid separation when the drawing gives one, plus the base's antenna height
//! less the rover's. An RTK vector runs between the antennas' phase centres
//! (the receiver can measure nothing else), so the antenna heights take it down
//! to the marks. The horizontal part needs no such reduction: the two plumb
//! lines are parallel to 0.2 mm over a kilometre at 2 m.

use std::collections::{HashMap, HashSet};

use crate::data_model::{
    AntennaHeightMethod, GeocentricCoordinate, GeodeticCoordinate, GnssAntenna, GnssSolution,
    Observation, SourceRecord, SurveyText,
};
use crate::reduction::{GridPosition, ReductionContext, format_number};
use crate::reduction_engine::{Engine, GridVector, Position, PositionOrigin};
use crate::reduction_formulas::{
    geocentric_from_geodetic, geodetic_from_geocentric, local_variances,
};
use crate::reduction_report::{
    AppliedCorrection, ComputationMethod, CorrectionKind, MisclosureReport, ReportObservation,
};

fn reject_row(engine: &mut Engine<'_>, index: usize, reason: &str) {
    let row = &mut engine.report.observations[index];
    row.rejected = true;
    row.rejection_reason = String::from(reason);
}

/// The drawing's grid position of an earth-centred point, through
/// geocentric_to_grid or, when the drawing gives only geodetic_to_grid, through
/// that after the GRS80 conversion (the ellipsoid GNSS frames use). Absent
/// when the drawing can do neither.
pub(crate) fn grid_of_geocentric(
    context: &ReductionContext,
    point: &GeocentricCoordinate,
) -> Option<GridPosition> {
    if let Some(to_grid) = &context.geocentric_to_grid {
        return to_grid(point);
    }
    if let Some(to_grid) = &context.geodetic_to_grid {
        return to_grid(&geodetic_from_geocentric(point));
    }
    None
}

/// The same for a geodetic point.
pub(crate) fn grid_of_geodetic(
    context: &ReductionContext,
    point: &GeodeticCoordinate,
) -> Option<GridPosition> {
    if let Some(to_grid) = &context.geodetic_to_grid {
        return to_grid(point);
    }
    if let Some(to_grid) = &context.geocentric_to_grid {
        return to_grid(&geocentric_from_geodetic(point));
    }
    None
}

/// Whether an antenna height can be taken off as a vertical offset: a
/// vertical or phase-centre height, or no height at all. A slant height or
/// one to a mark the model does not name cannot, and neither can a height
/// with no stated method (it could be slant).
pub(crate) fn reducible_antenna(antenna: &GnssAntenna) -> bool {
    match antenna.method {
        AntennaHeightMethod::Vertical | AntennaHeightMethod::PhaseCentre => true,
        // Nothing stated and nothing to reduce: the value is taken as it is.
        // A height with no method could be slant: not guessed.
        AntennaHeightMethod::Unknown => antenna.height == 0.0,
        AntennaHeightMethod::Slant | AntennaHeightMethod::Other => false,
    }
}

/// "1.500 m Vertical (bottom of mount)", for the report.
pub(crate) fn antenna_words(antenna: &GnssAntenna) -> String {
    let mut words = format!(
        "{} m {}",
        format_number(antenna.height, 3),
        antenna.method.text()
    );
    if !antenna.measured_to.is_empty() {
        words.push_str(&format!(" ({})", antenna.measured_to));
    }
    words
}

/// Turns the file's GNSS vectors into grid vectors: a grid baseline as it is,
/// a geocentric one through the context at its base's global position.
/// `rows[i]` is the report row of raw.observations[i]. A vector that cannot be
/// converted is rejected on its row with the reason, never dropped.
pub(crate) fn convert_gnss_vectors<'a>(engine: &mut Engine<'a>, rows: &[usize]) {
    let settings = engine.settings;
    let context = engine.context;
    let raw = engine.raw;
    let observations = &raw.observations;

    // Grid baselines: as the file gave them.
    for (i, observation) in observations.iter().enumerate() {
        let Observation::GnssBaseline(baseline) = observation else {
            continue;
        };
        let row = &mut engine.report.observations[rows[i]];
        row.raw = baseline.delta_northing.hypot(baseline.delta_easting);
        row.reduced = Some(row.raw);
        engine.vectors.push(GridVector {
            from: &baseline.from,
            to: &baseline.to,
            delta_northing: baseline.delta_northing,
            delta_easting: baseline.delta_easting,
            delta_height: Some(baseline.delta_up),
            sigma_northing: baseline.sigma_northing,
            sigma_easting: baseline.sigma_easting,
            sigma_height: baseline.sigma_up,
            row: rows[i],
            source: Some(&baseline.source),
            ..Default::default()
        });
    }

    // Geocentric vectors: the global position of every point that has one.
    // Looked up only, never iterated.
    let mut global: HashMap<&'a str, GeocentricCoordinate> = HashMap::new();
    let mut pending = Vec::new();
    for (i, observation) in observations.iter().enumerate() {
        match observation {
            Observation::GnssGlobalPosition(position) => {
                // A valid observation has exactly one of the two; the C++
                // reads the geodetic one unchecked where neither is set.
                let xyz = match (&position.geocentric, &position.geodetic) {
                    (Some(geocentric), _) => *geocentric,
                    (None, Some(geodetic)) => geocentric_from_geodetic(geodetic),
                    (None, None) => continue,
                };
                global.entry(&position.point).or_insert(xyz);
            }
            Observation::GnssGeocentricBaseline(vector) => {
                pending.push(i);
                let row = &mut engine.report.observations[rows[i]];
                row.raw = (vector.delta.x * vector.delta.x
                    + vector.delta.y * vector.delta.y
                    + vector.delta.z * vector.delta.z)
                    .sqrt();
                row.reduced = Some(row.raw);
            }
            _ => {}
        }
    }
    if pending.is_empty() {
        return;
    }
    // Either conversion will do: a drawing that converts latitude and
    // longitude converts X, Y, Z after GRS80, as the base's own position is.
    if context.geocentric_to_grid.is_none() && context.geodetic_to_grid.is_none() {
        for &i in &pending {
            reject_row(
                engine,
                rows[i],
                "the drawing's coordinate system cannot convert it to grid",
            );
        }
        engine.warn(
            format!(
                "{} GNSS vector(s) were not used: the drawing has no coordinate system to \
                 convert earth-centred vectors to grid. Give the drawing one and adjust again.",
                pending.len()
            ),
            SourceRecord::default(),
        );
        return;
    }

    let mut warned_geoid = false;
    // A vector's base may be the far end of another: convert in passes until
    // nothing more can be.
    let mut progress = true;
    while progress && !pending.is_empty() {
        progress = false;
        let mut still = Vec::new();
        for &i in &pending {
            let Observation::GnssGeocentricBaseline(vector) = &observations[i] else {
                continue;
            };
            let Some(&from) = global.get(vector.from.as_str()) else {
                still.push(i);
                continue;
            };
            progress = true;
            let to = GeocentricCoordinate {
                x: from.x + vector.delta.x,
                y: from.y + vector.delta.y,
                z: from.z + vector.delta.z,
            };
            global.entry(&vector.to).or_insert(to);
            let grid_from = grid_of_geocentric(context, &from);
            let grid_to = grid_of_geocentric(context, &to);
            let (Some(grid_from), Some(grid_to)) = (grid_from, grid_to) else {
                reject_row(
                    engine,
                    rows[i],
                    "the drawing's projection cannot convert it to grid",
                );
                engine.warn(
                    format!(
                        "GNSS vector {} -> {} was not used: the drawing's projection cannot \
                         convert it to grid there.",
                        vector.from, vector.to
                    ),
                    vector.source.clone(),
                );
                continue;
            };

            let mut grid = GridVector {
                from: &vector.from,
                to: &vector.to,
                delta_northing: grid_to.northing - grid_from.northing,
                delta_easting: grid_to.easting - grid_from.easting,
                row: rows[i],
                source: Some(&vector.source),
                ..Default::default()
            };

            {
                let row = &mut engine.report.observations[rows[i]];
                let horizontal = grid.delta_northing.hypot(grid.delta_easting);
                row.corrections.push(AppliedCorrection {
                    kind: CorrectionKind::Other,
                    amount: horizontal - row.raw,
                    factor: None,
                    note: format!(
                        "to grid: dN {} dE {}",
                        format_number(grid.delta_northing, 4),
                        format_number(grid.delta_easting, 4)
                    ),
                });
                row.reduced = Some(horizontal);
            }

            // The height difference mark to mark, as a row of its own, the
            // way a total-station pointing's is.
            let mut height = ReportObservation {
                kind: String::from("GNSS height difference"),
                from: vector.from.clone(),
                to: vector.to.clone(),
                raw: grid_to.ellipsoidal_height - grid_from.ellipsoidal_height,
                source: vector.source.clone(),
                ..Default::default()
            };
            let mut delta_height = height.raw;
            if let Some(geoid_separation) = &context.geoid_separation {
                let separation_from = geoid_separation(grid_from.northing, grid_from.easting);
                let separation_to = geoid_separation(grid_to.northing, grid_to.easting);
                if let (Some(separation_from), Some(separation_to)) =
                    (separation_from, separation_to)
                {
                    let change = -(separation_to - separation_from);
                    height.corrections.push(AppliedCorrection {
                        kind: CorrectionKind::Other,
                        amount: change,
                        factor: None,
                        note: format!(
                            "geoid separation {} to {} m",
                            format_number(separation_from, 3),
                            format_number(separation_to, 3)
                        ),
                    });
                    delta_height += change;
                } else if !warned_geoid {
                    warned_geoid = true;
                    engine.warn(
                        "The drawing gives no geoid separation at some GNSS vectors; their height \
                         differences are ellipsoidal.",
                        vector.source.clone(),
                    );
                }
            } else if !warned_geoid {
                warned_geoid = true;
                engine.warn(
                    "The drawing gives no geoid separation, so GNSS height differences are \
                     ellipsoidal (the geoid's slope over the vector is in them).",
                    SourceRecord::default(),
                );
            }
            if reducible_antenna(&vector.from_antenna) && reducible_antenna(&vector.to_antenna) {
                let antennas = vector.from_antenna.height - vector.to_antenna.height;
                height.corrections.push(AppliedCorrection {
                    kind: CorrectionKind::InstrumentAndTargetHeight,
                    amount: antennas,
                    factor: None,
                    note: format!(
                        "antennas: base {}, rover {}",
                        antenna_words(&vector.from_antenna),
                        antenna_words(&vector.to_antenna)
                    ),
                });
                delta_height += antennas;
                if vector.from_antenna.method == AntennaHeightMethod::Vertical
                    && vector.to_antenna.method == AntennaHeightMethod::Vertical
                    && vector.from_antenna.r#type != vector.to_antenna.r#type
                {
                    engine.warn(
                        format!(
                            "GNSS vector {} -> {}: the antenna heights are to the reference \
                             points of two different antennas, whose phase-centre offsets do not \
                             cancel and are not in the file.",
                            vector.from, vector.to
                        ),
                        vector.source.clone(),
                    );
                }
                height.reduced = Some(delta_height);
                grid.delta_height = Some(delta_height);
            } else {
                height.rejected = true;
                height.rejection_reason = String::from("an antenna height that is not vertical");
                engine.warn(
                    format!(
                        "GNSS vector {} -> {}: an antenna height is not a vertical one (base {}, \
                         rover {}), so its height difference was not used; its horizontal \
                         position was.",
                        vector.from,
                        vector.to,
                        antenna_words(&vector.from_antenna),
                        antenna_words(&vector.to_antenna)
                    ),
                    vector.source.clone(),
                );
            }
            engine.report.observations.push(height);
            grid.height_row = Some(engine.report.observations.len() - 1);

            // Precision: the file's X/Y/Z covariance turned to local north,
            // east and up at the base (the grid's convergence, a degree or
            // two, mixes north and east by less than the covariance is known).
            grid.sigma_northing = settings.apriori.gnss_horizontal;
            grid.sigma_easting = settings.apriori.gnss_horizontal;
            grid.sigma_height = settings.apriori.gnss_vertical;
            if settings.use_file_covariances && vector.covariance.stated() {
                let local = local_variances(&vector.covariance, &geodetic_from_geocentric(&from));
                if local.north > 0.0 && local.east > 0.0 && local.up > 0.0 {
                    grid.sigma_northing = local.north.sqrt();
                    grid.sigma_easting = local.east.sqrt();
                    grid.sigma_height = local.up.sqrt();
                }
            }
            if vector.solution == GnssSolution::Float || vector.solution == GnssSolution::Autonomous
            {
                engine.warn(
                    format!(
                        "GNSS vector {} -> {} is a {} solution.",
                        vector.from,
                        vector.to,
                        vector.solution.text()
                    ),
                    vector.source.clone(),
                );
            }
            engine.vectors.push(grid);
        }
        pending = still;
    }
    // The base's global position must come as a GNSS position observation
    // (or the far end of another vector). A reader that keeps a keyed-in
    // latitude and longitude only as the point's metadata has not handed one
    // over, so the sentence says what is missing from the observations, not
    // that the file has no such position anywhere.
    for &i in &pending {
        let Observation::GnssGeocentricBaseline(vector) = &observations[i] else {
            continue;
        };
        reject_row(
            engine,
            rows[i],
            "its base has no global position given as an observation",
        );
        engine.warn(
            format!(
                "GNSS vector {} -> {} was not used: its base {} has no global position \
                 (latitude, longitude or X, Y, Z) among the observations read, and one is needed \
                 to turn an earth-centred vector into grid. A position the file only keys in for \
                 the base is not yet read as one.",
                vector.from, vector.to, vector.from
            ),
            vector.source.clone(),
        );
    }
}

/// Places the far end of every vector whose base has a position, following
/// chains; a far end already positioned becomes a check. With `reradiate`
/// (after an adjustment moved the bases) only those targets are placed again.
/// Returns whether anything was placed.
pub(crate) fn radiate_gnss_vectors<'a>(
    engine: &mut Engine<'a>,
    reradiate: Option<&HashSet<&'a str>>,
) -> bool {
    let mut placed_any = false;
    let mut progress = true;
    while progress {
        progress = false;
        for v in 0..engine.vectors.len() {
            let vector = engine.vectors[v];
            if engine.report.observations[vector.row].rejected {
                continue;
            }
            let skip = match reradiate {
                Some(reradiate) => !reradiate.contains(vector.to),
                None => vector.radiated,
            };
            if skip {
                continue;
            }
            let Some(base) = engine.find(vector.from).copied() else {
                continue;
            };
            let mut computed = Position {
                northing: base.northing + vector.delta_northing,
                easting: base.easting + vector.delta_easting,
                origin: PositionOrigin::Computed,
                method: ComputationMethod::Gnss,
                ..Default::default()
            };
            if let (Some(base_height), Some(delta_height)) = (base.height, vector.delta_height) {
                computed.height = Some(base_height + delta_height);
            }
            if reradiate.is_some() {
                engine.place(vector.to, computed);
                continue;
            }
            engine.vectors[v].radiated = true;
            progress = true;
            let mut existing = match engine.find(vector.to).copied() {
                Some(existing) if existing.origin != PositionOrigin::FileOnly => existing,
                _ => {
                    engine.place(vector.to, computed);
                    placed_any = true;
                    continue;
                }
            };
            if existing.origin == PositionOrigin::Computed
                && existing.height.is_none()
                && computed.height.is_some()
            {
                existing.height = computed.height;
                if let Some(stored) = engine.find_mut(vector.to) {
                    stored.height = computed.height;
                }
            }
            let northing = computed.northing - existing.northing;
            let easting = computed.easting - existing.easting;
            let linear = northing.hypot(easting);
            let length = vector.delta_northing.hypot(vector.delta_easting);
            engine.report.misclosures.push(MisclosureReport {
                name: format!("{} by GNSS vector from {}", vector.to, vector.from),
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
            });
        }
        if reradiate.is_some() {
            break; // one pass: the bases have already moved
        }
    }
    placed_any
}
