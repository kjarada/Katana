// The RASTER VIEWSHED and LOS verbs (docs/terrain.md, "Viewshed and line of
// sight"): what can be seen from observers over a surface or an elevation
// raster, kept as a tinted derived reference raster with the visible area
// drawn and measured if asked; and one sight line between two points - the
// same on katana_cli, katana_mcp and the window's command line, where
// Terrain > Analysis > Viewshed and Line of Sight builds these lines.
//
//   RASTER VIEWSHED SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
//                   (OBSERVER x,y)... | OBSERVERS [<scope>] [height=1.7] [target=0]
//                   [max=<m>] [curvature=<k>|none] [areas=<layer>] [NAME <n>] [PREVIEW]
//   LOS SURFACE <name> | RASTER <id|name> | FILE <path> OBSERVER x,y TARGET x,y
//       [height=1.7] [target=0] [curvature=<k>|none] [step=<m>]
//       [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]
//
// GDAL computes a viewshed (`raster viewshed`, from one --position): each
// observer is its own run, and several are unioned here, cell by cell on
// the raster's grid - GDAL's cumulative mode refuses a position (the
// investigators' finding), and a count of observers is not the union a
// person asks for. `height=` is the eye above the ground, `target=` the
// height above the ground a cell counts as seen at, `max=` the distance
// beyond which nothing is computed, and `curvature=` GDAL's curvature and
// refraction coefficient (0.85714 unless given; `none` is 0). The kept
// raster holds 1 where some observer sees the cell and 0 elsewhere; its
// display copy is tinted, the unseen cells clear. `areas=<layer>` draws the
// visible area as closed polylines (GDAL's `raster polygonize`), one undo
// step, and the reply measures it: visible_cells= and area=.
//
// A sight line is native (terrain::lineOfSight): no algorithm of GDAL's
// answers one line with its clearance. The ground is a surface on its own
// triangles, or a raster's file through GDAL's interpolation.

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <limits>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <utility>
#include <variant>
#include <vector>

#include "analysis_support.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/entity/model.hpp"
#include "katana/interop/import.hpp"
#include "katana/terrain/line_of_sight.hpp"
#include "replies.hpp"
#include "terrain_verbs.hpp"
#include "vector_support.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
namespace vec = katana::app::geo::vector;
namespace an = katana::app::geo::analysis;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;

namespace {

using Clock = std::chrono::steady_clock;

constexpr const char* kViewshedUsage =
    "RASTER VIEWSHED SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path> (OBSERVER x,y)... "
    "| OBSERVERS [<scope>] [height=1.7] [target=0] [max=<m>] [curvature=<k>|none] "
    "[areas=<layer>] [NAME <n>] [PREVIEW]";
constexpr const char* kLosUsage =
    "LOS SURFACE <name> | RASTER <id|name> | FILE <path> OBSERVER x,y TARGET x,y [height=1.7] "
    "[target=0] [curvature=<k>|none] [step=<m>] [method=bilinear|nearest|cubic|cubicspline] "
    "[PREVIEW]";

// An eye height: a person standing, the height a sight line is usually
// asked from; any other is height=.
constexpr double kEyeHeight = 1.7;
// Each observer is a whole GDAL run over the raster: a hundred is already
// minutes on a large DEM, and more is a scope that took the wrong layer.
constexpr std::size_t kMaxObservers = 100;
// What GDAL writes for a visible cell (its --visible-value default, set
// explicitly so a changed default cannot change the union).
constexpr double kVisibleValue = 255.0;

katana::core::Error refusal(const std::string& why, const std::string& word = {})
{
    return makeError(ErrorCode::InvalidArgument, why, word);
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

// The options both verbs share.
struct SightWords {
    double height = kEyeHeight;
    double target = 0.0;
    double curvature = katana::terrain::kDefaultCurvature;
};

Result<SightWords> sightWords(const vec::VerbWords& words)
{
    SightWords sight;
    auto height = vec::numberOption(words, "height");
    if (!height) {
        return height.error();
    }
    sight.height = height->value_or(kEyeHeight);
    auto target = vec::numberOption(words, "target");
    if (!target) {
        return target.error();
    }
    sight.target = target->value_or(0.0);
    if (sight.height < 0.0 || sight.target < 0.0) {
        return refusal("height= and target= are heights above the ground, 0 or more");
    }
    if (const std::string* curvature = words.option("curvature")) {
        if (katana::core::equalsIgnoringCase(*curvature, "none")) {
            sight.curvature = 0.0;
        } else {
            const auto value = katana::core::parseFiniteDouble(*curvature);
            if (!value || *value < 0.0) {
                return refusal("curvature is a coefficient of 0 or more, or none",
                               "curvature=" + *curvature);
            }
            sight.curvature = *value;
        }
    }
    return sight;
}

// A station of a sight line: a place computed along it, so to the
// millimetre like the distances beside it (fixed3), where a point the user
// gave is echoed exactly (pointText). At full precision a station read
// 149.43965517241378,119.50431034482759.
std::string stationText(const Point2& point)
{
    return fixed3(point.x) + "," + fixed3(point.y);
}

struct Observer {
    Point2 at;
    std::optional<katana::entity::EntityId> entity;
};

// One observer's run, read back: its grid and which cells it sees.
struct Seen {
    std::array<double, 6> geotransform{};
    int width = 0, height = 0;
    std::vector<std::uint8_t> visible; // row major, 1 seen
    std::size_t cells = 0;             // seen
};

Result<Seen> readSeen(const gp::RunOutputs& outputs)
{
    Seen seen;
    std::vector<double> values;
    if (outputs.raster) {
        seen.geotransform = outputs.raster->info.geotransform;
        seen.width = outputs.raster->info.width;
        seen.height = outputs.raster->info.height;
        if (!outputs.raster->bands.empty()) {
            values = outputs.raster->bands.front();
        }
    } else if (outputs.file) {
        auto opened = katana::gis::GdalDataset::open(pathOf(*outputs.file));
        if (!opened) {
            return opened.error();
        }
        auto info = (*opened)->rasterInfo();
        if (!info) {
            return info.error();
        }
        seen.geotransform = info->geotransform;
        seen.width = info->width;
        seen.height = info->height;
        auto band = (*opened)->readBand(1);
        if (!band) {
            return band.error();
        }
        values = std::move(band).value();
    } else {
        return makeError(ErrorCode::CommandRejected, "raster viewshed wrote no raster");
    }
    seen.visible.resize(values.size());
    for (std::size_t i = 0; i < values.size(); ++i) {
        seen.visible[i] = values[i] == kVisibleValue ? 1U : 0U;
        seen.cells += seen.visible[i];
    }
    return seen;
}

// The runs unioned on one grid: every run's grid is a window of the input's
// (GDAL clamps it to max=), so each lies at a whole number of cells from
// the first; the union covers them all.
gp::RasterGrid unionOf(const std::vector<Seen>& runs, const std::string& crs)
{
    const auto& base = runs.front().geotransform;
    long long left = std::numeric_limits<long long>::max();
    long long top = left;
    long long right = std::numeric_limits<long long>::min();
    long long bottom = right;
    std::vector<std::pair<long long, long long>> offsets;
    for (const Seen& run : runs) {
        const long long column = std::llround((run.geotransform[0] - base[0]) / base[1]);
        const long long row = std::llround((run.geotransform[3] - base[3]) / base[5]);
        offsets.emplace_back(column, row);
        left = std::min(left, column);
        top = std::min(top, row);
        right = std::max(right, column + run.width);
        bottom = std::max(bottom, row + run.height);
    }
    gp::RasterGrid grid;
    grid.info.width = static_cast<int>(right - left);
    grid.info.height = static_cast<int>(bottom - top);
    grid.info.bandCount = 1;
    grid.info.hasGeotransform = true;
    grid.info.projectionWkt = crs;
    grid.info.geotransform = base;
    grid.info.geotransform[0] = base[0] + static_cast<double>(left) * base[1] +
                                static_cast<double>(top) * base[2];
    grid.info.geotransform[3] = base[3] + static_cast<double>(left) * base[4] +
                                static_cast<double>(top) * base[5];
    // Int16: a type every GDAL names alike, which polygonize reads as whole
    // numbers.
    grid.dataType = "Int16";
    std::vector<double> band(static_cast<std::size_t>(grid.info.width) *
                                 static_cast<std::size_t>(grid.info.height),
                             0.0);
    for (std::size_t r = 0; r < runs.size(); ++r) {
        const Seen& run = runs[r];
        const long long column0 = offsets[r].first - left;
        const long long row0 = offsets[r].second - top;
        for (int y = 0; y < run.height; ++y) {
            for (int x = 0; x < run.width; ++x) {
                if (run.visible[static_cast<std::size_t>(y) * static_cast<std::size_t>(run.width) +
                                static_cast<std::size_t>(x)] == 0U) {
                    continue;
                }
                const auto at = static_cast<std::size_t>(row0 + y) *
                                    static_cast<std::size_t>(grid.info.width) +
                                static_cast<std::size_t>(column0 + x);
                band[at] = 1.0;
            }
        }
    }
    grid.bands.push_back(std::move(band));
    grid.noData.emplace_back(std::nullopt);
    return grid;
}

// What the viewshed's work hands its apply.
struct Viewed {
    gp::RasterGrid grid;
    std::filesystem::path picture;
    std::optional<gp::FeatureSet> areas;
    std::vector<std::string> observerRecords;
    std::size_t visibleCells = 0;
    double visibleArea = 0.0;
    std::vector<std::string> warnings;
    double seconds = 0.0;
};

} // namespace

// ---- RASTER VIEWSHED ------------------------------------------------------------------------

Result<Prepared> prepareRasterViewshed(Context& context, const Tokens& tokens, std::string_view line)
{
    std::size_t at = 2;
    auto source = bindTerrainSource(context, tokens, at, "RASTER VIEWSHED");
    if (!source) {
        return source.error();
    }
    auto split = an::takeKeywordValues(tokens, at, {"OBSERVER", "NAME"});
    if (!split) {
        return split.error();
    }
    const vec::WordRules rules{{"height", "target", "max", "curvature", "areas"},
                               {"OBSERVERS", "PREVIEW"},
                               false,
                               kViewshedUsage};
    auto words = vec::readVerbWords(split->rest, 0, split->rest.size(), rules);
    if (!words) {
        return words.error();
    }
    auto sight = sightWords(*words);
    if (!sight) {
        return sight.error();
    }
    auto maxDistance = vec::numberOption(*words, "max");
    if (!maxDistance) {
        return maxDistance.error();
    }
    if (*maxDistance && !(**maxDistance > 0.0)) {
        return refusal("max is a distance above 0", "max=" + katana::core::formatExactReal(**maxDistance));
    }
    std::string areas;
    if (const std::string* layer = words->option("areas")) {
        if (auto valid = katana::entity::validateLayerPath(*layer); !valid) {
            return refusal("areas is a layer path: " + valid.error().message, *layer);
        }
        areas = *layer;
    }
    std::optional<std::string> name;
    std::vector<Observer> observers;
    for (const auto& [keyword, text] : split->taken) {
        if (keyword == "NAME") {
            if (name || text.empty()) {
                return refusal("NAME is the reference raster's name, once", text);
            }
            name = text;
            continue;
        }
        const auto point = an::pointOf(text);
        if (!point) {
            return refusal("OBSERVER is a point: OBSERVER 120.5,40", text);
        }
        observers.push_back({*point, std::nullopt});
    }
    const bool fromScope = words->has("OBSERVERS");
    if (fromScope && !observers.empty()) {
        return refusal("observers are OBSERVER x,y or OBSERVERS <scope>, not both");
    }
    if (!fromScope && words->scopeGiven) {
        return refusal("a scope names observers only after OBSERVERS: OBSERVERS LAYERS survey");
    }
    if (!fromScope && observers.empty()) {
        return refusal(std::string("RASTER VIEWSHED needs an observer: ") + kViewshedUsage);
    }
    std::vector<std::string> records{source->record};
    if (fromScope) {
        auto match = katana::cad::matchScope(context.document, words->scope,
                                             context.interpreter.scopeContext());
        if (!match) {
            return match.error();
        }
        std::map<std::string, std::size_t> skipped;
        for (const katana::entity::EntityId id : match->matched) {
            const katana::entity::Entity* entity = context.document.model().entities.find(id);
            if (entity == nullptr) {
                continue;
            }
            if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity->geometry)) {
                observers.push_back({point->position, id});
            } else {
                ++skipped[katana::core::lowered(std::string(katana::entity::toString(entity->type())))];
            }
        }
        std::string record = "scope arg=observers " + katana::cad::scopeRecord(*match) +
                             " points=" + std::to_string(observers.size());
        for (const auto& [type, count] : skipped) {
            record += " skipped." + type + "=" + std::to_string(count);
        }
        records.push_back(record);
    }
    if (observers.size() > kMaxObservers) {
        return refusal("at most " + std::to_string(kMaxObservers) +
                       " observers: each is a whole run over the raster");
    }
    std::string settings = "viewshed observers=" + std::to_string(observers.size()) +
                           " height=" + katana::core::formatExactReal(sight->height) +
                           " target=" + katana::core::formatExactReal(sight->target) +
                           " max=" + (*maxDistance ? katana::core::formatExactReal(**maxDistance)
                                                   : std::string()) +
                           " curvature=" + katana::core::formatExactReal(sight->curvature);
    const std::string rasterName =
        name.value_or(sourceName(context, source->source) + "-viewshed");
    if (observers.empty()) {
        records.insert(records.begin(), vec::notRunRecord("viewshed", "observers=0"));
        records.push_back(settings);
        return vec::answered("RASTER VIEWSHED", vec::joined(records));
    }
    if (words->has("PREVIEW")) {
        records.insert(records.begin(), "gis op=viewshed preview=yes");
        records.push_back(settings + " name=" + value(rasterName));
        records.push_back("preview valid=yes changed=no");
        return vec::answered("RASTER VIEWSHED", vec::joined(records));
    }

    Prepared prepared;
    prepared.title = "Viewshed";
    const DeferredDataset dataset = source->dataset;
    const std::filesystem::path scratch = context.scratch;
    const std::string crs = projectCrs(context);
    const SightWords options = *sight;
    const std::optional<double> reach = *maxDistance;
    const std::string commandName(katana::core::trimmed(line));
    prepared.work = [dataset, scratch, crs, options, reach, observers, areas, records, settings,
                     rasterName, commandName](const std::stop_token& stop,
                                              const Progress& progress) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        auto input = dataset();
        if (!input) {
            return input.error();
        }
        auto facts = rasterInfoOf(*input);
        if (!facts) {
            return facts.error();
        }
        const katana::geometry::Box2 extent = rasterExtent(*facts);
        for (const Observer& observer : observers) {
            if (!(observer.at.x >= extent.min.x && observer.at.x <= extent.max.x &&
                  observer.at.y >= extent.min.y && observer.at.y <= extent.max.y)) {
                return refusal("the observer is off the raster", an::pointText(observer.at));
            }
        }
        static std::atomic<std::uint64_t> made{0};
        const std::string run = std::to_string(++made);
        const std::filesystem::path folder = scratch / ("katana-viewshed-" + run);
        std::error_code error;
        std::filesystem::create_directories(folder, error);
        // The runs' files go with the folder, however the work ends.
        struct Cleanup {
            std::filesystem::path folder;
            ~Cleanup()
            {
                std::error_code ignored;
                std::filesystem::remove_all(folder, ignored);
            }
        } cleanup{folder};

        auto viewed = std::make_shared<Viewed>();
        std::vector<Seen> runs;
        const double share = 0.8 / static_cast<double>(observers.size());
        for (std::size_t i = 0; i < observers.size(); ++i) {
            const Observer& observer = observers[i];
            gp::RunRequest request;
            request.path = {"raster", "viewshed"};
            // Its own copy of the input: never a dataset shared between runs.
            request.values.emplace_back("input", gp::ArgValue(*input));
            // The eye height rides as the position's third value, X,Y,H:
            // GDAL 3.13 ignores --height beside a two-value position and
            // looks from its default 2 m instead (a 1 m wall 20 m off then
            // shadows to 20 x 2 / 1 = 40 m whatever height= said; measured).
            request.values.emplace_back("position",
                                        gp::ArgValue(gp::Scalar(std::vector<double>{
                                            observer.at.x, observer.at.y, options.height})));
            request.values.emplace_back("target-height", gp::ArgValue(gp::Scalar(options.target)));
            request.values.emplace_back("curvature-coefficient",
                                        gp::ArgValue(gp::Scalar(options.curvature)));
            request.values.emplace_back("visible-value", gp::ArgValue(gp::Scalar(kVisibleValue)));
            if (reach) {
                request.values.emplace_back("max-distance", gp::ArgValue(gp::Scalar(*reach)));
            }
            request.outputTo = gp::OutputTo::Memory;
            request.maxMemoryCells = 0;
            request.spillDirectory = utf8Of(folder);
            auto outputs = gp::run(request, stop,
                                   vec::progressSpan(progress, share * static_cast<double>(i),
                                                     share * static_cast<double>(i + 1)));
            if (!outputs) {
                return outputs.error();
            }
            for (const gp::Diagnostic& warning : outputs->diagnostics) {
                viewed->warnings.push_back(warning.message);
            }
            auto seen = readSeen(*outputs);
            if (!seen) {
                return seen.error();
            }
            viewed->observerRecords.push_back(
                "observer at=" + an::pointText(observer.at) +
                (observer.entity ? " entity=" + std::to_string(*observer.entity) : std::string()) +
                " visible_cells=" + std::to_string(seen->cells));
            runs.push_back(std::move(seen).value());
        }
        viewed->grid = unionOf(runs, crs.empty() ? facts->projectionWkt : crs);
        const auto& gt = viewed->grid.info.geotransform;
        const double cellArea = std::abs(gt[1] * gt[5] - gt[2] * gt[4]);
        for (const double cell : viewed->grid.bands.front()) {
            viewed->visibleCells += cell == 1.0 ? 1U : 0U;
        }
        viewed->visibleArea = static_cast<double>(viewed->visibleCells) * cellArea;

        RasterChain chain(gp::DatasetValue(viewed->grid), scratch);
        if (!areas.empty() && viewed->visibleCells != 0) {
            auto regions = chain.features({"raster", "polygonize"}, {"--attribute-name=visible"},
                                          stop, vec::progressSpan(progress, 0.8, 0.9));
            if (!regions) {
                return regions.error();
            }
            // The seen regions only, with no field of their own: the layer
            // says what they are.
            gp::FeatureSet seen;
            for (const gp::FeatureTable& table : regions->tables) {
                const auto field = vec::fieldIndex(table, "visible");
                gp::FeatureTable kept;
                kept.name = "visible";
                kept.kind = table.kind;
                kept.crsWkt = table.crsWkt;
                for (const gp::Feature& feature : table.features) {
                    const gp::FieldValue held =
                        field && *field < feature.values.size() ? feature.values[*field]
                                                                : gp::FieldValue{};
                    const bool isSeen =
                        (std::holds_alternative<std::int64_t>(held) && std::get<std::int64_t>(held) == 1) ||
                        (std::holds_alternative<double>(held) && std::get<double>(held) == 1.0);
                    if (isSeen) {
                        gp::Feature part;
                        part.parts = feature.parts;
                        kept.features.push_back(std::move(part));
                    }
                }
                if (!kept.features.empty()) {
                    seen.tables.push_back(std::move(kept));
                }
            }
            viewed->areas = std::move(seen);
        }
        // The display copy: seen cells tinted, the rest clear. No "nv"
        // entry: the union grid has no no-data value, and GDAL warns on
        // every run that it ignores one.
        auto colours = chain.write("colours.txt", "1 255 140 0 150\n0 0 0 0 0\n");
        if (!colours) {
            return colours.error();
        }
        auto coloured = chain.step({"raster", "color-map"},
                                   {"--color-map=" + utf8Of(*colours), "--add-alpha",
                                    "--color-selection=exact"},
                                   stop, vec::progressSpan(progress, 0.9, 1.0));
        if (!coloured) {
            return coloured.error();
        }
        auto kept = chain.keep(scratch / ("katana-viewshed-picture-" + run + ".tif"));
        if (!kept) {
            return kept.error();
        }
        viewed->picture = *kept;
        for (const gp::Diagnostic& warning : chain.warnings) {
            viewed->warnings.push_back(warning.message);
        }
        viewed->seconds = std::chrono::duration<double>(Clock::now() - start).count();
        return Apply([viewed, areas, records, settings, rasterName,
                      commandName](Context& ctx) -> Result<std::string> {
            std::vector<std::string> reply{vec::gisRecord("viewshed", viewed->seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.insert(reply.end(), viewed->observerRecords.begin(),
                         viewed->observerRecords.end());
            ApplyRequest request;
            request.target.kind = Target::Kind::Reference;
            request.target.name = rasterName;
            request.defaultName = rasterName;
            request.result.commandName = commandName;
            gp::RunOutputs outputs;
            outputs.raster = viewed->grid;
            auto applied = applyOutputs(ctx, request, std::move(outputs));
            std::error_code removed;
            if (!applied) {
                std::filesystem::remove(viewed->picture, removed);
                return applied.error();
            }
            reply.push_back(*applied);
            // Drawn as its tinted picture, whose grid is the values' own.
            auto picture = katana::interop::importRaster(viewed->picture);
            std::filesystem::remove(viewed->picture, removed);
            for (const Record& record : parseRecords(*applied)) {
                const auto id = record.kind == "output" ? record.get("id") : std::nullopt;
                const auto number = id ? katana::core::parseInteger(*id) : std::nullopt;
                katana::interop::RasterOverlay* raster =
                    number ? ctx.reference.findRaster(static_cast<katana::interop::ReferenceId>(*number))
                           : nullptr;
                if (raster != nullptr && picture) {
                    raster->rgba = std::move(picture->rgba);
                    raster->width = picture->width;
                    raster->height = picture->height;
                    raster->geotransform = picture->geotransform;
                }
            }
            if (!picture) {
                reply.push_back(warningRecord("the tinted picture could not be read, so the "
                                              "viewshed is drawn grey: " +
                                              picture.error().message));
            }
            if (ctx.changed) {
                ctx.changed();
            }
            if (viewed->areas) {
                igeo::ResultOptions result;
                result.targetLayer = areas;
                result.operation = "raster viewshed";
                result.commandName = commandName;
                auto plan = igeo::resultCommand(ctx.document.model(), *viewed->areas, result);
                if (!plan) {
                    return plan.error();
                }
                vec::Applied drawn;
                drawn.created = plan->created;
                drawn.skipped = plan->skipped;
                auto executed = vec::executeStep(ctx, std::move(plan->command));
                if (!executed) {
                    return executed.error();
                }
                std::string record = vec::outputRecord(areas, drawn);
                reply.push_back(record.replace(0, std::string("output arg=output").size(),
                                               "output arg=areas"));
                for (const std::string& warning : plan->warnings) {
                    reply.push_back(warningRecord(warning));
                }
            }
            reply.push_back(settings + " visible_cells=" + std::to_string(viewed->visibleCells) +
                            " area=" + fixed3(viewed->visibleArea));
            for (const std::string& warning : viewed->warnings) {
                reply.push_back(warningRecord(warning));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

// ---- LOS ------------------------------------------------------------------------------------

Result<Prepared> prepareLineOfSight(Context& context, const Tokens& tokens, std::string_view)
{
    std::size_t at = 1;
    auto ground = an::bindGround(context, tokens, at, "LOS");
    if (!ground) {
        return ground.error();
    }
    auto split = an::takeKeywordValues(tokens, at, {"OBSERVER", "TARGET"});
    if (!split) {
        return split.error();
    }
    const vec::WordRules rules{{"height", "target", "curvature", "step", "method"},
                               {"PREVIEW"},
                               false,
                               kLosUsage};
    auto words = vec::readVerbWords(split->rest, 0, split->rest.size(), rules);
    if (!words) {
        return words.error();
    }
    if (words->scopeGiven) {
        return refusal(std::string("LOS reads no scope: ") + kLosUsage);
    }
    auto sight = sightWords(*words);
    if (!sight) {
        return sight.error();
    }
    std::optional<Point2> observer;
    std::optional<Point2> target;
    for (const auto& [keyword, text] : split->taken) {
        const auto point = an::pointOf(text);
        if (!point) {
            return refusal(keyword + " is a point: " + keyword + " 120.5,40", text);
        }
        std::optional<Point2>& slot = keyword == "OBSERVER" ? observer : target;
        if (slot) {
            return refusal(keyword + " is given twice", text);
        }
        slot = *point;
    }
    if (!observer || !target) {
        return refusal(std::string("LOS needs an OBSERVER and a TARGET: ") + kLosUsage);
    }
    katana::gis::Resampling method = katana::gis::Resampling::Bilinear;
    if (const std::string* named = words->option("method")) {
        if (ground->surface) {
            return refusal("a surface is read on its own triangles; method= is for a raster",
                           "method=" + *named);
        }
        auto parsed = katana::gis::resamplingNamed(*named);
        if (!parsed) {
            return parsed.error();
        }
        method = *parsed;
    }
    auto step = vec::numberOption(*words, "step");
    if (!step) {
        return step.error();
    }
    if (*step && !(**step > 0.0)) {
        return refusal("step is a distance above 0", "step=" + katana::core::formatExactReal(**step));
    }
    const double length = std::hypot(target->x - observer->x, target->y - observer->y);
    std::vector<std::string> records{ground->record};
    if (words->has("PREVIEW")) {
        records.insert(records.begin(), "gis op=los preview=yes");
        records.push_back("sight observer=" + an::pointText(*observer) +
                          " target=" + an::pointText(*target) +
                          " distance=" + fixed3(length));
        records.push_back("preview valid=yes changed=no");
        return vec::answered("LOS", vec::joined(records));
    }

    Prepared prepared;
    prepared.title = "Line of Sight";
    const an::Ground from = *ground;
    const SightWords options = *sight;
    const std::optional<double> given = *step;
    const Point2 eye = *observer;
    const Point2 aim = *target;
    prepared.work = [from, method, options, given, eye, aim, length,
                     records](const std::stop_token& stop, const Progress&) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        auto open = an::openGround(from, method);
        if (!open) {
            return open.error();
        }
        katana::terrain::SightOptions walk;
        walk.observerHeight = options.height;
        walk.targetHeight = options.target;
        walk.curvature = options.curvature;
        // Half a raster cell resolves the raster; a surface, read exactly,
        // every 10 cm - finer than survey triangles - or a millionth of the
        // line when that is longer.
        walk.step = given.value_or(open->spacing.value_or(std::max(0.1, length / 1e6)));
        auto line = katana::terrain::lineOfSight(open->at, eye, aim, walk, stop);
        if (!line) {
            return line.error();
        }
        std::string record = "sight visible=" + std::string(line->visible ? "yes" : "no") +
                             " observer=" + an::pointText(eye) + " target=" + an::pointText(aim) +
                             " distance=" + fixed3(line->distance) +
                             " observer_z=" + fixed3(line->observerZ) +
                             " target_z=" + fixed3(line->targetZ);
        if (line->clearance) {
            record += " clearance=" + fixed3(*line->clearance) +
                      " clearance_at=" + stationText(*line->clearanceAt) +
                      " clearance_distance=" + fixed3(*line->clearanceDistance);
        }
        if (line->blockedAt) {
            record += " blocked_at=" + stationText(*line->blockedAt) +
                      " blocked_distance=" + fixed3(*line->blockedDistance) +
                      " blocked_ground=" + fixed3(*line->blockedGround);
        }
        record += " stations=" + std::to_string(line->stations) +
                  " unknown=" + std::to_string(line->unknown) +
                  " step=" + katana::core::formatExactReal(walk.step) +
                  " curvature=" + katana::core::formatExactReal(walk.curvature);
        const std::size_t unknown = line->unknown;
        const double seconds = std::chrono::duration<double>(Clock::now() - start).count();
        return Apply([records, record, unknown, seconds](Context&) -> Result<std::string> {
            std::vector<std::string> reply{vec::gisRecord("los", seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.push_back(record);
            if (unknown != 0) {
                reply.push_back(warningRecord(std::to_string(unknown) +
                                              " stations of the sight line are off the ground, "
                                              "and nothing there can hide the target"));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

} // namespace katana::app::geo
