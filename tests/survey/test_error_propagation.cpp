#include <gtest/gtest.h>

#include <cmath>
#include <limits>

#include "katana/math/numerics.hpp"
#include "katana/survey/cogo.hpp"
#include "katana/survey/error_propagation.hpp"
#include "support/property.hpp"

using namespace katana::survey;
using katana::core::ErrorCode;
using katana::math::kDegToRad;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;
using katana::test::Random;

namespace {

constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();

} // namespace

// ---- error ellipse ---------------------------------------------------------

TEST(SurveyErrorEllipse, AxisAlignedCovariance)
{
    // Variances 4 (north) and 1 (east): semi-axes 2 and 1, major axis due north.
    const auto north = errorEllipse({4.0, 1.0, 0.0});
    ASSERT_TRUE(north.ok());
    EXPECT_DOUBLE_EQ(north->semiMajor, 2.0);
    EXPECT_DOUBLE_EQ(north->semiMinor, 1.0);
    EXPECT_DOUBLE_EQ(north->orientation, 0.0);

    const auto east = errorEllipse({1.0, 4.0, 0.0});
    ASSERT_TRUE(east.ok());
    EXPECT_DOUBLE_EQ(east->semiMajor, 2.0);
    EXPECT_DOUBLE_EQ(east->semiMinor, 1.0);
    EXPECT_NEAR(east->orientation, kHalfPi, 1e-15);
}

TEST(SurveyErrorEllipse, CorrelatedCovariance)
{
    // [[2, 1], [1, 2]]: eigenvalues 3 and 1; positive correlation of N and E
    // stretches the ellipse along north-east (azimuth 45°).
    const auto positive = errorEllipse({2.0, 2.0, 1.0});
    ASSERT_TRUE(positive.ok());
    EXPECT_NEAR(positive->semiMajor, std::sqrt(3.0), 1e-15);
    EXPECT_NEAR(positive->semiMinor, 1.0, 1e-15);
    EXPECT_NEAR(positive->orientation, 45.0 * kDegToRad, 1e-15);

    // Negative correlation: north-west / south-east, reported in [0, pi) as 135°.
    const auto negative = errorEllipse({2.0, 2.0, -1.0});
    ASSERT_TRUE(negative.ok());
    EXPECT_NEAR(negative->semiMajor, std::sqrt(3.0), 1e-15);
    EXPECT_NEAR(negative->orientation, 135.0 * kDegToRad, 1e-15);
}

TEST(SurveyErrorEllipse, DegenerateCases)
{
    // Circle: the orientation is arbitrary and reported as 0.
    const auto circle = errorEllipse({9.0, 9.0, 0.0});
    ASSERT_TRUE(circle.ok());
    EXPECT_DOUBLE_EQ(circle->semiMajor, 3.0);
    EXPECT_DOUBLE_EQ(circle->semiMinor, 3.0);
    EXPECT_DOUBLE_EQ(circle->orientation, 0.0);

    // A fixed point has no ellipse at all.
    const auto point = errorEllipse({0.0, 0.0, 0.0});
    ASSERT_TRUE(point.ok());
    EXPECT_DOUBLE_EQ(point->semiMajor, 0.0);
    EXPECT_DOUBLE_EQ(point->semiMinor, 0.0);

    // Perfect correlation: a line, the minor axis is exactly zero, never NaN.
    const auto line = errorEllipse({1.0, 1.0, 1.0});
    ASSERT_TRUE(line.ok());
    EXPECT_NEAR(line->semiMajor, std::sqrt(2.0), 1e-15);
    EXPECT_DOUBLE_EQ(line->semiMinor, 0.0);
}

TEST(SurveyErrorEllipse, RejectsWhatIsNotACovariance)
{
    EXPECT_EQ(errorEllipse({-1.0, 1.0, 0.0}).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(errorEllipse({1.0, -1.0, 0.0}).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(errorEllipse({1.0, 1.0, 1.5}).error().code, ErrorCode::InvalidArgument); // |rho| > 1
    EXPECT_EQ(errorEllipse({kNaN, 1.0, 0.0}).error().code, ErrorCode::InvalidArgument);
}

TEST(SurveyErrorEllipse, PropertyAxesAreTheExtremesOfTheDirectionalVariance)
{
    Random random;
    for (int i = 0; i < 300; ++i) {
        const double sn = random.real(0.001, 0.05);
        const double se = random.real(0.001, 0.05);
        const double rho = random.real(-0.95, 0.95);
        const Covariance2 c{sn * sn, se * se, rho * sn * se};
        const auto ellipse = errorEllipse(c);
        ASSERT_TRUE(ellipse.ok());
        EXPECT_GE(ellipse->semiMajor, ellipse->semiMinor);
        EXPECT_GE(ellipse->orientation, 0.0);
        EXPECT_LT(ellipse->orientation, kPi);

        // Variance of the position along azimuth t: u^T C u with u = (cos t, sin t).
        const auto along = [&](double t) {
            const double n = std::cos(t), e = std::sin(t);
            return c.northing * n * n + 2.0 * c.northingEasting * n * e + c.easting * e * e;
        };
        const double scale = ellipse->semiMajor * ellipse->semiMajor;
        EXPECT_NEAR(along(ellipse->orientation), scale, 1e-12 * scale);
        EXPECT_NEAR(along(ellipse->orientation + kHalfPi),
                    ellipse->semiMinor * ellipse->semiMinor, 1e-12 * scale);
        // Rotation invariants: trace and determinant.
        const double minor2 = ellipse->semiMinor * ellipse->semiMinor;
        EXPECT_NEAR(scale + minor2, c.northing + c.easting, 1e-12 * scale);
        EXPECT_NEAR(scale * minor2, c.northing * c.easting - c.northingEasting * c.northingEasting,
                    1e-10 * scale * scale);
        // No direction is worse than the major axis.
        for (int k = 0; k < 16; ++k) {
            EXPECT_LE(along(kPi * k / 16.0), scale * (1.0 + 1e-12));
        }
    }
}

TEST(SurveyErrorEllipse, ConfidenceScale)
{
    // chi-square(2) quantile in closed form; 2.4477 is the familiar 95 % factor.
    EXPECT_NEAR(*ellipseConfidenceScale(0.95), std::sqrt(-2.0 * std::log(0.05)), 1e-14);
    EXPECT_NEAR(*ellipseConfidenceScale(0.95), 2.4477, 5e-5);
    // The standard ellipse itself holds 1 - exp(-1/2) = 39.35 % of the probability.
    EXPECT_NEAR(*ellipseConfidenceScale(1.0 - std::exp(-0.5)), 1.0, 1e-14);
    EXPECT_FALSE(ellipseConfidenceScale(0.0).ok());
    EXPECT_FALSE(ellipseConfidenceScale(1.0).ok());
    EXPECT_FALSE(ellipseConfidenceScale(kNaN).ok());
}

// ---- forward ---------------------------------------------------------------

TEST(SurveyErrorPropagation, ForwardAlongTheAxes)
{
    // 100 m due north, sigma_d = 5 mm, sigma_az = 1e-4 rad (20.6"):
    //   along the line  (north): sigma_d            = 0.005
    //   across the line (east) : d * sigma_az       = 0.010
    const auto north = propagateForward({}, 0.0, 100.0, 1e-4, 0.005);
    ASSERT_TRUE(north.ok());
    EXPECT_NEAR(north->northing, 0.005 * 0.005, 1e-18);
    EXPECT_NEAR(north->easting, 0.010 * 0.010, 1e-18);
    EXPECT_NEAR(north->northingEasting, 0.0, 1e-18);

    // Due east the roles swap.
    const auto east = propagateForward({}, kHalfPi, 100.0, 1e-4, 0.005);
    ASSERT_TRUE(east.ok());
    EXPECT_NEAR(east->northing, 0.010 * 0.010, 1e-18);
    EXPECT_NEAR(east->easting, 0.005 * 0.005, 1e-18);
    EXPECT_NEAR(east->northingEasting, 0.0, 1e-18);
}

TEST(SurveyErrorPropagation, ForwardAtFortyFiveDegrees)
{
    // cross^2 = 1e-4, along^2 = 2.5e-5, sin^2 = cos^2 = 1/2:
    //   var N = var E = (1e-4 + 2.5e-5)/2 = 6.25e-5
    //   cov   = sin cos (along^2 - cross^2) = (2.5e-5 - 1e-4)/2 = -3.75e-5
    const auto result = propagateForward({}, 45.0 * kDegToRad, 100.0, 1e-4, 0.005);
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(result->northing, 6.25e-5, 1e-18);
    EXPECT_NEAR(result->easting, 6.25e-5, 1e-18);
    EXPECT_NEAR(result->northingEasting, -3.75e-5, 1e-18);

    // Its ellipse: 10 mm across the line (azimuth 135°), 5 mm along it.
    const auto ellipse = errorEllipse(*result);
    ASSERT_TRUE(ellipse.ok());
    EXPECT_NEAR(ellipse->semiMajor, 0.010, 1e-14);
    EXPECT_NEAR(ellipse->semiMinor, 0.005, 1e-14);
    EXPECT_NEAR(ellipse->orientation, 135.0 * kDegToRad, 1e-12);
}

TEST(SurveyErrorPropagation, ForwardAddsTheStartCovariance)
{
    const Covariance2 start{4e-6, 9e-6, 1e-6};
    const auto free = propagateForward({}, 1.0, 250.0, 2e-5, 0.003);
    const auto tied = propagateForward(start, 1.0, 250.0, 2e-5, 0.003);
    ASSERT_TRUE(free.ok());
    ASSERT_TRUE(tied.ok());
    EXPECT_NEAR(tied->northing, free->northing + 4e-6, 1e-20);
    EXPECT_NEAR(tied->easting, free->easting + 9e-6, 1e-20);
    EXPECT_NEAR(tied->northingEasting, free->northingEasting + 1e-6, 1e-20);

    // Errorless observations move the start covariance unchanged.
    EXPECT_EQ(*propagateForward(start, 1.0, 250.0, 0.0, 0.0), start);
}

TEST(SurveyErrorPropagation, PropertyForwardMatchesNumericalJacobian)
{
    // J Sigma J^T with J obtained by central differences of forward() itself: an
    // independent check of the analytic partials.
    Random random;
    for (int i = 0; i < 100; ++i) {
        const double azimuth = random.real(0.0, kTwoPi);
        const double distance = random.real(10.0, 1000.0);
        const double sigmaAzimuth = random.real(1e-6, 1e-4);
        const double sigmaDistance = random.real(0.001, 0.01);

        const double hAz = 1e-6, hD = 1e-3;
        const auto at = [&](double az, double d) { return *forward({0.0, 0.0}, az, d); };
        const Coordinate2 azPlus = at(azimuth + hAz, distance), azMinus = at(azimuth - hAz, distance);
        const Coordinate2 dPlus = at(azimuth, distance + hD), dMinus = at(azimuth, distance - hD);
        Matrix jacobian(2, 2);
        jacobian(0, 0) = (azPlus.northing - azMinus.northing) / (2.0 * hAz);
        jacobian(1, 0) = (azPlus.easting - azMinus.easting) / (2.0 * hAz);
        jacobian(0, 1) = (dPlus.northing - dMinus.northing) / (2.0 * hD);
        jacobian(1, 1) = (dPlus.easting - dMinus.easting) / (2.0 * hD);
        Matrix sigma(2, 2);
        sigma(0, 0) = sigmaAzimuth * sigmaAzimuth;
        sigma(1, 1) = sigmaDistance * sigmaDistance;

        const auto general = propagateCovariance(jacobian, sigma);
        const auto special = propagateForward({}, azimuth, distance, sigmaAzimuth, sigmaDistance);
        ASSERT_TRUE(general.ok());
        ASSERT_TRUE(special.ok());
        const double scale = special->northing + special->easting;
        EXPECT_NEAR(special->northing, (*general)(0, 0), 1e-7 * scale);
        EXPECT_NEAR(special->easting, (*general)(1, 1), 1e-7 * scale);
        EXPECT_NEAR(special->northingEasting, (*general)(0, 1), 1e-7 * scale);
    }
}

TEST(SurveyErrorPropagation, ForwardRejectsBadInput)
{
    EXPECT_FALSE(propagateForward({}, 0.0, -1.0, 1e-5, 0.01).ok());
    EXPECT_FALSE(propagateForward({}, 0.0, 100.0, -1e-5, 0.01).ok());
    EXPECT_FALSE(propagateForward({}, 0.0, 100.0, 1e-5, kNaN).ok());
    EXPECT_FALSE(propagateForward({}, kNaN, 100.0, 1e-5, 0.01).ok());
    EXPECT_FALSE(propagateForward({-1.0, 1.0, 0.0}, 0.0, 100.0, 1e-5, 0.01).ok());
}

// ---- inverse ---------------------------------------------------------------

TEST(SurveyErrorPropagation, InverseFromIsotropicEndPoint)
{
    // Fixed start, end with sigma = 10 mm in every direction, 200 m away:
    //   sigma_distance = 0.010,  sigma_azimuth = 0.010 / 200 = 5e-5 rad, uncorrelated.
    const Covariance2 isotropic{1e-4, 1e-4, 0.0};
    for (const double azimuth : {0.0, 0.7, 2.0, 4.5}) {
        const Coordinate2 to = *forward({1000.0, 1000.0}, azimuth, 200.0);
        const auto result = propagateInverse({1000.0, 1000.0}, to, {}, isotropic);
        ASSERT_TRUE(result.ok());
        EXPECT_NEAR(result->sigmaDistance, 0.010, 1e-12) << azimuth;
        EXPECT_NEAR(result->sigmaAzimuth, 5e-5, 1e-14) << azimuth;
        EXPECT_NEAR(result->covariance, 0.0, 1e-16) << azimuth;
    }
}

TEST(SurveyErrorPropagation, InverseSeparatesAlongAndAcross)
{
    // Line due north. The end is uncertain by 3 mm north (along) and 8 mm east
    // (across); both ends contribute equally, so variances double.
    const Covariance2 each{9e-6, 64e-6, 0.0};
    const auto result = propagateInverse({0.0, 0.0}, {400.0, 0.0}, each, each);
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(result->sigmaDistance, 0.003 * std::sqrt(2.0), 1e-14);
    EXPECT_NEAR(result->sigmaAzimuth, 0.008 * std::sqrt(2.0) / 400.0, 1e-16);

    EXPECT_FALSE(propagateInverse({1.0, 1.0}, {1.0, 1.0}, each, each).ok()); // coincident
    EXPECT_FALSE(propagateInverse({0.0, 0.0}, {1.0, 1.0}, {-1.0, 0.0, 0.0}, each).ok());
}

TEST(SurveyErrorPropagation, ForwardThenInverseRecoversTheObservationSigmas)
{
    // Propagate sigma_az, sigma_d out to a point from an errorless start, then
    // back: the round trip must return exactly what went in.
    const double azimuth = 1.234, distance = 321.0, sigmaAzimuth = 3e-5, sigmaDistance = 0.004;
    const Coordinate2 start{500.0, 700.0};
    const Coordinate2 end = *forward(start, azimuth, distance);
    const auto covariance = propagateForward({}, azimuth, distance, sigmaAzimuth, sigmaDistance);
    ASSERT_TRUE(covariance.ok());
    const auto back = propagateInverse(start, end, {}, *covariance);
    ASSERT_TRUE(back.ok());
    EXPECT_NEAR(back->sigmaAzimuth, sigmaAzimuth, 1e-15);
    EXPECT_NEAR(back->sigmaDistance, sigmaDistance, 1e-13);
    EXPECT_NEAR(back->covariance, 0.0, 1e-18);
}

// ---- general law -----------------------------------------------------------

TEST(SurveyErrorPropagation, GeneralLawHandExample)
{
    // J = [[1,2],[3,4]], S = [[2,1],[1,3]]
    //   J S     = [[4, 7], [10, 15]]
    //   J S J^T = [[4+14, 12+28], [10+30, 30+60]] = [[18, 40], [40, 90]]
    const auto result = propagateCovariance(*Matrix::fromRows({{1, 2}, {3, 4}}),
                                            *Matrix::fromRows({{2, 1}, {1, 3}}));
    ASSERT_TRUE(result.ok());
    EXPECT_EQ(*result, *Matrix::fromRows({{18, 40}, {40, 90}}));

    // A 1 x k Jacobian gives the variance of one derived quantity: the sum of two
    // independent measurements has variance s1^2 + s2^2.
    const auto sum = propagateCovariance(*Matrix::fromRows({{1, 1}}),
                                         *Matrix::fromRows({{0.04, 0}, {0, 0.09}}));
    ASSERT_TRUE(sum.ok());
    EXPECT_EQ(sum->rows, 1u);
    EXPECT_NEAR((*sum)(0, 0), 0.13, 1e-16);
}

TEST(SurveyErrorPropagation, GeneralLawRejectsBadInput)
{
    const Matrix j = *Matrix::fromRows({{1, 2}, {3, 4}});
    EXPECT_EQ(propagateCovariance(j, *Matrix::fromRows({{1, 0, 0}, {0, 1, 0}, {0, 0, 1}}))
                  .error()
                  .code,
              ErrorCode::InvalidArgument); // k mismatch
    EXPECT_FALSE(propagateCovariance(j, *Matrix::fromRows({{2, 1}, {0, 3}})).ok()); // asymmetric
    EXPECT_FALSE(propagateCovariance(j, *Matrix::fromRows({{2, kNaN}, {kNaN, 3}})).ok());
    EXPECT_FALSE(propagateCovariance(j, *Matrix::fromRows({{1, 2}})).ok()); // not square
}
