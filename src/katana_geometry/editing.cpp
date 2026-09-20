#include "katana/geometry/editing.hpp"

#include <algorithm>
#include <cmath>
#include <string>

namespace katana::geometry {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::math::kTwoPi;
using katana::math::normalizeAngle;
using katana::math::normalizeAngleSigned;

// ---- Curve2 helpers ------------------------------------------------------------

IntersectionResult intersect(const Curve2& a, const Curve2& b)
{
    return std::visit([](const auto& lhs, const auto& rhs) { return intersect(lhs, rhs); }, a, b);
}

Point2 closestPoint(const Curve2& curve, const Point2& p)
{
    return std::visit([&](const auto& c) { return c.closestPoint(p); }, curve);
}

double distance(const Curve2& curve, const Point2& p)
{
    return std::visit([&](const auto& c) { return c.distanceTo(p); }, curve);
}

Box2 boundingBox(const Curve2& curve)
{
    return std::visit([](const auto& c) { return c.boundingBox(); }, curve);
}

// ---- offset ------------------------------------------------------------------------

Segment2 offset(const Segment2& segment, double distance)
{
    const Vec2 shift = segment.delta().normalized().perpendicular() * distance;
    return Segment2{segment.start + shift, segment.end + shift};
}

std::optional<Circle2> offset(const Circle2& circle, double distance)
{
    const double radius = circle.radius + distance;
    if (!(radius > tol::kGeometric)) {
        return std::nullopt;
    }
    return Circle2{circle.center, radius};
}

std::optional<Arc2> offset(const Arc2& arc, double distance)
{
    const double radius = arc.radius + distance;
    if (!(radius > tol::kGeometric)) {
        return std::nullopt;
    }
    return Arc2{arc.center, radius, arc.startAngle, arc.sweep};
}

Result<Polyline2> offset(const Polyline2& polyline, double distance)
{
    const Polyline2 clean = polyline.withoutDuplicateVertices();
    const std::size_t n = clean.vertices.size();
    if (n < 2 || (clean.closed && n < 3)) {
        return makeError(ErrorCode::InvalidGeometry,
                         "polyline has too few distinct vertices to offset",
                         "vertices=" + std::to_string(n));
    }

    const auto offsetLine = [&](std::size_t segmentIndex) {
        const Segment2 moved = offset(clean.segment(segmentIndex), distance);
        return Line2{moved.start, moved.delta()};
    };

    Polyline2 result;
    result.closed = clean.closed;
    result.vertices.reserve(n);
    const std::size_t segments = clean.segmentCount();
    for (std::size_t i = 0; i < n; ++i) {
        const bool hasIncoming = clean.closed || i > 0;
        const bool hasOutgoing = clean.closed || i + 1 < n;
        if (hasIncoming && hasOutgoing) {
            const Line2 incoming = offsetLine((i + segments - 1) % segments);
            const Line2 outgoing = offsetLine(i % segments);
            const IntersectionResult corner = intersect(incoming, outgoing);
            if (corner.kind == IntersectionKind::Points) {
                result.vertices.push_back(corner.points[0]); // mitre
                continue;
            }
            if (corner.kind == IntersectionKind::None) {
                // Parallel offsets that do not coincide: the path doubles back on itself.
                return makeError(ErrorCode::InvalidGeometry,
                                 "polyline reverses direction at a vertex; offset is undefined",
                                 "vertex=" + std::to_string(i));
            }
            result.vertices.push_back(outgoing.origin); // straight through
        } else if (hasOutgoing) {
            result.vertices.push_back(offsetLine(i).origin);
        } else {
            result.vertices.push_back(offset(clean.segment(i - 1), distance).end);
        }
    }
    return result;
}

Result<Curve2> offsetTowards(const Curve2& curve, double distance, const Point2& side)
{
    const double magnitude = std::abs(distance);
    if (!(magnitude > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidArgument, "offset distance must be non-zero");
    }
    if (const auto* segment = std::get_if<Segment2>(&curve)) {
        if (segment->isDegenerate()) {
            return makeError(ErrorCode::InvalidGeometry, "cannot offset a zero-length segment");
        }
        const double sideSign =
            Line2{segment->start, segment->delta()}.signedDistanceTo(side) >= 0.0 ? 1.0 : -1.0;
        return Curve2{offset(*segment, sideSign * magnitude)};
    }
    if (const auto* circle = std::get_if<Circle2>(&curve)) {
        const bool outward = circle->center.distanceTo(side) >= circle->radius;
        const auto moved = offset(*circle, outward ? magnitude : -magnitude);
        if (!moved) {
            return makeError(ErrorCode::InvalidGeometry,
                             "offset distance is not smaller than the circle radius");
        }
        return Curve2{*moved};
    }
    // Explicitly, not by elimination. std::get<Arc2> here assumed Arc2 was the
    // only alternative left; a fourth Curve2 alternative would have made it
    // throw std::bad_variant_access out of a function that returns Result and
    // promises not to throw - a compiling change that becomes a crash across an
    // interface boundary.
    const auto* arcPtr = std::get_if<Arc2>(&curve);
    if (arcPtr == nullptr) {
        return makeError(ErrorCode::Unsupported, "this curve kind cannot be offset");
    }
    const Arc2& arc = *arcPtr;
    const bool outward = arc.center.distanceTo(side) >= arc.radius;
    const auto moved = offset(arc, outward ? magnitude : -magnitude);
    if (!moved) {
        return makeError(ErrorCode::InvalidGeometry,
                         "offset distance is not smaller than the arc radius");
    }
    return Curve2{*moved};
}

// ---- trim ----------------------------------------------------------------------------

namespace {

// Isolated intersection points of `target` with every curve in `others`.
std::vector<Point2> crossingPoints(const Curve2& target, std::span<const Curve2> others)
{
    std::vector<Point2> points;
    for (const Curve2& other : others) {
        const IntersectionResult hit = intersect(target, other);
        if (hit.kind == IntersectionKind::Points) {
            points.insert(points.end(), hit.points.begin(), hit.points.begin() + hit.count);
        }
    }
    return points;
}

void sortUnique(std::vector<double>& values, double tolerance)
{
    std::sort(values.begin(), values.end());
    values.erase(std::unique(values.begin(), values.end(),
                             [&](double a, double b) { return b - a <= tolerance; }),
                 values.end());
}

// Shared trimming logic for open curves parameterised over [0, 1].
template <typename Piece>
Result<TrimResult> trimOpen(std::vector<double> cuts, double pickParameter,
                            double parameterTolerance, Piece piece)
{
    std::erase_if(cuts, [&](double t) {
        return t <= parameterTolerance || t >= 1.0 - parameterTolerance;
    });
    sortUnique(cuts, parameterTolerance);
    if (cuts.empty()) {
        return makeError(ErrorCode::InvalidGeometry,
                         "no cutting edge crosses the interior of the target");
    }
    double lo = 0.0;
    double hi = 1.0;
    for (const double cut : cuts) {
        if (cut <= pickParameter) {
            lo = cut;
        } else {
            hi = cut;
            break;
        }
    }
    TrimResult result;
    if (lo > 0.0) {
        result.remaining.push_back(piece(0.0, lo));
    }
    if (hi < 1.0) {
        result.remaining.push_back(piece(hi, 1.0));
    }
    return result;
}

} // namespace

Result<TrimResult> trim(const Curve2& target, std::span<const Curve2> cutters, const Point2& pick)
{
    const std::vector<Point2> crossings = crossingPoints(target, cutters);

    if (const auto* segment = std::get_if<Segment2>(&target)) {
        if (segment->isDegenerate()) {
            return makeError(ErrorCode::InvalidGeometry, "cannot trim a zero-length segment");
        }
        std::vector<double> cuts;
        for (const Point2& p : crossings) {
            cuts.push_back(segment->parameterOf(p));
        }
        return trimOpen(std::move(cuts), segment->parameterOf(pick),
                        tol::kGeometric / segment->length(), [&](double from, double to) {
                            return Curve2{Segment2{segment->pointAt(from), segment->pointAt(to)}};
                        });
    }

    if (const auto* arc = std::get_if<Arc2>(&target)) {
        if (!(arc->length() > tol::kGeometric)) {
            return makeError(ErrorCode::InvalidGeometry, "cannot trim a zero-length arc");
        }
        std::vector<double> cuts;
        for (const Point2& p : crossings) {
            cuts.push_back(arc->parameterOfAngle((p - arc->center).angle()));
        }
        const double pickParameter = arc->parameterOfAngle((pick - arc->center).angle());
        return trimOpen(std::move(cuts), pickParameter, tol::kGeometric / arc->length(),
                        [&](double from, double to) {
                            return Curve2{Arc2{arc->center, arc->radius,
                                               arc->startAngle + arc->sweep * from,
                                               arc->sweep * (to - from)}};
                        });
    }

    // Explicitly, for the same reason as offset() above.
    const auto* circlePtr = std::get_if<Circle2>(&target);
    if (circlePtr == nullptr) {
        return makeError(ErrorCode::Unsupported, "this curve kind cannot be trimmed");
    }
    const Circle2& circle = *circlePtr;
    if (!(circle.radius > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidGeometry, "cannot trim a zero-radius circle");
    }
    std::vector<double> angles;
    for (const Point2& p : crossings) {
        angles.push_back(normalizeAngle((p - circle.center).angle()));
    }
    const double angularTolerance = tol::kGeometric / circle.radius;
    sortUnique(angles, angularTolerance);
    if (angles.size() > 1 && angles.front() + kTwoPi - angles.back() <= angularTolerance) {
        angles.pop_back(); // the same crossing seen on both sides of the 0 / 2*pi seam
    }
    if (angles.size() < 2) {
        return makeError(ErrorCode::InvalidGeometry,
                         "a circle needs at least two crossings to be trimmed");
    }
    // Remove the gap [lo, hi) that contains the pick direction; keep hi -> lo.
    const double pickAngle = normalizeAngle((pick - circle.center).angle());
    std::size_t hiIndex = 0;
    while (hiIndex < angles.size() && angles[hiIndex] <= pickAngle) {
        ++hiIndex;
    }
    const double hi = angles[hiIndex % angles.size()];
    const double lo = angles[(hiIndex + angles.size() - 1) % angles.size()];
    TrimResult result;
    result.remaining.push_back(Arc2{circle.center, circle.radius, hi, normalizeAngle(lo - hi)});
    return result;
}

// ---- extend --------------------------------------------------------------------------

namespace {

Result<Segment2> extendSegment(const Segment2& segment, std::span<const Curve2> boundaries,
                               const Point2& pick)
{
    if (segment.isDegenerate()) {
        return makeError(ErrorCode::InvalidGeometry, "cannot extend a zero-length segment");
    }
    const bool extendEnd = pick.distanceTo(segment.end) <= pick.distanceTo(segment.start);
    const Point2 moving = extendEnd ? segment.end : segment.start;
    const Point2 fixed = extendEnd ? segment.start : segment.end;
    const Line2 ray{moving, (moving - fixed).normalized()}; // unit direction: t is a distance

    std::optional<double> nearest;
    for (const Curve2& boundary : boundaries) {
        const IntersectionResult hit =
            std::visit([&](const auto& b) { return intersect(ray, b); }, boundary);
        if (hit.kind != IntersectionKind::Points) {
            continue;
        }
        for (std::size_t i = 0; i < hit.count; ++i) {
            const double t = ray.parameterOf(hit.points[i]);
            if (t > tol::kGeometric && (!nearest || t < *nearest)) {
                nearest = t;
            }
        }
    }
    if (!nearest) {
        return makeError(ErrorCode::InvalidGeometry, "no boundary lies ahead of the segment end");
    }
    const Point2 reached = ray.pointAt(*nearest);
    return extendEnd ? Segment2{segment.start, reached} : Segment2{reached, segment.end};
}

// Extends the end of `arc` (in its sweep direction).
Result<Arc2> extendArcEnd(const Arc2& arc, std::span<const Curve2> boundaries)
{
    if (!(arc.radius > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidGeometry, "cannot extend a zero-radius arc");
    }
    const double angularTolerance = tol::kGeometric / arc.radius;
    const double available = kTwoPi - std::abs(arc.sweep);
    const Curve2 fullCircle = arc.circle();

    std::optional<double> nearest;
    for (const Point2& p : crossingPoints(fullCircle, boundaries)) {
        const double angle = (p - arc.center).angle();
        const double extra = arc.sweep >= 0.0 ? normalizeAngle(angle - arc.endAngle())
                                              : normalizeAngle(arc.endAngle() - angle);
        if (extra > angularTolerance && extra <= available && (!nearest || extra < *nearest)) {
            nearest = extra;
        }
    }
    if (!nearest) {
        return makeError(ErrorCode::InvalidGeometry, "no boundary lies ahead of the arc end");
    }
    const double direction = arc.sweep >= 0.0 ? 1.0 : -1.0;
    return Arc2{arc.center, arc.radius, arc.startAngle, arc.sweep + direction * *nearest};
}

} // namespace

Result<Curve2> extend(const Curve2& target, std::span<const Curve2> boundaries, const Point2& pick)
{
    if (const auto* segment = std::get_if<Segment2>(&target)) {
        auto extended = extendSegment(*segment, boundaries, pick);
        if (!extended) {
            return extended.error();
        }
        return Curve2{*extended};
    }
    if (const auto* arc = std::get_if<Arc2>(&target)) {
        const bool extendEnd = pick.distanceTo(arc->endPoint()) <= pick.distanceTo(arc->startPoint());
        auto extended = extendArcEnd(extendEnd ? *arc : arc->reversed(), boundaries);
        if (!extended) {
            return extended.error();
        }
        return Curve2{extendEnd ? *extended : extended->reversed()};
    }
    return makeError(ErrorCode::InvalidGeometry, "a circle has no end to extend");
}

// ---- fillet / chamfer ----------------------------------------------------------------

namespace {

struct Corner {
    Point2 apex;      // intersection of the supporting lines
    Point2 farFirst;  // kept end of the first segment
    Point2 farSecond; // kept end of the second segment
    Vec2 alongFirst;  // unit vector apex -> farFirst
    Vec2 alongSecond; // unit vector apex -> farSecond
    double reachFirst = 0.0;
    double reachSecond = 0.0;
    bool firstEndsAtApex = false; // the first segment's `end` is the corner side
    bool secondEndsAtApex = false;
};

Result<Corner> findCorner(const Segment2& first, const Segment2& second)
{
    if (first.isDegenerate() || second.isDegenerate()) {
        return makeError(ErrorCode::InvalidGeometry, "segments must have non-zero length");
    }
    const IntersectionResult apex = intersect(Line2{first.start, first.delta()},
                                              Line2{second.start, second.delta()});
    if (apex.kind != IntersectionKind::Points) {
        return makeError(ErrorCode::InvalidGeometry,
                         "segments are parallel; they have no corner to modify");
    }
    Corner corner;
    corner.apex = apex.points[0];
    corner.firstEndsAtApex =
        corner.apex.distanceTo(first.end) <= corner.apex.distanceTo(first.start);
    corner.secondEndsAtApex =
        corner.apex.distanceTo(second.end) <= corner.apex.distanceTo(second.start);
    corner.farFirst = corner.firstEndsAtApex ? first.start : first.end;
    corner.farSecond = corner.secondEndsAtApex ? second.start : second.end;
    corner.reachFirst = corner.apex.distanceTo(corner.farFirst);
    corner.reachSecond = corner.apex.distanceTo(corner.farSecond);
    if (corner.reachFirst <= tol::kGeometric || corner.reachSecond <= tol::kGeometric) {
        return makeError(ErrorCode::InvalidGeometry, "a segment ends exactly at the corner");
    }
    corner.alongFirst = (corner.farFirst - corner.apex) / corner.reachFirst;
    corner.alongSecond = (corner.farSecond - corner.apex) / corner.reachSecond;
    return corner;
}

Segment2 rebuilt(const Point2& farEnd, const Point2& cornerEnd, bool endsAtApex)
{
    return endsAtApex ? Segment2{farEnd, cornerEnd} : Segment2{cornerEnd, farEnd};
}

} // namespace

Result<FilletResult> fillet(const Segment2& first, const Segment2& second, double radius)
{
    if (!(radius >= 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "fillet radius must not be negative",
                         "radius=" + std::to_string(radius));
    }
    const auto found = findCorner(first, second);
    if (!found) {
        return found.error();
    }
    const Corner& corner = *found;

    FilletResult result;
    if (radius <= tol::kGeometric) {
        result.first = rebuilt(corner.farFirst, corner.apex, corner.firstEndsAtApex);
        result.second = rebuilt(corner.farSecond, corner.apex, corner.secondEndsAtApex);
        return result;
    }

    const double cosAngle = std::clamp(corner.alongFirst.dot(corner.alongSecond), -1.0, 1.0);
    const double halfAngle = 0.5 * std::acos(cosAngle);
    const double tangentDistance = radius / std::tan(halfAngle);
    // What must remain is a segment with real length, so the test is on the
    // REMAINDER, not on whether the cut overshoots by more than a tolerance.
    // `tangentDistance == reach` leaves a zero-length segment, which the model
    // then rejects with "line has zero length" - an error about the line, when
    // what is wrong is the radius the user typed. Filleting two 10 m lines with
    // r/tan(theta/2) == 10 is an entirely ordinary thing to try.
    //
    // The negated form also rejects the infinite tangentDistance produced when
    // cosAngle is exactly 1 (collinear segments), which a plain `>` comparison
    // against a finite reach would let through.
    if (!(corner.reachFirst - tangentDistance > tol::kGeometric) ||
        !(corner.reachSecond - tangentDistance > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidGeometry, "fillet radius is too large for the segments",
                         "radius=" + std::to_string(radius));
    }

    const Point2 tangentFirst = corner.apex + corner.alongFirst * tangentDistance;
    const Point2 tangentSecond = corner.apex + corner.alongSecond * tangentDistance;
    const Vec2 bisector = (corner.alongFirst + corner.alongSecond).normalized();
    const Point2 center = corner.apex + bisector * (radius / std::sin(halfAngle));

    const double startAngle = (tangentFirst - center).angle();
    const double sweep = normalizeAngleSigned((tangentSecond - center).angle() - startAngle);

    result.first = rebuilt(corner.farFirst, tangentFirst, corner.firstEndsAtApex);
    result.second = rebuilt(corner.farSecond, tangentSecond, corner.secondEndsAtApex);
    result.arc = Arc2{center, radius, startAngle, sweep};
    return result;
}

Result<ChamferResult> chamfer(const Segment2& first, const Segment2& second, double distanceFirst,
                              double distanceSecond)
{
    if (!(distanceFirst >= 0.0) || !(distanceSecond >= 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "chamfer distances must not be negative");
    }
    const auto found = findCorner(first, second);
    if (!found) {
        return found.error();
    }
    const Corner& corner = *found;
    // As in fillet: the remaining piece has to have length. A distance equal to
    // the segment's reach consumes it entirely and leaves nothing to rebuild.
    if (!(corner.reachFirst - distanceFirst > tol::kGeometric) ||
        !(corner.reachSecond - distanceSecond > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidGeometry,
                         "chamfer distance leaves no segment behind");
    }

    const Point2 cutFirst = corner.apex + corner.alongFirst * distanceFirst;
    const Point2 cutSecond = corner.apex + corner.alongSecond * distanceSecond;

    ChamferResult result;
    result.first = rebuilt(corner.farFirst, cutFirst, corner.firstEndsAtApex);
    result.second = rebuilt(corner.farSecond, cutSecond, corner.secondEndsAtApex);
    if (cutFirst.distanceTo(cutSecond) > tol::kGeometric) {
        result.bevel = Segment2{cutFirst, cutSecond};
    }
    return result;
}

} // namespace katana::geometry
