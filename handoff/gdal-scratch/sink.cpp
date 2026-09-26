#include "kit.hpp"
#include <memory>
static bool bind(GDALAlgorithm& alg, const char* name, GDALDataset* ds)
{ auto* arg = alg.GetArg(name); if (arg->GetType()==GAAT_DATASET) return arg->Set(ds); std::vector<GDALArgDatasetValue> v; v.emplace_back(ds); return arg->Set(std::move(v)); }
int main()
{
    setupGdal();
    printf("driver 'Memory' -> %p, 'MEM' -> %p\n", (void*)GetGDALDriverManager()->GetDriverByName("Memory"), (void*)GetGDALDriverManager()->GetDriverByName("MEM"));
    // 100k points into MEM, timed; then buffer.
    for (const char* drv : {"MEM", "GPKG"}) {
        auto t0 = std::chrono::steady_clock::now();
        GDALDataset* pts = GetGDALDriverManager()->GetDriverByName(drv)->Create(std::string(drv) == "GPKG" ? "/vsimem/k/ents.gpkg" : "", 0, 0, 0, GDT_Unknown, nullptr);
        OGRLayer* lyr = pts->CreateLayer("p", nullptr, wkbPoint25D, nullptr);
        OGRFieldDefn fid("katana_id", OFTInteger64); (void)lyr->CreateField(&fid);
        (void)lyr->StartTransaction();
        for (int i = 0; i < 100000; ++i) {
            OGRFeature f(lyr->GetLayerDefn()); f.SetField(0, (GIntBig)i);
            f.SetGeometry(std::make_unique<OGRPoint>(i % 1000, i / 1000, 1.0)); (void)lyr->CreateFeature(&f);
        }
        (void)lyr->CommitTransaction();
        double tb = ms(t0);
        auto a = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"vector", "buffer"});
        bind(*a, "input", pts); (*a)["output-format"] = "MEM"; a->GetArg("output")->SetDatasetName(""); (*a)["distance"] = 0.3; (*a)["quadrant-segments"] = 2;
        t0 = std::chrono::steady_clock::now();
        ErrorCollector ec; bool ok = a->Run();
        GDALDataset* o = ok ? a->GetArg("output")->Get<GDALArgDatasetValue>().GetDatasetIncreaseRefCount() : nullptr; a->Finalize();
        printf("%s: build 100k=%.0f ms, buffer run=%d %.0f ms, out features=%lld\n", drv, tb, ok, ms(t0), o ? (long long)o->GetLayer(0)->GetFeatureCount() : -1LL);
        ec.print(drv);
        if (o) o->Release();
        GDALClose(pts);
    }
    VSIUnlink("/vsimem/k/ents.gpkg");
    // Existing dataset object as the OUTPUT (sink), --update + --output-layer.
    {
        GDALDataset* pts = GetGDALDriverManager()->GetDriverByName("MEM")->Create("", 0, 0, 0, GDT_Unknown, nullptr);
        OGRLayer* lyr = pts->CreateLayer("p", nullptr, wkbPoint, nullptr);
        for (int i = 0; i < 5; ++i) { OGRFeature f(lyr->GetLayerDefn()); f.SetGeometry(std::make_unique<OGRPoint>(i * 10, 0)); (void)lyr->CreateFeature(&f); }
        GDALDataset* sink = GetGDALDriverManager()->GetDriverByName("MEM")->Create("", 0, 0, 0, GDT_Unknown, nullptr);
        for (double d : {1.0, 2.0}) {
            auto a = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(std::vector<std::string>{"vector", "buffer"});
            bind(*a, "input", pts);
            ErrorCollector ec;
            bool s = a->GetArg("output")->Set(sink);
            (*a)["update"] = true;
            (*a)["output-layer"] = std::string("buf") + std::to_string(int(d));
            (*a)["distance"] = d;
            bool ok = a->Run() && a->Finalize();
            printf("sink output set=%d run=%d sink layers=%d\n", s, ok, sink->GetLayerCount());
            ec.print("sink");
        }
        vectorSummary(sink, "sink");
        GDALClose(sink); GDALClose(pts);
    }
}
