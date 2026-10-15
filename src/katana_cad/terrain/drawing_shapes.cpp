// The one reader of the drawing's polygons and lines for the surface edits
// and the volumes (terrain/drawing_shapes.hpp, which states the rules).

#include "katana/cad/terrain/drawing_shapes.hpp"

#include <string>
#include <utility>
#include <variant>

#include "katana/core/text.hpp"
#include "katana/geometry/chording.hpp"
#include "katana/geometry/curves2d.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;

namespace {

ShapeOf none(NotAShape why) { return ShapeOf{std::nullopt, why}; }

ShapeOf some(DrawingShape shape)
{
    ShapeOf of;
    of.shape = std::move(shape);
    return of;
}

bool backAtItsStart(const std::vector<Point2>& vertices)
{
    return vertices.size() > 1 &&
           vertices.front().distanceTo(vertices.back()) <= katana::math::tolerance::kGeometric;
}

// A closed outline names each corner once: the first repeated at the end -
// as a ring drawn back to its start, and every closed curve's walk, give it -
// is taken off with its height, as often as it is there.
void dropClosingRepeat(DrawingShape& shape)
{
    while (backAtItsStart(shape.outline.vertices)) {
        shape.outline.vertices.pop_back();
        shape.heights.pop_back();
    }
}

ShapeOf asPolygon(DrawingShape shape)
{
    shape.outline.closed = true;
    dropClosingRepeat(shape);
    if (shape.outline.vertices.size() < 3) {
        return none(NotAShape::TooFew);
    }
    return some(std::move(shape));
}

ShapeOf asLine(DrawingShape shape)
{
    if (shape.outline.closed) {
        dropClosingRepeat(shape);
    }
    if (shape.outline.vertices.size() < 2) {
        return none(NotAShape::TooFew);
    }
    return some(std::move(shape));
}

DrawingShape vertexShape(const katana::geometry::Polyline2& polyline, const Entity& entity)
{
    DrawingShape shape;
    shape.outline = polyline;
    shape.heights = katana::entity::heightsOf(entity.properties, polyline.vertices.size());
    return shape;
}

// The walk of a curve polyline, its arcs chorded and every point at its
// height: a vertex's own, a chord point's interpolated along its arc. A
// closed one's walk ends on its first point again.
DrawingShape walkOf(const katana::geometry::CurvePolyline2& curve)
{
    DrawingShape shape;
    for (const auto& point : curve.tessellateWithHeights(katana::geometry::kCurveChordTolerance)) {
        shape.outline.vertices.push_back(point.position);
        shape.heights.push_back(point.height);
    }
    shape.outline.closed = curve.closed;
    shape.chorded = curve.hasArcs();
    return shape;
}

// A circle's or an ellipse's chords, every corner at the entity's one
// elevation: an elevations list would have to have a value per chord point,
// which no list written for the entity has (heightsOf passes it over).
DrawingShape chordShape(std::vector<Point2> ring, const Entity& entity)
{
    DrawingShape shape;
    shape.heights = katana::entity::heightsOf(entity.properties, ring.size());
    shape.outline.vertices = std::move(ring);
    shape.outline.closed = true;
    shape.chorded = true;
    return shape;
}

ShapeOf polygonOf(const Entity& entity)
{
    using namespace katana::geometry;
    if (const auto* polyline = std::get_if<Polyline2>(&entity.geometry)) {
        if (!polyline->closed && !backAtItsStart(polyline->vertices)) {
            return none(NotAShape::Open);
        }
        return asPolygon(vertexShape(*polyline, entity));
    }
    if (const auto* curve = std::get_if<CurvePolyline2>(&entity.geometry)) {
        DrawingShape walk = walkOf(*curve);
        if (!curve->closed && !backAtItsStart(walk.outline.vertices)) {
            return none(NotAShape::Open);
        }
        return asPolygon(std::move(walk));
    }
    if (const auto* circle = std::get_if<Circle2>(&entity.geometry)) {
        return asPolygon(chordShape(chordCircle(*circle, kCurveChordTolerance), entity));
    }
    if (const auto* ellipse = std::get_if<Ellipse2>(&entity.geometry)) {
        if (!ellipse->isFull()) {
            return none(NotAShape::Open);
        }
        return asPolygon(chordShape(ellipse->tessellate(kCurveChordTolerance), entity));
    }
    if (std::holds_alternative<Segment2>(entity.geometry) ||
        std::holds_alternative<Arc2>(entity.geometry)) {
        return none(NotAShape::Open);
    }
    return none(NotAShape::NoOutline);
}

ShapeOf lineOf(const Entity& entity)
{
    using namespace katana::geometry;
    if (const auto* segment = std::get_if<Segment2>(&entity.geometry)) {
        return asLine(vertexShape(Polyline2{{segment->start, segment->end}, false}, entity));
    }
    if (const auto* polyline = std::get_if<Polyline2>(&entity.geometry)) {
        return asLine(vertexShape(*polyline, entity));
    }
    if (const auto* curve = std::get_if<CurvePolyline2>(&entity.geometry)) {
        return asLine(walkOf(*curve));
    }
    return none(NotAShape::NoOutline);
}

} // namespace

std::optional<TypedCorner> parseTypedCorner(std::string_view word)
{
    std::vector<double> parts;
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
        parts.push_back(*value);
        if (comma == std::string_view::npos) {
            break;
        }
        start = comma + 1;
    }
    if (parts.size() != 2 && parts.size() != 3) {
        return std::nullopt;
    }
    TypedCorner corner{Point2(parts[0], parts[1]), std::nullopt};
    if (parts.size() == 3) {
        corner.z = parts[2];
    }
    return corner;
}

ShapeOf shapeOf(const Entity& entity, ShapeKind kind)
{
    ShapeOf of = kind == ShapeKind::Polygon ? polygonOf(entity) : lineOf(entity);
    if (of.shape) {
        of.shape->entity = entity.id;
    }
    return of;
}

ShapeOf shapeOf(const std::vector<TypedCorner>& corners, ShapeKind kind)
{
    DrawingShape shape;
    for (const TypedCorner& corner : corners) {
        shape.outline.vertices.push_back(corner.at);
        shape.heights.push_back(corner.z);
    }
    return kind == ShapeKind::Polygon ? asPolygon(std::move(shape)) : asLine(std::move(shape));
}

Result<ShapeWords> readShapeWords(const std::vector<std::string>& words, std::size_t& at)
{
    ShapeWords read;
    if (at >= words.size()) {
        return read;
    }
    if (katana::core::equalsIgnoringCase(words[at], "FROM")) {
        ++at;
        auto scope = parseScopeWords(words, at);
        if (!scope) {
            return scope.error();
        }
        read.scope = std::move(*scope);
        return read;
    }
    if (words[at].starts_with('#')) {
        while (at < words.size() && words[at].starts_with('#')) {
            const auto id = katana::core::parseInteger(std::string_view(words[at]).substr(1));
            if (!id || *id <= 0) {
                read.badId = words[at];
                return read;
            }
            read.ids.push_back(static_cast<EntityId>(*id));
            ++at;
        }
        return read;
    }
    while (at < words.size()) {
        const auto corner = parseTypedCorner(words[at]);
        if (!corner) {
            break;
        }
        read.corners.push_back(*corner);
        ++at;
    }
    return read;
}

Result<NamedEntities> entitiesNamed(const Document& document, const ShapeWords& words,
                                    const ScopeViewProvider& views)
{
    NamedEntities named;
    if (words.scope) {
        auto match = matchScope(document, *words.scope, views);
        if (!match) {
            return match.error();
        }
        named.ids = match->matched;
        named.scope = std::move(*match);
        return named;
    }
    for (const EntityId id : words.ids) {
        if (document.model().entities.find(id) == nullptr) {
            named.missing = id;
            return named;
        }
        named.ids.push_back(id);
    }
    return named;
}

Result<DrawingShapes> resolveShapes(const Document& document, const ShapeWords& words,
                                    ShapeKind kind, const ScopeViewProvider& views)
{
    DrawingShapes taken;
    if (!words.scope && words.ids.empty()) {
        ShapeOf typed = shapeOf(words.corners, kind);
        if (!typed.shape) {
            taken.refused = ShapeRefusal{std::nullopt, typed.why};
            return taken;
        }
        taken.shapes.push_back(std::move(*typed.shape));
        return taken;
    }
    auto named = entitiesNamed(document, words, views);
    if (!named) {
        return named.error();
    }
    if (named->missing) {
        taken.refused = ShapeRefusal{named->missing, NotAShape::NoEntity};
        return taken;
    }
    taken.scope = std::move(named->scope);
    for (const EntityId id : named->ids) {
        const Entity* entity = document.model().entities.find(id);
        if (entity == nullptr) {
            continue; // a scope matches what the drawing has; checked above for #ids
        }
        ShapeOf of = shapeOf(*entity, kind);
        if (of.shape) {
            taken.shapes.push_back(std::move(*of.shape));
        } else if (!taken.scope) {
            taken.refused = ShapeRefusal{id, of.why};
            return taken;
        } else if (of.why == NotAShape::Open) {
            ++taken.skipped.open;
        } else if (of.why == NotAShape::TooFew) {
            ++taken.skipped.tooFew;
        } else {
            ++taken.skipped.noOutline;
        }
    }
    return taken;
}

} // namespace katana::cad
