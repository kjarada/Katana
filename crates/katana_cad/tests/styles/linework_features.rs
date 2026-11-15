// Drawing the coded strings a field file strings itself (the survey's
// SurveyFeature; tests/cad/customisation/test_linework_features.cpp).
//
// Every expected value here is worked by hand from the project the test
// builds; the working is in the comment beside it.

use katana_cad::linework::draw_survey_features;
use katana_cad::{
    Document, LineworkNoteKind, SurveyFeatureOptions, SurveyFeatureResult, UnplacedFeatureReason,
};
use katana_commands::create_layer;
use katana_core::ErrorCode;
use katana_entity::entity::heights_of;
use katana_entity::{
    Entity, Geometry, Layer, PropertyValue, SurveyBreakline, SurveyMap, SurveyRule,
};
use katana_geometry::{Point2, Polyline2};
use katana_survey::data_model::{
    SurveyFeature, SurveyPoint, SurveyProject, UnpositionedPoint, validate_project,
};

// Shaped like the reference mapfile's rules, with invented names: a water
// main and a kerb are lines.
fn line_map() -> SurveyMap {
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
    map
}

fn mapped() -> Document {
    let mut document = Document::default();
    document.set_survey_map(line_map());
    document
}

fn point(id: &str, northing: f64, easting: f64, elevation: Option<f64>, code: &str) -> SurveyPoint {
    SurveyPoint {
        id: String::from(id),
        northing,
        easting,
        elevation,
        code: String::from(code),
        ..SurveyPoint::default()
    }
}

fn plain(id: &str, northing: f64, easting: f64) -> SurveyPoint {
    point(id, northing, easting, None, "")
}

fn feature(name: &str, code: &str, ids: &[&str], closed: bool) -> SurveyFeature {
    SurveyFeature {
        name: String::from(name),
        code: String::from(code),
        point_ids: ids.iter().map(|id| String::from(*id)).collect(),
        closed,
        ..SurveyFeature::default()
    }
}

fn nowhere(id: &str) -> UnpositionedPoint {
    UnpositionedPoint {
        id: String::from(id),
        ..UnpositionedPoint::default()
    }
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

// Plans and executes, failing the test on an error.
fn run(
    document: &mut Document,
    project: &SurveyProject,
    options: &SurveyFeatureOptions,
) -> SurveyFeatureResult {
    let mut result = match draw_survey_features(document, project, options) {
        Ok(result) => result,
        Err(error) => panic!("{}", error.describe()),
    };
    if let Some(command) = result.command.take() {
        assert!(document.execute(command).is_ok());
    }
    result
}

fn text_property(entity: &Entity, key: &str) -> String {
    match entity.properties.get(key) {
        None => String::from("<absent>"),
        Some(PropertyValue::Text(text)) => text.clone(),
        Some(_) => String::from("<not text>"),
    }
}

mod survey_features {
    use super::*;

    #[test]
    fn a_feature_is_drawn_through_the_projects_points_in_the_files_order_on_the_rules_layer_in_its_style()
     {
        let mut document = mapped();
        let mut main = feature("WM01", "WM", &["3", "1", "2"], false);
        main.description = String::from("water main");
        let project = SurveyProject {
            points: vec![
                point("1", 100.0, 200.0, Some(10.0), ""),
                plain("2", 100.0, 210.0),
                point("3", 110.0, 210.0, Some(12.0), ""),
            ],
            features: vec![main],
            ..SurveyProject::default()
        };

        let result = run(&mut document, &project, &SurveyFeatureOptions::default());

        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        let line = lines[0];
        // In the file's order 3, 1, 2, each at (easting, northing):
        // 3 = (210, 110), 1 = (200, 100), 2 = (210, 100).
        let shape = shape_of(line);
        assert!(!shape.closed);
        assert_eq!(shape.vertices.len(), 3);
        assert_eq!(shape.vertices[0], Point2::new(210.0, 110.0));
        assert_eq!(shape.vertices[1], Point2::new(200.0, 100.0));
        assert_eq!(shape.vertices[2], Point2::new(210.0, 100.0));
        // Heights in the same order: 3 has 12, 1 has 10, 2 has none.
        let heights = heights_of(&line.properties, 3);
        assert_eq!(heights.len(), 3);
        assert_eq!(heights[0], Some(12.0));
        assert_eq!(heights[1], Some(10.0));
        assert_eq!(heights[2], None, "absent is not zero");

        // WM -> rule WM*: model SURVEY SERVICES, linestyle WATR Main.
        assert_eq!(line.layer, "SURVEY SERVICES");
        assert_eq!(line.style, "WATR Main");
        assert_eq!(
            text_property(line, "code"),
            "WM",
            "the line carries the code the rules key on"
        );
        assert_eq!(text_property(line, "description"), "water main");

        assert_eq!(result.report.strings.len(), 1);
        let built = &result.report.strings[0];
        assert_eq!(built.name, "WM01");
        // The CODE is split: "WM" under "WM*" is the bare prefix, so no number.
        assert_eq!(built.key, "WM*");
        assert_eq!(built.number, "");
        assert!(
            built.points.is_empty(),
            "the points are the project's, not entities"
        );
        assert_eq!(built.point_numbers, vec!["3", "1", "2"]);
        assert_eq!(built.vertices, 3);
        assert_eq!(result.report.layers_created, vec!["SURVEY SERVICES"]);
        assert!(result.unplaced.is_empty());
        assert!(
            result.report.styling.is_some(),
            "styled when the command ran"
        );
    }

    #[test]
    fn a_closed_feature_closes_and_a_point_with_no_position_is_left_out_and_said_so() {
        let mut document = mapped();
        let project = SurveyProject {
            points: vec![
                plain("1", 0.0, 0.0),
                plain("2", 0.0, 10.0),
                plain("3", 10.0, 10.0),
            ],
            unpositioned_points: vec![nowhere("U")],
            features: vec![feature("", "KB", &["1", "U", "2", "3"], true)],
            ..SurveyProject::default()
        };

        let result = run(&mut document, &project, &SurveyFeatureOptions::default());

        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        let shape = shape_of(lines[0]);
        // U has nowhere to be, so 1, 2, 3 at (easting, northing), closed.
        assert!(shape.closed);
        assert_eq!(
            shape.vertices,
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 10.0)
            ]
        );
        assert_eq!(lines[0].layer, "SURVEY KERB");

        assert_eq!(result.report.strings.len(), 1);
        assert_eq!(
            result.report.strings[0].name, "KB",
            "no name: the code names it"
        );
        assert!(result.report.strings[0].closed);
        assert_eq!(result.report.strings[0].point_numbers, vec!["1", "2", "3"]);

        assert_eq!(result.report.notes.len(), 1);
        assert_eq!(
            result.report.notes[0].kind,
            LineworkNoteKind::UnpositionedPoint
        );
        assert_eq!(result.report.notes[0].point_number, "U");
        assert_eq!(result.report.notes[0].detail, "KB");
    }

    #[test]
    fn a_feature_that_cannot_make_a_line_is_reported_with_why_and_no_command_is_made() {
        let document = mapped();
        // [0]: 1 and U - one positioned point, no line.
        // [1]: 4 and 5 - both at (5, 5), no length.
        let project = SurveyProject {
            points: vec![
                plain("1", 0.0, 0.0),
                plain("4", 5.0, 5.0),
                plain("5", 5.0, 5.0),
            ],
            unpositioned_points: vec![nowhere("U")],
            features: vec![
                feature("K1", "KB", &["1", "U"], false),
                feature("K2", "KB", &["4", "5"], false),
            ],
            ..SurveyProject::default()
        };

        let planned = draw_survey_features(&document, &project, &SurveyFeatureOptions::default())
            .expect("planned");
        assert!(planned.command.is_none());
        assert!(planned.report.strings.is_empty());
        assert!(
            planned.report.layers_created.is_empty(),
            "no line, so no layer for one"
        );
        assert_eq!(planned.unplaced.len(), 2);
        assert_eq!(planned.unplaced[0].index, 0);
        assert_eq!(planned.unplaced[0].name, "K1");
        assert_eq!(
            planned.unplaced[0].reason,
            UnplacedFeatureReason::TooFewPoints
        );
        assert_eq!(planned.unplaced[1].index, 1);
        assert_eq!(planned.unplaced[1].code, "KB");
        assert_eq!(
            planned.unplaced[1].reason,
            UnplacedFeatureReason::Coincident
        );
    }

    #[test]
    fn a_code_no_rule_knows_goes_on_its_points_layer_and_is_said_to_be_unstyled() {
        let mut document = mapped();
        let project = SurveyProject {
            points: vec![
                point("1", 0.0, 0.0, None, "ZZ"),
                point("2", 0.0, 10.0, None, "ZZ"),
            ],
            features: vec![feature("fence", "ZZ", &["1", "2"], false)],
            ..SurveyProject::default()
        };
        let mut options = SurveyFeatureOptions::default();
        options.import.layer_per_code = true;

        let result = run(&mut document, &project, &options);

        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        // No rule, so no model: the layer the import gives point 1, which with
        // a layer per code is survey/points + its code ZZ.
        assert_eq!(lines[0].layer, "survey/points/ZZ");
        assert_eq!(result.report.layers_created, vec!["survey/points/ZZ"]);
        assert_eq!(result.report.notes.len(), 1);
        assert_eq!(result.report.notes[0].kind, LineworkNoteKind::NoRuleForName);
        assert_eq!(result.report.notes[0].detail, "ZZ");
        assert_eq!(
            result.report.strings[0].key, "",
            "no rule matched, so no key"
        );
    }

    #[test]
    fn an_import_that_wrote_no_code_still_gives_the_line_its_code_under_the_first_candidate_name() {
        let mut document = mapped();
        let project = SurveyProject {
            points: vec![plain("1", 0.0, 0.0), plain("2", 0.0, 10.0)],
            features: vec![feature("WM01", "WM", &["1", "2"], false)],
            ..SurveyProject::default()
        };
        let mut options = SurveyFeatureOptions::default();
        // The import wrote no code onto its points; the line still needs one,
        // because the rules style it by the code it carries.
        options.import.code_property = String::new();

        let result = run(&mut document, &project, &options);

        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        let line = lines[0];
        assert!(
            !line.properties.contains_key(""),
            "a property with no name is not a property"
        );
        // code_property_candidates()[0] is "code", as process_linework reads
        // when no point carries a candidate.
        assert_eq!(result.report.property, "code");
        assert_eq!(text_property(line, "code"), "WM");
        // So the rules reach it: WM -> WM*, linestyle WATR Main.
        assert_eq!(line.style, "WATR Main");
        let styling = result.report.styling_report().expect("styled");
        assert_eq!(styling.matched, 1);
    }

    #[test]
    fn when_layers_may_not_be_created_a_missing_one_is_refused_and_an_existing_one_used() {
        let mut document = mapped();
        let project = SurveyProject {
            points: vec![plain("1", 0.0, 0.0), plain("2", 0.0, 10.0)],
            features: vec![feature("WM01", "WM", &["1", "2"], false)],
            ..SurveyProject::default()
        };
        let mut options = SurveyFeatureOptions::default();
        options.coding.create_layers = false;

        // Neither SURVEY SERVICES (the rule's) nor survey/points (the points')
        // exists.
        let refused = draw_survey_features(&document, &project, &options)
            .expect_err("no layer may be created");
        assert_eq!(refused.code, ErrorCode::NotFound);

        // With the points' layer there, the line goes on it rather than being
        // refused.
        let points_layer = Layer {
            name: String::from("survey/points"),
            ..Layer::default()
        };
        assert!(document.execute(create_layer(points_layer)).is_ok());
        let result = run(&mut document, &project, &options);
        let lines = polylines_of(&document);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].layer, "survey/points");
        assert!(result.report.layers_created.is_empty());
        assert!(!document.model().layers.contains("SURVEY SERVICES"));
    }

    #[test]
    fn a_rule_model_that_is_not_a_layer_name_is_refused() {
        // Refused where it enters: SurveyMap::add validates the model as a
        // layer path, so draw_survey_features can never be handed a rule naming
        // "a//b". Its own check of the rule's layer stays as a second guard.
        let mut map = SurveyMap::default();
        let kerb = SurveyRule {
            key: String::from("KB*"),
            model: String::from("a//b"), // an empty level: validate_layer_path refuses it
            breakline: Some(SurveyBreakline::Line),
            ..SurveyRule::default()
        };
        let added = map.add(kerb).expect_err("refused");
        assert_eq!(added.code, ErrorCode::InvalidArgument);
        assert!(map.empty());
    }

    #[test]
    fn an_inconsistent_project_is_refused_with_the_validators_error() {
        let document = Document::default();
        let project = SurveyProject {
            points: vec![plain("1", 0.0, 0.0)],
            features: vec![feature("K1", "KB", &["1", "9"], false)], // 9 is in neither list
            ..SurveyProject::default()
        };

        let expected = validate_project(&project).expect_err("inconsistent");
        let planned = draw_survey_features(&document, &project, &SurveyFeatureOptions::default())
            .expect_err("refused");
        assert_eq!(planned.code, expected.code);
        assert!(document.model().entities.empty());
    }

    #[test]
    fn one_undo_removes_every_feature_line_and_the_layers_they_made() {
        let mut document = mapped();
        let project = SurveyProject {
            points: vec![
                plain("1", 0.0, 0.0),
                plain("2", 0.0, 10.0),
                plain("3", 5.0, 0.0),
                plain("4", 5.0, 10.0),
            ],
            features: vec![
                feature("WM01", "WM", &["1", "2"], false),
                feature("KB1", "KB", &["3", "4"], false),
            ],
            ..SurveyProject::default()
        };

        let result = run(&mut document, &project, &SurveyFeatureOptions::default());
        assert_eq!(polylines_of(&document).len(), 2);
        assert_eq!(
            result.report.layers_created,
            vec!["SURVEY KERB", "SURVEY SERVICES"]
        );

        assert!(document.undo().is_ok());
        assert!(document.model().entities.empty());
        assert!(!document.model().layers.contains("SURVEY KERB"));
        assert!(!document.model().layers.contains("SURVEY SERVICES"));
        assert!(!document.model().styles.contains("WATR Main"));
    }

    #[test]
    fn no_features_is_no_command_and_no_error() {
        let document = Document::default();
        let project = SurveyProject {
            points: vec![plain("1", 0.0, 0.0)],
            ..SurveyProject::default()
        };
        let planned = draw_survey_features(&document, &project, &SurveyFeatureOptions::default())
            .expect("planned");
        assert!(planned.command.is_none());
        assert!(planned.report.strings.is_empty());
        assert!(planned.unplaced.is_empty());
    }
}
