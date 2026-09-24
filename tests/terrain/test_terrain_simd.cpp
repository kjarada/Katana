// The terrain kernels (src/katana_terrain/simd) against their scalar
// references, through the public functions that call them: each call is made
// at the scalar level and at AVX2, and the answers must be the same bits.
//
// The whole suite also runs a second and third time as simd_scalar.terrain and
// simd_avx2.terrain (tests/terrain/CMakeLists.txt), so every hand-worked
// contour and locate test here and in the other files holds on both paths.

#include <gtest/gtest.h>

#include <bit>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <vector>

#include "katana/core/task_pool.hpp"
#include "katana/terrain/contours.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "simd_levels.hpp"
#include "terrain_test_support.hpp"

using namespace katana::terrain;
using namespace katana::terrain::testing;
using katana::core::SimdLevel;
using katana::test::atSimdLevel;
using katana::test::Random;

namespace {

bool sameBits(double a, double b)
{
    return std::bit_cast<std::uint64_t>(a) == std::bit_cast<std::uint64_t>(b);
}

void expectSameContours(const std::vector<Contour>& scalar, const std::vector<Contour>& avx2)
{
    ASSERT_EQ(scalar.size(), avx2.size());
    for (std::size_t i = 0; i < scalar.size(); ++i) {
        const Contour& a = scalar[i];
        const Contour& b = avx2[i];
        ASSERT_TRUE(sameBits(a.elevation, b.elevation)) << "contour " << i;
        ASSERT_EQ(a.major, b.major) << "contour " << i;
        ASSERT_EQ(a.line.closed, b.line.closed) << "contour " << i;
        ASSERT_EQ(a.line.vertices.size(), b.line.vertices.size()) << "contour " << i;
        for (std::size_t v = 0; v < a.line.vertices.size(); ++v) {
            ASSERT_TRUE(sameBits(a.line.vertices[v].x, b.line.vertices[v].x) &&
                        sameBits(a.line.vertices[v].y, b.line.vertices[v].y))
                << "contour " << i << " vertex " << v;
        }
    }
}

// Rolling ground whose elevations are rounded to 0.05 m, so that with the
// intervals below many vertices sit exactly on a level: the case where the
// settling loops of firstLevelAbove decide the answer.
TinSurface roundedGround(int pointCount, std::uint32_t seed)
{
    Random random(seed);
    std::vector<Point3> points;
    for (int i = 0; i < pointCount; ++i) {
        const double x = random.real(0.0, 120.0);
        const double y = random.real(0.0, 80.0);
        const double z = 7.0 + 2.0 * std::sin(x * 0.09) * std::cos(y * 0.06) + 0.01 * x;
        points.emplace_back(x, y, std::round(z * 20.0) / 20.0);
    }
    return buildFromPoints(std::move(points));
}

} // namespace

TEST(TerrainSimd, ContourLevelRangesAreTheScalarOnesOnEveryThreadCount)
{
    KATANA_REQUIRE_AVX2();
    const TinSurface surface = roundedGround(2500, 91);
    ASSERT_FALSE(surface.empty());
    // A negative, inexact base and intervals that do and do not divide 0.05:
    // the first estimate from the division is then off by one both ways.
    for (const double interval : {0.05, 0.1, 0.3, 1.0}) {
        for (const double base : {0.0, -0.37}) {
            SCOPED_TRACE(interval);
            SCOPED_TRACE(base);
            katana::core::TaskPool inlineOnly(0);
            const auto scalar = atSimdLevel(SimdLevel::Scalar, [&] {
                return contours(surface, interval, base, 5, &inlineOnly);
            });
            ASSERT_TRUE(scalar.ok()) << scalar.error().describe();
            ASSERT_GT(scalar.value().size(), 10u);
            for (const std::size_t workers : {0u, 3u}) {
                katana::core::TaskPool pool(workers);
                const auto avx2 = atSimdLevel(SimdLevel::Avx2, [&] {
                    return contours(surface, interval, base, 5, &pool);
                });
                ASSERT_TRUE(avx2.ok()) << avx2.error().describe();
                expectSameContours(scalar.value(), avx2.value());
            }
        }
    }
}

TEST(TerrainSimd, ContoursOfSurfacesTooSmallToFillTheKernelsFourLanes)
{
    KATANA_REQUIRE_AVX2();
    // One to seven triangles: every partial last step of the kernel, where
    // lanes past the end repeat the last triangle and must be dropped.
    Random random(5);
    std::vector<Point3> points{Point3(0, 0, 0.0), Point3(10, 0, 1.0), Point3(0, 10, 2.0)};
    for (int extra = 0; extra < 6; ++extra) {
        const TinSurface surface = buildFromPoints(points);
        ASSERT_FALSE(surface.empty());
        SCOPED_TRACE(surface.triangleCount());
        const auto scalar =
            atSimdLevel(SimdLevel::Scalar, [&] { return contours(surface, 0.25, 0.0, 4); });
        const auto avx2 =
            atSimdLevel(SimdLevel::Avx2, [&] { return contours(surface, 0.25, 0.0, 4); });
        ASSERT_TRUE(scalar.ok() && avx2.ok());
        expectSameContours(scalar.value(), avx2.value());
        points.emplace_back(random.real(-5.0, 15.0), random.real(-5.0, 15.0),
                            std::round(random.real(0.0, 3.0) * 4.0) / 4.0);
    }
}

TEST(TerrainSimd, LocateSkipsASliverTheSignTestsAcceptAndTakesTheNextCandidate)
{
    // A fan of ten triangles round the hub H = (0, 0), and in place of the
    // first one three: the sliver (H, R0, S), where S sits 5e-9 m above the
    // midpoint of H-R0, then (H, S, R1) and (S, R0, R1). The probe is the
    // midpoint of H and S, on the edge the sliver shares with (H, S, R1): both
    // pass the sign tests (the edge function along H-S is exactly zero, the
    // same two products subtracted), but the sliver's height, 5e-9 m over a
    // 10 m edge, is below kGeometric, so locate() must pass over it - it comes
    // first in the cell, being triangle 0 - and take triangle 1. All twelve
    // triangles touch the hub, so the probe's cell lists every one of them,
    // enough for the kernel path. With z(H) = 0 and z(S) = 4 the elevation at
    // the midpoint is 2, and the weight of R1 is zero.
    std::vector<Point3> vertices{Point3(0, 0, 0)}; // H
    for (int i = 0; i < 10; ++i) {
        const double angle = i * 0.2 * katana::math::kPi;
        vertices.emplace_back(10.0 * std::cos(angle), 10.0 * std::sin(angle), 1.0 + i);
    }
    vertices[1] = Point3(10.0, 0.0, 1.0); // R0 exactly on the axis
    const Point3 s(5.0, 5e-9, 4.0);
    vertices.push_back(s); // vertex 11
    std::vector<TinTriangle> triangles{TinTriangle{0, 1, 11}, TinTriangle{0, 11, 2},
                                       TinTriangle{11, 1, 2}};
    for (std::uint32_t i = 2; i <= 10; ++i) {
        triangles.push_back(TinTriangle{0, i, i == 10 ? 1u : i + 1});
    }
    auto created = TinSurface::create(vertices, triangles, {});
    ASSERT_TRUE(created.ok()) << created.error().describe();
    const Point2 probe(0.5 * s.x, 0.5 * s.y);

    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
            continue;
        }
        SCOPED_TRACE(katana::core::toString(level));
        const auto location = atSimdLevel(level, [&] { return created->locate(probe); });
        ASSERT_TRUE(location.has_value());
        EXPECT_EQ(location->triangle, 1u);
        EXPECT_TRUE(location->interior);
        EXPECT_EQ(location->weights[2], 0.0);
        const auto elevation = atSimdLevel(level, [&] { return created->elevationAt(probe); });
        ASSERT_TRUE(elevation.has_value());
        EXPECT_NEAR(*elevation, 2.0, 1e-12);
    }
}

TEST(TerrainSimd, LocateAndElevationsAreTheScalarOnesToTheBit)
{
    KATANA_REQUIRE_AVX2();
    const TinSurface surface = roundedGround(3000, 17);
    ASSERT_FALSE(surface.empty());
    // Random probes, half of them off the surface; every vertex (three or
    // more edge functions exactly zero); every edge midpoint (one zero, and
    // the next candidate decides); and points just outside the rim, which
    // pass 1 rejects and pass 2 settles.
    Random random(8);
    std::vector<Point2> probes;
    for (int i = 0; i < 4000; ++i) {
        probes.emplace_back(random.real(-20.0, 140.0), random.real(-20.0, 100.0));
    }
    for (const Point3& v : surface.vertices()) {
        probes.emplace_back(v.x, v.y);
        probes.emplace_back(v.x + 5e-8, v.y - 5e-8);
    }
    for (const TinTriangle& tri : surface.triangles()) {
        const Point3& a = surface.vertices()[tri[0]];
        const Point3& b = surface.vertices()[tri[1]];
        probes.emplace_back(0.5 * (a.x + b.x), 0.5 * (a.y + b.y));
    }

    std::size_t found = 0;
    for (const Point2& p : probes) {
        const auto scalar = atSimdLevel(SimdLevel::Scalar, [&] { return surface.locate(p); });
        const auto avx2 = atSimdLevel(SimdLevel::Avx2, [&] { return surface.locate(p); });
        ASSERT_EQ(scalar.has_value(), avx2.has_value()) << p;
        if (!scalar) {
            continue;
        }
        ++found;
        ASSERT_EQ(scalar->triangle, avx2->triangle) << p;
        ASSERT_EQ(scalar->interior, avx2->interior) << p;
        for (std::size_t k = 0; k < 3; ++k) {
            ASSERT_TRUE(sameBits(scalar->weights[k], avx2->weights[k])) << p << " weight " << k;
        }
    }
    EXPECT_GT(found, 3000u);

    const auto scalar = atSimdLevel(SimdLevel::Scalar, [&] { return surface.elevationsAt(probes); });
    const auto avx2 = atSimdLevel(SimdLevel::Avx2, [&] { return surface.elevationsAt(probes); });
    ASSERT_EQ(scalar.size(), avx2.size());
    for (std::size_t i = 0; i < scalar.size(); ++i) {
        ASSERT_EQ(scalar[i].has_value(), avx2[i].has_value()) << "probe " << i;
        if (scalar[i]) {
            ASSERT_TRUE(sameBits(*scalar[i], *avx2[i])) << "probe " << i;
        }
    }
}
