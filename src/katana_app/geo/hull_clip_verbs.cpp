// GIS HULL and GIS CLIP (docs/geoprocessing.md, "V3"): the boundary around
// what the scope takes - a survey's extent, the edge of the data - and what
// the scope takes clipped to the areas of a second scope or a file.
//
//   GIS HULL [<scope>] [convex | concave=<0..1>] [holes] [TO LAYER <path>] [PREVIEW]
//   GIS CLIP [<scope>] BY (<scope> | FILE <path> [LAYER <name>] [where="<sql>"])
//            [TO LAYER <path> | REPLACE] [PREVIEW]
//
// The convex hull is Katana's own (geometry::convexHull): nothing GDAL adds.
// The concave one is GDAL's `vector concave-hull` of every point and vertex
// in the scope as one multipoint - GEOS's ratio, 1 the convex hull and
// towards 0 the tightest. The clip is `vector clip --like`, which clips to
// the like dataset's geometries (measured: not merely their bounds), and
// takes a file's layer and WHERE clause itself.
//
// A clip draws the pieces on a layer, or with REPLACE cuts the entities in
// place: each keeps its id on its largest piece and the rest are made
// beside it (vector_support.hpp's reshape, REPAIR's too); one wholly
// outside the boundary is deleted; one wholly inside is left alone.

#include <algorithm>
#include <chrono>
#include <map>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "bindings.hpp"
#include "geo_verbs.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/geometry/polygon.hpp"
#include "katana/interop/geo/drawing_dataset.hpp"
#include "replies.hpp"
#include "vector_support.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
namespace cmd = katana::commands;
namespace vec = katana::app::geo::vector;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::EntityId;
using katana::gis::GeoPoint;
using katana::gis::GeometryKind;
using katana::gis::VectorGeometry;

namespace {

constexpr const char* kHullUsage =
    "GIS HULL [<scope>] [convex | concave=<0..1>] [holes] [TO LAYER <path>] [PREVIEW]";
constexpr const char* kClipUsage =
    "GIS CLIP [<scope>] BY (<scope> | FILE <path> [LAYER <name>] [where=\"<sql>\"]) "
    "[TO LAYER <path> | REPLACE] [PREVIEW]";

using Clock = std::chrono::steady_clock;

double secondsSince(Clock::time_point start)
{
    return std::chrono::duration<double>(Clock::now() - start).count();
}

katana::core::Status checkLayer(const std::string& layer)
{
    if (auto valid = katana::entity::validateLayerPath(layer); !valid) {
        return makeError(ErrorCode::InvalidArgument,
                         "TO LAYER needs a layer path: " + valid.error().message, layer);
    }
    return {};
}

// Every point and vertex of a feature set, once each.
std::vector<GeoPoint> verticesOf(const gp::FeatureSet& set)
{
    std::vector<GeoPoint> points;
    for (const gp::FeatureTable& table : set.tables) {
        for (const gp::Feature& feature : table.features) {
            for (const VectorGeometry& part : feature.parts) {
                for (const auto& ring : part.parts) {
                    points.insert(points.end(), ring.begin(), ring.end());
                }
            }
        }
    }
    std::ranges::sort(points, [](const GeoPoint& a, const GeoPoint& b) {
        return a.x != b.x ? a.x < b.x : a.y < b.y;
    });
    const auto repeats = std::ranges::unique(points, [](const GeoPoint& a, const GeoPoint& b) {
        return a.x == b.x && a.y == b.y;
    });
    points.erase(repeats.begin(), repeats.end());
    return points;
}

// ---- GIS HULL --------------------------------------------------------------------------------

Result<Prepared> prepareHull(Context& context, const Tokens& tokens, std::string_view line)
{
    const vec::WordRules rules{{"concave"}, {"CONVEX", "HOLES", "PREVIEW"}, true, kHullUsage};
    auto words = vec::readVerbWords(tokens, 2, tokens.size(), rules);
    if (!words) {
        return words.error();
    }
    auto ratio = vec::numberOption(*words, "concave");
    if (!ratio) {
        return ratio.error();
    }
    if (*ratio && words->has("CONVEX")) {
        return makeError(ErrorCode::InvalidArgument, "convex or concave=<ratio>, not both");
    }
    if (*ratio && !(**ratio >= 0.0 && **ratio <= 1.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "concave= is a ratio from 0 (the tightest) to 1 (the convex hull)",
                         "concave=" + *words->option("concave"));
    }
    if (words->has("HOLES") && !*ratio) {
        return makeError(ErrorCode::InvalidArgument,
                         "a convex hull has no holes: HOLES goes with concave=<ratio>", "HOLES");
    }
    auto layer = vec::layerTarget(*words, "GIS HULL", "gis/hull");
    if (!layer) {
        return layer.error();
    }
    if (auto valid = checkLayer(*layer); !valid) {
        return valid.error();
    }
    auto bound = vec::bindScope(context, words->scope);
    if (!bound) {
        return bound.error();
    }
    std::vector<std::string> records{scopeRecord("input", *bound)};
    for (const std::string& warning : bound->dataset.stats.warnings) {
        records.push_back(warningRecord(warning));
    }
    const std::vector<GeoPoint> points = verticesOf(bound->dataset.set);
    const std::optional<double> concave = *ratio;
    const bool holes = words->has("HOLES");
    const std::string settings =
        "hull kind=" + std::string(concave ? "concave" : "convex") +
        " ratio=" + (concave ? katana::core::formatExactReal(*concave) : std::string()) +
        " holes=" + (holes ? "yes" : "no") + " points=" + std::to_string(points.size());
    if (points.empty()) {
        records.insert(records.begin(), vec::notRunRecord("hull", ""));
        records.push_back(settings + " area=0.000");
        return vec::answered("GIS HULL", vec::joined(records));
    }
    if (words->has("PREVIEW")) {
        records.insert(records.begin(), "gis op=hull preview=yes");
        records.push_back(settings);
        records.push_back("preview valid=yes changed=no");
        return vec::answered("GIS HULL", vec::joined(records));
    }
    const std::string target = *layer;
    const std::string crs = projectCrs(context);
    const std::string commandName(katana::core::trimmed(line));
    Prepared prepared;
    prepared.title = "GIS HULL";
    prepared.work = [points, concave, holes, target, crs, settings, records, commandName](
                        const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        std::vector<std::string> warnings;
        gp::FeatureSet hull;
        gp::FeatureTable& table = hull.tables.emplace_back();
        table.name = "hull";
        table.kind = GeometryKind::Polygon;
        table.crsWkt = crs;
        if (!concave) {
            std::vector<katana::geometry::Point2> plan;
            for (const GeoPoint& point : points) {
                plan.emplace_back(point.x, point.y);
            }
            const std::vector<katana::geometry::Point2> ring = katana::geometry::convexHull(plan);
            if (ring.size() >= 3) {
                VectorGeometry polygon;
                polygon.kind = GeometryKind::Polygon;
                auto& exterior = polygon.parts.emplace_back();
                for (const katana::geometry::Point2& corner : ring) {
                    exterior.push_back(GeoPoint{corner.x, corner.y, 0.0});
                }
                table.features.push_back(gp::Feature{{polygon}, {}});
            }
        } else {
            // All the points as ONE multipoint: the hull of them together,
            // not one per entity.
            gp::FeatureSet input;
            gp::FeatureTable& cloud = input.tables.emplace_back();
            cloud.name = "points";
            cloud.kind = GeometryKind::Point;
            cloud.crsWkt = crs;
            gp::Feature all;
            for (const GeoPoint& point : points) {
                VectorGeometry one;
                one.kind = GeometryKind::Point;
                one.parts.push_back({GeoPoint{point.x, point.y, 0.0}});
                all.parts.push_back(std::move(one));
            }
            cloud.features.push_back(std::move(all));
            std::vector<std::string> gdalWords{"--ratio=" + katana::core::formatExactReal(*concave)};
            if (holes) {
                gdalWords.emplace_back("--allow-holes");
            }
            auto out = vec::runVector({"vector", "concave-hull"}, input, gdalWords, stop, progress,
                                      warnings);
            if (!out) {
                return out.error();
            }
            for (const gp::FeatureTable& made : out->tables) {
                for (const gp::Feature& feature : made.features) {
                    gp::Feature copy;
                    for (const VectorGeometry& part : feature.parts) {
                        if (part.kind == GeometryKind::Polygon) {
                            copy.parts.push_back(part);
                        }
                    }
                    if (!copy.parts.empty()) {
                        table.features.push_back(std::move(copy));
                    }
                }
            }
        }
        if (table.features.empty()) {
            warnings.emplace_back("the points in the scope are fewer than three or all in a line: "
                                  "they bound no area");
        }
        const vec::Measures measures = vec::measure(hull);
        const std::string summary = settings + " area=" + fixed3(measures.area);
        const double seconds = secondsSince(start);
        auto kept = std::make_shared<const gp::FeatureSet>(std::move(hull));
        const std::string operation = concave ? "vector concave-hull" : "convex hull";
        return Apply([kept, operation, target, summary, records, warnings, seconds,
                      commandName](Context& ctx) -> Result<std::string> {
            igeo::ResultOptions options;
            options.targetLayer = target;
            options.operation = operation;
            options.commandName = commandName;
            auto made = igeo::resultCommand(ctx.document.model(), *kept, options);
            if (!made) {
                return made.error();
            }
            vec::Applied applied;
            applied.created = made->created;
            applied.skipped = made->skipped;
            if (auto done = vec::executeStep(ctx, std::move(made->command)); !done) {
                return done.error();
            }
            std::vector<std::string> reply{vec::gisRecord("hull", seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.push_back(vec::outputRecord(target, applied));
            reply.push_back(summary);
            for (const std::string& warning : warnings) {
                reply.push_back(warningRecord(warning));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

// ---- GIS CLIP --------------------------------------------------------------------------------

struct Boundary {
    std::shared_ptr<const gp::FeatureSet> drawn; // the second scope's areas
    std::string path, layer, where;              // FILE
};

// The pieces of each clipped feature, gathered by the entity it came from:
// vector clip gives a piece a feature of its own.
std::map<EntityId, gp::Feature> piecesById(const gp::FeatureSet& clipped)
{
    std::map<EntityId, gp::Feature> pieces;
    for (const gp::FeatureTable& table : clipped.tables) {
        for (const gp::Feature& feature : table.features) {
            if (const auto id = vec::featureId(table, feature)) {
                gp::Feature& into = pieces[*id];
                into.parts.insert(into.parts.end(), feature.parts.begin(), feature.parts.end());
            }
        }
    }
    return pieces;
}

Result<Prepared> prepareClip(Context& context, const Tokens& tokens, std::string_view line)
{
    std::size_t by = tokens.size();
    for (std::size_t i = 2; i < tokens.size(); ++i) {
        if (tokens.is(i, "BY")) {
            by = i;
            break;
        }
    }
    if (by == tokens.size()) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string("GIS CLIP needs BY and the boundary: ") + kClipUsage);
    }
    const vec::WordRules rules{{"where"}, {"REPLACE", "PREVIEW"}, true, kClipUsage};
    auto subjectWords = vec::readVerbWords(tokens, 2, by, rules);
    if (!subjectWords) {
        return subjectWords.error();
    }
    std::size_t from = by + 1;
    Boundary boundary;
    if (tokens.is(from, "FILE")) {
        if (from + 1 >= tokens.size()) {
            return makeError(ErrorCode::InvalidArgument, "FILE needs a path", "FILE");
        }
        boundary.path = tokens[from + 1];
        from += 2;
        if (tokens.is(from, "LAYER")) {
            if (from + 1 >= tokens.size()) {
                return makeError(ErrorCode::InvalidArgument, "LAYER needs the file's layer name",
                                 "LAYER");
            }
            boundary.layer = tokens[from + 1];
            from += 2;
        }
    }
    auto byWords = vec::readVerbWords(tokens, from, tokens.size(), rules);
    if (!byWords) {
        return byWords.error();
    }
    if (!boundary.path.empty() && byWords->scopeGiven) {
        return makeError(ErrorCode::InvalidArgument, "BY takes a scope or FILE <path>, not both");
    }
    if (boundary.path.empty() && !byWords->scopeGiven) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string("BY needs the boundary: a scope or FILE <path>: ") + kClipUsage);
    }
    vec::VerbWords words = std::move(subjectWords).value();
    for (const auto& [key, text] : byWords->options) {
        if (!words.options.emplace(key, text).second) {
            return makeError(ErrorCode::InvalidArgument, key + "= is given twice", key);
        }
    }
    for (const std::string& flag : byWords->flags) {
        if (!words.flags.insert(flag).second) {
            return makeError(ErrorCode::InvalidArgument, flag + " is given twice", flag);
        }
    }
    if (byWords->target) {
        if (words.target) {
            return makeError(ErrorCode::InvalidArgument, "one TO per line");
        }
        words.target = byWords->target;
    }
    if (const std::string* where = words.option("where")) {
        if (boundary.path.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "where= filters a FILE boundary; a drawn one takes WHERE conditions",
                             "where=" + *where);
        }
        boundary.where = *where;
    }
    const bool replace = words.has("REPLACE");
    if (replace && words.target) {
        return makeError(ErrorCode::InvalidArgument,
                         "REPLACE cuts the entities in place; TO LAYER draws the pieces beside "
                         "them - one or the other");
    }
    auto layer = vec::layerTarget(words, "GIS CLIP", "gis/clip");
    if (!layer) {
        return layer.error();
    }
    if (auto valid = checkLayer(*layer); !valid) {
        return valid.error();
    }

    auto subject = vec::bindScope(context, words.scope);
    if (!subject) {
        return subject.error();
    }
    std::vector<std::string> records{scopeRecord("input", *subject)};
    for (const std::string& warning : subject->dataset.stats.warnings) {
        records.push_back(warningRecord(warning));
    }
    bool tookNothing = subject->dataset.set.tables.empty();
    if (boundary.path.empty()) {
        auto drawn = vec::bindScope(context, byWords->scope);
        if (!drawn) {
            return drawn.error();
        }
        records.push_back(scopeRecord("like", *drawn));
        gp::FeatureSet areas;
        if (const gp::FeatureTable* polygons = vec::tableNamed(drawn->dataset.set, "polygons")) {
            areas.tables.push_back(*polygons);
        }
        tookNothing = tookNothing || areas.tables.empty();
        boundary.drawn = std::make_shared<const gp::FeatureSet>(std::move(areas));
    } else {
        std::string record = "input arg=like source=file file=" + value(boundary.path);
        if (!boundary.layer.empty()) {
            record += " layer=" + value(boundary.layer);
        }
        if (!boundary.where.empty()) {
            record += " where=" + value(boundary.where);
        }
        records.push_back(record);
    }
    std::size_t features = 0;
    for (const gp::FeatureTable& table : subject->dataset.set.tables) {
        features += table.features.size();
    }
    const std::string settings = "clip mode=" + std::string(replace ? "replace" : "layer") +
                                 " features=" + std::to_string(features);
    if (tookNothing) {
        records.insert(records.begin(), vec::notRunRecord("clip", ""));
        records.push_back(settings);
        return vec::answered("GIS CLIP", vec::joined(records));
    }
    if (words.has("PREVIEW")) {
        records.insert(records.begin(), "gis op=clip preview=yes");
        records.push_back(settings);
        records.push_back("preview valid=yes changed=no");
        return vec::answered("GIS CLIP", vec::joined(records));
    }

    const auto input = std::make_shared<const gp::FeatureSet>(subject->dataset.set);
    std::vector<EntityId> ids;
    for (const auto& [id, feature] : vec::featuresById(*input)) {
        ids.push_back(id);
    }
    const auto copies = std::make_shared<const std::vector<katana::entity::Entity>>(
        replace ? vec::copiesOf(context.document, ids) : std::vector<katana::entity::Entity>{});
    const std::string target = *layer;
    const std::string commandName(katana::core::trimmed(line));
    Prepared prepared;
    prepared.title = "GIS CLIP";
    prepared.work = [input, boundary, replace, copies, target, settings, records, commandName](
                        const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        std::vector<std::string> warnings;
        gp::RunRequest request;
        request.path = {"vector", "clip"};
        request.values.emplace_back("input", gp::ArgValue(gp::DatasetValue(*input)));
        if (boundary.drawn) {
            request.values.emplace_back("like", gp::ArgValue(gp::DatasetValue(*boundary.drawn)));
        } else {
            gp::DatasetPath file;
            file.path = boundary.path;
            request.values.emplace_back("like", gp::ArgValue(gp::DatasetValue(file)));
            if (!boundary.layer.empty()) {
                request.tokens.push_back("--like-layer=" + boundary.layer);
            }
            if (!boundary.where.empty()) {
                request.tokens.push_back("--like-where=" + boundary.where);
            }
        }
        request.outputTo = gp::OutputTo::Memory;
        auto outputs = gp::run(request, stop, progress);
        if (!outputs) {
            return outputs.error();
        }
        for (const gp::Diagnostic& diagnostic : outputs->diagnostics) {
            if (!diagnostic.failure) {
                warnings.push_back(diagnostic.message);
            }
        }
        gp::FeatureSet clipped = vec::withoutEmpty(outputs->features.value_or(gp::FeatureSet{}));
        const vec::Measures measures = vec::measure(clipped);
        // What became of each entity: whole, cut, or gone.
        const auto before = vec::featuresById(*input);
        const std::map<EntityId, gp::Feature> pieces = piecesById(clipped);
        std::size_t whole = 0;
        std::vector<vec::Reshaped> cut;
        std::vector<EntityId> gone;
        for (const auto& [id, was] : before) {
            const auto found = pieces.find(id);
            if (found == pieces.end()) {
                gone.push_back(id);
            } else if (vec::sameShape(*was, found->second)) {
                ++whole;
            } else {
                cut.push_back(vec::Reshaped{id, found->second.parts, *was});
            }
        }
        const std::string summary = settings + " whole=" + std::to_string(whole) +
                                    " cut=" + std::to_string(cut.size()) +
                                    " outside=" + std::to_string(gone.size()) +
                                    " area=" + fixed3(measures.area) +
                                    " length=" + fixed3(measures.length);
        const double seconds = secondsSince(start);
        auto kept = std::make_shared<const gp::FeatureSet>(std::move(clipped));
        auto cuts = std::make_shared<const std::vector<vec::Reshaped>>(std::move(cut));
        return Apply([kept, cuts, gone, replace, copies, target, summary, records, warnings, seconds,
                      commandName](Context& ctx) -> Result<std::string> {
            std::vector<std::string> reply{vec::gisRecord("clip", seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            vec::Applied applied;
            if (replace) {
                // Cutting what changed since would overwrite a concurrent edit.
                if (auto same = unchangedSince(ctx, *copies); !same) {
                    return same.error();
                }
                auto reshape = vec::reshapeCommand(ctx.document.model(), *cuts, "vector clip",
                                                   commandName);
                if (!reshape) {
                    return reshape.error();
                }
                auto step = std::make_unique<cmd::Transaction>(commandName);
                if (reshape->command) {
                    step->add(std::move(reshape->command));
                }
                if (!gone.empty()) {
                    step->add(cmd::deleteEntities(gone));
                }
                auto created = vec::executeStep(ctx, step->size() == 0 ? nullptr : std::move(step));
                if (!created) {
                    return created.error();
                }
                for (std::string& split : vec::splitRecords(reshape->splits, *created)) {
                    reply.push_back(std::move(split));
                }
                applied.created = reshape->created;
                applied.updated = reshape->updated;
                applied.deleted = gone.size();
                applied.skipped = reshape->left;
                reply.push_back(vec::outputRecord("", applied));
                for (const std::string& warning : reshape->warnings) {
                    reply.push_back(warningRecord(warning));
                }
            } else {
                igeo::ResultOptions options;
                options.targetLayer = target;
                options.operation = "vector clip";
                options.commandName = commandName;
                std::vector<const gp::FeatureTable*> tables;
                for (const gp::FeatureTable& table : kept->tables) {
                    tables.push_back(&table);
                }
                gp::FeatureSet one;
                if (!tables.empty()) {
                    one.tables.push_back(vec::mergedTable(tables, "clip"));
                }
                auto made = igeo::resultCommand(ctx.document.model(), one, options);
                if (!made) {
                    return made.error();
                }
                applied.created = made->created;
                applied.skipped = made->skipped;
                if (auto done = vec::executeStep(ctx, std::move(made->command)); !done) {
                    return done.error();
                }
                reply.push_back(vec::outputRecord(target, applied));
            }
            reply.push_back(summary);
            for (const std::string& warning : warnings) {
                reply.push_back(warningRecord(warning));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

} // namespace

Result<Prepared> prepareGisHull(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareHull(context, tokens, line);
}

Result<Prepared> prepareGisClip(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareClip(context, tokens, line);
}

} // namespace katana::app::geo
