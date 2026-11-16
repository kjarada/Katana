//! Help > About Katana (main_window::panels::AboutDialog): a dialog of the window's own frame, where
//! it was the platform's warning box - its version, its words and its Close, driven by name.

use katana_window::drive::Harness;
use katana_window::main_window::panels::AboutDialog;

mod about_dialog {
    use super::*;

    fn drawn(dialog: &mut AboutDialog, harness: &mut Harness) {
        harness.target("aboutDialog");
        harness
            .show(|ui| dialog.ui(&ui.ctx().clone()))
            .expect("the dialog is drawn");
    }

    #[test]
    fn shows_the_programs_version_and_closes_from_its_close_button() {
        let mut dialog = AboutDialog::new();
        assert!(!dialog.is_open(), "made, it is not shown until asked");
        dialog.show();
        let mut harness = Harness::new();
        drawn(&mut dialog, &mut harness);
        let text = harness.report("aboutText").expect("its words are a named label");
        let version = katana_update::version::program_version_text();
        assert!(
            text.contains(&format!("Version {version}")),
            "{text}"
        );
        assert!(harness.control("aboutClose").is_some());
        harness.press("aboutClose");
        harness
            .run(|ui| dialog.ui(&ui.ctx().clone()))
            .expect("the press is taken");
        assert!(!dialog.is_open(), "Close hides it");
    }

    #[test]
    fn a_dialog_not_asked_for_draws_nothing() {
        let mut dialog = AboutDialog::new();
        let mut harness = Harness::new();
        harness.frame(|ui| dialog.ui(&ui.ctx().clone()));
        assert!(harness.control("aboutClose").is_none());
    }
}
