//! tests/geo/test_viewshed_and_los.cpp: the LineOfSight suite, the native
//! line of sight (katana_terrain::line_of_sight; docs/terrain.md "Viewshed
//! and line of sight").
//!
//! Every fixture is flat ground with, where there is one, a wall: the
//! hand-worked shadow is then similar triangles. Curvature is off (the
//! options' default) wherever a shadow's edge is worked by hand.

mod line_of_sight {
    use katana_core::ErrorCode;
    use katana_geometry::Point2;
    use katana_terrain::{EARTH_DIAMETER, SightOptions, line_of_sight};

    use crate::{double_eq, near};

    fn p(x: f64, y: f64) -> Point2 {
        Point2::new(x, y)
    }

    #[test]
    fn los_on_flat_ground_is_visible_with_clearance_equal_to_the_observer_height() {
        // Ground at 50; the eye and the aim both 1.7 m up: the sight is level,
        // 1.7 m over every station, exactly.
        let ground = |_: Point2| Some(50.0);
        let options = SightOptions {
            observer_height: 1.7,
            target_height: 1.7,
            ..SightOptions::default()
        };
        let line = match line_of_sight(&ground, p(0.0, 0.0), p(100.0, 0.0), &options, None) {
            Ok(line) => line,
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(line.visible);
        double_eq(line.distance, 100.0);
        double_eq(line.observer_z, 51.7);
        let clearance = line.clearance.expect("a clearance");
        near(clearance, 1.7, 1e-12);
        assert_eq!(line.stations, 99);
        assert!(line.blocked_at.is_none());
    }

    #[test]
    fn los_over_the_ridge_is_blocked_at_the_ridge() {
        // A 5 m ridge from x = 19.4 to 20.6; the eye 1.7 m over flat ground
        // at 0, the target on the ground 40 m off. Stations every 0.25 m: the
        // first on the ridge is x = 19.5, where the sight is 1.7 x (1 - 19.5 /
        // 40) = 0.87125 high, 4.12875 under the ridge top. The least clearance
        // is at the far side, 20.5 (the last station on it): 1.7 x (1 - 20.5 /
        // 40) - 5 = -4.17125.
        let ground = |at: Point2| {
            Some(if at.x >= 19.4 && at.x <= 20.6 {
                5.0
            } else {
                0.0
            })
        };
        let options = SightOptions {
            step: 0.25,
            ..SightOptions::default()
        };
        let line = line_of_sight(&ground, p(0.0, 0.0), p(40.0, 0.0), &options, None)
            .expect("a sight line");
        assert!(!line.visible);
        let blocked_at = line.blocked_at.expect("blocked");
        near(blocked_at.x, 19.5, 1e-12);
        near(line.blocked_distance.expect("blocked"), 19.5, 1e-12);
        double_eq(line.blocked_ground.expect("blocked"), 5.0);
        near(line.clearance.expect("a clearance"), -4.17125, 1e-12);
        near(line.clearance_at.expect("a clearance").x, 20.5, 1e-12);
    }

    #[test]
    fn behind_a_fence_the_target_is_seen_again_where_similar_triangles_say() {
        // A 1 m fence from x = 19.5 to 20.5, the eye 1.7 m up at 0, targets on
        // the ground. The sight grazing the fence's far top edge, (20.5, 1),
        // meets the ground at 20.5 x 1.7 / 0.7 = 49.786: a target at 49 m is
        // hidden, one at 50.5 m seen (its sight clears the edge by
        // 1.7 x (1 - 20.5 / 50.5) - 1 = 0.0099 m, the station nearest the edge
        // by more, being nearer the eye).
        let ground = |at: Point2| {
            Some(if at.x >= 19.5 && at.x <= 20.5 {
                1.0
            } else {
                0.0
            })
        };
        let options = SightOptions {
            step: 0.1,
            ..SightOptions::default()
        };
        let near_target = line_of_sight(&ground, p(0.0, 0.0), p(49.0, 0.0), &options, None)
            .expect("a sight line");
        assert!(!near_target.visible);
        let far_target = line_of_sight(&ground, p(0.0, 0.0), p(50.5, 0.0), &options, None)
            .expect("a sight line");
        assert!(far_target.visible);
    }

    #[test]
    fn the_earths_curve_hides_a_low_target_ten_kilometres_away() {
        // Flat ground at 0, the eye 2 m up, the target on the ground L = 10 km
        // off, De = 12 741 994 m. Without curvature the sight is clear all the
        // way. With a coefficient of 1 the ground d m out sinks d^2 / De - at
        // the target L^2 / De = 7.848 m - and the clearance d m out is
        //   C(d) = 2 (1 - d / L) - d (L - d) / De,
        // which is 0 where (d - L)(d - 2 De / L) = 0: hidden from
        // 2 De / L = 2548.399 m on, so the first station hidden (10 m apart)
        // is 2550. C is least where C'(d) = 0, at L / 2 + De / L = 6274.199;
        // of the stations the nearest below, 6270, is the least (C(6270) =
        // -1.089435, C(6280) = -1.089434).
        let length = 10000.0;
        let diameter = EARTH_DIAMETER;
        let ground = |_: Point2| Some(0.0);
        let mut options = SightOptions {
            observer_height: 2.0,
            step: 10.0,
            ..SightOptions::default()
        };
        let flat = line_of_sight(&ground, p(0.0, 0.0), p(length, 0.0), &options, None)
            .expect("a sight line");
        assert!(flat.visible);
        options.curvature = 1.0;
        let curved = line_of_sight(&ground, p(0.0, 0.0), p(length, 0.0), &options, None)
            .expect("a sight line");
        assert!(!curved.visible);
        near(curved.target_z, -length * length / diameter, 1e-9);
        near(curved.blocked_distance.expect("blocked"), 2550.0, 1e-9);
        let least = 6270.0;
        near(
            curved.clearance.expect("a clearance"),
            2.0 * (1.0 - least / length) - least * (length - least) / diameter,
            1e-9,
        );
        near(curved.clearance_at.expect("a clearance").x, least, 1e-9);
    }

    #[test]
    fn stations_off_the_ground_hide_nothing_and_are_counted() {
        // No ground between 10 and 20 m: unknown there, never a wall at 0 -
        // nor a pit that would make the clearance look larger than it is.
        let ground = |at: Point2| {
            if at.x > 10.0 && at.x < 20.0 {
                None
            } else {
                Some(0.0)
            }
        };
        let options = SightOptions {
            target_height: 1.7,
            ..SightOptions::default()
        };
        let line = line_of_sight(&ground, p(0.0, 0.0), p(30.0, 0.0), &options, None)
            .expect("a sight line");
        assert!(line.visible);
        assert_eq!(line.stations, 29);
        // Stations at 11 .. 19 m.
        assert_eq!(line.unknown, 9);
        near(line.clearance.expect("a clearance"), 1.7, 1e-12);
    }

    #[test]
    fn coincident_ends_an_end_off_the_ground_and_a_bad_step_are_refused() {
        let ground = |at: Point2| if at.x < 100.0 { Some(0.0) } else { None };
        let mut options = SightOptions::default();
        assert_eq!(
            line_of_sight(&ground, p(1.0, 1.0), p(1.0, 1.0), &options, None)
                .expect_err("coincident ends are refused")
                .code,
            ErrorCode::InvalidArgument
        );
        let off = line_of_sight(&ground, p(0.0, 0.0), p(150.0, 0.0), &options, None)
            .expect_err("a target off the ground is refused");
        assert!(off.message.contains("target"), "{}", off.message);
        let eye = line_of_sight(&ground, p(150.0, 0.0), p(0.0, 0.0), &options, None)
            .expect_err("an observer off the ground is refused");
        assert!(eye.message.contains("observer"), "{}", eye.message);
        options.step = 0.0;
        assert!(line_of_sight(&ground, p(0.0, 0.0), p(10.0, 0.0), &options, None).is_err());
        options.step = 1.0;
        options.curvature = -1.0;
        assert!(line_of_sight(&ground, p(0.0, 0.0), p(10.0, 0.0), &options, None).is_err());
    }
}
