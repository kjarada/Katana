//! The PDF canvas: the painters' Canvas over a page of a PDF, written by krilla. QPdfWriter's
//! place: a plotted drawing, a sheet set in one PDF or a PDF a sheet, and what printing hands the
//! operating system (the owner's decision of 2026-11-05).
//!
//! A page's device is QPdfWriter's: pixels at the page's resolution (`dpi` of them an inch), its
//! origin at the top left, y down; what is drawn there is scaled to the page's points (72 an inch)
//! as it is written. A canvas records what it is asked to draw, each mark with the clip and
//! transform it was drawn under, and a PdfDocument writes the recorded page into the file: krilla
//! writes a page through a surface that borrows the document, which a canvas a painter holds for a
//! whole drawing cannot.
//!
//! THE PDF'S TEXT (decided here). Text is written as text: each run of glyphs in the font file it
//! was shaped from, embedded as a subset with its ToUnicode map, so a PDF reader finds, selects
//! and copies a sheet's titles and labels. The glyphs and their places are canvas::text's - the
//! one shaping the raster canvas and the painters' metrics use - so a label sits in the PDF where
//! it sits in the PNG of the same sheet. REJECTED: krilla's own shaping of a string (its
//! `draw_text`), which shapes in one font with no fallback and with advances the painters'
//! metrics did not measure; and outlines, which no reader can search or copy.

use std::path::Path as FilePath;

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Size};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, LineCap, LineJoin, SpreadMethod, Stop, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::{Document, SerializeSettings};

use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::{ErrorCode, Result, Status};

use super::image::Image;
use super::painter::{
    Brush, BrushStyle, Canvas, Color, FillRule, Font, FontMetrics, IntRect, LinearGradient, Path,
    PathElement, Pen, PenCap, PenJoin, PointF, RectF, RenderHint, SizeF, TextFlags, Transform,
};
use super::raster::{MITER_LIMIT, bounded_opacity, dash_pattern, dash_unit, faded, gradient_stops};
use super::text::{self, Shaped, TextMetrics};

/// One step of a path, in the coordinates it is written in.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Segment {
    Move(f32, f32),
    Line(f32, f32),
    Cubic([f32; 6]),
    Close,
}

fn add_segments(out: &mut Vec<Segment>, elements: &[PathElement], dpi: f64, map: &Transform) {
    let at = |point: &PointF| {
        let mapped = map.map(*point);
        (mapped.x as f32, mapped.y as f32)
    };
    for element in elements {
        match element {
            PathElement::MoveTo(point) => {
                let (x, y) = at(point);
                out.push(Segment::Move(x, y));
            }
            PathElement::LineTo(point) => {
                let (x, y) = at(point);
                out.push(Segment::Line(x, y));
            }
            PathElement::CubicTo(c1, c2, end) => {
                let (x1, y1) = at(c1);
                let (x2, y2) = at(c2);
                let (x, y) = at(end);
                out.push(Segment::Cubic([x1, y1, x2, y2, x, y]));
            }
            PathElement::Close => out.push(Segment::Close),
            PathElement::Text { origin, font, text } => {
                let glyphs = text::text_path(font, dpi, *origin, text);
                add_segments(out, &glyphs.elements, dpi, map);
            }
        }
    }
}

/// A canvas path as segments, each point carried by `map`, a text in it as its outlines.
fn segments(path: &Path, dpi: f64, map: &Transform) -> Vec<Segment> {
    let mut out = Vec::new();
    add_segments(&mut out, &path.elements, dpi, map);
    out
}

fn krilla_path(segments: &[Segment]) -> Option<krilla::geom::Path> {
    let mut builder = PathBuilder::new();
    for segment in segments {
        match *segment {
            Segment::Move(x, y) => builder.move_to(x, y),
            Segment::Line(x, y) => builder.line_to(x, y),
            Segment::Cubic([x1, y1, x2, y2, x, y]) => builder.cubic_to(x1, y1, x2, y2, x, y),
            Segment::Close => builder.close(),
        }
    }
    builder.finish()
}

fn krilla_rule(rule: FillRule) -> krilla::paint::FillRule {
    match rule {
        FillRule::OddEven => krilla::paint::FillRule::EvenOdd,
        FillRule::Winding => krilla::paint::FillRule::NonZero,
    }
}

fn krilla_transform(transform: &Transform) -> krilla::geom::Transform {
    krilla::geom::Transform::from_row(
        transform.m11 as f32,
        transform.m12 as f32,
        transform.m21 as f32,
        transform.m22 as f32,
        transform.dx as f32,
        transform.dy as f32,
    )
}

/// 0 to 1 as krilla's normalised number; what is not a number is one.
fn normalized(value: f64) -> NormalizedF32 {
    NormalizedF32::new(value.clamp(0.0, 1.0) as f32).unwrap_or(NormalizedF32::ONE)
}

fn rgb_of(color: Color) -> rgb::Color {
    rgb::Color::new(color.r, color.g, color.b)
}

/// A colour's alpha with the painter's opacity, as a PDF opacity.
fn opacity_of(color: Color, opacity: f64) -> NormalizedF32 {
    normalized(f64::from(faded(color.a, opacity)) / 255.0)
}

/// One glyph as krilla writes it: its advance and offsets per em of the run's size, as krilla's
/// Glyph trait measures them (it asks for them at a size of one).
#[derive(Clone, Debug, PartialEq)]
struct PdfGlyph {
    id: u16,
    range: std::ops::Range<usize>,
    advance: f32,
    x_offset: f32,
    y_offset: f32,
}

impl krilla::text::Glyph for PdfGlyph {
    fn glyph_id(&self) -> krilla::text::GlyphId {
        krilla::text::GlyphId::new(u32::from(self.id))
    }

    fn text_range(&self) -> std::ops::Range<usize> {
        self.range.clone()
    }

    fn x_advance(&self, size: f32) -> f32 {
        self.advance * size
    }

    fn x_offset(&self, size: f32) -> f32 {
        self.x_offset * size
    }

    fn y_offset(&self, size: f32) -> f32 {
        self.y_offset * size
    }

    fn y_advance(&self, _size: f32) -> f32 {
        0.0
    }

    fn location(&self) -> Option<krilla::surface::Location> {
        None
    }
}

/// Glyphs of one font at one size, from `origin` on the baseline.
struct Run {
    font: krilla::text::Font,
    size: f32,
    origin: (f32, f32),
    text: String,
    glyphs: Vec<PdfGlyph>,
}

enum Mark {
    Fill {
        path: Vec<Segment>,
        fill: Fill,
    },
    Stroke {
        path: Vec<Segment>,
        stroke: Stroke,
    },
    Text {
        runs: Vec<Run>,
        fill: Fill,
    },
    Image {
        image: krilla::image::Image,
        width: f32,
        height: f32,
        opacity: f64,
    },
}

/// A mark with the clip (in the device's pixels) and transform (to them) it was drawn under.
struct Op {
    clips: Vec<(Vec<Segment>, FillRule)>,
    transform: Transform,
    mark: Mark,
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
    clips: Vec<(Vec<Segment>, FillRule)>,
    clipping: bool,
}

impl Default for State {
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
            clips: Vec::new(),
            clipping: false,
        }
    }
}

/// One page of a PDF being drawn (see the module comment); a PdfDocument writes it.
pub struct PdfCanvas {
    width_points: f64,
    height_points: f64,
    resolution: f64,
    state: State,
    saved: Vec<State>,
    ops: Vec<Op>,
    fonts: Vec<(cosmic_text::fontdb::ID, Option<krilla::text::Font>)>,
    metrics: TextMetrics,
}

impl PdfCanvas {
    /// A page `width_points` by `height_points` (72 an inch) drawn in pixels of `resolution` dots
    /// an inch (QPdfWriter::setResolution; 1200 is QPdfWriter's own).
    pub fn new(width_points: f64, height_points: f64, resolution: f64) -> Self {
        Self {
            width_points,
            height_points,
            resolution,
            state: State::default(),
            saved: Vec::new(),
            ops: Vec::new(),
            fonts: Vec::new(),
            metrics: TextMetrics::new(resolution),
        }
    }

    /// The page's size in points.
    pub fn page_size(&self) -> SizeF {
        SizeF::new(self.width_points, self.height_points)
    }

    pub fn resolution(&self) -> f64 {
        self.resolution
    }

    fn record(&mut self, transform: Transform, mark: Mark) {
        let clips = if self.state.clipping {
            self.state.clips.clone()
        } else {
            Vec::new()
        };
        self.ops.push(Op {
            clips,
            transform,
            mark,
        });
    }

    fn clip_to(&mut self, path: &Path, intersect: bool) {
        let device = self.device_transform();
        let clip = (segments(path, self.metrics.dpi(), &device), path.fill_rule);
        if intersect && self.state.clipping && !self.state.clips.is_empty() {
            self.state.clips.push(clip);
        } else {
            self.state.clips = vec![clip];
        }
        self.state.clipping = true;
    }

    /// The face of a font file this thread resolved, as krilla embeds it; None when its file
    /// cannot be read or parsed.
    fn pdf_font(&mut self, face: cosmic_text::fontdb::ID) -> Option<krilla::text::Font> {
        if let Some((_, font)) = self.fonts.iter().find(|(id, _)| *id == face) {
            return font.clone();
        }
        let font = text::face_data(face)
            .and_then(|(bytes, index)| krilla::text::Font::new(bytes.into(), index));
        self.fonts.push((face, font.clone()));
        font
    }

    /// A shaped line's glyphs, its baseline from `origin`, in the pen's colour: one run for each
    /// stretch of glyphs in one face at one size.
    fn record_text(&mut self, origin: PointF, shaped: &Shaped) {
        let pen = self.state.pen.clone();
        if pen.is_none() || shaped.glyphs.is_empty() {
            return;
        }
        let mut runs: Vec<Run> = Vec::new();
        let mut current: Option<(cosmic_text::fontdb::ID, f32)> = None;
        for (at, glyph) in shaped.glyphs.iter().enumerate() {
            let size = f64::from(glyph.size);
            // The pen's step to the next glyph, so a run keeps the shaped places exactly.
            let advance = shaped
                .glyphs
                .get(at + 1)
                .map_or(glyph.advance, |next| next.pen - glyph.pen);
            let placed = PdfGlyph {
                id: glyph.id,
                range: glyph.range.clone(),
                advance: (advance / size) as f32,
                x_offset: (glyph.x_offset / size) as f32,
                y_offset: (glyph.rise / size) as f32,
            };
            let key = (glyph.font, glyph.size);
            if current == Some(key)
                && let Some(run) = runs.last_mut()
            {
                run.glyphs.push(placed);
                continue;
            }
            current = Some(key);
            let Some(font) = self.pdf_font(glyph.font) else {
                // A face that cannot be embedded writes nothing; its glyphs are not text.
                current = None;
                continue;
            };
            runs.push(Run {
                font,
                size: glyph.size,
                origin: ((origin.x + glyph.pen) as f32, origin.y as f32),
                text: shaped.text.clone(),
                glyphs: vec![placed],
            });
        }
        if runs.is_empty() {
            return;
        }
        let fill = Fill {
            paint: rgb_of(pen.color).into(),
            opacity: opacity_of(pen.color, self.state.opacity),
            rule: krilla::paint::FillRule::NonZero,
        };
        let transform = self.device_transform();
        self.record(transform, Mark::Text { runs, fill });
    }

    fn brush_fill(&self, brush: &Brush, rule: FillRule) -> Option<Fill> {
        let rule = krilla_rule(rule);
        match &brush.style {
            BrushStyle::None => None,
            BrushStyle::Solid(color) => Some(Fill {
                paint: rgb_of(*color).into(),
                opacity: opacity_of(*color, self.state.opacity),
                rule,
            }),
            BrushStyle::Linear(gradient) => Some(self.gradient_fill(gradient, rule)),
        }
    }

    fn gradient_fill(&self, gradient: &LinearGradient, rule: krilla::paint::FillRule) -> Fill {
        let stops = gradient_stops(gradient);
        let opacity = self.state.opacity;
        // A gradient whose ends meet is its last colour, as a padded gradient is beyond its end.
        if gradient.start == gradient.end
            && let Some(&(_, last)) = stops.last()
        {
            return Fill {
                paint: rgb_of(last).into(),
                opacity: opacity_of(last, opacity),
                rule,
            };
        }
        let linear = krilla::paint::LinearGradient {
            x1: gradient.start.x as f32,
            y1: gradient.start.y as f32,
            x2: gradient.end.x as f32,
            y2: gradient.end.y as f32,
            transform: krilla::geom::Transform::identity(),
            spread_method: SpreadMethod::Pad,
            stops: stops
                .iter()
                .map(|&(position, color)| Stop {
                    offset: normalized(position),
                    color: rgb_of(color).into(),
                    opacity: opacity_of(color, opacity),
                })
                .collect(),
            anti_alias: false,
        };
        Fill {
            paint: linear.into(),
            opacity: NormalizedF32::ONE,
            rule,
        }
    }
}

impl Canvas for PdfCanvas {
    fn device_size(&self) -> SizeF {
        SizeF::new(
            self.width_points * self.resolution / 72.0,
            self.height_points * self.resolution / 72.0,
        )
    }

    fn device_pixel_ratio(&self) -> f64 {
        1.0
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

    /// Kept for a painter that reads it back; a PDF reader decides its own smoothing.
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

    /// The world transform: a PDF page's device has no scale of its own (QPdfWriter's ratio is
    /// one).
    fn device_transform(&self) -> Transform {
        self.state.world
    }

    fn set_clip_rect(&mut self, rect: RectF, intersect: bool) {
        let mut path = Path::new();
        path.add_rect(rect);
        self.clip_to(&path, intersect);
    }

    fn set_clip_path(&mut self, path: &Path, intersect: bool) {
        self.clip_to(path, intersect);
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
        let Some(fill) = self.brush_fill(brush, path.fill_rule) else {
            return;
        };
        let shape = segments(path, self.metrics.dpi(), &Transform::IDENTITY);
        if shape.is_empty() {
            return;
        }
        let transform = self.device_transform();
        self.record(transform, Mark::Fill { path: shape, fill });
    }

    /// As the raster canvas strokes: a cosmetic pen (or one of no width) in the device's pixels,
    /// written with no transform; any other in the painter's coordinates under its transform.
    fn stroke_path(&mut self, path: &Path, pen: &Pen) {
        if pen.is_none() {
            return;
        }
        let device = self.device_transform();
        let dpi = self.metrics.dpi();
        let cosmetic = pen.cosmetic || pen.width <= 0.0;
        let (shape, transform) = if cosmetic {
            (segments(path, dpi, &device), Transform::IDENTITY)
        } else {
            (segments(path, dpi, &Transform::IDENTITY), device)
        };
        if shape.is_empty() {
            return;
        }
        let unit = dash_unit(pen);
        let dash = dash_pattern(&pen.style).map(|pattern| StrokeDash {
            array: pattern
                .iter()
                .map(|length| (length * unit) as f32)
                .collect(),
            offset: (pen.dash_offset * unit) as f32,
        });
        let stroke = Stroke {
            paint: rgb_of(pen.color).into(),
            // A width of nothing is PDF's thinnest line the device can draw.
            width: if pen.width > 0.0 {
                pen.width as f32
            } else {
                0.0
            },
            miter_limit: MITER_LIMIT,
            line_cap: match pen.cap {
                PenCap::Flat => LineCap::Butt,
                PenCap::Square => LineCap::Square,
                PenCap::Round => LineCap::Round,
            },
            line_join: match pen.join {
                PenJoin::Miter => LineJoin::Miter,
                PenJoin::Bevel => LineJoin::Bevel,
                PenJoin::Round => LineJoin::Round,
            },
            opacity: opacity_of(pen.color, self.state.opacity),
            dash,
        };
        self.record(
            transform,
            Mark::Stroke {
                path: shape,
                stroke,
            },
        );
    }

    fn draw_text(&mut self, origin: PointF, text: &str) {
        if let Some(shaped) =
            text::shape_line(&self.state.font, self.metrics.dpi(), &text::one_line(text))
        {
            self.record_text(origin, &shaped);
        }
    }

    /// Laid out and clipped as the raster canvas lays it out (canvas::text).
    fn draw_text_in(&mut self, rect: RectF, flags: TextFlags, text: &str) -> RectF {
        let Some(laid) = text::lay_out(&self.state.font, self.metrics.dpi(), rect, flags, text)
        else {
            return RectF::new(rect.x, rect.y, 0.0, 0.0);
        };
        let clipped = !rect.contains_rect(&laid.bounds);
        if clipped {
            self.save();
            self.set_clip_rect(rect, true);
        }
        for (origin, shaped) in &laid.lines {
            self.record_text(*origin, shaped);
        }
        if clipped {
            self.restore();
        }
        laid.bounds
    }

    fn draw_image(&mut self, target: RectF, image: &Image, source: IntRect) {
        if source.is_empty() || image.is_null() || target.is_empty() {
            return;
        }
        let part = image.copy(source);
        let pixels =
            krilla::image::Image::from_rgba8(part.to_straight_rgba8(), part.width(), part.height());
        // The image's unit square laid on the target: moved to its corner under the transform.
        let transform = Transform::from_translate(target.x, target.y) * self.device_transform();
        let opacity = self.state.opacity;
        self.record(
            transform,
            Mark::Image {
                image: pixels,
                width: target.width as f32,
                height: target.height as f32,
                opacity,
            },
        );
    }

    fn metrics(&self) -> &dyn FontMetrics {
        &self.metrics
    }
}

/// A recorded page written onto krilla's surface, every push popped again.
fn replay(surface: &mut Surface<'_>, ops: Vec<Op>, points_per_pixel: f32) {
    surface.push_transform(&krilla::geom::Transform::from_scale(
        points_per_pixel,
        points_per_pixel,
    ));
    for op in ops {
        // A clip with nothing in it shows nothing of the mark.
        let clips: Option<Vec<(krilla::geom::Path, krilla::paint::FillRule)>> = op
            .clips
            .iter()
            .map(|(clip, rule)| krilla_path(clip).map(|path| (path, krilla_rule(*rule))))
            .collect();
        let Some(clips) = clips else {
            continue;
        };
        for (clip, rule) in &clips {
            surface.push_clip_path(clip, rule);
        }
        surface.push_transform(&krilla_transform(&op.transform));
        match op.mark {
            Mark::Fill { path, fill } => {
                if let Some(path) = krilla_path(&path) {
                    surface.set_stroke(None);
                    surface.set_fill(Some(fill));
                    surface.draw_path(&path);
                }
            }
            Mark::Stroke { path, stroke } => {
                if let Some(path) = krilla_path(&path) {
                    surface.set_fill(None);
                    surface.set_stroke(Some(stroke));
                    surface.draw_path(&path);
                }
            }
            Mark::Text { runs, fill } => {
                surface.set_stroke(None);
                surface.set_fill(Some(fill));
                for run in runs {
                    surface.draw_glyphs(
                        Point::from_xy(run.origin.0, run.origin.1),
                        run.glyphs.as_slice(),
                        run.font,
                        &run.text,
                        run.size,
                        false,
                    );
                }
            }
            Mark::Image {
                image,
                width,
                height,
                opacity,
            } => {
                if let Some(size) = Size::from_wh(width, height) {
                    let translucent = opacity < 1.0;
                    if translucent {
                        surface.push_opacity(normalized(opacity));
                    }
                    surface.draw_image(image, size);
                    if translucent {
                        surface.pop();
                    }
                }
            }
        }
        surface.pop();
        for _ in &clips {
            surface.pop();
        }
    }
    surface.pop();
}

/// A PDF of pages drawn on PdfCanvases (QPdfWriter's file).
pub struct PdfDocument {
    document: Document,
}

impl Default for PdfDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfDocument {
    /// A PDF whose page contents are compressed, as every plot is written.
    pub fn new() -> Self {
        Self {
            document: Document::new_with(SerializeSettings::default()),
        }
    }

    /// A PDF whose page contents and text maps are written as they are, for a test or a person
    /// to read: spaced too, each dictionary's key apart from its value ("/Subtype /Image", as
    /// QPdfWriter writes it, where krilla's compact form is "/Subtype/Image").
    pub fn uncompressed() -> Self {
        Self {
            document: Document::new_with(SerializeSettings {
                compress_content_streams: false,
                pretty: true,
                ..SerializeSettings::default()
            }),
        }
    }

    /// `page`, drawn, as the document's next page. InvalidArgument for a page of no size.
    pub fn add_page(&mut self, page: PdfCanvas) -> Status {
        let Some(settings) =
            PageSettings::from_wh(page.width_points as f32, page.height_points as f32)
        else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a PDF page must have a size",
                format!(
                    "{} x {} points",
                    katana_core::text::format_exact_real(page.width_points),
                    katana_core::text::format_exact_real(page.height_points)
                ),
            ));
        };
        let points_per_pixel = (72.0 / page.resolution) as f32;
        let mut pdf_page = self.document.start_page_with(settings);
        let mut surface = pdf_page.surface();
        replay(&mut surface, page.ops, points_per_pixel);
        surface.finish();
        pdf_page.finish();
        Ok(())
    }

    /// The PDF file's bytes. FileExportFailure when krilla cannot write it (a font it cannot
    /// embed).
    pub fn finish(self) -> Result<Vec<u8>> {
        self.document.finish().map_err(|error| {
            make_error(
                ErrorCode::FileExportFailure,
                "the PDF could not be written",
                format!("{error}"),
            )
        })
    }

    /// The PDF written to `path`.
    pub fn write(self, path: &FilePath) -> Status {
        let shown = String::from_utf8_lossy(&path_to_utf8(path)).into_owned();
        let bytes = self.finish().map_err(|error| {
            let context = format!("{shown}: {}", error.context);
            error.with_context(context)
        })?;
        std::fs::write(path, bytes).map_err(|error| {
            make_error(
                ErrorCode::FileExportFailure,
                format!("could not write the PDF file: {error}"),
                shown,
            )
        })
    }
}
