// The boundaries the volume verbs measure inside (terrain/volume_verbs.hpp):
// an entity, typed corners, or what a scope takes. The scope is the one
// shared grammar (scope_verbs.hpp), resolved by the one matcher.

#include "katana/cad/terrain/volume_verbs.hpp"

#include <string>
#include <utility>
#include <variant>

#include "katana/core/text.hpp"
#include "katana/geometry/chording.hpp"
#include "katana/geometry/curves2d.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

std::string idText(EntityId id)
{
    return "#" + std::to_string(id);
}

// What a kind of entity is called in a refusal.
const char* kindName(const Entity& entity)
{
    return std::visit(
        [](const auto& geometry) -> const char* {
            using Kind = std::decay_t<decltype(geometry)>;
            if constexpr (std::is_same_v<Kind, katana::geometry::Segment2>) {
                return "a line";
            } else if constexpr (std::is_same_v<Kind, katana::geometry::Arc2>) {
                return "an arc";
            } else if constexpr (std::is_same_v<Kind, katana::geometry::Polyline2> ||
                                 std::is_same_v<Kind, katana::geometry::CurvePolyline2>) {
                return "an open polyline";
            } else if constexpr (std::is_same_v<Kind, katana::geometry::Ellipse2>) {
                return "an ellipse";
            } else if constexpr (std::is_same_v<Kind, katana::geometry::Spline2>) {
                return "a spline";
            } else {
                return "no outline";
            }
        },
        entity.geometry);
}

// Whether a kind bounds nothing because it is open: a line, an arc, an open
// polyline. The rest that are not polygons are no outline at all.
bool isOpenOutline(const Entity& entity)
{
    return std::holds_alternative<katana::geometry::Segment2>(entity.geometry) ||
           std::holds_alternative<katana::geometry::Arc2>(entity.geometry) ||
           std::holds_alternative<katana::geometry::Polyline2>(entity.geometry) ||
           std::holds_alternative<katana::geometry::CurvePolyline2>(entity.geometry);
}

// Corners without the first repeated at the end, and their heights with them.
void dropClosingRepeat(VolumePolygon& polygon)
{
    auto& corners = polygon.ring.vertices;
    while (corners.size() > 1 &&
           corners.front().distanceTo(corners.back()) <= katana::math::tolerance::kGeometric) {
        corners.pop_back();
        if (polygon.heights.size() > corners.size()) {
            polygon.heights.pop_back();
        }
    }
    polygon.ring.closed = true;
}

// "x,y" or "x,y,z": nullopt for anything else, so a word that is not a
// corner ends a typed polygon.
std::optional<std::pair<Point2, std::optional<double>>> cornerOf(std::string_view word)
{
    std::vector<double> values;
    std::size_t start = 0;
    while (true) {
        const std::size_t comma = word.find(',', start);
        const std::string_view part =
            word.substr(start, comma == std::string_view::npos ? std::string_view::npos
                                                               : comma - start);
        const auto value = katana::core::parseFiniteDouble(katana::core::trimmed(part));
        if (!value) {
            return std::nullopt;
        }
        values.push_back(*value);
        if (comma == std::string_view::npos) {
            break;
        }
        start = comma + 1;
    }
    if (values.size() == 2) {
        return std::pair(Point2(values[0], values[1]), std::optional<double>{});
    }
    if (values.size() == 3) {
        return std::pair(Point2(values[0], values[1]), std::optional<double>(values[2]));
    }
    return std::nullopt;
}

bool isWord(std::string_view word, std::string_view expected)
{
    return katana::core::equalsIgnoringCase(word, expected);
}

} // namespace

Result<VolumePolygon> volumePolygonOf(const Entity& entity)
{
    VolumePolygon polygon;
    polygon.entity = entity.id;
    if (const auto* polyline = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
        if (polyline->closed) {
            polygon.ring = *polyline;
            polygon.heights =
                katana::entity::heightsOf(entity.properties, polyline->vertices.size());
            dropClosingRepeat(polygon);
            return polygon;
        }
    } else if (const auto* curve = std::get_if<katana::geometry::CurvePolyline2>(&entity.geometry)) {
        if (curve->closed) {
            if (curve->hasArcs()) {
                // The walk repeats its first point at the end when closed.
                for (const auto& point :
                     curve->tessellateWithHeights(katana::geometry::kCurveChordTolerance)) {
                    polygon.ring.vertices.push_back(point.position);
                    polygon.heights.push_back(point.height);
                }
                polygon.chorded = true;
            } else {
                polygon.ring.vertices = curve->positions();
                polygon.heights = curve->heights();
            }
            dropClosingRepeat(polygon);
            return polygon;
        }
    } else if (const auto* circle = std::get_if<katana::geometry::Circle2>(&entity.geometry)) {
        polygon.ring.vertices =
            katana::geometry::chordCircle(*circle, katana::geometry::kCurveChordTolerance);
        polygon.heights =
            katana::entity::heightsOf(entity.properties, polygon.ring.vertices.size());
        polygon.chorded = true;
        dropClosingRepeat(polygon);
        return polygon;
    }
    return makeError(ErrorCode::InvalidGeometry,
                     idText(entity.id) + " is " + kindName(entity) +
                         "; a polygon is a closed polyline or a circle",
                     idText(entity.id));
}

Result<VolumePolygons> readVolumePolygons(const Document& document,
                                          const std::vector<std::string>& words, std::size_t& at,
                                          const ScopeViewProvider& views)
{
    VolumePolygons result;
    if (at >= words.size()) {
        return makeError(ErrorCode::ParseFailure,
                         "a polygon is #id, three or more corners x,y, or FROM and a scope");
    }
    if (isWord(words[at], "FROM")) {
        ++at;
        auto scope = parseScopeWords(words, at);
        if (!scope) {
            return scope.error();
        }
        auto match = matchScope(document, *scope, views);
        if (!match) {
            return match.error();
        }
        for (const EntityId id : match->matched) {
            const Entity* entity = document.model().entities.find(id);
            if (entity == nullptr) {
                continue;
            }
            auto polygon = volumePolygonOf(*entity);
            if (polygon) {
                result.polygons.push_back(std::move(*polygon));
            } else if (isOpenOutline(*entity)) {
                ++result.skippedOpen;
            } else {
                ++result.skippedOther;
            }
        }
        result.scope = std::move(*match);
        return result;
    }
    if (words[at].starts_with('#')) {
        while (at < words.size() && words[at].starts_with('#')) {
            const auto id = katana::core::parseInteger(std::string_view(words[at]).substr(1));
            if (!id || *id <= 0) {
                return makeError(ErrorCode::ParseFailure, "an entity is #id, a whole number",
                                 words[at]);
            }
            const Entity* entity =
                document.model().entities.find(static_cast<EntityId>(*id));
            if (entity == nullptr) {
                return makeError(ErrorCode::NotFound, "the drawing has no entity " + words[at],
                                 words[at]);
            }
            auto polygon = volumePolygonOf(*entity);
            if (!polygon) {
                return polygon.error();
            }
            result.polygons.push_back(std::move(*polygon));
            ++at;
        }
        return result;
    }
    VolumePolygon typed;
    while (at < words.size()) {
        const auto corner = cornerOf(words[at]);
        if (!corner) {
            break;
        }
        typed.ring.vertices.push_back(corner->first);
        typed.heights.push_back(corner->second);
        ++at;
    }
    if (typed.ring.vertices.size() < 3) {
        return makeError(ErrorCode::ParseFailure,
                         "a typed polygon is three or more corners x,y or x,y,z",
                         at < words.size() ? words[at] : std::string());
    }
    dropClosingRepeat(typed);
    result.polygons.push_back(std::move(typed));
    return result;
}

} // namespace katana::cad
