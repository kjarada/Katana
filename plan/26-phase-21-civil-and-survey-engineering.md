<!-- Katana plan, section 26 of 47. Index: ../PLAN.MD. Previous: 25-phase-20-file-interoperability/03-20-3-survey-coding-programme.md. Next: 27-phase-22-drawing-and-plotting.md -->

# 26. Phase 21 — Civil and Survey Engineering

**STATUS: PARTIALLY DELIVERED — profiles and cross sections only.**

`katana::cad::extractSection` (`include/katana/cad/section.hpp`) cuts any number
of named surfaces along a plan alignment and reports elevation against station,
plus the drawing entities that cross it. `crossSectionLine` builds the
perpendicular at a station (using the bisector at a vertex, so a station landing
exactly on a bend has a defined direction), and `sectionStations` produces the
stationing without accumulating rounding.

The part that makes it engineering rather than a chart: samples are placed at
every alignment vertex, at every interval, **and at every station where the
alignment crosses a TIN triangle edge**. A fixed interval alone cuts the top off
every ridge — measured on a test roof, a 30 m interval reports a peak of 8.0
where the true ridge is 10.0, a 20% error in the highest point on the profile.
Crossings are found by bisecting between consecutive samples whose located
triangle differs; the alternative, walking the triangulation by adjacency, is
asymptotically better but is derailed by holes and by an alignment that leaves
and re-enters the surface, both of which are constant in real survey data.

Shown by `SectionViewWidget` in any tiled viewport cell: axes on round values,
station and elevation readout under the cursor, gaps drawn as gaps where the
alignment leaves the surface, slope breaks marked, crossings ticked, vertical
exaggeration.

**OUTSTANDING:** everything below - alignments with horizontal and vertical
geometry, profiles as design objects, corridors, parcels and grading.

**Where an alignment lives, decided before it is built.** An alignment is a
**named table in `entity::Model`**, beside `layers`, `linetypes` and
`dimensionStyles` - it is **NOT** an eighth `entity::Geometry` alternative.
Three reasons, in order of how expensive getting it wrong would be:

1. `docs/model.md` lists the places a new geometry kind must be threaded
   through, and separates the ones the compiler catches from the ones that
   **fail silently**. `cad::section.cpp appendCrossings` is on the silent list.
   The alignment is the thing sections are cut *along*. Adding it as a geometry
   kind would therefore silently degrade the one part of this phase that is
   already delivered and tested.
2. An alignment is not a shape with a position; it is a *definition* that a
   shape is derived from. It is referenced by name by profiles, corridors and
   cross-section sets, exactly as a TIN surface is - and `TinSurface` is
   already a named dataset rather than a geometry kind (`grep -c TinSurface
   include/katana/entity/entity.hpp` -> 0). Following that precedent costs
   nothing; inventing a second pattern costs the next contributor.
3. Undo is a before-image copy of a whole entity. An alignment edited one PI at
   a time would copy the entire horizontal geometry per keystroke.

**First slice DELIVERED: `katana::geometry::Spiral2`**
(`include/katana/geometry/spiral2.hpp`), the clothoid, in the geometry layer
beside `Arc2`. Kept in its general form - curvature from k0 to k1 over L -
because a real alignment also needs the transition between two curves and
the reverse spiral, and all three are one object. Evaluated by composite
Gauss-Legendre rather than the textbook power series, because the series is
exact only for the from-a-straight case and the general one has no closed
form; on half-radian panels the quadrature is exact to below 1e-20 relative.
Tested against the published Fresnel integrals (Abramowitz & Stegun 7.7) to
1e-14, the hand-derived series on an R = 300 m / L = 90 m transition, and an
independent adaptive-Simpson integration for the cases that have no series;
a right-hand spiral is the EXACT reflection of the left-hand one, asserted as
equality. Decision record in `docs/geometry.md`.

**Second slice DELIVERED: `katana::geometry::HorizontalAlignment` and
`solveAlignment`** (`include/katana/geometry/alignment.hpp`). An alignment is
defined by its PIs - each with a radius and spiral lengths - and the tangents,
spirals and arcs are DERIVED, so continuity holds by construction and a
definition that cannot be built is refused naming the PI. The tangent length
for unequal spirals, which the design texts do not give, was derived by
placing the shifted circle and checked by closing the chain independently in
Python (2e-14 m off the forward tangent); it reduces to the published
`T = k + (R + p) tan(D / 2)` for equal spirals and to `R tan(D / 2)` for
none. `SolvedAlignment` answers point, direction, curvature and left-offset
at a station, lists its key stations (TS, SC, CS, ST), and chords itself to
a `Polyline2` through the one sagitta rule - which is what lets a section be
cut along it with `extractSection` untouched, exactly as intended above.
Decision record in `docs/geometry.md`.

**Third slice DELIVERED: alignments in the application.** `entity::Alignment`
is a named table (the third on `NamedTable`) holding the PI definition only;
`add()` solves it, so an alignment that cannot be built is refused naming its
PI - including one arriving from a file, proved by a test that hand-edits a
saved row. Schema 7 stores `alignments` and `alignment_pis`. The `ALIGN` verb
defines, edits and prints a setting-out table that always carries the key
stations; the viewport draws every alignment as an amber overlay with a tick
and chainage label at each TS, SC, CS and ST; and Terrain > Cut Section Along
Alignment... cuts a section down one by name through the same routine the
selection path uses. `samples/site_plan` carries one. Records in
`docs/geometry.md` and `docs/cad.md`.

**Fourth slice DELIVERED: `katana::geometry::VerticalAlignment` and
`solveProfile`** (`include/katana/geometry/profile.hpp`), the design grade
line. Defined by PVIs - station, elevation, curve length - with symmetric
parabolic vertical curves, the tangents and curves derived, continuity at
every joint holding by the identity `z_PVT = z_PVI + g2 L / 2` rather than by
adjustment, and high and low points in closed form. Tested against the two
standard textbook curves, worked by hand and confirmed by direct evaluation
and a fine scan: sag low point 1020 @ 101.2, crest high point 550 @ 148.0.
Asymmetric curves are deliberately not provided. Record in
`docs/geometry.md`.

**Fifth slice DELIVERED: profiles in the application.** `Alignment::vertical`
holds the design profile as an `optional`, validated by solving so an
unbuildable one is refused naming its PVI - from the command line or from a
file. Schema 8 stores `alignment_pvis`; an alignment with no rows comes back
without a profile, proved by a test, because an empty one would fail to
solve and refuse the whole load. `ALIGN DESIGN` defines a profile in one
command (a profile cannot be grown from a single PVI, which the model
refuses - recorded in `docs/cad.md`), `PVI` appends, `PROFILE` prints it with
high and low points. `cad::appendDesignProfile` adds the design to a section
as a series sampled at every ground station plus the profile's key stations,
and Cut Section Along Alignment... passes it through, so the section view
shows design against ground with no change to the view. `samples/site_plan`
carries one: a sag whose low point the command reports at 76.226 @ 50.723.

**Sixth slice DELIVERED: corridor quantities.** `cad::corridorQuantities`
sets a template - half width, crossfall, cut and fill batters - across the
alignment at the design elevation, carries its edges to daylight by a march
and bisection against the TIN, and sums cut and fill by average end area with
the compensated sum, always sectioning the profile's key stations. A section
that cannot reach the ground contributes nothing and is counted, and the
report says the totals are not the whole job. Terrain > Corridor
Quantities... takes the assembly from a dialog and shows the schedule. Tested
against hand-worked sections checked independently in Python: 10, 11.68 and
10.75 m^2, and area-times-length exactly along a straight. Record in
`docs/cad.md`.

**Seventh slice DELIVERED: the corridor as a surface.** `cad::corridorSurface`
triangulates the template strings as breaklines with the daylight lines as
the boundary; Terrain > Corridor Surface... adds it to the scene, visible in
3D and in sections. It makes an independent check of the quantities
possible, and the tests make it: `compareSurfaces` of the corridor against
the ground (an exact overlay) and the end-area total (a different method
entirely) agree on 2000 m^3 to 1e-6 on a straight. Both commands share one
dialog. Record in `docs/cad.md`.

**Eighth slice DELIVERED: parcels.** `cad::parcelReport` gives the courses of
any closed polyline as quadrant bearings to the second and distances to the
millimetre, with area, perimeter and centroid; `legalDescription` writes the
deed wording; `parcelLabels` places text along each course, flipped so none
reads upside down, on the inside of the boundary, created in one undo step
by `PARCEL id LABEL`. Nothing is stored - a parcel is its boundary, computed
on demand. Built entirely on `katana::survey` (azimuths, bearings, DMS,
inverse, area, centroid); the one conversion from (x, y) to (northing,
easting) is in one function and the tests check bearings, not only areas,
because that is what an axis swap would break. Record in `docs/cad.md`.

**Ninth slice PARTIALLY DELIVERED: grading, the engine.** `cad::gradeToSurface`
takes a feature line - a closed pad at its design heights, or an open line
graded on one side - and carries a batter of the given cut or fill slope
outward from every vertex and every interval along every edge until it meets
the ground, by the corridor's march-and-bisect, then triangulates the feature
line, the daylight line and every batter as breaklines into the grading
surface and measures the earthwork against the ground with the exact overlay.
At a vertex the batter runs along the bisector and is FLATTER by the cosine
of half the corner, so the daylight corner is where the two batter planes
meet - the mitre a machine builds - not a chamfer 29% short. Tested against
hand-worked frustums (a 10 m pad 2 m up at 1 in 2: an 18 m daylight square
and 402.667 m^3, exact) and a sloping ground where each side daylights at its
own distance. **OUTSTANDING:** no way to reach it from the application yet
(Terrain > Grade Pad... is the intended command: select a closed polyline,
give elevation and slopes, get the surface and the daylight polyline);
rounded corners; benches; grading to a fixed width or elevation rather than
to a surface. Set aside on 2026-09-22 for the 12d programme in section 25.2,
which takes precedence over everything else in this plan until it is done.

**OUTSTANDING in this phase:** richer assemblies (kerbs, verges, benches,
subgrades) as a table of their own; the prismoidal correction, deliberately
not applied; grading in the application (above). `deleteAlignment` still has
no in-use guard because nothing references an alignment by name.

## Alignments

```text
tangent
circular curve
spiral
stationing
offsets
```

## Profiles

```text
existing ground
design profile
vertical curves
station/elevation
```

## Corridors

```text
alignment
profile
assembly
surface
cross sections
```

## Parcels

```text
boundaries
bearings
distances
areas
labels
legal descriptions
```

## Grading

```text
feature lines
pads
slopes
daylight
grading surfaces
```

---

