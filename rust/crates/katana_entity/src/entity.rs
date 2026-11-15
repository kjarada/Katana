//! The domain-independent entity model.
//!
//! An Entity is a plain value: identity + geometry + presentation attributes +
//! open-ended properties. It knows nothing about rendering, storage or
//! surveying. The renderer consumes entities; it never owns them.
//!
//! Every string that reaches the model is UTF-8: the C++ checked that with
//! `isValidUtf8` because its JSON writer throws on anything else, from a path
//! that promises not to throw. Here a `String` cannot hold anything else, so
//! the bytes are checked where they are decoded (the blob and JSON readers)
//! and nowhere after.

use std::collections::BTreeMap;

use katana_core::error::{ErrorCode, Result, make_error};
use katana_core::text::{
    equals_ignoring_case, format_exact_real, is_ascii_space, parse_finite_double,
};
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, Ellipse2, Point2, Polyline2, Segment2, Spline2, Vec2,
};
use katana_math::numerics::TWO_PI;

/// Named here as well as in core, because the model is where it is enforced
/// (on the bytes a reader is handed).
pub use katana_core::text_encoding::is_valid_utf8;

pub type EntityId = u64;
pub const INVALID_ENTITY_ID: EntityId = 0;

/// The text a C++ `toString` overload gives an enumeration. One trait for all
/// of them, so that `to_string(kind)` reads as the C++ does whichever
/// enumeration `kind` is.
pub trait EnumName: Copy {
    fn name(self) -> &'static str;
}

/// The C++ `toString` overload set for the enumerations.
pub fn to_string<T: EnumName>(value: T) -> &'static str {
    value.name()
}

/// Order matches the alternatives of Geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum EntityType {
    Point,
    Line,
    Arc,
    Polyline,
    Circle,
    Text,
    Dimension,
    Label,
    Leader,
    // The drawing system's kinds (docs/drawing.md), appended after the
    // annotation system's Label and Leader so the kind bytes never collide.
    CurvePolyline,
    Ellipse,
    Spline,
}

/// The number of alternatives of Geometry. ADDING A GEOMETRY KIND? It is
/// append-only: the variant index is the on-disk kind byte (geometry_blob.rs
/// writes `geometry.index()`), so reordering silently reinterprets every
/// project ever saved. docs/model.md lists every place that must change.
pub const GEOMETRY_KIND_COUNT: usize = 12;

/// Every EntityType, in the order of the variant.
pub const ENTITY_TYPES: [EntityType; GEOMETRY_KIND_COUNT] = [
    EntityType::Point,
    EntityType::Line,
    EntityType::Arc,
    EntityType::Polyline,
    EntityType::Circle,
    EntityType::Text,
    EntityType::Dimension,
    EntityType::Label,
    EntityType::Leader,
    EntityType::CurvePolyline,
    EntityType::Ellipse,
    EntityType::Spline,
];

impl EnumName for EntityType {
    fn name(self) -> &'static str {
        match self {
            Self::Point => "Point",
            Self::Line => "Line",
            Self::Arc => "Arc",
            Self::Polyline => "Polyline",
            Self::Circle => "Circle",
            Self::Text => "Text",
            Self::Dimension => "Dimension",
            Self::Label => "Label",
            Self::Leader => "Leader",
            Self::CurvePolyline => "CurvePolyline",
            Self::Ellipse => "Ellipse",
            Self::Spline => "Spline",
        }
    }
}

/// The kind named by to_string's name, in any case ("curvepolyline", "LINE").
pub fn entity_type_from_string(name: &str) -> Result<EntityType> {
    // Derived from the variant rather than naming the last enumerator: a kind
    // appended after Dimension was previously unreachable here, which made it
    // unparseable by name.
    for entity_type in ENTITY_TYPES {
        // In any case: callers once title-cased the name to the enumerator's
        // spelling, which turned "curvepolyline" into "Curvepolyline" and
        // refused the one kind with a capital inside its name.
        if equals_ignoring_case(to_string(entity_type).as_bytes(), name.as_bytes()) {
            return Ok(entity_type);
        }
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        "unknown entity type",
        name,
    ))
}

/// Case-insensitive ASCII comparison, ignoring '-', '_' and spaces, so that
/// "middle-centre", "MiddleCentre" and "middle centre" are one name.
pub(crate) fn same_name(first: &str, second: &str) -> bool {
    fn next(text: &[u8], at: &mut usize) -> i32 {
        while *at < text.len() && matches!(text[*at], b'-' | b'_' | b' ') {
            *at += 1;
        }
        if *at == text.len() {
            return -1;
        }
        let character = text[*at].to_ascii_lowercase();
        *at += 1;
        i32::from(character)
    }
    let (first, second) = (first.as_bytes(), second.as_bytes());
    let mut i = 0;
    let mut j = 0;
    loop {
        let x = next(first, &mut i);
        let y = next(second, &mut j);
        if x != y {
            return false;
        }
        if x < 0 {
            return true;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointGeometry {
    pub position: Point2,
}

/// How the arrow at the end of a dimension line or a leader is drawn. Declared
/// here rather than beside DimensionStyle (tables.rs) because a leader
/// carries its own; to_string and arrow_head_from_string are still in
/// tables.rs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ArrowHead {
    None = 0,
    Tick = 1,
    ClosedFilled = 2,
    Open = 3,
    Dot = 4,
}

/// Where a text's `position` sits on its block of lines: a row (bottom,
/// middle, top) and a column (left, centre, right). The bottom row is the
/// BASELINE of the last line, so BottomLeft is exactly the "left end of the
/// baseline" every text had before justification existed, and is the default.
///
/// The enumerator values are stored (geometry_blob.rs, serialization.rs by
/// name), so they are append-only like the geometry kinds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TextJustify {
    #[default]
    BottomLeft = 0,
    BottomCentre = 1,
    BottomRight = 2,
    MiddleLeft = 3,
    MiddleCentre = 4,
    MiddleRight = 5,
    TopLeft = 6,
    TopCentre = 7,
    TopRight = 8,
}

impl EnumName for TextJustify {
    fn name(self) -> &'static str {
        match self {
            Self::BottomLeft => "BL",
            Self::BottomCentre => "BC",
            Self::BottomRight => "BR",
            Self::MiddleLeft => "ML",
            Self::MiddleCentre => "MC",
            Self::MiddleRight => "MR",
            Self::TopLeft => "TL",
            Self::TopCentre => "TC",
            Self::TopRight => "TR",
        }
    }
}

/// "BL" ... "TR"; also the long forms ("MiddleCentre", "middle-center") and
/// "L", "C", "R" for the baseline row, which is how a person coming from
/// another CAD package writes them.
pub fn text_justify_from_string(name: &str) -> Result<TextJustify> {
    // The short names, the long ones in both spellings of centre, and the
    // baseline row's one-letter forms.
    const NAMES: [(&str, TextJustify); 25] = [
        ("BL", TextJustify::BottomLeft),
        ("BC", TextJustify::BottomCentre),
        ("BR", TextJustify::BottomRight),
        ("ML", TextJustify::MiddleLeft),
        ("MC", TextJustify::MiddleCentre),
        ("MR", TextJustify::MiddleRight),
        ("TL", TextJustify::TopLeft),
        ("TC", TextJustify::TopCentre),
        ("TR", TextJustify::TopRight),
        ("L", TextJustify::BottomLeft),
        ("C", TextJustify::BottomCentre),
        ("R", TextJustify::BottomRight),
        ("M", TextJustify::MiddleCentre),
        ("BottomLeft", TextJustify::BottomLeft),
        ("BottomCentre", TextJustify::BottomCentre),
        ("BottomCenter", TextJustify::BottomCentre),
        ("BottomRight", TextJustify::BottomRight),
        ("MiddleLeft", TextJustify::MiddleLeft),
        ("MiddleCentre", TextJustify::MiddleCentre),
        ("MiddleCenter", TextJustify::MiddleCentre),
        ("MiddleRight", TextJustify::MiddleRight),
        ("TopLeft", TextJustify::TopLeft),
        ("TopCentre", TextJustify::TopCentre),
        ("TopCenter", TextJustify::TopCentre),
        ("TopRight", TextJustify::TopRight),
    ];
    for (text, justify) in NAMES {
        if same_name(text, name) {
            return Ok(justify);
        }
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        "unknown justification (TL TC TR ML MC MR BL BC BR)",
        name,
    ))
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextGeometry {
    /// The justification point (BottomLeft: left end of the baseline).
    pub position: Point2,
    /// One or more lines, separated by '\n'. The first line is on top.
    pub text: String,
    /// MODEL units. For a paper-sized text this is the height it had at the
    /// annotation scale it was made or last scaled at: what the bounding box,
    /// picking and the spatial index go by.
    pub height: f64,
    /// Radians, counter-clockwise.
    pub rotation: f64,
    // Appended 2026-09-25, each defaulting to what a text written before then
    // was. A text with every one of them at its default is stored exactly as
    // before (geometry_blob.rs), so an older build still opens it.
    /// The text style it is set in; empty for none.
    pub style: String,
    /// Millimetres on paper; > 0 makes the text paper-sized. 0 takes the
    /// style's paper height, and with no style the text is a model-unit text.
    pub paper_height: f64,
    pub justify: TextJustify,
}

impl Default for TextGeometry {
    fn default() -> Self {
        Self {
            position: Point2::default(),
            text: String::new(),
            height: 2.5,
            rotation: 0.0,
            style: String::new(),
            paper_height: 0.0,
            justify: TextJustify::BottomLeft,
        }
    }
}

impl TextGeometry {
    /// The four members a text had before the annotation members, the others
    /// at their defaults: the C++ aggregate `TextGeometry{p, text, h, r}`.
    pub fn new(position: Point2, text: impl Into<String>, height: f64, rotation: f64) -> Self {
        Self {
            position,
            text: text.into(),
            height,
            rotation,
            ..Self::default()
        }
    }
}

/// A point on ANOTHER entity, named rather than copied, so that a dimension or
/// a leader that refers to it follows it when it moves. `entity` 0 is no
/// reference.
///
/// Append-only: the value is stored. Along and Inside were added on 2026-09-25
/// with smart leaders and are written in geometry blob version 3 only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AnchorPoint {
    /// A point's position, a text's insertion point.
    #[default]
    Position = 0,
    /// A line's or an arc's start, a polyline's first vertex.
    Start = 1,
    /// A line's or an arc's end, a polyline's last vertex.
    End = 2,
    /// A line's or an arc's midpoint.
    Mid = 3,
    /// An arc's or a circle's centre.
    Centre = 4,
    /// A polyline's vertex `index`.
    Vertex = 5,
    /// The midpoint of a polyline's segment `index`.
    SegmentMid = 6,
    /// ON the entity, `parameter` of the way along.
    Along = 7,
    /// Inside a closed figure.
    Inside = 8,
}

/// The last enumerator, for the checks that refuse a stored value past it.
pub const LAST_ANCHOR_POINT: AnchorPoint = AnchorPoint::Inside;

impl EnumName for AnchorPoint {
    fn name(self) -> &'static str {
        match self {
            Self::Position => "position",
            Self::Start => "start",
            Self::End => "end",
            Self::Mid => "mid",
            Self::Centre => "centre",
            Self::Vertex => "vertex",
            Self::SegmentMid => "segment-mid",
            Self::Along => "along",
            Self::Inside => "inside",
        }
    }
}

pub fn anchor_point_from_string(name: &str) -> Result<AnchorPoint> {
    for point in [
        AnchorPoint::Position,
        AnchorPoint::Start,
        AnchorPoint::End,
        AnchorPoint::Mid,
        AnchorPoint::Centre,
        AnchorPoint::Vertex,
        AnchorPoint::SegmentMid,
        AnchorPoint::Along,
        AnchorPoint::Inside,
    ] {
        if same_name(to_string(point), name) {
            return Ok(point);
        }
    }
    if same_name(name, "center") {
        return Ok(AnchorPoint::Centre);
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        "unknown anchor point",
        name,
    ))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnchorRef {
    /// An EntityId; 0 means "not associated".
    pub entity: u64,
    pub point: AnchorPoint,
    pub index: u32,
    /// Along's fraction, in [0, 1]; 0 for every other point (validate).
    pub parameter: f64,
}

impl AnchorRef {
    pub const fn associated(&self) -> bool {
        self.entity != 0
    }
}

/// What a dimension measures. Aligned is the only kind there was before
/// 2026-09-25, and the default. Append-only: the value is stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DimensionKind {
    #[default]
    Aligned = 0,
    Linear = 1,
    Angular = 2,
    Radius = 3,
    Diameter = 4,
    OrdinateX = 5,
    OrdinateY = 6,
}

impl EnumName for DimensionKind {
    fn name(self) -> &'static str {
        match self {
            Self::Aligned => "aligned",
            Self::Linear => "linear",
            Self::Angular => "angular",
            Self::Radius => "radius",
            Self::Diameter => "diameter",
            Self::OrdinateX => "ordinate-x",
            Self::OrdinateY => "ordinate-y",
        }
    }
}

pub fn dimension_kind_from_string(name: &str) -> Result<DimensionKind> {
    for kind in [
        DimensionKind::Aligned,
        DimensionKind::Linear,
        DimensionKind::Angular,
        DimensionKind::Radius,
        DimensionKind::Diameter,
        DimensionKind::OrdinateX,
        DimensionKind::OrdinateY,
    ] {
        if same_name(to_string(kind), name) {
            return Ok(kind);
        }
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        "unknown dimension kind",
        name,
    ))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimensionGeometry {
    pub start: Point2,
    pub end: Point2,
    pub offset: f64,
    /// Empty: show the measured value.
    pub text_override: String,
    // Appended 2026-09-25 with the kinds; a dimension with every one of them
    // at its default is stored exactly as before (geometry_blob.rs).
    pub kind: DimensionKind,
    /// The direction a Linear or ordinate dimension measures along, radians
    /// counter-clockwise from east.
    pub angle: f64,
    /// The apex of an angular dimension, the centre of a radial one, the
    /// datum of an ordinate one; unused by Aligned and Linear.
    pub vertex: Point2,
    /// The entities `start`, `end` and `vertex` follow.
    pub start_ref: AnchorRef,
    pub end_ref: AnchorRef,
    pub vertex_ref: AnchorRef,
}

impl DimensionGeometry {
    /// The four members a dimension had before the kinds, the others at their
    /// defaults: the C++ aggregate `DimensionGeometry{start, end, offset, text}`.
    pub fn new(start: Point2, end: Point2, offset: f64, text_override: impl Into<String>) -> Self {
        Self {
            start,
            end,
            offset,
            text_override: text_override.into(),
            ..Self::default()
        }
    }

    /// The measured value: a length in model units, or for Angular an angle in
    /// radians in [0, 2 pi). Signed for the ordinate kinds.
    pub fn measurement(&self) -> f64 {
        let along = Vec2::new(self.angle.cos(), self.angle.sin());
        match self.kind {
            DimensionKind::Aligned => self.start.distance_to(self.end),
            DimensionKind::Linear => (self.end - self.start).dot(along).abs(),
            DimensionKind::Angular => {
                let first = self.start - self.vertex;
                let second = self.end - self.vertex;
                let mut swept = first.cross(second).atan2(first.dot(second));
                if swept < 0.0 {
                    swept += TWO_PI;
                }
                swept
            }
            DimensionKind::Radius => self.start.distance_to(self.vertex),
            DimensionKind::Diameter => 2.0 * self.start.distance_to(self.vertex),
            DimensionKind::OrdinateX => (self.start - self.vertex).dot(along),
            DimensionKind::OrdinateY => (self.start - self.vertex).dot(along.perpendicular()),
        }
    }

    /// Whether `vertex` is one of the dimension's points.
    pub fn uses_vertex(&self) -> bool {
        self.kind != DimensionKind::Aligned && self.kind != DimensionKind::Linear
    }
}

/// A label that says something about another entity or an alignment through
/// a label style's template. It stores WHICH thing it labels and HOW, never
/// the text: the text is worked out from the target every time it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelGeometry {
    /// The EntityId labelled; 0 when `alignment` names the target.
    pub target: u64,
    /// A chainage label's alignment.
    pub alignment: String,
    /// Which piece of the target: a polyline's segment, -1 for every one.
    pub part: i32,
    /// The label style (required).
    pub style: String,
    /// Where the label attaches, kept up to date by the associative update.
    pub anchor: Point2,
    /// Dragged by hand: the text is put HERE.
    pub position: Option<Point2>,
    /// Replaces the template's text, verbatim, when not empty.
    pub text_override: String,
    /// The auto-label rule that made it; empty for one placed by hand.
    pub rule: String,
}

impl Default for LabelGeometry {
    fn default() -> Self {
        Self {
            target: 0,
            alignment: String::new(),
            part: -1,
            style: String::new(),
            anchor: Point2::default(),
            position: None,
            text_override: String::new(),
            rule: String::new(),
        }
    }
}

/// How a leader's note is framed. Append-only: the value is stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CalloutShape {
    #[default]
    None = 0,
    Box = 1,
    Circle = 2,
}

impl EnumName for CalloutShape {
    fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Box => "box",
            Self::Circle => "circle",
        }
    }
}

pub fn callout_shape_from_string(name: &str) -> Result<CalloutShape> {
    for shape in [CalloutShape::None, CalloutShape::Box, CalloutShape::Circle] {
        if same_name(to_string(shape), name) {
            return Ok(shape);
        }
    }
    if same_name(name, "balloon") {
        return Ok(CalloutShape::Circle);
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        "unknown callout shape (none box circle)",
        name,
    ))
}

/// A leader: an arrow from a feature through any number of bends to a note.
/// Sizes are PAPER millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct LeaderGeometry {
    /// The arrow's tip first, then each bend. At least two.
    pub vertices: Vec<Point2>,
    /// Lines separated by '\n'; empty for a bare leader.
    pub text: String,
    pub arrow: ArrowHead,
    pub callout: CalloutShape,
    /// The text style; empty for the default one.
    pub style: String,
    /// mm; 0 takes the style's (2.5 mm when it has none).
    pub paper_height: f64,
    /// mm on paper.
    pub arrow_size: f64,
    /// mm on paper; 0 for none.
    pub landing: f64,
    /// The entity the tip follows.
    pub tip_ref: AnchorRef,
    /// Appended with smart leaders: `text` is a TEMPLATE whose fields are the
    /// values of the tip's entity at the tip (leader_values.rs).
    pub fields: bool,
    /// The label style whose template the note is.
    pub label_style: String,
    /// Explicit outward direction, in radians; absent follows the last leg.
    pub landing_angle: Option<f64>,
}

impl Default for LeaderGeometry {
    fn default() -> Self {
        Self {
            vertices: Vec::new(),
            text: String::new(),
            arrow: ArrowHead::ClosedFilled,
            callout: CalloutShape::None,
            style: String::new(),
            paper_height: 0.0,
            arrow_size: 2.5,
            landing: 2.5,
            tip_ref: AnchorRef::default(),
            fields: false,
            label_style: String::new(),
            landing_angle: None,
        }
    }
}

/// The geometry of an entity. The order of the alternatives is the C++
/// std::variant's, and the index is the on-disk kind byte: APPEND ONLY.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Point(PointGeometry),
    Line(Segment2),
    Arc(Arc2),
    Polyline(Polyline2),
    Circle(Circle2),
    Text(TextGeometry),
    Dimension(DimensionGeometry),
    Label(LabelGeometry),
    Leader(LeaderGeometry),
    // 9, 10, 11: a polyline with arc segments and heights, the ellipse and
    // elliptical arc, the spline (curves2d).
    CurvePolyline(CurvePolyline2),
    Ellipse(Ellipse2),
    Spline(Spline2),
}

impl Default for Geometry {
    fn default() -> Self {
        Self::Point(PointGeometry::default())
    }
}

impl Geometry {
    /// The variant index: the on-disk kind byte.
    pub fn index(&self) -> usize {
        type_of(self) as usize
    }
}

pub fn type_of(geometry: &Geometry) -> EntityType {
    match geometry {
        Geometry::Point(_) => EntityType::Point,
        Geometry::Line(_) => EntityType::Line,
        Geometry::Arc(_) => EntityType::Arc,
        Geometry::Polyline(_) => EntityType::Polyline,
        Geometry::Circle(_) => EntityType::Circle,
        Geometry::Text(_) => EntityType::Text,
        Geometry::Dimension(_) => EntityType::Dimension,
        Geometry::Label(_) => EntityType::Label,
        Geometry::Leader(_) => EntityType::Leader,
        Geometry::CurvePolyline(_) => EntityType::CurvePolyline,
        Geometry::Ellipse(_) => EntityType::Ellipse,
        Geometry::Spline(_) => EntityType::Spline,
    }
}

macro_rules! geometry_from {
    ($($variant:ident($shape:ty)),+ $(,)?) => {
        $(impl From<$shape> for Geometry {
            fn from(value: $shape) -> Self {
                Self::$variant(value)
            }
        })+
    };
}

geometry_from!(
    Point(PointGeometry),
    Line(Segment2),
    Arc(Arc2),
    Polyline(Polyline2),
    Circle(Circle2),
    Text(TextGeometry),
    Dimension(DimensionGeometry),
    Label(LabelGeometry),
    Leader(LeaderGeometry),
    CurvePolyline(CurvePolyline2),
    Ellipse(Ellipse2),
    Spline(Spline2),
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Default for Color {
    fn default() -> Self {
        Self::new(255, 255, 255, 255)
    }
}

fn hex_digit(character: u8) -> i32 {
    match character {
        b'0'..=b'9' => i32::from(character - b'0'),
        b'a'..=b'f' => i32::from(character - b'a') + 10,
        b'A'..=b'F' => i32::from(character - b'A') + 10,
        _ => -1,
    }
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// "#RRGGBB" or "#RRGGBBAA".
    pub fn from_hex(text: &str) -> Result<Color> {
        let bytes = text.as_bytes();
        if (bytes.len() != 7 && bytes.len() != 9) || bytes[0] != b'#' {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "colour must be #RRGGBB or #RRGGBBAA",
                text,
            ));
        }
        let mut channels = [0_u8, 0, 0, 255];
        let mut i = 0;
        while i + 1 < bytes.len() {
            let high = hex_digit(bytes[i + 1]);
            let low = hex_digit(bytes[i + 2]);
            if high < 0 || low < 0 {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "colour contains a non-hexadecimal digit",
                    text,
                ));
            }
            channels[i / 2] = (high * 16 + low) as u8;
            i += 2;
        }
        Ok(Color::new(
            channels[0],
            channels[1],
            channels[2],
            channels[3],
        ))
    }

    /// Alpha omitted when opaque.
    pub fn to_hex(self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
        let mut text = String::from("#");
        let channels = [self.r, self.g, self.b, self.a];
        let count = if self.a == 255 { 3 } else { 4 };
        for &channel in &channels[..count] {
            text.push(char::from(DIGITS[usize::from(channel >> 4)]));
            text.push(char::from(DIGITS[usize::from(channel & 0x0F)]));
        }
        text
    }
}

/// The C++ std::variant<bool, std::int64_t, double, std::string>, in its order
/// (PropertyType numbers the alternatives the same way).
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
    Boolean(bool),
    Integer(i64),
    Real(f64),
    Text(String),
}

impl From<bool> for PropertyValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<i64> for PropertyValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f64> for PropertyValue {
    fn from(value: f64) -> Self {
        Self::Real(value)
    }
}

impl From<String> for PropertyValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for PropertyValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

/// Ordered so that iteration, serialisation and diffs are deterministic.
pub type PropertyMap = BTreeMap<String, PropertyValue>;

/// A property value as a person reads it: "true", "42", "31.25", the text
/// itself - the C++ `toString(const PropertyValue&)`. ONE definition, because
/// the property panel, the command line and the attribute manager must not
/// disagree about what a value says. A real is written with enough digits to
/// read back as the same double.
pub fn property_to_string(value: &PropertyValue) -> String {
    match value {
        PropertyValue::Boolean(flag) => String::from(if *flag { "true" } else { "false" }),
        PropertyValue::Integer(integer) => format!("{integer}"),
        // Shortest text that reads back as the same double (std::to_chars).
        PropertyValue::Real(real) => format_exact_real(*real),
        PropertyValue::Text(text) => text.clone(),
    }
}

/// "text", "integer", "real" or "boolean": what the attribute manager and
/// `PROP SET ... <type>` name the four kinds.
pub fn type_name(value: &PropertyValue) -> &'static str {
    match value {
        PropertyValue::Boolean(_) => "boolean",
        PropertyValue::Integer(_) => "integer",
        PropertyValue::Real(_) => "real",
        PropertyValue::Text(_) => "text",
    }
}

/// A 2D drawing has no Z of its own, so a point's height lives in a property.
/// `ELEVATION_PROPERTY` is one height, for a point; `ELEVATIONS_PROPERTY` the
/// per-vertex list for a 3D string, where a "null" entry means "not surveyed"
/// and is not the same as zero.
pub const ELEVATION_PROPERTY: &str = "elevation";
pub const ELEVATIONS_PROPERTY: &str = "elevations";

/// Makes the properties describe exactly `heights`, one per vertex: nothing
/// when no vertex has a height (absent is not zero), one `elevation` when
/// every vertex has the same height, otherwise an `elevations` list with
/// "null" where a vertex has none. A non-finite height is no height. The
/// property that does not apply is removed, so a rewrite cannot leave behind
/// a stale one that the reader would prefer.
pub fn set_heights(properties: &mut PropertyMap, heights: &[Option<f64>]) {
    properties.remove(ELEVATION_PROPERTY);
    properties.remove(ELEVATIONS_PROPERTY);
    let known = |z: &Option<f64>| z.is_some_and(f64::is_finite);
    if !heights.iter().any(known) {
        return;
    }
    // The first height is known whenever this is true: it is compared first.
    let same = heights.iter().all(|z| known(z) && *z == heights[0]);
    if same {
        properties.insert(
            String::from(ELEVATION_PROPERTY),
            PropertyValue::Real(heights[0].unwrap_or_default()),
        );
        return;
    }
    let mut list = String::new();
    for z in heights {
        if !list.is_empty() {
            list.push(' ');
        }
        match z {
            Some(height) if height.is_finite() => list.push_str(&format_exact_real(*height)),
            _ => list.push_str("null"),
        }
    }
    properties.insert(String::from(ELEVATIONS_PROPERTY), PropertyValue::Text(list));
}

/// The heights of `count` vertices: the `elevations` list when it has exactly
/// `count` entries, else `elevation` for every vertex, else none. A list of
/// the wrong length describes some other geometry, so it is passed over
/// rather than stretched to fit.
pub fn heights_of(properties: &PropertyMap, count: usize) -> Vec<Option<f64>> {
    if let Some(PropertyValue::Text(list)) = properties.get(ELEVATIONS_PROPERTY) {
        let bytes = list.as_bytes();
        let mut parsed = Vec::new();
        let mut start = 0;
        while start < bytes.len() {
            while start < bytes.len() && is_ascii_space(bytes[start]) {
                start += 1;
            }
            let mut end = start;
            while end < bytes.len() && !is_ascii_space(bytes[end]) {
                end += 1;
            }
            if end > start {
                // "null", or anything else that is not a finite number, is
                // no height.
                parsed.push(parse_finite_double(&bytes[start..end]));
            }
            start = end;
        }
        if parsed.len() == count {
            return parsed;
        }
    }
    let mut heights = vec![None; count];
    if let Some(found) = properties.get(ELEVATION_PROPERTY) {
        let z = match found {
            PropertyValue::Real(real) => Some(*real),
            PropertyValue::Integer(integer) => Some(*integer as f64),
            _ => None,
        };
        if let Some(height) = z.filter(|height| height.is_finite()) {
            heights.fill(Some(height));
        }
    }
    heights
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub geometry: Geometry,
    pub layer: String,
    /// Empty: ByLayer.
    pub style: String,
    /// Empty: ByLayer.
    pub color: Option<Color>,
    pub visible: bool,
    /// User / application attributes.
    pub properties: PropertyMap,
    /// Provenance: source file, import time, author...
    pub metadata: PropertyMap,
}

impl Default for Entity {
    fn default() -> Self {
        Self {
            id: INVALID_ENTITY_ID,
            geometry: Geometry::default(),
            layer: String::from("0"),
            style: String::new(),
            color: None,
            visible: true,
            properties: PropertyMap::new(),
            metadata: PropertyMap::new(),
        }
    }
}

impl Entity {
    /// The C++ `Entity::type()`.
    pub fn r#type(&self) -> EntityType {
        type_of(&self.geometry)
    }
}
