//! The twins of tests/qt_widgets/test_screen_fit_dialogs.cpp: the dialogs of the terrain and road
//! work on a screen too small for them (shell::dialog, docs/desktop.md "Fitting the screen"): Close,
//! Run and Add are the dialog's own buttons, and they stay on the dialog, below the scroll area,
//! whatever is scrolled.
//!
//! As the C++, each dialog is made taller than the screen (800 x 600, the C++'s offscreen one) by a
//! minimum height 400 over the screen's (DialogFrame::set_minimum_height), so the fit has to
//! scroll it whatever its own layout needs. What is tested is that each control named is in the
//! dialog's declared footer, in sight on the dialog, and stays where it is with the body scrolled
//! right down.

use katana_cad::Document;
use katana_window::drive::{ControlKind, Harness, Part};
use katana_window::host::RecordingHost;

use crate::window_support::ok;

/// The screen under the widget tests: the C++'s offscreen one.
const SCREEN: (f32, f32) = (800.0, 600.0);

/// makeTallerThanTheScreen: a minimum height over the screen's.
const TALLER: f32 = SCREEN.1 + 400.0;

/// A host that answers every line as a headless run does; none of these tests runs one.
fn answering() -> RecordingHost {
    RecordingHost::new().answering(katana_window::host::Answer::Fixed(ok("ran")))
}

/// `dialog`, drawn by `draw`, shown through the dialog frame on the short screen: it fits the
/// screen, it was scrolled (the premise), and each control named is in the footer, on the dialog,
/// and stays where it is with the body scrolled right down.
fn expect_in_sight(target: &str, names: &[&str], draw: &mut dyn FnMut(&mut egui::Ui)) {
    let mut harness = Harness::with_screen(SCREEN.0, SCREEN.1);
    harness.run(&mut *draw).expect("the dialog settles");
    let record = harness
        .target_record(target)
        .unwrap_or_else(|| panic!("{target} is drawn"));
    assert!(
        record.rect.height() <= SCREEN.1,
        "{target} is taller than its screen"
    );
    let scroll = record
        .scroll
        .unwrap_or_else(|| panic!("{target}: a dialog over the screen's height scrolls"));
    assert!(scroll.scrolls(), "{target} scrolls");
    let footer = record
        .footer
        .unwrap_or_else(|| panic!("{target}: its button row is kept out of the scroll"));
    let frame = harness.snapshot();
    let mut resting = Vec::new();
    for name in names {
        let control = frame
            .control(target, name)
            .unwrap_or_else(|| panic!("{name} is drawn"));
        assert_eq!(control.part, Part::Footer, "{name} is not in the footer");
        assert!(footer.contains_rect(control.rect), "{name} is not in the footer");
        assert!(control.visible, "{name}");
        assert!(record.rect.contains_rect(control.rect), "{name} is off the dialog");
        resting.push(control.rect);
    }
    // The body scrolled right down: the footer's controls stay where they were. The last control
    // of the body is scrolled to - not a record of a region (ControlKind::Other, the road object
    // dialog's own roadObjectBody), which takes no act.
    let last_body = frame
        .in_target(target)
        .filter(|control| control.part == Part::Body && control.kind != ControlKind::Other)
        .last()
        .map(|control| control.name.clone());
    if let Some(last) = last_body {
        harness
            .target(target)
            .scroll_to(&last)
            .run(&mut *draw)
            .expect("the dialog scrolls");
        let after = harness.snapshot();
        for (name, before) in names.iter().zip(&resting) {
            let now = after
                .control(target, name)
                .unwrap_or_else(|| panic!("{name} is drawn"));
            assert_eq!(now.rect, *before, "{name} moved with the body");
        }
    }
}

mod screen_fit_dialogs {
    use super::*;
    use katana_window::bim::road_object_dialog::RoadObjectDialog;
    use katana_window::terrain::surface_edit_dialogs::{
        SurfaceCombineDialog, SurfaceLevelsDialog, SurfaceLevelsKind,
    };
    use katana_window::terrain::volumes_dialog::VolumesDialog;

    #[test]
    fn surface_edit_heights_keeps_preview_run_close_and_the_reply_in_sight() {
        let mut host = answering();
        let mut dialog = SurfaceLevelsDialog::new(SurfaceLevelsKind::Heights, &mut host);
        dialog.frame_mut().set_minimum_height(TALLER);
        dialog.show(&host);
        expect_in_sight(
            "surfaceHeightsDialog",
            &[
                "surfaceHeightsPreview",
                "surfaceHeightsRun",
                "surfaceHeightsClose",
                "surfaceHeightsReply",
            ],
            &mut |ui| dialog.ui(ui, &mut host),
        );
    }

    #[test]
    fn surface_edit_delete_points_keeps_its_buttons_and_reply_in_sight_below_its_scope() {
        // The scope's own controls are in the body, above; the buttons are not scrolled away
        // behind them.
        let mut host = answering();
        let mut dialog = SurfaceLevelsDialog::new(SurfaceLevelsKind::DeletePoints, &mut host);
        dialog.frame_mut().set_minimum_height(TALLER);
        dialog.show(&host);
        expect_in_sight(
            "surfaceDeletePointsDialog",
            &[
                "surfaceDeletePointsPreview",
                "surfaceDeletePointsRun",
                "surfaceDeletePointsClose",
                "surfaceDeletePointsReply",
            ],
            &mut |ui| dialog.ui(ui, &mut host),
        );
    }

    #[test]
    fn combine_surfaces_keeps_preview_run_close_and_the_reply_in_sight() {
        let mut host = answering();
        let mut dialog = SurfaceCombineDialog::new(&mut host);
        dialog.frame_mut().set_minimum_height(TALLER);
        dialog.show(&host);
        expect_in_sight(
            "surfaceCombineDialog",
            &[
                "surfaceCombinePreview",
                "surfaceCombineRun",
                "surfaceCombineClose",
                "surfaceCombineReply",
            ],
            &mut |ui| dialog.ui(ui, &mut host),
        );
    }

    #[test]
    fn volumes_keeps_its_close_button_in_sight() {
        let mut host = answering();
        let mut dialog = VolumesDialog::new(&mut host);
        dialog.frame_mut().set_minimum_height(TALLER);
        dialog.show(&host);
        expect_in_sight("volumesDialog", &["volumesClose"], &mut |ui| {
            dialog.ui(ui, &mut host);
        });
    }

    #[test]
    fn road_objects_keeps_preview_add_close_and_what_they_reply_in_sight() {
        // The placing dialog's line, Preview, Add and Close, then the reply and the status line:
        // one footer, as the dialog made its own scrolling body for them.
        let mut host = answering();
        let document = Document::new(None);
        let mut dialog = RoadObjectDialog::new(&document);
        dialog.frame_mut().set_minimum_height(TALLER);
        dialog.show(host.document());
        expect_in_sight(
            "roadObjectDialog",
            &[
                "roadObjectLine",
                "roadObjectPreviewRun",
                "roadObjectAdd",
                "roadObjectClose",
                "roadObjectReply",
                "roadObjectStatus",
            ],
            &mut |ui| dialog.ui(ui, &mut host),
        );
    }
}
