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
        for (std::size_t j = i + 1; j < n; ++j) {
            const IntersectionResult hit = intersect(first, polygon.segment(j));
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
    const std::size_t clipCount = clipCcw.vertices.size();
    for (std::size_t edge = 0; edge < clipCount && !output.empty(); ++edge) {
        const Point2& edgeStart = clipCcw.vertices[edge];
        const Vec2 edgeDirection = clipCcw.vertices[(edge + 1) % clipCount] - edgeStart;
        const auto side = [&](const Point2& p) { return edgeDirection.cross(p - edgeStart); };

        const std::vector<Point2> input = std::move(output);
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

} // namespace katana::geometry
