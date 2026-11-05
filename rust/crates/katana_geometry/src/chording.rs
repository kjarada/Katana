use crate::scalar::max;
use crate::{Arc2, Circle2, Point2};
use katana_math::numerics::{TWO_PI, tolerance};

pub fn sagitta_chord_count(radius: f64, sweep_radians: f64, epsilon: f64) -> usize {
    if !(radius > 0.0) || !sweep_radians.is_finite() {
        return 1;
    }
    let ratio = (epsilon / radius).clamp(1.0e-12, 0.5);
    let step = 2.0 * (1.0 - ratio).acos();
    (sweep_radians.abs() / max(step, 1.0e-9))
        .ceil()
        .clamp(1.0, 8192.0) as usize
}

pub fn chord_arc(arc: &Arc2, epsilon: f64) -> Vec<Point2> {
    let mut output = Vec::new();
    if !(arc.radius > 0.0) || !arc.sweep.is_finite() || arc.sweep.abs() <= tolerance::ANGULAR {
        if arc.radius > 0.0 {
            output.push(arc.start_point());
            output.push(arc.end_point());
        }
        return output;
    }
    let count = sagitta_chord_count(arc.radius, arc.sweep, epsilon);
    output.reserve(count + 1);
    for index in 0..=count {
        output.push(arc.point_at(index as f64 / count as f64));
    }
    output
}

pub fn chord_circle(circle: &Circle2, epsilon: f64) -> Vec<Point2> {
    let full = Arc2::new(circle.center, circle.radius, 0.0, TWO_PI);
    let mut output = chord_arc(&full, epsilon);
    output.pop();
    output
}
