mod spiral2 {
    use crate::support::{double_eq, near};
    use katana_geometry::{Point2, Spiral2};
    use katana_math::numerics::PI;
    #[test]
    fn reproduces_the_published_fresnel_integrals() {
        let spiral = Spiral2::from_straight_to_radius(Point2::new(0.0, 0.0), 0.0, 1.0 / PI, 1.0);
        let half = spiral.point_at(0.5);
        near(half.x, 0.4923442258714464, 1e-14);
        near(half.y, 0.0647324328599993, 1e-14);
        let one = spiral.point_at(1.0);
        near(one.x, 0.7798934003768228, 1e-14);
        near(one.y, 0.4382591473903548, 1e-14);
        let two = spiral.point_at(2.0);
        near(two.x, 0.4882534060753407, 1e-14);
        near(two.y, 0.3434156783636982, 1e-14);
    }
    #[test]
    fn matches_the_highway_design_series_on_a_real_transition() {
        let spiral = Spiral2::from_straight_to_radius(Point2::new(0.0, 0.0), 0.0, 300.0, 90.0);
        let end = spiral.end_point();
        near(end.x, 89.797710828008, 1e-11);
        near(end.y, 4.492773032666, 1e-11);
        near(spiral.total_turn(), 0.15, 1e-15);
        near(spiral.flatness(), (300.0_f64 * 90.0).sqrt(), 1e-12);
    }
    #[test]
    fn handles_the_curve_to_curve_transition_the_textbook_form_has_no_formula_for() {
        let spiral = Spiral2 {
            start: Point2::new(0.0, 0.0),
            start_direction: PI / 6.0,
            start_curvature: 1.0 / 200.0,
            end_curvature: 1.0 / 100.0,
            length: 60.0,
        };
        let end = spiral.end_point();
        near(end.x, 44.587806890670, 1e-11);
        near(end.y, 39.384588253296, 1e-11);
        near(spiral.total_turn(), 0.45, 1e-15);
    }
    #[test]
    fn a_reverse_spiral_through_zero_curvature_is_still_integrated_finely_enough() {
        let spiral = Spiral2 {
            start: Point2::new(0.0, 0.0),
            start_curvature: 1.0 / 100.0,
            end_curvature: -1.0 / 100.0,
            length: 80.0,
            ..Default::default()
        };
        assert_eq!(spiral.total_turn(), 0.0);
        let end = spiral.end_point();
        near(end.x, 79.148831439120, 1e-11);
        near(end.y, 10.617983504393, 1e-11);
    }
    #[test]
    fn constant_curvature_is_exactly_a_circular_arc() {
        let spiral = Spiral2 {
            start: Point2::new(0.0, 0.0),
            start_curvature: 1.0 / 50.0,
            end_curvature: 1.0 / 50.0,
            length: 50.0 * PI / 2.0,
            ..Default::default()
        };
        assert!(spiral.is_degenerate());
        let end = spiral.end_point();
        near(end.x, 50.0, 1e-12);
        near(end.y, 50.0, 1e-12);
        let arc = spiral.as_arc().unwrap();
        near(arc.center.x, 0.0, 1e-12);
        near(arc.center.y, 50.0, 1e-12);
        double_eq(arc.radius, 50.0);
        near(arc.sweep, PI / 2.0, 1e-15);
        for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let from_arc = arc.point_at(fraction);
            let from_spiral = spiral.point_at(fraction * spiral.length);
            near(from_arc.x, from_spiral.x, 1e-12);
            near(from_arc.y, from_spiral.y, 1e-12);
        }
    }
    #[test]
    fn a_right_hand_arc_puts_its_centre_on_the_right() {
        let spiral = Spiral2 {
            start: Point2::new(10.0, 20.0),
            start_direction: PI / 2.0,
            start_curvature: -1.0 / 8.0,
            end_curvature: -1.0 / 8.0,
            length: 8.0 * PI,
        };
        let arc = spiral.as_arc().unwrap();
        near(arc.center.x, 18.0, 1e-12);
        near(arc.center.y, 20.0, 1e-12);
        near(arc.sweep, -PI, 1e-15);
    }
    #[test]
    fn zero_curvature_throughout_is_a_straight_line() {
        let spiral = Spiral2 {
            start: Point2::new(3.0, 4.0),
            start_direction: 4.0_f64.atan2(3.0),
            length: 10.0,
            ..Default::default()
        };
        let end = spiral.end_point();
        near(end.x, 3.0 + 6.0, 1e-13);
        near(end.y, 4.0 + 8.0, 1e-13);
        assert!(spiral.as_arc().is_none());
        assert_eq!(spiral.chord_count_for(0.001), 2);
        assert!(spiral.radius_at(5.0).is_infinite());
    }
    #[test]
    fn the_tangent_is_the_derivative_of_the_position() {
        let spiral = Spiral2::from_straight_to_radius(Point2::new(0.0, 0.0), 0.3, 150.0, 60.0);
        let step = 1e-4;
        for station in [5.0, 20.0, 45.0, 60.0] {
            let ahead = spiral.point_at(station + step);
            let behind = spiral.point_at(station - step);
            let tangent = spiral.tangent_at(station);
            near((ahead.x - behind.x) / (2.0 * step), tangent.x, 1e-7);
            near((ahead.y - behind.y) / (2.0 * step), tangent.y, 1e-7);
            near(tangent.length(), 1.0, 1e-15);
        }
    }
    #[test]
    fn a_mirrored_spiral_is_exactly_the_mirror_image() {
        let left = Spiral2::from_straight_to_radius(Point2::new(0.0, 0.0), 0.0, 300.0, 90.0);
        let right = Spiral2::from_straight_to_radius(Point2::new(0.0, 0.0), 0.0, -300.0, 90.0);
        for station in [10.0, 37.5, 90.0] {
            let first = left.point_at(station);
            let second = right.point_at(station);
            assert_eq!(first.x, second.x);
            assert_eq!(first.y, -second.y);
        }
    }
    #[test]
    fn curvature_and_direction_are_exactly_linear_and_quadratic() {
        let spiral = Spiral2 {
            start_curvature: 0.0,
            end_curvature: 1.0 / 256.0,
            length: 128.0,
            ..Default::default()
        };
        assert_eq!(spiral.curvature_at(64.0), 1.0 / 512.0);
        assert_eq!(spiral.direction_at(128.0), 0.25);
        assert_eq!(spiral.total_turn(), 0.25);
        assert_eq!(spiral.radius_at(128.0), 256.0);
    }
    #[test]
    fn chord_count_tightens_with_tolerance_and_never_falls_below_two() {
        let spiral = Spiral2::from_straight_to_radius(Point2::new(0.0, 0.0), 0.0, 300.0, 90.0);
        let coarse = spiral.chord_count_for(0.1);
        let fine = spiral.chord_count_for(0.001);
        assert!(coarse >= 2);
        assert!(fine > coarse);
        assert_eq!(fine, 60);
    }
}
