// Shared helpers for the scratch experiments.
#pragma once
#include <gdal_priv.h>
#include <gdalalgorithm.h>
#include <ogrsf_frmts.h>
#include <cpl_error.h>
#include <cpl_conv.h>
#include <cpl_string.h>
#include <cpl_vsi.h>

#include <atomic>
#include <chrono>
#include <cstdio>
#include <string>
#include <vector>

// Collects every CPLError raised on this thread while alive (thread-local
// handler stack), instead of only the last one.
struct ErrorCollector {
    struct Item { CPLErr cls; CPLErrorNum num; std::string msg; };
    std::vector<Item> items;
    static void CPL_STDCALL handler(CPLErr cls, CPLErrorNum num, const char* msg)
    {
        auto* self = static_cast<ErrorCollector*>(CPLGetErrorHandlerUserData());
        self->items.push_back({cls, num, msg ? msg : ""});
    }
    ErrorCollector() { CPLPushErrorHandlerEx(&ErrorCollector::handler, this); }
    ~ErrorCollector() { CPLPopErrorHandler(); }
    void print(const char* tag) const
    {
        for (auto& e : items)
            printf("  [%s] cls=%d num=%d %s\n", tag, int(e.cls), int(e.num), e.msg.c_str());
    }
};

struct Progress {
    int calls = 0;
    double last = -1;
    double cancelAt = 2.0;   // > 1 means never cancel
    std::atomic<bool>* stop = nullptr;
    std::string lastMsg;
    static int CPL_STDCALL fn(double complete, const char* msg, void* data)
    {
        auto* p = static_cast<Progress*>(data);
        ++p->calls;
        p->last = complete;
        if (msg && *msg) p->lastMsg = msg;
        if (p->stop && p->stop->load()) return FALSE;
        return complete >= p->cancelAt ? FALSE : TRUE;
    }
};

inline void setupGdal()
{
    if (!CPLGetConfigOption("GDAL_DATA", nullptr))
        CPLSetConfigOption("GDAL_DATA", "C:/msys64/ucrt64/share/gdal");
    GDALAllRegister();
}

// Load the sample ASCII grid into a MEM raster (as Katana would from its own
// terrain grid): the algorithm then never sees a file name.
inline GDALDataset* terrainInMem(const char* path = "C:/GitHubProjects/Katana/samples/gis/terrain.asc")
{
    GDALDataset* src = GDALDataset::Open(path, GDAL_OF_RASTER | GDAL_OF_READONLY);
    if (!src) { printf("open %s failed: %s\n", path, CPLGetLastErrorMsg()); return nullptr; }
    GDALDriver* mem = GetGDALDriverManager()->GetDriverByName("MEM");
    GDALDataset* ds = mem->Create("", src->GetRasterXSize(), src->GetRasterYSize(), 1, GDT_Float64, nullptr);
    double gt[6];
    src->GetGeoTransform(gt);
    ds->SetGeoTransform(gt);
    if (auto* srs = src->GetSpatialRef()) ds->SetSpatialRef(srs);
    std::vector<double> buf(size_t(src->GetRasterXSize()) * src->GetRasterYSize());
    (void)src->GetRasterBand(1)->RasterIO(GF_Read, 0, 0, src->GetRasterXSize(), src->GetRasterYSize(), buf.data(),
                                          src->GetRasterXSize(), src->GetRasterYSize(), GDT_Float64, 0, 0);
    (void)ds->GetRasterBand(1)->RasterIO(GF_Write, 0, 0, src->GetRasterXSize(), src->GetRasterYSize(), buf.data(),
                                         src->GetRasterXSize(), src->GetRasterYSize(), GDT_Float64, 0, 0);
    int ok = 0;
    double nd = src->GetRasterBand(1)->GetNoDataValue(&ok);
    if (ok) ds->GetRasterBand(1)->SetNoDataValue(nd);
    GDALClose(src);
    return ds;
}

inline double ms(std::chrono::steady_clock::time_point t0)
{
    return std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
}

inline void rasterSummary(GDALDataset* ds, const char* tag)
{
    if (!ds) { printf("  %s: null dataset\n", tag); return; }
    auto* b = ds->GetRasterBand(1);
    double mn, mx, mean, sd;
    CPLPushErrorHandler(CPLQuietErrorHandler);
    CPLErr e = b ? b->ComputeStatistics(false, &mn, &mx, &mean, &sd, nullptr, nullptr) : CE_Failure;
    CPLPopErrorHandler();
    printf("  %s: driver=%s desc='%s' %dx%dx%d type=%s", tag, ds->GetDriver() ? ds->GetDriver()->GetDescription() : "?",
           ds->GetDescription(), ds->GetRasterXSize(), ds->GetRasterYSize(), ds->GetRasterCount(),
           b ? GDALGetDataTypeName(b->GetRasterDataType()) : "-");
    if (e == CE_None) printf(" min=%.3f max=%.3f mean=%.3f", mn, mx, mean);
    printf("\n");
}

inline void vectorSummary(GDALDataset* ds, const char* tag)
{
    if (!ds) { printf("  %s: null dataset\n", tag); return; }
    printf("  %s: driver=%s layers=%d\n", tag, ds->GetDriver() ? ds->GetDriver()->GetDescription() : "?", ds->GetLayerCount());
    for (auto* lyr : ds->GetLayers()) {
        OGREnvelope env;
        (void)lyr->GetExtent(&env, true);
        printf("    layer '%s' geom=%s features=%lld fields=%d extent=[%.2f %.2f %.2f %.2f]\n", lyr->GetName(),
               OGRGeometryTypeToName(lyr->GetGeomType()), (long long)lyr->GetFeatureCount(),
               lyr->GetLayerDefn()->GetFieldCount(), env.MinX, env.MinY, env.MaxX, env.MaxY);
    }
}
