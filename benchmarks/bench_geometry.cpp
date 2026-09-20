// Benchmarks for the geometry operations the CAD commands lean on
// (PLAN.MD Phase 04: "Benchmark important operations").
//
// Inputs are generated at projected survey coordinates (easting ~5e5,
// northing ~5e6) because that is where the code actually runs, and where a
// formulation that loses precision would also tend to lose speed through
// denormals or extra branches.

#include <benchmark/benchmark.h>

#include <cmath>
#include <random>
#include <vector>

#include "katana/geometry/editing.hpp"
#include "katana/geometry/intersection.hpp"
#include "katana/geometry/polygon.hpp"

using namespace katana::geometry;

namespace {

constexpr double kEasting = 500000.0;
constexpr double kNorthing = 5000000.0;

// Fixed seed: benchmark inputs must not vary between runs, or timings are not
// comparable across commits.
std::mt19937_64 makeEngine()
{
    return std::mt19937_64(0x4B4154414E41ULL);
}

std::vector<Segment2> makeSegments(std::size_t count)
{
    auto engine = makeEngine();
    std::uniform_real_distribution<double> offset(-500.0, 500.0);
    std::vector<Segment2> segments;
    segments.reserve(count);
    for (std::size_t i = 0; i < count; ++i) {
        const Point2 start(kEasting + offset(engine), kNorthing + offset(engine));
        segments.push_back(Segment2{start, start + Vec2(offset(engine), offset(engine))});
    }
    return segments;
}

// Star-shaped polygon: vertices at increasing angles, so it is always simple.
Polyline2 makeStarPolygon(std::size_t vertices)
{
    auto engine = makeEngine();
    std::uniform_real_distribution<double> radius(50.0, 500.0);
    Polyline2 polygon;
    polygon.closed = true;
    polygon.vertices.reserve(vertices);
    for (std::size_t i = 0; i < vertices; ++i) {
        const double angle = katana::math::kTwoPi * static_cast<double>(i) /
                             static_cast<double>(vertices);
        const double r = radius(engine);
        polygon.vertices.emplace_back(kEasting + std::cos(angle) * r,
                                      kNorthing + std::sin(angle) * r);
    }
    return polygon;
}

// ---- intersection ------------------------------------------------------------

void BM_IntersectSegments(benchmark::State& state)
{
    const auto segments = makeSegments(static_cast<std::size_t>(state.range(0)));
    for (auto _ : state) {
        std::size_t hits = 0;
        for (std::size_t i = 1; i < segments.size(); ++i) {
            hits += intersect(segments[i - 1], segments[i]).exists() ? 1 : 0;
        }
        benchmark::DoNotOptimize(hits);
    }
    state.SetItemsProcessed(state.iterations() * (state.range(0) - 1));
}
BENCHMARK(BM_IntersectSegments)->Arg(1 << 10)->Arg(1 << 14);

// All-pairs intersection: the shape of an unindexed snap or trim query, and the
// motivation for the spatial index of Phase 18.
void BM_IntersectSegmentsAllPairs(benchmark::State& state)
{
    const auto segments = makeSegments(static_cast<std::size_t>(state.range(0)));
    for (auto _ : state) {
        std::size_t hits = 0;
        for (std::size_t i = 0; i < segments.size(); ++i) {
            for (std::size_t j = i + 1; j < segments.size(); ++j) {
                hits += intersect(segments[i], segments[j]).exists() ? 1 : 0;
            }
        }
        benchmark::DoNotOptimize(hits);
    }
    const auto n = static_cast<double>(state.range(0));
    state.SetItemsProcessed(static_cast<std::int64_t>(state.iterations() * n * (n - 1) / 2));
}
BENCHMARK(BM_IntersectSegmentsAllPairs)->Arg(256);

void BM_IntersectCircles(benchmark::State& state)
{
    auto engine = makeEngine();
    std::uniform_real_distribution<double> offset(-200.0, 200.0);
    std::uniform_real_distribution<double> radius(10.0, 200.0);
    std::vector<Circle2> circles;
    circles.reserve(4096);
    for (int i = 0; i < 4096; ++i) {
        circles.push_back(Circle2{Point2(kEasting + offset(engine), kNorthing + offset(engine)),
                                  radius(engine)});
    }
    for (auto _ : state) {
        std::size_t hits = 0;
        for (std::size_t i = 1; i < circles.size(); ++i) {
            hits += intersect(circles[i - 1], circles[i]).count;
        }
        benchmark::DoNotOptimize(hits);
    }
    state.SetItemsProcessed(state.iterations() * 4095);
}
BENCHMARK(BM_IntersectCircles);

// ---- polygons ----------------------------------------------------------------

void BM_PolygonArea(benchmark::State& state)
{
    const Polyline2 polygon = makeStarPolygon(static_cast<std::size_t>(state.range(0)));
    for (auto _ : state) {
        benchmark::DoNotOptimize(polygon.area());
    }
    state.SetItemsProcessed(state.iterations() * state.range(0));
}
BENCHMARK(BM_PolygonArea)->Arg(1 << 8)->Arg(1 << 14);

void BM_PointInPolygon(benchmark::State& state)
{
    const Polyline2 polygon = makeStarPolygon(1024);
    auto engine = makeEngine();
    std::uniform_real_distribution<double> offset(-600.0, 600.0);
    std::vector<Point2> probes;
    probes.reserve(1024);
    for (int i = 0; i < 1024; ++i) {
        probes.emplace_back(kEasting + offset(engine), kNorthing + offset(engine));
    }
    for (auto _ : state) {
        std::size_t inside = 0;
        for (const Point2& probe : probes) {
            inside += polygon.classify(probe) == Containment::Inside ? 1 : 0;
        }
        benchmark::DoNotOptimize(inside);
    }
    state.SetItemsProcessed(state.iterations() * 1024);
}
BENCHMARK(BM_PointInPolygon);

void BM_Triangulate(benchmark::State& state)
{
    const Polyline2 polygon = makeStarPolygon(static_cast<std::size_t>(state.range(0)));
    for (auto _ : state) {
        auto triangles = triangulate(polygon);
        benchmark::DoNotOptimize(triangles);
    }
    state.SetItemsProcessed(state.iterations() * state.range(0));
}
// Ear clipping is O(n^2); the arguments stay modest and the range shows the growth.
BENCHMARK(BM_Triangulate)->Arg(64)->Arg(256)->Arg(1024);

void BM_ConvexHull(benchmark::State& state)
{
    auto engine = makeEngine();
    std::uniform_real_distribution<double> offset(-500.0, 500.0);
    std::vector<Point2> points;
    points.reserve(static_cast<std::size_t>(state.range(0)));
    for (int i = 0; i < state.range(0); ++i) {
        points.emplace_back(kEasting + offset(engine), kNorthing + offset(engine));
    }
    for (auto _ : state) {
        auto hull = convexHull(points); // copies; that cost is part of the call
        benchmark::DoNotOptimize(hull);
    }
    state.SetItemsProcessed(state.iterations() * state.range(0));
}
BENCHMARK(BM_ConvexHull)->Arg(1 << 10)->Arg(1 << 16);

void BM_SimplifyPolyline(benchmark::State& state)
{
    auto engine = makeEngine();
    std::normal_distribution<double> noise(0.0, 0.05);
    Polyline2 path;
    path.vertices.reserve(static_cast<std::size_t>(state.range(0)));
    for (int i = 0; i < state.range(0); ++i) {
        path.vertices.emplace_back(kEasting + i * 0.5, kNorthing + noise(engine));
    }
    for (auto _ : state) {
        auto simplified = simplify(path, 0.1);
        benchmark::DoNotOptimize(simplified);
    }
    state.SetItemsProcessed(state.iterations() * state.range(0));
}
BENCHMARK(BM_SimplifyPolyline)->Arg(1 << 12);

// ---- editing ------------------------------------------------------------------

void BM_OffsetPolyline(benchmark::State& state)
{
    const Polyline2 polygon = makeStarPolygon(static_cast<std::size_t>(state.range(0)));
    for (auto _ : state) {
        auto offsetted = offset(polygon, 1.0);
        benchmark::DoNotOptimize(offsetted);
    }
    state.SetItemsProcessed(state.iterations() * state.range(0));
}
BENCHMARK(BM_OffsetPolyline)->Arg(1 << 8)->Arg(1 << 12);

void BM_Fillet(benchmark::State& state)
{
    const Segment2 first{Point2(kEasting + 10.0, kNorthing), Point2(kEasting, kNorthing)};
    const Segment2 second{Point2(kEasting, kNorthing), Point2(kEasting, kNorthing + 10.0)};
    for (auto _ : state) {
        auto result = fillet(first, second, 2.0);
        benchmark::DoNotOptimize(result);
    }
}
BENCHMARK(BM_Fillet);

void BM_TrimSegment(benchmark::State& state)
{
    const Curve2 target = Segment2{Point2(kEasting, kNorthing), Point2(kEasting + 100.0, kNorthing)};
    std::vector<Curve2> cutters;
    for (int i = 1; i < 32; ++i) {
        const double x = kEasting + i * 3.0;
        cutters.emplace_back(Segment2{Point2(x, kNorthing - 1.0), Point2(x, kNorthing + 1.0)});
    }
    const Point2 pick(kEasting + 50.0, kNorthing);
    for (auto _ : state) {
        auto trimmed = trim(target, cutters, pick);
        benchmark::DoNotOptimize(trimmed);
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(cutters.size()));
}
BENCHMARK(BM_TrimSegment);

} // namespace
