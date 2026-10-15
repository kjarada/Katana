// What the road model's cache saves (bim::bimModel, docs/road_design.md "The
// cache"): a kilometre of the urban street on a TIN, sectioned every 10 m, with a
// sign placed along it that stands on a second surface. The 3D views, the plan
// views and the dialogs ask for the model on every paint, so what matters is
// what an ordinary edit costs the next paint.
//
//   BM_RoadModelUnchanged                  nothing moved: the keys are compared
//   BM_RoadModelAfterAnUnrelatedSurface    a surface no corridor targets and no
//                                          object names is replaced, as an edit
//                                          of it does: it was a full rebuild of
//                                          every corridor; now the keys only
//   BM_RoadModelAfterTheTargetedSurface    the surface the corridor targets is
//                                          replaced: the corridors are made again
//   BM_RoadModelAfterTheNamedSurface       the surface the sign names is
//                                          replaced: the placed objects only
//
// Counters say how many times each half was made for each iteration, which is
// the point: 0 where nothing it reads moved.

#include <benchmark/benchmark.h>

#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace {

// A tilted grid, 1 040 m by 200 m at 10 m, a different surface for each `level`.
std::shared_ptr<const katana::terrain::TinSurface> tilted(double level)
{
    constexpr int kAlong = 105;
    constexpr int kAcross = 21;
    std::vector<katana::geometry::Point3> vertices;
    std::vector<katana::terrain::TinTriangle> triangles;
    for (int j = 0; j < kAcross; ++j) {
        for (int i = 0; i < kAlong; ++i) {
            const double x = -20.0 + 10.0 * i;
            const double y = -100.0 + 10.0 * j;
            vertices.emplace_back(x, y, level + 0.0005 * x + 0.02 * y);
        }
    }
    const auto at = [](int i, int j) { return static_cast<std::uint32_t>(j * kAlong + i); };
    for (int j = 0; j + 1 < kAcross; ++j) {
        for (int i = 0; i + 1 < kAlong; ++i) {
            triangles.push_back({at(i, j), at(i + 1, j), at(i + 1, j + 1)});
            triangles.push_back({at(i, j), at(i + 1, j + 1), at(i, j + 1)});
        }
    }
    auto made = katana::terrain::TinSurface::create(std::move(vertices), std::move(triangles));
    return std::make_shared<const katana::terrain::TinSurface>(std::move(*made));
}

struct Site {
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    // Two surfaces for each name, taken in turn so that every edit is a
    // different object.
    std::shared_ptr<const katana::terrain::TinSurface> low = tilted(8.0);
    std::shared_ptr<const katana::terrain::TinSurface> high = tilted(9.0);
    bool flip = false;

    Site()
    {
        katana::terrain::SurfaceStore store;
        for (const char* name : {"ground", "dem", "other"}) {
            (void)store.add(katana::terrain::NamedSurface{name, low, "benchmark", {}, true});
        }
        (void)document.setSurfaces(std::move(store), "BENCH_SURFACES");
        for (const char* line :
             {"ALIGN NEW A 0,0 1000,0", "ALIGN DESIGN A 0,10 1000,10", "ROADTEMPLATE DEFAULTS",
              "CORRIDOR NEW Main ALIGNMENT A TEMPLATE \"urban street\" surface=ground",
              "ROADOBJECT ADD sign ALIGNMENT A station=500 offset=4 surface=dem"}) {
            (void)interpreter.run(line);
        }
        (void)katana::cad::bim::bimModel(document);
    }

    // The surface `name` replaced by the other of the two.
    void edit(const std::string& name)
    {
        flip = !flip;
        katana::terrain::SurfaceStore store = document.surfaces();
        (void)store.replace(name, flip ? high : low);
        (void)document.setSurfaces(std::move(store), "BENCH_EDIT");
    }
};

void report(benchmark::State& state, const katana::cad::bim::BimBuildCounts& before,
            const katana::cad::bim::BimBuildCounts& after)
{
    const double iterations = static_cast<double>(state.iterations());
    state.counters["corridors_made"] =
        static_cast<double>(after.corridors - before.corridors) / iterations;
    state.counters["placed_made"] = static_cast<double>(after.placed - before.placed) / iterations;
}

void BM_RoadModelUnchanged(benchmark::State& state)
{
    Site site;
    const auto before = katana::cad::bim::bimBuildCounts(site.document);
    for (auto _ : state) {
        benchmark::DoNotOptimize(katana::cad::bim::bimModel(site.document));
    }
    report(state, before, katana::cad::bim::bimBuildCounts(site.document));
}

void editThenAsk(benchmark::State& state, const char* name)
{
    Site site;
    const auto before = katana::cad::bim::bimBuildCounts(site.document);
    for (auto _ : state) {
        site.edit(name);
        benchmark::DoNotOptimize(katana::cad::bim::bimModel(site.document));
    }
    report(state, before, katana::cad::bim::bimBuildCounts(site.document));
}

void BM_RoadModelAfterAnUnrelatedSurface(benchmark::State& state)
{
    editThenAsk(state, "other");
}

void BM_RoadModelAfterTheTargetedSurface(benchmark::State& state)
{
    editThenAsk(state, "ground");
}

void BM_RoadModelAfterTheNamedSurface(benchmark::State& state)
{
    editThenAsk(state, "dem");
}

} // namespace

BENCHMARK(BM_RoadModelUnchanged);
BENCHMARK(BM_RoadModelAfterAnUnrelatedSurface)->Unit(benchmark::kMillisecond);
BENCHMARK(BM_RoadModelAfterTheTargetedSurface)->Unit(benchmark::kMillisecond);
BENCHMARK(BM_RoadModelAfterTheNamedSurface)->Unit(benchmark::kMillisecond);
