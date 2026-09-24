// Terrain cost (PLAN.MD section 24 / Phase 19, section 32).
//
// Terrain was the largest area of the codebase with no benchmark, which is why
// Phase 19's outstanding list named a task graph, work stealing, TBB and SIMD
// without evidence that any of them is where the time goes. Rule 6 says profile
// before optimising; this file is that profile, so the next contributor argues
// from numbers rather than from the shape of the API.
//
// Sizes are what a real job carries: a drone survey of a small site is 100k to
// 500k ground points after classification, and a corridor survey is a few
// hundred thousand with several hundred breaklines through it.
//
// Every generator here is deterministic - a fixed-seed LCG, not <random>, whose
// sequence is identical on every platform and every standard library. A
// benchmark whose input varies run to run cannot show a regression.

#include <benchmark/benchmark.h>

#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <string>
#include <vector>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/cad/section.hpp"
#include "katana/cad/spatial_query.hpp"
#include "katana/core/task_pool.hpp"
#include "katana/geometry/spatial_index.hpp"
#include "katana/terrain/contours.hpp"
#include "katana/terrain/super_surface.hpp"
#include "katana/terrain/tiled_terrain.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "katana/terrain/volume.hpp"

using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::terrain::TinSurface;

namespace {

// Numerical Recipes' LCG constants (Press et al., 3rd ed., section 7.1). Chosen
// over <random> because the standard fixes no engine's output but this is plain
// arithmetic: the same points on every compiler, so a timing here is comparable
// with a timing taken on another machine.
class Lcg {
public:
    explicit Lcg(std::uint32_t seed) : state_(seed) {}

    // [0, 1)
    double next()
    {
        state_ = state_ * 1664525u + 1013904223u;
        return static_cast<double>(state_) / 4294967296.0;
    }

private:
    std::uint32_t state_;
};

// Ground that looks like ground: a broad fall across the site with two rises on
// it. A flat plane or a single dome is the easy case for a Delaunay
// triangulation and would measure faster than any real job.
double groundElevation(double x, double y)
{
    return 40.0 + x * 0.018 - y * 0.011 + 6.0 * std::sin(x * 0.004) * std::cos(y * 0.0035) +
           2.5 * std::sin(x * 0.02 + y * 0.017);
}

// `count` points scattered over a `span` metre square, as a photogrammetric or
// lidar ground classification delivers them - irregular, not on a grid.
std::vector<Point3> scatteredGround(std::size_t count, double span)
{
    Lcg rng(12345u);
    std::vector<Point3> points;
    points.reserve(count);
    for (std::size_t i = 0; i < count; ++i) {
        const double x = rng.next() * span;
        const double y = rng.next() * span;
        points.emplace_back(x, y, groundElevation(x, y));
    }
    return points;
}

// Breaklines across the site, as a kerb, a drain or a top of bank would run.
// These are what make the triangulation *constrained*, and a constrained edge
// costs more than an unconstrained one, so a build measured without them
// understates the real cost of survey data.
std::vector<katana::terrain::Breakline> ridgeBreaklines(std::size_t lines, std::size_t perLine,
                                                        double span)
{
    std::vector<katana::terrain::Breakline> breaklines;
    breaklines.reserve(lines);
    for (std::size_t i = 0; i < lines; ++i) {
        const double y = span * (static_cast<double>(i) + 0.5) / static_cast<double>(lines);
        katana::terrain::Breakline line;
        line.vertices.reserve(perLine);
        for (std::size_t j = 0; j < perLine; ++j) {
            const double x = span * static_cast<double>(j) / static_cast<double>(perLine - 1);
            line.vertices.emplace_back(x, y, groundElevation(x, y) + 0.4);
        }
        breaklines.push_back(std::move(line));
    }
    return breaklines;
}

// Built once and shared by the query benchmarks. Building a 200k-point surface
// inside each iteration would measure the build, not the query.
const TinSurface& sharedSurface()
{
    static const TinSurface surface = [] {
        katana::terrain::TinInput input;
        input.points = scatteredGround(200000, 1000.0);
        auto built = katana::terrain::buildTin(input);
        return built ? std::move(built->surface) : TinSurface{};
    }();
    return surface;
}

// A design surface a fixed height above the ground, so compareSurfaces has a
// real overlap to integrate rather than two identical meshes.
const TinSurface& sharedDesignSurface()
{
    static const TinSurface surface = [] {
        katana::terrain::TinInput input;
        Lcg rng(999u);
        input.points.reserve(50000);
        for (std::size_t i = 0; i < 50000; ++i) {
            const double x = rng.next() * 1000.0;
            const double y = rng.next() * 1000.0;
            input.points.emplace_back(x, y, groundElevation(x, y) + 1.5);
        }
        auto built = katana::terrain::buildTin(input);
        return built ? std::move(built->surface) : TinSurface{};
    }();
    return surface;
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

// The headline number. This is the one terrain operation every user reaches -
// it runs on import and on every surface rebuild - and it is single-threaded.
void BM_BuildTin(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const std::vector<Point3> points = scatteredGround(count, 1000.0);
    double triangles = 0.0;
    for (auto _ : state) {
        katana::terrain::TinInput input;
        input.points = points; // the copy is deliberate: buildTin consumes it
        auto built = katana::terrain::buildTin(input);
        if (built) {
            triangles = static_cast<double>(built->surface.triangleCount());
            benchmark::DoNotOptimize(triangles);
        }
    }
    state.SetItemsProcessed(static_cast<std::int64_t>(state.iterations()) *
                            static_cast<std::int64_t>(count));
    state.counters["triangles"] = triangles;
}
BENCHMARK(BM_BuildTin)
    ->Arg(25000)
    ->Arg(50000)
    ->Arg(100000)
    ->Arg(200000)
    ->Arg(400000)
    ->Unit(benchmark::kMillisecond);

// The same build with breaklines through it - what a corridor survey actually
// is. The gap between this and BM_BuildTin at the same point count is the price
// of constraint, and it is the number that says whether constrained insertion
// deserves attacking separately from the unconstrained build.
void BM_BuildTinWithBreaklines(benchmark::State& state)
{
    const auto lines = static_cast<std::size_t>(state.range(0));
    const std::vector<Point3> points = scatteredGround(100000, 1000.0);
    const auto breaklines = lines == 0 ? std::vector<katana::terrain::Breakline>{}
                                       : ridgeBreaklines(lines, 200, 1000.0);
    for (auto _ : state) {
        katana::terrain::TinInput input;
        input.points = points;
        input.breaklines = breaklines;
        auto built = katana::terrain::buildTin(input);
        bool ok = static_cast<bool>(built);
        benchmark::DoNotOptimize(ok);
    }
}
BENCHMARK(BM_BuildTinWithBreaklines)->Arg(0)->Arg(20)->Arg(100)->Unit(benchmark::kMillisecond);

// ---------------------------------------------------------------------------
// Querying
// ---------------------------------------------------------------------------

// elevationAt is the inner loop of every profile, every cross section and the
// viewport cursor readout. extractSection calls it once per sample and again
// per bisection step, so a section across a 1 km alignment is thousands of
// these. If it is slow, section extraction is slow and nothing about buildTin
// matters to that feature.
void BM_ElevationAt(benchmark::State& state)
{
    const TinSurface& surface = sharedSurface();
    Lcg rng(777u);
    std::vector<Point2> probes;
    probes.reserve(4096);
    for (std::size_t i = 0; i < 4096; ++i) {
        probes.emplace_back(rng.next() * 1000.0, rng.next() * 1000.0);
    }
    std::size_t i = 0;
    for (auto _ : state) {
        auto elevation = surface.elevationAt(probes[i++ % probes.size()]);
        benchmark::DoNotOptimize(elevation);
    }
    state.SetItemsProcessed(static_cast<std::int64_t>(state.iterations()));
}
BENCHMARK(BM_ElevationAt);

// ---------------------------------------------------------------------------
// Deriving
// ---------------------------------------------------------------------------

// Contours are generated on demand and the user waits for them. The interval
// sets how many bands are walked, so a coarse and a fine set are both measured:
// the cost should scale with the number of bands, and a departure from that
// would mean the band walk re-scans the whole surface per level.
void BM_Contours(benchmark::State& state)
{
    const TinSurface& surface = sharedSurface();
    const double interval = static_cast<double>(state.range(0)) * 0.1;
    for (auto _ : state) {
        auto lines = katana::terrain::contours(surface, interval);
        bool ok = static_cast<bool>(lines);
        benchmark::DoNotOptimize(ok);
    }
}
BENCHMARK(BM_Contours)->Arg(50)->Arg(10)->Arg(2)->Unit(benchmark::kMillisecond);

// The same on one thread: TaskPool(0) runs every level on the caller. The
// pair shows what the levels in parallel buy, and - since the answer is
// bit-identical by construction (test_contours.cpp) - that it costs nothing
// in exactness.
void BM_ContoursSerial(benchmark::State& state)
{
    static katana::core::TaskPool inlineOnly(0);
    const TinSurface& surface = sharedSurface();
    const double interval = static_cast<double>(state.range(0)) * 0.1;
    for (auto _ : state) {
        auto lines = katana::terrain::contours(surface, interval, 0.0, 5, &inlineOnly);
        bool ok = static_cast<bool>(lines);
        benchmark::DoNotOptimize(ok);
    }
}
BENCHMARK(BM_ContoursSerial)->Arg(50)->Arg(10)->Arg(2)->Unit(benchmark::kMillisecond);

// Cut and fill between two surfaces - the answer an earthworks job exists to
// produce. volume.hpp promises this samples nothing, so what is measured here
// is the exact overlay, and any future attempt to speed it up must keep that
// promise rather than trade it for a grid.
void BM_CompareSurfaces(benchmark::State& state)
{
    const TinSurface& existing = sharedSurface();
    const TinSurface& design = sharedDesignSurface();
    for (auto _ : state) {
        auto comparison = katana::terrain::compareSurfaces(existing, design);
        bool ok = static_cast<bool>(comparison);
        benchmark::DoNotOptimize(ok);
    }
}
BENCHMARK(BM_CompareSurfaces)->Unit(benchmark::kMillisecond);

// The comparison on one thread (TaskPool(0)): the same blocks, summed the
// same way, so the same bits as BM_CompareSurfaces - and the serial cost the
// parallel one is measured against.
void BM_CompareSurfacesSerial(benchmark::State& state)
{
    static katana::core::TaskPool inlineOnly(0);
    const TinSurface& existing = sharedSurface();
    const TinSurface& design = sharedDesignSurface();
    for (auto _ : state) {
        auto comparison = katana::terrain::compareSurfaces(existing, design, &inlineOnly);
        bool ok = static_cast<bool>(comparison);
        benchmark::DoNotOptimize(ok);
    }
}
BENCHMARK(BM_CompareSurfacesSerial)->Unit(benchmark::kMillisecond);


// ---------------------------------------------------------------------------
// Combining and batching
// ---------------------------------------------------------------------------

// A band running DIAGONALLY across the site. The diagonal is the point: its
// bounding box is nearly the whole site, so the cheap box reject in overlaps()
// almost never fires, while the strip it actually covers is narrow - so most
// base triangles reach the vertex scan that the box reject exists to avoid.
//
// An axis-aligned band of the same area does NOT measure this: its bounding box
// culls nine triangles in ten, and overlaps() returns on its first sample point
// for most of the rest. That version of this benchmark showed no difference
// between the exhaustive scan and the indexed one, which was a fact about the
// benchmark and not about the code.
const TinSurface& sharedBandSurface()
{
    static const TinSurface surface = [] {
        katana::terrain::TinInput input;
        Lcg rng(4242u);
        input.points.reserve(20000);
        for (std::size_t i = 0; i < 20000; ++i) {
            const double along = 50.0 + rng.next() * 900.0;
            const double across = (rng.next() - 0.5) * 100.0;
            const double x = along;
            const double y = along + across;
            input.points.emplace_back(x, y, groundElevation(x, y) + 2.0);
        }
        auto built = katana::terrain::buildTin(input);
        return built ? std::move(built->surface) : TinSurface{};
    }();
    return surface;
}

// A pad over 4% of the site - a building platform or a car park, which is the
// common shape - where the box reject does fire for most base triangles.
const TinSurface& sharedPadSurface()
{
    static const TinSurface surface = [] {
        katana::terrain::TinInput input;
        Lcg rng(2024u);
        input.points.reserve(20000);
        for (std::size_t i = 0; i < 20000; ++i) {
            const double x = 400.0 + rng.next() * 200.0;
            const double y = 400.0 + rng.next() * 200.0;
            input.points.emplace_back(x, y, groundElevation(x, y) + 2.0);
        }
        auto built = katana::terrain::buildTin(input);
        return built ? std::move(built->surface) : TinSurface{};
    }();
    return surface;
}

// What a 12d super tin costs on import: every triangle of a lower member is
// kept or dropped by asking whether a higher member covers its centroid.
//
// The two shapes are the two regimes, and both are measured on purpose. A pad
// is rejected by the members' bounding boxes for most triangles, so it is fast
// however the coverage test is written; a band is not, so it is the shape that
// says whether the coverage test itself is affordable. Optimising against the
// pad alone would show nothing and prove nothing.
void BM_CombineSurfaces(benchmark::State& state)
{
    const TinSurface& base = sharedSurface();
    const TinSurface& upper = state.range(0) == 0 ? sharedPadSurface() : sharedBandSurface();
    const std::vector<const TinSurface*> members{&base, &upper};
    double triangles = 0.0;
    for (auto _ : state) {
        auto combined = katana::terrain::combineSurfaces(members);
        if (combined) {
            triangles = static_cast<double>(combined->triangleCount());
            benchmark::DoNotOptimize(triangles);
        }
    }
    state.counters["triangles"] = triangles;
}
// 0: the pad (box reject fires). 1: the band (it does not).
BENCHMARK(BM_CombineSurfaces)->Arg(0)->Arg(1)->Unit(benchmark::kMillisecond);

// Spot elevations in bulk - a level sheet, a set of design checks, a contour
// label pass. This is elevationAt a hundred thousand times over, which is why
// it is worth spreading over cores while a single elevationAt is not: one query
// is around a microsecond, and waking a thread pool costs more than that.
void BM_ElevationsAt(benchmark::State& state)
{
    const TinSurface& surface = sharedSurface();
    Lcg rng(31337u);
    constexpr std::size_t kProbes = 100000;
    std::vector<Point2> probes;
    probes.reserve(kProbes);
    for (std::size_t i = 0; i < kProbes; ++i) {
        probes.emplace_back(rng.next() * 1000.0, rng.next() * 1000.0);
    }
    for (auto _ : state) {
        auto elevations = surface.elevationsAt(probes);
        benchmark::DoNotOptimize(elevations.size());
    }
    state.SetItemsProcessed(static_cast<std::int64_t>(state.iterations()) *
                            static_cast<std::int64_t>(kProbes));
}
BENCHMARK(BM_ElevationsAt)->Unit(benchmark::kMillisecond);

// A survey too big for one triangulation is tiled, and every tile is
// independent - so this measures whether that independence is actually spent.
// The tiles are evicted with the clock stopped, because what is being measured
// is the build and not the free.
void BM_TiledTerrainBuildAll(benchmark::State& state)
{
    katana::terrain::TinInput input;
    input.points = scatteredGround(150000, 1000.0);
    katana::terrain::TiledTerrainOptions options;
    options.tileSize = 125.0;   // 8 x 8 = 64 tiles over a 1 km site
    options.bufferWidth = 10.0; // several times the ~2.6 m mean point spacing
    auto terrain = katana::terrain::TiledTerrain::create(std::move(input), options);
    if (!terrain) {
        state.SkipWithError("the tiled terrain could not be created");
        return;
    }
    for (auto _ : state) {
        const auto status = terrain->buildAll();
        benchmark::DoNotOptimize(status.ok());
        state.PauseTiming();
        for (std::size_t tile = 0; tile < terrain->tileCount(); ++tile) {
            terrain->evictTile(tile);
        }
        state.ResumeTiming();
    }
    state.counters["tiles"] = static_cast<double>(terrain->tileCount());
}
BENCHMARK(BM_TiledTerrainBuildAll)->Unit(benchmark::kMillisecond);

// ---------------------------------------------------------------------------
// Sections: the crossing search
// ---------------------------------------------------------------------------

// Survey strings scattered over a 1 km site, as a detail survey delivers them:
// 30 000 short polylines of 2 to 12 vertices. The section is cut 60 m across
// the middle - a cross section, the common case, and the one where walking
// every entity in the drawing costs the most per crossing found.
struct SectionFixture {
    katana::entity::Model model;
    katana::geometry::SpatialIndex index;
};

const SectionFixture& sharedSectionFixture()
{
    static const SectionFixture fixture = [] {
        SectionFixture built;
        (void)built.model.layers.ensure("0");
        Lcg rng(777u);
        for (std::size_t i = 0; i < 30000; ++i) {
            katana::geometry::Polyline2 line;
            double x = rng.next() * 1000.0;
            double y = rng.next() * 1000.0;
            const std::size_t count = 2 + static_cast<std::size_t>(rng.next() * 11.0);
            for (std::size_t v = 0; v < count; ++v) {
                line.vertices.emplace_back(x, y);
                x += (rng.next() - 0.5) * 20.0;
                y += (rng.next() - 0.5) * 20.0;
            }
            katana::entity::Entity entity;
            entity.geometry = std::move(line);
            entity.layer = "0";
            (void)built.model.entities.add(std::move(entity));
        }
        std::vector<katana::geometry::SpatialEntry> entries;
        built.model.entities.forEach([&](const katana::entity::Entity& entity) {
            entries.push_back({static_cast<katana::geometry::SpatialId>(entity.id),
                               katana::cad::detail::queryExtents(built.model, entity)});
        });
        built.index.rebuild(entries);
        return built;
    }();
    return fixture;
}

// Arg 0: a 60 m cross section; 1: the 1.4 km diagonal of the whole site.
// Crossings only - no surface - so what is timed is the search.
void runSectionCrossings(benchmark::State& state, bool withIndex)
{
    const SectionFixture& fixture = sharedSectionFixture();
    katana::geometry::Polyline2 alignment;
    if (state.range(0) == 0) {
        alignment.vertices = {Point2(470.0, 500.0), Point2(530.0, 500.0)};
    } else {
        alignment.vertices = {Point2(0.0, 0.0), Point2(1000.0, 1000.0)};
    }
    katana::cad::SectionOptions options;
    options.interval = 1.0;
    options.includeSurfaceBreaks = false;
    options.spatialIndex = withIndex ? &fixture.index : nullptr;
    std::size_t crossings = 0;
    for (auto _ : state) {
        auto section = katana::cad::extractSection(alignment, {}, &fixture.model, options);
        crossings = section ? section->crossings.size() : 0;
        benchmark::DoNotOptimize(crossings);
    }
    state.counters["crossings"] = static_cast<double>(crossings);
}

void BM_SectionCrossings(benchmark::State& state) { runSectionCrossings(state, false); }
BENCHMARK(BM_SectionCrossings)->Arg(0)->Arg(1)->Unit(benchmark::kMillisecond);

void BM_SectionCrossingsIndexed(benchmark::State& state) { runSectionCrossings(state, true); }
BENCHMARK(BM_SectionCrossingsIndexed)->Arg(0)->Arg(1)->Unit(benchmark::kMillisecond);

// ---------------------------------------------------------------------------
// Real survey data, when it is on this machine
// ---------------------------------------------------------------------------
//
// Generated ground is regular in ways real ground is not, so the numbers that
// decide anything are also taken on real archives. Those are survey data that
// do not belong in the repository; name them in the environment:
//
//   KATANA_BENCH_TIN_12DA       a .12da archive holding a TIN (the largest is used)
//   KATANA_BENCH_DRAWING_12DA   a .12da archive of survey strings (for sections)
//
// Unset, these benchmarks skip with a message rather than fail, so the suite
// still runs anywhere.

std::string readWholeFile(const char* path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

katana::core::Result<katana::archive12d::DomainImport> importArchive(const char* variable)
{
    const char* path = std::getenv(variable);
    if (path == nullptr || *path == 0) {
        return katana::core::makeError(katana::core::ErrorCode::NotFound,
                                       std::string(variable) + " is not set");
    }
    const std::string bytes = readWholeFile(path);
    if (bytes.empty()) {
        return katana::core::makeError(katana::core::ErrorCode::NotFound,
                                       std::string("cannot read ") + path);
    }
    auto archive = katana::archive12d::readArchiveBytes(bytes);
    if (!archive) {
        return archive.error();
    }
    return katana::archive12d::toDomain(archive.value());
}

// The archive's largest TIN, and a design derived from it: every third vertex
// raised 0.3 m and triangulated afresh, so the design's triangles cross the
// existing ones as a real design's do (a copy of the same triangulation would
// pair each triangle with itself and measure the easy case).
struct ArchiveSurfaces {
    TinSurface existing;
    TinSurface design;
    std::string error;
};

const ArchiveSurfaces& archiveSurfaces()
{
    static const ArchiveSurfaces surfaces = [] {
        ArchiveSurfaces loaded;
        auto domain = importArchive("KATANA_BENCH_TIN_12DA");
        if (!domain) {
            loaded.error = domain.error().describe();
            return loaded;
        }
        std::size_t largest = 0;
        for (auto& surface : domain->surfaces) {
            if (surface.surface.triangleCount() > largest) {
                largest = surface.surface.triangleCount();
                loaded.existing = std::move(surface.surface);
            }
        }
        if (loaded.existing.empty()) {
            loaded.error = "the archive holds no TIN";
            return loaded;
        }
        katana::terrain::TinInput input;
        for (std::size_t v = 0; v < loaded.existing.vertexCount(); v += 3) {
            const Point3& p = loaded.existing.vertices()[v];
            input.points.emplace_back(p.x, p.y, p.z + 0.3);
        }
        katana::terrain::TinBuildOptions options;
        options.duplicatePoints = katana::terrain::DuplicatePointPolicy::Average;
        auto built = katana::terrain::buildTin(input, options);
        if (!built) {
            loaded.error = built.error().describe();
            return loaded;
        }
        loaded.design = std::move(built->surface);
        return loaded;
    }();
    return surfaces;
}

// Arg: the interval in tenths of a metre (10 = 1 m, 1 = 0.1 m), the two the
// perf map measured on the 229k-triangle archive TIN (279-361 ms and 500-510
// ms, serial, on a loaded machine).
void runArchiveContours(benchmark::State& state, katana::core::TaskPool* pool)
{
    const ArchiveSurfaces& surfaces = archiveSurfaces();
    if (!surfaces.error.empty()) {
        state.SkipWithError(surfaces.error.c_str());
        return;
    }
    const double interval = static_cast<double>(state.range(0)) * 0.1;
    std::size_t count = 0;
    for (auto _ : state) {
        auto lines = katana::terrain::contours(surfaces.existing, interval, 0.0, 5, pool);
        count = lines ? lines->size() : 0;
        benchmark::DoNotOptimize(count);
    }
    state.counters["contours"] = static_cast<double>(count);
    state.counters["triangles"] = static_cast<double>(surfaces.existing.triangleCount());
}

void BM_ArchiveTinContours(benchmark::State& state) { runArchiveContours(state, nullptr); }
BENCHMARK(BM_ArchiveTinContours)->Arg(10)->Arg(1)->Unit(benchmark::kMillisecond);

void BM_ArchiveTinContoursSerial(benchmark::State& state)
{
    static katana::core::TaskPool inlineOnly(0);
    runArchiveContours(state, &inlineOnly);
}
BENCHMARK(BM_ArchiveTinContoursSerial)->Arg(10)->Arg(1)->Unit(benchmark::kMillisecond);

void runArchiveCompare(benchmark::State& state, katana::core::TaskPool* pool)
{
    const ArchiveSurfaces& surfaces = archiveSurfaces();
    if (!surfaces.error.empty()) {
        state.SkipWithError(surfaces.error.c_str());
        return;
    }
    std::size_t pieces = 0;
    for (auto _ : state) {
        auto comparison =
            katana::terrain::compareSurfaces(surfaces.existing, surfaces.design, pool);
        pieces = comparison ? comparison->overlayTriangleCount : 0;
        benchmark::DoNotOptimize(pieces);
    }
    state.counters["pieces"] = static_cast<double>(pieces);
}

void BM_ArchiveTinCompareSurfaces(benchmark::State& state) { runArchiveCompare(state, nullptr); }
BENCHMARK(BM_ArchiveTinCompareSurfaces)->Unit(benchmark::kMillisecond);

void BM_ArchiveTinCompareSurfacesSerial(benchmark::State& state)
{
    static katana::core::TaskPool inlineOnly(0);
    runArchiveCompare(state, &inlineOnly);
}
BENCHMARK(BM_ArchiveTinCompareSurfacesSerial)->Unit(benchmark::kMillisecond);

// The archive's strings in a model, indexed as a document indexes them.
struct ArchiveDrawing {
    katana::entity::Model model;
    katana::geometry::SpatialIndex index;
    katana::geometry::Box2 bounds;
    std::string error;
};

const ArchiveDrawing& archiveDrawing()
{
    static const ArchiveDrawing drawing = [] {
        ArchiveDrawing loaded;
        auto domain = importArchive("KATANA_BENCH_DRAWING_12DA");
        if (!domain) {
            loaded.error = domain.error().describe();
            return loaded;
        }
        for (const auto& layer : domain->layersNeeded) {
            (void)loaded.model.layers.add(layer);
        }
        for (auto& entity : domain->entities) {
            (void)loaded.model.layers.ensure(entity.layer);
            (void)loaded.model.entities.add(std::move(entity));
        }
        std::vector<katana::geometry::SpatialEntry> entries;
        loaded.model.entities.forEach([&](const katana::entity::Entity& entity) {
            entries.push_back({static_cast<katana::geometry::SpatialId>(entity.id),
                               katana::cad::detail::queryExtents(loaded.model, entity)});
        });
        loaded.index.rebuild(entries);
        loaded.bounds = loaded.index.bounds();
        return loaded;
    }();
    return drawing;
}

// Arg 0: a 60 m cross section through the centre of the drawing; 1: its full
// diagonal (the perf map's 13.6 km line). 2 and 3: the same two with the
// document's index.
void BM_ArchiveSectionCrossings(benchmark::State& state)
{
    const ArchiveDrawing& drawing = archiveDrawing();
    if (!drawing.error.empty()) {
        state.SkipWithError(drawing.error.c_str());
        return;
    }
    const Point2 centre = drawing.bounds.center();
    katana::geometry::Polyline2 alignment;
    if (state.range(0) % 2 == 0) {
        alignment.vertices = {Point2(centre.x - 30.0, centre.y),
                              Point2(centre.x + 30.0, centre.y)};
    } else {
        alignment.vertices = {drawing.bounds.min, drawing.bounds.max};
    }
    katana::cad::SectionOptions options;
    options.interval = 1.0;
    options.includeSurfaceBreaks = false;
    options.spatialIndex = state.range(0) >= 2 ? &drawing.index : nullptr;
    std::size_t crossings = 0;
    for (auto _ : state) {
        auto section = katana::cad::extractSection(alignment, {}, &drawing.model, options);
        crossings = section ? section->crossings.size() : 0;
        benchmark::DoNotOptimize(crossings);
    }
    state.counters["crossings"] = static_cast<double>(crossings);
    state.counters["entities"] = static_cast<double>(drawing.model.entities.size());
}
BENCHMARK(BM_ArchiveSectionCrossings)->Arg(0)->Arg(1)->Arg(2)->Arg(3)->Unit(benchmark::kMillisecond);

} // namespace
