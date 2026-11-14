//! What the terrain and geoprocessing dialogs of wave 2 share beyond terrain_dialog_support and
//! geo_dialog_support, which the C++ wrote out in each dialog: the dialog in its frame, the Command
//! field with why there is no line as its placeholder, the row of Preview and Run, a choice whose
//! items carry the verb's word, a list of boxes to tick, and a list of points.
//!
//! It has no C++ file. In Qt each dialog built these from the same five or six calls; in egui each
//! is a function drawn every frame, and writing it once keeps the dialogs to what differs between
//! them.

use egui::{Response, Ui};
use katana_core::error::make_error;
use katana_core::text::parse_finite_double;
use katana_core::{Error, ErrorCode, Result};

use crate::customisation::scope_filter_views;
use crate::customisation::scope_filter_widget::ViewsFn;
use crate::drive::mailbox::{Act, fill_disabled, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, record, settle, take};
use crate::drive::{Choice, LineOptions, button, choice, line_with};
use crate::host::Host;
use crate::shell::DialogFrame;

/// InvalidArgument saying `message`: why a dialog's fields write no line.
pub(crate) fn invalid(message: impl Into<String>) -> Error {
    make_error(ErrorCode::InvalidArgument, message, "")
}

/// QString::toDouble of a text already known to read as a number.
pub(crate) fn number_of(text: &str) -> f64 {
    parse_finite_double(text.trim()).unwrap_or(0.0)
}

/// QString::remove(' '): a list as a line writes it, its blanks dropped.
pub(crate) fn without_blanks(text: &str) -> String {
    text.replace(' ', "")
}

/// The workspace's open views as a scope's View choice asks for them; None where the window has no
/// workspace (a test, a headless batch), and the View choice then names no view a line can carry.
pub(crate) fn host_views(host: &mut dyn Host) -> Option<ViewsFn> {
    host.views().map(|access| -> ViewsFn {
        let open = access.view_set();
        Box::new(move || scope_filter_views(&open.borrow()))
    })
}

/// A dialog drawn in the dialog frame: its body, which scrolls when it does not fit, and its
/// footer - its buttons and what they said.
pub(crate) trait Framed {
    fn frame_mut(&mut self) -> &mut DialogFrame;
    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host);
    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host);
}

/// One frame of `dialog` in its frame, lent the window; true when its title bar's close button was
/// clicked.
pub(crate) fn show_framed<D: Framed>(dialog: &mut D, ui: &mut Ui, host: &mut dyn Host) -> bool {
    let ctx = ui.ctx().clone();
    let mut frame = dialog.frame_mut().clone();
    let mut lent: (&mut D, &mut dyn Host) = (dialog, host);
    let shown = frame.show(
        &ctx,
        &mut lent,
        |ui, lent| lent.0.body(ui, &mut *lent.1),
        |ui, lent| lent.0.footer(ui, &mut *lent.1),
    );
    *lent.0.frame_mut() = frame;
    shown.closed
}

/// A form's row: its label, then what `add` draws beside it (QFormLayout::addRow).
pub(crate) fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        if !label.is_empty() {
            ui.label(label);
        }
        add(ui)
    })
    .inner
}

/// A one-line field with its placeholder.
pub(crate) fn field(ui: &mut Ui, name: &str, value: &mut String, placeholder: &str) -> Response {
    line_with(
        ui,
        name,
        value,
        LineOptions {
            placeholder,
            ..LineOptions::default()
        },
    )
}

/// The Command row: the line Run will run, read-only, or - as its placeholder - why the fields
/// write none.
pub(crate) fn command_field(ui: &mut Ui, name: &str, made: &Result<String>) -> Response {
    let (mut shown, placeholder) = match made {
        Ok(line) => (line.clone(), String::new()),
        Err(error) => (String::new(), error.message.clone()),
    };
    row(ui, "Command:", |ui| {
        line_with(
            ui,
            name,
            &mut shown,
            LineOptions {
                placeholder: &placeholder,
                read_only: true,
                ..LineOptions::default()
            },
        )
    })
}

/// The row of buttons under a dialog's fields: Preview, when it is offered - its name and whether
/// it is enabled - and the button that runs the line - its name, its text and whether it is
/// enabled. Which were clicked: Preview, then Run.
pub(crate) fn run_buttons(
    ui: &mut Ui,
    preview: Option<(&str, bool)>,
    run: (&str, &str, bool),
) -> (bool, bool) {
    ui.horizontal(|ui| {
        let previewed = preview.is_some_and(|(name, ready)| {
            ui.add_enabled_ui(ready, |ui| button(ui, name, "Preview").clicked())
                .inner
        });
        let (name, text, ready) = run;
        let ran = ui
            .add_enabled_ui(ready, |ui| button(ui, name, text).clicked())
            .inner;
        (previewed, ran)
    })
    .inner
}

/// A choice whose items carry the word a line writes (a QComboBox's item data): what it shows and
/// what it means, the first chosen as a QComboBox chooses the first item added.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WordChoice {
    labels: Vec<String>,
    words: Vec<String>,
    pub chosen: Choice,
}

impl WordChoice {
    /// The items, each its label and its word.
    pub(crate) fn new(items: &[(&str, &str)]) -> Self {
        Self {
            labels: items.iter().map(|(label, _)| String::from(*label)).collect(),
            words: items.iter().map(|(_, word)| String::from(*word)).collect(),
            chosen: if items.is_empty() {
                Choice::default()
            } else {
                Choice::at(0)
            },
        }
    }

    /// QComboBox::currentData: the chosen item's word, empty when none is chosen.
    pub(crate) fn word(&self) -> &str {
        self.chosen
            .index
            .and_then(|at| self.words.get(at))
            .map_or("", String::as_str)
    }

    /// setCurrentIndex(findData(word)): false, and nothing changed, when no item carries it.
    pub(crate) fn set_word(&mut self, word: &str) -> bool {
        match self.words.iter().position(|listed| listed == word) {
            Some(at) => {
                self.chosen.index = Some(at);
                true
            }
            None => false,
        }
    }

    pub(crate) fn ui(&mut self, ui: &mut Ui, name: &str) -> Response {
        choice(ui, name, &self.labels, &mut self.chosen)
    }
}

/// A list of boxes to tick (a QListWidget of checkable items) named `name`, at most `height` tall.
/// A fill makes the row of that text current, as fillField's does, which ticks nothing. True when a
/// box was ticked or cleared.
pub(crate) fn check_list(
    ui: &mut Ui,
    name: &str,
    items: &mut [(String, bool)],
    height: f32,
) -> bool {
    let mut current = None;
    check_list_with_current(ui, name, items, &mut current, height, false)
}

/// `check_list` whose rows can also be picked - `current` the row picked, shown as picked when
/// `shows_current` - for a list whose buttons move a row. A fill picks the row of that text.
pub(crate) fn check_list_with_current(
    ui: &mut Ui,
    name: &str,
    items: &mut [(String, bool)],
    current: &mut Option<usize>,
    height: f32,
    shows_current: bool,
) -> bool {
    let taken = take(ui, name);
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if ui.is_visible() && !ui.is_enabled() {
                    settle(ui, taken, Err(fill_disabled(&taken.posted)));
                } else if let Some(at) = items.iter().position(|(listed, _)| listed == text) {
                    *current = Some(at);
                    settle(ui, taken, Ok(()));
                } else {
                    settle(ui, taken, Err(lists_no(name, text)));
                }
            }
            Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
            Act::ScrollTo => {}
        }
    }
    let mut changed = false;
    let rect = egui::ScrollArea::vertical()
        .id_salt(name)
        .max_height(height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (at, (label, on)) in items.iter_mut().enumerate() {
                if shows_current {
                    ui.horizontal(|ui| {
                        if ui.checkbox(on, "").changed() {
                            changed = true;
                        }
                        if ui
                            .selectable_label(*current == Some(at), label.as_str())
                            .clicked()
                        {
                            *current = Some(at);
                        }
                    });
                } else if ui.checkbox(on, label.as_str()).changed() {
                    changed = true;
                }
            }
        })
        .inner_rect;
    let response = ui.interact(rect, ui.id().with(name), egui::Sense::hover());
    if let Some(taken) = &taken
        && taken.posted.act == Act::ScrollTo
    {
        response.scroll_to_me(None);
        settle(ui, taken, Ok(()));
    }
    let mut list = ControlRecord::new(name, ControlKind::Rows);
    list.items = items.iter().map(|(label, _)| label.clone()).collect();
    list.enabled = ui.is_enabled();
    list.visible = ui.is_visible();
    record(ui, &response, list);
    changed
}

/// The rows of a list of one column, as `drive::rows` takes them.
pub(crate) fn one_column(items: &[String]) -> Vec<Vec<String>> {
    items.iter().map(|item| vec![item.clone()]).collect()
}

/// `delete list->currentItem()`: the current row removed, when there is one; the row that took its
/// place, else the last, is current after, as a QListWidget moves its current row.
pub(crate) fn remove_current(items: &mut Vec<String>, current: &mut Option<usize>) {
    let Some(at) = *current else {
        return;
    };
    if at >= items.len() {
        *current = None;
        return;
    }
    items.remove(at);
    *current = if items.is_empty() {
        None
    } else {
        Some(at.min(items.len() - 1))
    };
}
