#pragma once

// The drawing curves beyond the primitives: a polyline whose segments may be
// arcs and whose vertices may carry heights, the ellipse (and elliptical arc),
// and the spline. The owner asked on 2026-09-25 for "a professional drawing
// system, with full vertex control"; these are the shapes such a system
// draws that primitives2d.hpp cannot hold.
//
// Why new types rather than widening Polyline2: Polyline2 is straight 2D
// segments by contract, and the offset, trim, extend, break, lengthen, join,
// hatch, parcel and grading code all rely on that - each walks
// `vertices[i] -> vertices[i+1]` as a Segment2. Adding a bulge to it would
// have turned every one of those into a silent approximation of any polyline
// with an arc. A new kind is refused loudly where it is not handled instead
// (docs/model.md, "Adding a geometry kind"), and docs/drawing.md sets the rule
// for which kind a polyline is stored as.
//
// Conventions as primitives2d.hpp: radians counter-clockwise from +x,
// kGeometric for coincidence, plain values, degenerate values representable.

#include <cstddef>
#include <optional>
#include <variant>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::geometry {

// ---- the polyline with arcs and heights ------------------------------------------

// One vertex of a CurvePolyline2 and the segment that STARTS at it.
//
// `bulge` is the tangent of a quarter of the angle the segment's arc includes,
// the convention of DXF's LWPOLYLINE and of every CAD program since: 0 is a
// straight segment, positive an arc turning counter-clockwise from this vertex
// to the next, negative clockwise, 1 a counter-clockwise semicircle. It is
// chosen over a centre or a radius because it is defined by the two ends
// alone - moving a vertex keeps the arc's shape, a scale keeps it, a reverse
// only negates it - and because it is what the file formats carry. The bulge
// of the last vertex of an open polyline starts no segment and is ignored.
//
// `height` is the vertex's elevation; nullopt is "not surveyed", which is not
// zero (the same distinction entity::heightsOf keeps).
struct CurveVertex {
    Point2 position;
    double bulge = 0.0;
    std::optional<double> height;

    friend bool operator==(const CurveVertex&, const CurveVertex&) = default;
};

// A segment of a CurvePolyline2 as a primitive.
using CurveSegment = std::variant<Segment2, Arc2>;

// The bulge of the arc from `start` to `end` whose included angle is `sweep`
// (positive counter-clockwise): tan(sweep / 4).
[[nodiscard]] double bulgeFromSweep(double sweep);
// The arc a segment from `start` to `end` with `bulge` describes, or nullopt
// when the bulge is (nearly) zero or the ends coincide - a straight segment.
[[nodiscard]] std::optional<Arc2> arcFromBulge(const Point2& start, const Point2& end,
                                               double bulge);
// The bulge of the arc from `start` through `through` to `end`; 0 when the
// three are collinear (the segment is straight).
[[nodiscard]] double bulgeThrough(const Point2& start, const Point2& through,
                                  const Point2& end);

struct CurvePolyline2 {
    std::vector<CurveVertex> vertices;
    bool closed = false;

    // From a straight polyline, with a height per vertex when `heights` has
    // one per vertex (otherwise none).
    [[nodiscard]] static CurvePolyline2
    fromPolyline(const Polyline2& polyline, const std::vector<std::optional<double>>& heights = {});
    // From plain positions, every segment straight and no heights.
    [[nodiscard]] static CurvePolyline2 fromPoints(const std::vector<Point2>& points,
                                                   bool isClosed = false);

    [[nodiscard]] std::size_t segmentCount() const;
    // The vertex a segment ends at (wraps to 0 for a closed polyline's last).
    [[nodiscard]] std::size_t segmentEnd(std::size_t index) const;
    [[nodiscard]] bool isArc(std::size_t index) const;
    [[nodiscard]] CurveSegment segment(std::size_t index) const;
    [[nodiscard]] double segmentLength(std::size_t index) const;

    [[nodiscard]] bool hasArcs() const;
    [[nodiscard]] bool hasHeights() const;
    [[nodiscard]] std::vector<Point2> positions() const;
    [[nodiscard]] std::vector<std::optional<double>> heights() const;

    [[nodiscard]] double length() const; // includes the closing segment when closed
    // Positive for counter-clockwise; the arcs' segments of area included.
    // 0 when open or with fewer than two vertices.
    [[nodiscard]] double signedArea() const;
    [[nodiscard]] double area() const;

    // The point of the polyline closest to `p`, its distance along the
    // polyline from the first vertex, and the segment it lies on. nullopt
    // when there are no vertices.
    struct Nearest {
        Point2 point;
        double station = 0.0;
        std::size_t segment = 0;
        double distance = 0.0;
    };
    [[nodiscard]] std::optional<Nearest> nearest(const Point2& p) const;
    [[nodiscard]] std::optional<Point2> closestPoint(const Point2& p) const;
    [[nodiscard]] std::optional<double> distanceTo(const Point2& p) const;
    // The point `station` along the polyline (clamped to its ends).
    [[nodiscard]] Point2 pointAtStation(double station) const;
    // The height `station` along the polyline, interpolated linearly by
    // length between the two ends of the segment it lies on; nullopt when
    // either end has no height.
    [[nodiscard]] std::optional<double> heightAtStation(double station) const;
    // The distance along the polyline at which vertex `index` lies.
    [[nodiscard]] double stationOfVertex(std::size_t index) const;

    [[nodiscard]] Box2 boundingBox() const;

    // The same path walked the other way: vertices reversed, each bulge moved
    // to the vertex that now starts its segment and negated.
    [[nodiscard]] CurvePolyline2 reversed() const;

    // The path as chords no farther than `tolerance` from any arc, first
    // point to last (the closing point repeated when closed, so the result is
    // an open walk of the whole path).
    [[nodiscard]] std::vector<Point2> tessellate(double tolerance) const;
    // The same walk with a height at every point: a vertex's own, and on an
    // arc's chords the height interpolated by length along it (none where an
    // end has none) - the rule heightAtStation states.
    struct HeightedPoint {
        Point2 position;
        std::optional<double> height;
    };
    [[nodiscard]] std::vector<HeightedPoint> tessellateWithHeights(double tolerance) const;
    // As a straight polyline: exact when there are no arcs (the closing
    // segment implied, as Polyline2 does), chorded within `tolerance`
    // otherwise.
    [[nodiscard]] Polyline2 toPolyline(double tolerance) const;
    // The segments as primitives, in order.
    [[nodiscard]] std::vector<CurveSegment> segments() const;

    friend bool operator==(const CurvePolyline2&, const CurvePolyline2&) = default;
};

// The polyline offset by `distance` to the left of its direction of travel
// (negative: right). Straight segments move parallel, arcs become concentric
// arcs; consecutive pieces are joined where their offsets meet, and with a
// round join about the original vertex where they do not (the outside of a
// turn between an arc and a line that part). Heights stay with their
// vertices. Fails with InvalidGeometry when an arc would shrink to nothing,
// and - as geometry::offset for Polyline2 - is only faithful while |distance|
// is below the local feature size.
[[nodiscard]] katana::core::Result<CurvePolyline2> offset(const CurvePolyline2& polyline,
                                                          double distance);

// ---- the ellipse --------------------------------------------------------------------

// An ellipse or elliptical arc, as DXF's ELLIPSE holds one: a centre, the
// vector from it to one end of the major axis, the minor-to-major ratio, and
// the arc as a start and a sweep of the ECCENTRIC ANOMALY t, where the point
// at t is centre + major cos t + minor sin t and minor is the major axis
// turned a quarter counter-clockwise and scaled by the ratio. A sweep of a
// full turn is the whole ellipse. The sweep is always positive
// (counter-clockwise in t); a mirror is re-expressed rather than stored as a
// negative sweep, so every consumer reads one orientation.
struct Ellipse2 {
    Point2 center;
    Vec2 majorAxis{1.0, 0.0};
    double ratio = 1.0;          // (0, 1]
    double startParameter = 0.0; // radians of eccentric anomaly
    double sweep = katana::math::kTwoPi; // (0, 2pi]

    [[nodiscard]] bool isFull() const;
    [[nodiscard]] double majorRadius() const { return majorAxis.length(); }
    [[nodiscard]] double minorRadius() const { return majorAxis.length() * ratio; }
    [[nodiscard]] Vec2 minorAxis() const { return majorAxis.perpendicular() * ratio; }
    [[nodiscard]] double endParameter() const { return startParameter + sweep; }

    [[nodiscard]] Point2 pointAtParameter(double t) const;
    [[nodiscard]] Vec2 derivativeAtParameter(double t) const;
    [[nodiscard]] Point2 startPoint() const { return pointAtParameter(startParameter); }
    [[nodiscard]] Point2 endPoint() const { return pointAtParameter(endParameter()); }
    // The eccentric anomaly of the point of the whole ellipse in the
    // direction of `p` from the centre (not the nearest point).
    [[nodiscard]] double parameterTowards(const Point2& p) const;
    // True when parameter `t` lies within the arc.
    [[nodiscard]] bool containsParameter(double t, double tolerance = 0.0) const;

    // The four ends of the axes that lie on the arc: t = 0, pi/2, pi, 3pi/2.
    [[nodiscard]] std::vector<Point2> quadrants() const;

    [[nodiscard]] double length() const;
    [[nodiscard]] Box2 boundingBox() const;
    // The nearest point of the arc to `p`, found by sampling and refined by
    // Newton's method on the tangency condition.
    [[nodiscard]] Point2 closestPoint(const Point2& p) const;
    [[nodiscard]] double closestParameter(const Point2& p) const;
    [[nodiscard]] double distanceTo(const Point2& p) const;
    // Start to end in chords within `tolerance` (the closing point repeated
    // for a full ellipse).
    [[nodiscard]] std::vector<Point2> tessellate(double tolerance) const;

    // The ellipse through a centre, the end of one axis and a point giving
    // the other axis's length (its distance from the first axis's line).
    // Whichever axis is longer becomes the major one. nullopt when degenerate.
    [[nodiscard]] static std::optional<Ellipse2> fromAxes(const Point2& centre,
                                                          const Point2& axisEnd,
                                                          double otherRadius);

    friend constexpr bool operator==(const Ellipse2&, const Ellipse2&) = default;
};

// ---- the spline --------------------------------------------------------------------

// A B-spline (a NURBS when `weights` is not empty), held as DXF's SPLINE holds
// one: degree, control points, knot vector, optional weights, and the fit
// points it was made to pass through, if any.
//
// A spline drawn through points keeps them: `fitPoints` is what the user gave
// and what the grips move, and the control points and knots are derived from
// them by interpolation (`throughPoints`) and stored so that drawing never
// has to solve anything. A spline drawn by control points has no fit points.
struct Spline2 {
    int degree = 3;
    std::vector<Point2> controlPoints;
    std::vector<double> knots;     // controlPoints.size() + degree + 1, non-decreasing
    std::vector<double> weights;   // empty, or one positive weight per control point
    std::vector<Point2> fitPoints; // empty, or the points the spline interpolates

    // The clamped spline of `order` (clamped to what the point count allows)
    // whose control polygon is `points`, knots uniform.
    [[nodiscard]] static katana::core::Result<Spline2>
    fromControlPoints(std::vector<Point2> points, int order = 3);
    // The clamped spline through `points` in order, parameterised by chord
    // length with averaged knots (The NURBS Book, 9.2.1). Two points give a
    // line and three a parabola when `order` asks for more than they allow.
    [[nodiscard]] static katana::core::Result<Spline2> throughPoints(std::vector<Point2> points,
                                                                     int order = 3);

    [[nodiscard]] bool isRational() const { return !weights.empty(); }
    [[nodiscard]] bool hasFitPoints() const { return !fitPoints.empty(); }
    // The parameter range [knots[degree], knots[n]].
    [[nodiscard]] double domainStart() const;
    [[nodiscard]] double domainEnd() const;
    [[nodiscard]] Point2 pointAt(double u) const;
    [[nodiscard]] Point2 startPoint() const { return pointAt(domainStart()); }
    [[nodiscard]] Point2 endPoint() const { return pointAt(domainEnd()); }
    [[nodiscard]] bool isClosedShape() const;

    // Start to end in chords whose midpoints lie within `tolerance` of the
    // curve (adaptive subdivision of each knot span).
    [[nodiscard]] std::vector<Point2> tessellate(double tolerance) const;
    [[nodiscard]] Polyline2 toPolyline(double tolerance) const;
    [[nodiscard]] double length() const;
    // The curve's extent, to a tenth of kCurveChordTolerance (not the
    // control polygon's, which can be far larger).
    [[nodiscard]] Box2 boundingBox() const;
    // To a tenth of kCurveChordTolerance, on the curve's chords.
    [[nodiscard]] Point2 closestPoint(const Point2& p) const;
    [[nodiscard]] double distanceTo(const Point2& p) const;

    // The spline walked the other way (fit points, control points, weights
    // reversed and the knots reflected).
    [[nodiscard]] Spline2 reversed() const;
    // Structural validity: degree 1..10, enough control points, the knot and
    // weight counts, non-decreasing knots with a non-empty domain.
    [[nodiscard]] katana::core::Status checkStructure() const;

    friend bool operator==(const Spline2&, const Spline2&) = default;
};

// The chord tolerance the drawing uses when it has to turn a curve into
// straight pieces with no view to ask (hit testing, snapping, sections,
// exports that have no curve): a millimetre at metre units, below what a
// plotted line resolves. A view chords to its own pixel size instead.
inline constexpr double kCurveChordTolerance = 1.0e-3;

} // namespace katana::geometry
