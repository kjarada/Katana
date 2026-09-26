#include "kit.hpp"
#include <memory>
int main()
{
    setupGdal();
    GDALDataset* dem = terrainInMem();
    auto a = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster", "slope"});
    { ErrorCollector ec; bool s = (*a)["input"].Set(dem); printf("C++ Set(GDALDataset*) on dataset_list -> %d refs=%d\n", s, dem->GetRefCount()); ec.print("set"); }
    auto b = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster", "slope"});
    std::vector<GDALArgDatasetValue> v; v.emplace_back(dem);
    printf("vector<GDALArgDatasetValue> Set -> %d refs=%d\n", (*b)["input"].Set(std::move(v)), dem->GetRefCount());
    b.reset();
    printf("after algorithm destroyed refs=%d\n", dem->GetRefCount());
    GDALClose(dem);
}
