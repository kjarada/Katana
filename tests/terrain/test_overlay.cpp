// The overlay of two surfaces (overlay.hpp): the region it may be cut to, the
// pieces it hands out, and the difference and envelope surfaces made of it.
// Every expected value is worked by hand from planes and squares in the
// comments, or is another method's answer for the same quantity
// (compareSurfaces, volumeToDatum) - never a previous run's.

#include <gtest/gtest.h>

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <vector>

#include "katana/core/task_pool.hpp"
#include "katana/terrain/overlay.hpp"
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

// A plane over [x0, x0 + size]^2: its four corners and `interior` points on
// it at random, so the triangulation is irregular but the surface is the plane.
TinSurface planeSurface(const Plane& plane, double size, std::size_t interior, std::uint64_t seed,
                        double x0 = 0.0, double y0 = 0.0)
{
    Random random(seed);
    return buildFromPoints(planePoints(plane, size, interior, random, x0, y0));
}

// Rolling ground over [0, 200]^2: two sine terms, so a second one lifted a
// little crosses it in many places and every sign case of the overlay runs.
TinSurface rolling(std::uint64_t seed, std::size_t interior, double lift)
{
    Random random(seed);
    const auto height = [lift](double x, double y) {
        return 30.0 + lift + 1.5 * std::sin(x * 0.07) * std::cos(y * 0.05) + 0.01 * x;
    };
    std::vector<Point3> points;
    for (const Point2 corner : {Point2(0, 0), Point2(200, 0), Point2(200, 200), Point2(0, 200)}) {
        points.emplace_back(corner.x, corner.y, height(corner.x, corner.y));
    }
    for (std::size_t i = 0; i < interior; ++i) {
        const double x = random.real(1.0, 199.0);
        const double y = random.real(1.0, 199.0);
        points.emplace_back(x, y, height(x, y));
    }
    return buildFromPoints(std::move(points));
}

// Adds up what it is told, block by block, and checks the blocks.
class Tally final : public OverlayVisitor {
  public:
    explicit Tally(std::size_t blocks) : areas_(blocks, 0.0), counts_(blocks, 0) {}

    void piece(std::size_t block, const OverlayPiece& piece) override
    {
        ASSERT_LT(block, areas_.size());
        areas_[block] += piece.area;
        ++counts_[block];
        if (piece.zTop != piece.zBase) {
            differing_ = true;
        }
        if (!(piece.area > 0.0)) {
            emptyPiece_ = true;
        }
    }

    [[nodiscard]] double area() const
    {
        double total = 0.0;
        for (const double a : areas_) {
            total += a;
        }
        return total;
    }
    [[nodiscard]] std::size_t pieces() const
    {
        std::size_t total = 0;
        for (const std::size_t c : counts_) {
            total += c;
        }
        return total;
    }
    [[nodiscard]] bool differing() const { return differing_; }
    [[nodiscard]] bool emptyPiece() const { return emptyPiece_; }

  private:
    std::vector<double> areas_;
    std::vector<std::size_t> counts_;
    bool differing_ = false;
    bool emptyPiece_ = false;
};

} // namespace

// ---- the region ---------------------------------------------------------------------------

TEST(OverlayRegion, ASquareIsTwoTrianglesOfItsArea)
{
    const auto region = OverlayRegion::fromPolygon(square(0, 0, 10));
    ASSERT_TRUE(region.ok()) << region.error().describe();
    EXPECT_EQ(region->triangles().size(), 2u);
    EXPECT_NEAR(region->area(), 100.0, 1e-12);
    EXPECT_TRUE(region->contains(Point2(5, 5)));
    EXPECT_TRUE(region->contains(Point2(10, 5))); // the boundary counts
    EXPECT_FALSE(region->contains(Point2(11, 5)));
    for (const auto& triangle : region->triangles()) {
        EXPECT_GT(triangle.signedArea(), 0.0); // counter-clockwise
    }
}

TEST(OverlayRegion, AHoleIsTakenOutOfTheArea)
{
    // 10 x 10 less a 2 x 2 hole: 96.
    const auto region = OverlayRegion::fromPolygon(square(0, 0, 10), {square(4, 4, 2)});
    ASSERT_TRUE(region.ok()) << region.error().describe();
    EXPECT_NEAR(region->area(), 96.0, 1e-12);
    EXPECT_FALSE(region->contains(Point2(5, 5)));
    EXPECT_TRUE(region->contains(Point2(4, 5))); // on the hole's edge: the region's boundary
    EXPECT_TRUE(region->contains(Point2(2, 2)));
}

TEST(OverlayRegion, ARingDrawnBackToItsStartIsTheSameRing)
{
    Polyline2 closedByHand = square(0, 0, 10);
    closedByHand.vertices.push_back(closedByHand.vertices.front());
    const auto region = OverlayRegion::fromPolygon(closedByHand);
    ASSERT_TRUE(region.ok()) << region.error().describe();
    EXPECT_NEAR(region->area(), 100.0, 1e-12);
}

TEST(OverlayRegion, ANonConvexPolygonIsTiledExactly)
{
    // An L: 40 x 20 and 20 x 20 more, 1200.
    const auto region = OverlayRegion::fromPolygon(
        ring({Point2(10, 10), Point2(50, 10), Point2(50, 30), Point2(30, 30), Point2(30, 50),
              Point2(10, 50)}));
    ASSERT_TRUE(region.ok()) << region.error().describe();
    EXPECT_NEAR(region->area(), 1200.0, 1e-10);
    EXPECT_FALSE(region->contains(Point2(40, 40))); // in the notch
}

TEST(OverlayRegion, ARingThatCrossesItselfOrHasNoAreaIsRefused)
{
    const auto bowTie =
        OverlayRegion::fromPolygon(ring({Point2(0, 0), Point2(10, 10), Point2(10, 0), Point2(0, 10)}));
    ASSERT_FALSE(bowTie.ok());
    EXPECT_EQ(bowTie.error().code, ErrorCode::InvalidGeometry);
    const auto two = OverlayRegion::fromPolygon(ring({Point2(0, 0), Point2(10, 0)}));
    ASSERT_FALSE(two.ok());
    EXPECT_EQ(two.error().code, ErrorCode::InvalidGeometry);
    const auto line =
        OverlayRegion::fromPolygon(ring({Point2(0, 0), Point2(5, 0), Point2(10, 0)}));
    ASSERT_FALSE(line.ok());
    EXPECT_EQ(line.error().code, ErrorCode::InvalidGeometry);
}

TEST(OverlayRegion, AtALevelItIsAFlatSurfaceOfItsArea)
{
    const auto region = OverlayRegion::fromPolygon(square(0, 0, 10));
    ASSERT_TRUE(region.ok());
    const auto flat = region->atLevel(12.5);
    ASSERT_TRUE(flat.ok()) << flat.error().describe();
    EXPECT_NEAR(flat->planArea(), 100.0, 1e-12);
    for (const Point3& v : flat->vertices()) {
        EXPECT_EQ(v.z, 12.5);
    }
    EXPECT_FALSE(region->atLevel(std::nan("")).ok());
}

// ---- the pieces -------------------------------------------------------------------------------

TEST(Overlay, ASurfaceWithItselfIsTiledByPiecesOfNoDifference)
{
    // Every piece is a triangle of both, read at its own corners: the same
    // height from both sides, bit for bit, and the pieces cover the surface's
    // plan area, 120^2.
    const TinSurface surface = planeSurface(Plane{0.2, -0.15, 30.0}, 120.0, 300, 7);
    Tally tally(overlayBlockCount(surface));
    ASSERT_TRUE(visitOverlay(surface, surface, tally).ok());
    EXPECT_NEAR(tally.area(), 14400.0, 1e-8);
    EXPECT_FALSE(tally.differing());
    EXPECT_FALSE(tally.emptyPiece());
}

TEST(Overlay, ARegionCutsThePiecesToItsOwnArea)
{
    // Two irregular planes over [0, 100]^2 and a triangle inside both whose
    // edges cross their triangles anywhere: its shoelace area, 1/2 |(b - a) x
    // (c - a)| = 1/2 |67.6 x 67.6 - 14.7 x 26.7| = 2088.635.
    const TinSurface base = planeSurface(Plane{0.01, 0.02, 5.0}, 100.0, 200, 3);
    const TinSurface top = planeSurface(Plane{-0.01, 0.0, 6.0}, 100.0, 150, 4);
    const auto region = OverlayRegion::fromPolygon(
        ring({Point2(13.7, 21.2), Point2(81.3, 35.9), Point2(40.4, 88.8)}));
    ASSERT_TRUE(region.ok());
    const double expected =
        0.5 * std::abs((81.3 - 13.7) * (88.8 - 21.2) - (35.9 - 21.2) * (40.4 - 13.7));
    Tally tally(overlayBlockCount(base));
    ASSERT_TRUE(visitOverlay(base, top, tally, &*region).ok());
    EXPECT_NEAR(tally.area(), expected, 1e-8);
    EXPECT_NEAR(region->area(), expected, 1e-9);
}

TEST(Overlay, SurfacesThatDoNotMeetVisitNothing)
{
    const TinSurface west = planeSurface(Plane{0, 0, 1}, 10.0, 5, 1);
    const TinSurface east = planeSurface(Plane{0, 0, 2}, 10.0, 5, 2, 50.0, 0.0);
    Tally tally(overlayBlockCount(west));
    ASSERT_TRUE(visitOverlay(west, east, tally).ok());
    EXPECT_EQ(tally.pieces(), 0u);
}

TEST(Overlay, AnEmptySurfaceIsRefused)
{
    const TinSurface surface = planeSurface(Plane{0, 0, 1}, 10.0, 5, 1);
    Tally tally(1);
    const auto status = visitOverlay(surface, TinSurface{}, tally);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
}

// ---- the difference surface --------------------------------------------------------------------

TEST(DifferenceSurface, TwoParallelPlanesDifferByTheirSeparationEverywhere)
{
    // The top is the base plane raised 1 m, triangulated from other points:
    // the difference is 1 at every vertex, over the 100 x 100 they share.
    const Plane ground{0.05, 0.02, 10.0};
    const TinSurface base = planeSurface(ground, 100.0, 120, 21);
    const TinSurface top = planeSurface(Plane{0.05, 0.02, 11.0}, 100.0, 90, 22);
    const auto difference = differenceSurface(base, top);
    ASSERT_TRUE(difference.ok()) << difference.error().describe();
    EXPECT_FALSE(difference->retriangulated);
    for (const Point3& v : difference->surface.vertices()) {
        // Heights near 10 to 17, so each is read to a few ulps of 16.
        EXPECT_NEAR(v.z, 1.0, 1e-12) << v.x << "," << v.y;
    }
    EXPECT_NEAR(difference->surface.planArea(), 10000.0, 1e-8);
    const auto volume = volumeToDatum(difference->surface, 0.0);
    ASSERT_TRUE(volume.ok());
    EXPECT_NEAR(volume->above, 10000.0, 1e-8);
    EXPECT_EQ(volume->below, 0.0);
}

TEST(DifferenceSurface, ItIntegratesToWhatCompareSurfacesMeasures)
{
    // The two methods cut the same pieces; the surface carries one height
    // per corner where the comparison reads each piece's own, which differ
    // in the last bits. Volumes of order 1e4 over ~2e4 pieces: 1e-6.
    const TinSurface base = rolling(31, 1500, 0.0);
    const TinSurface top = rolling(37, 1200, 0.3);
    const auto comparison = compareSurfaces(base, top);
    ASSERT_TRUE(comparison.ok());
    ASSERT_GT(comparison->cut, 1.0); // both signs: the two cross
    ASSERT_GT(comparison->fill, 1.0);
    const auto difference = differenceSurface(base, top);
    ASSERT_TRUE(difference.ok()) << difference.error().describe();
    const auto volume = volumeToDatum(difference->surface, 0.0);
    ASSERT_TRUE(volume.ok());
    EXPECT_NEAR(volume->above, comparison->fill, 1e-6);
    EXPECT_NEAR(volume->below, comparison->cut, 1e-6);
    EXPECT_NEAR(volume->net, comparison->net, 1e-6);
    EXPECT_NEAR(difference->surface.planArea(), comparison->planArea, 1e-6);
}

TEST(DifferenceSurface, IsTheSameSurfaceAtAnyThreadCount)
{
    const TinSurface base = rolling(41, 2500, 0.0);
    const TinSurface top = rolling(43, 1800, 0.2);
    katana::core::TaskPool serial(0);
    OverlaySurfaceOptions one;
    one.pool = &serial;
    const auto first = differenceSurface(base, top, one);
    ASSERT_TRUE(first.ok()) << first.error().describe();
    for (const std::size_t workers : {1u, 3u, 7u}) {
        katana::core::TaskPool pool(workers);
        OverlaySurfaceOptions many;
        many.pool = &pool;
        const auto again = differenceSurface(base, top, many);
        ASSERT_TRUE(again.ok());
        EXPECT_TRUE(again->surface == first->surface) << workers << " workers";
    }
}

TEST(DifferenceSurface, MadeThroughTheTriangulationItMeasuresTheSame)
{
    // The fallback path, asked for: the same vertices and every piece edge
    // constrained, so the same surface up to the triangulator's merges.
    const TinSurface base = rolling(51, 400, 0.0);
    const TinSurface top = rolling(53, 300, 0.25);
    const auto direct = differenceSurface(base, top);
    OverlaySurfaceOptions options;
    options.retriangulate = true;
    const auto rebuilt = differenceSurface(base, top, options);
    ASSERT_TRUE(direct.ok()) << direct.error().describe();
    ASSERT_TRUE(rebuilt.ok()) << rebuilt.error().describe();
    EXPECT_FALSE(direct->retriangulated);
    EXPECT_TRUE(rebuilt->retriangulated);
    const auto a = volumeToDatum(direct->surface, 0.0);
    const auto b = volumeToDatum(rebuilt->surface, 0.0);
    ASSERT_TRUE(a.ok() && b.ok());
    EXPECT_NEAR(a->above, b->above, 1e-6);
    EXPECT_NEAR(a->below, b->below, 1e-6);
    EXPECT_NEAR(direct->surface.planArea(), rebuilt->surface.planArea(), 1e-6);
}

TEST(DifferenceSurface, SurfacesWithNoGroundInCommonOrNoneAtAllAreRefused)
{
    const TinSurface west = planeSurface(Plane{0, 0, 1}, 10.0, 5, 1);
    const TinSurface east = planeSurface(Plane{0, 0, 2}, 10.0, 5, 2, 50.0, 0.0);
    const auto apart = differenceSurface(west, east);
    ASSERT_FALSE(apart.ok());
    EXPECT_EQ(apart.error().code, ErrorCode::InvalidGeometry);
    const auto empty = differenceSurface(west, TinSurface{});
    ASSERT_FALSE(empty.ok());
    EXPECT_EQ(empty.error().code, ErrorCode::InvalidArgument);
}

// ---- the envelope -----------------------------------------------------------------------------

TEST(EnvelopeSurface, TheUpperEnvelopeOfTwoCrossingPlanesIsTheHigherOfThem)
{
    // z = 0 and z = x - 50 over [0, 100]^2, each from its four corners: they
    // cross along x = 50.
    Random random;
    const TinSurface level = buildFromPoints(planePoints(Plane{0, 0, 0}, 100.0, 0, random));
    const TinSurface tilted = buildFromPoints(planePoints(Plane{1, 0, -50}, 100.0, 0, random));
    const auto upper = envelopeSurface(level, tilted, Envelope::Upper);
    ASSERT_TRUE(upper.ok()) << upper.error().describe();
    // max(0, x - 50): 0 west of x = 50, x - 50 east of it. Its volume is
    // 100 (the y extent) x 50^2 / 2 = 125 000, all above 0.
    const auto volume = volumeToDatum(upper->surface, 0.0);
    ASSERT_TRUE(volume.ok());
    EXPECT_NEAR(volume->above, 125000.0, 1e-7);
    // Where they meet the height is read along an edge of the overlay, to an
    // ulp or so of 50 either side of 0.
    EXPECT_NEAR(volume->below, 0.0, 1e-9);
    EXPECT_NEAR(*upper->surface.elevationAt(Point2(75, 20)), 25.0, 1e-12);
    EXPECT_NEAR(*upper->surface.elevationAt(Point2(25, 20)), 0.0, 1e-12);
    EXPECT_NEAR(upper->surface.planArea(), 10000.0, 1e-8);
}

TEST(EnvelopeSurface, TheLowerEnvelopeOfTwoCrossingPlanesIsTheLowerOfThem)
{
    Random random;
    const TinSurface level = buildFromPoints(planePoints(Plane{0, 0, 0}, 100.0, 0, random));
    const TinSurface tilted = buildFromPoints(planePoints(Plane{1, 0, -50}, 100.0, 0, random));
    const auto lower = envelopeSurface(level, tilted, Envelope::Lower);
    ASSERT_TRUE(lower.ok()) << lower.error().describe();
    // min(0, x - 50): the mirror of the upper one, 125 000 below 0.
    const auto volume = volumeToDatum(lower->surface, 0.0);
    ASSERT_TRUE(volume.ok());
    EXPECT_NEAR(volume->below, 125000.0, 1e-7);
    EXPECT_NEAR(volume->above, 0.0, 1e-9);
    EXPECT_NEAR(*lower->surface.elevationAt(Point2(25, 80)), -25.0, 1e-12);
}

TEST(EnvelopeSurface, UpperOverLowerIsTheMagnitudeOfTheDifferenceAndNeitherDipsBelowTheOther)
{
    // On rolling ground that crosses its lifted copy many times: upper minus
    // lower is |top - base|, so its fill is the comparison's cut plus fill,
    // and the upper envelope is nowhere below either surface.
    const TinSurface base = rolling(61, 800, 0.0);
    const TinSurface top = rolling(67, 700, 0.3);
    const auto comparison = compareSurfaces(base, top);
    ASSERT_TRUE(comparison.ok());
    const auto upper = envelopeSurface(base, top, Envelope::Upper);
    const auto lower = envelopeSurface(base, top, Envelope::Lower);
    ASSERT_TRUE(upper.ok()) << upper.error().describe();
    ASSERT_TRUE(lower.ok()) << lower.error().describe();
    const auto spread = compareSurfaces(lower->surface, upper->surface);
    ASSERT_TRUE(spread.ok());
    EXPECT_NEAR(spread->fill, comparison->fill + comparison->cut, 1e-6);
    EXPECT_NEAR(spread->cut, 0.0, 1e-6);
    for (const TinSurface* surface : {&base, &top}) {
        const auto above = compareSurfaces(upper->surface, *surface);
        ASSERT_TRUE(above.ok());
        EXPECT_NEAR(above->fill, 0.0, 1e-6); // nothing of either rises above it
    }
}

TEST(EnvelopeSurface, SurfacesWithNoGroundInCommonAreRefused)
{
    const TinSurface west = planeSurface(Plane{0, 0, 1}, 10.0, 5, 1);
    const TinSurface east = planeSurface(Plane{0, 0, 2}, 10.0, 5, 2, 50.0, 0.0);
    const auto apart = envelopeSurface(west, east, Envelope::Upper);
    ASSERT_FALSE(apart.ok());
    EXPECT_EQ(apart.error().code, ErrorCode::InvalidGeometry);
}
