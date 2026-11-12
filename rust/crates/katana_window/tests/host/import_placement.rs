//! Where an import lands, in the window (src/katana_window/src/import_placement.rs;
//! import_placement.hpp): the Placement group driven by its object names, File > Import's step and
//! the IMPORT line it makes, and the one place the window decides where data it has read goes, in a
//! headless session where nobody is asked. (VectorImportDialog's case is GA's.)
//!
//! The coordinates are those of samples/gis/parcels.geojson, which spans (180, 0) to (365, 165):
//! what the typed and katana_cli tests import.
//!
//! The C++ kept the remembered choice in QSettings under a name of the test's own; the Rust
//! keeps it in the host's settings (UserSettings, in memory in a test), which no other test shares.

use katana_cad::CommandInterpreter;
use katana_cad::import_placement::{ImportPlacement, ImportPlacementMode};
use katana_geometry::{Box2, Point2, Vec2};
use katana_window::drive::Harness;
use katana_window::host::{RecordingHost, UserSettings};
use katana_window::import_placement::{
    DialogResult, ImportPlacementBox, ImportPlacementDialog, decide_import_placement,
};

fn parcels() -> Box2 {
    Box2::new(Point2::new(180.0, 0.0), Point2::new(365.0, 165.0))
}

fn offset(east: f64, north: f64) -> ImportPlacement {
    ImportPlacement {
        mode: ImportPlacementMode::Offset,
        offset: Vec2::new(east, north),
    }
}

fn mode(mode: ImportPlacementMode) -> ImportPlacement {
    ImportPlacement {
        mode,
        offset: Vec2::default(),
    }
}

mod import_placement_box {
    use super::*;

    #[test]
    fn each_choice_is_its_placement_and_says_what_it_will_do_and_the_line_that_does_it() {
        let settings = UserSettings::in_memory();
        let drawing = Box2::new(Point2::new(1000.0, 2000.0), Point2::new(1010.0, 2005.0));
        let mut placement = ImportPlacementBox::new(drawing, "C:/Survey Data/site.dxf", &settings);
        let mut harness = Harness::new();
        harness
            .show(|ui| {
                placement.ui(ui);
            })
            .expect("the group is drawn");
        for name in [
            "importPlacementKeep",
            "importPlacementLocal",
            "importPlacementAlongside",
            "importPlacementOffset",
            "importOffsetE",
            "importOffsetN",
            "importPlacementShift",
        ] {
            assert!(harness.control(name).is_some(), "no {name}");
        }

        // Nothing remembered: its own coordinates, and the question if far away.
        assert_eq!(
            harness
                .control("importPlacementKeep")
                .expect("Keep")
                .checked,
            Some(true)
        );
        assert_eq!(placement.placement(), ImportPlacement::default());
        assert!(
            placement.said().contains("keeps its own coordinates"),
            "{}",
            placement.said()
        );
        assert!(
            placement
                .said()
                .ends_with("Typed: IMPORT \"C:/Survey Data/site.dxf\""),
            "{}",
            placement.said()
        );
        assert!(!harness.control("importOffsetE").expect("east").enabled);

        harness
            .press("importPlacementLocal")
            .run(|ui| {
                placement.ui(ui);
            })
            .expect("Local is clicked");
        assert_eq!(placement.placement().mode, ImportPlacementMode::Local);
        assert!(
            placement.said().contains("lower-left corner sits at 0,0"),
            "{}",
            placement.said()
        );
        assert!(
            placement
                .said()
                .ends_with("IMPORT \"C:/Survey Data/site.dxf\" LOCAL")
        );

        harness
            .press("importPlacementAlongside")
            .run(|ui| {
                placement.ui(ui);
            })
            .expect("Alongside is clicked");
        assert_eq!(placement.placement().mode, ImportPlacementMode::Alongside);
        assert!(
            placement
                .said()
                .contains("sits on the drawing's, at 1000.000,2000.000"),
            "{}",
            placement.said()
        );
        assert!(
            placement
                .said()
                .ends_with("IMPORT \"C:/Survey Data/site.dxf\" ALONGSIDE")
        );

        harness
            .press("importPlacementOffset")
            .run(|ui| {
                placement.ui(ui);
            })
            .expect("Move by is clicked");
        assert!(harness.control("importOffsetE").expect("east").enabled);
        harness
            .fill("importOffsetE", "10")
            .fill("importOffsetN", "-20.5")
            .run(|ui| {
                placement.ui(ui);
            })
            .expect("the offset is typed");
        assert_eq!(placement.placement(), offset(10.0, -20.5));
        assert!(
            placement
                .said()
                .contains("moved as one piece by 10.000,-20.500"),
            "{}",
            placement.said()
        );
        assert!(
            placement
                .said()
                .ends_with("IMPORT \"C:/Survey Data/site.dxf\" OFFSET=10,-20.5"),
            "{}",
            placement.said()
        );
    }

    #[test]
    fn alongside_an_empty_drawing_says_there_is_nothing_to_sit_beside() {
        let settings = UserSettings::in_memory();
        let mut placement = ImportPlacementBox::new(Box2::default(), "", &settings);
        let mut harness = Harness::new();
        harness
            .press("importPlacementAlongside")
            .run(|ui| {
                placement.ui(ui);
            })
            .expect("Alongside is clicked");
        assert!(
            placement.said().contains("The drawing is empty"),
            "{}",
            placement.said()
        );
        assert!(!placement.said().contains("Typed:"), "no file, no line");
    }

    #[test]
    fn the_choice_is_remembered_only_when_an_import_is_accepted_and_offered_next_time() {
        let mut host = RecordingHost::new();
        {
            let mut placement = ImportPlacementBox::new(Box2::default(), "", &host.parts.settings);
            // Not remembered: a click is no import.
            placement.set_placement(&offset(-5.25, 7.0));
        }
        assert_eq!(
            ImportPlacementBox::new(Box2::default(), "", &host.parts.settings).placement(),
            ImportPlacement::default()
        );
        {
            let mut dialog = ImportPlacementDialog::new(
                "C:/data/site.12da",
                Box2::default(),
                &host.parts.settings,
            );
            dialog
                .placement_box_mut()
                .set_placement(&offset(-5.25, 7.0));
            let mut harness = Harness::new();
            harness
                .target("importPlacementDialog")
                .press("importPlacementImport")
                .run(|ui| {
                    dialog.ui(ui, &mut host);
                })
                .expect("Import is pressed");
            assert_eq!(dialog.result(), Some(DialogResult::Accepted));
        }
        assert_eq!(
            ImportPlacementBox::new(Box2::default(), "", &host.parts.settings).placement(),
            offset(-5.25, 7.0)
        );
        {
            let mut dialog = ImportPlacementDialog::new(
                "C:/data/site.12da",
                Box2::default(),
                &host.parts.settings,
            );
            dialog
                .placement_box_mut()
                .set_placement(&mode(ImportPlacementMode::Local));
            let mut harness = Harness::new();
            harness
                .target("importPlacementDialog")
                .press("importPlacementCancel")
                .run(|ui| {
                    dialog.ui(ui, &mut host);
                })
                .expect("Cancel is pressed");
            assert_eq!(dialog.result(), Some(DialogResult::Rejected));
        }
        assert_eq!(
            ImportPlacementBox::new(Box2::default(), "", &host.parts.settings)
                .placement()
                .mode,
            ImportPlacementMode::Offset,
            "a cancelled import remembers nothing"
        );
    }
}

mod import_placement_dialog {
    use super::*;

    #[test]
    fn its_line_reads_back_as_the_path_and_the_placement_it_was_made_from() {
        // File > Import runs this line through the window's executor, which reads it with the
        // interpreter's import_argument: it must mean what was chosen, blanks in the path and all.
        let settings = UserSettings::in_memory();
        for chosen in [
            ImportPlacement::default(),
            mode(ImportPlacementMode::Local),
            mode(ImportPlacementMode::Alongside),
            offset(0.1, -1234567.125),
        ] {
            let mut dialog =
                ImportPlacementDialog::new("C:/Survey Data/job 7.12da", Box2::default(), &settings);
            assert_eq!(dialog.object_name(), "importPlacementDialog");
            dialog.placement_box_mut().set_placement(&chosen);
            let line = dialog.line();
            assert!(line.starts_with("IMPORT "), "{line}");
            let read = CommandInterpreter::import_argument(&line[6..])
                .unwrap_or_else(|error| panic!("{line}: {}", error.describe()));
            assert_eq!(read.path, "C:/Survey Data/job 7.12da", "{line}");
            assert_eq!(read.placement, chosen, "{line}");
        }
    }
}

mod decide_import_placement {
    use super::*;

    #[test]
    fn a_headless_import_far_from_the_drawing_keeps_its_coordinates_and_says_why() {
        let mut logged: Vec<(String, bool)> = Vec::new();
        // A 100 x 50 drawing at the origin and the parcels' shape 18 km east: at a zoom that shows
        // both (diagonal 18 186) the drawing's diagonal, 112, is under 1% of it, yet over 2% of the
        // parcels' 248 - lost by where it was put (interop's advise_placement).
        let drawing = Box2::new(Point2::new(0.0, 0.0), Point2::new(100.0, 50.0));
        let kept = decide_import_placement(
            true,
            &ImportPlacement::default(),
            &drawing,
            &Box2::new(Point2::new(18000.0, 0.0), Point2::new(18185.0, 165.0)),
            &mut |text: &str, is_error: bool| logged.push((String::from(text), is_error)),
        );
        assert!(!kept.cancelled);
        assert!(kept.shift.is_none());
        assert_eq!(logged.len(), 1);
        assert!(
            logged[0].0.ends_with("(kept: no one to ask)."),
            "{}",
            logged[0].0
        );
        assert!(logged[0].1);

        // Near the drawing: nothing to ask, nothing said.
        logged.clear();
        let near = decide_import_placement(
            true,
            &ImportPlacement::default(),
            &Box2::new(Point2::new(0.0, 0.0), Point2::new(200.0, 200.0)),
            &parcels(),
            &mut |text: &str, is_error: bool| logged.push((String::from(text), is_error)),
        );
        assert!(near.shift.is_none());
        assert!(logged.is_empty());
    }

    #[test]
    fn a_chosen_placement_is_never_asked_and_says_the_move_it_makes() {
        let mut logged: Vec<(String, bool)> = Vec::new();
        // Not headless: a chosen placement asks nothing, so this decides at once.
        let local = decide_import_placement(
            false,
            &mode(ImportPlacementMode::Local),
            &Box2::new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.5)),
            &parcels(),
            &mut |text: &str, is_error: bool| logged.push((String::from(text), is_error)),
        );
        assert_eq!(local.shift, Some(Vec2::new(180.0, 0.0)));
        assert!(local.ask.is_none());
        assert_eq!(logged.len(), 1);
        assert_eq!(
            logged[0].0,
            "LOCAL: moved as one piece by -180.000,0.000, so its lower-left corner sits at 0,0."
        );
        assert!(!logged[0].1);
    }
}
