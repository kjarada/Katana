use crate::intersection::intersect;
use crate::scalar::{max, min};
use crate::{Box2, IntersectionKind, Point2, Polyline2, Segment2, Triangle2};
use katana_core::error::{Error, ErrorCode, Result};
use katana_math::numerics::tolerance;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Clockwise,
    CounterClockwise,
    Degenerate,
}
pub fn orientation(polygon: &Polyline2) -> Orientation {
    if !polygon.closed || polygon.vertices.len() < 3 {
        return Orientation::Degenerate;
    }
    let signed_area = polygon.signed_area();
    if signed_area.abs() <= tolerance::GEOMETRIC * polygon.length() {
        return Orientation::Degenerate;
    }
    if signed_area > 0.0 {
        Orientation::CounterClockwise
    } else {
        Orientation::Clockwise
    }
}
pub fn is_convex(polygon: &Polyline2) -> bool {
    let count = polygon.vertices.len();
    if !polygon.closed || count < 3 {
        return false;
    }
    let mut sign = 0;
    for index in 0..count {
        let incoming = polygon.vertices[(index + 1) % count] - polygon.vertices[index];
        let outgoing =
            polygon.vertices[(index + 2) % count] - polygon.vertices[(index + 1) % count];
        let turn = incoming.cross(outgoing);
        if turn.abs() <= tolerance::ANGULAR * incoming.length() * outgoing.length() {
            continue;
        }
        let current = if turn > 0.0 { 1 } else { -1 };
        if sign == 0 {
            sign = current;
        } else if sign != current {
            return false;
        }
    }
    sign != 0
}
pub fn is_simple(polygon: &Polyline2) -> bool {
    let count = polygon.vertices.len();
    if !polygon.closed || count < 3 {
        return false;
    }
    for index in 0..count {
        let first = polygon.segment(index);
        if first.is_degenerate() {
            return false;
        }
        let first_box = first.bounding_box().inflated(4.0 * tolerance::GEOMETRIC);
        for other in index + 1..count {
            let second = polygon.segment(other);
            if !first_box.intersects(second.bounding_box()) {
                continue;
            }
            let hit = intersect(&first, &second);
            if !hit.exists() {
                continue;
            }
            let adjacent = other == index + 1 || (index == 0 && other == count - 1);
            if !adjacent || hit.kind == IntersectionKind::Overlap {
                return false;
            }
        }
    }
    true
}
fn turns_left(hull: &[Point2], size: usize, point: Point2) -> bool {
    (hull[size - 1] - hull[size - 2]).cross(point - hull[size - 2]) > 0.0
}
pub fn convex_hull(mut points: Vec<Point2>) -> Vec<Point2> {
    points.sort_by(|first, second| {
        if first.x < second.x || (first.x == second.x && first.y < second.y) {
            std::cmp::Ordering::Less
        } else if second.x < first.x || (second.x == first.x && second.y < first.y) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let mut hull = vec![Point2::default(); 2 * points.len()];
    let mut size = 0;
    for &point in &points {
        while size >= 2 && !turns_left(&hull, size, point) {
            size -= 1;
        }
        hull[size] = point;
        size += 1;
    }
    let lower_size = size + 1;
    for index in (0..points.len() - 1).rev() {
        while size >= lower_size && !turns_left(&hull, size, points[index]) {
            size -= 1;
        }
        hull[size] = points[index];
        size += 1;
    }
    hull.truncate(size - 1);
    hull
}
fn douglas_peucker(points: &[Point2], first: usize, last: usize, epsilon: f64, keep: &mut [bool]) {
    if last <= first + 1 {
        return;
    }
    let chord = Segment2::new(points[first], points[last]);
    let mut worst = -1.0;
    let mut worst_index = first;
    for (index, &point) in points.iter().enumerate().take(last).skip(first + 1) {
        let distance = chord.distance_to(point);
        if distance > worst {
            worst = distance;
            worst_index = index;
        }
    }
    if worst > epsilon {
        keep[worst_index] = true;
        douglas_peucker(points, first, worst_index, epsilon, keep);
        douglas_peucker(points, worst_index, last, epsilon, keep);
    }
}
pub fn simplify(polyline: &Polyline2, epsilon: f64) -> Polyline2 {
    if !(epsilon > 0.0) || polyline.vertices.len() < 3 {
        return polyline.clone();
    }
    let mut points = polyline.vertices.clone();
    if polyline.closed {
        points.push(points[0]);
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    *keep.last_mut().unwrap() = true;
    douglas_peucker(&points, 0, points.len() - 1, epsilon, &mut keep);
    let count = if polyline.closed {
        points.len() - 1
    } else {
        points.len()
    };
    Polyline2::new(
        points
            .into_iter()
            .zip(keep)
            .take(count)
            .filter_map(|(point, keep)| keep.then_some(point))
            .collect(),
        polyline.closed,
    )
}
pub fn clip(segment: &Segment2, bounds: &Box2) -> Option<Segment2> {
    if bounds.empty() {
        return None;
    }
    let delta = segment.delta();
    let denominators = [-delta.x, delta.x, -delta.y, delta.y];
    let distances = [
        segment.start.x - bounds.min.x,
        bounds.max.x - segment.start.x,
        segment.start.y - bounds.min.y,
        bounds.max.y - segment.start.y,
    ];
    let mut enter = 0.0;
    let mut exit = 1.0;
    for index in 0..4 {
        if denominators[index] == 0.0 {
            if distances[index] < 0.0 {
                return None;
            }
            continue;
        }
        let ratio = distances[index] / denominators[index];
        if denominators[index] < 0.0 {
            enter = max(enter, ratio);
        } else {
            exit = min(exit, ratio);
        }
        if enter > exit {
            return None;
        }
    }
    Some(Segment2::new(
        segment.point_at(enter),
        segment.point_at(exit),
    ))
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolylineRuns {
    pub points: Vec<Point2>,
    pub ends: Vec<usize>,
}
impl PolylineRuns {
    pub fn size(&self) -> usize {
        self.ends.len()
    }
    pub fn empty(&self) -> bool {
        self.ends.is_empty()
    }
    pub fn begin(&self, run: usize) -> usize {
        if run == 0 { 0 } else { self.ends[run - 1] }
    }
    pub fn clear(&mut self) {
        self.points.clear();
        self.ends.clear();
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolylineClip {
    Cut,
    WholeSegments,
}
fn close_run(runs: &mut PolylineRuns, open: &mut bool) {
    if *open {
        runs.ends.push(runs.points.len());
        *open = false;
    }
}
pub fn clip_polyline(vertices: &[Point2], closed: bool, bounds: &Box2, runs: &mut PolylineRuns) {
    clip_polyline_with_mode(vertices, closed, bounds, runs, PolylineClip::Cut);
}
pub fn clip_polyline_with_mode(
    vertices: &[Point2],
    closed: bool,
    bounds: &Box2,
    runs: &mut PolylineRuns,
    mode: PolylineClip,
) {
    let whole = mode == PolylineClip::WholeSegments;
    runs.clear();
    let count = vertices.len();
    if count < 2 || bounds.empty() {
        return;
    }
    let mut open = false;
    let segments = if closed { count } else { count - 1 };
    for index in 0..segments {
        let first = vertices[index];
        let last = vertices[(index + 1) % count];
        let delta_east = last.x - first.x;
        let delta_north = last.y - first.y;
        let denominators = [-delta_east, delta_east, -delta_north, delta_north];
        let distances = [
            first.x - bounds.min.x,
            bounds.max.x - first.x,
            first.y - bounds.min.y,
            bounds.max.y - first.y,
        ];
        let mut enter = 0.0;
        let mut exit = 1.0;
        let mut outside = false;
        for edge in 0..4 {
            if outside {
                break;
            }
            if denominators[edge] == 0.0 {
                outside = distances[edge] < 0.0;
                continue;
            }
            let ratio = distances[edge] / denominators[edge];
            if denominators[edge] < 0.0 {
                enter = max(enter, ratio);
            } else {
                exit = min(exit, ratio);
            }
            outside = enter > exit;
        }
        if outside {
            close_run(runs, &mut open);
            continue;
        }
        if whole {
            if !open {
                runs.points.push(first);
                open = true;
            }
            runs.points.push(last);
            continue;
        }
        let start = if enter == 0.0 {
            first
        } else {
            Point2::new(first.x + enter * delta_east, first.y + enter * delta_north)
        };
        let end = if exit == 1.0 {
            last
        } else {
            Point2::new(first.x + exit * delta_east, first.y + exit * delta_north)
        };
        if !(open && enter == 0.0) {
            close_run(runs, &mut open);
            runs.points.push(start);
            open = true;
        }
        runs.points.push(end);
        if exit != 1.0 {
            close_run(runs, &mut open);
        }
    }
    close_run(runs, &mut open);
}
pub fn clip_polygon(subject: &Polyline2, clip_region: &Polyline2) -> Result<Polyline2> {
    if !subject.closed {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "subject polygon must be closed",
        ));
    }
    if !is_convex(clip_region) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "clip region must be a convex polygon",
        ));
    }
    let clip_ccw = if orientation(clip_region) == Orientation::Clockwise {
        clip_region.reversed()
    } else {
        clip_region.clone()
    };
    let mut output = subject.vertices.clone();
    let mut input = Vec::new();
    let clip_count = clip_ccw.vertices.len();
    for edge in 0..clip_count {
        if output.is_empty() {
            break;
        }
        let edge_start = clip_ccw.vertices[edge];
        let edge_direction = clip_ccw.vertices[(edge + 1) % clip_count] - edge_start;
        std::mem::swap(&mut input, &mut output);
        output.clear();
        for index in 0..input.len() {
            let current = input[index];
            let previous = input[(index + input.len() - 1) % input.len()];
            let current_side = edge_direction.cross(current - edge_start);
            let previous_side = edge_direction.cross(previous - edge_start);
            let current_inside = current_side >= 0.0;
            let previous_inside = previous_side >= 0.0;
            if current_inside != previous_inside {
                let parameter = previous_side / (previous_side - current_side);
                output.push(previous + (current - previous) * parameter);
            }
            if current_inside {
                output.push(current);
            }
        }
    }
    Ok(Polyline2::new(output, true).without_duplicate_vertices())
}
pub type TriangleIndices = [usize; 3];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollinearVertices {
    Skip,
    Keep,
}
pub fn triangulate(polygon: &Polyline2) -> Result<Vec<TriangleIndices>> {
    triangulate_with_collinear(polygon, CollinearVertices::Skip)
}
pub fn triangulate_with_collinear(
    polygon: &Polyline2,
    collinear: CollinearVertices,
) -> Result<Vec<TriangleIndices>> {
    let count = polygon.vertices.len();
    if !polygon
        .vertices
        .iter()
        .all(|vertex| vertex.x.is_finite() && vertex.y.is_finite())
    {
        return Err(Error::new(
            ErrorCode::TriangulationFailure,
            "polygon has a vertex that is not finite",
        )
        .with_context(format!("vertices={count}")));
    }
    let winding = orientation(polygon);
    if winding == Orientation::Degenerate {
        return Err(Error::new(
            ErrorCode::TriangulationFailure,
            "polygon is open, has fewer than 3 vertices or has no area",
        )
        .with_context(format!("vertices={count}")));
    }
    let mut ring: Vec<usize> = (0..count).collect();
    if winding == Orientation::Clockwise {
        ring.reverse();
    }
    let vertices = &polygon.vertices;
    let mut triangles = Vec::with_capacity(count - 2);
    let passes = if collinear == CollinearVertices::Keep {
        2
    } else {
        1
    };
    while ring.len() > 3 {
        let mut clipped = false;
        for pass in 0..passes {
            if clipped {
                break;
            }
            for index in 0..ring.len() {
                let previous = ring[(index + ring.len() - 1) % ring.len()];
                let current = ring[index];
                let next = ring[(index + 1) % ring.len()];
                let incoming = vertices[current] - vertices[previous];
                let outgoing = vertices[next] - vertices[current];
                let turn = incoming.cross(outgoing);
                let turn_scale = tolerance::ANGULAR * incoming.length() * outgoing.length();
                if turn.abs() <= turn_scale {
                    if collinear == CollinearVertices::Skip {
                        ring.remove(index);
                        clipped = true;
                        break;
                    }
                    if pass == 0 || incoming.dot(outgoing) < 0.0 {
                        continue;
                    }
                    triangles.push([previous, current, next]);
                    ring.remove(index);
                    clipped = true;
                    break;
                }
                if turn < 0.0 {
                    continue;
                }
                let mut blocked = false;
                for &other in &ring {
                    if other == previous
                        || other == current
                        || other == next
                        || vertices[other] == vertices[previous]
                        || vertices[other] == vertices[current]
                        || vertices[other] == vertices[next]
                    {
                        continue;
                    }
                    let inside = (vertices[current] - vertices[previous])
                        .cross(vertices[other] - vertices[previous])
                        >= 0.0
                        && (vertices[next] - vertices[current])
                            .cross(vertices[other] - vertices[current])
                            >= 0.0
                        && (vertices[previous] - vertices[next])
                            .cross(vertices[other] - vertices[next])
                            >= 0.0;
                    if inside {
                        blocked = true;
                        break;
                    }
                }
                if blocked {
                    continue;
                }
                triangles.push([previous, current, next]);
                ring.remove(index);
                clipped = true;
                break;
            }
        }
        if !clipped {
            return Err(Error::new(
                ErrorCode::TriangulationFailure,
                "no ear found; the polygon is not simple",
            )
            .with_context(format!("remaining={}", ring.len())));
        }
    }
    let last = Triangle2::new(vertices[ring[0]], vertices[ring[1]], vertices[ring[2]]);
    if collinear == CollinearVertices::Keep || !last.is_degenerate() {
        triangles.push([ring[0], ring[1], ring[2]]);
    }
    Ok(triangles)
}

pub const MAX_HATCH_LINES: usize = 100000;
pub fn hatch_lines(boundary: &Polyline2, angle: f64, spacing: f64) -> Result<Vec<Segment2>> {
    hatch_lines_with_offset(boundary, angle, spacing, 0.0)
}
pub fn hatch_lines_with_offset(
    boundary: &Polyline2,
    angle: f64,
    spacing: f64,
    offset: f64,
) -> Result<Vec<Segment2>> {
    if !boundary.closed || boundary.vertices.len() < 3 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a hatch boundary must be a closed polygon of at least three vertices",
        ));
    }
    if !angle.is_finite() || !offset.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "hatch angle and offset must be finite",
        ));
    }
    if !(spacing > 0.0) || !spacing.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "hatch spacing must be a positive length",
        ));
    }
    for vertex in &boundary.vertices {
        if !vertex.x.is_finite() || !vertex.y.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "a hatch boundary must have finite coordinates",
            ));
        }
    }
    let along_east = angle.cos();
    let along_north = angle.sin();
    let normal_east = -along_north;
    let normal_north = along_east;
    let mut lowest = 0.0;
    let mut highest = 0.0;
    for (index, vertex) in boundary.vertices.iter().enumerate() {
        let across = vertex.x * normal_east + vertex.y * normal_north;
        lowest = if index == 0 {
            across
        } else {
            min(lowest, across)
        };
        highest = if index == 0 {
            across
        } else {
            max(highest, across)
        };
    }
    let first_exact = ((lowest - offset) / spacing).ceil();
    let last_exact = ((highest - offset) / spacing).floor();
    if !(last_exact >= first_exact) {
        return Ok(Vec::new());
    }
    let count_exact = last_exact - first_exact + 1.0;
    if !(count_exact <= MAX_HATCH_LINES as f64) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "hatch spacing is too fine for this boundary: {} lines, the limit is {MAX_HATCH_LINES}",
                count_exact as i64
            ),
        ));
    }
    let first = first_exact as i64;
    let count = count_exact as i64;
    let mut segments = Vec::new();
    let mut crossings = Vec::new();
    for index in 0..count {
        let across = offset + (first + index) as f64 * spacing;
        crossings.clear();
        for edge in 0..boundary.vertices.len() {
            let start = boundary.vertices[edge];
            let end = boundary.vertices[(edge + 1) % boundary.vertices.len()];
            let start_side = start.x * normal_east + start.y * normal_north - across;
            let end_side = end.x * normal_east + end.y * normal_north - across;
            let upward = start_side <= 0.0 && end_side > 0.0;
            let downward = end_side <= 0.0 && start_side > 0.0;
            if !upward && !downward {
                continue;
            }
            let parameter = start_side / (start_side - end_side);
            let east = start.x + parameter * (end.x - start.x);
            let north = start.y + parameter * (end.y - start.y);
            crossings.push(east * along_east + north * along_north);
        }
        if crossings.len() < 2 {
            continue;
        }
        crossings.sort_by(|first, second| {
            first
                .partial_cmp(second)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for pair in crossings.as_chunks::<2>().0 {
            let first = pair[0];
            let second = pair[1];
            if !(second > first) {
                continue;
            }
            segments.push(Segment2::new(
                Point2::new(
                    first * along_east + across * normal_east,
                    first * along_north + across * normal_north,
                ),
                Point2::new(
                    second * along_east + across * normal_east,
                    second * along_north + across * normal_north,
                ),
            ));
        }
    }
    Ok(segments)
}
