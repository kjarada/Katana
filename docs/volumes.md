# Volumes

Earthwork quantities between the drawing's surfaces: surface to surface,
surface to a level, stockpiles and pits, along an alignment by cross
sections, on a grid, inside polygons, and the cut/fill map. The owner's
request of 2026-10-15 asked for "volumes: all kinds". Volumes are what an
earthworks estimate, a tender and a claim are paid on, so every method here
is exact where it says it is, says which method it is in every reply, and is
held in the tests to a solid whose volume is known in closed form or to an
independent method.

The terrain layer does the arithmetic (`include/katana/terrain/overlay.hpp`,
`include/katana/terrain/volume.hpp`, `include/katana/terrain/section_volume.hpp`);
the verbs are `katana_cad`'s (`include/katana/cad/terrain/volume_verbs.hpp`),
so the window, `katana_cli` and `katana_mcp` run the same code; the window's
Terrain > Volumes only builds their lines
(`src/katana_qt/terrain/volumes_dialog.hpp`).

## The verbs

```
VOLUME SURFACES <base> <top> [INSIDE <polygon> [EACH]] [bulk=<f>] [shrink=<f>]
       [csv=<file> [OVERWRITE]]
VOLUME DATUM <surface> <level> [INSIDE <polygon> [EACH]] [bulk=] [shrink=] [csv=]
VOLUME STOCKPILE <surface> INSIDE <polygon> [base=boundary|lowest] [level=<z>] [EACH] [csv=]
VOLUME SECTIONS <base> <top> ALIGNMENT <name> [interval=10] [from=] [to=] [left=] [right=]
       [method=endarea|prismoidal|both] [bulk=] [shrink=] [csv=]
VOLUME GRID <base> <top> cell=<m> [INSIDE <polygon>] [bulk=] [shrink=] [csv=]
SURFACE DIFFERENCE <base> <top> [NAME <new>] [PREVIEW]
SURFACE ENVELOPE <a> <b> MAX|MIN [NAME <new>] [PREVIEW]

<polygon> := #id [#id ...] | x,y[,z] x,y[,z] x,y[,z] ... | FROM <scope> [WHERE k=v ...]
```

`HELP VOLUME` prints the same (`volumeVerbHelp`), and the window's Command
Reference has it as its Volumes section. Words are case-insensitive; a
surface or alignment name with a blank is quoted. The `key=value` options
are read wherever they stand - none is a WHERE key, so one may follow a
scope's filter - and a key the form does not take is refused by name, never
ignored.

**Cut and fill are read one way everywhere.** The first surface named is the
existing one (the base), the second the design (the top): fill is where the
top is above the base, cut where it is below, and `net = fill - cut`
(positive: material to bring in) in every record. That is `compareSurfaces`'
reading, which the corridor quantities share. A level is the design for
DATUM, so what stands above it is cut and what lies below is fill
(`volumeToDatum`'s "level design" reading); for STOCKPILE the level, or the
base made from the toe, is the reference, so a pile is cut - the volume that
removes it - and a pit is fill. DATUM and STOCKPILE add `above=` and
`below=`, which say the same without the earthworks reading.

**Replies.** One record per line (`docs/cad.md`), numbers to the millimetre
(cubic or square, as the DEM tools give theirs), a quantity of nothing never
signed:

```
volume method=exact base=ground top=design cut=12.345 fill=100.000 net=87.655 area=400.000 cut.area=95.000 fill.area=305.000
```

With polygons, first what the scope took (`boundary scope= ... matched=`,
the shared scope record), then
`polygons used= skipped.open= skipped.other=`, then with EACH one `polygon
index= entity= polygon.area= ...` record per polygon (`chorded=yes` when an
arc was chorded), then the total with `polygons=` and `polygon.area=`.
`area=` is always the plan area measured - where both surfaces (or the
surface and the polygon) have ground - and `polygon.area=` the polygons'
own, so a polygon reaching off a surface shows it. SECTIONS gives one
`section station= [key=yes] cut.area= fill.area= width=` record per section,
each after the first with its panel's `cut= fill=`, the running `cut.total=
fill.total=` and the mass haul ordinate `mass=`, then one `volume
method=endarea` and/or `volume method=prismoidal` total.

**What changes.** The VOLUME forms change nothing. `csv=<file>` writes the
records as a table (RFC 4180: CRLF, a field with a comma or quote quoted),
refusing a file already there unless OVERWRITE, as every verb that writes a
file does. SURFACE DIFFERENCE and ENVELOPE add a surface to the drawing
through `Document::setSurfaces`: one undo step, named after the verb; with
PREVIEW they say what they would add and add nothing.

**Refused, saying why:** a surface or an alignment the drawing has not got
(NotFound); surfaces with no ground in common, or polygons with nothing of
it inside (InvalidGeometry, naming both surfaces' extents); an `#id` that is
no polygon - not a closed polyline, curve polyline, circle or ellipse
(InvalidGeometry, saying what it is); fewer than three typed corners; EACH
without INSIDE; a factor that is not more than 0; polygons that overlap
without EACH. A scope that takes no closed
polygon is said - `polygons used=0` and `ran=no` - not refused (`docs/cad.md`, "Scope and filter").

## One overlay

Every volume between two surfaces integrates their difference, and the
difference is linear only where both surfaces are: inside one triangle of
each. `terrain::visitOverlay` cuts the common ground into such pieces - each
the intersection of a triangle of each surface, a convex polygon of at most
six corners, fanned from its first corner - and hands each to a visitor with
both surfaces' heights at its corners. It is the pair clipping
`compareSurfaces` already did (`docs/terrain.md`, "The fix: clip triangle
pairs"), moved out so the volumes inside a polygon, the stockpiles and the
difference and envelope surfaces cut the ground the same way.
`compareSurfaces` is now a visitor of it, with the same arithmetic in the
same order: its results are the bits they were. Nor did it slow down:
`BM_CompareSurfaces` (`benchmarks/bench_terrain.cpp`) in Release on the
24-thread development machine, five repetitions each, the two builds run
alternately twice on 2026-10-15, gave a median of 51.5 and 51.9 ms before
and 50.0 and 50.6 ms after, and `BM_CompareSurfacesSerial` 491 and 508 ms
before and 504 and 507 ms after - each within the spread of its own runs.

**The same bits at any thread count.** The base surface's triangles are cut
into blocks of a fixed `kOverlayBlockTriangles` (256), and a block's pieces
arrive on one thread in a fixed order: base triangle ascending, the top
triangles it meets ascending, then a region's triangles ascending, then the
fan. A visitor that sums each block into its own slot and combines the slots
in block order is deterministic, which `compareSurfaces`, the region
comparison and the difference surface are held to
(`VolumeInsidePolygon.GivesTheSameBitsOnOneThreadAsOnMany`,
`DifferenceSurface.IsTheSameSurfaceAtAnyThreadCount`).

**Joining the pieces into a surface.** A difference or an envelope is a TIN
made of the pieces, so a corner two pieces share must be one vertex. Each
corner is named by what it is - a vertex of one surface, or the crossing of
an edge of one with an edge of the other - and a crossing is placed from the
two edges' own end points in a fixed order, never from the corners of the
polygon being clipped, so the pieces on either side of an edge compute it to
the same bits. Two names for one place (a base vertex where the top has one
too, common where a design was built on the ground's own points) are welded.
The joined mesh goes to `TinSurface::create`; if floating point has left two
pieces overlapping by a sliver, which `create` refuses as two triangles along
one edge in the same direction, the same vertices and every piece edge go to
the constrained triangulation instead, whose exact predicates settle it, and
the triangles outside every piece are left out again (`retriangulated=yes`
in the reply). `DifferenceSurface.MadeThroughTheTriangulationItMeasuresTheSame`
asks for that path and holds its volumes to the direct one's.

## Inside a polygon

`terrain::OverlayRegion` is a polygon and its holes triangulated once - a
constrained Delaunay triangulation of its own vertices, so its triangles tile
it exactly and add no point - and the overlay's pieces are cut again by those
triangles. The polygon's edges are therefore followed exactly, through the
middle of triangles, not by whole triangles: a 1 m slab under a triangle
whose edges cross both surfaces' diagonals measures the triangle's shoelace
area, 2088.635 m3
(`VolumeInsidePolygon.TheEdgesOfAPolygonCutTrianglesWhereTheyLieNotByWholeTriangles`).

- **The polygon** is an `#id`, typed corners, or what a scope takes, read
  by the one reader of the drawing's polygons that the surface edits use too
  (`include/katana/cad/terrain/drawing_shapes.hpp`, which says what each kind
  of entity gives); `readVolumePolygons` only words its refusals. A closed
  polyline or curve polyline, one drawn back to its first vertex without
  being closed, a circle and a full ellipse are polygons, each at its
  heights; lines, arcs, elliptical arcs, open polylines and outlines of
  fewer than three corners are counted as `skipped.open` (they bound no
  area), and points, text and splines as `skipped.other`. An arc, a circle
  and an ellipse are chorded within `geometry::kCurveChordTolerance` (1 mm
  sagitta), and the record says `chorded=yes`.
- **One reader, the more capable rule.** The volumes and the surface edits
  were written side by side with a reader each, and the two had come to
  disagree: these verbs refused a full ellipse and a polyline drawn back to
  its start that the edits took, and the edits dropped a polygon's heights
  and refused FROM with no scope word, which these took as the selection.
  Where they differed the shared rule is the one that takes more, since each
  refusal was an accident of which reader was written, not a reason: so a
  polygon here may now be an ellipse or a polyline drawn back to its start
  (`VolumeVerbs.AFullEllipseAndAPolylineDrawnBackToItsStartAreVolumePolygons`).
  A closed outline of fewer than three corners, which used to be taken and
  then refused by the overlay, is now counted with what bounds no area.
  Rejected: keeping `VolumePolygon` as a thin adapter over the shared shape,
  which would have kept a second representation of the same polygon.
- **A volume to a level inside a polygon** compares the surface with the
  region made flat at the level (`OverlayRegion::atLevel`): the same
  overlay, so the same exactness.
- **Several polygons** are measured one at a time and summed. Two that
  overlap would count the ground they share twice: without EACH that is
  refused, naming them and the area; with EACH each has its own record and a
  `warning overlap=` record says the total counts it twice. The overlap is
  found by the overlay itself - a 1 m slab over one polygon cut by the other
  - and anything under a square millimetre is rounding, so polygons sharing
  an edge (cadastral lots) are not refused.
- **A polygon partly off the ground** measures the ground under it, and
  `area=` against `polygon.area=` says how much that was
  (`VolumeToLevelInsidePolygon.APolygonPartlyOffTheSurfaceMeasuresTheGroundUnderIt`).

## Stockpiles and pits

`terrain::stockpileVolume` measures a surface inside a boundary against a
base made from the boundary:

- **`base=boundary`** (the default): the boundary's heights triangulated
  inside it, the toe of the pile as it stands. The boundary's own height at
  every vertex when it has one at each (a 3D string surveyed along the toe,
  or typed `x,y,z` corners); otherwise the surface's at each vertex. A vertex
  off the surface with no height of its own is refused, naming it: its height
  would be a guess, and a base at 0 there would add a cone of nothing.
- **`base=lowest`**: a level plane at the lowest point of the surface along
  the boundary - its vertices and every place an edge of the surface crosses
  it, read exactly by the segment profiler below.
- **`level=<z>`**: a level plane at z.

A pile of facets on level ground is a pyramid, a third of its base times its
height: twelve faces on a toe of radius 20 hold (12/2) 20^2 sin 30 deg x 9 /
3 = 3600 m3. On sloping ground the toe's base follows the slope and the
answer does not move, because a pyramid's volume is its plan area times its
height plumb from the base plane. 360 faces come within 0.19 m3 of the cone's
pi r^2 h / 3, the circle's area beyond the polygon's
(`Stockpile.ManyFacetsApproachTheConeFormula`).

## Along an alignment

`terrain::volumesAlongAlignment` cuts cross sections square to an alignment
and takes the volume between consecutive ones from their areas, the way a
road job is measured and paid.

- **Stations:** every whole multiple of `interval=` (10, 20, 30 ... for 10,
  whatever the start), the range's ends and the alignment's key stations (TS,
  SC, CS, ST - `SolvedAlignment::keyStations`), where the geometry changes
  and a section is always cut. Two closer than `kCoordinate` (0.1 mm) are
  one. More than 100 000 sections is a unit mistake and refused, the cap
  `terrain::contours` puts on its levels.
- **Reach:** `left=` and `right=` from the centreline; absent, the diagonal
  of the ground the two surfaces share, so a section reaches across all of
  it wherever it is cut. That is right on a straight run and wrong anywhere
  the alignment turns: see "Sections that cross" below, which refuses it.
- **Sections that cross are refused.** The average end area takes the volume
  of a panel from the areas of the two sections that bound it, and is right
  only while the panels tile the ground. Where two sections cross - the
  inside of a curve cut deeper than its radius, where they all pass through
  its centre; the inside of a corner (a PI with no radius) deeper than the
  interval over the sine of its deflection; two straight legs at an angle,
  each swept by sections that reach across the other - the ground beyond the
  crossing is swept by both and counted by both. A 1 m slab holding 40 000 m3
  measured 62 101 m3 round a curve of radius 40 with the default reach, and
  62 101 again at an interval of 0.1 m: a shorter interval does not help,
  because the sections cross at the centre however close they are. The
  verbs now refuse it (InvalidArgument), naming the first two stations that
  cross and the longest reach at which none do - for a curve its radius -
  and measure nothing. `left=15 right=15` over the same curve gives
  30 x (240 + 20 pi) = 9084.956 m3, the band's width times its centreline's
  length
  (`VolumesAlongAlignment.SectionsAcrossACurveDeeperThanItsRadiusAreRefusedNotCountedTwice`).
  Sections that only meet at their ends (a reach equal to the radius, which
  ends every section of the curve on its centre) are not crossing; the test
  is a proper crossing by more than `kGeometric` at both ends. In the window
  the Left and Right fields are blank for "across the common ground", so a
  person on a curve gets the refusal in the status line and types a reach.
- **A section on a surface's boundary is whole.** A corridor's finished
  surface ends on its first and last sections, and a person measuring it along
  its own alignment cuts exactly there. The ends of a section are computed
  from the bearing, so on a north-going alignment they are tilted by
  cos(pi/2) = 6e-17 of the reach, 9e-15 m at 141 m, and an exact test keeps
  whichever half of the segment the tilt leaves inside the edge. Both ends of a
  segment within `kGeometric` (0.1 micrometre) of an edge's line put it ON the
  edge: inside, as the exactly parallel case always was. A 1 m slab holding
  10 000 m3 measured 9500 along north, south and west alignments (500 m3 gone
  from a paid quantity, with nothing in the reply to say so) and 10 000 along
  east, where sin 0 = 0 exactly
  (`VolumesAlongAlignment.ASlabMeasuredAlongAnyCardinalAlignmentIsWholeWhereItsEndSectionsLieOnItsBoundary`,
  and at every fifteenth degree with the slab's corners cut by the alignment
  itself, as a corridor's are). Rejected: cutting the end sections a hair inside
  the range, which fixes the ends only, to within that hair, and not a boundary
  that runs along the range.
- **The section areas are exact.** A section is a straight segment; each
  surface along it is the run of pieces where the segment crosses one of its
  triangles (`terrain::SegmentProfiler`, Cyrus-Beck clipping against each
  triangle the segment's box meets), and where a piece of one meets a piece
  of the other the difference is linear, so each span is a trapezoid - two
  triangles of opposite sign where the surfaces cross in it
  (`terrain::signedTrapezoid`). An edge's crossing and its height are
  computed from the edge's own vertices, so neighbouring pieces meet at the
  same bits (`SegmentProfiler.APlaneReadAlongASegmentIsThePlaneOnEveryPieceAndThePiecesMeetExactly`).
  A section's `width=` is the length on which both surfaces had a height.
  When no section of the range has any, the surfaces share no ground there
  (their boxes may meet over triangles that do not) and the verb is refused
  as every other form is, naming the alignment, both surfaces and what each
  covers; some sections empty and some not is a measurement, with
  `sections.empty=` saying how many.
- **The volumes are the methods'.** Between stations L apart with areas A1
  and A2, the average end area is L (A1 + A2) / 2 and the prismoidal formula
  L (A1 + 4 Am + A2) / 6, Am the area of the section cut at the middle
  station - not the mean of A1 and A2 - both as Ghilani and Wolf give them
  (*Elementary Surveying: An Introduction to Geomatics*, the chapter
  "Volumes"). The prismoidal formula is Simpson's rule on the area as a
  function of station, exact when that is a polynomial of degree three or
  less (Burden and Faires, *Numerical Analysis*, "Elements of Numerical
  Integration": Simpson's rule has degree of precision three). `method=both`
  reports both and the prismoidal correction, end area less prismoidal.
- **The mass haul ordinate** is the cut less the fill, summed from the start
  of the range, with the factors applied - the mass diagram's convention,
  cut positive. Rising, the job has cut to spare; falling, fill to bring.

The test of all three is a wedge whose section grows in width and depth
along the alignment, so its area is s^2 / 50: exactly 100^3 / 150 = 6666.667
m3, which the exact overlay also gives (0.1 x the triangle's 1000 m2 x its
centroid's x of 200/3). On sections 10 m apart the prismoidal formula gives
it exactly and the average end area 6700, over by the trapezoid rule's
(b - a) h^2 A'' / 12 = 33.333
(`VolumesAlongAlignment.OnAWedgeThePrismoidalFormulaIsExactAndTheEndAreaOverstatesIt`).

## The grid method

`terrain::gridVolume`: square cells on lines at whole multiples of the cell
(so grids of one cell size line up, as RASTER GRID's do), each counted when
its centre is on both surfaces - and inside the polygon, when there is one -
as a prism of the cell's area and the difference at its centre: the midpoint
rule. It is a cross-check of the exact methods and the way a grid of cells is
measured, and it converges on the exact answer as the cell shrinks. How fast
is known on a pyramid: its height is linear on every cell but those its four
ridges run through, and there the larger of two uniform offsets averages c/6
more than at the centre, so each such cell's prism stands 0.1 c too high
(for the 30 m pyramid on 100 m) - over the 200 / c ridge cells, 20 c^2 in
all, which the test holds to 1e-6 m3 on 5, 2, 1 and 0.5 m cells
(`GridVolume.TheMidpointRuleOverstatesAPyramidByATenthOfACellCubedInEachCellOnARidge`).
More than 25 million cells is refused, naming the cell that fits, as a
surface's raster export caps its grid.

The DEM tools' RASTER DIFFERENCE (`docs/terrain.md`, "The DEM tools") is
the grid method on rasters; VOLUME GRID is the same method on the drawing's
surfaces, so a raster's answer and a surface's can be set side by side.

## Factors

`bulk=` multiplies the cut and `shrink=` the fill, each as given, in the
reply only (`cut.adjusted= fill.adjusted= net.adjusted=`, and the mass
ordinate) - never the geometry, so the measured volumes stay what was
measured. Which measure the factors put the two in (bank, loose, compacted)
is the person's: the program does not decide it.

## The difference and envelope surfaces

SURFACE DIFFERENCE makes the top minus the base over their common ground
(`terrain::differenceSurface`): an isopach, linear on every overlay piece,
so exact. Positive is fill, negative cut, and it is given the built-in "Cut
Fill" style (blue cut below 0, red fill above) when the drawing still has
it, so it draws as the cut/fill map. Integrated against 0 its above and below
are the comparison's fill and cut
(`DifferenceSurface.ItIntegratesToWhatCompareSurfacesMeasures`). Its default
name is `<top>-<base>`, made unique; NAME must be free.

SURFACE ENVELOPE makes the higher (MAX) or lower (MIN) of two surfaces over
their common ground (`terrain::envelopeSurface`): a piece on which the two
cross is split along the line where they meet - a vertex per edge it crosses,
placed from the edge's ends in a fixed order so the pieces either side share
it - and each part is one surface's plane.

## In the window

Terrain > Volumes (`terrainVolumes`, a Terrain toolbar button, the painted
`Volumes` icon) opens `volumesDialog`, kept between uses: a page per method,
the boundary - Everywhere, the polygons the shared "Apply to" and "Only
those that match" controls take (`ScopeFilterWidget`, prefix `volumes`), or
typed corners - with EACH, the bulk and shrink factors, and the line the
fields make in `volumesCommand`. Measure runs it through the window's one
executor; the reply's records fill `volumesResults` (a column per key, in
the order first met) and, along an alignment, the mass haul diagram
(`volumesMassHaul`). Copy puts the table on the clipboard; Save CSV runs the
same line with `csv=` and OVERWRITE (the file dialog asks before choosing a
file); Make Cut/Fill Map (`CutFillMap` icon) runs SURFACE DIFFERENCE of the
page's two surfaces and offers the new surface at once. Every widget's
object name is listed in `src/katana_qt/terrain/volumes_dialog.hpp`.

## Through katana_mcp

No tool of its own: VOLUME and SURFACE DIFFERENCE are session lines, so
`katana_run_commands` runs them as the window does, and the replies are the
records an agent reads (`docs/mcp.md`, "Volumes through katana_run_commands").
A tool would restate the verbs' options as a schema and add nothing the
records do not carry.

## Tests

- Terrain (`tests/terrain/test_overlay.cpp`,
  `tests/terrain/test_volume_methods.cpp`,
  `tests/terrain/test_section_volume.cpp`): the region; the overlay's pieces;
  the difference and envelope surfaces against crossing planes and against
  `compareSurfaces`; two parallel planes 1 m apart over a 10 x 10 polygon,
  100 m3; a tilted plane against a level inside a square; a prism under a
  non-convex polygon; the stockpile pyramid on level and sloping ground and
  against each base; the grid's 20 c^2; the wedge; parallel planes along a
  curved alignment; sections on a surface's boundary on every cardinal
  bearing and at every fifteenth degree, and at survey coordinates; sections
  that cross, on a curve, a corner and two legs, and the band that clears
  them; every refusal; and the same bits at any thread count.
- The verbs (`tests/cad/terrain/test_volume_verbs.cpp`, the `VolumeVerbs`
  suite): every form through `CommandInterpreter::run`, EACH per polygon with
  an open line skipped, an ellipse and a polyline drawn back to its start as
  polygons, overlapping polygons, the factors, the CSV, the difference as one
  undo step, the envelope, the refusals.
- The polygons' reader (`tests/cad/terrain/test_drawing_shapes.cpp`, the
  `DrawingShapes` suites, shared with the surface edits): what each kind of
  entity gives and why one gives none, typed corners with and without
  heights, the words, and a scope that takes nothing.
- The window (`tests/qt_widgets/terrain/test_volumes_dialog.cpp`): each
  page's line by object name; Measure with the interpreter as the executor,
  filling the table and the mass haul diagram; Make Cut/Fill Map; Save CSV.
  And the real window, headless (`tests/geo/headless/volumes.cmake`):
  `qt_volumes_dialog_measures_inside_typed_corners_and_makes_the_cut_fill_map_headless`
  draws the two surfaces, opens Terrain > Volumes, measures inside typed
  corners (16 m3) and makes the cut/fill map.
- `katana_cli` (`src/katana_app/geo/cli/volumes.cmake`):
  `cli.volume_between_two_surfaces_is_the_slab_whole_and_inside_a_square`,
  `cli.volume_to_a_level_reads_what_lies_below_it_as_fill`,
  `cli.volume_sections_along_an_alignment_give_the_end_area_total`,
  `cli.volume_sections_northwards_over_the_slab_are_whole_at_its_edges`,
  `cli.volume_sections_round_a_curve_give_the_band_when_the_reach_is_short_of_its_radius`,
  `cli.volume_sections_round_a_curve_with_the_default_reach_are_refused_not_counted_twice`,
  `cli.surface_difference_keeps_the_cut_fill_map_as_a_surface`,
  `cli.volume_of_a_surface_that_is_not_there_is_refused`.
- `katana_mcp`: `McpServer.AVolumeBetweenTwoSurfacesIsMeasuredThroughTheCommandTool`.

## Decided

- **The overlay is the pair clipping, exposed - not the global constrained
  triangulation.** The earlier global overlay was 5.7 times slower
  (`docs/terrain.md`, "Where the time actually goes"), and the pieces a pair
  clip gives are the partition it built. The region is a third cut of the
  same clipper rather than a polygon boolean: the geometry layer has no
  polygon booleans (`include/katana/geometry/polygon.hpp`), and a triangle is
  convex, so the clipper already does it.
- **The difference surface joins the pieces directly, with the constrained
  triangulation as the fallback.** Re-triangulating every time would be
  exact too, but it is the cost the overlay was rid of; joining by name is
  linear in the pieces. Rejected: welding corners by distance, which merges
  two crossings a hair apart into one and tears the surface somewhere else.
- **The section areas are cut by segment clipping, not by `extractSection`.**
  The section view's sampler (`cad/section.hpp`) finds a surface's breaks by
  bisection to `kGeometric` between interval samples: right for a drawing,
  but an area integrated from it is exact only to the bisection, and it is
  in `cad`, where the terrain layer cannot reach. The clipper is closed form.
- **Sections that cross are refused, not cut short at the crossing.** Cutting
  each section off at its nearest crossing would give a number, but a silent
  one: the width measured is not the width asked for, and the ground beyond
  the crossing on a curve's inside belongs to no panel. A refusal that names
  the reach at which none cross costs a person one field retyped. Rejected too:
  refusing the default reach on any alignment that has a curve, which would
  also refuse an almost straight one whose sections cross nowhere within
  reach; the test is the crossing itself, so it follows the geometry.
- **A section on a boundary is on the surface within `kGeometric`.** The one
  tolerance of the policy (`include/katana/math/numerics.hpp`) is used, not a
  second one for sections; 0.1 micrometre is four orders above the rounding
  of a bearing at survey coordinates (9e-15 m at a 141 m reach, 1e-9 at the
  ulp of 6 000 000) and far below any ground that could be measured.
- **Average end area is the default method.** It is the method measurement
  standards and road contracts pay on (`docs/cad.md`, "Corridor
  quantities"), so it is what a quantity from here is compared with;
  prismoidal and both are a word away.
- **The mass ordinate takes cut as positive**, the mass diagram's convention,
  although `net=` takes fill as positive as every comparison here does: each
  is the convention of the document it is read against.
- **Overlapping polygons are refused for one total**, not unioned. A union
  needs polygon booleans this codebase does not have, and summing silently
  would count ground twice; EACH says each one's and warns of the overlap.
- **Numbers to the millimetre.** A cubic millimetre is far below any survey
  that made the surfaces; the full double in a reply would be noise, as the
  surface records found (`docs/terrain.md`, "One surface record"). The CSV
  carries the same numbers as the reply, so the two never disagree.
- **Volumes take no PREVIEW.** They change nothing; PREVIEW is for what does
  (SURFACE DIFFERENCE and ENVELOPE).

## Not done

- **The curvature correction on curved alignments.** A section's centroid
  travels a different distance than the centreline on a curve (Pappus): a
  panel of constant section area A between two radial cuts holds A (R -/+ e)
  theta, with e the centroid's offset from the centreline (inside minus),
  and end areas along the centreline give A R theta, so each panel is off by
  the fixed factor R / (R -/+ e) whatever its length. A shorter interval
  does NOT shrink it (a quarter circle of radius 50 with a strip 10 m wide on
  the inside measures 1785.398 m3 at intervals of 10, 1 and 0.1, against the
  true 1706.858 = 500 + (pi/4)(50^2 - 40^2) + 500, 4.6% over; on the outside
  the same 1785.398 against 1863.938, 4.2% under). It cancels for a section
  symmetric about the centreline (left= equal to right=: 3570.796, exact).
  The correction is not made; a person on a curve keeps the reach symmetric,
  or applies the eccentricity correction A (1 -/+ e/R) to the panel. Practice
  shortens the interval on curves for another reason, that the section changes
  quickly there.
- **Stretches of an alignment side by side.** Only crossing sections are
  refused. Two parts of the alignment that run side by side nearer than twice
  the reach, with no section of one crossing a section of the other, still
  sweep the same ground twice. A curve cannot do it (the legs either side of a
  curve of radius R are 2R apart, and a reach over R crosses at its centre), but
  a U-turn made of corners can, and so can two roads laid side by side in one
  alignment. The spacing of legs is not tested.
- **Polygons with holes from the drawing.** The region takes holes
  (`OverlayRegion::fromPolygon`), but the verbs take each closed outline as a
  polygon of its own: a tagged hole joining its area, as the GIS conversion
  reads it, is not read here yet.
- **Splines** are not taken as polygons (`skipped.other`): `Spline2` has no
  closed flag, and a spline drawn back to its start is not read as one.
- **A background job.** The verbs run on the thread that runs the line; on
  the window's that is the GUI thread, and a difference of two 200 000
  triangle surfaces takes a second or two. SURFACE FROM's job pattern is the
  way to move it.
- **Not measured: the difference surface's join.** The overlay it is made of
  is (`compareSurfaces`, "One overlay" above); the cost of joining its pieces
  into one surface on a large pair is not.
