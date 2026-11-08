//! View kinds and the layout presets (include/katana/cad/viewport_layout.hpp).
//!
//! The drawing area shows several views of one document at once - plan, a 3D
//! view, a section - each through its own camera. Until 2026-09-23 they were
//! TILES: a ViewportLayout class split the area into fixed cells, computed
//! every cell's pixel rectangle and answered which cell a click landed in.
//! The views are docks now (view_set; docs/cad.md, "The workspace"), so the
//! window owns the pixels and the hit test, and a view is identified by an id
//! rather than by its position in a split. What survives here is what a dock
//! layout still needs from the model:
//!
//! * ViewKind, what a view shows;
//! * the presets (LayoutKind) and their pictures (layout_rects), which the
//!   View menu offers and draws;
//! * dock_splits, the same pictures as a sequence of dock splits, tested to
//!   reproduce layout_rects exactly;
//! * the kind each slot of a preset opens (default_view_kind) and the camera
//!   a fresh view of a kind starts with (configure_camera).

use katana_entity::EnumName;
use katana_render::{Camera, Projection, StandardView};

/// What a view shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewKind {
    /// 2D top-down; the classic drawing editor.
    Plan,
    /// Orbiting 3D view of surfaces and drawing geometry.
    Model3D,
    /// Elevation against station along an alignment.
    Section,
    /// Fixed orthographic side view (front/back/left/right).
    Elevation,
}

/// The C++ `toString(ViewKind)`.
impl EnumName for ViewKind {
    fn name(self) -> &'static str {
        match self {
            Self::Plan => "Plan",
            Self::Model3D => "3D",
            Self::Section => "Section",
            Self::Elevation => "Elevation",
        }
    }
}

/// The standard splits. Named for what they give you rather than for the
/// geometry, because that is how they are chosen from a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutKind {
    Single,
    /// Two side by side.
    SplitVertical,
    /// Two stacked.
    SplitHorizontal,
    /// One tall on the left, two stacked on the right.
    ThreeLeft,
    /// One wide on top, two side by side beneath.
    ThreeTop,
    /// Four equal: the plan/3D/section/elevation arrangement.
    Quad,
}

/// The C++ `toString(LayoutKind)`.
impl EnumName for LayoutKind {
    fn name(self) -> &'static str {
        match self {
            Self::Single => "Single",
            Self::SplitVertical => "Two: Vertical",
            Self::SplitHorizontal => "Two: Horizontal",
            Self::ThreeLeft => "Three: Left",
            Self::ThreeTop => "Three: Top",
            Self::Quad => "Four: Equal",
        }
    }
}

/// How many views the preset arranges. Always >= 1.
pub fn cell_count(kind: LayoutKind) -> usize {
    layout_rects(kind).len()
}

/// A view's place in a preset, in normalised [0, 1] coordinates of the
/// drawing area, with the origin at the TOP-LEFT to match screen convention.
/// Half-open, so a point on a shared border belongs to exactly one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Default for CellRect {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }
}

impl CellRect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(&self, u: f64, v: f64) -> bool {
        // Half-open in each direction; a coordinate that is not a number is
        // in neither range, as in the C++'s comparisons.
        (self.x..self.x + self.width).contains(&u) && (self.y..self.y + self.height).contains(&v)
    }
}

/// The rectangles of a preset, top-left origin, in normalised coordinates, in
/// the order its views are placed.
pub fn layout_rects(kind: LayoutKind) -> Vec<CellRect> {
    match kind {
        LayoutKind::Single => vec![CellRect::new(0.0, 0.0, 1.0, 1.0)],
        LayoutKind::SplitVertical => vec![
            CellRect::new(0.0, 0.0, 0.5, 1.0),
            CellRect::new(0.5, 0.0, 0.5, 1.0),
        ],
        LayoutKind::SplitHorizontal => vec![
            CellRect::new(0.0, 0.0, 1.0, 0.5),
            CellRect::new(0.0, 0.5, 1.0, 0.5),
        ],
        LayoutKind::ThreeLeft => vec![
            CellRect::new(0.0, 0.0, 0.5, 1.0),
            CellRect::new(0.5, 0.0, 0.5, 0.5),
            CellRect::new(0.5, 0.5, 0.5, 0.5),
        ],
        LayoutKind::ThreeTop => vec![
            CellRect::new(0.0, 0.0, 1.0, 0.5),
            CellRect::new(0.0, 0.5, 0.5, 0.5),
            CellRect::new(0.5, 0.5, 0.5, 0.5),
        ],
        LayoutKind::Quad => vec![
            CellRect::new(0.0, 0.0, 0.5, 0.5),
            CellRect::new(0.5, 0.0, 0.5, 0.5),
            CellRect::new(0.0, 0.5, 0.5, 0.5),
            CellRect::new(0.5, 0.5, 0.5, 0.5),
        ],
    }
}

// The kind each slot of an arrangement opens. Ordered so that the first is
// always the plan view the user was already working in, and the four-up
// arrangement is the one a civil engineer expects: plan, 3D, section,
// elevation.
const DEFAULT_KINDS: [ViewKind; 4] = [
    ViewKind::Plan,
    ViewKind::Model3D,
    ViewKind::Section,
    ViewKind::Elevation,
];

/// The kind a view gets when an arrangement needs one more than are open:
/// plan, 3D, section, elevation - the four-up arrangement a civil engineer
/// expects. Slots past the fourth repeat the last.
pub fn default_view_kind(slot: usize) -> ViewKind {
    DEFAULT_KINDS[slot.min(DEFAULT_KINDS.len() - 1)]
}

/// Points a camera the way a fresh view of `kind` looks: a 3D view in
/// perspective from the south-west, an elevation orthographic from the front,
/// a plan orthographic from the top. A Section leaves the camera alone, so
/// switching back to the model kind the camera was set up for restores what
/// it had (ViewSet::set_kind, ViewState::camera_kind).
pub fn configure_camera(camera: &mut Camera, kind: ViewKind) {
    match kind {
        ViewKind::Model3D => {
            camera.set_projection(Projection::Perspective);
            camera.set_standard_view(StandardView::IsoSouthWest);
        }
        ViewKind::Elevation => {
            // Orthographic, because a side view is read off with a scale rule
            // and perspective would make that a lie.
            camera.set_projection(Projection::Orthographic);
            camera.set_standard_view(StandardView::Front);
        }
        ViewKind::Plan => {
            camera.set_projection(Projection::Orthographic);
            camera.set_standard_view(StandardView::Top);
        }
        ViewKind::Section => {
            // A section has its own 2D station/elevation transform; the
            // camera is left as-is so switching back to a model view restores
            // what it had.
        }
    }
}

/// How an arrangement is built out of docked views (the dock workspace of
/// 2026-09-23). View 0 is placed first; each step then splits a view already
/// placed in two, putting the next view to the right of it or below it, each
/// half the size. Applying the steps to rectangles reproduces
/// layout_rects(kind) exactly - a test asserts it - and the window applies the
/// same steps when it splits its docks, so the menu's picture of "Three:
/// Left" and the docks it produces cannot drift apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DockSplit {
    /// Index of a view already placed.
    pub existing: usize,
    /// Index of the view this step places.
    pub added: usize,
    /// True: `added` to the right; false: below.
    pub side_by_side: bool,
}

impl Default for DockSplit {
    fn default() -> Self {
        Self {
            existing: 0,
            added: 0,
            side_by_side: true,
        }
    }
}

impl DockSplit {
    pub const fn new(existing: usize, added: usize, side_by_side: bool) -> Self {
        Self {
            existing,
            added,
            side_by_side,
        }
    }
}

/// Steps for every view after the first, in order; empty for Single.
pub fn dock_splits(kind: LayoutKind) -> Vec<DockSplit> {
    // Each step halves `existing`, so the order matters: Quad splits the top
    // row first and then each half downwards, which is what gives four equal
    // quarters rather than one half and two quarters.
    match kind {
        LayoutKind::Single => Vec::new(),
        LayoutKind::SplitVertical => vec![DockSplit::new(0, 1, true)],
        LayoutKind::SplitHorizontal => vec![DockSplit::new(0, 1, false)],
        LayoutKind::ThreeLeft => vec![DockSplit::new(0, 1, true), DockSplit::new(1, 2, false)],
        LayoutKind::ThreeTop => vec![DockSplit::new(0, 1, false), DockSplit::new(1, 2, true)],
        LayoutKind::Quad => vec![
            DockSplit::new(0, 1, true),
            DockSplit::new(0, 2, false),
            DockSplit::new(1, 3, false),
        ],
    }
}
