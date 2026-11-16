//! The sheet editor: a project's sheets on paper, laid out by hand or generated, and plotted
//! (docs/plotting.md, "The sheet editor"). The port of sheet_editor.
//!
//! A window of its own beside the drawing, as the style manager is, rather than a view kind:
//! sheets are paper, not a way of looking at the model, and a sheet set is edited while the
//! drawing views go on showing the drawing.
//!
//! ```text
//! the sheets    listed on the left with thumbnails: add, duplicate, remove, reorder (drag),
//!               rename
//! the canvas    the current sheet painted by the sheet painter - exactly what plots - with its
//!               viewports picked (one, several, a rubber band), dragged and resized on handles,
//!               snapped to the drawing area, each other and the paper grid, copied and pasted;
//!               rulers in paper millimetres, and the paper and world position under the cursor;
//!               Shift-drag pans the drawing inside a plan viewport
//! properties    on the right: the selected viewport's, else the sheet's, with the title-block
//!               values this sheet overrides
//! the toolbar   generate sheets, add a view, tile with a preset, the title block and logo, plot
//!               the sheet or the whole set
//! the checks    below: what a plot would get wrong, checked again a moment after each change
//!               (plotting::sheet_checks)
//! ```
//!
//! Every change is ONE undoable step through cad's sheet_commands, a drag committed on release,
//! so the command stack never fills with the positions a drag passed through. The editor follows
//! the document and redraws on every change, undo and redo included.
//!
//! THE RUST SHAPE (the plan view's, docs/rust.md "Step 7", wave 2):
//!
//! * **Every call is lent the window** (`&mut dyn Host`): the drawing, its executor, its log and
//!   its clipboard. The editor owns no document. It follows the document at its next look
//!   (`observe`, called at the head of every method and frame): where the C++ listener rebuilt the
//!   list, the canvas and the properties inside the change, the Rust editor does so when it next
//!   sees the document's model revision moved.
//! * **The canvas's events are methods** - `mouse_press`, `mouse_move`, `mouse_release`,
//!   `mouse_double_click`, `wheel`, `key_press`, `key_release`, `leave` - in the canvas's own
//!   pixels, called on the editor as a Qt test sends an event to the canvas; `ui` is the adaptor
//!   from egui. The canvas keeps the set it shows as the document's `Arc<SheetSet>`, so what it
//!   asks between events (paper_to_widget, selected_ids) needs no document.
//! * **What the C++ said through `onMessage`** goes to the host's log (`Host::log`): the window's
//!   onMessage logged it, and a test's host keeps it. The line a dialog builds is run by the
//!   host's executor once the editor has been told there is one (`set_command_runner`), else by
//!   the interpreter's own sheet verbs on the host's drawing with "> line" and the reply logged,
//!   as the C++ did with no runner.
//! * **The edits the C++ made in place** - a drag, the toolbar's Add View and Tile, the panel's
//!   own fields - are the same cad functions on `Host::document_mut`, each one undoable step; the
//!   lines the C++ ran (VIEW SET, GENERATE, SHEETS ...) are run as lines. Rejected: turning every
//!   edit into a verb line, which would change what the log and the undo names say.
//! * **A field that the C++ left without an object name** is given one here, prefixed `sheet`,
//!   since every named control needs one (listed in the report of the port).
//! * **What a Qt dialog's exec waited for** opens without waiting (Generate Sheets, the title
//!   block, the hidden layers): a person answers it at a later frame, and a test fills it by its
//!   object names.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(a > b) is kept: it rejects NaN, which a > b alone would let through."
)]
#![allow(
    clippy::float_cmp,
    reason = "Values are compared exactly where the C++ compared them exactly: a box left as it was."
)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use egui::{Key, KeyboardShortcut, Modifiers, Ui};
use katana_cad::document::Document;
use katana_cad::plot::{PaperSize, SHEET_SCALES, paper_dimensions};
use katana_cad::plotting::frame::BUILT_IN_FRAME_ID;
use katana_cad::plotting::generators::{SheetTemplate, blank_sheet};
use katana_cad::plotting::layout::{
    TILING_GUTTER_MM, TILING_PRESET_COUNT, TilingPreset, preset_name, tile_viewports, tiling_area,
};
use katana_cad::plotting::plan_grid::{automatic_grid_interval, set_plan_grid};
use katana_cad::plotting::legend::set_legend_scope;
use katana_cad::plotting::preflight::{findings_on_sheets, summarize};
use katana_cad::plotting::section_fit::SectionFit;
use katana_cad::plotting::sheet_commands::{
    add_sheet, edit_sheet, edit_sheet_set, edit_viewport, import_logo, logo_path,
};
use katana_cad::plotting::sheet_set::{
    GridStyle, LegendScope, Revision, Sheet, SheetSet, SignOff, Viewport, ViewportKind,
    automatic_title, drawing_area, format_sheet_number, next_sheet_id, next_viewport_id,
    resolve_fields, scale_text,
};
use katana_cad::plotting::sheet_verbs::{SheetVerbContext, run_sheet_verb};
use katana_cad::plotting::viewport_edits::{
    BandMode, PAPER_GRID_MM, move_viewports, remove_viewports, snap_edge, snap_moving_rect,
    viewports_in_band,
};
use katana_cad::CommandInterpreter;
use katana_core::error::make_error;
use katana_core::text::parse_finite_double;
use katana_core::{ErrorCode, Status};
use katana_entity::EnumName;
use katana_geometry::alignment::solve_alignment;
use katana_geometry::{Box2, Point2};
use katana_math::numerics::{DEG_TO_RAD, RAD_TO_DEG};

use crate::canvas::{
    Brush, Canvas, Color, Image, IntRect, Pen, PenCap, PenJoin, PenStyle, PointF,
    RasterCanvas, RectF, RenderHint, SizeF, TextFlags,
};
use crate::drive::action::{
    Action, Menu, MenuItem, ToolBar, ToolBarItem, action_item, menu_items_ui, shortcut_text,
};
use crate::drive::controls::{self, Choice, LineOptions, SpinRange};
use crate::drive::mailbox::{Act, no_button, no_field, whole_out_of_range};
use crate::drive::registry::{
    ControlKind, ControlRecord, TargetKind, TargetSpec, begin_target, end_target, settle, take,
};
use crate::format::qt_fixed;
use crate::host::Host;
use crate::ifc_dialogs::{get_open_file_name, get_save_file_name};
use crate::plan_painter::{plan_drawn_bounds, plan_draws_in};
use crate::plotting::grid_properties::GridRows;
use crate::plotting::legend_properties::add_legend_properties;
use crate::plotting::plot_dialog::{PlotDialog, plot_interactively, suggested_plot_file};
use crate::plotting::plot_output::{PlotFormat, plot_request_for, plot_sheets_from};
use crate::plotting::sheet_arrange::{
    ArrangeDialogs, choose_paper_button, viewport_content,
};
use crate::plotting::sheet_checks::{SheetChecksDock, check_sheets_for};
use crate::plotting::sheet_list_widget::{SheetListRow, SheetListWidget};
use crate::plotting::sheet_readout::{SheetCursorReadout, readout_text};
use crate::plotting::sheet_rulers::{
    RulerView, paint_paper_grid, paint_sheet_rulers,
};
use crate::plotting::sheet_tables::resolved_sheet_set;
use crate::plotting::sheet_thumbnails::SheetThumbnails;
use crate::sheet_painter::{
    ResolvedViewport, SheetPaintCache, SheetPaintOptions, SheetPaintStats, SheetSource,
    paint_sheet, resolve_plan_viewport_on_sheet, resolve_section_viewport, sheet_frame,
};
use crate::shell::dialog;
use crate::survey::survey_dialogs::offstage;
use crate::viewport_widget::{KeyModifiers, MouseButton};

// ---- constants ---------------------------------------------------------------------------------

/// Logical pixels a millimetre, at the least and the most.
/// The properties dock's width (the look of 2026-11-16; it was 300): the side panels' width.
const PROPERTIES_WIDTH: f32 = 260.0;
/// The sheet list's width: a sheet's thumbnail and its number, name and paper beside it.
const SHEET_LIST_WIDTH: f32 = 220.0;
const MINIMUM_ZOOM: f64 = 0.3;
const MAXIMUM_ZOOM: f64 = 60.0;
const HANDLE_PIXELS: f64 = 7.0;
const SNAP_PIXELS: f64 = 8.0;
const MINIMUM_VIEWPORT_MM: f64 = 10.0;
/// How far a press must go before it is a drag, not a click: a hand's tremor on a click must not
/// move a viewport.
const DRAG_START_PIXELS: f64 = 3.0;

const DESK: Color = Color::rgb(88, 91, 99);
const SELECTION: Color = Color::rgb(0, 120, 215);
const GUIDE: Color = Color::rgb(230, 60, 160);
const CROSSING: Color = Color::rgb(0, 150, 70);

/// The size the canvas has before a frame lays it out: what an editor shown at 1400 x 900 leaves
/// it beside its list, properties and checks, near enough. Tests ask positions of paper points
/// (paper_to_widget), never pixels of this size.
const DEFAULT_CANVAS_SIZE: (f64, f64) = (860.0, 600.0);

pub(crate) const KINDS: [ViewportKind; 10] = [
    ViewportKind::Plan,
    ViewportKind::LongSection,
    ViewportKind::CrossSections,
    ViewportKind::Model3D,
    ViewportKind::KeyPlan,
    ViewportKind::Legend,
    ViewportKind::Notes,
    ViewportKind::Image,
    ViewportKind::SheetIndex,
    ViewportKind::Revisions,
];

pub(crate) fn kind_name(kind: ViewportKind) -> &'static str {
    match kind {
        ViewportKind::Plan => "Plan",
        ViewportKind::LongSection => "Long section",
        ViewportKind::CrossSections => "Cross sections",
        ViewportKind::Model3D => "3D snapshot",
        ViewportKind::Legend => "Legend",
        ViewportKind::Notes => "Notes",
        ViewportKind::Image => "Image",
        ViewportKind::KeyPlan => "Key plan",
        ViewportKind::SheetIndex => "Drawing register",
        ViewportKind::Revisions => "Revision table",
    }
}

pub(crate) const PAPERS: [PaperSize; 5] = [
    PaperSize::A0,
    PaperSize::A1,
    PaperSize::A2,
    PaperSize::A3,
    PaperSize::A4,
];

pub(crate) fn paper_name(paper: PaperSize) -> &'static str {
    match paper {
        PaperSize::A0 => "A0",
        PaperSize::A1 => "A1",
        PaperSize::A2 => "A2",
        PaperSize::A3 => "A3",
        PaperSize::A4 => "A4",
    }
}

const PRESETS: [TilingPreset; TILING_PRESET_COUNT] = [
    TilingPreset::Full,
    TilingPreset::Columns2,
    TilingPreset::Rows2,
    TilingPreset::Quad,
    TilingPreset::MainRight,
    TilingPreset::MainBelow,
    TilingPreset::MainCorner,
    TilingPreset::MainTwoRight,
];

pub(crate) fn scale_label(scale: f64) -> String {
    format!(
        "1:{}",
        qt_fixed(scale, if scale == scale.floor() { 0 } else { 2 })
    )
}

/// "1:500", "500" or "auto" (nothing).
pub(crate) fn parse_scale(text: &str) -> Option<f64> {
    let mut t = text.trim();
    if let Some(rest) = t.strip_prefix("1:") {
        t = rest;
    }
    // QString::toDouble passes over whitespace round the number.
    let value = parse_finite_double(t.trim())?;
    (value > 0.0).then_some(value)
}

/// The scales a scale box lists: "Auto" first when it has one, then every standard scale.
pub(crate) fn scale_items(with_auto: bool) -> Vec<String> {
    let mut items = Vec::new();
    if with_auto {
        items.push(String::from("Auto"));
    }
    items.extend(SHEET_SCALES.iter().map(|&scale| scale_label(scale)));
    items
}

fn rotated(v: Point2, radians: f64) -> Point2 {
    let c = radians.cos();
    let s = radians.sin();
    Point2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Where a rectangle of paper falls free on the sheet: the tiling area when the sheet has nothing
/// on it, else a panel of the size given in its middle.
fn free_place(sheet: &Sheet, width: f64, height: f64) -> Box2 {
    let area = tiling_area(sheet);
    if sheet.viewports.is_empty() {
        return area;
    }
    let width = width.min(area.width());
    let height = height.min(area.height());
    let c = area.center();
    Box2::new(
        Point2::new(c.x - width / 2.0, c.y - height / 2.0),
        Point2::new(c.x + width / 2.0, c.y + height / 2.0),
    )
}

/// A number as a verb line takes it: to a millionth, in its shortest form, as the replies write
/// numbers - "24" and "24.5", never "24.500000".
pub(crate) fn verb_number(value: f64) -> String {
    let mut text = qt_fixed((value * 1e6).round() / 1e6, 6);
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text == "-0" { String::from("0") } else { text }
}

/// A value in double quotes, as a verb line groups words: a name or a path may hold spaces. The
/// command line has no way to type a double quote in a value, so the dialogs never offer one.
pub(crate) fn quoted(text: &str) -> String {
    format!("\"{text}\"")
}

/// "20, 40 60;80" as stations= takes it: "20,40,60,80". None when a part is not a number, which
/// the caller says rather than run.
pub(crate) fn station_list(typed: &str) -> Option<String> {
    let mut numbers = Vec::new();
    for part in typed
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|part| !part.is_empty())
    {
        numbers.push(verb_number(parse_finite_double(part)?));
    }
    Some(numbers.join(","))
}

/// The first line of a reply, for the status bar.
fn first_line(text: &str) -> &str {
    text.split('\n').next().unwrap_or_default()
}

fn pen(color: Color, width: f64, style: PenStyle) -> Pen {
    Pen::with(color, width, style, PenCap::Square, PenJoin::Bevel)
}

// ---- what the sheets are drawn from -----------------------------------------------------------

/// What the sheets are drawn from, asked for at every paint: the drawing, the window's surfaces
/// and reference data, the logo, the fields. Handed the drawing it is drawn from, as the verbs
/// hand their closures the drawing they hold (the C++'s provider kept a reference to it).
pub type SourceProvider = Rc<dyn for<'d> Fn(&'d Document) -> SheetSource<'d>>;

/// What the sheet verbs are told that the document does not hold, from what the sheets are drawn
/// from (`source`): the extent GENERATE covers and what each tile draws, SHEETS CHECK with the
/// painter's knowledge, what a view shows, an automatic section's fit and the set as drawn
/// (cad's SheetVerbContext). The window's command line and the editor's own lines are given this
/// one, so a line means the same typed or built by a dialog.
pub fn sheet_verb_context_for(document: &Document, source: &SourceProvider) -> SheetVerbContext<'static> {
    let drawn = source(document);
    // GENERATE lays out what Generate Sheets always covered: everything the plan view draws. (The
    // sections' surfaces are the drawing's own shown ones, which the verb reads from the
    // Document.)
    let drawing_extent = plan_drawn_bounds(&drawn.plan, &Default::default(), &BTreeSet::new());
    let draws_in = Rc::clone(source);
    let check = Rc::clone(source);
    let content = Rc::clone(source);
    let fit = Rc::clone(source);
    let drawn_set = Rc::clone(source);
    SheetVerbContext {
        drawing_extent: Some(drawing_extent),
        // And each tile it cuts is kept only where that view draws something.
        draws_in: Some(Box::new(move |document: &Document, area: &Box2| {
            plan_draws_in(&draws_in(document).plan, &Default::default(), &BTreeSet::new(), area)
        })),
        // The rest ask for the source again when they are called: a verb may change the set
        // between being given the context and using it.
        check: Some(Box::new(move |document: &Document, sheets: &[usize]| {
            check_sheets_for(&document.sheet_set(), &check(document), sheets)
        })),
        content: Some(Box::new(move |document: &Document, viewport: &Viewport| {
            viewport_content(viewport, &content(document), &document.sheet_set())
        })),
        fit_section: Some(Box::new(move |document: &Document, viewport: &Viewport| {
            let mut cache = SheetPaintCache::new();
            resolve_section_viewport(viewport, &fit(document), &mut cache)
        })),
        drawn: Some(Box::new(move |document: &Document, set: &SheetSet| {
            resolved_sheet_set(set, &drawn_set(document), None)
        })),
    }
}

// ---- the canvas's input ------------------------------------------------------------------------

/// The keys the canvas tells apart (Qt::Key); every other key is `Other`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SheetKey {
    Space,
    Escape,
    Delete,
    Backspace,
    Left,
    Right,
    Up,
    Down,
    Tab,
    /// Shift+Tab, as Qt names it.
    Backtab,
    PageUp,
    PageDown,
    Other,
}

/// What a key on the canvas asks of the editor, beyond what the canvas does itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanvasAsks {
    /// Nothing: the canvas took the key, or had no use for it.
    Nothing,
    /// The canvas had no use for the key (QWidget::keyPressEvent's default).
    NotTaken,
    /// Delete: every selected viewport removed.
    Remove,
    /// The arrows: every selected viewport moved by so many millimetres.
    Nudge(Point2),
    /// PgUp and PgDn: the actions whose keys the canvas takes (sheetPreviousSheet,
    /// sheetNextSheet).
    PreviousSheet,
    NextSheet,
}

/// The pointer's shape over the canvas: what a press would do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CanvasCursor {
    #[default]
    Arrow,
    OpenHand,
    ClosedHand,
    SizeAll,
    /// A handle's: 0..7 clockwise from the top-left corner.
    Handle(usize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Drag {
    #[default]
    None,
    Move,
    Resize,
    Pan,
    PanView,
    Band,
}

// ============================================================================
// SheetCanvas
// ============================================================================

/// The paper: one sheet, painted and edited.
pub struct SheetCanvas {
    pub(crate) source: SourceProvider,
    /// The set shown, as the document held it when the editor last looked.
    pub(crate) set: Arc<SheetSet>,
    pub(crate) cache: SheetPaintCache,
    pub(crate) stats: SheetPaintStats,
    pub(crate) sheet: usize,
    pub(crate) selected: String,

    pub(crate) zoom: f64,
    pub(crate) origin: PointF,
    pub(crate) fitted: bool,
    pub(crate) width: f64,
    pub(crate) height: f64,
    /// Device pixels a logical pixel.
    pub(crate) ratio: f64,

    // The painted sheet, and what it was painted at.
    pub(crate) image: Image,
    pub(crate) image_origin: PointF,
    pub(crate) image_zoom: f64,
    pub(crate) version: u64,
    pub(crate) image_version: u64,
    pub(crate) renders: i32,

    pub(crate) drag: Drag,
    pub(crate) handle: usize,
    pub(crate) press_widget: PointF,
    pub(crate) press_paper: Point2,
    pub(crate) start_rect: Box2,
    pub(crate) live_rect: Box2,
    pub(crate) start_centre: Point2,
    /// Paper millimetres the drawing was dragged by.
    pub(crate) view_shift: Point2,
    pub(crate) guide_x: Option<f64>,
    pub(crate) guide_y: Option<f64>,

    /// Every selected id, the primary (`selected`) among them; in the order they were selected.
    pub(crate) selection: Vec<String>,
    /// A group move: each moving viewport's rectangle at the press, their bounds, and how far
    /// they have been dragged (snapped).
    pub(crate) start_rects: BTreeMap<String, Box2>,
    pub(crate) start_bounds: Box2,
    pub(crate) live_delta: Point2,
    /// A press has become a drag once the cursor has gone a few pixels.
    pub(crate) press_moved: bool,
    /// A click on one of several selected viewports keeps the group for a drag, and selects it
    /// alone when it is let go without one.
    pub(crate) press_hit: String,
    pub(crate) narrow_on_click: bool,
    /// The plan Shift-drag pans; a Shift-click without a drag toggles it.
    pub(crate) pan_view_id: String,
    /// The rubber band's far corner (press_paper is the other), and whether it adds to the
    /// selection rather than replacing it.
    pub(crate) band_corner: Point2,
    pub(crate) band_adds: bool,
    pub(crate) space_held: bool,
    pub(crate) snap_grid: bool,
    pub(crate) grid_mm: f64,
    pub(crate) rulers: bool,
    pub(crate) readout: SheetCursorReadout,
    pub(crate) cursor_paper: Option<Point2>,
    pub(crate) resolved: RefCell<BTreeMap<String, ResolvedViewport>>,
    pub(crate) resolved_version: RefCell<u64>,
    pub(crate) cursor: CanvasCursor,
    /// Counts every change of the selection: the properties are drawn again from it
    /// (onSelectionChanged).
    pub(crate) selection_serial: u64,
}

impl SheetCanvas {
    pub fn new(source: SourceProvider) -> Self {
        Self {
            source,
            set: Arc::new(SheetSet::default()),
            cache: SheetPaintCache::new(),
            stats: SheetPaintStats::default(),
            sheet: 0,
            selected: String::new(),
            zoom: 2.0,
            origin: PointF::new(20.0, 20.0),
            fitted: false,
            width: DEFAULT_CANVAS_SIZE.0,
            height: DEFAULT_CANVAS_SIZE.1,
            ratio: 1.0,
            image: Image::default(),
            image_origin: PointF::new(0.0, 0.0),
            image_zoom: 0.0,
            version: 1,
            image_version: 0,
            renders: 0,
            drag: Drag::None,
            handle: 0,
            press_widget: PointF::new(0.0, 0.0),
            press_paper: Point2::default(),
            start_rect: Box2::default(),
            live_rect: Box2::default(),
            start_centre: Point2::default(),
            view_shift: Point2::default(),
            guide_x: None,
            guide_y: None,
            selection: Vec::new(),
            start_rects: BTreeMap::new(),
            start_bounds: Box2::default(),
            live_delta: Point2::default(),
            press_moved: false,
            press_hit: String::new(),
            narrow_on_click: false,
            pan_view_id: String::new(),
            band_corner: Point2::default(),
            band_adds: false,
            space_held: false,
            snap_grid: false,
            grid_mm: PAPER_GRID_MM,
            rulers: true,
            readout: SheetCursorReadout::default(),
            cursor_paper: None,
            resolved: RefCell::new(BTreeMap::new()),
            resolved_version: RefCell::new(0),
            cursor: CanvasCursor::Arrow,
            selection_serial: 0,
        }
    }

    pub fn sheet(&self) -> usize {
        self.sheet
    }

    /// The selected viewport's id; empty for none (the sheet itself).
    pub fn selected(&self) -> &str {
        &self.selected
    }

    /// What the canvas has cut and drawn, per revision: the properties read an automatic
    /// section's scale through it, so nothing is cut twice.
    pub fn paint_cache(&mut self) -> &mut SheetPaintCache {
        &mut self.cache
    }

    /// Logical pixels per paper millimetre.
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    pub fn width(&self) -> f64 {
        self.width
    }

    pub fn height(&self) -> f64 {
        self.height
    }

    /// What the last paint reported: viewports it could not draw.
    pub fn last_stats(&self) -> &SheetPaintStats {
        &self.stats
    }

    /// How many times the sheet has been painted, for tests of the cache.
    pub fn renders(&self) -> i32 {
        self.renders
    }

    pub fn snaps_to_grid(&self) -> bool {
        self.snap_grid
    }

    pub fn grid_spacing(&self) -> f64 {
        self.grid_mm
    }

    pub fn rulers_shown(&self) -> bool {
        self.rulers
    }

    /// The readout at the cursor's last position over the canvas.
    pub fn cursor_readout(&self) -> &SheetCursorReadout {
        &self.readout
    }

    /// The pointer's shape over the canvas now.
    pub fn cursor(&self) -> CanvasCursor {
        self.cursor
    }

    /// The canvas laid out at `width` x `height` logical pixels (resizeEvent): the page is fitted
    /// again at the next paint, unless a drag is under way on a fitted page.
    pub fn resize(&mut self, width: f64, height: f64) {
        self.width = width;
        self.height = height;
        if !self.fitted || self.drag == Drag::None {
            self.fitted = false;
        }
    }

    pub fn set_device_ratio(&mut self, ratio: f64) {
        self.ratio = if ratio > 0.0 { ratio } else { 1.0 };
    }

    pub(crate) fn current_sheet(&self) -> Option<&Sheet> {
        self.set.sheets.get(self.sheet)
    }

    pub(crate) fn paper_height(&self) -> f64 {
        self.current_sheet().map_or(297.0, |sheet| {
            paper_dimensions(sheet.paper, sheet.landscape).height_mm
        })
    }

    pub(crate) fn viewport(&self, id: &str) -> Option<&Viewport> {
        self.current_sheet()?
            .viewports
            .iter()
            .find(|viewport| viewport.id == id)
    }

    /// The set shown, after the editor has looked at the document again.
    pub(crate) fn set_shown(&mut self, set: Arc<SheetSet>) {
        self.set = set;
    }

    pub fn set_sheet(&mut self, index: usize) {
        if index != self.sheet {
            self.sheet = index;
            self.selected.clear();
            self.selection.clear();
            self.fitted = false;
            self.invalidate();
            self.selection_changed();
        }
    }

    pub fn select(&mut self, viewport_id: &str) {
        let mut id = String::from(viewport_id);
        if !id.is_empty() && self.viewport(&id).is_none() {
            id.clear();
        }
        let selection = if id.is_empty() {
            Vec::new()
        } else {
            vec![id.clone()]
        };
        if id == self.selected && selection == self.selection {
            return;
        }
        self.selected = id;
        self.selection = selection;
        self.selection_changed();
    }

    /// Paint again from the document: after any change to it.
    pub fn invalidate(&mut self) {
        self.version += 1;
        self.prune_selection();
    }

    /// The whole paper in the window.
    pub fn fit_page(&mut self) {
        let paper = match self.current_sheet() {
            Some(sheet) => paper_dimensions(sheet.paper, sheet.landscape),
            None => paper_dimensions(PaperSize::A3, true),
        };
        let margin = 24.0;
        // The paper fitted to what the rulers leave.
        let inset = self.ruler_pixels();
        let w = self.width - inset;
        let h = self.height - inset;
        self.zoom = ((w - 2.0 * margin) / paper.width_mm)
            .min((h - 2.0 * margin) / paper.height_mm)
            .clamp(MINIMUM_ZOOM, MAXIMUM_ZOOM);
        self.origin = PointF::new(
            inset + (w - paper.width_mm * self.zoom) / 2.0,
            inset + (h - paper.height_mm * self.zoom) / 2.0,
        );
        self.fitted = true;
    }

    /// Paper `paper` filling the window with a margin; the page when empty.
    pub fn zoom_to(&mut self, paper: &Box2) {
        if paper.empty() || self.current_sheet().is_none() {
            self.fit_page();
            return;
        }
        let margin = 24.0;
        let inset = self.ruler_pixels();
        let w = self.width - inset;
        let h = self.height - inset;
        self.zoom = ((w - 2.0 * margin) / paper.width().max(1.0))
            .min((h - 2.0 * margin) / paper.height().max(1.0))
            .clamp(MINIMUM_ZOOM, MAXIMUM_ZOOM);
        // The box's centre in the middle of what the rulers leave.
        let c = paper.center();
        self.origin = PointF::new(
            inset + w / 2.0 - c.x * self.zoom,
            inset + h / 2.0 - (self.paper_height() - c.y) * self.zoom,
        );
        self.fitted = true;
    }

    pub fn paper_to_widget(&self, paper: Point2) -> PointF {
        self.origin
            + PointF::new(
                paper.x * self.zoom,
                (self.paper_height() - paper.y) * self.zoom,
            )
    }

    pub fn widget_to_paper(&self, widget: PointF) -> Point2 {
        let d = widget - self.origin;
        Point2::new(d.x / self.zoom, self.paper_height() - d.y / self.zoom)
    }

    pub(crate) fn widget_rect(&self, paper: &Box2) -> RectF {
        RectF::from_corners(
            self.paper_to_widget(Point2::new(paper.min.x, paper.max.y)),
            self.paper_to_widget(Point2::new(paper.max.x, paper.min.y)),
        )
    }

    pub(crate) fn viewport_at(&self, widget: PointF) -> String {
        let Some(sheet) = self.current_sheet() else {
            return String::new();
        };
        let p = self.widget_to_paper(widget);
        // Front to back: the one drawn last is the one on top.
        sheet
            .viewports
            .iter()
            .rev()
            .find(|viewport| !viewport.rect.empty() && viewport.rect.contains(p))
            .map(|viewport| viewport.id.clone())
            .unwrap_or_default()
    }

    fn handle_points(r: &RectF) -> [PointF; 8] {
        let c = r.center();
        [
            r.top_left(),
            PointF::new(c.x, r.top()),
            r.top_right(),
            PointF::new(r.right(), c.y),
            r.bottom_right(),
            PointF::new(c.x, r.bottom()),
            r.bottom_left(),
            PointF::new(r.left(), c.y),
        ]
    }

    /// 0..7 clockwise from the top-left corner; nothing when not on a handle of the selected
    /// viewport.
    pub(crate) fn handle_at(&self, widget: PointF) -> Option<usize> {
        // Handles only on a viewport selected alone: a group is moved, not sized.
        let v = self.viewport(&self.selected)?;
        if v.locked || self.selection.len() != 1 {
            return None;
        }
        let r = self.widget_rect(&v.rect);
        Self::handle_points(&r).iter().position(|handle| {
            (widget.x - handle.x).abs() <= HANDLE_PIXELS
                && (widget.y - handle.y).abs() <= HANDLE_PIXELS
        })
    }

    // ---- painting --------------------------------------------------------------------------

    fn render(&mut self, document: &Document) {
        let ratio = self.ratio;
        let width = ((self.width * ratio) as i64).max(1) as u32;
        let height = ((self.height * ratio) as i64).max(1) as u32;
        let mut image = Image::new(width, height);
        image.fill(DESK);
        self.image_origin = self.origin;
        self.image_zoom = self.zoom;
        self.image_version = self.version;
        let Some(sheet) = self.set.sheets.get(self.sheet) else {
            self.stats = SheetPaintStats::default();
            image.set_device_pixel_ratio(ratio);
            self.image = image;
            return;
        };
        self.renders += 1;
        let paper = paper_dimensions(sheet.paper, sheet.landscape);
        let options = SheetPaintOptions {
            pixels_per_millimetre: self.zoom * ratio,
            origin: PointF::new(self.origin.x * ratio, self.origin.y * ratio),
            construction: true,
            slot_hints: true,
            raster_dpi_cap: 110.0,
            ..SheetPaintOptions::default()
        };
        let source = (self.source)(document);
        let (origin, zoom) = (self.origin, self.zoom);
        let set = &self.set;
        let index = self.sheet;
        let cache = &mut self.cache;
        let mut stats = SheetPaintStats::default();
        RasterCanvas::paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
            // The paper's shadow on the desk.
            canvas.save();
            canvas.scale(ratio, ratio);
            canvas.fill_rect(
                RectF::new(
                    origin.x + 4.0,
                    origin.y + 4.0,
                    paper.width_mm * zoom,
                    paper.height_mm * zoom,
                ),
                &Brush::solid(Color::rgba(0, 0, 0, 70)),
            );
            canvas.restore();
            stats = paint_sheet(canvas, set, index, &source, &options, cache);
        });
        image.set_device_pixel_ratio(ratio);
        self.stats = stats;
        self.image = image;
    }

    /// The canvas as it shows now (paintEvent): the sheet, painted again only when it or the view
    /// has changed since the last paint, and over it the drawing area, the selection, the
    /// handles, a drag's guides and plate, the rubber band and the rulers. In device pixels.
    pub fn paint(&mut self, document: &Document) -> Image {
        if !self.fitted && self.current_sheet().is_some() {
            self.fit_page();
        }
        let ratio = self.ratio;
        let width = ((self.width * ratio) as i64).max(1) as u32;
        let height = ((self.height * ratio) as i64).max(1) as u32;
        let panning = self.drag == Drag::Pan;
        if !panning
            && (self.image_version != self.version
                || self.image_zoom != self.zoom
                || self.image_origin != self.origin
                || self.image.width() != width
                || self.image.height() != height)
        {
            self.render(document);
        }
        let mut frame = Image::new(width, height);
        let this = &*self;
        RasterCanvas::paint_image(&mut frame, &mut |canvas: &mut dyn Canvas| {
            canvas.scale(ratio, ratio);
            this.paint_over(canvas);
        });
        frame.set_device_pixel_ratio(ratio);
        frame
    }

    /// The last paint laid down with its top-left at `at`, in logical pixels.
    fn lay_image(&self, canvas: &mut dyn Canvas, at: PointF) {
        let ratio = self.ratio;
        canvas.draw_image(
            RectF::new(
                at.x,
                at.y,
                f64::from(self.image.width()) / ratio,
                f64::from(self.image.height()) / ratio,
            ),
            &self.image,
            IntRect::new(
                0,
                0,
                self.image.width() as i32,
                self.image.height() as i32,
            ),
        );
    }

    fn paint_over(&self, canvas: &mut dyn Canvas) {
        let rect = RectF::new(0.0, 0.0, self.width, self.height);
        canvas.fill_rect(rect, &Brush::solid(DESK));
        // While panning, the last paint moves with the hand and is painted afresh when it lets
        // go.
        self.lay_image(canvas, self.origin - self.image_origin);

        let Some(sheet) = self.current_sheet() else {
            canvas.set_pen(Pen::new(Color::WHITE));
            let _ = canvas.draw_text_in(
                rect,
                TextFlags::CENTER,
                "No sheets yet.\nUse Generate Sheets or New Sheet.",
            );
            return;
        };
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        let paper = paper_dimensions(sheet.paper, sheet.landscape);
        let mut ruler = RulerView {
            pixels_per_millimetre: self.zoom,
            paper_origin: self.origin,
            paper_width_mm: paper.width_mm,
            paper_height_mm: paper.height_mm,
            ..RulerView::default()
        };

        // The drawing dragged inside its viewport: the paint shifted, clipped to it.
        if self.drag == Drag::PanView
            && self.press_moved
            && let Some(v) = self.viewport(&self.pan_view_id)
        {
            let r = self.widget_rect(&v.rect);
            canvas.save();
            canvas.set_clip_rect(r, true);
            canvas.fill_rect(r, &Brush::solid(Color::WHITE));
            let shift = self.origin - self.image_origin
                + PointF::new(self.view_shift.x * self.zoom, -self.view_shift.y * self.zoom);
            self.lay_image(canvas, shift);
            canvas.restore();
        }

        // The paper grid, faint, while a drag snaps to it.
        if self.snap_grid {
            paint_paper_grid(canvas, &ruler, self.grid_mm);
            canvas.set_render_hint(RenderHint::Antialiasing, true);
        }

        // The drawing area, faint, so an empty sheet shows where views go.
        canvas.set_pen(pen(Color::rgba(0, 120, 215, 60), 1.0, PenStyle::Dash));
        canvas.set_brush(Brush::none());
        canvas.draw_rect(self.widget_rect(&drawing_area(sheet)));

        // What is being moved or sized, its own paint going with it.
        self.paint_dragged_content(canvas);

        // Every selected viewport outlined where it is now; a group's bounds.
        let mut bounds = Box2::default();
        for id in &self.selection {
            let b = self.live_rect_of(id);
            bounds.expand_box(b);
            if *id != self.selected && !b.empty() {
                canvas.set_pen(pen(SELECTION, 1.5, PenStyle::Solid));
                canvas.draw_rect(self.widget_rect(&b));
            }
        }
        ruler.highlight = bounds;
        if self.selection.len() > 1 && !bounds.empty() {
            canvas.set_pen(pen(SELECTION, 1.0, PenStyle::Dash));
            canvas.draw_rect(self.widget_rect(&bounds).adjusted(-4.0, -4.0, 4.0, 4.0));
        }

        // The primary, its handles, and the rectangle being dragged.
        if let Some(v) = self.viewport(&self.selected) {
            let dragging = self.press_moved
                && ((self.drag == Drag::Move && !self.start_rects.is_empty())
                    || self.drag == Drag::Resize);
            let b = if dragging && self.selection.len() > 1 {
                bounds
            } else {
                self.live_rect_of(&self.selected)
            };
            let r = self.widget_rect(&self.live_rect_of(&self.selected));
            canvas.set_pen(pen(SELECTION, 2.0, PenStyle::Solid));
            canvas.draw_rect(r);
            if !v.locked && self.selection.len() == 1 {
                canvas.set_brush(Brush::solid(Color::WHITE));
                canvas.set_pen(pen(SELECTION, 1.2, PenStyle::Solid));
                for at in Self::handle_points(&r) {
                    canvas.draw_rect(RectF::new(at.x - 4.0, at.y - 4.0, 8.0, 8.0));
                }
                canvas.set_brush(Brush::none());
            }
            if dragging {
                canvas.set_pen(pen(GUIDE, 1.0, PenStyle::Dash));
                if let Some(guide) = self.guide_x {
                    let x = self.paper_to_widget(Point2::new(guide, 0.0)).x;
                    canvas.draw_line(PointF::new(x, 0.0), PointF::new(x, self.height));
                }
                if let Some(guide) = self.guide_y {
                    let y = self.paper_to_widget(Point2::new(0.0, guide)).y;
                    canvas.draw_line(PointF::new(0.0, y), PointF::new(self.width, y));
                }
                // Where it is and how big, as the status line would say it.
                let around = self.widget_rect(&b);
                let label = format!(
                    "{}, {}   {} x {} mm",
                    qt_fixed(b.min.x, 1),
                    qt_fixed(b.min.y, 1),
                    qt_fixed(b.width(), 1),
                    qt_fixed(b.height(), 1)
                );
                let at = around.bottom_left() + PointF::new(4.0, 24.0);
                let font = canvas.font();
                let plate = canvas
                    .metrics()
                    .bounding_rect(&font, &label)
                    .translated(at.x, at.y)
                    .adjusted(-4.0, -2.0, 4.0, 2.0);
                canvas.fill_rect(plate, &Brush::solid(Color::rgba(30, 32, 38, 200)));
                canvas.set_pen(Pen::new(Color::WHITE));
                canvas.draw_text(at, &label);
            }
        }

        // The rubber band: a window solid and blue, a crossing dashed and green, as CAD programs
        // draw them.
        if self.drag == Drag::Band && self.press_moved {
            let crossing = self.band_corner.x < self.press_paper.x;
            let ink = if crossing { CROSSING } else { SELECTION };
            let band = self.band();
            canvas.set_pen(pen(
                ink,
                1.0,
                if crossing {
                    PenStyle::Dash
                } else {
                    PenStyle::Solid
                },
            ));
            canvas.set_brush(Brush::solid(ink.with_alpha(28)));
            canvas.draw_rect(self.widget_rect(&band));
            canvas.set_brush(Brush::none());
        }

        if self.rulers {
            ruler.cursor = self.cursor_paper;
            paint_sheet_rulers(canvas, SizeF::new(self.width, self.height), &ruler);
        }
    }

    /// The rubber band, corner to corner.
    pub(crate) fn band(&self) -> Box2 {
        Box2::new(
            Point2::new(
                self.press_paper.x.min(self.band_corner.x),
                self.press_paper.y.min(self.band_corner.y),
            ),
            Point2::new(
                self.press_paper.x.max(self.band_corner.x),
                self.press_paper.y.max(self.band_corner.y),
            ),
        )
    }

    // ---- the mouse -------------------------------------------------------------------------

    pub fn mouse_press(
        &mut self,
        document: &Document,
        at: PointF,
        button: MouseButton,
        modifiers: KeyModifiers,
    ) {
        self.press_widget = at;
        self.press_paper = self.widget_to_paper(at);
        self.guide_x = None;
        self.guide_y = None;
        self.press_moved = false;
        self.narrow_on_click = false;
        self.press_hit.clear();
        self.start_rects.clear();
        self.live_delta = Point2::default();
        // The middle button, or the left with Space held, drags the paper about.
        if button == MouseButton::Middle || (button == MouseButton::Left && self.space_held) {
            self.drag = Drag::Pan;
            self.cursor = CanvasCursor::ClosedHand;
            return;
        }
        if button != MouseButton::Left {
            return;
        }
        // The rulers are not paper.
        if at.x < self.ruler_pixels() || at.y < self.ruler_pixels() {
            return;
        }
        let shift = modifiers.shift;
        let control = modifiers.ctrl;
        if !shift
            && !control
            && let Some(handle) = self.handle_at(at)
        {
            self.drag = Drag::Resize;
            self.handle = handle;
            let rect = self.viewport(&self.selected).map(|v| v.rect).unwrap_or_default();
            self.start_rect = rect;
            self.live_rect = rect;
            return;
        }
        let hit = self.viewport_at(at);
        let Some(v) = self.viewport(&hit).cloned() else {
            // Empty paper: a rubber band, adding to the selection with Ctrl or Shift, else
            // replacing it when it is let go - so Escape part way leaves the selection as it was.
            self.band_adds = shift || control;
            self.drag = Drag::Band;
            self.band_corner = self.press_paper;
            return;
        };
        if shift && (v.kind == ViewportKind::Plan || v.kind == ViewportKind::KeyPlan) {
            // Shift-drag pans the drawing inside a plan; a Shift-click without a drag toggles the
            // plan in the selection (commit_drag).
            self.drag = Drag::PanView;
            self.pan_view_id = hit;
            self.view_shift = Point2::default();
            self.start_centre = self.resolved_plan(document, &v).centre;
            self.cursor = CanvasCursor::SizeAll;
            return;
        }
        if shift || control {
            self.toggle_selected(&hit);
            return;
        }
        if self.is_selected(&hit) {
            // One of a group: it becomes the primary and the group moves with it; let go without
            // moving, it is selected alone.
            self.narrow_on_click = self.selection.len() > 1;
            let selection = self.selection.clone();
            self.set_selection(&selection, &hit);
        } else {
            self.select(&hit);
        }
        self.press_hit = hit;
        self.start_bounds = Box2::default();
        let mut starts = BTreeMap::new();
        let mut bounds = Box2::default();
        if let Some(sheet) = self.current_sheet() {
            for each in &sheet.viewports {
                if self.is_selected(&each.id) && !each.locked && !each.rect.empty() {
                    starts.insert(each.id.clone(), each.rect);
                    bounds.expand_box(each.rect);
                }
            }
        }
        self.start_rects = starts;
        self.start_bounds = bounds;
        // A press on a locked member of a group still waits for the release: let go without
        // moving, it is selected alone; dragged, nothing moves.
        if !self.start_rects.is_empty() || self.narrow_on_click {
            self.drag = Drag::Move;
            self.start_rect = self.start_bounds;
            self.live_rect = self.start_bounds;
        }
    }

    fn update_drag(&mut self, widget: PointF, modifiers: KeyModifiers) {
        let Some(sheet) = self.current_sheet().cloned() else {
            return;
        };
        // A press is a click until the cursor has gone a few pixels from it.
        if !self.press_moved {
            let gone = widget - self.press_widget;
            if gone.x.hypot(gone.y) < DRAG_START_PIXELS {
                return;
            }
            self.press_moved = true;
            self.narrow_on_click = false;
            if self.drag == Drag::PanView {
                self.cursor = CanvasCursor::SizeAll;
            }
        }
        let now = self.widget_to_paper(widget);
        let delta = now - self.press_paper;
        // Alt: no snapping at all, to the edges or to the grid.
        let snap = !modifiers.alt;
        let tolerance = if snap { SNAP_PIXELS / self.zoom } else { 0.0 };
        let grid = if snap && self.snap_grid {
            self.grid_mm
        } else {
            0.0
        };
        let area = drawing_area(&sheet);
        // What the moving viewports snap to: every placed viewport not moving.
        let others: Vec<Box2> = sheet
            .viewports
            .iter()
            .filter(|v| {
                let moving = if self.drag == Drag::Move {
                    self.start_rects.contains_key(&v.id)
                } else {
                    v.id == self.selected
                };
                !moving && !v.rect.empty()
            })
            .map(|v| v.rect)
            .collect();
        self.guide_x = None;
        self.guide_y = None;
        match self.drag {
            Drag::Move if self.start_rects.is_empty() => {
                // Only locked viewports under the hand: nothing to move.
            }
            Drag::Move => {
                // The group's bounds snap, and every member moves as they do.
                let mut moved = Box2::new(
                    self.start_bounds.min + delta,
                    self.start_bounds.max + delta,
                );
                if snap {
                    let snapped = snap_moving_rect(&moved, &area, &others, tolerance, grid);
                    moved = snapped.rect;
                    self.guide_x = snapped.guide_x;
                    self.guide_y = snapped.guide_y;
                }
                self.live_rect = moved;
                self.live_delta = moved.min - self.start_bounds.min;
            }
            Drag::Resize => {
                let start = self.start_rect;
                let mut r = start;
                // Which edges this handle moves: 0 TL, 1 T, 2 TR, 3 R, 4 BR, 5 B, 6 BL, 7 L.
                let h = self.handle;
                let left = h == 0 || h == 6 || h == 7;
                let right = h == 2 || h == 3 || h == 4;
                let top = h == 0 || h == 1 || h == 2;
                let bottom = h == 4 || h == 5 || h == 6;
                let mut xs = vec![area.min.x, area.max.x];
                let mut ys = vec![area.min.y, area.max.y];
                for o in &others {
                    xs.extend([o.min.x, o.max.x]);
                    ys.extend([o.min.y, o.max.y]);
                }
                // An edge snaps to another edge in reach, else to the grid.
                let snap_to = |value: f64, targets: &[f64], guide: &mut Option<f64>| {
                    let snapped = snap_edge(value, targets, tolerance, grid);
                    if snapped.guide.is_some() {
                        *guide = snapped.guide;
                    }
                    snapped.value
                };
                let (mut guide_x, mut guide_y) = (None, None);
                if left {
                    r.min.x = snap_to(start.min.x + delta.x, &xs, &mut guide_x)
                        .min(r.max.x - MINIMUM_VIEWPORT_MM);
                }
                if right {
                    r.max.x = snap_to(start.max.x + delta.x, &xs, &mut guide_x)
                        .max(r.min.x + MINIMUM_VIEWPORT_MM);
                }
                if bottom {
                    r.min.y = snap_to(start.min.y + delta.y, &ys, &mut guide_y)
                        .min(r.max.y - MINIMUM_VIEWPORT_MM);
                }
                if top {
                    r.max.y = snap_to(start.max.y + delta.y, &ys, &mut guide_y)
                        .max(r.min.y + MINIMUM_VIEWPORT_MM);
                }
                self.guide_x = guide_x;
                self.guide_y = guide_y;
                self.live_rect = r;
            }
            Drag::PanView => self.view_shift = delta,
            Drag::Band => self.band_corner = now,
            Drag::None | Drag::Pan => {}
        }
    }

    pub fn mouse_move(&mut self, document: &Document, at: PointF, modifiers: KeyModifiers) {
        self.update_readout(document, at);
        match self.drag {
            Drag::Pan => {
                self.origin += at - self.press_widget;
                self.press_widget = at;
                return;
            }
            Drag::Move | Drag::Resize | Drag::PanView | Drag::Band => {
                self.update_drag(at, modifiers);
                return;
            }
            Drag::None => {}
        }
        // The cursor says what a press would do.
        self.cursor = if self.space_held {
            CanvasCursor::OpenHand
        } else if let Some(handle) = self.handle_at(at) {
            CanvasCursor::Handle(handle)
        } else if !self.viewport_at(at).is_empty() {
            CanvasCursor::SizeAll
        } else {
            CanvasCursor::Arrow
        };
    }

    fn commit_drag(&mut self, document: &mut Document) {
        let drag = self.drag;
        let moved = self.press_moved;
        self.drag = Drag::None;
        self.press_moved = false;
        self.guide_x = None;
        self.guide_y = None;
        self.cursor = CanvasCursor::Arrow;
        let Some(sheet) = self.current_sheet().cloned() else {
            return;
        };
        // A refused edit leaves the viewport where it was; the paint shows that.
        let _status: Status = match drag {
            Drag::Band => {
                if !moved {
                    // A click on empty paper: nothing selected, unless it was to add.
                    if !self.band_adds {
                        self.select("");
                    }
                } else {
                    let band = self.band();
                    let mode = if self.band_corner.x < self.press_paper.x {
                        BandMode::Crossing
                    } else {
                        BandMode::Window
                    };
                    let picked = viewports_in_band(&sheet, &band, mode);
                    if self.band_adds {
                        let mut ids = self.selection.clone();
                        ids.extend(picked);
                        let primary = self.selected.clone();
                        self.set_selection(&ids, &primary);
                    } else {
                        self.set_selection(&picked, "");
                    }
                }
                Ok(())
            }
            Drag::PanView => {
                let id = std::mem::take(&mut self.pan_view_id);
                let shift = std::mem::take(&mut self.view_shift);
                match self.viewport(&id).cloned() {
                    // A Shift-click, not a drag.
                    Some(_) if !moved => {
                        self.toggle_selected(&id);
                        Ok(())
                    }
                    Some(v) if shift.x != 0.0 || shift.y != 0.0 => {
                        let resolved = self.resolved_plan(document, &v);
                        // The drawing follows the hand, so the centre moves the other way.
                        let centre = self.start_centre
                            - rotated(shift, v.rotation) * (resolved.scale / 1000.0);
                        let scale = resolved.scale;
                        edit_viewport(
                            document,
                            &id,
                            |edited| {
                                edited.centre = centre;
                                edited.scale = scale;
                                edited.auto_centre = false;
                                edited.auto_scale = false;
                                Ok(())
                            },
                            "PAN_VIEWPORT",
                        )
                    }
                    _ => Ok(()),
                }
            }
            Drag::Move if !moved => {
                // A click on one of a group: that one alone.
                if self.narrow_on_click {
                    let hit = self.press_hit.clone();
                    self.select(&hit);
                }
                Ok(())
            }
            Drag::Move if self.start_rects.len() == 1 => {
                // One viewport takes the snapped rectangle exactly.
                let (id, start) = self
                    .start_rects
                    .iter()
                    .next()
                    .map(|(id, start)| (id.clone(), *start))
                    .unwrap_or_default();
                let rect = self.live_rect;
                if rect == start {
                    Ok(())
                } else {
                    edit_viewport(
                        document,
                        &id,
                        |edited| {
                            edited.rect = rect;
                            Ok(())
                        },
                        "MOVE_VIEWPORT",
                    )
                }
            }
            Drag::Move if self.live_delta.x != 0.0 || self.live_delta.y != 0.0 => {
                // A group: every member by the same distance, one step.
                let ids: Vec<String> = self.start_rects.keys().cloned().collect();
                move_viewports(document, self.sheet, &ids, self.live_delta, "MOVE_VIEWPORTS")
            }
            Drag::Resize if moved => match self.viewport(&self.selected) {
                Some(v) if self.live_rect != v.rect => {
                    let rect = self.live_rect;
                    let id = self.selected.clone();
                    edit_viewport(
                        document,
                        &id,
                        |edited| {
                            edited.rect = rect;
                            Ok(())
                        },
                        "RESIZE_VIEWPORT",
                    )
                }
                _ => Ok(()),
            },
            _ => Ok(()),
        };
        self.start_rects.clear();
        self.narrow_on_click = false;
        self.view_shift = Point2::default();
    }

    pub fn mouse_release(&mut self, document: &mut Document) {
        if self.drag == Drag::Pan {
            self.drag = Drag::None;
            self.cursor = CanvasCursor::Arrow;
            return;
        }
        if self.drag != Drag::None {
            self.commit_drag(document);
        }
    }

    pub fn mouse_double_click(&mut self, at: PointF, button: MouseButton) {
        if button != MouseButton::Left || at.x < self.ruler_pixels() || at.y < self.ruler_pixels() {
            return;
        }
        // A viewport fills the window; the desk or empty paper fits the page.
        let hit = self.viewport_at(at);
        if let Some(rect) = self.viewport(&hit).map(|v| v.rect) {
            self.select(&hit);
            self.zoom_to(&rect);
        } else {
            self.fit_page();
        }
    }

    /// The wheel turned by `notches` (a notch is 120 of Qt's angle delta), about `at`.
    pub fn wheel(&mut self, at: PointF, notches: f64) {
        if notches == 0.0 {
            return;
        }
        let before = self.zoom;
        self.zoom = (self.zoom * 1.2_f64.powf(notches)).clamp(MINIMUM_ZOOM, MAXIMUM_ZOOM);
        let d = at - self.origin;
        self.origin = at - d * (self.zoom / before);
        self.fitted = true;
    }

    /// A key pressed on the canvas (the C++'s event() for Tab, then keyPressEvent): what the
    /// canvas does itself, and what it asks of the editor.
    pub fn key_press(&mut self, key: SheetKey, modifiers: KeyModifiers, auto_repeat: bool) -> CanvasAsks {
        // The actions whose shortcuts the canvas takes, before its own keys, as Qt's shortcut map
        // does.
        match key {
            SheetKey::PageUp => return CanvasAsks::PreviousSheet,
            SheetKey::PageDown => return CanvasAsks::NextSheet,
            _ => {}
        }
        // Tab steps through the viewports rather than out of the canvas; on a sheet with none it
        // moves the focus on as usual.
        if (key == SheetKey::Tab || key == SheetKey::Backtab)
            && let Some(sheet) = self.current_sheet()
            && !katana_cad::plotting::viewport_edits::cycle_viewport(sheet, "", true).is_empty()
        {
            let back = key == SheetKey::Backtab || modifiers.shift;
            self.cycle_selection(!back);
            return CanvasAsks::Nothing;
        }
        // Space held: a left drag pans the paper, as in drawing programs.
        if key == SheetKey::Space {
            if !auto_repeat {
                self.space_held = true;
                if self.drag == Drag::None {
                    self.cursor = CanvasCursor::OpenHand;
                }
            }
            return CanvasAsks::Nothing;
        }
        if key == SheetKey::Escape {
            // A drag under way is abandoned, leaving everything where it was.
            if self.drag != Drag::None {
                self.cancel_drag();
            } else {
                self.select("");
            }
            return CanvasAsks::Nothing;
        }
        if self.selection.is_empty() {
            return CanvasAsks::NotTaken;
        }
        if key == SheetKey::Delete || key == SheetKey::Backspace {
            return CanvasAsks::Remove;
        }
        let step = if modifiers.shift { 10.0 } else { 1.0 };
        // Every selected viewport, one step; the locked ones stay.
        match key {
            SheetKey::Left => CanvasAsks::Nudge(Point2::new(-step, 0.0)),
            SheetKey::Right => CanvasAsks::Nudge(Point2::new(step, 0.0)),
            SheetKey::Up => CanvasAsks::Nudge(Point2::new(0.0, step)),
            SheetKey::Down => CanvasAsks::Nudge(Point2::new(0.0, -step)),
            _ => CanvasAsks::NotTaken,
        }
    }

    /// On one of a group the context menu is for the group; elsewhere the viewport under the
    /// pointer is selected first. The id under the pointer, empty for none.
    pub fn context_menu_at(&mut self, at: PointF) -> String {
        let hit = self.viewport_at(at);
        if !self.is_selected(&hit) {
            self.select(&hit);
        }
        hit
    }
}

// ============================================================================
// SheetEditor
// ============================================================================

/// What the panel's fields, the context menu and the dialogs asked for while they were drawn,
/// done once the frame's drawing has let the drawing go: the C++ queued most of these too, since
/// the edit rebuilds the panel that asked.
pub(crate) enum Queued {
    EditViewport {
        id: String,
        step: &'static str,
        change: Box<dyn FnOnce(&mut Viewport)>,
    },
    EditSheet {
        index: usize,
        step: &'static str,
        change: Box<dyn FnOnce(&mut Sheet)>,
    },
    Line(String),
    Report(String, bool),
    PlanGrid(String, GridStyle, f64),
    LegendScope(String, LegendScope),
    HiddenLayers(String),
    BrowseImage(String),
    ChoosePaper,
    Trigger(String),
    SelectSheet(usize),
    RenameSheet(usize, String),
}

/// A number box of the panel: the value it showed when it was built, what it holds now, and
/// whether something was typed that is not handed over yet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct NumberField {
    shown: f64,
    value: f64,
    typing: bool,
}

/// The panel's fields between frames, read again from the document whenever the panel is built
/// again (rebuildProperties): after a change to the document, a change of selection or sheet, and
/// a refused line.
#[derive(Default)]
pub(crate) struct PanelFields {
    built_for: Option<(u64, u64, usize, u64)>,
    numbers: BTreeMap<String, NumberField>,
    texts: BTreeMap<String, String>,
    choices: BTreeMap<String, Choice>,
    typing: BTreeSet<String>,
    /// The tooltips of the named controls drawn last, for an agent and a test to read
    /// (QWidget::toolTip): the foundation's records carry no tooltip.
    pub(crate) tips: BTreeMap<String, String>,
    pub(crate) grid: GridRows,
}

impl PanelFields {
    fn clear(&mut self) {
        self.numbers.clear();
        self.texts.clear();
        self.choices.clear();
        self.typing.clear();
        self.grid = GridRows::new();
    }
}

/// The Generate Sheets dialog's layouts, in its order: the GENERATE kind each runs, and what the
/// list says.
const GENERATE_LAYOUTS: [(&str, &str); 7] = [
    ("fit", "The drawing, fitted (one sheet, or tiles at a set scale)"),
    ("grid", "Tiles over the drawing, with a key plan"),
    ("strips", "Plan strips along an alignment"),
    ("profile", "Plan and profile along an alignment"),
    ("sections", "Cross sections along an alignment"),
    ("frames", "One sheet per imported plot frame"),
    ("register", "Drawing register (cover sheet)"),
];

/// The vertical exaggerations offered for cross sections; Auto fits it to the ground, as an
/// automatic section view does.
const EXAGGERATIONS: [f64; 5] = [1.0, 2.0, 5.0, 10.0, 20.0];

fn area_text(area: &Box2) -> String {
    format!(
        "{},{},{},{}",
        verb_number(area.min.x),
        verb_number(area.min.y),
        verb_number(area.max.x),
        verb_number(area.max.y)
    )
}

/// A chainage typed in a From or To box: nothing for an empty box (the alignment's end), else the
/// number; an error for anything else.
fn typed_chainage(text: &str, what: &str) -> katana_core::Result<Option<f64>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    parse_finite_double(text).map(Some).ok_or_else(|| {
        make_error(
            ErrorCode::ParseFailure,
            format!("{what} is a chainage in metres"),
            text,
        )
    })
}

/// Which cross sections Generate Sheets asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GenerateSections {
    #[default]
    None,
    Every,
    AtStations,
}

/// Which of Generate Sheets' controls a layout takes (the C++'s enable()).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GenerateEnabled {
    paper: bool,
    area: bool,
    corners: bool,
    scale: bool,
    alignment: bool,
    range: bool,
    overlap: bool,
    key_plan: bool,
    drawn_only: bool,
    no_sections: bool,
    every: bool,
    at_stations: bool,
    interval: bool,
    stations: bool,
    half_width: bool,
    sections_grid: bool,
    snapshot: bool,
    rotate: bool,
    replace: bool,
}

/// Generate Sheets: every layout and every option GENERATE takes. OK builds the GENERATE line and
/// runs it through the editor (run_line), so the dialog and the verb cannot come to lay out
/// different sheets.
///
/// Object names: the dialog sheetGenerateDialog; sheetGenerateLayout (the kinds, in
/// GENERATE_LAYOUTS' order), sheetGeneratePaper, sheetGenerateOrientation, sheetGenerateFrame;
/// sheetGenerateArea (the whole drawing, the current plan view, a window) and the window's
/// sheetGenerateAreaX0, Y0, X1 and Y1; sheetGenerateScale, sheetGenerateAlignment,
/// sheetGenerateFrom, sheetGenerateTo, sheetGenerateOverlap, sheetGenerateKeyPlan,
/// sheetGenerateDrawnOnly; the cross sections sheetGenerateNoSections, sheetGenerateEvery with
/// sheetGenerateInterval, sheetGenerateAtStations with sheetGenerateStations,
/// sheetGenerateHalfWidth, sheetGenerateRows, sheetGenerateColumns and sheetGenerateVe;
/// sheetGenerateModel3d, sheetGenerateLegend, sheetGenerateRotate, sheetGenerateReplace; the line
/// OK runs, sheetGenerateLine; and the buttons sheetGenerateOk and sheetGenerateCancel.
pub struct GenerateDialog {
    frame: crate::shell::DialogFrame,
    pub layout: Choice,
    pub paper: Choice,
    pub orientation: Choice,
    pub frame_on: bool,
    /// The area's items: what the list says, and what it stands for.
    areas: Vec<(&'static str, &'static str)>,
    pub area: Choice,
    pub corners: [f64; 4],
    pub scale: Choice,
    /// The alignments: "(none)" first, then each by name.
    alignments: Vec<String>,
    pub alignment: Choice,
    pub from: String,
    pub to: String,
    pub overlap: f64,
    pub key_plan: bool,
    pub drawn_only: bool,
    pub sections: GenerateSections,
    pub interval: f64,
    pub stations: String,
    pub half_width: f64,
    pub rows: i64,
    pub columns: i64,
    pub ve: Choice,
    pub snapshot: bool,
    pub legend: bool,
    pub rotate: bool,
    pub replace: bool,
    /// The layout the alignment was last chosen for.
    last_layout: Option<usize>,
}

/// What a frame of Generate Sheets came to.
pub(crate) enum GenerateAnswer {
    Open,
    Cancelled,
    /// OK: the line, or what is wrong with the choices, and where the new sheets go.
    Accepted {
        line: katana_core::Result<String>,
        cover: bool,
        replace: bool,
    },
}

impl GenerateDialog {
    fn new(extent: Box2, alignments: Vec<String>, plan_view: Option<Box2>) -> Self {
        let mut areas = vec![("The whole drawing", "drawing")];
        if plan_view.is_some_and(|shown| !shown.empty()) {
            areas.push(("The current plan view", "view"));
        }
        areas.push(("A window", "window"));
        let mut names = vec![String::from("(none)")];
        names.extend(alignments);
        let round = |value: f64| (value * 1000.0).round() / 1000.0;
        Self {
            // Opened with QDialog::open (or exec), and deleted on close.
            frame: crate::shell::DialogFrame::new("sheetGenerateDialog", "Generate Sheets")
                .modal(true)
                .deletes_on_close(true),
            layout: Choice::at(0),
            paper: Choice::at(3),
            orientation: Choice::at(0),
            frame_on: true,
            areas,
            area: Choice::at(0),
            corners: [
                round(extent.min.x),
                round(extent.min.y),
                round(extent.max.x),
                round(extent.max.y),
            ],
            scale: Choice {
                index: Some(0),
                text: String::from("Auto"),
                editable: true,
            },
            alignments: names,
            alignment: Choice::at(0),
            from: String::new(),
            to: String::new(),
            overlap: 10.0,
            key_plan: true,
            drawn_only: true,
            sections: GenerateSections::None,
            interval: 20.0,
            stations: String::new(),
            half_width: 20.0,
            rows: 3,
            columns: 2,
            ve: Choice::at(0),
            snapshot: false,
            legend: true,
            rotate: false,
            replace: false,
            last_layout: Some(0),
        }
    }

    fn kind(&self) -> &'static str {
        GENERATE_LAYOUTS[self.layout.index.unwrap_or(0).min(GENERATE_LAYOUTS.len() - 1)].0
    }

    fn aligned(&self) -> String {
        match self.alignment.index {
            Some(index) if index > 0 => self.alignments.get(index).cloned().unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn fixed_scale(&self) -> Option<f64> {
        parse_scale(&self.scale.text)
    }

    fn area_choice(&self) -> &'static str {
        self.area
            .index
            .and_then(|index| self.areas.get(index))
            .map_or("drawing", |(_, data)| *data)
    }

    fn ve_data(&self) -> String {
        match self.ve.index {
            Some(index) if index > 0 => verb_number(EXAGGERATIONS[index - 1]),
            _ => String::from("auto"),
        }
    }

    /// The layout's own fix-ups the C++ made when it changed: a fit starts on no alignment, a
    /// layout along an alignment on the first; a choice the layout does not take is moved off.
    fn normalise(&mut self) {
        let layout = self.layout.index;
        if layout != self.last_layout {
            self.last_layout = layout;
            let k = self.kind();
            if k == "fit" {
                self.alignment.index = Some(0);
            } else if self.alignment.index == Some(0)
                && self.alignments.len() > 1
                && (k == "strips" || k == "profile" || k == "sections")
            {
                self.alignment.index = Some(1);
            }
        }
        let sections = self.kind() == "sections";
        if sections && self.sections == GenerateSections::None {
            self.sections = GenerateSections::Every;
        } else if !sections && self.sections == GenerateSections::AtStations {
            self.sections = GenerateSections::None;
        }
    }

    fn enabled(&self) -> GenerateEnabled {
        let k = self.kind();
        let fit = k == "fit";
        let grid = k == "grid";
        let strips = k == "strips";
        let profile = k == "profile";
        let sections = k == "sections";
        let frames = k == "frames";
        let register = k == "register";
        let aligned = !self.aligned().is_empty();
        let fixed_strips = strips && self.fixed_scale().is_some();
        let area = (fit && !aligned) || grid;
        let every = fit || profile || sections;
        let every_checked = self.sections == GenerateSections::Every;
        GenerateEnabled {
            paper: !frames,
            area,
            corners: area && self.area_choice() == "window",
            scale: !frames && !register,
            alignment: fit || strips || profile || sections,
            range: fixed_strips,
            overlap: grid || fixed_strips,
            key_plan: grid || fixed_strips,
            drawn_only: grid || (fit && !aligned && self.fixed_scale().is_some()),
            // None for a plan's sections, At chainages for a sections layout.
            no_sections: fit || profile,
            every,
            at_stations: sections,
            interval: every && every_checked,
            stations: sections && self.sections == GenerateSections::AtStations,
            half_width: sections || ((fit || profile) && every_checked),
            sections_grid: sections,
            snapshot: fit || profile,
            // Only a plan of an area turns; a strip follows its alignment.
            rotate: fit && !aligned && self.sections == GenerateSections::None,
            replace: !register,
        }
    }

    /// The GENERATE line the choices make, every option the layout takes written out, so the line
    /// says the whole choice; or what is wrong with them.
    pub fn build_line(&self, plan_view: Option<Box2>) -> katana_core::Result<String> {
        let k = self.kind();
        let mut words = vec![String::from("GENERATE"), String::from(k)];
        let frames = k == "frames";
        let along = k == "strips" || k == "profile" || k == "sections";
        let aligned = self.aligned();
        if !frames {
            words.push(format!(
                "paper={}",
                self.paper.current_text(&paper_items())
            ));
            words.push(String::from(if self.orientation.index == Some(1) {
                "portrait"
            } else {
                "landscape"
            }));
        }
        words.push(format!("frame={}", if self.frame_on { "on" } else { "off" }));
        if k == "frames" || k == "register" {
            return Ok(words.join(" "));
        }
        let fit = k == "fit";
        if (fit && aligned.is_empty()) || k == "grid" {
            match self.area_choice() {
                "view" => match plan_view {
                    Some(shown) if !shown.empty() => words.push(format!("area={}", area_text(&shown))),
                    _ => {
                        return Err(katana_core::Error::new(
                            ErrorCode::InvalidState,
                            "there is no plan view to take the area from",
                        ));
                    }
                },
                "window" => {
                    let c = self.corners;
                    let window = Box2::new(
                        Point2::new(c[0].min(c[2]), c[1].min(c[3])),
                        Point2::new(c[0].max(c[2]), c[1].max(c[3])),
                    );
                    if !(window.width() > 0.0) || !(window.height() > 0.0) {
                        return Err(katana_core::Error::new(
                            ErrorCode::InvalidArgument,
                            "the window needs a width and a height",
                        ));
                    }
                    words.push(format!("area={}", area_text(&window)));
                }
                _ => {}
            }
        }
        if (fit || along) && !aligned.is_empty() {
            words.push(format!("alignment={}", quoted(&aligned)));
        }
        let fixed = self.fixed_scale();
        if k == "grid" {
            // Tiles are cut at a fixed scale; Auto is GENERATE grid's own 500.
            if let Some(fixed) = fixed {
                words.push(format!("scale={}", verb_number(fixed)));
            }
        } else {
            words.push(match fixed {
                Some(fixed) => format!("scale={}", verb_number(fixed)),
                None => String::from("scale=auto"),
            });
        }
        let fixed_strips = k == "strips" && fixed.is_some();
        if k == "grid" || fixed_strips {
            words.push(format!("overlap={}", verb_number(self.overlap)));
            words.push(format!("keyplan={}", if self.key_plan { "on" } else { "off" }));
        }
        // Tiles, which an area's plan at a fixed scale may spill onto too: those with nothing
        // drawn in them left out, or kept.
        if k == "grid" || (fit && aligned.is_empty() && fixed.is_some()) {
            words.push(format!("empty={}", if self.drawn_only { "off" } else { "on" }));
        }
        if fixed_strips {
            for (text, key) in [(&self.from, "from"), (&self.to, "to")] {
                if let Some(chainage) = typed_chainage(text, key)? {
                    words.push(format!("{key}={}", verb_number(chainage)));
                }
            }
        }
        let with_plan = fit || k == "profile";
        if with_plan {
            if self.sections == GenerateSections::Every {
                words.push(format!("interval={}", verb_number(self.interval)));
                words.push(format!("halfwidth={}", verb_number(self.half_width)));
            }
            words.push(format!("model3d={}", if self.snapshot { "on" } else { "off" }));
            words.push(format!("legend={}", if self.legend { "on" } else { "off" }));
        }
        if fit && self.rotate && self.enabled().rotate {
            words.push(String::from("rotate=on"));
        }
        if k == "sections" {
            if self.sections == GenerateSections::AtStations {
                match station_list(&self.stations) {
                    Some(list) if !list.is_empty() => words.push(format!("stations={list}")),
                    _ => {
                        return Err(make_error(
                            ErrorCode::ParseFailure,
                            "list the chainages to cut, separated by commas",
                            self.stations.clone(),
                        ));
                    }
                }
            } else {
                words.push(format!("interval={}", verb_number(self.interval)));
            }
            words.push(format!("halfwidth={}", verb_number(self.half_width)));
            words.push(format!("rows={}", self.rows));
            words.push(format!("columns={}", self.columns));
            words.push(format!("ve={}", self.ve_data()));
        }
        if self.replace {
            words.push(String::from("replace=on"));
        }
        Ok(words.join(" "))
    }

    fn ui(&mut self, ctx: &egui::Context, plan_view: Option<Box2>) -> GenerateAnswer {
        self.normalise();
        let mut frame = self.frame.clone();
        let mut answer = GenerateAnswer::Open;
        let shown = frame.show(
            ctx,
            &mut (&mut *self, &mut answer),
            |ui, (dialog, _)| dialog.body(ui, plan_view),
            |ui, (dialog, answer)| {
                let line = dialog.build_line(plan_view);
                // The one footer (shell::dialog::button_row): Cancel, then Generate, the primary,
                // last at the right - where the two sat at the left, Generate first and plain.
                let ((), (cancelled, ok)) = crate::shell::dialog::button_row(
                    ui,
                    |_| {},
                    |ui| {
                        let cancelled =
                            controls::button(ui, "sheetGenerateCancel", "Cancel").clicked();
                        let ok = ui
                            .add_enabled_ui(line.is_ok(), |ui| {
                                controls::primary(ui, "sheetGenerateOk", "Generate")
                            })
                            .inner
                            .clicked();
                        (cancelled, ok)
                    },
                );
                if ok {
                    let k = dialog.kind();
                    **answer = GenerateAnswer::Accepted {
                        line: dialog.build_line(plan_view),
                        cover: k == "register",
                        replace: dialog.replace,
                    };
                }
                if cancelled {
                    **answer = GenerateAnswer::Cancelled;
                }
            },
        );
        self.frame = frame;
        if shown.closed {
            answer = GenerateAnswer::Cancelled;
        }
        if !matches!(answer, GenerateAnswer::Open) {
            ctx.request_repaint();
        }
        answer
    }

    fn body(&mut self, ui: &mut Ui, plan_view: Option<Box2>) {
        ui.set_min_width(500.0);
        let on = self.enabled();
        let before = (
            self.layout.clone(),
            self.alignment.clone(),
            self.sections,
            self.area.clone(),
        );
        dialog::form(ui, |form| {
            let ui = form.ui();
            dialog::labelled(ui, "Layout", |ui| {
                let layouts: Vec<String> = GENERATE_LAYOUTS
                    .iter()
                    .map(|(_, text)| String::from(*text))
                    .collect();
                controls::choice(ui, "sheetGenerateLayout", &layouts, &mut self.layout);
            });

            // The paper, for every layout but the plot frames', which bring their own.
            dialog::labelled(ui, "Paper", |ui| {
                ui.horizontal(|ui| {
                    // The paper and its orientation a word's width each, the frame's box beside.
                    dialog::fixed_fields(ui, dialog::SHORT_FIELD_WIDTH);
                    ui.add_enabled_ui(on.paper, |ui| {
                        controls::choice(ui, "sheetGeneratePaper", &paper_items(), &mut self.paper);
                        let orientations = [String::from("Landscape"), String::from("Portrait")];
                        controls::choice(
                            ui,
                            "sheetGenerateOrientation",
                            &orientations,
                            &mut self.orientation,
                        );
                    });
                    controls::check(ui, "sheetGenerateFrame", "Frame and title block", &mut self.frame_on)
                        .on_hover_text(
                            "A portrait sheet has no frame: the title block is laid out for landscape",
                        );
                });
            });

            // What a fit or the tiles cover.
            dialog::labelled(ui, "Area", |ui| {
                let areas: Vec<String> = self.areas.iter().map(|(text, _)| String::from(*text)).collect();
                ui.add_enabled_ui(on.area, |ui| {
                    controls::choice(ui, "sheetGenerateArea", &areas, &mut self.area);
                });
            });
            dialog::labelled(ui, "Window", |ui| {
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(on.corners, |ui| {
                        let names = [
                            ("sheetGenerateAreaX0", "West edge (E)"),
                            ("sheetGenerateAreaY0", "South edge (N)"),
                            ("sheetGenerateAreaX1", "East edge (E)"),
                            ("sheetGenerateAreaY1", "North edge (N)"),
                        ];
                        for (at, (name, tip)) in names.iter().enumerate() {
                            controls::spin(
                                ui,
                                name,
                                &mut self.corners[at],
                                SpinRange::new(-1e9, 1e9, 1.0, 3),
                            )
                            .on_hover_text(*tip);
                        }
                    });
                });
            });

            dialog::labelled(ui, "Scale", |ui| {
                ui.add_enabled_ui(on.scale, |ui| {
                    controls::choice(ui, "sheetGenerateScale", &scale_items(true), &mut self.scale);
                });
            });
            dialog::labelled(ui, "Alignment", |ui| {
                ui.add_enabled_ui(on.alignment, |ui| {
                    controls::choice(ui, "sheetGenerateAlignment", &self.alignments, &mut self.alignment)
                        .on_hover_text("None: the drawing's only alignment, or for a fit the area");
                });
            });
            dialog::labelled(ui, "Chainages", |ui| {
                ui.horizontal(|ui| {
                    // The two chainages share the row, half each, either side of the "to".
                    let gap = ui.spacing().item_spacing.x;
                    let between = dialog::one_line_width(ui, "to") + 2.0 * gap;
                    dialog::fixed_fields(ui, (ui.available_width() - between) / 2.0);
                    ui.add_enabled_ui(on.range, |ui| {
                        controls::line_with(
                            ui,
                            "sheetGenerateFrom",
                            &mut self.from,
                            LineOptions {
                                placeholder: "the start",
                                ..LineOptions::default()
                            },
                        );
                        ui.label("to");
                        controls::line_with(
                            ui,
                            "sheetGenerateTo",
                            &mut self.to,
                            LineOptions {
                                placeholder: "the end",
                                ..LineOptions::default()
                            },
                        );
                    });
                });
            });
            dialog::labelled(ui, "Overlap", |ui| {
                ui.add_enabled_ui(on.overlap, |ui| {
                    controls::spin(
                        ui,
                        "sheetGenerateOverlap",
                        &mut self.overlap,
                        SpinRange::new(0.0, 1000.0, 1.0, 1).with_suffix(" m"),
                    );
                });
            });
            dialog::labelled(ui, "", |ui| {
                ui.add_enabled_ui(on.key_plan, |ui| {
                    controls::check(ui, "sheetGenerateKeyPlan", "A key plan sheet first", &mut self.key_plan);
                });
            });
            dialog::labelled(ui, "", |ui| {
                ui.add_enabled_ui(on.drawn_only, |ui| {
                    controls::check(
                        ui,
                        "sheetGenerateDrawnOnly",
                        "Only tiles with something drawn in them",
                        &mut self.drawn_only,
                    )
                    .on_hover_text(
                        "A tile over empty ground is left out, and the key plan shows the tiles kept",
                    );
                });
            });

            // Cross sections: after a fit's or a profile's plan, or the whole of a sections
            // layout; every so many metres or at the chainages listed.
            dialog::labelled(ui, "Cross sections", |ui| {
                ui.horizontal(|ui| {
                    let none = self.sections == GenerateSections::None;
                    if ui
                        .add_enabled_ui(on.no_sections, |ui| {
                            controls::radio(ui, "sheetGenerateNoSections", "None", none)
                        })
                        .inner
                        .clicked()
                    {
                        self.sections = GenerateSections::None;
                    }
                    let every = self.sections == GenerateSections::Every;
                    if ui
                        .add_enabled_ui(on.every, |ui| controls::radio(ui, "sheetGenerateEvery", "Every", every))
                        .inner
                        .clicked()
                    {
                        self.sections = GenerateSections::Every;
                    }
                    ui.add_enabled_ui(on.interval, |ui| {
                        controls::spin(
                            ui,
                            "sheetGenerateInterval",
                            &mut self.interval,
                            SpinRange::new(0.5, 10000.0, 1.0, 2).with_suffix(" m"),
                        );
                    });
                    let at = self.sections == GenerateSections::AtStations;
                    if ui
                        .add_enabled_ui(on.at_stations, |ui| {
                            controls::radio(ui, "sheetGenerateAtStations", "At chainages", at)
                        })
                        .inner
                        .clicked()
                    {
                        self.sections = GenerateSections::AtStations;
                    }
                    ui.add_enabled_ui(on.stations, |ui| {
                        // The list fills what the radio buttons leave of the row.
                        let width = ui.available_width();
                        controls::line_with(
                            ui,
                            "sheetGenerateStations",
                            &mut self.stations,
                            LineOptions {
                                placeholder: "e.g. 20, 40, 60",
                                width: Some(width),
                                ..LineOptions::default()
                            },
                        );
                    });
                });
            });
            dialog::labelled(ui, "Section half width", |ui| {
                ui.add_enabled_ui(on.half_width, |ui| {
                    controls::spin(
                        ui,
                        "sheetGenerateHalfWidth",
                        &mut self.half_width,
                        SpinRange::new(0.5, 1000.0, 1.0, 2).with_suffix(" m"),
                    );
                });
            });
            dialog::labelled(ui, "Sections down", |ui| {
                ui.add_enabled_ui(on.sections_grid, |ui| {
                    controls::whole(ui, "sheetGenerateRows", &mut self.rows, 1..=8);
                });
            });
            dialog::labelled(ui, "Sections across", |ui| {
                ui.add_enabled_ui(on.sections_grid, |ui| {
                    controls::whole(ui, "sheetGenerateColumns", &mut self.columns, 1..=6);
                });
            });
            dialog::labelled(ui, "Vertical exaggeration", |ui| {
                let mut ves = vec![String::from("Auto")];
                ves.extend(EXAGGERATIONS.iter().map(|x| format!("{} x", verb_number(*x))));
                ui.add_enabled_ui(on.sections_grid, |ui| {
                    controls::choice(ui, "sheetGenerateVe", &ves, &mut self.ve)
                        .on_hover_text("Auto: fitted to the ground each section cuts");
                });
            });

            dialog::labelled(ui, "", |ui| {
                ui.add_enabled_ui(on.snapshot, |ui| {
                    controls::check(ui, "sheetGenerateModel3d", "A 3D snapshot beside the plan", &mut self.snapshot);
                });
            });
            dialog::labelled(ui, "", |ui| {
                ui.add_enabled_ui(on.snapshot, |ui| {
                    controls::check(ui, "sheetGenerateLegend", "A legend beside the plan", &mut self.legend);
                });
            });
            dialog::labelled(ui, "", |ui| {
                ui.add_enabled_ui(on.rotate, |ui| {
                    controls::check(
                        ui,
                        "sheetGenerateRotate",
                        "Rotate the drawing to fill the sheet",
                        &mut self.rotate,
                    )
                    .on_hover_text(
                        "Turn the plan when that shows the drawing at a larger scale, or on fewer \
                         sheets, and no further than it needs",
                    );
                });
            });
            dialog::labelled(ui, "", |ui| {
                ui.add_enabled_ui(on.replace, |ui| {
                    controls::check(
                        ui,
                        "sheetGenerateReplace",
                        "Replace the sheets there are now",
                        &mut self.replace,
                    );
                });
            });
        });
        // The line OK runs, as it will be typed into the log: what a person learns the verb from,
        // and what an agent would type instead.
        let line = self.build_line(plan_view);
        let (text, colour) = match &line {
            Ok(line) => (line.clone(), None),
            Err(error) => (error.message.clone(), Some(crate::theme::error().to_egui())),
        };
        let mut rich = egui::RichText::new(text.as_str());
        if let Some(colour) = colour {
            rich = rich.color(colour);
        }
        controls::label_as(ui, "sheetGenerateLine", &text, egui::Label::new(rich).wrap());
        if (
            self.layout.clone(),
            self.alignment.clone(),
            self.sections,
            self.area.clone(),
        ) != before
        {
            // A layout's fix-ups and what it enables are worked out at the next frame.
            ui.ctx().request_repaint();
        }
    }
}

pub(crate) fn paper_items() -> Vec<String> {
    PAPERS.iter().map(|paper| String::from(paper_name(*paper))).collect()
}

/// Hidden layers... for a view: the layers it hides, ticked off, run as VIEW SET id hide= show=
/// for what changed (sheetHiddenLayersDialog; its list sheetHiddenLayersList, OK
/// sheetHiddenLayersOk).
pub struct HiddenLayersDialog {
    frame: crate::shell::DialogFrame,
    id: String,
    /// Each layer, whether the view hides it directly as it was opened, and whether it is ticked
    /// (shown) now.
    layers: Vec<(String, bool, bool)>,
}

impl HiddenLayersDialog {
    /// A layer ticked or not, as a click on its box does (QListWidgetItem::setCheckState).
    pub fn set_shown(&mut self, layer: &str, shown: bool) -> bool {
        match self.layers.iter_mut().find(|(name, _, _)| name == layer) {
            Some(entry) => {
                entry.2 = shown;
                true
            }
            None => false,
        }
    }

    /// The layers listed, and whether each is ticked.
    pub fn layers(&self) -> Vec<(String, bool)> {
        self.layers
            .iter()
            .map(|(name, _, shown)| (name.clone(), *shown))
            .collect()
    }

    /// The options OK runs: only what the ticks changed, a layer an option; an entry the view
    /// hides that is no layer of its own (a parent path) is left as it is.
    fn options(&self) -> Vec<String> {
        self.layers
            .iter()
            .filter(|(_, hidden, shown)| !*shown != *hidden)
            .map(|(name, _, shown)| {
                format!("{}{}", if *shown { "show=" } else { "hide=" }, quoted(name))
            })
            .collect()
    }

    /// One frame: Some(true) for OK, Some(false) for Cancel.
    fn ui(&mut self, ctx: &egui::Context) -> Option<bool> {
        let mut frame = self.frame.clone();
        let mut answer = None;
        let shown = frame.show(
            ctx,
            &mut (&mut *self, &mut answer),
            |ui, (dialog, _)| {
                let name = "sheetHiddenLayersList";
                let taken = take(ui, name);
                if let Some(taken) = &taken {
                    match &taken.posted.act {
                        Act::Fill(text) => settle(ui, taken, Err(no_field(name, text))),
                        Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                        Act::ScrollTo => settle(ui, taken, Ok(())),
                    }
                }
                let response = ui
                    .vertical(|ui| {
                        for (layer, _, shown) in &mut dialog.layers {
                            controls::check_box(ui, shown, layer.as_str());
                        }
                    })
                    .response;
                let mut record = ControlRecord::new(name, ControlKind::Rows);
                record.items = dialog
                    .layers
                    .iter()
                    .map(|(layer, _, shown)| format!("{layer} | {}", if *shown { "on" } else { "off" }))
                    .collect();
                controls::custom(ui, &response, record);
            },
            |ui, (_, answer)| {
                // The one footer (shell::dialog::button_row): Cancel, then OK, the primary, last.
                let ((), (cancelled, ok)) = crate::shell::dialog::button_row(
                    ui,
                    |_| {},
                    |ui| {
                        (
                            controls::button(ui, "sheetHiddenLayersCancel", "Cancel").clicked(),
                            controls::primary(ui, "sheetHiddenLayersOk", "OK").clicked(),
                        )
                    },
                );
                if ok {
                    **answer = Some(true);
                }
                if cancelled {
                    **answer = Some(false);
                }
            },
        );
        self.frame = frame;
        if shown.closed {
            answer = Some(false);
        }
        answer
    }
}

/// The Title Block dialog: the organisation, project, sign-offs, notes, revisions and logo every
/// sheet shares, kept as one step (TITLE_BLOCK) when it is accepted; the logo by its own step as
/// it is chosen. The C++ named none of its controls; here each is `sheetTitleBlock...`.
pub struct TitleBlockDialog {
    frame: crate::shell::DialogFrame,
    tab: usize,
    pub organisation: String,
    pub project_lines: [String; 4],
    pub client: String,
    pub set_number: String,
    pub numbering: String,
    pub coordinates: String,
    pub datum: String,
    pub model_name: String,
    /// Utilities located by, surveyed, compiled, reviewed and approved by: name and date.
    pub sign_offs: [(String, String); 5],
    pub notes: String,
    pub revisions: Vec<[String; 4]>,
    pub current_revision: Option<usize>,
    placeholders: [String; 3],
    logo_changed: bool,
}

const SIGN_OFF_LABELS: [&str; 5] = [
    "Utilities located by",
    "Surveyed by",
    "Compiled by",
    "Reviewed by",
    "Approved by",
];

/// What a frame of the title block dialog asked for.
enum TitleBlockAnswer {
    Open,
    Ok,
    Cancel,
    ChooseLogo,
    NoLogo,
}

impl TitleBlockDialog {
    fn new(set: &SheetSet, fields: &katana_cad::plotting::sheet_set::FieldContext) -> Self {
        let d = &set.defaults;
        let line = |i: usize| d.project_lines.get(i).cloned().unwrap_or_default();
        let sign = |s: &SignOff| (s.name.clone(), s.date.clone());
        Self {
            frame: crate::shell::DialogFrame::new("sheetTitleBlockDialog", "Title Block"),
            tab: 0,
            organisation: d.organisation.clone(),
            project_lines: [line(0), line(1), line(2), line(3)],
            client: d.client.clone(),
            set_number: d.set_number.clone(),
            numbering: set.numbering.clone(),
            coordinates: d.coordinate_system.clone(),
            datum: d.height_datum.clone(),
            model_name: d.model_name.clone(),
            sign_offs: [
                sign(&d.locator),
                sign(&d.surveyor),
                sign(&d.compiler),
                sign(&d.reviewer),
                sign(&d.approver),
            ],
            notes: d.notes.clone(),
            revisions: set
                .revisions
                .iter()
                .map(|r| [r.code.clone(), r.date.clone(), r.description.clone(), r.by.clone()])
                .collect(),
            current_revision: None,
            placeholders: [
                fields.project_name.clone(),
                fields.project_description.clone(),
                fields.coordinate_system.clone(),
            ],
            logo_changed: false,
        }
    }

    fn ui(&mut self, ctx: &egui::Context, logo: &str) -> TitleBlockAnswer {
        let mut frame = self.frame.clone();
        let mut answer = TitleBlockAnswer::Open;
        let shown = frame.show(
            ctx,
            &mut (&mut *self, &mut answer),
            |ui, (dialog, answer)| dialog.body(ui, logo, answer),
            |ui, (_, answer)| {
                // The one footer (shell::dialog::button_row): Cancel, then OK, the primary, last
                // at the right - where the two sat at the left, OK plain.
                let ((), (cancelled, ok)) = crate::shell::dialog::button_row(
                    ui,
                    |_| {},
                    |ui| {
                        (
                            controls::button(ui, "sheetTitleBlockCancel", "Cancel").clicked(),
                            controls::primary(ui, "sheetTitleBlockOk", "OK").clicked(),
                        )
                    },
                );
                if ok {
                    **answer = TitleBlockAnswer::Ok;
                }
                if cancelled {
                    **answer = TitleBlockAnswer::Cancel;
                }
            },
        );
        self.frame = frame;
        if shown.closed {
            answer = TitleBlockAnswer::Cancel;
        }
        answer
    }

    fn body(&mut self, ui: &mut Ui, logo: &str, answer: &mut TitleBlockAnswer) {
        ui.set_min_width(620.0);
        controls::tabs(
            ui,
            "sheetTitleBlockTabs",
            &["Project", "Sign-offs", "Notes", "Revisions", "Logo"],
            &mut self.tab,
        );
        let line = |ui: &mut Ui, name: &str, value: &mut String, placeholder: &str| {
            controls::line_with(
                ui,
                name,
                value,
                LineOptions {
                    placeholder,
                    ..LineOptions::default()
                },
            );
        };
        // Every page drawn, the ones behind the front one invisible, so a driver can fill a
        // control on any of them (docs/rust.md, wave 1).
        page(ui, 0, self.tab, |ui| {
            dialog::form(ui, |form| {
                let ui = form.ui();
                dialog::labelled(ui, "Organisation", |ui| {
                    line(ui, "sheetTitleBlockOrganisation", &mut self.organisation, "your organisation's name");
                });
                for i in 0..4 {
                    dialog::labelled(ui, &format!("Project line {}", i + 1), |ui| {
                        let placeholder = if i < 2 { self.placeholders[i].clone() } else { String::new() };
                        line(
                            ui,
                            &format!("sheetTitleBlockProjectLine{}", i + 1),
                            &mut self.project_lines[i],
                            &placeholder,
                        );
                    });
                }
                dialog::labelled(ui, "Client", |ui| {
                    line(ui, "sheetTitleBlockClient", &mut self.client, "");
                });
                dialog::labelled(ui, "Drawing set number", |ui| {
                    line(ui, "sheetTitleBlockSetNumber", &mut self.set_number, "");
                });
                dialog::labelled(ui, "Sheet numbering", |ui| {
                    line(ui, "sheetTitleBlockNumbering", &mut self.numbering, "{n}");
                });
                dialog::labelled(ui, "Coordinate system", |ui| {
                    let placeholder = self.placeholders[2].clone();
                    line(ui, "sheetTitleBlockCoordinates", &mut self.coordinates, &placeholder);
                });
                dialog::labelled(ui, "Height datum", |ui| {
                    line(ui, "sheetTitleBlockDatum", &mut self.datum, "");
                });
                dialog::labelled(ui, "Model name", |ui| {
                    line(ui, "sheetTitleBlockModelName", &mut self.model_name, "");
                });
            });
        });
        page(ui, 1, self.tab, |ui| {
            dialog::form(ui, |form| {
                for (i, label) in SIGN_OFF_LABELS.iter().enumerate() {
                    form.row(label, |ui| {
                        ui.horizontal(|ui| {
                            // The name a name's width, the date the rest of the row.
                            let (name, date) = &mut self.sign_offs[i];
                            dialog::fixed_fields(ui, dialog::SHORT_FIELD_WIDTH);
                            line(ui, &format!("sheetTitleBlockSignOffName{}", i + 1), name, "name");
                            dialog::fill_fields(ui);
                            line(
                                ui,
                                &format!("sheetTitleBlockSignOffDate{}", i + 1),
                                date,
                                "dd/mm/yy (blank: the plot date)",
                            );
                        });
                    });
                }
            });
        });
        page(ui, 2, self.tab, |ui| {
            ui.label("Printed in the title block's notes cell, wrapped to fit:");
            controls::text(ui, "sheetTitleBlockNotes", &mut self.notes);
        });
        page(ui, 3, self.tab, |ui| {
            let rows: Vec<Vec<String>> = self.revisions.iter().map(|row| row.to_vec()).collect();
            controls::rows(ui, "sheetTitleBlockRevisions", &rows, &mut self.current_revision);
            egui::Grid::new("sheetTitleBlockRevisionCells").num_columns(4).show(ui, |ui| {
                for header in ["Rev", "Date", "Description", "By"] {
                    ui.strong(header);
                }
                ui.end_row();
                for (r, row) in self.revisions.iter_mut().enumerate() {
                    for (c, cell) in row.iter_mut().enumerate() {
                        controls::line(ui, &format!("sheetTitleBlockRevision_{r}_{c}"), cell);
                    }
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                if controls::button(ui, "sheetTitleBlockAddRevision", "Add").clicked() {
                    let row = self.revisions.len();
                    let code = if row < 26 {
                        String::from(char::from(b'A' + row as u8))
                    } else {
                        format!("{row}")
                    };
                    let today = katana_cad::plotting::sheet_commands::frame_date(
                        katana_core::civil_date::today_utc(),
                    );
                    self.revisions.push([code, today, String::new(), String::new()]);
                }
                if controls::button(ui, "sheetTitleBlockRemoveRevision", "Remove").clicked()
                    && let Some(row) = self.current_revision
                    && row < self.revisions.len()
                {
                    self.revisions.remove(row);
                    self.current_revision = None;
                }
            });
        });
        page(ui, 4, self.tab, |ui| {
            ui.label(
                "Your organisation's logo, printed in the title block's logo slot (53 x 12 mm), \
                 its shape kept. PNG, JPEG, GIF or BMP.",
            );
            let said = if logo.is_empty() {
                String::from("No logo. The slot prints empty.")
            } else {
                format!("Logo: {logo}")
            };
            controls::label(ui, "sheetTitleBlockLogo", &said);
            ui.horizontal(|ui| {
                if controls::button(ui, "sheetTitleBlockChooseLogo", "Choose Logo...").clicked() {
                    *answer = TitleBlockAnswer::ChooseLogo;
                }
                if controls::button(ui, "sheetTitleBlockNoLogo", "No Logo").clicked() {
                    *answer = TitleBlockAnswer::NoLogo;
                }
            });
        });
    }
}

/// A file dialog the editor asks for: whether it saves, its title, where it starts and its
/// filter. A test answers it (`SheetEditor::choose_file`) where the C++ tests caught the
/// QFileDialog as it opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileAsk {
    pub save: bool,
    pub title: String,
    pub start: String,
    pub filter: String,
}

/// A question the editor puts (QMessageBox::question): its object name and its text. True is Yes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestionAsk {
    pub name: String,
    pub title: String,
    pub text: String,
}

/// How the window's actions take their keys: where the focus is anywhere in the editor, or only
/// on the widgets named (Qt::WidgetWithChildrenShortcut).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutScope {
    Window,
    Widgets(&'static [&'static str]),
}

/// The sheet editor (see the module comment).
pub struct SheetEditor {
    pub(crate) source: SourceProvider,
    pub(crate) canvas: SheetCanvas,
    pub(crate) list: SheetListWidget,
    pub(crate) checks: SheetChecksDock,
    pub(crate) thumbnails: SheetThumbnails,
    /// Each list row's picture: the sheet's id, its picture (the last painted, else blank paper)
    /// and a serial that moves when the picture does.
    pub(crate) pictures: Vec<(String, Image, u64)>,
    pub(crate) picture_serial: u64,
    pub(crate) textures: BTreeMap<String, (u64, egui::TextureHandle)>,
    pub(crate) canvas_texture: Option<egui::TextureHandle>,
    pub(crate) thumbnails_due: Option<Instant>,
    /// Whether the window's executor runs the lines (setCommandRunner): the host's `run`.
    pub(crate) runner: bool,
    pub(crate) headless: bool,
    /// Where the window's plan view looks, for Generate Sheets' "The current plan view"; unset or
    /// nothing, and the choice is not offered.
    pub plan_view_area: Option<Box<dyn Fn() -> Option<Box2>>>,
    /// The system's file dialog, or what a test answers in its place.
    #[allow(clippy::type_complexity, reason = "A boxed callback, named where it is set.")]
    pub choose_file: Option<Box<dyn FnMut(&FileAsk) -> String>>,
    /// The questions the editor puts, or what a test answers in their place; unset, a question
    /// opens a box and waits for it (in a headless session none is put).
    #[allow(clippy::type_complexity, reason = "A boxed callback, named where it is set.")]
    pub answer_question: Option<Box<dyn FnMut(&QuestionAsk) -> bool>>,
    /// The status bar's message (statusBar()->showMessage).
    pub(crate) status: String,
    /// The status bar's own label: the sheet count, and why the sheets could not be read.
    pub(crate) count_text: String,
    /// The cursor's readout on the status bar's right (sheetCursorStatus).
    pub(crate) cursor_status: String,
    /// The model revision the editor last looked at the document at.
    pub(crate) seen: Option<u64>,
    pub(crate) shown: bool,
    pub(crate) fields: PanelFields,
    pub(crate) rebuilds: u64,
    pub(crate) queued: Vec<Queued>,
    pub(crate) generate: Option<GenerateDialog>,
    pub(crate) hidden_layers: Option<HiddenLayersDialog>,
    pub(crate) title_block: Option<TitleBlockDialog>,
    pub(crate) arrange: ArrangeDialogs,
    pub(crate) plot_dialog: Option<PlotDialog>,
    /// The context menu open, where, and the viewport it is for.
    pub(crate) context_menu: Option<(egui::Pos2, String)>,
    pub(crate) rename: Option<(usize, String)>,
    /// Whether the clipboard holds views to paste, as the editor last asked it.
    pub(crate) paste_ready: bool,
    pub(crate) focus_canvas: bool,
    /// The egui adaptor's double-click and held buttons.
    pub(crate) egui_state: CanvasInput,
    /// The Match Scale To list as it was last made, and what it was made for (the model revision,
    /// the selection and the sheet): the window's menus are made every frame, and the list asks
    /// the scale every view is drawn at, which cuts an automatic section.
    #[allow(clippy::type_complexity, reason = "A one-entry cache keyed as its comment says.")]
    pub(crate) match_scale_cache: RefCell<Option<((u64, u64, usize), Menu)>>,
}

/// What the egui adaptor keeps between frames about the pointer over the canvas.
#[derive(Clone, Debug, Default)]
pub(crate) struct CanvasInput {
    held: Vec<MouseButton>,
    last_press: Option<(f64, PointF, MouseButton)>,
    inside: bool,
}

const DOUBLE_CLICK_SECONDS: f64 = 0.4;
const DOUBLE_CLICK_PIXELS: f64 = 5.0;

/// How long the list waits after a change before it paints thumbnails, so a burst of edits costs
/// one round of painting.
pub(crate) const THUMBNAIL_DELAY_MS: u64 = 30;

impl SheetEditor {
    pub fn new(source: SourceProvider) -> Self {
        let mut editor = Self {
            canvas: SheetCanvas::new(Rc::clone(&source)),
            source,
            list: SheetListWidget::new("sheetList"),
            checks: SheetChecksDock::new(),
            thumbnails: SheetThumbnails::default(),
            pictures: Vec::new(),
            picture_serial: 0,
            textures: BTreeMap::new(),
            canvas_texture: None,
            thumbnails_due: None,
            runner: false,
            headless: false,
            plan_view_area: None,
            choose_file: None,
            answer_question: None,
            status: String::new(),
            count_text: String::new(),
            cursor_status: String::new(),
            seen: None,
            shown: false,
            fields: PanelFields::default(),
            rebuilds: 0,
            queued: Vec::new(),
            generate: None,
            hidden_layers: None,
            title_block: None,
            arrange: ArrangeDialogs::default(),
            plot_dialog: None,
            context_menu: None,
            rename: None,
            paste_ready: false,
            focus_canvas: false,
            egui_state: CanvasInput::default(),
            match_scale_cache: RefCell::new(None),
        };
        editor.count_text = String::from("0 sheets");
        editor
    }

    /// What the sheets are drawn from, for `document`.
    pub fn source<'d>(&self, document: &'d Document) -> SheetSource<'d> {
        (self.source)(document)
    }

    pub fn source_provider(&self) -> &SourceProvider {
        &self.source
    }

    pub fn canvas(&self) -> &SheetCanvas {
        &self.canvas
    }

    pub fn canvas_mut(&mut self) -> &mut SheetCanvas {
        &mut self.canvas
    }

    pub fn list(&self) -> &SheetListWidget {
        &self.list
    }

    pub fn list_mut(&mut self) -> &mut SheetListWidget {
        &mut self.list
    }

    /// The Checks dock, run again a moment after every change; its findings are the last run's.
    pub fn checks(&self) -> &SheetChecksDock {
        &self.checks
    }

    pub fn checks_mut(&mut self) -> &mut SheetChecksDock {
        &mut self.checks
    }

    pub fn current_sheet(&self) -> usize {
        self.canvas.sheet()
    }

    /// The status bar's message, as the last edit or line left it.
    pub fn status_message(&self) -> &str {
        &self.status
    }

    /// The status bar's own label: "2 sheets".
    pub fn count_text(&self) -> &str {
        &self.count_text
    }

    /// The cursor's readout on the status bar (sheetCursorStatus).
    pub fn cursor_status(&self) -> &str {
        &self.cursor_status
    }

    /// The tooltip the named control of the panel was drawn with last.
    pub fn tooltip(&self, name: &str) -> Option<&str> {
        self.fields.tips.get(name).map(String::as_str)
    }

    /// The window's one executor runs the editor's lines from now (MainWindow::runVerbLine through
    /// `Host::run`); off, the editor runs them by the interpreter's own sheet verbs.
    pub fn set_command_runner(&mut self, on: bool) {
        self.runner = on;
    }

    /// A headless session (MainWindow::setHeadless): no file dialog and no question is ever put;
    /// the item says instead which verb asks nothing.
    pub fn set_headless(&mut self, headless: bool) {
        self.headless = headless;
    }

    pub fn headless(&self) -> bool {
        self.headless
    }

    /// The window shown (QWidget::show): the checks missed while it was hidden are scheduled, and
    /// the thumbnails painted.
    pub fn show(&mut self, host: &dyn Host) {
        self.observe(host);
        if !self.shown {
            self.shown = true;
            self.checks.shown_again();
        }
        self.refresh_thumbnails(host);
    }

    pub fn hide(&mut self) {
        self.shown = false;
    }

    pub fn is_visible(&self) -> bool {
        self.shown
    }

    /// The editor follows the document: when its model revision has moved since the last look,
    /// everything is read again from it (the C++'s listener, which called refresh).
    pub fn observe(&mut self, host: &dyn Host) {
        let revision = host.document().model_revision();
        if self.seen != Some(revision) {
            self.seen = Some(revision);
            self.refresh(host);
        } else {
            self.canvas.set_shown(host.document().sheet_set());
        }
    }

    /// Everything read again from the document: the list, the canvas, the properties.
    pub fn refresh(&mut self, host: &dyn Host) {
        let document = host.document();
        let set = document.sheet_set();
        self.canvas.set_shown(Arc::clone(&set));
        self.rebuild_list(host);
        self.canvas.invalidate();
        self.rebuild_properties();
        let count = set.sheets.len();
        let mut text = format!("{count} sheet{}", if count == 1 { "" } else { "s" });
        if let Err(error) = document.sheet_set_status() {
            text.push_str(&format!("  -  {}", error.describe()));
        }
        self.count_text = text;
        self.checks.schedule(self.shown);
    }

    fn rebuild_list(&mut self, host: &dyn Host) {
        let set = host.document().sheet_set();
        let keep = self
            .canvas
            .sheet()
            .min(set.sheets.len().saturating_sub(1));
        if !set.sheets.is_empty() {
            self.list.set_current_row(Some(keep));
        } else {
            self.list.set_current_row(None);
        }
        self.canvas.set_sheet(keep);
        self.refresh_thumbnails(host);
    }

    /// The text of the list's row `index`: its number and name, and its paper.
    pub fn list_text(set: &SheetSet, index: usize) -> String {
        let sheet = &set.sheets[index];
        let number = format_sheet_number(
            &set.numbering,
            index + 1,
            set.sheets.len(),
            &set.defaults.set_number,
        );
        format!(
            "{number}   {}\n{} {}",
            sheet.name,
            paper_name(sheet.paper),
            if sheet.landscape {
                "landscape"
            } else {
                "portrait"
            }
        )
    }

    /// The properties drawn again from the document at the next frame.
    pub(crate) fn rebuild_properties(&mut self) {
        self.rebuilds += 1;
    }

    pub fn set_current_sheet(&mut self, host: &dyn Host, index: usize) {
        self.observe(host);
        if index < host.document().sheet_set().sheets.len() {
            self.list.set_current_row(Some(index));
            self.canvas.set_sheet(index);
            self.rebuild_properties();
        }
    }

    /// `text` on the status bar and into the window's log, as an error when `error`: what the
    /// editor's menus say (the C++'s report, whose onMessage the window logged).
    pub fn report(&mut self, host: &mut dyn Host, text: &str, error: bool) {
        self.status = String::from(text);
        host.log(text, error);
    }

    /// `line` through the window's executor when there is one (set_command_runner); with none,
    /// through the interpreter's own sheet verbs (cad's run_sheet_verb) with
    /// sheet_verb_context_for this editor's source, the line and what it replied logged as the
    /// window's log would show them. The reply's first line, or the error, goes on the status bar.
    pub fn run_line(&mut self, host: &mut dyn Host, line: &str) -> crate::host::VerbOutcome {
        let outcome = if self.runner {
            // The window echoes the line and logs the reply itself.
            host.run(line)
        } else {
            host.log(&format!("> {line}"), false);
            let source = Rc::clone(&self.source);
            let provider = move |document: &Document| sheet_verb_context_for(document, &source);
            let reply = CommandInterpreter::tokenize(line)
                .and_then(|tokens| run_sheet_verb(host.document_mut(), &tokens, Some(&provider)));
            let outcome = match reply {
                Ok(reply) => crate::host::VerbOutcome::new(true, &reply, ""),
                Err(error) => crate::host::VerbOutcome::new(false, "", &error.describe()),
            };
            if outcome.ok {
                host.log(&outcome.reply, false);
            } else {
                host.log(&outcome.error, true);
            }
            outcome
        };
        self.status = String::from(first_line(if outcome.ok {
            &outcome.reply
        } else {
            &outcome.error
        }));
        if !outcome.ok {
            // A refused line changed nothing, so nothing redraws the panel: the field that was
            // typed into shows the stored value again.
            self.fields.clear();
            self.rebuild_properties();
        }
        self.observe(&*host);
        outcome
    }

    // ---- the edits the toolbar makes; each one undoable step ------------------------------

    /// A new blank sheet after the current one, shown.
    pub fn add_blank_sheet(&mut self, host: &mut dyn Host) -> Status {
        self.observe(&*host);
        let set = host.document().sheet_set();
        let mut sheet = blank_sheet(
            &SheetTemplate::default(),
            format!("SHEET {}", set.sheets.len() + 1),
        );
        sheet.id = next_sheet_id(&set);
        let at = if set.sheets.is_empty() {
            0
        } else {
            self.current_sheet() + 1
        };
        add_sheet(host.document_mut(), sheet, Some(at))?;
        self.observe(&*host);
        self.set_current_sheet(&*host, at);
        Ok(())
    }

    /// A viewport of `kind` on the current sheet, placed in the free drawing area, with sensible
    /// defaults (a plan fitted to the drawing, a section along the first alignment); selected. A
    /// new sheet first when there is none.
    pub fn add_viewport(&mut self, host: &mut dyn Host, kind: ViewportKind) -> Status {
        self.observe(&*host);
        if host.document().sheet_set().sheets.is_empty() {
            self.add_blank_sheet(host)?;
        }
        let set = host.document().sheet_set();
        let index = self.current_sheet();
        let Some(sheet) = set.sheets.get(index) else {
            return Ok(());
        };
        let mut v = Viewport {
            id: next_viewport_id(&set),
            kind,
            ..Viewport::default()
        };
        let alignments = host.document().model().alignments.names();
        match kind {
            ViewportKind::Plan => {
                v.rect = free_place(sheet, 200.0, 140.0);
                v.auto_scale = true;
                v.auto_centre = true;
                v.north_arrow = true;
                v.scale_bar = true;
            }
            ViewportKind::KeyPlan => {
                // Its outlines are the sheets' plans as they are when it is drawn (key_plan), numbered
                // as the set numbers them then; nothing is stored to go stale.
                v.rect = free_place(sheet, 90.0, 70.0);
                v.auto_scale = true;
                v.auto_centre = true;
                v.north_arrow = true;
            }
            ViewportKind::LongSection => {
                v.rect = free_place(sheet, 380.0, 110.0);
                v.scale = 500.0;
                v.vertical_exaggeration = 10.0;
                // The scale and exaggeration fitted to what it shows.
                v.auto_scale = true;
                v.auto_centre = true;
                if let Some(first) = alignments.first() {
                    v.source.alignment.clone_from(first);
                }
            }
            ViewportKind::CrossSections => {
                v.rect = free_place(sheet, 180.0, 110.0);
                v.scale = 200.0;
                v.auto_scale = true;
                v.auto_centre = true;
                v.source.section_half_width = 20.0;
                if let Some(first) = alignments.first() {
                    v.source.alignment.clone_from(first);
                    if let Some(a) = host.document().model().alignments.find(first)
                        && let Ok(solved) = solve_alignment(&a.horizontal)
                    {
                        v.source.stations =
                            vec![(solved.start_station() + solved.end_station()) / 2.0];
                    }
                }
            }
            ViewportKind::Model3D => v.rect = free_place(sheet, 160.0, 110.0),
            ViewportKind::Legend => v.rect = free_place(sheet, 80.0, 100.0),
            ViewportKind::Notes => {
                v.rect = free_place(sheet, 90.0, 80.0);
                v.text = String::from("1. ALL DIMENSIONS ARE IN METRES UNLESS NOTED OTHERWISE.");
            }
            ViewportKind::SheetIndex => v.rect = free_place(sheet, 200.0, 150.0),
            ViewportKind::Revisions => v.rect = free_place(sheet, 130.0, 60.0),
            ViewportKind::Image => {
                v.rect = free_place(sheet, 100.0, 80.0);
                let assets = self.source(host.document()).assets;
                let chosen = self.ask_file(&FileAsk {
                    save: false,
                    title: String::from("Image for the Sheet"),
                    start: assets.to_string_lossy().into_owned(),
                    filter: String::from("Images (*.png *.jpg *.jpeg *.bmp *.gif)"),
                });
                if chosen.is_empty() {
                    return Ok(());
                }
                let chosen = std::path::PathBuf::from(chosen);
                v.text = chosen
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let folder = chosen.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
                if !assets.as_os_str().is_empty() && folder != assets {
                    let copied = std::fs::create_dir_all(&assets)
                        .and_then(|()| std::fs::copy(&chosen, assets.join(&v.text)));
                    if let Err(error) = copied {
                        return Err(make_error(
                            ErrorCode::FileExportFailure,
                            "could not copy the image into the project",
                            format!("{error}"),
                        ));
                    }
                }
            }
        }
        let id = v.id.clone();
        edit_sheet(
            host.document_mut(),
            index,
            |edited| {
                edited.viewports.push(v);
                Ok(())
            },
            "ADD_VIEWPORT",
        )?;
        self.observe(&*host);
        self.canvas.select(&id);
        Ok(())
    }

    /// Every selected viewport (cad's remove_viewports).
    pub fn remove_selected_viewport(&mut self, host: &mut dyn Host) -> Status {
        self.observe(&*host);
        let ids = self.canvas.selected_ids();
        if ids.is_empty() {
            return Ok(());
        }
        let step = if ids.len() == 1 {
            "REMOVE_VIEWPORT"
        } else {
            "REMOVE_VIEWPORTS"
        };
        let status = remove_viewports(host.document_mut(), self.current_sheet(), &ids, step);
        self.observe(&*host);
        status
    }

    pub fn tile(&mut self, host: &mut dyn Host, preset: TilingPreset) -> Status {
        self.observe(&*host);
        if host.document().sheet_set().sheets.is_empty() {
            return Err(katana_core::Error::new(
                ErrorCode::InvalidState,
                "there is no sheet to tile",
            ));
        }
        let mut placed = 0;
        let status = edit_sheet(
            host.document_mut(),
            self.current_sheet(),
            |sheet| {
                placed = tile_viewports(sheet, preset, TILING_GUTTER_MM);
                Ok(())
            },
            "TILE_VIEWPORTS",
        );
        self.observe(&*host);
        if status.is_ok() {
            self.report(
                host,
                &format!("Tiled {placed} view{}.", if placed == 1 { "" } else { "s" }),
                false,
            );
        }
        status
    }

    /// Plots the current sheet, or every sheet, to one PDF at `path` in the set's page setup (its
    /// plot style and resolution). Problems are reported, and so are the errors the checks find
    /// on the sheets plotted (the Checks dock is shown), which never stop it.
    pub fn plot_to_pdf(&mut self, host: &mut dyn Host, path: &str, all_sheets: bool) -> Status {
        self.observe(&*host);
        let set = host.document().sheet_set();
        if !all_sheets && self.current_sheet() >= set.sheets.len() {
            return Err(katana_core::Error::new(
                ErrorCode::InvalidArgument,
                "there is no sheet to plot",
            ));
        }
        // The checks first. What they find on these sheets is reported with the plot, and the
        // Checks dock is brought up; the plot still goes ahead - an error there is paper wasted,
        // not a file that cannot be written.
        let errors = self.check_before_plot(&*host, all_sheets);
        // One PDF in the set's plot style (plotting::plot_output).
        let sheets = if all_sheets {
            String::new()
        } else {
            format!("{}", self.current_sheet() + 1)
        };
        let mut request = plot_request_for(&set.page_setup, path, &sheets);
        request.format = PlotFormat::Pdf;
        request.title.clone_from(&host.document().metadata().name);
        let mut cache = SheetPaintCache::new();
        let report = {
            let source = self.source(host.document());
            plot_sheets_from(&set, &request, &source, &mut cache, None)?
        };
        for problem in &report.problems {
            self.report(host, problem, true);
        }
        let mut done = report.summary(&request);
        if errors > 0 {
            done.push_str(&format!(
                " The checks found {errors} error{} on {}: the Checks panel lists them.",
                if errors == 1 { "" } else { "s" },
                if all_sheets { "the sheets" } else { "it" }
            ));
        }
        self.report(host, &done, false);
        Ok(())
    }

    /// Runs the checks on the sheets a plot takes (the current one, or all), shows the Checks dock
    /// when they find an error, and says how many.
    pub(crate) fn check_before_plot(&mut self, host: &dyn Host, all_sheets: bool) -> usize {
        let count = host.document().sheet_set().sheets.len();
        let mut pages = Vec::new();
        if !all_sheets && self.current_sheet() < count {
            pages.push(self.current_sheet());
        }
        let findings = self.check_now(host).to_vec();
        let errors = summarize(&findings_on_sheets(&findings, &pages)).errors;
        if errors > 0 {
            self.checks.show();
        }
        errors
    }

    /// The Plot dialog for the current sheet or every sheet, the checks run first: the dock comes
    /// up over the dialog when they find an error, so it is seen before the paper is.
    pub(crate) fn plot_interactive(&mut self, host: &mut dyn Host, all_sheets: bool) {
        let _ = self.check_before_plot(&*host, all_sheets);
        let set = host.document().sheet_set();
        let suggested = suggested_plot_file(host.document());
        let mut said: Vec<(String, bool)> = Vec::new();
        self.plot_dialog = plot_interactively(
            &set,
            self.current_sheet(),
            all_sheets,
            &suggested,
            &mut |text: &str, error: bool| said.push((String::from(text), error)),
        );
        for (text, error) in said {
            self.report(host, &text, error);
        }
    }

    // ---- Generate Sheets --------------------------------------------------------------------

    /// Generate Sheets opened (the toolbar's), not waited on: a headless session fills it by its
    /// object names (sheetGenerateDialog).
    pub fn open_generate_dialog(&mut self, host: &dyn Host) -> &mut GenerateDialog {
        self.observe(host);
        let document = host.document();
        let mut extent = plan_drawn_bounds(
            &self.source(document).plan,
            &Default::default(),
            &BTreeSet::new(),
        );
        if extent.empty() {
            extent = Box2::new(Point2::new(0.0, 0.0), Point2::new(100.0, 100.0));
        }
        let plan_view = self.plan_view_area.as_ref().and_then(|area| area());
        self.generate.insert(GenerateDialog::new(
            extent,
            document.model().alignments.names(),
            plan_view,
        ))
    }

    /// Generate Sheets, run: the C++ waited for it (exec); here it is opened as the toolbar opens
    /// it, and OK runs its line at the frame it is pressed.
    pub fn generate_sheets(&mut self, host: &dyn Host) -> &mut GenerateDialog {
        self.open_generate_dialog(host)
    }

    /// Generate Sheets, when it is open.
    pub fn generate_dialog(&mut self) -> Option<&mut GenerateDialog> {
        self.generate.as_mut()
    }

    fn accept_generate(
        &mut self,
        host: &mut dyn Host,
        line: katana_core::Result<String>,
        cover: bool,
        replace: bool,
    ) {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                self.report(host, &error.describe(), true);
                return;
            }
        };
        // Where the new sheets go: a register in front, the rest after the sheets there are, or in
        // their place.
        let first = if cover || replace {
            0
        } else {
            host.document().sheet_set().sheets.len()
        };
        if self.run_line(host, &line).ok && first < host.document().sheet_set().sheets.len() {
            self.set_current_sheet(&*host, first);
            self.canvas.fit_page();
        }
    }

    // ---- the title block -------------------------------------------------------------------

    /// The Title Block dialog opened (sheetTitleBlockDialog).
    pub fn edit_title_block(&mut self, host: &dyn Host) {
        self.observe(host);
        let set = host.document().sheet_set();
        let fields = self.source(host.document()).fields;
        self.title_block = Some(TitleBlockDialog::new(&set, &fields));
    }

    pub fn title_block_dialog(&mut self) -> Option<&mut TitleBlockDialog> {
        self.title_block.as_mut()
    }

    fn accept_title_block(&mut self, host: &mut dyn Host, dialog: &TitleBlockDialog) {
        // The logo is stored by its own step; everything else in one more.
        let logo = host.document().sheet_set().defaults.logo_asset.clone();
        let text = |value: &str| String::from(value.trim());
        let mut project_lines: Vec<String> =
            dialog.project_lines.iter().map(|line| text(line)).collect();
        while project_lines.last().is_some_and(String::is_empty) {
            project_lines.pop();
        }
        let sign_offs: Vec<SignOff> = dialog
            .sign_offs
            .iter()
            .map(|(name, date)| SignOff {
                name: text(name),
                date: text(date),
            })
            .collect();
        let revisions: Vec<Revision> = dialog
            .revisions
            .iter()
            .filter(|row| !row[0].trim().is_empty() || !row[2].trim().is_empty())
            .map(|row| Revision {
                code: text(&row[0]),
                date: text(&row[1]),
                description: text(&row[2]),
                by: text(&row[3]),
            })
            .collect();
        let status = edit_sheet_set(
            host.document_mut(),
            |s| {
                let e = &mut s.defaults;
                e.organisation = text(&dialog.organisation);
                e.project_lines = project_lines;
                e.client = text(&dialog.client);
                e.set_number = text(&dialog.set_number);
                e.coordinate_system = text(&dialog.coordinates);
                e.height_datum = text(&dialog.datum);
                e.model_name = text(&dialog.model_name);
                e.notes = text(&dialog.notes);
                e.locator = sign_offs[0].clone();
                e.surveyor = sign_offs[1].clone();
                e.compiler = sign_offs[2].clone();
                e.reviewer = sign_offs[3].clone();
                e.approver = sign_offs[4].clone();
                e.logo_asset = logo;
                s.numbering = if dialog.numbering.trim().is_empty() {
                    String::from("{n}")
                } else {
                    text(&dialog.numbering)
                };
                s.revisions = revisions;
                Ok(())
            },
            "TITLE_BLOCK",
        );
        match status {
            Err(error) => self.report(host, &error.describe(), true),
            Ok(()) if dialog.logo_changed => self.canvas.invalidate(),
            Ok(()) => {}
        }
        self.observe(&*host);
    }

    fn choose_logo(&mut self, host: &mut dyn Host) {
        if self.headless {
            self.report(
                host,
                "Choose Logo opens no file dialog in a headless session: type TITLEBLOCK LOGO \"path\"",
                true,
            );
            return;
        }
        let file = self.ask_file(&FileAsk {
            save: false,
            title: String::from("Logo"),
            start: String::new(),
            filter: String::from("Images (*.png *.jpg *.jpeg *.gif *.bmp)"),
        });
        if file.is_empty() {
            return;
        }
        match import_logo(host.document_mut(), std::path::Path::new(&file)) {
            Ok(_) => {
                if let Some(dialog) = &mut self.title_block {
                    dialog.logo_changed = true;
                }
            }
            Err(error) => host.warn_user("Logo", &error.describe()),
        }
        self.observe(&*host);
    }

    fn clear_logo(&mut self, host: &mut dyn Host) {
        let _ = edit_sheet_set(
            host.document_mut(),
            |s| {
                s.defaults.logo_asset.clear();
                Ok(())
            },
            "SHEET_LOGO",
        );
        if let Some(dialog) = &mut self.title_block {
            dialog.logo_changed = true;
        }
        self.observe(&*host);
    }

    // ---- the hidden layers -----------------------------------------------------------------

    /// Hidden layers... for the view `id`: the layers it hides, ticked off, run as VIEW SET id
    /// hide= show= for what changed.
    pub fn choose_hidden_layers(&mut self, host: &dyn Host, id: &str) {
        let set = host.document().sheet_set();
        let Some(viewport) = set
            .sheets
            .iter()
            .flat_map(|sheet| &sheet.viewports)
            .find(|viewport| viewport.id == id)
        else {
            return;
        };
        let layers = host
            .document()
            .model()
            .layers
            .all()
            .into_iter()
            .map(|layer| {
                let hidden = viewport.hidden_layers.hides_directly(&layer.name);
                (layer.name, hidden, !hidden)
            })
            .collect();
        self.hidden_layers = Some(HiddenLayersDialog {
            frame: crate::shell::DialogFrame::new(
                "sheetHiddenLayersDialog",
                "Layers Shown in This View",
            )
            .deletes_on_close(true),
            id: String::from(id),
            layers,
        });
    }

    pub fn hidden_layers_dialog(&mut self) -> Option<&mut HiddenLayersDialog> {
        self.hidden_layers.as_mut()
    }

    // ---- the system's dialogs --------------------------------------------------------------

    /// The system's file dialog, or the answer a test gives in its place.
    pub(crate) fn ask_file(&mut self, ask: &FileAsk) -> String {
        match self.choose_file.as_mut() {
            Some(choose) => choose(ask),
            None if ask.save => get_save_file_name(&ask.title, &ask.start, &ask.filter),
            None => get_open_file_name(&ask.title, &ask.start, &ask.filter),
        }
    }

    /// A question put, or the answer a test gives in its place; a box when neither (rfd's).
    pub(crate) fn ask(&mut self, ask: &QuestionAsk) -> bool {
        match self.answer_question.as_mut() {
            Some(answer) => answer(ask),
            None => {
                rfd::MessageDialog::new()
                    .set_title(&ask.title)
                    .set_description(&ask.text)
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    == rfd::MessageDialogResult::Yes
            }
        }
    }
}

/// The page `index` of a tabbed dialog whose page `front` is in front: drawn always, shown only
/// when it is in front, so a driver can fill a control on a page behind (docs/rust.md, wave 1).
fn page(ui: &mut Ui, index: usize, front: usize, add: impl FnOnce(&mut Ui)) {
    if index == front {
        ui.scope(add);
    } else {
        // Drawn where the page in front is and clipped away, taking no room: an invisible page
        // drawn in its turn took its height, so the Sign-offs page was drawn under the Project
        // page's empty room and the dialog was as tall as all its pages. Still invisible, so a
        // press of a button on a page behind is refused as it was.
        offstage(ui, &format!("sheetTitleBlockPage{index}"), |ui| {
            ui.set_invisible();
            add(ui);
        });
    }
}

// ---- the panel's fields --------------------------------------------------------------------------

impl PanelFields {
    /// A number box over `model`: Some(value) once typing into it is done (its editingFinished)
    /// with a value other than what it showed when it was built.
    fn number(&mut self, ui: &mut Ui, name: &str, model: f64, range: SpinRange) -> Option<f64> {
        let field = self.numbers.entry(String::from(name)).or_insert_with(|| {
            let shown = range.rounded(model);
            NumberField {
                shown,
                value: shown,
                typing: false,
            }
        });
        let response = controls::spin(ui, name, &mut field.value, range);
        if response.changed() {
            field.typing = true;
        }
        // Handed over when the typing is done: the box has let go of the keyboard and the pointer.
        if field.typing && !response.has_focus() && !response.dragged() {
            field.typing = false;
            if field.value != field.shown {
                return Some(field.value);
            }
        }
        None
    }

    /// The value number box `name` holds now.
    fn number_now(&self, name: &str) -> Option<f64> {
        self.numbers.get(name).map(|field| field.value)
    }

    fn shown(&self, name: &str) -> Option<f64> {
        self.numbers.get(name).map(|field| field.shown)
    }

    /// A one-line field over `model`: Some(text) once editing it is done with a change.
    fn line(&mut self, ui: &mut Ui, name: &str, model: &str, placeholder: &str) -> Option<String> {
        let value = self
            .texts
            .entry(String::from(name))
            .or_insert_with(|| String::from(model));
        let response = controls::line_with(
            ui,
            name,
            value,
            LineOptions {
                placeholder,
                ..LineOptions::default()
            },
        );
        if response.changed() {
            self.typing.insert(String::from(name));
        }
        if self.typing.contains(name) && !response.has_focus() {
            self.typing.remove(name);
            return Some(value.clone());
        }
        None
    }

    /// A choice over `items`: Some(text) when one is chosen, or one typed is done.
    fn choice(
        &mut self,
        ui: &mut Ui,
        name: &str,
        items: &[String],
        start: Choice,
    ) -> (Option<String>, egui::Response) {
        let chosen = self.choices.entry(String::from(name)).or_insert(start);
        let response = controls::choice(ui, name, items, chosen);
        if response.changed() {
            self.typing.insert(String::from(name));
        }
        if self.typing.contains(name) && !response.has_focus() {
            self.typing.remove(name);
            return (Some(chosen.current_text(items)), response);
        }
        (None, response)
    }

    fn check(&mut self, ui: &mut Ui, name: &str, text: &str, model: bool) -> Option<bool> {
        let mut on = model;
        controls::check(ui, name, text, &mut on).changed().then_some(on)
    }
}

/// A QSpinBox with a special value text: its minimum shows as `special` ("All"), as Qt shows it,
/// and a fill of that text chooses the minimum. One step when the typing is done.
fn whole_with_special(
    ui: &mut Ui,
    name: &str,
    value: &mut i64,
    range: std::ops::RangeInclusive<i64>,
    special: &str,
) -> egui::Response {
    let taken = take(ui, name);
    let mut changed = false;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                let number = if text.trim() == special {
                    Some(*range.start())
                } else {
                    katana_core::text::parse_integer(text.trim())
                };
                match number {
                    Some(number) if range.contains(&number) => {
                        changed = *value != number;
                        *value = number;
                        settle(ui, taken, Ok(()));
                    }
                    _ => settle(
                        ui,
                        taken,
                        Err(whole_out_of_range(name, *range.start(), *range.end(), text)),
                    ),
                }
            }
            Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
            Act::ScrollTo => settle(ui, taken, Ok(())),
        }
    }
    let minimum = *range.start();
    let label = String::from(special);
    let mut response = ui.add(
        egui::DragValue::new(value)
            .range(range)
            .custom_formatter(move |number, _| {
                if number as i64 == minimum {
                    label.clone()
                } else {
                    format!("{number}")
                }
            }),
    );
    if changed {
        response.mark_changed();
    }
    let mut record = ControlRecord::new(name, ControlKind::Whole);
    record.value = if *value == minimum {
        String::from(special)
    } else {
        format!("{value}")
    };
    controls::custom(ui, &response, record);
    response
}

impl SheetEditor {
    fn panel_key(&self) -> (u64, u64, usize, u64) {
        (
            self.seen.unwrap_or(0),
            self.canvas.selection_serial,
            self.canvas.sheet(),
            self.rebuilds,
        )
    }

    /// The properties: the selected viewport's, else the sheet's (rebuildProperties, drawn each
    /// frame from the document; the fields keep what is typed until the panel is built again).
    fn properties_ui(&mut self, ui: &mut Ui, host: &dyn Host) {
        let key = self.panel_key();
        if self.fields.built_for != Some(key) {
            self.fields.clear();
            self.fields.built_for = Some(key);
        }
        self.fields.tips.clear();
        let document = host.document();
        let set = document.sheet_set();
        let index = self.current_sheet();
        let Some(sheet) = set.sheets.get(index) else {
            ui.label("No sheet.");
            return;
        };
        let selected = sheet
            .viewports
            .iter()
            .find(|v| v.id == self.canvas.selected())
            .cloned();
        match selected {
            Some(v) => self.viewport_properties(ui, document, &set, index, &v),
            None => self.sheet_properties(ui, document, &set, index),
        }
    }

    fn edit(&mut self, id: &str, step: &'static str, change: impl FnOnce(&mut Viewport) + 'static) {
        self.queued.push(Queued::EditViewport {
            id: String::from(id),
            step,
            change: Box::new(change),
        });
    }

    /// The fields added for the verbs run the VIEW SET line a person would type, after the frame:
    /// the line rebuilds this panel, and with it the field that asked.
    fn view_set(&mut self, id: &str, options: &str) {
        self.queued
            .push(Queued::Line(format!("VIEW SET {id} {options}")));
    }

    #[allow(
        clippy::too_many_lines,
        reason = "One form, as the C++'s rebuildProperties builds it, field by field."
    )]
    fn viewport_properties(
        &mut self,
        ui: &mut Ui,
        document: &Document,
        set: &SheetSet,
        index: usize,
        v: &Viewport,
    ) {
        let id = v.id.clone();
        let count = self.canvas.selected_ids().len();
        if count > 1 {
            // Short lines, wrapped: the panel may be narrow.
            let note = format!("{count} views selected.\nThe properties are {id}'s.");
            controls::label_as(
                ui,
                "sheetSelectionNote",
                &note,
                egui::Label::new(note.as_str()).wrap(),
            )
            .on_hover_text(
                "A drag, the arrows, Delete, Cut, Copy and Duplicate act on every selected view",
            );
        }
        let source = self.source(document);
        let plan = v.kind == ViewportKind::Plan || v.kind == ViewportKind::KeyPlan;
        let section = v.kind == ViewportKind::LongSection || v.kind == ViewportKind::CrossSections;
        let cross = v.kind == ViewportKind::CrossSections;
        ui.strong(format!("{}  ({})", kind_name(v.kind), v.id));
        dialog::form(ui, |form| {
            let ui = form.ui();
            dialog::labelled(ui, "Title", |ui| {
                if let Some(title) = self.fields.line(ui, "sheetViewportTitle", &v.title, &automatic_title(v))
                    && title != v.title
                {
                    self.edit(&id, "EDIT_VIEWPORT", move |e| e.title = title);
                }
            });

            // An automatic section's exaggeration is the fitted one, shown as such and kept when a
            // fixed scale is chosen, so choosing one does not redraw the section at the stale
            // stored exaggeration.
            let mut fitted_exaggeration: Option<f64> = None;
            if plan || section {
                dialog::labelled(ui, if section { "Horizontal scale" } else { "Scale" }, |ui| {
                    let items = scale_items(true);
                    let automatic = Choice {
                        index: Some(0),
                        text: String::from("Auto"),
                        editable: true,
                    };
                    let (start, tip) = if plan && v.auto_scale {
                        let resolved = resolve_plan_viewport_on_sheet(v, &source, set, index);
                        (automatic, Some(format!("Automatic: now {}", scale_label(resolved.scale))))
                    } else if v.auto_scale {
                        // A section fits both its scale and its exaggeration.
                        let fit: SectionFit =
                            resolve_section_viewport(v, &source, self.canvas.paint_cache());
                        let mut fitted = v.clone();
                        fitted.scale = fit.scale;
                        fitted.vertical_exaggeration = fit.exaggeration;
                        fitted_exaggeration = Some(fit.exaggeration);
                        (automatic, Some(format!("Automatic: now {}", scale_text(&fitted))))
                    } else {
                        let label = scale_label(v.scale);
                        (
                            Choice {
                                index: items.iter().position(|item| *item == label),
                                text: label,
                                editable: true,
                            },
                            None,
                        )
                    };
                    let (chosen, response) = self.fields.choice(ui, "sheetViewportScale", &items, start);
                    if let Some(tip) = &tip {
                        response.on_hover_text(tip.as_str());
                        self.fields
                            .tips
                            .insert(String::from("sheetViewportScale"), tip.clone());
                    }
                    if let Some(text) = chosen {
                        if text == "Auto" {
                            self.edit(&id, "EDIT_VIEWPORT", |e| e.auto_scale = true);
                        } else if let Some(value) = parse_scale(&text) {
                            self.edit(&id, "EDIT_VIEWPORT", move |e| {
                                e.scale = value;
                                if e.auto_scale
                                    && let Some(fitted) = fitted_exaggeration
                                {
                                    e.vertical_exaggeration = fitted;
                                }
                                e.auto_scale = false;
                            });
                        } else {
                            self.fields.choices.remove("sheetViewportScale");
                        }
                    }
                });
            }
            if plan || v.kind == ViewportKind::Model3D {
                dialog::labelled(ui, "Rotation", |ui| {
                    if let Some(degrees) = self.fields.number(
                        ui,
                        "sheetViewportRotation",
                        v.rotation * RAD_TO_DEG,
                        SpinRange::new(-360.0, 360.0, 1.0, 3).with_suffix(" \u{b0}"),
                    ) {
                        self.edit(&id, "EDIT_VIEWPORT", move |e| e.rotation = degrees * DEG_TO_RAD);
                    }
                });
            }
            if plan || section {
                ui.scope(|ui| {
                    if let Some(on) =
                        self.fields
                            .check(ui, "sheetViewportAutoCentre", "Centre on what it shows", v.auto_centre)
                    {
                        self.edit(&id, "EDIT_VIEWPORT", move |e| e.auto_centre = on);
                    }
                });
                dialog::labelled(ui, if plan { "Centre E" } else { "Centre (ch/offset)" }, |ui| {
                    let x = ui
                        .add_enabled_ui(!v.auto_centre, |ui| {
                            self.fields
                                .number(ui, "sheetViewportCentreX", v.centre.x, SpinRange::new(-1e9, 1e9, 1.0, 3))
                        })
                        .inner;
                    if let Some(x) = x {
                        self.edit(&id, "EDIT_VIEWPORT", move |e| e.centre.x = x);
                    }
                });
                dialog::labelled(ui, if plan { "Centre N" } else { "Centre level" }, |ui| {
                    let y = ui
                        .add_enabled_ui(!v.auto_centre, |ui| {
                            self.fields
                                .number(ui, "sheetViewportCentreY", v.centre.y, SpinRange::new(-1e9, 1e9, 1.0, 3))
                        })
                        .inner;
                    if let Some(y) = y {
                        self.edit(&id, "EDIT_VIEWPORT", move |e| e.centre.y = y);
                    }
                });
            }
            if section {
                dialog::labelled(ui, "Vertical exaggeration", |ui| {
                    let ve = ui
                        .add_enabled_ui(!v.auto_scale, |ui| {
                            self.fields.number(
                                ui,
                                "sheetViewportExaggeration",
                                fitted_exaggeration.unwrap_or(v.vertical_exaggeration),
                                SpinRange::new(0.1, 100.0, 1.0, 2).with_suffix(" x"),
                            )
                        })
                        .inner;
                    if v.auto_scale {
                        self.fields.tips.insert(
                            String::from("sheetViewportExaggeration"),
                            String::from("Chosen with the automatic scale"),
                        );
                    }
                    if let Some(x) = ve {
                        self.edit(&id, "EDIT_VIEWPORT", move |e| e.vertical_exaggeration = x);
                    }
                });
            }
            if section || plan {
                dialog::labelled(ui, "Alignment", |ui| {
                    let mut items = vec![String::from(if plan { "(the drawing)" } else { "(none)" })];
                    items.extend(document.model().alignments.names());
                    let start = Choice::at(
                        items
                            .iter()
                            .skip(1)
                            .position(|name| *name == v.source.alignment)
                            .map_or(0, |at| at + 1),
                    );
                    let (chosen, _) = self.fields.choice(ui, "sheetViewportAlignment", &items, start);
                    if chosen.is_some() {
                        let at = self
                            .fields
                            .choices
                            .get("sheetViewportAlignment")
                            .and_then(|choice| choice.index)
                            .unwrap_or(0);
                        let name = if at == 0 {
                            String::new()
                        } else {
                            items[at].clone()
                        };
                        self.edit(&id, "EDIT_VIEWPORT", move |e| e.source.alignment = name);
                    }
                });
            }
            if v.kind == ViewportKind::LongSection || cross || (plan && !v.source.alignment.is_empty()) {
                let (from_tip, to_tip) = if cross {
                    ("The chainages cut every interval run from here", "...to here")
                } else {
                    (
                        "From equal to To: the whole alignment",
                        "From equal to To: the whole alignment",
                    )
                };
                dialog::labelled(ui, "Chainage from", |ui| {
                    // What the box shows, rounded: a box left as it was writes nothing.
                    if let Some(from) = self.fields.number(
                        ui,
                        "sheetChainageFrom",
                        v.source.chainage_from,
                        SpinRange::new(-1e9, 1e9, 1.0, 3),
                    ) {
                        self.view_set(&id, &format!("from={}", verb_number(from)));
                    }
                    self.fields
                        .tips
                        .insert(String::from("sheetChainageFrom"), String::from(from_tip));
                });
                dialog::labelled(ui, "Chainage to", |ui| {
                    if let Some(to) = self.fields.number(
                        ui,
                        "sheetChainageTo",
                        v.source.chainage_to,
                        SpinRange::new(-1e9, 1e9, 1.0, 3),
                    ) {
                        self.view_set(&id, &format!("to={}", verb_number(to)));
                    }
                    self.fields
                        .tips
                        .insert(String::from("sheetChainageTo"), String::from(to_tip));
                });
            }
            if cross {
                // Cut at the chainages listed, or every so many metres from one chainage to
                // another: the painter cuts the list when there is one (cad's viewport_stations),
                // so Every clears it, and cuts an interval only over a range, so Every with none
                // sets the alignment's whole length.
                let stations: Vec<String> =
                    v.source.stations.iter().map(|s| qt_fixed(*s, 3)).collect();
                let by_interval = v.source.stations.is_empty() && v.source.section_interval > 0.0;
                let listed_text = stations.join(", ");
                // Each way of cutting is its row's label, a radio button, with its field beside
                // it; what the button and the field ask is done once the row is drawn, the
                // button's first, as it was.
                let mut at_clicked = false;
                let typed = dialog::row_with_check(
                    ui,
                    "At chainages",
                    |ui| {
                        at_clicked =
                            controls::radio(ui, "sheetSectionsAtChainages", "At chainages", !by_interval)
                                .clicked();
                    },
                    |ui| {
                        ui.add_enabled_ui(!by_interval, |ui| {
                            self.fields
                                .line(ui, "sheetSectionStations", &listed_text, "e.g. 20, 40, 60")
                        })
                        .inner
                    },
                );
                if at_clicked && by_interval {
                    self.listed(&id, &listed_text);
                }
                if let Some(typed) = typed
                    && typed != listed_text
                {
                    self.listed(&id, &typed);
                }
                let whole = document
                    .model()
                    .alignments
                    .find(&v.source.alignment)
                    .and_then(|a| solve_alignment(&a.horizontal).ok())
                    .map(|solved| (solved.start_station(), solved.end_station()));
                let ranged = v.source.chainage_to > v.source.chainage_from;
                let mut every_clicked = false;
                let interval = dialog::row_with_check(
                    ui,
                    "Every",
                    |ui| {
                        every_clicked =
                            controls::radio(ui, "sheetSectionsEvery", "Every", by_interval).clicked();
                    },
                    |ui| {
                        ui.add_enabled_ui(by_interval, |ui| {
                            self.fields.number(
                                ui,
                                "sheetSectionInterval",
                                if by_interval {
                                    v.source.section_interval
                                } else {
                                    20.0
                                },
                                SpinRange::new(0.5, 10000.0, 1.0, 2).with_suffix(" m"),
                            )
                        })
                        .inner
                    },
                );
                if every_clicked && !by_interval {
                    let value = self.fields.number_now("sheetSectionInterval").unwrap_or(20.0);
                    let mut options = format!("stations=\"\" interval={}", verb_number(value));
                    if !ranged && let Some((start, end)) = whole {
                        options.push_str(&format!(
                            " from={} to={}",
                            verb_number(start),
                            verb_number(end)
                        ));
                    }
                    self.view_set(&id, &options);
                }
                if let Some(interval) = interval {
                    self.view_set(&id, &format!("interval={}", verb_number(interval)));
                }
                dialog::labelled(ui, "Half width", |ui| {
                    if let Some(w) = self.fields.number(
                        ui,
                        "sheetSectionHalfWidth",
                        if v.source.section_half_width > 0.0 {
                            v.source.section_half_width
                        } else {
                            20.0
                        },
                        SpinRange::new(0.5, 1000.0, 1.0, 2).with_suffix(" m"),
                    ) {
                        self.edit(&id, "EDIT_VIEWPORT", move |e| e.source.section_half_width = w);
                    }
                });
            }
            if v.kind == ViewportKind::Model3D {
                dialog::labelled(ui, "Camera height angle", |ui| {
                    if let Some(t) = self.fields.number(
                        ui,
                        "sheetViewportTilt",
                        v.tilt_degrees,
                        SpinRange::new(1.0, 89.0, 1.0, 1).with_suffix(" \u{b0}"),
                    ) {
                        self.edit(&id, "EDIT_VIEWPORT", move |e| e.tilt_degrees = t);
                    }
                });
            }
            if plan {
                ui.scope(|ui| {
                    if let Some(on) = self
                        .fields
                        .check(ui, "sheetViewportNorthArrow", "North arrow", v.north_arrow)
                    {
                        self.edit(&id, "EDIT_VIEWPORT", move |e| e.north_arrow = on);
                    }
                });
                ui.scope(|ui| {
                    if let Some(on) = self
                        .fields
                        .check(ui, "sheetViewportScaleBar", "Scale bar", v.scale_bar)
                    {
                        self.edit(&id, "EDIT_VIEWPORT", move |e| e.scale_bar = on);
                    }
                });
                let automatic = automatic_grid_interval(
                    resolve_plan_viewport_on_sheet(v, &source, set, index).scale,
                );
                self.fields.tips.insert(
                    String::from("sheetGridInterval"),
                    if automatic > 0.0 {
                        format!(
                            "The spacing in metres; Auto is about 50 mm apart on the paper - {} m now",
                            katana_core::text::format_general(automatic, 6)
                        )
                    } else {
                        String::from("The spacing in metres; Auto is about 50 mm apart on the paper")
                    },
                );
                // Once the list or box that asked has finished signalling: the edit rebuilds this
                // panel, them with it.
                if let Some((style, interval)) = self.fields.grid.show(ui, v, automatic) {
                    self.queued.push(Queued::PlanGrid(id.clone(), style, interval));
                }
            }
            // The painter leaves a section's hidden layers out as it does a plan's
            // (section_painter), so both offer the list.
            if plan || section {
                ui.scope(|ui| {
                    let text = format!("Hidden layers ({})...", v.hidden_layers.size());
                    if controls::button(ui, "sheetViewportHiddenLayers", &text).clicked() {
                        self.queued.push(Queued::HiddenLayers(id.clone()));
                    }
                });
            }
            if v.kind == ViewportKind::Notes {
                dialog::labelled(ui, "Text", |ui| {
                    let value = self
                        .fields
                        .texts
                        .entry(String::from("sheetViewportText"))
                        .or_insert_with(|| v.text.clone());
                    controls::text(ui, "sheetViewportText", value);
                });
                let typed = self
                    .fields
                    .texts
                    .get("sheetViewportText")
                    .cloned()
                    .unwrap_or_default();
                ui.scope(|ui| {
                    if controls::button(ui, "sheetViewportApplyText", "Apply text").clicked() {
                        self.edit(&id, "EDIT_VIEWPORT", move |e| e.text = typed);
                    }
                });
            }
            if v.kind == ViewportKind::Revisions {
                dialog::labelled(ui, "Newest revisions", |ui| {
                    // One step per value settled on, not per digit typed.
                    let mut limit = i64::try_from(v.revision_limit.min(999)).unwrap_or(999);
                    let response = whole_with_special(ui, "sheetRevisionLimit", &mut limit, 0..=999, "All")
                        .on_hover_text("Only the newest so many revisions; All shows every one");
                    if response.changed() && !response.has_focus() && !response.dragged() {
                        let n = usize::try_from(limit).unwrap_or(0);
                        self.edit(&id, "VIEWPORT_REVISION_LIMIT", move |e| e.revision_limit = n);
                    }
                });
            }
            if v.kind == ViewportKind::Image {
                dialog::labelled(ui, "Image", |ui| {
                    ui.horizontal(|ui| {
                        if let Some(file) = self.fields.line(
                            ui,
                            "sheetImageName",
                            &v.text,
                            "a file in the project's assets folder",
                        ) && file != v.text
                        {
                            self.edit(&id, "EDIT_VIEWPORT", move |e| e.text = file);
                        }
                        // Another picture: copied into the project's assets as VIEW SET file= copies it
                        // (cad's import_image_asset, up to 32 MB), and named in the same step.
                        if controls::button(ui, "sheetImageBrowse", "Browse...")
                            .on_hover_text("Choose the picture: it is copied into the project's assets")
                            .clicked()
                        {
                            self.queued.push(Queued::BrowseImage(id.clone()));
                        }
                    });
                });
            }
            if v.kind == ViewportKind::Legend
                && let Some(scope) = add_legend_properties(ui, set, index, v, &source)
            {
                self.queued.push(Queued::LegendScope(id.clone(), scope));
            }
            ui.scope(|ui| {
                if let Some(on) = self.fields.check(
                    ui,
                    "sheetViewportLocked",
                    "Locked (tiling and dragging leave it)",
                    v.locked,
                ) {
                    self.edit(&id, "EDIT_VIEWPORT", move |e| e.locked = on);
                }
            });
        });

        // Where the view is on the paper, in paper millimetres from its lower-left corner: typed,
        // as VIEW SET rect= takes it, and refused as a typed rectangle is when it misses the paper.
        ui.strong("On the paper");
        let exact = [v.rect.min.x, v.rect.min.y, v.rect.width(), v.rect.height()];
        let names = ["sheetViewportX", "sheetViewportY", "sheetViewportW", "sheetViewportH"];
        let mut finished = false;
        dialog::form(ui, |form| {
            let ui = form.ui();
            let rows = [
                ("Left", -10000.0, "The left edge, from the paper's left edge"),
                ("Bottom", -10000.0, "The bottom edge, from the paper's bottom edge"),
                ("Width", 1.0, ""),
                ("Height", 1.0, ""),
            ];
            for (at, (label, minimum, tip)) in rows.iter().enumerate() {
                dialog::labelled(ui, label, |ui| {
                    let response = self.fields.number(
                        ui,
                        names[at],
                        exact[at],
                        SpinRange::new(*minimum, 10000.0, 1.0, 1).with_suffix(" mm"),
                    );
                    finished |= response.is_some();
                    if !tip.is_empty() {
                        self.fields.tips.insert(String::from(names[at]), String::from(*tip));
                    }
                });
            }
        });
        if finished {
            // A box left as it was keeps its exact value, not its rounding to the tenth it shows;
            // and nothing changed writes nothing.
            let now: Vec<f64> = names
                .iter()
                .map(|name| self.fields.number_now(name).unwrap_or_default())
                .collect();
            let shown: Vec<f64> = names
                .iter()
                .map(|name| self.fields.shown(name).unwrap_or_default())
                .collect();
            if now != shown {
                let value: Vec<f64> = (0..4)
                    .map(|i| if now[i] == shown[i] { exact[i] } else { now[i] })
                    .collect();
                self.view_set(
                    &id,
                    &format!(
                        "rect={},{},{},{}",
                        verb_number(value[0]),
                        verb_number(value[1]),
                        verb_number(value[0] + value[2]),
                        verb_number(value[1] + value[3])
                    ),
                );
            }
        }

        let prefix = format!("{id}:");
        for problem in &self.canvas.last_stats().problems {
            if let Some(rest) = problem.strip_prefix(&prefix) {
                ui.colored_label(
                    crate::theme::error().to_egui(),
                    rest.trim_start(),
                );
            }
        }
    }

    /// The chainages typed, run as VIEW SET interval=0 stations=...; words that are not chainages
    /// said, and nothing run.
    fn listed(&mut self, id: &str, typed: &str) {
        match station_list(typed) {
            Some(list) => {
                let list = if list.is_empty() {
                    String::from("\"\"")
                } else {
                    list
                };
                self.view_set(id, &format!("interval=0 stations={list}"));
            }
            None => self.queued.push(Queued::Report(
                format!("Chainages are numbers separated by commas: {typed}"),
                true,
            )),
        }
    }

    fn sheet_properties(&mut self, ui: &mut Ui, document: &Document, set: &SheetSet, index: usize) {
        let sheet = &set.sheets[index];
        let edit = |queued: &mut Vec<Queued>, step: &'static str, change: Box<dyn FnOnce(&mut Sheet)>| {
            queued.push(Queued::EditSheet {
                index,
                step,
                change,
            });
        };
        ui.strong("Sheet");
        dialog::form(ui, |form| {
            let ui = form.ui();
            dialog::labelled(ui, "Name", |ui| {
                if let Some(name) = self.fields.line(ui, "sheetName", &sheet.name, "")
                    && name != sheet.name
                {
                    edit(&mut self.queued, "RENAME_SHEET", Box::new(move |s: &mut Sheet| s.name = name));
                }
            });
            dialog::labelled(ui, "Paper", |ui| {
                let papers = paper_items();
                let at = PAPERS.iter().position(|p| *p == sheet.paper).unwrap_or(3);
                let (chosen, _) = self.fields.choice(ui, "sheetPaper", &papers, Choice::at(at));
                if chosen.is_some() {
                    let picked = self
                        .fields
                        .choices
                        .get("sheetPaper")
                        .and_then(|choice| choice.index)
                        .unwrap_or(at);
                    if picked != at {
                        let paper = PAPERS[picked];
                        edit(&mut self.queued, "EDIT_SHEET", Box::new(move |s: &mut Sheet| s.paper = paper));
                    }
                }
            });
            dialog::labelled(ui, "Orientation", |ui| {
                let orientations = [String::from("Landscape"), String::from("Portrait")];
                let was = usize::from(!sheet.landscape);
                let (chosen, _) =
                    self.fields
                        .choice(ui, "sheetOrientation", &orientations, Choice::at(was));
                if chosen.is_some() {
                    let picked = self
                        .fields
                        .choices
                        .get("sheetOrientation")
                        .and_then(|choice| choice.index)
                        .unwrap_or(was);
                    if picked != was {
                        edit(
                            &mut self.queued,
                            "EDIT_SHEET",
                            Box::new(move |s: &mut Sheet| s.landscape = picked == 0),
                        );
                    }
                }
            });
            ui.scope(|ui| {
                if let Some(on) =
                    self.fields
                        .check(ui, "sheetFrame", "Frame and title block", !sheet.frame.is_empty())
                {
                    edit(
                        &mut self.queued,
                        "EDIT_SHEET",
                        Box::new(move |s: &mut Sheet| {
                            s.frame = if on {
                                String::from(BUILT_IN_FRAME_ID)
                            } else {
                                String::new()
                            };
                        }),
                    );
                }
            });
            ui.scope(|ui| {
                if let Some(on) = self
                    .fields
                    .check(ui, "sheetFrameLegend", "Utility legend block", sheet.frame_legend)
                {
                    edit(&mut self.queued, "EDIT_SHEET", Box::new(move |s: &mut Sheet| s.frame_legend = on));
                }
            });
            ui.scope(|ui| {
                if choose_paper_button(ui, set, index) {
                    self.queued.push(Queued::ChoosePaper);
                }
            });
        });

        // Title-block values: what each field prints, and what this sheet overrides.
        ui.strong("Title block on this sheet");
        ui.label(
            "A value here is for this sheet only. Values every sheet shares are set with Title \
             Block.",
        );
        let mut names: BTreeSet<String> = BTreeSet::new();
        if let Some(frame) = sheet_frame(sheet) {
            for text in &frame.texts {
                names.extend(text.fields.iter().cloned());
            }
        }
        let fields = self.source(document).fields;
        let resolved = resolve_fields(set, index, &fields);
        let rows: Vec<Vec<String>> = names
            .iter()
            .map(|field| vec![field.clone(), resolved.get(field).cloned().unwrap_or_default()])
            .collect();
        let mut current = None;
        // The cells as a list for a driver, not drawn: the form under it shows each cell and its
        // value in a column, where the list drew them again as snake_case keys in the text colour.
        offstage(ui, "sheetFieldTable", |ui| {
            controls::rows(ui, "sheetFieldTable", &rows, &mut current);
        });
        dialog::form(ui, |form| {
            let ui = form.ui();
            for field in &names {
                let printed = resolved.get(field).cloned().unwrap_or_default();
                let overridden = sheet.fields.contains_key(field);
                // The cell's key in words ("approver_date" is "Approver date"), as every form's
                // labels are.
                let words = field_words(field);
                dialog::row_with_text(
                    ui,
                    &words,
                    |ui| {
                        // A value this sheet overrides is named in strong text; the others in a
                        // form label's muted text.
                        let text = egui::RichText::new(words.as_str());
                        ui.label(if overridden {
                            text.strong()
                        } else {
                            text.color(crate::theme::text_muted().to_egui())
                        });
                    },
                    |ui| {
                        if let Some(value) =
                            self.fields.line(ui, &format!("sheetField_{field}"), &printed, "")
                            && value != printed
                        {
                            let field = field.clone();
                            edit(
                                &mut self.queued,
                                "SHEET_FIELD",
                                Box::new(move |s: &mut Sheet| {
                                    if value.is_empty() {
                                        s.fields.remove(&field);
                                    } else {
                                        s.fields.insert(field, value);
                                    }
                                }),
                            );
                        }
                    },
                );
            }
        });
        let clear = ui
            .add_enabled_ui(!sheet.fields.is_empty(), |ui| {
                controls::button(ui, "sheetClearFields", "Clear this sheet's values")
            })
            .inner;
        if clear.clicked() {
            edit(&mut self.queued, "SHEET_FIELD", Box::new(|s: &mut Sheet| s.fields.clear()));
        }
        for problem in &self.canvas.last_stats().problems {
            ui.colored_label(crate::theme::error().to_egui(), problem.as_str());
        }
    }

    /// What the panel, the menus and the dialogs asked for in the frame, done now that its drawing
    /// has let the drawing go.
    pub(crate) fn run_queued(&mut self, host: &mut dyn Host) {
        for queued in std::mem::take(&mut self.queued) {
            self.observe(&*host);
            match queued {
                Queued::EditViewport { id, step, change } => {
                    let status = edit_viewport(
                        host.document_mut(),
                        &id,
                        |viewport| {
                            change(viewport);
                            Ok(())
                        },
                        step,
                    );
                    if let Err(error) = status {
                        self.report(host, &error.describe(), true);
                    }
                }
                Queued::EditSheet {
                    index,
                    step,
                    change,
                } => {
                    let status = edit_sheet(
                        host.document_mut(),
                        index,
                        |sheet| {
                            change(sheet);
                            Ok(())
                        },
                        step,
                    );
                    if let Err(error) = status {
                        self.report(host, &error.describe(), true);
                    }
                }
                Queued::Line(line) => {
                    let _ = self.run_line(host, &line);
                }
                Queued::Report(text, error) => self.report(host, &text, error),
                Queued::PlanGrid(id, style, interval) => {
                    if let Err(error) = set_plan_grid(host.document_mut(), &id, style, interval) {
                        self.report(host, &error.describe(), true);
                    }
                }
                Queued::LegendScope(id, scope) => {
                    if let Err(error) = set_legend_scope(host.document_mut(), &id, scope) {
                        self.report(host, &error.describe(), true);
                    }
                }
                Queued::HiddenLayers(id) => self.choose_hidden_layers(&*host, &id),
                Queued::BrowseImage(id) => self.browse_image(host, &id),
                Queued::ChoosePaper => {
                    let _ = self.open_suggest_paper_dialog(host);
                }
                Queued::Trigger(name) => {
                    if let Err(error) = self.trigger(host, &name) {
                        self.report(host, &error.describe(), true);
                    }
                }
                Queued::SelectSheet(row) => self.set_current_sheet(&*host, row),
                Queued::RenameSheet(at, name) => {
                    let _ = edit_sheet(
                        host.document_mut(),
                        at,
                        |sheet| {
                            sheet.name = name;
                            Ok(())
                        },
                        "RENAME_SHEET",
                    );
                }
            }
            self.observe(&*host);
        }
    }

    /// The Image view's Browse: the picture chosen, copied into the project's assets as VIEW SET
    /// file= copies it.
    fn browse_image(&mut self, host: &mut dyn Host, id: &str) {
        let line = format!("VIEW SET {id} file=\"path\"");
        if self.headless {
            self.report(
                host,
                &format!("Browse opens no file dialog in a headless session: type {line}"),
                true,
            );
            return;
        }
        let assets = self.source(host.document()).assets;
        let chosen = self.ask_file(&FileAsk {
            save: false,
            title: String::from("Picture for the View"),
            start: assets.to_string_lossy().into_owned(),
            filter: String::from("Images (*.png *.jpg *.jpeg *.bmp *.gif)"),
        });
        if !chosen.is_empty() {
            let _ = self.run_line(host, &format!("VIEW SET {id} file={}", quoted(&chosen)));
        }
    }
}

// ---- the actions, the menus and the toolbar ------------------------------------------------------

fn shortcut(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(modifiers, key)
}

impl SheetEditor {
    /// The toolbar's Add View menu: a view of each kind (sheetAddView_<kind>).
    fn add_view_menu(&self) -> Menu {
        let mut menu = Menu::new("sheetAddViewMenu", "Add View");
        for kind in KINDS {
            menu = menu.action(Action::new(
                &format!("sheetAddView_{}", kind.name()),
                kind_name(kind),
            ));
        }
        menu
    }

    /// The toolbar's Tile menu: a preset each (sheetTile_<n>).
    fn tile_menu(&self) -> Menu {
        let mut menu = Menu::new("sheetTileMenu", "Tile");
        for (i, preset) in PRESETS.iter().enumerate() {
            menu = menu.action(Action::new(&format!("sheetTile_{i}"), preset_name(*preset)));
        }
        menu
    }

    /// The window's own actions, kept by no menu: Undo and Redo.
    fn window_actions(&self) -> Vec<Action> {
        vec![
            Action::new("sheetUndo", "Undo")
                .icon("Undo")
                .shortcut(shortcut(Modifiers::COMMAND, Key::Z)),
            Action::new("sheetRedo", "Redo")
                .icon("Redo")
                .shortcut(shortcut(Modifiers::COMMAND, Key::Y)),
        ]
    }

    /// The toolbar (sheetToolBar): generate sheets, a new sheet, add a view, tile, arrange, the
    /// title block, fit the page, check, and plot.
    pub fn tool_bar(&self, host: &dyn Host) -> ToolBar {
        let _ = host;
        let mut bar = ToolBar::new("sheetToolBar", "Sheets");
        let mut add = |item: ToolBarItem| bar.items.push(item);
        add(ToolBarItem::Action(
            Action::new("sheetGenerate", "Generate Sheets...")
                .icon("Properties")
                .tip("Lay sheets out for the drawing, an alignment or its sections")
                .opens("sheetGenerateDialog"),
        ));
        add(ToolBarItem::Action(
            Action::new("sheetNewSheet", "New Sheet").icon("New"),
        ));
        add(ToolBarItem::Widget(String::from("sheetAddViewButton")));
        add(ToolBarItem::Widget(String::from("sheetTileButton")));
        add(ToolBarItem::Widget(String::from("sheetArrangeButton")));
        add(ToolBarItem::Separator);
        add(ToolBarItem::Action(
            Action::new("sheetTitleBlock", "Title Block...")
                .icon("Properties")
                .tip(
                    "The organisation, project, sign-offs, notes, revisions and logo every sheet \
                     shares",
                )
                .opens("sheetTitleBlockDialog"),
        ));
        add(ToolBarItem::Separator);
        add(ToolBarItem::Action(
            Action::new("sheetFitPage", "Fit Page")
                .icon("ZoomExtents")
                .shortcut(shortcut(Modifiers::NONE, Key::Home)),
        ));
        add(ToolBarItem::Separator);
        add(ToolBarItem::Action(
            Action::new("sheetCheck", "Check Sheets").tip(
                "Find what a plot would get wrong: views off the paper or over nothing, sections \
                 off their alignment, blanks in the title block",
            ),
        ));
        add(ToolBarItem::Action(
            Action::new("sheetPlotSheet", "Plot Sheet...")
                .icon("Plot")
                .opens("plotDialog"),
        ));
        add(ToolBarItem::Action(
            Action::new("sheetPlotAll", "Plot All...")
                .icon("Plot")
                .shortcut(shortcut(Modifiers::COMMAND, Key::P))
                .opens("plotDialog"),
        ));
        bar
    }

    /// The menu bar: Sheet Set first, then Edit and View.
    pub fn menus(&self, host: &dyn Host) -> Vec<Menu> {
        vec![
            self.sheet_set_menu(),
            self.edit_menu(),
            self.view_menu(),
            self.add_view_menu(),
            self.tile_menu(),
            self.arrange_menu(host),
        ]
    }

    /// Every action of the editor - its own, its menus', its toolbar's and its toolbar buttons'
    /// menus' - as they stand now.
    pub fn actions(&self, host: &dyn Host) -> Vec<Action> {
        let mut found = self.window_actions();
        for menu in self.menus(host) {
            for action in menu.actions() {
                if !found.iter().any(|known| known.name == action.name) {
                    found.push(action.clone());
                }
            }
        }
        for action in self.tool_bar(host).actions() {
            if !found.iter().any(|known| known.name == action.name) {
                found.push(action.clone());
            }
        }
        found
    }

    /// The action `name` as it stands now (findChild<QAction*>).
    pub fn action(&self, host: &dyn Host, name: &str) -> Option<Action> {
        self.actions(host).into_iter().find(|action| action.name == name)
    }

    /// Where action `name` takes its key: PgUp and PgDn are left to a box of the properties, which
    /// pages with them, and taken only on the canvas and the list.
    pub fn shortcut_scope(&self, name: &str) -> ShortcutScope {
        match name {
            "sheetPreviousSheet" | "sheetNextSheet" => {
                ShortcutScope::Widgets(&["sheetCanvas", "sheetList"])
            }
            _ => ShortcutScope::Window,
        }
    }

    /// The action `name` triggered, as a click does (QAction::trigger): NotFound for a name the
    /// editor has no action of, InvalidState for a disabled one.
    pub fn trigger(&mut self, host: &mut dyn Host, name: &str) -> Status {
        self.observe(&*host);
        // The list's buttons, which are buttons, not actions.
        if name.starts_with("sheetList") && self.list_button(host, name) {
            self.observe(&*host);
            return Ok(());
        }
        let known = self.action(&*host, name);
        let context = matches!(
            name,
            "sheetBringToFront" | "sheetSendToBack" | "sheetFillDrawingArea"
        ) || name.starts_with("sheetMenuAddView_")
            || name.starts_with("sheetMenuTile_")
            || name.starts_with("sheetMatchScale_");
        match known {
            Some(action) if !action.enabled => {
                return Err(make_error(
                    ErrorCode::InvalidState,
                    "that menu item is disabled",
                    name,
                ));
            }
            None if !context => {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "no menu item or tool has that name",
                    name,
                ));
            }
            _ => {}
        }
        let report_failure = |editor: &mut Self, host: &mut dyn Host, status: Status| {
            if let Err(error) = status {
                editor.report(host, &error.describe(), true);
            }
        };
        if let Some(kind) = name
            .strip_prefix("sheetAddView_")
            .or_else(|| name.strip_prefix("sheetMenuAddView_"))
        {
            if let Some(kind) = KINDS.iter().find(|each| each.name() == kind) {
                let status = self.add_viewport(host, *kind);
                report_failure(self, host, status);
            }
            return Ok(());
        }
        if let Some(i) = name
            .strip_prefix("sheetTile_")
            .or_else(|| name.strip_prefix("sheetMenuTile_"))
        {
            if let Some(preset) = i.parse::<usize>().ok().and_then(|i| PRESETS.get(i)) {
                let status = self.tile(host, *preset);
                report_failure(self, host, status);
            }
            return Ok(());
        }
        if self.trigger_editing(host, name)
            || self.trigger_arrange(host, name)
            || self.trigger_sheet_set(host, name)
        {
            self.observe(&*host);
            return Ok(());
        }
        match name {
            "sheetUndo" => {
                let _ = host.document_mut().undo();
            }
            "sheetRedo" => {
                let _ = host.document_mut().redo();
            }
            "sheetGenerate" => {
                let _ = self.open_generate_dialog(&*host);
            }
            "sheetNewSheet" => {
                let _ = self.add_blank_sheet(host);
            }
            "sheetTitleBlock" => self.edit_title_block(&*host),
            "sheetFitPage" => self.canvas.fit_page(),
            "sheetCheck" => {
                let _ = self.check_sheets(host);
            }
            "sheetPlotSheet" => self.plot_interactive(host, false),
            "sheetPlotAll" => self.plot_interactive(host, true),
            "sheetBringToFront" | "sheetSendToBack" | "sheetFillDrawingArea" => {
                let id = self
                    .context_menu
                    .as_ref()
                    .map(|(_, id)| id.clone())
                    .unwrap_or_else(|| String::from(self.canvas.selected()));
                self.context_action(host, name, &id);
            }
            _ => {}
        }
        self.observe(&*host);
        Ok(())
    }

    /// The context menu's own items, for the view `viewport_id`.
    pub fn context_action(&mut self, host: &mut dyn Host, name: &str, viewport_id: &str) {
        let index = self.current_sheet();
        let id = String::from(viewport_id);
        let _ = match name {
            "sheetBringToFront" => edit_sheet(
                host.document_mut(),
                index,
                |s| {
                    if let Some(at) = s.viewports.iter().position(|v| v.id == id) {
                        s.viewports[at..].rotate_left(1);
                    }
                    Ok(())
                },
                "ORDER_VIEWPORT",
            ),
            "sheetSendToBack" => edit_sheet(
                host.document_mut(),
                index,
                |s| {
                    if let Some(at) = s.viewports.iter().position(|v| v.id == id) {
                        s.viewports[..=at].rotate_right(1);
                    }
                    Ok(())
                },
                "ORDER_VIEWPORT",
            ),
            "sheetFillDrawingArea" => {
                let set = host.document().sheet_set();
                let Some(sheet) = set.sheets.get(index) else {
                    return;
                };
                let area = tiling_area(sheet);
                edit_viewport(
                    host.document_mut(),
                    &id,
                    |v| {
                        v.rect = area;
                        Ok(())
                    },
                    "RESIZE_VIEWPORT",
                )
            }
            _ => Ok(()),
        };
        self.observe(&*host);
    }

    /// The canvas's context menu's items, for the viewport `viewport_id` (empty for the paper).
    pub fn context_menu_items(&self, host: &dyn Host, viewport_id: &str) -> Vec<MenuItem> {
        let mut arrange = self.arrange_menu(host);
        arrange.name = String::from("sheetContextArrangeMenu");
        let mut items = vec![MenuItem::Menu(arrange), MenuItem::Separator];
        let named = |name: &str| self.action(host, name).map(MenuItem::Action);
        if viewport_id.is_empty() {
            let mut add = Menu::new("sheetContextAddViewMenu", "Add View");
            for kind in KINDS {
                add = add.action(Action::new(
                    &format!("sheetMenuAddView_{}", kind.name()),
                    kind_name(kind),
                ));
            }
            let mut tiles = Menu::new("sheetContextTileMenu", "Tile");
            for (i, preset) in PRESETS.iter().enumerate() {
                tiles = tiles.action(Action::new(&format!("sheetMenuTile_{i}"), preset_name(*preset)));
            }
            items.push(MenuItem::Menu(add));
            items.push(MenuItem::Menu(tiles));
            items.push(MenuItem::Separator);
            items.extend(["sheetPaste", "sheetSelectAll", "sheetFitPage"].iter().filter_map(|n| named(n)));
        } else {
            items.push(MenuItem::Action(Action::new("sheetBringToFront", "Bring to Front")));
            items.push(MenuItem::Action(Action::new("sheetSendToBack", "Send to Back")));
            items.push(MenuItem::Action(Action::new(
                "sheetFillDrawingArea",
                "Fill the Drawing Area",
            )));
            items.push(MenuItem::Separator);
            // The Edit and View menus' own actions, shortcuts and all.
            items.extend(
                ["sheetCut", "sheetCopy", "sheetDuplicateViews", "sheetZoomSelection"]
                    .iter()
                    .filter_map(|n| named(n)),
            );
            items.push(MenuItem::Separator);
            items.extend(named("sheetDeleteViews"));
        }
        items
    }

    // ---- the canvas's events -----------------------------------------------------------------

    pub fn mouse_press(
        &mut self,
        host: &mut dyn Host,
        at: PointF,
        button: MouseButton,
        modifiers: KeyModifiers,
    ) {
        self.observe(&*host);
        self.focus_canvas = true;
        self.canvas.mouse_press(host.document(), at, button, modifiers);
        if button == MouseButton::Right {
            let id = self.canvas.context_menu_at(at);
            self.context_menu = Some((egui::pos2(at.x as f32, at.y as f32), id));
        }
    }

    pub fn mouse_move(&mut self, host: &mut dyn Host, at: PointF, modifiers: KeyModifiers) {
        self.observe(&*host);
        self.canvas.mouse_move(host.document(), at, modifiers);
        self.cursor_status = readout_text(self.canvas.cursor_readout());
    }

    pub fn mouse_release(&mut self, host: &mut dyn Host, _at: PointF, _button: MouseButton) {
        self.observe(&*host);
        self.canvas.mouse_release(host.document_mut());
        self.observe(&*host);
    }

    pub fn mouse_double_click(&mut self, host: &mut dyn Host, at: PointF, button: MouseButton) {
        self.observe(&*host);
        self.canvas.mouse_double_click(at, button);
    }

    pub fn wheel(&mut self, host: &dyn Host, at: PointF, notches: f64) {
        self.observe(host);
        self.canvas.wheel(at, notches);
    }

    /// A key pressed on the canvas: what the canvas does itself, and what it asks of the editor.
    pub fn key_press(&mut self, host: &mut dyn Host, key: SheetKey, modifiers: KeyModifiers) {
        self.observe(&*host);
        match self.canvas.key_press(key, modifiers, false) {
            CanvasAsks::Remove => {
                let _ = self.remove_selected_viewport(host);
            }
            CanvasAsks::Nudge(delta) => {
                let _ = self.nudge_selection(host, delta);
            }
            CanvasAsks::PreviousSheet => {
                let _ = self.trigger(host, "sheetPreviousSheet");
            }
            CanvasAsks::NextSheet => {
                let _ = self.trigger(host, "sheetNextSheet");
            }
            CanvasAsks::Nothing | CanvasAsks::NotTaken => {}
        }
        self.observe(&*host);
    }

    pub fn key_release(&mut self, key: SheetKey) {
        self.canvas.key_release(key, false);
    }

    pub fn leave(&mut self) {
        self.canvas.leave();
        self.cursor_status = readout_text(self.canvas.cursor_readout());
    }

    /// The canvas as it shows now (QWidget::grab), painted from the document.
    pub fn paint_canvas(&mut self, host: &dyn Host) -> Image {
        self.observe(host);
        self.canvas.paint(host.document())
    }

    /// What is due now: the checks a moment after the last change, a thumbnail, a sheet dropped in
    /// the list; then what the frame's controls asked for. The C++'s timers and queued calls, run
    /// at each frame and when a test waits.
    pub fn poll(&mut self, host: &mut dyn Host) {
        self.observe(&*host);
        if let Some((from, to)) = self.list.take_move_request()
            && let Err(error) = self.move_sheet_to(host, from, to)
        {
            self.report(host, &error.describe(), true);
        }
        self.run_queued(host);
        if self.checks.due() {
            let _ = self.check_now(&*host);
        }
        if self
            .thumbnails_due
            .is_some_and(|due| Instant::now() >= due)
        {
            self.paint_next_thumbnail(&*host);
        }
    }

    // ---- the window, in egui ---------------------------------------------------------------

    /// One frame of the editor: its menus, toolbar, sheet list, canvas, properties, checks and
    /// status bar, in the target `sheetEditor`, and its dialogs, each in its own frame.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.observe(&*host);
        self.update_paste_ready(host);
        let ctx = ui.ctx().clone();
        begin_target(ui, TargetSpec::new("sheetEditor", TargetKind::Window, "Sheets"));
        let whole = ui.max_rect();
        self.shortcut_pass(ui, host);
        let mut chosen: Option<String> = None;
        let menus = self.menus(&*host);
        let tool_bar = self.tool_bar(&*host);
        egui::Panel::top("sheetEditorMenus").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                for menu in menus.iter().take(3) {
                    menu_ui(ui, menu, &mut chosen);
                }
            });
            // The toolbar is a target of its own (QToolBar sheetToolBar): `--panel sheetToolBar`
            // grabs it once a dialog of the editor has closed.
            ui.horizontal(|ui| {
                begin_target(
                    ui,
                    TargetSpec::new(&tool_bar.name, TargetKind::ToolBar, &tool_bar.title),
                );
                tool_bar_look(ui);
                for item in &tool_bar.items {
                    match item {
                        ToolBarItem::Action(action) => {
                            // The shortcut in the tooltip, as the window's toolbars name theirs:
                            // drawn in the button ("Fit Page  Home", "Plot All...  Ctrl+P") it
                            // read as a second word of the button's.
                            let mut shown = action.clone();
                            let tip = match shown.shortcut.take() {
                                Some(key) => {
                                    let said = if action.tip.is_empty() {
                                        action.display_text()
                                    } else {
                                        action.tip.clone()
                                    };
                                    format!("{said} ({})", shortcut_text(&key))
                                }
                                None => action.tip.clone(),
                            };
                            let response = action_item(ui, &shown);
                            let response = if tip.is_empty() {
                                response
                            } else {
                                response.on_hover_text(tip)
                            };
                            if response.clicked() {
                                chosen = Some(action.name.clone());
                            }
                        }
                        ToolBarItem::Separator => {
                            ui.separator();
                        }
                        ToolBarItem::Widget(name) => {
                            let menu = match name.as_str() {
                                "sheetAddViewButton" => &menus[3],
                                "sheetTileButton" => &menus[4],
                                _ => &menus[5],
                            };
                            tool_button_menu(ui, name, menu, &mut chosen);
                        }
                    }
                }
                end_target(ui, ui.min_rect());
            });
        });
        egui::Panel::bottom("sheetEditorStatus").show(ui, |ui| {
            ui.horizontal(|ui| {
                let message = if self.status.is_empty() {
                    self.count_text.clone()
                } else {
                    self.status.clone()
                };
                controls::label(ui, "sheetStatus", &message);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    controls::label(ui, "sheetCursorStatus", &self.cursor_status);
                });
            });
        });
        // The dock's room is kept while it is closed, so the canvas keeps its size: in the C++ the
        // canvas grew into it and fitted its page again, which moved every paper point under the
        // pointer. Recorded as a difference.
        egui::Panel::bottom("sheetChecksDock")
            .resizable(false)
            .exact_size(170.0)
            .show(ui, |ui| {
                if !self.checks.is_visible() {
                    return;
                }
                // The dock's compact header: its title, and its close button at the right.
                let title = String::from(self.checks.window_title());
                egui::Sides::new().show(
                    ui,
                    |ui| {
                        ui.strong(title.as_str());
                    },
                    |ui| {
                        if ui.small_button("\u{d7}").clicked() {
                            self.checks.close();
                        }
                    },
                );
                self.checks.ui(ui);
            });
        if let Some(finding) = self.checks.take_activated() {
            self.activate_finding(host, finding);
        }
        egui::Panel::left("sheetListDock")
            .resizable(false)
            .exact_size(SHEET_LIST_WIDTH)
            .show(ui, |ui| {
            self.list_ui(ui, &*host);
        });
        // About the window's side panels' width (the look of 2026-11-16; it was 300): a form's
        // label and field.
        egui::Panel::right("sheetPropertiesDock")
            .resizable(false)
            .exact_size(PROPERTIES_WIDTH)
            .show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("sheetProperties").show(ui, |ui| {
                // Its rows share one label column, as a dialog's do. Its boxes and buttons are
                // drawn across the panel rather than under the fields: at PROPERTIES_WIDTH a box's
                // words, or "Choose paper for this scale", beside the label column ran past the
                // panel's edge, and the panel's content with them.
                dialog::form_panel(ui, |ui| self.properties_ui(ui, &*host));
            });
        });
        self.canvas_ui(ui, host);
        end_target(ui, whole);

        // The context menu, and the dialogs, each a target of its own.
        if let Some((at, id)) = self.context_menu.clone() {
            let items = self.context_menu_items(&*host, &id);
            let menu = Menu {
                items,
                ..Menu::new("sheetContextMenu", "")
            };
            let shown = crate::shell::popup_menu(&ctx, "sheetContextMenu", at, |ui| {
                menu_items_ui(ui, &menu, &mut chosen);
            });
            // Closed by an item chosen, Escape, or a click away from it.
            let away = ctx.input(|input| {
                input.key_pressed(Key::Escape)
                    || (input.pointer.any_pressed()
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|at| !shown.rect.contains(at)))
            });
            if chosen.is_some() || away {
                self.context_menu = None;
                if let Some(name) = chosen.take() {
                    // Kept for the item that acts on it, then let go.
                    self.context_menu = Some((at, id));
                    let _ = self.trigger(host, &name);
                    self.context_menu = None;
                }
            }
        }
        if let Some(name) = chosen
            && let Err(error) = self.trigger(host, &name)
        {
            self.report(host, &error.describe(), true);
        }
        self.dialogs_ui(&ctx, host);
        self.poll(host);
        if let Some(wait) = self.checks.time_to_run() {
            ctx.request_repaint_after(wait);
        }
        if self.thumbnails_due.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(THUMBNAIL_DELAY_MS));
        }
    }

    fn dialogs_ui(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        if let Some(mut dialog) = self.generate.take() {
            let plan_view = self.plan_view_area.as_ref().and_then(|area| area());
            match dialog.ui(ctx, plan_view) {
                GenerateAnswer::Open => self.generate = Some(dialog),
                GenerateAnswer::Cancelled => {}
                GenerateAnswer::Accepted {
                    line,
                    cover,
                    replace,
                } => self.accept_generate(host, line, cover, replace),
            }
        }
        if let Some(mut dialog) = self.hidden_layers.take() {
            match dialog.ui(ctx) {
                None => self.hidden_layers = Some(dialog),
                Some(false) => {}
                Some(true) => {
                    let options = dialog.options();
                    if !options.is_empty() {
                        let line = format!("VIEW SET {} {}", dialog.id, options.join(" "));
                        let _ = self.run_line(host, &line);
                    }
                }
            }
        }
        if let Some(mut dialog) = self.title_block.take() {
            let logo = host.document().sheet_set().defaults.logo_asset.clone();
            let shown = logo_path(host.document())
                .map(|path| path.to_string_lossy().into_owned())
                .filter(|_| !logo.is_empty())
                .unwrap_or_default();
            match dialog.ui(ctx, &shown) {
                TitleBlockAnswer::Open => self.title_block = Some(dialog),
                TitleBlockAnswer::Cancel => {}
                TitleBlockAnswer::Ok => self.accept_title_block(host, &dialog),
                TitleBlockAnswer::ChooseLogo => {
                    self.title_block = Some(dialog);
                    self.choose_logo(host);
                }
                TitleBlockAnswer::NoLogo => {
                    self.title_block = Some(dialog);
                    self.clear_logo(host);
                }
            }
        }
        self.arrange_dialogs_ui(ctx, host);
        self.plot_dialogs_ui(ctx, host);
    }

    /// The sheet list and its buttons: a new sheet, a copy, remove, up and down (the C++'s tool
    /// buttons had no names; here sheetListNew, sheetListCopy, sheetListRemove, sheetListUp and
    /// sheetListDown).
    fn list_ui(&mut self, ui: &mut Ui, host: &dyn Host) {
        let set = host.document().sheet_set();
        let rows: Vec<SheetListRow> = (0..set.sheets.len())
            .map(|index| SheetListRow {
                text: Self::list_text(&set, index),
                picture: self.list_texture(ui.ctx(), index),
            })
            .collect();
        let before = self.list.current_row();
        let response = self.list.show(ui, &rows);
        if response.changed()
            && let Some(row) = self.list.current_row()
            && Some(row) != before
        {
            self.queued.push(Queued::SelectSheet(row));
        }
        if response.double_clicked()
            && let Some(sheet) = set.sheets.get(self.current_sheet())
        {
            self.rename = Some((self.current_sheet(), sheet.name.clone()));
        }
        if response.has_focus() || response.hovered() {
            for (key, name) in [(Key::PageUp, "sheetPreviousSheet"), (Key::PageDown, "sheetNextSheet")] {
                if ui.input_mut(|input| input.consume_key(Modifiers::NONE, key)) {
                    self.queued.push(Queued::Trigger(String::from(name)));
                }
            }
        }
        ui.horizontal(|ui| {
            let buttons = [
                ("sheetListNew", "+", "A new blank sheet"),
                ("sheetListCopy", "Copy", "Duplicate this sheet"),
                ("sheetListRemove", "\u{2212}", "Remove this sheet"),
            ];
            for (name, text, tip) in buttons {
                if controls::button(ui, name, text).on_hover_text(tip).clicked() {
                    self.queued.push(Queued::Trigger(String::from(name)));
                }
            }
            // Up and Down are arrows painted, not characters: the window's font has no U+2191 or
            // U+2193, and they were drawn as two empty boxes.
            for (name, text, tip, up) in [
                ("sheetListUp", "Move Up", "Move this sheet up", true),
                ("sheetListDown", "Move Down", "Move this sheet down", false),
            ] {
                let side = ui.spacing().interact_size.y;
                let response = controls::button_as(
                    ui,
                    name,
                    text,
                    egui::Button::new("").min_size(egui::vec2(side, side)),
                )
                .on_hover_text(tip);
                paint_arrow(ui, response.rect, up);
                if response.clicked() {
                    self.queued.push(Queued::Trigger(String::from(name)));
                }
            }
        });
        if let Some((at, mut name)) = self.rename.take() {
            ui.horizontal(|ui| {
                ui.label("Name");
                controls::line(ui, "sheetRenameName", &mut name);
                let ok = controls::button(ui, "sheetRenameOk", "OK").clicked();
                let cancel = controls::button(ui, "sheetRenameCancel", "Cancel").clicked();
                if ok {
                    self.queued.push(Queued::RenameSheet(at, name.clone()));
                } else if !cancel {
                    self.rename = Some((at, name.clone()));
                }
            });
        }
    }

    /// The list's buttons, by name.
    pub(crate) fn list_button(&mut self, host: &mut dyn Host, name: &str) -> bool {
        let at = self.current_sheet();
        let count = host.document().sheet_set().sheets.len();
        match name {
            "sheetListNew" => {
                let _ = self.add_blank_sheet(host);
            }
            "sheetListCopy" => {
                if let Err(error) =
                    katana_cad::plotting::sheet_commands::duplicate_sheet(host.document_mut(), at)
                {
                    self.report(host, &error.describe(), true);
                } else {
                    self.observe(&*host);
                    self.set_current_sheet(&*host, at + 1);
                }
            }
            "sheetListRemove" => {
                if count > 0 {
                    let _ = katana_cad::plotting::sheet_commands::remove_sheet(host.document_mut(), at);
                }
            }
            "sheetListUp" => {
                if at > 0
                    && katana_cad::plotting::sheet_commands::move_sheet(host.document_mut(), at, at - 1)
                        .is_ok()
                {
                    self.observe(&*host);
                    self.set_current_sheet(&*host, at - 1);
                }
            }
            "sheetListDown" => {
                if at + 1 < count
                    && katana_cad::plotting::sheet_commands::move_sheet(host.document_mut(), at, at + 1)
                        .is_ok()
                {
                    self.observe(&*host);
                    self.set_current_sheet(&*host, at + 1);
                }
            }
            _ => return false,
        }
        true
    }

    /// The window's shortcuts: each enabled action whose key was pressed, unless a text box has
    /// the keyboard (a disabled action, and a box being typed in, leave the key alone).
    fn shortcut_pass(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        for action in self.actions(&*host) {
            let Some(keys) = action.shortcut else {
                continue;
            };
            if !action.enabled || self.shortcut_scope(&action.name) != ShortcutScope::Window {
                continue;
            }
            if ui.input_mut(|input| input.consume_shortcut(&keys)) {
                self.queued.push(Queued::Trigger(action.name.clone()));
            }
        }
    }

    /// The canvas, drawn into what the docks leave, its input turned into the event methods.
    fn canvas_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let wanted = ui.available_size().max(egui::vec2(200.0, 150.0));
        let (rect, response) = ui.allocate_exact_size(wanted, egui::Sense::click_and_drag());
        let width = f64::from(rect.width());
        let height = f64::from(rect.height());
        if (width, height) != (self.canvas.width(), self.canvas.height()) {
            self.canvas.resize(width, height);
        }
        self.canvas
            .set_device_ratio(f64::from(ui.ctx().pixels_per_point()));
        if self.focus_canvas {
            response.request_focus();
            self.focus_canvas = false;
        }
        let local = |at: egui::Pos2| {
            PointF::new(f64::from(at.x - rect.min.x), f64::from(at.y - rect.min.y))
        };
        let over = response.contains_pointer();
        let (events, now) = ui.input(|input| (input.events.clone(), input.time));
        for event in events {
            match event {
                egui::Event::PointerMoved(at) => {
                    if over || !self.egui_state.held.is_empty() {
                        let modifiers = KeyModifiers::from_egui(ui.input(|input| input.modifiers));
                        self.mouse_move(host, local(at), modifiers);
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
                        self.egui_state.held.push(button);
                        // A double click from the presses, as Qt delivers it at the second press.
                        let double = self.egui_state.last_press.is_some_and(|(when, place, which)| {
                            which == button
                                && now - when <= DOUBLE_CLICK_SECONDS
                                && (at.x - place.x).abs() <= DOUBLE_CLICK_PIXELS
                                && (at.y - place.y).abs() <= DOUBLE_CLICK_PIXELS
                        });
                        if double {
                            self.egui_state.last_press = None;
                            self.mouse_double_click(host, at, button);
                        } else {
                            self.egui_state.last_press = Some((now, at, button));
                            self.mouse_press(host, at, button, modifiers);
                        }
                    } else if let Some(held) =
                        self.egui_state.held.iter().position(|&held| held == button)
                    {
                        self.egui_state.held.remove(held);
                        self.mouse_release(host, at, button);
                    }
                }
                egui::Event::MouseWheel { delta, .. } => {
                    if over && let Some(at) = ui.input(|input| input.pointer.latest_pos()) {
                        self.wheel(&*host, local(at), f64::from(delta.y) / 50.0);
                    }
                }
                egui::Event::PointerGone => self.leave(),
                egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    repeat,
                    ..
                } => {
                    if !response.has_focus() {
                        continue;
                    }
                    let modifiers = KeyModifiers::from_egui(modifiers);
                    let key = match key {
                        Key::Space => SheetKey::Space,
                        Key::Escape => SheetKey::Escape,
                        Key::Delete => SheetKey::Delete,
                        Key::Backspace => SheetKey::Backspace,
                        Key::ArrowLeft => SheetKey::Left,
                        Key::ArrowRight => SheetKey::Right,
                        Key::ArrowUp => SheetKey::Up,
                        Key::ArrowDown => SheetKey::Down,
                        Key::Tab if modifiers.shift => SheetKey::Backtab,
                        Key::Tab => SheetKey::Tab,
                        Key::PageUp => SheetKey::PageUp,
                        Key::PageDown => SheetKey::PageDown,
                        _ => continue,
                    };
                    if pressed {
                        if repeat && key == SheetKey::Space {
                            continue;
                        }
                        self.key_press(host, key, modifiers);
                    } else {
                        self.key_release(key);
                    }
                }
                _ => {}
            }
        }
        if response.has_focus() {
            // The canvas's own keys are not egui's to move the focus with.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        } else if self.canvas.space_held {
            self.canvas.focus_out();
        }
        if self.egui_state.inside && !over && self.egui_state.held.is_empty() {
            self.leave();
        }
        self.egui_state.inside = over;
        if over {
            ui.ctx().set_cursor_icon(match self.canvas.cursor() {
                CanvasCursor::Arrow => egui::CursorIcon::Default,
                CanvasCursor::OpenHand => egui::CursorIcon::Grab,
                CanvasCursor::ClosedHand => egui::CursorIcon::Grabbing,
                CanvasCursor::SizeAll => egui::CursorIcon::Move,
                CanvasCursor::Handle(0 | 4) => egui::CursorIcon::ResizeNwSe,
                CanvasCursor::Handle(1 | 5) => egui::CursorIcon::ResizeVertical,
                CanvasCursor::Handle(2 | 6) => egui::CursorIcon::ResizeNeSw,
                CanvasCursor::Handle(_) => egui::CursorIcon::ResizeHorizontal,
            });
        }
        if ui.is_rect_visible(rect) {
            let picture = self.paint_canvas(&*host).to_color_image();
            let texture = match &mut self.canvas_texture {
                Some(texture) => {
                    texture.set(picture, egui::TextureOptions::NEAREST);
                    texture.id()
                }
                None => {
                    let texture =
                        ui.ctx()
                            .load_texture("sheetCanvas", picture, egui::TextureOptions::NEAREST);
                    let id = texture.id();
                    self.canvas_texture = Some(texture);
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
        controls::custom(ui, &response, ControlRecord::new("sheetCanvas", ControlKind::View));
    }
}

/// A menu drawn as a menu button with its items: a click on an action is the action chosen.
/// A menu of the editor's menu bar, drawn as the main window draws its own: its items by
/// drive::action::menu_items_ui (a tick or icon gutter, titled sections, inset separators,
/// submenus beside it) in one column that scrolls when it is taller than the screen.
fn menu_ui(ui: &mut Ui, menu: &Menu, chosen: &mut Option<String>) {
    ui.menu_button(menu.title.replace('&', ""), |ui| {
        crate::shell::dialog::menu_column(ui, |ui| {
            menu_items_ui(ui, menu, chosen);
        });
    });
}

/// A title block cell's key ("approver_date") as a form's label says it: words, in sentence case
/// ("Approver date").
fn field_words(key: &str) -> String {
    let spaced = key.replace('_', " ");
    let mut letters = spaced.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// An arrow up or down, centred in `rect`, in the text colour: a stem and a head, in the line
/// weight of the window's icons.
fn paint_arrow(ui: &Ui, rect: egui::Rect, up: bool) {
    let centre = rect.center();
    let half = rect.height().min(rect.width()) * 0.25;
    let sign = if up { -1.0 } else { 1.0 };
    let tip = centre + egui::vec2(0.0, sign * half);
    let tail = centre - egui::vec2(0.0, sign * half);
    let ink = egui::Stroke::new(1.5, ui.visuals().text_color());
    let painter = ui.painter();
    painter.line_segment([tail, tip], ink);
    let head = half * 0.7;
    painter.line_segment([tip, tip + egui::vec2(-head, -sign * head)], ink);
    painter.line_segment([tip, tip + egui::vec2(head, -sign * head)], ink);
}

/// The editor's toolbar drawn as the window's toolbars are: a button is flat at rest, its text
/// on the bar's own ground, and lit only under the pointer - where every button was a framed box,
/// a row of boxes unlike any other toolbar of the window.
fn tool_bar_look(ui: &mut Ui) {
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = egui::Stroke::NONE;
}

/// A toolbar button that opens its menu (QToolButton::InstantPopup), named `name`.
fn tool_button_menu(ui: &mut Ui, name: &str, menu: &Menu, chosen: &mut Option<String>) {
    let taken = take(ui, name);
    let response = ui
        .menu_button(menu.title.replace('&', ""), |ui| {
            crate::shell::dialog::menu_column(ui, |ui| {
                menu_items_ui(ui, menu, chosen);
            });
        })
        .response;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Press | Act::ScrollTo => settle(ui, taken, Ok(())),
            Act::Fill(text) => settle(ui, taken, Err(no_field(name, text))),
        }
    }
    let mut record = ControlRecord::new(name, ControlKind::Button);
    record.text = menu.title.clone();
    controls::custom(ui, &response, record);
}
