//! Twin of tests/survey/test_reduction_performance.cpp. Throughput: the
//! owner's first requirement. A synthetic job of 1 000 setups, each shooting 17
//! targets on both faces (HA, ZA and SD per pointing): 102 000 raw observations
//! plus the backsights. The test checks every target came out and records the
//! time; the bound is loose because a Debug build with checks on is several
//! times slower than the release the numbers in the commit message were
//! measured with.

mod reduction_support;
mod survey_support;

mod reduction_performance {
    use std::time::Instant;

    use crate::reduction_support::{
        arc_seconds, bare_settings, deg, dms, find_point, point, point_from, setup, shot,
        succeeded, unpositioned,
    };
    use crate::survey_support::near;
    use katana_core::text::format_exact_real;
    use katana_math::numerics::normalize_angle;
    use katana_survey::data_model::{ControlPoint, CoordinateSource, Face, SurveyProject};
    use katana_survey::reduction::{ReductionContext, reduce_and_adjust};
    use katana_survey::reduction_report::{ComputationMethod, render_text};
    use katana_survey::reduction_settings::{
        AdjustmentMethod, ControlSelection, ReductionSettings,
    };

    fn large_job(setups: usize, targets: usize, foresights: bool) -> SurveyProject {
        let mut project = SurveyProject::default();
        project.points.reserve(setups + 1);
        project.points.push(point("S0", 0.0, -100.0, Some(10.0)));
        project.unpositioned_points.reserve(setups * targets);
        project.stations.reserve(setups);
        for s in 1..=setups {
            let station = format!("S{s}");
            let back = format!("S{}", s - 1);
            // Stations along a line east, 100 m apart; the backsight is due west.
            project
                .points
                .push(point(&station, 0.0, 100.0 * (s as f64 - 1.0), Some(10.0)));
            let mut shots = Vec::with_capacity(2 * targets + 2);
            let mut index = 1;
            shots.push(shot(
                &back,
                index,
                Face::Left,
                Some(deg(270.0)),
                Some(deg(90.0)),
                Some(100.0),
                1.5,
            ));
            index += 1;
            shots.push(shot(
                &back,
                index,
                Face::Right,
                Some(deg(90.0)),
                Some(deg(90.0)),
                Some(100.0),
                1.5,
            ));
            index += 1;
            if foresights {
                // The next station, due east: what makes the stations a network
                // (an angle back -> forward and two distances at each).
                let forward = format!("S{}", s + 1);
                shots.push(shot(
                    &forward,
                    index,
                    Face::Left,
                    Some(deg(90.0)),
                    Some(deg(90.0)),
                    Some(100.0),
                    1.5,
                ));
                index += 1;
                shots.push(shot(
                    &forward,
                    index,
                    Face::Right,
                    Some(deg(270.0)),
                    Some(deg(90.0)),
                    Some(100.0),
                    1.5,
                ));
                index += 1;
            }
            for t in 0..targets {
                let target = format!("P{s}_{t}");
                project.unpositioned_points.push(unpositioned(&target));
                let direction = deg(10.0 + 20.0 * t as f64);
                let zenith = deg(88.0 + 0.2 * t as f64);
                let distance = 5.0 + 2.5 * t as f64;
                shots.push(shot(
                    &target,
                    index,
                    Face::Left,
                    Some(direction),
                    Some(zenith),
                    Some(distance),
                    1.8,
                ));
                index += 1;
                shots.push(shot(
                    &target,
                    index,
                    Face::Right,
                    Some(normalize_angle(direction + dms(180.0, 0.0, 2.0))),
                    Some(zenith + arc_seconds(4.0)),
                    Some(distance + 0.001),
                    1.8,
                ));
                index += 1;
            }
            project
                .stations
                .push(setup(&format!("setup{s}"), &station, 1.55, &back, &shots));
        }
        if foresights {
            project.points.push(point(
                &format!("S{}", setups + 1),
                0.0,
                100.0 * setups as f64,
                Some(10.0),
            ));
        }
        project
    }

    // Setups whose stations are placed one at a time, the shape of a job whose
    // stations were resected in the field: each stands on a point whose file
    // coordinates are the controller's own (Calculated, so not control), and
    // backsights a reference object RO<s> that nothing positions. Every station
    // falls back to its file coordinates in turn, one per round, and every
    // setup then waits for its RO. With `circle` the file records the circle
    // set on the RO (0 00 00) and each setup is in the end oriented on it;
    // without, each is given up with a warning.
    fn placed_one_at_a_time(setups: usize, targets: usize, circle: bool) -> SurveyProject {
        let mut project = SurveyProject::default();
        for s in 1..=setups {
            let station = format!("T{s}");
            let ro = format!("RO{s}");
            project.points.push(point_from(
                &station,
                0.0,
                100.0 * s as f64,
                Some(10.0),
                CoordinateSource::Calculated,
            ));
            project.unpositioned_points.push(unpositioned(&ro));
            let mut shots = Vec::new();
            let mut index = 1;
            shots.push(shot(
                &ro,
                index,
                Face::Left,
                Some(deg(0.0)),
                Some(deg(90.0)),
                Some(50.0),
                1.5,
            ));
            index += 1;
            shots.push(shot(
                &ro,
                index,
                Face::Right,
                Some(deg(180.0)),
                Some(deg(90.0)),
                Some(50.0),
                1.5,
            ));
            index += 1;
            for t in 0..targets {
                let target = format!("Q{s}_{t}");
                project.unpositioned_points.push(unpositioned(&target));
                let direction = deg(10.0 + 20.0 * t as f64);
                let distance = 5.0 + 2.5 * t as f64;
                shots.push(shot(
                    &target,
                    index,
                    Face::Left,
                    Some(direction),
                    Some(deg(90.0)),
                    Some(distance),
                    1.8,
                ));
                index += 1;
                shots.push(shot(
                    &target,
                    index,
                    Face::Right,
                    Some(normalize_angle(direction + deg(180.0))),
                    Some(deg(90.0)),
                    Some(distance),
                    1.8,
                ));
                index += 1;
            }
            let mut survey_station = setup(&format!("setup{s}"), &station, 1.55, &ro, &shots);
            if circle {
                survey_station.backsight_azimuth = Some(0.0);
            }
            project.stations.push(survey_station);
        }
        project
    }

    // The faster of two runs, so that a moment's load from elsewhere on the
    // machine does not decide a ratio.
    fn best_seconds(project: &SurveyProject, settings: &ReductionSettings) -> f64 {
        let mut best = 1e30_f64;
        for _ in 0..2 {
            let start = Instant::now();
            let outcome = reduce_and_adjust(project, settings, &ReductionContext::default());
            let seconds = start.elapsed().as_secs_f64();
            assert!(outcome.is_ok());
            best = best.min(seconds);
        }
        best
    }

    #[test]
    fn setups_placed_one_at_a_time_cost_linearly_in_their_number() {
        // Four times the setups must cost about four times as long. Trying every
        // waiting setup again whenever any point was placed made it quadratic
        // (sixteen times as long) and 1 000 setups of 30 shots took 12 s.
        // Below 9 leaves room for timing noise on either side of 4.
        let settings = bare_settings();
        for circle in [false, true] {
            let small = placed_one_at_a_time(200, 5, circle);
            let large = placed_one_at_a_time(800, 5, circle);
            let small_seconds = best_seconds(&small, &settings);
            let large_seconds = best_seconds(&large, &settings);
            println!(
                "[ reduction ] setups placed one at a time{}: 200 in {:.3} s, 800 in {:.3} s \
                 (ratio {:.1})",
                if circle {
                    ", oriented on the circle as set"
                } else {
                    ", never oriented"
                },
                small_seconds,
                large_seconds,
                large_seconds / small_seconds
            );
            assert!(
                large_seconds / small_seconds < 9.0,
                "{} circle",
                if circle { "with" } else { "without" }
            );
        }

        // What the runs computed: without a circle each setup is given up once;
        // with one, each is oriented on it and radiates its RO and its targets.
        // Setup 1's first target: reading 10 00 00 is azimuth 10 (orientation
        // 0 - 0), 5 m from T1 (N 0, E 100): N = 5 cos 10 = 4.924039,
        // E = 100 + 5 sin 10 = 100.868241.
        let unoriented = succeeded(reduce_and_adjust(
            &placed_one_at_a_time(50, 5, false),
            &settings,
            &ReductionContext::default(),
        ));
        let given_up = unoriented
            .report
            .warnings
            .iter()
            .filter(|warning| {
                warning
                    .text
                    .contains("has no position and no circle setting was recorded")
            })
            .count();
        assert_eq!(given_up, 50);
        assert!(unoriented.points.is_empty());

        let oriented = succeeded(reduce_and_adjust(
            &placed_one_at_a_time(50, 5, true),
            &settings,
            &ReductionContext::default(),
        ));
        assert_eq!(oriented.points.len(), 50 * 6);
        let first = find_point(&oriented, "Q1_0").expect("Q1_0");
        near(first.northing, 4.924039, 1e-6);
        near(first.easting, 100.868241, 1e-6);
    }

    #[test]
    fn a_hundred_thousand_observations_reduce_and_every_target_is_computed() {
        let project = large_job(1000, 17, false);
        let observations: usize = project
            .stations
            .iter()
            .map(|station| station.observations.len())
            .sum();
        assert!(observations >= 100000);

        let start = Instant::now();
        let outcome = reduce_and_adjust(
            &project,
            &ReductionSettings::default(),
            &ReductionContext::default(),
        );
        let reduced = Instant::now();
        let outcome = succeeded(outcome);
        let text = render_text(&outcome.report);
        let rendered = Instant::now();

        let reduce_seconds = (reduced - start).as_secs_f64();
        let render_seconds = (rendered - reduced).as_secs_f64();
        println!(
            "[ reduction ] {observations} observations reduced in {reduce_seconds:.3} s ({:.0} per \
             second); report text {} bytes in {render_seconds:.3} s",
            observations as f64 / reduce_seconds,
            text.len()
        );
        // gtest's RecordProperty: the measurement, in the test's output.
        println!(
            "observations={observations} reduce_seconds={}",
            format_exact_real(reduce_seconds)
        );

        // Every target, plus the stations used as known: 17 000 radiated points.
        let radiated = outcome
            .points
            .iter()
            .filter(|point| point.method == ComputationMethod::Radiation)
            .count();
        assert_eq!(radiated, 17000);
        assert_eq!(outcome.report.face_pairs.len(), 1000 * 18);
        assert!(reduce_seconds < 30.0, "{reduce_seconds} s");
    }

    #[test]
    fn a_network_of_setups_with_side_shots_adjusts_only_the_stations() {
        // Stations S0 .. S(n+1) on a line, S0 and S1 held; every setup sees the
        // station behind and the one ahead on both faces and 17 side shots. The
        // least squares takes the n stations only (the side shots are radiated
        // after), so its size is 2n unknowns whatever the number of shots.
        // KATANA_REDUCTION_NETWORK_SETUPS sets n for a measurement (default 200:
        // quick in a Debug build).
        let setups = std::env::var("KATANA_REDUCTION_NETWORK_SETUPS")
            .ok()
            .map_or(200, |text| strtoul(&text));
        let project = large_job(setups, 17, true);
        let observations: usize = project
            .stations
            .iter()
            .map(|station| station.observations.len())
            .sum();
        let settings = ReductionSettings {
            method: AdjustmentMethod::Network,
            control: vec![
                ControlSelection {
                    point: ControlPoint::fixed_horizontal("S0"),
                    ..Default::default()
                },
                ControlSelection {
                    point: ControlPoint::fixed_horizontal("S1"),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let start = Instant::now();
        let outcome = reduce_and_adjust(&project, &settings, &ReductionContext::default());
        let seconds = start.elapsed().as_secs_f64();
        let outcome = succeeded(outcome);
        assert_eq!(outcome.report.adjustments.len(), 1);
        let adjustment = &outcome.report.adjustments[0];
        println!(
            "[ reduction ] network: {observations} raw observations, {} adjusted in {} unknowns, \
             reduced and adjusted in {seconds:.3} s",
            adjustment.observations, adjustment.unknowns
        );
        println!("network_seconds={}", format_exact_real(seconds));
        // S2 .. S(n+1) free: 2n unknowns. Each setup gives two distances and one
        // angle (the backsight is the reference direction).
        assert_eq!(adjustment.unknowns, 2 * setups);
        assert_eq!(adjustment.observations, 3 * setups);
        // The side shots came out, radiated from the adjusted stations.
        let radiated = outcome
            .points
            .iter()
            .filter(|point| point.method == ComputationMethod::Radiation)
            .count();
        assert_eq!(radiated, 17 * setups);
    }

    // std::strtoul(text, nullptr, 10): the leading digits after any white
    // space, 0 where there are none.
    fn strtoul(text: &str) -> usize {
        let digits: String = text
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().unwrap_or(0)
    }
}
