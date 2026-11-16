//! The section and profile view: the port of section_view_widget.
//!
//! Drawn through the canvas rather than through the 3D rasteriser, deliberately: a section is a
//! measured DRAWING - axes, a datum, gridlines, station labels, annotated crossings - not a view of
//! a model. Text and hairlines are what the painter is good at, and a section never has enough
//! geometry for the software rasteriser to be worth its setup.
//!
//! The section itself is computed by katana_cad's extract_section and is not recomputed while the
//! user pans or zooms: this view only maps station and elevation to pixels.
//!
//! THE RUST SHAPE, as the plan view's (viewport_widget): one struct drawn into the rect it is
//! given, its events methods in its own pixels, what it raised queued (`take_events`), and the
//! view's `ViewState` - which holds the section and its exaggeration, so changing the view to
//! another kind and back shows the same section again - lent to every call. The drawing whose
//! selection it marks is lent too (`Option<&Document>`, None for a view given none, which draws
//! every crossing as it always was); `set_document` only listens to it, so a selection change
//! asks for a frame.
//!
//! The pan and zoom are the view's own, not the state's, as in the C++: a change of kind and back
//! frames the section afresh.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Keep the C++'s negated comparisons, which refuse NaN."
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use katana_cad::section::{SampleReason, Section, SectionCrossing};
use katana_cad::selection::{is_drawn, is_ghost};
use katana_cad::selection_style::{
    GHOST_ALPHA, GHOST_CROSSING_PIXELS, SELECTION_BLUE, SELECTION_GREEN, SELECTION_RED,
};
use katana_cad::view_set::ViewState;
use katana_cad::{Document, ListenerHandle};

use crate::canvas::{
    Brush, Canvas, Color, Font, FontSize, HAlign, Image, Pen, PenCap, PenJoin, PenStyle, PointF,
    RasterCanvas, RectF, RenderHint, TextFlags, VAlign,
};
use crate::format::qt_fixed;
use crate::theme::palette;
use crate::theme::style::ui_font;
use crate::view_focus::{FocusPrevious, FocusReason, focus_chooses_view};
use crate::viewport_widget::{KeyModifiers, MouseButton};

/// Room for the elevation labels on the left and the station labels below.
pub const LEFT_MARGIN: i32 = 62;
pub const BOTTOM_MARGIN: i32 = 26;
pub const TOP_MARGIN: i32 = 10;
pub const RIGHT_MARGIN: i32 = 12;

const ZOOM_PER_NOTCH: f64 = 1.15;

/// A selected entity's crossing: the selection colour, solid and 2 px against the others' 1 px
/// dashes, with a dot of 3.5 px radius at its level - a little wider than the 1.8 px surface lines
/// it sits on.
const SELECTED_CROSSING_PIXELS: f64 = 2.0;
const SELECTED_CROSSING_DOT: f64 = 3.5;

/// The plan's ground (the viewport token), every view's: it was (24, 26, 32), a third dark beside
/// the plan's and the 3D view's in Quad (the third look audit's H).
const BACKGROUND: Color = palette::viewport();
const GRID: Color = Color::rgb(48, 52, 62);
const GRID_MAJOR: Color = Color::rgb(74, 80, 94);
const AXIS: Color = Color::rgb(150, 158, 175);
const TEXT: Color = Color::rgb(196, 202, 214);
const CROSSING: Color = Color::rgb(210, 90, 90);

/// A distinct colour per surface, in the order they were cut. Existing ground first, then design:
/// the two a section almost always carries. None near the selection's colour (selection_style),
/// which marks a selected crossing and its dot. With the selection orange the second was (235,
/// 170, 80), CIE76 22.9 from it, and was made the light blue (130, 175, 245); with the selection
/// the accent's blue (#4c8dff, 2026-11-16) that blue was 28.8 from it and the orange is 118.2.
/// The nearest now is the magenta, 50.9.
const SURFACE_COLORS: [Color; 5] = [
    Color::rgb(120, 200, 120),
    Color::rgb(235, 170, 80),
    Color::rgb(220, 120, 200),
    Color::rgb(100, 205, 205),
    Color::rgb(230, 230, 130),
];

/// What the view says before anything is cut.
pub const NO_SECTION_MESSAGE: &str =
    "No section yet.\nSelect a line or polyline, then Terrain > Cut Section Along Selection.";

/// What the legend says of a section whose surfaces changed since the cut.
pub const SURFACES_CHANGED_MESSAGE: &str =
    "Surfaces changed since the cut: not cut again, see the log";

/// The smallest the view may be made (QWidget::setMinimumSize(80, 60)).
pub const MINIMUM_WIDTH: i32 = 80;
pub const MINIMUM_HEIGHT: i32 = 60;

/// A widget's size before anything has sized it: Qt's, for a widget never shown.
const UNSIZED_WIDTH: i32 = 640;
const UNSIZED_HEIGHT: i32 = 480;

/// The double click's time and distance, as the plan view reads them from raw presses.
const DOUBLE_CLICK_SECONDS: f64 = 0.4;
const DOUBLE_CLICK_PIXELS: f64 = 5.0;
/// Points of egui's smooth scroll that make one notch.
const WHEEL_POINTS_A_NOTCH: f64 = 50.0;

/// The plot area - inside the axes and their labels - of a view this size. Not clamped: resize
/// takes the difference of two of these, and a clamp would make that difference wrong for a view
/// smaller than its margins.
fn plot_size(width: i32, height: i32) -> (f64, f64) {
    (
        f64::from(width - LEFT_MARGIN - RIGHT_MARGIN),
        f64::from(height - TOP_MARGIN - BOTTOM_MARGIN),
    )
}

/// What the C++ view raised through its hooks, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum SectionViewEvent {
    /// The view was clicked, or the user moved the keyboard focus into it (view_focus).
    Activated,
    /// What the view shows now: after every paint the length and the crossings ("Section  length
    /// 120.00  7 crossings, 2 hidden in this view, 1 selected" - a ghost counts as selected), and
    /// on a mouse move the station and elevation under the cursor (onFrameStats).
    FrameStats(String),
}

/// How the view marks a crossing: its entity selected and drawn here, a ghost of one selected
/// where only this view hides it (with the view's switch on), or not at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    None,
    Selected,
    Ghost,
}

/// The egui adaptor's own state.
#[derive(Default)]
struct EguiSide {
    texture: Option<egui::TextureHandle>,
    held: Vec<MouseButton>,
    last_press: Option<(f64, PointF, MouseButton)>,
}

/// See the module comment.
pub struct SectionView {
    // First, so it is dropped first.
    registration: ListenerHandle,
    notifications: Arc<AtomicU64>,
    seen_notifications: u64,
    width: i32,
    height: i32,
    device_ratio: f64,
    /// Pixels per model unit, horizontally; vertically it is this times the exaggeration.
    scale: f64,
    /// The station at the left edge of the plot area.
    origin_station: f64,
    /// The elevation at its bottom edge.
    origin_elevation: f64,
    /// False until the section has been framed once at a real size. The first paint frames it.
    /// After that a resize frames it again at the new size until the user pans or zooms
    /// (`user_moved`), and from then on keeps their scale and the middle of what they were
    /// looking at.
    framed: bool,
    user_moved: bool,
    /// The size the last resize left this view at, which a resize measures from: a view resized
    /// while hidden is sent its resize when it is shown again, with no old size.
    last_size: Option<(i32, i32)>,
    last_drawn_crossings: usize,
    last_hidden_crossings: usize,
    last_selected_crossings: usize,
    last_ghost_crossings: usize,
    panning: bool,
    last_mouse: PointF,
    dirty: bool,
    picture: Image,
    events: Vec<SectionViewEvent>,
    egui: EguiSide,
}

impl Default for SectionView {
    fn default() -> Self {
        Self::new()
    }
}

impl SectionView {
    pub fn new() -> Self {
        Self {
            registration: ListenerHandle::default(),
            notifications: Arc::new(AtomicU64::new(0)),
            seen_notifications: 0,
            width: UNSIZED_WIDTH,
            height: UNSIZED_HEIGHT,
            device_ratio: 1.0,
            scale: 1.0,
            origin_station: 0.0,
            origin_elevation: 0.0,
            framed: false,
            user_moved: false,
            last_size: None,
            last_drawn_crossings: 0,
            last_hidden_crossings: 0,
            last_selected_crossings: 0,
            last_ghost_crossings: 0,
            panning: false,
            last_mouse: PointF::default(),
            dirty: true,
            picture: Image::default(),
            events: Vec::new(),
            egui: EguiSide::default(),
        }
    }

    /// Listens to `document`, whose selection this view marks on its crossings: a selection
    /// change, and a layer shown or hidden in the drawing, change which crossings are marked;
    /// the section itself is cut again by the window when the surfaces change, so this only asks
    /// for a frame. None stops listening.
    pub fn set_document(&mut self, document: Option<&Document>) {
        self.registration = ListenerHandle::default();
        if let Some(document) = document {
            let counter = Arc::clone(&self.notifications);
            self.registration = document.add_listener(move || {
                counter.fetch_add(1, Ordering::AcqRel);
            });
        }
        self.dirty = true;
    }

    /// What the view has raised since the last call of this, in order.
    pub fn take_events(&mut self) -> Vec<SectionViewEvent> {
        std::mem::take(&mut self.events)
    }

    /// The view's size in its own pixels.
    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    /// The device pixels a pixel of the view is: the picture is drawn at them.
    pub fn set_device_ratio(&mut self, ratio: f64) {
        if ratio.is_finite() && ratio > 0.0 && ratio != self.device_ratio {
            self.device_ratio = ratio;
            self.dirty = true;
        }
    }

    /// Whether a frame has been asked for since the last paint.
    pub fn needs_frame(&self) -> bool {
        self.dirty || self.notifications.load(Ordering::Acquire) != self.seen_notifications
    }

    /// The last paint.
    pub fn picture(&self) -> &Image {
        &self.picture
    }

    /// Replaces what is shown. An empty section draws the "no section" message rather than an
    /// empty grid, so it is obvious nothing has been cut yet.
    pub fn set_section(
        &mut self,
        state: &mut ViewState,
        document: Option<&Document>,
        section: Section,
    ) {
        state.section = Some(section);
        // A new cut has no design until its caller says it has (the workspace's show_section):
        // the last one's must not ride into it when it is cut again.
        state.section_design = None;
        // Cut from the drawing's surfaces as they are now: the window reads them just before it
        // hands the section over.
        state.section_surfaces = document.map_or(0, Document::surfaces_generation);
        self.framed = false;
        self.dirty = true;
    }

    /// Replaces the section with the same one cut again, from the drawing's surfaces as they are
    /// now: what is drawn changes, and the pan and zoom a person made stay as they were - the
    /// station and elevation at the middle of the plot keep their place. A view not moved by hand
    /// is still fitted to all of its section, so it is framed afresh: a surface raised by ten
    /// metres moves the extent it fits.
    pub fn replace_section(
        &mut self,
        state: &mut ViewState,
        document: Option<&Document>,
        section: Section,
    ) {
        state.section = Some(section);
        state.section_surfaces = document.map_or(0, Document::surfaces_generation);
        // A view whose pan or zoom a person set keeps it (`framed` stays true, so the next paint
        // does not frame it). One they have not touched fits the whole of its section, and the
        // new section's extent is not the old one's.
        if !self.user_moved {
            self.framed = false;
        }
        self.dirty = true;
    }

    pub fn clear_section(&mut self, state: &mut ViewState) {
        state.section = None;
        state.section_design = None;
        self.framed = false;
        self.dirty = true;
    }

    pub fn has_section(state: &ViewState) -> bool {
        state.section.is_some()
    }

    /// True when the drawing's surfaces have changed since the section was cut - hidden, shown,
    /// edited, removed, undone - so what is drawn is of surfaces it no longer has. The window
    /// cuts an open section again as the surfaces change, but cannot when no surface is left to
    /// cut or the cut is refused: then the view says so, in its legend and in its frame stats,
    /// rather than showing the old picture as the drawing's.
    pub fn surfaces_changed(state: &ViewState, document: Option<&Document>) -> bool {
        state.section.is_some()
            && document.is_some_and(|document| {
                document.surfaces_generation() != state.section_surfaces
            })
    }

    /// Vertical exaggeration. A profile at 1:1 over a kilometre is a flat line, so 10 is the
    /// usual default for a long section. A factor that is not finite and above 0 does nothing.
    pub fn set_vertical_exaggeration(&mut self, state: &mut ViewState, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        state.section_exaggeration = factor;
        self.framed = false; // the drawing's proportions changed; reframe it
        self.dirty = true;
    }

    pub fn vertical_exaggeration(state: &ViewState) -> f64 {
        state.section_exaggeration
    }

    /// Crossings the last paint drew: those whose layer this view does not hide and whose station
    /// was in view.
    pub fn last_drawn_crossing_count(&self) -> usize {
        self.last_drawn_crossings
    }

    /// Crossings the last paint left out because this view hides their layer - the layer their
    /// entity is on now (layer_of).
    pub fn last_hidden_crossing_count(&self) -> usize {
        self.last_hidden_crossings
    }

    /// Crossings the last paint drew as selected: a selected entity this view draws.
    pub fn last_selected_crossing_count(&self) -> usize {
        self.last_selected_crossings
    }

    /// Crossings the last paint drew as ghosts (ViewState::selection_ghosts): a selected entity
    /// the document draws and this view hides.
    pub fn last_ghost_crossing_count(&self) -> usize {
        self.last_ghost_crossings
    }

    // ---- station and elevation to pixels ------------------------------------------------------

    fn to_screen(&self, state: &ViewState, station: f64, elevation: f64) -> PointF {
        let x = f64::from(LEFT_MARGIN) + (station - self.origin_station) * self.scale;
        let y = f64::from(self.height - BOTTOM_MARGIN)
            - (elevation - self.origin_elevation) * self.scale * state.section_exaggeration;
        PointF::new(x, y)
    }

    fn station_at(&self, x: f64) -> f64 {
        self.origin_station + (x - f64::from(LEFT_MARGIN)) / self.scale
    }

    fn elevation_at(&self, state: &ViewState, y: f64) -> f64 {
        self.origin_elevation
            + (f64::from(self.height - BOTTOM_MARGIN) - y)
                / (self.scale * state.section_exaggeration)
    }

    /// Station and elevation under a point of this view, in its own pixels: the live readout, and
    /// what a headless check asks to prove that a resize kept the drawing where the user had put
    /// it. As a point: x the station, y the elevation.
    pub fn station_elevation_at(&self, state: &ViewState, pixel: PointF) -> PointF {
        PointF::new(self.station_at(pixel.x), self.elevation_at(state, pixel.y))
    }

    /// The middle of the plot area - the part inside the axes - in pixels.
    pub fn plot_centre(&self) -> PointF {
        let (wide, high) = plot_size(self.width, self.height);
        PointF::new(
            f64::from(LEFT_MARGIN) + 0.5 * wide,
            f64::from(TOP_MARGIN) + 0.5 * high,
        )
    }

    /// Inside the axes and their labels, in pixels.
    fn plot_rect(&self) -> RectF {
        RectF::new(
            f64::from(LEFT_MARGIN),
            f64::from(TOP_MARGIN),
            f64::from(self.width - LEFT_MARGIN - RIGHT_MARGIN),
            f64::from(self.height - TOP_MARGIN - BOTTOM_MARGIN),
        )
    }

    pub fn zoom_extents(&mut self, state: &ViewState) {
        self.frame_extents(state);
        self.dirty = true;
    }

    /// zoom_extents without asking for a frame, for the paint that frames.
    fn frame_extents(&mut self, state: &ViewState) {
        self.framed = true;
        self.user_moved = false;
        self.last_size = Some((self.width, self.height));
        let plot_width = (self.width - LEFT_MARGIN - RIGHT_MARGIN).max(1);
        let plot_height = (self.height - TOP_MARGIN - BOTTOM_MARGIN).max(1);

        let Some(section) = &state.section else {
            self.scale = 1.0;
            self.origin_station = 0.0;
            self.origin_elevation = 0.0;
            return;
        };
        let extent = section.extent();
        if extent.empty() {
            // The alignment missed every surface. Frame the station range anyway so the axis
            // still reads correctly and the gap is visible as a gap.
            self.scale = f64::from(plot_width) / katana_math::cpp::max(section.length, 1.0);
            self.origin_station = 0.0;
            self.origin_elevation = 0.0;
            return;
        }
        let exaggeration = state.section_exaggeration;
        let station_span = katana_math::cpp::max(extent.width(), 1e-6);
        let elevation_span = katana_math::cpp::max(extent.height(), 1e-6);

        // Fit whichever axis is tighter, with the exaggeration already applied to the vertical
        // one so that changing it reframes rather than clipping.
        let horizontal = f64::from(plot_width) / (station_span * 1.04);
        let vertical = f64::from(plot_height) / (elevation_span * exaggeration * 1.15);
        self.scale = katana_math::cpp::max(katana_math::cpp::min(horizontal, vertical), 1e-9);

        // Centre what is left over.
        let used_width = station_span * self.scale;
        let used_height = elevation_span * self.scale * exaggeration;
        self.origin_station =
            extent.min.x - (f64::from(plot_width) - used_width) * 0.5 / self.scale;
        self.origin_elevation = extent.min.y
            - (f64::from(plot_height) - used_height) * 0.5 / (self.scale * exaggeration);
    }

    /// A round number of model units that is at least `minimum_pixels` apart on screen: 1, 2, 5,
    /// 10, 20, 50 ... so gridlines land on values a surveyor would actually write down.
    fn nice_step(&self, state: &ViewState, minimum_pixels: f64, vertical: bool) -> f64 {
        let pixels_per_unit = if vertical {
            self.scale * state.section_exaggeration
        } else {
            self.scale
        };
        if !(pixels_per_unit > 0.0) {
            return 1.0;
        }
        let raw = minimum_pixels / pixels_per_unit;
        let magnitude = 10.0_f64.powf(katana_math::cpp::max(raw, 1e-12).log10().floor());
        for multiple in [1.0, 2.0, 5.0, 10.0] {
            if magnitude * multiple >= raw {
                return magnitude * multiple;
            }
        }
        magnitude * 10.0
    }

    /// Zooms by `factor` (above 1 closer) about `position`, in this view's pixels, keeping the
    /// station and elevation there where they are: the wheel's zoom, and ZOOM IN and OUT's about
    /// plot_centre. A factor that is not finite and above 0 does nothing.
    pub fn zoom_at(&mut self, state: &ViewState, position: PointF, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        // The station and elevation under `position` must not move.
        let station = self.station_at(position.x);
        let elevation = self.elevation_at(state, position.y);

        self.scale = katana_math::cpp::clamp(self.scale * factor, 1e-9, 1e9);

        self.origin_station = station - (position.x - f64::from(LEFT_MARGIN)) / self.scale;
        self.origin_elevation = elevation
            - (f64::from(self.height - BOTTOM_MARGIN) - position.y)
                / (self.scale * state.section_exaggeration);
        self.framed = true;
        self.user_moved = true; // the user's view now, kept on a resize
        self.dirty = true;
    }

    /// QWidget::resizeEvent: the view is `width` by `height` of its own pixels now.
    pub fn resize(&mut self, state: &ViewState, width: i32, height: i32) {
        self.width = width;
        self.height = height;
        self.dirty = true;
        // The size before this resize is the one this view last saw, not the event's old size: a
        // view resized while hidden - a section tabbed behind another view - is sent its resize
        // when it is shown again, with no old size, and the drawing jumped by the whole change.
        let before = self.last_size;
        self.last_size = Some((width, height));
        // A section never framed yet has nothing to keep; its first paint frames it.
        if !self.framed {
            return;
        }
        // One still showing what it was framed to goes on showing all of it, as the plan view
        // does: a view is routinely framed and THEN resized - a 3D view opened beside it takes
        // half its width - and keeping the scale cut both ends of the section off.
        if !self.user_moved {
            self.frame_extents(state);
            return;
        }
        // One the user has panned or zoomed keeps their scale, and the station and elevation at
        // the middle of the plot area stay there, as the plan view's transform keeps its centre:
        // the plot area grows or shrinks about its middle rather than throwing their zoom away.
        let Some((before_width, before_height)) = before else {
            return;
        };
        let previous = plot_size(before_width, before_height);
        let after = plot_size(width, height);
        // The station at the plot's middle is origin + width / (2 scale), and the elevation there
        // origin + height / (2 scale exaggeration); holding each fixed while the width or height
        // changes moves the origin by half the change.
        self.origin_station += 0.5 * (previous.0 - after.0) / self.scale;
        self.origin_elevation +=
            0.5 * (previous.1 - after.1) / (self.scale * state.section_exaggeration);
    }

    // ---- painting -----------------------------------------------------------------------------

    /// QWidget::paintEvent: the view painted at its size now, into `picture`.
    pub fn paint(&mut self, state: &ViewState, document: Option<&Document>) -> &Image {
        self.seen_notifications = self.notifications.load(Ordering::Acquire);
        self.dirty = false;
        // At the first paint, not the first resize: a paint happens at the size the view is seen
        // at, where a dock's first resize may be provisional.
        if !self.framed {
            self.frame_extents(state);
        }
        let wide = ((f64::from(self.width) * self.device_ratio).ceil() as u32).max(1);
        let high = ((f64::from(self.height) * self.device_ratio).ceil() as u32).max(1);
        let mut picture = Image::new(wide, high);
        picture.set_device_pixel_ratio(self.device_ratio);
        let whole = RectF::new(0.0, 0.0, f64::from(self.width), f64::from(self.height));
        RasterCanvas::paint_image(&mut picture, &mut |canvas: &mut dyn Canvas| {
            canvas.set_render_hint(RenderHint::Antialiasing, false);
            canvas.set_font(ui_font());
            canvas.fill_rect(whole, &Brush::solid(BACKGROUND));
            if state.section.is_none() {
                self.last_drawn_crossings = 0;
                self.last_hidden_crossings = 0;
                self.last_selected_crossings = 0;
                self.last_ghost_crossings = 0;
                canvas.set_pen(Pen::new(palette::text_muted()));
                canvas.draw_text_in(
                    whole.adjusted(12.0, 12.0, -12.0, -12.0),
                    TextFlags::CENTER.wrapped(),
                    NO_SECTION_MESSAGE,
                );
                return;
            }
            self.draw_grid(canvas, state);
            self.draw_crossings(canvas, state, document);
            self.draw_surfaces(canvas, state);
            self.draw_selected_crossings(canvas, state, document);
            self.draw_legend(canvas, state, document);
        });
        self.picture = picture;
        if let Some(section) = &state.section {
            let mut text = format!(
                "Section  length {}  {} crossings",
                qt_fixed(section.length, 2),
                section.crossings.len()
            );
            if self.last_hidden_crossings > 0 {
                text.push_str(&format!(
                    ", {} hidden in this view",
                    self.last_hidden_crossings
                ));
            }
            let marked = self.last_selected_crossings + self.last_ghost_crossings;
            if marked > 0 {
                text.push_str(&format!(", {marked} selected"));
            }
            if Self::surfaces_changed(state, document) {
                text.push_str(", surfaces changed since the cut");
            }
            self.events.push(SectionViewEvent::FrameStats(text));
        }
        &self.picture
    }

    fn draw_grid(&self, canvas: &mut dyn Canvas, state: &ViewState) {
        let plot = self.plot_rect();
        if plot.width <= 0.0 || plot.height <= 0.0 {
            return;
        }
        canvas.set_clip_rect(plot.adjusted(-1.0, -1.0, 1.0, 1.0), false);

        let station_step = self.nice_step(state, 70.0, false);
        let elevation_step = self.nice_step(state, 40.0, true);

        let first_station = (self.station_at(plot.left()) / station_step).ceil() * station_step;
        let last_station = self.station_at(plot.right());
        let first_elevation =
            (self.elevation_at(state, plot.bottom()) / elevation_step).ceil() * elevation_step;
        let last_elevation = self.elevation_at(state, plot.top());

        canvas.set_pen(Pen::with(
            GRID,
            1.0,
            PenStyle::Solid,
            PenCap::Square,
            PenJoin::Bevel,
        ));
        // Bounded so a wildly zoomed-out view cannot ask for a hundred thousand lines; nice_step
        // already keeps the count near the plot size, and this is the belt to its braces.
        let mut drawn = 0;
        let mut s = first_station;
        while s <= last_station && drawn < 500 {
            let x = self.to_screen(state, s, 0.0).x;
            canvas.draw_line(PointF::new(x, plot.top()), PointF::new(x, plot.bottom()));
            s += station_step;
            drawn += 1;
        }
        drawn = 0;
        let mut e = first_elevation;
        while e <= last_elevation && drawn < 500 {
            let y = self.to_screen(state, 0.0, e).y;
            let colour = if e.abs() < elevation_step * 0.5 {
                GRID_MAJOR
            } else {
                GRID
            };
            canvas.set_pen(Pen::with(
                colour,
                1.0,
                PenStyle::Solid,
                PenCap::Square,
                PenJoin::Bevel,
            ));
            canvas.draw_line(PointF::new(plot.left(), y), PointF::new(plot.right(), y));
            e += elevation_step;
            drawn += 1;
        }

        canvas.set_clipping(false);
        canvas.set_pen(Pen::new(AXIS));
        canvas.draw_line(plot.bottom_left(), plot.bottom_right());
        canvas.draw_line(plot.top_left(), plot.bottom_left());

        canvas.set_pen(Pen::new(TEXT));
        let original = canvas.font();
        let points = match original.size {
            FontSize::Points(points) => points,
            FontSize::Pixels(pixels) => f64::from(pixels) * 72.0 / 96.0,
        };
        let small = Font {
            size: FontSize::Points(katana_math::cpp::max(points - 1.0, 6.0)),
            ..original.clone()
        };
        canvas.set_font(small);

        drawn = 0;
        let mut s = first_station;
        while s <= last_station && drawn < 200 {
            let x = self.to_screen(state, s, 0.0).x;
            canvas.draw_text_in(
                RectF::new(
                    x - 40.0,
                    plot.bottom() + 3.0,
                    80.0,
                    f64::from(BOTTOM_MARGIN) - 4.0,
                ),
                TextFlags::aligned(HAlign::HCenter, VAlign::Top),
                &qt_fixed(s, 1),
            );
            s += station_step;
            drawn += 1;
        }
        drawn = 0;
        let mut e = first_elevation;
        while e <= last_elevation && drawn < 200 {
            let y = self.to_screen(state, 0.0, e).y;
            canvas.draw_text_in(
                RectF::new(2.0, y - 8.0, f64::from(LEFT_MARGIN) - 8.0, 16.0),
                TextFlags::aligned(HAlign::Right, VAlign::VCenter),
                &qt_fixed(e, 2),
            );
            e += elevation_step;
            drawn += 1;
        }
        canvas.set_font(original);
    }

    fn draw_surfaces(&self, canvas: &mut dyn Canvas, state: &ViewState) {
        let Some(section) = &state.section else {
            return;
        };
        canvas.set_clip_rect(self.plot_rect(), false);
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        for (s, surface) in section.surfaces.iter().enumerate() {
            let colour = SURFACE_COLORS[s % SURFACE_COLORS.len()];
            canvas.set_pen(Pen::with(
                colour,
                1.8,
                PenStyle::Solid,
                PenCap::Square,
                PenJoin::Bevel,
            ));
            // Broken into runs at every gap, so a stretch where the alignment left the surface is
            // a break in the line rather than a straight segment across ground that was never
            // measured.
            let mut run: Vec<PointF> = Vec::new();
            for sample in &surface.samples {
                let Some(elevation) = sample.elevation else {
                    if run.len() > 1 {
                        canvas.draw_polyline(&run);
                    }
                    run.clear();
                    continue;
                };
                run.push(self.to_screen(state, sample.station, elevation));
            }
            if run.len() > 1 {
                canvas.draw_polyline(&run);
            }

            // Slope breaks marked, because on a profile those are the points that get levelled
            // and set out.
            canvas.set_pen(Pen::with(
                colour,
                1.0,
                PenStyle::Solid,
                PenCap::Square,
                PenJoin::Bevel,
            ));
            canvas.set_brush(Brush::solid(colour));
            for (sample, reason) in surface.samples.iter().zip(&surface.reasons) {
                let Some(elevation) = sample.elevation else {
                    continue;
                };
                if *reason != SampleReason::SurfaceBreak {
                    continue;
                }
                let at = self.to_screen(state, sample.station, elevation);
                canvas.draw_ellipse(RectF::new(at.x - 2.2, at.y - 2.2, 4.4, 4.4));
            }
            canvas.set_brush(Brush::none());
        }
        canvas.set_render_hint(RenderHint::Antialiasing, false);
        canvas.set_clipping(false);
    }

    fn draw_crossings(&mut self, canvas: &mut dyn Canvas, state: &ViewState, document: Option<&Document>) {
        self.last_drawn_crossings = 0;
        self.last_hidden_crossings = 0;
        let Some(section) = &state.section else {
            return;
        };
        if section.crossings.is_empty() {
            return;
        }
        let plot = self.plot_rect();
        canvas.set_clip_rect(plot, false);
        canvas.set_pen(Pen::with(
            CROSSING,
            1.0,
            PenStyle::Dash,
            PenCap::Square,
            PenJoin::Bevel,
        ));
        for crossing in &section.crossings {
            // A crossing is where an entity on a layer cuts the section line, so a layer hidden in
            // this view takes its crossings out of it too. At paint time rather than by cutting
            // the section again: the section is shared, and which layers a view hides changes
            // far more often. The layer the entity is on now, as mark_of reads it (layer_of).
            if state.layers.hides(layer_of(document, crossing)) {
                self.last_hidden_crossings += 1;
                continue;
            }
            let x = self.to_screen(state, crossing.station, 0.0).x;
            if x < plot.left() || x > plot.right() {
                continue;
            }
            self.last_drawn_crossings += 1;
            // A selected one is drawn by draw_selected_crossings, over the surfaces.
            if mark_of(state, document, crossing) != Mark::Selected {
                canvas.draw_line(PointF::new(x, plot.top()), PointF::new(x, plot.bottom()));
            }
        }
        canvas.set_clipping(false);
    }

    /// The selected entities' crossings and their ghosts, after the surfaces so that no line is
    /// drawn over them.
    fn draw_selected_crossings(
        &mut self,
        canvas: &mut dyn Canvas,
        state: &ViewState,
        document: Option<&Document>,
    ) {
        self.last_selected_crossings = 0;
        self.last_ghost_crossings = 0;
        let Some(section) = &state.section else {
            return;
        };
        if document.is_none_or(|document| document.selection().empty()) {
            return;
        }
        let plot = self.plot_rect();
        canvas.set_clip_rect(plot, false);
        let selection = Color::rgb(SELECTION_RED, SELECTION_GREEN, SELECTION_BLUE);
        let faint = selection.with_alpha(u8::try_from(GHOST_ALPHA).unwrap_or(u8::MAX));
        let ghost_pen = Pen::with(
            faint,
            GHOST_CROSSING_PIXELS,
            PenStyle::Dot,
            PenCap::Flat,
            PenJoin::Bevel,
        );
        // Solid where every other crossing is dashed, and twice as wide, with a dot where it has a
        // level: the one the user picked, at a glance.
        let selected_pen = Pen::with(
            selection,
            SELECTED_CROSSING_PIXELS,
            PenStyle::Solid,
            PenCap::Square,
            PenJoin::Bevel,
        );
        canvas.set_brush(Brush::solid(selection));
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        for crossing in &section.crossings {
            let mark = mark_of(state, document, crossing);
            if mark == Mark::None {
                continue;
            }
            let x = self.to_screen(state, crossing.station, 0.0).x;
            if x < plot.left() || x > plot.right() {
                continue;
            }
            if mark == Mark::Ghost {
                canvas.set_pen(ghost_pen.clone());
                canvas.draw_line(PointF::new(x, plot.top()), PointF::new(x, plot.bottom()));
                self.last_ghost_crossings += 1;
                continue;
            }
            canvas.set_pen(selected_pen.clone());
            canvas.draw_line(PointF::new(x, plot.top()), PointF::new(x, plot.bottom()));
            if let Some(elevation) = crossing.elevation {
                let at = self.to_screen(state, crossing.station, elevation);
                canvas.draw_ellipse(RectF::new(
                    at.x - SELECTED_CROSSING_DOT,
                    at.y - SELECTED_CROSSING_DOT,
                    2.0 * SELECTED_CROSSING_DOT,
                    2.0 * SELECTED_CROSSING_DOT,
                ));
            }
            self.last_selected_crossings += 1;
        }
        canvas.set_render_hint(RenderHint::Antialiasing, false);
        canvas.set_brush(Brush::none());
        canvas.set_clipping(false);
    }

    fn draw_legend(&self, canvas: &mut dyn Canvas, state: &ViewState, document: Option<&Document>) {
        let Some(section) = &state.section else {
            return;
        };
        let mut y = TOP_MARGIN + 4;
        canvas.set_pen(Pen::new(TEXT));
        canvas.draw_text(
            PointF::new(f64::from(LEFT_MARGIN + 8), f64::from(y + 10)),
            &format!(
                "L {}   V x{}",
                qt_fixed(section.length, 2),
                qt_fixed(state.section_exaggeration, 1)
            ),
        );
        y += 16;
        for (s, surface) in section.surfaces.iter().enumerate() {
            canvas.set_pen(Pen::with(
                SURFACE_COLORS[s % SURFACE_COLORS.len()],
                2.0,
                PenStyle::Solid,
                PenCap::Square,
                PenJoin::Bevel,
            ));
            canvas.draw_line(
                PointF::new(f64::from(LEFT_MARGIN + 8), f64::from(y + 6)),
                PointF::new(f64::from(LEFT_MARGIN + 26), f64::from(y + 6)),
            );
            canvas.set_pen(Pen::new(TEXT));
            canvas.draw_text(
                PointF::new(f64::from(LEFT_MARGIN + 32), f64::from(y + 10)),
                &surface.name,
            );
            y += 15;
        }
        if Self::surfaces_changed(state, document) {
            canvas.set_pen(Pen::new(palette::error()));
            canvas.draw_text(
                PointF::new(f64::from(LEFT_MARGIN + 8), f64::from(y + 10)),
                SURFACES_CHANGED_MESSAGE,
            );
        }
    }

    // ---- interaction --------------------------------------------------------------------------

    /// Focus has arrived in this view for `reason`, from `previous`: Activated when that is the
    /// user choosing the view (view_focus's rule).
    pub fn focus_arrived(
        &mut self,
        reason: FocusReason,
        previous: Option<FocusPrevious>,
        view_floats: bool,
    ) {
        if focus_chooses_view(reason, previous, view_floats) {
            self.events.push(SectionViewEvent::Activated);
        }
    }

    pub fn mouse_press(&mut self, at: PointF, button: MouseButton, modifiers: KeyModifiers) {
        self.events.push(SectionViewEvent::Activated);
        if button == MouseButton::Middle || (button == MouseButton::Left && modifiers.shift) {
            self.panning = true;
            self.last_mouse = at;
        }
    }

    pub fn mouse_move(&mut self, state: &ViewState, at: PointF) {
        if self.panning {
            let delta = at - self.last_mouse;
            self.last_mouse = at;
            self.origin_station -= delta.x / self.scale;
            self.origin_elevation += delta.y / (self.scale * state.section_exaggeration);
            self.user_moved = true; // the user's view now, kept on a resize
            self.dirty = true;
            return;
        }
        if state.section.is_none() {
            return;
        }
        // Live readout: station and elevation under the cursor is the number someone actually
        // wants off a section.
        self.events.push(SectionViewEvent::FrameStats(format!(
            "Station {}   Elevation {}   (V x{})",
            qt_fixed(self.station_at(at.x), 3),
            qt_fixed(self.elevation_at(state, at.y), 3),
            qt_fixed(state.section_exaggeration, 1)
        )));
    }

    pub fn mouse_release(&mut self) {
        self.panning = false;
    }

    pub fn mouse_double_click(&mut self, state: &ViewState) {
        self.zoom_extents(state);
    }

    /// The wheel turned over `at` by `angle_delta` eighths of a degree, 120 a notch: a zoom about
    /// the cursor. True when the event was taken; a turn of nothing is not.
    pub fn wheel(&mut self, state: &ViewState, at: PointF, angle_delta: f64) -> bool {
        let notches = angle_delta / 120.0;
        if notches == 0.0 {
            return false;
        }
        self.zoom_at(state, at, ZOOM_PER_NOTCH.powf(notches));
        true
    }

    // ---- egui ---------------------------------------------------------------------------------

    /// The view drawn into the rectangle `ui` has left, never smaller than its minimum: the
    /// frame's input turned into the view's events, the view painted when a frame was asked for,
    /// and its picture shown. The caller reads `take_events` after.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ViewState,
        document: Option<&Document>,
    ) -> egui::Response {
        let wanted = ui
            .available_size()
            .max(egui::vec2(MINIMUM_WIDTH as f32, MINIMUM_HEIGHT as f32));
        let (rect, response) = ui.allocate_exact_size(wanted, egui::Sense::click_and_drag());
        self.set_device_ratio(f64::from(ui.ctx().pixels_per_point()));
        let width = rect.width().round() as i32;
        let height = rect.height().round() as i32;
        if (width, height) != (self.width, self.height) {
            self.resize(state, width, height);
        }
        let local =
            |at: egui::Pos2| PointF::new(f64::from(at.x - rect.min.x), f64::from(at.y - rect.min.y));
        let over = response.contains_pointer();
        let (events, now) = ui.input(|input| (input.events.clone(), input.time));
        for event in events {
            match event {
                egui::Event::PointerMoved(at) => {
                    if over || !self.egui.held.is_empty() {
                        self.mouse_move(state, local(at));
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
                            self.egui.last_press = None;
                            self.mouse_double_click(state);
                        } else {
                            self.egui.last_press = Some((now, at, button));
                            self.mouse_press(at, button, KeyModifiers::from_egui(modifiers));
                        }
                    } else if let Some(held) =
                        self.egui.held.iter().position(|&held| held == button)
                    {
                        self.egui.held.remove(held);
                        self.mouse_release();
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
                        let _ = self.wheel(state, local(at), notches * 120.0);
                    }
                }
                _ => {}
            }
        }
        if ui.is_rect_visible(rect) {
            if self.needs_frame() || self.egui.texture.is_none() {
                let picture = self.paint(state, document).to_color_image();
                match &mut self.egui.texture {
                    Some(texture) => texture.set(picture, egui::TextureOptions::NEAREST),
                    None => {
                        self.egui.texture = Some(ui.ctx().load_texture(
                            "sectionView",
                            picture,
                            egui::TextureOptions::NEAREST,
                        ));
                    }
                }
            }
            if let Some(texture) = &self.egui.texture {
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }
        response
    }
}

/// How the view marks `crossing` (Mark).
fn mark_of(state: &ViewState, document: Option<&Document>, crossing: &SectionCrossing) -> Mark {
    let Some(document) = document else {
        return Mark::None;
    };
    if !document.selection().contains(crossing.entity) {
        return Mark::None;
    }
    // The entity as it is now, not as the cut found it: the plan and 3D views' rules
    // (selection.rs), so an entity the document hides - its layer switched off, or it made
    // invisible since the cut - is marked in no view, and one on a layer only this view hides is
    // a ghost.
    let model = document.model();
    let Some(entity) = model.entities.find(crossing.entity) else {
        return Mark::None;
    };
    if is_drawn(model, entity, &state.layers) {
        return Mark::Selected;
    }
    if state.selection_ghosts && is_ghost(model, entity, &state.layers) {
        Mark::Ghost
    } else {
        Mark::None
    }
}

/// The layer a crossing is on NOW: its entity's, which mark_of reads too, so whether this view
/// hides a crossing and how it marks its selection go by one layer - the cut's, read by the one
/// and not the other, drew an entity moved onto a hidden layer both plain and as a ghost. The
/// cut's when the entity has gone since, or there is no document.
fn layer_of<'c>(document: Option<&'c Document>, crossing: &'c SectionCrossing) -> &'c str {
    if let Some(document) = document
        && let Some(entity) = document.model().entities.find(crossing.entity)
    {
        return &entity.layer;
    }
    &crossing.layer
}
