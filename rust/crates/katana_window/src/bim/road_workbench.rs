//! The BIM menu's road model (docs/road_design.md, "In the window"): road templates, corridors,
//! their quantities, and the road model shown in the views. The owner's request of 2026-10-15:
//! horizontal BIM - roads, not buildings. The port of bim/road_workbench.
//!
//! The menu, in sections:
//!
//! ```text
//! Templates   Road Template Manager...          (bimRoadTemplates)
//! Corridors   Corridors...                      (bimCorridors)
//! Model       Show Road Model                   (bimShowRoadModel, checkable)
//! Quantities  Corridor Quantities...            (bimCorridorQuantities, Ctrl+Shift+Q)
//! ```
//!
//! Show Road Model has a line, as View > Grid has GRID: ROADMODEL SHOW [ON|OFF], ON or OFF, or
//! alone the other of the two, replying roadmodel=on|off. It is the window's own (the session's
//! display state, like the grid's), taken here by `run_line` before the interpreter, so an agent
//! that drives the window need not switch the road layers off; katana_cli and katana_mcp have no
//! views and refuse it by name. Corridors... and Corridor Quantities... are shown in Terrain's
//! Corridors section too - the same actions (`corridors_action`, `quantities_action`), as File
//! shows the imports of the other menus. In a drawing with no corridor and no alignment that has a
//! design profile they open nothing and say where one is made; once it has a corridor they always
//! open. Each action's data names its dialog (roadTemplateManagerDialog, corridorDialog,
//! corridorQuantitiesDialog), which is how --dialog finds it.
//!
//! THE ROAD MODEL IN THE VIEWS. Each 3D view draws the cad layer's bim_model's objects as meshes in
//! their colours (bim_view's scene_meshes, put in the window's list of meshes), and each plan view,
//! plot and sheet draws their plan outlines (plan_painter, PlanSource::road_model). Show Road Model
//! takes them out of both; an object also follows its layer. The meshes are made again, at the
//! window's next frame, when the road model, the surfaces, the alignments or the layers change.
//!
//! THE RUST SHAPE. What the C++ workbench was handed as RoadServices is the host it is lent at
//! each call (its document, executor, log and headless flag), and what it called back - Terrain's
//! Alignment Manager, the window's road meshes, a repaint of every view - it asks for through
//! `take_events`. The meshes point into the road model this workbench keeps, so the window reads
//! them from it (`road_meshes`) when told they changed, and holds them no longer than until the
//! next change.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use egui::{Key, KeyboardShortcut, Modifiers, Ui};
use katana_cad::bim::bim_object::{BimModel, bim_model};
use katana_cad::bim::bim_view::scene_meshes;
use katana_cad::layer_overrides::NO_LAYER_OVERRIDES;
use katana_cad::scene::SceneMesh;
use katana_cad::{Document, DocumentChange, ListenerHandle};
use katana_geometry::TriangleMesh;

use super::corridor_dialog::CorridorDialog;
use super::corridor_quantities_dialog::CorridorQuantitiesDialog;
use super::road_object_workbench::{BIM_MENU, BIM_MENU_TITLE, BIM_TOOL_BAR, BIM_TOOL_BAR_TITLE};
use super::road_template_manager::RoadTemplateManagerDialog;
use crate::drive::action::{Action, Menu, ToolBar, ToolBarItem};
use crate::host::Host;
use crate::plan_painter::{road_model_shown, set_road_model_shown};
use crate::shell::workbench::{DockSpec, Workbench};

pub const TEMPLATES_ACTION: &str = "bimRoadTemplates";
pub const CORRIDORS_ACTION: &str = "bimCorridors";
pub const QUANTITIES_ACTION: &str = "bimCorridorQuantities";
pub const SHOW_ROAD_MODEL_ACTION: &str = "bimShowRoadModel";

/// What a corridor command says when no alignment has a design profile to make a corridor from.
pub const NO_PROFILE_MESSAGE: &str = "No alignment has a design profile. Define one in Terrain > \
                                      Alignment Manager (Vertical), or with ALIGN DESIGN name s,z \
                                      ...";

/// What ROADMODEL SHOW says to a word it does not take.
pub const SHOW_USAGE: &str = "usage: ROADMODEL SHOW [ON|OFF]";

/// What the C++ workbench asked of the window through its services, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoadWorkbenchEvent {
    /// Terrain > Alignment Manager, where an alignment and its design profile are made: opened
    /// when a corridor command finds none to use (never in a headless run).
    ShowAlignmentManager,
    /// The road model's meshes among the 3D views' meshes are to be replaced by `road_meshes`,
    /// this many of them (setRoadMeshes).
    RoadMeshes(usize),
    /// Every view is to be repainted, for the plan outlines (refreshViews).
    RefreshViews,
}

/// See the module comment.
pub struct RoadWorkbench {
    // First, so it is dropped first.
    registration: ListenerHandle,
    /// The parts of the document notifications heard since the last look.
    heard: Arc<AtomicU32>,
    shown: bool,
    template_manager: Option<RoadTemplateManagerDialog>,
    corridor_dialog: Option<CorridorDialog>,
    quantities_dialog: Option<CorridorQuantitiesDialog>,
    /// The model whose meshes the views were last handed; kept alive while they point into it.
    handed_model: Option<Arc<BimModel>>,
    /// The meshes handed, by address, to tell the same objects handed again from new ones.
    showing: Vec<*const TriangleMesh>,
    handed_over: bool,
    events: Vec<RoadWorkbenchEvent>,
}

impl RoadWorkbench {
    /// The workbench over `document`, its Show Road Model as the session has it.
    pub fn new(document: &Document) -> Self {
        let heard = Arc::new(AtomicU32::new(0));
        let parts = Arc::clone(&heard);
        let registration = document.add_change_listener(move |change| {
            parts.fetch_or(change.parts, Ordering::AcqRel);
        });
        Self {
            registration,
            heard,
            shown: road_model_shown(),
            template_manager: None,
            corridor_dialog: None,
            quantities_dialog: None,
            handed_model: None,
            showing: Vec::new(),
            handed_over: false,
            events: Vec::new(),
        }
    }

    /// What the workbench asked of the window since the last call of this, in order.
    pub fn take_events(&mut self) -> Vec<RoadWorkbenchEvent> {
        std::mem::take(&mut self.events)
    }

    // ---- the actions --------------------------------------------------------------------------

    pub fn templates_action(&self) -> Action {
        Action::new(TEMPLATES_ACTION, "Road &Template Manager...")
            .icon("RoadTemplate")
            .tip(
                "Road templates: lanes, shoulders, kerbs, footpaths, verges, medians, drains and \
                 batters, and their pavement courses",
            )
            .opens("roadTemplateManagerDialog")
    }

    /// Corridors...: in Terrain's Corridors section too, the same action.
    pub fn corridors_action(&self) -> Action {
        Action::new(CORRIDORS_ACTION, "Corr&idors...")
            .icon("Corridor")
            .tip(
                "Corridors: a road template along an alignment, its regions, superelevation and \
                 target surface; build its strings and surfaces",
            )
            .opens("corridorDialog")
    }

    /// Corridor Quantities...: in Terrain's Corridors section too, the same action.
    pub fn quantities_action(&self) -> Action {
        Action::new(QUANTITIES_ACTION, "Corridor &Quantities...")
            .icon("CorridorQuantities")
            .tip(
                "Each course's volume and area, each kerb's length, the earthworks cut and fill: \
                 the road model's quantities, and a CSV of them",
            )
            .shortcut(KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::Q,
            ))
            .opens("corridorQuantitiesDialog")
    }

    pub fn show_action(&self) -> Action {
        Action::new(SHOW_ROAD_MODEL_ACTION, "Show Road Mod&el")
            .icon("RoadModel")
            .tip("Draw the road model's 3D objects in the 3D views and their outlines in plan")
            .checkable(self.shown)
    }

    /// The BIM menu's road model sections.
    pub fn menu(&self) -> Menu {
        Menu::new(BIM_MENU, BIM_MENU_TITLE)
            .section("Templates")
            .action(self.templates_action())
            .section("Corridors")
            .action(self.corridors_action())
            .section("Model")
            .action(self.show_action())
            .section("Quantities")
            .action(self.quantities_action())
    }

    /// The BIM toolbar's road model buttons (fillToolBar): the three dialogs, a separator, Show
    /// Road Model.
    pub fn tool_bar(&self) -> ToolBar {
        let mut bar = ToolBar::new(BIM_TOOL_BAR, BIM_TOOL_BAR_TITLE);
        bar.items.push(ToolBarItem::Action(self.templates_action()));
        bar.items.push(ToolBarItem::Action(self.corridors_action()));
        bar.items.push(ToolBarItem::Action(self.quantities_action()));
        bar.items.push(ToolBarItem::Separator);
        bar.items.push(ToolBarItem::Action(self.show_action()));
        bar
    }

    /// Whether Show Road Model is checked.
    pub fn road_model_checked(&self) -> bool {
        self.shown
    }

    /// QAction::setChecked on Show Road Model: when it changes, the road model is drawn or not,
    /// and the log says so in the item's own words (QAction::toggled).
    pub fn set_show_checked(&mut self, host: &mut dyn Host, on: bool) {
        if on == self.shown {
            return;
        }
        self.shown = on;
        self.set_shown(host.document(), on);
        host.log(
            &format!("road model {}", if on { "shown" } else { "hidden" }),
            false,
        );
    }

    /// The road model drawn or not, in the views and the plan painter.
    fn set_shown(&mut self, document: &Document, on: bool) {
        set_road_model_shown(on);
        self.sync_views(document);
    }

    // ---- ROADMODEL SHOW -----------------------------------------------------------------------

    /// ROADMODEL SHOW [ON|OFF]: true when `line` is that line, which has then been carried out and
    /// said what it did (a usage that is not, as an error). False for any other line, which is the
    /// interpreter's.
    pub fn run_road_line(&mut self, host: &mut dyn Host, line: &str) -> bool {
        let words: Vec<&str> = line.split(' ').filter(|word| !word.is_empty()).collect();
        if words.len() < 2
            || !words[0].eq_ignore_ascii_case("ROADMODEL")
            || !words[1].eq_ignore_ascii_case("SHOW")
        {
            return false;
        }
        // ON, OFF, or nothing to toggle - and anything else refused, as GRID has it.
        let on = match words.as_slice() {
            [_, _] => Some(!self.shown),
            [_, _, word] if word.eq_ignore_ascii_case("ON") => Some(true),
            [_, _, word] if word.eq_ignore_ascii_case("OFF") => Some(false),
            _ => None,
        };
        let Some(on) = on else {
            host.log(SHOW_USAGE, true);
            return true;
        };
        // The action follows the line without its own message, which the line's reply replaces;
        // the views and the painter are set the same way.
        self.shown = on;
        self.set_shown(host.document(), on);
        host.log(
            &format!("roadmodel={}", if on { "on" } else { "off" }),
            false,
        );
        true
    }

    // ---- the dialogs --------------------------------------------------------------------------

    /// Road Template Manager...: made the first time, shown.
    pub fn open_templates(&mut self, host: &mut dyn Host) {
        match &mut self.template_manager {
            Some(dialog) => dialog.show(),
            None => self.template_manager = Some(RoadTemplateManagerDialog::new(host.document())),
        }
    }

    /// Corridors...: when the drawing has a corridor or a profiled alignment to make one from.
    pub fn open_corridors(&mut self, host: &mut dyn Host) {
        if !self.corridor_dialogs_may_open(host) {
            return;
        }
        match &mut self.corridor_dialog {
            Some(dialog) => dialog.show(),
            None => self.corridor_dialog = Some(CorridorDialog::new(host.document())),
        }
    }

    /// Corridor Quantities...: as Corridors....
    pub fn open_quantities(&mut self, host: &mut dyn Host) {
        if !self.corridor_dialogs_may_open(host) {
            return;
        }
        match &mut self.quantities_dialog {
            Some(dialog) => dialog.show(),
            None => {
                self.quantities_dialog = Some(CorridorQuantitiesDialog::new(host.document()));
            }
        }
    }

    pub fn template_manager(&self) -> Option<&RoadTemplateManagerDialog> {
        self.template_manager.as_ref()
    }

    pub fn corridor_dialog(&self) -> Option<&CorridorDialog> {
        self.corridor_dialog.as_ref()
    }

    pub fn quantities_dialog(&self) -> Option<&CorridorQuantitiesDialog> {
        self.quantities_dialog.as_ref()
    }

    /// Whether Corridors... and Corridor Quantities... open: when the drawing has a corridor (its
    /// dialogs show it, and its problems, whatever became of its alignment), else when an
    /// alignment has a design profile to make one from (have_profiled_alignment, which says where
    /// one is made when none has).
    fn corridor_dialogs_may_open(&mut self, host: &mut dyn Host) -> bool {
        // A corridor is made from an alignment with a design profile, so with none and no
        // corridor there is nothing for the dialogs to do but say where one is made. A drawing
        // that already has corridors has something to show whatever its alignments are: a
        // corridor whose profile was cleared or whose alignment was deleted is exactly what the
        // Corridors dialog's problems are there to name, and to rename, delete or re-point.
        if !host.document().corridors().corridors.is_empty() {
            return true;
        }
        self.have_profiled_alignment(host)
    }

    /// False, having said why and where an alignment is made, when no alignment has a design
    /// profile.
    fn have_profiled_alignment(&mut self, host: &mut dyn Host) -> bool {
        let profiled = host
            .document()
            .model()
            .alignments
            .all()
            .iter()
            .any(|alignment| alignment.vertical.is_some());
        if profiled {
            return true;
        }
        host.log(NO_PROFILE_MESSAGE, true);
        // Where one is defined, opened for the person; a headless run has nobody to show it to.
        if !host.headless() {
            self.events.push(RoadWorkbenchEvent::ShowAlignmentManager);
        }
        false
    }

    // ---- the road model in the views ----------------------------------------------------------

    /// What the road model and its visibility are made from changed since the last look: the
    /// meshes made again, once however many changes arrived - a listener must not rebuild inside
    /// the command that notified, and a script of a hundred template edits makes the model once.
    fn look(&mut self, document: &Document) {
        let parts = self.heard.swap(0, Ordering::AcqRel);
        if !self.registration.active() {
            return;
        }
        let watched = DocumentChange::ROAD_MODEL
            | DocumentChange::SURFACES
            | DocumentChange::ALIGNMENTS
            | DocumentChange::LAYERS
            | DocumentChange::REPLACED;
        if parts & watched != 0 {
            self.sync_views(document);
        }
    }

    /// Makes the road model's 3D meshes again now (the window calls it when it has put a new
    /// drawing in place).
    pub fn sync_views(&mut self, document: &Document) {
        let model = if self.shown {
            Some(bim_model(document))
        } else {
            None
        };
        let showing: Vec<*const TriangleMesh> = match &model {
            Some(model) => scene_meshes(model, document.model(), &NO_LAYER_OVERRIDES)
                .iter()
                .filter_map(|mesh| mesh.mesh.map(std::ptr::from_ref))
                .collect(),
            None => Vec::new(),
        };
        // Nothing to hand over when it is the same model showing the same objects: a layer
        // switched on elsewhere would make every 3D view build its terrain again for nothing.
        let same_model = match (&model, &self.handed_model) {
            (Some(now), Some(before)) => Arc::ptr_eq(now, before),
            (None, None) => true,
            _ => false,
        };
        if same_model && showing == self.showing && self.handed_over {
            return;
        }
        // The views take the new meshes before the model the old ones point into is let go: the
        // window reads them at the event, from the model kept now.
        let count = showing.len();
        self.handed_model = model;
        self.showing = showing;
        self.handed_over = true;
        self.events.push(RoadWorkbenchEvent::RoadMeshes(count));
        self.events.push(RoadWorkbenchEvent::RefreshViews);
    }

    /// The road model's meshes as last handed over: none while Show Road Model is off. They point
    /// into the model this workbench keeps until it hands over the next ones.
    pub fn road_meshes(&self, document: &Document) -> Vec<SceneMesh<'_>> {
        match &self.handed_model {
            Some(model) => scene_meshes(model, document.model(), &NO_LAYER_OVERRIDES),
            None => Vec::new(),
        }
    }
}

impl Workbench for RoadWorkbench {
    fn menus(&self) -> Vec<Menu> {
        vec![self.menu()]
    }

    fn tool_bars(&self) -> Vec<ToolBar> {
        vec![self.tool_bar()]
    }

    fn docks(&self) -> Vec<DockSpec> {
        Vec::new()
    }

    fn run_line(&mut self, host: &mut dyn Host, line: &str) -> bool {
        self.run_road_line(host, line)
    }

    fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool {
        match name {
            TEMPLATES_ACTION => self.open_templates(host),
            CORRIDORS_ACTION => self.open_corridors(host),
            QUANTITIES_ACTION => self.open_quantities(host),
            SHOW_ROAD_MODEL_ACTION => {
                let on = !self.shown;
                self.set_show_checked(host, on);
            }
            _ => return false,
        }
        true
    }

    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.look(host.document());
        if let Some(dialog) = &mut self.template_manager {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.corridor_dialog {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.quantities_dialog {
            dialog.ui(ui, host);
        }
    }
}
