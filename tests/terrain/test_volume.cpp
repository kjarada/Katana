#include <gtest/gtest.h>

#include <cmath>
#include <cstddef>
#include <limits>
#include <vector>

#include "katana/terrain/volume.hpp"
#include "terrain_test_support.hpp"

using namespace katana::terrain;
using namespace katana::terrain::testing;
using katana::core::ErrorCode;
using katana::test::Random;

// ---- volumeToDatum on shapes whose integral is known in closed form ---------------------

TEST(VolumeToDatum, TiltedPlaneSplitsSymmetricallyAboutItsMidHeight)
{
    // z = x over [0, 100]^2 against the datum 50. The integrand x - 50 is odd
    // about x = 50, so
    //   above = below = integral over x in (50, 100] of (x - 50) dx dy
    //                 = 100 * 50^2/2 = 125000 m^3,
    // and each half of the square contributes 5000 m^2 of plan area.
    Random random;
    const TinSurface surface =
        buildFromPoints(planePoints(Plane{1.0, 0.0, 0.0}, 100.0, 800, random));
    ASSERT_FALSE(surface.empty());

    const auto result = volumeToDatum(surface, 50.0);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const DatumVolume& volume = result.value();
    // ~1600 triangles, compensated sums, magnitudes <= 1.25e5: the relative
    // rounding of 1e-16 per term leaves absolute errors around 1e-10.
    EXPECT_NEAR(volume.above, 125000.0, 1e-6);
    EXPECT_NEAR(volume.below, 125000.0, 1e-6);
    EXPECT_NEAR(volume.net, 0.0, 1e-6);
    EXPECT_NEAR(volume.planAreaAbove, 5000.0, 1e-8);
    EXPECT_NEAR(volume.planAreaBelow, 5000.0, 1e-8);
    EXPECT_NEAR(volume.planAreaAbove + volume.planAreaBelow, surface.planArea(), 1e-8);
}

TEST(VolumeToDatum, PyramidMatchesOneThirdBaseTimesHeight)
{
    // V = (1/3) * base area * height = (1/3) * 100^2 * 30 = 100000 m^3, and the
    // whole base sits on the datum, so nothing is below it.
    Random random;
    const TinSurface surface = build(pyramidInput(100.0, 30.0, 400, random)).surface;
    ASSERT_FALSE(surface.empty());

    const auto result = volumeToDatum(surface, 0.0);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_NEAR(result.value().above, 100000.0, 1e-6);
    EXPECT_EQ(result.value().below, 0.0);
    EXPECT_NEAR(result.value().net, 100000.0, 1e-6);
    EXPECT_NEAR(result.value().planAreaAbove, 10000.0, 1e-8);
    EXPECT_EQ(result.value().planAreaBelow, 0.0);
}

TEST(VolumeToDatum, DatumAtTheMeanHeightOfAPyramidBalancesCutAndFill)
{
    // Same pyramid. Its mean height is V/A = 100000/10000 = 10 m, so the datum 10
    // gives net == 0 by construction. Above it stands the similar sub-pyramid of
    // height 30 - 10 = 20 whose base has side 100 (1 - 10/30) = 200/3, so
    //   above = below = (1/3) (200/3)^2 * 20 = 800000/27 m^3,
    //   planAreaAbove = (200/3)^2 = 40000/9 m^2, planAreaBelow = 10000 - 40000/9.
    Random random;
    const TinSurface surface = build(pyramidInput(100.0, 30.0, 400, random)).surface;
    ASSERT_FALSE(surface.empty());

    const auto result = volumeToDatum(surface, 10.0);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const DatumVolume& volume = result.value();
    EXPECT_NEAR(volume.above, 800000.0 / 27.0, 1e-6);
    EXPECT_NEAR(volume.below, 800000.0 / 27.0, 1e-6);
    EXPECT_NEAR(volume.net, 0.0, 1e-6);
    EXPECT_NEAR(volume.planAreaAbove, 40000.0 / 9.0, 1e-8);
    EXPECT_NEAR(volume.planAreaBelow, 10000.0 - 40000.0 / 9.0, 1e-8);
}

TEST(VolumeToDatum, ShiftingTheDatumShiftsTheNetVolumeByAreaTimesShift)
{
    // net(d) = integral of (z - d) = net(0) - d * planArea for every d, exactly,
    // because the plan area does not depend on the datum. A prism identity, so it
    // holds whatever the ground looks like.
    Random random;
    TinInput input;
    for (int i = 0; i < 700; ++i) {
        input.points.emplace_back(random.real(0, 300), random.real(0, 300), random.real(40, 90));
    }
    const TinSurface surface = build(input).surface;
    ASSERT_FALSE(surface.empty());

    const auto base = volumeToDatum(surface, 0.0);
    ASSERT_TRUE(base.ok()) << base.error().describe();
    for (const double datum : {-25.0, 37.5, 65.0, 120.0}) {
        const auto shifted = volumeToDatum(surface, datum);
        ASSERT_TRUE(shifted.ok()) << shifted.error().describe();
        const double expected = base.value().net - datum * surface.planArea();
        // Magnitudes up to 1e7 with ~1400 compensated terms.
        EXPECT_NEAR(shifted.value().net, expected, 1e-6) << "datum " << datum;
        EXPECT_GE(shifted.value().above, 0.0);
        EXPECT_GE(shifted.value().below, 0.0);
        EXPECT_NEAR(shifted.value().above - shifted.value().below, shifted.value().net, 1e-9);
        EXPECT_NEAR(shifted.value().planAreaAbove + shifted.value().planAreaBelow,
                    surface.planArea(), 1e-8);
    }
}

TEST(VolumeToDatum, RejectsEmptySurfaceAndNonFiniteDatum)
{
    Random random;
    const TinSurface surface = buildFromPoints(planePoints(Plane{0.1, 0.2, 5.0}, 10.0, 10, random));
    ASSERT_FALSE(surface.empty());

    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double inf = std::numeric_limits<double>::infinity();
    EXPECT_EQ(volumeToDatum(surface, nan).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(volumeToDatum(surface, inf).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(volumeToDatum(TinSurface{}, 0.0).error().code, ErrorCode::InvalidArgument);
}

// ---- compareSurfaces --------------------------------------------------------------------

TEST(CompareSurfaces, ASurfaceAgainstItselfIsExactlyZero)
{
    // Every overlay triangle is a triangle of both inputs, and the interpolation
    // weights at a triangle's own corners are exactly (1,0,0): the difference is
    // the zero double, not a small one.
    Random random;
    const TinSurface surface =
        buildFromPoints(planePoints(Plane{0.2, -0.15, 30.0}, 120.0, 300, random));
    ASSERT_FALSE(surface.empty());

    const auto result = compareSurfaces(surface, surface);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result.value().cut, 0.0);
    EXPECT_EQ(result.value().fill, 0.0);
    EXPECT_EQ(result.value().net, 0.0);
    EXPECT_EQ(result.value().cutArea, 0.0);
    EXPECT_EQ(result.value().fillArea, 0.0);
    EXPECT_NEAR(result.value().planArea, surface.planArea(), 1e-8);
    // The square is its own convex hull, so the overlay of the two identical
    // triangulations is that triangulation again.
    EXPECT_EQ(result.value().overlayTriangleCount, surface.triangleCount());
}

TEST(CompareSurfaces, ConstantRaiseIsAllFill)
{
    // Two independently triangulated planes over the same square, the design 2 m
    // above the existing everywhere: fill = 2 * 100^2 = 20000 m^3, no cut.
    Random existingRandom(11);
    Random designRandom(29);
    const Plane ground{0.1, -0.05, 10.0};
    const TinSurface existing = buildFromPoints(planePoints(ground, 100.0, 300, existingRandom));
    const TinSurface design =
        buildFromPoints(planePoints(Plane{ground.a, ground.b, ground.c + 2.0}, 100.0, 350,
                                    designRandom));
    ASSERT_FALSE(existing.empty());
    ASSERT_FALSE(design.empty());

    const auto result = compareSurfaces(existing, design);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_NEAR(result.value().fill, 20000.0, 1e-6);
    EXPECT_NEAR(result.value().cut, 0.0, 1e-9);
    EXPECT_NEAR(result.value().net, 20000.0, 1e-6);
    EXPECT_NEAR(result.value().planArea, 10000.0, 1e-6);
    EXPECT_NEAR(result.value().fillArea, 10000.0, 1e-6);
    EXPECT_NEAR(result.value().cutArea, 0.0, 1e-9);
    EXPECT_GT(result.value().overlayTriangleCount, existing.triangleCount());
}

TEST(CompareSurfaces, TiltedDesignGivesEqualCutAndFill)
{
    // existing: the level plane z = 0. design: z = x - 50 over [0, 100]^2.
    // zDesign - zExisting = x - 50 is odd about x = 50, so
    //   fill = cut = 100 * 50^2/2 = 125000 m^3 and each covers 5000 m^2.
    Random existingRandom(3);
    Random designRandom(101);
    const TinSurface existing =
        buildFromPoints(planePoints(Plane{0.0, 0.0, 0.0}, 100.0, 300, existingRandom));
    const TinSurface design =
        buildFromPoints(planePoints(Plane{1.0, 0.0, -50.0}, 100.0, 300, designRandom));

    const auto result = compareSurfaces(existing, design);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_NEAR(result.value().cut, 125000.0, 1e-6);
    EXPECT_NEAR(result.value().fill, 125000.0, 1e-6);
    EXPECT_NEAR(result.value().net, 0.0, 1e-6);
    EXPECT_NEAR(result.value().planArea, 10000.0, 1e-6);
    EXPECT_NEAR(result.value().cutArea, 5000.0, 1e-6);
    EXPECT_NEAR(result.value().fillArea, 5000.0, 1e-6);
}

TEST(CompareSurfaces, IsAntisymmetricInItsTwoArguments)
{
    // Swapping the surfaces turns every cut into a fill: a definition check that
    // does not depend on the ground being analytic.
    Random existingRandom(5);
    Random designRandom(7);
    TinInput existingInput;
    TinInput designInput;
    for (int i = 0; i < 400; ++i) {
        existingInput.points.emplace_back(existingRandom.real(0, 80), existingRandom.real(0, 80),
                                          existingRandom.real(20, 26));
        designInput.points.emplace_back(designRandom.real(0, 80), designRandom.real(0, 80),
                                        designRandom.real(21, 27));
    }
    // Shared corners so that both surfaces cover the same square exactly.
    for (const Point2 corner : {Point2(0, 0), Point2(80, 0), Point2(80, 80), Point2(0, 80)}) {
        existingInput.points.emplace_back(corner.x, corner.y, 23.0);
        designInput.points.emplace_back(corner.x, corner.y, 24.0);
    }
    const TinSurface existing = build(existingInput).surface;
    const TinSurface design = build(designInput).surface;

    const auto forward = compareSurfaces(existing, design);
    const auto reverse = compareSurfaces(design, existing);
    ASSERT_TRUE(forward.ok()) << forward.error().describe();
    ASSERT_TRUE(reverse.ok()) << reverse.error().describe();
    EXPECT_GT(forward.value().cut, 0.0); // the ground really does cross over
    EXPECT_GT(forward.value().fill, 0.0);
    // Magnitudes of a few thousand m^3 over the same overlay, so only rounding differs.
    EXPECT_NEAR(forward.value().cut, reverse.value().fill, 1e-9);
    EXPECT_NEAR(forward.value().fill, reverse.value().cut, 1e-9);
    EXPECT_NEAR(forward.value().net, -reverse.value().net, 1e-9);
    EXPECT_NEAR(forward.value().cutArea, reverse.value().fillArea, 1e-9);
    EXPECT_NEAR(forward.value().fillArea, reverse.value().cutArea, 1e-9);
    EXPECT_NEAR(forward.value().planArea, reverse.value().planArea, 1e-9);
    EXPECT_NEAR(forward.value().planArea, 6400.0, 1e-6);
    // cut and fill partition the compared area up to where the surfaces coincide,
    // which here is a set of measure zero.
    EXPECT_NEAR(forward.value().cutArea + forward.value().fillArea, forward.value().planArea,
                1e-6);
}

TEST(CompareSurfaces, OnlyTheCommonAreaIsCompared)
{
    // existing covers [0, 100]^2, design covers [50, 150] x [0, 100]: the common
    // ground is the 50 x 100 = 5000 m^2 strip, raised by 1 m, so fill = 5000 m^3.
    Random existingRandom(13);
    Random designRandom(17);
    const TinSurface existing =
        buildFromPoints(planePoints(Plane{0.0, 0.0, 0.0}, 100.0, 300, existingRandom));
    const TinSurface design =
        buildFromPoints(planePoints(Plane{0.0, 0.0, 1.0}, 100.0, 300, designRandom, 50.0, 0.0));

    const auto result = compareSurfaces(existing, design);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_NEAR(result.value().planArea, 5000.0, 1e-6);
    EXPECT_NEAR(result.value().fill, 5000.0, 1e-6);
    EXPECT_NEAR(result.value().cut, 0.0, 1e-9);
    EXPECT_NEAR(result.value().fillArea, 5000.0, 1e-6);
}

TEST(CompareSurfaces, DisjointSurfacesCompareNothing)
{
    Random first(19);
    Random second(23);
    const TinSurface existing =
        buildFromPoints(planePoints(Plane{0.0, 0.0, 0.0}, 10.0, 20, first));
    const TinSurface design =
        buildFromPoints(planePoints(Plane{0.0, 0.0, 5.0}, 10.0, 20, second, 1000.0, 1000.0));

    const auto result = compareSurfaces(existing, design);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result.value().planArea, 0.0);
    EXPECT_EQ(result.value().cut, 0.0);
    EXPECT_EQ(result.value().fill, 0.0);
    EXPECT_EQ(result.value().overlayTriangleCount, 0u);
}

TEST(CompareSurfaces, AgreesWithVolumeToDatumOnALevelDesign)
{
    // A level design surface at z = d is the datum d, so comparing against it must
    // reproduce volumeToDatum: cut = above, fill = below. Two independent code
    // paths (closed-form per triangle vs. exact overlay) for the same integral.
    Random random;
    TinInput input;
    for (int i = 0; i < 500; ++i) {
        input.points.emplace_back(random.real(0.5, 199.5), random.real(0.5, 199.5),
                                  random.real(10, 40));
    }
    for (const Point2 corner : {Point2(0, 0), Point2(200, 0), Point2(200, 200), Point2(0, 200)}) {
        input.points.emplace_back(corner.x, corner.y, 25.0);
    }
    const TinSurface existing = build(input).surface;
    ASSERT_FALSE(existing.empty());

    const double datum = 25.0;
    TinInput levelInput;
    for (const Point2 corner : {Point2(0, 0), Point2(200, 0), Point2(200, 200), Point2(0, 200)}) {
        levelInput.points.emplace_back(corner.x, corner.y, datum);
    }
    const TinSurface design = build(levelInput).surface;

    const auto toDatum = volumeToDatum(existing, datum);
    const auto compared = compareSurfaces(existing, design);
    ASSERT_TRUE(toDatum.ok()) << toDatum.error().describe();
    ASSERT_TRUE(compared.ok()) << compared.error().describe();
    // Volumes of order 1e5 reached through different splittings of the same
    // region; the overlay adds one barycentric interpolation per corner.
    EXPECT_NEAR(compared.value().cut, toDatum.value().above, 1e-6);
    EXPECT_NEAR(compared.value().fill, toDatum.value().below, 1e-6);
    EXPECT_NEAR(compared.value().cutArea, toDatum.value().planAreaAbove, 1e-6);
    EXPECT_NEAR(compared.value().fillArea, toDatum.value().planAreaBelow, 1e-6);
    EXPECT_NEAR(compared.value().planArea, existing.planArea(), 1e-6);
}

TEST(CompareSurfaces, RejectsEmptySurfaces)
{
    Random random;
    const TinSurface surface = buildFromPoints(planePoints(Plane{0.0, 0.0, 1.0}, 10.0, 8, random));
    ASSERT_FALSE(surface.empty());
    EXPECT_EQ(compareSurfaces(TinSurface{}, surface).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(compareSurfaces(surface, TinSurface{}).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(compareSurfaces(TinSurface{}, TinSurface{}).error().code, ErrorCode::InvalidArgument);
}
