//! The twin of tests/storage/test_survey_job_storage.cpp: survey jobs in a
//! project (schema 10) - every field back exactly and in order, the raw bytes
//! of each field file kept whole, the packed lists refused when damaged, and
//! a job too large for the database refused before a byte is written.

pub mod support;

use std::path::{Path, PathBuf};
use std::time::Instant;

use katana_core::ErrorCode;
use katana_entity::{Geometry, Layer};
use katana_geometry::Point2;
use katana_storage::{
    OpenMode, ProjectContents, ProjectMetadata, ProjectStore, SqliteDatabase, SurveyJob,
    SurveyJobFile, SurveyJobPoint,
};
use katana_survey::{
    AdjustmentMethod, AtmosphericCorrection, ControlOrigin, ControlPoint, ControlSelection,
    NetworkDimension, ReductionSettings,
};
use support::{Scratch, fixtures, repository};

fn open(path: &Path) -> SqliteDatabase {
    SqliteDatabase::open(path, OpenMode::ReadWriteCreate).unwrap()
}

fn create(project: &Path) -> ProjectStore {
    ProjectStore::create(project, &ProjectMetadata::default()).unwrap()
}

fn scratch(case: &str) -> (Scratch, PathBuf) {
    let scratch = Scratch::new("SurveyJobStorage", case);
    let project = scratch.project_dir();
    (scratch, project)
}

// A job with every field set to something the defaults are not, including
// bytes a TEXT column would mangle (a NUL, a lone CR, bytes that are not
// UTF-8) and doubles that only an exact round trip keeps.
fn full_job(id: &str) -> SurveyJob {
    // The C++ copies 51 and 17 bytes of its literals: each with its
    // terminating NUL.
    let mut source_bytes = b"*110001+0000000000000101 21.324+0000000009000000\r\n\0".to_vec();
    source_bytes.extend_from_slice(b"\0\r\xff\xfe binary tail\0");
    let mut settings = ReductionSettings {
        atmospheric: AtmosphericCorrection::Fixed,
        fixed_ppm: 12.4,
        method: AdjustmentMethod::Network,
        network_dimension: NetworkDimension::HorizontalAndLevels,
        // Not exactly 0.12 in binary.
        refraction_coefficient: 0.1 + 0.02,
        ..ReductionSettings::default()
    };
    settings.control.push(ControlSelection {
        point: ControlPoint::fixed_horizontal("CP;1"),
        origin: ControlOrigin::Drawing,
    });
    SurveyJob {
        id: String::from(id),
        name: String::from("Blue beach day 2"),
        format_id: String::from("leica-gsi"),
        parser_version: String::from("1.3"),
        source_file_name: String::from("DAY2.GSI"),
        source_bytes,
        sibling_files: vec![
            SurveyJobFile {
                name: String::from("DAY2.X01"),
                bytes: vec![0x01, 0x00, 0x02],
            },
            SurveyJobFile {
                name: String::from("day2.nav"),
                bytes: b"3.04 N: GNSS NAV DATA".to_vec(),
            },
        ],
        settings,
        layer: String::from("survey/day 2"),
        created_entities: vec![7, 8, 9, 4_000_000_000],
        report_text: String::from("Reduction report\n  101  N 6250000.125\n"),
        report_html: String::from("<!DOCTYPE html><p>101 &amp; 102</p>"),
        report_created_utc: String::from("2026-09-24T01:02:03Z"),
        imported_utc: String::from("2026-09-23T22:00:00Z"),
        placed_points: vec![
            SurveyJobPoint {
                point_id: String::from("101"),
                entity: 7,
                northing: 6250000.125,
                easting: 300000.0625,
                elevation: Some(12.5),
            },
            SurveyJobPoint {
                point_id: String::from("102"),
                entity: 8,
                northing: 1.0 / 3.0,
                easting: -0.1,
                elevation: None,
            },
            SurveyJobPoint {
                point_id: String::new(),
                entity: 9,
                // std::numeric_limits<double>::denorm_min().
                northing: f64::from_bits(1),
                easting: 0.0,
                elevation: Some(-0.0),
            },
        ],
        import_options: String::from(
            "katana-survey-import-options=1\nlayer-per-code=true\npoint-number-property=pt%0Ano\n",
        ),
    }
}

fn contents_with(jobs: Vec<SurveyJob>) -> ProjectContents {
    ProjectContents {
        layers: vec![Layer::default()],
        survey_jobs: jobs,
        ..ProjectContents::default()
    }
}

// Big enough to be over SQLite's own limit on a value and on a row -
// SQLITE_MAX_LENGTH, 1,000,000,000 bytes as the library is built (PRAGMA
// compile_options: MAX_LENGTH=1000000000) - and still under the 1 GiB a reader
// accepts: a long 1 Hz multi-GNSS RINEX session. The bytes are never written
// or read, and a zeroed allocation this size is mapped lazily, so the vector
// costs address space, not memory.
fn oversized_field_file() -> Vec<u8> {
    vec![0_u8; 1_020_000_000]
}

mod survey_job_storage {
    use super::*;

    #[test]
    fn a_job_starts_empty_with_default_reduction_settings() {
        let _scratch = scratch("AJobStartsEmptyWithDefaultReductionSettings");
        let job = SurveyJob::default();
        assert!(job.id.is_empty());
        assert!(job.source_bytes.is_empty());
        assert!(job.sibling_files.is_empty());
        assert!(job.created_entities.is_empty());
        assert!(job.placed_points.is_empty());
        assert_eq!(job.settings, ReductionSettings::default());
        assert!(ProjectContents::default().survey_jobs.is_empty());
    }

    #[test]
    fn every_field_of_every_job_comes_back_exactly_and_in_order() {
        let (_scratch, project) = scratch("EveryFieldOfEveryJobComesBackExactlyAndInOrder");
        let first = full_job("job-12");
        // A job with nothing but an id: empty is not missing.
        let second = SurveyJob {
            id: String::from("job-3"),
            ..SurveyJob::default()
        };
        {
            let mut store = create(&project);
            store
                .save(&contents_with(vec![first.clone(), second.clone()]))
                .unwrap();
        }
        let store = ProjectStore::open(&project).unwrap();
        let contents = store.load().unwrap();
        assert_eq!(contents.survey_jobs.len(), 2);
        // Creation order, not id order: "job-12" was made before "job-3" here.
        assert_eq!(contents.survey_jobs[0], first);
        assert_eq!(contents.survey_jobs[1], second);
        // Spelt out for the fields whose loss would be quiet.
        assert_eq!(contents.survey_jobs[0].source_bytes.len(), 68); // 51 + 17
        assert_eq!(
            contents.survey_jobs[0].settings.refraction_coefficient,
            0.1 + 0.02
        );
        assert!(contents.survey_jobs[0].placed_points[1].elevation.is_none());
        assert!(
            contents.survey_jobs[0].placed_points[2]
                .elevation
                .unwrap()
                .is_sign_negative()
        );
    }

    #[test]
    fn a_second_save_replaces_the_jobs_rather_than_adding_to_them() {
        let (_scratch, project) = scratch("ASecondSaveReplacesTheJobsRatherThanAddingToThem");
        let mut store = create(&project);
        store
            .save(&contents_with(vec![full_job("job-1"), full_job("job-2")]))
            .unwrap();
        // The same ids again would break the UNIQUE constraint if the first
        // save's rows were still there; one job fewer shows a removal sticks.
        store.save(&contents_with(vec![full_job("job-2")])).unwrap();
        let contents = store.load().unwrap();
        assert_eq!(contents.survey_jobs.len(), 1);
        assert_eq!(contents.survey_jobs[0].id, "job-2");
        assert_eq!(contents.survey_jobs[0].sibling_files.len(), 2);
    }

    #[test]
    fn a_job_without_an_id_or_with_another_jobs_id_is_refused_and_nothing_is_written() {
        let (_scratch, project) =
            scratch("AJobWithoutAnIdOrWithAnotherJobsIdIsRefusedAndNothingIsWritten");
        let mut store = create(&project);
        store.save(&contents_with(vec![full_job("job-1")])).unwrap();

        let unnamed = store
            .save(&contents_with(vec![full_job("")]))
            .expect_err("a job without an id was saved");
        assert_eq!(unnamed.code, ErrorCode::InvalidArgument);
        let twice = store
            .save(&contents_with(vec![full_job("job-5"), full_job("job-5")]))
            .expect_err("two jobs of one id were saved");
        assert_eq!(twice.code, ErrorCode::InvalidArgument);

        let contents = store.load().unwrap();
        assert_eq!(contents.survey_jobs.len(), 1);
        assert_eq!(contents.survey_jobs[0].id, "job-1");
    }

    // The size the brief names. Every byte value appears, so a column that
    // re-encoded text, stopped at a NUL or truncated would show.
    #[test]
    fn a_fifty_megabyte_field_file_and_its_sibling_round_trip_byte_for_byte() {
        let (_scratch, project) =
            scratch("AFiftyMegabyteFieldFileAndItsSiblingRoundTripByteForByte");
        const SIZE: usize = 50 * 1024 * 1024;
        let mut job = full_job("job-1");
        job.source_bytes.resize(SIZE, 0);
        // xorshift32: every byte value, no pattern.
        let mut state: u32 = 2463534242;
        for byte in &mut job.source_bytes {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = (state & 0xFF) as u8;
        }
        job.sibling_files[1].bytes = job.source_bytes[SIZE / 2..].to_vec(); // 25 MB more

        let started = Instant::now();
        {
            let mut store = create(&project);
            store.save(&contents_with(vec![job.clone()])).unwrap();
        }
        let saved = Instant::now();
        let store = ProjectStore::open(&project).unwrap();
        let contents = store.load().unwrap();
        let loaded = Instant::now();
        assert_eq!(contents.survey_jobs.len(), 1);
        assert!(contents.survey_jobs[0].source_bytes == job.source_bytes);
        assert!(contents.survey_jobs[0].sibling_files == job.sibling_files);
        assert_eq!(contents.survey_jobs[0], job);
        // Recorded, not asserted: a shared machine's timing is not a property.
        println!(
            "75 MB of survey job files: save {} ms, open + load {} ms",
            (saved - started).as_millis(),
            (loaded - saved).as_millis()
        );
    }

    // A project written by main's code before survey jobs existed (schema 9,
    // made with main's katana_cli: two points and a line on layer
    // survey/points, then SAVE). It opens with every entity as it was, no
    // jobs, migrated to the current schema after a backup - and then keeps a
    // job like any other.
    #[test]
    fn a_project_saved_before_survey_jobs_opens_unchanged_and_then_keeps_one() {
        let (_scratch, project) =
            scratch("AProjectSavedBeforeSurveyJobsOpensUnchangedAndThenKeepsOne");
        std::fs::create_dir_all(project.as_path()).unwrap();
        std::fs::copy(
            fixtures().join("schema9_main/project.db"),
            project.join("project.db"),
        )
        .unwrap();
        {
            let raw =
                SqliteDatabase::open(&project.join("project.db"), OpenMode::ReadOnly).unwrap();
            assert_eq!(raw.user_version().unwrap(), 9);
        }

        let mut store = ProjectStore::open(&project).unwrap();
        // Migrated all the way: through survey jobs (10) and on to whatever is
        // current.
        assert_eq!(
            store.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        assert_eq!(store.list_backups().len(), 1); // taken before the migration
        let mut contents = store.load().unwrap();
        assert!(contents.survey_jobs.is_empty());
        assert_eq!(contents.metadata.name, "schema9");
        assert_eq!(contents.next_entity_id, 4);
        assert_eq!(contents.entities.len(), 3);
        // The coordinates the CLI was given: POINT 300000.125,6250000.5 and
        // POINT 300010.25,6250020.75 - binary fractions, so exact.
        let position = |geometry: &Geometry| match geometry {
            Geometry::Point(point) => point.position,
            other => panic!("not a point: {other:?}"),
        };
        assert_eq!(
            position(&contents.entities[0].geometry),
            Point2::new(300000.125, 6250000.5)
        );
        assert_eq!(
            position(&contents.entities[1].geometry),
            Point2::new(300010.25, 6250020.75)
        );
        assert_eq!(contents.entities[2].layer, "survey/points");

        let mut job = full_job("job-4");
        job.created_entities = vec![1, 2];
        contents.survey_jobs.push(job.clone());
        store.save(&contents).unwrap();
        let again = store.load().unwrap();
        assert_eq!(again.survey_jobs.len(), 1);
        assert_eq!(again.survey_jobs[0], job);
        assert_eq!(again.entities.len(), 3);
    }

    // The sample drawing in the repository was saved by an older main still
    // (schema 8), so it runs two migrations to get here.
    #[test]
    fn the_sample_drawing_from_an_older_schema_opens_with_no_jobs() {
        let (_scratch, project) = scratch("TheSampleDrawingFromAnOlderSchemaOpensWithNoJobs");
        let sample = repository().join("samples/site_plan");
        assert!(sample.join("project.db").is_file());
        std::fs::create_dir_all(project.as_path()).unwrap();
        std::fs::copy(sample.join("project.db"), project.join("project.db")).unwrap();
        let store = ProjectStore::open(&project).unwrap();
        assert_eq!(
            store.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        let contents = store.load().unwrap();
        assert!(contents.survey_jobs.is_empty());
        assert!(!contents.entities.is_empty());
    }

    #[test]
    fn a_damaged_list_of_placed_points_fails_the_load_with_a_sentence() {
        let (_scratch, project) = scratch("ADamagedListOfPlacedPointsFailsTheLoadWithASentence");
        {
            let mut store = create(&project);
            store.save(&contents_with(vec![full_job("job-1")])).unwrap();
        }
        {
            let raw = open(&project.join("project.db"));
            // Cut the blob short in the middle of the first point.
            raw.execute(concat!(
                "UPDATE survey_jobs SET placed_points =",
                " substr(placed_points, 1, 20)"
            ))
            .unwrap();
        }
        let store = ProjectStore::open(&project).unwrap();
        let error = store.load().expect_err("a damaged list was loaded");
        assert_eq!(error.code, ErrorCode::DatabaseFailure);
        assert!(error.message.contains("damaged"));
        assert!(error.context.contains("job-1"));
    }

    #[test]
    fn settings_written_by_a_newer_version_are_refused_rather_than_half_read() {
        let (_scratch, project) =
            scratch("SettingsWrittenByANewerVersionAreRefusedRatherThanHalfRead");
        {
            let mut store = create(&project);
            store.save(&contents_with(vec![full_job("job-1")])).unwrap();
        }
        {
            let raw = open(&project.join("project.db"));
            raw.execute(concat!(
                "UPDATE survey_jobs SET settings =",
                " 'katana-reduction-settings=99' || char(10) || 'atmospheric=none'"
            ))
            .unwrap();
        }
        let store = ProjectStore::open(&project).unwrap();
        let error = store.load().expect_err("newer settings were loaded");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error.message.contains("job-1"));
    }

    // The packed lists are read back from a file that may have been damaged
    // or edited by hand, so every shortened form of a sound one must fail the
    // load with an error - never read past its end, never crash, never load as
    // a different job.
    #[test]
    fn every_truncation_of_the_packed_lists_is_refused_cleanly() {
        let (_scratch, project) = scratch("EveryTruncationOfThePackedListsIsRefusedCleanly");
        {
            let mut store = create(&project);
            store.save(&contents_with(vec![full_job("job-1")])).unwrap();
        }
        let (placed_size, created_size) = {
            let raw = open(&project.join("project.db"));
            let mut sizes = raw
                .prepare("SELECT length(placed_points), length(created_entities) FROM survey_jobs")
                .unwrap();
            assert!(sizes.step().unwrap());
            let placed = sizes.column_int64(0);
            let created = sizes.column_int64(1);
            raw.execute(concat!(
                "CREATE TABLE sound AS SELECT placed_points, created_entities",
                " FROM survey_jobs"
            ))
            .unwrap();
            (placed, created)
        };
        assert!(placed_size > 60);
        assert_eq!(created_size, 32); // four ids of eight bytes
        let load_after = |sql: &str| {
            {
                let raw = open(&project.join("project.db"));
                assert!(
                    raw.execute(&format!(
                        "{}{sql}",
                        concat!(
                            "UPDATE survey_jobs SET placed_points = (SELECT",
                            " placed_points FROM sound), created_entities = (SELECT",
                            " created_entities FROM sound);"
                        )
                    ))
                    .is_ok()
                );
            }
            let store = ProjectStore::open(&project).unwrap();
            store.load()
        };
        for keep in 0..placed_size {
            let contents = load_after(&format!(
                "UPDATE survey_jobs SET placed_points = substr(placed_points, 1, {keep})"
            ));
            assert!(contents.is_err(), "placed points cut to {keep} bytes");
        }
        for keep in 1..created_size {
            if keep % 8 == 0 {
                continue; // a whole number of ids is a shorter list, not damage
            }
            let contents = load_after(&format!(
                "UPDATE survey_jobs SET created_entities = substr(created_entities, 1, {keep})"
            ));
            assert!(contents.is_err(), "created entities cut to {keep} bytes");
        }
        // And the sound bytes still load: the loop did not pass by always
        // failing.
        let sound = load_after("").unwrap();
        assert_eq!(sound.survey_jobs[0], full_job("job-1"));
    }

    // A job too large for the database is refused before anything is
    // written, with a sentence that names its file and says what to do - never
    // SQLite's "string or blob too big" - and the project stays as it was last
    // saved.
    #[test]
    fn a_job_too_large_for_the_database_is_refused_with_a_sentence_and_nothing_is_written() {
        let (_scratch, project) =
            scratch("AJobTooLargeForTheDatabaseIsRefusedWithASentenceAndNothingIsWritten");
        let mut store = create(&project);
        store.save(&contents_with(vec![full_job("job-1")])).unwrap();

        let mut contents = contents_with(vec![full_job("job-1")]);
        contents.survey_jobs.push(full_job("job-2"));
        contents.survey_jobs[1].source_bytes = oversized_field_file();
        let file = store
            .save(&contents)
            .expect_err("an oversized job was saved");
        assert_eq!(file.code, ErrorCode::InvalidArgument, "{}", file.describe());
        assert!(file.message.contains("DAY2.GSI"), "{}", file.message);
        assert!(file.message.contains("too large"), "{}", file.message);
        assert!(!file.message.contains("blob"), "{}", file.message);

        // A file kept beside it has a row of its own, under the same limit.
        contents.survey_jobs[1].source_bytes = b"small".to_vec();
        contents.survey_jobs[1].sibling_files[0].bytes = oversized_field_file();
        let sibling = store
            .save(&contents)
            .expect_err("an oversized kept file was saved");
        assert_eq!(
            sibling.code,
            ErrorCode::InvalidArgument,
            "{}",
            sibling.describe()
        );
        assert!(sibling.message.contains("DAY2.X01"), "{}", sibling.message);
        assert!(sibling.message.contains("too large"), "{}", sibling.message);

        let loaded = store.load().unwrap();
        assert_eq!(loaded.survey_jobs.len(), 1);
        assert_eq!(loaded.survey_jobs[0], full_job("job-1"));
    }

    // The check counts the whole row - report and point lists as well as the
    // file - and each kept file's row on its own. At the limit is inside it.
    #[test]
    fn the_size_check_counts_the_whole_row_and_each_kept_file_on_its_own() {
        let _scratch = scratch("TheSizeCheckCountsTheWholeRowAndEachKeptFileOnItsOwn");
        let mut job = full_job("job-1");
        let row = ProjectStore::survey_job_row_bytes(&job);
        assert!(ProjectStore::check_survey_job_size(&job, row).is_ok());
        let over = ProjectStore::check_survey_job_size(&job, row - 1)
            .expect_err("a job over its limit passed");
        assert_eq!(over.code, ErrorCode::InvalidArgument);
        assert!(
            over.message
                .contains("survey job job-1 (DAY2.GSI) is too large"),
            "{}",
            over.message
        );

        job.report_html.push_str(&"x".repeat(1000));
        assert_eq!(ProjectStore::survey_job_row_bytes(&job), row + 1000);
        // One more placed point with a height and the id "104", by the packed
        // layout: entity 8 + northing 8 + easting 8 + height flag 1 + height 8
        // + id length 4 + the id 3 = 40 bytes; one more created entity, 8.
        job.placed_points.push(SurveyJobPoint {
            point_id: String::from("104"),
            entity: 10,
            northing: 1.0,
            easting: 2.0,
            elevation: Some(3.0),
        });
        job.created_entities.push(10);
        assert_eq!(
            ProjectStore::survey_job_row_bytes(&job),
            row + 1000 + 40 + 8
        );

        // A kept file's row is the job's id (5), its name (8) and its bytes,
        // and is not part of the job's row: 2 000 bytes is over a limit the
        // job's own row (about 1.2 kB, most of it the settings) is within.
        let mut with_file = full_job("job-1");
        with_file.sibling_files[0].bytes = vec![0x01; 2000];
        let job_row = ProjectStore::survey_job_row_bytes(&with_file);
        assert!(job_row < 5 + 8 + 2000);
        assert!(ProjectStore::check_survey_job_size(&with_file, 5 + 8 + 2000).is_ok());
        let file = ProjectStore::check_survey_job_size(&with_file, 5 + 8 + 1999)
            .expect_err("a kept file over its limit passed");
        assert!(
            file.message
                .contains("DAY2.X01, read with survey job job-1 (DAY2.GSI), is too large"),
            "{}",
            file.message
        );
    }

    // What the check counts is what SQLite stores - every column but the
    // position, byte for byte - and the limit sits under the linked library's
    // own with room for the record header.
    #[test]
    fn the_size_counted_is_the_size_stored_and_the_limit_is_under_the_librarys_own() {
        let (_scratch, project) =
            scratch("TheSizeCountedIsTheSizeStoredAndTheLimitIsUnderTheLibrarysOwn");
        {
            let mut store = create(&project);
            store.save(&contents_with(vec![full_job("job-1")])).unwrap();
        }
        let raw = open(&project.join("project.db"));
        // length() of a BLOB is bytes; of TEXT it is characters, hence the
        // casts.
        let mut stored = raw
            .prepare(concat!(
                "SELECT length(CAST(id AS BLOB)) + length(CAST(name AS BLOB))",
                " + length(CAST(format_id AS BLOB)) + length(CAST(parser_version AS BLOB))",
                " + length(CAST(source_file_name AS BLOB)) + length(source_bytes)",
                " + length(CAST(settings AS BLOB)) + length(CAST(layer AS BLOB))",
                " + length(created_entities) + length(placed_points)",
                " + length(CAST(report_text AS BLOB)) + length(CAST(report_html AS BLOB))",
                " + length(CAST(report_created_utc AS BLOB)) + length(CAST(imported_utc AS BLOB))",
                " + length(CAST(import_options AS BLOB)) FROM survey_jobs"
            ))
            .unwrap();
        assert!(stored.step().unwrap());
        assert_eq!(
            stored.column_int64(0) as u64,
            ProjectStore::survey_job_row_bytes(&full_job("job-1"))
        );

        assert_eq!(
            raw.length_limit(),
            1_000_000_000,
            "SQLITE_MAX_LENGTH as the library is built"
        );
        assert!(ProjectStore::MAX_SURVEY_JOB_BYTES + 1024 <= raw.length_limit() as u64);
    }
}
