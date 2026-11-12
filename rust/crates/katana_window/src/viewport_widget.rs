//! The 2D model-space view, the plan view: the port of viewport_widget.
//!
//! A view of the Document, never a second source of truth (Rule 3): it paints whatever the model
//! contains and turns mouse input into Commands. All geometry decisions (picking, snapping, view
//! maths) live in katana_cad, where they are unit tested; this only adapts them to the window's
//! events and to the one plan painter (plan_painter).
//!
//! WHAT A FRAME COSTS. The drawing is painted into an image kept between paints, and a paint whose
//! view, drawing, layers, library, selection, reference layers and meshes are all as they were
//! only lays that image down and draws the view's own furniture over it (tool previews, the snap
//! marker, the selection box, the prompt). A mouse move then costs the furniture, not the
//! drawing: measured on a 27.6k-entity survey drawing it was a whole repaint every move.
//!
//! THE RUST SHAPE. The C++ widget keeps `Document&` and `ViewState&` for its life, is sent Qt
//! events, and tells the window what happened through `std::function` hooks. Here:
//!
//! * **Every call is lent what it works on**: the drawing through a `PlanViewHost` (the document,
//!   and the window's reference data and meshes, which belong to the window) and the view's
//!   `ViewState`, which belongs to the workspace's ViewSet and outlives the view - it is what
//!   survives when the view changes kind or its dock floats, so the layers hidden in this view,
//!   and its pan and zoom, live there. The view owns no window-level state.
//! * **The events are methods** - `mouse_press`, `mouse_move`, `mouse_release`,
//!   `mouse_double_click`, `wheel`, `key_press`, `key_release`, `shortcut_override`, `leave`,
//!   `context_menu_key` - in the view's own pixels, the C++'s widget pixels. `ui` is the adaptor
//!   from egui: it takes the rectangle it is given, turns the frame's input into these calls,
//!   paints, and shows the picture. A test calls them as a Qt test sends an event.
//! * **What the C++ raised through hooks is queued** (`PlanViewEvent`, `take_events`): a closure
//!   cannot keep the window, and the window reads the queue after the call. The four hooks whose
//!   being SET changes what the view does itself are flags (`PlanViewHooks`): with
//!   `context_menu` unset a right-click cancels, with `repeat_tool` unset Enter starts the view's
//!   own last tool, with `zoom_extents` unset a middle double-click frames what the view draws,
//!   and with `text_typed` unset typed text is not taken.
//! * **The picture is a canvas image**: the kept drawing, and over it the furniture, painted by
//!   the raster canvas the plan painter and the feedback painter draw on, and handed to egui as a
//!   texture. So what a test reads back is what the screen shows.
//!
//! The pan and zoom are the state's `plan` and whether the view has been framed is its
//! `plan_framed`: both outlive the view. The view keeps only the box of the last frame, refitted
//! on a resize until the user pans or zooms; a view rebuilt over the same state starts without
//! it, which errs on the side of keeping the zoom.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use katana_cad::annotation::leader_presets::leader_context;
use katana_cad::dimension_draw::resolve_dimension_style;
use katana_cad::drawing::drafting::{TrackingPoints, constrain, track_acquired};
use katana_cad::drawing::grips::{Grip, grip_at};
use katana_cad::drawing::vertex_editing::read_polyline;
use katana_cad::interactive_tool::{PICK_APERTURE_PIXELS, SNAP_APERTURE_PIXELS, ToolInput};
use katana_cad::plot::fit_scale;
use katana_cad::scene::SceneMesh;
use katana_cad::selection::{pick_entity, pick_in_box};
use katana_cad::snapping::{SnapMode, SnapModes, snap};
use katana_cad::terrain::surface_plan::SurfaceProblem;
use katana_cad::tool_feedback::FeedbackRole;
use katana_cad::view_set::ViewState;
use katana_cad::view_transform::{DEFAULT_GRID_PIXELS, ViewTransform, grid_spacing};
use katana_cad::{
    BoxSelectionMode, Document, DocumentParts, LayerOverrides, ListenerHandle, PlotSettings,
    SelectionFilter, SnapRequest, SnapResult,
};
use katana_commands::{CommandPtr, delete_entities};
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, reply_quoted};
use katana_core::{ErrorCode, Result, Status};
use katana_entity::entity::to_string;
use katana_entity::{DimensionStyle, Entity, EntityId, Geometry, LeaderContext};
use katana_geometry::{Box2, Point2};
use katana_interop::ReferenceData;
use katana_math::numerics::tolerance;

use crate::canvas::{
    Brush, Canvas, Color, Elide, FillRule, Font, FontSize, FontWeight, HAlign, Image, IntRect, Pen,
    PenCap, PenJoin, PenStyle, PointF, RasterCanvas, RectF, RenderHint, TextFlags, VAlign,
};
use crate::customisation::style_painter::{CanvasRasterBackend, RasterBackend};
use crate::drawing::feedback_painter::{
    FeedbackFrame, band_height, lighter, overlay, paint_band, paint_feedback,
};
use crate::drawing::grip_controller::GripController;
use crate::drive::controls;
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::format::qt_fixed;
use crate::host::Host;
use crate::plan_painter::{
    PdfPageWriter, PlanFrame, PlanMedium, PlanPaintCache, PlanPaintOptions, PlanPaintStats,
    PlanSource, paint_plan, paint_plan_geometry, plan_drawn_bounds, plan_source_of,
    plot_plan_to_pdf,
};
use crate::theme::palette;
use crate::theme::style::{overlay_font, ui_font};
use crate::tools::tool_host::{Outcome, ToolHost, text_hook};
use crate::view_focus::{FocusPrevious, FocusReason, focus_chooses_view};

// The drawing's ground is the theme's viewport colour; a tool's preview, the grips, the snap
// marker and the prompt band draw in the overlay's colours (feedback_painter).

// The view's apertures have one home, beside the tools that measure in them (interactive_tool).
const WHEEL_ZOOM_STEP: f64 = 1.2;
/// Drags shorter than this are clicks, not selection boxes.
const DRAG_THRESHOLD_PIXELS: f64 = 4.0;
/// The margin Zoom Extents leaves round the drawing, as a fraction of the view on each side:
/// enough that a line along the edge of the drawing is not lost under the edge of the view.
const FRAME_MARGIN: f64 = 0.08;
/// The zoom of a view that has never been framed, in pixels per model unit: ten pixels a metre
/// shows an empty drawing as a few tens of metres of grid, a sensible first look for a site drawn
/// in metres.
const INITIAL_SCALE: f64 = 10.0;

/// The smallest a plan view may be made. It used to be 480 x 320, which stopped a dock being
/// shrunk below that and made its neighbours overlap it (the UX audit of 2026-09-23). Measured in
/// the headless screenshot's 1360 x 860 window with the Layers and Properties panels open: the
/// whole central area is 642 x 594 pixels, so 480 x 320 would not let two views sit side by side
/// at all, and each view of a 2 x 2 arrangement gets about 321 x 273 once its title bar is taken
/// off - less on a 1366 x 768 laptop. 160 x 120 fits that quarter with room to spare and still
/// leaves a usable drawing: at the grid's minimum spacing (cad's grid_spacing, 12 px) it is 13 by
/// 10 grid cells, twenty pick apertures across, and room for a snap marker and its label.
pub const MINIMUM_WIDTH: i32 = 160;
pub const MINIMUM_HEIGHT: i32 = 120;

/// A widget's size before anything has sized it: Qt's, for a widget never shown.
const UNSIZED_WIDTH: i32 = 640;
const UNSIZED_HEIGHT: i32 = 480;

/// The screen's logical resolution, which paper-sized marks on screen are sized by (Qt's
/// logicalDpiX on the platforms the window runs on).
const LOGICAL_DPI: f64 = 96.0;

/// Two presses of one button are a double click when the second comes within this many seconds
/// and pixels of the first (Qt's mouseDoubleClickInterval and mouseDoubleClickDistance).
const DOUBLE_CLICK_SECONDS: f64 = 0.4;
const DOUBLE_CLICK_PIXELS: f64 = 5.0;

/// The points a wheel's line is, for a wheel that reports points (egui's own line height).
const WHEEL_POINTS_A_NOTCH: f64 = 50.0;

const SELECT_PROMPT: &str =
    "Select: click an entity, drag right for a window, drag left for crossing";

// ---- the eight legacy tools ----------------------------------------------------------------

/// The window's first eight tools, from before the tool catalogue. Each one but Select is now a
/// catalogue tool (`tool_id`), run by the view's ToolHost like every other; the enum stays only
/// while the main window's toolbar and command words still name tools by it, and goes when they
/// name catalogue ids.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tool {
    #[default]
    Select,
    Point,
    Line,
    Polyline,
    Rectangle,
    Circle,
    Arc,
    Move,
    Copy,
}

impl Tool {
    /// Every legacy tool, Select first.
    pub const ALL: [Tool; 9] = [
        Tool::Select,
        Tool::Point,
        Tool::Line,
        Tool::Polyline,
        Tool::Rectangle,
        Tool::Circle,
        Tool::Arc,
        Tool::Move,
        Tool::Copy,
    ];

    /// The C++ `toString(Tool)`.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Point => "Point",
            Tool::Line => "Line",
            Tool::Polyline => "Polyline",
            Tool::Rectangle => "Rectangle",
            Tool::Circle => "Circle",
            Tool::Arc => "Arc",
            Tool::Move => "Move",
            Tool::Copy => "Copy",
        }
    }
}

/// The catalogue id a legacy tool runs as ("draw.line"); "" for Select. The catalogue tools that
/// do what the eight did: Arc is the three-point arc, as the old tool drew, and Circle is centre
/// and radius.
pub fn tool_id(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "",
        Tool::Point => "draw.point",
        Tool::Line => "draw.line",
        Tool::Polyline => "draw.polyline",
        Tool::Rectangle => "draw.rectangle",
        Tool::Circle => "draw.circle",
        Tool::Arc => "draw.arc",
        Tool::Move => "modify.move",
        Tool::Copy => "modify.copy",
    }
}

/// The legacy tool a catalogue id is, if it is one of the eight; Select for "".
pub fn legacy_tool(id: &str) -> Option<Tool> {
    Tool::ALL.into_iter().find(|&tool| id == tool_id(tool))
}

// ---- what a call is lent ---------------------------------------------------------------------

/// The drawing a plan view is lent for a call: the document, and what the window keeps beside it
/// and the view only paints - never mutates (Rule 3) - the reference data (imported imagery and
/// point clouds) and the session's meshes, drawn in plan as their footprints.
pub trait PlanViewHost {
    fn document(&self) -> &Document;
    fn document_mut(&mut self) -> &mut Document;
    /// The window's reference data; None where there is none.
    fn reference(&self) -> Option<&ReferenceData> {
        None
    }
    /// The session's meshes; empty where there are none.
    fn meshes(&self) -> &[SceneMesh<'_>] {
        &[]
    }
}

/// A document alone is a drawing with no reference data and no meshes.
impl PlanViewHost for Document {
    fn document(&self) -> &Document {
        self
    }

    fn document_mut(&mut self) -> &mut Document {
        self
    }
}

/// The parts of a drawing lent apart: by a window that keeps them as fields, and by a test.
pub struct ViewParts<'a> {
    pub document: &'a mut Document,
    pub reference: Option<&'a ReferenceData>,
    pub meshes: &'a [SceneMesh<'a>],
}

impl PlanViewHost for ViewParts<'_> {
    fn document(&self) -> &Document {
        self.document
    }

    fn document_mut(&mut self) -> &mut Document {
        self.document
    }

    fn reference(&self) -> Option<&ReferenceData> {
        self.reference
    }

    fn meshes(&self) -> &[SceneMesh<'_>] {
        self.meshes
    }
}

/// The window's host as a view's drawing (host::command_runner), with the meshes the window keeps.
pub struct HostView<'a> {
    pub host: &'a mut dyn Host,
    pub meshes: &'a [SceneMesh<'a>],
}

impl PlanViewHost for HostView<'_> {
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

// ---- the events, in and out ------------------------------------------------------------------

/// Qt::KeyboardModifiers, as far as the view asks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl KeyModifiers {
    pub const NONE: Self = Self {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };
    pub const SHIFT: Self = Self {
        shift: true,
        ..Self::NONE
    };
    pub const CTRL: Self = Self {
        ctrl: true,
        ..Self::NONE
    };

    /// egui's modifiers as Qt names them: its "command" is Ctrl off a Mac.
    pub fn from_egui(modifiers: egui::Modifiers) -> Self {
        Self {
            shift: modifiers.shift,
            ctrl: modifiers.ctrl || modifiers.command,
            alt: modifiers.alt,
            meta: modifiers.mac_cmd && !modifiers.command,
        }
    }

    /// QKeyEvent::matches(QKeySequence::Undo) for the Z key: Ctrl and nothing else.
    fn is_undo(self) -> bool {
        self == Self::CTRL
    }
}

/// Qt::MouseButton, as far as the view asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    /// Any other button: the view does nothing with it.
    Other,
}

/// The keys the view tells apart (Qt::Key); every other key is `Other`, and is text or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewKey {
    Escape,
    /// Return and Enter.
    Enter,
    Backspace,
    Space,
    Delete,
    Control,
    Z,
    Other,
}

/// What the C++ view raised through its hooks, in the order it raised them.
#[derive(Clone, Debug, PartialEq)]
pub enum PlanViewEvent {
    /// The prompt line changed ("Insert Vertex: Click on polyline 2 where the new vertex goes").
    Prompt(String),
    /// A refusal: a tool's, a command's, a grip's, a surface style's.
    Error(String),
    /// The cursor moved, after snapping, with what it snapped to.
    CursorMoved {
        world: Point2,
        snap: Option<SnapResult>,
    },
    /// Legacy: the legacy tool when one of the eight starts, and Select when a tool ends.
    ToolChanged(Tool),
    /// The catalogue id when a tool starts in this view, and "" when it ends.
    ActiveToolChanged(String),
    /// What a running tool reports that is not an error: "3 lines", "2 selected", a measurement.
    ToolMessage(String),
    /// Enter or Space with no tool running, when the owner repeats tools
    /// (`PlanViewHooks::repeat_tool`).
    RepeatTool,
    /// The user clicked into this view, or moved the keyboard focus into it, so the workspace can
    /// make it the active one. It can be raised twice for one click (the press, then the focus it
    /// gives); the workspace ignores re-activating the active view.
    Activated,
    /// Printable text typed into this view that the view has no use for itself: "type anywhere",
    /// as in AutoCAD, where typing LINE over the drawing starts the command.
    TextTyped(String),
    /// The shortcut menu asked for, at this pixel of the view.
    ContextMenu { at: PointF },
    /// A double click on an entity with no tool running, after its first click has selected it.
    EntityDoubleClicked(EntityId),
    /// This view's pan or zoom has just changed: a wheel notch, a middle-drag pan and a frame
    /// (Zoom Extents, zoom_to) with `by_user` true, the first paint's frame with false. Never
    /// raised by `hold_view` or by a change made to the state from outside, so a view following
    /// another never reports the move it was given.
    ViewMoved { by_user: bool },
    /// The Zoom Extents a middle double-click asks for, when the owner does it
    /// (`PlanViewHooks::zoom_extents`): it knows the link.
    ZoomExtents,
    /// After every paint, the view's statistics: "Plan  N drawn  X ms", and "(kept, Y ms)" when
    /// the frame reused the kept drawing.
    FrameStats(String),
    /// The answer to a point pick the view was told to wait for (`await_point_pick`): the left
    /// click's point in ground units, or None for a right click or Esc.
    PointPicked(Option<Point2>),
}

/// Which of the C++'s hooks the owner has set, where being set changes what the view does itself
/// (see the module comment).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PlanViewHooks {
    /// The owner repeats the last tool on Enter or Space at no prompt (the workspace knows the
    /// last tool run in ANY view). Unset, the view starts the last tool it ran itself.
    pub repeat_tool: bool,
    /// The owner shows the shortcut menu. Unset, a right-click with no tool running cancels.
    pub context_menu: bool,
    /// The owner does a middle double-click's Zoom Extents. Unset, the view frames what it draws.
    pub zoom_extents: bool,
    /// The owner takes text typed with no tool running (the command line).
    pub text_typed: bool,
}

/// What the last paint drew of the running tool's preview, by role (drawing/feedback_painter),
/// with its caption: what the headless pointer record prints and the tests assert.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreviewCounts {
    pub shapes: usize,
    pub markers: usize,
    pub target: usize,
    pub added: usize,
    pub removed: usize,
    pub enter: usize,
    pub focus: usize,
    pub refused: bool,
    pub caption: String,
}

/// The pointer's shape over the view (QCursor).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewCursor {
    #[default]
    Cross,
    ClosedHand,
}

// ---- what the kept drawing depends on --------------------------------------------------------

/// Whether plan views draw their lines as a cosmetic pixel: the View menu's "Thin screen lines
/// (faster)", on unless the user turns it off. Process-wide, as the C++'s is: the window has one
/// such switch.
static THIN_SCREEN_LINES: AtomicBool = AtomicBool::new(true);

/// Every plan view's lines on screen as a cosmetic one-pixel pen instead of the 1.5 px hairline:
/// 5-8x cheaper to stroke (docs/plan_view.md). Views read it at their next paint; plots keep
/// their paper-millimetre weights either way.
pub fn set_thin_screen_lines(thin: bool) {
    THIN_SCREEN_LINES.store(thin, Ordering::Relaxed);
}

pub fn thin_screen_lines() -> bool {
    THIN_SCREEN_LINES.load(Ordering::Relaxed)
}

/// FNV-1a, for the fingerprints of what the drawing depends on without the document saying so.
struct Fingerprint {
    value: u64,
}

impl Fingerprint {
    fn new() -> Self {
        Self {
            value: 1_469_598_103_934_665_603,
        }
    }

    fn add(&mut self, word: u64) {
        for byte in 0..8 {
            self.value ^= (word >> (8 * byte)) & 0xff;
            self.value = self.value.wrapping_mul(1_099_511_628_211);
        }
    }

    fn add_real(&mut self, number: f64) {
        self.add(number.to_bits());
    }
}

/// Everything the kept drawing image depends on: when any of it differs from the last paint's,
/// the drawing is painted again. The document's notifications count every command, undo,
/// selection change, library and current-layer change; the fingerprints catch what changes
/// without one - a reference layer shown or hidden in its panel, a mesh's style.
#[derive(Clone, Debug, PartialEq)]
struct DrawingKey {
    centre_x: f64,
    centre_y: f64,
    scale: f64,
    paper_style_scale: Option<f64>,
    width: f64,
    height: f64,
    device_ratio: f64,
    notifications: u64,
    model_revision: u64,
    library_generation: u64,
    selection: u64,
    references: u64,
    meshes: u64,
    layers: LayerOverrides,
    hidden_references: BTreeSet<u64>,
    grid: bool,
    thin_lines: bool,
    ghosts: bool,
}

/// What the tool host said through its hooks during one call, taken by the view after it.
enum HostSignal {
    /// The prompt line as it was when the hook was raised.
    Prompt(String),
    Message(String),
    Rejected(String),
    Started(String),
    Finished,
}

/// The running tool's prompt as the band and the command line show it: "Insert Vertex: Click on
/// polyline 2 where the new vertex goes".
fn prompt_line_of(tools: &ToolHost, document: &Document) -> String {
    match tools.info() {
        Some(info) if tools.active() => {
            format!("{}: {}", info.prompt_title(), tools.prompt(document))
        }
        _ => String::from(SELECT_PROMPT),
    }
}

/// Text a key press types, as opposed to a shortcut: every character printable and no Ctrl or Alt
/// held. Ctrl AND Alt together are let through, because that is how Windows reports AltGr, which
/// types @, { and the euro sign on most European keyboards - refusing it would make a coordinate
/// or a layer name untypable there.
fn is_typed_text(text: &str, modifiers: KeyModifiers) -> bool {
    if text.is_empty() {
        return false;
    }
    let chord = modifiers.ctrl || modifiers.alt || modifiers.meta;
    let alt_gr = modifiers.ctrl && modifiers.alt && !modifiers.meta;
    if chord && !alt_gr {
        return false;
    }
    text.chars().all(|character| !character.is_control())
}

fn to_screen_through(plan: &ViewTransform, world: Point2) -> PointF {
    let p = plan.world_to_screen(world);
    PointF::new(p.x, p.y)
}

fn to_world_through(plan: &ViewTransform, screen: PointF) -> Point2 {
    plan.screen_to_world(Point2::new(screen.x, screen.y))
}

/// What the snap marker is drawn from.
struct SnapMarks<'a> {
    plan: ViewTransform,
    object_tracking: bool,
    acquired: &'a [Point2],
    tracking_from: Option<Point2>,
    cursor_world: Point2,
    tracking_label: &'a str,
    active_snap: Option<SnapResult>,
}

fn polygon(canvas: &mut dyn Canvas, points: &[PointF]) {
    canvas.draw_polygon(points, FillRule::default());
}

fn circle(canvas: &mut dyn Canvas, centre: PointF, radius: f64) {
    canvas.draw_ellipse(RectF::new(
        centre.x - radius,
        centre.y - radius,
        2.0 * radius,
        2.0 * radius,
    ));
}

fn offset(p: PointF, dx: f64, dy: f64) -> PointF {
    PointF::new(p.x + dx, p.y + dy)
}

fn styled_pen(colour: Color, width: f64, style: PenStyle) -> Pen {
    Pen::with(colour, width, style, PenCap::Square, PenJoin::Bevel)
}

fn draw_snap_marker(canvas: &mut dyn Canvas, marks: &SnapMarks<'_>) {
    let colour = overlay::snap();
    let unsnapped = marks
        .active_snap
        .is_none_or(|snap| snap.mode == SnapMode::Grid);
    // Object snap tracking: a small cross on each acquired point, and the path the cursor is on,
    // dotted from the point it runs through.
    if marks.object_tracking {
        canvas.set_pen(Pen::new(colour));
        for &acquired in marks.acquired {
            let a = to_screen_through(&marks.plan, acquired);
            canvas.draw_line(offset(a, -4.0, 0.0), offset(a, 4.0, 0.0));
            canvas.draw_line(offset(a, 0.0, -4.0), offset(a, 0.0, 4.0));
        }
        if let Some(from) = marks.tracking_from {
            canvas.set_pen(styled_pen(colour, 1.0, PenStyle::Dot));
            canvas.draw_line(
                to_screen_through(&marks.plan, from),
                to_screen_through(&marks.plan, marks.cursor_world),
            );
        }
    }
    if !marks.tracking_label.is_empty() && unsnapped {
        // A drafting aid's tooltip: what constrained the point.
        let p = to_screen_through(&marks.plan, marks.cursor_world);
        canvas.set_pen(Pen::new(colour));
        canvas.set_font(overlay_font(11));
        canvas.draw_text(offset(p, 12.0, 18.0), marks.tracking_label);
    }
    let Some(snap) = marks.active_snap.filter(|snap| snap.mode != SnapMode::Grid) else {
        return;
    };
    let p = to_screen_through(&marks.plan, snap.point);
    let r = 6.0;
    canvas.set_pen(styled_pen(colour, 2.0, PenStyle::Solid));
    canvas.set_brush(Brush::none());
    let square = RectF::new(p.x - r, p.y - r, 2.0 * r, 2.0 * r);
    let diamond = [
        offset(p, 0.0, -r),
        offset(p, r, 0.0),
        offset(p, 0.0, r),
        offset(p, -r, 0.0),
    ];
    match snap.mode {
        SnapMode::Endpoint => canvas.draw_rect(square),
        SnapMode::Midpoint => polygon(
            canvas,
            &[offset(p, 0.0, -r), offset(p, r, r), offset(p, -r, r)],
        ),
        SnapMode::Center => circle(canvas, p, r),
        SnapMode::Intersection => {
            canvas.draw_line(offset(p, -r, -r), offset(p, r, r));
            canvas.draw_line(offset(p, -r, r), offset(p, r, -r));
        }
        SnapMode::Quadrant => {
            // A diamond on its point, filled.
            canvas.set_brush(Brush::solid(colour.with_alpha(90)));
            polygon(canvas, &diamond);
            canvas.set_brush(Brush::none());
        }
        SnapMode::Node => {
            // A circle with a cross in it.
            circle(canvas, p, r);
            canvas.draw_line(offset(p, -r * 0.7, -r * 0.7), offset(p, r * 0.7, r * 0.7));
            canvas.draw_line(offset(p, -r * 0.7, r * 0.7), offset(p, r * 0.7, -r * 0.7));
        }
        SnapMode::Extension => {
            // Three dots along the carried-on line.
            for dx in [-r, 0.0, r] {
                circle(canvas, offset(p, dx, 0.0), 1.2);
            }
        }
        SnapMode::Parallel => {
            // Two slanted strokes.
            canvas.draw_line(offset(p, -r, r * 0.6), offset(p, -r * 0.2, -r));
            canvas.draw_line(offset(p, r * 0.2, r), offset(p, r, -r * 0.6));
        }
        SnapMode::ApparentIntersection => {
            // A cross in a square.
            canvas.draw_rect(square);
            canvas.draw_line(offset(p, -r, -r), offset(p, r, r));
            canvas.draw_line(offset(p, -r, r), offset(p, r, -r));
        }
        // Perpendicular, tangent, nearest: a diamond.
        _ => polygon(canvas, &diamond),
    }
    canvas.set_font(overlay_font(11));
    canvas.draw_text(offset(p, r + 4.0, -r - 2.0), to_string(snap.mode));
}

/// What the preview or a grip drag of `shape` reads of the drawing: for a smart leader, the model
/// and the surfaces and alignments its note reads (`leaders` made on the first, so a preview of
/// anything else costs nothing); None for any other shape.
fn shape_source<'a>(
    document: &'a Document,
    shape: &Geometry,
    leaders: &mut Option<Arc<LeaderContext>>,
) -> Option<PlanSource<'a>> {
    if !matches!(shape, Geometry::Leader(_)) {
        return None;
    }
    let context = leaders.get_or_insert_with(|| Arc::new(leader_context(document)));
    Some(PlanSource {
        model: Some(document.model()),
        leaders: Some(Arc::clone(context)),
        ..PlanSource::default()
    })
}

/// The rectangle a wrapped text would take in `rect`, measured and not drawn. The canvas answers
/// it from a drawing in a rectangle (QPainter::drawText's boundingRect), and with no pen it lays
/// the text out and inks nothing: the font metrics have no wrapped measure of their own.
fn wrapped_bounds(
    canvas: &mut dyn Canvas,
    font: &Font,
    rect: RectF,
    flags: TextFlags,
    text: &str,
) -> RectF {
    canvas.save();
    canvas.set_pen(Pen::none());
    canvas.set_font(font.clone());
    let bounds = canvas.draw_text_in(rect, flags, text);
    canvas.restore();
    bounds
}

/// A blank grid gives a new user nothing to go on, so an empty drawing says what can be done with
/// it. Only for a drawing with NOTHING in it: a drawing whose layers are all hidden is not empty,
/// and saying so would be wrong.
fn draw_empty_hint(canvas: &mut dyn Canvas, width: i32, height: i32) {
    let body = ui_font();
    let headline = Font {
        size: match body.size {
            FontSize::Points(points) => FontSize::Points(points * 1.4),
            FontSize::Pixels(pixels) => {
                FontSize::Pixels((f64::from(pixels) * 1.4).round() as i32)
            }
        },
        weight: FontWeight::Bold,
        ..body.clone()
    };
    let title = "Nothing drawn yet";
    let detail = "Pick a drawing tool, or type a command such as LINE, CIRCLE or HELP in the \
                  command line.\nBring data in with File > Import, or from the GIS menu.";

    let margin = 24;
    // QRect::adjusted on the view's rectangle: its width and height less the margins.
    let area = IntRect::new(margin, margin, width - 2 * margin, height - 2 * margin);
    let title_height = canvas.metrics().height(&headline).round() as i32;
    let title_width = canvas.metrics().horizontal_advance(&headline, title).round() as i32;
    // About seventy characters of the interface font: a line that can be read at a glance rather
    // than one stretched across a wide view.
    let text_width = area.width.min(460);
    let wrapped = TextFlags::aligned(HAlign::HCenter, VAlign::Top).wrapped();
    let body_bounds = wrapped_bounds(
        canvas,
        &body,
        RectF::new(0.0, 0.0, f64::from(text_width), f64::from(area.height)),
        wrapped,
        detail,
    );
    let body_width = body_bounds.width.ceil() as i32;
    let body_height = body_bounds.height.ceil() as i32;
    let gap = 8;
    let block_height = title_height + gap + body_height;
    // A view too small to hold the hint shows the grid alone: a hint cut off at the edges says
    // less than none.
    if text_width < title_width || block_height > area.height {
        return;
    }
    // QRect::center: the middle pixel, the lower and lefter of two.
    let centre_x = area.x + (area.width - 1) / 2;
    let centre_y = area.y + (area.height - 1) / 2;
    let top = centre_y - block_height / 2;
    let left = centre_x - text_width / 2;
    let used = body_width.max(title_width);

    canvas.save();
    canvas.set_render_hint(RenderHint::TextAntialiasing, true);
    // A backing of the drawing's own ground, so the grid's axes, which cross right where the hint
    // sits, do not strike through the words.
    canvas.set_pen(Pen::none());
    canvas.set_brush(Brush::solid(palette::viewport().with_alpha(225)));
    canvas.draw_rounded_rect(
        RectF::new(
            f64::from(centre_x - used / 2 - 16),
            f64::from(top - 12),
            f64::from(used + 32),
            f64::from(block_height + 24),
        ),
        6.0,
        6.0,
    );
    canvas.set_brush(Brush::none());
    // The chrome's muted text, so the hint reads as the application talking and not as something
    // drawn.
    canvas.set_pen(Pen::new(palette::text_muted()));
    canvas.set_font(headline);
    canvas.draw_text_in(
        RectF::new(
            f64::from(left),
            f64::from(top),
            f64::from(text_width),
            f64::from(title_height),
        ),
        TextFlags::aligned(HAlign::HCenter, VAlign::Top),
        title,
    );
    canvas.set_font(body);
    canvas.draw_text_in(
        RectF::new(
            f64::from(left),
            f64::from(top + title_height + gap),
            f64::from(text_width),
            f64::from(body_height),
        ),
        wrapped,
        detail,
    );
    canvas.restore();
}

/// What egui's side of the view remembers between frames.
#[derive(Default)]
struct EguiSide {
    texture: Option<egui::TextureHandle>,
    /// Whether the pointer was over the view at the last frame.
    inside: bool,
    /// The buttons pressed in the view and not yet released there.
    held: Vec<MouseButton>,
    /// The last press: when, where and with which button, to tell a double click.
    last_press: Option<(f64, PointF, MouseButton)>,
}

/// See the module comment.
pub struct PlanView {
    // First, so it is dropped first: nothing counts into a half-dropped view.
    registration: ListenerHandle,
    /// Counted by the document listener: one per notification. The kept drawing is stale when it
    /// has moved (DrawingKey).
    notifications: Arc<AtomicU64>,
    name: String,
    width: i32,
    height: i32,
    device_ratio: f64,
    last_drawn_entities: usize,
    last_ghosts: usize,
    last_stats: PlanPaintStats,
    empty_hint_shown: bool,
    reported_surface_problems: Vec<String>,
    framed_box: Option<Box2>,

    /// The tool running in this view, if any.
    tools: ToolHost,
    signals: Rc<RefCell<Vec<HostSignal>>>,
    /// The selection's grips.
    grips: GripController,
    /// Where a grip press began, to tell a drag from a click.
    grip_press: PointF,
    typed: String,
    /// What each change at the running tool's selection step replaced, for Ctrl+Z there. A click
    /// or box there changes the DOCUMENT's selection, which the tool reads at Enter, so the tool
    /// never sees it and cannot step it back. An entry is the selection before a click or box
    /// changed it, or None for an input the tool took itself (All), which the tool steps back:
    /// one list, so both go back in the order made.
    selection_steps: Vec<Option<Vec<EntityId>>>,
    /// The ToolHost::generation the steps are for.
    selection_steps_of: u64,
    /// The tool Enter at no prompt runs again when the owner repeats none; "" until one has run
    /// here.
    last_tool_id: String,
    last_preview_count: usize,
    last_preview_counts: PreviewCounts,
    /// After snapping.
    cursor_world: Point2,
    /// Whether the pointer is over this view: until it first is, cursor_world is a place nobody
    /// pointed at - the model's origin - and a preview there (a started tool's, a headless run's)
    /// would mark whatever vertex lies at 0,0; once it has left, cursor_world is where it left,
    /// and a tool started from a menu or a toolbar previewed there. Set by every move, cleared by
    /// `leave`.
    pointer_seen: bool,
    /// The prompt band's line as last drawn, cut to fit.
    prompt_band: String,
    active_snap: Option<SnapResult>,
    /// What constrained the cursor (Ortho, Polar 45°, a lock): its tooltip.
    tracking_label: String,
    /// Object snap tracking (docs/drawing.md): the points acquired, and the one the cursor's
    /// tracking path runs from, drawn dotted.
    tracking: TrackingPoints,
    tracking_from: Option<Point2>,

    grid_visible: bool,

    /// What the painter keeps between frames - flattened definitions, dash patterns, fonts,
    /// imagery and point-cloud colours - owned here, one per view, and a second for plots so a
    /// plot does not throw away the screen's.
    paint_cache: PlanPaintCache,
    plot_cache: PlanPaintCache,
    /// What a symbol sprite is rasterised with and annotation text measured with.
    raster: Arc<dyn RasterBackend>,

    /// The drawing as last painted, and what it was painted for.
    drawing: Image,
    drawing_key: Option<DrawingKey>,
    drawing_paints: usize,
    /// Counted when the window says its reference data changed.
    reference_revision: u64,
    last_frame_ms: f64,
    last_drawing_ms: f64,
    /// The last paint whole: the drawing with the furniture over it.
    picture: Image,

    panning: bool,
    last_mouse: PointF,
    /// The selection rectangle's anchor (screen).
    box_start: Option<PointF>,
    box_end: PointF,

    /// Whether a point pick is waiting for this view's next click (`await_point_pick`).
    point_pick: bool,

    /// Which hooks the owner has set (see the module comment).
    pub hooks: PlanViewHooks,
    events: Vec<PlanViewEvent>,
    egui: EguiSide,
}

impl PlanView {
    /// A plan view of `document` through `state`. Only a view that has never been framed starts
    /// at the initial zoom: one rebuilt after a change of kind keeps the pan and zoom it had.
    pub fn new(document: &Document, state: &mut ViewState) -> Self {
        if !state.plan_framed {
            state.plan.scale = INITIAL_SCALE;
        }
        let notifications = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&notifications);
        let registration = document.add_listener(move || {
            counter.fetch_add(1, Ordering::AcqRel);
        });
        let mut view = Self {
            registration,
            notifications,
            // What a headless run reports it by (docs/headless.md), as its dock is View<id>.
            name: format!("PlanView{}", state.id),
            width: UNSIZED_WIDTH,
            height: UNSIZED_HEIGHT,
            device_ratio: 1.0,
            last_drawn_entities: 0,
            last_ghosts: 0,
            last_stats: PlanPaintStats::default(),
            empty_hint_shown: false,
            reported_surface_problems: Vec::new(),
            framed_box: None,
            tools: ToolHost::new(),
            signals: Rc::new(RefCell::new(Vec::new())),
            grips: GripController::new(),
            grip_press: PointF::default(),
            typed: String::new(),
            selection_steps: Vec::new(),
            selection_steps_of: 0,
            last_tool_id: String::new(),
            last_preview_count: 0,
            last_preview_counts: PreviewCounts::default(),
            cursor_world: Point2::default(),
            pointer_seen: false,
            prompt_band: String::new(),
            active_snap: None,
            tracking_label: String::new(),
            tracking: TrackingPoints::default(),
            tracking_from: None,
            grid_visible: true,
            paint_cache: PlanPaintCache::new(),
            plot_cache: PlanPaintCache::new(),
            raster: Arc::new(CanvasRasterBackend),
            drawing: Image::default(),
            drawing_key: None,
            drawing_paints: 0,
            reference_revision: 0,
            last_frame_ms: 0.0,
            last_drawing_ms: 0.0,
            picture: Image::default(),
            panning: false,
            last_mouse: PointF::default(),
            box_start: None,
            box_end: PointF::default(),
            point_pick: false,
            hooks: PlanViewHooks::default(),
            events: Vec::new(),
            egui: EguiSide::default(),
        };
        view.wire_tool_host();
        view
    }

    /// The tool host's hooks, each saying what it was told into the view's own queue, which the
    /// view takes after the call (`take_signals`): a hook cannot keep the view.
    fn wire_tool_host(&mut self) {
        let signals = Rc::clone(&self.signals);
        self.tools.on_prompt = Some(text_hook(move |tools, document, _prompt| {
            let line = prompt_line_of(tools, document);
            signals.borrow_mut().push(HostSignal::Prompt(line));
        }));
        let signals = Rc::clone(&self.signals);
        self.tools.on_message = Some(text_hook(move |_tools, _document, message| {
            signals
                .borrow_mut()
                .push(HostSignal::Message(String::from(message)));
        }));
        let signals = Rc::clone(&self.signals);
        self.tools.on_rejected = Some(text_hook(move |_tools, _document, message| {
            signals
                .borrow_mut()
                .push(HostSignal::Rejected(String::from(message)));
        }));
        let signals = Rc::clone(&self.signals);
        self.tools.on_started = Some(text_hook(move |_tools, _document, id| {
            signals
                .borrow_mut()
                .push(HostSignal::Started(String::from(id)));
        }));
        let signals = Rc::clone(&self.signals);
        self.tools.on_finished = Some(text_hook(move |_tools, _document, _id| {
            signals.borrow_mut().push(HostSignal::Finished);
        }));
    }

    /// What the tool host said during the call just made, as the C++ hooks acted on it.
    fn take_signals(&mut self) {
        let said: Vec<HostSignal> = std::mem::take(&mut *self.signals.borrow_mut());
        for signal in said {
            match signal {
                HostSignal::Prompt(line) => self.events.push(PlanViewEvent::Prompt(line)),
                HostSignal::Message(message) => {
                    self.events.push(PlanViewEvent::ToolMessage(message));
                }
                HostSignal::Rejected(message) => self.events.push(PlanViewEvent::Error(message)),
                HostSignal::Started(id) => {
                    self.last_tool_id.clone_from(&id);
                    let legacy = legacy_tool(&id);
                    self.events.push(PlanViewEvent::ActiveToolChanged(id));
                    if let Some(legacy) = legacy {
                        self.events.push(PlanViewEvent::ToolChanged(legacy));
                    }
                }
                HostSignal::Finished => {
                    self.typed.clear();
                    self.events
                        .push(PlanViewEvent::ActiveToolChanged(String::new()));
                    self.events.push(PlanViewEvent::ToolChanged(Tool::Select));
                }
            }
        }
    }

    /// What a grip's put-down, Delete or typed point was refused with.
    fn take_grip_errors(&mut self) {
        for error in std::mem::take(&mut self.grips.errors) {
            self.events.push(PlanViewEvent::Error(error));
        }
    }

    /// The view's apertures and hidden layers as they are now, written to the tool host before a
    /// call that may ask the tool: at every preview and click, so a zoom inside the tool is
    /// followed, and a layer hidden in this view is never picked through.
    fn lend_tools(&mut self, state: &ViewState) {
        self.tools.set_apertures(
            Some(state.plan.pixels_to_world(PICK_APERTURE_PIXELS)),
            Some(state.plan.pixels_to_world(SNAP_APERTURE_PIXELS)),
        );
        self.tools.set_view(Some(&state.layers));
    }

    fn notified(&self) -> u64 {
        self.notifications.load(Ordering::Acquire)
    }

    // ---- what the view is ---------------------------------------------------------------------

    /// The view's object name, "PlanView<id>".
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The view's size in its own pixels.
    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    /// False once the document the view was made over has been dropped.
    pub fn document_alive(&self) -> bool {
        self.registration.active()
    }

    /// What the view has raised since the last call of this, in order.
    pub fn take_events(&mut self) -> Vec<PlanViewEvent> {
        std::mem::take(&mut self.events)
    }

    /// A point pick waits for this view's next click, or waits no more (geo/point_pick, whose
    /// C++ filtered the view's events: an immediate-mode view takes the click itself). While one
    /// waits, a left press is the point - in ground units, through this view's transform, where
    /// the cursor really is - and selects nothing; a right press or Esc gives no point; either
    /// ends the wait and is raised as PointPicked. Middle-button panning and the wheel pass
    /// through, so the person can move the view to the place first. The workspace sets it on
    /// every plan view and clears the others when one answers (ViewAccess::pick_point).
    pub fn await_point_pick(&mut self, waiting: bool) {
        self.point_pick = waiting;
    }

    pub fn point_pick_waiting(&self) -> bool {
        self.point_pick
    }

    /// The waiting point pick's answer to a press of `button` at `at`, when it takes the press.
    fn pick_takes_press(&mut self, state: &ViewState, at: PointF, button: MouseButton) -> bool {
        if !self.point_pick || !matches!(button, MouseButton::Left | MouseButton::Right) {
            return false;
        }
        self.point_pick = false;
        let point = (button == MouseButton::Left).then(|| to_world_through(&state.plan, at));
        self.events.push(PlanViewEvent::PointPicked(point));
        true
    }

    /// The pointer's shape over the view: a closed hand while a middle drag pans.
    pub fn cursor(&self) -> ViewCursor {
        if self.panning {
            ViewCursor::ClosedHand
        } else {
            ViewCursor::Cross
        }
    }

    /// How many entities the last paint (or plot) drew: those that passed the visibility rule for
    /// this view and lay in the visible area. For the headless checks, which cannot look at
    /// pixels, to prove that a layer hidden in one view is gone from that view and from no other.
    pub fn last_drawn_entity_count(&self) -> usize {
        self.last_drawn_entities
    }

    /// Of the selection, how many the last paint drew as ghosts: selected, on a layer the document
    /// shows and this view hides.
    pub fn last_ghost_count(&self) -> usize {
        self.last_ghosts
    }

    /// Surface display (docs/surface_display.md): what the last paint drew of the drawing's
    /// surfaces, for the headless report.
    pub fn last_paint_stats(&self) -> &PlanPaintStats {
        &self.last_stats
    }

    /// Whether the last paint laid the "Nothing drawn yet" hint over the view: for a drawing with
    /// nothing in it, no tool running.
    pub fn empty_hint_shown(&self) -> bool {
        self.empty_hint_shown
    }

    /// The grips this view offers on the selection as its last paint built them: none on an
    /// entity on a layer this view hides, however selected.
    pub fn grip_count(&self) -> usize {
        self.grips.grips().len()
    }

    /// How many times the drawing itself has been painted, for the tests that prove a move does
    /// not repaint it.
    pub fn drawing_paint_count(&self) -> usize {
        self.drawing_paints
    }

    /// The last paint, whole, and the last time the drawing was painted, in milliseconds.
    pub fn last_frame_milliseconds(&self) -> f64 {
        self.last_frame_ms
    }

    pub fn last_drawing_milliseconds(&self) -> f64 {
        self.last_drawing_ms
    }

    /// The last paint: the drawing with the view's furniture over it, in device pixels.
    pub fn picture(&self) -> &Image {
        &self.picture
    }

    /// The running tool's id, "" when the view is selecting.
    pub fn active_tool_id(&self) -> String {
        self.tools.active_id()
    }

    pub fn tool_active(&self) -> bool {
        self.tools.active()
    }

    /// What the running tool wants next; None when none runs.
    pub fn tool_expects(&self, document: &Document) -> Option<ToolInput> {
        self.tools.expects(document)
    }

    /// The host itself, for a caller that wants its hooks or to drive it.
    pub fn tool_host(&self) -> &ToolHost {
        &self.tools
    }

    pub fn tool_host_mut(&mut self) -> &mut ToolHost {
        &mut self.tools
    }

    /// The grips shown on the selection while no tool runs (docs/drawing.md).
    pub fn grip_controller(&self) -> &GripController {
        &self.grips
    }

    pub fn grip_controller_mut(&mut self) -> &mut GripController {
        &mut self.grips
    }

    /// The points object snap tracking has acquired, most recent first.
    pub fn tracking_points(&self) -> &[Point2] {
        self.tracking.points()
    }

    /// What has been typed into the view for the running tool and not yet entered: shown after
    /// the prompt, sent by Enter, cleared by Esc.
    pub fn typed_input(&self) -> &str {
        &self.typed
    }

    /// How many rubber-band shapes and markers the last paint drew, for the headless tests that
    /// cannot look at pixels.
    pub fn last_preview_count(&self) -> usize {
        self.last_preview_count
    }

    pub fn last_preview_counts(&self) -> &PreviewCounts {
        &self.last_preview_counts
    }

    /// The running tool's prompt as the band along the bottom of the view last drew it, cut to
    /// the view's width - in the middle with nothing typed, so the tool's name and what Enter
    /// does both stay; empty with no tool.
    pub fn prompt_band_text(&self) -> &str {
        &self.prompt_band
    }

    pub fn grid_visible(&self) -> bool {
        self.grid_visible
    }

    pub fn set_grid_visible(&mut self, visible: bool) {
        self.grid_visible = visible;
    }

    /// The tool Enter at no prompt runs again; "" until one has run here.
    pub fn last_tool_id(&self) -> &str {
        &self.last_tool_id
    }

    /// The snaps are the document's drafting settings, shared by every view and the SNAP verb.
    pub fn set_snap_enabled(&mut self, cx: &mut dyn PlanViewHost, enabled: bool) {
        cx.document_mut().drafting_mut().snap_enabled = enabled;
        self.active_snap = None;
    }

    pub fn snap_enabled(&self, cx: &dyn PlanViewHost) -> bool {
        cx.document().drafting().snap_enabled
    }

    pub fn set_snap_modes(&mut self, cx: &mut dyn PlanViewHost, modes: SnapModes) {
        cx.document_mut().drafting_mut().snap_modes = modes;
    }

    pub fn snap_modes(&self, cx: &dyn PlanViewHost) -> SnapModes {
        cx.document().drafting().snap_modes
    }

    /// Drops the cached images and per-point colours. Call after a reference layer is added,
    /// removed, or has its display settings changed.
    pub fn invalidate_reference_cache(&mut self) {
        self.paint_cache.invalidate_references();
        self.reference_revision += 1;
    }

    /// The view's pick aperture in model units at the current zoom.
    fn pick_tolerance(state: &ViewState) -> f64 {
        state.plan.pixels_to_world(PICK_APERTURE_PIXELS)
    }

    /// Its snap aperture, the reach of an Endpoint snap and of a vertex pick.
    fn snap_aperture(state: &ViewState) -> f64 {
        state.plan.pixels_to_world(SNAP_APERTURE_PIXELS)
    }

    /// Whether grips take the input now: no tool running.
    fn grips_live(&self) -> bool {
        !self.tools.active()
    }

    /// The running tool's prompt as the band and the command line show it.
    pub fn prompt_line(&self, document: &Document) -> String {
        prompt_line_of(&self.tools, document)
    }

    fn update_prompt(&mut self, document: &Document) {
        let line = self.prompt_line(document);
        self.events.push(PlanViewEvent::Prompt(line));
    }

    fn run(&mut self, document: &mut Document, command: CommandPtr) {
        if let Err(error) = document.execute(command) {
            self.events.push(PlanViewEvent::Error(error.describe()));
        }
    }

    // ---- public controls ----------------------------------------------------------------------

    /// Legacy: Select stops the running tool; the others start their catalogue tool (`tool_id`).
    pub fn set_tool(&mut self, cx: &mut dyn PlanViewHost, state: &mut ViewState, tool: Tool) {
        self.lend_tools(state);
        let document = cx.document_mut();
        if tool == Tool::Select {
            self.typed.clear();
            self.box_start = None;
            if self.tools.active() {
                // Its on_finished reports the change.
                self.tools.cancel(document);
                self.take_signals();
            } else {
                // Reported even when nothing was running, as it always was: the window checks
                // its Select button from this at start-up.
                self.events.push(PlanViewEvent::ToolChanged(Tool::Select));
            }
            self.update_prompt(document);
            return;
        }
        // Every id in tool_id is in the catalogue (a test asserts it), so this cannot fail short
        // of a catalogue that lost a tool, and then says so.
        if let Err(error) = self.start_tool_on(document, state, tool_id(tool)) {
            self.events.push(PlanViewEvent::Error(error.describe()));
        }
    }

    /// The legacy tool running, Select when none is or when the running tool is not one of the
    /// eight.
    pub fn tool(&self) -> Tool {
        legacy_tool(&self.tools.active_id()).unwrap_or(Tool::Select)
    }

    /// Starts catalogue tool `id` in this view. NotFound for an unknown id.
    pub fn start_tool(
        &mut self,
        cx: &mut dyn PlanViewHost,
        state: &mut ViewState,
        id: &str,
    ) -> Status {
        self.lend_tools(state);
        self.start_tool_on(cx.document_mut(), state, id)
    }

    fn start_tool_on(&mut self, document: &mut Document, state: &ViewState, id: &str) -> Status {
        self.box_start = None;
        self.typed.clear();
        self.tools.set_pick_tolerance(Self::pick_tolerance(state));
        // The vertices chosen before the tool - the hot grips - are its handles
        // (ToolStart::handles), given to it once. Only while no tool runs are the grips the
        // user's: a tool running now has hidden them.
        if self.grips_live() {
            self.grips
                .refresh(document, Some(&state.layers), self.notified());
            self.tools.set_handles(self.grips.hot().to_vec());
        }
        let started = self.tools.start(document, id);
        self.take_signals();
        if started.is_ok() {
            // The grips go with the tool: a grip picked up before it was put down by the first
            // click after it ended - a GRIP_EDIT nobody asked for, moving a vertex the tool may
            // have renumbered.
            self.grips.reset();
        } else {
            self.tools.set_handles(Vec::new());
        }
        started
    }

    /// Starts catalogue tool `id` with `grip` its handle - hot, as a click on the grip before the
    /// tool makes it: what the shortcut menu's tool items do (plan_context_menu's StartToolOn).
    pub fn start_tool_on_grip(
        &mut self,
        cx: &mut dyn PlanViewHost,
        state: &mut ViewState,
        id: &str,
        grip: Grip,
    ) -> Status {
        self.grips
            .refresh(cx.document(), Some(&state.layers), self.notified());
        self.grips.set_hot(vec![grip]);
        self.start_tool(cx, state, id)
    }

    /// The pointer at model point `at` as a person's mouse is: the view is painted, `at` mapped
    /// through its transform, a move sent there and, with `click`, a left press and release with
    /// `modifiers`; then painted again. For the headless --hover and --click steps
    /// (docs/headless.md), which have no mouse. Answers the record they print - "pointer:
    /// action=hover x= y= tool= expects= shapes= markers= target= added= removed= enter= focus=
    /// refused= caption= prompt=", the caption and the prompt quoted as every reply's text is
    /// (core's reply_quoted), so core's read_reply_record reads it back. InvalidArgument for a
    /// point outside the view.
    pub fn pointer_at(
        &mut self,
        cx: &mut dyn PlanViewHost,
        state: &mut ViewState,
        at: Point2,
        click: bool,
        modifiers: KeyModifiers,
    ) -> Result<String> {
        // Painted first, so the transform is the one on screen (the paint sizes it and frames a
        // view that has never been framed).
        self.paint(cx, state);
        let pixel = to_screen_through(&state.plan, at);
        let inside = pixel.x >= 0.0
            && pixel.y >= 0.0
            && pixel.x <= f64::from(self.width)
            && pixel.y <= f64::from(self.height);
        if !inside {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the point {},{} is outside the view ({} x {} px); zoom to it first",
                    format_exact_real(at.x),
                    format_exact_real(at.y),
                    self.width,
                    self.height
                ),
                "",
            ));
        }
        // The same calls a mouse makes: the view cannot tell this pointer from a person's, so a
        // headless run drives what a person does.
        self.mouse_move(cx, state, pixel, modifiers);
        if click {
            self.mouse_press(cx, state, pixel, MouseButton::Left, modifiers);
            self.mouse_release(cx, state, pixel, MouseButton::Left, modifiers);
        }
        self.paint(cx, state);
        let counts = &self.last_preview_counts;
        let document = cx.document();
        let expects = self.tools.expects(document);
        // One record, written as every verb's reply is: the roles' keys are their names, and the
        // caption and the prompt go through reply_quoted - Move Vertex's caption ends in a
        // bearing's seconds mark, and quoted by hand it closed the value early and the line read
        // back as no record at all.
        let role = |kind: FeedbackRole, n: usize| format!(" {}={}", to_string(kind), n);
        Ok(format!(
            "pointer: action={} x={} y={} tool={} expects={} shapes={} markers={}{}{}{}{} \
             focus={} refused={} caption={} prompt={}",
            if click { "click" } else { "hover" },
            format_exact_real(at.x),
            format_exact_real(at.y),
            if self.tools.active() {
                self.tools.active_id()
            } else {
                String::from("none")
            },
            expects.map_or("none", to_string),
            counts.shapes,
            counts.markers,
            role(FeedbackRole::Target, counts.target),
            role(FeedbackRole::Added, counts.added),
            role(FeedbackRole::Removed, counts.removed),
            role(FeedbackRole::Enter, counts.enter),
            counts.focus,
            if counts.refused { "yes" } else { "no" },
            reply_quoted(&counts.caption),
            reply_quoted(&self.prompt_line(document)),
        ))
    }

    /// A whole line of typed input handed to the running tool, as the command line hands it over
    /// when Enter is pressed there. False, and nothing done, when no tool is running: the line is
    /// then the command line's.
    pub fn type_into_tool(
        &mut self,
        cx: &mut dyn PlanViewHost,
        state: &mut ViewState,
        text: &str,
    ) -> bool {
        if !self.tools.active() {
            return false;
        }
        self.lend_tools(state);
        let document = cx.document_mut();
        self.typed.clear();
        self.send_typed(document, text);
        self.update_prompt(document);
        true
    }

    /// The box Zoom Extents frames: what THIS view draws - the entities its layers let through,
    /// the reference layers visible and not hidden here, the meshes and the alignments - through
    /// the one function a plot fits to as well. Empty when this view has nothing to show.
    pub fn drawn_bounds(&self, cx: &dyn PlanViewHost, state: &ViewState) -> Box2 {
        plan_drawn_bounds(&paint_source(cx), &state.layers, &state.hidden_references)
    }

    /// Frames what THIS view draws. An empty box is fitted too: the transform's fit then goes
    /// back to the origin at one pixel a unit, which is what Zoom Extents on nothing has always
    /// done.
    pub fn zoom_extents(&mut self, cx: &dyn PlanViewHost, state: &mut ViewState) {
        let bounds = self.drawn_bounds(cx, state);
        self.frame(state, bounds);
    }

    /// Frames one specific box, e.g. a single reference layer.
    pub fn zoom_to(&mut self, state: &mut ViewState, bounds: &Box2) {
        if bounds.empty() {
            return;
        }
        self.frame(state, *bounds);
    }

    /// Fits `bounds` into the view, and keeps fitting it on every resize until the user pans or
    /// zooms. A view is routinely framed and THEN resized: an import frames every view and then
    /// opens a 3D view beside the plan, which takes part of its space, and the plan kept a scale
    /// chosen for a size it no longer had - the drawing ran off its edges. Once the user has
    /// panned or zoomed, a resize keeps their centre and scale instead.
    fn frame(&mut self, state: &mut ViewState, bounds: Box2) {
        state
            .plan
            .resize(f64::from(self.width), f64::from(self.height));
        state.plan.fit(&bounds, FRAME_MARGIN);
        state.plan_framed = true;
        self.framed_box = Some(bounds);
        self.events.push(PlanViewEvent::ViewMoved { by_user: true });
    }

    /// What the first paint of a view that has never been framed does. At the first paint rather
    /// than the first resize, because a paint is at the size the view is seen at - a dock's first
    /// resize can be a provisional one from before the workspace laid it out.
    fn frame_on_first_paint(&mut self, cx: &dyn PlanViewHost, state: &mut ViewState) {
        state.plan_framed = true;
        let bounds = self.drawn_bounds(cx, state);
        if !bounds.empty() {
            state.plan.fit(&bounds, FRAME_MARGIN);
            self.framed_box = Some(bounds);
        }
        // Not the user's move: a view linked while it had never been seen leads the link from
        // here - or, framed on nothing, takes the link's place: it said nothing then, and the
        // link it led showed two places, both views linked=yes.
        self.events
            .push(PlanViewEvent::ViewMoved { by_user: false });
    }

    /// Keeps the view where its state now says, as a linked view must: forgets the box it was
    /// framed on, so a resize keeps the centre and scale rather than refitting that box. Raises
    /// nothing.
    pub fn hold_view(&mut self) {
        self.framed_box = None;
    }

    /// Frames the view as its first paint would, unless it has been framed: for a ZOOM asked of
    /// a view not yet painted - a script's VIEWS OPEN plan then ZOOM IN, which run with no paint
    /// between - so the zoom starts from what the view draws and not from the place a new view
    /// holds until then.
    pub fn frame_if_unframed(&mut self, cx: &dyn PlanViewHost, state: &mut ViewState) {
        // The paint's own first steps, at the size the view has now.
        state
            .plan
            .resize(f64::from(self.width), f64::from(self.height));
        if !state.plan_framed {
            self.frame_on_first_paint(cx, state);
        }
    }

    /// Esc: clears typed input first; then ends the running tool (keeping the work the tool's own
    /// cancel says Esc keeps); with no tool running, abandons a selection box, and then clears
    /// the selection.
    pub fn cancel(&mut self, cx: &mut dyn PlanViewHost, state: &mut ViewState) {
        self.lend_tools(state);
        self.cancel_on(cx.document_mut());
    }

    fn cancel_on(&mut self, document: &mut Document) {
        if self.grips_live() && self.grips.escape() {
            return;
        }
        if !self.typed.is_empty() {
            // As in a command line: the first Esc takes back what was typed.
            self.typed.clear();
        } else if self.tools.active() {
            // A selection box the tool was being given goes; the tool's on_finished resets the
            // prompt.
            self.box_start = None;
            self.tools.cancel(document);
            self.take_signals();
            return;
        } else if self.box_start.is_some() {
            self.box_start = None;
        } else if !document.selection().empty() {
            document.selection_mut().clear();
            document.notify_selection_changed();
        }
        self.update_prompt(document);
    }

    /// Abandons the operation in progress: ends the running tool WITHOUT committing anything and
    /// drops typed input and a selection box. Required whenever the document under the view is
    /// replaced: the collected clicks belong to the drawing that is going away, and committing
    /// them into the new one silently produces an entity at coordinates the user never picked
    /// there. Safe before or after the replacement, since nothing is rebuilt from the document
    /// here.
    pub fn reset_interaction(&mut self, cx: &mut dyn PlanViewHost) {
        let document = cx.document_mut();
        self.grips.reset();
        self.typed.clear();
        self.box_start = None;
        self.active_snap = None;
        // Its on_finished reports that the tool ended.
        self.tools.abandon(document);
        self.take_signals();
        self.update_prompt(document);
    }

    /// The grip under `screen` (a pixel of this view) within the pick aperture, when the grips
    /// are showing: what the shortcut menu is opened on, when it is opened on one.
    pub fn grip_at(
        &mut self,
        cx: &dyn PlanViewHost,
        state: &ViewState,
        screen: PointF,
    ) -> Option<Grip> {
        if !self.grips_live() {
            return None;
        }
        self.grips
            .refresh(cx.document(), Some(&state.layers), self.notified());
        // The reach a press on a grip has (mouse_press).
        grip_at(
            self.grips.grips(),
            to_world_through(&state.plan, screen),
            Self::pick_tolerance(state),
        )
    }

    /// Focus has arrived in this view for `reason`, from `previous`: Activated when that is the
    /// user choosing the view (view_focus's rule; the workspace says which way the focus came).
    pub fn focus_arrived(
        &mut self,
        reason: FocusReason,
        previous: Option<FocusPrevious>,
        view_floats: bool,
    ) {
        if focus_chooses_view(reason, previous, view_floats) {
            self.events.push(PlanViewEvent::Activated);
        }
    }

    // ---- interaction --------------------------------------------------------------------------

    fn update_cursor(&mut self, document: &Document, state: &ViewState, screen: PointF) {
        let raw = to_world_through(&state.plan, screen);
        self.cursor_world = raw;
        self.pointer_seen = true;
        self.active_snap = None;

        // Snapping serves a tool that wants a point, and a grip being dragged: a pick of an
        // entity or a selection is made where the cursor really is.
        let gripping = self.grips_live() && self.grips.active();
        let wants_point = self.tools.expects(document) == Some(ToolInput::Point) || gripping;
        let base = if gripping {
            self.grips.base()
        } else {
            self.tools.last_point(document)
        };
        let drafting = *document.drafting();
        let aperture = Self::snap_aperture(state);
        if drafting.snap_enabled && wants_point {
            let found = {
                let tools = &self.tools;
                let request = SnapRequest {
                    cursor: raw,
                    aperture,
                    modes: drafting.snap_modes,
                    from: base,
                    grid_spacing: if self.grid_visible {
                        grid_spacing(state.plan.scale, DEFAULT_GRID_PIXELS)
                    } else {
                        0.0
                    },
                    grid_origin: Point2::default(),
                    view: Some(&state.layers),
                    // Only a snap the tool's step can use: Insert Vertex's point must be ON the
                    // line, and an arc's centre or another string's end beside it took the cursor
                    // off (InteractiveTool::takes_snap). A grip's drag takes every snap.
                    accept: if gripping {
                        None
                    } else {
                        Some(Box::new(move |snap: &SnapResult| {
                            tools.takes_snap(document, snap)
                        }))
                    },
                };
                // Through the Document's spatial index. Measured in Release on 100 000 entities:
                // 4404 us per mouse move scanning, 88 us indexed. The answer is identical either
                // way, so this is purely the cost.
                snap(document.model(), &request, Some(document.spatial_index()))
            };
            self.active_snap = found;
            if let Some(found) = found {
                self.cursor_world = found.point;
                if drafting.object_tracking && TrackingPoints::tracks(found.mode) {
                    self.tracking.acquire(found.point, tolerance::GEOMETRIC);
                }
            }
        }
        if !wants_point || !drafting.object_tracking {
            self.tracking.clear();
        }
        self.tracking_from = None;
        // With no object snap, the drafting aids (drawing/drafting.rs): the locks, ortho or
        // polar from the base, and object snap tracking.
        if wants_point
            && self
                .active_snap
                .is_none_or(|snap| snap.mode == SnapMode::Grid)
        {
            let mut constrained = constrain(base, self.cursor_world, &drafting, aperture);
            // Tracking paths when nothing else fixed the point: the horizontal and vertical
            // through each acquired point, and where two cross.
            if constrained.label.is_empty()
                && drafting.object_tracking
                && let Some(tracked) =
                    track_acquired(self.tracking.points(), self.cursor_world, aperture)
            {
                constrained = tracked;
                self.tracking_from = constrained.path_from;
            }
            self.cursor_world = constrained.point;
            self.tracking_label = constrained.label;
        } else {
            self.tracking_label.clear();
        }
        self.tools.set_cursor(self.cursor_world);
        self.events.push(PlanViewEvent::CursorMoved {
            world: self.cursor_world,
            snap: self.active_snap,
        });
    }

    /// The entity under `screen` that this view lets be picked, if any.
    fn entity_at(document: &Document, state: &ViewState, screen: PointF) -> Option<EntityId> {
        let filter = SelectionFilter {
            view: Some(&state.layers),
            ..SelectionFilter::default()
        };
        pick_entity(
            document.model(),
            to_world_through(&state.plan, screen),
            Self::pick_tolerance(state),
            &filter,
            Some(document.spatial_index()),
        )
    }

    /// A left click while a tool runs, by what the tool expects.
    fn tool_click(&mut self, document: &mut Document, state: &ViewState, screen: PointF) {
        // The aperture at this zoom, for a tool that finds geometry near a point itself; a tool
        // reads it when it (re)starts.
        self.tools.set_pick_tolerance(Self::pick_tolerance(state));
        match self.tools.expects(document).unwrap_or(ToolInput::Point) {
            ToolInput::Entity => {
                // Where the cursor really is, not a snapped point: the pick is of what is under
                // it. Nothing there is still a click, and the tool either takes it as a point or
                // says what it wants instead.
                let at = to_world_through(&state.plan, screen);
                let _ = match Self::entity_at(document, state, screen) {
                    Some(picked) => self.tools.entity(document, picked, at),
                    None => self.tools.point(document, at, None),
                };
            }
            ToolInput::Point | ToolInput::Value => {
                self.update_cursor(document, state, screen);
                // With what the snap found, so a point of an entity is handed on as one (an
                // annotation made from it follows the entity).
                let snap = self.active_snap;
                let _ = self
                    .tools
                    .point(document, self.cursor_world, snap.as_ref());
            }
            // Selected at the release, as the Select tool does.
            ToolInput::Selection => {}
        }
        self.take_signals();
    }

    /// Enter, Space and a right-click while a tool runs: the typed input when there is some,
    /// otherwise the tool's Enter.
    fn tool_enter(&mut self, document: &mut Document) {
        if self.typed.is_empty() {
            let _ = self.tools.enter(document);
            self.take_signals();
        } else {
            let text = std::mem::take(&mut self.typed);
            self.send_typed(document, &text);
        }
        self.update_prompt(document);
    }

    /// Typed text for the running tool; U (or Undo) at a selection step is step_back, so it takes
    /// back a click as Ctrl+Z does.
    fn send_typed(&mut self, document: &mut Document, text: &str) {
        let selecting = self.tools.expects(document) == Some(ToolInput::Selection);
        let word = text.trim();
        if selecting && (word.eq_ignore_ascii_case("U") || word.eq_ignore_ascii_case("Undo")) {
            // The tool's own U would skip the clicks it never saw.
            self.step_back(document);
            return;
        }
        let outcome = self.tools.typed(document, text);
        self.take_signals();
        if selecting
            && outcome == Outcome::Continue
            && self.tools.expects(document) == Some(ToolInput::Selection)
        {
            // Taken by the tool at its selection step (All): the tool holds it, so Ctrl+Z hands
            // it back to the tool, in its turn.
            self.selection_steps_now().push(None);
        }
    }

    /// Ctrl+Z while a tool runs: at a selection step, the last change there; otherwise, or with
    /// none left, the tool's own undo.
    fn step_back(&mut self, document: &mut Document) {
        let selecting = self.tools.expects(document) == Some(ToolInput::Selection);
        if selecting && let Some(step) = self.selection_steps_now().pop() {
            if let Some(before) = step {
                {
                    let DocumentParts {
                        model, selection, ..
                    } = document.parts_mut();
                    selection.set(before);
                    // An entity deleted since (by a panel, not this tool) stays gone.
                    let _ = selection.prune(&model.entities);
                }
                document.notify_selection_changed();
                self.update_prompt(document);
                return;
            }
            // An input the tool took itself: the tool steps it back, below.
        }
        // A tool with nothing to step back says so (on_rejected, then an Error).
        let _ = self.tools.undo(document);
        self.take_signals();
        self.update_prompt(document);
    }

    /// The selection steps, emptied first if the tool they were kept for has been replaced,
    /// restarted or ended since (ToolHost::generation).
    fn selection_steps_now(&mut self) -> &mut Vec<Option<Vec<EntityId>>> {
        if self.selection_steps_of != self.tools.generation() {
            self.selection_steps.clear();
            self.selection_steps_of = self.tools.generation();
        }
        &mut self.selection_steps
    }

    fn select_at(
        document: &mut Document,
        state: &ViewState,
        screen: PointF,
        modifiers: KeyModifiers,
    ) {
        let picked = Self::entity_at(document, state, screen);
        let selection = document.selection_mut();
        if modifiers.ctrl {
            if let Some(picked) = picked {
                selection.toggle(picked);
            }
        } else if modifiers.shift {
            if let Some(picked) = picked {
                selection.add(picked);
            }
        } else {
            selection.clear();
            if let Some(picked) = picked {
                selection.add(picked);
            }
        }
        document.notify_selection_changed();
    }

    /// Before the shortcut menu opens at `screen`: the entity there becomes the selection when
    /// nothing is selected. A right-click on an entity with nothing selected means "this one", as
    /// a left click would have; with a selection, the click may miss what the menu is for, so the
    /// selection stands.
    fn select_for_menu(document: &mut Document, state: &ViewState, screen: PointF) {
        if !document.selection().empty() {
            return;
        }
        if let Some(picked) = Self::entity_at(document, state, screen) {
            document.selection_mut().add(picked);
            document.notify_selection_changed();
        }
    }

    /// What a box from `from` to `to` picks through this view's layers: what it encloses dragged
    /// left to right, what it touches right to left.
    fn picked_in_box(
        document: &Document,
        state: &ViewState,
        from: PointF,
        to: PointF,
    ) -> Vec<EntityId> {
        let mut bounds = Box2::default();
        bounds.expand(to_world_through(&state.plan, from));
        bounds.expand(to_world_through(&state.plan, to));
        // Dragging left to right selects what is fully inside; right to left also takes whatever
        // the box touches.
        let mode = if to.x >= from.x {
            BoxSelectionMode::Window
        } else {
            BoxSelectionMode::Crossing
        };
        let filter = SelectionFilter {
            view: Some(&state.layers),
            ..SelectionFilter::default()
        };
        pick_in_box(
            document.model(),
            bounds,
            mode,
            &filter,
            Some(document.spatial_index()),
        )
    }

    fn select_in_box(
        document: &mut Document,
        state: &ViewState,
        from: PointF,
        to: PointF,
        modifiers: KeyModifiers,
    ) {
        let picked = Self::picked_in_box(document, state, from, to);
        let selection = document.selection_mut();
        if !(modifiers.shift || modifiers.ctrl) {
            selection.clear();
        }
        for id in picked {
            selection.add(id);
        }
        document.notify_selection_changed();
    }

    /// A click (or, `dragged`, a box) at a running tool's selection step. It GATHERS, as AutoCAD's
    /// "Select objects" does: a plain click or box adds what it picks, and one with Shift or Ctrl
    /// held (`take_out`) removes it. Replacing the selection at each plain click, as the Select
    /// tool does, would leave only the last of several cutting edges picked; and the Select
    /// tool's Ctrl toggles and its Shift adds, so neither could take a whole window of picks back
    /// out.
    fn gather_for_tool(
        &mut self,
        document: &mut Document,
        state: &ViewState,
        from: PointF,
        to: PointF,
        dragged: bool,
        take_out: bool,
    ) {
        let picked = if dragged {
            Self::picked_in_box(document, state, from, to)
        } else {
            Self::entity_at(document, state, to).into_iter().collect()
        };
        let before = document.selection().ids();
        {
            let selection = document.selection_mut();
            for id in picked {
                if take_out {
                    selection.remove(id);
                } else {
                    selection.add(id);
                }
            }
        }
        // All adds or all removes, so the count says whether anything changed.
        if document.selection().size() == before.len() {
            return;
        }
        self.selection_steps_now().push(Some(before));
        document.notify_selection_changed();
        // The prompt counts what is picked (Trim's "<use 2 edges>"), and the command line hears
        // it only through the Prompt event.
        self.update_prompt(document);
    }

    /// QWidget::resizeEvent: the view is `width` by `height` of its own pixels now. A view still
    /// showing what it was framed to goes on showing it at the new size. One the user has panned
    /// or zoomed keeps their centre and scale: resizing a dock must not undo their zoom.
    pub fn resize(&mut self, state: &mut ViewState, width: i32, height: i32) {
        self.width = width.max(1);
        self.height = height.max(1);
        state
            .plan
            .resize(f64::from(self.width), f64::from(self.height));
        if let Some(framed) = self.framed_box {
            state.plan.fit(&framed, FRAME_MARGIN);
        }
    }

    /// The device pixels a pixel of the view is (QWidget::devicePixelRatioF): the kept drawing is
    /// the view's size in DEVICE pixels, so on a scaled display it is as sharp as painting the
    /// view directly.
    pub fn set_device_ratio(&mut self, ratio: f64) {
        if ratio.is_finite() && ratio > 0.0 {
            self.device_ratio = ratio;
        }
    }

    pub fn mouse_press(
        &mut self,
        cx: &mut dyn PlanViewHost,
        state: &mut ViewState,
        at: PointF,
        button: MouseButton,
        modifiers: KeyModifiers,
    ) {
        if self.pick_takes_press(state, at, button) {
            return;
        }
        self.lend_tools(state);
        let document = cx.document_mut();
        self.events.push(PlanViewEvent::Activated);
        self.last_mouse = at;
        match button {
            MouseButton::Middle => {
                self.panning = true;
                return;
            }
            MouseButton::Right => {
                if self.grips_live() && self.grips.base().is_some() {
                    // A grip picked up is dropped, as Esc drops it.
                    self.grips.escape();
                    return;
                }
                if self.tools.active() {
                    // Enter, as in AutoCAD with its shortcut menu off: it finishes a chain, ends
                    // a selection or takes the prompt's default.
                    self.box_start = None;
                    self.tool_enter(document);
                } else if self.box_start.is_none() && self.hooks.context_menu {
                    // Nothing to finish or cancel, and a cancel here would only have cleared the
                    // selection the menu is about to act on.
                    Self::select_for_menu(document, state, at);
                    self.events.push(PlanViewEvent::ContextMenu { at });
                } else {
                    self.cancel_on(document);
                }
                return;
            }
            MouseButton::Other => return,
            MouseButton::Left => {}
        }
        // A grip under the cursor (or one picked up and waiting to be put down) takes the click
        // before the selection does.
        if self.grips_live() {
            self.grips
                .refresh(document, Some(&state.layers), self.notified());
            self.update_cursor(document, state, at);
            let taken = self.grips.press(
                document,
                to_world_through(&state.plan, at),
                Self::pick_tolerance(state),
                modifiers.shift,
                modifiers.ctrl,
            );
            self.take_grip_errors();
            if taken {
                self.grip_press = at;
                return;
            }
        }
        // Selecting, with no tool or for a tool that asks for a selection: a click or a box,
        // settled at the release.
        if !self.tools.active() || self.tools.expects(document) == Some(ToolInput::Selection) {
            self.box_start = Some(at);
            self.box_end = at;
            return;
        }
        self.tool_click(document, state, at);
    }

    pub fn mouse_move(
        &mut self,
        cx: &mut dyn PlanViewHost,
        state: &mut ViewState,
        at: PointF,
        modifiers: KeyModifiers,
    ) {
        self.lend_tools(state);
        let document = cx.document_mut();
        if self.panning {
            let delta = at - self.last_mouse;
            state.plan.pan_by_pixels(delta.x, delta.y);
            // The user's view now, kept on a resize.
            self.framed_box = None;
            self.events.push(PlanViewEvent::ViewMoved { by_user: true });
        } else if self.box_start.is_some() {
            self.box_end = at;
        }
        self.last_mouse = at;
        self.update_cursor(document, state, at);
        // Ctrl as the mouse reports it, where the key went to another widget.
        self.grips.set_ctrl(modifiers.ctrl);
        if self.grips_live() {
            self.grips
                .refresh(document, Some(&state.layers), self.notified());
            let moved = at - self.grip_press;
            let raw = to_world_through(&state.plan, at);
            let moved_pixels = if self.grips.pressed() {
                moved.x.hypot(moved.y)
            } else {
                0.0
            };
            self.grips.move_to(
                self.cursor_world,
                raw,
                Self::pick_tolerance(state),
                moved_pixels,
            );
            if self.grips.active() {
                // Now snapped from the grip's base.
                self.update_cursor(document, state, at);
                self.grips
                    .move_to(self.cursor_world, raw, Self::pick_tolerance(state), 0.0);
            }
        }
    }

    pub fn mouse_release(
        &mut self,
        cx: &mut dyn PlanViewHost,
        state: &mut ViewState,
        at: PointF,
        button: MouseButton,
        modifiers: KeyModifiers,
    ) {
        self.lend_tools(state);
        let document = cx.document_mut();
        if button == MouseButton::Middle {
            self.panning = false;
            return;
        }
        if button != MouseButton::Left {
            return;
        }
        if self.grips_live() {
            let taken = self.grips.release(document);
            self.take_grip_errors();
            if taken {
                return;
            }
        }
        if let Some(from) = self.box_start.take() {
            let to = at;
            let dragged = (to.x - from.x).abs() > DRAG_THRESHOLD_PIXELS
                || (to.y - from.y).abs() > DRAG_THRESHOLD_PIXELS;
            if self.tools.expects(document) == Some(ToolInput::Selection) {
                self.gather_for_tool(
                    document,
                    state,
                    from,
                    to,
                    dragged,
                    modifiers.shift || modifiers.ctrl,
                );
            } else if dragged {
                Self::select_in_box(document, state, from, to, modifiers);
            } else {
                Self::select_at(document, state, to, modifiers);
            }
        }
    }

    /// Qt delivers a double click as press, release, DOUBLE CLICK, release: this is the second
    /// press, which never reaches `mouse_press`. Swallowing it means a user clicking quickly
    /// while drawing silently loses that vertex, so a drawing tool has to consume it exactly as
    /// it would an ordinary press. With no tool, the first click has selected what is under the
    /// cursor and the second asks to edit it; empty space asks for nothing.
    pub fn mouse_double_click(
        &mut self,
        cx: &mut dyn PlanViewHost,
        state: &mut ViewState,
        at: PointF,
        button: MouseButton,
        modifiers: KeyModifiers,
    ) {
        if button == MouseButton::Middle {
            if self.hooks.zoom_extents {
                self.events.push(PlanViewEvent::ZoomExtents);
            } else {
                self.zoom_extents(cx, state);
            }
            return;
        }
        if self.tools.active() {
            self.mouse_press(cx, state, at, button, modifiers);
            return;
        }
        if button != MouseButton::Left {
            return;
        }
        // The first click of the two picked a grip up; the double click asks for the entity's
        // editor, not a grip following the cursor until the next click puts it down somewhere.
        if self.grips_live() && self.grips.base().is_some() {
            self.grips.escape();
        }
        if let Some(id) = Self::entity_at(cx.document(), state, at) {
            self.events.push(PlanViewEvent::EntityDoubleClicked(id));
        }
    }

    /// The wheel turned over pixel `at` by `angle_delta` eighths of a degree, 120 a notch
    /// (QWheelEvent::angleDelta's y): positive, away from the person, zooms in about the pointer.
    pub fn wheel(
        &mut self,
        cx: &mut dyn PlanViewHost,
        state: &mut ViewState,
        at: PointF,
        angle_delta: f64,
    ) {
        self.lend_tools(state);
        let notches = angle_delta / 120.0;
        if notches != 0.0 {
            state
                .plan
                .zoom_at(Point2::new(at.x, at.y), WHEEL_ZOOM_STEP.powf(notches));
            // The user's view now, kept on a resize.
            self.framed_box = None;
            self.lend_tools(state);
            self.update_cursor(cx.document(), state, at);
            self.events.push(PlanViewEvent::ViewMoved { by_user: true });
        }
    }

    /// Whether the view claims `key` from the window's shortcuts (Qt's ShortcutOverride), asked
    /// before the window's Undo and Erase may have it.
    ///
    /// Ctrl+Z while a tool runs is the TOOL's: it steps back inside the tool - the last vertex of
    /// a chain, the last pick - as U does, and never undoes a command underneath a tool still
    /// holding points. Delete with a vertex grip hot is the VIEW's: it removes those vertices.
    /// The window's Erase holds Delete as a shortcut, and taking it there erased the whole
    /// polyline. So is Delete while a tool runs that takes it (a vertex tool): the polyline the
    /// tool is editing is selected - its chosen vertex's grips needed that, and the tool selects
    /// what it has just edited - and Erase took the whole string from under it.
    pub fn shortcut_override(
        &mut self,
        cx: &dyn PlanViewHost,
        state: &ViewState,
        key: ViewKey,
        modifiers: KeyModifiers,
    ) -> bool {
        self.lend_tools(state);
        let document = cx.document();
        if self.tools.active() && key == ViewKey::Z && modifiers.is_undo() {
            return true;
        }
        if key == ViewKey::Delete && modifiers == KeyModifiers::NONE {
            if self.tools.active() && self.tools.takes_delete(document) {
                return true;
            }
            if self.grips_live() {
                self.grips
                    .refresh(document, Some(&state.layers), self.notified());
                if self.grips.has_hot_vertex() {
                    return true;
                }
            }
        }
        false
    }

    pub fn key_release(&mut self, key: ViewKey) {
        if key == ViewKey::Control {
            self.grips.set_ctrl(false);
        }
    }

    /// The pointer has left the view: no preview until it is back. Off to a menu, a toolbar or
    /// another view: there is no cursor here to preview for until the pointer comes back, and
    /// where it left is not where a tool chosen meanwhile is being aimed.
    pub fn leave(&mut self) {
        self.pointer_seen = false;
    }

    /// A key pressed with the view focused: `key` the key, `text` what it types ("" for a key
    /// that types nothing). Answers whether the view took it; a key it did not take is the
    /// window's.
    pub fn key_press(
        &mut self,
        cx: &mut dyn PlanViewHost,
        state: &mut ViewState,
        key: ViewKey,
        modifiers: KeyModifiers,
        text: &str,
    ) -> bool {
        if self.point_pick && key == ViewKey::Escape {
            self.point_pick = false;
            self.events.push(PlanViewEvent::PointPicked(None));
            return true;
        }
        self.lend_tools(state);
        let document = cx.document_mut();
        if key == ViewKey::Control {
            // A segment middle under the cursor shows the vertex a Ctrl-drag adds.
            self.grips.set_ctrl(true);
        }
        if self.tools.active() {
            // While a tool runs, what is typed is ITS input - a coordinate, a distance, an option
            // letter - kept here and shown after the prompt until Enter sends it, rather than
            // going to the window as a command.
            match key {
                ViewKey::Escape => {
                    self.cancel_on(document);
                    return true;
                }
                ViewKey::Enter => {
                    self.tool_enter(document);
                    return true;
                }
                ViewKey::Backspace => {
                    self.typed.pop();
                    self.update_prompt(document);
                    return true;
                }
                ViewKey::Space => {
                    // Space is Enter, as in AutoCAD, except inside something typed for a tool
                    // that wants text, where it is a space.
                    if self.typed.is_empty()
                        || self.tools.expects(document) != Some(ToolInput::Value)
                    {
                        self.tool_enter(document);
                        return true;
                    }
                }
                ViewKey::Z => {
                    if modifiers.is_undo() {
                        // The drawing's Undo is Esc and then Ctrl+Z.
                        self.step_back(document);
                        return true;
                    }
                }
                ViewKey::Delete => {
                    // Claimed from the window's Erase (shortcut_override); the tool says what it
                    // means - Delete Vertex's Enter, or nothing, said.
                    if modifiers == KeyModifiers::NONE && self.tools.takes_delete(document) {
                        let _ = self.tools.delete_key(document);
                        self.take_signals();
                        self.update_prompt(document);
                        return true;
                    }
                }
                ViewKey::Control | ViewKey::Other => {}
            }
            if is_typed_text(text, modifiers) {
                self.typed.push_str(text);
                self.update_prompt(document);
                return true;
            }
            return false;
        }
        // A grip picked up takes what is typed - a point, a distance - and Enter; hot vertex
        // grips take Delete.
        if self.grips.base().is_some() {
            match key {
                ViewKey::Enter => {
                    self.grips.enter(document);
                    self.take_grip_errors();
                    return true;
                }
                ViewKey::Backspace => {
                    self.grips.backspace();
                    return true;
                }
                ViewKey::Delete => {}
                ViewKey::Escape => {
                    self.cancel_on(document);
                    return true;
                }
                ViewKey::Space | ViewKey::Z | ViewKey::Control | ViewKey::Other => {
                    let typed = is_typed_text(text, modifiers);
                    let a_word = text.chars().next().is_some_and(char::is_alphabetic);
                    if typed && self.grips.typed().is_empty() && a_word {
                        // A word, not a point: nothing a grip takes starts with a letter (x,y,
                        // @dx,dy, <bearing, a distance). A click on a vertex is how it is chosen,
                        // and the tool's name typed next went into the grip's point and was
                        // refused there; now the grip is put down, still chosen, and the word
                        // goes to the command line below.
                        self.grips.escape();
                    } else if typed && self.grips.type_text(text) {
                        return true;
                    }
                }
            }
        }
        if key == ViewKey::Delete {
            let deleted = self.grips.delete_hot(document);
            self.take_grip_errors();
            if deleted {
                return true;
            }
        }
        match key {
            ViewKey::Escape => {
                self.cancel_on(document);
                true
            }
            ViewKey::Enter | ViewKey::Space => {
                // Enter or Space at no prompt repeats the last tool, as AutoCAD repeats the last
                // command: drawing a run of circles is a click on Circle and then Enter between
                // them.
                if self.hooks.repeat_tool {
                    self.events.push(PlanViewEvent::RepeatTool);
                } else if !self.last_tool_id.is_empty() {
                    let id = self.last_tool_id.clone();
                    if let Err(error) = self.start_tool_on(document, state, &id) {
                        self.events.push(PlanViewEvent::Error(error.describe()));
                    }
                }
                true
            }
            ViewKey::Delete => {
                if !document.selection().empty() {
                    let ids = document.selection().ids();
                    self.run(document, delete_entities(ids));
                }
                true
            }
            ViewKey::Backspace | ViewKey::Z | ViewKey::Control | ViewKey::Other => {
                // Type anywhere: the text goes to the window, which hands it to the command line,
                // instead of being dropped here.
                if self.hooks.text_typed && is_typed_text(text, modifiers) {
                    self.events
                        .push(PlanViewEvent::TextTyped(String::from(text)));
                    return true;
                }
                false
            }
        }
    }

    /// The keyboard's menu key: the shortcut menu at the pointer when it is over the view
    /// (`pointer`, in the view's pixels) and in the middle of it otherwise. Nothing while a tool
    /// runs or a selection box is being dragged, and nothing when the owner shows no menu. (A
    /// right-click was dealt with at the press.)
    pub fn context_menu_key(
        &mut self,
        cx: &mut dyn PlanViewHost,
        state: &ViewState,
        pointer: Option<PointF>,
    ) {
        if !self.hooks.context_menu || self.tools.active() || self.box_start.is_some() {
            return;
        }
        let over = pointer.filter(|at| {
            at.x >= 0.0
                && at.y >= 0.0
                && at.x < f64::from(self.width)
                && at.y < f64::from(self.height)
        });
        if let Some(at) = over {
            Self::select_for_menu(cx.document_mut(), state, at);
        }
        // QRect::center: the middle pixel.
        let at = over.unwrap_or_else(|| {
            PointF::new(
                f64::from((self.width - 1) / 2),
                f64::from((self.height - 1) / 2),
            )
        });
        self.events.push(PlanViewEvent::ContextMenu { at });
    }

    // ---- painting -----------------------------------------------------------------------------

    fn screen_options<'a>(&self, document: &Document, state: &ViewState) -> PlanPaintOptions<'a> {
        PlanPaintOptions {
            medium: PlanMedium::Screen,
            // Auto keeps library paper marks in screen millimetres; the view's explicit 1:N
            // instead gives them a model size, independently of zoom.
            pixels_per_millimetre: (LOGICAL_DPI / 25.4).max(1.0),
            paper_style_scale: state.paper_style_scale,
            grid: self.grid_visible,
            thin_lines: thin_screen_lines(),
            symbol_sprites: true,
            // Paper-sized annotation at the document's annotation scale (ANNOSCALE,
            // docs/annotation.md); a change of it is a command, so the kept drawing is repainted
            // by the model revision it moves.
            annotation_scale: document.annotation_scale(),
            selection_ghosts: state.selection_ghosts,
            raster: Some(Arc::clone(&self.raster)),
            ..PlanPaintOptions::default()
        }
    }

    fn drawing_key(&self, cx: &dyn PlanViewHost, state: &ViewState) -> DrawingKey {
        let document = cx.document();
        // The selection is announced (Document::notify_selection_changed), but a caller that
        // changes it and only asks for a repaint must still see it drawn: a selection is the
        // ids, in order, and hashing even a whole drawing's is a fraction of a millisecond.
        let mut selection = Fingerprint::new();
        for id in document.selection().ids() {
            selection.add(id);
        }
        let mut references = Fingerprint::new();
        references.add(self.reference_revision);
        if let Some(reference) = cx.reference() {
            // The panel shows, hides and fades a layer without a notification.
            for raster in reference.rasters() {
                references.add(raster.id);
                references.add(u64::from(raster.visible));
                references.add_real(raster.opacity);
            }
            for cloud in reference.point_clouds() {
                references.add(cloud.id);
                references.add(u64::from(cloud.visible));
                references.add(cloud.color_mode as u64);
                references.add_real(cloud.point_size);
                references.add(cloud.points.len() as u64);
            }
        }
        let mut meshes = Fingerprint::new();
        let listed = cx.meshes();
        if !listed.is_empty() {
            // The window's list, which grows and restyles in place.
            meshes.add(listed.len() as u64);
            for mesh in listed {
                meshes.add(
                    mesh.mesh
                        .map_or(0, |mesh| std::ptr::from_ref(mesh) as usize as u64),
                );
                meshes.add(u64::from(mesh.visible));
                meshes.add(mesh.style as u64);
                meshes.add(u64::from(mesh.flat_color));
            }
        }
        DrawingKey {
            centre_x: state.plan.center.x,
            centre_y: state.plan.center.y,
            scale: state.plan.scale,
            paper_style_scale: state.paper_style_scale,
            width: state.plan.width_pixels,
            height: state.plan.height_pixels,
            device_ratio: self.device_ratio,
            notifications: self.notified(),
            model_revision: document.model_revision(),
            library_generation: document.library_generation(),
            selection: selection.value,
            references: references.value,
            meshes: meshes.value,
            layers: state.layers.clone(),
            hidden_references: state.hidden_references.clone(),
            grid: self.grid_visible,
            thin_lines: thin_screen_lines(),
            ghosts: state.selection_ghosts,
        }
    }

    /// True when nothing at all is loaded: no entity, alignment, surface, reference layer or
    /// mesh. What the empty-drawing hint is shown for. Surfaces count, hidden or not as the
    /// layers are: the plan view draws them, and the 3D view's own rule says the same.
    fn drawing_is_empty(cx: &dyn PlanViewHost) -> bool {
        let document = cx.document();
        let model = document.model();
        model.entities.empty()
            && model.alignments.empty()
            && document.surfaces().empty()
            && cx.reference().is_none_or(ReferenceData::empty)
            && cx.meshes().is_empty()
    }

    /// Tells the person, once each, why part of a surface's style is not drawn: a refusal that
    /// only the SURFACE PLAN verb said was a style whose contours vanished without a word.
    /// Forgets a problem that has gone, so it is told again if it returns.
    fn report_surface_problems(&mut self, problems: &[SurfaceProblem]) {
        let mut told = Vec::with_capacity(problems.len());
        for problem in problems {
            let text = problem.text();
            if !self.reported_surface_problems.contains(&text) {
                self.events.push(PlanViewEvent::Error(text.clone()));
            }
            told.push(text);
        }
        self.reported_surface_problems = told;
    }

    /// QWidget::paintEvent: the view painted at its size now, into `picture`.
    pub fn paint(&mut self, cx: &dyn PlanViewHost, state: &mut ViewState) -> &Image {
        let frame_timer = Instant::now();
        state
            .plan
            .resize(f64::from(self.width), f64::from(self.height));
        if !state.plan_framed {
            self.frame_on_first_paint(cx, state);
        }
        self.lend_tools(state);
        let document = cx.document();
        let state: &ViewState = state;
        let view_rect = RectF::new(0.0, 0.0, f64::from(self.width), f64::from(self.height));
        let frame = PlanFrame {
            transform: state.plan,
            layers: Some(&state.layers),
            hidden_references: Some(&state.hidden_references),
            ..PlanFrame::default()
        };
        let options = self.screen_options(document, state);

        // The drawing, through the one plan painter the plot uses too, into an image kept until
        // something it depends on changes. The image is the view's size in DEVICE pixels, so on
        // a scaled display it is as sharp as painting the view directly, and laying it down is a
        // copy.
        let key = self.drawing_key(cx, state);
        let stale = self.drawing_key.as_ref() != Some(&key) || self.drawing.is_null();
        if stale {
            let drawing_timer = Instant::now();
            let pixels_wide = ((f64::from(self.width) * self.device_ratio).ceil() as u32).max(1);
            let pixels_high = ((f64::from(self.height) * self.device_ratio).ceil() as u32).max(1);
            if self.drawing.width() != pixels_wide || self.drawing.height() != pixels_high {
                self.drawing = Image::new(pixels_wide, pixels_high);
            }
            self.drawing.set_device_pixel_ratio(self.device_ratio);
            let source = paint_source(cx);
            let mut stats = PlanPaintStats::default();
            {
                let cache = &mut self.paint_cache;
                RasterCanvas::paint_image(&mut self.drawing, &mut |canvas: &mut dyn Canvas| {
                    canvas.fill_rect(view_rect, &Brush::solid(palette::viewport()));
                    stats = paint_plan(canvas, &source, &frame, &options, cache);
                });
            }
            self.last_drawn_entities = stats.entities_drawn;
            self.last_ghosts = stats.ghosts_drawn;
            self.report_surface_problems(&stats.surface_problems);
            self.last_stats = stats;
            self.drawing_key = Some(key);
            self.drawing_paints += 1;
            self.last_drawing_ms = drawing_timer.elapsed().as_secs_f64() * 1.0e3;
        }

        // The view's own furniture over the drawing: what a tool is making, the hint for an
        // empty drawing, the selection box, the snap and the prompt.
        self.empty_hint_shown = Self::drawing_is_empty(cx) && !self.tools.active();
        if self.grips_live() {
            // The grips are rebuilt only when the document has changed since.
            self.grips
                .refresh(document, Some(&state.layers), self.notified());
        }
        let mut picture = self.drawing.clone();
        let mut preview_count = 0;
        let mut preview_counts = PreviewCounts::default();
        let mut prompt_band = String::new();
        {
            let marks = SnapMarks {
                plan: state.plan,
                object_tracking: document.drafting().object_tracking,
                acquired: self.tracking.points(),
                tracking_from: self.tracking_from,
                cursor_world: self.cursor_world,
                tracking_label: &self.tracking_label,
                active_snap: self.active_snap,
            };
            let unsnapped = self
                .active_snap
                .is_none_or(|snap| snap.mode == SnapMode::Grid);
            let tools = &self.tools;
            let grips = &self.grips;
            let cache = &mut self.paint_cache;
            let cursor_world = self.cursor_world;
            let pointer_seen = self.pointer_seen;
            let typed = self.typed.as_str();
            let empty_hint = self.empty_hint_shown;
            let box_start = self.box_start;
            let box_end = self.box_end;
            let (width, height) = (self.width, self.height);
            let band_rect = IntRect::new(0, 0, width, height);
            let plan = state.plan;
            RasterCanvas::paint_image(&mut picture, &mut |canvas: &mut dyn Canvas| {
                canvas.set_render_hint(RenderHint::Antialiasing, true);

                // The running tool's preview. Nothing until the pointer has been over this view:
                // before that the cursor is the origin, where nobody pointed.
                let mut snap_drawn = false;
                if tools.active() && pointer_seen {
                    let feedback = tools.feedback(document, cursor_world);
                    // A shape in the preview is drawn as the geometry it will become, so a
                    // dimension or a text previews at its real size. Its style is what new work
                    // gets (the current layer and style), and it is never hatched: a closed
                    // outline previews as its outline.
                    let attributes = document.current_attributes();
                    let mut drawn = Entity {
                        layer: attributes.layer,
                        style: attributes.style,
                        ..Entity::default()
                    };
                    let mut leaders: Option<Arc<LeaderContext>> = None;
                    // The polyline in play shows its vertices, since the grips are hidden while
                    // a tool runs.
                    let focus: Vec<Point2> = feedback
                        .focus
                        .and_then(|id| document.model().entities.find(id))
                        .and_then(read_polyline)
                        .map(|polyline| polyline.positions())
                        .unwrap_or_default();
                    let band = f64::from(band_height(canvas.metrics()));
                    let counts = {
                        let preview_cache = &mut *cache;
                        let mut where_drawn = FeedbackFrame {
                            to_screen: Box::new(move |p: Point2| to_screen_through(&plan, p)),
                            draw_shape: Box::new(
                                |canvas: &mut dyn Canvas, shape: &Geometry| {
                                    let style = if matches!(shape, Geometry::Dimension(_)) {
                                        drawn.geometry = shape.clone();
                                        resolve_dimension_style(document.model(), &drawn).clone()
                                    } else {
                                        DimensionStyle::default()
                                    };
                                    // A smart leader previews with its note, read off the
                                    // drawing as it will be once placed; without the drawing it
                                    // was a bare arrow.
                                    let source = shape_source(document, shape, &mut leaders);
                                    paint_plan_geometry(
                                        canvas,
                                        &frame,
                                        &options,
                                        preview_cache,
                                        shape,
                                        &style,
                                        source.as_ref(),
                                    );
                                },
                            ),
                            visible: view_rect,
                            cursor: to_screen_through(&plan, cursor_world),
                            tracking_label: !marks.tracking_label.is_empty() && unsnapped,
                            band_height: band,
                            // The snap marker goes under the preview's glyphs: where a snap puts
                            // the point, the tool's new vertex is drawn, and it must stay legible
                            // there.
                            beneath_glyphs: Some(Box::new(|canvas: &mut dyn Canvas| {
                                draw_snap_marker(canvas, &marks);
                            })),
                        };
                        paint_feedback(canvas, &feedback, &focus, &mut where_drawn)
                    };
                    preview_count = counts.shapes + counts.markers;
                    preview_counts = PreviewCounts {
                        shapes: counts.shapes,
                        markers: counts.markers,
                        target: counts.target,
                        added: counts.added,
                        removed: counts.removed,
                        enter: counts.enter,
                        focus: counts.focus,
                        refused: feedback.refused,
                        caption: feedback.caption.clone(),
                    };
                    snap_drawn = true;
                }

                // The grips of the selection, and a grip being dragged, over the drawing.
                if !tools.active() {
                    let mut leaders: Option<Arc<LeaderContext>> = None;
                    let grip_cache = &mut *cache;
                    grips.paint(
                        canvas,
                        document,
                        &|p: Point2| to_screen_through(&plan, p),
                        &mut |canvas: &mut dyn Canvas, shape: &Geometry| {
                            let source = shape_source(document, shape, &mut leaders);
                            paint_plan_geometry(
                                canvas,
                                &frame,
                                &options,
                                grip_cache,
                                shape,
                                &DimensionStyle::default(),
                                source.as_ref(),
                            );
                        },
                        view_rect.adjusted(-8.0, -8.0, 8.0, 8.0),
                    );
                    if grips.base().is_some() {
                        // The grip's prompt and what has been typed for it, where a tool's goes.
                        let _ = paint_band(canvas, band_rect, &grips.prompt(), Elide::Left);
                    } else {
                        // A grip under the cursor says what it is and what it offers.
                        let hint = grips.hover_hint(document);
                        if !hint.is_empty() {
                            let _ = paint_band(canvas, band_rect, &hint, Elide::Right);
                        }
                    }
                }

                if empty_hint {
                    draw_empty_hint(canvas, width, height);
                }

                if let Some(start) = box_start {
                    let window = box_end.x >= start.x;
                    let fill = if window {
                        overlay::window_box()
                    } else {
                        overlay::crossing_box()
                    };
                    canvas.set_pen(styled_pen(
                        lighter(fill, 160),
                        1.0,
                        if window {
                            PenStyle::Solid
                        } else {
                            PenStyle::Dash
                        },
                    ));
                    canvas.set_brush(Brush::solid(fill));
                    canvas.draw_rect(RectF::from_corners(start, box_end).normalized());
                    canvas.set_brush(Brush::none());
                }
                if !snap_drawn {
                    draw_snap_marker(canvas, &marks);
                }

                // The running tool's prompt and what has been typed for it, in a band along the
                // bottom of the view. What has been typed follows the prompt with a caret, as it
                // will be sent, and a line too long for the view is cut at the left so it stays
                // in sight. With nothing typed it is cut in the middle, keeping both the tool's
                // name - a cut at the left lost it at the larger text sizes - and the options and
                // default a prompt ends with ("[Other side]", "<2.5>").
                if tools.active() {
                    let line = format!("{}  {}_", prompt_line_of(tools, document), typed);
                    prompt_band = paint_band(
                        canvas,
                        band_rect,
                        &line,
                        if typed.is_empty() {
                            Elide::Middle
                        } else {
                            Elide::Left
                        },
                    );
                }
            });
        }
        self.picture = picture;
        self.last_preview_count = preview_count;
        self.last_preview_counts = preview_counts;
        self.prompt_band = prompt_band;

        self.last_frame_ms = frame_timer.elapsed().as_secs_f64() * 1.0e3;
        // The drawing's own time, and this frame's when it only laid the kept drawing down: a
        // mouse move reads "kept", a pan the drawing.
        let stats = if stale {
            format!(
                "Plan  {} drawn  {} ms",
                self.last_drawn_entities,
                qt_fixed(self.last_drawing_ms, 1)
            )
        } else {
            format!(
                "Plan  {} drawn  {} ms (kept, {} ms)",
                self.last_drawn_entities,
                qt_fixed(self.last_drawing_ms, 1),
                qt_fixed(self.last_frame_ms, 1)
            )
        };
        self.events.push(PlanViewEvent::FrameStats(stats));
        &self.picture
    }

    /// Plots the drawing as this view shows it - its hidden layers and reference layers - through
    /// plot_plan_to_pdf (plan_painter): the same painter as the screen, on paper. Fails as that
    /// does. `problems`, when given, receives what the plot is without of a surface's style.
    pub fn plot_to_pdf(
        &mut self,
        cx: &dyn PlanViewHost,
        state: &ViewState,
        path: &Path,
        settings: &PlotSettings,
        problems: Option<&mut Vec<SurfaceProblem>>,
        writer: &mut dyn PdfPageWriter,
    ) -> Status {
        // Titled with the project's name when it has one, so a sheet opened on its own says
        // which drawing it came from.
        let title = cx
            .document()
            .project_directory()
            .and_then(|directory| {
                directory
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        plot_plan_to_pdf(
            path,
            settings,
            &paint_source(cx),
            &state.layers,
            &state.hidden_references,
            &mut self.plot_cache,
            &title,
            problems,
            writer,
        )
    }

    /// `settings` fitted to what this view draws (`drawn_bounds`, as Zoom Extents frames it): the
    /// first standard scale the drawing fits the printable area at, centred on it; the sheet
    /// otherwise as given. What Plot's Fit plots. Fails with InvalidArgument when the view draws
    /// nothing, and with whatever cad's fit_scale refuses.
    pub fn fitted_plot(
        &self,
        cx: &dyn PlanViewHost,
        state: &ViewState,
        mut settings: PlotSettings,
    ) -> Result<PlotSettings> {
        // What this view draws - its layers, reference layers, meshes and alignments - and not
        // the spatial index's bounds, which never shrink, count every arc's whole circle and
        // hidden layers, and miss alignments (audit QT-12, GEO-01).
        let extent = self.drawn_bounds(cx, state);
        settings.scale_denominator = fit_scale(&extent, &settings)?;
        settings.center = Point2::new(
            0.5 * (extent.min.x + extent.max.x),
            0.5 * (extent.min.y + extent.max.y),
        );
        Ok(settings)
    }

    /// The drawing as this view shows it, painted by the same plan painter into an image of
    /// `width` by `height` pixels on `background` (transparent for none): the view's centre, at
    /// the scale that fits what the view shows into the image, paper-sized marks scaled alike -
    /// so twice the view's size is the view at twice the resolution. The view's furniture (the
    /// grid, a tool's preview, the snap marker, the prompt) is left out. `as_plotted` draws it as
    /// a plot does, for a white ground: white pens black (D7) and line weights in millimetres of
    /// the image. Read-only: the view and its kept drawing are untouched. For SNAPSHOT
    /// (plotting/view_image_export).
    pub fn render_to_image(
        &self,
        cx: &dyn PlanViewHost,
        state: &ViewState,
        width: u32,
        height: u32,
        background: Color,
        as_plotted: bool,
    ) -> Image {
        let mut image = Image::new(width, height);
        image.fill(background);
        let mut frame = PlanFrame {
            transform: state.plan,
            layers: Some(&state.layers),
            hidden_references: Some(&state.hidden_references),
            ..PlanFrame::default()
        };
        // As a plot draws on paper, at the image's own millimetre: the paper colour rule needs
        // the plot's settings, the defaults of which are a colour plot with white pens printing
        // black.
        let plotted = PlotSettings::default();
        let mut options = self.screen_options(cx.document(), state);
        // The grid helps a person place a point; in a picture of the drawing it is a mesh of
        // lines over it.
        options.grid = false;
        // Before the first paint the view has no size of its own, and the image is drawn at the
        // view's scale as it stands.
        if frame.transform.width_pixels > 1.0 && frame.transform.height_pixels > 1.0 {
            let enlarge = (f64::from(width) / frame.transform.width_pixels)
                .min(f64::from(height) / frame.transform.height_pixels);
            frame.transform.scale *= enlarge;
            options.pixels_per_millimetre *= enlarge;
        }
        frame.transform.resize(f64::from(width), f64::from(height));
        if as_plotted {
            options.medium = PlanMedium::Paper;
            options.plot = Some(&plotted);
            options.symbol_sprites = false;
        }
        // A cache of its own: the view's kept drawing and its cache stay as the screen left
        // them.
        let mut cache = PlanPaintCache::new();
        let source = paint_source(cx);
        RasterCanvas::paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
            let _ = paint_plan(canvas, &source, &frame, &options, &mut cache);
        });
        image
    }

    // ---- egui ---------------------------------------------------------------------------------

    /// The view drawn into the rectangle `ui` has left, never smaller than the minimum: the
    /// frame's input turned into the view's events, the view painted, and its picture shown. The
    /// view is the control "PlanView<id>" (a View record) in whatever target the caller has
    /// open. The caller reads `take_events` after.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut dyn PlanViewHost,
        state: &mut ViewState,
    ) -> egui::Response {
        let wanted = ui.available_size().max(egui::vec2(
            MINIMUM_WIDTH as f32,
            MINIMUM_HEIGHT as f32,
        ));
        let (rect, response) = ui.allocate_exact_size(wanted, egui::Sense::click_and_drag());
        let width = rect.width().round() as i32;
        let height = rect.height().round() as i32;
        if (width, height) != (self.width, self.height) {
            self.resize(state, width, height);
        }
        self.set_device_ratio(f64::from(ui.ctx().pixels_per_point()));
        // A pointer position of the window in the view's own pixels.
        let local = |at: egui::Pos2| {
            PointF::new(f64::from(at.x - rect.min.x), f64::from(at.y - rect.min.y))
        };
        let over = response.contains_pointer();
        let (events, now) = ui.input(|input| (input.events.clone(), input.time));
        for event in events {
            match event {
                egui::Event::PointerMoved(at) => {
                    if over || !self.egui.held.is_empty() {
                        let modifiers =
                            KeyModifiers::from_egui(ui.input(|input| input.modifiers));
                        self.mouse_move(cx, state, local(at), modifiers);
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    let button = match button {
                        egui::PointerButton::Primary => MouseButton::Left,
                        egui::PointerButton::Secondary => MouseButton::Right,
                        egui::PointerButton::Middle => MouseButton::Middle,
                        _ => MouseButton::Other,
                    };
                    let modifiers = KeyModifiers::from_egui(modifiers);
                    let at = local(pos);
                    if pressed {
                        if !over {
                            continue;
                        }
                        response.request_focus();
                        self.egui.held.push(button);
                        let double = self.egui.last_press.is_some_and(|(when, place, which)| {
                            which == button
                                && now - when <= DOUBLE_CLICK_SECONDS
                                && (at.x - place.x).abs() <= DOUBLE_CLICK_PIXELS
                                && (at.y - place.y).abs() <= DOUBLE_CLICK_PIXELS
                        });
                        if double {
                            // The third press of a run starts a new click, as Qt's does.
                            self.egui.last_press = None;
                            self.mouse_double_click(cx, state, at, button, modifiers);
                        } else {
                            self.egui.last_press = Some((now, at, button));
                            self.mouse_press(cx, state, at, button, modifiers);
                        }
                    } else if let Some(held) =
                        self.egui.held.iter().position(|&held| held == button)
                    {
                        self.egui.held.remove(held);
                        self.mouse_release(cx, state, at, button, modifiers);
                    }
                }
                egui::Event::MouseWheel { unit, delta, .. } => {
                    if over && let Some(at) = ui.input(|input| input.pointer.latest_pos()) {
                        let notches = match unit {
                            egui::MouseWheelUnit::Point => {
                                f64::from(delta.y) / WHEEL_POINTS_A_NOTCH
                            }
                            _ => f64::from(delta.y),
                        };
                        self.wheel(cx, state, local(at), notches * 120.0);
                    }
                }
                egui::Event::PointerGone => {
                    self.leave();
                }
                egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } => {
                    if !response.has_focus() {
                        continue;
                    }
                    let modifiers = KeyModifiers::from_egui(modifiers);
                    // A key that types is taken as its Text event, which carries what it typed;
                    // only the keys the view tells apart are taken here. Space is both: taken
                    // here with its text, and its Text event passed over below.
                    let (key, text) = match key {
                        egui::Key::Escape => (ViewKey::Escape, ""),
                        egui::Key::Enter => (ViewKey::Enter, ""),
                        egui::Key::Backspace => (ViewKey::Backspace, ""),
                        egui::Key::Space => (ViewKey::Space, " "),
                        egui::Key::Delete => (ViewKey::Delete, ""),
                        egui::Key::Z if modifiers.is_undo() => (ViewKey::Z, ""),
                        _ => continue,
                    };
                    if pressed {
                        let _ = self.key_press(cx, state, key, modifiers, text);
                    } else {
                        self.key_release(key);
                    }
                }
                egui::Event::Text(text) => {
                    if response.has_focus() && text != " " {
                        let modifiers =
                            KeyModifiers::from_egui(ui.input(|input| input.modifiers));
                        let key = if text.eq_ignore_ascii_case("z") {
                            ViewKey::Z
                        } else {
                            ViewKey::Other
                        };
                        let _ = self.key_press(cx, state, key, modifiers, &text);
                    }
                }
                _ => {}
            }
        }
        // Ctrl as the frame reports it: egui has no event for a modifier alone, where Qt sends
        // the Control key's press and release.
        if response.has_focus() {
            let ctrl = ui.input(|input| input.modifiers.ctrl || input.modifiers.command);
            if ctrl != self.grips.ctrl() {
                self.grips.set_ctrl(ctrl);
            }
            // The view's own keys are not egui's to move the focus with.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: false,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        }
        if self.egui.inside && !over && self.egui.held.is_empty() {
            self.leave();
        }
        self.egui.inside = over;
        if over {
            ui.ctx().set_cursor_icon(match self.cursor() {
                ViewCursor::Cross => egui::CursorIcon::Crosshair,
                ViewCursor::ClosedHand => egui::CursorIcon::Grabbing,
            });
        }

        if ui.is_rect_visible(rect) {
            let picture = self.paint(cx, state).to_color_image();
            // One texture for the view's life, its pixels replaced each paint.
            let texture = match &mut self.egui.texture {
                Some(texture) => {
                    texture.set(picture, egui::TextureOptions::NEAREST);
                    texture.id()
                }
                None => {
                    let texture = ui.ctx().load_texture(
                        self.name.as_str(),
                        picture,
                        egui::TextureOptions::NEAREST,
                    );
                    let id = texture.id();
                    self.egui.texture = Some(texture);
                    id
                }
            };
            ui.painter().image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        controls::custom(
            ui,
            &response,
            ControlRecord::new(&self.name, ControlKind::View),
        );
        response
    }
}

/// What the plan painter draws from (plan_painter): the document with the window's reference
/// data and meshes.
fn paint_source<'a>(cx: &'a dyn PlanViewHost) -> PlanSource<'a> {
    let mut source = plan_source_of(cx.document());
    source.reference = cx.reference();
    let meshes = cx.meshes();
    source.meshes = (!meshes.is_empty()).then_some(meshes);
    source
}
