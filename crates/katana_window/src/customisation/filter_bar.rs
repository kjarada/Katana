//! The row above a manager's table: a search field and a set of exclusive chips - "All / Used /
//! Unused / Missing" in the style manager, "All / Rules / Unmapped" in the code manager - so every
//! manager filters the same way. The port of customisation/filter_bar.
//!
//! The bar only reports; it filters nothing itself. A dialog keeps its filtering in its own rows
//! and re-applies it from what the bar reports, which is every change of the text and of the
//! checked chip - typed and clicked AND set by the setters below (a filter is state, not an edit:
//! re-applying it is always safe, and a dialog restoring a remembered filter wants its list to
//! follow). The C++ calls a callback for each; an immediate-mode bar hands the changes back from
//! `ui` (and from `take_changes`, for a setter's), in the order they happened.
//!
//! Object names, for tests and the headless driver: "filterText" for the search field, and
//! "filter<Label>" for each chip with everything but the letters and digits of its label removed:
//! "filterAll", "filterUnused", and "filterNotused3" for "Not used (3)".

use egui::Ui;

use crate::drive::controls::{LineOptions, line_with, toggle};

/// What changed in the bar since it was last asked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterBarChanges {
    /// The text after its last change, when it changed: typed or set, as typed (case is the
    /// caller's to fold: names are case-sensitive, a search is not - decision D3).
    pub text: Option<String>,
    /// Every chip turned on, in order: clicked or set. Only the chip turned on is news, not the
    /// one an exclusive set turned off.
    pub chips: Vec<usize>,
}

impl FilterBarChanges {
    pub fn any(&self) -> bool {
        self.text.is_some() || !self.chips.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Chip {
    label: String,
    name: String,
}

/// A search field and exclusive chips (see the module comment).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterBar {
    text: String,
    placeholder: String,
    chips: Vec<Chip>,
    chip: Option<usize>,
    changes: FilterBarChanges,
}

impl FilterBar {
    /// `chips` are the labels, in order; the first is checked. No chips gives a search field
    /// alone.
    pub fn new(chips: &[&str]) -> Self {
        Self {
            text: String::new(),
            placeholder: String::from("Search"),
            chips: chips
                .iter()
                .map(|label| Chip {
                    label: String::from(*label),
                    name: Self::chip_object_name(label),
                })
                .collect(),
            chip: if chips.is_empty() { None } else { Some(0) },
            changes: FilterBarChanges::default(),
        }
    }

    /// The object name a chip labelled `label` gets: "filter" and the label's letters and digits.
    pub fn chip_object_name(label: &str) -> String {
        let mut name = String::from("filter");
        name.extend(
            label
                .chars()
                .filter(|character| character.is_alphanumeric()),
        );
        name
    }

    /// The search text as typed.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Sets the search text; a change is reported as typing is.
    pub fn set_text(&mut self, text: &str) {
        if self.text != text {
            self.text = String::from(text);
            self.changes.text = Some(self.text.clone());
        }
    }

    pub fn set_placeholder_text(&mut self, text: &str) {
        self.placeholder = String::from(text);
    }

    /// The checked chip, in the order the labels were given; None when there are no chips (the
    /// C++'s -1).
    pub fn chip(&self) -> Option<usize> {
        self.chip
    }

    /// The checked chip's label as it shows now; empty with no chips.
    pub fn chip_label(&self) -> String {
        self.chip
            .and_then(|at| self.chips.get(at))
            .map(|chip| chip.label.clone())
            .unwrap_or_default()
    }

    /// Checks chip `index`; an index out of range is ignored. A change is reported as a click is.
    pub fn set_chip(&mut self, index: usize) {
        if index >= self.chips.len() || self.chip == Some(index) {
            return;
        }
        self.chip = Some(index);
        self.changes.chips.push(index);
    }

    /// A chip's label, changed after construction - "Missing (3)" once the dialog has counted.
    /// The object name stays the original label's. An index out of range is ignored.
    pub fn set_chip_label(&mut self, index: usize, label: &str) {
        if let Some(chip) = self.chips.get_mut(index) {
            chip.label = String::from(label);
        }
    }

    /// The changes since the last time they were taken (a setter's, between two frames).
    pub fn take_changes(&mut self) -> FilterBarChanges {
        std::mem::take(&mut self.changes)
    }

    /// Draws the bar in a row - the field taking the room the chips leave - and answers every
    /// change since it was last asked, this frame's last.
    pub fn ui(&mut self, ui: &mut Ui) -> FilterBarChanges {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let field = line_with(
                ui,
                "filterText",
                &mut self.text,
                LineOptions {
                    placeholder: &self.placeholder,
                    ..LineOptions::default()
                },
            );
            if field.changed() {
                self.changes.text = Some(self.text.clone());
            }
            for (at, chip) in self.chips.iter().enumerate() {
                let checked = self.chip == Some(at);
                // Clicking the checked chip leaves it checked: there is always a filter.
                if toggle(ui, &chip.name, &chip.label, checked).clicked() && !checked {
                    self.chip = Some(at);
                    self.changes.chips.push(at);
                }
            }
        });
        self.take_changes()
    }
}
