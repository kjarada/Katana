// GIS CHECK, GIS REPAIR and GIS COVERAGE (docs/geoprocessing.md, "V4"):
// invalid geometry in imported data, and the gaps and overlaps of a
// subdivision or cadastral fabric - through GDAL's `vector check-geometry`,
// `make-valid`, `check-coverage` and `clean-coverage`, on the shared scope.
//
//   GIS CHECK [<scope>] [markers=<layer>] [PREVIEW]
//   GIS REPAIR [<scope>] [method=linework|structure] [PREVIEW]
//   GIS COVERAGE CHECK [<scope>] [gap=<m>] [markers=<layer>] [PREVIEW]
//   GIS COVERAGE CLEAN [<scope>] [gap=<m>] [snap=<m>]
//                      [merge=longest-border|max-area|min-area|min-index] REPLACE [PREVIEW]
//
// What was measured, and so is relied on here:
//   - check-geometry gives one feature per invalid input with its location
//     (a multipoint: a bow-tie's crossing) and GDAL's reason, and carries
//     katana_id when asked (--include-field);
//   - check-coverage refuses mixed geometry and carries no fields, but with
//     --include-valid gives one feature per input, in order - the invalid
//     edges, or nothing - which is how a problem is joined to its entity;
//   - clean-coverage gives every input back, in order and with its fields,
//     and closes only ENCLOSED gaps: a sliver open to the outside stays;
//   - both rewrite a ring's start and direction even where nothing moved, so
//     "changed" is judged by the ring, not by the vertex list.
//
// A check changes nothing but its markers. A repair and a clean change
// entities in place: the geometry is replaced (the id kept, so labels and
// associations survive), a result of several parts keeps the id on its
// largest and makes the rest, and the entities are compared with the copies
// taken at prepare before anything is written.

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
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "replies.hpp"
#include "vector_support.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace vec = katana::app::geo::vector;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::EntityId;
using katana::gis::GeoPoint;
using katana::gis::GeometryKind;
using katana::gis::VectorGeometry;

namespace {

constexpr const char* kCheckUsage = "GIS CHECK [<scope>] [markers=<layer>] [PREVIEW]";
constexpr const char* kRepairUsage = "GIS REPAIR [<scope>] [method=linework|structure] [PREVIEW]";
constexpr const char* kCoverageUsage =
    "GIS COVERAGE CHECK [<scope>] [gap=<m>] [markers=<layer>] [PREVIEW] | GIS COVERAGE CLEAN "
    "[<scope>] [gap=<m>] [snap=<m>] [merge=longest-border|max-area|min-area|min-index] REPLACE "
    "[PREVIEW]";

using Clock = std::chrono::steady_clock;

double secondsSince(Clock::time_point start)
{
    return std::chrono::duration<double>(Clock::now() - start).count();
}

std::string number(double value)
{
    return katana::core::formatExactReal(value);
}

// A place, to the millimetre as length= is, without trailing zeros: GEOS's
// arithmetic gives the middle of an edge from y = 0 to 40 as
// 20.000000000000007, which is 20 to any survey. (+ 0.0 makes a -0 of a
// rounded -0.0004 read 0.)
std::string at(const katana::geometry::Point2& point)
{
    const auto millimetres = [](double value) {
        return number(std::round(value * 1000.0) / 1000.0 + 0.0);
    };
    return millimetres(point.x) + "," + millimetres(point.y);
}

// A problem's words: GDAL's reason, lower case, blanks as hyphens.
std::string kindOf(const std::string& reason)
{
    std::string kind = katana::core::lowered(katana::core::trimmed(reason));
    std::ranges::replace(kind, ' ', '-');
    return kind.empty() ? std::string("invalid") : kind;
}

katana::core::Status checkMarkersLayer(const std::string* layer)
{
    if (layer == nullptr) {
        return {};
    }
    if (auto valid = katana::entity::validateLayerPath(*layer); !valid) {
        return makeError(ErrorCode::InvalidArgument,
                         "markers= needs a layer path: " + valid.error().message, *layer);
    }
    return {};
}

// A positive length option; nullopt when not given.
Result<std::optional<double>> lengthOption(const vec::VerbWords& words, const char* key)
{
    auto length = vec::numberOption(words, key);
    if (!length) {
        return length.error();
    }
    if (*length && !(**length > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, std::string(key) + "= is a length above 0",
                         std::string(key) + "=" + *words.option(key));
    }
    return *length;
}

// The drawing's features the verb reads, what its scope said, and the
// records that say so.
struct Scoped {
    BoundDrawing bound;
    std::vector<std::string> records;
};

Result<Scoped> scoped(Context& context, const katana::cad::ScopeWords& scope)
{
    auto bound = vec::bindScope(context, scope);
    if (!bound) {
        return bound.error();
    }
    Scoped out{std::move(bound).value(), {}};
    out.records.push_back(scopeRecord("input", out.bound));
    for (const std::string& warning : out.bound.dataset.stats.warnings) {
        out.records.push_back(warningRecord(warning));
    }
    return out;
}

// A problem found: what, which entity, where, and GDAL's reason.
struct Problem {
    std::string kind;
    std::optional<EntityId> entity;
    katana::geometry::Point2 at;
    std::string reason;
    std::optional<double> length;
    // A coverage problem's edge, drawn as a marker.
    std::vector<katana::geometry::Point2> edge;
};

std::string problemRecord(const Problem& problem)
{
    std::string record = "problem kind=" + value(problem.kind) +
                         " entity=" + (problem.entity ? std::to_string(*problem.entity) : "") +
                         " at=" + at(problem.at);
    if (problem.length) {
        record += " length=" + fixed3(*problem.length);
    }
    return record + " reason=" + value(problem.reason);
}

// The markers of `problems`, on `layer`, replacing the ones a previous run
// of `kind` left there, as one step; the output record.
Result<std::string> applyMarkers(Context& context, const std::string& layer, const std::string& kind,
                                 const std::vector<Problem>& problems, const std::string& commandName)
{
    std::vector<katana::entity::Entity> markers;
    for (const Problem& problem : problems) {
        katana::entity::Entity marker;
        if (problem.edge.size() >= 2) {
            katana::geometry::Polyline2 line;
            line.vertices = problem.edge;
            marker.geometry = std::move(line);
        } else {
            marker.geometry = katana::entity::PointGeometry{problem.at};
        }
        marker.properties["gis.problem"] = problem.kind;
        marker.properties["gis.reason"] = problem.reason;
        if (problem.entity) {
            marker.properties["gis.source"] = static_cast<std::int64_t>(*problem.entity);
        }
        markers.push_back(std::move(marker));
    }
    std::size_t removed = 0;
    vec::Applied applied;
    applied.created = markers.size();
    auto command = vec::replaceMarkers(context.document.model(), layer, kind, std::move(markers),
                                       commandName, removed);
    applied.deleted = removed;
    if (auto done = vec::executeStep(context, std::move(command)); !done) {
        return done.error();
    }
    std::string record = vec::outputRecord(layer, applied);
    record.replace(record.find("arg=output"), 10, "arg=markers");
    return record;
}

// ---- GIS CHECK -------------------------------------------------------------------------------

Result<Prepared> prepareCheck(Context& context, const Tokens& tokens, std::string_view line)
{
    const vec::WordRules rules{{"markers"}, {"PREVIEW"}, false, kCheckUsage};
    auto words = vec::readVerbWords(tokens, 2, tokens.size(), rules);
    if (!words) {
        return words.error();
    }
    const std::string* markers = words->option("markers");
    if (auto valid = checkMarkersLayer(markers); !valid) {
        return valid.error();
    }
    auto scope = scoped(context, words->scope);
    if (!scope) {
        return scope.error();
    }
    const gp::FeatureSet& set = scope->bound.dataset.set;
    // Points are always valid: what can be wrong is a line or an area.
    gp::FeatureSet input;
    std::size_t features = 0;
    for (const gp::FeatureTable& table : set.tables) {
        if (table.kind != GeometryKind::Point) {
            input.tables.push_back(table);
            features += table.features.size();
        }
    }
    if (input.tables.empty() && markers == nullptr) {
        std::vector<std::string> reply{vec::notRunRecord("check", "")};
        reply.insert(reply.end(), scope->records.begin(), scope->records.end());
        reply.push_back("check features=0 problems=0 entities=0");
        return vec::answered("GIS CHECK", vec::joined(reply));
    }
    if (words->has("PREVIEW")) {
        std::vector<std::string> reply{"gis op=check preview=yes"};
        reply.insert(reply.end(), scope->records.begin(), scope->records.end());
        reply.push_back("check features=" + std::to_string(features));
        reply.push_back("preview valid=yes changed=no");
        return vec::answered("GIS CHECK", vec::joined(reply));
    }
    const auto kept = std::make_shared<const gp::FeatureSet>(std::move(input));
    const std::optional<std::string> markerLayer =
        markers != nullptr ? std::optional<std::string>(*markers) : std::nullopt;
    const std::vector<std::string> records = scope->records;
    const std::string commandName(katana::core::trimmed(line));
    Prepared prepared;
    prepared.title = "GIS CHECK";
    prepared.work = [kept, features, markerLayer, records, commandName](
                        const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        std::vector<std::string> warnings;
        std::vector<Problem> problems;
        if (!kept->tables.empty()) {
            auto found = vec::runVector({"vector", "check-geometry"}, *kept,
                                        {"--include-field=katana_id"}, stop, progress, warnings);
            if (!found) {
                return found.error();
            }
            for (const gp::FeatureTable& table : found->tables) {
                const auto reason = vec::fieldIndex(table, "error");
                for (const gp::Feature& feature : table.features) {
                    Problem problem;
                    problem.entity = vec::featureId(table, feature);
                    if (reason && *reason < feature.values.size()) {
                        if (const auto* text = std::get_if<std::string>(&feature.values[*reason])) {
                            problem.reason = *text;
                        }
                    }
                    problem.kind = kindOf(problem.reason);
                    // One problem at each place GDAL found one.
                    bool placed = false;
                    for (const VectorGeometry& part : feature.parts) {
                        for (const auto& ring : part.parts) {
                            for (const GeoPoint& point : ring) {
                                Problem one = problem;
                                one.at = katana::geometry::Point2(point.x, point.y);
                                problems.push_back(std::move(one));
                                placed = true;
                            }
                        }
                    }
                    if (!placed) {
                        problems.push_back(problem);
                    }
                }
            }
        }
        std::set<EntityId> entities;
        for (const Problem& problem : problems) {
            if (problem.entity) {
                entities.insert(*problem.entity);
            }
        }
        const std::string summary = "check features=" + std::to_string(features) +
                                    " problems=" + std::to_string(problems.size()) +
                                    " entities=" + std::to_string(entities.size());
        const double seconds = secondsSince(start);
        return Apply([problems, markerLayer, records, summary, warnings, seconds,
                      commandName](Context& ctx) -> Result<std::string> {
            std::vector<std::string> reply{vec::gisRecord("check", seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            for (const Problem& problem : problems) {
                reply.push_back(problemRecord(problem));
            }
            reply.push_back(summary);
            if (markerLayer) {
                auto output = applyMarkers(ctx, *markerLayer, "check", problems, commandName);
                if (!output) {
                    return output.error();
                }
                reply.push_back(*output);
            }
            for (const std::string& warning : warnings) {
                reply.push_back(warningRecord(warning));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

// ---- reshaping: GIS REPAIR and GIS COVERAGE CLEAN ----------------------------------------------

// The entities whose features an algorithm gave back changed, with what
// they became; `unchanged` counts those it gave back as they were.
std::vector<vec::Reshaped> changedBy(const gp::FeatureSet& before, const gp::FeatureSet& after,
                                     std::size_t& unchanged)
{
    const auto was = vec::featuresById(before);
    std::vector<vec::Reshaped> changed;
    for (const gp::FeatureTable& table : after.tables) {
        for (const gp::Feature& feature : table.features) {
            const auto id = vec::featureId(table, feature);
            const auto found = id ? was.find(*id) : was.end();
            if (found == was.end()) {
                continue;
            }
            if (vec::sameShape(*found->second, feature)) {
                ++unchanged;
                continue;
            }
            vec::Reshaped one;
            one.id = *id;
            one.parts = feature.parts;
            one.before = *found->second;
            changed.push_back(std::move(one));
        }
    }
    return changed;
}

// The ids of the features of `set`: the entities an in-place apply may
// change, copied at prepare to be compared at apply.
std::vector<EntityId> idsOf(const gp::FeatureSet& set)
{
    std::vector<EntityId> ids;
    for (const auto& [id, feature] : vec::featuresById(set)) {
        ids.push_back(id);
    }
    return ids;
}

// The apply of a reshape: compared, then ONE command; the records.
Result<std::size_t> applyReshape(Context& context, const std::vector<katana::entity::Entity>& copies,
                                 const std::vector<vec::Reshaped>& changed, const std::string& op,
                                 const std::string& commandName, std::vector<std::string>& records)
{
    // Changing what changed since would overwrite a concurrent edit.
    if (auto same = unchangedSince(context, copies); !same) {
        return same.error();
    }
    auto reshape = vec::reshapeCommand(context.document.model(), changed, op, commandName);
    if (!reshape) {
        return reshape.error();
    }
    auto created = vec::executeStep(context, std::move(reshape->command));
    if (!created) {
        return created.error();
    }
    for (std::string& split : vec::splitRecords(reshape->splits, *created)) {
        records.push_back(std::move(split));
    }
    vec::Applied applied;
    applied.created = reshape->created;
    applied.updated = reshape->updated;
    applied.skipped = reshape->left;
    records.push_back(vec::outputRecord("", applied));
    for (const std::string& warning : reshape->warnings) {
        records.push_back(warningRecord(warning));
    }
    return reshape->splits.size();
}

// ---- GIS REPAIR -------------------------------------------------------------------------------

Result<Prepared> prepareRepair(Context& context, const Tokens& tokens, std::string_view line)
{
    const vec::WordRules rules{{"method"}, {"PREVIEW"}, false, kRepairUsage};
    auto words = vec::readVerbWords(tokens, 2, tokens.size(), rules);
    if (!words) {
        return words.error();
    }
    auto method = vec::choiceOption(*words, "method", {"linework", "structure"}, "linework");
    if (!method) {
        return method.error();
    }
    auto scope = scoped(context, words->scope);
    if (!scope) {
        return scope.error();
    }
    gp::FeatureSet input;
    for (const gp::FeatureTable& table : scope->bound.dataset.set.tables) {
        if (table.kind != GeometryKind::Point) {
            input.tables.push_back(table);
        }
    }
    const std::vector<EntityId> ids = idsOf(input);
    const std::string settings = "repair method=" + *method + " features=" + std::to_string(ids.size());
    if (input.tables.empty()) {
        std::vector<std::string> reply{vec::notRunRecord("repair", "")};
        reply.insert(reply.end(), scope->records.begin(), scope->records.end());
        reply.push_back(settings + " valid=0 repaired=0 split=0");
        return vec::answered("GIS REPAIR", vec::joined(reply));
    }
    if (words->has("PREVIEW")) {
        std::vector<std::string> reply{"gis op=repair preview=yes"};
        reply.insert(reply.end(), scope->records.begin(), scope->records.end());
        reply.push_back(settings);
        reply.push_back("preview valid=yes changed=no");
        return vec::answered("GIS REPAIR", vec::joined(reply));
    }
    const auto kept = std::make_shared<const gp::FeatureSet>(std::move(input));
    const auto copies =
        std::make_shared<const std::vector<katana::entity::Entity>>(vec::copiesOf(context.document, ids));
    const std::vector<std::string> records = scope->records;
    const std::string commandName(katana::core::trimmed(line));
    const std::string how = *method;
    Prepared prepared;
    prepared.title = "GIS REPAIR";
    prepared.work = [kept, copies, how, settings, records, commandName](
                        const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        std::vector<std::string> warnings;
        auto repaired = vec::runVector({"vector", "make-valid"}, *kept, {"--method=" + how}, stop,
                                       progress, warnings);
        if (!repaired) {
            return repaired.error();
        }
        std::size_t unchanged = 0;
        auto changed = std::make_shared<const std::vector<vec::Reshaped>>(
            changedBy(*kept, *repaired, unchanged));
        const double seconds = secondsSince(start);
        return Apply([changed, copies, unchanged, settings, records, warnings, seconds,
                      commandName](Context& ctx) -> Result<std::string> {
            std::vector<std::string> reply{vec::gisRecord("repair", seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            auto splits = applyReshape(ctx, *copies, *changed, "vector make-valid", commandName, reply);
            if (!splits) {
                return splits.error();
            }
            reply.push_back(settings + " valid=" + std::to_string(unchanged) +
                            " repaired=" + std::to_string(changed->size()) +
                            " split=" + std::to_string(*splits));
            for (const std::string& warning : warnings) {
                reply.push_back(warningRecord(warning));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

// ---- GIS COVERAGE ------------------------------------------------------------------------------

struct Ring {
    katana::geometry::Polyline2 exterior;
    std::vector<katana::geometry::Polyline2> holes;
};

katana::geometry::Polyline2 ringOf(const std::vector<GeoPoint>& points)
{
    katana::geometry::Polyline2 ring;
    ring.closed = true;
    for (const GeoPoint& point : points) {
        ring.vertices.emplace_back(point.x, point.y);
    }
    return ring;
}

// Strictly inside an area: inside its exterior and none of its holes, on
// neither boundary.
bool strictlyInside(const Ring& area, const katana::geometry::Point2& point)
{
    if (area.exterior.classify(point) != katana::geometry::Containment::Inside) {
        return false;
    }
    return std::ranges::none_of(area.holes, [&](const katana::geometry::Polyline2& hole) {
        return hole.classify(point) != katana::geometry::Containment::Outside;
    });
}

double boundaryDistance(const Ring& area, const katana::geometry::Point2& point)
{
    double best = area.exterior.distanceTo(point).value_or(1e300);
    for (const katana::geometry::Polyline2& hole : area.holes) {
        best = std::min(best, hole.distanceTo(point).value_or(1e300));
    }
    return best;
}

// What an invalid edge of area `self` is, from where its segments' middles
// lie against the other areas: inside one (an overlap); outside all, within
// `gap` of one (a gap GDAL was asked for); or on another's boundary without
// sharing its vertices (edges that do not match).
std::string coverageKind(const std::vector<Ring>& areas, std::size_t self,
                         const std::vector<katana::geometry::Point2>& edge, std::optional<double> gap)
{
    bool nearGap = false;
    for (std::size_t s = 1; s < edge.size(); ++s) {
        const katana::geometry::Point2 middle = (edge[s - 1] + edge[s]) * 0.5;
        bool inside = false;
        bool covered = false;
        double nearest = 1e300;
        for (std::size_t a = 0; a < areas.size(); ++a) {
            if (a == self) {
                continue;
            }
            inside = inside || strictlyInside(areas[a], middle);
            covered = covered || areas[a].exterior.classify(middle) !=
                                     katana::geometry::Containment::Outside;
            nearest = std::min(nearest, boundaryDistance(areas[a], middle));
        }
        if (inside) {
            return "overlap";
        }
        nearGap = nearGap || (gap && !covered && nearest > katana::math::tolerance::kGeometric &&
                              nearest <= *gap + katana::math::tolerance::kGeometric);
    }
    return nearGap ? "gap" : "mismatch";
}

std::string coverageReason(const std::string& kind)
{
    if (kind == "overlap") {
        return "the area overlaps a neighbour along this edge";
    }
    if (kind == "gap") {
        return "a gap narrower than gap= lies along this edge";
    }
    return "the edge meets a neighbour's without sharing its vertices";
}

double lengthOf(const std::vector<katana::geometry::Point2>& edge)
{
    double length = 0.0;
    for (std::size_t s = 1; s < edge.size(); ++s) {
        length += edge[s - 1].distanceTo(edge[s]);
    }
    return length;
}

// The point halfway along an edge.
katana::geometry::Point2 middleOf(const std::vector<katana::geometry::Point2>& edge)
{
    const double half = lengthOf(edge) / 2.0;
    double walked = 0.0;
    for (std::size_t s = 1; s < edge.size(); ++s) {
        const double step = edge[s - 1].distanceTo(edge[s]);
        if (walked + step >= half && step > 0.0) {
            return edge[s - 1] + (edge[s] - edge[s - 1]) * ((half - walked) / step);
        }
        walked += step;
    }
    return edge.empty() ? katana::geometry::Point2() : edge.front();
}

Result<Prepared> prepareCoverage(Context& context, const Tokens& tokens, std::string_view line)
{
    if (tokens.size() < 3 || !(tokens.is(2, "CHECK") || tokens.is(2, "CLEAN"))) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string("GIS COVERAGE takes CHECK or CLEAN: ") + kCoverageUsage,
                         tokens.size() > 2 ? tokens[2] : std::string());
    }
    const bool clean = tokens.is(2, "CLEAN");
    const vec::WordRules rules =
        clean ? vec::WordRules{{"gap", "snap", "merge"}, {"REPLACE", "PREVIEW"}, false, kCoverageUsage}
              : vec::WordRules{{"gap", "markers"}, {"PREVIEW"}, false, kCoverageUsage};
    auto words = vec::readVerbWords(tokens, 3, tokens.size(), rules);
    if (!words) {
        return words.error();
    }
    auto gap = lengthOption(*words, "gap");
    if (!gap) {
        return gap.error();
    }
    auto snap = lengthOption(*words, "snap");
    if (!snap) {
        return snap.error();
    }
    auto merge = vec::choiceOption(*words, "merge",
                                   {"longest-border", "max-area", "min-area", "min-index"}, "");
    if (!merge) {
        return merge.error();
    }
    const std::string* markers = words->option("markers");
    if (auto valid = checkMarkersLayer(markers); !valid) {
        return valid.error();
    }
    if (clean && !words->has("REPLACE")) {
        return makeError(ErrorCode::InvalidArgument,
                         "GIS COVERAGE CLEAN moves boundaries: add REPLACE to change the areas in "
                         "place. Surveyed legal boundaries must not be adjusted silently; GIS "
                         "COVERAGE CHECK reports first");
    }
    auto scope = scoped(context, words->scope);
    if (!scope) {
        return scope.error();
    }
    // check-coverage refuses anything but polygons (measured): the scope's
    // points and lines are left out, and counted.
    const auto& stats = scope->bound.dataset.stats;
    const std::size_t ignored = stats.points + stats.lines;
    if (ignored != 0) {
        scope->records.push_back(warningRecord(
            std::to_string(ignored) +
            " points and lines in the scope are not areas; a coverage is of areas only"));
    }
    gp::FeatureSet input;
    if (const gp::FeatureTable* areas = vec::tableNamed(scope->bound.dataset.set, "polygons")) {
        input.tables.push_back(*areas);
    }
    const std::size_t areaCount = input.tables.empty() ? 0 : input.tables.front().features.size();
    std::string settings = "coverage mode=" + std::string(clean ? "clean" : "check") +
                           " gap=" + (*gap ? number(**gap) : std::string());
    if (clean) {
        settings += " snap=" + (*snap ? number(**snap) : std::string()) + " merge=" + *merge;
    }
    settings += " areas=" + std::to_string(areaCount) + " ignored=" + std::to_string(ignored);
    if (input.tables.empty() && markers == nullptr) {
        std::vector<std::string> reply{vec::notRunRecord("coverage", "")};
        reply.insert(reply.end(), scope->records.begin(), scope->records.end());
        reply.push_back(settings);
        return vec::answered("GIS COVERAGE", vec::joined(reply));
    }
    if (words->has("PREVIEW")) {
        std::vector<std::string> reply{"gis op=coverage preview=yes"};
        reply.insert(reply.end(), scope->records.begin(), scope->records.end());
        reply.push_back(settings);
        reply.push_back("preview valid=yes changed=no");
        return vec::answered("GIS COVERAGE", vec::joined(reply));
    }

    const auto kept = std::make_shared<const gp::FeatureSet>(std::move(input));
    const std::vector<std::string> records = scope->records;
    const std::string commandName(katana::core::trimmed(line));
    const std::optional<double> gapWidth = *gap;
    Prepared prepared;
    prepared.title = "GIS COVERAGE";
    if (clean) {
        std::vector<std::string> gdalWords;
        if (gapWidth) {
            gdalWords.push_back("--maximum-gap-width=" + number(*gapWidth));
        }
        if (*snap) {
            gdalWords.push_back("--snapping-distance=" + number(**snap));
        }
        if (!merge->empty()) {
            gdalWords.push_back("--merge-strategy=" + *merge);
        }
        const auto copies = std::make_shared<const std::vector<katana::entity::Entity>>(
            vec::copiesOf(context.document, idsOf(*kept)));
        prepared.work = [kept, copies, gdalWords, settings, records, commandName](
                            const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
            const Clock::time_point start = Clock::now();
            std::vector<std::string> warnings;
            auto cleaned = vec::runVector({"vector", "clean-coverage"}, *kept, gdalWords, stop,
                                          progress, warnings);
            if (!cleaned) {
                return cleaned.error();
            }
            std::size_t unchanged = 0;
            auto changed = std::make_shared<const std::vector<vec::Reshaped>>(
                changedBy(*kept, *cleaned, unchanged));
            const double seconds = secondsSince(start);
            return Apply([changed, copies, settings, records, warnings, seconds,
                          commandName](Context& ctx) -> Result<std::string> {
                std::vector<std::string> reply{vec::gisRecord("coverage", seconds)};
                reply.insert(reply.end(), records.begin(), records.end());
                auto splits =
                    applyReshape(ctx, *copies, *changed, "vector clean-coverage", commandName, reply);
                if (!splits) {
                    return splits.error();
                }
                reply.push_back(settings + " changed=" + std::to_string(changed->size()));
                if (!changed->empty()) {
                    reply.push_back(warningRecord(
                        std::to_string(changed->size()) +
                        " boundaries moved: check them against the survey before relying on "
                        "them; one UNDO puts them back"));
                }
                for (const std::string& warning : warnings) {
                    reply.push_back(warningRecord(warning));
                }
                return vec::joined(reply);
            });
        };
        return prepared;
    }

    std::vector<std::string> gdalWords{"--include-valid"};
    if (gapWidth) {
        gdalWords.push_back("--maximum-gap-width=" + number(*gapWidth));
    }
    const std::optional<std::string> markerLayer =
        markers != nullptr ? std::optional<std::string>(*markers) : std::nullopt;
    prepared.work = [kept, gdalWords, gapWidth, markerLayer, settings, records, commandName](
                        const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        std::vector<std::string> warnings;
        std::vector<Problem> problems;
        if (!kept->tables.empty()) {
            auto found = vec::runVector({"vector", "check-coverage"}, *kept, gdalWords, stop,
                                        progress, warnings);
            if (!found) {
                return found.error();
            }
            const gp::FeatureTable& areas = kept->tables.front();
            std::vector<Ring> rings;
            for (const gp::Feature& feature : areas.features) {
                Ring ring;
                if (!feature.parts.empty() && !feature.parts.front().parts.empty()) {
                    ring.exterior = ringOf(feature.parts.front().parts.front());
                    for (std::size_t h = 1; h < feature.parts.front().parts.size(); ++h) {
                        ring.holes.push_back(ringOf(feature.parts.front().parts[h]));
                    }
                }
                rings.push_back(std::move(ring));
            }
            // --include-valid: one result per area, in order - how an edge
            // is joined to its entity, since check-coverage carries no field.
            std::size_t index = 0;
            for (const gp::FeatureTable& table : found->tables) {
                const bool joined = table.features.size() == areas.features.size();
                if (!joined) {
                    warnings.push_back("check-coverage gave back " +
                                       std::to_string(table.features.size()) + " results for " +
                                       std::to_string(areas.features.size()) +
                                       " areas; its problems name no entity");
                }
                for (const gp::Feature& feature : table.features) {
                    const std::size_t self = joined ? index : areas.features.size();
                    ++index;
                    for (const VectorGeometry& part : feature.parts) {
                        for (const auto& chain : part.parts) {
                            if (chain.size() < 2) {
                                continue;
                            }
                            Problem problem;
                            for (const GeoPoint& point : chain) {
                                problem.edge.emplace_back(point.x, point.y);
                            }
                            if (joined) {
                                problem.entity = vec::featureId(areas, areas.features[self]);
                            }
                            problem.kind = coverageKind(rings, self, problem.edge, gapWidth);
                            problem.reason = coverageReason(problem.kind);
                            problem.at = middleOf(problem.edge);
                            problem.length = lengthOf(problem.edge);
                            problems.push_back(std::move(problem));
                        }
                    }
                }
            }
        }
        std::set<EntityId> entities;
        std::map<std::string, std::size_t> kinds;
        for (const Problem& problem : problems) {
            if (problem.entity) {
                entities.insert(*problem.entity);
            }
            ++kinds[problem.kind];
        }
        const std::string summary = settings + " problems=" + std::to_string(problems.size()) +
                                    " entities=" + std::to_string(entities.size()) +
                                    " overlaps=" + std::to_string(kinds["overlap"]) +
                                    " gaps=" + std::to_string(kinds["gap"]) +
                                    " mismatches=" + std::to_string(kinds["mismatch"]);
        const double seconds = secondsSince(start);
        return Apply([problems, markerLayer, summary, records, warnings, seconds,
                      commandName](Context& ctx) -> Result<std::string> {
            std::vector<std::string> reply{vec::gisRecord("coverage", seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            for (const Problem& problem : problems) {
                reply.push_back(problemRecord(problem));
            }
            reply.push_back(summary);
            if (markerLayer) {
                auto output = applyMarkers(ctx, *markerLayer, "coverage", problems, commandName);
                if (!output) {
                    return output.error();
                }
                reply.push_back(*output);
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

Result<Prepared> prepareGisCheck(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareCheck(context, tokens, line);
}

Result<Prepared> prepareGisRepair(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareRepair(context, tokens, line);
}

Result<Prepared> prepareGisCoverage(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareCoverage(context, tokens, line);
}

} // namespace katana::app::geo
