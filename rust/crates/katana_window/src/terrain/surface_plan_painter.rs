//! The drawing's surfaces in the plan view, each in its display style (docs/surface_display.md,
//! "In plan"): the plan painter's surface layer, drawn beneath the entities by paint_plan, so the
//! plan view, File > Plot to PDF and every sheet viewport show a surface alike. The port of
//! terrain/surface_plan_painter.
//!
//! WHAT is drawn - the faces' image, which edges, which contours, the labels, to what level of
//! detail - is cad's surface_plan's decision, made and tested in katana_cad. This file only draws
//! it through the plan painter's frame: the pens (a hairline on screen, ISO pen widths on paper,
//! white printing black), the image laid where the model puts it, and the labels turned upright.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(a > b) is kept: it rejects NaN, which a > b alone would let through."
)]

use katana_cad::plot::{PlotColourMode, PlotSettings, paper_colour};
use katana_cad::terrain::surface_colouring::{SurfaceLegend, SurfaceLegendCard};
use katana_cad::terrain::surface_plan::{
    PLAN_PAPER_PIXEL_MILLIMETRES, SurfacePlan, SurfacePlanCache, SurfacePlanView, SurfaceProblem,
    plan_image_grid, surface_plan,
};
use katana_cad::terrain::surface_style::{SurfaceStyleSet, style_of};
use katana_cad::view_transform::ViewTransform;
use katana_entity::Color as EntityColor;
use katana_geometry::{Box2, Point2};
use katana_terrain::surface_store::SurfaceStore;
use katana_terrain::tin_surface::TinSurface;

use crate::canvas::{
    Brush, Canvas, Color, Font, FontMetrics, FontSize, Image, ImageFormat, LinearGradient, Path,
    Pen, PenCap, PenJoin, PenStyle, PointF, RectF, RenderHint, SizeF, Transform,
};
use crate::drawing::feedback_painter::{darker, lighter};
use crate::plotting::plot_style::plot_image;

// Pens: on screen in pixels, on paper in millimetres from the ISO 128 pen series (0.13, 0.18,
// 0.25, 0.35, 0.5 mm) - the finest for the triangles, which are context, heavier for what a
// reader follows: the boundary and the major contours.
pub const SURFACE_TRIANGLE_PAPER_MILLIMETRES: f64 = 0.13;
pub const SURFACE_MINOR_CONTOUR_PAPER_MILLIMETRES: f64 = 0.18;
pub const SURFACE_BREAKLINE_PAPER_MILLIMETRES: f64 = 0.25;
pub const SURFACE_MAJOR_CONTOUR_PAPER_MILLIMETRES: f64 = 0.35;
pub const SURFACE_BOUNDARY_PAPER_MILLIMETRES: f64 = 0.35;
/// The boundary, the breaklines and the majors on screen.
pub const SURFACE_HEAVY_LINE_PIXELS: f64 = 1.5;
/// A vertex's mark: a square this wide.
pub const SURFACE_POINT_PIXELS: f64 = 3.0;
pub const SURFACE_POINT_PAPER_MILLIMETRES: f64 = 0.5;
/// A contour label's text height: ISO 3098's smallest lettering on paper, 1.8 mm, and about that
/// at a screen's 96 dpi (1.8 mm is 6.8 px; 10 px reads at arm's length).
pub const SURFACE_LABEL_PIXELS: f64 = 10.0;
pub const SURFACE_LABEL_PAPER_MILLIMETRES: f64 = 1.8;

#[derive(Clone, Debug)]
pub struct SurfacePaintOptions<'a> {
    pub paper: bool,
    /// The device's pixels per millimetre (PlanPaintOptions::pixels_per_millimetre).
    pub pixels_per_millimetre: f64,
    /// On screen, device pixels per view pixel (the display's ratio), so the faces' image has a
    /// pixel per device pixel.
    pub device_ratio: f64,
    /// On paper, the share of the device's resolution the image is embedded at (the plan
    /// painter's imagery share: the imagery dpi cap), and at most this many pixels.
    pub imagery_share: f64,
    pub image_pixel_cap: usize,
    /// On paper, the plot's colour rule (cad's paper_colour, plot_image).
    pub plot: Option<&'a PlotSettings>,
    /// The frame's turn (PlanFrame::rotation): labels are kept upright on the device.
    pub rotation: f64,
    pub font_family: String,
    /// What a label is haloed in: the view's ground on screen, the paper on paper.
    pub halo: Color,
}

impl Default for SurfacePaintOptions<'_> {
    fn default() -> Self {
        Self {
            paper: false,
            pixels_per_millimetre: 96.0 / 25.4,
            device_ratio: 1.0,
            imagery_share: 1.0,
            image_pixel_cap: 16_000_000,
            plot: None,
            rotation: 0.0,
            font_family: String::from("Segoe UI"),
            halo: Color::rgb(0x1e, 0x23, 0x29),
        }
    }
}

/// A legend the surfaces drawn would show: whose, and what it says. The 3D scene's are the same
/// card (cad's SceneLayers::legends).
pub type PaintedLegend = SurfaceLegendCard;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfacePaintStats {
    /// Visible, with something in view.
    pub surfaces_drawn: usize,
    pub triangles_in_view: usize,
    pub edges_stroked: usize,
    pub triangles_tinted: usize,
    pub contour_runs: usize,
    pub labels: usize,
    pub image_pixels: usize,
    /// The pixels of the faces' image that cover the surface: cad's count, taken from the faces
    /// as it lays them (SurfacePlanStats::pixels_covered), not from the painted image - the C++
    /// takes it so, and the headless record's covered= is this.
    pub pixels_covered: usize,
    /// Why part of a style was not drawn, one for each surface that has one (its name, its style
    /// and the reason); empty when all was.
    pub problems: Vec<SurfaceProblem>,
    /// The legends of the surfaces drawn whose styles colour their faces, in the order drawn.
    pub legends: Vec<PaintedLegend>,
}

fn canvas_colour(colour: EntityColor) -> Color {
    Color::rgba(colour.r, colour.g, colour.b, colour.a)
}

/// A colour as the medium prints it: on paper the plot's rule (white prints black, greyscale,
/// monochrome - cad's paper_colour), on screen as it is.
fn ink_of(colour: EntityColor, options: &SurfacePaintOptions) -> Color {
    match options.plot {
        Some(plot) if options.paper => canvas_colour(paper_colour(colour, plot)),
        _ => canvas_colour(colour),
    }
}

/// A pen of `pixels` on screen - cosmetic, so a turn or a scale of the painter cannot thicken it -
/// or of `millimetres` of paper.
fn pen_of(colour: Color, pixels: f64, millimetres: f64, options: &SurfacePaintOptions) -> Pen {
    if options.paper {
        return Pen::with(
            colour,
            millimetres * options.pixels_per_millimetre,
            PenStyle::Solid,
            PenCap::Round,
            PenJoin::Round,
        );
    }
    Pen {
        cosmetic: true,
        ..Pen::with(
            colour,
            pixels,
            PenStyle::Solid,
            PenCap::Square,
            PenJoin::Bevel,
        )
    }
}

fn to_screen(view: &ViewTransform, points: &[Point2]) -> Vec<PointF> {
    points
        .iter()
        .map(|&point| {
            let p = view.world_to_screen(point);
            PointF::new(p.x, p.y)
        })
        .collect()
}

/// The faces' image as the canvas's: cad lays it premultiplied 0xAARRGGBB (a QImage's
/// ARGB32_Premultiplied, which the C++ wraps without a copy); the canvas's Image is premultiplied
/// RGBA bytes.
fn image_of(width: i32, height: i32, pixels: &[u32]) -> Option<Image> {
    let mut bytes = Vec::with_capacity(pixels.len() * 4);
    for &pixel in pixels {
        bytes.extend_from_slice(&[
            (pixel >> 16) as u8,
            (pixel >> 8) as u8,
            pixel as u8,
            (pixel >> 24) as u8,
        ]);
    }
    Image::from_bytes(
        u32::try_from(width).ok()?,
        u32::try_from(height).ok()?,
        ImageFormat::Rgba8Premultiplied,
        bytes,
    )
    .ok()
}

/// BT.709 luminance of a colour's sRGB values, near enough for "can this be read".
fn luminance(colour: Color) -> f64 {
    0.2126 * f64::from(colour.r) / 255.0
        + 0.7152 * f64::from(colour.g) / 255.0
        + 0.0722 * f64::from(colour.b) / 255.0
}

/// One surface's plan, drawn: what paint_surfaces_in_plan draws for each, and what the style
/// manager's preview draws.
pub fn paint_surface_plan(
    canvas: &mut dyn Canvas,
    plan: &SurfacePlan,
    view: &ViewTransform,
    options: &SurfacePaintOptions,
) {
    canvas.save();

    // The faces, the tint of what is too dense to stroke, and points too dense to mark: one image,
    // laid where its box is, each image pixel a device pixel on an unturned screen.
    if !plan.image.empty()
        && let Some(image) = image_of(plan.image.width, plan.image.height, &plan.image.pixels)
    {
        let size = plan.image.r#box.width() / f64::from(plan.image.width);
        let corner =
            view.world_to_screen(Point2::new(plan.image.r#box.min.x, plan.image.r#box.max.y));
        let step = size * view.scale;
        canvas.save();
        canvas.set_world_transform(
            Transform::new(step, 0.0, 0.0, step, corner.x, corner.y),
            true,
        );
        // Smoothed only on paper, where the image is coarser than the device; on screen each image
        // pixel is a device pixel.
        canvas.set_render_hint(RenderHint::SmoothPixmapTransform, options.paper);
        match options.plot {
            Some(plot) if options.paper && plot.colour_mode != PlotColourMode::Colour => {
                canvas.draw_image_at(PointF::new(0.0, 0.0), &plot_image(&image, plot));
            }
            _ => canvas.draw_image_at(PointF::new(0.0, 0.0), &image),
        }
        canvas.restore();
    }

    canvas.set_brush(Brush::none());
    if !plan.triangles.is_empty() {
        canvas.set_pen(pen_of(
            ink_of(plan.triangle_colour, options),
            1.0,
            SURFACE_TRIANGLE_PAPER_MILLIMETRES,
            options,
        ));
        let lines: Vec<(PointF, PointF)> = plan
            .triangles
            .iter()
            .map(|edge| {
                let a = view.world_to_screen(edge.start);
                let b = view.world_to_screen(edge.end);
                (PointF::new(a.x, a.y), PointF::new(b.x, b.y))
            })
            .collect();
        canvas.draw_lines(&lines);
    }
    // Minors then majors (surface_plan's order), each in its own pen.
    if !plan.contours.is_empty() {
        let minor = pen_of(
            ink_of(plan.minor_colour, options),
            1.0,
            SURFACE_MINOR_CONTOUR_PAPER_MILLIMETRES,
            options,
        );
        let major = pen_of(
            ink_of(plan.major_colour, options),
            SURFACE_HEAVY_LINE_PIXELS,
            SURFACE_MAJOR_CONTOUR_PAPER_MILLIMETRES,
            options,
        );
        for contour in &plan.contours {
            canvas.set_pen(if contour.major {
                major.clone()
            } else {
                minor.clone()
            });
            canvas.draw_polyline(&to_screen(view, &contour.points));
        }
    }
    if !plan.breaklines.is_empty() {
        canvas.set_pen(pen_of(
            ink_of(plan.breakline_colour, options),
            SURFACE_HEAVY_LINE_PIXELS,
            SURFACE_BREAKLINE_PAPER_MILLIMETRES,
            options,
        ));
        for line in &plan.breaklines {
            canvas.draw_polyline(&to_screen(view, line));
        }
    }
    if !plan.boundary.is_empty() {
        canvas.set_pen(pen_of(
            ink_of(plan.boundary_colour, options),
            SURFACE_HEAVY_LINE_PIXELS,
            SURFACE_BOUNDARY_PAPER_MILLIMETRES,
            options,
        ));
        for line in &plan.boundary {
            canvas.draw_polyline(&to_screen(view, line));
        }
    }
    if !plan.points.is_empty() {
        let side = if options.paper {
            SURFACE_POINT_PAPER_MILLIMETRES * options.pixels_per_millimetre
        } else {
            SURFACE_POINT_PIXELS
        };
        canvas.set_pen(Pen::none());
        canvas.set_brush(Brush::solid(ink_of(plan.point_colour, options)));
        let marks: Vec<RectF> = plan
            .points
            .iter()
            .map(|&point| {
                let p = view.world_to_screen(point);
                RectF::new(p.x - side / 2.0, p.y - side / 2.0, side, side)
            })
            .collect();
        canvas.draw_rects(&marks);
        canvas.set_brush(Brush::none());
    }

    // The labels on the majors, each haloed in the ground so it reads where the contour runs
    // through it, and turned to read upright on the device whatever the contour's direction and
    // the frame's turn.
    if !plan.labels.is_empty() {
        let pixels = if options.paper {
            SURFACE_LABEL_PAPER_MILLIMETRES * options.pixels_per_millimetre
        } else {
            SURFACE_LABEL_PIXELS
        };
        // A whole pixel size: at 300 dpi a 1.8 mm label is 21.3 px and set at 21, 0.03 mm short -
        // below a pen's width. (std::lround: half away from zero, as f64::round.)
        let font = Font::family(
            &options.font_family,
            FontSize::Pixels(1.max(pixels.round() as i32)),
        );
        // In the majors' colour - unless that is too near the halo's to be read (a dark brown
        // major on the screen's dark ground): then lighter on a dark halo, darker on a light one.
        // Luminance by the BT.709 weights on the sRGB values, near enough for "can this be read";
        // the 0.3 is chosen here, by looking: the default major brown (110, 70, 40), 0.30, on the
        // screen's ground (30, 35, 41), 0.13, is 0.16 apart and hard to read; the minor brown
        // (160, 120, 80), 0.49, is 0.36 apart and plain.
        let mut ink = ink_of(plan.major_colour, options);
        if (luminance(ink) - luminance(options.halo)).abs() < 0.3 {
            ink = if luminance(options.halo) < 0.5 {
                lighter(ink, 200)
            } else {
                darker(ink, 200)
            };
        }
        let halo = Pen::with(
            options.halo,
            katana_math::cpp::max(2.0, 0.25 * pixels),
            PenStyle::Solid,
            PenCap::Square,
            PenJoin::Round,
        );
        let degrees = 180.0 / std::f64::consts::PI;
        for label in &plan.labels {
            let at = view.world_to_screen(label.at);
            // The contour's direction on the device, clockwise in degrees (screen y runs down; the
            // frame turns the drawing counter-clockwise by `rotation`).
            let mut device = -(label.angle + options.rotation) * degrees;
            let mut turn = -label.angle * degrees;
            device = katana_math::cpp::remainder(device, 360.0);
            if device > 90.0 || device < -90.0 {
                turn += 180.0;
            }
            let (advance, ascent, descent) = {
                let metrics = canvas.metrics();
                (
                    metrics.horizontal_advance(&font, &label.text),
                    metrics.ascent(&font),
                    metrics.descent(&font),
                )
            };
            let mut path = Path::new();
            path.add_text(
                PointF::new(-advance / 2.0, (ascent - descent) / 2.0),
                &font,
                &label.text,
            );
            canvas.save();
            canvas.translate(at.x, at.y);
            canvas.rotate(turn);
            canvas.stroke_path(&path, &halo);
            canvas.fill_path(&path, &Brush::solid(ink));
            canvas.restore();
        }
    }
    canvas.restore();
}

/// Every visible surface of `surfaces`, in the order they were added, each in its style (cad's
/// style_of), seen through `view` (model to the viewport's pixels, y down) on a canvas that
/// already carries the frame's origin and turn. `visible` is the model box the frame shows (the
/// plan painter's visible_box). The canvas's state is the same afterwards.
pub fn paint_surfaces_in_plan(
    canvas: &mut dyn Canvas,
    surfaces: &SurfaceStore,
    styles: &SurfaceStyleSet,
    view: &ViewTransform,
    visible: &Box2,
    options: &SurfacePaintOptions,
    cache: &mut SurfacePlanCache,
) -> SurfacePaintStats {
    let mut stats = SurfacePaintStats::default();
    // Keep what the drawing still has, hidden or not: showing a surface again must not build its
    // grid again.
    let alive: Vec<&TinSurface> = surfaces
        .all()
        .iter()
        .filter_map(|named| named.surface.as_deref())
        .collect();
    cache.retain(&alive);
    if !(view.scale > 0.0) || visible.empty() {
        return stats;
    }

    // A view pixel in the model, which the level of detail measures: a logical pixel on screen,
    // PLAN_PAPER_PIXEL_MILLIMETRES of paper on paper.
    let units_per_pixel = if options.paper {
        PLAN_PAPER_PIXEL_MILLIMETRES * options.pixels_per_millimetre / view.scale
    } else {
        1.0 / view.scale
    };
    // An image pixel: a device pixel on screen; on paper the device's pixel at the imagery share
    // (the dpi cap).
    let image_pixel = if options.paper {
        1.0 / (view.scale * katana_math::cpp::clamp(options.imagery_share, 1.0e-6, 1.0))
    } else {
        1.0 / (view.scale * katana_math::cpp::max(options.device_ratio, 1.0e-6))
    };
    for named in surfaces.all() {
        let Some(surface) = named.surface.as_deref() else {
            continue;
        };
        if !named.visible || surface.empty() || !surface.bounds().intersects(*visible) {
            continue;
        }
        let style = style_of(styles, named);
        let grid = plan_image_grid(
            visible,
            surface.bounds(),
            image_pixel,
            options.image_pixel_cap,
        );
        let plan_view = SurfacePlanView {
            visible: *visible,
            units_per_pixel,
            image_box: grid.r#box,
            image_width: grid.width,
            image_height: grid.height,
            ..SurfacePlanView::default()
        };
        let drawn = surface_plan(named, style, &plan_view, cache);
        if drawn.stats.triangles_in_view == 0 {
            continue;
        }
        paint_surface_plan(canvas, &drawn, view, options);
        stats.surfaces_drawn += 1;
        stats.triangles_in_view += drawn.stats.triangles_in_view;
        stats.edges_stroked += drawn.stats.edges_stroked;
        stats.triangles_tinted += drawn.stats.triangles_tinted;
        stats.contour_runs += drawn.stats.contour_runs;
        stats.labels += drawn.stats.labels;
        stats.image_pixels += drawn.stats.image_pixels;
        stats.pixels_covered += drawn.stats.pixels_covered;
        if !drawn.problem.is_empty() {
            stats.problems.push(SurfaceProblem {
                surface: named.name.clone(),
                style: style.name.clone(),
                reason: drawn.problem.clone(),
            });
        }
        if let Some(legend) = &drawn.legend {
            stats.legends.push(SurfaceLegendCard {
                surface: named.name.clone(),
                style: style.name.clone(),
                legend: legend.clone(),
            });
        }
    }
    stats
}

// ---- the legend ---------------------------------------------------------------------------------

/// A legend in a panel holds at most this many rows; past that the last says how many more.
pub const LEGEND_ROWS: usize = 16;

/// A swatch legend's row: an entry each up to the cap, the last saying how many are left out.
struct LegendRow {
    colour: Color,
    label: String,
    more: bool,
}

/// A gradient's stop: its place along the bar, 0 at the foot to 1 at the head, by its value; and
/// the label written beside it, if it has one.
struct LegendStop {
    at: f64,
    colour: Color,
    label: String,
}

/// What a legend's panel is made of, laid out once so that the size the plan painter measures
/// before it draws (measure_surface_legend) and the picture paint_surface_legend makes are the
/// one calculation.
struct LegendLayout {
    rows: Vec<LegendRow>,
    stops: Vec<LegendStop>,
    title: String,
    line: f64,
    pad: f64,
    swatch: f64,
    bar_height: f64,
    width: f64,
    height: f64,
}

/// Where along the bar each entry of a gradient lies. By value when every entry has one and they
/// rise: the built-in Cut Fill style has stops at -2, -1, -0.5, 0, 0.5, 1 and 2, which are not
/// evenly spaced, and a colour placed by its index reads as the wrong depth against the labelled
/// ends. A ramp's nine samples are evenly spaced, so by value is by index for them; entries
/// without values fall back to by index.
fn gradient_positions(legend: &SurfaceLegend) -> Vec<f64> {
    let count = legend.entries.len();
    let mut at = vec![0.0; count];
    if count < 2 {
        return at;
    }
    let first = legend.entries[0].from;
    let last = legend.entries[count - 1].from;
    let valued = legend
        .entries
        .iter()
        .all(|entry| entry.from.is_some_and(f64::is_finite))
        && matches!((first, last), (Some(low), Some(high)) if high > low);
    for (i, entry) in legend.entries.iter().enumerate() {
        let place = match (valued, entry.from, first, last) {
            (true, Some(value), Some(low), Some(high)) => (value - low) / (high - low),
            _ => i as f64 / (count - 1) as f64,
        };
        at[i] = katana_math::cpp::clamp(place, 0.0, 1.0);
    }
    at
}

fn layout_legend(
    legend: &SurfaceLegend,
    caption: &str,
    font: &Font,
    metrics: &dyn FontMetrics,
) -> LegendLayout {
    let line = metrics.height(font);
    let mut layout = LegendLayout {
        rows: Vec::new(),
        stops: Vec::new(),
        title: legend.title.clone(),
        line,
        pad: 6.0,
        swatch: katana_math::cpp::max(10.0, line * 0.8),
        bar_height: 0.0,
        width: 0.0,
        height: 0.0,
    };
    let mut label_width = katana_math::cpp::max(
        metrics.horizontal_advance(font, caption),
        metrics.horizontal_advance(font, &layout.title),
    );
    if legend.gradient {
        let at = gradient_positions(legend);
        for (i, entry) in legend.entries.iter().enumerate() {
            layout.stops.push(LegendStop {
                at: at[i],
                colour: canvas_colour(entry.colour),
                label: entry.label.clone(),
            });
            label_width = katana_math::cpp::max(
                label_width,
                layout.swatch + layout.pad + metrics.horizontal_advance(font, &entry.label),
            );
        }
        layout.bar_height = 6.0 * layout.line;
    } else {
        let shown = if legend.entries.len() > LEGEND_ROWS {
            LEGEND_ROWS - 1
        } else {
            legend.entries.len()
        };
        // Highest first, as a map key reads: high ground at the top.
        for i in 0..shown {
            let entry = &legend.entries[legend.entries.len() - 1 - i];
            layout.rows.push(LegendRow {
                colour: canvas_colour(entry.colour),
                label: entry.label.clone(),
                more: false,
            });
        }
        if shown < legend.entries.len() {
            layout.rows.push(LegendRow {
                colour: Color::default(),
                label: format!("+ {} more", legend.entries.len() - shown),
                more: true,
            });
        }
        for row in &layout.rows {
            label_width = katana_math::cpp::max(
                label_width,
                layout.swatch + layout.pad + metrics.horizontal_advance(font, &row.label),
            );
        }
    }
    let body_height = if legend.gradient {
        layout.bar_height
    } else {
        layout.rows.len() as f64 * (layout.swatch + 2.0)
    };
    layout.width = label_width + 2.0 * layout.pad;
    layout.height = 2.0 * layout.line + body_height + 2.0 * layout.pad + 4.0;
    layout
}

/// The size a legend's panel will be, without drawing it: what a caller stacking legends asks to
/// see whether the next fits. Empty (0 x 0) for a legend with no entries.
pub fn measure_surface_legend(
    legend: &SurfaceLegend,
    caption: &str,
    font: &Font,
    metrics: &dyn FontMetrics,
) -> SizeF {
    if legend.entries.is_empty() {
        return SizeF::default();
    }
    let layout = layout_legend(legend, caption, font, metrics);
    SizeF::new(layout.width, layout.height)
}

/// A legend in a panel whose top-right corner is `top_right`, in the canvas's units: the caption
/// (the surface and its style), the title, then a swatch a band, stop or sector with its label -
/// or, for a smooth ramp or smooth stops, one bar from the lowest value at the foot to the highest
/// at the head, each stop at its own value along it and the ends always labelled (an inner label
/// where it clears the one above it by a line). At most LEGEND_ROWS rows; past that the last says
/// how many more. Returns the panel's rectangle.
pub fn paint_surface_legend(
    canvas: &mut dyn Canvas,
    legend: &SurfaceLegend,
    caption: &str,
    top_right: PointF,
    font: &Font,
    text: Color,
    backing: Color,
) -> RectF {
    if legend.entries.is_empty() {
        return RectF::default();
    }
    let (layout, ascent, descent) = {
        let metrics = canvas.metrics();
        (
            layout_legend(legend, caption, font, metrics),
            metrics.ascent(font),
            metrics.descent(font),
        )
    };
    let pad = layout.pad;
    let line = layout.line;
    let swatch = layout.swatch;
    let panel = RectF::new(
        top_right.x - layout.width,
        top_right.y,
        layout.width,
        layout.height,
    );

    canvas.save();
    canvas.set_render_hint(RenderHint::Antialiasing, true);
    canvas.set_font(font.clone());
    canvas.set_pen(Pen::none());
    canvas.set_brush(Brush::solid(backing));
    canvas.draw_rounded_rect(panel, 4.0, 4.0);
    canvas.set_pen(Pen::new(text));
    let mut y = panel.top() + pad;
    canvas.draw_text(PointF::new(panel.left() + pad, y + ascent), caption);
    y += line;
    canvas.draw_text(PointF::new(panel.left() + pad, y + ascent), &layout.title);
    y += line + 4.0;
    if legend.gradient {
        let bar = RectF::new(panel.left() + pad, y, swatch, layout.bar_height);
        let mut ramp = LinearGradient::new(bar.bottom_left(), bar.top_left());
        for stop in &layout.stops {
            ramp.set_color_at(stop.at, stop.colour);
        }
        canvas.set_pen(Pen::none());
        canvas.set_brush(Brush::linear(ramp));
        canvas.draw_rect(bar);
        canvas.set_pen(Pen::new(text));
        // The labelled stops beside the bar at their values: the two ends always (the head
        // against the bar's top, the foot against its bottom), an inner one only where it clears
        // the label above it and the foot's by a line.
        let centre_of = |at: f64| {
            katana_math::cpp::clamp(
                bar.bottom() - at * bar.height,
                bar.top() + 0.5 * line,
                bar.bottom() - 0.5 * line,
            )
        };
        // Highest first: the stops rise, so the last is the head.
        let labelled: Vec<&LegendStop> = layout
            .stops
            .iter()
            .filter(|stop| !stop.label.is_empty())
            .rev()
            .collect();
        // The labels written, in the order the C++ writes them.
        let mut written: Vec<&LegendStop> = Vec::new();
        if let Some(&head) = labelled.first() {
            written.push(head);
            if labelled.len() > 1 {
                let foot = centre_of(labelled[labelled.len() - 1].at);
                let mut above = centre_of(head.at);
                for &inner in &labelled[1..labelled.len() - 1] {
                    let here = centre_of(inner.at);
                    if here - above >= line && foot - here >= line {
                        written.push(inner);
                        above = here;
                    }
                }
                written.push(labelled[labelled.len() - 1]);
            }
        }
        for stop in written {
            canvas.draw_text(
                PointF::new(
                    bar.right() + pad,
                    centre_of(stop.at) + 0.5 * (ascent - descent),
                ),
                &stop.label,
            );
        }
    } else {
        for row in &layout.rows {
            if !row.more {
                canvas.set_pen(Pen::none());
                canvas.set_brush(Brush::solid(row.colour));
                canvas.draw_rect(RectF::new(panel.left() + pad, y, swatch, swatch));
            }
            canvas.set_pen(Pen::new(text));
            canvas.draw_text(
                PointF::new(panel.left() + pad + swatch + pad, y + swatch - descent),
                &row.label,
            );
            y += swatch + 2.0;
        }
    }
    canvas.restore();
    panel
}

/// What a stack of legends drew: each panel's caption and rectangle, in the order drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawnLegend {
    pub caption: String,
    pub panel: RectF,
}

/// Where and how a stack of legends goes down a view's corner.
#[derive(Clone, Debug)]
pub struct LegendStack {
    /// The top-right corner of the first panel, in the canvas's units.
    pub top_right: PointF,
    /// A later panel whose foot would pass this is left out and ends the stack: a key with its
    /// lowest rows cut off by the view's edge reads as a key that ends there. The first is drawn
    /// whatever its height (a view too short for it clips it as it clips anything).
    pub bottom: f64,
    /// The gap between one panel's foot and the next one's top.
    pub gap: f64,
    /// At most this many cards are considered (more would cover the drawing they explain); one
    /// that is empty counts against it and is not drawn.
    pub maximum: usize,
    pub font: Font,
    pub text: Color,
    pub backing: Color,
}

impl Default for LegendStack {
    fn default() -> Self {
        Self {
            top_right: PointF::default(),
            bottom: 0.0,
            gap: 10.0,
            maximum: 3,
            font: Font::default(),
            text: Color::default(),
            backing: Color::default(),
        }
    }
}

/// The legends of `cards` stacked down from `stack.top_right`, each captioned "surface - style",
/// in order: the one stack the plan view and the 3D view draw their legends through, so that they
/// stack, fit and leave out alike. Each is measured before it is drawn (measure_surface_legend),
/// so none is cut by the edge and none overlaps another. Returns what was drawn.
pub fn paint_surface_legend_stack(
    canvas: &mut dyn Canvas,
    cards: &[SurfaceLegendCard],
    stack: &LegendStack,
) -> Vec<DrawnLegend> {
    let mut drawn: Vec<DrawnLegend> = Vec::new();
    let mut corner = stack.top_right;
    for card in cards.iter().take(stack.maximum) {
        let caption = format!("{} - {}", card.surface, card.style);
        // Measured first, so that a panel that would not fit is never drawn.
        let size = measure_surface_legend(&card.legend, &caption, &stack.font, canvas.metrics());
        if size.width <= 0.0 || size.height <= 0.0 {
            continue;
        }
        if !drawn.is_empty() && corner.y + size.height > stack.bottom {
            break;
        }
        let panel = paint_surface_legend(
            canvas,
            &card.legend,
            &caption,
            corner,
            &stack.font,
            stack.text,
            stack.backing,
        );
        corner.y = panel.bottom() + stack.gap;
        drawn.push(DrawnLegend { caption, panel });
    }
    drawn
}
