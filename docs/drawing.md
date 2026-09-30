# The drawing system

The owner asked on 2026-09-25 for "a professional drawing system, with full
vertex control, under the Draw menu". This document is its record: the
geometry it added, the rule for which kind a polyline is stored as, and -
section by section as they landed - the grips, the vertex tools, the
Vertices panel, the draw tools, precision input, the snaps and the verbs an
agent drives it with. The arithmetic is `docs/geometry.md` ("The drawing
curves"); the kinds' place in the model is `docs/model.md` ("The kinds
appended since"); the tool machinery is `docs/tools.md`.

## Three new kinds, appended

| Kind | Index | What it holds |
|---|---|---|
| `CurvePolyline2` | 9 | vertices, each with a bulge (the arc of the segment starting there, DXF's convention) and an optional height |
| `Ellipse2` | 10 | centre, major axis vector, ratio, start and positive sweep of the eccentric anomaly |
| `Spline2` | 11 | degree, control points, knots, optional weights, and the fit points it was drawn through |

They were appended after the annotation system's Label (7) and Leader (8),
whose branch was merged first so that no kind byte could mean two things
(`docs/model.md`). All three are written in version 2 of the geometry blob
(`include/katana/entity/geometry_blob.hpp` has the layouts;
`GeometryBlobWireFormat.ACurvePolylinesLayoutIsPinned` pins the polyline's
against bytes written by hand, and the ellipse's and spline's beside it).
They hold no anchor, so the smart leaders' version 3 never has cause to be
written for them; a reader takes them in version 2 or 3 alike.

They raised the project schema to 12 (`docs/model.md`, "The tables, and the
migration that made each"), with no table change: a build that predates them
refuses such a project up front as one written by a newer Katana, instead of
opening it and failing entity by entity with "unknown geometry kind in
blob". The branch that added them left the schema at 11; the bump came with
the merge into main.

### Which kind a polyline is

A polyline is stored as the SIMPLEST kind that holds it:

- every segment straight: a `Polyline2`, with its heights, if any, in the
  `elevation` / `elevations` properties (`entity::setHeights`) - exactly as
  every 3D string the program already had is held: the archive import, the
  survey import, the surface builder and the vector exports all read them
  there, and trim, extend, break, lengthen, join, hatch and parcel all work
  on it;
- any segment an arc: a `CurvePolyline2`, whose heights are in the geometry
  (one per vertex, a missing one for "not surveyed") and whose elevation
  properties are removed.

The vertex editing layer (`cad/drawing/vertex_editing.hpp`, below) reads
either kind into one `CurvePolyline2` and writes the answer back by this
rule, so converting the last arc of a polyline to a line gives back a
`Polyline2` with its heights in its properties, and nothing downstream sees
a difference between a polyline drawn straight and one straightened.

Rejected: a bulge on `Polyline2` itself. Every consumer that walks
`vertices[i] -> vertices[i+1]` as a `Segment2` would have become a silent
approximation of any polyline with an arc; a new kind is refused loudly
where it is not handled (`docs/model.md` lists where).

Rejected: always storing a drawn polyline as a `CurvePolyline2`. Trim,
extend, break and lengthen work on `Polyline2`, and a 3D string drawn by the
3D Polyline tool must build a surface and export to a shapefile exactly as
an imported one does.

### Where the new kinds go

| Consumer | CurvePolyline2 | Ellipse2 | Spline2 |
|---|---|---|---|
| picking, box selection | exact | chords (1 mm) | chords (1 mm) |
| snaps | vertices, segment and arc midpoints, arc centres, perpendicular and tangent on arcs | centre, ends, Quadrant | ends, fit points (Node) |
| intersection / nearest snap, sections, trim and extend boundaries | exact pieces | chords | chords |
| plan view, plot | chords at a quarter pixel | the same | the same |
| 3D view | chords, heights interpolated along each segment | chords in plan | chords in plan |
| DXF write | LWPOLYLINE with bulges (group 42); heights one elevation, or this module's extended data | ELLIPSE | SPLINE |
| DXF read | LWPOLYLINE and 2D POLYLINE with bulges come in as `CurvePolyline2` (they were chorded before) | ELLIPSE in plan comes in as `Ellipse2` | SPLINE comes in as `Spline2` |
| archive export | a super string with arc segments and per-vertex heights | chords | chords |
| vector export, and every GDAL algorithm (the one conversion, `interop::geo::drawingDataset`; `docs/geoprocessing.md`) | chords, heights interpolated; an area when closed | chords in plan; an area when whole | chords in plan; an area when closed |
| surface from drawing (`cad::geo::surfaceInput`, the SURFACE verb) | chords with interpolated heights, as breaklines | - | - |
| DRAPE (`cad::geo::drapeCommand`) | a height at every vertex, into the geometry; arcs kept | - | - |
| move, rotate, scale, mirror | exact (a mirror negates the bulges) | exact (a mirror re-expresses the sweep) | exact |
| stretch | vertices in the window move, arcs keep their bulge | whole, when its centre is inside | fit (or control) points move, solved again |
| offset (the Offset tool and OFFSET) | concentric arcs, mitre or round joins (`geometry::offset`), the side from the nearest piece | refused | refused |
| explode | a line per straight segment and an arc per arc segment, each with its ends' heights | - | - |
| trim, break (the tools) | cut along the path by `geometry::subPath` / `wrappingPath`: an arc cut part-way keeps its circle, cut ends take interpolated heights, and each piece is stored by the rule above | refused | refused |

What is not done: extend, lengthen and join of a `CurvePolyline2` (each
still takes `Polyline2` only, and refuses a curve polyline by name); the archive IMPORT
still chords a super string's arcs into a `Polyline2`; an ellipse under a
non-uniform block insert in DXF is chorded rather than re-fitted.

## Grips

With entities selected and no tool running, the plan view draws a grip on
every handle of the selection: a square on each vertex, a smaller diamond on
each segment's middle (ON the arc, for an arc segment), a circle on each
centre, squares on quadrants, line and arc ends and middles, insertion
points, and a spline's fit (or control) points. Blue is cold, green under the
cursor, red hot - the colours a drafter already reads.

**Everything a grip does is headless** (`include/katana/cad/drawing/grips.hpp`):
`gripsOf` says which handles an entity offers, `applyGripDrag` what a drag
does to it, `gripDragCommand` makes the drag ONE undoable `GRIP_EDIT`
command over every entity it touches, and `deleteHotVertices` is Delete on
hot vertex grips (`VERTEX_DELETE`). `tests/cad/drawing/test_grips.cpp` tests
each kind. The view's part is `src/katana_qt/drawing/grip_controller.*`:
the gesture's state and the drawing of the squares, fed from the view's
mouse and keys before its own selection handling.

| Gesture | What happens |
|---|---|
| press on a grip and drag | the grip follows the cursor - object snapped from the grip's old position, and otherwise ortho, polar or locked (below) - and the release is one undo step |
| click on a grip, move, click | the same without holding the button |
| typed while a grip is picked up | `x,y`, `@dx,dy`, `@distance<angle` or `<bearing`, or a plain distance along the cursor; Enter places it |
| Shift+click | makes a grip hot or cold without grabbing it; grabbing one hot grip moves every hot grip by the same displacement |
| Ctrl+drag on a segment middle | inserts a vertex where it is dropped instead of stretching |
| Delete | removes the vertices of the hot vertex grips (one step); with no hot vertex grip Delete deletes the selection as before |
| Esc, right-click | drops a picked-up grip; a second Esc cools the hot grips, a third clears the selection |
| double-click on a grip | opens the entity's editor, as a double-click on it does, and leaves no grip picked up |
| a vertex tool chosen with grips hot | the hot grips are the tool's handles (below, "What a vertex tool acts on"), and the grips go with the tool |
| hover | the view's band says what the grip is and what it offers: "Vertex 2 of polyline 12, z 101.500: drag to move · click to pick up · Shift+click to choose · Delete removes the chosen · right-click for vertex tools"; a segment middle's names Ctrl+drag |
| Ctrl held over a segment middle | the middle is drawn as the vertex a Ctrl-drag there would add |
| right-click on a polyline's grip | the shortcut menu begins with the grip's own items (`docs/desktop.md`, "The plan view's shortcut menu") |

**Grips say what they do.** The gestures that were written down only here -
Shift to choose, Ctrl to add a vertex, Delete for the chosen, the grip's
menu - are said in the band while a grip is hovered
(`GripController::hoverHint`), cut at the right where a prompt is cut at the
left, since the hint's start says which grip it is. A drag is drawn as a
tool's preview is (`cad::gripFeedback`, through the one feedback painter):
the edited geometry dashed, the grip's old place as the Target ring, and for
a Ctrl-insert the new vertex as the Added disc, read from the edited
polyline. Ctrl is heard from the view's keys and from each mouse move's
modifiers, so a Ctrl pressed while another widget had the keyboard still
shows.

**Delete is the view's while a vertex grip is hot.** The window's Erase
holds Delete as a shortcut, and Qt offers a shortcut's key to the focused
widget first (`ShortcutOverride`). The view claimed only Ctrl+Z there, so in
the window Delete on a hot vertex erased the whole polyline: the view's own
Delete path was never reached, and only a bare view in a test ever took it.
The view now claims Delete when a vertex grip is hot
(`GripController::hasHotVertex`), and leaves it to Erase otherwise.

**Delete inside a vertex tool is the tool's.** A vertex tool works on a
SELECTED polyline - a vertex's grips are there only while it is, and the
tool selects what it has just edited (R5, below) - so Delete pressed inside
the tool went to Erase and took the whole string from under it: "Press
Enter to delete vertex 2 of polyline 2", Delete pressed instead, and
polyline 2 was gone. The view claims Delete while a tool runs that takes it
(`InteractiveTool::takesDelete`, the vertex tools): Delete Vertex with
vertices chosen reads it as its Enter and deletes them; every other vertex
tool, and Delete Vertex with nothing chosen, does nothing with it and says
why ("Delete erases nothing while Insert Vertex runs - it would take the
polyline being edited; press Esc first to erase the selection"). The other
tools leave Delete to Erase, as they always did. Rejected: Delete as Enter
in every vertex tool - Enter makes Change Start's start and Straighten's
straight line, and a key named Delete must never do those; and leaving the
polyline unselected after an edit when nothing was selected before - the
chosen-vertex path selects it all the same, and R5's one-click chain goes.

**A hot grip follows its vertex.** A hot or picked-up grip is matched again
after every change by its POSITION on the same entity (the same index
first, where twin vertices share a point), not by its index: a vertex
inserted before a hot vertex made the grip name the vertex before it, and
the next Delete took that one. Where nothing is at its point, its own
vertex may have been moved (`VERTEX MOVE`, a Vertices panel cell, another
view), and matched by position alone such a move silently unchose it: a
polyline's vertex (or segment middle) then keeps its index only when every
OTHER vertex of the polyline is where it was, index for index - the move
was its own. Anything more and it goes: two edits between one repaint and
the next (a `SCRIPT`, lines pasted, an agent) can delete the chosen vertex
and insert another ahead of it, leaving as many vertices as before, and
kept by its index the grip named a vertex nobody chose, which Enter or
Delete then removed. A grip whose vertex has gone - deleted, undone - goes
too. A grip of another kind (a centre, a quadrant, a line's end), which no
edit renumbers, keeps its index while its entity has as many of its kind.

**A grip says what its drag does.** An arc segment's middle keeps both ends
and bends the arc, and its hint says so ("drag to bend the arc"), where a
straight segment's says "drag to stretch"; a Ctrl-press on a middle, which
adds a vertex where it is put down, says so in the band ("Grip segment
middle, adding a vertex: ..."), where it said what a stretch says.

**A tool takes the grips with it.** Starting any tool hands the hot grips
to it and drops them (`GripController::reset`); a grip picked up and still
attached to the cursor was otherwise put down by the first click after the
tool ended, as a `GRIP_EDIT` nobody asked for. A double-click on a grip
likewise drops the grip its first click picked up.

What a drag means per handle: a vertex moves and every arc keeps its bulge
(its shape between moved ends); a straight segment's middle moves the
segment bodily - both end vertices, a stretch - while an arc segment's
middle reshapes the arc through the cursor; a line's end moves its end and
its middle moves it; an arc's end or middle reshapes it through the other
two; a centre moves the circle, arc or ellipse; a circle's quadrant sets
its radius; an ellipse's axis end sets that axis (on a full ellipse the axes
swap when one passes the other; on an elliptical arc that is refused); a
spline's fit point moves and the spline is solved through the new points.
Several hot grips on one polyline move each vertex ONCE, however many hot
grips name it. A grip put down where it was picked up makes no undo step.

`gripsOfSelection` gives no grips past 20 000 (a selection of a whole
survey would otherwise paint a million squares), and none on a locked or
hidden layer. Dimensions, labels and leaders offer none here: their handles
are the annotation system's.

Tested with real mouse and key events in
`tests/qt_widgets/drawing/test_plan_view_grips.cpp`: a drag is one undo step,
click-move-click, a typed point, Shift-hot grips moving together, Ctrl
inserting, Delete and Esc, snapping to an endpoint and ortho; and, each shown
to fail without its fix, Delete claimed from the window's shortcut
(`DeleteWithAHotVertexIsTheViewsKeyNotTheWindowsErase`), a double-click
leaving nothing picked up (`ADoubleClickOnAGripLeavesNothingPickedUp`), a hot
grip following its vertex (`AHotVertexFollowsItsVertexThroughAnInsertBeforeIt`)
and going where two edits leave no telling which it was
(`AHotVertexWhoseVertexWentInTwoEditsBetweenPaintsGoesToo`), Delete inside
a vertex tool the tool's (`InsideAVertexToolDeleteNeverErasesThePolylineItIsEditing`)
and a grip picked up before a tool never put down after it
(`AGripPickedUpBeforeAToolIsDroppedNotCommittedLater`). The hints
(`AHoveredGripSaysWhatItIsAndWhatCanBeDone`,
`AVertexWithAHeightSaysItsHeight`, which also holds the band to the vertex
tools' way of writing a height, `cad::heightText`: it wrote "z -0.000"
where a tool's label wrote "z 0.000") are asserted as text, and the Ctrl cue
as ink against the same view without Ctrl
(`CtrlOverASegmentMiddleShowsTheVertexADragWouldAdd`); a Ctrl-insert's
feedback in `tests/cad/drawing/test_grips.cpp`
(`GripFeedbackOfACtrlInsertShowsTheNewVertex`).

## Precision input

Every point a user gives passes through the drafting aids
(`include/katana/cad/drawing/drafting.hpp`), headless, reading ONE set of
settings: `Document::drafting()`. A view, the command line and an agent's
verbs therefore change the same state, and it is session state - not saved
with the drawing and not undone.

**Typed points** (`parsePrecisePoint`, which `parsePointInput` now
delegates to, so the tools and the command line read one grammar):

| Form | Meaning |
|---|---|
| `x,y` / `x,y,z` | absolute; a z is a height at a step that takes one (`InteractiveTool::takesHeights`: Point, Polyline 3D, Move Vertex's new place, Insert Vertex's), and dropped at any other |
| `@dx,dy` / `@dx,dy,dz` | relative to the last point. At a step that takes heights, dz is read as the tool's verb reads it: the vertex's height at Polyline 3D, as `PLINE3D` has it; a CHANGE of the picked vertex's height at Move Vertex (`InteractiveTool::dzIsAChange`, `lastHeight`), as `VERTEX MOVE` has it, refused where it has none - handed on as the height itself, Move Vertex's `@0,0,1` set a surveyed 101.5 to 1. At any other step it is dropped, as the z of `x,y,z` is: the change rule first applied to every tool, and `@0,5,0` at MOVE or LINE was refused; then Polyline 3D climbed dz in the window while `PLINE3D` put the same vertex at dz |
| `@distance<angle` | polar from the last point |
| `@distance<N45d30'15"E` | the direction as a quadrant bearing, always read as a bearing |

An angle is decimal degrees (`45.5`) or degrees, minutes and seconds
(`45d30'15"`, `45°30'15.25"`, `45d30'`). Its convention is a setting: by
default degrees counter-clockwise from east, the command line's convention
since it was written; with `AngleConvention::Bearing` a whole-circle bearing,
clockwise from north, as survey work reads it. A quadrant bearing cannot be
misread and so ignores the setting. Bearings are written back as `D°MM'SS"`
(`formatBearing`, `formatQuadrantBearing`), with rounding carried so
59.9999" never prints as 60".

**At a point prompt** the one router, `routeTypedInput`, now also takes the
drafting settings and the cursor:

- `<angle` locks the direction of the next points and `=distance` their
  length; `<` and `=` alone clear them. The locks apply to picked points
  (a typed coordinate is exact) until cleared.
- A plain number is first offered to the tool as its value - a radius, a
  count - and only if the tool refuses it at a point prompt is it DIRECT
  DISTANCE ENTRY: the point that far from the last one towards the cursor
  (along the angle lock when one is set). So `2.5` typed after a circle's
  centre is still its radius, and `5` typed after a line's first point is a
  5 m line towards the cursor.

**The cursor** is constrained by `constrain` when no object snap took it:
the angle lock, else ORTHO (straight across or up from the base), else
POLAR tracking (within the aperture of a multiple of the increment, 15° by
default), and then the length lock. The view prints what constrained the
point beside the cursor ("Polar 45°00'00"", "Ortho", "Length lock 10").
Object snap tracking (`trackAcquired`) snaps to the horizontal and vertical
paths through acquired points and, preferring them, to where two paths
cross. The plan view acquires a point (`TrackingPoints`) each time an object
snap whose point is worth tracking from - an end, a middle, a centre, a
crossing, a node, a quadrant - takes the cursor while tracking is on (F11 or
`TRACKING on`), keeps the seven most recent, marks each with a small cross
and draws the path the cursor is on dotted from the point it runs through;
it forgets them when no tool or grip wants a point. Tracking yields to an
object snap, a lock, ortho and polar: it only moves a cursor nothing else
has fixed. Acquiring is immediate rather than after a pause, since a view
cannot tell a pause from a slow mouse, and landing on an acquired point
again moves it to the front of the seven. Tested in
`tests/qt_widgets/drawing/test_plan_view_tracking.cpp`.

**One-shot snaps**: From (`fromBase`: a base point, then an offset, the `@`
implied) and Midpoint Between Two Points (`midBetween`).

Tested in `tests/cad/drawing/test_drafting.cpp`, including a typed radius
staying the circle's radius while a typed distance at a line prompt goes
along the cursor.

## Snaps added

Appended to `SnapMode` as new bits, the old bits' values unchanged
(`kAllSnapModes` grew from 0xFF to 0x1FFF): Quadrant (circle, arc and
ellipse axis ends), Node (point entities, a spline's fit points), Extension
(a line or arc carried past its end), Parallel (the line through the base
parallel to a nearby line), Apparent Intersection (where two lines would
cross if extended), and the one-shot From and MidBetween, which `snap()`
ignores. Extension, Apparent Intersection and Parallel look at the curves
within 25 apertures of the cursor, since what they find lies away from the
geometry that produces it; Extension and Parallel are "somewhere along a
line" snaps and so rank after Nearest - an endpoint beats them however close
they are.

Each new mode has its own marker in the plan view, beside its name: Quadrant
a filled diamond, Node a circle with a cross, Extension three dots,
Parallel two slanted strokes, Apparent Intersection a cross in a square.

## The Vertices tools (Draw > Vertices)

Eighteen catalogue tools in one family (`src/katana_cad/tools/modify_vertex.cpp`,
registered in `families.hpp` as `addModifyVertexTools`), each named
"Vertices, <tool>" so the menus gather them into a Vertices submenu of the
Draw menu (`toolFamily.Draw.Vertices`) and the Draw toolbar into one
drop-down button. The Vertices panel's toggle is in View > Panels.

| Tool (id, aliases) | Steps | Shows before the click | Command |
|---|---|---|---|
| Insert Vertex (`draw.vertex.insert`, `INSERTVERTEX`) | ONE click on the polyline where the vertex goes: always on the line (on an arc, on the arc); `x,y` typed is put on it too, and `x,y,z` keeps z as its height. Beside a hot vertex, on whichever of its segments is nearer; Enter there takes the middle of the segment after it, marked. A typed point or number and Enter's middle are exact: taken however few pixels the segment is | the segment that splits (Target), the new vertex (Added), "new vertex between 1 and 2 · 15.000 from 1"; beside a hot vertex, the vertex and where Enter would add (a ring round a dot, "Enter"), and Enter's caption where a click takes nothing - on the chosen vertex, past a segment's end; a POINTED click within the pick aperture of a vertex is refused, that vertex struck | `VERTEX_INSERT` |
| Delete Vertex (`draw.vertex.delete`, `DELETEVERTEX`) | click the vertex; or Enter - or the Delete key - for the hot vertices, any number, on any polylines | an X on it, its segments dashed red, the segment that joins its neighbours; with the pointer on another vertex, the chosen ones still marked as Enter's ("2 · Enter") | `VERTEX_DELETE` |
| Move Vertex (`draw.vertex.move`, `MOVEVERTEX`) | click the vertex (or one hot), then a point; `@dx,dy` is from the vertex, `x,y,z` sets its height and `@dx,dy,dz` changes it by dz (refused on a vertex with none) | the vertex where it was and where it goes, the segments either side as they will be, "moves 3.000 at 0°00'00"" | `VERTEX_MOVE` |
| Edit Vertices (`draw.vertex.edit`, `EDITVERTICES`, `VERTEX`) | the selected polyline (or a hot grip's) is in the panel already: Enter, or click another; a hot grip of another selected polyline is shown by Enter ("Press Enter to show polyline 13 in the Vertices panel") | the polyline, "polyline 12 · 5 vertices, open" | - |
| Straighten (`draw.vertex.straighten`, `STRAIGHTEN`) | click one vertex to keep, then the other (or two hot vertices and Enter, the prompt naming them in the order walked: "from vertex 1 to 3"); a closed polyline's shorter side, `O` its other side | both labelled "keep", an X on the vertices between (thinned on a dense string, and never over a "keep" ring) and the path that goes, the straight segment left | `STRAIGHTEN` |
| Weed (`draw.vertex.weed`, `WEED`, `SIMPLIFY`) | select, the tolerance, keep survey-point vertices Yes/No | - | `WEED` |
| Densify (`draw.vertex.densify`, `DENSIFY`) | select, the interval, the chord tolerance for arcs (0 keeps them) | - | `DENSIFY` |
| Close or Open (`draw.vertex.close`, `CLOSEOPEN`) | select; each open one closes, each closed one opens | - | `CLOSE_OPEN` |
| Change Start Vertex (`draw.vertex.start`, `STARTVERTEX`) | click the vertex of a closed polyline (or one hot, then Enter) | "new 0" on it; refused on an open polyline or vertex 0, the vertex struck with its number | `START_VERTEX` |
| Set Vertex Height (`draw.vertex.height`, `VERTEXZ`) | click the vertex, then the height, None, or a click on anything with a height; Enter takes its own height (no undo step), else the height interpolated along the polyline | its height ("z 101.500", "no height") and what it becomes ("vertex 1 · no height → 5.000"); at the height, over something with one, what a click there takes ("→ 7.500"), and elsewhere Enter's, captioned "Enter: " - a click on nothing is refused | `VERTEX_Z` |
| Interpolate Heights (`draw.vertex.interpolate`, `INTERPOLATEZ`) | select | - | `INTERPOLATE_Z` |
| Grade Between Vertices (`draw.vertex.grade`, `GRADE`) | click one vertex, then the other (both need heights; or two hot vertices and Enter); `O` for a closed polyline's other side | their heights, "→ 100.500" on each vertex between, "grade 0 → 2: 1 vertex, 100.000 to 101.000 (10.000 %)" | `GRADE` |
| Segment to Arc (`draw.vertex.arc`, `SEGMENTARC`) | click the segment (or a hot segment middle, or two hot neighbours), then a point the arc passes through | the segment, then the arc and its radius; refused for a point in line, and at the pick for a segment with no length | `SEGMENT_ARC` |
| Segment to Line (`draw.vertex.line`, `SEGMENTLINE`) | click the arc segment | the arc and the chord it becomes; refused on a straight segment | `SEGMENT_LINE` |
| Fillet Vertex (`draw.vertex.fillet`, `FILLETVERTEX`) | click the corner, the radius (the last one typed is offered) | the fillet at the last radius: the two segments the arc is tangent to, an X on the corner that goes, the two tangent points, the arc; refused at the pick at an end, beside an arc or in line, the vertex struck with its number | `FILLET_VERTEX` |
| Chamfer Vertex (`draw.vertex.chamfer`, `CHAMFERVERTEX`) | click the corner, the distance along the segment marked "d1", then along the one marked "d2" (the last ones typed are offered) | the two segments labelled d1 and d2, an X on the corner, and the bevel at the last distances; refused at the pick as Fillet is | `CHAMFER_VERTEX` |
| Merge Near Vertices (`draw.vertex.merge`, `MERGEVERTICES`) | select, the tolerance | - | `MERGE_VERTICES` |
| Snap Vertices to Grid (`draw.vertex.grid`, `SNAPVERTICES`) | select, the spacing | - | `SNAP_VERTICES` |

**One state machine, eighteen scripts.** Each tool is a list of steps - a
vertex, a segment, a polyline, a point ON a polyline, a point, a value, or
several polylines - and a PLAN (the one-polyline tools) or a finish (the
selection tools) that builds its ONE command over the arithmetic of
`polyline_vertices.hpp`. So a tool only collects arguments; what an edit
does is the same whether it comes from a tool, a grip, the panel or a verb.
A plan that fails at a value (a radius that does not fit) is refused and the
tool steps back to that input, keeping the picks before it. Tools that act
on one pick restart for the next, as Point and Circle do; tools that act on
a selection take the selection they were started on - the polylines in it
the view lets be edited (R1), so one whose layer was hidden or locked after
it was selected is left alone - and then Enter applies them ("Press Enter
to apply to the 1 selected polyline"). The values they
ask for are remembered for the next use, as a CAD program keeps the last
fillet radius: an edit's values are kept when it is committed
(`Script::remember`), never by a preview, which plans at the values kept.
The rewrite of 2026-09-30 dropped Fillet's and Chamfer's, and the test that
should have caught it "remembered" 1, the value they start with; it now
types 2.5, 1.5 and 0.75 (`FilletAndChamferRememberWhatWasTypedForTheNextUse`).

**Weed keeps survey points.** With Yes, a vertex on which a survey point
sits (a point entity with the survey import's point-number property, within
0.1 mm) is never weeded: a string's shots are its data.

Icons: `src/katana_qt/tools/icons_drawing.cpp`, one picture - a
three-vertex polyline - with what each tool does to it in the accent.

## What a vertex tool acts on

The owner's request of 2026-09-30: inserting a vertex gave "no visual clue
where the vertex is going", and a tool said "select a polyline to insert a
vertex" - "but what about the already selected vertex". The audit behind it
found more of the same: 12 of the 18 tools asked for a polyline already
selected; a vertex was the one NEAREST the click at any distance, so a click
mid-way along a 100 m segment deleted a vertex 50 m away; a survey point on a
vertex took the pick from its string; the preview was one generic band from
the vertex nearest the pick - which was not even the segment Insert split -
and Segment to Arc drew a straight line. The rules now, in
`src/katana_cad/tools/modify_vertex.cpp`:

- **R1, one function decides what a pick takes** (`resolve`), for the
  preview, the click and a typed point alike, so they cannot disagree. In
  order: the tool's HANDLES (the grips hot when it started, below); the
  nearest SELECTED polyline within reach; the polyline the view would pick
  there - polylines only (`ViewPick`, through the view's hidden layers).
  Nothing else: the entity the view picked for `entity(id, at)` is ignored.
  Whatever the tool finds itself - through the selection, the handles, a
  point's height for Set Height - passes THE visibility rule
  (`selection.hpp`, `isSelectable`) in the tool's view (`ToolContext::view`,
  `pickable`): a selected polyline whose layer was then hidden, in the
  drawing or in the view alone, was previewed over empty ground and edited,
  and on a locked layer the preview promised a click the command refused.
  Such a polyline is not named by the prompt; one on a locked layer is said
  to be before the prompt ("polyline 2 is on the locked layer BUILDING;
  unlock it to edit it. Click on a polyline where the new vertex goes").
  The rule holds for every tool of the family: the six that act on the
  selection (Weed, Densify, Close or Open, Interpolate Heights, Merge Near
  Vertices, Snap Vertices to Grid) take only its polylines the view lets be
  edited - Close or Open opened a hidden square - and Enter with none says
  so; Edit Vertices, which answers from the selection itself, too.
- **R2, reach.** A vertex within the view's 12 px (its snap aperture, the
  reach of an Endpoint snap), a segment or a point on the line within its
  8 px; only a handle reaches at any distance, because the user chose it.
  A typed `x,y` at a pick is taken as a click there is (R1): within the
  same reach, at the zoom of the moment - a typed number names a vertex at
  any zoom - and one that misses says so, with the reach in drawing units
  ("116,85 is not within 0.923 of a vertex the tool can take: a typed point
  is taken as a click there is, within the view's 12 px at this zoom; type
  the vertex's number, or zoom out"); it said the prompt alone, and the
  typist cannot see the reach. What a place given exactly - a typed point
  or number, Enter's marked middle (`Chosen::exact`,
  `InteractiveTool::exactPoint`) - is spared is the one rule about what a
  POINTER meant, Insert's "too close to vertex N": there only geometry's
  own refusal, a point on a vertex, applies. That rule turned down the
  marked middle of every segment 16 px long or less, the one the prompt
  had just offered, while the verb inserted there. (This paragraph, and
  the tool's header, said a typed point was refused ONLY where geometry
  refused it; it never was, at a pick.) Rejected: a typed point taking the
  nearest vertex at any distance - the 50 m delete, typed.
- **R3, the preview is the click.** Each one-polyline tool PLANS its edit
  with the same `geo::` call the commit makes, and the preview draws the
  plan by role (`tool_feedback.hpp`): Target, what the click takes; Added,
  what it makes - read from the result, so a vertex put on an arc is on the
  arc; Removed, what it takes away. A plan refused for the PICK (an end
  vertex for Fillet, an open polyline for Change Start, a vertex with no
  height for Grade, a segment with no length for Segment to Arc) is refused
  at the pick, in red beside the cursor, before any value is asked; one
  that fails only at a value's default (a radius that does not fit) says so
  and takes the pick. With handles answering every step (below) a click
  still picks anew, so where a click would take something the preview is
  THAT click's, with what Enter would act on still marked beside it ("2 ·
  Enter") - the chosen vertex was a plain square, and the prompt's "vertex
  2" was nowhere on screen; elsewhere - where a click takes nothing and is
  refused, keeping the handles - it is what Enter does, its caption
  beginning "Enter: ". It drew the handles' plan wherever the cursor was,
  and a click on another vertex deleted one nobody had marked. Insert
  beside a chosen vertex or segment is the same: past the segment's ends,
  and on the chosen vertex itself where the pointer rests after choosing
  it, a click takes nothing and the preview is Enter's. It put the point
  on an end there and said "too close to vertex N; zoom in", in red, at
  any distance and whatever the zoom - the first thing the tool showed.
  Back on the vertex a pick has just taken (Straighten's, Grade's first),
  a click is refused, "vertex 0 is picked already; click the other one",
  and the preview says so; it showed the pick as taken, and a preview not
  refused was a click that was.
- **Not answered in red.** A click the tool took is answered with what it
  did ("vertex 2 added to polyline 2 on segment 1 (5 vertices)") beside the
  cursor until the pointer moves off it by the pick aperture
  (`ToolHost::feedback`): the restarted Insert is right that a second
  click on the vertex it just made would be refused, but red straight after
  every success read as a failure. The hold takes away only the marks the
  tool flags as the refusal's reason (`FeedbackMark::refused`) and keeps
  the rest, so Straighten's first "keep" stays in sight straight after its
  pick. A refused click is shown as refused.
- **R4, a stale pick is refused, not reverted.** An edit is made from the
  polyline as it was picked; the tool and the command both compare it with
  the drawing, and a polyline changed since (the panel, an agent, another
  view) is refused: "polyline 12 changed since it was picked; pick it
  again". The edit used to overwrite it with the shape from the pick. What
  a tool holds - its picks, the grips chosen before it, Insert's anchor -
  is checked again at EVERY preview and input (`whyNotNow`): changed,
  deleted, or on a layer hidden or locked since, the preview is that
  refusal, with no marks where the polyline was, and the next input is
  refused with it and drops what was held, so the one after picks afresh.
  Only fresh picks were checked: a kept one was drawn from its copy, green,
  and then refused as stale at the click - or, on a layer hidden since,
  Enter edited what the view no longer drew.
- **R5, the tool stays on the polyline.** After an edit the polyline is
  selected (when at most one thing was), so the restarted tool has it as
  its own and the next edit is one click. One thing selected moves to the
  polyline just edited: it was mostly the tool's own choice after an
  earlier edit, and kept, the prompt named the first polyline while the
  user worked on another. Several selected are left as they are.
- **R6, handles are used once.** `ToolHost::make` gives the hot grips to the
  tool it makes and to no other: a restart after an edit that renumbered
  the vertices never sees them.
- **R7, numbering** counts from 0 in the prompts, captions, labels, the
  panel and the verbs.
- **R8, the other surfaces.** The looks need no verb. What the window takes
  from the mouse - hover, handles - is the window's; on `katana_cli` and
  `katana_mcp` the same edits are the `VERTEX` verbs with explicit numbers,
  and `VERTEX INSERT id #id@x,y[,z]` is Insert's click: the place on the
  polyline nearest x,y, on the line, with z its height. Such a point names
  its segment, so an `after=` naming another is refused - it put a point
  of segment 0 after vertex 1, and the polyline folded back. A rule the tools
  add is the verbs' too, as an option: Straighten's and Grade's side of a
  closed polyline is `cad::vertexRange` - the side with fewer vertices
  between, O the other - which `STRAIGHTEN` and `VERTEXZ GRADE` walk when
  asked (`side=short`, `side=long`). Their default stays what it always
  was, forward from the first number to the second. Rejected: the shorter
  side as the verbs' default (the first fix of this review took it): the
  same line - `STRAIGHTEN 1 3 1` on a closed hexagon - then took out one
  vertex where it had taken out three, and `VERTEXZ ... GRADE` regraded
  surveyed heights on the other side, with nothing in the reply to say so:
  an existing script's or agent's line changed meaning silently. A verb
  line names the order it walks, so forward is exact; the window's picks
  have no order a user means, so the shorter side is the window's.

**Handles.** The grips hot when a tool starts (a plain click on a grip makes
it hot, Shift+click adds one) answer its steps, and the prompt says what
Enter will do: "Press Enter to delete vertex 2 of polyline 12, or click
another vertex". A click that takes something then picks anew and drops
them; one that takes nothing is refused and keeps them. Delete takes any
number of hot vertices, on any polylines, as one step; Move one; Straighten
and Grade two on one polyline, or one for their first pick; Change Start one
on a closed polyline; Set Height, Fillet and Chamfer one; Segment to Arc and
to Line a hot segment middle or two neighbouring hot vertices; Edit Vertices
any grip's polyline, or the one selected polyline. Insert is answered by
nothing: a hot vertex or segment says WHERE, and the click (or Enter, for
the middle, marked) says the point. What was chosen and cannot be used is
never dropped without a word: the reason comes before the prompt - "2
vertices are chosen, and this tool moves one. Click the vertex of polyline
12 to move", "vertex 0 is an end of the polyline; a corner is where two
segments meet. Click the corner vertex of polyline 3 to round", and for
Delete with so many that too few would be left, the arithmetic's sentence.

**Typed at a pick.** `x,y` is a click there (R1), within the view's reach,
spared only Insert's pointer rule (R2), and a miss says why; a whole
number names vertex N (or segment N) of the polyline in play, read as the
verbs read an index (`core::parseInteger`), so a headless run can drive
every pick by typing. `#12.v3` typed into the window's command line at a
tool's prompt is still read as a comment (`main_window.cpp`), below.

**Choosing a vertex, then the tool.** A plain click on a vertex chooses it,
and also picks the grip up to take a typed point. A word typed next - the
tool's name, `INSERTVERTEX` - went into the grip's point and was refused
there. Nothing a grip takes starts with a letter (`x,y`, `@dx,dy`,
`<bearing`, a distance), so a letter typed first puts the grip down, still
chosen, and goes to the command line (`ViewportWidget::keyPressEvent`).

**The look** (`src/katana_qt/drawing/feedback_painter.hpp`, the one home of
the overlay's colours, the snap marker's yellow among them; each at least
3:1 against the view's ground): a Target piece a solid 3 px green line, arcs
as arcs; a Target vertex a green ring round a filled square, with its number
or "keep" beside it; an Added vertex a cyan disc with a dark "+"; what Enter
would act on (the `Enter` role) the disc's circle round a dot, its middle
the dark ground, with "Enter" or "2 · Enter" beside it - hollow, the segment
through it read as a circled minus beside the new vertex's circled plus. The
Enter role is what Enter acts on, whatever Enter does there: the place
Insert adds at, and the vertex chosen before Delete, Straighten or Change
Start while the pointer is on another - the one Enter deletes, keeps or
makes the start, as its label and the prompt say; with the pointer off the
vertices that same vertex shows Enter's plan itself (Delete's X), since the
preview is then Enter's. It is drawn before the Target vertices, so on a
short segment the chosen vertex's ring and square lie over Enter's place a
few pixels off: Enter's opaque ring, drawn after, had hidden them. A
Removed vertex is a red X on a dark disc, smaller than a ring, and never
drawn over a Target vertex's ring - an X inside a ring is a circled X,
"cancel", the near twin of the refusal's struck ring - and a run of them is
thinned as the focus squares are (a dense string's Straighten was one red
rope, the X beside each end across its "keep" ring). Fillet's corner is an
X, as Chamfer's is, with the two segments the arc is tangent to as its
Target: a Target ring on the corner with the X over it was that circled X,
and at the default radius - the arc a pixel or two across - all the
preview showed. A removed piece is red dashes over a dim stroke, so they
read over the orange selection dashes; the ghost of what changes, dashed
cyan as every tool's has always been; the polyline in play's vertices as
the grips' cold squares, hollow, vertex 0 numbered - the grips are hidden
while a tool runs - drawn FIRST, under every mark, and thinned so no two are
nearer than 10 px (each end kept): a survey string's hundreds were a solid
blue band over the target. Labels are DemiBold: at the regular weight a
digit's one-pixel stem fell across two columns at about half ink, and a
refused red "1" read at 2.9:1 against its chip (4.9:1 now). The caption is
on a dark chip, its stripe red when refused, at the first corner of the
cursor - below right, above right, below left, above left - that is in the
view, off the prompt band and clear of every mark, label and piece along
its length; it sat below right whatever was there, and hid Enter's place,
and at the view's edge it was pushed onto the very segment the preview
marked, since only a piece's middle was kept off.
**A refusal has its own shape as well as its colour**: the marks that say
why a click is refused (`FeedbackMark::refused`; when a refused preview
flags none, its Target marks) are a red ring struck through, the sign for
"not here", on a vertex and at a refused piece's middle. A refused pick
drawn as the accepted ring and square in red was the same khaki ring to a
red-green colour-blind eye (Machado, Oliveira and Fernandes 2009: the two
colours a CIE76 difference of about 5), and only the caption's words told
them apart. The flag names the reason, so the rest of the preview keeps its
colours: Insert too near a vertex strikes that vertex, with its number,
instead of reddening the whole segment - a flash at every vertex the pointer
passed, which named none - and the chosen vertex beside it stays the green
it was.
**The prompt band** (`paintBand`) is cut to the view in the middle while
nothing is typed, keeping the tool's name and the prompt's end, and at the
left once something is, keeping what is typed in sight: cut at the left, it
lost "Insert" at the larger text sizes. Insert's prompts with something
chosen put Enter's offer first, as every tool's "Press Enter to ..." does -
"Press Enter for a vertex at the marked middle of segment 2, or click beside
vertex 2 of polyline 2" - since the command line cuts its placeholder at
the right, and cut "or press En...".
The view's snap marker is drawn beneath the vertex glyphs
(`FeedbackFrame::beneathGlyphs`): the Midpoint triangle hid the new vertex's
"+", and the Intersection's X across the disc read as the Removed X.
**Insert takes only a snap ON the line** (`InteractiveTool::takesSnap`,
asked by the view through `SnapRequest::accept`, which passes a refused
candidate over for the next best): one within a pixel of the polyline it
would insert into, where the click would be taken. An arc segment offers
its Center while hovered, and another string's end beside the line is in
the snap aperture (12 px) but out of the pick's (8 px): each took the
cursor off the line, and Insert showed nothing, under the default snaps.
The line's Midpoint and a crossing are still snapped to; its own vertices
never are, being too close to insert at.
A refused pick keeps its Target marks, which say what the pick took, but
draws them as the refusal: they were green, and green promised the click
the caption said would be turned down (the ring on an open polyline's end
under Fillet Vertex). The prompt calls the tool by what it does ("Insert
Vertex: Click on polyline 2 where the new vertex goes"), not by its family's
menu name (`ToolInfo::title`). Nothing is drawn while the pointer is not
over the view: before it first is, the cursor is the origin, and a tool
started from the command line marked whatever vertex lay at 0,0; once it
has left for a menu or a toolbar, the cursor is where it left, and a tool
chosen there previewed at that spot - Move Vertex dragged the chosen vertex
to the view's edge (`ViewportWidget::leaveEvent`).

Rejected, and why:

- **A vertex bent off the line by an Insert click past the pick aperture.**
  How AutoCAD's Add Vertex feels, but the aperture is pixels: the same hand
  wobble is a bend at one zoom and not at another, and a bent survey string
  is wrong data. A bend is Ctrl+drag on a segment's middle grip, Move Vertex
  afterwards, or `VERTEX INSERT id x,y`.
- **The Nearest snap for Insert.** It lands on whatever lies nearest,
  another string included, and still bends the line past its reach.
- **The two-click Insert** (pick the polyline, then the point): the pick
  only chose the polyline, the segment came from the point, and the band
  between them showed neither; a run of inserts cost two clicks each.
- **A second preview entry point** (`previewFor`, a new virtual taking the
  pick): 46 `preview` overrides and `-Woverloaded-virtual`. The view's pick
  reaches a tool through its context instead (`docs/tools.md`).
- **Colours in `theme.hpp`**: its tokens are the chrome's, and it says the
  views keep their drawing colours.
- **A see-through green halo** for a Target: muddy over the orange
  selection. The solid 3 px line is not.
- **Enter beside a hot vertex following the cursor's side.** It would put
  the vertex where the preview is, but a keyboard's Enter meaning a
  different place with each mouse move cannot be named in a prompt, nor
  typed headless; Enter stays the middle of the segment after the vertex,
  and the preview marks it.
- **Holding off the red in the tool.** The tool is made anew at each
  restart and never hears the pointer; the host sees both the click and
  every move, so the hold is the host's (`ToolHost::feedback`).
- **Passing snaps over by mode** (no Center, no Endpoint for Insert): a
  mode cannot tell another string's end beside the line from one ON it,
  where a vertex is wanted, and Center is right for Move Vertex. The tool
  asks where the point lands instead.
- **A focus square for every vertex at any zoom.** At survey densities they
  merged into a band that hid the line; thinned, they still show where the
  vertices are.
- **Refusal as a fifth role.** A refused mark still says what the pick took
  - a Target - and the pointer record counts it so; a flag on the mark
  keeps both, and a preview that flags none needs no change.
- **A dashed ring for a refusal.** Dashes already mean "goes" in this
  overlay (a removed piece); the struck ring is the sign for "not here".
- **Drawing the Enter ring refused, and dropping the Enter offer, on a short
  segment** (the review's other suggestion). It would keep the pixel rule
  for keyboard input, whose place is exact at any zoom; the verb inserted
  there all along.
- **A stale pick dropped and the click then taken afresh.** The preview had
  said the click was refused; the click must be, and the next one picks.
- **The prompt said again after every refusal.** The window's status bar
  shows the refusal for six seconds, and the prompt said after it wiped it;
  it is said again only when the refusal changed it.
- **Straighten's refused first pick named by its number** (the review's
  suggestion, as Change Start's and Fillet's refused vertices now are).
  Back on the vertex its first pick took, the struck ring keeps "keep":
  that vertex IS kept - the first pick stands, and the strike and the
  caption ("vertex 0 is picked already; click the other one") refuse only a
  second click there - and the host's hold shows the same mark, unstruck,
  as the pick just taken. Change Start's "new 0" on an open polyline, and
  Chamfer's "d1" on an end, promised what the refusal turned down, and are
  the vertex's number now.
- **A whole-pixel origin for the labels** (the review's suggested cure for
  the dim "1"). Tried and measured: the renderer places text on whole
  pixels already, and the stem read the same, 2.9:1, from any origin; so
  did full hinting. The weight is what changes it.
- **The Enter glyph only for a place, a Target ring for a chosen vertex.**
  With the pointer on another vertex the preview is that click's, and a
  second Target ring would read as the click taking both; the label ("2 ·
  Enter") and the prompt say what Enter does to the vertex it marks.

Tested in `tests/cad/tools/test_modify_vertex.cpp`, each tool driven as a
user drives it, and `tests/cad/tools/test_vertex_tool_feedback.cpp`: every
cue's coordinates against values worked out by hand (a vertex on the arc
bulge +1 at (5,-5), halves of bulge sqrt(2) - 1; the centre (5,-5.25) and
radius 7.25 of an arc through (5,2); a closed hexagon picked 3 then 1
straightened on the short side), the handles (one step, used once, a stale
choice refused), the reach, and two properties over grids - an Insert
preview is exactly the vertex the click makes, and no pick tool's preview
promises what its click does not (`TheInsertPreviewIsWhatTheClickCommits`,
`APreviewNeverPromisesWhatTheClickDoesNot`, which print how many cases of
each kind they reached). The second covers every pick tool but Insert
(whose own grid is the first), 25 cases of 81 points, with and without
chosen vertices and at the step after a first pick: a preview refused, with
no mark, or the tool's idle picture is a refused click; one captioned
"Enter: " a refused click, then Enter's edit; ANY other a click the tool
takes. Once an edit is done it is the one shown - Added points vertices,
Removed ones gone, each ghost a piece of the result, "new 0" the start, a
"→ z" label the height, Set Height's caption the height set, Edit
Vertices' polyline selected - and no vertex is marked both taken and gone.
It had checked only previews showing an Added, a Removed or a ghost, over
five tools, and so nothing of Change Start, Set Height or Edit Vertices.
The review of 2026-09-30, each test shown to fail without its fix:
`WithAChosenVertexAClickIsPreviewedWhereItTakesSomethingAndEnterElsewhere`,
`BesideAChosenVertexThePlaceEnterAddsAtIsMarked`,
`FilletAndChamferRememberWhatWasTypedForTheNextUse`,
`MoveVertexTakesARelativeDzAsAChangeOfItsHeight`,
`APolylineOnAHiddenOrLockedLayerIsNotTakenThoughItIsSelected`,
`SetHeightTakesNoHeightFromAPointTheViewHides`,
`InsertTakesNoSnapOffTheArcItInsertsInto`,
`InsertTakesNoSnapBesideTheLineNorOnAVertexOfIt`,
`AChosenVertexTheToolCannotUseIsExplainedNotDropped` and
`TheToolStaysOnThePolylineLastEditedNotTheFirst`; the verbs' side and
anchored forms in `tests/cad/drawing/test_drawing_verbs.cpp`
(`StraightenAndGradeWalkForwardUnlessTheShortOrLongSideIsAskedFor`,
`VertexInsertAtAPointOfThePolylineTakesThatSegmentAndAHeight`) and
`cli.straighten_walks_forward_and_takes_the_short_side_when_asked`,
`cli.vertex_insert_on_the_line_takes_a_height`. The drawn cues in
`tests/qt_widgets/tools/test_vertex_tool_hover.cpp` (ink against a baseline,
and the drawing never repainted by a hover) and
`tests/qt_widgets/drawing/test_feedback_painter.cpp`, the hold in
`tests/qt_widgets/tools/test_tool_host.cpp`
(`AClickTheToolTookIsNotAnsweredInRedUntilThePointerLeavesIt`); the whole
window with the pointer steps (`docs/headless.md`), in
`qt_insert_vertex_shows_where_the_vertex_goes_headless`,
`qt_insert_vertex_puts_it_on_the_line_headless`,
`qt_insert_vertex_beside_the_vertex_clicked_first_headless`,
`qt_insert_vertex_is_answered_with_what_it_did_headless`,
`qt_insert_vertex_with_snaps_on_stays_on_the_line_headless`,
`qt_delete_vertex_takes_the_chosen_vertex_headless`,
`qt_delete_vertex_previews_the_click_not_the_chosen_vertex_headless`,
`qt_straighten_shows_what_goes_headless`,
`qt_segment_to_arc_shows_the_arc_headless` (the radius worked by hand) and
`qt_fillet_refuses_an_end_vertex_at_the_pick_headless`.

The second review of 2026-09-30, each test shown to fail with its own fix
taken out and the others in place: in
`test_vertex_tool_feedback.cpp`,
`BesideAChosenVertexAPointerPastItsSegmentsShowsWhatEnterDoesNotARefusal`,
`WithChosenVerticesTheClickPreviewStillMarksWhatEnterTakes`,
`EnterAndATypedPlaceAreTakenOnASegmentShorterThanTwoApertures`,
`APickEditedSinceIsShownRefusedAndTheNextInputDropsIt`,
`AChosenVertexOnALayerHiddenOrLockedSinceIsNotEditedByEnter`,
`TheSelectionToolsTakeNoSelectedPolylineOnAHiddenOrLockedLayer`,
`BackOnTheVertexJustPickedTheClickIsRefusedAndThePreviewSaysSo`,
`InsertingNearAVertexIsRefusedBeforeTheClick` (the vertex struck, no red
segment), `EditVerticesTakesNoSelectedPolylineOnAHiddenOrLockedLayer`,
`EditVerticesSaysEnterShowsAGripsPolylineThePanelDoesNotShowYet`,
`SegmentToArcRefusesASegmentWithNoLengthAtThePick`,
`ATypedPickNumberIsReadAsTheVerbsReadAnIndex` and
`ADzTypedWhereTheStepTakesNoHeightIsDroppedNotRefused` (Segment to Arc's
point, which takes no height, though a point step); the dz of typed relative
input in `MoveTool.ARelativePointWithADzMovesInPlanAsItAlwaysDid` and
`DrawLineTool.ARelativePointWithADzIsTakenInPlan` (both routes typed text
takes); the verb's `after=` in
`DrawingVerbs.VertexInsertRefusesAnAfterThatIsNotTheSegmentOfItsPoint`;
the drawing in `FeedbackPainter.ARefusedMarkDiffersInShapeNotOnlyInHueToARedGreenColourBlindEye`
(the painted marks passed through the deuteranopia matrix, with a control
that the two hues alone count nothing),
`OnlyTheMarkFlaggedAsTheReasonIsDrawnRefused`,
`EntersPlaceIsARingRoundADotWithNoLineThroughIt`,
`TheCaptionKeepsOffTheMarks` and `TheBandCutsWhereItIsToldAndSaysWhatItDrew`;
the view in `PlanViewTools.NoPreviewIsDrawnWhileThePointerIsOutsideTheView`,
`VertexToolHover.ThePointerRecordReadsBackAsOneRecordWithABearingInItsCaption`
and `ALongPromptIsCutInTheMiddleSoTheToolsNameStays`; the host in
`ToolHost.StraightensFirstPickIsHeldAsTakenAndItsRefusalShownOnceThePointerComesBack`
and `ARefusalThatDropsAStalePickSaysTheNewPromptAndNoOtherRepeatsIt`; the
grips in `PlanViewGrips.AHotVertexStaysChosenWhenItIsItselfMoved`,
`AWordTypedAfterAClickOnAGripGoesToTheCommandLineAndTheGripStaysChosen`,
`AnArcsMiddleSaysADragBendsIt` and
`ACtrlPressOnASegmentMiddleSaysItAddsAVertex`; and the whole window in
`qt_insert_vertex_beyond_a_chosen_corner_shows_enter_not_a_refusal_headless`,
`qt_insert_vertex_refuses_a_choice_edited_since_headless`,
`qt_pointer_record_quotes_a_bearing_as_a_reply_does_headless`,
`qt_close_or_open_leaves_a_selected_polyline_on_a_hidden_layer_headless`
and `qt_move_takes_a_relative_point_with_a_dz_in_plan_headless`.

The third review of 2026-09-30, each test shown to fail with its own fix
taken out and the others in place: in `test_vertex_tool_feedback.cpp`,
`AFilletsCornerIsMarkedAsTheVertexThatGoesNotAsATakenOne` (and Chamfer's
refused end), `ChangeStartShowsTheNewFirstVertexAndRefusesAnOpenPolyline`
(the refused vertex named), `AThreeDPolylineReadsARelativeDzAsThePLINE3DVerbDoes`
(the tool against the verb, one line), `ATypedPointOutOfReachSaysSoRatherThanRepeatingThePrompt`,
`StraightenAndGradeSayTheSideEnterTakesAndOtherSideFlipsIt`,
`EditVerticesMarksNoVertexForEnterWhereTheClickTakesTheSamePolyline`,
`SetHeightsValueStepPreviewsTheHeightAClickTakesAndEntersElsewhere` and the
widened `APreviewNeverPromisesWhatTheClickDoesNot`; the verbs'
forward walk in `DrawingVerbs.StraightenAndGradeWalkForwardUnlessTheShortOrLongSideIsAskedFor`
and `cli.straighten_walks_forward_and_takes_the_short_side_when_asked`;
the view in `PlanViewGrips.InsideAVertexToolDeleteNeverErasesThePolylineItIsEditing`,
`AHotVertexWhoseVertexWentInTwoEditsBetweenPaintsGoesToo` and
`AVertexWithAHeightSaysItsHeight` (the "-0.000"); the drawing in
`FeedbackPainter.ARunOfVerticesThatGoIsThinnedAndNeverCrossesOutAKeptVertex`,
`AChosenVertexIsDrawnOverEntersPlaceBesideIt`,
`ARefusedVertexsNumberReadsAtThreeToOneAgainstItsChip` (WCAG 2.1's
contrast, from the pixels) and
`TheCaptionKeepsOffAPieceAlongItsLengthNotOnlyItsMiddle`.

## The Vertices panel

A dock ("VerticesDock", hidden until asked for) holding
`src/katana_qt/drawing/vertex_panel.*`: the first polyline in the selection
as a table - index, easting, northing, height, bulge, and the bearing
(whole-circle, D°MM'SS") and chord distance of the segment that starts at
each vertex - that follows the selection and the drawing live. Typing into
a cell is ONE undoable `VERTEX_SET`: a coordinate moves the vertex, a height
sets it (empty clears it), a bulge reshapes the segment, and a bearing or a
distance moves the NEXT vertex so that the segment has it, as a traverse
table is edited. A value that does not parse changes nothing, puts the cell
back and says why in the panel's title. Insert After and Delete act on the
current row.

The rows and the cell edits are headless (`include/katana/cad/drawing/vertex_table.hpp`),
shared with the VERTEX verbs; the distance column is the CHORD's, so a
distance typed back into an arc segment gives the arc that chord, not a
longer arc. The dock is made with the drafting toolbar by
`drawing::installDrawingUi` (`src/katana_qt/drawing/drawing_ui.hpp`), a
few lines in the window: Ortho (F8), Polar (F10), Tracking (F11), Bearings,
and toggles for the snaps the drawing system added, each bound to the
document's drafting settings and re-read on `DocumentChange::Drafting`, so a
verb that changes a setting checks its button. Edit Vertices shows the dock
when it starts.

Tested in `tests/cad/drawing/test_vertex_table.cpp` and
`tests/qt_widgets/drawing/test_vertex_panel.cpp`.

## The draw tools

The drawing system's draw tools (`src/katana_cad/tools/draw_professional.cpp`,
registered as `addDrawProfessionalTools`) join the Draw family's Lines and
Curves groups beside the tools already there; a "<Family>, <Variant>" name
puts a tool in a submenu (Draw > Ellipse, Draw > Circle, Draw > Arc).

| Tool (id, aliases) | Steps | Command |
|---|---|---|
| Polyline (`draw.polyline`, `PLINE`, `PL`, `POLYLINE`) | points; A switches to arcs, L back to lines, S takes an arc through a second point, C closes, U takes back | `CREATE_POLYLINE` |
| 3D Polyline (`draw.polyline3d`, `PLINE3D`, `3DPOLY`, `3DPOLYLINE`) | points as x,y,z, or at the height of what they snap to, or at the current height (H) | `CREATE_POLYLINE` |
| Construction Line (`draw.xline`, `XLINE`, `XL`) | a base point, then a point on each line; H and V fix the direction | `CREATE_XLINE` |
| Ray (`draw.ray`, `RAY`) | a start point, then a point on each ray | `CREATE_RAY` |
| Double Line (`draw.dline`, `DLINE`, `DL`) | points along the path; W sets the width, C closes | `CREATE_DOUBLE_LINE` |
| Freehand Sketch (`draw.sketch`, `SKETCH`) | click to put the pen down, click to lift it, Enter keeps the strokes weeded to the tolerance (T) | `CREATE_SKETCH` |
| Revision Cloud (`draw.revcloud`, `REVCLOUD`) | outline points, or R and two corners; A sets the arc length | `CREATE_REVISION_CLOUD` |
| Spline (`draw.spline`, `SPLINE`, `SPL`) | fit points, or control points (C, and F back); D sets the degree; Enter finishes | `CREATE_SPLINE` |
| Ellipse, Axis and End (`draw.ellipse`, `ELLIPSE`, `EL`) | both ends of one axis, then the other half-axis | `CREATE_ELLIPSE` |
| Ellipse, Centre (`draw.ellipse.centre`) | the centre, the end of one axis, the other half-axis | `CREATE_ELLIPSE` |
| Ellipse, Arc (`draw.ellipse.arc`, `ELLIPSEARC`) | as Axis and End, then the start and end angles | `CREATE_ELLIPSE` |
| Circle, Tangent Tangent Tangent (`draw.circle.ttt`) | three lines or polyline segments; the circle touching all three nearest the picks | `CREATE_CIRCLE` |
| Arc, Start End Direction (`draw.arc.sed`) | the start, the end, the direction it leaves the start | `CREATE_ARC` |

**Polyline arcs.** In arc mode each new segment is the arc that leaves the
previous segment's end tangent to it (the first segment's heading is the
first chord's), stored as the DXF bulge of that segment; a polyline with
any arc is made as a curve polyline, one without as a plain polyline
(the storage rule above). Close is refused, not half-done, while fewer than
three vertices stand.

**Heights.** Point and 3D Polyline keep a current height (H), remembered
from one use to the next. A typed x,y,z gives that vertex its own; a point
snapped onto a vertex, point or line end that carries a height takes it
(`cad::heightAtPoint`, of what the view draws: a hidden point's height is not
taken); otherwise the current height applies. The typed input router hands a
tool a height through `InteractiveTool::point3d`; a 3D polyline's
`@dx,dy,dz` puts the vertex at a height of dz, as the `PLINE3D` verb reads
it - the window's tool and the verb make one polyline of one line. (For a
while the tool climbed dz from the last vertex while the verb did not;
whether both should climb is under Not done.)

**Construction lines are layer-marked.** The model has no infinite line, so
a construction line or ray is a line `kConstructionReach` (100 km) long on
the "construction" layer, which the tool creates, in the same undo step, if
the drawing has none (`include/katana/cad/drawing/construction.hpp`). That
layer and every layer beneath it is drawn and snapped to on screen but
never plotted (`plan_painter.cpp` skips it on paper) and never counted in
the drawing's extents (`selection.cpp`'s `drawnExtent`), so Zoom Extents
frames the drawing, not the reach of an aid.

**Reviewed variants.** Circle already offered Centre Radius, Centre
Diameter, 2 Points, 3 Points and Tangent Tangent Radius; Tangent Tangent
Tangent completes the set. Arc already offered 3 Points, Start Centre End,
Centre Start End and Start End Radius; Start End Direction is added. Polygon's inscribed, circumscribed and edge forms were
complete and are unchanged.

Tested in `tests/cad/tools/test_draw_professional.cpp` and
`tests/cad/tools/test_draw_lines.cpp`.

## The command line

The drawing system's verbs (`src/katana_cad/drawing/drawing_verbs.cpp`,
members of `CommandInterpreter` routed by `isDrawingVerb`) give an agent
everything the grips, the tools and the panel give a person. They follow
the annotation verbs' three rules: options are `key=value` in any order
after the positional arguments, and an unknown key is refused naming the
known ones; replies are records of `key=value` pairs, one per line, numbers
exact; and every edit is ONE step through `Document::execute`, built by the
same functions the grips and the tools use (`polyline_vertices.hpp`,
`vertex_editing.hpp`, `vertex_table.hpp`, `draw_shapes.hpp`). HELP lists
them after the annotation verbs.

A target is an id (`12` or `#12`) or `SELECTION` (every selected
polyline). Vertex indices count from 0. Points are those of precision
input: `x,y[,z]`, `@dx,dy[,dz]`, `@distance<direction` with the direction
in the ANGLES convention or as a quadrant bearing; a z is a vertex's height.

| Verb | Arguments | Reply |
|---|---|---|
| `VERTEX LIST` | id | the polyline record (`id kind closed vertices arcs heights length`), then per vertex `index x y z bulge bearing distance` - bearing in whole-circle degrees, distance the chord, `none` where there is none |
| `VERTEX INSERT` | id p `[after=N]`; p may be `#id@x,y[,z]` (the place on the polyline nearest x,y - ON the line, as the window's Insert Vertex puts a click - with z its height) or `#id.sN` (segment N's middle) | the record and `inserted=` (the new index); without after, into the segment such a point names, else the nearest segment. A point of ANOTHER entity is that entity's point, put into the nearest segment; a segment the polyline has not got is refused as "that entity has no such point" (the sentence said "to dimension to", read by every verb that takes a point of an entity), and a point on a vertex as one |
| `VERTEX DELETE` | id N `[N...]` | the record and `deleted=` |
| `VERTEX MOVE` | id N p | the record and the vertex's; `@` is from the vertex, a relative dz changes its height |
| `VERTEX SET` | id N `x= y= z=\|none bulge= bearing= distance=` | the record and the vertex's; the fields apply in turn, as typed into the Vertices panel |
| `WEED` | target `tolerance= [keep=on\|off]` | a record per polyline with `before=`; keep (on by default) keeps survey-point vertices |
| `DENSIFY` | target `interval= [chord=]` | a record per polyline |
| `STRAIGHTEN` | id N N `[side=short\|long]` | the record; forward from the first N to the second, round through vertex 0 of a closed polyline when the second is lower, as it always walked; with `side=short` a closed polyline's side with fewer vertices between the two (a tie walks forward from the first), the window's rule, and with `side=long` the other side, its O (`cad::vertexRange`); `side=long` on an open polyline is refused |
| `CLOSE`, `OPEN` | target (none: the selection) | a record per polyline; `OPEN` takes `#ids` or `SELECTION`, since `OPEN directory` opens a project |
| `STARTVERTEX` | id N | the record |
| `VERTEXZ` | id N z or none, target `INTERPOLATE`, or id `GRADE` N N `[side=short\|long]` | the record(s); GRADE walks a closed polyline as `STRAIGHTEN` does |
| `PLINE` (`PL`, `POLYLINE`) | p p `[ARC p...] [LINE p...] [CLOSE]` | the new polyline's record; in ARC each point ends a tangent arc, and CLOSE in ARC closes with one |
| `PLINE3D` (`3DPOLY`) | p p `[p...] [CLOSE] [z=]` | the record; z= is the height of points given without one |
| `SPLINE` (`SPL`) | p p `[p...] [control=on\|off] [degree=3]` | `id kind layer` |
| `ELLIPSE` (`EL`) | centre axis-end `minor=\|ratio= [start=deg end=deg]` | `id kind layer`; start and end are eccentric anomalies |
| `XLINE` (`XL`), `RAY` | base `p [p...]`, or `XLINE` base `angle=dir [angle=...]` | one `id kind layer` per line, on the construction layer (made in the same step when missing) |
| `DLINE` (`DL`) | p p `[p...] width= [CLOSE]` | a record per side |
| `ORTHO`, `TRACKING` | `[on\|off]` | the drafting record |
| `POLAR` | `[on\|off] [increment=deg]` | the drafting record |
| `ANGLES` | `[ccw\|bearing]` | the drafting record |
| `LOCK` | `[angle=dir\|none] [length=d\|none]`, or `OFF` | the drafting record |
| `SNAP` (`OSNAP`) | `[on\|off] [modes=a,b\|all\|none] [add=] [remove=]` | the drafting record |
| `DRAFTING` | - | `ortho polar increment tracking angles anglelock lengthlock snap modes` |

The drafting verbs set the document's drafting settings, which the views
share, and notify `DocumentChange::Drafting`, so the drafting toolbar's
buttons follow; they are settings, not drawing edits, so not undo steps.
`PLINE` is the drawing system's in every form, the plain `PLINE p p
[CLOSE]` included, so that every one replies with the id of what it made.

Only an `OPEN` of a project replaces the drawing. Which one a line is,
`CommandInterpreter::replacesDocument` says, and every front end asks it:
the window before its "discard the drawing?" question and its reset of the
backdrop and views, the session before its missing-customisation check,
`katana_mcp` before its unsaved-changes guard. Before the merge each of them
took any `OPEN` for a project's, so `OPEN #12` in the window asked to
discard the drawing and then zoomed to its extents, and through `katana_mcp`
it was refused for unsaved changes.

Tested in `tests/cad/drawing/test_drawing_verbs.cpp`.

## The merge into main

The drawing system was written on its own branch from main at 88b046b; main
gained smart leaders, the one scope grammar, the utility tools on scope, the
one executor for dialog lines and IFC 4.3 meanwhile, all written before these
kinds existed. The merge (2026-09-26) made each of them take the new kinds
or refuse them by name, never pass over them
(`tests/cad/drawing/test_new_kinds_in_main.cpp`, the IFC and scope-widget
tests named below):

| Where | What a curve polyline, ellipse or spline now does |
|---|---|
| anchors (`entity/anchor.hpp`) | a curve polyline offers Along (on an arc segment, the fraction of its sweep - ON the arc) and Inside when closed; an ellipse Along (the fraction of its sweep of eccentric anomaly) and Inside when whole; a spline only its Start and End - its parameter is not its length, so no fraction of it would stay where a note was put |
| smart leaders' values (`leader_values.cpp`) | a curve polyline gives a segment's or an arc's values where the tip is, chainage along it, level from its vertices' heights, length, vertices, and area and perimeter when closed; an ellipse its length, and area and perimeter when whole; a spline its length |
| LEADER FOR and Attach (`leader_edit.cpp`), the Leader tool's pick | inside a closed curve polyline or a whole ellipse, else halfway along; a spline at its start; the tool's tip can be put on a curve polyline and an ellipse |
| `#id@x,y`, `#id.alongN:t` | as a polyline for a curve polyline; a spline refused, saying to use its start or end |
| labels (`label_values.cpp`) | a curve polyline takes Segment labels (an arc segment as an arc), Arc labels on its arc segments, and an Area label when closed - its area exact |
| snaps that name a point (`snapAnchor`) | a curve polyline's vertices and segment middles, an ellipse's centre and ends, a spline's ends |
| `#12` with no part (`defaultAnchor`) | a curve polyline's or a spline's start, an ellipse's centre |
| the label keep-out (`labelKeepOut`) | a curve polyline's sides and chorded arcs; an ellipse's and a spline's chords of a 256th of their extent |
| `AREA` | a closed curve polyline and a whole ellipse, exactly |
| `PARCEL` and Survey > Parcel Report | a curve polyline refused as "a parcel with arc courses is not reported yet" (below) |
| `TYPE=` in the one scope grammar, `SELECT TYPE` | the names in any case (`entityTypeFromString`); `TYPE=CurvePolyline` was refused, title-cased to `Curvepolyline` |
| the "Only those that match" type boxes (`ScopeFilterWidget`) | a box for every kind, counted off the variant; the list stopped at Dimension |
| the utilities (`utility_data.cpp`) | a service run given an arc is still its line's run; a design centre line may be a curve polyline, chorded within a millimetre with its vertices' heights |
| IFC export (`katana_ifc/drawing.cpp`, `classification.cpp`) | a curve polyline one `IfcIndexedPolyCurve` with an `IfcArcIndex` through each arc's true middle; an ellipse `IfcEllipse`, trimmed by parameter for an arc; a spline `IfcBSplineCurveWithKnots` (rational with weights); a curve polyline or spline on a kerb, pipe or fence layer is that element; a rules file may name the three kinds |
| Cut Section along the selection | along a curve polyline's or a spline's chords |
| Alignment Manager, PIs from the selection | a curve polyline refused: its vertices are tangent points, not PIs |
| the object snap in the window | one owner, the document's drafting settings: View > Snap Modes, the drafting toolbar, `SNAP` and new views read and write the same, where the views had kept a copy that a click in the menu or a new view wrote back over the toolbar's modes; `SNAP` with `modes=`, `add=` or `remove=` typed in the window is the drawing verb (`SNAP <mode> ON\|OFF` stays the window's shorthand) |
| the Properties panel's Geometry group (`property_panel.cpp`) | a curve polyline its vertices, closed, arcs, heights, length and area when closed; an ellipse its centre, radii, rotation, sweep and length; a spline its degree, control and fit points and length. Main moved these rows out of `main_window.cpp` into the tree panel while this branch was open, and the second merge of main (after PR #9 and #11) carried the three kinds across |
| View > Panels > Vertices | Edit Vertices' icon, since main's `--check-menus` now requires an icon and a status tip on every item and that tool opens this panel |

## Not done

**Vertex editing (2026-09-30), what "What a vertex tool acts on" leaves.**

- A grip's shortcut menu is reached with the mouse only: no headless step
  right-clicks (`+` is a left click), so its items are tested on the menu
  itself (`tests/qt_widgets/test_plan_context_menu.cpp`) and each is a verb
  line or a tool an agent reaches directly.
- The Vertices panel is not joined to the view: choosing a row does not
  make that vertex hot, a hot grip does not choose the row, and its Insert
  After with no row chosen inserts after vertex 0 (`vertex_panel.cpp`).
- The whole-polyline tools (Weed, Densify, Close or Open, Interpolate, Merge,
  Snap to Grid) show nothing before Enter and report no counts ("removes 37
  of 412 vertices"); Close or Open does not say which it did.
- The Draw > Vertices submenu is one list of 18, in no sections, and the
  family sorts after every named group of the Draw toolbar.
- A grip's drag is drawn as the whole edited polyline, dashed, with no mark
  where the vertex goes (`cad::gripFeedback` takes `gripPreview`); Move
  Vertex draws only the two segments that change and a disc at the new
  place. The two should look alike.
- The caption chip keeps off every mark, piece and label, and the view's
  edges and band, but not the polyline in play's own lines: on a small or
  continuing string it can still lie over a segment no mark is on. Where
  every corner round the cursor covers something - a caption wider than
  the room left of the cursor, a piece crossing all four - it takes the one
  over the fewest.
- With the pointer off the view (over a menu or a toolbar) no preview is
  drawn at all, so a tool started from the menu with a vertex chosen shows
  nothing of the chosen vertex until the pointer is back over the view:
  the preview is a function of the cursor, and a tool has no way to say
  what it would show with none.
- A 3D polyline's `@dx,dy,dz` puts the vertex at a height of dz, in the
  window as in `PLINE3D`; Move Vertex's (and `VERTEX MOVE`'s) raises the
  vertex by dz. Relative in all three is the drafting convention, and the
  one the vertex tools follow, but making Polyline 3D climb means changing
  what an existing `PLINE3D` line draws, on every surface at once - the
  owner's call, not a review fix's. The tool alone was made to climb for a
  while, and the window and `katana_cli` drew two polylines from one line.
- Labels were measured in the headless (offscreen) renderer only: its
  DemiBold "1" reads at 4.9:1. The window's own renderer was not measured.
- Colours still shift meaning in one place: a hot grip is red, the colour a
  tool's preview gives what goes, and turns green once a tool takes it; and
  Move Vertex's new place wears the Added "+" though nothing is added.
- The Delete key on hot vertices (`cad::deleteHotVertices`) and Delete
  Vertex's several-vertex plan each group the vertices by polyline before
  `geometry::deleteVertices`. They differ on purpose - the key deletes the
  drawing's vertices as they are now (a hot grip follows its vertex), the
  tool what it previewed, refused when stale - but the grouping could be
  one helper.
- `#12.v3` typed at a tool's prompt in the window is read as a comment
  (`main_window.cpp`); a pick is typed as `x,y` or a vertex number instead.
- Trim, Fillet, Offset and Leader still pick with the aperture their tool
  was made with and no view's hidden layers (`ToolContext::pickTolerance`),
  so their previews can disagree with the click after a zoom (`pickUnder`
  would close it).
- The Properties panel numbers vertices from 1; the prompts, the captions,
  the panel and the verbs from 0.
- A vertex pick falls back to the view's pick of the NEAREST polyline: of
  two strings under the cursor, the one passing nearer is taken even when
  the other's vertex is the one in reach. The selected polyline is always
  preferred, so selecting the string meant settles it.
- A selected polyline on a locked layer is said to be locked before the
  prompt, but an unselected one under the cursor is passed over in silence,
  as the view's pick passes over it for every tool.
- The dense string's focus squares are thinned by a fixed 10 px, not by
  what is under the cursor: the square of the vertex nearest the cursor may
  be one left out.

**The one scope and filter (the contributors' contract, section 1.1).** The drawing verbs
predate the shared grammar and take `target = id | SELECTION` only - not
`VIEW`, `DRAWING`, `AREA x0,y0,x1,y1`, `LAYERS a,b [ONLY]` or `WHERE`:
`WEED`, `DENSIFY`, `CLOSE`, `OPEN` and `VERTEXZ ... INTERPOLATE`, which act
on whole polylines and so should take a scope. Their window tools (Draw >
Vertices: Weed, Densify, Close or Open, Interpolate Heights, Merge Near
Vertices, Snap Vertices to Grid) act on what is picked or selected, with no
"Apply to" / "Only those that match" controls (`ScopeFilterWidget`), and none
says what its scope took. `VERTEX`, `STRAIGHTEN`, `STARTVERTEX`, `VERTEXZ id
N` and `VERTEXZ id GRADE` name vertices of ONE polyline, where a scope has
nothing to choose. The retrofit is to read the target with
`parseScopeWords` / `resolveScope` and filter to polylines.

**The three surfaces (the contributors' contract, section 1).**

- The Vertices panel (`src/katana_qt/drawing/vertex_panel.cpp`) executes
  `cad::editPolyline` commands itself; it does not build `VERTEX SET` /
  `VERTEX INSERT` / `VERTEX DELETE` lines and hand them to
  `MainWindow::runVerbLine`, so its edits are not logged as lines.
- Grip drags and Delete on hot grips (`grip_controller.cpp`) execute
  `gripDragCommand` / `deleteHotVertices` directly; a vertex grip's drag is
  `VERTEX MOVE`, but the other handles (an arc segment's middle, a centre,
  a quadrant, a spline's fit points) have no verb.
- The drafting toolbar sets `Document::drafting()` itself rather than
  running `ORTHO`, `POLAR`, `TRACKING`, `ANGLES` and `SNAP` lines.
- No verb at all, so no `katana_cli` or `katana_mcp` path: Fillet Vertex,
  Chamfer Vertex, Merge Near Vertices, Snap Vertices to Grid, Freehand
  Sketch, Revision Cloud.

**The merge's own.**

- `PARCEL` refuses a lot with arc courses rather than chording it: a legal
  description's arc course is radius, arc length and chord, which
  `parcelReport` does not yet write.
- A leader can be put along a curve polyline or an ellipse, not a spline.
- The IFC import makes none of the three kinds: it reads an
  `IfcIndexedPolyCurve`'s arcs, a trimmed `IfcEllipse` and an
  `IfcBSplineCurveWithKnots` (rational or not) as chords within its curve
  tolerance into a `Polyline2`. (Before the merge it read an elliptical arc
  as the whole ellipse, and a B-spline as a point at 0,0.)
- The Dimension tool's pick (`annotate_dimension.cpp`) takes a line or a
  polyline's straight segment, not a curve polyline's: once a polyline has
  an arc, its straight sides are dimensioned by points (`DIM` with
  `#id.vertexN`), not by picking the side.
- View > Snap Modes lists the eight original modes; the drawing system's
  five (Quadrant, Node, Extension, Parallel, Apparent Intersection) are on
  the drafting toolbar and `SNAP add=`. Both show the one setting, the
  document's drafting settings.
- The Alignment Manager cannot take PIs from a curve polyline (the tangent
  points of its arcs are not PIs); it says so.
