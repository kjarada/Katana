// Snapping cost per mouse move (PLAN.MD section 32: sub-16 ms interaction).
//
// Snapping runs synchronously in the viewport's mouseMoveEvent, so whatever this
// measures is paid on every single mouse move. The polyline sizes are chosen to
// be ordinary survey data: a surveyed kerb line or contour easily carries a few
// thousand vertices.

#include <benchmark/benchmark.h>

#include <cmath>

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
