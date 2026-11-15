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

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Align, Layout, Pos2, Rect, Sense, Ui, UiBuilder, Vec2};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex, TabPath, Tree};
use katana_cad::interactive_tool::{ToolInput, tool_catalog};
use katana_cad::scene::{SceneMesh, SceneOptions, SceneSurface};
use katana_cad::scope_verbs::{ScopeViewFn, scope_view_of};
use katana_cad::section::{Section, SectionDesign};
use katana_cad::view_link::linkable;
use katana_cad::view_set::{NO_VIEW, ViewId, ViewSet, ViewState, restore_view_paper_scales};
use katana_cad::view_verbs::{
    ViewVerbHost, ZoomRequest, ZoomRequestKind, apply_plan_zoom, zoom_takes,
};
use katana_cad::viewport_layout::{
    LayoutKind, ViewKind, cell_count, default_view_kind, dock_splits,
};
use katana_cad::{CommandInterpreter, Document, ListenerHandle, SnapModes, SnapResult};
use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::EntityId;
use katana_entity::EnumName;
use katana_geometry::{Box2, Point2, Polyline2};
use katana_interop::ReferenceData;

use crate::canvas::PointF;
use crate::dock_chrome::{
    ALWAYS, BAR_HEIGHT, ChromeIcons, ChromeRequest, DockChrome, DockRole, DockTab, DockTitleBar,
    TitleBarButton,
};
use crate::drive::action::Action;
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
    KeyModifiers, PlanView, PlanViewEvent, PlanViewHooks, PlanViewHost, Tool, ViewKey,
};

/// The gap between two docked views, in points: the C++ window's, measured from its headless
/// screenshots (docs/rust.md, "The headless window has the C++'s geometry": two views split the
/// 660 points of the views' area 327 | 328).
pub const SEPARATOR: f32 = 5.0;
/// The tab strip over a group of tabbed views (QTabBar's height in the window's style), at the
/// top: tabs above the views they switch between, where the eye looks for a title.
pub const TAB_STRIP: f32 = 24.0;
/// The kind switcher's width: 22 and the 12 of the arrow's box beside the icon.
const KIND_BUTTON_WIDTH: f32 = 34.0;
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

/// A view's dock object name: what the saved layout finds it by, and its drive target.
pub fn dock_name(id: ViewId) -> String {
    format!("View{id}")
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
    if extent <= SEPARATOR + 2.0 {
        return 0.5;
    }
    let first = ((extent - SEPARATOR) / 2.0).floor().max(1.0);
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
/// included), less the tab strip of a group, and whether it is the page shown.
#[derive(Clone, Copy, Debug)]
struct Placed {
    rect: Rect,
    shown: bool,
    tabbed: bool,
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
                            tabbed,
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
        let placed = self.placements();
        let mut set = views.borrow_mut();
        for view in &mut self.docks {
            let (width, height) = match (self.fixed.get(&view.id), placed.get(&view.id)) {
                (Some(&fixed), _) => fixed,
                (None, Some(at)) => (
                    at.rect.width().round() as i32,
                    (at.rect.height() - BAR_HEIGHT).round().max(1.0) as i32,
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
        mut cx: Option<&mut dyn PlanViewHost>,
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
        cx: Option<&mut dyn PlanViewHost>,
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

    fn drain_all(&mut self, views: &RefCell<ViewSet>, mut cx: Option<&mut dyn PlanViewHost>) {
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
        cx: Option<&mut dyn PlanViewHost>,
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

    /// Maximise or Restore, as the view's own button does; pressing it activates the view first.
    pub fn toggle_maximised(&self, id: ViewId) {
        let mut inner = self.inner();
        inner.activate(self.views_cell(), id);
        let name = dock_name(id);
        let screen = inner.area.unwrap_or(Rect::NOTHING);
        let Inner { chrome, layout, .. } = &mut *inner;
        if let Some(chrome) = chrome {
            chrome.toggle_maximised(layout, &name, screen);
        }
        inner.relayout(self.views_cell());
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
        inner.layout.retain_tabs(|tab| !leaving.contains(&tab.0));
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
            inner.placements().get(&id).map_or(origin, |placed| {
                origin + Vec2::new(placed.rect.max.x, placed.rect.min.y + BAR_HEIGHT)
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
            // The bars are brought up to date where what they show changes (a link, a kind, a
            // view's own settings, the active view), never at each frame: a tool a caller took
            // off a bar stays off, as the C++'s setToolWanted holds.
            draw_docks(&mut inner, views, ui, area, &mut cx, scenery);
            draw_kind_menu(&mut inner, ui, area);
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
        let chosen = self.inner().kind_choice.take();
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
                    self.inner().activate(views, id);
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
                    // Pressing a view's Float is working in that view.
                    let mut inner = self.inner();
                    inner.activate(views, id);
                    let Inner { chrome, layout, .. } = &mut *inner;
                    if let Some(chrome) = chrome {
                        chrome.toggle_floating(layout, &name);
                    }
                    inner.relayout(views);
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
                        inner.kind_menu = if inner.kind_menu == Some(id) {
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
            let mut chosen = None;
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(strip)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| {
                    for (at, name) in tabs.iter().enumerate() {
                        let title = inner
                            .chrome
                            .as_ref()
                            .and_then(|chrome| chrome.title_bar(name))
                            .map_or_else(|| name.clone(), |bar| String::from(bar.title()));
                        if ui.selectable_label(at == active, title).clicked() {
                            chosen = Some(at);
                        }
                    }
                },
            );
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
    // The floating views, each a window inside the main window.
    let floating: Vec<(ViewId, Rect)> = inner
        .layout
        .iter_surfaces_indexed()
        .filter(|(index, _)| !index.is_main())
        .filter_map(|(index, surface)| {
            let tree = surface.node_tree()?;
            let name = tree.iter().filter_map(Node::tabs).flatten().next()?.0.clone();
            Some((index, name))
        })
        .filter_map(|(index, name)| {
            let id = dock_id(&name)?;
            Some((index, id))
        })
        .collect::<Vec<(SurfaceIndex, ViewId)>>()
        .into_iter()
        .filter_map(|(index, id)| {
            inner
                .layout
                .get_window_state(index)
                .map(|window| (id, window.rect()))
        })
        .collect();
    for (id, rect) in floating {
        let rect = if rect.is_finite() && rect.is_positive() {
            rect
        } else {
            Rect::from_min_size(area.min + Vec2::splat(40.0), Vec2::new(400.0, 300.0))
        };
        egui::Area::new(egui::Id::new(("katanaFloatingView", id)))
            .fixed_pos(rect.min)
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

/// The kind switcher's menu of the view it was opened on: a click on a kind changes the view.
fn draw_kind_menu(inner: &mut Inner, ui: &mut Ui, area: Rect) {
    let Some(id) = inner.kind_menu else {
        return;
    };
    let Some(at) = inner.placements().get(&id).map(|placed| placed.rect) else {
        inner.kind_menu = None;
        return;
    };
    let origin = area.min + at.min.to_vec2() + Vec2::new(6.0, BAR_HEIGHT);
    let mut chosen = None;
    egui::Area::new(egui::Id::new(("katanaViewKindMenu", id)))
        .order(egui::Order::Foreground)
        .fixed_pos(origin)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                for kind in KINDS {
                    if ui.button(kind.name()).clicked() {
                        chosen = Some(kind);
                    }
                }
            });
        });
    if let Some(kind) = chosen {
        inner.kind_menu = None;
        inner.kind_choice = Some((id, kind));
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
