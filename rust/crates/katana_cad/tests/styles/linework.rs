// Field-to-finish linework: joining coded survey points into lines
// (tests/cad/customisation/test_linework.cpp).
//
// Every expected value here is worked by hand from the points the test
// builds; the working is in the comment beside it.

use katana_cad::linework::{
    LineworkCodes, parse_field_code, process_linework, split_string_name, validate,
};
use katana_cad::{
    Document, LineworkControl, LineworkNote, LineworkNoteKind, LineworkOptions, LineworkOrder,
    LineworkReport, LineworkResult, UnplacedReason,
};
use katana_commands::create_entities;
use katana_core::ErrorCode;
use katana_entity::entity::{INVALID_ENTITY_ID, heights_of, set_heights};
use katana_entity::{
    Entity, EntityId, Geometry, PointGeometry, PropertyValue, SurveyBreakline, SurveyMap,
    SurveyRule,
};
use katana_geometry::{Point2, Polyline2};
use katana_math::numerics::PI;

use crate::{double_eq, near};

// Shaped like the reference mapfile's rules, with invented names: a water
// main and a kerb are lines, a tree is a point.
fn line_and_point_map() -> SurveyMap {
    let mut map = SurveyMap::default();
    let water = SurveyRule {
        key: String::from("WM*"),
        model: String::from("SURVEY SERVICES"),
        colour: String::from("blue"),
        linestyle: String::from("WATR Main"),
        breakline: Some(SurveyBreakline::Line),
        ..SurveyRule::default()
    };
    assert!(map.add(water).is_ok());

    let kerb = SurveyRule {
        key: String::from("KB*"),
        model: String::from("SURVEY KERB"),
        breakline: Some(SurveyBreakline::Line),
        ..SurveyRule::default()
    };
    assert!(map.add(kerb).is_ok());

    let tree = SurveyRule {
        key: String::from("TR*"),
        model: String::from("SURVEY DETAIL"),
        breakline: Some(SurveyBreakline::Point),
        ..SurveyRule::default()
    };
    assert!(map.add(tree).is_ok());
    map
}

fn mapped() -> Document {
    let mut document = Document::default();
    document.set_survey_map(line_and_point_map());
    document
}

// A surveyed point as survey import writes one: its code in "code", its
// number in "point", its height in "elevation" when it has one.
fn add_point(
    document: &mut Document,
    at: Point2,
    code: &str,
    number: &str,
    height: Option<f64>,
) -> EntityId {
    let mut entity = Entity {
        geometry: Geometry::Point(PointGeometry { position: at }),
        ..Entity::default()
    };
    if !code.is_empty() {
        entity
            .properties
            .insert(String::from("code"), PropertyValue::from(code));
    }
    if !number.is_empty() {
        entity
            .properties
            .insert(String::from("point"), PropertyValue::from(number));
    }
    if height.is_some() {
        set_heights(&mut entity.properties, &[height]);
    }
    assert!(document.execute(create_entities(vec![entity])).is_ok());
    let created = document.last_created_entities();
    assert_eq!(created.len(), 1);
    created.first().copied().unwrap_or(INVALID_ENTITY_ID)
}

fn point(document: &mut Document, x: f64, y: f64, code: &str, number: &str) -> EntityId {
    add_point(document, Point2::new(x, y), code, number, None)
}

fn polylines_of(document: &Document) -> Vec<&Entity> {
    let entities = &document.model().entities;
    entities
        .ids()
        .into_iter()
        .filter_map(|id| entities.find(id))
        .filter(|entity| matches!(entity.geometry, Geometry::Polyline(_)))
        .collect()
}

fn shape_of(entity: &Entity) -> &Polyline2 {
    match &entity.geometry {
        Geometry::Polyline(polyline) => polyline,
        _ => panic!("not a polyline"),
    }
}

fn text_property(entity: &Entity, key: &str) -> String {
    match entity.properties.get(key) {
        Some(PropertyValue::Text(text)) => text.clone(),
        _ => String::new(),
    }
}

// Plans and executes, failing the test on an error.
fn run(document: &mut Document, options: &LineworkOptions) -> LineworkResult {
    let mut result = match process_linework(document, options) {
        Ok(result) => result,
        Err(error) => panic!("{}", error.describe()),
    };
    if let Some(command) = result.command.take() {
        assert!(document.execute(command).is_ok());
    }
    result
}

fn run_default(document: &mut Document) -> LineworkReport {
    run(document, &LineworkOptions::default()).report
}

fn has_note(report: &LineworkReport, kind: LineworkNoteKind, detail: &str) -> bool {
    report
        .notes
        .iter()
        .any(|note| note.kind == kind && (detail.is_empty() || note.detail == detail))
}

// The notes of one kind, in the order made.
fn notes_of(report: &LineworkReport, kind: LineworkNoteKind) -> Vec<LineworkNote> {
    report
        .notes
        .iter()
        .filter(|note| note.kind == kind)
        .cloned()
        .collect()
}

fn points(list: &[(f64, f64)]) -> Vec<Point2> {
    list.iter().map(|&(x, y)| Point2::new(x, y)).collect()
}

// ---- the string-name rule -----------------------------------------------------------

mod string_name {
    use super::*;

    #[test]
    fn a_prefix_key_leaves_the_remainder_as_the_string_number() {
        let mut map = SurveyMap::default();
        let wide = SurveyRule {
            key: String::from("W*"),
            model: String::from("WIDE"),
            ..SurveyRule::default()
        };
        assert!(map.add(wide).is_ok());
        let water = SurveyRule {
            key: String::from("WM*"),
            model: String::from("WATER"),
            ..SurveyRule::default()
        };
        assert!(map.add(water).is_ok());

        // WM* (prefix "WM", 2 characters) is longer than W*, so it decides:
        // "WM01" is "WM" + "01".
        let split = split_string_name(&map, "WM01");
        assert!(split.matched);
        assert!(!split.fallback_only);
        assert_eq!(split.key, "WM*");
        assert_eq!(split.number, "01");

        // Only W* matches "WX7": "W" + "X7".
        assert_eq!(split_string_name(&map, "WX7").number, "X7");
    }

    #[test]
    fn a_code_that_is_exactly_its_key_has_no_number_and_so_is_one_string_per_code() {
        let mut map = SurveyMap::default();
        let water = SurveyRule {
            key: String::from("WM*"),
            model: String::from("WATER"),
            ..SurveyRule::default()
        };
        assert!(map.add(water).is_ok());
        let exact = SurveyRule {
            key: String::from("PABB"),
            model: String::from("DETAIL"),
            ..SurveyRule::default()
        };
        assert!(map.add(exact).is_ok());

        let bare = split_string_name(&map, "WM");
        assert_eq!(bare.key, "WM*");
        assert_eq!(bare.number, "", "\"WM\" under WM* leaves nothing over");

        let exact_split = split_string_name(&map, "PABB");
        assert_eq!(exact_split.key, "PABB");
        assert_eq!(exact_split.number, "", "an exact key is the whole code");
    }

    #[test]
    fn a_code_only_the_fallback_matches_is_its_own_key_and_said_to_be_fallback_only() {
        let mut map = SurveyMap::default();
        let anything = SurveyRule {
            key: String::from("*"),
            breakline: Some(SurveyBreakline::Line),
            ..SurveyRule::default()
        };
        assert!(map.add(anything).is_ok());

        let split = split_string_name(&map, "ZZ9");
        assert!(split.matched);
        assert!(split.fallback_only);
        assert_eq!(
            split.key, "ZZ9",
            "not \"*\" with a string number of \"ZZ9\""
        );
        assert_eq!(split.number, "");

        let none = split_string_name(&SurveyMap::default(), "ZZ9");
        assert!(!none.matched);
        assert_eq!(none.key, "");
    }
}

// ---- control codes -----------------------------------------------------------------

mod field_code {
    use super::*;

    #[test]
    fn the_first_token_is_the_name_and_the_rest_are_controls() {
        let codes = LineworkCodes::default();
        let start = parse_field_code("KB1 ST", &codes);
        assert_eq!(start.name, "KB1");
        assert_eq!(start.controls, vec![LineworkControl::Start]);

        let close = parse_field_code("  FL   CL ", &codes);
        assert_eq!(
            close.name, "FL",
            "blanks around and between tokens are not tokens"
        );
        assert_eq!(close.controls, vec![LineworkControl::Close]);

        let named = parse_field_code("CL", &codes);
        assert_eq!(
            named.name, "CL",
            "the first token is the name even spelled like a control"
        );
        assert!(named.controls.is_empty());
    }

    #[test]
    fn controls_are_matched_ignoring_case_but_the_name_keeps_its_case() {
        let parsed = parse_field_code("kb1 st bc", &LineworkCodes::default());
        assert_eq!(parsed.name, "kb1");
        assert_eq!(
            parsed.controls,
            vec![LineworkControl::Start, LineworkControl::ArcStart]
        );
    }

    #[test]
    fn an_unknown_token_is_kept_to_be_reported_not_dropped() {
        let parsed = parse_field_code("KB1 XX END 12", &LineworkCodes::default());
        assert_eq!(parsed.controls, vec![LineworkControl::End]);
        assert_eq!(parsed.unknown_tokens, vec!["XX", "12"]);
    }

    #[test]
    fn a_join_takes_the_point_number_after_it_and_says_when_there_is_none() {
        let joined = parse_field_code("FN3 JPN 105", &LineworkCodes::default());
        assert_eq!(joined.join_to, "105");
        assert!(!joined.join_without_target);
        assert!(
            joined.unknown_tokens.is_empty(),
            "105 is the join's, not unknown"
        );

        let forgotten = parse_field_code("FN3 JPN CL", &LineworkCodes::default());
        assert!(
            forgotten.join_without_target,
            "CL is a control, not a point called CL"
        );
        assert!(forgotten.has(LineworkControl::Close));
    }

    #[test]
    fn the_spellings_are_data_and_an_empty_one_switches_that_control_off() {
        let codes = LineworkCodes {
            start: String::from("B"),
            close: String::new(),
            ..LineworkCodes::default()
        };
        let parsed = parse_field_code("KB1 B CL", &codes);
        assert_eq!(parsed.controls, vec![LineworkControl::Start]);
        assert_eq!(parsed.unknown_tokens, vec!["CL"]);
    }
}

mod linework_codes {
    use super::*;

    #[test]
    fn ambiguous_or_unmatchable_spellings_are_refused() {
        assert!(
            validate(&LineworkCodes::default()).is_ok(),
            "the defaults are sound"
        );

        // "ST" is start, and tokens are matched ignoring case.
        let same = LineworkCodes {
            end: String::from("st"),
            ..LineworkCodes::default()
        };
        assert!(validate(&same).is_err());

        let blank = LineworkCodes {
            close: String::from("C L"),
            ..LineworkCodes::default()
        };
        assert!(validate(&blank).is_err());

        let off = LineworkCodes {
            arc_start: String::new(),
            arc_end: String::new(),
            ..LineworkCodes::default()
        };
        assert!(
            validate(&off).is_ok(),
            "two switched-off controls are not a clash"
        );

        let document = Document::default();
        let options = LineworkOptions {
            codes: same,
            ..LineworkOptions::default()
        };
        assert!(process_linework(&document, &options).is_err());
    }
}

// ---- joining ------------------------------------------------------------------------

mod linework {
    use super::*;

    #[test]
    fn two_strings_of_one_code_make_two_lines_on_the_rules_layer_in_the_style_codes_would_give() {
        let mut document = mapped();
        point(&mut document, 0.0, 0.0, "WM01", "1");
        point(&mut document, 0.0, 10.0, "WM02", "2");
        point(&mut document, 10.0, 0.0, "WM01", "3");
        point(&mut document, 10.0, 10.0, "WM02", "4");
        point(&mut document, 20.0, 0.0, "WM01", "5");

        let report = run_default(&mut document);
        assert_eq!(
            report.property, "code",
            "found, as apply_survey_codes finds it"
        );
        assert_eq!(report.strings.len(), 2);
        // WM01 is points 1, 3, 5; WM02 is 2, 4. Names in order.
        assert_eq!(report.strings[0].name, "WM01");
        assert_eq!(report.strings[0].key, "WM*");
        assert_eq!(report.strings[0].number, "01");
        assert_eq!(report.strings[0].point_numbers, vec!["1", "3", "5"]);
        assert!(!report.strings[0].closed);
        assert_eq!(report.strings[1].name, "WM02");
        assert_eq!(report.strings[1].point_numbers, vec!["2", "4"]);
        assert!(report.unplaced.is_empty());
        assert_eq!(report.layers_created, vec!["SURVEY SERVICES"]);

        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            shape_of(lines[0]).vertices,
            points(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)])
        );
        assert_eq!(
            shape_of(lines[1]).vertices,
            points(&[(0.0, 10.0), (10.0, 10.0)])
        );
        for line in &lines {
            assert_eq!(
                line.layer, "SURVEY SERVICES",
                "a rule's model is Katana's layer"
            );
            // apply_survey_codes names a style after the rule's linestyle.
            assert_eq!(line.style, "WATR Main");
        }
        assert_eq!(
            text_property(lines[0], "code"),
            "WM01",
            "the line carries its name, so codes apply to it again"
        );

        let styling = report.styling_report().expect("styled");
        assert_eq!(styling.matched, 2, "filled when the command ran");
        assert_eq!(styling.styles_created, vec!["WATR Main"]);
    }

    #[test]
    fn styling_touches_only_the_lines_not_the_other_coded_points_in_the_drawing() {
        let mut document = mapped();
        point(&mut document, 0.0, 0.0, "WM01", "1");
        point(&mut document, 10.0, 0.0, "WM01", "2");
        let lone = point(&mut document, 50.0, 50.0, "WM07", "3");

        run_default(&mut document);
        let entity = document.model().entities.find(lone).expect("the point");
        assert_eq!(
            entity.layer, "0",
            "applying codes to the lines is not applying them to all"
        );
        assert_eq!(entity.style, "");
    }

    #[test]
    fn start_and_end_split_one_string_into_several_lines() {
        let mut document = mapped();
        // END on 3 ends the first line; 4 begins another without being told.
        point(&mut document, 0.0, 0.0, "KB1", "1");
        point(&mut document, 1.0, 0.0, "KB1", "2");
        point(&mut document, 2.0, 0.0, "KB1 END", "3");
        point(&mut document, 3.0, 0.0, "KB1", "4");
        point(&mut document, 4.0, 0.0, "KB1", "5");
        // ST on 8 begins a new line, so 6-7 is one and 8-9 another.
        point(&mut document, 0.0, 5.0, "KB2", "6");
        point(&mut document, 1.0, 5.0, "KB2", "7");
        point(&mut document, 2.0, 5.0, "KB2 ST", "8");
        point(&mut document, 3.0, 5.0, "KB2", "9");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 4);
        assert_eq!(report.strings[0].point_numbers, vec!["1", "2", "3"]);
        assert_eq!(report.strings[1].point_numbers, vec!["4", "5"]);
        assert_eq!(report.strings[2].point_numbers, vec!["6", "7"]);
        assert_eq!(report.strings[3].point_numbers, vec!["8", "9"]);
        assert_eq!(polylines_of(&document).len(), 4);
    }

    #[test]
    fn close_closes_the_line_back_to_its_first_point() {
        let mut document = mapped();
        point(&mut document, 0.0, 0.0, "KB1", "1");
        point(&mut document, 10.0, 0.0, "KB1", "2");
        point(&mut document, 10.0, 10.0, "KB1", "3");
        point(&mut document, 0.0, 10.0, "KB1 CL", "4");
        point(&mut document, 50.0, 0.0, "KB1", "5"); // after CL: a new line, alone
        point(&mut document, 60.0, 0.0, "KB1", "6");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 2);
        assert!(report.strings[0].closed);
        assert!(!report.strings[1].closed);
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 2);
        assert!(shape_of(lines[0]).closed);
        // Four vertices; the closing side is the polyline's own, not a fifth.
        assert_eq!(shape_of(lines[0]).vertices.len(), 4);
        // 10 + 10 + 10 + 10 around the square.
        double_eq(shape_of(lines[0]).length(), 40.0);
    }

    #[test]
    fn a_lone_point_makes_no_line_and_is_reported() {
        let mut document = mapped();
        let lone = point(&mut document, 0.0, 0.0, "WM03", "7");

        let planned = process_linework(&document, &LineworkOptions::default()).expect("planned");
        assert!(
            planned.command.is_none(),
            "nothing to build is not an error"
        );
        assert_eq!(planned.report.unplaced.len(), 1);
        assert_eq!(planned.report.unplaced[0].id, lone);
        assert_eq!(planned.report.unplaced[0].reason, UnplacedReason::LonePoint);
        assert_eq!(planned.report.unplaced[0].point_number, "7");
        assert_eq!(planned.report.unplaced[0].code, "WM03");
        // WM* names model SURVEY SERVICES, which the drawing lacks - but no line
        // goes on it, so no command creates it, and the report must not say one
        // does.
        assert!(planned.report.layers_created.is_empty());
    }

    #[test]
    fn point_codes_are_left_alone_and_each_is_reported_with_why() {
        let mut document = mapped();
        let tree1 = point(&mut document, 0.0, 0.0, "TR01", "1");
        let tree2 = point(&mut document, 5.0, 0.0, "TR01", "2");
        let stranger = point(&mut document, 9.0, 0.0, "QQ", "3");
        let uncoded = point(&mut document, 12.0, 0.0, "", "4");

        let planned = process_linework(&document, &LineworkOptions::default()).expect("planned");
        assert!(planned.command.is_none());
        let unplaced = &planned.report.unplaced;
        assert_eq!(unplaced.len(), 4, "every point, in entity order");
        assert_eq!(unplaced[0].id, tree1);
        assert_eq!(unplaced[0].reason, UnplacedReason::PointCode);
        assert_eq!(unplaced[1].id, tree2);
        assert_eq!(unplaced[1].reason, UnplacedReason::PointCode);
        assert_eq!(unplaced[2].id, stranger);
        assert_eq!(unplaced[2].reason, UnplacedReason::NoRule);
        assert_eq!(unplaced[3].id, uncoded);
        assert_eq!(unplaced[3].reason, UnplacedReason::NoCode);
        assert_eq!(planned.report.considered, 4);
    }

    #[test]
    fn a_control_code_makes_a_line_even_without_a_rule_and_says_nothing_styles_it() {
        let mut document = mapped();
        point(&mut document, 0.0, 0.0, "XX ST", "1");
        point(&mut document, 4.0, 0.0, "XX END", "2");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 1);
        assert_eq!(
            report.strings[0].layer, "0",
            "no rule, so the points' own layer"
        );
        assert!(has_note(&report, LineworkNoteKind::NoRuleForName, "XX"));
    }

    #[test]
    fn an_unknown_token_is_reported_and_the_point_still_placed() {
        let mut document = mapped();
        let odd = point(&mut document, 0.0, 0.0, "KB1 ZIG", "1");
        point(&mut document, 4.0, 0.0, "KB1", "2");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 1);
        assert_eq!(report.strings[0].points[0], odd);
        assert_eq!(report.notes.len(), 1);
        assert_eq!(report.notes[0].kind, LineworkNoteKind::UnknownToken);
        assert_eq!(report.notes[0].id, odd);
        assert_eq!(report.notes[0].detail, "ZIG");
    }

    // ---- order --------------------------------------------------------------------------

    #[test]
    fn point_number_order_is_numeric_and_entity_order_is_creation_order() {
        // Created in the order A, B, C, numbered 10, 9, 11.
        let make = || {
            let mut document = mapped();
            point(&mut document, 0.0, 0.0, "KB1", "10"); // A
            point(&mut document, 5.0, 5.0, "KB1", "9"); // B
            point(&mut document, 10.0, 0.0, "KB1", "11"); // C
            document
        };

        let mut by_number = make();
        run_default(&mut by_number);
        let numbered = polylines_of(&by_number);
        assert_eq!(numbered.len(), 1);
        // 9 < 10 < 11 as numbers (as text "10" < "11" < "9"): B, A, C.
        assert_eq!(
            shape_of(numbered[0]).vertices,
            points(&[(5.0, 5.0), (0.0, 0.0), (10.0, 0.0)])
        );

        let mut by_entity = make();
        let options = LineworkOptions {
            order: LineworkOrder::EntityOrder,
            ..LineworkOptions::default()
        };
        run(&mut by_entity, &options);
        let created = polylines_of(&by_entity);
        assert_eq!(created.len(), 1);
        assert_eq!(
            shape_of(created[0]).vertices,
            points(&[(0.0, 0.0), (5.0, 5.0), (10.0, 0.0)])
        );
    }

    #[test]
    fn ordering_by_number_reports_a_point_without_one() {
        let mut document = mapped();
        point(&mut document, 0.0, 0.0, "KB1", "1");
        point(&mut document, 4.0, 0.0, "KB1", "2");
        let nameless = point(&mut document, 8.0, 0.0, "KB1", "");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 1);
        assert_eq!(report.strings[0].points.len(), 2);
        assert_eq!(report.unplaced.len(), 1);
        assert_eq!(report.unplaced[0].id, nameless);
        assert_eq!(report.unplaced[0].reason, UnplacedReason::NoPointNumber);
    }

    #[test]
    fn only_the_selected_entities_are_processed() {
        let mut document = mapped();
        let a = point(&mut document, 0.0, 0.0, "KB1", "1");
        let b = point(&mut document, 4.0, 0.0, "KB1", "2");
        point(&mut document, 8.0, 0.0, "KB1", "3");

        let options = LineworkOptions {
            ids: vec![b, a],
            ..LineworkOptions::default()
        };
        let report = run(&mut document, &options).report;
        assert_eq!(report.strings.len(), 1);
        assert_eq!(report.strings[0].point_numbers, vec!["1", "2"]);
        assert_eq!(report.considered, 2);
    }

    // ---- geometry the model can carry ------------------------------------------------

    #[test]
    fn heights_are_kept_per_vertex_and_a_missing_one_stays_missing() {
        let mut document = mapped();
        add_point(&mut document, Point2::new(0.0, 0.0), "KB1", "1", Some(10.5));
        add_point(&mut document, Point2::new(4.0, 0.0), "KB1", "2", None);
        add_point(
            &mut document,
            Point2::new(8.0, 0.0),
            "KB1",
            "3",
            Some(11.25),
        );

        run_default(&mut document);
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        let heights = heights_of(&lines[0].properties, 3);
        assert_eq!(heights.len(), 3);
        assert_eq!(heights[0], Some(10.5));
        assert_eq!(heights[1], None, "absent is not zero");
        assert_eq!(heights[2], Some(11.25));
    }

    #[test]
    fn a_curve_is_chorded_within_the_tolerance_through_its_surveyed_points() {
        let mut document = mapped();
        // A half circle of radius 10 about the origin, anticlockwise.
        point(&mut document, 10.0, 0.0, "KB1 BC", "1");
        point(&mut document, 0.0, 10.0, "KB1", "2");
        point(&mut document, -10.0, 0.0, "KB1 EC", "3");

        // Largest step t = 2 acos(1 - 0.35/10) = 2 acos(0.965) = 2 x 15.2 deg
        // = 30.4 deg. Each quarter (90 deg) needs ceil(90 / 30.4) = 3 chords of
        // 30 deg, whose sagitta is 10 (1 - cos 15 deg) = 0.341, within 0.35.
        let options = LineworkOptions {
            chord_tolerance: 0.35,
            ..LineworkOptions::default()
        };
        let report = run(&mut document, &options).report;
        assert_eq!(report.strings.len(), 1);
        assert_eq!(report.strings[0].curves, 1);
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        let vertices = &shape_of(lines[0]).vertices;
        // 1 + 3 + 3: vertices at 0, 30, 60, ... 180 degrees.
        assert_eq!(vertices.len(), 7);
        for (i, vertex) in vertices.iter().enumerate() {
            let angle = i as f64 * PI / 6.0;
            near(vertex.x, 10.0 * angle.cos(), 1e-9);
            near(vertex.y, 10.0 * angle.sin(), 1e-9);
        }
        assert_eq!(
            vertices[3],
            Point2::new(0.0, 10.0),
            "the surveyed point is a vertex, exactly"
        );
    }

    #[test]
    fn the_default_tolerance_chords_a_quarter_of_a_ten_metre_radius_into_twenty_five() {
        let mut document = mapped();
        point(&mut document, 10.0, 0.0, "KB1 BC", "1");
        point(&mut document, 0.0, 10.0, "KB1", "2");
        point(&mut document, -10.0, 0.0, "KB1 EC", "3");

        run_default(&mut document);
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        // t = 2 acos(1 - 0.005/10) = 2 acos(0.9995) = 2 x 0.031624 = 0.063248;
        // a quarter is 1.570796 / 0.063248 = 24.84, so 25 chords: 1 + 25 + 25.
        let vertices = &shape_of(lines[0]).vertices;
        assert_eq!(vertices.len(), 51);
        // Each chord spans 90/25 = 3.6 deg; its sagitta 10 (1 - cos 1.8 deg)
        // = 0.00493, within 5 mm.
        for (i, pair) in vertices.windows(2).enumerate() {
            let middle = (pair[0] + pair[1]) * 0.5;
            assert!(10.0 - middle.length() <= 0.005, "chord {}", i + 1);
        }
    }

    #[test]
    fn a_curve_with_nothing_between_its_ends_is_drawn_straight_and_said_so() {
        let mut document = mapped();
        point(&mut document, 0.0, 0.0, "KB1 BC", "1");
        point(&mut document, 10.0, 0.0, "KB1 EC", "2");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 1);
        assert_eq!(report.strings[0].curves, 0);
        assert_eq!(report.strings[0].vertices, 2);
        assert!(has_note(&report, LineworkNoteKind::CurveTooShort, ""));
    }

    #[test]
    fn three_points_and_rectangle_make_the_rectangle_they_describe() {
        let mut document = mapped();
        add_point(&mut document, Point2::new(0.0, 0.0), "KB1", "1", Some(1.0));
        add_point(&mut document, Point2::new(10.0, 0.0), "KB1", "2", Some(2.0));
        add_point(
            &mut document,
            Point2::new(9.0, 4.0),
            "KB1 RECT",
            "3",
            Some(3.0),
        );

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 1);
        assert!(report.strings[0].rectangle);
        assert!(report.strings[0].closed);
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        // Side (0,0)-(10,0); its left normal is (0,1) and (9,4) is 4 along it.
        assert_eq!(
            shape_of(lines[0]).vertices,
            points(&[(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (0.0, 4.0)])
        );
        assert!(shape_of(lines[0]).closed);
        let heights = heights_of(&lines[0].properties, 4);
        assert_eq!(
            heights,
            vec![Some(1.0), Some(2.0), None, None],
            "the constructed corners were not surveyed"
        );
    }

    #[test]
    fn a_join_draws_a_line_to_the_numbered_point_and_places_it() {
        let mut document = mapped();
        point(&mut document, 0.0, 0.0, "KB1", "101");
        point(&mut document, 10.0, 0.0, "KB1 JPN 201", "102");
        let tree = point(&mut document, 10.0, 5.0, "TR01", "201");
        point(&mut document, 20.0, 0.0, "KB1 JPN 999", "103");

        let report = run_default(&mut document);
        // KB1 101-102-103 is one line; 102 JPN 201 is a second.
        assert_eq!(report.strings.len(), 2);
        assert!(!report.strings[0].join);
        assert!(report.strings[1].join);
        assert_eq!(report.strings[1].point_numbers, vec!["102", "201"]);
        assert!(has_note(
            &report,
            LineworkNoteKind::JoinTargetMissing,
            "999"
        ));
        for unplaced in &report.unplaced {
            assert_ne!(unplaced.id, tree, "a point a join reached is in a line");
        }
    }

    #[test]
    fn a_join_with_no_point_number_is_reported_and_the_point_still_placed() {
        let mut document = mapped();
        let forgot = point(&mut document, 0.0, 0.0, "KB1 JPN", "1");
        point(&mut document, 10.0, 0.0, "KB1", "2");

        let report = run_default(&mut document);
        // KB1 1-2 is the one line; the join has no target, so no second line.
        assert_eq!(report.strings.len(), 1);
        assert!(!report.strings[0].join);
        assert_eq!(report.strings[0].points[0], forgot);
        let notes = notes_of(&report, LineworkNoteKind::JoinWithoutTarget);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, forgot);
        assert_eq!(notes[0].point_number, "1");
    }

    #[test]
    fn with_keep_points_off_a_join_target_no_run_placed_is_kept() {
        let mut document = mapped();
        let first = point(&mut document, 0.0, 0.0, "WM01", "1");
        let second = point(&mut document, 10.0, 0.0, "WM01 JPN 3", "2");
        let tree = point(&mut document, 10.0, 5.0, "TR01", "3");
        let uncoded = point(&mut document, 20.0, 5.0, "", "4");
        let third = point(&mut document, 20.0, 0.0, "WM01 JPN 4", "5");

        let options = LineworkOptions {
            keep_points: false,
            ..LineworkOptions::default()
        };
        let report = run(&mut document, &options).report;
        // WM01 is the run 1-2-5; 2 JPN 3 and 5 JPN 4 are two joins: three
        // lines.
        assert_eq!(report.strings.len(), 3);
        // Only the run's points 1, 2 and 5 are replaced by lines. The tree (a
        // point code) and the uncoded point were only reached by joins: no line
        // of their own stands for them, so deleting them would lose a tree and
        // an uncoded point outright.
        assert_eq!(report.points_removed, 3);
        let entities = &document.model().entities;
        assert!(entities.find(first).is_none());
        assert!(entities.find(second).is_none());
        assert!(entities.find(third).is_none());
        let kept = entities
            .find(tree)
            .expect("a point-coded join target is not the command's to delete");
        assert_eq!(text_property(kept, "code"), "TR01");
        assert_eq!(text_property(kept, "point"), "3");
        assert!(entities.find(uncoded).is_some());
        // Two points and three lines.
        assert_eq!(entities.size(), 5);
    }

    // ---- what could not be drawn as coded ----------------------------------------------

    #[test]
    fn closing_a_string_of_two_points_draws_it_open_and_says_so() {
        let mut document = mapped();
        point(&mut document, 0.0, 0.0, "KB1", "1");
        let closer = point(&mut document, 10.0, 0.0, "KB1 CL", "2");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 1);
        // Closed, 1-2-1 would run back over itself and enclose nothing.
        assert!(!report.strings[0].closed);
        assert_eq!(report.strings[0].vertices, 2);
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        assert!(!shape_of(lines[0]).closed);
        let notes = notes_of(&report, LineworkNoteKind::CloseTooShort);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, closer, "on the point that asked to close");
    }

    #[test]
    fn a_curve_through_points_in_a_straight_line_is_drawn_straight_and_not_counted_as_a_curve() {
        let mut document = mapped();
        let begin = point(&mut document, 0.0, 0.0, "KB1 BC", "1");
        point(&mut document, 5.0, 0.0, "KB1", "2");
        point(&mut document, 10.0, 0.0, "KB1 EC", "3");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 1);
        // (0,0), (5,0), (10,0) are on no circle: the three points are joined
        // straight and nothing is chorded, so there is no curve to count.
        assert_eq!(report.strings[0].curves, 0);
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            shape_of(lines[0]).vertices,
            points(&[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)])
        );
        let notes = notes_of(&report, LineworkNoteKind::CurveCollinear);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, begin);
    }

    #[test]
    fn a_curve_end_with_no_curve_begun_is_reported_and_the_line_drawn_straight() {
        let mut document = mapped();
        let ender = point(&mut document, 0.0, 0.0, "KB1 EC", "1");
        point(&mut document, 10.0, 0.0, "KB1", "2");
        point(&mut document, 10.0, 10.0, "KB1", "3");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 1);
        assert_eq!(report.strings[0].curves, 0);
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            shape_of(lines[0]).vertices,
            points(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)])
        );
        let notes = notes_of(&report, LineworkNoteKind::CurveEndWithoutStart);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, ender);
    }

    #[test]
    fn a_curve_never_ended_is_curved_to_the_last_point_and_the_note_says_what_was_done() {
        let mut document = mapped();
        // The half circle of radius 10 of the chording test, with its EC
        // forgotten.
        let begin = point(&mut document, 10.0, 0.0, "KB1 BC", "1");
        point(&mut document, 0.0, 10.0, "KB1", "2");
        point(&mut document, -10.0, 0.0, "KB1", "3");
        // And a curve begun on a string's last point: there is nothing after
        // it to curve through.
        point(&mut document, 0.0, 20.0, "KB2", "4");
        let late = point(&mut document, 10.0, 20.0, "KB2 BC", "5");

        let options = LineworkOptions {
            chord_tolerance: 0.35, // 30 degree chords, as worked in that test
            ..LineworkOptions::default()
        };
        let report = run(&mut document, &options).report;
        assert_eq!(report.strings.len(), 2);
        // KB1: curved to its last point as if EC were on it - 1 + 3 + 3
        // vertices.
        assert_eq!(report.strings[0].curves, 1);
        assert_eq!(report.strings[0].vertices, 7);
        // KB2: 4-5 straight, no curve.
        assert_eq!(report.strings[1].curves, 0);
        assert_eq!(report.strings[1].vertices, 2);

        let notes = notes_of(&report, LineworkNoteKind::CurveUnterminated);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].id, begin);
        assert_eq!(notes[0].detail, "taken to end at the string's last point");
        assert_eq!(notes[1].id, late);
        assert_eq!(
            notes[1].detail,
            "begun on the string's last point: nothing curved"
        );
    }

    #[test]
    fn rectangle_on_other_than_three_points_with_width_says_how_the_points_were_drawn_instead() {
        let mut document = mapped();
        // KB1: four points - no rectangle to construct, drawn closed through
        // them.
        point(&mut document, 0.0, 0.0, "KB1", "1");
        point(&mut document, 10.0, 0.0, "KB1", "2");
        point(&mut document, 10.0, 5.0, "KB1", "3");
        let four = point(&mut document, 0.0, 5.0, "KB1 RECT", "4");
        // KB2: two points - drawn open, since two points close on nothing.
        point(&mut document, 0.0, 20.0, "KB2", "5");
        let two = point(&mut document, 10.0, 20.0, "KB2 RECT", "6");
        // KB3: three points, the third on the first side's line - no width, so
        // drawn open. Side (0,40)-(10,40) has unit normal (0,1), and (5,40) is
        // (0,1).(5,0) = 0 from it.
        point(&mut document, 0.0, 40.0, "KB3", "7");
        point(&mut document, 10.0, 40.0, "KB3", "8");
        let flat = point(&mut document, 5.0, 40.0, "KB3 RECT", "9");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 3);
        for built in &report.strings {
            assert!(!built.rectangle, "{}", built.name);
        }
        assert!(report.strings[0].closed);
        assert_eq!(report.strings[0].vertices, 4);
        assert!(!report.strings[1].closed);
        assert_eq!(report.strings[1].vertices, 2);
        assert!(!report.strings[2].closed);
        assert_eq!(report.strings[2].vertices, 3);
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 3);
        assert!(
            shape_of(lines[0]).closed,
            "the note must say what was drawn"
        );
        assert!(!shape_of(lines[1]).closed);
        assert!(!shape_of(lines[2]).closed);

        let notes = notes_of(&report, LineworkNoteKind::RectangleShape);
        assert_eq!(notes.len(), 3);
        assert_eq!(notes[0].id, four);
        assert_eq!(notes[0].detail, "4 points: drawn closed through them");
        assert_eq!(notes[1].id, two);
        assert_eq!(notes[1].detail, "2 points: drawn open");
        assert_eq!(notes[2].id, flat);
        assert_eq!(notes[2].detail, "no width: drawn open");
        assert!(
            !has_note(&report, LineworkNoteKind::CloseTooShort, ""),
            "RECT on two points was never asked to close"
        );
    }

    #[test]
    fn two_points_of_one_string_with_one_number_are_both_placed_and_reported() {
        let mut document = mapped();
        point(&mut document, 0.0, 0.0, "KB1", "1");
        let again = point(&mut document, 5.0, 0.0, "KB1", "1");
        point(&mut document, 10.0, 0.0, "KB1", "2");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 1);
        // Equal numbers keep entity order: (0,0), (5,0), then 2 at (10,0).
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            shape_of(lines[0]).vertices,
            points(&[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)])
        );
        let notes = notes_of(&report, LineworkNoteKind::DuplicatePointNumber);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, again, "on the second of the two");
        assert_eq!(notes[0].detail, "1");
    }

    #[test]
    fn entities_asked_about_that_are_not_points_are_counted_not_processed() {
        let mut document = mapped();
        let a = point(&mut document, 0.0, 0.0, "KB1", "1");
        let b = point(&mut document, 10.0, 0.0, "KB1", "2");
        let mut other = Entity {
            geometry: Geometry::Polyline(Polyline2::new(
                points(&[(0.0, 50.0), (10.0, 50.0)]),
                false,
            )),
            ..Entity::default()
        };
        other
            .properties
            .insert(String::from("code"), PropertyValue::from("KB9"));
        assert!(document.execute(create_entities(vec![other])).is_ok());
        let line = document.last_created_entities()[0];

        let options = LineworkOptions {
            ids: vec![a, line, b],
            ..LineworkOptions::default()
        };
        let report = run(&mut document, &options).report;
        assert_eq!(report.considered, 2, "the two points");
        assert_eq!(report.not_points, 1, "the polyline");
        assert_eq!(report.strings.len(), 1);
        assert_eq!(report.strings[0].name, "KB1");
        assert!(
            report.unplaced.is_empty(),
            "a polyline is not an unplaced point"
        );
        let untouched = document.model().entities.find(line).expect("the polyline");
        assert_eq!(
            untouched.layer, "0",
            "not a point, so not styled as a line here"
        );
    }

    // ---- refusals -----------------------------------------------------------------------

    #[test]
    fn a_chord_tolerance_that_is_not_a_positive_finite_distance_is_refused() {
        let mut document = mapped();
        point(&mut document, 0.0, 0.0, "KB1", "1");
        point(&mut document, 10.0, 0.0, "KB1", "2");

        // Zero would make the chord step zero and the chord count infinite; a
        // negative or NaN one measures nothing; an infinite one allows no
        // chords.
        for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let options = LineworkOptions {
                chord_tolerance: tolerance,
                ..LineworkOptions::default()
            };
            let refused = process_linework(&document, &options)
                .expect_err("a tolerance that is not a positive finite distance");
            assert_eq!(
                refused.code,
                ErrorCode::InvalidArgument,
                "tolerance {tolerance}"
            );
        }
        assert!(polylines_of(&document).is_empty());
    }

    #[test]
    fn a_rule_model_that_is_not_a_layer_name_is_refused_not_drawn_on_another_layer() {
        // The survey map itself now refuses a rule whose model is not a layer
        // path (SurveyMap::add validates it), so such a rule can never reach
        // linework. process_linework's own check stays behind as a second
        // guard; what this pins is the property: the kerb is not drawn
        // anywhere.
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        let kerb = SurveyRule {
            key: String::from("KB*"),
            model: String::from("a//b"), // an empty level: validate_layer_path refuses it
            breakline: Some(SurveyBreakline::Line),
            ..SurveyRule::default()
        };
        let added = map
            .add(kerb)
            .expect_err("the map takes no rule it could not draw on a layer");
        assert_eq!(added.code, ErrorCode::InvalidArgument);
        document.set_survey_map(map);
        point(&mut document, 0.0, 0.0, "KB1", "1");
        point(&mut document, 10.0, 0.0, "KB1", "2");

        let result = run(&mut document, &LineworkOptions::default());
        assert!(result.command.is_none(), "no rule, so no line on any layer");
        assert_eq!(result.report.unplaced.len(), 2);
        assert_eq!(result.report.unplaced[0].reason, UnplacedReason::NoRule);
        assert_eq!(result.report.unplaced[1].reason, UnplacedReason::NoRule);
        assert!(polylines_of(&document).is_empty());
    }

    // ---- one command --------------------------------------------------------------------

    #[test]
    fn one_undo_removes_the_lines_their_layer_their_style_and_restores_the_points() {
        let mut document = mapped();
        point(&mut document, 0.0, 0.0, "WM01", "1");
        point(&mut document, 10.0, 0.0, "WM01", "2");
        point(&mut document, 20.0, 0.0, "WM01", "3");
        let before = document.model().entities.ids();

        let options = LineworkOptions {
            keep_points: false,
            ..LineworkOptions::default()
        };
        let report = run(&mut document, &options).report;
        assert_eq!(report.points_removed, 3);
        assert_eq!(
            document.model().entities.size(),
            1,
            "the line replaced its points"
        );
        let line = document.model().entities.ids()[0];
        assert!(document.model().layers.contains("SURVEY SERVICES"));
        assert!(document.model().styles.contains("WATR Main"));

        assert!(document.undo().is_ok());
        assert_eq!(
            document.model().entities.ids(),
            before,
            "the same points, the same ids"
        );
        assert!(!document.model().layers.contains("SURVEY SERVICES"));
        assert!(!document.model().styles.contains("WATR Main"));
        assert!(
            !(document.history().can_undo()
                && document.history().undo_name() == "PROCESS_LINEWORK")
        );

        assert!(document.redo().is_ok());
        assert_eq!(document.model().entities.size(), 1);
        let redone = document
            .model()
            .entities
            .find(line)
            .expect("redo brings the line back under its id");
        assert_eq!(redone.style, "WATR Main");
    }

    #[test]
    fn nothing_to_do_is_no_command_and_no_error() {
        let empty = Document::default();
        let planned = process_linework(&empty, &LineworkOptions::default()).expect("planned");
        assert!(planned.command.is_none());
        assert!(planned.report.strings.is_empty());
        assert_eq!(planned.report.considered, 0);
    }

    #[test]
    fn the_command_refuses_to_run_on_another_document() {
        let mut planned = mapped();
        point(&mut planned, 0.0, 0.0, "WM01", "1");
        point(&mut planned, 10.0, 0.0, "WM01", "2");
        let mut result = process_linework(&planned, &LineworkOptions::default()).expect("planned");
        let command = result.command.take().expect("a command");

        let mut other = Document::default();
        assert!(other.execute(command).is_err());
        assert!(polylines_of(&other).is_empty(), "and leaves nothing behind");
        assert!(!other.model().layers.contains("SURVEY SERVICES"));
    }

    #[test]
    fn a_line_only_the_fallback_rule_makes_is_reported_as_such() {
        let mut document = Document::default();
        let mut map = SurveyMap::default();
        let anything = SurveyRule {
            key: String::from("*"),
            breakline: Some(SurveyBreakline::Line),
            ..SurveyRule::default()
        };
        assert!(map.add(anything).is_ok());
        document.set_survey_map(map);
        point(&mut document, 0.0, 0.0, "ZZ9", "1");
        point(&mut document, 4.0, 0.0, "ZZ9", "2");

        let report = run_default(&mut document);
        assert_eq!(report.strings.len(), 1);
        assert_eq!(report.strings[0].key, "ZZ9");
        assert!(has_note(&report, LineworkNoteKind::FallbackOnlyName, "ZZ9"));
    }

    #[test]
    fn points_in_one_place_make_no_line_and_are_reported() {
        let mut document = mapped();
        let a = point(&mut document, 3.0, 3.0, "KB1", "1");
        let b = point(&mut document, 3.0, 3.0, "KB1", "2");

        let planned = process_linework(&document, &LineworkOptions::default()).expect("planned");
        assert!(planned.command.is_none());
        assert_eq!(planned.report.unplaced.len(), 2);
        assert_eq!(planned.report.unplaced[0].id, a);
        assert_eq!(
            planned.report.unplaced[0].reason,
            UnplacedReason::Coincident
        );
        assert_eq!(planned.report.unplaced[1].id, b);
        assert_eq!(planned.report.unplaced[1].code, "KB1");
    }
}
