//! What the GIS menu's option dialogs share (gis_dialog_support.hpp; gis_import_dialogs,
//! gis_export_dialog): their OK and Cancel row.

use egui::Ui;

use crate::drive::button;

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

/// OK, named for what it does ("Import", "Export") and enabled as `ok_enabled` says, and Cancel,
/// for the dialog whose object name is `dialog`.
pub fn ok_cancel(ui: &mut Ui, dialog: &str, ok_text: &str, ok_enabled: bool) -> OkCancel {
    let (run, cancel) = ok_cancel_names(dialog);
    ui.horizontal(|ui| {
        let accepted = ui
            .add_enabled_ui(ok_enabled, |ui| button(ui, &run, ok_text).clicked())
            .inner;
        let rejected = button(ui, &cancel, "Cancel").clicked();
        OkCancel { accepted, rejected }
    })
    .inner
}
