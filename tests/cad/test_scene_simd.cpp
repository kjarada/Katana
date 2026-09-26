// The scene build's kernels (src/katana_cad/simd/scene_avx2.cpp on x86-64,
// scene_neon.cpp on 64-bit ARM) against its scalar code: every draw list
// compared element by element, to the bit.
//
// Each test builds the same scene twice, once at each level, in one process.
// ctest also runs this suite with KATANA_SIMD=scalar and =kernel or =neon
// (simd_scalar.cad, simd_avx2.cad or simd_neon.cad), which puts every other
// scene test on each path in turn.

#include <gtest/gtest.h>

#include <bit>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/scene.hpp"
#include "katana/core/cpu_features.hpp"
#include "simd_levels.hpp"

using katana::cad::SceneBuilder;
using katana::cad::SceneLayers;
using katana::cad::SceneMesh;
using katana::cad::SceneOptions;
using katana::cad::SceneSurface;
using katana::cad::SurfaceColoring;
using katana::cad::SurfaceStyle;
using katana::core::SimdLevel;
using katana::render::DrawList;
using katana::terrain::TinSurface;

namespace {

// Puts the process at a level for a scope and back afterwards.
using AtLevel = katana::test::ScopedSimdLevel;

template <typename T> void expectSameBits(const std::vector<T>& scalar, const std::vector<T>& kernel,
                                          const std::string& what)
{
    ASSERT_EQ(scalar.size(), kernel.size()) << what;
    for (std::size_t i = 0; i < scalar.size(); ++i) {
        if (std::memcmp(&scalar[i], &kernel[i], sizeof(T)) != 0) {
            std::string detail;
            if constexpr (sizeof(T) % sizeof(std::uint32_t) == 0) {
                for (std::size_t w = 0; w < sizeof(T) / sizeof(std::uint32_t); ++w) {
                    std::uint32_t l = 0;
                    std::uint32_t r = 0;
                    std::memcpy(&l, reinterpret_cast<const char*>(&scalar[i]) + 4 * w, 4);
                    std::memcpy(&r, reinterpret_cast<const char*>(&kernel[i]) + 4 * w, 4);
                    detail += " " + std::to_string(l) + "/" + std::to_string(r);
                }
            }
            ADD_FAILURE() << what << " differs first at element " << i << " of " << scalar.size()
                          << " (scalar/kernel words:" << detail << ")";
            return;
        }
    }
}

void expectSameList(const DrawList& scalar, const DrawList& kernel, const std::string& what)
{
    expectSameBits(scalar.positions, kernel.positions, what + " positions");
    expectSameBits(scalar.colors, kernel.colors, what + " colours");
    expectSameBits(scalar.triangles, kernel.triangles, what + " triangles");
    expectSameBits(scalar.lines, kernel.lines, what + " lines");
    expectSameBits(scalar.points, kernel.points, what + " points");
}

void expectSameBox(const katana::math::AABB& a, const katana::math::AABB& b, const std::string& what)
{
    const double left[6] = {a.min.x, a.min.y, a.min.z, a.max.x, a.max.y, a.max.z};
    const double right[6] = {b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z};
    EXPECT_EQ(std::memcmp(left, right, sizeof(left)), 0) << what;
}

void expectSameLayers(const SceneLayers& scalar, const SceneLayers& kernel)
{
    expectSameList(scalar.terrain, kernel.terrain, "terrain");
    expectSameList(scalar.edges, kernel.edges, "edges");
    expectSameBits(scalar.edgeBase, kernel.edgeBase, "edge base");
    expectSameBits(scalar.edgeInk, kernel.edgeInk, "edge ink");
    expectSameBox(scalar.bounds, kernel.bounds, "bounds");
}

// A cells x cells grid of squares, two triangles each with the diagonal
// alternating, over rolling ground at map-grid coordinates. Its vertex count,
// (cells + 1)^2, is 1, 0 or 1 past a multiple of four, so the kernels' padded
// last block is exercised as well as whole ones.
TinSurface rollingTin(int cells, double east = 300000.0, double north = 6250000.0)
{
    const int side = cells + 1;
    std::vector<katana::geometry::Point3> vertices;
    for (int j = 0; j < side; ++j) {
        for (int i = 0; i < side; ++i) {
            const double x = 7.0 * i;
            const double y = 5.0 * j;
            vertices.emplace_back(east + x, north + y,
                                  40.0 + 6.0 * std::sin(x * 0.05) * std::cos(y * 0.07) +
                                      0.3 * std::sin(x * 0.9 + y * 0.4));
        }
    }
    std::vector<katana::terrain::TinTriangle> triangles;
    for (int j = 0; j < cells; ++j) {
        for (int i = 0; i < cells; ++i) {
            const auto v0 = static_cast<std::uint32_t>(j * side + i);
            const auto v1 = v0 + 1;
            const auto v2 = v0 + static_cast<std::uint32_t>(side) + 1;
            const auto v3 = v0 + static_cast<std::uint32_t>(side);
            if ((i + j) % 2 == 0) {
                triangles.push_back({v0, v1, v2});
                triangles.push_back({v0, v2, v3});
            } else {
                triangles.push_back({v0, v1, v3});
                triangles.push_back({v1, v2, v3});
            }
        }
    }
    auto surface = TinSurface::create(std::move(vertices), std::move(triangles));
    EXPECT_TRUE(surface.ok());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

SceneLayers terrainAt(SimdLevel level, const std::vector<SceneSurface>& surfaces,
                      const std::vector<SceneMesh>& meshes, const SceneOptions& options)
{
    AtLevel at(level);
    EXPECT_TRUE(at.ok());
    SceneBuilder builder;
    SceneLayers layers;
    builder.buildTerrain(surfaces, meshes, options, layers);
    return layers;
}

void expectSameTerrain(const std::vector<SceneSurface>& surfaces,
                       const std::vector<SceneMesh>& meshes, const SceneOptions& options)
{
    const SceneLayers scalar = terrainAt(SimdLevel::Scalar, surfaces, meshes, options);
    const SceneLayers kernel = terrainAt(katana::test::kernelLevel(), surfaces, meshes, options);
    expectSameLayers(scalar, kernel);
}

} // namespace

TEST(SceneKernels, ALitRampColouredTinBuildsTheSameDrawListsAtEveryLevel)
{
    KATANA_REQUIRE_SIMD_KERNELS();
    // 1 to 9 cells: 4 to 100 vertices, both sides of the dispatch threshold
    // and every remainder of four; then one big enough to be all kernel.
    for (const int cells : {1, 2, 3, 4, 5, 6, 7, 8, 9, 60}) {
        SCOPED_TRACE(cells);
        const TinSurface tin = rollingTin(cells);
        std::vector<SceneSurface> surfaces(1);
        surfaces[0].surface = &tin;
        surfaces[0].style = SurfaceStyle::Shaded;
        SceneOptions options;
        expectSameTerrain(surfaces, {}, options);
        // Exaggerated about a datum, from a low sun in the north-east.
        options.verticalExaggeration = 2.5;
        options.exaggerationDatum = 37.25;
        options.lightDirection = {0.6, 0.7, 0.25};
        expectSameTerrain(surfaces, {}, options);
    }
}

TEST(SceneKernels, EveryStyleAndColouringOfASurfaceBuildsTheSameAtEveryLevel)
{
    KATANA_REQUIRE_SIMD_KERNELS();
    const TinSurface a = rollingTin(11);
    const TinSurface b = rollingTin(6, 300020.0, 6250010.0);
    for (const SurfaceStyle style : {SurfaceStyle::Shaded, SurfaceStyle::ShadedWithEdges,
                                     SurfaceStyle::Wireframe, SurfaceStyle::Automatic}) {
        for (const SurfaceColoring coloring :
             {SurfaceColoring::Elevation, SurfaceColoring::Flat, SurfaceColoring::Slope}) {
            SCOPED_TRACE(static_cast<int>(style) * 10 + static_cast<int>(coloring));
            std::vector<SceneSurface> surfaces(2);
            surfaces[0].surface = &a;
            surfaces[1].surface = &b;
            for (SceneSurface& item : surfaces) {
                item.style = style;
                item.coloring = coloring;
                item.flatColor = katana::render::rgba(120, 200, 90, 180);
            }
            SceneOptions options;
            expectSameTerrain(surfaces, {}, options);
            options.lightDirection = {0.0, 0.0, 0.0}; // unlit: no normals at all
            expectSameTerrain(surfaces, {}, options);
        }
    }
}

TEST(SceneKernels, ASurfaceOfOneElevationTakesTheRampsMiddleColourAtEveryLevel)
{
    KATANA_REQUIRE_SIMD_KERNELS();
    // A flat TIN spans no elevation, so the ramp is its middle colour,
    // elevationRampColor(0.5) = (235, 220, 130), before the light shades it:
    // a flat face lit from straight above takes sky 1.0 and the whole sun.
    std::vector<katana::geometry::Point3> vertices;
    std::vector<katana::terrain::TinTriangle> triangles;
    for (int j = 0; j < 5; ++j) {
        for (int i = 0; i < 5; ++i) {
            vertices.emplace_back(10.0 * i, 10.0 * j, 12.0);
        }
    }
    for (std::uint32_t j = 0; j < 4; ++j) {
        for (std::uint32_t i = 0; i < 4; ++i) {
            const std::uint32_t v = j * 5 + i;
            triangles.push_back({v, v + 1, v + 6});
            triangles.push_back({v, v + 6, v + 5});
        }
    }
    auto tin = TinSurface::create(std::move(vertices), std::move(triangles));
    ASSERT_TRUE(tin.ok());
    std::vector<SceneSurface> surfaces(1);
    surfaces[0].surface = &*tin;
    surfaces[0].style = SurfaceStyle::Shaded;
    SceneOptions options;
    options.lightDirection = {0.0, 0.0, 1.0};
    options.skyAmbient = 1.0;
    options.groundAmbient = 0.0;
    options.sunStrength = 0.0;
    const SceneLayers scalar = terrainAt(SimdLevel::Scalar, surfaces, {}, options);
    const SceneLayers kernel = terrainAt(katana::test::kernelLevel(), surfaces, {}, options);
    expectSameLayers(scalar, kernel);
    ASSERT_EQ(kernel.terrain.colors.size(), 25u);
    EXPECT_EQ(kernel.terrain.colors[12], katana::render::rgba(235, 220, 130));
}

TEST(SceneKernels, MeshesBuildTheSameAtEveryLevelIncludingBadAndDegenerateFaces)
{
    KATANA_REQUIRE_SIMD_KERNELS();
    // Boxes of 12 faces, cut to every count from 1 to 25 so each remainder
    // of the kernel's four faces is met, with a face naming a vertex that is
    // not there (skipped at both levels), a face of zero area (left unlit)
    // and colours for some faces only.
    for (std::size_t faces = 1; faces <= 25; ++faces) {
        SCOPED_TRACE(faces);
        katana::geometry::TriangleMesh mesh;
        for (int box = 0; box < 3; ++box) {
            const auto first = static_cast<std::uint32_t>(mesh.vertices.size());
            for (int k = 0; k < 8; ++k) {
                mesh.vertices.emplace_back(300000.0 + 3.0 * box + ((k & 1) != 0 ? 1.1 : 0.0),
                                           6250000.0 + ((k & 2) != 0 ? 1.7 : 0.0),
                                           40.0 + 0.5 * box + ((k & 4) != 0 ? 2.3 : 0.0));
            }
            static constexpr std::uint32_t kFaces[12][3] = {
                {0, 2, 1}, {1, 2, 3}, {4, 5, 6}, {5, 7, 6}, {0, 1, 4}, {1, 5, 4},
                {2, 6, 3}, {3, 6, 7}, {0, 4, 2}, {2, 4, 6}, {1, 3, 5}, {3, 7, 5}};
            for (const auto& f : kFaces) {
                mesh.faces.push_back({first + f[0], first + f[1], first + f[2]});
            }
        }
        mesh.faces[4] = {0, 1, 999};  // names no vertex
        mesh.faces[7] = {2, 2, 3};    // no area
        mesh.faces.resize(faces);
        std::vector<SceneMesh> items(2);
        items[0].mesh = &mesh;
        items[1].mesh = &mesh;
        items[1].flatColor = katana::render::rgba(10, 250, 128, 200);
        for (std::size_t f = 0; f < faces / 2; ++f) {
            items[1].faceColors.push_back(
                katana::render::rgba(static_cast<std::uint8_t>(30 * f), 90, 255));
        }
        SceneOptions options;
        options.verticalExaggeration = 3.0;
        options.exaggerationDatum = 40.0;
        expectSameTerrain({}, items, options);
        options.lightDirection = {0.0, 0.0, 0.0};
        expectSameTerrain({}, items, options);
    }
}

TEST(SceneKernels, NormalsTooLongToSquareLightTheSameAtEveryLevel)
{
    KATANA_REQUIRE_SIMD_KERNELS();
    // std::hypot of a normal whose squares overflow a double: a surface lifted
    // 1e200 times over has face normals near 1e201 (a 7 x 5 m cell, relief of
    // metres), whose squares, near 1e402, are past DBL_MAX (1.8e308).
    // libstdc++ divides by the largest component first, libc++ scales by
    // 2^-532 when the largest passes 2^512 (1.3e154): the kernels copy
    // whichever the scalar code calls, and this is where the two part from
    // the naive formula. A mesh as large, and one with a NaN corner, too.
    const TinSurface tin = rollingTin(9);
    std::vector<SceneSurface> surfaces(1);
    surfaces[0].surface = &tin;
    surfaces[0].style = SurfaceStyle::Shaded;
    SceneOptions options;
    options.verticalExaggeration = 1.0e200;
    options.lightDirection = {0.3, -0.4, 0.8};
    expectSameTerrain(surfaces, {}, options);

    katana::geometry::TriangleMesh mesh;
    for (int k = 0; k < 8; ++k) {
        mesh.vertices.emplace_back((k & 1) != 0 ? 1.0e100 : 0.0, (k & 2) != 0 ? 3.0e100 : 0.0,
                                   (k & 4) != 0 ? 2.0e100 : 0.0);
    }
    mesh.vertices.emplace_back(std::numeric_limits<double>::quiet_NaN(), 1.0, 2.0);
    mesh.faces = {{0, 2, 1}, {1, 2, 3}, {4, 5, 6}, {5, 7, 6}, {0, 1, 8},
                  {0, 1, 4}, {1, 5, 4}, {2, 6, 3}, {3, 6, 7}};
    std::vector<SceneMesh> items(1);
    items[0].mesh = &mesh;
    SceneOptions plain;
    plain.lightDirection = {0.3, -0.4, 0.8};
    expectSameTerrain({}, items, plain);
}

TEST(SceneKernels, EdgeFadesEqualTheScalarBlendForEveryChannelPairAndEveryEighth)
{
    KATANA_REQUIRE_SIMD_KERNELS();
    // Every (base, ink) pair of channel values, each in all four channels at
    // once, with a count that ends mid-block. The kernel works in integers;
    // the scalar blend in doubles, so this is the proof they agree.
    SceneLayers layers;
    for (std::uint32_t base = 0; base < 256; ++base) {
        for (std::uint32_t ink = 0; ink < 256; ++ink) {
            layers.edgeBase.push_back(base * 0x01010101u);
            layers.edgeInk.push_back(ink * 0x01010101u);
        }
    }
    layers.edgeBase.resize(layers.edgeBase.size() - 3);
    layers.edgeInk.resize(layers.edgeBase.size());
    layers.edges.colors = layers.edgeBase;
    layers.edges.positions.assign(layers.edgeBase.size(), katana::render::Vec3(0.0, 0.0, 0.0));
    katana::render::Camera camera;
    camera.setViewportSize(800, 600);
    ASSERT_TRUE(camera.frame(katana::math::AABB(katana::render::Vec3(0.0, 0.0, 0.0),
                                                katana::render::Vec3(100.0, 100.0, 10.0))));
    const double pixel = camera.worldPerPixelAt(camera.distance());
    ASSERT_GT(pixel, 0.0);
    // Typical edges of 4 to 12 px step through every eighth of the fade.
    int eighthsSeen = 0;
    for (int tenths = 40; tenths <= 120; tenths += 2) {
        SCOPED_TRACE(tenths);
        SceneLayers::EdgeRun run;
        run.first = 0;
        run.count = static_cast<katana::render::VertexIndex>(layers.edgeBase.size());
        run.typicalEdge = pixel * tenths / 10.0;
        layers.edgeRuns = {run};
        SceneLayers scalar = layers;
        SceneLayers kernel = layers;
        {
            AtLevel at(SimdLevel::Scalar);
            SceneBuilder::fadeEdges(scalar, camera);
        }
        {
            AtLevel at(katana::test::kernelLevel());
            SceneBuilder::fadeEdges(kernel, camera);
        }
        EXPECT_EQ(scalar.edgeRuns[0].applied, kernel.edgeRuns[0].applied);
        expectSameBits(scalar.edges.colors, kernel.edges.colors, "faded colours");
        eighthsSeen |= 1 << static_cast<int>(kernel.edgeRuns[0].applied * 8.0f);
    }
    EXPECT_EQ(eighthsSeen, 0x1FF) << "every strength from 0 to 8 eighths";
}

TEST(SceneKernels, TheBoundsAreTheBoxOfEveryListEvenWhereAnExtremeIsASignedZero)
{
    // The scene's bounds are its lists' DrawList::bounds(), which visits each
    // vertex once, not once per primitive (tests/render/test_draw_list.cpp
    // holds it to the walk it replaced); the box must be theirs to the bit,
    // zeros' signs included.
    std::vector<katana::geometry::Point3> vertices = {
        {-0.0, 0.0, -0.0}, {10.0, -0.0, 0.0}, {10.0, 10.0, 5.0}, {0.0, 10.0, -0.0}};
    std::vector<katana::terrain::TinTriangle> triangles = {{0, 1, 2}, {0, 2, 3}};
    auto tin = TinSurface::create(std::move(vertices), std::move(triangles));
    ASSERT_TRUE(tin.ok());
    std::vector<SceneSurface> surfaces(1);
    surfaces[0].surface = &*tin;
    SceneOptions options;
    options.exaggerationDatum = -0.0;
    SceneBuilder builder;
    SceneLayers layers;
    builder.buildTerrain(surfaces, {}, options, layers);
    katana::math::AABB expected = layers.terrain.bounds();
    expected.expand(layers.edges.bounds());
    expectSameBox(expected, layers.bounds, "terrain bounds");
    EXPECT_EQ(layers.bounds.max.x, 10.0);
    EXPECT_EQ(layers.bounds.max.z, 5.0);
}
