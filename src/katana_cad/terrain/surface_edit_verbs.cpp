#include "katana/cad/terrain/surface_edit_verbs.hpp"

#include <algorithm>
#include <cmath>
#include <memory>
#include <numbers>
#include <set>
#include <utility>

#include "katana/cad/geo/surface_input.hpp"
#include "katana/cad/terrain/drawing_shapes.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/entity.hpp"
#include "katana/terrain/tin_edit.hpp"

namespace katana::cad {

namespace tin = katana::terrain;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::Polyline2;

namespace {

using Words = std::vector<std::string>;

bool is(std::string_view word, std::string_view keyword)
{
    return katana::core::equalsIgnoringCase(word, keyword);
}

std::string number(double value) { return katana::core::formatExactReal(value); }

// ---- typed coordinates -----------------------------------------------------------------

std::string describe(const Point2& at) { return number(at.x) + "," + number(at.y); }

Result<TypedCorner> requirePoint(const Words& words, std::size_t& at, std::string_view what)
{
    if (at >= words.size()) {
        return makeError(ErrorCode::ParseFailure, std::string(what) + " needs a point x,y");
    }
    const auto point = parseTypedCorner(words[at]);
    if (!point) {
        return makeError(ErrorCode::ParseFailure,
                         std::string(what) + " needs a point x,y, not this word", words[at]);
    }
    ++at;
    return *point;
}

Result<double> requireNumber(const Words& words, std::size_t& at, std::string_view what)
{
    if (at >= words.size()) {
        return makeError(ErrorCode::ParseFailure, std::string(what) + " needs a number");
    }
    const auto value = katana::core::parseFiniteDouble(words[at]);
    if (!value) {
        return makeError(ErrorCode::ParseFailure,
                         std::string(what) + " needs a number, not this word", words[at]);
    }
    ++at;
    return *value;
}

// key=value, the key in any case. nullopt when `word` is not one.
std::optional<std::pair<std::string, std::string>> option(std::string_view word)
{
    const std::size_t equals = word.find('=');
    if (equals == std::string_view::npos || equals == 0) {
        return std::nullopt;
    }
    return std::pair{katana::core::lowered(word.substr(0, equals)),
                     std::string(word.substr(equals + 1))};
}

// ---- the drawing's data: <polygon>, <line>, <points> ---------------------------------------
//
// Read by the one reader of the drawing's shapes (drawing_shapes.hpp), which
// the volumes read their polygons with too, so an entity is the same polygon
// to both; the replies and refusals here are this family's own words.

enum class DataKind { Polygon, Line, Points };

const char* nameOf(DataKind kind)
{
    switch (kind) {
    case DataKind::Polygon:
        return "a polygon";
    case DataKind::Line:
        return "a line";
    case DataKind::Points:
        return "points";
    }
    return "data";
}

Result<ShapeWords> readGeometryWords(const Words& words, std::size_t& at, DataKind kind)
{
    const std::string expected = std::string(nameOf(kind)) +
                                 " is #id, coordinates x,y[,z] ... or FROM <scope> [WHERE ...]";
    if (at >= words.size()) {
        return makeError(ErrorCode::ParseFailure, "expected " + expected);
    }
    const std::size_t first = at;
    auto read = readShapeWords(words, at);
    if (!read) {
        return read.error();
    }
    if (read->badId) {
        return makeError(ErrorCode::ParseFailure, "an entity id is # and a number", *read->badId);
    }
    if (read->empty()) {
        return makeError(ErrorCode::ParseFailure, "expected " + expected, words[first]);
    }
    return read;
}

struct ResolvedGeometry {
    std::vector<DrawingShape> shapes; // a <polygon>'s or a <line>'s
    std::vector<Point3> points;       // <points>'
    std::string record;               // what a FROM scope took; empty otherwise
};

// <points>: typed points, each with its level, or the points and levelled
// vertices of the entities named - what a surface is built from
// (geo::surfaceInput), not a shape.
Result<ResolvedGeometry> resolvePoints(const Document& document, const ShapeWords& words,
                                       const ScopeViewProvider& views)
{
    ResolvedGeometry resolved;
    if (!words.scope && words.ids.empty()) {
        for (const TypedCorner& corner : words.corners) {
            if (!corner.z) {
                return makeError(ErrorCode::InvalidArgument,
                                 "a point added to a surface needs its level: x,y,z",
                                 describe(corner.at));
            }
            resolved.points.emplace_back(corner.at.x, corner.at.y, *corner.z);
        }
        return resolved;
    }
    auto named = entitiesNamed(document, words, views);
    if (!named) {
        return named.error();
    }
    if (named->missing) {
        return makeError(ErrorCode::NotFound, "no entity of that id in the drawing",
                         "#" + std::to_string(*named->missing));
    }
    const auto input = geo::surfaceInput(document.model(), named->ids);
    resolved.points = input.input.points;
    for (const auto& line : input.input.breaklines) {
        resolved.points.insert(resolved.points.end(), line.vertices.begin(), line.vertices.end());
    }
    std::size_t skipped = 0;
    for (const auto& [reason, count] : input.skipped) {
        skipped += count;
    }
    if (named->scope) {
        resolved.record = scopeRecord(*named->scope) + " points=" +
                          std::to_string(resolved.points.size()) +
                          " skipped=" + std::to_string(skipped) +
                          " heightless_vertices=" + std::to_string(input.heightlessVertices);
    }
    // A scope that takes nothing is an answer, not a refusal (CLAUDE.md 1.1):
    // the record says what it took, and the verb says it ran nothing. Entities
    // NAMED that have no level to give are the person's mistake, and refused.
    if (resolved.points.empty() && !named->scope) {
        return makeError(ErrorCode::InvalidArgument, "nothing taken has a level to add as a point",
                         std::string("entities=") + std::to_string(named->ids.size()));
    }
    return resolved;
}

Result<ResolvedGeometry> resolveGeometry(const Document& document, const ShapeWords& words,
                                         DataKind kind, const ScopeViewProvider& views)
{
    if (kind == DataKind::Points) {
        return resolvePoints(document, words, views);
    }
    const bool polygon = kind == DataKind::Polygon;
    auto taken =
        resolveShapes(document, words, polygon ? ShapeKind::Polygon : ShapeKind::Line, views);
    if (!taken) {
        return taken.error();
    }
    if (const auto& refused = taken->refused) {
        if (!refused->entity) {
            // The count is the distinct corners left once a polygon's closing
            // repeat of its first corner is taken off (the reader's rule, said
            // by the reader), with what was typed beside it: "needs three
            // corners [corners=3]" contradicted itself.
            const std::string counted = "corners=" + std::to_string(refused->corners) +
                                        " typed=" + std::to_string(words.corners.size());
            return polygon ? makeError(ErrorCode::InvalidArgument,
                                       "a polygon typed in needs at least three distinct corners",
                                       counted)
                           : makeError(ErrorCode::InvalidArgument,
                                       "a line typed in needs at least two distinct vertices",
                                       counted);
        }
        const std::string id = "#" + std::to_string(*refused->entity);
        if (refused->why == NotAShape::NoEntity) {
            return makeError(ErrorCode::NotFound, "no entity of that id in the drawing", id);
        }
        return makeError(ErrorCode::InvalidArgument,
                         "the entity is not " + std::string(polygon ? kPolygonKinds : kLineKinds),
                         id);
    }
    ResolvedGeometry resolved;
    resolved.shapes = std::move(taken->shapes);
    if (taken->scope) {
        resolved.record = scopeRecord(*taken->scope) + (polygon ? " polygons=" : " lines=") +
                          std::to_string(resolved.shapes.size()) +
                          " skipped=" + std::to_string(taken->skipped.total());
    }
    // No shape from a FROM scope is an answer: the record says what it took
    // and skipped, and the edit does nothing. (Typed corners and #ids that are
    // no shape were refused above.)
    return resolved;
}

// The one polygon an edit takes.
Result<Polyline2> onePolygon(const ResolvedGeometry& geometry, std::string_view what)
{
    if (geometry.shapes.size() != 1) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(what) + " takes one polygon",
                         "polygons=" + std::to_string(geometry.shapes.size()));
    }
    return geometry.shapes.front().outline;
}

// ---- the edits --------------------------------------------------------------------------

// What a run of edits did, summed: the first's "before", the last's "after".
struct Outcome {
    tin::TinSurface surface;
    tin::TinEditReport report;
    bool levels = false; // the edit changes levels: report largest_change
    bool edges = false;  // the edit clears breakline edges: report edges_cleared, edges_held
};

void accumulate(Outcome& outcome, const tin::TinEditResult& step, bool first)
{
    const tin::TinEditReport& r = step.report;
    if (first) {
        outcome.report = r;
    } else {
        outcome.report.trianglesAfter = r.trianglesAfter;
        outcome.report.verticesAfter = r.verticesAfter;
        outcome.report.pointsAdded += r.pointsAdded;
        outcome.report.pointsRemoved += r.pointsRemoved;
        outcome.report.pointsRelevelled += r.pointsRelevelled;
        outcome.report.pointsMoved += r.pointsMoved;
        outcome.report.pointsHeld += r.pointsHeld;
        outcome.report.crossings += r.crossings;
        outcome.report.largestChange = std::max(outcome.report.largestChange, r.largestChange);
    }
    outcome.surface = step.surface;
}

Result<Outcome> one(Result<tin::TinEditResult> step)
{
    if (!step) {
        return step.error();
    }
    Outcome outcome;
    accumulate(outcome, *step, true);
    return outcome;
}

Outcome nothing(const tin::TinSurface& surface)
{
    Outcome outcome;
    outcome.surface = surface;
    outcome.report.trianglesBefore = outcome.report.trianglesAfter = surface.triangleCount();
    outcome.report.verticesBefore = outcome.report.verticesAfter = surface.vertexCount();
    return outcome;
}

std::string surfaceRecord(std::string_view name, std::string_view operation,
                          const Outcome& outcome, bool changed, bool preview,
                          std::string_view notRun = {})
{
    const tin::TinEditReport& r = outcome.report;
    std::string record = "surface name=" + recordValue(name) + " operation=" +
                         std::string(operation) +
                         " triangles_before=" + std::to_string(r.trianglesBefore) +
                         " triangles_after=" + std::to_string(r.trianglesAfter) +
                         " vertices_before=" + std::to_string(r.verticesBefore) +
                         " vertices_after=" + std::to_string(r.verticesAfter) +
                         " points_added=" + std::to_string(r.pointsAdded) +
                         " points_removed=" + std::to_string(r.pointsRemoved) +
                         " relevelled=" + std::to_string(r.pointsRelevelled) +
                         " moved=" + std::to_string(r.pointsMoved) +
                         " held=" + std::to_string(r.pointsHeld) +
                         " crossings=" + std::to_string(r.crossings);
    if (outcome.edges) {
        record += " edges_cleared=" + std::to_string(r.edgesCleared) +
                  " edges_held=" + std::to_string(r.edgesHeld);
    }
    if (outcome.levels) {
        record += " largest_change=" + number(r.largestChange);
    }
    record += changed ? " changed=yes" : " changed=no";
    if (!notRun.empty()) {
        record += " ran=no reason=" + recordValue(notRun);
    }
    if (preview) {
        record += " preview=yes";
    }
    return record;
}

Result<const katana::terrain::NamedSurface*> findSurface(const Document& document,
                                                          std::string_view name)
{
    const auto* named = document.surfaces().find(name);
    if (named == nullptr || named->surface == nullptr) {
        std::string known;
        for (const auto& surface : document.surfaces().all()) {
            known += (known.empty() ? "" : ", ") + surface.name;
        }
        return makeError(ErrorCode::NotFound, "no surface of that name in the drawing",
                         std::string(name) + (known.empty() ? " (the drawing has none)"
                                                            : " (surfaces: " + known + ")"));
    }
    return named;
}

// Reads key=value options while the words are options; refuses one not in `keys`.
Result<std::vector<std::pair<std::string, std::string>>>
readOptions(const Words& words, std::size_t& at, std::initializer_list<std::string_view> keys)
{
    std::vector<std::pair<std::string, std::string>> found;
    while (at < words.size()) {
        auto pair = option(words[at]);
        if (!pair) {
            break;
        }
        if (std::find(keys.begin(), keys.end(), pair->first) == keys.end()) {
            return makeError(ErrorCode::ParseFailure, "an option this operation does not take",
                             words[at]);
        }
        found.push_back(std::move(*pair));
        ++at;
    }
    return found;
}

Result<double> optionNumber(const std::string& key, const std::string& value)
{
    const auto parsed = katana::core::parseFiniteDouble(value);
    if (!parsed) {
        return makeError(ErrorCode::ParseFailure, key + "= needs a number", key + "=" + value);
    }
    return *parsed;
}

// INSIDE <polygon>, when it comes next. `tookNothing`: a scope that took no
// polygon - which is NOT "no INSIDE", the whole surface, but nothing to act on.
struct InsideRead {
    std::optional<Polyline2> polygon;
    bool tookNothing = false;
};
Result<InsideRead> readInside(const Document& document, const Words& words, std::size_t& at,
                              const ScopeViewProvider& views, std::string& record)
{
    if (at >= words.size() || !is(words[at], "INSIDE")) {
        return InsideRead{};
    }
    ++at;
    auto geometryWords = readGeometryWords(words, at, DataKind::Polygon);
    if (!geometryWords) {
        return geometryWords.error();
    }
    auto geometry = resolveGeometry(document, *geometryWords, DataKind::Polygon, views);
    if (!geometry) {
        return geometry.error();
    }
    record = geometry->record;
    if (geometry->shapes.empty()) {
        return InsideRead{std::nullopt, true};
    }
    auto polygon = onePolygon(*geometry, "INSIDE");
    if (!polygon) {
        return polygon.error();
    }
    return InsideRead{std::optional<Polyline2>(std::move(*polygon)), false};
}

Result<ResolvedGeometry> readGeometry(const Document& document, const Words& words,
                                      std::size_t& at, DataKind kind,
                                      const ScopeViewProvider& views)
{
    auto geometryWords = readGeometryWords(words, at, kind);
    if (!geometryWords) {
        return geometryWords.error();
    }
    return resolveGeometry(document, *geometryWords, kind, views);
}

std::vector<std::uint32_t> unionOf(const std::vector<std::vector<std::uint32_t>>& sets)
{
    std::set<std::uint32_t> all;
    for (const auto& set : sets) {
        all.insert(set.begin(), set.end());
    }
    return {all.begin(), all.end()};
}

struct EditOutcome {
    Outcome outcome;
    std::string operation; // as the reply and the step name say it
    std::string record;    // what a scope took
    std::string notRun;    // why nothing was done: the scope took nothing
};

Result<EditOutcome> runEdit(const Document& document, const tin::TinSurface& surface,
                            const Words& words, std::size_t& at, const ScopeViewProvider& views)
{
    if (at >= words.size()) {
        return makeError(ErrorCode::ParseFailure,
                         "SURFACE EDIT <name> needs an operation: DELETE, FLIP, ADD, BREAKLINE, "
                         "BOUNDARY, HOLE, RAISE, LOWER, SETLEVEL, SCALE, SMOOTH or THIN");
    }
    EditOutcome edit;
    const std::string op = katana::core::uppered(words[at++]);
    const auto take = [&edit](Result<Outcome> outcome) -> Status {
        if (!outcome) {
            return outcome.error();
        }
        edit.outcome = std::move(outcome).value();
        return {};
    };

    if (op == "DELETE") {
        if (at >= words.size()) {
            return makeError(ErrorCode::ParseFailure,
                             "DELETE needs TRIANGLE, TRIANGLES, LONG, THIN, POINT, POINTS or "
                             "BREAKLINE");
        }
        const std::string what = katana::core::uppered(words[at++]);
        if (what == "TRIANGLE") {
            edit.operation = "delete_triangle";
            auto point = requirePoint(words, at, "DELETE TRIANGLE");
            if (!point) {
                return point.error();
            }
            const auto triangle = tin::triangleAt(surface, point->at);
            if (!triangle) {
                return makeError(ErrorCode::InvalidArgument, "no triangle of the surface there",
                                 describe(point->at));
            }
            if (Status s = take(one(tin::deleteTriangles(surface, std::vector{*triangle}))); !s) {
                return s.error();
            }
        } else if (what == "TRIANGLES") {
            if (at >= words.size() || !(is(words[at], "INSIDE") || is(words[at], "OUTSIDE"))) {
                return makeError(ErrorCode::ParseFailure, "DELETE TRIANGLES needs INSIDE or OUTSIDE");
            }
            const bool inside = is(words[at++], "INSIDE");
            edit.operation = inside ? "delete_triangles_inside" : "delete_triangles_outside";
            auto geometry = readGeometry(document, words, at, DataKind::Polygon, views);
            if (!geometry) {
                return geometry.error();
            }
            edit.record = geometry->record;
            if (geometry->shapes.empty()) {
                // OUTSIDE of no polygon is not the whole surface: nothing was named.
                edit.outcome = nothing(surface);
                edit.notRun = "the scope took no polygon";
                return edit;
            }
            std::vector<std::vector<std::uint32_t>> sets;
            for (const DrawingShape& shape : geometry->shapes) {
                sets.push_back(tin::trianglesByPolygon(surface, shape.outline, true));
            }
            std::vector<std::uint32_t> chosen = unionOf(sets);
            if (!inside) {
                std::vector<std::uint32_t> outside;
                for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
                    if (!std::binary_search(chosen.begin(), chosen.end(), t)) {
                        outside.push_back(t);
                    }
                }
                chosen = std::move(outside);
            }
            if (chosen.empty()) {
                edit.outcome = nothing(surface);
            } else if (Status s = take(one(tin::deleteTriangles(surface, chosen))); !s) {
                return s.error();
            }
        } else if (what == "LONG" || what == "THIN") {
            const bool isLong = what == "LONG";
            edit.operation = isLong ? "delete_long" : "delete_thin";
            auto value = requireNumber(words, at, isLong ? "DELETE LONG" : "DELETE THIN");
            if (!value) {
                return value.error();
            }
            if (!(*value > 0.0) || (!isLong && *value >= 180.0)) {
                return makeError(ErrorCode::InvalidArgument,
                                 isLong ? "the length must be more than 0"
                                        : "the angle must be more than 0 and less than 180 degrees",
                                 number(*value));
            }
            const auto chosen =
                isLong ? tin::trianglesLongerThan(surface, *value)
                       : tin::trianglesThinnerThan(surface, *value * std::numbers::pi / 180.0);
            if (chosen.empty()) {
                edit.outcome = nothing(surface);
            } else if (Status s = take(one(tin::deleteTriangles(surface, chosen))); !s) {
                return s.error();
            }
        } else if (what == "POINT") {
            edit.operation = "delete_point";
            auto point = requirePoint(words, at, "DELETE POINT");
            if (!point) {
                return point.error();
            }
            auto options = readOptions(words, at, {"reach"});
            if (!options) {
                return options.error();
            }
            std::optional<double> reach;
            for (const auto& [key, value] : *options) {
                auto parsed = optionNumber(key, value);
                if (!parsed) {
                    return parsed.error();
                }
                if (!(*parsed >= 0.0)) {
                    return makeError(ErrorCode::InvalidArgument, "reach= cannot be negative",
                                     value);
                }
                reach = *parsed;
            }
            const auto vertex = tin::vertexAt(surface, point->at, reach);
            if (!vertex) {
                // Off the surface is said when the point is, whatever the reach: a
                // reach that found nothing near it on the surface is another thing.
                const bool onSurface = tin::triangleAt(surface, point->at).has_value();
                return makeError(
                    ErrorCode::InvalidArgument,
                    !reach ? "the point is off the surface"
                    : onSurface ? "no vertex of the surface within reach= of the point"
                                : "the point is off the surface, and no vertex is within reach=",
                    describe(point->at));
            }
            if (Status s = take(one(tin::deletePoints(surface, std::vector{*vertex}))); !s) {
                return s.error();
            }
        } else if (what == "POINTS") {
            if (at >= words.size() || !is(words[at], "INSIDE")) {
                return makeError(ErrorCode::ParseFailure, "DELETE POINTS needs INSIDE <polygon>");
            }
            ++at;
            edit.operation = "delete_points_inside";
            auto geometry = readGeometry(document, words, at, DataKind::Polygon, views);
            if (!geometry) {
                return geometry.error();
            }
            edit.record = geometry->record;
            if (geometry->shapes.empty()) {
                edit.outcome = nothing(surface);
                edit.notRun = "the scope took no polygon";
                return edit;
            }
            std::vector<std::vector<std::uint32_t>> sets;
            for (const DrawingShape& shape : geometry->shapes) {
                sets.push_back(tin::verticesInside(surface, shape.outline));
            }
            const auto chosen = unionOf(sets);
            if (chosen.empty()) {
                edit.outcome = nothing(surface);
            } else if (Status s = take(one(
                           tin::deletePoints(surface, chosen, tin::HeldPointRule::Skip)));
                       !s) {
                return s.error();
            }
        } else if (what == "BREAKLINE") {
            // The breakline edges along the lines, made ordinary: the surface's
            // triangles and vertices as they were, the flags gone.
            edit.operation = "delete_breakline";
            edit.outcome.edges = true;
            auto geometry = readGeometry(document, words, at, DataKind::Line, views);
            if (!geometry) {
                return geometry.error();
            }
            edit.record = geometry->record;
            if (geometry->shapes.empty()) {
                edit.outcome = nothing(surface);
                edit.outcome.edges = true;
                edit.notRun = "the scope took no line";
                return edit;
            }
            std::vector<Polyline2> lines;
            lines.reserve(geometry->shapes.size());
            for (const DrawingShape& shape : geometry->shapes) {
                lines.push_back(shape.outline);
            }
            if (Status s = take(one(tin::deleteBreaklines(surface, lines))); !s) {
                return s.error();
            }
            edit.outcome.edges = true;
        } else {
            return makeError(ErrorCode::ParseFailure,
                             "DELETE takes TRIANGLE, TRIANGLES, LONG, THIN, POINT, POINTS or "
                             "BREAKLINE",
                             words[at - 1]);
        }
    } else if (op == "FLIP") {
        edit.operation = "flip";
        auto point = requirePoint(words, at, "FLIP");
        if (!point) {
            return point.error();
        }
        const auto edge = tin::edgeAt(surface, point->at);
        if (!edge) {
            return makeError(ErrorCode::InvalidArgument, "no triangle of the surface there",
                             describe(point->at));
        }
        if (Status s = take(one(tin::flipEdge(surface, edge->triangle, edge->edge))); !s) {
            return s.error();
        }
    } else if (op == "ADD") {
        if (at >= words.size() || !(is(words[at], "POINT") || is(words[at], "POINTS"))) {
            return makeError(ErrorCode::ParseFailure, "ADD needs POINT x,y,z ... or POINTS <points>");
        }
        const bool typed = is(words[at++], "POINT");
        edit.operation = typed ? "add_point" : "add_points";
        std::vector<Point3> points;
        if (typed) {
            while (at < words.size()) {
                const auto point = parseTypedCorner(words[at]);
                if (!point) {
                    break;
                }
                if (!point->z) {
                    return makeError(ErrorCode::InvalidArgument,
                                     "a point added to a surface needs its level: x,y,z",
                                     words[at]);
                }
                points.emplace_back(point->at.x, point->at.y, *point->z);
                ++at;
            }
            if (points.empty()) {
                return makeError(ErrorCode::ParseFailure, "ADD POINT needs x,y,z");
            }
        } else {
            auto geometry = readGeometry(document, words, at, DataKind::Points, views);
            if (!geometry) {
                return geometry.error();
            }
            edit.record = geometry->record;
            points = std::move(geometry->points);
            if (points.empty()) {
                edit.outcome = nothing(surface);
                edit.notRun = "the scope took no point with a level";
                return edit;
            }
        }
        if (Status s = take(one(tin::addPoints(surface, points))); !s) {
            return s.error();
        }
    } else if (op == "BREAKLINE") {
        edit.operation = "breakline";
        auto geometry = readGeometry(document, words, at, DataKind::Line, views);
        if (!geometry) {
            return geometry.error();
        }
        edit.record = geometry->record;
        if (geometry->shapes.empty()) {
            edit.outcome = nothing(surface);
            edit.notRun = "the scope took no line";
            return edit;
        }
        // All the lines in ONE patch: lines of one edit that cross are judged
        // against each other (3D lines at different levels are refused whatever
        // order they are named in), and it is one rebuild, not one per line.
        std::vector<tin::BreaklineInput> lines;
        for (const DrawingShape& shape : geometry->shapes) {
            tin::BreaklineInput line;
            line.closed = shape.outline.closed;
            for (std::size_t i = 0; i < shape.outline.vertices.size(); ++i) {
                line.vertices.push_back({shape.outline.vertices[i], shape.heights[i]});
            }
            lines.push_back(std::move(line));
        }
        if (Status s = take(one(tin::addBreaklines(surface, lines))); !s) {
            return s.error();
        }
    } else if (op == "BOUNDARY" || op == "HOLE") {
        const bool boundary = op == "BOUNDARY";
        edit.operation = boundary ? "boundary" : "hole";
        auto geometry = readGeometry(document, words, at, DataKind::Polygon, views);
        if (!geometry) {
            return geometry.error();
        }
        edit.record = geometry->record;
        if (geometry->shapes.empty()) {
            edit.outcome = nothing(surface);
            edit.notRun = "the scope took no polygon";
            return edit;
        }
        if (boundary) {
            auto polygon = onePolygon(*geometry, "BOUNDARY");
            if (!polygon) {
                return polygon.error();
            }
            if (Status s = take(one(tin::clipToBoundary(surface, *polygon))); !s) {
                return s.error();
            }
        } else {
            // Every footprint, in the order the scope gave them: the footprints
            // that can be cut together are cut in one patch, so a scope of 100
            // buildings is one whole-surface rebuild, not 100.
            std::vector<Polyline2> footprints;
            footprints.reserve(geometry->shapes.size());
            for (const DrawingShape& shape : geometry->shapes) {
                footprints.push_back(shape.outline);
            }
            if (Status s = take(one(tin::cutHoles(surface, footprints))); !s) {
                return s.error();
            }
        }
    } else if (op == "RAISE" || op == "LOWER" || op == "SETLEVEL" || op == "SCALE") {
        edit.operation = katana::core::lowered(op);
        auto value = requireNumber(words, at, op);
        if (!value) {
            return value.error();
        }
        tin::HeightChange change;
        change.value = op == "LOWER" ? -*value : *value;
        change.operation = op == "SETLEVEL" ? tin::HeightOperation::SetLevel
                           : op == "SCALE"  ? tin::HeightOperation::Scale
                                            : tin::HeightOperation::Raise;
        auto options = readOptions(words, at, op == "SCALE" ? std::initializer_list<std::string_view>{"base"}
                                                            : std::initializer_list<std::string_view>{});
        if (!options) {
            return options.error();
        }
        for (const auto& [key, text] : *options) {
            auto parsed = optionNumber(key, text);
            if (!parsed) {
                return parsed.error();
            }
            change.base = *parsed;
        }
        auto inside = readInside(document, words, at, views, edit.record);
        if (!inside) {
            return inside.error();
        }
        if (inside->tookNothing) {
            edit.outcome = nothing(surface);
            edit.outcome.levels = true;
            edit.notRun = "the scope took no polygon";
            return edit;
        }
        if (Status s = take(one(tin::changeHeights(surface, change, inside->polygon))); !s) {
            return s.error();
        }
        edit.outcome.levels = true;
    } else if (op == "SMOOTH") {
        edit.operation = "smooth";
        auto options = readOptions(words, at, {"passes", "weight"});
        if (!options) {
            return options.error();
        }
        tin::SmoothOptions smooth;
        for (const auto& [key, text] : *options) {
            if (key == "passes") {
                const auto passes = katana::core::parseInteger(text);
                if (!passes || *passes < 1 || *passes > 1000) {
                    return makeError(ErrorCode::InvalidArgument,
                                     "passes= is a whole number from 1 to 1000", text);
                }
                smooth.passes = static_cast<int>(*passes);
            } else {
                auto parsed = optionNumber(key, text);
                if (!parsed) {
                    return parsed.error();
                }
                smooth.weight = *parsed;
            }
        }
        auto inside = readInside(document, words, at, views, edit.record);
        if (!inside) {
            return inside.error();
        }
        if (inside->tookNothing) {
            edit.outcome = nothing(surface);
            edit.outcome.levels = true;
            edit.notRun = "the scope took no polygon";
            return edit;
        }
        if (Status s = take(one(tin::smoothSurface(surface, smooth, inside->polygon))); !s) {
            return s.error();
        }
        edit.outcome.levels = true;
    } else if (op == "THIN") {
        edit.operation = "thin";
        auto tolerance = requireNumber(words, at, "THIN");
        if (!tolerance) {
            return tolerance.error();
        }
        if (Status s = take(one(tin::thinSurface(surface, *tolerance))); !s) {
            return s.error();
        }
        edit.outcome.levels = true;
    } else {
        return makeError(ErrorCode::ParseFailure,
                         "SURFACE EDIT takes DELETE, FLIP, ADD, BREAKLINE, BOUNDARY, HOLE, RAISE, "
                         "LOWER, SETLEVEL, SCALE, SMOOTH or THIN",
                         words[at - 1]);
    }
    return edit;
}

// NAME <new>, where it is required or allowed; empty when absent.
Result<std::string> readName(const Words& words, std::size_t& at, bool required,
                             std::string_view verb)
{
    if (at < words.size() && is(words[at], "NAME")) {
        if (at + 1 >= words.size() || words[at + 1].empty()) {
            return makeError(ErrorCode::ParseFailure, "NAME needs the new surface's name");
        }
        at += 2;
        return words[at - 1];
    }
    if (required) {
        return makeError(ErrorCode::ParseFailure,
                         std::string(verb) + " needs NAME <new>: the surface it makes");
    }
    return std::string{};
}

Status requireEnd(const Words& words, std::size_t at)
{
    if (at < words.size()) {
        return makeError(ErrorCode::ParseFailure, "a word the line does not take here", words[at]);
    }
    return {};
}

std::string stepNameOf(std::string_view operation)
{
    return "SURFACE_EDIT_" + katana::core::uppered(operation);
}

} // namespace

bool isSurfaceEditVerb(const std::vector<std::string>& tokens)
{
    if (tokens.size() < 2 || !is(tokens[0], "SURFACE")) {
        return false;
    }
    for (const char* word : {"EDIT", "MERGE", "PASTE", "CLIP", "OFFSET"}) {
        if (is(tokens[1], word)) {
            return true;
        }
    }
    return false;
}

Result<SurfaceEditPlan> planSurfaceEdit(const Document& document,
                                        const std::vector<std::string>& tokens,
                                        const ScopeViewProvider& views)
{
    if (!isSurfaceEditVerb(tokens)) {
        return makeError(ErrorCode::ParseFailure, "not a surface edit line");
    }
    const std::string verb = katana::core::uppered(tokens[1]);
    // PREVIEW anywhere after the verb's own two words and the surface's name
    // is the preview, wherever a person typed it - except where the grammar
    // wants a VALUE: a surface called Preview (the first surface of every
    // verb is at 2, a paste's base after INTO, a merge's members before
    // NAME), the new surface's NAME, a layer after LAYERS. The words arrive
    // with their quotes already off, so quoting the word cannot protect it;
    // the position does. (CommandInterpreter::modify does the same for a
    // layer called "preview" or "set".)
    const std::size_t firstFree = 3;
    Words words;
    bool preview = false;
    bool pastName = false; // a MERGE's members are the words before NAME
    for (std::size_t i = 0; i < tokens.size(); ++i) {
        bool isValue = false;
        if (i > 0) {
            const std::string_view before = tokens[i - 1];
            isValue = is(before, "NAME") || is(before, "INTO") || is(before, "LAYERS") ||
                      is(before, "LAYER");
        }
        if (verb == "MERGE" && !pastName && document.surfaces().find(tokens[i]) != nullptr) {
            isValue = true; // a surface of that name is named, not a flag
        }
        if (is(tokens[i], "NAME")) {
            pastName = true;
        }
        if (i >= firstFree && !isValue && is(tokens[i], "PREVIEW")) {
            preview = true;
        } else {
            words.push_back(tokens[i]);
        }
    }

    katana::terrain::SurfaceStore store = document.surfaces();
    SurfaceEditPlan plan;
    std::size_t at = 2;
    if (verb == "EDIT") {
        if (words.size() < 3) {
            return makeError(ErrorCode::ParseFailure, "SURFACE EDIT needs a surface's name");
        }
        auto named = findSurface(document, words[2]);
        if (!named) {
            return named.error();
        }
        const std::string name = (*named)->name;
        const tin::TinSurface& surface = *(*named)->surface;
        at = 3;
        auto edit = runEdit(document, surface, words, at, views);
        if (!edit) {
            return edit.error();
        }
        if (Status s = requireEnd(words, at); !s) {
            return s.error();
        }
        const bool changed = !(edit->outcome.surface == surface);
        plan.reply = (edit->record.empty() ? std::string{} : edit->record + "\n") +
                     surfaceRecord(name, edit->operation, edit->outcome, changed, preview,
                                   edit->notRun);
        plan.stepName = stepNameOf(edit->operation);
        if (changed && !preview) {
            if (Status s = store.replace(name, std::make_shared<const tin::TinSurface>(
                                                   std::move(edit->outcome.surface)));
                !s) {
                return s.error();
            }
            plan.surfaces = std::move(store);
        }
        return plan;
    }

    Outcome outcome;
    std::string target;    // the surface made or changed
    std::string source;    // what it was made from, for SURFACE LIST
    std::string record;    // what a scope took
    bool replaceInPlace = false;
    std::string operation = katana::core::lowered(verb);
    if (verb == "MERGE") {
        std::vector<const tin::TinSurface*> members;
        std::vector<std::string> names;
        while (at < words.size() && !is(words[at], "NAME")) {
            auto named = findSurface(document, words[at]);
            if (!named) {
                return named.error();
            }
            members.push_back((*named)->surface.get());
            names.push_back((*named)->name);
            ++at;
        }
        auto name = readName(words, at, true, "SURFACE MERGE");
        if (!name) {
            return name.error();
        }
        target = *name;
        auto merged = tin::mergeSurfaces(members);
        if (!merged) {
            return merged.error();
        }
        accumulate(outcome, *merged, true);
        source = "merge of ";
        for (std::size_t i = 0; i < names.size(); ++i) {
            source += (i == 0 ? "" : ", ") + names[i];
        }
    } else if (verb == "PASTE") {
        if (words.size() < 5 || !is(words[3], "INTO")) {
            return makeError(ErrorCode::ParseFailure, "SURFACE PASTE <top> INTO <base> [NAME <new>]");
        }
        auto top = findSurface(document, words[2]);
        if (!top) {
            return top.error();
        }
        auto base = findSurface(document, words[4]);
        if (!base) {
            return base.error();
        }
        at = 5;
        auto name = readName(words, at, false, "SURFACE PASTE");
        if (!name) {
            return name.error();
        }
        auto pasted = tin::pasteSurface(*(*base)->surface, *(*top)->surface);
        if (!pasted) {
            return pasted.error();
        }
        accumulate(outcome, *pasted, true);
        replaceInPlace = name->empty();
        target = replaceInPlace ? (*base)->name : *name;
        source = "paste of " + (*top)->name + " into " + (*base)->name;
    } else if (verb == "CLIP" || verb == "OFFSET") {
        if (words.size() < 4) {
            return makeError(ErrorCode::ParseFailure,
                             verb == "CLIP" ? "SURFACE CLIP <name> <polygon> NAME <new>"
                                            : "SURFACE OFFSET <name> <dz> NAME <new>");
        }
        auto named = findSurface(document, words[2]);
        if (!named) {
            return named.error();
        }
        at = 3;
        Result<tin::TinEditResult> made = makeError(ErrorCode::Internal, "not made");
        if (verb == "CLIP") {
            auto geometry = readGeometry(document, words, at, DataKind::Polygon, views);
            if (!geometry) {
                return geometry.error();
            }
            record = geometry->record;
            if (geometry->shapes.empty()) {
                // A scope that takes no polygon makes no surface, and says so.
                auto name = readName(words, at, true, "SURFACE CLIP");
                if (!name) {
                    return name.error();
                }
                if (Status s = requireEnd(words, at); !s) {
                    return s.error();
                }
                plan.reply = (record.empty() ? std::string{} : record + "\n") +
                             "surface name=" + recordValue(*name) +
                             " operation=clip changed=no ran=no reason=" +
                             recordValue("the scope took no polygon") +
                             (preview ? " preview=yes" : "");
                plan.stepName = "SURFACE_CLIP";
                return plan;
            }
            auto polygon = onePolygon(*geometry, "SURFACE CLIP");
            if (!polygon) {
                return polygon.error();
            }
            made = tin::clipToBoundary(*(*named)->surface, *polygon);
            source = "clip of " + (*named)->name;
        } else {
            auto dz = requireNumber(words, at, "SURFACE OFFSET");
            if (!dz) {
                return dz.error();
            }
            made = tin::offsetSurface(*(*named)->surface, *dz);
            source = "offset of " + (*named)->name + " by " + number(*dz);
            outcome.levels = true;
        }
        auto name = readName(words, at, true, verb == "CLIP" ? "SURFACE CLIP" : "SURFACE OFFSET");
        if (!name) {
            return name.error();
        }
        if (!made) {
            return made.error();
        }
        const bool levels = outcome.levels;
        accumulate(outcome, *made, true);
        outcome.levels = levels;
        target = *name;
    }
    if (Status s = requireEnd(words, at); !s) {
        return s.error();
    }
    if (!replaceInPlace && store.find(target) != nullptr) {
        return makeError(ErrorCode::AlreadyExists, "a surface of that name is already in the drawing",
                         target);
    }
    plan.reply = (record.empty() ? std::string{} : record + "\n") +
                 surfaceRecord(target, operation, outcome, true, preview);
    plan.stepName = "SURFACE_" + verb;
    if (preview) {
        return plan;
    }
    auto shared = std::make_shared<const tin::TinSurface>(std::move(outcome.surface));
    if (replaceInPlace) {
        if (Status s = store.replace(target, std::move(shared)); !s) {
            return s.error();
        }
    } else {
        katana::terrain::NamedSurface named;
        named.name = target;
        named.surface = std::move(shared);
        named.source = source;
        if (Status s = store.add(std::move(named)); !s) {
            return s.error();
        }
    }
    plan.surfaces = std::move(store);
    return plan;
}

Result<std::string> runSurfaceEditVerb(Document& document, const std::vector<std::string>& tokens,
                                       const ScopeViewProvider& views)
{
    auto plan = planSurfaceEdit(document, tokens, views);
    if (!plan) {
        return plan.error();
    }
    if (plan->surfaces) {
        if (Status s = document.setSurfaces(std::move(*plan->surfaces), plan->stepName); !s) {
            return s.error();
        }
    }
    return plan->reply;
}

std::string surfaceEditHelpSummary()
{
    return R"(Surfaces  SURFACE EDIT name DELETE TRIANGLE|POINT x,y | DELETE TRIANGLES INSIDE|OUTSIDE poly
          | DELETE LONG m | DELETE THIN deg | DELETE POINTS INSIDE poly | DELETE BREAKLINE line
          | FLIP x,y | ADD POINT
          x,y,z ... | ADD POINTS pts | BREAKLINE line | BOUNDARY poly | HOLE poly | RAISE|LOWER
          dz | SETLEVEL z | SCALE f [base=z] [INSIDE poly] | SMOOTH [passes=] [weight=] [INSIDE
          poly] | THIN tol    SURFACE MERGE a b ... NAME new | PASTE top INTO base [NAME new]
          | CLIP name poly NAME new | OFFSET name dz NAME new   one undo step each, PREVIEW;
          poly/line/pts: #id ... | x,y[,z] ... | FROM scope [WHERE ...] (HELP SURFACE EDIT)
)";
}

std::string surfaceEditVerbHelp()
{
    return R"(Surface edits (docs/surface_editing.md). Words are case-insensitive; quote a
name with blanks. Every edit is ONE undo step; PREVIEW (anywhere after the name)
works it out and changes nothing; an edit that changes nothing is no step.

SURFACE EDIT <name> DELETE TRIANGLE x,y            the triangle under x,y
SURFACE EDIT <name> DELETE TRIANGLES INSIDE|OUTSIDE <polygon>
                                                   by their centroids; several polygons: their union
SURFACE EDIT <name> DELETE LONG <length>           triangles with a side longer than length
SURFACE EDIT <name> DELETE THIN <degrees>          triangles with an angle below degrees
SURFACE EDIT <name> DELETE POINT x,y [reach=<m>]   the vertex of the triangle under x,y nearest
                                                   it (within reach); not one on a breakline
SURFACE EDIT <name> DELETE POINTS INSIDE <polygon> breakline and boundary vertices are held
SURFACE EDIT <name> DELETE BREAKLINE <line>        the breakline edges along the line become
                                                   ordinary edges; the triangles are as they were
SURFACE EDIT <name> FLIP x,y                       the edge nearest x,y of the triangle under it
SURFACE EDIT <name> ADD POINT x,y,z [x,y,z ...]    off the surface, a point extends it
SURFACE EDIT <name> ADD POINTS <points>            points and levelled vertices of the entities
SURFACE EDIT <name> BREAKLINE <line>               3D where a vertex has a level, draped where not
SURFACE EDIT <name> BOUNDARY <polygon>             keep what is inside, cut exactly
SURFACE EDIT <name> HOLE <polygon>                 remove what is inside, cut exactly
SURFACE EDIT <name> RAISE|LOWER <dz> [INSIDE <polygon>]
SURFACE EDIT <name> SETLEVEL <z> [INSIDE <polygon>]
SURFACE EDIT <name> SCALE <factor> [base=<z>] [INSIDE <polygon>]
                                                   about base, else the lowest level changed
SURFACE EDIT <name> SMOOTH [passes=3] [weight=0.5] [INSIDE <polygon>]
                                                   rim and breakline vertices held
SURFACE EDIT <name> THIN <tolerance>               remove vertices that move the surface by no
                                                   more than tolerance anywhere
SURFACE MERGE <a> <b> [...] NAME <new>             later surfaces win where they reach
SURFACE PASTE <top> INTO <base> [NAME <new>]       the top in the base with a clean seam; in
                                                   place unless NAME
SURFACE CLIP <name> <polygon> NAME <new>           a copy of the part inside the polygon
SURFACE OFFSET <name> <dz> NAME <new>              a copy raised by dz

<polygon>, <line>, <points>:  #id [#id ...]  |  x,y[,z] x,y[,z] ...  |  FROM <scope> [WHERE k=v ...]
  (the scope words of HELP: SELECTION | DRAWING | VIEW [id] | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY];
  options come before FROM). A scope that takes nothing is reported, not refused:
  changed=no ran=no reason="the scope took no polygon", and no undo step.

Reply: [scope=... matched=N polygons=|lines=|points= skipped=]
       surface name= operation= triangles_before= triangles_after= vertices_before=
       vertices_after= points_added= points_removed= relevelled= moved= held= crossings=
       [largest_change=] changed=yes|no [preview=yes]
)";
}

// ---- the tools' surface -------------------------------------------------------------------

namespace {

std::string& targetName()
{
    // One for the program, as the edit tools' EditDefaults are (docs/tools.md).
    static std::string name;
    return name;
}

} // namespace

namespace {

std::vector<std::pair<std::size_t, std::function<void()>>>& observers()
{
    static std::vector<std::pair<std::size_t, std::function<void()>>> all;
    return all;
}

} // namespace

const std::string& surfaceEditTarget() { return targetName(); }

void setSurfaceEditTarget(std::string name)
{
    targetName() = std::move(name);
    // A copy: an observer may add or remove observers.
    const auto notify = observers();
    for (const auto& [handle, observer] : notify) {
        observer();
    }
}

std::size_t addSurfaceEditTargetObserver(std::function<void()> observer)
{
    static std::size_t next = 0;
    observers().emplace_back(++next, std::move(observer));
    return next;
}

void removeSurfaceEditTargetObserver(std::size_t handle)
{
    auto& all = observers();
    all.erase(std::remove_if(all.begin(), all.end(),
                             [handle](const auto& entry) { return entry.first == handle; }),
              all.end());
}

Result<std::string> activeEditSurface(const Document& document)
{
    const auto& store = document.surfaces();
    if (!targetName().empty()) {
        if (const auto* named = store.find(targetName())) {
            return named->name;
        }
    }
    for (const auto& surface : store.all()) {
        if (surface.visible) {
            return surface.name;
        }
    }
    if (!store.empty()) {
        return store.all().front().name;
    }
    return makeError(ErrorCode::NotFound,
                     "the drawing has no surface to edit: make one first (Terrain > Surface From "
                     "Drawing, or SURFACE FROM)");
}

std::string surfaceNameWord(std::string_view name)
{
    const bool blank = name.empty() || std::any_of(name.begin(), name.end(), [](char c) {
                           return katana::core::isAsciiSpace(c);
                       });
    return blank ? "\"" + std::string(name) + "\"" : std::string(name);
}

} // namespace katana::cad
