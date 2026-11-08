pub use crate::curve_offset::offset;
use crate::primitives2d::{Arc2, Box2, Point2, Polyline2, Segment2, Vec2};
use katana_core::error::{Error, ErrorCode, Result, Status};
use katana_math::cpp::max;
use katana_math::numerics::{HALF_PI, PI, TWO_PI, normalize_angle, tolerance};

pub const CURVE_CHORD_TOLERANCE: f64 = 1.0e-3;
const STRAIGHT_BULGE: f64 = 1.0e-12;

fn subdivide(
    evaluate: &impl Fn(f64) -> Point2,
    first: (f64, Point2),
    last: (f64, Point2),
    epsilon: f64,
    depth: u32,
    output: &mut Vec<Point2>,
) {
    let middle = 0.5 * (first.0 + last.0);
    let midpoint = evaluate(middle);
    let chord = Segment2::new(first.1, last.1);
    let mut flat = chord.distance_to(midpoint) <= epsilon;
    if flat {
        flat = chord.distance_to(evaluate(0.5 * (first.0 + middle))) <= epsilon
            && chord.distance_to(evaluate(0.5 * (middle + last.0))) <= epsilon;
    }
    if flat || depth >= 14 {
        output.push(last.1);
        return;
    }
    subdivide(
        evaluate,
        first,
        (middle, midpoint),
        epsilon,
        depth + 1,
        output,
    );
    subdivide(
        evaluate,
        (middle, midpoint),
        last,
        epsilon,
        depth + 1,
        output,
    );
}

fn tessellate_range(
    evaluate: &impl Fn(f64) -> Point2,
    first: f64,
    last: f64,
    pieces: i32,
    epsilon: f64,
    output: &mut Vec<Point2>,
) {
    let safe_tolerance = max(epsilon, 1.0e-9);
    let mut previous = evaluate(first);
    if output.is_empty() {
        output.push(previous);
    }
    for index in 1..=pieces {
        let from = first + (last - first) * (index - 1) as f64 / pieces as f64;
        let to = if index == pieces {
            last
        } else {
            first + (last - first) * index as f64 / pieces as f64
        };
        let next = evaluate(to);
        subdivide(
            evaluate,
            (from, previous),
            (to, next),
            safe_tolerance,
            0,
            output,
        );
        previous = next;
    }
}

fn chain_length(chain: &[Point2]) -> f64 {
    let mut total = 0.0;
    for pair in chain.windows(2) {
        total += pair[0].distance_to(pair[1]);
    }
    total
}

pub fn bulge_from_sweep(sweep: f64) -> f64 {
    (sweep / 4.0).tan()
}
pub fn arc_from_bulge(start: Point2, end: Point2, bulge: f64) -> Option<Arc2> {
    let chord = end - start;
    let length = chord.length();
    if !(bulge.abs() > STRAIGHT_BULGE) || !(length > tolerance::GEOMETRIC) || !bulge.is_finite() {
        return None;
    }
    let radius = length * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
    let offset = length * (1.0 - bulge * bulge) / (4.0 * bulge);
    let center = (start + end) * 0.5 + chord.perpendicular() * (offset / length);
    Some(Arc2::new(
        center,
        radius,
        (start - center).angle(),
        4.0 * bulge.atan(),
    ))
}
pub fn bulge_through(start: Point2, through: Point2, end: Point2) -> f64 {
    Arc2::through_points(start, through, end).map_or(0.0, |arc| bulge_from_sweep(arc.sweep))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurveVertex {
    pub position: Point2,
    pub bulge: f64,
    pub height: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CurveSegment {
    Segment(Segment2),
    Arc(Arc2),
}
impl CurveSegment {
    pub fn length(&self) -> f64 {
        match self {
            Self::Segment(piece) => piece.length(),
            Self::Arc(piece) => piece.length(),
        }
    }
    pub fn point_at(&self, parameter: f64) -> Point2 {
        match self {
            Self::Segment(piece) => piece.point_at(parameter),
            Self::Arc(piece) => piece.point_at(parameter),
        }
    }
    pub fn bounding_box(&self) -> Box2 {
        match self {
            Self::Segment(piece) => piece.bounding_box(),
            Self::Arc(piece) => piece.bounding_box(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurvePolylineNearest {
    pub point: Point2,
    pub station: f64,
    pub segment: usize,
    pub distance: f64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeightedPoint {
    pub position: Point2,
    pub height: Option<f64>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CurvePolyline2 {
    pub vertices: Vec<CurveVertex>,
    pub closed: bool,
}
impl CurvePolyline2 {
    pub fn from_polyline(polyline: &Polyline2, heights: &[Option<f64>]) -> Self {
        let with_heights = heights.len() == polyline.vertices.len();
        Self {
            closed: polyline.closed,
            vertices: polyline
                .vertices
                .iter()
                .enumerate()
                .map(|(index, &position)| CurveVertex {
                    position,
                    bulge: 0.0,
                    height: if with_heights { heights[index] } else { None },
                })
                .collect(),
        }
    }
    pub fn from_points(points: &[Point2], is_closed: bool) -> Self {
        Self {
            closed: is_closed,
            vertices: points
                .iter()
                .map(|&position| CurveVertex {
                    position,
                    bulge: 0.0,
                    height: None,
                })
                .collect(),
        }
    }
    pub fn segment_count(&self) -> usize {
        if self.vertices.len() < 2 {
            0
        } else if self.closed {
            self.vertices.len()
        } else {
            self.vertices.len() - 1
        }
    }
    pub fn segment_end(&self, index: usize) -> usize {
        if index + 1 < self.vertices.len() {
            index + 1
        } else {
            0
        }
    }
    pub fn is_arc(&self, index: usize) -> bool {
        arc_from_bulge(
            self.vertices[index].position,
            self.vertices[self.segment_end(index)].position,
            self.vertices[index].bulge,
        )
        .is_some()
    }
    pub fn segment(&self, index: usize) -> CurveSegment {
        let first = self.vertices[index].position;
        let last = self.vertices[self.segment_end(index)].position;
        if let Some(arc) = arc_from_bulge(first, last, self.vertices[index].bulge) {
            CurveSegment::Arc(arc)
        } else {
            CurveSegment::Segment(Segment2::new(first, last))
        }
    }
    pub fn segment_length(&self, index: usize) -> f64 {
        self.segment(index).length()
    }
    pub fn has_arcs(&self) -> bool {
        (0..self.segment_count()).any(|index| self.is_arc(index))
    }
    pub fn has_heights(&self) -> bool {
        self.vertices.iter().any(|vertex| vertex.height.is_some())
    }
    pub fn positions(&self) -> Vec<Point2> {
        self.vertices.iter().map(|vertex| vertex.position).collect()
    }
    pub fn heights(&self) -> Vec<Option<f64>> {
        self.vertices.iter().map(|vertex| vertex.height).collect()
    }
    pub fn length(&self) -> f64 {
        let mut total = 0.0;
        for index in 0..self.segment_count() {
            total += self.segment_length(index);
        }
        total
    }
    pub fn signed_area(&self) -> f64 {
        if !self.closed || self.vertices.len() < 2 {
            return 0.0;
        }
        let origin = self.vertices[0].position;
        let mut twice = 0.0;
        for index in 0..self.vertices.len() {
            twice += (self.vertices[index].position - origin)
                .cross(self.vertices[self.segment_end(index)].position - origin);
        }
        let mut area = 0.5 * twice;
        for index in 0..self.segment_count() {
            if let CurveSegment::Arc(arc) = self.segment(index) {
                area += 0.5 * arc.radius * arc.radius * (arc.sweep - arc.sweep.sin());
            }
        }
        area
    }
    pub fn area(&self) -> f64 {
        self.signed_area().abs()
    }
    pub fn nearest(&self, point: Point2) -> Option<CurvePolylineNearest> {
        let first = self.vertices.first()?.position;
        let mut best = CurvePolylineNearest {
            point: first,
            station: 0.0,
            segment: 0,
            distance: first.distance_to(point),
        };
        let mut station = 0.0;
        for index in 0..self.segment_count() {
            let piece = self.segment(index);
            let (candidate, along) = match piece {
                CurveSegment::Segment(line) => {
                    let parameter = line.parameter_of(point);
                    (line.point_at(parameter), parameter * line.length())
                }
                CurveSegment::Arc(arc) => {
                    let candidate = arc.closest_point(point);
                    (
                        candidate,
                        arc.parameter_of_angle((candidate - arc.center).angle()) * arc.length(),
                    )
                }
            };
            let distance = candidate.distance_to(point);
            if distance < best.distance {
                best = CurvePolylineNearest {
                    point: candidate,
                    station: station + along,
                    segment: index,
                    distance,
                };
            }
            station += piece.length();
        }
        Some(best)
    }
    pub fn closest_point(&self, point: Point2) -> Option<Point2> {
        self.nearest(point).map(|found| found.point)
    }
    pub fn distance_to(&self, point: Point2) -> Option<f64> {
        self.nearest(point).map(|found| found.distance)
    }
    pub fn station_of_vertex(&self, index: usize) -> f64 {
        let mut station = 0.0;
        for current in 0..index.min(self.segment_count()) {
            station += self.segment_length(current);
        }
        station
    }
    pub fn point_at_station(&self, station: f64) -> Point2 {
        let Some(first) = self.vertices.first() else {
            return Point2::default();
        };
        let mut walked = 0.0;
        for index in 0..self.segment_count() {
            let piece = self.segment(index);
            let length = piece.length();
            if station <= walked + length || index + 1 == self.segment_count() {
                let parameter = if length > 0.0 {
                    ((station - walked) / length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                return piece.point_at(parameter);
            }
            walked += length;
        }
        first.position
    }
    pub fn height_at_station(&self, station: f64) -> Option<f64> {
        let first = self.vertices.first()?;
        if self.segment_count() == 0 {
            return first.height;
        }
        let mut walked = 0.0;
        for index in 0..self.segment_count() {
            let length = self.segment_length(index);
            if station <= walked + length || index + 1 == self.segment_count() {
                let first_height = self.vertices[index].height?;
                let last_height = self.vertices[self.segment_end(index)].height?;
                let parameter = if length > 0.0 {
                    ((station - walked) / length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                return Some(first_height + (last_height - first_height) * parameter);
            }
            walked += length;
        }
        None
    }
    pub fn bounding_box(&self) -> Box2 {
        let mut bounds = Box2::default();
        for vertex in &self.vertices {
            bounds.expand(vertex.position);
        }
        for index in 0..self.segment_count() {
            if self.is_arc(index) {
                bounds.expand_box(self.segment(index).bounding_box());
            }
        }
        bounds
    }
    pub fn reversed(&self) -> Self {
        let mut output = Self {
            vertices: Vec::with_capacity(self.vertices.len()),
            closed: self.closed,
        };
        let count = self.vertices.len();
        for index in 0..count {
            let mut vertex = self.vertices[count - 1 - index];
            vertex.bulge = if index + 1 < count {
                -self.vertices[count - 2 - index].bulge
            } else if self.closed {
                -self.vertices[count - 1].bulge
            } else {
                0.0
            };
            output.vertices.push(vertex);
        }
        output
    }
    pub fn tessellate(&self, epsilon: f64) -> Vec<Point2> {
        let mut output = Vec::new();
        let Some(first) = self.vertices.first() else {
            return output;
        };
        output.push(first.position);
        for index in 0..self.segment_count() {
            if let CurveSegment::Arc(arc) = self.segment(index) {
                let chords = crate::chording::chord_arc(&arc, epsilon);
                output.extend(chords.iter().skip(1).copied());
                if chords.len() > 1 {
                    *output.last_mut().unwrap() = self.vertices[self.segment_end(index)].position;
                }
            } else {
                output.push(self.vertices[self.segment_end(index)].position);
            }
        }
        output
    }
    pub fn tessellate_with_heights(&self, epsilon: f64) -> Vec<HeightedPoint> {
        let mut output = Vec::new();
        let Some(first) = self.vertices.first() else {
            return output;
        };
        output.push(HeightedPoint {
            position: first.position,
            height: first.height,
        });
        for index in 0..self.segment_count() {
            let from = self.vertices[index];
            let to = self.vertices[self.segment_end(index)];
            if let Some(arc) = arc_from_bulge(from.position, to.position, from.bulge) {
                let chords = crate::chording::chord_arc(&arc, epsilon);
                for current in 1..chords.len().saturating_sub(1) {
                    let parameter = current as f64 / (chords.len() - 1) as f64;
                    let height = from
                        .height
                        .zip(to.height)
                        .map(|(first_height, last_height)| {
                            first_height + (last_height - first_height) * parameter
                        });
                    output.push(HeightedPoint {
                        position: chords[current],
                        height,
                    });
                }
            }
            output.push(HeightedPoint {
                position: to.position,
                height: to.height,
            });
        }
        output
    }
    pub fn to_polyline(&self, epsilon: f64) -> Polyline2 {
        if !self.has_arcs() {
            return Polyline2::new(self.positions(), self.closed);
        }
        let mut points = self.tessellate(epsilon);
        if self.closed && points.len() > 1 {
            points.pop();
        }
        Polyline2::new(points, self.closed)
    }
    pub fn segments(&self) -> Vec<CurveSegment> {
        (0..self.segment_count())
            .map(|index| self.segment(index))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ellipse2 {
    pub center: Point2,
    pub major_axis: Vec2,
    pub ratio: f64,
    pub start_parameter: f64,
    pub sweep: f64,
}
impl Default for Ellipse2 {
    fn default() -> Self {
        Self {
            center: Point2::default(),
            major_axis: Vec2::new(1.0, 0.0),
            ratio: 1.0,
            start_parameter: 0.0,
            sweep: TWO_PI,
        }
    }
}
impl Ellipse2 {
    pub fn is_full(&self) -> bool {
        self.sweep >= TWO_PI - tolerance::ANGULAR
    }
    pub fn major_radius(&self) -> f64 {
        self.major_axis.length()
    }
    pub fn minor_radius(&self) -> f64 {
        self.major_axis.length() * self.ratio
    }
    pub fn minor_axis(&self) -> Vec2 {
        self.major_axis.perpendicular() * self.ratio
    }
    pub fn end_parameter(&self) -> f64 {
        self.start_parameter + self.sweep
    }
    pub fn point_at_parameter(&self, parameter: f64) -> Point2 {
        self.center + self.major_axis * parameter.cos() + self.minor_axis() * parameter.sin()
    }
    pub fn derivative_at_parameter(&self, parameter: f64) -> Vec2 {
        self.major_axis * -parameter.sin() + self.minor_axis() * parameter.cos()
    }
    pub fn start_point(&self) -> Point2 {
        self.point_at_parameter(self.start_parameter)
    }
    pub fn end_point(&self) -> Point2 {
        self.point_at_parameter(self.end_parameter())
    }
    pub fn parameter_towards(&self, point: Point2) -> f64 {
        let major = self.major_radius();
        let minor = self.minor_radius();
        if !(major > 0.0) || !(minor > 0.0) {
            return 0.0;
        }
        let delta = point - self.center;
        let unit_major = self.major_axis / major;
        (delta.dot(unit_major.perpendicular()) / minor).atan2(delta.dot(unit_major) / major)
    }
    pub fn contains_parameter(&self, parameter: f64) -> bool {
        self.contains_parameter_with_tolerance(parameter, 0.0)
    }
    pub fn contains_parameter_with_tolerance(&self, parameter: f64, epsilon: f64) -> bool {
        if self.is_full() {
            return true;
        }
        let travelled = normalize_angle(parameter - self.start_parameter);
        travelled <= self.sweep + epsilon || travelled >= TWO_PI - epsilon
    }
    pub fn quadrants(&self) -> Vec<Point2> {
        let mut output = Vec::new();
        for index in 0..4 {
            let parameter = HALF_PI * index as f64;
            if self.contains_parameter_with_tolerance(parameter, tolerance::ANGULAR) {
                output.push(self.point_at_parameter(parameter));
            }
        }
        output
    }
    pub fn length(&self) -> f64 {
        #[allow(
            clippy::excessive_precision,
            reason = "Preserve the quadrature node literals exactly as written in the C++ oracle."
        )]
        const NODES: [f64; 5] = [
            0.0,
            -0.5384693101056831,
            0.5384693101056831,
            -0.9061798459386640,
            0.9061798459386640,
        ];
        const WEIGHTS: [f64; 5] = [
            0.5688888888888889,
            0.4786286704993665,
            0.4786286704993665,
            0.2369268850561891,
            0.2369268850561891,
        ];
        let step = self.sweep / 64.0;
        let mut total = 0.0;
        for panel in 0..64 {
            let middle = self.start_parameter + step * (panel as f64 + 0.5);
            for index in 0..NODES.len() {
                total += WEIGHTS[index]
                    * self
                        .derivative_at_parameter(middle + 0.5 * step * NODES[index])
                        .length();
            }
        }
        total * 0.5 * step
    }
    pub fn bounding_box(&self) -> Box2 {
        let mut bounds = Box2::default();
        bounds.expand(self.start_point());
        bounds.expand(self.end_point());
        let minor = self.minor_axis();
        let east = minor.x.atan2(self.major_axis.x);
        let north = minor.y.atan2(self.major_axis.y);
        for parameter in [east, east + PI, north, north + PI] {
            if self.contains_parameter(parameter) {
                bounds.expand(self.point_at_parameter(parameter));
            }
        }
        bounds
    }
    pub fn closest_parameter(&self, point: Point2) -> f64 {
        let mut best_parameter = self.start_parameter;
        let mut best_distance = f64::INFINITY;
        for index in 0..=72 {
            let parameter = self.start_parameter + self.sweep * index as f64 / 72.0;
            let distance = self.point_at_parameter(parameter).distance_to(point);
            if distance < best_distance {
                best_distance = distance;
                best_parameter = parameter;
            }
        }
        let low = self.start_parameter;
        let high = self.start_parameter + self.sweep;
        let mut parameter = best_parameter;
        for _ in 0..30 {
            let offset = self.point_at_parameter(parameter) - point;
            let first = self.derivative_at_parameter(parameter);
            let second = (self.point_at_parameter(parameter) - self.center) * -1.0;
            let value = offset.dot(first);
            let derivative = first.dot(first) + offset.dot(second);
            if !(derivative.abs() > 0.0) {
                break;
            }
            let mut next = parameter - value / derivative;
            if !self.is_full() {
                next = next.clamp(low, high);
            }
            if self.point_at_parameter(next).distance_to(point)
                > self.point_at_parameter(parameter).distance_to(point) + 1e-15
            {
                break;
            }
            let converged = (next - parameter).abs() < 1e-14;
            parameter = next;
            if converged {
                break;
            }
        }
        parameter
    }
    pub fn closest_point(&self, point: Point2) -> Point2 {
        self.point_at_parameter(self.closest_parameter(point))
    }
    pub fn distance_to(&self, point: Point2) -> f64 {
        self.closest_point(point).distance_to(point)
    }
    pub fn tessellate(&self, epsilon: f64) -> Vec<Point2> {
        let mut output = Vec::new();
        let pieces = 4.max((self.sweep / (HALF_PI / 4.0)).ceil() as i32);
        tessellate_range(
            &|parameter| self.point_at_parameter(parameter),
            self.start_parameter,
            self.start_parameter + self.sweep,
            pieces,
            epsilon,
            &mut output,
        );
        output
    }
    pub fn from_axes(center: Point2, axis_end: Point2, other_radius: f64) -> Option<Self> {
        let axis = axis_end - center;
        let radius = axis.length();
        if !(radius > tolerance::GEOMETRIC)
            || !(other_radius > tolerance::GEOMETRIC)
            || !other_radius.is_finite()
        {
            return None;
        }
        let (major_axis, ratio) = if other_radius <= radius {
            (axis, other_radius / radius)
        } else {
            (
                axis.perpendicular() * (other_radius / radius),
                radius / other_radius,
            )
        };
        Some(Self {
            center,
            major_axis,
            ratio,
            ..Self::default()
        })
    }
}

fn find_span(knots: &[f64], degree: usize, count: usize, parameter: f64) -> usize {
    if parameter >= knots[count] {
        return count - 1;
    }
    if parameter <= knots[degree] {
        let mut span = degree;
        while span + 1 < count && knots[span + 1] <= parameter {
            span += 1;
        }
        return span;
    }
    let mut low = degree;
    let mut high = count;
    let mut middle = (low + high) / 2;
    while parameter < knots[middle] || parameter >= knots[middle + 1] {
        if parameter < knots[middle] {
            high = middle;
        } else {
            low = middle;
        }
        middle = (low + high) / 2;
    }
    middle
}
fn basis_functions(knots: &[f64], degree: usize, span: usize, parameter: f64) -> Vec<f64> {
    let mut basis = vec![0.0; degree + 1];
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    basis[0] = 1.0;
    for order in 1..=degree {
        left[order] = parameter - knots[span + 1 - order];
        right[order] = knots[span + order] - parameter;
        let mut saved = 0.0;
        for row in 0..order {
            let denominator = right[row + 1] + left[order - row];
            let temporary = if denominator != 0.0 {
                basis[row] / denominator
            } else {
                0.0
            };
            basis[row] = saved + right[row + 1] * temporary;
            saved = left[order - row] * temporary;
        }
        basis[order] = saved;
    }
    basis
}
fn without_repeats(points: Vec<Point2>) -> Vec<Point2> {
    let mut output: Vec<Point2> = Vec::with_capacity(points.len());
    for point in points {
        if output
            .last()
            .is_none_or(|last| last.distance_to(point) > tolerance::GEOMETRIC)
        {
            output.push(point);
        }
    }
    output
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spline2 {
    pub degree: i32,
    pub control_points: Vec<Point2>,
    pub knots: Vec<f64>,
    pub weights: Vec<f64>,
    pub fit_points: Vec<Point2>,
}
impl Default for Spline2 {
    fn default() -> Self {
        Self {
            degree: 3,
            control_points: Vec::new(),
            knots: Vec::new(),
            weights: Vec::new(),
            fit_points: Vec::new(),
        }
    }
}
impl Spline2 {
    pub fn from_control_points(points: Vec<Point2>, order: i32) -> Result<Self> {
        let points = without_repeats(points);
        if points.len() < 2 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a spline needs at least two different control points",
            ));
        }
        if order < 1 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a spline's degree must be at least 1",
            ));
        }
        let degree = order.min(points.len() as i32 - 1);
        let count = degree as usize;
        let mut knots = vec![0.0; count + 1];
        let inner = points.len() - count - 1;
        for index in 1..=inner {
            knots.push(index as f64 / (inner + 1) as f64);
        }
        knots.extend(std::iter::repeat_n(1.0, count + 1));
        Ok(Self {
            degree,
            knots,
            control_points: points,
            ..Self::default()
        })
    }
    pub fn through_points(points: Vec<Point2>, order: i32) -> Result<Self> {
        let points = without_repeats(points);
        let count = points.len();
        if count < 2 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a spline needs at least two different points to pass through",
            ));
        }
        if order < 1 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a spline's degree must be at least 1",
            ));
        }
        let degree = order.min(count as i32 - 1);
        let order_size = degree as usize;
        let mut parameters = vec![0.0; count];
        let mut total = 0.0;
        for index in 1..count {
            total += points[index].distance_to(points[index - 1]);
        }
        let mut walked = 0.0;
        for index in 1..count - 1 {
            walked += points[index].distance_to(points[index - 1]);
            parameters[index] = walked / total;
        }
        parameters[count - 1] = 1.0;
        let mut knots = vec![0.0; count + order_size + 1];
        knots[count..].fill(1.0);
        for start in 1..count - order_size {
            let mut sum = 0.0;
            for &parameter in &parameters[start..start + order_size] {
                sum += parameter;
            }
            knots[start + order_size] = sum / degree as f64;
        }
        let width = 2 * order_size + 1;
        let mut band = vec![vec![0.0; width]; count];
        for row in 0..count {
            let span = find_span(&knots, order_size, count, parameters[row]);
            let basis = basis_functions(&knots, order_size, span, parameters[row]);
            for (index, &value) in basis.iter().enumerate() {
                let column = span - order_size + index;
                if column + order_size >= row && column <= row + order_size {
                    band[row][column + order_size - row] = value;
                }
            }
        }
        let mut rhs = points.clone();
        for pivot_row in 0..count {
            let pivot = band[pivot_row][order_size];
            if !(pivot.abs() > 1e-300) {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "the points cannot be interpolated by a spline",
                ));
            }
            for row in pivot_row + 1..count.min(pivot_row + order_size + 1) {
                let factor = band[row][pivot_row + order_size - row] / pivot;
                if factor == 0.0 {
                    continue;
                }
                for column in pivot_row..count.min(pivot_row + order_size + 1) {
                    let pivot_entry = band[pivot_row][column + order_size - pivot_row];
                    band[row][column + order_size - row] -= factor * pivot_entry;
                }
                rhs[row] = rhs[row] - rhs[pivot_row] * factor;
            }
        }
        let mut control = vec![Point2::default(); count];
        for row in (0..count).rev() {
            let mut sum = rhs[row];
            for column in row + 1..count.min(row + order_size + 1) {
                sum -= control[column] * band[row][column + order_size - row];
            }
            control[row] = sum / band[row][order_size];
        }
        Ok(Self {
            degree,
            control_points: control,
            knots,
            fit_points: points,
            weights: Vec::new(),
        })
    }
    pub fn is_rational(&self) -> bool {
        !self.weights.is_empty()
    }
    pub fn has_fit_points(&self) -> bool {
        !self.fit_points.is_empty()
    }
    pub fn domain_start(&self) -> f64 {
        if self.knots.is_empty() {
            0.0
        } else {
            self.knots[self.degree.max(0) as usize]
        }
    }
    pub fn domain_end(&self) -> f64 {
        if self.knots.len() > self.control_points.len() {
            self.knots[self.control_points.len()]
        } else {
            0.0
        }
    }
    pub fn point_at(&self, parameter: f64) -> Point2 {
        let count = self.control_points.len();
        if count == 0 {
            return Point2::default();
        }
        if self.check_structure().is_err() {
            return self.control_points[0];
        }
        let parameter = parameter.clamp(self.domain_start(), self.domain_end());
        let degree = self.degree as usize;
        let span = find_span(&self.knots, degree, count, parameter);
        let basis = basis_functions(&self.knots, degree, span, parameter);
        let mut east = 0.0;
        let mut north = 0.0;
        let mut total_weight = 0.0;
        for (current, &basis_value) in basis.iter().enumerate() {
            let index = span - degree + current;
            let weight = if self.weights.is_empty() {
                1.0
            } else {
                self.weights[index]
            };
            east += basis_value * weight * self.control_points[index].x;
            north += basis_value * weight * self.control_points[index].y;
            total_weight += basis_value * weight;
        }
        if total_weight != 0.0 {
            Point2::new(east / total_weight, north / total_weight)
        } else {
            self.control_points[span]
        }
    }
    pub fn start_point(&self) -> Point2 {
        self.point_at(self.domain_start())
    }
    pub fn end_point(&self) -> Point2 {
        self.point_at(self.domain_end())
    }
    pub fn is_closed_shape(&self) -> bool {
        !self.control_points.is_empty()
            && self.start_point().distance_to(self.end_point()) <= tolerance::GEOMETRIC
    }
    pub fn tessellate(&self, epsilon: f64) -> Vec<Point2> {
        let mut output = Vec::new();
        if self.check_structure().is_err() {
            return output;
        }
        for index in self.degree as usize..self.control_points.len() {
            if self.knots[index + 1] > self.knots[index] {
                tessellate_range(
                    &|parameter| self.point_at(parameter),
                    self.knots[index],
                    self.knots[index + 1],
                    self.degree + 1,
                    epsilon,
                    &mut output,
                );
            }
        }
        output
    }
    pub fn to_polyline(&self, epsilon: f64) -> Polyline2 {
        let mut points = self.tessellate(epsilon);
        let closed = points.len() > 3
            && points[0].distance_to(*points.last().unwrap()) <= tolerance::GEOMETRIC;
        if closed {
            points.pop();
        }
        Polyline2::new(points, closed)
    }
    pub fn length(&self) -> f64 {
        chain_length(&self.tessellate(CURVE_CHORD_TOLERANCE * 1.0e-3))
    }
    pub fn bounding_box(&self) -> Box2 {
        let epsilon = CURVE_CHORD_TOLERANCE * 0.1;
        let mut bounds = Box2::default();
        for point in self.tessellate(epsilon) {
            bounds.expand(point);
        }
        if bounds.empty() {
            for &point in &self.control_points {
                bounds.expand(point);
            }
            return bounds;
        }
        bounds.inflated(epsilon)
    }
    pub fn closest_point(&self, point: Point2) -> Point2 {
        let chain = self.tessellate(CURVE_CHORD_TOLERANCE * 0.1);
        if chain.is_empty() {
            return self.control_points.first().copied().unwrap_or_default();
        }
        let mut best = chain[0];
        let mut best_distance = f64::INFINITY;
        for pair in chain.windows(2) {
            let candidate = Segment2::new(pair[0], pair[1]).closest_point(point);
            let distance = candidate.distance_to(point);
            if distance < best_distance {
                best_distance = distance;
                best = candidate;
            }
        }
        best
    }
    pub fn distance_to(&self, point: Point2) -> f64 {
        self.closest_point(point).distance_to(point)
    }
    pub fn reversed(&self) -> Self {
        let mut output = self.clone();
        output.control_points.reverse();
        output.weights.reverse();
        output.fit_points.reverse();
        if !self.knots.is_empty() {
            let first = self.knots[0];
            let last = *self.knots.last().unwrap();
            for index in 0..self.knots.len() {
                output.knots[index] = first + last - self.knots[self.knots.len() - 1 - index];
            }
        }
        output
    }
    pub fn check_structure(&self) -> Status {
        if self.degree < 1 || self.degree > 10 {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "spline degree must be from 1 to 10",
            )
            .with_context(format!("{}", self.degree)));
        }
        let degree = self.degree as usize;
        if self.control_points.len() < degree + 1 {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "spline has too few control points for its degree",
            ));
        }
        if self.knots.len() != self.control_points.len() + degree + 1 {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "spline knot count must be control points + degree + 1",
            ));
        }
        if !self.weights.is_empty() && self.weights.len() != self.control_points.len() {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "spline must have no weights or one per control point",
            ));
        }
        for index in 0..self.knots.len() {
            if !self.knots[index].is_finite()
                || (index > 0 && self.knots[index] < self.knots[index - 1])
            {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "spline knots must be finite and non-decreasing",
                ));
            }
        }
        for &weight in &self.weights {
            if !(weight.is_finite() && weight > 0.0) {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "spline weights must be positive",
                ));
            }
        }
        if !(self.domain_end() > self.domain_start()) {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "spline has an empty parameter range",
            ));
        }
        Ok(())
    }
}
