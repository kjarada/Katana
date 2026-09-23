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

} // namespace
