//! A sheet, painted: the frame and title block, and every viewport on it (docs/plotting.md, "The
//! sheet painter"). The port of sheet_painter.
//!
//! The model - cad's plotting - says what a sheet holds; this draws it, for the sheet editor on
//! screen and for a PDF on paper, through one function, so the two cannot disagree about anything
//! but the resolution.
//!
//! ```text
//! the frame      its lines at their measured weights and dashes, round caps; its texts in Arial
//!                with the 0.82 x factor applied as a canvas scale (never a font stretch), sized
//!                by cap height, fields filled from resolve_fields and centred in their cells; the
//!                legend glyphs; the user's logo fitted to its slot. The construction guide only
//!                when asked (the editor).
//! Plan           the drawing through paint_plan, at the viewport's scale and rotation, clipped to
//!                its rectangle, with its match lines.
//! KeyPlan        the same, faded, with the other sheets' outlines numbered.
//! LongSection    ground and design along an alignment, a grid, level and chainage labels and a
//! CrossSections  data band; cut once and cached; cut and fill shaded, crossings noted with level
//!                and depth, labels placed clear of each other; auto_scale fitted
//!                (resolve_section_viewport). Drawn by plotting::section_painter through this
//!                painter's pens.
//! Model3D        the 3D view's renderer, as a raster capped in resolution.
//! Legend         what the sheet's plans show (cad's legend), a sample each as it prints
//!                (plotting::legend_painter).
//! Notes, Image   text wrapped to the rectangle; a project image fitted.
//! ```
//!
//! North arrows, scale bars and titles are sized in paper millimetres.
//!
//! PAPER COORDINATES are millimetres from the bottom-left of the paper, Y up (cad's frame). The
//! device is Y down: SheetPaintOptions says where the paper's top-left corner is on it and how
//! many device pixels a millimetre is, and every paper point goes through that one mapping.
//!
//! Like paint_plan this reads no control and keeps what it learns in a cache the caller owns, so a
//! sheet set can be plotted off the window's thread.
//!
//! WHAT DIFFERS FROM THE C++, and why. A capital's height is measured as the ink of an "H"
//! (cap_height below): the canvas's FontMetrics has no cap height, and that is the seam the style
//! painter already uses. The PDF's title and creator are not written: the PDF document of the
//! canvas has no place for them yet (recorded for the integrator).

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(a > b) is kept: it rejects NaN, which a > b alone would let through."
)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path as FilePath, PathBuf};
use std::sync::Arc;

use katana_cad::document::Document;
use katana_cad::plot::{
    PlotSettings, millimetres_to_pixels, paper_dimensions, sheet_scale_at_least,
};
use katana_cad::plotting::frame::{
    Frame, FrameCell, FrameRole, HorizontalJustify, VerticalJustify, fit_image, frame_for,
};
use katana_cad::plotting::key_plan::{KeyPlanOutline, fit_key_plan, key_plan_outlines};
use katana_cad::plotting::legend::{LegendMetrics, layout_legend};
use katana_cad::plotting::plan_grid::{
    PlanGrid, PlanGridOptions, PlanPlacement, plan_grid, plan_world_to_paper,
};
use katana_cad::plotting::section_fit::SectionFit;
use katana_cad::plotting::sheet_set::{
    FieldContext, GridStyle, Sheet, SheetSet, Viewport, ViewportKind, WorldMarkKind,
    automatic_title, expand_template, mark_label, resolve_fields,
};
use katana_cad::plotting::tables::{TableLayout, layout_viewport_table};
use katana_cad::scene::{SceneBuilder, SceneLayers, SceneOptions, SceneSurface, render_layers};
use katana_cad::section::{
    Section, SectionOptions, SectionSurfaceInput, append_design_profile, extract_section,
};
use katana_cad::view_transform::ViewTransform;
use katana_core::error::make_error;
use katana_core::path_text::generic_path_utf8;
use katana_core::text::{format_fixed, format_general};
use katana_core::{ErrorCode, Result, Status};
use katana_entity::Color as EntityColor;
use katana_geometry::alignment::solve_alignment;
use katana_geometry::profile::solve_profile;
use katana_geometry::{Box2, Point2, Polyline2};
use katana_math::numerics::{DEG_TO_RAD, PI, RAD_TO_DEG};
use katana_render::{
    Camera, Framebuffer, Rasterizer, RenderOptions, alpha_of, blue_of, green_of, red_of, rgba,
};

use crate::canvas::{
    Brush, Canvas, Color, FillRule, Font, FontMetrics, FontSize, Image, IntRect, PdfCanvas,
    PdfDocument, Pen, PenCap, PenJoin, PenStyle, PointF, RasterCanvas, RectF, RenderHint,
    read_image,
};
use crate::customisation::style_painter::CanvasRasterBackend;
use crate::format::qt_fixed;
use crate::plan_painter::{
    PlanFrame, PlanMedium, PlanPaintCache, PlanPaintOptions, PlanSource, paint_plan,
    pdf_resolution_for, plan_drawn_bounds,
};
use crate::plotting::legend_painter::{LegendSampleContext, gather_legend, paint_legend_sample};
use crate::plotting::plot_style::{plot_fill, plot_image, plot_ink};
use crate::plotting::section_painter::{
    SectionCanvas, SectionCuts, SectionTextStyle, fit_section_viewport, paint_section_viewport,
};
use crate::plotting::sheet_tables::resolved_sheet_set;

// ---- what is drawn, and how ---------------------------------------------------------------------

/// What the sheets are drawn from. Only `plan.model` is required; each other member left empty
/// leaves out only what it provides.
#[derive(Clone, Default)]
pub struct SheetSource<'a> {
    pub plan: PlanSource<'a>,
    /// For the 3D snapshot, whose scene builder reads the document, and the survey code
    /// descriptions a legend is labelled with.
    pub document: Option<&'a Document>,
    /// Surfaces the sections are cut from and the 3D snapshot shows; hidden ones are left out, as
    /// the section view leaves them out.
    pub surfaces: Vec<SceneSurface<'a>>,
    /// Counts every change to the drawing (Document::model_revision): what the cached sections
    /// and snapshots are keyed on.
    pub revision: u64,
    pub fields: FieldContext,
    /// The set's logo, decoded once for every page. Null: the slot is empty.
    pub logo: Image,
    /// Where an Image viewport's file name is looked up.
    pub assets: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SheetPaintOptions {
    /// Paper everywhere but where the editor wants a screen's economies; the editor paints PAPER
    /// too, so what it shows is what plots.
    pub medium: PlanMedium,
    pub pixels_per_millimetre: f64,
    /// The device point the paper's top-left corner lands on.
    pub origin: PointF,
    /// The paper colour rule (white prints black) and the resolution a 3D snapshot is capped at.
    pub plot: PlotSettings,
    /// Editor furniture, never plotted: the construction guide at the paper's edge, and a faint
    /// name in each empty user slot (logo, organisation).
    pub construction: bool,
    pub slot_hints: bool,
    /// The 3D snapshot is rendered at no more than this many dots per inch and this many pixels,
    /// whatever the plot's resolution: an A1 panel at 300 dpi would be 70 megapixels of shaded
    /// terrain in every PDF.
    pub raster_dpi_cap: f64,
    pub raster_pixel_cap: usize,
}

impl Default for SheetPaintOptions {
    fn default() -> Self {
        Self {
            medium: PlanMedium::Paper,
            pixels_per_millimetre: 300.0 / 25.4,
            origin: PointF::new(0.0, 0.0),
            plot: PlotSettings::default(),
            construction: false,
            slot_hints: false,
            raster_dpi_cap: 200.0,
            raster_pixel_cap: 8_000_000,
        }
    }
}

/// What one paint drew, for the editor's status line and for tests.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetPaintStats {
    pub viewports_drawn: usize,
    pub frame_texts_drawn: usize,
    pub plan_entities_drawn: usize,
    pub logo_drawn: bool,
    /// What could not be drawn as asked, one line each. A viewport that could not be drawn at all
    /// ("vp3: no alignment named MC01") is outlined and left empty; a plan that was drawn without
    /// part of a surface's style ("vp1: ground (style Fine): contours: an interval of 0.000001 ...
    /// not drawn - set a larger interval") keeps the rest of its picture.
    pub problems: Vec<String>,
    /// Notes the sections left out for want of room clear of the others in their plots - a
    /// crossing's, the centreline's levels - each dropped rather than drawn over another label or
    /// past its plot.
    pub section_notes_dropped: usize,
}

/// A viewport's scale and centre once "auto" is decided: what is drawn, and what the editor
/// reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedViewport {
    pub scale: f64,
    pub centre: Point2,
}

impl Default for ResolvedViewport {
    fn default() -> Self {
        Self {
            scale: 500.0,
            centre: Point2::default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct CachedSection {
    revision: u64,
    section: Option<Arc<Section>>,
    failure: String,
    start_chainage: f64,
}

#[derive(Clone, Debug, Default)]
struct CachedImage {
    revision: u64,
    image: Image,
}

type Sections = BTreeMap<String, CachedSection>;

/// What the painter keeps between paints: one per editor, one per plotting thread. Sections and
/// snapshots are keyed on the source's revision, so a stale one is never drawn.
#[derive(Default)]
pub struct SheetPaintCache {
    plan: PlanPaintCache,
    sections: Sections,
    snapshots: BTreeMap<String, CachedImage>,
    images: BTreeMap<String, Image>,
    frames: BTreeMap<String, Arc<Result<Frame>>>,
}

impl SheetPaintCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn plan(&mut self) -> &mut PlanPaintCache {
        &mut self.plan
    }

    pub fn clear(&mut self) {
        self.plan.clear();
        self.sections.clear();
        self.snapshots.clear();
        self.images.clear();
        self.frames.clear();
    }
}

// ---- paper --------------------------------------------------------------------------------------

/// Paper millimetres (Y up) onto the device (Y down), for one sheet.
#[derive(Clone, Copy)]
struct Paper<'o> {
    height: f64,
    options: &'o SheetPaintOptions,
}

impl<'o> Paper<'o> {
    fn new(height_mm: f64, options: &'o SheetPaintOptions) -> Self {
        Self {
            height: height_mm,
            options,
        }
    }

    fn at(&self, p: Point2) -> PointF {
        paper_to_device(p, self.height, self.options)
    }

    fn at_box(&self, b: &Box2) -> RectF {
        paper_box_to_device(b, self.height, self.options)
    }

    fn mm(&self, millimetres: f64) -> f64 {
        millimetres * self.options.pixels_per_millimetre
    }

    fn ppmm(&self) -> f64 {
        self.options.pixels_per_millimetre
    }

    fn options(&self) -> &'o SheetPaintOptions {
        self.options
    }
}

fn canvas_colour(c: EntityColor) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a)
}

const INK: Color = Color::rgb(0, 0, 0);
const GRID_INK: Color = Color::rgb(190, 190, 190);
const FAINT: Color = Color::rgb(150, 150, 150);
// A key plan: the other sheets' outlines, and this sheet's own - "you are here".
const OUTLINE_INK: Color = Color::rgb(200, 0, 30);
const OUTLINE_FILL: Color = Color::rgba(200, 0, 30, 18);
const HERE_INK: Color = Color::rgb(0, 80, 180);
const HERE_FILL: Color = Color::rgba(0, 110, 230, 90);

// The plot style (plotting::plot_style) is applied HERE, in the pens, the text setter and
// paper_fill, and nowhere else: every colour the sheet painter puts on paper passes through one of
// them, so a colour mode or a line weight scale cannot miss a line. The paper's white is not a
// colour of the drawing and is used as it is.

/// A pen of `width_mm` on paper, round-capped as the frame is drawn; its colour and width as the
/// plot style prints them.
fn paper_pen(paper: &Paper<'_>, colour: Color, width_mm: f64) -> Pen {
    let plot = &paper.options().plot;
    Pen::with(
        plot_ink(colour, plot),
        katana_math::cpp::max(paper.mm(width_mm * plot.line_weight_scale), 0.01),
        PenStyle::Solid,
        PenCap::Round,
        PenJoin::Round,
    )
}

/// A dashed pen: `pattern_mm` in paper millimetres, flat-capped so that a dash is exactly as long
/// as it says (the canvas measures a pattern in pen widths, so the pattern is divided by the
/// width the style gives the pen).
fn dashed_pen(paper: &Paper<'_>, colour: Color, width_mm: f64, pattern_mm: &[f64]) -> Pen {
    let plot = &paper.options().plot;
    let width = width_mm * plot.line_weight_scale;
    let pattern = pattern_mm
        .iter()
        .map(|length| {
            katana_math::cpp::max(length / katana_math::cpp::max(width, 1e-3), 0.01)
        })
        .collect();
    Pen::with(
        plot_ink(colour, plot),
        katana_math::cpp::max(paper.mm(width), 0.01),
        PenStyle::Custom(pattern),
        PenCap::Flat,
        PenJoin::Round,
    )
}

/// An area of ink - a scale bar's black cells, a north arrow's half, a key plan's tinted sheet -
/// as the plot style prints a fill.
fn paper_fill(paper: &Paper<'_>, colour: Color) -> Brush {
    Brush::solid(plot_fill(colour, &paper.options().plot))
}

/// A light shading the sheet itself lays under something - a table's highlighted row, a key plan's
/// tinted sheets, a section's cut and fill - as the plot style prints it, but without the rule
/// that prints a near-white pen black: that rule is for the drawing's own white linework, and a
/// pale grey shading is pale on purpose.
fn paper_shade(paper: &Paper<'_>, colour: Color) -> Brush {
    let mut plot = paper.options().plot;
    plot.white_to_black = false;
    Brush::solid(plot_fill(colour, &plot))
}

fn rotated(v: Point2, radians: f64) -> Point2 {
    let c = radians.cos();
    let s = radians.sin();
    Point2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

// ---- text ---------------------------------------------------------------------------------------

// Text is set in a font at a reference pixel size and scaled to its exact cap height, so a 1.2 mm
// label is 1.2 mm at 150 dpi and at 600 - no font size is ever rounded to a whole device pixel.
const REFERENCE_PIXELS: i32 = 200;
/// When a font reports no cap height.
const ARIAL_CAP_RATIO: f64 = 0.716;

#[derive(Clone, Debug, PartialEq)]
struct TextStyle {
    cap_mm: f64,
    x_factor: f64,
    horizontal: HorizontalJustify,
    vertical: VerticalJustify,
    angle_degrees: f64,
    /// 0: 1.5 x the cap height.
    line_spacing_mm: f64,
    bold: bool,
    colour: Color,
    face: String,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            cap_mm: 2.0,
            x_factor: 1.0,
            horizontal: HorizontalJustify::Left,
            vertical: VerticalJustify::Bottom,
            angle_degrees: 0.0,
            line_spacing_mm: 0.0,
            bold: false,
            colour: INK,
            face: String::from("Arial"),
        }
    }
}

/// A face at the reference size, and its capital's height there.
#[derive(Clone)]
struct Measured {
    font: Font,
    cap: f64,
}

/// The height of a capital above the baseline (QFontMetricsF::capHeight). The canvas's FontMetrics
/// has no cap height; a capital H's ink is the measure Qt itself falls back to where a font gives
/// none, and the one the style painter uses.
fn cap_height(metrics: &dyn FontMetrics, font: &Font) -> f64 {
    -metrics.bounding_rect(font, "H").top()
}

/// Sets text on the paper. It holds no canvas: a width is asked of a canvas's metrics and a text
/// drawn on a canvas handed in, so the layouts that only measure (a table's, a grid's labels, a
/// section's) can hold it while the painter holds the canvas.
struct TextSetter<'o> {
    paper: Paper<'o>,
    fonts: RefCell<BTreeMap<String, Measured>>,
}

impl<'o> TextSetter<'o> {
    fn new(paper: Paper<'o>) -> Self {
        Self {
            paper,
            fonts: RefCell::new(BTreeMap::new()),
        }
    }

    fn measured(&self, metrics: &dyn FontMetrics, style: &TextStyle) -> Measured {
        let key = format!("{}{}", style.face, if style.bold { "|b" } else { "|" });
        if let Some(found) = self.fonts.borrow().get(&key) {
            return found.clone();
        }
        // The C++ also asks for no hinting and for kerning: the canvas's text is never hinted and
        // always kerned.
        let font = Font::family(&style.face, FontSize::Pixels(REFERENCE_PIXELS)).bold(style.bold);
        let cap = cap_height(metrics, &font);
        let measured = Measured {
            font,
            cap: if cap > 0.0 {
                cap
            } else {
                ARIAL_CAP_RATIO * f64::from(REFERENCE_PIXELS)
            },
        };
        self.fonts.borrow_mut().insert(key, measured.clone());
        measured
    }

    fn scale_for(&self, measured: &Measured, style: &TextStyle) -> f64 {
        self.paper.mm(style.cap_mm) / measured.cap
    }

    /// The width of one line, in paper millimetres.
    fn width_mm(&self, metrics: &dyn FontMetrics, line: &str, style: &TextStyle) -> f64 {
        let m = self.measured(metrics, style);
        metrics.horizontal_advance(&m.font, line) * self.scale_for(&m, style) * style.x_factor
            / self.paper.ppmm()
    }

    /// Draws `text` (lines split on '\n') anchored at `anchor`; later lines step down, whatever
    /// the justification, as the frame's rule is.
    fn draw(
        &self,
        canvas: &mut dyn Canvas,
        anchor: Point2,
        text: &str,
        style: &TextStyle,
        squeeze: f64,
    ) {
        if text.is_empty() {
            return;
        }
        let m = self.measured(canvas.metrics(), style);
        let k = self.scale_for(&m, style);
        let cap = m.cap;
        let spacing = if style.line_spacing_mm > 0.0 {
            style.line_spacing_mm
        } else {
            1.5 * style.cap_mm
        };
        canvas.save();
        canvas.set_font(m.font.clone());
        canvas.set_pen(Pen::new(plot_ink(style.colour, &self.paper.options().plot)));
        canvas.set_brush(Brush::none());
        let at = self.paper.at(anchor);
        canvas.translate(at.x, at.y);
        canvas.rotate(-style.angle_degrees);
        for (i, line) in text.split('\n').enumerate() {
            let width = canvas.metrics().horizontal_advance(&m.font, line);
            let x = match style.horizontal {
                HorizontalJustify::Centre => -0.5 * width,
                HorizontalJustify::Right => -width,
                HorizontalJustify::Left => 0.0,
            };
            let y = match style.vertical {
                VerticalJustify::Middle => 0.5 * cap,
                VerticalJustify::Top => cap,
                VerticalJustify::Bottom => 0.0,
            };
            canvas.save();
            canvas.translate(0.0, self.paper.mm(i as f64 * spacing));
            canvas.scale(k * style.x_factor * squeeze, k);
            canvas.draw_text(PointF::new(x, y), line);
            canvas.restore();
        }
        canvas.restore();
    }

    /// Words of `text` broken into lines no wider than `width_mm`.
    fn wrap(
        &self,
        metrics: &dyn FontMetrics,
        text: &str,
        style: &TextStyle,
        width_mm: f64,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            for word in paragraph.split(' ').filter(|word| !word.is_empty()) {
                let candidate = if line.is_empty() {
                    String::from(word)
                } else {
                    format!("{line} {word}")
                };
                if !line.is_empty() && self.width_mm(metrics, &candidate, style) > width_mm {
                    out.push(std::mem::replace(&mut line, String::from(word)));
                } else {
                    line = candidate;
                }
            }
            out.push(line);
        }
        out
    }
}

/// A white box behind a label drawn over a drawing, so it reads.
fn knock_out(canvas: &mut dyn Canvas, paper: &Paper<'_>, b: &Box2) {
    canvas.fill_rect(paper.at_box(b), &Brush::solid(Color::WHITE));
}

// ---- the frame's legend glyphs ------------------------------------------------------------------
//
// The frame names its legend symbols by style; the glyphs are the owner's app's, drawn in a -1..1
// box (Y down, as it drew them) scaled by the symbol's size, with a stroke of 0.18 mm whatever the
// size.

fn glyph(p: &mut dyn Canvas, style: &str, ink: Color, stroke: f64) {
    let key = style.to_ascii_lowercase();
    p.set_pen(Pen::with(
        ink,
        stroke,
        PenStyle::Solid,
        PenCap::Round,
        PenJoin::Round,
    ));
    p.set_brush(Brush::none());
    fn circle(p: &mut dyn Canvas, ink: Color, cx: f64, cy: f64, r: f64, fill: bool) {
        p.set_brush(if fill {
            Brush::solid(ink)
        } else {
            Brush::none()
        });
        p.draw_ellipse(RectF::new(cx - r, cy - r, 2.0 * r, 2.0 * r));
        p.set_brush(Brush::none());
    }
    fn rect(p: &mut dyn Canvas, hw: f64, hh: f64) {
        p.draw_rect(RectF::new(-hw, -hh, 2.0 * hw, 2.0 * hh));
    }
    fn line(p: &mut dyn Canvas, x1: f64, y1: f64, x2: f64, y2: f64) {
        p.draw_line(PointF::new(x1, y1), PointF::new(x2, y2));
    }
    fn triangle(p: &mut dyn Canvas, ink: Color, points: [(f64, f64); 3]) {
        p.set_brush(Brush::solid(ink));
        let points: Vec<PointF> = points.iter().map(|&(x, y)| PointF::new(x, y)).collect();
        p.draw_polygon(&points, FillRule::OddEven);
        p.set_brush(Brush::none());
    }
    fn meter(p: &mut dyn Canvas) {
        rect(p, 1.0, 1.0);
        let m = [
            PointF::new(-0.5, 0.5),
            PointF::new(-0.5, -0.5),
            PointF::new(0.0, 0.1),
            PointF::new(0.5, -0.5),
            PointF::new(0.5, 0.5),
        ];
        p.draw_polyline(&m);
    }
    match key.as_str() {
        "watr hydrant" => {
            circle(p, ink, 0.0, 0.0, 1.0, false);
            line(p, -1.0, 0.0, 1.0, 0.0);
            line(p, 0.0, -1.0, 0.0, 1.0);
        }
        "watr stop valve" => {
            triangle(p, ink, [(-1.0, -0.85), (-1.0, 0.85), (0.0, 0.0)]);
            triangle(p, ink, [(1.0, -0.85), (1.0, 0.85), (0.0, 0.0)]);
        }
        "watr meter" | "gas meter" => meter(p),
        "watr air valve" => {
            circle(p, ink, 0.0, 0.0, 0.55, false);
            line(p, 0.0, -0.55, 0.0, -1.0);
            line(p, -0.5, -1.0, 0.5, -1.0);
        }
        "sewr manhole cover" => {
            rect(p, 1.0, 1.0);
            circle(p, ink, 0.0, 0.0, 0.66, false);
        }
        "comm telephone single concrete pit" => rect(p, 1.0, 0.72),
        "comm telephone twin concrete pit" => {
            rect(p, 1.0, 0.72);
            line(p, 0.0, -0.72, 0.0, 0.72);
        }
        "comm telephone distribution pillar" => {
            rect(p, 0.62, 1.0);
            p.fill_rect(RectF::new(-0.62, -1.0, 1.24, 0.34), &Brush::solid(ink));
        }
        "comm telephone large sump" => {
            rect(p, 1.15, 0.85);
            line(p, -1.15, 0.85, 1.15, -0.85);
        }
        "elec cable junction box" => {
            rect(p, 1.0, 1.0);
            line(p, -1.0, 1.0, 1.0, -1.0);
        }
        "elec pole - power and light" => {
            circle(p, ink, 0.0, 0.0, 0.42, true);
            line(p, 0.0, 0.0, 1.15, 0.0);
            line(p, 1.15, -0.32, 1.15, 0.32);
            line(p, 0.0, 0.0, -0.88, 0.0);
            circle(p, ink, -1.12, 0.0, 0.24, false);
        }
        "elec pole - light" => {
            circle(p, ink, 0.0, 0.0, 0.42, true);
            line(p, 0.0, 0.0, 1.08, 0.0);
            circle(p, ink, 1.32, 0.0, 0.26, false);
        }
        "gas valve box" => {
            rect(p, 1.0, 1.0);
            triangle(p, ink, [(-0.6, -0.5), (-0.6, 0.5), (0.0, 0.0)]);
            triangle(p, ink, [(0.6, -0.5), (0.6, 0.5), (0.0, 0.0)]);
        }
        "gas test point" => {
            circle(p, ink, 0.0, 0.0, 1.0, false);
            line(p, -0.5, -0.4, 0.5, -0.4);
            line(p, 0.0, -0.4, 0.0, 0.6);
        }
        // A symbol with no glyph: an open square, as the app drew it.
        _ => rect(p, 0.8, 0.8),
    }
}

// ---- the frame ----------------------------------------------------------------------------------

/// The smallest cell of the frame holding `point`: the room a left-anchored field has to its
/// right.
fn cell_holding(frame: &Frame, point: Point2) -> Option<&FrameCell> {
    let mut best: Option<&FrameCell> = None;
    let mut best_area = 0.0;
    for cell in &frame.cells {
        if cell.id == "drawing_area" || !cell.rect.contains(point) {
            continue;
        }
        let area = cell.rect.width() * cell.rect.height();
        if best.is_none() || area < best_area {
            best = Some(cell);
            best_area = area;
        }
    }
    best
}

#[allow(
    clippy::too_many_arguments,
    reason = "The C++ paintFrame's arguments, the canvas first."
)]
fn paint_frame(
    canvas: &mut dyn Canvas,
    paper: &Paper<'_>,
    text: &TextSetter<'_>,
    frame: &Frame,
    sheet: &Sheet,
    fields: &BTreeMap<String, String>,
    source: &SheetSource<'_>,
    stats: &mut SheetPaintStats,
) {
    let options = paper.options();
    let shown = |role: FrameRole| role != FrameRole::Legend || sheet.frame_legend;

    // Lines.
    for line in &frame.polylines {
        if !shown(line.role) || (!line.plots && !options.construction) || line.points.len() < 2 {
            continue;
        }
        let polygon: Vec<PointF> = line.points.iter().map(|point| paper.at(*point)).collect();
        canvas.set_pen(if line.dash_mm > 0.0 && line.gap_mm > 0.0 {
            dashed_pen(
                paper,
                canvas_colour(line.colour),
                line.weight_mm,
                &[line.dash_mm, line.gap_mm],
            )
        } else {
            paper_pen(paper, canvas_colour(line.colour), line.weight_mm)
        });
        canvas.set_brush(Brush::none());
        if line.closed {
            canvas.draw_polygon(&polygon, FillRule::OddEven);
        } else {
            canvas.draw_polyline(&polygon);
        }
    }

    // Legend glyphs.
    if sheet.frame_legend {
        for symbol in &frame.symbols {
            canvas.save();
            let at = paper.at(symbol.at);
            canvas.translate(at.x, at.y);
            canvas.rotate(-symbol.rotation_degrees);
            let size = paper.mm(katana_math::cpp::max(symbol.size_mm, 0.01));
            canvas.scale(size, size);
            glyph(
                canvas,
                &symbol.style,
                plot_ink(canvas_colour(symbol.colour), &options.plot),
                0.18 * options.plot.line_weight_scale / katana_math::cpp::max(symbol.size_mm, 0.01),
            );
            canvas.restore();
        }
    }

    // The logo, in the slot the original organisation's logo was taken out of.
    if let Some(cell) = frame.cell("logo") {
        if !source.logo.is_null() {
            let at = fit_image(
                &cell.rect,
                f64::from(source.logo.width()),
                f64::from(source.logo.height()),
                1.0,
            );
            if !at.empty() {
                canvas.save();
                canvas.set_render_hint(RenderHint::SmoothPixmapTransform, true);
                draw_whole_image(
                    canvas,
                    paper.at_box(&at),
                    &plot_image(&source.logo, &options.plot),
                );
                canvas.restore();
                stats.logo_drawn = true;
            }
        } else if options.slot_hints {
            let hint = TextStyle {
                cap_mm: 2.0 * frame.scale,
                horizontal: HorizontalJustify::Centre,
                vertical: VerticalJustify::Middle,
                colour: FAINT,
                ..TextStyle::default()
            };
            text.draw(canvas, cell.rect.center(), "YOUR LOGO", &hint, 1.0);
        }
    }

    // Texts, fields filled.
    for item in &frame.texts {
        if !shown(item.role) {
            continue;
        }
        let mut style = TextStyle {
            cap_mm: item.cap_height_mm,
            x_factor: item.x_factor,
            horizontal: item.horizontal,
            vertical: item.vertical,
            angle_degrees: item.angle_degrees,
            line_spacing_mm: item.line_spacing_mm,
            bold: item.bold,
            colour: canvas_colour(item.colour),
            face: item.font_face.clone(),
        };

        let mut content = if item.fields.is_empty() {
            item.content.clone()
        } else {
            expand_template(&item.content, fields)
        };
        let mut anchor = item.anchor;
        let mut room = 0.0; // the width the text must fit, when it has a cell

        if !item.centred_in.is_empty() {
            if let Some(cell) = frame.cell(&item.centred_in) {
                anchor = cell.rect.center();
                style.horizontal = HorizontalJustify::Centre;
                style.vertical = VerticalJustify::Middle;
                room = cell.rect.width() - 0.6 * frame.scale;
                content = String::from(content.trim());
            }
        } else if item.role == FrameRole::Slot && item.horizontal == HorizontalJustify::Centre {
            if let Some(cell) = cell_holding(frame, anchor) {
                room = cell.rect.width() - 1.0 * frame.scale;
            }
        } else if item.angle_degrees == 0.0
            && item.horizontal == HorizontalJustify::Left
            && let Some(cell) = cell_holding(frame, anchor)
        {
            room = cell.rect.max.x - anchor.x - 0.4 * frame.scale;
        }

        if content.trim().is_empty() {
            if item.role == FrameRole::Slot
                && options.slot_hints
                && item.fields.len() == 1
                && item.fields[0] == "organisation"
            {
                let hint = TextStyle {
                    colour: FAINT,
                    ..style.clone()
                };
                text.draw(canvas, anchor, "ORGANISATION", &hint, 1.0);
            }
            continue;
        }

        // The notes slot holds a paragraph: wrapped to its cell, as many lines as fit above the
        // cell's foot.
        if item.role == FrameRole::Slot && item.horizontal == HorizontalJustify::Left && room > 0.0
        {
            let mut lines = text.wrap(canvas.metrics(), &content, &style, room);
            if let Some(cell) = cell_holding(frame, anchor) {
                let spacing = if item.line_spacing_mm > 0.0 {
                    item.line_spacing_mm
                } else {
                    1.5 * item.cap_height_mm
                };
                let fit = ((anchor.y - cell.rect.min.y) / spacing).floor() as i64 + 1;
                if lines.len() as i64 > fit {
                    lines.truncate(fit.max(1) as usize);
                }
            }
            text.draw(canvas, anchor, &lines.join("\n"), &style, 1.0);
            stats.frame_texts_drawn += 1;
            continue;
        }

        // A single line wider than its room is squeezed to fit, as the app did, rather than
        // running over the next rule.
        let mut squeeze = 1.0;
        if room > 0.0 && !content.contains('\n') {
            let width = text.width_mm(canvas.metrics(), &content, &style);
            if width > room {
                squeeze = room / width;
            }
        }
        text.draw(canvas, anchor, &content, &style, squeeze);
        stats.frame_texts_drawn += 1;
    }
}

/// QPainter::drawImage(target, image): the whole image into `target`.
fn draw_whole_image(canvas: &mut dyn Canvas, target: RectF, image: &Image) {
    let source = IntRect::new(0, 0, image.width() as i32, image.height() as i32);
    canvas.draw_image(target, image, source);
}

// ---- viewport furniture -------------------------------------------------------------------------

/// A round number of metres at most `maximum`: 1, 2 or 5 times a power of ten.
fn nice_at_most(maximum: f64) -> f64 {
    if !(maximum > 0.0) {
        return 0.0;
    }
    let magnitude = 10.0_f64.powf(maximum.log10().floor());
    for multiple in [5.0, 2.0, 1.0] {
        if magnitude * multiple <= maximum * (1.0 + 1e-9) {
            return magnitude * multiple;
        }
    }
    magnitude
}

/// QString::arg(double) and QString::number(double): six significant figures, as %g.
fn number(value: f64) -> String {
    format_general(value, 6)
}

fn metres(length: f64) -> String {
    if length >= 1000.0 && length % 1000.0 == 0.0 {
        format!("{} km", number(length / 1000.0))
    } else {
        format!("{} m", number(length))
    }
}

// The paper a plan's furniture knocks out white - the scale bar, the north arrow, the title - and
// so what a coordinate grid's labels keep off. Each paint function knocks out exactly its box, so
// the two cannot disagree.
fn scale_bar_box(rect: &Box2, scale: f64) -> Box2 {
    let mm_per_metre = 1000.0 / scale;
    let length_mm =
        nice_at_most(katana_math::cpp::min(50.0, 0.4 * rect.width()) / mm_per_metre) * mm_per_metre;
    let right = rect.max.x - 4.0;
    let base = rect.min.y + 5.0;
    Box2::new(
        Point2::new(right - length_mm - 3.0, base - 4.2),
        Point2::new(right + 3.0, base + 1.2 + 3.2),
    )
}

const NORTH_ARROW_RADIUS: f64 = 4.5;

fn north_arrow_centre(rect: &Box2) -> Point2 {
    Point2::new(
        rect.max.x - NORTH_ARROW_RADIUS - 3.5,
        rect.max.y - NORTH_ARROW_RADIUS - 5.5,
    )
}

fn north_arrow_box(rect: &Box2) -> Box2 {
    let centre = north_arrow_centre(rect);
    Box2::new(
        Point2::new(
            centre.x - NORTH_ARROW_RADIUS - 1.0,
            centre.y - NORTH_ARROW_RADIUS - 1.0,
        ),
        Point2::new(
            centre.x + NORTH_ARROW_RADIUS + 1.0,
            centre.y + NORTH_ARROW_RADIUS + 4.5,
        ),
    )
}

fn paint_scale_bar(
    canvas: &mut dyn Canvas,
    paper: &Paper<'_>,
    text: &TextSetter<'_>,
    rect: &Box2,
    scale: f64,
) {
    let mm_per_metre = 1000.0 / scale;
    let maximum_mm = katana_math::cpp::min(50.0, 0.4 * rect.width());
    let length = nice_at_most(maximum_mm / mm_per_metre);
    if !(length > 0.0) {
        return;
    }
    let length_mm = length * mm_per_metre;
    let first = length / 10.0_f64.powf(length.log10().floor());
    let divisions: i32 = if first.round() as i64 == 5 { 5 } else { 4 };
    let bar_h = 1.2;
    let right = rect.max.x - 4.0;
    let left = right - length_mm;
    let base = rect.min.y + 5.0;

    let mut label = TextStyle {
        cap_mm: 1.6,
        x_factor: 0.9,
        horizontal: HorizontalJustify::Centre,
        ..TextStyle::default()
    };
    knock_out(canvas, paper, &scale_bar_box(rect, scale));
    canvas.set_pen(paper_pen(paper, INK, 0.18));
    for i in 0..divisions {
        let x0 = left + length_mm * f64::from(i) / f64::from(divisions);
        let x1 = left + length_mm * f64::from(i + 1) / f64::from(divisions);
        let cell = paper.at_box(&Box2::new(
            Point2::new(x0, base),
            Point2::new(x1, base + bar_h),
        ));
        canvas.set_brush(if i % 2 == 0 {
            paper_fill(paper, INK)
        } else {
            Brush::solid(Color::WHITE)
        });
        canvas.draw_rect(cell);
    }
    canvas.set_brush(Brush::none());
    text.draw(
        canvas,
        Point2::new(left, base + bar_h + 0.8),
        "0",
        &label,
        1.0,
    );
    text.draw(
        canvas,
        Point2::new(left + length_mm / 2.0, base + bar_h + 0.8),
        &number(length / 2.0),
        &label,
        1.0,
    );
    text.draw(
        canvas,
        Point2::new(right, base + bar_h + 0.8),
        &metres(length),
        &label,
        1.0,
    );
    label.vertical = VerticalJustify::Top;
    text.draw(
        canvas,
        Point2::new((left + right) / 2.0, base - 0.8),
        &format!("SCALE 1:{}", qt_fixed(scale, 0)),
        &label,
        1.0,
    );
}

/// North, drawn where the world's +Y points on the paper.
fn paint_north_arrow(
    canvas: &mut dyn Canvas,
    paper: &Paper<'_>,
    text: &TextSetter<'_>,
    rect: &Box2,
    rotation: f64,
) {
    let radius = NORTH_ARROW_RADIUS;
    let centre = north_arrow_centre(rect);
    let north = PI / 2.0 - rotation; // paper angle of world +Y
    let at =
        |along: f64, across: f64| paper.at(centre + rotated(Point2::new(along, across), north));
    knock_out(canvas, paper, &north_arrow_box(rect));
    canvas.set_pen(paper_pen(paper, INK, 0.25));
    canvas.set_brush(Brush::none());
    let middle = paper.at(centre);
    let r = paper.mm(radius);
    canvas.draw_ellipse(RectF::new(middle.x - r, middle.y - r, 2.0 * r, 2.0 * r));
    let tip = at(radius + 0.8, 0.0);
    let tail = at(-radius + 0.8, 0.0);
    let left = at(-radius * 0.6, radius * 0.45);
    let right = at(-radius * 0.6, -radius * 0.45);
    canvas.set_brush(paper_fill(paper, INK));
    canvas.draw_polygon(&[tip, left, tail], FillRule::OddEven);
    canvas.set_brush(Brush::solid(Color::WHITE));
    canvas.draw_polygon(&[tip, right, tail], FillRule::OddEven);
    canvas.set_brush(Brush::none());
    let n = TextStyle {
        cap_mm: 2.5,
        bold: true,
        horizontal: HorizontalJustify::Centre,
        vertical: VerticalJustify::Middle,
        ..TextStyle::default()
    };
    text.draw(
        canvas,
        centre + rotated(Point2::new(radius + 2.6, 0.0), north),
        "N",
        &n,
        1.0,
    );
}

fn title_style() -> TextStyle {
    TextStyle {
        cap_mm: 2.5,
        bold: true,
        x_factor: 0.9,
        ..TextStyle::default()
    }
}

/// A coordinate grid's labels: small, narrow, in ink.
fn grid_label_style() -> TextStyle {
    TextStyle {
        cap_mm: 1.8,
        x_factor: 0.9,
        ..TextStyle::default()
    }
}

/// What a view is titled: its own title, else the automatic one at `scale`.
fn view_title(viewport: &Viewport, scale: f64) -> String {
    if !viewport.title.is_empty() {
        return viewport.title.clone();
    }
    let mut titled = viewport.clone();
    titled.scale = scale;
    automatic_title(&titled)
}

/// Where the title is set, and the box it knocks out; nothing when it is not drawn (no title, or
/// no room for one).
struct TitlePlace {
    at: Point2,
    width_mm: f64,
    knocked: Box2,
}

fn title_place(
    metrics: &dyn FontMetrics,
    text: &TextSetter<'_>,
    rect: &Box2,
    title: &str,
) -> Option<TitlePlace> {
    if title.is_empty() {
        return None;
    }
    let width = katana_math::cpp::min(
        text.width_mm(metrics, title, &title_style()),
        rect.width() - 6.0,
    );
    if !(width > 0.0) {
        return None;
    }
    let at = Point2::new(rect.min.x + 3.0, rect.min.y + 3.5);
    Some(TitlePlace {
        at,
        width_mm: width,
        knocked: Box2::new(
            Point2::new(at.x - 1.0, at.y - 1.6),
            Point2::new(at.x + width + 1.0, at.y + 3.4),
        ),
    })
}

/// The viewport's name and scale under its bottom-left corner, inside it.
fn paint_title(
    canvas: &mut dyn Canvas,
    paper: &Paper<'_>,
    text: &TextSetter<'_>,
    rect: &Box2,
    title: &str,
) {
    let Some(place) = title_place(canvas.metrics(), text, rect, title) else {
        return;
    };
    let style = title_style();
    let width = place.width_mm;
    let at = place.at;
    knock_out(canvas, paper, &place.knocked);
    let mut squeeze = 1.0;
    let full = text.width_mm(canvas.metrics(), title, &style);
    if full > width {
        squeeze = width / full;
    }
    text.draw(canvas, at, title, &style, squeeze);
    canvas.set_pen(paper_pen(paper, INK, 0.35));
    canvas.draw_line(
        paper.at(Point2::new(at.x, at.y - 0.9)),
        paper.at(Point2::new(at.x + width, at.y - 0.9)),
    );
}

fn paint_message(canvas: &mut dyn Canvas, text: &TextSetter<'_>, rect: &Box2, message: &str) {
    let style = TextStyle {
        cap_mm: 2.0,
        horizontal: HorizontalJustify::Centre,
        vertical: VerticalJustify::Middle,
        colour: FAINT,
        ..TextStyle::default()
    };
    text.draw(canvas, rect.center(), message, &style, 1.0);
}

/// A table laid out by cad's tables - the register, the revisions - drawn exactly where the layout
/// put each rule and text: shading, then rules, then text.
fn paint_table(
    canvas: &mut dyn Canvas,
    paper: &Paper<'_>,
    text: &TextSetter<'_>,
    table: &TableLayout,
) {
    // The highlight is a fill like any other, so the plot style prints it (a light grey drops out
    // of a monochrome plot, as a one-ink plotter's would; the row's rules and text still say which
    // it is).
    let shade = paper_shade(paper, Color::rgb(232, 232, 232));
    for shaded in &table.shaded {
        canvas.fill_rect(paper.at_box(shaded), &shade);
    }
    for rule in &table.rules {
        canvas.set_pen(paper_pen(paper, INK, rule.weight_mm));
        canvas.draw_line(paper.at(rule.from), paper.at(rule.to));
    }
    for item in &table.texts {
        let style = TextStyle {
            cap_mm: item.cap_mm,
            x_factor: item.x_factor,
            horizontal: item.horizontal,
            bold: item.bold,
            colour: if item.muted { FAINT } else { INK },
            ..TextStyle::default()
        };
        text.draw(canvas, item.anchor, &item.text, &style, item.squeeze);
    }
}

// ---- sections -----------------------------------------------------------------------------------

/// The section painter (plotting::section_painter) draws through this sheet's paper, pens and text
/// setter, so a section is styled as the rest.
struct SheetSectionCanvas<'c, 'o> {
    canvas: &'c mut dyn Canvas,
    paper: Paper<'o>,
    text: &'c TextSetter<'o>,
}

fn style_of(from: &SectionTextStyle) -> TextStyle {
    TextStyle {
        cap_mm: from.cap_mm,
        x_factor: from.x_factor,
        horizontal: from.horizontal,
        vertical: from.vertical,
        angle_degrees: from.angle_degrees,
        line_spacing_mm: from.line_spacing_mm,
        bold: from.bold,
        ..TextStyle::default()
    }
}

impl SectionCanvas for SheetSectionCanvas<'_, '_> {
    fn painter(&mut self) -> &mut dyn Canvas {
        &mut *self.canvas
    }

    fn at(&self, paper: Point2) -> PointF {
        self.paper.at(paper)
    }

    fn at_box(&self, paper: &Box2) -> RectF {
        self.paper.at_box(paper)
    }

    fn mm(&self, millimetres: f64) -> f64 {
        self.paper.mm(millimetres)
    }

    fn pen(&self, colour: Color, width_mm: f64) -> Pen {
        paper_pen(&self.paper, colour, width_mm)
    }

    fn dashed_pen(&self, colour: Color, width_mm: f64, pattern_mm: &[f64]) -> Pen {
        dashed_pen(&self.paper, colour, width_mm, pattern_mm)
    }

    fn fill(&self, colour: Color) -> Brush {
        paper_shade(&self.paper, colour)
    }

    fn text_width_mm(&self, line: &str, style: &SectionTextStyle) -> f64 {
        self.text
            .width_mm(self.canvas.metrics(), line, &style_of(style))
    }

    fn text(&mut self, anchor: Point2, text: &str, style: &SectionTextStyle, squeeze: f64) {
        self.text
            .draw(&mut *self.canvas, anchor, text, &style_of(style), squeeze);
    }

    fn knock_out(&mut self, paper: &Box2) {
        knock_out(&mut *self.canvas, &self.paper, paper);
    }
}

fn surface_inputs<'s>(source: &'s SheetSource<'_>) -> Vec<SectionSurfaceInput<'s>> {
    let mut inputs = Vec::new();
    for surface in &source.surfaces {
        if surface.visible && surface.surface.is_some() {
            inputs.push(SectionSurfaceInput {
                name: &surface.name,
                surface: surface.surface,
            });
        }
    }
    inputs
}

/// The long section along alignment `name` and the chainage its station 0 is at, cut once for a
/// revision of the drawing; the failure said when there is none.
fn long_section(
    sections: &mut Sections,
    source: &SheetSource<'_>,
    name: &str,
) -> std::result::Result<(Arc<Section>, f64), String> {
    let entry = sections.entry(format!("L|{name}")).or_default();
    if entry.revision != source.revision || (entry.section.is_none() && entry.failure.is_empty()) {
        *entry = CachedSection {
            revision: source.revision,
            ..CachedSection::default()
        };
        let model = source.plan.model;
        let alignment = model.and_then(|model| model.alignments.find(name));
        match alignment.map(|alignment| (alignment, solve_alignment(&alignment.horizontal))) {
            None => entry.failure = format!("no alignment named {name}"),
            Some((_, Err(error))) => entry.failure = error.describe(),
            Some((alignment, Ok(solved))) => {
                entry.start_chainage = solved.start_station();
                let inputs = surface_inputs(source);
                if inputs.is_empty() && alignment.vertical.is_none() {
                    entry.failure = String::from("no surface to cut and no design profile");
                } else {
                    let line = solved.to_polyline(0.01);
                    let options = SectionOptions {
                        interval: katana_math::cpp::max(line.length() / 2000.0, 0.1),
                        spatial_index: source.plan.index,
                        ..SectionOptions::default()
                    };
                    let mut section = extract_section(&line, &inputs, model, &options);
                    if let (Ok(cut), Some(design)) = (&mut section, &alignment.vertical) {
                        // The profile runs by chainage, the section by distance from the
                        // alignment's start: shifted by the start, the design lies over the ground
                        // however the chainage is numbered.
                        let mut vertical = design.clone();
                        for pvi in &mut vertical.pvis {
                            pvi.station -= entry.start_chainage;
                        }
                        if let Ok(profile) = solve_profile(&vertical)
                            && let Err(error) =
                                append_design_profile(cut, &profile, format!("design {name}"))
                        {
                            entry.failure = error.describe();
                        }
                    }
                    match section {
                        Err(error) => entry.failure = error.describe(),
                        Ok(cut) => {
                            if entry.failure.is_empty() {
                                entry.section = Some(Arc::new(cut));
                            }
                        }
                    }
                }
            }
        }
    }
    match &entry.section {
        Some(section) => Ok((Arc::clone(section), entry.start_chainage)),
        None => Err(entry.failure.clone()),
    }
}

/// The cross section of alignment `name` at `chainage`, `half_width` each side, cut once for a
/// revision of the drawing; the failure said when there is none.
fn cross_section(
    sections: &mut Sections,
    source: &SheetSource<'_>,
    name: &str,
    chainage: f64,
    half_width: f64,
) -> std::result::Result<Arc<Section>, String> {
    let key = format!("X|{name}|{chainage:.6}|{half_width:.6}");
    let entry = sections.entry(key).or_default();
    if entry.revision != source.revision || (entry.section.is_none() && entry.failure.is_empty()) {
        *entry = CachedSection {
            revision: source.revision,
            ..CachedSection::default()
        };
        let model = source.plan.model;
        let alignment = model.and_then(|model| model.alignments.find(name));
        let inputs = surface_inputs(source);
        match alignment {
            None => entry.failure = format!("no alignment named {name}"),
            Some(_) if inputs.is_empty() => {
                entry.failure =
                    String::from("no surface to cut: build one (Terrain > Surface From ...)");
            }
            Some(alignment) => match solve_alignment(&alignment.horizontal) {
                Err(error) => entry.failure = error.describe(),
                Ok(solved) => {
                    let left = solved.point_at_station_offset(chainage, half_width);
                    let right = solved.point_at_station_offset(chainage, -half_width);
                    match (left, right) {
                        (Some(left), Some(right)) => {
                            let across = Polyline2::new(vec![left, right], false);
                            let options = SectionOptions {
                                interval: katana_math::cpp::max(half_width / 100.0, 0.02),
                                spatial_index: source.plan.index,
                                ..SectionOptions::default()
                            };
                            match extract_section(&across, &inputs, model, &options) {
                                Err(error) => entry.failure = error.describe(),
                                Ok(cut) => entry.section = Some(Arc::new(cut)),
                            }
                        }
                        _ => {
                            entry.failure = format!(
                                "chainage {} is off the alignment",
                                format_fixed(chainage, 3)
                            );
                        }
                    }
                }
            },
        }
    }
    match &entry.section {
        Some(section) => Ok(Arc::clone(section)),
        None => Err(entry.failure.clone()),
    }
}

/// The cuts along `alignment`, for the section painter: each asked of the cache `sections` holds.
fn section_cuts<'c>(
    sections: &'c RefCell<&mut Sections>,
    source: &'c SheetSource<'_>,
    alignment: &'c str,
) -> SectionCuts<'c> {
    SectionCuts {
        long_section: Box::new(move || {
            long_section(&mut sections.borrow_mut(), source, alignment)
        }),
        cross_section: Box::new(move |chainage, half_width| {
            cross_section(
                &mut sections.borrow_mut(),
                source,
                alignment,
                chainage,
                half_width,
            )
        }),
        // The design profile's level at a cross section's centreline.
        profile_level: Some(Box::new(move |chainage| {
            let found = source
                .plan
                .model
                .and_then(|model| model.alignments.find(alignment))?;
            let vertical = found.vertical.as_ref()?;
            solve_profile(vertical).ok()?.elevation_at(chainage)
        })),
    }
}

// ---- the painter --------------------------------------------------------------------------------

struct SheetPainter<'a, 's> {
    canvas: &'a mut dyn Canvas,
    set: &'a SheetSet,
    index: usize,
    source: &'a SheetSource<'s>,
    options: &'a SheetPaintOptions,
    cache: &'a mut SheetPaintCache,
    stats: SheetPaintStats,
}

impl SheetPainter<'_, '_> {
    fn problem(&mut self, viewport: &Viewport, what: &str) {
        self.stats.problems.push(format!("{}: {what}", viewport.id));
    }

    fn paint_plan_viewport(
        &mut self,
        viewport: &Viewport,
        text: &TextSetter<'_>,
        paper: &Paper<'_>,
        scale_used: &mut f64,
    ) -> bool {
        if self.source.plan.model.is_none() {
            self.problem(viewport, "no drawing to show");
            return false;
        }
        let at = resolve_plan_viewport(viewport, self.source);
        *scale_used = at.scale;
        let device = paper.at_box(&viewport.rect);
        let mut transform = ViewTransform {
            center: at.centre,
            scale: (paper.ppmm() * 1000.0 / at.scale)
                .clamp(ViewTransform::MINIMUM_SCALE, ViewTransform::MAXIMUM_SCALE),
            ..ViewTransform::default()
        };
        transform.resize(device.width, device.height);
        let frame = PlanFrame {
            transform,
            origin: device.top_left(),
            rotation: -viewport.rotation,
            clip: true,
            layers: Some(&viewport.hidden_layers),
            ..PlanFrame::default()
        };
        let plan = PlanPaintOptions {
            medium: self.options.medium,
            pixels_per_millimetre: paper.ppmm(),
            plot: Some(&self.options.plot),
            // Imagery capped as the 3D snapshot is.
            raster_dpi_cap: self.options.raster_dpi_cap,
            font_family: String::from("Arial"),
            // Paper-sized annotation at this viewport's own scale (docs/annotation.md).
            annotation_scale: at.scale,
            // Annotation text measured by the fonts it is drawn in, as the C++ measures it.
            raster: Some(Arc::new(CanvasRasterBackend)),
            ..PlanPaintOptions::default()
        };
        let drawn = paint_plan(
            &mut *self.canvas,
            &self.source.plan,
            &frame,
            &plan,
            self.cache.plan(),
        );
        self.stats.plan_entities_drawn += drawn.entities_drawn;
        // A surface whose style the painter could not draw in whole - contours too fine to trace
        // are refused - is a problem of this viewport as much as a missing alignment is: the plan
        // is drawn, and a plot or a sheet that was silently without its contours is paper wasted.
        // It joins the sheet's problems, which the editor shows in the viewport's panel and every
        // plot reports.
        for refused in &drawn.surface_problems {
            self.problem(viewport, &refused.text());
        }

        if viewport.kind == ViewportKind::KeyPlan {
            // The drawing faded, so the sheets over it read first.
            self.canvas
                .fill_rect(device, &Brush::solid(Color::rgba(255, 255, 255, 165)));
        }
        let grid = self.plan_grid_for(viewport, &at, text);
        if let Some(grid) = &grid {
            self.paint_grid_strokes(grid, paper);
        }
        if viewport.kind == ViewportKind::KeyPlan {
            self.paint_key_plan(viewport, &at, text, paper);
        }
        self.paint_marks(viewport, &at, text, paper);
        if let Some(grid) = &grid {
            self.paint_grid_labels(grid, text, paper);
        }
        true
    }

    /// The coordinate grid (cad's plan_grid): worked out once per viewport, its strokes under the
    /// marks and its labels over them.
    fn plan_grid_for(
        &mut self,
        viewport: &Viewport,
        at: &ResolvedViewport,
        text: &TextSetter<'_>,
    ) -> Option<PlanGrid> {
        if viewport.grid_style == GridStyle::None {
            return None;
        }
        let style = grid_label_style();
        let grid = {
            let metrics = self.canvas.metrics();
            let label_width = |label: &str| text.width_mm(metrics, label, &style);
            let mut options = PlanGridOptions {
                label_cap_mm: style.cap_mm,
                label_width_mm: Some(&label_width),
                ..PlanGridOptions::default()
            };
            // The furniture paint_viewport draws over the plan, which the labels keep off.
            if viewport.north_arrow {
                options.keep_out.push(north_arrow_box(&viewport.rect));
            }
            if viewport.scale_bar {
                options
                    .keep_out
                    .push(scale_bar_box(&viewport.rect, at.scale));
            }
            if let Some(title) = title_place(
                metrics,
                text,
                &viewport.rect,
                &view_title(viewport, at.scale),
            ) {
                options.keep_out.push(title.knocked);
            }
            plan_grid(
                viewport,
                &PlanPlacement {
                    scale: at.scale,
                    centre: at.centre,
                },
                &options,
            )
        };
        match grid {
            Ok(grid) => Some(grid),
            Err(error) => {
                self.problem(viewport, &error.message);
                None
            }
        }
    }

    fn paint_grid_strokes(&mut self, grid: &PlanGrid, paper: &Paper<'_>) {
        // Lines light and fine, under everything; crosses and ticks in ink, as they are few and
        // must be found.
        self.canvas.set_pen(if grid.style == GridStyle::Lines {
            paper_pen(paper, GRID_INK, 0.13)
        } else {
            paper_pen(paper, INK, 0.18)
        });
        self.canvas.set_brush(Brush::none());
        for stroke in &grid.strokes {
            self.canvas
                .draw_line(paper.at(stroke.from), paper.at(stroke.to));
        }
    }

    fn paint_grid_labels(&mut self, grid: &PlanGrid, text: &TextSetter<'_>, paper: &Paper<'_>) {
        let mut style = grid_label_style();
        for label in &grid.labels {
            knock_out(&mut *self.canvas, paper, &label.box_value);
            style.angle_degrees = label.angle_degrees;
            style.horizontal = label.horizontal;
            style.vertical = label.vertical;
            text.draw(&mut *self.canvas, label.anchor, &label.text, &style, 1.0);
        }
    }

    /// A key plan's outlines, live (cad's key_plan).
    fn paint_key_plan(
        &mut self,
        viewport: &Viewport,
        at: &ResolvedViewport,
        text: &TextSetter<'_>,
        paper: &Paper<'_>,
    ) {
        let placed = PlanPlacement {
            scale: at.scale,
            centre: at.centre,
        };
        // Decides an automatic plan's placement for the key plans, as it is drawn.
        let source = self.source;
        let placer = |plan: &Viewport| {
            let at = resolve_plan_viewport(plan, source);
            PlanPlacement {
                scale: at.scale,
                centre: at.centre,
            }
        };
        let outlines = key_plan_outlines(self.set, self.index, Some(&placer));
        let on_paper = |outline: &KeyPlanOutline| -> Vec<Point2> {
            outline
                .corners
                .iter()
                .map(|corner| plan_world_to_paper(viewport, &placed, *corner))
                .collect()
        };
        // The other sheets first, then this sheet's over them in a fill of its own - "you are
        // here" - so no neighbour's outline hides it.
        for current in [false, true] {
            for outline in &outlines {
                if outline.current != current {
                    continue;
                }
                let ring: Vec<PointF> = on_paper(outline)
                    .into_iter()
                    .map(|point| paper.at(point))
                    .collect();
                self.canvas.set_pen(paper_pen(
                    paper,
                    if current { HERE_INK } else { OUTLINE_INK },
                    if current { 0.5 } else { 0.35 },
                ));
                self.canvas.set_brush(paper_shade(
                    paper,
                    if current { HERE_FILL } else { OUTLINE_FILL },
                ));
                self.canvas.draw_polygon(&ring, FillRule::OddEven);
            }
        }
        self.canvas.set_brush(Brush::none());
        // The numbers after every fill, so none is covered; each sized to its outline, so a small
        // sheet's number stays inside it.
        for outline in &outlines {
            if !outline.labelled || outline.label.is_empty() {
                continue;
            }
            let mut extent = Box2::default();
            for point in on_paper(outline) {
                extent.expand(point);
            }
            let style = TextStyle {
                cap_mm: (0.4 * katana_math::cpp::min(extent.width(), extent.height()))
                    .clamp(1.2, 3.0),
                bold: true,
                colour: if outline.current {
                    HERE_INK
                } else {
                    OUTLINE_INK
                },
                horizontal: HorizontalJustify::Centre,
                vertical: VerticalJustify::Middle,
                ..TextStyle::default()
            };
            text.draw(
                &mut *self.canvas,
                extent.center(),
                &outline.label,
                &style,
                1.0,
            );
        }
    }

    fn paint_marks(
        &mut self,
        viewport: &Viewport,
        at: &ResolvedViewport,
        text: &TextSetter<'_>,
        paper: &Paper<'_>,
    ) {
        if viewport.marks.is_empty() {
            return;
        }
        let mm_per_metre = 1000.0 / at.scale;
        let middle = viewport.rect.center();
        let on_paper =
            |world: Point2| middle + rotated(world - at.centre, -viewport.rotation) * mm_per_metre;
        self.canvas.save();
        self.canvas
            .set_clip_rect(paper.at_box(&viewport.rect), true);
        for mark in &viewport.marks {
            // A key plan draws its sheets' outlines live (paint_key_plan); the ones stored when it
            // was made go stale as the sheets change.
            if mark.points.len() < 2
                || (viewport.kind == ViewportKind::KeyPlan
                    && mark.kind == WorldMarkKind::SheetOutline)
            {
                continue;
            }
            let points: Vec<Point2> = mark.points.iter().map(|world| on_paper(*world)).collect();
            let line: Vec<PointF> = points.iter().map(|point| paper.at(*point)).collect();
            let label = mark_label(self.set, mark);
            if mark.kind == WorldMarkKind::SheetOutline {
                let ink = Color::rgb(200, 0, 30);
                self.canvas.set_pen(paper_pen(paper, ink, 0.35));
                self.canvas
                    .set_brush(paper_fill(paper, Color::rgba(200, 0, 30, 18)));
                self.canvas.draw_polygon(&line, FillRule::OddEven);
                self.canvas.set_brush(Brush::none());
                let mut centroid = Point2::default();
                for p in &points {
                    centroid = centroid + *p;
                }
                centroid = centroid * (1.0 / points.len() as f64);
                let style = TextStyle {
                    cap_mm: 3.0,
                    bold: true,
                    colour: ink,
                    horizontal: HorizontalJustify::Centre,
                    vertical: VerticalJustify::Middle,
                    ..TextStyle::default()
                };
                text.draw(&mut *self.canvas, centroid, &label, &style, 1.0);
                continue;
            }
            self.canvas
                .set_pen(dashed_pen(paper, INK, 0.5, &[8.0, 1.5, 1.0, 1.5]));
            self.canvas.draw_polyline(&line);
            // The label along the line, turned to read from the bottom or right.
            let a = points[0];
            let b = points[1];
            let mut angle = (b.y - a.y).atan2(b.x - a.x) * RAD_TO_DEG;
            if angle > 90.0 {
                angle -= 180.0;
            } else if angle <= -90.0 {
                angle += 180.0;
            }
            let style = TextStyle {
                cap_mm: 2.0,
                bold: true,
                angle_degrees: angle,
                horizontal: HorizontalJustify::Centre,
                ..TextStyle::default()
            };
            let mid = (a + b) * 0.5;
            let lift = rotated(Point2::new(0.0, 1.0), angle * DEG_TO_RAD);
            text.draw(&mut *self.canvas, mid + lift, &label, &style, 1.0);
        }
        self.canvas.restore();
    }

    fn paint_sections(
        &mut self,
        viewport: &Viewport,
        text: &TextSetter<'_>,
        paper: &Paper<'_>,
    ) -> bool {
        let result = {
            let sections = RefCell::new(&mut self.cache.sections);
            let mut cuts = section_cuts(&sections, self.source, &viewport.source.alignment);
            let mut canvas = SheetSectionCanvas {
                canvas: &mut *self.canvas,
                paper: *paper,
                text,
            };
            paint_section_viewport(&mut canvas, viewport, &mut cuts, self.source.plan.model)
        };
        for failure in &result.problems {
            self.problem(viewport, failure);
        }
        self.stats.section_notes_dropped += result.notes_dropped;
        result.drawn
    }

    /// A section viewport's scale and exaggeration: its own, or with auto_scale those
    /// fit_section_viewport chooses for what it shows.
    fn section_fit(
        &mut self,
        viewport: &Viewport,
        text: &TextSetter<'_>,
        paper: &Paper<'_>,
    ) -> SectionFit {
        let sections = RefCell::new(&mut self.cache.sections);
        let mut cuts = section_cuts(&sections, self.source, &viewport.source.alignment);
        let mut canvas = SheetSectionCanvas {
            canvas: &mut *self.canvas,
            paper: *paper,
            text,
        };
        fit_section_viewport(&mut canvas, viewport, &mut cuts, self.source.plan.model)
    }

    fn paint_snapshot(&mut self, viewport: &Viewport, paper: &Paper<'_>) -> bool {
        let Some(document) = self.source.document else {
            self.problem(viewport, "no 3D scene to show");
            return false;
        };
        let dpi = katana_math::cpp::min(self.options.raster_dpi_cap, paper.ppmm() * 25.4);
        let mut width = ((viewport.rect.width() * dpi / 25.4) as i32).max(16);
        let mut height = ((viewport.rect.height() * dpi / 25.4) as i32).max(16);
        let pixels = f64::from(width) * f64::from(height);
        if pixels > self.options.raster_pixel_cap as f64 {
            let shrink = (self.options.raster_pixel_cap as f64 / pixels).sqrt();
            width = ((f64::from(width) * shrink) as i32).max(16);
            height = ((f64::from(height) * shrink) as i32).max(16);
        }
        let key = format!(
            "{}|{}x{}|{:.4}|{:.4}",
            viewport.id, width, height, viewport.tilt_degrees, viewport.rotation
        );
        let stale = self
            .cache
            .snapshots
            .get(&key)
            .is_none_or(|entry| entry.image.is_null() || entry.revision != self.source.revision);
        if stale {
            let scene = SceneOptions {
                draw_grid: false,
                ..SceneOptions::default()
            };
            let mut builder = SceneBuilder::new();
            let mut layers = SceneLayers::default();
            let meshes = self.source.plan.meshes.unwrap_or(&[]);
            builder.build_terrain(&self.source.surfaces, meshes, &scene, &mut layers);
            builder.build_entities(document, &self.source.surfaces, &scene, &mut layers);
            if layers.bounds.empty() {
                self.problem(viewport, "the 3D scene is empty");
                return false;
            }
            let mut camera = Camera::default();
            camera.set_viewport_size(width, height);
            camera.set_orientation(
                -PI / 4.0 + viewport.rotation,
                viewport.tilt_degrees.clamp(1.0, 89.0) * DEG_TO_RAD,
            );
            // Whether there was anything to frame is known: the bounds are not empty.
            let _ = camera.frame(layers.bounds);
            let mut framebuffer = Framebuffer::default();
            if let Err(error) = framebuffer.resize(width, height) {
                self.problem(viewport, &error.describe());
                return false;
            }
            let mut rasterizer = Rasterizer::default();
            let render = RenderOptions {
                background: rgba(255, 255, 255),
                ..RenderOptions::default()
            };
            if let Err(error) = render_layers(
                &mut layers,
                &mut camera,
                &mut rasterizer,
                &mut framebuffer,
                render,
            ) {
                self.problem(viewport, &error.describe());
                return false;
            }
            // The framebuffer's colours are straight ARGB words (QImage::Format_ARGB32).
            let mut bytes = Vec::with_capacity(framebuffer.color().len() * 4);
            for &pixel in framebuffer.color() {
                bytes.extend_from_slice(&[
                    red_of(pixel),
                    green_of(pixel),
                    blue_of(pixel),
                    alpha_of(pixel),
                ]);
            }
            let image = match Image::from_straight_rgba8(width as u32, height as u32, bytes) {
                Ok(image) => image,
                Err(error) => {
                    self.problem(viewport, &error.describe());
                    return false;
                }
            };
            self.cache.snapshots.insert(
                key.clone(),
                CachedImage {
                    revision: self.source.revision,
                    image,
                },
            );
        }
        let Some(entry) = self.cache.snapshots.get(&key) else {
            return false;
        };
        self.canvas.save();
        self.canvas
            .set_render_hint(RenderHint::SmoothPixmapTransform, true);
        draw_whole_image(
            &mut *self.canvas,
            paper.at_box(&viewport.rect),
            &plot_image(&entry.image, &self.options.plot),
        );
        self.canvas.restore();
        true
    }

    fn paint_legend(&mut self, viewport: &Viewport, text: &TextSetter<'_>, paper: &Paper<'_>) {
        let model = self.source.plan.model;
        let head = TextStyle {
            cap_mm: 2.5,
            bold: true,
            ..TextStyle::default()
        };
        let r = &viewport.rect;
        text.draw(
            &mut *self.canvas,
            Point2::new(r.min.x + 2.5, r.max.y - 5.0),
            "LEGEND",
            &head,
            1.0,
        );
        self.canvas.set_pen(paper_pen(paper, INK, 0.35));
        let underline = text.width_mm(self.canvas.metrics(), "LEGEND", &head);
        self.canvas.draw_line(
            paper.at(Point2::new(r.min.x + 2.5, r.max.y - 6.0)),
            paper.at(Point2::new(r.min.x + 2.5 + underline, r.max.y - 6.0)),
        );
        let Some(model) = model else {
            return;
        };
        // What the plans show (cad's legend), each automatic plan at the window this painter draws
        // it at, so the legend lists what the plan beside it drew.
        let legend = match gather_legend(self.set, self.index, viewport, self.source) {
            Ok(legend) => legend,
            Err(error) => {
                self.problem(viewport, &error.describe());
                return;
            }
        };
        if legend.entries.is_empty() && self.options.slot_hints {
            paint_message(
                &mut *self.canvas,
                text,
                r,
                "Nothing to list: the plans show nothing",
            );
        }
        // Labels in capitals, as the frame letters; columns as wide as the widest needs, flowed to
        // fit (cad's layout_legend).
        let label = TextStyle {
            cap_mm: 1.8,
            x_factor: 0.9,
            vertical: VerticalJustify::Middle,
            ..TextStyle::default()
        };
        let mut names = Vec::new();
        let mut widths = Vec::new();
        for entry in &legend.entries {
            let name = entry.label.to_uppercase();
            widths.push(text.width_mm(self.canvas.metrics(), &name, &label));
            names.push(name);
        }
        let layout = layout_legend(r, &widths, &LegendMetrics::default());
        let samples = LegendSampleContext {
            model: Some(model),
            library: self.source.plan.library,
            colours: self.source.plan.colours,
            plot: Some(&self.options.plot),
            pixels_per_millimetre: paper.ppmm(),
            scale: if legend.scale > 0.0 {
                legend.scale
            } else {
                viewport.scale
            },
        };
        for cell in &layout.cells {
            paint_legend_sample(
                &mut *self.canvas,
                &legend.entries[cell.entry],
                paper.at_box(&cell.sample),
                &samples,
            );
            let mut squeeze = 1.0;
            let w = widths[cell.entry];
            if w > cell.label_room && cell.label_room > 0.0 {
                squeeze = katana_math::cpp::max(cell.label_room / w, 0.5);
            }
            text.draw(
                &mut *self.canvas,
                cell.label,
                &names[cell.entry],
                &label,
                squeeze,
            );
        }
        if let Some(more_at) = layout.more_at {
            let more = TextStyle {
                colour: FAINT,
                ..label.clone()
            };
            text.draw(
                &mut *self.canvas,
                more_at,
                &format!("+{} more", layout.more),
                &more,
                1.0,
            );
        }
    }

    fn paint_notes(&mut self, viewport: &Viewport, text: &TextSetter<'_>, paper: &Paper<'_>) {
        let head = TextStyle {
            cap_mm: 2.5,
            bold: true,
            ..TextStyle::default()
        };
        let r = &viewport.rect;
        text.draw(
            &mut *self.canvas,
            Point2::new(r.min.x + 2.5, r.max.y - 5.0),
            "NOTES",
            &head,
            1.0,
        );
        self.canvas.set_pen(paper_pen(paper, INK, 0.35));
        let underline = text.width_mm(self.canvas.metrics(), "NOTES", &head);
        self.canvas.draw_line(
            paper.at(Point2::new(r.min.x + 2.5, r.max.y - 6.0)),
            paper.at(Point2::new(r.min.x + 2.5 + underline, r.max.y - 6.0)),
        );
        let body = TextStyle {
            cap_mm: 2.0,
            x_factor: 0.9,
            vertical: VerticalJustify::Top,
            ..TextStyle::default()
        };
        let lines = text.wrap(
            self.canvas.metrics(),
            &viewport.text,
            &body,
            katana_math::cpp::max(r.width() - 5.0, 5.0),
        );
        let spacing = 1.6 * body.cap_mm;
        let fit = ((r.height() - 10.0) / spacing).floor() as i64;
        let shown = &lines[..lines.len().min(fit.max(0) as usize)];
        text.draw(
            &mut *self.canvas,
            Point2::new(r.min.x + 2.5, r.max.y - 9.0),
            &shown.join("\n"),
            &TextStyle {
                line_spacing_mm: spacing,
                ..body.clone()
            },
            1.0,
        );
    }

    fn paint_image(&mut self, viewport: &Viewport, paper: &Paper<'_>) -> bool {
        if viewport.text.is_empty() {
            self.problem(viewport, "no image chosen");
            return false;
        }
        let path = self.source.assets.join(&viewport.text);
        let key = path.to_string_lossy().into_owned();
        if !self.cache.images.contains_key(&key) {
            // A file that cannot be read is a null image, as QImage(path) is.
            let image = read_image(&path).unwrap_or_default();
            self.cache.images.insert(key.clone(), image);
        }
        let null = self.cache.images.get(&key).is_none_or(Image::is_null);
        if null {
            self.problem(
                viewport,
                &format!("cannot read the image {}", viewport.text),
            );
            return false;
        }
        let Some(image) = self.cache.images.get(&key) else {
            return false;
        };
        let at = fit_image(
            &viewport.rect,
            f64::from(image.width()),
            f64::from(image.height()),
            1.0,
        );
        self.canvas.save();
        self.canvas
            .set_render_hint(RenderHint::SmoothPixmapTransform, true);
        draw_whole_image(
            &mut *self.canvas,
            paper.at_box(&at),
            &plot_image(image, &self.options.plot),
        );
        self.canvas.restore();
        true
    }

    fn paint_table_viewport(
        &mut self,
        viewport: &Viewport,
        text: &TextSetter<'_>,
        paper: &Paper<'_>,
    ) {
        // The register reports each sheet's scale as its title block does, an automatic one
        // decided first.
        let resolved = (viewport.kind == ViewportKind::SheetIndex)
            .then(|| resolved_sheet_set(self.set, self.source, Some(&mut *self.cache)));
        let table = {
            // Laid out with this painter's own font, so what the layout measured is what is drawn.
            let metrics = self.canvas.metrics();
            let measure = |line: &str, cap_mm: f64, bold: bool| {
                let style = TextStyle {
                    cap_mm,
                    bold,
                    ..TextStyle::default()
                };
                text.width_mm(metrics, line, &style)
            };
            layout_viewport_table(
                resolved.as_ref().unwrap_or(self.set),
                self.index,
                viewport,
                Some(&measure),
            )
        };
        paint_table(&mut *self.canvas, paper, text, &table);
        if table.rows_hidden > 0 {
            self.problem(
                viewport,
                &format!(
                    "{} of {} rows do not fit; make the view larger",
                    table.rows_hidden,
                    table.rows_shown + table.rows_hidden
                ),
            );
        }
        if self.options.slot_hints
            && viewport.kind == ViewportKind::Revisions
            && table.rows_shown == 0
            && table.rows_hidden == 0
        {
            // On screen only: where the rows of an empty table come from.
            let hint = "No revisions yet: add them under Title Block";
            let style = TextStyle {
                cap_mm: 2.0,
                horizontal: HorizontalJustify::Centre,
                vertical: VerticalJustify::Middle,
                colour: FAINT,
                ..TextStyle::default()
            };
            let half = text.width_mm(self.canvas.metrics(), hint, &style) / 2.0 + 1.0;
            let at = viewport.rect.center();
            knock_out(
                &mut *self.canvas,
                paper,
                &Box2::new(
                    Point2::new(at.x - half, at.y - 2.0),
                    Point2::new(at.x + half, at.y + 2.0),
                ),
            );
            text.draw(&mut *self.canvas, at, hint, &style, 1.0);
        }
    }

    fn paint_viewport(&mut self, viewport: &Viewport, text: &TextSetter<'_>, paper: &Paper<'_>) {
        let device = paper.at_box(&viewport.rect);
        self.canvas.save();
        self.canvas.set_clip_rect(device, true);
        self.canvas.fill_rect(device, &Brush::solid(Color::WHITE));

        let mut scale = viewport.scale;
        let drawn = match viewport.kind {
            ViewportKind::Plan | ViewportKind::KeyPlan => {
                self.paint_plan_viewport(viewport, text, paper, &mut scale)
            }
            ViewportKind::LongSection | ViewportKind::CrossSections => {
                self.paint_sections(viewport, text, paper)
            }
            ViewportKind::Model3D => self.paint_snapshot(viewport, paper),
            ViewportKind::Legend => {
                self.paint_legend(viewport, text, paper);
                true
            }
            ViewportKind::Notes => {
                self.paint_notes(viewport, text, paper);
                true
            }
            ViewportKind::Image => self.paint_image(viewport, paper),
            ViewportKind::SheetIndex | ViewportKind::Revisions => {
                self.paint_table_viewport(viewport, text, paper);
                true
            }
        };
        if !drawn
            && self.options.slot_hints
            && let Some(last) = self.stats.problems.last()
        {
            // The words after the viewport's id. (The C++ takes the text from two past wherever
            // ": " is found, and from the second character when it is not.)
            let message = match last.find(": ") {
                Some(at) => last[at + 2..].to_owned(),
                None => last.chars().skip(1).collect(),
            };
            paint_message(&mut *self.canvas, text, &viewport.rect, &message);
        }

        let plan_like =
            viewport.kind == ViewportKind::Plan || viewport.kind == ViewportKind::KeyPlan;
        if drawn && plan_like && viewport.north_arrow {
            paint_north_arrow(
                &mut *self.canvas,
                paper,
                text,
                &viewport.rect,
                viewport.rotation,
            );
        }
        if drawn && plan_like && viewport.scale_bar {
            paint_scale_bar(&mut *self.canvas, paper, text, &viewport.rect, scale);
        }
        // A table's title is its heading, drawn by the table.
        if !matches!(
            viewport.kind,
            ViewportKind::Legend
                | ViewportKind::Notes
                | ViewportKind::Image
                | ViewportKind::SheetIndex
                | ViewportKind::Revisions
        ) {
            paint_title(
                &mut *self.canvas,
                paper,
                text,
                &viewport.rect,
                &view_title(viewport, scale),
            );
        }
        self.canvas.restore();

        self.canvas.set_pen(paper_pen(paper, INK, 0.25));
        self.canvas.set_brush(Brush::none());
        self.canvas.draw_rect(device);
        if drawn {
            self.stats.viewports_drawn += 1;
        }
    }

    fn run(mut self) -> SheetPaintStats {
        let Some(sheet) = self.set.sheets.get(self.index) else {
            return self.stats;
        };
        let dimensions = paper_dimensions(sheet.paper, sheet.landscape);
        let options = self.options;
        let paper = Paper::new(dimensions.height_mm, options);

        self.canvas.save();
        self.canvas.set_render_hint(RenderHint::Antialiasing, true);
        self.canvas
            .set_render_hint(RenderHint::TextAntialiasing, true);
        self.canvas.fill_rect(
            paper.at_box(&Box2::new(
                Point2::new(0.0, 0.0),
                Point2::new(dimensions.width_mm, dimensions.height_mm),
            )),
            &Brush::solid(Color::WHITE),
        );
        let text = TextSetter::new(paper);

        // An automatic scale is decided here - a plan's, and a section's scale and exaggeration -
        // and the title block reports the scale that was drawn.
        let mut drawn_set = self.set.clone();
        for at in 0..drawn_set.sheets[self.index].viewports.len() {
            let viewport = drawn_set.sheets[self.index].viewports[at].clone();
            let plan_like =
                viewport.kind == ViewportKind::Plan || viewport.kind == ViewportKind::KeyPlan;
            let section = viewport.kind == ViewportKind::LongSection
                || viewport.kind == ViewportKind::CrossSections;
            let drawn = &mut drawn_set.sheets[self.index].viewports[at];
            if plan_like && (viewport.auto_scale || viewport.auto_centre) {
                let resolved =
                    resolve_plan_viewport_on_sheet(&viewport, self.source, self.set, self.index);
                drawn.scale = resolved.scale;
                drawn.centre = resolved.centre;
                drawn.auto_scale = false;
                drawn.auto_centre = false;
            } else if section && viewport.auto_scale {
                let fit = self.section_fit(&viewport, &text, &paper);
                drawn.scale = fit.scale;
                drawn.vertical_exaggeration = fit.exaggeration;
                drawn.auto_scale = false;
            }
        }

        // The panels under the frame, back to front, as the app drew them.
        for viewport in &drawn_set.sheets[self.index].viewports {
            if !viewport.rect.empty() {
                self.paint_viewport(viewport, &text, &paper);
            }
        }

        if !sheet.frame.is_empty() {
            let key = format!("{}|{}|{}", sheet.frame, sheet.paper as i32, sheet.landscape);
            let found = Arc::clone(self.cache.frames.entry(key).or_insert_with(|| {
                Arc::new(frame_for(&sheet.frame, sheet.paper, sheet.landscape))
            }));
            match &*found {
                Ok(frame) => {
                    let fields = resolve_fields(&drawn_set, self.index, &self.source.fields);
                    paint_frame(
                        &mut *self.canvas,
                        &paper,
                        &text,
                        frame,
                        sheet,
                        &fields,
                        self.source,
                        &mut self.stats,
                    );
                }
                Err(error) => {
                    if sheet.landscape {
                        self.stats
                            .problems
                            .push(format!("frame: {}", error.describe()));
                    }
                }
            }
        }
        self.canvas.restore();
        self.stats
    }
}

// ---- public -------------------------------------------------------------------------------------

/// The one mapping from paper to device and back.
pub fn paper_to_device(paper: Point2, paper_height_mm: f64, options: &SheetPaintOptions) -> PointF {
    options.origin
        + PointF::new(
            paper.x * options.pixels_per_millimetre,
            (paper_height_mm - paper.y) * options.pixels_per_millimetre,
        )
}

/// A box of the paper on the device (the C++'s paperToDevice(Box2)).
pub fn paper_box_to_device(
    paper: &Box2,
    paper_height_mm: f64,
    options: &SheetPaintOptions,
) -> RectF {
    RectF::from_corners(
        paper_to_device(
            Point2::new(paper.min.x, paper.max.y),
            paper_height_mm,
            options,
        ),
        paper_to_device(
            Point2::new(paper.max.x, paper.min.y),
            paper_height_mm,
            options,
        ),
    )
}

pub fn device_to_paper(
    device: PointF,
    paper_height_mm: f64,
    options: &SheetPaintOptions,
) -> Point2 {
    let d = device - options.origin;
    Point2::new(
        d.x / options.pixels_per_millimetre,
        paper_height_mm - d.y / options.pixels_per_millimetre,
    )
}

/// The frame sheet `sheet` is drawn in, or nothing for a sheet without one (an empty frame id, or
/// a portrait sheet).
pub fn sheet_frame(sheet: &Sheet) -> Option<Frame> {
    if sheet.frame.is_empty() || !sheet.landscape {
        return None;
    }
    frame_for(&sheet.frame, sheet.paper, sheet.landscape).ok()
}

/// The scale and centre `viewport` is drawn at: its own, or for auto_scale the largest of
/// SHEET_SCALES at which what it shows fits, and for auto_centre the middle of it. Plans only; a
/// section's are resolve_section_viewport's.
pub fn resolve_plan_viewport(viewport: &Viewport, source: &SheetSource<'_>) -> ResolvedViewport {
    let mut at = ResolvedViewport {
        scale: viewport.scale,
        centre: viewport.centre,
    };
    let Some(model) = source.plan.model else {
        return at;
    };
    if (!viewport.auto_scale && !viewport.auto_centre) || viewport.rect.empty() {
        return at;
    }
    // What the viewport shows: its stretch of an alignment, or the drawing.
    let mut points: Vec<Point2> = Vec::new();
    let from = &viewport.source;
    if !from.alignment.is_empty()
        && let Some(alignment) = model.alignments.find(&from.alignment)
        && let Ok(solved) = solve_alignment(&alignment.horizontal)
    {
        let mut a = from.chainage_from;
        let mut b = from.chainage_to;
        if !(b > a) {
            a = solved.start_station();
            b = solved.end_station();
        }
        for i in 0..=64 {
            if let Some(p) = solved.point_at_station(a + (b - a) * f64::from(i) / 64.0) {
                points.push(p);
            }
        }
    }
    if points.is_empty() {
        let bounds = plan_drawn_bounds(&source.plan, &viewport.hidden_layers, &BTreeSet::new());
        if bounds.empty() {
            return at;
        }
        points = vec![
            bounds.min,
            bounds.max,
            Point2::new(bounds.min.x, bounds.max.y),
            Point2::new(bounds.max.x, bounds.min.y),
        ];
    }
    if viewport.auto_centre {
        let mut bounds = Box2::default();
        for p in &points {
            bounds.expand(rotated(*p, -viewport.rotation));
        }
        at.centre = rotated(bounds.center(), viewport.rotation);
    }
    if viewport.auto_scale {
        let mut half_w: f64 = 0.0;
        let mut half_h: f64 = 0.0;
        for p in &points {
            let d = rotated(*p - at.centre, -viewport.rotation);
            half_w = katana_math::cpp::max(half_w, d.x.abs());
            half_h = katana_math::cpp::max(half_h, d.y.abs());
        }
        // 4% to spare, so the drawing does not touch the viewport's edge.
        let needed = katana_math::cpp::max(
            2.0 * half_w * 1000.0 / viewport.rect.width(),
            2.0 * half_h * 1000.0 / viewport.rect.height(),
        ) * 1.04;
        if needed > 0.0
            && let Ok(scale) = sheet_scale_at_least(needed)
        {
            at.scale = scale;
        }
    }
    at
}

/// The same for a viewport on sheet `sheet_index` of `set`, which is what the painter draws: an
/// automatic key plan fits every sheet's plan outline (key_plan_outlines), the drawing only when
/// there is none (fit_key_plan). Any other viewport resolves as above.
pub fn resolve_plan_viewport_on_sheet(
    viewport: &Viewport,
    source: &SheetSource<'_>,
    set: &SheetSet,
    sheet_index: usize,
) -> ResolvedViewport {
    if viewport.kind != ViewportKind::KeyPlan
        || (!viewport.auto_scale && !viewport.auto_centre)
        || source.plan.model.is_none()
        || viewport.rect.empty()
    {
        return resolve_plan_viewport(viewport, source);
    }
    let placer = |plan: &Viewport| {
        let at = resolve_plan_viewport(plan, source);
        PlanPlacement {
            scale: at.scale,
            centre: at.centre,
        }
    };
    let outlines = key_plan_outlines(set, sheet_index, Some(&placer));
    // The drawing is only the fallback for a set with no plan to frame.
    let drawing = if outlines.is_empty() {
        plan_drawn_bounds(&source.plan, &viewport.hidden_layers, &BTreeSet::new())
    } else {
        Box2::default()
    };
    let fitted = fit_key_plan(viewport, &outlines, &drawing);
    ResolvedViewport {
        scale: fitted.scale,
        centre: fitted.centre,
    }
}

/// The horizontal scale and exaggeration section `viewport` is drawn at: its own, or for
/// auto_scale the largest standard scale at which its chainage range (the whole alignment without
/// one) or its cross sections' width fits its plot, and the largest exaggeration of the standard
/// ones at which its levels then fit (cad's section_fit). The sections are cut through `cache`, as
/// a paint cuts them; a section that cannot be cut keeps the viewport's own scale and
/// exaggeration.
pub fn resolve_section_viewport(
    viewport: &Viewport,
    source: &SheetSource<'_>,
    cache: &mut SheetPaintCache,
) -> SectionFit {
    // Measured as a paint measures, on a scratch device: the fit leaves room for the level labels,
    // and their width is the font's.
    let mut scratch = RasterCanvas::new(8, 8);
    let none = SheetSet::default();
    let options = SheetPaintOptions::default();
    let paper = Paper::new(0.0, &options);
    let text = TextSetter::new(paper);
    SheetPainter {
        canvas: &mut scratch,
        set: &none,
        index: 0,
        source,
        options: &options,
        cache,
        stats: SheetPaintStats::default(),
    }
    .section_fit(viewport, &text, &paper)
}

/// Paints sheet `index` of `set`. The canvas's state is the same afterwards.
pub fn paint_sheet(
    canvas: &mut dyn Canvas,
    set: &SheetSet,
    index: usize,
    source: &SheetSource<'_>,
    options: &SheetPaintOptions,
    cache: &mut SheetPaintCache,
) -> SheetPaintStats {
    SheetPainter {
        canvas,
        set,
        index,
        source,
        options,
        cache,
        stats: SheetPaintStats::default(),
    }
    .run()
}

/// A page of a PDF the size of `sheet`'s paper, drawn in pixels of `resolution` dots an inch: 72
/// points an inch, 25.4 millimetres an inch.
pub(crate) fn pdf_page_for(sheet: &Sheet, resolution: i32) -> PdfCanvas {
    let paper = paper_dimensions(sheet.paper, sheet.landscape);
    let points = |millimetres: f64| millimetres * 72.0 / 25.4;
    PdfCanvas::new(
        points(paper.width_mm),
        points(paper.height_mm),
        f64::from(resolution),
    )
}

/// The path as an error's context says it.
pub(crate) fn path_text(path: &FilePath) -> String {
    String::from_utf8_lossy(&generic_path_utf8(path)).into_owned()
}

/// Plots the sheets at `indices` of `set` (every sheet when empty), in that order, to one vector
/// PDF at `path`, each page the size of its sheet's paper, in the set's plot style (its colour
/// mode and line weight scale) at `dpi`. `problems` receives what paint_sheet reports, prefixed
/// with the sheet's name. FileExportFailure when the file cannot be written; InvalidArgument for
/// an index past the end or a set with nothing to plot.
///
/// `title` is the PDF's title in the C++ (the file's own name when empty) with Katana its creator;
/// the canvas's PDF document writes neither yet, so it is taken and not written.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ plotSheetsToPdf's arguments."
)]
pub fn plot_sheets_to_pdf(
    path: &FilePath,
    set: &SheetSet,
    indices: &[usize],
    source: &SheetSource<'_>,
    dpi: f64,
    cache: &mut SheetPaintCache,
    title: &str,
    problems: Option<&mut Vec<String>>,
) -> Status {
    write_sheets_pdf(
        path, set, indices, source, dpi, cache, title, problems, false,
    )
}

/// plot_sheets_to_pdf, written as PdfDocument::uncompressed writes a file, for a test that reads
/// it (docs/rust.md, "A test reads a PDF written readable").
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ plotSheetsToPdf's arguments."
)]
pub fn plot_sheets_to_readable_pdf(
    path: &FilePath,
    set: &SheetSet,
    indices: &[usize],
    source: &SheetSource<'_>,
    dpi: f64,
    cache: &mut SheetPaintCache,
    title: &str,
    problems: Option<&mut Vec<String>>,
) -> Status {
    write_sheets_pdf(
        path, set, indices, source, dpi, cache, title, problems, true,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "The C++ plotSheetsToPdf's arguments, and how the file is written."
)]
fn write_sheets_pdf(
    path: &FilePath,
    set: &SheetSet,
    indices: &[usize],
    source: &SheetSource<'_>,
    dpi: f64,
    cache: &mut SheetPaintCache,
    _title: &str,
    mut problems: Option<&mut Vec<String>>,
    readable: bool,
) -> Status {
    let mut pages: Vec<usize> = indices.to_vec();
    if pages.is_empty() {
        pages.extend(0..set.sheets.len());
    }
    if pages.is_empty() {
        return Err(katana_core::Error::new(
            ErrorCode::InvalidArgument,
            "there are no sheets to plot",
        ));
    }
    for &page in &pages {
        if page >= set.sheets.len() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "no sheet at that index",
                format!("{page}"),
            ));
        }
    }
    if !(dpi > 0.0) || !dpi.is_finite() {
        return Err(katana_core::Error::new(
            ErrorCode::InvalidArgument,
            "the resolution must be positive",
        ));
    }

    let resolution = pdf_resolution_for(dpi);
    // The file opened before anything is painted, as QPdfWriter's device is.
    if std::fs::File::create(path).is_err() {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "could not open the PDF for writing",
            path_text(path),
        ));
    }
    let mut document = if readable {
        PdfDocument::uncompressed()
    } else {
        PdfDocument::new()
    };
    let mut options = SheetPaintOptions {
        medium: PlanMedium::Paper,
        pixels_per_millimetre: millimetres_to_pixels(1.0, dpi),
        ..SheetPaintOptions::default()
    };
    // In the set's plot style, as every plot of the set is (cad's page_setup).
    options.plot.colour_mode = set.page_setup.colour_mode;
    options.plot.line_weight_scale = set.page_setup.line_weight_scale;
    options.plot.dpi = dpi;
    for &page in &pages {
        let mut canvas = pdf_page_for(&set.sheets[page], resolution.resolution);
        canvas.save();
        if resolution.scale != 1.0 {
            canvas.scale(resolution.scale, resolution.scale);
        }
        let stats = paint_sheet(&mut canvas, set, page, source, &options, cache);
        canvas.restore();
        if document.add_page(canvas).is_err() {
            return Err(make_error(
                ErrorCode::FileExportFailure,
                "could not start a page",
                path_text(path),
            ));
        }
        if let Some(problems) = problems.as_mut() {
            for line in &stats.problems {
                problems.push(format!("{} - {line}", set.sheets[page].name));
            }
        }
    }
    if document.write(path).is_err() {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "could not finish the PDF",
            path_text(path),
        ));
    }
    Ok(())
}
