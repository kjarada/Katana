// Volumes inside a polygon, to a level, of a stockpile and on a grid
// (volume.hpp). Every expected value is a solid whose volume is known in
// closed form - a prism, a pyramid, the volume between parallel planes - worked
// in the comment beside it.

#include <gtest/gtest.h>

#include <bit>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <vector>

#include "katana/core/task_pool.hpp"
#include "katana/math/numerics.hpp"
#include "katana/terrain/volume.hpp"
#include "terrain_test_support.hpp"

using namespace katana::terrain;
using namespace katana::terrain::testing;
using katana::core::ErrorCode;
using katana::geometry::Polyline2;
using katana::test::Random;

namespace {

Polyline2 ring(std::vector<Point2> vertices)
{
    Polyline2 polygon;
    polygon.vertices = std::move(vertices);
    polygon.closed = true;
    return polygon;
}

Polyline2 square(double x0, double y0, double size)
{
    return ring({Point2(x0, y0), Point2(x0 + size, y0), Point2(x0 + size, y0 + size),
                 Point2(x0, y0 + size)});
}

OverlayRegion regionOf(const Polyline2& polygon, const std::vector<Polyline2>& holes = {})
{
    auto region = OverlayRegion::fromPolygon(polygon, holes);
    EXPECT_TRUE(region.ok()) << region.error().describe();
    return region.ok() ? std::move(region).value() : OverlayRegion{};
}

TinSurface planeSurface(const Plane& plane, double size, std::size_t interior, std::uint64_t seed,
                        double x0 = 0.0, double y0 = 0.0)
{
    Random random(seed);
    return buildFromPoints(planePoints(plane, size, interior, random, x0, y0));
}

bool sameBits(double a, double b)
{
    return std::bit_cast<std::uint64_t>(a) == std::bit_cast<std::uint64_t>(b);
}

// A pile of `facets` planar faces: its toe a regular polygon of `facets`
// corners on a circle of `radius` round (0, 0), standing on the ground plane
// `ground`, its apex `height` above the ground at the centre - inside a
// 100 x 100 site of the same ground. The toe is a breakline, so no triangle
// crosses it, and the regular polygon with its centre triangulates as the
// fan from the apex: one face per side.
struct Pile {
    TinSurface surface;
    Polyline2 toe;
};

Pile pile(const Plane& ground, std::size_t facets, double radius, double height)
{
    TinInput input;
    for (const Point2 corner :
         {Point2(-50, -50), Point2(50, -50), Point2(50, 50), Point2(-50, 50)}) {
        input.points.emplace_back(corner.x, corner.y, ground.at(corner.x, corner.y));
    }
    Breakline toe;
    toe.closed = true;
    Pile result;
    result.toe.closed = true;
    for (std::size_t k = 0; k < facets; ++k) {
        const double angle =
            katana::math::kTwoPi * static_cast<double>(k) / static_cast<double>(facets);
        const double x = radius * std::cos(angle);
        const double y = radius * std::sin(angle);
        toe.vertices.emplace_back(x, y, ground.at(x, y));
        result.toe.vertices.emplace_back(x, y);
    }
    input.breaklines.push_back(toe);
    input.points.emplace_back(0.0, 0.0, ground.at(0, 0) + height);
    result.surface = build(input).surface;
    return result;
}

// The plan area of a regular polygon of n corners on a circle of radius r:
// n triangles of two radii at 2 pi / n, (n / 2) r^2 sin(2 pi / n).
double regularPolygonArea(std::size_t n, double r)
{
    return 0.5 * static_cast<double>(n) * r * r *
           std::sin(katana::math::kTwoPi / static_cast<double>(n));
}

} // namespace

// ---- surface to surface inside a polygon --------------------------------------------------------

TEST(VolumeInsidePolygon, TwoParallelPlanesOneMetreApartOverATenByTenPolygonHoldOneHundredCubicMetres)
{
    // A 1 m slab over 10 x 10 m: 100 m^3 of fill, none of cut, whatever the
    // two triangulations and however the polygon crosses them.
    const TinSurface base = planeSurface(Plane{0.05, 0.02, 10.0}, 100.0, 200, 11);
    const TinSurface top = planeSurface(Plane{0.05, 0.02, 11.0}, 100.0, 170, 12);
    const OverlayRegion region = regionOf(square(33.3, 41.7, 10.0));
    const auto volume = compareSurfaces(base, top, region);
    ASSERT_TRUE(volume.ok()) << volume.error().describe();
    // Heights to 17 read off planes: each depth within a few ulps of 1; 100
    // m^2 of pieces.
    EXPECT_NEAR(volume->fill, 100.0, 1e-9);
    EXPECT_NEAR(volume->cut, 0.0, 1e-12);
    EXPECT_NEAR(volume->planArea, 100.0, 1e-9);
}

TEST(VolumeInsidePolygon, TheEdgesOfAPolygonCutTrianglesWhereTheyLieNotByWholeTriangles)
{
    // Two triangles each, a 1 m slab, and a polygon whose edges cross both
    // diagonals: the slab under it is its area, 1/2 |(b - a) x (c - a)| =
    // 1/2 (67.6 x 67.6 - 14.7 x 26.7) = 2088.635.
    const TinSurface base = planeSurface(Plane{0, 0, 0}, 100.0, 0, 1);
    const TinSurface top = planeSurface(Plane{0, 0, 1}, 100.0, 0, 1);
    const OverlayRegion region =
        regionOf(ring({Point2(13.7, 21.2), Point2(81.3, 35.9), Point2(40.4, 88.8)}));
    const auto volume = compareSurfaces(base, top, region);
    ASSERT_TRUE(volume.ok());
    EXPECT_NEAR(volume->fill, 2088.635, 1e-9);
}

TEST(VolumeInsidePolygon, AHoleInThePolygonIsLeftOut)
{
    // 100 x 100 less 20 x 20, under a 1 m slab: 9600.
    const TinSurface base = planeSurface(Plane{0.01, 0.0, 3.0}, 100.0, 60, 5);
    const TinSurface top = planeSurface(Plane{0.01, 0.0, 4.0}, 100.0, 60, 6);
    const OverlayRegion region = regionOf(square(0, 0, 100), {square(40, 40, 20)});
    const auto volume = compareSurfaces(base, top, region);
    ASSERT_TRUE(volume.ok());
    EXPECT_NEAR(volume->fill, 9600.0, 1e-8);
    EXPECT_NEAR(volume->planArea, 9600.0, 1e-8);
}

TEST(VolumeInsidePolygon, CutAndFillSplitWhereTheSurfacesCrossInsideThePolygon)
{
    // Base z = 0, top z = x - 50, inside [25, 75]^2: the top is below the
    // base west of x = 50 and above it east, each half 50 (in y) x 25^2 / 2 =
    // 15 625.
    const TinSurface base = planeSurface(Plane{0, 0, 0}, 100.0, 40, 8);
    const TinSurface top = planeSurface(Plane{1, 0, -50}, 100.0, 40, 9);
    const auto volume = compareSurfaces(base, top, regionOf(square(25, 25, 50)));
    ASSERT_TRUE(volume.ok());
    EXPECT_NEAR(volume->cut, 15625.0, 1e-8);
    EXPECT_NEAR(volume->fill, 15625.0, 1e-8);
    EXPECT_NEAR(volume->net, 0.0, 1e-8);
    EXPECT_NEAR(volume->cutArea, 1250.0, 1e-9);
    EXPECT_NEAR(volume->fillArea, 1250.0, 1e-9);
}

TEST(VolumeInsidePolygon, GivesTheSameBitsOnOneThreadAsOnMany)
{
    Random random(71);
    TinInput rough;
    TinInput rougher;
    for (int i = 0; i < 3000; ++i) {
        rough.points.emplace_back(random.real(0, 200), random.real(0, 200), random.real(10, 14));
        rougher.points.emplace_back(random.real(0, 200), random.real(0, 200), random.real(10, 14));
    }
    const TinSurface base = build(rough).surface;
    const TinSurface top = build(rougher).surface;
    const OverlayRegion region = regionOf(
        ring({Point2(20, 20), Point2(180, 30), Point2(150, 170), Point2(90, 120), Point2(30, 160)}));
    katana::core::TaskPool serial(0);
    const auto one = compareSurfaces(base, top, region, &serial);
    ASSERT_TRUE(one.ok());
    for (const std::size_t workers : {1u, 3u, 7u}) {
        katana::core::TaskPool pool(workers);
        const auto many = compareSurfaces(base, top, region, &pool);
        ASSERT_TRUE(many.ok());
        EXPECT_TRUE(sameBits(one->cut, many->cut)) << workers;
        EXPECT_TRUE(sameBits(one->fill, many->fill)) << workers;
        EXPECT_TRUE(sameBits(one->planArea, many->planArea)) << workers;
    }
}

// ---- surface to a level inside a polygon --------------------------------------------------------

TEST(VolumeToLevelInsidePolygon, ATiltedPlaneSplitsAtTheLevelLineInsideASquare)
{
    // z = x against the level 50, inside [25, 75]^2: above it east of x = 50,
    // 50 x 25^2 / 2 = 15 625, and as much below west of it.
    const TinSurface surface = planeSurface(Plane{1, 0, 0}, 100.0, 300, 13);
    const auto volume = volumeToDatum(surface, 50.0, regionOf(square(25, 25, 50)));
    ASSERT_TRUE(volume.ok()) << volume.error().describe();
    EXPECT_NEAR(volume->above, 15625.0, 1e-8);
    EXPECT_NEAR(volume->below, 15625.0, 1e-8);
    EXPECT_NEAR(volume->planArea, 2500.0, 1e-9);
}

TEST(VolumeToLevelInsidePolygon, APrismOfFlatGroundAboveALevelIsItsDepthTimesItsArea)
{
    // Ground at 5 against the level 2 inside an L of 40 x 20 + 20 x 20 =
    // 1200 m^2: a prism 3 m deep, 3600 m^3, though the L is not convex.
    const TinSurface surface = planeSurface(Plane{0, 0, 5}, 100.0, 50, 14);
    const auto volume = volumeToDatum(
        surface, 2.0,
        regionOf(ring({Point2(10, 10), Point2(50, 10), Point2(50, 30), Point2(30, 30),
                       Point2(30, 50), Point2(10, 50)})));
    ASSERT_TRUE(volume.ok());
    EXPECT_NEAR(volume->above, 3600.0, 1e-9);
    EXPECT_NEAR(volume->below, 0.0, 1e-12);
    EXPECT_NEAR(volume->net, 3600.0, 1e-9);
}

TEST(VolumeToLevelInsidePolygon, APolygonPartlyOffTheSurfaceMeasuresTheGroundUnderIt)
{
    // [90, 110] x [0, 20] over ground that ends at x = 100: 10 x 20 = 200 m^2
    // of it on the ground, 3 m above the level, 600 m^3. The plan area says
    // what was measured.
    const TinSurface surface = planeSurface(Plane{0, 0, 5}, 100.0, 20, 15);
    const auto volume = volumeToDatum(surface, 2.0, regionOf(square(90, 0, 20)));
    ASSERT_TRUE(volume.ok());
    EXPECT_NEAR(volume->planArea, 200.0, 1e-9);
    EXPECT_NEAR(volume->above, 600.0, 1e-9);
}

TEST(VolumeToLevelInsidePolygon, AWholeSurfaceRegionAgreesWithTheVolumeToTheDatum)
{
    // A region that is the surface's own square is the plain volumeToDatum.
    Random random(16);
    TinInput input;
    for (const Point2 corner : {Point2(0, 0), Point2(80, 0), Point2(80, 80), Point2(0, 80)}) {
        input.points.emplace_back(corner.x, corner.y, 20.0);
    }
    for (int i = 0; i < 500; ++i) {
        input.points.emplace_back(random.real(1, 79), random.real(1, 79), random.real(15, 25));
    }
    const TinSurface surface = build(input).surface;
    const auto whole = volumeToDatum(surface, 19.0);
    const auto inside = volumeToDatum(surface, 19.0, regionOf(square(0, 0, 80)));
    ASSERT_TRUE(whole.ok() && inside.ok());
    EXPECT_NEAR(inside->above, whole->above, 1e-7);
    EXPECT_NEAR(inside->below, whole->below, 1e-7);
    EXPECT_NEAR(inside->planArea, 6400.0, 1e-8);
}

TEST(VolumeToLevelInsidePolygon, AFlatSurfaceAgainstItsOwnLevelHasNeitherCutAreaNorFillArea)
{
    // z = 10 over [0, 10]^2 against the level 10 inside a skewed quadrilateral:
    // above = below = 0 whatever the polygon. The region is compared with
    // the surface as a flat surface of its own, read by weights that sum to 1
    // only to a few ulps, so the difference at a corner that is no vertex was
    // +/- 1e-15: 44.6 m2 of the polygon were booked as cut area and 23.3 as
    // fill area, and none is either. The polygon's area by the shoelace sum:
    //   (0.123 x 0.321 - 9.876 x 0.456) + (9.876 x 9.9 - 9.5 x 0.321)
    //   + (9.5 x 9.7 - 0.2 x 9.9) + (0.2 x 0.456 - 0.123 x 9.7)
    //   = -4.463973 + 94.7229 + 90.17 - 1.1019 = 179.327027, halved.
    const TinSurface flat = buildFromPoints({Point3(0, 0, 10), Point3(10, 0, 10),
                                             Point3(10, 10, 10), Point3(0, 10, 10)});
    const OverlayRegion skewed = regionOf(ring({Point2(0.123, 0.456), Point2(9.876, 0.321),
                                                Point2(9.5, 9.9), Point2(0.2, 9.7)}));
    const OverlayRegion whole = regionOf(square(0, 0, 10));
    const OverlayRegion sliver = regionOf(ring({Point2(1.3, 1.1), Point2(8.7, 1.2), Point2(8.9, 1.4)}));
    for (const OverlayRegion* region : {&skewed, &whole, &sliver}) {
        const auto volume = volumeToDatum(flat, 10.0, *region);
        ASSERT_TRUE(volume.ok()) << volume.error().describe();
        EXPECT_NEAR(volume->planArea, region->area(), 1e-9);
        EXPECT_NEAR(volume->above, 0.0, 1e-9);
        EXPECT_NEAR(volume->below, 0.0, 1e-9);
        EXPECT_EQ(volume->planAreaAbove, 0.0);
        EXPECT_EQ(volume->planAreaBelow, 0.0);
    }
    EXPECT_NEAR(skewed.area(), 89.6635135, 1e-9);
    // Without a region the surface is read at its own vertices, exactly.
    const auto exact = volumeToDatum(flat, 10.0);
    ASSERT_TRUE(exact.ok());
    EXPECT_EQ(exact->planAreaAbove, 0.0);
    EXPECT_EQ(exact->planAreaBelow, 0.0);
}

TEST(VolumeToLevelInsidePolygon, RefusesANonFiniteLevelAnEmptySurfaceAndAnEmptyRegion)
{
    const TinSurface surface = planeSurface(Plane{0, 0, 5}, 10.0, 5, 17);
    const OverlayRegion region = regionOf(square(0, 0, 5));
    EXPECT_EQ(volumeToDatum(surface, std::nan(""), region).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(volumeToDatum(TinSurface{}, 0.0, region).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(volumeToDatum(surface, 0.0, OverlayRegion{}).error().code,
              ErrorCode::InvalidArgument);
}

// ---- stockpiles -------------------------------------------------------------------------------

TEST(Stockpile, APileOfFacetsOnLevelGroundIsAThirdOfItsBaseTimesItsHeight)
{
    // Twelve faces on a toe of radius 20: a pyramid on a regular dodecagon,
    // base (12 / 2) 20^2 sin 30 deg = 1200 m^2, height 9: V = 1200 x 9 / 3 =
    // 3600 m^3 above a base made of the toe's own heights.
    const Pile heap = pile(Plane{0, 0, 0}, 12, 20.0, 9.0);
    ASSERT_NEAR(regularPolygonArea(12, 20.0), 1200.0, 1e-9);
    const auto volume = stockpileVolume(heap.surface, heap.toe, StockpileOptions{});
    ASSERT_TRUE(volume.ok()) << volume.error().describe();
    EXPECT_NEAR(volume->volume.above, 3600.0, 1e-8);
    EXPECT_NEAR(volume->volume.below, 0.0, 1e-9);
    EXPECT_NEAR(volume->boundaryArea, 1200.0, 1e-9);
    EXPECT_FALSE(volume->boundaryHeights);
    EXPECT_FALSE(volume->level.has_value());
}

TEST(Stockpile, ManyFacetsApproachTheConeFormula)
{
    // 360 faces on a toe of radius 20, height 9: the pyramid's
    // (1/3) (360/2) 20^2 sin 1 deg x 9 = 3769.6, which the cone's
    // pi 20^2 x 9 / 3 = 3769.9 exceeds by the circle's area beyond the
    // polygon's, (1 - sin(2 pi / n) / (2 pi / n)) of it: 0.19 m^3.
    const Pile heap = pile(Plane{0, 0, 0}, 360, 20.0, 9.0);
    const auto volume = stockpileVolume(heap.surface, heap.toe, StockpileOptions{});
    ASSERT_TRUE(volume.ok()) << volume.error().describe();
    const double facets = regularPolygonArea(360, 20.0) * 9.0 / 3.0;
    const double cone = katana::math::kPi * 400.0 * 9.0 / 3.0;
    EXPECT_NEAR(volume->volume.above, facets, 1e-7);
    EXPECT_NEAR(volume->volume.above, cone, 0.2);
    EXPECT_LT(volume->volume.above, cone);
}

TEST(Stockpile, OnSlopingGroundTheToeBaseFollowsTheSlope)
{
    // The same pile on the ground z = 0.1 x + 5: the toe's heights make a
    // base in the plane of the ground, and a pyramid's volume is a third of
    // its plan area times its height measured plumb from that plane, so
    // still 1200 x 9 / 3 = 3600.
    const Pile heap = pile(Plane{0.1, 0, 5}, 12, 20.0, 9.0);
    const auto volume = stockpileVolume(heap.surface, heap.toe, StockpileOptions{});
    ASSERT_TRUE(volume.ok()) << volume.error().describe();
    EXPECT_NEAR(volume->volume.above, 3600.0, 1e-8);
    EXPECT_NEAR(volume->volume.below, 0.0, 1e-9);
}

TEST(Stockpile, OnSlopingGroundWithAPolygonWiderThanTheToeOnlyTheToeIsCutArea)
{
    // The pile of twelve faces on a toe of radius 20 (1200 m2) on the ground
    // z = 0.1 x + 5, measured inside the square [-30, 30]^2 of 3600 m2 with a
    // base made of the boundary's heights read off the ground: a plane. The
    // pile stands above it over the toe, 3600 m3, and everywhere else the
    // ground is the base: cut area 1200, fill area 0, and the other 2400 m2
    // neither (it was 700 m2 of cut round a toe of 400 and 75 of fill).
    const Pile heap = pile(Plane{0.1, 0, 5}, 12, 20.0, 9.0);
    const Polyline2 wider = square(-30, -30, 60);
    const auto volume = stockpileVolume(heap.surface, wider, StockpileOptions{});
    ASSERT_TRUE(volume.ok()) << volume.error().describe();
    EXPECT_NEAR(volume->volume.above, 3600.0, 1e-7);
    EXPECT_NEAR(volume->volume.below, 0.0, 1e-9);
    EXPECT_NEAR(volume->volume.planArea, 3600.0, 1e-8);
    EXPECT_NEAR(volume->volume.planAreaAbove, 1200.0, 1e-8);
    EXPECT_EQ(volume->volume.planAreaBelow, 0.0);

    // Against a level just under the whole polygon's ground the ground is
    // all above it: the whole 3600 m2 is cut area, the pile's faces and the
    // plane beside them alike.
    StockpileOptions level;
    level.base = StockpileBase::Level;
    level.level = -5.0;
    const auto beneath = stockpileVolume(heap.surface, wider, level);
    ASSERT_TRUE(beneath.ok()) << beneath.error().describe();
    EXPECT_NEAR(beneath->volume.planAreaAbove, 3600.0, 1e-8);
    EXPECT_EQ(beneath->volume.planAreaBelow, 0.0);
}

TEST(Stockpile, AgainstTheLowestPointOfTheToeTheWedgeOfGroundBelowItIsAdded)
{
    // On z = 0.1 x + 5 the toe's lowest point is its vertex at x = -20,
    // height 3. Under the pile, above 3, is the ground's wedge: the toe's
    // area times the ground's mean height over it less 3 - the plane at the
    // toe's centroid (0, 0), 5 - so 1200 x 2 = 2400 more: 6000.
    const Pile heap = pile(Plane{0.1, 0, 5}, 12, 20.0, 9.0);
    StockpileOptions options;
    options.base = StockpileBase::Lowest;
    const auto volume = stockpileVolume(heap.surface, heap.toe, options);
    ASSERT_TRUE(volume.ok()) << volume.error().describe();
    ASSERT_TRUE(volume->level.has_value());
    EXPECT_NEAR(*volume->level, 3.0, 1e-12);
    EXPECT_NEAR(volume->volume.above, 6000.0, 1e-8);
    EXPECT_NEAR(volume->volume.below, 0.0, 1e-9);
}

TEST(Stockpile, AgainstAGivenLevelTheGroundAboveItIsAdded)
{
    // Level 2, below the whole toe: 3600 + 1200 x (5 - 2) = 7200.
    const Pile heap = pile(Plane{0.1, 0, 5}, 12, 20.0, 9.0);
    StockpileOptions options;
    options.base = StockpileBase::Level;
    options.level = 2.0;
    const auto volume = stockpileVolume(heap.surface, heap.toe, options);
    ASSERT_TRUE(volume.ok()) << volume.error().describe();
    EXPECT_NEAR(volume->volume.above, 7200.0, 1e-8);
    EXPECT_EQ(*volume->level, 2.0);
}

TEST(Stockpile, APitBelowItsRimIsMeasuredBelowTheBase)
{
    // An inverted pile - a pit 9 deep - against its rim: 3600 below, none
    // above.
    const Pile pit = pile(Plane{0, 0, 0}, 12, 20.0, -9.0);
    const auto volume = stockpileVolume(pit.surface, pit.toe, StockpileOptions{});
    ASSERT_TRUE(volume.ok()) << volume.error().describe();
    EXPECT_NEAR(volume->volume.below, 3600.0, 1e-8);
    EXPECT_NEAR(volume->volume.above, 0.0, 1e-9);
}

TEST(Stockpile, AToeVertexOffTheGroundWithNoHeightOfItsOwnIsRefusedAndWithOneIsUsed)
{
    // The square [40, 60]^2 reaches past the 100 x 100 site (it ends at 50).
    const Pile heap = pile(Plane{0, 0, 0}, 12, 20.0, 9.0);
    const Polyline2 toe = square(40, 40, 20);
    const auto refused = stockpileVolume(heap.surface, toe, StockpileOptions{});
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(refused.error().message.find("off the surface"), std::string::npos);

    // Given its own heights, at 0, it is the base: the ground inside it is
    // flat at 0, so nothing is above or below it.
    StockpileOptions own;
    own.boundaryHeights = {0.0, 0.0, 0.0, 0.0};
    const auto used = stockpileVolume(heap.surface, toe, own);
    ASSERT_TRUE(used.ok()) << used.error().describe();
    EXPECT_TRUE(used->boundaryHeights);
    EXPECT_NEAR(used->volume.above, 0.0, 1e-9);
    EXPECT_NEAR(used->volume.planArea, 100.0, 1e-9); // the quarter on the site
}

TEST(Stockpile, AToeThatNeverMeetsTheGroundHasNoLowestPoint)
{
    const Pile heap = pile(Plane{0, 0, 0}, 12, 20.0, 9.0);
    StockpileOptions options;
    options.base = StockpileBase::Lowest;
    const auto refused = stockpileVolume(heap.surface, square(200, 200, 10), options);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidGeometry);
}

TEST(Stockpile, AToeThatIsNoPolygonIsRefused)
{
    const Pile heap = pile(Plane{0, 0, 0}, 12, 20.0, 9.0);
    const auto refused = stockpileVolume(
        heap.surface, ring({Point2(0, 0), Point2(10, 10), Point2(10, 0), Point2(0, 10)}),
        StockpileOptions{});
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidGeometry);
}

// ---- the grid method --------------------------------------------------------------------------

TEST(GridVolume, TheMidpointRuleOverstatesAPyramidByATenthOfACellCubedInEachCellOnARidge)
{
    // The pyramid on [0, 100]^2 with its apex 30 high: exactly 100 000 m^3
    // between it and the plane z = 0. Its height is 30 - 0.6 max(u, v), u
    // and v the distances from the centre lines, so on any cell of a grid
    // whose lines include x = y = 50 it is linear - and the midpoint rule
    // exact - except on the cells the four ridges u = v run through, 50 / c
    // of them each. There max(u, v) averages c / 6 more than at the centre
    // (the mean of the larger of two uniform offsets in [-c/2, c/2]), so the
    // cell's prism stands 0.6 c / 6 = 0.1 c too high: 0.1 c^3 a cell, and
    // over 200 / c cells, 20 c^2 in all.
    Random random;
    const TinSurface ground = buildFromPoints(planePoints(Plane{0, 0, 0}, 100.0, 0, random));
    const TinSurface pyramid = build(pyramidInput(100.0, 30.0, 0, random)).surface;
    const auto exact = compareSurfaces(ground, pyramid);
    ASSERT_TRUE(exact.ok());
    EXPECT_NEAR(exact->fill, 100000.0, 1e-7);
    for (const double cell : {5.0, 2.0, 1.0, 0.5}) {
        const auto grid = gridVolume(ground, pyramid, cell);
        ASSERT_TRUE(grid.ok()) << grid.error().describe();
        SCOPED_TRACE(cell);
        EXPECT_NEAR(grid->fill - exact->fill, 20.0 * cell * cell, 1e-6);
        EXPECT_EQ(grid->cells, static_cast<std::size_t>(std::lround(10000.0 / (cell * cell))));
        EXPECT_NEAR(grid->planArea, 10000.0, 1e-9);
        EXPECT_EQ(grid->cut, 0.0);
    }
}

TEST(GridVolume, InsideAPolygonOnlyTheCellsWhoseCentresAreInsideCount)
{
    // A 1 m slab inside [10, 20]^2 on 1 m cells: the hundred cells whose
    // centres lie inside, 100 m^3; a polygon offset half a cell takes the
    // centres of 10 x 10 cells all the same.
    const TinSurface base = planeSurface(Plane{0, 0, 0}, 100.0, 30, 21);
    const TinSurface top = planeSurface(Plane{0, 0, 1}, 100.0, 30, 22);
    const OverlayRegion region = regionOf(square(10, 10, 10));
    const auto grid = gridVolume(base, top, 1.0, &region);
    ASSERT_TRUE(grid.ok());
    EXPECT_EQ(grid->cells, 100u);
    EXPECT_NEAR(grid->fill, 100.0, 1e-12);
    const OverlayRegion shifted = regionOf(square(10.25, 10.25, 10));
    const auto offset = gridVolume(base, top, 1.0, &shifted);
    ASSERT_TRUE(offset.ok());
    EXPECT_EQ(offset->cells, 100u);
}

TEST(GridVolume, LotsSharingAnEdgeOnALineOfCentresCountItsCellsOnceNotTwice)
{
    // A 1 m slab over [0, 50]^2 on 1 m cells, whose centres are at k + 0.5.
    // Lot A is [5, 25.5] x [5, 45] and lot B [25.5, 45] x [5, 45]: they share
    // the edge x = 25.5, a line of centres. A holds the centres x = 5.5 ... 25.5
    // (21 of them) by y = 5.5 ... 44.5 (40): 840 cells; B holds x = 25.5 ...
    // 44.5 (20) by 40: 800. Measured on their own and summed that is 1640, the
    // 40 centres on x = 25.5 twice. Their union [5, 45]^2 holds x = 5.5 ... 44.5
    // (40) by 40 = 1600 cells, its area exactly, and the exact overlay agrees.
    const TinSurface base = planeSurface(Plane{0, 0, 0}, 50.0, 30, 31);
    const TinSurface top = planeSurface(Plane{0, 0, 1}, 50.0, 30, 32);
    const OverlayRegion lotA =
        regionOf(ring({Point2(5, 5), Point2(25.5, 5), Point2(25.5, 45), Point2(5, 45)}));
    const OverlayRegion lotB =
        regionOf(ring({Point2(25.5, 5), Point2(45, 5), Point2(45, 45), Point2(25.5, 45)}));
    const OverlayRegion both = regionOf(square(5, 5, 40));
    const auto alone = [&](const OverlayRegion& region) {
        const auto grid = gridVolume(base, top, 1.0, &region);
        EXPECT_TRUE(grid.ok());
        return grid.ok() ? grid->cells : std::size_t{0};
    };
    EXPECT_EQ(alone(lotA), 840u);
    EXPECT_EQ(alone(lotB), 800u);
    EXPECT_EQ(alone(both), 1600u);

    // One after the other, the second leaving the cells the first holds: the
    // cells add up to the union's whichever lot goes first.
    const auto ab =
        gridVolume(base, top, 1.0, &lotB, nullptr, std::span<const OverlayRegion>(&lotA, 1));
    const auto ba =
        gridVolume(base, top, 1.0, &lotA, nullptr, std::span<const OverlayRegion>(&lotB, 1));
    ASSERT_TRUE(ab.ok());
    ASSERT_TRUE(ba.ok());
    EXPECT_EQ(ab->cells, 800u - 40u); // B without the 40 centres A holds on x = 25.5
    EXPECT_EQ(ba->cells, 840u - 40u); // A without the same 40
    EXPECT_EQ(840u + ab->cells, 1600u);
    EXPECT_EQ(800u + ba->cells, 1600u);
    EXPECT_NEAR(ab->fill, 760.0, 1e-9);
    EXPECT_NEAR(ab->planArea, 760.0, 1e-9);
    EXPECT_EQ(ab->cut, 0.0);
    const auto exact = compareSurfaces(base, top, both);
    ASSERT_TRUE(exact.ok());
    EXPECT_NEAR(exact->fill, 1600.0, 1e-9);
}

TEST(GridVolume, OverlappingPolygonsCountEachCellOnceAndTheNeighboursOfTheCaseAreUnchanged)
{
    // A = [5, 30] x [5, 45] holds x = 5.5 ... 29.5 (25) by 40 = 1000 cells,
    // B = [20, 45] x [5, 45] holds x = 20.5 ... 44.5 (25) by 40 = 1000; they
    // overlap on x = 20.5 ... 29.5 (10 columns, 400 cells). The union holds
    // x = 5.5 ... 44.5 = 1600, so B after A counts 1000 - 400 = 600.
    const TinSurface base = planeSurface(Plane{0, 0, 0}, 50.0, 30, 33);
    const TinSurface top = planeSurface(Plane{0, 0, 1}, 50.0, 30, 34);
    const OverlayRegion a =
        regionOf(ring({Point2(5, 5), Point2(30, 5), Point2(30, 45), Point2(5, 45)}));
    const OverlayRegion b =
        regionOf(ring({Point2(20, 5), Point2(45, 5), Point2(45, 45), Point2(20, 45)}));
    const auto second =
        gridVolume(base, top, 1.0, &b, nullptr, std::span<const OverlayRegion>(&a, 1));
    ASSERT_TRUE(second.ok());
    EXPECT_EQ(second->cells, 600u);
    EXPECT_EQ(gridVolume(base, top, 1.0, &a)->cells + second->cells, 1600u);

    // The neighbours: nothing claimed is the grid as it was; a region claimed
    // by itself has nothing left (cells 0, not an error); a claim that is
    // elsewhere takes nothing from it.
    const auto none = gridVolume(base, top, 1.0, &a, nullptr, {});
    ASSERT_TRUE(none.ok());
    EXPECT_EQ(none->cells, 1000u);
    const auto itself =
        gridVolume(base, top, 1.0, &a, nullptr, std::span<const OverlayRegion>(&a, 1));
    ASSERT_TRUE(itself.ok());
    EXPECT_EQ(itself->cells, 0u);
    EXPECT_EQ(itself->fill, 0.0);
    const OverlayRegion elsewhere = regionOf(square(100, 100, 5));
    const auto apart =
        gridVolume(base, top, 1.0, &a, nullptr, std::span<const OverlayRegion>(&elsewhere, 1));
    ASSERT_TRUE(apart.ok());
    EXPECT_EQ(apart->cells, 1000u);
}

TEST(GridVolume, RefusesACellThatIsNoLengthAndAGridTooLargeNamingACellThatFits)
{
    const TinSurface base = planeSurface(Plane{0, 0, 0}, 100.0, 5, 23);
    const TinSurface top = planeSurface(Plane{0, 0, 1}, 100.0, 5, 24);
    EXPECT_EQ(gridVolume(base, top, 0.0).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(gridVolume(base, top, -1.0).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(gridVolume(base, top, std::nan("")).error().code, ErrorCode::InvalidArgument);
    // 100 m at 1 mm is 1e10 cells; 25 million fit at sqrt(1e4 / 2.5e7) = 0.02 m.
    const auto huge = gridVolume(base, top, 0.001);
    ASSERT_FALSE(huge.ok());
    EXPECT_EQ(huge.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(huge.error().message.find("0.02"), std::string::npos) << huge.error().message;
    EXPECT_EQ(gridVolume(base, TinSurface{}, 1.0).error().code, ErrorCode::InvalidArgument);
}

TEST(GridVolume, GivesTheSameBitsOnOneThreadAsOnMany)
{
    Random random(81);
    TinInput rough;
    for (int i = 0; i < 2000; ++i) {
        rough.points.emplace_back(random.real(0, 100), random.real(0, 100), random.real(0, 3));
    }
    const TinSurface base = build(rough).surface;
    const TinSurface top = planeSurface(Plane{0, 0, 1.5}, 100.0, 10, 82);
    katana::core::TaskPool serial(0);
    const auto one = gridVolume(base, top, 0.5, nullptr, &serial);
    ASSERT_TRUE(one.ok());
    for (const std::size_t workers : {2u, 5u}) {
        katana::core::TaskPool pool(workers);
        const auto many = gridVolume(base, top, 0.5, nullptr, &pool);
        ASSERT_TRUE(many.ok());
        EXPECT_TRUE(sameBits(one->cut, many->cut));
        EXPECT_TRUE(sameBits(one->fill, many->fill));
        EXPECT_EQ(one->cells, many->cells);
    }
}
