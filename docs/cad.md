# CAD engine

`katana_cad`: the UI-independent application logic that both front ends
drive - the open document, selection and picking, snapping, the view
transform, the command interpreter, the spatial index, the civil computations
(alignments, profiles, corridors, parcels, grading), plotting arithmetic, and
the style and symbol resolution every view, plot and preview draws through.

Until 2026-09-24 this document also held the desktop application, the
interactive tools and the headless driver. Code comments cite their sections
as `docs/cad.md`; this table says where each one went:

| Section cited as `docs/cad.md, "..."` | Now in |
|---|---|
| "Desktop application" (now "The window"), "Failure modes" (closing the window), "The look of the application", "The workspace", "The dock chrome", "The layer manager", "The attribute manager", "The Format menu", "Styles and Linetypes", "Symbol Library", "Survey Code Manager", "Panels refresh on the event loop", "The rules a dialog or panel follows" | `docs/desktop.md` |
| "Interactive tools", "The five families", "The tool host" | `docs/tools.md` |
| "Headless front end", `--screenshot` and the driver switches, "a headless session never opens a modal box" | `docs/headless.md` |

## Purpose

`katana_cad` holds everything an interactive CAD session needs that is not a
widget: the open document, selection and picking, snapping, the view transform
and the command-line interpreter. All of it is unit tested without a GUI, and
the Qt application and the CLI drive exactly the same objects. That split is
what lets the engine be exercised, scripted and regression-tested with no
display attached — and it is what will let an application API expose the
same operations without duplicating logic (Rule 2).

## Document

`Document` owns the `Model`, the `CommandStack`, the current `ProjectStore`, the
selection, the current layer, the current style and the project metadata - and,
as session data outside the model, the loaded style library and survey map
(decision D1; `docs/survey_coding.md`).

* The CURRENT STYLE (decision D9, `setCurrentStyle`) is what new work is drawn
  in - a CAD program's current linetype, or a current point style - so a symbol or a library
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

* `execute`/`undo`/`redo` delegate to the command stack. `execute` wraps each
  command with the associative update (`cad/annotation/associative.hpp`):
  when the command moves or reshapes an entity that a dimension, label or
  leader follows, the annotation is brought up to date by the SAME undo
  step, so an undo puts both back together.
* The ANNOTATION SCALE (`annotationScale`, `setAnnotationScale`) is the scale
  the plan view draws paper-sized annotation at, 1 : 1000 until one is set
  (`docs/annotation.md`). Setting it is one undoable step, and it is kept in
  the project's metadata under `annotation_scale`, a key the storage layer
  carries without reading it, so it needed no schema change.
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
`UNDO`, `REDO`, `NEW`, `OPEN`, `SAVE`, `LIST`, `INFO`, `HELP`, and the sheet,
annotation, utility and survey-code families below. Aliases include
`LT`/`LTYPE`, `DS`, `HA`, `ST`, `AL` and `PARC`. This list lacked the tables
and civil verbs until the audit of 2026-09-23.

The sheet verbs (`SHEETS`, `SHEET`, `VIEW`, `TILE`, `GENERATE`,
`TITLEBLOCK`, and `HELP SHEETS`) are handed to `plotting::runSheetVerb`;
`docs/plotting.md`, "Sheets on the command line", describes them.

The AS 5488 subsurface utility verbs (`UTILITY REPORT`, `VERIFY`,
`CLEARANCE`, `CHECK`, `DRAW`, and `HELP UTILITY`) are handed to
`utilities::runUtilityVerb` (`include/katana/cad/utilities/`);
`docs/subsurface_utilities.md`, "UTILITY on the command line", describes
them. They were `katana_cli`'s own until 2026-09-25; in the interpreter the
window's command line, `katana_cli` and `katana_mcp` all have them. The four
reports read files and never touch the drawing; `UTILITY DRAW` adds the graded
services - layers by type and quality level, a linetype per level, a
polyline per run at one level, a point per located vertex - as one undo
step, and its first reply record carries the `bounds=` a front end frames.

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
| `MODIFY [SELECTION\|DRAWING\|LAYERS a,b [ONLY]] [WHERE k=v ...] SET k=v ... [PREVIEW]` (`GM`, `GMODIFY`, `GLOBALMODIFY`) | Global Modify ("Global Modify", below): the entities in a scope and filter, the layers they sit on and the styles they wear, changed as one undo step; `PREVIEW` prints the plan and changes nothing |

The annotation verbs (2026-09-25; `docs/annotation.md`, "The verbs") are
`ANNOSCALE`, `TEXTSTYLE` (`TS`), `TEXT` and `MTEXT` with `style=`, `paper=`,
`justify=` and `rotation=`, `TEXTEDIT`, `LABELSTYLE` (`LS`), `LABEL`,
`AUTOLABEL`, the `DIM` kinds (`DIM LINEAR|HORIZONTAL|VERTICAL|ALIGNED|ANGULAR|
RADIUS|DIAMETER|ORDINATE|BASELINE|CONTINUE`), `LEADER` (`LE`) and `BALLOON`,
and `DIMSTYLE SET <name> PAPER on`. They take `key=value` options and reply
in `key=value` lines, so a script or an agent reads what happened; each edit
is one undo step. `CommandInterpreter::annotationHelpText` is their
reference, and `HELP` prints it after the rest. The plain `TEXT p height
"text"` and `DIM p p offset` are unchanged.

The survey-code verbs (`CODE [property]`, `CODE EXPLAIN code`, `CODE CENSUS
[property]`, `MAPFILE LIST [filter]`, `MAPFILE CHECK`; `docs/survey_coding.md`)
are handed to `runSurveyCodeVerb` (`include/katana/cad/survey_code_verbs.hpp`)
since 2026-09-26; they were `katana_cli`'s own, and the window refused them.
`CODE` is one undo step, and `MAPFILE CHECK` is refused - its whole lint in
the refusal, as `UTILITY CHECK` carries its check - when a rule has an error
(`mapfileCheckReply`). The standard colour names they need are archive12d's,
which cad may not see, so each front end passes the table with
`CommandInterpreter::setColourLookup`; without one no colour is known and
colours are left alone.

Each front end adds verbs of its own, because `katana_cad` may not see GDAL,
PDAL or the archive and customisation readers: the application's command line
(`MainWindow::runCommandLine`, whose `dispatchLine` the window's dialogs run
their lines through too - `docs/desktop.md`, "One executor: the command
runner") adds `IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]`, `EXPORT`, `INFO <file>`, `REFS`,
`COPC`, `CUSTOMISE [REPLACE] <file>...` (alone, the loaded customisation's
report, `cad::customisationReport`), `PLOTSHEETS`, the view verbs `ZOOM`,
`GRID` and `SNAP`, and `QUIT`; `katana_cli` adds the same interoperability
verbs and `CUSTOMISE` - its `--help` lists them. Both read an `IMPORT` line's
path and `LOCAL` with `CommandInterpreter::importArgument`, and hand `INFO`
with an entity id (`CommandInterpreter::isEntityId`, a whole number or `#n`)
to the interpreter unless a file of that name exists.

In the application a typed line is routed before the interpreter sees it
(`docs/tools.md`, "The tool host"): while a tool runs, the whole line is that tool's
answer; a single word that is a tool's alias or id starts the tool; the same
word with arguments (`LINE 0,0 10,0`) is still the interpreter's, which is
what scripts and the headless checks type; and an empty line is Enter in the
drawing.

## The project's coordinate system

A project keeps its coordinate system as text in its metadata
(`storage::ProjectMetadata::coordinateSystem`): `EPSG:7856`, or WKT or a PROJ
string for a system with no code, or nothing for local coordinates. GIS >
Online Data needs it to know where a web map goes, GIS imports reproject into
it, and the sheets print it in the title block, so it is set in one place:

- `Document::setCoordinateSystem` sets it as ONE undo step (`SET_CRS`). It
  reads anything PROJ reads, through `geodesy::CoordinateReferenceSystem`, and
  stores `EPSG:<code>` whenever the system has a code, so `7856`, `epsg:7856`
  and the system's WKT are one value. Text that names no system is refused
  (`InvalidCRS`) and changes nothing; the value it already has is no step.
- `include/katana/cad/project_crs.hpp` describes a system (its registered
  name, kind, units and area of use), lists the common ones (the GDA2020 and
  GDA94 MGA zones 49 to 56, the national Albers and geographic systems, WGS 84
  and its UTM zones, Web Mercator, New Zealand's and Great Britain's grids)
  and suggests the systems that suit a place, best first: in Australia the
  place's GDA2020 MGA zone, then GDA94's, then WGS 84 UTM, then WGS 84.
- On the command line, `CRS` shows it, `CRS SET EPSG:7856` (a code, WKT or
  PROJ) sets it, `CRS CLEAR` returns to local coordinates, `CRS FIND mga 56`
  searches the common list and `CRS SUGGEST 151.21,-33.87` names the systems
  for a place. Replies are one fact per line: `crs id=EPSG:7856 name="GDA2020
  / MGA zone 56" kind="projected" units=metre`.
- In the window, File > Project Coordinate System
  (`src/katana_qt/project_crs_dialog.hpp`) lists the common systems grouped,
  searches them, checks a typed one as it is typed, and sets it; the status
  bar shows the system and opens the dialog when clicked; and GIS > Online
  Data's Set Project CRS opens it with the systems for the typed box first.

The tests are `tests/cad/test_project_crs.cpp` and
`tests/qt_widgets/test_project_crs_dialog.cpp`.

## Threading and ownership

Single-threaded; everything belongs to the GUI thread (`docs/architecture.md`,
"Threading"). `MainWindow` owns the
`Document`, which owns the model, the command stack and the project store. The
viewport holds a reference to the document and must not outlive it. Listeners
must not mutate the document re-entrantly.

## Performance

Drawing, picking and snapping ask the spatial index ("Spatial
indexing", below) for the entities near the view or the cursor, with an exact
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
polyline already exceeded the 16 ms selection target (`docs/architecture.md`,
"Performance targets") by itself.

The targets ask for 60 FPS interaction and sub-16 ms selection. At drafting
scale a scan met them; the spatial index below is what holds them at hundreds
of thousands of entities, and the pick and cull call sites stay deliberately
narrow so that a different index could be put behind them.

## Spatial indexing

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
silent failure `docs/architecture.md` ("Error handling") forbids.

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
which is the green-over-red the working rules warn of (`docs/architecture.md`,
"Judge a build by its exit code"). Builds are now judged by
ninja's exit code.

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
documented as "millimetres on paper" since the layer table was built,
validated, persisted,
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

## Point symbols

A point is drawn with the symbol its *style* names - `Style::symbol`. Since
the survey coding work that is any name, resolved at draw time against the loaded style
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
while the 12da import's comment said the opposite and meant it. It was
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

`geometry::TriangleMesh` is a list of points and a
list of triangles naming three of them each: what an archive's `primitive_3d`
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

## Global Modify: a scope, a filter and a change, as one command

The owner asked on 2026-09-25 for "a global modification tool that can act on
data on a selected view, layers, selected features, etc., that can change
styles, symbols, layers, colour, etc." - 12d's Change panel, and AutoCAD's
Quick Select followed by the Properties palette. It is
`include/katana/cad/global_modify.hpp`, in three plain values, so the
window's dialog (`docs/desktop.md`, "Global Modify"), the command line's
`MODIFY` and the tests state a request the same way:

| Part | Type | Says |
|---|---|---|
| Where | `ModifyScope` | the selection; what one view draws (its own hidden layers, and optionally only what overlaps its visible area); named layers, with or without their sublayers; or the whole drawing |
| Which | `ModifyFilter` | types, layer and style patterns, the entity's own colour, a property and its value, a text's words, drawn only - `*` and `?` wildcards, case folded (`matchesPattern`) |
| What | `GlobalModify` | the entities' own attributes (`EntityModify`: layer, colour, style, shown, properties, text height, the symbol a point draws), the layers they sit on (`LayerModify`) and the styles they wear (`StyleModify`) |

`planGlobalModify` turns them into ONE command (a `commands::Transaction`
named `GLOBAL_MODIFY`), so one Undo puts back every entity, layer and style
(Rule 2), and a report, `GlobalModifyPlan`, worked out before anything runs:
what matched, which entities, layers and styles change, and what is left
alone and why. `summary()` is what the dialog shows as its preview and
`MODIFY` prints. Decisions worth knowing:

- **Absent is left alone.** Every field is a `std::optional`; one whose value
  may be ByLayer is an optional of an optional, the inner nullopt being
  ByLayer - the reading `Entity::color` already has. Asking for no change at
  all is refused as a mistake; matching nothing, or matching only what is
  already as asked, is a plan with no command, so no empty undo step is
  pushed.
- **Locked layers.** An entity on a locked layer is counted and left, as
  Match Properties leaves one - unless the same request unlocks that layer,
  which is then done first. A lock is done last, after the entities on the
  layer are changed. Moving onto a locked layer is refused before anything
  runs; moving onto a layer the drawing lacks makes it, and only when
  something actually moves.
- **A symbol goes through a style**, found or made by
  `cad::assignSymbolToPoints` (`include/katana/cad/symbol_assign.hpp`), because
  a point carries no symbol of its own. Other entities in scope are counted,
  not moved; a point given a style and a symbol ends in the symbol's style.
- **A style is shared.** Changing the styles the matched entities wear
  redraws everything wearing them, in the scope or not; the plan counts those
  (`styleReachesOthers`) and the summary says so, so no front end can hide
  it.
- **Layer settings** go to the layers the matched entities sit on - or, for a
  Layers scope, to the scope's layers themselves, entities or not.
- **One rule for linetype names.** The command line's check that a name is a
  model linetype or a library linestyle, and not a vertex symbol, moved from
  the interpreter to `cad::checkLinetypeName`
  (`include/katana/cad/style_catalogue.hpp`), which Global Modify asks too.

Tested in `tests/cad/customisation/test_global_modify.cpp`, each expectation
worked out by hand from the drawing its fixture lays out.

## What the managers stand on


What was merged on 2026-09-23 is the tested logic below Qt that the managers
above need, each piece reachable from the command line too; the dialogs of
2026-09-24 (`docs/desktop.md`, "The Format menu") render it and decide
nothing of their own. The managers of professional CAD and survey packages
share a vocabulary -
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
`PlanPainter::drawText` (a font asked for at hundreds of thousands of
pixels makes the raster engine allocate glyphs larger than any screen), and
not drawn under 3 px. The screen and the plot reach the painter through the
same `paintPlan` (`src/katana_qt/plan_painter.hpp`, docs/plan_view.md); a
plot calls it from `plotPlanToPdf`, which needs no widget.

**White prints black (decision D7).** `PlotSettings::whiteToBlack`, on by
default, and `cad::paperColour`: a pen whose every channel is at least 230 of
255 prints black, alpha kept, as colour 7 conventionally does in CAD. White is what a new
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

## The decisions of 2026-09-23 (D1 to D9), and where each lives

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

## A listener lives exactly as long as the thing it notifies

`Document::addListener` used to return nothing and offer no removal. A
`ViewportWidget` registered `[this] { update(); }` in its constructor; when an
imported archive brought a surface, the layout switched to a split view, the plan
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
