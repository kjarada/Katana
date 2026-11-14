//! GIS > Dataset Information (dataset_info_dialog.hpp; docs/interop.md, "Dataset information"):
//! what a file holds, as the INFO line reads it. The dialog builds INFO lines and runs them
//! through the window's one executor (`Host::run`), as a person or an agent would type them; it
//! never reads the file itself. Opened on a file it runs INFO <file> and INFO <file> JSON; its
//! buttons run INFO <file> STATS and INFO <file> CHECK.
//!
//! Object names:
//!
//! ```text
//! datasetInfoDialog     the dialog
//! datasetInfoTabs       Summary, Fields, Bands, JSON
//! datasetInfoSummary    the records that are not a field or a band: the dataset, raster,
//!                       layers, subdatasets, point cloud
//! datasetInfoFields     a table: Layer, Field, Type, Width
//! datasetInfoBands      a table: Band, Type, No data, Min, Max, Mean, Std dev, Overviews
//! datasetInfoJson       GDAL's own JSON (INFO ... JSON), indented
//! datasetInfoCopyJson   copies it to the clipboard
//! datasetInfoStats      INFO ... STATS: every band's statistics, computed from every pixel, no
//!                       sidecar written beside the file
//! datasetInfoCheck      INFO ... CHECK: every value read, and what failed
//! datasetInfoCommand    the line last run, exactly as it would be typed
//! datasetInfoReply      what the last line said: the check, or why it failed
//! datasetInfoClose      closes it
//! ```
//!
//! HOW A LINE'S ANSWER COMES. A line of the geoprocessing executor may start a background job,
//! whose reply comes when the job ends: the dialog hands what `Host::run` returned to
//! `Host::await_job`, which - when that started a job still running - tells the dialog the job's
//! outcome once it ends, and the dialog takes it at its next look (`poll`, and each frame). What
//! the C++'s DatasetInfoRunner carried, `run` and `await`, are the host's.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use egui::Ui;
use katana_app::geo::replies::{Record, parse_records};
use katana_core::json::Json;

use crate::annotation::dialog_support::table;
use crate::drive::{
    LineOptions, TextOptions, button, button_as, label_as, line_with, tabs, text_with,
};
use crate::host::{Host, VerbOutcome};
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::offstage;

/// The tabs, in their order.
const TABS: [&str; 4] = ["Summary", "Fields", "Bands", "JSON"];

const FIELD_HEADERS: [&str; 4] = ["Layer", "Field", "Type", "Width"];

const BAND_HEADERS: [&str; 8] = [
    "Band",
    "Type",
    "No data",
    "Min",
    "Max",
    "Mean",
    "Std dev",
    "Overviews",
];

/// The body's width: the C++ dialog's 760 less its frame.
const BODY_WIDTH: f32 = 730.0;

/// What an outcome is for: the records' tabs, or the JSON tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shows {
    Records,
    Json,
}

fn field(record: &Record, key: &str) -> String {
    String::from(record.get(key).unwrap_or(""))
}

/// A record as a person reads it: its word, then key=value, unquoted.
fn readable(record: &Record) -> String {
    let mut words = vec![record.kind.clone()];
    for (key, text) in &record.fields {
        words.push(format!("{key}={text}"));
    }
    words.join(" ")
}

/// QFileInfo::fileName: what follows the path's last separator.
fn file_name(path: &str) -> &str {
    let separators: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    path.rsplit(separators).next().unwrap_or(path)
}

/// A button whose C++ text marks its Alt letter with '&': drawn without the mark, and reported
/// with the text as the C++ wrote it.
fn mnemonic_button(ui: &mut Ui, name: &str, text: &str) -> egui::Response {
    button_as(ui, name, text, egui::Button::new(text.replace('&', "")))
}

/// GIS > Dataset Information (see the module comment).
pub struct DatasetInfoDialog {
    frame: DialogFrame,
    visible: bool,
    path: String,
    described: bool,
    /// Each band's min, max, mean and stddev once STATS has read them: a later reply (CHECK, the
    /// plain INFO) gives none, and the file is the same, so they are kept rather than blanked.
    statistics: BTreeMap<String, Vec<String>>,
    tab: usize,
    summary: String,
    fields: Vec<Vec<String>>,
    bands: Vec<Vec<String>>,
    json: String,
    copy_enabled: bool,
    command: String,
    reply: String,
    /// The outcomes of the jobs this dialog's lines started, told when each ended.
    heard: Rc<RefCell<Vec<(Shows, VerbOutcome)>>>,
}

impl DatasetInfoDialog {
    /// The dialog on `path`, shown: INFO <file> and INFO <file> JSON run through `host`.
    pub fn new(path: &str, host: &mut dyn Host) -> Self {
        let mut dialog = Self {
            frame: DialogFrame::new(
                "datasetInfoDialog",
                &format!("Dataset Information - {}", file_name(path)),
            ),
            visible: true,
            path: String::from(path),
            described: false,
            statistics: BTreeMap::new(),
            tab: 0,
            summary: String::new(),
            fields: Vec::new(),
            bands: Vec::new(),
            json: String::new(),
            copy_enabled: false,
            command: String::new(),
            reply: String::new(),
            heard: Rc::new(RefCell::new(Vec::new())),
        };
        dialog.run_line(host, "", Shows::Records);
        dialog.run_line(host, "JSON", Shows::Json);
        dialog
    }

    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    pub fn window_title(&self) -> &str {
        self.frame.title()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// The INFO line of this file with `words` after the path ("STATS"): the path quoted, as a
    /// person would type it.
    pub fn line(&self, words: &str) -> String {
        if words.is_empty() {
            format!("INFO \"{}\"", self.path)
        } else {
            format!("INFO \"{}\" {words}", self.path)
        }
    }

    /// Whether INFO <file> has answered, and said what the file holds. A job's answer counts
    /// once the dialog has looked (`poll`, or a frame).
    pub fn described(&self) -> bool {
        self.described
    }

    /// The Fields table's rows.
    pub fn fields(&self) -> &[Vec<String>] {
        &self.fields
    }

    /// The Bands table's rows.
    pub fn bands(&self) -> &[Vec<String>] {
        &self.bands
    }

    pub fn summary_text(&self) -> &str {
        &self.summary
    }

    pub fn json_text(&self) -> &str {
        &self.json
    }

    pub fn reply_text(&self) -> &str {
        &self.reply
    }

    pub fn command_text(&self) -> &str {
        &self.command
    }

    /// Takes what the jobs this dialog started have said since it last looked.
    pub fn poll(&mut self) {
        let heard: Vec<(Shows, VerbOutcome)> = std::mem::take(&mut *self.heard.borrow_mut());
        for (shows, outcome) in heard {
            self.take_outcome(shows, &outcome);
        }
    }

    fn take_outcome(&mut self, shows: Shows, outcome: &VerbOutcome) {
        match shows {
            Shows::Records => self.show_records(outcome),
            Shows::Json => self.show_json(outcome),
        }
    }

    /// `words`' line run, and its outcome shown - now, or when its job ends. Never after the
    /// dialog has gone: what a job says then is told to nobody.
    fn run_line(&mut self, host: &mut dyn Host, words: &str, shows: Shows) {
        let text = self.line(words);
        self.command.clone_from(&text);
        let outcome = host.run(&text);
        if outcome.ok {
            let heard = Rc::downgrade(&self.heard);
            let waits = host.await_job(
                &outcome,
                Box::new(move |done: &VerbOutcome, _host: &mut dyn Host| {
                    if let Some(heard) = heard.upgrade() {
                        heard.borrow_mut().push((shows, done.clone()));
                    }
                }),
            );
            if waits {
                self.reply = format!("Reading {}...", file_name(&self.path));
                return;
            }
        }
        self.take_outcome(shows, &outcome);
    }

    fn show_records(&mut self, outcome: &VerbOutcome) {
        if !outcome.ok {
            self.reply = if outcome.error.is_empty() {
                String::from("The line was refused.")
            } else {
                outcome.error.clone()
            };
            return;
        }
        self.described = true;
        let mut summary: Vec<String> = Vec::new();
        let mut checked: Vec<String> = Vec::new();
        self.fields.clear();
        self.bands.clear();
        for record in parse_records(&outcome.reply) {
            if record.kind == "field" {
                let subtype = field(&record, "subtype");
                let kind = if subtype.is_empty() {
                    field(&record, "type")
                } else {
                    format!("{} ({subtype})", field(&record, "type"))
                };
                self.fields.push(vec![
                    field(&record, "layer"),
                    field(&record, "name"),
                    kind,
                    field(&record, "width"),
                ]);
            } else if record.kind == "band" {
                let band = field(&record, "band");
                let mut numbers = vec![
                    field(&record, "min"),
                    field(&record, "max"),
                    field(&record, "mean"),
                    field(&record, "stddev"),
                ];
                if numbers.iter().any(|one| !one.is_empty()) {
                    self.statistics.insert(band.clone(), numbers.clone());
                } else if let Some(kept) = self.statistics.get(&band) {
                    numbers.clone_from(kept);
                }
                let mut row = vec![band, field(&record, "type"), field(&record, "nodata")];
                row.append(&mut numbers);
                row.push(field(&record, "overviews"));
                self.bands.push(row);
            } else if record.kind == "check" || record.kind == "problem" {
                checked.push(readable(&record));
            } else if record.kind != "overview" {
                summary.push(readable(&record));
            }
        }
        self.summary = summary.join("\n");
        self.reply = checked.join("\n");
    }

    fn show_json(&mut self, outcome: &VerbOutcome) {
        if !outcome.ok {
            self.json.clone_from(&outcome.error);
            self.copy_enabled = false;
            return;
        }
        // The executor gives it on one line; indented here for a person to read. What is not a
        // JSON document is shown as it came.
        self.json = match Json::parse(outcome.reply.as_bytes()) {
            Ok(document) if document.is_structured() => document.dump_with(4, ' ', false),
            _ => outcome.reply.clone(),
        };
        self.copy_enabled = true;
    }

    /// One frame of the dialog, while it is open.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        self.poll();
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
            self.visible = false;
        }
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.set_max_width(BODY_WIDTH);
        tabs(ui, "datasetInfoTabs", &TABS, &mut self.tab);
        let mut copy = false;
        for (page, title) in TABS.iter().enumerate() {
            if page == self.tab {
                copy |= self.page(ui, page);
            } else {
                // A page not in front is still there to be read and pressed, as a hidden tab's
                // widgets are.
                copy |= offstage(ui, title, |ui| self.page(ui, page));
            }
        }
        if copy && let Err(error) = host.clipboard().set_text(&self.json) {
            self.reply = error.describe();
        }
    }

    /// One tab's page; true when Copy as JSON was pressed.
    fn page(&mut self, ui: &mut Ui, page: usize) -> bool {
        let fixed = TextOptions {
            read_only: true,
            rows: 18,
            monospace: true,
        };
        match page {
            0 => {
                text_with(ui, "datasetInfoSummary", &mut self.summary, fixed);
                false
            }
            1 => {
                table(
                    ui,
                    "datasetInfoFields",
                    &FIELD_HEADERS,
                    &self.fields,
                    None,
                    &[],
                    None,
                );
                false
            }
            2 => {
                table(
                    ui,
                    "datasetInfoBands",
                    &BAND_HEADERS,
                    &self.bands,
                    None,
                    &[],
                    None,
                );
                false
            }
            _ => {
                text_with(ui, "datasetInfoJson", &mut self.json, fixed);
                ui.add_enabled_ui(self.copy_enabled, |ui| {
                    mnemonic_button(ui, "datasetInfoCopyJson", "&Copy as JSON")
                        .on_hover_text("GDAL's own description of the file, to the clipboard")
                        .clicked()
                })
                .inner
            }
        }
    }

    /// The line last run, what it said, and the buttons: kept below the scrolling body.
    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        line_with(
            ui,
            "datasetInfoCommand",
            &mut self.command,
            LineOptions {
                read_only: true,
                width: Some(f32::INFINITY),
                ..LineOptions::default()
            },
        )
        .on_hover_text("The line last run, as it would be typed");
        // GDAL's words, never markup.
        let reply = self.reply.clone();
        label_as(
            ui,
            "datasetInfoReply",
            &reply,
            egui::Label::new(reply.as_str()).wrap(),
        );
        let (stats, check, close) = ui
            .horizontal(|ui| {
                let stats = mnemonic_button(ui, "datasetInfoStats", "Compute &Statistics")
                    .on_hover_text(
                        "INFO ... STATS: every band's minimum, maximum, mean and standard \
                         deviation, read from every pixel. Nothing is written beside the file.",
                    )
                    .clicked();
                let check = mnemonic_button(ui, "datasetInfoCheck", "C&heck")
                    .on_hover_text(
                        "INFO ... CHECK: read every value of the file, and say what could not be \
                         read",
                    )
                    .clicked();
                let close = button(ui, "datasetInfoClose", "Close").clicked();
                (stats, check, close)
            })
            .inner;
        if stats {
            self.run_line(host, "STATS", Shows::Records);
        }
        if check {
            self.run_line(host, "CHECK", Shows::Records);
        }
        if close {
            self.visible = false;
        }
    }
}
