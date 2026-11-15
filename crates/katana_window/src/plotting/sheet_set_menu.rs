//! The Sheets editor's Sheet Set menu (docs/plotting.md, "The Sheet Set menu and Page Setup"): the
//! whole set kept in a JSON file and brought back, another set's sheets appended, the set copied
//! as JSON, and the page setup changed without plotting. The port of plotting/sheet_set_menu.
//!
//! Every item runs the SHEETS line a person would type through the editor (SheetEditor::run_line)
//! (in the window, its one executor), so the log shows what to type, the history keeps it, and
//! an edit is ONE undoable step:
//!
//! ```text
//! Save Sheet Set As...          sheetSaveSet     SHEETS SAVE "path"
//! Load Sheet Set...             sheetLoadSet     SHEETS LOAD "path"      LOAD_SHEETS
//! Append Sheets From File...    sheetAppendSet   SHEETS APPEND "path"    APPEND_SHEETS
//! Copy Sheet Set as JSON        sheetCopySetJson SHEETS JSON, its reply to the clipboard
//! Page Setup...                 sheetPageSetup   SHEETS PAGESETUP ...    PAGE_SETUP
//! ```
//!
//! Load asks before it replaces sheets there are (sheetLoadSetQuestion: "Undo brings them back").
//! A headless session opens no file dialog and asks nothing: the three file items name their verb
//! instead. Page Setup opens without waiting (pageSetupDialog, the Plot dialog's page-setup mode),
//! so a headless session fills it by its object names.

use katana_cad::document::Document;
use katana_core::path_text::generic_path_utf8;

use crate::drive::action::{Action, Menu};
use crate::host::Host;
use crate::plotting::plot_dialog::{PlotDialog, PlotDialogMode};
use crate::sheet_editor::{FileAsk, QuestionAsk, SheetEditor};

const SHEET_SET_FILES: &str = "Sheet sets (*.json)";

/// What the Sheet Set menu keeps between its uses: nothing yet - its items are the editor's
/// methods, and its one dialog is the editor's Plot dialog in its page-setup mode.
#[derive(Clone, Copy, Debug, Default)]
pub struct SheetSetMenu;

/// Where the file dialogs start: the project's folder, else the documents.
fn start_folder(document: &Document) -> String {
    match document.project_directory() {
        Some(directory) => directory.to_string_lossy().into_owned(),
        None => {
            let home = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            home.join("Documents").to_string_lossy().into_owned()
        }
    }
}

/// A path as a SHEETS line takes it: in double quotes, for the spaces, with '/' (Qt's one
/// separator, QDir::fromNativeSeparators).
fn quoted_path(path: &str) -> String {
    let generic = String::from_utf8_lossy(&generic_path_utf8(std::path::Path::new(path)))
        .replace('\\', "/");
    format!("\"{generic}\"")
}

impl SheetEditor {
    /// The Sheet Set menu (sheetSetMenu), first in the menu bar.
    pub(crate) fn sheet_set_menu(&self) -> Menu {
        Menu::new("sheetSetMenu", "Sheet &Set")
            .action(Action::new("sheetSaveSet", "&Save Sheet Set As...").tip(
                "Keep every sheet, its views, the title block and the page setup in a JSON file \
                 (SHEETS SAVE)",
            ))
            .action(Action::new("sheetLoadSet", "&Load Sheet Set...").tip(
                "Replace the sheets with a set kept in a JSON file; Undo brings them back (SHEETS \
                 LOAD)",
            ))
            .action(Action::new("sheetAppendSet", "&Append Sheets From File...").tip(
                "Put another set's sheets after these, their ids renumbered (SHEETS APPEND)",
            ))
            .action(Action::new("sheetCopySetJson", "&Copy Sheet Set as JSON").tip(
                "The whole set as the JSON the project keeps, on the clipboard (SHEETS JSON)",
            ))
            .separator()
            .action(
                Action::new("sheetPageSetup", "Page Set&up...")
                    .tip(
                        "How the set plots - colours, line weights, resolution, file names - kept \
                         with it without plotting (SHEETS PAGESETUP)",
                    )
                    // What a headless session's --dialog finds it by.
                    .opens("pageSetupDialog"),
            )
    }

    /// The menu's items triggered by name; false for a name that is none of them.
    pub(crate) fn trigger_sheet_set(&mut self, host: &mut dyn Host, name: &str) -> bool {
        match name {
            "sheetSaveSet" => self.save_sheet_set(host),
            "sheetLoadSet" => self.load_sheet_set(host),
            "sheetAppendSet" => self.append_sheets(host),
            "sheetCopySetJson" => self.copy_sheet_set_json(host),
            "sheetPageSetup" => {
                let _ = self.open_page_setup_dialog(&*host);
            }
            _ => return false,
        }
        true
    }

    /// A headless session has no one to pick a file or answer a question: the item says what to
    /// type instead, and does nothing.
    fn said_headless(&mut self, host: &mut dyn Host, verb: &str) -> bool {
        if !self.headless() {
            return false;
        }
        self.report(
            host,
            &format!("A headless session opens no file dialog: type {verb} \"path\" instead."),
            true,
        );
        true
    }

    fn save_sheet_set(&mut self, host: &mut dyn Host) {
        if self.said_headless(host, "SHEETS SAVE") {
            return;
        }
        let start = std::path::Path::new(&start_folder(host.document()))
            .join("sheets.json")
            .to_string_lossy()
            .into_owned();
        let mut path = self.ask_file(&FileAsk {
            save: true,
            title: String::from("Save Sheet Set As"),
            start,
            filter: String::from(SHEET_SET_FILES),
        });
        if path.is_empty() {
            return;
        }
        if !path.to_lowercase().ends_with(".json") {
            path.push_str(".json");
        }
        let _ = self.run_line(host, &format!("SHEETS SAVE {}", quoted_path(&path)));
    }

    fn load_sheet_set(&mut self, host: &mut dyn Host) {
        if self.said_headless(host, "SHEETS LOAD") {
            return;
        }
        let path = self.ask_file(&FileAsk {
            save: false,
            title: String::from("Load Sheet Set"),
            start: start_folder(host.document()),
            filter: String::from(SHEET_SET_FILES),
        });
        if path.is_empty() {
            return;
        }
        // Loading replaces every sheet. Undo brings them back, which the question says, so it is
        // no threat; with nothing to replace it is not asked.
        let document = host.document();
        let replaced = match document.sheet_set_status() {
            Err(error) => format!(
                "The project's sheets cannot be read ({}). Replace them? Undo brings them back.",
                error.message
            ),
            Ok(()) => {
                let count = document.sheet_set().sheets.len();
                if count > 0 {
                    format!(
                        "Replace the {count} sheet{} there are now? Undo brings them back.",
                        if count == 1 { "" } else { "s" }
                    )
                } else {
                    String::new()
                }
            }
        };
        if !replaced.is_empty()
            && !self.ask(&QuestionAsk {
                name: String::from("sheetLoadSetQuestion"),
                title: String::from("Load Sheet Set"),
                text: replaced,
            })
        {
            return;
        }
        let _ = self.run_line(host, &format!("SHEETS LOAD {}", quoted_path(&path)));
    }

    fn append_sheets(&mut self, host: &mut dyn Host) {
        if self.said_headless(host, "SHEETS APPEND") {
            return;
        }
        let path = self.ask_file(&FileAsk {
            save: false,
            title: String::from("Append Sheets From File"),
            start: start_folder(host.document()),
            filter: String::from(SHEET_SET_FILES),
        });
        if path.is_empty() {
            return;
        }
        let first = host.document().sheet_set().sheets.len();
        if self
            .run_line(host, &format!("SHEETS APPEND {}", quoted_path(&path)))
            .ok
        {
            self.set_current_sheet(&*host, first);
        }
    }

    fn copy_sheet_set_json(&mut self, host: &mut dyn Host) {
        let outcome = self.run_line(host, "SHEETS JSON");
        if outcome.ok {
            match host.clipboard().set_text(&outcome.reply) {
                Ok(()) => {
                    self.status = format!(
                        "The sheet set's JSON is on the clipboard: {} characters.",
                        outcome.reply.encode_utf16().count()
                    );
                }
                Err(error) => self.report(host, &error.describe(), true),
            }
        }
    }

    /// Page Setup for the editor's set, opened and not waited on: the Plot dialog in its
    /// page-setup mode, whose Save (pageSetupSave) runs PlotDialog::page_setup_line through the
    /// editor.
    pub fn open_page_setup_dialog(&mut self, host: &dyn Host) -> &mut PlotDialog {
        self.observe(host);
        let set = host.document().sheet_set();
        self.plot_dialog.insert(PlotDialog::new(
            &set,
            self.current_sheet(),
            true,
            "",
            PlotDialogMode::PageSetup,
        ))
    }
}
