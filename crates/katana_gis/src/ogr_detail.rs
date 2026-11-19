//! The conversions between Katana's plain geometry and OGR's, and the one way
//! a coordinate system is read (src/katana_io/ogr_detail.hpp, implemented in
//! gdal_adapter.cpp), for the adapter and the algorithm bridge, so that a file
//! read by IMPORT and a dataset handed back by an algorithm become the same
//! geometry by the same code.
//!
//! The Rust owners of OGR's objects are in the FFI modules and named here for
//! the code that converts: `OgrGeometry` and `GeometryView` in ffi/geometry.rs,
//! `SpatialReference` in ffi/srs.rs, each saying who frees what. Each of their
//! methods is one C call, so the conversions below are safe code.

use crate::ffi::{ogr, osr};
use crate::gdal_adapter::{GeoPoint, GeometryKind, VectorGeometry, VectorReadReport};

pub(crate) use crate::ffi::geometry::{GeometryView, OgrGeometry, flattened, geometry_type_name};
pub(crate) use crate::ffi::srs::SpatialReference;

// ---- spatial references --------------------------------------------------------------

/// Parses a coordinate system the way GDAL's own SetProjection would - WKT1,
/// WKT2, PROJJSON - but refuses to fetch anything: a string read from a file
/// is not permission to open another file or a URL. With the traditional GIS
/// axis order.
pub(crate) fn parse_crs(text: &str, reference: &mut SpatialReference) -> bool {
    reference.set_axis_mapping_strategy(osr::OAMS_TRADITIONAL_GIS_ORDER);
    if reference.import_from_wkt(text) == ogr::OGRERR_NONE {
        return true;
    }
    reference.set_from_user_input(text, &osr::SET_FROM_USER_INPUT_LIMITATIONS) == ogr::OGRERR_NONE
}

// ---- OGR geometry -> VectorGeometry ----------------------------------------------------

/// The points of a line or of a CircularString (both are simple curves): a
/// CircularString's points are the ends and the middles of its arcs.
fn points_of(line: GeometryView<'_>) -> Vec<GeoPoint> {
    let count = line.point_count();
    let mut points = Vec::with_capacity(count.max(0) as usize);
    let has3d = line.is_3d();
    for i in 0..count {
        let (x, y, z) = line.point(i);
        points.push(GeoPoint {
            x,
            y,
            z: if has3d { z } else { 0.0 },
        });
    }
    points
}

/// A curve's points appended to `points`, and the index each of its arcs
/// starts at appended to `arcs` (VectorGeometry::arcs). A CompoundCurve's
/// members meet end to start, so each after the first gives its points but
/// its first.
fn append_curve(curve: GeometryView<'_>, points: &mut Vec<GeoPoint>, arcs: &mut Vec<usize>) {
    let geometry_type = flattened(curve.geometry_type());
    if geometry_type == ogr::WKB_COMPOUND_CURVE {
        for i in 0..curve.member_count() {
            if let Some(member) = curve.member(i) {
                append_curve(member, points, arcs);
            }
        }
        return;
    }
    let own = points_of(curve);
    let start = if points.is_empty() {
        0
    } else {
        points.len() - 1
    };
    let skip = usize::from(!(points.is_empty() || own.is_empty()));
    points.extend_from_slice(&own[skip..]);
    if geometry_type == ogr::WKB_CIRCULAR_STRING {
        // (0, 1, 2), (2, 3, 4) ...: an arc every two points.
        let mut k = 0;
        while k + 2 < own.len() {
            arcs.push(start + k);
            k += 2;
        }
    }
}

fn is_curve(geometry_type: ogr::OgrWkbGeometryType) -> bool {
    matches!(
        flattened(geometry_type),
        ogr::WKB_CIRCULAR_STRING
            | ogr::WKB_COMPOUND_CURVE
            | ogr::WKB_CURVE_POLYGON
            | ogr::WKB_MULTI_CURVE
            | ogr::WKB_MULTI_SURFACE
    )
}

fn count(report: &mut VectorReadReport, what: &str) {
    *report.skipped.entry(what.to_owned()).or_insert(0) += 1;
}

/// Appends one VectorGeometry per SIMPLE geometry. A multi-geometry or
/// collection is recursed into so that every emitted VectorGeometry has
/// exactly one kind, which is what keeps the consumer free of nested-variant
/// handling. A curve is kept as its arcs when `keep_arcs` is set (IMPORT turns
/// them into Katana's arcs and circles), and made chords at GDAL's own step
/// otherwise; what has no Katana counterpart - a TIN, a polyhedral surface -
/// is counted in `report`, never silently dropped.
pub(crate) fn flatten(
    geometry: Option<GeometryView<'_>>,
    out: &mut Vec<VectorGeometry>,
    keep_arcs: bool,
    report: &mut VectorReadReport,
    depth: i32,
) {
    let Some(geometry) = geometry else {
        return;
    };
    if geometry.is_empty() {
        return;
    }
    // Defensive: OGR geometries are trees and a malformed file could in
    // principle nest collections deeply. 32 is far beyond anything meaningful.
    if depth > 32 {
        count(report, "nested too deep");
        return;
    }

    let geometry_type = flattened(geometry.geometry_type());
    if !keep_arcs && is_curve(geometry_type) {
        // The chords GDAL makes at its own step (OGR_ARC_STEPSIZE, 4 degrees):
        // what an algorithm is handed. IMPORT keeps the arcs and makes its
        // chords by the one rule EXPORT uses (docs/interop.md, "Curves").
        if let Some(linear) = geometry.linear_geometry() {
            report.curves_made_chords += 1;
            flatten(Some(linear.view()), out, keep_arcs, report, depth + 1);
        }
        return;
    }

    match geometry_type {
        ogr::WKB_POINT => {
            let is3d = geometry.is_3d();
            let (x, y, z) = geometry.point(0);
            out.push(VectorGeometry {
                kind: GeometryKind::Point,
                parts: vec![vec![GeoPoint {
                    x,
                    y,
                    z: if is3d { z } else { 0.0 },
                }]],
                has_z: is3d,
                ..VectorGeometry::default()
            });
        }
        ogr::WKB_LINE_STRING | ogr::WKB_CIRCULAR_STRING | ogr::WKB_COMPOUND_CURVE => {
            let mut points = Vec::new();
            let mut arcs = Vec::new();
            append_curve(geometry, &mut points, &mut arcs);
            let mut part = VectorGeometry {
                kind: GeometryKind::LineString,
                parts: vec![points],
                has_z: geometry.is_3d(),
                arcs: vec![arcs],
            };
            if !part.has_arcs() {
                part.arcs.clear();
            }
            out.push(part);
        }
        // A Triangle is a polygon of three sides (OGRTriangle is an OGRPolygon).
        ogr::WKB_POLYGON | ogr::WKB_TRIANGLE | ogr::WKB_CURVE_POLYGON => {
            let mut part = VectorGeometry {
                kind: GeometryKind::Polygon,
                ..VectorGeometry::default()
            };
            // The exterior ring and then the interior ones.
            for i in 0..geometry.member_count() {
                let Some(ring) = geometry.member(i) else {
                    continue;
                };
                if ring.is_empty() {
                    continue;
                }
                let mut points = Vec::new();
                let mut arcs = Vec::new();
                append_curve(ring, &mut points, &mut arcs);
                part.parts.push(points);
                part.arcs.push(arcs);
            }
            if part.parts.is_empty() {
                return;
            }
            part.has_z = geometry.is_3d();
            if !part.has_arcs() {
                part.arcs.clear();
            }
            out.push(part);
        }
        ogr::WKB_MULTI_POINT
        | ogr::WKB_MULTI_LINE_STRING
        | ogr::WKB_MULTI_POLYGON
        | ogr::WKB_MULTI_CURVE
        | ogr::WKB_MULTI_SURFACE
        | ogr::WKB_GEOMETRY_COLLECTION => {
            for i in 0..geometry.member_count() {
                flatten(geometry.member(i), out, keep_arcs, report, depth + 1);
            }
        }
        // Faces of a 3D model: Katana has no entity that is one, and a TIN
        // taken apart into triangles would be thousands of closed polylines
        // that are no longer a surface. Counted and said by name instead.
        ogr::WKB_TIN => count(report, "tin"),
        ogr::WKB_POLYHEDRAL_SURFACE => count(report, "polyhedral surface"),
        _ => count(
            report,
            &format!("unsupported {}", geometry_type_name(geometry_type)),
        ),
    }
}

/// Appends one VectorGeometry per simple member of `geometry`: a
/// multi-geometry or collection is recursed into. What cannot be represented
/// is skipped and said in `warnings`.
#[allow(
    dead_code,
    reason = "The algorithm bridge's conversion of a dataset GDAL hands back (processing.cpp), which is ported in wave L."
)]
pub(crate) fn flatten_ogr_geometry(
    geometry: Option<GeometryView<'_>>,
    out: &mut Vec<VectorGeometry>,
    warnings: &mut Vec<String>,
) {
    let mut report = VectorReadReport::default();
    flatten(geometry, out, false, &mut report, 0);
    for (what, count) in &report.skipped {
        warnings.push(format!("{count} {what} geometries were skipped"));
    }
    if report.curves_made_chords != 0 {
        warnings.push(format!(
            "{} curves were made chords at GDAL's own step",
            report.curves_made_chords
        ));
    }
}

// ---- VectorGeometry -> OGR geometry ----------------------------------------------------

/// Adds a vertex in the geometry's own dimension: OGR makes a geometry 3D the
/// moment one vertex is given a z, so a 2D one must never be handed a zero.
fn add_vertex(line: &OgrGeometry, point: &GeoPoint, has_z: bool) {
    if has_z {
        line.add_point(point.x, point.y, point.z);
    } else {
        line.add_point_2d(point.x, point.y);
    }
}

fn make_ring(points: &[GeoPoint], has_z: bool) -> OgrGeometry {
    let ring = OgrGeometry::create(ogr::WKB_LINEAR_RING);
    for point in points {
        add_vertex(&ring, point, has_z);
    }
    // OGR requires a closed ring; close it if the caller's data does not.
    if let (Some(first), Some(last)) = (points.first(), points.last())
        && (first.x != last.x || first.y != last.y)
    {
        add_vertex(&ring, first, has_z);
    }
    ring
}

/// The straight run of a curve from the last vertex taken to `end`.
fn straight_to(
    compound: &OgrGeometry,
    points: &[GeoPoint],
    has_z: bool,
    at: &mut usize,
    end: usize,
) {
    if end <= *at {
        return;
    }
    let line = OgrGeometry::create(ogr::WKB_LINE_STRING);
    for point in &points[*at..=end] {
        add_vertex(&line, point, has_z);
    }
    compound.add_geometry_directly(line);
    *at = end;
}

/// A part with arcs as the curve it was read from: straight runs as line
/// strings, each arc a CircularString of its three points, joined in a
/// CompoundCurve. A driver without curves makes its own chords of it.
fn make_curve(points: &[GeoPoint], arcs: &[usize], has_z: bool, closed: bool) -> OgrGeometry {
    let compound = OgrGeometry::create(ogr::WKB_COMPOUND_CURVE);
    let mut at = 0;
    for &arc in arcs {
        // An index that names no arc of this part (the C++'s size_t sum
        // cannot overflow there for any index a part can hold).
        if arc.checked_add(2).is_none_or(|end| end >= points.len()) || arc < at {
            continue;
        }
        straight_to(&compound, points, has_z, &mut at, arc);
        let circular = OgrGeometry::create(ogr::WKB_CIRCULAR_STRING);
        for point in &points[arc..=arc + 2] {
            add_vertex(&circular, point, has_z);
        }
        compound.add_geometry_directly(circular);
        at = arc + 2;
    }
    straight_to(&compound, points, has_z, &mut at, points.len() - 1);
    let first = &points[0];
    let last = &points[points.len() - 1];
    if closed && (first.x != last.x || first.y != last.y) {
        let closing = OgrGeometry::create(ogr::WKB_LINE_STRING);
        add_vertex(&closing, last, has_z);
        add_vertex(&closing, first, has_z);
        compound.add_geometry_directly(closing);
    }
    compound
}

/// A new OGR geometry the caller owns, in the geometry's own dimension (2D
/// stays 2D); None for a geometry with too few points for its kind.
pub(crate) fn make_ogr_geometry(geometry: &VectorGeometry) -> Option<OgrGeometry> {
    let arcs_of = |part: usize| geometry.arcs.get(part).filter(|arcs| !arcs.is_empty());
    match geometry.kind {
        GeometryKind::Point => {
            let p = geometry.parts.first()?.first()?;
            let point = OgrGeometry::create(ogr::WKB_POINT);
            if geometry.has_z {
                point.set_point(p.x, p.y, p.z);
            } else {
                point.set_point_2d(p.x, p.y);
            }
            Some(point)
        }
        GeometryKind::LineString => {
            let first = geometry.parts.first().filter(|part| part.len() >= 2)?;
            if let Some(arcs) = arcs_of(0) {
                return Some(make_curve(first, arcs, geometry.has_z, false));
            }
            let line = OgrGeometry::create(ogr::WKB_LINE_STRING);
            for p in first {
                add_vertex(&line, p, geometry.has_z);
            }
            Some(line)
        }
        GeometryKind::Polygon => {
            if geometry.parts.first().is_none_or(|part| part.len() < 3) {
                return None;
            }
            if geometry.has_arcs() {
                let polygon = OgrGeometry::create(ogr::WKB_CURVE_POLYGON);
                for (r, part) in geometry.parts.iter().enumerate() {
                    if part.len() < 3 {
                        continue;
                    }
                    let arcs = arcs_of(r).map_or(&[][..], Vec::as_slice);
                    polygon.add_geometry_directly(make_curve(part, arcs, geometry.has_z, true));
                }
                return Some(polygon);
            }
            let polygon = OgrGeometry::create(ogr::WKB_POLYGON);
            for part in &geometry.parts {
                if part.len() >= 3 {
                    polygon.add_geometry_directly(make_ring(part, geometry.has_z));
                }
            }
            Some(polygon)
        }
        GeometryKind::Unknown => None,
    }
}
