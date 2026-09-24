#include "reduction_formulas.hpp"

#include <cmath>

#include "katana/math/numerics.hpp"

namespace katana::survey::detail {

using katana::math::kPi;

namespace {

constexpr double kKelvinOffset = 273.15;
constexpr double kStandardPressure = 1013.25; // hPa

// N_gr of standard air for the carrier, IUGG 1999.
double standardGroupRefractivity()
{
    const double l2 = kCarrierWavelengthMicrometres * kCarrierWavelengthMicrometres;
    return 287.6155 + 4.88660 / l2 + 0.06800 / (l2 * l2);
}

// Bisection to the resolution of a double: a monotone function, a bracket, and
// no derivative that could send Newton out of it.
template <typename Increasing>
double solveIncreasing(Increasing&& f, double target, double low, double high)
{
    for (int i = 0; i < 200 && low < high; ++i) {
        const double middle = 0.5 * (low + high);
        if (middle <= low || middle >= high) {
            break;
        }
        if (f(middle) < target) {
            low = middle;
        } else {
            high = middle;
        }
    }
    return 0.5 * (low + high);
}

// P(|T| < t) for Student's t with nu degrees of freedom, exactly, by
// Abramowitz & Stegun 26.7.3 (nu odd) and 26.7.4 (nu even).
double studentCentralProbability(double t, std::size_t nu)
{
    const double theta = std::atan(t / std::sqrt(static_cast<double>(nu)));
    const double s = std::sin(theta);
    const double c = std::cos(theta);
    const double c2 = c * c;
    if (nu % 2 == 0) {
        // sin(theta) (1 + 1/2 cos^2 + 1*3/(2*4) cos^4 + ... ) to cos^(nu-2)
        double term = 1.0;
        double sum = 1.0;
        for (std::size_t k = 2; k + 2 <= nu; k += 2) {
            term *= c2 * static_cast<double>(k - 1) / static_cast<double>(k);
            sum += term;
        }
        return s * sum;
    }
    if (nu == 1) {
        return 2.0 * theta / kPi;
    }
    // 2/pi (theta + sin cos (1 + 2/3 cos^2 + 2*4/(3*5) cos^4 + ...)) to cos^(nu-3)
    double term = 1.0;
    double sum = 1.0;
    for (std::size_t k = 2; k + 3 <= nu; k += 2) {
        term *= c2 * static_cast<double>(k) / static_cast<double>(k + 1);
        sum += term;
    }
    return 2.0 / kPi * (theta + s * c * sum);
}

} // namespace

double saturationVapourPressure(double temperatureCelsius)
{
    return 6.1078 * std::pow(10.0, 7.5 * temperatureCelsius / (237.3 + temperatureCelsius));
}

double groupRefractivity(double temperatureCelsius, double pressureHectopascals,
                         double relativeHumidityPercent)
{
    const double kelvin = temperatureCelsius + kKelvinOffset;
    const double vapour =
        relativeHumidityPercent / 100.0 * saturationVapourPressure(temperatureCelsius);
    return kKelvinOffset / kStandardPressure * standardGroupRefractivity() *
               pressureHectopascals / kelvin -
           11.27 * vapour / kelvin;
}

double atmosphericPpm(double temperatureCelsius, double pressureHectopascals,
                      double relativeHumidityPercent)
{
    const double reference = groupRefractivity(kReferenceTemperatureCelsius,
                                               kReferencePressureHectopascals,
                                               kReferenceHumidityPercent);
    return reference -
           groupRefractivity(temperatureCelsius, pressureHectopascals, relativeHumidityPercent);
}

SlopeReduction reduceSlope(double slopeDistance, double zenithAngle, bool curvatureAndRefraction,
                           double refraction, double earthRadius)
{
    SlopeReduction reduction;
    reduction.measuredVertical = slopeDistance * std::cos(zenithAngle);
    reduction.measuredHorizontal = slopeDistance * std::sin(zenithAngle);
    if (curvatureAndRefraction) {
        const double a = (1.0 - 0.5 * refraction) / earthRadius;
        const double b = (1.0 - refraction) / (2.0 * earthRadius);
        reduction.horizontalCorrection =
            -a * reduction.measuredVertical * reduction.measuredHorizontal;
        reduction.verticalCorrection =
            b * reduction.measuredHorizontal * reduction.measuredHorizontal;
    }
    return reduction;
}

double heightReductionFactor(double meanHeight, double earthRadius)
{
    return earthRadius / (earthRadius + meanHeight);
}

GeocentricCoordinate geocentricFromGeodetic(const GeodeticCoordinate& geodetic)
{
    const double e2 = kGrs80Flattening * (2.0 - kGrs80Flattening);
    const double sinLatitude = std::sin(geodetic.latitude);
    const double cosLatitude = std::cos(geodetic.latitude);
    // Radius of curvature in the prime vertical.
    const double nu = kGrs80SemiMajor / std::sqrt(1.0 - e2 * sinLatitude * sinLatitude);
    const double h = geodetic.ellipsoidalHeight;
    return GeocentricCoordinate{(nu + h) * cosLatitude * std::cos(geodetic.longitude),
                                (nu + h) * cosLatitude * std::sin(geodetic.longitude),
                                (nu * (1.0 - e2) + h) * sinLatitude};
}

GeodeticCoordinate geodeticFromGeocentric(const GeocentricCoordinate& geocentric)
{
    const double a = kGrs80SemiMajor;
    const double f = kGrs80Flattening;
    const double b = a * (1.0 - f);
    const double e2 = f * (2.0 - f);
    const double ep2 = e2 / (1.0 - e2);
    const double p = std::hypot(geocentric.x, geocentric.y);
    GeodeticCoordinate geodetic;
    geodetic.longitude = std::atan2(geocentric.y, geocentric.x);
    // Bowring: iterate on the parametric latitude.
    double beta = std::atan2(geocentric.z * a, p * b);
    double latitude = 0.0;
    for (int i = 0; i < 4; ++i) {
        const double sinBeta = std::sin(beta);
        const double cosBeta = std::cos(beta);
        latitude = std::atan2(geocentric.z + ep2 * b * sinBeta * sinBeta * sinBeta,
                              p - e2 * a * cosBeta * cosBeta * cosBeta);
        beta = std::atan((1.0 - f) * std::tan(latitude));
    }
    geodetic.latitude = latitude;
    const double sinLatitude = std::sin(latitude);
    const double nu = a / std::sqrt(1.0 - e2 * sinLatitude * sinLatitude);
    // Near the poles p / cos(latitude) loses its digits; the z form does not.
    geodetic.ellipsoidalHeight = std::abs(latitude) < 0.25 * kPi
                                     ? p / std::cos(latitude) - nu
                                     : geocentric.z / sinLatitude - nu * (1.0 - e2);
    return geodetic;
}

LocalVariances localVariances(const GnssCovariance3& c, const GeodeticCoordinate& at)
{
    const double sinLatitude = std::sin(at.latitude);
    const double cosLatitude = std::cos(at.latitude);
    const double sinLongitude = std::sin(at.longitude);
    const double cosLongitude = std::cos(at.longitude);
    const double rows[3][3] = {
        {-sinLatitude * cosLongitude, -sinLatitude * sinLongitude, cosLatitude}, // north
        {-sinLongitude, cosLongitude, 0.0},                                      // east
        {cosLatitude * cosLongitude, cosLatitude * sinLongitude, sinLatitude},   // up
    };
    const double matrix[3][3] = {{c.xx, c.xy, c.xz}, {c.xy, c.yy, c.yz}, {c.xz, c.yz, c.zz}};
    double variances[3] = {};
    for (int r = 0; r < 3; ++r) {
        for (int i = 0; i < 3; ++i) {
            for (int j = 0; j < 3; ++j) {
                variances[r] += rows[r][i] * matrix[i][j] * rows[r][j];
            }
        }
    }
    return LocalVariances{variances[0], variances[1], variances[2]};
}

double normalQuantile(double probability)
{
    // Solved on the tail that holds the probability, so that 1 - 1e-9 is not
    // rounded to a cruder number before the search starts.
    if (probability < 0.5) {
        return -normalQuantile(1.0 - probability);
    }
    const double tail = 1.0 - probability;
    // Upper tail Q(z) = erfc(z / sqrt 2) / 2 decreases, so search on -Q.
    return solveIncreasing([](double z) { return -0.5 * std::erfc(z / std::sqrt(2.0)); }, -tail,
                           0.0, 40.0);
}

double studentTQuantile(double probability, std::size_t degreesOfFreedom)
{
    if (degreesOfFreedom > 200) {
        // A&S 26.7.5.
        const double z = normalQuantile(probability);
        const double nu = static_cast<double>(degreesOfFreedom);
        const double z2 = z * z;
        const double z3 = z2 * z;
        const double z5 = z3 * z2;
        const double z7 = z5 * z2;
        const double z9 = z7 * z2;
        const double g1 = (z3 + z) / 4.0;
        const double g2 = (5.0 * z5 + 16.0 * z3 + 3.0 * z) / 96.0;
        const double g3 = (3.0 * z7 + 19.0 * z5 + 17.0 * z3 - 15.0 * z) / 384.0;
        const double g4 =
            (79.0 * z9 + 776.0 * z7 + 1482.0 * z5 - 1920.0 * z3 - 945.0 * z) / 92160.0;
        return z + g1 / nu + g2 / (nu * nu) + g3 / (nu * nu * nu) + g4 / (nu * nu * nu * nu);
    }
    // P(T <= t) = (1 + P(|T| < t)) / 2 for t >= 0.
    const double central = 2.0 * probability - 1.0;
    return solveIncreasing(
        [degreesOfFreedom](double t) { return studentCentralProbability(t, degreesOfFreedom); },
        central, 0.0, 1e7);
}

double baardaCritical(double significance)
{
    return normalQuantile(1.0 - 0.5 * significance);
}

std::optional<double> tauCritical(double significance, std::size_t redundancy)
{
    if (redundancy < 2) {
        return std::nullopt;
    }
    const double t = studentTQuantile(1.0 - 0.5 * significance, redundancy - 1);
    const double r = static_cast<double>(redundancy);
    return t * std::sqrt(r / (r - 1.0 + t * t));
}

} // namespace katana::survey::detail
