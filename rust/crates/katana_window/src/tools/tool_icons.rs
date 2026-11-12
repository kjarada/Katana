//! Icons for the catalogue's interactive tools, by tool id (katana_cad::interactive_tool). A tool's
//! icon lives with its family - tools/icons_<family> paints the family's tools - so a tool is
//! added, drawn and tested in its family's files without editing a shared table.
//!
//! The painting conventions are the application's own (icons): a 24-unit grid, a 1.7-unit stroke,
//! a neutral tone for the object and the accent for what the command does to it. ToolInk carries
//! them, and the application's own icons are painted with it too: the C++ kept a copy of it in
//! icons.cpp ("this DUPLICATES icons.cpp's private Ink while that file is being changed by other
//! work; merging the two into this header is a follow-up"), and the port is that follow-up - one
//! ink, the same strokes.
//!
//! A tool's icon is reached by its id through the window's icon set (icons::paint_named_icon,
//! icons::texture): the C++ toolIcon's QIcon is that set's entry for the id.

use crate::canvas::{
    Brush, Canvas, Color, Elide, Font, FontMetrics, FontSize, FontWeight, Image, IntRect, Path,
    Pen, PenCap, PenJoin, PenStyle, PointF, RectF, RenderHint, SizeF, TextFlags, Transform,
};

pub use super::icons_annotate::paint_annotate_icon;
pub use super::icons_draw_curves::paint_draw_curve_icon;
pub use super::icons_draw_lines::paint_draw_line_icon;
pub use super::icons_drawing::paint_drawing_icon;
pub use super::icons_everyday::{
    paint_draw_divide_icon, paint_modify_length_icon, paint_property_icon, paint_select_icon,
};
pub use super::icons_inquiry::paint_inquiry_icon;
pub use super::icons_modify_edit::paint_modify_edit_icon;
pub use super::icons_modify_transform::paint_modify_transform_icon;
pub use super::icons_terrain::paint_terrain_edit_icon;

/// The design grid: every coordinate of an icon is in these units, and the painter is scaled so
/// that 24 of them fill whatever rectangle the icon is painted in.
pub const GRID: f64 = 24.0;
/// Stroke weight on that grid. 1.7 is the weight at which a 20 px toolbar icon still reads as a
/// line drawing and a 16 px menu icon does not fill in; the common 2.0 of icon sets drawn for
/// 24 px display closes up small counters (the label of Save, the inside of the magnet) when
/// shown at 16.
pub const STROKE: f64 = 1.7;

/// A pen and brush for each of the two tones, and the handful of marks every icon is built from.
/// Keeping the marks here is what keeps the set consistent: a node is the same node on every
/// draw tool.
pub struct ToolInk<'a> {
    canvas: &'a mut dyn Canvas,
    neutral: Color,
    accent: Color,
}

impl<'a> ToolInk<'a> {
    pub fn new(canvas: &'a mut dyn Canvas, neutral: Color, accent: Color) -> Self {
        Self {
            canvas,
            neutral,
            accent,
        }
    }

    fn tone(&self, accent: bool) -> Color {
        if accent { self.accent } else { self.neutral }
    }

    /// The path stroked with round caps and joins, `width` grid units wide.
    pub fn stroke(&mut self, path: &Path, accent: bool, width: f64) {
        let pen = Pen::with(
            self.tone(accent),
            width,
            PenStyle::Solid,
            PenCap::Round,
            PenJoin::Round,
        );
        self.canvas.set_brush(Brush::none());
        self.canvas.set_pen(pen);
        self.canvas.draw_path(path);
    }

    /// The path stroked dashed: dashes of 1.6 and gaps of 1.4 stroke widths, flat-ended.
    pub fn dashed(&mut self, path: &Path, accent: bool) {
        let pen = Pen::with(
            self.tone(accent),
            STROKE,
            PenStyle::Custom(vec![1.6, 1.4]),
            PenCap::Flat,
            PenJoin::Round,
        );
        self.canvas.set_brush(Brush::none());
        self.canvas.set_pen(pen);
        self.canvas.draw_path(path);
    }

    /// The path filled, its tone's alpha scaled by `alpha` out of 255 (QColor::setAlpha(alpha() *
    /// alpha / 255), in whole numbers as Qt's ints divide).
    pub fn fill(&mut self, path: &Path, accent: bool, alpha: i32) {
        let tone = self.tone(accent);
        let scaled = (i32::from(tone.a) * alpha / 255).clamp(0, 255) as u8;
        self.canvas.set_pen(Pen::none());
        self.canvas.set_brush(Brush::solid(tone.with_alpha(scaled)));
        self.canvas.draw_path(path);
    }

    pub fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, accent: bool, width: f64) {
        let mut path = start(x1, y1);
        path.line_to(pt(x2, y2));
        self.stroke(&path, accent, width);
    }

    pub fn dot(&mut self, x: f64, y: f64, radius: f64, accent: bool) {
        let path = circle(x, y, radius);
        self.fill(&path, accent, 255);
    }

    /// A vertex handle, as the viewport draws a grip: a small filled square.
    pub fn node(&mut self, x: f64, y: f64, accent: bool) {
        let path = rectangle(x - 1.5, y - 1.5, 3.0, 3.0);
        self.fill(&path, accent, 255);
    }

    /// Text on the grid (a dimension's figure, an "A" for Text), centred on (x, y), `size` grid
    /// units tall.
    pub fn label(&mut self, x: f64, y: f64, size: f64, text: &str, accent: bool) {
        let tone = self.tone(accent);
        self.canvas.save();
        let mut font = self.canvas.font();
        // Drawn large and scaled down: pixel sizes are integers.
        font.size = FontSize::Pixels(100);
        font.weight = FontWeight::Bold;
        self.canvas.set_font(font);
        self.canvas.set_pen(Pen::new(tone));
        self.canvas.translate(x, y);
        self.canvas.scale(size / 100.0, size / 100.0);
        self.canvas.draw_text_in(
            RectF::new(-300.0, -60.0, 600.0, 120.0),
            TextFlags::CENTER,
            text,
        );
        self.canvas.restore();
    }

    /// The canvas the ink paints on (ToolInk::painter), for what an icon draws past the marks.
    pub fn canvas(&mut self) -> &mut (dyn Canvas + 'a) {
        &mut *self.canvas
    }
}

/// QPointF(x, y).
pub fn pt(x: f64, y: f64) -> PointF {
    PointF::new(x, y)
}

/// A path that begins at (x, y) (QPainterPath(QPointF)).
pub fn start(x: f64, y: f64) -> Path {
    let mut path = Path::new();
    path.move_to(pt(x, y));
    path
}

/// The points joined in order, the last back to the first when `closed`.
pub fn polyline(points: &[PointF], closed: bool) -> Path {
    let mut path = Path::new();
    let mut first = true;
    for &point in points {
        if first {
            path.move_to(point);
            first = false;
        } else {
            path.line_to(point);
        }
    }
    if closed {
        path.close_subpath();
    }
    path
}

pub fn rectangle(x: f64, y: f64, w: f64, h: f64) -> Path {
    let mut path = Path::new();
    path.add_rect(RectF::new(x, y, w, h));
    path
}

pub fn circle(x: f64, y: f64, radius: f64) -> Path {
    let mut path = Path::new();
    path.add_ellipse(RectF::new(
        x - radius,
        y - radius,
        2.0 * radius,
        2.0 * radius,
    ));
    path
}

/// An arc of a circle, angles in degrees counter-clockwise from east as on paper (the painter's
/// y points down; this flips it).
pub fn arc(cx: f64, cy: f64, radius: f64, start_degrees: f64, sweep_degrees: f64) -> Path {
    // QPainterPath::arcTo already measures angles counter-clockwise on screen, which is what "as
    // on paper" means once y points down.
    let bounds = RectF::new(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius);
    let mut path = Path::new();
    path.arc_move_to(bounds, start_degrees);
    path.arc_to(bounds, start_degrees, sweep_degrees);
    path
}

/// Every family's painter in turn, as the cad families are listed (src/katana_cad/tools).
fn paint_tool(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    paint_draw_line_icon(tool_id, ink)
        || paint_draw_curve_icon(tool_id, ink)
        || paint_modify_transform_icon(tool_id, ink)
        || paint_modify_edit_icon(tool_id, ink)
        || paint_annotate_icon(tool_id, ink)
        || paint_inquiry_icon(tool_id, ink)
        || paint_draw_divide_icon(tool_id, ink)
        || paint_modify_length_icon(tool_id, ink)
        || paint_property_icon(tool_id, ink)
        || paint_select_icon(tool_id, ink)
        || paint_drawing_icon(tool_id, ink)
        || paint_terrain_edit_icon(tool_id, ink)
}

/// Paints the icon of tool `tool_id` into `rect` with the given colours. A tool whose family
/// paints nothing for it gets a plain glyph of its initial, so a missing icon is visible on the
/// sheet rather than an empty button.
pub fn paint_tool_icon(
    canvas: &mut dyn Canvas,
    tool_id: &str,
    rect: RectF,
    neutral: Color,
    accent: Color,
) {
    canvas.save();
    canvas.set_render_hint(RenderHint::Antialiasing, true);
    canvas.translate(rect.x, rect.y);
    canvas.scale(rect.width / GRID, rect.height / GRID);
    {
        let mut ink = ToolInk::new(canvas, neutral, accent);
        if !paint_tool(tool_id, &mut ink) {
            // The initial of the last part of the id, in a frame: visibly a placeholder, never
            // mistaken for a finished icon.
            let leaf = tool_id.rfind('.').map_or(tool_id, |at| &tool_id[at + 1..]);
            ink.stroke(&rectangle(3.0, 3.0, 18.0, 18.0), false, 1.0);
            let initial: String = match leaf.chars().next() {
                Some(first) => first.to_uppercase().collect(),
                None => String::from("?"),
            };
            ink.label(12.0, 12.0, 12.0, &initial, false);
        }
    }
    canvas.restore();
}

/// True when a family paints this tool.
pub fn has_tool_icon(tool_id: &str) -> bool {
    let mut scratch = NullCanvas;
    let mut ink = ToolInk::new(&mut scratch, Color::WHITE, Color::WHITE);
    paint_tool(tool_id, &mut ink)
}

// ---- a canvas that paints nothing ---------------------------------------------------------------

/// What `has_tool_icon` paints on: the C++ painted into a 24 px scratch pixmap and kept only the
/// answer, so nothing here is drawn at all.
struct NullCanvas;

struct NullMetrics;

impl FontMetrics for NullMetrics {
    fn ascent(&self, _font: &Font) -> f64 {
        0.0
    }
    fn descent(&self, _font: &Font) -> f64 {
        0.0
    }
    fn height(&self, _font: &Font) -> f64 {
        0.0
    }
    fn line_spacing(&self, _font: &Font) -> f64 {
        0.0
    }
    fn horizontal_advance(&self, _font: &Font, _text: &str) -> f64 {
        0.0
    }
    fn bounding_rect(&self, _font: &Font, _text: &str) -> RectF {
        RectF::default()
    }
    fn elided_text(&self, _font: &Font, text: &str, _mode: Elide, _width: f64) -> String {
        String::from(text)
    }
}

impl Canvas for NullCanvas {
    fn device_size(&self) -> SizeF {
        SizeF::new(GRID, GRID)
    }
    fn device_pixel_ratio(&self) -> f64 {
        1.0
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn pen(&self) -> Pen {
        Pen::default()
    }
    fn set_pen(&mut self, _pen: Pen) {}
    fn brush(&self) -> Brush {
        Brush::none()
    }
    fn set_brush(&mut self, _brush: Brush) {}
    fn font(&self) -> Font {
        Font::default()
    }
    fn set_font(&mut self, _font: Font) {}
    fn render_hint(&self, _hint: RenderHint) -> bool {
        false
    }
    fn set_render_hint(&mut self, _hint: RenderHint, _on: bool) {}
    fn opacity(&self) -> f64 {
        1.0
    }
    fn set_opacity(&mut self, _opacity: f64) {}
    fn world_transform(&self) -> Transform {
        Transform::IDENTITY
    }
    fn set_world_transform(&mut self, _transform: Transform, _combine: bool) {}
    fn device_transform(&self) -> Transform {
        Transform::IDENTITY
    }
    fn set_clip_rect(&mut self, _rect: RectF, _intersect: bool) {}
    fn set_clip_path(&mut self, _path: &Path, _intersect: bool) {}
    fn set_clipping(&mut self, _on: bool) {}
    fn has_clipping(&self) -> bool {
        false
    }
    fn draw_path(&mut self, _path: &Path) {}
    fn fill_path(&mut self, _path: &Path, _brush: &Brush) {}
    fn stroke_path(&mut self, _path: &Path, _pen: &Pen) {}
    fn draw_text(&mut self, _origin: PointF, _text: &str) {}
    fn draw_text_in(&mut self, rect: RectF, _flags: TextFlags, _text: &str) -> RectF {
        RectF::new(rect.center().x, rect.center().y, 0.0, 0.0)
    }
    fn draw_image(&mut self, _target: RectF, _image: &Image, _source: IntRect) {}
    fn metrics(&self) -> &dyn FontMetrics {
        &NullMetrics
    }
}
