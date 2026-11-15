// A survey job that is coded and strung as it is imported
// (tests/cad/test_survey_job_finish.cpp; SurveyJobImport::finish,
// survey_finish.rs): the job owns the lines as it owns the points, its stored
// options remember that it was finished, and a re-adjustment finishes it
// again.
//
// The reduction and the reader are INJECTED, as in survey_job.rs: the fake
// reduction returns points with known coordinates and codes and the features
// that string them, so every expected value is worked by hand from the shots
// a test lists. A survey point is (northing, easting) and a drawing point
// (x, y) = (easting, northing).

use std::sync::{Arc, Mutex};

use katana_cad::survey_finish::{describe, finish_warnings};
use katana_cad::survey_job::{read_survey_job_options, write_survey_job_options};
use katana_cad::{
    ColourOf, Document, HandEditPolicy, ImportSurveyJobCommand, ImportSurveyJobResults,
    ReadjustSurveyJobCommand, ReadjustSurveyJobResults, ReductionFunction, RemoveSurveyJobCommand,
    SurveyFinishOptions, SurveyFinishReport, SurveyFinishSkip, SurveyImportOptions, SurveyJob,
    SurveyJobContext, SurveyJobImport, SurveyJobOptions, SurveyJobReader, SurveyJobReadjustment,
    UnplacedFeatureReason,
};
use katana_commands::{create_entities, create_label_style, delete_entities, move_entities};
use katana_core::{ErrorCode, Result};
use katana_entity::{
    Color, Entity, EntityId, Geometry, LabelGeometry, LabelKind, LabelStyle, SurveyBreakline,
    SurveyMap, SurveyRule,
};
use katana_geometry::Vec2;
use katana_survey::data_model::{CoordinateSource, SurveyFeature, SurveyPoint, SurveyProject};
use katana_survey::reduction::{ComputedPoint, ReductionContext, ReductionOutcome};
use katana_survey::reduction_report::ReductionReport;
use katana_survey::reduction_settings::ReductionSettings;

use crate::{drawing_of, expect_ok, must, numbered, p, point_numbered, text_property, vertices_of};

#[derive(Clone, Debug)]
struct Shot {
    id: &'static str,
    northing: f64,
    easting: f64,
    code: &'static str,
}

fn shot(id: &'static str, northing: f64, easting: f64, code: &'static str) -> Shot {
    Shot {
        id,
        northing,
        easting,
        code,
    }
}

// Stands in for survey's reduce_and_adjust on a field file that keeps code and
// string number apart: whatever `shots` and `features` hold when it is called
// is what the "reduction" returns, and every call is counted.
#[derive(Default)]
struct FakeField {
    shots: Vec<Shot>,
    features: Vec<SurveyFeature>,
    calls: usize,
}

type Fake = Arc<Mutex<FakeField>>;

fn reduction_of(fake: &Fake) -> ReductionFunction {
    let fake = Arc::clone(fake);
    Box::new(
        move |raw: &SurveyProject,
              settings: &ReductionSettings,
              context: &ReductionContext|
              -> Result<ReductionOutcome> {
            let mut fake = fake.lock().unwrap();
            fake.calls += 1;
            let mut points = Vec::new();
            let mut computed = Vec::new();
            for shot in &fake.shots {
                points.push(SurveyPoint {
                    id: String::from(shot.id),
                    northing: shot.northing,
                    easting: shot.easting,
                    code: String::from(shot.code),
                    coordinate_source: CoordinateSource::Calculated,
                    ..SurveyPoint::default()
                });
                computed.push(ComputedPoint {
                    id: String::from(shot.id),
                    northing: shot.northing,
                    easting: shot.easting,
                    ..ComputedPoint::default()
                });
            }
            Ok(ReductionOutcome {
                reduced: SurveyProject {
                    name: raw.name.clone(),
                    points,
                    features: fake.features.clone(),
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

fn feature(code: &str, number: &str, ids: &[&str]) -> SurveyFeature {
    SurveyFeature {
        code: String::from(code),
        // The string number, as the field-file readers leave it.
        name: String::from(number),
        point_ids: ids.iter().map(|id| String::from(*id)).collect(),
        ..SurveyFeature::default()
    }
}

// Two strings of the joint code KJ, two shots each:
//   KJ 01: point 1 (N 0, E 0)  and point 2 (N 0, E 10)
//   KJ 02: point 3 (N 5, E 0)  and point 4 (N 5, E 10)
fn two_joints() -> Fake {
    Arc::new(Mutex::new(FakeField {
        shots: vec![
            shot("1", 0.0, 0.0, "KJ"),
            shot("2", 0.0, 10.0, "KJ"),
            shot("3", 5.0, 0.0, "KJ"),
            shot("4", 5.0, 10.0, "KJ"),
        ],
        features: vec![
            feature("KJ", "01", &["1", "2"]),
            feature("KJ", "02", &["3", "4"]),
        ],
        calls: 0,
    }))
}

fn joint_rule(colour: &str) -> SurveyRule {
    SurveyRule {
        key: String::from("KJ*"),
        model: String::from("SURVEY JOINT"),
        linestyle: String::from("Joint Line"),
        colour: String::from(colour),
        breakline: Some(SurveyBreakline::Line),
        ..SurveyRule::default()
    }
}

// "KJ*": a joint is a LINE on SURVEY JOINT, drawn with "Joint Line".
fn joint_map() -> SurveyMap {
    let mut map = SurveyMap::default();
    assert!(map.add(joint_rule("")).is_ok());
    map
}

fn import_request(codes: bool, linework: bool) -> SurveyJobImport {
    SurveyJobImport {
        job: SurveyJob {
            name: String::from("DAY1.FLD"),
            format_id: String::from("opcode-field-file"),
            parser_version: String::from("1.0"),
            source_file_name: String::from("DAY1.FLD"),
            source_bytes: b"the field file's bytes".to_vec(),
            // A top-level layer on purpose. Creating a layer under "survey"
            // also creates that parent, which the undo of ANY import leaves
            // behind - the layer command's undo removes only the layer it was
            // given - and these tests compare whole drawings to see what the
            // finish leaves, not that.
            layer: String::from("day1"),
            ..SurveyJob::default()
        },
        raw: SurveyProject {
            name: String::from("day one"),
            ..SurveyProject::default()
        },
        context: SurveyJobContext {
            created_utc: String::from("2026-10-06T10:00:00Z"),
            ..SurveyJobContext::default()
        },
        import_options: SurveyImportOptions {
            layer: String::from("day1"),
            ..SurveyImportOptions::default()
        },
        finish: SurveyFinishOptions {
            codes,
            linework,
            ..SurveyFinishOptions::default()
        },
        ..SurveyJobImport::default()
    }
}

// Imports the job and returns what its command filled, the command being the
// document's now.
#[track_caller]
fn import_job_with(
    document: &mut Document,
    fake: &Fake,
    request: SurveyJobImport,
) -> ImportSurveyJobResults {
    let command = ImportSurveyJobCommand::new(document, request, Some(reduction_of(fake)));
    let results = command.results();
    must(document, Box::new(command));
    results
}

// ... coded and strung, or neither.
#[track_caller]
fn import_job(document: &mut Document, fake: &Fake, finish: bool) -> ImportSurveyJobResults {
    import_job_with(document, fake, import_request(finish, finish))
}

// Re-adjusts the document's one job with the settings it has, and returns
// what the command filled, the command being the document's now.
#[track_caller]
fn readjust_how(
    document: &mut Document,
    fake: &Fake,
    finish: SurveyFinishOptions,
    hand_edits: HandEditPolicy,
) -> ReadjustSurveyJobResults {
    let job = &document.survey_jobs()[0];
    let request = SurveyJobReadjustment {
        job_id: job.id.clone(),
        settings: job.settings.clone(),
        context: SurveyJobContext {
            created_utc: String::from("2026-10-06T11:00:00Z"),
            ..SurveyJobContext::default()
        },
        hand_edits,
        finish,
        ..SurveyJobReadjustment::default()
    };
    let reader: SurveyJobReader = Box::new(|job: &SurveyJob| {
        Ok(SurveyProject {
            name: format!("re-read {}", job.source_file_name),
            ..SurveyProject::default()
        })
    });
    let command =
        ReadjustSurveyJobCommand::new(document, request, Some(reader), Some(reduction_of(fake)));
    let results = command.results();
    must(document, Box::new(command));
    results
}

#[track_caller]
fn readjust(document: &mut Document, fake: &Fake) -> ReadjustSurveyJobResults {
    readjust_how(
        document,
        fake,
        SurveyFinishOptions::default(),
        HandEditPolicy::Keep,
    )
}

fn is_line(document: &Document, id: EntityId) -> bool {
    document
        .model()
        .entities
        .find(id)
        .is_some_and(|entity| matches!(entity.geometry, Geometry::Polyline(_)))
}

// The job's lines - its created entities that are polylines - in the order
// the job lists them.
fn lines_of<'a>(document: &'a Document, job: &SurveyJob) -> Vec<&'a Entity> {
    job.created_entities
        .iter()
        .filter_map(|&id| document.model().entities.find(id))
        .filter(|entity| matches!(entity.geometry, Geometry::Polyline(_)))
        .collect()
}

fn line_count(document: &Document) -> usize {
    let mut count = 0;
    document.model().entities.for_each(|entity| {
        count += usize::from(matches!(entity.geometry, Geometry::Polyline(_)));
    });
    count
}

#[track_caller]
fn entity(document: &Document, id: EntityId) -> &Entity {
    match document.model().entities.find(id) {
        Some(entity) => entity,
        None => panic!("entity {id} is gone"),
    }
}

#[track_caller]
fn finished(report: Option<SurveyFinishReport>) -> SurveyFinishReport {
    match report {
        Some(report) => report,
        None => panic!("the job was finished"),
    }
}

// The text an import with every drawing option at its default has always
// written for SurveyJob::import_options: the versioned first line and the six
// keys, in the order the reader documents them.
const DEFAULT_OPTIONS_TEXT: &str = "katana-survey-import-options=1\n\
                                    layer-per-code=false\n\
                                    create-layers=true\n\
                                    code-property=code\n\
                                    point-number-property=point\n\
                                    description-property=description\n\
                                    record-source=true\n";

// ---- Import --------------------------------------------------------------------------

mod survey_job_finish {
    use super::*;

    #[test]
    fn the_job_owns_its_points_and_its_lines_and_the_reduction_runs_once() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();

        let command = import_job(&mut document, &fake, true);

        assert_eq!(
            fake.lock().unwrap().calls,
            1,
            "composed inside the job: validate's reduction is execute's"
        );
        assert_eq!(document.survey_jobs().len(), 1);
        let job = &document.survey_jobs()[0];
        // Four points, then the two lines KJ 01 and KJ 02.
        assert_eq!(job.created_entities.len(), 6);
        assert_eq!(
            job.placed_points.len(),
            4,
            "the points alone: the count of points"
        );
        for (i, &id) in job.created_entities.iter().enumerate() {
            assert_eq!(is_line(&document, id), i >= 4, "{i}");
        }
        assert_eq!(command.created_entities(), job.created_entities);
        assert_eq!(document.model().entities.size(), 6);

        let lines = lines_of(&document, job);
        assert_eq!(lines.len(), 2);
        assert_eq!(vertices_of(lines[0]), [p(0.0, 0.0), p(10.0, 0.0)]);
        assert_eq!(vertices_of(lines[1]), [p(0.0, 5.0), p(10.0, 5.0)]);
        assert_eq!(lines[0].layer, "SURVEY JOINT");
        assert_eq!(lines[0].style, "Joint Line");
        // The points were coded by their string names, KJ01 and KJ02 -> "KJ*".
        assert_eq!(numbered(&document, "1").layer, "SURVEY JOINT");
        assert_eq!(numbered(&document, "1").style, "Joint Line");
        assert_eq!(text_property(numbered(&document, "1"), "string"), "01");
        assert_eq!(text_property(numbered(&document, "4"), "string"), "02");

        let finish = finished(command.finish_report());
        assert_eq!(finish.points, 4);
        assert_eq!(finish.why_not_coded, SurveyFinishSkip::None);
        assert_eq!(finish.coding.coded, 4);
        assert_eq!(finish.coding.matched, 4);
        assert_eq!(finish.why_not_strung, SurveyFinishSkip::None);
        assert_eq!(finish.lines.len(), 2);
        assert_eq!(finish.unplaced(), 0);
        // In words: four points, all coded KJ and all ruled by "KJ*", which
        // made the one layer SURVEY JOINT and the one style Joint Line; two
        // lines.
        assert_eq!(
            describe(&finish),
            [
                "Survey codes were applied to the 4 point(s) drawn: 4 carry a code and 4 of \
                 those have a rule; 1 layer(s) and 1 style(s) were created.",
                "Linework: 2 line(s) drawn.",
            ]
        );
        assert!(finish_warnings(&finish).is_empty(), "nothing was left over");
    }

    #[test]
    fn one_undo_removes_points_lines_and_job_and_redo_brings_them_back_unchanged() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        let before = drawing_of(&document);

        import_job(&mut document, &fake, true);
        let after = drawing_of(&document);
        let imported = document.survey_jobs()[0].clone();
        assert_eq!(after.entities.len(), 6);

        assert!(document.undo().is_ok());
        assert!(document.survey_jobs().is_empty());
        assert_eq!(drawing_of(&document), before);
        assert!(document.undo().is_err(), "it was one step");

        assert!(document.redo().is_ok());
        assert_eq!(document.survey_jobs().len(), 1);
        assert_eq!(document.survey_jobs()[0], imported);
        assert_eq!(
            drawing_of(&document),
            after,
            "the same ids, layers and styles"
        );
        assert_eq!(
            fake.lock().unwrap().calls,
            1,
            "redo replays, it does not reduce or plan again"
        );
    }

    #[test]
    fn removing_the_job_with_its_points_leaves_nothing_of_it() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        let command = import_job(&mut document, &fake, true);
        assert_eq!(document.model().entities.size(), 6);
        let with_job = drawing_of(&document);
        let job = document.survey_jobs()[0].clone();

        let remove = RemoveSurveyJobCommand::new(&document, command.job_id(), true);
        must(&mut document, Box::new(remove));

        assert!(document.survey_jobs().is_empty());
        assert_eq!(
            document.model().entities.size(),
            0,
            "the lines went with the points"
        );
        // And one undo puts the lines back with them: the same entities, as
        // they were, and the job that owns them.
        assert!(document.undo().is_ok());
        assert_eq!(drawing_of(&document), with_job);
        assert_eq!(line_count(&document), 2);
        assert_eq!(document.survey_jobs().len(), 1);
        assert_eq!(document.survey_jobs()[0], job);
    }

    #[test]
    fn with_the_options_off_the_job_is_imported_exactly_as_it_always_was() {
        let mut document = Document::default();
        document.set_survey_map(joint_map()); // loaded, and not asked for
        let fake = two_joints();

        let command = import_job(&mut document, &fake, false);

        let job = &document.survey_jobs()[0];
        assert!(command.finish_report().is_none());
        assert_eq!(job.import_options, DEFAULT_OPTIONS_TEXT);
        assert_eq!(job.created_entities.len(), 4);
        assert_eq!(line_count(&document), 0);
        for number in ["1", "2", "3", "4"] {
            let Some(point) = point_numbered(&document, number) else {
                panic!("point {number} is drawn");
            };
            assert_eq!(point.layer, "day1", "{number}");
            assert_eq!(point.style, "", "{number}");
            assert_eq!(text_property(point, "code"), "KJ", "{number}");
            assert_eq!(text_property(point, "string"), "<absent>", "{number}");
        }
        assert!(!document.model().layers.contains("SURVEY JOINT"));
        assert!(!job.report_text.contains("Survey codes"));
        assert!(!job.report_text.contains("inework"));
    }

    #[test]
    fn a_finish_with_nothing_to_do_still_imports_the_job_and_its_report_says_why() {
        let mut document = Document::default(); // no survey codes loaded
        let fake = two_joints();

        let command = import_job(&mut document, &fake, true);

        let job = &document.survey_jobs()[0];
        assert_eq!(job.created_entities.len(), 4);
        assert_eq!(line_count(&document), 0);
        assert_eq!(numbered(&document, "1").layer, "day1");
        let finish = finished(command.finish_report());
        assert_eq!(finish.why_not_coded, SurveyFinishSkip::NoSurveyCodes);
        assert_eq!(finish.why_not_strung, SurveyFinishSkip::NoSurveyCodes);
        // Both steps were asked for and neither ran: each says why, and both
        // sentences are warnings of the job's report.
        let sentences = [
            "Survey codes were not applied: no survey codes are loaded.",
            "No linework was drawn: no survey codes are loaded.",
        ];
        assert_eq!(describe(&finish), sentences);
        assert_eq!(finish_warnings(&finish), sentences);
        for sentence in sentences {
            assert!(job.report_text.contains(sentence), "{sentence}");
        }
        // The choice is the job's all the same: it was imported to be
        // finished.
        let stored = expect_ok(read_survey_job_options(&job.import_options, &job.id));
        assert!(stored.apply_codes);
        assert!(stored.draw_linework);
    }

    // ---- Re-adjust ----

    #[test]
    fn a_readjustment_codes_its_new_points_and_redraws_the_jobs_lines_in_one_undo_step() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        import_job(&mut document, &fake, true);
        let imported = document.survey_jobs()[0].clone();
        let old_lines = [imported.created_entities[4], imported.created_entities[5]];
        let before = drawing_of(&document);

        // The new run moves point 2 two metres east and shoots a third point
        // of string 02:  KJ 01: 1 (N 0, E 0), 2 (N 0, E 12)
        //                KJ 02: 3 (N 5, E 0), 4 (N 5, E 10), 5 (N 5, E 20)
        {
            let mut fake = fake.lock().unwrap();
            fake.shots[1].easting = 12.0;
            fake.shots.push(shot("5", 5.0, 20.0, "KJ"));
            fake.features[1].point_ids.push(String::from("5"));
        }

        let command = readjust(&mut document, &fake);

        assert_eq!(command.changes().moved, ["2"]);
        assert_eq!(command.changes().created, ["5"]);
        let job = &document.survey_jobs()[0];

        // The new point is coded as its neighbours were: KJ02 -> "KJ*".
        let fifth = numbered(&document, "5");
        assert_eq!(fifth.layer, "SURVEY JOINT");
        assert_eq!(fifth.style, "Joint Line");
        assert_eq!(text_property(fifth, "string"), "02");

        // The job's two lines are the SAME two entities, at the new
        // coordinates: KJ 01 through the moved point 2, KJ 02 on through the
        // new point 5.
        assert_eq!(line_count(&document), 2);
        assert!(is_line(&document, old_lines[0]));
        assert!(is_line(&document, old_lines[1]));
        assert_eq!(
            vertices_of(entity(&document, old_lines[0])),
            [p(0.0, 0.0), p(12.0, 0.0)]
        );
        assert_eq!(
            vertices_of(entity(&document, old_lines[1])),
            [p(0.0, 5.0), p(10.0, 5.0), p(20.0, 5.0)]
        );
        assert_eq!(entity(&document, old_lines[0]).style, "Joint Line");
        // The job owns what it owned, in the order it listed it, and then the
        // point drawn now: four points, two lines, point 5.
        let mut owned = imported.created_entities.clone();
        owned.push(fifth.id);
        assert_eq!(job.created_entities, owned);
        assert_eq!(job.placed_points.len(), 5);

        let finish = finished(command.finish_report());
        assert_eq!(finish.points, 1, "the one point drawn now");
        assert_eq!(finish.coding.coded, 1);
        assert_eq!(finish.earlier_lines_redrawn, old_lines);
        assert!(finish.earlier_lines_removed.is_empty());
        assert!(finish.lines.is_empty(), "no line is a new entity");
        // In words: the layer and the style were there already.
        assert_eq!(
            describe(&finish),
            [
                "Survey codes were applied to the 1 point(s) drawn: 1 carry a code and 1 of \
                 those have a rule; 0 layer(s) and 0 style(s) were created.",
                "Linework: 0 line(s) drawn and 2 of the job's line(s) redrawn where this \
                 adjustment puts them.",
            ]
        );
        // Nothing of that is a warning of the job's report.
        assert!(finish_warnings(&finish).is_empty());
        assert!(!job.report_text.contains("Survey codes"));
        assert!(!job.report_text.contains("inework"));

        // One undo: the lines where they ran, the new point gone, the job as
        // it was; one redo: all of it again, the job included.
        let after = drawing_of(&document);
        let readjusted = document.survey_jobs()[0].clone();
        assert!(document.undo().is_ok());
        assert_eq!(drawing_of(&document), before);
        assert_eq!(document.survey_jobs()[0], imported);
        assert!(document.redo().is_ok());
        assert_eq!(drawing_of(&document), after);
        assert_eq!(document.survey_jobs()[0], readjusted);
        assert_eq!(
            fake.lock().unwrap().calls,
            2,
            "the import's reduction and the re-adjustment's, no more"
        );
    }

    #[test]
    fn a_job_imported_without_a_finish_is_never_coded_by_a_readjustment() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        import_job(&mut document, &fake, false);
        assert_eq!(
            document.survey_jobs()[0].import_options,
            DEFAULT_OPTIONS_TEXT
        );

        {
            let mut fake = fake.lock().unwrap();
            fake.shots.push(shot("5", 5.0, 20.0, "KJ"));
            fake.features[1].point_ids.push(String::from("5"));
        }
        // The caller's two flags are not what decides: the job's stored text
        // is.
        let asked = SurveyFinishOptions {
            codes: true,
            linework: true,
            ..SurveyFinishOptions::default()
        };
        let command = readjust_how(&mut document, &fake, asked, HandEditPolicy::Keep);

        assert_eq!(command.changes().created, ["5"]);
        assert!(command.finish_report().is_none());
        let fifth = numbered(&document, "5");
        assert_eq!(
            fifth.layer, "day1",
            "drawn as the job's others, and not coded"
        );
        assert_eq!(fifth.style, "");
        assert_eq!(text_property(fifth, "string"), "<absent>");
        assert_eq!(line_count(&document), 0);
        assert!(!document.model().layers.contains("SURVEY JOINT"));
    }

    #[test]
    fn when_the_linework_step_has_nothing_to_go_on_the_jobs_lines_stay_and_the_report_says_so() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        import_job(&mut document, &fake, true);
        let imported = document.survey_jobs()[0].clone();
        assert_eq!(line_count(&document), 2);

        // The survey codes are unloaded, then the job is re-adjusted: point 2
        // moves, and nothing says any more which codes are lines.
        document.set_survey_map(SurveyMap::default());
        fake.lock().unwrap().shots[1].easting = 12.0;
        let command = readjust(&mut document, &fake);

        assert_eq!(command.changes().moved, ["2"]);
        let job = &document.survey_jobs()[0];
        // Both lines stand, where the earlier run drew them, and are still the
        // job's.
        assert_eq!(job.created_entities, imported.created_entities);
        let lines = lines_of(&document, job);
        assert_eq!(lines.len(), 2);
        assert_eq!(vertices_of(lines[0]), [p(0.0, 0.0), p(10.0, 0.0)]);

        let finish = finished(command.finish_report());
        assert_eq!(finish.why_not_strung, SurveyFinishSkip::NoSurveyCodes);
        assert_eq!(finish.earlier_lines_kept, 2);
        assert!(finish.earlier_lines_removed.is_empty());
        assert!(finish.earlier_lines_redrawn.is_empty());
        // No point was new, so no coding was due; the lines are what to act
        // on.
        let stale = "No linework was drawn: no survey codes are loaded. The job's 2 line(s) are \
                     as the earlier adjustment drew them.";
        assert_eq!(
            describe(&finish),
            [
                "No new points were drawn, so no survey codes were applied.",
                stale,
            ]
        );
        assert_eq!(finish_warnings(&finish), [stale]);
        assert!(job.report_text.contains(stale));
        assert!(!job.report_text.contains("No new points were drawn"));
    }

    #[test]
    fn when_no_rule_knows_the_jobs_codes_any_more_its_lines_stay_as_they_were() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        import_job(&mut document, &fake, true);
        let imported = document.survey_jobs()[0].clone();

        // Other survey codes are loaded - there are rules, and none for KJ -
        // and then point 2 moves.
        let mut other = SurveyMap::default();
        let kerb = SurveyRule {
            key: String::from("KB*"),
            model: String::from("SURVEY KERB"),
            breakline: Some(SurveyBreakline::Line),
            ..SurveyRule::default()
        };
        assert!(other.add(kerb).is_ok());
        document.set_survey_map(other);
        fake.lock().unwrap().shots[1].easting = 12.0;
        let command = readjust(&mut document, &fake);

        assert_eq!(command.changes().moved, ["2"]);
        let job = &document.survey_jobs()[0];
        assert_eq!(job.created_entities, imported.created_entities);
        let lines = lines_of(&document, job);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            vertices_of(lines[0]),
            [p(0.0, 0.0), p(10.0, 0.0)],
            "where the earlier run drew it, though point 2 is now at E 12"
        );

        let finish = finished(command.finish_report());
        assert_eq!(finish.why_not_strung, SurveyFinishSkip::NoRuleMatches);
        assert_eq!(finish.earlier_lines_kept, 2);
        assert!(finish.earlier_lines_redrawn.is_empty());
        assert!(finish.earlier_lines_removed.is_empty());
        assert_eq!(finish.unplaced_features.len(), 2);
        assert_eq!(
            finish.unplaced_features[0].reason,
            UnplacedFeatureReason::NoRule
        );
        let stale = "No linework was drawn: no rule matches any of their codes. The job's 2 \
                     line(s) are as the earlier adjustment drew them.";
        assert_eq!(finish_warnings(&finish), [stale]);
        assert!(job.report_text.contains(stale));
    }

    #[test]
    fn a_readjustment_that_changes_nothing_leaves_the_jobs_lines_as_they_are() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        import_job(&mut document, &fake, true);
        let imported = document.survey_jobs()[0].clone();
        let before = drawing_of(&document);

        let command = readjust(&mut document, &fake); // the same run again

        assert!(command.changes().moved.is_empty());
        assert!(command.finish_report().is_none());
        assert_eq!(drawing_of(&document), before);
        assert_eq!(
            document.survey_jobs()[0].created_entities,
            imported.created_entities
        );
    }

    // ---- A finish with nothing to do, and what a job's report holds ----

    // The promise the front ends' defaults rest on: with no survey codes
    // loaded, asking for the finish changes NOTHING in the drawing - not a
    // layer, not a property. (The job's stored options do say it was asked
    // for.)
    #[test]
    fn with_no_survey_codes_loaded_a_finished_import_draws_exactly_what_a_plain_one_draws() {
        let mut plain = Document::default();
        let mut finished = Document::default(); // neither has survey codes
        import_job(&mut plain, &two_joints(), false);
        import_job(&mut finished, &two_joints(), true);

        assert_eq!(drawing_of(&finished), drawing_of(&plain));
        assert_eq!(
            text_property(numbered(&finished, "1"), "string"),
            "<absent>"
        );
        assert_eq!(
            finished.survey_jobs()[0].created_entities,
            plain.survey_jobs()[0].created_entities
        );
    }

    #[test]
    fn a_finish_that_did_all_it_was_asked_adds_no_warning_to_the_jobs_report() {
        let mut plain = Document::default();
        let mut finished_document = Document::default();
        plain.set_survey_map(joint_map());
        finished_document.set_survey_map(joint_map());

        let without = import_job(&mut plain, &two_joints(), false);
        let with = import_job(&mut finished_document, &two_joints(), true);

        // Four points coded, two lines drawn, nothing left over: what ran is
        // in finish_report(), and the report's warnings are those of the same
        // import with no finish at all - the reduction's and the drawing's
        // own.
        let finish = finished(with.finish_report());
        assert!(finish_warnings(&finish).is_empty());
        let Some(with_report) = with.report() else {
            panic!("the finished import has its report");
        };
        let Some(without_report) = without.report() else {
            panic!("the plain import has its report");
        };
        assert_eq!(with_report.warnings, without_report.warnings);
        let job = &finished_document.survey_jobs()[0];
        assert!(!job.report_text.contains("Survey codes"));
        assert!(!job.report_text.contains("inework"));
        assert_eq!(job.report_text, plain.survey_jobs()[0].report_text);
    }

    // ... and one that left something over adds exactly that: here two shots
    // of a code no rule knows, which are neither coded nor strung.
    #[test]
    fn what_a_finish_leaves_over_is_a_warning_of_the_jobs_report() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        {
            let mut fake = fake.lock().unwrap();
            fake.shots.push(shot("5", 9.0, 0.0, "ZZ"));
            fake.shots.push(shot("6", 9.0, 10.0, "ZZ"));
            fake.features.push(feature("ZZ", "1", &["5", "6"]));
        }

        let command = import_job(&mut document, &fake, true);

        let finish = finished(command.finish_report());
        // Six points, all coded; the four KJ are ruled. ZZ 1 is a string of the
        // file that no rule makes anything of.
        let left = [
            "No rule for the code(s): ZZ.",
            "1 string(s) of the file are in no line (no rule for its code: 1).",
        ];
        assert_eq!(
            describe(&finish),
            [
                "Survey codes were applied to the 6 point(s) drawn: 6 carry a code and 4 of \
                 those have a rule; 1 layer(s) and 1 style(s) were created.",
                left[0],
                "Linework: 2 line(s) drawn.",
                left[1],
            ]
        );
        assert_eq!(finish_warnings(&finish), left);
        let job = &document.survey_jobs()[0];
        let Some(report) = command.report() else {
            panic!("the import has its report");
        };
        for sentence in left {
            assert!(job.report_text.contains(sentence), "{sentence}");
            let times = report
                .warnings
                .iter()
                .filter(|warning| warning.text == sentence)
                .count();
            assert_eq!(times, 1, "{sentence}");
        }
        assert!(
            !job.report_text.contains("Linework: 2 line(s) drawn."),
            "what was done is not a warning"
        );
    }

    // ---- Re-adjust: the job's lines are redrawn where they stand ----

    #[test]
    fn a_readjustment_that_only_moves_points_redraws_the_jobs_lines_as_the_same_entities() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        import_job(&mut document, &fake, true);
        let imported = document.survey_jobs()[0].clone();
        let first = imported.created_entities[4]; // KJ 01
        let second = imported.created_entities[5]; // KJ 02
        let second_before = entity(&document, second).clone();

        // Point 2 moves two metres east: KJ 01 is then (N 0, E 0) to (N 0, E 12).
        fake.lock().unwrap().shots[1].easting = 12.0;
        let command = readjust(&mut document, &fake);

        assert_eq!(command.changes().moved, ["2"]);
        assert!(command.changes().created.is_empty());
        let job = &document.survey_jobs()[0];
        // The same six entities, in the same order: nothing deleted, nothing
        // new.
        assert_eq!(job.created_entities, imported.created_entities);
        assert_eq!(document.model().entities.size(), 6);
        assert!(is_line(&document, first));
        assert_eq!(
            vertices_of(entity(&document, first)),
            [p(0.0, 0.0), p(12.0, 0.0)]
        );
        // KJ 02, none of whose points moved, is not touched at all.
        assert!(is_line(&document, second));
        assert_eq!(*entity(&document, second), second_before);

        let finish = finished(command.finish_report());
        assert_eq!(finish.points, 0, "no point was drawn for the first time");
        assert_eq!(finish.why_not_coded, SurveyFinishSkip::NoPoints);
        assert_eq!(finish.why_not_strung, SurveyFinishSkip::None);
        // Both lines are strung again by this run - the second where it
        // already ran - and none is created or deleted.
        assert_eq!(finish.earlier_lines_redrawn, [first, second]);
        assert!(finish.lines.is_empty());
        assert!(finish.earlier_lines_removed.is_empty());
        assert_eq!(finish.earlier_lines_kept, 0);
        assert!(command.created_entities().is_empty());
        assert_eq!(
            describe(&finish),
            [
                "No new points were drawn, so no survey codes were applied.",
                "Linework: 0 line(s) drawn and 2 of the job's line(s) redrawn where this \
                 adjustment puts them.",
            ]
        );
        assert!(finish_warnings(&finish).is_empty());
    }

    // What deleting and redrawing the lines cost: everything that follows a
    // line - its label with the place it was dragged to and the text typed
    // over it - goes when the line does. The line is the same entity
    // afterwards, so the label is still on it, and follows it.
    #[test]
    fn a_label_on_a_jobs_line_stays_on_it_through_a_readjustment() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        import_job(&mut document, &fake, true);
        let line = document.survey_jobs()[0].created_entities[4]; // KJ 01
        let style = LabelStyle {
            name: String::from("Side"),
            kind: LabelKind::Segment,
            text: String::from("{length}"),
            ..LabelStyle::default()
        };
        must(&mut document, create_label_style(style));
        // On the line's one segment, whose midpoint is (5, 0); dragged two
        // metres off it, with its text typed over.
        let label_entity = Entity {
            geometry: Geometry::Label(LabelGeometry {
                target: line,
                part: 0,
                style: String::from("Side"),
                anchor: p(5.0, 0.0),
                position: Some(p(5.0, 2.0)),
                text_override: String::from("joint 01"),
                ..LabelGeometry::default()
            }),
            ..Entity::default()
        };
        must(&mut document, create_entities(vec![label_entity]));
        let label = document.last_created_entities()[0];

        fake.lock().unwrap().shots[1].easting = 12.0; // the line becomes (0, 0) to (12, 0)
        readjust(&mut document, &fake);

        let Some(kept) = document.model().entities.find(label) else {
            panic!("the label went with the line it was on");
        };
        let Geometry::Label(geometry) = &kept.geometry else {
            panic!("the label is a label");
        };
        assert_eq!(geometry.target, line);
        assert_eq!(geometry.text_override, "joint 01");
        // It follows the segment's new midpoint, (6, 0), keeping its place
        // beside it: one metre east of where it was dragged to.
        assert_eq!(geometry.anchor, p(6.0, 0.0));
        assert_eq!(geometry.position, Some(p(6.0, 2.0)));

        // And one undo puts the line and the label back where they were.
        assert!(document.undo().is_ok());
        let Geometry::Label(before) = &entity(&document, label).geometry else {
            panic!("the label is a label");
        };
        assert_eq!(before.anchor, p(5.0, 0.0));
        assert_eq!(before.position, Some(p(5.0, 2.0)));
    }

    // Under Keep a point the person moved stays where they put it, and one
    // they deleted stays deleted. A line of the job runs through each of its
    // points where the point STANDS in the drawing - so through the moved
    // point where it was moved to - and, for a point that is not in the
    // drawing, where the run puts it: deleting a mark does not take a vertex
    // out of the kerb.
    #[test]
    fn under_keep_a_line_runs_through_a_hand_moved_point_where_it_stands() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        import_job(&mut document, &fake, true);
        let second = numbered(&document, "2").id;
        let fourth = numbered(&document, "4").id;
        // By hand: point 2 one metre north, to (10, 1); point 4 deleted.
        must(
            &mut document,
            move_entities(vec![second], Vec2::new(0.0, 1.0)),
        );
        must(&mut document, delete_entities(vec![fourth]));

        // The new run moves points 1 and 3 two metres west and nothing else:
        //   KJ 01: 1 (N 0, E -2), 2 (N 0, E 10)     KJ 02: 3 (N 5, E -2), 4 (N 5, E 10)
        {
            let mut fake = fake.lock().unwrap();
            fake.shots[0].easting = -2.0;
            fake.shots[2].easting = -2.0;
        }
        let command = readjust(&mut document, &fake);

        let changes = command.changes();
        assert_eq!(changes.moved, ["1", "3"]);
        assert_eq!(changes.edited_by_hand, ["2"]);
        assert_eq!(changes.deleted_by_hand, ["4"]);
        let Geometry::Point(moved) = &entity(&document, second).geometry else {
            panic!("point 2 is a point");
        };
        assert_eq!(moved.position, p(10.0, 1.0), "the hand edit is kept");
        assert!(!document.model().entities.contains(fourth));

        let job = &document.survey_jobs()[0];
        let lines = lines_of(&document, job);
        assert_eq!(lines.len(), 2);
        // KJ 01: from point 1 where the run puts it to point 2 where it
        // stands.
        assert_eq!(vertices_of(lines[0]), [p(-2.0, 0.0), p(10.0, 1.0)]);
        // KJ 02: from point 3 to where the run puts the deleted point 4.
        assert_eq!(vertices_of(lines[1]), [p(-2.0, 5.0), p(10.0, 5.0)]);
        // ... which the report says, in the job's own report too: one vertex.
        let finish = finished(command.finish_report());
        assert_eq!(finish.vertices_at_deleted_points, 1);
        let at_deleted = "1 vertex(es) of the job's lines are at a point deleted from the \
                          drawing by hand: a string the file numbered still runs through where \
                          this adjustment puts it.";
        assert_eq!(finish_warnings(&finish), [at_deleted]);
        assert!(job.report_text.contains(at_deleted));
    }

    // Under Overwrite the deleted point is drawn again, so there is no vertex
    // without a point, and nothing to say.
    #[test]
    fn under_overwrite_a_deleted_point_is_drawn_again_and_its_line_runs_through_it() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        import_job(&mut document, &fake, true);
        let second = document.survey_jobs()[0].created_entities[5]; // KJ 02
        let fourth = numbered(&document, "4").id;
        must(&mut document, delete_entities(vec![fourth]));

        let command = readjust_how(
            &mut document,
            &fake,
            SurveyFinishOptions::default(),
            HandEditPolicy::Overwrite,
        );

        let changes = command.changes();
        assert_eq!(changes.deleted_by_hand, ["4"]);
        assert_eq!(changes.created, ["4"]);
        let again = numbered(&document, "4");
        // Drawn now, so coded now, as a new point of the job is.
        assert_eq!(again.layer, "SURVEY JOINT");
        assert_eq!(text_property(again, "string"), "02");
        let finish = finished(command.finish_report());
        assert_eq!(finish.points, 1);
        assert_eq!(finish.vertices_at_deleted_points, 0);
        assert!(finish_warnings(&finish).is_empty());
        // KJ 02 is the line it was, where it ran.
        assert!(is_line(&document, second));
        assert_eq!(
            vertices_of(entity(&document, second)),
            [p(0.0, 5.0), p(10.0, 5.0)]
        );
        assert_eq!(finish.earlier_lines_redrawn.len(), 2);
    }

    #[test]
    fn a_line_the_new_run_no_longer_strings_is_deleted_and_the_report_says_so() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        import_job(&mut document, &fake, true);
        let imported = document.survey_jobs()[0].clone();
        let first = imported.created_entities[4]; // KJ 01
        let second = imported.created_entities[5]; // KJ 02
        let before = drawing_of(&document);

        // The new run no longer computes point 4: string 02 is point 3 alone,
        // and one point makes no line.
        {
            let mut fake = fake.lock().unwrap();
            fake.shots.pop();
            fake.features[1].point_ids = vec![String::from("3")];
        }
        let command = readjust(&mut document, &fake);

        assert_eq!(command.changes().removed, ["4"]);
        assert!(
            is_line(&document, first),
            "KJ 01 is strung as it was, and stays"
        );
        assert!(!document.model().entities.contains(second));
        let job = &document.survey_jobs()[0];
        // Points 1, 2 and 3, and the one line.
        assert_eq!(
            job.created_entities,
            [
                imported.created_entities[0],
                imported.created_entities[1],
                imported.created_entities[2],
                first,
            ]
        );

        let finish = finished(command.finish_report());
        assert_eq!(finish.earlier_lines_redrawn, [first]);
        assert_eq!(finish.earlier_lines_removed, [second]);
        assert_eq!(finish.unplaced_features.len(), 1);
        assert_eq!(finish.unplaced_features[0].name, "KJ 02");
        assert_eq!(
            finish.unplaced_features[0].reason,
            UnplacedFeatureReason::TooFewPoints
        );
        let left = [
            "1 of the job's line(s) are no longer strung by this adjustment and were deleted, \
             with anything attached to them.",
            "1 string(s) of the file are in no line (fewer than two of its points have a \
             position: 1).",
        ];
        assert_eq!(
            describe(&finish),
            [
                "No new points were drawn, so no survey codes were applied.",
                "Linework: 0 line(s) drawn and 1 of the job's line(s) redrawn where this \
                 adjustment puts them.",
                left[0],
                left[1],
            ]
        );
        assert_eq!(finish_warnings(&finish), left);
        for sentence in left {
            assert!(job.report_text.contains(sentence), "{sentence}");
        }

        // One undo puts the point and the line back, as they were.
        assert!(document.undo().is_ok());
        assert_eq!(drawing_of(&document), before);
        assert_eq!(document.survey_jobs()[0], imported);
    }

    // ---- One option without the other, and what a re-adjustment is handed ----

    #[test]
    fn a_job_imported_for_codes_alone_is_stored_and_readjusted_for_codes_alone() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        let imported = import_job_with(&mut document, &fake, import_request(true, false));

        assert_eq!(
            document.survey_jobs()[0].import_options,
            format!("{DEFAULT_OPTIONS_TEXT}apply-codes=true\n")
        );
        assert_eq!(document.survey_jobs()[0].created_entities.len(), 4);
        assert_eq!(line_count(&document), 0);
        assert_eq!(numbered(&document, "1").layer, "SURVEY JOINT");
        let finish = finished(imported.finish_report());
        assert_eq!(finish.why_not_coded, SurveyFinishSkip::None);
        assert_eq!(finish.why_not_strung, SurveyFinishSkip::NotAsked);

        // A third shot of string 02. The caller's two flags say both; the
        // job's stored text says codes only, and that is what is done.
        {
            let mut fake = fake.lock().unwrap();
            fake.shots.push(shot("5", 5.0, 20.0, "KJ"));
            fake.features[1].point_ids.push(String::from("5"));
        }
        let asked = SurveyFinishOptions {
            codes: true,
            linework: true,
            ..SurveyFinishOptions::default()
        };
        let command = readjust_how(&mut document, &fake, asked, HandEditPolicy::Keep);

        let fifth = numbered(&document, "5");
        assert_eq!(fifth.layer, "SURVEY JOINT");
        assert_eq!(fifth.style, "Joint Line");
        assert_eq!(
            line_count(&document),
            0,
            "a job never strung is not strung now"
        );
        let finish = finished(command.finish_report());
        assert_eq!(finish.coding.coded, 1);
        assert_eq!(finish.why_not_strung, SurveyFinishSkip::NotAsked);
        assert_eq!(document.survey_jobs()[0].created_entities.len(), 5);
    }

    #[test]
    fn a_job_imported_for_linework_alone_is_stored_and_readjusted_for_linework_alone() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        let fake = two_joints();
        let imported = import_job_with(&mut document, &fake, import_request(false, true));

        let job = document.survey_jobs()[0].clone();
        assert_eq!(
            job.import_options,
            format!("{DEFAULT_OPTIONS_TEXT}draw-linework=true\n")
        );
        assert_eq!(job.created_entities.len(), 6);
        let second = job.created_entities[5]; // KJ 02
        // The points are where the import put them, unstyled; the lines are on
        // their rule's layer in its style.
        assert_eq!(numbered(&document, "1").layer, "day1");
        assert_eq!(numbered(&document, "1").style, "");
        assert_eq!(entity(&document, second).layer, "SURVEY JOINT");
        assert_eq!(entity(&document, second).style, "Joint Line");
        let finish = finished(imported.finish_report());
        assert_eq!(finish.why_not_coded, SurveyFinishSkip::NotAsked);
        assert_eq!(finish.why_not_strung, SurveyFinishSkip::None);

        {
            let mut fake = fake.lock().unwrap();
            fake.shots.push(shot("5", 5.0, 20.0, "KJ"));
            fake.features[1].point_ids.push(String::from("5"));
        }
        let asked = SurveyFinishOptions {
            codes: true,
            linework: true,
            ..SurveyFinishOptions::default()
        };
        let command = readjust_how(&mut document, &fake, asked, HandEditPolicy::Keep);

        let fifth = numbered(&document, "5");
        assert_eq!(fifth.layer, "day1", "a job never coded is not coded now");
        assert_eq!(fifth.style, "");
        // KJ 02 runs on through the new point, as the entity it was.
        assert!(is_line(&document, second));
        assert_eq!(
            vertices_of(entity(&document, second)),
            [p(0.0, 5.0), p(10.0, 5.0), p(20.0, 5.0)]
        );
        assert_eq!(line_count(&document), 2);
        let finish = finished(command.finish_report());
        assert_eq!(finish.why_not_coded, SurveyFinishSkip::NotAsked);
        assert_eq!(finish.why_not_strung, SurveyFinishSkip::None);
        assert_eq!(
            finish.earlier_lines_redrawn,
            [job.created_entities[4], second]
        );
    }

    // HOW a job is finished again is the caller's: here the colour lookup,
    // which is not job data. Handed on, the new point wears the style its
    // neighbours were given; without it "red" is a name with no colour, the
    // neighbours' style is not one that draws it, and a second style is made.
    #[test]
    fn a_readjustment_styles_a_new_point_with_the_colour_lookup_it_is_handed() {
        let mut map = SurveyMap::default();
        assert!(map.add(joint_rule("red")).is_ok());
        let red_colour = Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        };
        let red: ColourOf = Arc::new(move |name: &str| (name == "red").then_some(red_colour));
        for handed_on in [true, false] {
            let trace = if handed_on {
                "with the lookup"
            } else {
                "without it"
            };
            let mut document = Document::default();
            document.set_survey_map(map.clone());
            let fake = two_joints();
            let mut request = import_request(true, true);
            request.finish.coding.colour_of = Some(Arc::clone(&red));
            import_job_with(&mut document, &fake, request);
            let Some(style) = document.model().styles.find("Joint Line") else {
                panic!("the style Joint Line was made ({trace})");
            };
            assert_eq!(style.color, Some(red_colour), "{trace}");
            assert_eq!(document.model().styles.all().len(), 1, "{trace}");

            {
                let mut fake = fake.lock().unwrap();
                fake.shots.push(shot("5", 5.0, 20.0, "KJ"));
                fake.features[1].point_ids.push(String::from("5"));
            }
            let mut how = SurveyFinishOptions::default();
            if handed_on {
                how.coding.colour_of = Some(Arc::clone(&red));
            }
            let command = readjust_how(&mut document, &fake, how, HandEditPolicy::Keep);

            let fifth = numbered(&document, "5");
            let finish = finished(command.finish_report());
            if handed_on {
                assert_eq!(fifth.style, "Joint Line", "{trace}");
                assert!(finish.styles_created().is_empty(), "{trace}");
                assert_eq!(document.model().styles.all().len(), 1, "{trace}");
            } else {
                assert_eq!(fifth.style, "Joint Line (red)", "{trace}");
                assert_eq!(finish.styles_created(), ["Joint Line (red)"], "{trace}");
            }
        }
    }
}

// ---- The stored options ----------------------------------------------------------------

mod survey_job_options_text {
    use super::*;

    #[test]
    fn a_job_remembers_that_it_was_finished_and_one_that_was_not_writes_what_it_always_did() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        import_job(&mut document, &two_joints(), true);
        assert_eq!(
            document.survey_jobs()[0].import_options,
            format!("{DEFAULT_OPTIONS_TEXT}apply-codes=true\ndraw-linework=true\n")
        );

        // Not asked for: the keys are not written at all.
        assert_eq!(
            write_survey_job_options(&SurveyJobOptions::default()),
            DEFAULT_OPTIONS_TEXT
        );
    }

    #[test]
    fn the_text_round_trips_with_and_without_the_two_keys() {
        for codes in [false, true] {
            for linework in [false, true] {
                let trace = format!(
                    "{}{}",
                    if codes { "codes " } else { "" },
                    if linework { "linework" } else { "" }
                );
                let options = SurveyJobOptions {
                    import: SurveyImportOptions {
                        layer_per_code: true,
                        code_property: String::from("field%code"),
                        record_source: false,
                        ..SurveyImportOptions::default()
                    },
                    apply_codes: codes,
                    draw_linework: linework,
                };

                let text = write_survey_job_options(&options);
                assert_eq!(text.contains("apply-codes=true\n"), codes, "{trace}");
                assert_eq!(text.contains("draw-linework=true\n"), linework, "{trace}");
                let read = expect_ok(read_survey_job_options(&text, "job-7"));
                assert_eq!(read.apply_codes, codes, "{trace}");
                assert_eq!(read.draw_linework, linework, "{trace}");
                assert!(read.import.layer_per_code, "{trace}");
                assert_eq!(read.import.code_property, "field%code", "{trace}");
                assert!(!read.import.record_source, "{trace}");
                assert_eq!(write_survey_job_options(&read), text, "{trace}");
            }
        }
    }

    #[test]
    fn a_text_without_the_keys_reads_as_off_and_a_malformed_one_names_the_job() {
        // The text of every job imported before the keys existed, and empty
        // text.
        for text in [DEFAULT_OPTIONS_TEXT, ""] {
            let old = expect_ok(read_survey_job_options(text, "job-7"));
            assert!(!old.apply_codes, "{text:?}");
            assert!(!old.draw_linework, "{text:?}");
        }
        let off = expect_ok(read_survey_job_options(
            &format!("{DEFAULT_OPTIONS_TEXT}apply-codes=false\n"),
            "job-7",
        ));
        assert!(!off.apply_codes);

        let bad = read_survey_job_options(
            &format!("{DEFAULT_OPTIONS_TEXT}draw-linework=yes\n"),
            "job-7",
        );
        let Err(error) = bad else {
            panic!("draw-linework=yes is refused");
        };
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(error.message.contains("job-7"), "{}", error.message);
    }
}
