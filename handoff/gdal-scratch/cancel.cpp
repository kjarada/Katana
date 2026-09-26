// Systematic: does Run() report cancellation when progress returns FALSE?
#include "kit.hpp"
#include <functional>
#include <memory>

using AlgPtr = std::unique_ptr<GDALAlgorithm>;

static bool bind(GDALAlgorithm& alg, const char* name, GDALDataset* ds)
{
    GDALAlgorithmArg* arg = alg.GetArg(name);
    if (!arg) return false;
    if (arg->GetType() == GAAT_DATASET) return arg->Set(ds);
    std::vector<GDALArgDatasetValue> v;
    v.emplace_back(ds);
    return arg->Set(std::move(v));
}

struct Case {
    std::vector<std::string> path;
    bool vectorIn;
    std::function<void(GDALAlgorithm&)> extra;
};

int main(int argc, char** argv)
{
    setupGdal();
    const char* fmt = argc > 1 ? argv[1] : "MEM";
    // 600x600 synthetic DEM so every algorithm makes several progress calls.
    const int n = 600;
    GDALDataset* dem = GetGDALDriverManager()->GetDriverByName("MEM")->Create("", n, n, 1, GDT_Float32, nullptr);
    double gt[6] = {0, 1, 0, double(n), 0, -1};
    dem->SetGeoTransform(gt);
    OGRSpatialReference srs;
    srs.importFromEPSG(32630);
    dem->SetSpatialRef(&srs);
    std::vector<float> buf(size_t(n) * n);
    for (int y = 0; y < n; ++y)
        for (int x = 0; x < n; ++x) buf[size_t(y) * n + x] = float(20 + 10 * std::sin(x * 0.02) * std::cos(y * 0.017));
    (void)dem->GetRasterBand(1)->RasterIO(GF_Write, 0, 0, n, n, buf.data(), n, n, GDT_Float32, 0, 0);

    GDALDataset* pts = GetGDALDriverManager()->GetDriverByName("MEM")->Create("", 0, 0, 0, GDT_Unknown, nullptr);
    OGRLayer* lyr = pts->CreateLayer("p", &srs, wkbPoint25D, nullptr);
    for (int i = 0; i < 3000; ++i) {
        OGRFeature f(lyr->GetLayerDefn());
        double x = (i * 7919) % n, y = (i * 104729) % n;
        f.SetGeometry(std::make_unique<OGRPoint>(x, y, 20 + std::sin(x * 0.02) * 10));
        (void)lyr->CreateFeature(&f);
    }

    std::vector<Case> cases = {
        {{"raster", "hillshade"}, false, nullptr},
        {{"raster", "slope"}, false, nullptr},
        {{"raster", "aspect"}, false, nullptr},
        {{"raster", "roughness"}, false, nullptr},
        {{"raster", "tpi"}, false, nullptr},
        {{"raster", "viewshed"}, false, [](GDALAlgorithm& a) { a["position"] = std::vector<double>{300, 300}; }},
        {{"raster", "contour"}, false, [](GDALAlgorithm& a) { a["interval"] = 0.25; }},
        {{"raster", "polygonize"}, false, nullptr},
        {{"raster", "proximity"}, false, [](GDALAlgorithm& a) { a["target-values"] = std::vector<double>{25}; }},
        {{"raster", "fill-nodata"}, false, nullptr},
        {{"raster", "reproject"}, false, [](GDALAlgorithm& a) { a["dst-crs"] = "EPSG:4326"; }},
        {{"raster", "resize"}, false, [](GDALAlgorithm& a) { a["size"] = std::vector<std::string>{"1200", "1200"}; }},
        {{"raster", "calc"}, false, [](GDALAlgorithm& a) { a["calc"] = std::vector<std::string>{"X*2"}; }},
        {{"raster", "color-map"}, false, nullptr},
        {{"vector", "buffer"}, true, [](GDALAlgorithm& a) { a["distance"] = 3.0; }},
        {{"vector", "simplify"}, true, [](GDALAlgorithm& a) { a["tolerance"] = 1.0; }},
        {{"vector", "grid", "invdist"}, true, [](GDALAlgorithm& a) { a["size"] = std::vector<int>{300, 300}; }},
        {{"vector", "rasterize"}, true, [](GDALAlgorithm& a) { a["resolution"] = std::vector<double>{1, 1}; a["burn"] = std::vector<double>{1}; }},
        {{"vector", "concave-hull"}, true, [](GDALAlgorithm& a) { a["ratio"] = 0.3; }},
    };
    printf("%-28s %-6s %-6s %-5s %-7s %-8s %s\n", "algorithm", "full", "cancel", "calls", "outNull", "finalize", "errors");
    for (auto& c : cases) {
        std::string name;
        for (auto& p : c.path) name += p + " ";
        // Full run, counting progress calls.
        int fullCalls = 0;
        bool fullOk = false;
        {
            auto alg = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(c.path);
            if (!alg) { printf("%s: cannot instantiate\n", name.c_str()); continue; }
            bind(*alg, "input", c.vectorIn ? pts : dem);
            (*alg)["output-format"] = fmt;
            alg->GetArg("output")->SetDatasetName(std::string(fmt) == "MEM" ? "" : "/vsimem/c/out." + std::string(std::string(fmt) == "GTiff" ? "tif" : "gpkg"));
            if (alg->GetArg("overwrite")) (*alg)["overwrite"] = true;
            if (c.extra) c.extra(*alg);
            Progress p;
            ErrorCollector ec;
            fullOk = alg->Run(Progress::fn, &p) && alg->Finalize();
            fullCalls = p.calls;
            if (!fullOk) { printf("%-28s FULL RUN FAILED\n", name.c_str()); ec.print("full"); continue; }
        }
        // Cancel after the 2nd progress call.
        auto alg = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(c.path);
        bind(*alg, "input", c.vectorIn ? pts : dem);
        (*alg)["output-format"] = fmt;
        alg->GetArg("output")->SetDatasetName(std::string(fmt) == "MEM" ? "" : "/vsimem/c/out." + std::string(std::string(fmt) == "GTiff" ? "tif" : "gpkg"));
        if (alg->GetArg("overwrite")) (*alg)["overwrite"] = true;
        if (c.extra) c.extra(*alg);
        struct CancelAfter { int calls = 0; int after = 2; static int CPL_STDCALL fn(double, const char*, void* d) { auto* s = static_cast<CancelAfter*>(d); return ++s->calls < s->after; } } ca;
        ErrorCollector ec;
        bool ok = alg->Run(CancelAfter::fn, &ca);
        GDALDataset* o = alg->GetArg("output")->Get<GDALArgDatasetValue>().GetDatasetRef();
        bool fin = alg->Finalize();
        std::string errs;
        for (auto& e : ec.items) errs += "[" + e.msg + "] ";
        printf("%-28s %-6d %-6s %-5d %-7s %-8d %s\n", name.c_str(), fullCalls, ok ? "IGNORED" : "ok", ca.calls, o ? "no" : "yes", fin, errs.c_str());
    }
    GDALClose(pts);
    GDALClose(dem);
    return 0;
}
