//! Closed-form error ellipse shared by the public error_ellipse() (which
//! validates its input) and the network adjustment (whose covariance blocks are
//! positive semi-definite by construction and must not be rejected over
//! rounding). The twin of the C++ private header ellipse_core.hpp.

use katana_math::numerics::PI;

use crate::error_propagation::{Covariance2, ErrorEllipse};

/// Precondition: finite entries, non-negative variances.
pub(crate) fn ellipse_of(covariance: Covariance2) -> ErrorEllipse {
    let mean = 0.5 * (covariance.northing + covariance.easting);
    let half_difference = 0.5 * (covariance.northing - covariance.easting);
    let root = half_difference.hypot(covariance.northing_easting);

    // mean - root is >= 0 analytically; rounding may push it a hair below.
    // std::max(a, 0.0) is (a < 0.0) ? 0.0 : a.
    let minor_variance = mean - root;
    let semi_minor = if minor_variance < 0.0 {
        0.0
    } else {
        minor_variance
    }
    .sqrt();
    // The major axis direction t (from north towards east) maximises
    //   sN^2 cos^2 t + 2 sNE sin t cos t + sE^2 sin^2 t,
    // which gives tan 2t = 2 sNE / (sN^2 - sE^2).
    let mut orientation =
        0.5 * (2.0 * covariance.northing_easting).atan2(covariance.northing - covariance.easting);
    if orientation < 0.0 {
        orientation += PI;
    }
    ErrorEllipse {
        semi_major: (mean + root).sqrt(),
        semi_minor,
        orientation: if orientation >= PI { 0.0 } else { orientation },
    }
}
