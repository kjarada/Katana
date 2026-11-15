//! Annotate > Label Layout Report... (docs/annotation.md, "In the window"): how the drawing's
//! labels fare at the annotation scale - how many the placer put at their own place, how many it
//! moved (displaced), how many found no room (suppressed) and how many label nothing (orphaned) -
//! with the labels that found no room selected on request, to move or restyle them. The port of
//! annotation/label_layout_report.
//!
//! The dialog works nothing out itself. Run hands "LABEL LAYOUT" - with collisions=off when
//! labelLayoutCollisions is unticked - to the window's one executor (host::Host::run), and the
//! counts are read back out of the verb's reply by `parse_label_layout_reply`. Select Suppressed
//! runs "SELECT id...". Both lines are echoed and kept in the history as typed ones; neither
//! changes the drawing. Whoever opens it runs it once as it opens.
//!
//! Object names:
//!
//! ```text
//! labelLayoutDialog            the dialog (the action annotateLabelLayout)
//! labelLayoutCollisions        ticked: labels are kept clear of each other and of the linework,
//!                              as the plan view places them; unticked: collisions=off, each at
//!                              its own place
//! labelLayoutRun               run LABEL LAYOUT
//! labelLayoutCommand           the line Run runs (read-only)
//! labelLayoutPlaced, labelLayoutDisplaced, labelLayoutSuppressed, labelLayoutOrphaned
//!                              the counts
//! labelLayoutSelectSuppressed  select the labels with a piece that found no room; disabled when
//!                              there are none
//! labelLayoutOutput            the verb's whole reply (read-only)
//! labelLayoutStatus            what happened last
//! labelLayoutClose             close
//! ```

use std::collections::BTreeMap;

use egui::{Context, Ui};
use katana_core::text::{format_exact_real, parse_finite_double};
use katana_entity::EntityId;

use super::dialog_support::show_dialog;
use crate::drive::controls::{self, LineOptions, TextOptions};
use crate::host::Host;
use crate::shell::DialogFrame;

/// The dialog's object name and title.
const DIALOG: &str = "labelLayoutDialog";
const TITLE: &str = "Label Layout Report";

/// What LABEL LAYOUT replied: its summary record, and the labels named by its "suppressed=yes"
/// records, each once, in the order the verb gave them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabelLayoutSummary {
    /// The summary record was found.
    pub read: bool,
    pub scale: f64,
    pub considered: usize,
    pub placed: usize,
    pub displaced: usize,
    pub suppressed: usize,
    pub orphaned: usize,
    pub suppressed_labels: Vec<EntityId>,
}

/// One reply record's fields: key=value pairs split on blanks, a value in double quotes holding
/// blanks, with \" and \\ inside it, as the annotation verbs write them (annotation_verbs, field).
fn fields_of(line: &str) -> BTreeMap<String, String> {
    let line: Vec<char> = line.chars().collect();
    let find = |wanted: char, from: usize| (from..line.len()).find(|&at| line[at] == wanted);
    let mut fields = BTreeMap::new();
    let mut at = 0;
    while at < line.len() {
        while at < line.len() && line[at].is_whitespace() {
            at += 1;
        }
        let equals = find('=', at);
        let Some(equals) = equals.filter(|_| at < line.len()) else {
            break;
        };
        // A word with no '=' ("created") is not a field.
        if let Some(blank) = find(' ', at)
            && blank < equals
        {
            at = blank;
            continue;
        }
        let key: String = line[at..equals].iter().collect();
        at = equals + 1;
        let mut value = String::new();
        if at < line.len() && line[at] == '"' {
            at += 1;
            while at < line.len() && line[at] != '"' {
                if line[at] == '\\' && at + 1 < line.len() {
                    at += 1;
                }
                value.push(line[at]);
                at += 1;
            }
            at += 1; // the closing quote
        } else {
            while at < line.len() && !line[at].is_whitespace() {
                value.push(line[at]);
                at += 1;
            }
        }
        fields.insert(key, value);
    }
    fields
}

/// QString::toULongLong: a whole number, or None.
fn whole(text: &str) -> Option<u64> {
    text.parse::<u64>().ok()
}

/// The count `key` of `fields`; `ok` is cleared when it is missing or not a whole number.
fn count_of(fields: &BTreeMap<String, String>, key: &str, ok: &mut bool) -> usize {
    let value = fields.get(key).map(String::as_str).and_then(whole);
    *ok = *ok && value.is_some();
    value.unwrap_or(0) as usize
}

/// "LABEL LAYOUT", or "LABEL LAYOUT collisions=off".
pub fn label_layout_line(collisions: bool) -> String {
    String::from(if collisions {
        "LABEL LAYOUT"
    } else {
        "LABEL LAYOUT collisions=off"
    })
}

/// What LABEL LAYOUT's `reply` says (LabelLayoutSummary).
pub fn parse_label_layout_reply(reply: &str) -> LabelLayoutSummary {
    let mut summary = LabelLayoutSummary::default();
    for line in reply.split('\n').filter(|line| !line.is_empty()) {
        let fields = fields_of(line.trim());
        if fields.contains_key("considered") {
            let mut ok = true;
            summary.considered = count_of(&fields, "considered", &mut ok);
            summary.placed = count_of(&fields, "placed", &mut ok);
            summary.displaced = count_of(&fields, "displaced", &mut ok);
            summary.suppressed = count_of(&fields, "suppressed", &mut ok);
            summary.orphaned = count_of(&fields, "orphaned", &mut ok);
            let scale = fields
                .get("scale")
                .map(String::as_str)
                .and_then(parse_finite_double);
            summary.scale = scale.unwrap_or(0.0);
            summary.read = ok && scale.is_some();
            continue;
        }
        if fields.get("suppressed").is_some_and(|value| value == "yes")
            && let Some(id) = fields.get("label").map(String::as_str).and_then(whole)
            && !summary.suppressed_labels.contains(&id)
        {
            summary.suppressed_labels.push(id);
        }
    }
    summary
}

/// "SELECT 4 9"; empty for no ids.
pub fn select_line(ids: &[EntityId]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    let mut words = vec![String::from("SELECT")];
    words.extend(ids.iter().map(|id| format!("{id}")));
    words.join(" ")
}

/// The Label Layout Report (see the module comment).
pub struct LabelLayoutReportDialog {
    frame: DialogFrame,
    /// False for a dialog given no command line (the C++'s empty runner): Run and Select
    /// Suppressed say so and run nothing.
    command_line: bool,
    summary: LabelLayoutSummary,
    collisions: bool,
    command: String,
    placed: String,
    displaced: String,
    suppressed: String,
    orphaned: String,
    select_enabled: bool,
    output: String,
    status: String,
    run_pressed: bool,
    select_pressed: bool,
    close_pressed: bool,
}

impl Default for LabelLayoutReportDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl LabelLayoutReportDialog {
    /// The report, its lines run through the host it is drawn with.
    pub fn new() -> Self {
        let none = String::from("-");
        Self {
            frame: DialogFrame::new(DIALOG, TITLE),
            command_line: true,
            summary: LabelLayoutSummary::default(),
            collisions: true,
            command: label_layout_line(true),
            placed: none.clone(),
            displaced: none.clone(),
            suppressed: none.clone(),
            orphaned: none,
            select_enabled: false,
            output: String::new(),
            status: String::new(),
            run_pressed: false,
            select_pressed: false,
            close_pressed: false,
        }
    }

    /// The same report given no command line to run on.
    pub fn without_command_line(mut self) -> Self {
        self.command_line = false;
        self
    }

    /// Runs the layout; false, with the reason in the status, when the line was refused or its
    /// reply could not be read.
    pub fn run(&mut self, host: &mut dyn Host) -> bool {
        if !self.command_line {
            self.status = String::from("There is no command line to run it on.");
            return false;
        }
        let line = label_layout_line(self.collisions);
        self.command.clone_from(&line);
        let outcome = host.run(&line);
        self.output = if outcome.ok {
            outcome.reply.clone()
        } else {
            outcome.error.clone()
        };
        if !outcome.ok {
            self.show_summary(LabelLayoutSummary::default());
            self.status = if outcome.error.is_empty() {
                String::from("The line was refused.")
            } else {
                outcome.error
            };
            return false;
        }
        let summary = parse_label_layout_reply(&outcome.reply);
        self.show_summary(summary.clone());
        if !summary.read {
            self.status = String::from("The reply had no counts to read.");
            return false;
        }
        self.status = format!(
            "{} of {} label pieces placed at 1:{}.",
            summary.placed,
            summary.considered,
            format_exact_real(summary.scale)
        );
        true
    }

    /// Selects the suppressed labels of the last run.
    pub fn select_suppressed(&mut self, host: &mut dyn Host) -> bool {
        let line = select_line(&self.summary.suppressed_labels);
        if line.is_empty() {
            self.status = String::from("No label was suppressed.");
            return false;
        }
        if !self.command_line {
            self.status = String::from("There is no command line to run it on.");
            return false;
        }
        let outcome = host.run(&line);
        self.status = if outcome.ok {
            outcome.reply
        } else {
            outcome.error
        };
        outcome.ok
    }

    pub fn summary(&self) -> &LabelLayoutSummary {
        &self.summary
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    fn show_summary(&mut self, summary: LabelLayoutSummary) {
        let text = |value: usize| {
            if summary.read {
                format!("{value}")
            } else {
                String::from("-")
            }
        };
        self.placed = text(summary.placed);
        self.displaced = text(summary.displaced);
        self.suppressed = text(summary.suppressed);
        self.orphaned = text(summary.orphaned);
        self.select_enabled = !summary.suppressed_labels.is_empty();
        self.summary = summary;
    }

    /// Draws the dialog for this frame; false once it is closed (Close, or the title bar's close
    /// button).
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        let mut frame = std::mem::replace(&mut self.frame, DialogFrame::new(DIALOG, TITLE));
        let closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        if std::mem::take(&mut self.run_pressed) {
            self.run(host);
        }
        if std::mem::take(&mut self.select_pressed) {
            self.select_suppressed(host);
        }
        !(closed || std::mem::take(&mut self.close_pressed))
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        if controls::check(
            ui,
            "labelLayoutCollisions",
            "Keep labels clear of each other and of the linework",
            &mut self.collisions,
        )
        .on_hover_text(
            "As the plan view places them; unticked, every label is put at its own place however \
             they overlap",
        )
        .changed()
        {
            self.command = label_layout_line(self.collisions);
        }
        egui::Grid::new("labelLayoutCounts")
            .num_columns(2)
            .show(ui, |ui| {
                let counts = [
                    ("Placed", "labelLayoutPlaced", &self.placed),
                    (
                        "Displaced (moved to find room)",
                        "labelLayoutDisplaced",
                        &self.displaced,
                    ),
                    (
                        "Suppressed (no room)",
                        "labelLayoutSuppressed",
                        &self.suppressed,
                    ),
                    (
                        "Orphaned (label nothing)",
                        "labelLayoutOrphaned",
                        &self.orphaned,
                    ),
                ];
                for (title, name, value) in counts {
                    ui.label(title);
                    controls::label(ui, name, value);
                    ui.end_row();
                }
                ui.label("Command");
                controls::line_with(
                    ui,
                    "labelLayoutCommand",
                    &mut self.command,
                    LineOptions {
                        read_only: true,
                        ..LineOptions::default()
                    },
                );
                ui.end_row();
            });
        controls::text_with(
            ui,
            "labelLayoutOutput",
            &mut self.output,
            TextOptions {
                read_only: true,
                rows: 6,
                monospace: true,
            },
        );
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        controls::label(ui, "labelLayoutStatus", &self.status);
        ui.horizontal(|ui| {
            if controls::button(ui, "labelLayoutRun", "Run").clicked() {
                self.run_pressed = true;
            }
            ui.add_enabled_ui(self.select_enabled, |ui| {
                if controls::button(ui, "labelLayoutSelectSuppressed", "Select Suppressed")
                    .on_hover_text(
                        "Select the labels that found no room, to move them (Edit Label) or give \
                         them another style",
                    )
                    .clicked()
                {
                    self.select_pressed = true;
                }
            });
            if controls::button(ui, "labelLayoutClose", "Close").clicked() {
                self.close_pressed = true;
            }
        });
    }
}
