# Terrain

Decisions and measurements for `katana::terrain`. `PLAN.MD` sections 19 (Phase
14), 24 (Phase 19) and 32 govern this area.

## Where the time actually goes

Terrain was the largest area of the codebase with no benchmark, so Phase 19's
outstanding list had been written from the shape of the API rather than from
evidence: it named a task graph, work stealing, TBB and SIMD. `benchmarks/bench_terrain.cpp`
was added to settle it. Rule 6 — profile before optimising.

Release, GCC 16.2 UCRT64, 16 × 2496 MHz, L3 18 MiB, `--benchmark_min_time=0.3s`.
(First recorded here as GCC 15.2, from CLAUDE.md. The MSYS2 toolchain had been
upgraded to 16.2 an hour before these were taken - `pacman.log`, 2026-09-21
00:15 - so 16.2 is the compiler that produced every number in this file.)
Points are a fixed-seed LCG scatter over a 1 km square with a rolling ground
surface, which is what a lidar ground classification delivers; a flat plane
would have measured faster than any real job.

| Benchmark | Time | Note |
| --- | --- | --- |
| `buildTin` 25 000 pts | 87.2 ms | 49 970 triangles |
| `buildTin` 50 000 pts | 180 ms | 99 966 |
| `buildTin` 100 000 pts | 360 ms | 199 963 |
| `buildTin` 200 000 pts | 769 ms | 399 966 |
| `buildTin` 400 000 pts | **1681 ms** | 799 960 |
| `buildTin` 100k + 20 breaklines × 200 | 406 ms | +12% over unconstrained |
| `buildTin` 100k + 100 breaklines × 200 | 561 ms | +55% over unconstrained |
| `elevationAt` | 499 ns | 2.05 M/s |
| `contours`, 5.0 m interval | 48.3 ms | ~5 bands |
| `contours`, 1.0 m interval | 57.7 ms | ~25 bands |
| `contours`, 0.2 m interval | 95.4 ms | ~125 bands |
| **`compareSurfaces` 200k vs 50k** | 12 150 ms -> **2 141 ms** | see below |

### What that changes

**`compareSurfaces` was the bottleneck, not `buildTin`.** Twelve seconds, on a
site that is not large, for the single number an earthworks job exists to
produce — 7.2× the worst `buildTin` in the table and 32× the cost of building
the design surface it compares against. Nothing in Phase 19's original
outstanding list would have touched it. It has since been fixed; the diagnosis
is kept because it is the reason the fix took the shape it did.

The reason is in `volume.hpp`: the overlay merges both triangulations into one
constrained triangulation with *every* edge of *both* surfaces as a constraint,
plus every crossing point. Comparing a 200k-vertex surface with a 50k-vertex one
therefore means a constrained insert of roughly 250 000 vertices and 750 000
constraint edges — and the breakline rows above show constraint is what is
expensive: 100 breaklines of 200 vertices (20 000 constrained edges, 8% of the
point count) add 55% to a build.

That exactness is not negotiable and must not be traded for a sampled grid - it
is the property `volume.hpp` promises and the reason the answer can be signed
off. So the fix had to produce the *same* answer, faster.

### The fix: clip triangle pairs, do not triangulate the union

Instrumenting `compareSurfaces` split its 12 150 ms as:

| Stage | Time | Share |
| --- | --- | --- |
| collect points and constraints | 101 ms | 1% |
| **constrained triangulation of the overlay** | **9 937 ms** | **82%** |
| integrate over the overlay triangles | 2 072 ms | 17% |

250 000 merged points and 749 928 constraints, producing 1 981 985 overlay
triangles. The global triangulation was the whole cost.

It was also unnecessary. The overlay exists for exactly one reason: every piece
must lie inside one triangle of each surface, so that the elevation difference
is linear over it. The intersection of two triangles is already such a piece -
both are convex, so the intersection is a convex polygon of at most six corners,
and a fan from one of its corners gives triangles that need no new vertices. The
set of those intersections, over every overlapping pair, is the same partition
the global triangulation was building, obtained pairwise.

Pairs are found through `geometry::SpatialIndex` over the design triangles,
which is the broad phase the codebase already has rather than a second one.

**12 150 ms to 2 141 ms, 5.7x**, with 765 tests passing and no expected value
changed. `buildTin`, `elevationAt` and `contours` were re-measured after the
change and did not move.

Two things improved besides the time:

* **No point location.** The old integration called `locate(centroid)` on each
  surface for every overlay triangle, and fell back to sampling the corners
  individually when the centroid landed on a rim or a sliver - a fallback that
  is not exact. Clipping knows which pair produced each piece, so both planes
  are evaluated by barycentric weights that are in [0, 1] by construction.
* **One less dependency.** `volume.cpp` no longer includes the CDT backend or
  the point merger at all.

The clipper's inside test is `cross >= 0` rather than `> 0`, and that is
load-bearing, not a detail. When a subject corner lies exactly on a clip edge -
the normal case for triangles that share an edge - the cross product is exactly
zero in IEEE arithmetic, because it reduces to `a - a` for the two points that
define the edge. Treating it as inside means no crossing point is constructed
and the corner is reproduced bit for bit, so two triangles sharing an edge clip
to exactly that segment, whose fan has signed area exactly `0.0` and is
rejected. With `> 0` the same case would build a sliver of some arbitrary tiny
area, and `CompareSurfaces.ASurfaceAgainstItselfIsExactlyZero` would no longer
hold.

**What the cull is tested against.** CLAUDE.md section 4 requires proving a
culled run finds what an exhaustive one did.
`EveryPieceOfTheCommonGroundIsFoundByTheIndexedSearch` uses the one quantity
known in closed form: both surfaces carry the corners of the same 100 m square,
so the pieces must tile exactly 10 000 m^2, and any pair the index failed to
return is missing area. It was shown to fail rather than assumed to - dropping
one candidate in every seven moves `planArea` by 779 m^2 and fails it and five
older tests. Its sensitivity has a floor, recorded in the test: shrinking the
query box by 5 cm loses only sub-millimetre boundary slivers and is not caught.
`TheAnswerDoesNotDependOnHowTheGridBucketsTriangles` translates the whole site
12 345 m so every triangle lands in a different cell.

**Since done.** 2 141 ms was not fast; it was 5.7x less slow. The remaining
cost was spread across roughly two million pair clips, which the `TaskPool`
suits: existing triangles are independent and the only shared state is the
compensated sums. It now runs on the pool with per-block sums combined in block
order - see *compareSurfaces and contours on the TaskPool* below: about 220 ms
on this benchmark, and the same bits at any thread count.

**`buildTin` is second, and it is the one every user reaches** — it runs on
import and on every surface rebuild. 1681 ms at 400 000 points is over
`PLAN.MD` section 32's interaction budget by two orders of magnitude, although
it is a batch operation rather than a per-frame one. Cost per point rises from
3.49 µs at 25k to 4.20 µs at 400k, consistent with the n log n of incremental
Delaunay insertion rather than with anything quadratic.

Parallelising it is not straightforward and the plan should not pretend
otherwise: incremental Delaunay insertion is sequential by construction, and
Rule 7 requires the same triangulation whatever the thread count, so a
work-stealing insert is ruled out rather than merely unimplemented.

**`elevationAt` and `contours` are not problems.** `elevationAt` at 499 ns
sustains 2 M/s, so a cross section over a 1 km alignment spends under a
millisecond in surface queries — `extractSection`'s per-sample cost is not worth
attacking. Contours cost ~45 ms fixed plus a small per-band amount: 25× the
bands (5.0 m → 0.2 m interval) costs only 1.97× the time, which is the signature
of a single pass over the triangles rather than one pass per level. That is the
right shape already. (The levels are since traced in parallel, and the fixed
part was mostly the level range of each triangle being found twice; see below.)

### Why a fixed-seed LCG rather than `<random>`

The C++ standard fixes the *distribution* interfaces but not the output of
`std::mt19937`-fed distributions across implementations, so `<random>` would
give different points on a different standard library and the timings could not
be compared with the ones in this table. The LCG constants (Press et al.,
*Numerical Recipes* 3rd ed., section 7.1) are plain arithmetic on `uint32_t` and
produce an identical sequence everywhere.

## compareSurfaces and contours on the TaskPool

Both were serial on a 16-thread machine, and both are embarrassingly parallel.
Both now take an optional `core::TaskPool*` (null: `TaskPool::shared()`), as the
rasteriser does, and both give the **same bits at any thread count** - Rule 7 -
which `TaskPool(0)` against 1, 2, 3, 7 and 15 workers asserts bit for bit in
`test_volume.cpp` and `test_contours.cpp`.

**compareSurfaces.** The existing surface is cut into blocks of a *fixed* 256
triangles. Each block clips its triangles against their design candidates and
sums its own pieces in index order (existing triangle ascending, candidates in
ascending id, fan pieces in order) into its own compensated sums; the block sums
are then combined in block order. The sequence of floating-point operations is
therefore set by the data alone. The block size must never follow the thread
count, or that would stop being true. 256 triangles are a few hundred
microseconds of clipping, so the dispatch cost is noise, and 200k triangles
still make about 800 blocks to balance. Each block's sums fill one cache line
(`alignas(64)`), because neighbouring blocks are written by different threads.

One consequence is stated rather than hidden: summing per block changes the
last bits compared with the old single running sum. Both are compensated sums of
the same terms, and every hand-worked test passed unchanged. The new order is
now fixed, and it is the same on every machine and at every thread count.

**contours.** Each triangle's level range (a `floor` and two settling loops at
each end) is found once, in parallel, into its own slot. The old code found it
twice, once per bucketing pass, and that was most of the "~45 ms fixed" above.
The CSR bucketing stays serial and in triangle order, so every bucket is
ascending, as `LevelTracer::trace` documents. Levels are then traced in parallel
into one list per level, and the lists are joined in level order. A tracer's
visited stamps only need to be unique per level within the array it uses, so
each chunk of levels has its own array and numbers its levels from 1. Chunks
are sized to about four per thread, which bounds the arrays alive at once (4
bytes per triangle each) by the thread count. The chunking cannot change a
result.

Measured with `tools/compare_benchmarks.py --alternate 3`, 9 samples per cell,
min / median in ms. The machine was on AC with the CPU at 100% from other
agents, so these are ratios under load, not quiet-machine figures.

| Benchmark | before | before, again (A/A) | after | after, serial |
| --- | --- | --- | --- | --- |
| `BM_CompareSurfaces` 200k vs 50k | 2013 / 2276 | 2092 / 2340 | **210 / 220** | 1960 / 2098 |
| `BM_Contours/50` (5 m) | 57.0 / 63.2 | 52.8 / 56.1 | **9.7 / 11.4** | 38.9 / 41.9 |
| `BM_Contours/10` (1 m) | 62.9 / 74.9 | 68.2 / 75.5 | **16.7 / 18.1** | 50.1 / 56.2 |
| `BM_Contours/2` (0.2 m) | 102 / 115 | 101 / 133 | **32.5 / 40.5** | 92.3 / 100.5 |

That is **9.6x** for the comparison, and **3.1-5.9x** for contours, of which
10-32% comes from finding the level ranges once (the serial column). The serial
comparison is the old cost within the A/A spread: the blocks cost nothing.

On real survey data - the 229 462-triangle TIN of the owner's *Test 4 with Tin*
archive, against a design re-triangulated from every third vertex raised 0.3 m
(840 819 overlay pieces) - the same code, serial against parallel, with an A/A
control:

| Benchmark | serial | serial again | parallel | parallel again |
| --- | --- | --- | --- | --- |
| `BM_ArchiveTinCompareSurfaces` | 1535 / 1695 | 1570 / 1642 | **181 / 190** | 179 / 192 |
| `BM_ArchiveTinContours/10` (1 m, 1 056 contours) | 19.5 / 23.8 | 19.6 / 20.6 | **11.6 / 13.0** | 11.3 / 12.7 |
| `BM_ArchiveTinContours/1` (0.1 m, 10 752) | 84.1 / 92.2 | 73.3 / 80.3 | **35.7 / 41.3** | 30.5 / 34.1 |

The archive benchmarks read the file named by `KATANA_BENCH_TIN_12DA` (and the
section ones `KATANA_BENCH_DRAWING_12DA`), and they skip with a message when it
is unset. Survey data does not belong in the repository.

**Not done, and why.** Contours on a real TIN scale less well (1.7-2.6x) than
the comparison, because the serial CSR bucketing and the few long levels at a
1 m interval are left: the bucket fill must stay in triangle order, and a
parallel fill needs per-chunk offsets for every level, up to 100 000 levels. At
12-41 ms it was not worth that memory. `buildTin` was not split: CGAL's
constrained Delaunay insertion is sequential, and Rule 7 requires the one
triangulation a serial build gives. Tiling the input (`TiledTerrain`) changes
the triangles along every seam, so it is a different surface, not a faster
route to the same one. The TIN build is therefore taken off the GUI thread
instead (*Background jobs*, below).

## Section crossings

`extractSection` intersected every alignment segment with every segment of
every entity in the model. Now only entities that carry a plan curve (line,
polyline, arc, circle) are considered, and three cheap tests run before any
intersection:

1. the entity's box against the alignment's;
2. the box against each alignment segment's;
3. for polyline edges, whether both ends lie on the same side of the alignment
   segment's line.

`SectionOptions::spatialIndex` (the document's `spatialIndex()`) narrows the
entities to the alignment's neighbourhood when the query is small enough to be
worth it (`detail::worthIndexing`). Every test has a margin of 10 x `kGeometric`,
because `intersect()` accepts a crossing within `kGeometric` of both curves. A
fence stopping 5e-8 short of the alignment is still found, and with the margin
set to zero that test fails (checked by doing it). Its polyline form, on a
horizontal and a diagonal alignment, fails the same way when the side test has
no margin. Entities are visited in
ascending id on both paths, so ties at one station keep their order.

| `--alternate 3`, min / median ms | old search | boxes only | boxes and side | the same, again |
| --- | --- | --- | --- | --- |
| 28k-entity survey drawing, 60 m cross section | 15.3 / 20.9 | 4.1 / 5.4 | **4.2 / 5.2** | 4.2 / 5.1 |
| the same, with the document's index | 14.7 / 21.1 | 0.003 | **0.003** | 0.003 |
| 28k-entity drawing, its full diagonal | 20.9 / 26.3 | 17.9 / 28.0 | **8.0 / 10.1** | 7.4 / 9.8 |
| 30k generated strings, 60 m cross section | 18.1 / 18.6 | 4.6 / 7.4 | **4.6 / 5.1** | 4.6 / 5.7 |
| the same, with the index | 19.3 / 22.3 | 0.02 | **0.02** | 0.02 |
| 30k generated strings, 1.4 km diagonal | 27.4 / 30.7 | 26.7 / 34.3 | **9.0 / 10.0** | 8.5 / 9.6 |

A cross section is 4x faster from the box tests alone, and about 5 000x faster
with the index. A long section (the diagonal, whose box covers the whole
drawing, so the box tests reject nothing) is 2.6-3x faster from the same-side
test. The first A/B, run before the side test existed, showed the diagonal
unchanged (30.8 before, 31.4 after, 31.7 again).

## One store of named surfaces

`terrain::SurfaceStore` (`include/katana/terrain/surface_store.hpp`) holds
the surfaces a session works with, by name: one in the window, one in each
headless session. The geoprocessing verbs' `SURFACE <name>` finds a surface
by the same name on every front end (`docs/geoprocessing.md`).

- **Names are compared case-insensitively.** A name already in use is
  refused, AlreadyExists, and `uniqueName` gives "name (2)", "name (3)"...
  The window names a second import of a surface so, as the session names a
  second alignment.
- **A surface is a `shared_ptr<const TinSurface>`,** so a background job
  reads one while the views draw it, and nothing moves it.
- **Every change bumps `revision()`.** That is how the window knows to
  rebuild what its views draw (`MainWindow::syncSceneSurfaces`) without
  being told what changed.

Surfaces are still session data, not saved in the project: saving them is a
storage-schema decision of its own.

## Surfaces on every front end

The SURFACE verb (`src/katana_app/geo/surface_verbs.cpp`) makes, lists and
writes out the store's surfaces. It runs through the one geoprocessing
executor (`docs/geoprocessing.md`), so `katana_cli`, `katana_mcp` and the
window's command line run the same code, and the window's Terrain > Surface
From and GIS > Export Surface as DEM only build its lines:

```
SURFACE LIST [JSON] | SURFACE INFO <name> | SURFACE REMOVE <name>
SURFACE FROM RASTER <id|name> | FILE <path> [max=<points>] [AREA x0,y0,x1,y1] [NAME <n>]
SURFACE FROM CLOUD <id|name> [classes=2,...] [max=<points>] [NAME <n>]
SURFACE FROM <scope> [NAME <n>]
SURFACE EXPORT <name> <file> [cell=<m>] [type=Float32|Float64] [cog] [format=<driver>]
               [co=K=V]... [OVERWRITE]
```

Every form takes PREVIEW, which says what would be read and makes nothing.
A reply is key=value records: how the source was read (`sampled stride=
pixels= nodata= points=`, `thinned from= step= points=`, the drawing's
`scope` record), `merged duplicates=` when coincident points were averaged,
then `surface name= triangles= points= bounds= zmin= zmax= source=`.
`SURFACE LIST` lists the surfaces and, as `raster id= name= kind= width=
height= cell= file=` records, the rasters a terrain verb reads as `RASTER
<id|name>`; `SURFACE LIST JSON` is what `katana_terrain_list` hands an agent
(`docs/mcp.md`).

- **A raster is read at its true values.** `readRasterElevations` reads the
  band from its file through GDAL, never the 8-bit display copy a reference
  raster holds (audit QT-23), on the stride that keeps the extent under the
  cap: 400 000 points unless `max=` says (audit QT-24). `AREA` cuts the raster
  first, with GDAL's `raster clip --bbox`, so the cap is spent on the site and
  not on the whole sheet a DEM was delivered as; a window reaching past the
  raster takes what is there.
- **A cloud gives its ground,** or every return when nothing is classified
  as ground, which the reply says with a warning (`surfacePoints`, audit
  QT-10). `classes=` chooses the ASPRS classes instead, for a person who
  knows their data. The points are thinned to the cap on a stride.
- **The drawing is taken by the shared scope** (`docs/cad.md`, "Scope and
  filter"): `SURFACE FROM DRAWING WHERE DRAWN` is what the drawing shows,
  which is what the dialog offers first; `SURFACE FROM LAYERS ground ONLY`
  one layer. As the plan wrote it, `DRAWING` may also come before a
  narrower scope (`FROM DRAWING LAYERS ground`), naming the source. The
  rules that turn entities into survey points and breaklines are
  `cad::geo::surfaceInput` (`include/katana/cad/geo/surface_input.hpp`),
  moved out of the window so every front end shares them.
- **A name is the source's,** made unique ("terrain (2)"), unless `NAME`
  gives one, which must be free: AlreadyExists otherwise.
- **The DEM is written by GDAL's `raster convert`** (`exportSurfaceRaster`):
  the surface sampled at cell centres by `surfaceGrid`, stored as Float32
  unless `type=Float64` asks, a GeoTIFF tiled and DEFLATE-compressed with the
  floating-point predictor, a Cloud Optimised GeoTIFF with `cog` through
  GDAL's COG driver, and `co=K=V` over those defaults. A file already there
  is replaced only with OVERWRITE (AlreadyExists otherwise), as TO FILE's
  rule has it.
- **`TO SURFACE <name>`** keeps a GDAL run's raster as a surface
  (`src/katana_app/geo/bindings.cpp`, T0's block): its true values on the
  same capped stride, the spilled raster removed once read.

### Decided

- **A heightless entity is left out and counted, not put on the datum.** The
  window's Surface From Drawing put an entity with no `elevation` or
  `elevations` property at z = 0 and warned only when no entity at all had
  one. A 2D drawing (the site plan sample) became a flat surface at 0, and
  one plain line among levelled strings dug a trench to the datum - the
  audit IO-01 failure again (absent is not zero). The moved rules leave it
  out, count it (`skipped.heightless`, `vertices.heightless`) and refuse a
  scope with fewer than three heighted points, with the scope record in the
  refusal. Everything else is the window's rule, which
  `SurfaceInput.OnLevelledDataTheRulesGiveExactlyWhatTheWindowGave` checks
  against a verbatim copy of the old loop, and
  `SurfaceInput.OnTheSitePlanTheHeightlessAreLeftOutWhereTheWindowPutThemOnTheDatum`
  shows the difference on the sample: 13 vertices at 0 then, none now.
- **A line is a breakline of its two ends.** The window ignored LINE
  entities; a levelled line is as much a breakline as a two-vertex polyline.
- **Float32 by default.** It is the DEM convention and half the size; near
  1000 m its step is 6e-5 m, finer than any survey the surface came from.
  `SurfaceVerbs.SurfaceExportFloat32ReadsBackTheSurfaceWithinFloat32Precision`
  bounds the error by one Float32 step (7.63e-6 near 100): half from the
  Float32 heights the fixture is read as, half from the storage.
- **GDAL writes the DEM, not `GdalDataset::writeRaster`.** Only the
  algorithm's convert reaches every driver's creation options and the COG
  driver; the old writer made Float64 with none. The window's dialog and the
  verb share the one writer.
- **One dialog for the three Surface From items.** Each opens it on its own
  source, with Reference Data's chosen cloud or raster first, so the options
  the verb has (a window, a cap, classes, a name, the drawing's scope) are
  in the window too. Its fields follow the plan's `<d>` rule
  (`surfaceFromScope`, `surfaceFromCommand` ...), where the plan's text said
  `surfaceScope`.

### Not done

- **TO SURFACE triangulates in the apply.** The GDAL verb's work is F0's and
  ends with the run, so a raster kept as a surface is triangulated when the
  job's result is applied - on the window's GUI thread, up to 1.7 s at the
  400 000-point cap. SURFACE FROM triangulates in its work.
- **A 12d archive's tins are still dropped by katana_cli's IMPORT.** The
  session has a store now, and IMPORT's move into the one GIS executor (I0)
  is where they are to be kept in it.
- **Surfaces are not saved** with the project, as above.

## Background jobs

Long computations no longer run on the GUI thread behind a wait cursor.
`katana::qt::JobRunner` (`src/katana_qt/jobs.hpp`) runs a work function on its
own `std::jthread`. The contract that keeps the single-threaded document safe:

* **Work is pure.** It computes from inputs the job owns, copied on the GUI
  thread before it starts: never the document, a widget, or reference data
  that could be removed while it runs. It returns an `Apply` step.
* **Apply runs on the GUI thread.** It is delivered through
  `QMetaObject::invokeMethod` with a lambda (no moc) and changes the document as
  one command, or hands the result to the window. Nothing a job computed is
  visible before it is applied.
* **Cancel is cooperative, and a cancelled job never applies.** Work that
  watches `JobControl::stopRequested()` ends early. Work that cannot, such as a
  CGAL triangulation, runs to its end, and its result is then discarded.
* **Failure is never lost.** An `Error` returned by the work, or anything it or
  the Apply step throws (a `std::exception` or not), arrives in the job's
  `Finished` callback as `JobOutcome::Failed` with the message.
* **Progress costs the event loop nothing.** The worker stores it in atomics,
  and a 10 ms timer, running only while there are jobs, reads them into the
  status bar (a label, a progress bar that shows *busy* until a job reports a
  fraction, and Cancel). The same timer measures the longest event-loop pause
  while jobs run.
* **Destroying the runner** stops and joins its jobs and drops completions not
  yet delivered, so an Apply may safely capture the window that owns it.
  `JobRunner::of(window)` finds or creates a window's runner by object name, so
  the window needs no member for it.

Surface From Point Cloud, Raster and Drawing are jobs: the SURFACE FROM line
the dialog builds runs as the geoprocessing workbench's job
(`src/katana_qt/geo/geo_workbench.hpp`). The cloud's ground points and the
drawing's points and breaklines are copied when the line is prepared, on the
GUI thread (the document is single-threaded). The raster is read by the job,
from its path: GDAL keeps its error handlers per thread, and the reader opens
its own dataset. A headless run waits for the job, pumping a real event loop,
so the screenshot or report that follows sees the surface.

The wait is `QEventLoop::exec()`, quit by the job's completion. The first
version called `processEvents(WaitForMoreEvents)`, and on Windows, outside
`exec()`, that slept through both the worker's post and the timer and never
returned.

Measured (Debug, offscreen, three runs; `test_jobs.cpp` records them): a
60 000-point `buildTin` as a job took 2.79-2.90 s of work, and the GUI loop's
longest pause was 10-11 ms, one timer tick. A 400 ms job paused it 11-14 ms. The
control test blocks the GUI thread for 250 ms while a job runs, and the measure
reads at least 250 ms, so a measure that always read zero would fail it. Surface
From Point Cloud on `samples/gis/survey_scan.las` (29 512 ground points)
triangulated in 1.28 s in the background, and the event loop paused at most
24 ms, the Apply step included.

On real data (Release, headless, three runs), Surface From Drawing on the
owner's *Test 4 without tin* archive (27 886 entities, giving 197 287 vertices
and 394 561 triangles) triangulated in 2.27-2.77 s on the job. Before, the GUI
thread was blocked for all of that. Adding the result took 10-12 ms of GUI time.
The longest event-loop pause during the job was 342-444 ms. That pause is the
plan view repainting the 28k-entity drawing, which the event loop is now free
to do while the build runs; it is not the job. The control: the same 0.11 s
Surface From Point Cloud job pauses the loop 20-22 ms with the 20-entity sample
drawing loaded, and 325-362 ms with this drawing loaded as well. That repaint
cost is the plan view's (`docs/performance.md`).

Also still on the GUI thread after a surface lands: the 3D view rebuilds its
scene on the next paint. That too is the render view's cost, not the build's.

Only one job at a time gets the shared `TaskPool`. A `parallelRanges` issued
while the pool is busy runs inline, so a compareSurfaces job and a 3D frame at
the same moment will make one of them serial. That is slower, never wrong, and
it cannot deadlock (`task_pool.hpp`).

## Where the compensated sum lives now

Every area and volume here goes through a Neumaier compensated sum. It was
`katana::terrain::detail::CompensatedSum`, private to this module, until the
corridor quantities in `cad` needed the same thing; a second copy would have
been the defect CLAUDE.md section 1 names, so it moved to the public
`katana::math::CompensatedSum` (`include/katana/math/summation.hpp`) and the
terrain header now aliases it - the call sites in `volume.cpp` and
`tin_surface.cpp` read as they always did, and the 17 volume and area tests
passed unchanged through the move.

It is tested directly for the first time. The property that took two tries to
pin: Neumaier's variant differs from Kahan's exactly when a term is larger
than the running sum, so the test adds 1e15 FIRST and then a million 0.1s -
each of which plain addition rounds up to 0.125, the ulp at 1e15, landing at
125 000 instead of 100 000. The first draft added the small terms first and
asserted plain addition would drift; it barely did, and the +1e15 / -1e15
round trip then rounded the drift away to exactly 100 000 by luck. That order
is the case Kahan already handles and proves nothing about Neumaier.

## Culling that has to be exact (`super_surface.hpp`, `tin_surface.hpp`)

`combineSurfaces` asks, for every triangle of a lower member, whether a higher
member covers it - and answered by scanning every vertex of that member. On a
diagonal corridor over a 200 000-point base that is thirty-one seconds. The
member's vertices now go into a bucket grid, built once per member, and only the
candidates in the triangle's own cells are tested. **125x**, with the answer
unchanged: `docs/performance.md` has the numbers and the two shapes the
benchmark takes.

Two guards keep the indexed set a strict SUPERSET of what the exhaustive scan
accepted, which is the only thing that makes this safe:

- A triangle whose doubled signed area is exactly `0.0` falls back to the
  exhaustive scan. The three sign tests sum identically to that doubled area, so
  a zero one makes the accepted set a line or the whole plane, which no box
  bounds. A TIN does not contain such a triangle; the fallback is there because
  "does not" is not "cannot".
- The query box is grown by `kGeometric + 64 * eps * scale`, to cover rounding in
  the sign tests themselves.

`TheIndexedVertexTestKeepsExactlyTheTrianglesTheExhaustiveScanKept` asserts the
two agree. Its first version did **not** discriminate - coarse pads gave the grid
cells so much slack that a wrong box still passed - so it now carries a
knife-edge case whose deciding vertices sit exactly on a base triangle's box
edge. Shrinking the query box by 2 m fails it; that was checked by doing it.

`elevationsAt` spreads over `TaskPool` at a grain of 256 positions, a number
taken from the measured 1.09 us cost of one `elevationAt` - below one chunk it
runs inline rather than waking the pool, because waking it costs more than the
work. `TiledTerrain::buildAll` puts one tile in each chunk. Both are asserted to
give bit-identical answers to the serial path, and `buildAll` now builds every
tile even when one fails, returning the lowest-index failure; that is a
behaviour change and it is stated in the header.

## Hoisting a triangle's doubled area

`Triangle2::twiceSignedArea()` is exposed so that a caller evaluating many
points against one triangle can hoist it. `barycentric(p)` is now literally
`barycentric(p, twiceSignedArea())` behind an `isDegenerate()` check, so the
arithmetic is bit-identical and lives in one place rather than two.

## SIMD: tried, measured, left scalar

The terrain module has no AVX2 kernels. Two were written, both bit-identical to
their scalar references at every thread count and on both levels
(`simd_scalar.terrain` and `simd_avx2.terrain` passed), and neither paid when
measured against a same-binary control. The rule in docs/performance.md is that
a kernel that does not pay stays scalar, so both were removed. The code is in
the history of branch `simd-terrain` (commit `25c6010`) if the question comes up
again.

- **Contour level ranges** (`contours()`, the `floor` and settling loops of
  `firstLevelAbove`, four triangles per step). No change outside the A/A
  spread, on generated ground or on the owner's TIN, at a coarse or a fine
  interval. The phase is limited by loading each corner's elevation through the
  triangle's vertex indices. The kernel has to gather those loads, and on this
  processor gathers are no faster than the scalar loads they replace.
- **locate()'s containment test** (the three edge functions of up to ~8
  candidates a cell, four at a time). This was *slower*: a cell holds too few
  candidates to cover the gathers, and the first candidate usually already
  contains the point.

The rest was not attempted. The interpolation in `elevationsAt` is five
operations a query behind a `locate()` that costs about 200 ns. In the volume
pair clip, every lane's polygon grows and shrinks differently, and the
compensated sums have to be added in piece order.

Measured on an i7-1270P under heavy load from other agents' builds, with the
same binary at both levels (`KATANA_SIMD=scalar|avx2`) and a byte-identical copy
of it as the A/A control. The four arms ran in alternating rounds, reversing
the order every round, as `tools/compare_benchmarks.py --alternate` does. Each
cell is min / median real time.

| benchmark | scalar | scalar (A/A) | avx2 | avx2 (A/A) |
| --- | --- | --- | --- | --- |
| `BM_ElevationAt`, ns (30 samples) | 172 / 217 | 154 / 207 | 178 / 240 | 159 / 233 |
| `BM_ElevationsAt`, 100k probes, ms | 3.58 / 4.11 | 3.33 / 4.06 | 3.62 / 4.22 | 3.69 / 4.03 |
| `BM_ContoursSerial/50`, ms (24 samples) | 17.1 / 22.6 | 18.3 / 21.6 | 17.0 / 22.7 | 17.2 / 22.0 |
| `BM_ContoursSerial/10` | 21.2 / 28.9 | 22.7 / 27.0 | 22.3 / 29.4 | 21.8 / 28.7 |
| `BM_ContoursSerial/2` | 40.2 / 55.5 | 45.4 / 56.9 | 44.3 / 62.2 | 37.1 / 49.6 |
| `BM_ArchiveTinContoursSerial/10` | 10.8 / 14.6 | 12.7 / 14.8 | 8.9 / 14.2 | 9.1 / 11.7 |
| `BM_ArchiveTinContoursSerial/1` | 42.9 / 60.8 | 48.2 / 60.9 | 37.5 / 52.5 | 42.9 / 52.7 |

The contour rows come from the fourth of four runs, at above-normal priority.
Its 14% on `ArchiveTinContoursSerial/1` did not come back in the other three
runs: in those, the two levels overlapped within their A/A spreads. It is also
larger than the whole level-range phase could be at a 0.1 m interval, so it is
noise. The
parallel `BM_Contours` and `BM_ArchiveTinContours` moved by no more than their
A/A spreads in any run.
