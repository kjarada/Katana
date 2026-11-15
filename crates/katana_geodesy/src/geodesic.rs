//! Geodesics on the ellipsoid and the grid-to-ground reduction factors.
//!
//! The inverse and direct problems are solved with the geodesic routines that
//! ship with PROJ (C. F. F. Karney, "Algorithms for geodesics", J. Geodesy 87,
//! 2013): accurate to about 15 nanometres and convergent for every pair of
//! points, including nearly antipodal ones where Vincenty's iteration fails.
//!
//! Conventions: positions are `GeographicCoordinate` (degrees); heights are
//! ignored - a geodesic lies ON the ellipsoid. Azimuths are DEGREES CLOCKWISE
//! FROM NORTH in [0, 360), the surveying convention; this differs
//! deliberately from the radians-counter-clockwise convention of
//! katana_geometry, hence the unit in every field name.
//!
//! These functions are stateless and thread-safe.

use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::numerics::DEG_TO_RAD;

use crate::coordinate::GeographicCoordinate;
use crate::ellipsoid::Ellipsoid;
use crate::proj_internal;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GeodesicInverseResult {
    /// Along the ellipsoid surface.
    pub distance_metres: f64,
    /// At `from`, towards `to`.
    pub forward_azimuth_degrees: f64,
    /// At `to`, back towards `from`.
    pub reverse_azimuth_degrees: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GeodesicDirectResult {
    /// The height is 0.
    pub position: GeographicCoordinate,
    /// At `position`, back towards the start.
    pub reverse_azimuth_degrees: f64,
}

fn validate_ellipsoid(ellipsoid: &Ellipsoid) -> Status {
    let axis_valid = ellipsoid.semi_major_axis.is_finite() && ellipsoid.semi_major_axis > 0.0;
    // 1/f == 0 encodes a sphere; otherwise f must be a proper flattening < 1.
    let flattening_valid = ellipsoid.inverse_flattening == 0.0
        || (ellipsoid.inverse_flattening.is_finite() && ellipsoid.inverse_flattening > 1.0);
    if axis_valid && flattening_valid {
        return Ok(());
    }
    Err(Error::new(
        ErrorCode::InvalidArgument,
        "ellipsoid needs a positive semi-major axis and an inverse flattening that is 0 (sphere) \
         or greater than 1",
    ))
}

fn validate_position(position: &GeographicCoordinate) -> Status {
    if position.latitude.is_finite()
        && position.longitude.is_finite()
        && position.latitude.abs() <= 90.0
    {
        return Ok(());
    }
    Err(Error::new(
        ErrorCode::InvalidArgument,
        "geographic position must be finite with latitude in [-90, 90]",
    ))
}

// Wraps an azimuth from the library's (-180, 180] into the survey range
// [0, 360). Rust's % on floats is C's fmod.
fn to_compass_degrees(azimuth_degrees: f64) -> f64 {
    let mut wrapped = azimuth_degrees % 360.0;
    if wrapped < 0.0 {
        wrapped += 360.0;
    }
    if wrapped >= 360.0 { 0.0 } else { wrapped } // a tiny negative value can round up to 360
}

/// Shortest path between two positions. For coincident points the distance
/// is 0 and the azimuths carry no information. Errors (`InvalidArgument`):
/// ellipsoid with a <= 0 or an inverse flattening in (0, 1]; non-finite
/// positions; latitude outside [-90, 90].
pub fn geodesic_inverse(
    ellipsoid: &Ellipsoid,
    from: &GeographicCoordinate,
    to: &GeographicCoordinate,
) -> Result<GeodesicInverseResult> {
    validate_ellipsoid(ellipsoid)?;
    for position in [from, to] {
        validate_position(position)?;
    }

    // The azimuth at the end is the direction of travel on arrival, NOT the
    // back azimuth.
    let (distance, azimuth_at_start, azimuth_at_end) = proj_internal::geodesic_inverse(
        ellipsoid.semi_major_axis,
        ellipsoid.flattening(),
        [from.latitude, from.longitude],
        [to.latitude, to.longitude],
    );
    if !distance.is_finite() || !azimuth_at_start.is_finite() || !azimuth_at_end.is_finite() {
        return Err(Error::new(
            ErrorCode::Internal,
            "geodesic inverse computation did not produce a finite result",
        ));
    }
    Ok(GeodesicInverseResult {
        distance_metres: distance,
        forward_azimuth_degrees: to_compass_degrees(azimuth_at_start),
        reverse_azimuth_degrees: to_compass_degrees(azimuth_at_end + 180.0),
    })
}

/// Position reached from `from` after `distance_metres` along the geodesic
/// that leaves with `azimuth_degrees`. A negative distance travels backwards.
/// The resulting longitude is in [-180, 180].
pub fn geodesic_direct(
    ellipsoid: &Ellipsoid,
    from: &GeographicCoordinate,
    azimuth_degrees: f64,
    distance_metres: f64,
) -> Result<GeodesicDirectResult> {
    validate_ellipsoid(ellipsoid)?;
    validate_position(from)?;
    if !azimuth_degrees.is_finite() || !distance_metres.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "azimuth and distance must be finite",
        ));
    }

    let (latitude, longitude, azimuth_at_end) = proj_internal::geodesic_direct(
        ellipsoid.semi_major_axis,
        ellipsoid.flattening(),
        [from.latitude, from.longitude],
        azimuth_degrees,
        distance_metres,
    );
    if !latitude.is_finite() || !longitude.is_finite() || !azimuth_at_end.is_finite() {
        return Err(Error::new(
            ErrorCode::Internal,
            "geodesic direct computation did not produce a finite result",
        ));
    }
    Ok(GeodesicDirectResult {
        position: GeographicCoordinate::new(latitude, longitude, 0.0),
        reverse_azimuth_degrees: to_compass_degrees(azimuth_at_end + 180.0),
    })
}

/// Gaussian mean radius of curvature sqrt(M N) at a latitude, in metres: the
/// radius of the sphere that best fits the ellipsoid there, used for the
/// elevation factor.
pub fn gaussian_mean_radius(ellipsoid: &Ellipsoid, latitude_degrees: f64) -> f64 {
    // M = a (1 - e^2) / w^3, N = a / w, w^2 = 1 - e^2 sin^2(phi)  =>
    // sqrt(M N) = a sqrt(1 - e^2) / w^2.
    let e_squared = ellipsoid.eccentricity_squared();
    let sin_latitude = (latitude_degrees * DEG_TO_RAD).sin();
    ellipsoid.semi_major_axis * (1.0 - e_squared).sqrt()
        / (1.0 - e_squared * sin_latitude * sin_latitude)
}

/// Elevation (sea-level) factor R / (R + h): ratio of a distance on the
/// ellipsoid to the same distance measured horizontally at ellipsoidal height
/// h. The combined grid-to-ground factor is point_scale_factor *
/// elevation_factor.
pub fn elevation_factor(radius_metres: f64, ellipsoidal_height_metres: f64) -> f64 {
    radius_metres / (radius_metres + ellipsoidal_height_metres)
}
