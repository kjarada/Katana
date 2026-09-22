// Project save and load cost (PLAN.MD section 32: a small project opens in
// under 2 seconds).
//
// This exists to answer one question with a number rather than an opinion: is
// SQLite the bottleneck in opening and saving a drawing, or is it the JSON
// encoding of the geometry around it? The database choice recorded in
// docs/storage.md rests on this measurement, so it has to be repeatable.
//
// Each iteration uses its own directory and deletes it afterwards, so what is
// measured is a cold open rather than a warm page cache of the previous one.

#include <benchmark/benchmark.h>

#include <filesystem>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/geometry_blob.hpp"
#include "katana/entity/serialization.hpp"
#include "katana/storage/project_store.hpp"

namespace fs = std::filesystem;

using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::storage::ProjectStore;

namespace {

// A drawing shaped like real survey data: mostly short polylines (strings,
// kerbs, contours) on a handful of layers, with attributes.
Model drawingOf(std::size_t entities)
{
    Model model;
    for (const char* layer : {"asbuilt/road/kerb", "asbuilt/road/centreline",
                              "design/surface/tin1", "survey/points"}) {
        katana::entity::Layer definition;
        definition.name = layer;
        const auto status = model.layers.add(definition);
        benchmark::DoNotOptimize(status.ok());
    }
    const char* layers[] = {"asbuilt/road/kerb", "asbuilt/road/centreline",
                            "design/surface/tin1", "survey/points"};

    for (std::size_t i = 0; i < entities; ++i) {
        Polyline2 polyline;
        const double base = static_cast<double>(i);
        for (int v = 0; v < 8; ++v) {
            polyline.vertices.emplace_back(base * 1.7 + v * 0.9, base * 0.3 + v * 1.4);
        }
        Entity entity;
        entity.geometry = std::move(polyline);
        entity.layer = layers[i % 4];
        entity.properties.emplace("code", std::string("KB"));
        entity.properties.emplace("chainage", base * 2.5);
        const auto id = model.entities.add(std::move(entity));
        benchmark::DoNotOptimize(id.ok());
    }
    return model;
}

fs::path scratch(int index)
{
    return fs::temp_directory_path() /
           ("katana_bench_store_" + std::to_string(index));
}

void BM_ProjectSave(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const Model model = drawingOf(count);
    int index = 0;

    for (auto _ : state) {
        state.PauseTiming();
        const fs::path directory = scratch(index++);
        std::error_code ignored;
        fs::remove_all(directory, ignored);
        auto store = ProjectStore::create(directory, {});
        const bool ready = store.ok();
        state.ResumeTiming();

        if (ready) {
            const auto status = store->save(katana::storage::captureModel(model, {}));
            benchmark::DoNotOptimize(status.ok());
        }

        state.PauseTiming();
        // Destroy the store (closing the database) before deleting the
        // directory, or the open handle keeps the file alive on Windows.
        store = katana::core::makeError(katana::core::ErrorCode::Internal, "closed");
        fs::remove_all(directory, ignored);
        state.ResumeTiming();
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(count));
}
BENCHMARK(BM_ProjectSave)->Arg(1000)->Arg(10000)->Arg(50000)->Unit(benchmark::kMillisecond);

void BM_ProjectOpenAndLoad(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const Model model = drawingOf(count);

    // Written once; every iteration opens it fresh, which is what a user does.
    const fs::path directory = scratch(9000 + static_cast<int>(count % 100));
    std::error_code ignored;
    fs::remove_all(directory, ignored);
    {
        auto store = ProjectStore::create(directory, {});
        if (store.ok()) {
            const auto status = store->save(katana::storage::captureModel(model, {}));
            benchmark::DoNotOptimize(status.ok());
        }
    }

    for (auto _ : state) {
        auto store = ProjectStore::open(directory);
        if (store.ok()) {
            auto contents = store->load();
            benchmark::DoNotOptimize(contents.ok());
            if (contents.ok()) {
                Model loaded;
                // Consumed, as Document::open consumes it. `contents` is loaded
                // afresh every iteration, so there is nothing to reuse.
                const auto status = applyToModel(std::move(*contents), loaded);
                benchmark::DoNotOptimize(status.ok());
            }
        }
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(count));
    fs::remove_all(directory, ignored);
}
BENCHMARK(BM_ProjectOpenAndLoad)
    ->Arg(1000)
    ->Arg(10000)
    ->Arg(50000)
    ->Unit(benchmark::kMillisecond);

// The same geometry encoded and decoded WITHOUT touching a database, so the
// encoding cost can be subtracted from the save and load figures above. Split
// into two halves because save only encodes and load only decodes: measuring
// the round trip alone would leave the attribution to guesswork.
void BM_GeometryJsonEncode(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const Model model = drawingOf(count);
    std::vector<katana::entity::Entity> entities;
    model.entities.forEach([&](const Entity& entity) { entities.push_back(entity); });

    for (auto _ : state) {
        for (const Entity& entity : entities) {
            auto json = katana::entity::geometryToJson(entity.geometry);
            benchmark::DoNotOptimize(json.ok());
        }
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(count));
}
BENCHMARK(BM_GeometryJsonEncode)
    ->Arg(1000)
    ->Arg(10000)
    ->Arg(50000)
    ->Unit(benchmark::kMillisecond);

void BM_GeometryJsonDecode(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const Model model = drawingOf(count);
    std::vector<std::string> encoded;
    model.entities.forEach([&](const Entity& entity) {
        auto json = katana::entity::geometryToJson(entity.geometry);
        if (json.ok()) {
            encoded.push_back(*json);
        }
    });

    for (auto _ : state) {
        for (const std::string& json : encoded) {
            auto back = katana::entity::geometryFromJson(json);
            benchmark::DoNotOptimize(back.ok());
        }
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(count));
}
BENCHMARK(BM_GeometryJsonDecode)
    ->Arg(1000)
    ->Arg(10000)
    ->Arg(50000)
    ->Unit(benchmark::kMillisecond);


// The binary path that replaced the JSON one, measured the same way so
// docs/storage.md can be re-verified rather than trusted. The JSON benchmarks
// above are kept deliberately: they are the baseline the decision rests on,
// and a format change that regressed against them should be visible.
void BM_GeometryBlobEncode(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const Model model = drawingOf(count);
    std::vector<katana::entity::Entity> entities;
    model.entities.forEach([&](const Entity& entity) { entities.push_back(entity); });

    for (auto _ : state) {
        for (const Entity& entity : entities) {
            auto blob = katana::entity::geometryToBlob(entity.geometry);
            benchmark::DoNotOptimize(blob.ok());
        }
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(count));
}
BENCHMARK(BM_GeometryBlobEncode)
    ->Arg(1000)
    ->Arg(10000)
    ->Arg(50000)
    ->Unit(benchmark::kMillisecond);

void BM_GeometryBlobDecode(benchmark::State& state)
{
    const auto count = static_cast<std::size_t>(state.range(0));
    const Model model = drawingOf(count);
    std::vector<std::vector<std::byte>> encoded;
    model.entities.forEach([&](const Entity& entity) {
        auto blob = katana::entity::geometryToBlob(entity.geometry);
        if (blob.ok()) {
            encoded.push_back(std::move(*blob));
        }
    });

    for (auto _ : state) {
        for (const auto& blob : encoded) {
            auto back = katana::entity::geometryFromBlob(blob);
            benchmark::DoNotOptimize(back.ok());
        }
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(count));
}
BENCHMARK(BM_GeometryBlobDecode)
    ->Arg(1000)
    ->Arg(10000)
    ->Arg(50000)
    ->Unit(benchmark::kMillisecond);

} // namespace
