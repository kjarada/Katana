#include "kit.hpp"
#include <memory>
#include <thread>
#include <mutex>
#include <sstream>
static std::mutex gm; static std::vector<std::string> globalSeen;
static void CPL_STDCALL globalHandler(CPLErr, CPLErrorNum, const char* m) { std::lock_guard l(gm); std::ostringstream os; os << std::this_thread::get_id(); globalSeen.push_back(os.str() + ": " + m); }
int main()
{
    setupGdal();
    CPLSetErrorHandler(globalHandler);
    const int n = 1500;
    GDALDataset* big = GetGDALDriverManager()->GetDriverByName("MEM")->Create("", n, n, 1, GDT_Float32, nullptr);
    double gt[6] = {0, 1, 0, double(n), 0, -1}; big->SetGeoTransform(gt);
    for (const char* which : {"viewshed", "hillshade", "slope"}) {
        globalSeen.clear();
        auto a = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster", which});
        std::vector<GDALArgDatasetValue> v; v.emplace_back(big); (*a)["input"].Set(std::move(v));
        (*a)["output-format"] = "MEM"; a->GetArg("output")->SetDatasetName("");
        if (std::string(which) == "viewshed") (*a)["position"] = std::vector<double>{700, 700};
        struct C { int n = 0; static int CPL_STDCALL fn(double, const char*, void* d) { return ++static_cast<C*>(d)->n < 3; } } c;
        std::ostringstream me; me << std::this_thread::get_id();
        ErrorCollector ec;
        bool ok = a->Run(C::fn, &c);
        printf("%s run=%d caller-thread=%s collected=%zu global=%zu\n", which, ok, me.str().c_str(), ec.items.size(), globalSeen.size());
        ec.print("caller");
        for (auto& s : globalSeen) printf("  [global handler] %s\n", s.c_str());
    }
    GDALClose(big);
}
