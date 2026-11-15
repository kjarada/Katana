//! BIM > Corridor Quantities... (docs/road_design.md, "Quantities"): the road model's quantities -
//! each course's volume and area, each kerb's length, the earthworks cut and fill - for one
//! corridor or all, with totals by kind and material, copied or saved as CSV. The port of
//! bim/corridor_quantities_dialog.
//!
//! It runs CORRIDOR QUANTITIES <corridor>|ALL through the window's executor and shows the records it
//! replies, so the table and the verb cannot disagree; Save CSV runs the same line with csv=<file>.
//! A headless run opens no file dialog: the path is typed into its field.
//!
//! THE LISTING IS CLEARED when the drawing changes under it: an edit, an undo, a redo, a new or
//! opened drawing that leaves the road model it was listed from (bim::bim_model, what the verb
//! reads) no longer the drawing's. The status then says to press List Quantities again, and Copy
//! hands out nothing. It is not listed again by itself, which would put a CORRIDOR QUANTITIES line
//! in the command log after every edit; a change that left the road model as it was (a line drawn)
//! leaves the listing as it is.
//!
//! Object names:
//!
//! ```text
//! corridorQuantitiesDialog    the dialog (the action bimCorridorQuantities' data)
//! corridorQuantitiesCorridor  All, then each corridor
//! corridorQuantitiesRun       CORRIDOR QUANTITIES <corridor>|ALL
//! corridorQuantitiesTable     Object, Kind, Length, Width, Depth, Height, Area, Volume
//! corridorQuantitiesTotals    the cut, fill and net, and the totals by kind and material
//! corridorQuantitiesCsvPath   the file Save CSV writes
//! corridorQuantitiesBrowse    a file dialog for it (none in a headless run)
//! corridorQuantitiesSaveCsv   CORRIDOR QUANTITIES <corridor>|ALL csv="<file>" [OVERWRITE]
//! corridorQuantitiesOverwrite replace a file already there (the line gets OVERWRITE)
//! corridorQuantitiesCopy      the table to the clipboard, tab separated
//! corridorQuantitiesStatus    what the last line replied, or why it was refused
//! corridorQuantitiesClose
//! ```

use std::sync::Arc;

use egui::Ui;
use katana_cad::Document;
use katana_cad::bim::bim_object::{BimModel, bim_model};
use katana_core::text::read_reply_record;

use super::road_dialog_support::{
    Combo, StatusLine, Table, button, cell, field, muted, option, reply_lines, save_file_dialog,
    word,
};
use crate::customisation::document_watcher::DocumentWatcher;
use crate::drive::controls::check;
use crate::host::Host;
use crate::shell::dialog::DialogFrame;

/// The first item of the corridor choice: every corridor.
pub const ALL: &str = "All corridors";

/// The quantities a row shows, by the names the records carry them under.
const QUANTITIES: [&str; 6] = [
    "Length",
    "Width",
    "Depth",
    "Height",
    "GrossArea",
    "GrossVolume",
];

struct Body {
    watcher: DocumentWatcher,
    executor: bool,
    corridor: Combo,
    table: Table,
    totals: String,
    csv_path: String,
    overwrite: bool,
    status: StatusLine,
    /// The road model the table and the totals were listed from: the object CORRIDOR QUANTITIES
    /// reads (bim::bim_model), which is the same object for as long as nothing in the road model
    /// changes. A different one means the listing is of a drawing that is no longer there.
    listed_from: Option<Arc<BimModel>>,
    close: bool,
}

impl Body {
    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    fn which(&mut self) -> Option<String> {
        let shown = self.corridor.current_text();
        if shown.is_empty() || shown == ALL {
            return Some(String::from("ALL"));
        }
        word(&mut self.status, &shown, "the corridor's name")
    }

    /// Runs the line and shows its records: an object a row, the rest under it.
    fn show(&mut self, host: &mut dyn Host, line: &str) {
        if !self.alive() || !self.executor {
            self.status
                .set(format!("Nothing here can run {line}."), true);
            return;
        }
        let outcome = host.run(line);
        if !outcome.ok {
            if outcome.error.is_empty() {
                self.status.set(format!("{line} was refused."), true);
            } else {
                self.status.set(outcome.error.as_str(), true);
            }
            return;
        }
        self.table.clear();
        self.totals.clear();
        let mut summary: Vec<String> = Vec::new();
        for text in reply_lines(&outcome.reply) {
            let Some(record) = read_reply_record(text) else {
                continue;
            };
            let Some(kind) = record.words.first() else {
                continue;
            };
            let value = |key: &str| String::from(record.value(key).unwrap_or(""));
            match kind.as_str() {
                "object" => {
                    let mut cells = vec![cell(value("id")), cell(value("kind"))];
                    cells.extend(QUANTITIES.iter().map(|&name| cell(value(name))));
                    self.table.push_row(cells);
                }
                "quantities" => {
                    // The record leaves cut, fill and net empty when no earthworks were measured
                    // (no target surface): not a road with no cut.
                    let measured = value("cut");
                    let said = if measured.is_empty() {
                        format!(
                            "Cut and fill not measured (they need a target surface) over {} \
                             objects",
                            value("objects")
                        )
                    } else {
                        format!(
                            "Cut {measured} m3, fill {} m3, net {} m3 (fill less cut) over {} \
                             objects",
                            value("fill"),
                            value("net"),
                            value("objects")
                        )
                    };
                    summary.insert(0, said);
                }
                "total" => summary.push(format!(
                    "{} {}: {} of them, {} m long, {} m2, {} m3",
                    value("kind"),
                    value("material"),
                    value("count"),
                    value("length"),
                    value("area"),
                    value("volume")
                )),
                "problem" => summary.push(format!("Problem: {}", value("message"))),
                _ => {}
            }
        }
        self.totals = summary.join("\n");
        // The line has run and read the road model; the model it read is the one the drawing
        // holds now.
        self.listed_from = Some(bim_model(host.document()));
        let lines = reply_lines(&outcome.reply);
        self.status
            .set(lines.last().copied().unwrap_or(line), false);
    }

    /// After a change to the drawing - an edit, an undo, a redo, a new or opened drawing - the
    /// listing is cleared and the status says to list again when the road model it was listed from
    /// is no longer the drawing's. It is not listed again by itself: that would be a CORRIDOR
    /// QUANTITIES line in the command log after every edit, and a change that left the road model
    /// as it was (a line drawn) leaves the listing alone. True when it cleared.
    fn drop_if_stale(&mut self, document: &Document) -> bool {
        if !self.alive() {
            return false;
        }
        let Some(listed) = &self.listed_from else {
            return false;
        };
        if Arc::ptr_eq(&bim_model(document), listed) {
            return false;
        }
        self.listed_from = None;
        self.table.clear();
        self.totals.clear();
        self.status.set(
            "The drawing changed since the list: press List Quantities again.",
            true,
        );
        true
    }

    fn reload(&mut self, document: &Document) {
        if !self.alive() {
            return;
        }
        let keep = self.corridor.current_text();
        self.corridor.clear();
        self.corridor.add_item(ALL);
        for each in &document.corridors().corridors {
            self.corridor.add_item(&each.name);
        }
        let at = self.corridor.find_text(&keep).unwrap_or(0);
        self.corridor.set_current_index(Some(at));
    }

    fn press_run(&mut self, host: &mut dyn Host) {
        if let Some(corridor_word) = self.which() {
            self.show(host, &format!("CORRIDOR QUANTITIES {corridor_word}"));
        }
    }

    fn press_browse(&mut self, host: &mut dyn Host) {
        if host.headless() {
            self.status.set(
                "A headless session opens no file dialog: type the path in the field",
                true,
            );
            return;
        }
        if let Some(chosen) = save_file_dialog("Save Quantities", &self.csv_path, "CSV", "csv") {
            self.csv_path = chosen;
        }
    }

    fn press_save(&mut self, host: &mut dyn Host) {
        let Some(corridor_word) = self.which() else {
            return;
        };
        let path = String::from(self.csv_path.trim());
        if path.is_empty() {
            self.status
                .set("Type the file to save, or Browse for it.", true);
            return;
        }
        if let Some(file) = option(&mut self.status, "csv", &path) {
            let overwrite = if self.overwrite { " OVERWRITE" } else { "" };
            self.show(
                host,
                &format!("CORRIDOR QUANTITIES {corridor_word} {file}{overwrite}"),
            );
        }
    }

    fn press_copy(&mut self, host: &mut dyn Host) {
        // The watcher's delivery may not have run yet: never hand out a listing of a road model
        // the drawing no longer has.
        if self.drop_if_stale(host.document()) {
            return;
        }
        let text = self.table.tab_separated().join("\n");
        // QClipboard::setText cannot fail; a clipboard here can (the desktop's), and says why.
        if let Err(error) = host.clipboard().set_text(&text) {
            self.status.set(error.describe(), true);
            return;
        }
        self.status
            .set(format!("{} rows copied.", self.table.row_count()), false);
    }

    fn body_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal(|ui| {
            self.corridor.ui(ui, "corridorQuantitiesCorridor");
            if button(
                ui,
                "corridorQuantitiesRun",
                "List Quantities",
                "CORRIDOR QUANTITIES: every object's quantities and the totals",
            )
            .clicked()
            {
                self.press_run(host);
            }
        });
        self.table.ui(ui);
        muted(ui, "corridorQuantitiesTotals", &self.totals);
        ui.horizontal(|ui| {
            field(
                ui,
                "corridorQuantitiesCsvPath",
                &mut self.csv_path,
                "quantities.csv",
                "The file Save CSV writes",
            );
            if button(
                ui,
                "corridorQuantitiesBrowse",
                "Browse...",
                "Choose the file (no file dialog in a headless run)",
            )
            .clicked()
            {
                self.press_browse(host);
            }
            if button(
                ui,
                "corridorQuantitiesSaveCsv",
                "Save CSV",
                "CORRIDOR QUANTITIES ... csv=: the table as a CSV file",
            )
            .clicked()
            {
                self.press_save(host);
            }
            if button(
                ui,
                "corridorQuantitiesCopy",
                "Copy",
                "The table to the clipboard, tab separated",
            )
            .clicked()
            {
                self.press_copy(host);
            }
        });
        check(
            ui,
            "corridorQuantitiesOverwrite",
            "Replace the file if it is there (OVERWRITE)",
            &mut self.overwrite,
        )
        .on_hover_text(
            "A file already there is only replaced with OVERWRITE, as every report verb writes it",
        );
        self.status.ui(ui, "corridorQuantitiesStatus");
    }

    fn footer_ui(&mut self, ui: &mut Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, "corridorQuantitiesClose", "Close", "Close the dialog").clicked() {
                self.close = true;
            }
        });
    }
}

/// The Corridor Quantities dialog (see the module comment).
pub struct CorridorQuantitiesDialog {
    frame: DialogFrame,
    open: bool,
    body: Body,
}

impl CorridorQuantitiesDialog {
    /// The dialog over `document`, shown, All chosen.
    pub fn new(document: &Document) -> Self {
        let mut body = Body {
            watcher: DocumentWatcher::new(document),
            executor: true,
            corridor: Combo::new(),
            table: Table::new(
                "corridorQuantitiesTable",
                &[
                    "Object",
                    "Kind",
                    "Length (m)",
                    "Width (m)",
                    "Depth (m)",
                    "Height (m)",
                    "Area (m2)",
                    "Volume (m3)",
                ],
            )
            .max_height(320.0),
            totals: String::new(),
            csv_path: String::new(),
            overwrite: false,
            status: StatusLine::default(),
            listed_from: None,
            close: false,
        };
        body.reload(document);
        Self {
            frame: DialogFrame::new("corridorQuantitiesDialog", "Corridor Quantities"),
            open: true,
            body,
        }
    }

    /// A dialog with no executor (the C++'s empty CommandRunner): it says it cannot run a line.
    pub fn without_executor(mut self) -> Self {
        self.body.executor = false;
        self
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(&mut self) {
        self.open = true;
    }

    pub fn hide(&mut self) {
        self.open = false;
    }

    /// The table of objects (corridorQuantitiesTable).
    pub fn table(&self) -> &Table {
        &self.body.table
    }

    /// Draws the dialog for this frame, when it is shown; then the watcher's delivery, as the
    /// C++'s event loop delivered it after a click (corridor_dialog's `ui` says why).
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if self.open {
            let ctx = ui.ctx().clone();
            let mut lent = (&mut self.body, &mut *host);
            let shown = self.frame.show(
                &ctx,
                &mut lent,
                |ui, lent| lent.0.body_ui(ui, &mut *lent.1),
                |ui, lent| lent.0.footer_ui(ui),
            );
            if shown.closed || std::mem::take(&mut self.body.close) {
                self.open = false;
            }
        }
        if let Some(changes) = self.body.watcher.deliver(host.document())
            && changes.model
        {
            self.body.reload(host.document());
            self.body.drop_if_stale(host.document());
            ui.ctx().request_repaint();
        }
    }
}
