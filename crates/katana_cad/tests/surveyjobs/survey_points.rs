// The drawing's survey points (tests/cad/customisation/test_survey_points.cpp):
// the list the Point Manager and the Point Report show, the export's points,
// what an import does with ids the drawing already has, and the stated
// transformation.
//
// Coordinates differ in magnitude between easting and northing (E ~ 500 000,
// N ~ 7 000 000 or E ~ 1 000, N ~ 5 000), so a transposition cannot pass.

use katana_cad::survey_points::{
    declared_system_from_epsg, drawing_survey_points, format_point_report,
    format_survey_import_report, import_survey_points, point_report_csv, survey_points_among,
    to_survey_point, transform_survey_project,
};
use katana_cad::{
    Document, ExistingPointPolicy, SurveyImportOptions, SurveyImportSummary,
    SurveyPointImportReport,
};
use katana_commands::create_entities;
use katana_core::ErrorCode;
use katana_core::error::make_error;
use katana_entity::{Entity, EntityId, Geometry, PointGeometry};
use katana_geometry::Point2;
use katana_survey::data_model::{DeclaredUnits, LinearUnit, SurveyPoint, SurveyProject};

use crate::{double_eq, expect_ok, must, near, refused};

fn contains(text: &str, part: &str) -> bool {
    text.contains(part)
}

fn point_at(
    id: &str,
    northing: f64,
    easting: f64,
    elevation: Option<f64>,
    code: &str,
) -> SurveyPoint {
    let mut point = SurveyPoint {
        id: String::from(id),
        northing,
        easting,
        elevation,
        code: String::from(code),
        ..SurveyPoint::default()
    };
    point.source.format = String::from("Delimited points");
    point.source.file_name = String::from("site.csv");
    point
}

fn project_of(points: Vec<SurveyPoint>) -> SurveyProject {
    SurveyProject {
        units: DeclaredUnits {
            linear: LinearUnit::Metres,
            ..DeclaredUnits::default()
        },
        points,
        ..SurveyProject::default()
    }
}

fn defaults() -> SurveyImportOptions {
    SurveyImportOptions::default()
}

// Imports under the default options, or fails the test.
#[track_caller]
fn import_into(document: &mut Document, project: &SurveyProject, policy: ExistingPointPolicy) {
    let command = expect_ok(import_survey_points(
        document,
        project,
        &defaults(),
        policy,
        None,
    ));
    let Some(command) = command else {
        panic!("the import drew nothing");
    };
    must(document, command);
}

fn add_cad_point(document: &mut Document, position: Point2) -> EntityId {
    let entity = Entity {
        geometry: Geometry::Point(PointGeometry { position }),
        layer: String::from(document.current_layer()),
        ..Entity::default()
    };
    assert!(document.execute(create_entities(vec![entity])).is_ok());
    document.last_created_entities()[0]
}

mod survey_points {
    use super::*;

    #[test]
    fn imported_points_list_back_as_they_went_in_with_an_absent_elevation_still_absent() {
        let mut document = Document::default();
        import_into(
            &mut document,
            &project_of(vec![
                point_at("101", 7000000.5, 500000.25, Some(12.5), "CP"),
                point_at("102", 7000010.0, 500020.0, None, ""),
            ]),
            ExistingPointPolicy::Refuse,
        );
        let points = drawing_survey_points(&document, &defaults());
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].id, "101");
        assert_eq!(points[0].code, "CP");
        double_eq(points[0].easting, 500000.25);
        double_eq(points[0].northing, 7000000.5);
        let Some(elevation) = points[0].elevation else {
            panic!("101 has its height");
        };
        double_eq(elevation, 12.5);
        assert_eq!(points[0].source_file, "site.csv");
        assert_eq!(points[0].layer, "survey/points");
        assert_eq!(points[1].id, "102");
        assert!(points[1].elevation.is_none(), "absent is not zero");

        // And back to the survey model, north first, for a writer.
        let back = to_survey_point(&points[0]);
        double_eq(back.northing, 7000000.5);
        double_eq(back.easting, 500000.25);
        assert_eq!(back.code, "CP");
        assert!(to_survey_point(&points[1]).elevation.is_none());
    }

    #[test]
    fn a_point_entity_with_no_number_is_not_a_survey_point_and_is_counted_when_asked_for() {
        let mut document = Document::default();
        import_into(
            &mut document,
            &project_of(vec![point_at("7", 5000.0, 1000.0, Some(3.0), "")]),
            ExistingPointPolicy::Refuse,
        );
        let cad = add_cad_point(&mut document, Point2::new(1001.0, 5001.0));
        assert_eq!(drawing_survey_points(&document, &defaults()).len(), 1);

        let survey = drawing_survey_points(&document, &defaults())[0].entity;
        let asked: [EntityId; 3] = [cad, survey, 999999];
        let pick = survey_points_among(&document, &asked, &defaults());
        assert_eq!(pick.points.len(), 1);
        assert_eq!(pick.points[0].id, "7");
        assert_eq!(
            pick.not_survey_points, 2,
            "the CAD point and the id the drawing lacks"
        );
    }

    #[test]
    fn an_import_refused_for_ids_already_in_the_drawing_names_them_and_changes_nothing() {
        let mut document = Document::default();
        import_into(
            &mut document,
            &project_of(vec![
                point_at("1", 5000.0, 1000.0, None, ""),
                point_at("2", 5010.0, 1010.0, None, ""),
            ]),
            ExistingPointPolicy::Refuse,
        );
        let error = refused(import_survey_points(
            &document,
            &project_of(vec![
                point_at("2", 5020.0, 1020.0, None, ""),
                point_at("3", 5030.0, 1030.0, None, ""),
            ]),
            &defaults(),
            ExistingPointPolicy::Refuse,
            None,
        ));
        assert_eq!(error.code, ErrorCode::AlreadyExists);
        assert_eq!(error.context, "2");
        assert_eq!(drawing_survey_points(&document, &defaults()).len(), 2);
    }

    #[test]
    fn skipping_keeps_the_drawings_point_and_imports_only_the_new_ids() {
        let mut document = Document::default();
        import_into(
            &mut document,
            &project_of(vec![point_at("1", 5000.0, 1000.0, None, "")]),
            ExistingPointPolicy::Refuse,
        );
        let mut report = SurveyPointImportReport::default();
        let command = expect_ok(import_survey_points(
            &document,
            &project_of(vec![
                point_at("1", 5555.0, 1555.0, None, ""),
                point_at("2", 5010.0, 1010.0, None, ""),
            ]),
            &defaults(),
            ExistingPointPolicy::Skip,
            Some(&mut report),
        ));
        assert_eq!(report.skipped, 1);
        assert_eq!(report.import.points, 1);
        assert_eq!(report.existing_ids, ["1"]);
        let Some(command) = command else {
            panic!("point 2 is still to import");
        };
        must(&mut document, command);

        let points = drawing_survey_points(&document, &defaults());
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].id, "1");
        double_eq(points[0].easting, 1000.0); // the drawing's point 1 is kept
        assert_eq!(points[1].id, "2");
    }

    #[test]
    fn every_point_skipped_is_nothing_to_do_not_an_error() {
        let mut document = Document::default();
        import_into(
            &mut document,
            &project_of(vec![point_at("1", 5000.0, 1000.0, None, "")]),
            ExistingPointPolicy::Refuse,
        );
        let mut report = SurveyPointImportReport::default();
        let command = expect_ok(import_survey_points(
            &document,
            &project_of(vec![point_at("1", 5.0, 1.0, None, "")]),
            &defaults(),
            ExistingPointPolicy::Skip,
            Some(&mut report),
        ));
        assert!(command.is_none());
        assert_eq!(report.skipped, 1);
    }

    #[test]
    fn replacing_is_one_command_that_one_undo_puts_back() {
        let mut document = Document::default();
        import_into(
            &mut document,
            &project_of(vec![point_at("1", 5000.0, 1000.0, Some(10.0), "")]),
            ExistingPointPolicy::Refuse,
        );
        let mut report = SurveyPointImportReport::default();
        let command = expect_ok(import_survey_points(
            &document,
            &project_of(vec![
                point_at("1", 5555.0, 1555.0, Some(20.0), ""),
                point_at("2", 5010.0, 1010.0, None, ""),
            ]),
            &defaults(),
            ExistingPointPolicy::Replace,
            Some(&mut report),
        ));
        assert_eq!(report.replaced, 1);
        let Some(command) = command else {
            panic!("the replacement is a command");
        };
        must(&mut document, command);

        let points = drawing_survey_points(&document, &defaults());
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].id, "1");
        // The file's point 1 replaced the drawing's.
        double_eq(points[0].easting, 1555.0);
        double_eq(points[0].elevation.unwrap_or(f64::NAN), 20.0);

        assert!(document.undo().is_ok());
        let points = drawing_survey_points(&document, &defaults());
        assert_eq!(points.len(), 1);
        double_eq(points[0].easting, 1000.0);
        double_eq(points[0].elevation.unwrap_or(f64::NAN), 10.0);
    }

    #[test]
    fn keeping_both_leaves_two_points_of_that_id_and_says_so() {
        let mut document = Document::default();
        import_into(
            &mut document,
            &project_of(vec![point_at("1", 5000.0, 1000.0, None, "")]),
            ExistingPointPolicy::Refuse,
        );
        let mut report = SurveyPointImportReport::default();
        let command = expect_ok(import_survey_points(
            &document,
            &project_of(vec![point_at("1", 5555.0, 1555.0, None, "")]),
            &defaults(),
            ExistingPointPolicy::KeepBoth,
            Some(&mut report),
        ));
        let Some(command) = command else {
            panic!("keeping both imports the point");
        };
        must(&mut document, command);
        assert_eq!(drawing_survey_points(&document, &defaults()).len(), 2);
        assert!(!report.warnings.is_empty());
        assert!(
            contains(&report.warnings[0], "in the drawing twice"),
            "{}",
            report.warnings[0]
        );
    }

    #[test]
    fn the_southern_utm_zone_is_the_northern_one_ten_million_metres_up() {
        // UTM by definition (NGA.SIG.0012): the northern and southern zones of
        // the same number share the projection and differ only in the false
        // northing, 0 in the north and 10 000 000 m in the south. So a WGS 84
        // point at (E 400 000, N 5 000) in zone 30N (EPSG:32630) is
        // (400 000, 10 005 000) in zone 30S (EPSG:32730), and the height is not
        // touched.
        let moved = expect_ok(transform_survey_project(
            project_of(vec![
                point_at("A", 5000.0, 400000.0, Some(25.0), ""),
                point_at("B", 6000.0, 410000.0, None, ""),
            ]),
            32630,
            32730,
        ));
        assert_eq!(moved.points.len(), 2);
        near(moved.points[0].easting, 400000.0, 1e-6);
        near(moved.points[0].northing, 10005000.0, 1e-6);
        let Some(height) = moved.points[0].elevation else {
            panic!("A keeps its height");
        };
        double_eq(height, 25.0);
        assert!(moved.points[1].elevation.is_none(), "absent stays absent");
        near(moved.points[1].northing, 10006000.0, 1e-6);
        assert_eq!(moved.coordinate_system.epsg_code, 32730);
        assert!(!moved.coordinate_system.unknown);
        assert!(moved.metadata.contains_key("transformed from"));
    }

    // EPSG:32118 (NAD83 / New York Long Island, metres) and EPSG:2263 (the same
    // projection and datum in US survey feet, 1 ftUS = 1200/3937 m) describe the
    // SAME grid, so a point keeps its metres through a transformation between
    // them either way - but only if the model's metres are turned into the feet
    // system's own unit on its side of the transformation. The two tests below
    // run one direction each, because each direction needs a different half of
    // that conversion.

    #[test]
    fn transforming_into_a_us_survey_foot_system_gives_its_feet_back_as_metres() {
        // The target is in feet: the transformer returns 300 000 m as
        // 300 000 x 3937/1200 = 984 250 ftUS, which must be turned back into
        // metres. Taken as metres, the easting would come back as 984 250.
        let moved = expect_ok(transform_survey_project(
            project_of(vec![point_at("1", 60000.0, 300000.0, None, "")]),
            32118,
            2263,
        ));
        near(moved.points[0].easting, 300000.0, 1e-4);
        near(moved.points[0].northing, 60000.0, 1e-4);
    }

    #[test]
    fn transforming_out_of_a_us_survey_foot_system_feeds_it_feet_not_metres() {
        // The source is in feet: the model's 300 000 m must reach the
        // transformer as 984 250 ftUS. Fed as 300 000 ftUS it would come back
        // as 300 000 x 1200/3937 = 91 440.18 m, and the northing 60 000 as
        // 60 000 x 1200/3937 = 18 288.04 m.
        let moved = expect_ok(transform_survey_project(
            project_of(vec![point_at("1", 60000.0, 300000.0, None, "")]),
            2263,
            32118,
        ));
        near(moved.points[0].easting, 300000.0, 1e-4);
        near(moved.points[0].northing, 60000.0, 1e-4);
        assert_eq!(moved.coordinate_system.epsg_code, 32118);
    }

    #[test]
    fn a_geographic_system_is_refused_because_points_are_grid_coordinates() {
        let error = refused(transform_survey_project(
            project_of(vec![point_at("1", 5000.0, 1000.0, None, "")]),
            4326,
            32630,
        ));
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(declared_system_from_epsg(0).is_err());
        let declared = expect_ok(declared_system_from_epsg(32630));
        assert_eq!(declared.epsg_code, 32630);
    }

    #[test]
    fn the_point_report_and_its_csv_leave_an_absent_elevation_blank() {
        let mut document = Document::default();
        import_into(
            &mut document,
            &project_of(vec![
                point_at("101", 7000000.5, 500000.25, Some(12.5), "CP"),
                point_at("10,2", 7000010.0, 500020.0, None, ""),
            ]),
            ExistingPointPolicy::Refuse,
        );
        let points = drawing_survey_points(&document, &defaults());
        let report = format_point_report(&points, 3);
        assert!(contains(&report, "500000.250"), "{report}");
        assert!(contains(&report, "7000000.500"), "{report}");
        assert!(
            contains(&report, "2 point(s), 1 without an elevation"),
            "{report}"
        );
        // The second point's elevation is empty between its northing and its
        // (empty) code; its id holds a comma and is quoted.
        assert_eq!(
            point_report_csv(&points, 3),
            "Point,Easting,Northing,Elevation,Code,Description,Source file\r\n\
             101,500000.250,7000000.500,12.500,CP,,site.csv\r\n\
             \"10,2\",500020.000,7000010.000,,,,site.csv\r\n"
        );
    }

    #[test]
    fn the_import_report_says_what_will_be_created_or_what_blocks_it() {
        let document = Document::default();
        let summary = SurveyImportSummary {
            file_name: String::from("site.csv"),
            format: String::from("Delimited points 1.0"),
            records_read: 2,
            coordinate_system: String::from("unknown"),
            units: String::from("metres"),
            reader_warnings: vec![String::from("1 point has no elevation")],
            ..SurveyImportSummary::default()
        };
        let mut report = SurveyPointImportReport::default();
        let command = import_survey_points(
            &document,
            &project_of(vec![
                point_at("1", 5000.0, 1000.0, None, ""),
                point_at("2", 5010.0, 1010.0, None, ""),
            ]),
            &defaults(),
            ExistingPointPolicy::Refuse,
            Some(&mut report),
        );
        assert!(command.is_ok());
        let text = format_survey_import_report(&summary, &report, None);
        assert!(contains(&text, "Records read: 2"), "{text}");
        assert!(contains(&text, "  - 1 point has no elevation."), "{text}");
        assert!(contains(&text, "Points to create: 2"), "{text}");
        assert!(contains(&text, "Layers to create: survey/points"), "{text}");

        let blocking = make_error(ErrorCode::AlreadyExists, "ids already there", "2");
        let blocked = format_survey_import_report(&summary, &report, Some(&blocking));
        assert!(contains(&blocked, "Import blocked:"), "{blocked}");
        assert!(!contains(&blocked, "Points to create"), "{blocked}");
    }
}
