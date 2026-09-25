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
