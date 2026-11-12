//! The raster canvas: the painters' Canvas over an Image, drawn by tiny-skia, with text shaped by
//! cosmic-text (canvas::text) and drawn as its glyphs' outlines. QPainter on a QImage's place: the
//! plan view's picture, a sheet's PNG and TIFF, icons, previews and thumbnails.
//!
//! ANTIALIASING (decided here). QPainter's own defaults are kept, because the painters were
//! written against them: shapes are aliased until a painter sets RenderHint::Antialiasing (39 do),
//! text is antialiased unless TextAntialiasing is turned off, and an image drawn larger or smaller
//! than itself is sampled at the nearest pixel unless SmoothPixmapTransform asks for bilinear. An
//! aliased shape covers the pixels whose centres it holds, which is tiny-skia's rule and Qt's for
//! the shapes the painters draw: a 1 x 1 rectangle at whole coordinates is one pixel, which the
//! toolbar's overflow arrow is drawn in.
//!
//! An antialiased shape covers each pixel by the exact area of the pixel inside it
//! (canvas::coverage), as Qt's raster engine covers it, and tiny-skia blends it through that
//! coverage. tiny-skia's own antialiasing is not used for shapes: it samples a pixel on a 4 x 4
//! grid, putting every edge on a quarter of a pixel, so a pen 1.3 px wide inked as 1.25 and a
//! painted twin comparing two pens' ink read 3.33 where Qt's reads 4.0. A stroke is outlined by
//! tiny-skia's stroker and the outline covered exactly, so the toolkits' strokes differ only
//! where Qt's curve offsets are approximate (QStroker offsets a curve by shifting its control
//! points, within a quarter of a pixel). A stroke a pixel wide or thinner in the device stays
//! tiny-skia's hairline with its alpha scaled by its width, and an image drawn under a turning
//! transform keeps tiny-skia's edges.
//!
//! A clip is kept as a coverage mask of the device, aliased for a rectangle (Qt clips a rectangle
//! to whole pixels) and as the antialiasing hint says for a path, an antialiased one by its exact
//! coverage. Saving the state shares the mask until it is changed again.

use std::sync::Arc;

use tiny_skia::{
    FilterQuality, Mask, Paint, PathStroker, Pattern, PixmapMut, PixmapRef, Shader, SpreadMode,
    StrokeDash,
};

use super::coverage;
use super::image::{Image, ImageFormat};
use super::painter::{
    Brush, BrushStyle, Canvas, Color, FillRule, Font, FontMetrics, IntRect, LinearGradient, Path,
    PathElement, Pen, PenCap, PenJoin, PenStyle, PointF, RectF, RenderHint, SizeF, TextFlags,
    Transform,
};
use super::text::{self, SCREEN_DPI, TextMetrics};

/// Qt's default miter limit, 2 (QPen::miterLimit), is measured from the join point in units of
/// the pen's width; tiny-skia's and PDF's is the miter's whole length over the width, which is
/// twice the same distance over the same width.
pub(crate) const MITER_LIMIT: f32 = 4.0;

/// The dash pattern a pen style draws, in units of the pen's width (qpen.cpp: a dash 4, a dot 1, a
/// space 2); None for a solid line or none. An odd custom pattern is made even with a space of
/// one, as QPen::setDashPattern makes it.
pub(crate) fn dash_pattern(style: &PenStyle) -> Option<Vec<f64>> {
    const DASH: f64 = 4.0;
    const DOT: f64 = 1.0;
    const SPACE: f64 = 2.0;
    match style {
        PenStyle::None | PenStyle::Solid => None,
        PenStyle::Dash => Some(vec![DASH, SPACE]),
        PenStyle::Dot => Some(vec![DOT, SPACE]),
        PenStyle::DashDot => Some(vec![DASH, SPACE, DOT, SPACE]),
        PenStyle::DashDotDot => Some(vec![DASH, SPACE, DOT, SPACE, DOT, SPACE]),
        PenStyle::Custom(pattern) => {
            let mut pattern = pattern.clone();
            if !pattern.len().is_multiple_of(2) {
                pattern.push(1.0);
            }
            Some(pattern)
        }
    }
}

/// The length a dash pattern's unit is: the pen's width, and a pixel for a pen thinner than one
/// (a hairline's dashes are pixels).
pub(crate) fn dash_unit(pen: &Pen) -> f64 {
    if pen.width < 1.0 { 1.0 } else { pen.width }
}

/// QPainter::setOpacity's qBound(0, opacity, 1), which takes a NaN as one.
pub(crate) fn bounded_opacity(opacity: f64) -> f64 {
    let low = if opacity < 1.0 { opacity } else { 1.0 };
    if 0.0 < low { low } else { 0.0 }
}

/// An alpha faded by the painter's opacity.
pub(crate) fn faded(alpha: u8, opacity: f64) -> u8 {
    (f64::from(alpha) * opacity).round().clamp(0.0, 255.0) as u8
}

/// A grey image as RGBA, each grey its own red, green and blue, opaque.
pub(crate) fn rgba_of(image: &Image) -> Image {
    if image.format() == ImageFormat::Rgba8Premultiplied {
        return image.clone();
    }
    let data: Vec<u8> = image
        .bytes()
        .iter()
        .flat_map(|&grey| [grey, grey, grey, 255])
        .collect();
    let mut out = Image::from_bytes(
        image.width(),
        image.height(),
        ImageFormat::Rgba8Premultiplied,
        data,
    )
    .unwrap_or_else(|_| Image::new(image.width(), image.height()));
    out.set_device_pixel_ratio(image.device_pixel_ratio());
    out
}

/// An RGBA image as grey: each pixel's qGray, through Image::set_pixel on a grey image.
fn grey_of(image: &Image) -> Image {
    let mut out = Image::new_grey(image.width(), image.height());
    out.set_device_pixel_ratio(image.device_pixel_ratio());
    for y in 0..image.height() {
        for x in 0..image.width() {
            out.set_pixel(x, y, image.pixel(x, y));
        }
    }
    out
}

/// The state QPainter saves and restores as one.
#[derive(Clone)]
struct State {
    pen: Pen,
    brush: Brush,
    font: Font,
    antialiasing: bool,
    text_antialiasing: bool,
    smooth_pixmaps: bool,
    opacity: f64,
    world: Transform,
    clip: Option<Arc<Mask>>,
    clipping: bool,
}

impl Default for State {
    /// A new QPainter's: a black pen, no brush, the UI's font, only text antialiased, opaque, no
    /// transform and no clip.
    fn default() -> Self {
        Self {
            pen: Pen::default(),
            brush: Brush::none(),
            font: Font::default(),
            antialiasing: false,
            text_antialiasing: true,
            smooth_pixmaps: false,
            opacity: 1.0,
            world: Transform::IDENTITY,
            clip: None,
            clipping: false,
        }
    }
}

/// The painters' Canvas over an RGBA image (see the module comment).
pub struct RasterCanvas {
    image: Image,
    state: State,
    saved: Vec<State>,
    metrics: TextMetrics,
}

impl RasterCanvas {
    /// A transparent image of `width` by `height` pixels to paint on, at a ratio of one.
    pub fn new(width: u32, height: u32) -> Self {
        Self::on(Image::new(width, height))
    }

    /// `image` to paint on, as QPainter(&image): what it holds is painted over. A grey image is
    /// painted as RGBA (QPainter paints a Format_Grayscale8 image through RGB too); a painter that
    /// wants grey converts the result.
    pub fn on(image: Image) -> Self {
        Self {
            image: rgba_of(&image),
            state: State::default(),
            saved: Vec::new(),
            metrics: TextMetrics::new(SCREEN_DPI),
        }
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    /// The painted image (QPainter::end, then the image).
    pub fn into_image(self) -> Image {
        self.image
    }

    /// `paint` run on a canvas over `image`, the image painted in place (QPainter(&image), the
    /// painting, QPainter::end): the one call a painter that is handed an image needs. A grey
    /// image is painted as RGBA and comes back grey, each pixel Qt's grey of what was painted
    /// (qGray), as a Format_Grayscale8 image painted by QPainter does.
    pub fn paint_image(image: &mut Image, paint: &mut dyn FnMut(&mut dyn Canvas)) {
        let grey = image.format() == ImageFormat::Grey8;
        let mut canvas = Self::on(std::mem::take(image));
        paint(&mut canvas);
        let painted = canvas.into_image();
        *image = if grey { grey_of(&painted) } else { painted };
    }

    /// The device's logical resolution, which a font in points is sized at: 96 dots an inch, as a
    /// QImage's unless its dots per metre were set (a plotted sheet's at its plot resolution).
    pub fn set_logical_dpi(&mut self, dpi: f64) {
        self.metrics = TextMetrics::new(dpi);
    }

    pub fn logical_dpi(&self) -> f64 {
        self.metrics.dpi()
    }

    fn clip_to(&mut self, path: &Path, intersect: bool, anti_alias: bool) {
        let transform = sk_transform(&self.device_transform());
        let shape = sk_path(path, self.metrics.dpi());
        let rule = sk_fill_rule(path.fill_rule);
        let (width, height) = (self.image.width(), self.image.height());
        // With no clip in force, an intersection is the new clip itself (Qt's IntersectClip).
        let intersecting = intersect && self.state.clipping && self.state.clip.is_some();
        if intersecting {
            if let Some(clip) = self.state.clip.as_mut() {
                let mask = Arc::make_mut(clip);
                match &shape {
                    Some(shape) if anti_alias => {
                        if let Some(within) = exact_mask(shape, rule, transform, width, height) {
                            for (kept, &inside) in mask.data_mut().iter_mut().zip(within.data()) {
                                *kept = scaled(*kept, inside);
                            }
                        }
                    }
                    Some(shape) => mask.intersect_path(shape, rule, false, transform),
                    // Nothing to clip to: nothing is painted.
                    None => mask.clear(),
                }
            }
        } else {
            let mask = match &shape {
                Some(shape) if anti_alias => exact_mask(shape, rule, transform, width, height),
                Some(shape) => Mask::new(width, height).map(|mut mask| {
                    mask.fill_path(shape, rule, false, transform);
                    mask
                }),
                None => Mask::new(width, height),
            };
            let Some(mask) = mask else {
                return;
            };
            self.state.clip = Some(Arc::new(mask));
        }
        self.state.clipping = true;
    }

    fn fill_with(&mut self, path: &Path, brush: &Brush, anti_alias: bool) {
        let Some(shape) = sk_path(path, self.metrics.dpi()) else {
            return;
        };
        let Some(paint) = brush_paint(brush, self.state.opacity, anti_alias) else {
            return;
        };
        let transform = sk_transform(&self.device_transform());
        let rule = sk_fill_rule(path.fill_rule);
        if anti_alias {
            if let Some(device) = shape.transform(transform) {
                self.fill_exactly(&device, rule, paint, transform);
            }
            return;
        }
        let mask = clip_mask(&self.state);
        if let Some(mut pixmap) = pixmap_of(&mut self.image) {
            pixmap.fill_path(&shape, &paint, rule, transform, mask);
        }
    }

    /// Fills `shape`, already in device pixels, by `rule` with `paint`, each pixel at the exact
    /// area of it the shape covers (canvas::coverage) within the clip. `shader` carries the
    /// paint's shader from the painter's coordinates to the device's, as tiny-skia's fill_path
    /// carries it. The window the shape covers is painted a band of rows at a time, each band on
    /// its own and copied back: tiny-skia blends through a mask only of a pixmap's whole size.
    fn fill_exactly(
        &mut self,
        shape: &tiny_skia::Path,
        rule: tiny_skia::FillRule,
        mut paint: Paint<'static>,
        shader: tiny_skia::Transform,
    ) {
        let device_width = self.image.width() as usize;
        let Some(edges) = coverage::edges(shape, self.image.width(), self.image.height()) else {
            return;
        };
        paint.anti_alias = false;
        paint.shader.transform(shader);
        let (x, width) = (edges.x as usize, edges.width as usize);
        let row_bytes = width * 4;
        let device_row = device_width * 4;
        let mut first = 0;
        while first < edges.height {
            let rows = coverage::BAND_ROWS.min(edges.height - first);
            let top = (edges.y + first) as usize;
            let mut alpha = edges.band(rule, first, rows);
            first += rows;
            if let Some(clip) = clip_mask(&self.state) {
                let clip = clip.data();
                for row in 0..rows as usize {
                    let from = (top + row) * device_width + x;
                    let alphas = &mut alpha[row * width..(row + 1) * width];
                    for (covered, &kept) in alphas.iter_mut().zip(&clip[from..from + width]) {
                        *covered = scaled(*covered, kept);
                    }
                }
            }
            if alpha.iter().all(|&covered| covered == 0) {
                continue;
            }
            let Some(size) = tiny_skia::IntSize::from_wh(edges.width, rows) else {
                return;
            };
            let Some(mask) = Mask::from_vec(alpha, size) else {
                return;
            };
            let Some(mut part) = tiny_skia::Pixmap::new(edges.width, rows) else {
                return;
            };
            let Some(area) = tiny_skia::Rect::from_xywh(0.0, 0.0, width as f32, rows as f32) else {
                return;
            };
            let bytes = self.image.bytes_mut();
            for row in 0..rows as usize {
                let from = (top + row) * device_row + x * 4;
                part.data_mut()[row * row_bytes..(row + 1) * row_bytes]
                    .copy_from_slice(&bytes[from..from + row_bytes]);
            }
            let mut band_paint = paint.clone();
            band_paint
                .shader
                .transform(tiny_skia::Transform::from_translate(
                    -(x as f32),
                    -(top as f32),
                ));
            part.fill_rect(
                area,
                &band_paint,
                tiny_skia::Transform::identity(),
                Some(&mask),
            );
            for row in 0..rows as usize {
                let to = (top + row) * device_row + x * 4;
                bytes[to..to + row_bytes]
                    .copy_from_slice(&part.data()[row * row_bytes..(row + 1) * row_bytes]);
            }
        }
    }

    /// Glyph outlines filled in the pen's colour, antialiased as TextAntialiasing says.
    fn fill_glyphs(&mut self, glyphs: &Path) {
        let brush = Brush::solid(self.state.pen.color);
        let anti_alias = self.state.text_antialiasing;
        self.fill_with(glyphs, &brush, anti_alias);
    }
}

/// A coverage scaled by another, both 0 to 255, rounded: a shape's coverage within a clip's.
fn scaled(coverage: u8, by: u8) -> u8 {
    ((u32::from(coverage) * u32::from(by) + 127) / 255) as u8
}

/// The exact coverage (canvas::coverage) of `shape`, carried to the device by `transform`, as a
/// mask of the device `width` by `height`: an antialiased clip path's.
fn exact_mask(
    shape: &tiny_skia::Path,
    rule: tiny_skia::FillRule,
    transform: tiny_skia::Transform,
    width: u32,
    height: u32,
) -> Option<Mask> {
    let mut mask = Mask::new(width, height)?;
    let edges = shape
        .clone()
        .transform(transform)
        .and_then(|device| coverage::edges(&device, width, height));
    if let Some(edges) = edges {
        let (x, window) = (edges.x as usize, edges.width as usize);
        let data = mask.data_mut();
        let mut first = 0;
        while first < edges.height {
            let rows = coverage::BAND_ROWS.min(edges.height - first);
            let top = (edges.y + first) as usize;
            let alpha = edges.band(rule, first, rows);
            for row in 0..rows as usize {
                let from = (top + row) * width as usize + x;
                data[from..from + window].copy_from_slice(&alpha[row * window..(row + 1) * window]);
            }
            first += rows;
        }
    }
    Some(mask)
}

/// Whether tiny-skia strokes a pen `width` wide under `transform` as a hairline, its alpha scaled
/// by its width: both of the width's axes a pixel or less in the device by tiny-skia's fast length,
/// the longer component and half the shorter (painter.rs, treat_as_hairline).
fn hairline_in(width: f32, transform: tiny_skia::Transform) -> bool {
    let fast = |x: f32, y: f32| {
        let (x, y) = (x.abs(), y.abs());
        if x < y { y + x / 2.0 } else { x + y / 2.0 }
    };
    let across = fast(transform.sx * width, transform.ky * width);
    let down = fast(transform.kx * width, transform.sy * width);
    across <= 1.0 && down <= 1.0
}

/// The clip as a mask, or None when everything is painted.
fn clip_mask(state: &State) -> Option<&Mask> {
    if state.clipping {
        state.clip.as_deref()
    } else {
        None
    }
}

fn pixmap_of(image: &mut Image) -> Option<PixmapMut<'_>> {
    let (width, height) = (image.width(), image.height());
    PixmapMut::from_bytes(image.bytes_mut(), width, height)
}

/// Qt's affine transform as tiny-skia's: x' = m11 x + m21 y + dx, y' = m12 x + m22 y + dy.
pub(crate) fn sk_transform(transform: &Transform) -> tiny_skia::Transform {
    tiny_skia::Transform::from_row(
        transform.m11 as f32,
        transform.m12 as f32,
        transform.m21 as f32,
        transform.m22 as f32,
        transform.dx as f32,
        transform.dy as f32,
    )
}

fn sk_fill_rule(rule: FillRule) -> tiny_skia::FillRule {
    match rule {
        FillRule::OddEven => tiny_skia::FillRule::EvenOdd,
        FillRule::Winding => tiny_skia::FillRule::Winding,
    }
}

fn sk_color(color: Color, opacity: f64) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(color.r, color.g, color.b, faded(color.a, opacity))
}

fn add_elements(builder: &mut tiny_skia::PathBuilder, elements: &[PathElement], dpi: f64) {
    for element in elements {
        match element {
            PathElement::MoveTo(point) => builder.move_to(point.x as f32, point.y as f32),
            PathElement::LineTo(point) => builder.line_to(point.x as f32, point.y as f32),
            PathElement::CubicTo(c1, c2, end) => builder.cubic_to(
                c1.x as f32,
                c1.y as f32,
                c2.x as f32,
                c2.y as f32,
                end.x as f32,
                end.y as f32,
            ),
            PathElement::Close => builder.close(),
            PathElement::Text { origin, font, text } => {
                let glyphs = text::text_path(font, dpi, *origin, text);
                add_elements(builder, &glyphs.elements, dpi);
            }
        }
    }
}

/// A canvas path as tiny-skia's, a text in it turned into its outlines at `dpi`; None for an
/// empty path.
fn sk_path(path: &Path, dpi: f64) -> Option<tiny_skia::Path> {
    let mut builder = tiny_skia::PathBuilder::new();
    add_elements(&mut builder, &path.elements, dpi);
    builder.finish()
}

/// QGradient's stops, or its black-to-white when it was given none (QGradient::stops).
pub(crate) fn gradient_stops(gradient: &LinearGradient) -> Vec<(f64, Color)> {
    if gradient.stops.is_empty() {
        return vec![(0.0, Color::BLACK), (1.0, Color::WHITE)];
    }
    gradient
        .stops
        .iter()
        .map(|stop| (stop.position, stop.color))
        .collect()
}

fn linear_shader(gradient: &LinearGradient, opacity: f64) -> Option<Shader<'static>> {
    let stops = gradient_stops(gradient);
    let last = stops.last().map(|&(_, color)| color)?;
    let sk_stops = stops
        .iter()
        .map(|&(position, color)| {
            tiny_skia::GradientStop::new(position as f32, sk_color(color, opacity))
        })
        .collect();
    let shader = tiny_skia::LinearGradient::new(
        tiny_skia::Point::from_xy(gradient.start.x as f32, gradient.start.y as f32),
        tiny_skia::Point::from_xy(gradient.end.x as f32, gradient.end.y as f32),
        sk_stops,
        SpreadMode::Pad,
        tiny_skia::Transform::identity(),
    );
    // A gradient whose ends meet is its last colour, as a padded gradient is beyond its end.
    Some(shader.unwrap_or_else(|| Shader::SolidColor(sk_color(last, opacity))))
}

fn brush_paint(brush: &Brush, opacity: f64, anti_alias: bool) -> Option<Paint<'static>> {
    let shader = match &brush.style {
        BrushStyle::None => return None,
        BrushStyle::Solid(color) => Shader::SolidColor(sk_color(*color, opacity)),
        BrushStyle::Linear(gradient) => linear_shader(gradient, opacity)?,
    };
    Some(Paint {
        shader,
        anti_alias,
        ..Paint::default()
    })
}

fn sk_stroke(pen: &Pen, width: f32) -> tiny_skia::Stroke {
    let unit = dash_unit(pen);
    let dash = dash_pattern(&pen.style).and_then(|pattern| {
        StrokeDash::new(
            pattern
                .iter()
                .map(|length| (length * unit) as f32)
                .collect(),
            (pen.dash_offset * unit) as f32,
        )
    });
    tiny_skia::Stroke {
        width,
        miter_limit: MITER_LIMIT,
        line_cap: match pen.cap {
            PenCap::Flat => tiny_skia::LineCap::Butt,
            PenCap::Square => tiny_skia::LineCap::Square,
            PenCap::Round => tiny_skia::LineCap::Round,
        },
        // Qt's MiterJoin cuts a miter off at its limit; its bevelling join is BevelJoin.
        line_join: match pen.join {
            PenJoin::Miter => tiny_skia::LineJoin::MiterClip,
            PenJoin::Bevel => tiny_skia::LineJoin::Bevel,
            PenJoin::Round => tiny_skia::LineJoin::Round,
        },
        dash,
    }
}

impl Canvas for RasterCanvas {
    fn device_size(&self) -> SizeF {
        SizeF::new(
            f64::from(self.image.width()),
            f64::from(self.image.height()),
        )
    }

    fn device_pixel_ratio(&self) -> f64 {
        self.image.device_pixel_ratio()
    }

    fn save(&mut self) {
        self.saved.push(self.state.clone());
    }

    fn restore(&mut self) {
        if let Some(state) = self.saved.pop() {
            self.state = state;
        }
    }

    fn pen(&self) -> Pen {
        self.state.pen.clone()
    }

    fn set_pen(&mut self, pen: Pen) {
        self.state.pen = pen;
    }

    fn brush(&self) -> Brush {
        self.state.brush.clone()
    }

    fn set_brush(&mut self, brush: Brush) {
        self.state.brush = brush;
    }

    fn font(&self) -> Font {
        self.state.font.clone()
    }

    fn set_font(&mut self, font: Font) {
        self.state.font = font;
    }

    fn render_hint(&self, hint: RenderHint) -> bool {
        match hint {
            RenderHint::Antialiasing => self.state.antialiasing,
            RenderHint::TextAntialiasing => self.state.text_antialiasing,
            RenderHint::SmoothPixmapTransform => self.state.smooth_pixmaps,
        }
    }

    fn set_render_hint(&mut self, hint: RenderHint, on: bool) {
        match hint {
            RenderHint::Antialiasing => self.state.antialiasing = on,
            RenderHint::TextAntialiasing => self.state.text_antialiasing = on,
            RenderHint::SmoothPixmapTransform => self.state.smooth_pixmaps = on,
        }
    }

    fn opacity(&self) -> f64 {
        self.state.opacity
    }

    fn set_opacity(&mut self, opacity: f64) {
        self.state.opacity = bounded_opacity(opacity);
    }

    fn world_transform(&self) -> Transform {
        self.state.world
    }

    fn set_world_transform(&mut self, transform: Transform, combine: bool) {
        self.state.world = if combine {
            transform * self.state.world
        } else {
            transform
        };
    }

    fn device_transform(&self) -> Transform {
        let ratio = self.image.device_pixel_ratio();
        self.state.world * Transform::from_scale(ratio, ratio)
    }

    fn set_clip_rect(&mut self, rect: RectF, intersect: bool) {
        let mut path = Path::new();
        path.add_rect(rect);
        self.clip_to(&path, intersect, false);
    }

    fn set_clip_path(&mut self, path: &Path, intersect: bool) {
        let anti_alias = self.state.antialiasing;
        self.clip_to(path, intersect, anti_alias);
    }

    fn set_clipping(&mut self, on: bool) {
        self.state.clipping = on;
    }

    fn has_clipping(&self) -> bool {
        self.state.clipping
    }

    fn draw_path(&mut self, path: &Path) {
        let brush = self.state.brush.clone();
        self.fill_path(path, &brush);
        let pen = self.state.pen.clone();
        self.stroke_path(path, &pen);
    }

    fn fill_path(&mut self, path: &Path, brush: &Brush) {
        let anti_alias = self.state.antialiasing;
        self.fill_with(path, brush, anti_alias);
    }

    /// A pen of no width, or a cosmetic one, is stroked in the device's pixels whatever the
    /// transform (the path is carried there first); any other in the painter's coordinates, its
    /// width and dashes scaled with them.
    fn stroke_path(&mut self, path: &Path, pen: &Pen) {
        if pen.is_none() {
            return;
        }
        let Some(shape) = sk_path(path, self.metrics.dpi()) else {
            return;
        };
        let device = sk_transform(&self.device_transform());
        let cosmetic = pen.cosmetic || pen.width <= 0.0;
        let (shape, transform) = if cosmetic {
            match shape.transform(device) {
                Some(shape) => (shape, tiny_skia::Transform::identity()),
                None => return,
            }
        } else {
            (shape, device)
        };
        // A width of nothing is tiny-skia's hairline: a pixel wide.
        let width = if pen.width > 0.0 {
            pen.width as f32
        } else {
            0.0
        };
        let stroke = sk_stroke(pen, width);
        let paint = Paint {
            shader: Shader::SolidColor(sk_color(pen.color, self.state.opacity)),
            anti_alias: self.state.antialiasing,
            ..Paint::default()
        };
        // Antialiased and wider than a pixel: dashed and outlined as tiny-skia's stroke_path does
        // it, the outline covered exactly (see the module comment).
        if self.state.antialiasing && width > 0.0 && !hairline_in(width, transform) {
            let scale = PathStroker::compute_resolution_scale(&transform);
            let dashed = match &stroke.dash {
                Some(dash) => shape.dash(dash, scale),
                None => Some(shape),
            };
            let outline = dashed
                .and_then(|dashed| dashed.stroke(&stroke, scale))
                .and_then(|outline| outline.transform(transform));
            if let Some(outline) = outline {
                self.fill_exactly(&outline, tiny_skia::FillRule::Winding, paint, transform);
            }
            return;
        }
        let mask = clip_mask(&self.state);
        if let Some(mut pixmap) = pixmap_of(&mut self.image) {
            pixmap.stroke_path(&shape, &paint, &stroke, transform, mask);
        }
    }

    fn draw_text(&mut self, origin: PointF, text: &str) {
        if self.state.pen.is_none() {
            return;
        }
        let glyphs = text::text_path(&self.state.font, self.metrics.dpi(), origin, text);
        self.fill_glyphs(&glyphs);
    }

    /// Clipped to `rect` when the text runs out of it, as drawText clips without
    /// Qt::TextDontClip.
    fn draw_text_in(&mut self, rect: RectF, flags: TextFlags, text: &str) -> RectF {
        let Some(laid) = text::lay_out(&self.state.font, self.metrics.dpi(), rect, flags, text)
        else {
            return RectF::new(rect.x, rect.y, 0.0, 0.0);
        };
        if self.state.pen.is_none() {
            return laid.bounds;
        }
        let clipped = !rect.contains_rect(&laid.bounds);
        if clipped {
            self.save();
            self.set_clip_rect(rect, true);
        }
        for (origin, shaped) in &laid.lines {
            let glyphs = text::outline_of(shaped, *origin);
            self.fill_glyphs(&glyphs);
        }
        if clipped {
            self.restore();
        }
        laid.bounds
    }

    fn draw_image(&mut self, target: RectF, image: &Image, source: IntRect) {
        if source.is_empty() || image.is_null() {
            return;
        }
        let part = rgba_of(&image.copy(source));
        let (width, height) = (part.width(), part.height());
        let Some(pixels) = PixmapRef::from_bytes(part.bytes(), width, height) else {
            return;
        };
        let Some(area) = tiny_skia::Rect::from_xywh(
            target.x as f32,
            target.y as f32,
            target.width as f32,
            target.height as f32,
        ) else {
            return;
        };
        let quality = if self.state.smooth_pixmaps {
            FilterQuality::Bilinear
        } else {
            FilterQuality::Nearest
        };
        // The source's pixels laid over the target: scaled to its size, moved to its corner.
        let placed = tiny_skia::Transform::from_row(
            (target.width / f64::from(width)) as f32,
            0.0,
            0.0,
            (target.height / f64::from(height)) as f32,
            target.x as f32,
            target.y as f32,
        );
        let paint = Paint {
            shader: Pattern::new(
                pixels,
                SpreadMode::Pad,
                quality,
                self.state.opacity as f32,
                placed,
            ),
            anti_alias: self.state.antialiasing,
            ..Paint::default()
        };
        let transform = sk_transform(&self.device_transform());
        let mask = clip_mask(&self.state);
        if let Some(mut pixmap) = pixmap_of(&mut self.image) {
            pixmap.fill_rect(area, &paint, transform, mask);
        }
    }

    fn metrics(&self) -> &dyn FontMetrics {
        &self.metrics
    }
}
