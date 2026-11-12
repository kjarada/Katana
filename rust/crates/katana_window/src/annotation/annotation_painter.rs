//! Painting a cad annotation Drawing (docs/annotation.md, "Drawing"): the port of
//! annotation/annotation_painter.
//!
//! The layout - where every line of text, leader and callout goes - was done headlessly in
//! katana_cad; this only strokes, fills and sets the pieces through the plan painter's transform.
//! Kept out of plan_painter.rs so the annotation code lives in one directory and the painter gains
//! only the calls into it.

use std::collections::BTreeMap;
use std::sync::Arc;

use katana_cad::annotation::drawing::{Drawing, TextFace, TextMeasure, estimated_measure};
use katana_entity::Color as EntityColor;
use katana_entity::text_block::estimated_width;
use katana_geometry::Point2;
use katana_math::numerics::RAD_TO_DEG;

use crate::canvas::{
    Brush, Canvas, Color, FillRule, Font, FontMetrics, FontSize, FontWeight, Pen, PenStyle, PointF,
};

/// The faces annotation text is set in, one Font per face at a reference size, and their measures:
/// in the C++ a font made by family name resolves the family through the font database, which per
/// text per frame was measurable (plan_painter's font_for). One per paint.
pub struct AnnotationFonts {
    default_family: String,
    fonts: BTreeMap<(String, bool, bool), Font>,
    measure: TextMeasure,
}

/// The font for `face` at the reference size.
fn font_of(default_family: &str, face: &TextFace) -> Font {
    let family = if face.family.is_empty() {
        default_family
    } else {
        face.family.as_str()
    };
    let mut font = Font::family(family, FontSize::Pixels(AnnotationFonts::REFERENCE_PIXELS));
    font.weight = if face.bold {
        FontWeight::Bold
    } else {
        FontWeight::Normal
    };
    font.italic = face.italic;
    font
}

impl AnnotationFonts {
    /// The size the fonts are made at; a run is set by scaling it, so its height is exact to the
    /// fraction of a device pixel on screen and paper alike, and its measure is the font's at a
    /// size where the metrics are not rounded coarsely.
    pub const REFERENCE_PIXELS: i32 = 100;

    /// The fonts of `default_family`, measured by `metrics` (the raster backend's). None measures
    /// by cad's estimate (estimated_measure): a painter with no backend has no fonts to measure.
    pub fn new(default_family: &str, metrics: Option<Arc<dyn FontMetrics + Send + Sync>>) -> Self {
        let measure: TextMeasure = match metrics {
            Some(metrics) => {
                let family = String::from(default_family);
                // What layout_text measures with: the font's own advance widths, per unit of
                // height (the em size), so a centred line is centred on the sheet.
                let measured: TextMeasure = Arc::new(move |line: &str, face: &TextFace| {
                    metrics.horizontal_advance(&font_of(&family, face), line)
                        / f64::from(AnnotationFonts::REFERENCE_PIXELS)
                });
                measured
            }
            None => estimated_measure(),
        };
        Self {
            default_family: String::from(default_family),
            fonts: BTreeMap::new(),
            measure,
        }
    }

    /// The font for `face` at REFERENCE_PIXELS.
    pub fn font(&mut self, face: &TextFace) -> &Font {
        let key = (face.family.clone(), face.bold, face.italic);
        let family = &self.default_family;
        self.fonts
            .entry(key)
            .or_insert_with(|| font_of(family, face))
    }

    /// What layout_text measures with.
    pub fn measure(&self) -> TextMeasure {
        Arc::clone(&self.measure)
    }
}

/// Model to the painter's coordinates.
pub type ToDevice<'a> = Box<dyn Fn(Point2) -> PointF + 'a>;
/// The colour a style's own colour becomes on a medium.
pub type ColourOf<'a> = Box<dyn Fn(EntityColor) -> Color + 'a>;

pub struct AnnotationPaintTarget<'a> {
    /// Model to the painter's coordinates.
    pub to_device: ToDevice<'a>,
    /// Device pixels per model unit: the view scale.
    pub pixels_per_unit: f64,
    /// The pen the annotation is drawn in (its entity's resolved pen), and the colour a style's
    /// own colour becomes on this medium (white prints black on paper): called only when the
    /// drawing has a colour.
    pub pen: Pen,
    pub colour_of: Option<ColourOf<'a>>,
    /// What a mask is painted in: the paper, or the view's ground.
    pub background: Color,
    /// Text smaller than this on the device is a stroke along its baseline, as plain text below
    /// three pixels is: legible it is not, and a glyph there is noise.
    pub minimum_text_pixels: f64,
}

fn polygon_of(points: &[Point2], target: &AnnotationPaintTarget) -> Vec<PointF> {
    points
        .iter()
        .map(|&point| (target.to_device)(point))
        .collect()
}

/// Paints `drawing`: masks, then strokes, outlines and fills in the pen (or the drawing's own
/// colour), then the text.
pub fn paint_annotation_drawing(
    canvas: &mut dyn Canvas,
    drawing: &Drawing,
    target: &AnnotationPaintTarget,
    fonts: &mut AnnotationFonts,
) {
    if drawing.empty() {
        return;
    }
    canvas.save();
    let mut pen = target.pen.clone();
    if let (Some(colour), Some(colour_of)) = (drawing.colour, target.colour_of.as_ref()) {
        pen.color = colour_of(colour);
    }
    // Annotation is never dashed by a linetype.
    pen.style = PenStyle::Solid;
    let ink = pen.color;

    if !drawing.masks.is_empty() {
        canvas.set_pen(Pen::none());
        canvas.set_brush(Brush::solid(target.background));
        for mask in &drawing.masks {
            canvas.draw_polygon(&polygon_of(mask, target), FillRule::OddEven);
        }
    }
    canvas.set_pen(pen);
    canvas.set_brush(Brush::none());
    for stroke in &drawing.strokes {
        canvas.draw_polyline(&polygon_of(stroke, target));
    }
    for outline in &drawing.outlines {
        canvas.draw_polygon(&polygon_of(outline, target), FillRule::OddEven);
    }
    if !drawing.fills.is_empty() {
        canvas.set_brush(Brush::solid(ink));
        for fill in &drawing.fills {
            canvas.draw_polygon(&polygon_of(fill, target), FillRule::OddEven);
        }
        canvas.set_brush(Brush::none());
    }

    for run in &drawing.texts {
        let pixels = run.height * target.pixels_per_unit;
        let origin = (target.to_device)(run.origin);
        if pixels < target.minimum_text_pixels {
            // Too small to read: a stroke along the baseline keeps it discoverable, as a plain
            // text's does.
            let width = estimated_width(&run.text) * pixels * run.width_factor;
            canvas.draw_line(
                origin,
                origin + PointF::new(run.rotation.cos(), -run.rotation.sin()) * width,
            );
            continue;
        }
        canvas.save();
        canvas.translate(origin.x, origin.y);
        canvas.rotate(-run.rotation * RAD_TO_DEG); // device y points down
        // The slant: a vertical leans right by the oblique angle, which with y down is a negative
        // horizontal shear.
        if run.oblique != 0.0 {
            canvas.shear(-run.oblique.tan(), 0.0);
        }
        let scale = pixels / f64::from(AnnotationFonts::REFERENCE_PIXELS);
        canvas.scale(scale * run.width_factor, scale);
        canvas.set_font(fonts.font(&run.face).clone());
        canvas.set_pen(Pen::new(ink));
        canvas.draw_text(PointF::new(0.0, 0.0), &run.text);
        canvas.restore();
    }
    canvas.restore();
}
