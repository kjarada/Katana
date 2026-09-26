#include "kit.hpp"
#include <memory>
static bool bind(GDALAlgorithm& alg, const char* name, GDALDataset* ds)
{ auto* arg = alg.GetArg(name); if (arg->GetType()==GAAT_DATASET) return arg->Set(ds); std::vector<GDALArgDatasetValue> v; v.emplace_back(ds); return arg->Set(std::move(v)); }
struct Seq { std::vector<double> v; static int CPL_STDCALL fn(double c, const char*, void* d) { static_cast<Seq*>(d)->v.push_back(c); return TRUE; } };
static void report(const char* tag, const Seq& s)
{
    int back = 0; double maxBack = 0;
    for (size_t i = 1; i < s.v.size(); ++i) if (s.v[i] < s.v[i-1]) { ++back; maxBack = std::max(maxBack, s.v[i-1]-s.v[i]); }
    printf("%s: calls=%zu first=%.3f last=%.3f backwards=%d maxBack=%.3f samples:", tag, s.v.size(), s.v.empty()?-1:s.v.front(), s.v.empty()?-1:s.v.back(), back, maxBack);
    for (size_t i = 0; i < s.v.size(); i += std::max<size_t>(1, s.v.size()/12)) printf(" %.3f", s.v[i]);
    printf("\n");
}
int main()
{
    setupGdal();
    const int n = 2000;
    GDALDataset* big = GetGDALDriverManager()->GetDriverByName("MEM")->Create("", n, n, 1, GDT_Float32, nullptr);
    double gt[6] = {0, 1, 0, double(n), 0, -1}; big->SetGeoTransform(gt);
    std::vector<float> row(n);
    for (int y = 0; y < n; ++y) { for (int x = 0; x < n; ++x) row[x] = float(20 + 10*std::sin(x*0.01)*std::cos(y*0.013)); (void)big->GetRasterBand(1)->RasterIO(GF_Write, 0, y, n, 1, row.data(), n, 1, GDT_Float32, 0, 0); }
    for (const char* pl : {"read ! slope ! write", "read ! slope ! reclassify -m \"[0,5)=1; DEFAULT=3\" ! write", "read ! hillshade ! color-map --color-map /vsimem/ramp.txt ! write"}) {
        VSILFILE* f = VSIFOpenL("/vsimem/ramp.txt", "wb"); const char* ramp = "0 0 0 0\n255 255 255 255\n"; VSIFWriteL(ramp, 1, strlen(ramp), f); VSIFCloseL(f);
        auto a = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster","pipeline"});
        bind(*a, "input", big); (*a)["output-format"] = "MEM"; a->GetArg("output")->SetDatasetName(""); (*a)["pipeline"] = pl;
        Seq s; ErrorCollector ec; bool ok = a->Run(Seq::fn, &s); a->Finalize();
        printf("[%s] ok=%d ", pl, ok); report("", s); ec.print("p");
    }
    // calc with builtin dialect
    GDALClose(GetGDALDriverManager()->GetDriverByName("GTiff")->CreateCopy("/vsimem/k/dem.tif", big, false, nullptr, nullptr, nullptr));
    for (auto [dialect, expr] : std::vector<std::pair<const char*, const char*>>{{"builtin", "sum"}, {"builtin", "mean"}, {"muparser", "A*2"}}) {
        auto a = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster","calc"});
        std::vector<GDALArgDatasetValue> v; v.emplace_back(std::string("A=/vsimem/k/dem.tif")); v.emplace_back(std::string("B=/vsimem/k/dem.tif"));
        (*a)["input"].Set(std::move(v)); (*a)["calc"] = std::vector<std::string>{expr}; (*a)["dialect"] = dialect;
        (*a)["output-format"] = "MEM"; a->GetArg("output")->SetDatasetName("");
        ErrorCollector ec; bool ok = a->Run();
        GDALDataset* o = ok ? a->GetArg("output")->Get<GDALArgDatasetValue>().GetDatasetIncreaseRefCount() : nullptr; a->Finalize();
        printf("calc dialect=%s expr=%s ok=%d\n", dialect, expr, ok); rasterSummary(o, "calc"); if (o) o->Release(); ec.print("calc");
    }
    printf("dialect choices:"); { auto a = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster","calc"}); for (auto& c : (*a)["dialect"].GetChoices()) printf(" %s", c.c_str()); printf(" default=%s\n", (*a)["dialect"].GetDefault<std::string>().c_str()); }
    // Single algorithms: sequence shape
    for (auto path : std::vector<std::vector<std::string>>{{"raster","slope"},{"raster","hillshade"},{"raster","viewshed"}}) {
        auto a = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(path);
        bind(*a, "input", big); (*a)["output-format"] = "MEM"; a->GetArg("output")->SetDatasetName("");
        if (path[1]=="viewshed") (*a)["position"] = std::vector<double>{1000,1000};
        Seq s; ErrorCollector ec; bool ok = a->Run(Seq::fn, &s); a->Finalize();
        printf("[%s] ok=%d ", path[1].c_str(), ok); report("", s);
    }
    GDALClose(big);
}
