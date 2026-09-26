// Offsetting a polyline with arc segments (curves2d.hpp).
//
// Each straight segment moves parallel by the distance and each arc becomes
// the concentric arc of radius r -/+ d (minus on the side of its centre).
// The moved pieces no longer meet: at each vertex the two neighbours are
// joined where their SUPPORTING curves (the infinite line, the whole circle)
// cross nearest the vertex - a mitre for two lines, the natural corner for a
// line and an arc - and where they do not cross, which happens on the outside
// of a turn between a line and an arc that part, by a round join: an arc of
// radius |d| about the original vertex, as every CAD program's offset does.

#include <algorithm>
#include <cmath>
#include <limits>
#include <string>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/geometry/curves2d.hpp"
#include "katana/math/numerics.hpp"

namespace katana::geometry {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

struct Moved {
    bool arc = false;
    Point2 start;
    Point2 end;
    // A line's direction; an arc's centre, radius and sweep direction.
    Vec2 direction;
    Point2 centre;
    double radius = 0.0;
    double sweep = 0.0; // the original sweep, for its sign and size
};

std::vector<Point2> lineLine(const Moved& a, const Moved& b)
{
    const double denominator = a.direction.cross(b.direction);
    if (std::abs(denominator) <= tol::kAngular * a.direction.length() * b.direction.length()) {
        return {};
    }
    const double t = (b.start - a.start).cross(b.direction) / denominator;
    return {a.start + a.direction * t};
}

std::vector<Point2> lineCircle(const Moved& line, const Moved& circle)
{
    const Vec2 d = line.direction.normalized();
    const Vec2 f = line.start - circle.centre;
    const double b = f.dot(d);
    const double c = f.dot(f) - circle.radius * circle.radius;
    const double discriminant = b * b - c;
    if (discriminant < -tol::kGeometric * std::max(1.0, circle.radius)) {
        return {};
    }
    const double root = std::sqrt(std::max(0.0, discriminant));
    return {line.start + d * (-b - root), line.start + d * (-b + root)};
}

std::vector<Point2> circleCircle(const Moved& a, const Moved& b)
{
    const Vec2 between = b.centre - a.centre;
    const double distance = between.length();
    if (!(distance > tol::kGeometric) || distance > a.radius + b.radius + tol::kGeometric ||
        distance < std::abs(a.radius - b.radius) - tol::kGeometric) {
        return {};
    }
    const double along = (a.radius * a.radius - b.radius * b.radius + distance * distance) /
                         (2.0 * distance);
    const double across = std::sqrt(std::max(0.0, a.radius * a.radius - along * along));
    const Vec2 unit = between / distance;
    const Point2 foot = a.centre + unit * along;
    return {foot + unit.perpendicular() * across, foot - unit.perpendicular() * across};
}

std::vector<Point2> crossings(const Moved& a, const Moved& b)
{
    if (!a.arc && !b.arc) {
        return lineLine(a, b);
    }
    if (!a.arc) {
        return lineCircle(a, b);
    }
    if (!b.arc) {
        return lineCircle(b, a);
    }
    return circleCircle(a, b);
}

// The bulge of a moved arc between the points it now starts and ends at, in
// its own direction; nullopt when the join has walked past its end (the arc
// has been offset out of existence).
std::optional<double> arcBulge(const Moved& piece, const Point2& from, const Point2& to)
{
    const double a0 = (from - piece.centre).angle();
    const double a1 = (to - piece.centre).angle();
    double sweep = piece.sweep > 0.0 ? katana::math::normalizeAngle(a1 - a0)
                                     : -katana::math::normalizeAngle(a0 - a1);
    if (std::abs(piece.sweep) > katana::math::kTwoPi - 1e-9 && std::abs(sweep) < 1e-12) {
        sweep = piece.sweep; // a whole circle offset whole
    }
    if (std::abs(sweep - piece.sweep) > katana::math::kPi) {
        return std::nullopt;
    }
    return bulgeFromSweep(sweep);
}

} // namespace

Result<CurvePolyline2> offset(const CurvePolyline2& polyline, double distance)
{
    if (!std::isfinite(distance)) {
        return makeError(ErrorCode::InvalidArgument, "the offset distance is not finite");
    }
    const std::size_t count = polyline.segmentCount();
    if (count == 0) {
        return makeError(ErrorCode::InvalidGeometry, "a polyline needs a segment to offset");
    }
    std::vector<Moved> pieces;
    pieces.reserve(count);
    for (std::size_t i = 0; i < count; ++i) {
        const Point2& a = polyline.vertices[i].position;
        const Point2& b = polyline.vertices[polyline.segmentEnd(i)].position;
        Moved piece;
        if (const auto arc = arcFromBulge(a, b, polyline.vertices[i].bulge)) {
            piece.arc = true;
            piece.centre = arc->center;
            piece.sweep = arc->sweep;
            // Left of a counter-clockwise arc is its centre.
            piece.radius = arc->sweep > 0.0 ? arc->radius - distance : arc->radius + distance;
            if (!(piece.radius > tol::kGeometric)) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "an offset of " + katana::core::formatExactReal(std::abs(distance)) +
                                     " is more than the radius of arc segment " +
                                     std::to_string(i) + "; the arc would vanish");
            }
            piece.start = arc->center + (a - arc->center).normalized() * piece.radius;
            piece.end = arc->center + (b - arc->center).normalized() * piece.radius;
        } else {
            const Vec2 along = b - a;
            if (!(along.length() > tol::kGeometric)) {
                // A zero-length segment carries no direction; it is dropped by
                // taking its neighbours' join.
                piece.direction = Vec2{};
                piece.start = a;
                piece.end = b;
                pieces.push_back(piece);
                continue;
            }
            const Vec2 left = along.normalized().perpendicular() * distance;
            piece.direction = along;
            piece.start = a + left;
            piece.end = b + left;
        }
        pieces.push_back(piece);
    }

    struct Join {
        Point2 first;              // where the incoming piece ends
        std::optional<Point2> second; // a round join's other end
        double roundBulge = 0.0;
    };
    const auto joinAt = [&](std::size_t vertex, const Moved& in, const Moved& out) -> Join {
        if (in.start.distanceTo(in.end) <= 0.0 && !in.arc && in.direction == Vec2{}) {
            return Join{out.start, std::nullopt, 0.0};
        }
        if (in.end.distanceTo(out.start) <= tol::kGeometric) {
            return Join{in.end, std::nullopt, 0.0};
        }
        const Point2 middle = (in.end + out.start) * 0.5;
        std::optional<Point2> best;
        double bestDistance = std::numeric_limits<double>::infinity();
        for (const Point2& p : crossings(in, out)) {
            const double d = p.distanceTo(middle);
            if (d < bestDistance) {
                bestDistance = d;
                best = p;
            }
        }
        // A crossing far beyond the gap is the far side of a circle, not a
        // corner: the round join is the offset there.
        const double gap = in.end.distanceTo(out.start);
        if (best && bestDistance <= 4.0 * std::max(gap, std::abs(distance))) {
            return Join{*best, std::nullopt, 0.0};
        }
        const Point2& v = polyline.vertices[vertex].position;
        const Vec2 from = in.end - v;
        const Vec2 to = out.start - v;
        return Join{in.end, out.start, bulgeFromSweep(std::atan2(from.cross(to), from.dot(to)))};
    };

    std::vector<Join> joins(polyline.vertices.size());
    const std::size_t n = polyline.vertices.size();
    for (std::size_t v = 0; v < n; ++v) {
        const bool interior = polyline.closed || (v > 0 && v + 1 < n);
        if (!interior) {
            joins[v] = Join{v == 0 ? pieces.front().start : pieces.back().end, std::nullopt, 0.0};
            continue;
        }
        const std::size_t in = v == 0 ? count - 1 : v - 1;
        joins[v] = joinAt(v, pieces[in], pieces[v % count]);
    }

    CurvePolyline2 out;
    out.closed = polyline.closed;
    for (std::size_t v = 0; v < n; ++v) {
        const auto& height = polyline.vertices[v].height;
        const Join& join = joins[v];
        const Point2 startOfNext = join.second ? *join.second : join.first;
        double nextBulge = 0.0;
        if (v < count) {
            const Moved& piece = pieces[v];
            if (piece.arc) {
                const Point2 endOfPiece = joins[polyline.segmentEnd(v)].first;
                const auto bulge = arcBulge(piece, startOfNext, endOfPiece);
                if (!bulge) {
                    return makeError(ErrorCode::InvalidGeometry,
                                     "an offset of " +
                                         katana::core::formatExactReal(std::abs(distance)) +
                                         " consumes arc segment " + std::to_string(v));
                }
                nextBulge = *bulge;
            }
        }
        if (join.second) {
            out.vertices.push_back(CurveVertex{join.first, join.roundBulge, height});
            out.vertices.push_back(CurveVertex{*join.second, nextBulge, height});
        } else {
            out.vertices.push_back(CurveVertex{join.first, nextBulge, height});
        }
    }
    if (!out.closed) {
        out.vertices.back().bulge = 0.0;
    }
    return out;
}

} // namespace katana::geometry
