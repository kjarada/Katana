//! What the annotation dialogs share that is not a control (form_widgets has those): hearing the
//! drawing change, drawing a dialog in its frame with its host lent to its body and its footer,
//! and a one-line field's cursor, which the C++ dialogs had from QLineEdit.
//!
//! HEARING THE DRAWING. The C++ managers and the Edit Label and Edit Text dialogs add a Document
//! listener and refresh inside it, as the change is made. A Rust dialog cannot be reached from a
//! listener (it is `Send + 'static` and the dialog is the window's), so the listener only gathers
//! the parts each notification names (`Followed`), and the dialog reads them and refreshes at its
//! next look: the start of its frame, the start of each of its methods, and straight after a line
//! it ran. That is the same order as the C++'s: every change is seen before the dialog next reads
//! the drawing, and a line's own changes before the dialog goes on after running it. A refresh is
//! a re-read of the drawing, so several notifications seen at once are seen as one.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use egui::{Context, Ui};
use katana_cad::{Document, DocumentChange, ListenerHandle};
use katana_math::cpp;

use crate::drive::controls::{SpinRange, custom};
use crate::drive::mailbox::{Act, fill_disabled, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, settle, take};
use crate::host::Host;
use crate::shell::DialogFrame;

/// The parts of the Document notifications heard since the dialog last looked (see the module
/// comment).
pub(crate) struct Followed {
    // First, so it is dropped first: nothing is heard by a half-dropped dialog.
    registration: ListenerHandle,
    parts: Arc<AtomicU32>,
}

impl Followed {
    /// Listens to `document` from now.
    pub(crate) fn new(document: &Document) -> Self {
        let parts = Arc::new(AtomicU32::new(0));
        let heard = Arc::clone(&parts);
        let registration = document.add_change_listener(move |change| {
            heard.fetch_or(change.parts, Ordering::AcqRel);
        });
        Self {
            registration,
            parts,
        }
    }

    /// What was heard since the last call, none of it kept; nothing once the Document has gone,
    /// which is then never read.
    pub(crate) fn take(&self) -> DocumentChange<'static> {
        let parts = self.parts.swap(0, Ordering::AcqRel);
        DocumentChange::of(if self.registration.active() { parts } else { 0 })
    }
}

/// A dialog's part, drawn with the dialog and its host lent to it.
pub(crate) type DialogPart<D> = fn(&mut D, &mut Ui, &mut dyn Host);

/// `dialog` drawn in `frame`: `body`, which scrolls when the dialog does not fit, and `footer`,
/// its button row and what it says under them, each lent the dialog and the host in turn. True
/// when the title bar's close button was clicked.
pub(crate) fn show_dialog<D>(
    frame: &mut DialogFrame,
    ctx: &Context,
    dialog: &mut D,
    host: &mut dyn Host,
    body: DialogPart<D>,
    footer: DialogPart<D>,
) -> bool {
    let mut state: (&mut D, &mut dyn Host) = (dialog, host);
    frame
        .show(
            ctx,
            &mut state,
            |ui, state| body(&mut *state.0, ui, &mut *state.1),
            |ui, state| footer(&mut *state.0, ui, &mut *state.1),
        )
        .closed
}

/// What a table did this frame (`table`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TableShown {
    /// The row a click or a fill made current.
    pub(crate) chosen: Option<usize>,
    /// The command key was held as the row was clicked: it is added to the rows chosen, or taken
    /// from them, rather than put in their place (QAbstractItemView::ExtendedSelection).
    pub(crate) adding: bool,
    /// The row double-clicked.
    pub(crate) double_clicked: Option<usize>,
    /// The check column's box clicked: the row and the state it asks for.
    pub(crate) checked: Option<(usize, bool)>,
}

/// A QTableWidget of whole rows, named `name` (a control the module draws itself, as the plan's
/// section 4.1 allows): `headers` over `rows` of cells, the row `current` and the rows
/// `highlighted` shown chosen, and the column `checks` names, if any, a check box a row with its
/// cell's text beside it. A fill makes the row whose first cell is the text current, as fillField
/// matches column 0 and a click on the row does; it is reported as main.cpp reports a list, the
/// cells joined by " | ".
pub(crate) fn table(
    ui: &mut Ui,
    name: &str,
    headers: &[&str],
    rows: &[Vec<String>],
    current: Option<usize>,
    highlighted: &[bool],
    checks: Option<(usize, &[bool])>,
) -> TableShown {
    let mut shown = TableShown::default();
    let taken = take(ui, name);
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if ui.is_visible() && !ui.is_enabled() {
                    settle(ui, taken, Err(fill_disabled(&taken.posted)));
                } else if let Some(at) = rows
                    .iter()
                    .position(|row| row.first().is_some_and(|cell| cell == text))
                {
                    shown.chosen = Some(at);
                    settle(ui, taken, Ok(()));
                } else {
                    settle(ui, taken, Err(lists_no(name, text)));
                }
            }
            Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
            Act::ScrollTo => {}
        }
    }
    let adding = ui.input(|input| input.modifiers.command);
    let grid = egui::Grid::new(name)
        .striped(true)
        .num_columns(headers.len())
        .show(ui, |ui| {
            for header in headers {
                ui.strong(*header);
            }
            ui.end_row();
            for (at, row) in rows.iter().enumerate() {
                let selected = current == Some(at) || highlighted.get(at).copied().unwrap_or(false);
                for (column, cell) in row.iter().enumerate() {
                    if let Some((check_column, states)) = checks
                        && column == check_column
                    {
                        let mut on = states.get(at).copied().unwrap_or(false);
                        if ui.checkbox(&mut on, cell.as_str()).changed() {
                            shown.checked = Some((at, on));
                        }
                        continue;
                    }
                    let response = ui.selectable_label(selected, cell.as_str());
                    if response.double_clicked() {
                        shown.double_clicked = Some(at);
                    }
                    if response.clicked() {
                        shown.chosen = Some(at);
                        shown.adding = adding;
                    }
                }
                ui.end_row();
            }
        });
    let response = grid.response;
    if let Some(taken) = &taken
        && taken.posted.act == Act::ScrollTo
    {
        response.scroll_to_me(None);
        settle(ui, taken, Ok(()));
    }
    let mut record = ControlRecord::new(name, ControlKind::Rows);
    record.items = rows.iter().map(|row| row.join(" | ")).collect();
    record.value = shown
        .chosen
        .or(current)
        .and_then(|at| rows.get(at))
        .and_then(|row| row.first())
        .cloned()
        .unwrap_or_default();
    custom(ui, &response, record);
    shown
}

/// What a number box of `range` holds when it is set to `value` (QDoubleSpinBox::setValue): the
/// value rounded to the box's decimals as it shows it, then kept to its range. A value the box
/// cannot hold exactly is held as it shows it, so a form that compares what it shows with what
/// is stored sees the difference.
pub(crate) fn spin_value(range: &SpinRange, value: f64) -> f64 {
    cpp::clamp(range.rounded(value), range.minimum, range.maximum)
}

/// A one-line field's text with QLineEdit's cursor and selection, in characters: what the Insert
/// Value menu inserts at. Typed or filled, the field's cursor is at its end, as QLineEdit::setText
/// leaves it; `insert` replaces the selection, or puts the text at the cursor, and leaves the
/// cursor after it (QLineEdit::insert).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LineCursor {
    /// Where the cursor is, in characters; None is the end.
    pub(crate) cursor: Option<usize>,
    /// The selection, a start and an end in characters.
    pub(crate) selection: Option<(usize, usize)>,
}

impl LineCursor {
    /// Typed or filled: the cursor at the end, nothing selected.
    pub(crate) fn reset(&mut self) {
        self.cursor = None;
        self.selection = None;
    }

    /// `inserted` put into `text` as QLineEdit::insert puts it.
    pub(crate) fn insert(&mut self, text: &mut String, inserted: &str) {
        let characters: Vec<char> = text.chars().collect();
        let length = characters.len();
        let (start, end) = match self.selection {
            Some((from, to)) => (from.min(length), to.min(length)),
            None => {
                let at = self.cursor.unwrap_or(length).min(length);
                (at, at)
            }
        };
        let mut made: String = characters[..start].iter().collect();
        made.push_str(inserted);
        made.extend(characters[end..].iter());
        *text = made;
        self.cursor = Some(start + inserted.chars().count());
        self.selection = None;
    }

    /// The text selected in `text`; empty for none.
    pub(crate) fn selected(&self, text: &str) -> String {
        let Some((from, to)) = self.selection else {
            return String::new();
        };
        let characters: Vec<char> = text.chars().collect();
        let to = to.min(characters.len());
        characters[from.min(to)..to].iter().collect()
    }
}
