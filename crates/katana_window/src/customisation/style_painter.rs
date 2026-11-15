//! Painting a cad StyleDrawing on the canvas: the one place a library definition's strokes, dots,
//! texts and pens become pixels (the port of customisation/style_painter).
//!
//! The viewport, the plot and every preview and thumbnail paint through these functions, so none
//! of them can drift from the others. They used to be the viewport's own members, which is why a
//! preview drew something else: it could not reach them.
//!
//! Everything the caller decides comes in through a StylePaintTarget - the model-to-device
//! transform, the ENTITY PEN (its colour, width and cap: a preview that guessed its own pen showed
//! dashes one pen-width shorter than the plot, whose square caps grew every dash by that much) and
//! whether this is paper, where the paper colour rule (cad's paper_colour) turns a white library
//! pen black. Nothing here reads a widget.
//!
//! Also here: RasterBackend, what a painter needs of the raster backend beyond the canvas it is
//! handed. The C++ makes a QPainter over a QImage wherever it likes (a symbol sprite, a
//! thumbnail, a preview) and measures text with a QFontMetricsF of nothing but a font; the Rust
//! painters see only the Canvas trait, so the window hands them a backend for both. C's raster
//! canvas (tiny-skia, cosmic-text) is the window's.

use std::sync::Arc;

use katana_cad::plot::{PlotSettings, paper_colour};
use katana_cad::style_drawing::{StyleDrawing, StyleTextMark};
use katana_cad::view_transform::ViewTransform;
use katana_entity::ColourTable;
use katana_entity::colour_names::{resolve_colour, standard_colour};
use katana_geometry::{Box2, Point2};
use katana_math::numerics::RAD_TO_DEG;

use crate::canvas::{Canvas, Color, Font, FontMetrics, FontSize, Image, Pen, PenCap, PointF};

/// What a painter asks of the raster backend besides the canvas it paints on: a canvas over an
/// image of its own, as `QPainter painter(&image)` is in the C++ (a symbol's sprite, a thumbnail,
/// a picker's picture), and font measures it may keep and share, as a QFontMetricsF made from a
/// font alone is (annotation text is measured by cad's layout through a TextMeasure, an Arc that
/// may be called on any thread). The window's is C's raster backend; the integrator adapts it.
pub trait RasterBackend: Send + Sync {
    /// Paints `paint` onto `image` through a raster canvas over it (at the image's own device
    /// pixel ratio), and finishes before it returns.
    fn paint_image(&self, image: &mut Image, paint: &mut dyn FnMut(&mut dyn Canvas));
    /// The measures of the fonts the raster canvas draws with.
    fn metrics(&self) -> Arc<dyn FontMetrics + Send + Sync>;
}

/// The raster backend over the raster canvas (canvas::RasterCanvas), its fonts measured at the
/// screen's resolution: the one the window paints sprites, thumbnails and previews with.
pub struct CanvasRasterBackend;

impl RasterBackend for CanvasRasterBackend {
    fn paint_image(&self, image: &mut Image, paint: &mut dyn FnMut(&mut dyn Canvas)) {
        crate::canvas::RasterCanvas::paint_image(image, paint);
    }

    fn metrics(&self) -> Arc<dyn FontMetrics + Send + Sync> {
        Arc::new(crate::canvas::TextMetrics::screen())
    }
}

/// Where and how a style drawing is painted.
#[derive(Clone, Debug, Default)]
pub struct StylePaintTarget<'a> {
    /// Model units to device pixels.
    pub view: ViewTransform,
    /// The entity's own pen. An empty library pen ("view_colour") is exactly this; a named one
    /// changes only its colour.
    pub entity_pen: Pen,
    /// None on screen. On paper, the plot's settings: a library pen follows their paper colour
    /// rule. The entity pen's own colour is the caller's to have converted already, since the
    /// caller chose it.
    pub paper: Option<&'a PlotSettings>,
    /// Every stroke in the entity pen, library pens ignored - the selection highlight, which must
    /// read as one colour whatever the definition says.
    pub entity_pen_only: bool,
    /// The colour names of the session's customisation (the Document's customisation_state's
    /// colours), which a library pen is resolved against before the standard names: the
    /// Document's one resolver, reached through the table because nothing here sees a Document.
    /// None: the standard names alone, which is all a painter handed a library and no session
    /// has. Borrowed for the paint, as `paper` is.
    pub colours: Option<&'a ColourTable>,
}

/// Style texts at or above this many pixels are drawn at it: the same ceiling the viewport's
/// plain text has, for the same reason - a font asked for at a size of hundreds of thousands of
/// pixels, one zoom step from a survey's extent, makes the raster engine allocate glyphs larger
/// than any screen, and a glyph taller than the view reads the same at any height above it.
pub const MAXIMUM_STYLE_TEXT_PIXELS: f64 = 2000.0;
/// Style texts smaller than this are not drawn: unreadable, and the commonest cost of a
/// zoomed-out drawing full of labelled linestyles.
pub const MINIMUM_STYLE_TEXT_PIXELS: f64 = 3.0;

fn to_device(view: &ViewTransform, world: Point2) -> PointF {
    let p = view.world_to_screen(world);
    PointF::new(p.x, p.y)
}

/// The pen a library `pen` names, on the entity's pen: its colour from `colours` when given, then
/// from the standard colour names (entity's resolve_colour), through the paper colour rule when
/// `paper` is given, with the entity pen's width, cap, style and alpha kept. An empty name, or
/// one neither knows ("pen 035"), is the entity pen unchanged rather than a guess.
pub fn style_pen_for(
    entity_pen: &Pen,
    pen: &str,
    paper: Option<&PlotSettings>,
    colours: Option<&ColourTable>,
) -> Pen {
    if pen.is_empty() {
        return entity_pen.clone(); // "view_colour" in a library: whatever the entity is
    }
    // The customisation's own names, then the standard ones. A table with nothing in it is not
    // asked: it would fold the name to find nothing, on the path every stroke of a drawing with
    // no colours of its own takes.
    let colour = match colours {
        Some(table) if !table.empty() => resolve_colour(table, pen),
        _ => standard_colour(pen),
    };
    let Some(colour) = colour else {
        return entity_pen.clone(); // a pen name with no RGB here: not a guess
    };
    let ink = match paper {
        Some(settings) => paper_colour(colour, settings),
        None => colour,
    };
    // The entity pen's alpha, not the colour's: a stroke of a locked layer's entity fades with
    // the rest of it.
    Pen {
        color: Color::rgba(ink.r, ink.g, ink.b, entity_pen.color.a),
        ..entity_pen.clone()
    }
}

/// The face a style text is painted in at `pixels`: its own family when the machine has it,
/// otherwise a sans-serif of the same size. The C++ asks the font database whether the family is
/// installed and, when not, keeps the base face with a sans-serif style hint (every library text
/// names "Arial", which a Linux build does not have); the canvas's Font takes the family it
/// should try and falls back to the UI's sans itself, which is the same choice.
fn style_text_font(mut font: Font, text: &StyleTextMark, pixels: i32) -> Font {
    font.size = FontSize::Pixels(pixels.max(1));
    if !text.font.is_empty() {
        font.families = vec![text.font.clone()];
    }
    font
}

/// The height of a capital above the baseline (QFontMetricsF::capHeight). F0's FontMetrics has no
/// cap height; a capital H's ink is the measure Qt itself falls back to where a font gives none.
fn cap_height(metrics: &dyn FontMetrics, font: &Font) -> f64 {
    -metrics.bounding_rect(font, "H").top()
}

/// Where the baseline starts, relative to the anchor, in unrotated pixels. A library justifies as
/// "vertical-horizontal": "middle-centre", "top-left". A spelling this does not know draws from
/// the point, which is what an unjustified text already does.
fn justified_origin(metrics: &dyn FontMetrics, font: &Font, value: &str, justify: &str) -> PointF {
    let mut dx = 0.0;
    let mut dy = 0.0;
    if justify.contains("centre") || justify.contains("center") {
        dx = -0.5 * metrics.horizontal_advance(font, value);
    } else if justify.contains("right") {
        dx = -metrics.horizontal_advance(font, value);
    }
    if justify.contains("middle") {
        dy = 0.5 * cap_height(metrics, font);
    } else if justify.contains("top") {
        dy = cap_height(metrics, font);
    }
    PointF::new(dx, dy)
}

fn width_factor_of(text: &StyleTextMark) -> f64 {
    if text.width_factor.is_finite() && text.width_factor > 0.0 {
        text.width_factor
    } else {
        1.0
    }
}

/// One style text, in the canvas's pen.
pub fn paint_style_text(canvas: &mut dyn Canvas, text: &StyleTextMark, target: &StylePaintTarget) {
    if text.text.is_empty() || text.height.is_nan() || text.height <= 0.0 {
        return;
    }
    let pixels = text.height * target.view.scale;
    if pixels.is_nan() || pixels < MINIMUM_STYLE_TEXT_PIXELS {
        return; // smaller than it is worth painting, and unreadable anyway
    }
    canvas.save();
    let anchor = to_device(&target.view, text.at);
    canvas.translate(anchor.x, anchor.y);
    // Screen y grows downwards, so a counter-clockwise model angle turns the other way on the
    // page.
    canvas.rotate(-text.angle * RAD_TO_DEG);
    // 326 of the 514 texts in the reference libraries are drawn narrowed (0.8 or 0.85). Scaling
    // the painter rather than asking for a stretched font works with every font, stretch
    // variants or not.
    let factor = width_factor_of(text);
    if factor != 1.0 {
        canvas.scale(factor, 1.0);
    }
    // std::lround: half away from zero, as f64::round.
    let pixel_size = katana_math::cpp::min(pixels, MAXIMUM_STYLE_TEXT_PIXELS).round() as i32;
    let font = style_text_font(canvas.font(), text, pixel_size);
    canvas.set_font(font.clone());
    let origin = justified_origin(canvas.metrics(), &font, &text.text, &text.justify);
    canvas.draw_text(origin, &text.text);
    canvas.restore();
}

/// The model-space box a style text is painted over, measured with the font paint_style_text
/// would choose (from `base`), its justification, width factor and angle. cad's drawn_extent
/// cannot know a font and over-estimates on purpose, which is right for culling but shrinks a
/// labelled symbol to a third of its thumbnail; a picture fitted to what is painted uses this.
/// Ignores the pixel clamps, which depend on a view.
pub fn style_text_extent(text: &StyleTextMark, base: &Font, metrics: &dyn FontMetrics) -> Box2 {
    let mut bounds = Box2::default();
    if text.text.is_empty() || text.height.is_nan() || text.height <= 0.0 {
        return bounds;
    }
    // Measured at a reference size and scaled to the text's height: a glyph's proportions do not
    // depend on its size, and this needs no view.
    const REFERENCE_PIXELS: i32 = 100;
    let font = style_text_font(base.clone(), text, REFERENCE_PIXELS);
    let origin = justified_origin(metrics, &font, &text.text, &text.justify);
    // Pixels (y down) to model units (y up) about the anchor, then turned by the text's angle -
    // the same steps paint_style_text takes, in reverse.
    let unit = text.height / f64::from(REFERENCE_PIXELS);
    let factor = width_factor_of(text);
    let cosine = text.angle.cos();
    let sine = text.angle.sin();
    let left = origin.x * factor;
    let right = (origin.x + metrics.horizontal_advance(&font, &text.text)) * factor;
    let top = origin.y - metrics.ascent(&font);
    let bottom = origin.y + metrics.descent(&font);
    for (cx, cy) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
        let x = cx * unit;
        let y = -cy * unit;
        bounds.expand(Point2::new(
            text.at.x + x * cosine - y * sine,
            text.at.y + x * sine + y * cosine,
        ));
    }
    bounds
}

/// Every stroke's points and every text's style_text_extent.
pub fn painted_extent(drawing: &StyleDrawing, base: &Font, metrics: &dyn FontMetrics) -> Box2 {
    let mut bounds = Box2::default();
    for stroke in &drawing.strokes {
        for &vertex in &stroke.path.vertices {
            bounds.expand(vertex);
        }
    }
    for text in &drawing.texts {
        bounds.expand_box(style_text_extent(text, base, metrics));
    }
    bounds
}

/// Strokes, then texts. A one-point stroke is a library `dot`, painted as a round dot of the
/// pen's width whatever the pen's cap: a flat cap draws a zero-length line as nothing. The
/// canvas's pen is left as it was found.
pub fn paint_style_drawing(
    canvas: &mut dyn Canvas,
    drawing: &StyleDrawing,
    target: &StylePaintTarget,
) {
    let previous = canvas.pen();
    // One definition repeats the same `colour` along a whole line, so the last name's pen is kept
    // rather than looking the name up - which folds its case and allocates - for every stroke.
    let mut last_name = String::new();
    let mut last_pen = target.entity_pen.clone();
    let mut pen_for = |name: &str| -> Pen {
        if target.entity_pen_only || name.is_empty() {
            return target.entity_pen.clone();
        }
        if name != last_name {
            last_name = String::from(name);
            last_pen = style_pen_for(&target.entity_pen, name, target.paper, target.colours);
        }
        last_pen.clone()
    };
    for stroke in &drawing.strokes {
        let vertices = &stroke.path.vertices;
        let Some(&first) = vertices.first() else {
            continue;
        };
        if vertices.len() == 1 {
            let mut dot = pen_for(&stroke.pen);
            dot.cap = PenCap::Round;
            canvas.set_pen(dot);
            canvas.draw_point(to_device(&target.view, first));
            continue;
        }
        canvas.set_pen(pen_for(&stroke.pen));
        let mut polygon: Vec<PointF> = Vec::with_capacity(vertices.len() + 1);
        for &vertex in vertices {
            polygon.push(to_device(&target.view, vertex));
        }
        if stroke.path.closed {
            polygon.push(to_device(&target.view, first));
        }
        canvas.draw_polyline(&polygon);
    }
    for text in &drawing.texts {
        canvas.set_pen(pen_for(&text.pen));
        paint_style_text(canvas, text, target);
    }
    canvas.set_pen(previous);
}
