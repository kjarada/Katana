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
#include <vector>

#include "katana/terrain/contours.hpp"
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

} // namespace
