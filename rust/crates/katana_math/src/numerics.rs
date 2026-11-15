use crate::cpp;

pub const PI: f64 = std::f64::consts::PI;
pub const TWO_PI: f64 = 2.0 * PI;
pub const HALF_PI: f64 = 0.5 * PI;
pub const DEG_TO_RAD: f64 = PI / 180.0;
pub const RAD_TO_DEG: f64 = 180.0 / PI;

pub mod tolerance {
    pub const ABSOLUTE: f64 = 1e-12;
    pub const RELATIVE: f64 = 1e-9;
    pub const ANGULAR: f64 = 1e-10;
    pub const GEOMETRIC: f64 = 1e-7;
    pub const COORDINATE: f64 = 1e-4;
}

pub fn nearly_equal(first: f64, second: f64, epsilon: f64) -> bool {
    if first == second {
        return true;
    }
    if !first.is_finite() || !second.is_finite() {
        return false;
    }
    let difference = (first - second).abs();
    let scale = cpp::max(cpp::max(1.0, first.abs()), second.abs());
    difference <= epsilon * scale
}

pub fn nearly_zero(value: f64, epsilon: f64) -> bool {
    value.abs() <= epsilon
}

pub fn lengths_equal(first: f64, second: f64, epsilon: f64) -> bool {
    (first - second).abs() <= epsilon
}

pub fn clamp(value: f64, min_value: f64, max_value: f64) -> f64 {
    cpp::min(cpp::max(value, min_value), max_value)
}

pub fn lerp(first: f64, second: f64, parameter: f64) -> f64 {
    first + (second - first) * parameter
}

pub fn normalize_angle(radians: f64) -> f64 {
    let mut wrapped = radians % TWO_PI;
    if wrapped < 0.0 {
        wrapped += TWO_PI;
    }
    if wrapped >= TWO_PI { 0.0 } else { wrapped }
}

pub fn normalize_angle_signed(radians: f64) -> f64 {
    let wrapped = normalize_angle(radians);
    if wrapped > PI {
        wrapped - TWO_PI
    } else {
        wrapped
    }
}

pub fn angles_equal(first: f64, second: f64, epsilon: f64) -> bool {
    normalize_angle_signed(first - second).abs() <= epsilon
}

pub fn is_finite(value: f64) -> bool {
    value.is_finite()
}
