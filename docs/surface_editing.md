# Editing surfaces, and surfaces made of others

The owner asked on 2026-10-15 for "more tools to terrain, editing terrain ...
mixed terrain". Until then a `TinSurface` (`include/katana/terrain/tin_surface.hpp`)
was immutable and nothing changed one: `buildTin` made it, and that was all.
This document records how a surface is edited now, on every front end, and
why it is done that way. The surfaces themselves - kept in the drawing,
saved, undone, styled - are `docs/terrain.md`'s.

## What there is

| Layer | Where | What |
|---|---|---|
| terrain | `include/katana/terrain/tin_edit.hpp`, `src/katana_terrain/tin_edit.cpp`, `tin_patch.*`, `tin_thin.cpp` | pure functions: a surface in, a NEW surface or a refusal out |
| cad | `include/katana/cad/terrain/surface_edit_verbs.hpp`, `src/katana_cad/terrain/surface_edit_verbs.cpp` | `SURFACE EDIT`, `MERGE`, `PASTE`, `CLIP`, `OFFSET`, one undo step each |
| cad | `src/katana_cad/tools/terrain_edit.cpp` | nine catalogue tools, category Terrain, each running the verb |
| qt | `src/katana_qt/terrain/surface_edit_ui.*`, `surface_edit_dialogs.*`, `src/katana_qt/tools/icons_terrain.cpp` | Terrain > Edit Surface, the Surface Edit toolbar, nine dialogs |

The verbs (`HELP SURFACE EDIT` prints them all):

```
SURFACE EDIT <name> DELETE TRIANGLE x,y | DELETE TRIANGLES INSIDE|OUTSIDE <polygon>
                    DELETE LONG <length> | DELETE THIN <degrees>
                    DELETE POINT x,y [reach=<m>] | DELETE POINTS INSIDE <polygon>
                    DELETE BREAKLINE <line>
                    MOVE POINT x,y TO x2,y2 [reach=<m>]
                    FLIP x,y
                    ADD POINT x,y,z [x,y,z ...] | ADD POINTS <points>
                    BREAKLINE <line>
                    BOUNDARY <polygon> | HOLE <polygon>
                    RAISE|LOWER <dz> | SETLEVEL <z> | SCALE <factor> [base=<z>]  [INSIDE <polygon>]
                    SMOOTH [passes=3] [weight=0.5] [INSIDE <polygon>]
                    THIN <tolerance>
SURFACE MERGE <a> <b> ... NAME <new>          (later surfaces win)
SURFACE PASTE <top> INTO <base> [NAME <new>]  (in place unless NAME)
SURFACE CLIP <name> <polygon> NAME <new>
SURFACE OFFSET <name> <dz> NAME <new>
```

`<polygon>`, `<line>` and `<points>` are the drawing's data, the three ways
every verb on drawing data takes it (`docs/cad.md`, "Scope and filter"): `#id
...`, coordinates typed in (`x,y[,z] ...`), or `FROM <scope> [WHERE k=v ...]`
through the one scope parser (`include/katana/cad/scope_verbs.hpp`), the
reply's first record saying what the scope took (`scope=drawing matched=3
polygons=2 skipped=1`). A `<polygon>` and a `<line>` are read by the one
reader of the drawing's polygons that the volumes use too
(`include/katana/cad/terrain/drawing_shapes.hpp`, which says what each kind
of entity gives): a polygon is a closed polyline or curve polyline, one
drawn back to its first vertex, a circle or a full ellipse; a line a line,
polyline or curve polyline. Where the two readers had differed, the shared
rule is the one that takes more (`docs/volumes.md`, "Inside a polygon"):
for these verbs that made `FROM` with no scope word the selection, as the
scope grammar says of no scope word, where it was refused
(`SurfaceEditVerbs.FromWithNoScopeWordIsTheSelectionAsTheScopeGrammarHasIt`),
and a polygon typed back to its first corner that corner once
(`DrawingShapes.TypedCornersMakeAPolygonWithAndWithoutHeights`). The
words of the replies and refusals stay this family's own. `PREVIEW`,
anywhere after the surface's name, works the edit out and changes nothing -
except where the grammar wants a value: a surface called `Preview` (the first
surface of every verb, a paste's base after `INTO`, a merge's members), a
`NAME preview`, a layer after `LAYERS` are names, not the flag (the words
arrive with their quotes off, so only the position can tell, as it does for
`CommandInterpreter::modify`'s layer called "preview"). The reply is one
`surface` record:

```
surface name=ground operation=hole triangles_before=200 triangles_after=188 vertices_before=121
    vertices_after=125 points_added=4 points_removed=0 relevelled=0 moved=0 held=0 crossings=4
    [largest_change=] changed=yes [preview=yes]
```

LOWER is RAISE with the sign turned, added for the window's Raise / Lower
dialog and a person who would otherwise type `RAISE -0.3`.

**A scope that takes nothing is an answer** (`docs/cad.md`, "Scope and
filter"): `HOLE FROM DRAWING` over a drawing with no polygon
replies with the scope record (`scope=drawing matched=1 polygons=0
skipped=1`) and a surface record that says `changed=no ran=no
reason="the scope took no polygon"`, makes no undo step, and the next line of
a script or an agent's batch still runs. It was an `InvalidArgument`, which
stopped `katana_cli` and marked `katana_mcp`'s later commands not run, where
the volume verbs answered `ran=no`. Two cases are the reason a scope that
took nothing is not simply an empty list handed on: `DELETE TRIANGLES
OUTSIDE` of no polygon would be every triangle, and `RAISE 1 INSIDE FROM ...`
of no polygon would be the whole surface; both do nothing. `SURFACE CLIP` of
such a scope makes no surface and says so. What stays refused: a `#id` that is
no polygon or line, typed corners that are too few (counted after a polygon's
closing repeat of its first corner: `0,0 10,0 0,0` is `corners=2 typed=3`),
two polygons where `BOUNDARY` takes one.

## One way to change a triangulation

Every edit that changes triangles - adding and deleting points, breaklines,
boundaries, holes, a paste - goes through one mechanism,
`detail::rebuildPatch` (`src/katana_terrain/tin_patch.hpp`). An edit names a
REGION, the triangles it has to rebuild, and what goes into it: points, new
breakline segments, the edges of a cut. The region's vertices and the new
points are triangulated by THE constrained Delaunay kernel
(`src/katana_terrain/cdt_backend.hpp`) with the ring of edges round the
region as constraints, so the new triangles meet the untouched ones edge for
edge, and only the region changes. The rest of the surface is the same
triangles, vertex for vertex.

- **Which new triangles are inside the region is decided by topology, not
  by a point-in-polygon test.** A triangle on the left of a ring edge, as the
  region's own triangle ran it, is inside; inside spreads across every edge
  that is not a ring edge. The kernel splits a ring edge when something lies
  exactly on it, and the vertex list it reports along each constraint gives
  the sub-edges their direction, so a split edge still classifies.
- **A ring edge something splits grows the region.** A vertex of the surface
  lying exactly on a ring edge (a degenerate sliver in an imported surface),
  or a new line leaving the region, would split the edge the untouched
  neighbour shares; the neighbour then joins the region and the attempt is
  made again. A split RIM edge (no neighbour) needs no growth: a point on the
  rim splits it, which is what it should do. After 32 rounds the whole
  surface is the region, which is always correct, only slower.
- **The region is never bigger than the edit needs.** Added points take the
  triangles whose circumcircle holds them, reached across unconstrained edges
  (`detail::markInsertionCavity`, the Bowyer-Watson cavity) - so a point added
  to a Delaunay surface leaves it Delaunay; a line or a polygon takes the
  triangles it passes through (`detail::TriangleGrid::markAlong`, a uniform
  grid of the triangles, conservative by a rounding margin).
- **Exact where it decides.** Which side of an edge a point lies on, whether
  a point is in a triangle, on an edge or at a vertex (`detail::placeOf`), and
  whether two triangles make a convex quadrilateral (FLIP) are decided with
  the kernel's own exact predicate, `detail::orientation`, added to
  `cdt_backend` for it. Coincidence is `tolerance::kGeometric`, the rule
  buildTin merges by - and it is the rule for a point and an EDGE as well as
  a point and a vertex:
  - **A point within `kGeometric` of an edge it is not an end of is on it.**
    A point decimal arithmetic puts on a line (14.3, 4.29 on y = 0.3 x) is a
    few ulps off it, on one side or the other by luck of the last bit. The
    kernel decides only exactly, so such a point did not split the edge, and a
    new edge that ended at it crossed the old one a few ulps from its own end.
    CGAL's `Exact_predicates_tag` moves a crossing it cannot place to the
    nearer END of the crossed edge (`Constrained_triangulation_2.h`,
    `insert_intersection`, CGAL 6.2), tens of metres away when the crossed edge
    is a long breakline, and the constraint is reported running through that
    vertex. The result was a ramp where a level line was typed, a sliver of
    no width beside a breakline, a rectangle whose edge lay along a breakline
    refused ("touches or crosses itself", at the far end of the breakline) one
    time in four. `tryPatch` now bends the edge through the point before the
    kernel is asked (`pointsOnConstraints`: a/b becomes a/p and p/b, the point
    found by a grid of the edges' boxes, the nearest edge taking it): the
    junction is exact whatever the last bit was, a T-junction splits the old
    constraint as an exact one always did, and the point keeps the coordinates
    it was typed with. *Rejected:* moving the point onto the edge - no double is
    on a line of rational slope, so the point would still be a few ulps off,
    and the typed position would no longer be the vertex's.
  - **What is still left is refused, not trusted.** After the triangulation
    every vertex reported along a constraint must lie within the rounding
    margin of it; a surface vertex that close to a new edge, with the crossing
    snapped to the wrong end, is `InvalidGeometry` "an edge of the edit passes
    so close to the end of another that the kernel could not place the crossing
    on it: move the end onto the edge or clear of it".
  - **Within `kGeometric` beyond a RIM edge is on the surface.**
    `TinSurface::locate` and `elevationAt` count a position within
    `kGeometric` of an edge as on the surface, and `placeOf` said Outside for
    one that was beyond a rim edge by the exact test, so there were two
    definitions of "on the surface": a breakline ending on a boundary just cut
    was refused "off the surface" about half the time, and ADD POINT at such a
    position joined it to the rim with a fin of no width that rose to its level
    (a 10 m2 wall on a 100 m2 square). It is now an Edge of the rim triangle and
    goes through the patch like any point on the rim.
- **The result is canonical**, as buildTin's is (`detail::finishSurface`):
  vertices left without a triangle dropped, the rest in order, then the new
  ones; triangles rotated to start at their smallest vertex and sorted;
  constraint flags made to agree across every shared edge; validated by
  `TinSurface::create`. The same edit of the same surface gives the same bits.
- *Rejected:* re-triangulating the whole surface for each edit. It is simpler
  and it is wrong: an imported surface (a LandXML TIN, another program's
  design) is rarely Delaunay, and a global CDT would change its triangles
  everywhere, far from the edit. Constraining every old edge to stop that
  makes a 400 000-triangle surface a 600 000-constraint problem for one
  added point.
- *Rejected:* a second triangulator (ear clipping a star polygon, a fan).
  The kernel is exact and already in the module; two triangulators would
  come to disagree about the same polygon.

### Levels of new vertices

The kernel makes a vertex where two constraints cross. Its level comes from
the constraint whose level holds there, strongest first:

1. a paste's seam: the top surface's rim (`PatchRequest::cutHasLevels`);
2. an EXISTING constrained edge - a breakline or boundary of the surface;
3. a new 3D line segment (both ends with levels, or one given and one draped);
4. an unconstrained edge of the surface held for its shape;
5. otherwise the surface's level at the point (a draped line or polygon).

So a new breakline meets an old one at the old one's level, and bends there
rather than bending it - constrained edges are kept, which is the promise.
Two new 3D lines crossing at different levels are refused, as buildTin's
`CrossingBreaklinePolicy::ErrorOnConflict` refuses breaklines (`SURFACE FROM
DRAWING` asks for `Average` instead: it makes a surface from linework, which an
edit of a surface keeping its constrained edges does not). That holds for the
lines of ONE edit as for one line crossing itself: `BREAKLINE <line> <line>`
and `BREAKLINE FROM <scope>` put every line in one patch
(`tin::addBreaklines`), where they were added one after the other and the
earlier line, already a constrained edge, won the crossing - which level a
person got depended on the order the lines were named in. A POINT given
exactly on an old breakline keeps its own level, as a survey point on a
breakline does in buildTin.

A vertex of the surface that lies exactly on a new 3D line takes the line's
level: "straight in 3D" holds end to end, not only between the vertices the
kernel happened to make (a breakline typed along a row, column or diagonal of
a raster's lattice used to change its two ends and nothing between them, and
the same line moved 0.05 m off the row was straight). The exceptions are the
rule of the ranking again: a vertex on an EXISTING breakline keeps its level,
the line bending there; a vertex a point of the edit gave a level keeps it, and
a new line through it at another level is two levels at one place, refused as
two lines crossing are; where several new lines pass through one vertex they
must agree. These are `relevelled=` in the reply. A position a breakline
visits twice takes the level either visit gives it (the segments are judged
draped or not by the levels they then have), so `2,2 8,2 8,8 2,8 2,2,5` and
`2,2,5 8,2 8,8 2,8 2,2` are one surface; the level used to be the first
visit's, and silently none when the first was draped.

A stretch of a new breakline between two vertices that both lack a level
FOLLOWS the surface: the region's unconstrained edges are held as shape
constraints (`PatchRequest::keepShape`), a vertex is made at every crossing
at the old edge's level, and the surface keeps its shape exactly
(`TinEdit.ADrapedBreaklineKeepsTheShapeOfTheSurface`). A stretch with a level
at either end is straight in 3D, and the region is re-triangulated about it.
Boundaries, holes and the paste's cut always keep the shape: they cut
triangles, they do not reshape them.

### What every edit keeps

- **Constrained edges.** No edit removes a breakline or boundary edge it was
  not asked to (DELETE BREAKLINE is asked to, along a line, and only interior
  edges): DELETE POINT refuses a vertex on one
  (`HeldPointRule::Refuse`), DELETE POINTS INSIDE leaves such vertices and
  counts them (`held=`), SMOOTH and THIN hold them, FLIP refuses a
  constrained edge.
- **The footprint.** Deleting a rim vertex fills the gap only as far as the
  old rim allows: a new triangle that would reach past the rim edges at the
  deleted vertex (`segmentMeetsInterior`, exact) is not kept, so the
  footprint never grows (`TinEdit.DeletingACornerOfTheRimNeverGrowsTheFootprint`).
- **The surface given.** Every edit returns a new surface; the one handed in
  is never touched (`TinEdit.AnEditLeavesTheSurfaceItWasGivenAsItWas`), which
  is what lets the verb keep the old one for UNDO.

## Decided

- **A point off the surface extends it** by the triangles that join it to
  the rim edges it sees: the surface's rim vertices and the new points are
  triangulated with the rim edges as constraints, and the triangles outside
  the old footprint that have a new point as a corner are added
  (`detail::extendSurface`). A point in a hole joins the hole's rim the same
  way. Only the rim takes part, so the cost follows the rim, not the surface.
  The kernel triangulates the convex hull, so outside the footprint it also
  makes triangles of three rim vertices, and those are kept when the new
  triangles shut them in: the ears of a hole that a point in it did not reach
  (a point at the centre of a square hole filled only a fan of it, 25 m2 of 100
  left open as two ears, because cocircular rim vertices let the kernel choose
  a diagonal across a corner), and the inside of a notch the new triangles
  bridge (a point beyond an L's notch walled off a hole that was not there). A
  piece of those that reaches the hull is open to the outside and is left (a
  concavity the point does not cover); one that borders no new triangle is a
  hole no point is in, and stays a hole. *Rejected:* refusing a point off the
  surface (the brief asked that it extend it); joining it to the whole convex
  hull (that fills every concavity of the footprint between it and the
  surface, including those open to the outside).
- **A coordinate or level beyond `terrain::kMostMagnitude` (1e9 m) is refused**
  where it is typed - ADD POINT, a breakline's vertex, a polygon's corner, a
  height change and the level it makes: `RAISE 1e308` was finite, and the plan
  area of the surface it made read `nan`. A UTM easting is at most 1e7; the
  limit leaves areas and volumes (sums of products of three such numbers)
  finite. One gate in the edit core, so the window's dialogs and an agent's
  line meet it alike.
- **DELETE BREAKLINE makes the breakline edges along a line ordinary edges**
  (`terrain::deleteBreaklines`). It clears the constrained flag on both sides
  of every edge that LIES ALONG the line - both its ends within
  `tolerance::kGeometric` of one segment of it, the closing segment of a closed
  line included - and nothing else: the vertices and the triangles are exactly
  as they were, in the same order, so the edit is a flag per edge and no
  re-triangulation, and it is one undo step like the others. A line over part
  of a breakline clears the edges of that part; a line along none clears
  nothing, which is `changed=no` and no step, not a refusal; the line's levels
  are not asked, and its vertices need not be vertices of the surface. A vertex
  whose every constrained edge was cleared is then an ordinary vertex and
  DELETE POINT takes it (a vertex at the end of an edge that stays is refused as
  before). The reply adds `edges_cleared=` and `edges_held=` to the record,
  after `crossings=`. *Edges on the rim are never cleared*: a constrained rim
  edge is the surface's boundary, which BOUNDARY, HOLE and CLIP make, not a
  breakline, and `edges_held=` counts those along the line so that a line over a
  boundary that cleared nothing says why. *Rejected:* clearing the rim as well
  (the verb is named for breaklines, and a rim made unconstrained by a line
  drawn along it would be a boundary lost without anyone asking for it);
  matching an edge by its midpoint or by the nearest segment (a line that cuts
  across the corner of a breakline is near both ends of nothing); merging
  collinear segments of the line first (a TIN edge never spans a vertex of the
  breakline it came from, since that vertex is a vertex of the surface); and a
  catalogue tool that picks one polyline - the Terrain menu's dialog takes the
  lines by the shared scope, which a click on one entity cannot say.
- **MOVE POINT moves a vertex in plan as ONE patch** (`terrain::movePoint`).
  `MOVE POINT x,y TO x2,y2` names the vertex as DELETE POINT does (the corner of
  the triangle under `x,y` nearest it, or with `reach=` the nearest vertex within
  it) and puts it at `x2,y2` at its own level: the target is `x,y` only, since a
  level on it would be a change of level, which RAISE and ADD POINT make. The
  triangles round the old position and round the new one are rebuilt together -
  the vertex taken out of the patch's region and a point put in, by the same
  kernel and inside the same ring of edges as every other edit - so there is no
  triangle left to fold: a vertex taken across a neighbouring edge, or two cells
  away, comes out as a valid surface of the same footprint, and the vertex
  count, the triangle count (a polygon of n corners round k points has n + 2k - 2
  triangles, whoever joins them) and every other vertex are as they were. It is
  one undo step, not a DELETE POINT and an ADD POINT, which would be two. The
  report counts the vertex made as one added and the one taken out as one
  removed, and `moved=1`. *Refused*, each with its cause: a vertex at an end of a
  constrained edge (a breakline or boundary: DELETE BREAKLINE first, or the
  triangles round it deleted), a vertex on the rim or on the edge of a hole, a
  target off the surface (ADD POINT extends it; a move does not), a target within
  `tolerance::kGeometric` of another vertex. A move of less than that tolerance
  is no move: the surface as it was, `changed=no`. The rim is refused before the
  breakline, since a boundary or hole makes its rim edges constrained and
  "clear the breakline" would send a person after an edge DELETE BREAKLINE never
  clears. *Rejected:* moving a rim vertex (it reshapes the footprint, which
  BOUNDARY and HOLE do on purpose and a drag should not, and the brief did not ask
  for it); dragging the vertex and keeping its neighbours (a vertex taken
  across the ring of its star folds the triangles round it, and the check for that
  is a second implementation of what the kernel decides exactly); the target's
  level from the surface there (the person moves a point, not its level).
- **A point at an existing vertex gives it its level**, and no triangle
  changes (`relevelled=`). That is what a surveyor means by "the ground is
  here, at this level". Two points at one position with levels more than
  `tolerance::kCoordinate` apart are refused, as buildTin refuses them.
- **DELETE TRIANGLES INSIDE/OUTSIDE goes by centroid**, as survey packages
  do; DELETE LONG and DELETE THIN take every triangle of the surface that
  qualifies, not only those on the rim. A person trimming the long slivers
  of a convex hull runs DELETE LONG repeatedly, or BOUNDARY once.
- **A height edit inside a polygon moves the vertices inside it and nothing
  else**: the triangles that cross the polygon slope between the changed and
  unchanged vertices. To make the change start exactly at the polygon, add
  the polygon as a breakline first - two steps a person can see, rather than
  an edit that silently re-triangulates.
- **SCALE's base, when not given, is the lowest level it changes.** Scaling
  about 0 would multiply a design at RL 100 into the sky.
- **SMOOTH is Laplacian and Jacobi**: every free vertex moves `weight` of the
  way to the mean level of its neighbours along an edge, all at once per
  pass, so the result does not depend on vertex order. Rim and constrained
  vertices are held; plan positions never move.
- **THIN's bound holds against the ORIGINAL surface, everywhere.** A
  removal's change is measured exactly - both surfaces are linear on each
  piece of their overlay, so the largest difference is at the removed vertex
  or where a new diagonal of its neighbour polygon crosses an old spoke - and
  each triangle carries the sum of the changes made under it; a vertex goes
  only when that sum is within the tolerance. The flattest go first (the
  first measurement runs on the TaskPool, the order then sequential, so the
  result is the same at any thread count:
  `TinEdit.ThinningGivesTheSameSurfaceAtAnyThreadCount`). A tolerance of 0
  removes what changes nothing, to `tolerance::kGeometric`, the length
  tolerance by which two lengths are equal (`math::lengthsEqual`); without it
  rounding of the order of 1e-16 would keep every coplanar point.
  *Rejected:* measuring a removal at the removed vertex alone, the usual
  decimation shortcut: a new diagonal can cross a ridge the vertex was on and
  move the surface by more than the tolerance somewhere else, and the errors
  of successive removals add.
- **PASTE cuts the base exactly along the top's rim**, gives every seam
  vertex the top's level, inserts the seam's vertices into the top's rim
  triangles (a fan from the opposite corner, or from the centroid when two
  or three sides are split - the centroid is on the top's plane, so the top
  keeps its shape) and makes the seam constrained on both sides. No gap, no
  overlap (`TinEditCombine.APastedPadReplacesTheGroundInsideItsFootprintWithACleanSeam`:
  the area is the ground's to 1e-6, every seam vertex on the pad's rim at
  the pad's level). Where the top reaches past the base it is kept as it is.
- **MERGE is `combineSurfaces`** (`include/katana/terrain/super_surface.hpp`):
  later members win, a lower member's triangle dropped whole where a later
  one covers any AREA of it, so its seam is ragged by up to a triangle. A
  member that only ABUTS a triangle - shares an edge or a corner, or overlaps
  it by less than `kGeometric`, the sliver rounding leaves between two clips of
  one surface - covers none of it, and the triangle stays: two tiles of one
  survey merge into both, where the lower tile was dropped whole (or, along
  the seam, a row of it). The test is the pair of triangles, not samples: a
  lower triangle and a higher one share an area unless an edge line of the one
  parts them (the separating axis theorem; for triangles the edge lines are all
  the axes), read with the coincidence tolerance, found through a grid of the
  higher member's triangles. The old test - the lower triangle's corners and
  middle in the higher surface, the higher vertices in the triangle, the
  boundary counting as inside - found most overlaps, all contacts, and missed a
  strip that crossed a triangle whose vertices lay ON its edges only because
  the boundary counted. *Rejected:* making MERGE successive pastes (the exact
  union, but it changes the documented "fast, never refusing" MERGE, which is
  the archive import's rule). It is kept for what it is - the rule the archive
  import's combined surfaces use, fast, never refusing - and PASTE is the clean
  seam. The merged result is compacted (combineSurfaces keeps the vertices of
  dropped triangles).
- **HOLE of several footprints is cut in as few patches as are correct**
  (`terrain::cutHoles`; `HOLE FROM <scope>`, `HOLE #a #b ...`). A patch costs
  the whole surface - the stars of its vertices, the copies, `finishSurface`'s
  sort, `TinSurface::create`'s adjacency and locator, about a third of a second
  on 400 000 triangles - and a hole is cut where a polygon is, not where the
  surface is, so a hundred buildings cost a hundred rebuilds when cut one after
  the other. The footprints are taken in the order given and gathered into a
  batch while each one is **separate** from every one already in it - the
  outlines nowhere within `tolerance::kGeometric` of each other (boxes first,
  then only the edges that reach the other's box, decided with the exact
  predicate the kernel uses), and neither inside the other - **and** reaches
  no triangle one of them reaches (`detail::markReachedByPoint` and
  `TriangleGrid::markAlong`, the rule `rebuildPatch` grows its region by). The
  first footprint that is not, ends the batch: the batch is cut in one patch,
  and the footprint begins the next one on the surface the batch left. So
  overlapping, touching, nested and identical footprints, and two in one
  triangle, are cut one patch each, in the order given, exactly as the loop of
  `cutHole` cut them. Why both tests: separate outlines are what makes the cut
  correct - a patch decides which side of the cut a piece is on from the cut
  edges that bound it, and a piece between two footprints that overlap would be
  taken for outside both - while a shared triangle is what makes it the loop's
  surface: cut by two outlines at once, its pieces are triangulated together,
  where the loop triangulated the first's pieces and then cut those. The result
  is the loop's surface: the same triangles, vertices and constrained edges, in
  a different numbering of the vertices the cut made (`HoleCutStats::patches`
  counts the rebuilds; `TinEdit.HolesApartFromEachOtherAreCutInOnePatchAndGiveWhatCuttingThemOneByOneGives`
  and its neighbours compare the two runs vertex for vertex and triangle for
  triangle, the report field for field). A batch the kernel refuses is cut again one footprint at a time, so
  the refusal is the loop's, with its words. *Rejected:* one patch for every
  footprint whatever they are (wrong for the cases above); the union of the
  polygons first (the module has no polygon clipper, and a second one beside
  the kernel would come to disagree with it about the same polygons); cutting
  the footprints' patches on the `TaskPool` (the cost is the global rebuild
  each makes, not the footprint, and patches of one surface cannot be joined
  without a rebuild of the whole). Nothing here runs in parallel, so the
  result cannot depend on the thread count.
- **A record prints any finite number, in full.** The edits refuse a level past
  `kMostMagnitude`, but a project, an archive or a LandXML file may hold one,
  and the SURFACE records that name the surface (`bounds=`, `zmin=`, `zmax=`)
  printed `nan` for a FINITE level whose digits and three decimals filled more
  than the 64 characters their formatter had (1e61, a level of 61 digits,
  where 1e60 filled it to the last). The formatter was written twice, in
  `src/katana_cad/terrain/surface_verbs.cpp` and in
  `src/katana_app/geo/replies.cpp`; it is now one, `core::formatFixed`
  (`include/katana/core/text.hpp`), whose buffer is the longest fixed text of
  a double (DBL_MAX, 309 digits) and which the two `fixed3` call. The text of
  an ordinary value is unchanged byte for byte; a value that rounds to nothing
  has no sign ("-0.0004" is "0.000", as before; "-0.0005" is "-0.001": the
  double nearest it is a hair above the tie, and the exact value decides,
  which is also how `to_chars` rounds); non-finite values are what `to_chars`
  writes, "nan", "inf", "-inf". *Rejected:* printing the value in general
  notation when it is too large for a buffer (a record's `zmax=1e+61` would be
  a number of another shape than every other, and a reader that parses fixed
  text would take it for a bound it is not).
- **PASTE says why when the top's rim cannot be kept apart.** The seam is the
  top's rim, so its vertices are points of the patch, and the patch takes two
  within `tolerance::kGeometric` for one (the rule buildTin merges by): a top
  with two rim vertices that close - an imported surface's sliver - came out
  with a triangle naming one vertex twice, which `TinSurface::create` refused
  as "triangle names a vertex twice" with the number of a triangle of the
  result. `pasteSurface` now looks at the top's triangles after the patch, where
  two corners that are different vertices of the top have become one of the
  result, and refuses with the cause: InvalidGeometry, "two vertices of the top
  surface's rim land on one vertex of the result: they are closer to each other,
  or to one vertex of the base, than the coincidence tolerance", the vertices
  and where they are. The test is of the TRIANGLE and not of the pair: a top
  whose rim touches itself at a vertex that is duplicated (two triangles meeting
  at a point, each with a vertex of its own there) merges the pair into one
  vertex of a valid surface, and is pasted as it always was
  (`TinEditCombine.ATopWhoseRimTouchesItselfAtADuplicatedVertexIsStillPasted`).
  *Rejected:* looking at the rim's vertices before the patch, which would
  refuse that pasteable top as well, and merging the pair silently, which makes
  a triangle of no area.
- **CLIP is BOUNDARY on a copy, OFFSET is RAISE on a copy**: one function
  each, the verbs name them for the person making a new surface.
- **The active surface is the program's, not the drawing's.** Which surface
  a person is editing is not part of the drawing, so it is not saved or
  undone: `cad::surfaceEditTarget`, one for the program as the edit tools'
  `EditDefaults` are (`docs/tools.md`). A tool acts on it when the drawing
  has it, else on the first visible surface, else the first
  (`cad::activeEditSurface`).

## In the window

Terrain > **Edit Surface** (`terrainEditMenu`) holds the nine tools - Delete
Triangle, Flip Edge, Add Point, Delete Point, Move Point, Delete Triangles
Inside, Add Breakline, Boundary, Hole - and below them Raise, Lower or Set Level...
(`surfaceHeightsDialog`), Smooth... (`surfaceSmoothDialog`), Thin...
(`surfaceThinDialog`), Delete Long Triangles... (`surfaceDeleteLongDialog`),
and the four that take the drawing's data by a scope, which a click on one
entity cannot say: Delete Points Inside Polygons... (`surfaceDeletePointsDialog`,
`DELETE POINTS INSIDE FROM <scope>`), Delete Triangles by Polygons...
(`surfaceDeleteTrianglesDialog`, `DELETE TRIANGLES INSIDE|OUTSIDE FROM <scope>`:
the union of any number of polygons, which the one-pick tool cannot make
- OUTSIDE of the first of two already deletes what the second would keep),
Delete Breaklines Along Lines... (`surfaceDeleteBreaklineDialog`, menu item
`terrainSurfaceDeleteBreakline`, `DELETE BREAKLINE FROM <scope>`: the lines of
a layer, the selection or a view, with the scope's filter) and
Add Points from the Drawing... (`surfaceAddPointsDialog`, `ADD POINTS FROM
<scope>`). Each has the shared scope widget with no tick box in front of it
(the scope is what the dialog is about), the surface, the line it will run,
Preview and Run; the lines are made by `surfaceLevelsLine`, tested without a
window.
Terrain > **Combine Surfaces...** (`surfaceCombineDialog`) makes a merge, a
paste, a clip or an offset. The **Surface Edit** toolbar (`SurfaceEditToolBar`,
on the second row after Terrain) has them all, and at its head the active
surface (`surfaceEditActive`).

- **The tools are catalogue tools** (category Terrain, group Surface Edit,
  aliases `TINDELTRI`, `TINFLIP`, `TINADDPT`, `TINDELPT`, `TINMOVPT`,
  `TINDELIN`, `TINBRK`, `TINBOUNDARY`, `TINHOLE`), so the menu and toolbar are
  filled by
  `tools::fillToolMenus`, the window handing it this submenu and toolbar as
  the Terrain category's. Each turns what it was told into the SURFACE EDIT
  line a person would type, plans it with the verb's own `planSurfaceEdit`
  and returns `Document::surfacesCommand`'s command - one code path with the
  verb, one undo step - and reports the line and the reply. Each starts
  again after an edit, as Point does, so a run of flips is a run of clicks.
- **How a person picks the surface**: the toolbar's list, or a tool's own S
  option (S, then the name), which also sets the list: `cad::setSurfaceEditTarget`
  tells the observers registered with `cad::addSurfaceEditTargetObserver`, and
  the toolbar's list is one (`SurfaceEditUi::syncActive`). The list is wired to
  `currentIndexChanged`, not `activated`, so a choice made by the program (the
  headless driver's fill) points the tools too. A tool with no surface in the
  drawing says so in its prompt and refuses every click. The dialogs open on the
  first surface, not the target.
- **What a run of clicks keeps** is held by the catalogue entry that makes the
  tool (the host remakes the tool after each edit): the surface, and Delete
  Triangles Inside's Outside (O), which used to go back to inside after the
  first pick of a run.
- **Add Point** takes the click, then the level typed; Enter takes the
  surface's own level there (a vertex on the surface, no change of shape). A
  typed `x,y,z` adds the point in one go. **Delete Point** takes the vertex
  within the view's vertex reach (`vertexReach`, the snap aperture), passed
  to the verb as `reach=`: the corner of the triangle under the click when it
  is within reach, else the nearest vertex within reach wherever it is, so a
  click a hair outside a rim corner finds the corner (it was refused as "no
  vertex within reach" while one was 0.07 m away); a click off the surface with
  nothing within reach says it is off the surface. **Delete Triangles Inside** takes O for outside.
  **Move Point** takes two clicks: the vertex (the same reach; one the verb
  would refuse - a rim vertex, the end of a breakline or boundary edge - is
  refused at this click, before a target is asked for) and then where it goes.
  Between them the prompt says which vertex is held (`lastPoint`, for the
  rubber band), the preview marks the vertex where it is (Removed) and where
  the cursor would put it (Added), and says in red that a cursor off the surface
  is refused; a target the verb refuses (off the surface, on another vertex)
  leaves the vertex held for another click, and Undo or Enter lets it go. The
  line is `SURFACE EDIT <s> MOVE POINT <vertex's own coordinates> TO x,y`, so no
  reach decides which vertex it is on the second click.
- **The previews** mark what a click would take - the triangle (Removed),
  the edge (Target), the vertex with its level - and say in red why a click
  would be refused: off the surface, an edge on the rim, a breakline. Delete
  Point asks the core's own rule (`terrain::onConstrainedEdge`, the test
  `deletePoints` refuses by) and says "a breakline or boundary vertex is kept,
  not deleted" in red before the click, as the click would be refused; it was
  offered with `refused=no`.
- **The dialogs write lines** and hand them to the window's one executor
  (`MainWindow::runVerbLine`), the line shown in `<prefix>Command` as it will
  run, Preview running it with PREVIEW. "Only inside a polygon" and Combine's
  clip take the polygon through the shared scope widget
  (`src/katana_qt/customisation/scope_filter_widget.hpp`), written as `FROM
  <scope>`. The lines are made by pure functions (`surfaceLevelsLine`,
  `surfaceCombineLine`), tested without a window.
- **The dialogs follow the Document.** They are non-modal and kept, and they
  used to read the drawing's surfaces only when constructed, shown or after
  their own Run: an UNDO that removed the surface they offered left Run enabled
  on a line the verb refused (NotFound), and a surface made by a typed line
  could not be chosen until the dialog was closed and shown again (showing it
  from the menu while it was open sends no show event). Each now owns a
  `DocumentWatcher` (`src/katana_qt/customisation/document_watcher.hpp`), the
  last member so it dies first, which says from the event loop that the model
  changed - on every execute, undo and redo, and on New and Open - and the
  lists are read again, keeping what is chosen where it is still there
  (`SurfaceLevelsDialog.TheSurfaceListFollowsTheDrawingWhileTheDialogStaysOpen`,
  `SurfaceCombineDialog.TheListsFollowTheDrawingWhileTheDialogStaysOpen`).
  Rejected: a Document listener of each dialog's own on each kind of change
  (Surfaces, Replaced ...), reloading inside the notification: the watcher
  already defers and coalesces, as a table rebuilt inside the command that
  notified once crashed the layer panel (docs/cad.md).
- **Icons**: `Icon::SurfaceEdit`, `SurfaceLevels`, `SurfaceSmooth`,
  `SurfaceThin`, `SurfaceDeleteLong`, `SurfaceCombine` (`src/katana_qt/icons.cpp`)
  and the tools' own (`src/katana_qt/tools/icons_terrain.cpp`), each the same
  small TIN with what the tool does in the accent.
- `Document::surfacesCommand` is const: the command it builds captures the
  Document it will change and changes nothing until the host executes it, so a
  tool holding a const Document makes one without a cast. (The one
  `const_cast` is inside it, `surface_document.cpp`, safe because the command
  only ever runs through the non-const `Document::execute` of the Document that
  made it. The tool carried a cast of its own and a comment that gave a false
  reason for it.)
- **The tools read a pick with the verbs' reader.** Whether an entity is a
  polygon or a line is `cad::shapeOf` (`drawing_shapes.hpp`), asked by the tool
  and by the verb: the tool had a copy of the rule that refused a curve
  polyline drawn back to its start without the closed flag, which the verb
  cuts (`PLINE 20,20 60,20 ARC 60,60 LINE 20,60 20,20`).

## katana_cli and katana_mcp

The verbs are the interpreter's, so the window's command line, `katana_cli`
and `katana_mcp` (`katana_run_commands`) run the same code; the
geoprocessing executor keeps SURFACE LIST, INFO, REMOVE, FROM and EXPORT and
hands every other SURFACE line to the interpreter (`docs/terrain.md`). No MCP
tool of its own: the replies are records, and what an agent needs to choose
a surface is `katana_terrain_list`'s, which reads the edited surface
(`McpServer.ASurfaceEditThroughTheCommandToolIsWhatTheTerrainListReads`).

## Tests

- `tests/terrain/test_tin_edit.cpp`, `test_tin_edit_combine.cpp`,
  `test_super_surface.cpp`: every edit on hand-made surfaces - a square of two triangles flipped
  (`TinEdit.FlippingTheDiagonalOfASquareOfTwoTrianglesGivesTheOtherDiagonal`),
  a point in the middle giving four triangles at the worked levels, a hole of
  known area, raise by 1 adding exactly 1, smoothing holding breakline
  vertices, thinning at 0 removing exactly the two points whose neighbours
  are all on the plane, the bound of thinning checked on a dense sample, the
  paste's seam on the top's rim - and every refusal. The fixes of the review of
  2026-10-15 each have their hand-worked case: an end within rounding of an old
  breakline on either side (the old line y = 0.3 x, the point's cross product
  worked in exact rationals, the constrained edges meeting at it counted),
  a decimal parallelogram along a breakline cut and pasted (area 213.64), a 3D
  line through the vertices (k,k) of a unit grid at 4.5 + k, two lines crossing
  at 12 and 8 refused in either order, the rim point 1e-12 beyond with the 3D
  area worked as sqrt(2900) + sqrt(10400) / 2, figures of eight and a pinched
  polygon refused, a point in a square hole (area back to 400) and beyond an L's
  notch (500), two abutting squares merged (200) and a pyramid with a top that
  touches two faces at a point (400).
- Breaklines: `TinEdit.DeletingABreaklineMakesItsEdgesOrdinaryAndChangesNoTriangleNorVertex`
  (six lattice edges of a breakline along a row, cleared by a line that runs
  past both ends, the triangles and vertices equal, the middle vertex refused
  by DELETE POINT before and taken after), the same for a line over half of it
  (three edges, and the vertex at the end of the half that stays still
  refused), a line along nothing, across it, short of it and 2e-7 above it
  (beyond the tolerance, 1e-7: nothing; 5e-8 above: all six), a closed line's
  closing side, several lines, the constrained rim held and counted, and every
  refusal.
- Moving a vertex: `TinEdit.AMovedVertexKeepsItsLevelAndTheFootprintWhereverItIsTaken`
  (the vertex (2,2) of a plane at 1.5 taken to six places - inside its own star,
  across the ring of it onto a triangle beyond, onto the ring edge, onto the
  diagonal it is an end of, two cells away, onto a corner cell's diagonal - each
  leaving the area 16, the 25 vertices and 32 triangles, every other vertex where
  it was and the moved one at 1.5 and not the plane's there), a vertex taken
  across a breakline leaving it as long as it was, and every refusal.
- `tests/cad/terrain/test_surface_edit_verbs.cpp`: the verbs through the
  interpreter, one undo step, PREVIEW, `changed=no` with no step, every
  refusal; `test_surface_edit_tools.cpp`: the tools with `ToolDriver`;
  `test_drawing_shapes.cpp`: the polygons' and lines' reader these verbs
  share with the volumes - each kind of entity, typed corners with and
  without heights, the words, a scope that takes nothing.
- `tests/qt_widgets/terrain/test_surface_edit_dialogs.cpp`: the lines, the
  dialogs and the toolbar by object name;
  `qt_the_surface_heights_dialog_raises_the_surface_headless`: the real window,
  the dialog opened by its action and its line run through the executor.
- `cli.surface_edit_raise_moves_every_vertex_by_the_amount`,
  `cli.surface_edit_is_one_undo_step` (its figures anchored after the `1 undone`
  line: the SURFACE FROM reply already said the same numbers, so it passed
  whether or not UNDO restored anything),
  `cli.surface_edit_boundary_clips_to_the_typed_square`,
  `cli.surface_offset_makes_a_copy_by_name`,
  `cli.surface_edit_help_names_the_family`,
  `cli.surface_edit_of_no_such_surface_is_refused`, and the cases of the
  review worked on the sample terrain: a breakline ending on an old one stays
  level, a clip along a breakline, a breakline ending on a cut rim, a hole that
  crosses itself, a point that fills a hole, two abutting clips merged, a
  breakline through lattice vertices straight in 3D, crossing lines refused, a
  scope that takes nothing reported with the session going on
  (`src/katana_app/geo/cli/surface_edit.cmake`).

## Measured performance

The cases are in `benchmarks/bench_terrain.cpp` (`katana_benchmarks
--benchmark_filter=BM_Edit`), on the surface the review measured: 447 x 447
cells of 1 m, 399 618 triangles and 200 704 vertices, the most `SURFACE FROM`
makes by default. One iteration each, since the slow ones take seconds. The
polygons are regular, radius 150 m about the ground's middle; the footprints
are 100 rectangles of 8 x 6 m, one in each 40 m cell of a 10 x 10 lattice.

A polygon-scoped edit was not local in cost: every triangle or vertex was
classified against the whole polygon, so it cost the surface times the
polygon's corners, and HOLE FROM <scope> paid the whole-surface rebuild once
per footprint. They are now asked once per connected piece of triangles
outside the region (`rebuildPatch`) and through a banded polygon classifier
(`geometry::PolygonClassifier`, the same crossing rule as
`Polyline2::classify`, tested to agree with it point for point), and
footprints that can be cut together are cut in one patch (above).

| Case | Benchmark | Before: the review, wall time of a whole `katana_cli` run (the surface's load, 1.2 to 1.9 s, included) | Now: Release, `katana_benchmarks` |
|---|---|---|---|
| BOUNDARY, polygon of 8 / 500 / 5000 corners | `BM_EditBoundary` | 1.4 to 2.2 s / 4.5 s / 20.7 s | not measured |
| DELETE TRIANGLES INSIDE, 8 / 500 / 5000 | `BM_EditDeleteTrianglesInside` | 1.2 to 1.8 s / 4.9 s / 22.2 s | not measured |
| RAISE 1 INSIDE, 8 / 500 / 5000 | `BM_EditRaiseInside` | 2.3 s / - / 10.9 s | not measured |
| HOLE of 100 footprints, one at a time | `BM_EditHolesOneByOne` (`patches` 100) | 33.4 s (1.6 s of it the load) | not measured |
| HOLE of the same 100 footprints | `BM_EditHolesTogether` (`patches` 1) | - | not measured |
| THIN, tolerance 10 mm / 100 mm | `BM_EditThin` | "seconds", not measured | not measured |

The last column is empty because this change was written without a build, as
the owner's standing instruction for a session that edits asks: the
integration build measures it, and the figures go in the table with the commit
that records them. `BM_EditHolesOneByOne` is the
loop `HOLE FROM <scope>` was, kept in the same binary so that the two cases
are measured on one machine in one run.

## Not done

- **THIN runs on the GUI thread** from the window, and a 200 000-vertex surface
  is seconds (it triangulates each candidate's neighbour polygon twice, a few
  tens of microseconds each, then orders the flattest first; the first
  measurement runs on the `TaskPool`, the removal sequentially). It cannot be a
  background job yet: the window runs `SURFACE EDIT` through the interpreter on
  the GUI thread, and the jobs it already has - `SURFACE FROM`, the imports - are
  the geoprocessing executor's verbs, which copy their inputs on the GUI thread
  and land the result there. A cad verb as a job needs the same two halves for
  one undo step (the surface handed to the job as the shared immutable pointer
  it already is, and `Document::surfacesCommand` built where it lands), which
  is a change of the window's executor and not of this area.
- **Editing a vertex's level by dragging it in the window**: only ADD POINT at
  an existing vertex relevels it. (Moving a vertex in plan is MOVE POINT.)
- **Moving a rim vertex**, or the end of a breakline, is refused (above); a
  person reshapes the footprint with BOUNDARY and HOLE and a breakline with
  DELETE BREAKLINE and BREAKLINE.
- **A DELETE LONG that works from the rim inwards**, as some packages trim a
  convex hull, is not offered; BOUNDARY does the same job exactly.
