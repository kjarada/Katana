// Snapping cost per mouse move (PLAN.MD section 32: sub-16 ms interaction).
//
// Snapping runs synchronously in the viewport's mouseMoveEvent, so whatever this
// measures is paid on every single mouse move. The polyline sizes are chosen to
// be ordinary survey data: a surveyed kerb line or contour easily carries a few
// thousand vertices.

#include <benchmark/benchmark.h>

#include <cmath>

#include "katana/cad/selection.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/geometry/spatial_index.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/entity/model.hpp"

using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

// One long polyline through the origin, as a surveyed string would be.
katana::entity::Model modelWithPolyline(std::size_t vertices)
{
    katana::entity::Model model;
    Polyline2 polyline;
    polyline.vertices.reserve(vertices);
    for (std::size_t i = 0; i < vertices; ++i) {
        const double t = static_cast<double>(i);
        polyline.vertices.emplace_back(t * 0.1, std::sin(t * 0.05) * 5.0);
    }
    katana::entity::Entity entity;
    entity.geometry = std::move(polyline);
    const auto id = model.entities.add(std::move(entity));
    benchmark::DoNotOptimize(id.ok());
    return model;
}

katana::cad::SnapRequest requestAt(const Point2& cursor, katana::cad::SnapModes modes)
{
    katana::cad::SnapRequest request;
    request.cursor = cursor;
    request.aperture = 0.5; // a few pixels, converted to model units
    request.modes = modes;
    return request;
}

// The cursor sits over the middle of the polyline, so the entity-level bounding
// box filter cannot reject it: this is the case the per-segment work decides.
void BM_SnapOverPolyline(benchmark::State& state)
{
    const auto vertices = static_cast<std::size_t>(state.range(0));
    const katana::entity::Model model = modelWithPolyline(vertices);
    const double middle = static_cast<double>(vertices) * 0.05;
    const auto request = requestAt(Point2(middle, 0.0), katana::cad::kDefaultSnapModes);

    for (auto _ : state) {
        benchmark::DoNotOptimize(katana::cad::snap(model, request));
    }
    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_SnapOverPolyline)->Arg(100)->Arg(1000)->Arg(2000)->Arg(3000);

// Intersection snapping is the quadratic mode: it intersects every candidate
// curve with every other. Measured separately so a regression can be attributed.
void BM_SnapIntersectionOnly(benchmark::State& state)
{
    const auto vertices = static_cast<std::size_t>(state.range(0));
    const katana::entity::Model model = modelWithPolyline(vertices);
    const double middle = static_cast<double>(vertices) * 0.05;
    const auto request = requestAt(Point2(middle, 0.0), static_cast<katana::cad::SnapModes>(katana::cad::SnapMode::Intersection));

    for (auto _ : state) {
        benchmark::DoNotOptimize(katana::cad::snap(model, request));
    }
}
BENCHMARK(BM_SnapIntersectionOnly)->Arg(1000)->Arg(3000);

// Without Intersection the remaining modes are linear, so this is the floor the
// quadratic mode is paying on top of.
void BM_SnapEndpointOnly(benchmark::State& state)
{
    const auto vertices = static_cast<std::size_t>(state.range(0));
    const katana::entity::Model model = modelWithPolyline(vertices);
    const double middle = static_cast<double>(vertices) * 0.05;
    const auto request = requestAt(Point2(middle, 0.0), static_cast<katana::cad::SnapModes>(katana::cad::SnapMode::Endpoint));

    for (auto _ : state) {
        benchmark::DoNotOptimize(katana::cad::snap(model, request));
    }
}
BENCHMARK(BM_SnapEndpointOnly)->Arg(1000)->Arg(3000);

} // namespace

// ---- cost against ENTITY COUNT (PLAN.MD Phase 18) --------------------------------
//
// The polyline benchmarks above measure one long entity. These measure many
// short ones, which is the other shape real drawings take and the one a spatial
// index addresses: snapping, picking and box selection all walk every entity in
// the model today. A 250 000-entity drawing is an ordinary as-built.

namespace {

// Short 4-vertex strings scattered over a 1 km square, as a kerb or contour
// layer looks once it has been broken into segments.
katana::entity::Model modelWithEntities(std::size_t count)
{
    katana::entity::Model model;
    for (std::size_t i = 0; i < count; ++i) {
        const double t = static_cast<double>(i);
        // Cheap deterministic scatter; no RNG, so the layout is identical on
        // every machine and the numbers are comparable.
        const double x = std::fmod(t * 37.0, 1000.0);
        const double y = std::fmod(t * 91.0, 1000.0);
        Polyline2 polyline;
        for (int v = 0; v < 4; ++v) {
            polyline.vertices.emplace_back(x + v * 0.7, y + std::sin(t + v) * 0.9);
        }
        katana::entity::Entity entity;
        entity.geometry = std::move(polyline);
        const auto id = model.entities.add(std::move(entity));
        benchmark::DoNotOptimize(id.ok());
    }
    return model;
}

// The cursor sits in the middle of the scatter, so nothing is rejected by being
// far away: this is the cost of deciding, not of skipping.
void BM_SnapAcrossManyEntities(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const katana::entity::Model model = modelWithEntities(count);
    const auto request = requestAt(Point2(500.0, 500.0), katana::cad::kDefaultSnapModes);

    for (auto _ : state) {
        benchmark::DoNotOptimize(katana::cad::snap(model, request));
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(count));
}
BENCHMARK(BM_SnapAcrossManyEntities)
    ->Arg(1000)
    ->Arg(10000)
    ->Arg(100000)
    ->Arg(250000)
    ->Arg(500000)
    ->Unit(benchmark::kMicrosecond);

void BM_PickAcrossManyEntities(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const katana::entity::Model model = modelWithEntities(count);

    for (auto _ : state) {
        benchmark::DoNotOptimize(
            katana::cad::pickEntity(model, Point2(500.0, 500.0), 0.5));
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(count));
}
BENCHMARK(BM_PickAcrossManyEntities)
    ->Arg(1000)
    ->Arg(10000)
    ->Arg(100000)
    ->Unit(benchmark::kMicrosecond);

// A small window selection. The answer is a handful of entities however large
// the drawing is, so anything that scales with the drawing is pure waste.
void BM_BoxSelectSmallWindow(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const katana::entity::Model model = modelWithEntities(count);
    const katana::geometry::Box2 window(Point2(400.0, 400.0), Point2(410.0, 410.0));

    for (auto _ : state) {
        benchmark::DoNotOptimize(
            katana::cad::pickInBox(model, window, katana::cad::BoxSelectionMode::Crossing));
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(count));
}
BENCHMARK(BM_BoxSelectSmallWindow)
    ->Arg(1000)
    ->Arg(10000)
    ->Arg(100000)
    ->Unit(benchmark::kMicrosecond);

} // namespace

// The same queries THROUGH the index, so the win is a measurement rather than
// a claim. Same data, same cursor: only the broad phase differs.
namespace {

katana::geometry::SpatialIndex indexFor(const katana::entity::Model& model)
{
    std::vector<katana::geometry::SpatialEntry> entries;
    entries.reserve(model.entities.size());
    model.entities.forEach([&](const katana::entity::Entity& entity) {
        entries.push_back(katana::geometry::SpatialEntry{
            static_cast<katana::geometry::SpatialId>(entity.id),
            katana::entity::boundingBox(entity.geometry)});
    });
    katana::geometry::SpatialIndex index;
    index.rebuild(entries);
    return index;
}

void BM_SnapIndexed(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const katana::entity::Model model = modelWithEntities(count);
    const katana::geometry::SpatialIndex index = indexFor(model);
    const auto request = requestAt(Point2(500.0, 500.0), katana::cad::kDefaultSnapModes);

    for (auto _ : state) {
        benchmark::DoNotOptimize(katana::cad::snap(model, request, &index));
    }
}
BENCHMARK(BM_SnapIndexed)
    ->Arg(1000)
    ->Arg(10000)
    ->Arg(100000)
    ->Arg(250000)
    ->Arg(500000)
    ->Unit(benchmark::kMicrosecond);

void BM_PickIndexed(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const katana::entity::Model model = modelWithEntities(count);
    const katana::geometry::SpatialIndex index = indexFor(model);

    for (auto _ : state) {
        benchmark::DoNotOptimize(
            katana::cad::pickEntity(model, Point2(500.0, 500.0), 0.5, {}, &index));
    }
}
BENCHMARK(BM_PickIndexed)
    ->Arg(1000)
    ->Arg(10000)
    ->Arg(100000)
    ->Unit(benchmark::kMicrosecond);

void BM_BoxSelectIndexed(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const katana::entity::Model model = modelWithEntities(count);
    const katana::geometry::SpatialIndex index = indexFor(model);
    const katana::geometry::Box2 window(Point2(400.0, 400.0), Point2(410.0, 410.0));

    for (auto _ : state) {
        benchmark::DoNotOptimize(
            katana::cad::pickInBox(model, window, katana::cad::BoxSelectionMode::Crossing, {},
                                   &index));
    }
}
BENCHMARK(BM_BoxSelectIndexed)
    ->Arg(1000)
    ->Arg(10000)
    ->Arg(100000)
    ->Unit(benchmark::kMicrosecond);

// Building the index from scratch: what opening a project pays once.
void BM_SpatialIndexRebuild(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const katana::entity::Model model = modelWithEntities(count);
    std::vector<katana::geometry::SpatialEntry> entries;
    model.entities.forEach([&](const katana::entity::Entity& entity) {
        entries.push_back(katana::geometry::SpatialEntry{
            static_cast<katana::geometry::SpatialId>(entity.id),
            katana::entity::boundingBox(entity.geometry)});
    });

    katana::geometry::SpatialIndex index;
    for (auto _ : state) {
        index.rebuild(entries);
        benchmark::DoNotOptimize(index.size());
    }
}
BENCHMARK(BM_SpatialIndexRebuild)
    ->Arg(10000)
    ->Arg(100000)
    ->Arg(500000)
    ->Unit(benchmark::kMillisecond);

} // namespace
