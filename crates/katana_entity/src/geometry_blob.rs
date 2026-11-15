//! Binary encoding of entity geometry (docs/storage.md).
//!
//! WHY. Opening a 50 000-entity project cost 891 ms, of which 602 ms - 68% -
//! was parsing the JSON that geometry was stored as. An eight-vertex polyline
//! is 128 bytes of doubles; as JSON it is around 350 bytes of text that has to
//! be lexed, unescaped and converted decimal to binary on every load.
//!
//! FORMAT. Little-endian, length-prefixed, and versioned in its first byte so
//! a later change can be migrated rather than guessed at:
//!
//! ```text
//! u8   format version (BLOB_VERSION)
//! u8   geometry kind  (the Geometry variant index, i.e. EntityType)
//! ...  payload, per kind, all doubles in IEEE-754 little-endian
//! ```
//!
//! Strings are u32 length then bytes, and are still required to be valid
//! UTF-8, so that a blob can be re-encoded as JSON for an export.
//!
//! WHAT IT IS NOT. A storage format, not a wire protocol: no checksum, because
//! SQLite already protects the page it lives in, and no compression, because
//! the geometry is mostly incompressible doubles.
//!
//! EXACTNESS. Doubles are stored by their bit pattern, so a round trip is bit
//! exact - including negative zero, infinities and NaN payloads.
//!
//! Version 2 (2026-09-25) appends the annotation members to a text's and a
//! dimension's version-1 payload and adds the Label and Leader kinds. The
//! writer emits version 1 for a geometry version 1 can hold, so a drawing that
//! uses nothing new is stored byte for byte as before. Version 3 writes every
//! anchor reference with its parameter (a double after the index), allows the
//! Along and Inside anchor points, and appends a leader's `fields` (u8) and
//! `label_style` (string); it is written only for a dimension or a leader
//! that needs it. The drawing system's kinds (CurvePolyline: u32 count, u8
//! closed, then x, y, bulge, height per vertex, a NaN height for none;
//! Ellipse: centre, major axis, ratio, start, sweep; Spline: u8 degree,
//! control points, knots, weights, fit points, each list u32-counted) are
//! version-2 kinds and are read in version 2 or 3 alike.

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, CurveVertex, Ellipse2, Point2, Polyline2, Segment2, Spline2,
};

use crate::entity::{
    AnchorPoint, AnchorRef, ArrowHead, CalloutShape, DimensionGeometry, DimensionKind, EntityType,
    Geometry, LAST_ANCHOR_POINT, LabelGeometry, LeaderGeometry, PointGeometry, TextGeometry,
    TextJustify,
};

/// Bumped only when the layout changes. Readers accept every version they
/// know how to read.
pub const BLOB_VERSION: u8 = 1;
pub const BLOB_VERSION_ANNOTATION: u8 = 2;
pub const BLOB_VERSION_SMART_LEADER: u8 = 3;
/// Version 4 appends an explicit landing angle to a leader's version-3
/// payload. Only a leader that specifies one needs this version.
pub const BLOB_VERSION_LEADER_LANDING: u8 = 4;

/// A polyline of more vertices than this is not survey data, it is a corrupt
/// length field. Checked before any allocation, so a damaged blob cannot ask
/// for gigabytes. Ten million vertices is far past any real string.
const MAXIMUM_VERTICES: u32 = 10_000_000;
/// Strings in geometry are labels and dimension overrides, not documents.
const MAXIMUM_STRING_BYTES: u32 = 1_000_000;

// ---- writing ---------------------------------------------------------------------

fn put_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

/// Explicitly little-endian rather than the native layout, so a project
/// written on one machine reads on another.
fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// By bit pattern, so the round trip is exact for every value a double can
/// hold - negative zero and NaN included.
fn put_double(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&value.to_bits().to_le_bytes());
}

fn put_point(out: &mut Vec<u8>, point: Point2) {
    put_double(out, point.x);
    put_double(out, point.y);
}

/// (The C++ also refused text that is not UTF-8 here, the rule the JSON
/// writer enforces; a `String` cannot be anything else.)
fn put_string(out: &mut Vec<u8>, text: &str) -> Status {
    if text.len() > MAXIMUM_STRING_BYTES as usize {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "text is too long to store",
            format!("{}", text.len()),
        ));
    }
    put_u32(out, text.len() as u32);
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

fn put_anchor(out: &mut Vec<u8>, reference: &AnchorRef, version: u8) {
    put_u64(out, reference.entity);
    put_u8(out, reference.point as u8);
    put_u32(out, reference.index);
    if version >= BLOB_VERSION_SMART_LEADER {
        put_double(out, reference.parameter);
    }
}

fn put_points(out: &mut Vec<u8>, points: &[Point2]) -> Status {
    if points.len() > MAXIMUM_VERTICES as usize {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a geometry has more vertices than the format allows",
            format!("{}", points.len()),
        ));
    }
    put_u32(out, points.len() as u32);
    out.reserve(points.len() * 16);
    for &point in points {
        put_point(out, point);
    }
    Ok(())
}

/// Whether version 1 can hold `geometry`: every kind it had, with the members
/// added since at their defaults.
fn fits_version_one(geometry: &Geometry) -> bool {
    match geometry {
        Geometry::Text(text) => {
            text.style.is_empty()
                && text.paper_height == 0.0
                && text.justify == TextJustify::BottomLeft
                && !text.paper_height.is_sign_negative()
        }
        Geometry::Dimension(dimension) => {
            let plain = DimensionGeometry {
                start: dimension.start,
                end: dimension.end,
                offset: dimension.offset,
                text_override: dimension.text_override.clone(),
                ..DimensionGeometry::default()
            };
            // Compared by bits for the doubles, so a -0.0 angle is written in
            // the layout that keeps it.
            *dimension == plain
                && !dimension.angle.is_sign_negative()
                && !dimension.vertex.x.is_sign_negative()
                && !dimension.vertex.y.is_sign_negative()
        }
        // Every kind appended after Dimension is version 2's: a version-1
        // reader knows none of them.
        _ => geometry.index() <= EntityType::Dimension as usize,
    }
}

/// Whether version 2 can hold an anchor: one of the points it had, and no
/// parameter - compared by bits, so a -0.0 is written in the layout that
/// keeps it.
fn anchor_fits_version_two(reference: &AnchorRef) -> bool {
    (reference.point as u8) <= (AnchorPoint::SegmentMid as u8) && reference.parameter.to_bits() == 0
}

/// Whether version 2 can hold `geometry`: its anchors do, and a leader is not
/// a smart one.
fn fits_version_two(geometry: &Geometry) -> bool {
    match geometry {
        Geometry::Dimension(dimension) => {
            anchor_fits_version_two(&dimension.start_ref)
                && anchor_fits_version_two(&dimension.end_ref)
                && anchor_fits_version_two(&dimension.vertex_ref)
        }
        Geometry::Leader(leader) => {
            anchor_fits_version_two(&leader.tip_ref)
                && !leader.fields
                && leader.label_style.is_empty()
        }
        _ => true,
    }
}

/// Encodes `geometry`. Fails with InvalidArgument for a string longer than the
/// format can express, or a geometry larger than it allows - both of which are
/// already rejected on the way into the model, so reaching either means a
/// guard was bypassed.
pub fn geometry_to_blob(geometry: &Geometry) -> Result<Vec<u8>> {
    // Two header bytes plus the largest fixed payload; a polyline grows from
    // here.
    let mut out = Vec::with_capacity(48);
    let version = if matches!(geometry, Geometry::Leader(leader) if leader.landing_angle.is_some())
    {
        BLOB_VERSION_LEADER_LANDING
    } else if fits_version_one(geometry) {
        BLOB_VERSION
    } else if fits_version_two(geometry) {
        BLOB_VERSION_ANNOTATION
    } else {
        BLOB_VERSION_SMART_LEADER
    };
    let annotation = version >= BLOB_VERSION_ANNOTATION;
    put_u8(&mut out, version);
    put_u8(&mut out, geometry.index() as u8);

    match geometry {
        Geometry::Point(shape) => put_point(&mut out, shape.position),
        Geometry::Line(shape) => {
            put_point(&mut out, shape.start);
            put_point(&mut out, shape.end);
        }
        Geometry::Arc(shape) => {
            put_point(&mut out, shape.center);
            put_double(&mut out, shape.radius);
            put_double(&mut out, shape.start_angle);
            put_double(&mut out, shape.sweep);
        }
        Geometry::Polyline(shape) => {
            if shape.vertices.len() > MAXIMUM_VERTICES as usize {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "polyline has more vertices than the format allows",
                    format!("{}", shape.vertices.len()),
                ));
            }
            put_u32(&mut out, shape.vertices.len() as u32);
            put_u8(&mut out, u8::from(shape.closed));
            out.reserve(shape.vertices.len() * 16);
            for &vertex in &shape.vertices {
                put_point(&mut out, vertex);
            }
        }
        Geometry::Circle(shape) => {
            put_point(&mut out, shape.center);
            put_double(&mut out, shape.radius);
        }
        Geometry::Text(shape) => {
            put_point(&mut out, shape.position);
            put_double(&mut out, shape.height);
            put_double(&mut out, shape.rotation);
            put_string(&mut out, &shape.text)?;
            if annotation {
                put_string(&mut out, &shape.style)?;
                put_double(&mut out, shape.paper_height);
                put_u8(&mut out, shape.justify as u8);
            }
        }
        Geometry::Dimension(shape) => {
            put_point(&mut out, shape.start);
            put_point(&mut out, shape.end);
            put_double(&mut out, shape.offset);
            put_string(&mut out, &shape.text_override)?;
            if annotation {
                put_u8(&mut out, shape.kind as u8);
                put_double(&mut out, shape.angle);
                put_point(&mut out, shape.vertex);
                put_anchor(&mut out, &shape.start_ref, version);
                put_anchor(&mut out, &shape.end_ref, version);
                put_anchor(&mut out, &shape.vertex_ref, version);
            }
        }
        Geometry::Label(shape) => {
            put_u64(&mut out, shape.target);
            // The C++ static_cast<std::uint32_t> of the signed part.
            put_u32(&mut out, shape.part as u32);
            put_point(&mut out, shape.anchor);
            put_u8(&mut out, u8::from(shape.position.is_some()));
            if let Some(position) = shape.position {
                put_point(&mut out, position);
            }
            for text in [
                &shape.style,
                &shape.alignment,
                &shape.text_override,
                &shape.rule,
            ] {
                put_string(&mut out, text)?;
            }
        }
        Geometry::Leader(shape) => {
            put_points(&mut out, &shape.vertices)?;
            put_string(&mut out, &shape.text)?;
            put_string(&mut out, &shape.style)?;
            put_u8(&mut out, shape.arrow as u8);
            put_u8(&mut out, shape.callout as u8);
            put_double(&mut out, shape.paper_height);
            put_double(&mut out, shape.arrow_size);
            put_double(&mut out, shape.landing);
            put_anchor(&mut out, &shape.tip_ref, version);
            if version >= BLOB_VERSION_SMART_LEADER {
                put_u8(&mut out, u8::from(shape.fields));
                put_string(&mut out, &shape.label_style)?;
            }
            if version >= BLOB_VERSION_LEADER_LANDING
                && let Some(angle) = shape.landing_angle
            {
                put_double(&mut out, angle);
            }
        }
        Geometry::CurvePolyline(shape) => {
            // 32 bytes a vertex: x, y, bulge, height - a NaN height for "not
            // surveyed", which validation keeps from meaning anything else (a
            // height must be finite).
            if shape.vertices.len() > MAXIMUM_VERTICES as usize {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "polyline has more vertices than the format allows",
                    format!("{}", shape.vertices.len()),
                ));
            }
            put_u32(&mut out, shape.vertices.len() as u32);
            put_u8(&mut out, u8::from(shape.closed));
            out.reserve(shape.vertices.len() * 32);
            for vertex in &shape.vertices {
                put_point(&mut out, vertex.position);
                put_double(&mut out, vertex.bulge);
                put_double(&mut out, vertex.height.unwrap_or(f64::NAN));
            }
        }
        Geometry::Ellipse(shape) => {
            put_point(&mut out, shape.center);
            put_point(&mut out, shape.major_axis);
            put_double(&mut out, shape.ratio);
            put_double(&mut out, shape.start_parameter);
            put_double(&mut out, shape.sweep);
        }
        Geometry::Spline(shape) => {
            if !(0..=255).contains(&shape.degree)
                || shape.knots.len() > MAXIMUM_VERTICES as usize
                || shape.weights.len() > MAXIMUM_VERTICES as usize
            {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "spline is larger than the format allows",
                ));
            }
            put_u8(&mut out, shape.degree as u8);
            put_points(&mut out, &shape.control_points)?;
            for list in [&shape.knots, &shape.weights] {
                put_u32(&mut out, list.len() as u32);
                for &value in list {
                    put_double(&mut out, value);
                }
            }
            put_points(&mut out, &shape.fit_points)?;
        }
    }
    Ok(out)
}

// ---- reading ---------------------------------------------------------------------

/// A cursor that can only move forward and always checks first. Every read
/// goes through it, so there is one place where a truncated blob is caught.
struct Reader<'a> {
    blob: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(blob: &'a [u8]) -> Self {
        Self { blob, at: 0 }
    }

    fn has(&self, bytes: usize) -> bool {
        self.blob.len() - self.at >= bytes
    }

    fn remaining(&self) -> usize {
        self.blob.len() - self.at
    }

    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        if !self.has(N) {
            return None;
        }
        let mut bytes = [0_u8; N];
        bytes.copy_from_slice(&self.blob[self.at..self.at + N]);
        self.at += N;
        Some(bytes)
    }

    fn read_u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|bytes| bytes[0])
    }

    fn read_u32(&mut self) -> Option<u32> {
        self.take::<4>().map(u32::from_le_bytes)
    }

    fn read_u64(&mut self) -> Option<u64> {
        self.take::<8>().map(u64::from_le_bytes)
    }

    fn read_double(&mut self) -> Option<f64> {
        self.take::<8>()
            .map(|bytes| f64::from_bits(u64::from_le_bytes(bytes)))
    }

    fn read_point(&mut self) -> Option<Point2> {
        let x = self.read_double()?;
        let y = self.read_double()?;
        Some(Point2::new(x, y))
    }

    /// Version 3 adds the parameter and the points after SegmentMid; a
    /// version-2 anchor naming one of those is malformed.
    fn read_anchor(&mut self, version: u8) -> Option<AnchorRef> {
        let with_parameter = version >= BLOB_VERSION_SMART_LEADER;
        let last = if with_parameter {
            LAST_ANCHOR_POINT
        } else {
            AnchorPoint::SegmentMid
        };
        let entity = self.read_u64()?;
        let point = self.read_u8()?;
        let index = self.read_u32()?;
        if point > last as u8 {
            return None;
        }
        let point = anchor_point_from_byte(point)?;
        let parameter = if with_parameter {
            self.read_double()?
        } else {
            0.0
        };
        Some(AnchorRef {
            entity,
            point,
            index,
            parameter,
        })
    }

    fn read_points(&mut self) -> Option<Vec<Point2>> {
        let count = self.read_u32()?;
        if count > MAXIMUM_VERTICES || !self.has(count as usize * 16) {
            return None;
        }
        let mut points = Vec::with_capacity(count as usize);
        for _ in 0..count {
            points.push(self.read_point()?);
        }
        Some(points)
    }

    fn read_doubles(&mut self) -> Option<Vec<f64>> {
        let count = self.read_u32()?;
        if count > MAXIMUM_VERTICES || !self.has(count as usize * 8) {
            return None;
        }
        let mut values = Vec::with_capacity(count as usize);
        for _ in 0..count {
            values.push(self.read_double()?);
        }
        Some(values)
    }

    /// The bytes must be valid UTF-8: this is where the model's text is
    /// decoded.
    fn read_string(&mut self) -> Option<String> {
        let length = self.read_u32()?;
        if length > MAXIMUM_STRING_BYTES || !self.has(length as usize) {
            return None;
        }
        let bytes = &self.blob[self.at..self.at + length as usize];
        self.at += length as usize;
        String::from_utf8(bytes.to_vec()).ok()
    }
}

fn anchor_point_from_byte(byte: u8) -> Option<AnchorPoint> {
    Some(match byte {
        0 => AnchorPoint::Position,
        1 => AnchorPoint::Start,
        2 => AnchorPoint::End,
        3 => AnchorPoint::Mid,
        4 => AnchorPoint::Centre,
        5 => AnchorPoint::Vertex,
        6 => AnchorPoint::SegmentMid,
        7 => AnchorPoint::Along,
        8 => AnchorPoint::Inside,
        _ => return None,
    })
}

fn justify_from_byte(byte: u8) -> Option<TextJustify> {
    Some(match byte {
        0 => TextJustify::BottomLeft,
        1 => TextJustify::BottomCentre,
        2 => TextJustify::BottomRight,
        3 => TextJustify::MiddleLeft,
        4 => TextJustify::MiddleCentre,
        5 => TextJustify::MiddleRight,
        6 => TextJustify::TopLeft,
        7 => TextJustify::TopCentre,
        8 => TextJustify::TopRight,
        _ => return None,
    })
}

fn dimension_kind_from_byte(byte: u8) -> Option<DimensionKind> {
    Some(match byte {
        0 => DimensionKind::Aligned,
        1 => DimensionKind::Linear,
        2 => DimensionKind::Angular,
        3 => DimensionKind::Radius,
        4 => DimensionKind::Diameter,
        5 => DimensionKind::OrdinateX,
        6 => DimensionKind::OrdinateY,
        _ => return None,
    })
}

fn arrow_from_byte(byte: u8) -> Option<ArrowHead> {
    Some(match byte {
        0 => ArrowHead::None,
        1 => ArrowHead::Tick,
        2 => ArrowHead::ClosedFilled,
        3 => ArrowHead::Open,
        4 => ArrowHead::Dot,
        _ => return None,
    })
}

fn callout_from_byte(byte: u8) -> Option<CalloutShape> {
    Some(match byte {
        0 => CalloutShape::None,
        1 => CalloutShape::Box,
        2 => CalloutShape::Circle,
        _ => return None,
    })
}

fn truncated() -> Error {
    Error::new(
        ErrorCode::ParseFailure,
        "geometry blob is truncated or malformed",
    )
}

fn in_version_one(what: &str) -> Error {
    Error::new(
        ErrorCode::ParseFailure,
        format!("{what} in a version-1 geometry blob"),
    )
}

fn implausible_count(count: u32) -> Error {
    make_error(
        ErrorCode::ParseFailure,
        "polyline vertex count is implausible",
        format!("{count}"),
    )
}

fn read_segment(reader: &mut Reader<'_>) -> Option<Segment2> {
    let start = reader.read_point()?;
    let end = reader.read_point()?;
    Some(Segment2::new(start, end))
}

fn read_arc(reader: &mut Reader<'_>) -> Option<Arc2> {
    let center = reader.read_point()?;
    let radius = reader.read_double()?;
    let start_angle = reader.read_double()?;
    let sweep = reader.read_double()?;
    Some(Arc2::new(center, radius, start_angle, sweep))
}

fn read_circle(reader: &mut Reader<'_>) -> Option<Circle2> {
    let center = reader.read_point()?;
    let radius = reader.read_double()?;
    Some(Circle2::new(center, radius))
}

fn read_curve_vertex(reader: &mut Reader<'_>) -> Option<CurveVertex> {
    let position = reader.read_point()?;
    let bulge = reader.read_double()?;
    let height = reader.read_double()?;
    Some(CurveVertex {
        position,
        bulge,
        height: if height.is_nan() { None } else { Some(height) },
    })
}

fn read_ellipse(reader: &mut Reader<'_>) -> Option<Ellipse2> {
    let center = reader.read_point()?;
    let major_axis = reader.read_point()?;
    let ratio = reader.read_double()?;
    let start_parameter = reader.read_double()?;
    let sweep = reader.read_double()?;
    Some(Ellipse2 {
        center,
        major_axis,
        ratio,
        start_parameter,
        sweep,
    })
}

fn read_text(reader: &mut Reader<'_>, annotation: bool) -> Option<TextGeometry> {
    let mut text = TextGeometry {
        position: reader.read_point()?,
        height: reader.read_double()?,
        rotation: reader.read_double()?,
        text: reader.read_string()?,
        ..TextGeometry::default()
    };
    if annotation {
        text.style = reader.read_string()?;
        text.paper_height = reader.read_double()?;
        text.justify = justify_from_byte(reader.read_u8()?)?;
    }
    Some(text)
}

fn read_dimension(
    reader: &mut Reader<'_>,
    annotation: bool,
    version: u8,
) -> Option<DimensionGeometry> {
    let mut dimension = DimensionGeometry {
        start: reader.read_point()?,
        end: reader.read_point()?,
        offset: reader.read_double()?,
        text_override: reader.read_string()?,
        ..DimensionGeometry::default()
    };
    if annotation {
        dimension.kind = dimension_kind_from_byte(reader.read_u8()?)?;
        dimension.angle = reader.read_double()?;
        dimension.vertex = reader.read_point()?;
        dimension.start_ref = reader.read_anchor(version)?;
        dimension.end_ref = reader.read_anchor(version)?;
        dimension.vertex_ref = reader.read_anchor(version)?;
    }
    Some(dimension)
}

fn read_label(reader: &mut Reader<'_>) -> Option<LabelGeometry> {
    let target = reader.read_u64()?;
    let part = reader.read_u32()?;
    let anchor = reader.read_point()?;
    let has_position = reader.read_u8()?;
    let position = if has_position != 0 {
        Some(reader.read_point()?)
    } else {
        None
    };
    Some(LabelGeometry {
        target,
        // The C++ static_cast<std::int32_t> of the stored u32.
        part: part as i32,
        anchor,
        position,
        style: reader.read_string()?,
        alignment: reader.read_string()?,
        text_override: reader.read_string()?,
        rule: reader.read_string()?,
    })
}

fn read_leader(reader: &mut Reader<'_>, version: u8) -> Option<LeaderGeometry> {
    let vertices = reader.read_points()?;
    let text = reader.read_string()?;
    let style = reader.read_string()?;
    let arrow = reader.read_u8()?;
    let callout = reader.read_u8()?;
    let paper_height = reader.read_double()?;
    let arrow_size = reader.read_double()?;
    let landing = reader.read_double()?;
    let tip_ref = reader.read_anchor(version)?;
    let mut leader = LeaderGeometry {
        vertices,
        text,
        arrow: arrow_from_byte(arrow)?,
        callout: callout_from_byte(callout)?,
        style,
        paper_height,
        arrow_size,
        landing,
        tip_ref,
        ..LeaderGeometry::default()
    };
    if version >= BLOB_VERSION_SMART_LEADER {
        let fields = reader.read_u8()?;
        if fields > 1 {
            return None;
        }
        leader.label_style = reader.read_string()?;
        leader.fields = fields != 0;
    }
    if version >= BLOB_VERSION_LEADER_LANDING {
        leader.landing_angle = Some(reader.read_double()?);
    }
    Some(leader)
}

fn read_spline(reader: &mut Reader<'_>) -> Option<Spline2> {
    let degree = reader.read_u8()?;
    let control_points = reader.read_points()?;
    let knots = reader.read_doubles()?;
    let weights = reader.read_doubles()?;
    let fit_points = reader.read_points()?;
    Some(Spline2 {
        degree: i32::from(degree),
        control_points,
        knots,
        weights,
        fit_points,
    })
}

/// Decodes a blob produced by geometry_to_blob.
///
/// Fails with ParseFailure for anything malformed: an unknown version or
/// kind, a truncated payload, a length that runs past the end, a string that
/// is not valid UTF-8, or trailing bytes after a complete record. Trailing
/// bytes are an error rather than ignored - they mean the writer and reader
/// disagree about the layout, and continuing would decode later fields
/// wrongly.
///
/// This runs on data from disk, which may be damaged or may have been written
/// by a different version, so every length is checked against what remains
/// before it is used.
pub fn geometry_from_blob(blob: &[u8]) -> Result<Geometry> {
    let mut reader = Reader::new(blob);
    let (Some(version), Some(kind)) = (reader.read_u8(), reader.read_u8()) else {
        return Err(truncated());
    };
    if version != BLOB_VERSION
        && version != BLOB_VERSION_ANNOTATION
        && version != BLOB_VERSION_SMART_LEADER
        && version != BLOB_VERSION_LEADER_LANDING
    {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "unknown geometry blob version",
            format!("{version}"),
        ));
    }
    let annotation = version >= BLOB_VERSION_ANNOTATION;

    let geometry = match kind {
        0 => Geometry::Point(PointGeometry {
            position: reader.read_point().ok_or_else(truncated)?,
        }),
        1 => Geometry::Line(read_segment(&mut reader).ok_or_else(truncated)?),
        2 => Geometry::Arc(read_arc(&mut reader).ok_or_else(truncated)?),
        3 => {
            let (Some(count), Some(closed)) = (reader.read_u32(), reader.read_u8()) else {
                return Err(truncated());
            };
            if count > MAXIMUM_VERTICES {
                return Err(implausible_count(count));
            }
            // The length is checked against what is actually left BEFORE
            // reserving, so a corrupt count cannot turn into a huge allocation.
            if !reader.has(count as usize * 16) {
                return Err(truncated());
            }
            let mut vertices = Vec::with_capacity(count as usize);
            for _ in 0..count {
                vertices.push(reader.read_point().ok_or_else(truncated)?);
            }
            Geometry::Polyline(Polyline2::new(vertices, closed != 0))
        }
        4 => Geometry::Circle(read_circle(&mut reader).ok_or_else(truncated)?),
        5 => Geometry::Text(read_text(&mut reader, annotation).ok_or_else(truncated)?),
        6 => Geometry::Dimension(
            read_dimension(&mut reader, annotation, version).ok_or_else(truncated)?,
        ),
        7 => {
            if !annotation {
                return Err(in_version_one("a label"));
            }
            Geometry::Label(read_label(&mut reader).ok_or_else(truncated)?)
        }
        8 => {
            if !annotation {
                return Err(in_version_one("a leader"));
            }
            Geometry::Leader(read_leader(&mut reader, version).ok_or_else(truncated)?)
        }
        9 => {
            if !annotation {
                return Err(in_version_one("a curve polyline"));
            }
            let (Some(count), Some(closed)) = (reader.read_u32(), reader.read_u8()) else {
                return Err(truncated());
            };
            if count > MAXIMUM_VERTICES {
                return Err(implausible_count(count));
            }
            if !reader.has(count as usize * 32) {
                return Err(truncated());
            }
            let mut vertices = Vec::with_capacity(count as usize);
            for _ in 0..count {
                vertices.push(read_curve_vertex(&mut reader).ok_or_else(truncated)?);
            }
            Geometry::CurvePolyline(CurvePolyline2 {
                vertices,
                closed: closed != 0,
            })
        }
        10 => {
            if !annotation {
                return Err(in_version_one("an ellipse"));
            }
            Geometry::Ellipse(read_ellipse(&mut reader).ok_or_else(truncated)?)
        }
        11 => {
            if !annotation {
                return Err(in_version_one("a spline"));
            }
            Geometry::Spline(read_spline(&mut reader).ok_or_else(truncated)?)
        }
        _ => {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "unknown geometry kind in blob",
                format!("{kind}"),
            ));
        }
    };

    if reader.remaining() != 0 {
        // Not ignored: trailing bytes mean the writer and this reader disagree
        // about the layout, and every later field would be decoded wrongly.
        return Err(make_error(
            ErrorCode::ParseFailure,
            "geometry blob has trailing bytes",
            format!("{}", reader.remaining()),
        ));
    }
    Ok(geometry)
}
