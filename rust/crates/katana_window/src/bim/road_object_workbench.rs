//! BIM > Road Objects (docs/road_objects.md, "In the window"): the ROADOBJECT verb in the window.
//! The port of bim/road_object_workbench.
//!
//! Built as the utilities' workbench is: the main window (K) only composes it, with its rows in the
//! BIM menu and toolbar, and keeps it. The verb is the CommandInterpreter's, shared with katana_cli
//! and katana_mcp, and a typed ROADOBJECT line reaches it as any other does; what this adds is what
//! only a window has:
//!
//! * the menu's Road Objects section: Barrier, Guide Posts, Sign, Light Poles, Culvert, Wall, Fence
//!   and Line Marking (bimRoadBarrier, bimRoadGuidePosts, bimRoadSign, bimRoadLightPoles,
//!   bimRoadCulvert, bimRoadWall, bimRoadFence, bimRoadLineMarking), each opening the one placing
//!   dialog (bim::road_object_dialog) on its own page, and Road Objects... (bimRoadObjects), the
//!   manager (bim::road_object_manager);
//! * toolbar buttons for the ones placed most: barrier, sign, light poles, line marking, and the
//!   manager, after a separator on the BIM toolbar.
//!
//! Both dialogs run their lines through the window's one executor (host::Host::run), so a line from
//! a dialog is echoed, kept in the history and undone exactly as a typed one.
//!
//! The icons are the icon set's (I's bim::road_object_icons, painted by code through the canvas):
//! each action names its icon by the C++ enumerator's name ("RoadBarrier"), which the window's icon
//! cache draws.

use egui::Ui;
use katana_cad::road::road_objects::{ROAD_OBJECT_KIND_COUNT, RoadObjectKind};

use super::road_object_dialog::RoadObjectDialog;
use super::road_object_manager::RoadObjectManagerDialog;
use crate::customisation::scope_filter_widget::scope_filter_views;
use crate::drive::action::{Action, Menu, ToolBar, ToolBarItem};
use crate::host::Host;
use crate::shell::workbench::{DockSpec, Workbench};

/// The BIM menu, as the main window names it: the section is added to it.
pub const BIM_MENU: &str = "bimMenu";
pub const BIM_MENU_TITLE: &str = "&BIM";
/// The BIM toolbar, as the main window names it.
pub const BIM_TOOL_BAR: &str = "BIMToolBar";
pub const BIM_TOOL_BAR_TITLE: &str = "BIM";
/// The section's title in the menu.
pub const SECTION: &str = "Road Objects";

/// One kind's action. The section, in the order of the dialog's pages. The letters are ones the
/// BIM menu's road model section leaves (--check-shortcuts): B, G, N, L, U, W, F, M and, for the
/// manager, O.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KindAction {
    pub kind: RoadObjectKind,
    /// The icon's name in the window's icon set.
    pub icon: &'static str,
    pub text: &'static str,
    pub tip: &'static str,
    pub name: &'static str,
    /// Shown on the BIM toolbar too: the ones placed most.
    pub on_tool_bar: bool,
}

pub const KIND_ACTIONS: [KindAction; ROAD_OBJECT_KIND_COUNT] = [
    KindAction {
        kind: RoadObjectKind::Barrier,
        icon: "RoadBarrier",
        text: "&Barrier...",
        tip: "Place a safety barrier - W-beam, thrie-beam, concrete F-shape or single slope, wire \
              rope - along an alignment or along lines in the drawing: the ROADOBJECT ADD line, \
              one undo step",
        name: "bimRoadBarrier",
        on_tool_bar: true,
    },
    KindAction {
        kind: RoadObjectKind::GuidePost,
        icon: "RoadGuidePosts",
        text: "&Guide Posts...",
        tip: "Place guide posts at a spacing along an alignment or a line, or at points",
        name: "bimRoadGuidePosts",
        on_tool_bar: false,
    },
    KindAction {
        kind: RoadObjectKind::Sign,
        icon: "RoadSign",
        text: "Sig&n...",
        tip: "Place a sign - its plate's shape and size, one post or two, its code and legend - \
              at a station and offset, along a line or at points",
        name: "bimRoadSign",
        on_tool_bar: true,
    },
    KindAction {
        kind: RoadObjectKind::LightPole,
        icon: "RoadLightPole",
        text: "&Light Poles...",
        tip: "Place light poles - mounting height, outreach arm, lantern - at a spacing along an \
              alignment or a line, or at points",
        name: "bimRoadLightPoles",
        on_tool_bar: true,
    },
    KindAction {
        kind: RoadObjectKind::Culvert,
        icon: "RoadCulvert",
        text: "C&ulvert...",
        tip: "Place a culvert - pipe or box, cells, size, skew, inverts or cover, headwalls - \
              across an alignment at a station, or between two points",
        name: "bimRoadCulvert",
        on_tool_bar: false,
    },
    KindAction {
        kind: RoadObjectKind::Wall,
        icon: "RoadWall",
        text: "&Wall...",
        tip: "Place a retaining wall or a noise wall - height, thickness, footing - along an \
              alignment or a line",
        name: "bimRoadWall",
        on_tool_bar: false,
    },
    KindAction {
        kind: RoadObjectKind::Fence,
        icon: "RoadFence",
        text: "&Fence...",
        tip: "Place a fence - posts at a spacing and its wires - along an alignment or a line",
        name: "bimRoadFence",
        on_tool_bar: false,
    },
    KindAction {
        kind: RoadObjectKind::Marking,
        icon: "RoadLineMarking",
        text: "Line &Marking...",
        tip: "Place a line marking - edge, centre, lane or continuity line, with its dash \
              pattern - along an alignment at an offset, or along a line",
        name: "bimRoadLineMarking",
        on_tool_bar: true,
    },
];

/// The manager's action's object name.
pub const MANAGER_ACTION: &str = "bimRoadObjects";

/// The action that opens the placing dialog on `entry`'s page: its data names the dialog, which
/// is how --dialog finds it, since eight actions share one dialog.
pub fn kind_action(entry: &KindAction) -> Action {
    Action::new(entry.name, entry.text)
        .icon(entry.icon)
        .tip(entry.tip)
        .opens("roadObjectDialog")
}

/// Road Objects...: the manager.
pub fn manager_action() -> Action {
    Action::new(MANAGER_ACTION, "Road &Objects...")
        .icon("RoadObjects")
        .tip(
            "The drawing's road objects: list them with their quantities and problems, set their \
             keys, copy, delete and zoom to them",
        )
        .opens("roadObjectManagerDialog")
}

/// The workbench (see the module comment): the two dialogs, each made the first time it is asked
/// for.
#[derive(Default)]
pub struct RoadObjectWorkbench {
    dialog: Option<RoadObjectDialog>,
    manager: Option<RoadObjectManagerDialog>,
}

impl RoadObjectWorkbench {
    pub fn new() -> Self {
        Self::default()
    }

    /// The action of `kind`.
    pub fn action(&self, kind: RoadObjectKind) -> Action {
        kind_action(&KIND_ACTIONS[kind as usize])
    }

    pub fn manager_action(&self) -> Action {
        manager_action()
    }

    /// The BIM menu's Road Objects section, after what is already there: a section with its title,
    /// the kinds, the manager.
    pub fn menu(&self) -> Menu {
        let mut menu = Menu::new(BIM_MENU, BIM_MENU_TITLE).section(SECTION);
        for entry in &KIND_ACTIONS {
            menu = menu.action(kind_action(entry));
        }
        menu.action(manager_action())
    }

    /// The BIM toolbar's group: a separator after whatever the bar holds already - its name, and
    /// the road model's buttons - then the ones placed most and the manager.
    pub fn tool_bar(&self) -> ToolBar {
        let mut bar = ToolBar::new(BIM_TOOL_BAR, BIM_TOOL_BAR_TITLE);
        bar.items.push(ToolBarItem::Separator);
        for entry in KIND_ACTIONS.iter().filter(|entry| entry.on_tool_bar) {
            bar.items.push(ToolBarItem::Action(kind_action(entry)));
        }
        bar.items.push(ToolBarItem::Action(manager_action()));
        bar
    }

    /// The placing dialog, made the first time it is asked for, the workspace's views offered to
    /// its scope.
    pub fn dialog(&mut self, host: &mut dyn Host) -> &mut RoadObjectDialog {
        self.dialog.get_or_insert_with(|| {
            let view_set = host.views().map(|views| views.view_set());
            let mut made = RoadObjectDialog::new(host.document());
            if let Some(set) = view_set {
                made.set_views(Box::new(move || scope_filter_views(&set.borrow())));
                made.reload(host.document());
            }
            made
        })
    }

    /// The manager, made the first time it is asked for.
    pub fn manager(&mut self, host: &mut dyn Host) -> &mut RoadObjectManagerDialog {
        self.manager.get_or_insert_with(|| {
            let view_set = host.views().map(|views| views.view_set());
            let mut made = RoadObjectManagerDialog::new(host.document_mut());
            if let Some(set) = view_set {
                made.set_views(Box::new(move || scope_filter_views(&set.borrow())));
                made.reload(host.document_mut());
            }
            made
        })
    }

    /// The placing dialog as made, if it has been.
    pub fn placing_dialog(&self) -> Option<&RoadObjectDialog> {
        self.dialog.as_ref()
    }

    /// The manager as made, if it has been.
    pub fn manager_dialog(&self) -> Option<&RoadObjectManagerDialog> {
        self.manager.as_ref()
    }

    /// Shows the placing dialog on `kind`'s page.
    pub fn open(&mut self, host: &mut dyn Host, kind: RoadObjectKind) {
        let shown = self.dialog(host);
        shown.show_kind(kind);
        shown.show(host.document());
    }

    /// Shows the manager, read again from the drawing.
    pub fn open_manager(&mut self, host: &mut dyn Host) {
        let shown = self.manager(host);
        shown.reload(host.document_mut());
        shown.show();
    }
}

impl Workbench for RoadObjectWorkbench {
    fn menus(&self) -> Vec<Menu> {
        vec![self.menu()]
    }

    fn tool_bars(&self) -> Vec<ToolBar> {
        vec![self.tool_bar()]
    }

    fn docks(&self) -> Vec<DockSpec> {
        Vec::new()
    }

    /// ROADOBJECT is the interpreter's: no line is this workbench's own.
    fn run_line(&mut self, _host: &mut dyn Host, _line: &str) -> bool {
        false
    }

    fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool {
        if name == MANAGER_ACTION {
            self.open_manager(host);
            return true;
        }
        match KIND_ACTIONS.iter().find(|entry| entry.name == name) {
            Some(entry) => {
                self.open(host, entry.kind);
                true
            }
            None => false,
        }
    }

    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if let Some(dialog) = &mut self.dialog {
            dialog.ui(ui, host);
        }
        if let Some(manager) = &mut self.manager {
            manager.ui(ui, host);
        }
        // New in the manager opens the placing dialog on the chosen row's kind.
        let asked = self
            .manager
            .as_mut()
            .and_then(RoadObjectManagerDialog::take_place_request);
        if let Some(kind) = asked {
            self.open(host, kind);
        }
    }
}
