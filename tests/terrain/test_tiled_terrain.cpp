#include <gtest/gtest.h>

#include <cmath>
#include <cstddef>
#include <limits>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include "katana/terrain/tiled_terrain.hpp"
#include "terrain_test_support.hpp"

using namespace katana::terrain;
using namespace katana::terrain::testing;
using katana::core::ErrorCode;
using katana::test::Random;

namespace {

// Rough but smooth ground over [0, extent]^2, plus the four corners so that the
// data extent is exactly the square and the tile grid is predictable.
std::vector<Point3> roughGround(double extent, int count, Random& random)
{
    std::vector<Point3> points;
    const auto elevation = [](double x, double y) {
        return 60.0 + 8.0 * std::sin(0.02 * x) * std::cos(0.017 * y) + 0.01 * x;
    };
    for (const Point2 corner : {Point2(0, 0), Point2(extent, 0), Point2(extent, extent),
                                Point2(0, extent)}) {
        points.emplace_back(corner.x, corner.y, elevation(corner.x, corner.y));
    }
    for (int i = 0; i < count; ++i) {
        const double x = random.real(0.0, extent);
        const double y = random.real(0.0, extent);
        points.emplace_back(x, y, elevation(x, y));
    }
    return points;
}

TinInput inputFrom(std::vector<Point3> points)
{
    TinInput input;
    input.points = std::move(points);
    return input;
}

} // namespace

// ---- the tile grid ------------------------------------------------------------------------

TEST(TiledTerrainGrid, GeometryOfTheGridFollowsTheDataExtent)
{
    Random random;
    TiledTerrainOptions options;
    options.tileSize = 25.0;
    options.bufferWidth = 5.0;
    auto created = TiledTerrain::create(inputFrom(roughGround(100.0, 200, random)), options);
    ASSERT_TRUE(created.ok()) << created.error().describe();
    const TiledTerrain& terrain = created.value();

    // ceil(100/25) = 4 tiles each way.
    EXPECT_EQ(terrain.columns(), 4u);
    EXPECT_EQ(terrain.rows(), 4u);
    EXPECT_EQ(terrain.tileCount(), 16u);
    EXPECT_EQ(terrain.pointCount(), 204u);
    EXPECT_EQ(terrain.bounds().min, Point2(0, 0));
    EXPECT_EQ(terrain.bounds().max, Point2(100, 100));

    EXPECT_EQ(terrain.tileBounds(0).min, Point2(0, 0));
    EXPECT_EQ(terrain.tileBounds(0).max, Point2(25, 25));
    // Tile 9 = row 2, column 1.
    EXPECT_EQ(terrain.tileBounds(9).min, Point2(25, 50));
    EXPECT_EQ(terrain.tileBounds(9).max, Point2(50, 75));
    EXPECT_EQ(terrain.tileBounds(15).min, Point2(75, 75));
    EXPECT_EQ(terrain.tileBounds(15).max, Point2(100, 100));
    EXPECT_EQ(terrain.bufferedTileBounds(0).min, Point2(-5, -5));
    EXPECT_EQ(terrain.bufferedTileBounds(0).max, Point2(30, 30));

    EXPECT_EQ(terrain.tileAt(Point2(10, 10)), 0u);
    EXPECT_EQ(terrain.tileAt(Point2(30, 60)), 9u); // column 1, row 2
    EXPECT_EQ(terrain.tileAt(Point2(0, 0)), 0u);
    EXPECT_EQ(terrain.tileAt(Point2(100, 100)), 15u); // the far corner belongs to the last tile
    EXPECT_FALSE(terrain.tileAt(Point2(-0.5, 50)).has_value());
    EXPECT_FALSE(terrain.tileAt(Point2(50, 100.5)).has_value());
    EXPECT_FALSE(terrain.tileAt(Point2(std::numeric_limits<double>::quiet_NaN(), 5)).has_value());

    EXPECT_THROW((void)terrain.tileBounds(16), std::out_of_range);
    EXPECT_THROW((void)terrain.bufferedTileBounds(16), std::out_of_range);
}

// ---- agreement with a monolithic TIN --------------------------------------------------------

TEST(TiledTerrain, ATileThatSeesEveryPointReproducesTheMonolithicTinExactly)
{
    // With a buffer wider than the data, every tile triangulates the whole point
    // set in input order, which is precisely what buildTin() was given directly.
    // The surfaces must therefore be bitwise identical, arrays and all.
    Random random;
    const std::vector<Point3> points = roughGround(100.0, 400, random);
    const TinSurface monolithic = buildFromPoints(points);
    ASSERT_FALSE(monolithic.empty());

    TiledTerrainOptions options;
    options.tileSize = 25.0;
    options.bufferWidth = 500.0;
    auto created = TiledTerrain::create(inputFrom(points), options);
    ASSERT_TRUE(created.ok()) << created.error().describe();
    TiledTerrain& terrain = created.value();
    ASSERT_EQ(terrain.tileCount(), 16u);
    ASSERT_TRUE(terrain.buildAll().ok());

    for (std::size_t tile = 0; tile < terrain.tileCount(); ++tile) {
        ASSERT_TRUE(terrain.isBuilt(tile));
        ASSERT_NE(terrain.tileSurface(tile), nullptr);
        EXPECT_TRUE(*terrain.tileSurface(tile) == monolithic) << "tile " << tile;
    }

    // And then every in-bounds answer is certified, because no circumcircle can
    // reach data the tile did not see.
    Random queries(31);
    for (int i = 0; i < 300; ++i) {
        const Point2 p(queries.real(0.0, 100.0), queries.real(0.0, 100.0));
        const auto sample = terrain.sampleAt(p);
        ASSERT_TRUE(sample.ok()) << sample.error().describe();
        const auto expected = monolithic.elevationAt(p);
        ASSERT_EQ(sample.value().elevation.has_value(), expected.has_value()) << p;
        if (expected) {
            EXPECT_DOUBLE_EQ(*sample.value().elevation, *expected) << p;
            EXPECT_TRUE(sample.value().certified) << p;
        }
    }
}

TEST(TiledTerrain, CertifiedSamplesEqualTheMonolithicAnswer)
{
    // The contract of TileSample::certified. A buffer of ~3 point spacings over
    // 100 m tiles leaves the interior certified and the hull-adjacent long thin
    // triangles uncertified; wherever the flag is set, the tiled answer must be
    // the monolithic one.
    Random random;
    const std::vector<Point3> points = roughGround(400.0, 3000, random);
    const TinSurface monolithic = buildFromPoints(points);
    ASSERT_FALSE(monolithic.empty());

    TiledTerrainOptions options;
    options.tileSize = 100.0;
    options.bufferWidth = 20.0;
    auto created = TiledTerrain::create(inputFrom(points), options);
    ASSERT_TRUE(created.ok()) << created.error().describe();
    TiledTerrain& terrain = created.value();
    ASSERT_EQ(terrain.tileCount(), 16u);
    ASSERT_TRUE(terrain.buildAll().ok());

    // The same grid also goes to a terrain with no buffer at all, whose tiles
    // cannot see across their own borders.
    TiledTerrainOptions unbuffered = options;
    unbuffered.bufferWidth = 0.0;
    auto bare = TiledTerrain::create(inputFrom(points), unbuffered);
    ASSERT_TRUE(bare.ok()) << bare.error().describe();
    ASSERT_TRUE(bare.value().buildAll().ok());

    std::size_t certified = 0;
    std::size_t certifiedUnbuffered = 0;
    std::size_t checked = 0;
    for (int row = 0; row < 40; ++row) {
        for (int column = 0; column < 40; ++column) {
            const Point2 p(10.0 * static_cast<double>(column) + 5.0,
                           10.0 * static_cast<double>(row) + 5.0);
            const auto expected = monolithic.elevationAt(p);
            const auto sample = terrain.sampleAt(p);
            ASSERT_TRUE(sample.ok()) << sample.error().describe();
            ++checked;
            if (sample.value().certified && sample.value().elevation) {
                ++certified;
                ASSERT_TRUE(expected.has_value()) << p;
                // Same three vertices, but reached through a different triangle
                // rotation, so only the rounding of the barycentric weights
                // differs: ~1e-13 on a 400 m extent with 16 m of relief.
                EXPECT_NEAR(*sample.value().elevation, *expected, 1e-9) << p;
            }
            const auto plain = bare.value().sampleAt(p);
            ASSERT_TRUE(plain.ok()) << plain.error().describe();
            if (plain.value().certified && plain.value().elevation) {
                ++certifiedUnbuffered;
                ASSERT_TRUE(expected.has_value()) << p;
                EXPECT_NEAR(*plain.value().elevation, *expected, 1e-9) << p;
            }
        }
    }
    EXPECT_EQ(checked, 1600u);
    // Not vacuous in either direction. With a buffer of ~3 point spacings nearly
    // every interior query certifies. With no buffer a query within one
    // circumradius of an internal tile border cannot: its triangle's circumcircle
    // has to reach across a border the tile never looked past. Three internal
    // borders each way cross this grid, so the loss runs into the hundreds.
    EXPECT_GT(certified, 1200u);
    EXPECT_LT(certifiedUnbuffered, certified);
    EXPECT_LT(certifiedUnbuffered, 1300u);
}

TEST(TiledTerrain, APlaneIsReproducedExactlyByEveryTileCertifiedOrNot)
{
    // Any triangulation of points on a plane interpolates that plane, so tiling
    // cannot change the answer even where the certificate cannot be given.
    Random random;
    const Plane plane{0.03, -0.045, 210.0};
    const std::vector<Point3> points = planePoints(plane, 300.0, 1200, random);

    TiledTerrainOptions options;
    options.tileSize = 75.0;
    options.bufferWidth = 0.0; // deliberately none: tiles share no points at all
    auto created = TiledTerrain::create(inputFrom(points), options);
    ASSERT_TRUE(created.ok()) << created.error().describe();
    TiledTerrain& terrain = created.value();
    ASSERT_EQ(terrain.tileCount(), 16u);
    ASSERT_TRUE(terrain.buildAll().ok());

    Random queries(41);
    std::size_t answered = 0;
    for (int i = 0; i < 1500; ++i) {
        const Point2 p(queries.real(0.0, 300.0), queries.real(0.0, 300.0));
        const auto elevation = terrain.elevationAt(p);
        ASSERT_TRUE(elevation.ok()) << elevation.error().describe();
        if (!elevation.value()) {
            continue; // outside this tile's own convex hull
        }
        ++answered;
        // Linear interpolation of a linear function: only the rounding of the
        // barycentric weights, on elevations around 210 m.
        EXPECT_NEAR(*elevation.value(), plane.at(p.x, p.y), 1e-9) << p;
    }
    EXPECT_GT(answered, 1000u);
}

TEST(TiledTerrain, ComputeTileIsAPureFunctionOfTheTile)
{
    // computeTile() is const and must not depend on what has been built before,
    // so repeating it - and doing it after the tile was stored and evicted - has
    // to give a bitwise identical surface (PLAN Rule 7).
    Random random;
    TiledTerrainOptions options;
    options.tileSize = 60.0;
    options.bufferWidth = 10.0;
    auto created = TiledTerrain::create(inputFrom(roughGround(180.0, 600, random)), options);
    ASSERT_TRUE(created.ok()) << created.error().describe();
    TiledTerrain& terrain = created.value();
    ASSERT_EQ(terrain.tileCount(), 9u);

    for (std::size_t tile = 0; tile < terrain.tileCount(); ++tile) {
        const auto first = terrain.computeTile(tile);
        ASSERT_TRUE(first.ok()) << first.error().describe();
        const auto second = terrain.computeTile(tile);
        ASSERT_TRUE(second.ok()) << second.error().describe();
        EXPECT_TRUE(first.value() == second.value()) << "tile " << tile;

        ASSERT_TRUE(terrain.buildTile(tile).ok());
        ASSERT_NE(terrain.tileSurface(tile), nullptr);
        EXPECT_TRUE(*terrain.tileSurface(tile) == first.value()) << "tile " << tile;
        EXPECT_EQ(terrain.tileSurface(tile)->planArea(), first.value().planArea()); // bitwise
    }
}

// ---- building, eviction and the state a query needs -----------------------------------------

TEST(TiledTerrain, QueriesNeverBuildAndSayWhenTheTileIsMissing)
{
    Random random;
    TiledTerrainOptions options;
    options.tileSize = 50.0;
    options.bufferWidth = 8.0;
    auto created = TiledTerrain::create(inputFrom(roughGround(100.0, 300, random)), options);
    ASSERT_TRUE(created.ok()) << created.error().describe();
    TiledTerrain& terrain = created.value();
    ASSERT_EQ(terrain.tileCount(), 4u);

    const Point2 inside(25.0, 25.0);
    ASSERT_EQ(terrain.tileAt(inside), 0u);
    EXPECT_FALSE(terrain.isBuilt(0));
    EXPECT_EQ(terrain.tileSurface(0), nullptr);
    EXPECT_EQ(terrain.sampleAt(inside).error().code, ErrorCode::InvalidState);
    EXPECT_EQ(terrain.elevationAt(inside).error().code, ErrorCode::InvalidState);

    ASSERT_TRUE(terrain.ensureTileAt(inside).ok());
    EXPECT_TRUE(terrain.isBuilt(0));
    const auto sample = terrain.sampleAt(inside);
    ASSERT_TRUE(sample.ok()) << sample.error().describe();
    ASSERT_TRUE(sample.value().elevation.has_value());

    // Building again is a no-op, and the stored surface survives it.
    ASSERT_TRUE(terrain.buildTile(0).ok());
    EXPECT_TRUE(terrain.isBuilt(0));

    terrain.evictTile(0);
    EXPECT_FALSE(terrain.isBuilt(0));
    EXPECT_EQ(terrain.tileSurface(0), nullptr);
    EXPECT_EQ(terrain.sampleAt(inside).error().code, ErrorCode::InvalidState);
    // Rebuilding restores exactly the same surface.
    ASSERT_TRUE(terrain.buildTile(0).ok());
    const auto again = terrain.sampleAt(inside);
    ASSERT_TRUE(again.ok()) << again.error().describe();
    ASSERT_TRUE(again.value().elevation.has_value());
    EXPECT_EQ(*again.value().elevation, *sample.value().elevation); // bitwise

    // Outside the data extent there is certainly no surface, and saying so needs
    // no tile at all.
    const auto outside = terrain.sampleAt(Point2(-10.0, 50.0));
    ASSERT_TRUE(outside.ok()) << outside.error().describe();
    EXPECT_FALSE(outside.value().elevation.has_value());
    EXPECT_TRUE(outside.value().certified);
    EXPECT_TRUE(terrain.ensureTileAt(Point2(-10.0, 50.0)).ok()); // nothing to build

    EXPECT_EQ(terrain.buildTile(4).error().code, ErrorCode::InvalidArgument);
    terrain.evictTile(4); // out of range: ignored, not a crash
    EXPECT_FALSE(terrain.isBuilt(4));
    EXPECT_EQ(terrain.tileSurface(4), nullptr);
}

TEST(TiledTerrain, ATileWithTooFewPointsIsAnEmptySurfaceNotAnError)
{
    // Two clusters in opposite corners leave the middle tiles without enough
    // points to triangulate. That is terrain with a gap, not a failure.
    std::vector<Point3> points;
    Random random;
    for (int i = 0; i < 40; ++i) {
        points.emplace_back(random.real(0, 10), random.real(0, 10), 5.0);
        points.emplace_back(random.real(90, 100), random.real(90, 100), 9.0);
    }
    points.emplace_back(0, 0, 5.0);
    points.emplace_back(100, 100, 9.0);

    TiledTerrainOptions options;
    options.tileSize = 25.0;
    options.bufferWidth = 1.0;
    auto created = TiledTerrain::create(inputFrom(points), options);
    ASSERT_TRUE(created.ok()) << created.error().describe();
    TiledTerrain& terrain = created.value();
    ASSERT_EQ(terrain.tileCount(), 16u);
    ASSERT_TRUE(terrain.buildAll().ok());

    const std::size_t middle = 5; // row 1, column 1: [25, 50]^2, no points nearby
    ASSERT_TRUE(terrain.isBuilt(middle));
    ASSERT_NE(terrain.tileSurface(middle), nullptr);
    EXPECT_TRUE(terrain.tileSurface(middle)->empty());

    const auto sample = terrain.sampleAt(Point2(37.5, 37.5));
    ASSERT_TRUE(sample.ok()) << sample.error().describe();
    EXPECT_FALSE(sample.value().elevation.has_value());
    EXPECT_FALSE(sample.value().certified); // inside the extent, so nothing is proved

    const auto corner = terrain.sampleAt(Point2(5.0, 5.0));
    ASSERT_TRUE(corner.ok()) << corner.error().describe();
    ASSERT_TRUE(corner.value().elevation.has_value());
    EXPECT_DOUBLE_EQ(*corner.value().elevation, 5.0);
}

// ---- input validation -----------------------------------------------------------------------

TEST(TiledTerrainErrors, RejectsBadOptionsAndUnsupportedInput)
{
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double inf = std::numeric_limits<double>::infinity();
    Random random;
    const std::vector<Point3> points = roughGround(100.0, 50, random);

    TiledTerrainOptions good;
    good.tileSize = 25.0;
    good.bufferWidth = 5.0;

    for (const double bad : {0.0, -1.0, nan, inf}) {
        TiledTerrainOptions options = good;
        options.tileSize = bad;
        EXPECT_EQ(TiledTerrain::create(inputFrom(points), options).error().code,
                  ErrorCode::InvalidArgument);
    }
    for (const double bad : {-1.0, nan, inf}) {
        TiledTerrainOptions options = good;
        options.bufferWidth = bad;
        EXPECT_EQ(TiledTerrain::create(inputFrom(points), options).error().code,
                  ErrorCode::InvalidArgument);
    }

    // 100 m of extent at 0.1 m tiles is 1000 x 1000 tiles, past kMaxTileCount.
    TiledTerrainOptions tiny = good;
    tiny.tileSize = 0.1;
    EXPECT_EQ(TiledTerrain::create(inputFrom(points), tiny).error().code,
              ErrorCode::InvalidArgument);

    TinInput withBreakline = inputFrom(points);
    withBreakline.breaklines.push_back(Breakline{{Point3(0, 0, 0), Point3(50, 50, 1)}, false});
    EXPECT_EQ(TiledTerrain::create(withBreakline, good).error().code, ErrorCode::Unsupported);

    TinInput withBoundary = inputFrom(points);
    withBoundary.boundary =
        katana::geometry::Polyline2{{Point2(1, 1), Point2(90, 1), Point2(90, 90)}, true};
    EXPECT_EQ(TiledTerrain::create(withBoundary, good).error().code, ErrorCode::Unsupported);

    TinInput withHole = inputFrom(points);
    withHole.holes.push_back(
        katana::geometry::Polyline2{{Point2(40, 40), Point2(60, 40), Point2(60, 60)}, true});
    EXPECT_EQ(TiledTerrain::create(withHole, good).error().code, ErrorCode::Unsupported);

    TinInput tooFew;
    tooFew.points = {Point3(0, 0, 0), Point3(1, 0, 0)};
    EXPECT_EQ(TiledTerrain::create(tooFew, good).error().code, ErrorCode::TriangulationFailure);
    EXPECT_EQ(TiledTerrain::create(TinInput{}, good).error().code,
              ErrorCode::TriangulationFailure);

    TinInput nonFinite = inputFrom(points);
    nonFinite.points.emplace_back(10.0, nan, 3.0);
    const auto failure = TiledTerrain::create(nonFinite, good);
    ASSERT_FALSE(failure.ok());
    EXPECT_EQ(failure.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(failure.error().context.find("point="), std::string::npos);
}

TEST(TiledTerrainErrors, DegenerateExtentsStillGiveOneTile)
{
    // All points on one line: the extent has zero height, which must round up to a
    // single row rather than to zero rows. The tile itself cannot be triangulated.
    TinInput input;
    for (int i = 0; i < 10; ++i) {
        input.points.emplace_back(static_cast<double>(i), 0.0, 1.0);
    }
    TiledTerrainOptions options;
    options.tileSize = 4.0;
    options.bufferWidth = 0.0;
    auto created = TiledTerrain::create(input, options);
    ASSERT_TRUE(created.ok()) << created.error().describe();
    TiledTerrain& terrain = created.value();
    EXPECT_EQ(terrain.columns(), 3u); // ceil(9/4)
    EXPECT_EQ(terrain.rows(), 1u);
    EXPECT_EQ(terrain.tileCount(), 3u);
    ASSERT_TRUE(terrain.buildAll().ok());
    for (std::size_t tile = 0; tile < terrain.tileCount(); ++tile) {
        EXPECT_TRUE(terrain.tileSurface(tile)->empty()) << "tile " << tile;
    }
}
