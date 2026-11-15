use crate::support::near;
use katana_geometry::curves2d::*;
use katana_geometry::{Point2, Polyline2, Vec2};
use katana_math::numerics::{HALF_PI, PI, TWO_PI};

fn point(east: f64, north: f64) -> Point2 {
    Point2::new(east, north)
}
fn expect_near(actual: Point2, expected: Point2) {
    point_near(actual, expected, 1e-9);
}
fn point_near(actual: Point2, expected: Point2, epsilon: f64) {
    near(actual.x, expected.x, epsilon);
    near(actual.y, expected.y, epsilon);
}
fn arched_square() -> CurvePolyline2 {
    CurvePolyline2 {
        closed: true,
        vertices: vec![
            CurveVertex {
                position: point(0.0, 0.0),
                bulge: 0.0,
                height: Some(1.0),
            },
            CurveVertex {
                position: point(10.0, 0.0),
                bulge: 0.0,
                height: Some(2.0),
            },
            CurveVertex {
                position: point(10.0, 10.0),
                bulge: 1.0,
                height: Some(3.0),
            },
            CurveVertex {
                position: point(0.0, 10.0),
                bulge: 0.0,
                height: Some(4.0),
            },
        ],
    }
}

mod curve_polyline {
    use super::*;
    #[test]
    fn a_positive_unit_bulge_is_a_counter_clockwise_semicircle() {
        let arc = arc_from_bulge(point(0.0, 0.0), point(2.0, 0.0), 1.0).unwrap();
        expect_near(arc.center, point(1.0, 0.0));
        near(arc.radius, 1.0, 1e-12);
        near(arc.sweep, PI, 1e-12);
        expect_near(arc.start_point(), point(0.0, 0.0));
        expect_near(arc.end_point(), point(2.0, 0.0));
        expect_near(arc.midpoint(), point(1.0, -1.0));
    }
    #[test]
    fn a_quarter_bulge_gives_the_circle_through_the_ends() {
        let bulge = (PI / 8.0).tan();
        let arc = arc_from_bulge(point(5.0, 0.0), point(0.0, 5.0), bulge).unwrap();
        expect_near(arc.center, point(0.0, 0.0));
        near(arc.radius, 5.0, 1e-12);
        near(arc.sweep, HALF_PI, 1e-12);
        near(bulge_from_sweep(HALF_PI), bulge, 1e-15);
        near(
            bulge_through(
                point(5.0, 0.0),
                point(5.0 * 0.5_f64.sqrt(), 5.0 * 0.5_f64.sqrt()),
                point(0.0, 5.0),
            ),
            bulge,
            1e-12,
        );
        let other = arc_from_bulge(point(5.0, 0.0), point(0.0, 5.0), -bulge).unwrap();
        expect_near(other.center, point(5.0, 5.0));
        near(other.sweep, -HALF_PI, 1e-12);
    }
    #[test]
    fn a_zero_bulge_or_coincident_ends_is_straight() {
        assert!(arc_from_bulge(point(0.0, 0.0), point(1.0, 0.0), 0.0).is_none());
        assert!(arc_from_bulge(point(1.0, 1.0), point(1.0, 1.0), 0.5).is_none());
        assert_eq!(
            bulge_through(point(0.0, 0.0), point(1.0, 0.0), point(2.0, 0.0)),
            0.0
        );
    }
    #[test]
    fn length_and_area_count_the_arcs() {
        let path = arched_square();
        assert_eq!(path.segment_count(), 4);
        assert!(path.has_arcs());
        assert!(path.is_arc(2));
        assert!(!path.is_arc(0));
        near(path.length(), 30.0 + 5.0 * PI, 1e-9);
        near(path.signed_area(), 100.0 + 0.5 * PI * 25.0, 1e-9);
        let back = path.reversed();
        near(back.signed_area(), -(100.0 + 0.5 * PI * 25.0), 1e-9);
        near(back.length(), path.length(), 1e-9);
        near(back.bounding_box().max.y, 15.0, 1e-9);
    }
    #[test]
    fn the_bounding_box_holds_the_arc_not_only_its_ends() {
        let bounds = arched_square().bounding_box();
        near(bounds.min.x, 0.0, 1e-12);
        near(bounds.max.x, 10.0, 1e-12);
        near(bounds.min.y, 0.0, 1e-12);
        near(bounds.max.y, 15.0, 1e-9);
    }
    #[test]
    fn nearest_finds_stations_on_lines_and_arcs() {
        let path = arched_square();
        let on_base = path.nearest(point(4.0, -3.0)).unwrap();
        expect_near(on_base.point, point(4.0, 0.0));
        near(on_base.station, 4.0, 1e-9);
        assert_eq!(on_base.segment, 0);
        near(on_base.distance, 3.0, 1e-12);
        let crown = path.nearest(point(5.0, 20.0)).unwrap();
        expect_near(crown.point, point(5.0, 15.0));
        near(crown.station, 20.0 + 2.5 * PI, 1e-9);
        expect_near(path.point_at_station(crown.station), point(5.0, 15.0));
    }
    #[test]
    fn heights_interpolate_by_length_along_the_segment() {
        let path = arched_square();
        near(path.height_at_station(2.5).unwrap(), 1.25, 1e-12);
        near(path.height_at_station(20.0 + 2.5 * PI).unwrap(), 3.5, 1e-9);
        let mut gap = path.clone();
        gap.vertices[1].height = None;
        assert!(gap.height_at_station(2.5).is_none());
        assert!(path.has_heights());
        assert!(
            !CurvePolyline2::from_points(&[point(0.0, 0.0), point(1.0, 0.0)], false).has_heights()
        );
    }
    #[test]
    fn tessellation_ends_on_the_vertices_and_holds_the_tolerance() {
        let path = arched_square();
        let chain = path.tessellate(0.01);
        assert!(chain.len() > 6);
        assert_eq!(*chain.first().unwrap(), point(0.0, 0.0));
        assert_eq!(*chain.last().unwrap(), point(0.0, 0.0));
        for &sample in &chain {
            let on_arc = (sample.distance_to(point(5.0, 10.0)) - 5.0).abs();
            let on_side = sample
                .y
                .abs()
                .min(sample.x.abs())
                .min((sample.x - 10.0).abs());
            assert!(on_arc.min(on_side) < 0.011);
        }
        let flat = path.to_polyline(0.01);
        assert!(flat.closed);
        assert_eq!(flat.vertices.len(), chain.len() - 1);
        let square = Polyline2::new(
            vec![point(0.0, 0.0), point(1.0, 0.0), point(1.0, 1.0)],
            true,
        );
        assert_eq!(
            CurvePolyline2::from_polyline(&square, &[]).to_polyline(0.01),
            square
        );
    }
}

mod ellipse {
    use super::*;
    #[test]
    fn points_follow_the_eccentric_anomaly() {
        let ellipse = Ellipse2 {
            center: point(1.0, 2.0),
            major_axis: Vec2::new(4.0, 0.0),
            ratio: 0.5,
            ..Default::default()
        };
        expect_near(ellipse.point_at_parameter(0.0), point(5.0, 2.0));
        expect_near(ellipse.point_at_parameter(HALF_PI), point(1.0, 4.0));
        expect_near(ellipse.point_at_parameter(PI), point(-3.0, 2.0));
        assert!(ellipse.is_full());
        assert_eq!(ellipse.quadrants().len(), 4);
        let bounds = ellipse.bounding_box();
        expect_near(bounds.min, point(-3.0, 0.0));
        expect_near(bounds.max, point(5.0, 4.0));
        near(ellipse.parameter_towards(point(1.0, 10.0)), HALF_PI, 1e-12);
    }
    #[test]
    fn the_length_of_a_circle_and_of_an_ellipse_agree_with_their_formulae() {
        let circle = Ellipse2 {
            major_axis: Vec2::new(3.0, 4.0),
            ratio: 1.0,
            ..Default::default()
        };
        near(circle.length(), TWO_PI * 5.0, 1e-9);
        let mut ellipse = Ellipse2 {
            major_axis: Vec2::new(10.0, 0.0),
            ratio: 0.3,
            ..Default::default()
        };
        let major = 10.0;
        let minor = 3.0;
        let ratio: f64 = (major - minor) * (major - minor) / ((major + minor) * (major + minor));
        let mut sum = 0.0;
        let mut binomial = 1.0;
        for index in 0..200 {
            if index > 0 {
                binomial *= (0.5 - (index - 1) as f64) / index as f64;
            }
            sum += binomial * binomial * ratio.powf(index as f64);
        }
        let exact = PI * (major + minor) * sum;
        near(ellipse.length(), exact, 1e-9);
        ellipse.start_parameter = 0.0;
        ellipse.sweep = HALF_PI;
        near(ellipse.length(), exact / 4.0, 1e-9);
    }
    #[test]
    fn an_arc_has_only_the_quadrants_and_extremes_it_spans() {
        let ellipse = Ellipse2 {
            major_axis: Vec2::new(4.0, 0.0),
            ratio: 0.5,
            start_parameter: 0.0,
            sweep: HALF_PI,
            ..Default::default()
        };
        assert!(!ellipse.is_full());
        assert_eq!(ellipse.quadrants().len(), 2);
        let bounds = ellipse.bounding_box();
        expect_near(bounds.min, point(0.0, 0.0));
        expect_near(bounds.max, point(4.0, 2.0));
        assert!(ellipse.contains_parameter(0.5));
        assert!(!ellipse.contains_parameter(PI));
    }
    #[test]
    fn the_closest_point_is_where_the_normal_passes_through_the_point() {
        let ellipse = Ellipse2 {
            major_axis: Vec2::new(4.0, 0.0),
            ratio: 0.5,
            ..Default::default()
        };
        for sample in [
            point(6.0, 1.0),
            point(-1.0, 5.0),
            point(0.5, 0.2),
            point(-7.0, -3.0),
        ] {
            let parameter = ellipse.closest_parameter(sample);
            let offset = ellipse.point_at_parameter(parameter) - sample;
            near(
                offset.dot(ellipse.derivative_at_parameter(parameter))
                    / ellipse.derivative_at_parameter(parameter).length(),
                0.0,
                1e-9,
            );
            for index in 0..360 {
                assert!(
                    ellipse
                        .point_at_parameter(index as f64 * TWO_PI / 360.0)
                        .distance_to(sample)
                        + 1e-9
                        >= ellipse.distance_to(sample)
                );
            }
        }
    }
    #[test]
    fn from_axes_makes_the_longer_axis_the_major_one() {
        let flat = Ellipse2::from_axes(point(0.0, 0.0), point(10.0, 0.0), 4.0).unwrap();
        near(flat.ratio, 0.4, 1e-15);
        expect_near(flat.major_axis, point(10.0, 0.0));
        let tall = Ellipse2::from_axes(point(0.0, 0.0), point(2.0, 0.0), 5.0).unwrap();
        near(tall.ratio, 0.4, 1e-15);
        near(tall.major_radius(), 5.0, 1e-12);
        expect_near(tall.point_at_parameter(HALF_PI), point(-2.0, 0.0));
        assert!(Ellipse2::from_axes(point(0.0, 0.0), point(0.0, 0.0), 1.0).is_none());
        assert!(Ellipse2::from_axes(point(0.0, 0.0), point(1.0, 0.0), 0.0).is_none());
    }
    #[test]
    fn tessellation_stays_on_the_ellipse() {
        let ellipse = Ellipse2 {
            major_axis: Vec2::new(0.0, 20.0),
            ratio: 0.25,
            ..Default::default()
        };
        let chain = ellipse.tessellate(0.001);
        assert!(chain.len() > 20);
        expect_near(chain[0], ellipse.start_point());
        expect_near(*chain.last().unwrap(), ellipse.start_point());
        for pair in chain.windows(2) {
            let middle = (pair[0] + pair[1]) * 0.5;
            assert!(ellipse.distance_to(middle) < 0.0011);
        }
    }
}

mod spline {
    use super::*;
    #[test]
    fn a_fit_spline_passes_through_its_points() {
        let fit = vec![
            point(0.0, 0.0),
            point(3.0, 4.0),
            point(7.0, 3.0),
            point(10.0, 8.0),
            point(14.0, 5.0),
        ];
        let spline = Spline2::through_points(fit.clone(), 3).unwrap();
        assert!(spline.check_structure().is_ok());
        assert_eq!(spline.degree, 3);
        assert_eq!(spline.fit_points, fit);
        assert_eq!(spline.control_points.len(), fit.len());
        expect_near(spline.start_point(), fit[0]);
        expect_near(spline.end_point(), *fit.last().unwrap());
        for sample in fit {
            assert!(spline.distance_to(sample) < 2e-4);
        }
    }
    #[test]
    fn two_points_give_a_line_and_three_a_parabola() {
        let line = Spline2::through_points(vec![point(0.0, 0.0), point(4.0, 2.0)], 3).unwrap();
        assert_eq!(line.degree, 1);
        expect_near(line.point_at(0.5), point(2.0, 1.0));
        let parabola =
            Spline2::through_points(vec![point(-1.0, 1.0), point(0.0, 0.0), point(1.0, 1.0)], 3)
                .unwrap();
        assert_eq!(parabola.degree, 2);
        point_near(parabola.point_at(0.5), point(0.0, 0.0), 1e-12);
        assert!(Spline2::through_points(vec![point(1.0, 1.0), point(1.0, 1.0)], 3).is_err());
    }
    #[test]
    fn a_control_point_spline_starts_and_ends_on_its_polygon() {
        let spline = Spline2::from_control_points(
            vec![
                point(0.0, 0.0),
                point(0.0, 10.0),
                point(10.0, 10.0),
                point(10.0, 0.0),
            ],
            3,
        )
        .unwrap();
        assert!(!spline.has_fit_points());
        expect_near(spline.start_point(), point(0.0, 0.0));
        expect_near(spline.end_point(), point(10.0, 0.0));
        expect_near(spline.point_at(0.5), point(5.0, 7.5));
        assert!(spline.bounding_box().contains(spline.point_at(0.3)));
    }
    #[test]
    fn a_rational_quadratic_draws_an_exact_quarter_circle() {
        let arc = Spline2 {
            degree: 2,
            control_points: vec![point(1.0, 0.0), point(1.0, 1.0), point(0.0, 1.0)],
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            weights: vec![1.0, 0.5_f64.sqrt(), 1.0],
            ..Default::default()
        };
        assert!(arc.check_structure().is_ok());
        for index in 0..=10 {
            near(arc.point_at(index as f64 / 10.0).length(), 1.0, 1e-12);
        }
        near(arc.length(), HALF_PI, 1e-6);
    }
    #[test]
    fn reversing_walks_the_same_curve_backwards() {
        let spline = Spline2::through_points(
            vec![
                point(0.0, 0.0),
                point(2.0, 3.0),
                point(5.0, 1.0),
                point(8.0, 4.0),
            ],
            3,
        )
        .unwrap();
        let back = spline.reversed();
        assert!(back.check_structure().is_ok());
        for index in 0..=8 {
            let parameter = index as f64 / 8.0;
            point_near(
                back.point_at(
                    back.domain_start() + (back.domain_end() - back.domain_start()) * parameter,
                ),
                spline.point_at(
                    spline.domain_end() - (spline.domain_end() - spline.domain_start()) * parameter,
                ),
                1e-9,
            );
        }
    }
    #[test]
    fn bad_structure_is_refused_not_evaluated() {
        let mut bad = Spline2 {
            degree: 3,
            control_points: vec![point(0.0, 0.0), point(1.0, 1.0)],
            knots: vec![0.0, 0.0, 1.0, 1.0],
            ..Default::default()
        };
        assert!(bad.check_structure().is_err());
        assert!(bad.tessellate(0.01).is_empty());
        bad.degree = 1;
        bad.knots = vec![0.0, 0.0, 1.0, 1.0];
        bad.weights = vec![1.0, -1.0];
        assert!(bad.check_structure().is_err());
        bad.weights.clear();
        bad.knots = vec![0.0, 1.0, 0.5, 1.0];
        assert!(bad.check_structure().is_err());
    }
    #[test]
    fn tessellation_holds_the_tolerance() {
        let spline = Spline2::through_points(
            vec![
                point(0.0, 0.0),
                point(10.0, 20.0),
                point(20.0, -20.0),
                point(30.0, 20.0),
                point(40.0, 0.0),
            ],
            3,
        )
        .unwrap();
        let coarse = spline.tessellate(0.1);
        let fine = spline.tessellate(0.001);
        assert!(fine.len() > coarse.len());
        for pair in coarse.windows(2) {
            assert!(spline.distance_to((pair[0] + pair[1]) * 0.5) < 0.11);
        }
    }
}
