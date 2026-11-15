//! First-order propagation of variances: Sigma_y = J Sigma_x J^T for y = f(x)
//! linearised at the given values, plus the error ellipse of a 2D position.
//! First order means the standard deviations must be small against the
//! distances involved (sigma_azimuth << 1 rad, sigma << d), which holds for any
//! realistic survey.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::numerics::tolerance;

use crate::coordinate::Coordinate2;
use crate::ellipse_core::ellipse_of;
use crate::matrix::Matrix;

/// Symmetric 2 x 2 covariance of a horizontal position, m^2.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Covariance2 {
    /// Variance of the northing.
    pub northing: f64,
    /// Variance of the easting.
    pub easting: f64,
    /// Covariance.
    pub northing_easting: f64,
}

/// Standard (1 sigma, 39.3 % confidence) error ellipse.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ErrorEllipse {
    /// Metres.
    pub semi_major: f64,
    /// Metres.
    pub semi_minor: f64,
    /// Azimuth of the semi-major axis, clockwise from north, in [0, pi). 0 for a
    /// circular ellipse, whose orientation is arbitrary.
    pub orientation: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InverseUncertainty {
    /// Radians.
    pub sigma_azimuth: f64,
    /// Metres.
    pub sigma_distance: f64,
    /// Radians * metres.
    pub covariance: f64,
}

fn check_covariance(c: Covariance2, name: &str) -> Status {
    if !c.northing.is_finite() || !c.easting.is_finite() || !c.northing_easting.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{name} is not finite"),
        ));
    }
    if c.northing < 0.0 || c.easting < 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{name} has a negative variance"),
        ));
    }
    // Cauchy-Schwarz with the relative tolerance as head-room for a covariance
    // that is singular up to rounding (perfectly correlated components).
    let bound = c.northing * c.easting;
    if c.northing_easting * c.northing_easting > bound * (1.0 + tolerance::RELATIVE) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{name} is not positive semi-definite"),
        ));
    }
    Ok(())
}

// std::max(value, 0.0) is (value < 0.0) ? 0.0 : value.
fn at_least_zero(value: f64) -> f64 {
    if value < 0.0 { 0.0 } else { value }
}

/// Eigen-decomposition of the covariance in closed form:
///   lambda = (sN^2 + sE^2)/2 +- sqrt(((sN^2 - sE^2)/2)^2 + sNE^2)
///   orientation = atan2(2 sNE, sN^2 - sE^2) / 2
/// InvalidArgument when the matrix is not a covariance (negative variance,
/// |sNE| > sN sE, non-finite).
pub fn error_ellipse(covariance: Covariance2) -> Result<ErrorEllipse> {
    check_covariance(covariance, "covariance")?;
    Ok(ellipse_of(covariance))
}

/// Factor that scales the standard ellipse to confidence level `probability` in
/// (0, 1) when the variance factor is known: sqrt(chi2(2, p)) = sqrt(-2 ln(1 - p));
/// 2.4477 for 95 %. (With an estimated variance factor the rigorous factor is
/// sqrt(2 F(2, r, p)), which tends to this value as r grows.)
pub fn ellipse_confidence_scale(probability: f64) -> Result<f64> {
    if !(probability > 0.0 && probability < 1.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "probability must lie strictly between 0 and 1",
        )
        .with_context(format!("probability {}", format_exact_real(probability))));
    }
    // The chi-square distribution with 2 degrees of freedom has the closed-form
    // cdf 1 - exp(-x / 2).
    Ok((-2.0 * (-probability).ln_1p()).sqrt())
}

/// Covariance of the point computed by forward(start, azimuth, distance) when
/// the start has covariance `start` and azimuth and distance are uncorrelated
/// with standard deviations `sigma_azimuth` (radians) and `sigma_distance`
/// (metres):
///   J = [ -d sin(az)  cos(az) ]   (rows N, E; columns azimuth, distance)
///       [  d cos(az)  sin(az) ]
///   Sigma = Sigma_start + J diag(sigmaAzimuth^2, sigmaDistance^2) J^T
pub fn propagate_forward(
    start: Covariance2,
    azimuth: f64,
    distance: f64,
    sigma_azimuth: f64,
    sigma_distance: f64,
) -> Result<Covariance2> {
    check_covariance(start, "start covariance")?;
    if !azimuth.is_finite() || !distance.is_finite() || distance < 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "azimuth must be finite and distance finite and not negative",
        ));
    }
    if !sigma_azimuth.is_finite()
        || !sigma_distance.is_finite()
        || sigma_azimuth < 0.0
        || sigma_distance < 0.0
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "standard deviations must be finite and not negative",
        ));
    }
    let sine = azimuth.sin();
    let cosine = azimuth.cos();
    // (d sigma_az)^2
    let cross_track = distance * distance * sigma_azimuth * sigma_azimuth;
    let along_track = sigma_distance * sigma_distance;

    Ok(Covariance2 {
        northing: start.northing + sine * sine * cross_track + cosine * cosine * along_track,
        easting: start.easting + cosine * cosine * cross_track + sine * sine * along_track,
        northing_easting: start.northing_easting + sine * cosine * (along_track - cross_track),
    })
}

/// Standard deviations of the azimuth and distance computed by inverse(from, to)
/// from two mutually uncorrelated positions. InvalidArgument when the points
/// coincide.
pub fn propagate_inverse(
    from: Coordinate2,
    to: Coordinate2,
    from_covariance: Covariance2,
    to_covariance: Covariance2,
) -> Result<InverseUncertainty> {
    check_covariance(from_covariance, "from covariance")?;
    check_covariance(to_covariance, "to covariance")?;
    let dn = to.northing - from.northing;
    let de = to.easting - from.easting;
    let d = dn.hypot(de);
    if !d.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "coordinates are not finite",
        ));
    }
    if d <= tolerance::COORDINATE {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the points coincide, the azimuth is undefined",
        ));
    }
    // The Jacobians with respect to the two ends differ only in sign, so for
    // uncorrelated ends the covariances simply add.
    let nn = from_covariance.northing + to_covariance.northing;
    let ee = from_covariance.easting + to_covariance.easting;
    let ne = from_covariance.northing_easting + to_covariance.northing_easting;

    // Rows of J: azimuth (-dE, dN) / d^2, distance (dN, dE) / d.
    let az_n = -de / (d * d);
    let az_e = dn / (d * d);
    let d_n = dn / d;
    let d_e = de / d;

    Ok(InverseUncertainty {
        sigma_azimuth: at_least_zero(az_n * az_n * nn + 2.0 * az_n * az_e * ne + az_e * az_e * ee)
            .sqrt(),
        sigma_distance: at_least_zero(d_n * d_n * nn + 2.0 * d_n * d_e * ne + d_e * d_e * ee)
            .sqrt(),
        covariance: az_n * d_n * nn + (az_n * d_e + az_e * d_n) * ne + az_e * d_e * ee,
    })
}

/// General law: returns J Sigma J^T (m x m, exactly symmetric) for a Jacobian
/// J (m x k) and a symmetric covariance Sigma (k x k). InvalidArgument on
/// dimension mismatch, non-finite values or an asymmetric Sigma.
pub fn propagate_covariance(jacobian: &Matrix, covariance: &Matrix) -> Result<Matrix> {
    if !jacobian.is_consistent() || !covariance.is_consistent() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "matrix storage does not match its dimensions",
        ));
    }
    if covariance.rows != covariance.cols || jacobian.cols != covariance.rows {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "dimension mismatch: J is m x k, Sigma must be k x k",
        )
        .with_context(format!(
            "J {} x {}, Sigma {} x {}",
            jacobian.rows, jacobian.cols, covariance.rows, covariance.cols
        )));
    }
    if !jacobian.values.iter().all(|value| value.is_finite())
        || !covariance.values.iter().all(|value| value.is_finite())
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "matrices contain non-finite values",
        ));
    }
    let k = covariance.rows;
    for i in 0..k {
        for j in i + 1..k {
            let scale = (covariance[(i, i)] * covariance[(j, j)]).abs().sqrt();
            if (covariance[(i, j)] - covariance[(j, i)]).abs() > tolerance::RELATIVE * scale {
                return Err(
                    Error::new(ErrorCode::InvalidArgument, "covariance is not symmetric")
                        .with_context(format!("entry ({i}, {j})")),
                );
            }
        }
    }

    let m = jacobian.rows;
    // J Sigma
    let mut product = Matrix::new(m, k);
    for r in 0..m {
        for c in 0..k {
            let mut sum = 0.0;
            for i in 0..k {
                sum += jacobian[(r, i)] * covariance[(i, c)];
            }
            product[(r, c)] = sum;
        }
    }
    let mut result = Matrix::new(m, m);
    for r in 0..m {
        for c in r..m {
            let mut sum = 0.0;
            for i in 0..k {
                sum += product[(r, i)] * jacobian[(c, i)];
            }
            result[(r, c)] = sum;
            result[(c, r)] = sum;
        }
    }
    Ok(result)
}
