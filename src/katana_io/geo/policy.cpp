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

// The steps GDAL 3.13's three pipelines offer (their usage's
// pipeline_algorithms), judged by hand as the leaves are. Only `update`
// writes into a dataset that already exists - "Update the destination raster
// with the content of the input one", the vector one likewise - so it is
// Confirm, as `raster update` and `vector update` are. `edit` and `overview`
// are Confirm as leaves, which change the dataset they open, but as steps
// they change the piped dataset only, which the pipeline then writes where it
// is told (a new output, or OVERWRITE's business). `external` is refused
// outright (checkTokens). A step in neither list is Confirm until someone
// judges it; the contract test lists every one GDAL offers.
constexpr std::array<std::string_view, 1> kConfirmSteps{"update"};
constexpr std::array<std::string_view, 70> kSafeSteps{
    "as-features",
    "aspect",
    "blend",
    "buffer",
    "calc",
    "check-coverage",
    "check-geometry",
    "clean-coverage",
    "clip",
    "color-map",
    "combine",
    "compare",
    "concat",
    "concave-hull",
    "contour",
    "convex-hull",
    "create",
    "dissolve",
    "edit",
    "explode-collections",
    "export-schema",
    "fill-nodata",
    "filter",
    "footprint",
    "grid",
    "hillshade",
    "info",
    "limit",
    "make-point",
    "make-valid",
    "materialize",
    "mosaic",
    "neighbors",
    "nodata-to-alpha",
    "overview",
    "pansharpen",
    "partition",
    "pixel-info",
    "polygonize",
    "proximity",
    "rasterize",
    "read",
    "reclassify",
    "rename-layer",
    "reproject",
    "resize",
    "rgb-to-palette",
    "roughness",
    "scale",
    "segmentize",
    "select",
    "set-field-type",
    "set-geom-type",
    "set-type",
    "sieve",
    "simplify",
    "simplify-coverage",
    "slope",
    "sort",
    "sql",
    "stack",
    "swap-xy",
    "tee",
    "tile",
    "tpi",
    "tri",
    "unscale",
    "viewshed",
    "write",
    "zonal-stats",
};

// Options that let GDAL change a dataset that is already there rather than
// write a new one: replace it (--overwrite, --overwrite-layer), add to it
// (--append, --upsert, --update; rasterize's --add burns into the raster
// the output names, measured: its checksum changed with no --update; tile's
// --resume writes the missing tiles into a tile set already there). They
// need OVERWRITE on the line. Found by listing every boolean argument of
// every GDAL 3.13 algorithm and step (gdal --json-usage) and reading the
// description of each whose name suggested it.
constexpr std::array<std::string_view, 7> kChangesExisting{
    "--overwrite", "--overwrite-layer", "--append", "--update", "--upsert", "--add", "--resume"};

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

std::vector<PipelineWord> pipelineWords(std::string_view pipeline)
{
    // A step begins the text, and follows each '!' and each '[' that opens a
    // nested pipeline; its first word is its name.
    std::vector<PipelineWord> words;
    bool stepStart = true;
    bool first = true;
    std::size_t i = 0;
    while (i < pipeline.size()) {
        const char c = pipeline[i];
        if (c == '!' || c == '[') {
            stepStart = true;
            first = false;
            ++i;
            continue;
        }
        if (std::isspace(static_cast<unsigned char>(c)) != 0 || c == ']') {
            ++i;
            continue;
        }
        const std::size_t end = pipeline.find_first_of(" \t\r\n![]", i);
        std::string_view word =
            pipeline.substr(i, end == std::string_view::npos ? std::string_view::npos : end - i);
        i = end == std::string_view::npos ? pipeline.size() : end;
        // The pipeline given by name, as one word: --pipeline=read x ! ...
        constexpr std::string_view named = "--pipeline=";
        if (katana::core::lowered(word.substr(0, named.size())) == named) {
            word.remove_prefix(named.size());
            if (word.empty()) {
                continue;
            }
        }
        words.push_back(
            PipelineWord{std::string(word), stepStart && !word.starts_with('-'), first});
        stepStart = stepStart && word.starts_with('-');
    }
    return words;
}

bool hasExternalStep(std::string_view pipeline)
{
    return std::ranges::any_of(pipelineWords(pipeline), [](const PipelineWord& word) {
        return word.step && katana::core::lowered(word.text) == "external";
    });
}

bool isPipeline(const std::vector<std::string>& path)
{
    return !path.empty() && path.back() == "pipeline";
}

} // namespace detail

Policy stepPolicy(std::string_view step, std::string& reason)
{
    const std::string name = katana::core::lowered(step);
    if (name == "external") {
        reason = "refused";
        return Policy::Confirm;
    }
    if (listed(kConfirmSteps, name)) {
        reason = "listed";
        return Policy::Confirm;
    }
    if (listed(kSafeSteps, name)) {
        reason = "listed";
        return Policy::Safe;
    }
    reason = "unclassified";
    return Policy::Confirm;
}

namespace {

// The tail as one text, the way GDAL's pipeline argument reads it: words
// given one by one, or quoted as one.
std::string pipelineText(const std::vector<std::string>& tokens)
{
    std::string joined;
    for (const std::string& token : tokens) {
        joined += token + ' ';
    }
    return joined;
}

bool changesExisting(std::string_view word)
{
    const std::string name = katana::core::lowered(word.substr(0, word.find('=')));
    return listed(kChangesExisting, name);
}

} // namespace

TailEffects tailEffects(const std::vector<std::string>& path,
                        const std::vector<std::string>& tokens)
{
    TailEffects effects;
    if (!detail::isPipeline(path)) {
        const auto found = std::ranges::find_if(
            tokens, [](const std::string& token) { return changesExisting(token); });
        if (found != tokens.end()) {
            effects.overwriteWord = *found;
        }
        return effects;
    }
    // Every word of a pipeline, however it was quoted: a pipeline given as
    // one quoted text is still steps and options to GDAL.
    for (const detail::PipelineWord& word : detail::pipelineWords(pipelineText(tokens))) {
        if (effects.overwriteWord.empty() && changesExisting(word.text)) {
            effects.overwriteWord = word.text;
        }
        if (!word.step || !effects.confirmStep.empty()) {
            continue;
        }
        std::string reason;
        const Policy policy = stepPolicy(word.text, reason);
        // The text's first word may be a value of an option before the
        // pipeline (--output-format GTiff read ...), which GDAL refuses by
        // itself if it is no step; only a step after '!' or '[' that no one
        // has judged is taken as one GDAL may have added.
        if (policy == Policy::Confirm && !(reason == "unclassified" && word.first)) {
            effects.confirmStep = katana::core::lowered(word.text);
        }
    }
    return effects;
}

bool readsHeights(const std::vector<std::string>& path, const std::vector<std::string>& tokens)
{
    const auto has = [](const std::vector<std::string>& words, std::string_view option) {
        return std::ranges::any_of(words, [option](const std::string& word) {
            return katana::core::lowered(word.substr(0, word.find('='))) == option;
        });
    };
    // The grids that interpolate or summarise a value read it from Z unless
    // --zfield names a field; count and the two average-distance grids read
    // positions only. Rasterize reads Z with --3d.
    const auto gridReadsZ = [&](std::string_view method, const std::vector<std::string>& words) {
        const bool positionsOnly = method == "count" || method == "average-distance" ||
                                   method == "average-distance-points";
        return !positionsOnly && !has(words, "--zfield");
    };
    if (path.size() == 3 && path[0] == "vector" && path[1] == "grid") {
        return gridReadsZ(path[2], tokens);
    }
    if (path == std::vector<std::string>{"vector", "rasterize"}) {
        return has(tokens, "--3d");
    }
    if (!detail::isPipeline(path)) {
        return false;
    }
    // A pipeline reads heights when one of its steps does: each step's words
    // are judged on their own.
    const std::vector<detail::PipelineWord> words = detail::pipelineWords(pipelineText(tokens));
    for (std::size_t i = 0; i < words.size(); ++i) {
        if (!words[i].step) {
            continue;
        }
        const std::string step = katana::core::lowered(words[i].text);
        std::vector<std::string> own;
        std::size_t j = i + 1;
        for (; j < words.size() && !words[j].step; ++j) {
            own.push_back(words[j].text);
        }
        if (step == "grid" && !own.empty() && gridReadsZ(katana::core::lowered(own.front()), own)) {
            return true;
        }
        if (step == "rasterize" && has(own, "--3d")) {
            return true;
        }
    }
    return false;
}

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
