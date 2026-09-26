// Cancel, errors/validation, command-line parsing, pipelines, GDALG, C API,
// info output-string, streamed output.
#include "kit.hpp"
#include <memory>
#include <thread>

using AlgPtr = std::unique_ptr<GDALAlgorithm>;

static AlgPtr inst(std::vector<std::string> path)
{
    return GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(path);
}

static bool bind(GDALAlgorithm& alg, const char* name, GDALDataset* ds)
{
    GDALAlgorithmArg* arg = alg.GetArg(name);
    if (!arg) return false;
    if (arg->GetType() == GAAT_DATASET) return arg->Set(ds);
    std::vector<GDALArgDatasetValue> v;
    v.emplace_back(ds);
    return arg->Set(std::move(v));
}

static GDALDataset* outRef(GDALAlgorithm& alg)
{
    auto* a = alg.GetArg("output");
    return a && a->GetType() == GAAT_DATASET ? a->Get<GDALArgDatasetValue>().GetDatasetIncreaseRefCount() : nullptr;
}

static GDALDataset* bigDem(int n)
{
    GDALDataset* dem = terrainInMem();
    GDALDataset* big = GetGDALDriverManager()->GetDriverByName("MEM")->Create("", n, n, 1, GDT_Float32, nullptr);
    double gt[6] = {0, 0.1, 0, double(n) * 0.1, 0, -0.1};
    big->SetGeoTransform(gt);
    std::vector<float> row(n);
    for (int y = 0; y < n; ++y) {
        for (int x = 0; x < n; ++x) row[x] = float(20 + 10 * std::sin(x * 0.01) * std::cos(y * 0.013));
        (void)big->GetRasterBand(1)->RasterIO(GF_Write, 0, y, n, 1, row.data(), n, 1, GDT_Float32, 0, 0);
    }
    GDALClose(dem);
    return big;
}

int main()
{
    setupGdal();
    GDALDataset* dem = terrainInMem();

    printf("=== A. cancel from progress (return FALSE at 30%%) ===\n");
    {
        auto alg = inst({"raster", "hillshade"});
        bind(*alg, "input", dem);
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        Progress p;
        p.cancelAt = 0.3;
        ErrorCollector ec;
        bool ok = alg->Run(Progress::fn, &p);
        printf("run=%d calls=%d last=%.3f lastErr='%s' errno=%d\n", ok, p.calls, p.last, CPLGetLastErrorMsg(),
               CPLGetLastErrorNo());
        ec.print("cancel");
        GDALDataset* o = outRef(*alg);
        printf("output after cancel=%p\n", (void*)o);
        if (o) o->Release();
        printf("finalize after failed run=%d\n", alg->Finalize());
    }
    printf("=== A2. cancel from another thread (stop flag) on a 6000x6000 slope, latency ===\n");
    {
        GDALDataset* big = bigDem(6000);
        for (const char* which : {"slope", "hillshade", "viewshed", "contour"}) {
            auto alg = inst({"raster", which});
            bind(*alg, "input", big);
            (*alg)["output-format"] = "MEM";
            alg->GetArg("output")->SetDatasetName("");
            if (std::string(which) == "viewshed") (*alg)["position"] = std::vector<double>{300.0, 300.0};
            if (std::string(which) == "contour") (*alg)["interval"] = 0.5;
            std::atomic<bool> stop{false};
            Progress p;
            p.stop = &stop;
            std::chrono::steady_clock::time_point tStop;
            std::thread killer([&] {
                std::this_thread::sleep_for(std::chrono::milliseconds(80));
                tStop = std::chrono::steady_clock::now();
                stop = true;
            });
            ErrorCollector ec;
            auto t0 = std::chrono::steady_clock::now();
            bool ok = alg->Run(Progress::fn, &p);
            double total = ms(t0);
            killer.join();
            printf("%s: run=%d total=%.1f ms after-stop=%.1f ms calls=%d last=%.3f err='%s'\n", which, ok, total, ms(tStop),
                   p.calls, p.last, ec.items.empty() ? "" : ec.items.back().msg.c_str());
        }
        GDALClose(big);
    }

    printf("=== B. errors and validation ===\n");
    {
        auto alg = inst({"raster", "hillshade"});
        ErrorCollector ec;
        bool s1 = (*alg)["variant"].Set("bogus");
        bool s2 = (*alg)["altitude"].Set(100.0);
        bool s3 = (*alg)["band"].Set("notanint");
        bool s4 = (*alg)["zfactor"].Set(2);   // int into real
        GDALAlgorithmArg& missing = (*alg)["no-such-arg"];
        printf("set bogus choice=%d, altitude=100 -> %d, band='x' -> %d, zfactor(int)=%d, missing arg name='%s'\n", s1, s2,
               s3, s4, missing.GetName().c_str());
        ec.print("set");
    }
    {
        auto alg = inst({"vector", "buffer"});
        bind(*alg, "input", dem);  // raster into a vector input
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        ErrorCollector ec;
        bool ok = alg->Run();
        printf("buffer w/o distance + raster input: run=%d\n", ok);
        ec.print("validate");
    }
    {
        auto alg = inst({"raster", "contour"});
        bind(*alg, "input", dem);
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        (*alg)["interval"] = 1.0;
        (*alg)["levels"] = std::vector<std::string>{"30", "31"};
        ErrorCollector ec;
        bool ok = alg->ValidateArguments();
        printf("contour interval+levels: validate=%d\n", ok);
        ec.print("mutex");
    }
    {
        // Output already exists, no --overwrite.
        GDALDataset* t = GetGDALDriverManager()->GetDriverByName("GTiff")->CreateCopy("/vsimem/exists.tif", dem, false, nullptr, nullptr, nullptr);
        GDALClose(t);
        auto alg = inst({"raster", "hillshade"});
        bind(*alg, "input", dem);
        alg->GetArg("output")->SetDatasetName("/vsimem/exists.tif");
        ErrorCollector ec;
        bool ok = alg->Run();
        printf("existing output w/o overwrite: run=%d\n", ok);
        ec.print("exists");
        auto alg2 = inst({"raster", "hillshade"});
        bind(*alg2, "input", dem);
        alg2->GetArg("output")->SetDatasetName("/vsimem/exists.tif");
        (*alg2)["overwrite"] = true;
        ErrorCollector ec2;
        printf("with overwrite: run=%d finalize=%d\n", alg2->Run(), alg2->Finalize());
        ec2.print("overwrite");
        printf("run twice: ");
        ErrorCollector ec3;
        printf("%d\n", alg2->Run());
        ec3.print("twice");
        VSIUnlink("/vsimem/exists.tif");
    }

    printf("=== C. ParseCommandLineArguments on a leaf, dataset by /vsimem name ===\n");
    {
        GDALDataset* t = GetGDALDriverManager()->GetDriverByName("GTiff")->CreateCopy("/vsimem/dem.tif", dem, false, nullptr, nullptr, nullptr);
        GDALClose(t);
        auto alg = inst({"raster", "slope"});
        ErrorCollector ec;
        bool parsed = alg->ParseCommandLineArguments({"--unit=percent", "/vsimem/dem.tif", "--of", "MEM", "slope_out"});
        bool ok = parsed && alg->Run();
        GDALDataset* o = ok ? outRef(*alg) : nullptr;
        alg->Finalize();
        printf("parse=%d run=%d\n", parsed, ok);
        rasterSummary(o, "slope%");
        if (o) o->Release();
        ec.print("parse");
        // top-level path through the root "gdal" container
        auto root = GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate("gdal");
        ErrorCollector ec2;
        bool p2 = root->ParseCommandLineArguments({"raster", "aspect", "/vsimem/dem.tif", "--of", "MEM", "x"});
        GDALAlgorithm& actual = root->GetActualAlgorithm();
        bool r2 = p2 && actual.Run();
        GDALDataset* o2 = r2 ? outRef(actual) : nullptr;
        printf("root parse=%d actual='%s' run=%d\n", p2, actual.GetName().c_str(), r2);
        rasterSummary(o2, "aspect");
        if (o2) o2->Release();
        ec2.print("root");
    }

    printf("=== D. pipelines ===\n");
    {
        // D1: raster pipeline string, input bound to an open MEM dataset.
        auto alg = inst({"raster", "pipeline"});
        ErrorCollector ec;
        bind(*alg, "input", dem);
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        (*alg)["pipeline"] = "read ! hillshade --zfactor 2 ! write";
        Progress p;
        bool ok = alg->Run(Progress::fn, &p);
        GDALDataset* o = ok ? outRef(*alg) : nullptr;
        alg->Finalize();
        printf("D1 raster pipeline (bound input, 'read ! hillshade ! write'): run=%d calls=%d\n", ok, p.calls);
        rasterSummary(o, "D1");
        if (o) o->Release();
        ec.print("D1");
    }
    {
        // D1b: steps only, no read/write.
        auto alg = inst({"raster", "pipeline"});
        ErrorCollector ec;
        bind(*alg, "input", dem);
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        (*alg)["pipeline"] = "reproject --dst-crs EPSG:4326 ! hillshade";
        bool ok = alg->Run();
        GDALDataset* o = ok ? outRef(*alg) : nullptr;
        alg->Finalize();
        printf("D1b raster pipeline steps only: run=%d\n", ok);
        rasterSummary(o, "D1b");
        if (o) o->Release();
        ec.print("D1b");
    }
    {
        // D2: parse a whole pipeline command line with names.
        auto alg = inst({"raster", "pipeline"});
        ErrorCollector ec;
        bool parsed = alg->ParseCommandLineArguments(
            {"read", "/vsimem/dem.tif", "!", "slope", "!", "reclassify", "-m", "0:5=1; 5:15=2; DEFAULT=3", "!", "write", "--of", "MEM", "x"});
        Progress p;
        bool ok = parsed && alg->Run(Progress::fn, &p);
        GDALDataset* o = ok ? outRef(*alg) : nullptr;
        alg->Finalize();
        printf("D2 parsed pipeline: parse=%d run=%d calls=%d\n", parsed, ok, p.calls);
        rasterSummary(o, "D2");
        if (o) o->Release();
        ec.print("D2");
    }
    {
        // D3: mixed raster->vector pipeline through the generic "gdal pipeline".
        auto alg = inst({"pipeline"});
        ErrorCollector ec;
        bind(*alg, "input", dem);
        (*alg)["output-format"] = "MEM";
        alg->GetArg("output")->SetDatasetName("");
        (*alg)["pipeline"] = "read ! contour --interval 2 ! buffer 0.5 ! write";
        bool ok = alg->Run();
        GDALDataset* o = ok ? outRef(*alg) : nullptr;
        alg->Finalize();
        printf("D3 mixed pipeline raster->contour->buffer: run=%d\n", ok);
        vectorSummary(o, "D3");
        if (o) o->Release();
        ec.print("D3");
    }
    {
        // D4: GDALG: serialise a pipeline as a streamable .gdalg.json.
        auto alg = inst({"raster", "pipeline"});
        ErrorCollector ec;
        bool parsed = alg->ParseCommandLineArguments(
            {"read", "/vsimem/dem.tif", "!", "hillshade", "!", "write", "/vsimem/hs.gdalg.json"});
        bool ok = parsed && alg->Run() && alg->Finalize();
        printf("D4 GDALG: parse=%d run=%d\n", parsed, ok);
        ec.print("D4");
        if (ok) {
            vsi_l_offset len = 0;
            GByte* buf = VSIGetMemFileBuffer("/vsimem/hs.gdalg.json", &len, false);
            printf("  gdalg.json (%llu bytes): %.*s\n", (unsigned long long)len, int(len), buf ? (const char*)buf : "");
            GDALDataset* g = GDALDataset::Open("/vsimem/hs.gdalg.json", GDAL_OF_RASTER);
            rasterSummary(g, "opened gdalg");
            if (g) GDALClose(g);
        }
    }

    printf("=== E. info algorithms -> output-string ===\n");
    {
        auto alg = inst({"raster", "info"});
        bind(*alg, "input", dem);
        (*alg)["stats"] = true;
        bool ok = alg->Run();
        const std::string& s = (*alg)["output-string"].Get<std::string>();
        printf("raster info run=%d output-string %zu chars, starts: %.120s\n", ok, s.size(), s.c_str());
    }

    printf("=== F. C API equivalent ===\n");
    {
        GDALAlgorithmRegistryH reg = GDALGetGlobalAlgorithmRegistry();
        const char* path[] = {"raster", "hillshade", nullptr};
        GDALAlgorithmH h = GDALAlgorithmRegistryInstantiateAlgFromPath(reg, path);
        GDALAlgorithmArgH in = GDALAlgorithmGetArg(h, "input");
        GDALDatasetH dsh = GDALDataset::ToHandle(dem);
        bool b1 = GDALAlgorithmArgSetDatasets(in, 1, &dsh);
        GDALAlgorithmArgRelease(in);
        GDALAlgorithmArgH of = GDALAlgorithmGetArg(h, "output-format");
        GDALAlgorithmArgSetAsString(of, "MEM");
        GDALAlgorithmArgRelease(of);
        GDALAlgorithmArgH out = GDALAlgorithmGetArg(h, "output");
        GDALArgDatasetValueH v = GDALArgDatasetValueCreate();
        GDALArgDatasetValueSetName(v, "");
        GDALAlgorithmArgSetAsDatasetValue(out, v);
        GDALArgDatasetValueRelease(v);
        Progress p;
        bool ok = GDALAlgorithmRun(h, Progress::fn, &p);
        GDALArgDatasetValueH ov = GDALAlgorithmArgGetAsDatasetValue(out);
        GDALDatasetH od = GDALArgDatasetValueGetDatasetIncreaseRefCount(ov);
        GDALArgDatasetValueRelease(ov);
        GDALAlgorithmArgRelease(out);
        bool fin = GDALAlgorithmFinalize(h);
        printf("C API: setds=%d run=%d finalize=%d calls=%d\n", b1, ok, fin, p.calls);
        rasterSummary(GDALDataset::FromHandle(od), "C out");
        GDALReleaseDataset(od);
        GDALAlgorithmRelease(h);
        GDALAlgorithmRegistryRelease(reg);
    }

    printf("=== G. API usage JSON vs CLI --json-usage, hidden choices, streamed output ===\n");
    {
        auto alg = inst({"raster", "hillshade"});
        std::string j = alg->GetUsageAsJSON();
        printf("GetUsageAsJSON length=%zu\n", j.size());
        FILE* f = fopen("hillshade_api.json", "w");
        fputs(j.c_str(), f);
        fclose(f);
        auto* of = alg->GetArg("output-format");
        printf("output-format choices=%zu hidden choices:", of->GetChoices().size());
        for (auto& c : of->GetHiddenChoices()) printf(" %s", c.c_str());
        printf("\n");
        auto* vf = inst({"vector", "buffer"})->GetArg("output-format");
        printf("buffer output-format hidden choices:");
        for (auto& c : vf->GetHiddenChoices()) printf(" %s", c.c_str());
        printf("\n");
    }
    {
        // "stream" output: no materialised copy, a lazy dataset.
        auto alg = inst({"raster", "hillshade"});
        bind(*alg, "input", dem);
        ErrorCollector ec;
        (*alg)["output-format"] = "stream";
        alg->GetArg("output")->SetDatasetName("");
        auto t0 = std::chrono::steady_clock::now();
        bool ok = alg->Run();
        GDALDataset* o = ok ? outRef(*alg) : nullptr;
        printf("stream: run=%d %.2f ms driver=%s\n", ok, ms(t0), o && o->GetDriver() ? o->GetDriver()->GetDescription() : "-");
        alg->Finalize();
        alg.reset();
        rasterSummary(o, "stream (read after alg destroyed)");
        if (o) o->Release();
        ec.print("stream");
    }
    GDALClose(dem);
    VSIUnlink("/vsimem/dem.tif");
    return 0;
}
