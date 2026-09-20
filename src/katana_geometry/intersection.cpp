#include "katana/geometry/intersection.hpp"

#include <algorithm>
#include <cmath>

namespace katana::geometry {

namespace tol = katana::math::tolerance;

namespace {

IntersectionResult none()
{
    return {};
}

IntersectionResult onePoint(const Point2& p, bool tangent = false)
{
    IntersectionResult result;
    result.kind = IntersectionKind::Points;
    result.points[0] = p;
    result.count = 1;
    result.tangent = tangent;
    return result;
}

IntersectionResult twoPoints(const Point2& p, const Point2& q)
{
    IntersectionResult result;
    result.kind = IntersectionKind::Points;
    result.points = {p, q};
    result.count = 2;
    return result;
}

IntersectionResult overlap()
{
    IntersectionResult result;
    result.kind = IntersectionKind::Overlap;
    return result;
}

IntersectionResult overlap(const Point2& from, const Point2& to)
{
    IntersectionResult result;
    result.kind = IntersectionKind::Overlap;
    result.points = {from, to};
    result.count = 2;
    return result;
}

// sin(angle between u and v) <= kAngular. Zero vectors count as parallel.
bool parallel(const Vec2& u, const Vec2& v)
{
    return !(std::abs(u.cross(v)) > tol::kAngular * u.length() * v.length());
}

// Keeps the points of `candidates` for which `keep` holds, preserving order.
template <typename Predicate>
IntersectionResult filterPoints(const IntersectionResult& candidates, Predicate keep)
{
    if (candidates.kind != IntersectionKind::Points) {
        return candidates;
    }
    IntersectionResult result;
    result.tangent = candidates.tangent;
    for (std::size_t i = 0; i < candidates.count; ++i) {
        if (keep(candidates.points[i])) {
            result.points[result.count++] = candidates.points[i];
        }
    }
    result.kind = result.count > 0 ? IntersectionKind::Points : IntersectionKind::None;
    return result;
}

bool onSegment(const Segment2& segment, const Point2& p)
{
    return segment.distanceTo(p) <= tol::kGeometric;
}

bool onArc(const Arc2& arc, const Point2& p)
{
    const double angularTolerance = arc.radius > 0.0 ? tol::kGeometric / arc.radius : 0.0;
    return arc.containsAngle((p - arc.center).angle(), angularTolerance);
}

// A segment shorter than kGeometric behaves as the point at its start.
IntersectionResult pointAgainstSegment(const Point2& p, const Segment2& segment)
{
    return onSegment(segment, p) ? onePoint(p) : none();
}

} // namespace

// ---- line / segment ------------------------------------------------------------

IntersectionResult intersect(const Line2& a, const Line2& b)
{
    if (parallel(a.direction, b.direction)) {
        return a.distanceTo(b.origin) <= tol::kGeometric ? overlap() : none();
    }
    const double t = (b.origin - a.origin).cross(b.direction) / a.direction.cross(b.direction);
    return onePoint(a.pointAt(t));
}

IntersectionResult intersect(const Line2& line, const Segment2& segment)
{
    if (segment.isDegenerate()) {
        return line.distanceTo(segment.start) <= tol::kGeometric ? onePoint(segment.start)
                                                                 : none();
    }
    const IntersectionResult infinite = intersect(line, Line2{segment.start, segment.delta()});
    if (infinite.kind == IntersectionKind::Overlap) {
        return overlap(segment.start, segment.end);
    }
    return filterPoints(infinite, [&](const Point2& p) { return onSegment(segment, p); });
}

IntersectionResult intersect(const Segment2& a, const Segment2& b)
{
    if (a.isDegenerate()) {
        return pointAgainstSegment(a.start, b);
    }
    if (b.isDegenerate()) {
        return pointAgainstSegment(b.start, a);
    }

    const Vec2 da = a.delta();
    const Vec2 db = b.delta();
    const Vec2 offset = b.start - a.start;

    if (parallel(da, db)) {
        const Line2 carrier{a.start, da};
        if (carrier.distanceTo(b.start) > tol::kGeometric ||
            carrier.distanceTo(b.end) > tol::kGeometric) {
            return none();
        }
        // Collinear: intersect the parameter intervals along `a`.
        const double lengthA = da.length();
        const double t0 = carrier.parameterOf(b.start);
        const double t1 = carrier.parameterOf(b.end);
        const double lo = std::max(0.0, std::min(t0, t1));
        const double hi = std::min(1.0, std::max(t0, t1));
        const double paramTolerance = tol::kGeometric / lengthA;
        if (lo > hi + paramTolerance) {
            return none();
        }
        if (hi - lo <= paramTolerance) {
            return onePoint(a.pointAt(std::clamp(0.5 * (lo + hi), 0.0, 1.0)));
        }
        return overlap(a.pointAt(lo), a.pointAt(hi));
    }

    const double denominator = da.cross(db);
    const double t = offset.cross(db) / denominator;
    const Point2 p = a.start + da * t;
    // Accept the crossing when it lies within kGeometric of both segments; this
    // keeps endpoint touches that rounding pushed marginally outside [0, 1].
    if (!onSegment(a, p) || !onSegment(b, p)) {
        return none();
    }
    return onePoint(a.pointAt(std::clamp(t, 0.0, 1.0)));
}

// ---- circle ----------------------------------------------------------------------

IntersectionResult intersect(const Line2& line, const Circle2& circle)
{
    const Vec2 unit = line.direction.normalized();
    if (unit == Vec2{}) {
        return circle.classify(line.origin) == Containment::OnBoundary ? onePoint(line.origin)
                                                                       : none();
    }
    const Point2 foot = line.closestPoint(circle.center);
    const double distance = foot.distanceTo(circle.center);

    if (distance > circle.radius + tol::kGeometric) {
        return none();
    }
    if (std::abs(distance - circle.radius) <= tol::kGeometric) {
        return onePoint(foot, /*tangent=*/true);
    }
    // (r - d)(r + d) keeps accuracy when d is close to r.
    const double halfChord =
        std::sqrt((circle.radius - distance) * (circle.radius + distance));
    return twoPoints(foot - unit * halfChord, foot + unit * halfChord);
}

IntersectionResult intersect(const Segment2& segment, const Circle2& circle)
{
    if (segment.isDegenerate()) {
        return circle.classify(segment.start) == Containment::OnBoundary
                   ? onePoint(segment.start)
                   : none();
    }
    const IntersectionResult infinite = intersect(Line2{segment.start, segment.delta()}, circle);
    return filterPoints(infinite, [&](const Point2& p) { return onSegment(segment, p); });
}

IntersectionResult intersect(const Circle2& a, const Circle2& b)
{
    const Vec2 between = b.center - a.center;
    const double distance = between.length();
    const double radiusDifference = std::abs(a.radius - b.radius);

    if (distance <= tol::kGeometric) {
        return radiusDifference <= tol::kGeometric ? overlap() : none(); // coincident / concentric
    }
    if (distance > a.radius + b.radius + tol::kGeometric ||
        distance < radiusDifference - tol::kGeometric) {
        return none(); // separate / one inside the other
    }

    const Vec2 unit = between / distance;
    if (std::abs(distance - (a.radius + b.radius)) <= tol::kGeometric) {
        return onePoint(a.center + unit * a.radius, /*tangent=*/true); // external tangency
    }
    if (std::abs(distance - radiusDifference) <= tol::kGeometric) {
        // Internal tangency: the contact lies beyond the smaller circle's centre.
        const double direction = a.radius >= b.radius ? 1.0 : -1.0;
        return onePoint(a.center + unit * (direction * a.radius), /*tangent=*/true);
    }

    // Distance from a.center to the radical line, measured along `unit`.
    const double along =
        (distance * distance + a.radius * a.radius - b.radius * b.radius) / (2.0 * distance);
    const double halfChord = std::sqrt(std::max(0.0, (a.radius - along) * (a.radius + along)));
    const Point2 foot = a.center + unit * along;
    const Vec2 normal = unit.perpendicular();
    return twoPoints(foot - normal * halfChord, foot + normal * halfChord);
}

// ---- arc -------------------------------------------------------------------------

IntersectionResult intersect(const Line2& line, const Arc2& arc)
{
    return filterPoints(intersect(line, arc.circle()),
                        [&](const Point2& p) { return onArc(arc, p); });
}

IntersectionResult intersect(const Segment2& segment, const Arc2& arc)
{
    return filterPoints(intersect(segment, arc.circle()),
                        [&](const Point2& p) { return onArc(arc, p); });
}

IntersectionResult intersect(const Circle2& circle, const Arc2& arc)
{
    const IntersectionResult full = intersect(circle, arc.circle());
    if (full.kind == IntersectionKind::Overlap) {
        return overlap(arc.startPoint(), arc.endPoint());
    }
    return filterPoints(full, [&](const Point2& p) { return onArc(arc, p); });
}

IntersectionResult intersect(const Arc2& a, const Arc2& b)
{
    const IntersectionResult full = intersect(a.circle(), b.circle());
    if (full.kind != IntersectionKind::Overlap) {
        return filterPoints(full, [&](const Point2& p) { return onArc(a, p) && onArc(b, p); });
    }

    // Same supporting circle. The arcs share a portion when an end of one lies
    // strictly inside the other; otherwise they can only touch at their ends.
    const auto strictlyInside = [](const Arc2& arc, const Point2& p) {
        return onArc(arc, p) && p.distanceTo(arc.startPoint()) > tol::kGeometric &&
               p.distanceTo(arc.endPoint()) > tol::kGeometric;
    };
    const bool sameEnds =
        (a.startPoint().distanceTo(b.startPoint()) <= tol::kGeometric &&
         a.endPoint().distanceTo(b.endPoint()) <= tol::kGeometric) ||
        (a.startPoint().distanceTo(b.endPoint()) <= tol::kGeometric &&
         a.endPoint().distanceTo(b.startPoint()) <= tol::kGeometric);
    const bool sameMiddle = onArc(b, a.midpoint());
    if (strictlyInside(a, b.startPoint()) || strictlyInside(a, b.endPoint()) ||
        strictlyInside(b, a.startPoint()) || strictlyInside(b, a.endPoint()) ||
        (sameEnds && sameMiddle)) {
        return overlap();
    }

    IntersectionResult touching;
    for (const Point2& end : {b.startPoint(), b.endPoint()}) {
        const bool alreadyListed =
            touching.count == 1 && touching.points[0].distanceTo(end) <= tol::kGeometric;
        if (onArc(a, end) && !alreadyListed && touching.count < 2) {
            touching.points[touching.count++] = end;
        }
    }
    touching.kind = touching.count > 0 ? IntersectionKind::Points : IntersectionKind::None;
    return touching;
}

} // namespace katana::geometry
