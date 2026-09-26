#include "kit.hpp"
#include <memory>
static bool bind(GDALAlgorithm& alg, const char* name, GDALDataset* ds)
{ std::vector<GDALArgDatasetValue> v; v.emplace_back(ds); return alg.GetArg(name)->Set(std::move(v)); }
int main(int argc, char**)
{
    setupGdal();
    GDALDataset* dem = terrainInMem();
    GDALClose(GetGDALDriverManager()->GetDriverByName("GTiff")->CreateCopy("/vsimem/exists.tif", dem, false, nullptr, nullptr, nullptr));
    auto alg = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster", "hillshade"});
    bind(*alg, "input", dem);
    alg->GetArg("output")->SetDatasetName("/vsimem/exists.tif");
    { ErrorCollector ec; printf("alg1 run=%d\n", alg->Run()); ec.print("1"); }
    if (argc > 1) { alg.reset(); printf("alg1 destroyed\n"); }
    auto alg2 = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster", "hillshade"});
    bind(*alg2, "input", dem);
    alg2->GetArg("output")->SetDatasetName("/vsimem/exists.tif");
    (*alg2)["overwrite"] = true;
    { ErrorCollector ec; bool r = alg2->Run(); bool f = alg2->Finalize(); printf("alg2 run=%d fin=%d\n", r, f); ec.print("2"); }
    GDALClose(dem);
}
