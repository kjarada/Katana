//! Chi-square distribution for the global test of an adjustment. Implemented
//! from first principles, without external libraries:
//!
//!   cdf(x; k) = P(k/2, x/2), the regularised lower incomplete gamma function,
//!   evaluated by its power series for x/2 < k/2 + 1 and by the continued
//!   fraction of Q = 1 - P otherwise (both summed to machine precision). The
//!   prefactor x^a e^-x / Gamma(a) is formed in logarithms, for a >= 16 in the
//!   cancellation-free form a*(log1p(mu) - mu) with mu = (x - a)/a.
//!
//!   quantile(p; k) inverts the cdf by bisection to the resolution of a double.
//!
//! Accuracy: relative error of the cdf below ~1e-13 for k up to ~1e4 (verified
//! in the tests against the closed forms for k = 1, 2, 4 and tabulated critical
//! values); the quantile inherits it. Pure functions, safe to call concurrently.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_math::numerics::{PI, TWO_PI};

// Argument from which the Stirling series below is accurate to double
// precision: its first omitted term, 691 / (360360 z^11), is 1e-16 at z = 16.
const STIRLING_MINIMUM: f64 = 16.0;

// Loop guards, not tolerances. Both expansions need O(sqrt(a)) terms near
// x = a; the cap is reached only beyond ~1e9 degrees of freedom.
const MAX_SERIES_TERMS: i32 = 1_000_000;
// A double has fewer than 2100 binades plus 52 mantissa bits to bisect through.
const MAX_BISECTIONS: i32 = 4000;

const EPSILON: f64 = f64::EPSILON;
// Smallest magnitude allowed for a continued-fraction denominator (modified
// Lentz algorithm) so that its reciprocal stays finite.
const TINY: f64 = f64::MIN_POSITIVE / EPSILON;

// Tail of the Stirling series: lnGamma(z) - [(z - 1/2) ln z - z + ln(2 pi)/2],
// for z >= STIRLING_MINIMUM. Coefficients B_2k / (2k (2k - 1)).
fn stirling_correction(z: f64) -> f64 {
    let inverse = 1.0 / z;
    let inverse_squared = inverse * inverse;
    inverse
        * (1.0 / 12.0
            - inverse_squared
                * (1.0 / 360.0
                    - inverse_squared
                        * (1.0 / 1260.0
                            - inverse_squared * (1.0 / 1680.0 - inverse_squared / 1188.0))))
}

// ln Gamma(a) for a = k/2, k a positive integer, a < STIRLING_MINIMUM. Those are
// the only arguments a chi-square distribution produces below the Stirling range.
// Own implementation because lgamma writes the global `signgam` on some C
// libraries and is then not thread-safe.
//
// Gamma(n) = (n-1)! and Gamma(n + 1/2) = (2n-1)!! / 2^n * sqrt(pi), so the
// descending product below is exact: it is an integer numerator over a power of
// two, and the widest numerator in range, 29!! at a = 15.5, needs 53 bits. The
// result is therefore one correctly rounded logarithm.
//
// Reaching these values by shifting a up into the Stirling range and subtracting
// ln(a (a+1) ...) instead would be a full ulp worse in absolute terms, because
// both halves are then of size lnGamma(16) ~ 27.9. That matters here and nowhere
// else: lnGamma has zeros at a = 1 and a = 2, so an absolute error of ~4e-15
// there is an outright 4e-15 relative error in the prefactor, and so in the cdf
// for 2 and 4 degrees of freedom.
fn log_gamma_half_integer(a: f64) -> f64 {
    let mut product = 1.0;
    let mut factor = a - 1.0;
    while factor > 0.0 {
        product *= factor;
        factor -= 1.0;
    }
    // Half-odd a carries the sqrt(pi) of Gamma(1/2); integral a does not.
    let log_root_pi = if a == a.floor() { 0.0 } else { 0.5 * PI.ln() };
    product.ln() + log_root_pi
}

// ln( x^a e^-x / Gamma(a) ), for a = k/2 with k a positive integer.
fn log_prefactor(a: f64, x: f64) -> f64 {
    if a < STIRLING_MINIMUM {
        return a * x.ln() - x - log_gamma_half_integer(a);
    }
    // For large a the three terms are each ~a ln a and cancel to O(ln a).
    // Substituting Stirling's formula removes the cancellation analytically:
    //   a ln x - x - lnGamma(a) = a (ln(1 + mu) - mu) + ln(a / 2 pi)/2 - s(a),
    // with mu = (x - a) / a and s the Stirling tail.
    let mu = (x - a) / a;
    a * (mu.ln_1p() - mu) + 0.5 * (a / TWO_PI).ln() - stirling_correction(a)
}

// Regularised lower incomplete gamma function P(a, x), x > 0 and a = k/2 with k
// a positive integer (the precondition of log_prefactor above).
fn regularized_gamma_p(a: f64, x: f64) -> Result<f64> {
    let prefactor = log_prefactor(a, x).exp();
    if x < a + 1.0 {
        // P = prefactor * sum_{n>=0} x^n / (a (a+1) ... (a+n)); all terms positive.
        let mut term = 1.0 / a;
        let mut sum = term;
        for n in 1..=MAX_SERIES_TERMS {
            term *= x / (a + f64::from(n));
            sum += term;
            if term <= sum * EPSILON {
                let value = prefactor * sum;
                // std::min(value, 1.0) is (1.0 < value) ? 1.0 : value.
                return Ok(if 1.0 < value { 1.0 } else { value });
            }
        }
    } else {
        // Q = prefactor / (x+1-a - 1(1-a)/(x+3-a - 2(2-a)/(x+5-a - ...))),
        // evaluated with the modified Lentz algorithm.
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / TINY;
        let mut d = 1.0 / b;
        let mut fraction = d;
        for n in 1..=MAX_SERIES_TERMS {
            let index = f64::from(n);
            let an = -index * (index - a);
            b += 2.0;
            d = an * d + b;
            if d.abs() < TINY {
                d = TINY;
            }
            c = b + an / c;
            if c.abs() < TINY {
                c = TINY;
            }
            d = 1.0 / d;
            let delta = d * c;
            fraction *= delta;
            if (delta - 1.0).abs() <= EPSILON {
                let value = 1.0 - prefactor * fraction;
                // std::max(value, 0.0) is (value < 0.0) ? 0.0 : value.
                return Ok(if value < 0.0 { 0.0 } else { value });
            }
        }
    }
    Err(Error::new(
        ErrorCode::Internal,
        "incomplete gamma expansion did not converge",
    )
    .with_context(format!(
        "a={}, x={}",
        format_exact_real(a),
        format_exact_real(x)
    )))
}

/// P(X <= x) for X ~ chi-square with `degrees_of_freedom` >= 1. x < 0 gives 0.
/// InvalidArgument for a non-finite x or zero degrees of freedom; Internal when
/// the series fails to converge (not expected below ~1e9 degrees of freedom).
pub fn chi_square_cdf(x: f64, degrees_of_freedom: usize) -> Result<f64> {
    if degrees_of_freedom == 0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "chi-square needs at least one degree of freedom",
        ));
    }
    if !x.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "chi-square argument is not finite",
        ));
    }
    if x <= 0.0 {
        return Ok(0.0);
    }
    regularized_gamma_p(0.5 * degrees_of_freedom as f64, 0.5 * x)
}

/// x with P(X <= x) = probability, for probability in the open interval (0, 1).
pub fn chi_square_quantile(probability: f64, degrees_of_freedom: usize) -> Result<f64> {
    if degrees_of_freedom == 0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "chi-square needs at least one degree of freedom",
        ));
    }
    if !(probability > 0.0 && probability < 1.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "probability must lie strictly between 0 and 1",
        )
        .with_context(format!("probability {}", format_exact_real(probability))));
    }

    // Bracket: the mean of the distribution is its degrees of freedom.
    let mut low = 0.0;
    let mut high = degrees_of_freedom as f64;
    for _ in 0..MAX_BISECTIONS {
        if chi_square_cdf(high, degrees_of_freedom)? >= probability {
            break;
        }
        low = high;
        high *= 2.0;
    }

    // Bisection down to adjacent doubles: robust, deterministic, and the cdf is
    // monotone so there is exactly one root.
    for _ in 0..MAX_BISECTIONS {
        let middle = low + 0.5 * (high - low);
        if middle <= low || middle >= high {
            break;
        }
        if chi_square_cdf(middle, degrees_of_freedom)? < probability {
            low = middle;
        } else {
            high = middle;
        }
    }
    Ok(high)
}
