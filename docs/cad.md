# CAD application layer

`katana_cad` (UI-independent application logic), `katana_qt` (the desktop
application) and `katana_app` (`katana_cli`, the headless front end).

## Purpose

`katana_cad` holds everything an interactive CAD session needs that is not a
widget: the open document, selection and picking, snapping, the view transform
and the command-line interpreter. All of it is unit tested without a GUI, and
the Qt application and the CLI drive exactly the same objects. That split is
what lets the engine be exercised, scripted and regression-tested with no
display attached — and it is what will let the Phase 23 application API expose
the same operations without duplicating logic.

## Document

`Document` owns the `Model`, the `CommandStack`, the current `ProjectStore`, the
selection, the current layer and the project metadata.

* `execute`/`undo`/`redo` delegate to the command stack.
* Listeners are notified after anything observable changes — model, selection,
  current layer, project. Views rebuild from the document rather than tracking
  deltas.
* Undo can delete the entities that are selected and the layer that is current,
  so after every command the document prunes the selection to entities that
  still exist and falls back to layer `"0"` if the current one has gone.
* `open` validates the loaded project before touching the model, so a failed
  open leaves the current drawing exactly as it was. `save` takes a backup
  first. `saveAs` creates a new project directory.
* `isModified` combines the command stack's save-point state with metadata
  edits.

## Selection and picking

`SelectionSet` is an ordered, duplicate-free set of ids with `set`, `add`,
`remove`, `toggle` and `prune`.

Two visibility predicates express the difference the UI cares about:
`isDrawn` (visible entity on a visible layer — locked layers still draw) and
`isSelectable` (the same, but the layer must also be unlocked).

* `pickEntity` returns the nearest selectable entity whose *geometry* lies within
  a tolerance of the point — the tolerance is a pixel aperture converted to model
  units, so picking behaves identically at any zoom. Distance is to the drawn
  curve, not to a filled area: clicking the middle of a circle selects nothing,
  which is what a drafter expects. Ties go to the higher id, i.e. the entity
  drawn last, i.e. the one on top.
* `pickInBox` implements the two standard modes: `Window` selects only entities
  entirely inside the box, `Crossing` also takes entities the box touches. The
  crossing test is a real geometric test against the box edges, not a bounding
  box overlap — a large circle whose bounding box covers the selection box but
  whose curve passes nowhere near it is correctly *not* selected.
* `SelectionFilter` restricts by entity type and layer.

## View transform

`ViewTransform` maps model space (y up) to pixels (y down):

```
screen.x = w/2 + (world.x - centre.x) · scale
screen.y = h/2 - (world.y - centre.y) · scale
```

`zoomAt` keeps the model point under the cursor fixed, which is the behaviour
that makes zooming feel correct. Scale is clamped to [1e-7, 1e7] px per model
unit: beyond that a `double` can no longer resolve a pixel at survey coordinate
magnitudes, so panning and picking would become erratic rather than merely
useless. A property test asserts the anchor point stays under the cursor across
random zooms at UTM-magnitude coordinates, with a bound that scales with the
zoom level because one ulp at a 5e6 northing is about 9.3e-10 model units —
which is a visible fraction of a pixel when zoomed far in. That is a limit of
binary64, not of the implementation, and is documented rather than papered over.

`fit` shows a bounding box with a margin; an empty drawing centres at the origin
at scale 1, and a single point centres without changing zoom (a point has no
extent to fit).

`gridSpacing` returns a 1-2-5 sequence value such that adjacent lines are at
least a minimum number of pixels apart, so the grid stays legible at every zoom.

## Snapping

Requested modes are a bit set: `Endpoint`, `Midpoint`, `Center`, `Intersection`,
`Perpendicular`, `Tangent`, `Nearest`, `Grid`.

Resolution order is deliberate: the closest candidate among the *exact* modes
wins, with ties broken in the order endpoint, intersection, midpoint, centre,
perpendicular, tangent. Only if no exact candidate is inside the aperture does
`Nearest` apply, and only then `Grid`. An endpoint always beats merely being
somewhere on the line, which is how drafters expect snapping to behave.

Details that matter in practice:

* A **centre** is offered while the cursor hovers the *curve*, not only the
  centre point — but it ranks as if it were at the aperture edge, so any exact
  snap on that curve still wins.
* **Perpendicular** and **tangent** require a start point (the rubber-band
  origin) and are silently unavailable without one. Tangent points are computed
  from the angle `acos(r/d)`; a start point inside the circle has no tangent and
  yields nothing.
* An **arc's centre can lie far outside the arc's own extents** (a shallow arc on
  a large circle), so candidate gathering uses the full circle's bounding box
  for arcs. Perpendicular and tangent candidates are then discarded unless they
  fall within the sweep.
* **Intersection** snapping intersects the curves of all nearby entities
  pairwise, including each segment of a polyline.

## Command interpreter

`CommandInterpreter` turns one line of text into one command. It parses, then
builds the same `Command` objects the GUI uses — so everything typed is
validated, atomic and undoable, and nothing bypasses the model.

* Points: absolute `12.5,40`, relative `@3,4`, polar `@5<30` (distance < angle
  in degrees). Angles are degrees at the interface, radians internally.
* Numbers are parsed with `std::from_chars`, which is locale-independent: `1.5`
  never becomes `1,5` on a machine with a comma decimal separator. Trailing junk
  and non-finite values are rejected rather than silently truncated.
* Quoted strings carry spaces; an unterminated quote is a parse error.
* Verbs are case-insensitive with the usual CAD aliases (`L`, `C`, `PL`, `TR`,
  `EX`, `F`, `CHA`, `U`, …).
* `LINE` with more than two points creates a chain of lines as a single
  transaction — one undo step.

Commands: drawing (`POINT`, `LINE`, `PLINE`, `RECT`, `CIRCLE`, `ARC`, `TEXT`,
`DIM`), modification on the selection (`MOVE`, `COPY`, `ROTATE`, `SCALE`,
`MIRROR`, `ARRAY`, `ERASE`), editing (`OFFSET`, `TRIM`, `EXTEND`, `FILLET`,
`CHAMFER`), `SELECT`, `LAYER`, attributes (`CHLAYER`, `COLOR`, `PROP`), `UNDO`,
`REDO`, `NEW`, `OPEN`, `SAVE`, `LIST`, `INFO`, `HELP`.

## Desktop application

`katana_qt_app` is a `QMainWindow` with a viewport, dockable layer, property and
command panels, and a status bar showing cursor coordinates, the active snap and
the current layer.

The viewport draws with `QPainter` and turns mouse input into commands. It holds
no geometry of its own: it paints whatever the model contains (Rule 3), and the
Vulkan renderer of Phase 15 will replace the painting code without touching the
interaction logic.

Interaction: left click picks or starts a point; dragging left-to-right is a
window selection and right-to-left a crossing selection (shown by a solid or
dashed rubber band); middle-drag pans; the wheel zooms about the cursor; right
click or Esc cancels; Enter finishes a polyline and `C` closes it; Delete erases
the selection. Shift adds to the selection and Ctrl toggles.

Two rendering details are worth noting. Arcs and circles are tessellated in
*model* space with a chord count chosen for a sub-quarter-pixel sagitta, so a
very large radius with only a sliver on screen never hands Qt coordinates in the
millions. Text below three pixels tall is drawn as a baseline stroke rather than
glyphs, so a zoomed-out drawing stays legible instead of dissolving into
unreadable marks.

Layer visibility, locking, colour and current-layer selection are edited
directly in the layer table; each edit is a command, so it participates in undo.
Opening a project whose database is damaged offers to restore the newest sound
backup.

## Headless front end

`katana_cli` runs the same interpreter with no GUI:

```
katana_cli                                interactive
katana_cli script.kcs                     run a script, '#' comments
katana_cli -c "RECT 0,0 30,20" -c LIST
```

Scripts and `-c` batches stop at the first failing command and exit non-zero, so
the tool composes with shell pipelines and CI. Three end-to-end CTest cases use
it to draw, save, reopen in a *separate process* and verify the geometry, and to
confirm that invalid input fails the process.

## Threading and ownership

Single-threaded; everything belongs to the GUI thread. `MainWindow` owns the
`Document`, which owns the model, the command stack and the project store. The
viewport holds a reference to the document and must not outlive it. Listeners
must not mutate the document re-entrantly.

## Performance

Drawing is O(n) in entities per frame, with bounding-box culling against the
visible world bounds. Picking and snapping are O(n) with a bounding-box
pre-filter; snapping's intersection mode is O(k²) in the curves that survive the
filter, which is small because the filter is an aperture a few pixels wide.

That last clause is load-bearing, and it used not to hold. The pre-filter works
on whole ENTITIES, so a 3000-vertex surveyed string near the cursor contributed
all 3000 of its segments to the pairwise loop — 4.5 million intersections on
every mouse move, since snapping runs synchronously in `mouseMoveEvent`.
Polyline segments are now culled individually against the cursor's reach box
before they enter the candidate list. The cull is exact rather than an
approximation: every candidate is accepted only within `aperture` of the cursor
and lies on the curve that produced it, so a segment whose bounding box misses
the reach box cannot carry a point that would have been accepted.

Measured on a 16-thread 2.5 GHz x86-64, Release, one `snap()` call over the
middle of a polyline (`benchmarks/bench_cad.cpp`):

| Polyline vertices | Before | After |
|---|---|---|
| 100 | 233 µs | 7.0 µs |
| 1000 | 22.7 ms | 62 µs |
| 2000 | 90.8 ms | 131 µs |
| 3000 | 247 ms | 196 µs |

The 3000-vertex case is 1260× faster and the cost is now linear — dominated by
the endpoint scan, not the intersection loop. Before the fix a 1000-vertex
polyline already exceeded PLAN.MD §32's 16 ms budget by itself.

PLAN.MD §32 asks for 60 FPS interaction and sub-16 ms selection. At drafting
scale this is met comfortably, but the linear scans will not hold at 10⁶
entities: the spatial index of Phase 18 is the intended fix, and the pick and
cull call sites are deliberately narrow so that index can be dropped in behind
them.

## Failure modes

Every command failure surfaces as a message in the command log and, for errors,
the status bar; the model is untouched. Closing with unsaved changes prompts to
save, discard or cancel. A failed save reports the error and leaves the modified
flag set. A damaged project offers backup recovery and never deletes the
damaged file.

## Spatial indexing (Phase 18)

Snapping, picking and box selection each walked every entity in the model.
Measured in Release, 4-vertex strings scattered over a 1 km square:

| Entities | Snap per mouse move | Share of the 16 ms budget |
|---|---|---|
| 100 000 | 4.3 ms | 27% |
| 250 000 | 11.6 ms | 73% |
| 500 000 | 23.9 ms | over budget, before anything is drawn |

A quarter-million-entity as-built is ordinary, so the scan ran out before the
drawings did. `katana::geometry::SpatialIndex` is a sparse spatial hash grid
over bounding boxes, used as a broad phase: it returns a superset and every
exact test still runs, so it can make queries faster and cannot make them
different.

| Operation | 100 000 entities, scan | indexed | factor |
|---|---|---|---|
| Snap | 4404 µs | 88 µs | 50× |
| Pick | 3910 µs | 12.6 µs | 310× |
| Box select | 4442 µs | 0.040 µs | constant in drawing size |

At 500 000 entities snapping drops from 24.3 ms to 1.9 ms. It still grows with
**density** rather than count, because intersection snapping is quadratic in the
candidates inside the aperture; that is inherent to the mode.

### Two decisions worth knowing

**A hash grid, not an R-tree.** The R-tree is the textbook answer and handles
pathological size distributions better, but its insertion, splitting and
rebalancing are much more code and every one of those paths must stay correct
under the constant incremental edits a CAD document makes. The grid is O(1) on
insert, remove and update. `oversizedCount()` is exposed so the case where an
R-tree would win can be seen rather than guessed; that is the upgrade trigger.

**The indexed box is not always the bounding box.** Snapping an arc offers its
centre, and the centre of a shallow arc is far outside the arc's own bounding
box. Indexing plain bounding boxes would have made the broad phase reject the
arc and centre snap would have silently stopped working the moment the index
was switched on. `detail::queryExtents` therefore indexes the full circle for
an arc and the bounding box for everything else — while picking still tests the
true bounding box, so clicking an arc's centre does not select it. Both are
asserted in `tests/cad/test_indexed_queries.cpp`.

### Staying in step

`Document` maintains the index from the per-entity changes every command
reports, so a single click on a large drawing does not pay for a rebuild.
The whole index is rebuilt only when the model is replaced — a rebuild is also
what chooses the cell size from the data. It costs 7.8 ms at 100 000 entities
and 44.6 ms at 500 000, about 5% of the load time.

The equivalence tests run each query twice, scanned and indexed, and require
the results to be equal — including after moves, deletions, undo and redo.

### The per-frame path, and knowing when NOT to use the index

Picking and snapping happen on a click or a mouse move. Collecting the entities
a repaint has to draw happens on **every frame** — every pan, every zoom — so
it was the scan that mattered most. At 500 000 entities zoomed to 1% of the
extent it cost 22.3 ms per repaint to find the 500 entities on screen.

Measured, 100 000 entities, by the share of the drawing's **area** on screen:

| Area on screen | Visible | Scan | Indexed |
|---|---|---|---|
| 1% | 900 | 4060 µs | **170 µs** |
| 9% | 8 995 | 4029 µs | **2586 µs** |
| 25% | 25 195 | 4881 µs | **4147 µs** |
| 49% | 49 000 | **4426 µs** | 6173 µs |
| 100% | 100 000 | **4018 µs** | 8068 µs |

The last two rows are the point. **Asking an index for everything is slower
than walking the model once**: it gathers every id, sorts them, then looks each
one up again, against a single ordered traversal. A zoom-extents repaint is
exactly that case, and it is common.

So `forEachCandidate` chooses. Below ~35% of the indexed area it queries the
index; above it, it scans. The threshold is the measured crossover, not a
guess, and the table above is in the header next to it. After the change a
zoomed-out repaint costs 4360 µs against the scan's 4337 µs — the same, within
noise — while the zoomed-in case keeps its full win:

| 500 000 entities, 1% on screen | Before | After |
|---|---|---|
| Collect visible | 22.3 ms | **0.090 ms** (248×) |

Both paths are required to return identical results, including across the
threshold: `BothSidesOfTheScanCrossoverGiveTheSameAnswer` sweeps window sizes
from far below it to far above and compares.

## Alignments in the command line and the window

`ALIGN NEW name x,y x,y [x,y ...]` defines an alignment by its PIs, with no
curves; `ALIGN SET name index radius [spiralIn [spiralOut]]` rounds a corner;
`ALIGN PI name x,y [...]` appends one; `ALIGN START name station` sets the
chainage origin; `ALIGN STATIONS name interval` prints a setting-out table.
PI indices count from 0, to match the PI the solver names in its refusals.

**The station table always includes the key stations.** An interval table
that skipped the TS, SC, CS and ST would be useless in the field, because
those are the points that get pegged. The key stations are merged with the
interval stations, sorted, and de-duplicated to a nanometre, so a key station
that happens to land on the interval appears once.

**Chainage labels use `to_chars`, not `snprintf`.** The overlay prints
1234.5 as `1+234.50`. `snprintf` obeys the C locale and would print
`1+234,50` on a machine set to a decimal-comma locale - the same reason the
dimension formatter avoids it.

**The overlay solves each alignment per repaint.** A document has a handful
of alignments and a solve is a few spiral end-points; caching would need
invalidation on every edit for no gain anyone has measured. The comment in
`drawAlignments` says to measure before changing that.

**One section routine for both callers.** "Cut Section Along Selection" and
"Cut Section Along Alignment..." share `cutSectionAlong`, which gathers the
visible surfaces, cuts and shows. The alignment path chords the centreline at
10 mm: the section samples at most every 100 mm along it, so a finer polyline
would cost time and change nothing the section reports.

## Design profiles

`ALIGN DESIGN name station,elevation[,curveLength] ...` defines the design
grade line on an alignment, at least two PVIs; `ALIGN PVI name s z [L]`
appends one; `ALIGN PROFILE name` prints the PVIs, the solved tangents and
curves with their grades, and the high and low points; `ALIGN CLEARPROFILE`
removes it.

**Why `DESIGN` exists as well as `PVI`.** The first draft had only `PVI`,
adding one at a time, and the first one could never be added: a profile with
one PVI cannot be built, and the model rightly refuses to hold an alignment
whose profile does not solve. A profile therefore has to be born with at
least two PVIs, in one command, and `PVI` appends to one that exists. The
alternative - letting the model hold a partial profile - would have broken
the invariant that everything stored can be drawn.

**The design rides into the section as a series.** `cad::appendDesignProfile`
adds the profile to a `Section` as one more `SectionSurface`, sampled at
every station any ground series already has - so design and ground can be
read against each other at the same stations - plus the profile's key
stations and extrema, marked `SampleReason::ProfileVertex`. The section view
needed no change: it already draws every series in its palette, breaks a
line at every gap, and lists each series in its legend. A design shorter
than the section shows as a gap beyond its end rather than an invented
grade. Cut Section Along Alignment... passes the profile through
`cutSectionAlong` when the alignment has one.

## Corridor quantities

`cad::corridorQuantities` (`include/katana/cad/corridor.hpp`) sets a template
across an alignment at the design elevation at every station, carries its
edges to the ground by batter slopes, and sums the cut and fill between the
finished shape and the ground along the alignment. Terrain > Corridor
Quantities... drives it from a dialog and shows the schedule.

**The template is deliberately simple.** One carriageway of a half width
each side, at a crossfall, with a cut batter and a fill batter to daylight.
Real assemblies carry kerbs, verges, benches and subgrades; they are a later
need and belong in a table of their own, not in four more fields here. What
the simple one already answers is the quantity to within the accuracy of the
ground model, which is the accuracy anyone has.

**Average end area, and why.** The volume between two stations is the mean
of their cross-section areas times the distance between them. That is the
method every earthworks specification and measurement standard names and the
basis of payment in most road contracts, so it is the one a quantity from
this software can be compared with. The prismoidal correction is more
accurate where areas change quickly and is not applied; a caller wanting it
shortens the interval, which is what practice does. The profile's key
stations are always sectioned in addition to the interval, because the
quantities change character at a PVC and a PVT and a schedule that stepped
over them could not be checked against the design.

**Ground is queried, not sectioned.** Each cross section asks the TIN for
its elevation at a set of offsets directly rather than cutting a section with
`extractSection`. A section carries breaks and crossings this does not need,
and the daylight search wants the ground at arbitrary offsets rather than at
a fixed set of samples. Ground is sampled at every design vertex and every
half metre between, so an undulation between two template vertices is not
straightened out.

**Daylight by march and bisection.** The batter is marched out from the edge
in half-metre steps until it and the ground have changed order, then
bisected forty times - half a metre resolves any batter a machine can build,
and the bisection puts the crossing at 5e-13 m. A march rather than a closed
form because the ground is a TIN, piecewise planar with no formula for where
an arbitrary line meets it.

**A section that cannot reach the ground contributes nothing and is
counted.** When a batter runs off the surface within the assembly's maximum
width, the section is marked incomplete, the intervals touching it add no
volume, and the count is reported - the dialog says in so many words that
the totals are not the whole job. Inventing an elevation for it would be the
silent failure PLAN.MD section 36 forbids.

**Tested against hand-worked sections, checked independently.** A level
design 1 m under flat ground with 2:1 batters is the trapezoid (12 + 8) / 2 =
10 m^2; crossfall of 5% drops the edges and widens it to 11.68; ground rising
10% to the left daylights 3.5 m out on that side and 1 m out on the other for
10.75. Each was integrated numerically in Python by a method sharing nothing
with the trapezoid split in the code. A constant section along a straight
gives area times length exactly whatever the interval, which the end-area
method must.

**The corridor as a surface, and the check it makes possible.**
`cad::corridorSurface` triangulates the template strings of every complete
section - both daylights, both edges, the centreline - as breaklines, with
the daylight lines as the boundary so nothing is hulled across the inside of
a curve. Terrain > Corridor Surface... adds it to the scene like any other
surface, so the finished design is visible in 3D and in sections. Two
decisions: sections that cannot reach the ground are left out and the
strings broken there, and when any are, the boundary is not used (a ring
with gaps in it is not a ring) and the surface is hull-bounded instead, with
the count reported; and where two strings cross in plan on the inside of a
curve tighter than the corridor is wide, the crossing takes the mean of
their elevations rather than refusing the whole surface for it.

The surface makes an independent check of the quantities possible, and the
tests make it: `compareSurfaces` of the built corridor against the ground is
an exact overlay, the end-area total is a different method entirely, and on
a straight with a level design both are exact and agree on 2000 m^3 to 1e-6.
Two implementations that share nothing agreeing on the same number is the
strongest evidence either can have.

Both commands share `askCorridor`, one dialog returning the solved alignment
and profile, the ground and the assembly, rather than two copies of eighty
lines of dialog.

## Parcels

`cad::parcelReport` (`include/katana/cad/parcel.hpp`) turns any closed
polyline into its courses as bearings and distances, its area, perimeter and
centroid; `legalDescription` writes the deed wording; `parcelLabels` makes
the text entities. `PARCEL id`, `PARCEL id LEGAL [name]` and
`PARCEL id LABEL [height]` drive them.

**Nothing is stored.** A parcel is computed from its boundary on demand, so
the report can never disagree with the drawing and there is no second table
to keep in step when a corner is moved. Labels, when asked for, are ordinary
text entities the user owns afterwards - a snapshot, and honest about it by
being editable like any other text - created in one undo step, because
nobody wants to undo a parcel's labels one bearing at a time.

**Built on the survey layer, not beside it.** Azimuths, quadrant bearings,
DMS formatting, the inverse between two points, the signed area and the
centroid all already existed in `katana::survey`. The one thing this code
does that could be wrong is the conversion from the drawing's (x, y) to the
survey layer's (northing, easting), and it happens in exactly one function.
That is why the rectangle test checks the bearing of every course and not
only the area: swap the axes and the area stays 5000 while every bearing is
wrong. Bearings are formatted to whole seconds, the precision a deed quotes.

**Labels never read upside down.** Each course label lies along its course
but is flipped by a half turn when the course runs west or south, so all of
them read left to right or upward; and it is placed on the inside of the
boundary - left of the course for a counter-clockwise boundary, right for a
clockwise one - with the baseline moved a further text height inward when
the flip would otherwise hang the glyphs outside. Text width is estimated at
0.6 of the height per character for centring, because this layer has no
font metrics and only the centring depends on it.

**Two things this slice taught about the build.** `katana_cad` had never
linked `katana_survey`, because nothing in `cad` had needed it: the layering
rules allowed the include, and the link line did not follow. And the build
filter used in this session - showing only lines that begin with a source
path - hid that link failure and printed "built" over a missing binary,
which is the green-over-red CLAUDE.md warns of. Builds are now judged by
ninja's exit code.

## Plotting to PDF

File > Plot to PDF... paints the drawing onto a sheet. `cad/plot.hpp` holds
the arithmetic - paper sizes, the scale ladder, the sheet transform and the
fit rule - and is tested as arithmetic; `ViewportWidget::plotToPdf` holds the
`QPdfWriter` and the painter.

**One drawing code, two surfaces.** The plot calls the same `drawEntities`
and `drawAlignments` the screen uses, with the widget's `ViewTransform`
temporarily replaced by the sheet's - device pixels of the PDF at its
resolution - and put back afterwards. There is no second renderer for paper,
so the plot and the screen cannot disagree about anything but the paper.
That is Rule 3 applied to output: the renderer is never a second source of
truth, and neither is the plotter.

**Line weights finally mean what they say.** `Layer::lineWeight` has been
documented as "millimetres on paper" since Phase 09, validated, persisted,
and read by nothing that draws, because there was no paper. On screen every
line is still a 1.5 px hairline - a screen has no paper for a weight to be
millimetres of. On the plot the pen is `lineWeight * pixelsPerMillimetre`,
where a millimetre is `dpi / 25.4` device pixels by definition. The dash
pattern generator already took the pen width as a parameter, so linetypes
keep their model-unit lengths on paper too.

**ISO 216 sizes and a scale ladder.** Paper sizes are ISO 216:2007 Table 1.
Fitting picks the first of 1:100, 200, 250, 500, 1000, 2000, 2500, 5000,
10 000, 20 000, 25 000, 50 000 at which the drawing fits inside the margins,
because a scale bar is only useful when the scale is one a scale rule
carries; beyond the ladder the exact denominator is used so the plot still
fits. The first draft of the fit test checked only the sheet's width and got
the 10 km case wrong - on a landscape sheet the height binds first - and was
corrected from that derivation, not from the output.

**The sheet transform owns the margins.** The PDF writer's own page margins
are set to zero; a non-zero writer margin would shift the page under a
transform that already accounts for the margin, and the drawing would land
off-centre by exactly that amount.

**Not plotted.** Rasters and point clouds: at 300 dpi on A1 a backdrop image
would be resampled to tens of megapixels per plot, and a point cloud drawn
point by point would take minutes. Both are a later slice with their own
decisions about resolution. The grid, the snap marker and the selection are
screen furniture and are not drawn either.

**The PDF itself is tested, not only the arithmetic.** The PDF writing runs
inside the Qt widget and needs an application, which the unit-test suites do
not create - so the application grew a `--plot` switch: `katana_qt_app
<project> --plot out.pdf [--fit | --scale N] [--paper A3] [--portrait]
[--dpi N]` plots and exits without showing a window. The `qt_plot_headless`
test runs it under `QT_QPA_PLATFORM=offscreen` on the sample project and
checks the exit code, that a file of plausible size came out, and that it
begins with `%PDF`. Two rules it follows are worth keeping. It copies the
sample project before opening it, because opening can touch the project
directory and a test must never change repository data. And it compares the
header as hex: CMake's plain `file(READ ... LIMIT 4)` on this platform
returned `%PDF` plus a newline - five characters - and failed a perfectly
good PDF, while the `HEX` read is byte-exact. The dialog and the switch
share one `plotDrawingToPdf`, so the test exercises the same code the menu
does.

