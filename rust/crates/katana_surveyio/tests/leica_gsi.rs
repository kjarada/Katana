//! Twin of tests/surveyio/test_leica_gsi.cpp: Leica GSI-8 / GSI-16
//! (src/leica_gsi.rs).
//!
//! The fixtures in data/leica/ were built by hand from Leica's "GSI ONLINE for
//! Leica TPS and DNA" (November 2003): every expected value below is worked
//! from the word as written in the fixture and the unit digit's rule (pages
//! 5-6), in the comment beside it - never from what the reader returned.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "The expected angles are the C++ test's literals, digit for digit."
)]

mod instrument_support;

use std::path::PathBuf;

use katana_core::{ErrorCode, Result};
use katana_survey::{
    AngularUnit, CoordinateSource, CorrectionState, DistanceKind, DistanceObservation, Face,
    HorizontalDirectionObservation, LevelDifferenceObservation, LinearUnit, Observation,
    ObservationPrecision, SurveyPoint, SurveyProject, SurveyStation, UnpositionedPoint,
    ZenithAngleObservation,
};
use katana_surveyio::{
    DetectionOutcome, LEICA_GSI_FORMAT_ID, Manufacturer, PROBE_BYTES, ReadOptions, ReadResult,
    ReadWarning, describe, detect_format, format_registry, probe_of, read_survey,
};

use instrument_support::{Random32, atoi, data_folder, double_eq, files_under, near, slurp};

const PI: f64 = std::f64::consts::PI;
const GON: f64 = PI / 200.0;
const DEGREE: f64 = PI / 180.0;
// The fixtures' millimetres come through a division by 1000, so they are the
// nearest double to the decimal - double_eq is the right comparison. Angles
// pass through a product with pi, a few units in the last place.
const ANGLE_TOLERANCE: f64 = 1e-14;

fn leica_folder() -> PathBuf {
    data_folder().join("leica")
}

fn read_text_with(bytes: &[u8], name: &str, options: &ReadOptions) -> Result<ReadResult> {
    read_survey(format_registry(), LEICA_GSI_FORMAT_ID, bytes, name, options)
}

fn read_text(bytes: &[u8]) -> Result<ReadResult> {
    read_text_with(bytes, "job.gsi", &ReadOptions::default())
}

fn read_ok(text: &str) -> ReadResult {
    read_text(text.as_bytes()).unwrap_or_else(|error| panic!("{}", error.message))
}

fn read_fixture(name: &str) -> ReadResult {
    let bytes = slurp(leica_folder().join(name));
    assert!(!bytes.is_empty(), "fixture {name} is missing");
    read_text_with(&bytes, name, &ReadOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.message))
}

fn point<'a>(project: &'a SurveyProject, id: &str) -> Option<&'a SurveyPoint> {
    project.points.iter().find(|p| p.id == id)
}

fn unpositioned<'a>(project: &'a SurveyProject, id: &str) -> Option<&'a UnpositionedPoint> {
    project.unpositioned_points.iter().find(|p| p.id == id)
}

fn directions<'a>(
    station: &'a SurveyStation,
    target: &str,
) -> Vec<&'a HorizontalDirectionObservation> {
    station
        .observations
        .iter()
        .filter_map(|observation| match observation {
            Observation::HorizontalDirection(o) if o.to == target => Some(o),
            _ => None,
        })
        .collect()
}

fn all_zeniths(station: &SurveyStation) -> Vec<&ZenithAngleObservation> {
    station
        .observations
        .iter()
        .filter_map(|observation| match observation {
            Observation::ZenithAngle(o) => Some(o),
            _ => None,
        })
        .collect()
}

fn zeniths<'a>(station: &'a SurveyStation, target: &str) -> Vec<&'a ZenithAngleObservation> {
    all_zeniths(station)
        .into_iter()
        .filter(|o| o.to == target)
        .collect()
}

fn distances<'a>(station: &'a SurveyStation, target: &str) -> Vec<&'a DistanceObservation> {
    station
        .observations
        .iter()
        .filter_map(|observation| match observation {
            Observation::Distance(o) if o.to == target => Some(o),
            _ => None,
        })
        .collect()
}

fn levels<'a>(station: &'a SurveyStation, target: &str) -> Vec<&'a LevelDifferenceObservation> {
    station
        .observations
        .iter()
        .filter_map(|observation| match observation {
            Observation::LevelDifference(o) if o.to == target => Some(o),
            _ => None,
        })
        .collect()
}

// The warning about `record` whose message contains `text`, or None.
fn warning_about<'a>(result: &'a ReadResult, record: usize, text: &str) -> Option<&'a ReadWarning> {
    result
        .warnings
        .iter()
        .find(|warning| warning.record == record && warning.message.contains(text))
}

fn says(lines: &[String], text: &str) -> bool {
    lines.iter().any(|line| line.contains(text))
}

fn all_warnings(result: &ReadResult) -> String {
    let mut text = String::new();
    for warning in &result.warnings {
        text.push_str(&describe(warning));
        text.push('\n');
    }
    text
}

fn lacking(result: &ReadResult) -> String {
    let mut text = String::new();
    for line in &result.not_carried {
        text.push_str(line);
        text.push('\n');
    }
    text
}

mod leica_gsi {
    use super::*;

    // ---- The format ------------------------------------------------------------------

    #[test]
    fn the_format_is_registered_with_a_reader_and_says_what_it_can_read() {
        let format = format_registry()
            .find(LEICA_GSI_FORMAT_ID)
            .expect("the GSI format is registered");
        assert_eq!(format.manufacturer, Manufacturer::Leica);
        // 1.1: 60-second angle words, all-zero code words and backsights given
        // coordinates later in the file read differently from 1.0. 1.2:
        // negative vertical readings, sexagesimal words with no digit, and the
        // 32 and 33 of a shot whose zenith angle is not read all import
        // differently from 1.1, which was pushed without them.
        assert_eq!(format.parser_version, "1.2");
        assert_eq!(format.extensions, ["gsi"]);
        assert!(format.reads.points);
        assert!(format.reads.observations);
        assert!(format.reads.stations);
        assert!(format.reads.features);
        assert!(format.reads.instrument_settings);
        assert!(!format.reads.coordinate_system); // GSI declares none
        assert!(!format.reads.gnss);
        assert!(format_registry().reader(LEICA_GSI_FORMAT_ID).is_some());
    }

    // ---- A GSI-8 job with two setups (data/leica/tps_gsi8.gsi) ------------------------

    #[test]
    fn a_station_block_followed_by_measurements_is_one_setup_with_its_instrument_height() {
        let read = read_fixture("tps_gsi8.gsi");
        let project = &read.project;
        assert_eq!(project.stations.len(), 2);
        // Record 1: "88..10+00001550" - unit 0, last digit millimetres: 1.550 m.
        assert_eq!(project.stations[0].setup.id, "STN1");
        assert_eq!(project.stations[0].setup.point_id, "STN1");
        double_eq(project.stations[0].setup.instrument_height, 1.550);
        assert_eq!(project.stations[0].source.record_number, 1);
        // Record 9: "88..10+00001600" = 1.600 m.
        assert_eq!(project.stations[1].setup.point_id, "STN2");
        double_eq(project.stations[1].setup.instrument_height, 1.600);
        // GSI marks no backsight. Setup 1's first shot (record 2) is to BS, which
        // the file has given no coordinates: no backsight is named. Setup 2's
        // first shot (record 10) is to STN1, positioned by record 1: that is its
        // backsight. No circle setting is recorded for either.
        assert!(project.stations[0].backsight_point_id.is_empty());
        assert_eq!(project.stations[1].backsight_point_id, "STN1");
        assert!(project.stations[0].backsight_azimuth.is_none());
        assert!(project.stations[1].backsight_azimuth.is_none());
        assert!(says(
            &read.not_carried,
            "1 of 2 setups took the point of their first shot as the backsight"
        ));
        assert_eq!(read.records_read, 12);
        assert_eq!(read.records_skipped, 0);
    }

    #[test]
    fn a_stations_coordinates_are_keyed_in_when_its_words_say_so() {
        let read = read_fixture("tps_gsi8.gsi");
        let station = point(&read.project, "STN1").expect("STN1");
        // "84..10+00100000": input mode 1 (keyed in), unit 0: 100000 mm = 100.000 m.
        double_eq(station.easting, 100.0);
        double_eq(station.northing, 200.0);
        double_eq(station.elevation.expect("a height"), 50.0);
        assert_eq!(station.coordinate_source, CoordinateSource::Entered);
        assert_eq!(read.project.units.linear, LinearUnit::Metres);
        assert_eq!(read.project.units.angular, AngularUnit::Gons);
    }

    #[test]
    fn both_faces_of_one_target_are_two_pointings_and_face_right_zenith_is_the_face_left_equivalent()
     {
        let read = read_fixture("tps_gsi8.gsi");
        let setup = &read.project.stations[0];
        let directions = directions(setup, "BS");
        let zeniths = zeniths(setup, "BS");
        let distances = distances(setup, "BS");
        assert_eq!(directions.len(), 2);
        assert_eq!(zeniths.len(), 2);
        assert_eq!(distances.len(), 2);

        // Record 2: Hz "21.102+00000000" = 0 gon; V "22.102+10000000" = 100.00000
        // gon, under 200 gon: face left, zenith 100 gon = pi/2.
        assert_eq!(directions[0].pointing.face, Face::Left);
        near(directions[0].direction, 0.0, ANGLE_TOLERANCE);
        near(zeniths[0].angle, 100.0 * GON, ANGLE_TOLERANCE);
        // Record 3: Hz 200 gon = pi, the raw circle reading; V 300 gon is over
        // 200 gon: face right, and the zenith angle is 400 - 300 = 100 gon.
        assert_eq!(directions[1].pointing.face, Face::Right);
        near(directions[1].direction, 200.0 * GON, ANGLE_TOLERANCE);
        near(zeniths[1].angle, 100.0 * GON, ANGLE_TOLERANCE);
        assert_eq!(zeniths[1].pointing.face, Face::Right);
        // The three values of one block share one pointing; the two blocks differ.
        assert_eq!(directions[0].pointing, zeniths[0].pointing);
        assert_eq!(directions[0].pointing, distances[0].pointing);
        assert_eq!(directions[1].pointing, distances[1].pointing);
        assert_ne!(directions[0].pointing.index, 0);
        assert_ne!(directions[0].pointing.index, directions[1].pointing.index);
        // "31..00+00100000" = 100.000 m; "31..00+00100002" = 100.002 m.
        assert_eq!(distances[0].kind, DistanceKind::Slope);
        double_eq(distances[0].distance, 100.000);
        double_eq(distances[1].distance, 100.002);
    }

    #[test]
    fn an_observation_with_no_stated_precision_takes_the_default_ones() {
        let read = read_fixture("tps_gsi8.gsi");
        let setup = &read.project.stations[0];
        let defaults = ObservationPrecision::default();
        let directions = directions(setup, "BS");
        let distances = distances(setup, "BS");
        assert!(!directions.is_empty());
        assert!(!distances.is_empty());
        double_eq(directions[0].sigma, defaults.direction);
        // 2 mm + 2 ppm over 100 m: sqrt(0.002^2 + (2e-6 * 100)^2) = 0.00201 m.
        near(
            distances[0].sigma,
            (0.002 * 0.002 + 0.0002 * 0.0002_f64).sqrt(),
            1e-15,
        );
    }

    #[test]
    fn heights_go_on_every_shot_and_a_reflector_height_holds_until_the_next_one() {
        let read = read_fixture("tps_gsi8.gsi");
        let first = &read.project.stations[0];
        let second = &read.project.stations[1];
        // Record 2 sets hr "87..10+00001700" = 1.700 m; record 5 (point 101)
        // states none, so it is still 1.700.
        let to101 = distances(first, "101");
        assert_eq!(to101.len(), 1);
        double_eq(to101[0].target_height, 1.700);
        double_eq(to101[0].instrument_height, 1.550);
        let zenith101 = zeniths(first, "101");
        assert_eq!(zenith101.len(), 1);
        double_eq(zenith101[0].target_height, 1.700);
        double_eq(zenith101[0].instrument_height, 1.550);
        // Record 10 sets hr 1.500 at the second setup (hi 1.600).
        let to_stn1 = distances(second, "STN1");
        assert_eq!(to_stn1.len(), 1);
        double_eq(to_stn1[0].target_height, 1.500);
        double_eq(to_stn1[0].instrument_height, 1.600);
        // "31..00+00070711" = 70.711 m.
        double_eq(to_stn1[0].distance, 70.711);
    }

    #[test]
    fn word51s_ppm_and_prism_constant_are_the_setups_settings_and_were_applied() {
        let read = read_fixture("tps_gsi8.gsi");
        let instrument = &read.project.stations[0].instrument;
        // Record 2: "51..1.+0012-034" - each value signed: +12 ppm, -34 mm.
        double_eq(instrument.atmospheric_ppm.expect("a ppm"), 12.0);
        assert_eq!(instrument.atmospheric_ppm_state, CorrectionState::Applied);
        double_eq(instrument.prism_constant.expect("a prism constant"), -0.034);
        assert_eq!(instrument.prism_constant_state, CorrectionState::Applied);
        // The shot records the prism constant it was measured with.
        let to_bs = distances(&read.project.stations[0], "BS");
        assert!(!to_bs.is_empty());
        double_eq(
            to_bs[0].target.prism_constant.expect("a prism constant"),
            -0.034,
        );
        assert_eq!(
            to_bs[0].target.prism_constant_state,
            CorrectionState::Applied
        );
    }

    #[test]
    fn a_setups_own_ppm_and_prism_words_replace_those_carried_from_the_setup_before() {
        let read = read_fixture("tps_gsi8.gsi");
        let instrument = &read.project.stations[1].instrument;
        // Record 10: "59..16+00100000" - unit 6, four decimals: 100000 / 10^4 = 10
        // ppm (GET 59's own example is "59..16+02200000" = 220 ppm);
        // "58..16+00000000" = 0 mm. Record 2's 12 ppm / -34 mm carried until then.
        double_eq(instrument.atmospheric_ppm.expect("a ppm"), 10.0);
        double_eq(instrument.prism_constant.expect("a prism constant"), 0.0);
        // A replacement, not a change within the setup: no warning about it.
        assert!(
            warning_about(&read, 10, "ppm changes").is_none(),
            "{}",
            all_warnings(&read)
        );
    }

    #[test]
    fn a_code_block_codes_the_point_after_it_and_word71_codes_its_own_blocks_point() {
        let read = read_fixture("tps_gsi8.gsi");
        // Record 4: "410004+0000KERB 42....+00000001" before record 5 (point 101).
        let p101 = unpositioned(&read.project, "101").expect("101");
        assert_eq!(p101.code, "KERB");
        assert_eq!(
            p101.metadata
                .get("code block information 1")
                .map(String::as_str),
            Some("1")
        );
        // Record 6: "71....+0000KERB 72....+000000A2" - remark 1 is the code.
        let p102 = unpositioned(&read.project, "102").expect("102");
        assert_eq!(p102.code, "KERB");
        assert_eq!(
            p102.metadata.get("remark 2").map(String::as_str),
            Some("A2")
        );
        assert_eq!(
            read.project.metadata["code blocks belong to"],
            "the point after them"
        );
    }

    #[test]
    fn consecutive_points_with_one_code_are_one_feature_and_an_uncoded_point_ends_it() {
        let read = read_fixture("tps_gsi8.gsi");
        // 101 and 102 are KERB, 103 (record 7) has no code.
        assert_eq!(read.project.features.len(), 1);
        let kerb = &read.project.features[0];
        assert_eq!(kerb.code, "KERB");
        assert_eq!(kerb.point_ids, ["101", "102"]);
        assert_eq!(kerb.source.record_number, 5);
    }

    #[test]
    fn a_horizontal_distance_and_height_difference_with_no_angles_give_a_distance_and_a_level_difference()
     {
        let read = read_fixture("tps_gsi8.gsi");
        let setup = &read.project.stations[0];
        // Record 7: "32..00+00027000" = 27.000 m, "33..00-00000250" = -0.250 m,
        // and "87..10+00000000" sets the reflector height to 0.
        let distances = distances(setup, "103");
        assert_eq!(distances.len(), 1);
        assert_eq!(distances[0].kind, DistanceKind::Horizontal);
        double_eq(distances[0].distance, 27.0);
        double_eq(distances[0].target_height, 0.0);
        let levels = levels(setup, "103");
        assert_eq!(levels.len(), 1);
        assert_eq!(levels[0].from, "STN1");
        double_eq(levels[0].height_difference, -0.250);
        double_eq(levels[0].length, 27.0);
        assert!(levels[0].sigma > 0.0);
        // Hz "21.102+07000000" = 70 gon.
        let directions = directions(setup, "103");
        assert_eq!(directions.len(), 1);
        near(directions[0].direction, 70.0 * GON, ANGLE_TOLERANCE);
        assert_eq!(directions[0].pointing.face, Face::Unknown); // no V, no face
    }

    #[test]
    fn a_targets_coordinate_words_are_measured_coordinates() {
        let read = read_fixture("tps_gsi8.gsi");
        // Record 8: "81..00+00100500" - input mode 0 (measured), unit 0: 100.500 m.
        let p104 = point(&read.project, "104").expect("104");
        double_eq(p104.easting, 100.5);
        double_eq(p104.northing, 200.5);
        double_eq(p104.elevation.expect("a height"), 50.1);
        assert_eq!(p104.coordinate_source, CoordinateSource::FieldObserved);
        assert_eq!(p104.source.record_number, 8);
        assert_eq!(p104.source.format_version, "GSI-8");
        assert_eq!(p104.source.file_name, "tps_gsi8.gsi");
        // Targets named with no coordinates are unpositioned, never placed at 0,0.
        assert!(unpositioned(&read.project, "BS").is_some());
        assert!(point(&read.project, "BS").is_none());
    }

    #[test]
    fn a_zero_distance_is_no_distance_and_an_unread_word_is_said_once_for_the_file() {
        let read = read_fixture("tps_gsi8.gsi");
        // Record 11: "31..00+00000000" - Leica's "no distance".
        assert!(distances(&read.project.stations[1], "105").is_empty());
        assert_eq!(zeniths(&read.project.stations[1], "105").len(), 1);
        assert!(
            warning_about(&read, 11, "distance of zero").is_some(),
            "{}",
            all_warnings(&read)
        );
        // Record 12: word 25 (horizontal circle difference) is not read, and says so.
        assert!(
            warning_about(&read, 12, "word 25").is_some(),
            "{}",
            all_warnings(&read)
        );
    }

    #[test]
    fn the_import_says_what_the_file_does_not_carry() {
        let read = read_fixture("tps_gsi8.gsi");
        assert!(says(&read.not_carried, "no coordinate system"));
        assert!(says(&read.not_carried, "backsight"));
        assert!(says(&read.not_carried, "zenith angle"));
        // GSI ONLINE's SET/CONF 171 makes the horizontal circle's direction a
        // setting no word records; the fixture's word 21s were read clockwise.
        assert!(says(
            &read.not_carried,
            "direction of the horizontal circle"
        ));
        assert!(says(
            &read.not_carried,
            "word 21 was read as increasing clockwise"
        ));
        // Record 4 is a code block and the file ends with a point block (record
        // 12): read Before Point, and the import says it is the instrument's call.
        assert!(says(&read.not_carried, "<Rec Free Code:>"));
        assert!(says(
            &read.not_carried,
            "each code block was taken to code the point after it"
        ));
        // Both setups have an instrument height and a ppm: nothing said about them.
        assert!(!says(&read.not_carried, "no instrument height"));
        assert!(!says(&read.not_carried, "no atmospheric ppm"));
    }

    // ---- GSI-16 (data/leica/tps_gsi16.gsi) -----------------------------------------------

    #[test]
    fn gsi16_reads_tenths_and_hundredths_of_a_millimetre() {
        let read = read_fixture("tps_gsi16.gsi");
        let station = point(&read.project, "STN9").expect("STN9");
        // "84..16+0000000050000000": unit 6, last digit 1/10 mm: 50000000 / 10^4 =
        // 5000.0000 m; "86..16+0000000001234560" = 123.4560 m.
        double_eq(station.easting, 5000.0);
        double_eq(station.northing, 6000.0);
        double_eq(station.elevation.expect("a height"), 123.456);
        assert_eq!(read.project.stations.len(), 1);
        double_eq(read.project.stations[0].setup.instrument_height, 1.5);
        assert_eq!(read.project.source.format_version, "GSI-16");
        let p200 = distances(&read.project.stations[0], "P200");
        assert_eq!(p200.len(), 1);
        // "31..16+0000000000123456" = 12.3456 m; "87..16+...20000" = 2.0000 m.
        double_eq(p200[0].distance, 12.3456);
        double_eq(p200[0].target_height, 2.0);
        // "31..18+0000000001234567": unit 8, last digit 1/100 mm: 12.34567 m.
        let p201 = distances(&read.project.stations[0], "P201");
        assert_eq!(p201.len(), 1);
        double_eq(p201[0].distance, 12.34567);
    }

    #[test]
    fn gsi16_reads_decimal_degrees_and_sexagesimal_angles() {
        let read = read_fixture("tps_gsi16.gsi");
        let setup = &read.project.stations[0];
        // P200: "21.103+0000000009000000" - unit 3, five decimals: 90.00000 deg.
        // "22.103+0000000027000000" = 270 deg: face right, zenith 360 - 270 = 90.
        let hz200 = directions(setup, "P200");
        let v200 = zeniths(setup, "P200");
        assert_eq!(hz200.len(), 1);
        assert_eq!(v200.len(), 1);
        near(hz200[0].direction, 90.0 * DEGREE, ANGLE_TOLERANCE);
        near(v200[0].angle, 90.0 * DEGREE, ANGLE_TOLERANCE);
        assert_eq!(v200[0].pointing.face, Face::Right);
        // P201: "21.104+0000000004530150" - unit 4, DDD.MMSSs: 45 deg 30' 15.0" =
        // 45 + 30/60 + 15/3600 = 45.504166... deg.
        let hz201 = directions(setup, "P201");
        assert_eq!(hz201.len(), 1);
        near(
            hz201[0].direction,
            (45.0 + 30.0 / 60.0 + 15.0 / 3600.0) * DEGREE,
            ANGLE_TOLERANCE,
        );
        // The file mixes decimal degrees and sexagesimal, and says so.
        assert_eq!(read.project.units.angular, AngularUnit::DecimalDegrees);
        assert!(all_warnings(&read).contains("mixes angle units"));
    }

    #[test]
    fn the_station_blocks_serial_type_and_time_fill_the_instrument_settings() {
        let read = read_fixture("tps_gsi16.gsi");
        let instrument = &read.project.stations[0].instrument;
        assert_eq!(instrument.serial_number, "123456"); // "12....+0000000000123456"
        assert_eq!(instrument.model, "TS16"); // "13....+000000000000TS16"
        // Word 19 "03051015" is MMDDhhmm: 5 March, 10:15. Word 18 "2430500" is
        // YYSSmmm: 2024 (POSIX %y: 00-68 are 20xx), 30 s, 500 ms.
        assert!(instrument.time.known());
        assert_eq!(instrument.time.year, 2024);
        assert_eq!(instrument.time.month, 3);
        assert_eq!(instrument.time.day, 5);
        assert_eq!(instrument.time.hour, 10);
        assert_eq!(instrument.time.minute, 15);
        double_eq(instrument.time.second, 30.5);
        // No ppm or prism word anywhere: stated as unknown, not assumed.
        assert!(instrument.atmospheric_ppm.is_none());
        assert_eq!(instrument.atmospheric_ppm_state, CorrectionState::Unknown);
        assert!(says(&read.not_carried, "no atmospheric ppm"));
    }

    // ---- Sixty seconds (data/leica/rounded_seconds_gsi16.gsi) ------------------------------
    //
    // A sexagesimal angle's minutes and seconds run to 59, and 60.0 seconds is
    // exactly the next minute. A writer that rounds the seconds on their own,
    // without carrying into the minutes, writes 60 for a value within its
    // rounding of the next minute; one file seen had 24 words so, each agreeing
    // with its target's other readings only when read as the next minute
    // (docs/survey.md, "Leica GSI"). In the fixture, CP01 is keyed in 100 m due
    // north of STN1; P1 and P2 are measured with such words, P3 with 61 seconds,
    // which no file has shown a writer make.

    #[test]
    fn sixty_seconds_with_nothing_after_them_are_read_as_the_next_minute() {
        let read = read_fixture("rounded_seconds_gsi16.gsi");
        assert_eq!(read.project.stations.len(), 1);
        let setup = &read.project.stations[0];
        // Record 4, P1. Hz "21.324+0000000004459600": unit 4, DDD MM SS s, so 044
        // 59 60.0 - which is 045 00 00.0, pi/4 = 0.785398163397448310 rad. V
        // "22.324+0000000008959600" = 089 59 60.0, the carry running on through
        // the minutes into the degrees: 090 00 00.0 = pi/2, under half a turn, so
        // face left.
        let hz_p1 = directions(setup, "P1");
        let v_p1 = zeniths(setup, "P1");
        assert_eq!(hz_p1.len(), 1);
        assert_eq!(v_p1.len(), 1);
        near(hz_p1[0].direction, 0.785398163397448310, ANGLE_TOLERANCE);
        near(v_p1[0].angle, 1.570796326794896619, ANGLE_TOLERANCE);
        assert_eq!(v_p1[0].pointing.face, Face::Left);
        // Record 5, P2. V "22.324+0000000008627600" = 086 27 60.0 = 086 28 00.0 =
        // 86 + 28/60 = 86.4666... deg, which times pi/180 - worked in exact
        // rationals with a 75-digit pi, not by this reader - is
        // 1.509128026557763641 rad. Hz "21.324+0000000013500000" = 135 00 00.0 =
        // 3 pi / 4 = 2.356194490192344929 rad.
        let v_p2 = zeniths(setup, "P2");
        let hz_p2 = directions(setup, "P2");
        assert_eq!(v_p2.len(), 1);
        assert_eq!(hz_p2.len(), 1);
        near(v_p2[0].angle, 1.509128026557763641, ANGLE_TOLERANCE);
        near(hz_p2[0].direction, 2.356194490192344929, ANGLE_TOLERANCE);
        // Each slope distance keeps the zenith angle of its pointing, so each of
        // the two shots is its three observations.
        assert_eq!(distances(setup, "P1").len(), 1);
        assert_eq!(distances(setup, "P2").len(), 1);
        assert_eq!(read.records_read, 6);
        assert_eq!(read.records_skipped, 0);
    }

    #[test]
    fn the_import_says_once_how_many_angle_words_wrote_sixty_seconds_and_where_they_are() {
        let read = read_fixture("rounded_seconds_gsi16.gsi");
        // Three words write 60 seconds - record 4's two and record 5's V - and
        // they are one warning, at the first of them. It lists their records, so
        // each can be found, and shows the first as written and as read: record
        // 4's Hz "0000000004459600" is 044 59 60.0, which is 045 00 00.0.
        let summary = warning_about(&read, 4, "3 angle words write 60 seconds")
            .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(
            summary.message.contains(
                "(at records 4, 5; the first, word 21, 044 59 60.0, read as 045 00 00.0)"
            ),
            "{}",
            summary.message
        );
        assert!(
            summary.message.contains("next minute"),
            "{}",
            summary.message
        );
        // The file writes whole seconds and tenths; the reading is as near what
        // was measured as the writer's rounding, whatever its last digit.
        assert!(
            summary
                .message
                .contains("within the writer's rounding of what was measured"),
            "{}",
            summary.message
        );
        let about_sixty = read
            .warnings
            .iter()
            .filter(|warning| warning.message.contains("60 seconds"))
            .count();
        assert_eq!(about_sixty, 1);
        // None of the three is refused: that warning and record 6's are all.
        assert_eq!(read.warnings.len(), 2, "{}", all_warnings(&read));
    }

    #[test]
    fn seconds_past_sixty_are_no_rounding_and_their_word_is_still_refused_by_record() {
        let read = read_fixture("rounded_seconds_gsi16.gsi");
        let setup = &read.project.stations[0];
        // Record 6, P3: Hz "21.324+0000000009000610" = 090 00 61.0. No file has
        // shown a writer make 61, so the word is refused where it is, and only
        // it: the shot's zenith and distance are read.
        let refused = warning_about(&read, 6, "the seconds run to 59")
            .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(
            refused.message.contains("word 21 '0000000009000610'"),
            "{}",
            refused.message
        );
        assert!(
            refused.message.contains("that value was not read"),
            "{}",
            refused.message
        );
        assert!(directions(setup, "P3").is_empty());
        assert_eq!(zeniths(setup, "P3").len(), 1);
        assert_eq!(distances(setup, "P3").len(), 1);
    }

    // ---- Feet and mils (data/leica/feet_mil_gsi8.gsi) ------------------------------------

    #[test]
    fn feet_are_read_as_us_survey_feet_and_say_so_and_mils_have_four_decimals() {
        let read = read_fixture("feet_mil_gsi8.gsi");
        let station = point(&read.project, "STN5").expect("STN5");
        // "84..11+00328084": unit 1, last digit 1/1000 ft: 328.084 ft, and a US
        // survey foot is 1200/3937 m: 328.084 * 1200 / 3937 m.
        near(station.easting, 328.084 * 1200.0 / 3937.0, 1e-12);
        near(station.northing, 656.168 * 1200.0 / 3937.0, 1e-12);
        assert_eq!(read.project.units.linear, LinearUnit::UsSurveyFeet);
        assert!(all_warnings(&read).contains("US survey feet"));
        let setup = &read.project.stations[0];
        near(setup.setup.instrument_height, 5.0 * 1200.0 / 3937.0, 1e-12);
        // "21.105+16000000": unit 5, 6400 mil to the circle, four decimals:
        // 1600.0000 mil = a quarter turn = pi/2.
        let hz = directions(setup, "301");
        assert_eq!(hz.len(), 1);
        near(hz[0].direction, PI / 2.0, ANGLE_TOLERANCE);
        assert_eq!(read.project.units.angular, AngularUnit::Mils);
        let sd = distances(setup, "301");
        assert_eq!(sd.len(), 1);
        near(sd[0].distance, 32.808 * 1200.0 / 3937.0, 1e-12);
    }

    // ---- A coordinate list (data/leica/coordinates_gsi8.gsi) -----------------------------

    #[test]
    fn station_words_with_no_measurement_after_them_are_a_coordinate_list_not_setups() {
        let read = read_fixture("coordinates_gsi8.gsi");
        assert!(read.project.stations.is_empty());
        let p1 = point(&read.project, "1").expect("1");
        // "84..00+1234.567": the file writes its own decimal point; read as written.
        double_eq(p1.easting, 1234.567);
        double_eq(p1.northing, 2345.678);
        double_eq(p1.elevation.expect("a height"), 12.345);
        // "86..00+00012.35" - two decimals where the others have three.
        double_eq(
            point(&read.project, "2")
                .and_then(|p| p.elevation)
                .expect("a height"),
            12.35,
        );
        // Station words, input mode 0: "measured" says nothing about a station.
        assert_eq!(p1.coordinate_source, CoordinateSource::Unknown);
        assert!(
            warning_about(&read, 1, "decimal point").is_some(),
            "{}",
            all_warnings(&read)
        );
        assert!(
            warning_about(&read, 1, "coordinates of the point they name").is_some(),
            "{}",
            all_warnings(&read)
        );
        // Records 1-3 write a point in all nine coordinate words; record 4's three
        // ("81..00+01240000" first) and record 5's one leave it to the unit digit:
        // 4 of 13, the fewer, so they are the ones named, from record 4.
        let mixed = warning_about(&read, 4, "4 length words leave the decimal point")
            .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(mixed.message.contains("'01240000'"), "{}", mixed.message);
        assert!(
            mixed.message.contains("file's other 9 write one"),
            "{}",
            mixed.message
        );
    }

    #[test]
    fn text_in_a_height_word_is_the_points_code_and_the_import_says_so() {
        let read = read_fixture("coordinates_gsi8.gsi");
        // "88..00+00000STR": word 88 is the instrument height, and this is text.
        assert_eq!(point(&read.project, "1").expect("1").code, "STR");
        assert_eq!(point(&read.project, "3").expect("3").code, "FL");
        assert!(
            warning_about(&read, 1, "read as the point's code").is_some(),
            "{}",
            all_warnings(&read)
        );
        // Word 71 in its own block: "71....+00000TRE"; "81..00+01240000" = 1240.000.
        let a7 = point(&read.project, "A7").expect("A7");
        assert_eq!(a7.code, "TRE");
        double_eq(a7.easting, 1240.0);
        assert_eq!(read.project.features.len(), 3);
        assert_eq!(read.project.features[0].point_ids, ["1", "2"]);
        assert_eq!(read.project.features[1].code, "FL");
        assert_eq!(read.project.features[2].code, "TRE");
    }

    #[test]
    fn a_height_with_no_position_is_kept_in_metadata_not_placed_at_the_origin() {
        let read = read_fixture("coordinates_gsi8.gsi");
        // Record 5: "83..00+00014000" = 14.000 m and nothing else.
        assert!(point(&read.project, "5").is_none());
        let p5 = unpositioned(&read.project, "5").expect("5");
        assert_eq!(
            p5.metadata
                .get("height without a position")
                .map(String::as_str),
            Some("14.0")
        );
        assert!(
            warning_about(&read, 5, "height and no easting").is_some(),
            "{}",
            all_warnings(&read)
        );
    }

    // ---- Damage (data/leica/damaged_gsi8.gsi) --------------------------------------------

    #[test]
    fn damaged_words_and_lines_are_warnings_naming_their_record_and_the_rest_is_read() {
        let read = read_fixture("damaged_gsi8.gsi");
        // Record 4: Hz "1000000X" is not a number - that value only is lost.
        assert!(
            warning_about(&read, 4, "not a number").is_some(),
            "{}",
            all_warnings(&read)
        );
        let setup = &read.project.stations[0];
        assert!(directions(setup, "202").is_empty());
        assert_eq!(zeniths(setup, "202").len(), 1);
        // Record 5 is not GSI at all.
        assert!(
            warning_about(&read, 5, "not read").is_some(),
            "{}",
            all_warnings(&read)
        );
        assert_eq!(read.records_skipped, 1);
        assert_eq!(read.records_read, 6);
        // Record 6: word 99 is not a word this parser reads.
        assert!(
            warning_about(&read, 6, "word 99").is_some(),
            "{}",
            all_warnings(&read)
        );
    }

    #[test]
    fn a_point_given_two_positions_keeps_the_first_and_the_difference_is_reported() {
        let read = read_fixture("damaged_gsi8.gsi");
        // Records 2 and 3: "82..00+00010000" then "82..00+00010003" - 3 mm apart.
        let p201 = point(&read.project, "201").expect("201");
        double_eq(p201.northing, 10.0);
        let warning = warning_about(&read, 3, "given coordinates again")
            .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(warning.message.contains("0.003 m"), "{}", warning.message);
    }

    #[test]
    fn a_code_block_with_no_point_after_it_belongs_to_the_point_before() {
        let read = read_fixture("damaged_gsi8.gsi");
        // Record 1 is a point block and record 7, "410007+000000TR", the last:
        // the shape of a job recorded After Point, so TR codes 203 (record 6).
        let p203 = unpositioned(&read.project, "203").expect("203");
        assert_eq!(p203.code, "TR");
        assert!(
            warning_about(&read, 7, "After Point").is_some(),
            "{}",
            all_warnings(&read)
        );
        assert_eq!(
            read.project.metadata["code blocks belong to"],
            "the point before them"
        );
    }

    // Leica's TPS1200 Technical Reference Manual (version 5.0, 16.3 "Coding &
    // Linework Settings"): <Rec Free Code: Before Point / After Point> "determines
    // if a free code is stored before or after the point", and no GSI word says
    // which. The two fixtures hold the same survey recorded each way - points 1
    // and 2 KERB, point 3 TREE - and must read alike.
    #[test]
    fn the_same_codes_recorded_before_or_after_their_points_code_the_same_points() {
        for name in ["codes_before_gsi8.gsi", "codes_after_gsi8.gsi"] {
            let after = name == "codes_after_gsi8.gsi";
            let read = read_fixture(name);
            assert_eq!(read.project.points.len(), 3, "{name}");
            let p1 = point(&read.project, "1").expect("1");
            let p2 = point(&read.project, "2").expect("2");
            let p3 = point(&read.project, "3").expect("3");
            assert_eq!(p1.code, "KERB", "{name}");
            assert_eq!(p2.code, "KERB", "{name}");
            assert_eq!(p3.code, "TREE", "{name}");
            // No code was displaced into another point's metadata.
            assert!(!p3.metadata.contains_key("code block"), "{name}");
            // "81..00+00002000": unit 0, the last digit a millimetre, 2.000 m.
            double_eq(p3.easting, 2.0);
            double_eq(p3.northing, 2.1);

            assert_eq!(read.project.features.len(), 2, "{name}");
            assert_eq!(read.project.features[0].code, "KERB", "{name}");
            assert_eq!(read.project.features[0].point_ids, ["1", "2"], "{name}");
            // A feature starts at its first point's block: record 1 when the code
            // follows the point, record 2 when it precedes it.
            assert_eq!(
                read.project.features[0].source.record_number,
                if after { 1 } else { 2 },
                "{name}"
            );
            assert_eq!(read.project.features[1].code, "TREE", "{name}");
            assert_eq!(read.project.features[1].point_ids, ["3"], "{name}");

            assert_eq!(
                read.project.metadata["code blocks belong to"],
                if after {
                    "the point before them"
                } else {
                    "the point after them"
                },
                "{name}"
            );
            assert!(says(&read.not_carried, "<Rec Free Code:>"), "{name}");
            // The After Point file is told so at its first code block, record 2;
            // the Before Point file needs no warning.
            assert_eq!(
                warning_about(&read, 2, "After Point").is_some(),
                after,
                "{name}: {}",
                all_warnings(&read)
            );
            assert_eq!(
                read.warnings.is_empty(),
                !after,
                "{name}: {}",
                all_warnings(&read)
            );
        }
    }

    // The reviewer's case: a code block ending a round of shots. The file begins
    // with a code block, so it reads Before Point and the last code block has no
    // point after it. The block before it (record 6) is BS, face right, which was
    // NOT the last point the file named first (101, record 4).
    #[test]
    fn a_code_block_ending_a_round_of_shots_goes_to_the_point_of_the_block_before_it() {
        let read = read_ok(concat!(
            "410001+0000CTRL \r\n",
            "110002+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n",
            "110003+000000BS 21.102+00000000 22.102+10000000 31..00+00100000 \r\n",
            "110004+00000101 21.102+05000000 22.102+09500000 31..00+00025000 \r\n",
            "110005+00000101 21.102+25000000 22.102+30500000 31..00+00025000 \r\n",
            "110006+000000BS 21.102+20000000 22.102+30000000 31..00+00100000 \r\n",
            "410007+0000KERB \r\n",
        ));
        let bs = unpositioned(&read.project, "BS").expect("BS");
        let p101 = unpositioned(&read.project, "101").expect("101");
        assert_eq!(bs.code, "KERB");
        assert!(p101.code.is_empty(), "{}", p101.code);
        assert!(
            warning_about(&read, 7, "attached to the point before it, BS").is_some(),
            "{}",
            all_warnings(&read)
        );
        // CTRL, the first block, codes the point after it: the station.
        let station = point(&read.project, "STN1").expect("STN1");
        assert_eq!(station.code, "CTRL");
        assert_eq!(
            read.project.metadata["code blocks belong to"],
            "the point after them"
        );
    }

    // ---- Inline cases --------------------------------------------------------------------

    #[test]
    fn measurements_before_any_station_block_are_kept_under_a_setup_with_no_position() {
        let read = read_ok(concat!(
            "110001+00000001 21.102+00000000 22.102+10000000 31..00+00010000 \r\n",
            "110002+00000002 21.102+10000000 22.102+10000000 31..00+00020000 \r\n",
        ));
        assert_eq!(read.project.stations.len(), 1);
        let at = &read.project.stations[0].setup.point_id;
        assert_eq!(at, "GSI station");
        assert!(unpositioned(&read.project, at).is_some());
        assert_eq!(read.project.stations[0].observations.len(), 6);
        assert!(warning_about(&read, 1, "before any station block").is_some());
        assert!(says(
            &read.not_carried,
            "no instrument height on 1 of 1 setup"
        ));
        assert!(says(
            &read.not_carried,
            "no reflector height before 2 shots"
        ));
    }

    #[test]
    fn a_first_shot_to_a_point_positioned_earlier_is_the_backsight_and_one_positioned_by_itself_is_not()
     {
        // Record 1 keys in CP01 (input mode 1, "81..10"); record 2 sets up on
        // STN1; its first shot (record 3) is to CP01, which the file positioned
        // before it: the backsight. Setup 2's first shot (record 6) is to point
        // 7, whose coordinates come in that same block, measured: no backsight.
        let read = read_ok(concat!(
            "110001+0000CP01 81..10+00100000 82..10+00300000 83..10+00050000 \r\n",
            "110002+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n",
            "110003+0000CP01 21.102+00000000 22.102+10000000 31..00+00100000 \r\n",
            "110004+00000006 21.102+10000000 22.102+10000000 31..00+00020000 \r\n",
            "110005+0000STN2 84..10+00140000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n",
            "110006+00000007 21.102+00000000 22.102+10000000 31..00+00010000 81..00+00150000 ",
            "82..00+00200000 83..00+00050000 \r\n",
        ));
        let project = &read.project;
        assert_eq!(project.stations.len(), 2);
        assert_eq!(project.stations[0].backsight_point_id, "CP01");
        assert!(project.stations[0].metadata.contains_key("backsight"));
        assert!(project.stations[1].backsight_point_id.is_empty());
        assert!(!project.stations[1].metadata.contains_key("backsight"));
        assert!(says(
            &read.not_carried,
            "1 of 2 setups took the point of their first shot"
        ));
        assert!(says(
            &read.not_carried,
            "cannot be oriented until its backsight is known"
        ));
        // CP01 was keyed in, so it is known to the reduction as entered.
        assert_eq!(
            point(project, "CP01").expect("CP01").coordinate_source,
            CoordinateSource::Entered
        );
    }

    // A traverse is often begun on a mark whose coordinates the job states only
    // when the instrument stands on it later. The first setup's first shot is to
    // that mark, and a reduction, which has the whole file, can orient on it.
    #[test]
    fn a_first_shot_to_a_point_the_file_gives_coordinates_only_later_is_still_the_backsight() {
        // Setup 1 on STN1 (record 1) shoots STN2 first (record 2); STN2 is keyed
        // in only by its own setup, record 4. Setup 2's first shot is to STN1,
        // keyed in before it.
        let read = read_ok(concat!(
            "110001+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n",
            "110002+0000STN2 21.102+00000000 22.102+10000000 31..00+00100000 \r\n",
            "110003+00000101 21.102+05000000 22.102+10000000 31..00+00020000 \r\n",
            "110004+0000STN2 84..10+00100000 85..10+00300000 86..10+00050000 88..10+00001500 \r\n",
            "110005+0000STN1 21.102+00000000 22.102+10000000 31..00+00100000 \r\n",
        ));
        let project = &read.project;
        assert_eq!(project.stations.len(), 2);
        assert_eq!(project.stations[0].backsight_point_id, "STN2");
        let backsight = project.stations[0]
            .metadata
            .get("backsight")
            .expect("the backsight is said");
        assert!(backsight.contains("later in the file"), "{backsight}");
        assert_eq!(project.stations[1].backsight_point_id, "STN1");
        assert!(says(
            &read.not_carried,
            "2 of 2 setups took the point of their first shot as the backsight"
        ));
        assert!(says(
            &read.not_carried,
            "1 of them from coordinates later in the file"
        ));
        assert!(!says(
            &read.not_carried,
            "cannot be oriented until its backsight is known"
        ));
    }

    // The other side of that rule: a point positioned only by the setup's own
    // shots was positioned with the orientation the backsight is to give.
    #[test]
    fn a_first_shot_to_a_point_only_its_own_setup_positions_names_no_backsight() {
        // Record 2, the setup's first shot, is to 7, unpositioned then; record 3
        // shoots 7 again and gives the coordinates the instrument computed from
        // this setup (81-83, input mode 0: measured).
        let read = read_ok(concat!(
            "110001+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n",
            "110002+00000007 21.102+00000000 22.102+10000000 31..00+00010000 \r\n",
            "110003+00000007 21.102+20000000 22.102+30000000 31..00+00010000 81..00+00100000 ",
            "82..00+00210000 83..00+00050000 \r\n",
        ));
        assert_eq!(read.project.stations.len(), 1);
        assert!(read.project.stations[0].backsight_point_id.is_empty());
        assert!(!read.project.stations[0].metadata.contains_key("backsight"));
        assert!(point(&read.project, "7").is_some()); // positioned, but by this setup
        assert!(says(&read.not_carried, "no setup's first shot"));
    }

    // The carry is made in whole numbers before the angle becomes a real, so a
    // carried word is the same double as the word written with its carry made,
    // whatever the word's width. Adding 60/3600 of a degree as a real is not:
    // 213 + 46/60 + 60/3600, in doubles and times pi/180 as the reader turns it
    // into radians, is one unit in the last place (4.4e-16 rad) from 213 +
    // 47/60, which is why this test's angle is 213 46 60.0 (213 17 60.0, for
    // one, comes out the same both ways and would not tell them apart; both
    // worked in Python's doubles, whose arithmetic the build's -ffp-contract=off
    // makes the same).
    #[test]
    fn a_carried_angle_is_exactly_the_angle_written_with_its_carry_in_either_width() {
        // A GSI-8 station block, then GSI-16 and GSI-8 shots: P1 at 213 46 60.0,
        // P2 at 213 47 00.0 - 213.78333... deg = 3.731223052555210914 rad, exact
        // rationals and a 75-digit pi - and P3 the GSI-8 "21346600". P4's Hz is
        // 359 59 60.0: the carry runs through the minutes into the degrees, 360
        // 00 00.0, the circle's zero. Its V "08459600" is 084 59 60.0 = 085 00
        // 00.0 = 1.483529864195180140 rad.
        let read = read_ok(concat!(
            "110001+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n",
            "*110002+00000000000000P1 21.324+0000000021346600 22.324+0000000009000000 \r\n",
            "*110003+00000000000000P2 21.324+0000000021347000 22.324+0000000009000000 \r\n",
            "110004+000000P3 21.324+21346600 22.324+09000000 \r\n",
            "110005+000000P4 21.324+35959600 22.324+08459600 \r\n",
        ));
        let setup = &read.project.stations[0];
        let p1 = directions(setup, "P1");
        let p2 = directions(setup, "P2");
        let p3 = directions(setup, "P3");
        let p4 = directions(setup, "P4");
        assert_eq!(p1.len(), 1);
        assert_eq!(p2.len(), 1);
        assert_eq!(p3.len(), 1);
        assert_eq!(p4.len(), 1);
        near(p1[0].direction, 3.731223052555210914, ANGLE_TOLERANCE);
        assert_eq!(p1[0].direction, p2[0].direction);
        assert_eq!(p3[0].direction, p1[0].direction);
        // Zero on the circle, from either side of it.
        let from_zero = p4[0].direction.min(2.0 * PI - p4[0].direction);
        assert!(from_zero < ANGLE_TOLERANCE, "{}", p4[0].direction);
        let v4 = zeniths(setup, "P4");
        assert_eq!(v4.len(), 1);
        near(v4[0].angle, 1.483529864195180140, ANGLE_TOLERANCE);
        assert!(
            warning_about(&read, 2, "4 angle words write 60 seconds").is_some(),
            "{}",
            all_warnings(&read)
        );
    }

    #[test]
    fn sixty_seconds_with_tenths_or_minutes_of_sixty_are_no_rounding_and_are_refused() {
        // What no file has shown a writer's rounding make, each refused at its
        // record, with nothing carried:
        //   record 2: 086 27 60.1 - the seconds past 60.0;
        //   record 3: 086 60 00.0 - minutes of 60, which a rounding of the
        //             seconds leaves only if a second carry was also dropped
        //             (docs/survey.md, "Leica GSI");
        //   record 4: 086 27 99.0.
        let read = read_ok(concat!(
            "*110001+000000000000STN1 88..10+0000000000001500 \r\n",
            "*110002+00000000000000P1 21.324+0000000000000000 22.324+0000000008627601 \r\n",
            "*110003+00000000000000P2 21.324+0000000000000000 22.324+0000000008660000 \r\n",
            "*110004+00000000000000P3 21.324+0000000000000000 22.324+0000000008627990 \r\n",
        ));
        let setup = &read.project.stations[0];
        for id in ["P1", "P2", "P3"] {
            assert!(zeniths(setup, id).is_empty(), "{id}");
            assert_eq!(directions(setup, id).len(), 1, "{id}");
        }
        assert!(
            warning_about(&read, 2, "the seconds run to 59").is_some(),
            "{}",
            all_warnings(&read)
        );
        assert!(
            warning_about(&read, 3, "the minutes run to 59").is_some(),
            "{}",
            all_warnings(&read)
        );
        assert!(
            warning_about(&read, 4, "the seconds run to 59").is_some(),
            "{}",
            all_warnings(&read)
        );
        assert!(!all_warnings(&read).contains("60 seconds"));
    }

    // 60 seconds are the next minute only when the word itself writes both digits
    // of the seconds where the format puts them. A word that is not its block's
    // width is aligned from the right, and a written point's missing digits are
    // supplied as zeros: either can make a 60 of digits that were never the
    // seconds, and carrying it would turn a damaged word, refused at its record,
    // into an angle degrees wrong with no warning of its own.
    #[test]
    fn sixty_seconds_the_word_does_not_write_in_their_place_are_refused_not_carried() {
        //   record 2: GSI-16 "000000002134600", 15 characters - 213 46 00.0 with
        //             its last digit lost - reads from the right as 021 34 60.0,
        //             which carried would be 021 35 00.0, 192 degrees out;
        //   record 3: "000000000213.466" writes one digit of the seconds after
        //             its point, and is 60 only with the 0 supplied after it;
        //   record 4: GSI-8 "1600", 4 characters, filled from the left to
        //             000 01 60.0;
        //   record 5: "00000000213.4660", four digits after its point, 213 46
        //             60.0 with both digits written: carried;
        //   record 6: GSI-8 "21346600", its block's width: carried.
        let read = read_ok(concat!(
            "*110001+000000000000STN1 88..10+0000000000001500 \r\n",
            "*110002+00000000000000P1 21.324+000000002134600 22.324+0000000009000000 ",
            "31..00+0000000000100000 \r\n",
            "*110003+00000000000000P2 21.324+000000000213.466 22.324+0000000009000000 ",
            "31..00+0000000000100000 \r\n",
            "110004+000000P3 21.324+1600 22.324+09000000 31..00+00100000 \r\n",
            "*110005+00000000000000P4 21.324+00000000213.4660 22.324+0000000009000000 ",
            "31..00+0000000000100000 \r\n",
            "110006+000000P5 21.324+21346600 22.324+09000000 31..00+00100000 \r\n",
        ));
        let setup = &read.project.stations[0];
        // Refused where they are, and only those words: each shot keeps its
        // zenith angle and its distance.
        for id in ["P1", "P2", "P3"] {
            assert!(directions(setup, id).is_empty(), "{id}");
            assert_eq!(zeniths(setup, id).len(), 1, "{id}");
            assert_eq!(distances(setup, id).len(), 1, "{id}");
        }
        let wide = warning_about(
            &read,
            2,
            "only in a word of its block's width, 16 characters",
        )
        .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(
            wide.message.contains("word 21 '000000002134600'"),
            "{}",
            wide.message
        );
        assert!(
            wide.message
                .contains("this one has 15; that value was not read"),
            "{}",
            wide.message
        );
        let pointed = warning_about(
            &read,
            3,
            "it writes one digit of the seconds after its point",
        )
        .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(
            pointed.message.contains("word 21 '000000000213.466'"),
            "{}",
            pointed.message
        );
        let narrow = warning_about(
            &read,
            4,
            "only in a word of its block's width, 8 characters",
        )
        .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(
            narrow.message.contains("this one has 4;"),
            "{}",
            narrow.message
        );
        // Records 5 and 6 write both digits: 213 47 00.0 = 213 + 47/60 deg =
        // 3.731223052555210914 rad (exact rationals and a 75-digit pi).
        for id in ["P4", "P5"] {
            let hz = directions(setup, id);
            assert_eq!(hz.len(), 1, "{id}");
            near(hz[0].direction, 3.731223052555210914, ANGLE_TOLERANCE);
        }
        // And only those two are said to have been read as the next minute.
        let summary = warning_about(&read, 5, "2 angle words write 60 seconds")
            .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(
            summary
                .message
                .contains("(at records 5, 6; the first, word 21, 213 46 60, read as 213 47 00)"),
            "{}",
            summary.message
        );
    }

    // The note counts the 60-second words whose values became observations. One
    // in a code block, whose measurement words are not read, or in a shot at the
    // point the instrument stands on, which is not read, is not among them: the
    // import would otherwise say at one record both that the word was read as
    // the next minute and that it was not read.
    #[test]
    fn a_sixty_second_word_whose_value_is_not_kept_is_not_said_to_be_read_as_the_next_minute() {
        let read = read_ok(concat!(
            "*110001+000000000000STN1 84..10+0000000000100000 85..10+0000000000200000 ",
            "88..10+0000000000001500 \r\n",
            "*410002+000000000000KERB 21.324+0000000004459600 \r\n",
            "*110003+000000000000STN1 21.324+0000000004459600 22.324+0000000009000000 ",
            "31..00+0000000000100000 \r\n",
            "*110004+00000000000000P1 21.324+0000000001000000 22.324+0000000008959600 ",
            "31..00+0000000000100000 \r\n",
        ));
        assert!(
            warning_about(&read, 2, "a code block holds measurement words").is_some(),
            "{}",
            all_warnings(&read)
        );
        assert!(
            warning_about(&read, 3, "the point the instrument stands on").is_some(),
            "{}",
            all_warnings(&read)
        );
        // Record 4's V "0000000008959600", 089 59 60.0, is the one kept: 090 00
        // 00.0 = pi/2 = 1.570796326794896619 rad, face left.
        let v = zeniths(&read.project.stations[0], "P1");
        assert_eq!(v.len(), 1);
        near(v[0].angle, 1.570796326794896619, ANGLE_TOLERANCE);
        assert_eq!(v[0].pointing.face, Face::Left);
        let summary = warning_about(&read, 4, "1 angle word writes 60 seconds")
            .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(
            summary
                .message
                .contains("(at record 4: word 22, 089 59 60.0, read as 090 00 00.0)"),
            "{}",
            summary.message
        );
        assert!(
            summary
                .message
                .contains("so it was read as the next minute"),
            "{}",
            summary.message
        );
        let about_sixty = read
            .warnings
            .iter()
            .filter(|warning| warning.message.contains("60 seconds"))
            .count();
        assert_eq!(about_sixty, 1, "{}", all_warnings(&read));
    }

    // The unit digit names the circle - 400 gon, 360 degrees decimal or
    // sexagesimal, 6400 mil (GSI ONLINE, page 6) - and no circle reading is past
    // it. Such a word is damaged: refused at its record, not wrapped onto
    // the circle as a direction that looks like any other. A full circle is the
    // circle's zero, as a rounding up at the top of the circle writes it.
    //
    // The check has two edges, and a word on each side of both: at 360 degrees
    // in a sexagesimal word, where anything after 360 00 00.0 - in the tenths,
    // the seconds, or the minutes a 60-second carry made - is past it; and below
    // the negative full circle in the other units, whose values are signed.
    #[test]
    fn a_circle_reading_past_a_full_circle_is_refused_in_every_unit_and_a_full_circle_is_zero() {
        //   record 2: Hz 450 00 00.0      record 6: Hz 360.00001 degrees
        //   record 3: V 365 00 00.0       record 7: Hz 6400.0001 mil
        //   record 4: Hz 9999 59 60.0     record 8: Hz 360 00 00.0 - zero
        //   record 5: Hz 400.00001 gon    record 9: Hz 359 59 60.0 - zero
        //                                 record 10: Hz 400.00000 gon - zero
        //   record 11: Hz 360 00 00.1     record 15: Hz -360.00001 degrees
        //   record 12: Hz 360 00 01.0     record 16: Hz -6400.0001 mil
        //   record 13: Hz 360 00 60.0,    record 17: Hz -360 00 00.1
        //              carried to 360 01 00.0
        //   record 14: Hz -400.00001 gon  record 18: Hz -400.00000 gon - zero
        let read = read_ok(concat!(
            "110001+0000STN1 88..10+00001500 \r\n",
            "110002+000000P1 21.324+45000000 22.324+09000000 \r\n",
            "110003+000000P2 21.324+04500000 22.324+36500000 \r\n",
            "*110004+00000000000000P3 21.324+0000000999959600 22.324+0000000009000000 \r\n",
            "110005+000000P4 21.102+40000001 22.102+10000000 \r\n",
            "110006+000000P5 21.103+36000001 22.103+09000000 \r\n",
            "110007+000000P6 21.105+64000001 22.105+16000000 \r\n",
            "110008+000000P7 21.324+36000000 22.324+09000000 \r\n",
            "110009+000000P8 21.324+35959600 22.324+09000000 \r\n",
            "110010+000000P9 21.102+40000000 22.102+10000000 \r\n",
            "110011+000000PA 21.324+36000001 22.324+09000000 \r\n",
            "110012+000000PB 21.324+36000010 22.324+09000000 \r\n",
            "110013+000000PC 21.324+36000600 22.324+09000000 \r\n",
            "110014+000000PD 21.102-40000001 22.102+10000000 \r\n",
            "110015+000000PE 21.103-36000001 22.103+09000000 \r\n",
            "110016+000000PF 21.105-64000001 22.105+16000000 \r\n",
            "110017+000000PG 21.324-36000001 22.324+09000000 \r\n",
            "110018+000000PH 21.102-40000000 22.102+10000000 \r\n",
        ));
        let setup = &read.project.stations[0];
        let refused: [(&str, usize, &str); 12] = [
            ("P1", 2, "(360 degrees)"),
            ("P3", 4, "(360 degrees)"),
            ("P4", 5, "(400 gon)"),
            ("P5", 6, "(360 degrees)"),
            ("P6", 7, "(6400 mil)"),
            ("PA", 11, "(360 degrees)"),
            ("PB", 12, "(360 degrees)"),
            ("PC", 13, "(360 degrees)"),
            ("PD", 14, "(400 gon)"),
            ("PE", 15, "(360 degrees)"),
            ("PF", 16, "(6400 mil)"),
            ("PG", 17, "(360 degrees)"),
        ];
        for (id, record, circle) in refused {
            assert!(directions(setup, id).is_empty(), "{id}");
            assert_eq!(zeniths(setup, id).len(), 1, "{id}");
            assert!(
                warning_about(&read, record, &format!("is past a full circle {circle}")).is_some(),
                "{id}\n{}",
                all_warnings(&read)
            );
        }
        assert!(zeniths(setup, "P2").is_empty());
        assert_eq!(directions(setup, "P2").len(), 1);
        assert!(
            warning_about(
                &read,
                3,
                "word 22 '36500000' is past a full circle (360 degrees)"
            )
            .is_some(),
            "{}",
            all_warnings(&read)
        );
        // A negative word is shown with its sign, as written.
        assert!(
            warning_about(
                &read,
                14,
                "word 21 '-40000001' is past a full circle (400 gon)"
            )
            .is_some(),
            "{}",
            all_warnings(&read)
        );
        for id in ["P7", "P8", "P9", "PH"] {
            let hz = directions(setup, id);
            assert_eq!(hz.len(), 1, "{id}");
            // Zero on the circle, from either side of it.
            let from_zero = hz[0].direction.min(2.0 * PI - hz[0].direction);
            assert!(from_zero < ANGLE_TOLERANCE, "{id}: {}", hz[0].direction);
        }
        // 9999 59 60.0 and 360 00 60.0 were refused after their carry, so the one
        // word read as the next minute is P8's.
        let summary = warning_about(&read, 9, "1 angle word writes 60 seconds")
            .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(
            summary
                .message
                .contains("word 21, 359 59 60.0, read as 360 00 00.0"),
            "{}",
            summary.message
        );
    }

    // Word 22 is read as a zenith angle, and no zenith angle is negative. A
    // negative V is a damaged word, or an instrument's vertical angle setting
    // reaching the file: GSI ONLINE lists one (page 9, SET 44 "V angle READING":
    // 0 Zenith, 1 Horizontal, 2 Slope in percent), under which a sight below the
    // horizon reads negative, but does not say that word 22 follows it. Wrapped
    // onto the circle, as the reader once did, V -005 00 00 became 355 00 00 - a
    // face-right zenith of 5 degrees, the shot drawn near the vertical and half a
    // turn round from where it was measured. It is refused at its record, in
    // every unit. Word 21 has no such second meaning (no setting in GSI ONLINE
    // signs it; 171 turns it clockwise or counterclockwise), so a negative one
    // is the direction it names, counted the other way from zero, and is read on
    // the circle.
    #[test]
    fn a_negative_vertical_reading_is_refused_at_its_record_not_wrapped_into_a_face_right_zenith() {
        //   record 2: V -005 00 00.0      record 6: V -000 00 60.0, carried to
        //   record 3: V -5.00000 gon                -000 01 00.0: still negative
        //   record 4: V -5.00000 degrees  record 7: V -000 00 00.0 - zero
        //   record 5: V -80.0000 mil      record 8: Hz -090 00 00.0
        //                                 record 9: Hz -100.00000 gon
        let read = read_ok(concat!(
            "110001+0000STN1 88..10+00001500 \r\n",
            "110002+000000P1 21.324+04500000 22.324-00500000 31..00+00100000 \r\n",
            "110003+000000P2 21.102+05000000 22.102-00500000 31..00+00100000 \r\n",
            "110004+000000P3 21.103+04500000 22.103-00500000 31..00+00100000 \r\n",
            "110005+000000P4 21.105+08000000 22.105-00800000 31..00+00100000 \r\n",
            "110006+000000P5 21.324+04500000 22.324-00000600 31..00+00100000 \r\n",
            "110007+000000P6 21.324+04500000 22.324-00000000 31..00+00100000 \r\n",
            "110008+000000P7 21.324-09000000 22.324+09000000 31..00+00100000 \r\n",
            "110009+000000P8 21.102-10000000 22.102+30000000 31..00+00100000 \r\n",
        ));
        let setup = &read.project.stations[0];
        let refused: [(&str, usize, &str); 5] = [
            ("P1", 2, "word 22 '-00500000' is negative"),
            ("P2", 3, "word 22 '-00500000' is negative"),
            ("P3", 4, "word 22 '-00500000' is negative"),
            ("P4", 5, "word 22 '-00800000' is negative"),
            ("P5", 6, "word 22 '-00000600' is negative"),
        ];
        for (id, record, word) in refused {
            // The vertical reading alone: the shot keeps its direction and distance.
            assert!(zeniths(setup, id).is_empty(), "{id}");
            assert_eq!(directions(setup, id).len(), 1, "{id}");
            assert_eq!(distances(setup, id).len(), 1, "{id}");
            let warning = warning_about(&read, record, word)
                .unwrap_or_else(|| panic!("{id}\n{}", all_warnings(&read)));
            assert!(
                warning.message.contains("which no zenith angle is"),
                "{}",
                warning.message
            );
            // What wrote it is given as the possibilities it is, not as a fact:
            // GSI ONLINE lists SET 44 but does not say word 22 follows it.
            assert!(
                warning.message.contains(
                    "(a damaged word, or an instrument whose vertical angle setting - GSI \
                     ONLINE's SET 44"
                ),
                "{}",
                warning.message
            );
        }
        // P5's word writes 60 seconds and is refused, so nothing says it was
        // read as the next minute - as a count made where words are decoded,
        // rather than where their values are kept (kept_sixty), would.
        assert!(
            !all_warnings(&read).contains("60 seconds"),
            "{}",
            all_warnings(&read)
        );
        // -000 00 00.0 is zero, not below it: P6 has a zenith angle of 0.
        let p6 = zeniths(setup, "P6");
        assert_eq!(p6.len(), 1);
        assert_eq!(p6[0].angle, 0.0);
        assert!(
            warning_about(&read, 7, "word 22").is_none(),
            "{}",
            all_warnings(&read)
        );
        // Hz -090 00 00.0 is the direction 270 00 00.0 and -100 gon is 300 gon:
        // 3 pi / 2 = 4.712388980384689858 rad (a 50-digit pi) either way, with no
        // warning. P7's V of 090 00 00.0 is face left; P8's of 300 gon face right,
        // a zenith of 400 - 300 = 100 gon = pi / 2 = 1.570796326794896619 rad.
        let p7 = directions(setup, "P7");
        let p8 = directions(setup, "P8");
        assert_eq!(p7.len(), 1);
        assert_eq!(p8.len(), 1);
        near(p7[0].direction, 4.712388980384689858, ANGLE_TOLERANCE);
        near(p8[0].direction, 4.712388980384689858, ANGLE_TOLERANCE);
        assert_eq!(p7[0].pointing.face, Face::Left);
        assert_eq!(p8[0].pointing.face, Face::Right);
        let v8 = zeniths(setup, "P8");
        assert_eq!(v8.len(), 1);
        near(v8[0].angle, 1.570796326794896619, ANGLE_TOLERANCE);
        assert!(
            warning_about(&read, 8, "word 21").is_none(),
            "{}",
            all_warnings(&read)
        );
        assert!(
            warning_about(&read, 9, "word 21").is_none(),
            "{}",
            all_warnings(&read)
        );
        // Said once for the file: if a V setting wrote the negatives below the
        // horizon, it wrote positives above it, and those were read as zeniths.
        assert!(
            says(
                &read.not_carried,
                "word 22 was read as a zenith angle, the instruments' usual setting; 5 readings \
                 were refused as negative, which no zenith angle is (the first at record 2)"
            ),
            "{}",
            lacking(&read)
        );
        assert!(
            says(
                &read.not_carried,
                "the file's other word 22 readings are not zenith angles either"
            ),
            "{}",
            lacking(&read)
        );
    }

    // The note on the vertical angle setting says what was read. Where every
    // word 22 was refused as negative, none was read as a zenith angle: the note
    // says so, and neither that word 22 was read as one nor that the file's
    // other readings are in doubt - there are none. A negative V in a code block,
    // whose measurement words are not read, is refused and counted all the same:
    // the note counts refusals, each warned about at its record, as a sign of
    // what wrote the file. (The 60-second note says its words were read, and so
    // counts only those whose values were kept.)
    #[test]
    fn when_every_vertical_reading_is_refused_the_note_says_none_was_read_as_a_zenith_angle() {
        //   record 2: V -005 00 00.0 in a shot;
        //   record 3: V -005 00 00.0 in a code block;
        //   record 4: a shot with no V.
        let read = read_ok(concat!(
            "110001+0000STN1 88..10+00001500 \r\n",
            "110002+000000P1 21.324+04500000 22.324-00500000 31..00+00100000 \r\n",
            "410003+0000KERB 22.324-00500000 \r\n",
            "110004+000000P2 21.324+13500000 31..00+00100000 \r\n",
        ));
        let setup = &read.project.stations[0];
        assert!(all_zeniths(setup).is_empty());
        assert!(
            warning_about(&read, 2, "word 22 '-00500000' is negative").is_some(),
            "{}",
            all_warnings(&read)
        );
        assert!(
            warning_about(&read, 3, "word 22 '-00500000' is negative").is_some(),
            "{}",
            all_warnings(&read)
        );
        assert!(
            warning_about(&read, 3, "a code block holds measurement words").is_some(),
            "{}",
            all_warnings(&read)
        );
        assert!(
            says(
                &read.not_carried,
                "GSI does not record the vertical angle setting: 2 readings were refused as \
                 negative, which no zenith angle is (the first at record 2), and no word 22 was \
                 read as a zenith angle: such a reading is a damaged word"
            ),
            "{}",
            lacking(&read)
        );
        assert!(
            !says(&read.not_carried, "the instruments' usual setting"),
            "{}",
            lacking(&read)
        );
        assert!(
            !says(&read.not_carried, "other word 22 readings"),
            "{}",
            lacking(&read)
        );
    }

    // The instrument's own horizontal distance and height difference (32, 33)
    // are what it computed from the slope distance and its vertical reading.
    // Beside a zenith angle and a distance that were both read they are derived,
    // and not carried: the reduction computes its own. Where no zenith angle was
    // read beside the slope distance - none recorded, one refused as negative, or
    // one refused as past a full circle - the reduction can compute neither, and
    // they are the shot's only horizontal distance and height. They are carried
    // then, the horizontal distance as the shot's one distance, as in a block
    // with no slope distance, and the slope distance, which nothing can reduce
    // to horizontal, is not; so is a height difference beside a zenith angle
    // with no distance for the reduction to compute one from.
    #[test]
    fn a_shot_with_no_zenith_angle_read_keeps_its_own_horizontal_distance_and_height_difference() {
        // Each shot's 31 is 80.000 m, and its 32 and 33 are what an instrument
        // computes for that at a zenith of 094 30 00.0 with both heights equal:
        // 80 sin 94.5 deg = 79.7534 m, written 79.753, and 80 cos 94.5 deg =
        // -6.2767 m, written -6.277 (worked with a 75-digit pi).
        //   record 2: V -004 30 00.0, refused as negative;
        //   record 3: V 372 00 00.0, refused as past a full circle;
        //   record 4: no V;
        //   record 5: V 094 30 00.0, read: its 32 and 33 are derived;
        //   record 6: V read and no slope distance: its 32 is the distance and
        //             its 33 derived;
        //   record 7: V read and a 33 with no distance: the 33 is carried.
        let read = read_ok(concat!(
            "110001+0000STN1 88..10+00001500 \r\n",
            "110002+000000P1 21.324+05200000 22.324-00430000 31..00+00080000 32..00+00079753 ",
            "33..00-00006277 \r\n",
            "110003+000000P2 21.324+05200000 22.324+37200000 31..00+00080000 32..00+00079753 ",
            "33..00-00006277 \r\n",
            "110004+000000P3 21.324+05200000 31..00+00080000 32..00+00079753 33..00-00006277 \r\n",
            "110005+000000P4 21.324+05200000 22.324+09430000 31..00+00080000 32..00+00079753 ",
            "33..00-00006277 \r\n",
            "110006+000000P5 21.324+05200000 22.324+09430000 32..00+00079753 33..00-00006277 \r\n",
            "110007+000000P6 21.324+05200000 22.324+09430000 33..00-00006277 \r\n",
        ));
        let setup = &read.project.stations[0];
        for id in ["P1", "P2", "P3"] {
            assert!(zeniths(setup, id).is_empty(), "{id}");
            assert_eq!(directions(setup, id).len(), 1, "{id}");
            // One distance, the block's own horizontal one: 79.753 m.
            let distances = distances(setup, id);
            assert_eq!(distances.len(), 1, "{id}\n{}", lacking(&read));
            assert_eq!(distances[0].kind, DistanceKind::Horizontal, "{id}");
            double_eq(distances[0].distance, 79.753);
            let levels = levels(setup, id);
            assert_eq!(levels.len(), 1, "{id}\n{}", lacking(&read));
            assert_eq!(levels[0].from, "STN1", "{id}");
            double_eq(levels[0].height_difference, -6.277);
            double_eq(levels[0].length, 79.753);
        }
        // P4: the slope distance and the zenith angle; nothing else.
        let p4 = distances(setup, "P4");
        assert_eq!(p4.len(), 1);
        assert_eq!(p4[0].kind, DistanceKind::Slope);
        double_eq(p4[0].distance, 80.0);
        assert_eq!(zeniths(setup, "P4").len(), 1);
        assert!(levels(setup, "P4").is_empty());
        // P5: its own horizontal distance and the zenith angle, which give the height.
        let p5 = distances(setup, "P5");
        assert_eq!(p5.len(), 1);
        assert_eq!(p5[0].kind, DistanceKind::Horizontal);
        double_eq(p5[0].distance, 79.753);
        assert!(levels(setup, "P5").is_empty());
        // P6: the zenith angle and the height difference, over no known length.
        assert!(distances(setup, "P6").is_empty());
        let p6 = levels(setup, "P6");
        assert_eq!(p6.len(), 1, "{}", lacking(&read));
        double_eq(p6[0].height_difference, -6.277);
        assert_eq!(p6[0].length, 0.0);
        // What was not carried, and why: P4's and P5's derived values, and the
        // slope distances of P1, P2 and P3, the first at record 2.
        assert!(
            says(
                &read.not_carried,
                "2 blocks' instrument-computed horizontal distance (32) or height difference \
                 (33) beside a zenith angle and a distance that were read: not carried"
            ),
            "{}",
            lacking(&read)
        );
        assert!(
            says(
                &read.not_carried,
                "3 blocks have a slope distance (31) and no zenith angle that was read (none \
                 recorded, or one refused at its record; the first at record 2), so the slope \
                 distance cannot be reduced to horizontal: it was not carried, and the \
                 horizontal distance (32) the instrument computed for the same shot was carried \
                 in its place"
            ),
            "{}",
            lacking(&read)
        );
        assert!(
            warning_about(&read, 2, "is negative").is_some(),
            "{}",
            all_warnings(&read)
        );
        assert!(
            warning_about(&read, 3, "is past a full circle").is_some(),
            "{}",
            all_warnings(&read)
        );
    }

    // A sexagesimal word that writes no digit - "." - is no angle, and is refused
    // at its record as the same word is in gon, degrees or mil ("is not a
    // number"). The zeros the reader supplies to a short word would otherwise
    // make it 000 00 00.0: a shot at the circle's zero, with nothing to say so.
    #[test]
    fn a_sexagesimal_word_of_nothing_but_a_point_is_refused_not_read_as_zero() {
        //   record 2: Hz "." in unit 4, sexagesimal;
        //   record 3: Hz "." in unit 2, gon.
        let read = read_ok(concat!(
            "110001+0000STN1 88..10+00001500 \r\n",
            "110002+000000P1 21.324+. 22.324+09000000 \r\n",
            "110003+000000P2 21.102+. 22.102+10000000 \r\n",
        ));
        let setup = &read.project.stations[0];
        for id in ["P1", "P2"] {
            assert!(directions(setup, id).is_empty(), "{id}");
            assert_eq!(zeniths(setup, id).len(), 1, "{id}");
        }
        assert!(
            warning_about(
                &read,
                2,
                "word 21 '.' is not a sexagesimal angle: it has no digit; that value was not read"
            )
            .is_some(),
            "{}",
            all_warnings(&read)
        );
        assert!(
            warning_about(&read, 3, "word 21 '.' is not a number").is_some(),
            "{}",
            all_warnings(&read)
        );
    }

    // A letter anywhere in a sexagesimal word refuses it at its record, before
    // the seconds are looked at: a letter beside a 60 must not be carried into
    // an angle, nor a letter's place taken for a number.
    #[test]
    fn a_letter_in_any_field_of_a_sexagesimal_word_refuses_it_at_its_record() {
        //   record 2: in the degrees, "X4459600";
        //   record 3: in the minutes, beside seconds of 60, "044X6600";
        //   record 4: in the seconds, "04459A00";
        //   record 5: in the tenths, after seconds of 60, "0445960A".
        let read = read_ok(concat!(
            "110001+0000STN1 88..10+00001500 \r\n",
            "110002+000000P1 21.324+X4459600 22.324+09000000 \r\n",
            "110003+000000P2 21.324+044X6600 22.324+09000000 \r\n",
            "110004+000000P3 21.324+04459A00 22.324+09000000 \r\n",
            "110005+000000P4 21.324+0445960A 22.324+09000000 \r\n",
        ));
        let setup = &read.project.stations[0];
        let words: [(&str, usize); 4] = [("P1", 2), ("P2", 3), ("P3", 4), ("P4", 5)];
        for (id, record) in words {
            assert!(directions(setup, id).is_empty(), "{id}");
            assert_eq!(zeniths(setup, id).len(), 1, "{id}");
            assert!(
                warning_about(
                    &read,
                    record,
                    "is not a sexagesimal angle; that value was not read"
                )
                .is_some(),
                "{id}\n{}",
                all_warnings(&read)
            );
        }
        assert!(!all_warnings(&read).contains("60 seconds"));
    }

    // GSI right-justifies text and pads it with '0' (GET 11 and 41: "0000A110" is
    // point A110), so a code word of nothing but zeros is an empty one - as
    // Leica's Format Manager writes an unset code information word,
    // "43....+00000000" (Reference Guide V1.0, Annex 2) - not the code "0". Read
    // as "0", a job that records word 71 empty on every shot gives every point one
    // code and runs one feature through all of them.
    #[test]
    fn an_all_zero_remark_or_code_information_word_is_empty_not_the_code_zero() {
        let read = read_ok(concat!(
            "*110001+000000000000STN1 84..10+0000000000100000 85..10+0000000000200000 ",
            "88..10+0000000000001500 \r\n",
            "*110002+00000000000000P1 21.324+0000000000000000 22.324+0000000009000000 ",
            "71....+0000000000000000 \r\n",
            "*110003+00000000000000P2 21.324+0000000001000000 22.324+0000000009000000 ",
            "71....+0000000000000000 \r\n",
            "*110004+00000000000000P3 21.324+0000000002000000 22.324+0000000009000000 ",
            "71....+000000000000KERB \r\n",
            "*410005+000000000000TREE 42....+0000000000000000 43....+00000000000000A1 \r\n",
            "*110006+00000000000000P4 21.324+0000000003000000 22.324+0000000009000000 \r\n",
        ));
        let project = &read.project;
        for id in ["P1", "P2"] {
            let uncoded = unpositioned(project, id).expect(id);
            assert!(uncoded.code.is_empty(), "{id}: '{}'", uncoded.code);
        }
        assert_eq!(unpositioned(project, "P3").expect("P3").code, "KERB");
        // The code block (record 5) codes the point after it; its first
        // information word is empty, its second is A1.
        let p4 = unpositioned(project, "P4").expect("P4");
        assert_eq!(p4.code, "TREE");
        assert!(!p4.metadata.contains_key("code block information 1"));
        assert_eq!(
            p4.metadata
                .get("code block information 2")
                .map(String::as_str),
            Some("A1")
        );
        // P1 and P2 are uncoded, so no feature joins them: KERB and TREE only.
        assert_eq!(project.features.len(), 2);
        assert_eq!(project.features[0].code, "KERB");
        assert_eq!(project.features[1].code, "TREE");
        // Said once, at the first: the words 71 of records 2 and 3 and 42 of 5.
        let note = warning_about(&read, 2, "3 remark or code information words are all zeros")
            .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(note.message.contains("word 71"), "{}", note.message);
    }

    // The other side of that rule: a point's id (11) and a code block's code (41)
    // of nothing but zeros are "0". A point needs an id and a code block exists to
    // give a code, so neither is read as empty.
    #[test]
    fn an_all_zero_point_id_or_code_block_code_is_zero_not_empty() {
        // Record 1 names point "0" and keys it in at 100 E 200 N; record 2 is a
        // code block of code "0"; the file begins and ends with a point block, so
        // it reads Before Point and that code is P1's, the point after it.
        let read = read_ok(concat!(
            "110001+00000000 81..10+00100000 82..10+00200000 \r\n",
            "410002+00000000 \r\n",
            "110003+000000P1 81..10+00300000 82..10+00400000 \r\n",
        ));
        let zero = point(&read.project, "0").expect("0");
        double_eq(zero.easting, 100.0); // "81..10+00100000": unit 0, mm: 100.000 m
        double_eq(zero.northing, 200.0);
        let p1 = point(&read.project, "P1").expect("P1");
        assert_eq!(p1.code, "0");
        assert!(
            !all_warnings(&read).contains("all zeros"),
            "{}",
            all_warnings(&read)
        );
    }

    // Code information (42-49) comes in a code block as GSI ONLINE's PUT 41-49
    // give it, but "one may create any kind of GSI formats" (Leica's Format
    // Manager Reference Guide V1.0, Annex 2), and in a point's own block it can
    // only be about that point: it is kept in the point's metadata. A remark word
    // (71-79) in a code block could be the point's code and remarks or more about
    // the block's own code; it is not read, and the import says so rather than
    // dropping it without a word.
    #[test]
    fn code_information_in_a_points_own_block_is_kept_and_remarks_in_a_code_block_are_said_not_read()
     {
        let read = read_ok(concat!(
            "110001+000000P1 81..10+00100000 82..10+00200000 42....+0000CAT2 43....+00000000 \r\n",
            "410002+0000TREE 71....+00000OAK 72....+00000012 \r\n",
            "110003+000000P2 81..10+00300000 82..10+00400000 \r\n",
        ));
        let p1 = point(&read.project, "P1").expect("P1");
        assert_eq!(
            p1.metadata.get("code information 1").map(String::as_str),
            Some("CAT2")
        );
        assert!(!p1.metadata.contains_key("code information 2")); // all zeros: empty
        assert!(p1.code.is_empty(), "{}", p1.code);
        // The code block codes P2, the point after it; OAK and 12 went nowhere.
        let p2 = point(&read.project, "P2").expect("P2");
        assert_eq!(p2.code, "TREE");
        assert!(p2.metadata.is_empty());
        let note = warning_about(&read, 2, "2 remark words (71-79) are in a code block")
            .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(
            note.message.contains("(the first, word 71)"),
            "{}",
            note.message
        );
        assert!(
            note.message.contains("they were not read"),
            "{}",
            note.message
        );
    }

    // TargetInfo keeps the prism constant each distance was measured with, and
    // the setup the one it began with (data_model.rs). A shot whose word 51
    // states another is a changed prism or a wrong setting - in one file seen,
    // one of 15 distances from a setup to a mark was measured with +23 mm and is
    // 23 mm longer than the other 14 - so the import counts them and names the
    // first.
    #[test]
    fn distances_measured_with_another_prism_constant_than_their_setups_are_counted_and_keep_their_own()
     {
        // Word 51 "+0012+000" is 12 ppm and 0 mm; record 3's "+0012+023" is 23 mm.
        let read = read_ok(concat!(
            "110001+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n",
            "110002+000000BS 21.102+00000000 22.102+10000000 31..00+00100000 51..1.+0012+000 ",
            "87..10+00001500 \r\n",
            "110003+00000101 21.102+05000000 22.102+10000000 31..00+00020023 51..1.+0012+023 \r\n",
            "110004+00000102 21.102+06000000 22.102+10000000 31..00+00030000 51..1.+0012+000 \r\n",
        ));
        let setup = &read.project.stations[0];
        double_eq(
            setup.instrument.prism_constant.expect("a prism constant"),
            0.0,
        );
        let to101 = distances(setup, "101");
        let to102 = distances(setup, "102");
        assert_eq!(to101.len(), 1);
        assert_eq!(to102.len(), 1);
        double_eq(
            to101[0].target.prism_constant.expect("a prism constant"),
            0.023,
        );
        double_eq(
            to102[0].target.prism_constant.expect("a prism constant"),
            0.0,
        );
        let note = warning_about(
            &read,
            3,
            "1 distance was measured with a prism constant other than the one its setup",
        )
        .unwrap_or_else(|| panic!("{}", all_warnings(&read)));
        assert!(
            note.message
                .contains("0.023 m in setup STN1, which began with 0.0 m"),
            "{}",
            note.message
        );
    }

    // A monitoring pillar set up every half hour for a year is some 17,500 setups
    // on one point. Each gets its own id - STN1, STN1 (2), STN1 (3) ... - and
    // making the k-th must not cost more than making the first: the file of
    // setups on one point must read in about the time of the same file with
    // every setup on a different point. (A search for a free "(n)" from 2
    // upwards - 1 + 2 + ... + 2,999, some 4.5 million id lookups - read 3,000
    // setups on one point in 11.5 s and 3,000 on different points in 0.25 s,
    // Debug build.)
    #[test]
    fn thousands_of_setups_on_one_point_each_get_an_id_in_no_more_time_than_setups_on_different_points()
     {
        const SETUPS: usize = 3000;
        let job = |one_point: bool| {
            let mut text = String::new();
            for i in 1..=SETUPS {
                // 8 characters of GSI-8 data: STN1 padded, or 10000001 ... 10003000.
                let id = if one_point {
                    String::from("0000STN1")
                } else {
                    format!("{}", 10000000 + i)
                };
                text.push_str(&format!(
                    "110001+{id} 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \
                     \r\n110002+00000101 21.102+00000000 22.102+10000000 31..00+00010000 \r\n"
                ));
            }
            text
        };
        let one_point = job(true);
        let many_points = job(false);
        let reader = format_registry()
            .reader(LEICA_GSI_FORMAT_ID)
            .expect("a reader");

        let many_start = std::time::Instant::now();
        let many = reader(many_points.as_bytes(), "many.gsi", &ReadOptions::default());
        let one_start = std::time::Instant::now();
        let one = reader(one_point.as_bytes(), "one.gsi", &ReadOptions::default());
        let one_end = std::time::Instant::now();
        let many = many.unwrap_or_else(|error| panic!("{}", error.message));
        let one = one.unwrap_or_else(|error| panic!("{}", error.message));

        let stations = &one.project.stations;
        assert_eq!(stations.len(), SETUPS);
        assert_eq!(stations[0].setup.id, "STN1");
        assert_eq!(stations[1].setup.id, "STN1 (2)");
        assert_eq!(stations[SETUPS - 1].setup.id, "STN1 (3000)");
        let mut ids = std::collections::BTreeSet::new();
        for station in stations {
            assert_eq!(station.setup.point_id, "STN1");
            ids.insert(station.setup.id.clone());
        }
        assert_eq!(ids.len(), SETUPS); // no two alike
        assert_eq!(many.project.stations.len(), SETUPS);

        let many_seconds = (one_start - many_start).as_secs_f64();
        let one_seconds = (one_end - one_start).as_secs_f64();
        println!(
            "[ setups ] {SETUPS} on one point {one_seconds} s, on {SETUPS} points {many_seconds} s"
        );
        // Proportional reading gives about 1:1. The allowance of five times and
        // half a second is for a machine shared with other builds.
        assert!(one_seconds < 5.0 * many_seconds + 0.5);
    }

    #[test]
    fn a_shot_at_the_occupied_point_is_a_warning_not_an_invalid_observation() {
        let read = read_ok(concat!(
            "110001+0000STN1 88..10+00001500 \n",
            "110002+0000STN1 21.102+00000000 22.102+10000000 31..00+00010000 \n",
            "110003+00000002 21.102+10000000 22.102+10000000 31..00+00020000 \n",
        ));
        assert!(warning_about(&read, 2, "stands on").is_some());
        assert_eq!(read.project.stations[0].observations.len(), 3);
        assert_eq!(read.records_skipped, 1);
    }

    #[test]
    fn a_digital_levels_blocks_are_counted_not_read_as_total_station_shots() {
        // Page 40's example: "32...8+02505387 330.08+00125972" - a staff distance
        // and a staff reading, three-digit word 330 - after the DNA section's
        // special code block for line levelling BF, "410000+?......1".
        let read = read_ok(concat!(
            "410000+?......1 \r\n",
            "110001+0000A110 32...8+02505387 330.08+00125972 \r\n",
        ));
        assert!(read.project.stations.is_empty());
        let a110 = unpositioned(&read.project, "A110").expect("A110");
        assert!(
            warning_about(&read, 2, "levelling").is_some(),
            "{}",
            all_warnings(&read)
        );
        // The levelling method is not the point's code, and the import says so.
        assert!(a110.code.is_empty());
        assert!(
            warning_about(&read, 1, "special code block").is_some(),
            "{}",
            all_warnings(&read)
        );
    }

    #[test]
    fn carriage_return_only_and_line_feed_only_files_read_alike() {
        let lf = concat!(
            "110001+00000001 81..00+00001000 82..00+00002000 \n",
            "110002+00000002 81..00+00003000 82..00+00004000 \n",
        );
        let cr = lf.replace('\n', "\r");
        let a = read_text(lf.as_bytes()).expect("the LF file reads");
        let b = read_text(cr.as_bytes()).expect("the CR file reads");
        assert_eq!(a.project.points, b.project.points);
        assert_eq!(a.project.points.len(), 2);
    }

    #[test]
    fn a_file_with_no_gsi_block_is_an_error_not_an_empty_project() {
        for text in ["", "\r\n\r\n", "P,N,E,Z\n1,2,3,4\n"] {
            let error = read_text(text.as_bytes()).expect_err("no GSI block");
            assert_eq!(error.code, ErrorCode::ParseFailure);
            assert!(error.message.contains("word 11"));
        }
    }

    #[test]
    fn a_zero_default_precision_is_refused_because_every_observation_needs_a_sigma() {
        let options = ReadOptions {
            precision: ObservationPrecision {
                direction: 0.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let error = read_text_with(
            b"110001+00000001 81..00+00001000 82..00+00002000 \n",
            "job.gsi",
            &options,
        )
        .expect_err("a zero precision is refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}

// ---- Detection ------------------------------------------------------------------------

fn gsi_confidence(bytes: &[u8], name: &str) -> f64 {
    for result in format_registry().probe_all(&probe_of(bytes, name, false)) {
        if result.format_id == LEICA_GSI_FORMAT_ID {
            return result.signature.confidence;
        }
    }
    -1.0
}

// Small hand-written samples of the other formats Katana reads, so that the
// GSI probe is tested against them even before their own fixtures exist on
// this branch.
const OTHER_FORMATS: [(&str, &str); 8] = [
    (
        "job.jxl",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<JOBFile jobName=\"J\" \
         product=\"Trimble Access\" version=\"5.9\">\n<FieldBook>\n<PointRecord \
         ID=\"00000001\">\n<Name>1</Name>\n</PointRecord>\n</FieldBook>\n</JOBFile>\n",
    ),
    (
        "job.dc",
        "10NMJOB1            071118\r\n13TSTrimble Survey Controller\r\n\
         08KI1       5000.000     5000.000      100.000CP\r\n",
    ),
    (
        "job.rw5",
        "JB,NMJOB1,DT05-06-2024,TM10:15:30\nMO,AD0,UN0,SF1.00000000,EC1,EO0.0,AU0\n\
         OC,OP1,N 5000.000,E 5000.000,EL100.000,--STN\nBK,OP1,BP2,BS0.0000,BC0.0000\n\
         SS,OP1,FP3,AR90.0000,ZE90.0000,SD100.000,--TOPO\n",
    ),
    (
        "job.gt7",
        "JOB,MYJOB\nINST,GTS-700\nUNITS,M,D\nSTN,1,1.500,STN\nXYZ,5000,5000,100\n\
         BKB,2,0.0000,0.0000\nSS,3,1.600,TOPO\nSD,90.0000,90.0000,100.000\n",
    ),
    (
        "site0010.24o",
        "     3.04           OBSERVATION DATA    M                   RINEX VERSION \
         / TYPE\nsbf2rin-13.4.3                          20240305 101530 UTC PGM / \
         RUN BY / DATE\n                                                            \
         END OF HEADER\n",
    ),
    (
        "job.xml",
        "<?xml version=\"1.0\"?>\n<LandXML xmlns=\"http://www.landxml.org/schema/\
         LandXML-1.2\" version=\"1.2\">\n<CgPoints><CgPoint name=\"1\">5000 5000 \
         100</CgPoint></CgPoints>\n</LandXML>\n",
    ),
    (
        "points.csv",
        "P,N,E,Z,D\n1,6000.000,5000.000,100.000,IP\n2,6010.000,5010.000,101.000,IP\n",
    ),
    (
        "points.txt",
        "1 6000.000 5000.000 100.000 IP\n2 6010.000 5010.000 101.000 IP\n",
    ),
];

const GSI_FIXTURES: [&str; 9] = [
    "tps_gsi8.gsi",
    "tps_gsi16.gsi",
    "feet_mil_gsi8.gsi",
    "coordinates_gsi8.gsi",
    "codes_before_gsi8.gsi",
    "codes_after_gsi8.gsi",
    "rounded_seconds_gsi16.gsi",
    "negative_vertical_gsi8.gsi",
    "derived_distances_gsi8.gsi",
];

mod leica_gsi_detection {
    use super::*;

    #[test]
    fn every_gsi_fixture_is_identified_as_gsi_with_its_evidence() {
        for name in GSI_FIXTURES {
            let bytes = slurp(leica_folder().join(name));
            let detection = detect_format(&probe_of(&bytes, name, false));
            assert_eq!(
                detection.outcome(),
                DetectionOutcome::Identified,
                "{name}: {}",
                detection.summary()
            );
            assert_eq!(
                detection.format().expect("identified").id,
                LEICA_GSI_FORMAT_ID,
                "{name}"
            );
            assert!(detection.candidates()[0].evidence.contains("word 11 or 41"));
        }
        // Without its extension a GSI file is still GSI.
        let bytes = slurp(leica_folder().join("tps_gsi8.gsi"));
        let renamed = detect_format(&probe_of(&bytes, "export.txt", false));
        assert_eq!(
            renamed.outcome(),
            DetectionOutcome::Identified,
            "{}",
            renamed.summary()
        );
        assert_eq!(
            renamed.format().expect("identified").id,
            LEICA_GSI_FORMAT_ID
        );
    }

    #[test]
    fn the_gsi_probe_rules_out_every_other_formats_samples_and_fixtures() {
        for (name, text) in OTHER_FORMATS {
            assert_eq!(gsi_confidence(text.as_bytes(), name), 0.0, "{name}");
        }
        // Every fixture of every other format in this tree, whoever wrote it.
        let leica = leica_folder();
        let mut others = 0usize;
        for path in files_under(&data_folder()) {
            if path.parent() == Some(leica.as_path()) {
                continue; // our own GSI fixtures
            }
            let bytes = slurp(path.as_path());
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            assert_eq!(
                gsi_confidence(&bytes[..bytes.len().min(PROBE_BYTES)], &name),
                0.0,
                "{}",
                path.display()
            );
            others += 1;
        }
        assert!(others > 0);
    }
}

// ---- Robustness ------------------------------------------------------------------------

// Reads `bytes` and checks the one thing that must always hold: an answer -
// a project that passed validation, or an error with a message - and no crash.
fn reads_or_fails_cleanly(bytes: &[u8], what: &str) {
    if let Err(error) = read_text(bytes) {
        assert!(!error.message.is_empty(), "{what}");
        assert_ne!(error.code, ErrorCode::Internal, "{what}: {}", error.message);
    }
}

mod leica_gsi_robustness {
    use super::*;

    #[test]
    fn every_truncation_of_every_fixture_reads_or_fails_with_a_message() {
        let names = [
            "tps_gsi8.gsi",
            "tps_gsi16.gsi",
            "feet_mil_gsi8.gsi",
            "coordinates_gsi8.gsi",
            "damaged_gsi8.gsi",
            "codes_before_gsi8.gsi",
            "codes_after_gsi8.gsi",
            "rounded_seconds_gsi16.gsi",
            "negative_vertical_gsi8.gsi",
            "derived_distances_gsi8.gsi",
        ];
        for name in names {
            let bytes = slurp(leica_folder().join(name));
            assert!(!bytes.is_empty(), "{name}");
            for length in 0..=bytes.len() {
                reads_or_fails_cleanly(&bytes[..length], &format!("{name} cut at {length}"));
            }
        }
    }

    #[test]
    fn random_bytes_and_randomly_damaged_fixtures_read_or_fail_with_a_message() {
        let mut random = Random32::new(20260924); // fixed: a failure must be repeatable
        let base = slurp(leica_folder().join("tps_gsi8.gsi"));
        // Characters that matter to the grammar, so damage lands where it hurts.
        const GRAMMAR: &[u8] = b"0123456789+-.* \r\n\x1a";
        for round in 0..400usize {
            let mut noise = vec![0u8; round * 7 % 900];
            for c in &mut noise {
                *c = random.uniform(0, 255) as u8;
            }
            reads_or_fails_cleanly(&noise, &format!("noise round {round}"));

            let mut damaged = base.clone();
            for _ in 0..1 + round % 12 {
                let at = random.uniform(0, 255) as usize * damaged.len() / 256;
                damaged[at] = if round.is_multiple_of(2) {
                    random.uniform(0, 255) as u8
                } else {
                    GRAMMAR[random.uniform(0, 255) as usize % GRAMMAR.len()]
                };
            }
            reads_or_fails_cleanly(&damaged, &format!("damaged round {round}"));
        }
    }
}

// ---- Throughput ------------------------------------------------------------------------

fn field16(value: u64) -> String {
    let text = format!("{value}");
    format!("{}{text}", "0".repeat(16 - text.len().min(16)))
}

// A GSI-16 job of `bytes` or a little more: a setup every 100 shots, each shot
// Hz, V, slope distance and reflector height - the common Leica recording mask.
fn synthetic_job(bytes: usize) -> String {
    let mut text = String::with_capacity(bytes + 512);
    let mut block: u64 = 0;
    let mut shot: u64 = 0;
    while text.len() < bytes {
        if shot.is_multiple_of(100) {
            block += 1;
            text.push_str(&format!(
                "*11{}+{} 84..16+{} 85..16+{} 86..16+{} 88..16+{} \r\n",
                &field16(block)[12..],
                field16(1000000 + shot / 100),
                field16(50000000 + shot),
                field16(60000000 + shot),
                field16(1000000),
                field16(15500)
            ));
        }
        shot += 1;
        block += 1;
        text.push_str(&format!(
            "*11{}+{} 21.102+{} 22.102+{} 31..16+{} 87..16+{} \r\n",
            &field16(block)[12..],
            field16(shot),
            field16((shot * 7919) % 40000000),
            field16(9000000 + (shot * 31) % 2000000),
            field16(100000 + (shot * 131) % 5000000),
            field16(17000)
        ));
    }
    text
}

mod leica_gsi_throughput {
    use super::*;

    #[test]
    fn a_synthetic_job_is_read_and_its_speed_reported() {
        // KATANA_GSI_THROUGHPUT_MB sets the size (50 for the measurement the
        // parser's performance is judged by); the default keeps the suite quick.
        let mut megabytes = 4usize;
        if let Ok(setting) = std::env::var("KATANA_GSI_THROUGHPUT_MB") {
            megabytes = atoi(&setting).max(1) as usize;
        }
        let job = synthetic_job(megabytes * 1000 * 1000);
        let reader = format_registry()
            .reader(LEICA_GSI_FORMAT_ID)
            .expect("a reader");

        let start = std::time::Instant::now();
        let parsed = reader(job.as_bytes(), "synthetic.gsi", &ReadOptions::default());
        let parsed_at = std::time::Instant::now();
        let parsed = parsed.unwrap_or_else(|error| panic!("{}", error.message));
        let checked = read_text_with(job.as_bytes(), "synthetic.gsi", &ReadOptions::default());
        let checked_at = std::time::Instant::now();
        checked.unwrap_or_else(|error| panic!("{}", error.message));

        let seconds = (parsed_at - start).as_secs_f64();
        let with_check = (checked_at - parsed_at).as_secs_f64();
        let size = job.len() as f64 / 1e6;
        let observations: usize = parsed
            .project
            .stations
            .iter()
            .map(|station| station.observations.len())
            .sum();
        assert_eq!(parsed.warnings.len(), 0, "{}", all_warnings(&parsed));
        assert!(observations > 0);
        println!(
            "[ throughput ] leica-gsi: {size} MB, {} blocks, {observations} observations; reader \
             {seconds} s = {} MB/s; read_survey (with validate_project) {with_check} s = {} MB/s",
            parsed.records_read,
            size / seconds,
            size / with_check
        );
        println!(
            "megabytes_per_second={}",
            katana_core::text::format_exact_real(size / seconds)
        );
    }
}
