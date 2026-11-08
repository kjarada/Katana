// The survey finish (tests/cad/customisation/test_survey_finish.cpp): an
// import that also codes what it drew and strings it, as ONE undo step - and
// the string names it is built on (a code followed by its string number).
//
// Every expected value here is worked by hand from the small project the test
// builds; the working is in the comment beside it. A survey point is
// (northing, easting) and a drawing point (x, y) = (easting, northing).

use std::sync::Arc;

use katana_cad::linework::{draw_survey_features, process_linework};
use katana_cad::survey_coding::{apply_survey_codes, survey_lookup_name};
use katana_cad::survey_finish::{
    describe, finish_warnings, name_survey_strings, with_survey_finish,
};
use katana_cad::survey_points::import_survey_points;
use katana_cad::{
    ColourOf, Document, ExistingPointPolicy, LineworkOptions, LineworkOrder, SharedFinishReport,
    SurveyCodingOptions, SurveyCodingReport, SurveyFeatureOptions, SurveyFinishEarlier,
    SurveyFinishOptions, SurveyFinishReport, SurveyFinishSkip, SurveyImportOptions,
    UnplacedFeatureReason, UnplacedReason,
};
use katana_commands::{
    Command, create_entities, create_layer, delete_entities, move_entities, set_entity_layer,
};
use katana_core::ErrorCode;
use katana_entity::{
    Color, Entity, EntityId, EnumName, Geometry, Layer, PointGeometry, PropertyValue,
    SurveyAttribute, SurveyBreakline, SurveyMap, SurveyRule, SurveySection, SurveySymbol,
};
use katana_geometry::{Point2, Vec2};
use katana_survey::data_model::{SurveyFeature, SurveyPoint, SurveyProject};

use crate::{
    drawing_of, expect_ok, lines_of, must, numbered, p, point_count, refused, text_property,
    vertices_of,
};

#[track_caller]
fn add(map: &mut SurveyMap, rule: SurveyRule) {
    if let Err(error) = map.add(rule) {
        panic!("{}", error.describe());
    }
}

fn line_rule(key: &str, model: &str, linestyle: &str) -> SurveyRule {
    SurveyRule {
        key: String::from(key),
        model: String::from(model),
        linestyle: String::from(linestyle),
        breakline: Some(SurveyBreakline::Line),
        ..SurveyRule::default()
    }
}

// Every code gets the attribute Surveyed = yes: the kind of bare "*" rule a
// code file carries in its attribute sections. It says nothing about lines.
fn attribute_for_every_code() -> SurveyRule {
    SurveyRule {
        key: String::from("*"),
        section: SurveySection::StringAttribute,
        attributes: vec![SurveyAttribute {
            r#type: String::from("text"),
            name: String::from("Surveyed"),
            value: String::from("yes"),
        }],
        ..SurveyRule::default()
    }
}

// Shaped like a real code table, with invented names:
//   KJ*  a joint: a LINE on SURVEY JOINT, drawn with "Joint Line";
//   PS*  a survey mark: a POINT on SURVEY MARKS with the symbol "Peg", written
//        as a code file writes it - one rule for where it goes, one for its
//        symbol;
//   B1*  strings 10 to 19 of the one-letter code B: a LINE on SURVEY BARRIER;
//   *    the attribute rule above.
fn field_map() -> SurveyMap {
    let mut map = SurveyMap::default();
    add(&mut map, line_rule("KJ*", "SURVEY JOINT", "Joint Line"));
    add(
        &mut map,
        SurveyRule {
            key: String::from("PS*"),
            model: String::from("SURVEY MARKS"),
            breakline: Some(SurveyBreakline::Point),
            ..SurveyRule::default()
        },
    );
    add(
        &mut map,
        SurveyRule {
            key: String::from("PS*"),
            section: SurveySection::VertexSymbol,
            symbol: Some(SurveySymbol {
                style: String::from("Peg"),
                size: 0.5,
                ..SurveySymbol::default()
            }),
            ..SurveyRule::default()
        },
    );
    add(&mut map, line_rule("B1*", "SURVEY BARRIER", "Barrier"));
    add(&mut map, attribute_for_every_code());
    map
}

fn point(id: &str, northing: f64, easting: f64, code: &str) -> SurveyPoint {
    SurveyPoint {
        id: String::from(id),
        northing,
        easting,
        code: String::from(code),
        ..SurveyPoint::default()
    }
}

// A feature as the field-file readers' shared builder leaves one: the code,
// and the STRING NUMBER as its name.
fn feature(code: &str, number: &str, ids: &[&str]) -> SurveyFeature {
    SurveyFeature {
        code: String::from(code),
        name: String::from(number),
        point_ids: ids.iter().map(|id| String::from(*id)).collect(),
        ..SurveyFeature::default()
    }
}

// A field file whose records keep code and string number apart: every point
// has the code alone, and a feature per (code, number) strings its points in
// the order they were shot.
fn field_file() -> SurveyProject {
    SurveyProject {
        points: vec![
            point("1", 0.0, 0.0, "KJ"),
            point("2", 0.0, 10.0, "KJ"), // KJ, string 01
            point("3", 5.0, 0.0, "KJ"),
            point("4", 5.0, 10.0, "KJ"), // KJ, string 02
            point("5", 20.0, 0.0, "PS"),
            point("6", 20.0, 10.0, "PS"), // two survey marks
            point("7", 30.0, 0.0, "B"),
            point("8", 30.0, 10.0, "B"), // B, string 12
            point("9", 40.0, 0.0, "ZZ"),
            point("10", 40.0, 10.0, "ZZ"), // a code with no rule
        ],
        features: vec![
            feature("KJ", "01", &["1", "2"]),
            feature("KJ", "02", &["3", "4"]),
            feature("PS", "01", &["5", "6"]),
            feature("B", "12", &["7", "8"]),
            feature("ZZ", "1", &["9", "10"]),
        ],
        ..SurveyProject::default()
    }
}

fn codes_and_linework() -> SurveyFinishOptions {
    SurveyFinishOptions {
        codes: true,
        linework: true,
        ..SurveyFinishOptions::default()
    }
}

fn defaults() -> SurveyImportOptions {
    SurveyImportOptions::default()
}

// An import onto a TOP-LEVEL layer, for the tests that compare the whole
// drawing after an undo. Creating the default "survey/points" also creates
// its parent "survey", and the undo of any import - with a finish or without
// - leaves that parent behind: the layer command's undo removes only the
// layer it was given. Those tests are about what the FINISH leaves.
fn onto_a_top_level_layer() -> SurveyImportOptions {
    SurveyImportOptions {
        layer: String::from("field"),
        ..SurveyImportOptions::default()
    }
}

// The import of `project` as a job makes it - its string numbers named onto
// the points when it is to be finished - wrapped in the finish and executed
// as one command. The report is what the finish filled.
#[track_caller]
fn import_and_finish(
    document: &mut Document,
    mut project: SurveyProject,
    finish: &SurveyFinishOptions,
    options: &SurveyImportOptions,
    policy: ExistingPointPolicy,
) -> SurveyFinishReport {
    if finish.any() {
        name_survey_strings(&mut project.points, &project.features);
    }
    // What a job draws: the points, and nothing that refers to them (a
    // feature naming a point the policy skips would make the project of the
    // points that are left an inconsistent one).
    let drawable = SurveyProject {
        points: project.points.clone(),
        ..SurveyProject::default()
    };
    let points = expect_ok(import_survey_points(
        document, &drawable, options, policy, None,
    ));
    let report: SharedFinishReport = Arc::default();
    let strings = SurveyProject {
        points: project.points,
        features: project.features,
        ..SurveyProject::default()
    };
    let command = with_survey_finish(
        document,
        points,
        strings,
        options,
        finish.clone(),
        Some(Arc::clone(&report)),
        SurveyFinishEarlier::default(),
    );
    let Some(command) = command else {
        panic!("the finish made no command");
    };
    must(document, command);
    let finished = report.lock().unwrap();
    finished.clone()
}

#[track_caller]
fn import_finish(
    document: &mut Document,
    project: SurveyProject,
    finish: &SurveyFinishOptions,
) -> SurveyFinishReport {
    import_and_finish(
        document,
        project,
        finish,
        &defaults(),
        ExistingPointPolicy::Refuse,
    )
}

// A point as the survey import writes one, for the tests of the two
// functions on their own.
fn add_point(
    document: &mut Document,
    at: Point2,
    code: &str,
    number: &str,
    string: &str,
) -> EntityId {
    let mut entity = Entity {
        geometry: Geometry::Point(PointGeometry { position: at }),
        ..Entity::default()
    };
    entity
        .properties
        .insert(String::from("code"), PropertyValue::from(code));
    entity
        .properties
        .insert(String::from("point"), PropertyValue::from(number));
    if !string.is_empty() {
        entity.properties.insert(
            String::from(katana_cad::survey_coding::SURVEY_STRING_PROPERTY),
            PropertyValue::from(string),
        );
    }
    must(document, create_entities(vec![entity]));
    document.last_created_entities()[0]
}

// The layer of an entity the test made.
#[track_caller]
fn layer_of(document: &Document, id: EntityId) -> String {
    match document.model().entities.find(id) {
        Some(entity) => entity.layer.clone(),
        None => panic!("entity {id} is gone"),
    }
}

// ---- string names ---------------------------------------------------------------------

mod survey_finish_string_names {
    use super::*;

    #[test]
    fn a_code_is_looked_up_with_its_string_number_and_falls_back_to_the_code_alone() {
        let mut map = field_map();
        add(
            &mut map,
            SurveyRule {
                key: String::from("ZPBB"), // exact: one string per code
                model: String::from("SURVEY PITS"),
                ..SurveyRule::default()
            },
        );

        // "B" + "12" is "B12", which "B1*" answers; "B" alone has no rule.
        assert_eq!(survey_lookup_name(&map, "B", "12"), "B12");
        // "KJ" + "01" and "KJ" + "02" are two names, both answered by "KJ*".
        assert_eq!(survey_lookup_name(&map, "KJ", "01"), "KJ01");
        assert_eq!(survey_lookup_name(&map, "KJ", "02"), "KJ02");
        // "ZPBB3" is not the exact key "ZPBB", and nothing but "*" answers it:
        // the code alone, which the exact key does answer.
        assert_eq!(survey_lookup_name(&map, "ZPBB", "3"), "ZPBB");
        // "B7": "B1*" does not answer it and nothing answers "B", so it is
        // reported as the code it is, once, not once per string.
        assert_eq!(survey_lookup_name(&map, "B", "7"), "B");
        // No string number: the code, as it always was. No code: no name.
        assert_eq!(survey_lookup_name(&map, "KJ01", ""), "KJ01");
        assert_eq!(survey_lookup_name(&map, "", "12"), "");
    }

    // "Numbering a string never loses a rule the code had" where another rule
    // DOES answer the numbered name: a rule for a whole family of codes
    // ("PA*") answers "ZPBB3", and answers ZPBB itself less well than ZPBB's
    // own exact rule does.
    #[test]
    fn a_numbered_string_keeps_its_codes_exact_rule_over_a_broader_rule_for_the_name() {
        let mut map = SurveyMap::default();
        add(
            &mut map,
            SurveyRule {
                key: String::from("ZPBB"), // exact: this code, one string
                model: String::from("SURVEY PITS"),
                ..SurveyRule::default()
            },
        );
        add(
            &mut map,
            SurveyRule {
                key: String::from("PA*"), // every code that begins PA
                model: String::from("SURVEY PAVEMENT"),
                ..SurveyRule::default()
            },
        );

        // The family rule is all that answers "ZPBB3", and it is broader than
        // the code: ZPBB's exact rule is the more specific, so the code is
        // looked up.
        assert_eq!(survey_lookup_name(&map, "ZPBB", "3"), "ZPBB");
        // A code of the family with no exact rule keeps its name, as before.
        assert_eq!(survey_lookup_name(&map, "PAVE", "3"), "PAVE3");

        // And so the point is coded by its own rule, on its own layer.
        let mut document = Document::default();
        document.set_survey_map(map.clone());
        let numbered_point = add_point(&mut document, p(0.0, 0.0), "ZPBB", "1", "3");
        let planned = expect_ok(apply_survey_codes(
            &document,
            &SurveyCodingOptions::default(),
            None,
        ));
        let Some(planned) = planned else {
            panic!("the point is to be coded");
        };
        must(&mut document, planned);
        assert_eq!(layer_of(&document, numbered_point), "SURVEY PITS");

        // A prefix key that is the code itself, or reaches into the number,
        // WAS written for the numbered names of that code, beside the exact
        // key for the unnumbered one: there the name is looked up.
        add(
            &mut map,
            SurveyRule {
                key: String::from("ZPBB*"),
                model: String::from("SURVEY PIT STRINGS"),
                ..SurveyRule::default()
            },
        );
        assert_eq!(survey_lookup_name(&map, "ZPBB", "3"), "ZPBB3");
        assert_eq!(survey_lookup_name(&map, "ZPBB", ""), "ZPBB");
    }

    #[test]
    fn a_point_is_coded_by_its_code_followed_by_its_string_number() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        let barrier = add_point(&mut document, p(0.0, 0.0), "B", "1", "12");
        let other = add_point(&mut document, p(5.0, 0.0), "B", "2", "7");
        let whole = add_point(&mut document, p(9.0, 0.0), "B12 ST", "3", "");

        let mut report = SurveyCodingReport::default();
        let planned = expect_ok(apply_survey_codes(
            &document,
            &SurveyCodingOptions::default(),
            Some(&mut report),
        ));
        let Some(planned) = planned else {
            panic!("the points are to be coded");
        };
        must(&mut document, planned);

        // B in string 12 and "B12 ST" written whole are both B12 -> "B1*".
        assert_eq!(layer_of(&document, barrier), "SURVEY BARRIER");
        assert_eq!(
            document
                .model()
                .entities
                .find(barrier)
                .map(|e| e.style.as_str()),
            Some("Barrier")
        );
        assert_eq!(layer_of(&document, whole), "SURVEY BARRIER");
        // B in string 7 is "B7": no rule, so it stays where it was.
        assert_eq!(layer_of(&document, other), "0");
        // Reported under the name looked up: B12 (two points), and B (one).
        assert_eq!(report.codes.len(), 2);
        assert_eq!(report.codes[0].code, "B");
        assert_eq!(report.codes[0].entities, 1);
        assert!(!report.codes[0].matched);
        assert_eq!(report.codes[1].code, "B12");
        assert_eq!(report.codes[1].entities, 2);
        assert!(report.codes[1].matched);
    }

    #[test]
    fn points_of_one_code_in_two_strings_are_strung_as_two_lines_that_carry_code_and_number() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        // KJ string 01: points 1 and 2, and point 5 written "KJ01" whole.
        // KJ string 02: points 3 and 4.
        add_point(&mut document, p(0.0, 0.0), "KJ", "1", "01");
        add_point(&mut document, p(10.0, 0.0), "KJ", "2", "01");
        add_point(&mut document, p(0.0, 5.0), "KJ", "3", "02");
        add_point(&mut document, p(10.0, 5.0), "KJ", "4", "02");
        add_point(&mut document, p(20.0, 0.0), "KJ01", "5", "");

        let mut planned = expect_ok(process_linework(&document, &LineworkOptions::default()));
        let Some(command) = planned.command.take() else {
            panic!("the lines are to be drawn");
        };
        must(&mut document, command);

        // By point number: string KJ01 is 1, 2, 5 and string KJ02 is 3, 4.
        let strings = &planned.report.strings;
        assert_eq!(strings.len(), 2);
        assert_eq!(strings[0].name, "KJ01");
        assert_eq!(strings[0].key, "KJ*");
        assert_eq!(strings[0].number, "01");
        assert_eq!(strings[0].point_numbers, ["1", "2", "5"]);
        assert_eq!(strings[1].name, "KJ02");
        assert_eq!(strings[1].number, "02");
        assert_eq!(strings[1].point_numbers, ["3", "4"]);

        let lines = lines_of(&document);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            vertices_of(lines[0]),
            [p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)]
        );
        assert_eq!(vertices_of(lines[1]), [p(0.0, 5.0), p(10.0, 5.0)]);
        // Each carries what its first point carries, so the rule its points
        // found - "KJ*", by the name KJ01 - is the rule that styles it.
        assert_eq!(text_property(lines[0], "code"), "KJ");
        assert_eq!(text_property(lines[0], "string"), "01");
        assert_eq!(text_property(lines[1], "string"), "02");
        assert_eq!(lines[0].layer, "SURVEY JOINT");
        assert_eq!(lines[0].style, "Joint Line");
        assert_eq!(lines[1].style, "Joint Line");
    }

    #[test]
    fn naming_strings_gives_a_point_the_number_of_the_first_named_feature_of_its_own_code() {
        let mut project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "KJ"),
                point("2", 0.0, 1.0, "KJ ST"),
                point("3", 0.0, 2.0, "PS"),
                point("4", 0.0, 3.0, "KJ"),
            ],
            features: vec![
                feature("KB", "32", &["1"]),      // another code: not point 1's string
                feature("KJ", "01", &["1", "2"]), // the first of its own code
                feature("KJ", "02", &["1", "4"]), // a second: 1 keeps 01, 4 keeps the file's 77
                feature("PS", "", &["3"]),        // no number: nothing to give
            ],
            ..SurveyProject::default()
        };
        // The file's own column.
        project.points[3]
            .metadata
            .insert(String::from("string"), String::from("77"));

        name_survey_strings(&mut project.points, &project.features);

        let string_of = |point: &SurveyPoint| point.metadata.get("string").cloned();
        assert_eq!(string_of(&project.points[0]).as_deref(), Some("01"));
        // "KJ ST" is coded KJ: the first token is the code.
        assert_eq!(string_of(&project.points[1]).as_deref(), Some("01"));
        assert!(!project.points[2].metadata.contains_key("string"));
        assert_eq!(string_of(&project.points[3]).as_deref(), Some("77"));
    }

    #[test]
    fn a_feature_naming_a_point_that_is_not_among_the_points_names_the_others() {
        // Point 9 is the drawing's control, or has no coordinates: not drawn,
        // so not in the list. The feature still names point 1.
        let mut points = vec![point("1", 0.0, 0.0, "KJ")];
        let features = vec![feature("KJ", "01", &["9", "1"])];

        name_survey_strings(&mut points, &features);

        assert_eq!(points.len(), 1);
        assert_eq!(
            points[0].metadata.get("string").map(String::as_str),
            Some("01")
        );
    }
}

// ---- features under the rules ----------------------------------------------------------

mod survey_features_ruled {
    use super::*;

    fn ruled() -> SurveyFeatureOptions {
        SurveyFeatureOptions {
            only_ruled_lines: true,
            ..SurveyFeatureOptions::default()
        }
    }

    #[test]
    fn only_a_feature_a_rule_makes_a_line_is_drawn_and_the_others_say_why() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        let project = field_file();

        let mut planned = expect_ok(draw_survey_features(&document, &project, &ruled()));
        let Some(command) = planned.command.take() else {
            panic!("the lines are to be drawn");
        };
        must(&mut document, command);

        // KJ 01, KJ 02 and B 12 are lines by "KJ*" and "B1*"; reported as code
        // and number, with the number the file gave.
        let strings = &planned.report.strings;
        assert_eq!(strings.len(), 3);
        assert_eq!(strings[0].name, "KJ 01");
        assert_eq!(strings[0].key, "KJ*");
        assert_eq!(strings[0].number, "01");
        assert_eq!(strings[1].name, "KJ 02");
        assert_eq!(strings[2].name, "B 12");
        assert_eq!(strings[2].key, "B1*");
        assert_eq!(strings[2].number, "12");
        assert!(planned.report.notes.is_empty());

        // PS 01 is a point code by "PS*"; ZZ 1 is known to nothing but "*".
        assert_eq!(planned.unplaced.len(), 2);
        assert_eq!(planned.unplaced[0].index, 2);
        assert_eq!(planned.unplaced[0].name, "PS 01");
        assert_eq!(planned.unplaced[0].code, "PS");
        assert_eq!(planned.unplaced[0].reason, UnplacedFeatureReason::PointCode);
        assert_eq!(planned.unplaced[1].index, 4);
        assert_eq!(planned.unplaced[1].name, "ZZ 1");
        assert_eq!(planned.unplaced[1].reason, UnplacedFeatureReason::NoRule);

        let lines = lines_of(&document);
        assert_eq!(lines.len(), 3);
        assert_eq!(text_property(lines[0], "code"), "KJ");
        assert_eq!(text_property(lines[0], "string"), "01");
        assert_eq!(text_property(lines[2], "code"), "B");
        assert_eq!(text_property(lines[2], "string"), "12");
        // Styled by the same two: B12 -> "B1*" -> Barrier on SURVEY BARRIER.
        assert_eq!(lines[2].layer, "SURVEY BARRIER");
        assert_eq!(lines[2].style, "Barrier");
    }

    #[test]
    fn without_the_option_every_feature_is_still_drawn_and_named_as_it_was() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        let project = field_file();

        let mut planned = expect_ok(draw_survey_features(
            &document,
            &project,
            &SurveyFeatureOptions::default(),
        ));
        let Some(command) = planned.command.take() else {
            panic!("the lines are to be drawn");
        };
        must(&mut document, command);

        // All five, marks and the unknown code included, named by the name
        // alone.
        assert_eq!(planned.report.strings.len(), 5);
        assert_eq!(planned.report.strings[0].name, "01");
        assert_eq!(planned.report.strings[3].name, "12");
        assert!(planned.unplaced.is_empty());
        let lines = lines_of(&document);
        assert_eq!(lines.len(), 5);
        assert_eq!(text_property(lines[0], "string"), "<absent>");
    }

    #[test]
    fn a_feature_with_no_name_is_drawn_by_its_code_and_carries_no_string_number() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        let project = SurveyProject {
            points: vec![point("1", 0.0, 0.0, "KJ07"), point("2", 0.0, 10.0, "KJ07")],
            features: vec![feature("KJ07", "", &["1", "2"])],
            ..SurveyProject::default()
        };

        let mut planned = expect_ok(draw_survey_features(&document, &project, &ruled()));
        let Some(command) = planned.command.take() else {
            panic!("the line is to be drawn");
        };
        must(&mut document, command);

        // Called by its code, which "KJ*" answers and splits: number 07.
        assert_eq!(planned.report.strings.len(), 1);
        assert_eq!(planned.report.strings[0].name, "KJ07");
        assert_eq!(planned.report.strings[0].key, "KJ*");
        assert_eq!(planned.report.strings[0].number, "07");
        let lines = lines_of(&document);
        assert_eq!(lines.len(), 1);
        assert_eq!(text_property(lines[0], "code"), "KJ07");
        assert_eq!(text_property(lines[0], "string"), "<absent>");
        assert_eq!(lines[0].layer, "SURVEY JOINT");
    }

    #[test]
    fn only_the_features_asked_about_are_drawn_or_reported() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        let project = field_file(); // KJ 01, KJ 02, PS 01, B 12, ZZ 1
        let mut options = ruled();

        // KJ 02 and ZZ 1 alone: one line, one string with no rule - and nothing
        // said of the three that were not asked about.
        options.consider = vec![false, true, false, false, true];
        let some = expect_ok(draw_survey_features(&document, &project, &options));
        assert_eq!(some.report.strings.len(), 1);
        assert_eq!(some.report.strings[0].name, "KJ 02");
        assert_eq!(some.unplaced.len(), 1);
        assert_eq!(
            some.unplaced[0].index, 4,
            "its place among ALL the project's features"
        );
        assert_eq!(some.unplaced[0].name, "ZZ 1");

        // None of them is a list of falses, and is nothing to do - never "all".
        options.consider = vec![false; 5];
        let none = expect_ok(draw_survey_features(&document, &project, &options));
        assert!(none.command.is_none());
        assert!(none.report.strings.is_empty());
        assert!(none.unplaced.is_empty());

        // A list that is not one entry per feature could only be read by
        // guessing.
        options.consider = vec![true; 4];
        let wrong = refused(draw_survey_features(&document, &project, &options));
        assert_eq!(wrong.code, ErrorCode::InvalidArgument);

        // The whole project is checked whatever is asked about: a feature left
        // out that names a point the project lacks is still an inconsistency.
        let mut broken = project.clone();
        broken.features.push(feature("KJ", "09", &["1", "404"]));
        options.consider = vec![false; 6];
        let error = refused(draw_survey_features(&document, &broken, &options));
        assert_eq!(error.code, ErrorCode::NotFound);
    }
}

// ---- the finish ------------------------------------------------------------------------

mod survey_finish {
    use super::*;

    #[test]
    fn a_field_file_is_coded_and_strung_by_its_string_names() {
        let mut document = Document::default();
        document.set_survey_map(field_map());

        let report = import_finish(&mut document, field_file(), &codes_and_linework());

        // ---- the points: ten drawn, none removed, each on its rule's layer ----
        assert_eq!(report.points, 10);
        assert_eq!(point_count(&document), 10);
        let string_of_point = [
            ("1", "01"),
            ("2", "01"),
            ("3", "02"),
            ("4", "02"),
            ("5", "01"),
            ("6", "01"),
            ("7", "12"),
            ("8", "12"),
            ("9", "1"),
            ("10", "1"),
        ];
        for (number, string) in string_of_point {
            let entity = numbered(&document, number);
            assert_eq!(text_property(entity, "string"), string, "{number}");
            // The bare "*" rule's attribute reaches every code, as CODE gives
            // it.
            assert_eq!(text_property(entity, "Surveyed"), "yes", "{number}");
        }
        for number in ["1", "2", "3", "4"] {
            // KJ01, KJ02 -> "KJ*"
            assert_eq!(
                numbered(&document, number).layer,
                "SURVEY JOINT",
                "{number}"
            );
            assert_eq!(numbered(&document, number).style, "Joint Line", "{number}");
            assert_eq!(
                text_property(numbered(&document, number), "code"),
                "KJ",
                "{number}"
            );
        }
        for number in ["5", "6"] {
            // PS01 -> "PS*"
            assert_eq!(
                numbered(&document, number).layer,
                "SURVEY MARKS",
                "{number}"
            );
            assert_eq!(numbered(&document, number).style, "Peg", "{number}");
        }
        let Some(peg) = document.model().styles.find("Peg") else {
            panic!("the style Peg was made");
        };
        assert_eq!(peg.symbol, "Peg");
        assert_eq!(peg.symbol_size, 0.5);
        for number in ["7", "8"] {
            // B12 -> "B1*"
            assert_eq!(
                numbered(&document, number).layer,
                "SURVEY BARRIER",
                "{number}"
            );
            assert_eq!(numbered(&document, number).style, "Barrier", "{number}");
        }
        for number in ["9", "10"] {
            // ZZ: only "*", so it stays put
            assert_eq!(
                numbered(&document, number).layer,
                "survey/points",
                "{number}"
            );
            assert_eq!(numbered(&document, number).style, "", "{number}");
        }

        // ---- what the coding says ----
        assert_eq!(report.why_not_coded, SurveyFinishSkip::None);
        assert_eq!(report.coding.coded, 10);
        assert_eq!(report.coding.matched, 8); // all but the two ZZ
        assert_eq!(report.coding.fallback_only, 2); // the two ZZ
        assert_eq!(report.codes_with_no_rule(), ["ZZ"]);
        let codes: Vec<&str> = report
            .coding
            .codes
            .iter()
            .map(|row| row.code.as_str())
            .collect();
        assert_eq!(codes, ["B12", "KJ01", "KJ02", "PS01", "ZZ"]);
        assert_eq!(
            report.coding.layers_created,
            ["SURVEY BARRIER", "SURVEY JOINT", "SURVEY MARKS"]
        );
        assert_eq!(
            report.coding.styles_created,
            ["Barrier", "Joint Line", "Peg"]
        );

        // ---- the lines: two KJ lines, not one; the B line; none through the
        //      marks, none through ZZ ----
        assert_eq!(report.why_not_strung, SurveyFinishSkip::None);
        let lines = lines_of(&document);
        assert_eq!(lines.len(), 3);
        assert_eq!(report.lines.len(), 3);
        assert_eq!(vertices_of(lines[0]), [p(0.0, 0.0), p(10.0, 0.0)]);
        assert_eq!(vertices_of(lines[1]), [p(0.0, 5.0), p(10.0, 5.0)]);
        assert_eq!(vertices_of(lines[2]), [p(0.0, 30.0), p(10.0, 30.0)]);
        assert_eq!(lines[0].layer, "SURVEY JOINT");
        assert_eq!(lines[0].style, "Joint Line"); // the style its points were given
        assert_eq!(lines[1].layer, "SURVEY JOINT");
        assert_eq!(lines[2].layer, "SURVEY BARRIER");
        assert_eq!(lines[2].style, "Barrier");
        assert_eq!(text_property(lines[0], "string"), "01");
        assert_eq!(text_property(lines[1], "string"), "02");
        assert_eq!(report.features.strings.len(), 3);
        assert_eq!(report.features.strings[0].name, "KJ 01");
        assert_eq!(report.features.strings[1].name, "KJ 02");
        assert_eq!(report.features.strings[2].name, "B 12");
        assert_eq!(report.unplaced_features.len(), 2);
        assert_eq!(report.unplaced_features[0].name, "PS 01");
        assert_eq!(
            report.unplaced_features[0].reason,
            UnplacedFeatureReason::PointCode
        );
        assert_eq!(report.unplaced_features[1].name, "ZZ 1");
        assert_eq!(
            report.unplaced_features[1].reason,
            UnplacedFeatureReason::NoRule
        );
        // Every point is named by a feature, so none is left to be strung by
        // its code: no point is in two lines of its own string.
        assert_eq!(report.points_in_features, 10);
        assert_eq!(report.strung.considered, 0);
        assert!(report.strung.strings.is_empty());
        assert_eq!(report.unplaced(), 2);
        // The lines made no layer or style the coding had not: one of each
        // name.
        assert_eq!(report.layers_created(), report.coding.layers_created);
        assert_eq!(report.styles_created(), report.coding.styles_created);

        // ---- the same in words: what was done, and what was left over ----
        let no_rule = "No rule for the code(s): ZZ.";
        let not_strung = "2 string(s) of the file are in no line (no rule for its code: 1; its \
                          code is a point code: 1).";
        assert_eq!(
            describe(&report),
            [
                "Survey codes were applied to the 10 point(s) drawn: 10 carry a code and 8 of \
                 those have a rule; 3 layer(s) and 3 style(s) were created.",
                no_rule,
                "Linework: 3 line(s) drawn.",
                not_strung,
            ]
        );
        // What a person may have to act on: the code nobody wrote a rule for,
        // and its string. (The marks' string is in the count, and is no fault.)
        assert_eq!(finish_warnings(&report), [no_rule, not_strung]);
    }

    // A survey mark is a point: a file whose only strings in no line are those
    // of point codes has nothing left over.
    #[test]
    fn strings_and_points_of_point_codes_are_counted_and_are_no_warning() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        // Marks 3 and 4 are string 01 of PS; mark 5 is coded PS7 whole and
        // point 6 has no code, and no feature names either.
        let project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "KJ"),
                point("2", 0.0, 10.0, "KJ"),
                point("3", 20.0, 0.0, "PS"),
                point("4", 20.0, 10.0, "PS"),
                point("5", 30.0, 0.0, "PS7"),
                point("6", 40.0, 0.0, ""),
            ],
            features: vec![
                feature("KJ", "01", &["1", "2"]),
                feature("PS", "01", &["3", "4"]),
            ],
            ..SurveyProject::default()
        };

        let report = import_finish(&mut document, project, &codes_and_linework());

        assert_eq!(report.unplaced_features.len(), 1);
        assert_eq!(
            report.unplaced_features[0].reason,
            UnplacedFeatureReason::PointCode
        );
        assert_eq!(report.strung.unplaced.len(), 2);
        assert_eq!(report.strung.unplaced[0].reason, UnplacedReason::PointCode);
        assert_eq!(report.strung.unplaced[1].reason, UnplacedReason::NoCode);
        // Six points, five coded, all five ruled ("KJ*", "PS*"): two layers,
        // two styles. One line; one string and two points that are none.
        assert_eq!(
            describe(&report),
            [
                "Survey codes were applied to the 6 point(s) drawn: 5 carry a code and 5 of \
                 those have a rule; 2 layer(s) and 2 style(s) were created.",
                "Linework: 1 line(s) drawn.",
                "1 string(s) of the file are in no line (its code is a point code: 1).",
                "2 point(s) are in no line (no code: 1; its code is a point code: 1).",
            ]
        );
        assert!(finish_warnings(&report).is_empty());
    }

    #[test]
    fn one_undo_takes_back_points_codes_and_lines_and_redo_restores_exactly_the_same_drawing() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        let before = drawing_of(&document);

        let report = import_and_finish(
            &mut document,
            field_file(),
            &codes_and_linework(),
            &onto_a_top_level_layer(),
            ExistingPointPolicy::Refuse,
        );
        let after = drawing_of(&document);
        // Ten points and three lines; the created list is the points, then the
        // lines.
        assert_eq!(after.entities.len(), 13);
        let created = document.last_created_entities();
        assert_eq!(created.len(), 13);
        for (i, id) in created.iter().enumerate() {
            let entity = &after.entities[id];
            assert_eq!(
                matches!(entity.geometry, Geometry::Polyline(_)),
                i >= 10,
                "{i}"
            );
        }
        assert_eq!(created[10..].to_vec(), report.lines);

        assert!(document.undo().is_ok());
        assert_eq!(
            drawing_of(&document),
            before,
            "points, lines, layers and styles all gone"
        );
        assert!(
            document.undo().is_err(),
            "it was ONE step: nothing is left to undo"
        );

        assert!(document.redo().is_ok());
        assert_eq!(
            drawing_of(&document),
            after,
            "the same ids, layers, styles and properties"
        );
        assert_eq!(document.last_created_entities(), created);

        assert!(document.undo().is_ok());
        assert_eq!(drawing_of(&document), before);
    }

    #[test]
    fn a_file_with_no_features_is_strung_by_its_codes() {
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        add(&mut map, line_rule("KB*", "SURVEY KERB", ""));
        add(
            &mut map,
            SurveyRule {
                key: String::from("TR*"),
                model: String::from("SURVEY DETAIL"),
                breakline: Some(SurveyBreakline::Point),
                ..SurveyRule::default()
            },
        );
        document.set_survey_map(map);

        // A delimited point list: the whole string name and its control codes
        // in the code column, and no feature at all.
        let project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "KB01 ST"),
                point("2", 0.0, 10.0, "KB01"),
                point("3", 0.0, 20.0, "KB01 END"),
                point("4", 5.0, 0.0, "KB01 ST"),
                point("5", 5.0, 10.0, "KB01 END"),
                point("6", 9.0, 9.0, "TR1"),
            ],
            ..SurveyProject::default()
        };

        let report = import_finish(&mut document, project, &codes_and_linework());

        // Coded by the string name, controls left out: five KB01 and one TR1.
        assert_eq!(report.why_not_coded, SurveyFinishSkip::None);
        assert_eq!(report.coding.coded, 6);
        assert_eq!(report.coding.matched, 6);
        assert_eq!(report.coding.codes.len(), 2);
        assert_eq!(report.coding.codes[0].code, "KB01");
        assert_eq!(report.coding.codes[0].entities, 5);
        assert_eq!(report.coding.codes[1].code, "TR1");
        assert_eq!(numbered(&document, "3").layer, "SURVEY KERB");
        assert_eq!(numbered(&document, "6").layer, "SURVEY DETAIL");
        assert_eq!(
            text_property(numbered(&document, "1"), "string"),
            "<absent>"
        );

        // In point-number order string KB01 is 1 ST, 2, 3 END, 4 ST, 5 END: a
        // line through 1, 2, 3 and a second through 4, 5. The tree is a point.
        assert_eq!(report.why_not_strung, SurveyFinishSkip::None);
        let lines = lines_of(&document);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            vertices_of(lines[0]),
            [p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)]
        );
        assert_eq!(vertices_of(lines[1]), [p(0.0, 5.0), p(10.0, 5.0)]);
        assert_eq!(lines[0].layer, "SURVEY KERB");
        assert_eq!(text_property(lines[0], "code"), "KB01");
        assert_eq!(text_property(lines[0], "string"), "<absent>");
        assert!(report.features.strings.is_empty());
        assert_eq!(report.points_in_features, 0);
        assert_eq!(report.strung.strings.len(), 2);
        assert_eq!(report.strung.strings[0].name, "KB01");
        assert_eq!(report.strung.strings[0].key, "KB*");
        assert_eq!(report.strung.strings[0].number, "01");
        assert_eq!(report.strung.unplaced.len(), 1);
        assert_eq!(report.strung.unplaced[0].point_number, "6");
        assert_eq!(report.strung.unplaced[0].reason, UnplacedReason::PointCode);
        assert_eq!(point_count(&document), 6, "an import never deletes a point");
    }

    #[test]
    fn the_fallback_rule_alone_never_makes_a_line_even_when_it_says_line() {
        let mut map = SurveyMap::default();
        add(&mut map, line_rule("KB*", "SURVEY KERB", ""));
        // A catch-all that says every code is a line.
        add(
            &mut map,
            SurveyRule {
                key: String::from("*"),
                breakline: Some(SurveyBreakline::Line),
                ..SurveyRule::default()
            },
        );

        let project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "KB01"),
                point("2", 0.0, 10.0, "KB01"),
                point("3", 5.0, 0.0, "ZZ ST"),
                point("4", 5.0, 10.0, "ZZ END"),
            ],
            ..SurveyProject::default()
        };

        let mut document = Document::default();
        document.set_survey_map(map);
        let report = import_finish(&mut document, project, &codes_and_linework());

        // KB01 is a line by "KB*". ZZ has only "*" - and two control codes -
        // and neither makes it one here.
        let lines = lines_of(&document);
        assert_eq!(lines.len(), 1);
        assert_eq!(vertices_of(lines[0]), [p(0.0, 0.0), p(10.0, 0.0)]);
        assert_eq!(report.strung.unplaced.len(), 2);
        assert_eq!(report.strung.unplaced[0].point_number, "3");
        assert_eq!(report.strung.unplaced[0].reason, UnplacedReason::NoRule);
        assert_eq!(report.strung.unplaced[1].point_number, "4");
        assert_eq!(report.strung.unplaced[1].reason, UnplacedReason::NoRule);

        // Asked for by hand, without the option, the same two points ARE a
        // line: that is Process Linework's rule, and it is unchanged.
        let by_hand = LineworkOptions {
            ids: vec![numbered(&document, "3").id, numbered(&document, "4").id],
            ..LineworkOptions::default()
        };
        let planned = expect_ok(process_linework(&document, &by_hand));
        assert_eq!(planned.report.strings.len(), 1);
    }

    // The three ways a finish has nothing to do: each leaves the drawing
    // exactly as the same import leaves it without a finish, and says why.
    struct NothingToDo {
        what: &'static str,
        with_map: bool,
        points: Vec<SurveyPoint>,
        why: SurveyFinishSkip,
        // The reply word of the design brief.
        word: &'static str,
        // What each step says of itself, in words.
        codes_sentence: &'static str,
        linework_sentence: &'static str,
    }

    #[test]
    fn with_nothing_to_do_the_points_are_drawn_as_without_it_and_the_report_says_why() {
        let cases = [
            // No survey codes loaded: also a string by control codes is not
            // drawn.
            NothingToDo {
                what: "empty map",
                with_map: false,
                points: vec![
                    point("1", 0.0, 0.0, "KB01 ST"),
                    point("2", 0.0, 10.0, "KB01 END"),
                ],
                why: SurveyFinishSkip::NoSurveyCodes,
                word: "no-survey-codes",
                codes_sentence: "Survey codes were not applied: no survey codes are loaded.",
                linework_sentence: "No linework was drawn: no survey codes are loaded.",
            },
            // A coordinate list with no code column.
            NothingToDo {
                what: "no codes",
                with_map: true,
                points: vec![point("1", 0.0, 0.0, ""), point("2", 0.0, 10.0, "")],
                why: SurveyFinishSkip::NoCodesInFile,
                word: "no-codes-in-file",
                codes_sentence: "Survey codes were not applied: none of the points carries a code.",
                linework_sentence: "No linework was drawn: none of the points carries a code.",
            },
            // Codes nothing but the bare "*" answers: not even its attribute
            // is set.
            NothingToDo {
                what: "no rule",
                with_map: true,
                points: vec![
                    point("1", 0.0, 0.0, "QQ ST"),
                    point("2", 0.0, 10.0, "QQ END"),
                ],
                why: SurveyFinishSkip::NoRuleMatches,
                word: "no-rule-matches",
                codes_sentence: "Survey codes were not applied: no rule matches any of their \
                                 codes (QQ).",
                linework_sentence: "No linework was drawn: no rule matches any of their codes.",
            },
        ];
        for entry in cases {
            let what = entry.what;
            let project = SurveyProject {
                points: entry.points.clone(),
                ..SurveyProject::default()
            };

            let mut plain = Document::default();
            let mut finished = Document::default();
            if entry.with_map {
                plain.set_survey_map(field_map());
                finished.set_survey_map(field_map());
            }
            let _ = import_finish(&mut plain, project.clone(), &SurveyFinishOptions::default());
            let report = import_finish(&mut finished, project, &codes_and_linework());

            assert_eq!(drawing_of(&finished), drawing_of(&plain), "{what}");
            assert_eq!(report.points, 2, "{what}");
            assert_eq!(report.why_not_coded, entry.why, "{what}");
            assert_eq!(report.why_not_strung, entry.why, "{what}");
            assert_eq!(report.why_not_coded.name(), entry.word, "{what}");
            assert!(report.lines.is_empty(), "{what}");
            assert!(report.layers_created().is_empty(), "{what}");
            assert!(report.styles_created().is_empty(), "{what}");
            // Said in words too, one sentence a step - and each is something
            // to act on: the step was asked for and did not happen.
            let sentences = [entry.codes_sentence, entry.linework_sentence];
            assert_eq!(describe(&report), sentences, "{what}");
            assert_eq!(finish_warnings(&report), sentences, "{what}");
            // Still ONE undo step, which leaves what undoing the plain import
            // leaves (the parent of the default layer stays behind in both).
            assert!(finished.undo().is_ok(), "{what}");
            assert!(plain.undo().is_ok(), "{what}");
            assert!(finished.model().entities.empty(), "{what}");
            assert_eq!(drawing_of(&finished), drawing_of(&plain), "{what}");
            assert!(finished.undo().is_err(), "{what}");
        }
    }

    #[test]
    fn codes_that_match_nothing_are_named_in_the_report() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        let project = SurveyProject {
            points: vec![point("1", 0.0, 0.0, "QQ"), point("2", 0.0, 10.0, "RR")],
            features: vec![feature("QQ", "5", &["1"]), feature("RR", "", &["2"])],
            ..SurveyProject::default()
        };

        let report = import_finish(&mut document, project, &codes_and_linework());

        assert_eq!(report.why_not_coded, SurveyFinishSkip::NoRuleMatches);
        assert_eq!(report.coding.coded, 2);
        assert_eq!(report.coding.matched, 0);
        assert_eq!(report.codes_with_no_rule(), ["QQ", "RR"]);
        assert!(report.coding.styles_created.is_empty());
        assert_eq!(
            report.coding.changed, 0,
            "nothing was changed, so nothing is claimed"
        );
        assert_eq!(report.why_not_strung, SurveyFinishSkip::NoRuleMatches);
        // QQ 5 is a string of the file's own, and is refused as one. The
        // feature of RR has no name, so it is not one: its point is refused by
        // its code.
        assert_eq!(report.unplaced_features.len(), 1);
        assert_eq!(report.unplaced_features[0].name, "QQ 5");
        assert_eq!(report.unplaced_features[0].index, 0);
        assert_eq!(
            report.unplaced_features[0].reason,
            UnplacedFeatureReason::NoRule
        );
        assert_eq!(report.points_in_features, 1);
        assert_eq!(report.strung.unplaced.len(), 1);
        assert_eq!(report.strung.unplaced[0].point_number, "2");
        assert_eq!(report.strung.unplaced[0].reason, UnplacedReason::NoRule);
        assert!(lines_of(&document).is_empty());
        // Both steps say the same thing, naming the codes, and both are
        // warnings.
        let sentences = [
            "Survey codes were not applied: no rule matches any of their codes (QQ, RR).",
            "No linework was drawn: no rule matches any of their codes.",
        ];
        assert_eq!(describe(&report), sentences);
        assert_eq!(finish_warnings(&report), sentences);
        // The points still carry what the file said, for a rule added later:
        // the one thing a finish with nothing to do leaves behind, and only
        // when survey codes are loaded.
        assert_eq!(text_property(numbered(&document, "1"), "string"), "5");
        assert_eq!(
            text_property(numbered(&document, "1"), "Surveyed"),
            "<absent>"
        );
    }

    #[test]
    fn with_both_options_off_the_command_is_the_one_it_was_given() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        let project = field_file();
        let points = expect_ok(import_survey_points(
            &document,
            &project,
            &defaults(),
            ExistingPointPolicy::Refuse,
            None,
        ));
        let Some(points) = points else {
            panic!("the import draws the points");
        };
        let given: *const dyn Command = &*points;
        let report: SharedFinishReport = Arc::default();
        report.lock().unwrap().points = 99; // overwritten: the report is this call's

        let command = with_survey_finish(
            &document,
            Some(points),
            project.clone(),
            &defaults(),
            SurveyFinishOptions::default(),
            Some(Arc::clone(&report)),
            SurveyFinishEarlier::default(),
        );

        let Some(command) = command else {
            panic!("the command it was given");
        };
        let returned: *const dyn Command = &*command;
        assert!(
            std::ptr::addr_eq(returned, given),
            "unwrapped: the very command, so the very behaviour"
        );
        {
            let report = report.lock().unwrap();
            assert_eq!(report.points, 0);
            assert_eq!(report.why_not_coded, SurveyFinishSkip::NotAsked);
            assert_eq!(report.why_not_strung, SurveyFinishSkip::NotAsked);
            assert!(describe(&report).is_empty());
        }

        must(&mut document, command);
        // Ten points on the import's layer, uncoded and unstrung, with no
        // string number: exactly what the import alone draws.
        let mut alone = Document::default();
        alone.set_survey_map(field_map());
        let same = expect_ok(import_survey_points(
            &alone,
            &project,
            &defaults(),
            ExistingPointPolicy::Refuse,
            None,
        ));
        let Some(same) = same else {
            panic!("the import draws the points");
        };
        must(&mut alone, same);
        assert_eq!(drawing_of(&document), drawing_of(&alone));
        assert_eq!(
            text_property(numbered(&document, "1"), "string"),
            "<absent>"
        );
        assert!(lines_of(&document).is_empty());
    }

    #[test]
    fn nothing_to_import_is_still_no_command_and_the_report_says_no_points() {
        let document = Document::default();
        let report: SharedFinishReport = Arc::default();
        let codes_only = SurveyFinishOptions {
            codes: true,
            ..SurveyFinishOptions::default()
        };

        let command = with_survey_finish(
            &document,
            None,
            SurveyProject::default(),
            &defaults(),
            codes_only,
            Some(Arc::clone(&report)),
            SurveyFinishEarlier::default(),
        );

        assert!(command.is_none());
        let report = report.lock().unwrap();
        assert_eq!(report.why_not_coded, SurveyFinishSkip::NoPoints);
        assert_eq!(report.why_not_strung, SurveyFinishSkip::NotAsked);
    }

    // An empty id list means EVERY entity to apply_survey_codes and every
    // point to process_linework. A wrapped command that draws no point must
    // therefore code and string nothing - not the whole drawing.
    #[test]
    fn a_command_that_draws_no_point_leaves_the_rest_of_the_drawing_alone() {
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        add(&mut map, line_rule("KB*", "SURVEY KERB", ""));
        document.set_survey_map(map);
        // Two kerb points already there, uncoded: a CODE or a LINEWORK run
        // over the whole drawing would move them and join them.
        let first = add_point(&mut document, p(0.0, 0.0), "KB01", "1", "");
        let second = add_point(&mut document, p(10.0, 0.0), "KB01", "2", "");
        let before = drawing_of(&document);

        let layer = Layer {
            name: String::from("empty"),
            ..Layer::default()
        };
        let report: SharedFinishReport = Arc::default();
        let command = with_survey_finish(
            &document,
            Some(create_layer(layer)),
            SurveyProject::default(),
            &defaults(),
            codes_and_linework(),
            Some(Arc::clone(&report)),
            SurveyFinishEarlier::default(),
        );
        let Some(command) = command else {
            panic!("the finish wraps the command");
        };
        must(&mut document, command);

        let report = report.lock().unwrap().clone();
        assert_eq!(report.points, 0);
        assert_eq!(report.why_not_coded, SurveyFinishSkip::NoPoints);
        assert_eq!(report.why_not_strung, SurveyFinishSkip::NoPoints);
        assert_eq!(layer_of(&document, first), "0");
        assert_eq!(layer_of(&document, second), "0");
        assert_eq!(drawing_of(&document).entities, before.entities);
        assert!(document.model().layers.contains("empty"));
        assert!(!document.model().layers.contains("SURVEY KERB"));
    }

    #[test]
    fn a_step_that_fails_takes_the_whole_import_with_it_and_says_which_step() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        let before = drawing_of(&document);
        let project = field_file();
        let options = onto_a_top_level_layer();
        let points = expect_ok(import_survey_points(
            &document,
            &project,
            &options,
            ExistingPointPolicy::Refuse,
            None,
        ));

        // Start and end spelled alike: no control code can be read.
        let mut finish = codes_and_linework();
        finish.controls.end = finish.controls.start.clone();
        let command = with_survey_finish(
            &document,
            points,
            project.clone(),
            &options,
            finish,
            None,
            SurveyFinishEarlier::default(),
        );
        let Some(command) = command else {
            panic!("the finish wraps the import");
        };
        let error = refused(document.execute(command));

        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(
            drawing_of(&document),
            before,
            "refused before a point is drawn"
        );
        assert!(document.undo().is_err(), "and it left no undo step");

        // A failure AFTER the points are drawn: the features name a point the
        // project does not have, which draw_survey_features refuses. The
        // points and their codes are taken back with it.
        let again = expect_ok(import_survey_points(
            &document,
            &project,
            &options,
            ExistingPointPolicy::Refuse,
            None,
        ));
        let mut broken = project.clone();
        broken.features.push(feature("KJ", "09", &["1", "404"]));
        let second = with_survey_finish(
            &document,
            again,
            broken,
            &options,
            codes_and_linework(),
            None,
            SurveyFinishEarlier::default(),
        );
        let Some(second) = second else {
            panic!("the finish wraps the import");
        };
        let failed = refused(document.execute(second));
        // survey's validate_project's own refusal of a reference to a point the
        // project lacks (data_model.rs: NotFound), with the step named.
        assert_eq!(failed.code, ErrorCode::NotFound);
        assert!(failed.context.contains("features"), "{}", failed.context);
        assert_eq!(drawing_of(&document), before);
        assert!(
            document.undo().is_err(),
            "and this one left no undo step either"
        );
    }

    // ---- which strings are the file's own, and which points are left to their codes ----

    // A reader whose format has no string numbers makes a feature of every RUN
    // of consecutive shots of one code, with no name. Shot in sections across
    // a road - left kerb, right kerb, right kerb, left kerb - each kerb is then
    // several runs, some of one point, and none of them is the kerb. The
    // points' codes still say which string each is of, exactly as they do to
    // Process Linework.
    #[test]
    fn a_feature_with_no_name_is_not_a_string_of_the_file_so_its_points_are_strung_by_their_codes()
    {
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        add(&mut map, line_rule("KB*", "SURVEY KERB", ""));
        add(
            &mut map,
            SurveyRule {
                key: String::from("TR*"),
                model: String::from("SURVEY DETAIL"),
                breakline: Some(SurveyBreakline::Point),
                ..SurveyRule::default()
            },
        );
        document.set_survey_map(map);

        // Left kerb KB01 along E 0, right kerb KB02 along E 6, and a tree shot
        // between two shots of the left kerb. The runs of consecutive equal
        // codes: 1 | 2 3 | 4 5 | 6 | 7.
        let project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "KB01"),
                point("2", 0.0, 6.0, "KB02"),
                point("3", 10.0, 6.0, "KB02"),
                point("4", 10.0, 0.0, "KB01"),
                point("5", 20.0, 0.0, "KB01"),
                point("6", 25.0, 1.0, "TR1"),
                point("7", 30.0, 0.0, "KB01"),
            ],
            features: vec![
                feature("KB01", "", &["1"]),
                feature("KB02", "", &["2", "3"]),
                feature("KB01", "", &["4", "5"]),
                feature("TR1", "", &["6"]),
                feature("KB01", "", &["7"]),
            ],
            ..SurveyProject::default()
        };

        let report = import_finish(&mut document, project, &codes_and_linework());

        // By code, in point-number order: KB01 is 1, 4, 5, 7 and KB02 is 2, 3.
        assert_eq!(report.why_not_strung, SurveyFinishSkip::None);
        let lines = lines_of(&document);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            vertices_of(lines[0]),
            [p(0.0, 0.0), p(0.0, 10.0), p(0.0, 20.0), p(0.0, 30.0)]
        );
        assert_eq!(vertices_of(lines[1]), [p(6.0, 0.0), p(6.0, 10.0)]);
        assert_eq!(text_property(lines[0], "code"), "KB01");
        assert_eq!(text_property(lines[0], "string"), "<absent>");
        assert_eq!(lines[0].layer, "SURVEY KERB");
        // None of the five runs is drawn, or reported, as a string of the
        // file.
        assert!(report.features.strings.is_empty());
        assert!(report.unplaced_features.is_empty());
        assert_eq!(report.points_in_features, 0);
        assert_eq!(report.strung.considered, 7);
        assert_eq!(report.strung.strings.len(), 2);
        assert_eq!(report.strung.strings[0].name, "KB01");
        assert_eq!(report.strung.strings[0].point_numbers, ["1", "4", "5", "7"]);
        assert_eq!(report.strung.strings[1].name, "KB02");
        assert_eq!(report.strung.strings[1].point_numbers, ["2", "3"]);
        assert_eq!(report.strung.unplaced.len(), 1);
        assert_eq!(report.strung.unplaced[0].point_number, "6");
        assert_eq!(report.strung.unplaced[0].reason, UnplacedReason::PointCode);
    }

    // A controller's keyed line joins two points and has a name and a code of
    // its own. Its ends are still points of THEIR code's string: the line
    // does not take them out of it, whether a rule draws the line or not.
    #[test]
    fn the_ends_of_a_named_line_of_another_code_stay_in_their_own_codes_string() {
        for boundary_is_a_line in [false, true] {
            let trace = if boundary_is_a_line {
                "BDY* is a line"
            } else {
                "no rule for BDY"
            };
            let mut document = Document::default();
            let mut map = SurveyMap::default();
            add(&mut map, line_rule("KB*", "SURVEY KERB", ""));
            if boundary_is_a_line {
                add(&mut map, line_rule("BDY*", "SURVEY BOUNDARY", ""));
            }
            document.set_survey_map(map);

            // Four kerb shots along N 0, and a line "L1" coded BDY keyed
            // between the second and the third.
            let project = SurveyProject {
                points: vec![
                    point("1", 0.0, 0.0, "KB01"),
                    point("2", 0.0, 10.0, "KB01"),
                    point("3", 0.0, 20.0, "KB01"),
                    point("4", 0.0, 30.0, "KB01"),
                ],
                features: vec![feature("BDY", "L1", &["2", "3"])],
                ..SurveyProject::default()
            };

            let report = import_finish(&mut document, project, &codes_and_linework());

            let lines = lines_of(&document);
            let kerb = [p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0), p(30.0, 0.0)];
            if boundary_is_a_line {
                // The file's own string first, then the kerb through all four.
                assert_eq!(lines.len(), 2, "{trace}");
                assert_eq!(
                    vertices_of(lines[0]),
                    [p(10.0, 0.0), p(20.0, 0.0)],
                    "{trace}"
                );
                assert_eq!(text_property(lines[0], "code"), "BDY", "{trace}");
                assert_eq!(text_property(lines[0], "string"), "L1", "{trace}");
                assert_eq!(lines[0].layer, "SURVEY BOUNDARY", "{trace}");
                assert_eq!(vertices_of(lines[1]), kerb, "{trace}");
                assert!(report.unplaced_features.is_empty(), "{trace}");
            } else {
                assert_eq!(lines.len(), 1, "{trace}");
                assert_eq!(vertices_of(lines[0]), kerb, "{trace}");
                assert_eq!(report.unplaced_features.len(), 1, "{trace}");
                assert_eq!(report.unplaced_features[0].name, "BDY L1", "{trace}");
                assert_eq!(
                    report.unplaced_features[0].reason,
                    UnplacedFeatureReason::NoRule,
                    "{trace}"
                );
            }
            // The line is of another code, so it gave its ends no string
            // number and left them to their own.
            assert_eq!(
                text_property(numbered(&document, "2"), "string"),
                "<absent>",
                "{trace}"
            );
            assert_eq!(report.points_in_features, 0, "{trace}");
            assert_eq!(report.strung.considered, 4, "{trace}");
            assert_eq!(report.strung.strings.len(), 1, "{trace}");
            assert_eq!(
                report.strung.strings[0].point_numbers,
                ["1", "2", "3", "4"],
                "{trace}"
            );
        }
    }

    #[test]
    fn a_file_with_strings_of_its_own_and_points_outside_them_draws_both_kinds_of_line() {
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        add(&mut map, line_rule("KJ*", "SURVEY JOINT", ""));
        add(&mut map, line_rule("KB*", "SURVEY KERB", ""));
        document.set_survey_map(map);

        // Points 1 and 2 are string 01 of KJ, by a feature; 3, 4 and 5 are
        // coded KB01 whole and no feature names them.
        let project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "KJ"),
                point("2", 0.0, 10.0, "KJ"),
                point("3", 5.0, 0.0, "KB01"),
                point("4", 5.0, 10.0, "KB01"),
                point("5", 5.0, 20.0, "KB01"),
            ],
            features: vec![feature("KJ", "01", &["1", "2"])],
            ..SurveyProject::default()
        };

        let report = import_finish(&mut document, project, &codes_and_linework());

        let lines = lines_of(&document);
        assert_eq!(lines.len(), 2);
        assert_eq!(vertices_of(lines[0]), [p(0.0, 0.0), p(10.0, 0.0)]);
        assert_eq!(text_property(lines[0], "code"), "KJ");
        assert_eq!(text_property(lines[0], "string"), "01");
        assert_eq!(lines[0].layer, "SURVEY JOINT");
        assert_eq!(
            vertices_of(lines[1]),
            [p(0.0, 5.0), p(10.0, 5.0), p(20.0, 5.0)]
        );
        assert_eq!(text_property(lines[1], "code"), "KB01");
        assert_eq!(lines[1].layer, "SURVEY KERB");
        // Two points were the feature's; the other three were strung by code.
        assert_eq!(report.points_in_features, 2);
        assert_eq!(report.strung.considered, 3);
        assert_eq!(report.features.strings.len(), 1);
        assert_eq!(report.strung.strings.len(), 1);
        assert_eq!(report.strung.strings[0].name, "KB01");
        assert_eq!(report.lines.len(), 2);
        assert_eq!(report.unplaced(), 0);
    }

    // "It acts on what the import created" with something else there to act
    // on: the drawing already holds two kerb points of the very string name
    // the import brings.
    #[test]
    fn points_already_in_the_drawing_are_neither_coded_nor_strung_with_the_import() {
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        add(&mut map, line_rule("KB*", "SURVEY KERB", ""));
        document.set_survey_map(map);
        let first = add_point(&mut document, p(0.0, 0.0), "KB01", "1", "");
        let second = add_point(&mut document, p(10.0, 0.0), "KB01", "2", "");
        let before = drawing_of(&document);
        let steps = document.history().undo_count();

        let project = SurveyProject {
            points: vec![point("3", 5.0, 0.0, "KB01"), point("4", 5.0, 10.0, "KB01")],
            ..SurveyProject::default()
        };
        let report = import_and_finish(
            &mut document,
            project,
            &codes_and_linework(),
            &onto_a_top_level_layer(),
            ExistingPointPolicy::Refuse,
        );

        // Points 3 and 4 alone: coded, and joined to each other and not to 1
        // or 2.
        assert_eq!(report.points, 2);
        assert_eq!(report.coding.coded, 2);
        assert_eq!(report.strung.considered, 2);
        let lines = lines_of(&document);
        assert_eq!(lines.len(), 1);
        assert_eq!(vertices_of(lines[0]), [p(0.0, 5.0), p(10.0, 5.0)]);
        assert_eq!(numbered(&document, "3").layer, "SURVEY KERB");
        assert_eq!(numbered(&document, "4").layer, "SURVEY KERB");
        // The two that were there are as they were: uncoded, on layer "0".
        assert_eq!(
            document.model().entities.find(first),
            before.entities.get(&first)
        );
        assert_eq!(
            document.model().entities.find(second),
            before.entities.get(&second)
        );

        assert_eq!(document.history().undo_count(), steps + 1);
        assert!(document.undo().is_ok());
        assert_eq!(drawing_of(&document), before);
    }

    // A longer copy of a file already imported, topped up with Skip: the
    // import creates only the points that are new, and so draws only the
    // strings that run through one of them - not every string of the file a
    // second time.
    #[test]
    fn an_import_draws_only_the_strings_of_the_file_that_name_a_point_it_created() {
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        add(&mut map, line_rule("KJ*", "SURVEY JOINT", ""));
        document.set_survey_map(map);

        let day_one = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "KJ"),
                point("2", 0.0, 10.0, "KJ"),
                point("3", 5.0, 0.0, "KJ"),
                point("4", 5.0, 10.0, "KJ"),
            ],
            features: vec![
                feature("KJ", "01", &["1", "2"]),
                feature("KJ", "02", &["3", "4"]),
            ],
            ..SurveyProject::default()
        };
        let _ = import_finish(&mut document, day_one.clone(), &codes_and_linework());
        assert_eq!(lines_of(&document).len(), 2);

        // The same file a day later: one more shot, of string 02.
        let mut day_two = day_one;
        day_two.points.push(point("5", 5.0, 20.0, "KJ"));
        day_two.features[1].point_ids.push(String::from("5"));
        let report = import_and_finish(
            &mut document,
            day_two,
            &codes_and_linework(),
            &defaults(),
            ExistingPointPolicy::Skip,
        );

        assert_eq!(report.points, 1, "point 5 alone is new");
        // String 02 is drawn, through all three of its points; string 01, none
        // of whose points this import created, is not drawn again.
        assert_eq!(report.lines.len(), 1);
        let Some(drawn) = document.model().entities.find(report.lines[0]) else {
            panic!("the line is in the drawing");
        };
        assert_eq!(
            vertices_of(drawn),
            [p(0.0, 5.0), p(10.0, 5.0), p(20.0, 5.0)]
        );
        assert_eq!(report.features.strings.len(), 1);
        assert_eq!(report.features.strings[0].name, "KJ 02");
        assert!(report.unplaced_features.is_empty());
        assert_eq!(lines_of(&document).len(), 3);
        assert_eq!(report.points_in_features, 1);
        assert_eq!(text_property(numbered(&document, "5"), "string"), "02");
        assert_eq!(numbered(&document, "5").layer, "SURVEY JOINT");
    }

    // ---- one option without the other ----

    #[test]
    fn codes_alone_move_the_points_to_their_rules_and_draw_no_line() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        let codes_only = SurveyFinishOptions {
            codes: true,
            ..SurveyFinishOptions::default()
        };

        let report = import_finish(&mut document, field_file(), &codes_only);

        assert_eq!(report.why_not_coded, SurveyFinishSkip::None);
        assert_eq!(report.why_not_strung, SurveyFinishSkip::NotAsked);
        assert!(report.lines.is_empty());
        assert!(lines_of(&document).is_empty());
        assert_eq!(document.last_created_entities().len(), 10);
        // Coded exactly as with both options: by code and string number.
        assert_eq!(numbered(&document, "1").layer, "SURVEY JOINT");
        assert_eq!(numbered(&document, "5").style, "Peg");
        assert_eq!(numbered(&document, "7").layer, "SURVEY BARRIER");
        assert_eq!(text_property(numbered(&document, "7"), "string"), "12");
        assert_eq!(numbered(&document, "9").layer, "survey/points");
        assert_eq!(report.coding.matched, 8);
        assert_eq!(
            report.unplaced(),
            0,
            "nothing was strung, so nothing was left unstrung"
        );
    }

    #[test]
    fn linework_alone_draws_the_styled_lines_and_leaves_the_points_as_the_import_drew_them() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        let linework_only = SurveyFinishOptions {
            linework: true,
            ..SurveyFinishOptions::default()
        };

        let report = import_finish(&mut document, field_file(), &linework_only);

        assert_eq!(report.why_not_coded, SurveyFinishSkip::NotAsked);
        assert_eq!(report.coding.coded, 0);
        assert_eq!(report.why_not_strung, SurveyFinishSkip::None);
        // Every point where the import put it, unstyled, without the "*"
        // rule's attribute - and with its string number, which the lines are
        // found by.
        for number in ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"] {
            let entity = numbered(&document, number);
            assert_eq!(entity.layer, "survey/points", "{number}");
            assert_eq!(entity.style, "", "{number}");
            assert_eq!(text_property(entity, "Surveyed"), "<absent>", "{number}");
        }
        assert_eq!(text_property(numbered(&document, "3"), "string"), "02");

        // KJ 01, KJ 02 and B 12, each on its rule's layer in its rule's style.
        let lines = lines_of(&document);
        assert_eq!(lines.len(), 3);
        assert_eq!(vertices_of(lines[1]), [p(0.0, 5.0), p(10.0, 5.0)]);
        assert_eq!(lines[0].layer, "SURVEY JOINT");
        assert_eq!(lines[0].style, "Joint Line");
        assert_eq!(lines[1].style, "Joint Line");
        assert_eq!(lines[2].layer, "SURVEY BARRIER");
        assert_eq!(lines[2].style, "Barrier");
        // The layers and styles are the LINES' this time: the points made
        // none.
        assert!(report.coding.layers_created.is_empty());
        assert_eq!(report.layers_created(), ["SURVEY BARRIER", "SURVEY JOINT"]);
        assert_eq!(report.styles_created(), ["Barrier", "Joint Line"]);
        assert!(!document.model().layers.contains("SURVEY MARKS"));
    }

    // ---- what the options carry reaches the steps ----

    #[test]
    fn the_colour_lookup_styles_the_points_and_both_kinds_of_line_alike() {
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        let mut joint = line_rule("KJ*", "SURVEY JOINT", "Joint Line");
        joint.colour = String::from("red");
        add(&mut map, joint);
        document.set_survey_map(map);

        // String 01 by a feature, and two points coded KJ05 whole that no
        // feature names: a line of the file's and a line strung by code, of
        // one rule.
        let project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "KJ"),
                point("2", 0.0, 10.0, "KJ"),
                point("3", 5.0, 0.0, "KJ05"),
                point("4", 5.0, 10.0, "KJ05"),
            ],
            features: vec![feature("KJ", "01", &["1", "2"])],
            ..SurveyProject::default()
        };
        let red: ColourOf = Arc::new(|name: &str| {
            if name == "red" {
                Some(Color {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                })
            } else {
                None
            }
        });
        let mut finish = codes_and_linework();
        finish.coding.colour_of = Some(red);

        let report = import_finish(&mut document, project, &finish);

        // ONE style, coloured: a step that had lost the lookup would not know
        // "red" as a colour, could not reuse the points' style and would make
        // a second one for its lines.
        assert_eq!(report.styles_created(), ["Joint Line"]);
        let Some(style) = document.model().styles.find("Joint Line") else {
            panic!("the style Joint Line was made");
        };
        assert_eq!(
            style.color,
            Some(Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            })
        );
        assert_eq!(document.model().styles.all().len(), 1);
        for number in ["1", "2", "3", "4"] {
            assert_eq!(numbered(&document, number).style, "Joint Line", "{number}");
        }
        let lines = lines_of(&document);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].style, "Joint Line");
        assert_eq!(lines[1].style, "Joint Line");
    }

    #[test]
    fn the_control_codes_and_the_order_are_the_ones_the_options_give() {
        let mut map = SurveyMap::default();
        add(&mut map, line_rule("KB*", "SURVEY KERB", ""));
        // Four kerb shots; the first and the third say "BEG".
        let project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "KB01 BEG"),
                point("2", 0.0, 10.0, "KB01"),
                point("3", 5.0, 0.0, "KB01 BEG"),
                point("4", 5.0, 10.0, "KB01"),
            ],
            ..SurveyProject::default()
        };
        {
            // BEG is no control code by default: one line through all four.
            let mut document = Document::default();
            document.set_survey_map(map.clone());
            let _ = import_finish(&mut document, project.clone(), &codes_and_linework());
            let lines = lines_of(&document);
            assert_eq!(lines.len(), 1);
            assert_eq!(vertices_of(lines[0]).len(), 4);
        }
        {
            // Spelled as the start code, it begins a line at 1 and another at 3.
            let mut document = Document::default();
            document.set_survey_map(map.clone());
            let mut finish = codes_and_linework();
            finish.controls.start = String::from("BEG");
            let _ = import_finish(&mut document, project, &finish);
            let lines = lines_of(&document);
            assert_eq!(lines.len(), 2);
            assert_eq!(vertices_of(lines[0]), [p(0.0, 0.0), p(10.0, 0.0)]);
            assert_eq!(vertices_of(lines[1]), [p(0.0, 5.0), p(10.0, 5.0)]);
        }

        // The order: point "2" is listed, and so drawn, before point "1".
        let backwards = SurveyProject {
            points: vec![point("2", 0.0, 0.0, "KB01"), point("1", 0.0, 10.0, "KB01")],
            ..SurveyProject::default()
        };
        {
            let mut document = Document::default();
            document.set_survey_map(map.clone());
            let _ = import_finish(&mut document, backwards.clone(), &codes_and_linework());
            let lines = lines_of(&document);
            assert_eq!(lines.len(), 1);
            // By point number, the default: from 1 (E 10) to 2 (E 0).
            assert_eq!(vertices_of(lines[0]), [p(10.0, 0.0), p(0.0, 0.0)]);
        }
        {
            let mut document = Document::default();
            document.set_survey_map(map);
            let mut finish = codes_and_linework();
            finish.order = LineworkOrder::EntityOrder;
            let _ = import_finish(&mut document, backwards, &finish);
            let lines = lines_of(&document);
            assert_eq!(lines.len(), 1);
            // In the order drawn: from 2 (E 0) to 1 (E 10).
            assert_eq!(vertices_of(lines[0]), [p(0.0, 0.0), p(10.0, 0.0)]);
        }
    }

    #[test]
    fn an_import_under_other_property_names_is_coded_and_strung_by_those_names() {
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        add(&mut map, line_rule("KJ*", "SURVEY JOINT", ""));
        add(&mut map, line_rule("KB*", "SURVEY KERB", ""));
        document.set_survey_map(map);
        let options = SurveyImportOptions {
            code_property: String::from("fieldcode"),
            point_number_property: String::from("ptno"),
            ..SurveyImportOptions::default()
        };

        let project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "KJ"),
                point("2", 0.0, 10.0, "KJ"),
                point("3", 5.0, 0.0, "KB01"),
                point("4", 5.0, 10.0, "KB01"),
            ],
            features: vec![feature("KJ", "01", &["1", "2"])],
            ..SurveyProject::default()
        };

        let report = import_and_finish(
            &mut document,
            project,
            &codes_and_linework(),
            &options,
            ExistingPointPolicy::Refuse,
        );

        assert_eq!(report.coding.property, "fieldcode");
        assert_eq!(report.coding.coded, 4);
        assert_eq!(report.coding.matched, 4);
        // The same two lines as under the default names, carrying the code
        // where the points carry it.
        let lines = lines_of(&document);
        assert_eq!(lines.len(), 2);
        assert_eq!(vertices_of(lines[0]), [p(0.0, 0.0), p(10.0, 0.0)]);
        assert_eq!(text_property(lines[0], "fieldcode"), "KJ");
        assert_eq!(text_property(lines[0], "code"), "<absent>");
        assert_eq!(text_property(lines[0], "string"), "01");
        assert_eq!(vertices_of(lines[1]), [p(0.0, 5.0), p(10.0, 5.0)]);
        assert_eq!(text_property(lines[1], "fieldcode"), "KB01");
        assert_eq!(lines[1].layer, "SURVEY KERB");
        // The points found by their number under its own name: two the
        // feature's, two left to their code.
        assert_eq!(report.points_in_features, 2);
        assert_eq!(report.strung.considered, 2);
        assert_eq!(report.strung.property, "fieldcode");
    }

    // The degenerate import: no point number is written, so a point cannot be
    // found among the ids a feature lists. A point is then left to the file's
    // strings when the file has a numbered string of its CODE, and strung by
    // its code otherwise - in the order drawn, there being no number to order
    // by.
    #[test]
    fn an_import_that_writes_no_point_number_tells_the_files_points_by_their_code() {
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        add(&mut map, line_rule("KJ*", "SURVEY JOINT", ""));
        add(&mut map, line_rule("KB*", "SURVEY KERB", ""));
        document.set_survey_map(map);
        let options = SurveyImportOptions {
            point_number_property: String::new(),
            ..SurveyImportOptions::default()
        };
        let mut finish = codes_and_linework();
        finish.order = LineworkOrder::EntityOrder;

        let project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "KJ"),
                point("2", 0.0, 10.0, "KJ"),
                point("3", 5.0, 0.0, "KB01"),
                point("4", 5.0, 10.0, "KB01"),
            ],
            features: vec![feature("KJ", "01", &["1", "2"])],
            ..SurveyProject::default()
        };

        let report = import_and_finish(
            &mut document,
            project,
            &finish,
            &options,
            ExistingPointPolicy::Refuse,
        );

        assert_eq!(report.points, 4);
        let lines = lines_of(&document);
        assert_eq!(lines.len(), 2);
        // KJ 01 from the file's own coordinates; KB01 by code, in entity order.
        assert_eq!(vertices_of(lines[0]), [p(0.0, 0.0), p(10.0, 0.0)]);
        assert_eq!(text_property(lines[0], "string"), "01");
        assert_eq!(vertices_of(lines[1]), [p(0.0, 5.0), p(10.0, 5.0)]);
        assert_eq!(text_property(lines[1], "code"), "KB01");
        assert_eq!(
            report.points_in_features, 2,
            "the two KJ points: the file numbers a KJ string"
        );
        assert_eq!(report.strung.considered, 2);
        assert!(report.strung.unplaced.is_empty());
    }

    // ---- refusals ----

    #[test]
    fn the_command_refuses_a_document_it_was_not_planned_for() {
        let mut planned = Document::default();
        planned.set_survey_map(field_map());
        let mut other = Document::default();
        other.set_survey_map(field_map());
        let project = field_file();
        let points = expect_ok(import_survey_points(
            &planned,
            &project,
            &defaults(),
            ExistingPointPolicy::Refuse,
            None,
        ));
        let command = with_survey_finish(
            &planned,
            points,
            project,
            &defaults(),
            codes_and_linework(),
            None,
            SurveyFinishEarlier::default(),
        );
        let Some(command) = command else {
            panic!("the finish wraps the import");
        };

        let error = refused(other.execute(command));

        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(other.model().entities.empty());
        assert!(planned.model().entities.empty());
        assert_eq!(other.history().undo_count(), 0);
    }
}

// ---- which rule decides line or point --------------------------------------------------

mod linework_ruled {
    use super::*;

    // "A rule more specific than the bare * makes it a line": the first such
    // rule, most specific first, that SAYS line or point. A rule that says
    // neither passes the question on to the next one - but never to "*".
    #[test]
    fn the_first_rule_that_says_line_or_point_decides_and_the_fallback_is_never_asked() {
        let mut map = SurveyMap::default();
        // Every code is a line, says the catch-all.
        add(
            &mut map,
            SurveyRule {
                key: String::from("*"),
                breakline: Some(SurveyBreakline::Line),
                ..SurveyRule::default()
            },
        );
        // Where a mark goes, and nothing about lines.
        add(
            &mut map,
            SurveyRule {
                key: String::from("PS*"),
                model: String::from("SURVEY MARKS"),
                ..SurveyRule::default()
            },
        );
        add(&mut map, line_rule("KJ*", "SURVEY JOINT", ""));
        // The first ten joint strings: a layer, nothing about lines.
        add(
            &mut map,
            SurveyRule {
                key: String::from("KJ0*"),
                model: String::from("SURVEY JOINT EARLY"),
                ..SurveyRule::default()
            },
        );

        let mut document = Document::default();
        document.set_survey_map(map);
        add_point(&mut document, p(0.0, 0.0), "PS1", "1", "");
        add_point(&mut document, p(10.0, 0.0), "PS1", "2", "");
        add_point(&mut document, p(0.0, 5.0), "KJ01", "3", "");
        add_point(&mut document, p(10.0, 5.0), "KJ01", "4", "");

        let options = LineworkOptions {
            only_ruled_lines: true,
            ..LineworkOptions::default()
        };
        let planned = expect_ok(process_linework(&document, &options));

        // KJ01: "KJ0*" says neither, "KJ*" says line - a line, on the layer the
        // more specific rule names.
        assert_eq!(planned.report.strings.len(), 1);
        assert_eq!(planned.report.strings[0].name, "KJ01");
        assert_eq!(planned.report.strings[0].key, "KJ0*");
        assert_eq!(planned.report.strings[0].layer, "SURVEY JOINT EARLY");
        // PS1: "PS*" says neither and only "*" is left, which is not asked: a
        // point code, and not "no rule" either - a rule does know it.
        assert_eq!(planned.report.unplaced.len(), 2);
        assert_eq!(planned.report.unplaced[0].point_number, "1");
        assert_eq!(planned.report.unplaced[0].reason, UnplacedReason::PointCode);
        assert_eq!(planned.report.unplaced[1].reason, UnplacedReason::PointCode);

        // The same two questions asked of features.
        let project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, "PS"),
                point("2", 0.0, 10.0, "PS"),
                point("3", 5.0, 0.0, "KJ"),
                point("4", 5.0, 10.0, "KJ"),
            ],
            features: vec![
                feature("PS", "1", &["1", "2"]),
                feature("KJ", "01", &["3", "4"]),
            ],
            ..SurveyProject::default()
        };
        let ruled = SurveyFeatureOptions {
            only_ruled_lines: true,
            ..SurveyFeatureOptions::default()
        };
        let features = expect_ok(draw_survey_features(&document, &project, &ruled));
        assert_eq!(features.report.strings.len(), 1);
        assert_eq!(features.report.strings[0].name, "KJ 01");
        assert_eq!(features.unplaced.len(), 1);
        assert_eq!(features.unplaced[0].name, "PS 1");
        assert_eq!(
            features.unplaced[0].reason,
            UnplacedFeatureReason::PointCode
        );
    }

    #[test]
    fn a_line_of_points_that_carry_their_string_number_apart_reports_that_number_whole() {
        let mut document = Document::default();
        document.set_survey_map(field_map());
        // Code B in string 12: the key is "B1*", and what that key leaves of
        // the name "B12" is "2" - which is not the string's number.
        add_point(&mut document, p(0.0, 0.0), "B", "1", "12");
        add_point(&mut document, p(10.0, 0.0), "B", "2", "12");

        let planned = expect_ok(process_linework(&document, &LineworkOptions::default()));

        assert_eq!(planned.report.strings.len(), 1);
        assert_eq!(planned.report.strings[0].name, "B12");
        assert_eq!(planned.report.strings[0].key, "B1*");
        assert_eq!(planned.report.strings[0].number, "12");
        assert_eq!(planned.report.strings[0].layer, "SURVEY BARRIER");
    }
}

// ---- lines drawn before ----------------------------------------------------------------

mod linework_earlier_lines {
    use super::*;

    // A line an earlier run drew of the same string is redrawn as the SAME
    // entity: what a re-adjusted survey job's lines rest on.
    #[test]
    fn a_line_of_the_same_string_is_moved_not_replaced_and_the_others_are_left_alone() {
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        add(&mut map, line_rule("KB*", "SURVEY KERB", "Kerb"));
        document.set_survey_map(map);
        let second = {
            add_point(&mut document, p(0.0, 0.0), "KB01", "1", "");
            add_point(&mut document, p(10.0, 0.0), "KB01", "2", "")
        };
        add_point(&mut document, p(0.0, 5.0), "KB02", "3", "");
        let fourth = add_point(&mut document, p(10.0, 5.0), "KB02", "4", "");

        let mut first = expect_ok(process_linework(&document, &LineworkOptions::default()));
        let Some(command) = first.command.take() else {
            panic!("the lines are to be drawn");
        };
        must(&mut document, command);
        let drawn = document.last_created_entities();
        assert_eq!(drawn.len(), 2); // KB01, then KB02
        // The person puts the KB01 line on a layer of their own.
        let own = Layer {
            name: String::from("checked"),
            ..Layer::default()
        };
        must(&mut document, create_layer(own));
        must(&mut document, set_entity_layer(vec![drawn[0]], "checked"));
        let kerb_two_before = document.model().entities.find(drawn[1]).cloned();

        // Point 2 moves to (10, 2), and point 4 goes: KB02 is one point, no
        // line.
        must(
            &mut document,
            move_entities(vec![second], Vec2::new(0.0, 2.0)),
        );
        must(&mut document, delete_entities(vec![fourth]));

        let again = LineworkOptions {
            earlier_lines: drawn.clone(),
            ..LineworkOptions::default()
        };
        let mut planned = expect_ok(process_linework(&document, &again));
        assert_eq!(planned.report.strings.len(), 1);
        assert_eq!(planned.report.strings[0].name, "KB01");
        assert_eq!(planned.report.strings[0].redrawn, drawn[0]);
        assert_eq!(
            planned.report.strings[0].layer, "checked",
            "the layer it is on, not the rule's"
        );
        assert!(planned.report.layers_created.is_empty());
        assert!(
            planned.report.styling.is_none(),
            "no line is created, so none is styled"
        );
        let Some(command) = planned.command.take() else {
            panic!("the line is to be redrawn");
        };
        must(&mut document, command);

        // The same entity, on the person's layer in the style it had, through
        // where the points now are - and nothing was created.
        assert!(document.last_created_entities().is_empty());
        let Some(kerb_one) = document.model().entities.find(drawn[0]) else {
            panic!("KB01 is still there");
        };
        assert_eq!(vertices_of(kerb_one), [p(0.0, 0.0), p(10.0, 2.0)]);
        assert_eq!(kerb_one.layer, "checked");
        assert_eq!(kerb_one.style, "Kerb");
        assert_eq!(lines_of(&document).len(), 2);
        // The KB02 line, which no line planned now is, is not this run's to
        // touch.
        assert_eq!(
            document.model().entities.find(drawn[1]).cloned(),
            kerb_two_before
        );

        // Asked again with nothing moved, the line already runs there: nothing
        // to do, and it is still named as the line of that string.
        let unchanged = expect_ok(process_linework(&document, &again));
        assert!(unchanged.command.is_none());
        assert_eq!(unchanged.report.strings.len(), 1);
        assert_eq!(unchanged.report.strings[0].redrawn, drawn[0]);

        // One undo of the redraw puts the line back where it ran.
        assert!(document.undo().is_ok());
        let Some(kerb_one) = document.model().entities.find(drawn[0]) else {
            panic!("KB01 is still there");
        };
        assert_eq!(vertices_of(kerb_one), [p(0.0, 0.0), p(10.0, 0.0)]);
    }
}
