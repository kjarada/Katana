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
selection, the current layer, the current style and the project metadata - and,
as session data outside the model, the loaded style library and survey map
(decision D1; `docs/survey_coding.md`).

* The CURRENT STYLE (decision D9, `setCurrentStyle`) is what new work is drawn
  in - AutoCAD's CELTYPE, or a current point style - so a symbol or a library
  linestyle can be drawn WITH, not only applied afterwards. Empty means
  ByLayer and is the default. `currentAttributes()` hands the current layer
  and style to every drawing tool and verb. It is refused for a style the
  model lacks, and like the current layer it is session state, not a
  command. After any command, undo or redo that leaves it naming no style -
  a delete, a merge, a rename - it is CLEARED, not followed: no command event
  carries a rename's new name, and falling back to ByLayer can never put new
  work in the wrong style. PURGE keeps it although nothing wears it yet.
* `setStyleLibrary` and `setSurveyMap` replace the library or the map whole,
  notify, and bump `libraryGeneration()` or `surveyMapGeneration()`. A cache
  of flattened definitions, thumbnails or code lookups keys on those counters
  - never on a `LineStyle*` or `SurveyRule*`, which dangle when the whole is
  replaced, and never on "a listener fired", which a selection click also
  does.

* `execute`/`undo`/`redo` delegate to the command stack.
* Listeners are notified after anything observable changes — model, selection,
  current layer, project. Views rebuild from the document rather than tracking
  deltas.
* `modelRevision()` counts up by one for every command executed, undone or
  redone and for every new or opened drawing, and never for anything else - a
  selection, the current layer or style, a save, the metadata, or the library
  and map, which have their generations. So "has the drawing changed since I
  last looked" is one comparison. The managers' `DocumentWatcher` reads it; it
  used to compare a fingerprint of the history's counts and the tables' sizes,
  which could not tell an undo followed by a new command from nothing, nor a
  reopened project from the drawing it replaced.
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
`CHAMFER`), `SELECT`, `LAYER`, the tables (`LINETYPE`, `DIMSTYLE`, `HATCH`,
`STYLE`), civil (`ALIGN`, `PARCEL`), attributes (`CHLAYER`, `COLOR`, `PROP`),
`UNDO`, `REDO`, `NEW`, `OPEN`, `SAVE`, `LIST`, `INFO`, `HELP`. Aliases include
`LT`/`LTYPE`, `DS`, `HA`, `ST`, `AL` and `PARC`. This list lacked the tables
and civil verbs until the audit of 2026-09-23.

Added on 2026-09-23 for the style, linetype and symbol managers (fixing audit
CAD-06) and the Survey menu; `CommandInterpreter::helpText` is the reference:

| Verb | What it does |
|---|---|
| `STYLE SET <s> linetype <name>` | takes a model linetype, a loaded library linestyle or `ByLayer` (the layer's; decision D2) - it used to refuse library names the viewport draws |
| `STYLE SYMBOLS [filter]` | the symbols a style may name: the built-in shapes and the library's symbols (D3), filtered with case folded |
| `STYLE USAGE [name]` | how many entities wear each style, or who uses one, flagging a name worn but not in the table (`entity::tableUsage`) |
| `STYLE MERGE <from> <into>` | moves everything wearing `from` onto `into` and deletes `from`, one undo step |
| `STYLE CURRENT [name\|-\|ByLayer]` | the style new work is drawn in (D9); `-` or `ByLayer` clears it |
| `LINETYPE MERGE <from> <into>` | repoints every layer and style, then deletes `from`; `into` must be a model linetype |
| `LAYER LTYPE <layer> <name>` | also takes a library linestyle; refuses `ByLayer`, since a layer is what ByLayer inherits from |
| `PURGE [STYLES\|LINETYPES\|HATCHES\|ALL]` | deletes what nothing uses as one undo step, keeping the current style |
| `INVERSE`, `FORWARD` (`RADIATE`), `AREA` | the Survey menu's inverse, forward point and area, printed by the same formatters as its dialogs (`docs/survey.md`) |

Each front end adds verbs of its own, because `katana_cad` may not see GDAL,
PDAL or the archive and customisation readers: the application's command line
(`MainWindow::runCommandLine`) adds `IMPORT`, `EXPORT`, `INFO <file>`,
`REFS`, `CUSTOMISE [REPLACE] <file>...`, the view verbs `ZOOM`, `GRID` and
`SNAP`, and `QUIT`; `katana_cli` adds `IMPORT`, `EXPORT`, `REFS`, `COPC` and
the survey-code verbs (`CODE`, `CODE EXPLAIN`, `CODE CENSUS`, `MAPFILE LIST`,
`MAPFILE CHECK`, `CUSTOMISE [REPLACE]`; `docs/survey_coding.md`) - its
`--help` lists them. Of the survey-code verbs only `CUSTOMISE` is on the
application's command line; the rest are the Survey Code Manager's tabs.

In the application a typed line is routed before the interpreter sees it
("The tool host" below): while a tool runs, the whole line is that tool's
answer; a single word that is a tool's alias or id starts the tool; the same
word with arguments (`LINE 0,0 10,0`) is still the interpreter's, which is
what scripts and the headless checks type; and an empty line is Enter in the
drawing.

## Desktop application

`katana` (the target in `src/katana_qt`) is a `QMainWindow` around a
workspace of views, with dockable Layers, Properties, Command Line and
Reference Data panels. Its menu bar reads in the order a CAD user reads it -
File, Edit, View, Draw, Modify, Annotate, Format, Survey, Terrain, GIS, Help
(`MainWindow::buildActions`; each menu has an object name, `fileMenu` to
`helpMenu`). File keeps to files: the customisation is loaded from Format
and from Survey > Survey Coding, beside the managers of what it brings. The
status bar shows a view's running readout, the current layer, the active snap
and the cursor coordinates.

The readout (`FrameStatsLabel`) is a 3D view's frame time or a section's
station and elevation under the cursor, forwarded from whichever view raised
it (`ViewWorkspace::onFrameStats`) and kept apart from `onStatus`, which is
for messages, so the status bar's message never hides it. It shows the most
recent view's readout, not only the active view's, and keeps its last text
while a plan view is active. It has no automated test; a screenshot of a 3D
view is what checked it.

The plan viewport draws with `QPainter` and turns mouse input into commands. It
holds no geometry of its own: it paints whatever the model contains (Rule 3).
It is NOT on the `render::DrawList` path: the tiled software rasteriser of
Phase 15 draws the 3D and Elevation cells, and a Vulkan backend would replace
that rasteriser, not this painting code (PLAN.MD Phase 15). This paragraph used
to say the reverse.

Interaction: left click picks, or gives the running tool its point or its
entity; dragging left-to-right is a window selection and right-to-left a
crossing selection (shown by a solid or dashed rubber band); middle-drag pans;
the wheel zooms about the cursor; Delete erases the selection. With no tool
running, Shift adds to the selection and Ctrl toggles; Enter or Space starts
the last tool again; Esc abandons a box and then clears the selection; and a
right-click cancels as Esc does, since nothing is wired to the view's context
menu (`ViewportWidget::onContextMenu`). While a tool runs, Enter, Space and a
right-click are the tool's Enter, and Esc ends the tool first. What a tool
does with each is "The tool host", below.

Two rendering details are worth noting. Arcs and circles are tessellated in
*model* space with a chord count chosen for a sub-quarter-pixel sagitta, so a
very large radius with only a sliver on screen never hands Qt coordinates in the
millions. Text below three pixels tall is drawn as a baseline stroke rather than
glyphs, so a zoomed-out drawing stays legible instead of dissolving into
unreadable marks.

Layer visibility, locking and colour are edited in the layer panel or the
Layers dialog (Format > Layers..., Ctrl+L); each edit is a command, so it
participates in undo. Choosing the CURRENT layer is
`Document::setCurrentLayer`, deliberately not a command - it changes what the
next drawing tool does, not the drawing (see below). Opening a project whose
database is damaged offers to restore the newest sound backup.

**The current style and the Style row (decision D9).** The Properties
toolbar carries the style new work is drawn in (`CurrentStyleCombo`): ByLayer
first, then the drawing's styles by name. Choosing one is
`Document::setCurrentStyle`, session state like the current layer and so not a
command, and the log says "New work is drawn in ...". It acts on the box's
`currentIndexChanged`, kept out while `MainWindow::refreshStyleChoices`
rebuilds the list, so a headless `--fill` sets it as a person's pick does. A
current style the list does not hold is shown, marked "(not in the drawing)",
never replaced by the first choice - the QT-02 lesson. The Properties panel
has a Style row above its read-only table (`PropertyStyle`, editable, and
`PropertyStyleApply`): it shows the selection's style as it is, even a name
the drawing lacks, or "<varies>" when the selection disagrees, and Apply or
Enter gives the selection the style typed or chosen as one undo step
(`MainWindow::applyPropertyStyle`, `commands::setEntityStyle`). Only the
entities that change are in that step, so applying the style already shown
is no step at all; and nothing is applied from the box's own change signal.
A rename or delete of the current style clears it rather than following it
("Document", above), so the toolbar then shows ByLayer.
`qt_current_style_and_the_style_row_headless` drives both.

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

Drawing, picking and snapping ask the spatial index (Phase 18, "Spatial
indexing" below) for the entities near the view or the cursor, with an exact
bounding-box test after it; snapping's intersection mode is O(k²) in the curves
that survive, which is small because the filter is an aperture a few pixels
wide. (This paragraph said all three were O(n) scans awaiting the index for
some time after the index was in use.)

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
the status bar; the model is untouched. Closing the window asks twice, in this
order (`MainWindow::closeEvent`): first the Survey Code Manager, whose
unapplied rule edits live only in its buffer - it is shown and asks Apply /
Discard / Cancel over the rules it is about (`CustomisationWorkbench::confirmClose`),
first because its Apply changes the drawing the next question is about - and
then, with unsaved changes, save, discard or cancel (`confirmDiscard`). A
headless run has nobody to answer either, so it refuses and says why in the
log, never discarding anything unasked: a scripted `QUIT` with unapplied code
edits, or `NEW` after an edit, is refused, and the script can Apply or Revert
(`applyMap`, `revertMap`), `SAVE` or `UNDO` first. A failed save reports the
error and leaves the modified flag set. A damaged project offers backup
recovery and never deletes the damaged file.

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
not create - so the application grew a `--plot` switch: `katana
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

## The look of the application: theme, icons, toolbars

The viewport has been dark (`#1e2329`) from the start, and the window around
it was whatever grey Qt defaults to. A dark drawing in a light frame is most
of why the application looked unfinished. Three files fix that, and each was
shaped by what this toolchain does and does not have.

**`theme.hpp` - every chrome colour is a named token.** `window`, `panel`,
`raised`, `hover`, `border`, `text`, `textMuted`, `accent` and so on, as
functions. The stylesheet is ASSEMBLED from them rather than written with
colour literals, so the tokens are the only place a colour is decided and a
change of theme is a change to one file. Nothing else under `src/katana_qt`
should contain a widget colour literal; the viewports keep their own constants
for DRAWING colours (grid, snap marker, selection), which are content, not
chrome. The style is Fusion, because it is the one built-in style that honours
a palette completely - the native Windows style draws light controls into a
dark palette. The accent is blue, not the red of the logo: in an engineering
program red already means "error", and a checked tool must not read as one.

**`icons.hpp` - the icons are drawn in code.** Each is a vector drawing on a
24-unit grid painted by a `QIconEngine` at whatever size and device-pixel ratio
Qt asks for. There are no image files behind them, for four reasons: they are
crisp at every size because nothing is resampled; they take their colours from
the theme, so a theme change recolours all of them; a function that is wrong
does not compile, where a resource path that is wrong is a blank button; and
this toolchain has no Qt SVG module and no image tools, so an SVG or PNG set
would have been a dependency added for decoration. The language is one rule
applied everywhere: a 1.7-unit round stroke, neutral for the object and ACCENT
for the part the command acts on - the arrow of Import, the new geometry of a
draw tool, the cut line of a section. 1.7 rather than the usual 2.0 because at
16 px the heavier stroke closes up small counters (the label of Save, the
inside of the magnet).

**The application icon comes from the same painter.** `katana_make_icons`
writes `resources/katana.ico` (nine sizes, PNG-compressed entries), a 256 px
PNG, and `icon_sheet.png` - every icon at 96 px and at the 20 px it ships at.
The `.ico` is COMMITTED, not generated in the build: generating it would make
an ordinary build run a Qt program before it could compile a resource file,
which fails wherever that program cannot start, and buys nothing for an icon
that changes once a year. Below 64 px the sword is drawn larger and heavier
within its tile; at true proportions it is a few pixels wide on a taskbar and
disappears. `katana.rc.in` embeds the icon and a `VERSIONINFO` block whose
numbers come from `project(Katana VERSION ...)`, so a version is written in
exactly one place.

**The contact sheet earned its keep at once.** The first application icon had
the handle running UP the blade with the guard at the pommel - a sign error in
the tangent - and the Circle tool read as a minus sign in a ring. Both were
obvious on the sheet and invisible in the code.

**Toolbars share their actions with the menus.** `MainWindow::makeAction`
makes each `QAction` once - text, shortcut, icon, status tip and a tooltip
that names the command and shows its shortcut, since an icon-only button owes
the user its name - and the same object goes into the menu and the toolbar, so
the two cannot drift. File, Edit, Properties (the current style), View and
Format run along the top; Survey, Terrain and GIS have a second row of their
own (`addToolBarBreak`), because in one row they were squeezed to a button
each behind overflow arrows. Draw and Annotate run down the LEFT edge, where
every CAD program keeps its drawing tools, because they are the ones reached
for without looking and a strip beside the drawing is a shorter journey than a
row above it; Modify runs down the RIGHT edge, as in AutoCAD's classic layout,
because one column could not hold all three without hiding the last tools
behind an overflow arrow. Every toolbar and dock has an object name, which is
what `QMainWindow::saveState` keys a layout on. View > Panels brings back any
dock that has been closed.

**Every key reaches one thing.** `MainWindow::shortcutClashes` gathers every
key a person can press - each action's key sequences, any `QShortcut`, each
menu's Alt letter and each item's underlined letter within its own menu (two
items with one letter make the key cycle between them instead of choosing) -
and lists those that reach more than one thing, since Qt disables an
ambiguous shortcut for both. `katana --check-shortcuts` fails a run that has
any, and `qt_every_shortcut_and_menu_letter_reaches_one_thing_headless` runs
it. Ten underlined letters were changed to pass it (A&ttributes, Sym&bol
Library, Loa&d Customisation, In&verse, Toggle Pe&rspective and others).
The tool actions built from the catalogue have no underlined letters; their
aliases are in their tooltips.

**`katana --screenshot out.png`** lays the real main window out and grabs it to
a PNG, headlessly. It exists so that the LOOK can be reviewed - in a pull
request, or by a model that cannot watch a screen - the way `--plot` lets its
output be reviewed. The first screenshot found three things no test would
have: alignment station labels printing over one another where key stations
bunch up (a label is now skipped when it would land within 70 px of the last,
while the tick is always drawn - the tick is the information, the label only
names it); Zoom Extents ignoring alignments, which are drawn but are not
entities; and the command log opening at a third of the window's height. The
`qt_screenshot_headless` test builds the whole window - theme, toolbars, every
icon, the sample with its hatch and alignment - and checks a PNG of plausible
size comes out. It protects construction, not appearance.

The same switch has grown into a headless DRIVER for the window, its menus
and its dialogs, run by the `qt_*` tests through
`tools/check_screenshot.cmake`; the usage comment at the head of
`src/katana_qt/main.cpp` and the one above `-DDRIVE` in that script are the
reference. Everything is found by object name, which is why every action,
field, button and tab gets one. The steps run in the order given, with the
event loop run between them as it runs between two things a person does:

| Switch | `-DDRIVE` step | What it does |
|---|---|---|
| `--dialog NAME` (first called `--survey-dialog`, still accepted) | `@NAME` | triggers action NAME as a click does and makes the dialog it opened the target: the dialog named by the action's data (the Format managers carry `styleManagerDialog`, `symbolLibraryDialog`, `surveyCodeManagerDialog`), else NAME + `Dialog` (the Survey dialogs); says on stderr what opened, and whether it is modal |
| `--survey-dock ACTION` | `#ACTION` | shows the dock that action shows and makes it the target |
| `--panel NAME` | `%NAME` | makes the window's own dock, toolbar or menu NAME the target; a menu is opened under its title, so a grab shows what it offers |
| `--fill FIELD=TEXT` | `FIELD=TEXT` | a line or text box (`\n` a line break), a choice by its text (an editable one takes a name it does not list, as typing does), a spin or check box, a tab brought to the front by its text (`managerTabs=Linetypes`), or a list, grid or tree row selected by its text - the whole row where the view selects rows, as a click does |
| `--press BUTTON` | `!BUTTON` | clicks it; a disabled button fails the run |
| `--command TEXT` | `>TEXT` | runs TEXT as if typed on the command line - make styles and a selection, or start a tool by its alias and answer its prompts |
| `--enter` | `>` alone | Enter on an empty command line (an empty argument does not survive a CMake list) |
| `--report NAME` | `?NAME` | prints on stderr what the target's widget NAME shows - a label's text, a field's, a list's rows - or, for one of the window's actions, its text and whether it is checked (which tool the menus show running); for one of the window's menus (`formatMenu`), its title and every item with the status tip it shows, without opening it |
| `--trigger NAME` | `*NAME` | triggers menu item NAME in its turn among the steps (`--action` runs before them all) |

`--check-shortcuts` (`-DCHECK_SHORTCUTS=ON`) is described above, and
`-DDIALOG=NAME` is `-DSURVEY_DIALOG` for any menu's dialog. What the
target is at the end is what `--screenshot` grabs; steps that were all
commands leave the window. `-DEXPECT=<regex>` checks what the run logged -
a headless run echoes its log to stderr - `-DCOMPARE=` checks the files it
wrote against a reference, and `-DREFUSED=<regex>` requires the run to be
REFUSED - exit 1, never a crash - which is how a test shows something is NOT
offered: pressing a disabled button, or filling a choice the dialog does not
have, fails the run instead of doing nothing. `-DFORBID=<regex>` fails a run
whose output matches it at all, which is how a test holds what the
application SAYS to a vocabulary (the Format menu and the customisation log
name no other program: `qt_the_format_menu_and_the_customisation_log_name_no_other_program_headless`).
It is matched after the run's own paths - the copy, the output, the project,
the customisation folder, the work folder, the application's folder and the
checkout - are replaced by `<path>`, so where a checkout or worktree sits
cannot fail it; the rest of a line that holds a path is still read, and a
failure prints the text that was checked. `docs/survey.md` has the survey
import wizard's use of it. A headless run still never opens a modal box
("Panels refresh on the event loop" below): Format > Layers and Edit >
Attributes, still modal, say so in the log and name the switch that grabs
them (`--layer-manager`, `--attributes`), and a `QUIT` the window cannot take
without asking is refused ("Failure modes" above).

**Not done.** Settings are not persisted: toolbar positions, dock layout and
window geometry are not saved between sessions although every object now has
the name that would allow it (the dock chrome has the hooks, below). There is no
light theme. The dialogs (corridor, plot) are themed but plain. View >
Viewport Layout's actions still have no object names (View > Active Viewport
Shows has them: `viewShowsPlan`, `viewShows3D`, `viewShowsSection`,
`viewShowsElevation`). `resources/icon_sheet.png` is regenerated by
`katana_make_icons` and now carries the Survey icons and the four Format ones
(Purge Unused is a broom, not a second bin).

## The workspace: every view is a dock, and each has its own layers (PLAN.MD 47)

The user's request of 2026-09-23 was for panels and views that dock, move,
minimise, close and come back from the menus, views that go out onto another
screen, and per-view control of which layers show. Until then the views were
tiles: `ViewportContainer` placed widgets at the rectangles of a fixed split
(`cad::ViewportLayout`), so a view was identified by its POSITION. Every layout
change destroyed and rebuilt every widget, which lost the plan's zoom, every
section but the first and any half-picked clicks - and the application changed
the layout on its own, whenever a surface was built or a section cut.

**Each view is now a `QDockWidget` inside a nested `QMainWindow`**
(`katana_qt/view_workspace.*`), which is the main window's central widget. Qt's
dock machinery gives splitting, tabbing and floating onto any screen for free.
A NESTED window rather than the main window's own dock areas, because an
arrangement ("Four: Equal") must move views and never the Layers panel, and a
panel should not be droppable between two views. Rejected alternatives:

* *Keep the tiles and add pop-out windows.* Two ways of showing a view, and the
  tiled half still rebuilds on every change.
* *Qt Advanced Docking System* (LGPL-2.1+, packaged by MSYS2 for ucrt64 but not
  installed). It would give auto-hide side bars and native floating frames with
  a real minimise button off the shelf. It is a new third-party dependency, a
  change to the bundle and a licence decision, which belong to the owner; the
  plain-Qt design does not preclude moving to it later, because the views and
  their state do not know what hosts them.

**A `QMainWindow` makes itself a top-level window whatever parent it is given**
(its constructor ORs in `Qt::Window`), and a window placed as another main
window's central widget is an EMPTY layout item. The first build of the
workspace was given no space at all and every view vanished while the panels
filled the window. `setWindowFlags(Qt::Widget)` in the constructor is what
makes it a child; the comment there says so.

**A view's state lives in `cad::ViewSet`, not in its widget.** `ViewState`
holds the kind, the 3D camera, the plan zoom, the layers hidden in that view,
the reference layers hidden in it and its section. Views are identified by a
`ViewId` that is monotonic and never reused, like entity ids, so a queued event
for a closed view cannot find another. States are held by `unique_ptr` because
each widget holds a reference to its state; closing a view deletes the dock
(and so the widget) BEFORE dropping the state. Changing a view's kind replaces
only the widget inside the dock, so a 3D view switched to plan and back keeps
its orbit. `ViewSet::mostRecent(kind)` is what Plot, F9 and the Standard Views
act on: the active view when it is that kind, otherwise the one of that kind
the user touched last - so F9 still reaches the 3D view after a click in the
plan. The layout presets survive as arrangements: `cad::dockSplits(kind)` lists
the `splitDockWidget` steps that build each one, and a test asserts that
applying them to rectangles reproduces `layoutRects(kind)` exactly (PLAN.MD 47
slice 2).

**Showing a view never replaces one.** Building a surface or importing meshes
used to split the window and turn a cell into 3D; cutting a section turned the
ACTIVE cell into a section, which was the plan view being drawn in whenever the
user had just clicked there to select the alignment. `ensureView(kind)` raises
the view of that kind used most recently, or opens a new one beside the active
view.

**Zoom Extents frames the active view only**; one view's zoom is not another's
business, and the old container did one thing while its header said another.
After New, Open and an import every view is framed (`zoomExtentsAll`), because
all of them are looking at a drawing that has just changed under them.

**Per-view layers extend THE visibility rule rather than adding a second.**
`cad::isDrawn` and `isSelectable` take a `const LayerOverrides&` - the layers
one view hides - with NO default argument, so every caller has to say whether
it means a view or the document (`kNoLayerOverrides`); a default would let the
next consumer ignore the view it draws in, which is how the 3D view once came
to ignore layer visibility altogether (audit REN-03). The overrides are
SUBTRACTIVE: a view can hide what the document shows, never show what it hides,
so the rule stays a plain conjunction and a layer switched off in the Layers
panel cannot linger in some forgotten view. Hiding a path hides everything
beneath it, the document rule's ancestor semantics (PLAN.MD 5.1), tested by
walking the path's ancestors as `string_view` slices - no allocation, and an
immediate return for a view that hides nothing, since this is asked once per
entity per frame. The overrides reach snapping (`SnapRequest::view`), picking
and box selection (`SelectionFilter::view`) and the 3D scene
(`SceneOptions::layers`), so a layer hidden in a view can be neither snapped to
nor picked there - otherwise Delete would erase something the user cannot see.
Three consumers use the document rule on purpose, because their result is
shared and must not depend on which view was clicked last: the section cut
(a view is to hide the crossings on its hidden layers when it paints them,
PLAN.MD 47 slice 6), the typed SELECT, and Surface From Drawing.

Per-view layers are view state: not saved in the project, not undoable, and
not a document change - so nothing hears them through the document listener,
and the workspace repaints the one view that changed. Document listeners carry
no payload, so a view cannot follow a layer rename; every view's overrides are
pruned of names that no longer exist after each change, which is what stops a
later layer reusing the name from being born hidden. A view's Layers button
(`view_layers_popup.*`) is where they are set: it filters that view alone,
and isolating a tree node hides everything but that node's path.

### The dock chrome: one title bar for panels and views

The same request asked for panels and views that minimise, float, maximise
and close. `QDockWidget`'s own title bar has two buttons, Float and Close,
drawn by the style - and Fusion drew them light on the dark theme, which is
why `theme.cpp` used to strip them, so no dock could be closed or floated
except by dragging. It has no Minimise, no Maximise and nowhere for a view's
own controls. `DockTitleBar` (`src/katana_qt/dock_chrome.*`) replaces the
whole bar: `[icon] title ... [tools] [_] [float] [max] [x]`, with a view's
kind switcher where the icon is and its Layers button among the tools. Qt then
draws no buttons and gives a floating dock no native frame; it still resizes
one from its edges (4 px, outlined by the chrome, since Fusion's was one
invisible pixel), and dragging the bar still moves and re-docks it -
PROVIDED the bar ignores the mouse events it does not use. It does, and must
go on doing so; a double click is the one it takes, as the Float button.

**One owner.** One `DockChrome` per main window holds the tray and the
bookkeeping, and the panels (`MainWindow`) and the views (`ViewWorkspace`)
share it: a panel and a view minimised into two trays would be two ways of
doing one thing. It knows nothing of views or panels beyond the `DockRole` it
is told (`Panel`: Close hides it and the menus bring it back; `View`: it can
be maximised, and one view is active).

**Minimise is a tray, and why.** A floating dock is a `Qt::Tool` window and
must stay one: Qt relays a Tool window's shortcuts to its parent, so Esc,
Delete, Ctrl+Z and F9 keep working in a view floated onto another screen,
where a `Qt::Window` would lose every one (and Qt resets the flags on every
float anyway). A Tool window has no taskbar button, so the window manager's
minimise would lose it, and a docked panel is not a window at all. So
Minimise HIDES the dock and puts a button for it on `MinimisedToolBar` at the
bottom of the main window, and the button puts it back exactly where it was:
in its area and tab when docked, at its geometry on its screen when floating.
A hidden dock keeps its place in the layout tree, so "where" comes free; its
SIZE does not - the neighbours grow into the space and Qt shares the
shortfall equally when it comes back - so the sizes of the docks around it
are recorded and put back with it. Setting only the returning dock's size is
not enough: when the places of one row add up to more than the row has, Qt
takes the same number of pixels off each, so `DockChrome::applySizes` sets
every dock in both directions.

**Rejected: rolling a dock up to its title bar.** Measured offscreen in the
1360 x 860 window: Properties alone in the right area, rolled up, took the
whole column with it - 270 x 770 became 131 x 26, and the column's space went
to the drawing sideways; Layers rolled up above Reference Data narrowed the
left column from 300 to 135 px, and it stayed narrow after unrolling. Qt sizes
a dock area from its docks' hints, and a dock with its contents hidden hints
at the width of its bar. Only a floating dock behaved. Minimise does the job
for docked and floating docks alike and moves nothing else.

**Maximise (views only).** A docked view hides every other docked view of its
window until it is restored, which gives back the sizes they had; a floating
view fills the available geometry of its screen, one per screen. A maximise
is a temporary state, not a layout: the workspace undoes every docked
maximise before it opens or arranges views (a split made while the others
were hidden would be made against the wrong neighbours), a tray restore of a
docked view ends a maximise in its window, and everything is unmaximised
before a layout is saved.

**Not GroupedDragging**, in either window. With it, dragging a tabbed view
drags its whole tab group out into a `QDockWidgetGroupWindow` - Qt's own
window, with a native frame and none of the chrome - and a view in it can no
longer be floated or docked on its own.

**The side columns own the bottom corners.** `setCorner` gives both bottom
corners to the left and right areas, so the panels run the full height of
the window and the command line sits under the drawing only, between them -
AutoCAD's arrangement. Qt's default gives both corners to the bottom area,
which ran the command line under both columns and cut them short.

**The active view is always one on screen.** Zoom Extents and the view
menus act on the active view (and `mostRecent(kind)` prefers it), and its
title bar carries the accent, so an active view nobody can see is a menu
acting on nothing visible. Pressing a
view's title bar or any of its buttons makes it active. Minimising or closing
the active view hands on to a view that is ON SCREEN - floating, or a docked
view inside the workspace's rectangle - not merely one that is not hidden,
because a tab page behind the current one is not hidden: Qt moves it off the
window. The one that takes over is the first on screen in the order the views
were opened; the most recently used would be better, but `cad::ViewSet`
keeps its recency order private (a public `ViewSet::recent()` is the fix).
`openView` also activates the new view when the active one is hidden.

Two Qt 6.11.2 behaviours were measured on the way, and the code relies on
them: a tab group is laid out again AS SOON AS a page is hidden - the new
current page is already inside the window when the minimise hook runs, which
is why the on-screen test, not a `raise()`, is what picks the right view (the
`raise()` stays for a window never laid out, where no geometry means
anything); and `resizeDocks` leaves a hidden dock out of the total it gives
the enclosing row, so a view must be shown before it is sized - a stacked
split of a view already side by side came out 397 and 197 px of 600 until
`openView` showed the new dock first (297 each after).

Tested in `tests/qt_widgets/test_view_chrome.cpp` (`qt_widgets.ViewChrome.*`),
each test shown failing without its fix. Not yet tested outside a
hand-run harness, because `MainWindow` cannot be built into the widget test
target: the four panels' bars, their exact minimise and restore, F9 from a
floating panel, floating maximise across two screens. Not done: a Window
menu; saving and restoring the layout (`minimisedNames` and `minimiseNamed`
are the hooks, and `unmaximiseAll` must run before `saveState`); a floating
active view left minimised during an arrange stays hidden and active, since
only the tray button (`DockChrome::restore`) raises `onRestored`. The Survey
Point Manager dock (`SurveyPointsDock`) wears the chrome now that
`SurveyServices::chrome` hands it over, and the workbench has the chrome
forget the dock before deleting it; its minimise, tray and float have been
seen only in a screenshot, with no automated test.

## Point symbols (PLAN.MD 20.2, slice 2)

A point is drawn with the symbol its *style* names - `Style::symbol`. Since
20.3 slice 5 that is any name, resolved at draw time against the loaded style
library first and against the sixteen built-in shapes of `entity::symbolNames()`
(circle, square, triangle, diamond, cross, plus, tick, star, dot, ring, tree,
pole, manhole, arrow, flag, target) after, `builtInSymbolFor` guessing a shape
from a descriptive name last - at
`Style::symbolSize`, which is the symbol's width in model units, or 0 for the
viewport's own mark. `cad::symbolStrokes(name, centre, halfWidth, rotation)`
is the one definition of what each shape looks like: plain polylines in
model units, so the viewport, a plotter and a preview paint the same thing.
A circle is 24 chords, fixed: a symbol is small on any output and the eye
cannot tell 24 chords from a circle at that size.

Why the symbol is on the style and not on the entity: the survey
customisations that points with symbols come from define a symbol as a
linestyle drawn at a vertex, so "the style of a point" and "the style of a
line" are one table there; keeping them one table here means one manager, one
assignment command (`STYLE APPLY`) and one answer to "what does this point
look like". **Rejected:** a `symbol` field on `Entity` - a second place an
appearance can come from, and 27 000 surveyed points would each carry a
copy of what their style already says. **Rejected:** glyphs from a font or
SVG - a symbol has to plot as vectors at any scale and be picked by its
geometry, which strokes give for free.

Size is a width, not a radius, because that is what a library symbol's `size` is and
what a user types (`STYLE SET s symbolsize 1.5` is a 1.5 m manhole); the
viewport halves it for the strokes. A size of 0 draws a built-in shape at
the plain mark's pixel size, so a style that never got a size is still a
visible mark and not a dot of no extent, and draws a library definition at
its own scale.

Since the managers' foundations (2026-09-23) the question "which shape" is
`cad::resolveSymbol` - the library definition of the name, of either kind,
else the built-in shape of that name, else the shape the name suggests
(`SymbolKind::BuiltInFallback`, which a picker marks as a stand-in) - and
`cad::pointSymbolDrawing` gives the strokes whichever answers, the built-in
shapes wrapped as `StyleStrokes` in the entity's own pen, so the viewport, the
plot and a thumbnail cannot disagree. A symbol whose drawn extent is under
`kMinimumSymbolPixels` (3) on screen is drawn as a dot (`belowSymbolDetail`):
its strokes would be one smudge anyway, and ten thousand coded points zoomed
out would otherwise lay out every stroke of every one.

**Symbols on lines are drawn (decision D8).** This section used to end "Not
drawn, by decision rather than omission: a symbol on the vertices of a line",
while the 12da import's comment said the opposite and meant it. The lead
decided for what a survey drawing shows: a line whose style names a symbol draws it at
EVERY vertex (`cad::symbolVertices`: every vertex of a polyline, both ends of
a segment or an arc, a point's own position; a circle has none and a text or
dimension is not a line), as a survey drawing shows a fence's posts or a
string of drill holes. The symbol's name is never also used as a pattern along the line
(`resolveLinePattern`), so a style whose linetype names its own symbol - what
the 12da import writes - is a plain line with the symbols on it. Still not
drawn: a point symbol's rotation, since `Style` has no field for one and a new
field is a storage migration, deferred (decision D6); and symbols and library
linestyles in the 3D view, which draws neither.

## A mesh is not a surface

`geometry::TriangleMesh` (PLAN.MD 20.2 slice 4) is a list of points and a
list of triangles naming three of them each: what a 12d `primitive_3d`
carries, and what an IFC solid or an OBJ would. It is a separate type from
`terrain::TinSurface` on purpose. A surface is a function of x and y -
single-valued, sampled for a height, the thing a profile and a volume are
computed against. A mesh may be closed, may overhang, and may have several
sheets above one point; a pipe, a pit and a fence post are meshes. Giving
them one type would mean either a surface that cannot answer "the level
here" or a mesh that lies when asked.

What follows from that:

- **No elevation ramp.** A surface colours by height because height is what
  it is; a mesh takes one colour, or one per face where the file gives them.
- **Every edge of every face, or none.** A TIN halves its edge count by
  drawing each shared edge once, which it can because it knows its
  neighbours. A bag of triangles does not, and computing adjacency would
  cost more than the lines save - so `SceneMesh` defaults to `Shaded`. The
  numbers are in `scene.hpp` beside the default.
- **In plan it is a footprint.** `TriangleMesh::planHull()` is the convex
  hull of the vertices: exact and cheap whatever the topology, unlike a
  silhouette. It is a HULL and says so - a horseshoe-shaped mesh's hull
  covers ground the mesh does not. A degenerate footprint comes back as it
  is rather than as nothing: a vertical wall gives the two ends of a
  segment, because seen from above a wall IS a line and drawing that line is
  the truth about where it stands. One real archive brings 1 453 meshes of
  90 656 triangles, so drawing their triangles in plan would bury the
  drawing they are context for.
- **`triangle(i)` refuses a face that names a vertex which does not exist**,
  rather than trusting `validate()` to have been called. The scene builder
  is the last thing between a file and a read past the end of a vector.

## The layer manager, and why "move" is "rename"

The dock beside the drawing is the quick view; Format > Layers... (Ctrl+L,
first on the Format menu and toolbar, where AutoCAD keeps Layer; PLAN.MD 20.2
slice 6) is the whole table, with the fields a dock has no room for and the
operations that need it: move a layer under another parent, and put the
selection on a layer.

`LayerManagerDialog` follows the managers' rules ("The rules a dialog or
panel follows") and can be shown beside the drawing: it hears the Document
through a `DocumentWatcher`, so an undo or a typed command reloads it once,
from the event loop, keeping the selected layer and the form's unsaved edits
when that layer did not change underneath them; New, New Child and Rename or
Move ask for a name in a prompt row inside the dialog; a colour is typed as
`#RRGGBB` (`layerColourText`) or picked in a colour dialog opened with
`open()`, never `exec()`'d; and once its Document is gone it reads and changes
nothing, every control but Close disabled the first time a person reaches for
one. Its constructor is unchanged, and `selectedLayer()` and `selectLayer()`
are public for a caller that opens it on a layer. The WINDOW still `exec()`s
it from Format > Layers, so it is modal there and a headless run is told to
use `--layer-manager` instead; showing one kept instance, as the Format
workbench shows the other managers, is the remaining step. Selecting another
row drops unsaved form edits without asking, as the style manager does.

Moving needed no new command. A layer name is a path ("design/surface/tin1"
- see `layer_path.hpp` for why the tree is derived from the names rather
than stored), so moving a layer under another parent is exactly renaming it,
and `renameLayer` already carries the subtree and the entities on it as one
undo step. A "move" command would have been a second spelling of the same
thing.

Make Current is deliberately NOT a command: the current layer is session
state like the selection, and putting it on the undo stack would make Ctrl+Z
undo where the next line will be drawn - which is not what anyone means by
undo.

## The attribute manager: the tree 12d had, and what "varies" protects

A 12d string carries a tree of attributes; the importer flattens it to
properties keyed "Asset/Dimensions/Size", and per-vertex attributes to
"vertex/3/Name". The flat keys are honest - the model has one property map
per entity and no nesting - but unreadable on a survey string with thirty of
them, so the manager (PLAN.MD 20.2 slice 5) rebuilds the branches for
display and writes back the flat key. Nothing in the model changes: the tree
is a view of the names, exactly as the layer panel is a view of "/"-separated
layer paths (see `layer_path.hpp` for why that tree is derived and not
stored).

The decision worth keeping is `<varies>`. The dialog acts on the whole
selection, and shows a value only where every selected entity agrees on it;
a property only some of them carry counts as a disagreement too. The
alternative - showing the first entity's value - looks tidier and is a trap:
pressing Save would then write that value over the others without anyone
asking for it. Showing `<varies>` means a user who saves has said what they
want all of them to be.

Values are formatted by `entity::toString`, which is in `entity` and not in
the dialog, because the properties panel and the `PROP` command line print
the same values and must not disagree about them. A real is written with
enough digits to read back as the same double: a level shown as 31.2 that is
really 31.249 is a lie in survey work.

## The Format menu: the customisation workbench and its three managers

The owner's requests were to "enhance the linestyle and symbol and survey
codes managers" and to "make it professional CAD software". Format is where
AutoCAD keeps Layer, Linetype and Text Style, and it is where the three
managers and what goes with them now live:

```
Format
  Layers...                          formatLayers        (Ctrl+L)
  Styles and Linetypes...            formatStyles        -> styleManagerDialog
  Symbol Library...                  formatSymbols       -> symbolLibraryDialog
  Survey Code Manager...             formatSurveyCodes   -> surveyCodeManagerDialog
  ---
  Load Customisation...              loadCustomisation
  Replace Loaded Customisation...    replaceCustomisation
  ---
  Purge Unused...                    formatPurge
```

The Format toolbar carries Layers and the three managers. Survey > Survey
Coding shows the same code manager, Load and Replace actions - the same
`QAction` objects (`SurveyServices::codeManager`), so two menus cannot drift
apart.

**`CustomisationWorkbench`** (`src/katana_qt/customisation/customisation_workbench.*`)
is built like the Survey workbench: `MainWindow::buildFormatActions` makes the
menu and the toolbar and hands them over with `CustomisationServices` - the
Document, the view workspace, the window's action factory and log, whether
the session is headless (asked each time, since the window learns it after it
is built), and the window's own Layers, Load and Replace actions. So the
workbench never includes `main_window.hpp`, and a widget test builds it and
drives it (`tests/qt_widgets/customisation/test_customisation_workbench.cpp`).
It owns what the managers share: the picture cache (`DefinitionThumbnails`),
the session's linework control codes, and the one `CustomisationContext`
each manager is built from.

- **Non-modal, one of each, kept.** A manager is made the first time it is
  asked for and then hidden, not deleted, between uses (`QPointer` slots);
  asking again shows and raises the same one. That is what lets the code
  manager's unapplied edits survive closing it. Each opens on its first entry
  - the style manager's first rows, the symbol library's first symbol, the
  code manager's rule 0 - rather than on an empty pane.
- **Deleted before the Document.** The dialogs hold the Document and paint
  from the workbench's cache, and a window destroys its members BEFORE its
  child widgets, so `~CustomisationWorkbench` deletes them itself - and the
  window destroys the workbench before the Document.
- **"Show me what uses it"** (`CustomisationContext::selectAndShow`) selects
  the entities and frames them in the ACTIVE plan view only; the other views
  keep their zoom.
- **Found by the headless driver.** Each manager's action carries its
  dialog's object name as its data, which is how `--dialog formatStyles`
  finds the dialog it opened.
- **Purge Unused** (`purgeUnused`) deletes every style, linetype and hatch
  pattern nothing uses (`cad::planPurge`, `cad::purgeCommand`) as ONE undo
  step, keeping the current style, and names them in the log. An interactive
  session is asked first (the `confirm` hook, else a question box); a
  headless one is not. `qt_purge_unused_deletes_what_nothing_uses_as_one_undo_step_headless`
  purges, undoes and checks the style is back.
- **Closing the window asks the code manager first** ("Failure modes",
  above: `confirmClose`).

Loading and replacing a customisation are `docs/survey_coding.md` ("Loading a
customisation"): a load merges, Replace is asked for.

### Styles and Linetypes

One dialog for the `Style` and `Linetype` tables and the session's library
linestyles (PLAN.MD 20.2 slice 3, `src/katana_qt/style_manager.cpp`), because
they are one subject: a style names a linetype. An archive import brings 211
styles from one file - but NOT their linetypes: each style's linetype is a
LIBRARY name, which lives in the session's style library, not in the
model's Linetype table. So almost every real style names something the
drawing's own linetype table does not contain, and the dialog lists both.

What it shows, all from `include/katana/cad/style_manager_rows.hpp` so the
rows, filters and bulk edit are tested below Qt:

| Tab | Shows | Does |
|---|---|---|
| Styles | `cad::styleRows`: each style, how many entities wear it, and whether its linetype or symbol is missing; the chips All / Used / Unused / Missing and a search | a form (linetype and symbol through `NamePicker`, weight, colour or ByLayer, hatch, symbol size, description) with Save and Revert; New, Duplicate, Rename, Merge Into, Delete, Purge; Apply to Selection, Select Users, Make Current |
| Linetypes | `cad::linetypeRows`: the drawing's linetypes and the library's linestyles, by group, a name both hold marked (D2) | a pattern grid that edits a drawing linetype's dashes, gaps and dots in place; New, Duplicate, Rename, Merge Into, Delete, Purge; New Style Using This; Select Users |
| Diagnostics | `cad::styleDiagnostics`: `cad::missingNames`, then the D2 collisions, each with who uses it and what is drawn meanwhile | Select Users |

A `StylePreview` beside each form draws the style, or the linestyle, at a
plot scale on paper or on screen through the shared painter ("What a style
draws" below): lines at their PRINTED size, so a `paperstyle` looks the same
at every scale and a `worldstyle` shrinks as the scale's N grows; a symbol
fitted to the pane on its insertion point, with a scale bar in ground metres.
The dialog's Undo and Redo buttons are the drawing's own stack. A name is
asked for in a prompt row inside the dialog and a purge is checked in a panel,
never in a box, so a headless session drives every action by object name.

**QT-02's rule** (audit QT-02, fixed 2026-09-24). Save used to rewrite the
linetype and symbol of every archive-imported style: the form's boxes were
non-editable combos filled from the model's linetypes and the sixteen
built-in shapes, `setCurrentText` with a library name was a silent no-op on them,
and Save wrote back whatever they still showed. Now:

- a form writes back only the fields a person EDITED (`cad::StyleFields`,
  `applyEdit`) - even for one style, because a spin box cannot show every
  stored value exactly (a weight of 0.1234, a symbol size of 0.03125), so
  writing back what it shows would change a style nobody edited;
- an unedited Save is no command at all (`cad::editStylesCommand`,
  `commands::updateStyleIfChanged`), so Save is enabled whenever a style is
  selected and Revert only when there are edits;
- every name field keeps a name it cannot list - `NamePicker` for linetypes
  and symbols, `kept_name_combo.hpp` for the hatch and dimension-style
  combos;
- selecting several styles shows `<varies>` for the fields they differ in,
  and Save leaves those fields of each style alone unless they were edited.

The Layers dialog got the same fix; commands have no `updateLayerIfChanged`,
so its unchanged Save compares the `Layer`.

Two decisions from the first version still hold:

- **The table is read-only; the form edits.** Editing in the table would run
  a command from inside the table's own `itemChanged` signal - the shape of
  the crash recorded under "Panels refresh on the event loop" below. There is
  no `itemChanged` handler here at all, so the bug cannot be written. The
  dialog's own commands do not reload it either: they record what to select
  and the watcher's deferred reload does it, so there is one reload path.
- **A rename is a move, not an edit of a name field.** A `NamedTable` is
  keyed by name, so `renameStyle` removes, re-adds under the new name and
  repoints every holder (entities for a style; layers and styles for a
  linetype) in one command, and therefore one undo step. It then asks the
  DELETE guard whether anything still names the old item: the guard and the
  repoint are two readings of the same set of references, and a holder the
  repoint missed would leave an entity naming a style that no longer exists.
  Making them check each other costs one line and removes the class of bug
  that a second implementation of "who uses this" invites. A protected item
  ("continuous") is protected from a rename as much as from a delete -
  everything that resolves to it by name would silently change what it draws.
  Linetypes can be renamed too: a rename onto a library name is how a drawing
  linetype ends a D2 collision, since a model linetype cannot be MERGED into
  a library linestyle (`commands` cannot see the library).

Not done: the Linetypes tab's preview draws the STORED linetype, and only the
pattern strip shows unsaved grid edits; the Styles table has a Preview
picture only for a symbol or a library linestyle, not a drawing's dash
linetype; selecting another row drops unsaved form edits without asking (a
reload caused elsewhere keeps them); the Diagnostics summary still calls
every missing name one "that nothing defines", although each row's "drawn
as" says when it is "not a linestyle"; and in the dark theme the paper
preview is a large white pane while nothing is selected, and the Colour row
shows a disabled ByLayer button with a stray drop-down arrow.

### Symbol Library

AutoCAD's Blocks palette, MicroStation's cell selector and a civil package's
symbol chooser in one window (`src/katana_qt/customisation/symbol_library.*`):

- **left**, a tree of groups: All, Built-in, the library's `/` groups,
  "(ungrouped)", and "Not defined" when a name resolves to nothing;
- **centre**, a grid of 64-pixel pictures from the shared cache, captioned
  with the name and badged with how many entities draw it, under a filter
  bar: a search over name, group and survey code, and the chips All / In
  drawing / Used by codes / Missing / Vertex mode;
- **right**, a `StylePreview` at a plot scale (the insertion point marked,
  and a warning when it lies outside what the symbol draws), the details,
  and the actions.

What is listed is `cad::symbolLibrary`: D3's symbols (a definition is a
symbol when it is `mode vertex`, a survey code draws it as one, a style names
it as one, or its file is a symbol file), the built-in shapes, and every
symbol name a style OR A SURVEY CODE gives that nothing defines, in the
pickers' amber with the shape it is drawn as instead. The codes' names are the
library's own addition: `cad::missingNames` looks only at styles and layers.

**How big it prints.** The details say what a point wearing the symbol
prints: "2 x 2 mm at 1:500, 1 x 1 m on the ground". `cad::symbolPrintSize`
(`include/katana/cad/symbol_assign.hpp`) measures what `cad::symbolDrawing`
draws - its strokes AND the space each of its texts covers, since a symbol
that is a letter, or carries one above its mark, prints the letter too - at
the style's size (a width in model units; 0 for the definition's own), at
1:N. A text's size comes from a `TextExtent`: the dialog measures it with the
preview's own font (`styleTextExtent` in `style_painter`), so the number
agrees with the picture beside it, and without one cad estimates it
(`estimatedTextExtent`: 0.6 of the height per character, the height tall,
descenders not counted) - so the two can differ a little, 2.91 mm with Arial
against 3 mm estimated for the fixture's TEST Valve. A built-in shape, or the
stand-in for a missing name, has no size of its own at size 0: its print size
is the viewport's plain mark, and the pane says so rather than giving a
number.

The actions, each from a button and never from the grid's own selection
signal; each that changes the drawing is ONE undo step through
`Document::execute`:

| Button | What it does |
|---|---|
| Assign to Selected Points | `cad::assignSymbolToPoints`: the points among the selection move into a style that draws the symbol at the size given - one found that draws exactly that, else one made - and the log counts the points moved, already in it, not points, and not found |
| Set on Style | the chosen style's symbol (an inline style picker; `updateStyleIfChanged`) |
| Select Points Using | the points wearing a style that names it, selected and framed |
| Replace in Styles | `cad::replaceSymbolInStyles`: every style naming the current symbol names the one in the inline `NamePicker`; refused while that is empty |
| Load .4d... | `archive12d::readCustomisation`, MERGED into the session's library (D1: never a Replace from here), each file's added and replaced definitions in the log |
| Export Selected to .4d... | `archive12d::writeStyleLibrary` of the selected library definitions |

A headless session opens no file dialog: Load and Export are
`loadLibraryFile` and `exportSelectedTo`, which take a path, and the buttons
say in the log what to call. Not done: Assign makes a style with linetype
ByLayer and weight 0.25, and under D8 a line later put in that style draws
the symbol at its vertices; the grid's pictures are on the dark screen ground
only, though the preview switches; and the dialog reloads whole (two
`tableUsage` passes) on every command the watcher reports, not measured on a
250,000-entity drawing.

### Survey Code Manager

The survey code library a surveyor codes against - Civil 3D's description
keys, TBC's feature definitions - in five tabs, each
over one cad foundation, so the dialog decides nothing the CLI would say
differently: Code Table (`cad::codeTable` and `cad::explainCode`, with the
rule form), Codes in Drawing (`cad::codeCensus`), Issues
(`cad::lintSurveyMap`), Apply Codes (`cad::applySurveyCodes`, previewed before
it runs) and Linework (`cad::processLinework`, and the session's control
codes). Its edits go to a BUFFER and reach the drawing only on Apply; Revert
takes the drawing's map back; closing with unapplied edits asks. That, and
what each tab shows, is `docs/survey_coding.md` ("The Survey Code Manager").

### What the managers stand on

What was merged on 2026-09-23 is the tested logic below Qt that the managers
above need, each piece reachable from the command line too; the dialogs of
2026-09-24 render it and decide nothing of their own. Professional managers
(AutoCAD's, Civil 3D's, TBC's, MicroStation's) share a vocabulary -
usage counts, purge, merge, duplicate, a current style, pickers that browse a
library with pictures - and each item here is one of those, in the layer that
can test it.

- **Who uses what, in one pass** (`entity::tableUsage`,
  `include/katana/entity/table_usage.hpp`). Every style, linetype name,
  symbol name, hatch pattern and layer, with the layers, styles and entities
  that reach it. "Reaches" is `resolveDisplay`'s chain, read through the same
  two functions it reads (`resolvedLinetype`, `resolvedHatchPattern`), so an
  entity is counted against exactly the linetype it is drawn with. Names are
  kept whether or not a table defines them: a library linestyle a style names is
  not in the model, and a name nothing defines is exactly what a manager has
  to show. One pass for all rows, because a pass per row would be 800 styles
  over 250 000 entities. Three readers must not disagree and all read it: the
  delete guards, purge and the managers' "Used" column.
- **A refusal says how many and who first.** "that style is still used" became
  `Users::describe()`: "used by 1 layer, 1 style and 3 entities, e.g.
  layer=survey" - a layer first, since that is what a person fixes first,
  then a style, then the lowest entity id.
- **Merge, duplicate, purge** (`commands`, `entity_commands.hpp`).
  `mergeStyle` moves every entity wearing one style onto another and deletes
  it; `mergeLinetype` repoints every layer and style and deletes the linetype;
  both are one undo step and restore the holders they moved from before-images,
  not by moving everything back - so a holder that already named the target
  stays on it. `duplicateStyle`/`duplicateLinetype` copy under a new name.
  `purgeTableItems` deletes a set as ONE step and judges it as a set (a
  linetype named only by styles the same purge removes is free); it refuses an
  empty set, since an empty undo step is the shape of audit QT-01.
  `cad::planPurge` finds the set and iterates to a fixpoint, so purging a
  style frees the linetype only it named in the same purge; `continuous` and
  `none` are never in it, and the current style is kept. A merge INTO a
  library linestyle is not possible: `commands` cannot see the library, so
  `into` must be a model linetype (a rename onto the library name is the way,
  and its undo is now exact too).
- **Saving an unedited form is not an edit.** `updateStyleIfChanged` and
  `updateLinetypeIfChanged` return nullptr when nothing changed, so no undo
  step is pushed and the project is not marked modified; a Command cannot
  decline to be pushed, which is why it is the caller's helper.
- **ByLayer is a word** (`entity/display.hpp`). A linetype had no empty value
  to say "ByLayer" with - `""` is a name like any other - so a symbol-only or
  colour-only style overrode the layer's linetype with continuous whether it
  meant to or not. A `Style::linetype` of `ByLayer` (any case, since DXF
  reserves it and `validate(Linetype)` already refused it as a name) now
  inherits the layer's; `LAYER LTYPE` refuses it, because a layer is what
  ByLayer inherits from.
- **Pickers never drop a value** (`cad/style_catalogue.hpp`, decision D3).
  `linetypeChoices` offers ByLayer (for a style), the model's linetypes and
  every library definition offered as a linestyle - one entry per name, a
  collision marked; `symbolChoices` the built-in shapes and every library
  definition offered as a symbol (D3's four signals, `classifyDefinition`),
  a library definition hiding a built-in of the same name as it does when
  drawn. Each entry says its source (model, library, built-in, undefined), its
  file, group and units, and its users. `keepCurrent` puts the value being
  edited back, MARKED, when the list lacks it - that, not a more complete
  list, is what stops an editor rewriting a name it could not show, which is
  QT-02. Names are case-sensitive; `filterChoices` folds case.
- **What is wrong, listed, by one rule.** `cad::linetypeStatus` and
  `cad::symbolStatus` say what a name a Style or Layer gives draws as, worked
  out through `resolveLinePattern` and `resolveSymbol` - the viewport's own
  answers - as a `NameStatus`: Plain (`""`, ByLayer, and the plain lines
  `continuous`, `0` and `1` in any case, D4), OwnSymbol, Library, Katana (a
  model linetype or a built-in shape), NotALinestyle or Undefined. A name is
  missing (`isMissing`) exactly when what is drawn is a fallback: Undefined,
  defined nowhere; or NotALinestyle, a linetype that names only a `mode
  vertex` definition, which the viewport draws solid and a linetype picker
  does not offer (D2, D3) - the library DOES define it, as a symbol, so the
  fix is to pick a linestyle, not to load a library. `missingNames` gives
  every Style or Layer linetype and Style symbol that is missing, with its
  users, what is drawn instead and `MissingName::status`, linetypes first.
  Never a built-in symbol name (audit CAD-17, whose fix this rule
  finishes), ByLayer, a plain line, or a style's linetype that is its own
  symbol's name - what the 12da import writes for every symbol string, a
  plain line under the symbol (D8). Every list of the DRAWING's missing
  names reads this rule: the style manager's Missing chip and Diagnostics,
  the symbol library, `customisationCoverage` (which keeps its two reasons
  apart; `docs/survey_coding.md`, "Saying whether it is working") and so
  CUSTOMISE and the window's customisation log. Each once had a rule of its
  own, and they disagreed. The names a survey code file's rules give are judged apart
  - by the lint (`UnresolvedLinestyle`, `LinestyleIsVertex`) and by the
  load's "names the survey codes ask for" line - since a rule is not a style and
  has no style's own symbol. `NamePicker` marks the same
  names, with one exception: an own-symbol linetype is marked "(not
  defined)" there although `missingNames` leaves it out.
  `linetypeCollisions` gives the names both a model Linetype and a
  non-vertex library definition hold (D2's collisions; the library wins).
- **A current style** (D9): `Document::setCurrentStyle`, above.

All of it has command-line verbs (`STYLE USAGE`, `STYLE MERGE`,
`LINETYPE MERGE`, `STYLE SYMBOLS`, `STYLE CURRENT`, `PURGE`; "Command
interpreter" above). The layer halves of audits MOD-08, MOD-09 and MOD-12
are fixed as well (2026-09-24; `docs/model.md`, "Named tables"). Not yet: a
layer manager reading the per-layer counts in `TableUsage::layers` (it counts
with `countOnLayer`); a current style that FOLLOWS a rename; a cad-level merge
of a model linetype into a library linestyle (to settle a D2 collision); and
the code manager's own `linestyleState` in `code_manager_support.cpp`, a
plain / defined / wrong-kind rule that does not read `linetypeStatus`.

## What a style draws: one resolver, one painter

A name reaches the drawing from `Style::linetype`, `Layer::linetype` or
`Style::symbol`, and two tables can hold it: the model's Linetype table (DXF
dashes, saved in the project) and the session's style library (strokes,
texts and pens). Until 2026-09-23 the viewport looked in both with no rule for
which won, the preview looked in one, and the 3D view in the other.
`include/katana/cad/style_resolver.hpp` is now the one answer, for the
viewport, the plot and every preview and thumbnail:

- **A linetype name (decision D2).** A NON-vertex library definition of the
  name wins and is drawn by its own strokes, with NO Katana dash applied to
  them; otherwise a model Linetype gives dashes; otherwise the line is solid.
  A name both hold is a COLLISION: flagged (`ResolvedLinetype::collision`,
  listed by `linetypeCollisions`) and never an error - a project can meet a
  library that happens to reuse one of its names. `ByLayer` on a style is
  resolved to the layer's linetype before this is asked.
- **A symbol name.** The library definition, of either kind (most symbols the
  reference survey code file uses are not `mode vertex`), else a built-in shape - its
  own name's, or the one its words suggest, and the caller is told which.
- **Symbols on lines (decision D8)**: at every vertex, and the symbol's name
  never laid as a pattern - `resolveLinePattern`, "Point symbols" above.

**Nothing keeps a `LineStyle*` past the call.** A pointer into the library
dangles as soon as `setStyleLibrary` replaces it. What is kept across frames
is a `FlatDefinition` - a definition's strokes flattened once into runs, with
`factor` and the origin applied and its texts COPIED - in a `DefinitionCache`
keyed by name and `libraryGeneration()`: the first question at a new
generation empties it. A name the library lacks is remembered too, as an empty
answer, since the built-in fallback asks for it on every point of every
frame.

**Laying a linestyle for a view** (`layLinestyle`, `LinestyleOptions`; audit
CAD-04). The old `linestyleDrawing` laid a pattern along the WHOLE line and
cut it off at 20,000 repeats with no flag, so the tail of a long line vanished
and the viewport, having been told a definition applied, drew no plain line
either. Now:

- the budget is all-or-nothing, as `forEachDash`'s is: over
  `maximumInstances` nothing is laid (`OverBudget`) and the caller draws the
  plain line - a truncated pattern is a wrong drawing with no error anywhere;
- a period under `minimumPeriodPixels` (2) on screen is `TooFine`, and the
  plain line is drawn: below two pixels the repeats merge into a smudge the
  colour of the line, which is what the eye sees anyway, thousands of times
  cheaper;
- given the visible box, only the repeats that can reach it are laid - the box
  grown by one period and by how far the pattern reaches across the line - and
  each is laid where it falls on the WHOLE line, so the pattern keeps its
  phase from the first vertex and does not crawl as the view pans. Which
  repeats can reach the box is measured by where the MARKS reach
  (`drawnLowX`/`drawnHighX`: every run point, an arc's chords, a text's
  anchor), not by the pen's travel: an arc is written as a move to its
  centre, so a scallop's pen stands at the middle of a half-circle whose ends
  are a radius either side, and measured by the pen the last repeat at the
  end of a line was left out. A fuzz of 14,413 random definitions found 2,126
  such misses before that fix and none after; it was not committed, and
  `StyleDrawing` has a test per case.

The reference libraries write some arcs with a negative radius; the sign is not a side, and a
radius is taken as `|r|` (`docs/survey_coding.md`, "The grammar").

**One painter** (`src/katana_qt/customisation/style_painter.*`). The
viewport's private painting members became `paintStyleDrawing`,
`paintStyleText` and `stylePenFor`, because a preview could not reach them and
so drew something else. Everything the caller decides comes in a
`StylePaintTarget`: the model-to-device transform, the ENTITY PEN (its colour,
width and cap - a preview that guessed its own pen showed dashes one pen-width
shorter than the plot, whose square caps grow every dash), whether this is
paper, and `entityPenOnly` for the selection highlight, which must read as one
colour whatever the definition's pens say. A library pen changes only the
colour; a name `archive12d::standardColour` does not know leaves the entity
pen as it is rather than guessing. A one-point stroke is a library `dot`, painted
round whatever the cap (a flat cap draws a zero-length line as nothing). Style
texts are clamped at 2000 px, the plain-text ceiling in
`ViewportWidget::drawText` (a font asked for at hundreds of thousands of
pixels makes the raster engine allocate glyphs larger than any screen), and
not drawn under 3 px. The plot reaches the painter through
`ViewportWidget::drawEntities`, which `plotToPdf` reuses.

**White prints black (decision D7).** `PlotSettings::whiteToBlack`, on by
default, and `cad::paperColour`: a pen whose every channel is at least 230 of
255 prints black, alpha kept, as AutoCAD's colour 7 does. White is what a new
layer draws in on the dark screen and what 130 of the reference survey code file's 457
`map_data` rules ask for, and on white paper every such feature would vanish.
"Light grey" (211) and "light yellow" (255, 255, 224) keep their colour. It
applies to the entity's colour and to a library pen alike, and never on screen.

**Thumbnails** (`customisation/definition_thumbnails.*`). Small pictures of a
name as a symbol or as a linestyle along `styleSamplePath` - a straight run, a
sharp corner and a half-circle arc, so a pattern is seen turning and bending
- painted by the same painter from the same resolver, so a picker shows what
the viewport would draw. Each says when it is a STAND-IN (a symbol's built-in
fallback, or the plain line a linestyle name the library lacks is drawn as),
so a picker can mark such a name rather than drop it (D3). On a light ground a
picture is "on paper" - black entity pen, D7 applies - and on a dark one it is
the screen. Cached by kind, name, size and ground for ONE library generation
at a time, at most 4,096 pictures; painted on the GUI thread. Fitted to what
is actually painted (`paintedExtent`, which measures texts in their font),
not to `cad::drawnExtent`, whose deliberate over-estimate is right for culling
and shrank labelled symbols to a third of the picture. A linestyle picture
does not go through `styleSampleDrawing` (it needs a Model, and a project's
linetype must not enter a cache keyed on the library), and it is four periods
or eight reaches across, whichever is larger, so the pattern can be
recognised. The caller passes DEVICE pixels, so a HiDPI picker must multiply
by the device-pixel ratio.

The managers are built on it: `StylePreview` (the style manager's and the
symbol library's preview) paints through this painter from this resolver,
and every `NamePicker` entry and the symbol library's grid are these
thumbnails; the style manager's Diagnostics tab lists the collision flags
(`cad::styleDiagnostics`). Not yet: the 3D view draws neither symbols nor library
linestyles; and `NamePicker` paints every picture as it is built, the
drawing's own linetypes' uncached on every rebuild, so the code manager, whose
two pickers list about 800 definitions, takes about 2.4 s to build in a Debug
build on the reference map. Painting lazily, or caching the model pictures,
would help.

## The lead's decisions of 2026-09-23, and where each lives

Taken while the managers' foundations were built, and in force. They are
listed here so a later change can find the code that carries each one.

| | Decision | Where it is implemented |
|---|---|---|
| D1 | The style library and survey map are SESSION data on `cad::Document`: not undoable, not in the project. Map edits are made in an editor buffer and committed with `setSurveyMap` (Apply/Revert), and persist by EXPORT. A load MERGES by default; Replace is explicit. | `Document::setStyleLibrary`/`setSurveyMap` and the generation counters; `archive12d::mergeCustomisation` and `LoadMode`, which both front ends call (Format > Load Customisation... and Replace Loaded Customisation..., `CUSTOMISE [REPLACE]` in either command line; QT-21 fixed); the Survey Code Manager's buffer, Apply and Revert, and its Export Code File... (`writeMapFile`); the symbol library's Export Selected to .4d (`writeStyleLibrary`). No CLI export verb. `docs/survey_coding.md` |
| D2 | A linetype name: a non-vertex library definition wins (no dash on its strokes), else a model Linetype, else solid. `ByLayer` as a Style linetype inherits the layer's. | `cad::resolveLinetype`; `entity::isByLayer`, `resolvedLinetype`; `linetypeChoices`, `linetypeCollisions`; `STYLE SET ... linetype`; 12da export's `linestyleOf` |
| D3 | A library definition is a symbol if `mode vertex`, or a VertexSymbol rule names it, or a `Style::symbol` names it, or its file's name contains "symbol". Pickers always keep an unknown current name, marked (the QT-02 fix). Names are case-sensitive; search folds case. | `cad::classifyDefinition`, `symbolChoices`, `keepCurrent`, `filterChoices`; `LineStyle::source`; `codeTableRowMatches`; the lint's `SymbolNotSymbolCapable`; in the dialogs, `NamePicker` and the code manager's case-sensitive completers; the window's "(N symbols)" and CUSTOMISE's count |
| D4 | Survey coding chooses a code's style by appearance and reuses one that draws alike; new names follow from the rules. | `cad::applySurveyCodes` (`Appearance`, `drawsAs`, `existingStyleFor`, `nameFor`) |
| D5 | A code only the bare `*` answers is "fallback-only", not matched. | `entity::SurveyMatchKind`, `SurveyMatch::matched()`; `SurveyCodingReport::fallbackOnly`; `cad::splitStringName` |
| D6 | No storage schema migrations this round. | Why symbol rotation, a linetype scale and true arcs in linework wait; the metadata change is a key, not a column |
| D7 | White prints black on paper. | `cad::paperColour`, `PlotSettings::whiteToBlack`; `stylePenFor`; light-ground thumbnails |
| D8 | A line whose style names a symbol draws it at every vertex. | `cad::symbolVertices`, `resolveLinePattern`; the viewport's `drawEntities`; `styleSampleDrawing` |
| D9 | `Document::setCurrentStyle` feeds new work. | `Document::currentAttributes`; `STYLE CURRENT`; the Properties toolbar's current style; `PurgeOptions::keepStyles`, which Purge Unused fills |

## Grading: the batter belongs to the edge, not to the bisector

`cad::gradeToSurface` runs a batter outward from a feature line to the ground
using the corridor's daylight search - march in half-metre steps until the
batter and the ground change order, bisect to a millimetre - pointed
perpendicular to each edge and along the bisector at each vertex. The one
thing that had to be got right, and is easy to get wrong, is the slope IN
THE DIRECTION OF MARCH at a vertex. A 1 in 2 batter is 1 in 2 perpendicular
to its edge; along the bisector of a right-angle corner the same plane is
1 in 2√2. Marching the bisector at 1 in 2 puts the corner daylight 4 m out
on flat ground for a 2 m pad where the two batter planes actually meet at
4√2 m - a chamfered corner 29% short, and a volume to match. So the run per
rise is divided by the cosine of half the corner (`Sample::cosine`), and the
corner comes out mitred, which is what plane batters give and what the
frustum test checks exactly: 2/3 (100 + 324 + 180) = 402.667 m³.

**Rejected:** grading by offsetting the feature line a fixed width and
reading the ground there. That is a bench, not a batter: the daylight width
depends on the ground and differs on every side of the same pad on a slope.
**Rejected for now:** rounded (radial) corners, an option in every grading
package, because a plane batter to a mitre is what a bulldozer produces and
the rounded corner exists to look nicer on a drawing.

## Panels refresh on the event loop, never inside their own signal

Switching a layer off in the layer panel crashed the application. The chain
was: the box's `itemChanged` signal -> `execute(updateLayer)` -> the
document's change listener -> `refreshAll()` -> `refreshLayers()` ->
`QTreeWidget::clear()`, all synchronous, all while the item whose `setData`
raised the signal was still on Qt's stack. `clear()` deleted it, and
`QTreeWidgetItem::setData` read its parent pointer on the way out. The
property table had the same shape of bug behind an edited value.

The listener now calls `scheduleRefresh()`, which queues one `refreshAll()`
on the event loop however many times it is asked before that runs. It also
coalesces: a transaction of a hundred commands used to rebuild every panel a
hundred times. **Rejected:** guarding each slot with a re-entrancy flag,
because that leaves the next slot to make the same mistake; the rule is that
a document change never rebuilds a widget synchronously, and one function
holds it.

`katana --toggle-layer NAME --screenshot out.png` flips the box through the
real widget, headlessly, and refuses to continue if the tree was rebuilt
during the signal (it compares the item pointers before and after, without
dereferencing the old ones) or if the document and the panel disagree
afterwards. `qt_toggle_layer_headless` runs it on the sample; with the
listener made synchronous again it fails with "the layer panel was rebuilt
inside its own itemChanged signal", which is how the test earned its place.

A related rule, from the same afternoon: **a headless session never opens a
modal box.** `--plot` and `--screenshot` set `MainWindow::setHeadless`, under
which the "far from the current drawing" question keeps survey coordinates
and says so, and every "Import failed" box (`warnUser`) becomes a line in the
log. A scripted import of a file that did not exist used to sit on a warning
box until the test harness killed it. Since then: a question the window would
ask - discard unsaved changes, discard the code manager's unapplied edits - is
refused and said rather than answered; Format > Layers and Edit > Attributes,
still `exec()`'d, log that they are modal instead of opening; the Format
workbench tells each manager (`CustomisationServices::headless`), so the
symbol library and the code manager open no file dialog and the code
manager's close asks nothing; and Purge Unused does not ask.

## A listener lives exactly as long as the thing it notifies

`Document::addListener` used to return nothing and offer no removal. A
`ViewportWidget` registered `[this] { update(); }` in its constructor; when a
12d archive brought a surface, the layout switched to a split view, the plan
viewport was destroyed, and its registration stayed in the document. The
next command - a click on a layer's visibility box - called `update()` on
freed memory: a crash five runs in six, and a clean run the sixth time.

`addListener` now returns a `ListenerHandle` that owns the registration and
ends it when destroyed; `[[nodiscard]]`, so a registration cannot be made
without something owning it. The widget keeps the handle as a member declared
after the document reference, so it is the first thing destroyed. The handle
holds the registry through a `weak_ptr` because of a Qt ordering that is easy
to forget: child widgets are deleted in `~QWidget`, which runs AFTER the
window's own members - the `Document` among them - have been destroyed, so
the viewport's handle dies after the document it points to and must find
nothing there rather than something freed. `notify()` walks a copy of the ids
so that a listener may end a registration while notifications run.

**Rejected:** `QPointer` or a Qt signal in place of the std::function. The
Document is in `katana_cad`, which does not see Qt (Rule 4), and the fix
belongs where the defect is - a registry that hands out an obligation should
hand out the means to discharge it.

`qt_import_12da_then_toggle_headless` runs the scenario; the proof that the
mechanism works is in `test_cad.cpp`, since a run that survives by luck
still passes the headless test.

## The rules a dialog or panel follows

Collected here because each was paid for once, and a new manager is where
they are easiest to break:

- **No moc.** No `Q_OBJECT`, no custom signals: connections are lambdas and
  state is passed through `std::function` members (`DockTitleBar::onPressed`,
  `onMinimised`). A `QAbstractItemModel` or `QSortFilterProxyModel` subclass
  needs no `Q_OBJECT` and is fine.
- **Tables are read-only and a form edits**; a command is never run from a
  list's or table's own change signal ("The styles and linetypes manager").
- **A view reacting to the Document defers and coalesces** its reload to the
  event loop, never rebuilding a table inside its own signal ("Panels refresh
  on the event loop"). A manager does it through a `DocumentWatcher`
  (`customisation/document_watcher.*`), declared as its LAST member so it
  goes first: one delivery per turn of the event loop however many
  notifications came, saying what moved - the model (`Document::modelRevision`),
  the library or the map (their generations), the selection, the current
  layer or style.
- **A manager is non-modal and kept** by the workbench that opened it (the
  Format workbench, "The Format menu" above; the Survey workbench's dialogs),
  so it stays open beside the drawing and a buffer of unapplied edits
  survives hiding it.
- **A headless session never opens a modal box**, and every action, menu,
  field, button and tab has an object name, so tests and the headless driver
  can find it.
- **A dialog holding `Document&` must be deletable before the Document**, and
  a registration is owned by a `ListenerHandle` (above).
- **Logic that can be tested below Qt lives in `katana_cad`** -
  `src/katana_cad/customisation/*.cpp` and `include/katana/cad/*.hpp` are
  globbed, with tests in `tests/cad/customisation/` - and the dialog stays
  thin. What must be tested WITH Qt goes in `katana_qt_widget_tests`
  (`tests/qt_widgets/`, run offscreen as `qt_widgets.*`), which compiles
  `src/katana_qt/customisation/*.cpp` and `src/katana_qt/tools/*.cpp` and
  globs its own `customisation/` and `tools/` tests; `widget_harness.hpp`
  drives widgets by object name and asserts on the Document.


## Interactive tools: state machines in cad, a catalogue for the menus

The owner asked on 2026-09-23 for more CAD tools, with icons and menus, "to
make it professional CAD software". The tools the application had - Point,
Line, Polyline, Rectangle, Circle, Arc, Move, Copy - were a `switch` inside
`ViewportWidget::acceptPoint`, and the editing verbs (Rotate, Trim, Fillet,
...) existed only on the command line. Nothing about a tool's behaviour could
be tested without clicking, and every new tool meant another case in the view
and another hand-made action in the window.

**A tool is now a state machine in `katana_cad`**
(`include/katana/cad/interactive_tool.hpp`). The plan view tells it what the
user did - a snapped point, a picked entity, a typed value, Enter, Undo - and
the tool answers with the next prompt, a rubber-band preview for the cursor
and, when it completes, ONE command, so one undo removes the whole operation.
A tool never edits the document itself. Each is tested through
`tests/cad/tools/tool_driver.hpp`, which feeds it the inputs a user would and
executes what it returns, against geometry worked out by hand.

**One catalogue feeds everything that names a tool**: the menus, the toolbars,
the command-line aliases and the tooltips are built from `ToolInfo` entries,
so adding a tool is writing it and listing it in its family's file. The
catalogue refuses a duplicate id or alias, a lower-case alias and a
single-letter shortcut (a letter typed into a view goes to the command line);
a refusal is kept, not lost, and a test asserts there are none.

**The families are listed explicitly** (`src/katana_cad/tools/families.hpp`),
not self-registered. `katana_cad` is a static archive, and an object file
nothing references is dropped by the linker - taking its tools with it and
saying nothing; `src/katana_surveyio/CMakeLists.txt` measured that trap. The
source files are globbed so that families written at the same time do not
all edit one list; a family that goes missing is a link error, not a
silently empty menu.

**Typed input has one router**, `routeTypedInput`: text that looks like a
point (a comma, or the `@` of relative input) is a point - `x,y`, `@dx,dy`,
`@distance<angle` from the tool's last point - and anything else is a value,
so "12.5" reaches a Circle's radius rather than being refused as a malformed
point, and a clicked point and a typed one are the same input. The command
interpreter still has its own point parser with the same grammar; folding it
onto `parsePointInput` is a follow-up.

**Icons live with their family** (`src/katana_qt/tools/icons_<family>.cpp`,
by tool id), drawn to icons.cpp's conventions: a 24-unit grid, a 1.7-unit
stroke, neutral for the object and the accent for what the tool does to it. A
tool with no painter shows a framed initial, visibly a placeholder.
`katana_tool_icon_sheet` renders every catalogue tool's icon at menu, toolbar
and large size with its name and aliases, so an icon is reviewed without
launching the application. `tools::ToolInk` duplicates icons.cpp's private
`Ink` while other work is changing that file; merging them is a follow-up.

### The five families

Thirty-six tools in five families were merged on 2026-09-23, each family one
file (or a few) in `src/katana_cad/tools/`, each tool tested through
`ToolDriver` against geometry worked out by hand, and each with an icon. The
aliases are AutoCAD's plus the command interpreter's own spellings of the
same verbs, so a word means one thing whichever reads it:

| Family | Tools (aliases) |
|---|---|
| Draw > Lines (`draw_lines.cpp`) | Point (`POINT`, `PO`), Line (`LINE`, `L`), Polyline (`PLINE`, `PL`, `POLYLINE`), Rectangle (`RECTANG`, `REC`, `RECT`, `RECTANGLE`), Polygon (`POLYGON`, `POL`) |
| Draw > Curves (`draw_curves.cpp`) | Circle (`CIRCLE`, `C`) and its Centre Diameter, 2 Points, 3 Points and Tangent Tangent Radius variants; Arc (`ARC`, `A`, three points) and its Start Centre End, Centre Start End and Start End Radius variants - a variant has no verb, being an option of the general tool as in AutoCAD |
| Modify > Transform (`modify_transform.cpp`) | Move (`MOVE`, `M`), Copy (`COPY`, `CO`, `CP`), Rotate (`ROTATE`, `RO`), Scale (`SCALE`, `SC`), Mirror (`MIRROR`, `MI`), Stretch (`STRETCH`, `S`), Rectangular Array (`ARRAYRECT`, `ARRAY`, `AR`), Polar Array (`ARRAYPOLAR`), Erase (`ERASE`, `E`, `DELETE`, `DEL`) |
| Modify > Edit (`modify_edit*.cpp`) | Trim (`TRIM`, `TR`), Extend (`EXTEND`, `EX`), Offset (`OFFSET`, `O`), Fillet (`FILLET`, `F`), Chamfer (`CHAMFER`, `CHA`), Break (`BREAK`, `BR`), Break at Point (`BREAKATPOINT`), Join (`JOIN`, `J`), Explode (`EXPLODE`, `X`) |
| Annotate (`annotate*.cpp`) | Text (`TEXT`, `DTEXT`, `DT`), Linear Dimension (`DIMLINEAR`, `DLI`), Aligned Dimension (`DIMALIGNED`, `DAL`), Leader (`LEADER`, `LEAD`, `LE`) |

The sixth family in `families.hpp`, Inquiry (Distance, Area, ID Point, Angle,
List), is listed and empty. The Survey menu's tools (`docs/survey.md`) are
dialogs, not catalogue tools; whether they join the catalogue is the lead's
to decide.

**One command per tool session.** A tool that completes returns ONE command,
so one undo removes the whole operation: a chain of Line segments, every copy
Copy made before Enter, every cut of a Trim. The edit tools need more than
that, because a later pick can land on a piece an earlier pick made - which
has no id until a command runs - and a fillet must keep the side of each line
the user picked. So Trim, Extend, Offset, Fillet, Chamfer, Break and Join
work on an `EditSession` (`modify_edit_support.hpp`, family-internal): a
layer over the document that records what the session has made of each
entity and what it added, with `begin()`/`undo()` per operation for the `U`
inside the tool, and `commit()` turning the whole session into one command
when the tool finishes, as AutoCAD's U does after a TRIM. Every piece is
checked as it goes in (`drawable`, the geometry's own `validate`): if one is a
shape the drawing would refuse, the whole operation since `begin()` is
abandoned with a sentence saying why, so the command a session becomes is
always one the drawing accepts.

**What the edit tools remember and refuse.** `EditDefaults` holds the fillet
radius, the two chamfer distances, the offset distance (Through until one is
typed) and a 1 mm join tolerance, one object per program, as AutoCAD keeps
FILLETRAD, CHAMFERA/CHAMFERB and OFFSETDIST - a user who fillets at 5 expects
5 next time - and nothing of it is in the drawing. Offset refuses an INWARD
offset beyond the polyline's local feature size: `geometry::offset` mitres
each vertex, and past that size the sides pass each other, so an 8 x 8 square
offset inward by 5 came out as a 2 x 2 square drawn the other way round when
no such offset exists (`keepsItsSides`). Offset also refuses an object on a
locked layer, since the copy would land there. Break at Point refuses a
circle ("A circle has no ends, so one point cannot split it; use Break with
two points."). Fillet and Chamfer take lines only, Join always makes a
polyline, and a pending offset cannot itself be picked within the session.

The modify-edit tests reach `modify_edit_support.hpp` through an include
path of their own (`tests/cad/CMakeLists.txt`), as the family's sources do.

### The tool host: how a view runs a tool

The plan view's own `switch` is gone. Every tool the view runs is a catalogue
tool, run by a **`ToolHost`** (`src/katana_qt/tools/tool_host.*`) that sits
between the tool, which knows nothing of Qt, and the view, which knows nothing
of any one tool. It starts a tool with the document's current attributes
(layer and style, D9) and the live selection, hands it what the user did - a
snapped point, a picked entity, typed text, Enter, Esc, Undo - and when the
tool finishes executes its ONE command through the Document. It restarts the
tool when the tool asks (Circle, Point), and gives the tool the view's pick
aperture in model units, so Trim's preview and picks match the zoom. It
reports through hooks and opens nothing, so a test drives it by calling it
(`tests/qt_widgets/tools/`). A generation count, bumped whenever a tool is
made, remade or dropped, is how the host knows that a hook replaced the tool
it was dealing with: a tool started from a hook was once allocated at the
address of the one it replaced.

**Esc keeps what AutoCAD keeps.** Esc ends the tool, but a tool holding work
that its Enter only ever COMMITS - a Line or Polyline chain, the cuts of a
Trim or Extend, Offset's copies, a run of Fillets or Chamfers - is sent Enter
first, so Esc keeps that work as AutoCAD keeps the segments of a LINE
(`tools::escapeKeepsWork`, a list of tool ids). Every other tool is dropped
with nothing done, because its Enter at some step applies a DEFAULT - Move's
"use the first point as the displacement", Join's "join what is selected" -
which Esc must never do; Copy is dropped for that reason although its placed
copies are collected work. A Fillet or Chamfer at a VALUE prompt (its radius,
its distances) is first stepped back out of it: Enter there takes the prompt's
default - at Chamfer's second distance it stores both distances for every
later Chamfer - or, at Fillet's radius, only returns to the lines, and the
corners a Multiple run made would go with the tool. The list stands in for a
`cancel()` the tool interface does not have; a virtual commit-on-cancel on
`InteractiveTool` would replace it.

**A replaced drawing ends the tool.** New and Open call
`ViewWorkspace::resetInteraction`, which ends the running tool WITHOUT
committing anything (`ToolHost::abandon`): its picks and ids belong to the
drawing that is going, and restarting it would read the old drawing's layer
and selection. So does a plan view that is closed or changed into another
kind while its tool runs (`ViewWorkspace::stopToolIn`, called by `closeView`
before it finds the view to erase and by `buildContent`) - the view stops
the tool while it can still say so, which is what un-checks the tool in the
menus.

**Typed input belongs to the running tool.** While a tool runs, what is typed
over the view is kept in the view (`typedInput()`) and shown after the prompt
in a band along the bottom of the view, until Enter or Space sends it
(Space is a space inside a value being typed), Backspace takes a character
back and Esc clears it. With no tool running a printable key is the start of
a command and goes to the window's command line ("type anywhere",
`onTextTyped`), never a Ctrl or Alt chord, which is a shortcut. On the command
line itself (`MainWindow::runCommandLine`):

- while a tool runs, the whole line is the tool's answer
  (`ViewWorkspace::typeIntoTool`) - a point, a distance, an option - so
  Polyline's `C` closes it where on its own `C` would start a Circle. Nothing
  typed is transparent: `ZOOM` typed during a tool goes to the tool too;
- with none running, a single word that is a tool's alias or its catalogue id
  starts it (`tools::toolIdForCommand`: `L`, `line`, `TRIM`, `draw.circle.ttr`,
  aliases case-insensitively), and the tool's action is checked in the menus
  and toolbars as if it had been clicked; with arguments the word is the
  interpreter's (`LINE 0,0 10,0` draws at once);
- an empty line is Enter in the drawing (`ViewWorkspace::pressEnter`),
  delivered as a real Return key to the plan view running a tool - or, with
  none running, to the active plan view, which starts the last tool again -
  so the command line and the view share one Enter path.

`qt_a_tool_started_by_its_alias_draws_from_typed_points_headless` types
`LINE`, two points and two empty Enters, and checks the line and the check
marks.

**Ctrl+Z inside a tool is the tool's.** The view claims the key at
`ShortcutOverride`, so the window's Undo does not run, and acts on it at the
key press, since Qt may ask more than once for one press. It steps back the
tool's last input - the `U` inside LINE - and a tool with nothing to step
back says so rather than undoing the drawing: the drawing's Undo is Esc and
then Ctrl+Z.

**A tool's selection step gathers.** At a step that wants objects, a plain
click or box ADDS what it picks, as AutoCAD's "Select objects" does, and one
with Shift or Ctrl held takes it back out; replacing the selection at each
click, as the Select tool does, would leave only the last of several cutting
edges picked. The picks change the DOCUMENT's selection, which the tool reads
when Enter is pressed, so the view keeps what each click or box replaced and
Ctrl+Z (or a typed `U`) at that step takes them back in the order they were
made, together with any input the tool took itself (`All`).

**Enter or Space repeats.** With no tool running, Enter or Space in a plan view
starts the last tool again, as AutoCAD repeats the last command: a run of
circles is a click on Circle and then Enter between them. The workspace keeps
the last tool started in ANY plan view (`onRepeatTool`), so the repeat goes
through `ViewWorkspace::startTool` like every other start.

**One tool per workspace, and Esc to the busy view first.** A tool holds picks
made in one view, so it runs in the active plan view only, and starting one
stops, as Esc stops it, a tool running in any other plan view; it stays in the
view it started in, where AutoCAD would carry a command across viewports. Esc
(`ViewWorkspace::cancel`) reaches only the views running a tool or holding
typed input for one; only when none is does every plan view abandon its box
and clear the selection - so the first Esc ends the tool and keeps the
selection it was started on, and the second clears it.

**The menus are the catalogue.** `tools::fillToolMenus`
(`src/katana_qt/tools/tool_menus.*`) builds the Draw, Modify and Annotate
menus and toolbars from `cad::toolCatalog()`: one `QAction` per tool, shared by
its menu and its toolbar and named by the tool's id (`draw.line`), with its
family's icon, its shortcut, its tip as the status tip and a tooltip naming its
aliases ("Line (LINE, L)"). Groups are separated in a fixed order (Lines,
Curves, Transform, Edit, Text, Dimensions, Leaders); tools named "Family,
Variant" ("Circle, 2 Points") are gathered into a submenu by family, and on a
toolbar the family is ONE button that runs its first variant and drops the
rest down. A category no menu takes is still reached by its aliases, and the
window says so in its log at start-up. An action only asks the window to
start its tool (`MainWindow::startTool`), which decides the view. The tool
actions are checkable, in one exclusive group; Select (`toolSelect`, at the
head of the Draw toolbar) is not a tool but the absence of one: it stops what
runs, and is checked while nothing does. What is checked always follows what
runs (`MainWindow::showRunningTool`, from `onActiveToolChanged`), including
when a start is REFUSED - a click checks an action before its handler runs, so
a tool that could not start, for want of a plan view, used to stay checked
beside Select (`qt_a_tool_refused_for_want_of_a_plan_view_is_left_unchecked_headless`).
While a tool runs its prompt is the command line's placeholder text, where the
answer is typed.

Not done: `ViewportWidget` still has the `enum class Tool` of the first eight
tools and `setTool`, used for Select (`stopToolIn` calls it) and by
`test_plan_view_tools.cpp`; the window and the workspace name tools by id.
The picks of an entity step (Trim's edges, the part to cut) are not
highlighted, and `ToolContext` carries no view's layer overrides.
