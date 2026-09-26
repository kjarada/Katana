// Which GDAL algorithms may run without asking, and which words of a
// command-line tail are refused (docs/geoprocessing.md, "Safety").
//
// An algorithm that changes or removes EXISTING data in place - a file
// deleted, renamed, repacked, its overviews rebuilt, its pixels edited - is
// Confirm: the line must say CONFIRM, the toolbox's Confirm box must be
// ticked, an MCP call must pass confirm:true. Everything that only reads, or
// writes a new output the line names, is Safe; replacing an existing output is
// OVERWRITE's business, not the policy's.
//
// Two sources decide, and the stricter wins:
//   - the table below, one entry per leaf GDAL 3.13 has, judged by hand;
//   - a rule: an input dataset GDAL opens for update makes an algorithm
//     Confirm whatever the table says, so a leaf that gains such an input in a
//     later GDAL is caught without anyone reading its release notes.
// A leaf in neither - one a later GDAL adds - is Confirm until someone judges
// it: "unclassified", which the contract test lists by name.

#include <algorithm>
#include <array>
#include <cctype>
#include <string>
#include <string_view>
#include <vector>

#include "gdal_run_detail.hpp"
#include "katana/core/error.hpp"
#include "katana/core/text.hpp"

namespace katana::gis::processing {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

// Leaves that change or remove what already exists.
constexpr std::array<std::string_view, 18> kConfirm{
    "dataset copy",      "dataset delete",       "dataset rename",
    "driver gpkg repack", "driver openfilegdb repack",
    // Writes _metadata INTO the partitioned dataset it reads.
    "driver parquet create-metadata-file",
    "raster edit",       "raster overview add",  "raster overview delete",
    "raster overview refresh", "raster update", "vector update",
    "vsi copy",          "vsi delete",           "vsi move",
    "vsi sozip create",  "vsi sozip optimize",   "vsi sync",
};

// Leaves that read, or write only a new output the line names.
constexpr std::array<std::string_view, 103> kSafe{
    "convert",
    "dataset check",
    "dataset identify",
    "driver cog validate",
    "driver gpkg validate",
    "driver gti create",
    "driver pdf list-layers",
    "driver rpftoc create",
    "info",
    "mdim convert",
    "mdim info",
    "mdim mosaic",
    "pipeline",
    "raster as-features",
    "raster aspect",
    "raster blend",
    "raster calc",
    "raster clean-collar",
    "raster clip",
    "raster color-map",
    "raster compare",
    "raster contour",
    "raster convert",
    "raster create",
    "raster fill-nodata",
    "raster footprint",
    "raster hillshade",
    "raster index",
    "raster info",
    "raster mosaic",
    "raster neighbors",
    "raster nodata-to-alpha",
    "raster pansharpen",
    "raster pipeline",
    "raster pixel-info",
    "raster polygonize",
    "raster proximity",
    "raster reclassify",
    "raster reproject",
    "raster resize",
    "raster rgb-to-palette",
    "raster roughness",
    "raster scale",
    "raster select",
    "raster set-type",
    "raster sieve",
    "raster slope",
    "raster stack",
    "raster tile",
    "raster tpi",
    "raster tri",
    "raster unscale",
    "raster viewshed",
    "raster zonal-stats",
    "vector buffer",
    "vector check-coverage",
    "vector check-geometry",
    "vector clean-coverage",
    "vector clip",
    "vector combine",
    "vector concat",
    "vector concave-hull",
    "vector convert",
    "vector convex-hull",
    "vector create",
    "vector dissolve",
    "vector edit",
    "vector explode-collections",
    "vector export-schema",
    "vector filter",
    "vector grid average",
    "vector grid average-distance",
    "vector grid average-distance-points",
    "vector grid count",
    "vector grid invdist",
    "vector grid invdistnn",
    "vector grid linear",
    "vector grid maximum",
    "vector grid minimum",
    "vector grid nearest",
    "vector grid range",
    "vector index",
    "vector info",
    "vector layer-algebra",
    "vector make-point",
    "vector make-valid",
    "vector partition",
    "vector pipeline",
    "vector rasterize",
    "vector rename-layer",
    "vector reproject",
    "vector segmentize",
    "vector select",
    "vector set-field-type",
    "vector set-geom-type",
    "vector simplify",
    "vector simplify-coverage",
    "vector sort",
    "vector sql",
    "vector swap-xy",
    "vsi list",
    "vsi sozip list",
    "vsi sozip validate",
};

template <std::size_t N>
bool listed(const std::array<std::string_view, N>& table, std::string_view path)
{
    return std::ranges::find(table, path) != table.end();
}

// Words that would print instead of run, or reach past the algorithm:
// --config is how GDAL_ENABLE_EXTERNAL (programs run by a pipeline) and
// SPATIALITE_SECURITY (Spatialite's file functions) would be switched on.
constexpr std::array<std::string_view, 8> kRefusedWords{
    "--config", "--help", "-h", "--help-doc", "--json-usage", "--progress", "--quiet", "-q"};

} // namespace

namespace detail {

Policy classify(const std::vector<std::string>& path, const GDALAlgorithm& algorithm,
                std::string& reason)
{
    for (const auto& arg : algorithm.GetArgs()) {
        const GDALAlgorithmArgType type = arg->GetType();
        const bool dataset = type == GAAT_DATASET || type == GAAT_DATASET_LIST;
        if (dataset && arg->IsInput() && !arg->IsOutput() &&
            (arg->GetDatasetType() & GDAL_OF_UPDATE) != 0) {
            reason = "update";
            return Policy::Confirm;
        }
    }
    const std::string text = pathText(path);
    if (listed(kConfirm, text)) {
        reason = "listed";
        return Policy::Confirm;
    }
    if (listed(kSafe, text)) {
        reason = "listed";
        return Policy::Safe;
    }
    reason = "unclassified";
    return Policy::Confirm;
}

bool hasExternalStep(std::string_view pipeline)
{
    // A step begins the text, and follows each '!' and each '[' that opens a
    // nested pipeline; its first word is its name.
    bool stepStart = true;
    std::size_t i = 0;
    while (i < pipeline.size()) {
        const char c = pipeline[i];
        if (c == '!' || c == '[') {
            stepStart = true;
            ++i;
            continue;
        }
        if (std::isspace(static_cast<unsigned char>(c)) != 0 || c == ']') {
            ++i;
            continue;
        }
        const std::size_t end = pipeline.find_first_of(" \t\r\n![]", i);
        const std::string_view word =
            pipeline.substr(i, end == std::string_view::npos ? std::string_view::npos : end - i);
        if (stepStart && katana::core::lowered(word) == "external") {
            return true;
        }
        stepStart = false;
        i = end == std::string_view::npos ? pipeline.size() : end;
    }
    return false;
}

bool isPipeline(const std::vector<std::string>& path)
{
    return !path.empty() && path.back() == "pipeline";
}

} // namespace detail

Status checkTokens(const std::vector<std::string>& path, const std::vector<std::string>& tokens)
{
    for (const std::string& token : tokens) {
        const std::string word = katana::core::lowered(token.substr(0, token.find('=')));
        if (std::ranges::find(kRefusedWords, word) != kRefusedWords.end()) {
            return makeError(ErrorCode::InvalidArgument,
                             word == "--config"
                                 ? "--config is refused: it could switch on what runs programs "
                                   "or reads files beyond the algorithm's own"
                                 : "is refused: it prints instead of running; GDAL HELP "
                                   "<algorithm> describes the arguments",
                             token);
        }
    }
    if (detail::isPipeline(path)) {
        std::string joined;
        for (const std::string& token : tokens) {
            joined += token + ' ';
        }
        if (detail::hasExternalStep(joined)) {
            return makeError(ErrorCode::InvalidArgument,
                             "a pipeline step 'external' runs a program and is refused");
        }
    }
    return {};
}

} // namespace katana::gis::processing
