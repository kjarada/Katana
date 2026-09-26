// Enumerate GDAL 3.13's algorithm registry recursively and dump every
// algorithm and every argument's metadata as plain data (TSV + counts).
#include <gdal.h>
#include <gdalalgorithm.h>
#include <cpl_error.h>
#include <cpl_string.h>

#include <cstdio>
#include <cmath>
#include <map>
#include <string>
#include <vector>

static const char* typeName(GDALAlgorithmArgType t) { return GDALAlgorithmArgTypeName(t); }

static std::string join(const std::vector<std::string>& v, const char* sep = "|")
{
    std::string s;
    for (auto& e : v) { if (!s.empty()) s += sep; s += e; }
    return s;
}

static std::string dsTypes(int t)
{
    std::string s;
    if (t & GDAL_OF_RASTER) s += "raster,";
    if (t & GDAL_OF_VECTOR) s += "vector,";
    if (t & GDAL_OF_MULTIDIM_RASTER) s += "multidim,";
    if (t & GDAL_OF_UPDATE) s += "update,";
    if (!s.empty()) s.pop_back();
    return s;
}

static std::string defaultAsText(const GDALAlgorithmArg& a)
{
    if (!a.HasDefaultValue()) return "";
    switch (a.GetType()) {
    case GAAT_BOOLEAN: return a.GetDefault<bool>() ? "true" : "false";
    case GAAT_STRING: return a.GetDefault<std::string>();
    case GAAT_INTEGER: return std::to_string(a.GetDefault<int>());
    case GAAT_REAL: return CPLSPrintf("%.17g", a.GetDefault<double>());
    case GAAT_STRING_LIST: return join(a.GetDefault<std::vector<std::string>>(), ",");
    case GAAT_INTEGER_LIST: { std::string s; for (int v : a.GetDefault<std::vector<int>>()) s += std::to_string(v) + ","; return s; }
    case GAAT_REAL_LIST: { std::string s; for (double v : a.GetDefault<std::vector<double>>()) s += CPLSPrintf("%g,", v); return s; }
    default: return "?";
    }
}

struct Counts { int algs = 0, leaves = 0, containers = 0, hidden = 0, args = 0; std::map<std::string,int> byType; };

static FILE* tsvAlg;
static FILE* tsvArg;

static void walk(GDALAlgorithm& alg, const std::string& path, Counts& c, int depth)
{
    ++c.algs;
    const bool container = alg.HasSubAlgorithms();
    if (container) ++c.containers; else ++c.leaves;
    if (alg.IsHidden()) ++c.hidden;
    fprintf(tsvAlg, "%s\t%s\t%s\t%s\t%d\t%s\n", path.c_str(), join(alg.GetAliases()).c_str(),
            container ? "container" : "leaf", alg.IsHidden() ? "hidden" : "", alg.SupportsStreamedOutput(),
            alg.GetDescription().c_str());
    for (auto& up : alg.GetArgs()) {
        const GDALAlgorithmArg& a = *up;
        ++c.args;
        ++c.byType[typeName(a.GetType())];
        auto mn = a.GetMinValue();
        auto mx = a.GetMaxValue();
        std::string meta;
        for (auto& [k, v] : a.GetMetadata()) meta += k + "=" + join(v, ",") + ";";
        fprintf(tsvArg,
                "%s\t%s\t%s\t%s\t%s\treq=%d\tpos=%d\tin=%d\tout=%d\tcnt=%d..%d\tdef=%s\tchoices=%s\tmin=%s\tmax=%s\tds=%s\tdsin=%d\tdsout=%d\tmx=%s\tmd=%s\tdeps=%s\thid=%d/%d/%d\tcat=%s\tmeta=%s\tdesc=%s\n",
                path.c_str(), a.GetName().c_str(), a.GetShortName().c_str(), join(a.GetAliases()).c_str(),
                typeName(a.GetType()), a.IsRequired(), a.IsPositional(), a.IsInput(), a.IsOutput(),
                a.GetMinCount(), a.GetMaxCount() == GDALAlgorithmArgDecl::UNBOUNDED ? -1 : a.GetMaxCount(),
                defaultAsText(a).c_str(), join(a.GetChoices()).c_str(),
                !std::isnan(mn.first) ? CPLSPrintf("%s%g", mn.second ? "[" : "(", mn.first) : "", !std::isnan(mx.first) ? CPLSPrintf("%g%s", mx.first, mx.second ? "]" : ")") : "",
                (a.GetType() == GAAT_DATASET || a.GetType() == GAAT_DATASET_LIST) ? dsTypes(a.GetDatasetType()).c_str() : "",
                (a.GetType() == GAAT_DATASET || a.GetType() == GAAT_DATASET_LIST) ? a.GetDatasetInputFlags() : 0,
                (a.GetType() == GAAT_DATASET || a.GetType() == GAAT_DATASET_LIST) ? a.GetDatasetOutputFlags() : 0,
                a.GetMutualExclusionGroup().c_str(), a.GetMutualDependencyGroup().c_str(),
                join(a.GetDirectDependencies()).c_str(), a.IsHidden(), a.IsHiddenForCLI(), a.IsHiddenForAPI(),
                a.GetCategory().c_str(), meta.c_str(), a.GetDescription().c_str());
    }
    for (const auto& name : alg.GetSubAlgorithmNames()) {
        auto sub = alg.InstantiateSubAlgorithm(name);
        if (!sub) { fprintf(stderr, "cannot instantiate %s %s\n", path.c_str(), name.c_str()); continue; }
        walk(*sub, path + " " + name, c, depth + 1);
    }
}

int main()
{
    GDALAllRegister();
    auto& reg = GDALGlobalAlgorithmRegistry::GetSingleton();
    printf("top-level names:");
    for (auto& n : reg.GetNames()) printf(" %s", n.c_str());
    printf("\n");
    tsvAlg = fopen("algorithms.tsv", "w");
    tsvArg = fopen("arguments.tsv", "w");
    Counts c;
    auto root = reg.Instantiate(GDALGlobalAlgorithmRegistry::ROOT_ALG_NAME);
    if (!root) { printf("no root: %s\n", CPLGetLastErrorMsg()); return 1; }
    walk(*root, "gdal", c, 0);
    fclose(tsvAlg);
    fclose(tsvArg);
    printf("algorithms=%d leaves=%d containers=%d hidden=%d args=%d\n", c.algs, c.leaves, c.containers, c.hidden, c.args);
    for (auto& [k, v] : c.byType) printf("  type %s=%d\n", k.c_str(), v);
    return 0;
}
