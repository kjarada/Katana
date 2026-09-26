#pragma once

// GDAL's algorithm framework (the `gdal raster ...` / `gdal vector ...`
// command line, 3.11 onwards) behind plain types: the catalogue of what GDAL
// can do, each algorithm's arguments as data, and a run with progress and
// cancel (docs/geoprocessing.md, "The bridge").
//
// This is the ONLY code that includes gdalalgorithm.h (src/katana_io/geo/).
// Nothing here names a GDAL type, so the verbs, the MCP tools and the window
// above it are written against this header and never against GDAL's, whose
// algorithm API is still provisional ("the project reserves the right to
// modify, rename, reorganise ... until it is officially frozen").
//
// Every rule below was observed, not assumed: the probes that established
// them are listed in docs/geoprocessing.md with what each showed.
//
//   - The catalogue is walked from the root "gdal" algorithm: the registry's
//     own top-level list misses the `driver` family.
//   - A run uses a fresh algorithm instance (Run() is single-shot), and every
//     in-memory input becomes its own MEM dataset inside run(): one input
//     shared between concurrent runs gave 63 wrong results in 120.
//   - GDAL's command-line tail is parsed by GDAL itself AFTER the in-memory
//     datasets are bound, so list, CRS and choice parsing is exactly GDAL's.
//   - Run()'s return value is not trusted after a cancel (hillshade returns
//     true with a partial output): the progress adapter records that it
//     refused, and such a run is "cancelled" whatever GDAL said.
//   - Errors are collected per run by a thread-local handler. Errors raised on
//     GDAL's own worker threads skip it and reach the process-wide quiet
//     handler; the failure they cause still reaches the Result through Run().
//
// Threading: every function may be called from any thread. Concurrent runs
// share nothing but the read-only catalogue.

#include <cstddef>
#include <cstdint>
#include <functional>
#include <map>
#include <optional>
#include <stop_token>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/gis/gdal_adapter.hpp"

namespace katana::gis::processing {

// ---- what GDAL offers ------------------------------------------------------------------

enum class ArgType { Boolean, String, Integer, Real, StringList, IntegerList, RealList, Dataset, DatasetList };

// Bits of ArgSpec::datasetKinds.
enum DatasetKind : unsigned { Raster = 1, Vector = 2, Multidim = 4 };

// Safe runs without asking; Confirm changes or removes existing data in place
// (vsi delete, dataset rename, raster edit, overview add ...) and runs only
// when the line says CONFIRM (docs/geoprocessing.md, "Safety").
enum class Policy { Safe, Confirm };

// A bound of a numeric argument. GDAL's minimum for `contour --interval` is
// exclusive (0 is refused, 0.001 taken); 19 of its 82 minimums are.
struct Bound {
    double value = 0.0;
    bool inclusive = true;
};

using Scalar = std::variant<bool, std::string, int, double, std::vector<std::string>,
                            std::vector<int>, std::vector<double>>;

// One argument of an algorithm, as GDAL declares it. Arguments GDAL hides from
// its API (help, json-usage, config, progress ...) are not listed.
struct ArgSpec {
    std::string name, shortName, description, metaVar, category; // Base|Advanced|Esoteric|Common|...
    std::vector<std::string> aliases;                             // public only
    ArgType type = ArgType::String;
    bool required = false, positional = false, isInput = true, isOutput = false;
    int minCount = 0, maxCount = 1; // -1 = unbounded
    bool packedValues = false, repeatable = false, inPipelineStep = true;
    std::optional<Scalar> defaultValue;
    std::vector<std::string> choices;
    // Absent when GDAL's bound is NaN, which is how it says "no bound": absent
    // is not zero.
    std::optional<Bound> min, max;
    unsigned datasetKinds = 0;
    // The dataset is opened for update: the algorithm changes it in place.
    bool datasetUpdate = false;
    // How a dataset argument may be given: by name, as an open dataset, or
    // both (GDAL's GADV_NAME and GADV_OBJECT).
    bool acceptsName = false, acceptsObject = false;
    std::string exclusionGroup, dependencyGroup;
    std::vector<std::string> dependsOn;
    std::map<std::string, std::vector<std::string>> metadata; // required_capabilities, ...

    [[nodiscard]] bool isDataset() const
    {
        return type == ArgType::Dataset || type == ArgType::DatasetList;
    }
};

struct AlgorithmInfo {
    std::vector<std::string> path; // {"raster", "hillshade"}, without the root "gdal"
    std::vector<std::string> aliases; // public only
    std::string description, helpUrl;
    bool container = false;
    Policy policy = Policy::Confirm;
    // Why the policy is what it is: "listed" (the curated table), "update"
    // (an input is opened for update) or "unclassified" (no one has judged
    // it; Confirm until someone does). Containers are "container".
    std::string policyReason;
};

struct AlgorithmSpec {
    AlgorithmInfo info;
    std::string longDescription;
    std::vector<ArgSpec> args;
    std::string usageJson; // GDAL's --json-usage, verbatim
};

// ---- data handed to and taken from an algorithm ------------------------------------------

using FieldValue = std::variant<std::monostate, bool, std::int64_t, double, std::string>;
enum class FieldType { Boolean, Integer64, Real, String, Date, DateTime }; // dates as ISO 8601 text

struct FieldDef {
    std::string name;
    FieldType type = FieldType::String;
};

struct Feature {
    // The members of a multi-geometry; one for a simple geometry.
    std::vector<VectorGeometry> parts;
    std::vector<FieldValue> values; // parallel to FeatureTable::fields
};

struct FeatureTable {
    std::string name;
    GeometryKind kind = GeometryKind::Unknown;
    bool hasZ = false;
    std::string crsWkt;
    std::vector<FieldDef> fields;
    std::vector<Feature> features;
};

struct FeatureSet {
    std::vector<FeatureTable> tables;
};

struct RasterGrid {
    RasterInfo info;
    std::string dataType = "Float64"; // GDAL's name: Byte, Int16, Float32, Float64 ...
    std::vector<std::vector<double>> bands; // row major, top row first
    std::vector<std::optional<double>> noData; // per band
};

// A dataset GDAL opens itself: a file, a /vsi path or a URL.
struct DatasetPath {
    std::string path;
    std::vector<std::string> openOptions;
    std::string layer;
};

using DatasetValue = std::variant<DatasetPath, RasterGrid, FeatureSet>;
using ArgValue = std::variant<Scalar, DatasetValue, std::vector<DatasetValue>>;

enum class OutputTo { Memory, File };

struct RunRequest {
    std::vector<std::string> path; // {"raster","hillshade"}, {"pipeline"}
    // By long name, short name or alias, set before the tail is parsed.
    std::vector<std::pair<std::string, ArgValue>> values;
    // GDAL's own command-line tail, parsed by GDAL after `values` are bound.
    // An output the tail names ("hillshade dem.tif shade.tif") is written
    // there, whatever `outputTo` says.
    std::vector<std::string> tokens;
    OutputTo outputTo = OutputTo::Memory;
    std::string outputPath, outputFormat; // File: an empty format is chosen from the extension
    std::vector<std::string> creationOptions; // KEY=VALUE, File only
    bool overwrite = false;
    // A raster output with more cells (width x height x bands) goes to a
    // tiled GeoTIFF in spillDirectory instead of into memory; RunOutputs::file
    // names it. 0 spills every raster: how a derived reference raster is
    // written without passing through a std::vector.
    std::uint64_t maxMemoryCells = 64ull << 20;
    std::string spillDirectory;
    // Thread-local configuration options, allow-listed (GDAL_NUM_THREADS,
    // GDAL_CACHEMAX). They do not reach threads GDAL starts itself, so
    // nothing that must be right may depend on them.
    std::vector<std::pair<std::string, std::string>> config;
};

struct Diagnostic {
    bool failure = false;
    int code = 0; // GDAL's CPLE_ number
    std::string message;
};

struct RunOutputs {
    std::optional<RasterGrid> raster;
    std::optional<FeatureSet> features;
    std::optional<std::string> file, text;
    std::optional<int> returnCode;
    std::vector<Diagnostic> diagnostics; // GDAL's warnings; never a failure on success
    double seconds = 0.0;
};

struct Versions {
    std::string gdal, release, proj, geos;
    int rasterDrivers = 0, vectorDrivers = 0, algorithms = 0;
};

// ---- the bridge ----------------------------------------------------------------------------

// Every algorithm and group under the root "gdal", depth first in GDAL's
// order, walked once and kept.
[[nodiscard]] const std::vector<AlgorithmInfo>& catalogue();

// The algorithm `words` begins with: the longest run of words that names one,
// each word matched case-insensitively against a name or an alias, public or
// hidden ("raster warp" is "raster reproject"). `consumed` is set to how many
// words it took. NotFound naming the word no algorithm has; InvalidArgument
// for a group ("raster"), listing what is in it.
[[nodiscard]] katana::core::Result<std::vector<std::string>>
resolve(const std::vector<std::string>& words, std::size_t& consumed);

// An algorithm's arguments as data, and GDAL's own usage JSON. NotFound for a
// path that names nothing. A group has no arguments.
[[nodiscard]] katana::core::Result<AlgorithmSpec> describe(const std::vector<std::string>& path);

// The values GDAL offers for `arg` beginning with `prefix`: output formats
// that can do what the algorithm needs, CRS codes, ... Empty when it offers
// none or the argument does not exist.
[[nodiscard]] std::vector<std::string> suggest(const std::vector<std::string>& path,
                                               const std::string& arg, const std::string& prefix);

// Which arguments GDAL's command-line tail gives a value to, by long name,
// when `bound` (long names) are already set: options by name, and positional
// words in GDAL's order - an argument already bound takes none. Read from the
// declarations alone; nothing is opened. What the executor asks to know
// whether a tail names the output, or a dataset FROM also binds.
[[nodiscard]] katana::core::Result<std::vector<std::string>>
argumentsGiven(const std::vector<std::string>& path, const std::vector<std::string>& tokens,
               const std::vector<std::string>& bound);

// The words of a tail refused whatever the algorithm: --config (it could
// switch on GDAL_ENABLE_EXTERNAL or SPATIALITE_SECURITY), --help, --help-doc,
// --json-usage, --progress and --quiet, which would print instead of run, and
// a pipeline step `external`, which runs a program. InvalidArgument naming it.
[[nodiscard]] katana::core::Status checkTokens(const std::vector<std::string>& path,
                                               const std::vector<std::string>& tokens);

// Binds the request and asks GDAL to validate it, running nothing: required
// arguments, exclusions, dependencies, values. Datasets given by name are
// opened to be checked. Fails as run() does before it runs.
[[nodiscard]] katana::core::Status validate(const RunRequest& request);

// Runs one algorithm. Fails with
//   InvalidState "cancelled"   the stop was requested, before or during;
//   InvalidArgument            the request (an unknown argument, by name; a
//                              value GDAL refused, with its message; a
//                              missing or conflicting argument);
//   NotFound                   no algorithm has that path;
//   FileImportFailure          GDAL could not open an input;
//   FileExportFailure          GDAL could not write the output;
//   CommandRejected            GDAL ran and failed: its first failure message.
// `progress` is called with the fraction done, possibly from a GDAL thread.
[[nodiscard]] katana::core::Result<RunOutputs>
run(const RunRequest& request, const std::stop_token& stop = {},
    const std::function<void(double)>& progress = {});

[[nodiscard]] Versions versions();

// ---- words --------------------------------------------------------------------------------

// "raster hillshade".
[[nodiscard]] std::string pathText(const std::vector<std::string>& path);
// "boolean", "string", "integer", "real", "string_list", "integer_list",
// "real_list", "dataset", "dataset_list": GDAL's own words.
[[nodiscard]] std::string_view toString(ArgType type);
// "safe", "confirm".
[[nodiscard]] std::string_view toString(Policy policy);
// "raster", "vector", "multidim", joined with ','.
[[nodiscard]] std::string datasetKindsText(unsigned kinds);
// A value as GDAL's command line would write it: true, 2, 0.5, a,b,c.
[[nodiscard]] std::string toString(const Scalar& value);

// How many datasets run() has staged in memory under /vsimem/katana and not
// yet removed: 0 whenever no run is under way. For the tests, which prove the
// staging leaves nothing behind.
[[nodiscard]] std::size_t stagedDatasetCount();

} // namespace katana::gis::processing
