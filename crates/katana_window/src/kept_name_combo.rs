//! A plain choice of names - hatch patterns, dimension styles - that keeps a name it cannot list,
//! as NamePicker does for linetypes and symbols (audit QT-02, decision D3). The style manager and
//! the layer manager share it. The port of kept_name_combo.hpp.
//!
//! The rule: a form shows what is STORED, and writes back what is shown. A choice that silently
//! fails to show a name it lacks shows the previous row's name instead, and Save then writes
//! that - which is how an imported hatch or dimension style name was lost. So an unknown name is
//! put in the list, first, marked "(not defined)" in NamePicker's amber, and read back exactly.
//!
//! A form over several items whose values differ shows a "<varies>" row, which reads back as
//! None: "leave each item's own value".

use egui::{Response, Ui};

use crate::canvas::Color;
use crate::customisation::name_picker::UNDEFINED_NAME_COLOUR;
use crate::drive::controls::{self, Choice};

/// What the row for items that do not agree says.
pub const VARIES: &str = "<varies>";

/// One choice: what the list says, and the name it stands for ("(ByLayer)" for "", say).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeptChoice {
    pub label: String,
    pub name: String,
}

impl KeptChoice {
    pub fn new(label: &str, name: &str) -> Self {
        Self {
            label: String::from(label),
            name: String::from(name),
        }
    }
}

/// One row of the list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeptRow {
    pub label: String,
    /// The exact name; None on the <varies> row.
    pub name: Option<String>,
    /// True on a row `show` or `show_varies` inserted (a kept name, <varies>), which the next
    /// show removes again.
    pub inserted: bool,
    /// The colour a kept name is listed in; None for the theme's.
    pub foreground: Option<Color>,
    pub tool_tip: String,
}

/// See the module comment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeptNameCombo {
    rows: Vec<KeptRow>,
    current: Option<usize>,
}

impl KeptNameCombo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the list. What was shown is NOT kept: call `show` after. The first row is the one
    /// shown until then, as a QComboBox shows the first item put into it.
    pub fn fill(&mut self, choices: &[KeptChoice]) {
        self.rows = choices
            .iter()
            .map(|choice| KeptRow {
                label: choice.label.clone(),
                name: Some(choice.name.clone()),
                ..KeptRow::default()
            })
            .collect();
        self.current = if self.rows.is_empty() { None } else { Some(0) };
    }

    fn remove_inserted(&mut self) {
        let current_row = self.current.and_then(|at| self.rows.get(at)).cloned();
        self.rows.retain(|row| !row.inserted);
        // QComboBox::removeItem keeps the same item current where it can, else the row that took
        // its place.
        self.current = match current_row {
            Some(row) if !row.inserted => self.rows.iter().position(|kept| *kept == row),
            Some(_) if !self.rows.is_empty() => Some(0),
            _ => None,
        };
    }

    /// Shows `name`, inserting it marked when no row holds it exactly.
    pub fn show(&mut self, name: &str) {
        self.remove_inserted();
        if let Some(at) = self
            .rows
            .iter()
            .position(|row| row.name.as_deref() == Some(name))
        {
            self.current = Some(at);
            return;
        }
        self.rows.insert(
            0,
            KeptRow {
                label: format!("{name} (not defined)"),
                name: Some(String::from(name)),
                inserted: true,
                foreground: Some(UNDEFINED_NAME_COLOUR),
                tool_tip: format!(
                    "Nothing in this drawing defines \"{name}\"; it is kept as it is until you \
                     choose another"
                ),
            },
        );
        self.current = Some(0);
    }

    /// Shows "<varies>": the items being edited do not agree.
    pub fn show_varies(&mut self) {
        self.remove_inserted();
        self.rows.insert(
            0,
            KeptRow {
                label: String::from(VARIES),
                name: None,
                inserted: true,
                ..KeptRow::default()
            },
        );
        self.current = Some(0);
    }

    /// The name shown, exactly; None on the <varies> row (or an empty list).
    pub fn current(&self) -> Option<String> {
        self.current
            .and_then(|at| self.rows.get(at))
            .and_then(|row| row.name.clone())
    }

    /// QComboBox::currentText: what the row shown says.
    pub fn current_text(&self) -> String {
        self.current
            .and_then(|at| self.rows.get(at))
            .map(|row| row.label.clone())
            .unwrap_or_default()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    /// QComboBox::setCurrentIndex, for a row there is.
    pub fn set_current_index(&mut self, index: usize) {
        if index < self.rows.len() {
            self.current = Some(index);
        }
    }

    pub fn rows(&self) -> &[KeptRow] {
        &self.rows
    }

    /// QComboBox::findText: the row saying exactly `label`.
    pub fn find_text(&self, label: &str) -> Option<usize> {
        self.rows.iter().position(|row| row.label == label)
    }

    /// Draws it as the choice `name`; `changed()` when a person or a driver chose another row.
    pub fn ui(&mut self, ui: &mut Ui, name: &str) -> Response {
        let labels: Vec<String> = self.rows.iter().map(|row| row.label.clone()).collect();
        let mut choice = Choice {
            index: self.current,
            ..Choice::default()
        };
        let mut response = controls::choice(ui, name, &labels, &mut choice);
        if response.changed() {
            self.current = choice.index;
        }
        let tip = self
            .current
            .and_then(|at| self.rows.get(at))
            .map(|row| row.tool_tip.clone())
            .unwrap_or_default();
        if !tip.is_empty() {
            response = response.on_hover_text(tip);
        }
        response
    }
}
