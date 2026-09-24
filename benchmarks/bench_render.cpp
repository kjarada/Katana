// Software rasteriser cost (PLAN.MD Phase 15, section 32).
//
// The rasteriser had pixel tests and no timing until its clipping was rewritten
// (2026-09-23: true near plane and a guard band instead of w > 1e-6), which put
// work on the per-triangle path that every frame pays. This is where that cost,
// and any later change to the renderer, is measured.
//
// The scene is ground, because ground is what the 3D view spends its time on:
// a regular grid with a gentle roll, two triangles per cell, 1920x1080. Two
// camera positions, because they take different paths:
//
//   Framed   - the whole grid in view from an isometric eye. Every vertex is
//              inside the clip volume, so this is the pass-through path and
//              measures what the clip test costs when it finds nothing to do.
//   Within   - the eye low over the middle of the grid, as when walking a TIN.
//              Triangles cross the near plane behind and below the eye and the
//              guard band at the sides, so this measures the clipping itself.
//              Before the rewrite most of the crossing triangles were DROPPED
//              (the defect it fixed), so the old timing for this case did less
//              work and is not a like-for-like baseline.

#include <benchmark/benchmark.h>

#include <cmath>
#include <cstdint>
#include <cstdlib>

#include <fstream>
#include <iterator>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/cad/scene.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/task_pool.hpp"
#include "katana/render/rasterizer.hpp"

using katana::core::TaskPool;
using katana::math::Vec3;
using katana::render::Camera;
using katana::render::DrawList;
using katana::render::Framebuffer;
using katana::render::Projection;
using katana::render::Rasterizer;
using katana::render::RenderOptions;
using katana::render::rgba;

namespace {

constexpr int kWidth = 1920;
constexpr int kHeight = 1080;
constexpr double kGridSize = 1000.0; // metres across

// cells x cells squares over [-500, 500]^2, two triangles each. The roll is a
// few metres, so depth varies across the frame the way real ground does and
// the depth test is exercised rather than trivially passing.
DrawList groundGrid(int cells)
{
    DrawList list;
    const double step = kGridSize / static_cast<double>(cells);
    const int side = cells + 1;
    list.positions.reserve(static_cast<std::size_t>(side) * static_cast<std::size_t>(side));
    for (int j = 0; j < side; ++j) {
        for (int i = 0; i < side; ++i) {
            const double x = -kGridSize * 0.5 + step * static_cast<double>(i);
            const double y = -kGridSize * 0.5 + step * static_cast<double>(j);
            const double z = 3.0 * std::sin(x * 0.01) * std::cos(y * 0.013);
            list.addVertex(Vec3(x, y, z),
                           rgba(static_cast<std::uint8_t>(60 + (i * 7) % 120),
                                static_cast<std::uint8_t>(90 + (j * 5) % 120), 70));
        }
    }
    for (int j = 0; j < cells; ++j) {
        for (int i = 0; i < cells; ++i) {
            const auto v0 = static_cast<katana::render::VertexIndex>(j * side + i);
            const auto v1 = v0 + 1;
            const auto v2 = v0 + static_cast<katana::render::VertexIndex>(side) + 1;
            const auto v3 = v0 + static_cast<katana::render::VertexIndex>(side);
            list.addTriangle(v0, v1, v2);
            list.addTriangle(v0, v2, v3);
        }
    }
    return list;
}

Camera framedCamera(const DrawList& list)
{
    Camera camera;
    camera.setViewportSize(kWidth, kHeight);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(katana::render::StandardView::IsoSouthWest);
    if (!camera.frame(list.bounds())) {
        std::abort(); // a benchmark of an unframed scene measures nothing
    }
    return camera;
}

// Two metres above the middle of the grid, looking north and 15 degrees down:
// ground runs from under the eye to the horizon and far out to both sides.
Camera withinCamera()
{
    Camera camera;
    camera.setViewportSize(kWidth, kHeight);
    camera.setProjection(Projection::Perspective);
    camera.setTarget(Vec3(0.0, 30.0, -6.0));
    camera.setOrientation(-1.5707963267948966, 0.2617993877991494); // eye south, 15 deg up
    camera.setDistance(31.0);
    camera.setDepthRange(0.1, 2000.0);
    return camera;
}

void renderLoop(benchmark::State& state, const DrawList& list, const Camera& camera,
                TaskPool& pool)
{
    auto target = Framebuffer::create(kWidth, kHeight);
    if (!target.ok()) {
        state.SkipWithError("framebuffer");
        return;
    }
    Rasterizer rasterizer; // reused across frames, as a viewport does
    RenderOptions options;
    options.pool = &pool;
    std::uint64_t fragments = 0;
    for (auto _ : state) {
        const auto stats = rasterizer.render(list, camera, *target, options);
        if (!stats.ok()) {
            state.SkipWithError("render failed");
            return;
        }
        fragments = stats->fragments;
        benchmark::DoNotOptimize(target->color().data());
    }
    state.SetItemsProcessed(static_cast<std::int64_t>(state.iterations()) *
                            static_cast<std::int64_t>(list.triangles.size()));
    state.counters["triangles"] = static_cast<double>(list.triangles.size());
    state.counters["fragments"] = static_cast<double>(fragments);
}

void BM_RenderGroundFramed(benchmark::State& state)
{
    const DrawList list = groundGrid(static_cast<int>(state.range(0)));
    TaskPool pool;
    renderLoop(state, list, framedCamera(list), pool);
}
// 256 cells = 131k triangles; 724 cells = 1.05M.
BENCHMARK(BM_RenderGroundFramed)->Arg(256)->Arg(724)->Unit(benchmark::kMillisecond);

// Serial, so the number does not depend on how busy the other cores are.
void BM_RenderGroundFramedSerial(benchmark::State& state)
{
    const DrawList list = groundGrid(static_cast<int>(state.range(0)));
    TaskPool pool(0);
    renderLoop(state, list, framedCamera(list), pool);
}
BENCHMARK(BM_RenderGroundFramedSerial)->Arg(256)->Arg(724)->Unit(benchmark::kMillisecond);

void BM_RenderGroundWithin(benchmark::State& state)
{
    const DrawList list = groundGrid(static_cast<int>(state.range(0)));
    TaskPool pool;
    renderLoop(state, list, withinCamera(), pool);
}
BENCHMARK(BM_RenderGroundWithin)->Arg(256)->Arg(724)->Unit(benchmark::kMillisecond);

// ---- the whole scene: build and frame ---------------------------------------------
//
// The rasteriser cases above draw a bare grid of triangles. What the 3D view
// actually pays for is the SCENE: cad::SceneBuilder turning a surface, the
// drawing and the navigation grid into a DrawList, then one frame of it. The
// 3D view had no benchmark for either (docs/performance.md), so the maps of
// 2026-09-24 disagreed 3-5x about the same archive. This is the committed one.
//
// A synthetic survey: a rolling TIN of cells x cells squares over 1 km with its
// origin at MGA-like coordinates (the double transform has to earn its keep),
// 400 3D strings with per-vertex heights, 400 plan strings with none, and 400
// points - the mix a 12da archive brings. Built through the public
// SceneBuilder::build and rendered through a camera framed on sceneBounds at
// 1600x1000, which is what a 3D cell of that size shows at zoom extents.


constexpr double kEast = 300000.0;
constexpr double kNorth = 6250000.0;

struct SyntheticSurvey {
    katana::cad::Document document;
    katana::terrain::TinSurface surface;
    std::vector<katana::cad::SceneSurface> surfaces;
};

double rolling(double x, double y) { return 40.0 + 6.0 * std::sin(x * 0.012) * std::cos(y * 0.009); }

std::unique_ptr<SyntheticSurvey> syntheticSurvey(int cells)
{
    auto survey = std::make_unique<SyntheticSurvey>();
    const double step = kGridSize / static_cast<double>(cells);
    const int side = cells + 1;
    std::vector<katana::geometry::Point3> vertices;
    vertices.reserve(static_cast<std::size_t>(side) * static_cast<std::size_t>(side));
    for (int j = 0; j < side; ++j) {
        for (int i = 0; i < side; ++i) {
            const double x = step * static_cast<double>(i);
            const double y = step * static_cast<double>(j);
            vertices.emplace_back(kEast + x, kNorth + y, rolling(x, y));
        }
    }
    std::vector<katana::terrain::TinTriangle> triangles;
    triangles.reserve(static_cast<std::size_t>(cells) * static_cast<std::size_t>(cells) * 2);
    for (int j = 0; j < cells; ++j) {
        for (int i = 0; i < cells; ++i) {
            const auto v0 = static_cast<std::uint32_t>(j * side + i);
            const auto v1 = v0 + 1;
            const auto v2 = v0 + static_cast<std::uint32_t>(side) + 1;
            const auto v3 = v0 + static_cast<std::uint32_t>(side);
            triangles.push_back({v0, v1, v2}); // counter-clockwise in plan
            triangles.push_back({v0, v2, v3});
        }
    }
    auto surface = katana::terrain::TinSurface::create(std::move(vertices), std::move(triangles));
    if (!surface.ok()) {
        std::abort();
    }
    survey->surface = std::move(*surface);
    katana::cad::SceneSurface item;
    item.name = "Existing";
    item.surface = &survey->surface;
    survey->surfaces.push_back(item);

    std::vector<katana::entity::Entity> entities;
    for (int k = 0; k < 400; ++k) {
        // A 3D string across the site, surveyed every 5 m.
        katana::entity::Entity string3d;
        katana::geometry::Polyline2 path;
        std::vector<std::optional<double>> heights;
        const double y = 2.5 * static_cast<double>(k);
        for (double x = 0.0; x <= 400.0; x += 5.0) {
            path.vertices.emplace_back(kEast + x + 300.0, kNorth + y);
            heights.emplace_back(rolling(x + 300.0, y) + 0.2);
        }
        string3d.geometry = path;
        katana::entity::setHeights(string3d.properties, heights);
        entities.push_back(std::move(string3d));

        // A plan string with no heights: a lot boundary of 12 vertices.
        katana::entity::Entity plan;
        katana::geometry::Polyline2 lot;
        const double ox = 50.0 + 20.0 * static_cast<double>(k % 20);
        const double oy = 50.0 + 20.0 * static_cast<double>(k / 20);
        for (int v = 0; v < 12; ++v) {
            const double a = 0.5235987755982988 * static_cast<double>(v);
            lot.vertices.emplace_back(kEast + ox + 8.0 * std::cos(a), kNorth + oy + 8.0 * std::sin(a));
        }
        lot.closed = true;
        plan.geometry = lot;
        entities.push_back(std::move(plan));

        katana::entity::Entity point;
        point.geometry = katana::entity::PointGeometry{
            katana::geometry::Point2(kEast + 13.0 * static_cast<double>(k % 70),
                                     kNorth + 11.0 * static_cast<double>(k / 7))};
        entities.push_back(std::move(point));
    }
    if (!survey->document.execute(katana::commands::createEntities(std::move(entities)))) {
        std::abort();
    }
    return survey;
}

// cells = 256 is 131k triangles (a street); 512 is 524k (a corridor like the
// 229k-triangle archive the maps measured, with room to spare).
void BM_SceneBuild(benchmark::State& state)
{
    const auto survey = syntheticSurvey(static_cast<int>(state.range(0)));
    katana::cad::SceneBuilder builder; // reused across builds, as a view does
    katana::cad::SceneOptions options;
    DrawList list;
    for (auto _ : state) {
        builder.build(survey->document, survey->surfaces, options, list);
        benchmark::DoNotOptimize(list.positions.data());
    }
    state.counters["vertices"] = static_cast<double>(list.positions.size());
    state.counters["triangles"] = static_cast<double>(list.triangles.size());
    state.counters["lines"] = static_cast<double>(list.lines.size());
}
BENCHMARK(BM_SceneBuild)->Arg(256)->Arg(512)->Unit(benchmark::kMillisecond);

void BM_SceneFrame(benchmark::State& state)
{
    const auto survey = syntheticSurvey(static_cast<int>(state.range(0)));
    katana::cad::SceneBuilder builder;
    katana::cad::SceneOptions options;
    DrawList list;
    builder.build(survey->document, survey->surfaces, options, list);

    constexpr int kFrameWidth = 1600;
    constexpr int kFrameHeight = 1000;
    Camera camera;
    camera.setViewportSize(kFrameWidth, kFrameHeight);
    camera.setStandardView(katana::render::StandardView::IsoSouthWest);
    if (!camera.frame(katana::cad::sceneBounds(survey->document, survey->surfaces, options))) {
        std::abort();
    }
    auto target = Framebuffer::create(kFrameWidth, kFrameHeight);
    if (!target.ok()) {
        state.SkipWithError("framebuffer");
        return;
    }
    Rasterizer rasterizer;
    TaskPool pool;
    RenderOptions renderOptions;
    renderOptions.pool = &pool;
    std::uint64_t triangles = 0;
    for (auto _ : state) {
        const auto stats = rasterizer.render(list, camera, *target, renderOptions);
        if (!stats.ok()) {
            state.SkipWithError("render failed");
            return;
        }
        triangles = stats->trianglesRasterised;
        benchmark::DoNotOptimize(target->color().data());
    }
    state.counters["rasterised"] = static_cast<double>(triangles);
}
BENCHMARK(BM_SceneFrame)->Arg(256)->Arg(512)->Unit(benchmark::kMillisecond);

// What a selection click costs the 3D view: the overlay of the selected
// entities alone (SceneBuilder::buildSelection), the terrain and the drawing
// under it built once beforehand as the view keeps them. Compare with
// BM_SceneBuild, which is what every click cost when any document
// notification rebuilt the whole scene.
void BM_SceneSelectionBuild(benchmark::State& state)
{
    const auto survey = syntheticSurvey(static_cast<int>(state.range(0)));
    katana::cad::SceneBuilder builder;
    katana::cad::SceneOptions options;
    katana::cad::SceneLayers layers;
    builder.buildTerrain(survey->surfaces, {}, options, layers);
    builder.buildEntities(survey->document, survey->surfaces, options, layers);
    survey->document.selection().add(survey->document.model().entities.ids().front());
    for (auto _ : state) {
        builder.buildSelection(survey->document, survey->surfaces, options, layers);
        benchmark::DoNotOptimize(layers.selection.positions.data());
    }
    state.counters["lines"] = static_cast<double>(layers.selection.lines.size());
}
BENCHMARK(BM_SceneSelectionBuild)->Arg(256)->Arg(512)->Unit(benchmark::kMillisecond);

// One frame as the 3D view draws it (cad::renderLayers): the depth range
// fitted, the edges faded, and one pass per layer, the grid and the edges
// writing no depth. BM_SceneFrame draws the same scene as ONE list through
// the rasteriser alone; this is what a paint pays for it. cells = 64 is 8,192
// triangles, under kDenseSurfaceTriangles, so the edges pass is drawn too.
void BM_SceneLayersFrame(benchmark::State& state)
{
    const auto survey = syntheticSurvey(static_cast<int>(state.range(0)));
    katana::cad::SceneBuilder builder;
    katana::cad::SceneOptions options;
    katana::cad::SceneLayers layers;
    builder.buildTerrain(survey->surfaces, {}, options, layers);
    builder.buildEntities(survey->document, survey->surfaces, options, layers);
    builder.buildSelection(survey->document, survey->surfaces, options, layers);
    builder.buildGrid(options, layers);

    constexpr int kFrameWidth = 1600;
    constexpr int kFrameHeight = 1000;
    Camera camera;
    camera.setViewportSize(kFrameWidth, kFrameHeight);
    camera.setStandardView(katana::render::StandardView::IsoSouthWest);
    if (!camera.frame(layers.bounds)) {
        std::abort();
    }
    auto target = Framebuffer::create(kFrameWidth, kFrameHeight);
    if (!target.ok()) {
        state.SkipWithError("framebuffer");
        return;
    }
    Rasterizer rasterizer;
    TaskPool pool;
    RenderOptions renderOptions;
    renderOptions.pool = &pool;
    std::uint64_t triangles = 0;
    for (auto _ : state) {
        const auto stats =
            katana::cad::renderLayers(layers, camera, rasterizer, *target, renderOptions);
        if (!stats.ok()) {
            state.SkipWithError("render failed");
            return;
        }
        triangles = stats->trianglesRasterised;
        benchmark::DoNotOptimize(target->color().data());
    }
    state.counters["rasterised"] = static_cast<double>(triangles);
    state.counters["edges"] = static_cast<double>(layers.edges.lines.size());
}
BENCHMARK(BM_SceneLayersFrame)->Arg(64)->Arg(256)->Arg(512)->Unit(benchmark::kMillisecond);

// A real archive's frame, drawn as the 3D view draws it (cad::renderLayers) at
// 1600x1000 from the isometric eye framed on the scene. Synthetic ground is
// uniform; a real survey is not - dense TIN in the middle, long strings, big
// triangles at the hull - and the rasteriser's size cutoffs are only honest
// if they are judged on both. The archive is the owner's and is never
// committed: KATANA_BENCH_FRAME_12DA names it, and unset the benchmark skips.
void BM_ArchiveFrame(benchmark::State& state)
{
    const char* path = std::getenv("KATANA_BENCH_FRAME_12DA");
    if (path == nullptr || *path == 0) {
        state.SkipWithMessage("KATANA_BENCH_FRAME_12DA is not set");
        return;
    }
    std::ifstream in(path, std::ios::binary);
    const std::string bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    auto archive = katana::archive12d::readArchiveBytes(bytes);
    if (!archive) {
        state.SkipWithError("the archive does not read");
        return;
    }
    auto domain = katana::archive12d::toDomain(archive.value());
    if (!domain) {
        state.SkipWithError("the archive does not import");
        return;
    }
    katana::cad::Document document;
    for (auto& layer : domain->layersNeeded) {
        (void)document.execute(katana::commands::createLayer(std::move(layer)));
    }
    for (auto& style : domain->stylesNeeded) {
        (void)document.execute(katana::commands::createStyle(std::move(style)));
    }
    (void)document.execute(katana::commands::createEntities(std::move(domain->entities)));
    std::vector<katana::cad::SceneSurface> surfaces;
    for (const auto& imported : domain->surfaces) {
        katana::cad::SceneSurface item;
        item.name = imported.name;
        item.surface = &imported.surface;
        surfaces.push_back(item);
    }
    std::vector<katana::cad::SceneMesh> meshes;
    for (const auto& imported : domain->meshes) {
        katana::cad::SceneMesh item;
        item.name = imported.name;
        item.mesh = &imported.mesh;
        meshes.push_back(item);
    }
    katana::cad::SceneBuilder builder;
    katana::cad::SceneOptions options;
    katana::cad::SceneLayers layers;
    builder.buildTerrain(surfaces, meshes, options, layers);
    builder.buildEntities(document, surfaces, options, layers);
    builder.buildGrid(options, layers);

    constexpr int kFrameWidth = 1600;
    constexpr int kFrameHeight = 1000;
    Camera camera;
    camera.setViewportSize(kFrameWidth, kFrameHeight);
    camera.setStandardView(katana::render::StandardView::IsoSouthWest);
    if (!camera.frame(layers.bounds)) {
        state.SkipWithError("nothing to frame");
        return;
    }
    auto target = Framebuffer::create(kFrameWidth, kFrameHeight);
    if (!target.ok()) {
        state.SkipWithError("framebuffer");
        return;
    }
    Rasterizer rasterizer;
    TaskPool pool;
    RenderOptions renderOptions;
    renderOptions.pool = &pool;
    std::uint64_t triangles = 0;
    std::uint64_t fragments = 0;
    for (auto _ : state) {
        const auto stats =
            katana::cad::renderLayers(layers, camera, rasterizer, *target, renderOptions);
        if (!stats.ok()) {
            state.SkipWithError("render failed");
            return;
        }
        triangles = stats->trianglesRasterised;
        fragments = stats->fragments;
        benchmark::DoNotOptimize(target->color().data());
    }
    state.counters["rasterised"] = static_cast<double>(triangles);
    state.counters["fragments"] = static_cast<double>(fragments);
}
BENCHMARK(BM_ArchiveFrame)->Unit(benchmark::kMillisecond);

} // namespace
