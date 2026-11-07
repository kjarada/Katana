//! Twin of tests/survey/test_network_adjustment.cpp.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]

mod survey_support;

use katana_survey::{
    ControlPoint, DistanceKind, DistanceObservation, HorizontalDirectionObservation,
    LevelDifferenceObservation, SurveyNetwork, SurveyPoint,
};

fn point(id: &str, elevation: Option<f64>) -> SurveyPoint {
    SurveyPoint {
        id: String::from(id),
        elevation,
        ..Default::default()
    }
}

fn level(from: &str, to: &str, rise: f64, sigma: f64) -> LevelDifferenceObservation {
    LevelDifferenceObservation {
        from: String::from(from),
        to: String::from(to),
        height_difference: rise,
        sigma,
        ..Default::default()
    }
}

// Benchmark A held at 100, and B levelled from it twice: rises of 1.0 and 1.5,
// each with sigma 0.5. Worked by hand: equal weights, so B is the plain mean,
// 100 + 1.25 = 101.25; the residuals (adjusted - observed) are +0.25 and
// -0.25; v^T P v = 2 * 0.25^2 / 0.5^2 = 0.5 on one degree of freedom. Every
// number is exact in binary.
fn two_runs(
    height_of_a: Option<f64>,
    start_of_b: Option<f64>,
    control: ControlPoint,
) -> SurveyNetwork {
    let mut network = SurveyNetwork::default();
    assert!(network.add_point(point("A", height_of_a)).is_ok());
    assert!(network.add_point(point("B", start_of_b)).is_ok());
    assert!(network.add_control_point(control).is_ok());
    assert!(network.add_observation(level("A", "B", 1.0, 0.5)).is_ok());
    assert!(network.add_observation(level("A", "B", 1.5, 0.5)).is_ok());
    network
}

mod survey_level_network {
    use super::*;
    use crate::survey_support::double_eq;
    use katana_core::ErrorCode;
    use katana_survey::AdjustmentOptions;
    use katana_survey::network_adjustment::adjust_level_network;

    #[test]
    fn two_runs_from_a_held_benchmark_average_to_the_hand_worked_height() {
        let result = adjust_level_network(
            &two_runs(Some(100.0), Some(101.0), ControlPoint::fixed_vertical("A")),
            &AdjustmentOptions::default(),
        );
        let result = match result {
            Ok(result) => result,
            Err(error) => panic!("{}", error.message),
        };
        assert_eq!(result.elevations.len(), 2);
        assert_eq!(result.elevations[0].point_id, "A");
        assert_eq!(result.elevations[0].elevation, 100.0);
        assert_eq!(result.elevations[1].point_id, "B");
        double_eq(result.elevations[1].elevation, 101.25);
        assert_eq!(result.residuals.len(), 2);
        double_eq(result.residuals[0].residual, 0.25);
        double_eq(result.residuals[1].residual, -0.25);
        assert_eq!(result.statistics.degrees_of_freedom, 1);
        double_eq(result.statistics.weighted_squared_residuals, 0.5);
    }

    #[test]
    fn a_free_point_with_no_height_adjusts_to_the_same_answer() {
        // The model is linear, so where B starts is irrelevant - including when
        // the source never gave it a height at all.
        let result = adjust_level_network(
            &two_runs(Some(100.0), None, ControlPoint::fixed_vertical("A")),
            &AdjustmentOptions::default(),
        );
        let result = match result {
            Ok(result) => result,
            Err(error) => panic!("{}", error.message),
        };
        double_eq(result.elevations[1].elevation, 101.25);
    }

    #[test]
    fn a_held_benchmark_with_no_height_is_refused_rather_than_taken_as_zero() {
        // Taken as zero it would level B to 1.25 and report a perfect adjustment
        // onto the wrong datum.
        let held = adjust_level_network(
            &two_runs(None, Some(101.0), ControlPoint::fixed_vertical("A")),
            &AdjustmentOptions::default(),
        );
        let error = held.unwrap_err();
        assert_eq!(error.code, ErrorCode::AdjustmentFailure);
        assert!(error.message.contains("'A'"), "{}", error.message);

        let weighted = adjust_level_network(
            &two_runs(
                None,
                Some(101.0),
                ControlPoint::weighted_vertical("A", 0.001),
            ),
            &AdjustmentOptions::default(),
        );
        assert_eq!(weighted.unwrap_err().code, ErrorCode::AdjustmentFailure);
    }
}

mod survey_point_height {
    use super::*;
    use katana_core::ErrorCode;
    use katana_survey::SurveyProject;
    use katana_survey::data_model::validate_project;

    #[test]
    fn an_absent_height_is_valid_and_a_non_finite_one_is_not() {
        let mut project = SurveyProject {
            points: vec![point("NOZ", None)],
            ..Default::default()
        };
        assert!(validate_project(&project).is_ok());

        project.points.push(point("BAD", Some(f64::INFINITY)));
        let status = validate_project(&project);
        assert_eq!(status.unwrap_err().code, ErrorCode::InvalidArgument);

        let mut network = SurveyNetwork::default();
        assert!(network.add_point(point("NOZ", None)).is_ok());
        assert!(network.add_point(point("NAN", Some(f64::NAN))).is_err());
    }
}

mod survey_units {
    use katana_core::ErrorCode;
    use katana_math::UnitRatio;
    use katana_survey::LinearUnit;
    use katana_survey::data_model::metres_per;

    #[test]
    fn each_declared_unit_is_its_statutory_ratio_and_unknown_is_refused() {
        assert!(metres_per(LinearUnit::Metres).is_ok());
        assert_eq!(
            metres_per(LinearUnit::Metres).unwrap(),
            UnitRatio::new(1, 1)
        );
        assert_eq!(
            metres_per(LinearUnit::Feet).unwrap(),
            UnitRatio::new(381, 1250)
        );
        assert_eq!(
            metres_per(LinearUnit::UsSurveyFeet).unwrap(),
            UnitRatio::new(1200, 3937)
        );
        assert_eq!(
            metres_per(LinearUnit::Links).unwrap(),
            UnitRatio::new(12573, 62500)
        );
        let unknown = metres_per(LinearUnit::Unknown);
        assert_eq!(unknown.unwrap_err().code, ErrorCode::InvalidArgument);
    }
}

mod survey_unpositioned_points {
    use super::*;
    use katana_core::ErrorCode;
    use katana_survey::data_model::validate_project;
    use katana_survey::{
        HorizontalAngleObservation, Station, SurveyFeature, SurveyProject, SurveyStation,
        UnpositionedPoint,
    };

    #[test]
    fn a_reference_may_resolve_to_a_point_the_source_gave_no_coordinates_for() {
        // A raw file: setup on 1, a shot to 2 whose coordinates the file never
        // gives.
        let occupied = SurveyPoint {
            id: String::from("1"),
            ..Default::default()
        };
        let target = UnpositionedPoint {
            id: String::from("2"),
            code: String::from("TREE"),
            ..Default::default()
        };
        // Built as the C++ builds it and, as there, never added to the station.
        let _angle = HorizontalAngleObservation {
            at: String::from("1"),
            from: String::from("2"),
            to: String::from("2"),
            ..Default::default()
        };
        let distance = DistanceObservation {
            from: String::from("1"),
            to: String::from("2"),
            distance: 12.5,
            sigma: 0.002,
            ..Default::default()
        };
        let station = SurveyStation {
            setup: Station {
                id: String::from("S1"),
                point_id: String::from("1"),
                ..Default::default()
            },
            observations: vec![distance.into()],
            ..Default::default()
        };
        let line = SurveyFeature {
            code: String::from("TREE"),
            point_ids: vec![String::from("1"), String::from("2")],
            ..Default::default()
        };
        let mut project = SurveyProject {
            points: vec![occupied],
            unpositioned_points: vec![target],
            stations: vec![station],
            features: vec![line],
            ..Default::default()
        };
        let status = validate_project(&project);
        assert!(status.is_ok(), "{:?}", status.err().map(|e| e.message));

        // One id, two lists: refused, since a point cannot both have a position
        // and not.
        project.points.push(SurveyPoint {
            id: String::from("2"),
            ..Default::default()
        });
        let both = validate_project(&project);
        assert_eq!(both.unwrap_err().code, ErrorCode::AlreadyExists);

        project.points.pop();
        project
            .unpositioned_points
            .push(UnpositionedPoint::default());
        assert_eq!(
            validate_project(&project).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
}

// ---- Directions: one set per point, one orientation unknown ----------------------

const DEGREE: f64 = 3.14159265358979323846 / 180.0;

fn placed(id: &str, northing: f64, easting: f64) -> SurveyPoint {
    SurveyPoint {
        id: String::from(id),
        northing,
        easting,
        ..Default::default()
    }
}

fn direction(at: &str, to: &str, reading: f64, sigma: f64) -> HorizontalDirectionObservation {
    HorizontalDirectionObservation {
        at: String::from(at),
        to: String::from(to),
        direction: reading,
        sigma,
        ..Default::default()
    }
}

// A, B and C held 100 m north, east and south of P, and P (really at the
// origin) started at N 3, E -2. Read at P with the circle turned 30 degrees:
// reading = azimuth - 30, so A (azimuth 0) reads 330, B (90) 60, C (180) 150.
fn free_station(sigma_direction: f64) -> SurveyNetwork {
    let mut network = SurveyNetwork::default();
    assert!(network.add_point(placed("P", 3.0, -2.0)).is_ok());
    assert!(network.add_point(placed("A", 100.0, 0.0)).is_ok());
    assert!(network.add_point(placed("B", 0.0, 100.0)).is_ok());
    assert!(network.add_point(placed("C", -100.0, 0.0)).is_ok());
    for id in ["A", "B", "C"] {
        assert!(
            network
                .add_control_point(ControlPoint::fixed_horizontal(id))
                .is_ok()
        );
    }
    assert!(
        network
            .add_observation(direction("P", "A", 330.0 * DEGREE, sigma_direction))
            .is_ok()
    );
    assert!(
        network
            .add_observation(direction("P", "B", 60.0 * DEGREE, sigma_direction))
            .is_ok()
    );
    assert!(
        network
            .add_observation(direction("P", "C", 150.0 * DEGREE, sigma_direction))
            .is_ok()
    );
    network
}

fn horizontal_distance(from: &str, to: &str, metres: f64) -> DistanceObservation {
    DistanceObservation {
        from: String::from(from),
        to: String::from(to),
        distance: metres,
        sigma: 0.002,
        kind: DistanceKind::Horizontal,
        ..Default::default()
    }
}

mod survey_horizontal_network {
    use super::*;
    use crate::survey_support::near;
    use katana_core::ErrorCode;
    use katana_survey::network_adjustment::adjust_horizontal_network;
    use katana_survey::{AdjustedParameter, AdjustmentOptions, CoordinateComponent};

    #[test]
    fn three_directions_fix_a_free_point_and_the_orientation_of_its_set() {
        // Three readings for three unknowns (N, E and the set's orientation): the
        // point and the turn of the circle come back exactly, with nothing left
        // to check them.
        let result =
            match adjust_horizontal_network(&free_station(1e-5), &AdjustmentOptions::default()) {
                Ok(result) => result,
                Err(error) => panic!("{}", error.describe()),
            };
        assert_eq!(result.parameters.len(), 3);
        assert_eq!(
            result.parameters[2],
            AdjustedParameter {
                point_id: String::from("P"),
                component: CoordinateComponent::Orientation
            }
        );
        assert!(result.unused_observations.is_empty());
        assert_eq!(result.statistics.degrees_of_freedom, 0);
        let p = &result.stations[0];
        assert_eq!(p.point_id, "P");
        near(p.position.northing, 0.0, 1e-9);
        near(p.position.easting, 0.0, 1e-9);
        assert_eq!(result.orientations.len(), 1);
        assert_eq!(result.orientations[0].point_id, "P");
        near(result.orientations[0].orientation, 30.0 * DEGREE, 1e-12);
        for residual in &result.residuals {
            near(residual.residual, 0.0, 1e-12);
        }
    }

    #[test]
    fn a_direction_set_with_distances_is_adjusted_to_an_independent_solution() {
        // The same three readings, and distances 3 mm long to A, 2 mm short to B
        // and exact to C, each to 2 mm; the readings to 1e-5 rad. Worked apart
        // from this code, by the normal equations of the same model solved by
        // Cramer's rule and iterated to 1e-13 m (a scratch script, not Katana):
        //   N -0.000642851429, E 0.000222220744, orientation 29.999877224694 deg,
        //   residuals (computed - observed) of the readings -0.0164", -0.8840",
        //   +0.9004", of the distances -2.3571, +1.7778, -0.6429 mm;
        //   v'Pv 2.65674994700 on 3 degrees of freedom.
        // By hand, roughly: A and C pull P 1.5 mm south with weight 2 / 0.002^2
        // = 5e5, the reading to B holds it where it is with (0.01 / 1e-5)^2 * 2/3
        // = 6.7e5 (the orientation takes a third), so it moves 1.5 * 5 / 11.7 =
        // 0.64 mm south.
        let mut network = free_station(1e-5);
        assert!(
            network
                .add_observation(horizontal_distance("P", "A", 100.003))
                .is_ok()
        );
        assert!(
            network
                .add_observation(horizontal_distance("P", "B", 99.998))
                .is_ok()
        );
        assert!(
            network
                .add_observation(horizontal_distance("P", "C", 100.000))
                .is_ok()
        );
        let result = match adjust_horizontal_network(&network, &AdjustmentOptions::default()) {
            Ok(result) => result,
            Err(error) => panic!("{}", error.describe()),
        };
        assert_eq!(result.statistics.degrees_of_freedom, 3);
        let p = &result.stations[0];
        near(p.position.northing, -0.000642851429, 1e-9);
        near(p.position.easting, 0.000222220744, 1e-9);
        assert_eq!(result.orientations.len(), 1);
        near(
            result.orientations[0].orientation,
            29.999877224694 * DEGREE,
            1e-11,
        );
        assert_eq!(result.residuals.len(), 6);
        let arc_second = DEGREE / 3600.0;
        near(
            result.residuals[0].residual,
            -0.0164 * arc_second,
            0.0001 * arc_second,
        );
        near(
            result.residuals[1].residual,
            -0.8840 * arc_second,
            0.0001 * arc_second,
        );
        near(
            result.residuals[2].residual,
            0.9004 * arc_second,
            0.0001 * arc_second,
        );
        near(result.residuals[3].residual, -0.0023571, 1e-7);
        near(result.residuals[4].residual, 0.0017778, 1e-7);
        near(result.residuals[5].residual, -0.0006429, 1e-7);
        near(
            result.statistics.weighted_squared_residuals,
            2.65674994700,
            1e-8,
        );
        assert!(result.orientations[0].sigma > 0.0);
    }

    #[test]
    fn readings_either_side_of_zero_are_one_set() {
        // The circle turned 0.1 degrees: A, due north, reads 359.9, B 89.9 and C
        // 179.9 - one set with readings either side of zero. Azimuth less reading
        // is -359.9 for A and +0.1 for the others; the set's start is wrapped
        // about the first, so the orientation comes out 0.1 degrees, not half a
        // turn away.
        let mut network = SurveyNetwork::default();
        assert!(network.add_point(placed("P", 1.0, 1.0)).is_ok());
        assert!(network.add_point(placed("A", 100.0, 0.0)).is_ok());
        assert!(network.add_point(placed("B", 0.0, 100.0)).is_ok());
        assert!(network.add_point(placed("C", -100.0, 0.0)).is_ok());
        for id in ["A", "B", "C"] {
            assert!(
                network
                    .add_control_point(ControlPoint::fixed_horizontal(id))
                    .is_ok()
            );
        }
        for (to, reading) in [("A", 359.9), ("B", 89.9), ("C", 179.9)] {
            assert!(
                network
                    .add_observation(direction("P", to, reading * DEGREE, 1e-5))
                    .is_ok()
            );
        }
        let result = match adjust_horizontal_network(&network, &AdjustmentOptions::default()) {
            Ok(result) => result,
            Err(error) => panic!("{}", error.describe()),
        };
        near(result.stations[0].position.northing, 0.0, 1e-9);
        near(result.stations[0].position.easting, 0.0, 1e-9);
        assert_eq!(result.orientations.len(), 1);
        near(result.orientations[0].orientation, 0.1 * DEGREE, 1e-12);
    }

    #[test]
    fn a_point_on_the_circle_through_its_three_targets_is_rank_deficient() {
        // The danger circle: P at N 0, E -100 is on the circle through A, B and C
        // (radius 100 about the origin). Along the circle every angle between the
        // targets stays the same, so a move along it is a turn of the set, and
        // the readings cannot tell the two apart. Readings from P with the circle
        // at north: A at atan2(100, 100) = 45, B at atan2(200, 0) = 90, C at
        // atan2(100, -100) = 135 degrees; P started exactly there.
        let mut network = SurveyNetwork::default();
        assert!(network.add_point(placed("P", 0.0, -100.0)).is_ok());
        assert!(network.add_point(placed("A", 100.0, 0.0)).is_ok());
        assert!(network.add_point(placed("B", 0.0, 100.0)).is_ok());
        assert!(network.add_point(placed("C", -100.0, 0.0)).is_ok());
        for id in ["A", "B", "C"] {
            assert!(
                network
                    .add_control_point(ControlPoint::fixed_horizontal(id))
                    .is_ok()
            );
        }
        for (to, reading) in [("A", 45.0), ("B", 90.0), ("C", 135.0)] {
            assert!(
                network
                    .add_observation(direction("P", to, reading * DEGREE, 1e-5))
                    .is_ok()
            );
        }
        let error = adjust_horizontal_network(&network, &AdjustmentOptions::default()).unwrap_err();
        assert_eq!(error.code, ErrorCode::AdjustmentFailure);
        assert!(
            error.message.contains("rank deficient"),
            "{}",
            error.message
        );
        // What it cannot resolve are unknowns, not all of them coordinates: the
        // orientation of the set is one, and is named as such.
        assert!(
            error.message.contains("unknowns that cannot be resolved: "),
            "{}",
            error.message
        );
        assert!(
            !error
                .message
                .contains("coordinates that cannot be resolved"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("orientation(P)"),
            "{}",
            error.message
        );
    }
}
