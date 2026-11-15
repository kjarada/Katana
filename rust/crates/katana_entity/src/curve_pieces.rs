//! A geometry as the primitive curves the editing kernel understands
//! (Curve2: segments, arcs, circles), for the code that works on "the curves
//! of an entity" without caring which kind it is - intersection and nearest
//! snaps, section crossings, trim and extend boundaries, the surface builder's
//! breaklines.
//!
//! It exists because docs/model.md lists those places as the ones where a new
//! geometry kind is SILENTLY absent, and three kinds arrived at once
//! (docs/drawing.md). Rather than teach each place three more cases, each asks
//! this, and a kind is added here once:
//!
//!   * a Polyline2's and a CurvePolyline2's segments and arcs, exactly;
//!   * an Ellipse2 and a Spline2 as straight chords within `tolerance`, since
//!     Curve2 has no ellipse or spline - an intersection with one is then good
//!     to that tolerance, which the default (a millimetre, CURVE_CHORD_TOLERANCE)
//!     makes finer than any drawing resolves;
//!   * a segment, arc or circle as itself; everything else (points, text,
//!     annotation) as nothing.

use katana_geometry::chording::chord_arc;
use katana_geometry::{Curve2, CurveSegment, Point2, Segment2};
use katana_math::numerics::tolerance;

use crate::entity::Geometry;

fn append_chords(chain: &[Point2], out: &mut Vec<Curve2>) {
    for pair in chain.windows(2) {
        if pair[0].distance_to(pair[1]) > tolerance::GEOMETRIC {
            out.push(Curve2::Segment(Segment2::new(pair[0], pair[1])));
        }
    }
}

/// The pieces of `geometry`; pass CURVE_CHORD_TOLERANCE for the C++ default.
pub fn curve_pieces(geometry: &Geometry, tolerance: f64) -> Vec<Curve2> {
    let mut out = Vec::new();
    match geometry {
        Geometry::Line(segment) => out.push(Curve2::Segment(*segment)),
        Geometry::Arc(arc) => out.push(Curve2::Arc(*arc)),
        Geometry::Circle(circle) => out.push(Curve2::Circle(*circle)),
        Geometry::Polyline(polyline) => {
            for i in 0..polyline.segment_count() {
                out.push(Curve2::Segment(polyline.segment(i)));
            }
        }
        Geometry::CurvePolyline(curved) => {
            for i in 0..curved.segment_count() {
                out.push(match curved.segment(i) {
                    CurveSegment::Segment(segment) => Curve2::Segment(segment),
                    CurveSegment::Arc(arc) => Curve2::Arc(arc),
                });
            }
        }
        Geometry::Ellipse(ellipse) => append_chords(&ellipse.tessellate(tolerance), &mut out),
        Geometry::Spline(spline) => append_chords(&spline.tessellate(tolerance), &mut out),
        _ => {}
    }
    out
}

/// True for the kinds whose pieces are exact (not chords).
pub fn pieces_are_exact(geometry: &Geometry) -> bool {
    !matches!(geometry, Geometry::Ellipse(_) | Geometry::Spline(_))
}

/// The points of a geometry as a walk, for code that wants a polyline: the
/// vertices of a polyline, the chords of a curve polyline's arcs, of an
/// ellipse and of a spline; empty for kinds that are not linework. Pass
/// CURVE_CHORD_TOLERANCE for the C++ default.
pub fn linework(geometry: &Geometry, tolerance: f64) -> Vec<Point2> {
    match geometry {
        Geometry::Line(segment) => vec![segment.start, segment.end],
        Geometry::Polyline(polyline) => {
            let mut out = polyline.vertices.clone();
            if polyline.closed && !out.is_empty() {
                out.push(out[0]);
            }
            out
        }
        Geometry::CurvePolyline(curved) => curved.tessellate(tolerance),
        Geometry::Ellipse(ellipse) => ellipse.tessellate(tolerance),
        Geometry::Spline(spline) => spline.tessellate(tolerance),
        Geometry::Arc(arc) => chord_arc(arc, tolerance),
        _ => Vec::new(),
    }
}
