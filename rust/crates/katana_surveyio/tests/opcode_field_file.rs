//! Twin of tests/surveyio/test_opcode_field_file.cpp: the opcode field file
//! reader (src/opcode_field_file.rs).
//!
//! The fixtures are hand-built from the format's published description ("Field
//! File Format", sections 44.2 "Structure of the .fld File", 44.3 "Point
//! Description" and 44.8, one entry per opcode), in the layout of the field
//! files the reader was written against: a column after every opcode, CRLF line
//! ends. None holds a real job.
//!
//! ```text
//! data/fld/setup.fld      a total-station setup: 02, 03, 04, 05, 06, 07,
//!                         09, 29, 41, 72, 73, 100, one record a value short
//!                         and one opcode the reader does not read (18).
//! data/fld/gnss.fld       an RTK job: coordinates as " 2" (a blank before the
//!                         opcode) with a date and time in the column, each
//!                         with its receiver's "GNSS Solution" in a 124 ... 125
//!                         attribute group, a keyed-in 02, a bare 20, a mark
//!                         measured again, comments; Windows-1252, one 0xB0
//!                         degree sign.
//! data/fld/resection.fld  a resection (128 ... 129, with "5" and "7" written
//!                         without their zero), its residuals as comments, 16,
//!                         42, 43, 71, a coded check, then a 03 setup; UTF-8.
//! data/fld/rtk_setup.fld  two RTK marks, a setup on one backsighting the
//!                         other, and two shots with an offset each.
//! data/fld/two_backsights.fld  a setup backsighting an entered mark and then
//!                         a point nothing places, and a shot (the field file
//!                         cli tests draw it).
//! ```
//!
//! Every expected value below is worked from the record text: circle readings
//! are decimal degrees, so 45.00000000 is pi / 4.

mod opcode_support;
mod support;

use opcode_support as topcon_test;

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use katana_core::ErrorCode;
use katana_survey::{
    AngularUnit, ComputationMethod, ComputedPoint, CoordinateSource, CorrectionState, DistanceKind,
    DistanceObservation, Face, GnssPositionObservation, HorizontalDirectionObservation, LinearUnit,
    Observation, ObservationPrecision, ReductionContext, ReductionOutcome, ReductionReport,
    ReductionSettings, SurveyFeature, SurveyProject, ZenithAngleObservation, reduce_and_adjust,
};
use katana_surveyio::{
    DetectionOutcome, PROBE_BYTES, ReadResult, detect_format, format_registry, probe_of, to_string,
};

use support::{double_eq, near};
use topcon_test::{fixture, observations_to};

const ID: &str = "opcode-field-file";
const ANGLE_TOLERANCE: f64 = 1e-12;
// Plan positions through the reduction: its arithmetic on six-figure
// coordinates rounds at 1e-12 m, far inside this.
const POSITION_TOLERANCE: f64 = 1e-6;

fn degrees(value: f64) -> f64 {
    value * PI / 180.0
}

fn read_fixture(name: &str) -> ReadResult {
    let bytes = fixture(Path::new("fld").join(name));
    topcon_test::read(ID, &bytes, name).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn read_setup() -> ReadResult {
    read_fixture("setup.fld")
}

fn warned(result: &ReadResult, record: usize, words: &str) -> bool {
    result
        .warnings
        .iter()
        .any(|warning| warning.record == record && warning.message.contains(words))
}

// A file of tab-separated records, written line by line so a tab is seen.
fn lines<S: AsRef<str>>(rows: &[S]) -> String {
    let mut text = String::new();
    for row in rows {
        text.push_str(row.as_ref());
        text.push('\n');
    }
    text
}

fn read_lines<S: AsRef<str>>(rows: &[S]) -> ReadResult {
    topcon_test::read(ID, lines(rows).as_bytes(), "inline.fld")
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

// The metadata of a point, positioned or not; empty when there is no such point.
fn metadata_of(project: &SurveyProject, id: &str) -> BTreeMap<String, String> {
    if let Some(point) = topcon_test::point(project, id) {
        return point.metadata.clone();
    }
    if let Some(point) = topcon_test::unpositioned(project, id) {
        return point.metadata.clone();
    }
    BTreeMap::new()
}

fn value_of(metadata: &BTreeMap<String, String>, key: &str) -> String {
    metadata
        .get(key)
        .cloned()
        .unwrap_or_else(|| String::from("(absent)"))
}

// The features of `code`, in file order.
fn features_of<'p>(project: &'p SurveyProject, code: &str) -> Vec<&'p SurveyFeature> {
    project
        .features
        .iter()
        .filter(|feature| feature.code == code)
        .collect()
}

fn field_file_confidence(bytes: &[u8], name: &str) -> f64 {
    for probed in format_registry().probe_all(&probe_of(bytes, name, false)) {
        if probed.format_id == ID {
            return probed.signature.confidence;
        }
    }
    -1.0
}

// The GNSS positions a read job gives `id`, in file order.
fn gnss_positions_of(project: &SurveyProject, id: &str) -> Vec<GnssPositionObservation> {
    project
        .observations
        .iter()
        .filter_map(|observation| match observation {
            Observation::GnssPosition(position) if position.point == id => Some(position.clone()),
            _ => None,
        })
        .collect()
}

// The reduction of a read job: its defaults, without curvature and refraction,
// so that a position is the plan geometry a test works by hand.
fn reduced(project: &SurveyProject) -> ReductionOutcome {
    let settings = ReductionSettings {
        curvature_and_refraction: false,
        ..Default::default()
    };
    reduce_and_adjust(project, &settings, &ReductionContext::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

// The positions the reduction gives a read job, by point id.
fn reduced_positions(project: &SurveyProject) -> BTreeMap<String, ComputedPoint> {
    reduced(project)
        .points
        .into_iter()
        .map(|point| (point.id.clone(), point))
        .collect()
}

// How many of the reduction's warnings hold `words`.
fn reduction_warnings_with(report: &ReductionReport, words: &str) -> usize {
    report
        .warnings
        .iter()
        .filter(|warning| warning.text.contains(words))
        .count()
}

// Every regular file under `folder`, at any depth.
fn files_under(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

mod opcode_field_file {
    use super::*;

    // ---- Registration and detection --------------------------------------------------------

    #[test]
    fn is_registered_as_an_importable_format_reading_setups_shots_and_points() {
        let format = format_registry().find(ID).expect("registered");
        assert!(format.can_import);
        assert_eq!(format.extensions, ["fld"]);
        assert!(
            format.reads.points
                && format.reads.observations
                && format.reads.stations
                && format.reads.features
                && format.reads.coordinate_system
        );
        // An RTK position written as an 02 with its receiver's solution is a
        // GNSS position.
        assert!(format.reads.gnss);
        assert!(format_registry().reader(ID).is_some());
    }

    #[test]
    fn a_file_with_its_version_line_and_opcodes_is_identified() {
        let bytes = fixture("fld/setup.fld");
        let detection = detect_format(&probe_of(&bytes, "setup.fld", false));
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        assert_eq!(detection.format().unwrap().id, ID);
        assert!(detection.candidates()[0].evidence.contains("{Version"));
    }

    #[test]
    fn a_gnss_job_of_blank_padded_opcodes_and_bare_records_is_identified() {
        // gnss.fld's 34 records: five " 2" (a blank before the opcode), one
        // "02", one bare "20" and the groups and attributes. Each is a record,
        // and the six coordinates are what makes it a survey.
        let bytes = fixture("fld/gnss.fld");
        let detection = detect_format(&probe_of(&bytes, "gnss.fld", false));
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        assert_eq!(detection.format().unwrap().id, ID);
        assert!(
            detection.candidates()[0]
                .evidence
                .contains("34 of 34 records"),
            "{}",
            detection.candidates()[0].evidence
        );
    }

    #[test]
    fn another_format_named_fld_is_not_claimed() {
        // Comma-separated text with no numeric opcode and tab is some other
        // program's .fld: ruled out, not read.
        let other = "PT,1,1000.0,5000.0\nPT,2,1001.0,5001.0\n";
        for probed in format_registry().probe_all(&probe_of(other.as_bytes(), "job.fld", false)) {
            if probed.format_id == ID {
                assert_eq!(
                    probed.signature.confidence, 0.0,
                    "{}",
                    probed.signature.evidence
                );
            }
        }
    }

    #[test]
    fn a_numbered_list_of_points_is_not_taken_for_a_field_file() {
        // Tab-separated rows numbered 1 to 60, right-aligned: each opens with a
        // number, and rows 2, 3 and 7 with a coordinate's, setup's or shot's
        // opcode - but with five fields, where those records have nine or ten
        // (seven or eight for a setup).
        let mut rows = String::new();
        let mut numbers = String::new();
        for row in 1..=60 {
            let number = format!("{row}");
            rows.push_str(&format!(
                "{}{number}\t{}.500\t{}.250\t50.010\tTREE\r\n",
                " ".repeat(4 - number.len()),
                300000 + row,
                6250000 + row
            ));
            numbers.push_str(&format!("{number}\r\n"));
        }
        assert_eq!(field_file_confidence(rows.as_bytes(), "points.txt"), 0.0);
        assert_eq!(
            field_file_confidence(numbers.as_bytes(), "numbers.txt"),
            0.0
        );
    }

    #[test]
    fn a_field_file_behind_a_long_header_of_comments_is_identified() {
        // 250 comment lines, past the 200 lines the probe once looked at, then
        // a job: comments are no records, and the records decide.
        let mut bytes = String::from("{Version 6.0}\r\n");
        for line in 0..250 {
            bytes.push_str(&format!("// header line {line}\r\n"));
        }
        bytes.push_str(concat!(
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0\r\n",
            "03\t\t\t\tK\t\t\t1.5\r\n",
            "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0\r\n",
        ));
        let detection = detect_format(&probe_of(bytes.as_bytes(), "long_header.fld", false));
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        assert_eq!(detection.format().unwrap().id, ID);
    }

    // Every file under tests/surveyio/data, whichever format wrote it: this
    // probe rules out each that is not a .fld, and every .fld is identified as
    // this format. The table printed ("detect <file> ...") is what a change to
    // a probe is compared by, before and after.
    #[test]
    fn the_probe_claims_no_other_formats_fixture_or_sample() {
        let root = topcon_test::data_directory();
        let mut files = Vec::new();
        files_under(&root, &mut files);
        files.sort();
        let mut others = 0usize;
        for path in &files {
            let relative = path.strip_prefix(&root).unwrap();
            let bytes = fixture(relative);
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let truncated = bytes.len() > PROBE_BYTES;
            let probed = if truncated {
                &bytes[..PROBE_BYTES]
            } else {
                &bytes[..]
            };
            let detection = detect_format(&probe_of(probed, &name, truncated));
            let mut candidates = String::new();
            for candidate in detection.candidates() {
                if !candidates.is_empty() {
                    candidates.push(',');
                }
                candidates.push_str(&format!(
                    "{}:{}",
                    candidate.format_id,
                    (candidate.confidence * 100.0 + 0.5) as i32
                ));
            }
            println!(
                "detect {} outcome={} format={} candidates={}",
                relative.to_string_lossy().replace('\\', "/"),
                to_string(detection.outcome()),
                detection
                    .format()
                    .map_or_else(|_| String::from("-"), |format| format.id),
                if candidates.is_empty() {
                    "-"
                } else {
                    candidates.as_str()
                }
            );
            if path.extension().is_some_and(|extension| extension == "fld") {
                assert_eq!(
                    detection.outcome(),
                    DetectionOutcome::Identified,
                    "{name}: {}",
                    detection.summary()
                );
                if let Ok(format) = detection.format() {
                    assert_eq!(format.id, ID, "{name}");
                }
            } else {
                assert_eq!(
                    field_file_confidence(probed, &name),
                    0.0,
                    "{}",
                    path.display()
                );
                others += 1;
            }
        }
        assert!(others >= 20, "every other format's fixtures");
        for sample in &topcon_test::FOREIGN_SAMPLES {
            assert_eq!(
                field_file_confidence(sample.bytes.as_bytes(), sample.name),
                0.0,
                "{}",
                sample.name
            );
        }
    }

    // ---- The total-station fixture ---------------------------------------------------------

    #[test]
    fn entered_coordinates_are_positioned_points_easting_first() {
        let result = read_setup();
        let project = &result.project;
        let cp1 = topcon_test::point(project, "CP1").expect("CP1");
        double_eq(cp1.easting, 1000.0);
        double_eq(cp1.northing, 5000.0);
        double_eq(cp1.elevation.expect("a height"), 20.0);
        assert_eq!(cp1.coordinate_source, CoordinateSource::Entered);
        assert_eq!(cp1.source.record_number, 8);

        // Record 9 gives point ID 5 AND point name CP2: the name is the point.
        let cp2 = topcon_test::point(project, "CP2").expect("CP2");
        double_eq(cp2.northing, 5100.0);
        assert_eq!(cp2.metadata["point ID"], "5");
        assert!(topcon_test::point(project, "5").is_none());
        assert_eq!(project.points.len(), 2);
    }

    #[test]
    fn a_station_its_backsight_and_its_shots_become_one_setups_observations() {
        let result = read_setup();
        let project = &result.project;
        assert_eq!(project.stations.len(), 1);
        let station = &project.stations[0];
        assert_eq!(station.setup.point_id, "CP1");
        double_eq(station.setup.instrument_height, 1.5);
        assert_eq!(station.backsight_point_id, "CP2");
        near(
            station.backsight_azimuth.expect("a backsight circle"),
            0.0,
            ANGLE_TOLERANCE,
        );
        // Four pointings - backsight, 101, 102, the check on CP2 - of a
        // direction, a zenith and a slope distance each.
        assert_eq!(station.observations.len(), 12);

        // To CP2: the backsight (record 13) - 0, 90 degrees, 100 m, under
        // target height 1.600 - and the check measurement (record 22) - 0.001
        // degrees, 100.002 m, under the target height record 14 set, 2.000.
        let directions = observations_to::<HorizontalDirectionObservation>(station, "CP2");
        assert_eq!(directions.len(), 2);
        near(directions[0].direction, 0.0, ANGLE_TOLERANCE);
        near(directions[1].direction, degrees(0.001), ANGLE_TOLERANCE);
        let zeniths = observations_to::<ZenithAngleObservation>(station, "CP2");
        assert_eq!(zeniths.len(), 2);
        near(zeniths[0].angle, PI / 2.0, ANGLE_TOLERANCE);
        double_eq(zeniths[0].target_height, 1.6);
        let distances = observations_to::<DistanceObservation>(station, "CP2");
        assert_eq!(distances.len(), 2);
        double_eq(distances[0].distance, 100.0);
        assert_eq!(distances[0].kind, DistanceKind::Slope);
        double_eq(distances[0].instrument_height, 1.5);
        double_eq(distances[0].target_height, 1.6);
        double_eq(distances[1].distance, 100.002);
        double_eq(distances[1].target_height, 2.0);
        // CP2 was made by record 9: the check of record 22 is that record's.
        assert_eq!(
            topcon_test::point(project, "CP2").unwrap().metadata["record 22/check measurement"],
            "from setup CP1"
        );
    }

    #[test]
    fn a_zenith_past_one_eighty_is_face_right_held_as_its_face_left_equivalent() {
        let result = read_setup();
        let station = &result.project.stations[0];
        // 101: 45, 95 degrees - face left, as read.
        let direction101 = observations_to::<HorizontalDirectionObservation>(station, "101");
        assert_eq!(direction101.len(), 1);
        near(direction101[0].direction, PI / 4.0, ANGLE_TOLERANCE);
        let zenith101 = observations_to::<ZenithAngleObservation>(station, "101");
        assert_eq!(zenith101.len(), 1);
        near(zenith101[0].angle, degrees(95.0), ANGLE_TOLERANCE);
        assert_eq!(zenith101[0].pointing.face, Face::Left);
        // 102: 275 degrees is face right, the same line of sight as 85.
        let zenith102 = observations_to::<ZenithAngleObservation>(station, "102");
        assert_eq!(zenith102.len(), 1);
        near(zenith102[0].angle, degrees(85.0), ANGLE_TOLERANCE);
        assert_eq!(zenith102[0].pointing.face, Face::Right);
        let distance102 = observations_to::<DistanceObservation>(station, "102");
        assert_eq!(distance102.len(), 1);
        double_eq(distance102[0].distance, 30.0);
    }

    #[test]
    fn shots_are_unpositioned_points_strung_into_their_feature_with_their_attributes() {
        let result = read_setup();
        let project = &result.project;
        let p101 = topcon_test::unpositioned(project, "101").expect("101");
        let p102 = topcon_test::unpositioned(project, "102").expect("102");
        assert_eq!(p101.code, "EB");
        assert_eq!(p102.description, "kerb");
        assert_eq!(p101.metadata["Date"], "2026-08-26");
        assert_eq!(p101.metadata["Depth"], "0.6");
        assert_eq!(p101.metadata["additional text 18"], "Modified by hand");
        let eb = topcon_test::feature(project, "EB").expect("EB");
        assert_eq!(eb.point_ids, ["101", "102"]);
    }

    #[test]
    fn the_header_units_and_declared_system_are_kept_and_nothing_is_guessed() {
        let result = read_setup();
        let project = &result.project;
        assert_eq!(project.units.angular, AngularUnit::DecimalDegrees);
        assert_eq!(project.units.linear, LinearUnit::Metres);
        assert_eq!(project.metadata["version"], "{Version 6.0}");
        assert_eq!(project.metadata["header: Job name"], "fixture");
        // "// Field File Version 6" is no "Key : value": it is kept as written.
        assert_eq!(project.metadata["header"], "Field File Version 6");
        assert_eq!(
            project.metadata["notes"],
            "Hand-built from the format description"
        );
        // Declared by name, as the file spells it; no EPSG code is invented.
        assert_eq!(project.coordinate_system.name, "Australia/GDA2020, Zone 56");
        assert_eq!(project.coordinate_system.epsg_code, 0);
    }

    #[test]
    fn a_record_short_of_a_value_and_an_opcode_not_read_are_skipped_by_name() {
        let result = read_setup();
        // Read: the version line, 100, 29, two 02, 09, 03, two 05, 04, two 07,
        // 73, 72, 41, 06 - sixteen. Skipped: record 20 (a 07 one value short)
        // and record 21 (opcode 18, a circle feature, which the reader does not
        // import).
        assert_eq!(result.records_read, 16);
        assert_eq!(result.records_skipped, 2);
        assert!(warned(&result, 20, "which value is which cannot be told"));
        assert!(warned(
            &result,
            21,
            "opcode 18 (circle feature) is not one this reader imports"
        ));
        assert!(topcon_test::unpositioned(&result.project, "103").is_none());
    }

    #[test]
    fn a_unit_other_than_degrees_and_metres_is_refused_not_guessed() {
        let bytes = lines(&["100\t\tgons\tmetres", "02\t\t\t\tA\t\t\t10.0\t20.0\t1.0"]);
        let error = topcon_test::read(ID, bytes.as_bytes(), "gons.fld").expect_err("refused");
        assert_eq!(error.code, ErrorCode::FileImportFailure);
        assert!(error.message.contains("'gons'"), "{}", error.message);
        assert!(error.message.contains("record 1 "), "{}", error.message);
    }

    #[test]
    fn a_shot_before_any_station_and_a_value_that_is_not_a_number_are_warned_of() {
        let bytes = lines(&[
            "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEB\t01\t2\t\t\t10.0\tnan\t5.0",
        ]);
        let read = topcon_test::read(ID, bytes.as_bytes(), "early.fld")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        // A setup is a 03 or, since the reader reads resections, a 128.
        assert!(warned(
            &read,
            1,
            "with no setup (opcode 03 or 128) before it"
        ));
        assert!(warned(&read, 3, "vertical circle 'nan' is not a number"));
        // Record 3 still has its direction and distance; it has no zenith.
        let station = &read.project.stations[0];
        assert_eq!(
            observations_to::<HorizontalDirectionObservation>(station, "2").len(),
            1
        );
        assert_eq!(
            observations_to::<ZenithAngleObservation>(station, "2").len(),
            0
        );
        assert_eq!(
            observations_to::<DistanceObservation>(station, "2").len(),
            1
        );
    }

    #[test]
    fn a_file_with_nothing_to_read_is_an_error_not_an_empty_survey() {
        for bytes in [
            String::new(),
            String::from("{Version 6.0}\r\n"),
            lines(&["29\t\tonly a note"]),
        ] {
            let error = topcon_test::read(ID, bytes.as_bytes(), "empty.fld").expect_err("refused");
            assert_eq!(
                error.code,
                ErrorCode::FileImportFailure,
                "{}",
                error.describe()
            );
        }
    }

    // ---- The RTK fixture -------------------------------------------------------------------

    #[test]
    fn the_whole_gnss_job_is_read_with_nothing_skipped() {
        let result = read_fixture("gnss.fld");
        // The version line and the 34 records of lines 11-37 and 41-47; the
        // blank lines and comments are not records.
        assert_eq!(result.records_read, 35);
        assert_eq!(result.records_skipped, 0);
        // The file is not UTF-8 (record 0), and R001's second GNSS position, of
        // record 41, is 4 mm, 3 mm and 5 mm from where record 11 put it.
        assert_eq!(result.warnings.len(), 2);
        assert!(warned(&result, 0, "the file is not UTF-8"));
        assert!(warned(
            &result,
            41,
            "point 'R001' is given a GNSS position here (N 6200020.003, E 500010.004, elevation \
             12.505 m) that differs from the GNSS position of record 11"
        ));
        let project = &result.project;
        assert!(project.stations.is_empty());
        // CM1 is keyed in; R001 to R004 are the receiver's, placed by their
        // five GNSS positions, R001's two among them.
        assert_eq!(project.points.len(), 1);
        assert_eq!(project.points[0].id, "CM1");
        assert_eq!(project.unpositioned_points.len(), 4);
        assert_eq!(project.observations.len(), 5);
        assert_eq!(project.coordinate_system.name, "Local grid A");
        assert_eq!(project.metadata["header: Ellipsoid"], "GRS 1980");
        // Line 9 comes before the first coordinate, so it is the header's;
        // line 39, after it, is a note.
        assert_eq!(
            project.metadata["header"],
            "Field file of an RTK job, hand-built for the tests\nAntenna Height Change 1.800"
        );
        assert_eq!(project.metadata["notes"], "Antenna Height Change 2.000");
    }

    #[test]
    fn an_rtk_position_with_its_receivers_solution_is_a_gnss_position_and_a_keyed_in_one_is_entered()
     {
        let result = read_fixture("gnss.fld");
        let project = &result.project;
        // R001, record 11: X the easting, Y the northing, Z the mark's height,
        // with the a-priori GNSS precision the read was given (the defaults).
        let r001 = gnss_positions_of(project, "R001");
        assert_eq!(r001.len(), 2);
        double_eq(r001[0].easting, 500010.0);
        double_eq(r001[0].northing, 6200020.0);
        double_eq(r001[0].elevation, 12.5);
        let defaults = ObservationPrecision::default();
        assert_eq!(r001[0].sigma_northing, defaults.gnss_horizontal);
        assert_eq!(r001[0].sigma_easting, defaults.gnss_horizontal);
        assert_eq!(r001[0].sigma_elevation, defaults.gnss_vertical);
        assert_eq!(r001[0].source.record_number, 11);
        // Its measurement of record 41 is a position of its own.
        double_eq(r001[1].easting, 500010.004);
        double_eq(r001[1].northing, 6200020.003);
        assert_eq!(r001[1].source.record_number, 41);
        // Each of R001 to R004 names its solution, R004's in a group of
        // another name: none has coordinates of its own, each a GNSS position.
        for id in ["R001", "R002", "R003", "R004"] {
            assert!(topcon_test::point(project, id).is_none(), "{id}");
            assert!(topcon_test::unpositioned(project, id).is_some(), "{id}");
        }
        assert_eq!(gnss_positions_of(project, "R004").len(), 1);
        // CM1 names none: the format's directly entered coordinate.
        let cm1 = topcon_test::point(project, "CM1").expect("CM1");
        assert_eq!(cm1.coordinate_source, CoordinateSource::Entered);
        assert!(gnss_positions_of(project, "CM1").is_empty());
        // The attributes are kept as the receiver wrote them.
        assert_eq!(
            value_of(
                &metadata_of(project, "R001"),
                "GPS Information/GNSS Solution"
            ),
            "fixed"
        );
        assert_eq!(
            value_of(
                &metadata_of(project, "R002"),
                "GPS Information/GNSS Solution"
            ),
            "float"
        );
        assert!(project.control_points.is_empty());
    }

    #[test]
    fn the_reduction_holds_no_rtk_mark_as_control_and_checks_a_marks_second_position() {
        let outcome = reduced(&read_fixture("gnss.fld").project);
        let mut points: BTreeMap<String, ComputedPoint> = BTreeMap::new();
        for point in &outcome.points {
            points.insert(point.id.clone(), point.clone());
        }
        assert_eq!(points.len(), 5);
        // Where the file puts them: R001 by its first position, by GNSS; CM1,
        // keyed in, as the entered coordinate it is.
        assert_eq!(points["R001"].method, ComputationMethod::Gnss);
        double_eq(points["R001"].easting, 500010.0);
        double_eq(points["R001"].northing, 6200020.0);
        assert_eq!(points["R004"].method, ComputationMethod::Gnss);
        assert_eq!(points["CM1"].method, ComputationMethod::Control);
        // R001's second position checks its first: 6200020.003 - 6200020,
        // 500010.004 - 500010 and 12.505 - 12.5, to the double's rounding of
        // seven-figure values (under 1e-9 m; 1e-8 allowed).
        assert_eq!(outcome.report.misclosures.len(), 1);
        let check = &outcome.report.misclosures[0];
        assert!(
            check
                .name
                .contains("R001 by a second GNSS position (record 41)"),
            "{}",
            check.name
        );
        let (Some(northing), Some(easting), Some(height)) =
            (check.northing, check.easting, check.height)
        else {
            panic!("the check has its three components");
        };
        near(northing, 0.003, 1e-8);
        near(easting, 0.004, 1e-8);
        near(height, 0.005, 1e-8);
        // Drawn as calculated - by the receiver - not as entered control.
        let r001 = topcon_test::point(&outcome.reduced, "R001").expect("R001");
        assert_eq!(r001.coordinate_source, CoordinateSource::Calculated);
        assert_eq!(
            value_of(&r001.metadata, "GPS Information/GNSS Solution"),
            "fixed"
        );
    }

    #[test]
    fn only_the_receivers_solution_among_an_o2s_own_attributes_makes_it_a_gnss_position() {
        let result = read_lines(&[
            " 2\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0", // no attributes
            " 2\t\t\t\tB\t\t\t1010.0\t5000.0\t20.0",
            "124\t\t\tGPS Information\t0",
            "73\t\tAntenna Height\t2.000", // a receiver's group, but no solution
            "125\t\t\t\t0",
            " 2\t\t\t\tC\t\t\t1020.0\t5000.0\t20.0",
            "73\t\tgnss solution\tfixed", // in no group, in another case
            " 2\t\t\t\tD\t\t\t1030.0\t5000.0\t20.0",
            "03\t\t\t\tD\t\t\t1.5",
            // After a setup: not among the 02's own records, so D stays
            // entered (the attribute is still the current measurement point's,
            // as [FLD] gives 73 to it).
            "73\t\tGNSS Solution\tfixed",
            " 2\t\t\t\tE\t\t\t1040.0\t5000.0\t", // no Z
            "73\t\tGNSS Solution\tfixed",
        ]);
        for id in ["A", "B", "D", "E"] {
            let point = topcon_test::point(&result.project, id).expect(id);
            assert_eq!(point.coordinate_source, CoordinateSource::Entered, "{id}");
            assert!(gnss_positions_of(&result.project, id).is_empty(), "{id}");
        }
        assert!(topcon_test::point(&result.project, "C").is_none());
        assert_eq!(gnss_positions_of(&result.project, "C").len(), 1);
        assert!(warned(
            &result,
            11,
            "point 'E' is a receiver's solution (its 'GNSS Solution' attribute) with no height, \
             which a GNSS position has; it is kept as an entered coordinate"
        ));
        assert_eq!(result.records_skipped, 0);
    }

    #[test]
    fn an_entered_coordinate_after_a_gnss_position_of_its_point_keeps_the_gnss_position_beside_it()
    {
        // A mark measured by RTK, then keyed in 50 mm away in plan and height.
        // The entered coordinate is what the point holds, as in the other
        // order; what goes beside it is the position it does not hold, the
        // GNSS one. Before, the metadata called the held coordinates
        // "restated", and the GNSS position's values were nowhere on the point.
        let result = read_lines(&[
            " 2\t14/03/26 10:00:00\t\t\tA\t\t\t1000.05\t5000.05\t20.05",
            "73\t\tGNSS Solution\tfixed",
            "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
        ]);
        let a = topcon_test::point(&result.project, "A").expect("A");
        assert_eq!(a.coordinate_source, CoordinateSource::Entered);
        double_eq(a.easting, 1000.0);
        double_eq(a.northing, 5000.0);
        assert_eq!(
            value_of(&a.metadata, "GNSS position of record 1"),
            "N 5000.05, E 1000.05, elevation 20.05"
        );
        assert_eq!(
            value_of(&a.metadata, "coordinates restated at record 3"),
            "(absent)"
        );
        assert_eq!(gnss_positions_of(&result.project, "A").len(), 1);
        assert!(warned(
            &result,
            3,
            "point 'A' is given an entered coordinate here (N 5000, E 1000, elevation 20 m) that \
             differs from the GNSS position of record 1; the reduction holds the entered one, and \
             the other is kept beside it"
        ));
    }

    #[test]
    fn a_time_stamp_in_the_column_after_the_opcode_is_kept_on_the_point_it_dates() {
        let result = read_fixture("gnss.fld");
        let r002 = metadata_of(&result.project, "R002");
        assert_eq!(value_of(&r002, "time stamp"), "14/03/26 09:15:40.10");
        // CM1's column is empty: no time stamp.
        assert_eq!(
            value_of(&metadata_of(&result.project, "CM1"), "time stamp"),
            "(absent)"
        );

        // [FLD] 44.6's time_text in a total-station file dates the setup and
        // the shot; on an attribute it is not kept, which is said once.
        let w3c = read_lines(&[
            "03\t2015-09-28T06:40:00Z\t\t\tS\t\t\t1.5",
            "07\t2015-09-28T06:42:45Z\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
            "73\t2015-09-28T06:42:46Z\tDate\t2015-09-28",
            "73\t2015-09-28T06:42:47Z\tTime\t06:42:45",
        ]);
        assert_eq!(w3c.project.stations.len(), 1);
        assert_eq!(
            value_of(&w3c.project.stations[0].metadata, "time stamp"),
            "2015-09-28T06:40:00Z"
        );
        let shot = metadata_of(&w3c.project, "1");
        assert_eq!(value_of(&shot, "time stamp"), "2015-09-28T06:42:45Z");
        assert_eq!(value_of(&shot, "Date"), "2015-09-28");
        assert_eq!(value_of(&shot, "Time"), "06:42:45");
        assert!(warned(&w3c, 3, "is not kept"));
        assert!(!warned(&w3c, 4, "is not kept"));
        assert_eq!(w3c.records_skipped, 0);
    }

    #[test]
    fn the_column_holds_a_date_or_time_in_the_forms_controllers_write() {
        // A three-point RTK job for each form a controller's clock writes, one
        // with a blank before it as the opcodes have: every point is read, and
        // its time stamp kept as written, less the blank.
        for stamp in [
            "10:49:06",
            "03/04/2026",
            "3 Apr 2026 10:49",
            "2026-04-03T10:49:06+1000",
            "03/04/2026 10:49:06 PM",
            " 03/04/26 10:49:06.52",
            "2026-04-03 10:49:06 UTC",
            "03/04/26 10:49:06.52 AEST",
            "03/04/26 10:49:06 UTC+10",
            "20260403T104906Z",
        ] {
            let bytes = lines(&[
                format!(" 2\t{stamp}\tEP\t1\tG1\t\t\t500000.0\t6200000.0\t10.0"),
                format!(" 2\t{stamp}\tEP\t1\tG2\t\t\t500010.0\t6200000.0\t10.1"),
                format!(" 2\t{stamp}\tEP\t1\tG3\t\t\t500020.0\t6200000.0\t10.2"),
            ]);
            let read = topcon_test::read(ID, bytes.as_bytes(), "stamps.fld")
                .unwrap_or_else(|error| panic!("{stamp}: {}", error.describe()));
            assert_eq!(read.project.points.len(), 3, "{stamp}");
            assert_eq!(read.records_skipped, 0, "{stamp}");
            let expected =
                std::str::from_utf8(katana_core::text::trimmed(stamp.as_bytes())).unwrap();
            assert_eq!(
                value_of(&metadata_of(&read.project, "G2"), "time stamp"),
                expected,
                "{stamp}"
            );
        }
        // A total-station job stamped on a 12-hour clock: the 100 at its head
        // is read with the column, which its date says it has, as are the
        // fixed records after it, whose count fits the column's layout.
        let clock = read_lines(&[
            "100\t03/04/2026 10:49:00 PM\tdegrees\tmetres\tmillibars\tcelsius",
            "02\t03/04/2026 10:49:01 PM\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t03/04/2026 10:49:02 PM\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
            "03\t03/04/2026 10:49:03 PM\t\t\tK\t\t\t1.5",
            "04\t03/04/2026 10:49:04 PM\t\t\tB\t\t\t0.0\t90.0\t100.0",
            "07\t03/04/2026 10:49:05 PM\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
        ]);
        assert_eq!(clock.records_skipped, 0);
        assert_eq!(clock.project.stations.len(), 1);
        assert_eq!(clock.project.stations[0].backsight_point_id, "B");
        assert_eq!(
            value_of(&metadata_of(&clock.project, "1"), "time stamp"),
            "03/04/2026 10:49:05 PM"
        );
    }

    #[test]
    fn a_column_holding_text_that_is_not_a_date_is_skipped_not_read_as_a_value() {
        // "note", numbers alone or two of them joined, and codes with a month's
        // or a time's letters in them are no date or time.
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
            "07\tnote\tEP\t1\t2\t\t\t20.0\t90.0\t5.0",
            "07\t12\tEP\t1\t3\t\t\t30.0\t90.0\t5.0",
            "07\tMAR1\tEP\t1\t4\t\t\t40.0\t90.0\t5.0",
            "07\t1-2\tEP\t1\t5\t\t\t50.0\t90.0\t5.0",
            "07\tT1-2\tEP\t1\t6\t\t\t60.0\t90.0\t5.0",
            "07\tKB 10:49\tEP\t1\t7\t\t\t70.0\t90.0\t5.0",
        ]);
        assert!(warned(
            &result,
            3,
            "holds 'note' in the column after the opcode, which is neither blank nor a date or \
             time"
        ));
        assert!(warned(
            &result,
            4,
            "holds '12' in the column after the opcode"
        ));
        assert!(warned(
            &result,
            5,
            "holds 'MAR1' in the column after the opcode"
        ));
        assert!(warned(
            &result,
            6,
            "holds '1-2' in the column after the opcode"
        ));
        assert!(warned(
            &result,
            7,
            "holds 'T1-2' in the column after the opcode"
        ));
        // A word before the time is no zone's.
        assert!(warned(
            &result,
            8,
            "holds 'KB 10:49' in the column after the opcode"
        ));
        assert!(topcon_test::unpositioned(&result.project, "2").is_none());
        assert_eq!(result.records_skipped, 6);
    }

    #[test]
    fn attributes_inside_a_group_keep_the_group_and_a_remeasured_mark_keeps_both_measurements() {
        let result = read_fixture("gnss.fld");
        let r001 = metadata_of(&result.project, "R001");
        assert_eq!(value_of(&r001, "Survey Attributes/Kerb height"), "0.150");
        assert_eq!(value_of(&r001, "GPS Information/Antenna Height"), "1.800");
        assert_eq!(value_of(&r001, "GPS Information/GNSS Solution"), "fixed");
        // 0xB0 in a Windows-1252 file is the degree sign, U+00B0.
        assert_eq!(value_of(&r001, "GPS Information/Tilt"), "0\u{B0}10'00\"");
        // The reference station's coordinates, not R001's.
        assert_eq!(
            value_of(&r001, "Reference Information/Easting"),
            "500500.000"
        );
        assert_eq!(
            value_of(&r001, "Reference Information/Northing"),
            "6200500.000"
        );
        assert_eq!(value_of(&r001, "time stamp"), "14/03/26 09:15:02.40");
        // Record 41 measures R001 again: its coordinates, time and attributes
        // are the second measurement's, kept beside the first's.
        assert_eq!(
            value_of(&r001, "coordinates restated at record 41"),
            "N 6200020.003, E 500010.004, elevation 12.505"
        );
        assert_eq!(
            value_of(&r001, "record 41/time stamp"),
            "14/03/26 09:30:00.00"
        );
        assert_eq!(
            value_of(&r001, "record 41/GPS Information/Antenna Height"),
            "2.000"
        );
        assert_eq!(
            value_of(&r001, "record 41/GPS Information/GNSS Solution"),
            "fixed"
        );
        assert!(!r001.contains_key("Antenna Height"));
    }

    #[test]
    fn a_close_string_record_closes_the_current_string_and_a_later_point_starts_a_new_one() {
        let result = read_fixture("gnss.fld");
        // Record 32, a bare 20, closes KB 1 - the string of R003, the current
        // measurement point. R004, coded KB 1 after it, starts a new string.
        let kb = features_of(&result.project, "KB");
        assert_eq!(kb.len(), 2);
        assert_eq!(kb[0].name, "1");
        assert!(kb[0].closed);
        assert_eq!(kb[0].point_ids, ["R001", "R002", "R003"]);
        assert_eq!(kb[1].name, "1");
        assert!(!kb[1].closed);
        assert_eq!(kb[1].point_ids, ["R004"]);
        assert_eq!(result.project.features.len(), 2);
    }

    #[test]
    fn a_group_left_open_ends_at_the_next_point_or_at_a_group_of_its_level() {
        let result = read_lines(&[
            " 2\t14/03/26 09:15:02.40\tKB\t1\tR1\t\t\t500010.0\t6200020.0\t12.5",
            "124\t\t\tGPS Information\t0",
            "73\t\tAntenna Height\t1.800",
            // no 125: record 4 makes another point
            " 2\t14/03/26 09:15:40.10\tKB\t1\tR2\t\t\t500020.0\t6200020.0\t12.6",
            "124\t\t\tSurvey Attributes\t0",
            "73\t\tMaterial\tConcrete",
            // no 125: record 7 opens a group at level 0, where Survey
            // Attributes is
            "124\t\t\tReference Information\t0",
            "73\t\tEasting\t500500.000",
            "125\t\t\t\t0",
        ]);
        let r1 = metadata_of(&result.project, "R1");
        let r2 = metadata_of(&result.project, "R2");
        assert_eq!(value_of(&r1, "GPS Information/Antenna Height"), "1.800");
        assert_eq!(value_of(&r2, "Survey Attributes/Material"), "Concrete");
        assert_eq!(value_of(&r2, "Reference Information/Easting"), "500500.000");
        for key in r2.keys() {
            assert!(!key.contains("GPS Information"), "{key}");
            assert!(!key.contains("Survey Attributes/Reference"), "{key}");
        }
        assert!(warned(
            &result,
            2,
            "attribute group 'GPS Information' is not ended (opcode 125) before record 4, which \
             makes another point or a setup; it is ended there"
        ));
        assert!(warned(
            &result,
            5,
            "attribute group 'Survey Attributes' is not ended (opcode 125) before record 7, which \
             opens attribute group 'Reference Information' at its level"
        ));
        assert_eq!(result.records_skipped, 0);
        assert_eq!(result.warnings.len(), 2);
    }

    #[test]
    fn groups_that_are_never_ended_do_not_grow_the_names_of_later_attributes() {
        // A job of 400 points, each with three groups and no 125 at all: each
        // group ends at the next one of its level or at the next point, so no
        // name holds more than one group - where nesting every unended group
        // made names, and memory, grow with the square of the file.
        let mut bytes = String::new();
        for point in 0..400 {
            bytes.push_str(&format!(
                " 2\t14/03/26 09:15:02.40\tKB\t1\tP{point}\t\t\t{}.0\t6200000.0\t12.5\n",
                500000 + point
            ));
            for group in [
                "Survey Attributes",
                "GPS Information",
                "Reference Information",
            ] {
                bytes.push_str(&format!("124\t\t\t{group}\t0\n73\t\tValue\t1\n"));
            }
        }
        let read = topcon_test::read(ID, bytes.as_bytes(), "unended.fld")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut longest = 0usize;
        for point in &read.project.points {
            for key in point.metadata.keys() {
                longest = longest.max(key.len());
            }
        }
        assert_eq!(longest, "Reference Information/Value".len());
        assert_eq!(
            value_of(&metadata_of(&read.project, "P399"), "GPS Information/Value"),
            "1"
        );
    }

    // ---- The RTK setup fixture -------------------------------------------------------------

    #[test]
    fn a_setup_on_an_rtk_mark_is_oriented_on_another_and_its_offset_shots_land_where_the_file_says()
    {
        let result = read_fixture("rtk_setup.fld");
        assert_eq!(result.records_skipped, 0);
        assert!(result.warnings.is_empty(), "{}", result.warnings[0].message);
        // A (1000, 5000) and B (1000, 5100) are GNSS positions, which the
        // reduction places before any setup: the setup on A reads 30 degrees
        // on B, due north, so its orientation is 0 - 30 = -30 degrees and its
        // circle 120 is due east.
        assert_eq!(gnss_positions_of(&result.project, "A").len(), 1);
        assert_eq!(gnss_positions_of(&result.project, "B").len(), 1);
        assert!(result.project.points.is_empty());
        let positions = reduced_positions(&result.project);
        assert!(positions.contains_key("B"));
        assert_eq!(positions["B"].method, ComputationMethod::Gnss);
        near(positions["B"].easting, 1000.0, POSITION_TOLERANCE);
        near(positions["B"].northing, 5100.0, POSITION_TOLERANCE);
        // 101: 10 m out, level, and the radial offset of record 20 puts it
        // 0.5 m further from the station: 1010.5, 5000.
        assert!(positions.contains_key("101"));
        near(positions["101"].easting, 1010.5, POSITION_TOLERANCE);
        near(positions["101"].northing, 5000.0, POSITION_TOLERANCE);
        // 102: 20 m out, and the tangential offset of record 22, 0.5 m to the
        // right looking east from the station - south: 1020, 4999.5.
        assert!(positions.contains_key("102"));
        near(positions["102"].easting, 1020.0, POSITION_TOLERANCE);
        near(positions["102"].northing, 4999.5, POSITION_TOLERANCE);
    }

    // ---- The resection fixture -------------------------------------------------------------

    #[test]
    fn the_whole_resection_job_is_read_with_nothing_skipped() {
        let result = read_fixture("resection.fld");
        // The version line, 100, 09, two 02, 128, 5, 7, 7, 5, 7, 129 (lines 2
        // and 8-17) and the 15 records of lines 22-36: 27.
        assert_eq!(result.records_read, 27);
        assert_eq!(result.records_skipped, 0);
        // Nothing to warn of: the two offsets are applied.
        assert!(result.warnings.is_empty(), "{}", result.warnings[0].message);
        let project = &result.project;
        assert_eq!(project.stations.len(), 2);
        assert_eq!(project.points.len(), 2); // K1 and K2
        // S1, 101, 102 and 103, in the order first named.
        assert_eq!(project.unpositioned_points.len(), 4);
        assert_eq!(project.unpositioned_points[0].id, "S1");
        assert_eq!(project.unpositioned_points[3].id, "103");
        assert_eq!(project.coordinate_system.name, "Local grid B");
        assert!(topcon_test::any_not_carried_contains(
            &result,
            "no coordinates for the 1 setup(s) made by resection"
        ));
        // The reduction resects such a setup (docs/survey.md, "The reduction's
        // resection"), so the note must not tell a person it cannot.
        assert!(!topcon_test::any_not_carried_contains(
            &result,
            "does not compute a resection"
        ));
    }

    #[test]
    fn a_resection_starts_a_setup_on_its_named_point_and_its_shots_are_its_own() {
        let result = read_fixture("resection.fld");
        assert_eq!(result.project.stations.len(), 2);
        let s1 = &result.project.stations[0];
        assert_eq!(s1.setup.point_id, "S1");
        double_eq(s1.setup.instrument_height, 1.55);
        assert!(s1.backsight_point_id.is_empty());
        assert_eq!(
            value_of(&s1.metadata, "resection"),
            "least squares, record 11"
        );
        assert_eq!(value_of(&s1.metadata, "resection end"), "record 17");
        // Inside the block, "7" is opcode 07: K1 on both faces - 180 and 0
        // degrees, zenith 90 on face left and 270 on face right - and K2 at
        // 135 degrees under the target height "5" set to 1.700.
        let to_k1 = observations_to::<HorizontalDirectionObservation>(s1, "K1");
        assert_eq!(to_k1.len(), 2);
        near(to_k1[0].direction, PI, ANGLE_TOLERANCE);
        near(to_k1[1].direction, 0.0, ANGLE_TOLERANCE);
        let zeniths_k1 = observations_to::<ZenithAngleObservation>(s1, "K1");
        assert_eq!(zeniths_k1.len(), 2);
        assert_eq!(zeniths_k1[0].pointing.face, Face::Left);
        assert_eq!(zeniths_k1[1].pointing.face, Face::Right);
        near(zeniths_k1[1].angle, PI / 2.0, ANGLE_TOLERANCE);
        double_eq(zeniths_k1[0].target_height, 1.6);
        let to_k2 = observations_to::<DistanceObservation>(s1, "K2");
        assert_eq!(to_k2.len(), 2); // the resection's, and the check of record 31
        double_eq(to_k2[0].distance, 141.421);
        double_eq(to_k2[0].target_height, 1.7);
        double_eq(to_k2[1].distance, 141.42);
        // After the 129 the shots are still S1's: 101 at 90 degrees (and
        // 0.25 m to the right, the offset of record 24), 102 at 95.
        let to101 = observations_to::<HorizontalDirectionObservation>(s1, "101");
        assert_eq!(to101.len(), 1);
        near(
            to101[0].direction,
            PI / 2.0 + 0.25_f64.atan2(50.0),
            ANGLE_TOLERANCE,
        );
        assert_eq!(
            observations_to::<HorizontalDirectionObservation>(s1, "102").len(),
            1
        );
        // Six pointings of three observations each.
        assert_eq!(s1.observations.len(), 18);
        // The 03 at line 33 is the next setup, with its backsight and its shot.
        let k1 = &result.project.stations[1];
        assert_eq!(k1.setup.point_id, "K1");
        assert_eq!(k1.backsight_point_id, "K2");
        assert_eq!(observations_to::<DistanceObservation>(k1, "103").len(), 1);
        assert_eq!(observations_to::<DistanceObservation>(k1, "101").len(), 0);
    }

    #[test]
    fn the_resection_residual_comments_are_notes_on_the_resected_setup() {
        let result = read_fixture("resection.fld");
        let s1 = &result.project.stations[0];
        // Verbatim, the delta and degree signs as UTF-8; the rules are dropped.
        assert_eq!(
            value_of(&s1.metadata, "notes"),
            "Station Setup - Residuals\nID: K1 \u{394}E:0.001 \u{394}N:-0.002 \
             \u{394}HA:0\u{B0}00'01\""
        );
        // Nothing of the residual lines became header metadata.
        assert!(!result.project.metadata.contains_key("header: ID"));
    }

    #[test]
    fn multiple_coding_strings_the_current_point_into_a_second_feature() {
        let result = read_fixture("resection.fld");
        let project = &result.project;
        // 101 (EB 01) is coded NS 01 by record 25, and 102 PF (no string
        // number) by record 30; each keeps its own code.
        let ns = topcon_test::feature(project, "NS").expect("NS");
        assert_eq!(ns.name, "01");
        assert_eq!(ns.point_ids, ["101"]);
        let pf = topcon_test::feature(project, "PF").expect("PF");
        assert_eq!(pf.name, "");
        assert_eq!(pf.point_ids, ["102"]);
        let eb = topcon_test::feature(project, "EB").expect("EB");
        assert_eq!(eb.point_ids, ["101", "102", "103"]);
        assert_eq!(
            topcon_test::unpositioned(project, "101").unwrap().code,
            "EB"
        );
        assert_eq!(project.features.len(), 3);
    }

    #[test]
    fn a_check_measurement_keeps_its_code_without_stringing_its_target() {
        let result = read_fixture("resection.fld");
        // Record 31 checks K2 from S1 and is coded BS 11: no BS string, the
        // code kept. K2 was made by record 10, so what the check says is under
        // its record, with the attribute after it.
        assert!(topcon_test::feature(&result.project, "BS").is_none());
        let k2 = metadata_of(&result.project, "K2");
        assert_eq!(
            value_of(&k2, "record 31/check measurement"),
            "from setup S1, feature code BS, string number 11"
        );
        assert_eq!(value_of(&k2, "record 31/Time"), "10:15:00");
        assert!(!k2.contains_key("Time"));
        assert_eq!(topcon_test::point(&result.project, "K2").unwrap().code, "");
    }

    #[test]
    fn a_backsights_code_is_kept_with_its_setup_and_strings_nothing() {
        let result = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\tBS\t02\tB\t\tbacksight mark\t0.0\t90.0\t100.0",
        ]);
        assert_eq!(result.project.stations.len(), 1);
        assert_eq!(
            value_of(&result.project.stations[0].metadata, "backsight record 4"),
            "feature code BS, string number 02, comment backsight mark"
        );
        assert!(result.project.features.is_empty());
        let b = topcon_test::point(&result.project, "B").expect("B");
        assert_eq!(b.code, "");
        assert_eq!(b.description, "");
        assert!(!b.metadata.contains_key("backsight"));
    }

    #[test]
    fn offsets_move_the_shot_they_follow_and_keep_what_was_measured() {
        let result = read_fixture("resection.fld");
        let s1 = &result.project.stations[0];
        // 101: circle 90, zenith 90, 50 m, then record 24's tangential 0.25 m,
        // to the right looking from S1: the circle reading of a point 50 m out
        // and 0.25 m across, 90 + atan2(0.25, 50) degrees, hypot(50, 0.25) m
        // away, still level.
        let direction101 = observations_to::<HorizontalDirectionObservation>(s1, "101");
        assert_eq!(direction101.len(), 1);
        near(
            direction101[0].direction,
            PI / 2.0 + 0.25_f64.atan2(50.0),
            ANGLE_TOLERANCE,
        );
        let distance101 = observations_to::<DistanceObservation>(s1, "101");
        assert_eq!(distance101.len(), 1);
        near(distance101[0].distance, 50.0_f64.hypot(0.25), 1e-12);
        let zenith101 = observations_to::<ZenithAngleObservation>(s1, "101");
        assert_eq!(zenith101.len(), 1);
        near(zenith101[0].angle, PI / 2.0, ANGLE_TOLERANCE);
        // 102: circle 95, 50 m, then record 29's radial 0.5 m, away from S1:
        // the same circle, 50.5 m.
        let direction102 = observations_to::<HorizontalDirectionObservation>(s1, "102");
        assert_eq!(direction102.len(), 1);
        near(direction102[0].direction, degrees(95.0), ANGLE_TOLERANCE);
        let distance102 = observations_to::<DistanceObservation>(s1, "102");
        assert_eq!(distance102.len(), 1);
        near(distance102[0].distance, 50.5, 1e-12);
        // What was measured, and each offset, are kept with the point.
        let p101 = metadata_of(&result.project, "101");
        assert_eq!(
            value_of(&p101, "offset record 24"),
            "tangential 0.25 m from setup S1, applied to the shot of record 23"
        );
        assert_eq!(
            value_of(&p101, "shot record 23 as measured"),
            "horizontal circle 90.00000000, vertical circle 90.00000000, slope distance 50.000"
        );
        assert_eq!(
            value_of(&metadata_of(&result.project, "102"), "offset record 29"),
            "radial 0.5 m from setup S1, applied to the shot of record 28"
        );
    }

    #[test]
    fn an_integer_attribute_is_kept_like_the_others() {
        let result = read_fixture("resection.fld");
        let p101 = metadata_of(&result.project, "101");
        assert_eq!(value_of(&p101, "Tree Spread"), "5");
        assert_eq!(value_of(&p101, "Date"), "2026-03-14");

        let not_whole = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tPT\t1\t1\t\t\t10.0\t90.0\t5.0",
            "71\t\tTree Spread\t4.5",
        ]);
        assert!(warned(
            &not_whole,
            3,
            "which is not a whole number; it is kept as text"
        ));
        assert_eq!(
            value_of(&metadata_of(&not_whole.project, "1"), "Tree Spread"),
            "4.5"
        );
    }

    // ---- A point measured more than once ---------------------------------------------------

    #[test]
    fn each_measurement_of_a_point_keeps_its_attributes_under_its_record() {
        // A mark shot on both faces and checked: [FLD] makes each a point with
        // its own attributes. The first shot's are the point's; the others'
        // are under their records, none overwriting another.
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEB\t01\tM1\t\t\t30.0\t85.0\t40.00",
            "73\t\tTime\t10:15:00",
            "72\t\tTarget height\t1.800",
            "07\t\tEB\t01\tM1\t\t\t210.0\t275.0\t40.01",
            "73\t\tTime\t10:15:20",
            "72\t\tTarget height\t1.800",
            "06\t\t\t\tM1\t\t\t30.0\t85.0\t40.00",
            "73\t\tTime\t10:17:00",
        ]);
        let m1 = metadata_of(&result.project, "M1");
        assert_eq!(value_of(&m1, "Time"), "10:15:00");
        assert_eq!(value_of(&m1, "Target height"), "1.800");
        assert_eq!(value_of(&m1, "record 5/Time"), "10:15:20");
        assert_eq!(value_of(&m1, "record 5/Target height"), "1.800");
        assert_eq!(value_of(&m1, "record 8/Time"), "10:17:00");
        assert_eq!(value_of(&m1, "record 8/check measurement"), "from setup S");
        assert_eq!(result.records_read, 9);
        assert!(result.warnings.is_empty(), "{}", result.warnings[0].message);
    }

    #[test]
    fn a_name_given_twice_after_one_record_keeps_its_second_value_with_a_warning() {
        // A shot coded twice (07 then 16) followed by its attribute set twice:
        // what follows the 16 is still the shot's, so the second set's names
        // repeat the first's. A repeat with the same value adds nothing; one
        // with another value is kept under its own record, and said.
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tPP\t03\t501\t\t\t160.0\t93.0\t50.0",
            "16\t\tUC\t03\t\t\t",
            "73\t\tDepth\t0.750",
            "73\t\tCondition\tGood",
            "73\t\tDepth\t--",
            "73\t\tCondition\tGood",
        ]);
        let p501 = metadata_of(&result.project, "501");
        assert_eq!(value_of(&p501, "Depth"), "0.750");
        assert_eq!(value_of(&p501, "Depth (record 6)"), "--");
        assert_eq!(value_of(&p501, "Condition"), "Good");
        assert_eq!(value_of(&p501, "Condition (record 7)"), "(absent)");
        assert!(warned(
            &result,
            6,
            "point '501' is given 'Depth' again, as '--' after '0.750'; both are kept, this one \
             as 'Depth (record 6)'"
        ));
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.records_read, 7);
        assert_eq!(result.records_skipped, 0);
    }

    #[test]
    fn a_keyed_in_mark_measured_again_keeps_the_new_measurements_attributes_apart() {
        // CM1 keyed in with no attributes, then an RTK measurement of it at the
        // same coordinates: the solution's attributes and time are record 2's,
        // not the keyed-in mark's own.
        let result = read_lines(&[
            "02\t\t\t\tCM1\t\t\t500000.0\t6200000.0\t10.0",
            " 2\t14/03/26 09:15:02.40\t\t\tCM1\t\t\t500000.0\t6200000.0\t10.0",
            "124\t\t\tGPS Information\t0",
            "73\t\tGNSS Solution\tfixed",
            "125\t\t\t\t0",
        ]);
        let cm1 = metadata_of(&result.project, "CM1");
        assert_eq!(
            value_of(&cm1, "record 2/time stamp"),
            "14/03/26 09:15:02.40"
        );
        assert_eq!(
            value_of(&cm1, "record 2/GPS Information/GNSS Solution"),
            "fixed"
        );
        assert_eq!(value_of(&cm1, "GPS Information/GNSS Solution"), "(absent)");
        assert_eq!(value_of(&cm1, "time stamp"), "(absent)");
        assert!(result.warnings.is_empty(), "{}", result.warnings[0].message);
    }

    #[test]
    fn a_point_id_given_with_its_name_is_kept_once_and_a_different_one_under_its_record() {
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\t\t\t11\tCP\t\t10.0\t90.0\t5.0",
            "07\t\t\t\t12\tCP\t\t190.0\t270.0\t5.0",
            "07\t\t\t\t11\tCP\t\t10.0\t90.0\t5.0",
        ]);
        let cp = metadata_of(&result.project, "CP");
        assert_eq!(value_of(&cp, "point ID"), "11");
        assert_eq!(value_of(&cp, "record 3/point ID"), "12");
        assert_eq!(value_of(&cp, "record 4/point ID"), "(absent)");
        assert!(topcon_test::unpositioned(&result.project, "11").is_none());
    }

    #[test]
    fn a_later_measurements_comment_is_kept_under_its_record() {
        // The first shot's comment is the point's description; the second's,
        // which the point's one description cannot hold, is kept under its
        // record.
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEB\t01\t101\t\tkerb\t10.0\t90.0\t5.0",
            "07\t\tEB\t01\t101\t\tkerb top\t190.0\t270.0\t5.0",
        ]);
        let p101 = topcon_test::unpositioned(&result.project, "101").expect("101");
        assert_eq!(p101.description, "kerb");
        assert_eq!(value_of(&p101.metadata, "record 3/comment"), "kerb top");
        assert!(result.warnings.is_empty(), "{}", result.warnings[0].message);
    }

    #[test]
    fn a_setup_or_a_backsight_naming_only_the_point_id_of_a_named_coordinate_finds_it() {
        // 44.5: a setup's and a backsight's point is searched for by its name,
        // and by its point ID among the point IDs of the coordinates before.
        // The 03 names only ID 1001, which the 02 gave CP1, and the 04 only ID
        // 1002, BS's: the setup stands on CP1 and backsights BS - where it
        // stood on a new point "1001" with no position, and nothing was
        // radiated.
        let result = read_lines(&[
            "02\t\t\t\t1001\tCP1\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\t1002\tBS\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\t1001\t\t\t1.5",
            "04\t\t\t\t1002\t\t\t0.0\t90.0\t100.0",
            "07\t\tEP\t1\t101\t\t\t90.0\t90.0\t10.0",
        ]);
        assert_eq!(result.project.stations.len(), 1);
        let station = &result.project.stations[0];
        assert_eq!(station.setup.point_id, "CP1");
        assert_eq!(value_of(&station.metadata, "point ID"), "1001");
        assert_eq!(station.backsight_point_id, "BS");
        assert!(topcon_test::unpositioned(&result.project, "1001").is_none());
        let positions = reduced_positions(&result.project);
        assert!(positions.contains_key("101"));
        near(positions["101"].easting, 1010.0, POSITION_TOLERANCE);
        near(positions["101"].northing, 5000.0, POSITION_TOLERANCE);
    }

    #[test]
    fn a_record_giving_a_name_of_its_own_finds_the_point_its_id_names_in44_point5s_order() {
        // 44.5 searches the coordinates and then the shots, in each the name
        // among the names, the name among the IDs, the ID among the IDs, and
        // stops at the first found. Coordinates 1001 and 1002 have no name;
        // CP3 is ID 1003. The 03 names STN1 with ID 1001 and the 04 RO with ID
        // 1002: no coordinate or shot has those names, so each is found by its
        // ID (step 6) - where the setup stood on a new point STN1 with no
        // position and nothing was radiated. The name each gives is kept, not
        // made a point.
        //
        // 1001 at (1000, 5000, 20), HI 1.5 and target height 0, oriented on
        // 1002 due north at circle 0: 101 at circle 90, level, 10 m, is at
        // (1010, 5000), height 20 + 1.5 - 0 = 21.5; M5 at circle 270 at
        // (990, 5000); P7 at circle 0 at (1000, 5010).
        let result = read_lines(&[
            "02\t\t\t\t1001\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\t1002\t\t\t1000.0\t5100.0\t21.0",
            "02\t\t\t\t1003\tCP3\t\t1000.0\t4990.0\t20.0",
            "03\t\t\t\t1001\tSTN1\t\t1.5",
            "05\t\t0.0",
            "04\t\t\t\t1002\tRO\t\t0.0\t90.0\t100.0",
            "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
            "07\t\tEB\t01\t3001\tM5\t\t270.0\t90.0\t10.0",
            // A shot's name and a coordinate's ID: the coordinates come first
            // (step 6 before step 7), so this checks CP3, not the shot M5.
            "06\t\t\t\t1003\tM5\t\t180.0\t90.0\t10.0",
            "07\t\tEB\t01\t2001\tP7\t\t0.0\t90.0\t10.0",
            // A shot's ID (step 9): the setup stands on P7, the shot that gave
            // it. Oriented on 1001, due south at circle 180 (orientation 0):
            // 102 at circle 90, 5 m, is at (1005, 5010).
            "03\t\t\t\t2001\tSTNX\t\t1.5",
            "04\t\t\t\t1001\t\t\t180.0\t90.0\t10.0",
            "07\t\tEB\t02\t102\t\t\t90.0\t90.0\t5.0",
        ]);
        assert_eq!(result.records_skipped, 0);
        let project = &result.project;
        assert_eq!(project.stations.len(), 2);
        let first = &project.stations[0];
        assert_eq!(first.setup.point_id, "1001");
        assert_eq!(value_of(&first.metadata, "point name"), "STN1");
        assert_eq!(value_of(&first.metadata, "point ID"), "(absent)"); // the point's own
        assert_eq!(first.backsight_point_id, "1002");
        assert_eq!(
            value_of(&metadata_of(project, "1002"), "record 6/point name"),
            "RO"
        );
        assert_eq!(
            value_of(&metadata_of(project, "1002"), "record 6/point ID"),
            "(absent)"
        );
        // The check is on CP3, and what it calls it is kept there. M5 being a
        // point too, the check says which it took.
        let cp3 = metadata_of(project, "CP3");
        assert_eq!(
            value_of(&cp3, "record 9/check measurement"),
            "from setup 1001"
        );
        assert_eq!(value_of(&cp3, "record 9/point name"), "M5");
        assert!(warned(
            &result,
            9,
            "the check names point 'M5' with point ID 1003, and the format's search for its point \
             (44.5) finds point 'CP3' first: the check checks 'CP3', not the point of that name"
        ));
        assert_eq!(
            observations_to::<DistanceObservation>(first, "CP3").len(),
            1
        );
        assert_eq!(observations_to::<DistanceObservation>(first, "M5").len(), 1);
        let second = &project.stations[1];
        assert_eq!(second.setup.point_id, "P7");
        assert_eq!(value_of(&second.metadata, "point name"), "STNX");
        assert_eq!(value_of(&second.metadata, "point ID"), "2001");
        assert_eq!(second.backsight_point_id, "1001");
        for stray in ["STN1", "RO", "STNX"] {
            assert!(
                topcon_test::unpositioned(project, stray).is_none(),
                "{stray}"
            );
            assert!(topcon_test::point(project, stray).is_none(), "{stray}");
        }
        let positions = reduced_positions(project);
        assert_eq!(
            usize::from(positions.contains_key("101"))
                + usize::from(positions.contains_key("P7"))
                + usize::from(positions.contains_key("102")),
            3
        );
        near(positions["101"].easting, 1010.0, POSITION_TOLERANCE);
        near(positions["101"].northing, 5000.0, POSITION_TOLERANCE);
        near(
            positions["101"].elevation.expect("a height"),
            21.5,
            POSITION_TOLERANCE,
        );
        near(positions["P7"].easting, 1000.0, POSITION_TOLERANCE);
        near(positions["P7"].northing, 5010.0, POSITION_TOLERANCE);
        near(positions["102"].easting, 1005.0, POSITION_TOLERANCE);
        near(positions["102"].northing, 5010.0, POSITION_TOLERANCE);
    }

    #[test]
    fn a_backsight_named_by_another_points_id_makes_no_point_of_that_name() {
        // 44.5, step 5: the backsight's name, 1002, is CP2's point ID, so the
        // setup backsights CP2 - due north of A at circle 0, so 101 at circle
        // 90, 10 m, is at (1010, 5000). What the 04 calls it, name 1002 and ID
        // 77, is kept on CP2 under the record, where it made a point 1002 of
        // its own, with no position, that the reduction then reported as not
        // drawn.
        let result = read_lines(&[
            "02\t\t\t\t1002\tCP2\t\t1000.0\t5100.0\t21.0",
            "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
            "03\t\t\t\tA\t\t\t1.5",
            "04\t\t\t\t77\t1002\t\t0.0\t90.0\t100.0",
            "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
        ]);
        assert_eq!(result.project.stations.len(), 1);
        assert_eq!(result.project.stations[0].backsight_point_id, "CP2");
        let cp2 = metadata_of(&result.project, "CP2");
        assert_eq!(value_of(&cp2, "point ID"), "1002");
        assert_eq!(value_of(&cp2, "record 4/point name"), "1002");
        assert_eq!(value_of(&cp2, "record 4/point ID"), "77");
        assert_eq!(result.project.unpositioned_points.len(), 1);
        assert_eq!(result.project.unpositioned_points[0].id, "101");
        let positions = reduced_positions(&result.project);
        assert!(positions.contains_key("101"));
        near(positions["101"].easting, 1010.0, POSITION_TOLERANCE);
        near(positions["101"].northing, 5000.0, POSITION_TOLERANCE);
    }

    #[test]
    fn within_each_of44_point5s_lists_a_name_is_searched_for_before_an_id() {
        // Each record below gives a name and an ID that find two different
        // points in one list; 44.5 takes the name's. Coordinates P1 (ID X1),
        // due north of A, and P2 (ID Y1), due east; shots Q1 (ID S1) and Q2
        // (ID S2).
        //   record 5: name X1 is P1's ID (step 5), ID Y1 P2's (step 6): P1. The
        //     setup, oriented on P1 at circle 0, puts 101 at circle 90, 10 m,
        //     at (1010, 5000); oriented on P2 it would be at (1000, 4990).
        //   record 7: name P1 is P1's (step 4), ID Y1 P2's (step 6): P1.
        //   record 10: name Q1 is Q1's (step 7), ID S2 Q2's (step 9): Q1.
        //   record 11: name S1 is Q1's ID (step 8), ID S2 Q2's (step 9): Q1.
        let result = read_lines(&[
            "02\t\t\t\tX1\tP1\t\t1000.0\t5100.0\t21.0",
            "02\t\t\t\tY1\tP2\t\t1100.0\t5000.0\t21.0",
            "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
            "03\t\t\t\tA\t\t\t1.5",
            "04\t\t\t\tY1\tX1\t\t0.0\t90.0\t100.0",
            "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
            "06\t\t\t\tY1\tP1\t\t0.0\t90.0\t100.0",
            "07\t\tEB\t01\tS1\tQ1\t\t180.0\t90.0\t10.0",
            "07\t\tEB\t01\tS2\tQ2\t\t270.0\t90.0\t10.0",
            "06\t\t\t\tS2\tQ1\t\t180.0\t90.0\t10.0",
            "06\t\t\t\tS2\tS1\t\t180.0\t90.0\t10.0",
        ]);
        assert_eq!(result.records_skipped, 0);
        assert_eq!(result.project.stations.len(), 1);
        assert_eq!(result.project.stations[0].backsight_point_id, "P1");
        let p1 = metadata_of(&result.project, "P1");
        assert_eq!(value_of(&p1, "record 5/point name"), "X1");
        assert_eq!(value_of(&p1, "record 5/point ID"), "Y1");
        assert_eq!(value_of(&p1, "record 7/check measurement"), "from setup A");
        let q1 = metadata_of(&result.project, "Q1");
        assert_eq!(value_of(&q1, "record 10/check measurement"), "from setup A");
        assert_eq!(value_of(&q1, "record 11/check measurement"), "from setup A");
        for other in ["P2", "Q2"] {
            for key in metadata_of(&result.project, other).keys() {
                assert!(!key.contains("check measurement"), "{other}: {key}");
            }
        }
        let positions = reduced_positions(&result.project);
        assert!(positions.contains_key("101"));
        near(positions["101"].easting, 1010.0, POSITION_TOLERANCE);
        near(positions["101"].northing, 5000.0, POSITION_TOLERANCE);
    }

    #[test]
    fn a_record_whose_name_is_another_point_than_the_one44_point5_finds_is_warned_of() {
        // T1 is a point only a check (06) made, so it is in neither of 44.5's
        // lists, the coordinates and the shots. The second setup names T1 with
        // point ID 5, which shot 5 was given: 44.5 finds shot 5 by its ID
        // (step 9), and the setup stands on it, with a warning that its name
        // is another point's. Shot 5 is at (1010, 5000); from it A is on
        // bearing 270, read at circle 0, so 301 at circle 90, 10 m, is on
        // bearing 0: (1010, 5010).
        let result = read_lines(&[
            "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tA\t\t\t1.5",
            "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
            "07\t\tEB\t01\t5\t\t\t90.0\t90.0\t10.0",
            "06\t\t\t\t\tT1\t\t180.0\t90.0\t20.0",
            "03\t\t\t\t5\tT1\t\t1.5",
            "04\t\t\t\tA\t\t\t0.0\t90.0\t10.0",
            "07\t\tEB\t02\t301\t\t\t90.0\t90.0\t10.0",
        ]);
        assert_eq!(result.records_skipped, 0);
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(result.project.stations[1].setup.point_id, "5");
        assert_eq!(
            value_of(&result.project.stations[1].metadata, "point name"),
            "T1"
        );
        assert!(warned(
            &result,
            7,
            "the setup names point 'T1' with point ID 5, and the format's search for its point \
             (44.5) finds point '5' first: the setup stands on '5', not the point of that name"
        ));
        let positions = reduced_positions(&result.project);
        assert!(positions.contains_key("301"));
        near(positions["301"].easting, 1010.0, POSITION_TOLERANCE);
        near(positions["301"].northing, 5010.0, POSITION_TOLERANCE);
    }

    #[test]
    fn a_header_key_given_again_keeps_both_values_with_a_warning() {
        let result = read_lines(&[
            "// Job name : first",
            "// Job name : second",
            "// Coordinate System: Local grid E",
            "// Coordinate System: Local grid F",
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        ]);
        assert_eq!(result.project.metadata["header: Job name"], "first");
        assert_eq!(
            result.project.metadata["header: Job name (record 2)"],
            "second"
        );
        assert!(warned(
            &result,
            2,
            "the header gives 'Job name' again, as 'second' after 'first'; both are kept, this \
             one as 'header: Job name (record 2)'"
        ));
        // The system declared is the first.
        assert_eq!(result.project.coordinate_system.name, "Local grid E");
        assert_eq!(
            result.project.metadata["header: Coordinate System (record 4)"],
            "Local grid F"
        );
    }

    // ---- The backsight -----------------------------------------------------------------------

    #[test]
    fn a_backsights_stated_azimuth_is_kept_apart_from_its_circle_and_orients_a_setup_nothing_else_can()
     {
        // [FLD] 44.8, 04: "The azimuth_value ... may be specified when no
        // coordinate for the backsight point exists." CP1 at (1000, 5000); BSX
        // has no coordinates, reads 30 degrees and is given azimuth 45; the
        // shot reads 45, zenith 95, 50 m.
        let result = read_lines(&[
            "02\t\t\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
            "03\t\t\t\tCP1\t\t\t1.5",
            "04\t\t\t\tBSX\t\t\t30.0\t90.0\t50.0\t45.0",
            "07\t\tEB\t01\t101\t\t\t45.0\t95.0\t50.0",
        ]);
        assert_eq!(result.project.stations.len(), 1);
        let station = &result.project.stations[0];
        // The circle read on the backsight, and the azimuth the file states,
        // each in its own field.
        near(
            station.backsight_azimuth.expect("a circle"),
            degrees(30.0),
            ANGLE_TOLERANCE,
        );
        near(
            station.stated_backsight_azimuth.expect("an azimuth"),
            degrees(45.0),
            ANGLE_TOLERANCE,
        );
        // The orientation is 45 - 30 = 15 degrees, so the shot's bearing is
        // 60: 50 sin 95 m out, (1000 + that sin 60, 5000 + that cos 60) -
        // where the circle taken as the azimuth would put it on bearing 45,
        // 13 m away. The report says the bearings are the file's, not a circle
        // taken for them.
        let outcome = reduced(&result.project);
        let plan = 50.0 * degrees(95.0).sin();
        let shot = outcome
            .points
            .iter()
            .find(|p| p.id == "101")
            .expect("101 is placed");
        near(
            shot.easting,
            1000.0 + plan * degrees(60.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            shot.northing,
            5000.0 + plan * degrees(60.0).cos(),
            POSITION_TOLERANCE,
        );
        assert_eq!(
            reduction_warnings_with(&outcome.report, "oriented on the azimuth the file states"),
            1
        );
        assert_eq!(
            reduction_warnings_with(&outcome.report, "taken as a grid azimuth"),
            0
        );

        // With no azimuth the circle alone is recorded, and nothing is stated.
        let circle = read_lines(&[
            "02\t\t\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
            "03\t\t\t\tCP1\t\t\t1.5",
            "04\t\t\t\tBSX\t\t\t30.0\t90.0\t50.0",
        ]);
        assert_eq!(circle.project.stations.len(), 1);
        near(
            circle.project.stations[0]
                .backsight_azimuth
                .expect("a circle"),
            degrees(30.0),
            ANGLE_TOLERANCE,
        );
        assert!(
            circle.project.stations[0]
                .stated_backsight_azimuth
                .is_none()
        );
    }

    #[test]
    fn a_stated_azimuth_is_no_circle_reading_and_does_not_orient_a_setup_whose_backsight_has_coordinates()
     {
        // BSX has coordinates, due north of CP1; the backsight gives no
        // horizontal circle, and an azimuth of 45. Coordinates orient a setup
        // through its reading on the backsight, and there is none: the setup
        // is not oriented, and says why - rather than the azimuth taken for
        // the circle set on BSX, which turned the shot at 90 to bearing 45.
        let result = read_lines(&[
            "02\t\t\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tBSX\t\t\t1000.0\t5050.0\t20.0",
            "03\t\t\t\tCP1\t\t\t1.5",
            "04\t\t\t\tBSX\t\t\t\t90.0\t50.0\t45.0",
            "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t50.0",
        ]);
        assert_eq!(result.records_skipped, 0);
        let station = &result.project.stations[0];
        assert!(station.backsight_azimuth.is_none());
        near(
            station.stated_backsight_azimuth.expect("an azimuth"),
            degrees(45.0),
            ANGLE_TOLERANCE,
        );
        let outcome = reduced(&result.project);
        for point in &outcome.points {
            assert_ne!(point.id, "101");
        }
        assert_eq!(
            reduction_warnings_with(
                &outcome.report,
                "Setup CP1 cannot be oriented on its backsight BSX"
            ),
            1
        );
    }

    #[test]
    fn a_later_backsight_changes_neither_the_circle_nor_the_stated_azimuth() {
        // Face left with the azimuth, then face right without it: the setup
        // keeps the face-left circle, 30, and the azimuth, 45, so the shot at
        // 45 is on bearing 60, as with the first alone - where the face-right
        // reading taken for the circle set on BSX turned it to bearing 225.
        let result = read_lines(&[
            "02\t\t\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
            "03\t\t\t\tCP1\t\t\t1.5",
            "04\t\t\t\tBSX\t\t\t30.0\t90.0\t50.0\t45.0",
            "04\t\t\t\tBSX\t\t\t210.0\t270.0\t50.0",
            "07\t\tEB\t01\t101\t\t\t45.0\t95.0\t50.0",
            // A setup whose first backsight is face right: its circle is
            // 210 - 180. Its second gives another azimuth, kept apart, and
            // reads 30.01, so a circle taken from the last backsight rather
            // than the first would be 30.01 (the first setup's two readings,
            // 180 apart, cannot tell).
            "03\t\t\t\tCP1\t\t\t1.5",
            "04\t\t\t\tBSX\t\t\t210.0\t270.0\t50.0\t45.0",
            "04\t\t\t\tBSX\t\t\t30.01\t90.0\t50.0\t46.0",
        ]);
        assert_eq!(result.project.stations.len(), 2);
        for station in &result.project.stations {
            let (Some(circle), Some(azimuth)) =
                (station.backsight_azimuth, station.stated_backsight_azimuth)
            else {
                panic!("{}: a circle and an azimuth", station.setup.id);
            };
            near(circle, degrees(30.0), ANGLE_TOLERANCE);
            near(azimuth, degrees(45.0), ANGLE_TOLERANCE);
        }
        assert_eq!(
            value_of(
                &result.project.stations[1].metadata,
                "backsight azimuth record 8"
            ),
            "46.0 degrees"
        );
        assert!(warned(
            &result,
            8,
            "the backsight gives the azimuth 46.0 degrees, where an earlier backsight of setup \
             CP1 (2) gave another"
        ));
        let plan = 50.0 * degrees(95.0).sin();
        let positions = reduced_positions(&result.project);
        assert!(positions.contains_key("101"));
        near(
            positions["101"].easting,
            1000.0 + plan * degrees(60.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            positions["101"].northing,
            5000.0 + plan * degrees(60.0).cos(),
            POSITION_TOLERANCE,
        );
    }

    #[test]
    fn a_backsight_to_another_point_than_the_one_that_orients_the_setup_is_a_measurement_of_it() {
        // Two backsights to different points in one setup, each of these three
        // with the first able to orient it. The first names the backsight, and
        // the circle and the azimuth are that point's; the second is read as a
        // measurement of its own point. Before, the setup took its backsight
        // point from the last 04 and its circle and azimuth from the first, so
        // it was oriented on one point's circle less the reading on another's,
        // and every direction turned by the difference.
        //
        // K at (1000, 5000). R1, which nothing places, reads 0 and is given
        // azimuth 45: orientation 45 - 0 = 45 degrees. R2, placed by nothing
        // either, reads 90 and is given azimuth 135 (the same orientation,
        // 135 - 90). Shot 501 reads 90, level, 10 m: bearing 90 + 45 = 135, at
        // (1000 + 10 sin 135, 5000 + 10 cos 135) = (1007.0711, 4992.9289); R2
        // is radiated at bearing 135, 100 m out. The mixed setup read 45 less
        // the reading on R2, 90: orientation -45, and 501 on bearing 45.
        let stated = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tR1\t\t\t0.0\t90.0\t100.0\t45.0",
            "04\t\t\t\tR2\t\t\t90.0\t90.0\t100.0\t135.0",
            "07\t\tEB\t01\t501\t\t\t90.0\t90.0\t10.0",
        ]);
        assert_eq!(stated.records_skipped, 0);
        assert_eq!(stated.project.stations.len(), 1);
        let k = &stated.project.stations[0];
        assert_eq!(k.backsight_point_id, "R1");
        let (Some(circle), Some(azimuth)) = (k.backsight_azimuth, k.stated_backsight_azimuth)
        else {
            panic!("a circle and an azimuth");
        };
        near(circle, 0.0, ANGLE_TOLERANCE);
        near(azimuth, degrees(45.0), ANGLE_TOLERANCE);
        assert_eq!(
            value_of(&k.metadata, "backsight azimuth record 4"),
            "135.0 degrees"
        );
        assert!(warned(
            &stated,
            4,
            "a backsight to 'R2', where setup K backsights 'R1' (record 3), the first that can \
             orient it, with a horizontal circle and a stated azimuth: this one is read as a \
             measurement of its point; its azimuth, 135.0 degrees, is kept in the setup's metadata"
        ));
        let positions = reduced_positions(&stated.project);
        assert!(positions.contains_key("501") && positions.contains_key("R2"));
        near(
            positions["501"].easting,
            1000.0 + 10.0 * degrees(135.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            positions["501"].northing,
            5000.0 + 10.0 * degrees(135.0).cos(),
            POSITION_TOLERANCE,
        );
        near(
            positions["R2"].easting,
            1000.0 + 100.0 * degrees(135.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            positions["R2"].northing,
            5000.0 + 100.0 * degrees(135.0).cos(),
            POSITION_TOLERANCE,
        );

        // The first backsight has coordinates: R3, due north of K, reads 30, so
        // the orientation is 0 - 30 = -30 degrees. R4, which nothing places,
        // reads 120, 50 m out: bearing 90, at (1050, 5000); shot 502 at circle
        // 120, 10 m: (1010, 5000). The mixed setup, backsighting R4 with no
        // position, took R3's circle for a grid azimuth less the reading on
        // R4: 30 - 120 = -90, and 502 on bearing 30, with no reader warning.
        let known = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tR3\t\t\t30.0\t90.0\t100.0",
            "04\t\t\t\tR4\t\t\t120.0\t90.0\t50.0",
            "07\t\tEB\t01\t502\t\t\t120.0\t90.0\t10.0",
        ]);
        assert_eq!(known.project.stations.len(), 1);
        assert_eq!(known.project.stations[0].backsight_point_id, "R3");
        assert!(warned(
            &known,
            5,
            "a backsight to 'R4', where setup K backsights 'R3' (record 4), the first that can \
             orient it, with a horizontal circle, and coordinates the file gives: this one is read \
             as a measurement of its point"
        ));
        let outcome = reduced(&known.project);
        let mut placed: BTreeMap<String, ComputedPoint> = BTreeMap::new();
        for point in &outcome.points {
            placed.insert(point.id.clone(), point.clone());
        }
        assert!(placed.contains_key("502") && placed.contains_key("R4"));
        near(placed["R4"].easting, 1050.0, POSITION_TOLERANCE);
        near(placed["R4"].northing, 5000.0, POSITION_TOLERANCE);
        near(placed["502"].easting, 1010.0, POSITION_TOLERANCE);
        near(placed["502"].northing, 5000.0, POSITION_TOLERANCE);
        assert_eq!(
            reduction_warnings_with(&outcome.report, "taken as a grid azimuth"),
            0
        );

        // Where the two disagree - R5, the first, given azimuth 45 at circle 30
        // (orientation 15); R6, due east of K, reading 120 (orientation
        // 90 - 120 = -30) - the first can orient the setup, so it does, and the
        // second, which has coordinates, is a check on it: radiated at bearing
        // 120 + 15 = 135, 50 m out, it misses R6's (1050, 5000) by
        // 50 cos 135 = -35.355 m north and 1000 + 50 sin 135 - 1050 = -14.645
        // m east. Nothing is turned silently.
        let disagree = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tR6\t\t\t1050.0\t5000.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tR5\t\t\t30.0\t90.0\t100.0\t45.0",
            "04\t\t\t\tR6\t\t\t120.0\t90.0\t50.0",
        ]);
        assert!(warned(
            &disagree,
            5,
            "a backsight to 'R6', where setup K backsights 'R5' (record 4)"
        ));
        let checked = reduced(&disagree.project);
        assert_eq!(checked.report.misclosures.len(), 1);
        let check = &checked.report.misclosures[0];
        assert_eq!(check.name, "R6 from setup K");
        let (Some(northing), Some(easting)) = (check.northing, check.easting) else {
            panic!("the check has its two plan components");
        };
        near(northing, 50.0 * degrees(135.0).cos(), POSITION_TOLERANCE);
        near(
            easting,
            1000.0 + 50.0 * degrees(135.0).sin() - 1050.0,
            POSITION_TOLERANCE,
        );
    }

    #[test]
    fn a_backsight_that_cannot_orient_its_setup_gives_way_to_a_later_one_that_can() {
        // A 04 orients its setup where it gives a horizontal circle to a point
        // with a position (coordinates the file gives, or a measurement from an
        // earlier setup) or states an azimuth. In each case below the first 04
        // cannot and the second can; the second names the backsight, and the
        // first is read as a measurement. With the first naming it, the setup
        // was oriented on the first's circle taken as a grid azimuth, or not
        // at all, although the second could orient it.
        //
        // K at (1000, 5000), R3 entered due north of it. R4, which nothing
        // places, reads 120, 50 m out; R3 reads 30: the orientation is
        // 0 - 30 = -30 degrees, so R4 is on bearing 90, at (1050, 5000), and
        // 502 at circle 120, 10 m, at (1010, 5000). R4 has no coordinates to
        // check, and R3 is the backsight: nothing misses. With R4 naming the
        // backsight the circle 120 was taken for its grid azimuth, and 502
        // drawn on bearing 120 and R3 reported 51.8 m out.
        let control = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tR4\t\t\t120.0\t90.0\t50.0",
            "04\t\t\t\tR3\t\t\t30.0\t90.0\t100.0",
            "07\t\tEB\t01\t502\t\t\t120.0\t90.0\t10.0",
        ]);
        assert_eq!(control.records_skipped, 0);
        assert_eq!(control.project.stations.len(), 1);
        let k = &control.project.stations[0];
        assert_eq!(k.backsight_point_id, "R3");
        near(
            k.backsight_azimuth.expect("a circle"),
            degrees(30.0),
            ANGLE_TOLERANCE,
        );
        assert!(warned(
            &control,
            4,
            "a backsight to 'R4', where setup K backsights 'R3' (record 5), the first that can \
             orient it, with a horizontal circle, and coordinates the file gives: this one is read \
             as a measurement of its point"
        ));
        let outcome = reduced(&control.project);
        assert!(outcome.report.misclosures.is_empty());
        assert_eq!(
            reduction_warnings_with(&outcome.report, "taken as a grid azimuth"),
            0
        );
        let mut placed: BTreeMap<String, ComputedPoint> = BTreeMap::new();
        for point in &outcome.points {
            placed.insert(point.id.clone(), point.clone());
        }
        assert!(placed.contains_key("502") && placed.contains_key("R4"));
        near(placed["R4"].easting, 1050.0, POSITION_TOLERANCE);
        near(placed["R4"].northing, 5000.0, POSITION_TOLERANCE);
        near(placed["502"].easting, 1010.0, POSITION_TOLERANCE);
        near(placed["502"].northing, 5000.0, POSITION_TOLERANCE);

        // The same with R3's coordinates after the setup, and after the next
        // one begins: the reduction places every 02 before any setup, so they
        // still orient it. Settled as each 04 is read, or as the setup ends,
        // neither backsight could have, and R4's circle would.
        let later = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tR4\t\t\t120.0\t90.0\t50.0",
            "04\t\t\t\tR3\t\t\t30.0\t90.0\t100.0",
            "07\t\tEB\t01\t502\t\t\t120.0\t90.0\t10.0",
            "03\t\t\t\tK\t\t\t1.5",
            "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
        ]);
        assert_eq!(later.project.stations.len(), 2);
        assert_eq!(later.project.stations[0].backsight_point_id, "R3");
        let afterward = reduced_positions(&later.project);
        assert!(afterward.contains_key("502"));
        near(afterward["502"].easting, 1010.0, POSITION_TOLERANCE);
        near(afterward["502"].northing, 5000.0, POSITION_TOLERANCE);

        // R7, which nothing places, reads 0 with no azimuth; R8, placed by
        // nothing either, reads 30 and is given azimuth 45: orientation
        // 45 - 30 = 15 degrees. 503 at circle 120 is on bearing 135,
        // (1000 + 10 sin 135, 5000 + 10 cos 135); R7 on bearing 15, 50 m out.
        // With R7 naming the backsight R8's azimuth was only kept in the
        // metadata and the circle 0 taken for R7's grid azimuth: 503 on
        // bearing 120.
        let stated = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tR7\t\t\t0.0\t90.0\t50.0",
            "04\t\t\t\tR8\t\t\t30.0\t90.0\t100.0\t45.0",
            "07\t\tEB\t01\t503\t\t\t120.0\t90.0\t10.0",
        ]);
        assert_eq!(stated.project.stations.len(), 1);
        let by_azimuth = &stated.project.stations[0];
        assert_eq!(by_azimuth.backsight_point_id, "R8");
        let (Some(circle), Some(azimuth)) = (
            by_azimuth.backsight_azimuth,
            by_azimuth.stated_backsight_azimuth,
        ) else {
            panic!("a circle and an azimuth");
        };
        near(circle, degrees(30.0), ANGLE_TOLERANCE);
        near(azimuth, degrees(45.0), ANGLE_TOLERANCE);
        assert_eq!(
            value_of(&by_azimuth.metadata, "backsight azimuth record 4"),
            "(absent)"
        );
        assert!(warned(
            &stated,
            3,
            "a backsight to 'R7', where setup K backsights 'R8' (record 4), the first that can \
             orient it, with a horizontal circle and a stated azimuth"
        ));
        let by_stated = reduced_positions(&stated.project);
        assert!(by_stated.contains_key("503") && by_stated.contains_key("R7"));
        near(
            by_stated["503"].easting,
            1000.0 + 10.0 * degrees(135.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            by_stated["503"].northing,
            5000.0 + 10.0 * degrees(135.0).cos(),
            POSITION_TOLERANCE,
        );
        near(
            by_stated["R7"].easting,
            1000.0 + 50.0 * degrees(15.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            by_stated["R7"].northing,
            5000.0 + 50.0 * degrees(15.0).cos(),
            POSITION_TOLERANCE,
        );

        // R3 entered due north, with no horizontal circle read on it; R6
        // entered due east, reading 120: orientation 90 - 120 = -30, so 504 at
        // circle 120 is at (1010, 5000). With R3 naming the backsight the
        // setup had no direction to orient on, and 504 was not drawn.
        let no_circle = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
            "02\t\t\t\tR6\t\t\t1050.0\t5000.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tR3\t\t\t\t90.0\t100.0",
            "04\t\t\t\tR6\t\t\t120.0\t90.0\t50.0",
            "07\t\tEB\t01\t504\t\t\t120.0\t90.0\t10.0",
        ]);
        assert_eq!(no_circle.records_skipped, 0);
        assert_eq!(no_circle.project.stations.len(), 1);
        assert_eq!(no_circle.project.stations[0].backsight_point_id, "R6");
        let by_r6 = reduced_positions(&no_circle.project);
        assert!(by_r6.contains_key("504"));
        near(by_r6["504"].easting, 1010.0, POSITION_TOLERANCE);
        near(by_r6["504"].northing, 5000.0, POSITION_TOLERANCE);

        // A traverse: K, oriented on R3 at circle 0, shoots T1 at circle 90
        // and T2 at circle 180, 10 m each: (1010, 5000) and (1000, 4990). The
        // setup on T1 first reads X, which nothing places, at 50, then T2,
        // which K measured, at 135: T2 is on bearing 225 from T1, so the
        // orientation is 225 - 135 = 90 degrees. 505 at circle 0, 10 m, is on
        // bearing 90 from T1, at (1020, 5000); X at circle 50, 20 m, on bearing
        // 140. With X naming the backsight its circle was taken for its grid
        // azimuth: 505 on bearing 0.
        let traverse = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tR3\t\t\t0.0\t90.0\t100.0",
            "07\t\tEB\t01\tT1\t\t\t90.0\t90.0\t10.0",
            "07\t\tEB\t01\tT2\t\t\t180.0\t90.0\t10.0",
            "03\t\t\t\tT1\t\t\t1.5",
            "04\t\t\t\tX\t\t\t50.0\t90.0\t20.0",
            "04\t\t\t\tT2\t\t\t135.0\t90.0\t14.142135623731",
            "07\t\tEB\t02\t505\t\t\t0.0\t90.0\t10.0",
        ]);
        assert_eq!(traverse.records_skipped, 0);
        assert_eq!(traverse.project.stations.len(), 2);
        assert_eq!(traverse.project.stations[1].setup.point_id, "T1");
        assert_eq!(traverse.project.stations[1].backsight_point_id, "T2");
        assert!(warned(
            &traverse,
            8,
            "a backsight to 'X', where setup T1 backsights 'T2' (record 9), the first that can \
             orient it, with a horizontal circle, and measured from setup K"
        ));
        let by_traverse = reduced_positions(&traverse.project);
        assert!(by_traverse.contains_key("505") && by_traverse.contains_key("X"));
        near(by_traverse["505"].easting, 1020.0, POSITION_TOLERANCE);
        near(by_traverse["505"].northing, 5000.0, POSITION_TOLERANCE);
        near(
            by_traverse["X"].easting,
            1010.0 + 20.0 * degrees(140.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            by_traverse["X"].northing,
            5000.0 + 20.0 * degrees(140.0).cos(),
            POSITION_TOLERANCE,
        );
    }

    #[test]
    fn where_no_backsight_can_orient_its_setup_the_first_with_a_circle_names_it() {
        // R3 is entered but read with no horizontal circle; R9, which nothing
        // places, reads 40 with no azimuth. Neither can orient the setup, so
        // the first with a circle names the backsight and the reduction takes
        // that circle as set to the grid, saying so: orientation 40 - 40 = 0,
        // and 507 at circle 130, 10 m, is on bearing 130. The first 04 naming
        // it left the setup with no direction to orient on, and nothing was
        // drawn.
        let result = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tR3\t\t\t\t90.0\t100.0",
            "04\t\t\t\tR9\t\t\t40.0\t90.0\t50.0",
            "07\t\tEB\t01\t507\t\t\t130.0\t90.0\t10.0",
        ]);
        assert_eq!(result.project.stations.len(), 1);
        assert_eq!(result.project.stations[0].backsight_point_id, "R9");
        assert!(warned(
            &result,
            4,
            "a backsight to 'R3', where setup K backsights 'R9' (record 5), the first with a \
             horizontal circle, as none with one is to a point with a position or states an \
             azimuth: this one is read as a measurement of its point"
        ));
        let outcome = reduced(&result.project);
        assert_eq!(
            reduction_warnings_with(&outcome.report, "taken as a grid azimuth"),
            1
        );
        let shot = outcome
            .points
            .iter()
            .find(|p| p.id == "507")
            .expect("507 is placed");
        near(
            shot.easting,
            1000.0 + 10.0 * degrees(130.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            shot.northing,
            5000.0 + 10.0 * degrees(130.0).cos(),
            POSITION_TOLERANCE,
        );
    }

    // ---- Layouts -----------------------------------------------------------------------------

    #[test]
    fn a_file_without_the_empty_column_is_read_by_its_counts() {
        // [FLD]'s own syntax: the description straight after the opcode. The
        // 07 with a feature code decides it; the 02 and 03 with none fit
        // either.
        let bytes = lines(&[
            "100\tdegrees\tmetres",
            "02\t\t\tA\t\t\t10.0\t20.0\t1.0",
            "03\t\t\tA\t\t\t1.5",
            "07\tTREE\t\t7\t\t\t90.0\t90.0\t12.5",
        ]);
        let read = topcon_test::read(ID, bytes.as_bytes(), "plain.fld")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let a = topcon_test::point(&read.project, "A").expect("A");
        double_eq(a.easting, 10.0);
        double_eq(a.northing, 20.0);
        assert_eq!(read.project.stations.len(), 1);
        let station = &read.project.stations[0];
        let directions = observations_to::<HorizontalDirectionObservation>(station, "7");
        assert_eq!(directions.len(), 1);
        near(directions[0].direction, PI / 2.0, ANGLE_TOLERANCE);
        let distances = observations_to::<DistanceObservation>(station, "7");
        assert_eq!(distances.len(), 1);
        double_eq(distances[0].distance, 12.5);
        assert_eq!(
            topcon_test::unpositioned(&read.project, "7").unwrap().code,
            "TREE"
        );
        assert_eq!(read.records_skipped, 0);
    }

    #[test]
    fn a_backsight_of_ten_fields_fits_either_layout_and_decides_nothing() {
        // [FLD]'s syntax with an uncoded backsight that gives its azimuth: ten
        // fields, the count of a backsight in the column's layout without one.
        // It decides nothing; the coded 07 decides, and every record is read
        // as [FLD] writes it.
        let result = read_lines(&[
            "100\tdegrees\tmetres",
            "02\t\t\tA\t\t\t10.0\t20.0\t1.0",
            "02\t\t\tB\t\t\t10.0\t120.0\t1.0",
            "03\t\t\tA\t\t\t1.5",
            "04\t\t\tB\t\t\t0.0\t90.0\t100.0\t0.0",
            "07\tTREE\t\t7\t\t\t90.0\t90.0\t12.5",
        ]);
        assert_eq!(result.records_skipped, 0);
        assert!(result.warnings.is_empty(), "{}", result.warnings[0].message);
        assert_eq!(result.project.stations.len(), 1);
        let station = &result.project.stations[0];
        assert_eq!(station.backsight_point_id, "B");
        near(
            station.backsight_azimuth.expect("a circle"),
            0.0,
            ANGLE_TOLERANCE,
        );
        near(
            station.stated_backsight_azimuth.expect("an azimuth"),
            0.0,
            ANGLE_TOLERANCE,
        );
        assert_eq!(
            observations_to::<DistanceObservation>(station, "7")[0].distance,
            12.5
        );

        // With no coded record at all nothing decides, and the file is read as
        // [FLD] writes it.
        let uncoded = read_lines(&[
            "02\t\t\tA\t\t\t10.0\t20.0\t1.0",
            "02\t\t\tB\t\t\t10.0\t120.0\t1.0",
            "03\t\t\tA\t\t\t1.5",
            "04\t\t\tB\t\t\t0.0\t90.0\t100.0\t0.0",
            "07\t\t\t8\t\t\t90.0\t90.0\t12.5",
        ]);
        assert_eq!(uncoded.records_skipped, 0);
        assert_eq!(uncoded.project.stations.len(), 1);
        assert_eq!(uncoded.project.stations[0].backsight_point_id, "B");
        assert_eq!(
            observations_to::<DistanceObservation>(&uncoded.project.stations[0], "8").len(),
            1
        );
    }

    #[test]
    fn in_a_file_of_both_layouts_a_blank_first_field_takes_the_layout_most_records_have() {
        // Coded records of both layouts - records 1 to 3 without the column,
        // record 4 with it - then a backsight of ten fields with an empty
        // first: the column and no azimuth, or no column and the azimuth.
        let result = read_lines(&[
            "02\tK\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
            "02\tK\t\tCP2\t\t\t1000.0\t5100.0\t20.0",
            "03\tK\t\tCP1\t\t\t1.5",
            "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
            "04\t\t\t\tCP2\t\t\t0.0\t90.0\t100.0",
        ]);
        assert!(warned(
            &result,
            0,
            "1 fixed record(s) have the column after the opcode (blank or a date or time) and 3 \
             do not: each is read in the layout its fields show, and one whose first field is \
             blank in the layout most have, without it"
        ));
        // Record 4's first field is blank, and it does not fit the layout most
        // records have.
        assert!(warned(
            &result,
            4,
            "opcode 07 has 9 values where the format gives 8"
        ));
        // Record 5 read without the column has no horizontal circle; with it,
        // a whole backsight: which wrote it cannot be told.
        assert!(warned(
            &result,
            5,
            "opcode 04 has 9 values, which fit the format's layout both with the column after the \
             opcode and without it: read without it, as this file's records are, the horizontal \
             circle is blank, and read with it, it is not"
        ));
        assert_eq!(result.project.stations.len(), 1);
        assert!(result.project.stations[0].backsight_point_id.is_empty());
    }

    #[test]
    fn a_backsight_written_with_the_column_in_a_file_without_it_is_skipped_not_read_shifted() {
        // [FLD]'s layout, decided by its coded records, and one backsight of
        // ten fields written with the column: read without it, its horizontal
        // circle is blank and 0, 90 and 100 become its zenith, slope distance
        // and azimuth - an azimuth of 100 that turned the setup's shots, with
        // no warning. Read with it, it is whole. It is skipped, saying so.
        let result = read_lines(&[
            "02\tK\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
            "02\tK\t\tCP2\t\t\t1000.0\t5100.0\t20.0",
            "03\tK\t\tCP1\t\t\t1.5",
            "04\t\t\t\tCP2\t\t\t0.00000000\t90.00000000\t100.000",
            "07\tEB\t01\t101\t\t\t45.0\t95.0\t50.0",
        ]);
        assert!(warned(
            &result,
            4,
            "read without it, as this file's records are, the horizontal circle is blank, and read \
             with it, it is not; which value is which cannot be told"
        ));
        assert_eq!(result.records_skipped, 1);
        assert_eq!(result.project.stations.len(), 1);
        assert!(result.project.stations[0].backsight_point_id.is_empty());
        assert!(
            result.project.stations[0]
                .stated_backsight_azimuth
                .is_none()
        );
        assert!(!reduced_positions(&result.project).contains_key("101"));
    }

    #[test]
    fn a_record_whose_other_layout_reads_no_whole_record_is_read_in_the_files_layout() {
        // A job with the column after the opcode, and records that fit both
        // layouts with their first measured value blank in it. Read without
        // the column, each would be no record: the setup on A has "nail in
        // kerb" where its instrument height would be; the setup on B (its name
        // A's place, its comment a number) names no point; the backsight's
        // comment is where its horizontal circle would be. That is no sign
        // which layout wrote them, and each is read as the file's records are
        // - where any value there had them skipped, and every measurement after
        // a setup.
        //
        // A (1000, 5000) oriented on B, due north at circle 0: 101 at circle
        // 90, level, 10 m, is at (1010, 5000), 102 at circle 180 at
        // (1000, 4990). B oriented on A, due south at circle 180 (orientation
        // 0): 103 at circle 90, 10 m, at (1010, 5100). Neither setup states a
        // height: 0 is used.
        let result = read_lines(&[
            "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tB\t\t\t1000.0\t5100.0\t21.0",
            "03\t\t\t\tA\t\tnail in kerb\t",
            "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
            "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
            "07\t\tEB\t01\t102\t\t\t180.0\t90.0\t10.0",
            "03\t\t\t\t\tB\t1.5\t",
            "04\t\t\t\tA\t\t\t180.0\t90.0\t100.0",
            "04\t\t\t\tA\t\tbs nail\t\t90.0\t100.0",
            "07\t\tEB\t02\t103\t\t\t90.0\t90.0\t10.0",
        ]);
        assert_eq!(result.records_skipped, 0);
        assert!(warned(
            &result,
            3,
            "the setup states no instrument height; 0 is used"
        ));
        assert!(warned(
            &result,
            7,
            "the setup states no instrument height; 0 is used"
        ));
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(result.project.stations[0].setup.point_id, "A");
        assert_eq!(result.project.stations[1].setup.point_id, "B");
        // The comments describe the points the setups stand on.
        assert_eq!(
            topcon_test::point(&result.project, "A")
                .unwrap()
                .description,
            "nail in kerb"
        );
        assert_eq!(
            topcon_test::point(&result.project, "B")
                .unwrap()
                .description,
            "1.5"
        );
        // The backsight of record 9 has no horizontal circle, and a zenith and
        // a distance to A.
        assert_eq!(
            observations_to::<DistanceObservation>(&result.project.stations[1], "A").len(),
            2
        );
        let positions = reduced_positions(&result.project);
        assert_eq!(
            usize::from(positions.contains_key("101"))
                + usize::from(positions.contains_key("102"))
                + usize::from(positions.contains_key("103")),
            3
        );
        near(positions["101"].easting, 1010.0, POSITION_TOLERANCE);
        near(positions["101"].northing, 5000.0, POSITION_TOLERANCE);
        near(positions["102"].easting, 1000.0, POSITION_TOLERANCE);
        near(positions["102"].northing, 4990.0, POSITION_TOLERANCE);
        near(positions["103"].easting, 1010.0, POSITION_TOLERANCE);
        near(positions["103"].northing, 5100.0, POSITION_TOLERANCE);
    }

    #[test]
    fn a_record_written_without_the_column_in_a_job_with_it_is_read_by_its_fields() {
        // A job with the column, its backsights of ten fields, and one shot
        // written without it: its feature code first says so, and it is read.
        // Before, the one dissenting record made every record that fits both
        // layouts - both backsights - be skipped, and no setup was oriented.
        let result = read_lines(&[
            "02\t\t\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tCP2\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tCP1\t\t\t1.5",
            "04\t\t\t\tCP2\t\t\t0.0\t90.0\t100.0",
            "07\t\tEB\t01\t101\t\t\t45.0\t90.0\t50.0",
            "07\tEB\t01\t103\t\t\t47.0\t90.0\t50.0",
            "03\t\t\t\tCP2\t\t\t1.5",
            "04\t\t\t\tCP1\t\t\t180.0\t90.0\t100.0",
            "07\t\tEB\t01\t102\t\t\t135.0\t90.0\t50.0",
        ]);
        assert_eq!(result.records_skipped, 0);
        assert!(warned(
            &result,
            0,
            "6 fixed record(s) have the column after the opcode (blank or a date or time) and 1 \
             do not"
        ));
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(result.project.stations[0].backsight_point_id, "CP2");
        assert_eq!(result.project.stations[1].backsight_point_id, "CP1");
        // CP1 reads 0 on CP2, due north: orientation 0; CP2 reads 180 on CP1,
        // due south: orientation 0. Zeniths of 90, so each shot is 50 m out.
        let positions = reduced_positions(&result.project);
        assert_eq!(
            usize::from(positions.contains_key("101"))
                + usize::from(positions.contains_key("102"))
                + usize::from(positions.contains_key("103")),
            3
        );
        near(
            positions["101"].easting,
            1000.0 + 50.0 * degrees(45.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            positions["101"].northing,
            5000.0 + 50.0 * degrees(45.0).cos(),
            POSITION_TOLERANCE,
        );
        near(
            positions["103"].easting,
            1000.0 + 50.0 * degrees(47.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            positions["103"].northing,
            5000.0 + 50.0 * degrees(47.0).cos(),
            POSITION_TOLERANCE,
        );
        near(
            positions["102"].easting,
            1000.0 + 50.0 * degrees(135.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            positions["102"].northing,
            5100.0 + 50.0 * degrees(135.0).cos(),
            POSITION_TOLERANCE,
        );
    }

    #[test]
    fn in_a_file_no_record_decides_free_records_are_read_as_the_format_writes_them_as_the_fixed_ones_are()
     {
        // Uncoded records in [FLD]'s layout: none decides (a blank first field
        // is an empty feature code, or the column), so the file is read as
        // [FLD] writes it - its unnamed attributes, and its offset naming a
        // point by its ID, too. The column taken for them had made attributes
        // named by their values and an offset one value short.
        let result = read_lines(&[
            "100\tdegrees\tmetres",
            "02\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\tK\t\t\t1.5",
            "04\t\t\tB\t\t\t0.0\t90.0\t100.0",
            "07\t\t\tP5\t\t\t90.0\t90.0\t10.0",
            "73\t\tfence type A",
            "72\t\t0.450",
            "42\t\t\tP5\t\t\t0.500",
        ]);
        assert_eq!(result.records_skipped, 0);
        let p5 = metadata_of(&result.project, "P5");
        assert_eq!(
            value_of(&p5, "unnamed attribute (record 7)"),
            "fence type A"
        );
        assert_eq!(value_of(&p5, "unnamed attribute (record 8)"), "0.450");
        assert!(!p5.contains_key("fence type A"));
        // K oriented on B, due north at circle 0; P5 10 m east, then 0.5 m
        // further out.
        let positions = reduced_positions(&result.project);
        assert!(positions.contains_key("P5"));
        near(positions["P5"].easting, 1010.5, POSITION_TOLERANCE);
        near(positions["P5"].northing, 5000.0, POSITION_TOLERANCE);
    }

    #[test]
    fn a_feature_code_shaped_like_a_number_range_is_a_code_not_a_date() {
        // "1-2" is no date: a backsight of [FLD]'s layout coded so is read as
        // one, where it was taken to have the column and skipped for naming no
        // point.
        let result = read_lines(&[
            "02\tK\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
            "03\tK\t\tCP1\t\t\t1.5",
            "04\t1-2\t\tBSX\t\t\t30.0\t90.0\t100.000\t45.0",
        ]);
        assert_eq!(result.records_skipped, 0);
        let station = &result.project.stations[0];
        assert_eq!(station.backsight_point_id, "BSX");
        let (Some(circle), Some(azimuth)) =
            (station.backsight_azimuth, station.stated_backsight_azimuth)
        else {
            panic!("a circle and an azimuth");
        };
        near(circle, degrees(30.0), ANGLE_TOLERANCE);
        near(azimuth, degrees(45.0), ANGLE_TOLERANCE);
        assert_eq!(
            value_of(&station.metadata, "backsight record 3"),
            "feature code 1-2"
        );
    }

    #[test]
    fn a_fixed_record_that_fits_neither_layout_in_a_file_no_record_decided_says_how_it_was_read() {
        let result = read_lines(&[
            "03\t\t\tS\t\t\t1.5",
            "07\t\t\t1\t\t\t10.0\t90.0", // uncoded, and a value short of either layout
        ]);
        assert!(warned(
            &result,
            2,
            "opcode 07 has 7 values where the format gives 8 (no record shows the column after \
             the opcode as blank or a date or time, so the file is read without it)"
        ));
        assert_eq!(result.records_skipped, 1);
    }

    #[test]
    fn a_file_without_the_column_reads_a_record_of_fixed_form_that_has_it() {
        // [FLD]'s layout, decided by its coded records, and a target height, a
        // scale factor and the units written with the column: an empty value
        // before a value those records never have, so it is the column.
        let result = read_lines(&[
            "100\t\tdegrees\tmetres\tmillibars\tcelsius",
            "02\tK\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
            "03\tK\t\tCP1\t\t\t1.5",
            "05\t\t2.500",
            "09\t\t0.5",
            "07\tEB\t01\t701\t\t\t10.0\t95.0\t50.0",
        ]);
        assert_eq!(result.records_skipped, 0);
        assert_eq!(result.project.stations.len(), 1);
        let station = &result.project.stations[0];
        let zeniths = observations_to::<ZenithAngleObservation>(station, "701");
        assert_eq!(zeniths.len(), 1);
        double_eq(zeniths[0].target_height, 2.5);
        let distances = observations_to::<DistanceObservation>(station, "701");
        assert_eq!(distances.len(), 1);
        double_eq(distances[0].distance, 25.0); // 50 x 0.5
    }

    #[test]
    fn a_trailing_tab_is_not_a_value() {
        // Every fixed record ends with a tab, as the files' 128 does: one blank
        // field past the count, which the records are read without.
        let result = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0\t",
            "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0\t",
            "03\t\t\t\tK\t\t\t1.5\t",
            "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0\t",
            "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0\t",
        ]);
        assert_eq!(result.records_skipped, 0);
        assert_eq!(result.project.points.len(), 2);
        assert_eq!(result.project.stations.len(), 1);
        assert_eq!(result.project.stations[0].backsight_point_id, "B");
        double_eq(result.project.stations[0].setup.instrument_height, 1.5);
        assert_eq!(
            observations_to::<DistanceObservation>(&result.project.stations[0], "1")[0].distance,
            5.0
        );
    }

    // ---- Offsets ------------------------------------------------------------------------------

    #[test]
    fn offsets_put_their_point_where_the_format_says() {
        // S at (1000, 5000, 20), instrument and target 1.5 m, oriented on B due
        // north at circle 0: circle 90 is east, 0 north. [FLD] 44.8: a radial
        // offset along the line from the station, positive away; a tangential
        // one at right angles, negative to the left looking from the station;
        // a height offset added to the height.
        let result = read_lines(&[
            "02\t\t\t\tS\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tS\t\t\t1.5",
            "05\t\t1.5",
            "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
            "07\t\tEP\t1\tP1\t\t\t90.0\t90.0\t10.0",
            "42\t\t0.5",
            "07\t\tEP\t1\tP2\t\t\t90.0\t90.0\t20.0",
            "43\t\t0.5",
            "07\t\tEP\t1\tP3\t\t\t0.0\t90.0\t30.0",
            "43\t\t-0.5",
            "42\t\t-1.0",
            "07\t\tEP\t1\tP4\t\t\t45.0\t90.0\t10.0",
            "44\t\t0.25",
        ]);
        assert!(result.warnings.is_empty(), "{}", result.warnings[0].message);
        let positions = reduced_positions(&result.project);
        assert_eq!(
            usize::from(positions.contains_key("P1"))
                + usize::from(positions.contains_key("P2"))
                + usize::from(positions.contains_key("P3"))
                + usize::from(positions.contains_key("P4")),
            4
        );
        // P1: 10 m east, then 0.5 m further out.
        near(positions["P1"].easting, 1010.5, POSITION_TOLERANCE);
        near(positions["P1"].northing, 5000.0, POSITION_TOLERANCE);
        // P2: 20 m east, then 0.5 m to the right looking east: south.
        near(positions["P2"].easting, 1020.0, POSITION_TOLERANCE);
        near(positions["P2"].northing, 4999.5, POSITION_TOLERANCE);
        // P3: 30 m north, 0.5 m to the left looking north (west) and 1 m back
        // toward the station - each from the measured position, in either
        // order.
        near(positions["P3"].easting, 999.5, POSITION_TOLERANCE);
        near(positions["P3"].northing, 5029.0, POSITION_TOLERANCE);
        // P4: 10 m on bearing 45, its plan position unmoved, 0.25 m higher
        // than the station's 20 + 1.5 - 1.5.
        near(
            positions["P4"].easting,
            1000.0 + 10.0 * (PI / 4.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            positions["P4"].northing,
            5000.0 + 10.0 * (PI / 4.0).cos(),
            POSITION_TOLERANCE,
        );
        near(
            positions["P4"].elevation.expect("a height"),
            20.25,
            POSITION_TOLERANCE,
        );
    }

    #[test]
    fn an_offset_moves_the_point_it_names_and_one_that_cannot_be_read_is_skipped_saying_why() {
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
            "07\t\tEP\t1\t2\t\t\t20.0\t90.0\t5.0",
            "44\t\t0.1",              // height offset of the current point, 2
            "43\t\t\t\t1\t\t\t-0.3",  // tangential offset of point 1, by its ID
            "42\t\tEP\t1\t\t\t\t0.2", // radial offset of EP 1's last point, 2
            "42\t\t0.5\t0.0\t0.0",    // three values: neither of the two forms
            "42\t\tabc",              // not a number
            "42\t\tXX\t9\t\t\t\t0.1", // a string that is not open
            "42\t\t\t\tZZ\t\t\t0.1",  // a point no record made
        ]);
        let p1 = metadata_of(&result.project, "1");
        let p2 = metadata_of(&result.project, "2");
        assert_eq!(
            value_of(&p2, "offset record 4"),
            "height 0.1 m from setup S, applied to the shot of record 3"
        );
        assert_eq!(
            value_of(&p1, "offset record 5"),
            "tangential -0.3 m from setup S, applied to the shot of record 2"
        );
        assert_eq!(
            value_of(&p2, "offset record 6"),
            "radial 0.2 m from setup S, applied to the shot of record 3"
        );
        assert!(warned(
            &result,
            7,
            "with 3 value(s) where the format gives the offset"
        ));
        assert!(warned(
            &result,
            8,
            "radial offset (opcode 42) with no offset"
        ));
        assert!(warned(
            &result,
            9,
            "radial offset (opcode 42): there is no open string XX 9"
        ));
        assert!(warned(
            &result,
            10,
            "names no string and no point a record before it made"
        ));
        assert_eq!(result.records_skipped, 4);
        // Point 2 (circle 20, zenith 90, 5 m): 0.2 m further out and 0.1 m up,
        // so 5.2 m out on the same circle, rising 0.1 m. Point 1 (circle 10,
        // 5 m): 0.3 m to the left, the circle less atan2(0.3, 5).
        let station = &result.project.stations[0];
        let distance2 = observations_to::<DistanceObservation>(station, "2");
        assert_eq!(distance2.len(), 1);
        near(distance2[0].distance, 5.2_f64.hypot(0.1), 1e-12);
        let zenith2 = observations_to::<ZenithAngleObservation>(station, "2");
        assert_eq!(zenith2.len(), 1);
        near(zenith2[0].angle, 5.2_f64.atan2(0.1), ANGLE_TOLERANCE);
        let direction1 = observations_to::<HorizontalDirectionObservation>(station, "1");
        assert_eq!(direction1.len(), 1);
        near(
            direction1[0].direction,
            degrees(10.0) - 0.3_f64.atan2(5.0),
            ANGLE_TOLERANCE,
        );
    }

    #[test]
    fn an_offset_of_a_point_measured_twice_or_not_shot_from_the_setup_is_kept_not_applied() {
        let result = read_lines(&[
            "02\t\t\t\tC\t\t\t1000.0\t5000.0\t20.0",
            "42\t\t0.5", // an entered coordinate
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\tA\t\t\t10.0\t90.0\t5.0",
            "07\t\tEP\t1\tA\t\t\t190.0\t270.0\t5.0",
            "43\t\t0.5", // A is shot twice from S: [FLD] moves the second, a point of its own
            "07\t\tEP\t1\tD\t\t\t30.0\t90.0\t5.0",
            "03\t\t\t\tT\t\t\t1.5",
            "43\t\t\t\tD\t\t\t0.5", // D was shot from S, not from T
            "07\t\tEP\t1\tE\t\t\t90.0\t90.0\t1.0",
            "42\t\t-1.0", // 1 m back toward T: T's own mark, which no shot can see
        ]);
        assert!(warned(
            &result,
            2,
            "is kept in its metadata and not applied: point 'C' is the coordinate of record 1, \
             which is kept as the file states it"
        ));
        assert!(warned(
            &result,
            6,
            "is kept in its metadata and not applied: point 'A' was measured 2 times from setup \
             S: the format moves only the measurement an offset follows, a point of its own there, \
             and Katana, which makes one point of them, would put it between the moved and the \
             unmoved"
        ));
        assert!(warned(
            &result,
            9,
            "is kept in its metadata and not applied: point 'D' was not measured (opcode 07) from \
             the current setup"
        ));
        assert_eq!(
            value_of(&metadata_of(&result.project, "A"), "offset record 6"),
            "tangential 0.5 m from setup S, not applied"
        );
        assert_eq!(
            value_of(&metadata_of(&result.project, "D"), "offset record 9"),
            "tangential 0.5 m, not applied"
        );
        assert!(warned(
            &result,
            11,
            "is kept in its metadata and not applied: with the other offsets of its shot it puts \
             point 'E' on the station"
        ));
        assert_eq!(
            value_of(&metadata_of(&result.project, "E"), "offset record 11"),
            "radial -1 m from setup T, not applied"
        );
        // A's and E's shots are as measured.
        let to_a =
            observations_to::<HorizontalDirectionObservation>(&result.project.stations[0], "A");
        assert_eq!(to_a.len(), 2);
        near(to_a[0].direction, degrees(10.0), ANGLE_TOLERANCE);
        let to_e = observations_to::<DistanceObservation>(&result.project.stations[1], "E");
        assert_eq!(to_e.len(), 1);
        double_eq(to_e[0].distance, 1.0);
        assert!(!metadata_of(&result.project, "E").contains_key("shot record 10 as measured"));
        assert_eq!(result.records_skipped, 0);
    }

    #[test]
    fn two_offsets_of_one_kind_on_a_shot_are_kept_and_none_of_its_offsets_is_applied() {
        // Whether the second radial offset adds to the first or replaces it,
        // each being "from the specified points original position", the format
        // does not say: the shot is left as measured, its tangential offset
        // too.
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\tP1\t\t\t90.0\t90.0\t10.0",
            "42\t\t0.5",
            "42\t\t0.3",
            "43\t\t0.1",
        ]);
        for record in [3, 4, 5] {
            assert!(
                warned(
                    &result,
                    record,
                    "the shot of record 2 has two offsets of one kind, and whether the second adds \
                     to the first or replaces it the format does not say"
                ),
                "{record}"
            );
        }
        let station = &result.project.stations[0];
        double_eq(
            observations_to::<DistanceObservation>(station, "P1")[0].distance,
            10.0,
        );
        near(
            observations_to::<HorizontalDirectionObservation>(station, "P1")[0].direction,
            PI / 2.0,
            ANGLE_TOLERANCE,
        );
        assert_eq!(result.records_skipped, 0);
    }

    #[test]
    fn an_offset_of_a_shot_without_a_slope_distance_or_a_zenith_is_kept_not_applied() {
        // Shot 1 has no slope distance (its last field blank), shot 2 no
        // zenith: neither has a plan distance or a height to offset.
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t",
            "43\t\t0.2",
            "07\t\tEP\t1\t2\t\t\t20.0\t\t5.0",
            "42\t\t0.3",
        ]);
        assert!(warned(
            &result,
            3,
            "is kept in its metadata and not applied: the shot of record 2 has no slope distance \
             or no zenith, so the point has no plan distance or height to offset"
        ));
        assert!(warned(
            &result,
            5,
            "the shot of record 4 has no slope distance or no zenith"
        ));
        assert_eq!(
            value_of(&metadata_of(&result.project, "1"), "offset record 3"),
            "tangential 0.2 m from setup S, not applied"
        );
        let station = &result.project.stations[0];
        near(
            observations_to::<HorizontalDirectionObservation>(station, "1")[0].direction,
            degrees(10.0),
            ANGLE_TOLERANCE,
        );
        double_eq(
            observations_to::<DistanceObservation>(station, "2")[0].distance,
            5.0,
        );
        assert_eq!(result.records_skipped, 0);
    }

    // ---- Records that are not read, and what follows them -----------------------------------

    #[test]
    fn attributes_after_a_shot_that_was_not_read_are_not_given_to_the_point_before() {
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
            "73\t\tDate\td1",
            "07\t\tEB\t01\t2\t\t\t20.0\t90.0", // a value short
            "73\t\tDate\td2",
            "72\t\tDepth\t0.6",
        ]);
        let p1 = metadata_of(&result.project, "1");
        assert_eq!(value_of(&p1, "Date"), "d1");
        assert!(!p1.contains_key("Depth"));
        assert!(warned(
            &result,
            5,
            "of the point of record 4, which was not read"
        ));
        assert!(warned(
            &result,
            6,
            "of the point of record 4, which was not read"
        ));
        assert_eq!(result.records_skipped, 3);
    }

    #[test]
    fn what_acts_on_the_measurement_after_one_that_was_not_read_is_skipped_naming_it() {
        // A multiple coding, an offset and a bare close after a shot a value
        // short: each would act on point 1, the measurement before.
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
            "07\t\tEP\t1\t2\t\t\t20.0\t90.0", // a value short
            "16\t\tNS\t01\t\t\t",
            "42\t\t0.5",
            "20",
        ]);
        assert!(warned(
            &result,
            4,
            "multiple coding (opcode 16) of the measurement of record 3, which was not read"
        ));
        assert!(warned(
            &result,
            5,
            "radial offset (opcode 42) of the measurement of record 3, which was not read"
        ));
        assert!(warned(
            &result,
            6,
            "close string (opcode 20) of the measurement of record 3, which was not read"
        ));
        assert!(topcon_test::feature(&result.project, "NS").is_none());
        assert_eq!(features_of(&result.project, "EP").len(), 1);
        assert!(!features_of(&result.project, "EP")[0].closed);
        double_eq(
            observations_to::<DistanceObservation>(&result.project.stations[0], "1")[0].distance,
            5.0,
        );
        assert_eq!(result.records_skipped, 4);
    }

    #[test]
    fn a_line_that_is_no_record_leaves_nothing_current_so_what_follows_is_not_given_to_the_point_before()
     {
        // What a line that is no record made is not known: what follows it and
        // would describe that is skipped, naming the line, not given to P1 or
        // P3 - its date, an offset that moved P1's shot, a second string, a
        // close. A setup written with a plus leaves no setup for P4's shot.
        let result = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
            "07\t\tEP\t1\tP1\t\t\t90.0\t90.0\t10.0",
            "0007\t\tEP\t1\tP2\t\t\t180.0\t90.0\t10.0", // 6: four digits
            "73\t\tDate\td2",
            "42\t\t0.5",
            "16\t\tNS\t01\t\t\t",
            "20",
            "07\t\tEP\t1\tP3\t\t\t0.0\t90.0\t10.0", // 11
            "O7 written with a letter",             // 12: no number at all
            "72\t\tDepth\t9.9",
            "+3\t\t\t\tB\t\t\t1.5", // 14: a setup, with a plus
            "07\t\tEP\t1\tP4\t\t\t270.0\t90.0\t10.0",
        ]);
        assert!(warned(&result, 6, "'0007' is not an opcode"));
        assert!(warned(
            &result,
            7,
            "an attribute of the point of record 6, which was not read"
        ));
        assert!(warned(
            &result,
            8,
            "radial offset (opcode 42) of the measurement of record 6, which was not read"
        ));
        assert!(warned(
            &result,
            9,
            "of the measurement of record 6, which was not read"
        ));
        assert!(warned(
            &result,
            10,
            "of the measurement of record 6, which was not read"
        ));
        assert!(warned(
            &result,
            12,
            "is not an opcode: a record opens with a number"
        ));
        assert!(warned(
            &result,
            13,
            "an attribute of the point of record 12, which was not read"
        ));
        assert!(warned(
            &result,
            14,
            "it is taken for the setup (opcode 03) it would be; the measurements after it are not \
             read until the next setup"
        ));
        assert!(warned(
            &result,
            15,
            "measurement from the setup of record 14, which was not read"
        ));
        assert!(!metadata_of(&result.project, "P1").contains_key("Date"));
        assert!(!metadata_of(&result.project, "P3").contains_key("Depth"));
        assert!(topcon_test::feature(&result.project, "NS").is_none());
        assert!(!features_of(&result.project, "EP")[0].closed);
        assert_eq!(result.project.stations.len(), 1);
        let station = &result.project.stations[0];
        double_eq(
            observations_to::<DistanceObservation>(station, "P1")[0].distance,
            10.0,
        );
        assert!(observations_to::<DistanceObservation>(station, "P4").is_empty());
        assert_eq!(result.records_skipped, 9);
    }

    #[test]
    fn a_check_coordinate_or_an_opcode_the_format_does_not_define_ends_the_current_point() {
        // 14 checks a named coordinate: the note after it is the check's. What
        // an undefined opcode made is not known: the attribute after it is not
        // given to the shot before either.
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\t101\t\t\t10.0\t90.0\t5.0",
            "14\t\t\t\t\tCP2\t\t1000.010\t5100.020\t21.510",
            "73\t\tNote\tcheck of CP2",
            "07\t\tEP\t1\t102\t\t\t20.0\t90.0\t5.0",
            "150\t\tx",
            "73\t\tAfter\t1",
        ]);
        assert!(warned(
            &result,
            3,
            "opcode 14 (check coordinate) is not one this reader imports"
        ));
        assert!(warned(
            &result,
            4,
            "an attribute of the point of record 3, which was not read"
        ));
        assert!(warned(
            &result,
            6,
            "opcode 150 is not one the format defines"
        ));
        assert!(warned(
            &result,
            7,
            "an attribute of the point of record 6, which was not read"
        ));
        assert!(!metadata_of(&result.project, "101").contains_key("Note"));
        assert!(!metadata_of(&result.project, "102").contains_key("After"));
        assert_eq!(result.records_skipped, 4);
    }

    #[test]
    fn a_target_height_that_is_no_number_is_skipped_and_the_one_before_still_applies() {
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "05\t\t1.600",
            "05\t\tabc",
            "05\t\t",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        ]);
        assert!(warned(&result, 3, "target height 'abc' is not a number"));
        assert!(warned(
            &result,
            3,
            "target height record without a height the reader can read"
        ));
        assert!(warned(
            &result,
            4,
            "target height record without a height the reader can read"
        ));
        double_eq(
            observations_to::<DistanceObservation>(&result.project.stations[0], "1")[0]
                .target_height,
            1.6,
        );
        assert_eq!(result.records_skipped, 2);
    }

    #[test]
    fn a_setup_that_cannot_be_read_leaves_its_shots_unread_rather_than_filed_under_the_one_before()
    {
        let result = read_lines(&[
            "03\t\t\t\tA\t\t\t1.5",
            "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
            "03\t\t\t\tB\t\t\t1.5\t9", // a value too many
            "07\t\tEB\t01\t2\t\t\t20.0\t90.0\t5.0",
            "128\t\tS\t1.5", // neither the height alone nor a description and it
            "07\t\tEB\t01\t3\t\t\t30.0\t90.0\t5.0",
        ]);
        assert_eq!(result.project.stations.len(), 1);
        let a = &result.project.stations[0];
        assert_eq!(a.observations.len(), 3); // point 1 only
        assert!(observations_to::<DistanceObservation>(a, "2").is_empty());
        assert!(observations_to::<DistanceObservation>(a, "3").is_empty());
        assert!(warned(
            &result,
            3,
            "the measurements after it are not read until the next setup"
        ));
        assert!(warned(
            &result,
            4,
            "measurement from the setup of record 3, which was not read"
        ));
        assert!(warned(&result, 5, "resection (opcode 128) with 2 value(s)"));
        assert!(warned(
            &result,
            6,
            "measurement from the setup of record 5, which was not read"
        ));
        assert_eq!(result.records_skipped, 4);
    }

    #[test]
    fn a_resection_after_a_setup_starts_a_setup_of_its_own() {
        // The resection job's 128 blocks after a 03: before the reader read
        // 128, their shots were filed under that 03's setup and oriented by it.
        let result = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
            "128\t\t\t\t\tRS1\t\t1.600\t",
            "7\t\t\t\tK\tK\t\t180.0\t90.0\t100.0",
            "129",
            "07\t\tEB\t01\t2\t\t\t20.0\t90.0\t5.0",
        ]);
        assert_eq!(result.project.stations.len(), 2);
        let k = &result.project.stations[0];
        let resected = &result.project.stations[1];
        assert_eq!(k.setup.point_id, "K");
        assert_eq!(k.observations.len(), 3); // point 1 only
        assert_eq!(resected.setup.point_id, "RS1");
        double_eq(resected.setup.instrument_height, 1.6);
        assert_eq!(
            observations_to::<DistanceObservation>(resected, "K").len(),
            1
        );
        assert_eq!(
            observations_to::<DistanceObservation>(resected, "2").len(),
            1
        );
        assert_eq!(result.records_skipped, 0);
    }

    #[test]
    fn a_resection_of_the_descriptions_own_form_names_no_point_and_still_owns_its_shots() {
        // [FLD]'s .fld syntax: "128 instrument_height_value", no description.
        let result = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "128\t\t1.500",
            "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
            "129",
        ]);
        assert_eq!(result.project.stations.len(), 1);
        let setup = &result.project.stations[0];
        assert_eq!(setup.setup.point_id, "resection at record 2");
        double_eq(setup.setup.instrument_height, 1.5);
        assert_eq!(observations_to::<DistanceObservation>(setup, "1").len(), 1);
        assert_eq!(value_of(&setup.metadata, "resection end"), "record 4");
        assert_eq!(result.records_skipped, 0);
        // A 129 with no resection open says so.
        let stray = read_lines(&["03\t\t\t\tS\t\t\t1.5", "129"]);
        assert!(warned(
            &stray,
            2,
            "end of a resection (opcode 129) with no resection open"
        ));
    }

    #[test]
    fn a_resection_end_that_does_not_match_its_start_is_warned_of() {
        let result = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "128\t\t1.500",
            "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
            "139", // a Helmert end for a least squares resection
            "138\t\t1.500",
            "07\t\tEB\t01\t2\t\t\t20.0\t90.0\t5.0",
            "139\t\tstray",
        ]);
        assert!(warned(
            &result,
            4,
            "end of a resection (opcode 139) ends the resection of record 2, which is opcode 128"
        ));
        assert!(warned(
            &result,
            7,
            "end of a resection (opcode 139) carries values the format does not give it; they are \
             not kept"
        ));
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(
            value_of(&result.project.stations[1].metadata, "resection"),
            "Helmert, record 5"
        );
        assert_eq!(result.records_skipped, 0);
    }

    #[test]
    fn a_coded_setup_strings_nothing_and_keeps_its_code_with_the_setup() {
        // [FLD] 44.4 strings a measurement point; a setup makes none.
        let result = read_lines(&[
            "02\t\t\t\tS1\t\t\t1000.0\t5000.0\t20.0",
            "03\t\tKB\t1\tS1\t\tnail\t1.500",
            "07\t\tKB\t1\t101\t\t\t10.0\t90.0\t5.0",
            "128\t\tKB\t1\t\tR1\t\t0.000\t",
            "07\t\tKB\t1\t102\t\t\t20.0\t90.0\t5.0",
        ]);
        let kb = topcon_test::feature(&result.project, "KB").expect("KB");
        assert_eq!(kb.point_ids, ["101", "102"]);
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(
            value_of(&result.project.stations[0].metadata, "setup coding"),
            "feature code KB, string number 1, comment nail"
        );
        assert_eq!(
            value_of(&result.project.stations[1].metadata, "setup coding"),
            "feature code KB, string number 1"
        );
        // The comment still describes the point the setup stands on.
        assert_eq!(
            topcon_test::point(&result.project, "S1")
                .unwrap()
                .description,
            "nail"
        );
        assert_eq!(topcon_test::point(&result.project, "S1").unwrap().code, "");
    }

    #[test]
    fn a_resection_given_a_point_id_with_its_name_keeps_the_id() {
        // As a 03 does: the setup is on the name, and the ID is kept with it.
        let result = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "128\t\t\t\t77\tRS1\t\t1.550\t",
            "129",
        ]);
        assert_eq!(result.project.stations.len(), 1);
        assert_eq!(result.project.stations[0].setup.point_id, "RS1");
        assert_eq!(
            value_of(&result.project.stations[0].metadata, "point ID"),
            "77"
        );
        assert_eq!(result.records_skipped, 0);
    }

    #[test]
    fn a_string_is_closed_by_its_code_or_its_point_and_a_closing_that_finds_none_is_skipped() {
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
            "07\t\tEP\t1\t2\t\t\t20.0\t90.0\t5.0",
            "07\t\tEP\t1\t3\t\t\t30.0\t90.0\t5.0",
            "20\t\tEP\t1\t\t\t", // closes EP 1 by its code and number
            "20",                // the current string, EP 1, is closed already
            "07\t\tFE\t\t4\t\t\t40.0\t90.0\t5.0",
            "07\t\tFE\t\t5\t\t\t50.0\t90.0\t5.0",
            "20\t\t\t\t4\t\t", // closes the string of point 4: FE, two points
            "07\t\t\t\t6\t\t\t60.0\t90.0\t5.0",
            "20", // point 6 has no feature code: no current string
            "07\t\tEP\t1\t7\t\t\t70.0\t90.0\t5.0",
            "20\t\t\t\t\t\tnote", // a description naming neither a string nor a point
            "20\t\tEP\t1",        // two values
        ]);
        let ep = features_of(&result.project, "EP");
        assert_eq!(ep.len(), 2);
        assert!(ep[0].closed);
        assert_eq!(ep[0].point_ids, ["1", "2", "3"]);
        assert!(!ep[1].closed);
        assert_eq!(ep[1].point_ids, ["7"]);
        let fe = features_of(&result.project, "FE");
        assert_eq!(fe.len(), 1);
        assert!(fe[0].closed);
        assert!(warned(&result, 6, "there is no open string EP 1"));
        assert!(warned(&result, 9, "is closed with 2 point(s)"));
        assert!(warned(
            &result,
            11,
            "the point of record 10 has no feature code"
        ));
        assert!(warned(
            &result,
            13,
            "whose point description names neither a string nor a point"
        ));
        assert!(warned(
            &result,
            14,
            "with 2 value(s) where the format gives none, or a point description of 5"
        ));
        assert_eq!(result.records_read, 10);
        assert_eq!(result.records_skipped, 4);
    }

    #[test]
    fn a_bare_close_after_multiple_coding_closes_the_second_string() {
        // 44.4: the current string is the current measurement point's, and a
        // 16 makes a new measurement point in its own string - so the bare 20
        // after it closes TB 7, not the shot's HB 2.
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tHB\t2\t301\t\t\t10.0\t90.0\t5.0",
            "07\t\tHB\t2\t302\t\t\t20.0\t90.0\t5.0",
            "07\t\tTB\t7\t401\t\t\t30.0\t90.0\t5.0",
            "07\t\tTB\t7\t402\t\t\t40.0\t90.0\t5.0",
            "07\t\tHB\t2\t303\t\t\t50.0\t90.0\t5.0",
            "16\t\tTB\t7\t\t\t",
            "20",
        ]);
        let tb = features_of(&result.project, "TB");
        assert_eq!(tb.len(), 1);
        assert!(tb[0].closed);
        assert_eq!(tb[0].point_ids, ["401", "402", "303"]);
        let hb = features_of(&result.project, "HB");
        assert_eq!(hb.len(), 1);
        assert!(!hb[0].closed);
        assert_eq!(hb[0].point_ids, ["301", "302", "303"]);
        assert_eq!(result.records_skipped, 0);
    }

    #[test]
    fn a_multiple_coding_not_read_leaves_the_current_string_unknown_so_a_bare_close_is_skipped() {
        // A 16 makes its own string the current one. One that is not read -
        // this one has a sixth value, and the next no feature code - leaves
        // which string is current unknown: the bare 20 after it is skipped,
        // naming it, where it closed EB 01, the string of the shot before. A
        // measurement read after it makes its own string current, and so does
        // a 16 read after it: the last 20 closes TB 4, 301's second string,
        // not HB 3.
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEB\t01\t101\t\t\t10.0\t90.0\t5.0",
            "07\t\tEB\t01\t102\t\t\t20.0\t90.0\t5.0",
            "16\t\tNS\t01\t\t\t\tx",
            "20",
            "07\t\tFE\t2\t201\t\t\t30.0\t90.0\t5.0",
            "16\t\t\t\t\t\t",
            "20",
            "07\t\tFE\t2\t202\t\t\t40.0\t90.0\t5.0",
            "20",
            "07\t\tHB\t3\t301\t\t\t50.0\t90.0\t5.0",
            "16\t\t\t\t\t\t",
            "16\t\tTB\t4\t\t\t",
            "20",
        ]);
        assert!(warned(
            &result,
            14,
            "the string TB 4 is closed with 1 point(s)"
        ));
        let tb = features_of(&result.project, "TB");
        assert_eq!(tb.len(), 1);
        assert!(tb[0].closed);
        assert_eq!(tb[0].point_ids, ["301"]);
        let hb = features_of(&result.project, "HB");
        assert_eq!(hb.len(), 1);
        assert!(!hb[0].closed);
        assert!(warned(
            &result,
            12,
            "multiple coding (opcode 16) with no feature code"
        ));
        assert!(warned(
            &result,
            4,
            "multiple coding (opcode 16) with 6 value(s) where the format gives a point \
             description of 5"
        ));
        assert!(warned(
            &result,
            5,
            "close string (opcode 20): the current string is the one the multiple coding of \
             record 4 made current, which was not read"
        ));
        assert!(warned(
            &result,
            7,
            "multiple coding (opcode 16) with no feature code"
        ));
        assert!(warned(
            &result,
            8,
            "the multiple coding of record 7 made current, which was not read"
        ));
        let eb = features_of(&result.project, "EB");
        assert_eq!(eb.len(), 1);
        assert!(!eb[0].closed);
        let fe = features_of(&result.project, "FE");
        assert_eq!(fe.len(), 1);
        assert!(fe[0].closed);
        assert_eq!(fe[0].point_ids, ["201", "202"]);
        assert!(warned(
            &result,
            10,
            "the string FE 2 is closed with 2 point(s)"
        ));
        // Records 4, 5, 7, 8 and 12.
        assert_eq!(result.records_skipped, 5);
    }

    #[test]
    fn a_multiple_coding_a_close_and_an_offset_with_a_trailing_tab_are_read() {
        // One blank value past a record's count is a trailing tab, as a fixed
        // record's is: each of these was skipped for its count. K (1000, 5000)
        // oriented on B, due north at circle 0. The 16 strings 102 into NS 01
        // as well; the 42 moves 102, the current measurement, 0.5 m further
        // out, to 10.5 m on bearing 100; the 20 closes NS 01; the 43 moves
        // 101, 10 m due east, 0.3 m to the left looking from K: north, to
        // (1010, 5000.3).
        let result = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
            "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
            "07\t\tEB\t01\t102\t\t\t100.0\t90.0\t10.0",
            "16\t\tNS\t01\t\t\t\t",
            "42\t\t0.5\t",
            "20\t\tNS\t01\t\t\t\t",
            "43\t\t\t\t101\t\t\t-0.3\t",
        ]);
        assert_eq!(result.records_skipped, 0);
        let ns = features_of(&result.project, "NS");
        assert_eq!(ns.len(), 1);
        assert!(ns[0].closed);
        assert_eq!(ns[0].point_ids, ["102"]);
        let positions = reduced_positions(&result.project);
        assert!(positions.contains_key("101") && positions.contains_key("102"));
        near(
            positions["102"].easting,
            1000.0 + 10.5 * degrees(100.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            positions["102"].northing,
            5000.0 + 10.5 * degrees(100.0).cos(),
            POSITION_TOLERANCE,
        );
        near(positions["101"].easting, 1010.0, POSITION_TOLERANCE);
        near(positions["101"].northing, 5000.3, POSITION_TOLERANCE);
    }

    #[test]
    fn a_close_or_an_offset_finds_a_named_shot_by_its_point_id() {
        // [FLD] 44.8: an offset "adjusts the point with that point ID" and a 20
        // closes "the string containing that point ID". The shots are named P1,
        // P2 and P3, with IDs 11, 12 and 13; the offset names P2 and the close
        // P1 by the ID alone. Before, the ID was looked for among the names,
        // and both were skipped, saying no record had made the point. Two more
        // shots are NAMED 11 and 12, in EQ 1: the ID is looked for before the
        // name, so neither is moved nor has its string closed.
        //
        // K (1000, 5000) oriented on B, due north at circle 0: P2 at circle 80,
        // level, 10 m, moved 0.5 m further out is 10.5 m out on bearing 80;
        // the shot named 12, at circle 50, stays 10 m out.
        let result = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
            "07\t\tEP\t1\t11\tP1\t\t90.0\t90.0\t10.0",
            "07\t\tEP\t1\t12\tP2\t\t80.0\t90.0\t10.0",
            "07\t\tEP\t1\t13\tP3\t\t70.0\t90.0\t10.0",
            "07\t\tEQ\t1\t98\t11\t\t60.0\t90.0\t10.0",
            "07\t\tEQ\t1\t99\t12\t\t50.0\t90.0\t10.0",
            "42\t\t\t\t12\t\t\t0.5",
            "20\t\t\t\t11\t\t",
        ]);
        assert_eq!(result.records_skipped, 0);
        assert!(result.warnings.is_empty(), "{}", result.warnings[0].message);
        let ep = features_of(&result.project, "EP");
        assert_eq!(ep.len(), 1);
        assert!(ep[0].closed);
        assert_eq!(ep[0].point_ids, ["P1", "P2", "P3"]);
        let eq = features_of(&result.project, "EQ");
        assert_eq!(eq.len(), 1);
        assert!(!eq[0].closed);
        assert_eq!(
            value_of(&metadata_of(&result.project, "P2"), "offset record 10"),
            "radial 0.5 m from setup K, applied to the shot of record 6"
        );
        assert_eq!(
            value_of(&metadata_of(&result.project, "12"), "offset record 10"),
            "(absent)"
        );
        let positions = reduced_positions(&result.project);
        assert!(positions.contains_key("P2") && positions.contains_key("12"));
        near(
            positions["P2"].easting,
            1000.0 + 10.5 * degrees(80.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            positions["P2"].northing,
            5000.0 + 10.5 * degrees(80.0).cos(),
            POSITION_TOLERANCE,
        );
        near(
            positions["12"].easting,
            1000.0 + 10.0 * degrees(50.0).sin(),
            POSITION_TOLERANCE,
        );
        near(
            positions["12"].northing,
            5000.0 + 10.0 * degrees(50.0).cos(),
            POSITION_TOLERANCE,
        );
    }

    #[test]
    fn an_offset_or_a_close_by_a_point_id_a_shot_and_a_coordinate_share_acts_on_the_shot() {
        // Coordinate CP5 has point ID 5, and so has shot 5, which has no name.
        // [FLD] calls a point ID "normally unique", the ID a measurement is
        // given; an offset moves a shot, and a 20 closes "the string
        // containing that point ID". Both take the shot, saying so. Before,
        // the coordinate was taken, which no offset moves and no string holds:
        // the offset was kept unapplied and the 20 skipped. Coordinate CP9, in
        // string KB 1, and shot 9, in none, have point ID 9: only the
        // coordinate's string is open, so the 20 naming 9 closes it.
        //
        // A (1000, 5000) oriented on B, due north at circle 0: shot 5 at circle
        // 0, level, 50 m, moved 0.5 m further out is at (1000, 5050.5).
        let result = read_lines(&[
            "02\t\t\t\t5\tCP5\t\t1000.0\t5050.0\t20.0",
            "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tA\t\t\t1.5",
            "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
            "07\t\tEB\t01\t5\t\t\t0.0\t90.0\t50.0",
            "07\t\tEB\t01\t6\t\t\t90.0\t90.0\t10.0",
            "42\t\t\t\t5\t\t\t0.5",
            "20\t\t\t\t5\t\t",
            "02\t\tKB\t1\t8\tCP8\t\t1020.0\t5000.0\t20.0",
            "02\t\tKB\t1\t9\tCP9\t\t1030.0\t5000.0\t20.0",
            "07\t\t\t\t9\t\t\t180.0\t90.0\t10.0",
            "20\t\t\t\t9\t\t",
        ]);
        assert_eq!(result.records_skipped, 0);
        assert!(warned(
            &result,
            8,
            "radial offset (opcode 42) names point ID 5, which a shot gave point '5' and a \
             coordinate point 'CP5': it is taken for point '5', the shot's"
        ));
        assert!(warned(
            &result,
            9,
            "close string (opcode 20) names point ID 5, which a shot gave point '5' and a \
             coordinate point 'CP5': it is taken for point '5', the shot's"
        ));
        assert!(warned(
            &result,
            9,
            "the string of point '5' is closed with 2 point(s)"
        ));
        assert!(warned(
            &result,
            13,
            "close string (opcode 20) names point ID 9, which a shot gave point '9' and a \
             coordinate point 'CP9': it is taken for point 'CP9', the one on an open string"
        ));
        let kb = features_of(&result.project, "KB");
        assert_eq!(kb.len(), 1);
        assert!(kb[0].closed);
        assert_eq!(kb[0].point_ids, ["CP8", "CP9"]);
        let eb = features_of(&result.project, "EB");
        assert_eq!(eb.len(), 1);
        assert!(eb[0].closed);
        assert_eq!(
            value_of(&metadata_of(&result.project, "5"), "offset record 8"),
            "radial 0.5 m from setup A, applied to the shot of record 6"
        );
        assert_eq!(
            value_of(&metadata_of(&result.project, "CP5"), "offset record 8"),
            "(absent)"
        );
        let positions = reduced_positions(&result.project);
        assert!(positions.contains_key("5"));
        near(positions["5"].easting, 1000.0, POSITION_TOLERANCE);
        near(positions["5"].northing, 5050.5, POSITION_TOLERANCE);
    }

    #[test]
    fn the_file_end_record_stops_the_read_and_counts_what_follows() {
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
            "99",
            "07\t\tEP\t1\t2\t\t\t20.0\t90.0\t5.0",
            "// a comment after the end",
            "73\t\tDate\td",
        ]);
        assert_eq!(result.records_read, 3);
        assert_eq!(result.records_skipped, 2);
        assert!(warned(
            &result,
            3,
            "2 record(s) after the end of the file (opcode 99)"
        ));
        assert!(topcon_test::unpositioned(&result.project, "2").is_none());
        assert!(!metadata_of(&result.project, "1").contains_key("Date"));
    }

    #[test]
    fn records_after_the_file_end_do_not_decide_the_layout() {
        // Four records with the column after the opcode, the end of the file
        // (99), then five written without it. What follows the 99 is not read,
        // so it does not vote on the layout either: its five votes against
        // four made the file one without the column, and every record before
        // the 99 was refused - the file "holds no station, measurement or
        // coordinate".
        let result = read_lines(&[
            "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
            "03\t\t\t\tK\t\t\t1.5",
            "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
            "99",
            "07\tEB\t01\t201\t\t\t90.0\t90.0\t10.0",
            "07\tEB\t01\t202\t\t\t91.0\t90.0\t10.0",
            "07\tEB\t01\t203\t\t\t92.0\t90.0\t10.0",
            "07\tEB\t01\t204\t\t\t93.0\t90.0\t10.0",
            "07\tEB\t01\t205\t\t\t94.0\t90.0\t10.0",
        ]);
        assert_eq!(result.records_read, 5);
        assert_eq!(result.records_skipped, 5);
        assert!(warned(
            &result,
            5,
            "5 record(s) after the end of the file (opcode 99) are not read"
        ));
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.project.points.len(), 2);
        assert_eq!(result.project.stations.len(), 1);
        assert_eq!(
            observations_to::<DistanceObservation>(&result.project.stations[0], "101").len(),
            1
        );
    }

    #[test]
    fn an_opcode_only_the_xml_form_has_and_one_the_format_does_not_define_are_skipped_saying_so() {
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
            "140\t\tEP\t1\t9\t\t\t1.0\t2.0\t3.0",
            "73\t\tDate\td",
            "8\t\tx",
            "15\t\t0.001",
            "50\t\t\t\tS\t\t\t0.5",
            "51\t\tkerbs\tfor",
            "48",
            "47",
        ]);
        assert!(warned(
            &result,
            3,
            "opcode 140 (GNSS coordinate) is defined only for the format's XML form"
        ));
        // 140 makes a point in [FLD]: what follows it is not given to point 1.
        assert!(warned(
            &result,
            4,
            "of the point of record 3, which was not read"
        ));
        assert!(!metadata_of(&result.project, "1").contains_key("Date"));
        assert!(warned(
            &result,
            5,
            "opcode 08 is not one the format defines"
        ));
        assert!(warned(
            &result,
            6,
            "opcode 15 (vertical circle correction) is not one this reader imports; the \
             measurements after it are read without it"
        ));
        // 50 is a bearing datum difference for what follows; 51 starts a field
        // template, from which what follows may take its codes.
        assert!(warned(
            &result,
            7,
            "opcode 50 (backsight bearing) is not one this reader imports; the measurements after \
             it are read without it"
        ));
        assert!(warned(
            &result,
            8,
            "opcode 51 (template start) is not one this reader imports; the measurements after it \
             keep the codes they are written with"
        ));
        // 48 ends the current string: the points after it would start another.
        assert!(warned(
            &result,
            9,
            "opcode 48 (end string) is not one this reader imports; the points after it are \
             strung as if the string had not ended"
        ));
        // 47 ends it before the current point, which "becomes the first point
        // of a new string": that point stays where it is, too.
        assert!(warned(
            &result,
            10,
            "opcode 47 (new string) is not one this reader imports; the current point is not \
             moved into a new string of its own, and the points after it are strung as if the \
             string had not ended"
        ));
        assert_eq!(result.records_skipped, 8);
    }

    #[test]
    fn lines_that_are_no_record_say_why_and_indented_records_and_comments_are_read() {
        let result = read_lines(&[
            "  {Version 6.0}",
            "   // Coordinate System: Local grid D",
            "\t// Job name : indented by a tab",
            "03\t\t\t\tS\t\t\t1.5",
            "\t07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0", // a tab before the opcode, as the probe reads it
            "0007\t\tEP\t1\t2\t\t\t20.0\t90.0\t5.0",
            "+7\t\tEP\t1\t3\t\t\t30.0\t90.0\t5.0",
            "{Version 7.0}", // not the first record
            "\u{1A}",        // a DOS end-of-file byte
        ]);
        assert_eq!(result.project.metadata["version"], "{Version 6.0}");
        assert_eq!(result.project.coordinate_system.name, "Local grid D");
        assert_eq!(
            result.project.metadata["header: Job name"],
            "indented by a tab"
        );
        assert_eq!(
            observations_to::<DistanceObservation>(&result.project.stations[0], "1").len(),
            1
        );
        assert!(warned(
            &result,
            6,
            "'0007' is not an opcode: an opcode is a number of one to three digits"
        ));
        assert!(warned(
            &result,
            7,
            "'+7' is not an opcode: an opcode is a number"
        ));
        assert!(warned(
            &result,
            8,
            "'{Version 7.0}' is not an opcode: a record opens with a number"
        ));
        assert_eq!(result.records_read, 3);
        assert_eq!(result.records_skipped, 3);

        // A file indented throughout, which the probe identifies, is read.
        let indented = read_lines(&[
            "\t{Version 6.0}",
            "\t02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
            "\t03\t\t\t\tK\t\t\t1.5",
            "\t07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        ]);
        assert_eq!(indented.records_skipped, 0);
        assert_eq!(indented.records_read, 4);
    }

    #[test]
    fn an_unnamed_attribute_in_a_file_without_the_column_is_kept_under_its_record() {
        // "If there is no name for the attribute (name is just spaces or a
        // tab), then the attribute is unnamed": without the column,
        // "72<tab><tab>0.5" is a blank name and 0.5.
        let result = read_lines(&[
            "100\tdegrees\tmetres",
            "03\t\t\tA\t\t\t1.5",
            "07\tTREE\t\t7\t\t\t90.0\t90.0\t12.5",
            "72\t\t0.5",
            "72\tDepth\t1.5",
        ]);
        let p7 = metadata_of(&result.project, "7");
        assert_eq!(value_of(&p7, "unnamed attribute (record 4)"), "0.5");
        assert_eq!(value_of(&p7, "Depth"), "1.5");
        assert!(!p7.contains_key("0.5"));
    }

    #[test]
    fn an_attribute_with_extra_values_or_with_nothing_is_warned_of() {
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
            "72\t\tDepth\t0.6\t0.7",
            "71\t\tCount\t3\t4",
            "73\t\t",
            "73",
        ]);
        let p1 = metadata_of(&result.project, "1");
        assert_eq!(value_of(&p1, "Depth"), "0.6");
        assert_eq!(value_of(&p1, "Count"), "3");
        assert!(warned(
            &result,
            3,
            "attribute 'Depth' has values after its value; they are not kept"
        ));
        assert!(warned(
            &result,
            4,
            "attribute 'Count' has values after its value"
        ));
        assert!(warned(
            &result,
            5,
            "attribute record (opcode 73) with neither a name nor a value"
        ));
        assert!(warned(
            &result,
            6,
            "attribute record (opcode 73) with neither a name nor a value"
        ));
        assert_eq!(result.records_skipped, 2);
    }

    #[test]
    fn additional_text_keeps_its_spaces() {
        // "any spaces from column four onwards will be part of the text"
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
            "41\t\t  indented text  ",
        ]);
        assert_eq!(
            value_of(&metadata_of(&result.project, "1"), "additional text 3"),
            "  indented text  "
        );
    }

    #[test]
    fn a_scale_factor_is_in_the_distances_and_the_setup_says_it_is_applied() {
        let result = read_lines(&[
            "09\t\t1.0002",
            "03\t\t\t\tS\t\t\t1.5",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t100.000",
        ]);
        assert_eq!(result.project.stations.len(), 1);
        let station = &result.project.stations[0];
        double_eq(station.instrument.scale_factor.expect("a factor"), 1.0002);
        // The reader multiplied the distance (100 m x 1.0002 = 100.02 m), so a
        // reduction must not apply the factor again.
        assert_eq!(
            station.instrument.scale_factor_state,
            CorrectionState::Applied
        );
        let distances = observations_to::<DistanceObservation>(station, "1");
        assert_eq!(distances.len(), 1);
        near(distances[0].distance, 100.02, 1e-9);
    }

    #[test]
    fn a_setups_settings_state_the_scale_factor_every_one_of_its_distances_was_multiplied_by() {
        // S: a 09 after the 03, before any distance: the setup's every distance
        // (100 m x 0.5) is scaled by it. T: the factor changes after its first
        // shot, so its settings state none, and say why. U: a factor set
        // between setups is U's, and says nothing of T.
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "09\t\t0.5",
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t100.000",
            "03\t\t\t\tT\t\t\t1.5",
            "07\t\tEP\t1\t2\t\t\t20.0\t90.0\t100.000",
            "09\t\t1.0",
            "07\t\tEP\t1\t3\t\t\t30.0\t90.0\t100.000",
            "09\t\t2.0",
            "03\t\t\t\tU\t\t\t1.5",
            "07\t\tEP\t1\t4\t\t\t40.0\t90.0\t100.000",
        ]);
        assert_eq!(result.project.stations.len(), 3);
        let s = &result.project.stations[0];
        double_eq(s.instrument.scale_factor.expect("a factor"), 0.5);
        assert_eq!(s.instrument.scale_factor_state, CorrectionState::Applied);
        double_eq(
            observations_to::<DistanceObservation>(s, "1")[0].distance,
            50.0,
        );
        let t = &result.project.stations[1];
        assert!(t.instrument.scale_factor.is_none());
        assert_eq!(t.instrument.scale_factor_state, CorrectionState::Unknown);
        double_eq(
            observations_to::<DistanceObservation>(t, "2")[0].distance,
            50.0,
        );
        double_eq(
            observations_to::<DistanceObservation>(t, "3")[0].distance,
            100.0,
        );
        assert!(warned(
            &result,
            7,
            "the scale factor changed within setup T"
        ));
        let u = &result.project.stations[2];
        double_eq(u.instrument.scale_factor.expect("a factor"), 2.0);
        assert_eq!(u.instrument.scale_factor_state, CorrectionState::Applied);
        assert!(!warned(&result, 8, "changed within"));
        assert!(!warned(&result, 9, "changed within"));
        assert_eq!(result.records_skipped, 0);
    }

    #[test]
    fn attribute_groups_nest_and_a_group_ended_twice_or_left_open_is_warned_of() {
        let result = read_lines(&[
            "02\t\t\t\tP\t\t\t1000.0\t5000.0\t10.0",
            "124\t\t\tOuter\t0",
            "124\t\t\tInner\t1",
            "73\t\tA\t1",
            "125\t\t\t\t1",
            "73\t\tB\t2",
            "125\t\t\t\t0",
            "125\t\t\t\t0", // no group open
            "124\t\t\tLeft open\t3",
            "73\t\tC\t3",
        ]);
        let p = metadata_of(&result.project, "P");
        assert_eq!(value_of(&p, "Outer/Inner/A"), "1");
        assert_eq!(value_of(&p, "Outer/B"), "2");
        assert_eq!(value_of(&p, "Left open/C"), "3");
        assert!(warned(
            &result,
            8,
            "end of an attribute group (opcode 125) with no group open"
        ));
        assert!(warned(
            &result,
            9,
            "gives the level '3' and opens inside 0 group(s)"
        ));
        assert!(warned(
            &result,
            9,
            "is not ended (opcode 125) before the file ends"
        ));
        assert_eq!(result.records_skipped, 1);
    }

    #[test]
    fn a_group_record_that_does_not_fit_is_warned_of_or_skipped() {
        let result = read_lines(&[
            "02\t\t\t\tP\t\t\t1000.0\t5000.0\t10.0",
            "124\t\ta\tb\tc\td",  // four values: which is the name?
            "124\t\tx\tNamed\t0", // text before the name
            "73\t\tA\t1",
            "125\t\t\t\t5", // not the open group's level
            "124\t\t\t\t0", // no name
            "73\t\tB\t2",
            "125\t\t\tOther\t0", // names another group
        ]);
        assert!(warned(
            &result,
            2,
            "attribute group (opcode 124) with 4 value(s); which is its name cannot be told"
        ));
        assert!(warned(
            &result,
            3,
            "attribute group 'Named' has 'x' before its name"
        ));
        assert!(warned(
            &result,
            5,
            "the end of attribute group 'Named' gives the level '5', which is not the group's"
        ));
        assert!(warned(
            &result,
            6,
            "attribute group with no name: its attributes are kept without one"
        ));
        assert!(warned(
            &result,
            8,
            "the end of attribute group '' names 'Other'; the open group is ended"
        ));
        let p = metadata_of(&result.project, "P");
        assert_eq!(value_of(&p, "Named/A"), "1");
        assert_eq!(value_of(&p, "B"), "2");
        assert_eq!(result.records_skipped, 1);
    }

    #[test]
    fn a_group_written_without_its_level_keeps_its_name() {
        // The files' layout less the level: an empty value and the name, where
        // two values had been read as a name (the empty one) and a level.
        let result = read_lines(&[
            " 2\t\t\t\tP\t\t\t1000.0\t5000.0\t10.0",
            "124\t\t\tGPS Information",
            "73\t\tSolution\tfixed",
            "125\t\t\t\t0",
            "124\t\t\tReference",
            "73\t\tSolution\tnetwork",
            "125\t\t\t\t0",
        ]);
        let p = metadata_of(&result.project, "P");
        assert_eq!(value_of(&p, "GPS Information/Solution"), "fixed");
        assert_eq!(value_of(&p, "Reference/Solution"), "network");
        assert!(result.warnings.is_empty(), "{}", result.warnings[0].message);
    }

    #[test]
    fn groups_nested_past_the_bound_are_read_but_not_named_so_names_grow_no_faster_than_the_file() {
        // One point and 2000 groups, each opened inside the last at the next
        // level, an attribute in each: named, the last attribute's name held
        // 2000 group names, and the names - and memory - grew with the square
        // of the file. The first 16 groups are named, the rest read and warned
        // of; every attribute is kept.
        let mut bytes = String::from(" 2\t\t\t\tP\t\t\t1000.0\t5000.0\t10.0\n");
        for g in 1..=2000 {
            bytes.push_str(&format!("124\t\t\tG{g}\t{}\n73\t\tA{g}\tv\n", g - 1));
        }
        let read = topcon_test::read(ID, bytes.as_bytes(), "deep.fld")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let p = metadata_of(&read.project, "P");
        let mut sixteen = String::new();
        for g in 1..=16 {
            sixteen.push_str(&format!("G{g}/"));
        }
        assert_eq!(value_of(&p, &format!("{sixteen}A16")), "v");
        assert_eq!(value_of(&p, &format!("{sixteen}A17")), "v");
        assert_eq!(value_of(&p, &format!("{sixteen}A2000")), "v");
        let mut longest = 0usize;
        let mut kept = 0usize;
        for key in p.keys() {
            longest = longest.max(key.len());
            kept += usize::from(key.contains("/A"));
        }
        assert_eq!(kept, 2000);
        assert_eq!(longest, format!("{sixteen}A2000").len());
        // Group 17 opens at record 34: the point, then two records a group.
        assert!(warned(
            &read,
            34,
            "attribute group 'G17' opens inside 16 named group(s)"
        ));

        // Long names meet the length bound first: five of 100 characters and
        // their separators fit in 512, the sixth does not.
        let mut wide = String::from(" 2\t\t\t\tQ\t\t\t1000.0\t5000.0\t10.0\n");
        for g in 1..=6 {
            wide.push_str(&format!("124\t\t\t{}{g}\t{}\n", "W".repeat(99), g - 1));
        }
        wide.push_str("73\t\tLast\tv\n");
        let broad = topcon_test::read(ID, wide.as_bytes(), "wide.fld")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut five = String::new();
        for g in 1..=5 {
            five.push_str(&format!("{}{g}/", "W".repeat(99)));
        }
        assert_eq!(
            value_of(&metadata_of(&broad.project, "Q"), &format!("{five}Last")),
            "v"
        );
        assert!(warned(&broad, 7, "past the 16 levels or 512 characters"));
    }

    #[test]
    fn multiple_coding_that_names_its_own_point_or_has_no_code_is_warned_of() {
        let result = read_lines(&[
            "03\t\t\t\tS\t\t\t1.5",
            "16\t\tNS\t01\t\t\t", // no measurement yet
            "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
            "16\t\tNS\t01\tX9\t\tpole",
            "16\t\t\t\t\t\t", // no feature code
            "16\t\tNS",       // one value
        ]);
        assert!(warned(
            &result,
            2,
            "with no measurement (opcode 02 or 07) before it"
        ));
        let ns = topcon_test::feature(&result.project, "NS").expect("NS");
        assert_eq!(ns.point_ids, ["1"]);
        assert_eq!(
            value_of(
                &metadata_of(&result.project, "1"),
                "multiple coding record 4"
            ),
            "NS 01, point ID X9, comment pole"
        );
        assert!(warned(&result, 4, "names point 'X9'"));
        assert!(topcon_test::unpositioned(&result.project, "X9").is_none());
        assert!(warned(&result, 5, "with no feature code"));
        assert!(warned(
            &result,
            6,
            "with 1 value(s) where the format gives a point description"
        ));
        assert_eq!(result.records_skipped, 3);
    }

    #[test]
    fn reads_every_truncation_and_noise_of_its_fixtures_without_crashing() {
        let mut reads = 0;
        for name in [
            "setup.fld",
            "gnss.fld",
            "resection.fld",
            "rtk_setup.fld",
            "two_backsights.fld",
        ] {
            reads += topcon_test::read_every_truncation_and_noise(
                ID,
                &fixture(Path::new("fld").join(name)),
                name,
            );
        }
        assert!(reads > 4000);
    }
}
