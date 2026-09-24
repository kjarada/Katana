// Building the 3D scene (cad::SceneBuilder), stage by stage, at every SIMD
// level: BM_x/scalar and BM_x/avx2.
//
// bench_render.cpp times the whole build of a synthetic survey and a frame of
// it. This file splits the build into the stages a 3D view actually runs -
// terrain, then the drawing draped on it, then the selection overlay and the
// grid - because they are rebuilt on different events and cost different
// things: the terrain is per-vertex and per-facet arithmetic over hundreds of
// thousands of triangles, the drawing is draping walks over the TIN.
//
// Each benchmark is registered once per level. The scene tests prove the
// levels give the same draw lists element for element, so a pair differs only
// in time. The file also builds against a tree without scene kernels (main at
// ed5148a): both members of a pair then time the old code, so one binary of
// each tree gives a before-and-after under tools/compare_benchmarks.py
// --alternate with identical names.
//
// Real archives, when they are on this machine: set KATANA_BENCH_SCENE_12DA to
// up to three .12da paths separated by '|'. The /0, /1 and /2 members time
// them; unset, they skip with a message, so the suite still runs anywhere.

#include <benchmark/benchmark.h>

#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/cad/scene.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/cpu_features.hpp"

namespace {

using katana::cad::SceneBuilder;
using katana::cad::SceneLayers;
using katana::cad::SceneMesh;
using katana::cad::SceneOptions;
using katana::cad::SceneSurface;
using katana::math::Vec3;
using katana::render::Rgba;
using katana::render::rgba;

// Puts the process at `level` for one benchmark and back afterwards; the
// benchmark is skipped with the reason when the processor cannot run it.
class LevelScope {
  public:
    LevelScope(benchmark::State& state, int level)
    {
        const auto previous = katana::core::setSimdLevel(static_cast<katana::core::SimdLevel>(level));
        if (!previous) {
            state.SkipWithError(previous.error().message.c_str());
            ok_ = false;
            return;
        }
        previous_ = *previous;
    }
    ~LevelScope()
    {
        if (ok_) {
            (void)katana::core::setSimdLevel(previous_);
        }
    }
    LevelScope(const LevelScope&) = delete;
    LevelScope& operator=(const LevelScope&) = delete;
    [[nodiscard]] bool ok() const { return ok_; }

  private:
    bool ok_ = true;
    katana::core::SimdLevel previous_ = katana::core::SimdLevel::Scalar;
};

// ---- synthetic ground ---------------------------------------------------------------

constexpr double kEast = 300000.0;
constexpr double kNorth = 6250000.0;

double rolling(double x, double y)
{
    return 40.0 + 6.0 * std::sin(x * 0.012) * std::cos(y * 0.009) + 0.4 * std::sin(x * 0.31 + y * 0.17);
}

// cells x cells squares over 1 km at MGA-like coordinates, two triangles each,
// with the diagonal alternating so the vertex normals are not all alike.
katana::terrain::TinSurface groundTin(int cells)
{
    const double step = 1000.0 / static_cast<double>(cells);
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
            if ((i + j) % 2 == 0) {
                triangles.push_back({v0, v1, v2}); // counter-clockwise in plan
                triangles.push_back({v0, v2, v3});
            } else {
                triangles.push_back({v0, v1, v3});
                triangles.push_back({v1, v2, v3});
            }
        }
    }
    auto surface = katana::terrain::TinSurface::create(std::move(vertices), std::move(triangles));
    if (!surface.ok()) {
        std::abort();
    }
    return std::move(*surface);
}

const katana::terrain::TinSurface& cachedTin(int cells)
{
    static std::map<int, std::unique_ptr<katana::terrain::TinSurface>> cache;
    auto& slot = cache[cells];
    if (!slot) {
        slot = std::make_unique<katana::terrain::TinSurface>(groundTin(cells));
    }
    return *slot;
}

// Closed boxes of 12 triangles each, some faces coloured: the shape of a
// trimesh archive's pits and pipes, many small meshes.
struct MeshSet {
    std::vector<katana::geometry::TriangleMesh> meshes;
    std::vector<SceneMesh> items;
};

const MeshSet& meshSet()
{
    static const MeshSet set = [] {
        MeshSet built;
        constexpr int kMeshes = 2000;
        constexpr int kBoxesPerMesh = 5;
        built.meshes.reserve(kMeshes);
        for (int m = 0; m < kMeshes; ++m) {
            katana::geometry::TriangleMesh mesh;
            for (int b = 0; b < kBoxesPerMesh; ++b) {
                const double x0 = kEast + 7.0 * static_cast<double>(m % 60) + 1.3 * b;
                const double y0 = kNorth + 9.0 * static_cast<double>(m / 60) + 0.7 * b;
                const double z0 = rolling(x0 - kEast, y0 - kNorth) - 2.0;
                const double w = 0.6 + 0.1 * static_cast<double>(b);
                const auto first = static_cast<std::uint32_t>(mesh.vertices.size());
                for (int k = 0; k < 8; ++k) {
                    mesh.vertices.emplace_back(x0 + ((k & 1) != 0 ? w : 0.0),
                                               y0 + ((k & 2) != 0 ? w * 1.3 : 0.0),
                                               z0 + ((k & 4) != 0 ? 2.1 : 0.0));
                }
                static constexpr std::uint32_t kFaces[12][3] = {
                    {0, 2, 1}, {1, 2, 3}, {4, 5, 6}, {5, 7, 6}, {0, 1, 4}, {1, 5, 4},
                    {2, 6, 3}, {3, 6, 7}, {0, 4, 2}, {2, 4, 6}, {1, 3, 5}, {3, 7, 5}};
                for (const auto& f : kFaces) {
                    mesh.faces.push_back({first + f[0], first + f[1], first + f[2]});
                }
            }
            built.meshes.push_back(std::move(mesh));
        }
        for (std::size_t m = 0; m < built.meshes.size(); ++m) {
            SceneMesh item;
            item.name = "mesh";
            item.mesh = &built.meshes[m];
            if (m % 3 == 0) {
                for (std::size_t f = 0; f < built.meshes[m].triangleCount(); ++f) {
                    item.faceColors.push_back(rgba(static_cast<std::uint8_t>(60 + 13 * f % 190),
                                                   static_cast<std::uint8_t>(40 + 7 * m % 200), 90));
                }
            }
            built.items.push_back(std::move(item));
        }
        return built;
    }();
    return set;
}

// FNV-1a over every element of every list the stage built: the same digest
// from two builds, or two levels, means the same draw lists to the bit. Folded
// to 40 bits so the counter holds it exactly in a double.
class Digest {
  public:
    template <typename T> void add(const std::vector<T>& items)
    {
        const auto* bytes = reinterpret_cast<const unsigned char*>(items.data());
        for (std::size_t i = 0; i < items.size() * sizeof(T); ++i) {
            hash_ = (hash_ ^ bytes[i]) * 1099511628211ull;
        }
        hash_ = (hash_ ^ items.size()) * 1099511628211ull;
    }
    void add(const katana::render::DrawList& list)
    {
        add(list.positions);
        add(list.colors);
        add(list.triangles);
        add(list.lines);
        add(list.points);
    }
    [[nodiscard]] double value() const
    {
        return static_cast<double>((hash_ ^ (hash_ >> 40)) & 0xFF'FFFF'FFFFull);
    }

  private:
    std::uint64_t hash_ = 14695981039346656037ull;
};

double digestOf(const SceneLayers& layers)
{
    Digest digest;
    digest.add(layers.grid);
    digest.add(layers.terrain);
    digest.add(layers.edges);
    digest.add(layers.entities);
    digest.add(layers.selection);
    digest.add(layers.edgeBase);
    digest.add(layers.edgeInk);
    digest.add(std::vector<double>{layers.bounds.min.x, layers.bounds.min.y, layers.bounds.min.z,
                                   layers.bounds.max.x, layers.bounds.max.y, layers.bounds.max.z,
                                   layers.datum, layers.rampLow, layers.rampHigh});
    return digest.value();
}

// ---- the synthetic stages -------------------------------------------------------------

// A dense TIN (Automatic draws it Shaded): per-vertex normals, the elevation
// ramp and the hillshade. cells = 512 is 524k triangles.
void BM_SceneTerrainShaded(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    std::vector<SceneSurface> surfaces(1);
    surfaces[0].name = "Existing";
    surfaces[0].surface = &cachedTin(static_cast<int>(state.range(0)));
    SceneBuilder builder;
    SceneOptions options;
    SceneLayers layers;
    for (auto _ : state) {
        builder.buildTerrain(surfaces, {}, options, layers);
        benchmark::DoNotOptimize(layers.terrain.colors.data());
    }
    state.counters["vertices"] = static_cast<double>(layers.terrain.positions.size());
    state.counters["digest"] = digestOf(layers);
}
BENCHMARK_CAPTURE(BM_SceneTerrainShaded, scalar, 0)->Arg(512)->Unit(benchmark::kMillisecond);
BENCHMARK_CAPTURE(BM_SceneTerrainShaded, avx2, 1)->Arg(512)->Unit(benchmark::kMillisecond);

// A TIN under kDenseSurfaceTriangles, so its edges are built too, into the
// fading edge list: 128 cells is 32,768 triangles.
void BM_SceneTerrainEdges(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    std::vector<SceneSurface> surfaces(1);
    surfaces[0].name = "Design";
    surfaces[0].surface = &cachedTin(static_cast<int>(state.range(0)));
    SceneBuilder builder;
    SceneOptions options;
    SceneLayers layers;
    for (auto _ : state) {
        builder.buildTerrain(surfaces, {}, options, layers);
        benchmark::DoNotOptimize(layers.edges.colors.data());
    }
    state.counters["edges"] = static_cast<double>(layers.edges.lines.size());
    state.counters["digest"] = digestOf(layers);
}
BENCHMARK_CAPTURE(BM_SceneTerrainEdges, scalar, 0)->Arg(128)->Unit(benchmark::kMillisecond);
BENCHMARK_CAPTURE(BM_SceneTerrainEdges, avx2, 1)->Arg(128)->Unit(benchmark::kMillisecond);

// 2,000 meshes of 60 faces, lit per face.
void BM_SceneMeshes(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    const MeshSet& set = meshSet();
    SceneBuilder builder;
    SceneOptions options;
    SceneLayers layers;
    for (auto _ : state) {
        builder.buildTerrain({}, set.items, options, layers);
        benchmark::DoNotOptimize(layers.terrain.colors.data());
    }
    state.counters["triangles"] = static_cast<double>(layers.terrain.triangles.size());
    state.counters["digest"] = digestOf(layers);
}
BENCHMARK_CAPTURE(BM_SceneMeshes, scalar, 0)->Unit(benchmark::kMillisecond);
BENCHMARK_CAPTURE(BM_SceneMeshes, avx2, 1)->Unit(benchmark::kMillisecond);

// What an orbit pays when the triangles cross a fade step: every edge vertex
// recoloured. Two cameras, one each side of a step, taken in turn.
void BM_SceneFadeEdges(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    std::vector<SceneSurface> surfaces(1);
    surfaces[0].surface = &cachedTin(static_cast<int>(state.range(0)));
    SceneBuilder builder;
    SceneOptions options;
    SceneLayers layers;
    builder.buildTerrain(surfaces, {}, options, layers);
    katana::render::Camera near;
    near.setViewportSize(1600, 1000);
    near.setStandardView(katana::render::StandardView::IsoSouthWest);
    if (!near.frame(layers.bounds)) {
        std::abort();
    }
    katana::render::Camera far = near;
    // The typical edge is about 11 px framed; these put it at about 7 and 9 px.
    near.setDistance(near.distance() * 1.2);
    far.setDistance(far.distance() * 1.6);
    bool which = false;
    for (auto _ : state) {
        // Forgets what the edges show, so each call recolours every edge
        // vertex even where both cameras land on the same eighth.
        for (auto& run : layers.edgeRuns) {
            run.applied = -1.0f;
        }
        benchmark::DoNotOptimize(SceneBuilder::fadeEdges(layers, which ? near : far));
        which = !which;
    }
    state.counters["vertices"] = static_cast<double>(layers.edges.colors.size());
    state.counters["digest"] = digestOf(layers);
}
BENCHMARK_CAPTURE(BM_SceneFadeEdges, scalar, 0)->Arg(1)->Arg(2)->Arg(4)->Arg(128)->Unit(benchmark::kMicrosecond);
BENCHMARK_CAPTURE(BM_SceneFadeEdges, avx2, 1)->Arg(1)->Arg(2)->Arg(4)->Arg(128)->Unit(benchmark::kMicrosecond);

// Where the kernels start to pay: the build of one small TIN (a ShadedWithEdges
// design) of `cells` squares, and of one small mesh. The dispatch thresholds
// in scene.cpp (kSurfaceKernelMinimum, kMeshKernelMinimum) come from these.
void BM_SceneKernelBreakEvenSurface(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    std::vector<SceneSurface> surfaces(1);
    surfaces[0].surface = &cachedTin(static_cast<int>(state.range(0)));
    surfaces[0].style = katana::cad::SurfaceStyle::Shaded;
    SceneBuilder builder;
    SceneOptions options;
    SceneLayers layers;
    for (auto _ : state) {
        builder.buildTerrain(surfaces, {}, options, layers);
        benchmark::DoNotOptimize(layers.terrain.colors.data());
    }
    state.counters["vertices"] = static_cast<double>(layers.terrain.positions.size());
}
BENCHMARK_CAPTURE(BM_SceneKernelBreakEvenSurface, scalar, 0)->DenseRange(1, 4)->Unit(benchmark::kMicrosecond);
BENCHMARK_CAPTURE(BM_SceneKernelBreakEvenSurface, avx2, 1)->DenseRange(1, 4)->Unit(benchmark::kMicrosecond);

void BM_SceneKernelBreakEvenMesh(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    const MeshSet& set = meshSet();
    // The first mesh cut to state.range(0) faces.
    katana::geometry::TriangleMesh mesh = set.meshes[1];
    mesh.faces.resize(static_cast<std::size_t>(state.range(0)));
    std::vector<SceneMesh> items(1);
    items[0].mesh = &mesh;
    SceneBuilder builder;
    SceneOptions options;
    SceneLayers layers;
    for (auto _ : state) {
        builder.buildTerrain({}, items, options, layers);
        benchmark::DoNotOptimize(layers.terrain.colors.data());
    }
}
BENCHMARK_CAPTURE(BM_SceneKernelBreakEvenMesh, scalar, 0)->Arg(2)->Arg(4)->Arg(8)->Arg(16)->Arg(32)->Unit(benchmark::kMicrosecond);
BENCHMARK_CAPTURE(BM_SceneKernelBreakEvenMesh, avx2, 1)->Arg(2)->Arg(4)->Arg(8)->Arg(16)->Arg(32)->Unit(benchmark::kMicrosecond);

// ---- real archives -------------------------------------------------------------------

struct ArchiveScene {
    std::string error;
    katana::archive12d::DomainImport domain;
    katana::cad::Document document;
    std::vector<SceneSurface> surfaces;
    std::vector<SceneMesh> meshes;
};

std::vector<std::string> archivePaths()
{
    std::vector<std::string> paths;
    const char* list = std::getenv("KATANA_BENCH_SCENE_12DA");
    if (list == nullptr) {
        return paths;
    }
    std::string current;
    for (const char* c = list;; ++c) {
        if (*c == '|' || *c == 0) {
            if (!current.empty()) {
                paths.push_back(current);
            }
            current.clear();
            if (*c == 0) {
                break;
            }
        } else {
            current.push_back(*c);
        }
    }
    return paths;
}

// Loaded once per process, as the application holds it: the drawing in a
// document, the surfaces and meshes beside it, coloured as the import colours
// them.
const ArchiveScene& archiveScene(std::size_t index)
{
    static std::map<std::size_t, std::unique_ptr<ArchiveScene>> cache;
    auto& slot = cache[index];
    if (slot) {
        return *slot;
    }
    slot = std::make_unique<ArchiveScene>();
    ArchiveScene& scene = *slot;
    const auto paths = archivePaths();
    if (index >= paths.size()) {
        scene.error = "KATANA_BENCH_SCENE_12DA names no archive " + std::to_string(index);
        return scene;
    }
    std::ifstream in(paths[index], std::ios::binary);
    const std::string bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    if (bytes.empty()) {
        scene.error = "cannot read " + paths[index];
        return scene;
    }
    auto archive = katana::archive12d::readArchiveBytes(bytes);
    if (!archive) {
        scene.error = archive.error().describe();
        return scene;
    }
    auto domain = katana::archive12d::toDomain(archive.value());
    if (!domain) {
        scene.error = domain.error().describe();
        return scene;
    }
    scene.domain = std::move(*domain);
    auto transaction = std::make_unique<katana::commands::Transaction>("IMPORT");
    for (const auto& layer : scene.domain.layersNeeded) {
        if (!scene.document.model().layers.contains(layer.name)) {
            transaction->add(katana::commands::createLayer(layer));
        }
    }
    for (const auto& style : scene.domain.stylesNeeded) {
        if (!scene.document.model().styles.contains(style.name)) {
            transaction->add(katana::commands::createStyle(style));
        }
    }
    transaction->add(katana::commands::createEntities(scene.domain.entities));
    if (auto status = scene.document.execute(std::move(transaction)); !status) {
        scene.error = status.error().describe();
        return scene;
    }
    for (const auto& surface : scene.domain.surfaces) {
        SceneSurface item;
        item.name = surface.name;
        item.surface = &surface.surface;
        scene.surfaces.push_back(item);
    }
    for (const auto& mesh : scene.domain.meshes) {
        const auto toRgba = [](const std::optional<katana::entity::Color>& colour, Rgba fallback) {
            return colour ? rgba(colour->r, colour->g, colour->b) : fallback;
        };
        SceneMesh item;
        item.name = mesh.name;
        item.mesh = &mesh.mesh;
        item.flatColor = toRgba(mesh.color, rgba(190, 170, 140));
        for (const auto& colour : mesh.faceColors) {
            item.faceColors.push_back(toRgba(colour, item.flatColor));
        }
        scene.meshes.push_back(std::move(item));
    }
    return scene;
}

enum class Stage { Terrain, Entities, Everything };

void runArchive(benchmark::State& state, int level, Stage stage)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    const ArchiveScene& scene = archiveScene(static_cast<std::size_t>(state.range(0)));
    if (!scene.error.empty()) {
        state.SkipWithError(scene.error.c_str());
        return;
    }
    SceneBuilder builder;
    SceneOptions options;
    SceneLayers layers;
    builder.buildTerrain(scene.surfaces, scene.meshes, options, layers);
    for (auto _ : state) {
        switch (stage) {
        case Stage::Terrain:
            builder.buildTerrain(scene.surfaces, scene.meshes, options, layers);
            break;
        case Stage::Entities:
            builder.buildEntities(scene.document, scene.surfaces, options, layers);
            break;
        case Stage::Everything:
            // What a 3D view does on its first paint (RenderViewWidget).
            builder.buildTerrain(scene.surfaces, scene.meshes, options, layers);
            builder.buildEntities(scene.document, scene.surfaces, options, layers);
            builder.buildSelection(scene.document, scene.surfaces, options, layers);
            builder.buildGrid(options, layers);
            break;
        }
        benchmark::DoNotOptimize(layers.terrain.colors.data());
        benchmark::DoNotOptimize(layers.entities.colors.data());
    }
    state.counters["terrainVertices"] = static_cast<double>(layers.terrain.positions.size());
    state.counters["entityVertices"] = static_cast<double>(layers.entities.positions.size());
    state.counters["digest"] = digestOf(layers);
}

void BM_SceneArchiveTerrain(benchmark::State& state, int level)
{
    runArchive(state, level, Stage::Terrain);
}
void BM_SceneArchiveEntities(benchmark::State& state, int level)
{
    runArchive(state, level, Stage::Entities);
}
void BM_SceneArchiveBuild(benchmark::State& state, int level)
{
    runArchive(state, level, Stage::Everything);
}
BENCHMARK_CAPTURE(BM_SceneArchiveTerrain, scalar, 0)->DenseRange(0, 2)->Unit(benchmark::kMillisecond);
BENCHMARK_CAPTURE(BM_SceneArchiveTerrain, avx2, 1)->DenseRange(0, 2)->Unit(benchmark::kMillisecond);
BENCHMARK_CAPTURE(BM_SceneArchiveEntities, scalar, 0)->DenseRange(0, 2)->Unit(benchmark::kMillisecond);
BENCHMARK_CAPTURE(BM_SceneArchiveEntities, avx2, 1)->DenseRange(0, 2)->Unit(benchmark::kMillisecond);
BENCHMARK_CAPTURE(BM_SceneArchiveBuild, scalar, 0)->DenseRange(0, 2)->Unit(benchmark::kMillisecond);
BENCHMARK_CAPTURE(BM_SceneArchiveBuild, avx2, 1)->DenseRange(0, 2)->Unit(benchmark::kMillisecond);

} // namespace
