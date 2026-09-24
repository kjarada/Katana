// Snapping cost per mouse move (PLAN.MD section 32: sub-16 ms interaction).
//
// Snapping runs synchronously in the viewport's mouseMoveEvent, so whatever this
// measures is paid on every single mouse move. The polyline sizes are chosen to
// be ordinary survey data: a surveyed kerb line or contour easily carries a few
// thousand vertices.

#include <benchmark/benchmark.h>

#include <cmath>
#include <memory>

#include "generated_drawing.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/selection.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/cad/spatial_query.hpp"
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


// Collecting the entities a repaint has to draw. This is the per-FRAME scan:
// it runs on every pan, every zoom and every redraw, not just on a click.
// `zoomed` decides how much of the drawing is on screen - the interesting case
// is a user working at detail scale, where almost nothing is visible and the
// scan is pure waste.
void collectVisible(benchmark::State& state, bool indexed, double fraction)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const katana::entity::Model model = modelWithEntities(count);
    const katana::geometry::SpatialIndex index = indexFor(model);
    const double half = 500.0 * fraction;
    const katana::geometry::Box2 view(Point2(500.0 - half, 500.0 - half),
                                      Point2(500.0 + half, 500.0 + half));

    std::vector<katana::geometry::SpatialId> scratch;
    std::size_t drawn = 0;
    for (auto _ : state) {
        drawn = 0;
        katana::cad::detail::forEachCandidate(
            model, indexed ? &index : nullptr, view, scratch,
            [&](const katana::entity::Entity& entity) {
                // Stand-in for the per-entity draw work, so the loop cannot be
                // optimised away and the measurement is of the traversal.
                auto id = entity.id;
                benchmark::DoNotOptimize(id);
                ++drawn;
            });
        benchmark::DoNotOptimize(drawn);
    }
    state.counters["visible"] = static_cast<double>(drawn);
}

// Zoomed in to 1% of the extent: a handful of entities on screen.
void BM_CollectVisibleScan(benchmark::State& state) { collectVisible(state, false, 0.01); }
BENCHMARK(BM_CollectVisibleScan)
    ->Arg(10000)
    ->Arg(100000)
    ->Arg(500000)
    ->Unit(benchmark::kMicrosecond);

void BM_CollectVisibleIndexed(benchmark::State& state) { collectVisible(state, true, 0.01); }
BENCHMARK(BM_CollectVisibleIndexed)
    ->Arg(10000)
    ->Arg(100000)
    ->Arg(500000)
    ->Unit(benchmark::kMicrosecond);

// Zoomed out to the whole drawing: everything is visible, so the index cannot
// help and must not HURT. This is the case that says the change is safe.
void BM_CollectEverythingScan(benchmark::State& state) { collectVisible(state, false, 1.0); }
BENCHMARK(BM_CollectEverythingScan)->Arg(100000)->Unit(benchmark::kMicrosecond);

void BM_CollectEverythingIndexed(benchmark::State& state) { collectVisible(state, true, 1.0); }
BENCHMARK(BM_CollectEverythingIndexed)->Arg(100000)->Unit(benchmark::kMicrosecond);


// Finding the crossover, so the heuristic that picks between the two paths is
// chosen from a measurement rather than from intuition. `fraction` is the
// LINEAR share of the extent on screen, so the area share is its square.
void BM_CollectFractionScan(benchmark::State& state)
{
    collectVisible(state, false, static_cast<double>(state.range(1)) / 100.0);
}
BENCHMARK(BM_CollectFractionScan)
    ->Args({100000, 10})
    ->Args({100000, 20})
    ->Args({100000, 30})
    ->Args({100000, 50})
    ->Args({100000, 70})
    ->Unit(benchmark::kMicrosecond);

void BM_CollectFractionIndexed(benchmark::State& state)
{
    collectVisible(state, true, static_cast<double>(state.range(1)) / 100.0);
}
BENCHMARK(BM_CollectFractionIndexed)
    ->Args({100000, 10})
    ->Args({100000, 20})
    ->Args({100000, 30})
    ->Args({100000, 50})
    ->Args({100000, 70})
    ->Unit(benchmark::kMicrosecond);

} // namespace

// ---- the index after an IMPORT, through the Document ------------------------------
//
// Everything above hands a ready-built index to the query. These go through
// Document::execute, which is where an import lands and where the index is
// kept in step with the model, because that is where it went wrong: an index
// fed 27,886 incremental inserts while still on its default one-unit cell was
// measured at 722,715 buckets and 4,889 oversized entries, the import's
// execute took 0.87-1.26 s against about 28 ms for the create itself, and a
// window query was 36x slower until the project was reopened.

namespace {

// The one IMPORT transaction a file import makes: the layer the generated
// drawing is on (an entity on a layer the drawing lacks is refused), then
// every entity in one bulk create.
std::unique_ptr<katana::commands::Transaction>
importOf(const std::vector<katana::entity::Entity>& entities)
{
    auto transaction = std::make_unique<katana::commands::Transaction>("IMPORT");
    katana::entity::Layer layer;
    layer.name = "SURVEY";
    transaction->add(katana::commands::createLayer(layer));
    transaction->add(katana::commands::createEntities(entities));
    return transaction;
}

// What the user waits for when File > Import lands: the execute alone, not
// the reading of the file and not the document's construction or teardown.
void BM_ImportExecute(benchmark::State& state)
{
    const auto entities =
        katana::bench::generatedSurveyDrawing(static_cast<std::size_t>(state.range(0)));
    for (auto _ : state) {
        state.PauseTiming();
        auto document = std::make_unique<katana::cad::Document>();
        auto transaction = importOf(entities);
        state.ResumeTiming();
        const auto status = document->execute(std::move(transaction));
        state.PauseTiming();
        if (!status) {
            state.SkipWithError("import failed");
            break;
        }
        document.reset();
        state.ResumeTiming();
    }
}
BENCHMARK(BM_ImportExecute)
    ->Arg(katana::bench::kCorridorEntityCount)
    ->Unit(benchmark::kMillisecond);

// A 1% window query (by area) on the index the import left behind: what
// every snap, pick and plan repaint pays until the project is reopened.
void BM_WindowQueryAfterImport(benchmark::State& state)
{
    katana::cad::Document document;
    const auto status = document.execute(
        importOf(katana::bench::generatedSurveyDrawing(static_cast<std::size_t>(state.range(0)))));
    if (!status) {
        state.SkipWithError("import failed");
        return;
    }
    // The middle tenth of each side of the generated 10 km x 8 km extent.
    const katana::geometry::Box2 window(Point2(304500.0, 6253600.0), Point2(305500.0, 6254400.0));
    std::vector<katana::geometry::SpatialId> found;
    for (auto _ : state) {
        document.spatialIndex().query(window, found);
        benchmark::DoNotOptimize(found.data());
    }
    state.counters["found"] = static_cast<double>(found.size());
    state.counters["buckets"] = static_cast<double>(document.spatialIndex().bucketCount());
    state.counters["oversized"] = static_cast<double>(document.spatialIndex().oversizedCount());
}
BENCHMARK(BM_WindowQueryAfterImport)
    ->Arg(katana::bench::kCorridorEntityCount)
    ->Unit(benchmark::kMicrosecond);

// One line drawn into, and undone from, a drawing that already holds the
// import: the ordinary click. It must stay incremental - whatever makes an
// import rebuild the index must not make every click rebuild it.
void BM_DrawOneLineIntoImportedDrawing(benchmark::State& state)
{
    katana::cad::Document document;
    const auto status = document.execute(
        importOf(katana::bench::generatedSurveyDrawing(static_cast<std::size_t>(state.range(0)))));
    if (!status) {
        state.SkipWithError("import failed");
        return;
    }
    for (auto _ : state) {
        const auto drawn = document.execute(
            katana::commands::createLine(Point2(305000.0, 6254000.0), Point2(305012.0, 6254005.0)));
        const auto undone = document.undo();
        benchmark::DoNotOptimize(drawn.ok() && undone.ok());
    }
}
BENCHMARK(BM_DrawOneLineIntoImportedDrawing)
    ->Arg(katana::bench::kCorridorEntityCount)
    ->Unit(benchmark::kMicrosecond);

// A MOVE, and its undo, of a tenth of the imported drawing, and of one
// entity fewer. A rule that rebuilt the index for any command touching a
// tenth of the drawing was tried and measured here: 101 ms against 24 ms
// (min of 9) - two whole rebuilds for a move that changes no box's size. The
// rules kept (Document::applyToSpatialIndex) leave both incremental, so the
// two should cost the same: a step between them is that mistake back.
void moveAFractionOfAnImport(benchmark::State& state, std::size_t lessBy)
{
    katana::cad::Document document;
    const auto status = document.execute(
        importOf(katana::bench::generatedSurveyDrawing(static_cast<std::size_t>(state.range(0)))));
    if (!status) {
        state.SkipWithError("import failed");
        return;
    }
    // The fewest entities that are a tenth: 2,789 of 27,886 (27,890 >= 27,886),
    // where 2,788 (27,880) is not.
    const std::size_t count = (document.model().entities.size() + 9) / 10 - lessBy;
    std::vector<katana::entity::EntityId> ids;
    document.model().entities.forEach([&](const katana::entity::Entity& entity) {
        if (ids.size() < count) {
            ids.push_back(entity.id);
        }
    });
    for (auto _ : state) {
        const auto moved = document.execute(katana::commands::moveEntities(ids, {2.5, -1.5}));
        const auto undone = document.undo();
        benchmark::DoNotOptimize(moved.ok() && undone.ok());
    }
    state.counters["moved"] = static_cast<double>(ids.size());
}
void BM_MoveATenthOfAnImport(benchmark::State& state)
{
    moveAFractionOfAnImport(state, 0);
}
void BM_MoveJustUnderATenthOfAnImport(benchmark::State& state)
{
    moveAFractionOfAnImport(state, 1);
}
BENCHMARK(BM_MoveATenthOfAnImport)
    ->Arg(katana::bench::kCorridorEntityCount)
    ->Unit(benchmark::kMillisecond);
BENCHMARK(BM_MoveJustUnderATenthOfAnImport)
    ->Arg(katana::bench::kCorridorEntityCount)
    ->Unit(benchmark::kMillisecond);

} // namespace
