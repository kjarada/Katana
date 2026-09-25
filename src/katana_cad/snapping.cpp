#include "katana/cad/snapping.hpp"

#include "katana/cad/spatial_query.hpp"

#include <cmath>
#include <cstdint>
#include <variant>
#include <vector>

#include "katana/cad/selection.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/geometry/editing.hpp"

namespace katana::cad {

using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Arc2;
using katana::geometry::Box2;
using katana::geometry::Circle2;
using katana::geometry::Curve2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;

const char* toString(SnapMode mode)
{
    switch (mode) {
    case SnapMode::None:
        return "None";
    case SnapMode::Endpoint:
        return "Endpoint";
    case SnapMode::Midpoint:
        return "Midpoint";
    case SnapMode::Center:
        return "Center";
    case SnapMode::Intersection:
        return "Intersection";
    case SnapMode::Perpendicular:
        return "Perpendicular";
    case SnapMode::Tangent:
        return "Tangent";
    case SnapMode::Nearest:
        return "Nearest";
    case SnapMode::Grid:
        return "Grid";
    }
    return "Unknown";
}

namespace {

// Lower rank wins ties between equally distant candidates.
int rankOf(SnapMode mode)
{
    switch (mode) {
    case SnapMode::Endpoint:
        return 0;
    case SnapMode::Intersection:
        return 1;
    case SnapMode::Midpoint:
        return 2;
    case SnapMode::Center:
        return 3;
    case SnapMode::Perpendicular:
        return 4;
    case SnapMode::Tangent:
        return 5;
    default:
        return 6;
    }
}

class Collector {
  public:
    explicit Collector(const SnapRequest& request) : request_(request) {}

    // Candidate that must itself lie within the aperture of the cursor.
    void offer(const Point2& point, SnapMode mode, EntityId entity)
    {
        if (hasMode(request_.modes, mode)) {
            consider(point, mode, entity, point.distanceTo(request_.cursor));
        }
    }

    // A centre is offered while the cursor hovers the curve, not just the centre
    // itself. Hovering the curve ranks as far away as the aperture allows, so any
    // exact snap on that curve still wins.
    void offerCenter(const Point2& center, double distanceToCurve, EntityId entity)
    {
        if (!hasMode(request_.modes, SnapMode::Center)) {
            return;
        }
        const double toCenter = center.distanceTo(request_.cursor);
        if (toCenter <= request_.aperture) {
            consider(center, SnapMode::Center, entity, toCenter);
        } else if (distanceToCurve <= request_.aperture) {
            consider(center, SnapMode::Center, entity, request_.aperture);
        }
    }

    [[nodiscard]] const std::optional<SnapResult>& best() const { return best_; }

  private:
    void consider(const Point2& point, SnapMode mode, EntityId entity, double distance)
    {
        if (distance > request_.aperture) {
            return;
        }
        const bool better = !best_ || distance < bestDistance_ ||
                            (distance == bestDistance_ && rankOf(mode) < rankOf(best_->mode));
        if (better) {
            best_ = SnapResult{point, mode, entity};
            bestDistance_ = distance;
        }
    }

    const SnapRequest& request_;
    std::optional<SnapResult> best_;
    double bestDistance_ = 0.0;
};

// Points where a line from `from` meets the circle at a right angle: the two
// ends of the diameter through `from`.
std::vector<Point2> perpendicularFeet(const Circle2& circle, const Point2& from)
{
    const Vec2 direction = (from - circle.center).normalized();
    if (direction == Vec2{}) {
        return {}; // from the centre every radius is perpendicular; no unique foot
    }
    return {circle.center + direction * circle.radius, circle.center - direction * circle.radius};
}

// Tangent points of the lines through `from`. None when `from` is inside.
std::vector<Point2> tangentPoints(const Circle2& circle, const Point2& from)
{
    const Vec2 toFrom = from - circle.center;
    const double distance = toFrom.length();
    if (!(distance > circle.radius + katana::math::tolerance::kGeometric)) {
        return {};
    }
    // The radius to a tangent point makes angle acos(r/d) with the centre->from line.
    const double angle = std::acos(circle.radius / distance);
    const Vec2 unit = toFrom / distance;
    return {circle.center + unit.rotated(angle) * circle.radius,
            circle.center + unit.rotated(-angle) * circle.radius};
}

bool onArc(const Arc2& arc, const Point2& point)
{
    return arc.containsAngle((point - arc.center).angle(),
                             katana::math::tolerance::kGeometric / arc.radius);
}

struct EntitySnaps {
    Collector& collector;
    const SnapRequest& request;
    EntityId id;

    void segment(const Segment2& s, bool withEndpoints = true) const
    {
        if (withEndpoints) {
            collector.offer(s.start, SnapMode::Endpoint, id);
            collector.offer(s.end, SnapMode::Endpoint, id);
        }
        collector.offer(s.midpoint(), SnapMode::Midpoint, id);
        if (request.from && !s.isDegenerate()) {
            const katana::geometry::Line2 line{s.start, s.delta()};
            const Point2 foot = line.closestPoint(*request.from);
            if (s.distanceTo(foot) <= katana::math::tolerance::kGeometric) {
                collector.offer(foot, SnapMode::Perpendicular, id);
            }
        }
    }

    void operator()(const katana::entity::PointGeometry& g) const
    {
        collector.offer(g.position, SnapMode::Endpoint, id);
    }
    void operator()(const Segment2& g) const { segment(g); }
    void operator()(const Polyline2& g) const
    {
        for (const Point2& vertex : g.vertices) {
            collector.offer(vertex, SnapMode::Endpoint, id);
        }
        for (std::size_t i = 0; i < g.segmentCount(); ++i) {
            segment(g.segment(i), /*withEndpoints=*/false);
        }
    }
    void operator()(const Circle2& g) const
    {
        collector.offerCenter(g.center, g.distanceTo(request.cursor), id);
        if (request.from) {
            for (const Point2& p : perpendicularFeet(g, *request.from)) {
                collector.offer(p, SnapMode::Perpendicular, id);
            }
            for (const Point2& p : tangentPoints(g, *request.from)) {
                collector.offer(p, SnapMode::Tangent, id);
            }
        }
    }
    void operator()(const Arc2& g) const
    {
        collector.offer(g.startPoint(), SnapMode::Endpoint, id);
        collector.offer(g.endPoint(), SnapMode::Endpoint, id);
        collector.offer(g.midpoint(), SnapMode::Midpoint, id);
        collector.offerCenter(g.center, g.distanceTo(request.cursor), id);
        if (request.from) {
            for (const Point2& p : perpendicularFeet(g.circle(), *request.from)) {
                if (onArc(g, p)) {
                    collector.offer(p, SnapMode::Perpendicular, id);
                }
            }
            for (const Point2& p : tangentPoints(g.circle(), *request.from)) {
                if (onArc(g, p)) {
                    collector.offer(p, SnapMode::Tangent, id);
                }
            }
        }
    }
    void operator()(const katana::entity::TextGeometry& g) const
    {
        collector.offer(g.position, SnapMode::Endpoint, id); // insertion point
    }
    void operator()(const katana::entity::DimensionGeometry& g) const
    {
        collector.offer(g.start, SnapMode::Endpoint, id);
        collector.offer(g.end, SnapMode::Endpoint, id);
        if (g.usesVertex()) {
            collector.offer(g.vertex, SnapMode::Endpoint, id);
        }
    }
    // A label offers nothing: its text is not geometry anyone draws to.
    void operator()(const katana::entity::LabelGeometry&) const {}
    // A leader's tip and each bend, as a polyline's vertices.
    void operator()(const katana::entity::LeaderGeometry& g) const
    {
        for (const Point2& vertex : g.vertices) {
            collector.offer(vertex, SnapMode::Endpoint, id);
        }
    }
};

// Curves of an entity that can take part in intersection and nearest snaps.
//
// A polyline's SEGMENTS are culled individually against the cursor's reach. The
// entity-level filter admits the whole polyline when any part of it is near the
// cursor, so a 3000-vertex surveyed string contributed 3000 curves to a loop
// that intersects every pair - 4.5 million intersections, measured at 247 ms per
// mouse move in Release, against section 32's 16 ms interaction budget.
//
// The cull is exact, not an approximation. Every candidate is accepted only if
// it lies within `aperture` of the cursor (Collector::consider, and the Nearest
// pass below), and every candidate lies ON the curve that produced it. So an
// accepted point is inside `reach`, and the bounding box of the curve carrying
// it therefore meets `reach` too: a segment whose box misses `reach` cannot
// contribute a point that would have been accepted.
void appendCurves(const Entity& entity, const Box2& reach,
                  std::vector<std::pair<EntityId, Curve2>>& curves)
{
    if (const auto* segment = std::get_if<Segment2>(&entity.geometry)) {
        curves.emplace_back(entity.id, *segment);
    } else if (const auto* arc = std::get_if<Arc2>(&entity.geometry)) {
        curves.emplace_back(entity.id, *arc);
    } else if (const auto* circle = std::get_if<Circle2>(&entity.geometry)) {
        curves.emplace_back(entity.id, *circle);
    } else if (const auto* polyline = std::get_if<Polyline2>(&entity.geometry)) {
        for (std::size_t i = 0; i < polyline->segmentCount(); ++i) {
            const Segment2 piece = polyline->segment(i);
            if (piece.boundingBox().intersects(reach)) {
                curves.emplace_back(entity.id, piece);
            }
        }
    }
}

// An arc's centre can lie outside the arc's own extents; use the full circle.
} // namespace

std::optional<SnapResult> snap(const katana::entity::Model& model, const SnapRequest& request,
                               const katana::geometry::SpatialIndex* index)
{
    Collector collector(request);
    const Box2 reach = Box2(request.cursor, request.cursor).inflated(request.aperture);

    std::vector<const Entity*> nearby;
    std::vector<katana::geometry::SpatialId> scratch;
    // The extent test is detail::queryExtents, which is deliberately wider
    // than the bounding box for an arc so that its centre stays snappable.
    detail::forEachCandidate(model, index, reach, scratch, [&](const Entity& entity) {
        if (isDrawn(model, entity,
                    request.view != nullptr ? *request.view : kNoLayerOverrides)) {
            nearby.push_back(&entity);
        }
    });

    std::vector<std::pair<EntityId, Curve2>> curves; // used by Intersection and Nearest
    for (const Entity* entity : nearby) {
        std::visit(EntitySnaps{collector, request, entity->id}, entity->geometry);
        appendCurves(*entity, reach, curves);
    }

    const std::size_t pairLimit = hasMode(request.modes, SnapMode::Intersection) ? curves.size() : 0;
    for (std::size_t i = 0; i < pairLimit; ++i) {
        for (std::size_t j = i + 1; j < pairLimit; ++j) {
            const auto hit = katana::geometry::intersect(curves[i].second, curves[j].second);
            if (hit.kind != katana::geometry::IntersectionKind::Points) {
                continue;
            }
            for (std::size_t k = 0; k < hit.count; ++k) {
                collector.offer(hit.points[k], SnapMode::Intersection, curves[i].first);
            }
        }
    }
    if (collector.best()) {
        return collector.best();
    }

    if (hasMode(request.modes, SnapMode::Nearest)) {
        std::optional<SnapResult> nearest;
        double nearestDistance = request.aperture;
        for (const auto& [id, curve] : curves) {
            const Point2 point = katana::geometry::closestPoint(curve, request.cursor);
            const double distance = point.distanceTo(request.cursor);
            if (distance <= nearestDistance) {
                nearestDistance = distance;
                nearest = SnapResult{point, SnapMode::Nearest, id};
            }
        }
        if (nearest) {
            return nearest;
        }
    }

    if (hasMode(request.modes, SnapMode::Grid) && request.gridSpacing > 0.0) {
        const double spacing = request.gridSpacing;
        return SnapResult{Point2(std::round(request.cursor.x / spacing) * spacing,
                                 std::round(request.cursor.y / spacing) * spacing),
                          SnapMode::Grid, katana::entity::kInvalidEntityId};
    }
    return std::nullopt;
}

std::optional<katana::entity::AnchorRef> snapAnchor(const katana::entity::Model& model,
                                                    const SnapResult& snap)
{
    using katana::entity::AnchorPoint;
    using katana::entity::AnchorRef;
    if (snap.mode != SnapMode::Endpoint && snap.mode != SnapMode::Midpoint &&
        snap.mode != SnapMode::Center) {
        return std::nullopt;
    }
    const Entity* entity = model.entities.find(snap.entity);
    if (entity == nullptr) {
        return std::nullopt;
    }
    // The points each kind names, in the order a tie is settled: a
    // polyline's corners by their vertex, not as its start or end.
    std::vector<AnchorRef> names;
    const auto name = [&](AnchorPoint point, std::uint32_t index = 0) {
        names.push_back(AnchorRef{entity->id, point, index});
    };
    const auto& geometry = entity->geometry;
    if (std::holds_alternative<Segment2>(geometry)) {
        name(AnchorPoint::Start);
        name(AnchorPoint::End);
        name(AnchorPoint::Mid);
    } else if (std::holds_alternative<Arc2>(geometry)) {
        name(AnchorPoint::Start);
        name(AnchorPoint::End);
        name(AnchorPoint::Mid);
        name(AnchorPoint::Centre);
    } else if (std::holds_alternative<Circle2>(geometry)) {
        name(AnchorPoint::Centre);
    } else if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        for (std::uint32_t i = 0; i < polyline->vertices.size(); ++i) {
            name(AnchorPoint::Vertex, i);
        }
        for (std::uint32_t i = 0; i < polyline->segmentCount(); ++i) {
            name(AnchorPoint::SegmentMid, i);
        }
    } else if (std::holds_alternative<katana::entity::LeaderGeometry>(geometry)) {
        name(AnchorPoint::Start);
        name(AnchorPoint::End);
    } else {
        name(AnchorPoint::Position);
    }
    for (const AnchorRef& ref : names) {
        const auto at = katana::entity::resolveAnchor(*entity, ref);
        if (at && at->distanceTo(snap.point) <= katana::math::tolerance::kGeometric) {
            return ref;
        }
    }
    return std::nullopt;
}

} // namespace katana::cad
