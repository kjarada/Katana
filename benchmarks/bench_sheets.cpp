// What a large sheet set costs (docs/plotting.md): generating it, storing it
// as JSON, and one undoable edit of it.
//
// The set is what a long job makes: a 10 km road drawn plan-and-profile at
// 1:500 (53 sheets of 191 m) with cross sections every 20 m (501 sections,
// eight to a sheet: 63 sheets) - 116 sheets, 607 viewports. Every edit of a
// sheet writes the whole set's JSON once (an undo step keeps the parsed set
// beside the text, so undo and redo parse nothing), so the edit benchmark is
// the price of that design at this size; the counters report the set's size
// so a later change can be compared like for like. docs/plotting.md has the
// measured numbers.

#include <benchmark/benchmark.h>

#include <cstddef>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/sheet_json.hpp"

using namespace katana::cad::plotting;

namespace {

katana::entity::Model longRoad()
{
    katana::entity::Model model;
    katana::entity::Alignment road;
    road.name = "ROAD";
    road.horizontal.pis = {{katana::geometry::Point2(0.0, 0.0)},
                           {katana::geometry::Point2(10000.0, 0.0)}};
    benchmark::DoNotOptimize(model.alignments.add(road).ok());
    return model;
}

LayoutRequest longRoadRequest()
{
    LayoutRequest request;
    request.alignment = "ROAD";
    request.planAlongAlignment = true;
    request.longSection = true;
    request.crossSectionInterval = 20.0;
    request.scale = 500.0;
    return request;
}

SheetSet longRoadSet()
{
    SheetSet set;
    auto sheets = smartLayout(longRoad(), longRoadRequest());
    if (sheets.ok()) {
        set.sheets = std::move(*sheets);
    }
    return set;
}

std::size_t viewportCount(const SheetSet& set)
{
    std::size_t count = 0;
    for (const Sheet& sheet : set.sheets) {
        count += sheet.viewports.size();
    }
    return count;
}

void BM_SheetSmartLayoutLongRoad(benchmark::State& state)
{
    const katana::entity::Model model = longRoad();
    const LayoutRequest request = longRoadRequest();
    std::size_t sheets = 0;
    for (auto _ : state) {
        auto out = smartLayout(model, request);
        sheets = out.ok() ? out->size() : 0;
        benchmark::DoNotOptimize(sheets);
    }
    state.counters["sheets"] = static_cast<double>(sheets);
}
BENCHMARK(BM_SheetSmartLayoutLongRoad)->Unit(benchmark::kMillisecond);

void BM_SheetSetToJson(benchmark::State& state)
{
    const SheetSet set = longRoadSet();
    std::size_t bytes = 0;
    for (auto _ : state) {
        auto json = sheetSetToJson(set);
        bytes = json.ok() ? json->size() : 0;
        benchmark::DoNotOptimize(bytes);
    }
    state.counters["sheets"] = static_cast<double>(set.sheets.size());
    state.counters["viewports"] = static_cast<double>(viewportCount(set));
    state.counters["bytes"] = static_cast<double>(bytes);
}
BENCHMARK(BM_SheetSetToJson)->Unit(benchmark::kMillisecond);

void BM_SheetSetFromJson(benchmark::State& state)
{
    const auto json = sheetSetToJson(longRoadSet());
    const std::string text = json.ok() ? *json : std::string{};
    for (auto _ : state) {
        auto set = sheetSetFromJson(text);
        benchmark::DoNotOptimize(set.ok());
    }
    state.counters["bytes"] = static_cast<double>(text.size());
}
BENCHMARK(BM_SheetSetFromJson)->Unit(benchmark::kMillisecond);

// One undoable viewport edit and its undo, each followed by the read a
// painter or editor makes (sheetSet()): the whole round a drag's release
// costs. Undone every time so the history stays one step deep.
void BM_SheetEditViewportAndUndo(benchmark::State& state)
{
    katana::cad::Document document;
    benchmark::DoNotOptimize(document.setSheetSet(longRoadSet()).ok());
    const std::string id = document.sheetSet().sheets.back().viewports.back().id;
    for (auto _ : state) {
        const auto edited = editViewport(document, id, [](Viewport& viewport) {
            viewport.scale = 250.0;
            return katana::core::Status{};
        });
        benchmark::DoNotOptimize(edited.ok());
        benchmark::DoNotOptimize(document.sheetSet().sheets.size());
        benchmark::DoNotOptimize(document.undo().ok());
        benchmark::DoNotOptimize(document.sheetSet().sheets.size());
    }
    state.counters["sheets"] = static_cast<double>(document.sheetSet().sheets.size());
}
BENCHMARK(BM_SheetEditViewportAndUndo)->Unit(benchmark::kMillisecond);

} // namespace
