//! Twin of tests/survey/test_reduction.cpp: reduce_and_adjust end to end:
//! face pairs, orientation and radiation over two setups, the reduced project,
//! re-running with changed settings, and errors.

#![allow(
    clippy::excessive_precision,
    reason = "The expected values are the C++ tests' literals, digit for digit."
)]

mod reduction_support;
mod survey_support;

mod reduction {
    use crate::reduction_support::{
        arc_seconds, bare_settings, deg, dms, find_correction, find_point, find_row, point, setup,
        shot, succeeded, unpositioned,
    };
    use crate::survey_support::near;
    use katana_core::ErrorCode;
    use katana_math::numerics::PI;
    use katana_survey::data_model::{
        ControlPoint, CoordinateSource, DistanceKind, Face, GeodeticCoordinate,
        GnssGlobalPositionObservation, Observation, SurveyProject, validate_project,
    };
    use katana_survey::reduction::{
        GridPosition, ReductionContext, ReductionOutcome, reduce_and_adjust,
    };
    use katana_survey::reduction_report::{ComputationMethod, CorrectionKind, ReductionReport};
    use katana_survey::reduction_settings::{
        AdjustmentMethod, AtmosphericCorrection, ControlOrigin, ControlSelection, FaceHandling,
        HeightReduction, ReductionSettings, parse_reduction_settings, serialise_reduction_settings,
    };

    // Two setups, both faces, hand-worked (k = 0.13, R = 6 371 000 m):
    //
    // S1 on A (N 1000, E 1000, H 50), HI 1.5, backsight B (N 1100, E 1000): grid
    // azimuth A -> B = 0.
    //   B   FL 10 00 00, FR 190 00 02  -> mean 10 00 01
    //   orientation = 0 - 10 00 01 = -10 00 01
    //   P1  FL 100 00 01, FR 280 00 01 -> mean 100 00 01 -> azimuth 90 00 00
    //       zenith 90 both faces, slope 50.000 both, HT 1.5
    //       X = 0, Y = 50: HD = 50 exactly; dH = B Y^2 = 6.8278135e-8 * 2500
    //       = 0.000170695; P1 = (N 1000, E 1050, H 50.000170695)
    // S2 on P1, HI 1.6, backsight A: azimuth P1 -> A = 270.
    //   A   FL 0 00 00, FR 180 00 00 -> mean 0; slope 50.000 at zenith 90
    //   orientation = 270 00 00
    //   P2  FL 90 00 00, FR 270 00 00 -> mean 90 -> azimuth 0 (north)
    //       zenith 89 00 00 both faces, slope 100.000 both, HT 1.6
    //       X = 100 cos 89 = 1.745240644, Y = 99.984769516
    //       HD = Y - A X Y = 99.984769516 - 0.000025609 = 99.984743907
    //       dH = X + B Y^2 = 1.745240644 + 0.000682573 = 1.745923217
    //   P2 = (N 1099.984743907, E 1050, H 51.746093912)
    // The backsight check at S2: 50.000 measured against 50.000 computed = 0.
    fn two_setups() -> SurveyProject {
        let mut project = SurveyProject::default();
        project.points.push(point("A", 1000.0, 1000.0, Some(50.0)));
        project.points.push(point("B", 1100.0, 1000.0, Some(50.0)));
        project.unpositioned_points.push(unpositioned("P1"));
        project.unpositioned_points.push(unpositioned("P2"));
        project.stations.push(setup(
            "S1",
            "A",
            1.5,
            "B",
            &[
                shot("B", 1, Face::Left, Some(deg(10.0)), None, None, 0.0),
                shot(
                    "P1",
                    2,
                    Face::Left,
                    Some(dms(100.0, 0.0, 1.0)),
                    Some(deg(90.0)),
                    Some(50.0),
                    1.5,
                ),
                shot(
                    "P1",
                    3,
                    Face::Right,
                    Some(dms(280.0, 0.0, 1.0)),
                    Some(deg(90.0)),
                    Some(50.0),
                    1.5,
                ),
                shot(
                    "B",
                    4,
                    Face::Right,
                    Some(dms(190.0, 0.0, 2.0)),
                    None,
                    None,
                    0.0,
                ),
            ],
        ));
        project.stations.push(setup(
            "S2",
            "P1",
            1.6,
            "A",
            &[
                shot(
                    "A",
                    1,
                    Face::Left,
                    Some(deg(0.0)),
                    Some(deg(90.0)),
                    Some(50.0),
                    1.6,
                ),
                shot(
                    "P2",
                    2,
                    Face::Left,
                    Some(deg(90.0)),
                    Some(deg(89.0)),
                    Some(100.0),
                    1.6,
                ),
                shot(
                    "P2",
                    3,
                    Face::Right,
                    Some(deg(270.0)),
                    Some(deg(89.0)),
                    Some(100.0),
                    1.6,
                ),
                shot(
                    "A",
                    4,
                    Face::Right,
                    Some(deg(180.0)),
                    Some(deg(90.0)),
                    Some(50.0),
                    1.6,
                ),
            ],
        ));
        project
    }

    fn reduce(project: &SurveyProject, settings: &ReductionSettings) -> ReductionOutcome {
        succeeded(reduce_and_adjust(
            project,
            settings,
            &ReductionContext::default(),
        ))
    }

    #[test]
    fn two_setups_on_both_faces_radiate_to_the_hand_worked_coordinates() {
        let outcome = reduce(&two_setups(), &ReductionSettings::default());
        let report = &outcome.report;

        assert_eq!(report.setups.len(), 2);
        near(
            report.setups[0].backsight_reading.unwrap(),
            dms(10.0, 0.0, 1.0),
            1e-12,
        );
        near(
            report.setups[0].orientation_correction.unwrap(),
            -dms(10.0, 0.0, 1.0),
            1e-12,
        );
        // 270 as signed
        near(
            report.setups[1].orientation_correction.unwrap(),
            -deg(90.0),
            1e-12,
        );
        near(
            report.setups[1].backsight_distance_difference.unwrap(),
            0.0,
            1e-9,
        );

        let p1 = find_point(&outcome, "P1").expect("P1");
        let p2 = find_point(&outcome, "P2").expect("P2");
        near(p1.northing, 1000.0, 1e-9);
        near(p1.easting, 1050.0, 1e-9);
        near(p1.elevation.unwrap(), 50.000170695, 1e-9);
        assert_eq!(p1.method, ComputationMethod::Radiation);
        near(p2.northing, 1099.984743907, 1e-8);
        near(p2.easting, 1050.0, 1e-8);
        near(p2.elevation.unwrap(), 51.746093912, 1e-8);

        // Every raw observation has its row, in order: 4 directions + 2 zeniths
        // + 2 distances at S1 = 8, then its height difference; 4 + 4 + 4 at S2
        // and two height differences (A and P2).
        let raw = report
            .observations
            .iter()
            .filter(|row| row.kind != "height difference")
            .count();
        assert_eq!(raw, 8 + 12);

        // Face pairs: B, P1 at S1; A, P2 at S2. The B pair's spread is 2".
        assert_eq!(report.face_pairs.len(), 4);
        near(
            report.face_pairs[0].horizontal_spread.unwrap(),
            arc_seconds(2.0),
            1e-12,
        );
        assert!(report.face_pairs[0].within_tolerance);
    }

    #[test]
    fn a_face_pair_means_the_readings_and_reports_the_index_error_and_spreads() {
        // FL 45 00 10 / FR 225 00 04: FR - 180 = 45 00 04, mean 45 00 07, spread 6".
        // Zenith FL 88 00 20, FR raw 271 59 50 stored folded as 88 00 10:
        //   mean 88 00 15, index error (FL - FR') / 2 = 5", spread 10".
        // Slope 100.003 / 100.001: mean 100.002, spread 2 mm.
        let mut project = SurveyProject::default();
        project.points.push(point("A", 0.0, 0.0, Some(10.0)));
        project.points.push(point("B", 100.0, 0.0, Some(10.0)));
        project.unpositioned_points.push(unpositioned("T"));
        project.stations.push(setup(
            "S",
            "A",
            1.5,
            "B",
            &[
                shot("B", 1, Face::Left, Some(0.0), None, None, 0.0),
                shot(
                    "T",
                    2,
                    Face::Left,
                    Some(dms(45.0, 0.0, 10.0)),
                    Some(dms(88.0, 0.0, 20.0)),
                    Some(100.003),
                    1.5,
                ),
                shot(
                    "T",
                    3,
                    Face::Right,
                    Some(dms(225.0, 0.0, 4.0)),
                    Some(dms(88.0, 0.0, 10.0)),
                    Some(100.001),
                    1.5,
                ),
            ],
        ));
        let outcome = reduce(&project, &bare_settings());
        let report = &outcome.report;
        assert_eq!(report.face_pairs.len(), 1);
        let pair = &report.face_pairs[0];
        near(pair.horizontal_spread.unwrap(), arc_seconds(6.0), 1e-11);
        near(pair.zenith_spread.unwrap(), arc_seconds(10.0), 1e-11);
        near(pair.distance_spread.unwrap(), 0.002, 1e-9);
        assert!(pair.within_tolerance);

        let left = find_row(report, "horizontal direction", "T", 0).expect("left");
        let right = find_row(report, "horizontal direction", "T", 1).expect("right");
        near(
            find_correction(left, CorrectionKind::FaceMean)
                .unwrap()
                .amount,
            -arc_seconds(3.0),
            1e-11,
        );
        // FR: 225 00 04 -> 45 00 07 is -180 + 3".
        near(
            find_correction(right, CorrectionKind::FaceMean)
                .unwrap()
                .amount,
            -deg(180.0) + arc_seconds(3.0),
            1e-11,
        );
        // Oriented on B (reading 0, azimuth 0): the azimuth is the mean reading.
        near(left.reduced.unwrap(), dms(45.0, 0.0, 7.0), 1e-11);

        let zenith = find_row(report, "zenith angle", "T", 0).expect("zenith");
        near(zenith.reduced.unwrap(), dms(88.0, 0.0, 15.0), 1e-11);
        near(
            find_correction(zenith, CorrectionKind::FaceMean)
                .unwrap()
                .amount,
            -arc_seconds(5.0),
            1e-11,
        );
    }

    #[test]
    fn a_pair_outside_the_tolerance_is_flagged_and_excluded_only_when_the_settings_say_so() {
        // 15" between the faces against a 10" tolerance.
        let mut project = SurveyProject::default();
        project.points.push(point("A", 0.0, 0.0, None));
        project.points.push(point("B", 100.0, 0.0, None));
        project.unpositioned_points.push(unpositioned("T"));
        project.stations.push(setup(
            "S",
            "A",
            1.5,
            "B",
            &[
                shot("B", 1, Face::Left, Some(0.0), None, None, 0.0),
                shot(
                    "T",
                    2,
                    Face::Left,
                    Some(deg(45.0)),
                    Some(deg(90.0)),
                    Some(100.0),
                    0.0,
                ),
                shot(
                    "T",
                    3,
                    Face::Right,
                    Some(dms(225.0, 0.0, 15.0)),
                    Some(deg(90.0)),
                    Some(100.0),
                    0.0,
                ),
            ],
        ));
        let mut settings = bare_settings();
        let used = reduce(&project, &settings);
        assert_eq!(used.report.face_pairs.len(), 1);
        assert!(!used.report.face_pairs[0].within_tolerance);
        assert!(find_point(&used, "T").is_some());

        settings.face_tolerances.exclude_outside = true;
        let excluded = reduce(&project, &settings);
        assert!(find_point(&excluded, "T").is_none());
        let row = find_row(&excluded.report, "horizontal direction", "T", 0).expect("row");
        assert!(row.rejected);
        assert!(row.rejection_reason.contains("tolerance"));
    }

    #[test]
    fn face_left_only_reports_the_face_right_pointings_as_unused() {
        let settings = ReductionSettings {
            faces: FaceHandling::FaceLeftOnly,
            ..Default::default()
        };
        let outcome = reduce(&two_setups(), &settings);
        let mut unused = 0;
        for row in &outcome.report.observations {
            if row.rejected {
                assert_eq!(row.pointing.face, Face::Right);
                unused += 1;
            }
        }
        assert_eq!(unused, 1 + 3 + 3 + 3); // S1: B (1) + P1 (3); S2: A (3) + P2 (3)
        // P1 from face left only: 100 00 01 - 10 00 00 = 90 00 01, not 90 00 00.
        let p1 = find_point(&outcome, "P1").expect("P1");
        near(p1.northing, 1000.0 + 50.0 * dms(90.0, 0.0, 1.0).cos(), 1e-9);
    }

    #[test]
    fn the_reduced_project_carries_the_computed_points_as_calculated_and_is_valid() {
        let outcome = reduce(&two_setups(), &ReductionSettings::default());
        let reduced = &outcome.reduced;
        assert!(reduced.unpositioned_points.is_empty());
        let mut found = false;
        for p in &reduced.points {
            if p.id == "P2" {
                found = true;
                assert_eq!(p.coordinate_source, CoordinateSource::Calculated);
            }
            if p.id == "A" {
                // held, not computed
                assert_eq!(p.coordinate_source, CoordinateSource::Entered);
            }
        }
        assert!(found);
        // Oriented directions became azimuths, slope distances horizontal ones.
        let mut azimuths = 0;
        for observation in &reduced.stations[0].observations {
            if matches!(observation, Observation::Azimuth(_)) {
                azimuths += 1;
            }
            if let Observation::Distance(distance) = observation {
                assert_eq!(distance.kind, DistanceKind::Horizontal);
            }
        }
        assert_eq!(azimuths, 2); // B and P1, one per face pair
        if let Err(error) = validate_project(reduced) {
            panic!("{}", error.describe());
        }
    }

    #[test]
    fn changing_the_refraction_coefficient_changes_only_the_heights_and_the_curvature_terms() {
        let first = reduce(&two_setups(), &ReductionSettings::default());
        let changed = ReductionSettings {
            refraction_coefficient: 0.2,
            ..Default::default()
        };
        let context = ReductionContext {
            previous: first.points.clone(),
            ..Default::default()
        };
        let second = succeeded(reduce_and_adjust(&two_setups(), &changed, &context));

        // Directions and zeniths: identical. Distances and heights: only where
        // a curvature term is.
        assert_eq!(
            first.report.observations.len(),
            second.report.observations.len()
        );
        for (a, b) in first
            .report
            .observations
            .iter()
            .zip(&second.report.observations)
        {
            if a.angular {
                assert_eq!(a.reduced, b.reduced, "{} {}", a.kind, a.to);
            }
        }
        // P1's height: B Y^2 with k 0.2 is 0.8 / 12742000 * 2500 = 0.000156961,
        // against 0.000170695 with k 0.13: the shift is -0.000013734 m.
        let p1 = find_point(&second, "P1").expect("P1");
        near(p1.elevation.unwrap(), 50.000156961, 1e-9);
        for coordinate in &second.report.coordinates {
            if coordinate.point_id == "P1" {
                let shift = coordinate.shift_elevation.expect("a shift in height");
                near(shift, -0.000013734, 1e-9);
                near(coordinate.shift_northing.unwrap(), 0.0, 1e-12);
                near(coordinate.shift_easting.unwrap(), 0.0, 1e-12);
            }
        }
        assert_eq!(second.report.settings.refraction_coefficient, 0.2);
    }

    #[test]
    fn a_fixed_ppm_moves_every_target_radially_and_leaves_the_angles_alone() {
        // +100 ppm on P1's 50 m: 5 mm further east. P2's 100 m line grows by
        // 10 mm along its azimuth (north) and starts from the moved P1.
        let settings = ReductionSettings {
            atmospheric: AtmosphericCorrection::Fixed,
            fixed_ppm: 100.0,
            ..Default::default()
        };
        let base = reduce(&two_setups(), &ReductionSettings::default());
        let scaled = reduce(&two_setups(), &settings);
        let p1 = find_point(&scaled, "P1").expect("P1");
        let p1_base = find_point(&base, "P1").expect("P1");
        near(p1.easting - p1_base.easting, 0.005, 1e-9);
        let p2 = find_point(&scaled, "P2").expect("P2");
        let p2_base = find_point(&base, "P2").expect("P2");
        near(
            p2.northing - p2_base.northing,
            100.0 * deg(89.0).sin() * 1e-4,
            1e-6,
        );
    }

    #[test]
    fn control_missing_from_the_file_and_the_drawing_is_not_found() {
        let settings = ReductionSettings {
            control: vec![ControlSelection {
                point: ControlPoint::fixed3d("NOPE"),
                origin: ControlOrigin::File,
            }],
            ..Default::default()
        };
        let outcome = reduce_and_adjust(&two_setups(), &settings, &ReductionContext::default());
        let error = outcome.expect_err("unknown control");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[test]
    fn control_from_the_drawing_holds_the_point_but_is_not_returned_to_be_drawn_again() {
        let mut project = two_setups();
        // B only on the drawing: the file names it without coordinates.
        project.points.remove(1);
        project.unpositioned_points.push(unpositioned("B"));
        let context = ReductionContext {
            drawing_points: vec![point("B", 1100.0, 1000.0, Some(50.0))],
            ..Default::default()
        };
        let settings = ReductionSettings {
            control: vec![
                ControlSelection {
                    point: ControlPoint::fixed3d("A"),
                    origin: ControlOrigin::File,
                },
                ControlSelection {
                    point: ControlPoint::fixed3d("B"),
                    origin: ControlOrigin::Drawing,
                },
            ],
            ..Default::default()
        };
        let outcome = succeeded(reduce_and_adjust(&project, &settings, &context));
        assert!(find_point(&outcome, "B").is_none());
        let p1 = find_point(&outcome, "P1").expect("P1");
        near(p1.easting, 1050.0, 1e-9);
        let a = find_point(&outcome, "A").expect("A");
        assert_eq!(a.method, ComputationMethod::Control);
        assert_eq!(a.sigma_northing, Some(0.0));
    }

    #[test]
    fn a_setup_with_no_orientation_radiates_nothing_and_says_why() {
        let mut project = two_setups();
        project.stations[0].backsight_point_id.clear();
        let outcome = reduce(&project, &ReductionSettings::default());
        assert!(find_point(&outcome, "P1").is_none());
        let said = outcome
            .report
            .warnings
            .iter()
            .any(|warning| warning.text.contains("S1 has no backsight"));
        assert!(said);
    }

    #[test]
    fn a_gnss_position_is_converted_through_the_context_and_serves_as_a_station() {
        let mut project = two_setups();
        // A is now known only from GNSS.
        project.points.remove(0);
        project.unpositioned_points.push(unpositioned("A"));
        project.observations.push(Observation::GnssGlobalPosition(
            GnssGlobalPositionObservation {
                point: String::from("A"),
                geodetic: Some(GeodeticCoordinate {
                    latitude: 0.5,
                    longitude: 2.0,
                    ellipsoidal_height: 80.0,
                }),
                ..Default::default()
            },
        ));
        let context = ReductionContext {
            geodetic_to_grid: Some(Box::new(|_: &GeodeticCoordinate| {
                Some(GridPosition {
                    northing: 1000.0,
                    easting: 1000.0,
                    ellipsoidal_height: 80.0,
                })
            })),
            geoid_separation: Some(Box::new(|_: f64, _: f64| Some(30.0))),
            ..Default::default()
        };
        let outcome = succeeded(reduce_and_adjust(
            &project,
            &ReductionSettings::default(),
            &context,
        ));
        let a = find_point(&outcome, "A").expect("A");
        assert_eq!(a.method, ComputationMethod::Gnss);
        near(a.elevation.unwrap(), 50.0, 1e-12); // 80 ellipsoidal - 30 geoid
        let p1 = find_point(&outcome, "P1").expect("P1");
        near(p1.easting, 1050.0, 1e-9);
    }

    #[test]
    fn identical_inputs_give_identical_outcomes() {
        let a = reduce(&two_setups(), &ReductionSettings::default());
        let b = reduce(&two_setups(), &ReductionSettings::default());
        assert_eq!(a.points, b.points);
        assert_eq!(a.report, b.report);
        assert_eq!(a.reduced, b.reduced);
    }

    #[test]
    fn the_exclude_outside_tolerance_setting_survives_the_text_form() {
        // The one setting this builder added to the contract: it must
        // round-trip like every other, and text written before it existed must
        // read as off.
        let mut settings = ReductionSettings::default();
        settings.face_tolerances.exclude_outside = true;
        let parsed = parse_reduction_settings(&serialise_reduction_settings(&settings), None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(parsed.face_tolerances.exclude_outside);
        assert_eq!(parsed, settings);

        let older = parse_reduction_settings("katana-reduction-settings=1\nfaces=average\n", None)
            .expect("older text");
        assert!(!older.face_tolerances.exclude_outside);
    }

    #[test]
    fn readings_half_a_turn_apart_with_no_stated_face_are_paired_as_the_two_faces() {
        // The two-setup job with every face erased: 100 00 01 and 280 00 01 to
        // P1 must still mean to 100 00 01 (azimuth 90 00 00), not to 190 00 01.
        let mut project = two_setups();
        for station in &mut project.stations {
            for observation in &mut station.observations {
                match observation {
                    Observation::Distance(o) => o.pointing.face = Face::Unknown,
                    Observation::HorizontalAngle(o) => o.pointing.face = Face::Unknown,
                    Observation::VerticalAngle(o) => o.pointing.face = Face::Unknown,
                    Observation::ZenithAngle(o) => o.pointing.face = Face::Unknown,
                    Observation::HorizontalDirection(o) => o.pointing.face = Face::Unknown,
                    _ => {}
                }
            }
        }
        let outcome = reduce(&project, &ReductionSettings::default());
        let p1 = find_point(&outcome, "P1").expect("P1");
        near(p1.northing, 1000.0, 1e-9);
        near(p1.easting, 1050.0, 1e-9);
        assert_eq!(outcome.report.face_pairs.len(), 4);
    }

    #[test]
    fn the_face_pair_of_the_owners_neural_survey_engine_means_to_its_values() {
        // The pair in NeuralSurvey's engine/src/reduce.rs test
        // fl_fr_pair_cancels_collimation_and_index (run with cargo: it passes):
        //   FL direction 1.0 rad, zenith 1.4 rad; FR direction 1.0 + pi + 0.0005,
        //   zenith 2 pi - 1.4 + 0.0003.
        //   direction: FR - pi = 1.0005, mean 1.0 + 0.0005 / 2 = 1.00025 (its
        //   asserted value);
        //   zenith: FR folded 2 pi - (2 pi - 1.4 + 0.0003) = 1.3997, mean
        //   (1.4 + 1.3997) / 2 = 1.39985, its z_mean, the same formula.
        let mut project = SurveyProject::default();
        project.points.push(point("A", 0.0, 0.0, Some(10.0)));
        project.points.push(point("B", 100.0, 0.0, Some(10.0)));
        project.unpositioned_points.push(unpositioned("T"));
        project.stations.push(setup(
            "S",
            "A",
            1.5,
            "B",
            &[
                shot("B", 1, Face::Left, Some(0.0), None, None, 0.0),
                shot("T", 2, Face::Left, Some(1.0), Some(1.4), Some(100.0), 1.5),
                shot(
                    "T",
                    3,
                    Face::Right,
                    Some(1.0 + PI + 0.0005),
                    Some(2.0 * PI - 1.4 + 0.0003),
                    Some(100.0),
                    1.5,
                ),
            ],
        ));
        let outcome = reduce(&project, &bare_settings());
        let direction =
            find_row(&outcome.report, "horizontal direction", "T", 0).expect("direction");
        let zenith = find_row(&outcome.report, "zenith angle", "T", 0).expect("zenith");
        // oriented on B: reading 0 is azimuth 0
        near(direction.reduced.unwrap(), 1.00025, 1e-12);
        near(zenith.reduced.unwrap(), 1.39985, 1e-12);
    }

    // S1 stands on A but backsights P1, which nothing has placed yet; S2 on B
    // radiates P1 (azimuth B -> A is 180, reading 0 on A, P1 at reading 90 is
    // azimuth 270, 50 m: P1 = N 1100, E 950). S1 is then oriented on P1:
    // azimuth A -> P1 = atan2(-50, 100) = 333 26 05.8, and radiates Q.
    fn later_backsight() -> SurveyProject {
        let mut project = SurveyProject::default();
        project.points.push(point("A", 1000.0, 1000.0, Some(50.0)));
        project.points.push(point("B", 1100.0, 1000.0, Some(50.0)));
        project.unpositioned_points.push(unpositioned("P1"));
        project.unpositioned_points.push(unpositioned("Q"));
        project.stations.push(setup(
            "S1",
            "A",
            1.5,
            "P1",
            &[
                shot(
                    "P1",
                    1,
                    Face::Left,
                    Some(deg(0.0)),
                    Some(deg(90.0)),
                    Some(111.803399),
                    1.5,
                ),
                shot(
                    "Q",
                    2,
                    Face::Left,
                    Some(deg(90.0)),
                    Some(deg(90.0)),
                    Some(20.0),
                    1.5,
                ),
            ],
        ));
        project.stations.push(setup(
            "S2",
            "B",
            1.5,
            "A",
            &[
                shot(
                    "A",
                    1,
                    Face::Left,
                    Some(deg(0.0)),
                    Some(deg(90.0)),
                    Some(100.0),
                    1.5,
                ),
                shot(
                    "P1",
                    2,
                    Face::Left,
                    Some(deg(90.0)),
                    Some(deg(90.0)),
                    Some(50.0),
                    1.5,
                ),
            ],
        ));
        project
    }

    // P1 and Q where later_backsight() puts them. Q: azimuth 333 26 05.8 + 90 =
    // 63 26 05.8 from A, 20 m: N = 1000 + 20 cos(63.434949) = 1008.944272,
    // E = 1000 + 20 sin(63.434949) = 1017.888544.
    fn expect_later_backsight_coordinates(outcome: &ReductionOutcome) {
        let p1 = find_point(outcome, "P1").expect("P1");
        near(p1.northing, 1100.0, 1e-6);
        near(p1.easting, 950.0, 1e-6);
        let q = find_point(outcome, "Q").expect("Q");
        near(q.northing, 1008.944272, 1e-6);
        near(q.easting, 1017.888544, 1e-6);
    }

    fn warnings_with(report: &ReductionReport, words: &str) -> usize {
        crate::reduction_support::warnings_with(report, words)
    }

    #[test]
    fn a_setup_whose_backsight_a_later_setup_radiates_is_oriented_on_it_after_all() {
        let outcome = reduce(&later_backsight(), &bare_settings());
        expect_later_backsight_coordinates(&outcome);
        assert!(outcome.report.setups[0].orientation_correction.is_some());
        assert_eq!(warnings_with(&outcome.report, "cannot be oriented"), 0);
    }

    #[test]
    fn a_circle_set_on_a_backsight_with_no_position_yet_waits_for_the_backsights_coordinates() {
        // As above, but the file records the circle set on each backsight,
        // 0 00 00, as a controller does whether or not it is a grid azimuth.
        // S1's circle must not stand in for the azimuth A -> P1 while a later
        // setup can still place P1: taken at once it would put P1 at N 1111.803
        // E 1000 and Q at N 1000 E 1020, and S2's P1 would be a 51 m "check".
        let mut project = later_backsight();
        project.stations[0].backsight_azimuth = Some(0.0);
        project.stations[1].backsight_azimuth = Some(0.0);

        let radiated = reduce(&project, &bare_settings());
        expect_later_backsight_coordinates(&radiated);
        // S1 oriented on P1's coordinates: 333 26 05.8 - reading 0.
        let orientation = radiated.report.setups[0]
            .orientation_correction
            .expect("an orientation");
        near(orientation, (-50.0_f64).atan2(100.0), 1e-12);
        // Nothing was checked against a wrong position, and no circle was used.
        assert!(radiated.report.misclosures.is_empty());
        assert_eq!(warnings_with(&radiated.report, "circle"), 0);

        // The network, A and B held: P1 from S2's angle and distance and S1's
        // distance (111.803399 against sqrt(100^2 + 50^2) = 111.8033989, a
        // 0.1 micrometre residual), with no azimuth from a circle among them.
        let settings = ReductionSettings {
            method: AdjustmentMethod::Network,
            control: vec![
                ControlSelection {
                    point: ControlPoint::fixed_horizontal("A"),
                    ..Default::default()
                },
                ControlSelection {
                    point: ControlPoint::fixed_horizontal("B"),
                    ..Default::default()
                },
            ],
            ..bare_settings()
        };
        let adjusted = reduce(&project, &settings);
        expect_later_backsight_coordinates(&adjusted);
        assert_eq!(adjusted.report.adjustments.len(), 1);
        let network = &adjusted.report.adjustments[0];
        let factor = network.variance_factor.expect("a variance factor");
        assert!(factor < 1e-6);
        for residual in &network.residuals {
            assert!(
                !residual.observation.contains("circle"),
                "{}",
                residual.observation
            );
        }
    }

    #[test]
    fn a_setup_whose_backsight_nothing_places_is_oriented_on_the_circle_as_set_and_the_report_says_so()
     {
        // S1 on A backsights RO, which no coordinates and no other setup give.
        // The circle set on RO, 30 00 00, is then the only orientation there is:
        //   orientation = 30 - reading 0 on RO = 30;
        //   Q at reading 90 is azimuth 120, 20 m:
        //     N = 1000 + 20 cos 120 = 990, E = 1000 + 20 sin 120 = 1017.320508;
        //   RO at reading 0 is azimuth 30, 100 m:
        //     N = 1000 + 100 cos 30 = 1086.602540, E = 1000 + 100 sin 30 = 1050.
        let mut project = SurveyProject::default();
        project.points.push(point("A", 1000.0, 1000.0, Some(50.0)));
        project.unpositioned_points.push(unpositioned("RO"));
        project.unpositioned_points.push(unpositioned("Q"));
        project.stations.push(setup(
            "S1",
            "A",
            1.5,
            "RO",
            &[
                shot(
                    "RO",
                    1,
                    Face::Left,
                    Some(deg(0.0)),
                    Some(deg(90.0)),
                    Some(100.0),
                    1.5,
                ),
                shot(
                    "Q",
                    2,
                    Face::Left,
                    Some(deg(90.0)),
                    Some(deg(90.0)),
                    Some(20.0),
                    1.5,
                ),
            ],
        ));
        project.stations[0].backsight_azimuth = Some(deg(30.0));
        let outcome = reduce(&project, &bare_settings());
        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, 990.0, 1e-9);
        near(q.easting, 1017.320508, 1e-6);
        let ro = find_point(&outcome, "RO").expect("RO");
        near(ro.northing, 1086.602540, 1e-6);
        near(ro.easting, 1050.0, 1e-9);
        near(
            outcome.report.setups[0].orientation_correction.unwrap(),
            deg(30.0),
            1e-12,
        );
        assert_eq!(
            warnings_with(
                &outcome.report,
                "Setup S1: its backsight has no position, so its directions were oriented on the \
                 circle reading set on the backsight"
            ),
            1
        );
    }

    #[test]
    fn a_setup_whose_backsight_nothing_places_is_oriented_on_the_azimuth_the_file_states_not_the_circle()
     {
        // S1 on A backsights RO, which nothing places. The file states the
        // azimuth A -> RO, 30 00 00, and the circle set on RO, 0 00 00: a
        // circle zeroed on the backsight. The field software's orientation
        // (Sokkia SETX 29.2.6, A = H + BKB azimuth - BKB h.obs) is then 30 - 0:
        //   Q at reading 90 is azimuth 120, 20 m:
        //     N = 1000 + 20 cos 120 = 990, E = 1000 + 20 sin 120 = 1017.320508.
        // Taking the circle for the grid azimuth instead would put Q at azimuth
        // 90, N 1000 E 1020.
        let mut project = SurveyProject::default();
        project.points.push(point("A", 1000.0, 1000.0, Some(50.0)));
        project.unpositioned_points.push(unpositioned("RO"));
        project.unpositioned_points.push(unpositioned("Q"));
        project.stations.push(setup(
            "S1",
            "A",
            1.5,
            "RO",
            &[
                shot(
                    "RO",
                    1,
                    Face::Left,
                    Some(deg(0.0)),
                    Some(deg(90.0)),
                    Some(100.0),
                    1.5,
                ),
                shot(
                    "Q",
                    2,
                    Face::Left,
                    Some(deg(90.0)),
                    Some(deg(90.0)),
                    Some(20.0),
                    1.5,
                ),
            ],
        ));
        project.stations[0].backsight_azimuth = Some(deg(0.0));
        project.stations[0].stated_backsight_azimuth = Some(deg(30.0));
        let outcome = reduce(&project, &bare_settings());
        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, 990.0, 1e-9);
        near(q.easting, 1017.320508, 1e-6);
        let orientation = outcome.report.setups[0]
            .orientation_correction
            .expect("an orientation");
        near(orientation, deg(30.0), 1e-12);
        assert_eq!(
            warnings_with(
                &outcome.report,
                "Setup S1: its backsight has no position, so its directions were oriented on the \
                 azimuth the file states for the backsight"
            ),
            1
        );
        assert_eq!(warnings_with(&outcome.report, "taken as a grid azimuth"), 0);
    }

    #[test]
    fn with_no_reading_on_the_backsight_the_circle_set_on_it_is_what_the_azimuth_is_taken_from() {
        // S1 on A never observes its backsight RO. The file states the azimuth
        // A -> RO, 90 00 00, and the circle set on RO, 0 00 00: the orientation
        // is 90 - 0 = 90, so Q at reading 0, 100 m, is azimuth 90: N 1000,
        // E 1100, not due north, where the circle alone would put it.
        let mut project = SurveyProject::default();
        project.points.push(point("A", 1000.0, 1000.0, Some(50.0)));
        project.unpositioned_points.push(unpositioned("RO"));
        project.unpositioned_points.push(unpositioned("Q"));
        project.stations.push(setup(
            "S1",
            "A",
            1.5,
            "RO",
            &[shot(
                "Q",
                1,
                Face::Left,
                Some(deg(0.0)),
                Some(deg(90.0)),
                Some(100.0),
                1.5,
            )],
        ));
        project.stations[0].backsight_azimuth = Some(deg(0.0));
        project.stations[0].stated_backsight_azimuth = Some(deg(90.0));
        let stated = reduce(&project, &bare_settings());
        let q = find_point(&stated, "Q").expect("Q");
        near(q.northing, 1000.0, 1e-9);
        near(q.easting, 1100.0, 1e-9);

        // RO given coordinates, N 1000 E 1200 (azimuth 90 from A), and the
        // circle set on it 10 00 00. The file's azimuth, 45 on purpose, is not
        // used: coordinates orient the setup, less the circle set on RO, 90 -
        // 10 = 80, so Q at reading 0 is azimuth 80:
        //   N = 1000 + 100 cos 80 = 1017.364818, E = 1000 + 100 sin 80 = 1098.480775.
        project.unpositioned_points.remove(0);
        project.points.push(point("RO", 1000.0, 1200.0, Some(50.0)));
        project.stations[0].backsight_azimuth = Some(deg(10.0));
        project.stations[0].stated_backsight_azimuth = Some(deg(45.0));
        let placed = reduce(&project, &bare_settings());
        let q2 = find_point(&placed, "Q").expect("Q");
        near(q2.northing, 1017.364818, 1e-6);
        near(q2.easting, 1098.480775, 1e-6);
        assert_eq!(warnings_with(&placed.report, "azimuth the file states"), 0);
    }

    #[test]
    fn a_stated_azimuth_is_taken_less_the_setups_reading_on_the_backsight_not_less_the_circle() {
        // S1 on A backsights RO, which nothing places. The file states the
        // azimuth A -> RO, 30 00 00, the circle set on RO, 5 00 00, and the
        // setup reads RO at 10 00 00: the orientation is 30 - 10 = 20 (SETX
        // 29.2.6, the setup's reading standing in for the BKB h.obs), so Q at
        // reading 100, 20 m, is azimuth 120:
        //   N = 1000 + 20 cos 120 = 990, E = 1000 + 20 sin 120 = 1017.320508.
        // Less the circle it would be azimuth 125 (N 988.528, E 1016.383); the
        // stated azimuth alone, 130 (N 987.144, E 1015.321).
        let mut project = SurveyProject::default();
        project.points.push(point("A", 1000.0, 1000.0, Some(50.0)));
        project.unpositioned_points.push(unpositioned("RO"));
        project.unpositioned_points.push(unpositioned("Q"));
        project.stations.push(setup(
            "S1",
            "A",
            1.5,
            "RO",
            &[
                shot(
                    "RO",
                    1,
                    Face::Left,
                    Some(deg(10.0)),
                    Some(deg(90.0)),
                    Some(100.0),
                    1.5,
                ),
                shot(
                    "Q",
                    2,
                    Face::Left,
                    Some(deg(100.0)),
                    Some(deg(90.0)),
                    Some(20.0),
                    1.5,
                ),
            ],
        ));
        project.stations[0].backsight_azimuth = Some(deg(5.0));
        project.stations[0].stated_backsight_azimuth = Some(deg(30.0));
        let outcome = reduce(&project, &bare_settings());
        let orientation = outcome.report.setups[0]
            .orientation_correction
            .expect("an orientation");
        near(orientation, deg(20.0), 1e-12);
        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, 990.0, 1e-9);
        near(q.easting, 1017.320508, 1e-6);
        // The reduced project keeps both of the file's numbers.
        assert_eq!(outcome.reduced.stations.len(), 1);
        assert_eq!(
            outcome.reduced.stations[0].stated_backsight_azimuth,
            Some(deg(30.0))
        );
        assert_eq!(
            outcome.reduced.stations[0].backsight_azimuth,
            Some(deg(5.0))
        );
    }

    #[test]
    fn with_no_reading_on_the_backsight_the_stated_azimuth_is_taken_less_the_circle_set_on_it() {
        // S1 never observes RO. The file states the azimuth A -> RO, 30 00 00,
        // and the circle set on RO, 10 00 00: the orientation is 30 - 10 = 20,
        // so Q at reading 100, 20 m, is azimuth 120: N 990, E 1017.320508. The
        // stated azimuth alone would give azimuth 130.
        let mut project = SurveyProject::default();
        project.points.push(point("A", 1000.0, 1000.0, Some(50.0)));
        project.unpositioned_points.push(unpositioned("RO"));
        project.unpositioned_points.push(unpositioned("Q"));
        project.stations.push(setup(
            "S1",
            "A",
            1.5,
            "RO",
            &[shot(
                "Q",
                1,
                Face::Left,
                Some(deg(100.0)),
                Some(deg(90.0)),
                Some(20.0),
                1.5,
            )],
        ));
        project.stations[0].backsight_azimuth = Some(deg(10.0));
        project.stations[0].stated_backsight_azimuth = Some(deg(30.0));
        let outcome = reduce(&project, &bare_settings());
        let orientation = outcome.report.setups[0]
            .orientation_correction
            .expect("an orientation");
        near(orientation, deg(20.0), 1e-12);
        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, 990.0, 1e-9);
        near(q.easting, 1017.320508, 1e-6);
    }

    #[test]
    fn a_stated_azimuth_on_a_backsight_with_no_position_yet_waits_for_the_backsights_coordinates() {
        // As a_circle_set_on_a_backsight_with_no_position_yet_waits..., with an
        // azimuth the file states for S1's backsight P1 as well, 200 00 00,
        // wrong on purpose. P1 is placed by S2 later, and its coordinates orient
        // S1: the stated azimuth is the last resort, as the circle is.
        let mut project = later_backsight();
        project.stations[0].backsight_azimuth = Some(0.0);
        project.stations[0].stated_backsight_azimuth = Some(deg(200.0));
        let outcome = reduce(&project, &bare_settings());
        expect_later_backsight_coordinates(&outcome);
        assert_eq!(warnings_with(&outcome.report, "azimuth the file states"), 0);
        assert!(outcome.report.misclosures.is_empty());
    }

    #[test]
    fn a_setup_tried_again_for_its_backsight_says_each_thing_once() {
        // later_backsight() with no heights and distances reduced to the geoid:
        // S1 is reduced once while P1 has no position and again once S2 has
        // placed it. "Its point A has no height" is one sentence, not two. (With
        // no height nothing is reduced, so the coordinates stay the ones above.)
        let mut project = later_backsight();
        project.points[0].elevation = None;
        project.points[1].elevation = None;
        let settings = ReductionSettings {
            height_reduction: HeightReduction::Geoid,
            ..bare_settings()
        };
        let outcome = reduce(&project, &settings);
        assert_eq!(
            warnings_with(&outcome.report, "Setup S1: its point A has no height"),
            1
        );
        assert_eq!(
            warnings_with(&outcome.report, "Setup S2: its point B has no height"),
            1
        );
        expect_later_backsight_coordinates(&outcome);
    }
}
