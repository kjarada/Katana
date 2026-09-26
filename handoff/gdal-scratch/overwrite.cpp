// Why did --overwrite to an existing /vsimem/ GeoTIFF fail?
#include "kit.hpp"
#include <memory>

static bool bind(GDALAlgorithm& alg, const char* name, GDALDataset* ds)
{
    std::vector<GDALArgDatasetValue> v;
    v.emplace_back(ds);
    return alg.GetArg(name)->Set(std::move(v));
}

static void attempt(GDALDataset* dem, const std::string& path, bool precreate, bool viaCLI)
{
    if (precreate) {
        GDALDataset* t = GetGDALDriverManager()->GetDriverByName("GTiff")->CreateCopy(path.c_str(), dem, false, nullptr, nullptr, nullptr);
        GDALClose(t);
    }
    auto alg = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster", "hillshade"});
    ErrorCollector ec;
    bool ok;
    if (viaCLI) {
        GDALClose(GetGDALDriverManager()->GetDriverByName("GTiff")->CreateCopy("/vsimem/in.tif", dem, false, nullptr, nullptr, nullptr));
        ok = alg->ParseCommandLineArguments({"/vsimem/in.tif", path, "--overwrite"}) && alg->Run() && alg->Finalize();
    } else {
        bind(*alg, "input", dem);
        alg->GetArg("output")->SetDatasetName(path);
        (*alg)["overwrite"] = true;
        ok = alg->Run() && alg->Finalize();
    }
    printf("%-45s precreate=%d cli=%d -> %d\n", path.c_str(), precreate, viaCLI, ok);
    ec.print("ow");
}

int main()
{
    setupGdal();
    GDALDataset* dem = terrainInMem();
    attempt(dem, "/vsimem/a.tif", true, false);
    attempt(dem, "/vsimem/b.tif", false, false);
    attempt(dem, "/vsimem/c.tif", true, true);
    attempt(dem, "C:/GitHubProjects/Katana-wt/gdal-scratch/ow.tif", true, false);
    attempt(dem, "C:/GitHubProjects/Katana-wt/gdal-scratch/ow.tif", true, true);
    GDALClose(dem);
    return 0;
}
