// GIS BUFFER and GIS DISSOLVE (docs/geoprocessing.md, "V1"): easements,
// setbacks, corridors and clearance zones around drawn features, and areas
// merged by what their properties say - through GDAL's `vector buffer`,
// `vector combine` and `vector dissolve`, on the shared scope and filter.
//
//   GIS BUFFER [<scope>] distance=<m>|distance=prop:<key> [side=both|left|right]
//              [caps=round|flat|square] [joins=round|mitre|bevel] [dissolve[=k1,k2]]
//              [TO LAYER <path>] [PREVIEW]
//   GIS DISSOLVE [<scope>] [by=k1,k2] [keep=identical] [TO LAYER <path>] [REPLACE] [PREVIEW]
//
// What was measured, and so is relied on here:
//   - `vector dissolve` alone unions only the parts WITHIN each feature; merging
//     by a property takes `vector combine --group-by` first (and its output is
//     a geometry collection, which the bridge reads as parts);
//   - `vector buffer` drops Z: a result is in plan;
//   - GEOS buffers a line or a point inwards to nothing (an empty polygon);
//   - GDAL's default of 8 segments a quadrant strays 0.096 m from a 5 m arc,
//     so the quadrant's segments come from the curve tolerance by the sagitta
//     rule every chording in Katana follows (geometry/chording.hpp);
//   - a round buffer of a point is a circle, so it is drawn as one - exactly -
//     rather than as its chords.

#include <algorithm>
#include <chrono>
#include <cmath>
#include <map>
#include <memory>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include "bindings.hpp"
#include "geo_verbs.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/geometry/chording.hpp"
#include "katana/interop/geo/drawing_dataset.hpp"
#include "katana/math/numerics.hpp"
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

namespace {

constexpr const char* kBufferUsage =
    "GIS BUFFER [<scope>] distance=<m>|distance=prop:<key> [side=both|left|right] "
    "[caps=round|flat|square] [joins=round|mitre|bevel] [dissolve[=k1,k2]] [TO LAYER <path>] "
    "[PREVIEW]";
constexpr const char* kDissolveUsage =
    "GIS DISSOLVE [<scope>] [by=k1,k2] [keep=identical] [TO LAYER <path>] [REPLACE] [PREVIEW]";

// The chord tolerance a buffer's arcs are drawn to: drawingDataset's own
// default, the 1 mm sagitta EXPORT writes curves with.
constexpr double kCurveTolerance = 0.001;

using Clock = std::chrono::steady_clock;

double secondsSince(Clock::time_point start)
{
    return std::chrono::duration<double>(Clock::now() - start).count();
}

// A quadrant's segments for a buffer of `distance`: the fewest whose chords
// stay within the curve tolerance of the true arc.
int quadrantSegments(double distance)
{
    return static_cast<int>(
        katana::geometry::sagittaChordCount(std::abs(distance), katana::math::kHalfPi,
                                            kCurveTolerance));
}

std::string number(double value)
{
    return katana::core::formatExactReal(value);
}

katana::core::Status checkLayer(const std::string& layer)
{
    if (auto valid = katana::entity::validateLayerPath(layer); !valid) {
        return makeError(ErrorCode::InvalidArgument,
                         "TO LAYER needs a layer path: " + valid.error().message, layer);
    }
    return {};
}

// The distance a feature's property gives, when it gives one.
std::optional<double> distanceOf(const gp::FieldValue& value)
{
    if (const auto* real = std::get_if<double>(&value)) {
        return std::isfinite(*real) ? std::optional<double>(*real) : std::nullopt;
    }
    if (const auto* whole = std::get_if<std::int64_t>(&value)) {
        return static_cast<double>(*whole);
    }
    if (const auto* text = std::get_if<std::string>(&value)) {
        return katana::core::parseFiniteDouble(katana::core::trimmed(*text));
    }
    return std::nullopt;
}

// A round buffer of a point, drawn as the circle it is.
struct CircleResult {
    katana::geometry::Point2 centre;
    double radius = 0.0;
    katana::entity::PropertyMap properties;
};

struct BufferPlan {
    std::optional<double> distance;
    std::string property; // distance=prop:<key>
    std::string side = "both", caps = "round", joins = "round";
    bool dissolve = false;
    std::vector<std::string> dissolveBy;
    std::string layer = "gis/buffer";
};

// The words of combine and dissolve that merge `set` by `keys` - all of it
// into one when there are none - as `vector combine` then `vector dissolve`
// (dissolve alone unions only within a feature). One table named `name`.
Result<gp::FeatureSet> dissolved(const gp::FeatureSet& set, const std::vector<std::string>& keys,
                                 bool keepIdentical, const std::string& name,
                                 const std::stop_token& stop, const Progress& progress,
                                 std::vector<std::string>& warnings)
{
    std::vector<std::string> combine;
    for (const std::string& key : keys) {
        combine.push_back("--group-by=" + key);
    }
    if (keepIdentical) {
        combine.emplace_back("--add-extra-fields=always-identical");
    }
    auto combined = vec::runVector({"vector", "combine"}, set, combine, stop,
                                   vec::progressSpan(progress, 0.0, 0.5), warnings);
    if (!combined) {
        return combined.error();
    }
    auto merged = vec::runVector({"vector", "dissolve"}, *combined, {}, stop,
                                 vec::progressSpan(progress, 0.5, 1.0), warnings);
    if (!merged) {
        return merged.error();
    }
    gp::FeatureSet out = vec::withoutEmpty(std::move(merged).value());
    std::vector<const gp::FeatureTable*> tables;
    for (const gp::FeatureTable& table : out.tables) {
        tables.push_back(&table);
    }
    gp::FeatureSet one;
    if (!tables.empty()) {
        one.tables.push_back(vec::mergedTable(tables, name));
    }
    return one;
}

// ---- GIS BUFFER --------------------------------------------------------------------------------

Result<BufferPlan> readBuffer(const vec::VerbWords& words)
{
    BufferPlan plan;
    const std::string* distance = words.option("distance");
    if (distance == nullptr) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string("GIS BUFFER needs distance=<m> or distance=prop:<key>: ") +
                             kBufferUsage);
    }
    if (katana::core::lowered(*distance).starts_with("prop:")) {
        plan.property = std::string(katana::core::trimmed(distance->substr(5)));
        if (plan.property.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "distance=prop: names the property that holds each distance",
                             "distance=" + *distance);
        }
    } else {
        const auto value = katana::core::parseFiniteDouble(*distance);
        if (!value || *value == 0.0) {
            return makeError(ErrorCode::InvalidArgument,
                             "distance= is a length other than 0 (negative shrinks an area), or "
                             "prop:<key>",
                             "distance=" + *distance);
        }
        plan.distance = *value;
    }
    auto side = vec::choiceOption(words, "side", {"both", "left", "right"}, "both");
    auto caps = vec::choiceOption(words, "caps", {"round", "flat", "square"}, "round");
    auto joins = vec::choiceOption(words, "joins", {"round", "mitre", "miter", "bevel"}, "round");
    for (const auto* chosen : {&side, &caps, &joins}) {
        if (!*chosen) {
            return chosen->error();
        }
    }
    plan.side = *side;
    plan.caps = *caps;
    plan.joins = *joins == "miter" ? "mitre" : *joins;
    const std::string* dissolveBy = words.option("dissolve");
    if (dissolveBy != nullptr && words.has("DISSOLVE")) {
        return makeError(ErrorCode::InvalidArgument,
                         "DISSOLVE or dissolve=<keys>, not both", "dissolve=" + *dissolveBy);
    }
    plan.dissolve = words.has("DISSOLVE") || dissolveBy != nullptr;
    if (dissolveBy != nullptr) {
        plan.dissolveBy = vec::listOf(*dissolveBy);
        if (plan.dissolveBy.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "dissolve= names the properties to merge by; DISSOLVE alone merges "
                             "everything",
                             "dissolve=");
        }
    }
    auto layer = vec::layerTarget(words, "GIS BUFFER", "gis/buffer");
    if (!layer) {
        return layer.error();
    }
    plan.layer = *layer;
    if (auto valid = checkLayer(plan.layer); !valid) {
        return valid.error();
    }
    return plan;
}

// The features of `set` by the distance each is buffered by: one group for a
// distance given, one per value of the property otherwise. `skipped` counts
// the features whose property gives no distance other than 0.
std::map<double, gp::FeatureSet> byDistance(const gp::FeatureSet& set, const BufferPlan& plan,
                                            std::size_t& skipped)
{
    std::map<double, gp::FeatureSet> groups;
    if (plan.distance) {
        groups.emplace(*plan.distance, set);
        return groups;
    }
    for (const gp::FeatureTable& table : set.tables) {
        const auto field = vec::fieldIndex(table, plan.property);
        for (const gp::Feature& feature : table.features) {
            const std::optional<double> distance =
                field && *field < feature.values.size() ? distanceOf(feature.values[*field])
                                                        : std::nullopt;
            if (!distance || *distance == 0.0) {
                ++skipped;
                continue;
            }
            gp::FeatureSet& group = groups[*distance];
            gp::FeatureTable* into = nullptr;
            for (gp::FeatureTable& existing : group.tables) {
                into = existing.name == table.name ? &existing : into;
            }
            if (into == nullptr) {
                gp::FeatureTable header = table;
                header.features.clear();
                group.tables.push_back(std::move(header));
                into = &group.tables.back();
            }
            into->features.push_back(feature);
        }
    }
    return groups;
}

std::string bufferRecord(const BufferPlan& plan, std::size_t groups, const vec::Measures& result,
                         std::size_t circles, double circleArea, std::size_t empty,
                         std::size_t skipped)
{
    std::string record = "buffer distance=" +
                         (plan.distance ? number(*plan.distance) : "prop:" + value(plan.property));
    record += " side=" + plan.side + " caps=" + plan.caps + " joins=" + plan.joins;
    if (plan.distance) {
        record += " quadrant_segments=" + std::to_string(quadrantSegments(*plan.distance));
    }
    std::string dissolve = "no";
    if (plan.dissolve) {
        dissolve = "all";
        if (!plan.dissolveBy.empty()) {
            dissolve.clear();
            for (const std::string& key : plan.dissolveBy) {
                dissolve += (dissolve.empty() ? "" : ",") + key;
            }
        }
    }
    record += " dissolve=" + value(dissolve) + " groups=" + std::to_string(groups) +
              " features=" + std::to_string(result.features + circles) +
              " circles=" + std::to_string(circles) + " area=" + fixed3(result.area + circleArea) +
              " empty=" + std::to_string(empty);
    if (!plan.distance) {
        record += " skipped.no_distance=" + std::to_string(skipped);
    }
    return record;
}

Result<Prepared> prepareBuffer(Context& context, const Tokens& tokens, std::string_view line)
{
    const vec::WordRules rules{{"distance", "side", "caps", "joins", "dissolve"},
                               {"DISSOLVE", "PREVIEW"},
                               true,
                               kBufferUsage};
    auto words = vec::readVerbWords(tokens, 2, tokens.size(), rules);
    if (!words) {
        return words.error();
    }
    auto read = readBuffer(*words);
    if (!read) {
        return read.error();
    }
    const BufferPlan plan = std::move(read).value();
    auto bound = vec::bindScope(context, words->scope);
    if (!bound) {
        return bound.error();
    }
    std::vector<std::string> records{scopeRecord("input", *bound)};
    for (const std::string& warning : bound->dataset.stats.warnings) {
        records.push_back(warningRecord(warning));
    }
    const gp::FeatureSet& set = bound->dataset.set;
    if (set.tables.empty()) {
        records.insert(records.begin(), vec::notRunRecord("buffer", ""));
        return vec::answered("GIS BUFFER", vec::joined(records));
    }
    if (!plan.property.empty() && !vec::hasField(set, plan.property)) {
        return makeError(ErrorCode::InvalidArgument,
                         "no entity in the scope has the property distance=prop: names",
                         plan.property);
    }
    for (const std::string& key : plan.dissolveBy) {
        if (!vec::hasField(set, key)) {
            return makeError(ErrorCode::InvalidArgument,
                             "no entity in the scope has the property dissolve= names", key);
        }
    }
    if (words->has("PREVIEW")) {
        std::size_t skipped = 0;
        const auto groups = byDistance(set, plan, skipped);
        records.insert(records.begin(), "gis op=buffer preview=yes");
        records.push_back("buffer groups=" + std::to_string(groups.size()) +
                          (plan.distance ? std::string()
                                         : " skipped.no_distance=" + std::to_string(skipped)));
        records.push_back("preview valid=yes changed=no");
        return vec::answered("GIS BUFFER", vec::joined(records));
    }

    const auto input = std::make_shared<const gp::FeatureSet>(set);
    const std::string commandName(katana::core::trimmed(line));
    Prepared prepared;
    prepared.title = "GIS BUFFER";
    prepared.work = [input, plan, records, commandName](const std::stop_token& stop,
                                                         const Progress& progress) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        std::vector<std::string> warnings;
        std::size_t skipped = 0;
        std::map<double, gp::FeatureSet> groups = byDistance(*input, plan, skipped);
        // A round buffer of a point is a circle: drawn as one, exactly, when
        // nothing is to be merged with it.
        std::vector<CircleResult> circles;
        std::size_t empty = 0;
        const bool pointsAsCircles = plan.caps == "round" && !plan.dissolve;
        std::vector<gp::FeatureTable> buffered;
        std::size_t done = 0;
        for (auto& [distance, group] : groups) {
            if (pointsAsCircles) {
                for (auto table = group.tables.begin(); table != group.tables.end();) {
                    if (table->kind != katana::gis::GeometryKind::Point) {
                        ++table;
                        continue;
                    }
                    for (const gp::Feature& feature : table->features) {
                        if (distance < 0.0 || feature.parts.empty() ||
                            feature.parts.front().parts.empty() ||
                            feature.parts.front().parts.front().empty()) {
                            ++empty; // a point buffered inwards is nothing
                            continue;
                        }
                        CircleResult circle;
                        const katana::gis::GeoPoint& at = feature.parts.front().parts.front().front();
                        circle.centre = katana::geometry::Point2(at.x, at.y);
                        circle.radius = distance;
                        for (std::size_t f = 0; f < table->fields.size() && f < feature.values.size();
                             ++f) {
                            if (igeo::isBookkeepingField(table->fields[f].name)) {
                                continue;
                            }
                            if (auto property = vec::propertyOf(feature.values[f])) {
                                circle.properties[table->fields[f].name] = std::move(*property);
                            }
                        }
                        circle.properties["gis.op"] = std::string("vector buffer");
                        if (const auto id = vec::featureId(*table, feature)) {
                            circle.properties["gis.source"] = static_cast<std::int64_t>(*id);
                        }
                        circles.push_back(std::move(circle));
                    }
                    table = group.tables.erase(table);
                }
            }
            if (!group.tables.empty()) {
                const std::vector<std::string> gdalWords{
                    "--distance=" + number(distance), "--endcap-style=" + plan.caps,
                    "--join-style=" + plan.joins, "--side=" + plan.side,
                    "--quadrant-segments=" + std::to_string(quadrantSegments(distance))};
                const double from = static_cast<double>(done) / static_cast<double>(groups.size());
                const double to = static_cast<double>(done + 1) / static_cast<double>(groups.size());
                auto out = vec::runVector({"vector", "buffer"}, group, gdalWords, stop,
                                          vec::progressSpan(progress, plan.dissolve ? from * 0.5 : from,
                                                            plan.dissolve ? to * 0.5 : to),
                                          warnings);
                if (!out) {
                    return out.error();
                }
                for (gp::FeatureTable& table : out->tables) {
                    buffered.push_back(std::move(table));
                }
            }
            ++done;
        }
        std::vector<const gp::FeatureTable*> tables;
        for (const gp::FeatureTable& table : buffered) {
            tables.push_back(&table);
        }
        gp::FeatureSet features;
        if (!tables.empty()) {
            features.tables.push_back(vec::mergedTable(tables, "buffer"));
        }
        empty += vec::measure(features).empty;
        features = vec::withoutEmpty(std::move(features));
        if (plan.dissolve && !features.tables.empty()) {
            auto merged = dissolved(features, plan.dissolveBy, false, "buffer", stop,
                                    vec::progressSpan(progress, 0.5, 1.0), warnings);
            if (!merged) {
                return merged.error();
            }
            features = std::move(merged).value();
        }
        const vec::Measures measures = vec::measure(features);
        double circleArea = 0.0;
        for (const CircleResult& circle : circles) {
            circleArea += katana::math::kPi * circle.radius * circle.radius;
        }
        std::string summary = bufferRecord(plan, groups.size(), measures, circles.size(), circleArea,
                                           empty, skipped);
        const double seconds = secondsSince(start);
        auto kept = std::make_shared<gp::FeatureSet>(std::move(features));
        auto circleList = std::make_shared<std::vector<CircleResult>>(std::move(circles));
        return Apply([kept, circleList, plan, records, summary, warnings, seconds,
                      commandName](Context& ctx) -> Result<std::string> {
            const katana::entity::Model& model = ctx.document.model();
            igeo::ResultOptions options;
            options.targetLayer = plan.layer;
            options.operation = "vector buffer";
            options.commandName = commandName;
            auto made = igeo::resultCommand(model, *kept, options);
            if (!made) {
                return made.error();
            }
            auto step = std::make_unique<cmd::Transaction>(commandName);
            const bool layerMade = std::ranges::find(made->layers, plan.layer) != made->layers.end();
            if (made->command) {
                step->add(std::move(made->command));
            }
            if (!circleList->empty()) {
                if (!layerMade && !model.layers.contains(plan.layer)) {
                    katana::entity::Layer layer;
                    layer.name = plan.layer;
                    step->add(cmd::createLayer(layer));
                }
                std::vector<katana::entity::Entity> entities;
                for (const CircleResult& circle : *circleList) {
                    katana::entity::Entity entity;
                    entity.geometry = katana::geometry::Circle2{circle.centre, circle.radius};
                    entity.layer = plan.layer;
                    entity.properties = circle.properties;
                    entities.push_back(std::move(entity));
                }
                step->add(cmd::createEntities(std::move(entities)));
            }
            vec::Applied applied;
            applied.created = made->created + circleList->size();
            applied.skipped = made->skipped;
            auto created = vec::executeStep(ctx, step->size() == 0 ? nullptr : std::move(step));
            if (!created) {
                return created.error();
            }
            std::vector<std::string> reply{vec::gisRecord("buffer", seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.push_back(vec::outputRecord(plan.layer, applied));
            reply.push_back(summary);
            for (const std::string& warning : made->warnings) {
                reply.push_back(warningRecord(warning));
            }
            for (const std::string& warning : warnings) {
                reply.push_back(warningRecord(warning));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

// ---- GIS DISSOLVE ------------------------------------------------------------------------------

Result<Prepared> prepareDissolve(Context& context, const Tokens& tokens, std::string_view line)
{
    const vec::WordRules rules{{"by", "keep"}, {"REPLACE", "PREVIEW"}, true, kDissolveUsage};
    auto words = vec::readVerbWords(tokens, 2, tokens.size(), rules);
    if (!words) {
        return words.error();
    }
    const std::vector<std::string> keys =
        words->option("by") != nullptr ? vec::listOf(*words->option("by")) : std::vector<std::string>{};
    if (words->option("by") != nullptr && keys.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "by= names the properties to merge by; without it everything merges",
                         "by=");
    }
    auto keep = vec::choiceOption(*words, "keep", {"identical"}, "");
    if (!keep) {
        return keep.error();
    }
    const bool keepIdentical = *keep == "identical";
    auto layer = vec::layerTarget(*words, "GIS DISSOLVE", "gis/dissolve");
    if (!layer) {
        return layer.error();
    }
    if (auto valid = checkLayer(*layer); !valid) {
        return valid.error();
    }
    const bool replace = words->has("REPLACE");
    auto bound = vec::bindScope(context, words->scope);
    if (!bound) {
        return bound.error();
    }
    std::vector<std::string> records{scopeRecord("input", *bound)};
    for (const std::string& warning : bound->dataset.stats.warnings) {
        records.push_back(warningRecord(warning));
    }
    const gp::FeatureTable* areas = vec::tableNamed(bound->dataset.set, "polygons");
    const std::size_t others = bound->dataset.stats.points + bound->dataset.stats.lines;
    if (others != 0) {
        records.push_back(warningRecord(std::to_string(others) +
                                        " points and lines in the scope are not areas; a dissolve "
                                        "merges areas and leaves them as they are"));
    }
    if (areas == nullptr) {
        records.insert(records.begin(), vec::notRunRecord("dissolve", "areas=0"));
        return vec::answered("GIS DISSOLVE", vec::joined(records));
    }
    for (const std::string& key : keys) {
        if (!vec::fieldIndex(*areas, key)) {
            return makeError(ErrorCode::InvalidArgument,
                             "no area in the scope has the property by= names", key);
        }
    }
    // REPLACE deletes what went in: every area, and every hole joined into
    // one - read from the areas table itself, so what is deleted is what the
    // one conversion made an area (a closed polyline or curve polyline, a
    // circle, a whole ellipse, a closed spline), never a second rule of it.
    std::set<EntityId> wentIn;
    for (const gp::Feature& feature : areas->features) {
        if (const auto id = vec::featureId(*areas, feature)) {
            wentIn.insert(*id);
            if (const auto holes = bound->dataset.holes.find(*id);
                holes != bound->dataset.holes.end()) {
                wentIn.insert(holes->second.begin(), holes->second.end());
            }
        }
    }
    std::vector<EntityId> sources;
    for (const EntityId id : bound->match.matched) {
        if (wentIn.contains(id)) {
            sources.push_back(id);
        }
    }
    std::string byText;
    for (const std::string& key : keys) {
        byText += (byText.empty() ? "" : ",") + key;
    }
    const std::string settings = "by=" + value(byText) +
                                 " keep=" + (keepIdentical ? "identical" : "none") +
                                 " areas=" + std::to_string(areas->features.size()) +
                                 " replace=" + (replace ? "yes" : "no");
    if (words->has("PREVIEW")) {
        records.insert(records.begin(), "gis op=dissolve preview=yes");
        records.push_back("dissolve " + settings);
        records.push_back("preview valid=yes changed=no");
        return vec::answered("GIS DISSOLVE", vec::joined(records));
    }

    gp::FeatureSet input;
    input.tables.push_back(*areas);
    const auto kept = std::make_shared<const gp::FeatureSet>(std::move(input));
    const auto copies = std::make_shared<const std::vector<katana::entity::Entity>>(
        replace ? vec::copiesOf(context.document, sources) : std::vector<katana::entity::Entity>{});
    const std::string commandName(katana::core::trimmed(line));
    const std::string target = *layer;
    Prepared prepared;
    prepared.title = "GIS DISSOLVE";
    prepared.work = [kept, copies, keys, keepIdentical, replace, sources, target, settings, records,
                     commandName](const std::stop_token& stop,
                                  const Progress& progress) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        std::vector<std::string> warnings;
        auto merged = dissolved(*kept, keys, keepIdentical, "dissolve", stop, progress, warnings);
        if (!merged) {
            return merged.error();
        }
        const vec::Measures measures = vec::measure(*merged);
        const std::string summary = "dissolve " + settings +
                                    " groups=" + std::to_string(measures.features) +
                                    " area=" + fixed3(measures.area);
        const double seconds = secondsSince(start);
        auto result = std::make_shared<gp::FeatureSet>(std::move(merged).value());
        return Apply([result, copies, replace, sources, target, summary, records, warnings, seconds,
                      commandName](Context& ctx) -> Result<std::string> {
            if (replace) {
                // Deleting what changed since would lose a concurrent edit.
                if (auto same = unchangedSince(ctx, *copies); !same) {
                    return same.error();
                }
            }
            igeo::ResultOptions options;
            options.targetLayer = target;
            options.operation = "vector dissolve";
            options.commandName = commandName;
            auto made = igeo::resultCommand(ctx.document.model(), *result, options);
            if (!made) {
                return made.error();
            }
            vec::Applied applied;
            applied.created = made->created;
            applied.skipped = made->skipped;
            auto step = std::make_unique<cmd::Transaction>(commandName);
            if (made->command) {
                step->add(std::move(made->command));
            }
            if (replace && !sources.empty()) {
                applied.deleted = sources.size();
                step->add(cmd::deleteEntities(sources));
            }
            auto created = vec::executeStep(ctx, step->size() == 0 ? nullptr : std::move(step));
            if (!created) {
                return created.error();
            }
            std::vector<std::string> reply{vec::gisRecord("dissolve", seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.push_back(vec::outputRecord(target, applied));
            reply.push_back(summary);
            for (const std::string& warning : made->warnings) {
                reply.push_back(warningRecord(warning));
            }
            for (const std::string& warning : warnings) {
                reply.push_back(warningRecord(warning));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

} // namespace

Result<Prepared> prepareGisBuffer(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareBuffer(context, tokens, line);
}

Result<Prepared> prepareGisDissolve(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareDissolve(context, tokens, line);
}

} // namespace katana::app::geo
