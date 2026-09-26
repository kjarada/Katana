// The plan view's point clouds through paintPlan: what drawing from the float
// display copy changes against the all-double splat it replaced, that a frame
// is the same at both SIMD levels, that the kept layer image carries nothing
// from one frame into the next, and what the point budget draws.

#include <gtest/gtest.h>

#include <cmath>
#include <cstdint>
#include <limits>
#include <vector>

#include <QImage>
#include <QPainter>

#include "katana/core/cpu_features.hpp"
#include "katana/interop/reference_data.hpp"
#include "plan_painter.hpp"

using katana::core::SimdLevel;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::interop::PointCloudLayer;
using katana::interop::PointColorMode;
using katana::qt::PlanFrame;
using katana::qt::PlanPaintCache;
using katana::qt::PlanPaintOptions;
using katana::qt::PlanPaintStats;
using katana::qt::PlanSource;
using katana::qt::paintPlan;

namespace {

constexpr int kWidth = 800;
constexpr int kHeight = 500;

// 300,000 points over 1500 m by 1000 m at map-grid coordinates, in random
// order, as a LAS file's returns are.
PointCloudLayer cloudOf(PointColorMode mode)
{
    PointCloudLayer cloud;
    cloud.colorMode = mode;
    cloud.points.resize(300'000);
    std::uint64_t state = 7;
    const auto next = [&state] {
        state = state * 6364136223846793005ULL + 1442695040888963407ULL;
        return static_cast<double>(state >> 11) * 0x1.0p-53;
    };
    for (auto& p : cloud.points) {
        p.x = 300000.0 + 1500.0 * next();
        p.y = 6250000.0 + 1000.0 * next();
        p.z = 40.0 + 20.0 * next();
        cloud.bounds.minX = std::min(cloud.bounds.minX, p.x);
        cloud.bounds.minY = std::min(cloud.bounds.minY, p.y);
        cloud.bounds.minZ = std::min(cloud.bounds.minZ, p.z);
        cloud.bounds.maxX = std::max(cloud.bounds.maxX, p.x);
        cloud.bounds.maxY = std::max(cloud.bounds.maxY, p.y);
        cloud.bounds.maxZ = std::max(cloud.bounds.maxZ, p.z);
    }
    cloud.sourcePointCount = cloud.points.size();
    return cloud;
}

PlanFrame frameAround(const Box2& extent)
{
    PlanFrame frame;
    frame.transform.resize(kWidth, kHeight);
    frame.transform.fit(extent, 0.02);
    return frame;
}

QImage paint(const katana::interop::ReferenceData& reference, const PlanFrame& frame,
             const PlanPaintOptions& options, PlanPaintCache& cache, PlanPaintStats* stats = nullptr)
{
    QImage image(kWidth, kHeight, QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::transparent);
    QPainter painter(&image);
    PlanSource source;
    source.reference = &reference;
    const PlanPaintStats s = paintPlan(painter, source, frame, options, cache);
    if (stats != nullptr) {
        *stats = s;
    }
    return image;
}

// The splat as it was before the display copy, for points of one size:
// double coordinates straight from the stored points, in the file's order.
std::vector<bool> doubleSplat(const PointCloudLayer& cloud, const PlanFrame& frame)
{
    std::vector<bool> lit(static_cast<std::size_t>(kWidth * kHeight), false);
    const double s = frame.transform.scale;
    for (const auto& point : cloud.points) {
        const double sx = 0.5 * kWidth + (point.x - frame.transform.center.x) * s;
        const double sy = 0.5 * kHeight - (point.y - frame.transform.center.y) * s;
        if (!(sx > -1.0e6 && sx < 1.0e6 && sy > -1.0e6 && sy < 1.0e6)) {
            continue;
        }
        const int px = static_cast<int>(sx);
        const int py = static_cast<int>(sy);
        if (px >= 0 && py >= 0 && px < kWidth && py < kHeight) {
            lit[static_cast<std::size_t>(py * kWidth + px)] = true;
        }
    }
    return lit;
}

bool litNear(const std::vector<bool>& lit, int x, int y)
{
    for (int dy = -1; dy <= 1; ++dy) {
        for (int dx = -1; dx <= 1; ++dx) {
            const int nx = x + dx;
            const int ny = y + dy;
            if (nx >= 0 && ny >= 0 && nx < kWidth && ny < kHeight &&
                lit[static_cast<std::size_t>(ny * kWidth + nx)]) {
                return true;
            }
        }
    }
    return false;
}

class LevelScope {
  public:
    explicit LevelScope(SimdLevel level) : previous_(katana::core::activeSimdLevel())
    {
        ok_ = static_cast<bool>(katana::core::setSimdLevel(level));
    }
    ~LevelScope() { (void)katana::core::setSimdLevel(previous_); }
    LevelScope(const LevelScope&) = delete;
    LevelScope& operator=(const LevelScope&) = delete;
    [[nodiscard]] bool ok() const { return ok_; }

  private:
    SimdLevel previous_;
    bool ok_ = false;
};

} // namespace

TEST(PlanPainterCloud, FloatOffsetsMoveAFewPointsOnPixelEdgesByOnePixelAndNoneFurther)
{
    // One colour, so which point is on top cannot show: only where each point
    // lands. At this view a pixel is about 1.9 m; the stored offsets are
    // within 750 m of the origin, where a float's spacing is at most 2^-14 m,
    // so storing one moves it at most 0.03 mm, 1.6e-5 of a pixel. A point
    // changes pixel only when it lies that close to a pixel edge, and then by
    // one pixel.
    katana::interop::ReferenceData reference;
    (void)reference.add(cloudOf(PointColorMode::Flat));
    const PointCloudLayer& cloud = reference.pointClouds().front();
    const PlanFrame frame = frameAround(cloud.worldBounds());
    PlanPaintOptions options;
    options.cloudPointBudget = std::numeric_limits<std::size_t>::max();
    PlanPaintCache cache;
    const QImage image = paint(reference, frame, options, cache);
    std::vector<bool> drawn(static_cast<std::size_t>(kWidth * kHeight), false);
    for (int y = 0; y < kHeight; ++y) {
        for (int x = 0; x < kWidth; ++x) {
            drawn[static_cast<std::size_t>(y * kWidth + x)] = qAlpha(image.pixel(x, y)) != 0;
        }
    }
    const std::vector<bool> expected = doubleSplat(cloud, frame);
    int differing = 0;
    int lit = 0;
    for (int y = 0; y < kHeight; ++y) {
        for (int x = 0; x < kWidth; ++x) {
            const auto at = static_cast<std::size_t>(y * kWidth + x);
            lit += expected[at] ? 1 : 0;
            if (drawn[at] != expected[at]) {
                ++differing;
                EXPECT_TRUE(drawn[at] ? litNear(expected, x, y) : litNear(drawn, x, y))
                    << "pixel " << x << ", " << y << " moved more than one pixel";
            }
        }
    }
    EXPECT_GT(lit, 100'000);
    // Measured: 3 of 200,639 lit pixels. One in ten thousand (20 here) is room
    // for that, and still fails if the offsets lose precision - stored from a
    // far origin, or projected in float.
    EXPECT_LE(differing, lit / 10'000) << differing << " of " << lit << " lit pixels differ";
}

TEST(PlanPainterCloud, AFrameIsTheSameAtBothSimdLevels)
{
    // The kernel level is AVX2 on x86-64 or NEON on 64-bit ARM.
    const SimdLevel kernel = katana::core::detectedSimdLevel();
    if (kernel == SimdLevel::Scalar) {
        GTEST_SKIP() << "no SIMD kernels run on this processor: only the scalar path can run here";
    }
    katana::interop::ReferenceData reference;
    (void)reference.add(cloudOf(PointColorMode::Elevation));
    PlanFrame frame = frameAround(reference.pointClouds().front().worldBounds());
    frame.transform.scale *= 1.7;
    PlanPaintOptions options;
    options.cloudPointBudget = 120'000;
    QImage images[2];
    const SimdLevel levels[2] = {SimdLevel::Scalar, kernel};
    for (int i = 0; i < 2; ++i) {
        const LevelScope scope(levels[i]);
        ASSERT_TRUE(scope.ok());
        PlanPaintCache cache;
        images[i] = paint(reference, frame, options, cache);
    }
    EXPECT_TRUE(images[0] == images[1]);
}

TEST(PlanPainterCloud, TheKeptLayerImageCarriesNothingFromOneFrameIntoTheNext)
{
    // A frame after a zoom and a pan, drawn with the cache the frame before
    // used, is the frame a fresh cache draws.
    katana::interop::ReferenceData reference;
    (void)reference.add(cloudOf(PointColorMode::Elevation));
    PlanFrame frame = frameAround(reference.pointClouds().front().worldBounds());
    PlanPaintOptions options;
    PlanPaintCache kept;
    (void)paint(reference, frame, options, kept);
    frame.transform.scale *= 3.0;
    frame.transform.panByPixels(37.0, -21.0);
    const QImage second = paint(reference, frame, options, kept);
    PlanPaintCache fresh;
    EXPECT_TRUE(second == paint(reference, frame, options, fresh));
}

TEST(PlanPainterCloud, OverThePointBudgetAFrameDrawsTheBudgetsShareOfThePointsInView)
{
    katana::interop::ReferenceData reference;
    (void)reference.add(cloudOf(PointColorMode::Elevation));
    const PlanFrame frame = frameAround(reference.pointClouds().front().worldBounds());
    PlanPaintOptions options;
    options.cloudPointBudget = 30'000;
    PlanPaintCache cache;
    PlanPaintStats stats;
    (void)paint(reference, frame, options, cache, &stats);
    EXPECT_EQ(stats.cloudPointsInView, 300'000U);
    // Each of the ~290 tiles (300,000 / 1024) rounds its share up.
    EXPECT_GE(stats.cloudPointsDrawn, 30'000U);
    EXPECT_LE(stats.cloudPointsDrawn, 30'000U + 300U);
    options.cloudPointBudget = 300'000;
    (void)paint(reference, frame, options, cache, &stats);
    EXPECT_EQ(stats.cloudPointsDrawn, 300'000U);
}
