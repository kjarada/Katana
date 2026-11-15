//! BIM > Road Objects... (docs/road_objects.md, "In the window"): the drawing's road objects in
//! one window - listed, edited, copied, deleted, zoomed to - beside the dialog that places them
//! (bim::road_object_dialog). The port of bim/road_object_manager.
//!
//! NOTHING IS CHANGED HERE. Every edit builds the ROADOBJECT line a person would type - SET, COPY,
//! DELETE - or the ZOOM WINDOW line that frames the chosen objects, and hands it to the window's one
//! executor, so it is echoed, kept in the history and undone as one step, exactly as if typed. What
//! the table and the details show is read from the drawing and from the verb's own INFO, so the
//! window and the command line say the same thing.
//!
//! BY SCOPE. Beside the table stand the same "Apply to" and "Only those that match" controls every
//! dialog on drawing data shows (the shared ScopeFilterWidget, customisation::scope_filter_widget),
//! and three buttons that put what they say into ROADOBJECT LIST, QUANTITIES and DELETE
//! (docs/road_objects.md, "Scope and filter"): Select Matching runs LIST <scope> and chooses the
//! rows of the objects it took, Quantities runs QUANTITIES <scope> and shows the answer in the
//! details, Delete Matching runs DELETE <scope> - one undo step. A scope that takes nothing is
//! said, not refused. The filter's entity-only controls (types, style, text) are to be greyed: a
//! road object has none, and the verb would refuse them by name (`scope_control_greyed`; the shared
//! widget has no way yet to grey one of its controls - reported to the foundation).
//!
//! NON-MODAL, kept by the workbench. It hears the Document through a DocumentWatcher, so a
//! ROADOBJECT typed on the command line, an undo or an open reloads it once, keeping the chosen
//! objects.
//!
//! Object names, which the widget tests and the headless --dialog, --fill and --press steps drive
//! it by:
//!
//! ```text
//! roadObjectManagerDialog   the dialog (the action bimRoadObjects' data)
//! roadObjectTable           an object a row: id, kind, name, placement, from, to, offset, layer,
//!                           quantities, problem
//! roadObjectCount           how many objects, and how many problems
//! roadObjectInfo            ROADOBJECT INFO of the current row (read-only)
//! roadObjectKeys            key=value ... for Apply, and for Copy (cleared once run)
//! roadObjectApply           ROADOBJECT SET <id> <keys>
//! roadObjectCopy            ROADOBJECT COPY <id> [the keys]
//! roadObjectDelete          ROADOBJECT DELETE <ids> - the chosen rows
//! roadObjectManagerByScope  the group of the scope controls and their buttons
//! roadObjectManagerScope    the shared scope and filter widget (ScopeFilterWidget with the
//!                           prefix roadObjectManager: roadObjectManagerScopeDrawing,
//!                           roadObjectManagerScopeLayers, roadObjectManagerFilterProperty ...)
//! roadObjectSelectMatching  ROADOBJECT LIST <scope>: chooses the rows it took
//! roadObjectQuantities      ROADOBJECT QUANTITIES <scope>: shown in the details
//! roadObjectDeleteMatching  ROADOBJECT DELETE <scope>
//! roadObjectZoom            ZOOM WINDOW round the chosen objects
//! roadObjectNew             the placing dialog, on the current row's kind
//! roadObjectManagerStatus   what the last line replied, or why it was refused
//! roadObjectManagerClose
//! ```

use egui::Ui;
use katana_cad::Document;
use katana_cad::road::road_object_scope::plan_extent;
use katana_cad::road::road_object_verbs::run_road_object_verb;
use katana_cad::road::road_objects::{
    PlacementKind, RoadObjectKind, build_road_object, kind_title, layer_of, placement_context,
    placement_word, total_quantities,
};
use katana_geometry::{Box2, Point2};
use katana_math::cpp;

use super::road_dialog_support::{Table, button, cell, field, same_ignoring_case};
use crate::command_word::exact_number;
use crate::customisation::document_watcher::DocumentWatcher;
use crate::customisation::scope_filter_widget::{ScopeChoice, ScopeFilterWidget};
use crate::drive::controls::{TextOptions, custom, label_as, text_with};
use crate::drive::registry::{ControlKind, ControlRecord, simplified};
use crate::format::qt_fixed;
use crate::host::{Host, VerbOutcome};
use crate::shell::dialog::DialogFrame;

/// The table's columns.
pub const ID: usize = 0;
pub const KIND: usize = 1;
pub const NAME: usize = 2;
pub const PLACEMENT: usize = 3;
pub const FROM: usize = 4;
pub const TO: usize = 5;
pub const OFFSET: usize = 6;
pub const LAYER: usize = 7;
pub const QUANTITIES: usize = 8;
pub const PROBLEM: usize = 9;

/// Why the scope's entity-only controls are greyed: a road object has none of them.
pub const NOT_AN_ENTITY: &str = "A road object is not an entity: it has no type, style or text to \
                                 match. Its kind is a property: key kind, value barrier";

/// Whether the scope's control `name` is greyed in the manager: the type boxes, the style and the
/// text, which a road object has none of (the C++'s loop over the scope widget's children).
pub fn scope_control_greyed(name: &str) -> bool {
    name.starts_with("roadObjectManagerType")
        || name == "roadObjectManagerFilterStyle"
        || name == "roadObjectManagerFilterText"
}

/// The tip the manager gives the scope's control `name`; None where it keeps the widget's own.
pub fn scope_control_tip(name: &str) -> Option<&'static str> {
    if scope_control_greyed(name) {
        return Some(NOT_AN_ENTITY);
    }
    match name {
        "roadObjectManagerScopeSelection" => Some(
            "The objects placed along the selected lines; one placed along an alignment or at \
             points has no line to select",
        ),
        "roadObjectManagerScopeView" => Some(
            "The objects on a layer the view shows, and (with only what is on screen) whose plan \
             extent meets the view's area",
        ),
        "roadObjectManagerFilterProperty" => Some(
            "A key ROADOBJECT SET writes: kind, id, type, offset, station, layer ... - exactly, in \
             any case",
        ),
        _ => None,
    }
}

/// A real as the table shows it: to the millimetre, which is what a station or a quantity is set
/// out to.
fn shown(value: f64) -> String {
    qt_fixed(value, 3)
}

/// The plan box round the solids of the objects `ids` names, widened by a tenth each way and to
/// 10 m at least, so a sign is framed with what is round it; None when none of them is made.
pub fn road_objects_frame(document: &Document, ids: &[String]) -> Option<Box2> {
    let context = placement_context(document);
    let objects = document.road_objects();
    let mut frame = Box2::default();
    let mut any = false;
    for id in ids {
        let Some(object) = objects.find(id) else {
            continue;
        };
        // The same extent the scope's AREA and VIEW meet (road_object_scope).
        if let Some(extent) = plan_extent(&context, object) {
            frame.expand_box(extent);
            any = true;
        }
    }
    if !any {
        return None;
    }
    // A tenth more each way, and 10 m across at the least, so what is framed is seen with what
    // is round it.
    let centre = (frame.min + frame.max) / 2.0;
    let half = cpp::max3(
        (frame.max.x - frame.min.x) * 0.6,
        (frame.max.y - frame.min.y) * 0.6,
        5.0,
    );
    Some(Box2::new(
        centre - Point2::new(half, half),
        centre + Point2::new(half, half),
    ))
}

struct Body {
    watcher: DocumentWatcher,
    executor: bool,
    table: Table,
    count: String,
    info: String,
    keys: String,
    scope: ScopeFilterWidget,
    status: String,
    /// The placing dialog asked for by New, on this kind, for the workbench to open.
    place: Option<RoadObjectKind>,
    close: bool,
}

impl Body {
    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    /// The chosen rows' ids, in table order.
    fn selected_ids(&self) -> Vec<String> {
        self.table
            .selected_rows()
            .into_iter()
            .map(|row| self.table.text(row, ID))
            .collect()
    }

    /// The selection's handler (itemSelectionChanged): the details of the one object chosen.
    /// The buttons' enabling follows the selection where they are drawn.
    fn show_info(&mut self, document: &mut Document) {
        let ids = self.selected_ids();
        if ids.len() != 1 || !self.alive() {
            self.info.clear();
            return;
        }
        // The verb's own INFO - it changes nothing, so it is read here and not run through the
        // executor, which would keep it in the history.
        let tokens = [
            String::from("ROADOBJECT"),
            String::from("INFO"),
            ids[0].clone(),
        ];
        self.info = match run_road_object_verb(document, &tokens, None) {
            Ok(reply) => reply,
            Err(error) => error.describe(),
        };
    }

    fn select_object(&mut self, document: &mut Document, id: &str) -> bool {
        let Some(row) = (0..self.table.row_count())
            .find(|&row| same_ignoring_case(&self.table.text(row, ID), id))
        else {
            return false;
        };
        let before = self.table.selected_rows();
        self.table.clear_selection();
        self.table.select_row(row);
        if self.table.selected_rows() != before {
            self.show_info(document);
        }
        true
    }

    fn reload(&mut self, document: &mut Document) {
        let kept = self.selected_ids();
        let had = self.table.clear();
        if !self.alive() {
            self.count = String::from("No drawing.");
            self.scope.reload_views();
            if had {
                self.show_info(document);
            }
            return;
        }
        // The layers and the views to choose from follow the drawing and the window.
        self.scope.reload(document.model());
        let mut problems = 0usize;
        // A set that cannot be read is empty to road_objects(): without this the manager would
        // say "0 road objects, 0 problems" over a record that is there and kept (a newer Katana's,
        // or damaged).
        let unreadable = match document.road_objects_status() {
            Ok(()) => String::new(),
            Err(error) => {
                problems += 1;
                error.describe()
            }
        };
        let objects = document.road_objects();
        {
            let context = placement_context(document);
            for object in &objects.objects {
                let built = build_road_object(&context, object);
                problems += built.problems.len();
                let placement = &object.placement;
                let mut quantities = String::new();
                for quantity in total_quantities(&built.objects) {
                    quantities.push_str(&format!(
                        "{} {} {}  ",
                        quantity.name,
                        shown(quantity.value),
                        quantity.unit
                    ));
                }
                let along = placement.kind == PlacementKind::Alignment;
                let where_placed = if along {
                    placement.alignment.clone()
                } else {
                    format!(
                        "{} ({} points)",
                        placement_word(placement.kind),
                        placement.vertices.len()
                    )
                };
                let to = match placement.to {
                    Some(to) if along => shown(to),
                    _ => String::new(),
                };
                self.table.push_row(vec![
                    cell(object.id.as_str()),
                    cell(kind_title(object.kind)),
                    cell(object.name.as_str()),
                    cell(where_placed),
                    cell(if along {
                        shown(placement.station)
                    } else {
                        String::new()
                    }),
                    cell(to),
                    cell(if along {
                        shown(placement.offset)
                    } else {
                        String::new()
                    }),
                    cell(layer_of(object)),
                    cell(quantities.trim()),
                    cell(
                        built
                            .problems
                            .first()
                            .map(|problem| problem.message.clone())
                            .unwrap_or_default(),
                    ),
                ]);
            }
        }
        self.count = format!(
            "{} road objects, {problems} problems",
            objects.objects.len()
        );
        if !unreadable.is_empty() {
            self.count.push_str(&format!(
                " - the drawing's road objects cannot be read, and are kept as they are: \
                 {unreadable}"
            ));
        }
        // What was chosen stays chosen, all of it: choosing a row alone would keep only the last.
        let mut chose = false;
        for row in 0..self.table.row_count() {
            if kept.contains(&self.table.text(row, ID)) && self.table.select_also(row) {
                chose = true;
            }
        }
        if had || chose {
            self.show_info(document);
        }
    }

    /// Runs one line through the window's executor and says what it replied; the outcome when the
    /// line was accepted.
    fn execute(&mut self, host: &mut dyn Host, line: &str) -> Option<VerbOutcome> {
        if !self.executor {
            self.status = String::from("There is no command line to run it on here.");
            return None;
        }
        let outcome = host.run(line);
        self.status = if outcome.ok {
            outcome.reply.clone()
        } else {
            format!("Refused: {}", outcome.error)
        };
        outcome.ok.then_some(outcome)
    }

    fn run(&mut self, host: &mut dyn Host, line: &str) -> bool {
        self.execute(host, line).is_some()
    }

    /// The scope the controls say, as the words a line takes; the reason when they say none (no
    /// layer ticked, a colour that does not read ...).
    fn scope_words(&mut self) -> Option<String> {
        match self.scope.verb_words() {
            Ok(words) => Some(words),
            Err(error) => {
                self.status = format!("Refused: {}", error.message);
                None
            }
        }
    }

    // ---- the buttons ----------------------------------------------------------------------------

    fn press_apply(&mut self, host: &mut dyn Host) {
        let ids = self.selected_ids();
        let keys = simplified(&self.keys);
        if ids.len() != 1 || keys.is_empty() {
            self.status = String::from("Choose one object and give key=value to set.");
            return;
        }
        // The keys are spent once they are set; a refused line keeps them to be put right.
        if self.run(host, &format!("ROADOBJECT SET {} {keys}", ids[0])) {
            self.keys.clear();
        }
    }

    fn press_copy(&mut self, host: &mut dyn Host) {
        let ids = self.selected_ids();
        if ids.len() == 1 {
            // The keys field goes with the copy: station=, dx= and dy= are what move it, and no
            // other control says them.
            let keys = simplified(&self.keys);
            let line = if keys.is_empty() {
                format!("ROADOBJECT COPY {}", ids[0])
            } else {
                format!("ROADOBJECT COPY {} {keys}", ids[0])
            };
            if self.run(host, &line) {
                self.keys.clear();
            }
        }
    }

    fn press_delete(&mut self, host: &mut dyn Host) {
        let ids = self.selected_ids();
        if !ids.is_empty() {
            self.run(host, &format!("ROADOBJECT DELETE {}", ids.join(" ")));
        }
    }

    fn press_zoom(&mut self, host: &mut dyn Host) {
        if !self.alive() {
            return;
        }
        let Some(frame) = road_objects_frame(host.document(), &self.selected_ids()) else {
            self.status = String::from(
                "Nothing of those objects is made, so there is nothing to frame: ROADOBJECT \
                 PROBLEMS says why.",
            );
            return;
        };
        let line = format!(
            "ZOOM WINDOW {},{},{},{}",
            exact_number(frame.min.x),
            exact_number(frame.min.y),
            exact_number(frame.max.x),
            exact_number(frame.max.y)
        );
        self.run(host, &line);
    }

    fn press_select_matching(&mut self, host: &mut dyn Host) {
        let Some(words) = self.scope_words() else {
            return;
        };
        let Some(outcome) = self.execute(host, &format!("ROADOBJECT LIST {words}")) else {
            return;
        };
        // The records say which: `object id=...`, after the scope's own.
        let mut ids: Vec<String> = Vec::new();
        let mut record = String::new();
        for line in outcome.reply.split('\n') {
            if let Some(rest) = line.strip_prefix("object id=") {
                ids.push(String::from(rest.split(' ').next().unwrap_or("")));
            } else if line.starts_with("scope=") {
                record = String::from(line.trim());
            }
        }
        let before = self.table.selected_rows();
        self.table.clear_selection();
        for row in 0..self.table.row_count() {
            if ids.contains(&self.table.text(row, ID)) {
                self.table.select_also(row);
            }
        }
        if self.table.selected_rows() != before {
            self.show_info(host.document_mut());
        }
        self.status = format!(
            "{} chosen - {}",
            ids.len(),
            if record.is_empty() {
                "no scope record"
            } else {
                record.as_str()
            }
        );
    }

    fn press_quantities(&mut self, host: &mut dyn Host) {
        let Some(words) = self.scope_words() else {
            return;
        };
        if let Some(outcome) = self.execute(host, &format!("ROADOBJECT QUANTITIES {words}")) {
            self.status = String::from(outcome.reply.split('\n').next().unwrap_or(""));
            self.info = outcome.reply;
        }
    }

    fn press_delete_matching(&mut self, host: &mut dyn Host) {
        if let Some(words) = self.scope_words() {
            self.run(host, &format!("ROADOBJECT DELETE {words}"));
        }
    }

    fn press_new(&mut self, host: &mut dyn Host) {
        let mut kind = RoadObjectKind::Barrier;
        let ids = self.selected_ids();
        if let Some(first) = ids.first()
            && self.alive()
        {
            let objects = host.document().road_objects();
            if let Some(object) = objects.find(first) {
                kind = object.kind;
            }
        }
        self.place = Some(kind);
    }

    // ---- drawing --------------------------------------------------------------------------------

    fn body_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let chosen = self.table.selected_rows().len();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                crate::drive::controls::label(ui, "roadObjectCount", &self.count);
                if self.table.ui(ui).selection_changed {
                    self.show_info(host.document_mut());
                }
                text_with(
                    ui,
                    "roadObjectInfo",
                    &mut self.info,
                    TextOptions {
                        read_only: true,
                        rows: 6,
                        monospace: true,
                    },
                );
                ui.horizontal(|ui| {
                    field(
                        ui,
                        "roadObjectKeys",
                        &mut self.keys,
                        "key=value ...   e.g. offset=4.5 height=1.0 (ROADOBJECT KINDS)",
                        "Apply sets these on the chosen object; Copy makes a copy with them \
                         (station=, dx= and dy= move the copy, and any other key changes it)",
                    );
                    ui.add_enabled_ui(chosen == 1, |ui| {
                        if button(ui, "roadObjectApply", "&Apply", "").clicked() {
                            self.press_apply(host);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if button(ui, "roadObjectNew", "&New...", "").clicked() {
                        self.press_new(host);
                    }
                    ui.add_enabled_ui(chosen == 1, |ui| {
                        if button(ui, "roadObjectCopy", "C&opy", "").clicked() {
                            self.press_copy(host);
                        }
                    });
                    ui.add_enabled_ui(chosen > 0, |ui| {
                        if button(ui, "roadObjectDelete", "&Delete", "").clicked() {
                            self.press_delete(host);
                        }
                        if button(ui, "roadObjectZoom", "&Zoom To", "").clicked() {
                            self.press_zoom(host);
                        }
                    });
                });
                label_as(
                    ui,
                    "roadObjectManagerStatus",
                    &self.status,
                    egui::Label::new(self.status.as_str()).wrap(),
                );
            });
            // The shared scope and filter. A road object is a definition on a layer with a plan
            // extent, so the whole drawing is the natural start; the views are the window's.
            let by_scope = ui.group(|ui| {
                ui.strong("Road objects by scope");
                self.scope.ui(ui);
                ui.horizontal(|ui| {
                    if button(
                        ui,
                        "roadObjectSelectMatching",
                        "&Select Matching",
                        "Choose the rows of the objects the scope and the filter take \
                         (ROADOBJECT LIST <scope>)",
                    )
                    .clicked()
                    {
                        self.press_select_matching(host);
                    }
                    if button(
                        ui,
                        "roadObjectQuantities",
                        "&Quantities",
                        "The quantities of the objects the scope and the filter take, in the \
                         details (ROADOBJECT QUANTITIES <scope>)",
                    )
                    .clicked()
                    {
                        self.press_quantities(host);
                    }
                    if button(
                        ui,
                        "roadObjectDeleteMatching",
                        "Delete Matchin&g",
                        "Delete the objects the scope and the filter take, as one undo step \
                         (ROADOBJECT DELETE <scope>)",
                    )
                    .clicked()
                    {
                        self.press_delete_matching(host);
                    }
                });
            });
            custom(
                ui,
                &by_scope.response,
                ControlRecord::new("roadObjectManagerByScope", ControlKind::Other),
            );
        });
    }

    fn footer_ui(&mut self, ui: &mut Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, "roadObjectManagerClose", "&Close", "").clicked() {
                self.close = true;
            }
        });
    }
}

/// The road object manager (see the module comment).
pub struct RoadObjectManagerDialog {
    frame: DialogFrame,
    open: bool,
    body: Body,
}

impl RoadObjectManagerDialog {
    /// The manager over `document`, shown. (A Document is lent mutably: a chosen object's details
    /// are the verb's own INFO, which runs on it and changes nothing.)
    pub fn new(document: &mut Document) -> Self {
        let mut scope = ScopeFilterWidget::new("roadObjectManager");
        scope.set_choice(ScopeChoice::Drawing);
        for name in scope.control_names() {
            if scope_control_greyed(&name) {
                scope.grey(&name, NOT_AN_ENTITY);
            }
        }
        let mut body = Body {
            watcher: DocumentWatcher::new(document),
            executor: true,
            table: Table::new(
                "roadObjectTable",
                &[
                    "Id",
                    "Kind",
                    "Name",
                    "Placement",
                    "From",
                    "To",
                    "Offset",
                    "Layer",
                    "Quantities",
                    "Problem",
                ],
            )
            .extended()
            .max_height(300.0),
            count: String::new(),
            info: String::new(),
            keys: String::new(),
            scope,
            status: String::new(),
            place: None,
            close: false,
        };
        body.reload(document);
        Self {
            frame: DialogFrame::new("roadObjectManagerDialog", "Road Objects"),
            open: true,
            body,
        }
    }

    /// A manager with no executor (the C++'s empty CommandRunner): it says it cannot run.
    pub fn without_executor(mut self) -> Self {
        self.body.executor = false;
        self
    }

    /// The workspace's open views, for the scope's "What a view shows".
    pub fn set_views(&mut self, views: crate::customisation::scope_filter_widget::ViewsFn) {
        self.body.scope.views = Some(views);
    }

    /// The scope and filter, as Qt's widget tree gave a test its controls.
    pub fn scope_mut(&mut self) -> &mut ScopeFilterWidget {
        &mut self.body.scope
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(&mut self) {
        self.open = true;
    }

    pub fn hide(&mut self) {
        self.open = false;
    }

    /// Chooses the row of `id` alone (ignoring case); false when there is none.
    pub fn select_object(&mut self, document: &mut Document, id: &str) -> bool {
        self.body.select_object(document, id)
    }

    /// The chosen rows' ids, in table order.
    pub fn selected_ids(&self) -> Vec<String> {
        self.body.selected_ids()
    }

    /// The table again from the drawing, keeping what was chosen.
    pub fn reload(&mut self, document: &mut Document) {
        self.body.reload(document);
    }

    /// The table of objects (roadObjectTable).
    pub fn table(&self) -> &Table {
        &self.body.table
    }

    /// The placing dialog New asked for, on a kind, once: the workbench opens it (the C++
    /// context's `place`). None when New was not pressed since.
    pub fn take_place_request(&mut self) -> Option<RoadObjectKind> {
        self.body.place.take()
    }

    /// Draws the manager for this frame, when it is shown; then the watcher's delivery, as the
    /// C++'s event loop delivered it after a click (corridor_dialog's `ui` says why).
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if self.open {
            let ctx = ui.ctx().clone();
            let mut lent = (&mut self.body, &mut *host);
            let shown = self.frame.show(
                &ctx,
                &mut lent,
                |ui, lent| lent.0.body_ui(ui, &mut *lent.1),
                |ui, lent| lent.0.footer_ui(ui),
            );
            if shown.closed || std::mem::take(&mut self.body.close) {
                self.open = false;
            }
        }
        if let Some(changes) = self.body.watcher.deliver(host.document())
            && changes.model
        {
            self.body.reload(host.document_mut());
            ui.ctx().request_repaint();
        }
    }
}
