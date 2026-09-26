// Concrete in-process runs of GDAL 3.13 algorithms with datasets bound as
// objects (no temp files): hillshade, contour, buffer, viewshed, grid.
#include "kit.hpp"
#include <memory>

using AlgPtr = std::unique_ptr<GDALAlgorithm>;

static AlgPtr instantiate(std::vector<std::string> path)
{
    auto alg = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(path);
    if (!alg) printf("instantiate failed: %s\n", CPLGetLastErrorMsg());
    return alg;
}

// Bind an already-open dataset to a dataset or dataset-list argument.
static bool bindDataset(GDALAlgorithm& alg, const char* name, GDALDataset* ds)
{
    GDALAlgorithmArg* arg = alg.GetArg(name);
    if (!arg) { printf("no arg %s\n", name); return false; }
    if (arg->GetType() == GAAT_DATASET) return arg->Set(ds);
    if (arg->GetType() == GAAT_DATASET_LIST) {
        std::vector<GDALArgDatasetValue> v;
        v.emplace_back(ds);
        return arg->Set(std::move(v));
    }
    return false;
}

// Take the output dataset out of the algorithm with our own reference.
static GDALDataset* takeOutput(GDALAlgorithm& alg, const char* name = "output")
{
    GDALAlgorithmArg* arg = alg.GetArg(name);
    if (!arg || arg->GetType() != GAAT_DATASET) return nullptr;
    return arg->Get<GDALArgDatasetValue>().GetDatasetIncreaseRefCount();
}

// Run, then take our own reference to the output BEFORE Finalize() (which
// closes the algorithm's reference), then Finalize.
static bool run(GDALAlgorithm& alg, const char* tag, Progress& p, GDALDataset** out = nullptr)
{
    ErrorCollector ec;
    auto t0 = std::chrono::steady_clock::now();
    bool ok = alg.Run(Progress::fn, &p);
    GDALDataset* before = ok ? takeOutput(alg) : nullptr;
    int rcBefore = before ? before->GetRefCount() : 0;
    bool fin = ok ? alg.Finalize() : false;
    GDALDataset* after = takeOutput(alg);
    printf("%s: run=%d finalize=%d %.1f ms progress calls=%d last=%.3f out-before-finalize=%p (refs %d) out-after=%p\n", tag,
           ok, fin, ms(t0), p.calls, p.last, (void*)before, rcBefore, (void*)after);
    if (after) after->Release();
    if (out) *out = before; else if (before) before->Release();
    ec.print(tag);
    return ok && fin;
}

int main()
{
    setupGdal();
    GDALDataset* dem = terrainInMem();
    rasterSummary(dem, "dem");

    // 1. Hillshade: MEM in, MEM out.
    {
        auto alg = instantiate({"raster", "hillshade"});
        bindDataset(*alg, "input", dem);
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        (*alg)["zfactor"] = 2.0;
        (*alg)["variant"] = "combined";
        Progress p;
        GDALDataset* out = nullptr;
            if (run(*alg, "hillshade", p, &out)) {
            alg.reset(); // the output must outlive the algorithm
            rasterSummary(out, "hillshade out");
            if (out) out->Release();
        }
    }
    // 1b. Hillshade to /vsimem/ GeoTIFF.
    {
        auto alg = instantiate({"raster", "hillshade"});
        bindDataset(*alg, "input", dem);
        alg->GetArg("output")->SetDatasetName("/vsimem/katana/hs.tif");
        Progress p;
        GDALDataset* out = nullptr;
            if (run(*alg, "hillshade->vsimem gtiff", p, &out)) {
            rasterSummary(out, "hs.tif");
            if (out) out->Release();
            alg.reset();
            VSIStatBufL st;
            printf("  /vsimem/katana/hs.tif exists=%d size=%lld\n", VSIStatL("/vsimem/katana/hs.tif", &st) == 0,
                   (long long)st.st_size);
            VSIUnlink("/vsimem/katana/hs.tif");
        }
    }
    // 2. Contour: MEM raster in, MEM vector out.
    {
        auto alg = instantiate({"raster", "contour"});
        bindDataset(*alg, "input", dem);
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        (*alg)["interval"] = 1.0;
        (*alg)["elevation-name"] = "elev";
        (*alg)["3d"] = true;
        Progress p;
        GDALDataset* out = nullptr;
            if (run(*alg, "contour", p, &out)) {
            alg.reset();
            vectorSummary(out, "contour out");
            if (out) {
                auto* lyr = out->GetLayer(0);
                lyr->ResetReading();
                int n = 0;
                for (auto& f : *lyr) {
                    if (n++ < 3) {
                        auto* g = f->GetGeometryRef();
                        printf("    fid=%lld elev=%.2f geom=%s points=%d\n", (long long)f->GetFID(),
                               f->GetFieldAsDouble("elev"), g->getGeometryName(),
                               wkbFlatten(g->getGeometryType()) == wkbLineString ? g->toLineString()->getNumPoints() : -1);
                    }
                }
                out->Release();
            }
        }
    }
    // 3. Buffer: an in-memory vector layer built from "entities".
    GDALDataset* pts = nullptr;
    {
        GDALDriver* mem = GetGDALDriverManager()->GetDriverByName("MEM");
        pts = mem->Create("", 0, 0, 0, GDT_Unknown, nullptr);
        OGRSpatialReference srs;
        srs.importFromEPSG(32630);
        srs.SetAxisMappingStrategy(OAMS_TRADITIONAL_GIS_ORDER);
        OGRLayer* lyr = pts->CreateLayer("entities", &srs, wkbPoint25D, nullptr);
        OGRFieldDefn fid("katana_id", OFTInteger64);
        lyr->CreateField(&fid);
        OGRFieldDefn fz("z", OFTReal);
        lyr->CreateField(&fz);
        // A handful of spot heights.
        const double xyz[][3] = {{10, 10, 26.5}, {40, 20, 28.0}, {80, 30, 30.2}, {120, 60, 31.0}, {60, 100, 29.5},
                                 {20, 80, 27.2}, {150, 110, 33.0}, {100, 5, 29.0}};
        long long id = 100;
        for (auto& p : xyz) {
            OGRFeature f(lyr->GetLayerDefn());
            f.SetField("katana_id", id++);
            f.SetField("z", p[2]);
            f.SetGeometry(std::make_unique<OGRPoint>(p[0], p[1], p[2]));
            lyr->CreateFeature(&f);
        }
        vectorSummary(pts, "points in");

        auto alg = instantiate({"vector", "buffer"});
        bindDataset(*alg, "input", pts);
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        (*alg)["distance"] = 5.0;
        (*alg)["quadrant-segments"] = 4;
        Progress p;
        GDALDataset* out = nullptr;
            if (run(*alg, "buffer", p, &out)) {
            alg.reset();
            vectorSummary(out, "buffer out");
            if (out) {
                auto* lyr = out->GetLayer(0);
                auto f = std::unique_ptr<OGRFeature>(lyr->GetNextFeature());
                printf("    first: katana_id=%lld area=%.3f (pi*25=%.3f)\n", (long long)f->GetFieldAsInteger64("katana_id"),
                       f->GetGeometryRef()->toPolygon()->get_Area(), 3.14159265 * 25);
                out->Release();
            }
        }
    }
    // 4. Viewshed at a point.
    {
        auto alg = instantiate({"raster", "viewshed"});
        bindDataset(*alg, "input", dem);
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        (*alg)["position"] = std::vector<double>{80.0, 60.0};
        (*alg)["height"] = 1.7;
        (*alg)["target-height"] = 0.0;
        Progress p;
        GDALDataset* out = nullptr;
            if (run(*alg, "viewshed", p, &out)) {
            alg.reset();
            rasterSummary(out, "viewshed out");
            if (out) {
                auto* b = out->GetRasterBand(1);
                std::vector<uint8_t> v(size_t(out->GetRasterXSize()) * out->GetRasterYSize());
                (void)b->RasterIO(GF_Read, 0, 0, out->GetRasterXSize(), out->GetRasterYSize(), v.data(), out->GetRasterXSize(),
                                  out->GetRasterYSize(), GDT_UInt8, 0, 0);
                size_t vis = 0;
                for (auto c : v) vis += c == 255;
                printf("    visible cells=%zu of %zu\n", vis, v.size());
                out->Release();
            }
        }
    }
    // 5. Grid from points (invdist): vector MEM in, MEM raster out.
    {
        auto alg = instantiate({"vector", "grid", "invdist"});
        bindDataset(*alg, "input", pts);
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        (*alg)["zfield"] = "z";
        (*alg)["size"] = std::vector<int>{80, 60};
        (*alg)["extent"] = std::vector<double>{0, 0, 160, 120};
        (*alg)["power"] = 2.0;
        Progress p;
        GDALDataset* out = nullptr;
            if (run(*alg, "grid invdist", p, &out)) {
            alg.reset();
            rasterSummary(out, "grid out");
            if (out) out->Release();
        }
    }
    // 5b. Grid using the geometry Z (no zfield).
    {
        auto alg = instantiate({"vector", "grid", "linear"});
        bindDataset(*alg, "input", pts);
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        (*alg)["resolution"] = std::vector<double>{2.0, 2.0};
        (*alg)["extent"] = std::vector<double>{0, 0, 160, 120};
        Progress p;
        GDALDataset* out = nullptr;
            if (run(*alg, "grid linear", p, &out)) {
            alg.reset();
            rasterSummary(out, "grid linear out");
            if (out) out->Release();
        }
    }
    printf("dem refcount before close=%d pts refcount=%d\n", dem->GetRefCount(), pts->GetRefCount());
    GDALClose(pts);
    GDALClose(dem);
    return 0;
}
