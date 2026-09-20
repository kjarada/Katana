#pragma once

// Shared support for the geodesy tests: result-checking macros and the
// INDEPENDENT ORACLES that expected values are taken from. Nothing in this file
// calls PROJ or katana_geodesy for a number it then asserts against; the
// formulas are coded here from the literature so that a wrong answer from the
// library cannot validate itself.

#include <gtest/gtest.h>

#include <array>
#include <cmath>

#include "katana/geodesy/coordinate_reference_system.hpp"
#include "katana/geodesy/coordinate_transformer.hpp"
#include "katana/math/numerics.hpp"

#define ASSERT_OK(result) ASSERT_TRUE((result).ok()) << (result).error().describe()
#define EXPECT_OK(result) EXPECT_TRUE((result).ok()) << (result).error().describe()

namespace katana::geodesy::test {

// ---- comparison bounds ---------------------------------------------------------
// Numerical bounds come from the central policy (math::tolerance). The two
// helpers below are not tolerances of Katana but properties of reference DATA.

// A published value rounded to `lastDigit` (0.001 for millimetres) agrees with
// the true value to half a unit in its last place.
[[nodiscard]] constexpr double halfUnit(double lastDigit)
{
    return 0.5 * lastDigit;
}

// The longest degree on the WGS 84 ellipsoid: the degree of latitude at the
// pole, where the meridional radius of curvature is a/(1-f) = 6 399 594 m.
// Every other degree, of latitude or of longitude, is shorter.
inline constexpr double kLongestDegreeMetres = 111694.0;

// math::tolerance::kCoordinate (0.1 mm on the ground) expressed as an angle.
// Measured with the longest degree it is at most 0.1 mm on the ground
// everywhere, in latitude and in longitude. About 9.0e-10 degrees.
inline constexpr double kCoordinateAsDegrees =
    katana::math::tolerance::kCoordinate / kLongestDegreeMetres;

// Upper bound of the ground distance between two nearby geographic positions:
// the angular separation, longitude weighted by cos(latitude), measured with the
// longest degree. It never understates the distance.
[[nodiscard]] inline double groundSeparationMetres(const GeographicCoordinate& a,
                                                   const GeographicCoordinate& b)
{
    const double cosLatitude = std::cos(a.latitude * katana::math::kDegToRad);
    return kLongestDegreeMetres *
           std::hypot(a.latitude - b.latitude, (a.longitude - b.longitude) * cosLatitude);
}

// ---- WGS 84 defining constants (NIMA TR8350.2) --------------------------------
inline constexpr double kWgs84A = 6378137.0;
inline constexpr double kWgs84InverseF = 298.257223563;

// ---- Oracle 1: Transverse Mercator by the Krueger series ------------------------
// L. Krueger (1912) as revived by C. F. F. Karney, "Transverse Mercator with an
// accuracy of a few nanometers", J. Geodesy 85 (2011), eqs. (7)-(11), (35),
// series to the 6th order in the third flattening n. Valid for a latitude of
// origin of 0 (every UTM zone). Truncation error is below 5 nm within 35 degrees
// of the central meridian.
struct TransverseMercator {
    double a = kWgs84A;
    double inverseFlattening = kWgs84InverseF;
    double scaleFactor = 0.9996;     // UTM
    double centralMeridianDeg = 0.0; // zone 30: -3
    double falseEasting = 500000.0;  // UTM
    double falseNorthing = 0.0;      // UTM, northern hemisphere
};

struct GridPosition {
    double easting = 0.0;
    double northing = 0.0;
};

[[nodiscard]] inline GridPosition krugerForward(const TransverseMercator& tm, double latitudeDeg,
                                                double longitudeDeg)
{
    const double f = 1.0 / tm.inverseFlattening;
    const double n = f / (2.0 - f);
    const double n2 = n * n;
    const double n3 = n2 * n;
    const double n4 = n3 * n;
    const double n5 = n4 * n;
    const double n6 = n5 * n;

    // Rectifying radius A and the alpha coefficients, Karney (2011) eqs. (14), (35).
    const double rectifyingRadius =
        tm.a / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0 + n6 / 256.0);
    const std::array<double, 6> alpha = {
        n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0 - 127.0 * n5 / 288.0 +
            7891.0 * n6 / 37800.0,
        13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0 + 281.0 * n5 / 630.0 -
            1983433.0 * n6 / 1935360.0,
        61.0 * n3 / 240.0 - 103.0 * n4 / 140.0 + 15061.0 * n5 / 26880.0 +
            167603.0 * n6 / 181440.0,
        49561.0 * n4 / 161280.0 - 179.0 * n5 / 168.0 + 6601661.0 * n6 / 7257600.0,
        34729.0 * n5 / 80640.0 - 3418889.0 * n6 / 1995840.0,
        212378941.0 * n6 / 319334400.0,
    };

    const double e = std::sqrt(f * (2.0 - f));
    const double phi = latitudeDeg * katana::math::kDegToRad;
    const double lambda = (longitudeDeg - tm.centralMeridianDeg) * katana::math::kDegToRad;

    // Conformal latitude through tau' (eqs. 7-9), stable at all latitudes.
    const double tau = std::tan(phi);
    const double sigma = std::sinh(e * std::atanh(e * tau / std::sqrt(1.0 + tau * tau)));
    const double tauPrime =
        tau * std::sqrt(1.0 + sigma * sigma) - sigma * std::sqrt(1.0 + tau * tau);

    const double cosLambda = std::cos(lambda);
    const double xiPrime = std::atan2(tauPrime, cosLambda);
    const double etaPrime =
        std::asinh(std::sin(lambda) / std::sqrt(tauPrime * tauPrime + cosLambda * cosLambda));

    double xi = xiPrime;
    double eta = etaPrime;
    for (std::size_t j = 0; j < alpha.size(); ++j) {
        const double k = 2.0 * static_cast<double>(j + 1);
        xi += alpha[j] * std::sin(k * xiPrime) * std::cosh(k * etaPrime);
        eta += alpha[j] * std::cos(k * xiPrime) * std::sinh(k * etaPrime);
    }
    return GridPosition{tm.falseEasting + tm.scaleFactor * rectifyingRadius * eta,
                        tm.falseNorthing + tm.scaleFactor * rectifyingRadius * xi};
}

// Point scale factor and grid convergence of the same projection, obtained by
// differentiating krugerForward() numerically along the meridian:
//     k     = |d(E,N)/d(phi)| / M(phi)         M = meridional radius of curvature
//     gamma = -atan2(dE/dphi, dN/dphi)         azimuth of grid north
// (the projection is conformal, so the scale along the meridian is the scale in
// every direction). Central differences with a step of 1e-6 rad: truncation
// ~1e-12 relative, rounding ~1e-9 relative at UTM magnitudes.
struct OracleGridFactors {
    double scale = 1.0;
    double convergenceDeg = 0.0;
};

[[nodiscard]] inline OracleGridFactors krugerFactors(const TransverseMercator& tm,
                                                     double latitudeDeg, double longitudeDeg)
{
    constexpr double kStepRad = 1e-6;
    const double stepDeg = kStepRad * katana::math::kRadToDeg;
    const GridPosition north = krugerForward(tm, latitudeDeg + stepDeg, longitudeDeg);
    const GridPosition south = krugerForward(tm, latitudeDeg - stepDeg, longitudeDeg);
    const double dEasting = (north.easting - south.easting) / (2.0 * kStepRad);
    const double dNorthing = (north.northing - south.northing) / (2.0 * kStepRad);

    const double f = 1.0 / tm.inverseFlattening;
    const double e2 = f * (2.0 - f);
    const double sinPhi = std::sin(latitudeDeg * katana::math::kDegToRad);
    const double w2 = 1.0 - e2 * sinPhi * sinPhi;
    const double meridionalRadius = tm.a * (1.0 - e2) / (w2 * std::sqrt(w2));

    return OracleGridFactors{std::hypot(dEasting, dNorthing) / meridionalRadius,
                             -std::atan2(dEasting, dNorthing) * katana::math::kRadToDeg};
}

// ---- Oracle 2: geodetic -> geocentric, closed form -----------------------------
//     X = (N + h) cos(phi) cos(lambda)      N = a / sqrt(1 - e^2 sin^2(phi))
//     Y = (N + h) cos(phi) sin(lambda)
//     Z = (N (1 - e^2) + h) sin(phi)
struct Geocentric {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;
};

[[nodiscard]] inline Geocentric geodeticToGeocentric(double a, double inverseFlattening,
                                                     double latitudeDeg, double longitudeDeg,
                                                     double height)
{
    const double f = 1.0 / inverseFlattening;
    const double e2 = f * (2.0 - f);
    const double phi = latitudeDeg * katana::math::kDegToRad;
    const double lambda = longitudeDeg * katana::math::kDegToRad;
    const double primeVertical = a / std::sqrt(1.0 - e2 * std::sin(phi) * std::sin(phi));
    return Geocentric{(primeVertical + height) * std::cos(phi) * std::cos(lambda),
                      (primeVertical + height) * std::cos(phi) * std::sin(lambda),
                      (primeVertical * (1.0 - e2) + height) * std::sin(phi)};
}

// Quarter meridian (equator to pole) = rectifying radius * pi / 2.
[[nodiscard]] inline double quarterMeridian(double a, double inverseFlattening)
{
    const double f = 1.0 / inverseFlattening;
    const double n = f / (2.0 - f);
    const double n2 = n * n;
    return a / (1.0 + n) * (1.0 + n2 / 4.0 + n2 * n2 / 64.0 + n2 * n2 * n2 / 256.0) *
           katana::math::kHalfPi;
}

// Degrees from degrees, minutes, seconds (sign applied to the whole value).
[[nodiscard]] constexpr double dms(double degrees, double minutes, double seconds)
{
    return degrees + minutes / 60.0 + seconds / 3600.0;
}

// ---- helpers --------------------------------------------------------------------

[[nodiscard]] inline core::Result<CoordinateTransformer>
makeTransformer(int sourceEpsg, int targetEpsg, const TransformerOptions& options = {})
{
    auto source = CoordinateReferenceSystem::fromEpsg(sourceEpsg);
    if (!source) {
        return source.error();
    }
    auto target = CoordinateReferenceSystem::fromEpsg(targetEpsg);
    if (!target) {
        return target.error();
    }
    return CoordinateTransformer::create(*source, *target, options);
}

} // namespace katana::geodesy::test
