#include "katana/survey/statistics.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <string>

#include "katana/math/numerics.hpp"

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::math::kPi;
using katana::math::kTwoPi;

namespace {

// Argument from which the Stirling series below is accurate to double
// precision: its first omitted term, 691 / (360360 z^11), is 1e-16 at z = 16.
constexpr double kStirlingMinimum = 16.0;

// Loop guards, not tolerances. Both expansions need O(sqrt(a)) terms near
// x = a; the cap is reached only beyond ~1e9 degrees of freedom.
constexpr int kMaxSeriesTerms = 1'000'000;
// A double has fewer than 2100 binades plus 52 mantissa bits to bisect through.
constexpr int kMaxBisections = 4000;

constexpr double kEpsilon = std::numeric_limits<double>::epsilon();
// Smallest magnitude allowed for a continued-fraction denominator (modified
// Lentz algorithm) so that its reciprocal stays finite.
constexpr double kTiny = std::numeric_limits<double>::min() / kEpsilon;

// Tail of the Stirling series: lnGamma(z) - [(z - 1/2) ln z - z + ln(2 pi)/2],
// for z >= kStirlingMinimum. Coefficients B_2k / (2k (2k - 1)).
double stirlingCorrection(double z)
{
    const double inverse = 1.0 / z;
    const double inverseSquared = inverse * inverse;
    return inverse *
           (1.0 / 12.0 -
            inverseSquared *
                (1.0 / 360.0 -
                 inverseSquared *
                     (1.0 / 1260.0 - inverseSquared * (1.0 / 1680.0 - inverseSquared / 1188.0))));
}

// ln Gamma(a) for a = k/2, k a positive integer, a < kStirlingMinimum. Those are
// the only arguments a chi-square distribution produces below the Stirling range.
// Own implementation because std::lgamma writes the global `signgam` on some C
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
double logGammaHalfInteger(double a)
{
    double product = 1.0;
    for (double factor = a - 1.0; factor > 0.0; factor -= 1.0) {
        product *= factor;
    }
    // Half-odd a carries the sqrt(pi) of Gamma(1/2); integral a does not.
    const double logRootPi = a == std::floor(a) ? 0.0 : 0.5 * std::log(kPi);
    return std::log(product) + logRootPi;
}

// ln( x^a e^-x / Gamma(a) ), for a = k/2 with k a positive integer.
double logPrefactor(double a, double x)
{
    if (a < kStirlingMinimum) {
        return a * std::log(x) - x - logGammaHalfInteger(a);
    }
    // For large a the three terms are each ~a ln a and cancel to O(ln a).
    // Substituting Stirling's formula removes the cancellation analytically:
    //   a ln x - x - lnGamma(a) = a (ln(1 + mu) - mu) + ln(a / 2 pi)/2 - s(a),
    // with mu = (x - a) / a and s the Stirling tail.
    const double mu = (x - a) / a;
    return a * (std::log1p(mu) - mu) + 0.5 * std::log(a / kTwoPi) - stirlingCorrection(a);
}

// Regularised lower incomplete gamma function P(a, x), x > 0 and a = k/2 with k
// a positive integer (the precondition of logPrefactor above).
Result<double> regularizedGammaP(double a, double x)
{
    const double prefactor = std::exp(logPrefactor(a, x));
    if (x < a + 1.0) {
        // P = prefactor * sum_{n>=0} x^n / (a (a+1) ... (a+n)); all terms positive.
        double term = 1.0 / a;
        double sum = term;
        for (int n = 1; n <= kMaxSeriesTerms; ++n) {
            term *= x / (a + static_cast<double>(n));
            sum += term;
            if (term <= sum * kEpsilon) {
                return std::min(prefactor * sum, 1.0);
            }
        }
    } else {
        // Q = prefactor / (x+1-a - 1(1-a)/(x+3-a - 2(2-a)/(x+5-a - ...))),
        // evaluated with the modified Lentz algorithm.
        double b = x + 1.0 - a;
        double c = 1.0 / kTiny;
        double d = 1.0 / b;
        double fraction = d;
        for (int n = 1; n <= kMaxSeriesTerms; ++n) {
            const double index = static_cast<double>(n);
            const double an = -index * (index - a);
            b += 2.0;
            d = an * d + b;
            if (std::abs(d) < kTiny) {
                d = kTiny;
            }
            c = b + an / c;
            if (std::abs(c) < kTiny) {
                c = kTiny;
            }
            d = 1.0 / d;
            const double delta = d * c;
            fraction *= delta;
            if (std::abs(delta - 1.0) <= kEpsilon) {
                return std::max(1.0 - prefactor * fraction, 0.0);
            }
        }
    }
    return makeError(ErrorCode::Internal, "incomplete gamma expansion did not converge",
                     "a=" + std::to_string(a) + ", x=" + std::to_string(x));
}

} // namespace

Result<double> chiSquareCdf(double x, std::size_t degreesOfFreedom)
{
    if (degreesOfFreedom == 0) {
        return makeError(ErrorCode::InvalidArgument,
                         "chi-square needs at least one degree of freedom");
    }
    if (!std::isfinite(x)) {
        return makeError(ErrorCode::InvalidArgument, "chi-square argument is not finite");
    }
    if (x <= 0.0) {
        return 0.0;
    }
    return regularizedGammaP(0.5 * static_cast<double>(degreesOfFreedom), 0.5 * x);
}

Result<double> chiSquareQuantile(double probability, std::size_t degreesOfFreedom)
{
    if (degreesOfFreedom == 0) {
        return makeError(ErrorCode::InvalidArgument,
                         "chi-square needs at least one degree of freedom");
    }
    if (!(probability > 0.0 && probability < 1.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "probability must lie strictly between 0 and 1",
                         "probability " + std::to_string(probability));
    }

    // Bracket: the mean of the distribution is its degrees of freedom.
    double low = 0.0;
    double high = static_cast<double>(degreesOfFreedom);
    for (int i = 0; i < kMaxBisections; ++i) {
        const auto cdf = chiSquareCdf(high, degreesOfFreedom);
        if (!cdf) {
            return cdf.error();
        }
        if (*cdf >= probability) {
            break;
        }
        low = high;
        high *= 2.0;
    }

    // Bisection down to adjacent doubles: robust, deterministic, and the cdf is
    // monotone so there is exactly one root.
    for (int i = 0; i < kMaxBisections; ++i) {
        const double middle = low + 0.5 * (high - low);
        if (middle <= low || middle >= high) {
            break;
        }
        const auto cdf = chiSquareCdf(middle, degreesOfFreedom);
        if (!cdf) {
            return cdf.error();
        }
        if (*cdf < probability) {
            low = middle;
        } else {
            high = middle;
        }
    }
    return high;
}

} // namespace katana::survey
