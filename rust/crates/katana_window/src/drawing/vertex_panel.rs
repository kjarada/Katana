//! The Vertices panel (docs/drawing.md, "The Vertices panel"): the selected polyline's vertices as
//! a live table - index, easting, northing, height, bulge, and each segment's bearing (D°MM'SS")
//! and distance - edited in place, every edit one undoable step, following the selection. The port
//! of drawing/vertex_panel.
//!
//! A thin front end over katana_cad's drawing/vertex_table.rs, which says what the rows are and
//! what typing into a cell does, and drawing/vertex_editing.rs, which makes the edit one command.
//! It shows the first polyline in the selection; with none it says so.
//!
//! The Rust shape. The C++ panel reads the document again inside every notification; here a
//! notification is counted, and the panel reads the document again at its next look (`follow`,
//! which every call lent the document makes first), as every dialog of the port does. The table is
//! the control "vertexTable": a driver's fill selects the row whose first cell - the vertex's
//! index - is the text given, as a fill selects a row of any list.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use egui::Ui;
use katana_cad::drawing::drafting::AngleConvention;
use katana_cad::drawing::vertex_editing::{edit_polyline, is_polyline_entity, read_polyline};
use katana_cad::drawing::vertex_table::{
    VERTEX_COLUMN_COUNT, VertexColumn, cell_text, edit_vertex_cell, vertex_rows,
};
use katana_cad::{Document, ListenerHandle};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Status};
use katana_entity::EntityId;
use katana_entity::entity::to_string;
use katana_geometry::polyline_vertices::{delete_vertex, insert_vertex, minimum_vertices};
use katana_geometry::{CurvePolyline2, Point2};

use crate::drive::controls;
use crate::drive::mailbox::{Act, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, settle, take};

/// The table's columns in their order: a column's place is its number.
const COLUMNS: [VertexColumn; VERTEX_COLUMN_COUNT as usize] = [
    VertexColumn::Index,
    VertexColumn::Easting,
    VertexColumn::Northing,
    VertexColumn::Height,
    VertexColumn::Bulge,
    VertexColumn::Bearing,
    VertexColumn::Distance,
];

const TABLE_TIP: &str = "Vertices\nType into a cell to edit it: easting, northing, height (empty \
                         for none), bulge (0 straight), or the bearing and distance of the segment \
                         from this vertex, which move the next one. Each edit is one undo step.";

/// See the module comment.
pub struct VertexPanel {
    // First, so it is dropped first: nothing counts into a half-dropped panel.
    registration: ListenerHandle,
    /// Counted by the document's listener: one a notification.
    notified: Arc<AtomicU64>,
    /// The count the panel last read the document at; None before its first read.
    seen: Option<u64>,
    shown: Option<EntityId>,
    title: String,
    /// The table as the drawing has it, a row of cells a vertex.
    cells: Vec<Vec<String>>,
    /// The table as its fields hold it: what is being typed into a cell and not yet entered.
    typed: Vec<Vec<String>>,
    current: Option<usize>,
    insert_enabled: bool,
    delete_enabled: bool,
}

impl VertexPanel {
    /// The panel over `document`, read at once.
    pub fn new(document: &Document) -> Self {
        let notified = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&notified);
        let registration = document.add_listener(move || {
            counter.fetch_add(1, Ordering::AcqRel);
        });
        let mut panel = Self {
            registration,
            notified,
            seen: None,
            shown: None,
            title: String::new(),
            cells: Vec::new(),
            typed: Vec::new(),
            current: None,
            insert_enabled: false,
            delete_enabled: false,
        };
        panel.follow(document);
        panel
    }

    /// Reads the document again when it has notified since the last read.
    pub fn follow(&mut self, document: &Document) {
        let now = self.notified.load(Ordering::Acquire);
        if self.seen != Some(now) || !self.registration.active() {
            self.seen = Some(now);
            self.refresh(document);
        }
    }

    /// The polyline shown, if any.
    pub fn shown(&self) -> Option<EntityId> {
        self.shown
    }

    /// What the panel says above its table: the polyline shown, or why none is, or the last edit's
    /// refusal.
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn row_count(&self) -> usize {
        self.cells.len()
    }

    /// The text of a cell as the drawing has it; "" outside the table.
    pub fn cell(&self, row: usize, column: usize) -> &str {
        self.cells
            .get(row)
            .and_then(|cells| cells.get(column))
            .map_or("", String::as_str)
    }

    /// The table's current row (QTableWidget::currentRow).
    pub fn current_row(&self) -> Option<usize> {
        self.current
    }

    pub fn set_current_row(&mut self, row: Option<usize>) {
        self.current = row.filter(|&row| row < self.cells.len());
    }

    /// Reads the document again: the selection and the polyline's vertices.
    pub fn refresh(&mut self, document: &Document) {
        self.shown = None;
        let polyline = document
            .selection()
            .ids()
            .into_iter()
            .filter_map(|id| document.model().entities.find(id))
            .find(|entity| is_polyline_entity(entity));
        let Some((polyline, shape)) =
            polyline.and_then(|entity| read_polyline(entity).map(|shape| (entity, shape)))
        else {
            self.title = String::from("Select a polyline to see its vertices.");
            self.cells.clear();
            self.typed.clear();
            self.current = None;
            self.insert_enabled = false;
            self.delete_enabled = false;
            return;
        };
        self.shown = Some(polyline.id);
        self.title = format!(
            "Polyline {}: {} vertices, {}{}",
            polyline.id,
            shape.vertices.len(),
            if shape.closed { "closed" } else { "open" },
            if shape.has_arcs() { ", with arcs" } else { "" }
        );
        self.cells = vertex_rows(&shape)
            .iter()
            .map(|row| {
                COLUMNS
                    .iter()
                    .map(|&column| cell_text(row, column))
                    .collect()
            })
            .collect();
        self.typed.clone_from(&self.cells);
        self.current = self.current.filter(|&row| row < self.cells.len());
        self.insert_enabled = shape.segment_count() > 0;
        self.delete_enabled = shape.vertices.len() > minimum_vertices(&shape);
    }

    /// Types `text` into a cell and enters it, as the table's editor does - for the tests and the
    /// headless driver. The edit alone: `type_into_cell` also says a refusal and puts the cell
    /// back.
    pub fn edit_cell(
        &mut self,
        document: &mut Document,
        row: usize,
        column: usize,
        text: &str,
    ) -> Status {
        self.follow(document);
        let Some(shown) = self.shown else {
            return Err(make_error(
                ErrorCode::InvalidState,
                "no polyline is shown",
                "",
            ));
        };
        let Some(&which) = COLUMNS.get(column) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("there is no column {column}"),
                "",
            ));
        };
        let typed = String::from(text);
        let angles = document.drafting().angles;
        // The panel shows bearings, so a typed bearing is one unless it is a quadrant form (which
        // parse_direction reads either way).
        let convention = if which == VertexColumn::Bearing {
            AngleConvention::Bearing
        } else {
            angles
        };
        let done = document.execute(edit_polyline(
            shown,
            "VERTEX_SET",
            move |polyline: &CurvePolyline2| {
                edit_vertex_cell(polyline, row, which, &typed, convention)
            },
        ));
        self.follow(document);
        done
    }

    /// A cell's text changed by a person: the edit, and when it is refused the refusal said above
    /// the table and the cell put back as the drawing has it.
    pub fn type_into_cell(
        &mut self,
        document: &mut Document,
        row: usize,
        column: usize,
        text: &str,
    ) {
        if let Err(error) = self.edit_cell(document, row, column, text) {
            // Put the cell back as the drawing has it, and keep what was said.
            self.refresh(document);
            self.title = error.message;
        }
    }

    /// The Insert After button: a vertex halfway along the segment after the current row.
    pub fn insert_after_current(&mut self, document: &mut Document) {
        self.follow(document);
        let Some(shown) = self.shown else {
            return;
        };
        let row = self.current.unwrap_or(0);
        let _ = document.execute(edit_polyline(
            shown,
            "VERTEX_INSERT",
            move |polyline: &CurvePolyline2| {
                let Some(last) = polyline.segment_count().checked_sub(1) else {
                    // No segment to split: refused by the edit itself, naming the segment.
                    return insert_vertex(polyline, row, Point2::default(), None);
                };
                let segment = row.min(last);
                let middle = polyline.segment(segment).point_at(0.5);
                insert_vertex(polyline, segment, middle, None)
            },
        ));
        self.follow(document);
    }

    /// The Delete button: the current row's vertex.
    pub fn delete_current(&mut self, document: &mut Document) {
        self.follow(document);
        let (Some(shown), Some(row)) = (self.shown, self.current) else {
            return;
        };
        let done = document.execute(edit_polyline(
            shown,
            "VERTEX_DELETE",
            move |polyline: &CurvePolyline2| delete_vertex(polyline, row),
        ));
        self.follow(document);
        if let Err(error) = done {
            self.title = error.message;
        }
    }

    /// The panel ("vertexPanel"): its title, the table and the two buttons.
    pub fn ui(&mut self, ui: &mut Ui, document: &mut Document) {
        self.follow(document);
        controls::label(ui, "vertexPanelTitle", &self.title.clone());
        let entered = self.table_ui(ui);
        let mut insert = false;
        let mut delete = false;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(self.insert_enabled, |ui| {
                insert = controls::button(ui, "vertexInsertButton", "Insert After")
                    .on_hover_text("Adds a vertex halfway along the segment after the current row.")
                    .clicked();
            });
            ui.add_enabled_ui(self.delete_enabled, |ui| {
                delete = controls::button(ui, "vertexDeleteButton", "Delete")
                    .on_hover_text("Removes the current row's vertex.")
                    .clicked();
            });
        });
        if let Some((row, column, text)) = entered {
            self.type_into_cell(document, row, column, &text);
        }
        if insert {
            self.insert_after_current(document);
        }
        if delete {
            self.delete_current(document);
        }
    }

    /// The table ("vertexTable"); answers the cell a person has just entered a new text into.
    fn table_ui(&mut self, ui: &mut Ui) -> Option<(usize, usize, String)> {
        let taken = take(ui, "vertexTable");
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => {
                    if controls::accept_fill(ui, taken) {
                        match self
                            .cells
                            .iter()
                            .position(|row| row.first().is_some_and(|cell| cell == text))
                        {
                            Some(at) => {
                                self.current = Some(at);
                                settle(ui, taken, Ok(()));
                            }
                            None => settle(ui, taken, Err(lists_no("vertexTable", text))),
                        }
                    }
                }
                Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => {}
            }
        }
        let mut entered = None;
        let mut current = self.current;
        let cells = &self.cells;
        let typed = &mut self.typed;
        let shown = egui::ScrollArea::both()
            .id_salt("vertexTableScroll")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("vertexTableGrid")
                    .striped(true)
                    .show(ui, |ui| {
                        for column in COLUMNS {
                            ui.strong(to_string(column));
                        }
                        ui.end_row();
                        for (row, texts) in typed.iter_mut().enumerate() {
                            for (column, text) in texts.iter_mut().enumerate() {
                                if column == 0 {
                                    // The index is the row's handle: not edited, it selects.
                                    if ui
                                        .selectable_label(current == Some(row), text.as_str())
                                        .clicked()
                                    {
                                        current = Some(row);
                                    }
                                    continue;
                                }
                                let field = ui.add(
                                    egui::TextEdit::singleline(text)
                                        .id_salt(("vertexCell", row, column))
                                        .horizontal_align(egui::Align::RIGHT)
                                        .desired_width(84.0),
                                );
                                if field.gained_focus() {
                                    current = Some(row);
                                }
                                let was = cells
                                    .get(row)
                                    .and_then(|cells| cells.get(column))
                                    .map_or("", String::as_str);
                                if field.lost_focus() && text.as_str() != was {
                                    entered = Some((row, column, text.clone()));
                                }
                            }
                            ui.end_row();
                        }
                    })
                    .response
            });
        self.current = current;
        let response = shown.inner.on_hover_text(TABLE_TIP);
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            response.scroll_to_me(None);
            settle(ui, taken, Ok(()));
        }
        let mut record = ControlRecord::new("vertexTable", ControlKind::Rows);
        record.items = self.cells.iter().map(|row| row.join(" | ")).collect();
        record.value = self
            .current
            .and_then(|at| self.cells.get(at))
            .and_then(|row| row.first())
            .cloned()
            .unwrap_or_default();
        controls::custom(ui, &response, record);
        entered
    }
}
