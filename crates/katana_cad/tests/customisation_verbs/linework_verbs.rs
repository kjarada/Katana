//! LINEWORK on the command line (linework_verbs), and survey_import_finish, which says what the
//! drawing's customisation asks of a survey import (tests/cad/customisation/test_linework_verbs.cpp).
//!
//! Every expected value is worked by hand from the points a test lists: a point is (x, y), its
//! number is the property "point" and its code "code", as a survey import writes them. Entity ids
//! are monotonic, so in a drawing that held nothing the n-th entity made is entity n; a survey job
//! takes one id for its own number before its points take theirs.

use katana_cad::linework::process_linework;
use katana_cad::linework_verbs::{
    LINEWORK_STRINGS_LISTED, is_linework_verb, linework_verb_help, plan_linework,
    survey_import_finish,
};
use katana_cad::{
    CommandInterpreter, Document, ImportSurveyJobCommand, LineworkOptions, LineworkOrder,
    LineworkWords, ReductionFunction, SurveyFinishOptions, SurveyImportOptions, SurveyJob,
    SurveyJobAccess, SurveyJobContext, SurveyJobImport, UnplacedReason,
};
use katana_commands::{create_entities, delete_entities};
use katana_core::{ErrorCode, Result};
use katana_entity::entity::{heights_of, set_heights};
use katana_entity::{
    Color, ColourTable, CustomisationAutomation, Entity, EntityId, Geometry, LineworkCodes,
    PointGeometry, PropertyValue, SurveyBreakline, SurveyMap, SurveyRule,
};
use katana_geometry::{Point2, Polyline2};
use katana_survey::data_model::{CoordinateSource, SurveyFeature, SurveyPoint, SurveyProject};
use katana_survey::reduction::{ComputedPoint, ReductionContext, ReductionOutcome};
use katana_survey::reduction_report::ReductionReport;
use katana_survey::reduction_settings::ReductionSettings;

use crate::must;

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn text_value(text: &str) -> PropertyValue {
    PropertyValue::Text(String::from(text))
}

// A kerb is a LINE on KERB, drawn plain in the customisation's own colour
// "kerb red"; a mark is a POINT on MARKS.
fn kerb_map() -> SurveyMap {
    let mut map = SurveyMap::default();
    let kerb = SurveyRule {
        key: String::from("KB*"),
        model: String::from("KERB"),
        colour: String::from("kerb red"),
        breakline: Some(SurveyBreakline::Line),
        ..SurveyRule::default()
    };
    assert!(map.add(kerb).is_ok());
    let mark = SurveyRule {
        key: String::from("MK"),
        model: String::from("MARKS"),
        breakline: Some(SurveyBreakline::Point),
        ..SurveyRule::default()
    };
    assert!(map.add(mark).is_ok());
    map
}

struct Shot {
    number: String,
    x: f64,
    y: f64,
    code: String,
}

fn shot(number: &str, x: f64, y: f64, code: &str) -> Shot {
    Shot {
        number: String::from(number),
        x,
        y,
        code: String::from(code),
    }
}

// The shots as point entities on `layer`, made by ONE command: one undo step,
// and ids in the order listed.
#[track_caller]
fn add_points_on(document: &mut Document, shots: &[Shot], layer: &str) -> Vec<EntityId> {
    let mut entities = Vec::new();
    for shot in shots {
        let mut entity = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: p(shot.x, shot.y),
            }),
            layer: String::from(layer),
            ..Entity::default()
        };
        if !shot.number.is_empty() {
            entity
                .properties
                .insert(String::from("point"), text_value(&shot.number));
        }
        if !shot.code.is_empty() {
            entity
                .properties
                .insert(String::from("code"), text_value(&shot.code));
        }
        entities.push(entity);
    }
    must(document.execute(create_entities(entities)));
    document.last_created_entities()
}

// ... on layer 0.
#[track_caller]
fn add_points(document: &mut Document, shots: &[Shot]) -> Vec<EntityId> {
    add_points_on(document, shots, "0")
}

// Two kerbs and a mark:
//   KB1: 1 (0,0)  2 (10,0)  3 (20,0)
//   KB2: 4 (0,5)  5 (10,5)
//   MK:  6 (5,9)
#[track_caller]
fn two_kerbs_and_a_mark(document: &mut Document) -> Vec<EntityId> {
    add_points(
        document,
        &[
            shot("1", 0.0, 0.0, "KB1"),
            shot("2", 10.0, 0.0, "KB1"),
            shot("3", 20.0, 0.0, "KB1"),
            shot("4", 0.0, 5.0, "KB2"),
            shot("5", 10.0, 5.0, "KB2"),
            shot("6", 5.0, 9.0, "MK"),
        ],
    )
}

// A shot with a height, as a survey import draws one: a point's height is
// the one writer's (entity's set_heights).
fn levelled_point(number: &str, x: f64, y: f64, code: &str, height: f64) -> Entity {
    let mut entity = Entity {
        geometry: Geometry::Point(PointGeometry { position: p(x, y) }),
        ..Entity::default()
    };
    entity
        .properties
        .insert(String::from("point"), text_value(number));
    entity
        .properties
        .insert(String::from("code"), text_value(code));
    set_heights(&mut entity.properties, &[Some(height)]);
    entity
}

// A polyline somebody left in the drawing, carrying `code` as linework's own
// lines carry it and, when `string` is not empty, a string number beside it.
fn coded_line(
    vertices: Vec<Point2>,
    closed: bool,
    code: &str,
    string: &str,
    heights: &[Option<f64>],
    layer: &str,
) -> Entity {
    let mut entity = Entity {
        geometry: Geometry::Polyline(Polyline2::new(vertices, closed)),
        layer: String::from(layer),
        ..Entity::default()
    };
    entity
        .properties
        .insert(String::from("code"), text_value(code));
    if !string.is_empty() {
        entity
            .properties
            .insert(String::from("string"), text_value(string));
    }
    set_heights(&mut entity.properties, heights);
    entity
}

// The entities, made by ONE command, in the order given.
#[track_caller]
fn add_entities(document: &mut Document, entities: Vec<Entity>) -> Vec<EntityId> {
    must(document.execute(create_entities(entities)));
    document.last_created_entities()
}

// The reply to `line` from an interpreter of its own, failing the test when it is refused.
#[track_caller]
fn run(document: &mut Document, line: &str) -> String {
    let mut interpreter = CommandInterpreter::new();
    interpreter
        .run(document, line)
        .unwrap_or_else(|error| panic!("{line}: {}", error.describe()))
}

// The reply's lines, the one after its last line end included.
fn lines_of(reply: &str) -> Vec<String> {
    reply.split('\n').map(String::from).collect()
}

fn polylines_of(document: &Document) -> Vec<Entity> {
    let mut found = Vec::new();
    document.model().entities.for_each(|entity| {
        if matches!(entity.geometry, Geometry::Polyline(_)) {
            found.push(entity.clone());
        }
    });
    found
}

#[track_caller]
fn shape_of(line: &Entity) -> Polyline2 {
    match &line.geometry {
        Geometry::Polyline(polyline) => polyline.clone(),
        other => panic!("not a polyline: {other:?}"),
    }
}

#[track_caller]
fn vertices_of(line: &Entity) -> Vec<Point2> {
    shape_of(line).vertices
}

#[track_caller]
fn entity_of(document: &Document, id: EntityId) -> Entity {
    document
        .model()
        .entities
        .find(id)
        .unwrap_or_else(|| panic!("no entity {id}"))
        .clone()
}

fn text_property(entity: &Entity, key: &str) -> String {
    match entity.properties.get(key) {
        None => String::from("<absent>"),
        Some(PropertyValue::Text(text)) => text.clone(),
        Some(_) => String::from("<not text>"),
    }
}

// ---- a survey job, as tests/cad/test_survey_job_finish.cpp makes one -----------------------
//
// The reduction is injected: it returns the shots and the features listed,
// which is a field file that keeps code and string number apart.

struct FieldShot {
    id: &'static str,
    northing: f64,
    easting: f64,
    code: &'static str,
}

fn field_shot(id: &'static str, northing: f64, easting: f64, code: &'static str) -> FieldShot {
    FieldShot {
        id,
        northing,
        easting,
        code,
    }
}

fn reduction_of(shots: Vec<FieldShot>, features: Vec<SurveyFeature>) -> ReductionFunction {
    Box::new(
        move |raw: &SurveyProject,
              settings: &ReductionSettings,
              context: &ReductionContext|
              -> Result<ReductionOutcome> {
            let mut points = Vec::new();
            let mut computed = Vec::new();
            for shot in &shots {
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
                    features: features.clone(),
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
        name: String::from(number),
        point_ids: ids.iter().map(|id| String::from(*id)).collect(),
        ..SurveyFeature::default()
    }
}

// "KJ*": a joint is a LINE on JOINTS.
fn joint_map() -> SurveyMap {
    let mut map = SurveyMap::default();
    let joint = SurveyRule {
        key: String::from("KJ*"),
        model: String::from("JOINTS"),
        breakline: Some(SurveyBreakline::Line),
        ..SurveyRule::default()
    };
    assert!(map.add(joint).is_ok());
    map
}

// Imports a job of two strings of the joint code KJ, two shots each, on the
// layer "day1" (a survey point is (northing, easting); drawn, (x, y) is
// (easting, northing)):
//   KJ 01: point 1 (N 0, E 0) and point 2 (N 0, E 10)
//   KJ 02: point 3 (N 5, E 0) and point 4 (N 5, E 10)
// In a drawing that held nothing the job is job-1, the points are entities 2
// to 5 and - coded and strung - the lines KJ 01 and KJ 02 are entities 6 and 7.
#[track_caller]
fn import_two_joints(document: &mut Document, codes: bool, linework: bool) {
    let shots = vec![
        field_shot("1", 0.0, 0.0, "KJ"),
        field_shot("2", 0.0, 10.0, "KJ"),
        field_shot("3", 5.0, 0.0, "KJ"),
        field_shot("4", 5.0, 10.0, "KJ"),
    ];
    let features = vec![
        feature("KJ", "01", &["1", "2"]),
        feature("KJ", "02", &["3", "4"]),
    ];
    let request = SurveyJobImport {
        job: SurveyJob {
            name: String::from("DAY1.FLD"),
            format_id: String::from("opcode-field-file"),
            parser_version: String::from("1.0"),
            source_file_name: String::from("DAY1.FLD"),
            source_bytes: b"the field file's bytes".to_vec(),
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
    };
    let command =
        ImportSurveyJobCommand::new(document, request, Some(reduction_of(shots, features)));
    must(document.execute(Box::new(command)));
}

// Every entity of the drawing, in the order the drawing visits them.
fn every_entity(document: &Document) -> Vec<EntityId> {
    let mut ids = Vec::new();
    document
        .model()
        .entities
        .for_each(|entity| ids.push(entity.id));
    ids
}

// ---- the grammar -------------------------------------------------------------------------

mod linework_verb {
    use super::*;

    #[test]
    fn a_word_the_grammar_does_not_take_is_refused_by_name_and_nothing_is_drawn() {
        struct Refusal {
            line: &'static str,
            code: ErrorCode,
            said: &'static str,
            word: &'static str,
        }
        fn refusal(
            line: &'static str,
            code: ErrorCode,
            said: &'static str,
            word: &'static str,
        ) -> Refusal {
            Refusal {
                line,
                code,
                said,
                word,
            }
        }
        let refusals = [
            refusal(
                "LINEWORK ORDER",
                ErrorCode::ParseFailure,
                "ORDER needs number or entity after it",
                "ORDER",
            ),
            refusal(
                "LINEWORK ORDER sideways",
                ErrorCode::ParseFailure,
                "ORDER takes number",
                "sideways",
            ),
            refusal(
                "LINEWORK ORDER number ORDER entity",
                ErrorCode::ParseFailure,
                "ORDER is given twice",
                "ORDER",
            ),
            // PROPERTY and CHORD each take the word after them, once.
            refusal(
                "LINEWORK PROPERTY",
                ErrorCode::ParseFailure,
                "PROPERTY needs the property's name after it",
                "PROPERTY",
            ),
            refusal(
                "LINEWORK PROPERTY \"\"",
                ErrorCode::ParseFailure,
                "PROPERTY needs the property's name after it",
                "PROPERTY",
            ),
            refusal(
                "LINEWORK PROPERTY code PROPERTY code",
                ErrorCode::ParseFailure,
                "PROPERTY is given twice",
                "PROPERTY",
            ),
            refusal(
                "LINEWORK CHORD",
                ErrorCode::ParseFailure,
                "CHORD needs a length after it",
                "CHORD",
            ),
            refusal(
                "LINEWORK CHORD 0.1 CHORD 0.1",
                ErrorCode::ParseFailure,
                "CHORD is given twice",
                "CHORD",
            ),
            // A chord length is a length: zero would ask for chords without end,
            // and a negative, a non-finite or a worded one measures nothing.
            refusal(
                "LINEWORK CHORD 0",
                ErrorCode::ParseFailure,
                "CHORD takes a length greater than 0",
                "0",
            ),
            refusal(
                "LINEWORK CHORD -0.005",
                ErrorCode::ParseFailure,
                "CHORD takes a length greater than 0",
                "-0.005",
            ),
            refusal(
                "LINEWORK CHORD inf",
                ErrorCode::ParseFailure,
                "CHORD takes a length greater than 0",
                "inf",
            ),
            refusal(
                "LINEWORK CHORD fine",
                ErrorCode::ParseFailure,
                "CHORD takes a length greater than 0",
                "fine",
            ),
            // A scope after the verb's own words is not read as one.
            refusal(
                "LINEWORK PREVIEW DRAWING",
                ErrorCode::ParseFailure,
                "the scope and its WHERE come first",
                "DRAWING",
            ),
            refusal(
                "LINEWORK ORDER entity WHERE TYPE=point",
                ErrorCode::ParseFailure,
                "the scope and its WHERE come first",
                "WHERE",
            ),
            refusal(
                "LINEWORK KEEP",
                ErrorCode::ParseFailure,
                "not a LINEWORK word",
                "KEEP",
            ),
            // The shared scope parser's own refusals, in its words.
            refusal(
                "LINEWORK DRAWING SELECTION",
                ErrorCode::InvalidArgument,
                "give one scope",
                "SELECTION",
            ),
            refusal(
                "LINEWORK WHERE TYPE=blob",
                ErrorCode::InvalidArgument,
                "",
                "blob",
            ),
            refusal(
                "LINEWORK LAYERS nowhere",
                ErrorCode::NotFound,
                "layer does not exist",
                "nowhere",
            ),
            // Headless there is no view: VIEW is refused in favour of AREA.
            refusal(
                "LINEWORK VIEW",
                ErrorCode::InvalidState,
                "AREA x0,y0,x1,y1",
                "",
            ),
        ];
        for refusal in &refusals {
            let line = refusal.line;
            let mut document = Document::default();
            document.set_survey_map(kerb_map());
            two_kerbs_and_a_mark(&mut document);
            let mut interpreter = CommandInterpreter::new();
            let error = match interpreter.run(&mut document, line) {
                Ok(reply) => panic!("{line} was accepted: {reply}"),
                Err(error) => error,
            };
            // The type of a WHERE value is the scope parser's to refuse, with
            // whatever code it uses: only that it names the word is held here.
            if refusal.said.is_empty() {
                assert!(
                    error.describe().contains(refusal.word),
                    "{line}: {}",
                    error.describe()
                );
            } else {
                assert_eq!(error.code, refusal.code, "{line}: {}", error.describe());
                assert!(
                    error.message.contains(refusal.said),
                    "{line}: {}",
                    error.describe()
                );
                assert!(
                    error.describe().contains(refusal.word),
                    "{line}: {}",
                    error.describe()
                );
            }
            assert!(polylines_of(&document).is_empty(), "{line}");
            assert_eq!(
                document.history().undo_count(),
                1,
                "the points, and no step after them: {line}"
            );
        }
    }

    #[test]
    fn the_verb_and_its_words_are_read_in_any_case() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        two_kerbs_and_a_mark(&mut document);
        assert_eq!(
            run(&mut document, "linework drawing order ENTITY preview"),
            run(&mut document, "LINEWORK DRAWING ORDER entity PREVIEW")
        );
    }

    // ---- the scope -----------------------------------------------------------------------

    #[test]
    fn with_no_scope_word_it_takes_the_selection_when_there_is_one_else_the_drawing_and_says_which()
    {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        let ids = two_kerbs_and_a_mark(&mut document);
        assert_eq!(ids.len(), 6);

        // Nothing selected: the whole drawing. Six points; KB1 and KB2 are lines
        // of 3 and 2 points, and the mark is a point code.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK PREVIEW")),
            [
                "linework scope=drawing matched=6 considered=6 lines=2 unplaced=1 notes=0 \
                 preview=yes",
                "string name=KB1 key=KB* number=1 points=3 vertices=3 closed=no layer=KERB",
                "string name=KB2 key=KB* number=2 points=2 vertices=2 closed=no layer=KERB",
                "unplaced reason=\"its code is a point code\" points=1"
            ]
        );

        // Points 4 and 5 selected: the selection, which is KB2 alone.
        document.selection_mut().set(vec![ids[3], ids[4]]);
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK PREVIEW")),
            [
                "linework scope=selection matched=2 considered=2 lines=1 unplaced=0 notes=0 \
                 preview=yes",
                "string name=KB2 key=KB* number=2 points=2 vertices=2 closed=no layer=KERB"
            ]
        );
        // A scope word is the scope, whatever is selected.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK DRAWING PREVIEW"))[0],
            "linework scope=drawing matched=6 considered=6 lines=2 unplaced=1 notes=0 preview=yes"
        );
        // SELECTION with nothing selected takes nothing, and says so: no failure.
        document.selection_mut().clear();
        assert_eq!(
            run(&mut document, "LINEWORK SELECTION PREVIEW"),
            "linework scope=selection matched=0 considered=0 lines=0 unplaced=0 notes=0 \
             preview=yes"
        );
        // A bare WHERE filters the default scope: the drawing, nothing being
        // selected, and of it the two points whose code is KB2.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK WHERE PROP=code:KB2 PREVIEW"))[0],
            "linework scope=drawing where=\"PROP=code:KB2\" matched=2 considered=2 lines=1 \
             unplaced=0 notes=0 preview=yes"
        );
    }

    #[test]
    fn a_layer_an_area_and_a_filter_each_string_only_what_they_take() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        run(&mut document, "LAYER NEW north");
        run(&mut document, "LAYER NEW south");
        //   KB1 on north: 1 (0,100)  2 (10,100)
        //   KB2 on south: 3 (0,0)    4 (10,0)
        add_points_on(
            &mut document,
            &[shot("1", 0.0, 100.0, "KB1"), shot("2", 10.0, 100.0, "KB1")],
            "north",
        );
        add_points_on(
            &mut document,
            &[shot("3", 0.0, 0.0, "KB2"), shot("4", 10.0, 0.0, "KB2")],
            "south",
        );

        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK LAYERS north PREVIEW")),
            [
                "linework scope=layers layers=north sublayers=yes matched=2 considered=2 \
                 lines=1 unplaced=0 notes=0 preview=yes",
                "string name=KB1 key=KB* number=1 points=2 vertices=2 closed=no layer=KERB"
            ]
        );
        // The window (-1,-1) to (11,1) holds points 3 and 4 only.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK AREA -1,-1,11,1 PREVIEW")),
            [
                "linework scope=area area=-1,-1,11,1 matched=2 considered=2 lines=1 \
                 unplaced=0 notes=0 preview=yes",
                "string name=KB2 key=KB* number=2 points=2 vertices=2 closed=no layer=KERB"
            ]
        );
        assert_eq!(
            lines_of(&run(
                &mut document,
                "LINEWORK DRAWING WHERE LAYER=south PREVIEW"
            ))[0],
            "linework scope=drawing where=\"LAYER=south\" matched=2 considered=2 lines=1 \
             unplaced=0 notes=0 preview=yes"
        );
    }

    // ---- preview, and the run ------------------------------------------------------------

    #[test]
    fn a_preview_plans_and_changes_nothing() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        two_kerbs_and_a_mark(&mut document);
        let revision = document.model_revision();

        let reply = run(&mut document, "LINEWORK PREVIEW");

        assert!(reply.contains(" lines=2 "), "{reply}");
        assert!(!reply.contains("entity="), "nothing was drawn to name");
        assert_eq!(document.model().entities.size(), 6);
        assert!(!document.model().layers.contains("KERB"));
        assert_eq!(document.history().undo_count(), 1);
        assert_eq!(document.model_revision(), revision);
    }

    #[test]
    fn a_run_draws_styled_lines_as_one_undo_step_and_removes_no_point() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        // The rule's colour is a name of the customisation's own: only the
        // Document's resolver knows it is #AA0000.
        let mut colours = ColourTable::default();
        assert!(colours.add("kerb red", Color::new(170, 0, 0, 255)).is_ok());
        document.set_colour_table(colours);
        two_kerbs_and_a_mark(&mut document);

        // The points are entities 1 to 6, so the lines are 7 and 8, in the order
        // the reply lists them.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=6 considered=6 lines=2 unplaced=1 notes=0",
                "string name=KB1 key=KB* number=1 points=3 vertices=3 closed=no layer=KERB \
                 entity=7",
                "string name=KB2 key=KB* number=2 points=2 vertices=2 closed=no layer=KERB \
                 entity=8",
                "unplaced reason=\"its code is a point code\" points=1"
            ]
        );

        let first = entity_of(&document, 7);
        let second = entity_of(&document, 8);
        assert_eq!(
            vertices_of(&first),
            [p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)]
        );
        assert_eq!(vertices_of(&second), [p(0.0, 5.0), p(10.0, 5.0)]);
        assert_eq!(first.layer, "KERB");
        assert_eq!(text_property(&first, "code"), "KB1");
        // A plain line in a colour: the style is named "Plain" (survey_coding.rs,
        // which names a style after its linestyle, else its symbol, else that).
        assert_eq!(first.style, "Plain");
        let style = document
            .model()
            .styles
            .find("Plain")
            .expect("the style Plain");
        assert_eq!(style.color, Some(Color::new(170, 0, 0, 255)));

        // No point was removed, and none was coded: that is CODE's.
        let mut points = 0;
        document.model().entities.for_each(|entity| {
            if matches!(entity.geometry, Geometry::Point(_)) {
                points += 1;
                assert_eq!(entity.layer, "0");
                assert_eq!(entity.style, "");
            }
        });
        assert_eq!(points, 6);

        // One step: the lines, their layer and their style go together.
        assert_eq!(document.history().undo_count(), 2);
        must(document.undo());
        assert_eq!(document.model().entities.size(), 6);
        assert!(polylines_of(&document).is_empty());
        assert!(!document.model().layers.contains("KERB"));
        assert!(document.model().styles.find("Plain").is_none());
        assert_eq!(document.history().undo_count(), 1);
    }

    #[test]
    fn the_control_codes_are_the_ones_the_document_holds() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        //   KB1: 1 (0,0)  2 (10,0)  3 (20,0) "S"  4 (30,0)
        add_points(
            &mut document,
            &[
                shot("1", 0.0, 0.0, "KB1"),
                shot("2", 10.0, 0.0, "KB1"),
                shot("3", 20.0, 0.0, "KB1 S"),
                shot("4", 30.0, 0.0, "KB1"),
            ],
        );

        // By the default spellings "S" is no control: one line through all four,
        // and the token is noted.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK PREVIEW")),
            [
                "linework scope=drawing matched=4 considered=4 lines=1 unplaced=0 notes=1 \
                 preview=yes",
                "string name=KB1 key=KB* number=1 points=4 vertices=4 closed=no layer=KERB",
                "note kind=\"unknown token\" count=1"
            ]
        );

        // The customisation spells start "S": point 3 begins a second line.
        let codes = LineworkCodes {
            start: String::from("S"),
            ..LineworkCodes::default()
        };
        must(document.set_linework_codes(codes));
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK PREVIEW")),
            [
                "linework scope=drawing matched=4 considered=4 lines=2 unplaced=0 notes=0 \
                 preview=yes",
                "string name=KB1 key=KB* number=1 points=2 vertices=2 closed=no layer=KERB",
                "string name=KB1 key=KB* number=1 points=2 vertices=2 closed=no layer=KERB"
            ]
        );
    }

    #[test]
    fn order_says_whether_a_string_runs_by_point_number_or_as_its_points_were_drawn() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        // Drawn in this order: number 3 at (0,0), 1 at (10,0), 2 at (20,0), and a
        // shot with no number at (30,0).
        add_points(
            &mut document,
            &[
                shot("3", 0.0, 0.0, "KB1"),
                shot("1", 10.0, 0.0, "KB1"),
                shot("2", 20.0, 0.0, "KB1"),
                shot("", 30.0, 0.0, "KB1"),
            ],
        );

        // ORDER number is the default, said aloud.
        assert_eq!(
            run(&mut document, "LINEWORK ORDER number PREVIEW"),
            run(&mut document, "LINEWORK PREVIEW")
        );
        // By number: 1, 2, 3; the shot with no number has no place.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=4 considered=4 lines=1 unplaced=1 notes=0",
                "string name=KB1 key=KB* number=1 points=3 vertices=3 closed=no layer=KERB \
                 entity=5",
                "unplaced reason=\"no point number to order it by\" points=1"
            ]
        );
        assert_eq!(
            vertices_of(&entity_of(&document, 5)),
            [p(10.0, 0.0), p(20.0, 0.0), p(0.0, 0.0)]
        );
        must(document.undo());

        // As drawn: all four, in the order they were made.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK ORDER entity")),
            [
                "linework scope=drawing matched=4 considered=4 lines=1 unplaced=0 notes=0",
                "string name=KB1 key=KB* number=1 points=4 vertices=4 closed=no layer=KERB \
                 entity=6"
            ]
        );
        assert_eq!(
            vertices_of(&entity_of(&document, 6)),
            [p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0), p(30.0, 0.0)]
        );
    }

    // PROPERTY, as CODE has it: three shots of one kerb whose code is kept under
    // "fcode", a name that is not among those a code is looked for under
    // (survey_coding's code_property_candidates). Found, they carry no code at
    // all; named, they are KB1: one line through the three.
    #[test]
    fn property_names_the_property_the_codes_are_read_from() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        let mut shots = Vec::new();
        for i in 0..3 {
            let mut point = Entity {
                geometry: Geometry::Point(PointGeometry {
                    position: p(10.0 * f64::from(i), 0.0),
                }),
                ..Entity::default()
            };
            point
                .properties
                .insert(String::from("point"), text_value(&format!("{}", i + 1)));
            point
                .properties
                .insert(String::from("fcode"), text_value("KB1"));
            shots.push(point);
        }
        add_entities(&mut document, shots);

        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK PREVIEW")),
            [
                "linework scope=drawing matched=3 considered=3 lines=0 unplaced=3 notes=0 \
                 preview=yes",
                "unplaced reason=\"no code\" points=3"
            ]
        );
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK PROPERTY fcode PREVIEW")),
            [
                "linework scope=drawing matched=3 considered=3 lines=1 unplaced=0 notes=0 \
                 preview=yes",
                "string name=KB1 key=KB* number=1 points=3 vertices=3 closed=no layer=KERB"
            ]
        );

        // Drawn, the line carries the code where its points carry it, so that
        // CODE PROPERTY fcode finds the rule for the line that it finds for them.
        run(&mut document, "LINEWORK DRAWING PROPERTY fcode");
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            vertices_of(&lines[0]),
            [p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)]
        );
        assert_eq!(text_property(&lines[0], "fcode"), "KB1");
        assert_eq!(text_property(&lines[0], "code"), "<absent>");
    }

    // CHORD: a half circle of radius 10 about the origin, as the linework cases
    // of tests/cad/customisation/test_linework.cpp work it - begun at (10,0),
    // through (0,10), ended at (-10,0), each quarter chorded on its own.
    //   unsaid, 0.005: the largest step is 2 acos(1 - 0.005/10) = 0.063248 rad,
    //     and a quarter, 1.570796 rad, is 24.84 of them: 25 chords a quarter,
    //     1 + 25 + 25 = 51 vertices;
    //   CHORD 0.35: 2 acos(1 - 0.35/10) = 2 x 15.2 = 30.4 degrees, so 3 chords of
    //     30 degrees a quarter: 1 + 3 + 3 = 7 vertices, at 0, 30 ... 180 degrees.
    #[test]
    fn chord_says_how_far_the_segments_drawn_for_a_curve_may_stray_from_it() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        add_points(
            &mut document,
            &[
                shot("1", 10.0, 0.0, "KB1 BC"),
                shot("2", 0.0, 10.0, "KB1"),
                shot("3", -10.0, 0.0, "KB1 EC"),
            ],
        );

        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK PREVIEW")),
            [
                "linework scope=drawing matched=3 considered=3 lines=1 unplaced=0 notes=0 \
                 preview=yes",
                "string name=KB1 key=KB* number=1 points=3 vertices=51 closed=no layer=KERB"
            ]
        );
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK CHORD 0.35 PREVIEW")),
            [
                "linework scope=drawing matched=3 considered=3 lines=1 unplaced=0 notes=0 \
                 preview=yes",
                "string name=KB1 key=KB* number=1 points=3 vertices=7 closed=no layer=KERB"
            ]
        );

        run(&mut document, "LINEWORK CHORD 0.35");
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        let vertices = vertices_of(&lines[0]);
        assert_eq!(vertices.len(), 7);
        assert_eq!(
            vertices[3],
            p(0.0, 10.0),
            "the surveyed point is a vertex, exactly"
        );
        // The middle of each 30 degree chord is 10 cos 15 deg = 9.659 from the
        // centre: 0.341 inside the arc, within the 0.35 asked for.
        for (chord, ends) in (1..).zip(vertices.windows(2)) {
            let middle = (ends[0] + ends[1]) * 0.5;
            let inside = 10.0 - middle.length();
            assert!((inside - 0.341).abs() <= 0.001, "chord {chord}: {inside}");
        }
    }

    // The fields of a string record that a line of plain shots cannot tell apart:
    // how many points against how many vertices, whether it closed, whether it is
    // a join.
    #[test]
    fn a_string_record_says_its_points_its_vertices_whether_it_closed_and_whether_it_is_a_join() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        //   KB1: 1 (0,0)   2 (10,0)   3 (10,5) "RECT"   three points, the rectangle on the
        //        side 1-2 as wide as 3 stands off it: 4 vertices, the last (0,5)
        //   KB2: 4 (0,20)  5 (10,20)  6 (10,30) "CL"    a closed triangle
        //   KB3: 7 (0,40)  8 (10,40) "JPN 1"            a run of two, and a join to point 1
        add_points(
            &mut document,
            &[
                shot("1", 0.0, 0.0, "KB1"),
                shot("2", 10.0, 0.0, "KB1"),
                shot("3", 10.0, 5.0, "KB1 RECT"),
                shot("4", 0.0, 20.0, "KB2"),
                shot("5", 10.0, 20.0, "KB2"),
                shot("6", 10.0, 30.0, "KB2 CL"),
                shot("7", 0.0, 40.0, "KB3"),
                shot("8", 10.0, 40.0, "KB3 JPN 1"),
            ],
        );

        // By name, and of one name its runs before its joins: entities 9 to 12.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=8 considered=8 lines=4 unplaced=0 notes=0",
                "string name=KB1 key=KB* number=1 points=3 vertices=4 closed=yes layer=KERB \
                 entity=9",
                "string name=KB2 key=KB* number=2 points=3 vertices=3 closed=yes layer=KERB \
                 entity=10",
                "string name=KB3 key=KB* number=3 points=2 vertices=2 closed=no layer=KERB \
                 entity=11",
                "string name=KB3 key=KB* number=3 points=2 vertices=2 closed=no layer=KERB \
                 join=yes entity=12"
            ]
        );

        assert_eq!(polylines_of(&document).len(), 4);
        let rectangle = shape_of(&entity_of(&document, 9));
        assert_eq!(
            rectangle.vertices,
            [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0), p(0.0, 5.0)]
        );
        assert!(rectangle.closed);
        let triangle = shape_of(&entity_of(&document, 10));
        assert_eq!(
            triangle.vertices,
            [p(0.0, 20.0), p(10.0, 20.0), p(10.0, 30.0)]
        );
        assert!(triangle.closed);
        let run_of_two = shape_of(&entity_of(&document, 11));
        assert_eq!(run_of_two.vertices, [p(0.0, 40.0), p(10.0, 40.0)]);
        assert!(!run_of_two.closed);
        // The join runs from the point that asks for it to the point it names.
        let join = shape_of(&entity_of(&document, 12));
        assert_eq!(join.vertices, [p(10.0, 40.0), p(0.0, 0.0)]);
        assert!(!join.closed);
    }

    // Each reason a point is in no line is a record, in the order the reasons are
    // declared (linework.rs, UnplacedReason), whatever order the points come in.
    #[test]
    fn each_reason_a_point_is_in_no_line_is_a_record_in_the_order_the_reasons_are_declared() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        //   6 and 7, KB3, stand in one place        every point of its string is in one place
        //   5, KB2, is alone                        the only point of its string
        //   a KB1 shot with no number               no point number to order it by
        //   3, MK                                   its code is a point code
        //   2, ZZ, which no rule knows              no rule for its code and no control code
        //   1, with no code                         no code
        add_points(
            &mut document,
            &[
                shot("6", 30.0, 0.0, "KB3"),
                shot("7", 30.0, 0.0, "KB3"),
                shot("5", 20.0, 0.0, "KB2"),
                shot("", 9.0, 9.0, "KB1"),
                shot("3", 5.0, 9.0, "MK"),
                shot("2", 5.0, 0.0, "ZZ"),
                shot("1", 0.0, 0.0, ""),
            ],
        );

        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=7 considered=7 lines=0 unplaced=7 notes=0",
                "unplaced reason=\"no code\" points=1",
                "unplaced reason=\"no rule for its code and no control code\" points=1",
                "unplaced reason=\"its code is a point code\" points=1",
                "unplaced reason=\"no point number to order it by\" points=1",
                "unplaced reason=\"the only point of its string\" points=1",
                "unplaced reason=\"every point of its string is in one place\" points=2"
            ]
        );
        assert!(polylines_of(&document).is_empty());
        assert_eq!(
            document.history().undo_count(),
            1,
            "the points; nothing was drawn"
        );
    }

    // ---- points their survey job has strung ----------------------------------------------

    #[test]
    fn points_their_survey_job_has_strung_are_left_out_and_counted() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        import_two_joints(&mut document, true, true);
        assert_eq!(polylines_of(&document).len(), 2);
        // Two shots of another string, drawn by hand and no job's: 10 (0,20) and
        // 11 (10,20), entities 8 and 9.
        add_points(
            &mut document,
            &[
                shot("10", 0.0, 20.0, "KJ77"),
                shot("11", 10.0, 20.0, "KJ77"),
            ],
        );

        // The drawing is the job's four points and two lines and the two loose
        // points: 8 entities. The four are left out; of the other four, two are
        // points, and they are one line, entity 10.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=8 considered=2 lines=1 unplaced=0 notes=0",
                "left_out=4 reason=strung-by-their-job",
                "string name=KJ77 key=KJ* number=77 points=2 vertices=2 closed=no \
                 layer=JOINTS entity=10"
            ]
        );
        // The job's lines were not drawn a second time: its two, and the new one.
        assert_eq!(polylines_of(&document).len(), 3);
        assert_eq!(
            vertices_of(&entity_of(&document, 10)),
            [p(0.0, 20.0), p(10.0, 20.0)]
        );
        // And the job still owns exactly what it did.
        assert_eq!(document.survey_jobs().len(), 1);
        assert_eq!(
            document.survey_jobs()[0].created_entities,
            [2, 3, 4, 5, 6, 7]
        );
    }

    #[test]
    fn a_job_imported_with_linework_off_is_strung_by_the_verb() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        import_two_joints(&mut document, true, false); // coded, not strung
        assert!(polylines_of(&document).is_empty());

        // Its points carry their string numbers beside the code, so KJ in string
        // 01 and KJ in string 02 are two lines: entities 6 and 7.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=4 considered=4 lines=2 unplaced=0 notes=0",
                "string name=KJ01 key=KJ* number=01 points=2 vertices=2 closed=no \
                 layer=JOINTS entity=6",
                "string name=KJ02 key=KJ* number=02 points=2 vertices=2 closed=no \
                 layer=JOINTS entity=7"
            ]
        );
        assert_eq!(
            vertices_of(&entity_of(&document, 6)),
            [p(0.0, 0.0), p(10.0, 0.0)]
        );
        assert_eq!(
            vertices_of(&entity_of(&document, 7)),
            [p(0.0, 5.0), p(10.0, 5.0)]
        );
    }

    #[test]
    fn a_point_whose_string_the_job_no_longer_has_a_line_of_is_strung_again() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        import_two_joints(&mut document, true, true);
        // The line of KJ 01, entity 6, is deleted by hand.
        must(document.execute(delete_entities(vec![6])));

        // Points 3 and 4 are still on the job's line KJ 02 and are left out;
        // points 1 and 2 have no line, and are strung: entity 8.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=5 considered=2 lines=1 unplaced=0 notes=0",
                "left_out=2 reason=strung-by-their-job",
                "string name=KJ01 key=KJ* number=01 points=2 vertices=2 closed=no \
                 layer=JOINTS entity=8"
            ]
        );
        assert_eq!(
            vertices_of(&entity_of(&document, 8)),
            [p(0.0, 0.0), p(10.0, 0.0)]
        );
        assert_eq!(polylines_of(&document).len(), 2);
    }

    #[test]
    fn a_job_whose_options_cannot_be_read_refuses_the_run_that_touches_it_and_is_named() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        import_two_joints(&mut document, true, true);
        add_points(
            &mut document,
            &[
                shot("10", 0.0, 20.0, "KJ77"),
                shot("11", 10.0, 20.0, "KJ77"),
            ],
        );
        // The job's option text, damaged: whether it strung its points is not known.
        SurveyJobAccess::jobs(&mut document)[0].import_options =
            String::from("not the options text");
        let steps = document.history().undo_count();

        let mut interpreter = CommandInterpreter::new();
        let refused = interpreter
            .run(&mut document, "LINEWORK")
            .expect_err("a job whose options do not read");
        assert_eq!(refused.code, ErrorCode::ParseFailure);
        assert!(
            refused
                .context
                .contains("LINEWORK cannot tell whether survey job job-1 strung its own points"),
            "{}",
            refused.describe()
        );
        assert_eq!(polylines_of(&document).len(), 2);
        assert_eq!(document.history().undo_count(), steps);

        // A scope that takes nothing of that job does not read it: the loose
        // points are on layer 0, the job's on JOINTS.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK LAYERS 0 PREVIEW"))[0],
            "linework scope=layers layers=0 sublayers=yes matched=2 considered=2 lines=1 \
             unplaced=0 notes=0 preview=yes"
        );
    }

    // The other way a job's options cannot be read: a later Katana wrote them.
    // Its version line is all this one reads of them (survey_job.rs).
    #[test]
    fn a_job_drawn_by_a_newer_katana_refuses_the_run_as_unsupported_and_is_named() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        import_two_joints(&mut document, true, true);
        SurveyJobAccess::jobs(&mut document)[0].import_options =
            String::from("katana-survey-import-options=2\ndraw-linework=true\n");
        let steps = document.history().undo_count();

        let mut interpreter = CommandInterpreter::new();
        let refused = interpreter
            .run(&mut document, "LINEWORK")
            .expect_err("a job a newer Katana drew");
        assert_eq!(refused.code, ErrorCode::Unsupported);
        assert!(
            refused.message.contains(
                "survey job job-1 was drawn by a newer Katana (drawing options version 2)"
            ),
            "{}",
            refused.describe()
        );
        assert!(
            refused
                .context
                .contains("LINEWORK cannot tell whether survey job job-1 strung its own points"),
            "{}",
            refused.describe()
        );
        assert_eq!(polylines_of(&document).len(), 2);
        assert_eq!(document.history().undo_count(), steps);
    }

    // left_out= counts what the scope took and the job strung - not the rest of
    // that job's points, which nobody asked about.
    #[test]
    fn only_the_points_the_scope_took_are_counted_as_left_to_their_job() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        import_two_joints(&mut document, true, true);

        // Entities 2 and 3 are points 1 and 2, the string KJ 01; the job's other
        // two points are not selected.
        document.selection_mut().set(vec![2, 3]);
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=selection matched=2 considered=0 lines=0 unplaced=0 notes=0",
                "left_out=2 reason=strung-by-their-job"
            ]
        );
        // One point of each string - entity 3 of KJ 01 and entity 4 of KJ 02 -
        // and the line of KJ 01, entity 6, which is no point.
        document.selection_mut().set(vec![3, 4, 6]);
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=selection matched=3 considered=0 lines=0 unplaced=0 notes=0",
                "left_out=2 reason=strung-by-their-job"
            ]
        );
        assert_eq!(polylines_of(&document).len(), 2);
    }

    // ---- a line the drawing already holds --------------------------------------------------

    #[test]
    fn a_second_run_draws_no_line_the_drawing_already_holds() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        two_kerbs_and_a_mark(&mut document);
        // The first run draws KB1 and KB2, entities 7 and 8.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK"))[0],
            "linework scope=drawing matched=6 considered=6 lines=2 unplaced=1 notes=0"
        );
        assert_eq!(document.history().undo_count(), 2);
        let revision = document.model_revision();

        // The second takes the 6 points and the 2 lines, and finds both lines
        // drawn: they run through 3 + 2 points. The mark is still a point code.
        let again = [
            "linework scope=drawing matched=8 considered=6 lines=0 unplaced=1 notes=0",
            "left_out=5 reason=already-drawn lines=2",
            "unplaced reason=\"its code is a point code\" points=1",
        ];
        assert_eq!(lines_of(&run(&mut document, "LINEWORK")), again);
        assert_eq!(document.model().entities.size(), 8);
        assert_eq!(polylines_of(&document).len(), 2);
        assert_eq!(
            document.history().undo_count(),
            2,
            "a run that draws nothing is no step"
        );
        assert_eq!(document.model_revision(), revision);

        // A preview answers alike.
        let mut preview: Vec<String> = again.iter().map(|line| String::from(*line)).collect();
        preview[0].push_str(" preview=yes");
        assert_eq!(lines_of(&run(&mut document, "LINEWORK PREVIEW")), preview);
    }

    #[test]
    fn a_string_that_has_gained_a_point_is_another_line_and_is_drawn_while_the_others_are_not() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        two_kerbs_and_a_mark(&mut document);
        run(&mut document, "LINEWORK"); // KB1 and KB2: entities 7 and 8
        // A fourth shot of KB1, point 7 at (30,0): entity 9.
        add_points(&mut document, &[shot("7", 30.0, 0.0, "KB1")]);

        // KB1 through four points is not the line of three that entity 7 is, and
        // is drawn: entity 10. KB2 is as entity 8 has it, and is not.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=9 considered=7 lines=1 unplaced=1 notes=0",
                "left_out=2 reason=already-drawn lines=1",
                "string name=KB1 key=KB* number=1 points=4 vertices=4 closed=no layer=KERB \
                 entity=10",
                "unplaced reason=\"its code is a point code\" points=1"
            ]
        );
        assert_eq!(
            vertices_of(&entity_of(&document, 10)),
            [p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0), p(30.0, 0.0)]
        );
        // The line that was there is nobody's to remove but the person's.
        assert_eq!(
            vertices_of(&entity_of(&document, 7)),
            [p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)]
        );
        assert_eq!(polylines_of(&document).len(), 3);
    }

    // What "the drawing already holds it" means: the code, the string number, the
    // vertices and whether they close, and the heights - and nothing else. Two
    // levelled shots of KB1, point 1 at (0,0) 5 m high and point 2 at (10,0) 6 m
    // high, beside one polyline somebody left in the drawing.
    #[test]
    fn only_a_line_of_the_same_code_string_number_vertices_and_heights_is_the_line_already_drawn() {
        struct Given {
            what: &'static str,
            vertices: Vec<Point2>,
            closed: bool,
            code: &'static str,
            string: &'static str,
            heights: Vec<Option<f64>>,
            layer: &'static str,
            the_line: bool,
        }
        let through = vec![p(0.0, 0.0), p(10.0, 0.0)];
        let surveyed = vec![Some(5.0), Some(6.0)];
        let cases = [
            Given {
                what: "the very line",
                vertices: through.clone(),
                closed: false,
                code: "KB1",
                string: "",
                heights: surveyed.clone(),
                layer: "0",
                the_line: true,
            },
            Given {
                what: "the very line, since moved to another layer",
                vertices: through.clone(),
                closed: false,
                code: "KB1",
                string: "",
                heights: surveyed.clone(),
                layer: "elsewhere",
                the_line: true,
            },
            Given {
                what: "another code",
                vertices: through.clone(),
                closed: false,
                code: "KB2",
                string: "",
                heights: surveyed.clone(),
                layer: "0",
                the_line: false,
            },
            Given {
                what: "a string number beside the code",
                vertices: through.clone(),
                closed: false,
                code: "KB1",
                string: "1",
                heights: surveyed.clone(),
                layer: "0",
                the_line: false,
            },
            Given {
                what: "another vertex",
                vertices: vec![p(0.0, 0.0), p(10.0, 1.0)],
                closed: false,
                code: "KB1",
                string: "",
                heights: surveyed.clone(),
                layer: "0",
                the_line: false,
            },
            Given {
                what: "a vertex more",
                vertices: vec![p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)],
                closed: false,
                code: "KB1",
                string: "",
                heights: vec![Some(5.0), Some(6.0), Some(7.0)],
                layer: "0",
                the_line: false,
            },
            Given {
                what: "closed, where the string is open",
                vertices: through.clone(),
                closed: true,
                code: "KB1",
                string: "",
                heights: surveyed.clone(),
                layer: "0",
                the_line: false,
            },
            Given {
                what: "another height",
                vertices: through.clone(),
                closed: false,
                code: "KB1",
                string: "",
                heights: vec![Some(5.0), Some(7.0)],
                layer: "0",
                the_line: false,
            },
            Given {
                what: "no heights",
                vertices: through.clone(),
                closed: false,
                code: "KB1",
                string: "",
                heights: Vec::new(),
                layer: "0",
                the_line: false,
            },
        ];
        for given in &cases {
            let what = given.what;
            let mut document = Document::default();
            document.set_survey_map(kerb_map());
            run(&mut document, "LAYER NEW elsewhere");
            // The shots are entities 1 and 2, the line that was there entity 3.
            add_entities(
                &mut document,
                vec![
                    levelled_point("1", 0.0, 0.0, "KB1", 5.0),
                    levelled_point("2", 10.0, 0.0, "KB1", 6.0),
                ],
            );
            add_entities(
                &mut document,
                vec![coded_line(
                    given.vertices.clone(),
                    given.closed,
                    given.code,
                    given.string,
                    &given.heights,
                    given.layer,
                )],
            );

            let reply = lines_of(&run(&mut document, "LINEWORK"));
            if given.the_line {
                assert_eq!(
                    reply,
                    [
                        "linework scope=drawing matched=3 considered=2 lines=0 unplaced=0 notes=0",
                        "left_out=2 reason=already-drawn lines=1"
                    ],
                    "{what}"
                );
                assert_eq!(document.model().entities.size(), 3, "{what}");
                // Nothing is created for a line that is not drawn.
                assert!(!document.model().layers.contains("KERB"), "{what}");
            } else {
                assert_eq!(
                    reply,
                    [
                        "linework scope=drawing matched=3 considered=2 lines=1 unplaced=0 notes=0",
                        "string name=KB1 key=KB* number=1 points=2 vertices=2 closed=no \
                         layer=KERB entity=4"
                    ],
                    "{what}"
                );
                let line = entity_of(&document, 4);
                assert_eq!(vertices_of(&line), through, "{what}");
                assert_eq!(heights_of(&line.properties, 2), surveyed, "{what}");
            }
        }
    }

    // What the reviewer of this verb found: a job's line erased by hand, its
    // points strung again by the verb - and then again by every run after.
    #[test]
    fn a_line_the_verb_drew_where_a_jobs_was_erased_is_not_drawn_a_second_time() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        import_two_joints(&mut document, true, true);
        must(document.execute(delete_entities(vec![6]))); // the line of KJ 01
        run(&mut document, "LINEWORK"); // strings points 1 and 2 again: entity 8
        assert_eq!(polylines_of(&document).len(), 2);

        // The four points, the job's line 7 and the verb's line 8. Points 3 and 4
        // are their job's; points 1 and 2 are on a line no job's record names,
        // and the drawing holds it.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=6 considered=2 lines=0 unplaced=0 notes=0",
                "left_out=2 reason=strung-by-their-job",
                "left_out=2 reason=already-drawn lines=1"
            ]
        );
        assert_eq!(polylines_of(&document).len(), 2);
    }

    // ---- nothing to go on ------------------------------------------------------------------

    #[test]
    fn with_no_survey_codes_loaded_only_a_control_code_makes_a_line() {
        let mut document = Document::default(); // no survey map
        //   XX: 1 (0,0) "ST"  2 (10,0) "END";  YY: 3 (5,5)
        add_points(
            &mut document,
            &[
                shot("1", 0.0, 0.0, "XX ST"),
                shot("2", 10.0, 0.0, "XX END"),
                shot("3", 5.0, 5.0, "YY"),
            ],
        );

        // Asked for by name, a control code still draws: XX, on its points'
        // layer, with nothing to style it - which the note says. YY has neither
        // a rule nor a control code.
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=3 considered=3 lines=1 unplaced=1 notes=1",
                "string name=XX key=\"\" number=\"\" points=2 vertices=2 closed=no layer=0 \
                 entity=4",
                "unplaced reason=\"no rule for its code and no control code\" points=1",
                "note kind=\"no rule for its code: nothing styles the line\" count=1"
            ]
        );
        let line = entity_of(&document, 4);
        assert_eq!(vertices_of(&line), [p(0.0, 0.0), p(10.0, 0.0)]);
        assert_eq!(line.style, "");
    }

    // What HELP LINEWORK says of a code no rule makes a line: a control code
    // joins the points that carry one, and a shot of the same code between them
    // that carries none is no part of the line.
    //   KB: 1 (0,0) "ST"   2 (10,0)   3 (10,10) "CL"
    #[test]
    fn with_no_rule_for_a_code_only_its_points_that_carry_a_control_code_are_joined() {
        let shots = [
            shot("1", 0.0, 0.0, "KB ST"),
            shot("2", 10.0, 0.0, "KB"),
            shot("3", 10.0, 10.0, "KB CL"),
        ];

        // No rule: points 1 and 3 are the string, which is then two points and
        // cannot close; point 2 is in no line.
        let mut document = Document::default();
        add_points(&mut document, &shots);
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=3 considered=3 lines=1 unplaced=1 notes=2",
                "string name=KB key=\"\" number=\"\" points=2 vertices=2 closed=no layer=0 \
                 entity=4",
                "unplaced reason=\"no rule for its code and no control code\" points=1",
                "note kind=\"closing a string of two points, drawn open\" count=1",
                "note kind=\"no rule for its code: nothing styles the line\" count=1"
            ]
        );
        assert_eq!(
            vertices_of(&entity_of(&document, 4)),
            [p(0.0, 0.0), p(10.0, 10.0)]
        );

        // With a rule that makes KB a line all three are its points, and it
        // closes on the third.
        let mut ruled = Document::default();
        ruled.set_survey_map(kerb_map());
        add_points(&mut ruled, &shots);
        assert_eq!(
            lines_of(&run(&mut ruled, "LINEWORK")),
            [
                "linework scope=drawing matched=3 considered=3 lines=1 unplaced=0 notes=0",
                "string name=KB key=KB* number=\"\" points=3 vertices=3 closed=yes layer=KERB \
                 entity=4"
            ]
        );
        assert_eq!(
            vertices_of(&entity_of(&ruled, 4)),
            [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)]
        );
    }

    #[test]
    fn points_with_no_code_and_a_drawing_with_no_points_are_answered_with_zeros_and_no_undo_step() {
        let mut empty = Document::default();
        assert_eq!(
            run(&mut empty, "LINEWORK"),
            "linework scope=drawing matched=0 considered=0 lines=0 unplaced=0 notes=0"
        );
        assert_eq!(empty.history().undo_count(), 0);

        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        add_points(
            &mut document,
            &[
                shot("1", 0.0, 0.0, ""),
                shot("2", 10.0, 0.0, ""),
                shot("3", 20.0, 0.0, ""),
            ],
        );
        assert_eq!(
            lines_of(&run(&mut document, "LINEWORK")),
            [
                "linework scope=drawing matched=3 considered=3 lines=0 unplaced=3 notes=0",
                "unplaced reason=\"no code\" points=3"
            ]
        );
        assert_eq!(
            document.history().undo_count(),
            1,
            "the points; a run that draws nothing is no step"
        );

        // A scope that takes entities none of which is a point: looked at, none
        // considered.
        let mut lines = Document::default();
        run(&mut lines, "LINE 0,0 10,0");
        assert_eq!(
            run(&mut lines, "LINEWORK"),
            "linework scope=drawing matched=1 considered=0 lines=0 unplaced=0 notes=0"
        );
    }

    #[test]
    fn a_reply_lists_fifty_lines_and_counts_the_rest() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        // 52 kerbs, KB1 to KB52, two shots each.
        let mut shots = Vec::new();
        for kerb in 1..=52 {
            let code = format!("KB{kerb}");
            shots.push(shot(
                &format!("{}", 2 * kerb - 1),
                0.0,
                f64::from(kerb),
                &code,
            ));
            shots.push(shot(&format!("{}", 2 * kerb), 10.0, f64::from(kerb), &code));
        }
        add_points(&mut document, &shots);

        let reply = lines_of(&run(&mut document, "LINEWORK PREVIEW"));
        assert_eq!(LINEWORK_STRINGS_LISTED, 50);
        assert_eq!(
            reply.len(),
            52,
            "the first record, 50 lines and the count of the rest"
        );
        assert_eq!(
            reply[0],
            "linework scope=drawing matched=104 considered=104 lines=52 unplaced=0 notes=0 \
             preview=yes"
        );
        for line in &reply[1..=50] {
            assert!(line.starts_with("string name=KB"), "{line}");
        }
        assert_eq!(reply.last().map(String::as_str), Some("strings_more=2"));
    }

    // ---- help ------------------------------------------------------------------------------

    #[test]
    fn the_help_names_the_verb_and_help_linework_is_its_own_text() {
        // The grammar as each says it, on two lines since PROPERTY and CHORD joined
        // it (the one line each held before, and these).
        let help = CommandInterpreter::help_text();
        assert!(help.contains(concat!(
            "LINEWORK [scope] [WHERE k=v ...] [PROPERTY name] [ORDER number|entity]\n",
            "          [CHORD length] [PREVIEW]"
        )));
        assert!(help.contains("HELP LINEWORK"));

        let mut document = Document::default();
        let own = run(&mut document, "HELP LINEWORK");
        assert_eq!(own, linework_verb_help());
        for words in [
            concat!(
                "LINEWORK [<scope>] [WHERE k=v ...] [PROPERTY <name>] [ORDER number|entity]\n",
                "         [CHORD <length>] [PREVIEW]"
            ),
            "PROPERTY  the property the codes are read from, as CODE's PROPERTY",
            "CHORD     how far, in the drawing's units, the straight segments drawn for a curve",
            "left_out=<n> reason=strung-by-their-job",
            "strings_more=<n>",
            "ONE undo step",
        ] {
            assert!(own.contains(words), "{words}");
        }
        // A reply, the help included, never holds the word a script's check for
        // a failed line looks for.
        assert!(!own.contains("error"));
        assert!(is_linework_verb("linework"));
        assert!(!is_linework_verb("LINE"));
    }

    // The help says what the verb does and no more: the record of a line already
    // drawn, that a control code alone joins only the points that carry one, and
    // that a job imported with linework off is strung by code and not as its
    // file strung it (each is a test above, or in tests/app/test_survey_verbs.cpp).
    #[test]
    fn the_help_says_what_is_left_out_and_what_stringing_by_code_does_not_do() {
        let help = linework_verb_help();
        for words in [
            "left_out=<n> reason=already-drawn lines=<m>",
            "a line the drawing already holds",
            "is strung only through the points that carry a",
            "strings its points BY THEIR CODES",
            "comes out as one open line",
            "[join=yes]",
        ] {
            assert!(help.contains(words), "{words}");
        }
        // And the summary in HELP names the second thing left out.
        assert!(CommandInterpreter::help_text().contains("and lines already drawn, are left out"));
    }
}

// ---- the plan, apart from the words --------------------------------------------------------

mod linework_plan {
    use super::*;

    // plan_linework is what the verb plans with, for a caller that shows the whole
    // plan: over the two kerbs and the mark it is KB1 and KB2 and the mark in no
    // line, with every point named - and nothing is changed.
    #[test]
    fn is_the_verbs_own_plan_in_full_and_changes_nothing() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        let ids = two_kerbs_and_a_mark(&mut document);
        let revision = document.model_revision();

        let plan = plan_linework(&document, &ids, &LineworkWords::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));

        assert!(plan.strung_by_their_job.is_empty());
        assert!(plan.planned.command.is_some());
        let report = &plan.planned.report;
        assert_eq!(report.considered, 6);
        assert_eq!(report.strings.len(), 2);
        assert_eq!(report.strings[0].name, "KB1");
        assert_eq!(report.strings[0].point_numbers, ["1", "2", "3"]);
        assert_eq!(report.strings[1].name, "KB2");
        assert_eq!(report.strings[1].point_numbers, ["4", "5"]);
        assert_eq!(report.unplaced.len(), 1);
        assert_eq!(report.unplaced[0].point_number, "6");
        assert_eq!(report.unplaced[0].reason, UnplacedReason::PointCode);
        assert_eq!(document.model().entities.size(), 6);
        assert_eq!(document.history().undo_count(), 1);
        assert_eq!(document.model_revision(), revision);
    }

    // An empty list is a scope that took nothing. It is never handed on: to
    // process_linework an empty list is every point in the drawing.
    #[test]
    fn an_empty_list_plans_nothing_rather_than_every_point() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        two_kerbs_and_a_mark(&mut document);

        let plan = plan_linework(&document, &[], &LineworkWords::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));

        assert!(plan.planned.command.is_none());
        assert_eq!(plan.planned.report.considered, 0);
        assert!(plan.planned.report.strings.is_empty());
        assert!(plan.planned.report.unplaced.is_empty());
    }

    // The two things the verb leaves out are left out of the plan too: the points
    // their job has strung (the job's four, entities 2 to 5) are named and not
    // planned, and a line the drawing already holds is listed as already drawn
    // and makes no command.
    #[test]
    fn leaves_out_what_the_verb_leaves_out_and_says_which() {
        let mut document = Document::default();
        document.set_survey_map(joint_map());
        import_two_joints(&mut document, true, true);
        // Two loose shots of another string: entities 8 and 9.
        let loose = add_points(
            &mut document,
            &[
                shot("10", 0.0, 20.0, "KJ77"),
                shot("11", 10.0, 20.0, "KJ77"),
            ],
        );
        assert_eq!(loose, [8, 9]);
        let everything = every_entity(&document);

        let first = plan_linework(&document, &everything, &LineworkWords::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(first.strung_by_their_job, [2, 3, 4, 5]);
        assert_eq!(first.planned.report.considered, 2);
        assert_eq!(first.planned.report.strings.len(), 1);
        assert_eq!(first.planned.report.strings[0].name, "KJ77");
        assert!(first.planned.command.is_some());

        // Drawn by the verb; planned again, that line is the drawing's already.
        run(&mut document, "LINEWORK DRAWING");
        let everything = every_entity(&document);
        let second = plan_linework(&document, &everything, &LineworkWords::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(second.planned.command.is_none());
        assert!(second.planned.report.strings.is_empty());
        assert_eq!(second.planned.report.already_drawn.len(), 1);
        assert_eq!(second.planned.report.already_drawn[0].name, "KJ77");
    }

    // The words reach the plan as they reach the verb's: the property, the order
    // and the chord length (the half circle of the CHORD test above, 7 vertices
    // at 0.35), and a chord length that is no length is process_linework's refusal.
    #[test]
    fn takes_the_property_the_order_and_the_chord_length_a_line_would_say() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        // Drawn 3, 1, 2: by number the string runs 1, 2, 3; as drawn, 3, 1, 2.
        let ids = add_points(
            &mut document,
            &[
                shot("3", -10.0, 0.0, "KB1 EC"),
                shot("1", 10.0, 0.0, "KB1 BC"),
                shot("2", 0.0, 10.0, "KB1"),
            ],
        );

        let mut words = LineworkWords {
            chord: Some(0.35),
            ..LineworkWords::default()
        };
        let by_number = plan_linework(&document, &ids, &words)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(by_number.planned.report.strings.len(), 1);
        assert_eq!(
            by_number.planned.report.strings[0].point_numbers,
            ["1", "2", "3"]
        );
        assert_eq!(by_number.planned.report.strings[0].vertices, 7);

        words.order = LineworkOrder::EntityOrder;
        let as_drawn = plan_linework(&document, &ids, &words)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(as_drawn.planned.report.strings.len(), 1);
        assert_eq!(
            as_drawn.planned.report.strings[0].point_numbers,
            ["3", "1", "2"]
        );

        words.property = String::from("fcode"); // which none of the three carries
        let elsewhere = plan_linework(&document, &ids, &words)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(elsewhere.planned.report.strings.is_empty());
        assert_eq!(elsewhere.planned.report.unplaced.len(), 3);

        words.chord = Some(0.0);
        let refused = plan_linework(&document, &ids, &words).expect_err("a chord of 0");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
    }
}

mod linework_already_drawn {
    use super::*;

    // The rule is the verb's to ask for: process_linework itself draws again, as
    // it always did and as an import's own run must (two imports of one file
    // each draw, and own, their lines).
    #[test]
    fn only_a_run_told_to_leaves_out_the_lines_the_drawing_holds() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        two_kerbs_and_a_mark(&mut document);
        let mut options = LineworkOptions::default(); // every point of the drawing, the defaults
        let mut first = process_linework(&document, &options)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let command = first
            .command
            .take()
            .expect("the first run has lines to draw");
        must(document.execute(command));
        assert_eq!(polylines_of(&document).len(), 2); // KB1 and KB2: entities 7 and 8

        let again = process_linework(&document, &options)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(again.command.is_some());
        assert_eq!(again.report.strings.len(), 2);
        assert!(again.report.already_drawn.is_empty());

        options.skip_lines_already_drawn = true;
        let told = process_linework(&document, &options)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(told.command.is_none(), "nothing to build");
        assert!(told.report.strings.is_empty());
        assert_eq!(told.report.already_drawn.len(), 2);
        assert_eq!(told.report.already_drawn[0].name, "KB1");
        assert_eq!(told.report.already_drawn[0].points, [1, 2, 3]);
        assert_eq!(told.report.already_drawn[0].vertices, 3);
        assert_eq!(told.report.already_drawn[0].layer, "KERB");
        assert_eq!(told.report.already_drawn[1].name, "KB2");
        assert_eq!(told.report.already_drawn[1].points, [4, 5]);
        // Their points are in a line: only the mark is in none.
        assert_eq!(told.report.unplaced.len(), 1);
        assert_eq!(told.report.unplaced[0].id, 6);

        // And no point is removed on account of a line this run did not draw.
        options.keep_points = false;
        let replacing = process_linework(&document, &options)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(replacing.command.is_none());
        assert_eq!(replacing.report.points_removed, 0);
    }
}

// ---- what a survey import does beyond its points -------------------------------------------

mod survey_import_finish_choice {
    use super::*;

    #[test]
    fn with_no_survey_codes_loaded_nothing_is_handed_on_whatever_was_asked() {
        let document = Document::default(); // the default: both switches on, no survey codes
        let unsaid = survey_import_finish(&document, None, None);
        assert!(unsaid.codes_asked);
        assert!(unsaid.linework_asked);
        assert!(!unsaid.survey_codes_loaded);
        assert!(!unsaid.options.codes);
        assert!(!unsaid.options.linework);
        assert!(!unsaid.options.any(), "the import is the plain one");

        let said = survey_import_finish(&document, Some(true), Some(true));
        assert!(said.codes_asked);
        assert!(!said.options.any());
    }

    #[test]
    fn the_line_says_whether_and_where_it_says_nothing_the_documents_switch_does() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());

        let unsaid = survey_import_finish(&document, None, None);
        assert!(unsaid.survey_codes_loaded);
        assert!(unsaid.options.codes);
        assert!(unsaid.options.linework);

        // Said on the line, each on its own.
        let no_codes = survey_import_finish(&document, Some(false), None);
        assert!(!no_codes.codes_asked);
        assert!(!no_codes.options.codes);
        assert!(no_codes.options.linework);
        let no_lines = survey_import_finish(&document, None, Some(false));
        assert!(no_lines.options.codes);
        assert!(!no_lines.linework_asked);
        assert!(!no_lines.options.linework);

        // The customisation's switches are what an unsaid word takes ...
        document.set_automation(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: false,
        });
        let off = survey_import_finish(&document, None, None);
        assert!(!off.codes_asked);
        assert!(!off.linework_asked);
        assert!(!off.options.any());
        // ... and a said one overrides.
        let on = survey_import_finish(&document, Some(true), None);
        assert!(on.options.codes);
        assert!(!on.options.linework);
    }

    // Each word against ITS OWN switch, both ways: LINEWORK on over a linework
    // switch that is off, and each word off over a switch that is on.
    #[test]
    fn a_word_said_overrides_its_own_switch_whichever_way_either_stands() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        document.set_automation(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: false,
        });

        let lines = survey_import_finish(&document, None, Some(true));
        assert!(!lines.codes_asked);
        assert!(lines.linework_asked);
        assert!(!lines.options.codes);
        assert!(lines.options.linework);
        let both = survey_import_finish(&document, Some(true), Some(true));
        assert!(both.options.codes);
        assert!(both.options.linework);

        document.set_automation(CustomisationAutomation {
            codes_on_survey_import: true,
            linework_on_survey_import: true,
        });
        let no_lines = survey_import_finish(&document, None, Some(false));
        assert!(no_lines.codes_asked);
        assert!(!no_lines.linework_asked);
        assert!(no_lines.options.codes);
        assert!(!no_lines.options.linework);
        let neither = survey_import_finish(&document, Some(false), Some(false));
        assert!(!neither.codes_asked);
        assert!(!neither.linework_asked);
        assert!(!neither.options.any());
    }

    #[test]
    fn the_colours_the_control_codes_and_the_order_are_the_documents() {
        let mut document = Document::default();
        document.set_survey_map(kerb_map());
        let mut colours = ColourTable::default();
        assert!(colours.add("kerb red", Color::new(170, 0, 0, 255)).is_ok());
        document.set_colour_table(colours);
        let codes = LineworkCodes {
            start: String::from("S"),
            close: String::from("SHUT"),
            ..LineworkCodes::default()
        };
        must(document.set_linework_codes(codes.clone()));

        let finish = survey_import_finish(&document, None, None);
        assert_eq!(finish.options.controls, codes);
        assert_eq!(finish.options.order, LineworkOrder::PointNumber);
        let colour_of = finish
            .options
            .coding
            .colour_of
            .as_ref()
            .expect("the finish has a colour lookup");
        // The customisation's own name, then a standard one, then no name at all.
        assert_eq!(colour_of("kerb red"), Some(Color::new(170, 0, 0, 255)));
        assert_eq!(colour_of("blue"), Some(Color::new(0, 0, 255, 255)));
        assert!(colour_of("no such colour").is_none());
    }
}
