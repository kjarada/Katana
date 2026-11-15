//! The twins of tests/qt_widgets/test_project_crs_dialog.cpp: File > Project Coordinate System
//! (katana_window::project_crs_dialog), driven by its controls' object names as a person uses
//! them, each change one undo step through the CRS verb the dialog builds. The window's executor
//! is a recording host that runs each line on its own interpreter and drawing - the C++ dialog's
//! fallback when it was given no runner, and the runner its second case wrote. Expected values are
//! the C++'s.

use katana_cad::Document;
use katana_commands::{EntityAttributes, create_point};
use katana_geometry::Point2;
use katana_window::project_crs_dialog::{ProjectCrsDialog, project_crs_label};

use super::Bench;
use super::window_support::interpreter_host;

/// The dialog over `document` and `place`, shown, on the harness; its lines run on the drawing.
fn crs_bench(document: Document, place: Option<(f64, f64)>) -> Bench<ProjectCrsDialog> {
    let host = interpreter_host(document);
    let mut dialog = ProjectCrsDialog::new(host.document(), place);
    dialog.show();
    Bench::new("projectCrsDialog", dialog, host)
}

/// Every system's code, in the order the list shows them.
fn codes(dialog: &ProjectCrsDialog) -> Vec<String> {
    dialog
        .groups()
        .iter()
        .flat_map(|group| group.choices.iter().map(|choice| choice.id.clone()))
        .collect()
}

fn with_system(code: &str) -> Document {
    let mut document = Document::new(None);
    document
        .set_coordinate_system(code, Document::SET_CRS)
        .expect("the system is set");
    document
}

mod project_crs_dialog {
    use super::*;

    #[test]
    fn typing_a_code_checks_it_and_set_sets_it_as_one_step() {
        let mut bench = crs_bench(Document::new(None), None);
        bench.fill("projectCrsText", "7856");
        let check = bench.text("projectCrsCheck");
        assert!(check.contains("GDA2020 / MGA zone 56"), "{check}");
        bench.fill("projectCrsText", "no such system");
        let check = bench.text("projectCrsCheck");
        assert!(check.starts_with("Not a coordinate system"), "{check}");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(
            bench
                .host
                .document()
                .metadata()
                .coordinate_system
                .is_empty()
        );

        let before = bench.host.document().history().undo_count();
        bench.fill("projectCrsText", "epsg:7856");
        assert!(bench.dialog.apply(&mut bench.host));
        assert_eq!(
            bench.host.document().metadata().coordinate_system,
            "EPSG:7856"
        );
        assert_eq!(bench.host.document().history().undo_count(), before + 1);
        assert_eq!(
            project_crs_label(bench.host.document()),
            "EPSG:7856  GDA2020 / MGA zone 56"
        );
    }

    // Set is the CRS verb, handed to the executor as a typed line would be: what a .prj holds,
    // pasted, goes on the line with its quotes, and the verb takes it as the dialog's check did.
    // The WKT is an ESRI-style .prj for GDA2020 / MGA zone 56 written out by hand (the C++'s
    // test_project_crs.cpp says where its numbers come from); it carries no authority, so it is
    // stored as typed.
    #[test]
    fn set_runs_the_crs_verb_and_a_pasted_wkt_keeps_its_quotes() {
        let prj = "PROJCS[\"GDA2020_MGA_Zone_56\",GEOGCS[\"GCS_GDA2020\",DATUM[\"GDA2020\",\
                   SPHEROID[\"GRS_1980\",6378137.0,298.257222101]],PRIMEM[\"Greenwich\",0.0],\
                   UNIT[\"Degree\",0.0174532925199433]],PROJECTION[\"Transverse_Mercator\"],\
                   PARAMETER[\"False_Easting\",500000.0],PARAMETER[\"False_Northing\",10000000.0],\
                   PARAMETER[\"Central_Meridian\",153.0],PARAMETER[\"Scale_Factor\",0.9996],\
                   PARAMETER[\"Latitude_Of_Origin\",0.0],UNIT[\"Meter\",1.0]]";
        let mut bench = crs_bench(Document::new(None), None);
        bench.fill("projectCrsText", prj);
        let before = bench.host.document().history().undo_count();
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.check()
        );
        assert_eq!(bench.host.lines(), [format!("CRS SET {prj}")]);
        assert_eq!(bench.host.document().metadata().coordinate_system, prj);
        assert_eq!(bench.host.document().history().undo_count(), before + 1);

        // A refusal is the verb's, shown in the check line; nothing changes.
        bench.fill("projectCrsText", "no such system");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(
            bench.dialog.check().starts_with("Not set: InvalidCRS"),
            "{}",
            bench.dialog.check()
        );
        assert_eq!(bench.host.document().metadata().coordinate_system, prj);

        bench.fill("projectCrsText", "");
        assert!(bench.dialog.apply(&mut bench.host));
        assert_eq!(
            bench.host.lines().last().map(String::as_str),
            Some("CRS CLEAR")
        );
        assert!(
            bench
                .host
                .document()
                .metadata()
                .coordinate_system
                .is_empty()
        );
    }

    #[test]
    fn a_place_is_offered_its_zone_first_and_search_filters() {
        // Sydney.
        let mut bench = crs_bench(Document::new(None), Some((151.21, -33.87)));
        let groups = bench.dialog.groups();
        assert!(groups.len() > 1);
        assert_eq!(groups[0].title, "Suggested for this place");
        assert_eq!(groups[0].choices[0].id, "EPSG:7856");

        bench.fill("projectCrsSearch", "mga 55");
        assert_eq!(codes(&bench.dialog), ["EPSG:7855", "EPSG:28355"]);

        // Choosing an item fills the text box, and Set sets it.
        let first = bench.dialog.groups()[0].choices[0].name.clone();
        bench.fill("projectCrsList", &first);
        assert_eq!(bench.text("projectCrsText"), "EPSG:7855");
        assert!(bench.dialog.apply(&mut bench.host));
        assert_eq!(
            bench.host.document().metadata().coordinate_system,
            "EPSG:7855"
        );
    }

    #[test]
    fn a_place_typed_is_suggested_its_zone_first() {
        // File > Project Coordinate System opens with no place: one is typed. Perth is at 115.86
        // E, in UTM zone floor((115.86 + 180) / 6) + 1 = 50: GDA2020 / MGA zone 50 is EPSG:7850.
        let mut bench = crs_bench(Document::new(None), None);
        assert!(
            bench.text("projectCrsPlace").is_empty(),
            "no system and no drawing: no place to give"
        );
        assert!(!bench.dialog.groups().is_empty());
        assert_ne!(bench.dialog.groups()[0].title, "Suggested for this place");

        bench.fill("projectCrsSearch", "utm");
        bench.fill("projectCrsPlace", "115.86 -31.95");
        bench.press("projectCrsSuggest");
        assert!(bench.text("projectCrsSearch").is_empty());
        assert_eq!(bench.dialog.groups()[0].title, "Suggested for this place");
        assert_eq!(bench.dialog.groups()[0].choices[0].id, "EPSG:7850");
    }

    #[test]
    fn a_place_that_is_not_one_is_said_and_suggests_nothing() {
        let mut bench = crs_bench(Document::new(None), None);
        for text in ["north of here", "151.21", "151.21, -33.87, 4", ""] {
            bench.fill("projectCrsPlace", text);
            assert!(!bench.dialog.suggest(), "{text}");
            assert!(bench.dialog.check().starts_with("Not a place"), "{text}");
        }
        // Numbers, but no longitude: suggest_coordinate_systems says which.
        bench.fill("projectCrsPlace", "200, 10");
        assert!(!bench.dialog.suggest());
        assert!(
            bench.dialog.check().contains("longitude"),
            "{}",
            bench.dialog.check()
        );
    }

    #[test]
    fn the_drawings_centre_is_filled_in_as_a_longitude_and_a_latitude() {
        // In WGS 84 / UTM zone 56S (EPSG:32756) the central meridian, 153 E, is at the false
        // easting 500 000, and the equator at the southern false northing 10 000 000 (the UTM
        // definition): a point there is 153, 0 exactly, with no datum between the two systems to
        // shift it.
        let mut document = with_system("EPSG:32756");
        document
            .execute(create_point(
                Point2::new(500000.0, 10000000.0),
                EntityAttributes::default(),
            ))
            .expect("the point is made");
        let bench = crs_bench(document, None);
        let place = bench.text("projectCrsPlace");
        let parts: Vec<&str> = place.split(", ").collect();
        assert_eq!(parts.len(), 2, "{}", parts.join("|"));
        // Printed to 6 decimals, a tenth of a metre.
        let longitude: f64 = parts[0].parse().expect("a longitude");
        let latitude: f64 = parts[1].parse().expect("a latitude");
        assert!((longitude - 153.0).abs() <= 1e-6, "{longitude}");
        assert!(latitude.abs() <= 1e-6, "{latitude}");
        // Filled in, not yet suggested: that is Suggest's.
        assert_ne!(bench.dialog.groups()[0].title, "Suggested for this place");
    }

    #[test]
    fn local_coordinates_clears_it() {
        let mut bench = crs_bench(with_system("EPSG:28356"), None);
        bench.press("projectCrsClear");
        assert!(
            bench
                .host
                .document()
                .metadata()
                .coordinate_system
                .is_empty()
        );
        assert_eq!(
            project_crs_label(bench.host.document()),
            "no coordinate system"
        );
    }

    // Non-modal, the dialog follows the drawing: a system set elsewhere (a typed CRS SET, an
    // undo) is shown, in the text box too until something else is typed there.
    #[test]
    fn a_system_set_elsewhere_is_shown_and_what_is_typed_is_kept() {
        let mut bench = crs_bench(with_system("EPSG:28356"), None);
        bench
            .host
            .document_mut()
            .set_coordinate_system("EPSG:7856", Document::SET_CRS)
            .expect("the system is set");
        bench.draw();
        let current = bench.text("projectCrsCurrent");
        assert!(current.starts_with("Now: EPSG:7856"), "{current}");
        assert_eq!(bench.text("projectCrsText"), "EPSG:7856");

        bench.fill("projectCrsText", "EPSG:7855");
        bench.host.document_mut().undo().expect("undone");
        bench.draw();
        let current = bench.text("projectCrsCurrent");
        assert!(current.starts_with("Now: EPSG:28356"), "{current}");
        assert_eq!(bench.text("projectCrsText"), "EPSG:7855", "typed, so kept");
    }

    // Left open across a new or opened drawing, it shows that drawing's system, and Set carries
    // nothing of the last one's over. It once showed the old project's system, and Set put it on
    // the new one.
    #[test]
    fn a_new_drawing_is_shown_afresh_and_set_carries_nothing_over() {
        // Sydney, as GIS > Online Data gives a place.
        let mut bench = crs_bench(with_system("EPSG:28356"), Some((151.21, -33.87)));
        assert_eq!(bench.dialog.groups()[0].title, "Suggested for this place");

        bench.host.document_mut().new_document();
        bench.draw();
        assert_eq!(bench.text("projectCrsCurrent"), "Now: no coordinate system");
        assert!(bench.text("projectCrsText").is_empty());
        assert!(bench.text("projectCrsPlace").is_empty());
        assert_ne!(
            bench.dialog.groups()[0].title,
            "Suggested for this place",
            "the place was the last drawing's"
        );
        // The opened drawing's own system, then Set: nothing changes.
        bench
            .host
            .document_mut()
            .set_coordinate_system("EPSG:7856", Document::SET_CRS)
            .expect("the system is set");
        bench.draw();
        assert_eq!(bench.text("projectCrsText"), "EPSG:7856");
        let steps = bench.host.document().history().undo_count();
        assert!(bench.dialog.apply(&mut bench.host));
        assert_eq!(
            bench.host.document().metadata().coordinate_system,
            "EPSG:7856"
        );
        assert_eq!(bench.host.document().history().undo_count(), steps);
    }
}
