//! Twin of tests/surveyio/test_reader.cpp.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use katana_core::{ErrorCode, Result};
use katana_survey::SurveyPoint;
use katana_surveyio::{
    DELIMITED_POINTS_FORMAT_ID, FormatContent, FormatDescriptor, FormatReader, FormatRegistry,
    FormatSignature, ImportResult, MAX_SURVEY_FILE_BYTES, Manufacturer, ProbeInput, ReadOptions,
    ReadResult, ReadWarning, SiblingFile, SiblingLookup, describe, format_registry, read_survey,
    ruled_out, siblings_in_folder, siblings_in_memory, to_import_result,
};

use support::Scratch;

fn probe_nothing(_: &ProbeInput<'_>) -> FormatSignature {
    ruled_out()
}

fn fake_format(id: &str, can_import: bool) -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(id),
        human_name: String::from("Fake instrument"),
        manufacturer: Manufacturer::Other,
        reads: FormatContent {
            points: true,
            observations: true,
            stations: true,
            ..Default::default()
        },
        can_import,
        can_export: !can_import,
        parser_version: String::from("0.9"),
        extensions: vec![String::from("fak")],
    }
}

fn point(id: &str) -> SurveyPoint {
    SurveyPoint {
        id: String::from(id),
        northing: 100.0,
        easting: 200.0,
        ..Default::default()
    }
}

// One point per line of input, named by the line; the sibling "extra.fak",
// when asked for and present, adds one more.
fn read_lines(bytes: &[u8], file_name: &str, options: &ReadOptions) -> Result<ReadResult> {
    let mut result = ReadResult::default();
    let mut line = 0;
    let mut start = 0;
    while start < bytes.len() {
        let cut = bytes[start..].iter().position(|&c| c == b'\n');
        let end = cut.map_or(bytes.len(), |at| start + at);
        let text = &bytes[start..end];
        start = cut.map_or(bytes.len(), |at| start + at + 1);
        line += 1;
        if text.is_empty() {
            result.warnings.push(ReadWarning {
                file_name: String::from(file_name),
                record: line,
                message: String::from("empty record"),
            });
            result.records_skipped += 1;
            continue;
        }
        result.project.points.push(point(
            std::str::from_utf8(text).expect("the fixture is UTF-8"),
        ));
        result.records_read += 1;
    }
    if let Some(siblings) = &options.siblings {
        match siblings("extra.fak") {
            Ok(extra) => result.project.points.push(point(
                std::str::from_utf8(&extra).expect("the fixture is UTF-8"),
            )),
            Err(error) => result.warnings.push(ReadWarning {
                file_name: String::from("extra.fak"),
                record: 0,
                message: error.message,
            }),
        }
    }
    result
        .not_carried
        .push(String::from("no instrument heights"));
    Ok(result)
}

// Asks for a sibling by a path taken from "the file" and reports what it got.
fn read_traversal(_: &[u8], _: &str, options: &ReadOptions) -> Result<ReadResult> {
    let mut result = ReadResult::default();
    let siblings = options.siblings.as_ref().expect("the test gives a lookup");
    let sibling = siblings("..\\..\\secret.txt");
    result.warnings.push(ReadWarning {
        file_name: String::new(),
        record: 0,
        message: match sibling {
            Ok(_) => String::from("read it"),
            Err(error) => error.message,
        },
    });
    Ok(result)
}

fn read_duplicates(_: &[u8], _: &str, _: &ReadOptions) -> Result<ReadResult> {
    let mut result = ReadResult::default();
    result.project.points = vec![point("1"), point("1")];
    Ok(result)
}

fn read_throwing(_: &[u8], _: &str, _: &ReadOptions) -> Result<ReadResult> {
    // The twin of the C++ reader's std::runtime_error.
    panic!("record 3 made the reader throw");
}

fn registry_with(reader: FormatReader) -> FormatRegistry {
    let mut registry = FormatRegistry::default();
    assert!(
        registry
            .add(
                fake_format("fake-lines", true),
                Some(probe_nothing),
                Some(reader)
            )
            .is_ok()
    );
    registry
}

mod survey_reader {
    use super::*;

    #[test]
    fn the_registry_accepts_a_reader_beside_a_probe() {
        let mut registry = FormatRegistry::default();
        assert!(
            registry
                .add(
                    fake_format("fake-lines", true),
                    Some(probe_nothing),
                    Some(read_lines)
                )
                .is_ok()
        );
        assert!(
            registry
                .add(
                    fake_format("fake-detect-only", true),
                    Some(probe_nothing),
                    None
                )
                .is_ok()
        );
        // The very reader that was registered: compared by address, as the C++
        // compares the function pointers.
        assert!(
            registry
                .reader("fake-lines")
                .is_some_and(|reader| std::ptr::fn_addr_eq(reader, read_lines as FormatReader))
        );
        assert!(registry.reader("fake-detect-only").is_none());
        assert!(registry.reader("no-such-format").is_none());
    }

    #[test]
    fn a_reader_for_a_format_that_cannot_import_is_refused() {
        let mut registry = FormatRegistry::default();
        let status = registry.add(
            fake_format("fake-export", false),
            Some(probe_nothing),
            Some(read_lines),
        );
        assert_eq!(
            status.expect_err("the reader is refused").code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn read_survey_dispatches_to_the_reader_and_stamps_the_format() {
        let registry = registry_with(read_lines);
        let read = read_survey(
            &registry,
            "fake-lines",
            b"A\n\nB\n",
            "C:\\jobs\\site.fak",
            &ReadOptions::default(),
        );
        let read = read.unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read.project.points.len(), 2);
        assert_eq!(read.project.points[1].id, "B");
        assert_eq!(read.records_read, 2);
        assert_eq!(read.records_skipped, 1);
        assert_eq!(read.format_id, "fake-lines");
        assert_eq!(read.parser_version, "0.9");
        assert_eq!(read.warnings.len(), 1);
        // The reader was handed the file NAME, never the path.
        assert_eq!(read.warnings[0].file_name, "site.fak");
        assert_eq!(read.warnings[0].record, 2);
        assert_eq!(
            describe(&read.warnings[0]),
            "site.fak record 2: empty record"
        );
        assert!(read.siblings_read.is_empty());
    }

    #[test]
    fn sibling_files_are_served_by_name_and_recorded_with_their_bytes() {
        let registry = registry_with(read_lines);
        let options = ReadOptions {
            siblings: Some(siblings_in_memory(vec![SiblingFile {
                name: String::from("EXTRA.FAK"),
                bytes: b"X".to_vec(),
            }])),
            ..Default::default()
        };
        let read = read_survey(&registry, "fake-lines", b"A\n", "site.fak", &options)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read.project.points.len(), 2);
        assert_eq!(read.project.points[1].id, "X"); // found ignoring ASCII case
        assert_eq!(read.siblings_read.len(), 1);
        assert_eq!(
            read.siblings_read[0],
            SiblingFile {
                name: String::from("extra.fak"),
                bytes: b"X".to_vec(),
            }
        );
    }

    #[test]
    fn a_missing_sibling_is_not_found_and_the_reader_carries_on() {
        let registry = registry_with(read_lines);
        let options = ReadOptions {
            siblings: Some(siblings_in_memory(Vec::new())),
            ..Default::default()
        };
        let read = read_survey(&registry, "fake-lines", b"A\n", "site.fak", &options)
            .expect("the reader carries on");
        assert_eq!(read.warnings.len(), 1);
        assert!(
            read.warnings[0]
                .message
                .contains("no file called 'extra.fak'")
        );
    }

    #[test]
    fn a_sibling_named_by_a_path_is_refused_before_any_lookup_runs() {
        let lookups = Arc::new(AtomicI32::new(0));
        let counted = Arc::clone(&lookups);
        let lookup: SiblingLookup = Arc::new(move |_: &str| -> Result<Vec<u8>> {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(b"secret".to_vec())
        });
        let options = ReadOptions {
            siblings: Some(lookup),
            ..Default::default()
        };
        let registry = registry_with(read_traversal);
        let read = read_survey(&registry, "fake-lines", b"x", "site.fak", &options)
            .expect("the reader runs");
        assert_eq!(lookups.load(Ordering::SeqCst), 0);
        assert!(read.warnings[0].message.contains("refused"));
        assert!(read.siblings_read.is_empty());
    }

    #[test]
    fn a_reader_that_returns_an_inconsistent_project_is_an_import_failure() {
        let registry = registry_with(read_duplicates);
        let error = read_survey(
            &registry,
            "fake-lines",
            b"x",
            "site.fak",
            &ReadOptions::default(),
        )
        .expect_err("the project does not hold together");
        assert_eq!(error.code, ErrorCode::FileImportFailure);
        assert!(error.message.contains("Fake instrument"));
    }

    #[test]
    fn a_reader_that_throws_is_reported_not_fatal() {
        let registry = registry_with(read_throwing);
        let error = read_survey(
            &registry,
            "fake-lines",
            b"x",
            "site.fak",
            &ReadOptions::default(),
        )
        .expect_err("the reader panicked");
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(error.message.contains("record 3"));
    }

    #[test]
    fn an_unknown_format_is_not_found_and_a_format_without_a_reader_is_unsupported() {
        let mut registry = FormatRegistry::default();
        assert!(
            registry
                .add(
                    fake_format("fake-detect-only", true),
                    Some(probe_nothing),
                    None
                )
                .is_ok()
        );
        let options = ReadOptions::default();
        assert_eq!(
            read_survey(&registry, "no-such-format", b"x", "a.fak", &options)
                .expect_err("there is no such format")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            read_survey(&registry, "fake-detect-only", b"x", "a.fak", &options)
                .expect_err("the format has no reader")
                .code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn the_coordinate_file_format_is_read_through_its_layout_not_a_reader() {
        let error = read_survey(
            format_registry(),
            DELIMITED_POINTS_FORMAT_ID,
            b"1,100,200\n",
            "points.csv",
            &ReadOptions::default(),
        )
        .expect_err("the delimited format has no reader");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error.context.contains("column layout"));
    }

    #[test]
    fn a_folder_lookup_finds_a_file_beside_the_job_ignoring_case_and_nothing_else() {
        let scratch = Scratch::new("katana-surveyio-reader-test");
        scratch.write("job/JOB.X01", b"dbx bytes");
        scratch.write("outside.txt", b"not yours");
        let job = scratch.root.join("job");
        let lookup = siblings_in_folder(job.clone(), MAX_SURVEY_FILE_BYTES);
        let found = lookup("job.x01").unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(found, b"dbx bytes");
        let code = |name: &str| lookup(name).expect_err("the name is refused").code;
        assert_eq!(code("..\\outside.txt"), ErrorCode::InvalidArgument);
        assert_eq!(code("../outside.txt"), ErrorCode::InvalidArgument);
        assert_eq!(code(".."), ErrorCode::InvalidArgument);
        assert_eq!(code("missing.x02"), ErrorCode::NotFound);
        // A cap smaller than the file refuses it.
        assert_eq!(
            siblings_in_folder(job, 4)("JOB.X01")
                .expect_err("the file is over the cap")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_read_result_becomes_the_wizards_import_result_with_nothing_lost() {
        let read = ReadResult {
            format_id: String::from("fake-lines"),
            records_read: 1,
            records_skipped: 2,
            warnings: vec![
                ReadWarning {
                    file_name: String::from("a.fak"),
                    record: 7,
                    message: String::from("bad angle"),
                },
                ReadWarning {
                    file_name: String::new(),
                    record: 0,
                    message: String::from("no header"),
                },
            ],
            not_carried: vec![String::from("no instrument heights")],
            project: katana_survey::SurveyProject {
                points: vec![point("1")],
                ..Default::default()
            },
            ..Default::default()
        };
        let imported: ImportResult = to_import_result(read);
        assert_eq!(imported.format_id, "fake-lines");
        assert_eq!(imported.project.points.len(), 1);
        assert_eq!(imported.records_skipped, 2);
        assert_eq!(
            imported.warnings,
            [
                "a.fak record 7: bad angle",
                "no header",
                "not in the file: no instrument heights",
            ]
        );
    }
}
