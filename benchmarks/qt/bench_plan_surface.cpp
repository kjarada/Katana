// What a plan-view frame of a large surface costs through the one painter
// (paintPlan; docs/surface_display.md "Speed"): a 401 408-triangle rolling
// TIN at 1600 x 1000, at extents and zoomed in five and twenty-five times,
// in the styles that cost the most differently. A frame is a real frame: the
// view pans a pixel each iteration, as in bench_plan_paint.cpp, with the
// painter's cache (the cull grid, the traced contours) kept as the view
// keeps it.
//
//   BM_PlanPaintSurface/<style>/<zoom>
//
// styles: 0 boundary, 1 triangles, 2 contours (1 m, labelled majors), 3
// elevation bands, 4 everything at once. zoom: 1, 5, 25.

#include <benchmark/benchmark.h>

#include <cmath>
#include <memory>
#include <vector>

#include <QImage>
#include <QPainter>

#include "katana/cad/terrain/surface_style.hpp"
#include "katana/entity/model.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "plan_painter.hpp"

namespace {

using katana::cad::SurfaceDisplayStyle;
using katana::cad::SurfaceFaces;

constexpr int kCells = 448;
constexpr double kSide = 1000.0;
constexpr int kWidth = 1600;
constexpr int kHeight = 1000;

std::shared_ptr<const katana::terrain::TinSurface> rollingSurface()
{
    static const auto surface = [] {
        std::vector<katana::geometry::Point3> vertices;
        std::vector<katana::terrain::TinTriangle> triangles;
        const double step = kSide / kCells;
        for (int j = 0; j <= kCells; ++j) {
            for (int i = 0; i <= kCells; ++i) {
                const double x = i * step;
                const double y = j * step;
                vertices.emplace_back(300000.0 + x, 6250000.0 + y,
                                      30.0 + 10.0 * std::sin(x / 80.0) * std::cos(y / 60.0) + 0.02 * x);
            }
        }
        const auto at = [](int i, int j) { return static_cast<std::uint32_t>(j * (kCells + 1) + i); };
        for (int j = 0; j < kCells; ++j) {
            for (int i = 0; i < kCells; ++i) {
                triangles.push_back({at(i, j), at(i + 1, j), at(i + 1, j + 1)});
                triangles.push_back({at(i, j), at(i + 1, j + 1), at(i, j + 1)});
            }
        }
        auto made = katana::terrain::TinSurface::create(std::move(vertices), std::move(triangles));
        return std::make_shared<const katana::terrain::TinSurface>(std::move(*made));
    }();
    return surface;
}

SurfaceDisplayStyle styleNumber(int which)
{
    SurfaceDisplayStyle style;
    style.name = "bench";
    switch (which) {
    case 1:
        style.showTriangles = true;
        break;
    case 2:
        style.contours.show = true;
        style.contours.interval = 1.0;
        style.contours.majorEvery = 5;
        style.contours.labelMajor = true;
        break;
    case 3:
        style.planFaces = SurfaceFaces::Elevation;
        style.banded = true;
        break;
    case 4:
        style.planFaces = SurfaceFaces::Elevation;
        style.banded = true;
        style.showTriangles = true;
        style.showPoints = true;
        style.contours.show = true;
        style.contours.labelMajor = true;
        break;
    default:
        break;
    }
    return style;
}

void BM_PlanPaintSurface(benchmark::State& state)
{
    const auto surface = rollingSurface();
    katana::cad::SurfaceStyleSet styles;
    styles.styles = {styleNumber(static_cast<int>(state.range(0)))};
    styles.defaultStyle = "bench";
    katana::terrain::SurfaceStore store;
    (void)store.add({"ground", surface, "benchmark"});
    static const katana::entity::Model empty;
    katana::qt::PlanSource source;
    source.model = &empty;
    source.surfaces = &store;
    source.surfaceStyles = &styles;
    katana::qt::PlanFrame frame;
    frame.transform.resize(kWidth, kHeight);
    frame.transform.fit(surface->bounds(), 0.05);
    frame.transform.scale *= static_cast<double>(state.range(1));
    katana::qt::PlanPaintOptions options;
    options.thinLines = true;
    katana::qt::PlanPaintCache cache;
    QImage image(kWidth, kHeight, QImage::Format_ARGB32_Premultiplied);
    katana::qt::PlanPaintStats stats;
    int step = 0;
    for (auto _ : state) {
        // A pixel one way, then back: nothing is the same as last frame.
        frame.transform.panByPixels((step++ % 2 == 0) ? 1.0 : -1.0, 0.0);
        image.fill(0xff1e2329u);
        QPainter painter(&image);
        stats = katana::qt::paintPlan(painter, source, frame, options, cache);
        painter.end();
        benchmark::DoNotOptimize(image.constBits());
    }
    state.counters["inView"] = static_cast<double>(stats.surfaceTrianglesInView);
    state.counters["stroked"] = static_cast<double>(stats.surfaceEdgesStroked);
    state.counters["tinted"] = static_cast<double>(stats.surfaceTrianglesTinted);
    state.counters["covered"] = static_cast<double>(stats.surfacePixelsCovered);
    state.counters["contours"] = static_cast<double>(stats.surfaceContourRuns);
}

} // namespace

BENCHMARK(BM_PlanPaintSurface)
    ->ArgsProduct({{0, 1, 2, 3, 4}, {1, 5, 25}})
    ->ArgNames({"style", "zoom"})
    ->Unit(benchmark::kMillisecond);
