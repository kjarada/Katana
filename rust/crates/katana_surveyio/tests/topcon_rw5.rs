//! Twin of tests/surveyio/test_topcon_rw5.cpp: the RW5 reader
//! (src/topcon_rw5.rs).
//!
//! Fixtures are hand-built from the TDS Raw Data Record Specification (Survey
//! Pro 3.6) and the Carlson SurvCE RW5 format 3.03; gnss.rw5 is made of the
//! SurvCE 3.03 document's own sample records, and survce250_us_feet.rw5 is the
//! sample job printed in Carlson's "SurvCE Version 2.50 Raw File records".
//! Every expected value is worked below from the record text, not taken from
//! the reader's output.

#![allow(
    clippy::excessive_precision,
    reason = "Expected values are the C++ test's literals, digit for digit."
)]

mod topcon_support;

use std::f64::consts::PI;
use std::time::Instant;

use katana_core::ErrorCode;
use katana_core::text::parse_finite_double;
use katana_survey::{
    AngularUnit, AntennaHeightMethod, AzimuthObservation, CorrectionState, DistanceKind,
    DistanceObservation, Face, GeocentricCoordinate, GnssGeocentricBaselineObservation,
    GnssSolution, HorizontalDirectionObservation, LevelDifferenceObservation, LinearUnit,
    Observation, Pointing, VerticalAngleObservation, ZenithAngleObservation,
};
use katana_surveyio::{
    DetectionOutcome, ReadOptions, ReadResult, detect_format, format_registry, probe_of,
};

use topcon_support::{
    FOREIGN_SAMPLES, any_not_carried_contains, any_warning_contains, dms, double_eq, feature,
    fixture, near, observations_to, ok, point, read, read_every_truncation_and_noise, unpositioned,
};

const RW5: &str = "tds-rw5";
// radians: far below 0.001"
const ANGLE_TOLERANCE: f64 = 1e-12;

fn read_fixture(name: &str) -> ReadResult {
    let bytes = fixture(format!("rw5/{name}"));
    ok(read(RW5, bytes, name))
}

// WGS 84 geodetic to geocentric, the textbook closed form (a = 6378137 m,
// 1/f = 298.257223563), written out here so that a vector can be checked
// against the two positions a file gives for its ends without the reader.
fn wgs84_geocentric(latitude: f64, longitude: f64, height: f64) -> GeocentricCoordinate {
    let a: f64 = 6378137.0;
    let f: f64 = 1.0 / 298.257223563;
    let e2 = f * (2.0 - f);
    let n = a / (1.0 - e2 * latitude.sin() * latitude.sin()).sqrt();
    GeocentricCoordinate {
        x: (n + height) * latitude.cos() * longitude.cos(),
        y: (n + height) * latitude.cos() * longitude.sin(),
        z: (n * (1.0 - e2) + height) * latitude.sin(),
    }
}

fn first_vector(result: &ReadResult) -> Option<&GnssGeocentricBaselineObservation> {
    result
        .project
        .observations
        .iter()
        .find_map(|observation| match observation {
            Observation::GnssGeocentricBaseline(vector) => Some(vector),
            _ => None,
        })
}

fn vectors(result: &ReadResult) -> Vec<GnssGeocentricBaselineObservation> {
    result
        .project
        .observations
        .iter()
        .filter_map(|observation| match observation {
            Observation::GnssGeocentricBaseline(vector) => Some(vector.clone()),
            _ => None,
        })
        .collect()
}

mod topcon_rw5 {
    use super::*;

    #[test]
    fn is_registered_as_an_importable_format_that_reads_setups_observations_and_gnss() {
        let format = format_registry().find(RW5).expect("registered");
        assert!(format.can_import);
        assert!(!format.can_export);
        assert_eq!(format.parser_version, "1.0");
        assert_eq!(format.extensions, ["rw5"]);
        assert!(
            format.reads.observations
                && format.reads.stations
                && format.reads.points
                && format.reads.features
                && format.reads.instrument_settings
                && format.reads.gnss
        );
        assert!(format_registry().reader(RW5).is_some());
    }

    #[test]
    fn packed_degrees_are_read_as_degrees_minutes_and_seconds_not_as_a_decimal() {
        let result = read_fixture("setup_metres.rw5");
        assert_eq!(result.project.stations.len(), 1);
        let directions =
            observations_to::<HorizontalDirectionObservation>(&result.project.stations[0], "10");
        // SS, FD and FR to point 10
        assert_eq!(directions.len(), 3);
        // AR90.3333 is 90 33' 33" (= 326013"), not 90.3333 degrees.
        near(directions[0].direction, dms(90, 33, 33.0), ANGLE_TOLERANCE);
        // FR AR270.3336 is 270 33' 36".
        near(directions[2].direction, dms(270, 33, 36.0), ANGLE_TOLERANCE);
    }

    #[test]
    fn an_occupy_record_becomes_one_setup_with_coordinates_heights_and_backsight() {
        let result = read_fixture("setup_metres.rw5");
        let project = &result.project;
        assert_eq!(project.name, "KATANA TEST");
        assert_eq!(project.units.linear, LinearUnit::Metres);
        assert_eq!(project.units.angular, AngularUnit::DegreesMinutesSeconds);
        assert_eq!(project.stations.len(), 1);
        let station = &project.stations[0];
        assert_eq!(station.setup.id, "1");
        assert_eq!(station.setup.point_id, "1");
        // LS,HI1.550 came after the OC record but before any shot.
        double_eq(station.setup.instrument_height, 1.55);
        assert_eq!(station.backsight_point_id, "2");
        // BC0.0000
        double_eq(station.backsight_azimuth.expect("a circle"), 0.0);
        // BS0.0000
        assert_eq!(station.metadata["backsight azimuth (radians)"], "0");
        assert_eq!(station.metadata["notes"], "Setup check ok");

        let occupied = point(project, "1").expect("1");
        double_eq(occupied.northing, 1000.0);
        double_eq(occupied.easting, 2000.0);
        double_eq(occupied.elevation.expect("an elevation"), 50.0);
        // the SP record, first to position it
        assert_eq!(occupied.source.record_number, 3);
        // Shot targets with no coordinates stay unpositioned.
        assert!(unpositioned(project, "10").is_some());
        assert!(unpositioned(project, "12").is_some());
        assert!(point(project, "10").is_none());
    }

    #[test]
    fn a_sideshot_gives_direction_zenith_and_slope_distance_sharing_one_pointing() {
        let result = read_fixture("setup_metres.rw5");
        let station = &result.project.stations[0];
        let directions = observations_to::<HorizontalDirectionObservation>(station, "10");
        let zeniths = observations_to::<ZenithAngleObservation>(station, "10");
        let distances = observations_to::<DistanceObservation>(station, "10");
        assert!(!directions.is_empty());
        assert!(!zeniths.is_empty());
        assert!(!distances.is_empty());
        // SS,OP1,FP10 is the second shot of the setup: pointing 2, face left
        // (ZE91.1015 is below 180 degrees).
        let expected = Pointing {
            index: 2,
            face: Face::Left,
        };
        assert_eq!(directions[0].pointing, expected);
        assert_eq!(zeniths[0].pointing, expected);
        assert_eq!(distances[0].pointing, expected);
        near(zeniths[0].angle, dms(91, 10, 15.0), ANGLE_TOLERANCE);
        double_eq(distances[0].distance, 25.55);
        assert_eq!(distances[0].kind, DistanceKind::Slope);
        double_eq(distances[0].instrument_height, 1.55);
        double_eq(distances[0].target_height, 1.8);
        double_eq(zeniths[0].target_height, 1.8);
        // Default precision: 3" and 2 mm + 2 ppm -> hypot(0.002, 2e-6 * 25.55).
        double_eq(directions[0].sigma, 3.0 * PI / 648000.0);
        double_eq(distances[0].sigma, 0.002_f64.hypot(2e-6 * 25.55));
        assert_eq!(distances[0].source.record_number, 9);
        assert_eq!(distances[0].source.file_name, "setup_metres.rw5");
    }

    #[test]
    fn direct_and_reverse_records_carry_their_face_and_reverse_zeniths_are_folded() {
        let result = read_fixture("setup_metres.rw5");
        let station = &result.project.stations[0];
        let zeniths = observations_to::<ZenithAngleObservation>(station, "2");
        // SS, BD, BR
        assert_eq!(zeniths.len(), 3);
        // BD
        assert_eq!(
            zeniths[1].pointing,
            Pointing {
                index: 5,
                face: Face::Left
            }
        );
        // BR
        assert_eq!(
            zeniths[2].pointing,
            Pointing {
                index: 6,
                face: Face::Right
            }
        );
        // BR ZE270.3458: 360 00' 00" - 270 34' 58" = 89 25' 02" (1296000" - 974098").
        near(zeniths[2].angle, dms(89, 25, 2.0), ANGLE_TOLERANCE);
        // SS to 11 at ZE271.3000 is a single face-right shot: 88 30' 00".
        let eleven = observations_to::<ZenithAngleObservation>(station, "11");
        assert_eq!(eleven.len(), 1);
        assert_eq!(eleven[0].pointing.face, Face::Right);
        near(eleven[0].angle, dms(88, 30, 0.0), ANGLE_TOLERANCE);
        // FR ZE268.4952: 360 - 268 49' 52" = 91 10' 08".
        let ten = observations_to::<ZenithAngleObservation>(station, "10");
        assert_eq!(ten.len(), 3);
        near(ten[2].angle, dms(91, 10, 8.0), ANGLE_TOLERANCE);
        assert_eq!(ten[2].pointing.face, Face::Right);
    }

    #[test]
    fn a_zero_slope_distance_is_a_warning_and_no_distance_not_a_zero_length_observation() {
        let result = read_fixture("setup_metres.rw5");
        let station = &result.project.stations[0];
        assert!(observations_to::<DistanceObservation>(station, "12").is_empty());
        assert_eq!(
            observations_to::<ZenithAngleObservation>(station, "12").len(),
            1
        );
        assert!(any_warning_contains(&result, 12, "no distance measured"));
        // LS,HR2.000 (record 11) is the target height from then on.
        double_eq(
            observations_to::<ZenithAngleObservation>(station, "12")[0].target_height,
            2.0,
        );
    }

    #[test]
    fn the_mode_record_sets_scale_curvature_and_the_prism_constant_in_metres() {
        let result = read_fixture("setup_metres.rw5");
        let settings = &result.project.stations[0].instrument;
        // SF0.99960000, which the collector applies when it computes, not to a
        // recorded distance.
        double_eq(settings.scale_factor.expect("a scale factor"), 0.9996);
        assert_eq!(settings.scale_factor_state, CorrectionState::NotApplied);
        // EC1: a recorded HD or CE may include it; raw readings never do.
        assert_eq!(
            settings.curvature_refraction_state,
            CorrectionState::Unknown
        );
        // EO1.0 inch = 0.0254 m, state not stated by the format.
        double_eq(settings.prism_constant.expect("a prism constant"), 0.0254);
        assert_eq!(settings.prism_constant_state, CorrectionState::Unknown);
        // Each distance carries the offset in force when it was measured.
        let distances = observations_to::<DistanceObservation>(&result.project.stations[0], "10");
        assert!(!distances.is_empty());
        double_eq(
            distances[0]
                .target
                .prism_constant
                .expect("the shot's constant"),
            0.0254,
        );
        assert_eq!(
            distances[0].target.prism_constant_state,
            CorrectionState::Unknown
        );
        assert_eq!(settings.atmospheric_ppm_state, CorrectionState::Unknown);
        assert!(any_not_carried_contains(&result, "no atmospheric settings"));
    }

    #[test]
    fn an_edm_offset_changed_mid_setup_is_carried_by_the_shots_after_it_only() {
        let bytes = concat!(
            "MO,AD0,UN1,SF1.0,EC0,EO0.0,AU0\n",
            "OC,OP1,N 0.0,E 0.0,EL0.0\n",
            "SS,OP1,FP2,AR0.0000,ZE90.0000,SD10.000\n",
            "MO,AD0,UN1,SF1.0,EC0,EO-1.0,AU0\n",
            "SS,OP1,FP3,AR90.0000,ZE90.0000,SD10.000\n"
        );
        let result = ok(read(RW5, bytes, "prism.rw5"));
        let station = &result.project.stations[0];
        // The setup began under EO0.0.
        double_eq(station.instrument.prism_constant.expect("a constant"), 0.0);
        let before = observations_to::<DistanceObservation>(station, "2");
        let after = observations_to::<DistanceObservation>(station, "3");
        assert!(before.len() == 1 && after.len() == 1);
        double_eq(before[0].target.prism_constant.expect("a constant"), 0.0);
        // EO-1.0 inch = -0.0254 m.
        double_eq(after[0].target.prism_constant.expect("a constant"), -0.0254);
    }

    #[test]
    fn descriptions_become_codes_and_string_points_into_features_in_observation_order() {
        let result = read_fixture("setup_metres.rw5");
        let project = &result.project;
        let kerb = feature(project, "KB1").expect("KB1");
        // 10 then 11; the FD and FR shots to 10 add no second vertex.
        assert_eq!(kerb.point_ids, ["10", "11"]);
        let control = feature(project, "CP").expect("CP");
        assert_eq!(control.point_ids, ["1", "2"]);
        let tree = unpositioned(project, "12").expect("12");
        assert_eq!(tree.code, "TREE");
        // The note runs to the end of the line, commas and all.
        assert_eq!(tree.description, "TREE oak, 30 cm");
        assert_eq!(point(project, "1").expect("1").description, "CP control");
    }

    #[test]
    fn feet_and_grads_are_converted_and_south_azimuths_turned_through_half_a_circle() {
        let result = read_fixture("feet_grads.rw5");
        let project = &result.project;
        assert_eq!(project.units.linear, LinearUnit::Feet);
        assert_eq!(project.units.angular, AngularUnit::Gons);
        let occupied = point(project, "100").expect("100");
        // 1524 m
        double_eq(occupied.northing, 5000.0 * 0.3048);
        double_eq(occupied.elevation.expect("an elevation"), 100.0 * 0.3048);
        let station = &project.stations[0];
        double_eq(station.setup.instrument_height, 5.0 * 0.3048);
        // BS250 gon, south azimuth: 250 + 200 = 450 gon = 50 gon = pi / 4.
        let backsight =
            parse_finite_double(station.metadata["backsight azimuth (radians)"].as_bytes())
                .expect("a number");
        near(backsight, 50.0 * PI / 200.0, ANGLE_TOLERANCE);
        let direction = observations_to::<HorizontalDirectionObservation>(station, "102");
        assert_eq!(direction.len(), 1);
        // AR100 gon
        near(direction[0].direction, PI / 2.0, ANGLE_TOLERANCE);
        let distance = observations_to::<DistanceObservation>(station, "102");
        assert_eq!(distance.len(), 1);
        // 30.48 m
        double_eq(distance[0].distance, 100.0 * 0.3048);
        double_eq(distance[0].target_height, 6.0 * 0.3048);
        // TR with AZ50 gon (south) is an azimuth of 250 gon = 1.25 pi, VA10 gon
        // an elevation of pi / 20, and HD200 ft a horizontal 60.96 m.
        let azimuth = observations_to::<AzimuthObservation>(station, "103");
        assert_eq!(azimuth.len(), 1);
        near(azimuth[0].azimuth, 1.25 * PI, ANGLE_TOLERANCE);
        let vertical = observations_to::<VerticalAngleObservation>(station, "103");
        assert_eq!(vertical.len(), 1);
        near(vertical[0].angle, PI / 20.0, ANGLE_TOLERANCE);
        let flat = observations_to::<DistanceObservation>(station, "103");
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].kind, DistanceKind::Horizontal);
        double_eq(flat[0].distance, 200.0 * 0.3048);
    }

    #[test]
    fn us_survey_feet_are_converted_with_the_us_survey_foot() {
        let bytes = concat!(
            "MO,AD0,UN2,SF1.0,EC0,EO0.0,AU0\n",
            "SP,PN7,N 3937.000,E 7874.000,EL39.370\n"
        );
        let result = ok(read(RW5, bytes, "us.rw5"));
        let stored = point(&result.project, "7").expect("7");
        // 1 US survey foot = 1200/3937 m, so 3937 ft = 1200 m exactly.
        near(stored.northing, 1200.0, 1e-9);
        near(stored.easting, 2400.0, 1e-9);
        near(stored.elevation.expect("an elevation"), 12.0, 1e-9);
        assert_eq!(result.project.units.linear, LinearUnit::UsSurveyFeet);
    }

    #[test]
    fn a_file_with_no_mode_record_is_refused_at_its_first_distance_rather_than_guessed() {
        let error =
            read(RW5, "JB,NMX\nOC,OP1,N 10.0,E 20.0,EL1.0\n", "nomode.rw5").expect_err("refused");
        assert_eq!(error.code, ErrorCode::FileImportFailure);
        assert!(error.message.contains("record 2"));
        assert!(error.message.contains("feet"));
    }

    #[test]
    fn a_point_stored_twice_keeps_its_first_coordinates_and_warns_with_both() {
        let result = read_fixture("journal.rw5");
        let first = point(&result.project, "1").expect("1");
        double_eq(first.northing, 100.0);
        assert_eq!(
            first.metadata["coordinates restated at record 3"],
            "N 100.005, E 200, elevation 10"
        );
        assert!(any_warning_contains(&result, 3, "record 2"));
    }

    #[test]
    fn a_backsight_taken_again_after_shots_starts_a_new_setup_on_the_same_point() {
        let result = read_fixture("journal.rw5");
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(result.project.stations[0].setup.id, "1");
        assert_eq!(result.project.stations[1].setup.id, "1 (2)");
        assert_eq!(result.project.stations[1].setup.point_id, "1");
        // BC90.0000 on the second.
        near(
            result.project.stations[1]
                .backsight_azimuth
                .expect("a circle"),
            PI / 2.0,
            ANGLE_TOLERANCE,
        );
        assert_eq!(
            observations_to::<HorizontalDirectionObservation>(&result.project.stations[1], "4")
                .len(),
            1
        );
    }

    #[test]
    fn a_change_in_elevation_becomes_a_ground_to_ground_level_difference() {
        let result = read_fixture("journal.rw5");
        let differences =
            observations_to::<LevelDifferenceObservation>(&result.project.stations[1], "5");
        assert_eq!(differences.len(), 1);
        // CE1.250 with no LS record: HI 0 and HR 0, so 1.25 + 0 - 0.
        double_eq(differences[0].height_difference, 1.25);
        double_eq(differences[0].length, 30.0);
        assert!(any_not_carried_contains(&result, "no instrument heights"));
    }

    #[test]
    fn records_katana_does_not_import_are_warned_by_number_and_counted() {
        let result = read_fixture("journal.rw5");
        assert!(any_warning_contains(&result, 9, "SL record not imported"));
        assert!(any_warning_contains(&result, 10, "not a record type"));
        assert_eq!(result.records_skipped, 2);
        assert_eq!(result.records_read, 9);
    }

    #[test]
    fn no_field_or_note_is_dropped_without_a_word_and_off_centre_shots_are_kept() {
        let bytes = concat!(
            "MO,AD0,UN1,SF1.0,EC0,EO0.0,AU0\n",   // 1
            "OC,OP1,N 0.0,E 0.0,EL0.0\n",         // 2
            "BK,OP1,BP2,BC0.0000,--sun behind\n", // 3
            "SS,OP1,FP3,AR10.0000,AZ20.0000,ZE90.0000,SD5.000,FE12.345,QQ7\n", // 4
            "OF,AR11.0000,ZE90.0000,SD5.100,--tree centre\n", // 5
            "SS,OP1,FP4,AR30.0000,ZE90.0000,SD6.000,QQ8\n"  // 6
        );
        let result = ok(read(RW5, bytes, "fields.rw5"));
        assert_eq!(result.records_read, 6);
        assert_eq!(result.records_skipped, 0);
        let station = &result.project.stations[0];
        // A backsight has no use for a note; it is kept with the setup.
        assert_eq!(station.metadata["notes"], "BK note: sun behind");
        // AR and AZ on one shot: AR (10 00' 00") is read, AZ named, not imported.
        assert!(any_warning_contains(&result, 4, "gives AR and also AZ"));
        let direction = observations_to::<HorizontalDirectionObservation>(station, "3");
        assert_eq!(direction.len(), 1);
        near(direction[0].direction, dms(10, 0, 0.0), ANGLE_TOLERANCE);
        assert!(observations_to::<AzimuthObservation>(station, "3").is_empty());
        // FE, the collector's foresight elevation, stays with the point.
        assert_eq!(
            unpositioned(&result.project, "3").expect("3").metadata["foresight elevation (collector, m)"],
            "12.345"
        );
        // QQ is in no specification: warned where first met, then counted.
        assert!(any_warning_contains(&result, 4, "field 'QQ' (value '7')"));
        assert!(!any_warning_contains(&result, 6, "field 'QQ'"));
        assert!(any_warning_contains(
            &result,
            0,
            "'QQ' field of SS records was not imported on 2 records, the first record 4"
        ));
        // The off-centre shot is kept, as written, with the setup.
        assert_eq!(
            station.metadata["off-centre shot, record 5 (file units)"],
            "AR 11.0000, ZE 90.0000, SD 5.100, note tree centre"
        );
        assert!(any_warning_contains(&result, 5, "not applied"));
    }

    #[test]
    fn past_ten_thousand_warnings_the_rest_are_counted_in_one_warning_not_listed() {
        // Two good records, then 25,000 of a type the specification does not
        // define: lines 3 to 25,002. The first 10,000 (lines 3 to 10,002) are
        // listed; the other 15,000, from line 10,003, are one closing warning.
        let mut bytes = String::from("MO,AD0,UN1,SF1.0,EC0,EO0.0,AU0\nSP,PN1,N 1.0,E 2.0,EL3.0\n");
        for _ in 0..25000 {
            bytes.push_str("ZZ,XX1\n");
        }
        let result = ok(read(RW5, bytes, "noisy.rw5"));
        assert_eq!(result.records_read, 2);
        assert_eq!(result.records_skipped, 25000);
        assert_eq!(result.warnings.len(), 10001);
        assert_eq!(result.warnings[9999].record, 10002);
        let last = result.warnings.last().expect("a closing warning");
        assert_eq!(last.record, 0);
        assert!(
            last.message
                .contains("15000 more warnings, from record 10003 on"),
            "{}",
            last.message
        );
    }

    #[test]
    fn gnss_positions_are_geodetic_in_wgs84_from_degrees_minutes_seconds() {
        let result = read_fixture("gnss.rw5");
        let observations = &result.project.observations;
        // BP, GPS, G1
        assert_eq!(observations.len(), 3);
        let Observation::GnssGlobalPosition(base) = &observations[0] else {
            panic!("the base is a global position");
        };
        assert_eq!(base.point, "001");
        let base_position = base.geodetic.expect("geodetic");
        // LA40.364883159071 = 40 36' 48.83159071"; LN-3.420590114521 is west.
        near(
            base_position.latitude,
            dms(40, 36, 48.83159071),
            ANGLE_TOLERANCE,
        );
        near(
            base_position.longitude,
            -dms(3, 42, 5.90114521),
            ANGLE_TOLERANCE,
        );
        double_eq(base_position.ellipsoidal_height, 765.698);
        // ATARP: the height AG1.000 is to the antenna reference point.
        double_eq(base.antenna.height, 1.0);
        assert_eq!(base.antenna.method, AntennaHeightMethod::Vertical);
        assert_eq!(base.reference_frame, "WGS 84");

        let Observation::GnssGlobalPosition(rover) = &observations[1] else {
            panic!("the rover is a global position");
        };
        assert_eq!(rover.point, "701");
        let rover_position = rover.geodetic.expect("geodetic");
        near(
            rover_position.latitude,
            dms(42, 21, 46.3092),
            ANGLE_TOLERANCE,
        );
        near(
            rover_position.longitude,
            -dms(71, 8, 14.09184),
            ANGLE_TOLERANCE,
        );
        double_eq(rover_position.ellipsoidal_height, -21.8459);
        // The rod height in force (LS,HR2.000) is the phase-centre height.
        double_eq(rover.antenna.height, 2.0);
        assert_eq!(rover.antenna.method, AntennaHeightMethod::PhaseCentre);
        assert_eq!(
            unpositioned(&result.project, "701").expect("701").code,
            "CP"
        );
    }

    #[test]
    fn gnss_vector_records_become_one_geocentric_baseline_with_its_covariance() {
        let result = read_fixture("gnss.rw5");
        let Observation::GnssGeocentricBaseline(vector) = &result.project.observations[2] else {
            panic!("a vector");
        };
        assert_eq!(vector.from, "0");
        assert_eq!(vector.to, "3");
        double_eq(vector.delta.x, -3444.43582);
        double_eq(vector.delta.y, -4918.56294);
        double_eq(vector.delta.z, 3862.46787);
        double_eq(vector.covariance.xx, 0.00013303);
        double_eq(vector.covariance.yy, 0.00008997);
        double_eq(vector.covariance.zz, 0.00014449);
        double_eq(vector.covariance.xy, -0.00005549);
        double_eq(vector.covariance.xz, 0.00010642);
        double_eq(vector.covariance.yz, -0.00005627);
        // G4 BVARP: the base end is the antenna reference point, AG1.000 above
        // the mark. RVAPC: the rover end is the phase centre, LS,HR2.000 above.
        assert_eq!(vector.from_antenna.method, AntennaHeightMethod::Vertical);
        double_eq(vector.from_antenna.height, 1.0);
        assert_eq!(vector.to_antenna.method, AntennaHeightMethod::PhaseCentre);
        double_eq(vector.to_antenna.height, 2.0);
        assert_eq!(vector.source.record_number, 6);
        assert_eq!(
            unpositioned(&result.project, "3").expect("3").metadata["GNSS vector note"],
            "2013/12/06 15:44:50,(Average) - Base ID read at rover: 0"
        );
    }

    #[test]
    fn a_surv_ce_vector_is_in_metres_whatever_the_jobs_distance_unit() {
        // Carlson's own sample from "SurvCE Version 2.50 Raw File records"
        // (5/4/2010), a US-survey-feet job (MO UN2). The document: "The DX, DY
        // and DZ values are phase center to phase center. ALL THE VALUES ARE
        // ALWAYS IN METERS."
        let result = read_fixture("survce250_us_feet.rw5");
        assert_eq!(result.project.units.linear, LinearUnit::UsSurveyFeet);
        let vector = first_vector(&result).expect("a vector");
        assert_eq!(vector.from, "733");
        assert_eq!(vector.to, "BWC1+A");
        double_eq(vector.delta.x, 5692.192);
        double_eq(vector.delta.y, 6823.564);
        double_eq(vector.delta.z, 12978.073);
        // The variances are square metres too; they and the vector agree.
        double_eq(vector.covariance.xx, 0.00863666);
        // The file itself says metres: its base (BP) and rover (GPS) positions,
        // both to the phase centre, are 15,728.71 m apart through the earth -
        //   base  30 16' 08.94090052" N, 97 47' 13.43999946" W, h 175.4530 m
        //   rover 30 24' 16.17091114" N, 97 44' 16.79812958" W, h 231.637722 m
        // - and sqrt(5692.192^2 + 6823.564^2 + 12978.073^2) = 15,728.71 m, the
        // components agreeing to 0.7 mm. Read as US feet the vector is 4,794 m.
        let base = wgs84_geocentric(dms(30, 16, 8.94090052), -dms(97, 47, 13.43999946), 175.4530);
        let rover = wgs84_geocentric(
            dms(30, 24, 16.17091114),
            -dms(97, 44, 16.79812958),
            231.637722,
        );
        near(vector.delta.x, rover.x - base.x, 0.002);
        near(vector.delta.y, rover.y - base.y, 0.002);
        near(vector.delta.z, rover.z - base.z, 0.002);
    }

    #[test]
    fn a_surv_ce_vector_runs_from_phase_centre_to_phase_centre_with_the_rod_height_at_the_rover() {
        // [SCE] 2.50: "You will get the rod height of the rover from the LS
        // record prior to the vector records. The LS,HR value is from phase
        // center to the ground ... THE UNITS CAN BE FEET OR METERS"; and of the
        // BP record: "add [AG and PA] together to get the Phase Center to
        // Ground value", always in metres.
        let result = read_fixture("survce250_us_feet.rw5");
        let vector = first_vector(&result).expect("a vector");
        // LS,HR6.9344 US survey feet = 6.9344 * 1200 / 3937 = 2.1136 m.
        double_eq(vector.to_antenna.height, 6.9344 * 1200.0 / 3937.0);
        assert_eq!(vector.to_antenna.method, AntennaHeightMethod::PhaseCentre);
        // AG2.000 + PA0.114 = 2.114 m, the base's phase centre above its mark.
        double_eq(vector.from_antenna.height, 2.114);
        assert_eq!(vector.from_antenna.method, AntennaHeightMethod::PhaseCentre);
    }

    #[test]
    fn ag4_record_moves_a_vector_end_between_the_phase_centre_and_the_reference_point() {
        let bytes = concat!(
            "MO,AD0,UN1,SF1.00000000,EC0,EO0.0,AU0\n", // 1
            "BP,PN001,LA40.364883159071,LN-3.420590114521,EL765.6980,AG1.000,PA0.084,ATARP,",
            "SRROVER,--\n",                                                    // 2
            "LS,HR2.000\n",                                                    // 3
            "G0,2013/12/06 15:44:50,(Average) - Base ID read at rover: 001\n", // 4
            "G1,BP001,PN3,DX-3444.43582,DY-4918.56294,DZ3862.46787\n",         // 5
            "G4,BVAPC,RVAPC\n",                                                // 6
            "G0,2013/12/06 15:49:50,(Average) - Base ID read at rover: 001\n", // 7
            "G1,BP001,PN4,DX-3440.00000,DY-4910.00000,DZ3860.00000\n",         // 8
            "G4,BVARP,RVARP\n"                                                 // 9
        );
        let result = ok(read(RW5, bytes, "ends.rw5"));
        let vectors = vectors(&result);
        assert_eq!(vectors.len(), 2);
        // BVAPC on a base whose AG is to the reference point (ATARP): the phase
        // centre is AG1.000 + PA0.084 = 1.084 m above the mark, not 1.000.
        double_eq(vectors[0].from_antenna.height, 1.084);
        assert_eq!(
            vectors[0].from_antenna.method,
            AntennaHeightMethod::PhaseCentre
        );
        double_eq(vectors[0].to_antenna.height, 2.0);
        assert_eq!(
            vectors[0].to_antenna.method,
            AntennaHeightMethod::PhaseCentre
        );
        // BVARP: AG itself, 1.000 m.
        double_eq(vectors[1].from_antenna.height, 1.0);
        assert_eq!(
            vectors[1].from_antenna.method,
            AntennaHeightMethod::Vertical
        );
        // RVARP: the vector ends at the rover's reference point but HR is to its
        // phase centre ([SCE] 3.03: "GPS heights always to be recorded to phase
        // center"), and no record gives the offset between them: the height is
        // not passed off as either, and the record says so.
        double_eq(vectors[1].to_antenna.height, 2.0);
        assert_eq!(vectors[1].to_antenna.method, AntennaHeightMethod::Other);
        assert!(any_warning_contains(&result, 9, "phase centre"));
    }

    #[test]
    fn a_baseline_note_ends_where_its_solution_class_and_precisions_begin() {
        // [TDS] record 30, the one record whose note is not its last field:
        // "BL,DC%s,PN%s,DX%s,DY%s,DZ%s,--%s,GM%s,CL%s,HP%s,VP%s".
        let bytes = concat!(
            "MO,AD0,UN1,SF1.00000000,EC0,EO0.0,AU0\n", // 1
            "BP,PN1,LA40.364883159071,LN-3.420590114521,HT765.698,SG0\n", // 2
            "BL,DC2,PN2,DX10.000,DY20.000,DZ30.000,--CP fence, north,GM4,CL1,HP0.012,VP0.020\n", // 3
            "BL,DC2,PN3,DX11.000,DY21.000,DZ31.000,--CP,GM3,CL2,HP0.015,VP0.025\n"               // 4
        );
        let result = ok(read(RW5, bytes, "baselines.rw5"));
        let vectors = vectors(&result);
        assert_eq!(vectors.len(), 2);
        // [TDS] GM: 4 RTKFixed, 3 RTKFloat.
        assert_eq!(vectors[0].solution, GnssSolution::Fixed);
        assert_eq!(vectors[1].solution, GnssSolution::Float);
        let two = unpositioned(&result.project, "2").expect("2");
        let three = unpositioned(&result.project, "3").expect("3");
        // The note keeps its own comma; the fields after it are fields.
        assert_eq!(two.code, "CP");
        assert_eq!(two.description, "CP fence, north");
        assert_eq!(three.code, "CP");
        assert_eq!(two.metadata["horizontal precision (m)"], "0.012");
        assert_eq!(two.metadata["vertical precision (m)"], "0.02");
        assert_eq!(three.metadata["horizontal precision (m)"], "0.015");
        // [TDS] CL: 1 Normal, 2 Control; DC 2 ModeRover.
        assert_eq!(two.metadata["classification"], "normal");
        assert_eq!(three.metadata["classification"], "control");
        assert_eq!(two.metadata["GNSS derivation"], "rover");
        // Both points string into one CP feature.
        let control = feature(&result.project, "CP").expect("CP");
        assert_eq!(control.point_ids, ["2", "3"]);
        assert_eq!(result.project.features.len(), 1);
        for warning in &result.warnings {
            assert!(
                !warning.message.contains("not imported"),
                "{}",
                warning.message
            );
        }
    }

    #[test]
    fn every_value_of_every_off_centre_shot_is_kept_with_its_setup() {
        // [TDS] record 16 writes "OF,OL%s,--Right Angle Offset", "OF,HD%s,--
        // Horizontal Distance Offset", "OF,LR%s,--Left / Right Offset" and
        // "OF,VD%s,--Elevation Offset" beside [SCE]'s AR, ZE and SD.
        let bytes = concat!(
            "MO,AD0,UN1,SF1.0,EC0,EO0.0,AU0\n",          // 1
            "OC,OP1,N 0.0,E 0.0,EL0.0\n",                // 2
            "SS,OP1,FP9,AR10.0000,ZE90.0000,SD5.000\n",  // 3
            "OF,OL0.500,--Right Angle Offset\n",         // 4
            "SS,OP1,FP8,AR20.0000,ZE90.0000,SD6.000\n",  // 5
            "OF,OL0.750,--Right Angle Offset\n",         // 6
            "OF,HD1.250,--Horizontal Distance Offset\n", // 7
            "OF,LR-0.300,--Left / Right Offset\n",       // 8
            "OF,VD0.100,--Elevation Offset\n"            // 9
        );
        let result = ok(read(RW5, bytes, "offsets.rw5"));
        let metadata = &result.project.stations[0].metadata;
        assert_eq!(
            metadata["off-centre shot, record 4 (file units)"],
            "OL 0.500, note Right Angle Offset"
        );
        assert_eq!(
            metadata["off-centre shot, record 6 (file units)"],
            "OL 0.750, note Right Angle Offset"
        );
        assert_eq!(
            metadata["off-centre shot, record 7 (file units)"],
            "HD 1.250, note Horizontal Distance Offset"
        );
        assert_eq!(
            metadata["off-centre shot, record 8 (file units)"],
            "LR -0.300, note Left / Right Offset"
        );
        assert_eq!(
            metadata["off-centre shot, record 9 (file units)"],
            "VD 0.100, note Elevation Offset"
        );
        for warning in &result.warnings {
            assert!(
                !warning.message.contains("specifications define"),
                "{}",
                warning.message
            );
        }
    }

    #[test]
    fn every_field_the_specifications_define_is_read_not_warned_about_as_unknown() {
        // One record of every type the reader imports, with every field [TDS]
        // 3.6 and [SCE] 3.03 give that type (a shot's alternatives on separate
        // shots, as the specifications allow one of each group).
        let bytes = concat!(
            "JB,NMALL,DT09-24-2026,TM10:00:00\n",
            "MO,AD0,UN1,SF1.00000000,EC0,EO0.0,AU0\n",
            "CS,CO4,ZGUTM,ZNZone 56,DNWGS84\n",
            "OC,OP1,N 1000.0,E 2000.0,EL50.0,--CP\n",
            "BK,OP1,BP2,BS0.0000,BC0.0000\n",
            "LS,HI1.500,HR1.800\n",
            "TR,OP1,FP3,AR10.0000,ZE90.0000,SD5.000,--A\n",
            "SS,OP1,FP4,AZ20.0000,VA1.0000,SD6.000,--A\n",
            "OB,OP1,FP5,AL30.0000,CE0.500,HD7.000,--A\n",
            "SS,OP1,FP6,BRN40.0000E,ZE90.0000,SD8.000,FE51.000,--A\n",
            "SS,OP1,FP7,DR5.0000,ZE90.0000,SD9.000,--A\n",
            "SS,OP1,FP8,DL5.0000,ZE90.0000,SD9.500,--A\n",
            "BD,OP1,FP2,AR0.0000,ZE90.0000,SD10.000,--A\n",
            "BR,OP1,FP2,AR180.0000,ZE270.0000,SD10.000,--A\n",
            "FD,OP1,FP3,AR10.0000,ZE90.0000,SD5.000,--A\n",
            "FR,OP1,FP3,AR190.0000,ZE270.0000,SD5.000,--A\n",
            "SK,OP1,FP9,AR50.0000,ZE90.0000,SD11.000,--A\n",
            "RB,OP1,BP2,AR0.0000,ZE90.0000,SD10.000,HR1.800,--A\n",
            "RF,OP1,FP3,AR10.0000,ZE90.0000,SD5.000,HR1.800,--A\n",
            "OF,AR11.0000,ZE90.0000,SD5.100\n",
            "OF,OL0.500,--Right Angle Offset\n",
            "OF,HD1.250,--Horizontal Distance Offset\n",
            "OF,LR-0.300,--Left / Right Offset\n",
            "OF,VD0.100,--Elevation Offset\n",
            "SP,PN20,N 1010.0,E 2010.0,EL51.0,--B\n",
            "AP,PN21,N 1011.0,E 2011.0,EL51.1,--B\n",
            "GS,PN22,N 1012.0,E 2012.0,EL51.2,--B\n",
            "GR,PN23,N 1013.0,E 2013.0,EL51.3,--B\n",
            "FC,PN20,FNKB\n",
            "AT,TNcolour,TVred\n",
            "DP,PN21\n",
            "SP,PN24,N 1014.0,E 2014.0,EL51.4\n",
            "EP,TM12:30:45,LA40.364883159071,LN-3.420590114521,HT765.698,RH0.010,RV0.020,DH0.9,",
            "DV1.4,GM4,CL1\n",
            "AH,DC2,MA2.000,ME1,RA2.084\n",
            "EQ,DC2,RXR10,RS123,AN5,AI1,ATTRMR10,TS456,TA0.000,HO0.000,VO0.000\n",
            "BP,PN30,LA40.364883159071,LN-3.420590114521,HT765.698,SG0\n",
            "BL,DC2,PN31,DX10.000,DY20.000,DZ30.000,--CP,GM4,CL1,HP0.012,VP0.020\n",
            "CV,DC2,SV8,SC1.0,XX0.0001,XY0.00001,XZ0.00001,YY0.0001,YZ0.00001,ZZ0.0002\n",
            "BP,PN40,LA40.364883159071,LN-3.420590114521,EL765.6980,AG1.000,PA0.084,ATARP,",
            "SRROVER,--BASE\n",
            "GPS,PN41,LA42.214630920,LN-71.081409184,EL-21.8459,--CP\n",
            "G0,2013/12/06 15:44:50,(Average) - Base ID read at rover: 40\n",
            "G1,BP40,PN42,DX-3444.43582,DY-4918.56294,DZ3862.46787\n",
            "G2,VX0.00013303,VY0.00008997,VZ0.00014449\n",
            "G3,XY-0.00005549,XZ0.00010642,YZ-0.00005627\n",
            "G4,BVARP,RVAPC\n"
        );
        let result = ok(read(RW5, bytes, "every_field.rw5"));
        assert_eq!(result.records_skipped, 0);
        for warning in &result.warnings {
            assert!(
                !warning.message.contains("field '"),
                "record {}: {}",
                warning.record,
                warning.message
            );
            assert!(
                !warning.message.contains("was not imported on"),
                "{}",
                warning.message
            );
        }
        // Where the fields no import needs went.
        let project = &result.project;
        assert_eq!(
            project.metadata["coordinate system option"],
            "chosen from library"
        );
        assert_eq!(project.metadata["rover antenna tape adjustment (m)"], "0");
        assert_eq!(project.metadata["rover antenna number"], "5");
        assert_eq!(
            point(project, "24").expect("24").metadata["GNSS time"],
            "12:30:45"
        );
        assert_eq!(
            point(project, "24").expect("24").metadata["classification"],
            "normal"
        );
        assert_eq!(
            unpositioned(project, "30").expect("30").metadata["setup group"],
            "0"
        );
        assert_eq!(
            unpositioned(project, "31").expect("31").metadata["minimum satellites"],
            "8"
        );
    }

    #[test]
    fn detection_identifies_each_rw5_fixture_and_no_other_format_claims_them() {
        for name in [
            "setup_metres.rw5",
            "feet_grads.rw5",
            "gnss.rw5",
            "journal.rw5",
        ] {
            let bytes = fixture(format!("rw5/{name}"));
            let detection = detect_format(&probe_of(&bytes, name, false));
            assert_eq!(
                detection.outcome(),
                DetectionOutcome::Identified,
                "{name}: {}",
                detection.summary()
            );
            assert_eq!(detection.format().expect("identified").id, RW5, "{name}");
        }
        // Without its extension an RW5 file is still recognised by its records.
        let bytes = fixture("rw5/setup_metres.rw5");
        let renamed = detect_format(&probe_of(&bytes, "export.txt", false));
        assert_eq!(
            renamed.outcome(),
            DetectionOutcome::Identified,
            "{}",
            renamed.summary()
        );
        assert_eq!(renamed.format().expect("identified").id, RW5);
    }

    #[test]
    fn the_probe_rules_out_coordinate_files_and_gts_raw_data() {
        let registry = format_registry();
        for name in [
            "survey_points_pnezd.csv",
            "survey_points_headerless.txt",
            "survey_points_headerless_penz.txt",
            "survey_points_pnezd_utm30s.csv",
            "gts/setup.gt7",
        ] {
            let bytes = fixture(name);
            for probed in registry.probe_all(&probe_of(&bytes, name, false)) {
                if probed.format_id == RW5 {
                    assert!(
                        probed.signature.confidence < 0.5,
                        "{name}: {}",
                        probed.signature.evidence
                    );
                }
            }
        }
    }

    #[test]
    fn the_probe_does_not_claim_leica_trimble_rinex_or_land_xml_files() {
        for sample in &FOREIGN_SAMPLES {
            let input = probe_of(sample.bytes.as_bytes(), sample.name, false);
            for probed in format_registry().probe_all(&input) {
                if probed.format_id == RW5 {
                    assert!(
                        probed.signature.confidence < 0.5,
                        "{}: {}",
                        sample.name,
                        probed.signature.evidence
                    );
                }
            }
            let detection = detect_format(&input);
            if let Ok(format) = detection.format() {
                assert_ne!(format.id, RW5, "{}", sample.name);
            }
        }
    }

    #[test]
    fn every_truncation_and_random_bytes_give_an_error_or_a_result_never_a_crash() {
        let mut reads = 0;
        for name in [
            "setup_metres.rw5",
            "feet_grads.rw5",
            "gnss.rw5",
            "journal.rw5",
        ] {
            reads += read_every_truncation_and_noise(RW5, &fixture(format!("rw5/{name}")), name);
        }
        assert!(reads > 3000);
    }

    // A synthetic job: setups of radial shots. Sized by the environment variable
    // KATANA_SURVEYIO_THROUGHPUT_MB (a small default keeps the suite quick); the
    // rate is printed for the record, and the counts checked.
    #[test]
    fn a_read_of_a_large_synthetic_job_reads_every_record_and_reports_its_rate() {
        let mut megabytes = 2usize;
        if let Ok(wanted) = std::env::var("KATANA_SURVEYIO_THROUGHPUT_MB") {
            // strtoul: the leading digits, 0 for none.
            let digits: String = wanted.chars().take_while(char::is_ascii_digit).collect();
            megabytes = digits.parse().unwrap_or(0);
        }
        let mut bytes = String::from(
            "JB,NMTHROUGHPUT,DT09-24-2026,TM10:00:00\nMO,AD0,UN1,SF1.00000000,EC0,EO0.0,AU0\n",
        );
        let mut setups = 0usize;
        let mut shots = 0usize;
        while bytes.len() < megabytes * 1024 * 1024 {
            setups += 1;
            let station = format!("S{setups}");
            bytes.push_str(&format!(
                "OC,OP{station},N 5000.0000,E 5000.0000,EL100.000,--CP\n"
            ));
            bytes.push_str(&format!("BK,OP{station},BPB{setups},BS0.0000,BC0.0000\n"));
            bytes.push_str("LS,HI1.550,HR1.800\n");
            for shot in 0..100 {
                shots += 1;
                bytes.push_str(&format!(
                    "SS,OP{station},FP{shots},AR{}.1530,ZE91.1015,SD{}.2345,--KB{}\n",
                    shot * 3,
                    10 + shot,
                    shot % 7
                ));
            }
        }
        // The reader on its own, then the whole door (read_survey adds the size
        // caps and validate_project, which is not this reader's cost).
        let reader = format_registry().reader(RW5).expect("a reader");
        let reader_seconds = {
            let start = Instant::now();
            let alone = reader(bytes.as_bytes(), "throughput.rw5", &ReadOptions::default());
            let seconds = start.elapsed().as_secs_f64();
            assert!(alone.is_ok());
            seconds
        }; // freed here, outside both timings
        let start = Instant::now();
        let result = ok(read(RW5, &bytes, "throughput.rw5"));
        let seconds = start.elapsed().as_secs_f64();
        assert_eq!(result.records_read, 2 + setups * 3 + shots);
        assert_eq!(result.records_skipped, 0);
        assert_eq!(result.project.stations.len(), setups);
        let mb = bytes.len() as f64 / (1024.0 * 1024.0);
        println!(
            "RW5 throughput: {mb} MB, {shots} shots; reader alone {reader_seconds} s, {} MB/s; \
             read_survey (with validate_project) {seconds} s, {} MB/s",
            mb / reader_seconds,
            mb / seconds
        );
    }
}
