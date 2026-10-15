// Volume cost (docs/volumes.md, "Not measured"): two timings the volumes'
// document estimated and never took.
//
//   * The tests that refuse sections sweeping the same ground twice - sections
//     that cross one another, and stretches of the alignment that run side by
//     side - on a long run cut closely. Each indexes the boxes of the sections
//     (or of the strips between them) and tests the pairs whose boxes overlap.
//     On an alignment along an axis no boxes overlap; on a diagonal one every
//     section's box covers the whole run, so every pair is tested: n (n - 1) / 2
//     of each. A diagonal alignment 10 km long cut every 0.1 m is 100 000
//     sections and about 5e9 pairs of each test; the benchmark takes the same
//     run at 1250 to 10 000 sections, fits the growth (Complexity), and the
//     document extrapolates, flagged as an extrapolation.
//   * The join of the difference surface's pieces on a large pair: the surfaces
//     of SURFACE DIFFERENCE are the overlay's pieces joined into a TIN, and the
//     overlay alone is compareSurfaces. The difference between the two timings,
//     on the same pair, is the join.
//
// Every generator here is deterministic, as bench_terrain.cpp's are: a
// fixed-seed LCG, not <random>.

#include <benchmark/benchmark.h>

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <utility>
#include <vector>

#include "katana/geometry/alignment.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/overlay.hpp"
#include "katana/terrain/section_volume.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "katana/terrain/volume.hpp"

using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::terrain::TinSurface;

namespace {

// Numerical Recipes' LCG constants (Press et al., 3rd ed., section 7.1), as
// bench_terrain.cpp uses them: the same points on every compiler.
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

TinSurface surfaceOf(std::vector<Point3> points)
{
    katana::terrain::TinInput input;
    input.points = std::move(points);
    auto built = katana::terrain::buildTin(input);
    return built ? std::move(built->surface) : TinSurface{};
}

// ---------------------------------------------------------------------------
// The tests that refuse sections sweeping the same ground twice
// ---------------------------------------------------------------------------

// A flat square 1200 m on a side at height `z`: two triangles, so that cutting
// a section costs next to nothing and what the benchmark times is the tests
// over the sections' boxes and not the clipping.
TinSurface slab(double z)
{
    return surfaceOf({Point3(0, 0, z), Point3(1200, 0, z), Point3(1200, 1200, z),
                      Point3(0, 1200, z)});
}

// Sections across the run from `from` to `to`, `state.range(0)` of them, with
// the default reach (the diagonal of the ground the two surfaces share, 1697 m
// each way): along an axis their boxes are thin and do not meet; along the
// diagonal every box covers the whole run.
void sections(benchmark::State& state, const Point2& from, const Point2& to)
{
    static const TinSurface base = slab(0.0);
    static const TinSurface top = slab(1.0);
    katana::geometry::HorizontalAlignment definition;
    definition.pis = {katana::geometry::AlignmentPI{from}, katana::geometry::AlignmentPI{to}};
    auto solved = katana::geometry::solveAlignment(definition);
    if (!solved || base.empty() || top.empty()) {
        state.SkipWithError("the run or the slabs could not be made");
        return;
    }
    const auto count = static_cast<double>(state.range(0));
    katana::terrain::AlignmentVolumeOptions options;
    options.interval = solved->length() / (count - 1.0);
    for (auto _ : state) {
        auto volumes = katana::terrain::volumesAlongAlignment(base, top, *solved, options);
        if (!volumes) {
            state.SkipWithError("volumesAlongAlignment refused the run");
            return;
        }
        benchmark::DoNotOptimize(volumes->endAreaFill);
    }
    state.SetComplexityN(state.range(0));
}

// 700 m due east: nothing to test but the sections' own boxes.
void BM_VolumeSectionsAlongAnAxis(benchmark::State& state)
{
    sections(state, Point2(100, 600), Point2(800, 600));
}
BENCHMARK(BM_VolumeSectionsAlongAnAxis)
    ->Arg(1250)
    ->Arg(2500)
    ->Arg(5000)
    ->Arg(10000)
    ->Complexity()
    ->Unit(benchmark::kMillisecond);

// 990 m on the diagonal: every pair of sections and every pair of strips has
// boxes that overlap, and is tested. 10 000 sections is a section every 0.1 m.
void BM_VolumeSectionsAlongADiagonal(benchmark::State& state)
{
    sections(state, Point2(100, 100), Point2(800, 800));
}
BENCHMARK(BM_VolumeSectionsAlongADiagonal)
    ->Arg(1250)
    ->Arg(2500)
    ->Arg(5000)
    ->Arg(10000)
    ->Complexity(benchmark::oNSquared)
    ->Unit(benchmark::kMillisecond);

// ---------------------------------------------------------------------------
// The difference surface's join
// ---------------------------------------------------------------------------

// Ground that looks like ground, as bench_terrain.cpp's: a broad fall with two
// rises on it.
double groundElevation(double x, double y)
{
    return 40.0 + x * 0.018 - y * 0.011 + 6.0 * std::sin(x * 0.004) * std::cos(y * 0.0035) +
           2.5 * std::sin(x * 0.02 + y * 0.017);
}

// 100 000 scattered points are about 200 000 triangles. The design is another
// scatter of the same size over the same square, a metre and a half above the
// ground with a ripple of 3 m on it, so the two cross: pieces of cut and pieces
// of fill, and the pieces a crossing splits.
const TinSurface& largeGround()
{
    static const TinSurface surface = [] {
        Lcg rng(2024u);
        std::vector<Point3> points;
        points.reserve(100000);
        for (std::size_t i = 0; i < 100000; ++i) {
            const double x = rng.next() * 1000.0;
            const double y = rng.next() * 1000.0;
            points.emplace_back(x, y, groundElevation(x, y));
        }
        return surfaceOf(std::move(points));
    }();
    return surface;
}

const TinSurface& largeDesign()
{
    static const TinSurface surface = [] {
        Lcg rng(4048u);
        std::vector<Point3> points;
        points.reserve(100000);
        for (std::size_t i = 0; i < 100000; ++i) {
            const double x = rng.next() * 1000.0;
            const double y = rng.next() * 1000.0;
            points.emplace_back(x, y,
                                groundElevation(x, y) + 1.5 + 3.0 * std::sin(x * 0.013 + y * 0.009));
        }
        return surfaceOf(std::move(points));
    }();
    return surface;
}

// The overlay and its sum: what SURFACE DIFFERENCE does before it joins.
void BM_VolumesOverlayOfALargePair(benchmark::State& state)
{
    const TinSurface& base = largeGround();
    const TinSurface& top = largeDesign();
    for (auto _ : state) {
        auto comparison = katana::terrain::compareSurfaces(base, top);
        if (!comparison) {
            state.SkipWithError("compareSurfaces refused the pair");
            return;
        }
        benchmark::DoNotOptimize(comparison->fill);
    }
    state.counters["base_triangles"] = static_cast<double>(base.triangleCount());
    state.counters["top_triangles"] = static_cast<double>(top.triangleCount());
}
BENCHMARK(BM_VolumesOverlayOfALargePair)->Unit(benchmark::kMillisecond);

// The overlay and the join into a surface: SURFACE DIFFERENCE's cost. Less the
// timing above, on the same pair, it is the join of the pieces.
void BM_VolumesDifferenceSurfaceOfALargePair(benchmark::State& state)
{
    const TinSurface& base = largeGround();
    const TinSurface& top = largeDesign();
    std::size_t pieces = 0;
    std::size_t triangles = 0;
    for (auto _ : state) {
        auto difference = katana::terrain::differenceSurface(base, top);
        if (!difference) {
            state.SkipWithError("differenceSurface refused the pair");
            return;
        }
        pieces = difference->pieces;
        triangles = difference->surface.triangleCount();
        benchmark::DoNotOptimize(triangles);
    }
    state.counters["pieces"] = static_cast<double>(pieces);
    state.counters["triangles"] = static_cast<double>(triangles);
}
BENCHMARK(BM_VolumesDifferenceSurfaceOfALargePair)->Unit(benchmark::kMillisecond);

} // namespace
