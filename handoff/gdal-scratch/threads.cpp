// Thread safety: concurrent algorithm runs, error isolation, thread-local config.
#include "kit.hpp"
#include <barrier>
#include <memory>
#include <mutex>
#include <thread>

static bool bind(GDALAlgorithm& alg, const char* name, GDALDataset* ds)
{
    GDALAlgorithmArg* arg = alg.GetArg(name);
    if (arg->GetType() == GAAT_DATASET) return arg->Set(ds);
    std::vector<GDALArgDatasetValue> v;
    v.emplace_back(ds);
    return arg->Set(std::move(v));
}

static std::unique_ptr<GDALAlgorithm> inst(std::vector<std::string> p)
{
    return GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(p);
}

static std::vector<double> gDem;
static int gW, gH;
static double gGt[6];

static GDALDataset* demCopy()
{
    GDALDataset* ds = GetGDALDriverManager()->GetDriverByName("MEM")->Create("", gW, gH, 1, GDT_Float64, nullptr);
    ds->SetGeoTransform(gGt);
    (void)ds->GetRasterBand(1)->RasterIO(GF_Write, 0, 0, gW, gH, gDem.data(), gW, gH, GDT_Float64, 0, 0);
    return ds;
}

static GDALDataset* pointsLayer()
{
    GDALDataset* pts = GetGDALDriverManager()->GetDriverByName("MEM")->Create("", 0, 0, 0, GDT_Unknown, nullptr);
    OGRLayer* lyr = pts->CreateLayer("p", nullptr, wkbPoint25D, nullptr);
    for (int i = 0; i < 200; ++i) {
        OGRFeature f(lyr->GetLayerDefn());
        double x = (i * 37) % 170, y = (i * 53) % 130;
        f.SetGeometry(std::make_unique<OGRPoint>(x, y, 25 + std::sin(x * 0.05) * 5));
        (void)lyr->CreateFeature(&f);
    }
    return pts;
}

static double rasterSum(GDALDataset* ds)
{
    std::vector<double> v(size_t(ds->GetRasterXSize()) * ds->GetRasterYSize());
    (void)ds->GetRasterBand(1)->RasterIO(GF_Read, 0, 0, ds->GetRasterXSize(), ds->GetRasterYSize(), v.data(),
                                         ds->GetRasterXSize(), ds->GetRasterYSize(), GDT_Float64, 0, 0);
    double s = 0;
    for (double d : v) s += d;
    return s;
}

struct Result { double hs = 0, contourLen = 0, bufArea = 0, vis = 0, grid = 0; };

static GDALDataset* runOut(GDALAlgorithm& alg)
{
    if (!alg.Run()) return nullptr;
    GDALDataset* o = alg.GetArg("output")->Get<GDALArgDatasetValue>().GetDatasetIncreaseRefCount();
    alg.Finalize();
    return o;
}

static Result work(GDALDataset* dem, GDALDataset* pts)
{
    Result r;
    {
        auto a = inst({"raster", "hillshade"});
        bind(*a, "input", dem); (*a)["output-format"] = "MEM"; a->GetArg("output")->SetDatasetName("");
        if (auto* o = runOut(*a)) { r.hs = rasterSum(o); o->Release(); }
    }
    {
        auto a = inst({"raster", "contour"});
        bind(*a, "input", dem); (*a)["output-format"] = "MEM"; a->GetArg("output")->SetDatasetName("");
        (*a)["interval"] = 0.5;
        if (auto* o = runOut(*a)) {
            for (auto& f : *o->GetLayer(0)) r.contourLen += f->GetGeometryRef()->toLineString()->get_Length();
            o->Release();
        }
    }
    {
        auto a = inst({"vector", "buffer"});
        bind(*a, "input", pts); (*a)["output-format"] = "MEM"; a->GetArg("output")->SetDatasetName("");
        (*a)["distance"] = 2.0;
        if (auto* o = runOut(*a)) {
            for (auto& f : *o->GetLayer(0)) r.bufArea += OGR_G_Area(OGRGeometry::ToHandle(f->GetGeometryRef()));
            o->Release();
        }
    }
    {
        auto a = inst({"raster", "viewshed"});
        bind(*a, "input", dem); (*a)["output-format"] = "MEM"; a->GetArg("output")->SetDatasetName("");
        (*a)["position"] = std::vector<double>{80.0, 60.0}; (*a)["height"] = 1.7;
        if (auto* o = runOut(*a)) { r.vis = rasterSum(o); o->Release(); }
    }
    {
        auto a = inst({"vector", "grid", "invdist"});
        bind(*a, "input", pts); (*a)["output-format"] = "MEM"; a->GetArg("output")->SetDatasetName("");
        (*a)["size"] = std::vector<int>{60, 40}; (*a)["extent"] = std::vector<double>{0, 0, 170, 130};
        if (auto* o = runOut(*a)) { r.grid = rasterSum(o); o->Release(); }
    }
    return r;
}

static bool same(const Result& a, const Result& b)
{
    return a.hs == b.hs && std::abs(a.contourLen - b.contourLen) < 1e-6 && std::abs(a.bufArea - b.bufArea) < 1e-6 &&
           a.vis == b.vis && a.grid == b.grid;
}

int main()
{
    setupGdal();
    // Registry singleton first touched from 8 threads at once.
    {
        std::vector<std::thread> ts;
        std::barrier sync(8);
        std::atomic<int> okCount{0};
        for (int i = 0; i < 8; ++i)
            ts.emplace_back([&] {
                sync.arrive_and_wait();
                auto a = inst({"raster", "hillshade"});
                if (a && a->GetArgs().size() > 10) ++okCount;
            });
        for (auto& t : ts) t.join();
        printf("concurrent first registry use: %d/8 ok\n", okCount.load());
    }
    {
        GDALDataset* src = terrainInMem();
        gW = src->GetRasterXSize(); gH = src->GetRasterYSize();
        src->GetGeoTransform(gGt);
        gDem.resize(size_t(gW) * gH);
        (void)src->GetRasterBand(1)->RasterIO(GF_Read, 0, 0, gW, gH, gDem.data(), gW, gH, GDT_Float64, 0, 0);
        GDALClose(src);
    }
    GDALDataset* dem = demCopy();
    GDALDataset* pts = pointsLayer();
    Result base = work(dem, pts);
    printf("baseline: hs=%.0f contourLen=%.3f bufArea=%.3f vis=%.0f grid=%.3f\n", base.hs, base.contourLen, base.bufArea,
           base.vis, base.grid);

    // 1. Own datasets per thread.
    {
        const int T = 8, N = 15;
        std::atomic<int> good{0}, bad{0};
        auto t0 = std::chrono::steady_clock::now();
        std::vector<std::thread> ts;
        for (int t = 0; t < T; ++t)
            ts.emplace_back([&, t] {
                CPLSetThreadLocalConfigOption("GDAL_NUM_THREADS", t % 2 ? "1" : "ALL_CPUS");
                for (int i = 0; i < N; ++i) {
                    GDALDataset* d = demCopy();
                    GDALDataset* p = pointsLayer();
                    Result r = work(d, p);
                    (same(r, base) ? good : bad)++;
                    GDALClose(p);
                    GDALClose(d);
                }
            });
        for (auto& t : ts) t.join();
        printf("own datasets: %d threads x %d: good=%d bad=%d in %.0f ms\n", T, N, good.load(), bad.load(), ms(t0));
    }
    // 2. The SAME input datasets shared by all threads (read-only use).
    {
        const int T = 8, N = 15;
        std::atomic<int> good{0}, bad{0};
        std::vector<std::thread> ts;
        for (int t = 0; t < T; ++t)
            ts.emplace_back([&] {
                for (int i = 0; i < N; ++i) (same(work(dem, pts), base) ? good : bad)++;
            });
        for (auto& t : ts) t.join();
        printf("SHARED MEM datasets: good=%d bad=%d (not guaranteed safe; vector layers have one read cursor)\n", good.load(),
               bad.load());
    }
    // 2b. Shared raster wrapped by GDALGetThreadSafeDataset.
    {
        GDALDataset* safe = GDALDataset::FromHandle(GDALGetThreadSafeDataset(GDALDataset::ToHandle(dem), GDAL_OF_RASTER, nullptr));
        printf("GDALGetThreadSafeDataset(MEM) -> %p (%s)\n", (void*)safe, safe ? "ok" : CPLGetLastErrorMsg());
        if (safe) {
            std::atomic<int> good{0}, bad{0};
            std::vector<std::thread> ts;
            for (int t = 0; t < 8; ++t)
                ts.emplace_back([&] {
                    GDALDataset* p = pointsLayer();
                    for (int i = 0; i < 10; ++i) (same(work(safe, p), base) ? good : bad)++;
                    GDALClose(p);
                });
            for (auto& t : ts) t.join();
            printf("thread-safe wrapper shared raster: good=%d bad=%d\n", good.load(), bad.load());
            safe->Release();
        }
    }
    // 3. Error isolation: thread-local handlers see only their own thread's errors.
    {
        std::atomic<int> leaks{0}, own{0};
        std::vector<std::thread> ts;
        for (int t = 0; t < 8; ++t)
            ts.emplace_back([&, t] {
                for (int i = 0; i < 50; ++i) {
                    ErrorCollector ec;
                    auto a = inst({"vector", "buffer"});
                    GDALDataset* p = pointsLayer();
                    bind(*a, "input", p); (*a)["output-format"] = "MEM"; a->GetArg("output")->SetDatasetName("");
                    if (t % 2) (*a)["distance"] = 1.0;  // odd threads succeed, even threads fail (no distance)
                    bool ok = a->Run();
                    if (t % 2) { if (!ec.items.empty()) ++leaks; }
                    else { if (!ok && ec.items.size() == 1) ++own; else ++leaks; }
                    GDALClose(p);
                }
            });
        for (auto& t : ts) t.join();
        printf("error isolation: own-errors-seen=%d leaks=%d\n", own.load(), leaks.load());
    }
    // 4. Thread-local config visibility.
    {
        CPLSetConfigOption("KATANA_PROBE", "global");
        std::string inThread, other;
        std::thread a([&] {
            CPLSetThreadLocalConfigOption("KATANA_PROBE", "local");
            inThread = CPLGetConfigOption("KATANA_PROBE", "");
            std::thread b([&] { other = CPLGetConfigOption("KATANA_PROBE", ""); });
            b.join();
        });
        a.join();
        printf("thread-local config: same thread='%s', thread spawned from it='%s', main='%s'\n", inThread.c_str(),
               other.c_str(), CPLGetConfigOption("KATANA_PROBE", ""));
    }
    GDALClose(pts);
    GDALClose(dem);
    return 0;
}
