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
    EXPECT_FLOAT_EQ(target->depthAt(10, 10), 0.0f)
        << "depth must clear to the far plane, which reversed Z puts at 0";
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

namespace {

// A level eye 50 units south of the origin, looking north: the view the
// near-plane tests below are worked out for.
Camera levelEyeCamera(int width, int height, double nearPlane, double farPlane)
{
    Camera camera;
    camera.setViewportSize(width, height);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(StandardView::Front); // looking north from the south
    camera.setTarget(Vec3(0.0, 0.0, 0.0));
    camera.setDistance(50.0); // eye at y = -50
    camera.setDepthRange(nearPlane, farPlane);
    return camera;
}

// 80 px is the size the first test below was written at. 400 px and 1920x1080
// are sizes at which clipping at w = 1e-6, as the rasteriser once did, put the
// cut vertex's screen coordinate beyond INT_MAX, so the float-to-int conversion
// in the binning was undefined and dropped the whole primitive - its visible
// part included (audit of 2026-09-23; the threshold for that scene was about
// 213 px).
constexpr std::pair<int, int> kNearPlaneSizes[] = {{80, 80}, {400, 400}, {1920, 1080}};

} // namespace

TEST(RenderRasterizer, GeometryBehindTheNearPlaneIsClippedNotMirrored)
{
    for (const auto& [width, height] : kNearPlaneSizes) {
        SCOPED_TRACE(testing::Message() << width << "x" << height);
        auto target = Framebuffer::create(width, height);
        ASSERT_TRUE(target.ok());
        TaskPool pool(0);
        const Camera camera = levelEyeCamera(width, height, 10.0, 500.0); // near at y = -40

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

        // Every written depth must be inside [0, 1]: a vertex that slipped
        // through the divide with a negative w lands outside it.
        std::size_t outOfRange = 0;
        for (const float depth : target->depth()) {
            outOfRange += (depth >= 0.0f && depth <= 1.0f) ? 0u : 1u;
        }
        EXPECT_EQ(outOfRange, 0u);
        EXPECT_GT(countPixels(*target, kRed), 0u) << "the visible part must still be drawn";
    }
}

TEST(RenderRasterizer, GroundRunningBehindTheEyeCoversTheLowerViewAndNothingAbove)
{
    // Flat ground 10 units below a level eye, running from 950 units behind it
    // to 1050 ahead. Worked out without the program: the eye looks
    // horizontally, so the horizon is the image's middle row and no ray above
    // it meets the ground. A ray through the bottom quarter descends at least
    // half as steeply as the frustum's lower edge, tan(22.5 deg) for the
    // default 45 degree field of view, so it meets the ground within
    // 10 / (0.5 * 0.414) = 48 units: inside the ground, past the near plane,
    // within the far one, and within 1.78 * 0.414 * 48 = 36 units of the
    // centre line across the widest image here. Both triangles cross the eye
    // plane, which is the case clipping at w = 1e-6 lost.
    for (const auto& [width, height] : kNearPlaneSizes) {
        SCOPED_TRACE(testing::Message() << width << "x" << height);
        auto target = Framebuffer::create(width, height);
        ASSERT_TRUE(target.ok());
        TaskPool pool(0);
        const Camera camera = levelEyeCamera(width, height, 1.0, 5000.0);

        DrawList list;
        const auto v0 = list.addVertex(Vec3(-1000.0, -1000.0, -10.0), kRed);
        const auto v1 = list.addVertex(Vec3(1000.0, -1000.0, -10.0), kRed);
        const auto v2 = list.addVertex(Vec3(1000.0, 1000.0, -10.0), kRed);
        const auto v3 = list.addVertex(Vec3(-1000.0, 1000.0, -10.0), kRed);
        list.addTriangle(v0, v1, v2);
        list.addTriangle(v0, v2, v3);

        Rasterizer rasterizer;
        const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
        ASSERT_TRUE(stats.ok()) << stats.error().describe();

        std::size_t skyNotBackground = 0;
        std::size_t groundNotRed = 0;
        for (int y = 0; y < height; ++y) {
            for (int x = 0; x < width; ++x) {
                if (y < height / 2) {
                    skyNotBackground += target->colorAt(x, y) == kBackground ? 0u : 1u;
                } else if (y >= height * 3 / 4) {
                    groundNotRed += target->colorAt(x, y) == kRed ? 0u : 1u;
                }
            }
        }
        EXPECT_EQ(skyNotBackground, 0u) << "nothing may be drawn above the horizon";
        EXPECT_EQ(groundNotRed, 0u) << "every pixel of the bottom quarter looks at the ground";
    }
}

TEST(RenderRasterizer, ALineRunningBehindTheEyeDrawsItsVisiblePart)
{
    // A centreline on the same ground, from 50 units behind the eye to 150
    // ahead, directly below it: it projects onto the image's centre column,
    // from the bottom edge (ground 24 units ahead) up towards the horizon.
    for (const auto& [width, height] : kNearPlaneSizes) {
        SCOPED_TRACE(testing::Message() << width << "x" << height);
        auto target = Framebuffer::create(width, height);
        ASSERT_TRUE(target.ok());
        TaskPool pool(0);
        const Camera camera = levelEyeCamera(width, height, 1.0, 5000.0);

        DrawList list;
        list.addSegment(Vec3(0.0, -100.0, -10.0), Vec3(0.0, 100.0, -10.0), kBlue, 3.0f);

        Rasterizer rasterizer;
        const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
        ASSERT_TRUE(stats.ok()) << stats.error().describe();
        // The centre column's pixel centres are half a pixel from the line,
        // inside its 3 px width.
        EXPECT_EQ(target->colorAt(width / 2, height - 1), kBlue) << "the part under the eye";
        EXPECT_EQ(target->colorAt(width / 2, height * 3 / 4), kBlue);
        EXPECT_EQ(target->colorAt(width / 2, height / 4), kBackground) << "above the horizon";
    }
}

TEST(RenderRasterizer, ClippingAtTheGuardBandDoesNotMoveAVisibleEdge)
{
    // A triangle whose long edge is the line z = x / 2 through the image
    // centre and whose other two edges are at least m / 2 units away. Inside
    // the image it covers exactly the pixels whose centres are below that line,
    // for any m large enough to put the other edges out of sight: clipping it
    // must cut the triangle, not bend the edge that shows. At m = 1e10 the
    // unclipped screen coordinates overflow an int, and a cut computed in float
    // lands at the image centre instead of on the band (2 + -3.1e8 is -3.1e8 in
    // float), losing a quarter of the image - both checked by hand-mutating the
    // rasteriser (2026-09-23: 2048 and 768 wrong pixels respectively).
    //
    // A level elevation, not plan view: Top is clamped just short of looking
    // straight down (camera.hpp), so a vertex 1e10 away in y leaks thousands of
    // units into depth, the near plane cuts the triangle small before the
    // guard band sees it, and that hides the precision this is about.
    Camera camera = planCamera(64, 64);
    camera.setStandardView(StandardView::Front); // x to the right, z up, exactly
    TaskPool pool(0);
    for (const double m : {1.0e3, 1.0e6, 1.0e10}) {
        SCOPED_TRACE(testing::Message() << "m = " << m);
        auto target = Framebuffer::create(64, 64);
        ASSERT_TRUE(target.ok());
        DrawList list;
        const auto a = list.addVertex(Vec3(-m, 0.0, -m / 2.0), kRed);
        const auto b = list.addVertex(Vec3(m, 0.0, m / 2.0), kRed);
        const auto c = list.addVertex(Vec3(m, 0.0, -m), kRed);
        list.addTriangle(a, b, c);

        Rasterizer rasterizer;
        const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
        ASSERT_TRUE(stats.ok()) << stats.error().describe();

        // One unit per pixel, origin at the centre: pixel (col, row) has its
        // centre at x = col + 0.5 - 32, z = 32 - row - 0.5, so its offset from
        // the line, z - x / 2 = 47.25 - row - col / 2, is an odd multiple of
        // 0.25 - never zero, so every pixel has a right answer.
        std::size_t wrong = 0;
        for (int row = 0; row < 64; ++row) {
            for (int col = 0; col < 64; ++col) {
                const double x = col + 0.5 - 32.0;
                const double z = 32.0 - row - 0.5;
                const auto expected = z - x / 2.0 < 0.0 ? kRed : kBackground;
                wrong += target->colorAt(col, row) == expected ? 0u : 1u;
            }
        }
        EXPECT_EQ(wrong, 0u);
    }
}

TEST(RenderRasterizer, ALineFarLongerThanTheImageDrawsAcrossAllOfIt)
{
    // Horizontal, at y = 0.25: screen row 31.75, so a 1 px line covers rows
    // [31.25, 32.25] and every centre on row 31 (31.5) is inside it. Its ends
    // are m units off either side; at m = 1e10 they overflowed an int unclipped.
    const Camera camera = planCamera(64, 64);
    TaskPool pool(0);
    for (const double m : {1.0e3, 1.0e6, 1.0e10}) {
        SCOPED_TRACE(testing::Message() << "m = " << m);
        auto target = Framebuffer::create(64, 64);
        ASSERT_TRUE(target.ok());
        DrawList list;
        list.addSegment(Vec3(-m, 0.25, 0.0), Vec3(m, 0.25, 0.0), kBlue, 1.0f);

        Rasterizer rasterizer;
        const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
        ASSERT_TRUE(stats.ok()) << stats.error().describe();
        std::size_t missing = 0;
        for (int col = 0; col < 64; ++col) {
            missing += target->colorAt(col, 31) == kBlue ? 0u : 1u;
        }
        EXPECT_EQ(missing, 0u);
    }
}

TEST(RenderRasterizer, APointCentredJustOutsideTheImageStillDrawsThePartInside)
{
    // Centred one unit beyond the right edge, 5 px across: its left two
    // columns are inside the image. Rejecting points by their centre alone
    // would lose them, which is one reason the guard band is wider than the
    // image.
    auto target = Framebuffer::create(64, 64);
    ASSERT_TRUE(target.ok());
    const Camera camera = planCamera(64, 64);
    TaskPool pool(0);

    DrawList list;
    const auto v = list.addVertex(Vec3(33.0, 0.0, 0.0), kGreen); // screen x = 65
    list.addPoint(v, 5.0f);

    Rasterizer rasterizer;
    const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
    ASSERT_TRUE(stats.ok()) << stats.error().describe();
    EXPECT_EQ(target->colorAt(63, 32), kGreen);
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

    // One pixel footprint towards the eye (DrawLine::depthBias is in those
    // since the depth-range fit; it was NDC depth, 1e-4 here).
    const std::size_t biased = renderWith(1.0f);
    EXPECT_GT(biased, 100u) << "a biased edge must win against the surface it lies in";
}

TEST(RenderRasterizer, PointsAreDrawnAtExactlyTheirRequestedPixelSize)
{
    // planCamera: one unit per pixel, the origin at the image's centre
    // (32, 32) and world +y up the screen. A point covers the pixels whose
    // CENTRES (i + 0.5) lie in [c - size/2, c + size/2).
    //   size 5 at world (0.5, 0.5) -> screen (32.5, 31.5): x in [30, 35)
    //     holds centres 30.5..34.5, five of them; y likewise: 25 pixels.
    //   size 4 at world (0, 0) -> screen (32, 32): [30, 34) holds 30.5..33.5,
    //     four: 16 pixels.
    // It drew floor(c - h)..floor(c + h), a pixel too many each way: 36 and
    // 25 (audit REN-10).
    const auto drawn = [](const Vec3& at, float size) {
        auto target = Framebuffer::create(64, 64);
        EXPECT_TRUE(target.ok());
        Camera camera = planCamera(64, 64);
        TaskPool pool(0);
        DrawList list;
        list.addPoint(list.addVertex(at, kGreen), size);
        Rasterizer rasterizer;
        const auto stats = rasterizer.render(list, camera, *target, serialOptions(pool));
        EXPECT_TRUE(stats.ok());
        EXPECT_EQ(stats->pointsSubmitted, 1u);
        return countPixels(*target, kGreen);
    };
    EXPECT_EQ(drawn(Vec3(0.5, 0.5, 0.0), 5.0f), 25u);
    EXPECT_EQ(drawn(Vec3(0.0, 0.0, 0.0), 4.0f), 16u);
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
