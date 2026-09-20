#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <vector>

#include "katana/math/numerics.hpp"
#include "katana/survey/cogo.hpp"
#include "support/property.hpp"

using namespace katana::survey;
using katana::core::ErrorCode;
using katana::math::kDegToRad;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;
using katana::test::kPropertyIterations;
using katana::test::Random;

namespace {

constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();

// Spacing of doubles at a UTM northing of 5e6 is 2^-30 ~ 9.3e-10 m. A position
// computed there cannot be better than a few of these.
constexpr double kUtmRounding = 4.0 * 9.4e-10;

// L-shaped parcel, (northing, easting). In map axes (x = E, y = N):
//   (0,0) (6,0) (6,2) (2,2) (2,5) (0,5), counter-clockwise.
// Split into [0,6]x[0,2] (area 12, centroid x=3, y=1) and [0,2]x[2,5] (area 6,
// centroid x=1, y=3.5):
//   area      = 18
//   perimeter = 6 + 2 + 4 + 3 + 2 + 5 = 22
//   centroid  x = (12*3 + 6*1) / 18 = 7/3,  y = (12*1 + 6*3.5) / 18 = 11/6
std::vector<Coordinate2> lShape()
{
    return {{0, 0}, {0, 6}, {2, 6}, {2, 2}, {5, 2}, {5, 0}};
}

std::vector<Coordinate2> translated(std::vector<Coordinate2> vertices, double dN, double dE)
{
    for (Coordinate2& v : vertices) {
        v.northing += dN;
        v.easting += dE;
    }
    return vertices;
}

// Random star-shaped (hence simple) polygon around `centre`, listed
// counter-clockwise in map axes (x = easting, y = northing).
//
// For vertices at polar angle t_i and radius r_i about a common centre the
// shoelace sum is sum_i r_i r_{i+1} sin(t_{i+1} - t_i), so the ring is
// counter-clockwise exactly when every angular gap lies in (0, pi). Independent
// uniform angles, merely sorted, do not give that: a few draws clustered in one
// half-plane leave a gap above pi whose negative term can outweigh the rest.
// One jittered angle per equal sector does: consecutive vertices are
// (2 pi / count) (1 + u_{i+1} - u_i) apart with the jitters u in [0, 1/2), which
// is within (pi / count, 3 pi / count], and 3 pi / count <= pi for count >= 3.
std::vector<Coordinate2> randomPolygon(Random& random, const Coordinate2& centre)
{
    const int count = random.integer(3, 12);
    const double sector = kTwoPi / static_cast<double>(count);
    std::vector<Coordinate2> vertices;
    for (int i = 0; i < count; ++i) {
        const double angle = sector * (static_cast<double>(i) + random.real(0.0, 0.5));
        const double radius = random.real(20.0, 300.0);
        vertices.push_back({centre.northing + radius * std::sin(angle),
                            centre.easting + radius * std::cos(angle)});
    }
    return vertices;
}

} // namespace

// ---- inverse ---------------------------------------------------------------

TEST(SurveyCogo, InverseCardinalAndDiagonalDirections)
{
    const Coordinate2 origin{1000.0, 2000.0};
    struct Case {
        double dN, dE, azimuthDegrees, distance;
    };
    // Azimuth is clockwise from north: east is 90°, south 180°, west 270°.
    const Case cases[] = {
        {100, 0, 0, 100},    {0, 100, 90, 100},     {-100, 0, 180, 100},
        {0, -50, 270, 50},   {100, 100, 45, 100 * std::sqrt(2.0)},
        {-100, 100, 135, 100 * std::sqrt(2.0)},     {-100, -100, 225, 100 * std::sqrt(2.0)},
        {100, -100, 315, 100 * std::sqrt(2.0)},
    };
    for (const Case& c : cases) {
        const auto result = inverse(origin, {origin.northing + c.dN, origin.easting + c.dE});
        ASSERT_TRUE(result.ok());
        EXPECT_NEAR(result->azimuth, c.azimuthDegrees * kDegToRad, 1e-15) << c.azimuthDegrees;
        EXPECT_NEAR(result->distance, c.distance, 1e-12) << c.azimuthDegrees;
    }
}

TEST(SurveyCogo, InverseThreeFourFive)
{
    // dN = 3, dE = 4: distance 5, azimuth = angle whose cosine is 3/5.
    const auto result = inverse({1000.0, 2000.0}, {1003.0, 2004.0});
    ASSERT_TRUE(result.ok());
    EXPECT_DOUBLE_EQ(result->distance, 5.0);
    EXPECT_NEAR(result->azimuth, std::acos(0.6), 1e-15);
    // The reverse line differs by exactly 180°.
    const auto reverse = inverse({1003.0, 2004.0}, {1000.0, 2000.0});
    ASSERT_TRUE(reverse.ok());
    EXPECT_NEAR(reverse->azimuth, std::acos(0.6) + kPi, 1e-15);
}

TEST(SurveyCogo, InverseAzimuthStaysBelowTwoPi)
{
    // A hair west of north must give an azimuth just under 2*pi, never 2*pi or < 0.
    const auto result = inverse({0.0, 0.0}, {1000.0, -1e-9});
    ASSERT_TRUE(result.ok());
    EXPECT_LT(result->azimuth, kTwoPi);
    EXPECT_GT(result->azimuth, kTwoPi - 1e-11);
}

TEST(SurveyCogo, InverseOfCoincidentPointsIsAnError)
{
    const Coordinate2 p{5000000.0, 500000.0};
    const auto same = inverse(p, p);
    ASSERT_FALSE(same.ok());
    EXPECT_EQ(same.error().code, ErrorCode::InvalidArgument);

    // 0.05 mm apart: the same ground mark under the 0.1 mm coordinate tolerance.
    EXPECT_FALSE(inverse({0.0, 0.0}, {5e-5, 0.0}).ok());
    // 1 mm apart is a legitimate, if short, line.
    EXPECT_TRUE(inverse({0.0, 0.0}, {1e-3, 0.0}).ok());

    EXPECT_FALSE(inverse({kNaN, 0.0}, {1.0, 1.0}).ok());
    EXPECT_FALSE(inverse({0.0, 0.0}, {1.0, std::numeric_limits<double>::infinity()}).ok());
}

// ---- forward ---------------------------------------------------------------

TEST(SurveyCogo, ForwardKnownGeometry)
{
    // az 30°, d 200: dN = 200 cos 30° = 100 sqrt(3), dE = 200 sin 30° = 100.
    const auto p = forward({1000.0, 2000.0}, 30.0 * kDegToRad, 200.0);
    ASSERT_TRUE(p.ok());
    EXPECT_NEAR(p->northing, 1000.0 + 100.0 * std::sqrt(3.0), 1e-12);
    EXPECT_NEAR(p->easting, 2100.0, 1e-12);

    // Zero distance stays put; an azimuth beyond a full turn is accepted.
    EXPECT_EQ(*forward({7.0, 8.0}, 1.0, 0.0), (Coordinate2{7.0, 8.0}));
    const auto wrapped = forward({0.0, 0.0}, kTwoPi + kHalfPi, 10.0);
    ASSERT_TRUE(wrapped.ok());
    EXPECT_NEAR(wrapped->northing, 0.0, 1e-14);
    EXPECT_NEAR(wrapped->easting, 10.0, 1e-14);
}

TEST(SurveyCogo, ForwardRejectsBadInput)
{
    EXPECT_EQ(forward({0, 0}, 0.0, -1.0).error().code, ErrorCode::InvalidArgument);
    EXPECT_FALSE(forward({0, 0}, kNaN, 1.0).ok());
    EXPECT_FALSE(forward({0, 0}, 0.0, kNaN).ok());
    EXPECT_FALSE(forward({kNaN, 0}, 0.0, 1.0).ok());
}

TEST(SurveyCogo, PropertyForwardOfInverseReturnsTheTarget)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Coordinate2 a{5000000.0 + random.real(-5000.0, 5000.0),
                            500000.0 + random.real(-5000.0, 5000.0)};
        const Coordinate2 b{a.northing + random.real(-2000.0, 2000.0),
                            a.easting + random.real(-2000.0, 2000.0)};
        const auto line = inverse(a, b);
        if (!line.ok()) {
            continue; // coincident draw
        }
        EXPECT_GE(line->azimuth, 0.0);
        EXPECT_LT(line->azimuth, kTwoPi);
        const auto back = forward(a, line->azimuth, line->distance);
        ASSERT_TRUE(back.ok());
        EXPECT_NEAR(back->northing, b.northing, kUtmRounding);
        EXPECT_NEAR(back->easting, b.easting, kUtmRounding);
    }
}

TEST(SurveyCogo, PropertyInverseOfForwardReturnsAzimuthAndDistance)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Coordinate2 a{random.real(-1000.0, 1000.0), random.real(-1000.0, 1000.0)};
        const double azimuth = random.real(0.0, kTwoPi);
        const double distance = random.real(1.0, 2000.0);
        const auto b = forward(a, azimuth, distance);
        ASSERT_TRUE(b.ok());
        const auto line = inverse(a, *b);
        ASSERT_TRUE(line.ok());
        EXPECT_NEAR(line->distance, distance, 1e-9);
        // Compared through sine and cosine so that 0 and 2*pi agree.
        EXPECT_NEAR(std::sin(line->azimuth - azimuth), 0.0, 1e-10);
        EXPECT_NEAR(std::cos(line->azimuth - azimuth), 1.0, 1e-10);
    }
}

// ---- polygons --------------------------------------------------------------

TEST(SurveyCogo, TriangleAreaAndOrientation)
{
    // Map axes: (0,0) -> (E4,N0) -> (E0,N3): counter-clockwise, legs 4 and 3.
    const std::vector<Coordinate2> ccw = {{0, 0}, {0, 4}, {3, 0}};
    EXPECT_DOUBLE_EQ(*polygonSignedArea(ccw), 6.0);
    EXPECT_DOUBLE_EQ(*polygonArea(ccw), 6.0);
    EXPECT_DOUBLE_EQ(*polygonPerimeter(ccw), 12.0); // 3 + 4 + 5

    const std::vector<Coordinate2> cw = {{3, 0}, {0, 4}, {0, 0}};
    EXPECT_DOUBLE_EQ(*polygonSignedArea(cw), -6.0);
    EXPECT_DOUBLE_EQ(*polygonArea(cw), 6.0);

    // Centroid of a triangle is the mean of its vertices: N = 1, E = 4/3.
    const auto centroid = polygonCentroid(ccw);
    ASSERT_TRUE(centroid.ok());
    EXPECT_NEAR(centroid->northing, 1.0, 1e-15);
    EXPECT_NEAR(centroid->easting, 4.0 / 3.0, 1e-15);
}

TEST(SurveyCogo, LShapedParcelHandComputed)
{
    const auto parcel = lShape();
    EXPECT_DOUBLE_EQ(*polygonArea(parcel), 18.0);
    EXPECT_DOUBLE_EQ(*polygonSignedArea(parcel), 18.0);
    EXPECT_DOUBLE_EQ(*polygonPerimeter(parcel), 22.0);
    const auto centroid = polygonCentroid(parcel);
    ASSERT_TRUE(centroid.ok());
    EXPECT_NEAR(centroid->easting, 7.0 / 3.0, 1e-14);
    EXPECT_NEAR(centroid->northing, 11.0 / 6.0, 1e-14);

    // An explicit closing vertex adds a zero-length edge and changes nothing.
    auto closed = parcel;
    closed.push_back(parcel.front());
    EXPECT_DOUBLE_EQ(*polygonArea(closed), 18.0);
    EXPECT_DOUBLE_EQ(*polygonPerimeter(closed), 22.0);
    EXPECT_NEAR(polygonCentroid(closed)->easting, 7.0 / 3.0, 1e-14);
}

TEST(SurveyCogo, RegressionParcelAtUtmMagnitudeKeepsFullPrecision)
{
    // Vertices in units of 1/1024 m so that every coordinate, and every
    // translation by the UTM offset, is an exact double. The reference area is
    // computed in exact 64-bit integer arithmetic: no floating point involved.
    constexpr std::int64_t kUnit = 1024;
    struct Units {
        std::int64_t northing, easting;
    };
    const std::vector<Units> local = {
        {0, 0}, {13 * kUnit + 517, 142 * kUnit + 333}, {97 * kUnit + 259, 171 * kUnit + 801},
        {121 * kUnit + 77, 64 * kUnit + 1001},         {60 * kUnit + 5, -22 * kUnit - 611},
    };
    std::int64_t twiceAreaUnits = 0; // in (1/1024 m)^2, x = easting, y = northing
    for (std::size_t i = 0; i < local.size(); ++i) {
        const Units& a = local[i];
        const Units& b = local[(i + 1) % local.size()];
        twiceAreaUnits += a.easting * b.northing - b.easting * a.northing;
    }
    const double exactArea =
        std::abs(static_cast<double>(twiceAreaUnits)) / (2.0 * static_cast<double>(kUnit * kUnit));
    ASSERT_GT(exactArea, 10000.0); // a parcel of ~1.3 ha, sanity of the fixture

    const std::int64_t originNorthing = 5000000 * kUnit + 777; // N ~ 5e6
    const std::int64_t originEasting = 500000 * kUnit + 333;   // E ~ 5e5
    std::vector<Coordinate2> utm;
    for (const Units& v : local) {
        utm.push_back({static_cast<double>(originNorthing + v.northing) / kUnit,
                       static_cast<double>(originEasting + v.easting) / kUnit});
    }

    const auto area = polygonArea(utm);
    ASSERT_TRUE(area.ok());
    // Coordinate differences are exact and their products fit in 53 bits, so the
    // local-origin evaluation is exact here.
    EXPECT_NEAR(*area, exactArea, 1e-9);

    // The textbook shoelace on the raw coordinates multiplies numbers of ~5e5 and
    // ~5e6: each product (~2.5e12) rounds at ~5e-4 m^2. This is the failure the
    // local origin exists to prevent; if it ever "passes", the fixture is too easy.
    double naiveTwice = 0.0;
    for (std::size_t i = 0; i < utm.size(); ++i) {
        const Coordinate2& a = utm[i];
        const Coordinate2& b = utm[(i + 1) % utm.size()];
        naiveTwice += a.easting * b.northing - b.easting * a.northing;
    }
    const double naiveError = std::abs(0.5 * std::abs(naiveTwice) - exactArea);
    EXPECT_GT(naiveError, 1e-6);
    EXPECT_LT(std::abs(*area - exactArea), naiveError);

    // Centroid: the local and the UTM parcel differ by the exact offset.
    std::vector<Coordinate2> small;
    for (const Units& v : local) {
        small.push_back({static_cast<double>(v.northing) / kUnit,
                         static_cast<double>(v.easting) / kUnit});
    }
    const auto smallCentroid = polygonCentroid(small);
    const auto utmCentroid = polygonCentroid(utm);
    ASSERT_TRUE(smallCentroid.ok());
    ASSERT_TRUE(utmCentroid.ok());
    EXPECT_NEAR(utmCentroid->northing - static_cast<double>(originNorthing) / kUnit,
                smallCentroid->northing, kUtmRounding);
    EXPECT_NEAR(utmCentroid->easting - static_cast<double>(originEasting) / kUnit,
                smallCentroid->easting, kUtmRounding);
    EXPECT_NEAR(*polygonPerimeter(utm), *polygonPerimeter(small), 1e-9);
}

TEST(SurveyCogo, PolygonRejectsBadInput)
{
    const std::vector<Coordinate2> two = {{0, 0}, {1, 1}};
    EXPECT_EQ(polygonArea(two).error().code, ErrorCode::InvalidArgument);
    EXPECT_FALSE(polygonPerimeter(two).ok());
    EXPECT_FALSE(polygonCentroid(two).ok());
    EXPECT_FALSE(polygonArea(std::vector<Coordinate2>{}).ok());

    const std::vector<Coordinate2> bad = {{0, 0}, {1, kNaN}, {2, 0}};
    EXPECT_FALSE(polygonArea(bad).ok());
    EXPECT_FALSE(polygonSignedArea(bad).ok());
}

TEST(SurveyCogo, CollinearPolygonHasZeroAreaAndNoCentroid)
{
    const std::vector<Coordinate2> line = {{0, 0}, {10, 10}, {20, 20}, {5, 5}};
    EXPECT_DOUBLE_EQ(*polygonArea(line), 0.0);
    const auto centroid = polygonCentroid(line);
    ASSERT_FALSE(centroid.ok());
    EXPECT_EQ(centroid.error().code, ErrorCode::InvalidArgument);
}

TEST(SurveyCogo, PropertyAreaInvariantUnderRotationOfTheVertexList)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        auto polygon = randomPolygon(random, {5000000.0, 500000.0});
        const double area = *polygonSignedArea(polygon);
        const double perimeter = *polygonPerimeter(polygon);
        const Coordinate2 centroid = *polygonCentroid(polygon);
        EXPECT_GT(area, 0.0); // generated counter-clockwise

        std::rotate(polygon.begin(), polygon.begin() + random.integer(1, 2), polygon.end());
        // Only the local origin changes, i.e. the rounding of ~1e5 m^2 products.
        EXPECT_NEAR(*polygonSignedArea(polygon), area, 1e-9 * area);
        EXPECT_NEAR(*polygonPerimeter(polygon), perimeter, 1e-9);
        EXPECT_NEAR(polygonCentroid(polygon)->northing, centroid.northing, kUtmRounding);
        EXPECT_NEAR(polygonCentroid(polygon)->easting, centroid.easting, kUtmRounding);
    }
}

TEST(SurveyCogo, PropertyReversalFlipsTheSignOnly)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        auto polygon = randomPolygon(random, {1000.0, 1000.0});
        const double signedArea = *polygonSignedArea(polygon);
        const double area = *polygonArea(polygon);
        const Coordinate2 centroid = *polygonCentroid(polygon);

        std::reverse(polygon.begin(), polygon.end());
        EXPECT_NEAR(*polygonSignedArea(polygon), -signedArea, 1e-9 * area);
        EXPECT_NEAR(*polygonArea(polygon), area, 1e-9 * area);
        EXPECT_NEAR(polygonCentroid(polygon)->northing, centroid.northing, 1e-9);
        EXPECT_NEAR(polygonCentroid(polygon)->easting, centroid.easting, 1e-9);
    }
}

TEST(SurveyCogo, PropertyAreaInvariantUnderTranslation)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const auto polygon = randomPolygon(random, {0.0, 0.0});
        const double area = *polygonArea(polygon);
        const double perimeter = *polygonPerimeter(polygon);

        // Integer metre offsets of UTM size. Adding them rounds each vertex by up
        // to half an ulp at 5e6 (4.7e-10 m); shifting every vertex of a polygon by
        // at most e changes its area by at most perimeter * e.
        const double dN = 5000000.0 + random.integer(-100000, 100000);
        const double dE = 500000.0 + random.integer(-100000, 100000);
        const auto moved = translated(polygon, dN, dE);
        EXPECT_NEAR(*polygonArea(moved), area, perimeter * 1e-9);
        EXPECT_NEAR(*polygonPerimeter(moved), perimeter, polygon.size() * 2e-9);
    }
}

// ---- slope reductions ------------------------------------------------------

TEST(SurveyCogo, SlopeReductions)
{
    // 100 m at zenith 60°: horizontal 100 sin 60° = 50 sqrt(3), rise 100 cos 60° = 50.
    const double zenith = 60.0 * kDegToRad;
    EXPECT_NEAR(*horizontalDistance(100.0, zenith), 50.0 * std::sqrt(3.0), 1e-12);
    // + instrument 1.5 - target 1.2
    EXPECT_NEAR(*trigonometricHeightDifference(100.0, zenith, 1.5, 1.2), 50.3, 1e-12);
    // A level sight has no rise; looking down (zenith > 90°) is negative.
    EXPECT_NEAR(*trigonometricHeightDifference(100.0, kHalfPi, 0.0, 0.0), 0.0, 1e-13);
    EXPECT_NEAR(*trigonometricHeightDifference(100.0, 120.0 * kDegToRad, 0.0, 0.0), -50.0, 1e-12);

    EXPECT_FALSE(horizontalDistance(-1.0, zenith).ok());
    EXPECT_FALSE(horizontalDistance(100.0, -0.1).ok());
    EXPECT_FALSE(horizontalDistance(100.0, kPi + 0.1).ok());
    EXPECT_FALSE(trigonometricHeightDifference(100.0, zenith, kNaN, 0.0).ok());
}
