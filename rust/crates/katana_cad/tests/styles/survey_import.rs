// Survey field data into the drawing (PLAN.MD 45; tests/cad/test_survey_import.cpp).
//
// The test this file exists for is ImportedPointsAreNotTransposed. Every other
// failure mode here announces itself; a transposed survey does not - it is
// internally consistent, it plots, and it is in the wrong place.

use katana_cad::survey_coding::code_property_candidates;
use katana_cad::survey_import::{
    COORDINATE_SOURCE_PROPERTY, SOURCE_FILE_META, SOURCE_FORMAT_META, SOURCE_FORMAT_VERSION_META,
    SOURCE_MANUFACTURER_META, SOURCE_RECORD_META, import_survey_project,
};
use katana_cad::{Document, SurveyImportOptions, SurveyImportReport};
use katana_core::ErrorCode;
use katana_entity::entity::ELEVATION_PROPERTY;
use katana_entity::{Entity, Geometry, PropertyMap, PropertyValue};
use katana_survey::data_model::{
    CoordinateSource, SourceRecord, SurveyPoint, SurveyProject, UnpositionedPoint,
};

use crate::double_eq;

// Northing and easting of unmistakably different magnitudes, so that a
// transposition cannot pass any assertion here by coincidence. These are the
// shape of a real projected survey coordinate: northing in the millions,
// easting in the hundreds of thousands.
const NORTHING: f64 = 5_432_109.25;
const EASTING: f64 = 456_789.5;

// The C++ point_at's default height is 0.0: a point with a height of zero.
fn point_at(id: &str, northing: f64, easting: f64, elevation: f64) -> SurveyPoint {
    SurveyPoint {
        id: String::from(id),
        northing,
        easting,
        elevation: Some(elevation),
        ..SurveyPoint::default()
    }
}

fn one_leica_point() -> SurveyProject {
    let mut point = point_at("101", NORTHING, EASTING, 42.125);
    point.code = String::from("BM");
    point.description = String::from("brass plug in kerb");
    point.coordinate_source = CoordinateSource::FieldObserved;
    point.source = SourceRecord {
        manufacturer: String::from("Leica"),
        format: String::from("GSI-16"),
        format_version: String::from("16 byte words"),
        file_name: String::from("FIELD_2026.GSI"),
        record_number: 183,
    };
    SurveyProject {
        points: vec![point],
        ..SurveyProject::default()
    }
}

// Executes the command the bridge produced into a document the caller owns.
fn import_into(
    document: &mut Document,
    project: &SurveyProject,
    options: &SurveyImportOptions,
    report: Option<&mut SurveyImportReport>,
) {
    let command = match import_survey_project(document, project, options, report) {
        Ok(command) => command,
        Err(error) => panic!("{}", error.describe()),
    };
    if let Some(command) = command
        && let Err(error) = document.execute(command)
    {
        panic!("{}", error.describe());
    }
}

fn only_entity(document: &Document) -> Option<&Entity> {
    let ids = document.model().entities.ids();
    if ids.len() != 1 {
        return None;
    }
    document.model().entities.find(ids[0])
}

fn real_property(entity: &Entity, key: &str) -> f64 {
    match entity.properties.get(key) {
        Some(PropertyValue::Real(value)) => *value,
        _ => f64::NAN,
    }
}

fn text_property(map: &PropertyMap, key: &str) -> String {
    match map.get(key) {
        Some(PropertyValue::Text(text)) => text.clone(),
        _ => String::new(),
    }
}

mod survey_import {
    use super::*;

    // THE test. A survey point is (northing, easting) - north first, as a field
    // book and LandXML write it. A CAD Point2 is (x, y), which is EASTING
    // first. The two magnitudes here differ by a factor of ten, so a swap
    // cannot pass.
    #[test]
    fn imported_points_are_not_transposed() {
        let mut document = Document::default();
        import_into(
            &mut document,
            &one_leica_point(),
            &SurveyImportOptions::default(),
            None,
        );
        let entity = only_entity(&document).expect("one entity");

        let Geometry::Point(point) = &entity.geometry else {
            panic!("a survey point should import as a point entity");
        };
        double_eq(point.position.x, EASTING); // x is EASTING
        double_eq(point.position.y, NORTHING); // y is NORTHING
    }

    #[test]
    fn the_point_keeps_its_number_code_description_and_height() {
        let mut document = Document::default();
        import_into(
            &mut document,
            &one_leica_point(),
            &SurveyImportOptions::default(),
            None,
        );
        let entity = only_entity(&document).expect("one entity");

        assert_eq!(text_property(&entity.properties, "point"), "101");
        assert_eq!(text_property(&entity.properties, "code"), "BM");
        assert_eq!(
            text_property(&entity.properties, "description"),
            "brass plug in kerb"
        );
        double_eq(real_property(entity, ELEVATION_PROPERTY), 42.125);
    }

    // The height goes in the property the archive import writes and the surface
    // builder reads. A second name for it would build every drawing's surface
    // flat.
    #[test]
    fn the_height_uses_the_same_property_the_surface_builder_reads() {
        assert_eq!(ELEVATION_PROPERTY, "elevation");
        let mut document = Document::default();
        import_into(
            &mut document,
            &one_leica_point(),
            &SurveyImportOptions::default(),
            None,
        );
        let entity = only_entity(&document).expect("one entity");
        assert!(entity.properties.contains_key(ELEVATION_PROPERTY));
    }

    // PLAN.MD 45.3b. A point the file gave no height for must not arrive at the
    // datum: 0.0 is a real height, and a surface built from it is pulled down to
    // zero under every such point. It arrives with no elevation property at
    // all - which the surface builder already reads as "leave this point out" -
    // and the report counts it and says so.
    #[test]
    fn a_point_with_no_height_arrives_without_one_rather_than_at_the_datum() {
        let mut project = one_leica_point();
        let mut flat = point_at("102", NORTHING + 10.0, EASTING, 0.0);
        flat.elevation = None;
        project.points.push(flat);

        let mut document = Document::default();
        let mut report = SurveyImportReport::default();
        import_into(
            &mut document,
            &project,
            &SurveyImportOptions::default(),
            Some(&mut report),
        );
        assert_eq!(report.points, 2);
        assert_eq!(report.points_without_elevation, 1);
        let last = report.warnings.last().expect("a warning");
        assert!(last.contains("1 point(s) have no height"), "{last}");

        let mut with_height = 0;
        for id in document.model().entities.ids() {
            let entity = document.model().entities.find(id).expect("an entity");
            let has_height = entity.properties.contains_key(ELEVATION_PROPERTY);
            if text_property(&entity.properties, "point") == "102" {
                assert!(!has_height, "the unheighted point must carry no elevation");
            } else {
                assert!(has_height);
                with_height += 1;
            }
        }
        assert_eq!(with_height, 1);
    }

    // A raw observation file names its targets without coordinates. They are
    // not drawn - there is nowhere to draw them - and the report says so,
    // including when that leaves nothing at all to import.
    #[test]
    fn a_point_with_no_coordinates_is_reported_and_never_drawn_at_the_origin() {
        let mut project = SurveyProject {
            unpositioned_points: vec![UnpositionedPoint {
                id: String::from("7"),
                ..UnpositionedPoint::default()
            }],
            ..SurveyProject::default()
        };

        let mut document = Document::default();
        let mut report = SurveyImportReport::default();
        let nothing = import_survey_project(
            &document,
            &project,
            &SurveyImportOptions::default(),
            Some(&mut report),
        )
        .expect("planned");
        assert!(nothing.is_none(), "no positioned point, so nothing to do");
        assert_eq!(report.points_without_position, 1);
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("no coordinates"));

        project.points.push(point_at("101", NORTHING, EASTING, 1.0));
        import_into(
            &mut document,
            &project,
            &SurveyImportOptions::default(),
            Some(&mut report),
        );
        assert_eq!(report.points, 1);
        assert_eq!(report.points_without_position, 1);
        assert_eq!(document.model().entities.ids().len(), 1);
    }

    // The brief's section 18: the drawing alone has to answer "where did this
    // come from", down to the record.
    #[test]
    fn every_point_records_the_file_and_record_it_came_from() {
        let mut document = Document::default();
        import_into(
            &mut document,
            &one_leica_point(),
            &SurveyImportOptions::default(),
            None,
        );
        let entity = only_entity(&document).expect("one entity");

        assert_eq!(
            text_property(&entity.metadata, SOURCE_MANUFACTURER_META),
            "Leica"
        );
        assert_eq!(
            text_property(&entity.metadata, SOURCE_FORMAT_META),
            "GSI-16"
        );
        assert_eq!(
            text_property(&entity.metadata, SOURCE_FORMAT_VERSION_META),
            "16 byte words"
        );
        assert_eq!(
            text_property(&entity.metadata, SOURCE_FILE_META),
            "FIELD_2026.GSI"
        );

        let record = entity
            .metadata
            .get(SOURCE_RECORD_META)
            .expect("a record number");
        let PropertyValue::Integer(number) = record else {
            panic!("a record number is a number, so a report can sort on it");
        };
        assert_eq!(*number, 183);
    }

    // Calculated must be distinguishable from observed, because a report may
    // not present a derived coordinate as evidence.
    #[test]
    fn an_observed_coordinate_is_distinguishable_from_a_calculated_one() {
        let mut observed = point_at("1", NORTHING, EASTING, 0.0);
        observed.coordinate_source = CoordinateSource::FieldObserved;
        let mut calculated = point_at("2", NORTHING + 10.0, EASTING + 10.0, 0.0);
        calculated.coordinate_source = CoordinateSource::Calculated;
        let project = SurveyProject {
            points: vec![observed, calculated],
            ..SurveyProject::default()
        };

        let mut document = Document::default();
        import_into(
            &mut document,
            &project,
            &SurveyImportOptions::default(),
            None,
        );
        let ids = document.model().entities.ids();
        assert_eq!(ids.len(), 2);

        let entities = &document.model().entities;
        let source = |id| {
            entities
                .find(id)
                .map(|entity| text_property(&entity.properties, COORDINATE_SOURCE_PROPERTY))
                .unwrap_or_default()
        };
        assert_eq!(source(ids[0]), "field observed");
        assert_eq!(source(ids[1]), "calculated");
    }

    // A whole import is one undo step. This is the brief's section 19 and it is
    // the reason the bridge returns one command rather than a list of them.
    #[test]
    fn one_undo_removes_the_whole_import_and_one_redo_brings_it_back() {
        let mut project = SurveyProject::default();
        for i in 1..=25_i32 {
            let offset = f64::from(i);
            project.points.push(point_at(
                &format!("{i}"),
                NORTHING + offset,
                EASTING + offset,
                10.0 + offset,
            ));
        }

        let mut document = Document::default();
        let command =
            import_survey_project(&document, &project, &SurveyImportOptions::default(), None)
                .expect("planned")
                .expect("a command");
        assert!(document.execute(command).is_ok());
        assert_eq!(document.model().entities.size(), 25);

        assert!(document.undo().is_ok());
        assert_eq!(
            document.model().entities.size(),
            0,
            "one undo, not twenty-five"
        );

        assert!(document.redo().is_ok());
        assert_eq!(document.model().entities.size(), 25);
    }

    // The layer the points need is created inside the same transaction, so the
    // undo above takes it away again too.
    #[test]
    fn the_layer_is_created_in_the_same_transaction_and_undone_with_it() {
        let mut document = Document::default();
        let options = SurveyImportOptions {
            layer: String::from("survey/field/2026"),
            ..SurveyImportOptions::default()
        };

        let mut report = SurveyImportReport::default();
        let command =
            import_survey_project(&document, &one_leica_point(), &options, Some(&mut report))
                .expect("planned")
                .expect("a command");
        assert_eq!(report.layers_created, vec!["survey/field/2026"]);

        assert!(document.execute(command).is_ok());
        assert!(document.model().layers.find("survey/field/2026").is_some());

        assert!(document.undo().is_ok());
        assert!(
            document.model().layers.find("survey/field/2026").is_none(),
            "the layer came in with the import, so it goes out with it"
        );
    }

    #[test]
    fn a_layer_per_code_puts_each_code_on_its_own_layer_beneath_the_target() {
        let mut kerb = point_at("1", NORTHING, EASTING, 0.0);
        kerb.code = String::from("EP");
        let mut mark = point_at("2", NORTHING + 5.0, EASTING + 5.0, 0.0);
        mark.code = String::from("BM");
        let uncoded = point_at("3", NORTHING + 9.0, EASTING + 9.0, 0.0);
        let project = SurveyProject {
            points: vec![kerb, mark, uncoded],
            ..SurveyProject::default()
        };

        let options = SurveyImportOptions {
            layer_per_code: true,
            ..SurveyImportOptions::default()
        };
        let mut report = SurveyImportReport::default();
        let mut document = Document::default();
        import_into(&mut document, &project, &options, Some(&mut report));

        // Name order, because the layers are created from an ordered set.
        assert_eq!(
            report.layers_created,
            vec!["survey/points", "survey/points/BM", "survey/points/EP"]
        );
        assert!(document.model().layers.find("survey/points/EP").is_some());
        assert!(document.model().layers.find("survey/points/BM").is_some());
        // An uncoded point has nowhere else to go than the target layer itself.
        assert!(document.model().layers.find("survey/points").is_some());
    }

    // A layer name IS a path, so a '/' in a field code would silently build a
    // subtree. It is replaced instead.
    #[test]
    fn a_separator_in_a_field_code_does_not_silently_create_a_layer_subtree() {
        let mut point = point_at("1", NORTHING, EASTING, 0.0);
        point.code = String::from("KERB/TOP");
        let project = SurveyProject {
            points: vec![point],
            ..SurveyProject::default()
        };

        let options = SurveyImportOptions {
            layer_per_code: true,
            ..SurveyImportOptions::default()
        };
        let mut document = Document::default();
        import_into(&mut document, &project, &options, None);

        assert!(
            document
                .model()
                .layers
                .find("survey/points/KERB_TOP")
                .is_some()
        );
        assert!(
            document.model().layers.find("survey/points/KERB").is_none(),
            "a code is user data and must not be able to nest a layer"
        );
    }

    #[test]
    fn a_project_with_no_points_is_nothing_to_do_rather_than_a_failure() {
        let document = Document::default();
        let command = import_survey_project(
            &document,
            &SurveyProject::default(),
            &SurveyImportOptions::default(),
            None,
        )
        .expect("an empty project is not an error");
        assert!(command.is_none(), "and it is not a command either");
    }

    // The bridge asks the survey layer rather than checking again itself, so an
    // inconsistent project is refused with that layer's own error.
    #[test]
    fn an_inconsistent_project_is_refused_before_the_drawing_is_touched() {
        let project = SurveyProject {
            points: vec![
                point_at("1", NORTHING, EASTING, 0.0),
                point_at("1", 0.0, 0.0, 0.0),
            ],
            ..SurveyProject::default()
        };

        let document = Document::default();
        let command =
            import_survey_project(&document, &project, &SurveyImportOptions::default(), None);
        assert!(command.is_err(), "two points cannot share an id");
        assert_eq!(document.model().entities.size(), 0);
    }

    #[test]
    fn refusing_to_create_layers_refuses_the_import_rather_than_moving_the_points() {
        let options = SurveyImportOptions {
            create_layers: false,
            ..SurveyImportOptions::default()
        };

        let document = Document::default();
        let Err(refused) = import_survey_project(&document, &one_leica_point(), &options, None)
        else {
            panic!("refused");
        };
        assert_eq!(refused.code, ErrorCode::NotFound);
        assert_eq!(
            document.model().entities.size(),
            0,
            "a point on the wrong layer is worse than an error"
        );
    }

    // The file's own extra fields survive the import, but never on top of a
    // field the import itself writes.
    #[test]
    fn extra_fields_from_the_file_are_kept_but_do_not_overwrite_the_imports_own() {
        let mut point = point_at("101", NORTHING, EASTING, 0.0);
        point.code = String::from("BM");
        point
            .metadata
            .insert(String::from("prism"), String::from("0.030"));
        point
            .metadata
            .insert(String::from("code"), String::from("SHOULD NOT WIN"));
        let project = SurveyProject {
            points: vec![point],
            ..SurveyProject::default()
        };

        let mut report = SurveyImportReport::default();
        let mut document = Document::default();
        import_into(
            &mut document,
            &project,
            &SurveyImportOptions::default(),
            Some(&mut report),
        );
        let entity = only_entity(&document).expect("one entity");

        assert_eq!(text_property(&entity.properties, "prism"), "0.030");
        assert_eq!(
            text_property(&entity.properties, "code"),
            "BM",
            "the code column wins"
        );
        assert_eq!(
            report.warnings.len(),
            1,
            "and the clash is reported, not hidden"
        );
        assert!(report.warnings[0].contains("code"));
    }

    // The default property names are the ones apply_survey_codes() looks for,
    // so importing field data and then applying a mapfile is one workflow.
    #[test]
    fn the_default_code_property_is_one_the_survey_coding_looks_for() {
        let options = SurveyImportOptions::default();
        assert!(
            code_property_candidates().contains(&options.code_property.as_str()),
            "an import whose code property the coding cannot find is two workflows, not one"
        );
    }
}
