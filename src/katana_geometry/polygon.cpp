#include "katana/geometry/polygon.hpp"

#include <algorithm>
#include <cmath>
#include <numeric>
#include <string>

#include "katana/geometry/intersection.hpp"

namespace katana::geometry {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

Orientation orientation(const Polyline2& polygon)
{
    if (!polygon.closed || polygon.vertices.size() < 3) {
        return Orientation::Degenerate;
    }
    const double signedArea = polygon.signedArea();
    // Negligible when thinner than kGeometric across its own perimeter.
    if (std::abs(signedArea) <= tol::kGeometric * polygon.length()) {
        return Orientation::Degenerate;
    }
    return signedArea > 0.0 ? Orientation::CounterClockwise : Orientation::Clockwise;
}

bool isConvex(const Polyline2& polygon)
{
    const std::size_t n = polygon.vertices.size();
    if (!polygon.closed || n < 3) {
        return false;
    }
    int sign = 0;
    for (std::size_t i = 0; i < n; ++i) {
        const Vec2 incoming = polygon.vertices[(i + 1) % n] - polygon.vertices[i];
        const Vec2 outgoing = polygon.vertices[(i + 2) % n] - polygon.vertices[(i + 1) % n];
        const double turn = incoming.cross(outgoing);
        if (std::abs(turn) <= tol::kAngular * incoming.length() * outgoing.length()) {
            continue; // straight through
        }
        const int current = turn > 0.0 ? 1 : -1;
        if (sign == 0) {
            sign = current;
        } else if (sign != current) {
            return false;
        }
    }
    return sign != 0;
}

bool isSimple(const Polyline2& polygon)
{
    const std::size_t n = polygon.vertices.size();
    if (!polygon.closed || n < 3) {
        return false;
    }
    for (std::size_t i = 0; i < n; ++i) {
        const Segment2 first = polygon.segment(i);
        if (first.isDegenerate()) {
            return false;
        }
        // intersect() accepts a hit only when the crossing point is within
        // kGeometric of BOTH segments, and the collinear branch only when one
        // lies within kGeometric of the other's carrier and their parameter
        // ranges meet to within kGeometric along it. Either way a pair it
        // reports is at most 2 * kGeometric apart, so boxes further apart than
        // that cannot intersect - and rejecting them costs two comparisons
        // instead of the eight-or-so hypots intersect() spends saying no. The
        // margin is 4 * kGeometric to leave the bound room it does not need.
        const Box2 firstBox = first.boundingBox().inflated(4.0 * tol::kGeometric);
        for (std::size_t j = i + 1; j < n; ++j) {
            const Segment2 second = polygon.segment(j);
            if (!firstBox.intersects(second.boundingBox())) {
                continue;
            }
            const IntersectionResult hit = intersect(first, second);
            if (!hit.exists()) {
                continue;
            }
            const bool adjacent = (j == i + 1) || (i == 0 && j == n - 1);
            if (!adjacent || hit.kind == IntersectionKind::Overlap) {
                return false;
            }
        }
    }
    return true;
}

std::vector<Point2> convexHull(std::vector<Point2> points)
{
    std::sort(points.begin(), points.end(), [](const Point2& a, const Point2& b) {
        return a.x < b.x || (a.x == b.x && a.y < b.y);
    });
    points.erase(std::unique(points.begin(), points.end()), points.end());
    if (points.size() < 3) {
        return points;
    }

    std::vector<Point2> hull(2 * points.size());
    std::size_t size = 0;
    const auto turnsLeft = [&](const Point2& p) {
        return (hull[size - 1] - hull[size - 2]).cross(p - hull[size - 2]) > 0.0;
    };
    for (const Point2& p : points) { // lower hull
        while (size >= 2 && !turnsLeft(p)) {
            --size;
        }
        hull[size++] = p;
    }
    const std::size_t lowerSize = size + 1;
    for (std::size_t i = points.size() - 1; i-- > 0;) { // upper hull
        while (size >= lowerSize && !turnsLeft(points[i])) {
            --size;
        }
        hull[size++] = points[i];
    }
    hull.resize(size - 1); // the last point repeats the first
    return hull;
}

namespace {

void douglasPeucker(const std::vector<Point2>& points, std::size_t first, std::size_t last,
                    double tolerance, std::vector<bool>& keep)
{
    if (last <= first + 1) {
        return;
    }
    const Segment2 chord{points[first], points[last]};
    double worst = -1.0;
    std::size_t worstIndex = first;
    for (std::size_t i = first + 1; i < last; ++i) {
        const double distance = chord.distanceTo(points[i]);
        if (distance > worst) {
            worst = distance;
            worstIndex = i;
        }
    }
    if (worst > tolerance) {
        keep[worstIndex] = true;
        douglasPeucker(points, first, worstIndex, tolerance, keep);
        douglasPeucker(points, worstIndex, last, tolerance, keep);
    }
}

} // namespace

Polyline2 simplify(const Polyline2& polyline, double tolerance)
{
    if (!(tolerance > 0.0) || polyline.vertices.size() < 3) {
        return polyline;
    }
    std::vector<Point2> points = polyline.vertices;
    if (polyline.closed) {
        points.push_back(points.front()); // open the ring at vertex 0
    }
    std::vector<bool> keep(points.size(), false);
    keep.front() = true;
    keep.back() = true;
    douglasPeucker(points, 0, points.size() - 1, tolerance, keep);

    Polyline2 result;
    result.closed = polyline.closed;
    const std::size_t count = polyline.closed ? points.size() - 1 : points.size();
    for (std::size_t i = 0; i < count; ++i) {
        if (keep[i]) {
            result.vertices.push_back(points[i]);
        }
    }
    return result;
}

std::optional<Segment2> clip(const Segment2& segment, const Box2& box)
{
    if (box.empty()) {
        return std::nullopt;
    }
    const Vec2 d = segment.delta();
    const double p[4] = {-d.x, d.x, -d.y, d.y};
    const double q[4] = {segment.start.x - box.min.x, box.max.x - segment.start.x,
                         segment.start.y - box.min.y, box.max.y - segment.start.y};
    double enter = 0.0;
    double exit = 1.0;
    for (int i = 0; i < 4; ++i) {
        if (p[i] == 0.0) {
            if (q[i] < 0.0) {
                return std::nullopt; // parallel to this edge and outside it
            }
            continue;
        }
        const double r = q[i] / p[i];
        if (p[i] < 0.0) {
            enter = std::max(enter, r);
        } else {
            exit = std::min(exit, r);
        }
        if (enter > exit) {
            return std::nullopt;
        }
    }
    return Segment2{segment.pointAt(enter), segment.pointAt(exit)};
}

Result<Polyline2> clipPolygon(const Polyline2& subject, const Polyline2& clipRegion)
{
    if (!subject.closed) {
        return makeError(ErrorCode::InvalidGeometry, "subject polygon must be closed");
    }
    if (!isConvex(clipRegion)) {
        return makeError(ErrorCode::InvalidGeometry, "clip region must be a convex polygon");
    }
    Polyline2 clipCcw = clipRegion;
    if (orientation(clipCcw) == Orientation::Clockwise) {
        clipCcw = clipCcw.reversed();
    }

    std::vector<Point2> output = subject.vertices;
    // Two buffers swapped per edge rather than one moved from: a move leaves
    // `output` with no capacity, so every clip edge grew its result from
    // nothing again. Swapping hands back the buffer the previous edge read
    // from, and after the first edge neither allocates.
    std::vector<Point2> input;
    const std::size_t clipCount = clipCcw.vertices.size();
    for (std::size_t edge = 0; edge < clipCount && !output.empty(); ++edge) {
        const Point2& edgeStart = clipCcw.vertices[edge];
        const Vec2 edgeDirection = clipCcw.vertices[(edge + 1) % clipCount] - edgeStart;
        const auto side = [&](const Point2& p) { return edgeDirection.cross(p - edgeStart); };

        input.swap(output);
        output.clear();
        for (std::size_t i = 0; i < input.size(); ++i) {
            const Point2& current = input[i];
            const Point2& previous = input[(i + input.size() - 1) % input.size()];
            const double currentSide = side(current);
            const double previousSide = side(previous);
            const bool currentInside = currentSide >= 0.0;
            const bool previousInside = previousSide >= 0.0;
            if (currentInside != previousInside) {
                const double t = previousSide / (previousSide - currentSide);
                output.push_back(previous + (current - previous) * t);
            }
            if (currentInside) {
                output.push_back(current);
            }
        }
    }
    return Polyline2{std::move(output), true}.withoutDuplicateVertices();
}

Result<std::vector<TriangleIndices>> triangulate(const Polyline2& polygon)
{
    const std::size_t n = polygon.vertices.size();
    const Orientation winding = orientation(polygon);
    if (winding == Orientation::Degenerate) {
        return makeError(ErrorCode::TriangulationFailure,
                         "polygon is open, has fewer than 3 vertices or has no area",
                         "vertices=" + std::to_string(n));
    }

    std::vector<std::size_t> ring(n);
    std::iota(ring.begin(), ring.end(), std::size_t{0});
    if (winding == Orientation::Clockwise) {
        std::reverse(ring.begin(), ring.end());
    }
    const auto& v = polygon.vertices;

    std::vector<TriangleIndices> triangles;
    triangles.reserve(n - 2);

    while (ring.size() > 3) {
        bool clipped = false;
        for (std::size_t i = 0; i < ring.size(); ++i) {
            const std::size_t prev = ring[(i + ring.size() - 1) % ring.size()];
            const std::size_t cur = ring[i];
            const std::size_t next = ring[(i + 1) % ring.size()];

            const Vec2 incoming = v[cur] - v[prev];
            const Vec2 outgoing = v[next] - v[cur];
            const double turn = incoming.cross(outgoing);
            const double turnScale = tol::kAngular * incoming.length() * outgoing.length();
            if (std::abs(turn) <= turnScale) {
                ring.erase(ring.begin() + static_cast<std::ptrdiff_t>(i)); // collinear: drop
                clipped = true;
                break;
            }
            if (turn < 0.0) {
                continue; // reflex vertex cannot be an ear
            }

            bool blocked = false;
            for (const std::size_t other : ring) {
                if (other == prev || other == cur || other == next || v[other] == v[prev] ||
                    v[other] == v[cur] || v[other] == v[next]) {
                    continue;
                }
                const bool inside = (v[cur] - v[prev]).cross(v[other] - v[prev]) >= 0.0 &&
                                    (v[next] - v[cur]).cross(v[other] - v[cur]) >= 0.0 &&
                                    (v[prev] - v[next]).cross(v[other] - v[next]) >= 0.0;
                if (inside) {
                    blocked = true;
                    break;
                }
            }
            if (blocked) {
                continue;
            }
            triangles.push_back({prev, cur, next});
            ring.erase(ring.begin() + static_cast<std::ptrdiff_t>(i));
            clipped = true;
            break;
        }
        if (!clipped) {
            return makeError(ErrorCode::TriangulationFailure,
                             "no ear found; the polygon is not simple",
                             "remaining=" + std::to_string(ring.size()));
        }
    }

    const Triangle2 last{v[ring[0]], v[ring[1]], v[ring[2]]};
    if (!last.isDegenerate()) {
        triangles.push_back({ring[0], ring[1], ring[2]});
    }
    return triangles;
}

Result<std::vector<Segment2>> hatchLines(const Polyline2& boundary, double angle, double spacing,
                                         double offset)
{
    if (!boundary.closed || boundary.vertices.size() < 3) {
        return makeError(ErrorCode::InvalidGeometry,
                         "a hatch boundary must be a closed polygon of at least three vertices");
    }
    if (!std::isfinite(angle) || !std::isfinite(offset)) {
        return makeError(ErrorCode::InvalidArgument, "hatch angle and offset must be finite");
    }
    if (!(spacing > 0.0) || !std::isfinite(spacing)) {
        return makeError(ErrorCode::InvalidArgument, "hatch spacing must be a positive length");
    }
    for (const Point2& vertex : boundary.vertices) {
        if (!std::isfinite(vertex.x) || !std::isfinite(vertex.y)) {
            return makeError(ErrorCode::InvalidGeometry,
                             "a hatch boundary must have finite coordinates");
        }
    }

    // Along the lines, and across them. Every position is handled as a pair of
    // projections onto these, which keeps the whole routine independent of the
    // angle instead of special-casing vertical and horizontal families.
    const double dx = std::cos(angle);
    const double dy = std::sin(angle);
    const double nx = -dy;
    const double ny = dx;

    double lowest = 0.0;
    double highest = 0.0;
    for (std::size_t i = 0; i < boundary.vertices.size(); ++i) {
        const double across = boundary.vertices[i].x * nx + boundary.vertices[i].y * ny;
        lowest = i == 0 ? across : std::min(lowest, across);
        highest = i == 0 ? across : std::max(highest, across);
    }

    // Line k sits at offset + k * spacing, measured from the world origin, so
    // the family is a property of the drawing rather than of this boundary.
    const double firstExact = std::ceil((lowest - offset) / spacing);
    const double lastExact = std::floor((highest - offset) / spacing);
    if (!(lastExact >= firstExact)) {
        return std::vector<Segment2>{}; // the boundary falls between two lines
    }
    const double countExact = lastExact - firstExact + 1.0;
    if (!(countExact <= static_cast<double>(kMaxHatchLines))) {
        return makeError(ErrorCode::InvalidArgument,
                         "hatch spacing is too fine for this boundary: " +
                             std::to_string(static_cast<long long>(countExact)) +
                             " lines, the limit is " + std::to_string(kMaxHatchLines));
    }
    const auto first = static_cast<long long>(firstExact);
    const auto count = static_cast<long long>(countExact);

    std::vector<Segment2> segments;
    std::vector<double> crossings;
    for (long long k = 0; k < count; ++k) {
        const double across = offset + static_cast<double>(first + k) * spacing;
        crossings.clear();
        for (std::size_t e = 0; e < boundary.vertices.size(); ++e) {
            const Point2& p = boundary.vertices[e];
            const Point2& q = boundary.vertices[(e + 1) % boundary.vertices.size()];
            const double sp = p.x * nx + p.y * ny - across;
            const double sq = q.x * nx + q.y * ny - across;
            // Half-open: an edge counts when it starts on or below the line and
            // ends above it, or the reverse. A vertex lying exactly on the line
            // therefore belongs to one of its two edges and not the other, so
            // it contributes one crossing - not two (which would close the
            // interval immediately) and not none (which would leak the fill out
            // through the vertex). This is the convention point-in-polygon ray
            // casting uses, for the same reason.
            const bool upward = sp <= 0.0 && sq > 0.0;
            const bool downward = sq <= 0.0 && sp > 0.0;
            if (!upward && !downward) {
                continue;
            }
            const double t = sp / (sp - sq);
            const double x = p.x + t * (q.x - p.x);
            const double y = p.y + t * (q.y - p.y);
            crossings.push_back(x * dx + y * dy);
        }
        if (crossings.size() < 2) {
            continue;
        }
        std::sort(crossings.begin(), crossings.end());
        // Even-odd: inside between the first and second crossing, the third and
        // fourth, and so on, so a concave notch is left unfilled rather than
        // bridged. An odd count cannot occur for a closed boundary under the
        // rule above, but the loop is written so that a stray one is dropped
        // rather than paired with nothing.
        for (std::size_t i = 0; i + 1 < crossings.size(); i += 2) {
            const double a = crossings[i];
            const double b = crossings[i + 1];
            if (!(b > a)) {
                continue; // a tangency, not a span
            }
            segments.push_back(Segment2{Point2(a * dx + across * nx, a * dy + across * ny),
                                        Point2(b * dx + across * nx, b * dy + across * ny)});
        }
    }
    return segments;
}

} // namespace katana::geometry
