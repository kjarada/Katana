//! Twin of tests/surveyio/test_sokkia_sdr.cpp: the Sokkia SDR reader
//! (src/sokkia_sdr.rs).
//!
//! data/sdr/traverse.sdr is written by hand from the SDR33 record layouts of
//! "Interfacing with the SOKKIA SDR Electronic Field Book" (October 1999),
//! section 3.6.2: a header in degrees, metres, millibars and Celsius with the
//! coordinates east first; two keyed control marks; a setup on CP1 with two
//! rounds of face 1 and face 2 to its backsight CP2 and to T1; a block of
//! deleted ("DD") records; a setup on T1, its circle zeroed on its backsight
//! CP1 (azimuth 270), observing CP1 and T2. Point ids are right-justified in
//! their 16 columns, as some writers pad them. Every value below is read off
//! the record text by hand: the fields of an observation are slope distance
//! (columns 37-52), vertical reading (53-68) and horizontal reading (69-84),
//! in decimal degrees, so 87.00000000 is 87 * pi / 180.

#![allow(
    clippy::excessive_precision,
    reason = "Expected values are the C++ test's literals, digit for digit."
)]

mod topcon_support;

use std::collections::BTreeSet;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use katana_core::text::parse_finite_double;
use katana_core::{ErrorCode, Result};
use katana_survey::data_model::validate_observation;
use katana_survey::{
    AngularUnit, AtmosphericCorrection, ComputedPoint, CoordinateSource, CorrectionState,
    DistanceKind, DistanceObservation, Face, HorizontalDirectionObservation, LinearUnit,
    PrismConstantPolicy, ReductionContext, ReductionOutcome, ReductionReport, ReductionSettings,
    SurveyProject, SurveyStation, SurveyTimestamp, ZenithAngleObservation, reduce_and_adjust,
};
use katana_surveyio::{
    DetectionOutcome, IDENTIFIED_CONFIDENCE, PROBE_BYTES, ReadResult, detect_format,
    format_registry, probe_of,
};

use topcon_support::{
    FOREIGN_SAMPLES, any_not_carried_contains, any_warning_contains, data_directory, double_eq,
    feature, fixture, near, observations_to, ok, point, read, read_every_truncation_and_noise,
    unpositioned,
};

const ID: &str = "sokkia-sdr";
const ANGLE_TOLERANCE: f64 = 1e-12;
const LENGTH_TOLERANCE: f64 = 1e-9;

// The lines of a file, each ended with LF.
macro_rules! records {
    ($($line:expr),* $(,)?) => {{
        let mut text = String::new();
        $(
            text.push_str(&($line));
            text.push('\n');
        )*
        text
    }};
}

fn degrees(value: f64) -> f64 {
    value * PI / 180.0
}

fn read_traverse() -> ReadResult {
    ok(read(ID, fixture("sdr/traverse.sdr"), "traverse.sdr"))
}

fn warned(result: &ReadResult, record: usize, words: &str) -> bool {
    any_warning_contains(result, record, words)
}

fn warnings_with(result: &ReadResult, words: &str) -> usize {
    result
        .warnings
        .iter()
        .filter(|warning| warning.message.contains(words))
        .count()
}

fn report_warnings_with(report: &ReductionReport, words: &str) -> usize {
    report
        .warnings
        .iter()
        .filter(|warning| warning.text.contains(words))
        .count()
}

// A field of `width` characters, the text left-justified: [SDR] 3.2 pads an
// alpha field on the right, and a real is written from its first column.
fn field(text: &str, width: usize) -> String {
    let mut padded = String::from(text);
    padded.truncate(width);
    while padded.len() < width {
        padded.push(' ');
    }
    padded
}

fn f16(text: &str) -> String {
    field(text, 16)
}

fn f10(text: &str) -> String {
    field(text, 10)
}

// An SDR33 header: version, serial number, date and time, then the six
// options ([SDR] 3.6.2).
fn header33(options: &str) -> String {
    format!(
        "00NM{}0001{}{options}",
        f16("SDR33 V04-03.00"),
        f16("28-Sep-26 08:00")
    )
}

// The C++'s header33() with its default options: degrees, metres, mbar,
// Celsius, north first, angles right.
fn header() -> String {
    header33("113111")
}

// A header written as the Nikon and Spectra manuals give theirs: no blank
// between "SDR33" and the version.
fn header_nikon(options: &str) -> String {
    format!(
        "00NM{}0000{}{options}",
        f16("SDR33V04-01"),
        f16("28-Sep-26 08:00")
    )
}

// The lines of a file, each ended as `ending` says.
fn ended(lf_text: &str, ending: &str) -> String {
    lf_text.replace('\n', ending)
}

// An SDR33 instrument record: a manual EDM, theodolite "HAND" serial 1,
// mounting 3, the vertical angle option given, no offsets, prism constant 0.
fn instrument33_with(vertical_option: char) -> String {
    format!(
        "01NM:{}000000{}0000013{vertical_option}{}{}{}",
        field("", 16),
        f16("HAND"),
        f16(""),
        f16(""),
        f16("0")
    )
}

fn instrument33() -> String {
    instrument33_with('1')
}

fn read_text(text: impl AsRef<[u8]>) -> Result<ReadResult> {
    read(ID, text, "job.sdr")
}

// The i-th setup; a read that went wrong fails the test rather than indexing
// past the end.
fn setup_at(project: &SurveyProject, i: usize) -> &SurveyStation {
    project
        .stations
        .get(i)
        .unwrap_or_else(|| panic!("no setup {i}: {} were read", project.stations.len()))
}

// The value of `stated`, or a failure when it is absent.
fn value_of(stated: Option<f64>) -> f64 {
    stated.expect("a value the file states was not read")
}

// The first of `found`, or a failure when it is empty.
fn first_of<T: Clone>(found: &[T]) -> T {
    found
        .first()
        .cloned()
        .expect("no observation of the kind asked for")
}

fn station_named<'a>(project: &'a SurveyProject, id: &str) -> Option<&'a SurveyStation> {
    project
        .stations
        .iter()
        .find(|station| station.setup.id == id)
}

fn confidence_of(bytes: impl AsRef<[u8]>, name: &str, format: &str) -> f64 {
    for probed in format_registry().probe_all(&probe_of(bytes.as_ref(), name, false)) {
        if probed.format_id == format {
            return probed.signature.confidence;
        }
    }
    // not registered
    -1.0
}

// The reduction with every correction off, so a position worked by hand from
// the record text is what it gives.
fn reduced(project: &SurveyProject) -> Result<ReductionOutcome> {
    let settings = ReductionSettings {
        atmospheric: AtmosphericCorrection::None,
        prism_constant_policy: PrismConstantPolicy::None,
        curvature_and_refraction: false,
        ..Default::default()
    };
    reduce_and_adjust(project, &settings, &ReductionContext::default())
}

fn computed<'a>(outcome: &'a ReductionOutcome, id: &str) -> Option<&'a ComputedPoint> {
    outcome.points.iter().find(|point| point.id == id)
}

fn backsight(to: &str, azimuth: &str, circle: &str) -> String {
    format!(
        "07NM{}{}{}{}",
        f16("P1"),
        f16(to),
        f16(azimuth),
        f16(circle)
    )
}

fn shot_at(face: &str, to: &str, circle: &str, zenith: &str) -> String {
    format!(
        "09{face}{}{}{}{}{}",
        f16("P1"),
        f16(to),
        f16("50.0"),
        f16(zenith),
        f16(circle)
    )
}

fn shot(face: &str, to: &str, circle: &str) -> String {
    shot_at(face, to, circle, "90.0")
}

fn setup_on_p1() -> String {
    format!(
        "02NM{}{}{}{}{}",
        f16("P1"),
        f16(""),
        f16(""),
        f16(""),
        f16("1.5")
    )
}

fn target(height: &str) -> String {
    format!("03NM{}", f16(height))
}

// A is at N 2000 E 1000 and B 100 m due north of it, both keyed; a setup on A
// at 1.5 m with a 1.5 m target, and 100 m level shots from A: what the tests
// about the order of backsight records and shots start from.
fn on_a() -> Vec<String> {
    vec![
        header(),
        instrument33(),
        format!(
            "08KI{}{}{}{}{}",
            f16("A"),
            f16("2000.000"),
            f16("1000.000"),
            f16("50.000"),
            f16("")
        ),
        format!(
            "08KI{}{}{}{}{}",
            f16("B"),
            f16("2100.000"),
            f16("1000.000"),
            f16("50.000"),
            f16("")
        ),
        format!(
            "02NM{}{}{}{}{}",
            f16("A"),
            f16(""),
            f16(""),
            f16(""),
            f16("1.500")
        ),
        target("1.500"),
    ]
}

fn from_a_at(face: &str, to: &str, circle: &str, distance: &str) -> String {
    format!(
        "09{face}{}{}{}{}{}",
        f16("A"),
        f16(to),
        f16(distance),
        f16("90.0"),
        f16(circle)
    )
}

fn from_a(face: &str, to: &str, circle: &str) -> String {
    from_a_at(face, to, circle, "100.000")
}

fn backsight_from_a(code: &str, to: &str, azimuth: &str, circle: &str) -> String {
    format!(
        "07{code}{}{}{}{}",
        f16("A"),
        f16(to),
        f16(azimuth),
        f16(circle)
    )
}

fn records_text(records: &[String]) -> String {
    let mut text = String::new();
    for record in records {
        text.push_str(record);
        text.push('\n');
    }
    text
}

// Where the reduction, every correction off, puts `id`: N and E, or a failure
// when it placed nothing there.
fn expect_at(outcome: &ReductionOutcome, id: &str, northing: f64, easting: f64) {
    let point = computed(outcome, id).unwrap_or_else(|| panic!("{id} is not placed"));
    near(point.northing, northing, 1e-6);
    near(point.easting, easting, 1e-6);
}

// An SDR2x job in metres (options 111111: degrees, metres, mmHg, Celsius,
// north first): A at N 1000 E 1000 and B 100 m due north, keyed, a setup on A
// at 0 m with a 0 m target, so every 100 m shot at zenith 90 is 100 m level
// from the mark.
fn sdr2x_on_a() -> Vec<String> {
    vec![
        format!(
            "00NM{}0000{}111111",
            f16("SDR20 V03-05"),
            f16("05-Mar-91 17:30")
        ),
        format!("10NM{}", f16("SETS")),
        format!("08KI0001{}{}{}", f10("1000.0"), f10("1000.0"), f10("100.0")),
        format!("08KI0002{}{}{}", f10("1100.0"), f10("1000.0"), f10("100.0")),
        format!(
            "01NM1{}000000{}00003131{}{}{}",
            field("", 16),
            field("", 16),
            f10(""),
            f10(""),
            f10("0.000")
        ),
        format!(
            "02SC0001{}{}{}{}",
            f10("1000.0"),
            f10("1000.0"),
            f10("100.0"),
            f10("0.0")
        ),
        format!("03NM{}", f10("0.0")),
    ]
}

fn sdr2x_shot(face: &str, to: &str, zenith: &str, circle: &str) -> String {
    format!(
        "09{face}0001{to}{}{}{}",
        f10("100.0"),
        f10(zenith),
        f10(circle)
    )
}

// Every regular file under `folder`, in name order.
fn files_under(folder: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", folder.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files_under(&path, found);
        } else if path.is_file() {
            found.push(path);
        }
    }
}

mod sokkia_sdr {
    use super::*;

    // ---- Registration and detection ------------------------------------------------

    #[test]
    fn is_registered_as_an_importable_sokkia_format_reading_setups_shots_and_points() {
        let format = format_registry().find(ID).expect("registered");
        assert!(format.can_import);
        assert!(!format.can_export);
        assert_eq!(format.manufacturer, katana_surveyio::Manufacturer::Sokkia);
        assert_eq!(format.extensions, ["sdr"]);
        assert!(
            format.reads.points
                && format.reads.observations
                && format.reads.stations
                && format.reads.instrument_settings
                && format.reads.features
        );
        assert!(!format.reads.gnss);
        assert!(!format.reads.coordinate_system);
        assert!(format_registry().reader(ID).is_some());
    }

    #[test]
    fn its_header_identifies_the_file_whatever_it_is_named() {
        let bytes = fixture("sdr/traverse.sdr");
        for name in [
            "traverse.sdr",
            "traverse.txt",
            "traverse.dc",
            "traverse.raw",
            "traverse",
        ] {
            let detection = detect_format(&probe_of(&bytes, name, false));
            assert_eq!(
                detection.outcome(),
                DetectionOutcome::Identified,
                "{name}: {}",
                detection.summary()
            );
            assert_eq!(detection.format().expect("identified").id, ID, "{name}");
            let evidence = &detection.candidates()[0].evidence;
            assert!(evidence.contains("header SDR33 V04-03.00"), "{evidence}");
        }
        // The Survey Controller probe steps aside for an SDR header even in a
        // file named .dc, which a controller can write SDR33 into.
        assert_eq!(confidence_of(&bytes, "traverse.dc", "trimble-dc"), 0.0);
        // No other format claims it at all, whatever it is called.
        for name in [
            "traverse.sdr",
            "traverse.txt",
            "traverse.dc",
            "traverse.raw",
            "traverse.csv",
            "traverse.gsi",
            "traverse.rw5",
            "traverse.fld",
        ] {
            for probed in format_registry().probe_all(&probe_of(&bytes, name, false)) {
                if probed.format_id != ID {
                    assert_eq!(
                        probed.signature.confidence, 0.0,
                        "{name}: {} {}",
                        probed.format_id, probed.signature.evidence
                    );
                }
            }
        }
    }

    #[test]
    fn an_edited_header_named_dc_is_still_sdr_and_not_survey_controller() {
        // [SDR] 3.6.2 gives the header the derivation code ED as well as NM.
        let bytes = records![
            format!(
                "00ED{}0001{}113111",
                f16("SDR33 V04-03.00"),
                f16("28-Sep-26 08:00")
            ),
            format!("10NM{}121111", f16("EDITED")),
        ];
        assert_eq!(confidence_of(&bytes, "job.dc", "trimble-dc"), 0.0);
        let detection = detect_format(&probe_of(bytes.as_bytes(), "job.dc", false));
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        assert_eq!(detection.format().expect("identified").id, ID);
    }

    #[test]
    fn its_probe_rules_out_every_other_formats_fixtures_and_samples() {
        let data = data_directory();
        let mut others = 0usize;
        let sdr_folder = data.join("sdr");
        let mut check = |folder: &Path| {
            let mut files = Vec::new();
            files_under(folder, &mut files);
            for path in files {
                if path.parent() == Some(sdr_folder.as_path()) {
                    continue;
                }
                let bytes = fixture(&path);
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                assert_eq!(
                    confidence_of(&bytes[..bytes.len().min(PROBE_BYTES)], &name, ID),
                    0.0,
                    "{}",
                    path.display()
                );
                others += 1;
            }
        };
        check(&data);
        let tests = data
            .parent()
            .and_then(Path::parent)
            .expect("tests/surveyio/data is two folders down");
        check(&tests.join("qt_widgets").join("survey").join("data"));
        assert!(others > 30);
        for sample in &FOREIGN_SAMPLES {
            assert_eq!(
                confidence_of(sample.bytes, sample.name, ID),
                0.0,
                "{}",
                sample.name
            );
        }
        // Records of the same shape with no SDR header: a Survey Controller job
        // that has lost its header, and another program's raw file.
        assert_eq!(
            confidence_of(
                "10NMJOB1            071118\r\n13TSTrimble Survey Controller\r\n\
                 08KI1       5000.000     5000.000      100.000CP\r\n",
                "job.dc",
                ID
            ),
            0.0
        );
        assert_eq!(
            confidence_of("10NMJOB    121111\n13OOUNITS\n", "job.raw", ID),
            0.0
        );
        // Four digits and blanks are a record's shape too: a headerless point
        // list must not be taken for one.
        assert_eq!(
            confidence_of(
                "1001 5000.000 2000.000 50.000\n1002 5001.000 2001.000 51.000\n",
                "points.txt",
                ID
            ),
            0.0
        );
    }

    #[test]
    fn an_sdr_extension_without_a_header_is_only_a_suggestion() {
        let headerless = records![
            format!("10NM{}121111", f16("NOHEADER")),
            format!(
                "02NM{}{}{}{}{}",
                f16("P1"),
                f16(""),
                f16(""),
                f16(""),
                f16("1.5")
            ),
        ];
        assert!(confidence_of(&headerless, "job.sdr", ID) > 0.0);
        assert!(confidence_of(&headerless, "job.sdr", ID) < IDENTIFIED_CONFIDENCE);
        assert_eq!(confidence_of(&headerless, "job.txt", ID), 0.0);
        assert!(confidence_of("hello\n", "notes.sdr", ID) <= 0.1);
        let error = read_text(headerless).expect_err("refused");
        assert!(error.message.contains("header"));
    }

    #[test]
    fn a_header_with_some_odd_lines_still_identifies_the_file_and_says_how_few_are_records() {
        // No other format writes an SDR header, so 3 odd lines in 18 - a
        // damaged stretch the reader skips line by line - do not leave the
        // file unrecognised. A clean file is surer than this one.
        let mut lines = vec![header(), setup_on_p1()];
        for _ in 0..13 {
            lines.push(target("1.5"));
        }
        for odd in ["garbled line one", "garbled line two", "garbled line three"] {
            lines.push(String::from(odd));
        }
        let text = records_text(&lines);
        let odd = confidence_of(&text, "job.txt", ID);
        assert!(odd >= IDENTIFIED_CONFIDENCE);
        assert!(odd < confidence_of(records![header(), setup_on_p1()], "job.txt", ID));
        let detection = detect_format(&probe_of(text.as_bytes(), "job.txt", false));
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        let evidence = &detection.candidates()[0].evidence;
        assert!(evidence.contains("but only 15 of 18 lines"), "{evidence}");
    }

    #[test]
    fn a_leading_dos_end_of_file_line_is_framing_to_the_probe_as_to_the_reader() {
        // The reader passes over a line of Ctrl-Z; so must detection, or the
        // header behind it is not seen.
        let text = format!(
            "\x1a\r\n{}",
            ended(&records![header(), setup_on_p1()], "\r\n")
        );
        let detection = detect_format(&probe_of(text.as_bytes(), "job.sdr", false));
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        assert_eq!(detection.format().expect("identified").id, ID);
        let result = ok(read_text(text));
        assert_eq!(result.project.source.format, "SDR33");
        assert_eq!(result.records_skipped, 0);
    }

    #[test]
    fn an_stx_glued_to_the_header_is_framing_and_the_header_is_read() {
        // [SDR] chapter 3 opens a transmission with STX CR LF; a writer that
        // leaves out the line end glues the header to it.
        let text = format!("\x02{}", records![header(), setup_on_p1()]);
        let detection = detect_format(&probe_of(text.as_bytes(), "job.sdr", false));
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        assert_eq!(detection.format().expect("identified").id, ID);
        let result = ok(read_text(text));
        assert_eq!(result.records_read, 2);
        assert_eq!(result.records_skipped, 0);
        assert_eq!(result.project.source.format_version, "SDR33 V04-03.00");
    }

    // ---- The header ---------------------------------------------------------------

    #[test]
    fn a_short_or_stray_header_record_is_an_answer_and_not_a_crash() {
        // A second "00" record two characters long once its blanks are gone,
        // three, or naming no version: the header reader must not cut past its
        // end. Each is skipped, naming the file's own header, and the file
        // reads on - one line must not cost the rest.
        for stray in ["00  ", "00A ", "00NM"] {
            let result = ok(read_text(records![
                header(),
                stray,
                setup_on_p1(),
                target("1.5")
            ]));
            assert_eq!(result.records_skipped, 1, "{stray}");
            assert_eq!(result.records_read, 3, "{stray}");
            assert!(warned(&result, 2, "follows the file's own (record 1)"));
            assert_eq!(result.project.stations.len(), 1);
        }
        // As the first line, read as SDR because it was asked to be: refused in
        // a sentence.
        for first in ["00  ", "00A ", "00NM"] {
            let error = read_text(records![first, setup_on_p1()]).expect_err("refused");
            assert_eq!(error.code, ErrorCode::FileImportFailure);
            assert!(
                error.message.contains("names no version"),
                "{first}: {}",
                error.message
            );
        }
    }

    #[test]
    fn a_second_header_of_a_version_this_reader_does_not_read_is_skipped() {
        let result = ok(read_text(records![
            header(),
            setup_on_p1(),
            format!(
                "00NM{}0001{}113111",
                f16("SDR44 V01-00"),
                f16("28-Sep-26 08:00")
            ),
        ]));
        assert_eq!(result.records_skipped, 1);
        assert!(warned(
            &result,
            3,
            "a header record that is not one this reader can read follows the file's own \
             (record 1)"
        ));
    }

    #[test]
    fn a_header_ending_in_white_space_reads_its_options() {
        for padding in ["\t", "  ", " \t "] {
            let result = ok(read_text(records![
                header33("123111") + padding,
                format!(
                    "02NM{}{}{}{}{}",
                    f16("P1"),
                    f16(""),
                    f16(""),
                    f16(""),
                    f16("10.0")
                ),
            ]));
            assert_eq!(result.warnings.len(), 0, "{padding:?}");
            // Option 2 of 42: feet, so 10 is 3.048 m.
            near(
                setup_at(&result.project, 0).setup.instrument_height,
                3.048,
                LENGTH_TOLERANCE,
            );
        }
    }

    #[test]
    fn the_version_is_read_one_way_for_the_metadata_and_the_provenance() {
        // Blanks between the name and the version, as some writers pad them:
        // one reading, "SDR33 V04-04", in both places.
        let result = ok(read_text(records![
            format!(
                "00NM{}0042{}113111",
                f16("SDR33    V04-04"),
                f16("01-Feb-26 07:15")
            ),
            setup_on_p1(),
        ]));
        assert_eq!(result.project.metadata["header: version"], "SDR33 V04-04");
        assert_eq!(result.project.source.format_version, "SDR33 V04-04");
        assert_eq!(result.project.metadata["header: serial number"], "0042");
    }

    #[test]
    fn a_header_version_other_than_sdr33_or_sdr2x_is_refused() {
        let error = read_text(records![
            format!(
                "00NM{}0001{}113111",
                f16("SDR44 V01-00"),
                f16("28-Sep-26 08:00")
            ),
            setup_on_p1(),
        ])
        .expect_err("refused");
        assert_eq!(error.code, ErrorCode::FileImportFailure);
        assert!(
            error.message.contains("names the version 'SDR44 V01-00'"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_second_header_with_the_same_units_reads_on_and_one_with_others_is_refused() {
        // [SDR] chapter 3: each transmission opens with a header, so two
        // joined files hold two. The same units: read on. Others: every number
        // after the second would mean something else.
        let same = ok(read_text(records![
            header(),
            setup_on_p1(),
            header(),
            format!(
                "02NM{}{}{}{}{}",
                f16("P2"),
                f16(""),
                f16(""),
                f16(""),
                f16("1.6")
            ),
        ]));
        assert_eq!(same.records_read, 4);
        assert_eq!(same.project.stations.len(), 2);
        let other = read_text(records![
            header33("113111"),
            setup_on_p1(),
            header33("123111")
        ])
        .expect_err("refused");
        assert!(other.message.contains("record 3 "), "{}", other.message);
        assert!(
            other
                .message
                .contains("a second header record states other units")
        );
    }

    #[test]
    fn a_header_of_another_length_is_read_from_its_last_six_characters_and_says_so() {
        // 44 characters: the date takes 21-38, the options 39-44 ("123111":
        // degrees and feet).
        let header = format!(
            "00NM{}{}123111",
            f16("SDR33 V04-03.00"),
            field("28-Sep-26 08:00", 18)
        );
        assert_eq!(header.len(), 44);
        let result = ok(read_text(records![
            header,
            format!(
                "02NM{}{}{}{}{}",
                f16("P1"),
                f16(""),
                f16(""),
                f16(""),
                f16("10.0")
            ),
        ]));
        assert!(warned(
            &result,
            1,
            "the header record is 44 characters where the format's is 46"
        ));
        near(
            setup_at(&result.project, 0).setup.instrument_height,
            3.048,
            LENGTH_TOLERANCE,
        );
    }

    #[test]
    fn angle_option_four_is_mils_in_the_nikon_header_form_and_degrees_otherwise_each_said() {
        // [SDR] 3.5 defines 1 to 3. The Nikon and Spectra manuals give 4 as mils
        // in headers written "SDR33V04-01"; Trimble's writer gives 4 as
        // quadrant bearings, which Sokkia's manual says are degrees underneath.
        // A shot at 90 on the circle: pi/2 in degrees, 90 * pi / 3200 in mils.
        let cases: [(String, f64, AngularUnit, &str); 2] = [
            (
                header33("413111"),
                PI / 2.0,
                AngularUnit::DecimalDegrees,
                "so the angles are read in degrees",
            ),
            (
                header_nikon("413111"),
                90.0 * PI / 3200.0,
                AngularUnit::Mils,
                "so the angles are read in mils",
            ),
        ];
        for (header, circle, unit, words) in cases {
            let result = ok(read_text(records![
                header,
                instrument33(),
                setup_on_p1(),
                target("1.5"),
                shot("F1", "P2", "90.0"),
            ]));
            assert!(warned(&result, 1, "angle unit option is 4"), "{header}");
            assert!(warned(&result, 1, words), "{header}");
            assert_eq!(result.project.units.angular, unit);
            let direction = first_of(&observations_to::<HorizontalDirectionObservation>(
                setup_at(&result.project, 0),
                "P2",
            ));
            near(direction.direction, circle, ANGLE_TOLERANCE);
        }
    }

    #[test]
    fn coordinates_are_read_east_first_under_options_two_and_three_each_with_its_own_warning() {
        // [SDR] 3.5 defines option 2 as "E-N-Elev", and Trimble's published
        // writer puts the easting first under it; but 3.6.2 names the first
        // field the northing, and Sokkia's own E-N-Elev example (chapter 2,
        // V04-04.30) is a printed report ([SDR] 2.4), which shows the display
        // order and not the file's. So the easting is read first, and said
        // once. Trimble's 3, "Y-X-Z", is east first too, but the format does
        // not define it and the field book's third display order is
        // south-west-elevation: its own warning. Each once, at the first record
        // that states coordinates, however many follow.
        let keyed = |id: &str, first: &str, second: &str| {
            format!(
                "08KI{}{}{}{}{}",
                f16(id),
                f16(first),
                f16(second),
                f16(""),
                f16("")
            )
        };
        let cases = [
            (
                "113121",
                "east, north, elevation (option 2)",
                "coordinate order option is 2, east-north-elevation, and the coordinates are read \
                 easting first",
            ),
            (
                "113131",
                "east, north, elevation (option 3)",
                "coordinate order option is 3, which the format's publisher does not define",
            ),
        ];
        for (options, order, words) in cases {
            let east = ok(read_text(records![
                header33(options),
                keyed("A", "500000.000", "5000000.000"),
                keyed("B", "500100.000", "5000000.000"),
            ]));
            let a = point(&east.project, "A").expect("A");
            double_eq(a.easting, 500000.0);
            double_eq(a.northing, 5000000.0);
            let b = point(&east.project, "B").expect("B");
            double_eq(b.easting, 500100.0);
            assert_eq!(east.warnings.len(), 1, "{options}");
            assert!(warned(&east, 2, words), "{options}");
            assert!(warned(&east, 2, "check a known point"), "{options}");
            assert_eq!(east.project.metadata["header: coordinate order"], order);
        }
        let three = ok(read_text(records![
            header33("113131"),
            keyed("A", "500000.000", "5000000.000"),
            keyed("B", "500100.000", "5000000.000"),
        ]));
        assert!(warned(&three, 2, "south-west-elevation"));
        // A file under option 2 that states no coordinates has nothing to warn
        // about.
        let none = ok(read_text(records![header33("113121"), setup_on_p1()]));
        assert_eq!(none.warnings.len(), 0);
        // Option 1 is the format's first order, north first, and needs no word.
        let north = ok(read_text(records![
            header33("113111"),
            keyed("A", "5000000.000", "500000.000")
        ]));
        assert_eq!(north.warnings.len(), 0);
        assert_eq!(
            north.project.metadata["header: coordinate order"],
            "north, east, elevation (option 1)"
        );
    }

    #[test]
    fn an_undefined_angle_or_distance_unit_is_refused_naming_the_header_and_the_option() {
        // [SDR] 3.5: angles 1 to 3, distances 1 and 2 (and Trimble's 3); a
        // header with five options or none cannot say its units at all.
        let cases = [
            ("513111", "the header's angle unit option is '5'"),
            ("143111", "the header's distance unit option is '4'"),
            ("11311", "does not end in its six unit and order options"),
            ("", "does not end in its six unit and order options"),
        ];
        for (options, reason) in cases {
            let error = read_text(records![header33(options), format!("10NM{}", f16("J"))])
                .expect_err("refused");
            assert_eq!(error.code, ErrorCode::FileImportFailure);
            assert!(error.message.contains("record 1 "), "{}", error.message);
            assert!(error.message.contains(reason), "{}", error.message);
        }
    }

    #[test]
    fn coordinates_under_an_undefined_order_are_refused_but_a_file_without_them_reads() {
        let station = setup_on_p1();
        let refused = read_text(records![
            header33("113151"),
            station,
            format!(
                "08KI{}{}{}{}{}",
                f16("A"),
                f16("1.0"),
                f16("2.0"),
                f16(""),
                f16("")
            ),
        ])
        .expect_err("refused");
        assert!(refused.message.contains("record 3 "));
        assert!(refused.message.contains("coordinate order option is '5'"));
        let reads = ok(read_text(records![header33("113151"), station]));
        assert_eq!(reads.project.stations.len(), 1);
    }

    #[test]
    fn an_undocumented_angles_left_option_is_refused() {
        let error = read_text(records![header33("113112"), format!("10NM{}", f16("J"))])
            .expect_err("refused");
        assert!(error.message.contains("angles left/right option is '2'"));
    }

    #[test]
    fn the_forty_two_character_header_of_a_published_writer_reads_its_options() {
        // Trimble's published SDR33 writer: no serial number, so the date is in
        // 21-36 and the options in 37-42; here feet and Fahrenheit.
        let header = format!(
            "00NM{}{}123211",
            f16("SDR33 V04-04.25"),
            f16("28-Sep-26 08:00")
        );
        assert_eq!(header.len(), 42);
        let result = ok(read_text(records![
            header,
            format!(
                "02NM{}{}{}{}{}",
                f16("P1"),
                f16(""),
                f16(""),
                f16(""),
                f16("10.0")
            ),
        ]));
        assert_eq!(result.project.units.linear, LinearUnit::Feet);
        near(
            setup_at(&result.project, 0).setup.instrument_height,
            3.048,
            LENGTH_TOLERANCE,
        );
        assert_eq!(
            result.project.metadata["header: date and time"],
            "28-Sep-26 08:00"
        );
        assert_eq!(result.warnings.len(), 0);
    }

    // ---- The traverse fixture -------------------------------------------------------

    #[test]
    fn the_header_gives_the_units_and_the_job_its_name_and_switches() {
        let result = read_traverse();
        let project = &result.project;
        assert_eq!(project.units.linear, LinearUnit::Metres);
        assert_eq!(project.units.angular, AngularUnit::DecimalDegrees);
        assert_eq!(project.name, "FIXTURE TRAV");
        assert_eq!(project.metadata["header: version"], "SDR33 V04-03.00");
        assert_eq!(project.metadata["header: serial number"], "0001");
        assert_eq!(project.metadata["header: date and time"], "28-Sep-26 08:00");
        // Job flags 122211: elevations recorded, atmospheric correction on,
        // curvature and refraction on (0.14), sea level off.
        assert_eq!(project.metadata["job: atmospheric correction"], "on");
        assert_eq!(
            project.metadata["job: curvature and refraction correction"],
            "on"
        );
        assert_eq!(project.metadata["job: sea level correction"], "off");
        assert_eq!(project.source.manufacturer, "Sokkia");
        assert_eq!(project.source.format, "SDR33");
        assert_eq!(project.source.format_version, "SDR33 V04-03.00");
        assert!(project.coordinate_system.epsg_code <= 0);
    }

    #[test]
    fn every_live_record_is_read_and_every_deleted_one_skipped() {
        // 51 lines: 4 blank, 7 deleted (33 to 39), 40 records read, and a
        // warning for each deleted one, and one more: the header's coordinate
        // order option 2 ([SDR] 3.5's E-N-Elev) is read east first, which no
        // Sokkia file shows, said once at the first coordinates (record 8) and
        // in the metadata.
        let result = read_traverse();
        assert_eq!(result.records_read, 40);
        assert_eq!(result.records_skipped, 7);
        assert_eq!(result.warnings.len(), 8);
        for record in 33..=39 {
            assert!(warned(&result, record, "deleted record"), "{record}");
        }
        assert!(warned(&result, 8, "coordinate order option is 2"));
        assert!(warned(&result, 35, "02NM (station)"));
        assert!(warned(&result, 39, "'DDDD'"));
        assert_eq!(
            result.project.metadata["header: coordinate order"],
            "east, north, elevation (option 2)"
        );
    }

    #[test]
    fn keyed_control_is_read_easting_first_under_option_two() {
        let result = read_traverse();
        let cp1 = point(&result.project, "CP1").expect("CP1");
        // Record 8: "500000.000" in 21-36, "5000000.000" in 37-52, option 2.
        double_eq(cp1.easting, 500000.0);
        double_eq(cp1.northing, 5000000.0);
        double_eq(cp1.elevation.expect("an elevation"), 100.0);
        assert_eq!(cp1.coordinate_source, CoordinateSource::Entered);
        assert_eq!(cp1.source.record_number, 8);
        let cp2 = point(&result.project, "CP2").expect("CP2");
        double_eq(cp2.northing, 5000100.0);
        double_eq(cp2.easting, 500000.0);
        assert_eq!(result.project.points.len(), 2);
        // The traverse marks are named, not placed.
        assert!(unpositioned(&result.project, "T1").is_some());
        assert!(unpositioned(&result.project, "T2").is_some());
        assert_eq!(result.project.unpositioned_points.len(), 2);
    }

    #[test]
    fn each_station_record_is_one_setup_with_its_rounds_together() {
        let result = read_traverse();
        let project = &result.project;
        assert_eq!(project.stations.len(), 2);
        let first = setup_at(project, 0);
        assert_eq!(first.setup.id, "CP1");
        assert_eq!(first.setup.point_id, "CP1");
        double_eq(first.setup.instrument_height, 1.5);
        assert_eq!(first.source.record_number, 12);
        // Two rounds of four pointings, each of a direction, a zenith and a
        // slope distance.
        assert_eq!(first.observations.len(), 24);
        assert_eq!(first.metadata["backsight rounds"], "2");
        let second = setup_at(project, 1);
        assert_eq!(second.setup.id, "T1");
        double_eq(second.setup.instrument_height, 1.55);
        assert_eq!(second.observations.len(), 12);
        assert_eq!(second.metadata["backsight rounds"], "1");
    }

    #[test]
    fn the_backsight_records_azimuth_and_circle_reading_each_have_their_field() {
        let result = read_traverse();
        // Records 14 and 23: azimuth 0, circle 0 on CP2, in both rounds.
        let first = setup_at(&result.project, 0);
        assert_eq!(first.backsight_point_id, "CP2");
        near(value_of(first.backsight_azimuth), 0.0, ANGLE_TOLERANCE);
        near(
            value_of(first.stated_backsight_azimuth),
            0.0,
            ANGLE_TOLERANCE,
        );
        assert_eq!(
            first.metadata["backsight circle readings (radians)"],
            "0, 0"
        );
        // Record 45: T1's circle zeroed on CP1, whose azimuth is 270 - the
        // circle is the model's backsight_azimuth, the azimuth its own field.
        let second = setup_at(&result.project, 1);
        assert_eq!(second.backsight_point_id, "CP1");
        near(value_of(second.backsight_azimuth), 0.0, ANGLE_TOLERANCE);
        near(
            value_of(second.stated_backsight_azimuth),
            degrees(270.0),
            ANGLE_TOLERANCE,
        );
    }

    #[test]
    fn face_one_and_face_two_are_pointings_of_one_target_read_as_distance_zenith_circle() {
        let result = read_traverse();
        let first = setup_at(&result.project, 0);
        // Records 18, 19, 27, 28: F1 150.000 87 90, F2 150.002 273 270, F1
        // 150.001 87 90, F2 150.003 273 270. The face 2 zenith 273 is the line
        // 360 - 273 = 87 on face 1; its circle reading stays as read.
        let directions = observations_to::<HorizontalDirectionObservation>(first, "T1");
        let zeniths = observations_to::<ZenithAngleObservation>(first, "T1");
        let distances = observations_to::<DistanceObservation>(first, "T1");
        assert_eq!(directions.len(), 4);
        assert_eq!(zeniths.len(), 4);
        assert_eq!(distances.len(), 4);
        let circle = [90.0, 270.0, 90.0, 270.0];
        let slope = [150.000, 150.002, 150.001, 150.003];
        let face = [Face::Left, Face::Right, Face::Left, Face::Right];
        let record = [18, 19, 27, 28];
        for (i, direction) in directions.iter().enumerate() {
            near(direction.direction, degrees(circle[i]), ANGLE_TOLERANCE);
            near(zeniths[i].angle, degrees(87.0), ANGLE_TOLERANCE);
            near(distances[i].distance, slope[i], LENGTH_TOLERANCE);
            assert_eq!(distances[i].kind, DistanceKind::Slope);
            assert_eq!(direction.pointing.face, face[i], "{i}");
            // One record is one pointing: its three observations share it.
            assert_eq!(direction.pointing, zeniths[i].pointing);
            assert_eq!(direction.pointing, distances[i].pointing);
            assert_ne!(direction.pointing.index, 0);
            assert_eq!(direction.source.record_number, record[i]);
            double_eq(distances[i].instrument_height, 1.5);
        }
        assert_ne!(directions[0].pointing.index, directions[1].pointing.index);
        // Every observation stated a precision the model accepts.
        for observation in &first.observations {
            assert!(validate_observation(observation).is_ok());
        }
    }

    #[test]
    fn a_target_height_holds_until_the_next_and_a_deleted_one_sets_nothing() {
        let result = read_traverse();
        let first = setup_at(&result.project, 0);
        for zenith in observations_to::<ZenithAngleObservation>(first, "CP2") {
            // records 15, 20, 24, 29
            double_eq(zenith.target_height, 1.6);
        }
        for distance in observations_to::<DistanceObservation>(first, "T1") {
            // records 17, 26
            double_eq(distance.target_height, 1.65);
        }
        // Record 46, the first shot of the second setup, has no 03 before it in
        // its setup: the last live one (record 29, 1.600) holds, and the
        // deleted DD03NM of record 37 (9.999) does not.
        let second = setup_at(&result.project, 1);
        let to_cp1 = observations_to::<DistanceObservation>(second, "CP1");
        assert_eq!(to_cp1.len(), 2);
        assert_eq!(to_cp1[0].source.record_number, 46);
        double_eq(to_cp1[0].target_height, 1.6);
        double_eq(to_cp1[0].instrument_height, 1.55);
        for distance in observations_to::<DistanceObservation>(second, "T2") {
            // record 47
            double_eq(distance.target_height, 1.7);
        }
    }

    #[test]
    fn deleted_records_change_nothing_that_follows() {
        let result = read_traverse();
        let project = &result.project;
        // The deleted setup on T9 is neither a setup nor a point.
        assert!(station_named(project, "T9").is_none());
        assert!(point(project, "T9").is_none());
        assert!(unpositioned(project, "T9").is_none());
        // Its weather (1000 mbar, 35 C) reached no setup: the second has its
        // own, record 42's.
        let second = setup_at(project, 1);
        double_eq(value_of(second.instrument.pressure_hectopascals), 1012.8);
        double_eq(value_of(second.instrument.temperature_celsius), 21.5);
    }

    #[test]
    fn instrument_weather_and_scale_settle_on_the_setup_they_are_written_before() {
        let result = read_traverse();
        let first = setup_at(&result.project, 0);
        let instrument = &first.instrument;
        assert_eq!(instrument.model, "TEST THEODOLITE");
        assert_eq!(instrument.serial_number, "004711");
        // Record 7's prism constant, -30 mm ([SDR] 3.3.4), in the distances.
        double_eq(value_of(instrument.prism_constant), -0.030);
        assert_eq!(instrument.prism_constant_state, CorrectionState::Applied);
        // Record 11: 1013.25 mbar, which is hPa, and 20 C.
        double_eq(value_of(instrument.pressure_hectopascals), 1013.25);
        double_eq(value_of(instrument.temperature_celsius), 20.0);
        assert_eq!(instrument.atmospheric_ppm_state, CorrectionState::Unknown);
        double_eq(value_of(instrument.scale_factor), 1.0);
        assert_eq!(instrument.scale_factor_state, CorrectionState::NotApplied);
        double_eq(value_of(instrument.refraction_coefficient), 0.14);
        assert_eq!(
            instrument.curvature_refraction_state,
            CorrectionState::NotApplied
        );
        // Records 41 and 42 come after the first setup's shots and before the
        // second's 02: they are the second's, and no "changed in the middle"
        // warning is due.
        assert!(!warned(&result, 41, "middle"));
        assert!(!warned(&result, 42, "middle"));
        assert!(any_not_carried_contains(&result, "atmospheric correction"));
    }

    #[test]
    fn time_stamps_date_their_setup_and_notes_are_kept() {
        let result = read_traverse();
        let project = &result.project;
        // Records 4 and 5, before any setup.
        let notes = &project.metadata["notes"];
        assert!(
            notes.contains("TS: Time Date 09/28/2026 Time 08:05:00"),
            "{notes}"
        );
        assert!(notes.contains("Written by hand from the published SDR33 layouts"));
        // Record 31 dates the first setup, record 44 the second; the deleted
        // one (record 39) dates nothing.
        assert_eq!(
            setup_at(project, 0).instrument.time,
            SurveyTimestamp {
                year: 2026,
                month: 9,
                day: 28,
                hour: 8,
                minute: 31,
                second: 10.0,
                time_system: String::new(),
            }
        );
        assert_eq!(
            setup_at(project, 1).instrument.time,
            SurveyTimestamp {
                year: 2026,
                month: 9,
                day: 28,
                hour: 8,
                minute: 52,
                second: 40.0,
                time_system: String::new(),
            }
        );
        assert!(setup_at(project, 1).metadata["notes"].contains("08:52:40"));
    }

    #[test]
    fn a_keyed_azimuth_is_kept_with_its_setup() {
        // Record 13: 11TP CP1 to CP2, azimuth 0, no distances. The setup's 07
        // orients it, so the keyed azimuth is kept, not used.
        let result = read_traverse();
        let first = setup_at(&result.project, 0);
        assert_eq!(first.metadata["azimuth to CP2 (radians)"], "0");
        assert_eq!(first.metadata["azimuth to CP2, description"], "BS AZ");
        assert!(!first.metadata.contains_key("oriented by"));
    }

    #[test]
    fn coded_points_are_strung_by_their_code() {
        let result = read_traverse();
        let control = feature(&result.project, "CTRL").expect("CTRL");
        assert_eq!(control.point_ids, ["CP1", "CP2"]);
        let traverse = feature(&result.project, "TRAV").expect("TRAV");
        assert_eq!(traverse.point_ids, ["T1", "T2"]);
        assert_eq!(result.project.features.len(), 2);
    }

    #[test]
    fn the_traverse_reduces_to_the_positions_worked_by_hand() {
        // From CP1 (500000 E, 5000000 N) oriented on CP2, due north at circle
        // 0, T1 at circle 90 and zenith 87, its four distances meaning
        // 150.0015: 150.0015 sin 87 = 149.795928 east. From T1 oriented on CP1
        // (azimuth 270 from the coordinates, circle 0 on it), T2 at circle 90 is
        // azimuth 360, due north, at zenith 91 and 80.001: 80.001 sin 91 =
        // 79.988815 north. With no curvature and refraction, those exactly.
        let result = read_traverse();
        let outcome = ok(reduced(&result.project));
        let t1 = computed(&outcome, "T1").expect("T1");
        near(t1.easting, 500000.0 + 150.0015 * degrees(87.0).sin(), 1e-6);
        near(t1.northing, 5000000.0, 1e-6);
        let t2 = computed(&outcome, "T2").expect("T2");
        near(t2.easting, t1.easting, 1e-6);
        near(t2.northing, 5000000.0 + 80.001 * degrees(91.0).sin(), 1e-6);
    }

    // ---- Layouts and units --------------------------------------------------------------

    #[test]
    fn the_sdr2x_layout_reads_four_character_ids_and_ten_character_reals() {
        // [SDR] 3.6.1: the same records with a 4-digit point number and
        // 10-character reals. 02: 5-8 point, 9-18 N, 19-28 E, 29-38 Z, 39-48
        // instrument height; 09: 5-8, 9-12, 13-22 SD, 23-32 VA, 33-42 HA.
        let text = records![
            format!(
                "00NM{}0000{}113111",
                f16("SDR20 V03-05"),
                f16("18-Jan-80 20:34")
            ),
            format!("10NM{}", f16("SMALL")),
            format!(
                "01NM1{}000000{}00003131{}{}{}",
                field("", 16),
                f16("DT2"),
                f10(""),
                f10(""),
                f10("0.000")
            ),
            format!(
                "02KI0001{}{}{}{}{}",
                f10("5100.000"),
                f10("200.000"),
                f10("50.000"),
                f10("1.000"),
                f16("DEAD DOG")
            ),
            format!("07TP00010002{}{}", f10("14.00000"), f10("0.00000")),
            format!("03NM{}", f10("1.200")),
            format!(
                "09F100010002{}{}{}{}",
                f10("42.500"),
                f10("91.50000"),
                f10("0.00000"),
                f16("BS")
            ),
            format!(
                "09F200010002{}{}{}{}",
                f10("42.502"),
                f10("268.50000"),
                f10("180.00000"),
                f16("BS")
            ),
        ];
        let result = ok(read_text(text));
        assert_eq!(result.project.source.format, "SDR2x");
        assert_eq!(result.records_skipped, 0);
        let station = point(&result.project, "0001").expect("0001");
        // option 1: north first
        double_eq(station.northing, 5100.0);
        double_eq(station.easting, 200.0);
        // 02KI
        assert_eq!(station.coordinate_source, CoordinateSource::Entered);
        assert_eq!(result.project.stations.len(), 1);
        let setup = setup_at(&result.project, 0);
        double_eq(setup.setup.instrument_height, 1.0);
        assert_eq!(setup.backsight_point_id, "0002");
        // [SDR] chapter 4's sample: the azimuth 14, the circle zeroed on 0002.
        near(
            value_of(setup.stated_backsight_azimuth),
            degrees(14.0),
            ANGLE_TOLERANCE,
        );
        near(value_of(setup.backsight_azimuth), 0.0, ANGLE_TOLERANCE);
        let distances = observations_to::<DistanceObservation>(setup, "0002");
        assert_eq!(distances.len(), 2);
        near(distances[0].distance, 42.500, LENGTH_TOLERANCE);
        near(distances[1].distance, 42.502, LENGTH_TOLERANCE);
        double_eq(distances[0].target_height, 1.2);
        let zeniths = observations_to::<ZenithAngleObservation>(setup, "0002");
        assert_eq!(zeniths.len(), 2);
        // 360 - 268.5
        near(zeniths[1].angle, degrees(91.5), ANGLE_TOLERANCE);
    }

    #[test]
    fn option_one_reads_the_northing_first() {
        let result = ok(read_text(records![
            header33("113111"),
            format!(
                "08KI{}{}{}{}{}",
                f16("A"),
                f16("5000000.000"),
                f16("500000.000"),
                f16("10.000"),
                f16("")
            ),
        ]));
        let a = point(&result.project, "A").expect("A");
        double_eq(a.northing, 5000000.0);
        double_eq(a.easting, 500000.0);
    }

    #[test]
    fn gons_and_mils_are_turned_to_radians() {
        // 100 gons and 1600 mils are each a quarter circle ([SDR] 3.3.1: 400
        // gons, 6400 mils to the circle).
        for (options, circle) in [("213111", "100.0"), ("313111", "1600.0")] {
            let result = ok(read_text(records![
                header33(options),
                instrument33(),
                setup_on_p1(),
                target("1.5"),
                format!(
                    "09F1{}{}{}{}{}",
                    f16("P1"),
                    f16("P2"),
                    f16("10.0"),
                    f16(circle),
                    f16(circle)
                ),
            ]));
            let directions = observations_to::<HorizontalDirectionObservation>(
                setup_at(&result.project, 0),
                "P2",
            );
            assert_eq!(directions.len(), 1, "{options}");
            near(directions[0].direction, PI / 2.0, ANGLE_TOLERANCE);
            let zeniths =
                observations_to::<ZenithAngleObservation>(setup_at(&result.project, 0), "P2");
            assert_eq!(zeniths.len(), 1, "{options}");
            near(zeniths[0].angle, PI / 2.0, ANGLE_TOLERANCE);
            assert_eq!(
                result.project.units.angular,
                if options.starts_with('2') {
                    AngularUnit::Gons
                } else {
                    AngularUnit::Mils
                }
            );
        }
    }

    #[test]
    fn feet_and_us_survey_feet_are_turned_to_metres() {
        let read_shot = |options: &str, unit_note: &str| {
            let mut lines = vec![header33(options)];
            if !unit_note.is_empty() {
                lines.push(format!("13DU{unit_note}"));
            }
            lines.push(instrument33());
            lines.push(format!(
                "02NM{}{}{}{}{}",
                f16("P1"),
                f16(""),
                f16(""),
                f16(""),
                f16("5.0")
            ));
            lines.push(target("5.0"));
            lines.push(format!(
                "09F1{}{}{}{}{}",
                f16("P1"),
                f16("P2"),
                f16("100.0"),
                f16("90.0"),
                f16("0.0")
            ));
            read_text(records_text(&lines))
        };
        // International foot 0.3048 m exactly; US survey foot 1200/3937 m.
        let feet = ok(read_shot("123111", ""));
        assert_eq!(feet.project.units.linear, LinearUnit::Feet);
        let setup = setup_at(&feet.project, 0);
        near(setup.setup.instrument_height, 1.524, LENGTH_TOLERANCE);
        let shot_in_feet = first_of(&observations_to::<DistanceObservation>(setup, "P2"));
        near(shot_in_feet.distance, 30.48, LENGTH_TOLERANCE);
        near(shot_in_feet.target_height, 1.524, LENGTH_TOLERANCE);
        // [SDR] 3.3.2: 13DU3 narrows the header's feet to US survey feet.
        let us_feet = ok(read_shot("123111", "3:US Feet:"));
        assert_eq!(us_feet.project.units.linear, LinearUnit::UsSurveyFeet);
        let shot_in_us_feet = first_of(&observations_to::<DistanceObservation>(
            setup_at(&us_feet.project, 0),
            "P2",
        ));
        near(
            shot_in_us_feet.distance,
            100.0 * 1200.0 / 3937.0,
            LENGTH_TOLERANCE,
        );
        // A published writer puts 3 in the header itself.
        let three = ok(read_shot("133111", ""));
        assert_eq!(three.project.units.linear, LinearUnit::UsSurveyFeet);
        // A note with no code, or one the format does not define, says nothing
        // the header has not: skipped, and the header's feet hold.
        for note in ["", "9:Parsecs:"] {
            let odd = ok(read_shot(
                "123111",
                if note.is_empty() { " " } else { note },
            ));
            assert_eq!(odd.records_skipped, 1, "{note}");
            assert!(warned(&odd, 2, "the header's unit, feet, holds"), "{note}");
            near(
                setup_at(&odd.project, 0).setup.instrument_height,
                1.524,
                LENGTH_TOLERANCE,
            );
        }
    }

    #[test]
    fn pressure_and_temperature_units_are_converted() {
        let weather = |options: &str, pressure: &str, temperature: &str| {
            read_text(records![
                header33(options),
                format!("05NM{}{}", f16(pressure), f16(temperature)),
                setup_on_p1(),
            ])
        };
        // The conventional millimetre of mercury is 13595.1 kg/m3 x 9.80665
        // m/s2 x 1 mm; an inch of mercury 25.4 of them. 68 F is 20 C exactly.
        // Options 3 and 4 are the pressure and temperature units.
        let mm_hg = ok(weather("111211", "760.0", "68.0"));
        near(
            value_of(setup_at(&mm_hg.project, 0).instrument.pressure_hectopascals),
            13595.1 * 9.80665 * 0.760 / 100.0,
            1e-9,
        );
        near(
            value_of(setup_at(&mm_hg.project, 0).instrument.temperature_celsius),
            20.0,
            1e-12,
        );
        let in_hg = ok(weather("112111", "29.92", "68.0"));
        near(
            value_of(setup_at(&in_hg.project, 0).instrument.pressure_hectopascals),
            13595.1 * 9.80665 * (29.92 * 0.0254) / 100.0,
            1e-9,
        );
        near(
            value_of(setup_at(&in_hg.project, 0).instrument.temperature_celsius),
            68.0,
            1e-12,
        );
        let fahrenheit = ok(weather("113211", "1000.0", "68.0"));
        let in_fahrenheit = &setup_at(&fahrenheit.project, 0).instrument;
        near(value_of(in_fahrenheit.temperature_celsius), 20.0, 1e-12);
        double_eq(value_of(in_fahrenheit.pressure_hectopascals), 1000.0);
        // An undefined pressure or temperature unit: said once, and that value
        // not read.
        let no_pressure = ok(weather("119111", "1000.0", "20.0"));
        assert!(warned(&no_pressure, 1, "pressure unit option is '9'"));
        assert!(
            setup_at(&no_pressure.project, 0)
                .instrument
                .pressure_hectopascals
                .is_none()
        );
        double_eq(
            value_of(
                setup_at(&no_pressure.project, 0)
                    .instrument
                    .temperature_celsius,
            ),
            20.0,
        );
        let no_temperature = ok(weather("113711", "1000.0", "20.0"));
        assert!(warned(&no_temperature, 1, "temperature unit option is '7'"));
        assert!(
            setup_at(&no_temperature.project, 0)
                .instrument
                .temperature_celsius
                .is_none()
        );
        double_eq(
            value_of(
                setup_at(&no_temperature.project, 0)
                    .instrument
                    .pressure_hectopascals,
            ),
            1000.0,
        );
    }

    #[test]
    fn a_distance_unit_note_that_disagrees_with_the_header_governs_the_distances_after_it() {
        // [SDR] 3.3.2: "All distances specified after this Note record are
        // displayed in the specified distance units." Right after a metres
        // header, US feet: the instrument height 10 is 10 x 1200/3937 m.
        let early = ok(read_text(records![
            header33("113111"),
            "13DU3:US Feet:",
            format!(
                "02NM{}{}{}{}{}",
                f16("P1"),
                f16(""),
                f16(""),
                f16(""),
                f16("10.0")
            ),
        ]));
        assert!(warned(
            &early,
            2,
            "the distance unit note says US survey feet where the header (record 1) says \
             metres"
        ));
        assert_eq!(early.project.units.linear, LinearUnit::UsSurveyFeet);
        near(
            setup_at(&early.project, 0).setup.instrument_height,
            10.0 * 1200.0 / 3937.0,
            LENGTH_TOLERANCE,
        );
        // Later in a feet file, metres from the note on: the height before it
        // stays in feet (10 ft = 3.048 m), the target height and distance after
        // it are metres.
        let late = ok(read_text(records![
            header33("123111"),
            format!(
                "02NM{}{}{}{}{}",
                f16("P1"),
                f16(""),
                f16(""),
                f16(""),
                f16("10.0")
            ),
            "13DU1:Meters:",
            target("1.5"),
            shot("F1", "P2", "0.0"),
        ]));
        assert!(warned(&late, 3, "so they are read in metres"));
        let setup = setup_at(&late.project, 0);
        near(setup.setup.instrument_height, 3.048, LENGTH_TOLERANCE);
        let distance = first_of(&observations_to::<DistanceObservation>(setup, "P2"));
        near(distance.distance, 50.0, LENGTH_TOLERANCE);
        near(distance.target_height, 1.5, LENGTH_TOLERANCE);
    }

    // ---- The job and the instrument -----------------------------------------------------

    #[test]
    fn a_second_job_is_read_into_the_same_survey_and_says_so() {
        let result = ok(read_text(records![
            header(),
            format!("10NM{}121111", f16("FIRST")),
            setup_on_p1(),
            format!("10NM{}121111", f16("SECOND")),
        ]));
        assert_eq!(result.project.name, "FIRST");
        assert_eq!(result.project.metadata["job at record 4"], "SECOND");
        assert!(warned(&result, 4, "a second job ('SECOND') begins here"));
        assert_eq!(result.records_read, 4);
    }

    #[test]
    fn with_record_elev_no_a_point_is_given_no_elevation() {
        // [SDR] 3.6.2's second job flag, "Include elevation": 1 No, 2 Yes.
        let keyed = format!(
            "08KI{}{}{}{}{}",
            f16("A"),
            f16("5000.0"),
            f16("200.0"),
            f16("50.0"),
            f16("")
        );
        let without = ok(read_text(records![
            header(),
            format!("10NM{}111111", f16("J")),
            keyed
        ]));
        let a = point(&without.project, "A").expect("A");
        assert!(a.elevation.is_none());
        assert_eq!(without.project.metadata["job: elevations"], "not recorded");
        let with = ok(read_text(records![
            header(),
            format!("10NM{}121111", f16("J")),
            keyed
        ]));
        double_eq(
            value_of(point(&with.project, "A").expect("A").elevation),
            50.0,
        );
    }

    #[test]
    fn the_nikon_header_form_reads_the_refraction_option_as_nikons_constant() {
        // Job flags 122211: curvature and refraction on, refraction option 1 -
        // 0.14 to Sokkia ([SDR] 3.5), 0.132 to Nikon (its manual, page 172).
        let job = format!("10NM{}122211", f16("J"));
        let sokkia = ok(read_text(records![header(), job, setup_on_p1()]));
        double_eq(
            value_of(
                setup_at(&sokkia.project, 0)
                    .instrument
                    .refraction_coefficient,
            ),
            0.14,
        );
        let nikon = ok(read_text(records![
            header_nikon("113111"),
            job,
            setup_on_p1()
        ]));
        double_eq(
            value_of(
                setup_at(&nikon.project, 0)
                    .instrument
                    .refraction_coefficient,
            ),
            0.132,
        );
    }

    #[test]
    fn correction_notes_of_an_sdr2x_job_give_its_switches() {
        // SDR2x has no job flags; its corrections are 13CP notes ([SDR] chapter
        // 4: "Atmos crn: N").
        let result = ok(read_text(records![
            format!(
                "00NM{}0000{}113111",
                f16("SDR20 V03-05"),
                f16("18-Jan-80 20:34")
            ),
            format!("10NM{}", f16("J")),
            "13CPAtmos crn: Y",
            "13CPC and R crn: N",
            "13CPSea level crn: N",
            format!("02NM0001{}{}{}{}", f10(""), f10(""), f10(""), f10("1.5")),
        ]));
        let metadata = &result.project.metadata;
        assert_eq!(metadata["job: atmospheric correction"], "on");
        assert_eq!(metadata["job: curvature and refraction correction"], "off");
        assert_eq!(metadata["job: sea level correction"], "off");
        assert!(any_not_carried_contains(&result, "this job's switch is on"));
    }

    #[test]
    fn time_stamps_in_trimbles_form_point_to_raw_distances_in_what_the_file_did_not_carry() {
        // Trimble's writer stamps "Time Date MM/DD/YYYY Time HH:MM:SS" and
        // writes its distances without the atmospheric correction; the field
        // book stamps "DD-Mon-YY HH:MM". The state stays unknown either way, but
        // what the file did not carry says which writer's form the file is in.
        let stamped = |stamp: &str| {
            read_text(records![
                header(),
                format!("10NM{}122211", f16("J")),
                setup_on_p1(),
                format!("13TS{stamp}"),
            ])
        };
        let trimble = ok(stamped("Time Date 03/14/2026 Time 08:15:00"));
        assert!(any_not_carried_contains(
            &trimble,
            "this file's time stamps are written as Trimble's writer writes them"
        ));
        assert_eq!(
            setup_at(&trimble.project, 0)
                .instrument
                .atmospheric_ppm_state,
            CorrectionState::Unknown
        );
        let field_book = ok(stamped("18-Jan-80 20:14"));
        assert!(!any_not_carried_contains(
            &field_book,
            "Trimble's writer writes them"
        ));
        assert!(any_not_carried_contains(
            &field_book,
            "this job's switch is on"
        ));
    }

    #[test]
    fn an_undefined_vertical_angle_option_drops_the_zeniths_after_it() {
        let result = ok(read_text(records![
            header(),
            instrument33_with('7'),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"),
            shot("F1", "P2", "0.0"),
        ]));
        assert!(warned(&result, 2, "vertical angle option is '7'"));
        let setup = setup_at(&result.project, 0);
        assert!(observations_to::<ZenithAngleObservation>(setup, "P2").is_empty());
        assert_eq!(
            observations_to::<HorizontalDirectionObservation>(setup, "P2").len(),
            1
        );
        assert_eq!(observations_to::<DistanceObservation>(setup, "P2").len(), 1);
    }

    #[test]
    fn edm_and_reflector_offsets_are_warned_about_and_kept() {
        let result = ok(read_text(records![
            header(),
            format!(
                "01NM:{}000000{}00000131{}{}{}",
                field("", 16),
                f16("HAND"),
                f16("0.050"),
                f16("-0.020"),
                f16("0")
            ),
            setup_on_p1(),
        ]));
        assert!(warned(
            &result,
            2,
            "EDM offset is 0.05 m, which is not applied"
        ));
        assert!(warned(
            &result,
            2,
            "reflector offset is -0.02 m, which is not applied"
        ));
        assert_eq!(
            result.project.metadata["instrument: EDM offset (m)"],
            "0.05"
        );
        assert_eq!(
            result.project.metadata["instrument: reflector offset (m)"],
            "-0.02"
        );
    }

    #[test]
    fn no_instrument_or_weather_record_is_said_once_and_in_what_the_file_did_not_carry() {
        let result = ok(read_text(records![
            header(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"),
            shot("F1", "P2", "0.0"),
            shot("F1", "P3", "10.0"),
        ]));
        assert!(warned(
            &result,
            5,
            "no instrument record comes before the first observation"
        ));
        assert_eq!(warnings_with(&result, "no instrument record"), 1);
        assert!(any_not_carried_contains(&result, "no instrument record"));
        assert!(any_not_carried_contains(
            &result,
            "no pressure and temperature"
        ));
    }

    // ---- Records that say nothing, or come too early -------------------------------------

    #[test]
    fn empty_weather_scale_collimation_and_target_records_are_skipped() {
        let result = ok(read_text(records![
            header(),
            setup_on_p1(),
            "05NM",
            "06NM",
            format!("06NM{}", f16("0.0")),
            "04CL",
            "03NM",
        ]));
        assert_eq!(result.records_skipped, 5);
        assert!(warned(&result, 3, "neither a pressure nor a temperature"));
        assert!(warned(&result, 4, "without a positive factor"));
        assert!(warned(&result, 5, "without a positive factor"));
        assert!(warned(&result, 6, "collimation record with no value"));
        assert!(warned(&result, 7, "target height record with no height"));
    }

    #[test]
    fn a_collimation_record_is_kept_with_its_setup() {
        // [SDR] 3.6.2 COL: vertical then horizontal collimation, angles; 10" is
        // 0.00277778 degrees. Kept with the setup it falls in, as well as
        // applied to the readings after it (the next tests).
        let result = ok(read_text(records![
            header(),
            setup_on_p1(),
            format!("04CL{}{}", f16("0.00277778"), f16("-0.00138889")),
        ]));
        let metadata = &setup_at(&result.project, 0).metadata;
        near(
            value_of(parse_finite_double(
                metadata["collimation, vertical (radians)"].as_bytes(),
            )),
            0.00277778 * PI / 180.0,
            1e-15,
        );
        near(
            value_of(parse_finite_double(
                metadata["collimation, horizontal (radians)"].as_bytes(),
            )),
            -0.00138889 * PI / 180.0,
            1e-15,
        );
    }

    #[test]
    fn a_collimation_correction_is_added_on_face_one_and_taken_off_on_face_two() {
        // [SETX] 29.2.5: face 1 a2 = a1 + Vc, b2 = b1 + Hc; face 2 a2 = a1 - Vc,
        // b2 = b1 - Hc, a1 the zenith equivalent of 29.2.3. Here Vc 0.02 and Hc
        // 0.01 degrees. A shot before the 04 is not corrected; a reading with
        // no vertical angle is face 1 (29.2.3), so an MD with none gets + Hc.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            shot("F1", "P0", "45.0"),                      // 5
            format!("04CL{}{}", f16("0.02"), f16("0.01")), // 6
            shot_at("F1", "P2", "90.0", "88.0"),           // 7
            shot_at("F2", "P2", "270.0", "272.0"),         // 8
            format!(
                "09MD{}{}{}{}{}",
                f16("P1"),
                f16("P3"),
                f16("40.0"),
                f16(""),
                f16("30.0")
            ), // 9
        ]));
        let setup = setup_at(&result.project, 0);
        let before = first_of(&observations_to::<HorizontalDirectionObservation>(
            setup, "P0",
        ));
        near(before.direction, degrees(45.0), ANGLE_TOLERANCE);
        let directions = observations_to::<HorizontalDirectionObservation>(setup, "P2");
        let zeniths = observations_to::<ZenithAngleObservation>(setup, "P2");
        assert_eq!(directions.len(), 2);
        assert_eq!(zeniths.len(), 2);
        near(directions[0].direction, degrees(90.01), ANGLE_TOLERANCE);
        near(zeniths[0].angle, degrees(88.02), ANGLE_TOLERANCE);
        near(directions[1].direction, degrees(269.99), ANGLE_TOLERANCE);
        // 272 on face 2 is 88 on face 1, less Vc.
        near(zeniths[1].angle, degrees(87.98), ANGLE_TOLERANCE);
        let md = first_of(&observations_to::<HorizontalDirectionObservation>(
            setup, "P3",
        ));
        near(md.direction, degrees(30.01), ANGLE_TOLERANCE);
        assert_eq!(
            setup.metadata["collimation of record 6"],
            "applied to its readings ([SETX] 29.2.5)"
        );
    }

    #[test]
    fn a_collimation_correction_moves_a_shot_on_one_face_and_leaves_a_face_pairs_mean() {
        // An instrument whose line of sight is 36" left of square: face 1 reads
        // 0.01 degrees low, face 2 0.01 high, and its 04 gives Hc 0.01. The
        // backsight B, due north of A, read 359.99 and 180.01: corrected, 0 and
        // 180, a pair in agreement, whose mean - the orientation - is what the
        // raw pair's was. C on face 1 only, at circle 90, 100 m level: azimuth
        // 90.01, so N = 1000 + 100 cos 90.01 = 999.98254670, E = 1000 + 100 sin
        // 90.01 = 1099.99999848 - 17.45 mm from where the raw reading puts it.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            format!(
                "08KI{}{}{}{}{}",
                f16("B"),
                f16("1100.000"),
                f16("1000.000"),
                f16("100.000"),
                f16("")
            ),
            format!(
                "02KI{}{}{}{}{}",
                f16("A"),
                f16("1000.000"),
                f16("1000.000"),
                f16("100.000"),
                f16("0.000")
            ),
            format!("04CL{}{}", f16("0.0"), f16("0.01")),
            target("0.000"),
            format!("07NM{}{}{}{}", f16("A"), f16("B"), f16("0.0"), f16("0.0")),
            format!(
                "09F1{}{}{}{}{}",
                f16("A"),
                f16("B"),
                f16("100.000"),
                f16("90.0"),
                f16("359.99")
            ),
            format!(
                "09F2{}{}{}{}{}",
                f16("A"),
                f16("B"),
                f16("100.000"),
                f16("270.0"),
                f16("180.01")
            ),
            format!(
                "09F1{}{}{}{}{}",
                f16("A"),
                f16("C"),
                f16("100.000"),
                f16("90.0"),
                f16("90.0")
            ),
        ]));
        let outcome = ok(reduced(&result.project));
        near(
            outcome.report.setups[0]
                .orientation_correction
                .expect("an orientation correction"),
            0.0,
            1e-12,
        );
        // The corrected faces agree: no pair is outside the reduction's
        // tolerance, as the raw pair (72" apart) would be.
        assert_eq!(
            report_warnings_with(&outcome.report, "face pair outside tolerance"),
            0
        );
        let c = computed(&outcome, "C").expect("C");
        near(c.northing, 1000.0 + 100.0 * degrees(90.01).cos(), 1e-9);
        near(c.easting, 1000.0 + 100.0 * degrees(90.01).sin(), 1e-9);
        near(c.northing, 999.98254670, 1e-8);
    }

    #[test]
    fn a_collimation_holds_until_another_instrument_type_or_another_job() {
        // [SETX] 13.1: a collimation applies "until either the instrument type
        // is changed or a new collimation record is added", and "Collimation is
        // not maintained across all jobs" (the Level 5 manual, chapter 13, the
        // same). With Hc 0.01 degrees a face 1 shot at circle 90 reads 90.01
        // while it holds and 90 after. An instrument record restating the EDM
        // type ':' keeps it; one of type '=' ends it, and so does a second job
        // - each said where it happens.
        let other_type = format!(
            "01NM={}000000{}00000131{}{}{}",
            field("", 16),
            f16("HAND"),
            f16(""),
            f16(""),
            f16("0")
        );
        let setup_on = |at: &str| {
            format!(
                "02NM{}{}{}{}{}",
                f16(at),
                f16(""),
                f16(""),
                f16(""),
                f16("1.5")
            )
        };
        let shot_from = |at: &str, to: &str| {
            format!(
                "09F1{}{}{}{}{}",
                f16(at),
                f16(to),
                f16("50.0"),
                f16("90.0"),
                f16("90.0")
            )
        };
        let collimation = format!("04CL{}{}", f16("0.0"), f16("0.01"));
        let result = ok(read_text(records![
            header(),
            format!("10NM{}121111", f16("FIRST")),
            instrument33(),
            setup_on("P1"),
            target("1.5"),
            collimation,                            // 6
            shot_from("P1", "T1"),                  // 7
            instrument33(),                         // 8
            setup_on("P2"),                         // 9
            shot_from("P2", "T2"),                  // 10
            other_type,                             // 11
            setup_on("P3"),                         // 12
            shot_from("P3", "T3"),                  // 13
            collimation,                            // 14
            shot_from("P3", "T4"),                  // 15
            format!("10NM{}121111", f16("SECOND")), // 16
            setup_on("P4"),                         // 17
            shot_from("P4", "T5"),                  // 18
        ]));
        let project = &result.project;
        let expected = [
            ("P1", "T1", 90.01),
            ("P2", "T2", 90.01),
            ("P3", "T3", 90.0),
            ("P3", "T4", 90.01),
            ("P4", "T5", 90.0),
        ];
        for (setup, to, circle) in expected {
            let station = station_named(project, setup).expect("the setup");
            let direction = first_of(&observations_to::<HorizontalDirectionObservation>(
                station, to,
            ));
            near(direction.direction, degrees(circle), ANGLE_TOLERANCE);
        }
        assert!(warned(
            &result,
            11,
            "the instrument type changes here (EDM type ':' to '='), so the collimation of \
             record 6 is not applied after it"
        ));
        assert!(warned(
            &result,
            16,
            "the collimation of record 14 is not applied in job 'SECOND'"
        ));
        assert_eq!(warnings_with(&result, "collimation of record"), 2);
    }

    #[test]
    fn an_instrument_type_changed_in_the_middle_of_a_setup_ends_its_collimation_there_and_says_so()
    {
        // Hc 0.01 degrees: C, shot on face 1 at circle 90, reads 90.01. An 01 of
        // EDM type '=' (the setup's is ':') comes before D, a shot of the same
        // setup. The setup keeps the settings it began with, but a collimation
        // is applied reading by reading and ends at another instrument type
        // ([SETX] 13.1), so D reads 90 - and the warning about the change in the
        // middle of the setup names the collimation as the exception, where it
        // said the setup kept everything.
        let other_type = format!(
            "01NM={}000000{}00000131{}{}{}",
            field("", 16),
            f16("HAND"),
            f16(""),
            f16(""),
            f16("0")
        );
        let result = ok(read_text(records![
            header(),
            instrument33(),
            format!("04CL{}{}", f16("0.0"), f16("0.01")), // 3
            setup_on_p1(),                                // 4
            target("1.5"),                                // 5
            backsight("P2", "0.0", "0.0"),                // 6
            shot("F1", "C", "90.0"),                      // 7
            other_type,                                   // 8
            shot("F1", "D", "90.0"),                      // 9
        ]));
        assert_eq!(result.project.stations.len(), 1);
        let setup = setup_at(&result.project, 0);
        near(
            first_of(&observations_to::<HorizontalDirectionObservation>(
                setup, "C",
            ))
            .direction,
            degrees(90.01),
            ANGLE_TOLERANCE,
        );
        near(
            first_of(&observations_to::<HorizontalDirectionObservation>(
                setup, "D",
            ))
            .direction,
            degrees(90.0),
            ANGLE_TOLERANCE,
        );
        assert!(warned(
            &result,
            8,
            "so the collimation of record 3 is not applied after it"
        ));
        assert!(warned(
            &result,
            8,
            "the instrument changes in the middle of setup 'P1'; the setup keeps the values it \
             began with, and these apply from the next setup - all but the collimation of record \
             3, which ends here all the same"
        ));
        assert_eq!(
            setup.metadata["the instrument changed at record 8"],
            "not applied to this setup, but the collimation of record 3 ends at it"
        );
    }

    #[test]
    fn records_that_name_no_point_or_come_before_any_station_are_skipped() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            format!("07NM{}{}{}{}", f16("P1"), f16("P2"), f16("0.0"), f16("0.0")), // 3
            format!(
                "09F1{}{}{}{}{}",
                f16("P1"),
                f16("P2"),
                f16("50.0"),
                f16("90.0"),
                f16("0.0")
            ), // 4
            format!(
                "02NM{}{}{}{}{}",
                f16(""),
                f16(""),
                f16(""),
                f16(""),
                f16("1.5")
            ), // 5
            format!("08KI{}{}{}", f16(""), f16("1.0"), f16("2.0")),                // 6
            format!("11KI{}{}{}", f16("P1"), f16(""), f16("45.0")),                // 7
            setup_on_p1(),                                                         // 8
        ]));
        assert_eq!(result.records_skipped, 5);
        assert!(warned(
            &result,
            3,
            "backsight record with no station record before it"
        ));
        assert!(warned(
            &result,
            4,
            "observation with no station record before it"
        ));
        assert!(warned(&result, 5, "station record names no point"));
        assert!(warned(&result, 6, "coordinate record names no point"));
        assert!(warned(
            &result,
            7,
            "reduced observation record names no point"
        ));
        assert_eq!(result.project.stations.len(), 1);
    }

    #[test]
    fn a_backsight_or_observation_that_names_no_target_or_its_own_point_is_skipped() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            format!("07NM{}{}{}{}", f16("P1"), f16(""), f16("0.0"), f16("0.0")), // 5
            format!("07NM{}{}{}{}", f16("P1"), f16("P1"), f16("0.0"), f16("0.0")), // 6
            format!(
                "09F1{}{}{}{}{}",
                f16("P1"),
                f16(""),
                f16("50.0"),
                f16("90.0"),
                f16("0.0")
            ), // 7
        ]));
        assert_eq!(result.records_skipped, 3);
        assert!(warned(
            &result,
            5,
            "backsight record names no backsight point"
        ));
        assert!(warned(&result, 6, "backsight record from 'P1' to itself"));
        assert!(warned(&result, 7, "observation names no target point"));
        assert!(setup_at(&result.project, 0).backsight_point_id.is_empty());
        assert!(setup_at(&result.project, 0).observations.is_empty());
    }

    #[test]
    fn a_point_given_one_ordinate_only_is_imported_without_coordinates_and_says_so() {
        let result = ok(read_text(records![
            header(),
            format!(
                "08KI{}{}{}{}",
                f16("A"),
                f16("5000.0"),
                f16(""),
                f16("10.0")
            ),
        ]));
        assert!(warned(
            &result,
            2,
            "point 'A' is given only one of its northing and easting; it is imported without \
             coordinates"
        ));
        assert!(result.project.points.is_empty());
        assert!(unpositioned(&result.project, "A").is_some());
    }

    #[test]
    fn an_elevation_with_no_northing_or_easting_is_kept_with_its_point_and_said() {
        // A benchmark keyed with its height alone, and a setup on another with
        // its height alone ([SDR] 3.3 (ii): blank is "not measured"). Neither
        // can place a point; each height is kept, not dropped. With the job's
        // "Record elev" No an elevation is a placeholder ([SETX] 4) and is not.
        let kept = ok(read_text(records![
            header(),
            format!(
                "08KI{}{}{}{}{}",
                f16("BM1"),
                f16(""),
                f16(""),
                f16("123.456"),
                f16("BENCH")
            ),
            format!(
                "02NM{}{}{}{}{}",
                f16("BM2"),
                f16(""),
                f16(""),
                f16("99.0"),
                f16("1.5")
            ),
        ]));
        assert!(warned(
            &kept,
            2,
            "point 'BM1' is given an elevation (123.456 m) and no northing or easting"
        ));
        assert!(warned(&kept, 3, "point 'BM2' is given an elevation (99 m)"));
        let bm1 = unpositioned(&kept.project, "BM1").expect("BM1");
        assert_eq!(bm1.metadata["height without a position"], "123.456");
        let bm2 = unpositioned(&kept.project, "BM2").expect("BM2");
        assert_eq!(bm2.metadata["height without a position"], "99");
        let placeholder = ok(read_text(records![
            header(),
            format!("10NM{}111111", f16("J")),
            format!(
                "08KI{}{}{}{}{}",
                f16("BM1"),
                f16(""),
                f16(""),
                f16("123.456"),
                f16("")
            ),
        ]));
        assert_eq!(placeholder.warnings.len(), 0);
        let unheighted = unpositioned(&placeholder.project, "BM1").expect("BM1");
        assert!(
            !unheighted
                .metadata
                .contains_key("height without a position")
        );
    }

    #[test]
    fn a_setup_with_no_instrument_height_uses_zero_and_says_so() {
        let result = ok(read_text(records![header(), format!("02NM{}", f16("P1"))]));
        assert!(warned(&result, 2, "states no instrument height; 0 is used"));
        double_eq(setup_at(&result.project, 0).setup.instrument_height, 0.0);
    }

    #[test]
    fn coordinates_are_entered_observed_calculated_or_unknown_by_their_derivation_code() {
        // [SDR] 3.4: KI keyed in, TP a topographical shot stored as a position,
        // AJ the traverse adjustment, TV the traverse, RS the resection.
        let coded = |code: &str, id: &str| {
            format!(
                "08{code}{}{}{}{}{}",
                f16(id),
                f16("5000.0"),
                f16("200.0"),
                f16(""),
                f16("")
            )
        };
        let result = ok(read_text(records![
            header(),
            coded("KI", "A"),
            coded("TP", "B"),
            coded("AJ", "C"),
            coded("TV", "D"),
            coded("RS", "E"),
            coded("NM", "F"),
        ]));
        let expected = [
            ("A", CoordinateSource::Entered),
            ("B", CoordinateSource::FieldObserved),
            ("C", CoordinateSource::Calculated),
            ("D", CoordinateSource::Calculated),
            ("E", CoordinateSource::Calculated),
            ("F", CoordinateSource::Unknown),
        ];
        for (id, source) in expected {
            let p = point(&result.project, id).expect("the point");
            assert_eq!(p.coordinate_source, source, "{id}");
        }
    }

    #[test]
    fn the_points_latest_coordinates_are_its_own_as_the_field_book_keeps_them() {
        // [SDR] chapter 2's traverse, feet, in the SDR2x records a 4-digit job
        // is sent in ([SDR] 3.2.5): 0002 and 0003 are stored as positions (POS
        // TP), 0003 restated by its setup (STN TP, 0.005 and 200.005), and the
        // traverse adjustment's POS AJ records come last. [SDR] 2.3: "the latest
        // coordinates are the best" - 0002 N -0.002 E 99.996 Elev -0.002 and
        // 0003 N 0.004 E 199.996 Elev 0.006, the adjusted ones. The earlier
        // ones stay in each point's metadata, each change said.
        let result = ok(read_text(records![
            format!(
                "00NM{}0000{}122211",
                f16("SDR20 V03-05"),
                f16("06-Jan-80 00:23")
            ),
            format!("10NM{}", f16("TRAVERSE")),
            format!("08KI0001{}{}{}", f10("0.000"), f10("0.000"), f10("0.000")),
            format!("08KI0005{}{}{}", f10("100.000"), f10("0.000"), f10("")),
            format!(
                "02TP0001{}{}{}{}",
                f10("0.000"),
                f10("0.000"),
                f10("0.000"),
                f10("0.000")
            ),
            format!("07TP00010005{}{}", f10("0.000000"), f10("0.000000")),
            format!("08TP0002{}{}{}", f10("0.000"), f10("100.000"), f10("0.000")), // 7
            format!(
                "02TP0002{}{}{}{}",
                f10("0.000"),
                f10("100.000"),
                f10("0.000"),
                f10("0.000")
            ), // 8
            format!("08TP0003{}{}{}", f10("0.003"), f10("200.001"), f10("0.008")), // 9
            format!(
                "02TP0003{}{}{}{}",
                f10("0.005"),
                f10("200.005"),
                f10("0.010"),
                f10("0.000")
            ), // 10
            format!(
                "08AJ0002{}{}{}",
                f10("-0.002"),
                f10("99.996"),
                f10("-0.002")
            ), // 11
            format!("08AJ0003{}{}{}", f10("0.004"), f10("199.996"), f10("0.006")), // 12
        ]));
        let foot: f64 = 0.3048;
        let adjusted = [
            (
                "0002",
                -0.002,
                99.996,
                -0.002,
                11,
                "coordinates of record 7, superseded",
            ),
            (
                "0003",
                0.004,
                199.996,
                0.006,
                12,
                "coordinates of record 10, superseded",
            ),
        ];
        for (id, northing, easting, elevation, record, superseded) in adjusted {
            let point = point(&result.project, id).expect("the point");
            near(point.northing, northing * foot, LENGTH_TOLERANCE);
            near(point.easting, easting * foot, LENGTH_TOLERANCE);
            near(
                value_of(point.elevation),
                elevation * foot,
                LENGTH_TOLERANCE,
            );
            assert_eq!(point.coordinate_source, CoordinateSource::Calculated);
            assert_eq!(point.source.record_number, record);
            assert!(point.metadata.contains_key(superseded), "{id}");
            assert!(
                warned(&result, record, "these, the latest, are kept"),
                "{id}"
            );
        }
        // 0003's setup restated it differently from its POS TP; the same
        // coordinates again (0002's setup, 0001's) are a repeat and say nothing.
        assert!(warned(
            &result,
            10,
            "point '0003' is given different coordinates here"
        ));
        assert!(
            point(&result.project, "0003")
                .expect("0003")
                .metadata
                .contains_key("coordinates of record 9, superseded")
        );
        assert_eq!(warnings_with(&result, "is given different coordinates"), 3);
    }

    #[test]
    fn a_shots_position_view_sent_beside_it_never_supersedes_a_points_coordinates() {
        // [SETX] 27.2: a field book sending both its current view and the POS
        // view writes "a raw observation record followed by a position record";
        // the shot stays in OBS view, which "will NOT overwrite a previous
        // coordinate if it exists in POS view" (8.5.2). From A (N 1000 E 1000),
        // oriented on B due north, 1.5 m instrument and target: C, keyed at N
        // 1000 E 1100, is checked by a shot of 100.013 m whose POS view says E
        // 1100.013: C stays where it was keyed, and Entered, the POS view in its
        // metadata, said at the 08; D, placed by nothing else, takes its shot's
        // POS view; E, shot twice, takes the later POS view: a point "in OBS
        // view only" is overwritten (8.5.2); F, keyed at N 900, is given N
        // 900.004 by an 08 TP after a shot of another point - no POS view, but
        // a position record, which supersedes as a shot stored in POS view
        // does. A later setup on C, restating its keyed coordinates, changes
        // nothing.
        let keyed = |id: &str, n: &str, e: &str| {
            format!(
                "08KI{}{}{}{}{}",
                f16(id),
                f16(n),
                f16(e),
                f16("50.000"),
                f16("")
            )
        };
        let shot_to = |to: &str, distance: &str, circle: &str| {
            format!(
                "09F1{}{}{}{}{}",
                f16("A"),
                f16(to),
                f16(distance),
                f16("90.0"),
                f16(circle)
            )
        };
        let position = |id: &str, n: &str, e: &str| {
            format!(
                "08TP{}{}{}{}{}",
                f16(id),
                f16(n),
                f16(e),
                f16("50.000"),
                f16("")
            )
        };
        let result = ok(read_text(records![
            header(),
            instrument33(),
            keyed("A", "1000.000", "1000.000"), // 3
            keyed("B", "1100.000", "1000.000"), // 4
            keyed("C", "1000.000", "1100.000"), // 5
            keyed("F", "900.000", "1000.000"),  // 6
            format!(
                "02NM{}{}{}{}{}",
                f16("A"),
                f16(""),
                f16(""),
                f16(""),
                f16("1.500")
            ), // 7
            target("1.500"),                    // 8
            format!("07NM{}{}{}{}", f16("A"), f16("B"), f16("0.0"), f16("0.0")), // 9
            shot_to("B", "100.000", "0.0"),     // 10
            shot_to("C", "100.013", "90.0"),    // 11
            position("C", "1000.000", "1100.013"), // 12
            shot_to("D", "50.000", "45.0"),     // 13
            position("D", "1035.355", "1035.355"), // 14
            shot_to("E", "30.000", "180.0"),    // 15
            position("E", "970.000", "1000.000"), // 16
            shot_to("E", "30.002", "180.0"),    // 17
            position("E", "969.998", "1000.000"), // 18
            shot_to("T", "10.000", "270.0"),    // 19
            position("F", "900.004", "1000.000"), // 20
            format!(
                "02NM{}{}{}{}{}",
                f16("C"),
                f16("1000.000"),
                f16("1100.000"),
                f16("50.000"),
                f16("1.500")
            ), // 21
        ]));
        let project = &result.project;
        let c = point(project, "C").expect("C");
        double_eq(c.easting, 1100.0);
        double_eq(c.northing, 1000.0);
        assert_eq!(c.coordinate_source, CoordinateSource::Entered);
        assert_eq!(c.source.record_number, 5);
        assert_eq!(
            c.metadata["coordinates restated at record 12"],
            "N 1000, E 1100.013, elevation 50"
        );
        assert!(warned(
            &result,
            12,
            "those are kept and these are in the point's metadata, since they are the POS view \
             of the observation at record 11"
        ));
        let d = point(project, "D").expect("D");
        double_eq(d.northing, 1035.355);
        assert_eq!(d.coordinate_source, CoordinateSource::FieldObserved);
        let e = point(project, "E").expect("E");
        double_eq(e.northing, 969.998);
        assert!(warned(&result, 18, "these, the latest, are kept"));
        let f = point(project, "F").expect("F");
        double_eq(f.northing, 900.004);
        assert_eq!(f.coordinate_source, CoordinateSource::FieldObserved);
        assert!(warned(&result, 20, "these, the latest, are kept"));
        // Nothing said of C at its setup: the same coordinates again.
        assert!(!warned(&result, 21, "point 'C'"));
        // C is control: the reduction holds it where it was keyed.
        let outcome = ok(reduced(project));
        let held = computed(&outcome, "C").expect("C");
        near(held.northing, 1000.0, 1e-9);
        near(held.easting, 1100.0, 1e-9);
    }

    #[test]
    fn a_position_view_after_an_mc_or_red_record_is_that_observations_and_never_supersedes_control()
    {
        // [SETX] 27.2: with more than one view on, the field book writes "more
        // than one record for each observation record", one after another; an
        // MC or RED record has the views a raw one has (chapter 6: "MC and Red
        // records can also be stored in Pos view"), and 8.5.5 prints an
        // averaged OBS MC followed by its POS TP. From A (N 2000 E 1000),
        // oriented on B due north, every shot level: C, keyed at N 2000 E 1100,
        // checked on face 1 and face 2 at 100.013 m and by the MC averaging
        // them, each followed by its POS view, E 1100.013 (a set sent in its
        // current and POS views); D, keyed at N 1900 E 1000, by an MC alone at
        // azimuth 180, 100.013 m, and its POS view N 1899.987 (the MC and POS
        // views); E, keyed at N 2000 E 900, by a RED at azimuth 270, 100.013 m,
        // and its POS view E 899.987 (the RED and POS views); G, keyed at N
        // 2050 E 1000, by a shot at circle 0, 50.013 m, its MC and then its POS
        // view N 2050.013 (the OBS, MC and POS views). Each stays where it was
        // keyed, and Entered, every POS view said at its 08 with the record it
        // is the view of. H, placed by nothing else, takes the POS view of an
        // MC at azimuth 90, 40 m - N 2000 E 1040: the MC's observation is lost,
        // no raw one repeating it, and its position is not.
        let keyed = |id: &str, n: &str, e: &str| {
            format!(
                "08KI{}{}{}{}{}",
                f16(id),
                f16(n),
                f16(e),
                f16("50.000"),
                f16("")
            )
        };
        let observed = |code: &str, to: &str, distance: &str, vertical: &str, horizontal: &str| {
            format!(
                "09{code}{}{}{}{}{}",
                f16("A"),
                f16(to),
                f16(distance),
                f16(vertical),
                f16(horizontal)
            )
        };
        let position = |code: &str, id: &str, n: &str, e: &str| {
            format!(
                "08{code}{}{}{}{}{}",
                f16(id),
                f16(n),
                f16(e),
                f16("50.000"),
                f16("")
            )
        };
        let result = ok(read_text(records![
            header(),
            instrument33(),
            keyed("A", "2000.000", "1000.000"), // 3
            keyed("B", "2100.000", "1000.000"), // 4
            keyed("C", "2000.000", "1100.000"), // 5
            keyed("D", "1900.000", "1000.000"), // 6
            keyed("E", "2000.000", "900.000"),  // 7
            keyed("G", "2050.000", "1000.000"), // 8
            format!(
                "02NM{}{}{}{}{}",
                f16("A"),
                f16(""),
                f16(""),
                f16(""),
                f16("1.500")
            ), // 9
            target("1.500"),                    // 10
            format!("07NM{}{}{}{}", f16("A"), f16("B"), f16("0.0"), f16("0.0")), // 11
            observed("F1", "C", "100.013", "90.0", "90.0"), // 12
            position("SC", "C", "2000.000", "1100.013"), // 13
            observed("F2", "C", "100.013", "270.0", "270.0"), // 14
            position("SC", "C", "2000.000", "1100.013"), // 15
            observed("MC", "C", "100.013", "90.0", "90.0"), // 16
            position("SC", "C", "2000.000", "1100.013"), // 17
            observed("MC", "D", "100.013", "90.0", "180.0"), // 18
            position("TP", "D", "1899.987", "1000.000"), // 19
            format!(
                "11TP{}{}{}{}{}",
                f16("A"),
                f16("E"),
                f16("270.0"),
                f16("100.013"),
                f16("0.000")
            ), // 20
            position("TP", "E", "2000.000", "899.987"), // 21
            observed("F1", "G", "50.013", "90.0", "0.0"), // 22
            observed("MC", "G", "50.013", "90.0", "0.0"), // 23
            position("TP", "G", "2050.013", "1000.000"), // 24
            observed("MC", "H", "40.000", "90.0", "90.0"), // 25
            position("TP", "H", "2000.000", "1040.000"), // 26
        ]));
        let project = &result.project;
        let held = [
            ("C", 2000.0, 1100.0, 5),
            ("D", 1900.0, 1000.0, 6),
            ("E", 2000.0, 900.0, 7),
            ("G", 2050.0, 1000.0, 8),
        ];
        for (id, northing, easting, keyed_at) in held {
            let p = point(project, id).expect("the point");
            double_eq(p.northing, northing);
            double_eq(p.easting, easting);
            assert_eq!(p.coordinate_source, CoordinateSource::Entered, "{id}");
            assert_eq!(p.source.record_number, keyed_at, "{id}");
        }
        let views = [
            (
                13,
                "the POS view of the observation at record 12, which the field book keeps in OBS \
                 view",
            ),
            (
                15,
                "the POS view of the observation at record 14, which the field book keeps in OBS \
                 view",
            ),
            (
                17,
                "the POS view of the corrected observation (MC) at record 16, which the field \
                 book keeps in MC view",
            ),
            (
                19,
                "the POS view of the corrected observation (MC) at record 18, which the field \
                 book keeps in MC view",
            ),
            (
                21,
                "the POS view of the reduced observation (RED) at record 20, which the field book \
                 keeps in RED view",
            ),
            (
                24,
                "the POS view of the corrected observation (MC) at record 23, which the field \
                 book keeps in MC view",
            ),
        ];
        for (record, words) in views {
            assert!(
                warned(
                    &result,
                    record,
                    "those are kept and these are in the point's metadata"
                ),
                "{record}"
            );
            assert!(warned(&result, record, words), "{record}");
        }
        assert_eq!(warnings_with(&result, "these, the latest, are kept"), 0);
        assert_eq!(
            point(project, "C").expect("C").metadata["coordinates restated at record 17"],
            "N 2000, E 1100.013, elevation 50"
        );
        let h = point(project, "H").expect("H");
        double_eq(h.northing, 2000.0);
        double_eq(h.easting, 1040.0);
        assert_eq!(h.coordinate_source, CoordinateSource::FieldObserved);
        assert_eq!(h.source.record_number, 26);
        // The MCs of D and H and the RED of E have no raw twin: their
        // observations are lost; those of C and G repeat raw shots.
        assert!(warned(&result, 25, "so this shot is lost"));
        assert!(any_not_carried_contains(
            &result,
            "2 corrected (MC) observation(s)"
        ));
        assert!(any_not_carried_contains(
            &result,
            "1 reduced (RED) observation(s)"
        ));
    }

    #[test]
    fn a_coordinate_record_is_a_shots_position_view_only_straight_after_the_shot() {
        // [SETX] 27.2 writes an observation's views one after another, so an 08
        // with another record between it and a shot of its point is a record
        // of its own. C, keyed at N 2000 E 1100, is shot at azimuth 90, 100.013
        // m; after a note, an 08 TP gives it E 1100.013: a position record, the
        // latest, which C takes. [SETX] 8.5.5 prints a new averaged position
        // straight after the POS view it averages: D, keyed at N 1900 E 1000,
        // is shot at azimuth 180, 100.013 m; its POS view (N 1899.987) is set
        // aside, and the 08 TP after it, N 1899.9935 - the mean of 1900 and
        // 1899.987 - is D's new position.
        let keyed = |id: &str, n: &str, e: &str| {
            format!(
                "08KI{}{}{}{}{}",
                f16(id),
                f16(n),
                f16(e),
                f16("50.000"),
                f16("")
            )
        };
        let position = |id: &str, n: &str, e: &str| {
            format!(
                "08TP{}{}{}{}{}",
                f16(id),
                f16(n),
                f16(e),
                f16("50.000"),
                f16("")
            )
        };
        let result = ok(read_text(records![
            header(),
            instrument33(),
            keyed("A", "2000.000", "1000.000"), // 3
            keyed("B", "2100.000", "1000.000"), // 4
            keyed("C", "2000.000", "1100.000"), // 5
            keyed("D", "1900.000", "1000.000"), // 6
            format!(
                "02NM{}{}{}{}{}",
                f16("A"),
                f16(""),
                f16(""),
                f16(""),
                f16("1.500")
            ), // 7
            target("1.500"),                    // 8
            format!("07NM{}{}{}{}", f16("A"), f16("B"), f16("0.0"), f16("0.0")), // 9
            format!(
                "09F1{}{}{}{}{}",
                f16("A"),
                f16("C"),
                f16("100.013"),
                f16("90.0"),
                f16("90.0")
            ), // 10
            "13NMchecked C",                    // 11
            position("C", "2000.000", "1100.013"), // 12
            format!(
                "09F1{}{}{}{}{}",
                f16("A"),
                f16("D"),
                f16("100.013"),
                f16("90.0"),
                f16("180.0")
            ), // 13
            position("D", "1899.987", "1000.000"), // 14
            position("D", "1899.9935", "1000.000"), // 15
        ]));
        let c = point(&result.project, "C").expect("C");
        double_eq(c.easting, 1100.013);
        assert_eq!(c.coordinate_source, CoordinateSource::FieldObserved);
        assert_eq!(c.source.record_number, 12);
        assert!(warned(&result, 12, "these, the latest, are kept"));
        let d = point(&result.project, "D").expect("D");
        double_eq(d.northing, 1899.9935);
        assert_eq!(d.source.record_number, 15);
        assert!(warned(
            &result,
            14,
            "the POS view of the observation at record 13"
        ));
        assert!(warned(
            &result,
            15,
            "from those of record 6; these, the latest, are kept"
        ));
    }

    #[test]
    fn a_point_id_holding_a_control_byte_is_skipped_not_merged_with_another() {
        // Line noise in an id field: "\x01" and "\x02" blanked would both be one
        // point named with a blank, and two shots 110 degrees apart would place
        // it at neither. Each record is skipped, naming the byte.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"),
            shot("F1", "P2", "0.0"),
            format!(
                "09F1{}{}{}{}{}",
                f16("P1"),
                f16("\x01"),
                f16("10.0"),
                f16("90.0"),
                f16("10.0")
            ), // 7
            format!(
                "09F1{}{}{}{}{}",
                f16("P1"),
                f16("\x02"),
                f16("20.0"),
                f16("90.0"),
                f16("120.0")
            ), // 8
            format!(
                "08KI{}{}{}{}{}",
                f16("Q\x7f"),
                f16("1.0"),
                f16("2.0"),
                f16(""),
                f16("")
            ), // 9
            format!(
                "02NM{}{}{}{}{}",
                f16("\x01"),
                f16(""),
                f16(""),
                f16(""),
                f16("1.5")
            ), // 10
        ]));
        assert_eq!(result.records_skipped, 4);
        assert!(warned(&result, 7, "control byte \\x01"));
        assert!(warned(&result, 8, "control byte \\x02"));
        assert!(warned(&result, 9, "control byte \\x7F"));
        assert!(warned(&result, 10, "control byte \\x01"));
        assert_eq!(result.project.stations.len(), 1);
        assert!(result.project.points.is_empty());
        // The setup's point and its backsight, and no point named with noise.
        let named: BTreeSet<&str> = result
            .project
            .unpositioned_points
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(named, BTreeSet::from(["P1", "P2"]));
    }

    // ---- Observations ---------------------------------------------------------------------

    #[test]
    fn a_zero_or_negative_slope_distance_is_no_distance() {
        for (distance, words) in [
            ("0.000", "slope distance of 0 read as no"),
            ("-5.0", "slope distance of -5 read as no"),
        ] {
            let result = ok(read_text(records![
                header(),
                instrument33(),
                setup_on_p1(),
                target("1.5"),
                backsight("P2", "0.0", "0.0"),
                format!(
                    "09F1{}{}{}{}{}",
                    f16("P1"),
                    f16("P2"),
                    f16(distance),
                    f16("90.0"),
                    f16("0.0")
                ),
            ]));
            assert!(warned(&result, 6, words), "{distance}");
            let setup = setup_at(&result.project, 0);
            assert!(observations_to::<DistanceObservation>(setup, "P2").is_empty());
            assert_eq!(
                observations_to::<ZenithAngleObservation>(setup, "P2").len(),
                1
            );
            assert_eq!(
                observations_to::<HorizontalDirectionObservation>(setup, "P2").len(),
                1
            );
        }
    }

    #[test]
    fn a_vertical_reading_outside_a_full_circle_is_not_imported() {
        for reading in ["400.0", "-10.0"] {
            let result = ok(read_text(records![
                header(),
                instrument33(),
                setup_on_p1(),
                target("1.5"),
                backsight("P2", "0.0", "0.0"),
                shot_at("F1", "P2", "0.0", reading),
            ]));
            assert!(
                warned(&result, 6, "vertical reading outside a full circle"),
                "{reading}"
            );
            let setup = setup_at(&result.project, 0);
            assert!(observations_to::<ZenithAngleObservation>(setup, "P2").is_empty());
            assert_eq!(observations_to::<DistanceObservation>(setup, "P2").len(), 1);
        }
    }

    #[test]
    fn an_observation_or_a_reduced_one_with_no_value_is_skipped_and_names_nothing() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            format!("09F1{}{}", f16("P1"), f16("P2")),
            format!("11KI{}{}", f16("P1"), f16("P3")),
        ]));
        assert_eq!(result.records_skipped, 2);
        assert!(warned(&result, 5, "observation with no readable value"));
        assert!(warned(
            &result,
            6,
            "reduced observation record with no value"
        ));
        // A record that is skipped names no point.
        assert!(unpositioned(&result.project, "P2").is_none());
        assert!(unpositioned(&result.project, "P3").is_none());
    }

    #[test]
    fn a_derived_view_of_a_line_its_setup_never_observes_raw_says_the_shot_is_lost() {
        // A field book set to send the MC view ([SDR] 2.1): writers disagree
        // about what an MC holds, so it is not imported - and the read says the
        // shots are lost, not that they were counted already. An MC of 0004 and
        // a RED of 0003, lines the setup never observes raw, are lost however
        // they stand to its raw observation of 0002; an MC of 0002 is that raw
        // one again.
        let result = ok(read_text(records![
            format!(
                "00NM{}0000{}113111",
                f16("SDR20 V03-05"),
                f16("18-Jan-80 20:34")
            ),
            format!(
                "01NM1{}000000{}00003131{}{}{}",
                field("", 16),
                f16("DT2"),
                f10(""),
                f10(""),
                f10("0.000")
            ),
            format!(
                "02KI0001{}{}{}{}",
                f10("5100.000"),
                f10("200.000"),
                f10("50.000"),
                f10("1.000")
            ),
            "12SC0001002",
            format!(
                "09MC00010004{}{}{}",
                f10("42.500"),
                f10("91.50000"),
                f10("14.00000")
            ), // 5
            format!(
                "11TP00010003{}{}{}",
                f10("14.00000"),
                f10("42.480"),
                f10("-1.112")
            ), // 6
            format!(
                "09F100010002{}{}{}",
                f10("42.500"),
                f10("91.50000"),
                f10("0.00000")
            ), // 7
            format!(
                "09MC00010002{}{}{}",
                f10("42.500"),
                f10("91.50000"),
                f10("14.00000")
            ), // 8
            format!(
                "11TP00010003{}{}{}",
                f10("14.00000"),
                f10("42.480"),
                f10("-1.112")
            ), // 9
        ]));
        assert_eq!(result.records_skipped, 4);
        assert!(warned(
            &result,
            5,
            "from '0001' to '0004', which no raw observation of its setup repeats, is not \
             imported"
        ));
        assert!(warned(&result, 5, "so this shot is lost"));
        assert!(warned(
            &result,
            6,
            "which no raw observation of its setup repeats, is not imported"
        ));
        assert!(warned(&result, 8, "since it would count them twice"));
        assert!(warned(
            &result,
            9,
            "which no raw observation of its setup repeats"
        ));
        assert!(any_not_carried_contains(
            &result,
            "1 corrected (MC) observation(s) that no raw observation of their setup repeats"
        ));
        assert!(any_not_carried_contains(
            &result,
            "2 reduced (RED) observation(s) with distances that no raw observation"
        ));
    }

    #[test]
    fn a_derived_view_is_counted_twice_only_beside_a_raw_observation_of_its_own_line() {
        // The setup on P1 observes P2 raw. An MC to P3 and a RED from P9 have no
        // raw twin, however many other shots the setup holds: each is lost, and
        // said to be - and so are an MC and a RED from P9 to P2: the setup
        // observes P2, but from P1, so theirs is another line. An MC to P2 and a
        // RED from P1 to P2 repeat the raw shot.
        let mc = |from: &str, to: &str, circle: &str| {
            format!(
                "09MC{}{}{}{}{}",
                f16(from),
                f16(to),
                f16("50.0"),
                f16("90.0"),
                f16(circle)
            )
        };
        let red = |from: &str, to: &str, azimuth: &str| {
            format!(
                "11TP{}{}{}{}{}",
                f16(from),
                f16(to),
                f16(azimuth),
                f16("50.0"),
                f16("0.1")
            )
        };
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"), // 5
            shot("F1", "P2", "0.0"),       // 6
            mc("P1", "P3", "45.0"),        // 7
            mc("P1", "P2", "0.0"),         // 8
            red("P9", "P3", "45.0"),       // 9
            red("P1", "P2", "0.0"),        // 10
            mc("P9", "P2", "0.0"),         // 11
            red("P9", "P2", "0.0"),        // 12
        ]));
        assert_eq!(result.records_skipped, 6);
        assert!(warned(
            &result,
            7,
            "from 'P1' to 'P3', which no raw observation of its setup repeats"
        ));
        assert!(warned(&result, 7, "so this shot is lost"));
        assert!(warned(
            &result,
            8,
            "raw observations this setup holds of 'P2'"
        ));
        assert!(warned(&result, 8, "since it would count them twice"));
        assert!(warned(
            &result,
            9,
            "from 'P9' to 'P3', which no raw observation of its setup repeats"
        ));
        assert!(warned(&result, 10, "since it would count them twice"));
        assert!(warned(
            &result,
            11,
            "from 'P9' to 'P2', which no raw observation of its setup repeats"
        ));
        assert!(warned(
            &result,
            12,
            "from 'P9' to 'P2', which no raw observation of its setup repeats"
        ));
        assert!(any_not_carried_contains(
            &result,
            "2 corrected (MC) observation(s)"
        ));
        assert!(any_not_carried_contains(
            &result,
            "2 reduced (RED) observation(s)"
        ));
    }

    #[test]
    fn a_derived_view_read_before_its_raw_twin_is_counted_twice_and_not_lost() {
        // The verdict on an MC or a RED waits for its setup's end: read before
        // the raw observation of its line, it repeats that observation as much
        // as one read after it. Its setup is all of its 02's, so a 07 on a moved
        // circle between the two (a new setup on P1 for the orientation) does
        // not part them. A raw observation of the line from a later 02 is
        // another setup's: an MC still waiting when that 02 comes is lost.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"), // 5
            format!(
                "09MC{}{}{}{}{}",
                f16("P1"),
                f16("P3"),
                f16("50.0"),
                f16("90.0"),
                f16("45.0")
            ), // 6
            format!(
                "11TP{}{}{}{}{}",
                f16("P1"),
                f16("P4"),
                f16("45.0"),
                f16("50.0"),
                f16("0.1")
            ), // 7
            shot("F1", "P3", "45.0"),      // 8
            backsight("P2", "0.0", "90.0"), // 9
            shot("F1", "P4", "135.0"),     // 10
            format!(
                "09MC{}{}{}{}{}",
                f16("P1"),
                f16("P5"),
                f16("50.0"),
                f16("90.0"),
                f16("20.0")
            ), // 11
            setup_on_p1(),                 // 12
            target("1.5"),                 // 13
            shot("F1", "P5", "20.0"),      // 14
        ]));
        assert_eq!(result.project.stations.len(), 3);
        assert_eq!(result.records_skipped, 3);
        assert!(warned(
            &result,
            6,
            "raw observations this setup holds of 'P3'"
        ));
        assert!(warned(&result, 6, "since it would count them twice"));
        assert!(warned(
            &result,
            7,
            "raw observations this setup holds of 'P4'"
        ));
        assert!(warned(&result, 7, "since it would count them twice"));
        assert!(warned(
            &result,
            11,
            "from 'P1' to 'P5', which no raw observation of its setup repeats"
        ));
        assert!(warned(&result, 11, "so this shot is lost"));
        assert!(any_not_carried_contains(
            &result,
            "1 corrected (MC) observation(s)"
        ));
        assert!(!any_not_carried_contains(
            &result,
            "reduced (RED) observation(s)"
        ));
    }

    #[test]
    fn a_derived_view_after_a_later_station_record_is_lost_though_an_earlier_one_observed_its_line()
    {
        // P1 observes P2 raw; a second 02 on P1 begins another occupation. An MC
        // of P1 -> P2 after it has no raw twin in its own occupation - the raw
        // shot before the 02 is another observation record, its views sent
        // beside it - so the MC is lost, and said to be.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"), // 5
            shot("F1", "P2", "0.0"),       // 6
            setup_on_p1(),                 // 7
            target("1.5"),                 // 8
            format!(
                "09MC{}{}{}{}{}",
                f16("P1"),
                f16("P2"),
                f16("50.0"),
                f16("90.0"),
                f16("0.0")
            ), // 9
        ]));
        assert!(warned(
            &result,
            9,
            "from 'P1' to 'P2', which no raw observation of its setup repeats"
        ));
        assert!(warned(&result, 9, "so this shot is lost"));
        assert!(!warned(&result, 9, "would count them twice"));
        assert!(any_not_carried_contains(
            &result,
            "1 corrected (MC) observation(s)"
        ));
    }

    // ---- Setups, rounds, sets and backsights ----------------------------------------------

    #[test]
    fn a_backsight_to_another_point_after_shots_begins_a_new_setup() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"),
            shot("F1", "P2", "0.0"),
            shot("F1", "P3", "10.0"),
            backsight("P3", "45.0", "10.0"),
            shot("F1", "P3", "10.0"),
            shot("F1", "P4", "20.0"),
        ]));
        let project = &result.project;
        assert_eq!(project.stations.len(), 2);
        assert_eq!(setup_at(project, 0).backsight_point_id, "P2");
        assert_eq!(setup_at(project, 0).observations.len(), 6);
        // The re-oriented half is a setup of its own on the same point, with
        // the same instrument height: azimuth 45, circle 10 on P3.
        assert_eq!(setup_at(project, 1).setup.id, "P1 (2)");
        assert_eq!(setup_at(project, 1).setup.point_id, "P1");
        double_eq(setup_at(project, 1).setup.instrument_height, 1.5);
        assert_eq!(setup_at(project, 1).backsight_point_id, "P3");
        near(
            value_of(setup_at(project, 1).stated_backsight_azimuth),
            degrees(45.0),
            ANGLE_TOLERANCE,
        );
        near(
            value_of(setup_at(project, 1).backsight_azimuth),
            degrees(10.0),
            ANGLE_TOLERANCE,
        );
        assert_eq!(setup_at(project, 1).observations.len(), 6);
        assert_eq!(setup_at(project, 1).source.record_number, 8);
    }

    #[test]
    fn a_circle_moved_between_rounds_begins_a_new_setup_and_one_within_a_minute_does_not() {
        // Round 2's circle reading on the backsight is 59" from round 1's
        // (0.01638888 degrees): another round. Round 3's is 61" (0.01694444):
        // the circle was moved, so a new setup.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"),
            shot("F1", "P2", "0.0"),
            backsight("P2", "0.0", "0.01638888"),
            shot("F1", "P2", "0.01638888"),
            backsight("P2", "0.0", "0.01694444"),
            shot("F1", "P2", "0.01694444"),
        ]));
        let project = &result.project;
        assert_eq!(project.stations.len(), 2);
        assert_eq!(setup_at(project, 0).metadata["backsight rounds"], "2");
        assert_eq!(setup_at(project, 0).observations.len(), 6);
        assert_eq!(setup_at(project, 1).setup.id, "P1 (2)");
        assert_eq!(setup_at(project, 1).observations.len(), 3);
    }

    #[test]
    fn a_second_backsight_before_any_shot_replaces_the_first() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"),
            backsight("P3", "90.0", "0.0"),
            shot("F1", "P3", "0.0"),
        ]));
        assert_eq!(result.project.stations.len(), 1);
        assert_eq!(setup_at(&result.project, 0).backsight_point_id, "P3");
        near(
            value_of(setup_at(&result.project, 0).stated_backsight_azimuth),
            degrees(90.0),
            ANGLE_TOLERANCE,
        );
        near(
            value_of(setup_at(&result.project, 0).backsight_azimuth),
            0.0,
            ANGLE_TOLERANCE,
        );
        assert!(warned(
            &result,
            6,
            "before any observation on the first (record 5) replaces it"
        ));
    }

    #[test]
    fn a_backsight_record_closing_its_set_orients_that_set() {
        // Sokkia's Set Collection writes each set's back-bearing (07 SC) after
        // the set ([SDR] chapter 2). Set 2 is observed with the circle moved 90
        // degrees; its own 07 says so, and its observations go with it into a
        // setup of their own. From CP1 (N 5000000, E 500000) oriented on CP2
        // due north, T1 is 50 m due east in both sets: at circle 90 in set 1
        // (CP2 at 0) and at 180 in set 2 (CP2 at 90).
        let obs = |face: &str, to: &str, distance: &str, zenith: &str, circle: &str| {
            format!(
                "09{face}{}{}{}{}{}",
                f16("CP1"),
                f16(to),
                f16(distance),
                f16(zenith),
                f16(circle)
            )
        };
        let set = |number: &str| format!("12SC{}  4{number}111", f16("CP1"));
        let closing = |circle: &str| {
            format!(
                "07SC{}{}{}{}",
                f16("CP1"),
                f16("CP2"),
                f16("0.0"),
                f16(circle)
            )
        };
        let result = ok(read_text(records![
            header(),
            instrument33(),
            format!(
                "08KI{}{}{}{}{}",
                f16("CP2"),
                f16("5000100.000"),
                f16("500000.000"),
                f16("10.000"),
                f16("")
            ),
            format!(
                "02KI{}{}{}{}{}",
                f16("CP1"),
                f16("5000000.000"),
                f16("500000.000"),
                f16("10.000"),
                f16("1.500")
            ),
            target("1.500"), // 5
            set("  1"),      // 6
            obs("F1", "CP2", "100.000", "90.0", "0.0"),
            obs("F1", "T1", "50.000", "90.0", "90.0"),
            obs("F2", "T1", "50.000", "270.0", "270.0"),
            obs("F2", "CP2", "100.000", "270.0", "180.0"),
            closing("0.0"), // 11
            set("  2"),     // 12
            obs("F1", "CP2", "100.000", "90.0", "90.0"),
            obs("F1", "T1", "50.000", "90.0", "180.0"),
            obs("F2", "T1", "50.000", "270.0", "0.0"),
            obs("F2", "CP2", "100.000", "270.0", "270.0"),
            closing("90.0"), // 17
        ]));
        let project = &result.project;
        assert_eq!(project.stations.len(), 2);
        assert_eq!(setup_at(project, 0).observations.len(), 12);
        let second = setup_at(project, 1);
        assert_eq!(second.setup.id, "CP1 (2)");
        assert_eq!(second.source.record_number, 17);
        assert_eq!(second.observations.len(), 12);
        near(
            value_of(second.backsight_azimuth),
            degrees(90.0),
            ANGLE_TOLERANCE,
        );
        assert_eq!(
            second.metadata["set at record 12"],
            "number 2, 4 observation(s)"
        );
        // Its four records are its pointings 1 to 4, in order.
        let mut pointings = BTreeSet::new();
        for direction in observations_to::<HorizontalDirectionObservation>(second, "T1") {
            pointings.insert(direction.pointing.index);
        }
        for direction in observations_to::<HorizontalDirectionObservation>(second, "CP2") {
            pointings.insert(direction.pointing.index);
        }
        assert_eq!(pointings, BTreeSet::from([1, 2, 3, 4]));
        assert_eq!(
            warnings_with(&result, "has no observation of its backsight"),
            0
        );
        let outcome = ok(reduced(project));
        let t1 = computed(&outcome, "T1").expect("T1");
        near(t1.northing, 5000000.0, 1e-6);
        near(t1.easting, 500050.0, 1e-6);
    }

    #[test]
    fn shots_before_a_setups_first_backsight_record_that_none_of_them_observes_are_not_oriented_by_it()
     {
        // [SETX] 8.2: a back-bearing record "orients subsequent observations";
        // 8.2.1: shots taken with no backsight are azimuths. D1, D2 and D3 at
        // circle 30, 120 and 210 come before the 07 (B at azimuth 0, circle 90),
        // so they are azimuths: D1 at N 2000 + 100 cos 30 = 2086.6025404, E
        // 1050; D2 at N 1950, E 1086.6025404; D3 at N 1913.3974596, E 950. After
        // the 07 the circle reads 90 on north: C at circle 180 is azimuth 90, N
        // 2000 E 1100.
        let mut records = on_a();
        records.extend([
            from_a("F1", "D1", "30.0"),
            from_a("F1", "D2", "120.0"),
            from_a("F1", "D3", "210.0"),
            backsight_from_a("NM", "B", "0.0", "90.0"),
            from_a("F1", "B", "90.0"),
            from_a("F1", "C", "180.0"),
        ]);
        let result = ok(read_text(records_text(&records)));
        let project = &result.project;
        assert_eq!(project.stations.len(), 2);
        assert_eq!(setup_at(project, 0).observations.len(), 9);
        assert_eq!(setup_at(project, 1).setup.id, "A (2)");
        assert_eq!(setup_at(project, 1).observations.len(), 6);
        assert!(warned(
            &result,
            10,
            "setup 'A' has 3 observation record(s) before its first backsight record, and none is \
             of its backsight 'B'"
        ));
        assert!(warned(
            &result,
            5,
            "its horizontal readings are taken as azimuths"
        ));
        let outcome = ok(reduced(project));
        let c30 = 100.0 * degrees(30.0).cos();
        expect_at(&outcome, "D1", 2000.0 + c30, 1050.0);
        expect_at(&outcome, "D2", 1950.0, 1000.0 + c30);
        expect_at(&outcome, "D3", 2000.0 - c30, 950.0);
        expect_at(&outcome, "C", 2000.0, 1100.0);
    }

    #[test]
    fn a_first_backsight_record_orients_a_shot_of_its_backsight_on_its_circle_before_it() {
        // B read at circle 90 before the 07 that gives B azimuth 0 and circle
        // 90: the circle read B there as the 07 says, so it was not set anew at
        // the 07, and the shot was read on the orientation it gives - one setup
        // holds everything. C at 180 is azimuth 90: N 2000 E 1100.
        let mut records = on_a();
        records.extend([
            from_a("F1", "B", "90.0"),
            backsight_from_a("TP", "B", "0.0", "90.0"),
            from_a("F1", "C", "180.0"),
        ]);
        let result = ok(read_text(records_text(&records)));
        assert_eq!(result.project.stations.len(), 1);
        assert_eq!(warnings_with(&result, "observation record(s) before"), 0);
        let outcome = ok(reduced(&result.project));
        expect_at(&outcome, "C", 2000.0, 1100.0);
    }

    #[test]
    fn a_first_backsight_record_orients_no_shots_before_it_that_read_its_backsight_on_another_circle()
     {
        // The 07 gives B azimuth 0 and circle 0; after it B reads 0 and C 90, so
        // C is at azimuth 90: N 2000 E 1100. Before it, B read at 90 says the
        // circle was set anew at the 07 ([SETX] 8.2 has it orient what
        // follows): those shots stay a setup of their own, as they do where one
        // reading of B before it is on its circle and another is not. B read at
        // 0 on face 1, or 180 on face 2 - the same line, less half a circle -
        // and the circle was not set anew: one setup holds everything, its
        // readings on B all 0, and C is at azimuth 90 either way. (Oriented on a
        // mean with the stray 90 in it, C would be at 45 or 60.)
        let face_two_on_b = format!(
            "09F2{}{}{}{}{}",
            f16("A"),
            f16("B"),
            f16("100.000"),
            f16("270.0"),
            f16("180.0")
        );
        let cases: [(Vec<String>, usize); 4] = [
            (vec![from_a("F1", "B", "90.0")], 2),
            (vec![from_a("F1", "B", "90.0"), from_a("F1", "B", "0.0")], 2),
            (vec![from_a("F1", "B", "0.0")], 1),
            (vec![from_a("F1", "B", "0.0"), face_two_on_b.clone()], 1),
        ];
        for (index, (before, setups)) in cases.into_iter().enumerate() {
            let mut records = on_a();
            records.extend(before);
            let backsight_record = records.len() + 1;
            records.push(backsight_from_a("NM", "B", "0.0", "0.0"));
            records.push(from_a("F1", "B", "0.0"));
            records.push(from_a("F1", "C", "90.0"));
            let result = ok(read_text(records_text(&records)));
            assert_eq!(result.project.stations.len(), setups, "case {index}");
            assert_eq!(
                warned(
                    &result,
                    backsight_record,
                    "more than a minute of arc from this record's circle reading on it: the \
                     circle was set anew"
                ),
                setups == 2,
                "case {index}"
            );
            let outcome = ok(reduced(&result.project));
            expect_at(&outcome, "C", 2000.0, 1100.0);
        }
    }

    #[test]
    fn a_shot_of_the_backsight_with_no_horizontal_reading_shows_no_circle_to_a_first_backsight_record()
     {
        // B shot before the setup's first 07 with a distance and a zenith and
        // no horizontal reading: nothing shows it read on the 07's circle, so it
        // stays a setup of its own and the warning says why. The 07 (B at
        // azimuth 0, circle 0) orients what follows: C at circle 90 is azimuth
        // 90, N 2000 E 1100.
        let mut records = on_a();
        records.extend([
            format!(
                "09F1{}{}{}{}{}",
                f16("A"),
                f16("B"),
                f16("100.000"),
                f16("90.0"),
                f16("")
            ),
            backsight_from_a("NM", "B", "0.0", "0.0"),
            from_a("F1", "B", "0.0"),
            from_a("F1", "C", "90.0"),
        ]);
        let result = ok(read_text(records_text(&records)));
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(setup_at(&result.project, 0).observations.len(), 2);
        assert!(warned(
            &result,
            8,
            "setup 'A' has 1 observation record(s) before its first backsight record, and none \
             gives a horizontal reading of its backsight 'B'"
        ));
        let outcome = ok(reduced(&result.project));
        expect_at(&outcome, "C", 2000.0, 1100.0);
    }

    #[test]
    fn a_collimation_correction_does_not_take_a_reading_off_its_backsight_records_circle() {
        // Hc 0.02 degrees (72") adds 72" to every face 1 reading and takes it
        // off face 2. B is read before the setup's first 07, whose circle is 0:
        // (a) at raw 0, the circle a writer that puts the raw face 1 reading in
        // the 07 gives: corrected, 0.02 - 72" off it, and on it all the same,
        // the minute widened by the correction; (b) at raw 359.98 and 180.02,
        // an instrument 72" out of collimation, and a circle that is the field
        // book's face mean: corrected, 0 and 180, on it with no widening; (c) at
        // raw 45, a circle moved: 45 degrees off it, which still shows. C is at
        // azimuth 90 each time (it and B carry the same correction), N 2000 E
        // 1100.
        let face_two_on_b = format!(
            "09F2{}{}{}{}{}",
            f16("A"),
            f16("B"),
            f16("100.000"),
            f16("270.0"),
            f16("180.02")
        );
        let cases: [(Vec<String>, &str, &str, usize); 3] = [
            (vec![from_a("F1", "B", "0.0")], "0.0", "90.0", 1),
            (
                vec![from_a("F1", "B", "359.98"), face_two_on_b.clone()],
                "359.98",
                "89.98",
                1,
            ),
            (vec![from_a("F1", "B", "45.0")], "0.0", "90.0", 2),
        ];
        for (index, (before, after_on_b, after_on_c, setups)) in cases.into_iter().enumerate() {
            let mut records = on_a();
            records.push(format!("04CL{}{}", f16("0.0"), f16("0.02")));
            records.extend(before);
            let backsight_record = records.len() + 1;
            records.push(backsight_from_a("NM", "B", "0.0", "0.0"));
            records.push(from_a("F1", "B", after_on_b));
            records.push(from_a("F1", "C", after_on_c));
            let result = ok(read_text(records_text(&records)));
            assert_eq!(result.project.stations.len(), setups, "case {index}");
            assert_eq!(
                warned(&result, backsight_record, "the circle was set anew"),
                setups == 2,
                "case {index}"
            );
            let outcome = ok(reduced(&result.project));
            expect_at(&outcome, "C", 2000.0, 1100.0);
        }
    }

    #[test]
    fn a_shot_of_the_backsight_on_a_moved_circle_goes_with_the_backsight_record_after_it() {
        // Each round's shot of B (azimuth 0) before its 07, the circle moved 45
        // degrees for the second: B at 90, 07 circle 90, C at 180; B at 135, 07
        // circle 135, C at 225. B read at 135 is on the second circle, not the
        // first (90): it was read after the circle moved, so it goes with the
        // 07 after it, and each setup reads B on its own circle. C is at 180 -
        // 90 = 225 - 135 = 90 in each: N 2000 E 1100. (Left in the first, B's
        // 90 and 135 mean 112.5, and C would be at 67.5.) The same readings with
        // each 07 before its round read the same.
        for shot_first in [true, false] {
            let mut records = on_a();
            for (circle, on_c) in [("90.0", "180.0"), ("135.0", "225.0")] {
                if shot_first {
                    records.push(from_a("F1", "B", circle));
                    records.push(backsight_from_a("TP", "B", "0.0", circle));
                } else {
                    records.push(backsight_from_a("TP", "B", "0.0", circle));
                    records.push(from_a("F1", "B", circle));
                }
                records.push(from_a("F1", "C", on_c));
            }
            let result = ok(read_text(records_text(&records)));
            assert_eq!(result.project.stations.len(), 2, "{shot_first}");
            assert_eq!(setup_at(&result.project, 0).observations.len(), 6);
            assert_eq!(setup_at(&result.project, 1).observations.len(), 6);
            assert_eq!(
                warnings_with(&result, "has no observation of its backsight"),
                0
            );
            let outcome = ok(reduced(&result.project));
            expect_at(&outcome, "C", 2000.0, 1100.0);
        }
        // A round that ends on face 2 of B stays with its own circle, even when
        // the next is moved half a circle: B's face 2 reading 270 is 90 on face
        // 1, the first circle's, not the second's 270. The second setup reads C
        // at 0 on a circle reading 270 on B: azimuth 90 again.
        let mut records = on_a();
        records.extend([
            backsight_from_a("NM", "B", "0.0", "90.0"),
            from_a("F1", "B", "90.0"),
            from_a("F1", "C", "180.0"),
            format!(
                "09F2{}{}{}{}{}",
                f16("A"),
                f16("C"),
                f16("100.000"),
                f16("270.0"),
                f16("0.0")
            ),
            format!(
                "09F2{}{}{}{}{}",
                f16("A"),
                f16("B"),
                f16("100.000"),
                f16("270.0"),
                f16("270.0")
            ),
            backsight_from_a("NM", "B", "0.0", "270.0"),
            from_a("F1", "B", "270.0"),
            from_a("F1", "C", "0.0"),
        ]);
        let result = ok(read_text(records_text(&records)));
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(setup_at(&result.project, 0).observations.len(), 12);
        assert_eq!(setup_at(&result.project, 1).observations.len(), 6);
        let outcome = ok(reduced(&result.project));
        expect_at(&outcome, "C", 2000.0, 1100.0);
    }

    #[test]
    fn a_shot_of_the_backsight_on_a_moved_circle_moves_when_a_backsight_record_gives_no_circle() {
        // The circle moved 45 degrees between two rounds on B (azimuth 0), C at
        // azimuth 90 in each: N 2000 E 1100. Where the setup's 07 gives no
        // circle, its first reading of B (90) stands for it, and B read at 135
        // before the next 07 (circle 135) is on that one's circle and not on
        // 90, so it moves - B's first shot after that 07 or before it alike.
        // Where the next 07 gives none, B read at 45 before it is off the
        // setup's circle (0), which is enough. Left behind, B's two readings
        // mean 22.5 degrees off the first round's, and C lands at azimuth 67.5,
        // 2 x 100 x sin 11.25 = 39.0 m off.
        let cases: [(&str, Vec<String>); 3] = [
            (
                "first 07 without a circle",
                vec![
                    backsight_from_a("NM", "B", "0.0", ""),
                    from_a("F1", "B", "90.0"),
                    from_a("F1", "C", "180.0"),
                    from_a("F1", "B", "135.0"),
                    backsight_from_a("NM", "B", "0.0", "135.0"),
                    from_a("F1", "C", "225.0"),
                ],
            ),
            (
                "first 07 without a circle, after its backsight shot",
                vec![
                    from_a("F1", "B", "90.0"),
                    backsight_from_a("TP", "B", "0.0", ""),
                    from_a("F1", "C", "180.0"),
                    from_a("F1", "B", "135.0"),
                    backsight_from_a("TP", "B", "0.0", "135.0"),
                    from_a("F1", "C", "225.0"),
                ],
            ),
            (
                "next 07 without a circle",
                vec![
                    backsight_from_a("NM", "B", "0.0", "0.0"),
                    from_a("F1", "B", "0.0"),
                    from_a("F1", "C", "90.0"),
                    from_a("F1", "B", "45.0"),
                    backsight_from_a("NM", "B", "0.0", ""),
                    from_a("F1", "C", "135.0"),
                ],
            ),
        ];
        for (name, case_records) in cases {
            let mut records = on_a();
            records.extend(case_records);
            let result = ok(read_text(records_text(&records)));
            assert_eq!(result.project.stations.len(), 2, "{name}");
            assert_eq!(setup_at(&result.project, 0).observations.len(), 6, "{name}");
            assert_eq!(setup_at(&result.project, 1).observations.len(), 6, "{name}");
            assert_eq!(
                warnings_with(&result, "has no observation of its backsight"),
                0,
                "{name}"
            );
            let outcome = ok(reduced(&result.project));
            expect_at(&outcome, "C", 2000.0, 1100.0);
        }
        // Every shot since the setup's 07 (circle 0) is of B, at 45: the next
        // 07, which gives no circle, replaces it, and says the shots read B off
        // the first one's circle. C at 135 is azimuth 135 - 45 = 90 again.
        let mut records = on_a();
        records.extend([
            backsight_from_a("NM", "B", "0.0", "0.0"),
            from_a("F1", "B", "45.0"),
            backsight_from_a("NM", "B", "0.0", ""),
            from_a("F1", "C", "135.0"),
        ]);
        let result = ok(read_text(records_text(&records)));
        assert_eq!(result.project.stations.len(), 1);
        assert!(warned(
            &result,
            9,
            "this backsight record replaces the one at record 7: every observation since that \
             one reads its backsight 'B' off that one's circle, so the circle was set anew before \
             them"
        ));
        let outcome = ok(reduced(&result.project));
        expect_at(&outcome, "C", 2000.0, 1100.0);
    }

    #[test]
    fn rounds_whose_backsight_records_give_no_circle_say_when_their_readings_of_the_backsight_spread()
     {
        // Two rounds, each opened by a 07 to B at azimuth 0 with no circle
        // reading: one setup, since nothing tells their circles apart. B read at
        // 90 in the first and 135 in the second, on face 1 both, is 45 degrees
        // apart - the circle moved - and the read says so at the second. B at
        // 90 in both says nothing; nor does B at 90 on face 1 and 270.02 on
        // face 2, 72" apart as face 1 reads them, which is an instrument's
        // collimation, not the circle moving.
        let rounds = |first: Vec<String>, second: Vec<String>| {
            let mut records = on_a();
            records.push(backsight_from_a("NM", "B", "0.0", ""));
            records.extend(first);
            records.push(backsight_from_a("NM", "B", "0.0", ""));
            records.extend(second);
            read_text(records_text(&records))
        };
        let face_two_on_b = format!(
            "09F2{}{}{}{}{}",
            f16("A"),
            f16("B"),
            f16("100.000"),
            f16("270.0"),
            f16("270.02")
        );
        let moved = ok(rounds(
            vec![from_a("F1", "B", "90.0"), from_a("F1", "C", "180.0")],
            vec![from_a("F1", "B", "135.0"), from_a("F1", "C", "225.0")],
        ));
        assert_eq!(moved.project.stations.len(), 1);
        assert!(warned(
            &moved,
            11,
            "setup 'A' reads its backsight 'B' here more than a minute of arc from its reading \
             at record 8 on the same face, and its backsight records give no circle reading"
        ));
        let same = ok(rounds(
            vec![from_a("F1", "B", "90.0"), from_a("F1", "C", "180.0")],
            vec![from_a("F1", "B", "90.0"), from_a("F1", "C", "180.0")],
        ));
        assert_eq!(warnings_with(&same, "reads its backsight"), 0);
        let faces = ok(rounds(
            vec![
                from_a("F1", "B", "90.0"),
                face_two_on_b,
                from_a("F1", "C", "180.0"),
            ],
            vec![from_a("F1", "B", "90.0"), from_a("F1", "C", "180.0")],
        ));
        assert_eq!(warnings_with(&faces, "reads its backsight"), 0);
    }

    #[test]
    fn a_setups_first_backsight_record_leaves_a_keyed_azimuth_to_orient_the_shots_before_it() {
        // An 11 keys the azimuth A -> B, 60. B at circle 20 (80 m) and D1 at 50
        // come before the setup's first 07, which names E: the keyed azimuth
        // orients them, 60 - 20 = 40, so B is at azimuth 60 - N 2040, E 1000 +
        // 80 sin 60 = 1069.2820323 - and D1 at 90, N 2000 E 1100. The 07 (E at
        // azimuth 0, circle 90, E never observed and unplaced) orients C at
        // circle 225 on 0 - 90: azimuth 135, N 2000 - 70.7106781, E 1070.7106781.
        let mut records = on_a();
        let before_last = records.len() - 1;
        records.insert(
            before_last,
            format!("11KI{}{}{}", f16("A"), f16("B"), f16("60.0")),
        );
        records.extend([
            from_a_at("F1", "B", "20.0", "80.000"),
            from_a("F1", "D1", "50.0"),
            backsight_from_a("NM", "E", "0.0", "90.0"),
            from_a("F1", "C", "225.0"),
        ]);
        // Here B is not keyed: the keyed azimuth, not its coordinates, orients.
        let _ = records.remove(3);
        let result = ok(read_text(records_text(&records)));
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(
            setup_at(&result.project, 0).metadata["oriented by"],
            "the azimuth to 'B' keyed at record 5"
        );
        let outcome = ok(reduced(&result.project));
        let s60 = 80.0 * degrees(60.0).sin();
        let r45 = 100.0 * degrees(45.0).cos();
        expect_at(&outcome, "B", 2040.0, 1000.0 + s60);
        expect_at(&outcome, "D1", 2000.0, 1100.0);
        expect_at(&outcome, "C", 2000.0 - r45, 1000.0 + r45);
    }

    #[test]
    fn a_sets_closing_backsight_record_does_not_orient_the_shots_before_the_set() {
        // D1 at circle 30, then a set: B at 90 and C at 180, closed by its 07 SC
        // (B at azimuth 0, circle 90). The 07 SC orients the set, not D1, which
        // no backsight record orients: D1 is an azimuth, N 2086.6025404 E 1050;
        // C is at azimuth 90, N 2000 E 1100.
        let mut records = on_a();
        records.extend([
            from_a("F1", "D1", "30.0"),
            format!("12SC{}  2  1111", f16("A")),
            from_a("F1", "B", "90.0"),
            from_a("F1", "C", "180.0"),
            backsight_from_a("SC", "B", "0.0", "90.0"),
        ]);
        let result = ok(read_text(records_text(&records)));
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(setup_at(&result.project, 0).observations.len(), 3);
        assert_eq!(setup_at(&result.project, 1).observations.len(), 6);
        assert!(warned(
            &result,
            11,
            "setup 'A' has 1 observation record(s) before the set this backsight record closes \
             (record 8)"
        ));
        let outcome = ok(reduced(&result.project));
        expect_at(&outcome, "D1", 2000.0 + 100.0 * degrees(30.0).cos(), 1050.0);
        expect_at(&outcome, "C", 2000.0, 1100.0);
    }

    #[test]
    fn a_keyed_azimuth_that_only_a_set_observes_does_not_orient_the_shots_before_the_set() {
        // An 11 keys A -> B at azimuth 0, then D1 at circle 30, then a set (B at
        // 90, C at 180) closed by its 07 SC (B at azimuth 0, circle 90). The set
        // goes to a setup of its own, and its readings of B with it: D1's setup
        // is left with the keyed azimuth to B and no reading of B, so nothing
        // orients it, and the read says so at the 11. C is at 180 - 90 = 90: N
        // 2000 E 1100.
        let mut records = on_a();
        records.extend([
            format!("11KI{}{}{}", f16("A"), f16("B"), f16("0.0")),
            from_a("F1", "D1", "30.0"),
            format!("12SC{}  2  1111", f16("A")),
            from_a("F1", "B", "90.0"),
            from_a("F1", "C", "180.0"),
            backsight_from_a("SC", "B", "0.0", "90.0"),
        ]);
        let result = ok(read_text(records_text(&records)));
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(setup_at(&result.project, 0).observations.len(), 3);
        assert!(warned(
            &result,
            7,
            "has no backsight record, and it does not observe 'B', the point of the azimuth \
             keyed here; the reduction cannot orient it"
        ));
        let outcome = ok(reduced(&result.project));
        expect_at(&outcome, "C", 2000.0, 1100.0);
    }

    #[test]
    fn only_set_collections_backsight_record_closes_a_set_and_any_other_orients_what_follows() {
        // Rounds each opened by a 07 NM and a 12: C at circle 90 on the first (B
        // at azimuth 0, circle 0), D at 225 on the second (circle 45). The
        // second 07 is not Set Collection's (SC), so it orients D and not C: C
        // at azimuth 90, N 2000 E 1100; D at 225 - 45 = 180, N 1900 E 1000.
        let mut records = on_a();
        records.extend([
            backsight_from_a("NM", "B", "0.0", "0.0"),
            format!("12SC{}  1  1111", f16("A")),
            from_a("F1", "C", "90.0"),
            backsight_from_a("NM", "B", "0.0", "45.0"),
            format!("12SC{}  1  2111", f16("A")),
            from_a("F1", "D", "225.0"),
        ]);
        let result = ok(read_text(records_text(&records)));
        assert_eq!(result.project.stations.len(), 2);
        assert_eq!(setup_at(&result.project, 0).observations.len(), 3);
        assert_eq!(warnings_with(&result, "replaces"), 0);
        let outcome = ok(reduced(&result.project));
        expect_at(&outcome, "C", 2000.0, 1100.0);
        expect_at(&outcome, "D", 1900.0, 1000.0);
    }

    #[test]
    fn a_sets_closing_backsight_record_replaces_one_that_oriented_nothing_but_the_set() {
        // A 07 NM, then a set with nothing between them, closed by a 07 SC whose
        // circle is 10 degrees on: the set was observed on the 07 SC's circle,
        // and the first oriented no other shot, so the second replaces it.
        let mut records = on_a();
        records.extend([
            backsight_from_a("NM", "B", "0.0", "0.0"),
            format!("12SC{}  2  1111", f16("A")),
            from_a("F1", "B", "10.0"),
            from_a("F1", "C", "100.0"),
            backsight_from_a("SC", "B", "0.0", "10.0"),
        ]);
        let result = ok(read_text(records_text(&records)));
        assert_eq!(result.project.stations.len(), 1);
        assert!(warned(
            &result,
            11,
            "this backsight record, closing the set of record 8, replaces the one at record 7, \
             since no observation came between that one and the set"
        ));
        near(
            value_of(setup_at(&result.project, 0).backsight_azimuth),
            degrees(10.0),
            ANGLE_TOLERANCE,
        );
    }

    #[test]
    fn an_sdr2x_set_record_after_its_observations_closes_them_and_their_backsight_record_orients_them()
     {
        // [SDR] chapter 2: from V04-02 on, a 4-digit-point job writes each set's
        // SET record after its raw observations, before its MC records, and Set
        // Collection its back-bearing (07 SC) after those. Set 1 on circle 0 on
        // B: C (0003) at circle 90 is azimuth 90, N 1000 E 1100. Set 2 on circle
        // 45: D (0004) at circle 225 is azimuth 225 - 45 = 180, N 900 E 1000.
        // Each set is its own setup, oriented on its own circle.
        let mut records = sdr2x_on_a();
        records.extend([
            String::from("13SCSet #: 1"),
            sdr2x_shot("F1", "0002", "90.0", "0.0"),
            sdr2x_shot("F1", "0003", "90.0", "90.0"),
            sdr2x_shot("F2", "0003", "270.0", "270.0"),
            sdr2x_shot("F2", "0002", "270.0", "180.0"),
            String::from("12SC0001004"),
            format!("09MC00010002{}{}{}", f10("100.0"), f10("90.0"), f10("0.0")),
            format!("09MC00010003{}{}{}", f10("100.0"), f10("90.0"), f10("90.0")),
            format!("07SC00010002{}{}", f10("0.0"), f10("0.0")),
            String::from("13SCSet #: 2"),
            sdr2x_shot("F1", "0002", "90.0", "45.0"),
            sdr2x_shot("F1", "0004", "90.0", "225.0"),
            sdr2x_shot("F2", "0004", "270.0", "45.0"),
            sdr2x_shot("F2", "0002", "270.0", "225.0"),
            String::from("12SC0001004"),
            format!("09MC00010002{}{}{}", f10("100.0"), f10("90.0"), f10("0.0")),
            format!(
                "09MC00010004{}{}{}",
                f10("100.0"),
                f10("90.0"),
                f10("180.0")
            ),
            format!("07SC00010002{}{}", f10("0.0"), f10("45.0")),
        ]);
        let result = ok(read_text(records_text(&records)));
        let project = &result.project;
        assert_eq!(project.stations.len(), 2);
        assert_eq!(setup_at(project, 0).observations.len(), 12);
        let second = setup_at(project, 1);
        assert_eq!(second.setup.id, "0001 (2)");
        assert_eq!(second.observations.len(), 12);
        near(
            value_of(second.backsight_azimuth),
            degrees(45.0),
            ANGLE_TOLERANCE,
        );
        assert_eq!(
            second.metadata["set at record 22"],
            "004 observation(s), closing the 4 before it"
        );
        assert_eq!(
            warnings_with(&result, "has no observation of its backsight"),
            0
        );
        assert_eq!(warnings_with(&result, "since it would count them twice"), 4);
        let outcome = ok(reduced(project));
        expect_at(&outcome, "0003", 1000.0, 1100.0);
        expect_at(&outcome, "0004", 900.0, 1000.0);
    }

    #[test]
    fn an_sdr2x_set_record_closes_no_more_than_its_count_or_what_follows_a_set_note() {
        // A 07 TP (B at azimuth 0, circle 0) orients E at circle 30: azimuth 30,
        // N 1000 + 100 cos 30 = 1086.6025404, E 1050. Then a set on circle 45 -
        // C at 135, azimuth 90, N 1000 E 1100 - closed by a 12 after it and a
        // 07 SC. The set is the 12's count, 4, of the 5 records since the 07
        // TP; or, where the 12 states no count, what follows the field book's
        // "Set #" note. Either way E keeps the first orientation.
        for counted in [true, false] {
            let mut records = sdr2x_on_a();
            records.push(format!("07TP00010002{}{}", f10("0.0"), f10("0.0")));
            records.push(sdr2x_shot("F1", "0005", "90.0", "30.0"));
            if !counted {
                records.push(String::from("13SCSet #: 1"));
            }
            records.extend([
                sdr2x_shot("F1", "0002", "90.0", "45.0"),
                sdr2x_shot("F1", "0003", "90.0", "135.0"),
                sdr2x_shot("F2", "0003", "270.0", "315.0"),
                sdr2x_shot("F2", "0002", "270.0", "225.0"),
                String::from(if counted {
                    "12SC0001004"
                } else {
                    "12SC0001   "
                }),
                format!("07SC00010002{}{}", f10("0.0"), f10("45.0")),
            ]);
            let result = ok(read_text(records_text(&records)));
            assert_eq!(result.project.stations.len(), 2, "{counted}");
            assert_eq!(setup_at(&result.project, 0).observations.len(), 3);
            assert_eq!(setup_at(&result.project, 1).observations.len(), 12);
            let outcome = ok(reduced(&result.project));
            expect_at(
                &outcome,
                "0005",
                1000.0 + 100.0 * degrees(30.0).cos(),
                1050.0,
            );
            expect_at(&outcome, "0003", 1000.0, 1100.0);
        }
    }

    #[test]
    fn an_sdr2x_set_record_written_before_its_set_opens_it_as_the_earlier_field_book_wrote_it() {
        // V04-01 wrote an SDR2x set's 12 first, as SDR33 does ([SDR] chapter 2:
        // SET, the raw observations, a note, the MC records, the BKB SC), and
        // the header cannot tell it from V04-02's: every 4-digit job is sent as
        // "SDR20 V03-05". After a 07 NM (0002 at azimuth 0, circle 0) and a side
        // shot of 0005 at circle 30 comes a 12 and a set on circle 45 (0003 at
        // circle 135) closed by its 07 SC. The raw observations after the 12
        // are its set, so 0005 keeps the first orientation - azimuth 30, N 1000
        // + 100 cos 30 = 1086.6025404, E 1050 - and 0003 is at azimuth 135 - 45
        // = 90, N 1000 E 1100. With the 12's count, without one, and with the
        // note and the MC records between the set and its 07 SC alike.
        for form in 0..3 {
            let mut records = sdr2x_on_a();
            records.push(format!("07NM00010002{}{}", f10("0.0"), f10("0.0")));
            records.push(sdr2x_shot("F1", "0005", "90.0", "30.0"));
            records.push(String::from(if form == 1 {
                "12SC0001   "
            } else {
                "12SC0001004"
            }));
            records.extend([
                sdr2x_shot("F1", "0002", "90.0", "45.0"),
                sdr2x_shot("F1", "0003", "90.0", "135.0"),
                sdr2x_shot("F2", "0003", "270.0", "315.0"),
                sdr2x_shot("F2", "0002", "270.0", "225.0"),
            ]);
            if form == 2 {
                records.push(String::from(
                    "13SCThe following MCs are derived from set(s) 1.",
                ));
                records.push(format!(
                    "09MC00010002{}{}{}",
                    f10("100.0"),
                    f10("90.0"),
                    f10("0.0")
                ));
                records.push(format!(
                    "09MC00010003{}{}{}",
                    f10("100.0"),
                    f10("90.0"),
                    f10("90.0")
                ));
            }
            records.push(format!("07SC00010002{}{}", f10("0.0"), f10("45.0")));
            let result = ok(read_text(records_text(&records)));
            assert_eq!(result.project.stations.len(), 2, "form {form}");
            assert_eq!(setup_at(&result.project, 0).observations.len(), 3);
            assert_eq!(setup_at(&result.project, 1).observations.len(), 12);
            assert_eq!(warnings_with(&result, "replaces the one at record"), 0);
            let outcome = ok(reduced(&result.project));
            expect_at(
                &outcome,
                "0005",
                1000.0 + 100.0 * degrees(30.0).cos(),
                1050.0,
            );
            expect_at(&outcome, "0003", 1000.0, 1100.0);
        }
    }

    #[test]
    fn a_bad_set_ends_after_its_count_of_observations() {
        // [SDR] 3.6.2 SET: source point, count, set number, bad marker (2 is
        // "Bad set", 3.5), return sight, prompt order. Its two observations are
        // not used; the shot after them is.
        let with_count = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"),
            format!("12SC{}002001211", f16("P1")),
            shot("F1", "P2", "0.0"),
            shot("F2", "P2", "180.0"),
            shot("F1", "P3", "10.0"),
        ]));
        assert_eq!(with_count.records_skipped, 2);
        assert!(warned(
            &with_count,
            6,
            "set 001 at 'P1' is marked bad; its 002 observation(s)"
        ));
        assert_eq!(
            observations_to::<DistanceObservation>(setup_at(&with_count.project, 0), "P3").len(),
            1
        );
        // With no count, the set runs to the next set or setup.
        let without_count = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"),
            format!("12SC{}   001211", f16("P1")),
            shot("F1", "P2", "0.0"),
            shot("F2", "P2", "180.0"),
            shot("F1", "P3", "10.0"),
        ]));
        assert_eq!(without_count.records_skipped, 3);
        assert!(warned(&without_count, 6, "up to the next set or setup"));
        // A bad set that counts none takes none of the shots after it.
        let of_none = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"),
            format!("12SC{}000001211", f16("P1")),
            shot("F1", "P2", "0.0"),
            shot("F2", "P2", "180.0"),
        ]));
        assert_eq!(of_none.records_skipped, 0);
        assert!(warned(&of_none, 6, "is marked bad; its 000 observation(s)"));
    }

    #[test]
    fn the_sdr2x_set_layout_has_no_set_number_and_no_bad_marker() {
        // [SDR] 3.6.1 SET: 5-8 the point, 9-11 the count, and nothing more.
        let result = ok(read_text(records![
            format!(
                "00NM{}0000{}113111",
                f16("SDR20 V03-05"),
                f16("18-Jan-80 20:34")
            ),
            format!("02NM0001{}{}{}{}", f10(""), f10(""), f10(""), f10("1.5")),
            "12SC0001004",
        ]));
        assert_eq!(
            setup_at(&result.project, 0).metadata["set at record 3"],
            "004 observation(s)"
        );
        assert_eq!(warnings_with(&result, "marked bad"), 0);
    }

    #[test]
    fn a_bad_sets_observations_are_skipped_and_a_good_sets_read() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            format!("12SC{}002001211", f16("P1")),
            shot("F1", "P2", "0.0"),
            shot("F2", "P2", "180.0"),
            format!("12SC{}002002111", f16("P1")),
            shot("F1", "P2", "0.0"),
            shot("F2", "P2", "180.0"),
        ]));
        assert_eq!(result.records_skipped, 2);
        assert!(warned(&result, 5, "set 001 at 'P1' is marked bad"));
        assert!(warned(&result, 6, "marks bad"));
        assert!(warned(&result, 7, "marks bad"));
        assert_eq!(setup_at(&result.project, 0).observations.len(), 6);
        let first_read = first_of(&observations_to::<DistanceObservation>(
            setup_at(&result.project, 0),
            "P2",
        ));
        assert_eq!(first_read.source.record_number, 9);
    }

    #[test]
    fn a_setup_that_never_observes_its_backsight_is_oriented_by_its_azimuth_less_its_circle() {
        // [SETX] 29.2.6: A = H + BKB azimuth - BKB h.obs. The 07 gives RO's
        // azimuth 90 and the circle 0 on it; RO is never observed. T1 at circle
        // 0, 100 m level, is azimuth 90: 100 m due east of CP1.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            format!(
                "02KI{}{}{}{}{}",
                f16("P1"),
                f16("5000000.000"),
                f16("500000.000"),
                f16("10.000"),
                f16("1.500")
            ),
            target("1.500"),
            backsight("RO", "90.0", "0.0"),
            format!(
                "09F1{}{}{}{}{}",
                f16("P1"),
                f16("T1"),
                f16("100.000"),
                f16("90.0"),
                f16("0.0")
            ),
        ]));
        assert!(warned(
            &result,
            5,
            "orients it by the backsight record's azimuth less its circle reading"
        ));
        let outcome = ok(reduced(&result.project));
        let t1 = computed(&outcome, "T1").expect("T1");
        near(t1.northing, 5000000.0, 1e-6);
        near(t1.easting, 500100.0, 1e-6);
    }

    #[test]
    fn a_stated_azimuth_orients_a_setup_whose_circle_was_zeroed_on_its_backsight() {
        // 07: azimuth 123 to RO, circle 0 on it; RO observed at 0, T at circle
        // 90, 50 m level. With nothing placing RO, T is at azimuth 123 + 90 - 0
        // = 213: N = 5000000 + 50 cos 213 = 4999958.0664716,
        // E = 500000 + 50 sin 213 = 499972.7680482.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            format!(
                "02KI{}{}{}{}{}",
                f16("P1"),
                f16("5000000.000"),
                f16("500000.000"),
                f16("10.000"),
                f16("1.500")
            ),
            target("1.500"),
            backsight("RO", "123.0", "0.0"),
            format!(
                "09F1{}{}{}{}{}",
                f16("P1"),
                f16("RO"),
                f16("80.000"),
                f16("90.0"),
                f16("0.0")
            ),
            format!(
                "09F1{}{}{}{}{}",
                f16("P1"),
                f16("T"),
                f16("50.000"),
                f16("90.0"),
                f16("90.0")
            ),
        ]));
        let setup = setup_at(&result.project, 0);
        near(
            value_of(setup.stated_backsight_azimuth),
            degrees(123.0),
            ANGLE_TOLERANCE,
        );
        near(value_of(setup.backsight_azimuth), 0.0, ANGLE_TOLERANCE);
        let outcome = ok(reduced(&result.project));
        let t = computed(&outcome, "T").expect("T");
        near(t.northing, 4999958.0664716, 1e-6);
        near(t.easting, 499972.7680482, 1e-6);
    }

    #[test]
    fn a_setup_with_no_backsight_record_is_oriented_by_its_keyed_azimuth() {
        // Sokkia's own example, [SDR] chapter 2, "The same job with
        // observations stored in OBS view", written in the SDR2x records of
        // 3.6.1: the setup has no back-bearing record, only an azimuth keyed to
        // 0101 (23-56'15") and an observation of 0101 at 0-00'00". Its printed
        // positions, the same job in POS view: 1000 at N 745427.696 E 415468.380
        // Elev 27.702, 1001 (the average of its two faces) at N 745215.777 E
        // 415357.357 Elev 22.656 - feet, printed to 0.001, so each within half
        // of that (1000's northing works out 0.00049 ft from its printed value).
        // Angles are in decimal degrees here as the file holds them: 88-54'28"
        // is 88.9077778.
        let result = ok(read_text(records![
            format!(
                "00NM{}0000{}122211",
                f16("SDR20 V03-05"),
                f16("06-Jan-80 00:00")
            ),
            format!("10NM{}", f16("SIMPLE OBS TOPO")),
            "13CPSea level crn: N",
            "13CPAtmos crn: N",
            format!("06NM{}", f10("1.00000000")),
            "13TS05-Jan-80 23:48",
            format!(
                "01NM1{}000000{}00000031{}{}{}",
                field("", 16),
                field("", 16),
                f10(""),
                f10(""),
                f10("0.000")
            ),
            format!(
                "02TP0100{}{}{}{}{}",
                f10("745256.356"),
                f10("415265.958"),
                f10("23.256"),
                f10("5.230"),
                f16("OIT II")
            ),
            format!(
                "11KI01000101{}{}{}{}",
                f10("23.9375000"),
                f10(""),
                f10(""),
                f16("BS AZ")
            ), // 9
            format!("03NM{}", f10("5.840")),
            format!(
                "09F101000101{}{}{}{}",
                f10(""),
                f10(""),
                f10("0.00000000"),
                f16("BS")
            ),
            format!(
                "09F101001000{}{}{}{}",
                f10("265.250"),
                f10("88.9077778"),
                f10("25.8163889"),
                f16("TREE")
            ),
            format!(
                "09F101001001{}{}{}{}",
                f10("100.000"),
                f10("90.0000000"),
                f10("90.0000000"),
                f16("BLDG CNR F1")
            ),
            format!(
                "09F201001001{}{}{}{}",
                f10("100.005"),
                f10("270.011111"),
                f10("270.005556"),
                f16("BLDG CNR F2")
            ),
            format!(
                "09MC01001001{}{}{}{}",
                f10("100.004"),
                f10("90.3438889"),
                f10("113.940278"),
                f16("BLDG CNR AVG")
            ),
        ]));
        let setup = setup_at(&result.project, 0);
        assert_eq!(setup.backsight_point_id, "0101");
        near(
            value_of(setup.stated_backsight_azimuth),
            degrees(23.9375),
            ANGLE_TOLERANCE,
        );
        assert!(setup.backsight_azimuth.is_none());
        assert_eq!(
            setup.metadata["oriented by"],
            "the azimuth to '0101' keyed at record 9"
        );
        assert!(warned(&result, 15, "since it would count them twice"));
        let outcome = ok(reduced(&result.project));
        let foot: f64 = 0.3048;
        let half_printed_unit = 0.0005 * foot + 1e-9;
        let printed = [
            ("1000", 745427.696, 415468.380, 27.702),
            ("1001", 745215.777, 415357.357, 22.656),
        ];
        for (id, northing, easting, elevation) in printed {
            let point = computed(&outcome, id).expect("the point");
            near(point.northing, northing * foot, half_printed_unit);
            near(point.easting, easting * foot, half_printed_unit);
            near(
                value_of(point.elevation),
                elevation * foot,
                half_printed_unit,
            );
        }
    }

    #[test]
    fn a_setup_with_neither_a_backsight_nor_a_keyed_azimuth_takes_its_readings_as_azimuths() {
        // [SETX] 8.2.1: with the backsight skipped, "Horizontal angles stored
        // are treated as azimuths". T at circle 90, 50 m level: due east.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            format!(
                "02KI{}{}{}{}{}",
                f16("P1"),
                f16("5000000.000"),
                f16("500000.000"),
                f16("10.000"),
                f16("1.500")
            ),
            target("1.500"),
            format!(
                "09F1{}{}{}{}{}",
                f16("P1"),
                f16("T"),
                f16("50.000"),
                f16("90.0"),
                f16("90.0")
            ),
        ]));
        assert!(warned(
            &result,
            3,
            "its horizontal readings are taken as azimuths"
        ));
        let setup = setup_at(&result.project, 0);
        assert!(setup.backsight_point_id.is_empty());
        near(value_of(setup.backsight_azimuth), 0.0, ANGLE_TOLERANCE);
        let outcome = ok(reduced(&result.project));
        let t = computed(&outcome, "T").expect("T");
        near(t.northing, 5000000.0, 1e-6);
        near(t.easting, 500050.0, 1e-6);
    }

    #[test]
    fn a_stated_azimuth_is_taken_less_the_setups_reading_on_the_backsight_or_else_less_its_circle()
    {
        // [SDR] chapter 2's traverse, at 0003: "BKB TP 0003-0002 Azimuth
        // 269-59'50" H.obs 270-00'00"" and "OBS F1 0003-0004 S.Dist 100.008
        // V.obs 90-00'10" H.obs 90-00'20"" - in decimal degrees, as the file
        // holds them, azimuth 269.99722222, circle 270, and 90.00277778 and
        // 90.00555556 on 0004. Here 0002 has no coordinates, so nothing but the
        // file's azimuth orients the setup ([SETX] 29.2.6, A = H + BKB azimuth
        // - BKB h.obs). Never observed, 0002's h.obs is the 07's circle: 0004 is
        // at azimuth 90.00555556 + 269.99722222 - 270. Observed at 270.00138889
        // (5" off the circle), the setup's own reading stands in: azimuth
        // 90.00555556 + 269.99722222 - 270.00138889. Feet, 0003 keyed at N
        // 0.005 E 200.005.
        let job = |observed: bool| {
            let mut records = vec![
                format!(
                    "00NM{}0000{}122211",
                    f16("SDR20 V03-05"),
                    f16("06-Jan-80 00:23")
                ),
                format!("10NM{}", f16("TRAVERSE")),
                format!("08KI0003{}{}{}", f10("0.005"), f10("200.005"), f10("0.010")),
                format!(
                    "01NM1{}000000{}00000031{}{}{}",
                    field("", 16),
                    field("", 16),
                    f10(""),
                    f10(""),
                    f10("0.000")
                ),
                format!(
                    "02TP0003{}{}{}{}",
                    f10("0.005"),
                    f10("200.005"),
                    f10("0.010"),
                    f10("0.000")
                ),
                format!("07TP00030002{}{}", f10("269.997222"), f10("270.000000")),
                format!("03NM{}", f10("0.000")),
            ];
            if observed {
                records.push(format!(
                    "09F100030002{}{}{}",
                    f10(""),
                    f10(""),
                    f10("270.001389")
                ));
            }
            records.push(format!(
                "09F100030004{}{}{}",
                f10("100.008"),
                f10("90.002778"),
                f10("90.005556")
            ));
            records_text(&records)
        };
        for observed in [false, true] {
            let result = ok(read_text(job(observed)));
            let outcome = ok(reduced(&result.project));
            let on_backsight = if observed { 270.001389 } else { 270.0 };
            let azimuth = degrees(90.005556 + 269.997222 - on_backsight);
            let horizontal = 100.008 * degrees(90.002778).sin() * 0.3048;
            expect_at(
                &outcome,
                "0004",
                0.005 * 0.3048 + horizontal * azimuth.cos(),
                200.005 * 0.3048 + horizontal * azimuth.sin(),
            );
            near(
                outcome.report.setups[0]
                    .orientation_correction
                    .expect("an orientation correction"),
                degrees(269.997222 - on_backsight),
                1e-12,
            );
        }
    }

    #[test]
    fn a_backsight_record_with_no_circle_on_a_backsight_never_observed_says_only_coordinates_orient_it()
     {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            format!("07NM{}{}{}{}", f16("P1"), f16("RO"), f16("45.0"), f16("")),
            shot("F1", "T", "10.0"),
        ]));
        assert!(warned(
            &result,
            5,
            "has no observation of its backsight 'RO'; the backsight record gives no circle \
             reading on it either"
        ));
        assert!(setup_at(&result.project, 0).backsight_azimuth.is_none());
    }

    #[test]
    fn a_keyed_azimuth_to_a_point_the_setup_never_observes_cannot_orient_it() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            format!("11KI{}{}{}", f16("P1"), f16("RO"), f16("45.0")),
            target("1.5"),
            shot("F1", "T", "10.0"),
        ]));
        assert!(warned(
            &result,
            4,
            "has no backsight record, and it does not observe 'RO', the point of the azimuth \
             keyed here; the reduction cannot orient it"
        ));
        assert_eq!(setup_at(&result.project, 0).backsight_point_id, "RO");
    }

    #[test]
    fn a_setup_that_never_observes_its_backsight_says_so() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"),
            shot("F1", "P3", "10.0"),
        ]));
        assert!(warned(&result, 5, "no observation of its backsight 'P2'"));
    }

    #[test]
    fn a_shot_from_another_point_than_the_setup_is_skipped() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            format!(
                "09F1{}{}{}{}{}",
                f16("P9"),
                f16("P2"),
                f16("50.0"),
                f16("90.0"),
                f16("0.0")
            ),
            format!("07NM{}{}{}{}", f16("P9"), f16("P2"), f16("0.0"), f16("0.0")),
            shot("F1", "P1", "0.0"),
        ]));
        assert_eq!(result.records_skipped, 3);
        assert!(warned(&result, 5, "from 'P9' while the setup is on 'P1'"));
        assert!(warned(&result, 6, "from 'P9' while the setup is on 'P1'"));
        assert!(warned(&result, 7, "to itself"));
        assert!(setup_at(&result.project, 0).observations.is_empty());
    }

    #[test]
    fn every_change_in_the_middle_of_a_setup_is_reported() {
        // An instrument record (a new prism, -30 mm) and then the weather, both
        // between two shots of one setup: each is said, not only the last.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "0.0"),
            shot("F1", "P2", "0.0"),
            format!(
                "01NM:{}000000{}00000131{}{}{}",
                field("", 16),
                f16("HAND"),
                f16(""),
                f16(""),
                f16("-30")
            ), // 7
            format!("05NM{}{}", f16("990.0"), f16("15.0")), // 8
            shot("F1", "P3", "10.0"),
        ]));
        assert!(warned(
            &result,
            7,
            "the instrument changes in the middle of setup 'P1'"
        ));
        assert!(warned(
            &result,
            8,
            "the weather changes in the middle of setup 'P1'"
        ));
        let metadata = &setup_at(&result.project, 0).metadata;
        assert_eq!(
            metadata["the instrument changed at record 7"],
            "not applied to this setup"
        );
        assert_eq!(
            metadata["the weather changed at record 8"],
            "not applied to this setup"
        );
    }

    // ---- Faces and vertical readings --------------------------------------------------------

    #[test]
    fn horizon_vertical_readings_become_zenith_angles() {
        // Vertical angle option 2, "measured upwards from horizontal" ([SDR]
        // 3.5): +3 degrees is the zenith 87 on face 1; 177 is the same line on
        // face 2 (its zenith reading 360 - 87 = 273, less 90 ... 90 - 273 =
        // -183, which is 177).
        let result = ok(read_text(records![
            header(),
            instrument33_with('2'),
            setup_on_p1(),
            target("1.5"),
            backsight("P2", "0.0", "10.0"),
            shot_at("F1", "P2", "10.0", "3.0"),
            shot_at("F2", "P2", "190.0", "177.0"),
        ]));
        let zeniths = observations_to::<ZenithAngleObservation>(setup_at(&result.project, 0), "P2");
        assert_eq!(zeniths.len(), 2);
        near(zeniths[0].angle, degrees(87.0), 1e-12);
        assert_eq!(zeniths[0].pointing.face, Face::Left);
        near(zeniths[1].angle, degrees(87.0), 1e-12);
        assert_eq!(zeniths[1].pointing.face, Face::Right);
        assert_eq!(result.warnings.len(), 0);
    }

    #[test]
    fn a_multiple_distance_observation_takes_its_face_from_its_vertical_reading() {
        // [SETX] 29.2.3: 0 to 180 degrees is face 1, 180 to 360 face 2.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            shot_at("MD", "P2", "10.0", "88.0"),
            shot_at("MD", "P2", "190.0", "272.0"),
        ]));
        let directions =
            observations_to::<HorizontalDirectionObservation>(setup_at(&result.project, 0), "P2");
        assert_eq!(directions.len(), 2);
        assert_eq!(directions[0].pointing.face, Face::Left);
        assert_eq!(directions[1].pointing.face, Face::Right);
    }

    #[test]
    fn a_face_two_record_with_a_face_one_reading_warns_and_keeps_the_records_face() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            shot_at("F2", "P2", "10.0", "88.0"),
        ]));
        assert!(warned(&result, 5, "F2 says face right"));
        let zeniths = observations_to::<ZenithAngleObservation>(setup_at(&result.project, 0), "P2");
        assert_eq!(zeniths.len(), 1);
        assert_eq!(zeniths[0].pointing.face, Face::Right);
        near(zeniths[0].angle, degrees(88.0), ANGLE_TOLERANCE);
    }

    // ---- What is not imported, and why -----------------------------------------------------

    #[test]
    fn derived_views_and_unknown_records_are_skipped_with_their_reason() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            format!(
                "09MC{}{}{}{}{}",
                f16("P1"),
                f16("P2"),
                f16("50.0"),
                f16("90.0"),
                f16("45.0")
            ),
            format!(
                "11TP{}{}{}{}{}",
                f16("P1"),
                f16("P2"),
                f16("45.0"),
                f16("50.0"),
                f16("0.1")
            ),
            format!("25RO{}", f16("P1")),
            format!(
                "09TP{}{}{}{}{}",
                f16("P1"),
                f16("P2"),
                f16("50.0"),
                f16("90.0"),
                f16("45.0")
            ),
            "NOT A RECORD",
            shot("F1", "P2", "45.0"),
        ]));
        assert_eq!(result.records_skipped, 5);
        assert!(warned(&result, 5, "corrected observation (MC)"));
        assert!(warned(&result, 6, "reduced observation"));
        assert!(warned(&result, 7, "25RO (road station)"));
        assert!(warned(&result, 8, "derivation code 'TP'"));
        assert!(warned(&result, 9, "'NOT A RECORD' is not a record"));
        assert_eq!(setup_at(&result.project, 0).observations.len(), 3);
    }

    #[test]
    fn weather_changed_in_the_middle_of_a_setup_warns_and_is_not_applied() {
        let result = ok(read_text(records![
            header(),
            format!("05NM{}{}", f16("1000.0"), f16("10.0")),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            shot("F1", "P2", "0.0"),
            format!("05NM{}{}", f16("990.0"), f16("15.0")),
            shot("F1", "P3", "10.0"),
        ]));
        assert!(warned(
            &result,
            7,
            "the weather changes in the middle of setup 'P1'"
        ));
        let instrument = &setup_at(&result.project, 0).instrument;
        double_eq(value_of(instrument.pressure_hectopascals), 1000.0);
        double_eq(value_of(instrument.temperature_celsius), 10.0);
    }

    #[test]
    fn not_a_number_warns_with_its_record_and_is_read_as_not_measured() {
        let observation = |slope: &str| {
            format!(
                "09F1{}{}{}{}{}",
                f16("P1"),
                f16("P2"),
                f16(slope),
                f16("90.0"),
                f16("0.0")
            )
        };
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            observation("5O.0"),
        ]));
        assert!(warned(&result, 5, "slope distance '5O.0' is not a number"));
        let setup = setup_at(&result.project, 0);
        assert!(observations_to::<DistanceObservation>(setup, "P2").is_empty());
        assert_eq!(
            observations_to::<ZenithAngleObservation>(setup, "P2").len(),
            1
        );
        // [SDR] 3.3 (i) allows a minus, digits and a point: an exponent or a
        // plus sign is not a real of the format, however a C library reads it.
        for text in ["1e3", "+50.0", "5.0.0", "-"] {
            let odd = ok(read_text(records![
                header(),
                instrument33(),
                setup_on_p1(),
                target("1.5"),
                observation(text),
            ]));
            assert!(warned(&odd, 5, "is not a number"), "{text}");
            assert!(
                observations_to::<DistanceObservation>(setup_at(&odd.project, 0), "P2").is_empty(),
                "{text}"
            );
        }
    }

    #[test]
    fn a_shot_before_any_target_height_warns_and_says_so() {
        let result = ok(read_text(records![
            header(),
            instrument33(),
            setup_on_p1(),
            shot("F1", "P2", "0.0")
        ]));
        assert!(warned(&result, 4, "no target height record"));
        assert!(any_not_carried_contains(&result, "no target height"));
        double_eq(
            first_of(&observations_to::<DistanceObservation>(
                setup_at(&result.project, 0),
                "P2",
            ))
            .target_height,
            0.0,
        );
    }

    #[test]
    fn a_file_without_a_header_or_with_nothing_in_it_is_an_error() {
        let cases = [
            (String::new(), "holds no SDR header record"),
            (String::from("\n\n"), "holds no SDR header record"),
            (
                records![format!("10NM{}", f16("J"))],
                "does not begin with an SDR header record",
            ),
            (records![header()], "holds no setup, observation or point"),
            (
                records![header(), format!("10NM{}", f16("J")), "13NMA NOTE"],
                "holds no setup, observation or point",
            ),
        ];
        for (text, words) in cases {
            let error = read_text(&text).expect_err("refused");
            assert_eq!(error.code, ErrorCode::FileImportFailure, "{text:?}");
            assert!(error.message.contains(words), "{}", error.message);
        }
    }

    // ---- Lines, framing, text -------------------------------------------------------------

    #[test]
    fn lf_cr_lf_and_cr_line_ends_read_alike() {
        let lines = records![
            header(),
            instrument33(),
            setup_on_p1(),
            target("1.5"),
            shot("F1", "P2", "0.0"),
        ];
        let lf = ok(read_text(&lines));
        let crlf = ok(read_text(ended(&lines, "\r\n")));
        let cr = ok(read_text(ended(&lines, "\r")));
        assert_eq!(lf.project, crlf.project);
        assert_eq!(lf.project, cr.project);
        assert_eq!(lf.records_read, 5);
    }

    #[test]
    fn transmission_framing_is_not_a_record_and_its_checksum_is_kept() {
        // [SDR] chapter 3: STX CR LF first, ETX and a five-digit checksum last.
        let result = ok(read_text(ended(
            &records!["\x02", header(), setup_on_p1(), "\x0312345"],
            "\r\n",
        )));
        assert_eq!(result.records_read, 2);
        assert_eq!(result.records_skipped, 0);
        assert_eq!(result.project.metadata["transmission checksum"], "12345");
    }

    #[test]
    fn a_last_record_with_no_line_end_is_read_and_said_to_be_perhaps_cut_short() {
        // [SDR] chapter 3 ends every record with CR LF. A transfer cut inside
        // the last record leaves a real that is still a number - 1.6 of 1.65
        // here - so the read says the file may have been cut.
        let mut cut = records![header(), setup_on_p1(), target("1.65")];
        // the line end, the 12 blanks padding the field, and the 5
        cut.truncate(cut.len() - 14);
        assert!(cut.ends_with("03NM1.6"));
        let result = ok(read_text(cut));
        assert_eq!(result.records_read, 3);
        assert!(warned(
            &result,
            3,
            "the file ends inside this record, with no line end after it"
        ));
        // With its line end, or with framing after it, the last record is whole.
        for whole in [
            records![header(), setup_on_p1(), target("1.65")],
            records![header(), setup_on_p1(), target("1.65")] + "\x1a",
            ended(&records![header(), setup_on_p1(), target("1.65")], "\r\n") + "\x0312345",
        ] {
            let read = ok(read_text(whole));
            assert_eq!(warnings_with(&read, "no line end"), 0);
        }
    }

    #[test]
    fn every_deletion_mark_is_stepped_over_and_a_long_run_is_quoted_by_its_length() {
        // A record type is two digits, so every leading D is part of the mark:
        // "DDD" is reported as written, with the record it deletes named, and a
        // run of twenty by its length rather than twenty D's in the reply.
        let record = shot("F1", "P2", "0.0");
        let result = ok(read_text(records![
            header(),
            setup_on_p1(),
            format!("DDD{record}"),
            format!("{}{record}", "D".repeat(20)),
            format!("D{record}"),
        ]));
        assert_eq!(result.records_skipped, 3);
        assert!(warned(
            &result,
            3,
            "a deleted record (marked 'DDD'), 09F1 (observation), is not imported"
        ));
        assert!(warned(
            &result,
            4,
            "a deleted record (marked with a run of 20 D's), 09F1 (observation), is not imported"
        ));
        // One D is no mark: the line is not a record.
        assert!(warned(&result, 5, "is not a record"));
        assert!(setup_at(&result.project, 0).observations.is_empty());
    }

    #[test]
    fn a_dos_end_of_file_mark_is_framing_and_other_control_bytes_are_shown_in_their_warning() {
        // A file copied through MS-DOS ends in Ctrl-Z (0x1A): not a record, and
        // no warning. A line of NULs is not a record either, and its warning
        // shows the bytes rather than blanks nobody can tell from a space.
        let ctrl_z = ok(read_text(records![header(), setup_on_p1()] + "\x1a"));
        assert_eq!(ctrl_z.records_read, 2);
        assert_eq!(ctrl_z.records_skipped, 0);
        assert_eq!(ctrl_z.warnings.len(), 0);
        let nul = ok(read_text(records![header(), setup_on_p1(), "\0\0\0\0"]));
        assert_eq!(nul.records_skipped, 1);
        assert!(warned(&nul, 3, "'\\x00\\x00\\x00\\x00' is not a record"));
    }

    #[test]
    fn time_stamps_in_the_field_books_own_forms_are_read() {
        // [SDR] chapter 4 writes "18-Jan-80 20:14", its V04-04.30 example
        // "May-12-97 10:53"; a two-digit year follows POSIX strptime %y.
        let stamp = |year, month, day, hour, minute, second| SurveyTimestamp {
            year,
            month,
            day,
            hour,
            minute,
            second,
            time_system: String::new(),
        };
        let stamps = [
            ("18-Jan-80 20:14", stamp(1980, 1, 18, 20, 14, 0.0)),
            ("May-12-97 10:53", stamp(1997, 5, 12, 10, 53, 0.0)),
            ("07-Mar-21 13:36", stamp(2021, 3, 7, 13, 36, 0.0)),
            (
                "Time Date 12/31/1999 Time 23:59:58",
                stamp(1999, 12, 31, 23, 59, 58.0),
            ),
            // Leap days: 2024 is divisible by 4, 2000 by 400.
            (
                "Time Date 02/29/2024 Time 10:00:00",
                stamp(2024, 2, 29, 10, 0, 0.0),
            ),
            ("29-Feb-00 10:00", stamp(2000, 2, 29, 10, 0, 0.0)),
        ];
        for (text, expected) in stamps {
            let result = ok(read_text(records![
                header(),
                setup_on_p1(),
                format!("13TS{text}")
            ]));
            assert_eq!(
                setup_at(&result.project, 0).instrument.time,
                expected,
                "{text}"
            );
        }
        let odd = ok(read_text(records![
            header(),
            setup_on_p1(),
            "13TSsometime tomorrow"
        ]));
        assert!(warned(&odd, 3, "time stamp 'sometime tomorrow'"));
        assert!(!setup_at(&odd.project, 0).instrument.time.known());
    }

    #[test]
    fn a_day_that_is_not_in_its_month_dates_nothing() {
        // February has no 31st, and no 29th in 2023 or 1900 (divisible by 100,
        // not by 400): each is kept as a note and dates no setup. Nor is there a
        // day 271 or a month 257, which a calendar keeping a byte of each would
        // take for the 15th and January.
        for text in [
            "Time Date 02/31/2022 Time 10:00:00",
            "31-Feb-99 10:00",
            "Feb-31-99 10:00",
            "Time Date 02/29/2023 Time 10:00:00",
            "Time Date 02/29/1900 Time 10:00:00",
            "31-Apr-21 08:00",
            "Time Date 02/271/2022 Time 10:00:00",
            "Time Date 257/15/2022 Time 10:00:00",
            "271-Feb-22 10:00",
        ] {
            let result = ok(read_text(records![
                header(),
                setup_on_p1(),
                format!("13TS{text}")
            ]));
            assert!(warned(&result, 3, "is not a date and time"), "{text}");
            assert!(
                !setup_at(&result.project, 0).instrument.time.known(),
                "{text}"
            );
        }
    }

    #[test]
    fn a_name_outside_ascii_does_not_shift_the_fields_after_it() {
        // "Pé1" is three characters and, in UTF-8, four bytes: the northing
        // after it still starts in column 21.
        let result = ok(read_text(records![
            header(),
            format!(
                "08KIP\u{e9}{}1{}{}{}",
                " ".repeat(13),
                f16("5000.0"),
                f16("200.0"),
                f16("")
            ),
        ]));
        let point = point(&result.project, "P\u{e9}             1").expect("the point");
        double_eq(point.northing, 5000.0);
        double_eq(point.easting, 200.0);
        assert!(warned(&result, 2, "outside the ASCII"));
    }

    #[test]
    fn a_file_that_is_not_utf8_is_read_as_its_guessed_encoding_and_says_so() {
        // 0xE9 alone is not UTF-8: the file is decoded as a single-byte code
        // page, and the read says it guessed.
        let mut bytes = records![header()].into_bytes();
        bytes.extend_from_slice(b"08KIP\xe9");
        bytes.extend_from_slice(" ".repeat(14).as_bytes());
        bytes.extend_from_slice(
            format!("{}{}{}\n", f16("5000.0"), f16("200.0"), f16("")).as_bytes(),
        );
        let result = ok(read_text(bytes));
        assert!(warned(&result, 0, "the file is not UTF-8"));
        let point = point(&result.project, "P\u{e9}").expect("the point");
        double_eq(point.northing, 5000.0);
    }

    #[test]
    fn short_records_read_their_fields_and_nothing_past_their_end() {
        // [SDR] chapter 5: "trailing blanks may be truncated". The 02 below
        // ends after its instrument height, the 03 after three characters.
        let result = ok(read_text(records![
            header(),
            instrument33(),
            format!("02NM{}{}{}{}1.5", f16("P1"), f16(""), f16(""), f16("")),
            "03NM1.6",
            backsight("P2", "0.0", "0.0"),
            shot("F1", "P2", "0.0"),
        ]));
        double_eq(setup_at(&result.project, 0).setup.instrument_height, 1.5);
        double_eq(
            first_of(&observations_to::<DistanceObservation>(
                setup_at(&result.project, 0),
                "P2",
            ))
            .target_height,
            1.6,
        );
        assert_eq!(result.warnings.len(), 0);
    }

    #[test]
    fn every_truncation_and_random_bytes_come_back_as_an_answer() {
        let bytes = fixture("sdr/traverse.sdr");
        let reads = read_every_truncation_and_noise(ID, &bytes, "traverse.sdr");
        assert!(reads > bytes.len());
    }
}
