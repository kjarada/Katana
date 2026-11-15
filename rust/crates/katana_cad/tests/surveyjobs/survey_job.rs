// Survey jobs (tests/cad/test_survey_job.cpp): import, re-adjust and remove,
// each one undo step, with the reduction and the reader INJECTED - these
// tests hand the commands a fake reduction whose coordinates are known, so
// what is tested is the job and the drawing, not the reduction's arithmetic.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use katana_cad::survey_job::{default_reduction, reduce_for_drawing, reduction_context_for};
use katana_cad::survey_points::{drawing_survey_points, import_survey_points};
use katana_cad::{
    Document, ExistingPointPolicy, HandEditPolicy, ImportSurveyJobCommand,
    ReadjustSurveyJobCommand, ReductionFunction, RemoveSurveyJobCommand, SurveyImportOptions,
    SurveyJob, SurveyJobAccess, SurveyJobContext, SurveyJobFile, SurveyJobImport, SurveyJobPoint,
    SurveyJobReader, SurveyJobReadjustment,
};
use katana_commands::{delete_entities, move_entities};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::heights_of;
use katana_entity::{Entity, EntityId, EnumName, Geometry};
use katana_geometry::{Point2, Vec2};
use katana_storage::ProjectStore;
use katana_survey::data_model::{
    ControlConstraint, ControlPoint, CoordinateSource, SurveyPoint, SurveyProject,
    UnpositionedPoint,
};
use katana_survey::reduction::{ComputedPoint, ReductionContext, ReductionOutcome};
use katana_survey::reduction_report::ReductionReport;
use katana_survey::reduction_settings::{
    AdjustmentMethod, ControlOrigin, ControlSelection, ReductionSettings, TraverseRule,
};

use crate::{ScratchDirectory, entities_of, expect_ok, must, near, refused};

#[derive(Clone, Copy, Debug, Default)]
struct Coordinates {
    northing: f64,
    easting: f64,
    elevation: Option<f64>,
}

fn at(northing: f64, easting: f64, elevation: Option<f64>) -> Coordinates {
    Coordinates {
        northing,
        easting,
        elevation,
    }
}

// Stands in for survey's reduce_and_adjust: whatever `points` holds when it
// is called is what the "reduction" computes, and every call is recorded.
#[derive(Default)]
struct FakeReduction {
    points: BTreeMap<String, Coordinates>,
    // Named by the file with no coordinates and left so by the "reduction",
    // as reduce_and_adjust leaves a point it does not compute - and a control
    // point it takes from the drawing, which it does not compute.
    unpositioned: Vec<String>,
    failure: Option<Error>,
    calls: usize,
    last_settings: ReductionSettings,
    // What the C++ keeps of the context it is handed (its previous run): a
    // survey ReductionContext holds functions and is not Clone.
    last_previous: Vec<ComputedPoint>,
    last_raw: SurveyProject,
}

type Fake = Arc<Mutex<FakeReduction>>;

fn fake_reduction() -> Fake {
    Arc::default()
}

fn points_of(points: &[(&str, Coordinates)]) -> BTreeMap<String, Coordinates> {
    points
        .iter()
        .map(|(id, at)| (String::from(*id), *at))
        .collect()
}

fn reduction_of(fake: &Fake) -> ReductionFunction {
    let fake = Arc::clone(fake);
    Box::new(
        move |raw: &SurveyProject,
              settings: &ReductionSettings,
              context: &ReductionContext|
              -> Result<ReductionOutcome> {
            let mut fake = fake.lock().unwrap();
            fake.calls += 1;
            fake.last_raw = raw.clone();
            fake.last_settings = settings.clone();
            fake.last_previous = context.previous.clone();
            if let Some(failure) = &fake.failure {
                return Err(failure.clone());
            }
            let mut points = Vec::new();
            let mut computed = Vec::new();
            for (id, at) in &fake.points {
                points.push(SurveyPoint {
                    id: id.clone(),
                    northing: at.northing,
                    easting: at.easting,
                    elevation: at.elevation,
                    code: String::from("PEG"),
                    coordinate_source: CoordinateSource::Calculated,
                    ..SurveyPoint::default()
                });
                computed.push(ComputedPoint {
                    id: id.clone(),
                    northing: at.northing,
                    easting: at.easting,
                    elevation: at.elevation,
                    ..ComputedPoint::default()
                });
            }
            let unpositioned = fake
                .unpositioned
                .iter()
                .map(|id| UnpositionedPoint {
                    id: id.clone(),
                    ..UnpositionedPoint::default()
                })
                .collect();
            Ok(ReductionOutcome {
                reduced: SurveyProject {
                    name: raw.name.clone(),
                    points,
                    unpositioned_points: unpositioned,
                    ..SurveyProject::default()
                },
                points: computed,
                report: ReductionReport {
                    created_utc: context.created_utc.clone(),
                    input: context.input.clone(),
                    settings: settings.clone(),
                    ..ReductionReport::default()
                },
            })
        },
    )
}

// The raw project a reader would have returned for the job's bytes; the fake
// reduction ignores its content, so a name is enough to recognise it.
fn raw_project(name: &str) -> SurveyProject {
    SurveyProject {
        name: String::from(name),
        ..SurveyProject::default()
    }
}

fn import_request(layer: &str) -> SurveyJobImport {
    SurveyJobImport {
        job: SurveyJob {
            name: String::from("DAY1.GSI"),
            format_id: String::from("leica-gsi"),
            parser_version: String::from("1.0"),
            source_file_name: String::from("DAY1.GSI"),
            // 29 bytes, the C++ literal's own terminating NUL among them.
            source_bytes: b"*110001+0000000000000101\r\n\0\xff\0".to_vec(),
            sibling_files: vec![SurveyJobFile {
                name: String::from("DAY1.X01"),
                bytes: b"sibling".to_vec(),
            }],
            settings: ReductionSettings {
                method: AdjustmentMethod::Traverse,
                ..ReductionSettings::default()
            },
            layer: String::from(layer),
            ..SurveyJob::default()
        },
        raw: raw_project("day one"),
        context: SurveyJobContext {
            created_utc: String::from("2026-09-24T10:00:00Z"),
            ..SurveyJobContext::default()
        },
        import_options: SurveyImportOptions {
            layer: String::from(layer),
            ..SurveyImportOptions::default()
        },
        ..SurveyJobImport::default()
    }
}

fn day_one() -> SurveyJobImport {
    import_request("survey/day1")
}

// The reader the application would bind to surveyio's reader; here it checks
// it was handed the job's own stored bytes and records the call.
fn reader_for(calls: &Arc<AtomicUsize>, expected: Vec<u8>) -> SurveyJobReader {
    let calls = Arc::clone(calls);
    Box::new(move |job: &SurveyJob| -> Result<SurveyProject> {
        calls.fetch_add(1, Ordering::SeqCst);
        if job.source_bytes != expected {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "not the stored bytes",
            ));
        }
        Ok(raw_project(&format!("re-read {}", job.source_file_name)))
    })
}

fn reads() -> Arc<AtomicUsize> {
    Arc::new(AtomicUsize::new(0))
}

fn stored_bytes(document: &Document) -> Vec<u8> {
    document.survey_jobs()[0].source_bytes.clone()
}

// The entity whose "point" property reads `id`, of any kind.
fn point_entity<'a>(document: &'a Document, id: &str) -> Option<&'a Entity> {
    let entities = &document.model().entities;
    let mut found = None;
    for entity_id in entities.ids() {
        if let Some(entity) = entities.find(entity_id)
            && entity
                .properties
                .get("point")
                .is_some_and(|value| katana_entity::entity::property_to_string(value) == id)
        {
            found = Some(entity);
        }
    }
    found
}

#[track_caller]
fn point_of<'a>(document: &'a Document, id: &str) -> &'a Entity {
    match point_entity(document, id) {
        Some(entity) => entity,
        None => panic!("no point {id}"),
    }
}

fn placed_point<'a>(job: &'a SurveyJob, id: &str) -> Option<&'a SurveyJobPoint> {
    job.placed_points
        .iter()
        .find(|placed| placed.point_id == id)
}

#[track_caller]
fn position_of(entity: &Entity) -> Point2 {
    match &entity.geometry {
        Geometry::Point(point) => point.position,
        _ => panic!("entity {} is not a point", entity.id),
    }
}

fn elevation_of(entity: &Entity) -> Option<f64> {
    heights_of(&entity.properties, 1).first().copied().flatten()
}

fn has_warning(report: &Option<ReductionReport>, parts: &[&str]) -> bool {
    report.as_ref().is_some_and(|report| {
        report
            .warnings
            .iter()
            .any(|warning| parts.iter().all(|part| warning.text.contains(part)))
    })
}

// Imports a job whose reduction computes 101, 102 and 103, and returns its id.
#[track_caller]
fn import_three_points(
    document: &mut Document,
    fake: &Fake,
    policy: ExistingPointPolicy,
) -> String {
    fake.lock().unwrap().points = points_of(&[
        ("101", at(6_250_000.0, 300_000.0, Some(10.0))),
        ("102", at(6_250_010.0, 300_020.0, Some(11.0))),
        ("103", at(6_250_030.0, 300_005.0, None)),
    ]);
    let request = SurveyJobImport {
        existing_points: policy,
        ..day_one()
    };
    let command = ImportSurveyJobCommand::new(document, request, Some(reduction_of(fake)));
    let results = command.results();
    must(document, Box::new(command));
    results.job_id()
}

#[track_caller]
fn import_three(document: &mut Document, fake: &Fake) -> String {
    import_three_points(document, fake, ExistingPointPolicy::Refuse)
}

// Somebody else's survey point, as the import wizard would have drawn it.
#[track_caller]
fn draw_foreign_point(document: &mut Document, id: &str, northing: f64, easting: f64) -> EntityId {
    let project = SurveyProject {
        points: vec![SurveyPoint {
            id: String::from(id),
            northing,
            easting,
            elevation: Some(5.0),
            ..SurveyPoint::default()
        }],
        ..SurveyProject::default()
    };
    let options = SurveyImportOptions {
        layer: String::from("control"),
        ..SurveyImportOptions::default()
    };
    let command = expect_ok(import_survey_points(
        document,
        &project,
        &options,
        ExistingPointPolicy::Refuse,
        None,
    ));
    let Some(command) = command else {
        panic!("the point is drawn");
    };
    must(document, command);
    document.last_created_entities()[0]
}

#[track_caller]
fn readjust_with(
    document: &mut Document,
    request: SurveyJobReadjustment,
    reader: SurveyJobReader,
    fake: &Fake,
) -> katana_cad::ReadjustSurveyJobResults {
    let command =
        ReadjustSurveyJobCommand::new(document, request, Some(reader), Some(reduction_of(fake)));
    let results = command.results();
    must(document, Box::new(command));
    results
}

// A control point held fixed in plan, from where `origin` says.
fn control(id: &str, origin: ControlOrigin) -> ControlSelection {
    ControlSelection {
        point: ControlPoint::fixed_horizontal(id),
        origin,
    }
}

// ---- The Document's list -------------------------------------------------------------

mod survey_jobs {
    use super::*;

    #[test]
    fn a_new_document_has_no_jobs() {
        let document = Document::default();
        assert!(document.survey_jobs().is_empty());
        assert!(document.find_survey_job("job-1").is_none());
        assert_eq!(document.survey_jobs_generation(), 0);
    }

    #[test]
    fn the_commands_door_finds_and_counts_changes_to_the_job_list() {
        let mut document = Document::default();
        let job = SurveyJob {
            id: String::from("job-1"),
            name: String::from("site.gsi"),
            ..SurveyJob::default()
        };
        SurveyJobAccess::jobs(&mut document).push(job);
        SurveyJobAccess::changed(&mut document);
        let Some(found) = document.find_survey_job("job-1") else {
            panic!("job-1 is on the list");
        };
        assert_eq!(found.name, "site.gsi");
        assert_eq!(document.survey_jobs_generation(), 1);
    }
}

// ---- Import --------------------------------------------------------------------------

mod survey_job_import_command {
    use super::*;

    #[test]
    fn importing_keeps_the_whole_job_and_draws_its_points_on_the_chosen_layer() {
        let mut document = Document::default();
        let fake = fake_reduction();
        let request = day_one();
        let id = import_three(&mut document, &fake);

        assert!(!id.is_empty());
        assert_eq!(document.survey_jobs().len(), 1);
        let job = &document.survey_jobs()[0];
        assert_eq!(job.id, id);
        // Everything the request carried is kept as it was...
        assert_eq!(job.source_bytes, request.job.source_bytes);
        assert_eq!(job.sibling_files, request.job.sibling_files);
        assert_eq!(job.settings, request.job.settings);
        assert_eq!(job.format_id, "leica-gsi");
        assert_eq!(job.layer, "survey/day1");
        // ...the reduction ran with the job's settings on the raw project...
        {
            let fake = fake.lock().unwrap();
            assert_eq!(fake.calls, 1); // once: validate's result is reused by execute
            assert_eq!(fake.last_settings, request.job.settings);
            assert_eq!(fake.last_raw.name, "day one");
        }
        // ...and its report is the job's, rendered.
        assert_eq!(job.report_created_utc, "2026-09-24T10:00:00Z");
        assert_eq!(job.imported_utc, "2026-09-24T10:00:00Z");
        assert!(!job.report_text.is_empty());
        assert!(!job.report_html.is_empty());

        // Three points on the job's layer, the survey model's (N, E) as (x, y).
        assert_eq!(job.created_entities.len(), 3);
        assert_eq!(job.placed_points.len(), 3);
        for placed in &job.placed_points {
            let Some(entity) = document.model().entities.find(placed.entity) else {
                panic!("{} is drawn", placed.point_id);
            };
            assert_eq!(entity.layer, "survey/day1");
            assert_eq!(
                position_of(entity),
                Point2::new(placed.easting, placed.northing)
            );
            assert_eq!(elevation_of(entity), placed.elevation);
        }
        let p102 = point_of(&document, "102");
        assert_eq!(position_of(p102), Point2::new(300_020.0, 6_250_010.0));
        // Absent, not 0.
        assert!(elevation_of(point_of(&document, "103")).is_none());
    }

    #[test]
    fn one_undo_removes_the_points_and_the_job_and_redo_brings_both_back_unchanged() {
        let mut document = Document::default();
        let fake = fake_reduction();
        import_three(&mut document, &fake);
        let imported = document.survey_jobs()[0].clone();
        let generation = document.survey_jobs_generation();

        assert!(document.undo().is_ok());
        assert!(document.survey_jobs().is_empty());
        assert_eq!(document.model().entities.size(), 0);
        assert!(document.survey_jobs_generation() > generation);

        assert!(document.redo().is_ok());
        assert_eq!(document.survey_jobs().len(), 1);
        // Same id, same entity ids, all of it.
        assert_eq!(document.survey_jobs()[0], imported);
        for placed in &imported.placed_points {
            assert!(
                document.model().entities.contains(placed.entity),
                "{}",
                placed.point_id
            );
        }
        assert_eq!(
            fake.lock().unwrap().calls,
            1,
            "redo replays, it does not reduce again"
        );
    }

    #[test]
    fn a_reduction_that_fails_imports_nothing_and_says_why() {
        let mut document = Document::default();
        let fake = fake_reduction();
        fake.lock().unwrap().failure = Some(Error::new(
            ErrorCode::AdjustmentFailure,
            "the network has no control, so it cannot be held",
        ));
        let revision = document.model_revision();
        let command = ImportSurveyJobCommand::new(&document, day_one(), Some(reduction_of(&fake)));
        let error = refused(document.execute(Box::new(command)));
        assert_eq!(error.code, ErrorCode::AdjustmentFailure);
        assert!(document.survey_jobs().is_empty());
        assert_eq!(document.model().entities.size(), 0);
        assert_eq!(document.model_revision(), revision);
    }

    #[test]
    fn a_job_number_is_never_handed_out_twice_even_after_an_undo() {
        let mut document = Document::default();
        let fake = fake_reduction();
        let first = import_three(&mut document, &fake);
        assert!(document.undo().is_ok());
        // The same file again, after the first import was undone: a report or
        // a log line naming the first job must never come to mean this one.
        let second = import_three(&mut document, &fake);
        assert_ne!(first, second);
        fake.lock().unwrap().points.clear(); // a job that draws nothing still takes a number
        let command = ImportSurveyJobCommand::new(&document, day_one(), Some(reduction_of(&fake)));
        let results = command.results();
        must(&mut document, Box::new(command));
        let third = results.job_id();
        let fourth = import_three_points(&mut document, &fake, ExistingPointPolicy::KeepBoth);
        assert_ne!(third, second);
        assert_ne!(fourth, third);
        assert_eq!(document.survey_jobs().len(), 3);
    }

    #[test]
    fn a_control_point_taken_from_the_drawing_is_not_drawn_again() {
        let mut document = Document::default();
        let held = draw_foreign_point(&mut document, "CP1", 6_249_990.0, 299_990.0);
        let fake = fake_reduction();
        fake.lock().unwrap().points = points_of(&[
            ("CP1", at(6_249_990.0, 299_990.0, Some(5.0))),
            ("201", at(6_250_001.0, 300_001.0, None)),
        ]);
        let mut request = day_one();
        request
            .job
            .settings
            .control
            .push(control("CP1", ControlOrigin::Drawing));
        let command = ImportSurveyJobCommand::new(&document, request, Some(reduction_of(&fake)));
        must(&mut document, Box::new(command));
        let job = &document.survey_jobs()[0];
        assert_eq!(job.placed_points.len(), 1);
        assert_eq!(job.placed_points[0].point_id, "201");
        // CP1 once, and 201.
        assert_eq!(
            drawing_survey_points(&document, &SurveyImportOptions::default()).len(),
            2
        );
        assert!(document.model().entities.contains(held));
    }

    // A file that gives no coordinates for the point it is held at - a
    // traverse whose first station is taken from the drawing - leaves that
    // point among the reduction's unpositioned ones, since the drawing, not
    // the reduction, placed it. It has a position, so it is not one of the
    // points the import says it could not draw for want of one: that sentence
    // once counted it, and told the person to reduce the observations to place
    // a point already held.
    #[test]
    fn a_control_point_taken_from_the_drawing_is_not_counted_as_having_no_position() {
        let mut document = Document::default();
        draw_foreign_point(&mut document, "CP1", 6_249_990.0, 299_990.0);
        let fake = fake_reduction();
        {
            let mut fake = fake.lock().unwrap();
            fake.points = points_of(&[("201", at(6_250_001.0, 300_001.0, None))]);
            fake.unpositioned = vec![String::from("CP1"), String::from("205")];
        }
        let mut request = day_one();
        request
            .job
            .settings
            .control
            .push(control("CP1", ControlOrigin::Drawing));
        let command = ImportSurveyJobCommand::new(&document, request, Some(reduction_of(&fake)));
        let results = command.results();
        must(&mut document, Box::new(command));
        let Some(report) = results.report() else {
            panic!("the import has its report");
        };
        let unplaced: Vec<&str> = report
            .warnings
            .iter()
            .map(|warning| warning.text.as_str())
            .filter(|text| text.contains("named in the source with no coordinates"))
            .collect();
        // 205 alone: the file names it, and nothing placed it.
        assert_eq!(unplaced.len(), 1);
        assert!(
            unplaced[0].starts_with("1 point(s) are named"),
            "{}",
            unplaced[0]
        );
        assert_eq!(document.survey_jobs()[0].placed_points.len(), 1);
    }

    #[test]
    fn a_point_the_drawing_already_has_follows_the_chosen_policy_and_is_reported() {
        let mut document = Document::default();
        draw_foreign_point(&mut document, "102", 1.0, 2.0);
        let fake = fake_reduction();
        fake.lock().unwrap().points =
            points_of(&[("101", at(10.0, 20.0, None)), ("102", at(11.0, 21.0, None))]);

        let command = ImportSurveyJobCommand::new(&document, day_one(), Some(reduction_of(&fake)));
        let error = refused(document.execute(Box::new(command)));
        assert_eq!(error.code, ErrorCode::AlreadyExists);
        assert!(document.survey_jobs().is_empty());

        let skip = SurveyJobImport {
            existing_points: ExistingPointPolicy::Skip,
            ..day_one()
        };
        let command = ImportSurveyJobCommand::new(&document, skip, Some(reduction_of(&fake)));
        let results = command.results();
        must(&mut document, Box::new(command));
        let job = &document.survey_jobs()[0];
        assert_eq!(job.placed_points.len(), 1);
        assert_eq!(job.placed_points[0].point_id, "101");
        // The skip is in the report of what happened to the data.
        assert!(results.report().is_some());
        assert!(has_warning(&results.report(), &["not imported", "102"]));
    }

    // A field file the reader accepts (it takes up to 1 GiB) but the project
    // could never save - SQLite holds at most 1,000,000,000 bytes in a value or
    // a row - is refused at the import, with a sentence naming the file,
    // before the reduction runs and before anything is drawn. Accepted, it
    // would make every later save of the whole project fail. The bytes are
    // zeroed pages the allocator maps and never touches, so the vector costs
    // address space, not memory.
    #[test]
    fn a_field_file_too_large_for_the_project_is_refused_before_anything_is_done() {
        let mut document = Document::default();
        let fake = fake_reduction();
        fake.lock().unwrap().points = points_of(&[("101", at(6_250_000.0, 300_000.0, Some(10.0)))]);
        let mut request = day_one();
        request.job.source_bytes = vec![0; 1_020_000_000];
        let revision = document.model_revision();
        let command = ImportSurveyJobCommand::new(&document, request, Some(reduction_of(&fake)));
        let error = refused(document.execute(Box::new(command)));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("DAY1.GSI"), "{}", error.message);
        assert!(error.message.contains("too large"), "{}", error.message);
        assert_eq!(
            fake.lock().unwrap().calls,
            0,
            "refused before the reduction"
        );
        assert!(document.survey_jobs().is_empty());
        assert_eq!(document.model().entities.size(), 0);
        assert_eq!(document.model_revision(), revision);
    }

    // validate() can see only the file; the report and the point lists come
    // with execute(). A file exactly as large as a job may be, once the rest
    // of what the request holds is counted, passes the first check - and the
    // job it makes does not. It is refused whole: nothing left drawn, no job
    // on the list.
    #[test]
    fn a_job_its_report_and_points_take_past_what_a_project_holds_is_refused_whole() {
        let mut document = Document::default();
        let fake = fake_reduction();
        fake.lock().unwrap().points = points_of(&[("101", at(6_250_000.0, 300_000.0, Some(10.0)))]);
        let mut request = day_one();
        let rest = SurveyJob {
            source_bytes: Vec::new(),
            ..request.job.clone()
        };
        let file = ProjectStore::MAX_SURVEY_JOB_BYTES - ProjectStore::survey_job_row_bytes(&rest);
        request.job.source_bytes = vec![0; usize::try_from(file).unwrap_or(usize::MAX)];
        assert_eq!(
            ProjectStore::survey_job_row_bytes(&request.job),
            ProjectStore::MAX_SURVEY_JOB_BYTES
        );

        let command = ImportSurveyJobCommand::new(&document, request, Some(reduction_of(&fake)));
        let error = refused(document.execute(Box::new(command)));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.contains("DAY1.GSI is too large"),
            "{}",
            error.message
        );
        assert_eq!(fake.lock().unwrap().calls, 1); // past the first check, refused at the second
        assert!(document.survey_jobs().is_empty());
        assert_eq!(document.model().entities.size(), 0);
    }

    #[test]
    fn a_job_survives_saving_and_opening_and_a_new_drawing_has_none() {
        let scratch = ScratchDirectory::new("survey-job-save");
        let project = scratch.path.join("site.katana");
        let saved = {
            let mut document = Document::default();
            let fake = fake_reduction();
            import_three(&mut document, &fake);
            let saved = document.survey_jobs()[0].clone();
            expect_ok(document.save_as(&project));
            assert_eq!(
                document.survey_jobs()[0],
                saved,
                "the save lends the jobs, not keeps them"
            );
            // Twice: the tables are rewritten, not appended to.
            expect_ok(document.save());
            saved
        };
        let mut reopened = Document::default();
        expect_ok(reopened.open(&project));
        assert_eq!(reopened.survey_jobs().len(), 1);
        assert_eq!(reopened.survey_jobs()[0], saved);
        reopened.new_document();
        assert!(reopened.survey_jobs().is_empty());
    }

    // The import command's own reduction is reduce_for_drawing, so the
    // refusal reaches every import that does not hand it another - the
    // SURVEY IMPORT line's - with the drawing as it was.
    #[test]
    fn its_reduction_refuses_an_id_the_drawing_has_at_two_places() {
        let mut document = Document::default();
        draw_alongside(&mut document, "CP1", 5_000_000.0, 500_000.0, Some(100.0));
        draw_alongside(&mut document, "CP1", 5_000_000.0, 600_000.0, Some(100.0));
        let entities = document.model().entities.size();
        let request = SurveyJobImport {
            job: SurveyJob {
                settings: holding_from_the_drawing("CP1"),
                ..day_one().job
            },
            raw: naming_cp1(),
            context: expect_ok(reduction_context_for(&document)),
            ..day_one()
        };
        let command = ImportSurveyJobCommand::new(&document, request, Some(default_reduction()));
        let error = refused(document.execute(Box::new(command)));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error
                .message
                .contains("Control point CP1 is on the drawing 2 times"),
            "{}",
            error.describe()
        );
        assert!(document.survey_jobs().is_empty());
        assert_eq!(document.model().entities.size(), entities);
    }
}

// ---- Re-adjust -----------------------------------------------------------------------

fn readjustment_of(document: &Document, policy: HandEditPolicy) -> SurveyJobReadjustment {
    let job = &document.survey_jobs()[0];
    SurveyJobReadjustment {
        job_id: job.id.clone(),
        settings: job.settings.clone(),
        hand_edits: policy,
        ..SurveyJobReadjustment::default()
    }
}

// The survey point entity whose `property` reads `value`.
fn entity_with<'a>(document: &'a Document, property: &str, value: &str) -> Option<&'a Entity> {
    let entities = &document.model().entities;
    let mut found = None;
    for id in entities.ids() {
        if let Some(entity) = entities.find(id)
            && entity
                .properties
                .get(property)
                .is_some_and(|field| katana_entity::entity::property_to_string(field) == value)
        {
            found = Some(entity);
        }
    }
    found
}

// An import drawn with every drawing option away from its default: a layer
// per field code, and the point's fields under other property names.
fn import_drawn_per_code() -> SurveyJobImport {
    let mut request = day_one();
    request.import_options = SurveyImportOptions {
        layer_per_code: true,
        point_number_property: String::from("ptno"),
        code_property: String::from("fieldcode"),
        description_property: String::from("remark"),
        record_source: false,
        ..request.import_options
    };
    request
}

// Every point the fake reduction computes has code PEG, so with a layer per
// code each one belongs on survey/day1/PEG, numbered under "ptno".
#[track_caller]
fn expect_drawn_per_code(document: &Document, point_id: &str) {
    // Drawn under the default name: found there to show where it went.
    let entity = entity_with(document, "ptno", point_id)
        .or_else(|| entity_with(document, "point", point_id));
    let Some(entity) = entity else {
        panic!("{point_id} is drawn");
    };
    assert_eq!(entity.layer, "survey/day1/PEG", "{point_id}");
    assert!(!entity.properties.contains_key("point"), "{point_id}");
    let Some(code) = entity.properties.get("fieldcode") else {
        panic!("{point_id} has its code under fieldcode");
    };
    assert_eq!(
        katana_entity::entity::property_to_string(code),
        "PEG",
        "{point_id}"
    );
}

mod survey_job_readjust_command {
    use super::*;

    #[test]
    fn readjusting_moves_only_the_jobs_points_and_one_undo_puts_everything_back() {
        let mut document = Document::default();
        let foreign = draw_foreign_point(&mut document, "900", 6_250_100.0, 300_100.0);
        let fake = fake_reduction();
        let id = import_three(&mut document, &fake);
        let before = document.survey_jobs()[0].clone();
        let foreign_before = document.model().entities.find(foreign).cloned();
        let entities_before = entities_of(&document);

        // The new settings shift 101 and 102 by a few millimetres and give 103
        // a height.
        fake.lock().unwrap().points = points_of(&[
            ("101", at(6_250_000.004, 300_000.002, Some(10.001))),
            ("102", at(6_250_010.0, 300_020.0, Some(11.0))),
            ("103", at(6_250_029.997, 300_005.0, Some(3.25))),
            ("900", at(1.0, 1.0, Some(1.0))), // the job's reduction knows a 900 too
        ]);
        let request = SurveyJobReadjustment {
            job_id: id.clone(),
            settings: ReductionSettings {
                method: AdjustmentMethod::Network,
                traverse_rule: TraverseRule::LeastSquares,
                ..before.settings.clone()
            },
            context: SurveyJobContext {
                created_utc: String::from("2026-09-25T08:00:00Z"),
                ..SurveyJobContext::default()
            },
            parser_version: String::from("1.1"),
            ..SurveyJobReadjustment::default()
        };
        let calls = reads();
        let results = readjust_with(
            &mut document,
            request.clone(),
            reader_for(&calls, before.source_bytes.clone()),
            &fake,
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        {
            let fake = fake.lock().unwrap();
            assert_eq!(fake.last_raw.name, "re-read DAY1.GSI");
            // The previous run handed to the reduction is what the job placed.
            assert_eq!(fake.last_previous.len(), 3);
            assert_eq!(fake.last_previous[0].id, before.placed_points[0].point_id);
            assert_eq!(
                fake.last_previous[0].northing,
                before.placed_points[0].northing
            );
        }

        let p101 = point_of(&document, "101");
        assert_eq!(
            p101.id, before.placed_points[0].entity,
            "moved, not re-created"
        );
        assert_eq!(position_of(p101), Point2::new(300_000.002, 6_250_000.004));
        assert_eq!(elevation_of(p101), Some(10.001));
        assert_eq!(elevation_of(point_of(&document, "103")), Some(3.25));
        assert_eq!(
            document.model().entities.find(foreign).cloned(),
            foreign_before,
            "not the job's point"
        );
        // 900 is somebody else's id: not drawn over, and said so.
        let changes = results.changes();
        assert_eq!(changes.not_drawn, ["900"]);
        assert_eq!(changes.moved, ["101", "103"]);

        let after = &document.survey_jobs()[0];
        assert_eq!(after.settings, request.settings);
        assert_eq!(after.parser_version, "1.1");
        assert_eq!(after.report_created_utc, "2026-09-25T08:00:00Z");
        assert_eq!(after.source_bytes, before.source_bytes);
        assert_eq!(after.placed_points[0].northing, 6_250_000.004);

        // ONE undo: coordinates, settings, report - exactly.
        assert!(document.undo().is_ok());
        assert_eq!(document.survey_jobs()[0], before);
        assert_eq!(entities_of(&document), entities_before);

        assert!(document.redo().is_ok());
        assert_eq!(document.survey_jobs()[0].settings, request.settings);
        assert_eq!(
            position_of(point_of(&document, "101")),
            Point2::new(300_000.002, 6_250_000.004)
        );
    }

    #[test]
    fn a_point_moved_by_hand_is_reported_and_kept_or_overwritten_as_asked() {
        for policy in [HandEditPolicy::Keep, HandEditPolicy::Overwrite] {
            let trace = policy.name();
            let mut document = Document::default();
            let fake = fake_reduction();
            let id = import_three(&mut document, &fake);
            let p102 = point_of(&document, "102").id;
            // The person nudges 102 half a metre east.
            must(
                &mut document,
                move_entities(vec![p102], Vec2::new(0.5, 0.0)),
            );

            fake.lock().unwrap().points.insert(
                String::from("102"),
                at(6_250_010.010, 300_020.010, Some(11.0)),
            );
            let request = SurveyJobReadjustment {
                job_id: id,
                hand_edits: policy,
                ..SurveyJobReadjustment::default()
            };
            let calls = reads();
            let bytes = stored_bytes(&document);
            let results = readjust_with(&mut document, request, reader_for(&calls, bytes), &fake);

            assert_eq!(results.changes().edited_by_hand, ["102"], "{trace}");
            assert!(results.report().is_some(), "{trace}");
            assert!(
                has_warning(&results.report(), &["Point 102", "by hand"]),
                "{trace}"
            );
            let Some(moved) = document.model().entities.find(p102) else {
                panic!("102 is still there");
            };
            if policy == HandEditPolicy::Keep {
                assert_eq!(
                    position_of(moved),
                    Point2::new(300_020.5, 6_250_010.0),
                    "{trace}"
                );
            } else {
                assert_eq!(
                    position_of(moved),
                    Point2::new(300_020.010, 6_250_010.010),
                    "{trace}"
                );
            }
        }
    }

    #[test]
    fn a_point_deleted_by_hand_stays_deleted_unless_overwrite_draws_it_again() {
        for policy in [HandEditPolicy::Keep, HandEditPolicy::Overwrite] {
            let trace = policy.name();
            let mut document = Document::default();
            let fake = fake_reduction();
            let id = import_three(&mut document, &fake);
            let p103 = point_of(&document, "103").id;
            must(&mut document, delete_entities(vec![p103]));

            let request = SurveyJobReadjustment {
                job_id: id,
                hand_edits: policy,
                ..SurveyJobReadjustment::default()
            };
            let calls = reads();
            let bytes = stored_bytes(&document);
            let results = readjust_with(&mut document, request, reader_for(&calls, bytes), &fake);
            let changes = results.changes();
            assert_eq!(changes.deleted_by_hand, ["103"], "{trace}");
            let again = point_entity(&document, "103");
            if policy == HandEditPolicy::Keep {
                assert!(again.is_none(), "{trace}");
                assert!(changes.created.is_empty(), "{trace}");
            } else {
                let Some(again) = again else {
                    panic!("103 is drawn again");
                };
                assert_eq!(again.layer, "survey/day1", "{trace}");
                assert_eq!(changes.created, ["103"], "{trace}");
            }
            assert_eq!(document.survey_jobs()[0].placed_points.len(), 3, "{trace}");
        }
    }

    #[test]
    fn a_point_no_longer_computed_is_deleted_and_a_new_one_is_drawn_on_the_jobs_layer() {
        let mut document = Document::default();
        let fake = fake_reduction();
        let id = import_three(&mut document, &fake);
        let old103 = point_of(&document, "103").id;
        {
            let mut fake = fake.lock().unwrap();
            fake.points.remove("103");
            fake.points
                .insert(String::from("104"), at(6_250_040.0, 300_040.0, Some(12.0)));
        }

        let request = SurveyJobReadjustment {
            job_id: id,
            ..SurveyJobReadjustment::default()
        };
        let calls = reads();
        let bytes = stored_bytes(&document);
        let results = readjust_with(&mut document, request, reader_for(&calls, bytes), &fake);
        assert_eq!(results.changes().removed, ["103"]);
        assert_eq!(results.changes().created, ["104"]);
        assert!(!document.model().entities.contains(old103));
        let p104 = point_of(&document, "104");
        assert_eq!(p104.layer, "survey/day1");
        let job = &document.survey_jobs()[0];
        assert_eq!(job.placed_points.len(), 3);
        assert_eq!(job.placed_points[2].point_id, "104");
        assert_eq!(job.placed_points[2].entity, p104.id);

        assert!(document.undo().is_ok());
        assert!(document.model().entities.contains(old103));
        assert!(point_entity(&document, "104").is_none());
    }

    // A control mark the job drew itself - held from the FILE at the import -
    // and then held FROM THE DRAWING by a re-adjustment. The new run took the
    // point where the drawing has it, so there is nothing to move it to;
    // deleting it would take away the very mark the adjustment was just held
    // to, and the next "Edit adjustment" with the same settings would find no
    // CP1 to hold.
    #[test]
    fn a_job_point_the_new_settings_hold_from_the_drawing_is_left_where_it_is() {
        for policy in [HandEditPolicy::Keep, HandEditPolicy::Overwrite] {
            let trace = policy.name();
            let mut document = Document::default();
            let fake = fake_reduction();
            fake.lock().unwrap().points = points_of(&[
                ("CP1", at(6_249_990.0, 299_990.0, Some(5.0))),
                ("201", at(6_250_001.0, 300_001.0, None)),
            ]);
            let mut request = day_one();
            request
                .job
                .settings
                .control
                .push(control("CP1", ControlOrigin::File));
            let import = ImportSurveyJobCommand::new(&document, request, Some(reduction_of(&fake)));
            let imported = import.results();
            must(&mut document, Box::new(import));
            let id = imported.job_id();
            // CP1 is the job's.
            assert_eq!(document.survey_jobs()[0].placed_points.len(), 2, "{trace}");
            let cp1_before = point_of(&document, "CP1").clone();

            let mut readjust = SurveyJobReadjustment {
                job_id: id,
                hand_edits: policy,
                settings: document.survey_jobs()[0].settings.clone(),
                ..SurveyJobReadjustment::default()
            };
            readjust.settings.control[0].origin = ControlOrigin::Drawing;
            fake.lock()
                .unwrap()
                .points
                .insert(String::from("201"), at(6_250_001.004, 300_001.0, None));
            // The second run needs CP1 still on the drawing.
            for run in 1..=2 {
                let context = expect_ok(reduction_context_for(&document));
                assert_eq!(context.drawing_points.len(), 2, "{trace} {run}");
                readjust.context = context;
                let calls = reads();
                let bytes = stored_bytes(&document);
                let results = readjust_with(
                    &mut document,
                    readjust.clone(),
                    reader_for(&calls, bytes),
                    &fake,
                );
                let changes = results.changes();
                assert!(changes.removed.is_empty(), "{trace} {run}");
                assert!(changes.edited_by_hand.is_empty(), "{trace} {run}");
                if run == 1 {
                    assert_eq!(changes.moved, ["201"], "{trace} {run}");
                } else {
                    assert!(changes.moved.is_empty(), "{trace} {run}");
                }

                // The mark the adjustment was held to.
                let Some(cp1) = point_entity(&document, "CP1") else {
                    panic!("CP1 is still on the drawing ({trace} {run})");
                };
                assert_eq!(*cp1, cp1_before, "{trace} {run}");
                let job = &document.survey_jobs()[0];
                assert_eq!(job.placed_points.len(), 2, "{trace} {run}");
                // Still the job's, for Remove.
                let Some(placed) = placed_point(job, "CP1") else {
                    panic!("CP1 is still the job's ({trace} {run})");
                };
                assert_eq!(placed.entity, cp1_before.id, "{trace} {run}");
            }
        }
    }

    // The same mark, moved by hand before it is held from the drawing: the
    // new run held it where the person put it, so under either policy that is
    // where it stays - "overwrite" has no other coordinates to give it - and
    // the report says why it did not move.
    #[test]
    fn a_job_point_moved_by_hand_and_then_held_from_the_drawing_stays_where_it_was_put() {
        for policy in [HandEditPolicy::Keep, HandEditPolicy::Overwrite] {
            let trace = policy.name();
            let mut document = Document::default();
            let fake = fake_reduction();
            fake.lock().unwrap().points = points_of(&[
                ("CP1", at(6_249_990.0, 299_990.0, Some(5.0))),
                ("201", at(6_250_001.0, 300_001.0, None)),
            ]);
            let mut request = day_one();
            request
                .job
                .settings
                .control
                .push(control("CP1", ControlOrigin::File));
            let import = ImportSurveyJobCommand::new(&document, request, Some(reduction_of(&fake)));
            let imported = import.results();
            must(&mut document, Box::new(import));
            let cp1 = point_of(&document, "CP1").id;
            // The person re-marks CP1 0.25 m north: (299 990, 6 249 990.25).
            must(
                &mut document,
                move_entities(vec![cp1], Vec2::new(0.0, 0.25)),
            );

            let mut readjust = SurveyJobReadjustment {
                job_id: imported.job_id(),
                hand_edits: policy,
                settings: document.survey_jobs()[0].settings.clone(),
                ..SurveyJobReadjustment::default()
            };
            readjust.settings.control[0].origin = ControlOrigin::Drawing;
            readjust.context = expect_ok(reduction_context_for(&document));
            let calls = reads();
            let bytes = stored_bytes(&document);
            let results = readjust_with(&mut document, readjust, reader_for(&calls, bytes), &fake);

            let Some(moved) = document.model().entities.find(cp1) else {
                panic!("CP1 is still there");
            };
            assert_eq!(
                position_of(moved),
                Point2::new(299_990.0, 6_249_990.25),
                "{trace}"
            );
            let changes = results.changes();
            assert!(changes.removed.is_empty(), "{trace}");
            assert_eq!(changes.edited_by_hand, ["CP1"], "{trace}");
            assert!(
                has_warning(&results.report(), &["Point CP1", "from the drawing"]),
                "{trace}"
            );
            // Still recorded where the JOB put it, so the edit stays visible as
            // one: a later run that computes CP1 again sees a hand-moved mark.
            let Some(placed) = placed_point(&document.survey_jobs()[0], "CP1") else {
                panic!("CP1 is still the job's");
            };
            assert_eq!(placed.northing, 6_249_990.0, "{trace}");
        }
    }

    // The job's later points belong with its first ones: a point a
    // re-adjustment computes for the first time, and one deleted by hand that
    // Overwrite draws again, go where and as the import put the others - on
    // the PEG layer, so switching that layer off hides all of the job's pegs,
    // not just the first.
    #[test]
    fn a_point_drawn_by_a_readjustment_is_drawn_as_the_import_drew_the_others() {
        let mut document = Document::default();
        let fake = fake_reduction();
        fake.lock().unwrap().points = points_of(&[("101", at(6_250_000.0, 300_000.0, Some(10.0)))]);
        let command = ImportSurveyJobCommand::new(
            &document,
            import_drawn_per_code(),
            Some(reduction_of(&fake)),
        );
        must(&mut document, Box::new(command));
        expect_drawn_per_code(&document, "101");

        // A new point...
        fake.lock()
            .unwrap()
            .points
            .insert(String::from("102"), at(6_250_010.0, 300_020.0, Some(11.0)));
        let calls = reads();
        let bytes = stored_bytes(&document);
        let request = readjustment_of(&document, HandEditPolicy::Keep);
        let first = readjust_with(&mut document, request, reader_for(&calls, bytes), &fake);
        assert_eq!(first.changes().created, ["102"]);
        expect_drawn_per_code(&document, "102");

        // ...and a deleted one drawn again.
        let Some(p101) = entity_with(&document, "ptno", "101") else {
            panic!("101 is drawn");
        };
        let p101 = p101.id;
        must(&mut document, delete_entities(vec![p101]));
        let bytes = stored_bytes(&document);
        let request = readjustment_of(&document, HandEditPolicy::Overwrite);
        let second = readjust_with(&mut document, request, reader_for(&calls, bytes), &fake);
        assert_eq!(second.changes().created, ["101"]);
        expect_drawn_per_code(&document, "101");
    }

    // The drawing options are part of the job, so they are saved with it: a
    // re-adjustment in a later session draws as the import did too.
    #[test]
    fn the_imports_drawing_options_survive_saving_and_opening() {
        let scratch = ScratchDirectory::new("survey-job-options");
        let project = scratch.path.join("site.katana");
        let fake = fake_reduction();
        fake.lock().unwrap().points = points_of(&[("101", at(6_250_000.0, 300_000.0, Some(10.0)))]);
        {
            let mut document = Document::default();
            let command = ImportSurveyJobCommand::new(
                &document,
                import_drawn_per_code(),
                Some(reduction_of(&fake)),
            );
            must(&mut document, Box::new(command));
            expect_ok(document.save_as(&project));
        }
        let mut reopened = Document::default();
        expect_ok(reopened.open(&project));
        fake.lock()
            .unwrap()
            .points
            .insert(String::from("102"), at(6_250_010.0, 300_020.0, Some(11.0)));
        let calls = reads();
        let bytes = stored_bytes(&reopened);
        let request = readjustment_of(&reopened, HandEditPolicy::Keep);
        readjust_with(&mut reopened, request, reader_for(&calls, bytes), &fake);
        expect_drawn_per_code(&reopened, "101");
        expect_drawn_per_code(&reopened, "102");
    }

    // The options come back from a project file, which may be damaged or
    // edited by hand: each malformed form ends the run before the reduction,
    // with a sentence naming the job, and changes nothing - never a guess at
    // the options.
    #[test]
    fn drawing_options_that_cannot_be_read_end_the_run_and_change_nothing() {
        let cases = [
            ("layer-per-code=true\n", ErrorCode::ParseFailure), // no version line
            ("katana-survey-import-options=x\n", ErrorCode::ParseFailure), // not a number
            ("katana-survey-import-options=0\n", ErrorCode::ParseFailure),
            (
                "katana-survey-import-options=1\nlayer-per-code\n",
                ErrorCode::ParseFailure,
            ),
            (
                "katana-survey-import-options=1\nlayer-per-code=maybe\n",
                ErrorCode::ParseFailure,
            ),
            (
                "katana-survey-import-options=1\ncode-property=%4\n",
                ErrorCode::ParseFailure,
            ),
            (
                "katana-survey-import-options=1\ncode-property=%G1\n",
                ErrorCode::ParseFailure,
            ),
            // The C++'s "\0\xff\r\n=": a first line with no '='. The job's
            // text is a String here, which holds the UTF-8 half of it.
            ("\0\u{ff}\r\n=", ErrorCode::ParseFailure),
            (
                "katana-survey-import-options=2\nlayer-per-code=true\n",
                ErrorCode::Unsupported,
            ),
        ];
        for (text, code) in cases {
            let mut document = Document::default();
            let fake = fake_reduction();
            let id = import_three(&mut document, &fake);
            SurveyJobAccess::jobs(&mut document)[0].import_options = String::from(text);
            let before = document.survey_jobs()[0].clone();
            let revision = document.model_revision();
            let calls = fake.lock().unwrap().calls;
            let read = reads();
            let command = ReadjustSurveyJobCommand::new(
                &document,
                readjustment_of(&document, HandEditPolicy::Overwrite),
                Some(reader_for(&read, before.source_bytes.clone())),
                Some(reduction_of(&fake)),
            );
            let error = refused(document.execute(Box::new(command)));
            assert_eq!(error.code, code, "{text:?}");
            assert!(error.message.contains(&id), "{text:?}: {}", error.message);
            assert_eq!(
                fake.lock().unwrap().calls,
                calls,
                "{text:?}: refused before the reduction"
            );
            assert_eq!(document.survey_jobs()[0], before, "{text:?}");
            assert_eq!(document.model_revision(), revision, "{text:?}");
        }
    }

    // A key a later build added is skipped, as the reduction settings' is,
    // and the rest still apply. A property name holding a line break and a
    // '%' comes back as it was: the text form encodes them.
    #[test]
    fn a_drawing_option_a_later_build_added_is_skipped_and_the_rest_still_apply() {
        let mut document = Document::default();
        let fake = fake_reduction();
        fake.lock().unwrap().points = points_of(&[("101", at(6_250_000.0, 300_000.0, Some(10.0)))]);
        let mut request = import_drawn_per_code();
        request.import_options.point_number_property = String::from("pt%\nno");
        let command = ImportSurveyJobCommand::new(&document, request, Some(reduction_of(&fake)));
        must(&mut document, Box::new(command));
        assert!(entity_with(&document, "pt%\nno", "101").is_some());
        SurveyJobAccess::jobs(&mut document)[0]
            .import_options
            .push_str("label-height=2.5\n");

        fake.lock()
            .unwrap()
            .points
            .insert(String::from("102"), at(6_250_010.0, 300_020.0, Some(11.0)));
        let calls = reads();
        let bytes = stored_bytes(&document);
        let request = readjustment_of(&document, HandEditPolicy::Keep);
        readjust_with(&mut document, request, reader_for(&calls, bytes), &fake);
        let Some(p102) = entity_with(&document, "pt%\nno", "102") else {
            panic!("102 is numbered under the stored name");
        };
        assert_eq!(p102.layer, "survey/day1/PEG");
    }

    // A job exactly as large as a project keeps, re-adjusted into one more
    // point: the job would no longer fit, so the re-adjustment is refused and
    // changes nothing, rather than leaving every later save of the drawing to
    // fail.
    #[test]
    fn a_readjustment_that_would_take_the_job_past_what_a_project_holds_is_refused() {
        let mut document = Document::default();
        let fake = fake_reduction();
        let id = import_three(&mut document, &fake);
        {
            let job = &mut SurveyJobAccess::jobs(&mut document)[0];
            // No stored report to start with, so the re-adjustment can only
            // make the job larger (a report where there was none, and one more
            // point). Left in place, the import's report can be longer than
            // the re-adjustment's (measured on main at 75acaf0: the job came
            // out 393 bytes smaller even with its fourth point), and a job
            // that still fits is rightly accepted.
            job.report_text.clear();
            job.report_html.clear();
            let rest = ProjectStore::survey_job_row_bytes(job) - job.source_bytes.len() as u64;
            let size = ProjectStore::MAX_SURVEY_JOB_BYTES - rest;
            job.source_bytes = vec![0; usize::try_from(size).unwrap_or(usize::MAX)];
            assert_eq!(
                ProjectStore::survey_job_row_bytes(job),
                ProjectStore::MAX_SURVEY_JOB_BYTES
            );
        }
        let job = &document.survey_jobs()[0];
        let settings = job.settings.clone();
        let report_text = job.report_text.clone();
        let placed = job.placed_points.clone();
        let created = job.created_entities.clone();
        let entities_before = entities_of(&document);

        {
            let mut fake = fake.lock().unwrap();
            fake.points
                .insert(String::from("104"), at(6_250_040.0, 300_040.0, Some(12.0)));
            fake.points.insert(
                String::from("101"),
                at(6_250_000.004, 300_000.0, Some(10.0)),
            );
        }
        let mut request = readjustment_of(&document, HandEditPolicy::Keep);
        request.settings.method = AdjustmentMethod::Network;
        // The stored bytes are not compared here: that would read 900 MB.
        let reader: SurveyJobReader = Box::new(|_: &SurveyJob| Ok(raw_project("re-read")));
        let command = ReadjustSurveyJobCommand::new(
            &document,
            request,
            Some(reader),
            Some(reduction_of(&fake)),
        );
        let error = refused(document.execute(Box::new(command)));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        let expected = format!("survey job {id} (DAY1.GSI) is too large");
        assert!(error.message.contains(&expected), "{}", error.message);

        let after = &document.survey_jobs()[0];
        assert_eq!(after.settings, settings);
        assert_eq!(after.report_text, report_text);
        assert_eq!(after.placed_points, placed);
        assert_eq!(after.created_entities, created);
        assert_eq!(
            entities_of(&document),
            entities_before,
            "101 not moved, 104 not drawn"
        );
    }

    #[test]
    fn a_reader_or_reduction_that_fails_changes_nothing() {
        let mut document = Document::default();
        let fake = fake_reduction();
        let id = import_three(&mut document, &fake);
        let before = document.survey_jobs()[0].clone();
        let revision = document.model_revision();

        let mut request = SurveyJobReadjustment {
            job_id: id,
            ..SurveyJobReadjustment::default()
        };
        let broken: SurveyJobReader = Box::new(|_: &SurveyJob| {
            Err(Error::new(
                ErrorCode::ParseFailure,
                "record 12 of DAY1.GSI cannot be read",
            ))
        });
        let command = ReadjustSurveyJobCommand::new(
            &document,
            request.clone(),
            Some(broken),
            Some(reduction_of(&fake)),
        );
        let error = refused(document.execute(Box::new(command)));
        assert_eq!(error.code, ErrorCode::ParseFailure);

        fake.lock().unwrap().failure =
            Some(Error::new(ErrorCode::AdjustmentFailure, "datum defect"));
        let calls = reads();
        let command = ReadjustSurveyJobCommand::new(
            &document,
            request.clone(),
            Some(reader_for(&calls, before.source_bytes.clone())),
            Some(reduction_of(&fake)),
        );
        let error = refused(document.execute(Box::new(command)));
        assert_eq!(error.code, ErrorCode::AdjustmentFailure);

        request.job_id = String::from("job-does-not-exist");
        let command = ReadjustSurveyJobCommand::new(
            &document,
            request,
            Some(reader_for(&calls, before.source_bytes.clone())),
            Some(reduction_of(&fake)),
        );
        let error = refused(document.execute(Box::new(command)));
        assert_eq!(error.code, ErrorCode::NotFound);

        assert_eq!(document.survey_jobs()[0], before);
        assert_eq!(document.model_revision(), revision);
    }
}

// ---- Remove --------------------------------------------------------------------------

mod survey_job_remove_command {
    use super::*;

    #[test]
    fn removing_a_job_with_its_points_deletes_both_and_undo_restores_both() {
        let mut document = Document::default();
        let fake = fake_reduction();
        let id = import_three(&mut document, &fake);
        let before = document.survey_jobs()[0].clone();
        let foreign = draw_foreign_point(&mut document, "900", 1.0, 2.0);

        let command = RemoveSurveyJobCommand::new(&document, id, true);
        must(&mut document, Box::new(command));
        assert!(document.survey_jobs().is_empty());
        for &created in &before.created_entities {
            assert!(!document.model().entities.contains(created));
        }
        assert!(document.model().entities.contains(foreign));

        assert!(document.undo().is_ok());
        assert_eq!(document.survey_jobs().len(), 1);
        assert_eq!(document.survey_jobs()[0], before);
        for &created in &before.created_entities {
            assert!(document.model().entities.contains(created));
        }
    }

    #[test]
    fn removing_a_job_without_its_points_leaves_the_points_in_the_drawing() {
        let mut document = Document::default();
        let fake = fake_reduction();
        let id = import_three(&mut document, &fake);
        let created = document.survey_jobs()[0].created_entities.clone();
        let command = RemoveSurveyJobCommand::new(&document, id.clone(), false);
        must(&mut document, Box::new(command));
        assert!(document.survey_jobs().is_empty());
        for &entity in &created {
            assert!(document.model().entities.contains(entity));
        }
        let command = RemoveSurveyJobCommand::new(&document, id, false);
        let missing = refused(document.execute(Box::new(command)));
        assert_eq!(missing.code, ErrorCode::NotFound);
    }
}

// ---- The drawing's part of the reduction -----------------------------------------------

mod survey_job_context {
    use super::*;

    fn on_system(system: &str) -> Document {
        let mut document = Document::default();
        let mut metadata = document.metadata().clone();
        metadata.coordinate_system = String::from(system);
        document.set_metadata(metadata);
        document
    }

    #[test]
    fn the_drawings_survey_points_are_offered_as_control_by_their_ids() {
        let mut document = Document::default();
        draw_foreign_point(&mut document, "CP1", 6_249_990.0, 299_990.0);
        let context = expect_ok(reduction_context_for(&document));
        assert_eq!(context.drawing_points.len(), 1);
        assert_eq!(context.drawing_points[0].id, "CP1");
        assert_eq!(context.drawing_points[0].northing, 6_249_990.0);
        assert_eq!(context.drawing_points[0].easting, 299_990.0);
        assert_eq!(context.drawing_points[0].elevation, Some(5.0));
        // A local drawing: nothing a projection could answer, so nothing is.
        assert!(context.grid_scale_factor.is_none());
        assert!(context.geoid_separation.is_none());
        assert!(context.geocentric_to_grid.is_none());
        assert!(context.geodetic_to_grid.is_none());
    }

    // EPSG:28356, GDA94 / MGA zone 56: Transverse Mercator, k0 = 0.9996, false
    // easting 500 000 m, central meridian 153 E, GRS80.
    //
    // On the central meridian the point scale factor IS k0, by definition of
    // the projection: 0.9996 at E 500 000 whatever the northing.
    //
    // 100 km east of it, at N 6 250 000 (latitude about 33.89 S), by hand:
    //   x = 100 000 / 0.9996 = 100 040.016 m on the ellipsoid
    //   sin(33.89) = 0.557600, e2 = 0.00669438, w = 1 - e2 sin2 = 0.9979186
    //   nu = a / sqrt(w) = 6 378 137 / 0.9989588 = 6 384 785 m
    //   rho = a (1 - e2) / w^1.5 = 6 335 439 / 0.9968795 = 6 355 271 m
    //   rho nu = 4.057704e13 m2
    //   k = k0 (1 + x2 / (2 rho nu) + x4 / (24 rho2 nu2))
    //     = 0.9996 (1 + 1.233211e-4 + 2.53e-9) = 0.9997233
    // The terms left out are below 1e-9; the latitude is known to 0.01 degree,
    // which moves rho nu by under 0.05% and k by under 1e-7.
    #[test]
    fn the_grid_scale_factor_comes_from_the_drawings_projection() {
        let document = on_system("EPSG:28356");
        let context = expect_ok(reduction_context_for(&document));
        let Some(factor) = context.grid_scale_factor.as_ref() else {
            panic!("the projection gives a scale factor");
        };

        let Some(on_meridian) = factor(6_250_000.0, 500_000.0, 0.0) else {
            panic!("the central meridian is in the projection");
        };
        near(on_meridian, 0.9996, 1e-9);

        let Some(east) = factor(6_250_000.0, 600_000.0, 0.0) else {
            panic!("100 km east is in the projection");
        };
        near(east, 0.9997233, 2e-7);

        // Symmetric about the central meridian: 100 km west is the same.
        let Some(west) = factor(6_250_000.0, 400_000.0, 0.0) else {
            panic!("100 km west is in the projection");
        };
        near(west, east, 1e-9);

        // No geoid model and no GNSS frame: left empty, never guessed.
        assert!(context.geoid_separation.is_none());
        assert!(context.geodetic_to_grid.is_none());
    }

    #[test]
    fn the_horizontal_part_of_a_compound_system_gives_the_scale_factor() {
        // MGA zone 56 + AHD height.
        let document = on_system("EPSG:28356+5711");
        let context = expect_ok(reduction_context_for(&document));
        let Some(factor) = context.grid_scale_factor.as_ref() else {
            panic!("the horizontal part gives a scale factor");
        };
        let Some(on_meridian) = factor(6_250_000.0, 500_000.0, 0.0) else {
            panic!("the central meridian is in the projection");
        };
        near(on_meridian, 0.9996, 1e-9);
    }

    #[test]
    fn a_coordinate_system_that_cannot_be_read_is_an_error_not_a_local_drawing() {
        let document = on_system("EPSG:999999");
        let error = refused(reduction_context_for(&document));
        assert!(error.message.contains("EPSG:999999"), "{}", error.message);
    }
}

// ---- The reduction a job on the drawing runs ------------------------------------------

fn survey_point(id: &str, northing: f64, easting: f64, elevation: Option<f64>) -> SurveyPoint {
    SurveyPoint {
        id: String::from(id),
        northing,
        easting,
        elevation,
        ..SurveyPoint::default()
    }
}

// CP1 held from the drawing, fixed in all three.
fn holding_from_the_drawing(id: &str) -> ReductionSettings {
    ReductionSettings {
        control: vec![ControlSelection {
            point: ControlPoint::fixed3d(id),
            origin: ControlOrigin::Drawing,
        }],
        ..ReductionSettings::default()
    }
}

// A file that names CP1 and gives it no coordinates, and gives A some: all a
// reduction of control and entered points needs.
fn naming_cp1() -> SurveyProject {
    SurveyProject {
        points: vec![survey_point("A", 5_000_050.0, 500_050.0, Some(101.0))],
        unpositioned_points: vec![UnpositionedPoint {
            id: String::from("CP1"),
            ..UnpositionedPoint::default()
        }],
        ..SurveyProject::default()
    }
}

// A survey point drawn beside whatever the drawing has, one of its id
// included.
#[track_caller]
fn draw_alongside(
    document: &mut Document,
    id: &str,
    northing: f64,
    easting: f64,
    elevation: Option<f64>,
) {
    let project = SurveyProject {
        points: vec![survey_point(id, northing, easting, elevation)],
        ..SurveyProject::default()
    };
    let options = SurveyImportOptions {
        layer: String::from("control"),
        ..SurveyImportOptions::default()
    };
    let command = expect_ok(import_survey_points(
        document,
        &project,
        &options,
        ExistingPointPolicy::KeepBoth,
        None,
    ));
    let Some(command) = command else {
        panic!("the point is drawn");
    };
    must(document, command);
}

fn warnings_of(outcome: &ReductionOutcome) -> Vec<String> {
    outcome
        .report
        .warnings
        .iter()
        .map(|warning| warning.text.clone())
        .collect()
}

fn drawing_of_points(points: Vec<SurveyPoint>) -> ReductionContext {
    ReductionContext {
        drawing_points: points,
        ..ReductionContext::default()
    }
}

mod survey_job_drawing_reduction {
    use super::*;

    // The reduction holds the first point of an id, and which is first is
    // only the order the points were drawn in: held from a drawing that has
    // CP1 at two places, a job would sit on either with nothing to say which
    // (100 km apart here). Refused instead, with both places, in the drawing's
    // order.
    #[test]
    fn an_id_the_drawing_has_at_two_places_is_not_held_and_both_places_are_named() {
        let context = drawing_of_points(vec![
            survey_point("CP1", 5_000_000.0, 500_000.0, Some(100.0)),
            survey_point("CP1", 5_000_000.0, 600_000.0, Some(100.0)),
        ]);
        let error = refused(reduce_for_drawing(
            &naming_cp1(),
            &holding_from_the_drawing("CP1"),
            &context,
        ));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(
            error.message,
            "Control point CP1 is on the drawing 2 times, at E 500000.0000 N 5000000.0000 \
             Z 100.0000 and E 600000.0000 N 5000000.0000 Z 100.0000, so which one to hold is \
             not known; rename or delete all but one."
        );

        // Three, one with no height - a mark of its own, since a height held
        // from one would not be held from it.
        let context = drawing_of_points(vec![
            survey_point("CP1", 5_000_000.0, 500_000.0, Some(100.0)),
            survey_point("CP1", 5_000_000.0, 500_000.0, None),
            survey_point("CP1", 5_000_000.0, 500_000.0, Some(100.0)),
        ]);
        let three = refused(reduce_for_drawing(
            &naming_cp1(),
            &holding_from_the_drawing("CP1"),
            &context,
        ));
        assert_eq!(
            three.message,
            "Control point CP1 is on the drawing 3 times, at E 500000.0000 N 5000000.0000 \
             Z 100.0000, E 500000.0000 N 5000000.0000 (no height) and E 500000.0000 \
             N 5000000.0000 Z 100.0000, so which one to hold is not known; rename or delete \
             all but one."
        );
    }

    // Points of one id that are one ground mark - closer than math's
    // tolerance::COORDINATE, 0.1 mm, across and in height - hold the same job
    // whichever is taken, so they are held. 0.05 mm apart is one mark; 0.15
    // mm apart, across or in height, is two.
    #[test]
    fn points_of_one_id_at_one_mark_are_held_and_two_marks_are_not() {
        let context = drawing_of_points(vec![
            survey_point("CP1", 5_000_000.0, 500_000.0, Some(100.0)),
            survey_point("CP1", 5_000_000.000_05, 500_000.0, Some(100.00005)),
        ]);
        let one = expect_ok(reduce_for_drawing(
            &naming_cp1(),
            &holding_from_the_drawing("CP1"),
            &context,
        ));
        for warning in warnings_of(&one) {
            assert!(!warning.contains("Control point CP1"), "{warning}");
        }

        let marks: [(f64, f64); 2] = [(5_000_000.000_15, 100.0), (5_000_000.0, 100.00015)];
        for (northing, height) in marks {
            let context = drawing_of_points(vec![
                survey_point("CP1", 5_000_000.0, 500_000.0, Some(100.0)),
                survey_point("CP1", northing, 500_000.0, Some(height)),
            ]);
            let two = refused(reduce_for_drawing(
                &naming_cp1(),
                &holding_from_the_drawing("CP1"),
                &context,
            ));
            assert_eq!(two.code, ErrorCode::InvalidArgument, "{height}");
        }
    }

    // Held from the drawing, a point the file never names - measured to or
    // from by nothing in it - changes nothing, which the report says before
    // anything else: an id typed wrong is the likely reason.
    #[test]
    fn a_held_id_the_file_never_names_is_reported_first_as_changing_nothing() {
        let context = drawing_of_points(vec![survey_point(
            "BM1",
            7_000_000.0,
            700_000.0,
            Some(10.0),
        )]);
        let outcome = expect_ok(reduce_for_drawing(
            &naming_cp1(),
            &holding_from_the_drawing("BM1"),
            &context,
        ));
        let warnings = warnings_of(&outcome);
        assert!(!warnings.is_empty());
        assert_eq!(
            warnings[0],
            "Control point BM1 is held from the drawing, but the file names no point BM1, so \
             holding it changes nothing."
        );

        // Named with no coordinates, as CP1 is: holding it is what places it.
        let context = drawing_of_points(vec![survey_point(
            "CP1",
            5_000_000.0,
            500_000.0,
            Some(100.0),
        )]);
        let named = expect_ok(reduce_for_drawing(
            &naming_cp1(),
            &holding_from_the_drawing("CP1"),
            &context,
        ));
        for warning in warnings_of(&named) {
            assert!(!warning.contains("Control point CP1"), "{warning}");
        }
    }

    // Held from the drawing, a point the file gives other coordinates is held
    // where the drawing has it; the report says so with both, first, since the
    // file's other coordinates stay in the file's own frame.
    #[test]
    fn a_held_id_the_file_gives_other_coordinates_is_reported_with_both() {
        let mut raw = naming_cp1();
        raw.unpositioned_points.clear();
        raw.points
            .push(survey_point("CP1", 5_000_000.0, 500_000.0, Some(100.0)));
        let context = drawing_of_points(vec![survey_point(
            "CP1",
            6_000_000.0,
            600_000.0,
            Some(200.0),
        )]);
        let moved = expect_ok(reduce_for_drawing(
            &raw,
            &holding_from_the_drawing("CP1"),
            &context,
        ));
        let warnings = warnings_of(&moved);
        assert!(!warnings.is_empty());
        assert_eq!(
            warnings[0],
            "Control point CP1 is held where the drawing has it, E 600000.0000 N 6000000.0000 \
             Z 200.0000, not where the file gives it, E 500000.0000 N 5000000.0000 Z 100.0000."
        );

        // The drawing's with no height is not the file's with one.
        let context = drawing_of_points(vec![survey_point("CP1", 5_000_000.0, 500_000.0, None)]);
        let mut plan = holding_from_the_drawing("CP1");
        plan.control[0].point.elevation.constraint = ControlConstraint::Free;
        let flat = expect_ok(reduce_for_drawing(&raw, &plan, &context));
        let warnings = warnings_of(&flat);
        assert!(!warnings.is_empty());
        assert_eq!(
            warnings[0],
            "Control point CP1 is held where the drawing has it, E 500000.0000 N 5000000.0000 \
             (no height), not where the file gives it, E 500000.0000 N 5000000.0000 Z 100.0000."
        );

        // One mark with the file's: nothing to say.
        let context = drawing_of_points(vec![survey_point(
            "CP1",
            5_000_000.000_05,
            500_000.0,
            Some(100.0),
        )]);
        let same = expect_ok(reduce_for_drawing(
            &raw,
            &holding_from_the_drawing("CP1"),
            &context,
        ));
        for warning in warnings_of(&same) {
            assert!(!warning.contains("Control point CP1"), "{warning}");
        }
    }
}
