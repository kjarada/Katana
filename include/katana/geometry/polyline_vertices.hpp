#pragma once

// Vertex editing of a polyline: the arithmetic behind the grips, the Draw >
// Vertices tools, the Vertices panel and the VERTEX verbs (docs/drawing.md).
//
// Every operation is a pure function from one CurvePolyline2 to another, so
// the grips, the tools, the panel and the command line cannot come to
// disagree about what "delete a vertex" or "weed to 5 cm" means - they are
// four front ends on these. A straight polyline goes in as a CurvePolyline2
// with no bulges (CurvePolyline2::fromPolyline) and the caller decides which
// kind the answer is stored as (cad/vertex_editing.hpp).
//
// Indices are vertex indices unless a parameter says segment; segment i runs
// from vertex i to vertex i+1 (to vertex 0 for a closed polyline's last).
// Failures are InvalidArgument for an index or a value out of range and
// InvalidGeometry for an edit that would leave less than a polyline (fewer
// than two vertices, three when closed), each with a sentence a user reads.
//
// Heights follow the vertices they belong to. A vertex an operation makes
// (an insert, a densify, a fillet's tangent points) takes the height
// interpolated by length between the ends of the segment it is made on, and
// none when either end has none: an invented height on a survey string is
// worse than a missing one.

#include <cstddef>
#include <optional>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/curves2d.hpp"

namespace katana::geometry {

using PolylineResult = katana::core::Result<CurvePolyline2>;

// The vertex nearest to `p`, and the segment nearest to `p` (by distance to
// the segment itself). nullopt for an empty polyline / one with no segment.
[[nodiscard]] std::optional<std::size_t> nearestVertex(const CurvePolyline2& polyline,
                                                       const Point2& p);
[[nodiscard]] std::optional<std::size_t> nearestSegment(const CurvePolyline2& polyline,
                                                        const Point2& p);

// The fewest vertices a polyline may have: two, or three when closed.
[[nodiscard]] std::size_t minimumVertices(const CurvePolyline2& polyline);

// ---- one vertex ------------------------------------------------------------------------

// Splits segment `segment` with a new vertex at `at`. On an arc the new vertex
// is `at` projected onto the arc and the two halves keep the arc's circle;
// on a straight segment it is `at` itself (a vertex may be inserted off the
// line, which is how a segment is bent). `height` overrides the interpolated
// height.
[[nodiscard]] PolylineResult insertVertex(const CurvePolyline2& polyline, std::size_t segment,
                                          const Point2& at,
                                          std::optional<double> height = std::nullopt);
// Removes vertex `index`; the two segments either side become one straight
// segment. Refused when it would leave fewer than minimumVertices().
[[nodiscard]] PolylineResult deleteVertex(const CurvePolyline2& polyline, std::size_t index);
// Removes every listed vertex at once (the several hot grips of one Delete).
[[nodiscard]] PolylineResult deleteVertices(const CurvePolyline2& polyline,
                                            std::vector<std::size_t> indices);
// Moves vertex `index` to `to`. Each arc keeps its bulge, so it keeps its
// shape between its moved ends, and the vertex keeps its height.
[[nodiscard]] PolylineResult moveVertex(const CurvePolyline2& polyline, std::size_t index,
                                        const Point2& to);
[[nodiscard]] PolylineResult setVertexHeight(const CurvePolyline2& polyline, std::size_t index,
                                             std::optional<double> height);
// Sets the bulge of segment `segment` (0 makes it straight).
[[nodiscard]] PolylineResult setSegmentBulge(const CurvePolyline2& polyline, std::size_t segment,
                                             double bulge);

// ---- segments ---------------------------------------------------------------------------

// Makes segment `segment` the arc from its start through `through` to its end.
// Refused when `through` is in line with the ends.
[[nodiscard]] PolylineResult segmentToArc(const CurvePolyline2& polyline, std::size_t segment,
                                          const Point2& through);
[[nodiscard]] PolylineResult segmentToLine(const CurvePolyline2& polyline, std::size_t segment);
// Moves segment `segment` bodily by `delta` (a midpoint grip's stretch): both
// its end vertices move, and the neighbouring segments stretch to follow.
[[nodiscard]] PolylineResult moveSegment(const CurvePolyline2& polyline, std::size_t segment,
                                         const Vec2& delta);

// ---- ranges ------------------------------------------------------------------------------

// The vertices strictly between `from` and `to`, walking forward from `from`
// (and round through the start of a closed polyline when to < from).
[[nodiscard]] std::vector<std::size_t> verticesBetween(const CurvePolyline2& polyline,
                                                       std::size_t from, std::size_t to);
// Straighten: removes the vertices strictly between `from` and `to`, leaving
// one straight segment from `from` to `to`.
[[nodiscard]] PolylineResult straighten(const CurvePolyline2& polyline, std::size_t from,
                                        std::size_t to);
// Grade: gives the vertices strictly between `from` and `to` heights on the
// straight grade between theirs, by length along the polyline. Both ends need
// heights.
[[nodiscard]] PolylineResult gradeBetween(const CurvePolyline2& polyline, std::size_t from,
                                          std::size_t to);
// Interpolate: every vertex with no height that lies between two with heights
// takes the height interpolated by length between them. The ends of an open
// polyline beyond the first and last heights stay without (no extrapolation).
[[nodiscard]] PolylineResult interpolateHeights(const CurvePolyline2& polyline);

// The open path from station `from` to station `to` along the polyline
// (0 <= from < to <= length; the closing segment of a closed one counts):
// every vertex between kept, an arc cut part-way keeping its circle (its
// bulge rescaled to the part's sweep), and the heights of the cut ends
// interpolated by length on their segments (none when either end has none;
// a vertex's own when the cut falls on it). What Break and Trim cut out.
[[nodiscard]] CurvePolyline2 subPath(const CurvePolyline2& polyline, double from, double to);
// A closed polyline's open path from `from` forward, through the first
// vertex, round to `to` (to < from).
[[nodiscard]] CurvePolyline2 wrappingPath(const CurvePolyline2& polyline, double from, double to);

// ---- whole polyline ---------------------------------------------------------------------------

// Douglas-Peucker: removes vertices while no removed vertex lies farther than
// `tolerance` from the simplified path in plan, nor (where both it and the
// path have heights there) farther than `tolerance` in height. The ends of an
// open polyline, the vertices either end of an arc segment, and every vertex
// with keep[i] true stay. A closed polyline keeps at least three vertices.
[[nodiscard]] PolylineResult weed(const CurvePolyline2& polyline, double tolerance,
                                  const std::vector<bool>& keep = {});
// Densify: every straight segment longer than `step` is divided into equal
// parts no longer than it; with `chordTolerance` > 0, every arc is replaced
// by straight chords within that tolerance of it (and then divided by `step`
// too). A step of 0 leaves straight segments as they are.
[[nodiscard]] PolylineResult densify(const CurvePolyline2& polyline, double step,
                                     double chordTolerance = 0.0);
// Consecutive vertices within `tolerance` of each other are merged into the
// first of them (and the closing duplicate of a closed polyline removed).
[[nodiscard]] PolylineResult mergeNearVertices(const CurvePolyline2& polyline, double tolerance);
// Every vertex moved to the nearest node of the grid of `spacing` through
// `origin`; vertices that then coincide are merged.
[[nodiscard]] PolylineResult snapToGrid(const CurvePolyline2& polyline, double spacing,
                                        const Point2& origin = Point2{});
// Close joins the last vertex back to the first (dropping a last vertex that
// already lies on the first); Open removes the closing segment.
[[nodiscard]] PolylineResult closePolyline(const CurvePolyline2& polyline);
[[nodiscard]] PolylineResult openPolyline(const CurvePolyline2& polyline);
// A closed polyline renumbered to start at vertex `index`; its shape is
// unchanged.
[[nodiscard]] PolylineResult changeStartVertex(const CurvePolyline2& polyline, std::size_t index);

// ---- corners ----------------------------------------------------------------------------------

// Rounds vertex `index`, where two straight segments meet, with a tangent arc
// of `radius`: the vertex is replaced by the two tangent points. Refused at
// the ends of an open polyline, beside an arc, for collinear segments, or
// when the arc would not fit on the shorter segment.
[[nodiscard]] PolylineResult filletVertex(const CurvePolyline2& polyline, std::size_t index,
                                          double radius);
// Cuts vertex `index` with a straight bevel `first` back along the incoming
// segment and `second` along the outgoing one.
[[nodiscard]] PolylineResult chamferVertex(const CurvePolyline2& polyline, std::size_t index,
                                           double first, double second);

} // namespace katana::geometry
