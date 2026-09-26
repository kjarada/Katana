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

- **One surface record.** IMPORT of an archive lists its surfaces with the
  same `surface` record as SURFACE LIST, written once in `surface_verbs.cpp`,
  with heights and bounds to the millimetre. The IMPORT lane and the terrain
  lane each wrote one, one to three decimals and one exact. Once merged they
  were two definitions of one function, and every program that linked
  `katana_app` failed. The exact one was dropped: on a text grid, which GDAL
  reads as Float32, it writes the height 24.892 as `24.892000198364258`,
  noise from the storage type and not a measurement. The same merge left two
  `rasterInfoOf`; the one kept is `terrain_steps.cpp`'s, which also carries a
  grid's no-data value.

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
- **A .12da archive's tins are still dropped by katana_cli's IMPORT.** The
  session has a store now, and IMPORT's move into the one GIS executor (I0)
  is where they are to be kept in it.
- **Surfaces are not saved** with the project, as above.

## Contours

The CONTOUR verb (`src/katana_app/geo/contour_verbs.cpp`) draws contour
lines of a surface or an elevation raster. It runs through the one
geoprocessing executor, so `katana_cli`, `katana_mcp` (through
`katana_run_commands`) and the window's command line run the same code, and
the window's Terrain > Analysis > Contours (`contoursDialog`) only builds its
line:

```
CONTOUR SURFACE <name> | RASTER <id|name> | FILE <path> interval=<m> [major=5]
        [base=0] [layer=terrain/contours] [smooth=3|5] [<scope>] [PREVIEW]
```

The reply is the source's `input` record, the scope's (`scope arg=boundary
...`, then `areas used= skipped.open= skipped.points=`), then `contours
method=tin|grid cell= levels= count= major= minor= layer= smoothed=` and
`output ... created=`.

- **One verb, two engines, one output.** A SURFACE is traced exactly on its
  triangles by `terrain::contours` - the tracer above, finished and
  measured, and until now never offered to anyone. A RASTER or FILE is
  contoured by GDAL's `raster contour` at its full resolution, always with
  `--elevation-name` (GDAL writes no level without it, only an ID) and
  `--3d`. GDAL's lines are turned into the tracer's `terrain::Contour`
  (`interop::geo::contoursFromFeatures`), so which contour is major, which
  layer it goes to and what height it carries are decided in one place,
  `interop::geo::contourCommand`: polylines on `<layer>/major` and
  `<layer>/minor`, a ring closed, every vertex at the level
  (`entity::setHeights`, so the `elevation` property), in one undo step.
- **Major by the tracer's rule.** Levels are `base + k * interval`; a level
  is major when k is a multiple of `major` (0: none). GDAL's `--offset` is
  the base.
- **A scope, last on the line, gives boundaries.** Its closed shapes -
  closed polylines and circles, a tagged hole joining its area, read by the
  one drawing conversion (`drawingDataset`) - are the areas the contours are
  kept inside; both engines' lines are cut at the boundary itself
  (`interop::geo::clipContours`: split where a line crosses a ring, the
  piece kept when its middle is inside an area and in none of its holes, on
  a ring counting as inside; a cut ring's piece through its first vertex is
  joined across it). A raster is first cut by GDAL's `raster clip --bbox`
  to the areas' box, a cell wider (and the smoothing kernel's half-width
  more), within the raster, so only the site is contoured and the cut lines
  still reach the boundary. Open lines and points bound nothing: counted
  (`skipped.open`, `skipped.points`) and warned about; a scope with no
  closed shape draws nothing and says so, as a scope that takes nothing is
  not an error.
- **`smooth=3|5`** runs GDAL's gaussian `raster neighbors` over a raster
  first, for presentation, and the reply says `smoothed=yes`. The sizes are
  GDAL's: its gaussian kernel has no other. A gaussian is symmetric and sums
  to one, so it leaves a plane's contours where they were away from the
  edges (`ContourVerb.SmoothingAPlaneLeavesItsContoursWhereTheyWere`).
- **Too many levels is refused before anything is drawn.** The tracer
  refuses more than 100 000 levels (a unit mistake, not a wish); a raster's
  levels are counted over its values read on a stride of about a million
  cells first, and refused the same way. A sample's range is within the
  whole's, so a count that is already too many is certainly too many.

### Decided

- **Both engines cut at the boundary, not GDAL's cut by geometry.** GDAL's
  `raster clip --geometry` makes the cells outside no-data, so a raster's
  contours would stop half a cell or more inside the boundary while a
  surface's reached it. One clipper for both gives one answer; the raster is
  still cut first, to a box, for speed.
- **A raster's box stays within the raster.** Cut with
  `--allow-bbox-outside-source`, GDAL fills the part of the box past the
  raster with 0 when the raster has no no-data value, and the contours drew
  a cliff from 0 to the ground at the raster's edge (found by hand: 202
  levels on the plane fixture instead of 3).
- **The dialog has a `contourClip` box** beside the plan's fields: the
  shared scope controls always name a scope, and "no boundary" is none of
  them.
- **GDAL carries a raster's contours to its edge, a surface's stop at its
  last vertex.** On the plane fixture, a raster contour runs y = 0 to 30,
  the one on the surface made from it y = 0.5 to 29.5 (the cell centres).
  Both are what the engines are; the reply's `method` says which ran.

Tolerances in `test_contour_verb.cpp` come from the fixture: `plane.asc` is
read as Float32, each height within 3.815e-6 of its value near 100, which a
gradient of 0.05 turns into at most 7.63e-5 m along x; on a surface of exact
heights the tracer is within 1e-9.

### Not done

- **No CRS check between the raster and the project.** A raster in another
  coordinate system draws its contours where its coordinates say; judging
  equivalence needs `OGRSpatialReference::IsSame`, which the verbs cannot
  see (D1/I1's reference-layer facts are where it belongs).
- **Cancel is checked around GDAL's steps and after the tracer,** which has
  no stop token of its own; the tracer runs in tens of milliseconds on real
  TINs (above).

## Shading

The RASTER SHADE verb (`src/katana_app/geo/shade_verbs.cpp`) draws an
elevation source as a picture and keeps it as a derived reference raster,
which the plan view and the sheet painter draw as they draw any raster.
The window's Terrain > Analysis > Terrain Shading (`terrainShadingDialog`)
only builds its line:

```
RASTER SHADE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
             [style=hillshade|relief|relief+hillshade|slope|plain] [azimuth=315]
             [altitude=45] [z=1] [variant=regular|combined|multidirectional|igor]
             [ramp=terrain|diverging|slope|grey|<file>] [range=<min>,<max>]
             [NAME <n>] [save=<file.tif>] [OVERWRITE] [PREVIEW]
```

The reply: the `input` record, `resampled from= to=` when the source was
averaged down, `shade style=` with the light (`azimuth= altitude= z=
variant=`) or the unit, `ramp name= min= max= from=data|range`, the `output`
record of the reference raster, `saved file=` with `save=`, then one
`legend value= r= g= b=` per colour the picture was painted with.

- **GDAL's algorithms make the picture,** step by step through a
  `RasterChain` (`terrain_steps.cpp`): `raster hillshade`; `raster
  color-map` with the ramp spread over the range, written as GDAL's
  colour-map text; `raster blend --operator hsv-value` for relief over
  hillshade (the relief's hue and saturation, the hillshade's value); and
  `raster slope`, in degrees, for slope shading. Each step's raster goes to
  a file of the chain's own, removed with it, so a failed or cancelled run
  leaves nothing and no DEM is held whole in memory.
- **Every style ends as an RGBA picture.** A hillshade is passed through a
  colour map of its own greys (1 to 255 to themselves, its no-data 0
  transparent), because the raster reader stretches a single grey band over
  its own range: flat ground, 181 everywhere, would have been drawn
  mid-grey. So what is drawn is what GDAL computed
  (`ShadeVerb.FlatGroundHillshadesTo181EverywhereAtAltitude45`: 1 + 254 sin
  45 degrees = 180.6).
- **At display resolution.** A raster longer than the 4096 pixels a
  reference raster's display copy keeps is averaged down to it first
  (`raster resize --resampling average`, by the smallest whole step that
  fits), since a finer picture would only be decimated again to be drawn.
  A surface is sampled at its CELL, or the cell suggested for its extent.
- **Ramps** (`include/katana/interop/geo/colour_ramps.hpp`): terrain
  (hypsometric tints), diverging (blue, white, red: on design minus
  existing, cut is blue and fill red), slope (green, pale yellow, red) and
  grey, the ColorBrewer colours named there. Each style has its own - relief
  terrain, slope slope, plain grey - and `ramp=` chooses another, or a GDAL
  colour-map file of a person's own, whose value lines (percentages placed
  on the range as GDAL places them) are then the legend. The ramp spans the
  data's least and greatest values, read in full at display resolution,
  unless `range=` says; the diverging ramp's span is made symmetric about
  zero, so zero is its white. A span of one value is widened a unit about
  it. The ends are written exactly, so the cells at the range's ends take
  the ramp's end colours exactly
  (`ShadeVerb.ARampMapsItsEndpointsExactly`).
- **The reference raster** is named `NAME`, or the source's name and the
  style's (`terrain-relief-hillshade`), made unique; its role is Derived,
  its derivation the line, and its display style the style.
- **`save=`** also writes the picture as rendered - a tiled, DEFLATE
  GeoTIFF with the source's georeferencing - for delivery; a file already
  there only with OVERWRITE.

### Decided

- **Rendered once, into the picture, not at draw time.** The plan said a
  display style of an elevation raster; drawing it at draw time would put
  GDAL in the painter. A derived picture is drawn by the code that draws
  every raster, plots with the sheet, and can be saved; `displayStyle` says
  which picture it is. Restyling is running the verb again.
- **The light is refused where there is none.** `azimuth=`, `altitude=`,
  `z=` and `variant=` belong to the two hillshade styles; `ramp=` and
  `range=` to the coloured ones. A line that gives them elsewhere is
  refused, naming them, rather than quietly ignored.
- **Slope shading is in degrees:** bounded (0 to 90) and what a legend is
  read in; RASTER SLOPE (below) takes percent too.

### Not done

- **Restyling a raster already there.** A shading is a new reference raster;
  the Reference Data dock's display menu the plan sketched is D2's.
- **A legend on the sheet.** The legend is in the reply (and the dialog's);
  placing it on a sheet is the plotting legend's work.

## Slope and aspect

RASTER SLOPE and RASTER ASPECT (`src/katana_app/geo/slope_verbs.cpp`) keep
the slope or the aspect of a surface or an elevation raster as a derived
reference raster, and RASTER SLOPE draws slope classes as areas. The
window's Terrain > Analysis > Slope and Aspect (`slopeAnalysisDialog`) only
builds their lines:

```
RASTER SLOPE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
             [unit=percent|degree] [classes=<b1>,<b2>,...] [areas=terrain/slope]
             [min_area=<m2>] [NAME <n>] [<scope>] [PREVIEW]
RASTER ASPECT <source> [NAME <n>] [<scope>] [PREVIEW]
```

The reply: the `input` record, the scope's (`scope arg=area ...`, `areas
used= skipped.open= skipped.points=`), `slope unit= method=horn raster=WxH
cell=` (or `aspect convention=azimuth flat=nodata ...`), `sieved min_area=
cells=` with `min_area=`, the reference raster's `output` record, then with
classes `output arg=areas ... created=`, one `class name= from= to= unit=
area= polygons=` per class and a `legend value= r= g= b=` per class colour.

- **GDAL computes them:** `raster slope` and `raster aspect`, Horn's 3 x 3
  window, the edges interpolated by GDAL's own edge rule (a corner of a
  plane reads half its gradient: 2.5 % on the 5 % fixture). Aspect is
  degrees clockwise from north; flat ground has none (no-data).
- **The raster kept is the values,** at full precision, so another verb reads
  it as `RASTER <id>` (zonal statistics of slope, contours of it); its
  display copy is replaced by a coloured picture of the same grid - by class
  when there are classes, else the slope ramp over the slope's range, and
  aspect's eight compass colours round the circle, north at both ends - so
  it is not drawn as a grey stretch of its values.
- **Percent by default,** as grades are read in civil design (a 1:4 batter
  is 25 %); `unit=degree` for degrees.
- **Classes.** `classes=5,10,25` makes [0,5), [5,10), [10,25) and 25 and up,
  named `0-5`, `5-10`, `10-25`, `25+`. GDAL's `raster reclassify` sorts the
  cells (to Int16: its UInt8 cannot hold the -9999 no-data), `raster sieve`
  merges regions under `min_area` - converted to whole cells, rounded up -
  into their largest neighbour, and `raster polygonize` draws each region.
  Each is a closed polyline on `<areas>/<class>` (holes tagged, as every
  result's are), with `slope_class`, `slope_from`, `slope_to` (none on the
  open top class: absent is not zero) and `slope_unit`, through F0's
  `resultCommand`: one undo step. The class record's `area` sums the
  regions' areas, holes taken out.
- **A scope keeps the analysis inside its closed shapes.** The raster is cut
  to their box, a cell wider and within the raster, so the slope at the
  boundary is Horn's on real ground and not an edge rule; the slope is then
  cut to the shapes by GDAL's `raster clip --geometry`, which keeps every
  cell a shape touches; and the class areas, whole cells, are cut to the
  shapes exactly by `vector clip`, so each class's area is its area inside
  them and the classes add up to the shapes
  (`SlopeVerbs.AScopeKeepsTheAnalysisInsideItsClosedShapes`: 52.531 m2, the
  triangle's, where the touched cells are 66). A scope with no closed shape
  makes nothing and says so.

### Decided

- **The first class begins at 0, not -inf.** No slope is below 0, and a
  class bounded by -inf took in the -9999 no-data GDAL gives a cell it
  cannot compute (the investigators' finding); 0 needs no pass over the data
  to find its least value. The top class is open (`inf]`) for the same
  reason: no pass over the data to find its greatest.
- **Values kept, picture drawn.** RASTER SHADE's slope style keeps a picture;
  RASTER SLOPE keeps the numbers, which analysis needs, and draws a picture
  of them.
- **The class areas are cut exactly, the picture is not.** A picture of
  cells is cells; an area report and the drawn areas are measured, and a
  staircase along a lot boundary would overstate every class there.

### Not done

- **A CRS check** between the raster and the drawing's shapes, as for
  CONTOUR.
- **Aspect classes** (north-facing, ...): the aspect raster is there for
  them; a class grammar for directions is not.

## Statistics by area

RASTER ZONAL (`src/katana_app/geo/zonal_verbs.cpp`) measures a surface or an
elevation raster inside each closed shape a scope takes and writes the
results on the shapes themselves. The window's Terrain > Analysis >
Statistics by Area (`zonalStatsDialog`) only builds its line:

```
RASTER ZONAL SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
             [<scope>] [stats=mean,min,max,count,sum] [prefix=zone]
             [pixels=fractional|centre|all-touched] [csv=<file>] [OVERWRITE] [PREVIEW]
```

The reply: `gis op=zonal`, the `input` record, `scope arg=zones ...`, `zones
used= skipped.open= skipped.points=`, the in-place `output ... updated=`
record, one `zone entity=<id> <stat>=<value> ...` per zone (a statistic
with no number has no field), and `zonal stats= prefix= pixels= zones=`.

- **GDAL computes them:** `raster zonal-stats`, exactextract's method. The
  zones are handed over as one table carrying `katana_id` only, which is how
  each result finds its entity again.
- **Fractional by default.** A cell counts by the part of it the shape
  covers, so `count` is an area in cells: a 40 x 30 m lot on 1.5 m cells
  counts 1200 / 2.25 = 533.333 wherever it lies, and a mean is weighted by
  those parts (on a plane it is the plane at the shape's centroid).
  `centre` takes a cell whose centre is inside (GDAL's `default`),
  `all-touched` every cell the shape touches.
- **Only closed shapes are zones.** Closed polylines and circles, a tagged
  hole joined to its area (the one drawing-to-features conversion). Open
  lines and points bound no area: GDAL logs "Non-polygonal geometry" for one
  and the other zones came back with zero counts (the investigators'
  finding), so they are left out before GDAL sees them, counted and warned
  of.
- **Written in place, one undo step.** `<prefix>_<stat>` properties
  (`zone_mean`, `zone_count` ...) through the one result writer
  (`ResultMode::SetProperties`). Before it writes, the apply compares the
  zones with the copies taken when the line was prepared: a zone edited
  while the job ran is refused ("the drawing changed while the job ran"),
  never written over.
- **Absent is not zero.** A zone off the raster has a count of 0 and no
  mean; the non-finite number GDAL gives becomes no property, and a
  `<prefix>_<stat>` an earlier run left on the zone is removed in the same
  step - it was the number of somewhere else.
- `csv=` writes the zone rows as well, and replaces a file only with
  `OVERWRITE`.

### Decided

- **The statistics are the single-number ones.** GDAL's list-valued
  statistics (`values`, `frac`, `unique`, `coverage`) and the weighted ones
  (they need a second raster) have no single property to become.
- **Properties, not a new layer.** The lot is what the question is about;
  a copy of it carrying the numbers would go stale when the lot is edited,
  and a property is what a label, a WHERE filter and a report already read.
- **`centre` rather than GDAL's `default`**, so the word says which cells.

### Not done

- **No CRS check** between the raster and the drawing's shapes, as for
  CONTOUR and RASTER SLOPE.

## Sampling and drape

RASTER SAMPLE reports the height of the ground at points, and DRAPE gives
it to the drawing's points and vertices (`src/katana_app/geo/drape_verbs.cpp`).
The window's Terrain > Analysis > Drape and Sample Heights (`drapeDialog`,
tabs Drape and Sample) only builds their lines:

```
RASTER SAMPLE SURFACE <name> | RASTER <id|name> | FILE <path> [AT x,y]...
              [<scope>] [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]
DRAPE SURFACE <name> | RASTER <id|name> | FILE <path> [<scope>]
      [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]
```

- **The ground is read at full precision.** A surface on its own triangles
  (`TinSurface::elevationAt`), exactly - never through a grid of it; a
  raster's file through GDAL's own interpolation
  (`GDALRasterInterpolateAtPoint`, `include/katana/gis/raster_sampling.hpp`),
  bilinear unless `method=` says. Bilinear on a plane is the plane, which
  is what `DrapeAndSample.SampleOnAPlaneIsExactWithBilinear` holds it to.
  `method=` with a surface is refused, as is `CELL`.
- **Off the ground there is no height.** A point off the raster, or one
  whose interpolation window touches a no-data cell, has none: a sample
  says `ground=no` (no `z`), and a drape leaves the vertex heightless and
  counts it (`off=`, `entities_off=`).
- **RASTER SAMPLE** reads the points given `AT`, and the point entities the
  scope takes (the selection when there is neither), one `sample [entity=]
  at=x,y z=` record each, then `samples method= count= on= off=`. It
  changes nothing.
- **DRAPE** sets the heights of the points, lines and polylines the scope
  takes: a point at its position, a line at its ends, a polyline at every
  vertex. What has no vertices a height belongs to (an arc, a circle, a
  text) is left and counted by type. The heights are computed on the job's
  worker (`cad::geo::drapeHeights`, `include/katana/cad/geo/drape.hpp`,
  given the ground as a callback so katana_cad never sees GDAL) and written
  on the GUI thread by one command (`cad::geo::drapeCommand`, through
  `entity::setHeights`): one undo step, after the entities are compared with
  their copies from prepare. A second drape on the same ground changes
  nothing and pushes no step.
- **A picked point is a typed point.** The dialog's Pick
  (`GeoServices::pickPoint`, `src/katana_qt/geo/point_pick.hpp`) takes the
  next left click in a plan view and writes it into the line as `AT x,y`.

### Decided

- **A vertex off the ground loses its old height.** The drape defines the
  heights of what it takes; a height kept from before would be another
  surface's, mixed in silently. Rejected: keeping it, which draws a string
  half on the ground and half at heights no one can trace.
- **GDAL interpolates, not Katana.** The raster is GDAL's to read; a second
  bilinear of Katana's would be a second definition of "the height between
  cells" to keep in step with the rest of the GDAL verbs.
- **The pick is an event filter, not a catalogue tool.** A tool runs in one
  view's tool host and ends in a command; a pick is any view's next click
  and changes nothing.

### Not done

- **The pick does not snap.** It takes the click where it is; snapping is
  the running tool's, and a pick is no tool.

## Viewshed and line of sight

RASTER VIEWSHED says what can be seen from observers over a surface or an
elevation raster; LOS says whether one point can be seen from another
(`src/katana_app/geo/viewshed_verbs.cpp`). The window's Terrain > Analysis >
Viewshed and Line of Sight (`viewshedDialog`, tabs Viewshed and Line of
Sight) only builds their lines:

```
RASTER VIEWSHED SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
                (OBSERVER x,y)... | OBSERVERS [<scope>] [height=1.7] [target=0]
                [max=<m>] [curvature=<k>|none] [areas=<layer>] [NAME <n>] [PREVIEW]
LOS SURFACE <name> | RASTER <id|name> | FILE <path> OBSERVER x,y TARGET x,y
    [height=1.7] [target=0] [curvature=<k>|none] [step=<m>]
    [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]
```

- **GDAL computes a viewshed,** `raster viewshed` from one `--position`.
  Several observers - typed, picked, or the point entities a scope takes
  after `OBSERVERS` (at most 100: each is a whole run) - are run one at a
  time and unioned here, cell by cell on the raster's grid: GDAL's
  cumulative mode refuses a position (the investigators' finding), and a
  count of observers is not the union a person asks for.
- **The options.** `height=` is the eye above the ground (1.7, a person
  standing), `target=` how high above the ground a cell counts as seen,
  `max=` how far to look (GDAL then also cuts its grid to that reach), and
  `curvature=` GDAL's curvature-and-refraction coefficient, 0.85714 unless
  given (`none` is 0). Katana passes every one of them explicitly, so a
  changed default in GDAL cannot change a result.
- **What is kept.** A derived reference raster holding 1 where some
  observer sees the cell and 0 elsewhere, named `<source>-viewshed` unless
  `NAME` says; its display copy is tinted orange where seen and clear
  elsewhere. `areas=<layer>` draws the seen regions as closed polylines
  (GDAL's `raster polygonize`), one undo step. The reply has an `observer
  at= [entity=] visible_cells=` per observer and `viewshed observers=
  height= target= max= curvature= visible_cells= area=`.
- **A sight line is native** (`include/katana/terrain/line_of_sight.hpp`):
  no algorithm of GDAL's answers one line with its clearance. It walks from
  the eye to the aim at stations `step=` apart - half a raster cell, or
  every 10 cm on a surface, which is read exactly - and reports `sight
  visible= distance= observer_z= target_z= clearance= clearance_at=
  blocked_at= blocked_distance= blocked_ground= stations= unknown=`. A
  grazing sight (clearance exactly 0) sees; a station off the ground hides
  nothing and is counted as unknown, never read as ground at 0. Curvature
  lowers the ground by the coefficient x d^2 / 12 741 994 m, GDAL's rule
  and sphere, so a sight line and a viewshed of the same ground agree.
- **The hand-worked tests switch curvature off** (`curvature=none`): the
  shadow behind a wall is then similar triangles exactly. A 1 m wall 20 m
  from a 1.7 m eye hides the flat ground from 21 to 48 m out and not from
  49 m, since the grazing sight meets the ground at 20 x 1.7 / 0.7 =
  48.571 m.

### Decided

- **The observers' union is Katana's, on the grid.** Each run's grid is a
  window of the input's, so the union is a lookup, not a resampling.
- **The kept raster is the answer, 1 and 0,** not GDAL's 255 and 0: it is
  what another verb reads (RASTER ZONAL of it counts the seen cells of a
  lot), and its picture is a separate tinted copy.
- **Rejected: the fixture of the plan** (a 5 m wall, the observer at 1.7 m,
  hidden from 20 x 5 / (5 - 1.7)). An eye below the wall top sees nothing
  behind it, so no shadow ends; the fixture here puts the eye above a 1 m
  wall, where the shadow's far edge is the similar-triangles value.
- **The sight line's least clearance is between the ends,** not at them:
  at the observer it is the eye height and at the target the target height
  by definition, and a target on the ground would make every answer 0.

### Not done

- **Observer heights from the entities.** `height=` is one height above
  the ground for every observer; a mast of its own height per point is not
  read.
- **The pick does not snap** (as for the drape).

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

## DEMs from GDAL

DEMs are made and worked on through GDAL's algorithms, on the one
geoprocessing executor (`docs/geoprocessing.md`): `RASTER GRID` makes one from
surveyed points, and the DEM tools mosaic, clip, fill, trace, reproject and
difference them. The results are reference rasters (derived, with the line
that made them as their derivation), files, or closed polylines on a layer. The DEM verbs share `src/katana_app/geo/dem_support.hpp`:
a curated verb's own `key=value` options read wherever they stand, band 1 read
at full precision, a cell's area, and a raster written where a line said kept
in place.

### Gridding points to a DEM

```
RASTER GRID [<scope>] [method=linear|invdist|invdistnn|nearest|average|...]
            [cell=<m> | size=<columns>x<rows>] [z=geometry|<property>]
            [extent=x0,y0,x1,y1] [power=<p>] [radius=<m>] [NAME <name>]
            [TO REFERENCE [<name>] | TO FILE <path> [FORMAT <driver>] | TO SURFACE <name>]
            [OVERWRITE] [PREVIEW]
```

`src/katana_app/geo/grid_verbs.cpp`. The points the scope takes, and the
vertices of its lines and areas, are gridded by GDAL's `vector grid <method>`.
The scope is the shared one (`SELECTION | DRAWING | VIEW | AREA | LAYERS`,
then `WHERE`), so the points of one layer, of one code or of what a view shows
are gridded as MODIFY would take them. Without TO the DEM is a reference
raster named `dem`, or NAME's name.

- **The methods are GDAL's own**, the leaves of `vector grid` read from its
  catalogue at run time: a method a GDAL upgrade adds is offered with no
  change. `power=` and `radius=` are refused for a method that has no such
  argument, naming the methods that do.
- **Heights: absent is not zero.** With `z=geometry` (the default) a vertex's
  height is the drawing's, and an entity without a height at every vertex is
  left out and counted (`skipped.heightless`). GDAL reads a 2D point as
  z = 0: measured, four points at 100 m and one without a height between them
  made the middle 0. With `z=<property>`, the property is GDAL's `--zfield`,
  and an entity without a number there is left out and counted
  (`skipped.no_z`): GDAL skips a null field but reads a word as 0 (measured,
  "x" on the middle point made it 0).
- **Every cell is the size asked for.** GDAL takes a resolution only with an
  extent, and fits the extent by stretching the cells: measured, 10 m at a
  3 m cell came back as 3 cells of 3.333 m. So the extent defaults to the
  bounds of the vertices used, grown outwards to whole cells from the origin:
  every point is inside the grid, every cell is `cell=` square, and grids of
  one cell size line up. A given `extent=` keeps its lower-left corner and
  grows up and right to whole cells. `size=` takes the extent as it is.
  Without either, the cell is `interop::suggestedCellSize` of the extent, as
  a surface's raster export chooses it.
- **Cells no point reaches hold no data**, `interop::geo::kGridNoData`
  declared on the band. GDAL's default no-data value is 0, which a reader
  takes for ground at the datum.
- **Bounded.** A grid of more than 25 million cells (the surfaces' own cap)
  is refused, naming the cell that would fit.
- **The options are the verb's wherever they stand.** `cell=5` after a
  `WHERE LAYER=spots` filter is the verb's, not a condition: none of the
  verb's keys is a WHERE key (`splitOptions`).
- **What the scope took is said**, in the scope record every geoprocessing
  verb gives; a scope that takes nothing, or only heightless points, answers
  `ran=no` and nothing runs.
- **TO SURFACE** is the terrain session's: until its store takes a raster
  result, the executor refuses it after the run, and the dialog does not
  offer it.

The reply:

```
grid method=linear algorithm="vector grid linear" z=geometry cell=5 extent=0,0,100,100 size=20x20 seconds=0.044
input arg=input source=drawing
scope arg=input scope=drawing matched=121 used=121 points=121 lines=0 polygons=0
output arg=output kind=raster target=reference id=1 name=ground raster=20x20 file="..." persisted=no
```

**The window.** Terrain > DEM > Grid Points to DEM (`terrainGrid`) opens
`gridDemDialog` (`src/katana_qt/geo/grid_dem_dialog.hpp`): the points' scope
and filter (Global Modify's controls), the method from GDAL's catalogue, cell
or size, where the heights come from (geometry, or a property the drawing
holds numbers in), extent, power and radius when the method takes them, and
the name. It builds the line (`gridDemCommandLine`, a pure function), shows
it in `gridCommand`, and Run hands it to the window's one executor, where it
runs as a background job with progress and Cancel; the reply comes back into
`gridReply` when the job ends. What the dialogs share - the command, preview,
run and reply panel, and how a dialog hears that its job ended - is
`src/katana_qt/geo/geo_dialog_support.hpp`.

**Tests.** `tests/geo/test_grid_verb.cpp` (the executor, a Session and the
MCP server's `katana_run_commands`), `tests/qt_widgets/geo/test_grid_dem_dialog.cpp`,
`cli.raster_grid_*` (`src/katana_app/geo/cli/grid.cmake`) and
`qt_grid_points_to_dem_dialog_grids_the_drawing_headless`
(`tests/geo/headless/grid.cmake`). Every expected value is worked from the
plane the points are surveyed on, z = 100 + x/10 + y/20: a linear grid
reproduces a plane exactly, so every cell centre holds the plane's value -
at (52.5, 47.5), 100 + 5.25 + 2.375 = 107.625 - to Float64 rounding (the
output is Float64; 1e-9). Inverse distance with a radius of 8 m on a 10 m
lattice takes the four points round each 10 m cell's centre, 7.07 m away,
equally weighted, and the mean of a plane at four symmetric corners is the
plane at the centre: 107.75 at (55, 45).

### The DEM tools

```
RASTER MOSAIC <raster> [<raster>...] [resolution=same|highest|lowest|average|<x>,<y>] [SAVE <file>]
RASTER CLIP <raster> AREA x0,y0,x1,y1 | <scope>
RASTER FILL <raster> [distance=<cells>] [smoothing=<n>] [strategy=invdist|nearest]
RASTER FOOTPRINT <raster> [TO LAYER <path> | TO FILE <path>]
RASTER REPROJECT <raster> [crs=<crs> | like=<raster>] [from=<crs>] [resampling=<method>] [cell=<m>]
RASTER DIFFERENCE <raster> <raster> [<scope>] [resampling=<method>]
  and on each: [NAME <name>] [TO REFERENCE [<name>] | TO FILE <path> [FORMAT <driver>]]
               [OVERWRITE] [PREVIEW]

<raster> := RASTER <id|name> | SURFACE <name> [CELL <m>] | FILE <path>
```

`src/katana_app/geo/dem_verbs.cpp`, on GDAL's `raster mosaic`, `clip`,
`fill-nodata`, `footprint`, `reproject` and `calc`. A raster result is a
derived reference raster (`mosaic`, `clip`, `filled`, `reprojected`,
`difference`, or NAME's name); the footprint is closed polylines on
`gis/footprint`, one undo step.

- **MOSAIC is a VRT.** The tiles are read where they are, so a mosaic of a
  hundred tiles costs a small file, kept in the derived folder with the
  tiles' absolute paths. `SAVE <file>` writes the mosaic out as a file of its
  own (GeoTIFF for `.tif`) and keeps that file as the reference raster. A
  keyword rather than `save=`: an option's value is one unquoted word, and a
  path may hold blanks. A `FILE` source may be a folder - every file in it
  GDAL reads as a raster, so a `.prj` or a note is no tile - or a pattern
  (`tiles/*.tif`); either is expanded on the worker, in name order, so the
  mosaic is the same whatever order the file system lists them. A surface is
  no tile and is refused. Tiles whose bands differ in colour interpretation
  are refused in GDAL's words ("heterogeneous band color interpretation").
- **CLIP's AREA is the box itself.** Any other scope clips to the closed
  boundaries it takes (GDAL's cutline: a cell stays when its centre is
  inside); points and open lines bound nothing, and a scope with no closed
  boundary answers `ran=no`. `AREA ... WHERE` is refused: a box has no
  boundaries to filter. The boundaries carry the project's coordinate system
  and GDAL brings them into the raster's; when either has none they are taken
  to agree.
- **FILL counts.** The reply says how many cells were empty, how many were
  filled and how many are left; a raster with none empty answers `ran=no` and
  makes nothing.
- **REPROJECT refuses a raster that says nothing of where it is.** GDAL
  reprojects it from nowhere without a word (measured: the plane with no CRS
  "reprojected" to EPSG:28356 came back unchanged). `from=<crs>` says where
  it is. With no `crs=` the target is the project's coordinate system, and a
  drawing with none refuses, naming `crs=` and `like=`. `like=<raster>` is a
  reference raster whose grid - system, extent, cells - the result takes.
- **DIFFERENCE is the first minus the second.** Positive is fill (the first
  above the second: design above ground), negative is cut. The second is
  aligned to the first's grid only when the grids differ, by `raster
  reproject` with the first's extent, size and system said outright
  (bilinear unless `resampling=` says otherwise): GDAL's `--like` says the
  same in one word but is ignored, without a word, when the rasters have no
  CRS (measured: a 2 m grid "aligned" to a 1 m one stayed 2 m). One raster
  with a system and the other without is refused: there is nothing to align
  by. The subtraction is `raster calc`'s builtin `diff` (this GDAL has
  neither muparser nor ExprTk). An optional scope clips the difference to its
  closed boundaries before it is summed. The volumes are the cells' depths
  summed with `math::CompensatedSum`, times the cell's area, and labelled
  with the method: `method=grid label="grid method, cell 1 m"`.

A difference's reply:

```
difference algorithm="raster calc" aligned=yes cell=1 cells=1200 area=1200.000 cut=0.000 fill=360.000 net=360.000 method=grid label="grid method, cell 1 m" seconds=0.021
input arg=first source=file file=raised.tif
input arg=second source=file file=plane.asc
output arg=output kind=raster target=reference id=1 name=difference raster=40x30 file="..." persisted=no
```

**The window.** Terrain > DEM > DEM Tools (`terrainDemTools`) opens
`demToolsDialog` (`src/katana_qt/geo/dem_tools_dialog.hpp`), a tab per tool.
Each tab's raster comes from a binding picker (`binding_picker.hpp`: a
reference raster, a surface and its cell, or a file); CLIP's boundaries and
DIFFERENCE's limit are Global Modify's scope and filter controls. The lines
are made by `demToolCommandLine`, a pure function, and run through the
window's one executor. When a job ends the pickers reload, so what one tool
made is offered to the next.

**Tests.** `tests/geo/test_dem_verbs.cpp` (the executor, a Session and the
MCP server), `tests/qt_widgets/geo/test_dem_tools_dialog.cpp`,
`cli.raster_*` (`src/katana_app/geo/cli/dem.cmake`) and
`qt_dem_tools_clip_then_footprint_through_the_dialog_headless`
(`tests/geo/headless/dem.cmake`). The values, by hand, on plane.asc (40 x 30
cells of 1 m, z = 100 + 0.05x, read as Float32):

- the halves of the plane mosaicked again are the whole: GDAL's `raster
  compare` returns 0;
- AREA 0,0,20,15 on 1 m cells: 20 x 15;
- a 3 x 3 hole filled: every filled cell lies between the plane's values on
  the one-cell ring round the hole, 100.925 and 101.125 - an inverse-distance
  mean has positive weights, so it lies between the values it weighs; its
  weights are not symmetric, so the centre is not exact (measured 0.049 m
  off);
- the footprint of a full raster is its extent: 0,0 - 40,30, 1200 m2;
- reprojected into its own system, a raster keeps its grid and every value;
- the plane raised 0.3 m minus the plane: 0.3 in every cell, to an ulp of
  100 (1.4e-14; the raised copy is written from the very values GDAL reads),
  and 0.3 x 1200 = 360 m3 of fill; the other way round, 360 m3 of cut; within
  a 10 x 10 m rectangle, 30 m3;
- a second raster on 2 m cells aligned to the first's 1 m grid by bilinear
  interpolation, which reproduces a linear function exactly between the
  centres it interpolates: -0.3 in every cell with 2 m centres on both
  sides, to 1e-9.
