// The point-cloud splat (point_splat.hpp): the projection held to its scalar
// loop at every SIMD level, the splat's image the same at every thread count
// and level, which point is on top where points share a pixel, and what the
// point budget draws. The kernel takes four points a step from 8 points, so
// projection lengths run through every remainder past that.

#include <gtest/gtest.h>

#include <array>
#include <cstdint>
#include <limits>
#include <vector>

#include "katana/core/task_pool.hpp"
#include "katana/geometry/point_splat.hpp"
#include "simd_levels.hpp"

using katana::core::SimdLevel;
using katana::core::TaskPool;
using katana::geometry::buildSplatCloud;
using katana::geometry::kOffImage;
using katana::geometry::PixelProjection;
using katana::geometry::Point2;
using katana::geometry::projectToPixels;
using katana::geometry::SplatCloud;
using katana::geometry::splatCloud;
using katana::geometry::SplatView;
using katana::test::atSimdLevel;

namespace {

struct Pixels {
    std::vector<std::int32_t> x;
    std::vector<std::int32_t> y;
    bool operator==(const Pixels&) const = default;
};

Pixels project(const PixelProjection& p, const std::vector<float>& xs, const std::vector<float>& ys)
{
    Pixels out{std::vector<std::int32_t>(xs.size()), std::vector<std::int32_t>(xs.size())};
    projectToPixels(p, xs, ys, out.x, out.y);
    return out;
}

// x, y and a colour, as a cloud's points are stored: an array of structs.
struct Source {
    double x = 0.0;
    double y = 0.0;
    std::uint32_t colour = 0;
};

SplatCloud cloudOf(const std::vector<Source>& points)
{
    std::vector<std::uint32_t> colours;
    for (const Source& s : points) {
        colours.push_back(s.colour);
    }
    return buildSplatCloud(&points.data()->x, &points.data()->y, sizeof(Source), points.size(),
                           colours.data(), TaskPool::shared());
}

std::vector<Source> randomCloud(std::size_t count, double width, double height)
{
    std::vector<Source> points(count);
    std::uint64_t state = 12345;
    const auto next = [&state] {
        state = state * 6364136223846793005ULL + 1442695040888963407ULL;
        return static_cast<double>(state >> 11) * 0x1.0p-53;
    };
    for (std::size_t i = 0; i < count; ++i) {
        points[i].x = 500000.0 + width * next();
        points[i].y = 7000000.0 + height * next();
        points[i].colour = 0xff000000u | static_cast<std::uint32_t>(i * 2654435761u >> 8);
    }
    return points;
}

std::vector<std::uint32_t> splatInto(const SplatCloud& cloud, const SplatView& view, TaskPool& pool,
                                     katana::geometry::SplatStats* stats = nullptr)
{
    std::vector<std::uint32_t> image(static_cast<std::size_t>(view.width * view.height), 0);
    const auto s = splatCloud(cloud, view, image.data(), static_cast<std::size_t>(view.width), pool);
    if (stats != nullptr) {
        *stats = s;
    }
    return image;
}

} // namespace

TEST(ProjectToPixels, APointsPixelIsItsProjectionTruncatedTowardsZeroAndOffTheScaleIsNoPixel)
{
    // Half width 50, half height 40, centre (10, 20), 2 pixels a unit:
    //   (12.25, 19.5): sx = 50 + 2.25 * 2 = 54.5 -> 54, sy = 40 + 0.5 * 2 = 41
    //   (9.75, 20.25): sx = 49.5 -> 49, sy = 39.5 -> 39
    //   (-15.25, 20):  sx = 50 - 25.25 * 2 = -0.5 -> 0 (towards zero, not down)
    //   (-20, 20):     sx = -10
    //   (500010, 20):  sx = 1000050, past 1e6: no pixel for either coordinate
    //   (NaN, 20):     no pixel
    //   x giving sx = -1e6 exactly: outside, the bound is strict
    const PixelProjection p{50.0, 40.0, 10.0, 20.0, 2.0};
    const float nan = std::numeric_limits<float>::quiet_NaN();
    const std::vector<float> xs{12.25F, 9.75F, -15.25F, -20.0F, 500010.0F, nan, -500015.0F, 12.25F};
    const std::vector<float> ys{19.5F, 20.25F, 20.0F, 20.0F, 20.0F, 20.0F, 20.0F, 19.5F};
    const Pixels expected{{54, 49, 0, -10, kOffImage, kOffImage, kOffImage, 54},
                          {41, 39, 40, 40, kOffImage, kOffImage, kOffImage, 41}};
    EXPECT_EQ(atSimdLevel(SimdLevel::Scalar, [&] { return project(p, xs, ys); }), expected);
    if (katana::test::kernelsAvailable()) {
        EXPECT_EQ(atSimdLevel(katana::test::kernelLevel(), [&] { return project(p, xs, ys); }), expected);
    }
}

TEST(ProjectToPixels, EveryLengthGivesTheScalarLoopsPixelsAtTheKernelLevel)
{
    KATANA_REQUIRE_SIMD_KERNELS();
    const float inf = std::numeric_limits<float>::infinity();
    const float nan = std::numeric_limits<float>::quiet_NaN();
    // Pixel edges, zeros of both signs, the 1e6 bounds and what is past them.
    const std::array<float, 16> awkward{0.0F,  -0.0F,     0.5F,  -0.5F, 1.0F,   -1.0F,
                                        3.75F, -3.75F,    inf,   -inf,  nan,    499995.0F,
                                        -499995.0F, 499995.5F, 1.0e30F, -1.0e-30F};
    const PixelProjection p{100.0, 80.0, 0.25, -0.5, 2.0};
    for (std::size_t length = 0; length <= 40; ++length) {
        std::vector<float> xs(length);
        std::vector<float> ys(length);
        for (std::size_t i = 0; i < length; ++i) {
            xs[i] = awkward[(i * 5 + length) % awkward.size()];
            ys[i] = awkward[(i * 3 + 7) % awkward.size()];
        }
        const Pixels scalar = atSimdLevel(SimdLevel::Scalar, [&] { return project(p, xs, ys); });
        const Pixels kernel = atSimdLevel(katana::test::kernelLevel(), [&] { return project(p, xs, ys); });
        EXPECT_EQ(scalar, kernel) << "length " << length;
    }
}

TEST(SplatCloud, TheImageIsTheSameAtEveryThreadCountAndSimdLevel)
{
    // 200,000 points on 640 x 480 pixels: most pixels are hit by several
    // points, so the order they are written in shows.
    const SplatCloud cloud = cloudOf(randomCloud(200'000, 300.0, 200.0));
    for (const int radius : {0, 1}) {
        for (const std::size_t budget : {std::size_t{50'000}, std::size_t{1'000'000}}) {
            SplatView view;
            view.centre = Point2(500150.0, 7000100.0);
            view.scale = 2.3;
            view.width = 640;
            view.height = 480;
            view.radius = radius;
            view.pointBudget = budget;
            TaskPool one(0);
            const auto reference =
                atSimdLevel(SimdLevel::Scalar, [&] { return splatInto(cloud, view, one); });
            for (const std::size_t workers : {1U, 2U, 15U}) {
                TaskPool pool(workers);
                EXPECT_EQ(atSimdLevel(SimdLevel::Scalar, [&] { return splatInto(cloud, view, pool); }),
                          reference)
                    << workers + 1 << " threads, radius " << radius << ", budget " << budget;
                if (katana::test::kernelsAvailable()) {
                    const SimdLevel kernel = katana::test::kernelLevel();
                    EXPECT_EQ(atSimdLevel(kernel, [&] { return splatInto(cloud, view, pool); }),
                              reference)
                        << katana::core::toString(kernel) << ", " << workers + 1
                        << " threads, radius " << radius;
                }
            }
        }
    }
}

TEST(SplatCloud, WherePointsShareAPixelTheLastInTheFilesOrderIsOnTopAndTheBudgetDrawsTheFirst)
{
    // Three points on one pixel: one tile, and each level of detail takes the
    // first of them its cell has not had, so the display order is the file's.
    const SplatCloud cloud =
        cloudOf({{10.0, 10.0, 0xffff0000u}, {10.0, 10.0, 0xff00ff00u}, {10.0, 10.0, 0xff0000ffu}});
    SplatView view;
    view.centre = Point2(10.0, 10.0);
    view.width = 8;
    view.height = 8;
    TaskPool pool(0);
    // The centre pixel is (4, 4): sx = 4 + 0 * 1.
    EXPECT_EQ(splatInto(cloud, view, pool)[4 * 8 + 4], 0xff0000ffu);
    // A budget of one of the three: ceil(3 * 1 / 3) = 1 point, the coarsest.
    view.pointBudget = 1;
    katana::geometry::SplatStats stats;
    EXPECT_EQ(splatInto(cloud, view, pool, &stats)[4 * 8 + 4], 0xffff0000u);
    EXPECT_EQ(stats.pointsInView, 3U);
    EXPECT_EQ(stats.pointsDrawn, 1U);
}

TEST(SplatCloud, OverBudgetEveryPartOfTheCloudStillGetsItsShareOfPoints)
{
    // 100,000 points over 400 x 400 units drawn at a pixel a unit, with a
    // budget of 10,000: each 100 x 100 quarter of a quarter still has points.
    const SplatCloud cloud = cloudOf(randomCloud(100'000, 400.0, 400.0));
    SplatView view;
    view.centre = Point2(500200.0, 7000200.0);
    view.width = 400;
    view.height = 400;
    view.pointBudget = 10'000;
    TaskPool pool(3);
    katana::geometry::SplatStats stats;
    const auto image = splatInto(cloud, view, pool, &stats);
    EXPECT_EQ(stats.pointsInView, 100'000U);
    // Each tile rounds its share up, so at most one point a tile over.
    EXPECT_GE(stats.pointsDrawn, 10'000U);
    EXPECT_LE(stats.pointsDrawn, 10'000U + cloud.tiles.size());
    for (int by = 0; by < 4; ++by) {
        for (int bx = 0; bx < 4; ++bx) {
            int lit = 0;
            for (int y = by * 100; y < (by + 1) * 100; ++y) {
                for (int x = bx * 100; x < (bx + 1) * 100; ++x) {
                    lit += image[static_cast<std::size_t>(y * 400 + x)] != 0 ? 1 : 0;
                }
            }
            // A sixteenth of 10,000 is 625; an even thinning is not far off.
            EXPECT_GT(lit, 400) << "block " << bx << ", " << by;
        }
    }
    view.pointBudget = 100'000;
    (void)splatInto(cloud, view, pool, &stats);
    EXPECT_EQ(stats.pointsDrawn, 100'000U);
}

TEST(SplatCloud, PointsThatCannotBeDrawnAreLeftOutOfTheDisplayCopy)
{
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double inf = std::numeric_limits<double>::infinity();
    const SplatCloud cloud = cloudOf({{1.0, 2.0, 1u}, {nan, 2.0, 2u}, {3.0, inf, 3u}, {5.0, 6.0, 4u}});
    EXPECT_EQ(cloud.sourceCount, 4U);
    ASSERT_EQ(cloud.size(), 2U);
    // The origin is the middle of the drawable points, (3, 4).
    EXPECT_EQ(cloud.origin.x, 3.0);
    EXPECT_EQ(cloud.origin.y, 4.0);
    EXPECT_EQ(cloud.colours[0] + cloud.colours[1], 5u);
}

TEST(SplatCloud, OnlyTheRowsTheCloudCanReachAreClearedAndDrawn)
{
    // 100 x 100 pixels centred on (0, 0) at a pixel a unit: (0, 40) is row
    // 50 - 40 = 10 and (0, 30) row 20, so rows 10 to 20 are the cloud's.
    const SplatCloud cloud = cloudOf({{0.0, 40.0, 0xff111111u}, {0.0, 30.0, 0xff222222u}});
    SplatView view;
    view.width = 100;
    view.height = 100;
    std::vector<std::uint32_t> image(100 * 100, 0xdeadbeefu);
    TaskPool pool(0);
    const auto stats = splatCloud(cloud, view, image.data(), 100, pool);
    EXPECT_EQ(stats.rowBegin, 10);
    EXPECT_EQ(stats.rowEnd, 21);
    EXPECT_EQ(image[9 * 100 + 50], 0xdeadbeefu);
    EXPECT_EQ(image[21 * 100 + 50], 0xdeadbeefu);
    EXPECT_EQ(image[10 * 100 + 50], 0xff111111u);
    EXPECT_EQ(image[20 * 100 + 50], 0xff222222u);
    EXPECT_EQ(image[15 * 100 + 50], 0u);
}
