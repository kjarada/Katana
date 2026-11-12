//! Help > Keyboard Shortcuts (keyboard_shortcuts_dialog.hpp; docs/desktop.md, "The Command
//! Reference and the keyboard shortcuts"): every key the window answers to, where the command is
//! in the menus and what it does, searchable; a key two commands share - which then reaches
//! neither - is marked, from the same check the headless --check-shortcuts runs (the window's
//! shortcut_clashes, K's).
//!
//! The dialog is non-modal and changes nothing. Object names:
//!
//! ```text
//! keyboardShortcutsDialog    the dialog (helpKeyboardShortcuts)
//! keyboardShortcutsSearch    the filter: every word must occur in a row
//! keyboardShortcutsTable     Key | Command | Menu | What it does; a key in a clash is marked
//!                            "(clash)" and drawn in the error colour
//! keyboardShortcutsClashes   the clashes, or that there are none
//! keyboardShortcutsCount     how many rows the filter shows
//! keyboardShortcutsClose     close
//! ```
//!
//! The table is recorded for a driver as Qt's model held it: every row, a filtered one too, its
//! cells joined by " | " (main.cpp's report of an item view).

use std::collections::BTreeSet;

use egui::{RichText, Ui};

use crate::command_log::contains_ignoring_case;
use crate::drive::{
    ControlKind, ControlRecord, LineOptions, button, custom, label, label_as, line_with,
};
use crate::shell::DialogFrame;
use crate::theme::palette;

/// One key, as the window's actions give it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShortcutRow {
    /// QKeySequence::PortableText ("Ctrl+Shift+E"), as shortcut_clashes names a key.
    pub key: String,
    /// As the menu shows it, without its '&'.
    pub command: String,
    /// "File", "Draw > Circle"; "(no menu)" for a key no menu shows.
    pub menu: String,
    /// The status tip.
    pub tip: String,
}

/// A key as the platform writes it (QKeySequence's NativeText): "Ctrl+L" on Windows and Linux; on
/// macOS, where Ctrl is the Command key, the symbols in Qt's order - Control, Option, Shift,
/// Command - before the key ("⇧⌘L").
pub fn native_key_text(portable: &str) -> String {
    if !cfg!(target_os = "macos") {
        return String::from(portable);
    }
    let mut rest = portable;
    let mut held = (false, false, false, false);
    loop {
        if let Some(after) = rest.strip_prefix("Ctrl+") {
            held.3 = true;
            rest = after;
        } else if let Some(after) = rest.strip_prefix("Alt+") {
            held.1 = true;
            rest = after;
        } else if let Some(after) = rest.strip_prefix("Shift+") {
            held.2 = true;
            rest = after;
        } else if let Some(after) = rest.strip_prefix("Meta+") {
            held.0 = true;
            rest = after;
        } else {
            break;
        }
    }
    let mut text = String::new();
    for (on, sign) in [
        (held.0, '\u{2303}'),
        (held.1, '\u{2325}'),
        (held.2, '\u{21e7}'),
        (held.3, '\u{2318}'),
    ] {
        if on {
            text.push(sign);
        }
    }
    text.push_str(rest);
    text
}

/// The columns: Key, Command, Menu, What it does.
const COLUMNS: [&str; 4] = ["Key", "Command", "Menu", "What it does"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ShortcutsState {
    rows: Vec<ShortcutRow>,
    /// Each row's cells as shown: the key marked when it clashes.
    cells: Vec<[String; 4]>,
    clashing: Vec<bool>,
    clash_text: String,
    search: String,
    shown: Vec<bool>,
    filtered_for: Option<String>,
    /// A column the person sorted by, and whether descending; in the menus' order until then.
    sort: Option<(usize, bool)>,
    close: bool,
}

impl ShortcutsState {
    /// setFilter: every word typed must occur in a row (its cells, and the key as typed).
    fn filter(&mut self) {
        if self.filtered_for.as_deref() == Some(self.search.as_str()) {
            return;
        }
        self.filtered_for = Some(self.search.clone());
        let words: Vec<&str> = self
            .search
            .split(' ')
            .filter(|word| !word.is_empty())
            .collect();
        self.shown = self
            .rows
            .iter()
            .zip(&self.cells)
            .map(|(row, cells)| {
                // The key as typed, "Ctrl+L", is the key cell's data, for the search: on macOS the
                // cell shows the native "⌘L", which "ctrl+l" would not find.
                let mut haystack = String::new();
                for (column, text) in cells.iter().enumerate() {
                    haystack.push_str(text);
                    haystack.push(' ');
                    if column == 0 {
                        haystack.push_str(&row.key);
                    }
                    haystack.push(' ');
                }
                words
                    .iter()
                    .all(|word| contains_ignoring_case(&haystack, word))
            })
            .collect();
    }

    fn count_text(&self) -> String {
        let total = self.rows.len();
        if self.search.split(' ').all(str::is_empty) {
            format!("{total} keys")
        } else {
            let shown = self.shown.iter().filter(|shown| **shown).count();
            format!("{shown} of {total} keys")
        }
    }

    /// The rows in the order shown: the menus' order, or sorted by the column chosen.
    fn order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.rows.len()).collect();
        if let Some((column, descending)) = self.sort {
            order.sort_by(|a, b| {
                let ordering = self.cells[*a][column].cmp(&self.cells[*b][column]);
                if descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
        }
        order
    }

    fn body(&mut self, ui: &mut Ui) {
        line_with(
            ui,
            "keyboardShortcutsSearch",
            &mut self.search,
            LineOptions {
                placeholder: "Search: a key (Ctrl+L), a command or a menu",
                ..LineOptions::default()
            },
        );
        self.filter();
        let order = self.order();
        let error = palette::error().to_egui();
        let table = egui::Grid::new("keyboardShortcutsGrid")
            .striped(true)
            .show(ui, |ui| {
                for (column, title) in COLUMNS.iter().enumerate() {
                    if ui.button(RichText::new(*title).strong()).clicked() {
                        self.sort = match self.sort {
                            Some((sorted, descending)) if sorted == column => {
                                Some((column, !descending))
                            }
                            _ => Some((column, false)),
                        };
                    }
                }
                ui.end_row();
                for &at in &order {
                    if !self.shown.get(at).copied().unwrap_or(true) {
                        continue;
                    }
                    for text in &self.cells[at] {
                        let mut shown = RichText::new(text.as_str());
                        if self.clashing[at] {
                            shown = shown.color(error);
                        }
                        ui.label(shown);
                    }
                    ui.end_row();
                }
            });
        let mut record = ControlRecord::new("keyboardShortcutsTable", ControlKind::Rows);
        record.items = self.cells.iter().map(|cells| cells.join(" | ")).collect();
        custom(ui, &table.response, record);
        label_as(
            ui,
            "keyboardShortcutsClashes",
            &self.clash_text,
            egui::Label::new(self.clash_text.as_str()).wrap(),
        );
    }

    fn footer(&mut self, ui: &mut Ui) {
        let count = self.count_text();
        ui.horizontal(|ui| {
            label(ui, "keyboardShortcutsCount", &count);
            if button(ui, "keyboardShortcutsClose", "Close").clicked() {
                self.close = true;
            }
        });
    }
}

/// Help > Keyboard Shortcuts (see the module comment).
pub struct KeyboardShortcutsDialog {
    frame: DialogFrame,
    state: ShortcutsState,
    /// Whether it is shown.
    pub open: bool,
}

impl KeyboardShortcutsDialog {
    /// `clashes` are the window's shortcut_clashes lines, "Ctrl+L: formatLayers, Line".
    pub fn new(rows: Vec<ShortcutRow>, clashes: &[String]) -> Self {
        // The keys of the clashes: a clash line is "<key>: <what shares it>".
        let clashing_keys: BTreeSet<&str> = clashes
            .iter()
            .map(|clash| clash.split(": ").next().unwrap_or(""))
            .collect();
        let clashing: Vec<bool> = rows
            .iter()
            .map(|row| clashing_keys.contains(row.key.as_str()))
            .collect();
        let cells = rows
            .iter()
            .zip(&clashing)
            .map(|(row, clash)| {
                let mut key = native_key_text(&row.key);
                if *clash {
                    key.push_str(" (clash)");
                }
                [key, row.command.clone(), row.menu.clone(), row.tip.clone()]
            })
            .collect();
        let clash_text = if clashes.is_empty() {
            String::from("Every key reaches one command.")
        } else {
            format!(
                "{} {} more than one command, and Qt gives such a key to none of them: {}",
                clashes.len(),
                if clashes.len() == 1 {
                    "key reaches"
                } else {
                    "keys reach"
                },
                clashes.join("; ")
            )
        };
        let mut state = ShortcutsState {
            rows,
            cells,
            clashing,
            clash_text,
            ..ShortcutsState::default()
        };
        state.filter();
        Self {
            frame: DialogFrame::new("keyboardShortcutsDialog", "Keyboard Shortcuts"),
            state,
            open: true,
        }
    }

    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    /// Shows the rows matching `text` (what typing in the search does).
    pub fn set_filter(&mut self, text: &str) {
        self.state.search = String::from(text);
        self.state.filter();
    }

    /// How many rows the filter shows.
    pub fn shown_rows(&self) -> usize {
        self.state.shown.iter().filter(|shown| **shown).count()
    }

    /// Draws the dialog while it is open.
    pub fn ui(&mut self, ui: &mut Ui) {
        if !self.open {
            return;
        }
        let ctx = ui.ctx().clone();
        let shown = self.frame.show(
            &ctx,
            &mut self.state,
            |ui, state| state.body(ui),
            |ui, state| state.footer(ui),
        );
        if shown.closed || std::mem::take(&mut self.state.close) {
            self.open = false;
        }
    }
}
