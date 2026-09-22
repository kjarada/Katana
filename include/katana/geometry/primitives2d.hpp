#pragma once

// 2D geometric primitives (PLAN.MD Phase 03).
//
// Conventions
//   * Angles are radians, counter-clockwise from +x.
//   * "Left" of a directed line is the side its counter-clockwise normal points to.
//   * Coincidence decisions use math::tolerance::kGeometric (model units) and
//     parallelism decisions use math::tolerance::kAngular; nothing else.
//   * Types are plain values. Degenerate values (zero-length segment, zero
//     radius) are representable; operations document how they treat them.

#include <array>
#include <cstddef>
#include <limits>
#include <optional>
#include <vector>

#include "katana/math/numerics.hpp"
#include "katana/math/vec2.hpp"

namespace katana::geometry {

using Vec2 = katana::math::Vec2;
using Point2 = katana::math::Vec2;

enum class Containment { Outside, OnBoundary, Inside };

// Axis-aligned bounding box. Default constructed boxes are empty.
struct Box2 {
    Point2 min{std::numeric_limits<double>::infinity(), std::numeric_limits<double>::infinity()};
    Point2 max{-std::numeric_limits<double>::infinity(),
               -std::numeric_limits<double>::infinity()};

    constexpr Box2() = default;
    constexpr Box2(const Point2& minIn, const Point2& maxIn) : min(minIn), max(maxIn) {}

    [[nodiscard]] constexpr bool empty() const { return min.x > max.x || min.y > max.y; }
    [[nodiscard]] constexpr double width() const { return empty() ? 0.0 : max.x - min.x; }
    [[nodiscard]] constexpr double height() const { return empty() ? 0.0 : max.y - min.y; }
    [[nodiscard]] constexpr Point2 center() const { return (min + max) * 0.5; }

    constexpr void expand(const Point2& p)
    {
        min = Point2(std::min(min.x, p.x), std::min(min.y, p.y));
        max = Point2(std::max(max.x, p.x), std::max(max.y, p.y));
    }
    constexpr void expand(const Box2& other)
    {
        if (!other.empty()) {
            expand(other.min);
            expand(other.max);
        }
    }

    // Grown by `margin` on every side (shrunk when negative).
    [[nodiscard]] constexpr Box2 inflated(double margin) const
    {
        return empty() ? Box2{}
                       : Box2(Point2(min.x - margin, min.y - margin),
                              Point2(max.x + margin, max.y + margin));
    }

    // Boundary inclusive.
    [[nodiscard]] constexpr bool contains(const Point2& p) const
    {
        return p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y;
    }
    [[nodiscard]] constexpr bool contains(const Box2& other) const
    {
        return !other.empty() && contains(other.min) && contains(other.max);
    }
    [[nodiscard]] constexpr bool intersects(const Box2& o) const
    {
        return !empty() && !o.empty() && min.x <= o.max.x && max.x >= o.min.x &&
               min.y <= o.max.y && max.y >= o.min.y;
    }

    friend constexpr bool operator==(const Box2&, const Box2&) = default;
};

// Infinite line through `origin` along `direction` (need not be unit length).
struct Line2 {
    Point2 origin;
    Vec2 direction{1.0, 0.0};

    // nullopt when the points coincide (closer than kGeometric).
    [[nodiscard]] static std::optional<Line2> through(const Point2& a, const Point2& b);

    [[nodiscard]] Point2 pointAt(double t) const { return origin + direction * t; }

    // Parameter of the orthogonal projection of `p`. 0 for a zero direction.
    [[nodiscard]] double parameterOf(const Point2& p) const;
    [[nodiscard]] Point2 closestPoint(const Point2& p) const { return pointAt(parameterOf(p)); }

    // Positive on the left of the direction. 0 for a zero direction.
    [[nodiscard]] double signedDistanceTo(const Point2& p) const;
    [[nodiscard]] double distanceTo(const Point2& p) const;
};

struct Segment2 {
    Point2 start;
    Point2 end;

    [[nodiscard]] Vec2 delta() const { return end - start; }
    [[nodiscard]] double length() const { return start.distanceTo(end); }
    [[nodiscard]] Point2 midpoint() const { return (start + end) * 0.5; }
    [[nodiscard]] Point2 pointAt(double t) const { return start + (end - start) * t; }
    [[nodiscard]] Segment2 reversed() const { return Segment2{end, start}; }
    [[nodiscard]] bool isDegenerate() const;

    // Parameter in [0, 1] of the point of the segment closest to `p`.
    [[nodiscard]] double parameterOf(const Point2& p) const;
    [[nodiscard]] Point2 closestPoint(const Point2& p) const { return pointAt(parameterOf(p)); }
    [[nodiscard]] double distanceTo(const Point2& p) const;
    [[nodiscard]] Box2 boundingBox() const;

    friend constexpr bool operator==(const Segment2&, const Segment2&) = default;
};

struct Circle2 {
    Point2 center;
    double radius = 0.0;

    [[nodiscard]] double area() const { return katana::math::kPi * radius * radius; }
    [[nodiscard]] double perimeter() const { return katana::math::kTwoPi * radius; }
    [[nodiscard]] Point2 pointAtAngle(double radians) const;

    [[nodiscard]] Containment classify(const Point2& p) const;
    [[nodiscard]] bool contains(const Point2& p) const
    {
        return classify(p) != Containment::Outside;
    }

    // Closest point on the circumference. For `p` at the centre every point is
    // equally close; the point at angle 0 is returned.
    [[nodiscard]] Point2 closestPoint(const Point2& p) const;
    // Distance to the circumference (not to the disc).
    [[nodiscard]] double distanceTo(const Point2& p) const;
    [[nodiscard]] Box2 boundingBox() const;

    friend constexpr bool operator==(const Circle2&, const Circle2&) = default;
};

// Circular arc from `startAngle`, sweeping by `sweep` radians. Positive sweep is
// counter-clockwise, negative clockwise; |sweep| <= 2*pi.
struct Arc2 {
    Point2 center;
    double radius = 0.0;
    double startAngle = 0.0;
    double sweep = 0.0;

    // Arc from `a` through `b` to `c`. nullopt when the points are collinear or
    // coincident.
    [[nodiscard]] static std::optional<Arc2> throughPoints(const Point2& a, const Point2& b,
                                                           const Point2& c);

    [[nodiscard]] double endAngle() const { return startAngle + sweep; }
    [[nodiscard]] double length() const;
    [[nodiscard]] Circle2 circle() const { return Circle2{center, radius}; }
    [[nodiscard]] Point2 pointAtAngle(double radians) const;
    [[nodiscard]] Point2 pointAt(double t) const { return pointAtAngle(startAngle + sweep * t); }
    [[nodiscard]] Point2 startPoint() const { return pointAtAngle(startAngle); }
    [[nodiscard]] Point2 endPoint() const { return pointAtAngle(endAngle()); }
    [[nodiscard]] Point2 midpoint() const { return pointAt(0.5); }
    [[nodiscard]] Arc2 reversed() const { return Arc2{center, radius, endAngle(), -sweep}; }

    // True when the direction `radians` lies within the sweep, widened by
    // `angularTolerance` at both ends.
    [[nodiscard]] bool containsAngle(double radians, double angularTolerance = 0.0) const;

    // Fraction in [0, 1] along the sweep of the arc point closest to direction
    // `radians`. Directions outside the sweep clamp to the nearer end.
    [[nodiscard]] double parameterOfAngle(double radians) const;

    [[nodiscard]] Point2 closestPoint(const Point2& p) const;
    [[nodiscard]] double distanceTo(const Point2& p) const;
    [[nodiscard]] Box2 boundingBox() const;

    friend constexpr bool operator==(const Arc2&, const Arc2&) = default;
};

// Chain of straight segments. When `closed`, an implicit segment joins the last
// vertex back to the first and the polyline bounds a polygon.
struct Polyline2 {
    std::vector<Point2> vertices;
    bool closed = false;

    [[nodiscard]] std::size_t segmentCount() const;
    [[nodiscard]] Segment2 segment(std::size_t index) const;

    [[nodiscard]] double length() const; // includes the closing segment when closed
    [[nodiscard]] double perimeter() const { return length(); }

    // Positive for counter-clockwise polygons. 0 when open or fewer than 3
    // vertices. Evaluated relative to the first vertex so that large projected
    // coordinates do not destroy precision.
    [[nodiscard]] double signedArea() const;
    [[nodiscard]] double area() const;

    // Area centroid of a closed polygon; nullopt when the area is degenerate.
    [[nodiscard]] std::optional<Point2> centroid() const;

    [[nodiscard]] Containment classify(const Point2& p) const; // Outside when open
    [[nodiscard]] bool contains(const Point2& p) const
    {
        return classify(p) != Containment::Outside;
    }

    // nullopt when there are no vertices.
    [[nodiscard]] std::optional<Point2> closestPoint(const Point2& p) const;
    [[nodiscard]] std::optional<double> distanceTo(const Point2& p) const;
    [[nodiscard]] Box2 boundingBox() const;

    [[nodiscard]] Polyline2 reversed() const;
    // Copy without consecutive vertices closer than kGeometric (and without a
    // closing duplicate of the first vertex when closed).
    [[nodiscard]] Polyline2 withoutDuplicateVertices() const;

    friend bool operator==(const Polyline2&, const Polyline2&) = default;
};

// Axis-aligned rectangle: `origin` is the minimum corner; width, height >= 0.
struct Rectangle2 {
    Point2 origin;
    double width = 0.0;
    double height = 0.0;

    // Normalises any two opposite corners.
    [[nodiscard]] static Rectangle2 fromCorners(const Point2& a, const Point2& b);

    [[nodiscard]] double area() const { return width * height; }
    [[nodiscard]] double perimeter() const { return 2.0 * (width + height); }
    [[nodiscard]] std::array<Point2, 4> corners() const; // counter-clockwise from origin
    [[nodiscard]] Polyline2 toPolyline() const;
    [[nodiscard]] Box2 boundingBox() const;
    [[nodiscard]] Containment classify(const Point2& p) const;
    [[nodiscard]] bool contains(const Point2& p) const
    {
        return classify(p) != Containment::Outside;
    }
    [[nodiscard]] Point2 closestPoint(const Point2& p) const; // on the boundary
    [[nodiscard]] double distanceTo(const Point2& p) const;   // to the boundary

    friend constexpr bool operator==(const Rectangle2&, const Rectangle2&) = default;
};

struct Triangle2 {
    Point2 a;
    Point2 b;
    Point2 c;

    [[nodiscard]] double signedArea() const; // positive when a,b,c are counter-clockwise
    // The cross product signedArea() halves, and the divisor of the barycentric
    // weights. Exposed so a caller evaluating many points in one triangle can
    // hoist it out of the loop rather than recompute it per point.
    [[nodiscard]] double twiceSignedArea() const;
    [[nodiscard]] double area() const;
    [[nodiscard]] double perimeter() const;
    [[nodiscard]] Point2 centroid() const;
    [[nodiscard]] bool isDegenerate() const; // collinear or coincident vertices
    [[nodiscard]] Box2 boundingBox() const;
    [[nodiscard]] Containment classify(const Point2& p) const;
    [[nodiscard]] bool contains(const Point2& p) const
    {
        return classify(p) != Containment::Outside;
    }

    // Weights (wa, wb, wc) with p = wa*a + wb*b + wc*c. nullopt when degenerate.
    [[nodiscard]] std::optional<std::array<double, 3>> barycentric(const Point2& p) const;
    // The same weights, from a doubled signed area the caller already has. The
    // one-argument form is this one behind isDegenerate(), which costs three
    // hypots and an area - both loop invariants when many points are weighted
    // against one triangle, as the surface overlay in terrain/volume.cpp does.
    // Meaningless when `twiceArea` is zero; ask isDegenerate() once, outside
    // the loop.
    [[nodiscard]] std::array<double, 3> barycentric(const Point2& p, double twiceArea) const;
    // nullopt when degenerate.
    [[nodiscard]] std::optional<Circle2> circumcircle() const;

    friend constexpr bool operator==(const Triangle2&, const Triangle2&) = default;
};

} // namespace katana::geometry
