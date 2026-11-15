//! The one painter's surface: the subset of QPainter the window's painters call, so the plan, the
//! sheets, the sections, the legends, the icons and the previews are painted once, onto the screen,
//! a PNG, a TIFF or a PDF alike (the plan's section 4.4). C writes its two backends: raster
//! (tiny-skia, with text shaped by cosmic-text) and PDF (krilla).
//!
//! The census it was taken from: every call on a QPainter in src/katana_qt (the variables
//! `painter`, `painter_`, `p`, `layer`, `imagePainter`; read-only scripts over the tree,
//! 2026-11-11), by count:
//!
//! ```text
//! setPen 196        setBrush 109      drawLine 90       drawText 52       save/restore 50
//! setRenderHint 49  fillRect 41       drawRect 31       drawEllipse 22    setFont 21
//! drawPolygon 21    translate 19      drawPolyline 19   drawImage 19      scale 16
//! setClipRect 14    drawPath 13       font 10           rotate 8          pen 8
//! drawRoundedRect 8 fillPath 6        drawPoint 5       setTransform 4    setClipping 4
//! deviceTransform 4 fontMetrics 3     drawPixmap 3      device 3          worldTransform 2
//! transform 2       setWorldTransform 2  setOpacity 2   setClipPath 2     drawRects 2
//! testRenderHint 1  strokePath 1      shear 1           setRenderHints 1  renderHints 1
//! brush 1           drawPoints 1      drawLines 1
//! ```
//!
//! Render hints: Antialiasing 39, SmoothPixmapTransform 7, TextAntialiasing 4. Pen styles:
//! NoPen 22, SolidLine 21, DashLine 13, DotLine 4, CustomDashLine 4 (setDashPattern 8); caps FlatCap
//! 17, RoundCap 16, SquareCap 1; joins RoundJoin 15 (setJoinStyle 3); cosmetic pens 2. Brushes:
//! NoBrush 60, solid colours, QLinearGradient 7; no pattern brush and no composition mode. Text:
//! drawText at a baseline point or in a rectangle with Qt::Align* (Right 20, Center 18, VCenter 16,
//! Left 10, Top 5, HCenter 4, Bottom 1), TextWordWrap 7, TextSingleLine 1; elided text (ElideMiddle
//! 3, ElideRight 2). Fonts: setBold 24, setPixelSize 14, setPointSizeF 7, setItalic 4, families and
//! style hints 2 each, capitalisation and letter spacing 1 each (a menu's section title), DemiBold 1.
//! Metrics: horizontalAdvance 19, lineSpacing, height, ascent, descent, boundingRect,
//! elidedText. Paths (QPainterPath): lineTo 24, cubicTo 21, quadTo 11, moveTo 11, closeSubpath 9,
//! addEllipse 8, arcTo and arcMoveTo 5 each, addRect 5, addRoundedRect 3, addPolygon 3,
//! translated 3, addText 1. Images: ARGB32_Premultiplied 18, ARGB32 7, RGB32 3, Grayscale8 2,
//! RGBA8888 and RGB888 1 each.
//!
//! What is not here, because no painter calls it: composition modes, pattern brushes, radial and
//! conical gradients, clip regions, the paint engine's native painting, background modes.

use std::ops::{Add, AddAssign, Mul, Neg, Sub};

use super::image::Image;

// ---- geometry ----------------------------------------------------------------------------------

/// QPointF: a point in the painter's coordinates, x right and y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointF {
    pub x: f64,
    pub y: f64,
}

impl PointF {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl Add for PointF {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl AddAssign for PointF {
    fn add_assign(&mut self, other: Self) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl Sub for PointF {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f64> for PointF {
    type Output = Self;
    fn mul(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }
}

impl Neg for PointF {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

/// QSizeF.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SizeF {
    pub width: f64,
    pub height: f64,
}

impl SizeF {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// QRectF: its left and top, and its width and height. `right()` is `left() + width()`, with no
/// pixel subtracted (that is QRect's, IntRect's here).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl RectF {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// From two corners, as given (QRectF(topLeft, bottomRight)).
    pub fn from_corners(top_left: PointF, bottom_right: PointF) -> Self {
        Self::new(
            top_left.x,
            top_left.y,
            bottom_right.x - top_left.x,
            bottom_right.y - top_left.y,
        )
    }

    pub fn left(&self) -> f64 {
        self.x
    }

    pub fn top(&self) -> f64 {
        self.y
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn top_left(&self) -> PointF {
        PointF::new(self.x, self.y)
    }

    pub fn top_right(&self) -> PointF {
        PointF::new(self.right(), self.y)
    }

    pub fn bottom_left(&self) -> PointF {
        PointF::new(self.x, self.bottom())
    }

    pub fn bottom_right(&self) -> PointF {
        PointF::new(self.right(), self.bottom())
    }

    /// QRectF::center: the left plus half the width, the top plus half the height.
    pub fn center(&self) -> PointF {
        PointF::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn size(&self) -> SizeF {
        SizeF::new(self.width, self.height)
    }

    /// QRectF::isNull: both sides zero.
    pub fn is_null(&self) -> bool {
        self.width == 0.0 && self.height == 0.0
    }

    /// QRectF::isEmpty: a side that is not positive.
    pub fn is_empty(&self) -> bool {
        !(self.width > 0.0 && self.height > 0.0)
    }

    /// QRectF::adjusted: each edge moved by its own amount.
    pub fn adjusted(&self, left: f64, top: f64, right: f64, bottom: f64) -> Self {
        Self::new(
            self.x + left,
            self.y + top,
            self.width + right - left,
            self.height + bottom - top,
        )
    }

    pub fn translated(&self, dx: f64, dy: f64) -> Self {
        Self::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// The same rectangle with a positive width and height (QRectF::normalized).
    pub fn normalized(&self) -> Self {
        let mut out = *self;
        if out.width < 0.0 {
            out.x += out.width;
            out.width = -out.width;
        }
        if out.height < 0.0 {
            out.y += out.height;
            out.height = -out.height;
        }
        out
    }

    /// QRectF::contains(QPointF): the edges included.
    pub fn contains(&self, point: PointF) -> bool {
        let r = self.normalized();
        point.x >= r.x && point.x <= r.right() && point.y >= r.y && point.y <= r.bottom()
    }

    /// QRectF::contains(QRectF): every point of `other` inside, edges included.
    pub fn contains_rect(&self, other: &RectF) -> bool {
        let (a, b) = (self.normalized(), other.normalized());
        b.x >= a.x && b.right() <= a.right() && b.y >= a.y && b.bottom() <= a.bottom()
    }

    /// QRectF::united: the smallest rectangle holding both; an empty one is ignored.
    pub fn united(&self, other: &RectF) -> Self {
        if other.is_null() {
            return *self;
        }
        if self.is_null() {
            return *other;
        }
        let (a, b) = (self.normalized(), other.normalized());
        let left = katana_math::cpp::min(a.x, b.x);
        let top = katana_math::cpp::min(a.y, b.y);
        let right = katana_math::cpp::max(a.right(), b.right());
        let bottom = katana_math::cpp::max(a.bottom(), b.bottom());
        Self::new(left, top, right - left, bottom - top)
    }

    /// QRectF::intersected: the overlap; an empty rectangle when there is none.
    pub fn intersected(&self, other: &RectF) -> Self {
        let (a, b) = (self.normalized(), other.normalized());
        let left = katana_math::cpp::max(a.x, b.x);
        let top = katana_math::cpp::max(a.y, b.y);
        let right = katana_math::cpp::min(a.right(), b.right());
        let bottom = katana_math::cpp::min(a.bottom(), b.bottom());
        if left > right || top > bottom {
            return Self::default();
        }
        Self::new(left, top, right - left, bottom - top)
    }
}

/// QRect, for regions of an image in whole pixels. `right()` is `x + width - 1`, as QRect's is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct IntRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl IntRect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width - 1
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height - 1
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }

    pub fn to_rect_f(&self) -> RectF {
        RectF::new(
            f64::from(self.x),
            f64::from(self.y),
            f64::from(self.width),
            f64::from(self.height),
        )
    }
}

/// QTransform, affine: a point (x, y) maps to (m11 x + m21 y + dx, m12 x + m22 y + dy). Composition
/// is Qt's: `a * b` applies `a` first, then `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub m11: f64,
    pub m12: f64,
    pub m21: f64,
    pub m22: f64,
    pub dx: f64,
    pub dy: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Qt's degrees-to-radians factor (qtransform.cpp's deg2rad, 0.017453292519943295769): pi over
/// 180, the same double.
const DEG_TO_RAD: f64 = std::f64::consts::PI / 180.0;

impl Transform {
    pub const IDENTITY: Self = Self {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        dx: 0.0,
        dy: 0.0,
    };

    pub const fn new(m11: f64, m12: f64, m21: f64, m22: f64, dx: f64, dy: f64) -> Self {
        Self {
            m11,
            m12,
            m21,
            m22,
            dx,
            dy,
        }
    }

    /// QTransform::fromTranslate.
    pub const fn from_translate(dx: f64, dy: f64) -> Self {
        Self::new(1.0, 0.0, 0.0, 1.0, dx, dy)
    }

    /// QTransform::fromScale.
    pub const fn from_scale(sx: f64, sy: f64) -> Self {
        Self::new(sx, 0.0, 0.0, sy, 0.0, 0.0)
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// QTransform::translate: the translation applied before this transform.
    pub fn translated(&self, dx: f64, dy: f64) -> Self {
        if dx == 0.0 && dy == 0.0 {
            return *self;
        }
        let mut out = *self;
        out.dx += dx * self.m11 + dy * self.m21;
        out.dy += dy * self.m22 + dx * self.m12;
        out
    }

    /// QTransform::scale: the scale applied before this transform.
    pub fn scaled(&self, sx: f64, sy: f64) -> Self {
        let mut out = *self;
        out.m11 *= sx;
        out.m12 *= sx;
        out.m21 *= sy;
        out.m22 *= sy;
        out
    }

    /// QTransform::rotate, by `degrees` clockwise on a screen whose y runs down; the quarter turns
    /// exact, as Qt makes them.
    pub fn rotated(&self, degrees: f64) -> Self {
        if degrees == 0.0 {
            return *self;
        }
        let (mut sin, mut cos) = (0.0, 0.0);
        if degrees == 90.0 || degrees == -270.0 {
            sin = 1.0;
        } else if degrees == 270.0 || degrees == -90.0 {
            sin = -1.0;
        } else if degrees == 180.0 {
            cos = -1.0;
        } else {
            let radians = DEG_TO_RAD * degrees;
            sin = radians.sin();
            cos = radians.cos();
        }
        let mut out = *self;
        out.m11 = cos * self.m11 + sin * self.m21;
        out.m12 = cos * self.m12 + sin * self.m22;
        out.m21 = -sin * self.m11 + cos * self.m21;
        out.m22 = -sin * self.m12 + cos * self.m22;
        out
    }

    /// QTransform::shear.
    pub fn sheared(&self, sh: f64, sv: f64) -> Self {
        let mut out = *self;
        out.m11 = self.m11 + sv * self.m21;
        out.m12 = self.m12 + sv * self.m22;
        out.m21 = sh * self.m11 + self.m21;
        out.m22 = sh * self.m12 + self.m22;
        out
    }

    pub fn map(&self, point: PointF) -> PointF {
        PointF::new(
            self.m11 * point.x + self.m21 * point.y + self.dx,
            self.m12 * point.x + self.m22 * point.y + self.dy,
        )
    }

    /// QTransform::mapRect: the bounding rectangle of the four mapped corners.
    pub fn map_rect(&self, rect: &RectF) -> RectF {
        let corners = [
            self.map(rect.top_left()),
            self.map(rect.top_right()),
            self.map(rect.bottom_left()),
            self.map(rect.bottom_right()),
        ];
        let mut left = corners[0].x;
        let mut right = corners[0].x;
        let mut top = corners[0].y;
        let mut bottom = corners[0].y;
        for corner in &corners[1..] {
            left = katana_math::cpp::min(left, corner.x);
            right = katana_math::cpp::max(right, corner.x);
            top = katana_math::cpp::min(top, corner.y);
            bottom = katana_math::cpp::max(bottom, corner.y);
        }
        RectF::new(left, top, right - left, bottom - top)
    }

    /// The transform's determinant (QTransform::determinant, affine).
    pub fn determinant(&self) -> f64 {
        self.m11 * self.m22 - self.m12 * self.m21
    }

    /// QTransform::inverted: None for a transform with no inverse.
    pub fn inverted(&self) -> Option<Self> {
        let det = self.determinant();
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let m11 = self.m22 / det;
        let m12 = -self.m12 / det;
        let m21 = -self.m21 / det;
        let m22 = self.m11 / det;
        let dx = -(self.dx * m11 + self.dy * m21);
        let dy = -(self.dx * m12 + self.dy * m22);
        Some(Self::new(m11, m12, m21, m22, dx, dy))
    }
}

impl Mul for Transform {
    type Output = Self;

    /// Qt's `a * b`: `a` applied first, then `b`.
    fn mul(self, b: Self) -> Self {
        Self::new(
            self.m11 * b.m11 + self.m12 * b.m21,
            self.m11 * b.m12 + self.m12 * b.m22,
            self.m21 * b.m11 + self.m22 * b.m21,
            self.m21 * b.m12 + self.m22 * b.m22,
            self.dx * b.m11 + self.dy * b.m21 + b.dx,
            self.dx * b.m12 + self.dy * b.m22 + b.dy,
        )
    }
}

// ---- colour, pen, brush -------------------------------------------------------------------------

/// QColor as the painters use it: 8 bits a channel, alpha not premultiplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// QColor::setAlpha, as a new colour.
    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// QColor::name(): "#rrggbb", lower case, the alpha left out.
    pub fn name(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// QColor::fromString for the forms the window writes: "#rgb", "#rrggbb" and "#aarrggbb", in
    /// either case; None for anything else.
    pub fn from_name(text: &str) -> Option<Self> {
        let hex = text.strip_prefix('#')?;
        if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let nibble = |at: usize| u8::from_str_radix(&hex[at..=at], 16).ok();
        let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
        match hex.len() {
            3 => Some(Self::rgb(nibble(0)? * 17, nibble(1)? * 17, nibble(2)? * 17)),
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::rgba(byte(2)?, byte(4)?, byte(6)?, byte(0)?)),
            _ => None,
        }
    }

    /// The colour as egui paints it (premultiplied).
    pub fn to_egui(self) -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(self.r, self.g, self.b, self.a)
    }
}

impl From<Color> for egui::Color32 {
    fn from(color: Color) -> Self {
        color.to_egui()
    }
}

/// Qt::PenStyle.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum PenStyle {
    /// Qt::NoPen: nothing is stroked.
    None,
    #[default]
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
    /// Qt::CustomDashLine: dashes and gaps in units of the pen's width (QPen::setDashPattern).
    Custom(Vec<f64>),
}

/// Qt::PenCapStyle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PenCap {
    Flat,
    #[default]
    Square,
    Round,
}

/// Qt::PenJoinStyle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PenJoin {
    Miter,
    #[default]
    Bevel,
    Round,
}

/// QPen. Qt's defaults: black, a width of one, solid, square caps, bevelled joins. A width of zero
/// is a cosmetic hairline, a pixel wide whatever the transform, as is any pen made cosmetic.
#[derive(Clone, Debug, PartialEq)]
pub struct Pen {
    pub color: Color,
    pub width: f64,
    pub style: PenStyle,
    pub cap: PenCap,
    pub join: PenJoin,
    /// QPen::setDashOffset, in units of the width.
    pub dash_offset: f64,
    /// QPen::setCosmetic: the width is in device pixels, not scaled by the transform.
    pub cosmetic: bool,
}

impl Default for Pen {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            width: 1.0,
            style: PenStyle::Solid,
            cap: PenCap::Square,
            join: PenJoin::Bevel,
            dash_offset: 0.0,
            cosmetic: false,
        }
    }
}

impl Pen {
    /// QPen(color): solid, a width of one.
    pub fn new(color: Color) -> Self {
        Self {
            color,
            ..Self::default()
        }
    }

    /// QPen(color, width, style, cap, join).
    pub fn with(color: Color, width: f64, style: PenStyle, cap: PenCap, join: PenJoin) -> Self {
        Self {
            color,
            width,
            style,
            cap,
            join,
            ..Self::default()
        }
    }

    /// Qt::NoPen.
    pub fn none() -> Self {
        Self {
            style: PenStyle::None,
            ..Self::default()
        }
    }

    pub fn is_none(&self) -> bool {
        self.style == PenStyle::None
    }
}

/// A colour at a place along a gradient, 0 to 1 (QGradient::setColorAt).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GradientStop {
    pub position: f64,
    pub color: Color,
}

/// QLinearGradient: colours along the line from `start` to `end`, padded beyond.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinearGradient {
    pub start: PointF,
    pub end: PointF,
    pub stops: Vec<GradientStop>,
}

impl LinearGradient {
    pub fn new(start: PointF, end: PointF) -> Self {
        Self {
            start,
            end,
            stops: Vec::new(),
        }
    }

    /// QGradient::setColorAt: a stop at `position` replaces one already there, and the stops stay
    /// in order.
    pub fn set_color_at(&mut self, position: f64, color: Color) {
        let stop = GradientStop { position, color };
        match self.stops.iter().position(|kept| kept.position >= position) {
            Some(at) if self.stops[at].position == position => self.stops[at] = stop,
            Some(at) => self.stops.insert(at, stop),
            None => self.stops.push(stop),
        }
    }
}

/// What fills a shape.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum BrushStyle {
    /// Qt::NoBrush.
    #[default]
    None,
    Solid(Color),
    Linear(LinearGradient),
}

/// QBrush.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Brush {
    pub style: BrushStyle,
}

impl Brush {
    pub fn none() -> Self {
        Self {
            style: BrushStyle::None,
        }
    }

    pub fn solid(color: Color) -> Self {
        Self {
            style: BrushStyle::Solid(color),
        }
    }

    pub fn linear(gradient: LinearGradient) -> Self {
        Self {
            style: BrushStyle::Linear(gradient),
        }
    }

    pub fn is_none(&self) -> bool {
        self.style == BrushStyle::None
    }
}

impl From<Color> for Brush {
    fn from(color: Color) -> Self {
        Self::solid(color)
    }
}

// ---- text --------------------------------------------------------------------------------------

/// A font's size: in points (QFont::setPointSizeF) or in pixels (setPixelSize).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FontSize {
    Points(f64),
    Pixels(i32),
}

impl Default for FontSize {
    /// Qt's default application font size on the platforms the window runs on: 9 points.
    fn default() -> Self {
        Self::Points(9.0)
    }
}

/// QFont::Weight, as the painters set it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontWeight {
    #[default]
    Normal,
    DemiBold,
    Bold,
}

/// QFont: the families asked for in order (the first the machine has is used; the UI's sans is
/// the fallback), the size, and the styles the painters set.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub families: Vec<String>,
    pub size: FontSize,
    pub weight: FontWeight,
    pub italic: bool,
    /// QFont::AllUppercase.
    pub all_uppercase: bool,
    /// QFont::PercentageSpacing: 100 is the font's own.
    pub letter_spacing_percent: f64,
    /// QFont::setFixedPitch / Qt's TypeWriter style hint: a fixed-pitch face wanted.
    pub fixed_pitch: bool,
}

impl Default for Font {
    /// The UI's own face at Qt's default size, its own spacing.
    fn default() -> Self {
        Self {
            families: Vec::new(),
            size: FontSize::default(),
            weight: FontWeight::Normal,
            italic: false,
            all_uppercase: false,
            letter_spacing_percent: 100.0,
            fixed_pitch: false,
        }
    }
}

impl Font {
    /// The UI's own face at `size`.
    pub fn sized(size: FontSize) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }

    /// A family at `size`.
    pub fn family(family: &str, size: FontSize) -> Self {
        Self {
            families: vec![String::from(family)],
            ..Self::sized(size)
        }
    }

    /// QFont::setBold.
    pub fn bold(mut self, bold: bool) -> Self {
        self.weight = if bold {
            FontWeight::Bold
        } else {
            FontWeight::Normal
        };
        self
    }
}

/// Qt::AlignLeft, AlignRight, AlignHCenter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HAlign {
    #[default]
    Left,
    Right,
    HCenter,
}

/// Qt::AlignTop, AlignBottom, AlignVCenter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VAlign {
    #[default]
    Top,
    Bottom,
    VCenter,
}

/// The flags of QPainter::drawText(rect, flags, text): where in the rectangle the text goes, and
/// whether it wraps at words. No flags is Qt's default: left and top, on one line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextFlags {
    pub horizontal: HAlign,
    pub vertical: VAlign,
    /// Qt::TextWordWrap.
    pub word_wrap: bool,
    /// Qt::TextSingleLine: a line break is drawn as a space.
    pub single_line: bool,
}

impl TextFlags {
    /// Qt::AlignCenter.
    pub const CENTER: Self = Self {
        horizontal: HAlign::HCenter,
        vertical: VAlign::VCenter,
        word_wrap: false,
        single_line: false,
    };

    pub const fn aligned(horizontal: HAlign, vertical: VAlign) -> Self {
        Self {
            horizontal,
            vertical,
            word_wrap: false,
            single_line: false,
        }
    }

    pub const fn wrapped(self) -> Self {
        Self {
            word_wrap: true,
            ..self
        }
    }
}

/// Qt::TextElideMode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Elide {
    Left,
    #[default]
    Right,
    Middle,
    None,
}

/// QFontMetricsF over the fonts the canvas draws with. The integer QFontMetrics of the C++ is this
/// rounded where the caller rounds: its height is the rounded ascent plus the rounded descent.
pub trait FontMetrics {
    fn ascent(&self, font: &Font) -> f64;
    fn descent(&self, font: &Font) -> f64;
    /// The ascent plus the descent.
    fn height(&self, font: &Font) -> f64;
    /// The height plus the leading: the step from one baseline to the next.
    fn line_spacing(&self, font: &Font) -> f64;
    fn horizontal_advance(&self, font: &Font, text: &str) -> f64;
    /// The rectangle the text's ink takes, its origin on the baseline at the text's start.
    fn bounding_rect(&self, font: &Font, text: &str) -> RectF;
    /// The text cut to `width` with an ellipsis where `mode` says (QFontMetrics::elidedText).
    fn elided_text(&self, font: &Font, text: &str, mode: Elide, width: f64) -> String;
}

// ---- paths -------------------------------------------------------------------------------------

/// Qt::FillRule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FillRule {
    /// QPainterPath's and drawPolygon's default.
    #[default]
    OddEven,
    Winding,
}

/// One element of a path.
#[derive(Clone, Debug, PartialEq)]
pub enum PathElement {
    MoveTo(PointF),
    LineTo(PointF),
    /// Two control points and the end.
    CubicTo(PointF, PointF, PointF),
    /// The subpath closed back to its start (the line to it is the element before).
    Close,
    /// A text's glyph outlines, its baseline starting at `origin` (QPainterPath::addText): the
    /// backend turns it into outlines with the font it draws with.
    Text {
        origin: PointF,
        font: Font,
        text: String,
    },
}

/// The kappa of a quarter ellipse's cubic (QT_PATH_KAPPA in qstroker.cpp).
const PATH_KAPPA: f64 = 0.552_284_749_8;

/// QPainterPath: subpaths of lines and cubic curves, with Qt's rules for where a new element
/// starts - an empty path begins at the origin, a move after a move replaces it, and after a
/// closed shape (a rectangle, an ellipse, a close) the next line begins a new subpath where the
/// last ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub elements: Vec<PathElement>,
    pub fill_rule: FillRule,
    require_move_to: bool,
}

impl Path {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Where the path is now: the end of its last element, the origin when it is empty.
    pub fn current_position(&self) -> PointF {
        for element in self.elements.iter().rev() {
            match element {
                PathElement::MoveTo(point) | PathElement::LineTo(point) => return *point,
                PathElement::CubicTo(_, _, end) => return *end,
                PathElement::Close => return self.subpath_start(),
                PathElement::Text { .. } => continue,
            }
        }
        PointF::default()
    }

    /// Where the last subpath began.
    fn subpath_start(&self) -> PointF {
        self.elements
            .iter()
            .rev()
            .find_map(|element| match element {
                PathElement::MoveTo(point) => Some(*point),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// The start of a line or curve: an empty path begins at the origin, as QPainterPath's
    /// ensureData makes it; after a closed shape, a new subpath where the last one ended.
    fn begin_segment(&mut self) {
        if self.elements.is_empty() {
            self.elements.push(PathElement::MoveTo(PointF::default()));
        } else if self.require_move_to {
            let at = self.current_position();
            self.elements.push(PathElement::MoveTo(at));
        }
        self.require_move_to = false;
    }

    pub fn move_to(&mut self, point: PointF) {
        self.require_move_to = false;
        if let Some(PathElement::MoveTo(last)) = self.elements.last_mut() {
            *last = point;
            return;
        }
        self.elements.push(PathElement::MoveTo(point));
    }

    /// QPainterPath::lineTo; a line to the point the path is already at adds nothing, as Qt's
    /// does not.
    pub fn line_to(&mut self, point: PointF) {
        self.begin_segment();
        if self.current_position() == point {
            return;
        }
        self.elements.push(PathElement::LineTo(point));
    }

    pub fn cubic_to(&mut self, c1: PointF, c2: PointF, end: PointF) {
        self.begin_segment();
        self.elements.push(PathElement::CubicTo(c1, c2, end));
    }

    /// QPainterPath::quadTo, as the cubic Qt turns it into.
    pub fn quad_to(&mut self, control: PointF, end: PointF) {
        self.begin_segment();
        let start = self.current_position();
        let c1 = start + (control - start) * (2.0 / 3.0);
        let c2 = end + (control - end) * (2.0 / 3.0);
        self.elements.push(PathElement::CubicTo(c1, c2, end));
    }

    /// QPainterPath::closeSubpath: a line back to the subpath's start, and the subpath closed.
    pub fn close_subpath(&mut self) {
        if self.elements.is_empty() || self.require_move_to {
            return;
        }
        let start = self.subpath_start();
        if self.current_position() != start {
            self.elements.push(PathElement::LineTo(start));
        }
        self.elements.push(PathElement::Close);
        self.require_move_to = true;
    }

    /// QPainterPath::addRect: a closed subpath round the rectangle from its top left, clockwise on
    /// a screen whose y runs down.
    pub fn add_rect(&mut self, rect: RectF) {
        self.move_to(rect.top_left());
        self.elements.push(PathElement::LineTo(rect.top_right()));
        self.elements.push(PathElement::LineTo(rect.bottom_right()));
        self.elements.push(PathElement::LineTo(rect.bottom_left()));
        self.elements.push(PathElement::LineTo(rect.top_left()));
        self.elements.push(PathElement::Close);
        self.require_move_to = true;
    }

    /// QPainterPath::addPolygon: an open subpath through the points.
    pub fn add_polygon(&mut self, points: &[PointF]) {
        let Some((first, rest)) = points.split_first() else {
            return;
        };
        self.move_to(*first);
        for point in rest {
            self.elements.push(PathElement::LineTo(*point));
        }
    }

    /// QPainterPath::addEllipse: four quarter curves from the rightmost point, the first towards
    /// the bottom (Qt's qt_curves_for_arc with a sweep of -360 degrees).
    pub fn add_ellipse(&mut self, rect: RectF) {
        if rect.is_null() {
            return;
        }
        let x = rect.x;
        let y = rect.y;
        let w = rect.width;
        let w2 = rect.width / 2.0;
        let w2k = w2 * PATH_KAPPA;
        let h = rect.height;
        let h2 = rect.height / 2.0;
        let h2k = h2 * PATH_KAPPA;
        let p = PointF::new;
        self.move_to(p(x + w, y + h2));
        let quarters = [
            // 0 -> 270 degrees
            (
                p(x + w, y + h2 + h2k),
                p(x + w2 + w2k, y + h),
                p(x + w2, y + h),
            ),
            // 270 -> 180 degrees
            (p(x + w2 - w2k, y + h), p(x, y + h2 + h2k), p(x, y + h2)),
            // 180 -> 90 degrees
            (p(x, y + h2 - h2k), p(x + w2 - w2k, y), p(x + w2, y)),
            // 90 -> 0 degrees
            (p(x + w2 + w2k, y), p(x + w, y + h2 - h2k), p(x + w, y + h2)),
        ];
        for (c1, c2, end) in quarters {
            self.elements.push(PathElement::CubicTo(c1, c2, end));
        }
        self.elements.push(PathElement::Close);
        self.require_move_to = true;
    }

    /// The point of the ellipse inscribed in `rect` at `degrees`, counter-clockwise from three
    /// o'clock as Qt measures arcs on a screen whose y runs down.
    pub fn ellipse_point(rect: RectF, degrees: f64) -> PointF {
        let radians = DEG_TO_RAD * degrees;
        let center = rect.center();
        PointF::new(
            center.x + rect.width / 2.0 * radians.cos(),
            center.y - rect.height / 2.0 * radians.sin(),
        )
    }

    /// QPainterPath::arcMoveTo: a new subpath at the point of the ellipse at `degrees`.
    pub fn arc_move_to(&mut self, rect: RectF, degrees: f64) {
        if rect.is_null() {
            return;
        }
        self.move_to(Self::ellipse_point(rect, degrees));
    }

    /// QPainterPath::arcTo: a line to the arc's start, then the arc of the ellipse inscribed in
    /// `rect` from `start_degrees` through `sweep_degrees` (positive counter-clockwise), in cubic
    /// pieces of at most a quarter turn. Qt's own pieces split its quarter-ellipse curves where the
    /// arc starts and ends; these are each fitted to their own angle (k = 4/3 tan(a/4)), which
    /// lies within a few millionths of the radius of Qt's.
    pub fn arc_to(&mut self, rect: RectF, start_degrees: f64, sweep_degrees: f64) {
        if rect.is_null() {
            return;
        }
        let sweep = sweep_degrees.clamp(-360.0, 360.0);
        let start = Self::ellipse_point(rect, start_degrees);
        self.line_to(start);
        if sweep == 0.0 {
            return;
        }
        let pieces = (sweep.abs() / 90.0).ceil().max(1.0);
        let step = sweep / pieces;
        let rx = rect.width / 2.0;
        let ry = rect.height / 2.0;
        let center = rect.center();
        // The cubic for a circular arc of `step` degrees, scaled to the ellipse.
        let k = 4.0 / 3.0 * (DEG_TO_RAD * step / 4.0).tan();
        let mut angle = start_degrees;
        // `pieces` is at most four: the sweep was clamped to a turn.
        for _ in 0..(pieces as u32) {
            let a0 = DEG_TO_RAD * angle;
            let a1 = DEG_TO_RAD * (angle + step);
            let (s0, c0) = a0.sin_cos();
            let (s1, c1) = a1.sin_cos();
            let p0 = PointF::new(center.x + rx * c0, center.y - ry * s0);
            let p1 = PointF::new(center.x + rx * c1, center.y - ry * s1);
            let control1 = PointF::new(p0.x - k * rx * s0, p0.y - k * ry * c0);
            let control2 = PointF::new(p1.x + k * rx * s1, p1.y + k * ry * c1);
            self.elements
                .push(PathElement::CubicTo(control1, control2, p1));
            angle += step;
        }
    }

    /// QPainterPath::addRoundedRect with Qt::AbsoluteSize: corners of radius `x_radius` across and
    /// `y_radius` down, each no more than half the side; a plain rectangle when either is none.
    pub fn add_rounded_rect(&mut self, rect: RectF, x_radius: f64, y_radius: f64) {
        let r = rect.normalized();
        if r.is_null() {
            return;
        }
        let half_w = r.width / 2.0;
        let half_h = r.height / 2.0;
        let x_percent = if half_w == 0.0 {
            0.0
        } else {
            100.0 * katana_math::cpp::min(x_radius, half_w) / half_w
        };
        let y_percent = if half_h == 0.0 {
            0.0
        } else {
            100.0 * katana_math::cpp::min(y_radius, half_h) / half_h
        };
        if x_percent <= 0.0 || y_percent <= 0.0 {
            self.add_rect(r);
            return;
        }
        let (x, y, w, h) = (r.x, r.y, r.width, r.height);
        let rxx2 = w * x_percent / 100.0;
        let ryy2 = h * y_percent / 100.0;
        self.arc_move_to(RectF::new(x, y, rxx2, ryy2), 180.0);
        self.arc_to(RectF::new(x, y, rxx2, ryy2), 180.0, -90.0);
        self.arc_to(RectF::new(x + w - rxx2, y, rxx2, ryy2), 90.0, -90.0);
        self.arc_to(
            RectF::new(x + w - rxx2, y + h - ryy2, rxx2, ryy2),
            0.0,
            -90.0,
        );
        self.arc_to(RectF::new(x, y + h - ryy2, rxx2, ryy2), 270.0, -90.0);
        self.close_subpath();
    }

    /// QPainterPath::addText: the text's outlines, its baseline from `origin`.
    pub fn add_text(&mut self, origin: PointF, font: &Font, text: &str) {
        self.elements.push(PathElement::Text {
            origin,
            font: font.clone(),
            text: String::from(text),
        });
        self.require_move_to = true;
    }

    /// QPainterPath::translated.
    pub fn translated(&self, dx: f64, dy: f64) -> Self {
        let offset = PointF::new(dx, dy);
        let mut out = self.clone();
        for element in &mut out.elements {
            match element {
                PathElement::MoveTo(point) | PathElement::LineTo(point) => *point += offset,
                PathElement::CubicTo(c1, c2, end) => {
                    *c1 += offset;
                    *c2 += offset;
                    *end += offset;
                }
                PathElement::Close => {}
                PathElement::Text { origin, .. } => *origin += offset,
            }
        }
        out
    }

    /// QPainterPath::controlPointRect: the bounds of every point and control point (a text's
    /// origin only).
    pub fn control_point_rect(&self) -> RectF {
        let mut points = Vec::new();
        for element in &self.elements {
            match element {
                PathElement::MoveTo(point) | PathElement::LineTo(point) => points.push(*point),
                PathElement::CubicTo(c1, c2, end) => points.extend([*c1, *c2, *end]),
                PathElement::Close => {}
                PathElement::Text { origin, .. } => points.push(*origin),
            }
        }
        let Some(first) = points.first() else {
            return RectF::default();
        };
        let (mut left, mut top, mut right, mut bottom) = (first.x, first.y, first.x, first.y);
        for point in &points[1..] {
            left = katana_math::cpp::min(left, point.x);
            top = katana_math::cpp::min(top, point.y);
            right = katana_math::cpp::max(right, point.x);
            bottom = katana_math::cpp::max(bottom, point.y);
        }
        RectF::new(left, top, right - left, bottom - top)
    }
}

// ---- the canvas --------------------------------------------------------------------------------

/// QPainter::RenderHint, the three the painters set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderHint {
    Antialiasing,
    TextAntialiasing,
    SmoothPixmapTransform,
}

/// The painter's surface: what QPainter does for the window's painters, over a raster image or a
/// PDF page. State is QPainter's - a pen, a brush, a font, the render hints, an opacity, a world
/// transform and a clip, saved and restored together - and every shape is drawn with the pen and
/// filled with the brush as QPainter draws it.
///
/// The backends implement the required methods; the shapes are provided as paths, which a
/// backend may override where it has a faster way to the same pixels.
pub trait Canvas {
    /// The painted device's size in its own pixels (QPaintDevice::width and height).
    fn device_size(&self) -> SizeF;
    /// How many device pixels make a logical one (QPaintDevice::devicePixelRatio): 1.25 on a
    /// screen at 125 %, the image's own ratio for an image.
    fn device_pixel_ratio(&self) -> f64;

    /// QPainter::save: the whole state, onto a stack.
    fn save(&mut self);
    /// QPainter::restore: the state saved last; nothing when nothing was saved.
    fn restore(&mut self);

    fn pen(&self) -> Pen;
    fn set_pen(&mut self, pen: Pen);
    fn brush(&self) -> Brush;
    fn set_brush(&mut self, brush: Brush);
    fn font(&self) -> Font;
    fn set_font(&mut self, font: Font);
    fn render_hint(&self, hint: RenderHint) -> bool;
    fn set_render_hint(&mut self, hint: RenderHint, on: bool);
    fn opacity(&self) -> f64;
    /// QPainter::setOpacity: 0 to 1, multiplying everything drawn after.
    fn set_opacity(&mut self, opacity: f64);

    /// QPainter::worldTransform (and transform()).
    fn world_transform(&self) -> Transform;
    /// QPainter::setWorldTransform: in place of the transform, or after it when `combine`.
    fn set_world_transform(&mut self, transform: Transform, combine: bool);
    /// QPainter::deviceTransform: the world transform followed by the device's own scale.
    fn device_transform(&self) -> Transform;

    /// QPainter::setClipRect with Qt::ReplaceClip, or Qt::IntersectClip when `intersect`; in the
    /// coordinates the world transform maps.
    fn set_clip_rect(&mut self, rect: RectF, intersect: bool);
    /// QPainter::setClipPath, as setClipRect.
    fn set_clip_path(&mut self, path: &Path, intersect: bool);
    /// QPainter::setClipping: false paints everywhere again, keeping the clip for a later true.
    fn set_clipping(&mut self, on: bool);
    fn has_clipping(&self) -> bool;

    /// QPainter::drawPath: filled with the brush, then stroked with the pen.
    fn draw_path(&mut self, path: &Path);
    /// QPainter::fillPath: filled with `brush` alone.
    fn fill_path(&mut self, path: &Path, brush: &Brush);
    /// QPainter::strokePath: stroked with `pen` alone.
    fn stroke_path(&mut self, path: &Path, pen: &Pen);

    /// QPainter::drawText(point, text): the baseline of the text's first line at `origin`, in the
    /// pen's colour and the font.
    fn draw_text(&mut self, origin: PointF, text: &str);
    /// QPainter::drawText(rect, flags, text): laid out in `rect` as `flags` say. Answers the
    /// rectangle the text took (drawText's boundingRect).
    fn draw_text_in(&mut self, rect: RectF, flags: TextFlags, text: &str) -> RectF;

    /// QPainter::drawImage(target, image, source): the `source` pixels of `image` scaled into
    /// `target`, smoothly when SmoothPixmapTransform is set.
    fn draw_image(&mut self, target: RectF, image: &Image, source: IntRect);

    /// The metrics of the fonts this canvas draws with (QPainter::fontMetrics and QFontMetricsF).
    fn metrics(&self) -> &dyn FontMetrics;

    // ---- provided: transforms ----

    /// QPainter::translate.
    fn translate(&mut self, dx: f64, dy: f64) {
        let transform = self.world_transform().translated(dx, dy);
        self.set_world_transform(transform, false);
    }

    /// QPainter::scale.
    fn scale(&mut self, sx: f64, sy: f64) {
        let transform = self.world_transform().scaled(sx, sy);
        self.set_world_transform(transform, false);
    }

    /// QPainter::rotate, in degrees.
    fn rotate(&mut self, degrees: f64) {
        let transform = self.world_transform().rotated(degrees);
        self.set_world_transform(transform, false);
    }

    /// QPainter::shear.
    fn shear(&mut self, sh: f64, sv: f64) {
        let transform = self.world_transform().sheared(sh, sv);
        self.set_world_transform(transform, false);
    }

    /// QPainter::setTransform (the world transform's other name).
    fn set_transform(&mut self, transform: Transform) {
        self.set_world_transform(transform, false);
    }

    /// QPainter::resetTransform.
    fn reset_transform(&mut self) {
        self.set_world_transform(Transform::IDENTITY, false);
    }

    // ---- provided: shapes ----

    /// QPainter::drawLine: stroked with the pen.
    fn draw_line(&mut self, from: PointF, to: PointF) {
        let mut path = Path::new();
        path.move_to(from);
        path.line_to(to);
        let pen = self.pen();
        self.stroke_path(&path, &pen);
    }

    /// QPainter::drawLines.
    fn draw_lines(&mut self, lines: &[(PointF, PointF)]) {
        for &(from, to) in lines {
            self.draw_line(from, to);
        }
    }

    /// QPainter::drawPolyline: open, stroked with the pen.
    fn draw_polyline(&mut self, points: &[PointF]) {
        if points.len() < 2 {
            return;
        }
        let mut path = Path::new();
        path.add_polygon(points);
        let pen = self.pen();
        self.stroke_path(&path, &pen);
    }

    /// QPainter::drawPolygon: closed, filled with the brush by `rule` and stroked with the pen.
    fn draw_polygon(&mut self, points: &[PointF], rule: FillRule) {
        if points.is_empty() {
            return;
        }
        let mut path = Path::new();
        path.fill_rule = rule;
        path.add_polygon(points);
        path.close_subpath();
        self.draw_path(&path);
    }

    /// QPainter::drawRect.
    fn draw_rect(&mut self, rect: RectF) {
        let mut path = Path::new();
        path.add_rect(rect);
        self.draw_path(&path);
    }

    /// QPainter::drawRects.
    fn draw_rects(&mut self, rects: &[RectF]) {
        for rect in rects {
            self.draw_rect(*rect);
        }
    }

    /// QPainter::fillRect: filled with `brush`, no outline.
    fn fill_rect(&mut self, rect: RectF, brush: &Brush) {
        let mut path = Path::new();
        path.add_rect(rect);
        self.fill_path(&path, brush);
    }

    /// QPainter::drawRoundedRect with Qt::AbsoluteSize.
    fn draw_rounded_rect(&mut self, rect: RectF, x_radius: f64, y_radius: f64) {
        let mut path = Path::new();
        path.add_rounded_rect(rect, x_radius, y_radius);
        self.draw_path(&path);
    }

    /// QPainter::drawEllipse(rect).
    fn draw_ellipse(&mut self, rect: RectF) {
        let mut path = Path::new();
        path.add_ellipse(rect);
        self.draw_path(&path);
    }

    /// QPainter::drawPoint: a dot of the pen's width (one pixel for a hairline), round for a
    /// round cap and square otherwise, in the pen's colour.
    fn draw_point(&mut self, at: PointF) {
        let pen = self.pen();
        if pen.is_none() {
            return;
        }
        let width = if pen.width > 0.0 { pen.width } else { 1.0 };
        let square = RectF::new(at.x - width / 2.0, at.y - width / 2.0, width, width);
        let mut path = Path::new();
        if pen.cap == PenCap::Round {
            path.add_ellipse(square);
        } else {
            path.add_rect(square);
        }
        self.fill_path(&path, &Brush::solid(pen.color));
    }

    /// QPainter::drawPoints.
    fn draw_points(&mut self, points: &[PointF]) {
        for point in points {
            self.draw_point(*point);
        }
    }

    /// QPainter::drawImage(point, image): the whole image, a device pixel of it a logical one
    /// divided by its ratio, its top left at `at`.
    fn draw_image_at(&mut self, at: PointF, image: &Image) {
        let ratio = image.device_pixel_ratio();
        let target = RectF::new(
            at.x,
            at.y,
            f64::from(image.width()) / ratio,
            f64::from(image.height()) / ratio,
        );
        let source = IntRect::new(0, 0, image.width() as i32, image.height() as i32);
        self.draw_image(target, image, source);
    }
}

#[cfg(test)]
mod tests {
    use super::{Color, Path, PathElement, PointF, RectF, Transform};

    // Qt's composition: translate then scale maps the origin to the translation scaled.
    #[test]
    fn transforms_compose_as_qt_composes_them() {
        let t = Transform::IDENTITY.translated(10.0, 20.0).scaled(2.0, 3.0);
        assert_eq!(t.map(PointF::new(1.0, 1.0)), PointF::new(12.0, 23.0));
        let quarter = Transform::IDENTITY.rotated(90.0);
        assert_eq!(quarter.map(PointF::new(1.0, 0.0)), PointF::new(0.0, 1.0));
        let both = Transform::from_translate(5.0, 0.0) * Transform::from_scale(2.0, 2.0);
        assert_eq!(both.map(PointF::new(1.0, 1.0)), PointF::new(12.0, 2.0));
        let back = both.inverted().expect("a scale and a move invert");
        assert_eq!(back.map(PointF::new(12.0, 2.0)), PointF::new(1.0, 1.0));
    }

    // A rectangle is five points and a close, from the top left (QPainterPath::addRect), and a
    // line after it starts a subpath where the rectangle ended.
    #[test]
    fn a_rectangle_is_a_closed_subpath_and_what_follows_starts_anew() {
        let mut path = Path::new();
        path.add_rect(RectF::new(0.0, 0.0, 2.0, 1.0));
        path.line_to(PointF::new(5.0, 5.0));
        assert_eq!(path.elements.len(), 8);
        assert_eq!(path.elements[5], PathElement::Close);
        assert_eq!(path.elements[6], PathElement::MoveTo(PointF::new(0.0, 0.0)));
    }

    #[test]
    fn a_colours_name_is_lower_case_hex_and_reads_back() {
        let colour = Color::rgb(0x4c, 0x9f, 0xfe);
        assert_eq!(colour.name(), "#4c9ffe");
        assert_eq!(Color::from_name("#4C9FFE"), Some(colour));
        assert_eq!(Color::from_name("#fff"), Some(Color::WHITE));
        assert_eq!(Color::from_name("orange"), None);
    }
}
