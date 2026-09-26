#include "katana/cad/annotation/dimension_build.hpp"

#include <cmath>
#include <string>
#include <variant>

#include "katana/cad/dimension_draw.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::annotation {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::AnchorPoint;
using katana::entity::AnchorRef;
using katana::entity::DimensionGeometry;
using katana::entity::DimensionKind;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
namespace tol = katana::math::tolerance;

namespace {

Result<DimensionGeometry> checked(DimensionGeometry dimension)
{
    if (auto status = katana::entity::validate(dimension); !status) {
        return status.error();
    }
    return dimension;
}

Vec2 unit(double angle)
{
    return Vec2(std::cos(angle), std::sin(angle));
}

} // namespace

Result<AnchoredPoint> anchoredPoint(const katana::entity::Model& model, const AnchorRef& ref)
{
    const katana::entity::Entity* entity = model.entities.find(ref.entity);
    if (entity == nullptr) {
        return makeError(ErrorCode::NotFound, "entity does not exist",
                         "id=" + std::to_string(ref.entity));
    }
    const auto point = katana::entity::resolveAnchor(*entity, ref);
    if (!point) {
        return makeError(ErrorCode::InvalidArgument,
                         "that entity has no such point to dimension to",
                         "id=" + std::to_string(ref.entity) + " " + katana::entity::describe(ref));
    }
    return AnchoredPoint{*point, ref};
}

AnchorPoint defaultAnchor(const katana::entity::Geometry& geometry)
{
    if (std::holds_alternative<katana::geometry::Arc2>(geometry) ||
        std::holds_alternative<katana::geometry::Circle2>(geometry) ||
        std::holds_alternative<katana::geometry::Ellipse2>(geometry)) {
        return AnchorPoint::Centre;
    }
    if (std::holds_alternative<katana::geometry::Segment2>(geometry) ||
        std::holds_alternative<katana::geometry::Polyline2>(geometry) ||
        std::holds_alternative<katana::geometry::CurvePolyline2>(geometry) ||
        std::holds_alternative<katana::geometry::Spline2>(geometry) ||
        std::holds_alternative<katana::entity::LeaderGeometry>(geometry)) {
        return AnchorPoint::Start;
    }
    return AnchorPoint::Position;
}

Result<DimensionGeometry> alignedDimension(const AnchoredPoint& a, const AnchoredPoint& b,
                                           const Point2& at)
{
    const Vec2 span = b.point - a.point;
    if (!(span.length() > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidArgument, "the two points coincide");
    }
    DimensionGeometry dimension;
    dimension.start = a.point;
    dimension.end = b.point;
    dimension.startRef = a.ref;
    dimension.endRef = b.ref;
    dimension.offset = (at - a.point).dot(span.normalized().perpendicular());
    return checked(dimension);
}

Result<DimensionGeometry> linearDimension(const AnchoredPoint& a, const AnchoredPoint& b,
                                          const Point2& at, std::optional<double> angle)
{
    double direction = 0.0;
    if (angle) {
        direction = *angle;
    } else {
        // Where `at` is beyond the box of the two points: further above or
        // below it than beside it reads as a horizontal dimension.
        const Point2 middle = (a.point + b.point) * 0.5;
        const double halfWidth = 0.5 * std::abs(b.point.x - a.point.x);
        const double halfHeight = 0.5 * std::abs(b.point.y - a.point.y);
        const double beside = std::max(0.0, std::abs(at.x - middle.x) - halfWidth);
        const double above = std::max(0.0, std::abs(at.y - middle.y) - halfHeight);
        const bool horizontal = above >= beside;
        direction = horizontal ? 0.0 : 0.5 * katana::math::kPi;
        // Nothing to measure that way: the other way is what was meant.
        if (std::abs((b.point - a.point).dot(unit(direction))) <= tol::kGeometric) {
            direction = horizontal ? 0.5 * katana::math::kPi : 0.0;
        }
    }
    const Vec2 along = unit(direction);
    if (!(std::abs((b.point - a.point).dot(along)) > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the two points are level along the dimension's direction");
    }
    DimensionGeometry dimension;
    dimension.kind = DimensionKind::Linear;
    dimension.angle = direction;
    dimension.start = a.point;
    dimension.end = b.point;
    dimension.startRef = a.ref;
    dimension.endRef = b.ref;
    dimension.offset = (at - a.point).dot(along.perpendicular());
    return checked(dimension);
}

Result<DimensionGeometry> angularDimension(const AnchoredPoint& vertex, const AnchoredPoint& first,
                                           const AnchoredPoint& second,
                                           const std::optional<Point2>& at)
{
    DimensionGeometry dimension;
    dimension.kind = DimensionKind::Angular;
    dimension.vertex = vertex.point;
    dimension.vertexRef = vertex.ref;
    dimension.start = first.point;
    dimension.startRef = first.ref;
    dimension.end = second.point;
    dimension.endRef = second.ref;
    if (at) {
        const Vec2 toAt = *at - vertex.point;
        dimension.offset = toAt.length();
        // The side `at` is on: when the arc through it is not in the
        // counter-clockwise sweep from first to second, the rays swap and
        // the other angle is measured.
        const double sweep = dimension.measurement();
        const double atAngle =
            katana::math::normalizeAngle(toAt.angle() - (first.point - vertex.point).angle());
        if (atAngle > sweep) {
            std::swap(dimension.start, dimension.end);
            std::swap(dimension.startRef, dimension.endRef);
        }
    }
    return checked(dimension);
}

Result<DimensionGeometry> angularBetweenLines(const katana::entity::Model& model,
                                              katana::entity::EntityId first,
                                              katana::entity::EntityId second, const Point2& at)
{
    const auto lineOf = [&](katana::entity::EntityId id) -> Result<Segment2> {
        const katana::entity::Entity* entity = model.entities.find(id);
        if (entity == nullptr) {
            return makeError(ErrorCode::NotFound, "entity does not exist", "id=" + std::to_string(id));
        }
        const auto* line = std::get_if<Segment2>(&entity->geometry);
        if (line == nullptr) {
            return makeError(ErrorCode::InvalidArgument, "an angle between lines needs two lines",
                             "id=" + std::to_string(id));
        }
        return *line;
    };
    const auto a = lineOf(first);
    const auto b = lineOf(second);
    if (!a) {
        return a.error();
    }
    if (!b) {
        return b.error();
    }
    const Vec2 da = a->end - a->start;
    const Vec2 db = b->end - b->start;
    const double denominator = da.cross(db);
    if (std::abs(denominator) <= tol::kAngular * da.length() * db.length()) {
        return makeError(ErrorCode::InvalidArgument, "the lines are parallel: there is no angle");
    }
    const double t = (b->start - a->start).cross(db) / denominator;
    const Point2 vertex = a->start + da * t;
    // Each ray runs to the line's end on the side `at` is: the end whose
    // direction from the vertex is nearer the direction to `at`.
    const auto rayEnd = [&](const Segment2& line, katana::entity::EntityId id) {
        const Vec2 toAt = at - vertex;
        const double startScore = (line.start - vertex).dot(toAt);
        const double endScore = (line.end - vertex).dot(toAt);
        const bool useEnd = endScore >= startScore;
        return AnchoredPoint{useEnd ? line.end : line.start,
                             AnchorRef{id, useEnd ? AnchorPoint::End : AnchorPoint::Start, 0}};
    };
    return angularDimension(AnchoredPoint{vertex, {}}, rayEnd(*a, first), rayEnd(*b, second), at);
}

Result<DimensionGeometry> radialDimension(const katana::entity::Model& model,
                                          katana::entity::EntityId curve, const Point2& at,
                                          bool diameter)
{
    const katana::entity::Entity* entity = model.entities.find(curve);
    if (entity == nullptr) {
        return makeError(ErrorCode::NotFound, "entity does not exist", "id=" + std::to_string(curve));
    }
    Point2 centre;
    double radius = 0.0;
    if (const auto* circle = std::get_if<katana::geometry::Circle2>(&entity->geometry)) {
        centre = circle->center;
        radius = circle->radius;
    } else if (const auto* arc = std::get_if<katana::geometry::Arc2>(&entity->geometry)) {
        centre = arc->center;
        radius = arc->radius;
    } else {
        return makeError(ErrorCode::InvalidArgument, "a radius or diameter needs a circle or an arc",
                         "id=" + std::to_string(curve));
    }
    const Vec2 toAt = at - centre;
    const Vec2 direction = toAt.length() > tol::kGeometric ? toAt.normalized() : Vec2(1.0, 0.0);
    DimensionGeometry dimension;
    dimension.kind = diameter ? DimensionKind::Diameter : DimensionKind::Radius;
    dimension.vertex = centre;
    dimension.vertexRef = AnchorRef{curve, AnchorPoint::Centre, 0};
    dimension.start = centre + direction * radius;
    dimension.offset = std::max(0.0, toAt.length() - radius);
    return checked(dimension);
}

Result<DimensionGeometry> ordinateDimension(const AnchoredPoint& datum, const AnchoredPoint& feature,
                                            const Point2& at, std::optional<bool> xAxis)
{
    const Vec2 leader = at - feature.point;
    const bool x = xAxis.value_or(std::abs(leader.y) > std::abs(leader.x));
    DimensionGeometry dimension;
    dimension.kind = x ? DimensionKind::OrdinateX : DimensionKind::OrdinateY;
    dimension.vertex = datum.point;
    dimension.vertexRef = datum.ref;
    dimension.start = feature.point;
    dimension.startRef = feature.ref;
    dimension.end = at;
    return checked(dimension);
}

Result<std::vector<DimensionGeometry>> baselineDimensions(const DimensionGeometry& base,
                                                          const std::vector<AnchoredPoint>& points,
                                                          double spacing)
{
    if (base.kind != DimensionKind::Aligned && base.kind != DimensionKind::Linear) {
        return makeError(ErrorCode::InvalidArgument,
                         "a baseline chain continues an aligned or linear dimension");
    }
    if (!std::isfinite(spacing) || !(spacing > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "the baseline spacing must be positive");
    }
    std::vector<DimensionGeometry> chain;
    const double outward = base.offset >= 0.0 ? 1.0 : -1.0;
    double offset = base.offset;
    for (const AnchoredPoint& point : points) {
        offset += outward * spacing;
        DimensionGeometry next = base;
        next.end = point.point;
        next.endRef = point.ref;
        next.textOverride.clear();
        next.offset = offset;
        auto made = checked(next);
        if (!made) {
            return made.error();
        }
        chain.push_back(std::move(*made));
    }
    return chain;
}

Result<std::vector<DimensionGeometry>> continuedDimensions(const DimensionGeometry& base,
                                                           const std::vector<AnchoredPoint>& points)
{
    if (base.kind != DimensionKind::Aligned && base.kind != DimensionKind::Linear) {
        return makeError(ErrorCode::InvalidArgument,
                         "a continued chain continues an aligned or linear dimension");
    }
    std::vector<DimensionGeometry> chain;
    DimensionGeometry previous = base;
    for (const AnchoredPoint& point : points) {
        DimensionGeometry next = previous;
        next.textOverride.clear();
        next.start = previous.end;
        next.startRef = previous.endRef;
        next.end = point.point;
        next.endRef = point.ref;
        if (base.kind == DimensionKind::Linear) {
            // On the previous dimension's line: that line is the points X
            // with (X - start) . n = offset, so from the new start it is at
            // offset - (new start - old start) . n.
            const Vec2 normal = unit(base.angle).perpendicular();
            next.offset = previous.offset - (next.start - previous.start).dot(normal);
        } else {
            // An aligned chain turns with each span; each keeps the offset.
            next.offset = previous.offset;
        }
        auto made = checked(next);
        if (!made) {
            return made.error();
        }
        chain.push_back(*made);
        previous = std::move(*made);
    }
    return chain;
}

double baselineSpacing(const katana::entity::Model& model, const katana::entity::Entity& base,
                       double scale)
{
    const auto style = dimensionStyleAtScale(resolveDimensionStyle(model, base), scale);
    return 1.5 * style.textHeight;
}

Result<katana::commands::CommandPtr> dimensionChain(const katana::entity::Model& model,
                                                    katana::entity::EntityId base,
                                                    const std::vector<AnchoredPoint>& points,
                                                    DimensionChain kind,
                                                    std::optional<double> spacing, double scale)
{
    const katana::entity::Entity* entity = model.entities.find(base);
    const auto* dimension =
        entity != nullptr ? std::get_if<DimensionGeometry>(&entity->geometry) : nullptr;
    if (dimension == nullptr) {
        return makeError(ErrorCode::InvalidArgument, "that entity is not a dimension",
                         "id=" + std::to_string(base));
    }
    if (points.empty()) {
        return makeError(ErrorCode::InvalidArgument, "a chain needs at least one more point");
    }
    auto chain = kind == DimensionChain::Baseline
                     ? baselineDimensions(*dimension, points,
                                          spacing.value_or(baselineSpacing(model, *entity, scale)))
                     : continuedDimensions(*dimension, points);
    if (!chain) {
        return chain.error();
    }
    katana::commands::ChangeSet changes;
    for (auto& geometry : *chain) {
        katana::entity::Entity made;
        made.geometry = std::move(geometry);
        made.layer = entity->layer;
        made.style = entity->style;
        made.color = entity->color;
        changes.add.push_back(std::move(made));
    }
    return katana::commands::CommandPtr(std::make_unique<katana::commands::ChangeSetCommand>(
        kind == DimensionChain::Baseline ? "DIM_BASELINE" : "DIM_CONTINUE",
        [changes](const katana::commands::CommandContext&)
            -> Result<katana::commands::ChangeSet> { return changes; }));
}

} // namespace katana::cad::annotation
