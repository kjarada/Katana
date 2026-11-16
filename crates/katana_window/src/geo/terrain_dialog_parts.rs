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
use crate::drive::{Choice, LineOptions, button, choice, line_with, primary};
use crate::host::Host;
use crate::shell::{DialogFrame, dialog};

/// Why a dialog that reads a surface writes no line while the drawing has none: one sentence in
/// every such dialog, where three said it three ways ("there is no surface; make one first
/// (Terrain > Surface From)", "the drawing has no surface yet; make one with ...", "there is no
/// surface to export; build one from the Terrain menu"). No field's name before it: the
/// condition is the drawing's, not a field's.
pub(crate) const NO_SURFACE: &str = "There is no surface: make one with Terrain > Surface From";

/// What a choice of the drawing's surfaces says while the drawing has none
/// (drive::controls::choice_with_hint), muted in the control: the Surface Edit toolbar's, the
/// surface edit dialogs', Volumes', Combine Surfaces' and Export Surface as DEM's.
pub(crate) const NO_SURFACE_HINT: &str = "No surface";

/// What a choice of the drawing's surfaces and reference rasters says while it has neither
/// (terrain_dialog_support::SourcePicker).
pub(crate) const NO_SOURCE_HINT: &str = "No surface or raster";

/// Why a dialog that reads a surface or a raster writes no line while the drawing has neither,
/// in the same words in each (Contours, Drape and Sample, Slope, Shading, Viewshed, Statistics
/// by Area), where each said it with its own verb.
pub(crate) const NO_SURFACE_OR_RASTER: &str =
    "There is no surface or raster: make a surface with Terrain > Surface From, or import a DEM";

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

/// A form's row: its label in the column every dialog's form shares, then what `add` draws
/// beside it, a field filling the rest (QFormLayout::addRow; shell::dialog::labelled).
pub(crate) fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    dialog::labelled(ui, label, add)
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

/// The Command row: the line Run will run, read-only, and empty while the fields write none. Why
/// they write none is the footer's status (`run_buttons`), not the field's placeholder: a
/// sentence in the field read as the command, and was cut with "..." at the field's end ("Surface:
/// there is no surface to export; build one from the Terrain ..."). A dialog's status has one home,
/// the footer's left.
pub(crate) fn command_field(ui: &mut Ui, name: &str, made: &Result<String>) -> Response {
    let mut shown = made.clone().unwrap_or_default();
    row(ui, "Command", |ui| {
        line_with(
            ui,
            name,
            &mut shown,
            LineOptions {
                read_only: true,
                ..LineOptions::default()
            },
        )
    })
}

/// Why a run dialog's fields write no line - the message of `made`'s error - or "" when they write
/// one: what its footer's status says until something is run.
pub(crate) fn why_no_line(made: &Result<String>) -> String {
    match made {
        Ok(_) => String::new(),
        Err(error) => error.message.clone(),
    }
}

/// Why a dialog's fields write no line, as its footer's status says it: `message` - an error's
/// words, which start in lower case to follow a field's name ("the scope: ...") - begun with a
/// capital, and nothing put before it. The GIS and utility dialogs put "Nothing to run yet: "
/// before it, and with the field prefixes their errors already carry a status read as three
/// prefixes in a row (Clip's "Nothing to run yet: the boundary: the scope: tick ...").
pub(crate) fn why_status(message: &str) -> String {
    let mut characters = message.trim().chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

/// A run dialog's status (its footer's left): the last thing said - what a run answered, a
/// refusal - and, while nothing has been said, why the fields write no line, as a hint in the
/// secondary text. Why there was no line was the Command field's placeholder, where a sentence
/// read as the command and was cut with "..." at the field's end; a dialog's status has one home.
///
/// A status said is put away when the fields stop writing a line - the line it spoke of is gone -
/// unless it was said since the line was last read (`read_line`): a picker's refusal or a text the
/// steps cannot follow comes with the change of line it explains.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RunStatus {
    said: String,
    error: bool,
    why: String,
    fresh: bool,
}

impl RunStatus {
    /// Says `text`, in the error colour when `error`.
    pub(crate) fn set(&mut self, text: &str, error: bool) {
        self.said = String::from(text);
        self.error = error;
        self.fresh = true;
    }

    /// The last thing said ("" for nothing), and whether it was a refusal.
    pub(crate) fn said(&self) -> &str {
        &self.said
    }

    pub(crate) fn is_error(&self) -> bool {
        self.error
    }

    /// Takes in why the fields write no line now ("" when they write one).
    pub(crate) fn read_line(&mut self, why: String) {
        if why != self.why && !why.is_empty() && !self.fresh {
            self.said.clear();
            self.error = false;
        }
        self.why = why;
        self.fresh = false;
    }

    /// What the status shows and whether in the error colour (shell::dialog::status_colour).
    pub(crate) fn shown(&self) -> (String, bool) {
        if self.said.is_empty() {
            (self.why.clone(), false)
        } else {
            (self.said.clone(), self.error)
        }
    }
}

/// Which of a run dialog's footer buttons (`run_buttons`) were clicked this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RunPressed {
    pub preview: bool,
    pub run: bool,
    pub close: bool,
}

/// A run dialog's row of buttons, the design's one footer (shell::dialog::button_row): at the
/// left Preview, when it is offered - its name and whether it is enabled - as it reads the drawing
/// and changes nothing, then the status - its label's name and why the fields write no line
/// (`why_no_line`), "" when they write one - in the secondary text, a hint before anything was done
/// (shell::dialog::status_colour); at the right Close, when the row has one (a tab's row drawn off
/// the page has none: the dialog's Close is the page in front's), then the button that runs the
/// line - its name, its text and whether it is enabled - in the accent, last.
pub(crate) fn run_buttons(
    ui: &mut Ui,
    preview: Option<(&str, bool)>,
    status: (&str, &str),
    run: (&str, &str, bool),
    close: Option<&str>,
) -> RunPressed {
    let (previewed, (closed, ran)) = dialog::button_row(
        ui,
        |ui| {
            let previewed = preview.is_some_and(|(name, ready)| {
                ui.add_enabled_ui(ready, |ui| button(ui, name, "Preview").clicked())
                    .inner
            });
            let (name, text) = status;
            dialog::status(ui, name, text, false);
            previewed
        },
        |ui| {
            let closed = close.is_some_and(|name| button(ui, name, "Close").clicked());
            let (name, text, ready) = run;
            let ran = ui
                .add_enabled_ui(ready, |ui| primary(ui, name, text).clicked())
                .inner;
            (closed, ran)
        },
    );
    RunPressed {
        preview: previewed,
        run: ran,
        close: closed,
    }
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
            labels: items
                .iter()
                .map(|(label, _)| String::from(*label))
                .collect(),
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
    // A whole number of rows, so the fold never cuts a box in half (Statistics by Area's fifth
    // statistic showed its top half): `height` held down to the rows it holds whole.
    let spacing = ui.spacing().item_spacing.y;
    let row = ui.spacing().interact_size.y + spacing;
    let whole_rows = ((height + spacing) / row).floor().max(1.0);
    let rect = egui::ScrollArea::vertical()
        .id_salt(name)
        .max_height(whole_rows * row - spacing)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (at, (label, on)) in items.iter_mut().enumerate() {
                if shows_current {
                    ui.horizontal(|ui| {
                        if crate::drive::controls::check_box(ui, on, "").changed() {
                            changed = true;
                        }
                        if ui
                            .selectable_label(*current == Some(at), label.as_str())
                            .clicked()
                        {
                            *current = Some(at);
                        }
                    });
                } else if crate::drive::controls::check_box(ui, on, label.as_str()).changed() {
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
