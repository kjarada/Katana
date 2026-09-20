#pragma once

// Intersections of 2D primitives (PLAN.MD Phases 03/04).
//
// Every pair returns the same IntersectionResult:
//
//   None     no common point.
//   Points   1 or 2 isolated common points in `points[0 .. count)`.
//            `tangent` is set when the curves touch without crossing.
//   Overlap  infinitely many common points (collinear overlapping segments,
//            coincident lines or circles, arcs sharing part of a circle).
//            For bounded overlaps `points` holds the two ends of the shared
//            portion (count == 2); for unbounded/closed ones count == 0.
//
// Decisions use tolerance::kGeometric for coincidence and tolerance::kAngular
// for parallelism. Lines meeting at an angle below kAngular are "parallel":
// their intersection would be too ill-conditioned to be meaningful.
// Points are reported in a deterministic order (increasing parameter along the
// first operand).

#include <array>
#include <cstddef>

#include "katana/geometry/primitives2d.hpp"

namespace katana::geometry {

enum class IntersectionKind { None, Points, Overlap };

struct IntersectionResult {
    IntersectionKind kind = IntersectionKind::None;
    std::array<Point2, 2> points{};
    std::size_t count = 0;
    bool tangent = false;

    [[nodiscard]] bool exists() const { return kind != IntersectionKind::None; }
};

[[nodiscard]] IntersectionResult intersect(const Line2& a, const Line2& b);
[[nodiscard]] IntersectionResult intersect(const Line2& line, const Segment2& segment);
[[nodiscard]] IntersectionResult intersect(const Segment2& a, const Segment2& b);

[[nodiscard]] IntersectionResult intersect(const Line2& line, const Circle2& circle);
[[nodiscard]] IntersectionResult intersect(const Segment2& segment, const Circle2& circle);
[[nodiscard]] IntersectionResult intersect(const Circle2& a, const Circle2& b);

[[nodiscard]] IntersectionResult intersect(const Line2& line, const Arc2& arc);
[[nodiscard]] IntersectionResult intersect(const Segment2& segment, const Arc2& arc);
[[nodiscard]] IntersectionResult intersect(const Circle2& circle, const Arc2& arc);
[[nodiscard]] IntersectionResult intersect(const Arc2& a, const Arc2& b);

// Symmetric overloads so callers need not remember the argument order.
[[nodiscard]] inline IntersectionResult intersect(const Segment2& s, const Line2& l)
{
    return intersect(l, s);
}
[[nodiscard]] inline IntersectionResult intersect(const Circle2& c, const Line2& l)
{
    return intersect(l, c);
}
[[nodiscard]] inline IntersectionResult intersect(const Circle2& c, const Segment2& s)
{
    return intersect(s, c);
}
[[nodiscard]] inline IntersectionResult intersect(const Arc2& a, const Line2& l)
{
    return intersect(l, a);
}
[[nodiscard]] inline IntersectionResult intersect(const Arc2& a, const Segment2& s)
{
    return intersect(s, a);
}
[[nodiscard]] inline IntersectionResult intersect(const Arc2& a, const Circle2& c)
{
    return intersect(c, a);
}

} // namespace katana::geometry
