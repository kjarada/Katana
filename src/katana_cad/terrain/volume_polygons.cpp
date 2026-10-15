// The boundaries the volume verbs measure inside (terrain/volume_verbs.hpp):
// the drawing's polygons as the one reader reads them (drawing_shapes.hpp),
// refused in these verbs' own words.

#include "katana/cad/terrain/volume_verbs.hpp"

#include <string>
#include <type_traits>
#include <utility>
#include <variant>

#include "katana/geometry/curves2d.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;

namespace {

std::string idText(EntityId id)
{
    return "#" + std::to_string(id);
}

// What an entity that is no polygon is called in a refusal.
const char* kindName(const Entity& entity, NotAShape why)
{
    if (why == NotAShape::TooFew) {
        return "an outline of fewer than three corners";
    }
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
                return "an elliptical arc"; // a full ellipse is a polygon
            } else if constexpr (std::is_same_v<Kind, katana::geometry::Spline2>) {
                return "an open spline"; // one that returns to its start is a polygon
            } else {
                return "no outline";
            }
        },
        entity.geometry);
}

} // namespace

Result<DrawingShapes> readVolumePolygons(const Document& document,
                                         const std::vector<std::string>& words, std::size_t& at,
                                         const ScopeViewProvider& views)
{
    if (at >= words.size()) {
        return makeError(ErrorCode::ParseFailure,
                         "a polygon is #id, three or more corners x,y, or FROM and a scope");
    }
    auto read = readShapeWords(words, at);
    if (!read) {
        return read.error();
    }
    if (read->badId) {
        return makeError(ErrorCode::ParseFailure, "an entity is #id, a whole number", *read->badId);
    }
    // A ring tagged as a hole joins the polygon it lies in: a lot with a hole
    // measures its area less the hole, not both as polygons of their own.
    ShapeOptions options;
    options.joinTaggedHoles = true;
    auto polygons = resolveShapes(document, *read, ShapeKind::Polygon, views, options);
    if (!polygons) {
        return polygons.error();
    }
    if (const auto& refused = polygons->refused) {
        if (!refused->entity) {
            return makeError(ErrorCode::ParseFailure,
                             "a typed polygon is three or more corners x,y or x,y,z",
                             at < words.size() ? words[at] : std::string());
        }
        const std::string id = idText(*refused->entity);
        if (refused->why == NotAShape::NoEntity) {
            return makeError(ErrorCode::NotFound, "the drawing has no entity " + id, id);
        }
        return makeError(ErrorCode::InvalidGeometry,
                         id + " is " +
                             kindName(*document.model().entities.find(*refused->entity),
                                      refused->why) +
                             "; a polygon is " + std::string(kPolygonKinds),
                         id);
    }
    return std::move(*polygons);
}

} // namespace katana::cad
