// What a plan-view frame of a point cloud costs: synthetic clouds of two, ten
// and twenty million points in random order at zoom extents, one zoomed in,
// and the owner's LAS cloud archive when KATANA_BENCH_CLOUD_12DA names it.
//
// Driven through paintPlan() and nothing newer, so that the SAME source
// measures the painter before the cloud splat was vectorised and after, and
// tools/compare_benchmarks.py --alternate can run the two builds side by side.
// A frame is a real frame: the view pans one pixel each way between
// iterations, as in bench_plan_paint.cpp.

#include <benchmark/benchmark.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <memory>
#include <utility>

#include <QImage>
#include <QPainter>

#include "katana/interop/archive12d.hpp"
#include "katana/interop/reference_data.hpp"
#include "plan_painter.hpp"

namespace {

using katana::geometry::Box2;
using katana::geometry::Point2;

constexpr int kWidth = 1600;
constexpr int kHeight = 1000;

// A survey-sized block at map-grid coordinates, 1500 m by 1000 m, in random
// order as a LAS file's returns are: the splat's writes land anywhere in the
// image, which is what makes it memory-bound.
katana::interop::PointCloudLayer syntheticCloud(std::size_t count)
{
    katana::interop::PointCloudLayer cloud;
    cloud.name = "synthetic";
    cloud.points.resize(count);
    std::uint64_t state = 0x9e3779b97f4a7c15ULL;
    const auto next = [&state] {
        state = state * 6364136223846793005ULL + 1442695040888963407ULL;
        return static_cast<double>(state >> 11) * 0x1.0p-53;
    };
    for (auto& point : cloud.points) {
        point.x = 300000.0 + 1500.0 * next();
        point.y = 6250000.0 + 1000.0 * next();
        point.z = 40.0 + 10.0 * std::sin(point.x * 0.01) + 2.0 * next();
        point.intensity = 1000.0 * next();
        cloud.bounds.minX = std::min(cloud.bounds.minX, point.x);
        cloud.bounds.minY = std::min(cloud.bounds.minY, point.y);
        cloud.bounds.minZ = std::min(cloud.bounds.minZ, point.z);
        cloud.bounds.maxX = std::max(cloud.bounds.maxX, point.x);
        cloud.bounds.maxY = std::max(cloud.bounds.maxY, point.y);
        cloud.bounds.maxZ = std::max(cloud.bounds.maxZ, point.z);
    }
    cloud.sourcePointCount = count;
    return cloud;
}

void paintFrames(benchmark::State& state, const katana::interop::ReferenceData& reference,
                 const Box2& extent, double zoom)
{
    katana::qt::PlanFrame frame;
    frame.transform.resize(kWidth, kHeight);
    frame.transform.fit(extent, 0.02);
    frame.transform.scale *= zoom;
    katana::qt::PlanSource source;
    source.reference = &reference;
    katana::qt::PlanPaintOptions options;
    katana::qt::PlanPaintCache cache;
    QImage image(kWidth, kHeight, QImage::Format_ARGB32_Premultiplied);
    double direction = 1.0;
    for (auto _ : state) {
        frame.transform.panByPixels(direction, 0.0);
        direction = -direction;
        QPainter painter(&image);
        painter.fillRect(image.rect(), QColor(0x1e, 0x23, 0x29));
        (void)katana::qt::paintPlan(painter, source, frame, options, cache);
    }
}

void BM_PlanCloud(benchmark::State& state, double zoom)
{
    katana::interop::ReferenceData reference;
    (void)reference.add(syntheticCloud(static_cast<std::size_t>(state.range(0))));
    const Box2 extent = reference.pointClouds().front().worldBounds();
    paintFrames(state, reference, extent, zoom);
    state.counters["points"] = static_cast<double>(state.range(0));
}

// The owner's archive: set KATANA_BENCH_CLOUD_12DA to its path (a copy - it is
// real data and is never committed).
void BM_PlanCloudArchive(benchmark::State& state)
{
    const char* path = std::getenv("KATANA_BENCH_CLOUD_12DA");
    if (path == nullptr || !std::filesystem::exists(path)) {
        state.SkipWithMessage("KATANA_BENCH_CLOUD_12DA does not name an archive");
        return;
    }
    auto imported = katana::interop::importArchive12d(path);
    if (!imported || imported->clouds.empty()) {
        state.SkipWithError("the archive holds no point cloud");
        return;
    }
    katana::interop::ReferenceData reference;
    Box2 extent;
    std::size_t points = 0;
    for (auto& cloud : imported->clouds) {
        points += cloud.points.size();
        extent.expand(cloud.worldBounds().min);
        extent.expand(cloud.worldBounds().max);
        (void)reference.add(std::move(cloud));
    }
    paintFrames(state, reference, extent, 1.0);
    state.counters["points"] = static_cast<double>(points);
}

} // namespace

BENCHMARK_CAPTURE(BM_PlanCloud, extents, 1.0)
    ->Arg(2'000'000)->Arg(10'000'000)->Arg(20'000'000)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
// Zoomed in eight times: most of the cloud is off screen.
BENCHMARK_CAPTURE(BM_PlanCloud, zoomed, 8.0)
    ->Arg(10'000'000)->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK(BM_PlanCloudArchive)->Unit(benchmark::kMicrosecond)->UseRealTime();
