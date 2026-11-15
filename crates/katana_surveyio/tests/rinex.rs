//! Twin of tests/surveyio/test_rinex.cpp: the RINEX observation reader
//! (src/rinex.rs).
//!
//! The fixtures in data/rinex/ are synthetic and small, built by hand from the
//! specifications' tables (RINEX 2.11 tables A1/A2, 3.05 tables A2/A3, 4.00
//! section 5). Every expected value below is worked out from how the fixture
//! was designed - counted epochs, satellites, lines - with the working in a
//! comment; none is copied from the reader's output.

mod instrument_support;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use katana_core::{Error, ErrorCode, Result};
use katana_survey::data_model::to_string as survey_text;
use katana_survey::{
    AntennaHeightMethod, GnssGlobalPositionObservation, GnssSolution, Observation, SurveyProject,
};
use katana_surveyio::{
    DetectionOutcome, FormatContent, Manufacturer, PROBE_BYTES, RINEX_OBSERVATION_FORMAT_ID,
    ReadOptions, ReadResult, SiblingFile, SiblingLookup, describe, detect_format, format_registry,
    probe_of, read_survey, siblings_in_memory,
};

use instrument_support::{Random32, atoi, data_folder, double_eq, files_under, slurp};

// The fixtures sit beside the C++ test, under tests/surveyio/data/rinex.
fn fixture_folder() -> PathBuf {
    data_folder().join("rinex")
}

fn fixture(name: &str) -> Vec<u8> {
    let bytes = std::fs::read(fixture_folder().join(name));
    assert!(bytes.is_ok(), "missing fixture {name}");
    bytes.unwrap_or_default()
}

fn line_count(text: &[u8]) -> usize {
    text.iter().filter(|&&c| c == b'\n').count()
}

fn read_rinex_with(bytes: &[u8], name: &str, siblings: Vec<SiblingFile>) -> Result<ReadResult> {
    let options = ReadOptions {
        siblings: Some(siblings_in_memory(siblings)),
        ..Default::default()
    };
    read_survey(
        format_registry(),
        RINEX_OBSERVATION_FORMAT_ID,
        bytes,
        name,
        &options,
    )
}

fn read_rinex(bytes: &[u8], name: &str) -> Result<ReadResult> {
    read_rinex_with(bytes, name, Vec::new())
}

fn read_fixture(name: &str) -> Result<ReadResult> {
    read_rinex(&fixture(name), name)
}

fn read_fixture_with(name: &str, siblings: Vec<SiblingFile>) -> Result<ReadResult> {
    read_rinex_with(&fixture(name), name, siblings)
}

fn sibling(name: &str, bytes: Vec<u8>) -> SiblingFile {
    SiblingFile {
        name: String::from(name),
        bytes,
    }
}

fn ok(read: Result<ReadResult>) -> ReadResult {
    read.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn any_warning_contains(result: &ReadResult, text: &str) -> bool {
    result
        .warnings
        .iter()
        .any(|warning| warning.message.contains(text))
}

fn any_not_carried_contains(result: &ReadResult, text: &str) -> bool {
    result.not_carried.iter().any(|line| line.contains(text))
}

fn describe_warnings(result: &ReadResult) -> String {
    let mut text = String::new();
    for warning in &result.warnings {
        text.push_str(&describe(warning));
        text.push('\n');
    }
    text
}

fn position_at(project: &SurveyProject, index: usize) -> &GnssGlobalPositionObservation {
    match &project.observations[index] {
        Observation::GnssGlobalPosition(position) => position,
        other => panic!("observation {index} is not a GNSS position: {other:?}"),
    }
}

fn probe_confidence(bytes: &[u8], name: &str) -> f64 {
    let input = probe_of(
        &bytes[..bytes.len().min(PROBE_BYTES)],
        name,
        bytes.len() > PROBE_BYTES,
    );
    for probed in format_registry().probe_all(&input) {
        if probed.format_id == RINEX_OBSERVATION_FORMAT_ID {
            return probed.signature.confidence;
        }
    }
    -1.0
}

// A header record: content in columns 1-60, label in 61-80.
fn header(content: &str, label: &str) -> String {
    let mut line = String::from(content);
    line.truncate(60);
    format!("{line:<60}{label}\n")
}

const DEFAULT_APPROX: &str = " -4646000.0000  2553000.0000 -3534000.0000";

// A minimal RINEX 3.04 GPS header with four observation types, for the
// hand-built damaged files below. 11 lines.
fn minimal_v3_header(approx: &str) -> String {
    [
        header(
            "     3.04           OBSERVATION DATA    G",
            "RINEX VERSION / TYPE",
        ),
        header("katana test", "PGM / RUN BY / DATE"),
        header("M1", "MARKER NAME"),
        header("", "OBSERVER / AGENCY"),
        header("", "REC # / TYPE / VERS"),
        header("", "ANT # / TYPE"),
        header(approx, "APPROX POSITION XYZ"),
        header(
            "        1.0000        0.0000        0.0000",
            "ANTENNA: DELTA H/E/N",
        ),
        header("G    4 C1C L1C D1C S1C", "SYS / # / OBS TYPES"),
        header(
            "  2024     9    12    10     0    0.0000000     GPS",
            "TIME OF FIRST OBS",
        ),
        header("", "END OF HEADER"),
    ]
    .concat()
}

fn epoch_v3(minute: i32, second: f64, flag: i32, count: i32) -> String {
    format!("> 2024 09 12 10 {minute:02}{second:11.7}  {flag}{count:3}\n")
}

fn satellite_v3(satellite: &str) -> String {
    format!("{satellite}  21000000.000   110000001.50008     -2000.250          44.500\n")
}

fn gnss_counts(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
    pairs
        .iter()
        .map(|&(system, count)| (String::from(system), count))
        .collect()
}

// ---- Registration and detection ----------------------------------------------------

mod rinex_format {
    use super::*;

    #[test]
    fn is_registered_with_a_reader_and_says_it_reads_gnss_sessions_only() {
        let descriptor = format_registry()
            .find(RINEX_OBSERVATION_FORMAT_ID)
            .expect("the RINEX format is registered");
        assert_eq!(descriptor.human_name, "RINEX observation");
        assert_eq!(descriptor.manufacturer, Manufacturer::OpenStandard);
        assert_eq!(descriptor.parser_version, "1.0");
        assert!(descriptor.can_import);
        assert!(!descriptor.can_export);
        // It reads GNSS sessions and an approximate position - no surveyed
        // points, no total-station observations or setups, no coded strings.
        assert_eq!(
            descriptor.reads,
            FormatContent {
                gnss: true,
                ..Default::default()
            }
        );
        assert!(
            format_registry()
                .reader(RINEX_OBSERVATION_FORMAT_ID)
                .is_some()
        );
        assert!(
            descriptor
                .extensions
                .iter()
                .any(|extension| extension == "rnx")
        );
    }

    #[test]
    fn every_observation_fixture_is_identified_as_rinex_observation() {
        for name in [
            "test2560.24o",
            "TEST00AUS_R_20242561000_01H_30S_MO.rnx",
            "ROVR00AUS_R_20242561100_01H_01S_GO.rnx",
            "BASE00AUS_R_20242561200_01H_30S_MO.rnx",
            "rinex305_example1.rnx",
        ] {
            let bytes = fixture(name);
            let detection = detect_format(&probe_of(&bytes, name, false));
            assert_eq!(
                detection.outcome(),
                DetectionOutcome::Identified,
                "{name}: {}",
                detection.summary()
            );
            assert_eq!(
                detection.format().expect("identified").id,
                RINEX_OBSERVATION_FORMAT_ID,
                "{name}"
            );
            assert!(
                detection.candidates()[0]
                    .evidence
                    .contains("RINEX VERSION / TYPE"),
                "the evidence says what was seen: {}",
                detection.candidates()[0].evidence
            );
        }
    }

    #[test]
    fn the_probe_rules_out_every_survey_fixture_that_is_not_rinex() {
        // Every other format's fixtures in tests/surveyio/data - whatever parsers
        // have added theirs - plus hand-written openings of the other instrument
        // formats. None of them may look like RINEX.
        let mut looked = 0usize;
        for path in files_under(&data_folder()) {
            if path
                .parent()
                .and_then(|parent| parent.file_name())
                .is_some_and(|folder| folder == "rinex")
            {
                continue;
            }
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            assert_eq!(
                probe_confidence(&slurp(path.as_path()), &name),
                0.0,
                "{name}"
            );
            looked += 1;
        }
        assert!(looked >= 4, "the delimited-points fixtures at least");

        let others: [(&str, &[u8]); 10] = [
            (
                "job.gsi",
                b"*110001+0000000000000A01 21.324+0000000000000000 22.324+0000000009000000\n",
            ),
            (
                "job.gsi",
                b"110001+000000A1 21.324+00000000 22.324+09000000 31..00+00000000\n",
            ),
            (
                "job.jxl",
                b"<?xml version=\"1.0\"?>\n<JOBFile jobName=\"x\" version=\"5.6\">\n",
            ),
            (
                "job.rw5",
                b"JB,NMJOB,DT09-12-2024,TM10:00:00\nMO,AD0,UN2,SF1.00000000,EC1,EO0.0\n",
            ),
            (
                "job.gts",
                b"JOB     test\nSTN     1,1.500,\nBKB     2,0.0000,0.0000\n",
            ),
            ("job.raw", b"10NMJOB    121111\n13OOUNITS  \n"),
            (
                "points.csv",
                b"P1,1000.000,2000.000,50.000,IP\nP2,1010.000,2000.000,50.100,IP\n",
            ),
            (
                "job.xml",
                b"<?xml version=\"1.0\"?>\n<LandXML xmlns=\"http://www.landxml.org/schema/LandXML-1.2\">\n",
            ),
            ("empty.rnx", b""),
            ("photo.gz", b"\x1f\x8b\x08\x00\x00\x00\x00\x00"),
        ];
        for (name, bytes) in others {
            assert_eq!(probe_confidence(bytes, name), 0.0, "{name}");
        }
    }

    #[test]
    fn packed_files_are_recognised_and_refused_with_the_step_that_unpacks_them() {
        // (Hatanaka-compressed files are expanded, not refused: rinex_compact.rs.)
        // gzip and Unix compress: only the name can say RINEX is inside.
        let gzip: &[u8] = b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\x03";
        double_eq(
            probe_confidence(gzip, "TEST00AUS_R_20242561000_01H_30S_MO.rnx.gz"),
            0.8,
        );
        let gz = read_rinex(gzip, "TEST00AUS_R_20242561000_01H_30S_MO.rnx.gz")
            .expect_err("a gzip file is refused");
        assert_eq!(gz.code, ErrorCode::Unsupported);
        assert!(gz.message.contains("gzip"));
        assert!(gz.message.contains("Decompress"));

        let compress: &[u8] = b"\x1f\x9d\x90\x00";
        double_eq(probe_confidence(compress, "test2560.24d.Z"), 0.8);
        let z = read_rinex(compress, "test2560.24d.Z").expect_err("a .Z file is refused");
        assert!(z.message.contains("Unix-compressed"));
    }

    #[test]
    fn a_navigation_file_is_recognised_as_rinex_but_refused_as_an_occupation() {
        let navigation = fixture("test2560.24n");
        double_eq(probe_confidence(&navigation, "test2560.24n"), 0.75);
        let error = read_rinex(&navigation, "test2560.24n").expect_err("a navigation file");
        assert_eq!(error.code, ErrorCode::FileImportFailure);
        assert!(error.message.contains("navigation file"));
        assert!(error.message.contains("Import the observation file"));
    }
}

// ---- RINEX 2.11 ----------------------------------------------------------------------

mod rinex2 {
    use super::*;

    #[test]
    fn the_session_carries_the_headers_marker_receiver_and_antenna() {
        let read = ok(read_fixture("test2560.24o"));
        let project = &read.project;
        assert_eq!(project.gnss_sessions.len(), 1);
        let session = &project.gnss_sessions[0];
        assert_eq!(session.marker_name, "TEST");
        assert_eq!(session.marker_number, "12345M001");
        assert_eq!(session.observer, "A SURVEYOR");
        assert_eq!(session.agency, "KATANA TESTS");
        assert_eq!(session.receiver_serial, "5001234");
        assert_eq!(session.receiver_type, "LEICA GR50");
        assert_eq!(session.receiver_firmware, "4.50/7.000");
        // The IGS name keeps the blanks between model and radome.
        assert_eq!(session.antenna.r#type, "LEIAR20         LEIM");
        assert_eq!(session.antenna.serial_number, "7654321");
        // ANTENNA: DELTA H/E/N 1.5230 0.0010 -0.0020: a vertical height to the ARP.
        double_eq(session.antenna.height, 1.523);
        double_eq(session.antenna.east_offset, 0.001);
        double_eq(session.antenna.north_offset, -0.002);
        assert_eq!(session.antenna.method, AntennaHeightMethod::Vertical);
        assert_eq!(session.format_version, "2.11");
        assert_eq!(session.source.file_name, "test2560.24o");
        assert_eq!(session.source.record_number, 4, "MARKER NAME is line 4");
        assert_eq!(project.name, "TEST");
    }

    #[test]
    fn epochs_are_counted_with_their_times_interval_and_satellites_per_system() {
        let read = ok(read_fixture("test2560.24o"));
        let session = &read.project.gnss_sessions[0];
        // Observation epochs (flags 0 and 1) at 10:00:00, 10:00:30, 10:01:00,
        // 10:01:30 and 10:02:00; the flag 4 and flag 5 events are not epochs.
        assert_eq!(session.epoch_count, 5);
        assert_eq!(survey_text(&session.first_epoch), "2024-09-12T10:00:00 GPS");
        assert_eq!(survey_text(&session.last_epoch), "2024-09-12T10:02:00 GPS");
        double_eq(session.interval_seconds.expect("an interval"), 30.0);
        // GPS: G01-G09 in the first epoch, G10 in the fourth -> 10.
        // GLONASS: R01-R05 in the first, R06 in the fourth -> 6.
        // Galileo: E11 in the last -> 1.
        assert_eq!(
            session.satellites_per_system,
            gnss_counts(&[("GPS", 10), ("GLONASS", 6), ("Galileo", 1)])
        );
        // 14 + 5 + 4 + 2 + 4 satellite records in the five epochs.
        assert_eq!(read.project.metadata["satellite records"], "29");
        assert_eq!(
            read.project.metadata["observation types"],
            "C1 L1 L2 P2 D1 S1 S2"
        );
    }

    #[test]
    fn every_line_is_a_record_read_and_the_only_warning_is_the_power_failure() {
        let bytes = fixture("test2560.24o");
        let read = ok(read_rinex(&bytes, "test2560.24o"));
        // Header 17 lines; epoch 1: its line, one continuation (14 satellites >
        // 12) and 14 x 2 observation lines (7 types = 5 + 2) = 30; epoch 2: 1 +
        // 5 x 2 = 11; the flag 4 event and its comment = 2; epoch 4: 1 + 4 x 2 =
        // 9; the flag 5 event = 1; epoch 6: 1 + 2 x 2 = 5; epoch 7: 1 + 4 x 2 = 9.
        // 17 + 30 + 11 + 2 + 9 + 1 + 5 + 9 = 84.
        assert_eq!(line_count(&bytes), 84);
        assert_eq!(read.records_read, 84);
        assert_eq!(read.records_skipped, 0);
        assert_eq!(read.warnings.len(), 1, "{}", describe_warnings(&read));
        // Epoch 2 is line 17 + 30 + 1 = 48, and carries event flag 1.
        assert_eq!(read.warnings[0].record, 48);
        assert!(read.warnings[0].message.contains("lost power"));
        let metadata = &read.project.metadata;
        assert_eq!(metadata["power failures (event flag 1)"], "1");
        assert_eq!(
            metadata["external events (event flag 5)"],
            "1: 2024-09-12T10:01:15 GPS"
        );
        assert_eq!(
            metadata["comments"],
            "synthetic, from RINEX 2.11 tables A1 and A2\na comment after an event flag"
        );
        assert_eq!(metadata["leap seconds"], "18");
        assert_eq!(metadata["wavelength factors"], "1     1");
    }

    #[test]
    fn the_approximate_position_is_an_autonomous_geocentric_position_with_a_metres_sigma() {
        let read = ok(read_fixture("test2560.24o"));
        let project = &read.project;
        // The marker is named, not positioned: no grid coordinate is invented.
        assert!(project.points.is_empty());
        assert_eq!(project.unpositioned_points.len(), 1);
        assert_eq!(project.unpositioned_points[0].id, "TEST");
        assert_eq!(
            project.unpositioned_points[0].metadata["position"],
            "approximate only: the RINEX header's APPROX POSITION XYZ"
        );
        assert_eq!(project.observations.len(), 1);
        let position = position_at(project, 0);
        assert_eq!(position.point, "TEST");
        let geocentric = position.geocentric.expect("geocentric");
        assert!(position.geodetic.is_none());
        double_eq(geocentric.x, -4646000.1234);
        double_eq(geocentric.y, 2553000.5678);
        double_eq(geocentric.z, -3534000.9012);
        // 10 m per axis: 100 m^2 on the diagonal, nothing off it.
        double_eq(position.covariance.xx, 100.0);
        double_eq(position.covariance.yy, 100.0);
        double_eq(position.covariance.zz, 100.0);
        double_eq(position.covariance.xy, 0.0);
        assert_eq!(position.solution, GnssSolution::Autonomous);
        assert!(
            position.reference_frame.is_empty(),
            "the file declares none"
        );
        double_eq(position.antenna.height, 1.523);
        assert_eq!(
            position.source.record_number, 9,
            "APPROX POSITION XYZ is line 9"
        );
        assert!(any_not_carried_contains(&read, "survey-grade position"));
        assert!(any_not_carried_contains(&read, "reference frame"));
        assert!(any_not_carried_contains(&read, "29 satellite records"));
    }

    #[test]
    fn the_navigation_file_beside_it_is_read_and_summarised() {
        let read = ok(read_fixture_with(
            "test2560.24o",
            vec![sibling("test2560.24n", fixture("test2560.24n"))],
        ));
        // Two GPS ephemerides of eight lines each.
        assert_eq!(
            read.project.metadata["navigation files"],
            "test2560.24n (RINEX 2.11)"
        );
        assert_eq!(read.project.metadata["broadcast ephemerides GPS"], "2");
        assert_eq!(read.siblings_read.len(), 1);
        assert_eq!(read.siblings_read[0].name, "test2560.24n");
        assert_eq!(
            read.warnings.len(),
            1,
            "still only the power failure: {}",
            describe_warnings(&read)
        );
    }

    #[test]
    fn new_observation_types_after_an_event_change_how_many_lines_each_satellite_takes() {
        // Two types (one line a satellite) until the flag 4 event at line 9
        // redefines them as seven (two lines a satellite: five and two).
        let one_line = "  21000000.123   110000001.50008\n";
        let two_lines = "  21000000.123   110000001.50008  85000005.50007  21000004.123       \
                         -2000.250\n        44.500          40.250\n";
        let bytes = [
            header(
                "     2.11           OBSERVATION DATA    G (GPS)",
                "RINEX VERSION / TYPE",
            ),
            header("M2", "MARKER NAME"),
            header("     2    C1    L1", "# / TYPES OF OBSERV"),
            header(
                "  2024     9    12    10     0    0.0000000     GPS",
                "TIME OF FIRST OBS",
            ),
            header("", "END OF HEADER"), // lines 1-5
            String::from(" 24  9 12 10  0  0.0000000  0  2G01G02\n"),
            String::from(one_line),                // 6, 7
            String::from(one_line),                // 8
            format!("{}  4  1\n", " ".repeat(26)), // 9
            header(
                "     7    C1    L1    L2    P2    D1    S1    S2",
                "# / TYPES OF OBSERV",
            ), // 10
            String::from(" 24  9 12 10  0 30.0000000  0  2G01G03\n"),
            String::from(two_lines),
            String::from(two_lines), // 11, 12-15
            String::from(" 24  9 12 10  1  0.0000000  0  1G04\n"),
            String::from(two_lines), // 16, 17-18
        ]
        .concat();
        let read = ok(read_rinex(bytes.as_bytes(), "m2.24o"));
        let session = &read.project.gnss_sessions[0];
        assert_eq!(session.epoch_count, 3);
        assert_eq!(
            session.satellites_per_system["GPS"], 4,
            "G01, G02, G03 and G04"
        );
        assert_eq!(read.records_read, 18);
        assert_eq!(read.records_skipped, 0);
        assert_eq!(read.warnings.len(), 1, "{}", describe_warnings(&read));
        assert_eq!(read.warnings[0].record, 10);
        assert!(read.warnings[0].message.contains("change here from 2 to 7"));
        assert_eq!(
            read.project.metadata["observation types"],
            "C1 L1 L2 P2 D1 S1 S2"
        );
    }
}

// ---- RINEX 3.04 ----------------------------------------------------------------------

mod rinex3 {
    use super::*;

    #[test]
    fn a_mixed_file_counts_its_epochs_and_satellites_per_system_and_skips_cycle_slips() {
        let name = "TEST00AUS_R_20242561000_01H_30S_MO.rnx";
        let read = ok(read_fixture(name));
        assert_eq!(read.project.gnss_sessions.len(), 1);
        let session = &read.project.gnss_sessions[0];
        assert_eq!(session.marker_name, "PILLAR 7");
        assert_eq!(session.receiver_type, "TRIMBLE ALLOY");
        assert_eq!(session.antenna.r#type, "TRM59800.00     NONE");
        double_eq(session.antenna.height, 0.0);
        // Four observation epochs at 10:00:00, 10:00:30, 10:01:00, 10:01:30; the
        // flag 6 cycle-slip record at 10:00:30 is not an epoch.
        assert_eq!(session.epoch_count, 4);
        assert_eq!(survey_text(&session.first_epoch), "2024-09-12T10:00:00 GPS");
        assert_eq!(survey_text(&session.last_epoch), "2024-09-12T10:01:30 GPS");
        double_eq(session.interval_seconds.unwrap_or(0.0), 30.0);
        // GPS G05 G12 G15; GLONASS R01 R08; Galileo E11 E30; BeiDou C20.
        assert_eq!(
            session.satellites_per_system,
            gnss_counts(&[("GPS", 3), ("GLONASS", 2), ("Galileo", 2), ("BeiDou", 1)])
        );
        assert_eq!(session.format_version, "3.04");
        // 6 + 5 + 4 + 3 satellite records; one cycle-slip record.
        assert_eq!(read.project.metadata["satellite records"], "18");
        assert_eq!(
            read.project.metadata["cycle slip records (event flag 6)"],
            "1"
        );
        let marker = &read.project.unpositioned_points[0];
        assert_eq!(marker.metadata["marker type"], "GEODETIC");
        assert_eq!(marker.metadata["marker number"], "P7");
    }

    #[test]
    fn observation_types_with_continuation_lines_and_header_records_reach_the_metadata() {
        let name = "TEST00AUS_R_20242561000_01H_30S_MO.rnx";
        let bytes = fixture(name);
        let read = ok(read_rinex(&bytes, name));
        let metadata = &read.project.metadata;
        // Sixteen GPS types: thirteen on the record, three on its continuation.
        assert_eq!(
            metadata["observation types GPS"],
            "C1C L1C D1C S1C C2W L2W D2W S2W C5Q L5Q D5Q S5Q C1L L1L D1L S1L"
        );
        assert_eq!(metadata["observation types BeiDou"], "C2I L2I D2I S2I");
        assert_eq!(
            metadata["GLONASS slots and frequency numbers"],
            "3 R01  1 R02 -4 R08  6"
        );
        assert_eq!(metadata["signal strength unit"], "DBHZ");
        assert_eq!(metadata["RINEX version"], "3.04");
        assert_eq!(metadata["time system"], "GPS");
        // Header 23 lines; epochs 1 + 6, 1 + 5, 1 + 1 (cycle slip), 1 + 4, 1 + 3.
        // 23 + 7 + 6 + 2 + 5 + 4 = 47.
        assert_eq!(line_count(&bytes), 47);
        assert_eq!(read.records_read, 47);
        assert_eq!(read.records_skipped, 0);
        // The one record RINEX does not define is kept and said to be.
        assert_eq!(read.warnings.len(), 1, "{}", describe_warnings(&read));
        assert_eq!(read.warnings[0].record, 22);
        assert!(read.warnings[0].message.contains("MY OWN RECORD"));
        assert_eq!(
            metadata["header record MY OWN RECORD"],
            "something a receiver invented"
        );
    }

    // ---- RINEX 3.05: stop and go ---------------------------------------------------------

    #[test]
    fn event_flags_split_a_stop_and_go_file_into_one_session_per_occupation() {
        let name = "ROVR00AUS_R_20242561100_01H_01S_GO.rnx";
        let read = ok(read_fixture(name));
        let project = &read.project;
        // START (3 epochs), flag 2 -> moving (2 epochs), flag 3 -> CP1 (3 epochs),
        // flag 4 changes the antenna height -> CP1 again (2 epochs), flag 2 ->
        // moving (1 epoch), flag 3 -> CP2 (1 epoch).
        assert_eq!(project.gnss_sessions.len(), 4);
        let markers = ["START", "CP1", "CP1", "CP2"];
        let epochs = [3usize, 3, 2, 1];
        let heights = [2.0, 2.0, 1.8, 1.8];
        let gps = [4usize, 5, 4, 4];
        for i in 0..4 {
            let session = &project.gnss_sessions[i];
            assert_eq!(session.marker_name, markers[i], "{i}");
            assert_eq!(session.epoch_count, epochs[i], "{i}");
            double_eq(session.antenna.height, heights[i]);
            assert_eq!(session.satellites_per_system["GPS"], gps[i], "{i}");
            assert_eq!(
                session.receiver_type, "SEPT MOSAIC-X5",
                "the equipment moves with the rover"
            );
        }
        assert_eq!(
            survey_text(&project.gnss_sessions[1].first_epoch),
            "2024-09-12T11:00:06 GPS"
        );
        assert_eq!(
            survey_text(&project.gnss_sessions[1].last_epoch),
            "2024-09-12T11:00:08 GPS"
        );
        assert_eq!(
            survey_text(&project.gnss_sessions[3].first_epoch),
            "2024-09-12T11:00:14 GPS"
        );
        assert_eq!(project.metadata["kinematic epochs"], "3");

        // Three markers, each once; positions for START and both CP1 sessions,
        // none for CP2, whose occupation states no APPROX POSITION XYZ.
        assert_eq!(project.unpositioned_points.len(), 3);
        assert_eq!(project.observations.len(), 3);
        assert_eq!(position_at(project, 0).point, "START");
        assert_eq!(position_at(project, 1).point, "CP1");
        double_eq(
            position_at(project, 1).geocentric.expect("geocentric").x,
            -4646110.0,
        );
        double_eq(position_at(project, 2).antenna.height, 1.8);
        assert!(any_not_carried_contains(&read, "a position for CP2"));
        assert!(any_not_carried_contains(
            &read,
            "3 epochs recorded while the antenna was moving"
        ));
    }

    #[test]
    fn each_occupation_change_is_warned_at_the_event_that_caused_it() {
        let name = "ROVR00AUS_R_20242561100_01H_01S_GO.rnx";
        let bytes = fixture(name);
        let read = ok(read_rinex(&bytes, name));
        // Header 12 lines; three static epochs of 1 + 4 lines (13-27); the flag 2
        // event 28 and its comment 29; two kinematic epochs of 1 + 3 (30-37); the
        // flag 3 event 38 and its three records (39-41); three epochs of 1 + 5
        // (42-59); the flag 4 event 60 and its record 61; two epochs of 1 + 4
        // (62-71); flag 2 at 72; one epoch of 1 + 1 (73-74); flag 3 at 75 and its
        // record 76; one epoch of 1 + 4 (77-81).
        assert_eq!(line_count(&bytes), 81);
        assert_eq!(read.records_read, 81);
        assert_eq!(read.warnings.len(), 3, "{}", describe_warnings(&read));
        assert_eq!(read.warnings[0].record, 60);
        assert!(
            read.warnings[0]
                .message
                .contains("a new session starts here")
        );
        assert_eq!(read.warnings[1].record, 75);
        assert!(read.warnings[1].message.contains("1.8000 m"));
        assert_eq!(
            read.warnings[2].record, 28,
            "the kinematic epochs, from the first flag 2"
        );
        assert!(
            read.warnings[2]
                .message
                .contains("3 epochs were recorded with the antenna moving")
        );
    }

    #[test]
    fn the_specifications_own_example_reads_as_its_comments_describe() {
        // RINEX 3.05 (IGS/RTCM RINEX WG, 1 December 2020) table A4, example #1,
        // laid out by the format descriptors of tables A2 and A3 (the printed
        // "." ellipsis lines are not records and are left out). What it holds,
        // by its own comments: site A 9080 observed at 13:10:36 and 13:10:54 with
        // G06 G09 G12, R21 R22, E11 and S20; "FROM NOW ON KINEMATIC DATA" at
        // 13:11:12 and one moving epoch; "THE START OF A NEW SITE", A 9081 with an
        // antenna height of 0.9050, observed at 13:12:06 and 13:14:12 with G06
        // G09 G12 G16; an external event at 13:13:01.2345678.
        let name = "rinex305_example1.rnx";
        let bytes = fixture(name);
        let read = ok(read_rinex(&bytes, name));
        let project = &read.project;
        assert_eq!(project.gnss_sessions.len(), 2);
        let first = &project.gnss_sessions[0];
        assert_eq!(first.marker_name, "A 9080");
        assert_eq!(first.marker_number, "9080.1.34");
        assert_eq!(first.epoch_count, 2);
        assert_eq!(survey_text(&first.first_epoch), "2006-03-24T13:10:36 GPS");
        assert_eq!(survey_text(&first.last_epoch), "2006-03-24T13:10:54 GPS");
        double_eq(first.interval_seconds.unwrap_or(0.0), 18.0);
        double_eq(first.antenna.height, 0.903);
        assert_eq!(first.antenna.r#type, "ROVER");
        assert_eq!(
            first.satellites_per_system,
            gnss_counts(&[("GPS", 3), ("GLONASS", 2), ("Galileo", 1), ("SBAS", 1)])
        );

        let second = &project.gnss_sessions[1];
        assert_eq!(second.marker_name, "A 9081");
        assert_eq!(second.marker_number, "9081.1.34");
        assert_eq!(second.epoch_count, 2);
        assert_eq!(survey_text(&second.first_epoch), "2006-03-24T13:12:06 GPS");
        assert_eq!(survey_text(&second.last_epoch), "2006-03-24T13:14:12 GPS");
        double_eq(second.antenna.height, 0.905);
        assert_eq!(second.satellites_per_system["GPS"], 4);
        assert_eq!(
            second.receiver_type, "GEODETIC",
            "REC # / TYPE / VERS: X1234A123, GEODETIC"
        );

        assert_eq!(project.metadata["kinematic epochs"], "1");
        assert_eq!(
            project.metadata["external events (event flag 5)"],
            "1: 2006-03-24T13:13:01.2345678 GPS"
        );
        // A 9080 has the header's position (4375274, 587466, 4589095); A 9081 is
        // given none.
        assert_eq!(project.observations.len(), 1);
        assert_eq!(position_at(project, 0).point, "A 9080");
        double_eq(
            position_at(project, 0).geocentric.expect("geocentric").z,
            4589095.0,
        );
        assert!(any_not_carried_contains(&read, "a position for A 9081"));

        // 78 lines, every one a record. Two warnings: A 9081's epochs are 126 s
        // apart against the header's 18 s (reported at its MARKER NAME, line 57),
        // and the one kinematic epoch (reported at the flag 2 event, line 48:
        // header 33 lines, then 1 + 5 and 1 + 7 for the two epochs).
        assert_eq!(line_count(&bytes), 78);
        assert_eq!(read.records_read, 78);
        assert_eq!(read.records_skipped, 0);
        assert_eq!(read.warnings.len(), 2, "{}", describe_warnings(&read));
        assert_eq!(read.warnings[0].record, 57);
        assert!(read.warnings[0].message.contains("mostly 126.000 s apart"));
        assert_eq!(read.warnings[1].record, 48);
    }
}

// ---- RINEX 4.01 ----------------------------------------------------------------------

mod rinex4 {
    use super::*;

    #[test]
    fn a_version_four_file_is_read_with_its_new_header_records_and_long_named_navigation_file() {
        let name = "BASE00AUS_R_20242561200_01H_30S_MO.rnx";
        let navigation = "BASE00AUS_R_20242561200_01H_MN.rnx";
        let read = ok(read_fixture_with(
            name,
            vec![sibling(navigation, fixture(navigation))],
        ));
        assert!(read.warnings.is_empty(), "{}", describe_warnings(&read));
        let session = &read.project.gnss_sessions[0];
        assert_eq!(session.marker_name, "BASE");
        assert_eq!(session.format_version, "4.01");
        assert_eq!(session.epoch_count, 2);
        // No INTERVAL record: the epochs, 12:00:00 and 12:00:30, give 30 s.
        double_eq(session.interval_seconds.unwrap_or(0.0), 30.0);
        double_eq(session.antenna.height, 0.305);
        assert_eq!(
            session.satellites_per_system,
            gnss_counts(&[("GPS", 2), ("Galileo", 1)])
        );
        let metadata = &read.project.metadata;
        assert_eq!(metadata["DOI"], "https://doi.org/10.0000/katana-fixture");
        assert_eq!(metadata["licence of use"], "CC BY 4.0");
        assert_eq!(
            metadata["station information"],
            "https://example.org/station/BASE"
        );
        // The long observation name ..._01H_30S_MO.rnx has its mixed navigation
        // file at ..._01H_MN.rnx: G05, G07 (LNAV) and E11 (INAV); the ION record
        // is not an ephemeris.
        assert_eq!(
            metadata["navigation files"],
            format!("{navigation} (RINEX 4.01)")
        );
        assert_eq!(metadata["broadcast ephemerides GPS"], "2");
        assert_eq!(metadata["broadcast ephemerides Galileo"], "1");
        let position = position_at(&read.project, 0);
        double_eq(position.geocentric.expect("geocentric").y, 2553200.25);
    }
}

mod rinex_navigation {
    use super::*;

    #[test]
    fn a_missing_navigation_file_is_said_and_an_unreadable_one_is_a_warning() {
        // None beside it: the metadata names what was looked for.
        let alone = ok(read_fixture("test2560.24o"));
        assert_eq!(
            alone.project.metadata["navigation files"],
            "none beside the observation file (looked for test2560.24n, test2560.24g, \
             test2560.24l, test2560.24h, test2560.24p)"
        );

        // A file with the right name that is not a navigation file.
        let wrong = ok(read_fixture_with(
            "test2560.24o",
            vec![sibling("test2560.24g", b"not RINEX at all\n".to_vec())],
        ));
        assert!(any_warning_contains(
            &wrong,
            "the navigation file test2560.24g is not used"
        ));

        // A lookup that fails for another reason than absence.
        let unreadable: SiblingLookup = Arc::new(|_: &str| -> Result<Vec<u8>> {
            Err(Error::new(
                ErrorCode::FileImportFailure,
                "the disk is unreadable",
            ))
        });
        let options = ReadOptions {
            siblings: Some(unreadable),
            ..Default::default()
        };
        let bytes = fixture("test2560.24o");
        let failing = ok(read_survey(
            format_registry(),
            RINEX_OBSERVATION_FORMAT_ID,
            &bytes,
            "test2560.24o",
            &options,
        ));
        assert!(any_warning_contains(
            &failing,
            "could not be read: the disk is unreadable"
        ));

        // Any other name: stem.nav, as converters write it.
        let navigation = fixture("test2560.24n");
        let other = ok(read_rinex_with(
            &bytes,
            "site.obs",
            vec![sibling("site.nav", navigation)],
        ));
        assert_eq!(
            other.project.metadata["navigation files"],
            "site.nav (RINEX 2.11)"
        );
    }
}

// ---- Damaged and unusual files ---------------------------------------------------------

mod rinex_damage {
    use super::*;

    #[test]
    fn an_epoch_promising_more_satellites_than_follow_is_warned_and_the_next_epoch_still_counts() {
        // Header 11 lines; epoch at line 12 promises 3 satellites, 2 follow
        // (13, 14); the next epoch at 15 has 1 (16).
        let bytes = [
            minimal_v3_header(DEFAULT_APPROX),
            epoch_v3(0, 0.0, 0, 3),
            satellite_v3("G01"),
            satellite_v3("G02"),
            epoch_v3(0, 30.0, 0, 1),
            satellite_v3("G03"),
        ]
        .concat();
        let read = ok(read_rinex(bytes.as_bytes(), "damaged.rnx"));
        assert_eq!(read.project.gnss_sessions[0].epoch_count, 2);
        assert_eq!(
            read.project.gnss_sessions[0].satellites_per_system["GPS"],
            3
        );
        assert_eq!(read.warnings.len(), 1, "{}", describe_warnings(&read));
        assert_eq!(read.warnings[0].record, 12);
        assert!(
            read.warnings[0]
                .message
                .contains("lists 3 satellites but only 2")
        );
    }

    #[test]
    fn unreadable_records_are_warned_by_line_and_counted_as_skipped() {
        // Line 12 epoch; 13 a good G01; 14 a G02 with letters in it; 15 an E05
        // for which the header declares no types; 16 an epoch with month 13 and
        // one satellite (17); 18 a line that is no record at all, 19 more of the
        // same; 20 a good epoch with 21 its satellite.
        let bytes = [
            minimal_v3_header(DEFAULT_APPROX),
            epoch_v3(0, 0.0, 0, 3),
            satellite_v3("G01"),
            String::from("G02  2100000X.000\n"),
            satellite_v3("E05"),
            String::from("> 2024 13 12 10 00 30.0000000  0  1\n"),
            satellite_v3("G04"),
            String::from("garbage\n"),
            String::from("more garbage\n"),
            epoch_v3(1, 0.0, 0, 1),
            satellite_v3("G05"),
        ]
        .concat();
        let read = ok(read_rinex(bytes.as_bytes(), "damaged.rnx"));
        let session = &read.project.gnss_sessions[0];
        assert_eq!(
            session.epoch_count, 2,
            "10:00:00 and 10:01:00; the month-13 epoch is not one"
        );
        assert_eq!(session.satellites_per_system["GPS"], 2, "G01 and G05 only");
        assert!(!session.satellites_per_system.contains_key("Galileo"));
        let lines: Vec<usize> = read.warnings.iter().map(|warning| warning.record).collect();
        assert_eq!(lines, vec![14, 15, 16, 18], "{}", describe_warnings(&read));
        assert!(any_warning_contains(
            &read,
            "holds characters an observation field"
        ));
        assert!(any_warning_contains(
            &read,
            "declares no observation types for Galileo"
        ));
        assert!(any_warning_contains(&read, "month 13 does not exist"));
        assert!(any_warning_contains(&read, "skipped it and the next line"));
        // Skipped: 14, 15, the bad epoch 16 and its record 17, 18 and 19.
        assert_eq!(read.records_skipped, 6);
    }

    #[test]
    fn a_rinex_two_observation_line_with_foreign_characters_is_warned_and_its_satellite_not_counted()
     {
        let bytes = fixture("test2560.24o");
        // Line 20 is G01's first observation line in the first epoch (header 17,
        // epoch line 18, continuation 19). Put a letter in it. The lines are cut
        // as std::getline cuts them, at LF only.
        let mut lines: Vec<Vec<u8>> = bytes.split(|&c| c == b'\n').map(<[u8]>::to_vec).collect();
        if bytes.ends_with(b"\n") {
            lines.pop();
        }
        assert!(lines.len() >= 20);
        lines[19][5] = b'Z';
        let mut damaged = Vec::new();
        for line in &lines {
            damaged.extend_from_slice(line);
            damaged.push(b'\n');
        }
        let read = ok(read_rinex(&damaged, "test2560.24o"));
        assert!(
            read.warnings.iter().any(|warning| warning.record == 20
                && warning.message.contains("holds characters")),
            "{}",
            describe_warnings(&read)
        );
        assert_eq!(read.records_skipped, 1);
        // G01 is still seen in later epochs, so the count is unchanged; the
        // satellite records drop by the one that could not be read.
        assert_eq!(read.project.metadata["satellite records"], "28");
        assert_eq!(
            read.project.gnss_sessions[0].satellites_per_system["GPS"],
            10
        );
    }

    #[test]
    fn an_event_with_a_damaged_time_still_has_its_header_records_read() {
        // Line 12 epoch at M1 with G01 (13); line 14 a new site occupation whose
        // month is 13, followed by MARKER NAME M2 (15); line 16 an epoch with G02
        // (17), which belongs to M2.
        let bytes = [
            minimal_v3_header(DEFAULT_APPROX),
            epoch_v3(0, 0.0, 0, 1),
            satellite_v3("G01"),
            String::from("> 2024 13 12 10 00 15.0000000  3  1\n"),
            header("M2", "MARKER NAME"),
            epoch_v3(0, 30.0, 0, 1),
            satellite_v3("G02"),
        ]
        .concat();
        let read = ok(read_rinex(bytes.as_bytes(), "event.rnx"));
        assert_eq!(read.project.gnss_sessions.len(), 2);
        assert_eq!(read.project.gnss_sessions[1].marker_name, "M2");
        assert_eq!(read.project.gnss_sessions[1].epoch_count, 1);
        assert!(
            read.warnings.iter().any(|warning| warning.record == 14
                && warning.message.contains("month 13 does not exist")),
            "{}",
            describe_warnings(&read)
        );
        assert_eq!(read.records_skipped, 0);
    }

    #[test]
    fn a_file_cut_inside_an_epoch_is_warned_and_what_was_read_is_kept() {
        let bytes = [
            minimal_v3_header(DEFAULT_APPROX),
            epoch_v3(0, 0.0, 0, 1),
            satellite_v3("G01"),
            epoch_v3(0, 30.0, 0, 4),
            satellite_v3("G02"),
        ]
        .concat();
        let read = ok(read_rinex(bytes.as_bytes(), "cut.rnx"));
        assert_eq!(read.project.gnss_sessions[0].epoch_count, 2);
        assert_eq!(read.warnings.len(), 1, "{}", describe_warnings(&read));
        assert_eq!(read.warnings[0].record, 14);
        assert!(
            read.warnings[0]
                .message
                .contains("3 of its 4 satellite records are missing")
        );
    }

    #[test]
    fn epochs_out_of_order_and_a_wrong_header_interval_are_warned() {
        let mut head = minimal_v3_header(DEFAULT_APPROX);
        let end = head
            .find(&format!("{}END OF HEADER", " ".repeat(60)))
            .expect("the header ends");
        head.insert_str(end, &header("     5.000", "INTERVAL"));
        // Header now 12 lines: epochs at 13 (10:00:30), 15 (10:00:00, earlier
        // than the one before), 17 (10:01:00), 19 (10:01:30) and 21 (10:02:00).
        // Gaps after the backward step: 60, 30, 30 - mostly 30 s, not the 5 s the
        // header says.
        let bytes = [
            head,
            epoch_v3(0, 30.0, 0, 1),
            satellite_v3("G01"),
            epoch_v3(0, 0.0, 0, 1),
            satellite_v3("G01"),
            epoch_v3(1, 0.0, 0, 1),
            satellite_v3("G01"),
            epoch_v3(1, 30.0, 0, 1),
            satellite_v3("G01"),
            epoch_v3(2, 0.0, 0, 1),
            satellite_v3("G01"),
        ]
        .concat();
        let read = ok(read_rinex(bytes.as_bytes(), "order.rnx"));
        let session = &read.project.gnss_sessions[0];
        assert_eq!(survey_text(&session.first_epoch), "2024-09-12T10:00:00 GPS");
        assert_eq!(survey_text(&session.last_epoch), "2024-09-12T10:02:00 GPS");
        assert_eq!(session.epoch_count, 5);
        // The header's, as stated.
        double_eq(session.interval_seconds.unwrap_or(0.0), 5.0);
        assert!(
            read.warnings.iter().any(|warning| warning.record == 15
                && warning.message.contains("earlier than the epoch before it")),
            "{}",
            describe_warnings(&read)
        );
        assert!(
            any_warning_contains(
                &read,
                "interval of 5.000 s, but the epochs of M1 are mostly 30.000 s apart"
            ),
            "{}",
            describe_warnings(&read)
        );
        // TIME OF FIRST OBS says 10:00:00 and the earliest epoch is 10:00:00.
        assert!(!any_warning_contains(&read, "TIME OF FIRST OBS"));
    }

    #[test]
    fn a_zero_or_implausible_approximate_position_gives_no_position() {
        let zero = [
            minimal_v3_header("        0.0000        0.0000        0.0000"),
            epoch_v3(0, 0.0, 0, 1),
            satellite_v3("G01"),
        ]
        .concat();
        let none = ok(read_rinex(zero.as_bytes(), "zero.rnx"));
        assert!(none.project.observations.is_empty());
        assert!(any_not_carried_contains(
            &none,
            "a position for M1 (its header position is 0, 0, 0)"
        ));
        assert!(!any_not_carried_contains(&none, "survey-grade"));

        // Kilometres where metres belong: 6 371 m from the centre of the earth.
        let kilometres = [
            minimal_v3_header("    -4646.0000     2553.0000    -3534.0000"),
            epoch_v3(0, 0.0, 0, 1),
            satellite_v3("G01"),
        ]
        .concat();
        let wrong = ok(read_rinex(kilometres.as_bytes(), "km.rnx"));
        assert!(wrong.project.observations.is_empty());
        assert_eq!(wrong.warnings.len(), 1, "{}", describe_warnings(&wrong));
        assert_eq!(wrong.warnings[0].record, 7);
        // sqrt(4646^2 + 2553^2 + 3534^2) = 6371.2 m = 6.4 km.
        assert!(
            wrong.warnings[0]
                .message
                .contains("6.4 km from the earth's centre")
        );
    }

    #[test]
    fn a_header_without_a_marker_name_names_the_marker_after_the_file() {
        let marker = header("M1", "MARKER NAME");
        let head = minimal_v3_header(DEFAULT_APPROX).replacen(&marker, "", 1);
        let bytes = [head, epoch_v3(0, 0.0, 0, 1), satellite_v3("G01")].concat();
        let read = ok(read_rinex(
            bytes.as_bytes(),
            "SITE00AUS_R_20242561000_01H_30S_GO.rnx",
        ));
        assert_eq!(read.project.gnss_sessions[0].marker_name, "SITE00AUS");
        assert_eq!(read.project.unpositioned_points[0].id, "SITE00AUS");
        assert!(any_warning_contains(
            &read,
            "no MARKER NAME; the marker is named 'SITE00AUS'"
        ));
    }

    #[test]
    fn files_that_cannot_be_read_at_all_are_errors_a_surveyor_can_act_on() {
        let error_of = |bytes: &str, name: &str| {
            read_rinex(bytes.as_bytes(), name).expect_err("the file is refused")
        };
        assert!(error_of("", "empty.rnx").message.contains("is empty"));
        assert!(
            error_of("P1,1,2,3\n", "points.rnx")
                .message
                .contains("not a RINEX file")
        );

        let mut no_end = minimal_v3_header(DEFAULT_APPROX);
        let end = no_end
            .find(&format!("{}END OF HEADER", " ".repeat(60)))
            .expect("the header ends");
        no_end.truncate(end);
        assert!(
            error_of(&no_end, "noend.rnx")
                .message
                .contains("no END OF HEADER")
        );

        let types = header("G    4 C1C L1C D1C S1C", "SYS / # / OBS TYPES");
        let no_types = minimal_v3_header(DEFAULT_APPROX).replacen(&types, "", 1);
        assert!(
            error_of(&no_types, "notypes.rnx")
                .message
                .contains("declares no observation types")
        );

        let mut version_one = minimal_v3_header(DEFAULT_APPROX);
        version_one.replace_range(0..9, "     1.00");
        let old = error_of(&version_one, "old.rnx");
        assert_eq!(old.code, ErrorCode::Unsupported);
        assert!(old.message.contains("versions 2, 3 and 4"));

        let mut meteorological = minimal_v3_header(DEFAULT_APPROX);
        meteorological.replace_range(20..21, "M");
        assert!(
            error_of(&meteorological, "met.rnx")
                .message
                .contains("meteorological")
        );
    }

    #[test]
    fn cr_lf_line_ends_read_exactly_as_lf_do() {
        // The fixture as checked out may have either; make both from it.
        let name = "TEST00AUS_R_20242561000_01H_30S_MO.rnx";
        let lf: Vec<u8> = fixture(name).into_iter().filter(|&c| c != b'\r').collect();
        let mut crlf = Vec::with_capacity(lf.len() * 2);
        for &c in &lf {
            if c == b'\n' {
                crlf.push(b'\r');
            }
            crlf.push(c);
        }
        let a = ok(read_rinex(&lf, name));
        let b = ok(read_rinex(&crlf, name));
        assert_eq!(a.project, b.project);
        assert_eq!(a.records_read, b.records_read);
        assert_eq!(a.warnings, b.warnings);
    }
}

// ---- Robustness -----------------------------------------------------------------------

// Reading must end in a value or an error - and an error must never be the
// reader panicking (read_survey turns that into Internal).
fn expect_handled(bytes: &[u8], name: &str, what: &str) {
    if let Err(error) = read_rinex(bytes, name) {
        assert_ne!(
            error.code,
            ErrorCode::Internal,
            "{what}: {}",
            error.describe()
        );
    }
}

const ALL_FIXTURES: [&str; 12] = [
    "test2560.24o",
    "TEST00AUS_R_20242561000_01H_30S_MO.rnx",
    "ROVR00AUS_R_20242561100_01H_01S_GO.rnx",
    "BASE00AUS_R_20242561200_01H_30S_MO.rnx",
    "test2560.24d",
    "TEST00AUS_R_20242561000_01H_30S_MO.crx",
    "ROVR00AUS_R_20242561100_01H_01S_GO.crx",
    "BASE00AUS_R_20242561200_01H_30S_MO.crx",
    "rinex305_example1.crx",
    "rinex305_example1.rnx",
    "test2560.24n",
    "BASE00AUS_R_20242561200_01H_MN.rnx",
];

fn epochs_in(project: &SurveyProject) -> usize {
    project
        .gnss_sessions
        .iter()
        .map(|session| session.epoch_count)
        .sum()
}

mod rinex_robustness {
    use super::*;

    #[test]
    fn every_truncation_of_every_fixture_is_read_or_refused_without_a_crash() {
        for name in ALL_FIXTURES {
            let bytes = fixture(name);
            let whole = read_rinex(&bytes, name)
                .map(|read| epochs_in(&read.project))
                .unwrap_or(0);
            for length in 0..=bytes.len() {
                match read_rinex(&bytes[..length], name) {
                    Err(error) => assert_ne!(
                        error.code,
                        ErrorCode::Internal,
                        "{name} cut at {length}: {}",
                        error.describe()
                    ),
                    // A part of a file never holds more epochs than the whole.
                    Ok(read) => {
                        assert!(epochs_in(&read.project) <= whole, "{name} cut at {length}")
                    }
                }
            }
        }
    }

    #[test]
    fn random_bytes_and_random_damage_are_read_or_refused_without_a_crash() {
        let mut random = Random32::new(20240912); // fixed: a failure must be repeatable
        for i in 0..300 {
            let length = u64::from(random.next_u32()) % 3000;
            let mut bytes = vec![0u8; length as usize];
            for c in &mut bytes {
                *c = random.uniform(0, 255) as u8;
            }
            expect_handled(&bytes, "random.rnx", &format!("random bytes {i}"));
            // The same bytes behind a real RINEX first line, so they reach the
            // header and body readers instead of stopping at the version check.
            let mut behind = minimal_v3_header(DEFAULT_APPROX).into_bytes();
            behind.extend_from_slice(&bytes);
            expect_handled(&behind, "random.rnx", &format!("random body {i}"));
        }
        // Damage: a few bytes of each fixture replaced with digits, blanks, signs,
        // '>' and line ends - the characters that change what a record means.
        let alphabet: &[u8] = b"0123456789 .->\n\rGREC*";
        for name in ALL_FIXTURES {
            let original = fixture(name);
            for i in 0..200 {
                let mut bytes = original.clone();
                let changes = 1 + u64::from(random.next_u32()) % 8;
                for _ in 0..changes {
                    // C++17 evaluates the right of an assignment first: the
                    // character is drawn before the place.
                    let character = alphabet[random.uniform(0, alphabet.len() as u64 - 1) as usize];
                    let at = (u64::from(random.next_u32()) % bytes.len() as u64) as usize;
                    bytes[at] = character;
                }
                expect_handled(&bytes, name, &format!("{name} damaged {i}"));
            }
        }
    }
}

// ---- Throughput -------------------------------------------------------------------------

// A synthetic RINEX 3.04 file at 1 Hz: 30 satellites (12 GPS, 8 GLONASS, 10
// Galileo), eight observation types each - 131-character satellite records,
// about 4 kB an epoch.
fn synthetic_v3(epochs: usize) -> String {
    let mut text = [
        header(
            "     3.04           OBSERVATION DATA    M",
            "RINEX VERSION / TYPE",
        ),
        header("katana test", "PGM / RUN BY / DATE"),
        header("BIG", "MARKER NAME"),
        header(
            " -4646000.0000  2553000.0000 -3534000.0000",
            "APPROX POSITION XYZ",
        ),
        header(
            "        1.0000        0.0000        0.0000",
            "ANTENNA: DELTA H/E/N",
        ),
        header(
            "G    8 C1C L1C D1C S1C C2W L2W D2W S2W",
            "SYS / # / OBS TYPES",
        ),
        header(
            "R    8 C1C L1C D1C S1C C2C L2C D2C S2C",
            "SYS / # / OBS TYPES",
        ),
        header(
            "E    8 C1X L1X D1X S1X C5X L5X D5X S5X",
            "SYS / # / OBS TYPES",
        ),
        header("     1.000", "INTERVAL"),
        header(
            "  2024     9    12     0     0    0.0000000     GPS",
            "TIME OF FIRST OBS",
        ),
        header("", "END OF HEADER"),
    ]
    .concat();
    let mut satellites = Vec::new();
    for n in 1..=12 {
        satellites.push(format!("G{n:02}"));
    }
    for n in 1..=8 {
        satellites.push(format!("R{n:02}"));
    }
    for n in 1..=10 {
        satellites.push(format!("E{n:02}"));
    }
    // Eight F14.3,I1,I1 fields of 16 characters: 128.
    let values = [
        "  21000000.123  ",
        " 110000001.50008",
        "     -2000.250  ",
        "        44.500  ",
        "  21000004.123  ",
        "  85000005.50007",
        "     -1500.250  ",
        "        40.250  ",
    ]
    .concat();
    text.reserve(epochs * (57 + 30 * (3 + values.len() + 1)));
    for e in 0..epochs {
        let day = e / 86400;
        let hour = (e / 3600) % 24;
        let minute = (e / 60) % 60;
        let second = (e % 60) as f64;
        text.push_str(&format!(
            "> 2024 09 {:02} {hour:02} {minute:02}{second:11.7}  0 30\n",
            12 + day
        ));
        for satellite in &satellites {
            text.push_str(satellite);
            text.push_str(&values);
            text.push('\n');
        }
    }
    text
}

// The same in RINEX 2.11: GPS and GLONASS, 30 satellites, seven observation
// types - two lines a satellite, about 3.5 kB an epoch.
fn synthetic_v2(epochs: usize) -> String {
    let mut text = [
        header(
            "     2.11           OBSERVATION DATA    M (MIXED)",
            "RINEX VERSION / TYPE",
        ),
        header("katana test", "PGM / RUN BY / DATE"),
        header("BIG", "MARKER NAME"),
        header(
            " -4646000.0000  2553000.0000 -3534000.0000",
            "APPROX POSITION XYZ",
        ),
        header(
            "        1.0000        0.0000        0.0000",
            "ANTENNA: DELTA H/E/N",
        ),
        header(
            "     7    C1    L1    L2    P2    D1    S1    S2",
            "# / TYPES OF OBSERV",
        ),
        header("     1.000", "INTERVAL"),
        header(
            "  2024     9    12     0     0    0.0000000     GPS",
            "TIME OF FIRST OBS",
        ),
        header("", "END OF HEADER"),
    ]
    .concat();
    let mut list = String::new();
    for n in 1..=18 {
        list.push_str(&format!("G{n:02}"));
    }
    for n in 1..=12 {
        list.push_str(&format!("R{n:02}"));
    }
    // Seven F14.3,I1,I1 fields: five on the first line, two on the second.
    let first = [
        "  21000000.123  ",
        " 110000001.50008",
        "  85000005.50007",
        "  21000004.123  ",
        "     -2000.250  ",
    ]
    .concat();
    let second = "        44.500          40.250";
    for e in 0..epochs {
        let hour = (e / 3600) % 24;
        let minute = (e / 60) % 60;
        let seconds = (e % 60) as f64;
        text.push_str(&format!(
            " 24  9 12 {hour:2} {minute:2}{seconds:11.7}  0 30"
        ));
        text.push_str(&list[..36]);
        text.push('\n');
        let mut at = 36;
        while at < list.len() {
            text.push_str(&" ".repeat(32));
            text.push_str(&list[at..list.len().min(at + 36)]);
            text.push('\n');
            at += 36;
        }
        for _ in 0..30 {
            text.push_str(&first);
            text.push('\n');
            text.push_str(second);
            text.push('\n');
        }
    }
    text
}

fn megabytes_per_second(bytes: usize, elapsed: std::time::Duration) -> f64 {
    bytes as f64 / 1e6 / elapsed.as_secs_f64().max(1e-9)
}

fn throughput_megabytes() -> usize {
    match std::env::var("KATANA_RINEX_THROUGHPUT_MB") {
        Ok(wanted) => atoi(&wanted).max(1) as usize,
        Err(_) => 50,
    }
}

mod rinex_throughput {
    use super::*;

    #[test]
    fn a50_megabyte_one_hertz_file_is_counted_not_stored() {
        // ~4 kB an epoch: 12 500 epochs is about 50 MB. KATANA_RINEX_THROUGHPUT_MB
        // scales it for a measurement by hand (200 for a day-sized file).
        let epochs = throughput_megabytes() * 250;
        let bytes = synthetic_v3(epochs);
        let start = std::time::Instant::now();
        let read = read_rinex(bytes.as_bytes(), "BIG00AUS_R_20242560000_01D_01S_MO.rnx");
        let elapsed = start.elapsed();
        let read = ok(read);
        assert!(read.warnings.is_empty(), "{}", describe_warnings(&read));
        let session = &read.project.gnss_sessions[0];
        assert_eq!(session.epoch_count, epochs);
        assert_eq!(
            session.satellites_per_system,
            gnss_counts(&[("GPS", 12), ("GLONASS", 8), ("Galileo", 10)])
        );
        assert_eq!(
            read.project.metadata["satellite records"],
            format!("{}", epochs * 30)
        );
        let rate = megabytes_per_second(bytes.len(), elapsed);
        println!(
            "[ RINEX    ] read {} MB, {epochs} epochs in {} s: {rate} MB/s",
            bytes.len() as f64 / 1e6,
            elapsed.as_secs_f64()
        );
        println!(
            "megabytes_per_second={}",
            katana_core::text::format_exact_real(rate)
        );
    }

    #[test]
    fn a50_megabyte_rinex_two_file_is_counted_not_stored() {
        // ~3.5 kB an epoch; a day at 1 Hz is 86 400 epochs, so the time of day wraps
        // only past 300 MB.
        let epochs = (throughput_megabytes() * 282).min(86_400);
        let bytes = synthetic_v2(epochs);
        let start = std::time::Instant::now();
        let read = read_rinex(bytes.as_bytes(), "big02560.24o");
        let elapsed = start.elapsed();
        let read = ok(read);
        assert!(read.warnings.is_empty(), "{}", describe_warnings(&read));
        let session = &read.project.gnss_sessions[0];
        assert_eq!(session.epoch_count, epochs);
        assert_eq!(
            session.satellites_per_system,
            gnss_counts(&[("GPS", 18), ("GLONASS", 12)])
        );
        assert_eq!(
            read.project.metadata["satellite records"],
            format!("{}", epochs * 30)
        );
        let rate = megabytes_per_second(bytes.len(), elapsed);
        println!(
            "[ RINEX 2  ] read {} MB, {epochs} epochs in {} s: {rate} MB/s",
            bytes.len() as f64 / 1e6,
            elapsed.as_secs_f64()
        );
        println!(
            "megabytes_per_second={}",
            katana_core::text::format_exact_real(rate)
        );
    }
}
