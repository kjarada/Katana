// What drawing a surface in plan costs (cad::surfacePlan,
// docs/surface_display.md "Speed"): a 401 408-triangle rolling TIN - 448 x
// 448 cells over a kilometre at map-grid coordinates - in the styles that
// cost the most differently, through a 1600 x 1000 view at extents and zoomed
// in five and twenty-five times about its centre.
//
//   BM_SurfacePlan/<style>/<zoom>   a paint with the cache warm: the cull,
//                                   the level of detail, the image, the
//                                   contours' runs and labels
//   BM_SurfacePlanFirst/<style>     the first paint, which also builds the
//                                   cull grid and traces the contours
//   BM_SurfaceSceneStyled/<style>   the 3D view's terrain built in the style
//
// styles: 0 boundary, 1 triangles, 2 contours (1 m, labelled majors), 3
// elevation bands, 4 smooth elevation, 5 slope bands, 6 everything at once.
// zoom: 1, 5, 25. Counters say what each paint drew.

#include <benchmark/benchmark.h>

#include <cmath>
#include <memory>
#include <vector>

#include "katana/cad/scene.hpp"
#include "katana/cad/terrain/surface_plan.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace {

using katana::cad::SurfaceDisplayStyle;
using katana::cad::SurfaceFaces;
using katana::geometry::Box2;
using katana::geometry::Point2;

constexpr int kCells = 448; // 2 x 448 x 448 = 401 408 triangles
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
        break;
    case 5:
        style.planFaces = SurfaceFaces::Slope;
        style.banded = true;
        style.stops = {{0, {0, 160, 60, 255}},   {2, {120, 200, 60, 255}}, {5, {230, 220, 60, 255}},
                       {10, {240, 160, 40, 255}}, {20, {230, 80, 30, 255}}, {33, {180, 20, 20, 255}}};
        break;
    case 6:
        style.planFaces = SurfaceFaces::Elevation;
        style.banded = true;
        style.showTriangles = true;
        style.showPoints = true;
        style.showBreaklines = true;
        style.contours.show = true;
        style.contours.labelMajor = true;
        break;
    default:
        break;
    }
    return style;
}

// The plan view's 1600 x 1000 at extents (the kilometre over 950 px, as Zoom
// Extents leaves a margin) and `zoom` times closer about the centre.
katana::cad::SurfacePlanView viewAt(double zoom, const Box2& bounds)
{
    const double unitsPerPixel = kSide / 950.0 / zoom;
    const Point2 centre = bounds.center();
    katana::cad::SurfacePlanView view;
    view.visible = Box2(Point2(centre.x - 0.5 * kWidth * unitsPerPixel, centre.y - 0.5 * kHeight * unitsPerPixel),
                        Point2(centre.x + 0.5 * kWidth * unitsPerPixel, centre.y + 0.5 * kHeight * unitsPerPixel));
    view.unitsPerPixel = unitsPerPixel;
    const auto grid = katana::cad::planImageGrid(view.visible, bounds, unitsPerPixel, 16'000'000);
    view.imageBox = grid.box;
    view.imageWidth = grid.width;
    view.imageHeight = grid.height;
    return view;
}

void count(benchmark::State& state, const katana::cad::SurfacePlan& plan)
{
    state.counters["inView"] = static_cast<double>(plan.stats.trianglesInView);
    state.counters["stroked"] = static_cast<double>(plan.stats.edgesStroked);
    state.counters["tinted"] = static_cast<double>(plan.stats.trianglesTinted);
    state.counters["covered"] = static_cast<double>(plan.stats.pixelsCovered);
    state.counters["contourVertices"] = static_cast<double>(plan.stats.contourVertices);
    state.counters["labels"] = static_cast<double>(plan.stats.labels);
}

void BM_SurfacePlan(benchmark::State& state)
{
    const auto surface = rollingSurface();
    const katana::terrain::NamedSurface named{"ground", surface, "benchmark"};
    const SurfaceDisplayStyle style = styleNumber(static_cast<int>(state.range(0)));
    const auto view = viewAt(static_cast<double>(state.range(1)), surface->bounds());
    katana::cad::SurfacePlanCache cache;
    katana::cad::SurfacePlan plan = katana::cad::surfacePlan(named, style, view, cache); // warm
    for (auto _ : state) {
        plan = katana::cad::surfacePlan(named, style, view, cache);
        benchmark::DoNotOptimize(plan.stats.trianglesInView);
    }
    count(state, plan);
}

void BM_SurfacePlanFirst(benchmark::State& state)
{
    const auto surface = rollingSurface();
    const katana::terrain::NamedSurface named{"ground", surface, "benchmark"};
    const SurfaceDisplayStyle style = styleNumber(static_cast<int>(state.range(0)));
    const auto view = viewAt(1.0, surface->bounds());
    katana::cad::SurfacePlan plan;
    for (auto _ : state) {
        katana::cad::SurfacePlanCache cache;
        plan = katana::cad::surfacePlan(named, style, view, cache);
        benchmark::DoNotOptimize(plan.stats.trianglesInView);
    }
    count(state, plan);
}

void BM_SurfaceSceneStyled(benchmark::State& state)
{
    const auto surface = rollingSurface();
    SurfaceDisplayStyle style = styleNumber(static_cast<int>(state.range(0)));
    style.faces3d = style.planFaces == SurfaceFaces::None ? SurfaceFaces::Elevation : style.planFaces;
    katana::cad::SceneSurface item;
    item.name = "ground";
    item.surface = surface.get();
    katana::cad::applySurfaceDisplay(item, style);
    katana::cad::SceneBuilder builder;
    katana::cad::SceneLayers layers;
    for (auto _ : state) {
        builder.buildTerrain({item}, {}, katana::cad::SceneOptions{}, layers);
        benchmark::DoNotOptimize(layers.terrain.triangles.size());
    }
    state.counters["triangles"] = static_cast<double>(layers.terrain.triangles.size());
}

} // namespace

BENCHMARK(BM_SurfacePlan)
    ->ArgsProduct({{0, 1, 2, 3, 4, 5, 6}, {1, 5, 25}})
    ->ArgNames({"style", "zoom"})
    ->Unit(benchmark::kMillisecond);
BENCHMARK(BM_SurfacePlanFirst)->DenseRange(0, 6)->ArgName("style")->Unit(benchmark::kMillisecond);
// Elevation smooth (4) is the kernels' path; bands (3) and slope (5) the
// scalar per-face paths.
BENCHMARK(BM_SurfaceSceneStyled)->Arg(3)->Arg(4)->Arg(5)->ArgName("style")->Unit(benchmark::kMillisecond);
