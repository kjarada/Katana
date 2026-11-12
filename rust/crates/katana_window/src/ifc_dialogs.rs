//! File > Import IFC and File > Export IFC (docs/ifc.md, "In the window"). The port of
//! ifc_dialogs.
//!
//! The choices of an IFC exchange and nothing else: neither dialog reads or writes a file, and
//! neither calls the IFC module to do so. Each writes the line its choices mean - EXPORT, IMPORT,
//! INFO or IFC RULES, with katana_ifc::front_end's grammar (format_export_line,
//! format_import_line) - shows it as it is edited, and hands it to the window's command line
//! through its context, where it is echoed and run exactly as if it had been typed, by the code
//! katana_cli's line runs. So there is nothing a dialog can do that a typed line, a script or an
//! agent cannot. Both are non-modal and kept by the window - a path is typed or browsed for in a
//! field rather than asked for up front - so that a headless run drives them by name (--dialog
//! fileExportIfc, --fill, --press) and nothing waits on a box no one can see.
//!
//! The export dialog's table previews what the file will hold: for each layer, service, alignment
//! and surface, how many objects become which IFC class and why. It is read from the reply of the
//! line with PREVIEW added - the writer's own account (IfcExport::tally), made by writing the file
//! in memory, and read by the one reader of that reply (read_export_objects) - so what is shown is
//! what is written, and an agent asking PREVIEW gets the same rows. A class that is wrong for a
//! project's layers is put right with a rules file (parse_classification_rules), which IFC RULES
//! writes from the defaults.
//!
//! Object names, the interface tests and the headless driver use:
//!
//! ```text
//! fileExportIfcDialog   ifcExportFile, ifcExportBrowse, ifcExportEntities,
//!                       ifcExportSelectedOnly, ifcExportAlignments, ifcExportSurfaces,
//!                       ifcExportSchedule, ifcExportScheduleBrowse, ifcExportSchema,
//!                       ifcExportSchemaBrowse, ifcExportSpacing, ifcExportRules,
//!                       ifcExportRulesBrowse, ifcExportSaveRules, ifcExportCrs, ifcExportCommand,
//!                       ifcExportClasses, ifcExportCheck, ifcExportPreview, ifcExportExport,
//!                       ifcExportClose, ifcExportRoadModel
//! fileImportIfcDialog   ifcImportFile, ifcImportBrowse, ifcImportDescribe, ifcImportSummary,
//!                       ifcImportLocal, ifcImportAlignments, ifcImportElements,
//!                       ifcImportSurfaces, ifcImportTolerance, ifcImportTakeCrs,
//!                       ifcImportCommand, ifcImportCheck, ifcImportImport, ifcImportClose
//! ```
//!
//! The window's side - its counts, and runIfcCommand, which runs an IFC line and answers the
//! interpreter's own Result so that a cancel keeps its code - is the context's, supplied by the
//! main window and by a test. Whether the session is headless is the host's.
//!
//! This module also holds the window's door to the system's file dialogs (QFileDialog's
//! getOpenFileName and getSaveFileName, through rfd), which the utilities dialog shares: never
//! called in a headless session, where every Browse says what to fill instead.

use std::path::Path;

use egui::{RichText, Ui};
use katana_core::text::{
    ReplyRecord, format_general, parse_finite_double, read_reply_record, split_lines,
};
use katana_core::{ErrorCode, Result};
use katana_ifc::{
    ClassTally, ExportArguments, ImportArguments, ImportOptions, format_export_line,
    format_import_line, format_info_line, format_rules_line, read_export_objects,
};
use katana_survey::subsurface::GradingSettings;

use crate::drive::controls::{self, LineOptions, TextOptions};
use crate::format::grouped;
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::theme;

// ---- the system's file dialogs ------------------------------------------------------------------

/// QDir::toNativeSeparators: '/' written '\' on Windows; the text as it is elsewhere.
pub(crate) fn to_native_separators(text: &str) -> String {
    if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        String::from(text)
    }
}

/// QDir::fromNativeSeparators: '\' read as '/' on Windows; the text as it is elsewhere.
pub(crate) fn from_native_separators(text: &str) -> String {
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        String::from(text)
    }
}

/// A QFileDialog filter ("CSV files (*.csv);;All files (*)") as rfd's: each entry's name and the
/// extensions of its patterns ("*" for every file).
fn qt_filters(filter: &str) -> Vec<(String, Vec<String>)> {
    filter
        .split(";;")
        .filter_map(|entry| {
            let open = entry.find('(')?;
            let close = open + entry[open..].find(')')?;
            let extensions = entry[open + 1..close]
                .split_whitespace()
                .map(|pattern| String::from(pattern.strip_prefix("*.").unwrap_or(pattern)))
                .collect();
            Some((String::from(entry[..open].trim()), extensions))
        })
        .collect()
}

/// The dialog `title` with `filter`, starting at `start` - a folder, or a file whose folder it opens
/// in with its name given - as QFileDialog takes its directory argument.
fn file_dialog(title: &str, start: &str, filter: &str) -> rfd::FileDialog {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    if !start.is_empty() {
        let path = Path::new(start);
        if path.is_dir() {
            dialog = dialog.set_directory(path);
        } else {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                dialog = dialog.set_directory(parent);
            }
            if let Some(name) = path.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
        }
    }
    for (name, extensions) in qt_filters(filter) {
        dialog = dialog.add_filter(name, extensions.as_slice());
    }
    dialog
}

/// QFileDialog::getOpenFileName: the file chosen, empty when none was. A person's dialog: never
/// called in a headless session.
pub(crate) fn get_open_file_name(title: &str, start: &str, filter: &str) -> String {
    file_dialog(title, start, filter)
        .pick_file()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// QFileDialog::getSaveFileName: the file named, empty when none was. A person's dialog: never
/// called in a headless session.
pub(crate) fn get_save_file_name(title: &str, start: &str, filter: &str) -> String {
    file_dialog(title, start, filter)
        .save_file()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ---- what both dialogs share --------------------------------------------------------------------

/// The text of a path field as the filesystem takes it.
fn path_of(text: &str) -> String {
    from_native_separators(text.trim())
}

/// Ends in ".ifc", any letter case.
fn is_ifc_name(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b".ifc")
}

/// The first record of a reply - "ifc exported ...", "ifc imported ..." - as the verb wrote it.
fn head_record(reply: &str) -> Option<ReplyRecord> {
    let lines = split_lines(reply.as_bytes());
    let first = std::str::from_utf8(lines.first()?).ok()?;
    read_reply_record(first)
}

/// A reply's field, for a sentence; "?" when it is not there.
fn field(record: &Option<ReplyRecord>, key: &str) -> String {
    String::from(
        record
            .as_ref()
            .and_then(|record| record.value(key))
            .unwrap_or("?"),
    )
}

/// "IfcPipeSegment RIGIDSEGMENT", "IfcDistributionChamberElement USERDEFINED (HEADWALL)", or that
/// nothing was written.
fn class_text(row: &ClassTally) -> String {
    if row.entity.is_empty() {
        return String::from("not written");
    }
    let mut text = row.entity.clone();
    if !row.predefined_type.is_empty() {
        text.push(' ');
        text.push_str(&row.predefined_type);
    }
    if !row.object_type.is_empty() {
        text.push_str(&format!(" ({})", row.object_type));
    }
    text
}

/// The tip of the read-only field showing the line a dialog runs.
const COMMAND_TIP: &str = "The command this runs, exactly as it could be typed on the command \
                           line, given to katana_cli or sent by an agent";

/// The read-only field showing the line the dialog runs, as it is edited: what a fill writes there
/// is drawn over by the dialog's line at the next frame.
fn command_field(ui: &mut Ui, name: &str, line: &str) {
    let mut shown = String::from(line);
    controls::line_with(
        ui,
        name,
        &mut shown,
        LineOptions {
            read_only: true,
            width: Some(560.0),
            ..LineOptions::default()
        },
    )
    .on_hover_text(COMMAND_TIP);
}

/// A path field with its placeholder; true when it changed.
fn path_line(ui: &mut Ui, name: &str, value: &mut String, placeholder: &str) -> bool {
    controls::line_with(
        ui,
        name,
        value,
        LineOptions {
            placeholder,
            width: Some(420.0),
            ..LineOptions::default()
        },
    )
    .changed()
}

/// The check label: what stops the line, or what the last run did, in the error colour or the
/// muted one.
fn check_label(ui: &mut Ui, name: &str, text: &str, is_error: bool) {
    let colour = if is_error {
        theme::error()
    } else {
        theme::text_muted()
    };
    controls::label_as(
        ui,
        name,
        text,
        egui::Label::new(RichText::new(text).color(colour.to_egui()))
            .wrap()
            .selectable(true),
    );
}

/// A note in the muted colour, wrapped.
fn note(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(theme::text_muted().to_egui())).wrap());
}

/// Runs a line on the window's command line - echoed and run as a typed IFC line is - and answers
/// its reply (key=value records) or its error, both in the log: MainWindow::runIfcCommand. Lent the
/// window for the call.
pub type IfcRunLine = Box<dyn FnMut(&mut dyn Host, &str) -> Result<String>>;

// ---- export -------------------------------------------------------------------------------------

/// What the export dialog shows of the window, read each time it is shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IfcExportState {
    pub entities: usize,
    pub selected: usize,
    pub alignments: usize,
    pub surfaces: usize,
    /// The project's, as a person reads it; empty for none.
    pub coordinate_system: String,
    /// It has an EPSG code, which the file needs.
    pub georeferenced: bool,
    /// The road model's objects (bim_model): its corridors' courses, kerbs and earthworks, and the
    /// road objects placed along it.
    pub road_objects: usize,
}

/// The window's counts, read from it each time the dialog asks.
pub type IfcStateFn = Box<dyn Fn(&dyn Host) -> IfcExportState>;

/// What the export dialog asks of the window.
#[derive(Default)]
pub struct IfcExportContext {
    /// None: the counts stay the dialog's first, all nothing.
    pub state: Option<IfcStateFn>,
    /// None: Preview, Export and Save Default Rules run nothing.
    pub run_line: Option<IfcRunLine>,
}

/// What the import dialog asks of the window.
#[derive(Default)]
pub struct IfcImportContext {
    /// None: Describe and Import run nothing.
    pub run_line: Option<IfcRunLine>,
}

/// File > Export IFC (see the module comment).
pub struct IfcExportDialog {
    frame: DialogFrame,
    visible: bool,
    state_of: Option<IfcStateFn>,
    run_line: Option<IfcRunLine>,
    state: IfcExportState,
    file: String,
    schedule: String,
    schema: String,
    spacing: String,
    rules: String,
    entities: bool,
    selected_only: bool,
    alignments: bool,
    surfaces: bool,
    road_model: bool,
    tally: Vec<ClassTally>,
    current_row: Option<usize>,
    check_text: String,
    check_error: bool,
    /// What the check label last said of its own accord, so that a result (written, or refused) is
    /// not overwritten until a field changes.
    showing_result: bool,
}

/// A dialog and the window, lent together to the dialog frame's body and footer.
struct Lent<'a, 'h, D> {
    dialog: &'a mut D,
    host: &'a mut (dyn Host + 'h),
}

impl IfcExportDialog {
    /// The dialog over `context`, the window's counts read from `host`.
    pub fn new(context: IfcExportContext, host: &dyn Host) -> Self {
        let mut dialog = Self {
            frame: DialogFrame::new("fileExportIfcDialog", "Export IFC"),
            visible: false,
            state_of: context.state,
            run_line: context.run_line,
            state: IfcExportState::default(),
            file: String::new(),
            schedule: String::new(),
            schema: String::new(),
            // QString::number(double): six significant digits, as %g writes them.
            spacing: format_general(GradingSettings::default().maximum_detected_spacing, 6),
            rules: String::new(),
            entities: true,
            selected_only: false,
            alignments: true,
            surfaces: true,
            road_model: true,
            tally: Vec::new(),
            current_row: None,
            check_text: String::new(),
            check_error: false,
            showing_result: false,
        };
        dialog.refresh(host);
        dialog
    }

    pub fn is_modal(&self) -> bool {
        self.frame.is_modal()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Shows the dialog. Opened again, the last export's words and table are about a drawing that
    /// may have changed since: they are cleared, and the counts read again (its showEvent).
    pub fn show(&mut self, host: &dyn Host) {
        if !self.visible {
            self.visible = true;
            self.showing_result = false;
            self.show_tally(&[]);
            self.refresh(host);
        }
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// The choices as they stand, as the verb's arguments; meaningful when `check` is empty.
    pub fn arguments(&self) -> ExportArguments {
        let mut out = ExportArguments {
            path: path_of(&self.file),
            ..ExportArguments::default()
        };
        let schedule = path_of(&self.schedule);
        if !schedule.is_empty() {
            out.schedule = Some(schedule);
        }
        let schema = path_of(&self.schema);
        if !schema.is_empty() {
            out.schema = Some(schema);
        }
        let rules = path_of(&self.rules);
        if !rules.is_empty() {
            out.rules = Some(rules);
        }
        // SPACING is said only when it matters and is not the verb's own default, so that the
        // line is what a person would type.
        if out.schedule.is_some()
            && let Some(spacing) = parse_finite_double(self.spacing.trim())
            && spacing != GradingSettings::default().maximum_detected_spacing
        {
            out.spacing = Some(spacing);
        }
        out.entities = self.entities;
        out.selected = out.entities && self.selected_only;
        out.road_model = self.road_model;
        out.alignments = self.alignments;
        out.surfaces = self.surfaces;
        out
    }

    /// The line Export runs: `arguments` written out, or empty while `check` says what stops it.
    pub fn line(&self) -> String {
        if !self.check().is_empty() {
            return String::new();
        }
        format_export_line(&self.arguments()).unwrap_or_default()
    }

    /// What stops an export, as a sentence; empty when nothing does.
    pub fn check(&self) -> String {
        let file = path_of(&self.file);
        if file.is_empty() {
            return String::from("Name the .ifc file to write.");
        }
        if !is_ifc_name(&file) {
            return String::from("The file must end in .ifc.");
        }
        self.check_without_file()
    }

    /// The same, but for the file: what stops a preview, which writes none.
    pub fn check_without_file(&self) -> String {
        if !path_of(&self.schema).is_empty() && path_of(&self.schedule).is_empty() {
            return String::from(
                "A delivery schema describes a schedule: choose the schedule it describes too.",
            );
        }
        if parse_finite_double(self.spacing.trim()).is_none_or(|spacing| spacing <= 0.0) {
            return String::from("The detected spacing must be a positive number of metres.");
        }
        let asked = self.arguments();
        if asked.selected && self.state.selected == 0 {
            return String::from(
                "Selected entities only is ticked, and nothing is selected: select what to \
                 export, or untick it.",
            );
        }
        let entities = if asked.selected {
            self.state.selected
        } else {
            self.state.entities
        };
        let drawing = (asked.entities && entities > 0)
            || (asked.alignments && self.state.alignments > 0)
            || (asked.surfaces && self.state.surfaces > 0);
        let road = asked.road_model && self.state.road_objects > 0;
        if !drawing && !road && path_of(&self.schedule).is_empty() {
            return String::from(
                "Nothing to write: the chosen parts of the drawing are empty, and no utility \
                 schedule is chosen.",
            );
        }
        for path in [&self.file, &self.schedule, &self.schema, &self.rules] {
            if path.contains('"') {
                return String::from(
                    "A path holding a double quote cannot be written in a command; rename the \
                     file.",
                );
            }
        }
        String::new()
    }

    /// QLineEdit::setText on the path or spacing field `name` - as code sets it, so a field a
    /// person could not type into now takes it too - with its change handled as typing's. False for
    /// a name that is none of them.
    pub fn set_field_text(&mut self, name: &str, text: &str) -> bool {
        let (slot, is_file) = match name {
            "ifcExportFile" => (&mut self.file, true),
            "ifcExportSchedule" => (&mut self.schedule, false),
            "ifcExportSchema" => (&mut self.schema, false),
            "ifcExportSpacing" => (&mut self.spacing, false),
            "ifcExportRules" => (&mut self.rules, false),
            _ => return false,
        };
        if slot.as_str() != text {
            *slot = String::from(text);
            if is_file {
                self.file_changed();
            } else {
                self.choice_changed();
            }
        }
        true
    }

    pub fn set_file(&mut self, path: &str) {
        self.set_field_text("ifcExportFile", &to_native_separators(path));
    }

    /// Reads the window's state again - counts, selection, coordinate system. The window calls it
    /// as its drawing changes, and the dialog before each preview and export, so a choice is never
    /// judged by counts that were.
    pub fn refresh(&mut self, host: &dyn Host) {
        if let Some(state_of) = &self.state_of {
            self.state = state_of(host);
        }
        self.recheck();
    }

    /// A changed choice makes the last account out of date: it is cleared, so that a table never
    /// stands beside "Ready" showing what would no longer be written.
    fn choice_changed(&mut self) {
        self.showing_result = false;
        self.show_tally(&[]);
        self.recheck();
    }

    /// The file alone changes no class, and keeps the account.
    fn file_changed(&mut self) {
        self.showing_result = false;
        self.recheck();
    }

    fn recheck(&mut self) {
        let problem = self.check();
        if !problem.is_empty() {
            // What stops the export is said, even over the last result: a disabled button with a
            // stale "Wrote ..." beside it says nothing.
            self.showing_result = false;
            self.say(&problem, true);
            return;
        }
        if self.showing_result {
            return;
        }
        let asked = self.arguments();
        let mut parts = Vec::new();
        if asked.entities {
            parts.push(if asked.selected {
                format!("{} selected entities", grouped(self.state.selected as u64))
            } else {
                format!("{} entities", grouped(self.state.entities as u64))
            });
        }
        if asked.alignments && self.state.alignments > 0 {
            parts.push(format!(
                "{} alignments",
                grouped(self.state.alignments as u64)
            ));
        }
        if asked.surfaces && self.state.surfaces > 0 {
            parts.push(format!("{} surfaces", grouped(self.state.surfaces as u64)));
        }
        if asked.schedule.is_some() {
            parts.push(String::from("the schedule's services"));
        }
        if asked.road_model && self.state.road_objects > 0 {
            parts.push(format!(
                "{} road model objects",
                grouped(self.state.road_objects as u64)
            ));
        }
        self.say(
            &format!(
                "Ready to write {}. Preview shows the class each becomes.",
                parts.join(", ")
            ),
            false,
        );
    }

    fn say(&mut self, text: &str, is_error: bool) {
        self.check_text = String::from(text);
        self.check_error = is_error;
    }

    /// What ifcExportCheck says.
    pub fn check_label(&self) -> &str {
        &self.check_text
    }

    pub fn check_is_error(&self) -> bool {
        self.check_error
    }

    /// The preview, or the account of the export just written, as the table shows it: one row per
    /// ClassTally.
    pub fn show_tally(&mut self, tally: &[ClassTally]) {
        self.tally = tally.to_vec();
        self.current_row = None;
    }

    /// The table's rows, each its cells: from, objects, IFC class, system, why.
    pub fn table(&self) -> Vec<Vec<String>> {
        self.tally
            .iter()
            .map(|item| {
                vec![
                    item.source.clone(),
                    grouped(item.count as u64),
                    class_text(item),
                    item.system.clone(),
                    item.why.clone(),
                ]
            })
            .collect()
    }

    /// Fills the table from a reply's object records, and returns them; None, having said why,
    /// when they do not read.
    pub fn show_reply(&mut self, reply: &str) -> Option<Vec<ClassTally>> {
        match read_export_objects(reply) {
            Ok(objects) => {
                self.show_tally(&objects);
                Some(objects)
            }
            Err(error) => {
                self.show_tally(&[]);
                self.say(&error.describe(), true);
                None
            }
        }
    }

    /// Preview: the export's own line with PREVIEW added, its reply's account in the table.
    pub fn preview(&mut self, host: &mut dyn Host) {
        if self.run_line.is_none() {
            return;
        }
        self.refresh(&*host); // the drawing, and its selection, as they are now
        let problem = self.check_without_file();
        if !problem.is_empty() {
            self.say(&problem, true);
            return;
        }
        // The export's own line, PREVIEW added: a file not yet named is given one, which PREVIEW
        // never writes.
        let mut asked = self.arguments();
        if asked.path.is_empty() || !is_ifc_name(&path_of(&self.file)) {
            asked.path = String::from("preview.ifc");
        }
        asked.preview = true;
        let written = match format_export_line(&asked) {
            Ok(written) => written,
            Err(error) => {
                self.say(&error.describe(), true);
                return;
            }
        };
        let reply = match self.run_line.as_mut() {
            Some(run_line) => run_line(host, &written),
            None => return,
        };
        self.showing_result = true;
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                self.show_tally(&[]);
                self.say(&error.describe(), true);
                return;
            }
        };
        let Some(tally) = self.show_reply(&reply) else {
            return;
        };
        let objects: usize = tally
            .iter()
            .filter(|row| !row.entity.is_empty())
            .map(|row| row.count)
            .sum();
        self.say(
            &format!(
                "Preview: {} objects would be written, none as a proxy unless a rule asks for \
                 one; no file has been written.",
                grouped(objects as u64)
            ),
            false,
        );
    }

    /// Export: the line handed to the window, and what was written shown.
    pub fn export_file(&mut self, host: &mut dyn Host) {
        self.refresh(&*host); // the drawing, and its selection, as they are now
        let problem = self.check();
        if !problem.is_empty() {
            self.say(&problem, true);
            return;
        }
        let line = self.line();
        let reply = match self.run_line.as_mut() {
            Some(run_line) => run_line(host, &line),
            None => return,
        };
        self.showing_result = true;
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                self.say(&error.describe(), true);
                return;
            }
        };
        if self.show_reply(&reply).is_none() {
            return;
        }
        let head = head_record(&reply);
        self.say(
            &format!(
                "Wrote {}: {} instances (the log has the reply).",
                path_of(&self.file),
                field(&head, "instances")
            ),
            false,
        );
    }

    /// Save Default Rules: IFC RULES to a file a person names; headless, what to type instead.
    pub fn save_rules(&mut self, host: &mut dyn Host) {
        if host.headless() {
            self.say(
                "Save Default Rules asks a person where; type IFC RULES <file.csv> on the command \
                 line instead.",
                true,
            );
            self.showing_result = true;
            return;
        }
        let mut chosen = get_save_file_name(
            "Save Default Rules",
            "classification_rules.csv",
            "Rules (*.csv)",
        );
        if chosen.is_empty() || self.run_line.is_none() {
            return;
        }
        let bytes = chosen.as_bytes();
        if !(bytes.len() >= 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b".csv")) {
            chosen.push_str(".csv");
        }
        let written = match format_rules_line(&chosen) {
            Ok(written) => written,
            Err(error) => {
                self.say(&error.describe(), true);
                return;
            }
        };
        let reply = match self.run_line.as_mut() {
            Some(run_line) => run_line(host, &written),
            None => return,
        };
        self.showing_result = true;
        if let Err(error) = reply {
            self.say(&error.describe(), true);
            return;
        }
        self.set_field_text("ifcExportRules", &to_native_separators(&chosen));
        self.showing_result = true; // after the field's own change cleared it
        self.say(
            &format!(
                "Wrote the default rules to {chosen}: edit them for the project's layers, and \
                 they are used from there."
            ),
            false,
        );
    }

    /// A Browse button: a file dialog on the desktop; headless, a sentence, as nobody could answer
    /// one there.
    fn browse(&mut self, host: &dyn Host, name: &str, title: &str, filter: &str, save: bool) {
        if host.headless() {
            self.say(
                "Browse asks a person to choose a file; type its path instead.",
                true,
            );
            self.showing_result = true;
            return;
        }
        let start = match name {
            "ifcExportFile" => path_of(&self.file),
            "ifcExportSchedule" => path_of(&self.schedule),
            "ifcExportSchema" => path_of(&self.schema),
            _ => path_of(&self.rules),
        };
        let mut chosen = if save {
            get_save_file_name(title, &start, filter)
        } else {
            get_open_file_name(title, &start, filter)
        };
        if chosen.is_empty() {
            return;
        }
        if save && !is_ifc_name(&chosen) {
            chosen.push_str(".ifc");
        }
        self.set_field_text(name, &to_native_separators(&chosen));
    }

    /// What ifcExportCrs says of the project's coordinate system.
    pub fn crs_text(&self) -> String {
        if self.state.georeferenced {
            return format!(
                "Georeferenced in {}: coordinates are written from a local origin near the work, \
                 which an IfcMapConversion records.",
                self.state.coordinate_system
            );
        }
        let why = if self.state.coordinate_system.is_empty() {
            String::from("the project has no coordinate system")
        } else {
            format!(
                "{} is not one EPSG code, which is how IFC4X3_ADD2 names a system",
                self.state.coordinate_system
            )
        };
        format!(
            "Not georeferenced: {why}, so coordinates are written as they are. File > Project \
             Coordinate System sets one."
        )
    }

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent = Lent {
            dialog: &mut *self,
            host: &mut *host,
        };
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| lent.dialog.body(ui, &mut *lent.host),
            |ui, lent| lent.dialog.footer(ui, &mut *lent.host),
        );
        self.frame = frame;
        if shown.closed {
            self.hide();
        }
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.set_max_width(780.0);
        note(
            ui,
            "IFC 4.3 (IFC4X3_ADD2): alignments as their design, the drawing by class - kerbs, \
             fences, pits, pipes - the road model as roads, their parts and elements, and an AS \
             5488 investigation graded and typed by its services. Nothing is written as a proxy.",
        );
        ui.horizontal(|ui| {
            ui.label("File:");
            if path_line(
                ui,
                "ifcExportFile",
                &mut self.file,
                "the .ifc file to write; it is replaced whole",
            ) {
                self.file_changed();
            }
            if controls::button(ui, "ifcExportBrowse", "Browse...").clicked() {
                self.browse(
                    &*host,
                    "ifcExportFile",
                    "Export IFC",
                    "IFC 4.3 (*.ifc)",
                    true,
                );
            }
        });

        let state = self.state.clone();
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Drawing:");
            changed |= controls::check(
                ui,
                "ifcExportEntities",
                &format!("Drawing entities ({})", grouped(state.entities as u64)),
                &mut self.entities,
            )
            .on_hover_text(
                "The drawing's entities, each as the class its layer, survey code or string name \
                 makes it",
            )
            .changed();
        });
        // Kept as chosen: a selection cleared while the dialog is open must refuse the export
        // (check), not quietly widen it to everything.
        let selected_text = if state.selected == 0 {
            String::from("Selected entities only (nothing is selected)")
        } else {
            format!(
                "Selected entities only ({} selected)",
                grouped(state.selected as u64)
            )
        };
        ui.add_enabled_ui(self.entities, |ui| {
            changed |= controls::check(
                ui,
                "ifcExportSelectedOnly",
                &selected_text,
                &mut self.selected_only,
            )
            .on_hover_text("Only the entities selected in the drawing")
            .changed();
        });
        changed |= controls::check(
            ui,
            "ifcExportAlignments",
            &format!("Alignments ({})", grouped(state.alignments as u64)),
            &mut self.alignments,
        )
        .on_hover_text(
            "Every alignment, as IfcAlignment: its PIs' design and its profile's, with stations",
        )
        .changed();
        changed |= controls::check(
            ui,
            "ifcExportSurfaces",
            &format!(
                "The drawing's surfaces ({})",
                grouped(state.surfaces as u64)
            ),
            &mut self.surfaces,
        )
        .on_hover_text("The drawing's surfaces, as IfcGeographicElement TERRAIN")
        .changed();
        // The road model: NOROADMODEL when unticked (docs/ifc.md, "Road model").
        changed |= controls::check(
            ui,
            "ifcExportRoadModel",
            &format!(
                "Road model ({} objects)",
                grouped(state.road_objects as u64)
            ),
            &mut self.road_model,
        )
        .on_hover_text(
            "The road model's 3D objects - its corridors' courses, kerbs and earthworks, and the \
             road objects - as IfcRoad, its parts and their elements",
        )
        .changed();
        if changed {
            self.choice_changed();
        }

        ui.horizontal(|ui| {
            ui.label("Utility schedule:");
            if path_line(
                ui,
                "ifcExportSchedule",
                &mut self.schedule,
                "optional: an AS 5488 schedule (.csv)",
            ) {
                self.choice_changed();
            }
            if controls::button(ui, "ifcExportScheduleBrowse", "Browse...").clicked() {
                self.browse(
                    &*host,
                    "ifcExportSchedule",
                    "Utility Schedule",
                    "Utility schedule (*.csv);;All files (*)",
                    false,
                );
            }
        });
        let schema_live = !path_of(&self.schedule).is_empty() || !path_of(&self.schema).is_empty();
        ui.horizontal(|ui| {
            ui.label("Delivery schema:");
            ui.add_enabled_ui(schema_live, |ui| {
                if path_line(
                    ui,
                    "ifcExportSchema",
                    &mut self.schema,
                    "optional: the schema it was written to (.csv)",
                ) {
                    self.choice_changed();
                }
            });
            if controls::button(ui, "ifcExportSchemaBrowse", "Browse...").clicked() {
                self.browse(
                    &*host,
                    "ifcExportSchema",
                    "Delivery Schema",
                    "Delivery schema (*.csv);;All files (*)",
                    false,
                );
            }
        });
        ui.horizontal(|ui| {
            ui.label("Detected spacing (m):");
            if controls::line(ui, "ifcExportSpacing", &mut self.spacing)
                .on_hover_text(
                    "The longest detected spacing that keeps QL-B, in metres: a path interpolated \
                     across a longer gap was not traced (the project specification sets it)",
                )
                .changed()
            {
                self.choice_changed();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Classification rules:");
            let typed = controls::line_with(
                ui,
                "ifcExportRules",
                &mut self.rules,
                LineOptions {
                    placeholder: "optional: a project's classification rules (.csv)",
                    width: Some(330.0),
                    ..LineOptions::default()
                },
            )
            .on_hover_text(
                "Rules tried before the defaults: a layer, survey code or string name word and the \
                 IFC class it makes (docs/ifc.md, \"Classification rules\")",
            )
            .changed();
            if typed {
                self.choice_changed();
            }
            if controls::button(ui, "ifcExportRulesBrowse", "Browse...").clicked() {
                self.browse(
                    &*host,
                    "ifcExportRules",
                    "Classification Rules",
                    "Rules (*.csv);;All files (*)",
                    false,
                );
            }
            if controls::button(ui, "ifcExportSaveRules", "Save Default Rules...")
                .on_hover_text(
                    "Write the default rules to a file, to edit into the project's own (IFC RULES)",
                )
                .clicked()
            {
                self.save_rules(&mut *host);
                ui.ctx().request_repaint();
            }
        });

        let crs = self.crs_text();
        controls::label_as(
            ui,
            "ifcExportCrs",
            &crs,
            egui::Label::new(RichText::new(crs.as_str()).color(theme::text_muted().to_egui()))
                .wrap(),
        );
        ui.horizontal(|ui| {
            ui.label("Command:");
            let line = self.line();
            command_field(ui, "ifcExportCommand", &line);
        });
        ui.label("From | Objects | IFC class | System | Why");
        let rows = self.table();
        controls::rows(ui, "ifcExportClasses", &rows, &mut self.current_row).on_hover_text(
            "What the file holds, by where it came from: the writer's own account, from Preview or \
             the export just written",
        );
    }

    /// The check under the table and the buttons: kept below the scrolling body.
    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let check = self.check_text.clone();
        check_label(ui, "ifcExportCheck", &check, self.check_error);
        // A preview needs everything but the file.
        let preview_live = self.check_without_file().is_empty();
        let export_live = self.check().is_empty();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(preview_live, |ui| {
                if controls::button(ui, "ifcExportPreview", "Preview")
                    .on_hover_text("What would be written, class by class; no file is written")
                    .clicked()
                {
                    self.preview(&mut *host);
                    ui.ctx().request_repaint();
                }
            });
            ui.add_enabled_ui(export_live, |ui| {
                if controls::button(ui, "ifcExportExport", "Export").clicked() {
                    self.export_file(&mut *host);
                    ui.ctx().request_repaint();
                }
            });
            if controls::button(ui, "ifcExportClose", "Close").clicked() {
                self.hide();
            }
        });
    }
}

// ---- import -------------------------------------------------------------------------------------

/// File > Import IFC (see the module comment).
pub struct IfcImportDialog {
    frame: DialogFrame,
    visible: bool,
    run_line: Option<IfcRunLine>,
    file: String,
    summary: String,
    local: bool,
    alignments: bool,
    elements: bool,
    surfaces: bool,
    tolerance: String,
    take_crs: bool,
    check_text: String,
    check_error: bool,
    showing_result: bool,
    /// Browse chose a file this frame: Describe follows, with the window.
    pending_describe: bool,
}

impl IfcImportDialog {
    pub fn new(context: IfcImportContext) -> Self {
        let mut dialog = Self {
            frame: DialogFrame::new("fileImportIfcDialog", "Import IFC"),
            visible: false,
            run_line: context.run_line,
            file: String::new(),
            summary: String::new(),
            local: false,
            alignments: true,
            elements: true,
            surfaces: true,
            // QString::number(double): six significant digits, as %g writes them.
            tolerance: format_general(ImportOptions::default().curve_tolerance, 6),
            take_crs: true,
            check_text: String::new(),
            check_error: false,
            showing_result: false,
            pending_describe: false,
        };
        dialog.recheck();
        dialog
    }

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

    pub fn arguments(&self) -> ImportArguments {
        let mut out = ImportArguments {
            path: path_of(&self.file),
            local: self.local,
            alignments: self.alignments,
            elements: self.elements,
            surfaces: self.surfaces,
            ..ImportArguments::default()
        };
        // TOLERANCE only when it is not the reader's own default: the line is what a person would
        // type.
        if let Some(tolerance) = parse_finite_double(self.tolerance.trim())
            && tolerance != ImportOptions::default().curve_tolerance
        {
            out.tolerance = Some(tolerance);
        }
        // The dialog always answers, so its line never asks: moved to the origin, the data is in
        // no system and there is nothing to take.
        if !out.local {
            out.take_coordinate_system = Some(self.take_crs);
        }
        out
    }

    pub fn line(&self) -> String {
        if !self.check().is_empty() {
            return String::new();
        }
        format_import_line(&self.arguments()).unwrap_or_default()
    }

    pub fn check(&self) -> String {
        let file = path_of(&self.file);
        if file.is_empty() {
            return String::from("Name the IFC file to import.");
        }
        if !is_ifc_name(&file) {
            return String::from("The file must end in .ifc.");
        }
        if parse_finite_double(self.tolerance.trim()).is_none_or(|tolerance| tolerance <= 0.0) {
            return String::from("The curve tolerance must be a positive number of metres.");
        }
        if !self.alignments && !self.elements && !self.surfaces {
            return String::from("Nothing to import: tick alignments, elements or surfaces.");
        }
        if self.file.contains('"') {
            return String::from(
                "A path holding a double quote cannot be written in a command; rename the file.",
            );
        }
        String::new()
    }

    fn recheck(&mut self) {
        let problem = self.check();
        if !problem.is_empty() {
            self.showing_result = false;
            self.say(&problem, true);
            return;
        }
        if self.showing_result {
            return;
        }
        self.say(
            "Ready to import, as one step Undo takes back, surfaces included.",
            false,
        );
    }

    fn say(&mut self, text: &str, is_error: bool) {
        self.check_text = String::from(text);
        self.check_error = is_error;
    }

    /// What ifcImportCheck says.
    pub fn check_label(&self) -> &str {
        &self.check_text
    }

    pub fn check_is_error(&self) -> bool {
        self.check_error
    }

    /// What ifcImportSummary shows.
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The file's field set, as code sets it: a changed file clears what was described of the
    /// last one.
    pub fn set_file(&mut self, path: &str) {
        let text = to_native_separators(path);
        if self.file != text {
            self.file = text;
            self.file_changed();
        }
    }

    fn file_changed(&mut self) {
        self.showing_result = false;
        self.summary.clear();
        self.recheck();
    }

    fn option_changed(&mut self) {
        self.showing_result = false;
        self.recheck();
    }

    /// Shows what the file holds, as Describe does (INFO's reply), from a description made
    /// elsewhere (--import-options makes it first).
    pub fn set_summary(&mut self, text: &str) {
        self.summary = String::from(text);
        // Read, the file's earlier failure no longer stands.
        self.showing_result = false;
        self.recheck();
    }

    /// Describe: INFO run on the file, its reply shown.
    pub fn describe(&mut self, host: &mut dyn Host) {
        let path = path_of(&self.file);
        if path.is_empty() {
            return;
        }
        let Some(run_line) = self.run_line.as_mut() else {
            return;
        };
        let described = match format_info_line(&path) {
            Ok(written) => run_line(host, &written),
            Err(error) => Err(error),
        };
        match described {
            Ok(text) => self.set_summary(&text),
            Err(error) => {
                self.summary.clear();
                self.showing_result = true;
                self.say(&error.describe(), true);
            }
        }
    }

    /// Import: the line handed to the window, and what came in said.
    pub fn import_file(&mut self, host: &mut dyn Host) {
        let problem = self.check();
        if !problem.is_empty() {
            self.say(&problem, true);
            return;
        }
        let line = self.line();
        let reply = match self.run_line.as_mut() {
            Some(run_line) => run_line(host, &line),
            None => return,
        };
        self.showing_result = true;
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                // Declining the far-apart question is a cancel, not a failure.
                let cancelled = error.code == ErrorCode::CommandRejected;
                let said = if cancelled {
                    String::from("Import cancelled.")
                } else {
                    error.describe()
                };
                self.say(&said, !cancelled);
                return;
            }
        };
        let head = head_record(&reply);
        self.say(
            &format!(
                "Imported {} entities, {} alignments and {} surfaces from {}: the log has the \
                 reply.",
                field(&head, "entities"),
                field(&head, "alignments"),
                field(&head, "surfaces"),
                path_of(&self.file)
            ),
            false,
        );
    }

    /// Browse: a file dialog on the desktop, then Describe; headless, a sentence.
    fn browse(&mut self, host: &dyn Host) {
        if host.headless() {
            self.say(
                "Browse asks a person to choose a file; type its path instead.",
                true,
            );
            self.showing_result = true;
            return;
        }
        let chosen = get_open_file_name(
            "Import IFC",
            &path_of(&self.file),
            "IFC (*.ifc);;All files (*)",
        );
        if !chosen.is_empty() {
            let text = to_native_separators(&chosen);
            if self.file != text {
                self.file = text;
                self.file_changed();
            }
            self.pending_describe = true;
        }
    }

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent = Lent {
            dialog: &mut *self,
            host: &mut *host,
        };
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| lent.dialog.body(ui, &mut *lent.host),
            |ui, lent| lent.dialog.footer(ui, &mut *lent.host),
        );
        self.frame = frame;
        if shown.closed {
            self.hide();
        }
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.set_max_width(600.0);
        ui.horizontal(|ui| {
            ui.label("File:");
            if path_line(
                ui,
                "ifcImportFile",
                &mut self.file,
                "an IFC file: IFC2X3, IFC4 or IFC4X3",
            ) {
                self.file_changed();
            }
            if controls::button(ui, "ifcImportBrowse", "Browse...").clicked() {
                self.browse(&*host);
            }
        });
        if std::mem::take(&mut self.pending_describe) {
            self.describe(&mut *host);
        }
        let mut summary = self.summary.clone();
        controls::text_with(
            ui,
            "ifcImportSummary",
            &mut summary,
            TextOptions {
                read_only: true,
                rows: 10,
                monospace: true,
            },
        );
        if self.summary.is_empty() {
            ui.weak(
                "What the file holds - its schema, coordinate system, classes, alignments and \
                 surfaces - shown by Describe.",
            );
        }
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Bring in:");
            changed |= controls::check(
                ui,
                "ifcImportAlignments",
                "Alignments",
                &mut self.alignments,
            )
            .on_hover_text(
                "Alignments as PIs and PVIs, checked against the file to 10 mm; else their \
                     exact geometry as a polyline",
            )
            .changed();
            changed |= controls::check(
                ui,
                "ifcImportElements",
                "Elements and annotations",
                &mut self.elements,
            )
            .on_hover_text("Elements and annotations, with their property sets")
            .changed();
            changed |= controls::check(ui, "ifcImportSurfaces", "Surfaces", &mut self.surfaces)
                .on_hover_text(
                    "Terrain as the drawing's surfaces (saved with the project, and taken back by \
                     Undo with the import)",
                )
                .changed();
        });
        ui.horizontal(|ui| {
            ui.label("Place:");
            changed |= controls::check(
                ui,
                "ifcImportLocal",
                "Move to the origin (LOCAL)",
                &mut self.local,
            )
            .on_hover_text("Move everything to sit at the origin, as one piece")
            .changed();
        });
        // Moved to the origin, the data is no longer in the file's system.
        ui.add_enabled_ui(!self.local, |ui| {
            changed |= controls::check(
                ui,
                "ifcImportTakeCrs",
                "Take the file's coordinate system when the project has none",
                &mut self.take_crs,
            )
            .on_hover_text(
                "When the file names an EPSG code and the project has no coordinate system, the \
                 project takes the file's",
            )
            .changed();
        });
        ui.horizontal(|ui| {
            ui.label("Curves within (m):");
            changed |= controls::line(ui, "ifcImportTolerance", &mut self.tolerance)
                .on_hover_text(
                    "How far a chorded curve - an arc in a polycurve, a transition other than the \
                     clothoid - may stand from the true one, in metres",
                )
                .changed();
        });
        if changed {
            self.option_changed();
        }
        ui.horizontal(|ui| {
            ui.label("Command:");
            let line = self.line();
            command_field(ui, "ifcImportCommand", &line);
        });
    }

    /// The check and the buttons: kept below the scrolling body.
    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let check = self.check_text.clone();
        check_label(ui, "ifcImportCheck", &check, self.check_error);
        let import_live = self.check().is_empty();
        ui.horizontal(|ui| {
            if controls::button(ui, "ifcImportDescribe", "Describe")
                .on_hover_text("Read the file and say what it holds; nothing is imported")
                .clicked()
            {
                self.describe(&mut *host);
                ui.ctx().request_repaint();
            }
            ui.add_enabled_ui(import_live, |ui| {
                if controls::button(ui, "ifcImportImport", "Import").clicked() {
                    self.import_file(&mut *host);
                    ui.ctx().request_repaint();
                }
            });
            if controls::button(ui, "ifcImportClose", "Close").clicked() {
                self.hide();
            }
        });
    }
}
