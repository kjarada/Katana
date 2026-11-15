//! Twin of tests/survey/test_reduction_gnss.cpp: GNSS vectors in the
//! reduction: an RTK controller's earth-centred vectors (what a Trimble job's
//! ECEF deltas and a Topcon RW5 vector are) converted to grid at their base,
//! radiated, checked, and adjusted as baselines.
//!
//! The projection in these tests is one a hand can follow: grid north is +Y,
//! grid east is +X and the height is +Z, about a base at X0, Y0, Z0 that maps
//! to N 5000, E 3000, h 100. A real projection bends and scales; the
//! reduction only ever takes the difference of two converted points, so a
//! linear one tests exactly what the reduction does with it.

mod reduction_support;
mod survey_support;

mod reduction_gnss {
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::reduction_support::{
        bare_settings, find_correction, find_point, find_row, point, succeeded, unpositioned,
    };
    use crate::survey_support::{double_eq, near};
    use katana_math::numerics::HALF_PI;
    use katana_survey::data_model::{
        AntennaHeightMethod, ControlComponent, ControlConstraint, ControlPoint,
        GeocentricCoordinate, GeodeticCoordinate, GnssAntenna, GnssCovariance3,
        GnssGeocentricBaselineObservation, GnssGlobalPositionObservation, Observation,
        SourceRecord, SurveyProject,
    };
    use katana_survey::reduction::{
        GridPosition, ReductionContext, ReductionOutcome, reduce_and_adjust,
    };
    use katana_survey::reduction_report::{ComputationMethod, CorrectionKind, ReductionReport};
    use katana_survey::reduction_settings::{
        AdjustmentMethod, ControlSelection, NetworkDimension, ReductionSettings,
    };

    const X0: f64 = -4600000.0;
    const Y0: f64 = 2600000.0;
    const Z0: f64 = -3600000.0;

    fn linear_grid(c: &GeocentricCoordinate) -> Option<GridPosition> {
        Some(GridPosition {
            northing: 5000.0 + (c.y - Y0),
            easting: 3000.0 + (c.x - X0),
            ellipsoidal_height: 100.0 + (c.z - Z0),
        })
    }

    fn record(number: usize) -> SourceRecord {
        SourceRecord {
            manufacturer: String::from("Test"),
            format: String::from("synthetic"),
            format_version: String::from("1"),
            file_name: String::from("rtk.job"),
            record_number: number,
        }
    }

    fn global_at(point: &str, x: f64, y: f64, z: f64) -> GnssGlobalPositionObservation {
        GnssGlobalPositionObservation {
            point: String::from(point),
            geocentric: Some(GeocentricCoordinate { x, y, z }),
            source: record(1),
            ..Default::default()
        }
    }

    fn vector_of(
        from: &str,
        to: &str,
        dx: f64,
        dy: f64,
        dz: f64,
    ) -> GnssGeocentricBaselineObservation {
        GnssGeocentricBaselineObservation {
            from: String::from(from),
            to: String::from(to),
            delta: GeocentricCoordinate {
                x: dx,
                y: dy,
                z: dz,
            },
            source: record(2),
            ..Default::default()
        }
    }

    fn antenna(height: f64, method: AntennaHeightMethod) -> GnssAntenna {
        GnssAntenna {
            height,
            method,
            ..Default::default()
        }
    }

    // B1 (N 5000, E 3000, H 60) with its global position at X0, Y0, Z0; B2
    // (N 5100, E 3000, H 60) with its global position 100 m further along +Y.
    // R has no coordinates: only vectors reach it.
    fn two_bases() -> SurveyProject {
        let mut project = SurveyProject::default();
        project.points.push(point("B1", 5000.0, 3000.0, Some(60.0)));
        project.points.push(point("B2", 5100.0, 3000.0, Some(60.0)));
        project.unpositioned_points.push(unpositioned("R"));
        project
            .observations
            .push(Observation::GnssGlobalPosition(global_at("B1", X0, Y0, Z0)));
        project
            .observations
            .push(Observation::GnssGlobalPosition(global_at(
                "B2",
                X0,
                Y0 + 100.0,
                Z0,
            )));
        project
    }

    fn linear_context() -> ReductionContext {
        ReductionContext {
            geocentric_to_grid: Some(Box::new(linear_grid)),
            ..Default::default()
        }
    }

    fn warned(report: &ReductionReport, words: &str) -> bool {
        report
            .warnings
            .iter()
            .any(|warning| warning.text.contains(words))
    }

    fn reduce(
        project: &SurveyProject,
        settings: &ReductionSettings,
        context: &ReductionContext,
    ) -> ReductionOutcome {
        succeeded(reduce_and_adjust(project, settings, context))
    }

    #[test]
    fn a_vector_from_a_base_with_a_global_position_radiates_the_rover_mark_to_mark() {
        // B1 -> R: dX 30, dY 40, dZ 2, so in grid dN = +40, dE = +30 and the
        // ellipsoidal height rises 2.000.
        //   geoid N = 40 + 0.001 (n - 5000): 40.000 at B1, 40.040 at R, so the
        //   orthometric rise is 2.000 - 0.040 = 1.960;
        //   antennas to the phase centre, base 1.500, rover 2.000: +1.500 - 2.000;
        //   dH mark to mark = 1.960 - 0.500 = 1.460.
        // R = N 5040.000, E 3030.000, H 60.000 + 1.460 = 61.460.
        let mut project = two_bases();
        let mut vector = vector_of("B1", "R", 30.0, 40.0, 2.0);
        vector.from_antenna = antenna(1.5, AntennaHeightMethod::PhaseCentre);
        vector.to_antenna = antenna(2.0, AntennaHeightMethod::PhaseCentre);
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector));
        let mut context = linear_context();
        context.geoid_separation = Some(Box::new(|northing: f64, _: f64| {
            Some(40.0 + 0.001 * (northing - 5000.0))
        }));

        let outcome = reduce(&project, &bare_settings(), &context);
        let r = find_point(&outcome, "R").expect("R");
        near(r.northing, 5040.0, 1e-9);
        near(r.easting, 3030.0, 1e-9);
        near(r.elevation.expect("a height"), 61.460, 1e-9);
        assert_eq!(r.method, ComputationMethod::Gnss);

        // The vector's row: its length in space, sqrt(30^2 + 40^2 + 2^2) =
        // sqrt(2504) = 50.039984, to its grid length 50.000.
        let row = find_row(&outcome.report, "GNSS geocentric baseline", "R", 0).expect("row");
        near(row.raw, 50.0399840, 1e-6);
        near(row.reduced.expect("a reduced length"), 50.0, 1e-9);
        assert!(!row.rejected);
        // Its height row: 2.000 ellipsoidal, -0.040 geoid, -0.500 antennas.
        let height =
            find_row(&outcome.report, "GNSS height difference", "R", 0).expect("height row");
        near(height.raw, 2.0, 1e-9);
        assert_eq!(height.corrections.len(), 2);
        near(height.corrections[0].amount, -0.040, 1e-9);
        assert_eq!(
            height.corrections[1].kind,
            CorrectionKind::InstrumentAndTargetHeight
        );
        near(height.corrections[1].amount, -0.5, 1e-12);
        near(height.reduced.unwrap(), 1.460, 1e-9);
    }

    #[test]
    fn a_vector_whose_base_has_no_global_position_is_rejected_with_the_reason_not_dropped() {
        let mut project = two_bases();
        project.points.push(point("B3", 4000.0, 3000.0, Some(60.0))); // grid only
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector_of(
                "B3", "R", 30.0, 40.0, 2.0,
            )));
        let outcome = reduce(&project, &bare_settings(), &linear_context());
        let row = find_row(&outcome.report, "GNSS geocentric baseline", "R", 0).expect("row");
        assert!(row.rejected);
        assert!(
            row.rejection_reason
                .contains("no global position given as an observation")
        );
        // What is missing is an observation of it: a file may well key the
        // base's latitude and longitude in somewhere the reduction does not read.
        assert!(warned(
            &outcome.report,
            "its base B3 has no global position (latitude, longitude or X, Y, Z) among the \
             observations read"
        ));
        assert!(!warned(&outcome.report, "the file gives its base"));
        assert!(find_point(&outcome, "R").is_none());
    }

    #[test]
    fn vectors_convert_through_the_geodetic_conversion_when_the_drawing_gives_only_that() {
        // A drawing that converts latitude and longitude only. The base is at
        // latitude 0, longitude 0, height 0: on GRS80, X = a, Y = Z = 0. The
        // drawing here takes a geodetic point back to X, Y, Z by the textbook
        // formula and maps it as N = Z, E = Y, h = X - a, so the vector B1 -> R,
        // dX 0, dY 30, dZ 40, must come out as dN 40, dE 30: R = N 40, E 30,
        // whatever route the conversion takes, to the GRS80 inverse's precision.
        const A: f64 = 6378137.0;
        const F: f64 = 1.0 / 298.257222101;
        let mut project = SurveyProject::default();
        project.unpositioned_points.push(unpositioned("B1"));
        project.unpositioned_points.push(unpositioned("R"));
        project.observations.push(Observation::GnssGlobalPosition(
            GnssGlobalPositionObservation {
                point: String::from("B1"),
                geodetic: Some(GeodeticCoordinate::default()),
                source: record(1),
                ..Default::default()
            },
        ));
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector_of(
                "B1", "R", 0.0, 30.0, 40.0,
            )));
        let context = ReductionContext {
            geodetic_to_grid: Some(Box::new(|g: &GeodeticCoordinate| {
                let e2 = F * (2.0 - F);
                let n = A / (1.0 - e2 * g.latitude.sin() * g.latitude.sin()).sqrt();
                let x = (n + g.ellipsoidal_height) * g.latitude.cos() * g.longitude.cos();
                let y = (n + g.ellipsoidal_height) * g.latitude.cos() * g.longitude.sin();
                let z = (n * (1.0 - e2) + g.ellipsoidal_height) * g.latitude.sin();
                Some(GridPosition {
                    northing: z,
                    easting: y,
                    ellipsoidal_height: x - A,
                })
            })),
            ..Default::default()
        };
        let outcome = reduce(&project, &bare_settings(), &context);
        let row = find_row(&outcome.report, "GNSS geocentric baseline", "R", 0).expect("row");
        assert!(!row.rejected, "{}", row.rejection_reason);
        let r = find_point(&outcome, "R").expect("R");
        near(r.northing, 40.0, 1e-6);
        near(r.easting, 30.0, 1e-6);
        assert!(!warned(&outcome.report, "no coordinate system"));
    }

    #[test]
    fn a_global_position_is_taken_down_to_the_mark_by_its_antenna_height() {
        // G200's position is where its antenna was: the linear grid puts
        // X0, Y0, Z0 - 50 at N 5000, E 3000, h 100 - 50 = 50.000 (no geoid, so
        // ellipsoidal). The antenna is 2.000 m vertical to its reference point,
        // so the mark is at 50.000 - 2.000 = 48.000.
        // A vector G200 -> R, dX 30, dY 40, dZ 1, with 2.000 m vertical antennas
        // at both ends: dH mark to mark = 1.000 + 2.000 - 2.000 = 1.000, so
        // R = N 5040, E 3030, H 48.000 + 1.000 = 49.000, on the same vertical
        // reference as G200, not 2 m above it.
        let mut project = SurveyProject::default();
        project.unpositioned_points.push(unpositioned("G200"));
        project.unpositioned_points.push(unpositioned("R"));
        let mut position = global_at("G200", X0, Y0, Z0 - 50.0);
        position.antenna = antenna(2.0, AntennaHeightMethod::Vertical);
        position.antenna.measured_to = String::from("BottomOfAntennaMount");
        project
            .observations
            .push(Observation::GnssGlobalPosition(position));
        let mut vector = vector_of("G200", "R", 30.0, 40.0, 1.0);
        vector.from_antenna = antenna(2.0, AntennaHeightMethod::Vertical);
        vector.to_antenna = antenna(2.0, AntennaHeightMethod::Vertical);
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector));

        let outcome = reduce(&project, &bare_settings(), &linear_context());
        let g200 = find_point(&outcome, "G200").expect("G200");
        near(g200.elevation.expect("a height"), 48.0, 1e-9);
        let r = find_point(&outcome, "R").expect("R");
        near(r.elevation.expect("a height"), 49.0, 1e-9);

        // The position's row: 50.000 at the antenna, -2.000 for the antenna,
        // 48.000 at the mark.
        let row =
            find_row(&outcome.report, "GNSS geocentric position", "G200", 0).expect("the row");
        near(row.raw, 50.0, 1e-9);
        let taken = find_correction(row, CorrectionKind::InstrumentAndTargetHeight)
            .expect("the antenna height");
        near(taken.amount, -2.0, 1e-12);
        assert!(taken.note.contains("BottomOfAntennaMount"));
        near(row.reduced.expect("a reduced height"), 48.0, 1e-9);
        // A height to the antenna's reference point leaves its phase-centre
        // offset in, and the report says so.
        assert!(warned(
            &outcome.report,
            "1 GNSS position(s) have antenna heights to the antenna's reference point"
        ));
    }

    #[test]
    fn a_global_position_with_a_slant_antenna_height_keeps_its_place_and_leaves_its_height_out() {
        let mut project = SurveyProject::default();
        project.unpositioned_points.push(unpositioned("G300"));
        let mut position = global_at("G300", X0, Y0, Z0 - 50.0);
        position.antenna = antenna(1.62, AntennaHeightMethod::Slant);
        project
            .observations
            .push(Observation::GnssGlobalPosition(position));
        let outcome = reduce(&project, &bare_settings(), &linear_context());
        let g300 = find_point(&outcome, "G300").expect("G300");
        near(g300.northing, 5000.0, 1e-9);
        assert!(g300.elevation.is_none());
        assert!(warned(
            &outcome.report,
            "GNSS position of G300: its antenna height (1.620 m"
        ));
    }

    #[test]
    fn a_second_gnss_position_of_a_point_is_a_check_on_the_first_not_silently_unused() {
        // G1 occupied twice: N 5000.000 (record 1), then 40 mm further north
        // (record 3). The first places it; the second is a check: +0.040 N,
        // 0 E, 0.040 linear.
        let mut project = SurveyProject::default();
        project.unpositioned_points.push(unpositioned("G1"));
        project
            .observations
            .push(Observation::GnssGlobalPosition(global_at("G1", X0, Y0, Z0)));
        let mut again = global_at("G1", X0, Y0 + 0.040, Z0);
        again.source.record_number = 3;
        project
            .observations
            .push(Observation::GnssGlobalPosition(again));
        let outcome = reduce(&project, &bare_settings(), &linear_context());
        let g1 = find_point(&outcome, "G1").expect("G1");
        near(g1.northing, 5000.0, 1e-9);
        assert_eq!(outcome.report.misclosures.len(), 1);
        let check = &outcome.report.misclosures[0];
        assert_eq!(check.name, "G1 by a second GNSS position (record 3)");
        near(check.northing.unwrap(), 0.040, 1e-9);
        near(check.easting.unwrap(), 0.0, 1e-9);
        near(check.linear.unwrap(), 0.040, 1e-9);
    }

    #[test]
    fn two_gnss_positions_of_one_point_enter_the_network_each_with_its_own_value() {
        // The same two occupations adjusted: two observations of G1's northing,
        // 5000.000 and 5000.040, and two of its easting, both 3000.000, each at
        // the a-priori 10 mm; unknowns G1's N and E (the positions are the datum).
        //   N = mean = 5000.020; v = +0.020, -0.020 (adjusted - observed);
        //   E = 3000.000; v = 0, 0.
        //   v'Pv = 2 (0.020 / 0.010)^2 = 8; redundancy 4 - 2 = 2;
        //   variance factor 8 / 2 = 4.
        // Built from one seeded value twice, it would be N 5000.000 with
        // variance factor 0.
        let mut project = SurveyProject::default();
        project.unpositioned_points.push(unpositioned("G1"));
        project
            .observations
            .push(Observation::GnssGlobalPosition(global_at("G1", X0, Y0, Z0)));
        let mut again = global_at("G1", X0, Y0 + 0.040, Z0);
        again.source.record_number = 3;
        project
            .observations
            .push(Observation::GnssGlobalPosition(again));
        let settings = ReductionSettings {
            method: AdjustmentMethod::Network,
            ..bare_settings()
        };
        let outcome = reduce(&project, &settings, &linear_context());
        let g1 = find_point(&outcome, "G1").expect("G1");
        near(g1.northing, 5000.020, 1e-7);
        near(g1.easting, 3000.0, 1e-7);
        assert_eq!(outcome.report.adjustments.len(), 1);
        let network = &outcome.report.adjustments[0];
        assert_eq!(network.observations, 4);
        assert_eq!(network.redundancy, 2);
        near(
            network.variance_factor.expect("a variance factor"),
            4.0,
            1e-5,
        );
        let mut first = None;
        let mut second = None;
        for residual in &network.residuals {
            if residual.observation == "GNSS position G1 (record 1) (north)" {
                first = Some(residual.residual);
            } else if residual.observation == "GNSS position G1 (record 3) (north)" {
                second = Some(residual.residual);
            }
        }
        near(first.expect("the first's residual"), 0.020, 1e-7);
        near(second.expect("the second's residual"), -0.020, 1e-7);
    }

    #[test]
    fn a_geocentric_position_keeps_the_covariance_its_file_states_turned_to_north_east_and_up() {
        // On the equator at the prime meridian, X = a: north is +Z, east is +Y,
        // up is +X. A covariance of XX 2.5e-5, YY 6.4e-5, ZZ 3.6e-5 is sigma
        // north sqrt(ZZ) = 0.006, east sqrt(YY) = 0.008, up sqrt(XX) = 0.005;
        // the horizontal sigma is the larger, 0.008. Not the a-priori 10 / 20 mm.
        const A: f64 = 6378137.0;
        let mut project = SurveyProject::default();
        project.unpositioned_points.push(unpositioned("G1"));
        let mut position = global_at("G1", A, 0.0, 0.0);
        position.covariance = GnssCovariance3 {
            xx: 2.5e-5,
            yy: 6.4e-5,
            zz: 3.6e-5,
            xy: 0.0,
            xz: 0.0,
            yz: 0.0,
        };
        project
            .observations
            .push(Observation::GnssGlobalPosition(position));
        let context = ReductionContext {
            geocentric_to_grid: Some(Box::new(|c: &GeocentricCoordinate| {
                Some(GridPosition {
                    northing: c.z,
                    easting: c.y,
                    ellipsoidal_height: c.x - 6378137.0 + 10.0,
                })
            })),
            ..Default::default()
        };
        let outcome = reduce(&project, &bare_settings(), &context);
        let g1 = find_point(&outcome, "G1").expect("G1");
        near(g1.sigma_northing.expect("a precision"), 0.008, 1e-12);
        near(g1.sigma_easting.expect("a precision"), 0.008, 1e-12);
        near(g1.sigma_elevation.expect("a precision"), 0.005, 1e-12);
    }

    #[test]
    fn without_a_coordinate_system_every_vector_is_rejected_and_the_report_says_why() {
        let mut project = two_bases();
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector_of(
                "B1", "R", 30.0, 40.0, 2.0,
            )));
        let outcome = reduce(&project, &bare_settings(), &ReductionContext::default());
        let row = find_row(&outcome.report, "GNSS geocentric baseline", "R", 0).expect("row");
        assert!(row.rejected);
        assert!(warned(&outcome.report, "1 GNSS vector(s) were not used"));
    }

    #[test]
    fn a_slant_antenna_height_keeps_the_horizontal_position_and_leaves_the_height_out() {
        let mut project = two_bases();
        let mut vector = vector_of("B1", "R", 30.0, 40.0, 2.0);
        vector.from_antenna = antenna(1.5, AntennaHeightMethod::Vertical);
        vector.to_antenna = antenna(1.62, AntennaHeightMethod::Slant);
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector));
        let outcome = reduce(&project, &bare_settings(), &linear_context());
        let r = find_point(&outcome, "R").expect("R");
        near(r.northing, 5040.0, 1e-9);
        assert!(r.elevation.is_none());
        let height =
            find_row(&outcome.report, "GNSS height difference", "R", 0).expect("height row");
        assert!(height.rejected);
        assert!(warned(&outcome.report, "not a vertical one"));
    }

    #[test]
    fn a_second_vector_to_the_same_rover_is_a_check_with_its_misclosure() {
        // B2 -> R: dX 30, dY -59.990, dZ 1.020 puts R at N 5100 - 59.990 =
        // 5040.010: 10 mm north of where B1's vector put it, 20 mm higher.
        let mut project = two_bases();
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector_of(
                "B1", "R", 30.0, 40.0, 1.0,
            )));
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector_of(
                "B2", "R", 30.0, -59.990, 1.020,
            )));
        let outcome = reduce(&project, &bare_settings(), &linear_context());
        let r = find_point(&outcome, "R").expect("R");
        near(r.northing, 5040.0, 1e-9); // the first vector placed it
        assert_eq!(outcome.report.misclosures.len(), 1);
        let check = &outcome.report.misclosures[0];
        assert_eq!(check.name, "R by GNSS vector from B2");
        near(check.northing.unwrap(), 0.010, 1e-9);
        near(check.easting.unwrap(), 0.0, 1e-9);
        near(check.height.unwrap(), 0.020, 1e-9);
    }

    #[test]
    fn two_vectors_to_one_rover_adjust_to_their_mean_with_the_hand_worked_statistics() {
        // As above, adjusted: horizontal and levels, both bases held.
        // Horizontal: each vector gives two equations (N, E) at the a-priori
        // 10 mm; unknowns R's N and E. 4 - 2 = redundancy 2.
        //   R = mean: N 5040.005, E 3030.000; v(N) = +/-0.005, v(E) = 0.
        //   v'Pv = 2 (0.005 / 0.010)^2 = 0.5; variance factor 0.5 / 2 = 0.25.
        //   Qxx(R) = sigma^2 / 2 = 5e-5 m^2 per axis; scaled by 0.25:
        //   sigma(N) = sigma(E) = sqrt(1.25e-5) = 0.0035355.
        //   The 95 % ellipse scale for two dimensions: sqrt(-2 ln 0.05) =
        //   sqrt(5.991465) = 2.447747.
        // Levels: 1.000 and 1.020 at the a-priori 20 mm; one unknown.
        //   R = 61.010; v = +/-0.010; v'Pv = 2 (0.010 / 0.020)^2 = 0.5, r = 1.
        let mut project = two_bases();
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector_of(
                "B1", "R", 30.0, 40.0, 1.0,
            )));
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector_of(
                "B2", "R", 30.0, -59.990, 1.020,
            )));
        let settings = ReductionSettings {
            method: AdjustmentMethod::Network,
            network_dimension: NetworkDimension::HorizontalAndLevels,
            control: vec![
                ControlSelection {
                    point: ControlPoint::fixed3d("B1"),
                    ..Default::default()
                },
                ControlSelection {
                    point: ControlPoint::fixed3d("B2"),
                    ..Default::default()
                },
            ],
            ..bare_settings()
        };

        let outcome = reduce(&project, &settings, &linear_context());
        let r = find_point(&outcome, "R").expect("R");
        near(r.northing, 5040.005, 1e-9);
        near(r.easting, 3030.0, 1e-9);
        near(r.elevation.expect("a height"), 61.010, 1e-9);
        assert_eq!(r.method, ComputationMethod::NetworkLeastSquares);
        near(r.sigma_northing.expect("a precision"), 0.0035355, 1e-7);

        assert_eq!(outcome.report.adjustments.len(), 2);
        let horizontal = &outcome.report.adjustments[0];
        assert_eq!(horizontal.observations, 4);
        assert_eq!(horizontal.unknowns, 2);
        assert_eq!(horizontal.redundancy, 2);
        // To 1e-7: the vectors end on Y values of 2.6e6 m, whose last digit is
        // 5e-10 m, a relative 1e-7 of the 5 mm residuals.
        near(
            horizontal.variance_factor.expect("a variance factor"),
            0.25,
            1e-7,
        );
        assert_eq!(horizontal.ellipses.len(), 1);
        assert_eq!(horizontal.ellipses[0].point_id, "R");
        near(horizontal.ellipses[0].confidence_scale, 2.447747, 1e-6);
        near(horizontal.ellipses[0].standard.semi_major, 0.0035355, 1e-7);

        let levels = &outcome.report.adjustments[1];
        assert_eq!(levels.redundancy, 1);
        near(
            levels.variance_factor.expect("a variance factor"),
            0.5,
            1e-9,
        );
        // Without a geoid the heights are ellipsoidal, and the report says so.
        assert!(warned(&outcome.report, "no geoid separation"));
    }

    #[test]
    fn a_weighted_control_height_moves_by_its_share_of_the_misclosure() {
        // As above, but B2 held horizontally and WEIGHTED in height at 20 mm.
        // Minimise (R - 61.000)^2 + (R - B2 - 1.020)^2 + (B2 - 60.000)^2, all
        // at 20 mm:
        //   dR:  2R - B2 = 62.020      dB2: -R + 2 B2 = 58.980
        //   3R = 2 * 62.020 + 58.980 = 183.020: R = 61.006667, B2 = 59.993333.
        //   v = +0.006667, -0.006667 (the vectors), -0.006667 (B2's height);
        //   v'Pv = 3 (0.006667 / 0.020)^2 = 0.333333, r = 3 - 2 = 1.
        let mut project = two_bases();
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector_of(
                "B1", "R", 30.0, 40.0, 1.0,
            )));
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector_of(
                "B2", "R", 30.0, -59.990, 1.020,
            )));
        let mut b2 = ControlPoint::fixed_horizontal("B2");
        b2.elevation = ControlComponent {
            constraint: ControlConstraint::Weighted,
            sigma: 0.020,
        };
        let settings = ReductionSettings {
            method: AdjustmentMethod::Network,
            network_dimension: NetworkDimension::HorizontalAndLevels,
            control: vec![
                ControlSelection {
                    point: ControlPoint::fixed3d("B1"),
                    ..Default::default()
                },
                ControlSelection {
                    point: b2,
                    ..Default::default()
                },
            ],
            ..bare_settings()
        };

        let outcome = reduce(&project, &settings, &linear_context());
        near(
            find_point(&outcome, "R").unwrap().elevation.unwrap(),
            61.006667,
            1e-6,
        );
        near(
            find_point(&outcome, "B2").unwrap().elevation.unwrap(),
            59.993333,
            1e-6,
        );
        // Horizontally B2 is held: it does not move.
        near(find_point(&outcome, "B2").unwrap().northing, 5100.0, 1e-12);
        let levels = &outcome.report.adjustments[1];
        assert_eq!(levels.unknowns, 2);
        assert_eq!(levels.redundancy, 1);
        near(levels.variance_factor.unwrap(), 0.333333, 1e-6);
        let mut control_residual = false;
        for residual in &levels.residuals {
            if residual.observation == "control height B2" {
                control_residual = true;
                near(residual.residual, -0.006667, 1e-6);
            }
        }
        assert!(control_residual);
    }

    #[test]
    fn the_files_covariance_is_turned_to_north_east_and_up_at_the_base() {
        // A base on the equator at the prime meridian, X = a = 6 378 137: there
        // north is +Z, east is +Y and up is +X, so a covariance of
        // XX 4e-4, YY 1e-4, ZZ 9e-6 is sigma north sqrt(ZZ) = 0.003,
        // east sqrt(YY) = 0.010, up sqrt(XX) = 0.020.
        const A: f64 = 6378137.0;
        let mut project = SurveyProject::default();
        project.points.push(point("B1", 0.0, 0.0, Some(10.0)));
        project.points.push(point("B2", 100.0, 0.0, Some(10.0)));
        project.unpositioned_points.push(unpositioned("R"));
        project
            .observations
            .push(Observation::GnssGlobalPosition(global_at(
                "B1", A, 0.0, 0.0,
            )));
        project
            .observations
            .push(Observation::GnssGlobalPosition(global_at(
                "B2", A, 0.0, 100.0,
            )));
        let mut first = vector_of("B1", "R", 0.0, 30.0, 40.0);
        first.covariance = GnssCovariance3 {
            xx: 4e-4,
            yy: 1e-4,
            zz: 9e-6,
            xy: 0.0,
            xz: 0.0,
            yz: 0.0,
        };
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(first));
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector_of(
                "B2", "R", 0.0, 30.0, -60.0,
            )));
        let context = ReductionContext {
            geocentric_to_grid: Some(Box::new(|c: &GeocentricCoordinate| {
                Some(GridPosition {
                    northing: c.z,
                    easting: c.y,
                    ellipsoidal_height: c.x - 6378137.0 + 10.0,
                })
            })),
            ..Default::default()
        };
        let settings = ReductionSettings {
            method: AdjustmentMethod::Network,
            control: vec![
                ControlSelection {
                    point: ControlPoint::fixed_horizontal("B1"),
                    ..Default::default()
                },
                ControlSelection {
                    point: ControlPoint::fixed_horizontal("B2"),
                    ..Default::default()
                },
            ],
            ..bare_settings()
        };

        let outcome = reduce(&project, &settings, &context);
        let horizontal = &outcome.report.adjustments[0];
        let mut north = None;
        let mut east = None;
        for residual in &horizontal.residuals {
            if residual.observation == "GNSS vector B1 -> R (north)" {
                north = Some(residual.sigma);
            } else if residual.observation == "GNSS vector B1 -> R (east)" {
                east = Some(residual.sigma);
            }
        }
        near(north.expect("the north sigma"), 0.003, 1e-12);
        near(east.expect("the east sigma"), 0.010, 1e-12);
    }

    #[test]
    fn a_geodetic_base_position_is_taken_to_xyz_on_the_grs80_ellipsoid() {
        // Latitude 0, longitude 0, height 0 is X = a = 6 378 137, Y = Z = 0;
        // latitude 90 is Z = b = a (1 - f) = 6 356 752.314140.
        let mut project = SurveyProject::default();
        project.points.push(point("B1", 0.0, 0.0, Some(10.0)));
        project.unpositioned_points.push(unpositioned("R"));
        project.observations.push(Observation::GnssGlobalPosition(
            GnssGlobalPositionObservation {
                point: String::from("B1"),
                geodetic: Some(GeodeticCoordinate::default()),
                ..Default::default()
            },
        ));
        project
            .observations
            .push(Observation::GnssGeocentricBaseline(vector_of(
                "B1", "R", 0.0, 30.0, 40.0,
            )));
        // The C++ lambda captures the list by reference; the context's
        // functions here are Fn, so the list is shared and borrowed mutably
        // inside.
        let asked: Rc<RefCell<Vec<GeocentricCoordinate>>> = Rc::new(RefCell::new(Vec::new()));
        let recorder = Rc::clone(&asked);
        let context = ReductionContext {
            geocentric_to_grid: Some(Box::new(move |c: &GeocentricCoordinate| {
                recorder.borrow_mut().push(*c);
                Some(GridPosition {
                    northing: c.z,
                    easting: c.y,
                    ellipsoidal_height: 0.0,
                })
            })),
            ..Default::default()
        };
        let outcome = reduce(&project, &bare_settings(), &context);
        {
            // The base's position is converted, then the rover's, base + delta.
            let asked = asked.borrow();
            assert!(asked.len() >= 2);
            double_eq(asked[0].x, 6378137.0);
            double_eq(asked[0].y, 0.0);
            double_eq(asked[0].z, 0.0);
            let last = asked.last().expect("a conversion");
            double_eq(last.x, 6378137.0);
            double_eq(last.y, 30.0);
            double_eq(last.z, 40.0);
        }
        let r = find_point(&outcome, "R").expect("R");
        near(r.northing, 40.0, 1e-9);
        near(r.easting, 30.0, 1e-9);

        project.observations[0] = Observation::GnssGlobalPosition(GnssGlobalPositionObservation {
            point: String::from("B1"),
            geodetic: Some(GeodeticCoordinate {
                latitude: HALF_PI,
                longitude: 0.0,
                ellipsoidal_height: 0.0,
            }),
            ..Default::default()
        });
        asked.borrow_mut().clear();
        assert!(reduce_and_adjust(&project, &bare_settings(), &context).is_ok());
        let asked = asked.borrow();
        assert!(!asked.is_empty());
        near(asked[0].z, 6356752.314140, 1e-6);
    }
}
