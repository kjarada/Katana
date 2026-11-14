//! Terrain > Edit Surface in the window (terrain/surface_edit_ui.hpp; docs/terrain.md, "Editing a
//! surface"): the submenu and the Surface Edit toolbar the tool catalogue fills with the editing
//! tools, the dialogs' items below them, Combine Surfaces, and the toolbar's list of the surface
//! the tools act on.
//!
//! ```text
//! terrainEditMenu        Terrain > Edit Surface: the tools, then "Levels and Triangles"
//! SurfaceEditToolBar     surfaceEditActive (the active surface), the tools, the dialogs' items
//! terrainSurfaceHeights ... terrainSurfaceDeleteBreakline, terrainCombineSurfaces
//! ```
//!
//! The Rust shape. The C++ made a QMenu and a QToolBar, lent them to the tool catalogue's fill, and
//! added to them after. Here a menu and a toolbar are plain data (drive::action), so the window
//! takes them (`menu`, `tool_bar`), hands them to `tools::fill_tool_menus` as its "Terrain" targets,
//! and gives them back to `finish`, which adds the dialogs' items and puts the submenu and Combine
//! Surfaces at the end of the Terrain menu. The dialogs are kept here, made the first time their
//! item is chosen.
//!
//! The list follows the drawing and the tools' target at its next look (`active_ui`), where the
//! C++ listened to the document and to the target's observers.

use egui::Ui;
use katana_cad::terrain::surface_edit_verbs::{
    active_edit_surface, set_surface_edit_target, surface_edit_target,
};

use super::surface_edit_dialogs::{
    SurfaceCombineDialog, SurfaceLevelsDialog, SurfaceLevelsKind, surface_levels_prefix,
};
use crate::drive::action::{Action, Menu, MenuItem, ToolBar, ToolBarItem};
use crate::drive::{Choice, choice};
use crate::host::Host;

/// One of the dialogs' items: its icon, text, status tip, object name and what it opens.
struct Item {
    icon: &'static str,
    text: &'static str,
    tip: &'static str,
    name: &'static str,
    kind: SurfaceLevelsKind,
}

// The letters are each one item's in the submenu. The tools choose theirs (tools::with_mnemonic)
// after Combine Surfaces has B, so Add Breakline, finding A and B taken, takes R: the levels item
// is V, not R, which --check-shortcuts found taken twice. These take V, S, N and L (Smooth had M
// until the Move Point tool came: a tool takes the initial of its name, and M is that of Move).
const ITEMS: [Item; 8] = [
    Item {
        icon: "SurfaceLevels",
        text: "Raise, Lower or Set Le&vel...",
        tip: "Raise, lower, set or scale the levels of the surface, or of the part inside a \
              polygon",
        name: "terrainSurfaceHeights",
        kind: SurfaceLevelsKind::Heights,
    },
    Item {
        icon: "SurfaceSmooth",
        text: "&Smooth...",
        tip: "Smooth the surface's levels, holding its rim and breaklines",
        name: "terrainSurfaceSmooth",
        kind: SurfaceLevelsKind::Smooth,
    },
    Item {
        icon: "SurfaceThin",
        text: "Thi&n...",
        tip: "Remove the points that move the surface by no more than a tolerance",
        name: "terrainSurfaceThin",
        kind: SurfaceLevelsKind::Thin,
    },
    Item {
        icon: "SurfaceDeleteLong",
        text: "Delete &Long Triangles...",
        tip: "Delete the triangles with a side too long or an angle too thin",
        name: "terrainSurfaceDeleteLong",
        kind: SurfaceLevelsKind::DeleteLong,
    },
    // The tools take one picked entity; these take any number, by a scope (the polygons of a
    // layer, the selection, what a view shows) as every tool that reads the drawing does. No
    // underlined letter: the tools above took the free ones, and --check-shortcuts counts each
    // once.
    Item {
        icon: "SurfaceEdit",
        text: "Delete Points Inside Polygons...",
        tip: "Delete the points of the surface inside the polygons a scope takes (breakline and \
              rim points are kept)",
        name: "terrainSurfaceDeletePoints",
        kind: SurfaceLevelsKind::DeletePoints,
    },
    Item {
        icon: "SurfaceDeleteLong",
        text: "Delete Triangles by Polygons...",
        tip: "Delete the triangles inside, or outside, the union of the polygons a scope takes",
        name: "terrainSurfaceDeleteTriangles",
        kind: SurfaceLevelsKind::DeleteTriangles,
    },
    Item {
        icon: "SurfaceLevels",
        text: "Add Points from the Drawing...",
        tip: "Add the points and levelled vertices a scope takes to the surface; a point off it \
              extends it",
        name: "terrainSurfaceAddPoints",
        kind: SurfaceLevelsKind::AddPoints,
    },
    Item {
        icon: "SurfaceEdit",
        text: "Delete Breaklines Along Lines...",
        tip: "Make the breakline edges that lie along the lines a scope takes ordinary edges (the \
              triangles stay; a point on one can then be deleted)",
        name: "terrainSurfaceDeleteBreakline",
        kind: SurfaceLevelsKind::DeleteBreakline,
    },
];

const COMBINE_NAME: &str = "terrainCombineSurfaces";
const ACTIVE_NAME: &str = "surfaceEditActive";

/// Terrain > Edit Surface in the window (see the module comment).
pub struct SurfaceEditUi {
    menu: Menu,
    tool_bar: ToolBar,
    surfaces: Vec<String>,
    active: Choice,
    /// The drawing's surfaces as the list last read them (their generation).
    surfaces_seen: Option<u64>,
    levels: Vec<SurfaceLevelsDialog>,
    combine: Option<SurfaceCombineDialog>,
}

impl SurfaceEditUi {
    /// The submenu and the toolbar, empty but for the toolbar's list, and the list read from the
    /// drawing. Made before the tool catalogue fills the menus, so it can fill these.
    pub fn new(host: &dyn Host) -> Self {
        let mut tool_bar = ToolBar::new("SurfaceEditToolBar", "Surface Edit");
        tool_bar
            .items
            .push(ToolBarItem::Widget(String::from(ACTIVE_NAME)));
        tool_bar.items.push(ToolBarItem::Separator);
        let mut made = Self {
            menu: Menu::new("terrainEditMenu", "Edi&t Surface"),
            tool_bar,
            surfaces: Vec::new(),
            active: Choice::default(),
            surfaces_seen: None,
            levels: Vec::new(),
            combine: None,
        };
        made.refresh_surfaces(host);
        made
    }

    /// The submenu's icon and status tip, as the window shows a submenu's.
    pub fn menu_icon() -> &'static str {
        "SurfaceEdit"
    }

    pub fn menu_tip() -> &'static str {
        "Edit the active surface's triangles, points, breaklines, boundary and levels, each one \
         undo step (SURFACE EDIT)"
    }

    pub fn menu(&self) -> &Menu {
        &self.menu
    }

    pub fn tool_bar(&self) -> &ToolBar {
        &self.tool_bar
    }

    /// The action `name` among the submenu's and the toolbar's.
    pub fn action(&self, name: &str) -> Option<Action> {
        self.menu
            .actions()
            .into_iter()
            .chain(self.tool_bar.actions())
            .find(|action| action.name == name)
            .cloned()
    }

    /// After the tools are in - `menu` and `tool_bar` as the catalogue filled them: the dialogs'
    /// items below them, and the Edit Surface submenu and Combine Surfaces at the end of `terrain`
    /// (its Surfaces section, where the window calls this).
    pub fn finish(&mut self, menu: Menu, tool_bar: ToolBar, terrain: &mut Menu) {
        self.menu = menu;
        self.tool_bar = tool_bar;
        self.menu
            .items
            .push(MenuItem::Section(String::from("Levels and Triangles")));
        self.tool_bar.items.push(ToolBarItem::Separator);
        for item in &ITEMS {
            let action = Action::new(item.name, item.text)
                .icon(item.icon)
                .tip(item.tip)
                .opens(&format!("{}Dialog", surface_levels_prefix(item.kind)));
            self.menu.items.push(MenuItem::Action(action.clone()));
            self.tool_bar.items.push(ToolBarItem::Action(action));
        }
        let combine = Action::new(COMBINE_NAME, "Com&bine Surfaces...")
            .icon("SurfaceCombine")
            .tip(
                "Merge surfaces, paste one into another with a clean seam, or make a clipped or \
                 offset copy",
            )
            .opens("surfaceCombineDialog");
        self.tool_bar
            .items
            .push(ToolBarItem::Action(combine.clone()));
        terrain.items.push(MenuItem::Menu(self.menu.clone()));
        terrain.items.push(MenuItem::Action(combine));
    }

    /// The surfaces the toolbar's list offers.
    pub fn surfaces(&self) -> &[String] {
        &self.surfaces
    }

    /// The surface the list shows; empty when the drawing has none.
    pub fn active_text(&self) -> String {
        self.active.current_text(&self.surfaces)
    }

    /// Refills the toolbar's surface list from the drawing.
    pub fn refresh_surfaces(&mut self, host: &dyn Host) {
        let document = host.document();
        self.surfaces_seen = Some(document.surfaces_generation());
        self.surfaces = document
            .surfaces()
            .all()
            .iter()
            .map(|surface| surface.name.clone())
            .collect();
        self.active.index = active_edit_surface(document)
            .ok()
            .and_then(|name| self.surfaces.iter().position(|listed| *listed == name));
    }

    /// Points the list at the surface the tools act on now, without choosing it again: a tool's
    /// own [Surface] option sets the target, not the list.
    pub fn sync_active(&mut self, host: &dyn Host) {
        let Ok(name) = active_edit_surface(host.document()) else {
            return;
        };
        if let Some(at) = self.surfaces.iter().position(|listed| *listed == name) {
            self.active.index = Some(at);
        }
    }

    /// Chooses the list's item `index`, as a person's choice or a driver's fill does: the tools
    /// are pointed at that surface.
    pub fn choose_active(&mut self, index: usize) {
        if let Some(name) = self.surfaces.get(index) {
            self.active.index = Some(index);
            set_surface_edit_target(name.clone());
        }
    }

    /// The toolbar's list of the active surface (surfaceEditActive), which the window draws where
    /// the toolbar has it: read again when the drawing's surfaces changed, pointed at the tools'
    /// target, and a choice in it points the tools.
    pub fn active_ui(&mut self, ui: &mut Ui, host: &dyn Host) {
        if self.surfaces_seen != Some(host.document().surfaces_generation()) {
            self.refresh_surfaces(host);
        }
        self.sync_active(host);
        let has_surfaces = !self.surfaces.is_empty();
        let changed = ui
            .add_enabled_ui(has_surfaces, |ui| {
                choice(ui, ACTIVE_NAME, &self.surfaces, &mut self.active)
                    .on_hover_text(
                        "Active surface: the surface the Edit Surface tools act on. A tool's own \
                         S option chooses it too.",
                    )
                    .changed()
            })
            .inner;
        if changed && let Some(index) = self.active.index {
            self.choose_active(index);
        }
    }

    /// The levels dialog of `kind`, made the first time it is asked for.
    pub fn levels_dialog(
        &mut self,
        host: &mut dyn Host,
        kind: SurfaceLevelsKind,
    ) -> &mut SurfaceLevelsDialog {
        let found = self.levels.iter().position(|dialog| dialog.kind() == kind);
        let at = match found {
            Some(at) => at,
            None => {
                self.levels.push(SurfaceLevelsDialog::new(kind, host));
                self.levels.len() - 1
            }
        };
        &mut self.levels[at]
    }

    /// The Combine Surfaces dialog, made the first time it is asked for.
    pub fn combine_dialog(&mut self, host: &mut dyn Host) -> &mut SurfaceCombineDialog {
        if self.combine.is_none() {
            self.combine = Some(SurfaceCombineDialog::new(host));
        }
        self.combine
            .as_mut()
            .expect("the dialog was made just above")
    }

    /// Triggers the item `name`: its dialog shown, made the first time. False for a name that is
    /// none of the dialogs' items (a tool's, which the window runs).
    pub fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool {
        if name == COMBINE_NAME {
            self.combine_dialog(host).show(&*host);
            return true;
        }
        match ITEMS.iter().find(|item| item.name == name) {
            Some(item) => {
                self.levels_dialog(host, item.kind).show(&*host);
                true
            }
            None => false,
        }
    }

    /// One frame of the dialogs that are shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        for dialog in &mut self.levels {
            dialog.ui(ui, host);
        }
        if let Some(combine) = &mut self.combine {
            combine.ui(ui, host);
        }
    }

    /// The tools' target, as the tools read it; for the window's status and the tests.
    pub fn target() -> String {
        surface_edit_target()
    }
}
