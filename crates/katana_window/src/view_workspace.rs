//! The drawing views as docks: the port of view_workspace (the user's request of 2026-09-23 for
//! views that dock, move, minimise, close and go out of the window).
//!
//! Each view is a dock of the workspace's own dock area, and its state (katana_cad's ViewState:
//! camera, plan zoom, hidden layers, section) lives in the ViewSet for as long as the view is open.
//! A dock holds one of three views - the plan view (viewport_widget), the 3D and elevation view
//! (render_view_widget) and the section view (section_view_widget) - and only a plan view edits.
//! The window's grid and snap settings apply to EVERY plan view, and are kept so that a view
//! opened later matches; a TOOL holds picks made in one view, so it runs in the active plan view
//! alone (`start_tool`), and starting one stops any other.
//!
//! THE RUST SHAPE. The C++ workspace is a nested QMainWindow holding a `Document&`, the window's
//! reference data and scene lists by pointer, a command runner and the std::function hooks the
//! window sets. Here:
//!
//! * **The drawing is lent** for each call that needs it: `&mut dyn PlanViewHost` (the document
//!   with the window's reference data and meshes; a Document alone is one) for what only touches
//!   the drawing, `&mut dyn Host` (the window's one executor) for what runs a line, and `Scenery`
//!   (the window's surfaces and meshes) where a 3D view or a plan view's mesh footprints are drawn
//!   or framed.
//! * **The views, their docks and their bars are shared** with the verbs' host (`Rc`), as the
//!   C++ workspace is the interpreter's ViewVerbHost: VIEWS and ZOOM reach the views while the
//!   window's executor runs a line that a bar of this very workspace asked for. Everything the
//!   workspace keeps is behind one RefCell that no call holds across a line.
//! * **A line that reaches the views is run through `WorkspaceLines::run_line`**, which the
//!   window's executor calls instead of the interpreter's own `run`. The verbs run with the
//!   Document lent to the interpreter, so the host cannot read it in the middle of a ZOOM - and a
//!   zoom frames what a view draws. So a ZOOM is first run on a probe interpreter over a copy of
//!   the views, which records the zoom it asks for; the zoom is then made with the drawing lent;
//!   and the line is run for real, its host answering with what the zoom moved. The reply is the
//!   C++'s, the line is in the history once, and the document is never edited twice (ZOOM edits
//!   nothing). Rejected: the zoom deferred to the next frame (its reply would say where the view
//!   was, not where it went); what each view draws worked out before every line (a 3D view's
//!   scope zoom needs the entities the scope took, which only the verb knows).
//! * **The hooks are events** (`WorkspaceEvent`, `take_events`), read by the window after a call,
//!   and the two hooks whose being SET changes what the workspace does are flags: with no shortcut
//!   menu taker a right-click cancels (`set_context_menu_taker`).
//! * **The dock area is the workspace's own**: an egui_dock DockState, laid out by egui_dock's own
//!   split arithmetic and drawn here, a tab strip only over a group of more than one view, and the
//!   chrome's bar (dock_chrome) on each. egui_dock's DockArea was rejected for the views: it draws
//!   a 24-point tab bar over every dock, where the C++ views have only their 26-point title bar
//!   (the measured headless geometry, docs/rust.md "Step 7"), with its own close and add buttons,
//!   and its drags would put a view among the main window's panels, which the nested window was
//!   made to prevent. A view floats as a window inside the main window.
//!
//! THE CHROME is the workspace's own `DockChrome<ViewTab>`: the C++ shares the main window's, so
//! one tray holds a minimised panel and a minimised view, and the window draws the workspace's
//! tray buttons beside its own (`tray_actions`, `restore_view`).
//!
//! A FLOATING VIEW is a window inside the workspace at a rectangle the chrome keeps for it
//! (`DockChrome::float_rect`), in points from the workspace's top left - so it holds while a
//! panel beside the views is hidden, and is what a saved session records. Float lifts a view off
//! its docked place; its title bar drags it; Maximise fills the workspace with it.
//!
//! A VIEW'S TITLE BAR HAS A SHORTCUT MENU (`BAR_MENU`): what its window buttons do, and the three
//! tilings, each going the way the button or the View menu's item goes (`trigger_bar_menu`).
//!
//! THE VIEWS BETWEEN SESSIONS (`saved_views`, `restore_views`; docs/desktop.md, "The views
//! between sessions"): which views are open, under the ids they had, what each shows, the link
//! and the arrangement. What a view looks AT - its zoom, its camera, the layers it hides, its
//! section - is a drawing's and is not kept with the window's.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Align, Layout, Pos2, Rect, Sense, Ui, UiBuilder, Vec2};
use egui_dock::{DockState, Node, NodeIndex, TabPath, Tree};
use katana_cad::interactive_tool::{ToolInput, tool_catalog};
use katana_cad::scene::{MeshPlanDisplay, SceneMesh, SceneOptions, SceneSurface};
use katana_cad::scope_verbs::{ScopeViewFn, scope_view_of};
use katana_cad::section::{Section, SectionDesign};
use katana_cad::view_link::linkable;
use katana_cad::view_set::{NO_VIEW, ViewId, ViewSet, ViewState, restore_view_paper_scales};
use katana_cad::view_verbs::{
    TileOutcome, ViewVerbHost, ZoomRequest, ZoomRequestKind, apply_plan_zoom, zoom_takes,
};
use katana_cad::viewport_layout::{
    LayoutKind, TileKind, ViewKind, cell_count, default_view_kind, dock_splits, tile_plan,
};
use katana_cad::{CommandInterpreter, Document, ListenerHandle, SnapModes, SnapResult};
use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::EntityId;
use katana_entity::EnumName;
use katana_geometry::{Box2, Point2, Polyline2};
use katana_interop::ReferenceData;

use crate::canvas::PointF;
use crate::dock_chrome::{
    ALWAYS, BAR_HEIGHT, BarFacts, ChromeIcons, ChromeRequest, DEFAULT_FLOAT, DockChrome, DockRole,
    DockTab, DockTitleBar, TitleBarButton, WindowButton,
};
use crate::drive::action::{Action, Menu, MenuItem};
use crate::drive::registry::{TargetKind, TargetSpec, begin_target, end_target};
use crate::host::{Host, PickId, VerbOutcome, ViewAccess};
use crate::render_view_widget::{GpuSession, RenderScene, RenderView, RenderViewEvent};
use crate::section_view_widget::{SectionView, SectionViewEvent};
use crate::theme::palette;
use crate::tools::tool_host::is_transparent_command;
use crate::view_focus::{FocusPrevious, FocusReason};
use crate::view_layers_popup::{ViewLayersPopup, view_layers_line, view_references_line};
use crate::view_scale_dialog::ViewScaleDialog;
use crate::viewport_widget::{
    INITIAL_SCALE, KeyModifiers, PlanView, PlanViewEvent, PlanViewHooks, PlanViewHost, Tool,
    ViewKey,
};

/// The gap between two docked views, in points: the design's 4 (2026-11-16), the gap between the
/// main window's docks too, so every seam of the window is one width. The C++ window's was 5,
/// measured from its headless screenshots (docs/rust.md, "The headless window has the C++'s
/// geometry": two views split the 660 points of the views' area 327 | 328).
pub const SEPARATOR: f32 = 4.0;
/// The tab strip over a group of tabbed views (QTabBar's height in the window's style), at the
/// top: tabs above the views they switch between, where the eye looks for a title.
pub const TAB_STRIP: f32 = 24.0;
/// The kind switcher's width: the 20 point button and 12 for the arrow's box beside the icon, as
/// the C++'s 22 and 12 - so its icon sits where every other title bar button's does (3, 3), and its
/// arrow 3 points clear of it.
const KIND_BUTTON_WIDTH: f32 = 32.0;
/// The optional zoom tools' priorities on a view's bar: Zoom to Selection outlasts In and Out,
/// which the wheel does as well.
const ZOOM_IN_OUT_PRIORITY: i32 = 1;
const ZOOM_SELECTION_PRIORITY: i32 = 2;
/// Also on the main View toolbar, so this duplicate yields before the zoom tools.
const PAPER_SCALE_PRIORITY: i32 = 0;

/// The kinds the switcher offers, in its order.
const KINDS: [ViewKind; 4] = [
    ViewKind::Plan,
    ViewKind::Model3D,
    ViewKind::Section,
    ViewKind::Elevation,
];

fn is_render_kind(kind: ViewKind) -> bool {
    kind == ViewKind::Model3D || kind == ViewKind::Elevation
}

/// The icon a view's kind is drawn with: its bar's and its switcher's (the C++ Icon enumerators).
fn kind_icon(kind: ViewKind) -> &'static str {
    match kind {
        ViewKind::Plan => "ViewPlan",
        ViewKind::Model3D => "View3D",
        ViewKind::Section => "ViewSection",
        ViewKind::Elevation => "ViewElevation",
    }
}

/// The tilings, where they are offered - View > Viewport Layout, the Window menu and a view's
/// title bar menu: kind, action name, icon, tip. One table, so the three places show one action;
/// each runs VIEWS TILE with the kind's word (`tiling_line`), as a typed line would.
pub const TILINGS: [(TileKind, &str, &str, &str); 3] = [
    (
        TileKind::Grid,
        "viewTile",
        "LayoutTile",
        "Give every open view a place of its own of equal size, in a grid; opens and closes none \
         (VIEWS TILE)",
    ),
    (
        TileKind::Columns,
        "viewTileSideBySide",
        "LayoutTileColumns",
        "Put the open views side by side, each the same width; opens and closes none (VIEWS TILE \
         COLUMNS)",
    ),
    (
        TileKind::Rows,
        "viewTileStacked",
        "LayoutTileRows",
        "Stack the open views one above the other, each the same height; opens and closes none \
         (VIEWS TILE ROWS)",
    ),
];

/// The line a tiling's item hands the window's executor: None for a name that is no tiling's.
pub fn tiling_line(action: &str) -> Option<String> {
    TILINGS
        .iter()
        .find(|(_, name, ..)| *name == action)
        .map(|(kind, ..)| format!("VIEWS TILE {}", kind.word().to_ascii_uppercase()))
}

/// The tilings as a menu's rows, in the table's order.
pub fn tiling_actions() -> Vec<Action> {
    TILINGS
        .into_iter()
        .map(|(kind, name, icon, tip)| Action::new(name, kind.name()).icon(icon).tip(tip))
        .collect()
}

/// A view's title bar menu as a drive target: a step presses one of its items in it.
pub const BAR_MENU: &str = "ViewBarMenu";
/// Its own items, by object name: what the bar's window buttons do, for the view it was opened on.
pub const BAR_MENU_FLOAT: &str = "viewBarFloat";
pub const BAR_MENU_MINIMISE: &str = "viewBarMinimise";
pub const BAR_MENU_MAXIMISE: &str = "viewBarMaximise";
pub const BAR_MENU_CLOSE: &str = "viewBarClose";

/// The most views a saved session may name. More than anyone arranges on purpose - the tilings'
/// own cases stop at twelve, and thirty-two tiled in the headless window's 794 point drawing area
/// are six across, 129 points each, less than a view's bar needs for its always-shown buttons -
/// and few enough that a damaged or hand-edited record cannot make the start open docks by the
/// thousand, each with a widget and, in 3D, a scene of its own.
pub const MOST_SAVED_VIEWS: usize = 32;
/// The version `saved_views` writes; `restore_views` reads no other.
pub const SAVED_VIEWS_VERSION: i64 = 1;
/// The least a floating view is: room for its bar's always-shown buttons and a little drawing
/// under it. A saved rectangle smaller than this is a damaged one.
const LEAST_FLOAT: Vec2 = Vec2::new(120.0, 80.0);
/// How much of a floating view's title bar stays inside the workspace when it is dragged or a
/// saved rectangle is brought into a smaller window: enough of the title to press and drag back.
const FLOAT_REACH: f32 = 60.0;
/// How far a view lifts off its docked place when it is floated: a title bar's height down and
/// across, so the place it left shows behind it.
const FLOAT_LIFT: f32 = BAR_HEIGHT;

/// `rect`, a floating view's in the workspace's own points, kept where its title bar can be
/// reached in a workspace of `room`: its top inside, and FLOAT_REACH of its bar inside across.
/// The rectangle keeps its size; a workspace with no room yet keeps it as it is.
fn kept_in_reach(rect: Rect, room: Vec2) -> Rect {
    if !room.x.is_finite() || !room.y.is_finite() || room.x <= 0.0 || room.y <= 0.0 {
        return rect;
    }
    let reach = FLOAT_REACH.min(rect.width());
    let x = rect
        .min
        .x
        .clamp(reach - rect.width(), (room.x - reach).max(reach - rect.width()));
    let y = rect.min.y.clamp(0.0, (room.y - BAR_HEIGHT).max(0.0));
    Rect::from_min_size(Pos2::new(x, y), rect.size())
}

/// Where a view docked at `place` floats when its Float lifts it off: the same size less the
/// lift, FLOAT_LIFT down and across from its corner, so it stays over the place it left and is
/// seen to have come out of it.
fn lifted(place: Rect) -> Rect {
    let size = Vec2::new(
        (place.width() - FLOAT_LIFT).max(LEAST_FLOAT.x),
        (place.height() - FLOAT_LIFT).max(LEAST_FLOAT.y),
    );
    Rect::from_min_size(place.min + Vec2::splat(FLOAT_LIFT), size)
}

/// A view's dock object name: what the saved layout finds it by, and its drive target.
pub fn dock_name(id: ViewId) -> String {
    format!("View{id}")
}

/// The egui Area view `id` floats in. The main window names the same Area to keep its own
/// floating panels over a floating view that fills the workspace (`docking_ui`), so the id has
/// this one definition.
pub fn floating_view_area(id: ViewId) -> egui::Id {
    egui::Id::new(("katanaFloatingView", id))
}

fn dock_id(name: &str) -> Option<ViewId> {
    name.strip_prefix("View").and_then(|id| id.parse().ok())
}

fn no_such_view(id: ViewId) -> Error {
    make_error(ErrorCode::NotFound, "no such view", format!("{id}"))
}

/// The ZOOM line a transparent command typed while a tool runs stands for - ZOOM, Z, 'ZOOM or 'Z
/// and whatever follows - or "" for one ZOOM is not (PAN has no verb: the tool host hands it over
/// all the same). Which words are ZOOM is the interpreter's reading (verb_of: its alias table and
/// its apostrophe rule), never a list of this file's.
pub fn transparent_zoom_line(typed: &str) -> String {
    let text = typed.trim();
    if CommandInterpreter::verb_of(text) != "ZOOM" {
        return String::new();
    }
    match text.find(' ') {
        Some(blank) => format!("ZOOM{}", &text[blank..]),
        None => String::from("ZOOM"),
    }
}

/// `line` for view `id`, unless it names a view of its own. A ZOOM typed at a tool's prompt is for
/// the view the tool picks its points in: without its view= it zoomed the ACTIVE view, which a
/// click on another view's bar or drawing had made another.
pub fn for_view(line: &str, id: ViewId) -> String {
    let named = line.split(' ').filter(|word| !word.is_empty()).any(|word| {
        word.len() >= 5 && word.is_char_boundary(5) && word[..5].eq_ignore_ascii_case("view=")
    });
    if named {
        String::from(line)
    } else {
        format!("{line} view={id}")
    }
}

/// A view's dock in the workspace's dock area: its object name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewTab(pub String);

impl DockTab for ViewTab {
    fn dock_name(&self) -> &str {
        &self.0
    }
}

/// What the window lends its views beside the drawing: the surfaces and meshes it keeps, one list
/// for every view (the C++'s setSurfaces and setMeshes, borrowed for the call).
#[derive(Clone, Copy, Default)]
pub struct Scenery<'s> {
    pub surfaces: &'s [SceneSurface<'s>],
    pub meshes: &'s [SceneMesh<'s>],
}

/// What the C++ workspace raised through its hooks, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum WorkspaceEvent {
    /// A view's message for the person (onStatus).
    Status(String),
    /// A view's own running readout (onFrameStats): a 3D view's frame time, a section's station.
    FrameStats(String),
    /// The active view changed.
    ActiveChanged,
    /// A view opened, closed or changed kind: the Window menu lists them.
    ViewsChanged,
    /// The running tool's prompt.
    Prompt(String),
    Error(String),
    CursorMoved {
        world: Point2,
        snap: Option<SnapResult>,
    },
    /// A tool started (its catalogue id) or ended ("") in any plan view.
    ActiveToolChanged(String),
    /// What a running tool reports that is not an error (onToolMessage; the window shows it as a
    /// status when it has no place of its own for it).
    ToolMessage(String),
    /// Printable text typed into a plan view with no tool running ("type anywhere").
    TextTyped(String),
    /// A plan view's shortcut menu, asked for with no tool running, at this pixel of that view.
    ContextMenu { view: ViewId, at: PointF },
    /// A double click on an entity in a plan view with no tool running.
    EntityDoubleClicked(EntityId),
    /// The link changed: the View menu's check mark.
    LinksChanged,
    /// A view's own settings changed (its hidden layers, its ghosts).
    ViewSettingsChanged,
}

/// What `recut_sections` did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionRecut {
    /// The views whose section was cut again.
    pub cut: Vec<ViewId>,
    /// The views whose section could not be, and why. Each keeps the section it had, which says it
    /// is of surfaces that have changed (SectionView::surfaces_changed).
    pub refused: Vec<(ViewId, Error)>,
}

/// One open view: its id and the widget its kind is drawn by.
struct View {
    id: ViewId,
    plan: Option<PlanView>,
    render: Option<RenderView>,
    section: Option<SectionView>,
}

/// How the verbs' host answers (see the module comment).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum HostMode {
    /// The views themselves.
    #[default]
    Direct,
    /// A ZOOM probed on a copy of the views: the zoom it asks for is recorded, and refused.
    Record,
    /// The ZOOM for real: the zoom was made, and the host answers with what it moved.
    Replay,
}

/// The views' bars and the chrome, the layout and the views' widgets: everything of the workspace
/// a call changes, behind one RefCell (see the module comment).
struct Inner {
    docks: Vec<View>,
    layout: DockState<ViewTab>,
    chrome: Option<DockChrome<ViewTab>>,
    /// The views' area as last drawn, in points; None before the first draw.
    area: Option<Rect>,
    pixels_per_point: f32,
    grid_visible: bool,
    options: SceneOptions<'static>,
    gpu: GpuSession,
    last_tool_id: String,
    context_menu_taker: bool,
    /// True while view_moved is moving the link: a view moved by it never reports that move.
    following: bool,
    paper_scale_problem: String,
    scale_dialog: Option<ViewScaleDialog>,
    /// A view whose kind switcher's menu is open, and a kind chosen there, changed after the
    /// frame.
    kind_menu: Option<ViewId>,
    kind_choice: Option<(ViewId, ViewKind)>,
    /// The view whose kind menu a click outside it closed this frame (draw_kind_menu).
    kind_menu_dismissed: Option<ViewId>,
    /// The pointer as the last frame saw it, in the window's points: where a bar's shortcut menu
    /// opens and a floating view's drag starts, both asked once the frame is drawn.
    pointer: Option<Pos2>,
    /// A floating view being dragged by its title bar, and the pointer's offset from its corner.
    float_drag: Option<(ViewId, Vec2)>,
    /// The view whose title bar's shortcut menu is open, and where (the window's points); an
    /// item chosen there, carried out after the frame.
    bar_menu: Option<(ViewId, Pos2)>,
    bar_menu_choice: Option<(ViewId, String)>,
    events: Vec<WorkspaceEvent>,
    /// Lines a bar or a transparent zoom asked for, run through the window's executor once
    /// nothing of the workspace is borrowed.
    lines: Rc<RefCell<Vec<String>>>,
    /// Views opened by a verb, whose widget is made at the next call lent the drawing.
    unbuilt: Vec<ViewId>,
    /// A view event that needs the drawing, met where none was lent (a dialog's zoom through
    /// ViewAccess): handled at the next call lent it.
    held_events: Vec<(ViewId, PlanViewEvent)>,
    zoom_extents_all_asked: bool,
    picks: BTreeMap<PickId, Option<Option<Point2>>>,
    pick_waiting: Option<PickId>,
    next_pick: PickId,
    /// The reference layers the window has, by id, as last lent: what has_reference answers and
    /// what a Layers button counts.
    references: BTreeSet<u64>,
    /// Views pinned to a size of their own whatever their dock's (QWidget::setFixedSize).
    fixed: BTreeMap<ViewId, (i32, i32)>,
    /// How many moves each plan view has reported to the workspace.
    reported: BTreeMap<ViewId, usize>,
}

/// The workspace's state, shared with the verbs' host (see the module comment).
struct Shared {
    views: Rc<RefCell<ViewSet>>,
    /// The copy a ZOOM is probed on.
    scratch: RefCell<ViewSet>,
    mode: Cell<HostMode>,
    recorded: RefCell<Vec<ZoomRequest>>,
    replies: RefCell<VecDeque<Result<Vec<ViewId>>>>,
    inner: RefCell<Inner>,
    /// The drawing has notified since the paper scales were last restored.
    notified: Arc<AtomicBool>,
    /// Kept for its life: dropped, the drawing stops telling the workspace.
    _registration: ListenerHandle,
    popup: RefCell<Option<ViewLayersPopup>>,
}

/// A recorded zoom's refusal: the probe stops at the zoom.
fn deferred() -> Error {
    make_error(
        ErrorCode::InvalidState,
        "the zoom is made with the drawing lent",
        "",
    )
}

/// The drawing lent through the window's executor, with the meshes it keeps.
struct HostDrawing<'l, 'h> {
    host: &'l mut (dyn Host + 'h),
    meshes: &'l [SceneMesh<'l>],
}

impl PlanViewHost for HostDrawing<'_, '_> {
    fn document(&self) -> &Document {
        self.host.document()
    }

    fn document_mut(&mut self) -> &mut Document {
        self.host.document_mut()
    }

    fn reference(&self) -> Option<&ReferenceData> {
        Some(self.host.reference())
    }

    fn meshes(&self) -> &[SceneMesh<'_>] {
        self.meshes
    }
}

// ---- the layout ----------------------------------------------------------------------------------

/// `value` on the device pixel grid, as egui_dock rounds a split's borders.
fn on_pixel(value: f32, pixels_per_point: f32) -> f32 {
    (value * pixels_per_point).round() / pixels_per_point
}

/// The fraction that splits `extent` into two halves with the separator between - the first
/// (extent - separator) / 2 rounded down, the second what is left - by egui_dock's arithmetic,
/// whose borders are the middle less and plus half the separator: the C++'s resizeDocks in pixels,
/// so that the other docks of the row keep theirs.
fn half_fraction(extent: f32) -> f32 {
    equal_fraction(extent, 2)
}

/// The fraction that cuts the first of `parts` equal places off `extent`, a separator between each
/// two: the first (extent - the separators) / parts rounded down, the rest left for the cuts that
/// follow, each worked from what is left - so the places of a row differ by a point at most. The
/// bare 1 / parts takes half a separator off each side of every cut, which each place cut later
/// pays again: of five across W points the first is W / 5 - 2 and the last W / 5 - 4.17, by
/// egui_dock's arithmetic (leaf_rects), and the gap grows with the count. An extent with no room
/// for a point each, or none known yet, is cut by 1 / parts.
fn equal_fraction(extent: f32, parts: usize) -> f32 {
    let count = parts.max(1) as f32;
    let gaps = SEPARATOR * (count - 1.0);
    if parts < 2 || !extent.is_finite() || extent <= gaps + count {
        return 1.0 / count;
    }
    let first = ((extent - gaps) / count).floor().max(1.0);
    ((first + SEPARATOR / 2.0) / extent).clamp(0.0, 1.0)
}

/// Every leaf of `tree` laid out in `area`, by egui_dock's own split arithmetic.
fn leaf_rects(tree: &Tree<ViewTab>, area: Rect, pixels_per_point: f32) -> Vec<(NodeIndex, Rect)> {
    let mut out = Vec::new();
    let mut stack = vec![(NodeIndex::root(), area)];
    while let Some((index, rect)) = stack.pop() {
        if index.0 >= tree.len() {
            continue;
        }
        match &tree[index] {
            Node::Leaf(_) => out.push((index, rect)),
            Node::Horizontal(split) => {
                let middle = rect.min.x + rect.width() * split.fraction;
                let left = on_pixel(middle - SEPARATOR * 0.5, pixels_per_point);
                let right = on_pixel(middle + SEPARATOR * 0.5, pixels_per_point);
                stack.push((
                    index.right(),
                    Rect::from_min_max(Pos2::new(right, rect.min.y), rect.max),
                ));
                stack.push((
                    index.left(),
                    Rect::from_min_max(rect.min, Pos2::new(left, rect.max.y)),
                ));
            }
            Node::Vertical(split) => {
                let middle = rect.min.y + rect.height() * split.fraction;
                let top = on_pixel(middle - SEPARATOR * 0.5, pixels_per_point);
                let bottom = on_pixel(middle + SEPARATOR * 0.5, pixels_per_point);
                stack.push((
                    index.right(),
                    Rect::from_min_max(Pos2::new(rect.min.x, bottom), rect.max),
                ));
                stack.push((
                    index.left(),
                    Rect::from_min_max(rect.min, Pos2::new(rect.max.x, top)),
                ));
            }
            Node::Empty => {}
        }
    }
    out.sort_by_key(|(index, _)| index.0);
    out
}

/// Where a docked view is in the dock area: its leaf's rectangle (the whole dock, its bar
/// included) less the tab strip of a group, and whether it is the page shown.
#[derive(Clone, Copy, Debug)]
struct Placed {
    rect: Rect,
    shown: bool,
}

// ---- Inner: the views, their docks and their bars ------------------------------------------------

impl Inner {
    fn find(&self, id: ViewId) -> Option<&View> {
        self.docks.iter().find(|view| view.id == id)
    }

    fn find_mut(&mut self, id: ViewId) -> Option<&mut View> {
        self.docks.iter_mut().find(|view| view.id == id)
    }

    fn path_of(&self, id: ViewId) -> Option<TabPath> {
        let name = dock_name(id);
        self.layout.find_tab_from(|tab| tab.0 == name)
    }

    /// In the dock area at all: not minimised, and not hidden by a maximise.
    fn shown(&self, id: ViewId) -> bool {
        self.path_of(id).is_some()
    }

    fn floating(&self, id: ViewId) -> bool {
        self.path_of(id).is_some_and(|path| !path.surface.is_main())
    }

    /// Where a floating view is, in points from the workspace's top left - the chrome's record
    /// of it; None for a view that does not float.
    fn float_rect(&self, id: ViewId) -> Option<Rect> {
        if !self.floating(id) {
            return None;
        }
        let name = dock_name(id);
        Some(
            self.chrome
                .as_ref()
                .and_then(|chrome| chrome.float_rect(&name))
                .unwrap_or(DEFAULT_FLOAT),
        )
    }

    /// The workspace's size as last drawn; nothing before the first draw.
    fn room(&self) -> Vec2 {
        self.area.map_or(Vec2::ZERO, |area| area.size())
    }

    /// Where a view's dock is, docked or floating, in points from the workspace's top left: what
    /// a menu or a popup of its bar is placed by. None for a view that is not in the dock area,
    /// or before the first draw.
    fn place_of(&self, id: ViewId) -> Option<Rect> {
        self.float_rect(id)
            .or_else(|| self.placements().get(&id).map(|placed| placed.rect))
    }

    /// The Float button: a docked view lifted off its place into a window over it, or a floating
    /// one docked back where it came from.
    fn toggle_floating(&mut self, views: &RefCell<ViewSet>, id: ViewId) {
        let name = dock_name(id);
        // A maximise ends first, so the place a view lifts off is its own and not the whole
        // workspace it had filled.
        if let Some(chrome) = &mut self.chrome {
            let _ = chrome.unmaximise(&mut self.layout, &name);
        }
        let place = self.placements().get(&id).map(|placed| placed.rect);
        let room = self.room();
        if let Some(chrome) = &mut self.chrome {
            chrome.toggle_floating(&mut self.layout, &name);
            // The chrome knows nothing of where the dock stood: floating now, it is put over the
            // place it left.
            if let Some(place) = place {
                let _ = chrome.set_float_rect(&name, kept_in_reach(lifted(place), room));
            }
        }
        self.float_drag = None;
        self.relayout(views);
    }

    /// The pointer as this frame has it: a floating view being dragged by its title bar goes
    /// with it while the button is held - its bar kept where it can be reached - and stays where
    /// it is when the button is let go.
    fn drag_floating(&mut self, pointer: Option<Pos2>, down: bool) {
        let Some((id, grip)) = self.float_drag else {
            return;
        };
        match (pointer, self.area, self.float_rect(id)) {
            (Some(pointer), Some(area), Some(rect)) => {
                let corner = pointer - grip - area.min.to_vec2();
                let moved = kept_in_reach(Rect::from_min_size(corner, rect.size()), area.size());
                if let Some(chrome) = &mut self.chrome {
                    let _ = chrome.set_float_rect(&dock_name(id), moved);
                }
            }
            // Docked or closed meanwhile, or the pointer gone: there is nothing to drag.
            _ => self.float_drag = None,
        }
        if !down {
            self.float_drag = None;
        }
    }

    /// A press on a floating view's title bar picks it up, by where on the bar the pointer is.
    /// A maximised view fills the workspace and is not moved.
    fn begin_float_drag(&mut self, id: ViewId) {
        let maximised = self
            .chrome
            .as_ref()
            .is_some_and(|chrome| chrome.is_maximised(&dock_name(id)));
        self.float_drag = match (self.pointer, self.area, self.float_rect(id)) {
            (Some(pointer), Some(area), Some(rect)) if !maximised => {
                Some((id, pointer - (area.min + rect.min.to_vec2())))
            }
            _ => None,
        };
    }

    /// The docked views laid out in the area as last drawn.
    fn placements(&self) -> BTreeMap<ViewId, Placed> {
        let mut out = BTreeMap::new();
        let Some(area) = self.area else {
            return out;
        };
        let area = Rect::from_min_size(Pos2::ZERO, area.size());
        let tree = self.layout.main_surface();
        if tree.is_empty() {
            return out;
        }
        for (index, rect) in leaf_rects(tree, area, self.pixels_per_point) {
            let Node::Leaf(leaf) = &tree[index] else {
                continue;
            };
            let tabbed = leaf.tabs.len() > 1;
            let mut dock = rect;
            if tabbed {
                dock.min.y = (dock.min.y + TAB_STRIP).min(dock.max.y);
            }
            for (at, tab) in leaf.tabs.iter().enumerate() {
                if let Some(id) = dock_id(&tab.0) {
                    out.insert(
                        id,
                        Placed {
                            rect: dock,
                            shown: at == leaf.active.0,
                        },
                    );
                }
            }
        }
        out
    }

    /// Where the person can see it: in the dock area, and floating or the page of its group shown.
    fn on_screen(&self, id: ViewId) -> bool {
        if self.floating(id) {
            return true;
        }
        self.placements().get(&id).is_some_and(|placed| placed.shown)
    }

    /// The size each docked plan and section view has in the area as last drawn, given at once:
    /// what a split or an arrangement left a view is what the next zoom of it frames, with no frame
    /// drawn between (the C++'s sendPostedEvents of the layout).
    fn relayout(&mut self, views: &RefCell<ViewSet>) {
        // A floating view's dock is its window: sized as a docked one is, by its rectangle.
        let mut places: BTreeMap<ViewId, Rect> = self
            .placements()
            .into_iter()
            .map(|(id, placed)| (id, placed.rect))
            .collect();
        for view in &self.docks {
            if let Some(rect) = self.float_rect(view.id) {
                places.insert(view.id, rect);
            }
        }
        let mut set = views.borrow_mut();
        for view in &mut self.docks {
            let (width, height) = match (self.fixed.get(&view.id), places.get(&view.id)) {
                (Some(&fixed), _) => fixed,
                (None, Some(at)) => (
                    at.width().round() as i32,
                    (at.height() - BAR_HEIGHT).round().max(1.0) as i32,
                ),
                (None, None) => continue,
            };
            let Some(state) = set.find_mut(view.id) else {
                continue;
            };
            if let Some(plan) = &mut view.plan
                && plan.size() != (width, height)
            {
                plan.resize(state, width, height);
            }
            if let Some(section) = &mut view.section {
                section.resize(state, width, height);
            }
            if let Some(render) = &mut view.render {
                render.take_layout_size(width, height);
            }
        }
    }

    fn push(&mut self, event: WorkspaceEvent) {
        self.events.push(event);
    }

    // ---- the bars --------------------------------------------------------------------------

    fn bar_mut(&mut self, id: ViewId) -> Option<&mut DockTitleBar> {
        self.chrome
            .as_mut()
            .and_then(|chrome| chrome.title_bar_mut(&dock_name(id)))
    }

    /// Puts the chrome's title bar on a view's dock with the view's own tools.
    fn install_chrome(&mut self, views: &RefCell<ViewSet>, id: ViewId) {
        let Some(kind) = views.borrow().find(id).map(|state| state.kind) else {
            return;
        };
        let Some(chrome) = &mut self.chrome else {
            return;
        };
        let name = dock_name(id);
        if chrome.title_bar(&name).is_some() {
            return;
        }
        let bar = chrome.install(&name, kind_icon(kind), DockRole::View);
        // The kind switcher stands where the icon was: what a view shows is the first thing about
        // it, and the icon already said it. Its menu opens on a click, so it is widened for the
        // arrow beside its icon.
        bar.set_leading(
            TitleBarButton::new(
                "ViewKindButton",
                kind_icon(kind),
                "What This View Shows",
                "Plan, 3D, section or elevation. The view keeps its zoom and the layers it hides \
                 for when it comes back.",
            )
            .width(KIND_BUTTON_WIDTH)
            .with_menu(),
        );
        // Linked views, first among the view's tools: the one a person reaches for while working
        // across two views. Checked from the link, never from the click (update_link_button).
        bar.add_tool(
            TitleBarButton::new("ViewLinkButton", "ViewUnlinked", "Link this view", "")
                .checkable(false),
            ALWAYS,
        );
        bar.add_tool(
            TitleBarButton::new("ViewLayersButton", "Layers", "Layers", ""),
            ALWAYS,
        );
        bar.add_tool(
            TitleBarButton::new("ViewScaleButton", "ViewPlan", "Paper Style Scale", ""),
            PAPER_SCALE_PRIORITY,
        );
        // The zoom tools, each a ZOOM line through the command runner. In, Out and Zoom to
        // Selection are optional: a narrow view drops In and Out first, then Selection, and keeps
        // Zoom Extents; each is in the View menu as well.
        bar.add_tool(
            TitleBarButton::new("ViewZoomInButton", "ZoomIn", "Zoom In", ""),
            ZOOM_IN_OUT_PRIORITY,
        );
        bar.add_tool(
            TitleBarButton::new("ViewZoomOutButton", "ZoomOut", "Zoom Out", ""),
            ZOOM_IN_OUT_PRIORITY,
        );
        bar.add_tool(
            TitleBarButton::new(
                "ViewZoomSelectionButton",
                "ZoomSelection",
                "Zoom to Selection",
                "",
            ),
            ZOOM_SELECTION_PRIORITY,
        );
        bar.add_tool(
            TitleBarButton::new("ViewZoomExtentsButton", "ZoomExtents", "Zoom Extents", ""),
            ALWAYS,
        );
        self.update_bar(views, id);
    }

    /// Everything a view's bar shows that follows its state: its title and kind, its Link, its
    /// Layers, its Paper Style Scale and its zoom tools.
    fn update_bar(&mut self, views: &RefCell<ViewSet>, id: ViewId) {
        let set = views.borrow();
        let Some(state) = set.find(id) else {
            return;
        };
        let title = ViewSet::title(state);
        let kind = state.kind;
        let linked = state.linked;
        let hidden = hidden_count_of(state, &self.references);
        let scale = state.paper_style_scale;
        let others: Vec<String> = set
            .linked_views()
            .into_iter()
            .filter(|member| *member != id)
            .filter_map(|member| set.find(member).map(ViewSet::title))
            .collect();
        drop(set);
        let active = views.borrow().active_id() == id;
        let Some(bar) = self.bar_mut(id) else {
            return;
        };
        bar.set_title(&title);
        bar.set_icon(kind_icon(kind));
        bar.set_active(active);
        if let Some(mut leading) = bar.leading().cloned() {
            leading.icon = String::from(kind_icon(kind));
            bar.set_leading(leading);
        }

        // The Link button shows on a plan view's bar alone; checked, with the closed chain, while
        // the view is linked; dashed while a link of one waits for a second view.
        bar.set_tool_wanted("ViewLinkButton", linkable(kind));
        if let Some(link) = bar.tool_mut("ViewLinkButton") {
            link.checked = Some(linked);
            link.icon = String::from(if linked { "ViewLinked" } else { "ViewUnlinked" });
            link.waiting = linked && others.is_empty();
            let (label, tip) = if !linked {
                (
                    String::from("Link this view"),
                    String::from(
                        "Linked plan views pan and zoom together. Link a second view to keep \
                         them in step; the view you last panned or zoomed is the one the others \
                         come to.",
                    ),
                )
            } else if others.is_empty() {
                (
                    String::from("Linked, waiting"),
                    String::from("Link another plan view to keep them in step. Click to unlink."),
                )
            } else {
                let with = if others.len() == 1 {
                    others[0].clone()
                } else {
                    format!(
                        "{} and {}",
                        others[..others.len() - 1].join(", "),
                        others[others.len() - 1]
                    )
                };
                (
                    String::from("Linked"),
                    format!("Pans and zooms with {with}. Click to unlink."),
                )
            };
            link.set_tip(&label, &tip);
        }

        // A filtered view must never be mistaken for missing data, so the button changes its
        // glyph, not only its tooltip. "Of its own": hiding a parent is one entry however many
        // layers lie beneath it.
        if let Some(layers) = bar.tool_mut("ViewLayersButton") {
            layers.icon = String::from(if hidden > 0 {
                "ViewLayersFiltered"
            } else {
                "Layers"
            });
            let tip = if hidden > 0 {
                format!(
                    "{title} hides {hidden} layer(s) or reference layer(s) of its own, with what \
                     lies beneath them. Choose which layers it shows; the Layers panel hides a \
                     layer in every view."
                )
            } else {
                format!(
                    "Choose which layers {title} shows. It shows everything the drawing shows; \
                     the Layers panel hides a layer in every view."
                )
            };
            layers.set_tip("Layers in this view", &tip);
        }

        bar.set_tool_wanted("ViewScaleButton", kind == ViewKind::Plan);
        if let Some(button) = bar.tool_mut("ViewScaleButton") {
            let scale = scale.map_or_else(
                || String::from("Auto"),
                |scale| format!("1:{}", format_exact_real(scale)),
            );
            button.set_tip(
                &format!("Paper Style Scale: {scale}"),
                "Size paper-based linestyles and symbols without changing the camera zoom, \
                 annotation scale or plots (VIEWS SCALE).",
            );
        }

        // What ZOOM takes on each kind (zoom_takes, the verb's own rule): a tool the view's kind
        // would refuse is not offered; and what each does there, which differs by kind - only a
        // view that can be linked says the linked views follow.
        bar.set_tool_wanted(
            "ViewZoomInButton",
            zoom_takes(kind, ZoomRequestKind::In),
        );
        bar.set_tool_wanted(
            "ViewZoomOutButton",
            zoom_takes(kind, ZoomRequestKind::Out),
        );
        bar.set_tool_wanted(
            "ViewZoomSelectionButton",
            zoom_takes(kind, ZoomRequestKind::Scope),
        );
        let follow = if linkable(kind) {
            "; linked views follow"
        } else {
            ""
        };
        let about = match kind {
            ViewKind::Plan => "about the centre of this view",
            ViewKind::Section => "about the middle of the section's plot",
            _ => "about the middle of this view, as the wheel zooms there",
        };
        let tips = [
            (
                "ViewZoomInButton",
                "Zoom In",
                format!("Twice as close {about}{follow}."),
            ),
            (
                "ViewZoomOutButton",
                "Zoom Out",
                format!("Twice as far {about}{follow}."),
            ),
            (
                "ViewZoomSelectionButton",
                "Zoom to Selection",
                format!(
                    "Frame what is selected, as this view shows it, faint ghosts \
                     included{follow}. Nothing selected, or none of it shown here: nothing \
                     moves."
                ),
            ),
            (
                "ViewZoomExtentsButton",
                "Zoom Extents",
                format!(
                    "Frame everything this view shows{follow}. View > Zoom Extents does the \
                     same for the active view."
                ),
            ),
        ];
        for (name, label, tip) in tips {
            if let Some(button) = bar.tool_mut(name) {
                button.set_tip(label, &tip);
            }
        }
    }

    fn update_bars(&mut self, views: &RefCell<ViewSet>) {
        let ids: Vec<ViewId> = self.docks.iter().map(|view| view.id).collect();
        for id in ids {
            self.update_bar(views, id);
        }
    }

    /// Every view's link button, and the View menu's (LinksChanged).
    fn update_link_buttons(&mut self, views: &RefCell<ViewSet>) {
        self.update_bars(views);
        self.push(WorkspaceEvent::LinksChanged);
    }

    // ---- the widgets -----------------------------------------------------------------------

    /// Builds the widget for the state's kind, dropping what the dock held before - its running
    /// tool stopped first, as Esc stops it, so the window hears the tool end.
    fn build_content(
        &mut self,
        views: &RefCell<ViewSet>,
        id: ViewId,
        cx: &mut dyn PlanViewHost,
    ) {
        self.stop_tool_in(views, id, cx);
        let grid = self.grid_visible;
        let options = self.options;
        let gpu = self.gpu.clone();
        let lines = Rc::clone(&self.lines);
        let mut set = views.borrow_mut();
        let Some(state) = set.find_mut(id) else {
            return;
        };
        let Some(view) = self.docks.iter_mut().find(|view| view.id == id) else {
            return;
        };
        view.plan = None;
        view.render = None;
        view.section = None;
        match state.kind {
            ViewKind::Plan => {
                let mut plan = PlanView::new(cx.document(), state);
                plan.set_grid_visible(grid);
                wire_tools(&mut plan, id, lines);
                view.plan = Some(plan);
            }
            ViewKind::Model3D | ViewKind::Elevation => {
                let mut render = RenderView::new(cx.document(), state, &gpu);
                render.set_context(cx.document(), options);
                view.render = Some(render);
            }
            ViewKind::Section => {
                let mut section = SectionView::new();
                // For the selection on its crossings (docs/desktop.md, "The selection in every
                // view").
                section.set_document(Some(cx.document()));
                view.section = Some(section);
            }
        }
        drop(set);
        self.update_bar(views, id);
        self.relayout(views);
    }

    /// The widgets of views a verb opened, made now the drawing is lent.
    fn build_unbuilt(&mut self, views: &RefCell<ViewSet>, cx: &mut dyn PlanViewHost) {
        for id in std::mem::take(&mut self.unbuilt) {
            self.build_content(views, id, cx);
        }
    }

    /// Stops the tool running in `id`'s plan view, as Esc stops it, when it runs one: before that
    /// widget is dropped, since a plan view dropped mid-tool never reports that its tool ended.
    fn stop_tool_in(&mut self, views: &RefCell<ViewSet>, id: ViewId, cx: &mut dyn PlanViewHost) {
        let running = self
            .find(id)
            .and_then(|view| view.plan.as_ref())
            .is_some_and(PlanView::tool_active);
        if !running {
            return;
        }
        self.with_plan(views, id, |plan, state| plan.set_tool(cx, state, Tool::Select));
        self.drain_plan(views, id, Some(&mut *cx));
    }

    /// `f` with plan view `id` and its state; None when it is no open plan view.
    fn with_plan<R>(
        &mut self,
        views: &RefCell<ViewSet>,
        id: ViewId,
        f: impl FnOnce(&mut PlanView, &mut ViewState) -> R,
    ) -> Option<R> {
        let plan = self.find_mut(id).and_then(|view| view.plan.as_mut())?;
        let mut set = views.borrow_mut();
        let state = set.find_mut(id)?;
        Some(f(plan, state))
    }

    fn plan_ids(&self) -> Vec<ViewId> {
        self.docks
            .iter()
            .filter(|view| view.plan.is_some())
            .map(|view| view.id)
            .collect()
    }

    fn running_plan(&self) -> Option<ViewId> {
        self.docks
            .iter()
            .find(|view| view.plan.as_ref().is_some_and(PlanView::tool_active))
            .map(|view| view.id)
    }

    /// The active view when it is a plan view, else the plan view used most recently.
    fn active_plan(&self, views: &RefCell<ViewSet>) -> Option<ViewId> {
        let id = views.borrow().most_recent(ViewKind::Plan).map(|state| state.id)?;
        self.find(id)
            .is_some_and(|view| view.plan.is_some())
            .then_some(id)
    }

    // ---- what a plan view raised -----------------------------------------------------------

    /// Handles what plan view `id` has raised, as the C++ hooks did. `cx` is the drawing where it
    /// is lent; without it, what needs it waits for the next call that lends it.
    fn drain_plan(
        &mut self,
        views: &RefCell<ViewSet>,
        id: ViewId,
        mut cx: Option<&mut (dyn PlanViewHost + '_)>,
    ) {
        let events = match self.find_mut(id).and_then(|view| view.plan.as_mut()) {
            Some(plan) => plan.take_events(),
            None => return,
        };
        for event in events {
            self.plan_event(views, id, event, cx.as_deref_mut());
        }
    }

    fn plan_event(
        &mut self,
        views: &RefCell<ViewSet>,
        id: ViewId,
        event: PlanViewEvent,
        cx: Option<&mut (dyn PlanViewHost + '_)>,
    ) {
        match event {
            PlanViewEvent::Prompt(text) => self.push(WorkspaceEvent::Prompt(text)),
            PlanViewEvent::Error(text) => self.push(WorkspaceEvent::Error(text)),
            PlanViewEvent::CursorMoved { world, snap } => {
                self.push(WorkspaceEvent::CursorMoved { world, snap });
            }
            PlanViewEvent::ToolChanged(_) => {}
            PlanViewEvent::ActiveToolChanged(tool) => {
                if !tool.is_empty() {
                    self.last_tool_id.clone_from(&tool);
                }
                self.push(WorkspaceEvent::ActiveToolChanged(tool));
            }
            PlanViewEvent::ToolMessage(text) => self.push(WorkspaceEvent::ToolMessage(text)),
            PlanViewEvent::RepeatTool => match cx {
                // Through start_tool, not the view's own: a view starting a tool itself would
                // leave one running in another view. The view Enter was pressed in becomes the
                // active one, so it is where the tool runs.
                Some(cx) => {
                    if self.last_tool_id.is_empty() {
                        return;
                    }
                    self.activate(views, id);
                    let tool = self.last_tool_id.clone(); // start_tool's events rewrite it
                    if let Err(error) = self.start_tool(views, cx, &tool) {
                        self.push(WorkspaceEvent::Error(error.describe()));
                    }
                }
                None => self.held_events.push((id, PlanViewEvent::RepeatTool)),
            },
            PlanViewEvent::Activated => self.activate(views, id),
            PlanViewEvent::TextTyped(text) => self.push(WorkspaceEvent::TextTyped(text)),
            PlanViewEvent::ContextMenu { at } => {
                if self.context_menu_taker {
                    self.push(WorkspaceEvent::ContextMenu { view: id, at });
                } else {
                    // With no taker the right-click cancels, as a view with no handler does.
                    match cx {
                        Some(cx) => {
                            self.with_plan(views, id, |plan, state| plan.cancel(cx, state));
                            self.drain_plan(views, id, None);
                        }
                        None => self
                            .held_events
                            .push((id, PlanViewEvent::ContextMenu { at })),
                    }
                }
            }
            PlanViewEvent::EntityDoubleClicked(entity) => {
                self.push(WorkspaceEvent::EntityDoubleClicked(entity));
            }
            PlanViewEvent::ViewMoved { by_user } => {
                *self.reported.entry(id).or_default() += 1;
                if !by_user && cx.is_none() && views.borrow().find(id).is_some_and(|s| s.linked) {
                    self.held_events
                        .push((id, PlanViewEvent::ViewMoved { by_user }));
                } else {
                    self.view_moved(views, id, by_user, cx.map(|cx| &*cx));
                }
            }
            PlanViewEvent::ZoomExtents => match cx {
                Some(cx) => {
                    let _ = self.zoom_extents(views, id, cx, Scenery::default());
                }
                None => self.held_events.push((id, PlanViewEvent::ZoomExtents)),
            },
            PlanViewEvent::FrameStats(text) => self.push(WorkspaceEvent::FrameStats(text)),
            PlanViewEvent::PointPicked(point) => {
                if let Some(pick) = self.pick_waiting.take() {
                    self.picks.insert(pick, Some(point));
                }
                for plan in self.docks.iter_mut().filter_map(|view| view.plan.as_mut()) {
                    plan.await_point_pick(false);
                }
            }
        }
    }

    fn drain_render(&mut self, views: &RefCell<ViewSet>, id: ViewId) {
        let events = match self.find_mut(id).and_then(|view| view.render.as_mut()) {
            Some(render) => {
                render.deliver();
                render.take_events()
            }
            None => return,
        };
        for event in events {
            match event {
                RenderViewEvent::Activated => self.activate(views, id),
                RenderViewEvent::Status(text) => self.push(WorkspaceEvent::Status(text)),
                RenderViewEvent::FrameStats(text) => self.push(WorkspaceEvent::FrameStats(text)),
            }
        }
    }

    fn drain_section(&mut self, views: &RefCell<ViewSet>, id: ViewId) {
        let events = match self.find_mut(id).and_then(|view| view.section.as_mut()) {
            Some(section) => section.take_events(),
            None => return,
        };
        for event in events {
            match event {
                SectionViewEvent::Activated => self.activate(views, id),
                SectionViewEvent::FrameStats(text) => self.push(WorkspaceEvent::FrameStats(text)),
            }
        }
    }

    fn drain_all(&mut self, views: &RefCell<ViewSet>, mut cx: Option<&mut (dyn PlanViewHost + '_)>) {
        if cx.is_some() {
            for (id, event) in std::mem::take(&mut self.held_events) {
                self.plan_event(views, id, event, cx.as_deref_mut());
            }
        }
        let ids: Vec<ViewId> = self.docks.iter().map(|view| view.id).collect();
        for id in ids {
            self.drain_plan(views, id, cx.as_deref_mut());
            self.drain_render(views, id);
            self.drain_section(views, id);
        }
    }

    // ---- activation ------------------------------------------------------------------------

    fn activate(&mut self, views: &RefCell<ViewSet>, id: ViewId) {
        {
            let mut set = views.borrow_mut();
            if set.active_id() == id || set.activate(id).is_err() {
                return;
            }
        }
        self.update_active_marks(views);
        self.push(WorkspaceEvent::ActiveChanged);
    }

    fn update_active_marks(&mut self, views: &RefCell<ViewSet>) {
        let active = views.borrow().active_id();
        let ids: Vec<ViewId> = self.docks.iter().map(|view| view.id).collect();
        for id in ids {
            if let Some(bar) = self.bar_mut(id) {
                bar.set_active(id == active);
            }
        }
    }

    /// Makes view `id` the current page of its group, as a click on its tab does.
    fn raise(&mut self, id: ViewId) {
        if let Some(path) = self.path_of(id) {
            let _ = self.layout.set_active_tab(path);
        }
    }

    /// After `hidden` was hidden (minimised) while it was the active view: activates a view on
    /// screen, the first in the order opened; when none is, the first unhidden view, raised so
    /// that it is the current page of its group. Nothing when no other view is open and unhidden.
    fn activate_showing_instead_of(&mut self, views: &RefCell<ViewSet>, hidden: ViewId) {
        // Leaving the active view hidden would leave the menus acting on a view nobody can see.
        // The most recently active would be the better choice, but ViewSet keeps that order to
        // itself (it uses it when the active view is REMOVED).
        if views.borrow().active_id() != hidden {
            return;
        }
        let ids: Vec<ViewId> = self.docks.iter().map(|view| view.id).collect();
        if let Some(&id) = ids
            .iter()
            .find(|&&id| id != hidden && self.on_screen(id))
        {
            self.activate(views, id);
            return;
        }
        if let Some(&id) = ids.iter().find(|&&id| id != hidden && self.shown(id)) {
            self.activate(views, id);
            self.raise(id);
        }
    }

    // ---- open, close, kind -----------------------------------------------------------------

    /// A new view of `kind`, splitting the active view into equal halves along its longer side,
    /// or on its own when no view is docked; its widget made when `cx` is lent, else at the next
    /// call that lends the drawing.
    fn open_view(
        &mut self,
        views: &RefCell<ViewSet>,
        kind: ViewKind,
        activate_it: bool,
        cx: Option<&mut (dyn PlanViewHost + '_)>,
    ) -> ViewId {
        // A split made while the other views were hidden would be made against the wrong
        // neighbours.
        if let Some(chrome) = &mut self.chrome {
            chrome.unmaximise_docked(&mut self.layout);
        }
        let (id, previous) = {
            let mut set = views.borrow_mut();
            let id = set.add(kind).id;
            (id, set.active_id())
        };
        self.docks.push(View {
            id,
            plan: None,
            render: None,
            section: None,
        });
        let tab = ViewTab(dock_name(id));
        let beside = (previous != id).then_some(previous);
        let beside_path = beside
            .and_then(|beside| self.path_of(beside))
            .filter(|path| path.surface.is_main());
        let placed = self.placements();
        // What the C++ reports of a plan view a verb opened (VIEWS OPEN's record) is the view as
        // its widget was made: ViewportWidget's constructor sets the initial zoom, and the dock's
        // show() after splitDockWidget sends the widget the size Qt's split gave it before
        // resizeDocks evened the halves - half the active view's side, 330 of its 660 points where
        // the layout then leaves it 328 (docs/rust.md, step 7, "A view a verb opens"). The widget
        // here is made only when the drawing is next lent, so the state starts where the C++'s did.
        if cx.is_none() && kind == ViewKind::Plan {
            let split = beside
                .filter(|_| beside_path.is_some())
                .and_then(|beside| placed.get(&beside))
                .map(|at| {
                    let (width, height) = (at.rect.width(), at.rect.height());
                    if height > width {
                        (width, height / 2.0 - BAR_HEIGHT)
                    } else {
                        ((width / 2.0).floor(), height - BAR_HEIGHT)
                    }
                });
            let mut set = views.borrow_mut();
            if let Some(state) = set.find_mut(id) {
                if !state.plan_framed {
                    state.plan.scale = INITIAL_SCALE;
                }
                if let Some((width, height)) = split {
                    state
                        .plan
                        .resize(f64::from(width.round()), f64::from(height.round().max(1.0)));
                }
            }
        }
        match beside_path {
            Some(path) => {
                // Along the longer side, so both halves keep a usable shape; before the first
                // layout a dock's size means nothing: side by side, in halves.
                let rect = beside.and_then(|beside| placed.get(&beside).map(|at| at.rect));
                let tree = self.layout.main_surface_mut();
                match rect {
                    Some(rect) if rect.height() > rect.width() => {
                        let _ = tree.split_below(
                            path.node,
                            half_fraction(rect.height()),
                            vec![tab],
                        );
                    }
                    Some(rect) => {
                        let _ =
                            tree.split_right(path.node, half_fraction(rect.width()), vec![tab]);
                    }
                    None => {
                        let _ = tree.split_right(path.node, 0.5, vec![tab]);
                    }
                }
            }
            None => self.add_alone(tab),
        }
        self.install_chrome(views, id);
        match cx {
            Some(cx) => self.build_content(views, id, cx),
            None => self.unbuilt.push(id),
        }
        // Opened while the active view is hidden - every view minimised - the new view is the one
        // the person can see, so the menus act on it even when the caller did not ask for it.
        let active_hidden = beside.is_some_and(|beside| {
            self.find(beside).is_some() && !self.shown(beside)
        });
        if activate_it || active_hidden {
            self.activate(views, id);
        }
        self.update_active_marks(views);
        self.relayout(views);
        self.push(WorkspaceEvent::ViewsChanged);
        id
    }

    /// A dock where no docked view is to split: the dock area's first place. A dock area with no
    /// docked view starts again from this one, the floating views left where they are.
    fn add_alone(&mut self, tab: ViewTab) {
        let empty = self.layout.main_surface().num_tabs() == 0;
        if empty {
            *self.layout.main_surface_mut() = Tree::new(vec![tab]);
        } else {
            self.layout.push_to_first_leaf(tab);
        }
    }

    fn close_view(
        &mut self,
        views: &RefCell<ViewSet>,
        id: ViewId,
        cx: &mut dyn PlanViewHost,
    ) -> Status {
        // Before the widget is dropped: a tool left running there would never report its end.
        if self.find(id).is_some() {
            self.stop_tool_in(views, id, cx);
        }
        let Some(at) = self.docks.iter().position(|view| view.id == id) else {
            return Err(no_such_view(id));
        };
        self.docks.remove(at);
        let was_active = views.borrow().active_id();
        let name = dock_name(id);
        // Closing the maximised view brings back the views it hid, and a minimised one's tray
        // button goes with it.
        if let Some(chrome) = &mut self.chrome {
            chrome.forget(&mut self.layout, &name);
        }
        if let Some(path) = self.path_of(id) {
            let _ = self.layout.remove_tab(path);
        }
        let _ = views.borrow_mut().remove(id);
        if let Some(dialog) = &mut self.scale_dialog {
            dialog.refresh(NO_VIEW);
        }
        // It left the link, and a view it leaves alone left with it.
        self.update_link_buttons(views);
        if was_active == id {
            // ViewSet hands on to the view used most recently, which knows nothing of docks: it
            // may be minimised, or a tab page parked behind another. A minimised one hands on
            // again to a view that shows; a tab page is raised.
            let now = views.borrow().active_id();
            if self.find(now).is_some() {
                if !self.shown(now) {
                    self.activate_showing_instead_of(views, now);
                } else if !self.floating(now) {
                    self.raise(now);
                }
            }
        }
        self.update_active_marks(views);
        if was_active == id {
            self.push(WorkspaceEvent::ActiveChanged);
        }
        self.relayout(views);
        self.push(WorkspaceEvent::ViewsChanged);
        Ok(())
    }

    fn set_view_kind(
        &mut self,
        views: &RefCell<ViewSet>,
        id: ViewId,
        kind: ViewKind,
        cx: &mut dyn PlanViewHost,
    ) -> Status {
        let current = views.borrow().find(id).map(|state| state.kind);
        let (Some(current), true) = (current, self.find(id).is_some()) else {
            return Err(no_such_view(id));
        };
        if current == kind {
            return Ok(());
        }
        views.borrow_mut().set_kind(id, kind)?;
        self.build_content(views, id, cx);
        // A linked view that changed kind left the link (ViewSet::set_kind).
        self.update_link_buttons(views);
        self.push(WorkspaceEvent::ActiveChanged);
        self.push(WorkspaceEvent::ViewsChanged);
        Ok(())
    }

    /// VIEWS ACTIVATE: made active and brought where it can be seen - out of the tray, or to the
    /// front of its group.
    fn show_and_activate(&mut self, views: &RefCell<ViewSet>, id: ViewId) -> Status {
        if self.find(id).is_none() {
            return Err(no_such_view(id));
        }
        self.bring_back(views, id);
        self.activate(views, id);
        Ok(())
    }

    /// Through the tray when it is there, so it comes back where it was; else raised.
    fn bring_back(&mut self, views: &RefCell<ViewSet>, id: ViewId) {
        let name = dock_name(id);
        let restored = match &mut self.chrome {
            Some(chrome) => chrome.restore(&mut self.layout, &name),
            None => false,
        };
        if restored {
            self.after_restore(views, id);
        } else {
            self.raise(id);
        }
        self.relayout(views);
    }

    /// Back from the tray while the active view is still hidden - every view was minimised, the
    /// active one among them: the view brought back is the one the menus should act on.
    fn after_restore(&mut self, views: &RefCell<ViewSet>, id: ViewId) {
        let active = views.borrow().active_id();
        if self.find(active).is_none() || !self.shown(active) {
            self.activate(views, id);
        }
    }

    // ---- tiling ----------------------------------------------------------------------------

    /// VIEWS TILE: the views in the dock area re-docked each in a place of its own of equal size
    /// (tile_plan), the active one first and the rest in the order opened; none opened, none
    /// closed. A floating view is docked in - a person who asks for the views to be arranged
    /// wants them all arranged; a minimised one stays in the tray; a maximise is undone first.
    /// One docked view, or none, is left as it is.
    fn tile(&mut self, views: &RefCell<ViewSet>, kind: TileKind) -> TileOutcome {
        // A maximise is a passing state, not a layout: the views a docked one hid are back before
        // any is counted, and a floating one's record is gone before its view is docked.
        if let Some(chrome) = &mut self.chrome {
            chrome.unmaximise_all(&mut self.layout);
        }
        let tray: Vec<String> = self
            .chrome
            .as_ref()
            .map(DockChrome::minimised_names)
            .unwrap_or_default();
        let active = views.borrow().active_id();
        let mut ids: Vec<ViewId> = self
            .docks
            .iter()
            .map(|view| view.id)
            .filter(|&id| self.shown(id))
            .collect();
        if let Some(at) = ids.iter().position(|&id| id == active) {
            let first = ids.remove(at);
            ids.insert(0, first);
        }
        let docked = ids.iter().filter(|&&id| self.floating(id)).count();
        let outcome = TileOutcome {
            tiled: ids.len(),
            docked,
            minimised: tray.len(),
        };
        if ids.is_empty() || (ids.len() == 1 && docked == 0) {
            // Nothing to arrange; the undone maximise may still have resized a view.
            self.relayout(views);
            return outcome;
        }
        // A minimised view remembers the whole layout it left (dock_chrome's Minimised), and its
        // tray button puts that layout back when the same views are open - which would undo the
        // tiling at the first view restored. So each is brought back, parked on the tiling's
        // first place, and minimised again from there, in the tray's order: its button then
        // brings it back as a tab on the first tile, the tiles as they are. (A floating one is
        // left floating and minimised again from its window.)
        if let Some(chrome) = &mut self.chrome {
            for name in &tray {
                chrome.restore(&mut self.layout, name);
            }
        }
        let parked: Vec<String> = tray
            .iter()
            .filter(|name| {
                self.layout
                    .find_tab_from(|tab| tab.0 == **name)
                    .is_some_and(|path| path.surface.is_main())
            })
            .cloned()
            .collect();
        let leaving: BTreeSet<String> = ids
            .iter()
            .map(|id| dock_name(*id))
            .chain(parked.iter().cloned())
            .collect();
        crate::dock_chrome::remove_tabs(&mut self.layout, |tab| leaving.contains(&tab.0));
        // Each view's place in points as the steps cut it, where the area has been laid out: a
        // cut's fraction is worked from the extent it cuts (equal_fraction).
        let mut tree = Tree::new(vec![ViewTab(dock_name(ids[0]))]);
        let mut sizes: Vec<Option<Vec2>> = vec![None; ids.len()];
        sizes[0] = self.area.map(|area| area.size());
        for step in tile_plan(ids.len(), kind) {
            let existing = dock_name(ids[step.existing]);
            let Some((node, _)) = tree.find_tab_from(|tab| tab.0 == existing) else {
                continue;
            };
            let fraction = match sizes[step.existing] {
                Some(mut size) => {
                    let extent = if step.side_by_side { size.x } else { size.y };
                    let fraction = equal_fraction(extent, step.parts);
                    // egui_dock's borders: the middle less and plus half the separator.
                    let kept = (extent * fraction - SEPARATOR * 0.5).max(0.0);
                    let rest = (extent - extent * fraction - SEPARATOR * 0.5).max(0.0);
                    let mut added = size;
                    if step.side_by_side {
                        size.x = kept;
                        added.x = rest;
                    } else {
                        size.y = kept;
                        added.y = rest;
                    }
                    sizes[step.existing] = Some(size);
                    sizes[step.added] = Some(added);
                    fraction
                }
                // Before the first layout a place's size means nothing: the bare share.
                None => equal_fraction(f32::NAN, step.parts),
            };
            let tab = vec![ViewTab(dock_name(ids[step.added]))];
            if step.side_by_side {
                let _ = tree.split_right(node, fraction, tab);
            } else {
                let _ = tree.split_below(node, fraction, tab);
            }
        }
        *self.layout.main_surface_mut() = tree;
        for name in &parked {
            self.layout.push_to_first_leaf(ViewTab(name.clone()));
        }
        if let Some(chrome) = &mut self.chrome {
            for name in &tray {
                chrome.minimise(&mut self.layout, name);
            }
            chrome.refresh(&mut self.layout);
        }
        self.relayout(views);
        self.push(WorkspaceEvent::ViewsChanged);
        outcome
    }

    // ---- the tools -------------------------------------------------------------------------

    fn start_tool(
        &mut self,
        views: &RefCell<ViewSet>,
        cx: &mut dyn PlanViewHost,
        id: &str,
    ) -> Status {
        let Some(target) = self.active_plan(views) else {
            return Err(make_error(
                ErrorCode::InvalidState,
                "there is no plan view to draw in",
                id,
            ));
        };
        if tool_catalog().find(id).is_none() {
            return Err(make_error(
                ErrorCode::NotFound,
                format!("there is no tool '{id}'"),
                "",
            ));
        }
        // One tool at a time in the workspace: a second running in another view would hold picks
        // the person can no longer see is pending.
        for other in self.plan_ids() {
            if other != target {
                self.stop_tool_in(views, other, cx);
            }
        }
        let started = self
            .with_plan(views, target, |plan, state| plan.start_tool(cx, state, id))
            .unwrap_or(Ok(()));
        self.drain_plan(views, target, Some(&mut *cx));
        started
    }

    // ---- the zoom --------------------------------------------------------------------------

    /// What THIS view draws, or nothing for a view that is no plan view.
    fn drawn_bounds(&self, views: &RefCell<ViewSet>, id: ViewId, cx: &dyn PlanViewHost) -> Box2 {
        let set = views.borrow();
        match (
            self.find(id).and_then(|view| view.plan.as_ref()),
            set.find(id),
        ) {
            (Some(plan), Some(state)) => plan.drawn_bounds(cx, state),
            _ => Box2::default(),
        }
    }

    /// What the link's members draw between them, each through its own hidden layers.
    fn link_drawn_bounds(&self, views: &RefCell<ViewSet>, cx: &dyn PlanViewHost) -> Box2 {
        let members = views.borrow().linked_views();
        let mut bounds = Box2::default();
        for member in members {
            bounds.expand_box(self.drawn_bounds(views, member, cx));
        }
        bounds
    }

    /// The member that frames the link: the one the person moved last, else the lowest id.
    fn link_framer(views: &RefCell<ViewSet>) -> ViewId {
        let set = views.borrow();
        let members = set.linked_views();
        members
            .first()
            .map_or(NO_VIEW, |first| set.link_leader_for(*first))
    }

    fn zoom_extents(
        &mut self,
        views: &RefCell<ViewSet>,
        id: ViewId,
        cx: &mut dyn PlanViewHost,
        scenery: Scenery<'_>,
    ) -> Status {
        let Some(view) = self.find(id) else {
            return Err(no_such_view(id));
        };
        if view.plan.is_some() {
            // A linked view that draws nothing would go to the origin at a pixel a unit and take
            // the link there, millions of units off a survey: it frames what the link draws
            // instead. Here, where every Zoom Extents of one view comes, so none can miss it.
            let linked = views.borrow().find(id).is_some_and(|state| state.linked);
            if linked && self.drawn_bounds(views, id, cx).empty() {
                let link = self.link_drawn_bounds(views, cx);
                if !link.empty() {
                    self.with_plan(views, id, |plan, state| plan.zoom_to(state, &link));
                    self.drain_plan(views, id, Some(&mut *cx));
                    return Ok(());
                }
            }
            self.with_plan(views, id, |plan, state| plan.zoom_extents(cx, state));
            self.drain_plan(views, id, Some(&mut *cx));
        } else if view.render.is_some() {
            let scene = RenderScene {
                document: cx.document(),
                surfaces: scenery.surfaces,
                meshes: cx.meshes(),
            };
            let mut set = views.borrow_mut();
            if let (Some(render), Some(state)) = (
                self.docks
                    .iter_mut()
                    .find(|view| view.id == id)
                    .and_then(|view| view.render.as_mut()),
                set.find_mut(id),
            ) {
                render.zoom_extents(&scene, state);
            }
        } else if view.section.is_some() {
            let set = views.borrow();
            if let (Some(section), Some(state)) = (
                self.docks
                    .iter_mut()
                    .find(|view| view.id == id)
                    .and_then(|view| view.section.as_mut()),
                set.find(id),
            ) {
                section.zoom_extents(state);
            }
        }
        Ok(())
    }

    fn zoom_extents_all(&mut self, views: &RefCell<ViewSet>, cx: &mut dyn PlanViewHost) {
        // The link, once: on everything its views draw between them.
        let framer = Self::link_framer(views);
        let link = self.link_drawn_bounds(views, cx);
        let ids: Vec<ViewId> = self.docks.iter().map(|view| view.id).collect();
        for id in ids {
            let linked = views.borrow().find(id).is_some_and(|state| state.linked);
            if linked {
                if id == framer && self.find(id).is_some_and(|view| view.plan.is_some()) {
                    // An empty box frames nothing, as Zoom Extents on nothing always has.
                    if link.empty() {
                        self.with_plan(views, id, |plan, state| plan.zoom_extents(cx, state));
                    } else {
                        self.with_plan(views, id, |plan, state| plan.zoom_to(state, &link));
                    }
                    self.drain_plan(views, id, Some(&mut *cx));
                }
                continue; // the others follow the framer
            }
            let _ = self.zoom_extents(views, id, cx, Scenery::default());
        }
    }

    fn zoom_to(&mut self, views: &RefCell<ViewSet>, bounds: &Box2) {
        let framer = Self::link_framer(views);
        for id in self.plan_ids() {
            let linked = views.borrow().find(id).is_some_and(|state| state.linked);
            if linked && id != framer {
                continue; // follows the framer
            }
            self.with_plan(views, id, |plan, state| plan.zoom_to(state, bounds));
            self.drain_plan(views, id, None);
        }
    }

    /// A plan view's pan or zoom changed: noted when the person made it, and when the view is
    /// linked the others take its centre and scale and every member holds that view on a resize.
    fn view_moved(
        &mut self,
        views: &RefCell<ViewSet>,
        id: ViewId,
        by_user: bool,
        cx: Option<&dyn PlanViewHost>,
    ) {
        if self.following {
            return;
        }
        self.following = true;
        if by_user {
            views.borrow_mut().note_moved(id);
        }
        let linked = views.borrow().find(id).is_some_and(|state| state.linked);
        if linked {
            let mut source = id;
            let draws_nothing = !by_user
                && cx.is_some_and(|cx| {
                    self.find(id).is_some_and(|view| view.plan.is_some())
                        && self.drawn_bounds(views, id, cx).empty()
                });
            if draws_nothing {
                // Framed at its first paint on nothing: no place to lead the link to. It takes
                // the link's place instead, from the member moved last that has framed; with none
                // framed yet, it waits for the first that frames.
                source = Self::framed_member_besides(views, id);
            }
            if source != NO_VIEW {
                let followers = views.borrow_mut().follow(source);
                for follower in followers {
                    if let Some(plan) = self.find_mut(follower).and_then(|view| view.plan.as_mut())
                    {
                        plan.hold_view();
                    }
                }
                // The view that moved holds it too: a frame refitted on its next resize would
                // change its scale, which no follower would hear of.
                if let Some(plan) = self.find_mut(source).and_then(|view| view.plan.as_mut()) {
                    plan.hold_view();
                }
            }
        }
        self.following = false;
    }

    /// A member of the link other than `id` that has framed something - the one the person moved
    /// last, else the first opened - or NO_VIEW.
    fn framed_member_besides(views: &RefCell<ViewSet>, id: ViewId) -> ViewId {
        let set = views.borrow();
        let mut best = NO_VIEW;
        let mut latest = 0;
        for member in set.linked_views() {
            let Some(state) = set.find(member) else {
                continue;
            };
            if member == id || !state.plan_framed {
                continue;
            }
            if best == NO_VIEW || state.last_moved > latest {
                best = member;
                latest = state.last_moved;
            }
        }
        best
    }

    /// The verbs' zoom, made with the drawing lent (the C++'s zoomView).
    fn zoom_view(
        &mut self,
        views: &RefCell<ViewSet>,
        cx: &mut dyn PlanViewHost,
        scenery: Scenery<'_>,
        request: &ZoomRequest,
    ) -> Result<Vec<ViewId>> {
        let id = request.view;
        let Some(view) = self.find(id) else {
            return Err(no_such_view(id));
        };
        if views.borrow().find(id).is_none() {
            return Err(no_such_view(id));
        }
        let (is_plan, is_render, is_section) = (
            view.plan.is_some(),
            view.render.is_some(),
            view.section.is_some(),
        );
        match request.kind {
            ZoomRequestKind::Extents => {
                // Through the view's own Zoom Extents, where a linked view that draws nothing
                // frames what the link draws.
                self.zoom_extents(views, id, cx, scenery)?;
            }
            ZoomRequestKind::Scope | ZoomRequestKind::Window => {
                if request.kind == ZoomRequestKind::Scope && is_render {
                    // What a scope took that the view shows, framed where the scene puts it.
                    let scene = RenderScene {
                        document: cx.document(),
                        surfaces: scenery.surfaces,
                        meshes: cx.meshes(),
                    };
                    let mut set = views.borrow_mut();
                    let framed = match (
                        self.docks
                            .iter_mut()
                            .find(|view| view.id == id)
                            .and_then(|view| view.render.as_mut()),
                        set.find_mut(id),
                    ) {
                        (Some(render), Some(state)) => {
                            render.zoom_to_entities(&scene, state, &request.ids)
                        }
                        _ => false,
                    };
                    if !framed {
                        return Ok(Vec::new());
                    }
                } else {
                    if request.kind == ZoomRequestKind::Scope && request.window.empty() {
                        return Ok(Vec::new());
                    }
                    // A box, or the extent of what a scope took, framed as Zoom Extents frames the
                    // drawing (the view's margin, at its size).
                    if !is_plan {
                        return Err(make_error(
                            ErrorCode::InvalidArgument,
                            "ZOOM frames a box in a plan view",
                            format!("{id}"),
                        ));
                    }
                    let window = request.window;
                    self.with_plan(views, id, |plan, state| plan.zoom_to(state, &window));
                    self.drain_plan(views, id, Some(&mut *cx));
                }
            }
            ZoomRequestKind::In
            | ZoomRequestKind::Out
            | ZoomRequestKind::Factor
            | ZoomRequestKind::Centre => {
                let factor = if request.kind == ZoomRequestKind::Out {
                    1.0 / request.factor
                } else {
                    request.factor
                };
                if is_render && request.kind != ZoomRequestKind::Centre {
                    // As the wheel turned over the middle of the view, by the notches that make
                    // `factor`.
                    let scene = RenderScene {
                        document: cx.document(),
                        surfaces: scenery.surfaces,
                        meshes: cx.meshes(),
                    };
                    let mut set = views.borrow_mut();
                    if let (Some(render), Some(state)) = (
                        self.docks
                            .iter_mut()
                            .find(|view| view.id == id)
                            .and_then(|view| view.render.as_mut()),
                        set.find_mut(id),
                    ) {
                        render.zoom_by(&scene, state, factor);
                    }
                } else if is_section && request.kind != ZoomRequestKind::Centre {
                    // About the middle of the plot, as a plan view zooms about its centre.
                    let set = views.borrow();
                    if let (Some(section), Some(state)) = (
                        self.docks
                            .iter_mut()
                            .find(|view| view.id == id)
                            .and_then(|view| view.section.as_mut()),
                        set.find(id),
                    ) {
                        let middle = section.plot_centre();
                        section.zoom_at(state, middle, factor);
                    }
                } else {
                    if !is_plan {
                        return Err(make_error(
                            ErrorCode::InvalidArgument,
                            "only a plan view zooms that way here",
                            format!("{id}"),
                        ));
                    }
                    // A view that has framed nothing yet frames what it draws first, as its first
                    // paint would: zoomed from the place a new view holds until then and marked
                    // framed, it never framed the drawing, and a link took the others there too.
                    self.with_plan(views, id, |plan, state| plan.frame_if_unframed(cx, state));
                    self.drain_plan(views, id, Some(&mut *cx));
                    self.with_plan(views, id, |plan, state| {
                        let _ = apply_plan_zoom(&mut state.plan, request);
                        state.plan_framed = true;
                        // The person's view now: kept on a resize, as after the wheel.
                        plan.hold_view();
                    });
                    self.view_moved(views, id, true, Some(&*cx));
                }
            }
        }
        let set = views.borrow();
        let mut moved = vec![id];
        if set.find(id).is_some_and(|state| state.linked) {
            moved.extend(
                set.linked_views()
                    .into_iter()
                    .filter(|member| *member != id),
            );
        }
        Ok(moved)
    }

    /// The verbs changed the link behind the widgets: a linked view holds the link's view, which a
    /// refit on resize would lose.
    fn link_changed(&mut self, views: &RefCell<ViewSet>, ids: &[ViewId]) {
        for &id in ids {
            let linked = views.borrow().find(id).is_some_and(|state| state.linked);
            if let Some(plan) = self.find_mut(id).and_then(|view| view.plan.as_mut())
                && linked
            {
                plan.hold_view();
            }
        }
        self.update_link_buttons(views);
    }

    /// One view's own hidden layers, references or ghosts changed: no document change, so no
    /// listener hears it.
    fn view_settings_changed(&mut self, views: &RefCell<ViewSet>, id: ViewId) {
        let Some(view) = self.find_mut(id) else {
            return;
        };
        if let Some(render) = &mut view.render {
            render.invalidate_scene();
        } else if let Some(plan) = &mut view.plan {
            plan.invalidate_reference_cache();
        }
        self.update_bar(views, id);
        self.push(WorkspaceEvent::ViewSettingsChanged);
    }

    fn refresh_paper_scales(&mut self, views: &RefCell<ViewSet>, document: &Document) {
        let restored = restore_view_paper_scales(document, &mut views.borrow_mut());
        match restored {
            Err(error) => {
                let problem = error.describe();
                if problem != self.paper_scale_problem {
                    self.push(WorkspaceEvent::Error(problem.clone()));
                }
                self.paper_scale_problem = problem;
            }
            Ok(restored) => {
                self.paper_scale_problem.clear();
                for id in &restored {
                    self.view_settings_changed(views, *id);
                }
                if !restored.is_empty()
                    && let Some(dialog) = &mut self.scale_dialog
                {
                    dialog.refresh(NO_VIEW);
                }
            }
        }
    }
}

/// How many layers and reference layers a view hides that still exist: what its Layers button
/// reports. Only reference ids that still name a layer count: a removed image's id stays in the
/// view's set (ids are never reused) and must not make the view look filtered.
fn hidden_count_of(state: &ViewState, references: &BTreeSet<u64>) -> usize {
    let kept = state
        .hidden_references
        .iter()
        .filter(|id| references.contains(id))
        .count();
    state.layers.hidden().len() + kept
}

/// The reference layers `reference` holds, by id.
fn reference_ids(reference: Option<&ReferenceData>) -> BTreeSet<u64> {
    reference.map_or_else(BTreeSet::new, |reference| {
        reference
            .rasters()
            .iter()
            .map(|raster| raster.id)
            .chain(reference.point_clouds().iter().map(|cloud| cloud.id))
            .collect()
    })
}

/// A plan view's hooks as the workspace sets them: it repeats tools, shows the shortcut menu, does
/// the Zoom Extents of a middle double-click and takes typed text - each read when it is raised,
/// so a view made before the window set its hooks reaches them. A ZOOM typed while a tool runs is
/// the view's, not the tool's: its line, naming this view, is queued for the window's executor and
/// the tool stays at its step, as AutoCAD resumes LINE after 'ZOOM. PAN has no verb, so the tool
/// host goes on refusing it by name.
fn wire_tools(plan: &mut PlanView, id: ViewId, lines: Rc<RefCell<Vec<String>>>) {
    plan.hooks = PlanViewHooks {
        repeat_tool: true,
        context_menu: true,
        zoom_extents: true,
        text_typed: true,
    };
    plan.tool_host_mut().on_transparent = Some(Rc::new(move |_tools, _document, command| {
        let line = transparent_zoom_line(command);
        if line.is_empty() {
            return false;
        }
        lines.borrow_mut().push(for_view(&line, id));
        true
    }));
}

// ---- the verbs' host ----------------------------------------------------------------------------

impl ViewVerbHost for Shared {
    fn views(&self) -> &RefCell<ViewSet> {
        if self.mode.get() == HostMode::Record {
            &self.scratch
        } else {
            self.views.as_ref()
        }
    }

    fn open(&self, kind: ViewKind) -> Result<ViewId> {
        if self.mode.get() == HostMode::Record {
            return Ok(self.scratch.borrow_mut().add(kind).id);
        }
        // Its widget is made when the drawing is next lent: the verbs hold it now.
        Ok(self
            .inner
            .borrow_mut()
            .open_view(&self.views, kind, true, None))
    }

    fn activate(&self, id: ViewId) -> Status {
        if self.mode.get() == HostMode::Record {
            return self.scratch.borrow_mut().activate(id);
        }
        self.inner.borrow_mut().show_and_activate(&self.views, id)
    }

    fn zoom(&self, request: &ZoomRequest) -> Result<Vec<ViewId>> {
        match self.mode.get() {
            HostMode::Record => {
                self.recorded.borrow_mut().push(request.clone());
                Err(deferred())
            }
            HostMode::Replay => self.replies.borrow_mut().pop_front().unwrap_or_else(|| {
                Err(make_error(
                    ErrorCode::InvalidState,
                    "the zoom was not made before the line ran",
                    "",
                ))
            }),
            HostMode::Direct => Err(make_error(
                ErrorCode::InvalidState,
                "a zoom frames what a view draws, and is run through the workspace's line \
                 runner, which lends it the drawing",
                format!("{}", request.view),
            )),
        }
    }

    fn changed(&self, ids: &[ViewId]) {
        if self.mode.get() == HostMode::Record {
            return;
        }
        self.inner.borrow_mut().link_changed(&self.views, ids);
    }

    fn settings_changed(&self, id: ViewId) {
        if self.mode.get() == HostMode::Record {
            return;
        }
        self.inner.borrow_mut().view_settings_changed(&self.views, id);
    }

    fn has_reference(&self, id: u64) -> bool {
        self.inner.borrow().references.contains(&id)
    }

    fn mesh_plan_display(&self) -> Option<MeshPlanDisplay> {
        Some(crate::plan_painter::mesh_plan_display())
    }

    fn set_mesh_plan_display(&self, display: MeshPlanDisplay) -> Status {
        // The window's one switch: every plan view repaints at its next frame, its kept drawing
        // being keyed on it (viewport_widget's DrawingKey), and plots and sheets read it as they
        // are made (plan_source_of).
        if self.mode.get() != HostMode::Record {
            crate::plan_painter::set_mesh_plan_display(display);
        }
        Ok(())
    }

    fn tile(&self, kind: TileKind) -> Result<TileOutcome> {
        if self.mode.get() == HostMode::Record {
            // A probe arranges nothing: the copy it runs on has no docks.
            return Ok(TileOutcome::default());
        }
        Ok(self.inner.borrow_mut().tile(&self.views, kind))
    }
}

impl Shared {
    /// What the next call lent the drawing does first: the paper scales restored when the drawing
    /// notified, the widgets of views a verb opened made, and the reference layers counted.
    fn look(&self, cx: &mut dyn PlanViewHost) {
        let mut inner = self.inner.borrow_mut();
        inner.references = reference_ids(cx.reference());
        inner.build_unbuilt(&self.views, cx);
        if self.notified.swap(false, Ordering::AcqRel) {
            inner.refresh_paper_scales(&self.views, cx.document());
        }
        if std::mem::take(&mut inner.zoom_extents_all_asked) {
            inner.zoom_extents_all(&self.views, cx);
        }
        inner.drain_all(&self.views, Some(&mut *cx));
    }

    /// The scope word VIEW's answer: the workspace's views.
    fn scope_provider(&self) -> Box<ScopeViewFn<'static>> {
        let views = Rc::clone(&self.views);
        Box::new(move |id| scope_view_of(&views.borrow(), id))
    }
}

// ---- the line runner ----------------------------------------------------------------------------

/// The workspace as the window's executor reaches it: the line runner a line that may reach the
/// views goes through (see the module comment). Cheap to clone; it holds the workspace's shared
/// state, not the workspace.
#[derive(Clone)]
pub struct WorkspaceLines {
    shared: Rc<Shared>,
}

impl WorkspaceLines {
    /// The verbs' host the interpreter is given (CommandInterpreter::set_view_host).
    pub fn verb_host(&self) -> Rc<dyn ViewVerbHost> {
        Rc::clone(&self.shared) as Rc<dyn ViewVerbHost>
    }

    /// The scope word VIEW's provider (CommandInterpreter::set_scope_context).
    pub fn scope_provider(&self) -> Box<ScopeViewFn<'static>> {
        self.shared.scope_provider()
    }

    /// Runs `line` with `interpreter` over the drawing `cx` lends, the views reached through this
    /// workspace: the interpreter is given the workspace as its views' host and its scope's VIEW,
    /// and a ZOOM is probed, made and run as the module comment says.
    pub fn run_line(
        &self,
        interpreter: &mut CommandInterpreter,
        cx: &mut dyn PlanViewHost,
        surfaces: &[SceneSurface<'_>],
        line: &str,
    ) -> Result<String> {
        interpreter.set_view_host(Some(self.verb_host()));
        interpreter.set_scope_context(Some(self.scope_provider()));
        self.shared.look(cx);
        let reply = if CommandInterpreter::verb_of(line) == "ZOOM" {
            self.run_zoom(interpreter, cx, surfaces, line)
        } else {
            interpreter.run(cx.document_mut(), line)
        };
        self.shared.look(cx);
        reply
    }

    fn run_zoom(
        &self,
        interpreter: &mut CommandInterpreter,
        cx: &mut dyn PlanViewHost,
        surfaces: &[SceneSurface<'_>],
        line: &str,
    ) -> Result<String> {
        let shared = &self.shared;
        // The probe, over a copy of the views: ZOOM reads the drawing and edits none of it.
        *shared.scratch.borrow_mut() = shared.views.borrow().clone();
        shared.recorded.borrow_mut().clear();
        shared.mode.set(HostMode::Record);
        let mut probe = CommandInterpreter::new();
        probe.set_view_host(Some(self.verb_host()));
        probe.set_scope_context(Some(self.scope_provider()));
        let _ = probe.run(cx.document_mut(), line);
        shared.mode.set(HostMode::Direct);
        let recorded = std::mem::take(&mut *shared.recorded.borrow_mut());
        if recorded.is_empty() {
            // Nothing to frame, or refused before the zoom: the line as it is.
            return interpreter.run(cx.document_mut(), line);
        }
        // The meshes are the drawing's, lent with it.
        let scenery = Scenery {
            surfaces,
            meshes: &[],
        };
        for request in &recorded {
            let moved = shared
                .inner
                .borrow_mut()
                .zoom_view(&shared.views, cx, scenery, request);
            shared.replies.borrow_mut().push_back(moved);
        }
        shared.mode.set(HostMode::Replay);
        let reply = interpreter.run(cx.document_mut(), line);
        shared.mode.set(HostMode::Direct);
        shared.replies.borrow_mut().clear();
        reply
    }

    /// A runner of a recording host's own (host::recording's Answer::Custom) that runs each line
    /// through this workspace on the host's interpreter: the C++ workspace's own interpreter, which
    /// it ran its bars' lines with until the window handed it its executor.
    pub fn answer(&self) -> crate::host::recording::CustomRunner {
        let lines = self.clone();
        Box::new(move |parts, line| {
            let mut cx = crate::viewport_widget::ViewParts {
                document: &mut parts.document,
                reference: Some(&parts.reference),
                meshes: &[],
            };
            match lines.run_line(&mut parts.interpreter, &mut cx, &[], line) {
                Ok(reply) => VerbOutcome::new(true, &reply, ""),
                Err(error) => VerbOutcome::new(false, "", &error.describe()),
            }
        })
    }
}

// ---- the workspace ------------------------------------------------------------------------------

/// The drawing views as docks (see the module comment). A clone is another handle on the same
/// workspace: the window keeps one and lends another to its host, through which a dialog reaches
/// the views (Host::views).
#[derive(Clone)]
pub struct ViewWorkspace {
    shared: Rc<Shared>,
}

impl ViewWorkspace {
    /// Opens one plan view, docked: a headless plot needs a plan view in a window never shown.
    pub fn new(document: &Document) -> Self {
        Self::with_gpu(document, GpuSession::headless())
    }

    /// The workspace whose 3D views are drawn by `gpu`'s rules.
    pub fn with_gpu(document: &Document, gpu: GpuSession) -> Self {
        let notified = Arc::new(AtomicBool::new(true));
        let heard = Arc::clone(&notified);
        let registration = document.add_listener(move || heard.store(true, Ordering::Release));
        let inner = Inner {
            docks: Vec::new(),
            layout: DockState::new(Vec::new()),
            chrome: None,
            area: None,
            pixels_per_point: 1.0,
            grid_visible: true,
            options: SceneOptions::default(),
            gpu,
            last_tool_id: String::new(),
            context_menu_taker: false,
            following: false,
            paper_scale_problem: String::new(),
            scale_dialog: None,
            kind_menu: None,
            kind_choice: None,
            kind_menu_dismissed: None,
            pointer: None,
            float_drag: None,
            bar_menu: None,
            bar_menu_choice: None,
            events: Vec::new(),
            lines: Rc::new(RefCell::new(Vec::new())),
            unbuilt: Vec::new(),
            held_events: Vec::new(),
            zoom_extents_all_asked: false,
            picks: BTreeMap::new(),
            pick_waiting: None,
            next_pick: 1,
            references: BTreeSet::new(),
            fixed: BTreeMap::new(),
            reported: BTreeMap::new(),
        };
        let shared = Rc::new(Shared {
            views: Rc::new(RefCell::new(ViewSet::new())),
            scratch: RefCell::new(ViewSet::new()),
            mode: Cell::new(HostMode::Direct),
            recorded: RefCell::new(Vec::new()),
            replies: RefCell::new(VecDeque::new()),
            inner: RefCell::new(inner),
            notified,
            _registration: registration,
            popup: RefCell::new(None),
        });
        let workspace = Self { shared };
        {
            let mut inner = workspace.shared.inner.borrow_mut();
            inner.open_view(&workspace.shared.views, ViewKind::Plan, true, None);
            // The first view's widget, made now: a view made before the window set its hooks
            // still reaches them, since they are read when raised.
            let id = inner.unbuilt.pop().unwrap_or(NO_VIEW);
            inner.build_first(&workspace.shared.views, id, document);
            inner.refresh_paper_scales(&workspace.shared.views, document);
        }
        workspace.shared.notified.store(false, Ordering::Release);
        // Raised before the window could hear them, as the C++ constructor's hooks were unset.
        workspace.inner().events.clear();
        workspace
    }

    fn views_cell(&self) -> &RefCell<ViewSet> {
        &self.shared.views
    }

    fn inner(&self) -> std::cell::RefMut<'_, Inner> {
        self.shared.inner.borrow_mut()
    }

    /// What the workspace does first at every call lent the drawing, done now for a caller that
    /// lends it without drawing: the paper scales restored when the drawing notified (a Document
    /// listener's work in the C++, done at the next look), the widgets of views a verb opened
    /// made, and what the views raised handled.
    pub fn follow_drawing(&self, cx: &mut dyn PlanViewHost) {
        self.shared.look(cx);
    }

    /// The line runner the window's executor runs lines through.
    pub fn lines(&self) -> WorkspaceLines {
        WorkspaceLines {
            shared: Rc::clone(&self.shared),
        }
    }

    /// The verbs' host (ViewWorkspace::verbHost).
    pub fn verb_host(&self) -> Rc<dyn ViewVerbHost> {
        self.lines().verb_host()
    }

    // ---- chrome ----------------------------------------------------------------------------

    /// Gives every open and future view the window's title bar - its kind switcher, its Layers,
    /// Zoom Extents and the window buttons - with the active view's marked. Without it the views
    /// keep a plain title.
    pub fn set_chrome(&self, icons: ChromeIcons) {
        let mut inner = self.inner();
        inner.chrome = Some(DockChrome::new(icons));
        let ids: Vec<ViewId> = inner.docks.iter().map(|view| view.id).collect();
        for id in ids {
            inner.install_chrome(self.views_cell(), id);
        }
        inner.update_active_marks(self.views_cell());
    }

    pub fn has_chrome(&self) -> bool {
        self.inner().chrome.is_some()
    }

    /// View `id`'s title bar as it stands; None without chrome or for a view not open.
    pub fn title_bar(&self, id: ViewId) -> Option<DockTitleBar> {
        self.inner()
            .chrome
            .as_ref()
            .and_then(|chrome| chrome.title_bar(&dock_name(id)).cloned())
    }

    /// `f` with view `id`'s title bar, to change what it shows (QToolButton's own calls on a
    /// bar); None without chrome or for a view not open.
    pub fn with_title_bar<R>(&self, id: ViewId, f: impl FnOnce(&mut DockTitleBar) -> R) -> Option<R> {
        self.inner().bar_mut(id).map(f)
    }

    /// The tray's buttons for the minimised views (`Restore<dock>`), for the window's tray.
    pub fn tray_actions(&self) -> Vec<Action> {
        self.inner()
            .chrome
            .as_ref()
            .map(DockChrome::tray_actions)
            .unwrap_or_default()
    }

    // ---- the docks, as the chrome moves them -----------------------------------------------

    /// Minimise, as the view's own button does: hidden to the tray, and the active view handed on
    /// to one on screen.
    pub fn minimise_view(&self, id: ViewId) {
        let mut inner = self.inner();
        let name = dock_name(id);
        let Inner { chrome, layout, .. } = &mut *inner;
        if let Some(chrome) = chrome {
            chrome.minimise(layout, &name);
        }
        inner.activate_showing_instead_of(self.views_cell(), id);
        inner.relayout(self.views_cell());
    }

    /// The tray button: back where it was. False when it was not minimised.
    pub fn restore_view(&self, id: ViewId) -> bool {
        let mut inner = self.inner();
        let name = dock_name(id);
        let Inner { chrome, layout, .. } = &mut *inner;
        let restored = chrome
            .as_mut()
            .is_some_and(|chrome| chrome.restore(layout, &name));
        if restored {
            inner.after_restore(self.views_cell(), id);
            inner.relayout(self.views_cell());
        }
        restored
    }

    /// `restore_view` by the view's dock name, as the window's tray holds it (`Restore<dock>`).
    pub fn restore_named(&self, dock: &str) -> bool {
        dock_id(dock).is_some_and(|id| self.restore_view(id))
    }

    /// Maximise or Restore, as the view's own button does; pressing it activates the view first.
    /// A docked view has the workspace to itself, the other docked views hidden; a floating one
    /// fills the workspace over them, and Restore gives it back the rectangle it had.
    pub fn toggle_maximised(&self, id: ViewId) {
        let mut inner = self.inner();
        inner.activate(self.views_cell(), id);
        let name = dock_name(id);
        // The room a floating view fills, in the points its rectangle is kept in: from the
        // workspace's top left. It was the workspace's place on the screen, which a rectangle
        // read from its own corner would have drawn that far off it.
        let screen = inner.area.map_or(Rect::NOTHING, |area| {
            Rect::from_min_size(Pos2::ZERO, area.size())
        });
        let Inner { chrome, layout, .. } = &mut *inner;
        if let Some(chrome) = chrome {
            chrome.toggle_maximised(layout, &name, screen);
        }
        inner.float_drag = None;
        inner.relayout(self.views_cell());
    }

    /// Float or Dock, as the view's own button does: a docked view is lifted off its place into
    /// a window over it, inside the workspace; a floating one goes back where it came from.
    /// Pressing it is working in that view, which is made active first.
    pub fn toggle_floating(&self, id: ViewId) {
        let mut inner = self.inner();
        inner.activate(self.views_cell(), id);
        inner.toggle_floating(self.views_cell(), id);
    }

    /// Where floating view `id` is, in points from the workspace's top left; None for a view
    /// that does not float.
    pub fn float_rect(&self, id: ViewId) -> Option<Rect> {
        self.inner().float_rect(id)
    }

    /// Puts floating view `id` at `rect` (points from the workspace's top left), as a drag of its
    /// title bar does, its bar kept where it can be reached. False for a view that does not
    /// float, a maximised one, and a rectangle that is no place.
    pub fn set_float_rect(&self, id: ViewId, rect: Rect) -> bool {
        let mut inner = self.inner();
        let name = dock_name(id);
        if !inner.floating(id) || !rect.is_finite() || !rect.is_positive() {
            return false;
        }
        let room = inner.room();
        let placed = match &mut inner.chrome {
            Some(chrome) if !chrome.is_maximised(&name) => {
                chrome.set_float_rect(&name, kept_in_reach(rect, room))
            }
            _ => false,
        };
        if placed {
            inner.relayout(self.views_cell());
        }
        placed
    }

    pub fn is_maximised(&self, id: ViewId) -> bool {
        self.inner()
            .chrome
            .as_ref()
            .is_some_and(|chrome| chrome.is_maximised(&dock_name(id)))
    }

    pub fn is_minimised(&self, id: ViewId) -> bool {
        self.inner()
            .chrome
            .as_ref()
            .is_some_and(|chrome| chrome.is_minimised(&dock_name(id)))
    }

    /// A click on view `id`'s tab: it is the page its group shows.
    pub fn bring_to_front(&self, id: ViewId) {
        self.inner().raise(id);
    }

    /// Where the person can see it: in the dock area, and floating or the page of its group shown.
    pub fn on_screen(&self, id: ViewId) -> bool {
        self.inner().on_screen(id)
    }

    /// View `id`'s dock in the workspace, its bar included, in points from the workspace's top
    /// left, as the area was last laid out; None for a view not docked or before the first draw.
    pub fn dock_rect(&self, id: ViewId) -> Option<Rect> {
        self.inner().placements().get(&id).map(|placed| placed.rect)
    }

    /// The workspace's own rectangle, from its top left: what dock_rect is measured in.
    pub fn rect(&self) -> Rect {
        self.inner()
            .area
            .map_or(Rect::NOTHING, |area| Rect::from_min_size(Pos2::ZERO, area.size()))
    }

    /// The gap between two docked views.
    pub fn separator(&self) -> f32 {
        SEPARATOR
    }

    /// Pins view `id`'s widget to `width` by `height` pixels whatever its dock's size, or lets it
    /// fill its dock again (None): QWidget::setFixedSize, as the C++ tests set a view's size.
    pub fn set_fixed_size(&self, id: ViewId, size: Option<(i32, i32)>) {
        let mut inner = self.inner();
        match size {
            Some(size) => {
                inner.fixed.insert(id, size);
            }
            None => {
                inner.fixed.remove(&id);
            }
        }
        inner.relayout(self.views_cell());
    }

    /// How many moves plan view `id` has reported to the workspace (its ViewMoved): a view moved
    /// by the link never reports the move it was given.
    pub fn reported_moves(&self, id: ViewId) -> usize {
        self.inner().reported.get(&id).copied().unwrap_or(0)
    }

    /// Lays the docks out in `area` as a draw would, without drawing: the views are sized as they
    /// will be seen.
    pub fn set_area(&self, area: Rect) {
        let mut inner = self.inner();
        inner.area = Some(area);
        inner.relayout(self.views_cell());
    }

    // ---- views -----------------------------------------------------------------------------

    /// The open views, shared as the window keeps them; borrow for the read and let go.
    pub fn view_set(&self) -> Rc<RefCell<ViewSet>> {
        Rc::clone(&self.shared.views)
    }

    pub fn active_id(&self) -> ViewId {
        self.views_cell().borrow().active_id()
    }

    /// A copy of view `id`'s state, for a reader that does not want to hold the set.
    pub fn state(&self, id: ViewId) -> Option<ViewState> {
        self.views_cell().borrow().find(id).cloned()
    }

    /// `f` with view `id`'s state, to change it as a test or the window changes a ViewState.
    pub fn with_state<R>(&self, id: ViewId, f: impl FnOnce(&mut ViewState) -> R) -> Option<R> {
        self.views_cell().borrow_mut().find_mut(id).map(f)
    }

    /// A new view of `kind`, splitting the active view into equal halves - side by side when it is
    /// wider than it is tall, stacked when taller - or on its own when no view is docked. Made
    /// active when `activate` is true, and also when the active view is hidden (minimised).
    pub fn open_view(&self, cx: &mut dyn PlanViewHost, kind: ViewKind, activate: bool) -> ViewId {
        let mut inner = self.inner();
        let id = inner.open_view(self.views_cell(), kind, activate, Some(&mut *cx));
        inner.drain_all(self.views_cell(), Some(&mut *cx));
        id
    }

    /// Closes a view: its widget dropped and its state too. When it was the active view, the one
    /// activated in its place is one the person can see. NotFound for an id that is not open.
    pub fn close_view(&self, cx: &mut dyn PlanViewHost, id: ViewId) -> Status {
        let mut inner = self.inner();
        let closed = inner.close_view(self.views_cell(), id, cx);
        inner.drain_all(self.views_cell(), Some(&mut *cx));
        drop(inner);
        if closed.is_ok() {
            self.close_popup_of(id);
        }
        closed
    }

    /// Changes what a view shows, keeping its dock where it is and the rest of its state.
    pub fn set_view_kind(&self, cx: &mut dyn PlanViewHost, id: ViewId, kind: ViewKind) -> Status {
        let mut inner = self.inner();
        let changed = inner.set_view_kind(self.views_cell(), id, kind, cx);
        inner.drain_all(self.views_cell(), Some(&mut *cx));
        changed
    }

    /// The keyboard focus has arrived in view `id` for `reason`, from `previous` (None: no widget
    /// held it). egui keeps no reason, so the window says which way the focus came; the view is
    /// made active when that is the person choosing it (view_focus's rule), a view floating when
    /// its dock is a window of its own.
    pub fn focus_arrived(&self, id: ViewId, reason: FocusReason, previous: Option<FocusPrevious>) {
        let mut inner = self.inner();
        let floats = inner.floating(id);
        if let Some(view) = inner.find_mut(id) {
            if let Some(plan) = &mut view.plan {
                plan.focus_arrived(reason, previous, floats);
            } else if let Some(render) = &mut view.render {
                render.focus_arrived(reason, previous, floats);
            } else if let Some(section) = &mut view.section {
                section.focus_arrived(reason, previous, floats);
            }
        }
        inner.drain_all(self.views_cell(), None);
    }

    /// Whether view `id`'s dock is a window of its own.
    pub fn is_floating(&self, id: ViewId) -> bool {
        self.inner().floating(id)
    }

    /// NotFound for an id that is not open.
    pub fn activate_view(&self, id: ViewId) -> Status {
        let mut inner = self.inner();
        if inner.find(id).is_none() {
            return Err(no_such_view(id));
        }
        inner.activate(self.views_cell(), id);
        Ok(())
    }

    /// The view of `kind` the person used most recently, shown and raised; or a new one beside the
    /// active view when none is open. Never turns a view the person is working in into another.
    pub fn ensure_view(&self, cx: &mut dyn PlanViewHost, kind: ViewKind) -> ViewId {
        let recent = self
            .views_cell()
            .borrow()
            .most_recent(kind)
            .map(|state| state.id);
        let mut inner = self.inner();
        if let Some(id) = recent {
            if inner.find(id).is_some() {
                inner.bring_back(self.views_cell(), id);
            }
            return id;
        }
        let id = inner.open_view(self.views_cell(), kind, false, Some(&mut *cx));
        inner.drain_all(self.views_cell(), Some(&mut *cx));
        id
    }

    /// Re-docks the open views into a preset arrangement (dock_splits), opening views of the
    /// default kinds when fewer are docked than the preset has places. Extra docked views are
    /// tabbed onto the last place; floating views stay where the person put them; a minimised
    /// docked view takes its place and leaves the tray. A maximised view is restored first.
    pub fn arrange(&self, cx: &mut dyn PlanViewHost, kind: LayoutKind) {
        let mut inner = self.inner();
        let views = self.views_cell();
        {
            let Inner { chrome, layout, .. } = &mut *inner;
            if let Some(chrome) = chrome {
                chrome.unmaximise_docked(layout);
            }
        }
        let places = cell_count(kind);
        let mut ids: Vec<ViewId> = inner
            .docks
            .iter()
            .map(|view| view.id)
            .filter(|&id| !inner.floating(id))
            .collect();
        // The kinds the arrangement expects and the person does not have yet, in the preset's
        // order: plan, 3D, section, elevation.
        for slot in 0..4 {
            if ids.len() >= places {
                break;
            }
            let wanted = default_view_kind(slot);
            let present = ids.iter().any(|id| {
                views
                    .borrow()
                    .find(*id)
                    .is_some_and(|state| state.kind == wanted)
            });
            if !present {
                ids.push(inner.open_view(views, wanted, false, Some(&mut *cx)));
            }
        }
        while ids.len() < places {
            let wanted = default_view_kind(ids.len());
            ids.push(inner.open_view(views, wanted, false, Some(&mut *cx)));
        }
        // The docked views taken out, and put back as the preset's steps say: each halves a view
        // already placed, the new one to its right or below it.
        let leaving: BTreeSet<String> = ids.iter().map(|id| dock_name(*id)).collect();
        crate::dock_chrome::remove_tabs(&mut inner.layout, |tab| leaving.contains(&tab.0));
        let mut tree = Tree::new(vec![ViewTab(dock_name(ids[0]))]);
        for step in dock_splits(kind) {
            let existing = dock_name(ids[step.existing]);
            if let Some((node, _)) = tree.find_tab_from(|tab| tab.0 == existing) {
                let tab = vec![ViewTab(dock_name(ids[step.added]))];
                if step.side_by_side {
                    let _ = tree.split_right(node, 0.5, tab);
                } else {
                    let _ = tree.split_below(node, 0.5, tab);
                }
            }
        }
        let last = dock_name(ids[places - 1]);
        for extra in ids.iter().skip(places) {
            if let Some((node, _)) = tree.find_tab_from(|tab| tab.0 == last)
                && let Ok(leaf) = tree.leaf_mut(node)
            {
                leaf.append_tab(ViewTab(dock_name(*extra)));
            }
        }
        *inner.layout.main_surface_mut() = tree;
        {
            let Inner { chrome, layout, .. } = &mut *inner;
            if let Some(chrome) = chrome {
                chrome.refresh(layout);
            }
        }
        inner.relayout(views);
        inner.push(WorkspaceEvent::ViewsChanged);
        inner.drain_all(views, Some(&mut *cx));
    }

    /// Arranges the views that are open, each in a place of its own of equal size - a grid, side
    /// by side or stacked - opening none and closing none: what VIEWS TILE does, which is how the
    /// View menu's Tile items reach it (docs/desktop.md, "Tiling the open views"). The active
    /// view takes the first place and the rest follow in the order opened; a floating view is
    /// docked into the tiling, a minimised one stays in the tray, a maximised one is restored
    /// first. One docked view, or none, is left as it is, and counted all the same.
    pub fn tile(&self, kind: TileKind) -> TileOutcome {
        let mut inner = self.inner();
        let outcome = inner.tile(self.views_cell(), kind);
        inner.drain_all(self.views_cell(), None);
        outcome
    }

    /// The open views' ids, in the order they were opened.
    pub fn view_ids(&self) -> Vec<ViewId> {
        self.inner().docks.iter().map(|view| view.id).collect()
    }

    pub fn is_plan_view(&self, id: ViewId) -> bool {
        self.inner()
            .find(id)
            .is_some_and(|view| view.plan.is_some())
    }

    /// The active view's kind; Plan when none is open.
    pub fn active_view_kind(&self) -> ViewKind {
        self.views_cell()
            .borrow()
            .active()
            .map_or(ViewKind::Plan, |state| state.kind)
    }

    /// The active view when it is a plan view, else the plan view used most recently, else None:
    /// what Plot, F9 and the Standard Views act on.
    pub fn active_plan_view(&self) -> Option<ViewId> {
        self.inner().active_plan(self.views_cell())
    }

    /// The same for the 3D and elevation views.
    pub fn active_render_view(&self) -> Option<ViewId> {
        let set = self.views_cell().borrow();
        let state = match set.active() {
            Some(active) if is_render_kind(active.kind) => Some(active),
            _ => set
                .most_recent(ViewKind::Model3D)
                .or_else(|| set.most_recent(ViewKind::Elevation)),
        };
        let id = state.map(|state| state.id)?;
        drop(set);
        self.inner()
            .find(id)
            .is_some_and(|view| view.render.is_some())
            .then_some(id)
    }

    pub fn active_section_view(&self) -> Option<ViewId> {
        let id = self
            .views_cell()
            .borrow()
            .most_recent(ViewKind::Section)
            .map(|state| state.id)?;
        self.inner()
            .find(id)
            .is_some_and(|view| view.section.is_some())
            .then_some(id)
    }

    /// Every open plan view, in the order opened.
    pub fn plan_views(&self) -> Vec<ViewId> {
        self.inner().plan_ids()
    }

    /// `f` with plan view `id` and its state; None for an id that is no open plan view. Its events
    /// are handled afterwards, without the drawing (what needs it waits for the next call).
    pub fn with_plan_view<R>(
        &self,
        id: ViewId,
        f: impl FnOnce(&mut PlanView, &mut ViewState) -> R,
    ) -> Option<R> {
        let mut inner = self.inner();
        let result = inner.with_plan(self.views_cell(), id, f);
        inner.drain_plan(self.views_cell(), id, None);
        result
    }

    /// The same, lent the drawing, so that what the view raises is handled at once as the C++
    /// hooks were.
    pub fn with_plan_view_lent<R>(
        &self,
        cx: &mut dyn PlanViewHost,
        id: ViewId,
        f: impl FnOnce(&mut PlanView, &mut ViewState, &mut dyn PlanViewHost) -> R,
    ) -> Option<R> {
        let mut inner = self.inner();
        let result = inner.with_plan(self.views_cell(), id, |plan, state| f(plan, state, cx));
        inner.drain_all(self.views_cell(), Some(&mut *cx));
        result
    }

    /// `f` with 3D or elevation view `id`, its state and the scene; None for no such view.
    pub fn with_render_view<R>(
        &self,
        id: ViewId,
        document: &Document,
        scenery: Scenery<'_>,
        f: impl FnOnce(&mut RenderView, &RenderScene<'_>, &mut ViewState) -> R,
    ) -> Option<R> {
        let mut inner = self.inner();
        let scene = RenderScene {
            document,
            surfaces: scenery.surfaces,
            meshes: scenery.meshes,
        };
        let result = {
            let render = inner.find_mut(id).and_then(|view| view.render.as_mut())?;
            let mut set = self.views_cell().borrow_mut();
            let state = set.find_mut(id)?;
            f(render, &scene, state)
        };
        inner.drain_render(self.views_cell(), id);
        Some(result)
    }

    /// `f` with section view `id` and its state; None for no such view.
    pub fn with_section_view<R>(
        &self,
        id: ViewId,
        f: impl FnOnce(&mut SectionView, &mut ViewState) -> R,
    ) -> Option<R> {
        let mut inner = self.inner();
        let result = {
            let section = inner.find_mut(id).and_then(|view| view.section.as_mut())?;
            let mut set = self.views_cell().borrow_mut();
            let state = set.find_mut(id)?;
            f(section, state)
        };
        inner.drain_section(self.views_cell(), id);
        Some(result)
    }

    // ---- what every view is given ----------------------------------------------------------

    /// The scene options every 3D view is given; never a view's layers, which are its own.
    pub fn set_scene_options(&self, document: &Document, options: SceneOptions<'static>) {
        let mut inner = self.inner();
        inner.options = SceneOptions {
            layers: None,
            ..options
        };
        let options = inner.options;
        for view in &mut inner.docks {
            if let Some(render) = &mut view.render {
                render.set_context(document, options);
            }
        }
    }

    pub fn scene_options(&self) -> SceneOptions<'static> {
        self.inner().options
    }

    /// The window's surfaces or meshes changed (setSurfaces, setMeshes): every 3D view rebuilds
    /// its scene, and every plan view draws the meshes' footprints again.
    pub fn scenery_changed(&self) {
        let mut inner = self.inner();
        for view in &mut inner.docks {
            if let Some(render) = &mut view.render {
                render.invalidate_scene();
            }
        }
    }

    /// The window's reference data was added to, removed from or changed (setReferenceData,
    /// invalidateReferenceCache): every plan view's imagery is painted again, every 3D view
    /// rebuilt, and the Layers buttons count what still exists.
    pub fn invalidate_reference_cache(&self, reference: Option<&ReferenceData>) {
        let mut inner = self.inner();
        inner.references = reference_ids(reference);
        for view in &mut inner.docks {
            if let Some(plan) = &mut view.plan {
                plan.invalidate_reference_cache();
            }
            if let Some(render) = &mut view.render {
                render.invalidate_scene();
            }
        }
        inner.update_bars(self.views_cell());
    }

    /// How many layers and reference layers a view hides that still exist: what its Layers
    /// button reports. 0 for an id that is not open.
    pub fn hidden_count(&self, id: ViewId) -> usize {
        let references = self.inner().references.clone();
        self.views_cell()
            .borrow()
            .find(id)
            .map_or(0, |state| hidden_count_of(state, &references))
    }

    /// Shows `section` in the section view used most recently, opening one beside the active view
    /// when none is open; `design`, the design profile cut beside the ground, is kept in the view's
    /// state with the section's own alignment: the recipe that cuts it again.
    pub fn show_section(
        &self,
        cx: &mut dyn PlanViewHost,
        section: Section,
        design: Option<SectionDesign>,
    ) -> bool {
        let id = self.ensure_view(cx, ViewKind::Section);
        let shown = self.with_section_view(id, |view, state| {
            view.set_section(state, Some(cx.document()), section);
            state.section_design = design;
        });
        shown.is_some()
    }

    /// Cuts again, with `cutter`, every section the views hold that was cut from a generation of
    /// the drawing's surfaces other than the present one - a section view shown or not, each from
    /// its own recipe. A view whose section a person has panned or zoomed keeps their pan and zoom.
    /// A section of the present generation is not cut again, so asking twice cuts once.
    pub fn recut_sections(
        &self,
        document: &Document,
        cutter: &mut dyn FnMut(&Polyline2, Option<&SectionDesign>) -> Result<Section>,
    ) -> SectionRecut {
        let generation = document.surfaces_generation();
        let mut result = SectionRecut::default();
        let ids: Vec<ViewId> = self
            .views_cell()
            .borrow()
            .views()
            .iter()
            .map(|state| state.id)
            .collect();
        for id in ids {
            let recipe = self.views_cell().borrow().find(id).and_then(|state| {
                let section = state.section.as_ref()?;
                (state.section_surfaces != generation)
                    .then(|| (section.alignment.clone(), state.section_design.clone()))
            });
            let Some((alignment, design)) = recipe else {
                continue;
            };
            let section = match cutter(&alignment, design.as_ref()) {
                Ok(section) => section,
                Err(error) => {
                    result.refused.push((id, error));
                    continue;
                }
            };
            let has_widget = self.inner().find(id).is_some_and(|view| view.section.is_some());
            if has_widget {
                self.with_section_view(id, |view, state| {
                    view.replace_section(state, Some(document), section);
                });
            } else if let Some(state) = self.views_cell().borrow_mut().find_mut(id) {
                // A section a view turned to another kind keeps, to show again.
                state.section = Some(section);
                state.section_surfaces = generation;
            }
            result.cut.push(id);
        }
        result
    }

    // ---- the tools -------------------------------------------------------------------------

    /// Starts catalogue tool `id` in the active plan view, first stopping, as Esc stops it, a tool
    /// running in any other plan view. NotFound for an id the catalogue does not have;
    /// InvalidState when no plan view is open.
    pub fn start_tool(&self, cx: &mut dyn PlanViewHost, id: &str) -> Status {
        let mut inner = self.inner();
        inner.build_unbuilt(self.views_cell(), cx);
        let started = inner.start_tool(self.views_cell(), cx, id);
        inner.drain_all(self.views_cell(), Some(&mut *cx));
        started
    }

    /// The running tool's id in whichever plan view runs one, "" when none.
    pub fn active_tool_id(&self) -> String {
        let inner = self.inner();
        inner
            .running_plan()
            .and_then(|id| inner.find(id))
            .and_then(|view| view.plan.as_ref())
            .map(PlanView::active_tool_id)
            .unwrap_or_default()
    }

    /// True when the running tool waits for typed text (ToolInput::Value) rather than a point or a
    /// pick: the command line then hands it a typed line before reading it as any verb.
    pub fn tool_takes_text(&self, document: &Document) -> bool {
        let inner = self.inner();
        inner
            .running_plan()
            .and_then(|id| inner.find(id))
            .and_then(|view| view.plan.as_ref())
            .is_some_and(|plan| plan.tool_expects(document) == Some(ToolInput::Value))
    }

    /// The tool Enter at no prompt would run again, in any plan view; "" until one has run.
    pub fn last_tool_id(&self) -> String {
        self.inner().last_tool_id.clone()
    }

    /// Hands a whole typed line to the running tool - the command line's Enter while a tool runs;
    /// a ZOOM typed there runs as its line through the window's executor, the tool at its step.
    /// False, and nothing done, when no tool runs.
    pub fn type_into_tool(&self, host: &mut dyn Host, scenery: Scenery<'_>, text: &str) -> bool {
        let taken = {
            let mut inner = self.inner();
            let Some(id) = inner.running_plan() else {
                return false;
            };
            let mut cx = HostDrawing {
                host: &mut *host,
                meshes: scenery.meshes,
            };
            let taken = inner
                .with_plan(self.views_cell(), id, |plan, state| {
                    plan.type_into_tool(&mut cx, state, text)
                })
                .unwrap_or(false);
            inner.drain_all(self.views_cell(), Some(&mut cx));
            taken
        };
        self.run_queued(host);
        taken
    }

    /// Select: stops the tool running in any plan view, as Esc stops it.
    pub fn stop_tool(&self, cx: &mut dyn PlanViewHost) {
        let mut inner = self.inner();
        for id in inner.plan_ids() {
            inner.stop_tool_in(self.views_cell(), id, cx);
        }
        inner.drain_all(self.views_cell(), Some(&mut *cx));
    }

    /// Where Enter and the headless pointer go: the plan view running a tool, else the active plan
    /// view; None when none is open.
    pub fn command_target_view(&self) -> Option<ViewId> {
        let inner = self.inner();
        inner
            .running_plan()
            .or_else(|| inner.active_plan(self.views_cell()))
    }

    /// Enter pressed on an empty command line is Enter pressed in the drawing: delivered as the
    /// key itself to the plan view running a tool, or with none running to the active plan view,
    /// which starts the last tool again. False when no plan view is open.
    pub fn press_enter(&self, cx: &mut dyn PlanViewHost) -> bool {
        let Some(id) = self.command_target_view() else {
            return false;
        };
        let mut inner = self.inner();
        inner.with_plan(self.views_cell(), id, |plan, state| {
            plan.key_press(cx, state, ViewKey::Enter, KeyModifiers::NONE, "")
        });
        inner.drain_all(self.views_cell(), Some(&mut *cx));
        true
    }

    /// The pointer at model point `at`, as a mouse moves it there (and, with `click`, clicks), in
    /// the command target view: what the headless --hover and --click steps do. InvalidState when
    /// no plan view is open.
    pub fn pointer_at(
        &self,
        cx: &mut dyn PlanViewHost,
        at: Point2,
        click: bool,
        modifiers: KeyModifiers,
    ) -> Result<String> {
        let Some(id) = self.command_target_view() else {
            return Err(make_error(
                ErrorCode::InvalidState,
                "there is no plan view to point in",
                "",
            ));
        };
        let mut inner = self.inner();
        let record = inner
            .with_plan(self.views_cell(), id, |plan, state| {
                plan.pointer_at(cx, state, at, click, modifiers)
            })
            .unwrap_or_else(|| Err(no_such_view(id)));
        inner.drain_all(self.views_cell(), Some(&mut *cx));
        record
    }

    /// Whether `line`, typed now, is a ZOOM for the view rather than an answer for the tool running
    /// in a plan view.
    pub fn runs_transparently(&self, document: &Document, line: &str) -> bool {
        if self.active_tool_id().is_empty() {
            return false;
        }
        is_transparent_command(line, self.tool_takes_text(document))
            && !transparent_zoom_line(line).is_empty()
    }

    // ---- settings every plan view shares ---------------------------------------------------

    pub fn set_grid_visible(&self, visible: bool) {
        let mut inner = self.inner();
        inner.grid_visible = visible;
        inner.options.draw_grid = visible;
        let options = inner.options;
        for view in &mut inner.docks {
            if let Some(plan) = &mut view.plan {
                plan.set_grid_visible(visible);
            }
            if let Some(render) = &mut view.render {
                render.invalidate_scene();
                let _ = options;
            }
        }
    }

    pub fn grid_visible(&self) -> bool {
        self.inner().grid_visible
    }

    /// The document's drafting settings hold the snaps: the drafting toolbar and SNAP set the same
    /// ones, so the views keep no copy that could overwrite them.
    pub fn set_snap_enabled(&self, cx: &mut dyn PlanViewHost, enabled: bool) {
        cx.document_mut().drafting_mut().snap_enabled = enabled;
        {
            let mut inner = self.inner();
            for view in &mut inner.docks {
                if let Some(plan) = &mut view.plan {
                    plan.set_snap_enabled(cx, enabled);
                }
            }
        }
        cx.document().notify_drafting_changed();
    }

    pub fn snap_enabled(document: &Document) -> bool {
        document.drafting().snap_enabled
    }

    pub fn set_snap_modes(document: &mut Document, modes: SnapModes) {
        document.drafting_mut().snap_modes = modes;
        document.notify_drafting_changed();
    }

    /// Esc: the plan views running a tool (or holding typed input for one) cancel it; when none
    /// is, every plan view cancels (clears its box and the selection). Only the views busy with a
    /// tool hear it: another clearing the selection at the same key press would take away the
    /// selection the tool was started on, where AutoCAD's second Esc does.
    pub fn cancel(&self, cx: &mut dyn PlanViewHost) {
        let mut inner = self.inner();
        let plans = inner.plan_ids();
        let busy: Vec<ViewId> = plans
            .iter()
            .copied()
            .filter(|&id| {
                inner
                    .find(id)
                    .and_then(|view| view.plan.as_ref())
                    .is_some_and(|plan| plan.tool_active() || !plan.typed_input().is_empty())
            })
            .collect();
        let hearing = if busy.is_empty() { plans } else { busy };
        for id in hearing {
            inner.with_plan(self.views_cell(), id, |plan, state| plan.cancel(cx, state));
        }
        inner.drain_all(self.views_cell(), Some(&mut *cx));
    }

    /// Every plan view, floating and minimised ones included: the clicks collected anywhere belong
    /// to the drawing that is going away.
    pub fn reset_interaction(&self, cx: &mut dyn PlanViewHost) {
        let mut inner = self.inner();
        for view in &mut inner.docks {
            if let Some(plan) = &mut view.plan {
                plan.reset_interaction(cx);
            }
        }
        inner.drain_all(self.views_cell(), Some(&mut *cx));
    }

    /// The drawing was replaced (File > New, Open): every 3D view rebuilds its WHOLE scene, and
    /// every section, docked or kept by a view of another kind, is dropped - it was cut from
    /// surfaces that are gone.
    pub fn drawing_replaced(&self) {
        // States first: a view turned from a section into a plan keeps its section to show again.
        for state in self.views_cell().borrow_mut().views_mut() {
            state.section = None;
            state.section_design = None;
        }
        let mut inner = self.inner();
        let mut set = self.views_cell().borrow_mut();
        for view in &mut inner.docks {
            if let Some(render) = &mut view.render {
                render.invalidate_scene();
            } else if let Some(section) = &mut view.section
                && let Some(state) = set.find_mut(view.id)
            {
                section.clear_section(state);
            }
        }
    }

    // ---- the zoom --------------------------------------------------------------------------

    /// Frames one view, active or not: its bar's Zoom Extents, ZOOM EXTENTS and a plan view's
    /// middle double-click. The views linked with it follow, and a linked view that draws nothing
    /// frames what the link draws. NotFound for an id that is not open.
    pub fn zoom_extents(
        &self,
        cx: &mut dyn PlanViewHost,
        scenery: Scenery<'_>,
        id: ViewId,
    ) -> Status {
        let mut inner = self.inner();
        let framed = inner.zoom_extents(self.views_cell(), id, cx, scenery);
        inner.drain_all(self.views_cell(), Some(&mut *cx));
        framed
    }

    /// Frames every view: after New, Open and an import. The link is framed once, on everything
    /// its views draw between them, in the member the person moved last (else the lowest id), and
    /// the others follow it.
    pub fn zoom_extents_all(&self, cx: &mut dyn PlanViewHost) {
        let mut inner = self.inner();
        inner.zoom_extents_all(self.views_cell(), cx);
        inner.drain_all(self.views_cell(), Some(&mut *cx));
    }

    /// Frames `bounds` in every plan view; in the link, once, in the member that frames it.
    pub fn zoom_to(&self, bounds: &Box2) {
        self.inner().zoom_to(self.views_cell(), bounds);
    }

    // ---- linked views and the bars' lines --------------------------------------------------

    /// Lines a bar or a typed zoom asked for, run through the window's executor.
    fn run_queued(&self, host: &mut dyn Host) {
        loop {
            let lines = {
                let inner = self.inner();
                let mut queued = inner.lines.borrow_mut();
                std::mem::take(&mut *queued)
            };
            if lines.is_empty() {
                return;
            }
            for line in lines {
                let _ = host.run(&line);
            }
        }
    }

    /// What a view's Link button does: VIEWS UNLINK <id> when the view is linked, else VIEWS LINK
    /// <id> TO <the view the person moved last>, through the window's executor. Every Link button
    /// is then set from the link as it stands, so a refused line leaves each as it was.
    pub fn toggle_link(&self, host: &mut dyn Host, id: ViewId) {
        let line = {
            let set = self.views_cell().borrow();
            let Some(state) = set.find(id) else {
                return;
            };
            // The view the person moved last leads, whichever of the two is clicked first.
            if state.linked {
                format!("VIEWS UNLINK {id}")
            } else {
                format!("VIEWS LINK {id} TO {}", set.link_leader_for(id))
            }
        };
        let _ = host.run(&line);
        self.inner().update_link_buttons(self.views_cell());
    }

    /// Frames the selection in view `id` by its Zoom to Selection line, ZOOM SELECTION view=<id>,
    /// through the window's executor: logged, and followed by the views linked with it.
    pub fn zoom_to_selection(&self, host: &mut dyn Host, id: ViewId) {
        let _ = host.run(&format!("ZOOM SELECTION view={id}"));
    }

    /// What the Layers popup's "Show the selection on hidden layers" box does: VIEWS SET <id>
    /// ghosts=on|off, through the window's executor.
    pub fn set_selection_ghosts(&self, host: &mut dyn Host, id: ViewId, on: bool) {
        let _ = host.run(&format!(
            "VIEWS SET {id} ghosts={}",
            if on { "on" } else { "off" }
        ));
    }

    /// VIEWS SHOW|HIDE <id> <layers>, through the window's executor; a line that cannot be made
    /// is refused as one the interpreter refused.
    pub fn set_view_layers_shown(
        &self,
        host: &mut dyn Host,
        id: ViewId,
        layers: &[String],
        shown: bool,
    ) -> VerbOutcome {
        match view_layers_line(if shown { "SHOW" } else { "HIDE" }, id, layers) {
            Ok(line) => host.run(&line),
            Err(error) => VerbOutcome::new(false, "", &error.describe()),
        }
    }

    /// VIEWS SHOW <id> ALL: the layers and the reference layers.
    pub fn show_everything_in_view(&self, host: &mut dyn Host, id: ViewId) -> VerbOutcome {
        host.run(&format!("VIEWS SHOW {id} ALL"))
    }

    /// VIEWS ISOLATE <id> <layer>.
    pub fn isolate_layer_in_view(
        &self,
        host: &mut dyn Host,
        id: ViewId,
        layer: &str,
    ) -> VerbOutcome {
        match view_layers_line("ISOLATE", id, &[String::from(layer)]) {
            Ok(line) => host.run(&line),
            Err(error) => VerbOutcome::new(false, "", &error.describe()),
        }
    }

    /// VIEWS SHOW|HIDE <id> REFERENCE <ref>.
    pub fn set_view_reference_shown(
        &self,
        host: &mut dyn Host,
        id: ViewId,
        reference: u64,
        shown: bool,
    ) -> VerbOutcome {
        host.run(&view_references_line(
            if shown { "SHOW" } else { "HIDE" },
            id,
            &[reference],
        ))
    }

    /// Drops per-view hidden layers that name no layer any more (after a delete or a rename), in
    /// every view.
    pub fn prune_view_layers(&self, document: &Document) {
        let changed: Vec<ViewId> = self
            .views_cell()
            .borrow_mut()
            .views_mut()
            .into_iter()
            .filter_map(|state| {
                (state.layers.prune_missing(&document.model().layers) > 0).then_some(state.id)
            })
            .collect();
        let mut inner = self.inner();
        for id in changed {
            inner.view_settings_changed(self.views_cell(), id);
        }
    }

    /// One view's own settings changed (its hidden layers, references or ghosts), set from
    /// outside the verbs: repainted, and its Layers button told.
    pub fn view_settings_changed(&self, id: ViewId) {
        self.inner().view_settings_changed(self.views_cell(), id);
    }

    // ---- hooks ---------------------------------------------------------------------------

    /// What the views raised since this was last asked, in order.
    pub fn take_events(&self) -> Vec<WorkspaceEvent> {
        std::mem::take(&mut self.inner().events)
    }

    /// Whether the window shows a plan view's shortcut menu (onContextMenu set). Without a taker a
    /// right-click cancels, as a view with no handler does.
    pub fn set_context_menu_taker(&self, takes: bool) {
        self.inner().context_menu_taker = takes;
    }

    // ---- the per-view dialogs --------------------------------------------------------------

    /// Opens the Layers popup of view `id`, under its Layers button - its bar's right end, as the
    /// area was last laid out; false for an id that is not open. One at a time: a popup closes on
    /// a click outside it.
    pub fn show_layers_popup(
        &self,
        document: &Document,
        reference: Option<&ReferenceData>,
        id: ViewId,
    ) -> bool {
        let anchor = {
            let inner = self.inner();
            if inner.find(id).is_none() {
                return false;
            }
            let origin = inner.area.map_or(Pos2::ZERO, |area| area.min);
            // Docked or floating: under the view's own bar, where its Layers button is.
            inner.place_of(id).map_or(origin, |place| {
                origin + Vec2::new(place.max.x, place.min.y + BAR_HEIGHT)
            })
        };
        let popup = ViewLayersPopup::new(self, document, reference, id, anchor);
        *self.shared.popup.borrow_mut() = Some(popup);
        true
    }

    /// `f` with the open Layers popup and this workspace; None when none is open.
    pub fn with_layers_popup<R>(
        &self,
        f: impl FnOnce(&mut ViewLayersPopup, &ViewWorkspace) -> R,
    ) -> Option<R> {
        let mut popup = self.shared.popup.borrow_mut().take()?;
        let result = f(&mut popup, self);
        let mut slot = self.shared.popup.borrow_mut();
        if slot.is_none() && popup.is_open() {
            *slot = Some(popup);
        }
        Some(result)
    }

    pub fn layers_popup_open(&self) -> bool {
        self.shared
            .popup
            .borrow()
            .as_ref()
            .is_some_and(ViewLayersPopup::is_open)
    }

    fn close_popup_of(&self, id: ViewId) {
        let mut slot = self.shared.popup.borrow_mut();
        if slot.as_ref().is_some_and(|popup| popup.view() == id) {
            *slot = None;
        }
    }

    /// The same non-modal command builder for View > Paper Style Scale and the plan view's own
    /// bar; false unless `id` is an open plan view.
    pub fn show_scale_dialog(&self, id: ViewId) -> bool {
        let is_plan = self
            .views_cell()
            .borrow()
            .find(id)
            .is_some_and(|state| state.kind == ViewKind::Plan);
        if !is_plan {
            return false;
        }
        let mut inner = self.inner();
        let mut dialog = inner
            .scale_dialog
            .take()
            .unwrap_or_else(|| ViewScaleDialog::new(Rc::clone(&self.shared.views), id));
        dialog.refresh(id);
        dialog.show();
        inner.scale_dialog = Some(dialog);
        true
    }

    /// `f` with the Paper Style Scale dialog; None until it has been opened.
    pub fn with_scale_dialog<R>(&self, f: impl FnOnce(&mut ViewScaleDialog) -> R) -> Option<R> {
        let mut dialog = self.inner().scale_dialog.take()?;
        let result = f(&mut dialog);
        self.inner().scale_dialog = Some(dialog);
        Some(result)
    }

    // ---- a title bar's shortcut menu -------------------------------------------------------

    /// The rows of view `id`'s title bar menu as it would show now: Float or Dock, Minimise,
    /// Maximise or Restore and Close - the bar's own buttons, by `BAR_MENU_FLOAT` and its
    /// siblings - then the three tilings, by the names the View menu has them under. None for a
    /// view that is not open, or a workspace with no chrome.
    pub fn bar_menu(&self, id: ViewId) -> Option<Menu> {
        self.inner().bar_menu_rows(self.views_cell(), id)
    }

    /// Opens view `id`'s title bar menu, as a right-click on its bar does: at `at` (the window's
    /// points), or under the bar's left end when no place is given - the door a run with no
    /// pointer has. The view is made active: the menu acts on it. False for a view with no menu.
    pub fn show_bar_menu(&self, id: ViewId, at: Option<Pos2>) -> bool {
        let mut inner = self.inner();
        if inner.bar_menu_rows(self.views_cell(), id).is_none() {
            return false;
        }
        inner.activate(self.views_cell(), id);
        let origin = inner.area.map_or(Pos2::ZERO, |area| area.min);
        let under_bar = inner.place_of(id).map_or(origin, |place| {
            origin + place.min.to_vec2() + Vec2::new(6.0, BAR_HEIGHT)
        });
        inner.kind_menu = None;
        inner.bar_menu = Some((id, at.unwrap_or(under_bar)));
        true
    }

    /// The view whose title bar menu is open; None when none is.
    pub fn bar_menu_open(&self) -> Option<ViewId> {
        self.inner().bar_menu.map(|(id, _)| id)
    }

    /// Item `name` of view `id`'s title bar menu chosen, as a click on it: each goes the way the
    /// thing it stands for goes - Float, Minimise, Maximise and Close as the bar's own buttons
    /// do, a tiling by its VIEWS TILE line through the window's executor, logged as a typed one
    /// is. The menu closes. NotFound for a view with no menu and for a name it does not show.
    pub fn trigger_bar_menu(
        &self,
        host: &mut dyn Host,
        scenery: Scenery<'_>,
        id: ViewId,
        name: &str,
    ) -> Status {
        let Some(menu) = self.bar_menu(id) else {
            return Err(no_such_view(id));
        };
        if !menu.actions().iter().any(|action| action.name == name) {
            return Err(make_error(
                ErrorCode::NotFound,
                "a view's title bar menu has no such item",
                name,
            ));
        }
        self.inner().bar_menu = None;
        let dock = dock_name(id);
        match name {
            BAR_MENU_FLOAT => self.chrome_request(host, scenery, ChromeRequest::Float(dock)),
            BAR_MENU_MINIMISE => self.chrome_request(host, scenery, ChromeRequest::Minimise(dock)),
            BAR_MENU_MAXIMISE => self.chrome_request(host, scenery, ChromeRequest::Maximise(dock)),
            BAR_MENU_CLOSE => self.chrome_request(host, scenery, ChromeRequest::Close(dock)),
            _ => {
                if let Some(line) = tiling_line(name) {
                    let _ = host.run(&line);
                }
            }
        }
        Ok(())
    }

    // ---- the views between sessions --------------------------------------------------------

    /// What a session keeps of the views, as text (JSON, `SAVED_VIEWS_VERSION`): each open view's
    /// id and what it shows, in the order opened, whether it is linked and shows the selection's
    /// ghosts, the active view, the docked arrangement - every split and its share, every
    /// place's pages and the one in front - and each floating view's rectangle.
    ///
    /// A maximise is ended and every minimised view brought out of the tray first: both are a
    /// session's passing states, not a layout (docs/desktop.md, "The dock chrome"), so the views
    /// are recorded open, where they are or would come back to. This is called as the window
    /// closes; called at another time it leaves the tray empty.
    ///
    /// Not recorded, because they are a drawing's and not the window's: where a view looks (its
    /// plan centre and scale, its camera), the layers it hides, and its section. The paper style
    /// scale is the project's already, by view id (VIEWS SCALE) - which is why the ids are kept.
    pub fn saved_views(&self) -> String {
        self.inner().saved_views(self.views_cell())
    }

    /// Opens the views `text` records (`saved_views`), arranged as they were: how many. A view
    /// open now whose id the record names is kept as it is (its kind changed where the record's
    /// differs) and every other is closed; each view the record names that is not open is opened
    /// under its id, or under a new one when this session has given that id out already. A
    /// section view comes back with nothing cut: the drawing it was cut from is not this one.
    ///
    /// Refused, and NOTHING changed, for a text that is not what `saved_views` writes - not
    /// JSON, another version, more than `MOST_SAVED_VIEWS` views, a kind or a split it does not
    /// know, an arrangement that does not place each view once - so a damaged or stale record
    /// leaves the window the views it started with.
    pub fn restore_views(&self, cx: &mut dyn PlanViewHost, text: &str) -> Result<usize> {
        let saved = SavedViews::parse(text)?;
        let restored = {
            let mut inner = self.inner();
            let restored = inner.restore_views(self.views_cell(), &saved, &mut *cx);
            inner.drain_all(self.views_cell(), Some(&mut *cx));
            restored
        };
        // A popup of a view that may be gone.
        *self.shared.popup.borrow_mut() = None;
        // The project's paper style scales are kept by view id: read again, for the ids back.
        self.shared.notified.store(true, Ordering::Release);
        Ok(restored)
    }

    // ---- the frame -------------------------------------------------------------------------

    /// The workspace drawn into the rectangle `ui` has left: every docked view in its place with
    /// its bar, the floating views over it, the kind menu, the Layers popup and the Paper Style
    /// Scale dialog; then what was pressed is done - a bar's lines run through the window's
    /// executor - and what the views raised is handled.
    pub fn ui(&self, ui: &mut Ui, host: &mut dyn Host, scenery: Scenery<'_>) {
        let area = ui.available_rect_before_wrap();
        let _ = ui.allocate_rect(area, Sense::hover());
        {
            let mut cx = HostDrawing {
                host: &mut *host,
                meshes: scenery.meshes,
            };
            self.shared.look(&mut cx);
            let mut inner = self.inner();
            let views = self.views_cell();
            let ppp = ui.ctx().pixels_per_point();
            if inner.area != Some(area) || inner.pixels_per_point != ppp {
                inner.area = Some(area);
                inner.pixels_per_point = ppp;
                inner.relayout(views);
            }
            {
                let Inner { chrome, layout, .. } = &mut *inner;
                if let Some(chrome) = chrome {
                    chrome.refresh(layout);
                }
            }
            // The pointer, for what is asked once the frame is drawn (a bar's menu, the start
            // of a drag), and a floating view in hand moved with it before it is drawn.
            let (pointer, down) = ui.input(|input| {
                (input.pointer.interact_pos(), input.pointer.primary_down())
            });
            inner.pointer = pointer;
            inner.drag_floating(pointer, down);
            // The bars are brought up to date where what they show changes (a link, a kind, a
            // view's own settings, the active view), never at each frame: a tool a caller took
            // off a bar stays off, as the C++'s setToolWanted holds.
            draw_docks(&mut inner, views, ui, area, &mut cx, scenery);
            draw_kind_menu(&mut inner, views, ui, area);
            draw_bar_menu(&mut inner, views, ui);
        }
        self.after_frame(host, scenery);
        self.dialogs_ui(ui, host);
    }

    /// What the frame's presses asked, carried out once nothing is drawn.
    fn after_frame(&self, host: &mut dyn Host, scenery: Scenery<'_>) {
        let requests = {
            let mut inner = self.inner();
            inner
                .chrome
                .as_mut()
                .map(DockChrome::take_requests)
                .unwrap_or_default()
        };
        for request in requests {
            self.chrome_request(host, scenery, request);
        }
        // An item of a bar's shortcut menu, carried out as a press of it is.
        let item = self.inner().bar_menu_choice.take();
        if let Some((id, name)) = item {
            let _ = self.trigger_bar_menu(host, scenery, id, &name);
        }
        let chosen = self.inner().kind_choice.take();
        self.inner().kind_menu_dismissed = None;
        if let Some((id, kind)) = chosen {
            let mut cx = HostDrawing {
                host: &mut *host,
                meshes: scenery.meshes,
            };
            let _ = self.set_view_kind(&mut cx, id, kind);
            let _ = self.activate_view(id);
        }
        {
            let mut cx = HostDrawing {
                host: &mut *host,
                meshes: scenery.meshes,
            };
            let mut inner = self.inner();
            inner.drain_all(self.views_cell(), Some(&mut cx));
        }
        self.run_queued(host);
    }

    fn chrome_request(&self, host: &mut dyn Host, scenery: Scenery<'_>, request: ChromeRequest) {
        let views = self.views_cell();
        match request {
            ChromeRequest::Pressed(name) => {
                if let Some(id) = dock_id(&name) {
                    let mut inner = self.inner();
                    inner.activate(views, id);
                    // A floating view is moved by its title bar, as a window is.
                    inner.begin_float_drag(id);
                }
            }
            ChromeRequest::Menu(name) => {
                if let Some(id) = dock_id(&name) {
                    // A right-click on a view's bar is choosing that view: the menu acts on it.
                    let at = self.inner().pointer;
                    let _ = self.show_bar_menu(id, at);
                }
            }
            ChromeRequest::Minimise(name) => {
                if let Some(id) = dock_id(&name) {
                    self.minimise_view(id);
                }
            }
            ChromeRequest::Restore(name) => {
                if let Some(id) = dock_id(&name) {
                    let _ = self.restore_view(id);
                }
            }
            ChromeRequest::Maximise(name) => {
                if let Some(id) = dock_id(&name) {
                    self.toggle_maximised(id);
                }
            }
            ChromeRequest::Float(name) => {
                if let Some(id) = dock_id(&name) {
                    self.toggle_floating(id);
                }
            }
            ChromeRequest::Close(name) => {
                if let Some(id) = dock_id(&name) {
                    let mut cx = HostDrawing {
                        host: &mut *host,
                        meshes: scenery.meshes,
                    };
                    let _ = self.close_view(&mut cx, id);
                }
            }
            ChromeRequest::Tool(name, tool) => {
                let Some(id) = dock_id(&name) else {
                    return;
                };
                // Pressing a view's own tools is working in that view: it is active before the
                // tool acts.
                self.inner().activate(views, id);
                match tool.as_str() {
                    "ViewKindButton" => {
                        let mut inner = self.inner();
                        inner.kind_menu = if inner.kind_menu == Some(id)
                            || inner.kind_menu_dismissed == Some(id)
                        {
                            None
                        } else {
                            Some(id)
                        };
                    }
                    "ViewLinkButton" => self.toggle_link(host, id),
                    "ViewLayersButton" => {
                        let _ = self.show_layers_popup(host.document(), Some(host.reference()), id);
                    }
                    "ViewScaleButton" => {
                        let _ = self.show_scale_dialog(id);
                    }
                    "ViewZoomInButton" => {
                        let _ = host.run(&format!("ZOOM IN view={id}"));
                    }
                    "ViewZoomOutButton" => {
                        let _ = host.run(&format!("ZOOM OUT view={id}"));
                    }
                    "ViewZoomSelectionButton" => self.zoom_to_selection(host, id),
                    "ViewZoomExtentsButton" => {
                        let _ = host.run(&format!("ZOOM EXTENTS view={id}"));
                    }
                    _ => {}
                }
            }
        }
    }

    /// The Layers popup and the Paper Style Scale dialog, drawn after the docks.
    fn dialogs_ui(&self, ui: &mut Ui, host: &mut dyn Host) {
        let _ = self.with_layers_popup(|popup, workspace| popup.ui(ui, workspace, host));
        let dialog = self.inner().scale_dialog.take();
        if let Some(mut dialog) = dialog {
            dialog.ui(ui, host);
            self.inner().scale_dialog = Some(dialog);
        }
    }
}

impl Inner {
    /// The first view's widget, made from the drawing alone, before any host is lent: a plan view
    /// is made from a read of it.
    fn build_first(&mut self, views: &RefCell<ViewSet>, id: ViewId, document: &Document) {
        let grid = self.grid_visible;
        let lines = Rc::clone(&self.lines);
        let mut set = views.borrow_mut();
        let Some(state) = set.find_mut(id) else {
            return;
        };
        let Some(view) = self.docks.iter_mut().find(|view| view.id == id) else {
            return;
        };
        let mut plan = PlanView::new(document, state);
        plan.set_grid_visible(grid);
        wire_tools(&mut plan, id, lines);
        view.plan = Some(plan);
        drop(set);
        self.update_bar(views, id);
    }

    // ---- a title bar's shortcut menu -------------------------------------------------------

    /// The rows of view `id`'s title bar menu: what its window buttons do, worded as the buttons
    /// are for the view's state now (Float is Dock while it floats, Maximise is Restore while it
    /// is maximised), then the three tilings. None for a view that is not open or has no bar.
    fn bar_menu_rows(&self, views: &RefCell<ViewSet>, id: ViewId) -> Option<Menu> {
        self.find(id)?;
        let name = dock_name(id);
        let chrome = self.chrome.as_ref()?;
        let bar = chrome.title_bar(&name)?;
        let title = views.borrow().find(id).map(ViewSet::title)?;
        // As the view is now, not as the last frame's refresh found it: the menu may be asked
        // for between a Float and the next frame.
        let facts = BarFacts {
            floating: self.floating(id),
            maximised: chrome.is_maximised(&name),
        };
        let mut menu = Menu::new(BAR_MENU, &title).section("This view");
        let shown = bar.window_buttons();
        for (item, which) in [
            (BAR_MENU_FLOAT, WindowButton::Float),
            (BAR_MENU_MINIMISE, WindowButton::Minimise),
            (BAR_MENU_MAXIMISE, WindowButton::Maximise),
            (BAR_MENU_CLOSE, WindowButton::Close),
        ] {
            // A button the bar does not have (a view that may not float) has no item either.
            if !shown.contains(&which) {
                continue;
            }
            let button = bar.window_button(which, facts);
            menu.items.push(MenuItem::Action(
                Action::new(item, &button.label)
                    .icon(which.icon(facts))
                    .tip(&button.tip),
            ));
        }
        menu.items
            .push(MenuItem::Section(String::from("Arrange the open views")));
        menu.items
            .extend(tiling_actions().into_iter().map(MenuItem::Action));
        Some(menu)
    }

    // ---- the views between sessions --------------------------------------------------------

    /// What a saved session records of the views (`ViewWorkspace::saved_views`).
    fn saved_views(&mut self, views: &RefCell<ViewSet>) -> String {
        // A maximise and the tray are a session's, not a layout's: every view is recorded open,
        // in the place it has or would come back to.
        if let Some(chrome) = &mut self.chrome {
            chrome.unmaximise_all(&mut self.layout);
            for name in chrome.minimised_names() {
                let _ = chrome.restore(&mut self.layout, &name);
            }
            chrome.refresh(&mut self.layout);
        }
        self.relayout(views);
        let set = views.borrow();
        let records: Vec<Json> = set
            .views()
            .into_iter()
            .filter(|state| self.find(state.id).is_some())
            .map(|state| {
                Json::object_of([
                    ("id", Json::from(state.id)),
                    ("kind", Json::from(kind_word(state.kind))),
                    ("linked", Json::from(state.linked)),
                    ("ghosts", Json::from(state.selection_ghosts)),
                ])
            })
            .collect();
        let docked = saved_node(self.layout.main_surface(), NodeIndex::root())
            .map_or(Json::Null, |node| node.to_json());
        let floating: Vec<Json> = self
            .docks
            .iter()
            .filter_map(|view| self.float_rect(view.id).map(|rect| (view.id, rect)))
            .map(|(id, rect)| {
                Json::object_of([
                    ("view", Json::from(id)),
                    ("x", Json::from(f64::from(rect.min.x))),
                    ("y", Json::from(f64::from(rect.min.y))),
                    ("width", Json::from(f64::from(rect.width()))),
                    ("height", Json::from(f64::from(rect.height()))),
                ])
            })
            .collect();
        Json::object_of([
            ("version", Json::from(SAVED_VIEWS_VERSION)),
            ("views", Json::Array(records)),
            ("active", Json::from(set.active_id())),
            ("docked", docked),
            ("floating", Json::Array(floating)),
        ])
        .dump()
    }

    /// A view of a saved session opened again under the id it had - unless this session has
    /// already given that id out, when it gets a new one: an id is never reused (ViewSet). Its
    /// dock is placed by the arrangement being restored, not here.
    fn open_restored(
        &mut self,
        views: &RefCell<ViewSet>,
        id: ViewId,
        kind: ViewKind,
        cx: &mut dyn PlanViewHost,
    ) -> ViewId {
        let id = {
            let mut set = views.borrow_mut();
            let restored = if set.issued(id) {
                None
            } else {
                set.restore(id, kind).ok().map(|state| state.id)
            };
            match restored {
                Some(id) => id,
                None => set.add(kind).id,
            }
        };
        self.docks.push(View {
            id,
            plan: None,
            render: None,
            section: None,
        });
        self.install_chrome(views, id);
        self.build_content(views, id, cx);
        id
    }

    /// The views as `saved` has them (`ViewWorkspace::restore_views`): how many are open.
    fn restore_views(
        &mut self,
        views: &RefCell<ViewSet>,
        saved: &SavedViews,
        cx: &mut dyn PlanViewHost,
    ) -> usize {
        // The views open now that the record names are kept, as the views they are; every other
        // is closed.
        let open: Vec<ViewId> = self.docks.iter().map(|view| view.id).collect();
        for id in open.iter().copied() {
            if !saved.views.iter().any(|view| view.id == id) {
                let _ = self.close_view(views, id, cx);
            }
        }
        // What the chrome keeps of a view that stays - a tray button, a maximise, where it
        // floated from - is of the arrangement being replaced.
        if let Some(chrome) = &mut self.chrome {
            for id in &open {
                chrome.forget(&mut self.layout, &dock_name(*id));
            }
        }
        // Each recorded view open, under the id it has now.
        let mut ids: BTreeMap<ViewId, ViewId> = BTreeMap::new();
        for view in &saved.views {
            let id = if self.find(view.id).is_some() {
                let _ = self.set_view_kind(views, view.id, view.kind, cx);
                view.id
            } else {
                self.open_restored(views, view.id, view.kind, cx)
            };
            ids.insert(view.id, id);
        }
        let now = |id: ViewId| ids.get(&id).copied().unwrap_or(id);
        // The arrangement, made afresh: the docked tree with its shares and its pages in front,
        // and a window for each floating view.
        let mut layout: DockState<ViewTab> = DockState::new(Vec::new());
        if let Some(docked) = &saved.docked {
            *layout.main_surface_mut() = tree_of(docked, &now);
        }
        for (id, _) in &saved.floating {
            let _ = layout.add_window(vec![ViewTab(dock_name(now(*id)))]);
        }
        self.layout = layout;
        let room = self.room();
        if let Some(chrome) = &mut self.chrome {
            // The chrome sees which docks float, and gives each a rectangle to be put at.
            chrome.refresh(&mut self.layout);
            for (id, rect) in &saved.floating {
                let _ = chrome.set_float_rect(&dock_name(now(*id)), kept_in_reach(*rect, room));
            }
        }
        // Each view's own switches, the link and the active view.
        let linked: Vec<ViewId> = saved
            .views
            .iter()
            .filter(|view| view.linked && linkable(view.kind))
            .map(|view| now(view.id))
            .collect();
        {
            let mut set = views.borrow_mut();
            let _ = set.unlink_all();
            for view in &saved.views {
                if let Some(state) = set.find_mut(now(view.id)) {
                    state.selection_ghosts = view.ghosts;
                }
            }
            // A link of one waits for a second view, which a new session has not been asked for.
            if linked.len() > 1 {
                let _ = set.link(&linked, None);
            }
            let _ = set.activate(now(saved.active));
        }
        self.float_drag = None;
        self.bar_menu = None;
        self.kind_menu = None;
        self.link_changed(views, &linked);
        self.update_active_marks(views);
        self.relayout(views);
        self.push(WorkspaceEvent::ActiveChanged);
        self.push(WorkspaceEvent::ViewsChanged);
        saved.views.len()
    }
}

/// Draws the docked views in `area` and the floating ones over it.
fn draw_docks(
    inner: &mut Inner,
    views: &RefCell<ViewSet>,
    ui: &mut Ui,
    area: Rect,
    cx: &mut HostDrawing<'_, '_>,
    scenery: Scenery<'_>,
) {
    let tree = inner.layout.main_surface();
    let leaves = if tree.is_empty() || tree.num_tabs() == 0 {
        Vec::new()
    } else {
        leaf_rects(
            tree,
            Rect::from_min_size(area.min, area.size()),
            inner.pixels_per_point,
        )
    };
    // The leaves' tabs, read before drawing: each draw lends the layout's chrome mutably.
    let mut pages: Vec<(Rect, Vec<String>, usize)> = Vec::new();
    for (index, rect) in leaves {
        if let Node::Leaf(leaf) = &inner.layout.main_surface()[index] {
            pages.push((
                rect,
                leaf.tabs.iter().map(|tab| tab.0.clone()).collect(),
                leaf.active.0,
            ));
        }
    }
    for (rect, tabs, active) in pages {
        let mut dock = rect;
        if tabs.len() > 1 {
            let strip = Rect::from_min_size(rect.min, Vec2::new(rect.width(), TAB_STRIP));
            dock.min.y = strip.max.y;
            // The views' tabs drawn as every tab strip of the window is (drive::controls::tabs):
            // the one in front over the accent's bar, the others muted - not egui's selectable
            // labels, blue pills. Named by the group's first view, so two groups' strips differ.
            let titles: Vec<String> = tabs
                .iter()
                .map(|name| {
                    inner
                        .chrome
                        .as_ref()
                        .and_then(|chrome| chrome.title_bar(name))
                        .map_or_else(|| name.clone(), |bar| String::from(bar.title()))
                })
                .collect();
            let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
            let strip_name = format!("viewTabs{}", tabs.first().map_or("", String::as_str));
            let mut current = active;
            let chosen = ui
                .scope_builder(
                    UiBuilder::new()
                        .max_rect(strip)
                        .layout(Layout::left_to_right(Align::Center)),
                    |ui| {
                        ui.set_clip_rect(strip.intersect(ui.clip_rect()));
                        crate::drive::controls::tabs(ui, &strip_name, &titles, &mut current)
                            .changed()
                            .then_some(current)
                    },
                )
                .inner;
            if let Some(at) = chosen
                && let Some(id) = tabs.get(at).and_then(|name| dock_id(name))
            {
                inner.raise(id);
            }
        }
        if let Some(id) = tabs.get(active).and_then(|name| dock_id(name)) {
            draw_dock(inner, views, ui, dock, id, cx, scenery);
        }
    }
    // The floating views, each a window inside the workspace at the rectangle the chrome keeps
    // for it (points from the workspace's top left), in the order opened. A maximised one fills
    // the workspace as it is now; any other is brought where its bar can be reached, which a
    // workspace made smaller since (a panel shown, the window narrowed, a session restored on a
    // smaller screen) may have left it out of.
    let floating: Vec<ViewId> = inner
        .docks
        .iter()
        .map(|view| view.id)
        .filter(|&id| inner.floating(id))
        .collect();
    for id in floating {
        let Some(kept) = inner.float_rect(id) else {
            continue;
        };
        let name = dock_name(id);
        let maximised = inner
            .chrome
            .as_ref()
            .is_some_and(|chrome| chrome.is_maximised(&name));
        let within = if maximised {
            Rect::from_min_size(Pos2::ZERO, area.size())
        } else {
            kept_in_reach(kept, area.size())
        };
        if within != kept
            && let Some(chrome) = &mut inner.chrome
        {
            let _ = chrome.set_float_rect(&name, within);
        }
        let rect = within.translate(area.min.to_vec2());
        // Not constrained to the screen by egui: where the window may go is `kept_in_reach`'s to
        // say, and an area egui had moved would be drawn off the rectangle its dock is laid out in.
        egui::Area::new(floating_view_area(id))
            .fixed_pos(rect.min)
            .constrain(false)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(rect.size());
                ui.set_max_size(rect.size());
                draw_dock(inner, views, ui, rect, id, cx, scenery);
            });
    }
}

/// One view's dock: its bar (the chrome's, or a plain title) and its widget under it, as the
/// target View<id>.
fn draw_dock(
    inner: &mut Inner,
    views: &RefCell<ViewSet>,
    ui: &mut Ui,
    rect: Rect,
    id: ViewId,
    cx: &mut HostDrawing<'_, '_>,
    scenery: Scenery<'_>,
) {
    let name = dock_name(id);
    let Inner {
        chrome,
        docks,
        fixed,
        ..
    } = inner;
    let pinned = fixed.get(&id).copied();
    let Some(view) = docks.iter_mut().find(|view| view.id == id) else {
        return;
    };
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min))
            .id_salt(("katanaViewDock", id)),
        |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let mut content = |ui: &mut Ui| match pinned {
                Some((width, height)) => {
                    let at = Rect::from_min_size(
                        ui.cursor().min,
                        Vec2::new(width as f32, height as f32),
                    );
                    ui.scope_builder(UiBuilder::new().max_rect(at), |ui| {
                        draw_widget(ui, view, views, cx, scenery);
                    });
                }
                None => draw_widget(ui, view, views, cx, scenery),
            };
            match chrome {
                Some(chrome) => {
                    chrome.dock_ui(ui, &name, content);
                }
                None => {
                    // A plain title, as a dock without the window's chrome shows.
                    let title = views
                        .borrow()
                        .find(id)
                        .map(ViewSet::title)
                        .unwrap_or_default();
                    begin_target(ui, TargetSpec::new(&name, TargetKind::Dock, &title));
                    let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), BAR_HEIGHT));
                    ui.allocate_rect(bar, Sense::hover());
                    ui.painter().rect_filled(bar, 0.0, palette::window().to_egui());
                    ui.painter().text(
                        bar.left_center() + Vec2::new(6.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        title,
                        egui::TextStyle::Body.resolve(ui.style()),
                        palette::text().to_egui(),
                    );
                    content(ui);
                    end_target(ui, rect);
                }
            }
        },
    );
}

/// The view's widget in what its dock leaves under the bar.
fn draw_widget(
    ui: &mut Ui,
    view: &mut View,
    views: &RefCell<ViewSet>,
    cx: &mut HostDrawing<'_, '_>,
    scenery: Scenery<'_>,
) {
    let mut set = views.borrow_mut();
    let Some(state) = set.find_mut(view.id) else {
        return;
    };
    if let Some(plan) = &mut view.plan {
        plan.ui(ui, cx, state);
    } else if let Some(render) = &mut view.render {
        let scene = RenderScene {
            document: cx.host.document(),
            surfaces: scenery.surfaces,
            meshes: scenery.meshes,
        };
        render.ui(ui, &scene, state);
    } else if let Some(section) = &mut view.section {
        section.ui(ui, state, Some(cx.host.document()));
    }
}

/// The kind switcher's menu as a drive target: a step presses one of its kinds in it.
pub const KIND_MENU: &str = "ViewKindMenu";

/// The tip of each kind in the kind switcher's menu, as View > Active Viewport Shows words it for
/// the active view.
fn kind_tip(kind: ViewKind) -> &'static str {
    match kind {
        ViewKind::Plan => "Show the plan in this view",
        ViewKind::Model3D => "Show the model in 3D in this view",
        ViewKind::Section => "Show the section cut last in this view",
        ViewKind::Elevation => "Show an elevation of the model in this view",
    }
}

/// The kind switcher's menu of the view it was opened on: a click on a kind changes the view. It
/// is drawn as every menu of the window is - egui's menu frame, each kind an action item
/// ("viewKindPlan" ...) with its icon and the view's own kind ticked - where it was four framed
/// buttons with neither. A click outside it closes it, as a menu's does.
fn draw_kind_menu(inner: &mut Inner, views: &RefCell<ViewSet>, ui: &mut Ui, area: Rect) {
    let Some(id) = inner.kind_menu else {
        return;
    };
    // A floating view's switcher opens its menu too: asked of the docked places alone, the menu
    // of a floating view never opened.
    let Some(at) = inner.place_of(id) else {
        inner.kind_menu = None;
        return;
    };
    let shows = views.borrow().find(id).map(|state| state.kind);
    let origin = area.min + at.min.to_vec2() + Vec2::new(6.0, BAR_HEIGHT);
    let mut chosen = None;
    let shown = crate::shell::popup_menu(ui.ctx(), KIND_MENU, origin, |ui| {
        // The look egui gives a menu it opens itself: items framed only under the pointer.
        egui::containers::menu::menu_style(ui.style_mut());
        for kind in KINDS {
            let item = crate::drive::Action::new(&format!("viewKind{}", kind.name()), kind.name())
                .icon(kind_icon(kind))
                .tip(kind_tip(kind))
                .checkable(shows == Some(kind));
            if crate::drive::action_item(ui, &item).clicked() {
                chosen = Some(kind);
            }
        }
    });
    let clicked_outside = ui.input(|input| {
        input.pointer.any_click()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|at| !shown.rect.contains(at))
    });
    if let Some(kind) = chosen {
        inner.kind_menu = None;
        inner.kind_choice = Some((id, kind));
    } else if clicked_outside {
        // The press on the switcher itself toggles the menu after the frame: kept from opening
        // it again in the frame that closed it.
        inner.kind_menu = None;
        inner.kind_menu_dismissed = Some(id);
    }
}

/// The shortcut menu of the title bar it was opened on (`BAR_MENU`), drawn as every menu of the
/// window is: a click on an item is carried out after the frame (`trigger_bar_menu`), a click
/// outside it closes it, as a menu's does.
fn draw_bar_menu(inner: &mut Inner, views: &RefCell<ViewSet>, ui: &mut Ui) {
    let Some((id, at)) = inner.bar_menu else {
        return;
    };
    // Closed meanwhile, its view has no menu.
    let Some(menu) = inner.bar_menu_rows(views, id) else {
        inner.bar_menu = None;
        return;
    };
    let mut chosen = None;
    let shown = crate::shell::popup_menu(ui.ctx(), BAR_MENU, at, |ui| {
        crate::drive::action::menu_items_ui(ui, &menu, &mut chosen);
    });
    let clicked_outside = ui.input(|input| {
        input.pointer.any_click()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|at| !shown.rect.contains(at))
    });
    if let Some(name) = chosen {
        inner.bar_menu = None;
        inner.bar_menu_choice = Some((id, name));
    } else if clicked_outside {
        inner.bar_menu = None;
    }
}

// ---- the views between sessions ------------------------------------------------------------------

/// One view of a saved session: the id it had, what it showed, and its own switches that are the
/// window's and not a drawing's.
#[derive(Clone, Debug, PartialEq)]
struct SavedView {
    id: ViewId,
    kind: ViewKind,
    linked: bool,
    ghosts: bool,
}

/// The docked arrangement of a saved session: the dock tree as it was, a place holding its views
/// in their tab order.
#[derive(Clone, Debug, PartialEq)]
enum SavedNode {
    Leaf {
        views: Vec<ViewId>,
        /// The page shown.
        front: ViewId,
    },
    Split {
        /// True: `second` to the right of `first`; false: below it.
        side_by_side: bool,
        /// `first`'s share of the place, egui_dock's fraction.
        share: f32,
        first: Box<SavedNode>,
        second: Box<SavedNode>,
    },
}

/// A saved session's views, read and found whole: every id of the arrangement is a view of the
/// list, and every view has one place.
#[derive(Clone, Debug, PartialEq)]
struct SavedViews {
    /// In the order opened.
    views: Vec<SavedView>,
    active: ViewId,
    /// None when no view was docked.
    docked: Option<SavedNode>,
    /// The floating views, each at its rectangle in points from the workspace's top left.
    floating: Vec<(ViewId, Rect)>,
}

/// The word a saved view's kind is written with: VIEWS OPEN's.
fn kind_word(kind: ViewKind) -> String {
    kind.name().to_ascii_lowercase()
}

fn kind_from_word(word: &str) -> Option<ViewKind> {
    KINDS
        .into_iter()
        .find(|kind| word.eq_ignore_ascii_case(kind.name()))
}

/// Why a saved session's views are not restored: `what` is wrong with the record.
fn damaged(what: &str) -> Error {
    make_error(
        ErrorCode::ParseFailure,
        "the saved views are damaged",
        what,
    )
}

/// A whole number, written as either of JSON's integers.
fn whole(value: &Json) -> Option<u64> {
    match value {
        Json::NumberUnsigned(number) => Some(*number),
        Json::NumberInteger(number) => u64::try_from(*number).ok(),
        _ => None,
    }
}

/// A view id: a whole number from 1.
fn saved_id(value: &Json) -> Option<ViewId> {
    whole(value)
        .and_then(|number| ViewId::try_from(number).ok())
        .filter(|id| *id != NO_VIEW)
}

/// A finite number, in points.
fn points(value: &Json) -> Option<f32> {
    if !value.is_number() {
        return None;
    }
    value
        .get::<f64>()
        .ok()
        .filter(|number| number.is_finite())
        .map(|number| number as f32)
}

impl SavedNode {
    fn to_json(&self) -> Json {
        match self {
            Self::Leaf { views, front } => Json::object_of([
                (
                    "views",
                    Json::array_of(views.iter().map(|id| Json::from(*id))),
                ),
                ("front", Json::from(*front)),
            ]),
            Self::Split {
                side_by_side,
                share,
                first,
                second,
            } => Json::object_of([
                (
                    "split",
                    Json::from(if *side_by_side { "columns" } else { "rows" }),
                ),
                ("share", Json::from(f64::from(*share))),
                ("first", first.to_json()),
                ("second", second.to_json()),
            ]),
        }
    }

    /// The node `value` describes; `depth` is how far under the root it is. A tree of n places
    /// is at most n - 1 deep, so one deeper than the views a session may name is no tree this
    /// wrote, and is refused before it is followed further.
    fn from_json(value: &Json, depth: usize) -> Result<Self> {
        if depth > MOST_SAVED_VIEWS {
            return Err(damaged(
                "the arrangement is nested deeper than its views allow",
            ));
        }
        if let Some(split) = value.find("split") {
            let side_by_side = match split.as_str() {
                Some("columns") => true,
                Some("rows") => false,
                _ => return Err(damaged("a split is columns or rows")),
            };
            // egui_dock takes a fraction from 0 to 1 and panics on any other; a place of no size
            // at all (0 or 1) is no arrangement a person made.
            let share = value
                .find("share")
                .and_then(points)
                .filter(|share| *share > 0.0 && *share < 1.0)
                .ok_or_else(|| damaged("a split's share is a number between 0 and 1"))?;
            let part = |key: &str| -> Result<Box<Self>> {
                let node = value
                    .find(key)
                    .ok_or_else(|| damaged("a split has a first and a second part"))?;
                Ok(Box::new(Self::from_json(node, depth + 1)?))
            };
            return Ok(Self::Split {
                side_by_side,
                share,
                first: part("first")?,
                second: part("second")?,
            });
        }
        let views = value
            .find("views")
            .and_then(Json::as_array)
            .ok_or_else(|| damaged("a place lists its views"))?
            .iter()
            .map(|id| saved_id(id).ok_or_else(|| damaged("a view id is a whole number from 1")))
            .collect::<Result<Vec<ViewId>>>()?;
        let Some(&first) = views.first() else {
            return Err(damaged("a place holds at least one view"));
        };
        // A front page the place does not hold is the first one's: no reason to refuse the rest.
        let front = value
            .find("front")
            .and_then(saved_id)
            .filter(|front| views.contains(front))
            .unwrap_or(first);
        Ok(Self::Leaf { views, front })
    }

    /// Every view the node holds, in place order.
    fn collect_ids(&self, ids: &mut Vec<ViewId>) {
        match self {
            Self::Leaf { views, .. } => ids.extend(views.iter().copied()),
            Self::Split { first, second, .. } => {
                first.collect_ids(ids);
                second.collect_ids(ids);
            }
        }
    }

    /// The views of its first place: the top left one.
    fn first_leaf(&self) -> &[ViewId] {
        match self {
            Self::Leaf { views, .. } => views,
            Self::Split { first, .. } => first.first_leaf(),
        }
    }
}

impl SavedViews {
    /// Reads what `ViewWorkspace::saved_views` wrote, and holds it to what that writes: one
    /// version, at most MOST_SAVED_VIEWS views of known kinds under ids used once, and an
    /// arrangement that places each of them once and no other. Anything else is refused whole
    /// with what is wrong, so that a damaged record changes nothing.
    fn parse(text: &str) -> Result<Self> {
        let json = Json::parse(text).map_err(|error| {
            make_error(
                ErrorCode::ParseFailure,
                "the saved views are not JSON",
                error.what,
            )
        })?;
        let version = json
            .find("version")
            .and_then(whole)
            .ok_or_else(|| damaged("there is no version"))?;
        if i64::try_from(version).ok() != Some(SAVED_VIEWS_VERSION) {
            return Err(make_error(
                ErrorCode::Unsupported,
                "the saved views are of a version this build does not read",
                format!("{version}"),
            ));
        }
        let records = json
            .find("views")
            .and_then(Json::as_array)
            .ok_or_else(|| damaged("there is no list of views"))?;
        if records.is_empty() {
            return Err(damaged("the list of views is empty"));
        }
        if records.len() > MOST_SAVED_VIEWS {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the saved views name {} views, and a session restores {MOST_SAVED_VIEWS} at \
                     most",
                    records.len()
                ),
                "",
            ));
        }
        let mut views: Vec<SavedView> = Vec::with_capacity(records.len());
        for record in records {
            let id = record
                .find("id")
                .and_then(saved_id)
                .ok_or_else(|| damaged("a view id is a whole number from 1"))?;
            if views.iter().any(|view| view.id == id) {
                return Err(damaged("a view id is named twice"));
            }
            let kind = record
                .find("kind")
                .and_then(Json::as_str)
                .and_then(kind_from_word)
                .ok_or_else(|| damaged("a view shows plan, 3d, section or elevation"))?;
            views.push(SavedView {
                id,
                kind,
                linked: record
                    .find("linked")
                    .and_then(Json::as_bool)
                    .unwrap_or(false),
                ghosts: record
                    .find("ghosts")
                    .and_then(Json::as_bool)
                    .unwrap_or(true),
            });
        }
        let docked = match json.find("docked") {
            Some(node) if !node.is_null() => Some(SavedNode::from_json(node, 0)?),
            _ => None,
        };
        let mut floating: Vec<(ViewId, Rect)> = Vec::new();
        if let Some(windows) = json.find("floating").and_then(Json::as_array) {
            for window in windows {
                let id = window
                    .find("view")
                    .and_then(saved_id)
                    .ok_or_else(|| damaged("a floating view is named by its id"))?;
                let number = |key: &str| {
                    window.find(key).and_then(points).ok_or_else(|| {
                        damaged("a floating view has x, y, width and height in points")
                    })
                };
                let (x, y, width, height) = (
                    number("x")?,
                    number("y")?,
                    number("width")?,
                    number("height")?,
                );
                if width < LEAST_FLOAT.x || height < LEAST_FLOAT.y {
                    return Err(damaged("a floating view is smaller than a view can be"));
                }
                floating.push((
                    id,
                    Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, height)),
                ));
            }
        }
        // Every view in one place, and no place for a view the list does not have.
        let mut placed: Vec<ViewId> = Vec::new();
        if let Some(docked) = &docked {
            docked.collect_ids(&mut placed);
        }
        placed.extend(floating.iter().map(|(id, _)| *id));
        for (at, id) in placed.iter().enumerate() {
            if !views.iter().any(|view| view.id == *id) {
                return Err(damaged(
                    "the arrangement places a view the list does not name",
                ));
            }
            if placed[..at].contains(id) {
                return Err(damaged("the arrangement places a view twice"));
            }
        }
        if placed.len() != views.len() {
            return Err(damaged(
                "a view of the list has no place in the arrangement",
            ));
        }
        // An active view the list does not have is the first one's: the rest is still good.
        let active = json
            .find("active")
            .and_then(saved_id)
            .filter(|active| views.iter().any(|view| view.id == *active))
            .unwrap_or(views[0].id);
        Ok(Self {
            views,
            active,
            docked,
            floating,
        })
    }
}

/// The docked arrangement under node `index` of `tree`, as a saved session records it; None for
/// a node that holds no view. A split with a view on one side only is that side.
fn saved_node(tree: &Tree<ViewTab>, index: NodeIndex) -> Option<SavedNode> {
    if index.0 >= tree.len() {
        return None;
    }
    match &tree[index] {
        Node::Leaf(leaf) => {
            let views: Vec<ViewId> = leaf
                .tabs
                .iter()
                .filter_map(|tab| dock_id(&tab.0))
                .collect();
            let first = views.first().copied()?;
            let front = leaf
                .tabs
                .get(leaf.active.0)
                .and_then(|tab| dock_id(&tab.0))
                .unwrap_or(first);
            Some(SavedNode::Leaf { views, front })
        }
        Node::Horizontal(split) | Node::Vertical(split) => {
            let side_by_side = matches!(&tree[index], Node::Horizontal(_));
            match (
                saved_node(tree, index.left()),
                saved_node(tree, index.right()),
            ) {
                (Some(first), Some(second)) => Some(SavedNode::Split {
                    side_by_side,
                    // Inside what `restore_views` reads: egui_dock's own fractions are, and a
                    // place dragged to nothing would otherwise make the record a refused one.
                    share: split.fraction.clamp(0.01, 0.99),
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            }
        }
        Node::Empty => None,
    }
}

/// The dock tree `node` records, each view's dock named by the id it has now (`now`).
fn tree_of(node: &SavedNode, now: &dyn Fn(ViewId) -> ViewId) -> Tree<ViewTab> {
    let tabs = |views: &[ViewId]| -> Vec<ViewTab> {
        views
            .iter()
            .map(|id| ViewTab(dock_name(now(*id))))
            .collect()
    };
    let mut tree = Tree::new(tabs(node.first_leaf()));
    grow(&mut tree, NodeIndex::root(), node, &tabs);
    tree
}

/// Makes the place at `at` of `tree`, which holds the views of `node`'s first place, into the
/// whole of `node`: a split cuts it, `first` keeping the place and `second` given the rest, and
/// each is grown in turn; a place shows the page it showed.
fn grow(
    tree: &mut Tree<ViewTab>,
    at: NodeIndex,
    node: &SavedNode,
    tabs: &dyn Fn(&[ViewId]) -> Vec<ViewTab>,
) {
    match node {
        SavedNode::Leaf { views, front } => {
            if let Some(page) = views.iter().position(|id| id == front)
                && let Ok(leaf) = tree.leaf_mut(at)
            {
                let _ = leaf.set_active_tab(page);
            }
        }
        SavedNode::Split {
            side_by_side,
            share,
            first,
            second,
        } => {
            let added = tabs(second.first_leaf());
            let [kept, new] = if *side_by_side {
                tree.split_right(at, *share, added)
            } else {
                tree.split_below(at, *share, added)
            };
            grow(tree, kept, first, tabs);
            grow(tree, new, second, tabs);
        }
    }
}

// ---- the views as a dialog sees them ------------------------------------------------------------

impl ViewAccess for ViewWorkspace {
    fn view_set(&self) -> Rc<RefCell<ViewSet>> {
        Rc::clone(&self.shared.views)
    }

    fn zoom_to(&mut self, area: &Box2) {
        ViewWorkspace::zoom_to(self, area);
    }

    /// Done at the next frame, lent the drawing: a dialog reaches the views through its host, which
    /// it cannot lend them back.
    fn zoom_extents_all(&mut self) {
        self.inner().zoom_extents_all_asked = true;
    }

    /// Its line queued for the window's executor, which runs it at the end of the next frame.
    fn zoom_to_selection(&mut self, view: ViewId) {
        let inner = self.inner();
        inner
            .lines
            .borrow_mut()
            .push(format!("ZOOM SELECTION view={view}"));
    }

    fn active_plan_view(&self) -> Option<ViewId> {
        ViewWorkspace::active_plan_view(self)
    }

    fn plan_views(&self) -> Vec<ViewId> {
        ViewWorkspace::plan_views(self)
    }

    fn refresh_all(&mut self) {
        self.scenery_changed();
    }

    fn pick_point(&mut self) -> PickId {
        let mut inner = self.inner();
        if let Some(waiting) = inner.pick_waiting.take() {
            inner.picks.insert(waiting, Some(None));
        }
        let pick = inner.next_pick;
        inner.next_pick += 1;
        let plans = inner.plan_ids();
        if plans.is_empty() {
            inner.picks.insert(pick, Some(None));
            return pick;
        }
        inner.picks.insert(pick, None);
        inner.pick_waiting = Some(pick);
        for view in &mut inner.docks {
            if let Some(plan) = &mut view.plan {
                plan.await_point_pick(true);
            }
        }
        pick
    }

    fn picked_point(&mut self, pick: PickId) -> Option<Option<Point2>> {
        let mut inner = self.inner();
        match inner.picks.get(&pick).copied() {
            Some(Some(answer)) => {
                inner.picks.remove(&pick);
                Some(answer)
            }
            _ => None,
        }
    }
}
