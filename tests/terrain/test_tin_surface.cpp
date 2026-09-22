#include <gtest/gtest.h>

#include <cmath>
#include <cstddef>
#include <limits>
#include <optional>
#include <vector>

#include "katana/core/task_pool.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "terrain_test_support.hpp"

using namespace katana::terrain;
using namespace katana::terrain::testing;
using katana::core::ErrorCode;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;
using katana::test::Random;

// ---- a plane must be reproduced exactly ------------------------------------------------

class PlaneSurface : public ::testing::Test {
  protected:
    void SetUp() override
    {
        Random random;
        surface_ = buildFromPoints(planePoints(plane_, kSize, 1500, random));
        ASSERT_FALSE(surface_.empty());
    }

    static constexpr double kSize = 100.0;
    const Plane plane_{0.3, -0.2, 12.5};
    TinSurface surface_;
};

TEST_F(PlaneSurface, ElevationMatchesThePlaneEverywhere)
{
    Random random(7);
    for (int i = 0; i < 2000; ++i) {
        const Point2 p(random.real(0, kSize), random.real(0, kSize));
        const auto z = surface_.elevationAt(p);
        ASSERT_TRUE(z.has_value()) << p;
        // Linear interpolation of a linear function is exact; what remains is the
        // rounding of the barycentric weights (eps * edge / height of thin
        // Delaunay triangles, times a relief of <= 50 m): orders below 1e-9.
        EXPECT_NEAR(*z, plane_.at(p.x, p.y), 1e-9) << p;
    }
}

TEST_F(PlaneSurface, SlopeAndAspectOfEveryTriangle)
{
    // |grad z| = sqrt(a^2 + b^2). Steepest descent runs along -grad = (-a, -b) =
    // (-0.3, +0.2): north-west. As an azimuth (clockwise from +y):
    // atan2(east, north) = atan2(-0.3, 0.2), wrapped into [0, 2 pi).
    const double expectedSlope = std::sqrt(0.3 * 0.3 + 0.2 * 0.2);
    const double expectedAspect = std::atan2(-0.3, 0.2) + kTwoPi;
    std::size_t checked = 0;
    for (std::size_t t = 0; t < surface_.triangleCount(); ++t) {
        const auto slopeAspect = surface_.triangleSlopeAspect(t);
        if (!slopeAspect) {
            continue;
        }
        ++checked;
        EXPECT_NEAR(slopeAspect->slope, expectedSlope, 1e-9);
        EXPECT_NEAR(slopeAspect->slopeAngle, std::atan(expectedSlope), 1e-9);
        ASSERT_TRUE(slopeAspect->aspect.has_value());
        EXPECT_NEAR(*slopeAspect->aspect, expectedAspect, 1e-8);
    }
    EXPECT_EQ(checked, surface_.triangleCount()) << "no triangle of this data is a sliver";

    const auto slope = surface_.slopeAt(Point2(40, 60));
    const auto aspect = surface_.aspectAt(Point2(40, 60));
    ASSERT_TRUE(slope.has_value());
    ASSERT_TRUE(aspect.has_value());
    EXPECT_NEAR(*slope, expectedSlope, 1e-9);
    EXPECT_NEAR(*aspect, expectedAspect, 1e-8);
    EXPECT_FALSE(surface_.slopeAt(Point2(-1, 50)).has_value());
    EXPECT_FALSE(surface_.aspectAt(Point2(-1, 50)).has_value());
}

TEST_F(PlaneSurface, PlanAndSurfaceArea)
{
    // A plane tilts every plan area element by the same factor sqrt(1 + a^2 + b^2).
    EXPECT_NEAR(surface_.planArea(), kSize * kSize, 1e-8);
    EXPECT_NEAR(surface_.surfaceArea(), kSize * kSize * std::sqrt(1.0 + 0.09 + 0.04), 1e-8);
    EXPECT_NEAR(surface_.minElevation(), plane_.at(0, kSize), 1e-12);  // a > 0, b < 0
    EXPECT_NEAR(surface_.maxElevation(), plane_.at(kSize, 0), 1e-12);
    EXPECT_EQ(surface_.bounds().min, Point2(0, 0));
    EXPECT_EQ(surface_.bounds().max, Point2(kSize, kSize));
}

TEST_F(PlaneSurface, SpotElevationsAreBatchedElevationAt)
{
    const std::vector<Point2> positions{Point2(10, 10), Point2(-5, 10), Point2(99.5, 0.5),
                                        Point2(50, 200)};
    const auto spots = surface_.elevationsAt(positions);
    ASSERT_EQ(spots.size(), 4u);
    ASSERT_TRUE(spots[0].has_value());
    EXPECT_NEAR(*spots[0], plane_.at(10, 10), 1e-9);
    EXPECT_FALSE(spots[1].has_value());
    ASSERT_TRUE(spots[2].has_value());
    EXPECT_NEAR(*spots[2], plane_.at(99.5, 0.5), 1e-9);
    EXPECT_FALSE(spots[3].has_value());
}

// ---- aspect convention --------------------------------------------------------------------

TEST(TinSurfaceAspect, CardinalDirections)
{
    Random random;
    struct Case {
        Plane plane;
        double azimuth; // of steepest descent, clockwise from north
    };
    const Case cases[] = {
        {Plane{0, -1, 0}, 0.0},          // z falls as y grows: descends to the north
        {Plane{-1, 0, 0}, kHalfPi},      // descends to the east
        {Plane{0, 1, 0}, kPi},           // descends to the south
        {Plane{1, 0, 0}, 3.0 * kHalfPi}, // descends to the west
        {Plane{-1, -1, 0}, kPi / 4.0},   // north-east
    };
    for (const Case& c : cases) {
        const TinSurface surface = buildFromPoints(planePoints(c.plane, 10.0, 20, random));
        const auto aspect = surface.aspectAt(Point2(5, 5));
        ASSERT_TRUE(aspect.has_value());
        EXPECT_TRUE(katana::math::anglesEqual(*aspect, c.azimuth, 1e-9)) << *aspect;
        EXPECT_GE(*aspect, 0.0);
        EXPECT_LT(*aspect, kTwoPi);
    }
}

TEST(TinSurfaceAspect, FlatGroundHasNoAspect)
{
    Random random;
    const TinSurface surface = buildFromPoints(planePoints(Plane{0, 0, 42.0}, 10.0, 20, random));
    const auto slopeAspect = surface.triangleSlopeAspect(0);
    ASSERT_TRUE(slopeAspect.has_value());
    EXPECT_DOUBLE_EQ(slopeAspect->slope, 0.0);
    EXPECT_DOUBLE_EQ(slopeAspect->slopeAngle, 0.0);
    EXPECT_FALSE(slopeAspect->aspect.has_value());
    ASSERT_TRUE(surface.slopeAt(Point2(5, 5)).has_value());
    EXPECT_FALSE(surface.aspectAt(Point2(5, 5)).has_value());
}

TEST(TinSurfaceAspect, SlopeAngleOfA45DegreeFace)
{
    const TinSurface surface =
        buildFromPoints({Point3(0, 0, 0), Point3(1, 0, 1), Point3(1, 1, 1), Point3(0, 1, 0)});
    const auto slopeAspect = surface.triangleSlopeAspect(0);
    ASSERT_TRUE(slopeAspect.has_value());
    EXPECT_NEAR(slopeAspect->slope, 1.0, 1e-15);
    EXPECT_NEAR(slopeAspect->slopeAngle, kPi / 4.0, 1e-15);
    // Unit square tilted by 45 degrees about the y axis: area sqrt(2).
    EXPECT_NEAR(surface.surfaceArea(), std::sqrt(2.0), 1e-15);
}

// ---- location on edges, vertices and the rim ----------------------------------------------

TEST(TinSurfaceLocate, EdgesVerticesAndRim)
{
    // Two triangles sharing the diagonal (0,0)-(10,10).
    const TinSurface surface = buildFromPoints(
        {Point3(0, 0, 0), Point3(10, 0, 10), Point3(10, 10, 20), Point3(0, 10, 10)});
    ASSERT_EQ(surface.triangleCount(), 2u);
    // z = x + y on all four corners, hence on both triangles.
    EXPECT_NEAR(*surface.elevationAt(5, 5), 10.0, 1e-12);   // on the shared edge
    EXPECT_NEAR(*surface.elevationAt(0, 0), 0.0, 1e-12);    // vertex
    EXPECT_NEAR(*surface.elevationAt(10, 10), 20.0, 1e-12); // vertex
    EXPECT_NEAR(*surface.elevationAt(10, 4), 14.0, 1e-12);  // rim
    EXPECT_NEAR(*surface.elevationAt(3, 0), 3.0, 1e-12);    // rim

    // Within kGeometric (1e-7) outside the rim still counts as on the surface ...
    const auto justOutside = surface.locate(Point2(10.0 + 5e-8, 4.0));
    ASSERT_TRUE(justOutside.has_value());
    EXPECT_FALSE(justOutside->interior);
    EXPECT_NEAR(*surface.elevationAt(10.0 + 5e-8, 4.0), 14.0, 1e-9);
    EXPECT_TRUE(surface.elevationAt(-5e-8, -5e-8).has_value()); // beyond a corner
    // ... further out does not.
    EXPECT_FALSE(surface.elevationAt(10.0 + 1e-6, 4.0).has_value());
    EXPECT_FALSE(surface.elevationAt(-1e-6, 5.0).has_value());
    EXPECT_FALSE(surface.elevationAt(5.0, 10.0 + 1e-6).has_value());

    const auto inside = surface.locate(Point2(7, 2));
    ASSERT_TRUE(inside.has_value());
    EXPECT_TRUE(inside->interior);
    EXPECT_NEAR(inside->weights[0] + inside->weights[1] + inside->weights[2], 1.0, 1e-15);

    const double nan = std::numeric_limits<double>::quiet_NaN();
    EXPECT_FALSE(surface.elevationAt(nan, 1.0).has_value());
    EXPECT_FALSE(TinSurface{}.elevationAt(1.0, 1.0).has_value());
}

TEST(TinSurfaceLocate, AgreesWithBruteForceOnARoughSurface)
{
    // The grid index must find exactly the triangle a linear scan would find.
    Random random;
    std::vector<Point3> points;
    for (int i = 0; i < 1500; ++i) {
        points.emplace_back(random.real(0, 300), random.real(0, 120), random.real(0, 50));
    }
    const TinSurface surface = buildFromPoints(points);
    for (int i = 0; i < 1500; ++i) {
        const Point2 p(random.real(-10, 310), random.real(-10, 130));
        std::optional<double> expected;
        for (std::size_t t = 0; t < surface.triangleCount() && !expected; ++t) {
            const auto plan = surface.planTriangle(t);
            if (plan.classify(p) == katana::geometry::Containment::Outside) {
                continue;
            }
            if (const auto w = plan.barycentric(p)) {
                const TinTriangle& tri = surface.triangles()[t];
                expected = (*w)[0] * surface.vertices()[tri[0]].z +
                           (*w)[1] * surface.vertices()[tri[1]].z +
                           (*w)[2] * surface.vertices()[tri[2]].z;
            }
        }
        const auto actual = surface.elevationAt(p);
        ASSERT_EQ(actual.has_value(), expected.has_value()) << p;
        if (expected) {
            EXPECT_NEAR(*actual, *expected, 1e-9) << p;
        }
    }
}

// ---- TinSurface::create -------------------------------------------------------------------

TEST(TinSurfaceCreate, DerivesAdjacencyAndStatistics)
{
    auto created = TinSurface::create(
        {Point3(0, 0, 1), Point3(2, 0, 1), Point3(2, 2, 3), Point3(0, 2, 3)},
        {TinTriangle{0, 1, 2}, TinTriangle{0, 2, 3}}, {0b100, 0b001});
    ASSERT_TRUE(created.ok()) << created.error().describe();
    const TinSurface& surface = *created;
    EXPECT_EQ(surface.neighbors()[0], (TinNeighbors{kNoTriangle, kNoTriangle, 1}));
    EXPECT_EQ(surface.neighbors()[1], (TinNeighbors{0, kNoTriangle, kNoTriangle}));
    EXPECT_TRUE(surface.isEdgeConstrained(0, 2));
    EXPECT_TRUE(surface.isEdgeConstrained(1, 0));
    EXPECT_FALSE(surface.isEdgeConstrained(0, 0));
    EXPECT_DOUBLE_EQ(surface.planArea(), 4.0);
    EXPECT_NEAR(surface.surfaceArea(), 4.0 * std::sqrt(2.0), 1e-15); // z = 1 + y: 45 degrees
    EXPECT_NEAR(*surface.elevationAt(1.0, 0.5), 1.5, 1e-15);
    EXPECT_THROW((void)surface.planTriangle(2), std::out_of_range);
}

TEST(TinSurfaceCreate, RebuildsTheBuildersSurface)
{
    Random random;
    TinInput input = pyramidInput(10.0, 5.0, 300, random);
    const TinSurface built = build(input).surface;
    auto recreated =
        TinSurface::create(built.vertices(), built.triangles(), built.constrainedEdges());
    ASSERT_TRUE(recreated.ok()) << recreated.error().describe();
    // Adjacency derived from the index triples == adjacency reported by the kernel.
    EXPECT_TRUE(*recreated == built);
    EXPECT_EQ(recreated->planArea(), built.planArea());
}

TEST(TinSurfaceCreate, RejectsInvalidInput)
{
    const std::vector<Point3> square{Point3(0, 0, 0), Point3(2, 0, 0), Point3(2, 2, 0),
                                     Point3(0, 2, 0)};
    const auto code = [&](std::vector<Point3> vertices, std::vector<TinTriangle> triangles,
                          std::vector<std::uint8_t> flags = {}) {
        auto result =
            TinSurface::create(std::move(vertices), std::move(triangles), std::move(flags));
        return result.ok() ? std::optional<ErrorCode>{} : result.error().code;
    };
    EXPECT_EQ(code(square, {TinTriangle{0, 1, 7}}), ErrorCode::InvalidArgument);
    EXPECT_EQ(code(square, {TinTriangle{0, 1, 1}}), ErrorCode::InvalidArgument);
    EXPECT_EQ(code(square, {TinTriangle{0, 2, 1}}), ErrorCode::InvalidGeometry); // clockwise
    EXPECT_EQ(code(square, {TinTriangle{0, 1, 2}}, {0, 0}), ErrorCode::InvalidArgument);
    EXPECT_EQ(code(square, {TinTriangle{0, 1, 2}, TinTriangle{0, 2, 3}}, {0b100, 0b000}),
              ErrorCode::InvalidArgument); // flags disagree across the diagonal
    // The same triangle twice: its edges run twice in the same direction.
    EXPECT_EQ(code(square, {TinTriangle{0, 1, 2}, TinTriangle{0, 1, 2}}),
              ErrorCode::InvalidGeometry);
    std::vector<Point3> nonFinite = square;
    nonFinite[1].z = std::numeric_limits<double>::infinity();
    EXPECT_EQ(code(nonFinite, {TinTriangle{0, 1, 2}}), ErrorCode::InvalidArgument);

    // No triangles is the valid empty surface.
    auto empty = TinSurface::create(square, {});
    ASSERT_TRUE(empty.ok());
    EXPECT_TRUE(empty->empty());
    EXPECT_DOUBLE_EQ(empty->planArea(), 0.0);
}

// ---- large projected coordinates -------------------------------------------------------------

// The same surface at the origin and shifted to UTM magnitudes (E 500 000,
// N 5 000 000). Coordinates are multiples of 1/8 m, so the shift is exact in
// binary64 (5 000 000 + k/8 needs 23 + 3 bits) and both inputs describe
// precisely the same geometry. Every computation in the module works on
// coordinate differences, which are then identical, so the results must agree
// to rounding level - 1e-10 relative is already generous.
TEST(TinSurfaceLargeCoordinates, ExactShiftGivesTheSameSurface)
{
    constexpr double kEast = 500000.0;
    constexpr double kNorth = 5000000.0;
    Random random;
    std::vector<Point3> local;
    std::vector<Point3> shifted;
    for (int i = 0; i < 2000; ++i) {
        const double x = random.integer(0, 8000) / 8.0;
        const double y = random.integer(0, 8000) / 8.0;
        const double z = 100.0 + 10.0 * std::sin(0.01 * x) * std::cos(0.013 * y);
        local.emplace_back(x, y, z);
        shifted.emplace_back(x + kEast, y + kNorth, z);
    }
    TinBuildOptions options;
    options.duplicatePoints = DuplicatePointPolicy::KeepFirst; // the integer grid may repeat
    TinInput localInput;
    localInput.points = local;
    TinInput shiftedInput;
    shiftedInput.points = shifted;
    const TinSurface a = build(localInput, options).surface;
    const TinSurface b = build(shiftedInput, options).surface;

    ASSERT_EQ(a.triangleCount(), b.triangleCount());
    EXPECT_EQ(a.triangles(), b.triangles()) << "exact predicates: identical topology";
    EXPECT_NEAR(a.planArea(), b.planArea(), 1e-10 * a.planArea());
    EXPECT_NEAR(a.surfaceArea(), b.surfaceArea(), 1e-10 * a.surfaceArea());
    for (int i = 0; i < 1000; ++i) {
        const double x = random.integer(0, 64000) / 64.0;
        const double y = random.integer(0, 64000) / 64.0;
        const auto za = a.elevationAt(x, y);
        const auto zb = b.elevationAt(x + kEast, y + kNorth);
        ASSERT_EQ(za.has_value(), zb.has_value());
        if (za) {
            EXPECT_NEAR(*za, *zb, 1e-10);
        }
    }
}

// Arbitrary (not exactly shiftable) coordinates on a plane. Adding 5e6 rounds
// every coordinate by up to half an ulp (4.7e-10 m); with |grad z| = 0.36 the
// elevation of a data point or query can move by up to 0.36 * sqrt(2) * 4.7e-10
// = 2.4e-10 m. Data and query rounding together stay below 1e-9 m.
TEST(TinSurfaceLargeCoordinates, PlaneAtUtmMagnitude)
{
    constexpr double kEast = 500000.0;
    constexpr double kNorth = 5000000.0;
    const Plane plane{0.3, -0.2, 12.5};
    Random random;
    std::vector<Point3> points = planePoints(plane, 100.0, 1500, random);
    for (Point3& p : points) {
        p.x += kEast;
        p.y += kNorth;
    }
    const TinSurface surface = buildFromPoints(points);
    EXPECT_NEAR(surface.planArea(), 1e4, 1e-5); // perimeter 400 m * 4.7e-10 m rounding
    EXPECT_NEAR(surface.surfaceArea(), 1e4 * std::sqrt(1.13), 1e-5);
    for (int i = 0; i < 1000; ++i) {
        const double x = random.real(0, 100);
        const double y = random.real(0, 100);
        const auto z = surface.elevationAt(x + kEast, y + kNorth);
        ASSERT_TRUE(z.has_value());
        EXPECT_NEAR(*z, plane.at(x, y), 1e-9);
    }
    const auto slopeAspect = surface.triangleSlopeAspect(0);
    ASSERT_TRUE(slopeAspect.has_value());
    // Slope from coordinates carrying 4.7e-10 m of rounding over edges of ~1 m.
    EXPECT_NEAR(slopeAspect->slope, std::sqrt(0.13), 1e-7);
}

// ---- elevationsAt runs across the cores and must not notice ----------------

TEST(TinSurfaceBatchQuery, ElevationsAtGivesExactlyWhatElevationAtGivesOneAtATime)
{
    Random random(4242);
    const Plane plane{0.25, -0.125, 40.0}; // exact in binary, so a mismatch is a real one
    const TinSurface surface = buildFromPoints(planePoints(plane, 100.0, 4000, random));
    ASSERT_FALSE(surface.empty());

    // Well past the 256-position chunk, so the work is genuinely shared out,
    // and deliberately half off the surface so both answers - a level and no
    // level at all - are carried back from the worker threads.
    std::vector<Point2> probes;
    probes.reserve(5000);
    for (int i = 0; i < 5000; ++i) {
        probes.emplace_back(random.real(-50.0, 150.0), random.real(-50.0, 150.0));
    }

    const std::vector<std::optional<double>> batch = surface.elevationsAt(probes);
    ASSERT_EQ(batch.size(), probes.size());

    std::size_t onSurface = 0;
    std::size_t offSurface = 0;
    for (std::size_t i = 0; i < probes.size(); ++i) {
        const std::optional<double> one = surface.elevationAt(probes[i]);
        ASSERT_EQ(batch[i].has_value(), one.has_value()) << "probe " << i;
        if (one.has_value()) {
            EXPECT_EQ(*batch[i], *one) << "probe " << i; // identical bits, not merely close
            ++onSurface;
        } else {
            ++offSurface;
        }
    }
    EXPECT_GT(onSurface, 1000u) << "the probes must actually land on the surface";
    EXPECT_GT(offSurface, 1000u) << "and off it";
}

TEST(TinSurfaceBatchQuery, ElevationsAtGivesTheSameAnswerOnOneThreadAsOnAllOfThem)
{
    Random random(99);
    const Plane plane{-0.5, 0.25, 8.0};
    const TinSurface surface = buildFromPoints(planePoints(plane, 64.0, 3000, random));
    ASSERT_FALSE(surface.empty());

    std::vector<Point2> probes;
    probes.reserve(3000);
    for (int i = 0; i < 3000; ++i) {
        probes.emplace_back(random.real(-8.0, 72.0), random.real(-8.0, 72.0));
    }

    const std::vector<std::optional<double>> many = surface.elevationsAt(probes);
    // A parallelRanges() issued while the pool is already busy runs its whole
    // range on the calling thread (task_pool.hpp), so this is the same call
    // made with a concurrency of one. Rule 7 says it must be the same bytes.
    std::vector<std::optional<double>> one;
    katana::core::TaskPool::shared().parallelRanges(
        0, 1, 1, [&](std::size_t, std::size_t) { one = surface.elevationsAt(probes); });
    EXPECT_EQ(one, many);
}
