//! GIS > Processing - GDAL > Formats... (geo/formats_dialog.hpp; docs/interop.md, "Formats"):
//! every format this build of GDAL reads and writes, and a driver's options, as the FORMATS verb
//! answers - read-only, non-modal, changing nothing.
//!
//! The table is the verb's: the dialog builds the FORMATS line its choices make, shows it
//! (gisFormatsCommand), and fills the table from the records the one geoprocessing executor
//! prepares for it (`Host::geo`). FORMATS changes nothing and answers at prepare, so nothing runs
//! as a job and the command log is not filled with a hundred records at every keystroke;
//! gisFormatsRun puts the line and its records in the log through the window's executor
//! (`Host::run`), for a person who wants them there or in a script.
//!
//! Object names:
//!
//! ```text
//! gisFormatsDialog      the dialog (action gisFormats)
//! gisFormatsKind        Raster and vector | Raster | Vector
//! gisFormatsCapability  Reads or writes | Reads | Writes
//! gisFormatsFilter      words each row holds: a name, a description, an extension
//! gisFormatsTable       Driver | Description | Kinds | Reads | Writes | Extensions | /vsi
//! gisFormatsCount       how many formats the line lists, and GDAL's version
//! gisFormatsCommand     the FORMATS line the table shows
//! gisFormatsOptions     the selected driver's options (FORMATS OPTIONS <driver>):
//!                       List | Name | Type | Default | Choices | Description
//! gisFormatsRun         runs the line through the window's executor
//! gisFormatsClose       close
//! ```
//!
//! The table follows its choices at the dialog's next look - the frame after a choice changed -
//! where the C++ refilled it inside the change's signal.

use egui::Ui;
use katana_app::geo::prepare;
use katana_app::geo::replies::{Record, parse_records};

use super::buffer_dialog::Words;
use super::geo_workbench::GeoMenus;
use crate::annotation::dialog_support::table;
use crate::drive::{Action, LineOptions, button, label, line_with};
use crate::host::{Host, with_geo};
use crate::shell::DialogFrame;

/// The formats table's columns.
const FORMAT_HEADERS: [&str; 7] = [
    "Driver",
    "Description",
    "Kinds",
    "Reads",
    "Writes",
    "Extensions",
    "/vsi",
];

/// The options table's columns.
const OPTION_HEADERS: [&str; 6] = ["List", "Name", "Type", "Default", "Choices", "Description"];

/// The body's width: the C++ dialog's 980 less its frame.
const BODY_WIDTH: f32 = 950.0;

/// What the one executor prepares for a line that answers at once: its records, or the refusal
/// as a record of its own kind.
fn records_of(host: &mut dyn Host, line: &str) -> Vec<Record> {
    match with_geo(host, |context| prepare(context, line)) {
        Err(error) => vec![Record {
            kind: String::from("error"),
            fields: vec![(String::from("text"), error.describe())],
            body: Vec::new(),
        }],
        Ok(prepared) => match &prepared.reply {
            Some(reply) => parse_records(reply),
            None => Vec::new(),
        },
    }
}

/// A word of the line: FORMATS reads RASTER, WRITE ... as its own words, so a filter word that is
/// one of them is quoted to stay text. A quote cannot be said in a line at all, so it is dropped.
fn filter_word(word: &str) -> String {
    let word = word.replace('"', "");
    for keyword in ["RASTER", "VECTOR", "READ", "WRITE", "JSON", "OPTIONS"] {
        if word.eq_ignore_ascii_case(keyword) {
            return format!("\"{word}\"");
        }
    }
    word
}

fn field(record: &Record, key: &str) -> String {
    String::from(record.get(key).unwrap_or(""))
}

/// The menu item, action gisFormats under GIS > Processing - GDAL: it shows the one FormatsDialog
/// of the window, made the first time and kept (`KeptFormatsDialog`). The action names the dialog
/// it shows, which is how --dialog finds it.
pub fn add_formats_item(menus: &mut GeoMenus) {
    let action = Action::new("gisFormats", "&Formats...")
        .icon("Processing")
        .tip(
            "Every format this build of GDAL reads and writes, and each driver's options \
             (FORMATS)",
        )
        .opens("gisFormatsDialog");
    menus.add_to_gis("Processing - GDAL", action);
}

/// The window's one Formats dialog: made the first time its action is triggered, and kept, as
/// the window's other non-modal ones are.
#[derive(Default)]
pub struct KeptFormatsDialog {
    dialog: Option<FormatsDialog>,
}

impl KeptFormatsDialog {
    pub fn new() -> Self {
        Self::default()
    }

    /// What the action gisFormats does: the dialog made the first time, and shown.
    pub fn trigger(&mut self, host: &mut dyn Host) {
        let dialog = self
            .dialog
            .get_or_insert_with(|| FormatsDialog::new(&mut *host));
        dialog.show();
    }

    /// The dialog, once it has been made.
    pub fn dialog(&self) -> Option<&FormatsDialog> {
        self.dialog.as_ref()
    }

    pub fn dialog_mut(&mut self) -> Option<&mut FormatsDialog> {
        self.dialog.as_mut()
    }

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if let Some(dialog) = &mut self.dialog {
            dialog.ui(ui, host);
        }
    }
}

/// GIS > Processing - GDAL > Formats... (see the module comment).
pub struct FormatsDialog {
    frame: DialogFrame,
    visible: bool,
    kind: Words,
    capability: Words,
    filter: String,
    /// The line the table shows.
    command: String,
    rows: Vec<Vec<String>>,
    current: Option<usize>,
    count: String,
    options: Vec<Vec<String>>,
    option: Option<usize>,
}

impl FormatsDialog {
    /// The dialog over the window's executor, its table filled.
    pub fn new(host: &mut dyn Host) -> Self {
        let mut dialog = Self {
            frame: DialogFrame::new("gisFormatsDialog", "GDAL Formats"),
            visible: false,
            kind: Words::new(&["Raster and vector", "Raster", "Vector"]),
            capability: Words::new(&["Reads or writes", "Reads", "Writes"]),
            filter: String::new(),
            command: String::new(),
            rows: Vec::new(),
            current: None,
            count: String::new(),
            options: Vec::new(),
            option: None,
        };
        dialog.refresh(host);
        dialog
    }

    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    /// QDialog::isModal: never.
    pub fn is_modal(&self) -> bool {
        self.frame.is_modal()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// The FORMATS line the choices make.
    pub fn line(&self) -> String {
        let mut text = String::from("FORMATS");
        match self.kind.chosen.index {
            Some(1) => text.push_str(" RASTER"),
            Some(2) => text.push_str(" VECTOR"),
            _ => {}
        }
        match self.capability.chosen.index {
            Some(1) => text.push_str(" READ"),
            Some(2) => text.push_str(" WRITE"),
            _ => {}
        }
        for word in self.filter.split(' ').filter(|word| !word.is_empty()) {
            text.push(' ');
            text.push_str(&filter_word(word));
        }
        text
    }

    /// Rows of the formats table.
    pub fn format_count(&self) -> usize {
        self.rows.len()
    }

    /// The formats table's rows, a cell a column.
    pub fn formats(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// The options table's rows: the selected driver's.
    pub fn options(&self) -> &[Vec<String>] {
        &self.options
    }

    /// What gisFormatsCount says.
    pub fn count_text(&self) -> &str {
        &self.count
    }

    /// The line gisFormatsCommand shows: the one the table was filled from.
    pub fn command_text(&self) -> &str {
        &self.command
    }

    /// Selects the row `row` of the formats table, as a click on it does, and shows its driver's
    /// options.
    pub fn select_row(&mut self, host: &mut dyn Host, row: usize) {
        if row < self.rows.len() {
            self.current = Some(row);
            self.show_options(host);
        }
    }

    fn refresh(&mut self, host: &mut dyn Host) {
        let text = self.line();
        self.command.clone_from(&text);
        let records = records_of(host, &text);
        // The rows are replaced, and none is selected: in the registry's order (by driver).
        self.rows.clear();
        self.current = None;
        self.count.clear();
        for record in &records {
            if record.kind == "error" {
                self.count = field(record, "text");
                continue;
            }
            if record.kind == "listed" {
                let formats = field(record, "formats");
                self.count = format!(
                    "{formats} {} (GDAL {})",
                    if formats == "1" { "format" } else { "formats" },
                    field(record, "gdal")
                );
                continue;
            }
            if record.kind != "format" {
                continue;
            }
            let yes_no = |kinds: String| {
                if kinds == "no" { String::new() } else { kinds }
            };
            self.rows.push(vec![
                field(record, "driver"),
                field(record, "description"),
                field(record, "kind"),
                yes_no(field(record, "read")),
                yes_no(field(record, "write")),
                field(record, "extensions").replace(',', " "),
                field(record, "vsi"),
            ]);
        }
        self.show_options(host);
    }

    fn show_options(&mut self, host: &mut dyn Host) {
        self.options.clear();
        self.option = None;
        let Some(driver) = self
            .current
            .and_then(|row| self.rows.get(row))
            .and_then(|row| row.first())
        else {
            return;
        };
        let name = driver.replace('"', "");
        for record in records_of(host, &format!("FORMATS OPTIONS \"{name}\"")) {
            if record.kind != "option" {
                continue;
            }
            self.options.push(vec![
                field(&record, "list"),
                field(&record, "name"),
                field(&record, "type"),
                field(&record, "default"),
                field(&record, "choices").replace(',', " "),
                field(&record, "description"),
            ]);
        }
    }

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        // The table is the line its choices make now.
        if self.line() != self.command {
            self.refresh(host);
        }
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent: (&mut Self, &mut dyn Host) = (&mut *self, &mut *host);
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| lent.0.body(ui, &mut *lent.1),
            |ui, lent| lent.0.footer(ui, &mut *lent.1),
        );
        self.frame = frame;
        if shown.closed {
            self.hide();
        }
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.set_max_width(BODY_WIDTH);
        ui.horizontal(|ui| {
            crate::drive::choice(ui, "gisFormatsKind", &self.kind.items, &mut self.kind.chosen);
            crate::drive::choice(
                ui,
                "gisFormatsCapability",
                &self.capability.items,
                &mut self.capability.chosen,
            );
            line_with(
                ui,
                "gisFormatsFilter",
                &mut self.filter,
                LineOptions {
                    placeholder: "Words to look for: a name, a description, an extension (fgb)",
                    width: Some(f32::INFINITY),
                    ..LineOptions::default()
                },
            );
        });
        let rows = &self.rows;
        let current = self.current;
        let chosen = egui::ScrollArea::both()
            .id_salt("gisFormatsTable")
            .max_height(300.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                table(
                    ui,
                    "gisFormatsTable",
                    &FORMAT_HEADERS,
                    rows,
                    current,
                    &[],
                    None,
                )
            })
            .inner
            .chosen;
        if let Some(row) = chosen
            && self.current != Some(row)
        {
            self.current = Some(row);
            self.show_options(host);
        }
        let options = &self.options;
        let option = self.option;
        let chosen = egui::ScrollArea::both()
            .id_salt("gisFormatsOptions")
            .max_height(200.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                table(
                    ui,
                    "gisFormatsOptions",
                    &OPTION_HEADERS,
                    options,
                    option,
                    &[],
                    None,
                )
            })
            .inner
            .chosen;
        if chosen.is_some() {
            self.option = chosen;
        }
    }

    /// The count, the line, Run and Close: kept below the scrolling body.
    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal(|ui| {
            label(ui, "gisFormatsCount", &self.count);
            label(ui, "gisFormatsCommand", &self.command).on_hover_text(
                "The line this table shows; type it, or put it in a script, for the same records",
            );
            if button(ui, "gisFormatsRun", "Run in Command Line")
                .on_hover_text(
                    "Run the line through the command line, so the records are in its log",
                )
                .clicked()
            {
                let line = self.line();
                let _ = host.run(&line);
            }
            if button(ui, "gisFormatsClose", "Close").clicked() {
                self.hide();
            }
        });
    }
}
