//! Coordinate geometry: inverse / forward, polygon area, perimeter and
//! centroid, slope reductions.
//!
//! Numerical assumptions
//!   * Plane (grid) computations; no scale factor, convergence or curvature.
//!   * azimuth = atan2(dE, dN), clockwise from grid north, in [0, 2*pi).
//!   * Two positions closer than tolerance::COORDINATE (0.1 mm) are the same
//!     ground mark; the azimuth between them is undefined.
//!   * Polygon integrals are evaluated relative to the first vertex. With UTM
//!     sized coordinates (E ~ 5e5, N ~ 5e6) the products of a naive shoelace sum
//!     are ~1e12 and their rounding (~1e-4 m^2 each) would swamp the area of a
//!     parcel; the coordinate differences are small and (nearly) exact.
//!
//! A polygon is given by its vertices in order, implicitly closed; a repeated
//! closing vertex is harmless. All polygon functions need at least 3 finite
//! vertices (InvalidArgument otherwise). Self-intersecting polygons are not
//! detected; the signed areas of their lobes cancel.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::numerics::{PI, normalize_angle, tolerance};

use crate::coordinate::Coordinate2;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InverseResult {
    /// Radians, [0, 2*pi).
    pub azimuth: f64,
    /// Metres.
    pub distance: f64,
}

fn is_finite(c: Coordinate2) -> bool {
    c.northing.is_finite() && c.easting.is_finite()
}

fn check_polygon(vertices: &[Coordinate2]) -> Status {
    if vertices.len() < 3 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a polygon needs at least 3 vertices",
        )
        .with_context(format!("vertex count {}", vertices.len())));
    }
    if let Some(index) = vertices.iter().position(|vertex| !is_finite(*vertex)) {
        return Err(
            Error::new(ErrorCode::InvalidArgument, "polygon vertex is not finite")
                .with_context(format!("vertex index {index}")),
        );
    }
    Ok(())
}

// Sums of the shoelace integrals about the first vertex, in local east (x) /
// north (y) axes.
#[derive(Default)]
struct PolygonSums {
    twice_area: f64,
    // 6 * A * centroid offset east
    moment_east: f64,
    // 6 * A * centroid offset north
    moment_north: f64,
    extent_squared: f64,
}

fn polygon_sums(vertices: &[Coordinate2]) -> PolygonSums {
    let origin = vertices[0];
    let mut sums = PolygonSums::default();
    let mut x0 = 0.0;
    let mut y0 = 0.0;
    for i in 1..=vertices.len() {
        let next = vertices[i % vertices.len()];
        let x1 = next.easting - origin.easting;
        let y1 = next.northing - origin.northing;
        let cross = x0 * y1 - x1 * y0;
        sums.twice_area += cross;
        sums.moment_east += (x0 + x1) * cross;
        sums.moment_north += (y0 + y1) * cross;
        // std::max(a, b) is (a < b) ? b : a.
        let extent = x1 * x1 + y1 * y1;
        if sums.extent_squared < extent {
            sums.extent_squared = extent;
        }
        x0 = x1;
        y0 = y1;
    }
    sums
}

fn check_slope(slope_distance: f64, zenith_angle: f64) -> Status {
    if !slope_distance.is_finite() || slope_distance < 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "slope distance must be finite and not negative",
        )
        .with_context(format!(
            "slope distance {}",
            format_exact_real(slope_distance)
        )));
    }
    // A non-finite angle is refused first, so the range test sees finite values
    // only and agrees with the C++ "angle < 0 || angle > pi".
    if !zenith_angle.is_finite() || !(0.0..=PI).contains(&zenith_angle) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "zenith angle must lie in [0, pi]",
        )
        .with_context(format!("zenith angle {}", format_exact_real(zenith_angle))));
    }
    Ok(())
}

/// Azimuth and horizontal distance from `from` to `to`. InvalidArgument for
/// non-finite input or when the points coincide (azimuth undefined).
pub fn inverse(from: Coordinate2, to: Coordinate2) -> Result<InverseResult> {
    if !is_finite(from) || !is_finite(to) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "inverse: coordinates are not finite",
        ));
    }
    let delta_northing = to.northing - from.northing;
    let delta_easting = to.easting - from.easting;
    let distance = delta_northing.hypot(delta_easting);
    if distance <= tolerance::COORDINATE {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "inverse: the points coincide, the azimuth is undefined",
        )
        .with_context(format!("separation {} m", format_exact_real(distance))));
    }
    Ok(InverseResult {
        azimuth: normalize_angle(delta_easting.atan2(delta_northing)),
        distance,
    })
}

/// Position reached from `from` along `azimuth` (any finite value) over
/// `distance` >= 0. InvalidArgument otherwise.
pub fn forward(from: Coordinate2, azimuth: f64, distance: f64) -> Result<Coordinate2> {
    if !is_finite(from) || !azimuth.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "forward: start or azimuth is not finite",
        ));
    }
    if !distance.is_finite() || distance < 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "forward: distance must be finite and not negative",
        )
        .with_context(format!("distance {}", format_exact_real(distance))));
    }
    Ok(Coordinate2 {
        northing: from.northing + distance * azimuth.cos(),
        easting: from.easting + distance * azimuth.sin(),
    })
}

/// Positive when the vertices run counter-clockwise on the map (east to the
/// right, north up), negative when clockwise.
pub fn polygon_signed_area(vertices: &[Coordinate2]) -> Result<f64> {
    check_polygon(vertices)?;
    Ok(0.5 * polygon_sums(vertices).twice_area)
}

pub fn polygon_area(vertices: &[Coordinate2]) -> Result<f64> {
    Ok(polygon_signed_area(vertices)?.abs())
}

pub fn polygon_perimeter(vertices: &[Coordinate2]) -> Result<f64> {
    check_polygon(vertices)?;
    let mut perimeter = 0.0;
    for (i, a) in vertices.iter().enumerate() {
        let b = vertices[(i + 1) % vertices.len()];
        perimeter += (b.northing - a.northing).hypot(b.easting - a.easting);
    }
    Ok(perimeter)
}

/// Area centroid. InvalidArgument when the area is degenerate (collinear points).
pub fn polygon_centroid(vertices: &[Coordinate2]) -> Result<Coordinate2> {
    check_polygon(vertices)?;
    let sums = polygon_sums(vertices);
    // Degenerate when the area is negligible against the polygon's own extent
    // (a dimensionless ratio, hence the absolute tolerance).
    if !(sums.twice_area.abs() > tolerance::ABSOLUTE * sums.extent_squared) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "polygon centroid: the area is degenerate (collinear vertices)",
        ));
    }
    let origin = vertices[0];
    Ok(Coordinate2 {
        northing: origin.northing + sums.moment_north / (3.0 * sums.twice_area),
        easting: origin.easting + sums.moment_east / (3.0 * sums.twice_area),
    })
}

/// Horizontal distance of a slope distance measured at `zenith_angle`:
/// s * sin(z). Requires s >= 0 and z in [0, pi].
pub fn horizontal_distance(slope_distance: f64, zenith_angle: f64) -> Result<f64> {
    check_slope(slope_distance, zenith_angle)?;
    Ok(slope_distance * zenith_angle.sin())
}

/// Trigonometric height difference H(target mark) - H(instrument mark):
/// s * cos(z) + instrumentHeight - targetHeight. No curvature or refraction
/// correction, so it is meant for sight lengths below a few hundred metres.
pub fn trigonometric_height_difference(
    slope_distance: f64,
    zenith_angle: f64,
    instrument_height: f64,
    target_height: f64,
) -> Result<f64> {
    check_slope(slope_distance, zenith_angle)?;
    if !instrument_height.is_finite() || !target_height.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "instrument and target heights must be finite",
        ));
    }
    Ok(slope_distance * zenith_angle.cos() + instrument_height - target_height)
}
