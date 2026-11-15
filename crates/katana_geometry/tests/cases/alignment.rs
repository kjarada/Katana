use crate::support::near;
use katana_core::ErrorCode;
use katana_geometry::Point2;
use katana_geometry::alignment::*;
use katana_math::numerics::{PI, normalize_angle_signed};
fn point(east: f64, north: f64) -> Point2 {
    Point2::new(east, north)
}
fn pi(point: Point2, radius: f64) -> AlignmentPi {
    AlignmentPi {
        point,
        radius,
        ..Default::default()
    }
}
fn three_pis(
    first: Point2,
    middle: Point2,
    last: Point2,
    radius: f64,
    spiral_in: f64,
    spiral_out: f64,
) -> HorizontalAlignment {
    HorizontalAlignment {
        pis: vec![
            pi(first, 0.0),
            AlignmentPi {
                point: middle,
                radius,
                spiral_in,
                spiral_out,
            },
            pi(last, 0.0),
        ],
        start_station: 0.0,
    }
}
fn simple_curve() -> HorizontalAlignment {
    three_pis(
        point(0.0, 0.0),
        point(100.0, 0.0),
        point(100.0, 100.0),
        50.0,
        0.0,
        0.0,
    )
}
fn equal_spirals() -> HorizontalAlignment {
    let deflection = PI / 6.0;
    three_pis(
        point(0.0, 0.0),
        point(400.0, 0.0),
        point(400.0 + 300.0 * deflection.cos(), 300.0 * deflection.sin()),
        300.0,
        90.0,
        90.0,
    )
}
fn unequal_right_turn() -> HorizontalAlignment {
    let deflection = -40.0 * PI / 180.0;
    three_pis(
        point(0.0, 0.0),
        point(300.0, 0.0),
        point(300.0 + 250.0 * deflection.cos(), 250.0 * deflection.sin()),
        200.0,
        60.0,
        40.0,
    )
}
fn kinds(solved: &SolvedAlignment) -> Vec<AlignmentElementKind> {
    solved.elements().iter().map(kind_of).collect()
}
mod alignment {
    use super::*;
    use AlignmentElementKind::{Arc, Spiral, Tangent};
    #[test]
    fn a_simple_curve_reduces_to_the_textbook_tangent_length() {
        let solved = solve_alignment(&simple_curve()).unwrap();
        assert_eq!(kinds(&solved), vec![Tangent, Arc, Tangent]);
        let elements = solved.elements();
        assert_eq!(elements.len(), 3);
        near(elements[0].length, 50.0, 1e-12);
        near(elements[1].length, 50.0 * PI / 2.0, 1e-12);
        near(elements[2].length, 50.0, 1e-12);
        near(solved.length(), 100.0 + 50.0 * PI / 2.0, 1e-12);
        let tangent_start = solved.point_at_station(50.0).unwrap();
        near(tangent_start.x, 50.0, 1e-12);
        near(tangent_start.y, 0.0, 1e-12);
        let middle = solved.point_at_station(50.0 + 25.0 * PI / 2.0).unwrap();
        near(middle.x, 50.0 + 50.0 * (-PI / 4.0).cos(), 1e-9);
        near(middle.y, 50.0 + 50.0 * (-PI / 4.0).sin(), 1e-9);
        near(solved.direction_at_station(50.0).unwrap(), 0.0, 1e-12);
        near(
            solved.direction_at_station(solved.end_station()).unwrap(),
            PI / 2.0,
            1e-9,
        );
        near(
            solved.curvature_at_station(60.0).unwrap(),
            1.0 / 50.0,
            1e-15,
        );
        assert_eq!(solved.curvature_at_station(10.0).unwrap(), 0.0);
    }
    #[test]
    fn equal_spirals_match_the_published_design_formula() {
        let solved = solve_alignment(&equal_spirals()).unwrap();
        assert_eq!(kinds(&solved), vec![Tangent, Spiral, Arc, Spiral, Tangent]);
        let elements = solved.elements();
        assert_eq!(elements.len(), 5);
        near(elements[0].length, 274.347770458528, 1e-9);
        near(elements[1].length, 90.0, 1e-12);
        near(elements[2].length, 67.079632679490, 1e-9);
        near(elements[3].length, 90.0, 1e-12);
        let station = elements[4].start_station;
        let location = solved.point_at_station(station).unwrap();
        near(location.x, 508.818022825068, 1e-9);
        near(location.y, 62.826114770736, 1e-9);
        near(
            solved.direction_at_station(station).unwrap(),
            PI / 6.0,
            1e-9,
        );
        near(
            solved.direction_at_station(solved.end_station()).unwrap(),
            PI / 6.0,
            1e-12,
        );
    }
    #[test]
    fn unequal_spirals_on_a_right_turn_close_on_the_forward_tangent() {
        let solved = solve_alignment(&unequal_right_turn()).unwrap();
        let elements = solved.elements();
        assert_eq!(elements.len(), 5);
        near(elements[0].length, 197.603146939247, 1e-9);
        near(elements[1].length, 60.0, 1e-12);
        near(elements[2].length, 89.626340159546, 1e-9);
        near(elements[3].length, 40.0, 1e-12);
        near(elements[4].length, 250.0 - 93.556128402017, 1e-9);
        let station = elements[4].start_station;
        let location = solved.point_at_station(station).unwrap();
        near(location.x, 371.668152282091, 1e-9);
        near(location.y, -60.136720147060, 1e-9);
        near(
            solved.direction_at_station(station).unwrap(),
            -40.0 * PI / 180.0,
            1e-9,
        );
        near(
            solved
                .curvature_at_station(elements[2].start_station + 1.0)
                .unwrap(),
            -1.0 / 200.0,
            1e-15,
        );
        near(
            solved
                .curvature_at_station(elements[1].start_station + 30.0)
                .unwrap(),
            -0.5 / 200.0,
            1e-15,
        );
    }
    #[test]
    fn every_joint_is_continuous_in_position_and_direction() {
        for definition in [simple_curve(), equal_spirals(), unequal_right_turn()] {
            let solved = solve_alignment(&definition).unwrap();
            let joints = solved.key_stations();
            assert!(joints.len() >= 3);
            let mut checked = 0;
            for &station in &joints[1..joints.len() - 1] {
                let step = 1e-6;
                let before = solved.point_at_station(station - step).unwrap();
                let after = solved.point_at_station(station + step).unwrap();
                near(before.distance_to(after), 2.0 * step, 1e-9);
                let before_direction = solved.direction_at_station(station - step).unwrap();
                let after_direction = solved.direction_at_station(station + step).unwrap();
                near(
                    normalize_angle_signed(after_direction - before_direction),
                    0.0,
                    1e-7,
                );
                checked += 1;
            }
            assert!(checked >= 2);
        }
    }
    #[test]
    fn offsets_are_measured_to_the_left_looking_along_increasing_station() {
        let solved = solve_alignment(&simple_curve()).unwrap();
        let middle = 50.0 + 25.0 * PI / 2.0;
        let left = solved.point_at_station_offset(middle, 5.0).unwrap();
        let right = solved.point_at_station_offset(middle, -5.0).unwrap();
        near(left.distance_to(point(50.0, 50.0)), 45.0, 1e-9);
        near(right.distance_to(point(50.0, 50.0)), 55.0, 1e-9);
        let on_tangent = solved.point_at_station_offset(10.0, 3.0).unwrap();
        near(on_tangent.x, 10.0, 1e-12);
        near(on_tangent.y, 3.0, 1e-12);
    }
    #[test]
    fn the_chorded_polyline_stays_within_tolerance_and_has_no_doubled_joints() {
        let solved = solve_alignment(&simple_curve()).unwrap();
        let epsilon = 0.01;
        let polyline = solved.to_polyline(epsilon);
        assert!(polyline.vertices.len() >= 4);
        assert!(!polyline.closed);
        assert_eq!(polyline.vertices[0], point(0.0, 0.0));
        near(
            polyline
                .vertices
                .last()
                .unwrap()
                .distance_to(point(100.0, 100.0)),
            0.0,
            1e-9,
        );
        for pair in polyline.vertices.windows(2) {
            assert!(pair[0].distance_to(pair[1]) > 1e-9);
        }
        let mut on_arc = 0;
        for pair in polyline.vertices.windows(2) {
            if (pair[0].distance_to(point(50.0, 50.0)) - 50.0).abs() < 1e-9
                && (pair[1].distance_to(point(50.0, 50.0)) - 50.0).abs() < 1e-9
            {
                let middle = (pair[0] + pair[1]) * 0.5;
                assert!((middle.distance_to(point(50.0, 50.0)) - 50.0).abs() <= epsilon * 1.001);
                on_arc += 1;
            }
        }
        assert!(on_arc > 5);
    }
    #[test]
    fn a_radius_of_zero_leaves_a_kink_rather_than_failing() {
        let solved = solve_alignment(&three_pis(
            point(0.0, 0.0),
            point(100.0, 0.0),
            point(100.0, 100.0),
            0.0,
            0.0,
            0.0,
        ))
        .unwrap();
        assert_eq!(solved.elements().len(), 2);
        near(solved.length(), 200.0, 1e-12);
        assert_eq!(solved.key_stations(), vec![0.0, 100.0, 200.0]);
        near(solved.direction_at_station(99.0).unwrap(), 0.0, 1e-12);
        near(solved.direction_at_station(101.0).unwrap(), PI / 2.0, 1e-12);
    }
    #[test]
    fn stations_start_where_the_job_says_and_stop_at_the_ends() {
        let mut definition = simple_curve();
        definition.start_station = 1000.0;
        let solved = solve_alignment(&definition).unwrap();
        assert_eq!(solved.start_station(), 1000.0);
        near(solved.end_station(), 1100.0 + 50.0 * PI / 2.0, 1e-12);
        near(solved.point_at_station(1050.0).unwrap().x, 50.0, 1e-12);
        assert!(solved.point_at_station(999.0).is_none());
        assert!(
            solved
                .point_at_station(solved.end_station() + 1.0)
                .is_none()
        );
        assert!(solved.element_at(500.0).is_none());
        assert!(solved.point_at_station(solved.end_station()).is_some());
    }
    #[test]
    fn refuses_definitions_that_cannot_be_built_and_names_the_pi() {
        let one = HorizontalAlignment {
            pis: vec![pi(point(0.0, 0.0), 0.0)],
            ..Default::default()
        };
        assert_eq!(
            solve_alignment(&one).unwrap_err().code,
            ErrorCode::InvalidGeometry
        );
        let coincident = solve_alignment(&three_pis(
            point(0.0, 0.0),
            point(0.0, 0.0),
            point(100.0, 100.0),
            50.0,
            0.0,
            0.0,
        ))
        .unwrap_err();
        assert_eq!(coincident.code, ErrorCode::InvalidGeometry);
        assert!(coincident.describe().contains("PI 0"));
        let reversal = solve_alignment(&three_pis(
            point(0.0, 0.0),
            point(100.0, 0.0),
            point(0.0, 0.0),
            50.0,
            0.0,
            0.0,
        ))
        .unwrap_err();
        assert_eq!(reversal.code, ErrorCode::InvalidGeometry);
        assert!(reversal.describe().contains("PI 1"));
        let deflection = PI / 6.0;
        let too_much = solve_alignment(&three_pis(
            point(0.0, 0.0),
            point(400.0, 0.0),
            point(400.0 + 300.0 * deflection.cos(), 300.0 * deflection.sin()),
            300.0,
            200.0,
            200.0,
        ))
        .unwrap_err();
        assert_eq!(too_much.code, ErrorCode::InvalidGeometry);
        assert!(too_much.describe().contains("PI 1"));
        let overlap = HorizontalAlignment {
            pis: vec![
                pi(point(0.0, 0.0), 0.0),
                pi(point(100.0, 0.0), 80.0),
                pi(point(100.0, 100.0), 80.0),
                pi(point(200.0, 100.0), 0.0),
            ],
            ..Default::default()
        };
        let error = solve_alignment(&overlap).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(error.describe().contains("PI 1"));
        assert!(error.describe().contains("PI 2"));
        assert_eq!(
            solve_alignment(&three_pis(
                point(0.0, 0.0),
                point(100.0, 0.0),
                point(100.0, 100.0),
                50.0,
                -1.0,
                0.0
            ))
            .unwrap_err()
            .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            solve_alignment(&three_pis(
                point(0.0, 0.0),
                point(100.0, 0.0),
                point(100.0, 100.0),
                f64::NAN,
                0.0,
                0.0
            ))
            .unwrap_err()
            .code,
            ErrorCode::InvalidArgument
        );
    }
    #[test]
    fn back_to_back_curves_are_allowed_when_the_tangent_between_them_is_exactly_used_up() {
        let definition = HorizontalAlignment {
            pis: vec![
                pi(point(0.0, 0.0), 0.0),
                pi(point(100.0, 0.0), 50.0),
                pi(point(100.0, 100.0), 50.0),
                pi(point(200.0, 100.0), 0.0),
            ],
            ..Default::default()
        };
        let solved = solve_alignment(&definition).unwrap();
        assert_eq!(kinds(&solved), vec![Tangent, Arc, Arc, Tangent]);
        for element in solved.elements() {
            assert!(element.length > 1.0);
        }
        let joint = solved.elements()[2].start_station;
        assert!(solved.curvature_at_station(joint - 1.0).unwrap() > 0.0);
        assert!(solved.curvature_at_station(joint + 1.0).unwrap() < 0.0);
    }
    #[test]
    fn a_straight_alignment_gives_the_foots_station_and_the_signed_distance_left_positive() {
        let definition = HorizontalAlignment {
            pis: vec![pi(point(0.0, 0.0), 0.0), pi(point(100.0, 0.0), 0.0)],
            start_station: 1000.0,
        };
        let solved = solve_alignment(&definition).unwrap();
        for (sample, station, offset) in [
            (point(30.0, 5.0), 1030.0, 5.0),
            (point(30.0, -5.0), 1030.0, -5.0),
            (point(0.0, 7.0), 1000.0, 7.0),
            (point(100.0, 0.0), 1100.0, 0.0),
        ] {
            let found = solved.station_offset(sample).unwrap();
            near(found.station, station, 1e-9);
            near(found.offset, offset, 1e-9);
        }
    }
    #[test]
    fn a_point_beyond_either_end_has_no_station_rather_than_the_ends_one() {
        let definition = HorizontalAlignment {
            pis: vec![pi(point(0.0, 0.0), 0.0), pi(point(100.0, 0.0), 0.0)],
            ..Default::default()
        };
        let solved = solve_alignment(&definition).unwrap();
        assert!(solved.station_offset(point(-1.0, 0.0)).is_none());
        assert!(solved.station_offset(point(101.0, 3.0)).is_none());
        assert!(solved.station_offset(point(f64::NAN, 0.0)).is_none());
        assert!(solved.station_offset(point(f64::INFINITY, 0.0)).is_none());
    }
    #[test]
    fn on_a_curve_the_station_is_along_the_arc_and_inside_the_turn_is_left() {
        let solved = solve_alignment(&simple_curve()).unwrap();
        let cosine = (-PI / 4.0).cos();
        let sine = (-PI / 4.0).sin();
        let inside = solved
            .station_offset(point(50.0 + 40.0 * cosine, 50.0 + 40.0 * sine))
            .unwrap();
        near(inside.station, 50.0 + 50.0 * PI / 4.0, 1e-9);
        near(inside.offset, 10.0, 1e-9);
        let outside = solved
            .station_offset(point(50.0 + 60.0 * cosine, 50.0 + 60.0 * sine))
            .unwrap();
        near(outside.station, 50.0 + 50.0 * PI / 4.0, 1e-9);
        near(outside.offset, -10.0, 1e-9);
        let exit = solved.station_offset(point(105.0, 70.0)).unwrap();
        near(exit.station, 50.0 + 50.0 * PI / 2.0 + 20.0, 1e-9);
        near(exit.offset, -5.0, 1e-9);
        let across = solved.station_offset(point(35.0, 64.0)).unwrap();
        near(across.station, 35.0, 1e-9);
        near(across.offset, 64.0, 1e-9);
    }
    #[test]
    fn outside_a_kink_the_point_is_at_the_p_is_station_its_distance_from_the_pi() {
        let solved = solve_alignment(&three_pis(
            point(0.0, 0.0),
            point(100.0, 0.0),
            point(100.0, 100.0),
            0.0,
            0.0,
            0.0,
        ))
        .unwrap();
        let corner = solved.station_offset(point(110.0, -10.0)).unwrap();
        near(corner.station, 100.0, 1e-9);
        near(corner.offset, -10.0 * 2.0_f64.sqrt(), 1e-9);
        let inside = solved.station_offset(point(90.0, 5.0)).unwrap();
        near(inside.station, 90.0, 1e-9);
        near(inside.offset, 5.0, 1e-9);
    }
    #[test]
    fn station_offset_undoes_point_at_station_offset_on_spirals_and_arcs() {
        for definition in [equal_spirals(), unequal_right_turn()] {
            let solved = solve_alignment(&definition).unwrap();
            for element in solved.elements() {
                let station = element.start_station + 0.37 * element.length;
                for offset in [5.0, -5.0] {
                    let sample = solved.point_at_station_offset(station, offset).unwrap();
                    let found = solved.station_offset(sample).unwrap();
                    near(found.station, station, 1e-8);
                    near(found.offset, offset, 1e-8);
                }
            }
        }
    }
}
