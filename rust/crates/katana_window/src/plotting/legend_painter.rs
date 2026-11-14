//! The legend on paper (docs/plotting.md, "The smart legend"): what a Legend viewport lists as the
//! painter sees it, and each entry's sample drawn as the plan prints it. The port of
//! plotting/legend_painter.
//!
//! The legend panel (the sheet painter's paint_legend) gathers the entries here, lays them out
//! (cad's layout_legend) and sets their labels; the samples come from the same resolutions the
//! plan painter uses, so a legend cannot show a reader a mark the plan does not print:
//!
//! ```text
//! Line    a library linestyle's own strokes (cad's resolve_line_pattern and the shared style
//!         painter), else a model linetype's dashes, else a plain line - in the entry's colour
//!         through the paper colour rule and its weight in paper millimetres; a style's symbol at
//!         each end, as the plan puts one at every vertex.
//! Symbol  the symbol at its plotted size: a size on the ground at the legend's scale, the
//!         definition's own, or the plain mark's for a built-in shape - shrunk only when it would
//!         not fit its cell.
//! Point   the plain point mark, 2 mm across as on the plan.
//! Area    a swatch filled or hatched with the entry's pattern at the legend's scale, outlined as
//!         a line of the entry is.
//! Text    "Abc" in the entry's colour.
//! ```
//!
//! Reads no control; the canvas's state is the same afterwards.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(a > b) is kept: it rejects NaN, which a > b alone would let through."
)]

use std::sync::OnceLock;

use katana_cad::dashing::{DashOptions, qt_dash_pattern};
use katana_cad::hatching::{HatchDrawing, HatchOptions, hatch_drawing, hatch_segments};
use katana_cad::plot::{PlotSettings, paper_colour, paper_fill_colour};
use katana_cad::plotting::legend::{
    Legend, LegendEntry, LegendKind, LegendOptions, compute_legend,
};
use katana_cad::plotting::sheet_set::{PlanWindow, SheetSet, Viewport};
use katana_cad::style_drawing::{StyleDrawing, style_drawing_of_style};
use katana_cad::style_resolver::{LinetypeKind, point_symbol_drawing, resolve_line_pattern};
use katana_cad::view_transform::ViewTransform;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{ColourTable, Model, StyleLibrary};
use katana_geometry::{Point2, Polyline2};

use crate::canvas::{
    Brush, Canvas, Color, FillRule, Font, FontSize, Path, Pen, PenCap, PenStyle, PointF, RectF,
};
use crate::customisation::style_painter::{StylePaintTarget, paint_style_drawing, painted_extent};
use crate::plan_painter::{HATCH_LINE_PAPER_MILLIMETRES, POINT_MARKER_PAPER_MILLIMETRES};
use crate::sheet_painter::{SheetSource, resolve_plan_viewport};

/// What the Legend viewport `legend` on sheet `sheet_index` of `set` lists: cad's compute_legend
/// at the viewport's scope, each automatic plan at the window the painter draws it at
/// (resolve_plan_viewport), with the source's spatial index and, through its document, the survey
/// code library. What the painter prints and what the editor reports of it.
pub fn gather_legend(
    set: &SheetSet,
    sheet_index: usize,
    legend: &Viewport,
    source: &SheetSource<'_>,
) -> Result<Legend> {
    let Some(model) = source.plan.model else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no drawing to list",
        ));
    };
    let window = |plan: &Viewport| {
        let at = resolve_plan_viewport(plan, source);
        PlanWindow {
            scale: at.scale,
            centre: at.centre,
        }
    };
    let options = LegendOptions {
        window: Some(&window),
        index: source.plan.index,
        codes: source.document.map(|document| document.survey_map()),
        ..LegendOptions::default()
    };
    compute_legend(model, set, sheet_index, legend.legend_scope, &options)
}

#[derive(Clone, Copy)]
pub struct LegendSampleContext<'a> {
    /// The drawing's linetypes and hatch patterns; None draws plain lines.
    pub model: Option<&'a Model>,
    /// Library linestyles and symbols; None draws the built-in shapes.
    pub library: Option<&'a StyleLibrary>,
    /// The session's own colour names, for a pen inside a library definition
    /// (StylePaintTarget::colours); None resolves the standard names alone.
    pub colours: Option<&'a ColourTable>,
    /// The paper colour rule; None leaves colours as they are.
    pub plot: Option<&'a PlotSettings>,
    /// Device pixels per paper millimetre: what a line weight is multiplied by.
    pub pixels_per_millimetre: f64,
    /// 1 : scale, what a size on the ground is drawn at: a symbol's size in metres, a world
    /// linestyle's period, a hatch's spacing. Not positive: 1 : 500.
    pub scale: f64,
}

impl Default for LegendSampleContext<'_> {
    fn default() -> Self {
        Self {
            model: None,
            library: None,
            colours: None,
            plot: None,
            pixels_per_millimetre: 300.0 / 25.4,
            scale: 500.0,
        }
    }
}

fn no_model() -> &'static Model {
    static EMPTY: OnceLock<Model> = OnceLock::new();
    EMPTY.get_or_init(Model::new)
}

fn no_library() -> &'static StyleLibrary {
    static NONE: OnceLock<StyleLibrary> = OnceLock::new();
    NONE.get_or_init(StyleLibrary::default)
}

/// What the sample is drawn through: the cell's own little map, whose origin is the cell's centre
/// and whose units are metres on the ground at the legend's scale, so a pattern or a symbol sized
/// on the ground comes out the size it prints.
struct Sample<'a> {
    canvas: &'a mut dyn Canvas,
    entry: &'a LegendEntry,
    cell: RectF,
    context: &'a LegendSampleContext<'a>,
    model: &'a Model,
    library: &'a StyleLibrary,
    metres_per_millimetre: f64,
    view: ViewTransform,
    ink: Color,
    /// A solid area's colour on paper.
    fill_ink: Color,
    /// The plot style's line weight scale.
    line_scale: f64,
    pen_width: f64,
    pen: Pen,
    target: StylePaintTarget<'a>,
}

impl<'a> Sample<'a> {
    fn new(
        canvas: &'a mut dyn Canvas,
        entry: &'a LegendEntry,
        cell: RectF,
        context: &'a LegendSampleContext<'a>,
    ) -> Self {
        let scale = if context.scale > 0.0 && context.scale.is_finite() {
            context.scale
        } else {
            500.0
        };
        let metres_per_millimetre = scale / 1000.0;
        let mut view = ViewTransform {
            center: Point2::new(0.0, 0.0),
            scale: context.pixels_per_millimetre / metres_per_millimetre,
            ..ViewTransform::default()
        };
        view.resize(cell.width, cell.height);
        let colour = match context.plot {
            Some(plot) => paper_colour(entry.colour, plot),
            None => entry.colour,
        };
        let ink = Color::rgba(colour.r, colour.g, colour.b, colour.a);
        // A solid area prints as the plan prints a fill (cad's paper_fill_colour): in monochrome
        // black or white by its lightness, not the pen's black.
        let area = match context.plot {
            Some(plot) => paper_fill_colour(entry.colour, plot),
            None => entry.colour,
        };
        let fill_ink = Color::rgba(area.r, area.g, area.b, area.a);
        // The plan painter's pen: the weight in paper millimetres, times the plot style's line
        // weight scale, as the plan beside it is drawn.
        let line_scale = context.plot.map_or(1.0, |plot| plot.line_weight_scale);
        let pen_width = katana_math::cpp::max(entry.line_weight, 0.0)
            * line_scale
            * context.pixels_per_millimetre;
        let pen = Pen {
            color: ink,
            width: pen_width,
            ..Pen::default()
        };
        let target = StylePaintTarget {
            view,
            entity_pen: Pen {
                cap: PenCap::Flat,
                ..pen.clone()
            },
            paper: context.plot,
            entity_pen_only: false,
            colours: context.colours,
        };
        Self {
            canvas,
            entry,
            cell,
            context,
            model: match context.model {
                Some(model) => model,
                None => no_model(),
            },
            library: match context.library {
                Some(library) => library,
                None => no_library(),
            },
            metres_per_millimetre,
            view,
            ink,
            fill_ink,
            line_scale,
            pen_width,
            pen,
            target,
        }
    }

    fn paint(&mut self) -> bool {
        match self.entry.kind {
            LegendKind::Line => self.line(),
            LegendKind::Symbol => self.symbol(),
            LegendKind::Point => self.point(),
            LegendKind::Area => self.area(),
            LegendKind::Text => self.text(),
        }
    }

    /// Half the cell, in metres on the ground.
    fn half_width(&self) -> f64 {
        0.5 * self.cell.width / self.view.scale
    }

    fn half_height(&self) -> f64 {
        0.5 * self.cell.height / self.view.scale
    }

    fn at(&self, world: Point2) -> PointF {
        let p = self.view.world_to_screen(world);
        PointF::new(p.x, p.y)
    }

    /// A line of the entry along `path`: what the plan draws it with.
    fn stroke(&mut self, path: &Polyline2) {
        let pattern = resolve_line_pattern(
            self.model,
            self.library,
            &self.entry.linetype,
            &self.entry.symbol,
        );
        if pattern.kind == LinetypeKind::LibraryDefinition
            && let Some(definition) = pattern.definition
        {
            let drawing = style_drawing_of_style(definition, path, self.metres_per_millimetre);
            if !drawing.empty() {
                paint_style_drawing(&mut *self.canvas, &drawing, &self.target);
                return;
            }
        }
        let mut pen = self.pen.clone();
        if pattern.kind == LinetypeKind::ModelLinetype
            && let Some(linetype) = pattern.linetype
        {
            let dash = DashOptions {
                view_scale: self.view.scale,
                ..DashOptions::default()
            };
            let dashes = qt_dash_pattern(
                linetype,
                &dash,
                katana_math::cpp::max(self.pen_width, 0.01),
            );
            if !dashes.is_empty() {
                pen.style = PenStyle::Custom(dashes);
            }
        }
        let polygon: Vec<PointF> = path.vertices.iter().map(|vertex| self.at(*vertex)).collect();
        self.canvas.set_pen(pen);
        self.canvas.set_brush(Brush::none());
        if path.closed {
            self.canvas.draw_polygon(&polygon, FillRule::OddEven);
        } else {
            self.canvas.draw_polyline(&polygon);
        }
    }

    /// The symbol the entry names, as the plan stamps it.
    fn symbol_at(&self, at: Point2) -> StyleDrawing {
        point_symbol_drawing(
            self.library,
            &self.entry.symbol,
            at,
            self.entry.symbol_size,
            0.0,
            self.metres_per_millimetre,
            POINT_MARKER_PAPER_MILLIMETRES * self.metres_per_millimetre,
        )
    }

    fn line(&mut self) -> bool {
        // The whole cell's width, so a pattern shows as much of itself as ten millimetres of the
        // plan would.
        let half = self.half_width();
        let path = Polyline2::new(vec![Point2::new(-half, 0.0), Point2::new(half, 0.0)], false);
        self.stroke(&path);
        // A style's symbol sits at every vertex of a line (decision D8).
        if !self.entry.symbol.is_empty() {
            for end in &path.vertices {
                let drawing = self.symbol_at(*end);
                paint_style_drawing(&mut *self.canvas, &drawing, &self.target);
            }
        }
        true
    }

    fn symbol(&mut self) -> bool {
        let drawing = self.symbol_at(Point2::new(0.0, 0.0));
        if drawing.empty() {
            return false;
        }
        // At its plotted size, unless that overflows the cell: then as large as fits, centred, the
        // pen's width kept. (QFont("Arial"): the face at the application's size.)
        let extent = painted_extent(
            &drawing,
            &Font::family("Arial", FontSize::default()),
            self.canvas.metrics(),
        );
        let mut fitted = self.target.clone();
        if !extent.empty() {
            let mut shrink: f64 = 1.0;
            if extent.width() > 2.0 * self.half_width() {
                shrink = katana_math::cpp::min(shrink, 2.0 * self.half_width() / extent.width());
            }
            if extent.height() > 2.0 * self.half_height() {
                shrink = katana_math::cpp::min(shrink, 2.0 * self.half_height() / extent.height());
            }
            fitted.view.scale = self.view.scale * shrink;
            fitted.view.center = extent.center();
        }
        paint_style_drawing(&mut *self.canvas, &drawing, &fitted);
        true
    }

    fn point(&mut self) -> bool {
        // The plain mark: a cross 2 mm across (POINT_MARKER_PAPER_MILLIMETRES).
        let r = POINT_MARKER_PAPER_MILLIMETRES * self.context.pixels_per_millimetre;
        let centre = self.at(Point2::new(0.0, 0.0));
        self.canvas.set_pen(self.pen.clone());
        self.canvas
            .draw_line(centre + PointF::new(-r, 0.0), centre + PointF::new(r, 0.0));
        self.canvas
            .draw_line(centre + PointF::new(0.0, -r), centre + PointF::new(0.0, r));
        true
    }

    fn area(&mut self) -> bool {
        // A swatch a little inside the cell, so its outline's weight shows.
        let inset = 0.3 * self.metres_per_millimetre;
        let w = katana_math::cpp::max(self.half_width() - inset, 0.0);
        let h = katana_math::cpp::max(self.half_height() - inset, 0.0);
        let swatch = Polyline2::new(
            vec![
                Point2::new(-w, -h),
                Point2::new(w, -h),
                Point2::new(w, h),
                Point2::new(-w, h),
            ],
            true,
        );
        if let Some(pattern) = self.model.hatch_patterns.find(&self.entry.hatch_pattern)
            && !pattern.draws_nothing()
        {
            let options = HatchOptions {
                view_scale: self.view.scale,
                ..HatchOptions::default()
            };
            let polygon: Vec<PointF> = swatch
                .vertices
                .iter()
                .map(|vertex| self.at(*vertex))
                .collect();
            match hatch_drawing(pattern, &options) {
                HatchDrawing::None => {}
                HatchDrawing::Solid => {
                    let mut path = Path::new();
                    path.add_polygon(&polygon);
                    path.close_subpath();
                    self.canvas.fill_path(&path, &Brush::solid(self.fill_ink));
                }
                HatchDrawing::Lines => {
                    // The finest standard pen, as the plan hatches on paper.
                    self.canvas.set_pen(Pen {
                        color: self.ink,
                        width: HATCH_LINE_PAPER_MILLIMETRES
                            * self.line_scale
                            * self.context.pixels_per_millimetre,
                        ..Pen::default()
                    });
                    for hatch in hatch_segments(&swatch, pattern) {
                        let (from, to) = (self.at(hatch.start), self.at(hatch.end));
                        self.canvas.draw_line(from, to);
                    }
                }
            }
        }
        self.stroke(&swatch);
        true
    }

    fn text(&mut self) -> bool {
        // Two millimetres of capitals, or what the cell's height allows.
        let cap_mm =
            katana_math::cpp::min(2.0, 0.8 * self.cell.height / self.context.pixels_per_millimetre);
        // 0.716: Arial's cap height over its em, as the sheet painter has it.
        let pixels = ((cap_mm * self.context.pixels_per_millimetre / 0.716).round() as i32).max(1);
        self.canvas
            .set_font(Font::family("Arial", FontSize::Pixels(pixels)));
        self.canvas.set_pen(Pen::new(self.ink));
        let left = self.at(Point2::new(-self.half_width(), 0.0));
        self.canvas.draw_text(
            left + PointF::new(0.0, 0.5 * cap_mm * self.context.pixels_per_millimetre),
            "Abc",
        );
        true
    }
}

/// Paints `entry`'s sample into `cell`, in the canvas's device units. False when nothing was drawn
/// (an empty cell, a symbol that draws nothing).
pub fn paint_legend_sample(
    canvas: &mut dyn Canvas,
    entry: &LegendEntry,
    cell: RectF,
    context: &LegendSampleContext<'_>,
) -> bool {
    if !(cell.width > 0.0) || !(cell.height > 0.0) || !(context.pixels_per_millimetre > 0.0) {
        return false;
    }
    canvas.save();
    canvas.translate(cell.x, cell.y);
    // A pattern's strokes may reach above and below its line; they stop half a row's gap past the
    // cell, where the next row begins.
    let spill = 0.45 * context.pixels_per_millimetre;
    canvas.set_clip_rect(
        RectF::new(0.0, -spill, cell.width, cell.height + 2.0 * spill),
        true,
    );
    let drawn = Sample::new(&mut *canvas, entry, cell, context).paint();
    canvas.restore();
    drawn
}
