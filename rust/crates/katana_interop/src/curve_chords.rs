//! Shared by the katana_interop modules, never public
//! (src/katana_interop/curve_chords.hpp): how many chords a curve is written
//! as, so that EXPORT and the geoprocessing bindings (geo/drawing_dataset.rs)
//! turn an arc into the same polyline.

/// Number of chords needed so a circular arc of `radius` spanning `sweep`
/// never deviates from the polyline by more than `tolerance`.
///
/// The sagitta of a chord subtending an angle phi is r * (1 - cos(phi/2)), so
/// the largest admissible phi is 2 * acos(1 - tolerance / r). When the
/// tolerance is at or beyond the radius the whole arc is within tolerance of
/// a single chord and the formula degenerates, so that case is handled
/// separately rather than left to produce a NaN.
pub(crate) fn chord_count(radius: f64, sweep: f64, tolerance: f64) -> i32 {
    let abs_sweep = sweep.abs();
    if !(radius > 0.0) || !(abs_sweep > 0.0) {
        return 1;
    }
    if !(tolerance > 0.0) {
        return 64; // a caller that asked for zero error gets a fine default
    }
    if tolerance >= radius {
        return 1;
    }
    let max_angle = 2.0 * (1.0 - tolerance / radius).acos();
    if !(max_angle > 0.0) || !max_angle.is_finite() {
        return 4096;
    }
    // The C++'s static_cast<int> of a quotient past int's range (an infinite
    // sweep) is undefined; Rust's cast saturates, and the clamp takes either
    // to 4096.
    let count = (abs_sweep / max_angle).ceil() as i32;
    count.clamp(1, 4096)
}
