// The GDAL algorithm bridge (include/katana/gis/processing.hpp,
// docs/geoprocessing.md "The bridge"): GDAL 3.13's C++ algorithm API behind
// plain types. The rules each block follows were measured by the probes in
// docs/geoprocessing.md; a rule here without a probe behind it is a defect.

#include "katana/gis/processing.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <cctype>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <memory>
#include <mutex>
#include <set>
#include <sstream>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include <cpl_conv.h>
#include <cpl_error.h>
#include <cpl_string.h>
#include <cpl_vsi.h>
#include <gdal_priv.h>
#include <gdalalgorithm.h>
#include <ogr_api.h>
#include <ogr_spatialref.h>
#include <ogrsf_frmts.h>

#include "gdal_registry.hpp"
#include "katana/core/text.hpp"
#include "gdal_run_detail.hpp"
#include "ogr_detail.hpp"

namespace katana::gis::processing {

using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

// ---- words ---------------------------------------------------------------------------------

std::string joined(const std::vector<std::string>& items, std::string_view separator)
{
    std::string text;
    for (const std::string& item : items) {
        text += (text.empty() ? "" : std::string(separator)) + item;
    }
    return text;
}

std::filesystem::path pathOf(const std::string& utf8)
{
    return std::filesystem::path(std::u8string(utf8.begin(), utf8.end()));
}

std::string utf8Of(const std::filesystem::path& path)
{
    const std::u8string text = path.generic_u8string();
    return std::string(text.begin(), text.end());
}

// A dataset name that is a file on disk, as opposed to /vsimem/..., a /vsi
// chain, a URL or a connection string.
bool isPlainFile(const std::string& name)
{
    return !name.empty() && !name.starts_with("/vsi") && name.find("://") == std::string::npos &&
           name.find(':') != 0;
}

// ---- errors: one collector per run, on the run's own thread ------------------------------

// CPLPushErrorHandlerEx is a per-thread stack, so a collector sees exactly the
// errors of the run on its thread (8 threads x 50 runs: 200 errors seen, 0
// leaked between them). Errors GDAL raises on threads it starts itself - the
// viewshed's - skip it and reach the process-wide CPLQuietErrorHandler that
// gdal_adapter.cpp installs; the failure they cause still reaches Run().
class ErrorCollector {
  public:
    ErrorCollector() { CPLPushErrorHandlerEx(&ErrorCollector::handler, this); }
    ~ErrorCollector() { CPLPopErrorHandler(); }
    ErrorCollector(const ErrorCollector&) = delete;
    ErrorCollector& operator=(const ErrorCollector&) = delete;

    [[nodiscard]] const std::vector<Diagnostic>& items() const { return items_; }

    // The first failure raised since `from`, or nullptr.
    [[nodiscard]] const Diagnostic* firstFailure(std::size_t from = 0) const
    {
        for (std::size_t i = from; i < items_.size(); ++i) {
            if (items_[i].failure) {
                return &items_[i];
            }
        }
        return nullptr;
    }

  private:
    static void CPL_STDCALL handler(CPLErr kind, CPLErrorNum number, const char* message)
    {
        if (kind == CE_Debug || kind == CE_None) {
            return;
        }
        auto* self = static_cast<ErrorCollector*>(CPLGetErrorHandlerUserData());
        self->items_.push_back(
            Diagnostic{kind >= CE_Failure, static_cast<int>(number), message ? message : ""});
    }

    std::vector<Diagnostic> items_;
};

// The refusal for a failure GDAL raised, in `code`, carrying its own message
// - which already says which algorithm ("hillshade: ...").
Error gdalError(const ErrorCollector& errors, ErrorCode code, std::string fallback,
                std::size_t from = 0)
{
    if (const Diagnostic* failure = errors.firstFailure(from)) {
        return makeError(code, failure->message);
    }
    return makeError(code, std::move(fallback));
}

Error cancelled()
{
    return makeError(ErrorCode::InvalidState, "cancelled");
}

// ---- thread-local configuration --------------------------------------------------------------

// Allow-listed: options that tune a run and cannot change what it reads,
// writes or runs. Thread-local, so concurrent runs cannot see each other's;
// and for that reason they do not reach threads GDAL starts (measured), so
// nothing that must be right may depend on them.
constexpr std::array<std::string_view, 2> kAllowedConfig{"GDAL_NUM_THREADS", "GDAL_CACHEMAX"};

class ThreadConfig {
  public:
    ThreadConfig() = default;
    ThreadConfig(const ThreadConfig&) = delete;
    ThreadConfig& operator=(const ThreadConfig&) = delete;
    ~ThreadConfig()
    {
        for (auto it = previous_.rbegin(); it != previous_.rend(); ++it) {
            CPLSetThreadLocalConfigOption(it->first.c_str(),
                                          it->second ? it->second->c_str() : nullptr);
        }
    }

    Status set(const std::vector<std::pair<std::string, std::string>>& options)
    {
        for (const auto& [key, value] : options) {
            if (std::ranges::find(kAllowedConfig, key) == kAllowedConfig.end()) {
                return makeError(ErrorCode::InvalidArgument,
                                 "only GDAL_NUM_THREADS and GDAL_CACHEMAX may be set for a run",
                                 key);
            }
            const char* before = CPLGetThreadLocalConfigOption(key.c_str(), nullptr);
            previous_.emplace_back(key, before != nullptr ? std::optional<std::string>(before)
                                                          : std::nullopt);
            CPLSetThreadLocalConfigOption(key.c_str(), value.c_str());
        }
        return {};
    }

  private:
    std::vector<std::pair<std::string, std::optional<std::string>>> previous_;
};

// ---- sidecars ----------------------------------------------------------------------------

// No .aux.xml may be left beside a person's file by a run that only reads it:
// `raster info --stats` writes one when the dataset closes. Neither switch GDAL
// offers does it (measured, docs/geoprocessing.md "Sidecars"): a thread-local
// GDAL_PAM_ENABLED=NO stops the writing but also the READING of a sidecar the
// file already has - its metadata, no-data or georeferencing - and
// GDAL_PAM_PROXY_DIR is used only where the file's own folder is not
// writable. So the sidecar beside each input read by name is noted before the
// run and put back as it was after it: removed when the run made it, restored
// byte for byte when the run changed it. Inputs opened for update are left
// alone: what such an algorithm writes is the change asked for.
class SidecarGuard {
  public:
    SidecarGuard() = default;
    SidecarGuard(const SidecarGuard&) = delete;
    SidecarGuard& operator=(const SidecarGuard&) = delete;
    ~SidecarGuard() { restore(); }

    void watch(const std::string& name)
    {
        if (!isPlainFile(name)) {
            return;
        }
        std::filesystem::path sidecar = pathOf(name);
        sidecar += ".aux.xml";
        if (std::ranges::any_of(kept_, [&](const Kept& kept) { return kept.sidecar == sidecar; })) {
            return;
        }
        Kept kept{sidecar, std::nullopt};
        std::error_code error;
        if (std::filesystem::exists(sidecar, error)) {
            std::ifstream in(sidecar, std::ios::binary);
            kept.bytes = std::string(std::istreambuf_iterator<char>(in), {});
        }
        kept_.push_back(std::move(kept));
    }

    // Once every dataset is closed: the algorithm is gone.
    void restore()
    {
        for (const Kept& kept : kept_) {
            std::error_code error;
            const bool exists = std::filesystem::exists(kept.sidecar, error);
            if (!kept.bytes) {
                if (exists) {
                    std::filesystem::remove(kept.sidecar, error);
                }
                continue;
            }
            std::string now;
            if (exists) {
                std::ifstream in(kept.sidecar, std::ios::binary);
                now = std::string(std::istreambuf_iterator<char>(in), {});
            }
            if (!exists || now != *kept.bytes) {
                std::ofstream out(kept.sidecar, std::ios::binary | std::ios::trunc);
                out << *kept.bytes;
            }
        }
        kept_.clear();
    }

  private:
    struct Kept {
        std::filesystem::path sidecar;
        std::optional<std::string> bytes;
    };
    std::vector<Kept> kept_;
};

// ---- staging for arguments that take names only ------------------------------------------------

// raster calc's inputs, the index inputs and a few more accept a dataset by
// NAME only, so an in-memory grid or feature set is written under
// /vsimem/katana/<run>/ for them and the folder removed when the run ends.
constexpr const char* kStagingRoot = "/vsimem/katana";

class Staging {
  public:
    Staging() = default;
    Staging(const Staging&) = delete;
    Staging& operator=(const Staging&) = delete;
    ~Staging()
    {
        if (!folder_.empty()) {
            VSIRmdirRecursive(folder_.c_str());
        }
    }

    Result<std::string> stage(GDALDataset& dataset, bool raster)
    {
        if (folder_.empty()) {
            static std::atomic<std::uint64_t> runs{0};
            folder_ = std::string(kStagingRoot) + "/" + std::to_string(++runs);
            VSIMkdirRecursive(folder_.c_str(), 0755);
        }
        const std::string name =
            folder_ + "/" + std::to_string(++count_) + (raster ? ".tif" : ".gpkg");
        GDALDriver* driver =
            GetGDALDriverManager()->GetDriverByName(raster ? "GTiff" : "GPKG");
        if (driver == nullptr) {
            return makeError(ErrorCode::Internal, "GDAL has no driver to stage a dataset with",
                             raster ? "GTiff" : "GPKG");
        }
        GDALDataset* copy =
            driver->CreateCopy(name.c_str(), &dataset, FALSE, nullptr, nullptr, nullptr);
        if (copy == nullptr) {
            return makeError(ErrorCode::Internal, "could not stage an input for GDAL", name);
        }
        GDALClose(copy);
        return name;
    }

  private:
    std::string folder_;
    int count_ = 0;
};

// ---- the catalogue -------------------------------------------------------------------------

struct CatalogueData {
    std::vector<AlgorithmInfo> infos;
    // Parallel to infos: GDAL's hidden aliases ("warp" for reproject), which
    // resolve() accepts and nothing lists.
    std::vector<std::vector<std::string>> hiddenAliases;
};

std::unique_ptr<GDALAlgorithm> rootAlgorithm()
{
    katana::gis::detail::ensureGdalRegistered();
    return GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(
        GDALGlobalAlgorithmRegistry::ROOT_ALG_NAME);
}

void walk(const GDALAlgorithm& algorithm, const std::vector<std::string>& path,
          CatalogueData& out)
{
    for (const std::string& name : algorithm.GetSubAlgorithmNames()) {
        std::unique_ptr<GDALAlgorithm> sub = algorithm.InstantiateSubAlgorithm(name, false);
        if (!sub || sub->IsHidden()) {
            continue;
        }
        AlgorithmInfo info;
        info.path = path;
        info.path.push_back(name);
        std::vector<std::string> hidden;
        bool pastSeparator = false;
        for (const std::string& alias : sub->GetAliases()) {
            if (alias == GDALAlgorithmRegistry::HIDDEN_ALIAS_SEPARATOR) {
                pastSeparator = true;
                continue;
            }
            (pastSeparator ? hidden : info.aliases).push_back(alias);
        }
        info.description = sub->GetDescription();
        info.helpUrl = sub->GetHelpFullURL();
        info.container = sub->HasSubAlgorithms();
        if (info.container) {
            // A group runs nothing; its members carry the policy.
            info.policy = Policy::Safe;
            info.policyReason = "container";
        } else {
            info.policy = detail::classify(info.path, *sub, info.policyReason);
        }
        out.infos.push_back(info);
        out.hiddenAliases.push_back(std::move(hidden));
        if (info.container) {
            walk(*sub, info.path, out);
        }
    }
}

const CatalogueData& catalogueData()
{
    static CatalogueData data;
    static std::once_flag once;
    std::call_once(once, [] {
        ErrorCollector quiet; // anything the walk says is not the caller's business
        if (const auto root = rootAlgorithm()) {
            walk(*root, {}, data);
        }
    });
    return data;
}

const AlgorithmInfo* findInfo(const std::vector<std::string>& path)
{
    for (const AlgorithmInfo& info : catalogueData().infos) {
        if (info.path == path) {
            return &info;
        }
    }
    return nullptr;
}

// A fresh instance for every use: Run() is single-shot (a second call fails
// with a misleading ": No such file or directory"). Walked down from the root,
// which reaches the driver family the registry's own lookup misses.
Result<std::unique_ptr<GDALAlgorithm>> instantiate(const std::vector<std::string>& path)
{
    if (path.empty()) {
        return makeError(ErrorCode::InvalidArgument, "name an algorithm; GDAL LIST names them");
    }
    std::unique_ptr<GDALAlgorithm> current = rootAlgorithm();
    for (const std::string& name : path) {
        if (!current) {
            break;
        }
        current = current->InstantiateSubAlgorithm(name, false);
    }
    if (!current) {
        return makeError(ErrorCode::NotFound, "no GDAL algorithm has that path", pathText(path));
    }
    return current;
}

std::string childrenOf(const std::vector<std::string>& path)
{
    std::vector<std::string> children;
    for (const AlgorithmInfo& info : catalogueData().infos) {
        if (info.path.size() == path.size() + 1 &&
            std::equal(path.begin(), path.end(), info.path.begin())) {
            children.push_back(info.path.back());
        }
    }
    return joined(children, " ");
}

// ---- argument specs ------------------------------------------------------------------------

ArgType argTypeOf(GDALAlgorithmArgType type)
{
    switch (type) {
    case GAAT_BOOLEAN:
        return ArgType::Boolean;
    case GAAT_STRING:
        return ArgType::String;
    case GAAT_INTEGER:
        return ArgType::Integer;
    case GAAT_REAL:
        return ArgType::Real;
    case GAAT_STRING_LIST:
        return ArgType::StringList;
    case GAAT_INTEGER_LIST:
        return ArgType::IntegerList;
    case GAAT_REAL_LIST:
        return ArgType::RealList;
    case GAAT_DATASET:
        return ArgType::Dataset;
    case GAAT_DATASET_LIST:
        return ArgType::DatasetList;
    }
    return ArgType::String;
}

std::optional<Bound> boundOf(const std::pair<double, bool>& bound)
{
    // NaN is GDAL's "no bound"; the inclusive flag says nothing about that.
    if (std::isnan(bound.first)) {
        return std::nullopt;
    }
    return Bound{bound.first, bound.second};
}

std::optional<Scalar> defaultOf(const GDALAlgorithmArg& arg)
{
    if (!arg.HasDefaultValue()) {
        return std::nullopt;
    }
    switch (arg.GetType()) {
    case GAAT_BOOLEAN:
        return Scalar(arg.GetDefault<bool>());
    case GAAT_STRING:
        return Scalar(arg.GetDefault<std::string>());
    case GAAT_INTEGER:
        return Scalar(arg.GetDefault<int>());
    case GAAT_REAL:
        return Scalar(arg.GetDefault<double>());
    case GAAT_STRING_LIST:
        return Scalar(arg.GetDefault<std::vector<std::string>>());
    case GAAT_INTEGER_LIST:
        return Scalar(arg.GetDefault<std::vector<int>>());
    case GAAT_REAL_LIST:
        return Scalar(arg.GetDefault<std::vector<double>>());
    case GAAT_DATASET:
    case GAAT_DATASET_LIST:
        break;
    }
    return std::nullopt;
}

bool isDatasetArg(const GDALAlgorithmArg& arg)
{
    return arg.GetType() == GAAT_DATASET || arg.GetType() == GAAT_DATASET_LIST;
}

ArgSpec specOf(const GDALAlgorithmArg& arg)
{
    ArgSpec spec;
    spec.name = arg.GetName();
    spec.shortName = arg.GetShortName();
    spec.description = arg.GetDescription();
    spec.metaVar = arg.GetMetaVar();
    spec.category = arg.GetCategory();
    spec.aliases = arg.GetAliases();
    spec.type = argTypeOf(arg.GetType());
    spec.required = arg.IsRequired();
    spec.positional = arg.IsPositional();
    spec.isInput = arg.IsInput();
    spec.isOutput = arg.IsOutput();
    spec.minCount = arg.GetMinCount();
    spec.maxCount = arg.GetMaxCount() == GDALAlgorithmArgDecl::UNBOUNDED ? -1 : arg.GetMaxCount();
    spec.packedValues = arg.GetPackedValuesAllowed();
    spec.repeatable = arg.GetRepeatedArgAllowed();
    spec.inPipelineStep = arg.IsAvailableInPipelineStep();
    spec.defaultValue = defaultOf(arg);
    spec.choices = arg.GetChoices();
    spec.min = boundOf(arg.GetMinValue());
    spec.max = boundOf(arg.GetMaxValue());
    if (isDatasetArg(arg)) {
        const GDALArgDatasetType kinds = arg.GetDatasetType();
        spec.datasetKinds = ((kinds & GDAL_OF_RASTER) != 0 ? DatasetKind::Raster : 0u) |
                            ((kinds & GDAL_OF_VECTOR) != 0 ? DatasetKind::Vector : 0u) |
                            ((kinds & GDAL_OF_MULTIDIM_RASTER) != 0 ? DatasetKind::Multidim : 0u);
        spec.datasetUpdate = (kinds & GDAL_OF_UPDATE) != 0;
        const int flags = arg.GetDatasetInputFlags();
        spec.acceptsName = (flags & GADV_NAME) != 0;
        spec.acceptsObject = (flags & GADV_OBJECT) != 0;
    }
    spec.exclusionGroup = arg.GetMutualExclusionGroup();
    spec.dependencyGroup = arg.GetMutualDependencyGroup();
    spec.dependsOn = arg.GetDirectDependencies();
    for (const auto& [key, values] : arg.GetMetadata()) {
        spec.metadata[key] = values;
    }
    return spec;
}

// ---- the tail: which arguments its words give values to ------------------------------------------

// The name of the last step of a pipeline and whether it has words after it:
// "... ! write out.tif" names the output, "... ! write" leaves it to the run.
// An `update` step names the dataset it writes into as its output, as write
// does: bound a second one by the run, GDAL refused every such pipeline with
// "update: Positional values starting at 'b.tif' are not expected" (measured).
bool pipelineNamesOutput(std::string_view pipeline)
{
    const std::size_t bang = pipeline.rfind('!');
    std::istringstream last(std::string(bang == std::string_view::npos ? pipeline
                                                                       : pipeline.substr(bang + 1)));
    std::string step;
    std::string argument;
    last >> step;
    if (katana::core::lowered(step) != "write" && katana::core::lowered(step) != "update") {
        return false;
    }
    while (last >> argument) {
        if (!argument.starts_with("-")) {
            return true;
        }
    }
    return false;
}

// Whether a pipeline's last step prints rather than writes a dataset: the
// steps GDAL 3.13 declares with an output-string (its pipeline usage's
// pipeline_algorithms: compare, export-schema, info). Any other last step -
// write, or a processing step whose result is the pipeline's output - makes
// a dataset. A pipeline has an output-string for those three, which is no
// reason to leave its output unbound when it ends in write (measured: every
// pipeline to a layer or a reference raster failed with "write: Positional
// arguments starting at 'OUTPUT' have not been specified").
bool pipelinePrints(std::string_view pipeline)
{
    const std::size_t bang = pipeline.rfind('!');
    std::istringstream last(std::string(bang == std::string_view::npos ? pipeline
                                                                       : pipeline.substr(bang + 1)));
    std::string step;
    last >> step;
    step = katana::core::lowered(step);
    return step == "info" || step == "compare" || step == "export-schema";
}

bool pipelineNamesInput(std::string_view pipeline)
{
    const std::size_t bang = pipeline.find('!');
    std::istringstream first(std::string(pipeline.substr(0, bang)));
    std::string step;
    std::string argument;
    first >> step;
    if (katana::core::lowered(step) != "read") {
        return false;
    }
    while (first >> argument) {
        if (!argument.starts_with("-")) {
            return true;
        }
    }
    return false;
}

struct TailReading {
    std::set<std::string> given; // long names
    std::vector<std::string> positional;
};

// Options by name (--name, --name=value, -n, and aliases, as GDAL's GetArg
// resolves them); a word after an option that is not a flag is its value;
// everything else is positional, assigned as GDAL assigns it: in declaration
// order, skipping what is already set, each list keeping back what the
// required ones after it need.
TailReading readTail(GDALAlgorithm& algorithm, const std::vector<std::string>& tokens,
                     const std::set<std::string>& bound, bool pipeline)
{
    TailReading reading;
    for (std::size_t i = 0; i < tokens.size(); ++i) {
        const std::string& token = tokens[i];
        const bool longOption = token.size() > 2 && token.starts_with("--");
        const bool shortOption = token.size() == 2 && token[0] == '-' &&
                                 std::isalpha(static_cast<unsigned char>(token[1])) != 0;
        if (!longOption && !shortOption) {
            reading.positional.push_back(token);
            continue;
        }
        const std::size_t equals = token.find('=');
        const std::string name = longOption
                                     ? token.substr(2, equals == std::string::npos ? std::string::npos
                                                                                   : equals - 2)
                                     : token.substr(1);
        const GDALAlgorithmArg* arg = algorithm.GetArg(name);
        if (arg == nullptr) {
            continue; // GDAL refuses it by name when it parses the tail
        }
        reading.given.insert(arg->GetName());
        if (equals == std::string::npos && arg->GetType() != GAAT_BOOLEAN) {
            ++i;
        }
    }
    if (pipeline) {
        // One positional: the pipeline text, which may name its input and
        // output in its read and write steps.
        std::string text;
        for (const std::string& word : reading.positional) {
            text += word + ' ';
        }
        if (!reading.positional.empty()) {
            reading.given.insert("pipeline");
        }
        if (pipelineNamesInput(text)) {
            reading.given.insert("input");
        }
        if (pipelineNamesOutput(text)) {
            reading.given.insert("output");
        }
        return reading;
    }
    std::vector<const GDALAlgorithmArg*> open;
    for (const auto& arg : algorithm.GetArgs()) {
        if (arg->IsPositional() && !bound.contains(arg->GetName()) &&
            !reading.given.contains(arg->GetName())) {
            open.push_back(arg.get());
        }
    }
    std::size_t left = reading.positional.size();
    for (std::size_t k = 0; k < open.size() && left > 0; ++k) {
        std::size_t reserve = 0;
        for (std::size_t j = k + 1; j < open.size(); ++j) {
            if (open[j]->IsRequired()) {
                reserve += static_cast<std::size_t>(std::max(1, open[j]->GetMinCount()));
            }
        }
        const bool list = open[k]->GetMaxCount() > 1;
        const std::size_t most =
            list ? static_cast<std::size_t>(open[k]->GetMaxCount()) : std::size_t{1};
        std::size_t take = std::min(left > reserve ? left - reserve : std::size_t{0}, most);
        if (take == 0 && open[k]->IsRequired()) {
            take = 1;
        }
        if (take > 0) {
            reading.given.insert(open[k]->GetName());
            left -= std::min(take, left);
        }
    }
    return reading;
}

// ---- datasets in memory ----------------------------------------------------------------------

struct DatasetRef {
    GDALDataset* dataset = nullptr;
    DatasetRef() = default;
    explicit DatasetRef(GDALDataset* owned) : dataset(owned) {}
    DatasetRef(const DatasetRef&) = delete;
    DatasetRef& operator=(const DatasetRef&) = delete;
    DatasetRef(DatasetRef&& other) noexcept : dataset(std::exchange(other.dataset, nullptr)) {}
    DatasetRef& operator=(DatasetRef&& other) noexcept
    {
        std::swap(dataset, other.dataset);
        return *this;
    }
    ~DatasetRef()
    {
        if (dataset != nullptr) {
            dataset->Release();
        }
    }
};

GDALDriver* memDriver()
{
    return GetGDALDriverManager()->GetDriverByName("MEM");
}

Result<DatasetRef> memRaster(const RasterGrid& grid)
{
    const RasterInfo& info = grid.info;
    const int bands = static_cast<int>(grid.bands.size());
    if (info.width <= 0 || info.height <= 0 || bands <= 0) {
        return makeError(ErrorCode::InvalidArgument, "a grid needs a size and at least one band");
    }
    const std::size_t cells = static_cast<std::size_t>(info.width) * static_cast<std::size_t>(info.height);
    for (const auto& band : grid.bands) {
        if (band.size() != cells) {
            return makeError(ErrorCode::InvalidArgument,
                             "a band of the grid does not hold width x height values");
        }
    }
    const GDALDataType type = GDALGetDataTypeByName(grid.dataType.c_str());
    if (type == GDT_Unknown) {
        return makeError(ErrorCode::InvalidArgument, "GDAL has no data type of that name",
                         grid.dataType);
    }
    DatasetRef dataset(memDriver()->Create("", info.width, info.height, bands, type, nullptr));
    if (dataset.dataset == nullptr) {
        return makeError(ErrorCode::Internal, "GDAL could not make a grid in memory");
    }
    double transform[6];
    std::copy(info.geotransform.begin(), info.geotransform.end(), transform);
    (void)dataset.dataset->SetGeoTransform(transform);
    if (!info.projectionWkt.empty()) {
        OGRSpatialReference crs;
        if (katana::gis::detail::parseCrs(info.projectionWkt, crs)) {
            (void)dataset.dataset->SetSpatialRef(&crs);
        }
    }
    for (int b = 0; b < bands; ++b) {
        GDALRasterBand* band = dataset.dataset->GetRasterBand(b + 1);
        // RasterIO takes one non-const buffer for reading and writing alike;
        // GF_Write only reads it, and copying a band to satisfy the type
        // would double the memory of the largest thing a run holds.
        auto& values = const_cast<std::vector<double>&>(grid.bands[static_cast<std::size_t>(b)]);
        if (band->RasterIO(GF_Write, 0, 0, info.width, info.height, values.data(), info.width,
                           info.height, GDT_Float64, 0, 0, nullptr) != CE_None) {
            return makeError(ErrorCode::Internal, "GDAL could not fill a grid in memory");
        }
        const std::optional<double> noData =
            static_cast<std::size_t>(b) < grid.noData.size()
                ? grid.noData[static_cast<std::size_t>(b)]
                : (b == 0 ? info.noDataValue : std::nullopt);
        if (noData) {
            (void)band->SetNoDataValue(*noData);
        }
    }
    return dataset;
}

OGRFieldType ogrFieldType(FieldType type, OGRFieldSubType& subType)
{
    subType = OFSTNone;
    switch (type) {
    case FieldType::Boolean:
        subType = OFSTBoolean;
        return OFTInteger;
    case FieldType::Integer64:
        return OFTInteger64;
    case FieldType::Real:
        return OFTReal;
    case FieldType::String:
        return OFTString;
    case FieldType::Date:
        return OFTDate;
    case FieldType::DateTime:
        return OFTDateTime;
    }
    return OFTString;
}

OGRwkbGeometryType layerType(const FeatureTable& table)
{
    const bool multi = std::ranges::any_of(
        table.features, [](const Feature& feature) { return feature.parts.size() > 1; });
    OGRwkbGeometryType type = wkbUnknown;
    switch (table.kind) {
    case GeometryKind::Point:
        type = multi ? wkbMultiPoint : wkbPoint;
        break;
    case GeometryKind::LineString:
        type = multi ? wkbMultiLineString : wkbLineString;
        break;
    case GeometryKind::Polygon:
        type = multi ? wkbMultiPolygon : wkbPolygon;
        break;
    case GeometryKind::Unknown:
        return wkbUnknown;
    }
    return table.hasZ ? wkbSetZ(type) : type;
}

OGRGeometry* featureGeometry(const Feature& feature, GeometryKind kind)
{
    if (feature.parts.empty()) {
        return nullptr;
    }
    if (feature.parts.size() == 1) {
        return katana::gis::detail::makeOgrGeometry(feature.parts.front());
    }
    OGRGeometryCollection* collection = nullptr;
    switch (kind) {
    case GeometryKind::Point:
        collection = new OGRMultiPoint();
        break;
    case GeometryKind::LineString:
        collection = new OGRMultiLineString();
        break;
    case GeometryKind::Polygon:
        collection = new OGRMultiPolygon();
        break;
    case GeometryKind::Unknown:
        collection = new OGRGeometryCollection();
        break;
    }
    for (const VectorGeometry& part : feature.parts) {
        if (OGRGeometry* member = katana::gis::detail::makeOgrGeometry(part)) {
            (void)collection->addGeometryDirectly(member);
        }
    }
    return collection;
}

Result<DatasetRef> memVector(const FeatureSet& set)
{
    DatasetRef dataset(memDriver()->Create("", 0, 0, 0, GDT_Unknown, nullptr));
    if (dataset.dataset == nullptr) {
        return makeError(ErrorCode::Internal, "GDAL could not make a feature set in memory");
    }
    int unnamed = 0;
    for (const FeatureTable& table : set.tables) {
        OGRSpatialReference crs;
        const bool hasCrs =
            !table.crsWkt.empty() && katana::gis::detail::parseCrs(table.crsWkt, crs);
        const std::string name =
            table.name.empty() ? "features" + std::to_string(++unnamed) : table.name;
        OGRLayer* layer = dataset.dataset->CreateLayer(name.c_str(), hasCrs ? &crs : nullptr,
                                                       layerType(table), nullptr);
        if (layer == nullptr) {
            return makeError(ErrorCode::Internal, "GDAL could not make a layer in memory", name);
        }
        for (const FieldDef& field : table.fields) {
            OGRFieldSubType subType = OFSTNone;
            OGRFieldDefn definition(field.name.c_str(), ogrFieldType(field.type, subType));
            definition.SetSubType(subType);
            if (layer->CreateField(&definition) != OGRERR_NONE) {
                return makeError(ErrorCode::InvalidArgument, "GDAL refused a field", field.name);
            }
        }
        for (const Feature& feature : table.features) {
            OGRFeature ogr(layer->GetLayerDefn());
            for (std::size_t f = 0; f < table.fields.size() && f < feature.values.size(); ++f) {
                const int index = static_cast<int>(f);
                std::visit(
                    [&](const auto& value) {
                        using Held = std::decay_t<decltype(value)>;
                        if constexpr (std::is_same_v<Held, std::monostate>) {
                            ogr.SetFieldNull(index);
                        } else if constexpr (std::is_same_v<Held, bool>) {
                            ogr.SetField(index, value ? 1 : 0);
                        } else if constexpr (std::is_same_v<Held, std::int64_t>) {
                            ogr.SetField(index, static_cast<GIntBig>(value));
                        } else if constexpr (std::is_same_v<Held, double>) {
                            ogr.SetField(index, value);
                        } else {
                            ogr.SetField(index, value.c_str());
                        }
                    },
                    feature.values[f]);
            }
            if (OGRGeometry* geometry = featureGeometry(feature, table.kind)) {
                if (hasCrs) {
                    geometry->assignSpatialReference(layer->GetSpatialRef());
                }
                (void)ogr.SetGeometryDirectly(geometry);
            }
            if (layer->CreateFeature(&ogr) != OGRERR_NONE) {
                return makeError(ErrorCode::InvalidArgument, "GDAL refused a feature", name);
            }
        }
    }
    return dataset;
}

// ---- reading an output back -----------------------------------------------------------------

std::string wktOf(const OGRSpatialReference* crs)
{
    if (crs == nullptr) {
        return {};
    }
    char* text = nullptr;
    const char* options[] = {"FORMAT=WKT2_2019", nullptr};
    if (crs->exportToWkt(&text, options) != OGRERR_NONE) {
        CPLFree(text);
        return {};
    }
    std::string wkt = text != nullptr ? text : "";
    CPLFree(text);
    return wkt;
}

Result<RasterGrid> readGrid(GDALDataset& dataset)
{
    RasterGrid grid;
    RasterInfo& info = grid.info;
    info.width = dataset.GetRasterXSize();
    info.height = dataset.GetRasterYSize();
    info.bandCount = dataset.GetRasterCount();
    info.projectionWkt = wktOf(dataset.GetSpatialRef());
    double transform[6];
    if (dataset.GetGeoTransform(transform) == CE_None) {
        std::copy(std::begin(transform), std::end(transform), info.geotransform.begin());
        info.hasGeotransform = true;
    }
    const std::size_t cells = static_cast<std::size_t>(info.width) * static_cast<std::size_t>(info.height);
    for (int b = 1; b <= info.bandCount; ++b) {
        GDALRasterBand* band = dataset.GetRasterBand(b);
        if (b == 1) {
            grid.dataType = GDALGetDataTypeName(band->GetRasterDataType());
        }
        std::vector<double> values(cells);
        if (band->RasterIO(GF_Read, 0, 0, info.width, info.height, values.data(), info.width,
                           info.height, GDT_Float64, 0, 0, nullptr) != CE_None) {
            return makeError(ErrorCode::Internal, "GDAL could not read an output band back");
        }
        grid.bands.push_back(std::move(values));
        int has = 0;
        const double noData = band->GetNoDataValue(&has);
        grid.noData.push_back(has != 0 ? std::optional<double>(noData) : std::nullopt);
    }
    info.noDataValue = grid.noData.empty() ? std::nullopt : grid.noData.front();
    return grid;
}

FieldType fieldTypeOf(const OGRFieldDefn& field)
{
    switch (field.GetType()) {
    case OFTInteger:
        return field.GetSubType() == OFSTBoolean ? FieldType::Boolean : FieldType::Integer64;
    case OFTInteger64:
        return FieldType::Integer64;
    case OFTReal:
        return FieldType::Real;
    case OFTDate:
        return FieldType::Date;
    case OFTDateTime:
        return FieldType::DateTime;
    default:
        return FieldType::String;
    }
}

GeometryKind kindOf(OGRwkbGeometryType type)
{
    switch (wkbFlatten(type)) {
    case wkbPoint:
    case wkbMultiPoint:
        return GeometryKind::Point;
    case wkbLineString:
    case wkbMultiLineString:
        return GeometryKind::LineString;
    case wkbPolygon:
    case wkbMultiPolygon:
        return GeometryKind::Polygon;
    default:
        return GeometryKind::Unknown;
    }
}

// An ISO 8601 date or date-time, as FieldType::Date and DateTime carry them.
std::string isoDate(const OGRFeature& feature, int index, bool withTime)
{
    int year = 0, month = 0, day = 0, hour = 0, minute = 0, zone = 0;
    float second = 0.0F;
    if (feature.GetFieldAsDateTime(index, &year, &month, &day, &hour, &minute, &second, &zone) ==
        FALSE) {
        return feature.GetFieldAsString(index);
    }
    // Whole numbers only: printf's %f would follow the C locale's decimal point.
    const auto two = [](int value) { return (value < 10 ? "0" : "") + std::to_string(value); };
    std::string text = std::to_string(year) + "-" + two(month) + "-" + two(day);
    if (withTime) {
        const int whole = static_cast<int>(second);
        const int millis = static_cast<int>(std::lround((second - static_cast<float>(whole)) * 1000.0F));
        text += "T" + two(hour) + ":" + two(minute) + ":" + two(whole);
        if (millis > 0) {
            text += "." + std::string(millis < 100 ? (millis < 10 ? "00" : "0") : "") +
                    std::to_string(millis);
        }
    }
    return text;
}

FeatureSet readFeatures(GDALDataset& dataset, std::vector<Diagnostic>& diagnostics)
{
    FeatureSet set;
    std::vector<std::string> warnings;
    for (OGRLayer* layer : dataset.GetLayers()) {
        FeatureTable table;
        table.name = layer->GetName();
        table.kind = kindOf(layer->GetGeomType());
        table.hasZ = wkbHasZ(layer->GetGeomType()) != 0;
        table.crsWkt = wktOf(layer->GetSpatialRef());
        OGRFeatureDefn* definition = layer->GetLayerDefn();
        for (int f = 0; f < definition->GetFieldCount(); ++f) {
            const OGRFieldDefn* field = definition->GetFieldDefn(f);
            table.fields.push_back(FieldDef{field->GetNameRef(), fieldTypeOf(*field)});
        }
        layer->ResetReading();
        for (auto& ogr : *layer) {
            Feature feature;
            katana::gis::detail::flattenOgrGeometry(ogr->GetGeometryRef(), feature.parts, warnings);
            if (table.kind == GeometryKind::Unknown && !feature.parts.empty()) {
                table.kind = feature.parts.front().kind;
            }
            for (const VectorGeometry& part : feature.parts) {
                table.hasZ = table.hasZ || part.hasZ;
            }
            for (int f = 0; f < definition->GetFieldCount(); ++f) {
                if (!ogr->IsFieldSetAndNotNull(f)) {
                    feature.values.emplace_back(std::monostate{});
                    continue;
                }
                switch (table.fields[static_cast<std::size_t>(f)].type) {
                case FieldType::Boolean:
                    feature.values.emplace_back(ogr->GetFieldAsInteger(f) != 0);
                    break;
                case FieldType::Integer64:
                    feature.values.emplace_back(static_cast<std::int64_t>(ogr->GetFieldAsInteger64(f)));
                    break;
                case FieldType::Real:
                    feature.values.emplace_back(ogr->GetFieldAsDouble(f));
                    break;
                case FieldType::Date:
                    feature.values.emplace_back(isoDate(*ogr, f, false));
                    break;
                case FieldType::DateTime:
                    feature.values.emplace_back(isoDate(*ogr, f, true));
                    break;
                case FieldType::String:
                    feature.values.emplace_back(std::string(ogr->GetFieldAsString(f)));
                    break;
                }
            }
            table.features.push_back(std::move(feature));
        }
        set.tables.push_back(std::move(table));
    }
    for (std::string& warning : warnings) {
        diagnostics.push_back(Diagnostic{false, 0, std::move(warning)});
    }
    return set;
}

// A colour map's picture made in memory says what its bands are. GDAL 3.13's
// raster color-map leaves every band's colour interpretation Undefined when
// it writes to MEM (a GTiff output gets red, green, blue and alpha from the
// driver's own defaults, which is why the command line looks right). Copied
// on so, the picture reads as three undesignated colour bands and a fourth
// no reader may take for alpha - it could as well be near infrared - and
// every cell the map made clear is drawn opaque. Its bands are, by the
// algorithm's definition, red, green, blue and, with --add-alpha, alpha.
void stampColourMapBands(const std::vector<std::string>& path, GDALDataset& dataset)
{
    if (path != std::vector<std::string>{"raster", "color-map"}) {
        return;
    }
    const int count = dataset.GetRasterCount();
    if (count != 3 && count != 4) {
        return;
    }
    constexpr GDALColorInterp kRoles[] = {GCI_RedBand, GCI_GreenBand, GCI_BlueBand, GCI_AlphaBand};
    for (int i = 1; i <= count; ++i) {
        if (dataset.GetRasterBand(i)->GetColorInterpretation() != GCI_Undefined) {
            return; // said already: a later GDAL that sets them is taken at its word
        }
    }
    for (int i = 1; i <= count; ++i) {
        dataset.GetRasterBand(i)->SetColorInterpretation(kRoles[i - 1]);
    }
}

// A raster too big for memory, as the tiled GeoTIFF a derived reference
// raster is kept in: DEFLATE, with the floating-point predictor for
// floating-point data (docs/geoprocessing.md, "Outputs").
Result<std::string> spill(GDALDataset& dataset, const std::string& directory)
{
    std::filesystem::path folder = directory.empty()
                                       ? std::filesystem::temp_directory_path()
                                       : pathOf(directory);
    std::error_code error;
    std::filesystem::create_directories(folder, error);
    static std::atomic<std::uint64_t> spilled{0};
    const std::filesystem::path file =
        folder / ("katana-" + std::to_string(CPLGetPID()) + "-" + std::to_string(++spilled) + ".tif");
    const GDALDataType type = dataset.GetRasterBand(1)->GetRasterDataType();
    const bool floating = GDALDataTypeIsFloating(type) != 0;
    CPLStringList options;
    options.SetNameValue("TILED", "YES");
    options.SetNameValue("COMPRESS", "DEFLATE");
    if (floating) {
        options.SetNameValue("PREDICTOR", "3");
    }
    options.SetNameValue("BIGTIFF", "IF_SAFER");
    GDALDriver* gtiff = GetGDALDriverManager()->GetDriverByName("GTiff");
    const std::string name = utf8Of(file);
    GDALDataset* copy =
        gtiff->CreateCopy(name.c_str(), &dataset, FALSE, options.List(), nullptr, nullptr);
    if (copy == nullptr) {
        return makeError(ErrorCode::FileExportFailure, "could not write the raster to a file",
                         name);
    }
    GDALClose(copy);
    return name;
}

// ---- binding a request --------------------------------------------------------------------

// Declared in the order that makes destruction safe: the algorithm goes
// first, closing what it opened, then our references to the inputs in
// memory, and last the staged copies it may have held open.
struct Binding {
    Staging staging;
    std::vector<DatasetRef> owned; // our references to the MEM inputs
    GDALAlgorithmArg* output = nullptr; // the dataset output, when there is one
    bool memoryOutput = false;
    std::string outputFile; // the file written, when it is one
    bool outputExisted = false;
    std::unique_ptr<GDALAlgorithm> algorithm;
};

std::string suggestion(const GDALAlgorithm& algorithm, const std::string& name)
{
    const std::string near = algorithm.GetSuggestionForArgumentName(name);
    return near.empty() ? std::string() : "; did you mean " + near + "?";
}

Result<GDALArgDatasetValue> datasetValue(const GDALAlgorithmArg& arg, const DatasetValue& value,
                                         Binding& bound)
{
    if (const auto* path = std::get_if<DatasetPath>(&value)) {
        return GDALArgDatasetValue(path->path);
    }
    const bool raster = std::holds_alternative<RasterGrid>(value);
    auto dataset = raster ? memRaster(std::get<RasterGrid>(value))
                          : memVector(std::get<FeatureSet>(value));
    if (!dataset) {
        return dataset.error();
    }
    const int flags = arg.GetDatasetInputFlags();
    if ((flags & GADV_OBJECT) != 0) {
        GDALArgDatasetValue held(dataset->dataset);
        bound.owned.push_back(std::move(dataset).value());
        return held;
    }
    auto staged = bound.staging.stage(*dataset->dataset, raster);
    if (!staged) {
        return staged.error();
    }
    return GDALArgDatasetValue(*staged);
}

Status bindValue(GDALAlgorithm& algorithm, const std::string& name, const ArgValue& value,
                 Binding& bound, const ErrorCollector& errors)
{
    GDALAlgorithmArg* arg = algorithm.GetArg(name);
    if (arg == nullptr || arg->IsHiddenForAPI()) {
        return makeError(ErrorCode::InvalidArgument,
                         "no argument of " + algorithm.GetName() + " is called " + name +
                             suggestion(algorithm, name),
                         name);
    }
    const std::size_t mark = errors.items().size();
    const auto refused = [&]() -> Status {
        return gdalError(errors, ErrorCode::InvalidArgument,
                         "GDAL refused the value of " + arg->GetName(), mark);
    };
    if (const auto* scalar = std::get_if<Scalar>(&value)) {
        if (isDatasetArg(*arg)) {
            // A dataset by name, as the command line gives one.
            std::vector<std::string> names;
            if (const auto* one = std::get_if<std::string>(scalar)) {
                names.push_back(*one);
            } else if (const auto* many = std::get_if<std::vector<std::string>>(scalar)) {
                names = *many;
            } else {
                return makeError(ErrorCode::InvalidArgument,
                                 arg->GetName() + " is a dataset: give a name or a source");
            }
            if (arg->GetType() == GAAT_DATASET) {
                return names.size() == 1 && arg->SetDatasetName(names.front()) ? Status{}
                                                                                : refused();
            }
            std::vector<GDALArgDatasetValue> list;
            for (const std::string& each : names) {
                list.emplace_back(each);
            }
            return arg->Set(std::move(list)) ? Status{} : refused();
        }
        const bool set = std::visit([&](const auto& held) { return arg->Set(held); }, *scalar);
        return set ? Status{} : refused();
    }
    if (!isDatasetArg(*arg)) {
        return makeError(ErrorCode::InvalidArgument,
                         arg->GetName() + " is not a dataset; it takes a value", arg->GetName());
    }
    std::vector<const DatasetValue*> values;
    if (const auto* one = std::get_if<DatasetValue>(&value)) {
        values.push_back(one);
    } else {
        for (const DatasetValue& each : std::get<std::vector<DatasetValue>>(value)) {
            values.push_back(&each);
        }
    }
    if (values.empty()) {
        return makeError(ErrorCode::InvalidArgument, arg->GetName() + " was given no dataset");
    }
    for (const DatasetValue* each : values) {
        if (const auto* path = std::get_if<DatasetPath>(each)) {
            if (!path->openOptions.empty()) {
                GDALAlgorithmArg* openOptions = algorithm.GetArg("open-option");
                if (openOptions == nullptr || !openOptions->Set(path->openOptions)) {
                    return makeError(ErrorCode::InvalidArgument,
                                     algorithm.GetName() + " takes no open options");
                }
            }
            if (!path->layer.empty()) {
                GDALAlgorithmArg* layer = algorithm.GetArg("input-layer");
                if (layer == nullptr || !layer->Set(std::vector<std::string>{path->layer})) {
                    return makeError(ErrorCode::InvalidArgument,
                                     algorithm.GetName() + " takes no input layer");
                }
            }
        }
    }
    if (arg->GetType() == GAAT_DATASET) {
        if (values.size() != 1) {
            return makeError(ErrorCode::InvalidArgument,
                             arg->GetName() + " takes one dataset, not " +
                                 std::to_string(values.size()));
        }
        auto held = datasetValue(*arg, *values.front(), bound);
        if (!held) {
            return held.error();
        }
        const bool set = held->GetDatasetRef() != nullptr
                             ? arg->Set(held->GetDatasetRef())
                             : arg->SetDatasetName(held->GetName());
        return set ? Status{} : refused();
    }
    std::vector<GDALArgDatasetValue> list;
    for (const DatasetValue* each : values) {
        auto held = datasetValue(*arg, *each, bound);
        if (!held) {
            return held.error();
        }
        list.push_back(std::move(held).value());
    }
    // Set with a moved list: writing into the argument's own list does not
    // mark it set.
    return arg->Set(std::move(list)) ? Status{} : refused();
}

// A GDAL failure where an input is opened is FileImportFailure; where the
// output is written, FileExportFailure; anything else as `otherwise`.
Error classified(const ErrorCollector& errors, const Binding& bound, ErrorCode otherwise,
                 std::string fallback, std::size_t from = 0)
{
    const Diagnostic* failure = errors.firstFailure(from);
    if (failure == nullptr) {
        return makeError(otherwise, std::move(fallback));
    }
    const bool mentionsOutput = !bound.outputFile.empty() &&
                                failure->message.find(bound.outputFile) != std::string::npos;
    if (failure->code == CPLE_OpenFailed || failure->code == CPLE_FileIO) {
        return makeError(mentionsOutput ? ErrorCode::FileExportFailure
                                        : ErrorCode::FileImportFailure,
                         failure->message);
    }
    return makeError(otherwise, failure->message);
}

void watchInputs(GDALAlgorithm& algorithm, SidecarGuard& sidecars)
{
    for (const auto& arg : algorithm.GetArgs()) {
        if (!isDatasetArg(*arg) || arg->IsOutput() || !arg->IsExplicitlySet() ||
            (arg->GetDatasetType() & GDAL_OF_UPDATE) != 0) {
            continue;
        }
        if (arg->GetType() == GAAT_DATASET) {
            sidecars.watch(arg->Get<GDALArgDatasetValue>().GetName());
        } else {
            for (const GDALArgDatasetValue& each : arg->Get<std::vector<GDALArgDatasetValue>>()) {
                sidecars.watch(each.GetName());
            }
        }
    }
}

Status bind(const RunRequest& request, Binding& bound, const ErrorCollector& errors,
            SidecarGuard& sidecars)
{
    auto algorithm = instantiate(request.path);
    if (!algorithm) {
        return algorithm.error();
    }
    if ((*algorithm)->HasSubAlgorithms()) {
        return makeError(ErrorCode::InvalidArgument,
                         pathText(request.path) + " is a group: " + childrenOf(request.path),
                         pathText(request.path));
    }
    bound.algorithm = std::move(algorithm).value();
    GDALAlgorithm& alg = *bound.algorithm;

    std::set<std::string> boundNames;
    for (const auto& [name, value] : request.values) {
        if (name == "pipeline") {
            if (const auto* scalar = std::get_if<Scalar>(&value)) {
                if (const auto* text = std::get_if<std::string>(scalar);
                    text != nullptr && detail::hasExternalStep(*text)) {
                    return makeError(ErrorCode::InvalidArgument,
                                     "a pipeline step 'external' runs a program and is refused");
                }
            }
        }
        if (auto status = bindValue(alg, name, value, bound, errors); !status) {
            return status;
        }
        boundNames.insert(alg.GetArg(name)->GetName());
    }

    std::vector<std::string> tokens = request.tokens;
    const bool pipeline = detail::isPipeline(request.path);
    const TailReading tail = readTail(alg, tokens, boundNames, pipeline);
    if (pipeline && !boundNames.contains("pipeline") && tail.positional.size() == 1 &&
        tail.positional.front().find_first_of(" !") != std::string::npos) {
        // The pipeline as one quoted word: set as the argument it is, since
        // GDAL's parser reads a pipeline from separate words.
        const std::string text = tail.positional.front();
        if (!alg.GetArg("pipeline")->Set(text)) {
            return gdalError(errors, ErrorCode::InvalidArgument, "GDAL refused the pipeline");
        }
        std::erase(tokens, text);
    }

    // The output: in memory, or the file asked for, unless the tail names it.
    GDALAlgorithmArg* output = alg.GetArg("output");
    if (output != nullptr && output->GetType() != GAAT_DATASET) {
        output = nullptr; // raster tile's, vector partition's: a folder name
    }
    bound.output = output;
    const bool tailNamesOutput = output != nullptr && tail.given.contains(output->GetName());
    if (output != nullptr && !tailNamesOutput && !output->IsExplicitlySet()) {
        // An info-like algorithm whose output is optional prints to
        // output-string by default, and is left to.
        GDALAlgorithmArg* steps = pipeline ? alg.GetArg("pipeline") : nullptr;
        const bool printsByDefault =
            !output->IsRequired() && alg.GetArg("output-string") != nullptr &&
            (steps == nullptr || steps->GetType() != GAAT_STRING ||
             pipelinePrints(steps->Get<std::string>()));
        if (request.outputTo == OutputTo::Memory && !printsByDefault) {
            if (GDALAlgorithmArg* format = alg.GetArg("output-format");
                format != nullptr && !tail.given.contains(format->GetName()) && !format->Set("MEM")) {
                return gdalError(errors, ErrorCode::InvalidArgument,
                                 pathText(request.path) + " cannot write to memory");
            }
            if (!output->SetDatasetName("")) {
                return gdalError(errors, ErrorCode::InvalidArgument,
                                 pathText(request.path) + " cannot write to memory");
            }
            bound.memoryOutput = true;
        } else if (request.outputTo == OutputTo::File) {
            if (request.outputPath.empty()) {
                return makeError(ErrorCode::InvalidArgument, "a file output needs a path");
            }
            if (!output->SetDatasetName(request.outputPath)) {
                return gdalError(errors, ErrorCode::InvalidArgument, "GDAL refused the output");
            }
            if (!request.outputFormat.empty()) {
                GDALAlgorithmArg* format = alg.GetArg("output-format");
                if (format == nullptr || !format->Set(request.outputFormat)) {
                    return gdalError(errors, ErrorCode::InvalidArgument,
                                     "GDAL refused the output format " + request.outputFormat);
                }
            }
            if (!request.creationOptions.empty()) {
                GDALAlgorithmArg* options = alg.GetArg("creation-option");
                if (options == nullptr || !options->Set(request.creationOptions)) {
                    return gdalError(errors, ErrorCode::InvalidArgument,
                                     pathText(request.path) + " takes no creation options");
                }
            }
            bound.outputFile = request.outputPath;
        }
    }
    // Replacing a file the request, or GDAL's own words, name: whoever built
    // the request has asked (OVERWRITE); the words' own --overwrite is theirs.
    if (request.overwrite && !tail.given.contains("overwrite")) {
        if (GDALAlgorithmArg* overwrite = alg.GetArg("overwrite");
            overwrite != nullptr && overwrite->GetType() == GAAT_BOOLEAN) {
            (void)overwrite->Set(true);
        }
    }

    if (!tokens.empty()) {
        const std::size_t mark = errors.items().size();
        if (!alg.ParseCommandLineArguments(tokens)) {
            return classified(errors, bound, ErrorCode::InvalidArgument,
                              "GDAL could not read the arguments", mark);
        }
    }
    if (tailNamesOutput && output->IsExplicitlySet()) {
        bound.outputFile = output->Get<GDALArgDatasetValue>().GetName();
    }
    if (!bound.outputFile.empty() && isPlainFile(bound.outputFile)) {
        std::error_code error;
        bound.outputExisted = std::filesystem::exists(pathOf(bound.outputFile), error);
    }
    watchInputs(alg, sidecars);
    return {};
}

// ---- progress and cancel -------------------------------------------------------------------

struct ProgressState {
    const std::stop_token* stop = nullptr;
    const std::function<void(double)>* progress = nullptr;
    // Set when the adapter told GDAL to stop. What makes a run cancelled,
    // whatever Run() returns: hillshade returns true after a refused progress
    // call, with a partial output.
    std::atomic<bool> refused{false};
};

int CPL_STDCALL progressAdapter(double complete, const char*, void* data)
{
    auto* state = static_cast<ProgressState*>(data);
    if (*state->progress) {
        (*state->progress)(std::clamp(complete, 0.0, 1.0));
    }
    if (state->stop->stop_requested()) {
        state->refused = true;
        return FALSE;
    }
    return TRUE;
}

} // namespace

// ---- the public functions ------------------------------------------------------------------------

std::string pathText(const std::vector<std::string>& path)
{
    return joined(path, " ");
}

std::string_view toString(ArgType type)
{
    switch (type) {
    case ArgType::Boolean:
        return "boolean";
    case ArgType::String:
        return "string";
    case ArgType::Integer:
        return "integer";
    case ArgType::Real:
        return "real";
    case ArgType::StringList:
        return "string_list";
    case ArgType::IntegerList:
        return "integer_list";
    case ArgType::RealList:
        return "real_list";
    case ArgType::Dataset:
        return "dataset";
    case ArgType::DatasetList:
        return "dataset_list";
    }
    return "string";
}

std::string_view toString(Policy policy)
{
    return policy == Policy::Safe ? "safe" : "confirm";
}

std::string datasetKindsText(unsigned kinds)
{
    std::vector<std::string> words;
    if ((kinds & DatasetKind::Raster) != 0) {
        words.emplace_back("raster");
    }
    if ((kinds & DatasetKind::Vector) != 0) {
        words.emplace_back("vector");
    }
    if ((kinds & DatasetKind::Multidim) != 0) {
        words.emplace_back("multidim");
    }
    return joined(words, ",");
}

std::string toString(const Scalar& value)
{
    const auto real = [](double number) { return katana::core::formatExactReal(number); };
    return std::visit(
        [&](const auto& held) -> std::string {
            using Held = std::decay_t<decltype(held)>;
            if constexpr (std::is_same_v<Held, bool>) {
                return held ? "true" : "false";
            } else if constexpr (std::is_same_v<Held, std::string>) {
                return held;
            } else if constexpr (std::is_same_v<Held, int>) {
                return std::to_string(held);
            } else if constexpr (std::is_same_v<Held, double>) {
                return real(held);
            } else if constexpr (std::is_same_v<Held, std::vector<std::string>>) {
                return joined(held, ",");
            } else if constexpr (std::is_same_v<Held, std::vector<int>>) {
                std::vector<std::string> words;
                for (const int each : held) {
                    words.push_back(std::to_string(each));
                }
                return joined(words, ",");
            } else {
                std::vector<std::string> words;
                for (const double each : held) {
                    words.push_back(real(each));
                }
                return joined(words, ",");
            }
        },
        value);
}

const std::vector<AlgorithmInfo>& catalogue()
{
    return catalogueData().infos;
}

Result<std::vector<std::string>> resolve(const std::vector<std::string>& words,
                                         std::size_t& consumed)
{
    consumed = 0;
    if (words.empty()) {
        return makeError(ErrorCode::InvalidArgument, "name an algorithm; GDAL LIST names them");
    }
    const CatalogueData& data = catalogueData();
    std::vector<std::string> path;
    const AlgorithmInfo* reached = nullptr;
    for (std::size_t i = 0; i < words.size(); ++i) {
        const std::string word = katana::core::lowered(words[i]);
        const AlgorithmInfo* next = nullptr;
        for (std::size_t e = 0; e < data.infos.size() && next == nullptr; ++e) {
            const AlgorithmInfo& info = data.infos[e];
            if (info.path.size() != path.size() + 1 ||
                !std::equal(path.begin(), path.end(), info.path.begin())) {
                continue;
            }
            const auto named = [&](const std::string& name) { return katana::core::lowered(name) == word; };
            if (named(info.path.back()) || std::ranges::any_of(info.aliases, named) ||
                std::ranges::any_of(data.hiddenAliases[e], named)) {
                next = &info;
            }
        }
        if (next == nullptr) {
            break;
        }
        reached = next;
        path = next->path;
        consumed = i + 1;
        if (!next->container) {
            return path;
        }
    }
    if (reached == nullptr) {
        return makeError(ErrorCode::NotFound,
                         "no GDAL algorithm is called that; GDAL LIST names them: " +
                             childrenOf({}),
                         words.front());
    }
    return makeError(ErrorCode::InvalidArgument,
                     pathText(path) + " is a group: name one of " + childrenOf(path),
                     pathText(path));
}

Result<AlgorithmSpec> describe(const std::vector<std::string>& path)
{
    const AlgorithmInfo* info = findInfo(path);
    if (info == nullptr) {
        return makeError(ErrorCode::NotFound, "no GDAL algorithm has that path", pathText(path));
    }
    ErrorCollector quiet;
    auto algorithm = instantiate(path);
    if (!algorithm) {
        return algorithm.error();
    }
    AlgorithmSpec spec;
    spec.info = *info;
    spec.longDescription = (*algorithm)->GetLongDescription();
    spec.usageJson = (*algorithm)->GetUsageAsJSON();
    if (!info->container) {
        for (const auto& arg : (*algorithm)->GetArgs()) {
            if (arg->IsHidden() || arg->IsHiddenForAPI()) {
                continue;
            }
            spec.args.push_back(specOf(*arg));
        }
    }
    return spec;
}

std::vector<std::string> suggest(const std::vector<std::string>& path, const std::string& arg,
                                 const std::string& prefix)
{
    ErrorCollector quiet;
    auto algorithm = instantiate(path);
    if (!algorithm) {
        return {};
    }
    const GDALAlgorithmArg* found = (*algorithm)->GetArg(arg);
    return found != nullptr ? found->GetAutoCompleteChoices(prefix) : std::vector<std::string>{};
}

Result<std::vector<std::string>> argumentsGiven(const std::vector<std::string>& path,
                                                const std::vector<std::string>& tokens,
                                                const std::vector<std::string>& bound)
{
    ErrorCollector quiet;
    auto algorithm = instantiate(path);
    if (!algorithm) {
        return algorithm.error();
    }
    std::set<std::string> names;
    for (const std::string& name : bound) {
        const GDALAlgorithmArg* arg = (*algorithm)->GetArg(name);
        names.insert(arg != nullptr ? arg->GetName() : name);
    }
    const TailReading tail = readTail(**algorithm, tokens, names, detail::isPipeline(path));
    return std::vector<std::string>(tail.given.begin(), tail.given.end());
}

Status validate(const RunRequest& request)
{
    katana::gis::detail::ensureGdalRegistered();
    if (auto status = checkTokens(request.path, request.tokens); !status) {
        return status;
    }
    ErrorCollector errors;
    ThreadConfig config;
    if (auto status = config.set(request.config); !status) {
        return status;
    }
    SidecarGuard sidecars;
    Binding bound;
    if (auto status = bind(request, bound, errors, sidecars); !status) {
        return status;
    }
    const std::size_t mark = errors.items().size();
    if (!bound.algorithm->ValidateArguments()) {
        return classified(errors, bound, ErrorCode::InvalidArgument,
                          "GDAL refused the arguments", mark);
    }
    bound.algorithm.reset();
    return {};
}

Result<RunOutputs> run(const RunRequest& request, const std::stop_token& stop,
                       const std::function<void(double)>& progress)
{
    katana::gis::detail::ensureGdalRegistered();
    if (auto status = checkTokens(request.path, request.tokens); !status) {
        return status.error();
    }
    if (stop.stop_requested()) {
        return cancelled();
    }
    const auto started = std::chrono::steady_clock::now();
    ErrorCollector errors;
    ThreadConfig config;
    if (auto status = config.set(request.config); !status) {
        return status.error();
    }
    // Declared before `bound`, so the sidecars are put back after every
    // dataset the run opened has closed.
    SidecarGuard sidecars;
    Binding bound;
    if (auto status = bind(request, bound, errors, sidecars); !status) {
        return status.error();
    }
    GDALAlgorithm& algorithm = *bound.algorithm;
    // Validated apart from the run, so that what is wrong with the request is
    // InvalidArgument and not a failure of the algorithm.
    std::size_t mark = errors.items().size();
    if (!algorithm.ValidateArguments()) {
        return classified(errors, bound, ErrorCode::InvalidArgument, "GDAL refused the arguments",
                          mark);
    }
    if (stop.stop_requested()) {
        return cancelled();
    }

    ProgressState state;
    state.stop = &stop;
    state.progress = &progress;
    mark = errors.items().size();
    const bool ran = algorithm.Run(&progressAdapter, &state);
    const bool wasCancelled = state.refused || stop.stop_requested();
    // Taken BEFORE Finalize, which drops the algorithm's reference and so,
    // for a dataset in memory, the dataset.
    DatasetRef output;
    if (ran && !wasCancelled && bound.output != nullptr) {
        output = DatasetRef(bound.output->Get<GDALArgDatasetValue>().GetDatasetIncreaseRefCount());
    }
    const bool finalized = algorithm.Finalize();
    if (wasCancelled) {
        output = DatasetRef();
        bound.algorithm.reset();
        // A partial file the run began is not left looking like a result.
        if (!bound.outputFile.empty() && !bound.outputExisted && isPlainFile(bound.outputFile)) {
            std::error_code error;
            std::filesystem::remove(pathOf(bound.outputFile), error);
        }
        return cancelled();
    }
    if (!ran || !finalized) {
        return classified(errors, bound, ErrorCode::CommandRejected,
                          "GDAL reported a failure and said nothing about it", mark);
    }

    RunOutputs outputs;
    if (bound.memoryOutput && output.dataset != nullptr) {
        GDALDataset& dataset = *output.dataset;
        stampColourMapBands(request.path, dataset);
        if (dataset.GetRasterCount() > 0) {
            const std::uint64_t cells = static_cast<std::uint64_t>(dataset.GetRasterXSize()) *
                                        static_cast<std::uint64_t>(dataset.GetRasterYSize()) *
                                        static_cast<std::uint64_t>(dataset.GetRasterCount());
            if (cells > request.maxMemoryCells) {
                auto file = spill(dataset, request.spillDirectory);
                if (!file) {
                    return file.error();
                }
                outputs.file = *file;
            } else {
                auto grid = readGrid(dataset);
                if (!grid) {
                    return grid.error();
                }
                outputs.raster = std::move(grid).value();
            }
        }
        if (dataset.GetLayerCount() > 0) {
            outputs.features = readFeatures(dataset, outputs.diagnostics);
        }
    } else if (!bound.outputFile.empty()) {
        outputs.file = bound.outputFile;
    }
    if (const GDALAlgorithmArg* text = algorithm.GetArg("output-string");
        text != nullptr && text->GetType() == GAAT_STRING) {
        const std::string& printed = text->Get<std::string>();
        if (!printed.empty()) {
            outputs.text = printed;
        }
    }
    if (const GDALAlgorithmArg* code = algorithm.GetArg("return-code");
        code != nullptr && code->GetType() == GAAT_INTEGER) {
        outputs.returnCode = code->Get<int>();
    }
    for (std::size_t i = 0; i < errors.items().size(); ++i) {
        if (!errors.items()[i].failure) {
            outputs.diagnostics.push_back(errors.items()[i]);
        }
    }
    output = DatasetRef();
    bound.algorithm.reset();
    outputs.seconds =
        std::chrono::duration<double>(std::chrono::steady_clock::now() - started).count();
    return outputs;
}

Versions versions()
{
    katana::gis::detail::ensureGdalRegistered();
    Versions result;
    result.gdal = GDALVersionInfo("RELEASE_NAME");
    // "GDAL 3.13.2 "Iowa City", released 2026/..": the name between quotes.
    const std::string full = GDALVersionInfo("--version");
    if (const std::size_t open = full.find('"'); open != std::string::npos) {
        if (const std::size_t close = full.find('"', open + 1); close != std::string::npos) {
            result.release = full.substr(open + 1, close - open - 1);
        }
    }
    int major = 0, minor = 0, patch = 0;
    OSRGetPROJVersion(&major, &minor, &patch);
    result.proj = std::to_string(major) + "." + std::to_string(minor) + "." + std::to_string(patch);
    if (OGRGetGEOSVersion(&major, &minor, &patch)) {
        result.geos =
            std::to_string(major) + "." + std::to_string(minor) + "." + std::to_string(patch);
    }
    GDALDriverManager* drivers = GetGDALDriverManager();
    for (int i = 0; i < drivers->GetDriverCount(); ++i) {
        GDALDriver* driver = drivers->GetDriver(i);
        result.rasterDrivers += driver->GetMetadataItem(GDAL_DCAP_RASTER) != nullptr ? 1 : 0;
        result.vectorDrivers += driver->GetMetadataItem(GDAL_DCAP_VECTOR) != nullptr ? 1 : 0;
    }
    for (const AlgorithmInfo& info : catalogue()) {
        result.algorithms += info.container ? 0 : 1;
    }
    return result;
}

std::size_t stagedDatasetCount()
{
    katana::gis::detail::ensureGdalRegistered();
    char** entries = VSIReadDirRecursive(kStagingRoot);
    std::size_t files = 0;
    for (char** entry = entries; entry != nullptr && *entry != nullptr; ++entry) {
        const std::string name = *entry;
        files += name.ends_with(".tif") || name.ends_with(".gpkg") ? 1u : 0u;
    }
    CSLDestroy(entries);
    return files;
}

} // namespace katana::gis::processing
