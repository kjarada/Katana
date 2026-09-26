// The RASTER SAMPLE and DRAPE verbs (docs/terrain.md, "Sampling and
// drape"): the height of the ground at points, reported, and given to the
// drawing's points and vertices in ONE undo step - the same on katana_cli,
// katana_mcp and the window's command line, where Terrain > Analysis > Drape
// and Sample Heights builds these lines.
//
//   RASTER SAMPLE SURFACE <name> | RASTER <id|name> | FILE <path> [AT x,y]...
//                 [<scope>] [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]
//   DRAPE SURFACE <name> | RASTER <id|name> | FILE <path> [<scope>]
//         [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]
//
// The ground is a surface read on its own triangles - exactly, never through
// a grid of it - or a raster's file, which GDAL interpolates
// (GDALRasterInterpolateAtPoint; bilinear unless method= says). A point off
// the ground, or beside a no-data cell, has no height: a sample says
// ground=no, and a drape leaves the vertex heightless and counts it. Absent
// is not zero.
//
// RASTER SAMPLE reads the points given AT, and the points the scope takes -
// the selection when no scope word and no AT is given. It changes nothing.
// DRAPE sets the heights of the points, lines and polylines the scope takes
// through cad::geo::drapeCommand (entity::setHeights), after comparing them
// with the copies taken when the line was prepared: an entity edited while
// the job ran is not written over.

#include <chrono>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <variant>
#include <vector>

#include "analysis_support.hpp"
#include "katana/cad/geo/drape.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/model.hpp"
#include "replies.hpp"
#include "vector_support.hpp"

namespace katana::app::geo {

namespace vec = katana::app::geo::vector;
namespace an = katana::app::geo::analysis;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;

namespace {

using Clock = std::chrono::steady_clock;

constexpr const char* kSampleUsage =
    "RASTER SAMPLE SURFACE <name> | RASTER <id|name> | FILE <path> [AT x,y]... [<scope>] "
    "[method=bilinear|nearest|cubic|cubicspline] [PREVIEW]";
constexpr const char* kDrapeUsage =
    "DRAPE SURFACE <name> | RASTER <id|name> | FILE <path> [<scope>] "
    "[method=bilinear|nearest|cubic|cubicspline] [PREVIEW]";

// method=, which only a raster has: a surface is linear on its triangles.
Result<katana::gis::Resampling> methodOf(const vec::VerbWords& words, const an::Ground& ground)
{
    const std::string* method = words.option("method");
    if (method == nullptr) {
        return katana::gis::Resampling::Bilinear;
    }
    if (ground.surface) {
        return makeError(ErrorCode::InvalidArgument,
                         "a surface is read on its own triangles; method= is for a raster",
                         "method=" + *method);
    }
    return katana::gis::resamplingNamed(*method);
}

std::string methodText(const an::Ground& ground, katana::gis::Resampling method)
{
    return ground.surface ? std::string("triangles") : std::string(katana::gis::toString(method));
}

// The matched entities, as the scope parser and matcher give them.
Result<katana::cad::ScopeMatch> matchOf(Context& context, const katana::cad::ScopeWords& scope)
{
    return katana::cad::matchScope(context.document, scope, context.interpreter.scopeContext());
}

std::string heightText(double z)
{
    return katana::core::formatExactReal(z);
}

} // namespace

// ---- RASTER SAMPLE --------------------------------------------------------------------------

Result<Prepared> prepareRasterSample(Context& context, const Tokens& tokens, std::string_view)
{
    std::size_t at = 2;
    auto ground = an::bindGround(context, tokens, at, "RASTER SAMPLE");
    if (!ground) {
        return ground.error();
    }
    auto split = an::takeKeywordValues(tokens, at, {"AT"});
    if (!split) {
        return split.error();
    }
    const vec::WordRules rules{{"method"}, {"PREVIEW"}, false, kSampleUsage};
    auto words = vec::readVerbWords(split->rest, 0, split->rest.size(), rules);
    if (!words) {
        return words.error();
    }
    auto method = methodOf(*words, *ground);
    if (!method) {
        return method.error();
    }
    std::vector<Point2> points;
    for (const auto& [keyword, text] : split->taken) {
        const auto point = an::pointOf(text);
        if (!point) {
            return makeError(ErrorCode::InvalidArgument, "AT is a point: AT 12.5,40", text);
        }
        points.push_back(*point);
    }
    std::vector<std::string> records{ground->record};
    // The points the scope takes: the selection's when neither a scope nor
    // an AT is given, as every verb's missing scope is.
    std::vector<std::pair<katana::entity::EntityId, Point2>> entities;
    if (words->scopeGiven || points.empty()) {
        auto match = matchOf(context, words->scope);
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
                entities.emplace_back(id, point->position);
            } else {
                ++skipped[katana::core::lowered(std::string(katana::entity::toString(entity->type())))];
            }
        }
        std::string record = "scope arg=input " + katana::cad::scopeRecord(*match) +
                             " points=" + std::to_string(entities.size());
        for (const auto& [type, count] : skipped) {
            record += " skipped." + type + "=" + std::to_string(count);
        }
        records.push_back(record);
    }
    const std::string settings = "samples method=" + methodText(*ground, *method);
    if (points.empty() && entities.empty()) {
        records.insert(records.begin(), vec::notRunRecord("sample", "points=0"));
        records.push_back(settings + " count=0");
        return vec::answered("RASTER SAMPLE", vec::joined(records));
    }
    if (words->has("PREVIEW")) {
        records.insert(records.begin(), "gis op=sample preview=yes");
        records.push_back(settings + " count=" + std::to_string(points.size() + entities.size()));
        records.push_back("preview valid=yes changed=no");
        return vec::answered("RASTER SAMPLE", vec::joined(records));
    }

    Prepared prepared;
    prepared.title = "Sample Heights";
    const an::Ground from = *ground;
    const katana::gis::Resampling how = *method;
    prepared.work = [from, how, points, entities, records,
                     settings](const std::stop_token& stop, const Progress&) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        auto open = an::openGround(from, how);
        if (!open) {
            return open.error();
        }
        std::vector<std::string> reply;
        std::size_t on = 0;
        const auto sampled = [&](const std::string& head, const Point2& where) {
            const std::optional<double> z = open->at(where);
            on += z ? 1U : 0U;
            reply.push_back(head + "at=" + an::pointText(where) +
                            (z ? " z=" + heightText(*z) : std::string(" ground=no")));
        };
        for (const Point2& point : points) {
            sampled("sample ", point);
        }
        for (const auto& [id, point] : entities) {
            if (stop.stop_requested()) {
                return makeError(ErrorCode::InvalidState, "cancelled");
            }
            sampled("sample entity=" + std::to_string(id) + " ", point);
        }
        const std::size_t count = points.size() + entities.size();
        std::string summary = settings + " count=" + std::to_string(count) +
                              " on=" + std::to_string(on) + " off=" + std::to_string(count - on);
        const double seconds = std::chrono::duration<double>(Clock::now() - start).count();
        auto lines = std::make_shared<std::vector<std::string>>(std::move(reply));
        return Apply([lines, records, summary, seconds, count, on](Context&) -> Result<std::string> {
            std::vector<std::string> out{vec::gisRecord("sample", seconds)};
            out.insert(out.end(), records.begin(), records.end());
            out.insert(out.end(), lines->begin(), lines->end());
            out.push_back(summary);
            if (on != count) {
                out.push_back(warningRecord(std::to_string(count - on) +
                                            " points lie off the ground and have no height"));
            }
            return vec::joined(out);
        });
    };
    return prepared;
}

// ---- DRAPE ----------------------------------------------------------------------------------

Result<Prepared> prepareDrape(Context& context, const Tokens& tokens, std::string_view line)
{
    std::size_t at = 1;
    auto ground = an::bindGround(context, tokens, at, "DRAPE");
    if (!ground) {
        return ground.error();
    }
    const vec::WordRules rules{{"method"}, {"PREVIEW"}, false, kDrapeUsage};
    auto words = vec::readVerbWords(tokens, at, tokens.size(), rules);
    if (!words) {
        return words.error();
    }
    auto method = methodOf(*words, *ground);
    if (!method) {
        return method.error();
    }
    auto match = matchOf(context, words->scope);
    if (!match) {
        return match.error();
    }
    // What has vertices a height belongs to; the rest is counted and left.
    std::vector<katana::entity::Entity> targets;
    std::map<std::string, std::size_t> skipped;
    std::size_t vertices = 0;
    for (const katana::entity::EntityId id : match->matched) {
        const katana::entity::Entity* entity = context.document.model().entities.find(id);
        if (entity == nullptr) {
            continue;
        }
        const std::size_t count = katana::cad::geo::heightVertices(*entity).size();
        if (count == 0) {
            ++skipped[katana::core::lowered(std::string(katana::entity::toString(entity->type())))];
            continue;
        }
        vertices += count;
        targets.push_back(*entity);
    }
    std::string skippedText;
    for (const auto& [type, count] : skipped) {
        skippedText += " skipped." + type + "=" + std::to_string(count);
    }
    std::vector<std::string> records{ground->record,
                                     "scope arg=input " + katana::cad::scopeRecord(*match) +
                                         " used=" + std::to_string(targets.size()) + skippedText};
    const std::string settings = "drape method=" + methodText(*ground, *method);
    if (targets.empty()) {
        records.insert(records.begin(), vec::notRunRecord("drape", "entities=0"));
        records.push_back(settings + " entities=0 vertices=0");
        return vec::answered("DRAPE", vec::joined(records));
    }
    if (words->has("PREVIEW")) {
        records.insert(records.begin(), "gis op=drape preview=yes");
        records.push_back(settings + " entities=" + std::to_string(targets.size()) +
                          " vertices=" + std::to_string(vertices));
        records.push_back("preview valid=yes changed=no");
        return vec::answered("DRAPE", vec::joined(records));
    }

    Prepared prepared;
    prepared.title = "Drape";
    const an::Ground from = *ground;
    const katana::gis::Resampling how = *method;
    const auto copies = std::make_shared<const std::vector<katana::entity::Entity>>(std::move(targets));
    const std::string commandName(katana::core::trimmed(line));
    prepared.work = [from, how, copies, records, settings,
                     commandName](const std::stop_token& stop, const Progress&) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        auto open = an::openGround(from, how);
        if (!open) {
            return open.error();
        }
        auto heights = std::make_shared<katana::cad::geo::DrapeHeights>(
            katana::cad::geo::drapeHeights(*copies, open->at, stop));
        if (heights->stopped || stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        const double seconds = std::chrono::duration<double>(Clock::now() - start).count();
        return Apply([heights, copies, records, settings, seconds,
                      commandName](Context& ctx) -> Result<std::string> {
            // In place: the entities as they were when the line was read.
            if (auto same = unchangedSince(ctx, *copies); !same) {
                return same.error();
            }
            katana::cad::geo::DrapeCommand made =
                katana::cad::geo::drapeCommand(ctx.document.model(), heights->entities, commandName);
            vec::Applied applied;
            applied.updated = made.changed;
            applied.skipped = made.missing;
            auto executed = vec::executeStep(ctx, std::move(made.command));
            if (!executed) {
                return executed.error();
            }
            std::vector<std::string> reply{vec::gisRecord("drape", seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.push_back(vec::outputRecord("", applied));
            reply.push_back(settings + " entities=" + std::to_string(heights->entities.size()) +
                            " vertices=" + std::to_string(heights->vertices) +
                            " off=" + std::to_string(heights->verticesOff) +
                            " entities_off=" + std::to_string(heights->entitiesOff) +
                            " unchanged=" + std::to_string(made.unchanged));
            if (heights->verticesOff != 0) {
                reply.push_back(warningRecord(std::to_string(heights->verticesOff) +
                                              " vertices lie off the ground and were left without "
                                              "a height"));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

} // namespace katana::app::geo
