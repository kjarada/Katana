#pragma once

// Editing a TIN surface, and surfaces made of others (docs/surface_editing.md).
//
// A TinSurface is immutable: every edit here is a pure function that returns
// a NEW surface, or a refusal saying specifically why - the surface handed in
// is never touched, so the caller decides whether the result replaces it (one
// undo step through cad::Document::setSurfaces) or is kept beside it.
//
// What every edit keeps, unless it is the edit's purpose to change it:
//   * THE CONSTRAINED EDGES. A breakline or boundary edge stays an edge, flag
//     and all; a vertex on one is never moved by smoothing nor deleted, and a
//     new line crossing one meets it at the level the old edge has there.
//   * THE FOOTPRINT. Only the triangles an edit has to change are rebuilt
//     (re-triangulated by the one constrained Delaunay kernel, cdt_backend),
//     inside the ring of edges around them, which is held; the rest of the
//     surface is the same triangles, vertex for vertex. Holes stay holes.
//   * EXACTNESS where it decides. Which side of an edge a point lies on, and
//     whether two triangles make a convex quadrilateral, are decided with the
//     triangulation's own exact predicate (detail::orientation), not with a
//     tolerance. Where the kernel has to make a vertex - a new line crossing
//     an old edge - the vertex's level is interpolated along the edge whose
//     level holds there (docs/surface_editing.md, "Levels of new vertices").
//
// An edit that rebuilds triangles gives a canonical surface, as buildTin's
// is: vertices of the old surface in their order (those left without a
// triangle dropped), then new ones; every triangle rotated to start at its
// smallest vertex and the triangles sorted. A level edit (RAISE, SMOOTH ...)
// keeps the triangles as they were, in the order they were. An edit that
// changes nothing returns the surface as it was, not a reordered copy of it.
// The same edit of the same surface gives the same bits.

#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::core {
class TaskPool;
}

namespace katana::terrain {

using Polyline2 = katana::geometry::Polyline2;

// The largest coordinate or level, in metres, an edit accepts. Beyond a
// projected grid or a reduced level by orders of magnitude (a UTM easting is
// 1e7 at most), and well inside what areas and volumes (sums of products of
// three such numbers) hold: a typing slip such as RAISE 1e308 is refused
// where it is made, instead of making a surface whose plan area reads nan.
inline constexpr double kMostMagnitude = 1.0e9;

// What an edit did: what a verb's reply says.
struct TinEditReport {
    std::size_t trianglesBefore = 0;
    std::size_t trianglesAfter = 0;
    std::size_t verticesBefore = 0;
    std::size_t verticesAfter = 0;
    // Vertices the edit made: the points added, a breakline's or a
    // polygon's vertices, and the vertices made where a new line crossed an
    // old edge (counted again in `crossings`).
    std::size_t pointsAdded = 0;
    // Vertices the edit took out (deleted, thinned, or left without a
    // triangle by a deletion, a boundary or a hole).
    std::size_t pointsRemoved = 0;
    // Vertices of the surface a point given at their position gave a new
    // level to (ADD POINT on an existing vertex, a 3D breakline through one,
    // the seam of a paste).
    std::size_t pointsRelevelled = 0;
    // Vertices whose level a height edit or smoothing changed, or that
    // movePoint moved in plan (the vertex it makes is a new one, so that edit
    // also counts one added and one removed).
    std::size_t pointsMoved = 0;
    // Vertices an edit left alone on purpose: on a breakline or the rim
    // (smoothing, deleting points inside a polygon).
    std::size_t pointsHeld = 0;
    // Vertices made where a new line crossed an edge of the surface.
    std::size_t crossings = 0;
    // deleteBreaklines: the breakline edges made ordinary edges, and the
    // constrained edges along the line it left alone on the rim (the
    // surface's boundary, not a breakline).
    std::size_t edgesCleared = 0;
    std::size_t edgesHeld = 0;
    // The largest change of level at any vertex (heights, smoothing), or the
    // bound on how far a thinned surface lies from the original (thinning).
    double largestChange = 0.0;
};

struct TinEditResult {
    TinSurface surface;
    TinEditReport report;
};

// ---- what an edit acts on ------------------------------------------------------------
//
// The finders a front end uses to turn a click or a polygon into the indices
// the edits take. A position off the surface finds nothing.

// The triangle under `position` (TinSurface::locate).
[[nodiscard]] std::optional<std::uint32_t> triangleAt(const TinSurface& surface,
                                                      const Point2& position);

struct TinEdgeRef {
    std::uint32_t triangle = kNoTriangle;
    std::uint32_t edge = 0; // edge k joins the triangle's vertex k and k + 1
};
// The edge of the triangle under `position` that `position` lies nearest.
[[nodiscard]] std::optional<TinEdgeRef> edgeAt(const TinSurface& surface, const Point2& position);

// The corner of the triangle under `position` that lies nearest it; nullopt
// off the surface. With a `reach` (the snap aperture of a click) that corner
// when it is within reach; else the vertex nearest `position` within reach,
// wherever it is: a click a hair outside the rim, or in a thin triangle whose
// corners are all farther than the vertex beside it, finds that vertex.
// (Where a corner is within reach the answer is the one it always was.)
[[nodiscard]] std::optional<std::uint32_t> vertexAt(const TinSurface& surface,
                                                    const Point2& position,
                                                    std::optional<double> reach = std::nullopt);

// Whether `vertex` is an end of a constrained (breakline or boundary) edge:
// the rule deletePoints refuses such a vertex by, for a front end to say so
// before the click rather than after (the one test, not a second copy).
[[nodiscard]] bool onConstrainedEdge(const TinSurface& surface, std::uint32_t vertex);

// The triangles whose centroid lies inside `polygon` (on its edge counts as
// inside), or outside it. `polygon` is taken as closed.
[[nodiscard]] std::vector<std::uint32_t>
trianglesByPolygon(const TinSurface& surface, const Polyline2& polygon, bool inside);
// The triangles with a side longer than `length` in plan.
[[nodiscard]] std::vector<std::uint32_t> trianglesLongerThan(const TinSurface& surface,
                                                             double length);
// The triangles whose smallest angle in plan is below `radians`.
[[nodiscard]] std::vector<std::uint32_t> trianglesThinnerThan(const TinSurface& surface,
                                                              double radians);
// The vertices that lie inside `polygon` or on its edge, ascending.
[[nodiscard]] std::vector<std::uint32_t> verticesInside(const TinSurface& surface,
                                                        const Polyline2& polygon);

// A vertex an edit holds: on the rim of the surface, or at an end of a
// constrained (breakline or boundary) edge. One flag per vertex.
[[nodiscard]] std::vector<std::uint8_t> heldVertices(const TinSurface& surface);

// ---- deleting -------------------------------------------------------------------------

// The surface without the given triangles (repeats ignored). Fails with
//   InvalidArgument  an index out of range, or none given;
//   InvalidState     every triangle would go.
[[nodiscard]] katana::core::Result<TinEditResult>
deleteTriangles(const TinSurface& surface, std::span<const std::uint32_t> triangles);

// How deletePoints treats a vertex on a breakline or a boundary edge, which
// it never deletes (the constrained edge would go with it).
enum class HeldPointRule {
    Refuse, // fail, naming the vertex: the one vertex a person pointed at
    Skip,   // leave it, and count it (TinEditReport::pointsHeld): a polygon's worth
};

// The surface without the given vertices: the triangles around them are
// re-triangulated without them, inside the ring of edges around them. A
// vertex on the rim may be deleted; the gap it leaves is filled only as far
// as the old rim allows, so the footprint never grows. Fails with
//   InvalidArgument  an index out of range, or none given;
//   InvalidGeometry  a vertex on a constrained edge under HeldPointRule::Refuse;
//   InvalidState     every triangle would go;
//   TriangulationFailure  from the kernel.
[[nodiscard]] katana::core::Result<TinEditResult>
deletePoints(const TinSurface& surface, std::span<const std::uint32_t> vertices,
             HeldPointRule held = HeldPointRule::Refuse);

// ---- changing the triangulation --------------------------------------------------------

// The vertex moved in plan to `target`, keeping its level: the triangles round
// the old position and round the new one are rebuilt as ONE patch (the vertex
// taken out and a point put in, by the one constrained Delaunay kernel, inside
// the ring of edges round both), so the result is a valid surface whatever the
// distance moved - a vertex taken across a neighbouring edge is not dragged
// through it, the triangles that would have folded are not there to fold - and
// the rest of the surface is the same triangles. A target within
// tolerance::kGeometric of the vertex's own position is no move: the surface as
// it was. Fails with
//   InvalidArgument  a vertex out of range or in no triangle, a target that is
//                    not finite or is beyond kMostMagnitude;
//   InvalidGeometry  the vertex is at an end of a constrained edge (a breakline
//                    or boundary: DELETE BREAKLINE first), or on the rim or
//                    the edge of a hole (the footprint is not moved by a vertex;
//                    BOUNDARY and HOLE reshape it), the target is off the
//                    surface (ADD POINT extends it) or within kGeometric of
//                    another vertex;
//   TriangulationFailure  from the kernel.
// The report counts the vertex as moved (one) and, since the vertex made is a
// new one, as one added and one removed.
[[nodiscard]] katana::core::Result<TinEditResult>
movePoint(const TinSurface& surface, std::uint32_t vertex, const Point2& target);

// The edge swapped for the other diagonal of the quadrilateral its two
// triangles make. Fails with
//   InvalidArgument  a triangle or edge out of range;
//   InvalidGeometry  the edge is on the rim (no triangle across it), the
//                    edge is constrained (a breakline or boundary), or the
//                    quadrilateral is not strictly convex, which would fold
//                    the surface over itself.
[[nodiscard]] katana::core::Result<TinEditResult>
flipEdge(const TinSurface& surface, std::uint32_t triangle, std::uint32_t edge);

// The surface with the points added as vertices. A point on the surface is
// inserted where it lies and the triangles whose circumcircle it falls in
// are re-triangulated around it (as an incremental Delaunay insertion would,
// stopping at constrained edges); a point on a breakline splits it and keeps
// its own level, as buildTin does. A point at an existing vertex (within
// tolerance::kGeometric) gives that vertex its level. A point off the
// surface - outside it, or in a hole - extends it by the triangles that join
// it to the rim edges it sees (docs/surface_editing.md, "A point off the
// surface"). Fails with
//   InvalidArgument  no points, or a non-finite coordinate;
//   InvalidGeometry  two points at one position with levels that differ by
//                    more than tolerance::kCoordinate;
//   TriangulationFailure  from the kernel.
[[nodiscard]] katana::core::Result<TinEditResult> addPoints(const TinSurface& surface,
                                                            std::span<const Point3> points);

// One vertex of a breakline being added: where, and its level - none to take
// the surface's there (draped).
struct BreaklineVertex {
    Point2 position;
    std::optional<double> z;
};

// The line made constrained edges of the surface. A vertex with a level is a
// 3D point the surface is pulled to (an existing vertex there takes it); one
// without takes the surface's level. A stretch between two vertices that both
// lack a level follows the surface: a vertex is made wherever it crosses an
// edge, at the edge's level, so the surface keeps its shape and only gains
// the edges. A stretch with a level at either end is straight in 3D. Where
// the line crosses an existing breakline it meets it at that breakline's
// level. A vertex of the surface lying exactly on a 3D stretch takes the
// stretch's level, so the line is straight end to end (one on an existing
// breakline keeps its level, as where they cross). A vertex of the line that
// is within tolerance::kGeometric of an existing edge is on it: the edge is
// split there. A position the line visits twice has the level either visit
// gives it. Fails with
//   InvalidArgument  fewer than two vertices, a non-finite coordinate or level,
//                    one beyond kMostMagnitude;
//   InvalidGeometry  a vertex off the surface, the line leaving the surface
//                    (across the rim or a hole), the line crossing itself at
//                    two levels, two levels at one position, an edge passing
//                    so close to the end of another that the crossing cannot
//                    be placed on it, or no length at all;
//   TriangulationFailure  from the kernel.
[[nodiscard]] katana::core::Result<TinEditResult>
addBreakline(const TinSurface& surface, std::span<const BreaklineVertex> vertices, bool closed);

// The constrained edges that lie along any of `lines` made ordinary edges, so
// a breakline the surface was given (or a seam) is no longer one: the triangles
// and the vertices are exactly as they were, only the flags go, on both sides
// of each edge. An edge lies along a line when both its ends are within
// tolerance::kGeometric of ONE segment of it (a line's closing segment
// included when it is closed), so a line over part of a breakline clears the
// edges of that part and no other, and a vertex of the line need not be a vertex
// of the surface. The levels of the line are not asked. An edge on the rim is
// never cleared - a constrained rim edge is the surface's boundary, which
// BOUNDARY and HOLE make - and is counted in `edgesHeld`. A vertex whose every
// constrained edge was cleared can then be deleted (deletePoints). No edge
// along the lines is no change: the surface as it was. Fails with
//   InvalidArgument  no line, a line of fewer than two vertices, a coordinate
//                    that is not finite or is beyond kMostMagnitude;
//   InvalidGeometry  lines whose vertices all coincide.
[[nodiscard]] katana::core::Result<TinEditResult>
deleteBreaklines(const TinSurface& surface, std::span<const Polyline2> lines);

// One line of an edit that adds several (BreaklineVertex's rules).
struct BreaklineInput {
    std::vector<BreaklineVertex> vertices;
    bool closed = false;
};
// Several lines made constrained edges in ONE patch, so lines of the same
// edit that cross are checked against each other as the two halves of one
// line are: 3D lines crossing at different levels are refused (the answer
// does not depend on the order they are named), and a draped line meets a 3D
// one at its level. Refusals as addBreakline's, for any of the lines.
[[nodiscard]] katana::core::Result<TinEditResult>
addBreaklines(const TinSurface& surface, std::span<const BreaklineInput> lines);

// The part of the surface inside `boundary` (an outer clip), or outside
// `hole`. The polygon's edges become the new rim, exactly: triangles they
// cross are cut along them, the new vertices lying on the old surface, so the
// surface inside keeps its shape; the new rim edges are constrained, as
// buildTin's boundary edges are. Either orientation. Fails with
//   InvalidArgument  a non-finite coordinate;
//   InvalidGeometry  fewer than 3 distinct vertices, no area, or a polygon
//                    that touches or crosses itself (a corner it visits
//                    twice, or one on another edge, included);
//   InvalidState     nothing of the surface would be left;
//   TriangulationFailure  from the kernel.
// A polygon that changes nothing (a boundary around the whole surface, a
// hole beside it) returns the surface as it was.
[[nodiscard]] katana::core::Result<TinEditResult> clipToBoundary(const TinSurface& surface,
                                                                 const Polyline2& boundary);
[[nodiscard]] katana::core::Result<TinEditResult> cutHole(const TinSurface& surface,
                                                          const Polyline2& hole);

// How cutHoles went about it: for a test and a benchmark to see that disjoint
// footprints shared a patch, not for a front end.
struct HoleCutStats {
    // Whole-surface rebuilds made (copies, finishSurface, TinSurface::create):
    // one for every batch of footprints that could be cut together.
    std::size_t patches = 0;
};

// cutHole of each polygon in turn, each on the surface the one before left:
// the surface, and a report that is the first step's "before", the last's
// "after" and the counts of all summed (largestChange the greatest). The
// same surface as that loop gives - the same triangles, the same vertices,
// the same constrained edges, to within the numbering of the vertices the
// edit made - but footprints that can be cut together are cut in ONE patch,
// so 100 buildings cost one whole-surface rebuild (about a third of a second
// on 400 000 triangles), not 100.
//
// Footprints share a patch when each is separate from every other in the
// batch - further than tolerance::kGeometric from it everywhere, neither
// inside the other - and no triangle of the surface is reached by two of
// their edges. Anything else (overlapping, touching, nested, identical
// footprints, two in one triangle) is cut in a patch of its own, in the
// order the polygons are given, as it always was. A refusal is the loop's
// refusal: when a batch fails it is cut again one polygon at a time and the
// first refusal is the answer. Fails with
//   InvalidArgument  no polygon, or a coordinate that is not finite or is
//                    beyond kMostMagnitude;
//   InvalidGeometry  a polygon with fewer than 3 distinct vertices, no area,
//                    or that touches or crosses itself;
//   InvalidState     everything of the surface would be cut away;
//   TriangulationFailure  from the kernel.
// A polygon that changes nothing leaves the surface as it was, as cutHole's.
[[nodiscard]] katana::core::Result<TinEditResult>
cutHoles(const TinSurface& surface, std::span<const Polyline2> holes,
         HoleCutStats* stats = nullptr);

// ---- changing levels --------------------------------------------------------------------

enum class HeightOperation {
    Raise,    // z + value (a negative value lowers)
    SetLevel, // value
    Scale,    // base + value * (z - base)
};

struct HeightChange {
    HeightOperation operation = HeightOperation::Raise;
    double value = 0.0;
    // Scale only: the level that stays where it is. None: the lowest level
    // of the vertices changed.
    std::optional<double> base{};
};

// Every vertex's level changed, or only those inside `inside` (on its edge
// counts). The triangles are not touched, so a triangle with corners on
// both sides of the polygon slopes between them; a breakline along the
// polygon first (addBreakline) makes the change start exactly at it. Fails
// with InvalidArgument for a non-finite value or base, a level beyond
// kMostMagnitude, and for a polygon with no vertex of the surface inside it.
[[nodiscard]] katana::core::Result<TinEditResult>
changeHeights(const TinSurface& surface, const HeightChange& change,
              const std::optional<Polyline2>& inside = std::nullopt);

struct SmoothOptions {
    int passes = 3;
    // How far each pass moves a vertex towards the mean level of its
    // neighbours: 0 not at all, 1 all the way. In (0, 1].
    double weight = 0.5;
};
// Laplacian smoothing of the levels: each pass moves every free vertex
// `weight` of the way to the mean level of the vertices it shares an edge
// with, all at once (one pass reads only the levels before it, so the
// result does not depend on the order of the vertices). Vertices on the rim
// and on constrained edges are held. Plan positions never move. Fails with
// InvalidArgument for passes outside 1..1000, a weight outside (0, 1], and a
// polygon with no free vertex inside it.
[[nodiscard]] katana::core::Result<TinEditResult>
smoothSurface(const TinSurface& surface, const SmoothOptions& options,
              const std::optional<Polyline2>& inside = std::nullopt);

// The surface with the vertices removed that change it by no more than
// `tolerance` vertically, anywhere - not only at the vertex: each removal is
// measured as the largest difference between the surface before and after
// it over the triangles it replaces, and the differences of successive
// removals are added, so the bound holds against the ORIGINAL surface. The
// flattest vertices go first. Held vertices (rim, constrained edges) stay.
// A tolerance of 0 removes only the vertices whose removal changes nothing,
// to the length tolerance (tolerance::kGeometric) by which lengths are
// equal. Runs its first measurement on `pool` (null: TaskPool::shared());
// the result is the same at any thread count. Fails with InvalidArgument for
// a negative or non-finite tolerance and an empty surface.
[[nodiscard]] katana::core::Result<TinEditResult>
thinSurface(const TinSurface& surface, double tolerance, katana::core::TaskPool* pool = nullptr);

// ---- surfaces made of others ("mixed terrain") ---------------------------------------------

// The surface `top` laid into `base`: inside the top's footprint the top is
// the surface, everywhere else the base, joined along the top's rim by a
// clean seam - the base cut exactly along the rim, every seam vertex on the
// top's rim at the top's level, the seam edges constrained. The base's
// triangles the rim crosses are cut, not dropped whole (compare
// combineSurfaces). Where the top reaches past the base it is kept as it is.
// Fails with InvalidArgument for an empty top or base, and with
// TriangulationFailure from the kernel.
[[nodiscard]] katana::core::Result<TinEditResult> pasteSurface(const TinSurface& base,
                                                               const TinSurface& top);

// A copy raised by `dz` (changeHeights with Raise over the whole surface).
[[nodiscard]] katana::core::Result<TinEditResult> offsetSurface(const TinSurface& surface,
                                                                double dz);

// combineSurfaces (super_surface.hpp) - later members win, a lower member's
// triangle dropped whole where a later one covers any of it - with a report.
// Fails with InvalidArgument for fewer than two surfaces, or an empty one,
// and InvalidState when nothing is left.
[[nodiscard]] katana::core::Result<TinEditResult>
mergeSurfaces(const std::vector<const TinSurface*>& inRankOrder);

} // namespace katana::terrain
