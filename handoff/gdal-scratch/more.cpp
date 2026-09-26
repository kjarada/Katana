// calc via /vsimem names, pipelines with bound inputs + cancel, mixing object
// binding with command-line parsing, Serialize, usage text, run twice.
#include "kit.hpp"
#include <memory>

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
static GDALDataset* outRef(GDALAlgorithm& alg)
{
    return alg.GetArg("output")->Get<GDALArgDatasetValue>().GetDatasetIncreaseRefCount();
}

int main()
{
    setupGdal();
    GDALDataset* dem = terrainInMem();

    printf("=== calc: inputs by /vsimem name only ===\n");
    {
        GDALClose(GetGDALDriverManager()->GetDriverByName("GTiff")->CreateCopy("/vsimem/k/dem.tif", dem, false, nullptr, nullptr, nullptr));
        auto a = inst({"raster", "calc"});
        ErrorCollector ec;
        (*a)["input"].SetDatasetName("A=/vsimem/k/dem.tif");  // dataset_list: try name form
        printf("SetDatasetName on dataset_list: explicitly set=%d\n", (*a)["input"].IsExplicitlySet());
        (*a)["calc"] = std::vector<std::string>{"A * 2 - 10"};
        (*a)["output-format"] = "MEM";
        a->GetArg("output")->SetDatasetName("");
        bool ok = a->Run();
        GDALDataset* o = ok ? outRef(*a) : nullptr;
        a->Finalize();
        printf("calc run=%d\n", ok);
        rasterSummary(o, "calc");
        if (o) o->Release();
        ec.print("calc");
        // dataset_list with explicit vector of names
        auto b = inst({"raster", "calc"});
        ErrorCollector ec2;
        std::vector<GDALArgDatasetValue> v;
        v.emplace_back(std::string("A=/vsimem/k/dem.tif"));
        v.emplace_back(std::string("B=/vsimem/k/dem.tif"));
        (*b)["input"].Set(std::move(v));
        (*b)["calc"] = std::vector<std::string>{"A - B"};
        (*b)["output-format"] = "MEM";
        b->GetArg("output")->SetDatasetName("");
        ok = b->Run();
        o = ok ? outRef(*b) : nullptr;
        b->Finalize();
        printf("calc A-B run=%d\n", ok);
        rasterSummary(o, "calc A-B");
        if (o) o->Release();
        ec2.print("calc2");
    }

    printf("=== bind objects, then parse the rest of the command line ===\n");
    {
        auto a = inst({"raster", "hillshade"});
        bind(*a, "input", dem);
        a->GetArg("output")->SetDatasetName("");
        ErrorCollector ec;
        bool parsed = a->ParseCommandLineArguments({"--zfactor=2", "--variant", "multidirectional", "--of", "MEM"});
        printf("parse after binding: %d\n", parsed);
        ec.print("mixed");
        bool ok = parsed && a->Run();
        GDALDataset* o = ok ? outRef(*a) : nullptr;
        a->Finalize();
        rasterSummary(o, "mixed");
        if (o) o->Release();
        // Serialize every explicitly set argument back to a CLI fragment.
        std::string cmd = "gdal raster hillshade";
        for (auto& arg : a->GetArgs()) {
            if (!arg->IsExplicitlySet()) continue;
            std::string s;
            bool okS = arg->Serialize(s);
            cmd += " " + (okS ? s : "<" + arg->GetName() + ":unserialisable>");
        }
        printf("serialised: %s\n", cmd.c_str());
        ErrorCollector ec3;
        printf("run twice -> %d\n", a->Run());
        ec3.print("twice");
    }

    printf("=== raster pipeline with a bound input on 3000x3000: progress granularity + cancel ===\n");
    {
        const int n = 3000;
        GDALDataset* big = GetGDALDriverManager()->GetDriverByName("MEM")->Create("", n, n, 1, GDT_Float32, nullptr);
        double gt[6] = {0, 1, 0, double(n), 0, -1};
        big->SetGeoTransform(gt);
        std::vector<float> row(n);
        for (int y = 0; y < n; ++y) {
            for (int x = 0; x < n; ++x) row[x] = float(20 + 10 * std::sin(x * 0.01) * std::cos(y * 0.013));
            (void)big->GetRasterBand(1)->RasterIO(GF_Write, 0, y, n, 1, row.data(), n, 1, GDT_Float32, 0, 0);
        }
        for (double cancelAt : {2.0, 0.5}) {
            auto a = inst({"raster", "pipeline"});
            bind(*a, "input", big);
            (*a)["output-format"] = "MEM";
            a->GetArg("output")->SetDatasetName("");
            (*a)["pipeline"] = "read ! slope ! reclassify -m \"[0,5)=1; [5,15)=2; DEFAULT=3\" ! write";
            Progress p;
            p.cancelAt = cancelAt;
            ErrorCollector ec;
            auto t0 = std::chrono::steady_clock::now();
            bool ok = a->Run(Progress::fn, &p);
            GDALDataset* o = ok ? outRef(*a) : nullptr;
            a->Finalize();
            printf("cancelAt=%.1f run=%d calls=%d last=%.3f %.0f ms\n", cancelAt, ok, p.calls, p.last, ms(t0));
            rasterSummary(o, "pipeline out");
            if (o) o->Release();
            ec.print("pipe");
        }
        GDALClose(big);
    }

    printf("=== vector pipeline with bound input ===\n");
    {
        GDALDataset* pts = GetGDALDriverManager()->GetDriverByName("MEM")->Create("", 0, 0, 0, GDT_Unknown, nullptr);
        OGRLayer* lyr = pts->CreateLayer("spots", nullptr, wkbPoint25D, nullptr);
        OGRFieldDefn fz("z", OFTReal);
        (void)lyr->CreateField(&fz);
        for (int i = 0; i < 100; ++i) {
            OGRFeature f(lyr->GetLayerDefn());
            f.SetField("z", 20.0 + i * 0.2);
            f.SetGeometry(std::make_unique<OGRPoint>(i * 3.0, (i % 10) * 3.0, 20 + i * 0.2));
            (void)lyr->CreateFeature(&f);
        }
        auto a = inst({"vector", "pipeline"});
        bind(*a, "input", pts);
        (*a)["output-format"] = "MEM";
        a->GetArg("output")->SetDatasetName("");
        (*a)["pipeline"] = "read ! filter --where \"z > 35\" ! buffer 1 ! write";
        ErrorCollector ec;
        Progress p;
        bool ok = a->Run(Progress::fn, &p);
        GDALDataset* o = ok ? outRef(*a) : nullptr;
        a->Finalize();
        printf("vector pipeline run=%d calls=%d\n", ok, p.calls);
        vectorSummary(o, "vpipe");
        if (o) o->Release();
        ec.print("vpipe");
        // vector info on the bound dataset
        auto info = inst({"vector", "info"});
        bind(*info, "input", pts);
        (*info)["output-format"] = "text";
        (*info)["summary"] = true;
        info->Run();
        printf("vector info (text, summary):\n%s", (*info)["output-string"].Get<std::string>().c_str());
        GDALClose(pts);
    }

    printf("=== CLI usage text ===\n");
    {
        auto a = inst({"vector", "buffer"});
        printf("%s\n", a->GetUsageForCLI(true).c_str());
    }
    GDALClose(dem);
    return 0;
}
