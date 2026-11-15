use crate::chording::chord_arc;
use crate::{Arc2, Point2, Polyline2, Segment2, Spiral2, Vec2};
use katana_core::error::{Error, ErrorCode, Result};
use katana_core::text::format_exact_real;
use katana_math::cpp::max;
use katana_math::numerics::{PI, RAD_TO_DEG, normalize_angle_signed, tolerance};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AlignmentPi {
    pub point: Point2,
    pub radius: f64,
    pub spiral_in: f64,
    pub spiral_out: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HorizontalAlignment {
    pub pis: Vec<AlignmentPi>,
    pub start_station: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlignmentShape {
    Segment(Segment2),
    Arc(Arc2),
    Spiral(Spiral2),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlignmentElement {
    pub shape: AlignmentShape,
    pub start_station: f64,
    pub length: f64,
    pub pi: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignmentElementKind {
    Tangent,
    Spiral,
    Arc,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StationOffset {
    pub station: f64,
    pub offset: f64,
}
#[derive(Clone, Debug, Default)]
pub struct SolvedAlignment {
    elements: Vec<AlignmentElement>,
    start_station: f64,
    length: f64,
}

pub fn kind_of(element: &AlignmentElement) -> AlignmentElementKind {
    match element.shape {
        AlignmentShape::Segment(_) => AlignmentElementKind::Tangent,
        AlignmentShape::Arc(_) => AlignmentElementKind::Arc,
        AlignmentShape::Spiral(_) => AlignmentElementKind::Spiral,
    }
}
fn direction_of(from: Point2, to: Point2) -> f64 {
    (to.y - from.y).atan2(to.x - from.x)
}
fn unit_at(direction: f64) -> Vec2 {
    Vec2::new(direction.cos(), direction.sin())
}
fn pi_label(index: usize) -> String {
    format!("PI {index}")
}
#[derive(Default)]
struct SpiralOffsets {
    angle: f64,
    shift: f64,
    extension: f64,
}
fn spiral_offsets(radius: f64, length: f64) -> SpiralOffsets {
    if !(length > 0.0) {
        return SpiralOffsets::default();
    }
    let angle = length / (2.0 * radius);
    let end =
        Spiral2::from_straight_to_radius(Point2::new(0.0, 0.0), 0.0, radius, length).end_point();
    SpiralOffsets {
        angle,
        shift: end.y - radius * (1.0 - angle.cos()),
        extension: end.x - radius * angle.sin(),
    }
}
#[derive(Clone, Copy, Default)]
struct CurveAtPi {
    present: bool,
    sign: f64,
    radius: f64,
    spiral_in: f64,
    spiral_out: f64,
    arc_angle: f64,
    tangent_in: f64,
    tangent_out: f64,
    direction_in: f64,
    tangent_start: Point2,
}

pub fn solve_alignment(definition: &HorizontalAlignment) -> Result<SolvedAlignment> {
    let pis = &definition.pis;
    let count = pis.len();
    if count < 2 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "an alignment needs at least two PIs",
        ));
    }
    if !definition.start_station.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the start station must be finite",
        ));
    }
    for (index, pi) in pis.iter().enumerate() {
        if !pi.point.x.is_finite()
            || !pi.point.y.is_finite()
            || !pi.radius.is_finite()
            || !pi.spiral_in.is_finite()
            || !pi.spiral_out.is_finite()
        {
            return Err(
                Error::new(ErrorCode::InvalidArgument, "a PI has a non-finite value")
                    .with_context(pi_label(index)),
            );
        }
        if pi.spiral_in < 0.0 || pi.spiral_out < 0.0 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a spiral length cannot be negative",
            )
            .with_context(pi_label(index)));
        }
        if index > 0 && pis[index - 1].point.distance_to(pi.point) < tolerance::COORDINATE {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "consecutive PIs coincide, so the tangent between them has no direction",
            )
            .with_context(format!("{} and {}", pi_label(index - 1), pi_label(index))));
        }
    }
    let mut curves = vec![
        CurveAtPi {
            sign: 1.0,
            ..Default::default()
        };
        count
    ];
    for index in 1..count - 1 {
        let curve = &mut curves[index];
        curve.direction_in = direction_of(pis[index - 1].point, pis[index].point);
        let direction_out = direction_of(pis[index].point, pis[index + 1].point);
        let deflection = normalize_angle_signed(direction_out - curve.direction_in);
        let radius = pis[index].radius.abs();
        if !(radius > 0.0) || deflection.abs() <= tolerance::ANGULAR {
            continue;
        }
        if PI - deflection.abs() <= tolerance::ANGULAR {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "the alignment reverses on itself, and no curve can round a 180 degree corner",
            )
            .with_context(pi_label(index)));
        }
        let incoming = spiral_offsets(radius, pis[index].spiral_in);
        let outgoing = spiral_offsets(radius, pis[index].spiral_out);
        let mut arc_angle = deflection.abs() - incoming.angle - outgoing.angle;
        if arc_angle < -tolerance::ANGULAR {
            return Err(Error::new(ErrorCode::InvalidGeometry, "the spirals use more deflection than the corner has, leaving no room for the curve").with_context(format!("{}: deflection {} deg, spirals {} deg", pi_label(index), format_exact_real(deflection.abs() * RAD_TO_DEG), format_exact_real((incoming.angle + outgoing.angle) * RAD_TO_DEG))));
        }
        arc_angle = max(arc_angle, 0.0);
        let sine = deflection.abs().sin();
        let cosine = deflection.abs().cos();
        curve.present = true;
        curve.sign = if deflection > 0.0 { 1.0 } else { -1.0 };
        curve.radius = radius;
        curve.spiral_in = pis[index].spiral_in;
        curve.spiral_out = pis[index].spiral_out;
        curve.arc_angle = arc_angle;
        curve.tangent_in = incoming.extension
            + ((radius + outgoing.shift) - (radius + incoming.shift) * cosine) / sine;
        curve.tangent_out = outgoing.extension
            + ((radius + incoming.shift) - (radius + outgoing.shift) * cosine) / sine;
        curve.tangent_start = pis[index].point
            - (pis[index].point - pis[index - 1].point).normalized() * curve.tangent_in;
    }
    for index in 0..count - 1 {
        let available = pis[index].point.distance_to(pis[index + 1].point)
            - if curves[index].present {
                curves[index].tangent_out
            } else {
                0.0
            }
            - if curves[index + 1].present {
                curves[index + 1].tangent_in
            } else {
                0.0
            };
        if available < -tolerance::COORDINATE {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "the curves overlap: their tangent lengths exceed the distance between the PIs",
            )
            .with_context(format!(
                "{} and {}, short by {} m",
                pi_label(index),
                pi_label(index + 1),
                format_exact_real(-available)
            )));
        }
    }
    let mut solved = SolvedAlignment {
        start_station: definition.start_station,
        ..Default::default()
    };
    let mut station = definition.start_station;
    let mut cursor = pis[0].point;
    for index in 1..count {
        let curve = curves[index];
        let target = if curve.present {
            curve.tangent_start
        } else {
            pis[index].point
        };
        let tangent_length = cursor.distance_to(target);
        if tangent_length > tolerance::GEOMETRIC {
            solved.elements.push(AlignmentElement {
                shape: AlignmentShape::Segment(Segment2::new(cursor, target)),
                start_station: station,
                length: tangent_length,
                pi: index,
            });
            station += tangent_length;
            cursor = target;
        }
        if !curve.present {
            continue;
        }
        let curvature = curve.sign / curve.radius;
        let mut direction = curve.direction_in;
        if curve.spiral_in > 0.0 {
            let spiral = Spiral2 {
                start: cursor,
                start_direction: direction,
                start_curvature: 0.0,
                end_curvature: curvature,
                length: curve.spiral_in,
            };
            solved.elements.push(AlignmentElement {
                shape: AlignmentShape::Spiral(spiral),
                start_station: station,
                length: curve.spiral_in,
                pi: index,
            });
            station += curve.spiral_in;
            cursor = spiral.end_point();
            direction = spiral.end_direction();
        }
        if curve.arc_angle > tolerance::ANGULAR {
            let constant = Spiral2 {
                start: cursor,
                start_direction: direction,
                start_curvature: curvature,
                end_curvature: curvature,
                length: curve.radius * curve.arc_angle,
            };
            let arc = constant.as_arc().unwrap();
            solved.elements.push(AlignmentElement {
                shape: AlignmentShape::Arc(arc),
                start_station: station,
                length: constant.length,
                pi: index,
            });
            station += constant.length;
            cursor = arc.end_point();
            direction += curve.sign * curve.arc_angle;
        }
        if curve.spiral_out > 0.0 {
            let spiral = Spiral2 {
                start: cursor,
                start_direction: direction,
                start_curvature: curvature,
                end_curvature: 0.0,
                length: curve.spiral_out,
            };
            solved.elements.push(AlignmentElement {
                shape: AlignmentShape::Spiral(spiral),
                start_station: station,
                length: curve.spiral_out,
                pi: index,
            });
            station += curve.spiral_out;
            cursor = spiral.end_point();
        }
    }
    solved.length = station - definition.start_station;
    Ok(solved)
}

fn point_on(element: &AlignmentElement, local: f64) -> Point2 {
    match element.shape {
        AlignmentShape::Segment(shape) => shape.point_at(local / element.length),
        AlignmentShape::Arc(shape) => shape.point_at(local / element.length),
        AlignmentShape::Spiral(shape) => shape.point_at(local),
    }
}
fn direction_on(element: &AlignmentElement, local: f64) -> f64 {
    match element.shape {
        AlignmentShape::Segment(shape) => direction_of(shape.start, shape.end),
        AlignmentShape::Arc(shape) => {
            let radial = shape.start_angle + shape.sweep * (local / element.length);
            radial + if shape.sweep > 0.0 { 0.5 } else { -0.5 } * PI
        }
        AlignmentShape::Spiral(shape) => shape.direction_at(local),
    }
}
fn curvature_on(element: &AlignmentElement, local: f64) -> f64 {
    match element.shape {
        AlignmentShape::Segment(_) => 0.0,
        AlignmentShape::Arc(shape) => (if shape.sweep > 0.0 { 1.0 } else { -1.0 }) / shape.radius,
        AlignmentShape::Spiral(shape) => shape.curvature_at(local),
    }
}
fn ahead(element: &AlignmentElement, local: f64, point: Point2) -> f64 {
    (point - point_on(element, local)).dot(unit_at(direction_on(element, local)))
}
impl SolvedAlignment {
    pub fn elements(&self) -> &[AlignmentElement] {
        &self.elements
    }
    pub fn start_station(&self) -> f64 {
        self.start_station
    }
    pub fn end_station(&self) -> f64 {
        self.start_station + self.length
    }
    pub fn length(&self) -> f64 {
        self.length
    }
    pub fn contains_station(&self, station: f64) -> bool {
        station >= self.start_station && station <= self.end_station()
    }
    pub fn element_at(&self, station: f64) -> Option<&AlignmentElement> {
        if self.elements.is_empty() || !self.contains_station(station) {
            return None;
        }
        let after = self
            .elements
            .partition_point(|element| !(station < element.start_station));
        Some(&self.elements[after.saturating_sub(1)])
    }
    pub fn point_at_station(&self, station: f64) -> Option<Point2> {
        self.element_at(station)
            .map(|element| point_on(element, station - element.start_station))
    }
    pub fn direction_at_station(&self, station: f64) -> Option<f64> {
        self.element_at(station)
            .map(|element| direction_on(element, station - element.start_station))
    }
    pub fn curvature_at_station(&self, station: f64) -> Option<f64> {
        self.element_at(station)
            .map(|element| curvature_on(element, station - element.start_station))
    }
    pub fn point_at_station_offset(&self, station: f64, offset: f64) -> Option<Point2> {
        let point = self.point_at_station(station)?;
        let direction = self.direction_at_station(station)?;
        Some(Point2::new(
            point.x - direction.sin() * offset,
            point.y + direction.cos() * offset,
        ))
    }
    pub fn station_offset(&self, point: Point2) -> Option<StationOffset> {
        if self.elements.is_empty() || !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        let mut best = None;
        let mut best_distance = f64::INFINITY;
        let mut consider = |station, foot, direction| {
            let away: Vec2 = point - foot;
            let distance = away.length();
            if distance < best_distance {
                best_distance = distance;
                let right = unit_at(direction).cross(away) < 0.0;
                best = Some(StationOffset {
                    station,
                    offset: if right { -distance } else { distance },
                });
            }
        };
        for (element_index, element) in self.elements.iter().enumerate() {
            let length = element.length;
            let turn = max(
                curvature_on(element, 0.0).abs(),
                curvature_on(element, length).abs(),
            ) * length;
            let stretches = ((turn / (PI / 8.0)).ceil() as usize).clamp(1, 64);
            let mut from = 0.0;
            let mut from_ahead = ahead(element, 0.0, point);
            for index in 1..=stretches {
                let to = if index == stretches {
                    length
                } else {
                    length * index as f64 / stretches as f64
                };
                let to_ahead = ahead(element, to, point);
                if from_ahead >= 0.0 && to_ahead <= 0.0 {
                    let mut low = from;
                    let mut high = to;
                    if from_ahead == 0.0 {
                        high = from;
                    } else if to_ahead == 0.0 {
                        low = to;
                    }
                    for _ in 0..200 {
                        let middle = 0.5 * (low + high);
                        if !(middle > low && middle < high) {
                            break;
                        }
                        if ahead(element, middle, point) >= 0.0 {
                            low = middle;
                        } else {
                            high = middle;
                        }
                    }
                    let local = 0.5 * (low + high);
                    consider(
                        element.start_station + local,
                        point_on(element, local),
                        direction_on(element, local),
                    );
                }
                from = to;
                from_ahead = to_ahead;
            }
            if element_index + 1 < self.elements.len() {
                let next = &self.elements[element_index + 1];
                if ahead(element, length, point) > 0.0 && ahead(next, 0.0, point) < 0.0 {
                    let bisector =
                        unit_at(direction_on(element, length)) + unit_at(direction_on(next, 0.0));
                    consider(
                        element.start_station + length,
                        point_on(element, length),
                        if bisector.length() > 0.0 {
                            bisector.y.atan2(bisector.x)
                        } else {
                            direction_on(element, length)
                        },
                    );
                }
            }
        }
        best
    }
    pub fn to_polyline(&self, epsilon: f64) -> Polyline2 {
        let mut polyline = Polyline2::default();
        for element in &self.elements {
            let points = match element.shape {
                AlignmentShape::Segment(shape) => vec![shape.start, shape.end],
                AlignmentShape::Arc(shape) => chord_arc(&shape, epsilon),
                AlignmentShape::Spiral(shape) => {
                    let count = shape.chord_count_for(epsilon);
                    (0..count)
                        .map(|index| {
                            let fraction = index as f64 / (count - 1) as f64;
                            shape.point_at(fraction * shape.length)
                        })
                        .collect()
                }
            };
            let first = if polyline.vertices.is_empty() { 0 } else { 1 };
            polyline.vertices.extend(points.into_iter().skip(first));
        }
        polyline
    }
    pub fn key_stations(&self) -> Vec<f64> {
        let mut stations = vec![self.start_station];
        stations.extend(
            self.elements
                .iter()
                .map(|element| element.start_station + element.length),
        );
        stations
    }
}
