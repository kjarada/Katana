//! The drawing system's part of the window (docs/drawing.md): the Vertices dock and the drafting
//! aids' toggles, made in one call so that the window gains a few lines, not a section. The port
//! of drawing/drawing_ui.
//!
//! ```text
//!   Vertices dock      "VerticesDock", holding the Vertices panel ("vertexPanel"); its toggle is
//!                      in View > Panels ("actionVerticesPanel"), and Draw > Vertices > Edit
//!                      Vertices opens it
//!   Drafting toolbar   "DraftingToolbar": Ortho (F8, "actionOrtho"), Polar tracking (F10,
//!                      "actionPolar"), Object snap tracking (F11, "actionObjectTracking"), angles
//!                      as bearings ("actionBearings"), and the snaps the drawing system added
//!                      ("actionSnapQuadrant", "actionSnapNode", "actionSnapExtension",
//!                      "actionSnapParallel", "actionSnapApparentIntersection"), each bound to
//!                      the document's drafting settings and read from them whenever its row is
//!                      asked for, so a verb that turns ortho on checks its button.
//! ```
//!
//! The Rust shape. The C++ makes QActions that listen to the document; here the actions are rows
//! (drive::action), made from the document's drafting settings each time the window asks for
//! them, so there is nothing to keep in step. The window (K) puts `panel_action` in View > Panels,
//! `drafting_bar` in its bottom toolbar area and `dock` among its docks, and hands a trigger of
//! one of these names to `trigger`.

use egui::{Key, KeyboardShortcut, Modifiers, Ui};
use katana_cad::Document;
use katana_cad::drawing::drafting::{AngleConvention, DraftingSettings};
use katana_cad::snapping::{SnapMode, SnapModes};
use katana_entity::entity::to_string;

use super::vertex_panel::VertexPanel;
use crate::drive::action::{Action, ToolBar, ToolBarItem};
use crate::shell::workbench::DockSpec;

/// The Vertices dock's object name.
pub const VERTICES_DOCK: &str = "VerticesDock";
/// The Vertices dock's toggle in View > Panels.
pub const VERTICES_PANEL_ACTION: &str = "actionVerticesPanel";
/// The drafting toolbar's object name.
pub const DRAFTING_TOOLBAR: &str = "DraftingToolbar";

/// A checkable action bound to one flag of the drafting settings.
struct Bound {
    name: &'static str,
    text: &'static str,
    tip: &'static str,
    shortcut: Option<Key>,
    read: fn(&DraftingSettings) -> bool,
    write: fn(&mut DraftingSettings, bool),
}

const BOUND: [Bound; 3] = [
    Bound {
        name: "actionOrtho",
        text: "Ortho",
        tip: "Ortho: points from a base go straight across or straight up (F8)",
        shortcut: Some(Key::F8),
        read: |settings| settings.ortho,
        write: |settings, on| settings.ortho = on,
    },
    Bound {
        name: "actionPolar",
        text: "Polar",
        tip: "Polar tracking: a direction near a multiple of 15 degrees snaps to it (F10)",
        shortcut: Some(Key::F10),
        read: |settings| settings.polar,
        write: |settings, on| settings.polar = on,
    },
    Bound {
        name: "actionObjectTracking",
        text: "Tracking",
        tip: "Object snap tracking: paths through points the cursor has rested on (F11)",
        shortcut: Some(Key::F11),
        read: |settings| settings.object_tracking,
        write: |settings, on| settings.object_tracking = on,
    },
];

const BEARINGS_ACTION: &str = "actionBearings";

/// The snaps the drawing system added, each with its toggle's name.
const NEW_SNAPS: [(SnapMode, &str); 5] = [
    (SnapMode::Quadrant, "actionSnapQuadrant"),
    (SnapMode::Node, "actionSnapNode"),
    (SnapMode::Extension, "actionSnapExtension"),
    (SnapMode::Parallel, "actionSnapParallel"),
    (
        SnapMode::ApparentIntersection,
        "actionSnapApparentIntersection",
    ),
];

/// See the module comment.
pub struct DrawingUi {
    pub vertex_panel: VertexPanel,
    /// Whether the Vertices dock is shown: hidden until asked for, by Edit Vertices or its toggle.
    vertices_shown: bool,
}

impl DrawingUi {
    /// The drawing system's part of the window over `document` (the C++ installDrawingUi).
    pub fn install(document: &Document) -> Self {
        Self {
            vertex_panel: VertexPanel::new(document),
            vertices_shown: false,
        }
    }

    /// The Vertices dock, for the window's right dock area.
    pub fn dock(&self) -> DockSpec {
        DockSpec::new(VERTICES_DOCK, "Vertices")
    }

    pub fn vertices_shown(&self) -> bool {
        self.vertices_shown
    }

    /// Shows and raises the Vertices dock (Edit Vertices does this).
    pub fn show_vertices(&mut self) {
        self.vertices_shown = true;
    }

    /// The Vertices dock's toggle, for View > Panels. Its picture is Edit Vertices': that tool
    /// opens this panel.
    pub fn panel_action(&self) -> Action {
        Action::new(VERTICES_PANEL_ACTION, "&Vertices")
            .icon("draw.vertex.edit")
            .tip("Show the selected polyline's vertices in a table to edit in place")
            .checkable(self.vertices_shown)
            .opens(VERTICES_DOCK)
    }

    /// The drafting toolbar, its toggles checked as `document`'s drafting settings are now.
    pub fn drafting_bar(&self, document: &Document) -> ToolBar {
        let settings = document.drafting();
        let mut bar = ToolBar::new(DRAFTING_TOOLBAR, "Drafting");
        for bound in &BOUND {
            let mut action = Action::new(bound.name, bound.text)
                .tip(bound.tip)
                .checkable((bound.read)(settings));
            if let Some(key) = bound.shortcut {
                action = action.shortcut(KeyboardShortcut::new(Modifiers::NONE, key));
            }
            bar.items.push(ToolBarItem::Action(action));
        }
        bar.items.push(ToolBarItem::Action(
            Action::new(BEARINGS_ACTION, "Bearings")
                .tip(
                    "Angles typed after < are whole-circle bearings, clockwise from north, \
                     instead of degrees counter-clockwise from east",
                )
                .checkable(settings.angles == AngleConvention::Bearing),
        ));
        bar.items.push(ToolBarItem::Separator);
        for (mode, name) in NEW_SNAPS {
            let bit = mode as SnapModes;
            bar.items.push(ToolBarItem::Action(
                Action::new(name, to_string(mode))
                    .tip(&format!("Object snap: {}", to_string(mode)))
                    .checkable((settings.snap_modes & bit) != 0),
            ));
        }
        bar
    }

    /// The action `name` of the drawing system's, as it is now; None when it has none of that
    /// name.
    pub fn action(&self, document: &Document, name: &str) -> Option<Action> {
        if name == VERTICES_PANEL_ACTION {
            return Some(self.panel_action());
        }
        self.drafting_bar(document)
            .actions()
            .into_iter()
            .find(|action| action.name == name)
            .cloned()
    }

    /// Triggers the action `name` as a click does: the flag it is bound to is turned over and the
    /// document says its drafting settings changed, so View > Snap Modes and the views follow
    /// (one owner: the document's drafting settings). False when the drawing system has no action
    /// of that name.
    pub fn trigger(&mut self, document: &mut Document, name: &str) -> bool {
        if name == VERTICES_PANEL_ACTION {
            self.vertices_shown = !self.vertices_shown;
            return true;
        }
        if let Some(bound) = BOUND.iter().find(|bound| bound.name == name) {
            let on = !(bound.read)(document.drafting());
            (bound.write)(document.drafting_mut(), on);
            document.notify_drafting_changed();
            return true;
        }
        if name == BEARINGS_ACTION {
            let angles = if document.drafting().angles == AngleConvention::Bearing {
                AngleConvention::Counterclockwise
            } else {
                AngleConvention::Bearing
            };
            document.drafting_mut().angles = angles;
            document.notify_drafting_changed();
            return true;
        }
        if let Some((mode, _)) = NEW_SNAPS.iter().find(|(_, action)| *action == name) {
            let bit = *mode as SnapModes;
            let modes = document.drafting().snap_modes;
            let next = if (modes & bit) != 0 {
                modes & !bit
            } else {
                modes | bit
            };
            document.drafting_mut().snap_modes = next;
            document.notify_drafting_changed();
            return true;
        }
        false
    }

    /// The Vertices dock's content, the panel.
    pub fn dock_ui(&mut self, ui: &mut Ui, document: &mut Document) {
        self.vertex_panel.ui(ui, document);
    }
}
