// GPU frame time against the software rasteriser, same scene, same camera,
// same 1920x1080 (docs/gpu.md, "Measurements").
//
// Three kinds of number, because they answer different questions:
//
//   BM_Gpu*        the GPU's own time for the frame, from QRhi timestamps
//                  (UseManualTime: the reported time IS the GPU time). What
//                  the renderer costs the graphics hardware.
//   BM_Gpu*Wall    wall time from beginning the frame to the GPU finishing
//                  it, the read-back excluded: what a synchronous caller
//                  waits, including recording and submission on the CPU.
//   BM_Cpu*        the software rasteriser on every core (TaskPool::shared,
//                  as the 3D view uses it) - the path the GPU replaces.
//
// Scenes:
//
//   Ground/256, /724  bench_render.cpp's framed ground grid, 131k and 1.05M
//                     triangles: the same grid, camera and size, so the CPU
//                     rows here and there measure the same thing.
//   Scene             a real archive through the application's own scene
//                     builder (default options: surfaces shaded with edges,
//                     baked lighting), framed as the 3D view frames it (SW
//                     isometric, cad::sceneBounds). The file is named by the
//                     KATANA_BENCH_SCENE environment variable - it is survey
//                     data and not in the repository - and the case is
//                     skipped without it. docs/gpu.md records the owner's
//                     'Test 4 with Tin.12da'.
//
// The GPU cases skip when there is no Direct3D 11 hardware device; WARP is
// never timed (it is a test device, not a renderer anyone should use).

#include <benchmark/benchmark.h>

#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include <QGuiApplication>

#include "gpu/offscreen_gpu.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/scene.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/interop/archive12d.hpp"
#include "katana/render/rasterizer.hpp"

using katana::math::Vec3;
using katana::qt::gpu::Expansion;
using katana::qt::gpu::FrameSettings;
using katana::qt::gpu::GpuDevice;
using katana::qt::gpu::OffscreenGpu;
using katana::qt::gpu::OffscreenOptions;
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
constexpr double kGridSize = 1000.0;

// bench_render.cpp's groundGrid, unchanged, so the two benchmarks agree.
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

Camera framedCamera(const katana::math::AABB& box)
{
    Camera camera;
    camera.setViewportSize(kWidth, kHeight);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(katana::render::StandardView::IsoSouthWest);
    if (!camera.frame(box)) {
        std::abort(); // a benchmark of an unframed scene measures nothing
    }
    return camera;
}

// ---- the archive scene, loaded once ------------------------------------------------

struct LoadedScene {
    DrawList list;
    Camera camera;
    std::string error; // why there is no scene; empty when there is one
};

const LoadedScene& archiveScene()
{
    static const LoadedScene scene = [] {
        LoadedScene loaded;
        const char* path = std::getenv("KATANA_BENCH_SCENE");
        if (path == nullptr || *path == '\0') {
            loaded.error = "set KATANA_BENCH_SCENE to a .12da archive to time a real scene";
            return loaded;
        }
        auto imported = katana::interop::importArchive12d(path);
        if (!imported) {
            loaded.error = "import failed: " + imported.error().describe();
            return loaded;
        }
        katana::cad::Document document;
        auto transaction = std::make_unique<katana::commands::Transaction>("IMPORT");
        for (const auto& layer : imported->layersNeeded) {
            if (!document.model().layers.contains(layer.name)) {
                transaction->add(katana::commands::createLayer(layer));
            }
        }
        for (const auto& style : imported->stylesNeeded) {
            if (!document.model().styles.contains(style.name)) {
                transaction->add(katana::commands::createStyle(style));
            }
        }
        if (!imported->entities.empty()) {
            transaction->add(katana::commands::createEntities(std::move(imported->entities)));
            if (auto status = document.execute(std::move(transaction)); !status) {
                loaded.error = "execute failed: " + status.error().describe();
                return loaded;
            }
        }
        // Held for the life of the process: the scene builder reads them.
        static std::vector<std::unique_ptr<katana::terrain::TinSurface>> tins;
        static std::vector<std::unique_ptr<katana::geometry::TriangleMesh>> meshes;
        std::vector<katana::cad::SceneSurface> surfaces;
        std::vector<katana::cad::SceneMesh> sceneMeshes;
        for (auto& surface : imported->surfaces) {
            tins.push_back(std::make_unique<katana::terrain::TinSurface>(std::move(surface.surface)));
            katana::cad::SceneSurface item;
            item.name = surface.name;
            item.surface = tins.back().get();
            surfaces.push_back(item);
        }
        for (auto& mesh : imported->meshes) {
            meshes.push_back(std::make_unique<katana::geometry::TriangleMesh>(std::move(mesh.mesh)));
            katana::cad::SceneMesh item;
            item.name = mesh.name;
            item.mesh = meshes.back().get();
            sceneMeshes.push_back(item);
        }
        const katana::cad::SceneOptions options; // the application's defaults
        katana::cad::SceneBuilder builder;
        builder.build(document, surfaces, options, loaded.list, sceneMeshes);
        loaded.camera =
            framedCamera(katana::cad::sceneBounds(document, surfaces, options, sceneMeshes));
        return loaded;
    }();
    return scene;
}

// ---- loops ---------------------------------------------------------------------------

std::unique_ptr<OffscreenGpu> hardwareGpu(benchmark::State& state, int samples,
                                          Expansion expansion)
{
    OffscreenOptions options;
    options.device = GpuDevice::Hardware;
    options.timestamps = true;
    options.sampleCount = samples;
    options.expansion = expansion;
    auto gpu = OffscreenGpu::create(kWidth, kHeight, options);
    if (!gpu) {
        state.SkipWithError(gpu.error().describe());
        return nullptr;
    }
    return std::move(gpu.value());
}

void counters(benchmark::State& state, const DrawList& list)
{
    state.counters["triangles"] = static_cast<double>(list.triangles.size());
    state.counters["lines"] = static_cast<double>(list.lines.size());
}

// `manual`: report the GPU's timestamp as the iteration time.
void gpuLoop(benchmark::State& state, const DrawList& list, const Camera& camera, bool manual,
             int samples = 4, Expansion expansion = Expansion::GeometryShader)
{
    auto gpu = hardwareGpu(state, samples, expansion);
    if (!gpu) {
        return;
    }
    gpu->renderer().setDrawList(list);
    // The first frames compile the shaders and upload the scene; the 3D view
    // pays that once, not per frame, so they are not timed.
    for (int warm = 0; warm < 3; ++warm) {
        if (!gpu->renderFrame(camera)) {
            state.SkipWithError("GPU frame failed");
            return;
        }
    }
    double gpuTotal = 0.0;
    for (auto _ : state) {
        const auto started = std::chrono::steady_clock::now();
        const auto stats = gpu->renderFrame(camera);
        const auto finished = std::chrono::steady_clock::now();
        if (!stats) {
            state.SkipWithError(stats.error().describe());
            return;
        }
        const double gpuMs = gpu->lastGpuMilliseconds();
        gpuTotal += gpuMs;
        if (manual) {
            if (!(gpuMs > 0.0)) {
                state.SkipWithError("this device reports no GPU timestamps");
                return;
            }
            state.SetIterationTime(gpuMs * 1.0e-3);
        } else {
            benchmark::DoNotOptimize(finished - started);
        }
    }
    counters(state, list);
    state.counters["gpu_ms"] =
        state.iterations() > 0 ? gpuTotal / static_cast<double>(state.iterations()) : 0.0;
}

void cpuLoop(benchmark::State& state, const DrawList& list, const Camera& camera)
{
    auto target = Framebuffer::create(kWidth, kHeight);
    if (!target.ok()) {
        state.SkipWithError("framebuffer");
        return;
    }
    Rasterizer rasterizer; // reused across frames, as a viewport does
    for (auto _ : state) {
        const auto stats = rasterizer.render(list, camera, *target, RenderOptions{});
        if (!stats.ok()) {
            state.SkipWithError("render failed");
            return;
        }
        benchmark::DoNotOptimize(target->color().data());
    }
    counters(state, list);
}

const DrawList& ground(int cells)
{
    static const DrawList small = groundGrid(256);
    static const DrawList large = groundGrid(724);
    return cells == 256 ? small : large;
}

void BM_GpuGround(benchmark::State& state)
{
    const DrawList& list = ground(static_cast<int>(state.range(0)));
    gpuLoop(state, list, framedCamera(list.bounds()), true);
}
// 256 cells = 131k triangles; 724 cells = 1.05M.
BENCHMARK(BM_GpuGround)->Arg(256)->Arg(724)->UseManualTime()->Unit(benchmark::kMillisecond);

void BM_GpuGroundWall(benchmark::State& state)
{
    const DrawList& list = ground(static_cast<int>(state.range(0)));
    gpuLoop(state, list, framedCamera(list.bounds()), false);
}
BENCHMARK(BM_GpuGroundWall)->Arg(256)->Arg(724)->UseRealTime()->Unit(benchmark::kMillisecond);

void BM_CpuGround(benchmark::State& state)
{
    const DrawList& list = ground(static_cast<int>(state.range(0)));
    cpuLoop(state, list, framedCamera(list.bounds()));
}
BENCHMARK(BM_CpuGround)->Arg(256)->Arg(724)->UseRealTime()->Unit(benchmark::kMillisecond);

void BM_GpuScene(benchmark::State& state)
{
    const LoadedScene& scene = archiveScene();
    if (!scene.error.empty()) {
        state.SkipWithError(scene.error);
        return;
    }
    gpuLoop(state, scene.list, scene.camera, true);
}
BENCHMARK(BM_GpuScene)->UseManualTime()->Unit(benchmark::kMillisecond);

void BM_GpuSceneWall(benchmark::State& state)
{
    const LoadedScene& scene = archiveScene();
    if (!scene.error.empty()) {
        state.SkipWithError(scene.error);
        return;
    }
    gpuLoop(state, scene.list, scene.camera, false);
}
BENCHMARK(BM_GpuSceneWall)->UseRealTime()->Unit(benchmark::kMillisecond);

void BM_CpuScene(benchmark::State& state)
{
    const LoadedScene& scene = archiveScene();
    if (!scene.error.empty()) {
        state.SkipWithError(scene.error);
        return;
    }
    cpuLoop(state, scene.list, scene.camera);
}
BENCHMARK(BM_CpuScene)->UseRealTime()->Unit(benchmark::kMillisecond);

// Where the scene's GPU time goes: part 0 is everything, 1 the filled
// triangles alone, 2 the lines alone, 3 the lines with the camera panned so
// far aside that none reaches the screen - what their vertex and geometry
// work costs with no pixels to fill; at 1 or 4 samples per pixel.
void BM_GpuSceneParts(benchmark::State& state)
{
    const LoadedScene& scene = archiveScene();
    if (!scene.error.empty()) {
        state.SkipWithError(scene.error);
        return;
    }
    DrawList list = scene.list;
    if (state.range(0) == 1) {
        list.lines.clear();
        list.points.clear();
    } else if (state.range(0) >= 2) {
        list.triangles.clear();
        list.points.clear();
    }
    Camera camera = scene.camera;
    if (state.range(0) == 3) {
        camera.panPixels(6000.0, 0.0);
    }
    gpuLoop(state, list, camera, true, static_cast<int>(state.range(1)));
}
BENCHMARK(BM_GpuSceneParts)
    ->ArgsProduct({{0, 1, 2, 3}, {1, 4}})
    ->UseManualTime()
    ->Unit(benchmark::kMillisecond);

// The two ways of widening lines and points (shader_library.hpp): 0 the
// geometry shader, the default; 1 six-vertex instances, the fallback for a
// device without a geometry stage. Scene lines alone (part 2 above), where
// the choice is all that differs, and lines alone off screen (part 3).
void BM_GpuSceneLinesBy(benchmark::State& state)
{
    const LoadedScene& scene = archiveScene();
    if (!scene.error.empty()) {
        state.SkipWithError(scene.error);
        return;
    }
    DrawList list = scene.list;
    list.triangles.clear();
    list.points.clear();
    Camera camera = scene.camera;
    if (state.range(1) == 1) {
        camera.panPixels(6000.0, 0.0);
    }
    const Expansion expansion =
        state.range(0) == 0 ? Expansion::GeometryShader : Expansion::Instanced;
    gpuLoop(state, list, camera, true, 4, expansion);
}
BENCHMARK(BM_GpuSceneLinesBy)
    ->ArgsProduct({{0, 1}, {0, 1}})
    ->UseManualTime()
    ->Unit(benchmark::kMillisecond);

} // namespace

// QRhi needs a QGuiApplication; offscreen, because a benchmark has no window
// and raw QRhi on Direct3D 11 renders into textures there all the same.
int main(int argc, char** argv)
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QGuiApplication application(argc, argv);
    benchmark::Initialize(&argc, argv);
    if (benchmark::ReportUnrecognizedArguments(argc, argv)) {
        return 1;
    }
    benchmark::RunSpecifiedBenchmarks();
    benchmark::Shutdown();
    return 0;
}
