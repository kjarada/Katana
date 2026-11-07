//! Twin of tests/survey/test_leveling.cpp.

mod survey_support;

mod survey_leveling {
    use crate::survey_support::{double_eq, error_code, near};
    use katana_core::ErrorCode;
    use katana_survey::leveling::{
        allowable_level_misclosure, compute_level_run, level_difference_sigma,
    };
    use katana_survey::{LevelAdjustment, LevelRun, LevelSetup};

    fn setup(id: &str, backsight: f64, foresight: f64, distance: f64) -> LevelSetup {
        LevelSetup {
            foresight_point_id: String::from(id),
            backsight,
            foresight,
            distance,
        }
    }

    // Level loop from BM A (100.000) back to BM A, hand computed:
    //   setup   BS      FS      HI = elev + BS     elevation = HI - FS
    //     1    1.500   1.200    101.500            TP1 100.300
    //     2    1.800   0.900    102.100            TP2 101.200
    //     3    0.700   1.894    101.900            A   100.006
    //   sum BS 4.000, sum FS 3.994: sum BS - sum FS = +0.006 = last - first.
    //   Closing on A = 100.000: misclosure = 100.006 - 100.000 = +0.006.
    fn loop_run() -> LevelRun {
        LevelRun {
            name: String::from("loop"),
            start_elevation: 100.0,
            setups: vec![
                setup("TP1", 1.500, 1.200, 100.0),
                setup("TP2", 1.800, 0.900, 200.0),
                setup("A", 0.700, 1.894, 300.0),
            ],
            closing_elevation: Some(100.0),
        }
    }

    #[test]
    fn height_of_instrument_and_elevations() {
        let result = compute_level_run(&loop_run(), LevelAdjustment::None).unwrap();
        assert_eq!(result.points.len(), 3);

        let heights = [101.500, 102.100, 101.900];
        let elevations = [100.300, 101.200, 100.006];
        let ids = ["TP1", "TP2", "A"];
        for (i, point) in result.points.iter().enumerate() {
            assert_eq!(point.point_id, ids[i]);
            near(point.height_of_instrument, heights[i], 1e-12);
            near(point.elevation, elevations[i], 1e-12);
            // No adjustment requested: nothing is corrected.
            double_eq(point.correction, 0.0);
            double_eq(point.adjusted_elevation, point.elevation);
        }
        near(result.sum_backsights, 4.000, 1e-12);
        near(result.sum_foresights, 3.994, 1e-12);
        near(result.total_distance, 600.0, 1e-12);

        assert!(result.misclosure.is_some());
        near(result.misclosure.unwrap(), 0.006, 1e-12);
        // The page check of every level book: sum BS - sum FS = last - first
        // elevation.
        near(
            result.sum_backsights - result.sum_foresights,
            result.points[result.points.len() - 1].elevation - 100.0,
            1e-12,
        );
    }

    #[test]
    fn adjustment_by_setups() {
        // -0.006 over 3 setups: -0.002 per setup, accumulating.
        let result = compute_level_run(&loop_run(), LevelAdjustment::BySetups).unwrap();
        let corrections = [-0.002, -0.004, -0.006];
        let adjusted = [100.298, 101.196, 100.000];
        for (i, point) in result.points.iter().enumerate() {
            near(point.correction, corrections[i], 1e-12);
            near(point.adjusted_elevation, adjusted[i], 1e-12);
        }
        // the raw misclosure is still reported
        near(result.misclosure.unwrap(), 0.006, 1e-12);
    }

    #[test]
    fn adjustment_by_distance() {
        // Distances 100, 200, 300 (total 600): cumulative 100, 300, 600.
        //   -0.006 * 100/600 = -0.001   -0.006 * 300/600 = -0.003   -0.006 * 600/600 = -0.006
        let result = compute_level_run(&loop_run(), LevelAdjustment::ByDistance).unwrap();
        let corrections = [-0.001, -0.003, -0.006];
        let adjusted = [100.299, 101.197, 100.000];
        for (i, point) in result.points.iter().enumerate() {
            near(point.correction, corrections[i], 1e-12);
            near(point.adjusted_elevation, adjusted[i], 1e-12);
        }
    }

    #[test]
    fn line_between_two_benchmarks() {
        // BM1 50.000 -> BM2 known 52.347. One setup per turning point.
        //   HI 51.250 -> TP 50.120 ; HI 52.780 -> BM2 52.350. Misclosure +0.003.
        let run = LevelRun {
            start_elevation: 50.0,
            setups: vec![
                setup("TP", 1.250, 1.130, 0.0),
                setup("BM2", 2.660, 0.430, 0.0),
            ],
            closing_elevation: Some(52.347),
            ..Default::default()
        };
        let result = compute_level_run(&run, LevelAdjustment::BySetups).unwrap();
        near(result.points[0].elevation, 50.120, 1e-12);
        near(result.points[1].elevation, 52.350, 1e-12);
        near(result.misclosure.unwrap(), 0.003, 1e-12);
        // -0.0015
        near(result.points[0].adjusted_elevation, 50.1185, 1e-12);
        near(result.points[1].adjusted_elevation, 52.347, 1e-12);
    }

    #[test]
    fn open_run_has_no_misclosure() {
        let mut run = loop_run();
        run.closing_elevation = None;
        let result = compute_level_run(&run, LevelAdjustment::None).unwrap();
        assert!(result.misclosure.is_none());
        near(
            result.points[result.points.len() - 1].elevation,
            100.006,
            1e-12,
        );

        // There is nothing to distribute, and saying so beats silently doing
        // nothing.
        assert_eq!(
            error_code(compute_level_run(&run, LevelAdjustment::BySetups)),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn rejects_bad_input() {
        let empty = LevelRun::default();
        assert_eq!(
            error_code(compute_level_run(&empty, LevelAdjustment::None)),
            ErrorCode::InvalidArgument
        );

        let mut bad_reading = loop_run();
        bad_reading.setups[1].backsight = f64::NAN;
        assert!(compute_level_run(&bad_reading, LevelAdjustment::None).is_err());

        let mut bad_start = loop_run();
        bad_start.start_elevation = f64::NAN;
        assert!(compute_level_run(&bad_start, LevelAdjustment::None).is_err());

        let mut bad_closing = loop_run();
        bad_closing.closing_elevation = Some(f64::NAN);
        assert!(compute_level_run(&bad_closing, LevelAdjustment::None).is_err());

        let mut negative_distance = loop_run();
        negative_distance.setups[0].distance = -1.0;
        assert!(compute_level_run(&negative_distance, LevelAdjustment::None).is_err());

        let mut no_distances = loop_run();
        for setup in &mut no_distances.setups {
            setup.distance = 0.0;
        }
        assert!(compute_level_run(&no_distances, LevelAdjustment::BySetups).is_ok());
        assert_eq!(
            error_code(compute_level_run(
                &no_distances,
                LevelAdjustment::ByDistance
            )),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn allowable_misclosure() {
        // 12 mm * sqrt(K km), expressed in metres.
        near(
            allowable_level_misclosure(0.012, 1000.0).unwrap(),
            0.012,
            1e-15,
        );
        near(
            allowable_level_misclosure(0.012, 4000.0).unwrap(),
            0.024,
            1e-15,
        );
        near(
            allowable_level_misclosure(0.012, 600.0).unwrap(),
            0.012 * 0.6_f64.sqrt(),
            1e-15,
        );
        double_eq(allowable_level_misclosure(0.012, 0.0).unwrap(), 0.0);

        // The 0.006 loop above over 0.6 km is inside 12 mm sqrt(K) = 9.3 mm.
        assert!(0.006 < allowable_level_misclosure(0.012, 600.0).unwrap());

        assert!(allowable_level_misclosure(-0.012, 1000.0).is_err());
        assert!(allowable_level_misclosure(0.012, -1.0).is_err());
        assert!(allowable_level_misclosure(f64::NAN, 1000.0).is_err());

        // 1 mm per sqrt(km) over 250 m: 0.001 * sqrt(0.25) = 0.5 mm.
        near(level_difference_sigma(0.001, 250.0).unwrap(), 0.0005, 1e-16);
    }
}
