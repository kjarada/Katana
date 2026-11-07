//! Resolving an AnchorRef (entity.rs): the point on another entity that a
//! dimension, a leader or a label follows. One function, because the
//! associative update, the command line's "#12.end" points and the label
//! values all ask the same question and must get the same answer.

use katana_core::text::format_exact_real;
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::{CurveSegment, Point2, Polyline2};
use katana_math::numerics::{TWO_PI, normalize_angle};

use crate::entity::{AnchorPoint, AnchorRef, Entity, Geometry, to_string};
use crate::scalar;

fn clamped_parameter(reference: &AnchorRef) -> f64 {
    if reference.parameter.is_finite() {
        scalar::clamp(reference.parameter, 0.0, 1.0)
    } else {
        0.0
    }
}

/// A point inside a closed figure: the centroid when it is inside (every
/// convex lot, most others), otherwise the middle of the widest run inside
/// the figure along the horizontal through the centroid - an L-shaped lot's
/// centroid can be in the notch. Where an area label and a leader's Inside
/// tip go. Deterministic. `figure` is closed and has at least three vertices.
pub fn inside_point(figure: &Polyline2) -> Point2 {
    let centroid = figure.centroid();
    if let Some(centroid) = centroid
        && figure.contains(centroid)
    {
        return centroid;
    }
    let through = centroid.unwrap_or(figure.vertices[0]);
    let mut crossings = Vec::new();
    let n = figure.vertices.len();
    for i in 0..n {
        let a = figure.vertices[i];
        let b = figure.vertices[(i + 1) % n];
        if (a.y > through.y) != (b.y > through.y) {
            let t = (through.y - a.y) / (b.y - a.y);
            crossings.push(a.x + t * (b.x - a.x));
        }
    }
    // std::sort; total_cmp, because a slice sort may panic on an order that is
    // not total. It differs from the C++'s < only in how it places a NaN and
    // the two zeros, and neither changes a run's width.
    crossings.sort_by(f64::total_cmp);
    let mut best_width = -1.0;
    let mut best = through;
    let mut i = 0;
    while i + 1 < crossings.len() {
        let width = crossings[i + 1] - crossings[i];
        if width > best_width {
            best_width = width;
            best = Point2::new(0.5 * (crossings[i] + crossings[i + 1]), through.y);
        }
        i += 2;
    }
    best
}

/// The point `reference` names on `entity` (whose id is not checked against
/// reference.entity), or None when that kind of entity offers no such point -
/// a circle has no start, a line no vertex 7 - so a dimension keeps its last
/// point rather than jumping to an invented one.
///
///   Point      Position
///   Line       Start, End, Mid, Along
///   Arc        Start, End, Mid, Centre, Along
///   Circle     Centre, Along, Inside (its centre)
///   Polyline   Start, End, Vertex i, SegmentMid i (the closing segment of a
///              closed polyline is the last one), Along (segment i), Inside
///              (a closed one of three vertices or more)
///   Text       Position
///   Leader     Start (its tip), End (its last vertex)
///
/// Along's parameter is clamped to [0, 1]; one that is not a number, or is
/// infinite, reads as 0, the start - a stored reference is never refused for
/// it.
pub fn resolve_anchor(entity: &Entity, reference: &AnchorRef) -> Option<Point2> {
    match &entity.geometry {
        Geometry::Point(point) => {
            (reference.point == AnchorPoint::Position).then_some(point.position)
        }
        Geometry::Line(line) => match reference.point {
            AnchorPoint::Start => Some(line.start),
            AnchorPoint::End => Some(line.end),
            AnchorPoint::Mid => Some(line.start + (line.end - line.start) * 0.5),
            AnchorPoint::Along => Some(line.point_at(clamped_parameter(reference))),
            _ => None,
        },
        Geometry::Arc(arc) => match reference.point {
            AnchorPoint::Start => Some(arc.point_at(0.0)),
            AnchorPoint::End => Some(arc.point_at(1.0)),
            AnchorPoint::Mid => Some(arc.point_at(0.5)),
            AnchorPoint::Centre => Some(arc.center),
            AnchorPoint::Along => Some(arc.point_at(clamped_parameter(reference))),
            _ => None,
        },
        Geometry::Circle(circle) => match reference.point {
            AnchorPoint::Centre | AnchorPoint::Inside => Some(circle.center),
            AnchorPoint::Along => {
                Some(circle.point_at_angle(clamped_parameter(reference) * TWO_PI))
            }
            _ => None,
        },
        Geometry::Polyline(polyline) => {
            let v = &polyline.vertices;
            let (first, last) = (v.first()?, v.last()?);
            let segments = polyline.segment_count();
            match reference.point {
                AnchorPoint::Start => Some(*first),
                AnchorPoint::End => Some(*last),
                AnchorPoint::Vertex => v.get(reference.index as usize).copied(),
                AnchorPoint::SegmentMid | AnchorPoint::Along => {
                    if reference.index as usize >= segments {
                        return None;
                    }
                    let t = if reference.point == AnchorPoint::SegmentMid {
                        0.5
                    } else {
                        clamped_parameter(reference)
                    };
                    Some(polyline.segment(reference.index as usize).point_at(t))
                }
                AnchorPoint::Inside => {
                    if !polyline.closed || v.len() < 3 {
                        return None;
                    }
                    Some(inside_point(polyline))
                }
                _ => None,
            }
        }
        Geometry::Text(text) => (reference.point == AnchorPoint::Position).then_some(text.position),
        Geometry::Dimension(_) | Geometry::Label(_) => None,
        // The C++ reads front() and back() unchecked: a leader in the model has
        // at least two vertices.
        Geometry::Leader(leader) => match reference.point {
            AnchorPoint::Start => leader.vertices.first().copied(),
            AnchorPoint::End => leader.vertices.last().copied(),
            _ => None,
        },
        // The drawing system's kinds (docs/drawing.md): a curve polyline as a
        // polyline, its segment middles on the arcs; an ellipse's and a
        // spline's ends and an ellipse's centre.
        Geometry::CurvePolyline(polyline) => {
            let v = &polyline.vertices;
            let (first, last) = (v.first()?, v.last()?);
            match reference.point {
                AnchorPoint::Start => Some(first.position),
                AnchorPoint::End => Some(last.position),
                AnchorPoint::Vertex => v
                    .get(reference.index as usize)
                    .map(|vertex| vertex.position),
                AnchorPoint::SegmentMid | AnchorPoint::Along => {
                    if reference.index as usize >= polyline.segment_count() {
                        return None;
                    }
                    // On an arc segment, the fraction of its sweep: ON the arc.
                    let t = if reference.point == AnchorPoint::SegmentMid {
                        0.5
                    } else {
                        clamped_parameter(reference)
                    };
                    Some(polyline.segment(reference.index as usize).point_at(t))
                }
                AnchorPoint::Inside => {
                    if !polyline.closed || v.len() < 3 {
                        return None;
                    }
                    // Its chords to a millimetre: the inside point is a place
                    // for a note, not a measurement.
                    Some(inside_point(&polyline.to_polyline(CURVE_CHORD_TOLERANCE)))
                }
                _ => None,
            }
        }
        Geometry::Ellipse(ellipse) => match reference.point {
            AnchorPoint::Centre => Some(ellipse.center),
            AnchorPoint::Start => Some(ellipse.start_point()),
            AnchorPoint::End => Some(ellipse.end_point()),
            // The fraction of its sweep of eccentric anomaly - of a turn, for
            // a whole ellipse, as for a circle.
            AnchorPoint::Along => Some(ellipse.point_at_parameter(
                ellipse.start_parameter + clamped_parameter(reference) * ellipse.sweep,
            )),
            AnchorPoint::Inside => ellipse.is_full().then_some(ellipse.center),
            _ => None,
        },
        Geometry::Spline(spline) => {
            if spline.check_structure().is_err() {
                return None;
            }
            match reference.point {
                AnchorPoint::Start => Some(spline.start_point()),
                AnchorPoint::End => Some(spline.end_point()),
                _ => None,
            }
        }
    }
}

/// The point of `entity` nearest `near`, NAMED so that it follows the entity:
/// Along for a line, an arc, a circle, a polyline and a curve polyline (on a
/// polyline, the segment it is nearest - the first of equals - and how far
/// along it; on a curve polyline's arc segment, the fraction of its sweep),
/// and an ellipse (the fraction of its sweep of eccentric anomaly), Position
/// for a point and a text. None for a dimension, a label or a leader, which
/// offer no place to be attached to, for a spline (its parameter is not its
/// length; its ends are Start and End), and for a polyline with no segment of
/// any length. The reference names entity.id.
pub fn nearest_anchor(entity: &Entity, near: Point2) -> Option<AnchorRef> {
    let mut reference = AnchorRef {
        entity: entity.id,
        point: AnchorPoint::Along,
        ..AnchorRef::default()
    };
    match &entity.geometry {
        Geometry::Point(_) | Geometry::Text(_) => {
            reference.point = AnchorPoint::Position;
            Some(reference)
        }
        Geometry::Line(line) => {
            reference.parameter = line.parameter_of(near);
            Some(reference)
        }
        Geometry::Arc(arc) => {
            let radial = near - arc.center;
            reference.parameter = arc.parameter_of_angle(radial.y.atan2(radial.x));
            Some(reference)
        }
        Geometry::Circle(circle) => {
            let radial = near - circle.center;
            // A turn is [0, 1): the angle 2 pi is the angle 0, and a fraction of
            // exactly 1 would describe the same point twice.
            let turn = normalize_angle(radial.y.atan2(radial.x)) / TWO_PI;
            reference.parameter = if turn < 1.0 { turn } else { 0.0 };
            Some(reference)
        }
        Geometry::Polyline(polyline) => {
            let mut best = f64::INFINITY;
            let mut found = false;
            for i in 0..polyline.segment_count() {
                let segment = polyline.segment(i);
                if segment.is_degenerate() {
                    continue; // a repeated vertex has no "along"
                }
                let distance = segment.distance_to(near);
                if distance < best {
                    best = distance;
                    found = true;
                    reference.index = i as u32;
                    reference.parameter = segment.parameter_of(near);
                }
            }
            found.then_some(reference)
        }
        Geometry::CurvePolyline(curve) => {
            let nearest = curve.nearest(near)?;
            if curve.segment_count() == 0 || curve.segment_length(nearest.segment) <= 0.0 {
                return None;
            }
            reference.index = nearest.segment as u32;
            reference.parameter = match curve.segment(nearest.segment) {
                CurveSegment::Segment(segment) => {
                    scalar::clamp(segment.parameter_of(near), 0.0, 1.0)
                }
                CurveSegment::Arc(arc) => {
                    let radial = nearest.point - arc.center;
                    scalar::clamp(arc.parameter_of_angle(radial.y.atan2(radial.x)), 0.0, 1.0)
                }
            };
            Some(reference)
        }
        Geometry::Ellipse(ellipse) => {
            let along = (ellipse.closest_parameter(near) - ellipse.start_parameter) / ellipse.sweep;
            reference.parameter = if ellipse.is_full() {
                // A turn is [0, 1), as for a circle.
                let turn = along - along.floor();
                if turn < 1.0 { turn } else { 0.0 }
            } else {
                scalar::clamp(along, 0.0, 1.0)
            };
            Some(reference)
        }
        // A spline offers no place along it: its parameter is not its length,
        // and no fraction of it would stay where the note was put when its
        // points move. Its ends are named by Start and End.
        Geometry::Spline(_) | Geometry::Dimension(_) | Geometry::Label(_) | Geometry::Leader(_) => {
            None
        }
    }
}

/// "end", "vertex 3", "segment-mid 2", "along 1 0.25": what LIST and the
/// command line say.
pub fn describe(reference: &AnchorRef) -> String {
    let mut text = String::from(to_string(reference.point));
    if reference.point == AnchorPoint::Vertex || reference.point == AnchorPoint::SegmentMid {
        text.push_str(&format!(" {}", reference.index));
    } else if reference.point == AnchorPoint::Along {
        // Exact, so what LIST prints reads back as the same place.
        text.push_str(&format!(
            " {} {}",
            reference.index,
            format_exact_real(reference.parameter)
        ));
    }
    text
}
