//! The sheet editor's list of sheets (docs/plotting.md, "Editing on the canvas"): a thumbnail and
//! the number and name of each sheet, dragged up and down to reorder the set. The port of
//! plotting/sheet_list_widget.
//!
//! A drop does not move the row itself. It asks for the move (take_move_request), the editor makes
//! it as one undoable step (cad's move_sheet), and the list is drawn again from the document - so
//! the list never shows an order the set does not have, and an undo puts both back.
//!
//! The C++ queued its request so that it ran after the drop had returned (the move rebuilds the
//! list, and a drop must not have its list cleared under it). Here the request is kept and the
//! editor takes it after the frame's drawing, which is the same order of things.

use egui::{Rect, Response, Sense, Ui};

use crate::drive::controls;
use crate::drive::mailbox::{Act, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, settle, take};

/// One row of the list: what it says, and its sheet's picture when there is one.
#[derive(Clone, Debug)]
pub struct SheetListRow {
    /// The sheet's number and name.
    pub text: String,
    /// The thumbnail, as a texture the editor keeps, and its size in points.
    pub picture: Option<egui::load::SizedTexture>,
}

/// The list of sheets (see the module comment). `name` is its object name.
#[derive(Clone, Debug)]
pub struct SheetListWidget {
    name: String,
    current: Option<usize>,
    /// Where each row was drawn last, in points: what a drop is placed by.
    rows: Vec<Rect>,
    /// The sheet at `from` dropped to end up at `to`, until the editor takes it.
    move_request: Option<(usize, usize)>,
}

impl SheetListWidget {
    pub fn new(name: &str) -> Self {
        Self {
            name: String::from(name),
            current: None,
            rows: Vec::new(),
            move_request: None,
        }
    }

    /// QListWidget::count, as the list was last drawn.
    pub fn count(&self) -> usize {
        self.rows.len()
    }

    /// QListWidget::currentRow.
    pub fn current_row(&self) -> Option<usize> {
        self.current
    }

    /// QListWidget::setCurrentRow.
    pub fn set_current_row(&mut self, row: Option<usize>) {
        self.current = row;
    }

    /// Where row `row` was drawn last (QListWidget::visualItemRect).
    pub fn row_rect(&self, row: usize) -> Option<Rect> {
        self.rows.get(row).copied()
    }

    /// The row a drop at height `y` (in points, as the rows were drawn) goes before: the row under
    /// it, or the one after when it is on the lower half of that row; in the spacing between two
    /// rows, the lower of them; count() below the last.
    pub fn insertion_row_at(&self, y: f32) -> usize {
        // Before the first row whose middle is below the point. Not the row under the point: in
        // the spacing between two rows, or beside a short row, there is none, and a drop there
        // must still go between those rows, not to the end.
        self.rows
            .iter()
            .position(|row| y <= row.center().y)
            .unwrap_or(self.rows.len())
    }

    /// What a drop at height `y` does, without the drag: asks for the current sheet to go where
    /// the drop falls (take_move_request, after the caller has returned). False when it would stay
    /// where it is.
    pub fn drop_at(&mut self, y: f32) -> bool {
        let Some(from) = self.current else {
            return false;
        };
        let before = self.insertion_row_at(y);
        // Before a row below it is after one fewer once it has left its place.
        let to = if before > from { before - 1 } else { before };
        if to == from {
            return false;
        }
        self.move_request = Some((from, to));
        true
    }

    /// The move a drop asked for - the sheet at `from` to end up at `to` - once; the editor makes
    /// it after the drop has finished, never from inside it.
    pub fn take_move_request(&mut self) -> Option<(usize, usize)> {
        self.move_request.take()
    }

    /// Draws the list: a row a sheet, the current one chosen. A click chooses a row (the response
    /// is changed); a row dragged and let go asks for the move.
    pub fn show(&mut self, ui: &mut Ui, rows: &[SheetListRow]) -> Response {
        let taken = take(ui, &self.name);
        let mut changed = false;
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => match rows.iter().position(|row| row.text == *text) {
                    Some(at) => {
                        changed = self.current != Some(at);
                        self.current = Some(at);
                        settle(ui, taken, Ok(()));
                    }
                    None => settle(ui, taken, Err(lists_no(&self.name, text))),
                },
                Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => {}
            }
        }
        let mut drawn = Vec::with_capacity(rows.len());
        let mut dropped_at = None;
        let mut response = ui
            .push_id(self.name.as_str(), |ui| {
                for (at, row) in rows.iter().enumerate() {
                    let shown = ui
                        .horizontal(|ui| {
                            if let Some(picture) = row.picture {
                                ui.image(picture);
                            }
                            ui.selectable_label(self.current == Some(at), row.text.as_str())
                        })
                        .response;
                    let held = ui.interact(
                        shown.rect,
                        ui.id().with(("sheet", at)),
                        Sense::click_and_drag(),
                    );
                    if held.clicked() || held.drag_started() {
                        changed |= self.current != Some(at);
                        self.current = Some(at);
                    }
                    if held.drag_stopped() {
                        dropped_at = held.interact_pointer_pos().map(|pointer| pointer.y);
                    }
                    drawn.push(shown.rect);
                }
            })
            .response;
        self.rows = drawn;
        if let Some(y) = dropped_at {
            let _ = self.drop_at(y);
        }
        if changed {
            response.mark_changed();
        }
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            response.scroll_to_me(None);
            settle(ui, taken, Ok(()));
        }
        let mut record = ControlRecord::new(&self.name, ControlKind::Rows);
        record.items = rows.iter().map(|row| row.text.clone()).collect();
        record.value = self
            .current
            .and_then(|at| rows.get(at))
            .map(|row| row.text.clone())
            .unwrap_or_default();
        controls::custom(ui, &response, record);
        response
    }
}
