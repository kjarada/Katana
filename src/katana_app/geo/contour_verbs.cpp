// The CONTOUR verb (terrain_verbs.hpp, docs/terrain.md "Contours"): contour
// lines of a surface or an elevation raster drawn as polylines, the same on
// katana_cli, katana_mcp and the window's command line, where Terrain >
// Analysis > Contours builds these lines.
//
//   CONTOUR SURFACE <name> | RASTER <id|name> | FILE <path> interval=<m> [major=5]
//           [base=0] [layer=terrain/contours] [smooth=3|5] [<scope>] [PREVIEW]
//
// One verb, two engines, one output:
//   - a SURFACE is traced exactly on its triangles (terrain::contours, the
//     tracer the window's surfaces have had and never showed);
//   - a RASTER or FILE is contoured by GDAL's `raster contour` at its full
//     resolution, always with --elevation-name (without it GDAL writes no
//     level) and --3d, and its lines turned into the tracer's Contour.
// So which contour is major, its layer and its height are decided in one
// place (interop/geo/contour_entities): polylines on <layer>/major and
// <layer>/minor, each vertex at the level, in one undo step.
//
// A scope, last on the line, gives the closed shapes the contours are kept
// inside: both engines' lines are cut at the boundary itself. A raster is
// first cut by GDAL to the areas' box, a margin of cells wider so the cut
// lines still reach the boundary, and so only the site is contoured. Open
// shapes in the scope bound nothing: counted and said. smooth= runs GDAL's
// gaussian `raster neighbors` over a raster first (3 or 5 cells, the sizes
// GDAL has), for presentation, and the reply says smoothed=yes.

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <limits>
#include <memory>
#include <set>
#include <string>
#include <utility>
#include <variant>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/terrain/contours.hpp"
#include "replies.hpp"
#include "terrain_verbs.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kUsage =
    "CONTOUR SURFACE <name> | RASTER <id|name> | FILE <path> interval=<m> [major=5] [base=0] "
    "[layer=terrain/contours] [smooth=3|5] [<scope>: the closed shapes to keep them inside] "
    "[PREVIEW]";

katana::core::Error refusal(const std::string& why, const std::string& word = {})
{
    return makeError(ErrorCode::InvalidArgument, why, word);
}

struct ContourWords {
    std::optional<double> interval;
    std::size_t majorEvery = 5;
    double base = 0.0;
    std::string layer = "terrain/contours";
    int smooth = 0;
    std::optional<katana::cad::ScopeWords> scope;
    bool preview = false;
};

Result<ContourWords> contourWords(const Tokens& tokens, std::size_t at)
{
    ContourWords words;
    const std::string takes =
        "CONTOUR takes interval=, major=, base=, layer=, smooth=, a scope and PREVIEW";
    while (at < tokens.size()) {
        if (tokens.is(at, "PREVIEW")) {
            words.preview = true;
            ++at;
        } else if (const auto option = keyValue(tokens, at)) {
            const auto& [key, text] = *option;
            if (key == "interval") {
                auto interval = positiveOption("interval", text);
                if (!interval) {
                    return interval.error();
                }
                words.interval = *interval;
            } else if (key == "major") {
                const auto every = katana::core::parseInteger(text);
                if (!every || *every < 0) {
                    return refusal("major is every how many contours is a major one, 0 for none",
                                   text);
                }
                words.majorEvery = static_cast<std::size_t>(*every);
            } else if (key == "base") {
                auto base = numberOption("base", text);
                if (!base) {
                    return base.error();
                }
                words.base = *base;
            } else if (key == "layer") {
                if (auto valid = katana::entity::validateLayerPath(text); !valid) {
                    return refusal("layer is a layer path: " + valid.error().message, text);
                }
                words.layer = text;
            } else if (key == "smooth") {
                const auto size = katana::core::parseInteger(text);
                if (!size || (*size != 0 && *size != 3 && *size != 5)) {
                    // GDAL's gaussian kernel has these two sizes.
                    return refusal("smooth is 3 or 5 cells (GDAL's gaussian kernels), or 0 for none",
                                   text);
                }
                words.smooth = static_cast<int>(*size);
            } else {
                return refusal(takes, tokens[at]);
            }
            ++at;
        } else if (!tokens.quoted[at] && katana::cad::isScopeWord(tokens[at])) {
            if (words.scope) {
                return refusal("CONTOUR takes one scope", tokens[at]);
            }
            auto scope = katana::cad::parseScopeWords(tokens.words, at);
            if (!scope) {
                return scope.error();
            }
            words.scope = std::move(scope).value();
        } else {
            return refusal(takes, tokens[at]);
        }
    }
    if (!words.interval) {
        return refusal("CONTOUR needs interval=<m>, the height between contours");
    }
    return words;
}

// The contours of the report: `contours method=tin|grid cell= levels= count=
// major= minor= layer= smoothed=`.
std::string contoursRecord(const std::string& method, const std::optional<double>& cell,
                           const igeo::ContourPlan& plan, const std::string& layer, bool smoothed)
{
    return "contours method=" + method +
           " cell=" + (cell ? katana::core::formatExactReal(*cell) : std::string()) +
           " levels=" + std::to_string(plan.levels) + " count=" + std::to_string(plan.created) +
           " major=" + std::to_string(plan.major) + " minor=" + std::to_string(plan.minor) +
           " layer=" + value(layer) + " smoothed=" + (smoothed ? "yes" : "no");
}

// How many levels base + k * interval lie in [low, high].
double levelsBetween(double low, double high, double interval, double base)
{
    return std::floor((high - base) / interval) - std::ceil((low - base) / interval) + 1.0;
}

// The least and greatest values of a raster, on a stride that reads about a
// million of its cells: what the level count is judged by before GDAL is
// asked for millions of lines. A sample's range is within the whole's, so a
// count over it that is already too many is certainly too many.
std::optional<std::pair<double, double>> sampledRange(const gp::DatasetValue& dataset)
{
    const auto* file = std::get_if<gp::DatasetPath>(&dataset);
    if (file == nullptr) {
        return std::nullopt;
    }
    auto opened = katana::gis::GdalDataset::open(
        std::filesystem::path(std::u8string(file->path.begin(), file->path.end())));
    if (!opened) {
        return std::nullopt;
    }
    auto info = (*opened)->rasterInfo();
    if (!info) {
        return std::nullopt;
    }
    const double cells = static_cast<double>(info->width) * static_cast<double>(info->height);
    const int stride = std::max(1, static_cast<int>(std::ceil(std::sqrt(cells / 1.0e6))));
    auto samples = (*opened)->readBandSampled(1, stride);
    if (!samples) {
        return std::nullopt;
    }
    double low = std::numeric_limits<double>::infinity();
    double high = -low;
    for (const double sample : samples->values) {
        if (!std::isfinite(sample) || (samples->noDataValue && sample == *samples->noDataValue)) {
            continue;
        }
        low = std::min(low, sample);
        high = std::max(high, sample);
    }
    if (!(low <= high)) {
        return std::nullopt;
    }
    return std::pair{low, high};
}

katana::core::Error tooManyLevels(double levels)
{
    return makeError(ErrorCode::InvalidArgument,
                     "contour interval is too small for the relief of the raster",
                     "levels=" + katana::core::formatExactReal(levels) +
                         " limit=" + std::to_string(katana::terrain::kMaxContourLevels));
}

// What the work hands the apply: the contours, and how they were made.
struct Traced {
    std::vector<katana::terrain::Contour> contours;
    std::string method;
    std::optional<double> cell;
    std::vector<std::string> warnings;
};

Result<Traced> traceSurface(const katana::terrain::TinSurface& surface, const ContourWords& words,
                            const std::vector<igeo::ContourBoundary>& areas,
                            const std::stop_token& stop)
{
    auto traced = katana::terrain::contours(surface, *words.interval, words.base, words.majorEvery);
    if (!traced) {
        return traced.error();
    }
    if (stop.stop_requested()) {
        return makeError(ErrorCode::InvalidState, "cancelled");
    }
    Traced result;
    result.method = "tin";
    result.contours = words.scope ? igeo::clipContours(*traced, areas) : std::move(traced).value();
    return result;
}

Result<Traced> traceRaster(const gp::DatasetValue& input, const ContourWords& words,
                           const std::vector<igeo::ContourBoundary>& areas,
                           const katana::geometry::Box2& box, const std::filesystem::path& scratch,
                           const std::stop_token& stop, const Progress& progress)
{
    RasterChain chain(input, scratch);
    auto facts = chain.info();
    if (!facts) {
        return facts.error();
    }
    Traced result;
    result.method = "grid";
    result.cell = squareCellOf(*facts);
    if (words.scope) {
        // Cut to the areas' box, a margin wider: a contour is traced up to
        // the last cell centre, and the smoothing kernel reads its half-width
        // beyond, so the lines still reach the boundary where they are cut.
        // Within the raster: past its edge GDAL would fill the box with 0
        // where the raster has no no-data value, and contour the cliff.
        const auto& gt = facts->geotransform;
        const double cell = std::max(std::hypot(gt[1], gt[4]), std::hypot(gt[2], gt[5]));
        const katana::geometry::Box2 wanted = box.inflated(cell * (1.0 + words.smooth / 2));
        const katana::geometry::Box2 extent = rasterExtent(*facts);
        const katana::geometry::Box2 cut(
            {std::max(wanted.min.x, extent.min.x), std::max(wanted.min.y, extent.min.y)},
            {std::min(wanted.max.x, extent.max.x), std::min(wanted.max.y, extent.max.y)});
        if (cut.empty() || cut.width() < cell || cut.height() < cell) {
            return result; // the areas lie off the raster: no contour there
        }
        auto clipped =
            chain.step({"raster", "clip"},
                       {"--bbox=" + gdalNumber(cut.min.x) + "," + gdalNumber(cut.min.y) + "," +
                        gdalNumber(cut.max.x) + "," + gdalNumber(cut.max.y)},
                       stop, progress);
        if (!clipped) {
            return clipped.error();
        }
    }
    if (words.smooth != 0) {
        auto smoothed = chain.step({"raster", "neighbors"},
                                   {"--kernel=gaussian", "--size=" + std::to_string(words.smooth)},
                                   stop, progress);
        if (!smoothed) {
            return smoothed.error();
        }
    }
    if (const auto range = sampledRange(chain.current())) {
        const double levels =
            levelsBetween(range->first, range->second, *words.interval, words.base);
        if (levels > static_cast<double>(katana::terrain::kMaxContourLevels)) {
            return tooManyLevels(levels);
        }
    }
    std::vector<std::string> tokens{"--interval=" + gdalNumber(*words.interval),
                                    "--elevation-name=" + std::string(igeo::kContourElevationField),
                                    "--3d"};
    if (words.base != 0.0) {
        tokens.push_back("--offset=" + gdalNumber(words.base));
    }
    auto features = chain.features({"raster", "contour"}, std::move(tokens), stop, progress);
    if (!features) {
        return features.error();
    }
    auto contours = igeo::contoursFromFeatures(*features, igeo::kContourElevationField,
                                               *words.interval, words.base, words.majorEvery);
    if (!contours) {
        return contours.error();
    }
    result.contours = words.scope ? igeo::clipContours(*contours, areas) : std::move(contours).value();
    for (const gp::Diagnostic& warning : chain.warnings) {
        result.warnings.push_back(warning.message);
    }
    return result;
}

} // namespace

Result<Prepared> prepareContour(Context& context, const Tokens& tokens, std::string_view line)
{
    std::size_t at = 1;
    auto source = bindTerrainSource(context, tokens, at, "CONTOUR");
    if (!source) {
        return source.error();
    }
    auto words = contourWords(tokens, at);
    if (!words) {
        return words.error();
    }
    const bool tin = source->source.kind == Source::Kind::Surface;
    std::vector<std::string> records;
    if (tin) {
        // Traced on the triangles: no cell is read, whatever CELL said.
        if (source->cellGiven) {
            return refusal("a surface is contoured exactly on its triangles; CELL is for a "
                           "surface sampled into a grid",
                           source->source.surface);
        }
        if (words->smooth != 0) {
            return refusal("smooth= is a raster's; a surface is contoured exactly on its "
                           "triangles");
        }
        records.push_back("input arg=input source=surface name=" + value(source->source.surface));
    } else {
        records.push_back(source->record);
    }

    ScopeAreas areas;
    if (words->scope) {
        auto bound = bindAreas(context, *words->scope, "boundary");
        if (!bound) {
            return bound.error();
        }
        areas = std::move(bound).value();
        records.insert(records.end(), areas.records.begin(), areas.records.end());
    }
    const bool nothingInside = words->scope && areas.areas.empty();
    if (words->preview || nothingInside) {
        // A scope with no closed shape keeps no contour: said, not refused.
        records.push_back(contoursRecord(tin ? "tin" : "grid", std::nullopt, igeo::ContourPlan{},
                                         words->layer, words->smooth != 0));
        records.push_back(words->preview ? "preview valid=yes changed=no"
                                         : "output arg=output kind=vector target=layer layer=" +
                                               value(words->layer) + " created=0");
        return answeredWith("CONTOUR", joinedRecords(records));
    }

    Prepared prepared;
    prepared.title = "Contours";
    const ContourWords options = *words;
    const std::string layer = words->layer;
    const std::string command(katana::core::trimmed(line));
    auto shared = std::make_shared<const ScopeAreas>(std::move(areas));
    std::shared_ptr<const katana::terrain::TinSurface> surface = source->surface;
    DeferredDataset dataset = source->dataset;
    const std::filesystem::path scratch = context.scratch;
    prepared.work = [surface, dataset, options, shared, scratch, records, layer,
                     command](const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        Result<Traced> traced = makeError(ErrorCode::Internal, "not traced");
        if (surface) {
            traced = traceSurface(*surface, options, shared->areas, stop);
        } else {
            auto input = dataset();
            if (!input) {
                return input.error();
            }
            traced = traceRaster(*input, options, shared->areas, shared->box, scratch, stop,
                                 progress);
        }
        if (!traced) {
            return traced.error();
        }
        auto kept = std::make_shared<Traced>(std::move(traced).value());
        return Apply([kept, records, layer, command, smoothed = options.smooth != 0](
                         Context& ctx) -> Result<std::string> {
            igeo::ContourEntityOptions entityOptions;
            entityOptions.layer = layer;
            entityOptions.commandName = command;
            auto plan = igeo::contourCommand(ctx.document.model(), kept->contours, entityOptions);
            if (!plan) {
                return plan.error();
            }
            std::vector<katana::entity::EntityId> created;
            if (plan->command) {
                if (auto status = ctx.document.execute(std::move(plan->command)); !status) {
                    return status.error();
                }
                created = ctx.document.lastCreatedEntities();
            }
            std::vector<std::string> reply = records;
            reply.push_back(contoursRecord(kept->method, kept->cell, *plan, layer, smoothed));
            reply.push_back("output arg=output kind=vector target=layer layer=" + value(layer) +
                            " created=" + std::to_string(plan->created));
            for (const std::string& warning : kept->warnings) {
                reply.push_back(warningRecord(warning));
            }
            if (ctx.frame && !created.empty()) {
                katana::geometry::Box2 box;
                for (const katana::entity::EntityId id : created) {
                    if (const katana::entity::Entity* entity = ctx.document.model().entities.find(id)) {
                        box.expand(katana::entity::boundingBox(entity->geometry));
                    }
                }
                ctx.frame(box);
            }
            return joinedRecords(reply);
        });
    };
    return prepared;
}

std::string contourUsage()
{
    return kUsage;
}

} // namespace katana::app::geo
