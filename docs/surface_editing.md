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
| cad | `src/katana_cad/tools/terrain_edit.cpp` | eight catalogue tools, category Terrain, each running the verb |
| qt | `src/katana_qt/terrain/surface_edit_ui.*`, `surface_edit_dialogs.*`, `src/katana_qt/tools/icons_terrain.cpp` | Terrain > Edit Surface, the Surface Edit toolbar, five dialogs |

The verbs (`HELP SURFACE EDIT` prints them all):

```
SURFACE EDIT <name> DELETE TRIANGLE x,y | DELETE TRIANGLES INSIDE|OUTSIDE <polygon>
                    DELETE LONG <length> | DELETE THIN <degrees>
                    DELETE POINT x,y [reach=<m>] | DELETE POINTS INSIDE <polygon>
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
anywhere after the surface's name, works the edit out and changes nothing.
The reply is one `surface` record:

```
surface name=ground operation=hole triangles_before=200 triangles_after=188 vertices_before=121
    vertices_after=125 points_added=4 points_removed=0 relevelled=0 moved=0 held=0 crossings=4
    [largest_change=] changed=yes [preview=yes]
```

LOWER is RAISE with the sign turned, added for the window's Raise / Lower
dialog and a person who would otherwise type `RAISE -0.3`.

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
  buildTin merges by.
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
`CrossingBreaklinePolicy::ErrorOnConflict` refuses breaklines. A POINT given
exactly on an old breakline keeps its own level, as a survey point on a
breakline does in buildTin.

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
  not asked to: DELETE POINT refuses a vertex on one
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
  *Rejected:* refusing a point off the surface (the brief asked that it
  extend it); joining it to the whole convex hull (that fills every
  concavity of the footprint between it and the surface).
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
  one covers any of it, so its seam is ragged by up to a triangle. It is
  kept for what it is - the rule the archive import's combined surfaces use,
  fast, never refusing - and PASTE is the clean seam. The merged result is
  compacted (combineSurfaces keeps the vertices of dropped triangles).
- **CLIP is BOUNDARY on a copy, OFFSET is RAISE on a copy**: one function
  each, the verbs name them for the person making a new surface.
- **The active surface is the program's, not the drawing's.** Which surface
  a person is editing is not part of the drawing, so it is not saved or
  undone: `cad::surfaceEditTarget`, one for the program as the edit tools'
  `EditDefaults` are (`docs/tools.md`). A tool acts on it when the drawing
  has it, else on the first visible surface, else the first
  (`cad::activeEditSurface`).

## In the window

Terrain > **Edit Surface** (`terrainEditMenu`) holds the eight tools - Delete
Triangle, Flip Edge, Add Point, Delete Point, Delete Triangles Inside, Add
Breakline, Boundary, Hole - and below them Raise, Lower or Set Level...
(`surfaceHeightsDialog`), Smooth... (`surfaceSmoothDialog`), Thin...
(`surfaceThinDialog`) and Delete Long Triangles... (`surfaceDeleteLongDialog`).
Terrain > **Combine Surfaces...** (`surfaceCombineDialog`) makes a merge, a
paste, a clip or an offset. The **Surface Edit** toolbar (`SurfaceEditToolBar`,
on the second row after Terrain) has them all, and at its head the active
surface (`surfaceEditActive`).

- **The tools are catalogue tools** (category Terrain, group Surface Edit,
  aliases `TINDELTRI`, `TINFLIP`, `TINADDPT`, `TINDELPT`, `TINDELIN`,
  `TINBRK`, `TINBOUNDARY`, `TINHOLE`), so the menu and toolbar are filled by
  `tools::fillToolMenus`, the window handing it this submenu and toolbar as
  the Terrain category's. Each turns what it was told into the SURFACE EDIT
  line a person would type, plans it with the verb's own `planSurfaceEdit`
  and returns `Document::surfacesCommand`'s command - one code path with the
  verb, one undo step - and reports the line and the reply. Each starts
  again after an edit, as Point does, so a run of flips is a run of clicks.
- **How a person picks the surface**: the toolbar's list, or a tool's own S
  option (S, then the name), which also sets the list. A tool with no surface
  in the drawing says so in its prompt and refuses every click.
- **Add Point** takes the click, then the level typed; Enter takes the
  surface's own level there (a vertex on the surface, no change of shape). A
  typed `x,y,z` adds the point in one go. **Delete Point** takes the vertex
  within the view's vertex reach (`vertexReach`, the snap aperture), passed
  to the verb as `reach=`. **Delete Triangles Inside** takes O for outside.
- **The previews** mark what a click would take - the triangle (Removed),
  the edge (Target), the vertex with its level - and say in red why a click
  would be refused: off the surface, an edge on the rim, a breakline.
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
- `Document::surfacesCommand` is not const, since the command it builds
  captures the Document it will change; a tool is handed a const Document so
  that it cannot change it before the host executes the command. The tool
  casts the const away to BUILD the command, which changes nothing - the one
  `const_cast` in the family, said where it is (`terrain_edit.cpp`).

## katana_cli and katana_mcp

The verbs are the interpreter's, so the window's command line, `katana_cli`
and `katana_mcp` (`katana_run_commands`) run the same code; the
geoprocessing executor keeps SURFACE LIST, INFO, REMOVE, FROM and EXPORT and
hands every other SURFACE line to the interpreter (`docs/terrain.md`). No MCP
tool of its own: the replies are records, and what an agent needs to choose
a surface is `katana_terrain_list`'s, which reads the edited surface
(`McpServer.ASurfaceEditThroughTheCommandToolIsWhatTheTerrainListReads`).

## Tests

- `tests/terrain/test_tin_edit.cpp`, `test_tin_edit_combine.cpp`: every edit
  on hand-made surfaces - a square of two triangles flipped
  (`TinEdit.FlippingTheDiagonalOfASquareOfTwoTrianglesGivesTheOtherDiagonal`),
  a point in the middle giving four triangles at the worked levels, a hole of
  known area, raise by 1 adding exactly 1, smoothing holding breakline
  vertices, thinning at 0 removing exactly the two points whose neighbours
  are all on the plane, the bound of thinning checked on a dense sample, the
  paste's seam on the top's rim - and every refusal.
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
  `cli.surface_edit_is_one_undo_step`,
  `cli.surface_edit_boundary_clips_to_the_typed_square`,
  `cli.surface_offset_makes_a_copy_by_name`,
  `cli.surface_edit_help_names_the_family`,
  `cli.surface_edit_of_no_such_surface_is_refused`
  (`src/katana_app/geo/cli/surface_edit.cmake`).

## Not done

- **Measured performance.** No benchmark yet; the region keeps an edit local,
  but THIN triangulates each candidate's neighbour polygon twice (a few tens
  of microseconds each) and runs on the GUI thread from the window: a
  200 000-vertex surface is seconds. A `benchmarks/bench_terrain.cpp` case
  and a background job for THIN are the follow-up.
- **Deleting a breakline**, or a vertex on one: refused, by the rule that
  constrained edges are kept. A DELETE BREAKLINE that unflags edges is the
  natural addition.
- **Moving a vertex in plan**, and editing a vertex's level by dragging in
  the window: only ADD POINT at an existing vertex relevels it.
- **PASTE with a top whose rim touches itself** (a hole touching the outer
  rim) is handled, but a top with two rim vertices closer than
  `tolerance::kGeometric` is refused by `TinSurface::create` with a message
  that names the triangle, not the cause.
- **A DELETE LONG that works from the rim inwards**, as some packages trim a
  convex hull, is not offered; BOUNDARY does the same job exactly.
