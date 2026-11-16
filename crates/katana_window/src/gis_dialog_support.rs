//! What the GIS menu's option dialogs share (gis_dialog_support.hpp; gis_import_dialogs,
//! gis_export_dialog): their row of Close and the act.

use egui::Ui;

use crate::drive::{button, primary};

/// What the OK and Cancel row did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OkCancel {
    /// OK was pressed (QDialog::accept).
    pub accepted: bool,
    /// Cancel was pressed (QDialog::reject).
    pub rejected: bool,
}

/// The object names of a dialog's OK and Cancel: the dialog's own without "Dialog", then Run and
/// Cancel (vectorImportRun, vectorImportCancel), so a headless run can press them
/// (docs/headless.md) as it presses every GIS dialog's Run.
pub fn ok_cancel_names(dialog: &str) -> (String, String) {
    let prefix = dialog.strip_suffix("Dialog").unwrap_or(dialog);
    (format!("{prefix}Run"), format!("{prefix}Cancel"))
}

/// OK, named for what it does ("Import", "Export") and enabled as `ok_enabled` says, and Close
/// (the Cancel of the C++, named so still), for the dialog whose object name is `dialog`. It reads
/// "Close" as every dialog's way out does: an option dialog changes nothing until its act is
/// pressed, so there is nothing for a Cancel to undo, and five words for leaving a dialog read as
/// five conventions (the third look audit).
pub fn ok_cancel(ui: &mut Ui, dialog: &str, ok_text: &str, ok_enabled: bool) -> OkCancel {
    ok_cancel_beside(ui, dialog, ok_text, ok_enabled, |_| {}).1
}

/// `ok_cancel` with `leading` at the left of the row - what reads the file and changes nothing (a
/// Preview) and what it said - as the design's one footer has it (shell::dialog::button_row).
pub fn ok_cancel_beside<L>(
    ui: &mut Ui,
    dialog: &str,
    ok_text: &str,
    ok_enabled: bool,
    leading: impl FnOnce(&mut Ui) -> L,
) -> (L, OkCancel) {
    let (run, cancel) = ok_cancel_names(dialog);
    // At the right: Close, then OK - the dialog's act - last, in the accent.
    crate::shell::dialog::button_row(ui, leading, |ui| {
        let rejected = button(ui, &cancel, "Close").clicked();
        let accepted = ui
            .add_enabled_ui(ok_enabled, |ui| primary(ui, &run, ok_text).clicked())
            .inner;
        OkCancel { accepted, rejected }
    })
}
