// Export cost on a drawing of real size.
//
// Export is not interactive, but it is waited for: a GeoPackage of the owner's
// 27,886-entity corridor drawing was measured at 67 s through Katana, against
// 3.2 s for ogr2ogr writing the same features. The difference was the
// transaction: SQLite commits, and so syncs the file, once per transaction,
// and Katana gave each feature its own. The benchmark writes the generated
// stand-in for that drawing (generated_drawing.hpp).

#include <benchmark/benchmark.h>

#include <atomic>
#include <filesystem>
#include <string>
#include <system_error>

#include "generated_drawing.hpp"
#include "katana/entity/model.hpp"
#include "katana/interop/export.hpp"

namespace {

katana::entity::Model modelOf(std::size_t count)
{
    katana::entity::Model model;
    for (katana::entity::Entity& entity : katana::bench::generatedSurveyDrawing(count)) {
        const auto layer = model.layers.ensure(entity.layer);
        const auto id = model.entities.add(std::move(entity));
        benchmark::DoNotOptimize(layer.ok() && id.ok());
    }
    return model;
}

// A file of this process's own, so two benchmark binaries run side by side
// (tools/compare_benchmarks.py --alternate runs them in turn, but an A/A pair
// is the same binary) never write one file.
std::filesystem::path scratchFile(const char* extension)
{
    static std::atomic<int> serial{0};
    return std::filesystem::temp_directory_path() /
           ("katana_bench_export_" + std::to_string(katana::bench::kCorridorEntityCount) + "_" +
            std::to_string(static_cast<unsigned long long>(
                std::filesystem::file_time_type::clock::now().time_since_epoch().count())) +
            "_" + std::to_string(serial++) + extension);
}

void exportTo(benchmark::State& state, const char* extension)
{
    const katana::entity::Model model = modelOf(static_cast<std::size_t>(state.range(0)));
    const std::filesystem::path path = scratchFile(extension);
    std::uint64_t written = 0;
    for (auto _ : state) {
        auto result = katana::interop::exportVector(model, path);
        if (!result) {
            state.SkipWithError(result.error().describe().c_str());
            break;
        }
        written = result->featuresWritten;
    }
    state.counters["features"] = static_cast<double>(written);
    std::error_code ignored;
    std::filesystem::remove(path, ignored);
}

void BM_ExportGeoPackage(benchmark::State& state)
{
    exportTo(state, ".gpkg");
}
// 2,000 for the alternating comparison, which runs every benchmark several
// times per binary; the full corridor for the one-off before-and-after.
BENCHMARK(BM_ExportGeoPackage)
    ->Arg(2000)
    ->Arg(katana::bench::kCorridorEntityCount)
    ->Unit(benchmark::kMillisecond)
    ->Iterations(1);

} // namespace
