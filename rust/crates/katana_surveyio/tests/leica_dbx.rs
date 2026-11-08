//! Twin of tests/surveyio/test_leica_dbx.cpp: Leica DBX (src/leica_dbx.rs),
//! recognised and refused.
//!
//! The fixtures data/leica/dbx/JOB1.XCF and JOB1.X01 are a few made-up binary
//! bytes each - DBX has no published layout, so nothing about their content is
//! meant to resemble one. What is tested is the recognition (by name, binary
//! content and the job's other file beside it) and the sentence a person gets.

mod instrument_support;

use std::path::PathBuf;

use katana_core::ErrorCode;
use katana_surveyio::{
    DetectionOutcome, FormatContent, LEICA_DBX_FORMAT_ID, MAX_SURVEY_FILE_BYTES, Manufacturer,
    PROBE_BYTES, ReadOptions, detect_format, format_registry, probe_of, read_survey,
    siblings_in_folder,
};

use instrument_support::{Random32, data_folder, files_under, slurp};

fn dbx_folder() -> PathBuf {
    data_folder().join("leica").join("dbx")
}

fn dbx_confidence(bytes: &[u8], name: &str) -> f64 {
    for result in format_registry().probe_all(&probe_of(bytes, name, false)) {
        if result.format_id == LEICA_DBX_FORMAT_ID {
            return result.signature.confidence;
        }
    }
    -1.0
}

const EXPORT_ADVICE: &str =
    "export the job from Captivate/Infinity as GSI-16 or LandXML (or HeXML) and import that";

mod leica_dbx {
    use super::*;

    #[test]
    fn the_format_is_registered_as_reading_nothing() {
        let format = format_registry()
            .find(LEICA_DBX_FORMAT_ID)
            .expect("the DBX format is registered");
        assert_eq!(format.manufacturer, Manufacturer::Leica);
        assert_eq!(format.reads, FormatContent::default());
        assert!(format.human_name.contains("export GSI or LandXML"));
        // A reader exists only to give the refusal (see leica_dbx.rs).
        assert!(format_registry().reader(LEICA_DBX_FORMAT_ID).is_some());
    }

    #[test]
    fn a_jobs_index_file_is_identified_as_dbx() {
        let bytes = slurp(dbx_folder().join("JOB1.XCF"));
        assert!(!bytes.is_empty());
        let detection = detect_format(&probe_of(&bytes, "JOB1.XCF", false));
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        assert_eq!(
            detection.format().expect("identified").id,
            LEICA_DBX_FORMAT_ID
        );
        assert!(detection.candidates()[0].evidence.contains("index file"));
        // A data file of the job is recognised as well.
        assert!(dbx_confidence(&slurp(dbx_folder().join("JOB1.X01")), "JOB1.X01") >= 0.7);
    }

    #[test]
    fn a_gimp_image_a_text_file_and_other_extensions_are_not_dbx() {
        assert_eq!(dbx_confidence(b"gimp xcf v011\0\0\0", "picture.xcf"), 0.0);
        assert!(dbx_confidence(b"plain text, not a job\r\n", "notes.xcf") < 0.7);
        assert_eq!(dbx_confidence(b"\0\x01\x02", "job.gsi"), 0.0);
        assert_eq!(dbx_confidence(b"\0\x01\x02", "job.x00"), 0.0);
        assert_eq!(
            dbx_confidence(b"110001+00000001 81..00+00001000 \r\n", "job.gsi"),
            0.0
        );
    }

    #[test]
    fn reading_a_job_is_refused_with_the_export_to_make_instead() {
        let bytes = slurp(dbx_folder().join("JOB1.XCF"));
        let options = ReadOptions {
            siblings: Some(siblings_in_folder(dbx_folder(), MAX_SURVEY_FILE_BYTES)),
            ..Default::default()
        };
        let error = read_survey(
            format_registry(),
            LEICA_DBX_FORMAT_ID,
            &bytes,
            "JOB1.XCF",
            &options,
        )
        .expect_err("a DBX job is refused");
        assert_eq!(error.code, ErrorCode::Unsupported);
        // The data file is found beside the index (asked for as "JOB1.x01"; the
        // folder lookup ignores case), which confirms the file is a job.
        assert!(
            error
                .message
                .contains("This is a Leica DBX job (JOB1.XCF with JOB1.x01"),
            "{}",
            error.message
        );
        assert!(error.message.contains(EXPORT_ADVICE));
    }

    #[test]
    fn a_job_file_on_its_own_is_refused_with_the_same_advice() {
        let error = read_survey(
            format_registry(),
            LEICA_DBX_FORMAT_ID,
            b"\0\x01",
            "LONE.X01",
            &ReadOptions::default(),
        )
        .expect_err("a DBX file is refused");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(
            error
                .message
                .contains("looks like a file of a Leica DBX job")
        );
        assert!(error.message.contains(EXPORT_ADVICE));
    }

    #[test]
    fn no_other_formats_fixture_is_taken_for_a_dbx_job() {
        // Every fixture in this tree outside the DBX folder, whoever wrote it:
        // none of them is a DBX file, so the probe must rule each one out.
        let dbx = dbx_folder();
        let mut others = 0usize;
        for path in files_under(&data_folder()) {
            if path.parent() == Some(dbx.as_path()) {
                continue;
            }
            let bytes = slurp(path.as_path());
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            assert_eq!(
                dbx_confidence(&bytes[..bytes.len().min(PROBE_BYTES)], &name),
                0.0,
                "{}",
                path.display()
            );
            others += 1;
        }
        assert!(others > 0);
    }

    #[test]
    fn random_bytes_under_a_jobs_names_are_probed_and_refused_without_a_crash() {
        // The reader looks at no byte of the file, and the probe only at whether
        // it is binary; both must still answer for any content, of any length.
        let mut random = Random32::new(20260924); // fixed: a failure must be repeatable
        for round in 0..200usize {
            let mut noise = vec![0u8; round * 37 % 5000];
            for c in &mut noise {
                *c = random.uniform(0, 255) as u8;
            }
            for name in ["JOB.XCF", "job.x01", "job.x13"] {
                let confidence = dbx_confidence(&noise, name);
                assert!(confidence >= 0.0, "{name} round {round}");
                assert!(confidence <= 1.0, "{name} round {round}");
                let error = read_survey(
                    format_registry(),
                    LEICA_DBX_FORMAT_ID,
                    &noise,
                    name,
                    &ReadOptions::default(),
                )
                .expect_err("a DBX file is refused");
                assert_eq!(error.code, ErrorCode::Unsupported);
            }
        }
    }
}
