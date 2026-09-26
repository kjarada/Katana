#include "katana/cad/snapping.hpp"

#include "katana/cad/spatial_query.hpp"

#include <cmath>
#include <cstdint>
#include <variant>
#include <vector>

#include "katana/entity/curve_pieces.hpp"
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
    case SnapMode::Quadrant:
        return "Quadrant";
    case SnapMode::Node:
        return "Node";
    case SnapMode::Extension:
        return "Extension";
    case SnapMode::Parallel:
        return "Parallel";
    case SnapMode::ApparentIntersection:
        return "Apparent Intersection";
    case SnapMode::From:
        return "From";
    case SnapMode::MidBetween:
        return "Midpoint Between Two Points";
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
    // The drawing system's modes rank after the originals, and the ones that
    // are about lines NOT drawn (extensions, apparent crossings, parallels)
    // after the ones on drawn geometry.
    case SnapMode::Node:
        return 6;
    case SnapMode::Quadrant:
        return 7;
    case SnapMode::ApparentIntersection:
        return 8;
    case SnapMode::Extension:
        return 9;
    case SnapMode::Parallel:
        return 10;
    default:
        return 11;
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
        collector.offer(g.position, SnapMode::Node, id);
    }
    void quadrants(const Circle2& circle, const Arc2* within = nullptr) const
    {
        for (int k = 0; k < 4; ++k) {
            const Point2 p = circle.pointAtAngle(katana::math::kHalfPi * k);
            if (within == nullptr || onArc(*within, p)) {
                collector.offer(p, SnapMode::Quadrant, id);
            }
        }
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
        quadrants(g);
        if (request.from) {
            for (const Point2& p : perpendicularFeet(g, *request.from)) {
                collector.offer(p, SnapMode::Perpendicular, id);
            }
            for (const Point2& p : tangentPoints(g, *request.from)) {
                collector.offer(p, SnapMode::Tangent, id);
            }
        }
    }
    void operator()(const Arc2& g) const { arc(g); }
    void arc(const Arc2& g, bool withEndpoints = true) const
    {
        if (withEndpoints) {
            collector.offer(g.startPoint(), SnapMode::Endpoint, id);
            collector.offer(g.endPoint(), SnapMode::Endpoint, id);
        }
        collector.offer(g.midpoint(), SnapMode::Midpoint, id);
        collector.offerCenter(g.center, g.distanceTo(request.cursor), id);
        quadrants(g.circle(), &g);
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
    // Each vertex an endpoint; a straight segment as a Polyline2's, an arc
    // segment as an Arc2 (its midpoint, its centre, perpendicular and
    // tangent feet on it).
    void operator()(const katana::geometry::CurvePolyline2& g) const
    {
        for (const auto& vertex : g.vertices) {
            collector.offer(vertex.position, SnapMode::Endpoint, id);
        }
        for (std::size_t i = 0; i < g.segmentCount(); ++i) {
            const auto piece = g.segment(i);
            if (const auto* line = std::get_if<Segment2>(&piece)) {
                segment(*line, /*withEndpoints=*/false);
            } else {
                arc(std::get<Arc2>(piece), /*withEndpoints=*/false);
            }
        }
    }
    // The centre (while the curve is hovered, as a circle's), the ends of an
    // arc, and the ends of the axes that lie on it (the Quadrant snap).
    void operator()(const katana::geometry::Ellipse2& g) const
    {
        collector.offerCenter(g.center, g.distanceTo(request.cursor), id);
        if (!g.isFull()) {
            collector.offer(g.startPoint(), SnapMode::Endpoint, id);
            collector.offer(g.endPoint(), SnapMode::Endpoint, id);
            collector.offer(g.pointAtParameter(g.startParameter + 0.5 * g.sweep),
                            SnapMode::Midpoint, id);
        }
        for (const Point2& p : g.quadrants()) {
            collector.offer(p, SnapMode::Quadrant, id);
        }
    }
    // The ends, and the fit points a spline was drawn through (Node: they
    // are points the user placed, as a point entity is).
    void operator()(const katana::geometry::Spline2& g) const
    {
        if (!g.checkStructure()) {
            return;
        }
        collector.offer(g.startPoint(), SnapMode::Endpoint, id);
        collector.offer(g.endPoint(), SnapMode::Endpoint, id);
        for (const Point2& p : g.fitPoints) {
            collector.offer(p, SnapMode::Node, id);
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
    } else {
        // Every other linework kind through the one list of pieces
        // (entity/curve_pieces.hpp): a curve polyline's segments and arcs,
        // an ellipse's and a spline's chords - culled the same way.
        for (const Curve2& piece : katana::entity::curvePieces(entity.geometry)) {
            if (katana::geometry::boundingBox(piece).intersects(reach)) {
                curves.emplace_back(entity.id, piece);
            }
        }
    }
}

// ---- the snaps to lines that are not drawn --------------------------------------------
//
// Extension, Apparent Intersection and Parallel find points AWAY from the
// geometry that produces them - on a line carried on past its end, where two
// lines would cross, on the parallel through the last point - so they look
// at the curves of a wider neighbourhood than the aperture (kTrackingReach
// apertures), and accept only candidates within the aperture of the cursor
// like every other snap.
constexpr double kTrackingReach = 25.0;

void offerExtensions(Collector& collector, const SnapRequest& request,
                     const std::vector<std::pair<EntityId, Curve2>>& curves)
{
    for (const auto& [id, curve] : curves) {
        if (const auto* line = std::get_if<Segment2>(&curve)) {
            if (line->isDegenerate()) {
                continue;
            }
            const katana::geometry::Line2 carried{line->start, line->delta()};
            const double t = carried.parameterOf(request.cursor);
            if (t < 0.0 || t > 1.0) {
                collector.offer(carried.pointAt(t), SnapMode::Extension, id);
            }
        } else if (const auto* arc = std::get_if<Arc2>(&curve)) {
            const Vec2 toCursor = request.cursor - arc->center;
            if (toCursor.length() > 0.0 && !onArc(*arc, arc->center + toCursor)) {
                collector.offer(arc->center + toCursor.normalized() * arc->radius,
                                SnapMode::Extension, id);
            }
        }
    }
}

void offerApparentIntersections(Collector& collector,
                                const std::vector<std::pair<EntityId, Curve2>>& curves)
{
    for (std::size_t i = 0; i < curves.size(); ++i) {
        const auto* a = std::get_if<Segment2>(&curves[i].second);
        if (a == nullptr || a->isDegenerate()) {
            continue;
        }
        for (std::size_t j = i + 1; j < curves.size(); ++j) {
            const auto* b = std::get_if<Segment2>(&curves[j].second);
            if (b == nullptr || b->isDegenerate() || curves[i].first == curves[j].first) {
                continue;
            }
            const Vec2 da = a->delta();
            const Vec2 db = b->delta();
            const double denominator = da.cross(db);
            if (std::abs(denominator) <= katana::math::tolerance::kAngular * da.length() * db.length()) {
                continue;
            }
            const double t = (b->start - a->start).cross(db) / denominator;
            const double u = (b->start - a->start).cross(da) / denominator;
            // A real crossing is Intersection's; this is only the ones that
            // need at least one of the lines carried on.
            if (t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0) {
                continue;
            }
            collector.offer(a->pointAt(t), SnapMode::ApparentIntersection, curves[i].first);
        }
    }
}

void offerParallels(Collector& collector, const SnapRequest& request,
                    const std::vector<std::pair<EntityId, Curve2>>& curves)
{
    if (!request.from) {
        return;
    }
    for (const auto& [id, curve] : curves) {
        const auto* line = std::get_if<Segment2>(&curve);
        if (line == nullptr || line->isDegenerate()) {
            continue;
        }
        const katana::geometry::Line2 parallel{*request.from, line->delta()};
        const Point2 foot = parallel.closestPoint(request.cursor);
        if (foot.distanceTo(*request.from) > katana::math::tolerance::kGeometric) {
            collector.offer(foot, SnapMode::Parallel, id);
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

    const bool tracking = hasMode(request.modes, SnapMode::Extension) ||
                          hasMode(request.modes, SnapMode::ApparentIntersection) ||
                          (hasMode(request.modes, SnapMode::Parallel) && request.from);
    // Extension and Parallel are "somewhere along a line" snaps, like
    // Nearest: an exact point on drawn geometry beats them however close
    // they are, so they are collected apart and asked after Nearest.
    Collector along(request);
    if (tracking) {
        const Box2 wide = reach.inflated(request.aperture * kTrackingReach);
        std::vector<std::pair<EntityId, Curve2>> far;
        detail::forEachCandidate(model, index, wide, scratch, [&](const Entity& entity) {
            if (isDrawn(model, entity,
                        request.view != nullptr ? *request.view : kNoLayerOverrides)) {
                appendCurves(entity, wide, far);
            }
        });
        if (hasMode(request.modes, SnapMode::Extension)) {
            offerExtensions(along, request, far);
        }
        if (hasMode(request.modes, SnapMode::ApparentIntersection)) {
            offerApparentIntersections(collector, far);
        }
        if (hasMode(request.modes, SnapMode::Parallel)) {
            offerParallels(along, request, far);
        }
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
    if (along.best()) {
        return along.best();
    }

    if (hasMode(request.modes, SnapMode::Grid) && request.gridSpacing > 0.0) {
        const double spacing = request.gridSpacing;
        const Vec2 d = request.cursor - request.gridOrigin;
        return SnapResult{request.gridOrigin + Vec2(std::round(d.x / spacing) * spacing,
                                                    std::round(d.y / spacing) * spacing),
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
