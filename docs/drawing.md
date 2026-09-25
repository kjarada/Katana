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
against bytes written by hand).

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
| vector export | chords, heights interpolated | chords | chords |
| surface from drawing | chords with interpolated heights, as breaklines | - | - |
| move, rotate, scale, mirror | exact (a mirror negates the bulges) | exact (a mirror re-expresses the sweep) | exact |
| stretch | vertices in the window move, arcs keep their bulge | whole, when its centre is inside | fit (or control) points move, solved again |
| offset | concentric arcs, mitre or round joins (`geometry::offset`) | refused | refused |

What is not done: trim, extend, break, lengthen and join of a
`CurvePolyline2` (each still takes `Polyline2` only); the archive IMPORT
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
inserting, Delete and Esc, snapping to an endpoint and ortho.

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
| `x,y` / `x,y,z` | absolute; a z is a height for a tool that takes one (`InteractiveTool::point3d`) |
| `@dx,dy` / `@dx,dy,dz` | relative to the last point |
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
cross.

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
