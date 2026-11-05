use crate::curves2d::{arc_from_bulge, bulge_from_sweep};
use crate::editing::Offset;
use crate::scalar::max;
use crate::{CurvePolyline2, CurveVertex, Point2, Vec2};
use katana_core::error::{Error, ErrorCode, Result};
use katana_core::text::format_exact_real;
use katana_math::numerics::{PI, TWO_PI, normalize_angle, tolerance};

#[derive(Clone, Copy, Default)]
struct Moved {
    arc: bool,
    start: Point2,
    end: Point2,
    direction: Vec2,
    center: Point2,
    radius: f64,
    sweep: f64,
}
fn line_line(first: &Moved, second: &Moved) -> Vec<Point2> {
    let denominator = first.direction.cross(second.direction);
    if denominator.abs()
        <= tolerance::ANGULAR * first.direction.length() * second.direction.length()
    {
        return Vec::new();
    }
    let parameter = (second.start - first.start).cross(second.direction) / denominator;
    vec![first.start + first.direction * parameter]
}
fn line_circle(line: &Moved, circle: &Moved) -> Vec<Point2> {
    let direction = line.direction.normalized();
    let from = line.start - circle.center;
    let linear = from.dot(direction);
    let constant = from.dot(from) - circle.radius * circle.radius;
    let discriminant = linear * linear - constant;
    if discriminant < -tolerance::GEOMETRIC * max(1.0, circle.radius) {
        return Vec::new();
    }
    let root = max(0.0, discriminant).sqrt();
    vec![
        line.start + direction * (-linear - root),
        line.start + direction * (-linear + root),
    ]
}
fn circle_circle(first: &Moved, second: &Moved) -> Vec<Point2> {
    let between = second.center - first.center;
    let distance = between.length();
    if !(distance > tolerance::GEOMETRIC)
        || distance > first.radius + second.radius + tolerance::GEOMETRIC
        || distance < (first.radius - second.radius).abs() - tolerance::GEOMETRIC
    {
        return Vec::new();
    }
    let along = (first.radius * first.radius - second.radius * second.radius + distance * distance)
        / (2.0 * distance);
    let across = max(0.0, first.radius * first.radius - along * along).sqrt();
    let unit = between / distance;
    let foot = first.center + unit * along;
    vec![
        foot + unit.perpendicular() * across,
        foot - unit.perpendicular() * across,
    ]
}
fn crossings(first: &Moved, second: &Moved) -> Vec<Point2> {
    if !first.arc && !second.arc {
        line_line(first, second)
    } else if !first.arc {
        line_circle(first, second)
    } else if !second.arc {
        line_circle(second, first)
    } else {
        circle_circle(first, second)
    }
}
fn arc_bulge(piece: &Moved, from: Point2, to: Point2) -> Option<f64> {
    let first = (from - piece.center).angle();
    let last = (to - piece.center).angle();
    let mut sweep = if piece.sweep > 0.0 {
        normalize_angle(last - first)
    } else {
        -normalize_angle(first - last)
    };
    if piece.sweep.abs() > TWO_PI - 1e-9 && sweep.abs() < 1e-12 {
        sweep = piece.sweep;
    }
    if (sweep - piece.sweep).abs() > PI {
        None
    } else {
        Some(bulge_from_sweep(sweep))
    }
}
#[derive(Clone, Copy, Default)]
struct Join {
    first: Point2,
    second: Option<Point2>,
    round_bulge: f64,
}

pub fn offset(polyline: &CurvePolyline2, distance: f64) -> Result<CurvePolyline2> {
    if !distance.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the offset distance is not finite",
        ));
    }
    let count = polyline.segment_count();
    if count == 0 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a polyline needs a segment to offset",
        ));
    }
    let mut pieces = Vec::with_capacity(count);
    for index in 0..count {
        let first = polyline.vertices[index].position;
        let last = polyline.vertices[polyline.segment_end(index)].position;
        let mut piece = Moved::default();
        if let Some(arc) = arc_from_bulge(first, last, polyline.vertices[index].bulge) {
            piece.arc = true;
            piece.center = arc.center;
            piece.sweep = arc.sweep;
            piece.radius = if arc.sweep > 0.0 {
                arc.radius - distance
            } else {
                arc.radius + distance
            };
            if !(piece.radius > tolerance::GEOMETRIC) {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    format!(
                        "an offset of {} is more than the radius of arc segment {index}; the arc would vanish",
                        format_exact_real(distance.abs())
                    ),
                ));
            }
            piece.start = arc.center + (first - arc.center).normalized() * piece.radius;
            piece.end = arc.center + (last - arc.center).normalized() * piece.radius;
        } else {
            let along = last - first;
            if !(along.length() > tolerance::GEOMETRIC) {
                piece.direction = Vec2::default();
                piece.start = first;
                piece.end = last;
                pieces.push(piece);
                continue;
            }
            let left = along.normalized().perpendicular() * distance;
            piece.direction = along;
            piece.start = first + left;
            piece.end = last + left;
        }
        pieces.push(piece);
    }
    let join_at = |vertex: usize, incoming: &Moved, outgoing: &Moved| -> Join {
        if incoming.start.distance_to(incoming.end) <= 0.0
            && !incoming.arc
            && incoming.direction == Vec2::default()
        {
            return Join {
                first: outgoing.start,
                ..Default::default()
            };
        }
        if incoming.end.distance_to(outgoing.start) <= tolerance::GEOMETRIC {
            return Join {
                first: incoming.end,
                ..Default::default()
            };
        }
        let middle = (incoming.end + outgoing.start) * 0.5;
        let mut best = None;
        let mut best_distance = f64::INFINITY;
        for point in crossings(incoming, outgoing) {
            let separation = point.distance_to(middle);
            if separation < best_distance {
                best_distance = separation;
                best = Some(point);
            }
        }
        let gap = incoming.end.distance_to(outgoing.start);
        if let Some(best) = best
            && best_distance <= 4.0 * max(gap, distance.abs())
        {
            return Join {
                first: best,
                ..Default::default()
            };
        }
        let position = polyline.vertices[vertex].position;
        let from = incoming.end - position;
        let to = outgoing.start - position;
        Join {
            first: incoming.end,
            second: Some(outgoing.start),
            round_bulge: bulge_from_sweep(from.cross(to).atan2(from.dot(to))),
        }
    };
    let vertex_count = polyline.vertices.len();
    let mut joins = vec![Join::default(); vertex_count];
    for (vertex, join) in joins.iter_mut().enumerate() {
        let interior = polyline.closed || (vertex > 0 && vertex + 1 < vertex_count);
        if !interior {
            *join = Join {
                first: if vertex == 0 {
                    pieces[0].start
                } else {
                    pieces[count - 1].end
                },
                ..Default::default()
            };
            continue;
        }
        let incoming = if vertex == 0 { count - 1 } else { vertex - 1 };
        *join = join_at(vertex, &pieces[incoming], &pieces[vertex % count]);
    }
    let mut output = CurvePolyline2 {
        closed: polyline.closed,
        ..Default::default()
    };
    for vertex in 0..vertex_count {
        let height = polyline.vertices[vertex].height;
        let join = joins[vertex];
        let start_of_next = join.second.unwrap_or(join.first);
        let mut next_bulge = 0.0;
        if vertex < count {
            let piece = pieces[vertex];
            if piece.arc {
                let end_of_piece = joins[polyline.segment_end(vertex)].first;
                next_bulge = arc_bulge(&piece, start_of_next, end_of_piece).ok_or_else(|| {
                    Error::new(
                        ErrorCode::InvalidGeometry,
                        format!(
                            "an offset of {} consumes arc segment {vertex}",
                            format_exact_real(distance.abs())
                        ),
                    )
                })?;
            }
        }
        if let Some(second) = join.second {
            output.vertices.push(CurveVertex {
                position: join.first,
                bulge: join.round_bulge,
                height,
            });
            output.vertices.push(CurveVertex {
                position: second,
                bulge: next_bulge,
                height,
            });
        } else {
            output.vertices.push(CurveVertex {
                position: join.first,
                bulge: next_bulge,
                height,
            });
        }
    }
    if !output.closed {
        output.vertices.last_mut().unwrap().bulge = 0.0;
    }
    Ok(output)
}
impl Offset for CurvePolyline2 {
    type Output = Result<CurvePolyline2>;
    fn offset(&self, distance: f64) -> Self::Output {
        offset(self, distance)
    }
}
