#pragma once

// Polygon algorithms (PLAN.MD Phase 04). A polygon is a closed Polyline2.
//
// Not yet provided: boolean set operations (union / intersection / difference
// of arbitrary polygons) and polygons with holes. Clipping is available against
// convex clip regions only.

#include <array>
#include <cstddef>
#include <optional>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::geometry {

enum class Orientation { Clockwise, CounterClockwise, Degenerate };

// Degenerate when open, fewer than 3 vertices, or the area is negligible.
[[nodiscard]] Orientation orientation(const Polyline2& polygon);

// True when every turn has the same sign (collinear vertices allowed).
[[nodiscard]] bool isConvex(const Polyline2& polygon);

// True when no two edges meet other than consecutive edges at their shared
// vertex. O(n^2); intended for validation, not for bulk data.
[[nodiscard]] bool isSimple(const Polyline2& polygon);

// Andrew's monotone chain. Counter-clockwise, without collinear points and
// without repeating the first point. Fewer than 3 distinct points are returned
// as they are (deduplicated).
[[nodiscard]] std::vector<Point2> convexHull(std::vector<Point2> points);

// Douglas-Peucker: drops vertices closer than `tolerance` to the simplified
// chain. Endpoints of open polylines are always kept. tolerance <= 0 returns
// the input unchanged.
[[nodiscard]] Polyline2 simplify(const Polyline2& polyline, double tolerance);

// Liang-Barsky. nullopt when the segment lies outside the box. A segment that
// only touches the box boundary yields a zero-length segment.
[[nodiscard]] std::optional<Segment2> clip(const Segment2& segment, const Box2& box);

// The pieces of a polyline that lie in a box, in order along it: what a
// renderer strokes when most of a long line is off screen. A contour crossing
// the whole site handed to the painter as it is costs as much when a sliver
// of it shows as when all of it does, because every vertex is transformed and
// every segment stroked whether or not it can reach a pixel.
//
// A run is a maximal stretch of consecutive segments that meet the box. Each
// segment is clipped on its own parameter (Liang-Barsky, as clip() does), and
// a run is broken where one leaves the box, so a line that goes out and comes
// back in is two runs and not one run with a chord across the outside. A
// vertex inside the box is copied EXACTLY, not recomputed, so a polyline that
// lies wholly in the box comes back as one run equal to it - a renderer that
// clips draws it pixel for pixel as one that does not. A crossing point is
// computed, and lies on the box's boundary. `closed` adds the segment from
// the last vertex back to the first, which ends the last run at the first
// vertex; the runs are not joined across it, since a closed polyline drawn
// as a polyline has two ends there too. Every run has at least two points.
//
// Deterministic: the same vertices and box give the same runs, bit for bit.
struct PolylineRuns {
    std::vector<Point2> points;    // every run's points, one run after another
    std::vector<std::size_t> ends; // one past each run's last point in `points`

    [[nodiscard]] std::size_t size() const { return ends.size(); }
    [[nodiscard]] bool empty() const { return ends.empty(); }
    [[nodiscard]] std::size_t begin(std::size_t run) const { return run == 0 ? 0 : ends[run - 1]; }
    void clear()
    {
        points.clear();
        ends.clear();
    }
};

// How a segment that crosses the box's boundary is kept: cut at the
// boundary (Cut), or whole, both its vertices as they are (WholeSegments).
// WholeSegments is for a renderer that must paint exactly the pixels the
// unclipped line paints: a segment cut short is the same line in exact
// arithmetic but not in a rasteriser's fixed-point edges, whose
// antialiasing then differs by a level or two along its whole length. Kept
// whole, only the segments that miss the box are dropped - for a long line
// seen in part, nearly all of it.
enum class PolylineClip { Cut, WholeSegments };

// Replaces `runs` with the pieces of `vertices` in `box` (see PolylineRuns).
// With WholeSegments a run is a stretch of consecutive segments that each
// meet the box, every point a vertex of the polyline. An empty box, or fewer
// than two vertices, gives no runs. `runs` is an out-parameter so that a
// caller clipping thousands of polylines a frame reuses one buffer.
void clipPolyline(const std::vector<Point2>& vertices, bool closed, const Box2& box,
                  PolylineRuns& runs, PolylineClip mode = PolylineClip::Cut);

// Sutherland-Hodgman clipping of `subject` against the convex polygon `clip`
// (either orientation). The result may have no vertices. Fails with
// InvalidGeometry when `clip` is not a convex polygon or `subject` is not closed.
[[nodiscard]] katana::core::Result<Polyline2> clipPolygon(const Polyline2& subject,
                                                          const Polyline2& clip);

using TriangleIndices = std::array<std::size_t, 3>;

// Ear-clipping triangulation of a simple polygon without holes. Indices refer
// to `polygon.vertices`; triangles are counter-clockwise. Vertices collinear
// with their neighbours are not referenced by any triangle. O(n^2).
// Fails with TriangulationFailure for non-simple or degenerate input.
[[nodiscard]] katana::core::Result<std::vector<TriangleIndices>>
triangulate(const Polyline2& polygon);

// ---- hatching ---------------------------------------------------------------------

// The most lines one hatch may produce. A pattern finer than this over a given
// boundary is not a drawing, it is a solid fill that takes a minute to render,
// and the user has almost certainly typed a spacing in the wrong unit - a
// 0.5 mm pattern spacing entered as metres over a 200 m parcel is 400 lines,
// the same mistake entered as millimetres is 400 000. Reporting that is a
// service; silently drawing it is not (PLAN.MD section 36).
inline constexpr std::size_t kMaxHatchLines = 100000;

// Segments filling `boundary` with a family of parallel lines, for hatching.
//
// `angle` is measured counter-clockwise from the +x axis, in radians, and
// `spacing` is the perpendicular distance between neighbouring lines, in model
// units - so a hatch keeps its size on the ground at every zoom, exactly as a
// linetype pattern does.
//
// `offset` shifts the family perpendicular to itself. It is measured from the
// WORLD ORIGIN, not from the boundary, and that is the whole point: two
// adjacent parcels hatched with the same pattern produce lines that continue
// across the shared edge instead of each starting afresh at its own corner,
// which is what makes a hatched drawing look drawn rather than assembled.
//
// The boundary may be concave. It is filled by the even-odd rule: crossings
// along each line are sorted and taken in pairs, so a line entering and leaving
// a concave notch contributes two segments rather than one spanning the gap. An
// edge is counted as crossed under a half-open rule, so a vertex lying exactly
// on a hatch line is counted once rather than twice or not at all.
//
// Fails with InvalidGeometry when `boundary` is not a closed polygon of at
// least three vertices, InvalidArgument for a non-finite angle or offset or a
// spacing that is not positive, or a spacing so fine for this boundary that
// the family would exceed `kMaxHatchLines`. That last error names the count
// and the limit, because the useful reply is which unit was meant.
[[nodiscard]] katana::core::Result<std::vector<Segment2>>
hatchLines(const Polyline2& boundary, double angle, double spacing, double offset = 0.0);

} // namespace katana::geometry
