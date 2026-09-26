#include "kit.hpp"
#include <memory>
int main()
{
    setupGdal();
    GDALAlgorithmRegistryH reg = GDALGetGlobalAlgorithmRegistry();
    char** names = GDALAlgorithmRegistryGetAlgNames(reg);
    printf("C API top-level names:"); for (char** p = names; p && *p; ++p) printf(" %s", *p); printf("\n"); CSLDestroy(names);
    const char* path[] = {"driver", "gpkg", "validate", nullptr};
    GDALAlgorithmH h = GDALAlgorithmRegistryInstantiateAlgFromPath(reg, path);
    printf("C API instantiate driver/gpkg/validate -> %p\n", (void*)h);
    if (h) GDALAlgorithmRelease(h);
    const char* rootPath[] = {"gdal", nullptr};
    h = GDALAlgorithmRegistryInstantiateAlg(reg, "gdal");
    printf("C API instantiate 'gdal' root -> %p subalgs:", (void*)h);
    if (h) { char** s = GDALAlgorithmGetSubAlgorithmNames(h); for (char** p = s; p && *p; ++p) printf(" %s", *p); CSLDestroy(s); GDALAlgorithmRelease(h); }
    printf("\n");
    (void)rootPath;
    GDALAlgorithmRegistryRelease(reg);

    auto a = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster", "hillshade"});
    auto fmts = a->GetArg("output-format")->GetAutoCompleteChoices("");
    printf("hillshade output-format autocomplete: %zu choices, first:", fmts.size());
    for (size_t i = 0; i < std::min<size_t>(fmts.size(), 15); ++i) printf(" %s", fmts[i].c_str());
    printf("\n");
    auto b = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"vector", "buffer"});
    auto vf = b->GetArg("output-format")->GetAutoCompleteChoices("");
    printf("buffer output-format autocomplete: %zu choices, MEM present=%d GPKG=%d DXF=%d\n", vf.size(),
           std::find(vf.begin(), vf.end(), "MEM") != vf.end(), std::find(vf.begin(), vf.end(), "GPKG") != vf.end(), std::find(vf.begin(), vf.end(), "DXF") != vf.end());
    auto r = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster", "reproject"});
    for (const char* n : {"resampling", "dst-crs"}) {
        auto* arg = r->GetArg(n);
        auto c = arg->GetAutoCompleteChoices("EPSG:283");
        printf("reproject --%s: choices=%zu autocomplete('EPSG:283')=%zu first=%s\n", n, arg->GetChoices().size(), c.size(), c.empty() ? "" : c[0].c_str());
    }
    // output left unset
    auto c = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"raster", "slope"});
    GDALDataset* dem = terrainInMem();
    std::vector<GDALArgDatasetValue> v; v.emplace_back(dem); (*c)["input"].Set(std::move(v));
    (*c)["output-format"] = "MEM";
    { ErrorCollector ec; printf("slope with output unset: run=%d\n", c->Run()); ec.print("unset"); }
    // Container run
    auto g = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"vector", "grid"});
    { ErrorCollector ec; printf("container 'vector grid' Run: %d; args=%zu\n", g->Run(), g->GetArgs().size()); ec.print("container"); }
    GDALClose(dem);
}
