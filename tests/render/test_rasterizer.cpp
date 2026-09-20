// Software rasteriser (PLAN.MD Phase 15).
//
// These are pixel assertions, not smoke tests: the whole reason the first
// renderer is on the CPU is that its output can be checked exactly, so that the
// clipping and depth rules are nailed down before a Vulkan backend has to
// reproduce them.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <utility>
#include <vector>

#include "katana/core/task_pool.hpp"
#include "katana/render/rasterizer.hpp"

using katana::core::TaskPool;
using katana::math::Vec3;
using katana::render::Camera;
using katana::render::DrawList;
using katana::render::Framebuffer;
using katana::render::Projection;
using katana::render::Rasterizer;
using katana::render::rgba;
using katana::render::RenderOptions;
using katana::render::StandardView;

namespace {

constexpr katana::render::Rgba kBackground = rgba(0, 0, 0);
constexpr katana::render::Rgba kRed = rgba(255, 0, 0);
constexpr katana::render::Rgba kGreen = rgba(0, 255, 0);
constexpr katana::render::Rgba kBlue = rgba(0, 0, 255);

// A camera looking straight down at the XY plane, orthographic, with exactly
// one world unit per pixel and the origin at the image centre. Every geometric
// assertion below is then a statement about pixels that can be read off by hand.
Camera planCamera(int width, int height)
{
    Camera camera;
    camera.setViewportSize(width, height);
    camera.setProjection(Projection::Orthographic);
    camera.setStandardView(StandardView::Top);
    camera.setTarget(Vec3(0.0, 0.0, 0.0));
    camera.setDistance(1000.0);
    camera.setOrthographicHeight(static_cast<double>(height));
    camera.setDepthRange(1.0, 2000.0);
    return camera;
}

std::size_t countPixels(const Framebuffer& fb, katana::render::Rgba color)
{
    return static_cast<std::size_t>(std::count(fb.color().begin(), fb.color().end(), color));
}

RenderOptions serialOptions(TaskPool& pool)
{
    RenderOptions options;
    options.background = kBackground;
    options.pool = &pool;
    return options;
}

} // namespace

TEST(RenderRasterizer, RejectsATargetThatDoesNotMatchTheCamera)
{
    auto target = Framebuffer::create(64, 64);
    ASSERT_TRUE(target.ok());
    Camera camera = planCamera(32, 32); // deliberately the wrong size

    Rasterizer rasterizer;
    DrawList list;
    const auto result = rasterizer.render(list, camera, *target);
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, katana::core::ErrorCode::InvalidArgument);
}

TEST(RenderRasterizer, AnEmptyListLeavesTheClearedBackground)
{
    auto target = Framebuffer::create(64, 48);
    ASSERT_TRUE(target.ok());
    Camera camera = planCamera(64, 48);
    TaskPool pool(0);

    Rasterizer rasterizer;
    DrawList list;
    const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
    ASSERT_TRUE(stats.ok()) << stats.error().describe();
    EXPECT_EQ(stats->fragments, 0u);
    EXPECT_EQ(countPixels(*target, kBackground), 64u * 48u);
    EXPECT_FLOAT_EQ(target->depthAt(10, 10), 1.0f) << "depth must clear to the far plane";
}

TEST(RenderRasterizer, FillsExactlyTheAreaOfAnAxisAlignedTriangle)
{
    auto target = Framebuffer::create(100, 100);
    ASSERT_TRUE(target.ok());
    Camera camera = planCamera(100, 100);
    TaskPool pool(0);

    // A right triangle with legs of 40 world units = 40 pixels, so the filled
    // area is 800 pixels give or take the boundary.
    DrawList list;
    const auto a = list.addVertex(Vec3(-20.0, -20.0, 0.0), kRed);
    const auto b = list.addVertex(Vec3(20.0, -20.0, 0.0), kRed);
    const auto c = list.addVertex(Vec3(-20.0, 20.0, 0.0), kRed);
    list.addTriangle(a, b, c);

    Rasterizer rasterizer;
    const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
    ASSERT_TRUE(stats.ok()) << stats.error().describe();
    EXPECT_EQ(stats->trianglesSubmitted, 1u);
    EXPECT_EQ(stats->trianglesRasterised, 1u);

    const std::size_t filled = countPixels(*target, kRed);
    // Half of 40x40. The tolerance is the boundary row and column, which a
    // half-open sampling rule may or may not include.
    EXPECT_NEAR(static_cast<double>(filled), 800.0, 45.0) << "filled " << filled;

    // Inside is red, well outside is background.
    EXPECT_EQ(target->colorAt(45, 65), kRed);
    EXPECT_EQ(target->colorAt(90, 90), kBackground);
}

TEST(RenderRasterizer, TheNearerTriangleWinsWhicheverOrderTheyAreSubmitted)
{
    Camera camera = planCamera(40, 40);
    TaskPool pool(0);

    // Both cover the whole image; the green one is 10 units higher, so it is
    // nearer the camera looking down and must survive.
    const auto build = [](bool nearFirst) {
        DrawList list;
        const auto addQuad = [&list](double z, katana::render::Rgba color) {
            const auto v0 = list.addVertex(Vec3(-50.0, -50.0, z), color);
            const auto v1 = list.addVertex(Vec3(50.0, -50.0, z), color);
            const auto v2 = list.addVertex(Vec3(50.0, 50.0, z), color);
            const auto v3 = list.addVertex(Vec3(-50.0, 50.0, z), color);
            list.addTriangle(v0, v1, v2);
            list.addTriangle(v0, v2, v3);
        };
        if (nearFirst) {
            addQuad(10.0, kGreen);
            addQuad(0.0, kRed);
        } else {
            addQuad(0.0, kRed);
            addQuad(10.0, kGreen);
        }
        return list;
    };

    for (bool nearFirst : {true, false}) {
        auto target = Framebuffer::create(40, 40);
        ASSERT_TRUE(target.ok());
        Rasterizer rasterizer;
        const DrawList list = build(nearFirst);
        const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
        ASSERT_TRUE(stats.ok()) << stats.error().describe();
        EXPECT_EQ(countPixels(*target, kGreen), 40u * 40u)
            << "submission order must not decide occlusion (nearFirst=" << nearFirst << ")";
        EXPECT_EQ(countPixels(*target, kRed), 0u);
    }
}

TEST(RenderRasterizer, GeometryBehindTheNearPlaneIsClippedNotMirrored)
{
    auto target = Framebuffer::create(80, 80);
    ASSERT_TRUE(target.ok());
    TaskPool pool(0);

    Camera camera;
    camera.setViewportSize(80, 80);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(StandardView::Front); // looking north from the south
    camera.setTarget(Vec3(0.0, 0.0, 0.0));
    camera.setDistance(50.0);                    // eye at y = -50
    camera.setDepthRange(10.0, 500.0);           // near plane at y = -40

    // A big triangle straddling the near plane: one vertex is behind the eye.
    DrawList list;
    const auto a = list.addVertex(Vec3(-30.0, -100.0, 0.0), kRed); // behind the eye
    const auto b = list.addVertex(Vec3(30.0, 20.0, -20.0), kRed);
    const auto c = list.addVertex(Vec3(30.0, 20.0, 20.0), kRed);
    list.addTriangle(a, b, c);

    Rasterizer rasterizer;
    const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
    ASSERT_TRUE(stats.ok()) << stats.error().describe();
    // Clipping the triangle against the near plane yields a quad, hence two
    // screen triangles rather than one.
    EXPECT_EQ(stats->trianglesSubmitted, 1u);
    EXPECT_GE(stats->trianglesRasterised, 1u);

    // Every written depth must be inside [0, 1]: a vertex that slipped through
    // the divide with a negative w lands outside it.
    for (std::size_t i = 0; i < target->depth().size(); ++i) {
        EXPECT_GE(target->depth()[i], 0.0f);
        EXPECT_LE(target->depth()[i], 1.0f);
    }
    EXPECT_GT(countPixels(*target, kRed), 0u) << "the visible part must still be drawn";
}

TEST(RenderRasterizer, ATriangleEntirelyBehindTheEyeDrawsNothing)
{
    auto target = Framebuffer::create(64, 64);
    ASSERT_TRUE(target.ok());
    TaskPool pool(0);

    Camera camera;
    camera.setViewportSize(64, 64);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(StandardView::Front);
    camera.setTarget(Vec3(0.0, 0.0, 0.0));
    camera.setDistance(50.0);
    camera.setDepthRange(1.0, 500.0);

    DrawList list;
    const auto a = list.addVertex(Vec3(-10.0, -200.0, 0.0), kRed);
    const auto b = list.addVertex(Vec3(10.0, -200.0, 0.0), kRed);
    const auto c = list.addVertex(Vec3(0.0, -180.0, 10.0), kRed);
    list.addTriangle(a, b, c);

    Rasterizer rasterizer;
    const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
    ASSERT_TRUE(stats.ok()) << stats.error().describe();
    EXPECT_EQ(stats->trianglesRasterised, 0u);
    EXPECT_EQ(countPixels(*target, kBackground), 64u * 64u);
}

TEST(RenderRasterizer, LinesHonourTheirPixelWidth)
{
    Camera camera = planCamera(100, 100);
    TaskPool pool(0);

    for (float width : {1.0f, 3.0f, 7.0f}) {
        auto target = Framebuffer::create(100, 100);
        ASSERT_TRUE(target.ok());
        DrawList list;
        // 60 world units = 60 pixels, horizontal across the middle.
        list.addSegment(Vec3(-30.0, 0.0, 0.0), Vec3(30.0, 0.0, 0.0), kBlue, width);

        Rasterizer rasterizer;
        const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
        ASSERT_TRUE(stats.ok()) << stats.error().describe();
        EXPECT_EQ(stats->linesSubmitted, 1u);

        const auto filled = static_cast<double>(countPixels(*target, kBlue));
        const double expected = 60.0 * static_cast<double>(width);
        EXPECT_NEAR(filled, expected, expected * 0.25 + 60.0)
            << "width " << width << " filled " << filled;
    }
}

TEST(RenderRasterizer, ALineSeenEndOnStillLeavesAMark)
{
    auto target = Framebuffer::create(64, 64);
    ASSERT_TRUE(target.ok());
    Camera camera = planCamera(64, 64); // looking straight down
    TaskPool pool(0);

    // A vertical pipe: in plan view its two ends land on the same pixel. A
    // naive widening divides by a zero length and the pipe vanishes.
    DrawList list;
    list.addSegment(Vec3(0.0, 0.0, -5.0), Vec3(0.0, 0.0, 5.0), kBlue, 3.0f);

    Rasterizer rasterizer;
    const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
    ASSERT_TRUE(stats.ok()) << stats.error().describe();
    EXPECT_GT(countPixels(*target, kBlue), 0u);
}

TEST(RenderRasterizer, DepthBiasLiftsWireframeOffTheSurfaceItBounds)
{
    Camera camera = planCamera(60, 60);
    TaskPool pool(0);

    const auto renderWith = [&](float bias) {
        auto target = Framebuffer::create(60, 60);
        EXPECT_TRUE(target.ok());
        DrawList list;
        const auto v0 = list.addVertex(Vec3(-25.0, -25.0, 0.0), kRed);
        const auto v1 = list.addVertex(Vec3(25.0, -25.0, 0.0), kRed);
        const auto v2 = list.addVertex(Vec3(25.0, 25.0, 0.0), kRed);
        list.addTriangle(v0, v1, v2);
        // Exactly coplanar with the triangle's edge: without a bias this is a
        // coin toss per pixel.
        list.addSegment(Vec3(-25.0, -25.0, 0.0), Vec3(25.0, -25.0, 0.0), kGreen, 3.0f, bias);

        Rasterizer rasterizer;
        const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
        EXPECT_TRUE(stats.ok());
        return countPixels(*target, kGreen);
    };

    const std::size_t biased = renderWith(1.0e-4f);
    EXPECT_GT(biased, 100u) << "a biased edge must win against the surface it lies in";
}

TEST(RenderRasterizer, PointsAreDrawnAtTheirRequestedPixelSize)
{
    auto target = Framebuffer::create(64, 64);
    ASSERT_TRUE(target.ok());
    Camera camera = planCamera(64, 64);
    TaskPool pool(0);

    DrawList list;
    const auto v = list.addVertex(Vec3(0.0, 0.0, 0.0), kGreen);
    list.addPoint(v, 5.0f);

    Rasterizer rasterizer;
    const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
    ASSERT_TRUE(stats.ok()) << stats.error().describe();
    EXPECT_EQ(stats->pointsSubmitted, 1u);
    const std::size_t filled = countPixels(*target, kGreen);
    EXPECT_GE(filled, 16u);
    EXPECT_LE(filled, 36u) << "a 5 px point must not smear across the image";
}

TEST(RenderRasterizer, BackfaceCullingDiscardsOnlyTheFarSide)
{
    Camera camera = planCamera(50, 50);
    TaskPool pool(0);

    // Counter-clockwise in plan, so it faces up, towards a camera looking down.
    DrawList list;
    const auto v0 = list.addVertex(Vec3(-20.0, -20.0, 0.0), kRed);
    const auto v1 = list.addVertex(Vec3(20.0, -20.0, 0.0), kRed);
    const auto v2 = list.addVertex(Vec3(0.0, 20.0, 0.0), kRed);
    list.addTriangle(v0, v1, v2);

    RenderOptions options = serialOptions(pool);
    options.backfaceCull = true;

    auto fromAbove = Framebuffer::create(50, 50);
    ASSERT_TRUE(fromAbove.ok());
    Rasterizer rasterizer;
    ASSERT_TRUE(rasterizer.render(list, camera, *fromAbove, options).ok());
    EXPECT_GT(countPixels(*fromAbove, kRed), 0u) << "the front face must survive culling";

    Camera below = planCamera(50, 50);
    below.setStandardView(StandardView::Bottom);
    auto fromBelow = Framebuffer::create(50, 50);
    ASSERT_TRUE(fromBelow.ok());
    ASSERT_TRUE(rasterizer.render(list, below, *fromBelow, options).ok());
    EXPECT_EQ(countPixels(*fromBelow, kRed), 0u) << "the back face must be culled";

    // And with culling off it is visible from both sides, which is the default
    // precisely because a survey surface is inspected from underneath.
    options.backfaceCull = false;
    ASSERT_TRUE(rasterizer.render(list, below, *fromBelow, options).ok());
    EXPECT_GT(countPixels(*fromBelow, kRed), 0u);
}

TEST(RenderRasterizer, ThreadCountDoesNotChangeASinglePixel)
{
    // Rule 7. The binning is chunked by primitive index rather than by core, so
    // a tile visits its primitives in the same order on every machine. This is
    // the assertion that keeps that true.
    Camera camera = planCamera(320, 240);

    DrawList list;
    // Enough primitives to span several chunks, at overlapping depths so that
    // any order-dependence shows up as a different pixel.
    for (int i = 0; i < 400; ++i) {
        const double t = static_cast<double>(i);
        const double x = std::fmod(t * 7.0, 300.0) - 150.0;
        const double y = std::fmod(t * 11.0, 220.0) - 110.0;
        const double z = std::fmod(t * 3.0, 20.0) - 10.0;
        const auto color = rgba(static_cast<std::uint8_t>(i * 3 % 256),
                                static_cast<std::uint8_t>(i * 7 % 256),
                                static_cast<std::uint8_t>(i * 11 % 256));
        const auto a = list.addVertex(Vec3(x, y, z), color);
        const auto b = list.addVertex(Vec3(x + 40.0, y + 5.0, z), color);
        const auto c = list.addVertex(Vec3(x + 10.0, y + 35.0, z), color);
        list.addTriangle(a, b, c);
        list.addLine(a, b, 2.0f);
        list.addPoint(c, 3.0f);
    }

    std::vector<katana::render::Rgba> reference;
    std::vector<float> referenceDepth;
    for (std::size_t workers : {std::size_t{0}, std::size_t{1}, std::size_t{3},
                                std::size_t{7}}) {
        TaskPool pool(workers);
        auto target = Framebuffer::create(320, 240);
        ASSERT_TRUE(target.ok());
        Rasterizer rasterizer;
        RenderOptions options;
        options.background = kBackground;
        options.pool = &pool;
        const auto stats = rasterizer.render(list, camera, *target, options);
        ASSERT_TRUE(stats.ok()) << stats.error().describe();
        EXPECT_GT(stats->fragments, 0u);

        if (reference.empty()) {
            reference = target->color();
            referenceDepth = target->depth();
        } else {
            EXPECT_EQ(target->color(), reference) << workers << " workers changed the colour";
            EXPECT_EQ(target->depth(), referenceDepth) << workers << " workers changed the depth";
        }
    }
}

TEST(RenderRasterizer, ReusingARasterizerGivesTheSameFrameAsAFreshOne)
{
    // The scratch buffers are reused across frames to avoid reallocating; a
    // stale entry left in one would show as a ghost of the previous frame.
    Camera camera = planCamera(80, 80);
    TaskPool pool(2);

    DrawList first;
    const auto a = first.addVertex(Vec3(-30.0, -30.0, 0.0), kRed);
    const auto b = first.addVertex(Vec3(30.0, -30.0, 0.0), kRed);
    const auto c = first.addVertex(Vec3(0.0, 30.0, 0.0), kRed);
    first.addTriangle(a, b, c);

    DrawList second;
    const auto d = second.addVertex(Vec3(-10.0, -10.0, 0.0), kGreen);
    const auto e = second.addVertex(Vec3(10.0, -10.0, 0.0), kGreen);
    const auto f = second.addVertex(Vec3(0.0, 10.0, 0.0), kGreen);
    second.addTriangle(d, e, f);

    Rasterizer reused;
    auto target = Framebuffer::create(80, 80);
    ASSERT_TRUE(target.ok());
    RenderOptions options;
    options.background = kBackground;
    options.pool = &pool;
    ASSERT_TRUE(reused.render(first, camera, *target, options).ok());
    ASSERT_TRUE(reused.render(second, camera, *target, options).ok());

    Rasterizer fresh;
    auto expected = Framebuffer::create(80, 80);
    ASSERT_TRUE(expected.ok());
    ASSERT_TRUE(fresh.render(second, camera, *expected, options).ok());

    EXPECT_EQ(target->color(), expected->color());
    EXPECT_EQ(countPixels(*target, kRed), 0u) << "the first frame must leave no ghost";
}

TEST(RenderRasterizer, PrimitivesSpanningManyTilesAreDrawnWhole)
{
    // One triangle covering the entire image crosses every 64x64 tile boundary.
    // A binning mistake shows up as missing tiles, which is visible as an exact
    // pixel count rather than as "looks fine".
    auto target = Framebuffer::create(200, 150);
    ASSERT_TRUE(target.ok());
    Camera camera = planCamera(200, 150);
    TaskPool pool(4);

    DrawList list;
    const auto v0 = list.addVertex(Vec3(-500.0, -500.0, 0.0), kRed);
    const auto v1 = list.addVertex(Vec3(500.0, -500.0, 0.0), kRed);
    const auto v2 = list.addVertex(Vec3(500.0, 500.0, 0.0), kRed);
    const auto v3 = list.addVertex(Vec3(-500.0, 500.0, 0.0), kRed);
    list.addTriangle(v0, v1, v2);
    list.addTriangle(v0, v2, v3);

    Rasterizer rasterizer;
    RenderOptions options;
    options.background = kBackground;
    options.pool = &pool;
    const auto stats = rasterizer.render(list, camera, *target, options);
    ASSERT_TRUE(stats.ok()) << stats.error().describe();
    EXPECT_EQ(countPixels(*target, kRed), 200u * 150u) << "every tile must be covered";
    EXPECT_EQ(stats->tiles, 4u * 3u); // ceil(200/64) x ceil(150/64)
}

TEST(RenderRasterizer, OffScreenGeometryCostsNoFragments)
{
    auto target = Framebuffer::create(64, 64);
    ASSERT_TRUE(target.ok());
    Camera camera = planCamera(64, 64);
    TaskPool pool(0);

    DrawList list;
    const auto a = list.addVertex(Vec3(1000.0, 1000.0, 0.0), kRed);
    const auto b = list.addVertex(Vec3(1100.0, 1000.0, 0.0), kRed);
    const auto c = list.addVertex(Vec3(1000.0, 1100.0, 0.0), kRed);
    list.addTriangle(a, b, c);

    Rasterizer rasterizer;
    const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
    ASSERT_TRUE(stats.ok()) << stats.error().describe();
    EXPECT_EQ(stats->fragments, 0u);
    EXPECT_EQ(stats->binEntries, 0u) << "geometry outside the image must not be binned at all";
}
