// DXF export and import: the native module (katana_dxf) against GDAL's
// driver, on the same generated survey drawing, through a file both ways.
//
// It exists because the GDAL path was measured at 116 s to export a real
// 28 000 entity survey drawing - about 2 ms a feature - and a claim that the
// native writer and reader are faster must rest on something repeatable.
// The native pair runs at 1 000 entities, beside GDAL at the same size for the
// ratio, and at 28 000, the size of that real drawing; GDAL runs at 1 000
// only, since at 28 000 one iteration is a minute and a half.
//
// The drawing: points with a height, 3D strings of six vertices, labels,
// arcs, circles and lines on twenty layers - the shape of a surveyed site.

#include <benchmark/benchmark.h>

#include <cstdint>
#include <filesystem>
#include <numbers>
#include <string>

#include "katana/dxf/reader.hpp"
#include "katana/dxf/writer.hpp"
#include "katana/entity/model.hpp"

#if defined(KATANA_BENCH_WITH_INTEROP)
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#endif

namespace {

using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Point2;

// A small fixed generator (the standard's LCG constants) so every run and
// every binary builds the identical drawing.
struct Sequence {
    std::uint64_t state = 0x2545F4914F6CDD1Dull;
    double next()
    {
        state = state * 6364136223846793005ull + 1442695040888963407ull;
        return static_cast<double>(state >> 11) / static_cast<double>(1ull << 53);
    }
};

Model surveyDrawing(std::size_t count)
{
    Model model;
    for (int i = 0; i < 20; ++i) {
        (void)model.layers.add(katana::entity::Layer{.name = "survey/layer " + std::to_string(i)});
    }
    Sequence random;
    const Point2 origin(502000.0, 6250000.0);
    for (std::size_t i = 0; i < count; ++i) {
        Entity entity;
        entity.layer = "survey/layer " + std::to_string(i % 20);
        const Point2 at = origin + katana::geometry::Vec2(random.next() * 800.0, random.next() * 600.0);
        const double pick = random.next();
        std::vector<std::optional<double>> heights;
        if (pick < 0.5) {
            entity.geometry = katana::entity::PointGeometry{at};
            heights = {30.0 + random.next() * 10.0};
        } else if (pick < 0.75) {
            katana::geometry::Polyline2 line;
            for (int v = 0; v < 6; ++v) {
                line.vertices.push_back(at + katana::geometry::Vec2(v * 2.0, random.next()));
                heights.emplace_back(30.0 + random.next() * 10.0);
            }
            entity.geometry = std::move(line);
        } else if (pick < 0.85) {
            entity.geometry = katana::entity::TextGeometry{at, "RL 32.45", 0.5, random.next()};
        } else if (pick < 0.9) {
            entity.geometry = katana::geometry::Arc2{at, 1.0 + random.next() * 20.0, random.next(),
                                                     0.5 + random.next() * 2.0};
        } else if (pick < 0.95) {
            entity.geometry = katana::geometry::Circle2{at, 0.3 + random.next()};
        } else {
            entity.geometry = katana::geometry::Segment2{at, at + katana::geometry::Vec2(3.0, 4.0)};
        }
        if (!heights.empty()) {
            katana::entity::setHeights(entity.properties, heights);
        }
        (void)model.entities.add(std::move(entity));
    }
    return model;
}

std::filesystem::path scratch(const char* name)
{
    return std::filesystem::temp_directory_path() / name;
}

void BM_DxfWriteNative(benchmark::State& state)
{
    const Model model = surveyDrawing(static_cast<std::size_t>(state.range(0)));
    const auto path = scratch("katana_bench_native_write.dxf");
    for (auto _ : state) {
        auto written = katana::dxf::writeDxfFile(model, path);
        if (!written) {
            state.SkipWithError(written.error().describe().c_str());
            break;
        }
        benchmark::DoNotOptimize(written->bytesWritten);
    }
    std::filesystem::remove(path);
}
BENCHMARK(BM_DxfWriteNative)->Arg(1000)->Arg(28000)->Unit(benchmark::kMillisecond);

void BM_DxfReadNative(benchmark::State& state)
{
    const Model model = surveyDrawing(static_cast<std::size_t>(state.range(0)));
    const auto path = scratch("katana_bench_native_read.dxf");
    if (!katana::dxf::writeDxfFile(model, path)) {
        state.SkipWithError("could not write the fixture");
        return;
    }
    for (auto _ : state) {
        auto read = katana::dxf::readDxfFile(path);
        if (!read) {
            state.SkipWithError(read.error().describe().c_str());
            break;
        }
        benchmark::DoNotOptimize(read->entities.size());
    }
    std::filesystem::remove(path);
}
BENCHMARK(BM_DxfReadNative)->Arg(1000)->Arg(28000)->Unit(benchmark::kMillisecond);

#if defined(KATANA_BENCH_WITH_INTEROP)

void BM_DxfWriteGdal(benchmark::State& state)
{
    const Model model = surveyDrawing(static_cast<std::size_t>(state.range(0)));
    const auto path = scratch("katana_bench_gdal_write.dxf");
    for (auto _ : state) {
        std::filesystem::remove(path);
        auto written = katana::interop::exportVector(model, path);
        if (!written) {
            state.SkipWithError(written.error().describe().c_str());
            break;
        }
        benchmark::DoNotOptimize(written->featuresWritten);
    }
    std::filesystem::remove(path);
}
BENCHMARK(BM_DxfWriteGdal)->Arg(1000)->Unit(benchmark::kMillisecond);

// Reads the file the NATIVE writer made, so both readers read the same bytes.
void BM_DxfReadGdal(benchmark::State& state)
{
    const Model model = surveyDrawing(static_cast<std::size_t>(state.range(0)));
    const auto path = scratch("katana_bench_gdal_read.dxf");
    if (!katana::dxf::writeDxfFile(model, path)) {
        state.SkipWithError("could not write the fixture");
        return;
    }
    for (auto _ : state) {
        auto read = katana::interop::importVector(path);
        if (!read) {
            state.SkipWithError(read.error().describe().c_str());
            break;
        }
        benchmark::DoNotOptimize(read->entities.size());
    }
    std::filesystem::remove(path);
}
BENCHMARK(BM_DxfReadGdal)->Arg(1000)->Unit(benchmark::kMillisecond);

#endif

} // namespace
