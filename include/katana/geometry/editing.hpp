#pragma once

// Curve editing operations used by the CAD commands (PLAN.MD Phase 04):
// offset, trim, extend, fillet, chamfer.
//
// All functions are pure: they take geometry and return new geometry. Failures
// are reported as InvalidGeometry / InvalidArgument errors, never silently.

#include <optional>
#include <span>
#include <variant>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/intersection.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::geometry {

using Curve2 = std::variant<Segment2, Arc2, Circle2>;

[[nodiscard]] IntersectionResult intersect(const Curve2& a, const Curve2& b);
[[nodiscard]] Point2 closestPoint(const Curve2& curve, const Point2& p);
[[nodiscard]] double distance(const Curve2& curve, const Point2& p);
[[nodiscard]] Box2 boundingBox(const Curve2& curve);

// ---- offset ------------------------------------------------------------------
// Segments and polylines: positive `distance` is to the left of the direction
// of travel. Circles and arcs: positive `distance` grows the radius.

[[nodiscard]] Segment2 offset(const Segment2& segment, double distance);

// nullopt when the offset radius would not be positive.
[[nodiscard]] std::optional<Circle2> offset(const Circle2& circle, double distance);
[[nodiscard]] std::optional<Arc2> offset(const Arc2& arc, double distance);

// Mitre joins at every vertex. The result is not cleaned of self-intersections,
// so it is only valid while |distance| is below the local feature size (half the
// narrowest neck, the radius of the tightest concave corner).
[[nodiscard]] katana::core::Result<Polyline2> offset(const Polyline2& polyline, double distance);

// Offsets `curve` by |distance| towards the side on which `side` lies.
[[nodiscard]] katana::core::Result<Curve2> offsetTowards(const Curve2& curve, double distance,
                                                         const Point2& side);

// ---- trim / extend -----------------------------------------------------------

struct TrimResult {
    std::vector<Curve2> remaining; // 1 or 2 pieces of the target
};

// Removes the portion of `target` that contains the point nearest to `pick` and
// is bounded by intersections with `cutters` (or by the target's own ends).
// A trimmed circle becomes an arc. Fails when no cutter crosses the target
// (a circle needs two crossings).
[[nodiscard]] katana::core::Result<TrimResult>
trim(const Curve2& target, std::span<const Curve2> cutters, const Point2& pick);

// Lengthens the end of `target` nearest to `pick` until it meets the first of
// `boundaries`. Segments extend along their line, arcs along their circle.
// Fails when no boundary lies ahead, or for circles (nothing to extend).
[[nodiscard]] katana::core::Result<Curve2>
extend(const Curve2& target, std::span<const Curve2> boundaries, const Point2& pick);

// ---- fillet / chamfer --------------------------------------------------------
// Both operate on the corner where the supporting lines of the two segments
// meet, keeping the end of each segment that is farther from that corner. The
// returned segments preserve the direction of the inputs.

struct FilletResult {
    Segment2 first;
    Segment2 second;
    std::optional<Arc2> arc; // from `first` to `second`; absent for radius 0
};

[[nodiscard]] katana::core::Result<FilletResult> fillet(const Segment2& first,
                                                        const Segment2& second, double radius);

struct ChamferResult {
    Segment2 first;
    Segment2 second;
    std::optional<Segment2> bevel; // from `first` to `second`; absent for zero distances
};

[[nodiscard]] katana::core::Result<ChamferResult> chamfer(const Segment2& first,
                                                          const Segment2& second,
                                                          double distanceFirst,
                                                          double distanceSecond);

} // namespace katana::geometry
