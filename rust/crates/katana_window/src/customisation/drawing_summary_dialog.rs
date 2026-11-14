//! File > Drawing Summary: the drawing at a glance, kept live beside it (docs/desktop.md, "File >
//! Drawing Summary"). The port of customisation/drawing_summary_dialog.
//!
//! What STATUS reports (cad's document_status) and more that a person wants in view and the window
//! showed nowhere: the project's full path, what the drawing holds (entities, layers, alignments,
//! sheets), what is current (layer, style, annotation scale, coordinate system), the selection,
//! the history with the next step each way, and the customisation - what a bare CUSTOMISE replies
//! (cad's format_customisation_reply of the Document's own state): the counts, its name and where
//! it came from, its sources in load order, the automation and the linework codes, the names the
//! project was drawn with that are not loaded, and this drawing's coverage - with the names no
//! loaded library defines listed: a double-click, or Show in Styles and Linetypes, opens Format >
//! Styles and Linetypes on its Missing chip, searching for that name.
//!
//! It changes nothing. Copy as JSON runs STATUS JSON through the window's one executor and copies
//! the reply, so what is copied is exactly what an agent reads. It had a Load Customisation
//! button, which triggered the Format menu's item of that name; the item went with the files it
//! loaded, and the button with it rather than be left triggering a name that is no longer there. A
//! customisation is loaded in File > Settings (settings_dialog) or by the CUSTOMISE line, and the
//! heading over the report says so. The dialog follows the Document through a DocumentWatcher,
//! refreshing once a frame however many commands ran.
//!
//! Object names:
//!
//! ```text
//! drawingSummaryDialog              the dialog (fileDrawingSummary)
//! drawingSummaryProject             the project's full path and whether it is saved
//! drawingSummaryDrawing             entities, layers, alignments, sheets
//! drawingSummaryCurrent             layer, style, annotation scale, CRS
//! drawingSummarySelection           how many are selected
//! drawingSummaryHistory             undo and redo depth, and the next of each
//! drawingSummaryCustomisationLabel  its heading, which names File > Settings
//! drawingSummaryCustomisation       the customisation report (read-only)
//! drawingSummaryUnresolved          the names no loaded library defines; a double-click opens
//!                                   the style manager at Missing on it
//! drawingSummaryShowMissing         the same for the chosen name
//! drawingSummaryCopyJson            copy STATUS JSON
//! drawingSummaryRefresh             read everything again
//! drawingSummaryStatus              what happened last
//! drawingSummaryClose               close
//! ```

use std::rc::Rc;

use egui::{Context, Ui};
use katana_cad::customisation_report::{customisation_summary, format_customisation_reply};
use katana_cad::document::Document;
use katana_cad::document_status::document_status;
use katana_core::text::format_general;

use super::document_watcher::DocumentWatcher;
use crate::annotation::dialog_support::show_dialog;
use crate::drive::controls::{self, TextOptions, custom};
use crate::drive::mailbox::{Act, fill_disabled, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, settle, take};
use crate::host::Host;
use crate::shell::DialogFrame;

const DIALOG: &str = "drawingSummaryDialog";
const TITLE: &str = "Drawing Summary";
/// The width the dialog is laid out at (the C++'s resize(640, 600), less its margins).
const BODY_WIDTH: f32 = 616.0;

/// What the dialog is given by the window, besides the host it is drawn with: the drawing and the
/// executor are the host's.
#[derive(Clone, Default)]
pub struct DrawingSummaryContext {
    /// False when nothing can run a line (the C++'s empty `run`): Copy as JSON then says so.
    pub has_runner: bool,
    /// Opens the style manager at its Missing chip, searching for the name.
    pub show_missing: Option<Rc<dyn Fn(&str)>>,
    /// The project's coordinate system as the status bar says it (projectCrsLabel); None, the
    /// project's own text, or that it has none.
    pub crs: Option<Rc<dyn Fn() -> String>>,
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// What a list did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ListShown {
    chosen: Option<usize>,
    activated: Option<usize>,
}

/// A QListWidget of names, named `name`: a click or a fill makes a row current, a double-click
/// activates it. Reported as main.cpp reports a list.
fn name_list(ui: &mut Ui, name: &str, rows: &[String], current: Option<usize>) -> ListShown {
    let mut shown = ListShown::default();
    let taken = take(ui, name);
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if ui.is_visible() && !ui.is_enabled() {
                    settle(ui, taken, Err(fill_disabled(&taken.posted)));
                } else if let Some(at) = rows.iter().position(|row| row == text) {
                    shown.chosen = Some(at);
                    settle(ui, taken, Ok(()));
                } else {
                    settle(ui, taken, Err(lists_no(name, text)));
                }
            }
            Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
            Act::ScrollTo => {}
        }
    }
    let drawn = ui.push_id(name, |ui| {
        for (at, row) in rows.iter().enumerate() {
            let response = ui.selectable_label(current == Some(at), row.as_str());
            if response.double_clicked() {
                shown.activated = Some(at);
            }
            if response.clicked() {
                shown.chosen = Some(at);
            }
        }
    });
    let response = drawn.response;
    if let Some(taken) = &taken
        && taken.posted.act == Act::ScrollTo
    {
        response.scroll_to_me(None);
        settle(ui, taken, Ok(()));
    }
    let mut record = ControlRecord::new(name, ControlKind::Rows);
    record.items = rows.to_vec();
    record.value = shown
        .chosen
        .or(current)
        .and_then(|at| rows.get(at))
        .cloned()
        .unwrap_or_default();
    custom(ui, &response, record);
    shown
}

/// See the module comment.
pub struct DrawingSummaryDialog {
    context: DrawingSummaryContext,
    frame: DialogFrame,
    project: String,
    drawing: String,
    current: String,
    selection: String,
    history: String,
    customisation: String,
    unresolved: Vec<String>,
    unresolved_current: Option<usize>,
    status: String,
    /// The Close button was pressed this frame.
    closing: bool,
    // Last, as in the C++: it goes with the dialog and delivers nothing to a half-dropped one.
    watcher: DocumentWatcher,
}

impl DrawingSummaryDialog {
    pub fn new(document: &Document, context: DrawingSummaryContext) -> Self {
        let mut dialog = Self {
            context,
            frame: DialogFrame::new(DIALOG, TITLE),
            project: String::new(),
            drawing: String::new(),
            current: String::new(),
            selection: String::new(),
            history: String::new(),
            customisation: String::new(),
            unresolved: Vec::new(),
            unresolved_current: None,
            status: String::new(),
            closing: false,
            watcher: DocumentWatcher::new(document),
        };
        dialog.refresh(document);
        dialog
    }

    /// Reads everything again: what the watcher and Refresh do. `document` is the Document the
    /// dialog was made with; nothing is read once it has gone.
    pub fn refresh(&mut self, document: &Document) {
        if !self.watcher.document_alive() {
            return;
        }
        let status = document_status(document);

        self.project = format!(
            "{}{}",
            status
                .project
                .as_deref()
                .unwrap_or("none - not saved to a project yet"),
            if status.modified {
                ", with unsaved changes"
            } else {
                ", saved"
            }
        );
        self.drawing = format!(
            "{}, {}, {}, {}",
            counted(status.entities, "entity", "entities"),
            counted(status.layers, "layer", "layers"),
            counted(status.alignments, "alignment", "alignments"),
            counted(document.sheet_set().sheets.len(), "sheet", "sheets")
        );
        let crs = match &self.context.crs {
            Some(crs) => crs(),
            None if document.metadata().coordinate_system.is_empty() => {
                String::from("no coordinate system")
            }
            None => document.metadata().coordinate_system.clone(),
        };
        self.current = format!(
            "layer {}, style {}, annotation scale 1:{}, {crs}",
            status.current_layer,
            if status.current_style.is_empty() {
                "ByLayer"
            } else {
                status.current_style.as_str()
            },
            format_general(document.annotation_scale(), 10)
        );
        self.selection = if status.selected == 0 {
            String::from("nothing selected")
        } else {
            format!("{} selected", counted(status.selected, "entity", "entities"))
        };
        let next = |name: &str| {
            if name.is_empty() {
                String::new()
            } else {
                format!(" (next: {name})")
            }
        };
        self.history = format!(
            "{} to undo{}, {} to redo{}",
            counted(status.undo_steps, "step", "steps"),
            next(document.history().undo_name()),
            counted(status.redo_steps, "step", "steps"),
            next(document.history().redo_name())
        );

        // What a bare CUSTOMISE replies, word for word, of the Document's own state: the pane and
        // the command line cannot come to say it differently.
        let summary = customisation_summary(document);
        let report = format_customisation_reply(&summary);
        self.customisation = String::from(report.trim_end_matches('\n'));
        // QListWidget::clear: nothing is chosen in the list read again.
        self.unresolved.clone_from(&summary.coverage.unresolved);
        self.unresolved_current = None;
    }

    /// What Copy as JSON does: STATUS JSON run, its reply copied and returned; empty, with the
    /// status saying why, when it was refused.
    pub fn copy_json(&mut self, host: &mut dyn Host) -> String {
        if !self.context.has_runner {
            self.status = String::from("Nothing here can run a command.");
            return String::new();
        }
        // Through the one executor, as typed: what is copied is what an agent reads from STATUS
        // JSON, katana_status or katana://status.
        let outcome = host.run("STATUS JSON");
        if !outcome.ok {
            self.status = format!("STATUS JSON was refused: {}", outcome.error);
            return String::new();
        }
        // QClipboard::setText answers nothing; a clipboard that cannot be written is not this
        // dialog's to report.
        let _ = host.clipboard().set_text(&outcome.reply);
        self.status = String::from("Copied the drawing's state as JSON (STATUS JSON).");
        outcome.reply
    }

    /// A name of the list activated - a double-click on it, or the button on the chosen one,
    /// which is how the headless driver, choosing a row with --fill, reaches it too.
    pub fn show_missing(&mut self, row: usize) {
        let (Some(name), Some(show)) = (self.unresolved.get(row), &self.context.show_missing)
        else {
            return;
        };
        show(name.as_str());
        self.status = format!("Styles and Linetypes shows the styles that name {name}.");
    }

    /// What happened last.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Draws the dialog for this frame - the drawing's changes since the last first; false once it
    /// is closed, by its Close button or its title bar's.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        if self.watcher.deliver(host.document()).is_some() {
            self.refresh(host.document());
        }
        let mut frame = std::mem::replace(&mut self.frame, DialogFrame::new(DIALOG, TITLE));
        let mut closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        closed |= std::mem::take(&mut self.closing);
        !closed
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        ui.set_width(BODY_WIDTH);
        egui::Grid::new("drawingSummaryForm")
            .num_columns(2)
            .show(ui, |ui| {
                let rows = [
                    ("Project:", "drawingSummaryProject", &self.project),
                    ("Drawing:", "drawingSummaryDrawing", &self.drawing),
                    ("Current:", "drawingSummaryCurrent", &self.current),
                    ("Selection:", "drawingSummarySelection", &self.selection),
                    ("History:", "drawingSummaryHistory", &self.history),
                ];
                for (title, name, text) in rows {
                    ui.label(title);
                    controls::label_as(ui, name, text, egui::Label::new(text.as_str()).wrap());
                    ui.end_row();
                }
            });
        // Says where the customisation is changed: the summary only shows it.
        controls::label(
            ui,
            "drawingSummaryCustomisationLabel",
            "Customisation (File > Settings loads, writes or resets it):",
        );
        // Wrapped at the pane's edge: the reply's records are longer than the pane is wide (the
        // linework record is 143 characters), and left unwrapped the end of each lay out of sight
        // behind a scroll bar. A record still begins its own line and starts with its word, so a
        // wrapped one reads as one.
        let mut report = self.customisation.clone();
        controls::text_with(
            ui,
            "drawingSummaryCustomisation",
            &mut report,
            TextOptions {
                read_only: true,
                rows: 8,
                monospace: true,
            },
        );
        ui.add(
            egui::Label::new(
                "Names the drawing's styles give that no loaded library defines - click one to \
                 see its styles among the Missing in Styles and Linetypes:",
            )
            .wrap(),
        );
        let shown = name_list(
            ui,
            "drawingSummaryUnresolved",
            &self.unresolved,
            self.unresolved_current,
        );
        if let Some(at) = shown.chosen {
            self.unresolved_current = Some(at);
        }
        if let Some(at) = shown.activated {
            self.show_missing(at);
        }
        ui.add_enabled_ui(self.unresolved_current.is_some(), |ui| {
            if controls::button(
                ui,
                "drawingSummaryShowMissing",
                "Show in Styles and Linetypes",
            )
            .on_hover_text(
                "Open Format > Styles and Linetypes on its Missing styles, searching for the \
                 chosen name",
            )
            .clicked()
                && let Some(at) = self.unresolved_current
            {
                self.show_missing(at);
            }
        });
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        controls::label_as(
            ui,
            "drawingSummaryStatus",
            &self.status,
            egui::Label::new(self.status.as_str()).wrap(),
        );
        ui.horizontal(|ui| {
            if controls::button(ui, "drawingSummaryCopyJson", "Copy as JSON")
                .on_hover_text("Copy the drawing's state as STATUS JSON gives it")
                .clicked()
            {
                self.copy_json(host);
            }
            if controls::button(ui, "drawingSummaryRefresh", "Refresh")
                .on_hover_text("Read the drawing's state again")
                .clicked()
            {
                self.refresh(host.document());
                self.status = String::from("Read again.");
            }
            if controls::button(ui, "drawingSummaryClose", "Close")
                .on_hover_text("Close the summary")
                .clicked()
            {
                self.closing = true;
            }
        });
    }
}
