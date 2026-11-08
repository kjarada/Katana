//! Twin of tests/surveyio/test_trimble_dc.cpp: Trimble Survey Controller DC
//! (src/trimble_dc.rs), recognised and refused, because Trimble has not
//! published its record layouts (the reasons are in that file). The fixture is
//! synthetic: lines in the shape of the records, never interpreted beyond their
//! record type.

mod support;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use katana_core::ErrorCode;
use katana_surveyio::{
    DetectionOutcome, FormatContent, Manufacturer, ReadOptions, detect_format, format_registry,
    probe_of, read_survey,
};

use support::Mt19937;

const ID: &str = "trimble-dc";

fn data_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/surveyio/data"
    ))
}

// The file's bytes, empty when it cannot be read, as the C++'s ifstream gives.
fn read_file(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

fn fixture() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| read_file(&data_dir().join("trimble_dc").join("synthetic_job.dc")))
}

fn dc_confidence(bytes: &[u8], name: &str) -> f64 {
    for result in format_registry().probe_all(&probe_of(bytes, name, false)) {
        if result.format_id == ID {
            return result.signature.confidence;
        }
    }
    -1.0 // not registered at all
}

fn read_dc(bytes: &[u8], name: &str) -> katana_core::Result<katana_surveyio::ReadResult> {
    read_survey(format_registry(), ID, bytes, name, &ReadOptions::default())
}

mod trimble_dc {
    use super::*;

    #[test]
    fn is_registered_saying_it_reads_nothing() {
        let format = format_registry().find(ID).expect("registered");
        assert_eq!(format.manufacturer, Manufacturer::Trimble);
        assert_eq!(format.reads, FormatContent::default());
        assert!(format.human_name.contains("export JobXML"));
        assert_eq!(format.extensions, ["dc"]);
    }

    #[test]
    fn a_survey_controller_file_is_identified_by_its_header_record() {
        let detection = detect_format(&probe_of(fixture(), "synthetic_job.dc", false));
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
                .contains("Survey Controller")
        );
        // The header alone identifies it whatever the file is called.
        assert!(dc_confidence(fixture(), "renamed.txt") >= 0.9);
    }

    #[test]
    fn reading_it_is_refused_with_what_to_export_instead_and_what_the_file_holds() {
        let error = read_dc(fixture(), "synthetic_job.dc").expect_err("refused");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error.message.contains("JobXML (.jxl)"));
        assert!(error.message.contains("synthetic_job.dc"));
        // 8 lines: 00 x1, 10 x1, 13 x1, 77 x1, D9 x3, E0 x1 - counted by hand.
        assert_eq!(
            error.context,
            "8 record(s): 00 x1 10 x1 13 x1 77 x1 D9 x3 E0 x1"
        );
    }

    #[test]
    fn a_sokkia_sdr_file_is_not_claimed_even_when_named_dc() {
        let sdr = concat!(
            "00NMSDR33 V04-04.02    01-Jan-24 00:00 111111\n",
            "10NMJOB1           111111\n",
            "02TP0001    1000.0000 2000.0000 50.000 1.500\n",
        );
        assert_eq!(dc_confidence(sdr.as_bytes(), "job.dc"), 0.0);
        assert_eq!(dc_confidence(sdr.as_bytes(), "job.sdr"), 0.0);
    }

    #[test]
    fn other_formats_are_not_claimed_by_the_dc_probe() {
        let mut others: Vec<(String, Vec<u8>)> = vec![
            (
                String::from("tps_gnss_job.jxl"),
                read_file(&data_dir().join("trimble_jxl").join("tps_gnss_job.jxl")),
            ),
            (
                String::from("job.gsi"),
                b"*110001+0000000000000A01 21.022+0000000009000000 22.022+0000000009000000\n"
                    .to_vec(),
            ),
            (
                String::from("job.rw5"),
                b"JB,NMJOB,DT03-05-2024,TM09:00:00\nMO,AD0,UN2,SF1.00000000,EC0,EO0.0,AU0\n"
                    .to_vec(),
            ),
            (
                String::from("obs.24o"),
                b"     3.04           OBSERVATION DATA    M                   RINEX VERSION / TYPE\n"
                    .to_vec(),
            ),
        ];
        for entry in std::fs::read_dir(data_dir())
            .expect("the data folder")
            .flatten()
        {
            let path = entry.path();
            if path.is_file() {
                others.push((
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    read_file(&path),
                ));
            }
        }
        for (name, bytes) in &others {
            assert_eq!(dc_confidence(bytes, name), 0.0, "{name}");
        }
    }

    #[test]
    fn a_dc_extension_alone_is_only_a_weak_suggestion() {
        assert!(dc_confidence(b"hello world\n", "notes.dc") <= 0.1);
        let detection = detect_format(&probe_of(b"hello world\n", "notes.dc", false));
        assert_ne!(detection.outcome(), DetectionOutcome::Identified);
    }

    #[test]
    fn every_truncation_and_random_bytes_probe_and_refuse_without_a_crash() {
        let bytes = fixture();
        for length in 0..=bytes.len() {
            let cut = &bytes[..length];
            let confidence = dc_confidence(cut, "synthetic_job.dc");
            assert!(confidence >= 0.0);
            assert!(confidence <= 1.0);
            assert!(read_dc(cut, "synthetic_job.dc").is_err());
        }
        let mut random = Mt19937::new(7);
        for _ in 0..200 {
            let mut noise = vec![0u8; random.next_u32() as usize % 4096];
            for c in &mut noise {
                *c = (random.next_u32() & 0xFF) as u8;
            }
            let confidence = dc_confidence(&noise, "noise.dc");
            assert!(confidence >= 0.0);
            assert!(confidence <= 1.0);
            assert!(read_dc(&noise, "noise.dc").is_err());
        }
    }
}
