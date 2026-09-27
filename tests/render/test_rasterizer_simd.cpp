// The rasteriser's fast paths (rasterizer.cpp, "conservative coverage", and
// src/katana_render/simd/): tiles left out at binning, rows bounded before the
// fill, eight pixels a step on AVX2 (four on NEON), four vertices a step in the
// transform.
// Each one is only allowed to change the time a frame takes. So every scene
// here is drawn at both SIMD levels and on 0, 1, 3 and 7 worker threads, and
// all eight frames must agree in every colour and every depth bit (Rule 7);
// where a count can be worked out by hand, it is checked too.
//
// The scenes aim at the edges of the fast paths: triangles far larger than a
// tile, slivers thinner than a pixel, lines across the whole view, points,
// equal depths, perspective (where 1/w and so the colour vary per pixel), and
// passes that neither clear nor write depth.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <string>
#include <vector>

#include "katana/core/cpu_features.hpp"
#include "katana/core/task_pool.hpp"
#include "katana/render/rasterizer.hpp"
#include "simd_levels.hpp"

using katana::core::SimdLevel;
using katana::core::TaskPool;
using katana::math::Vec3;
using katana::render::Camera;
using katana::render::DrawList;
using katana::render::Framebuffer;
using katana::render::Projection;
using katana::render::Rasterizer;
using katana::render::RenderOptions;
using katana::render::RenderStats;
using katana::render::Rgba;
using katana::render::rgba;
using katana::render::StandardView;

namespace {

constexpr Rgba kBackground = rgba(0, 0, 0);
constexpr Rgba kRed = rgba(255, 0, 0);
constexpr Rgba kGreen = rgba(0, 255, 0);
constexpr int kWidth = 640;
constexpr int kHeight = 480;

// Straight down, orthographic, one world unit per pixel, the origin at the
// image centre: world (x, y) is pixel column x + 320, row 240 - y.
Camera planCamera()
{
    Camera camera;
    camera.setViewportSize(kWidth, kHeight);
    camera.setProjection(Projection::Orthographic);
    camera.setStandardView(StandardView::Top);
    camera.setTarget(Vec3(0.0, 0.0, 0.0));
    camera.setDistance(1000.0);
    camera.setOrthographicHeight(static_cast<double>(kHeight));
    camera.setDepthRange(1.0, 2000.0);
    return camera;
}

// An eye low over the scene, looking across it, so 1/w - and with it the
// perspective-correct colour - changes from pixel to pixel.
Camera perspectiveCamera()
{
    Camera camera;
    camera.setViewportSize(kWidth, kHeight);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(StandardView::IsoSouthWest);
    camera.setTarget(Vec3(0.0, 0.0, 0.0));
    camera.setDistance(400.0);
    camera.setDepthRange(1.0, 4000.0);
    return camera;
}

struct Frame {
    std::vector<Rgba> color;
    std::vector<float> depth;
    RenderStats stats;
};

// One frame at `level` on a pool of `workers` threads. `before`, when given,
// is drawn first into the same target, as a view draws several passes.
Frame draw(const DrawList& list, const Camera& camera, SimdLevel level, std::size_t workers,
           const RenderOptions& options, const DrawList* before = nullptr)
{
    const SimdLevel was = katana::core::activeSimdLevel();
    EXPECT_TRUE(katana::core::setSimdLevel(level).ok());
    TaskPool pool(workers);
    auto target = Framebuffer::create(kWidth, kHeight);
    EXPECT_TRUE(target.ok());
    Rasterizer rasterizer;
    Frame frame;
    if (before != nullptr) {
        RenderOptions first;
        first.background = kBackground;
        first.pool = &pool;
        EXPECT_TRUE(rasterizer.render(*before, camera, *target, first).ok());
    }
    RenderOptions mine = options;
    mine.pool = &pool;
    const auto stats = rasterizer.render(list, camera, *target, mine);
    EXPECT_TRUE(stats.ok());
    if (stats.ok()) {
        frame.stats = *stats;
    }
    frame.color = target->color();
    frame.depth = target->depth();
    (void)katana::core::setSimdLevel(was);
    return frame;
}

// Draws `list` at every level on every thread count and expects one frame.
// Returns it, drawn at the scalar level on one thread.
Frame everyWayTheSame(const DrawList& list, const Camera& camera,
                      RenderOptions options = RenderOptions{},
                      const DrawList* before = nullptr)
{
    options.background = kBackground;
    const Frame reference = draw(list, camera, SimdLevel::Scalar, 0, options, before);
    for (const SimdLevel level : katana::test::simdLevels()) {
        for (const std::size_t workers : {std::size_t{0}, std::size_t{1}, std::size_t{3},
                                          std::size_t{7}}) {
            const Frame frame = draw(list, camera, level, workers, options, before);
            const std::string how = std::string(katana::core::toString(level)) + " on " +
                                    std::to_string(workers) + " workers";
            EXPECT_EQ(frame.color, reference.color) << how << " changed a colour";
            EXPECT_EQ(frame.depth, reference.depth) << how << " changed a depth";
            EXPECT_EQ(frame.stats.fragments, reference.stats.fragments) << how;
        }
    }
    return reference;
}

std::size_t countPixels(const std::vector<Rgba>& color, Rgba wanted)
{
    return static_cast<std::size_t>(std::count(color.begin(), color.end(), wanted));
}

// An axis-aligned square of two triangles, corners (x0, y0) and (x1, y1) at
// height z.
void addSquare(DrawList& list, double x0, double y0, double x1, double y1, double z, Rgba color)
{
    const auto a = list.addVertex(Vec3(x0, y0, z), color);
    const auto b = list.addVertex(Vec3(x1, y0, z), color);
    const auto c = list.addVertex(Vec3(x1, y1, z), color);
    const auto d = list.addVertex(Vec3(x0, y1, z), color);
    list.addTriangle(a, b, c);
    list.addTriangle(a, c, d);
}

} // namespace

TEST(RenderRasterizerSimd, ASquareManyTilesAcrossFillsExactlyItsOwnPixels)
{
    // 300 x 200 world units at one unit a pixel, corners on pixel edges: the
    // pixel centres inside are exactly 300 x 200 = 60,000. The diagonal the
    // two triangles share runs 3 columns for every 2 rows, so it passes a
    // quarter of a pixel from the nearest centres and no pixel is written
    // twice. It spans 6 x 5 tiles, some wholly inside one triangle, the rest
    // cut by the diagonal - the kernel's covered and tested cases both.
    DrawList list;
    addSquare(list, -150.0, -100.0, 150.0, 100.0, 0.0, kRed);
    const Frame frame = everyWayTheSame(list, planCamera());
    EXPECT_EQ(countPixels(frame.color, kRed), 60000u);
    EXPECT_EQ(frame.stats.fragments, 60000u);
}

TEST(RenderRasterizerSimd, OverlappingLargeTrianglesInPerspectiveAgreeAtEveryLevel)
{
    // Large triangles at crossing depths, each vertex its own colour, seen in
    // perspective: every pixel runs the depth test and the perspective-correct
    // colour. 36 vertices: nine steps of the transform kernel's four.
    DrawList list;
    for (int i = 0; i < 12; ++i) {
        const double t = static_cast<double>(i);
        const auto a = list.addVertex(Vec3(-300.0 + 40.0 * t, -250.0, 10.0 * std::sin(t)),
                                      rgba(static_cast<std::uint8_t>(20 * i), 40, 200));
        const auto b = list.addVertex(Vec3(250.0, -200.0 + 30.0 * t, -20.0 + 3.0 * t),
                                      rgba(200, static_cast<std::uint8_t>(15 * i), 60));
        const auto c = list.addVertex(Vec3(-100.0 + 10.0 * t, 300.0, 30.0 - 5.0 * t),
                                      rgba(90, 160, static_cast<std::uint8_t>(18 * i)));
        list.addTriangle(a, b, c);
    }
    const Frame frame = everyWayTheSame(list, perspectiveCamera());
    EXPECT_GT(frame.stats.fragments, 100000u) << "the scene must reach the fast paths at all";
}

TEST(RenderRasterizerSimd, SliversThinnerThanAPixelAgreeAtEveryLevel)
{
    // Long triangles a fraction of a pixel wide at many angles: their boxes
    // are large, so they take the row bounds and the kernel, while almost no
    // pixel centre is inside them. Where the float edge tests are closest to
    // their rounding, the bounds must still let every one of those through.
    DrawList list;
    for (int i = 0; i < 90; ++i) {
        const double angle = 0.0349 * static_cast<double>(i); // 2 degrees a step
        const double c = std::cos(angle);
        const double s = std::sin(angle);
        const double width = 0.05 + 0.01 * static_cast<double>(i % 7);
        const auto color = rgba(static_cast<std::uint8_t>(i * 2), 200, 100);
        const auto a = list.addVertex(Vec3(-220.0 * c, -220.0 * s, 0.1 * i), color);
        const auto b = list.addVertex(Vec3(220.0 * c, 220.0 * s, 0.1 * i), color);
        const auto d = list.addVertex(Vec3(220.0 * c - width * s, 220.0 * s + width * c, 0.1 * i),
                                      color);
        list.addTriangle(a, b, d);
    }
    const Frame frame = everyWayTheSame(list, planCamera());
    EXPECT_GT(frame.stats.fragments, 0u);
}

TEST(RenderRasterizerSimd, LinesAcrossTheViewAreBinnedOnlyInTheTilesTheyCross)
{
    // A 1 px line corner to corner of a 640 x 480 view, 10 x 8 tiles of 64 px.
    // Widened into two triangles, each with a box the size of the view, it
    // used to be binned in all 80 tiles twice over, 160 entries. Going 10
    // tiles across and 8 down it crosses at most 10 + 8 - 1 = 17 of them, and
    // each triangle is now kept only where it can reach: at most 34.
    DrawList list;
    const auto a = list.addVertex(Vec3(-319.0, -239.0, 0.0), kRed);
    const auto b = list.addVertex(Vec3(319.0, 239.0, 0.0), kRed);
    list.addLine(a, b, 1.0f);
    // And lines at other angles and widths, to compare frames on.
    for (int i = 0; i < 24; ++i) {
        const double angle = 0.2618 * static_cast<double>(i); // 15 degrees a step
        const auto p = list.addVertex(Vec3(0.0, 0.0, 1.0), kGreen);
        const auto q = list.addVertex(Vec3(300.0 * std::cos(angle), 230.0 * std::sin(angle), 1.0),
                                      rgba(0, 255, static_cast<std::uint8_t>(10 * i)));
        list.addLine(p, q, 1.0f + static_cast<float>(i % 4));
    }
    DrawList diagonal;
    const auto d0 = diagonal.addVertex(Vec3(-319.0, -239.0, 0.0), kRed);
    const auto d1 = diagonal.addVertex(Vec3(319.0, 239.0, 0.0), kRed);
    diagonal.addLine(d0, d1, 1.0f);
    const Frame alone = everyWayTheSame(diagonal, planCamera());
    EXPECT_LE(alone.stats.binEntries, 34u);
    EXPECT_GT(alone.stats.fragments, 0u);
    everyWayTheSame(list, planCamera());
}

TEST(RenderRasterizerSimd, PointsAmongLargeTrianglesAgreeAtEveryLevel)
{
    // Points send the frame through its three sweeps (filled, decide, lines
    // and points), and the filled sweep here is large triangles.
    DrawList list;
    addSquare(list, -200.0, -150.0, 200.0, 150.0, 0.0, kRed);
    for (int i = 0; i < 60; ++i) {
        const auto p = list.addVertex(Vec3(-190.0 + 6.5 * i, -140.0 + 4.7 * i, (i % 3) - 1.0),
                                      rgba(0, static_cast<std::uint8_t>(4 * i), 255));
        list.addPoint(p, 1.0f + static_cast<float>(i % 7), 1.5f);
    }
    const Frame frame = everyWayTheSame(list, planCamera());
    EXPECT_GT(frame.stats.fragments, 120000u);
}

TEST(RenderRasterizerSimd, AtEqualDepthsTheFirstSubmittedLargeSquareKeepsEveryPixel)
{
    // Reversed Z with a strictly-greater test: of two equal depths the first
    // written stays. Both squares cover the same 200 x 200 = 40,000 pixels,
    // red submitted first, so not one pixel may turn green - at any level, on
    // any number of threads.
    DrawList list;
    addSquare(list, -100.0, -100.0, 100.0, 100.0, 5.0, kRed);
    addSquare(list, -100.0, -100.0, 100.0, 100.0, 5.0, kGreen);
    const Frame frame = everyWayTheSame(list, planCamera());
    EXPECT_EQ(countPixels(frame.color, kRed), 40000u);
    EXPECT_EQ(countPixels(frame.color, kGreen), 0u);
}

TEST(RenderRasterizerSimd, TheClearLeavesFarDepthAndAPassWithoutClearOrDepthKeepsIt)
{
    // The clear writes the reversed-Z far value, 0, everywhere. A second pass
    // that neither clears nor writes depth paints its square over the first
    // pass's (it is tested against depth 0 outside and against the first
    // square's depth inside, where it is nearer) and leaves the depth buffer
    // exactly as the first pass left it.
    DrawList first;
    addSquare(first, -150.0, -100.0, 0.0, 100.0, 0.0, kRed);
    DrawList second;
    addSquare(second, -50.0, -50.0, 150.0, 50.0, 10.0, kGreen);

    const Frame cleared = everyWayTheSame(first, planCamera());
    EXPECT_EQ(countPixels(cleared.color, kRed), 150u * 200u);
    EXPECT_EQ(static_cast<std::size_t>(std::count(cleared.depth.begin(), cleared.depth.end(), 0.0f)),
              static_cast<std::size_t>(kWidth * kHeight) - 150u * 200u);

    RenderOptions overlay;
    overlay.clear = false;
    overlay.depthWrite = false;
    const Frame layered = everyWayTheSame(second, planCamera(), overlay, &first);
    EXPECT_EQ(countPixels(layered.color, kGreen), 200u * 100u);
    EXPECT_EQ(countPixels(layered.color, kRed), 150u * 200u - 50u * 100u);
    EXPECT_EQ(layered.depth, cleared.depth) << "a pass without depth writes wrote depth";
}

TEST(RenderRasterizerSimd, TrianglesOfEverySizeFromAFixedSeedAgreeAtEveryLevel)
{
    // 3,000 triangles from a fixed linear congruential sequence, from a tenth
    // of a pixel to several times the view, at random depths, in perspective:
    // every size cutoff is crossed many times over, and many boxes are cut by
    // the guard band and the near plane.
    DrawList list;
    std::uint64_t state = 0x2545F4914F6CDD1Dull;
    const auto next = [&state] {
        state = state * 6364136223846793005ull + 1442695040888963407ull;
        return static_cast<double>(state >> 11) * 0x1p-53; // [0, 1)
    };
    for (int i = 0; i < 3000; ++i) {
        const double size = std::pow(10.0, -1.0 + 4.0 * next()); // 0.1 to 1000 units
        const double cx = 600.0 * next() - 300.0;
        const double cy = 600.0 * next() - 300.0;
        const double cz = 100.0 * next() - 50.0;
        const auto corner = [&] {
            return Vec3(cx + size * (next() - 0.5), cy + size * (next() - 0.5),
                        cz + 0.2 * size * (next() - 0.5));
        };
        const auto color = rgba(static_cast<std::uint8_t>(255.0 * next()),
                                static_cast<std::uint8_t>(255.0 * next()),
                                static_cast<std::uint8_t>(255.0 * next()));
        const auto a = list.addVertex(corner(), color);
        const auto b = list.addVertex(corner(), rgba(255, 255, 255));
        const auto c = list.addVertex(corner(), color);
        list.addTriangle(a, b, c);
    }
    const Frame frame = everyWayTheSame(list, perspectiveCamera());
    EXPECT_GT(frame.stats.fragments, 0u);
}
