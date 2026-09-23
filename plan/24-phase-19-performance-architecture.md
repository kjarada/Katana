<!-- Katana plan, section 24 of 47. Index: ../PLAN.MD. Previous: 23-phase-18-spatial-indexing.md. Next: 25-phase-20-file-interoperability/00-overview.md -->

# 24. Phase 19 — Performance Architecture

**STATUS: PARTIALLY DELIVERED.**

`katana::core::TaskPool` (`include/katana/core/task_pool.hpp`): a `std::jthread`
pool created once and kept warm, offering `parallelFor` and `parallelRanges`
over an index space. It drives the renderer's three stages.

The contract is that parallelism must never change a result: `body` is called
exactly once per index, concurrent calls touch disjoint state, and the visit
order is unspecified and must not matter. Tests assert identical output at 0, 1,
3 and 8 workers. A nested submission runs inline rather than waiting, so it
cannot deadlock; an exception is captured and rethrown on the submitting thread
rather than crossing a thread boundary unhandled.

Also delivered here, because both were measured rather than assumed:

* **Snapping is no longer quadratic in polyline length.** Measured in Release at
  3000 vertices: 247 ms per mouse move before, 0.196 ms after a per-segment cull
  against the cursor reach — 1260x, and now linear. An equivalence test asserts
  the cull changes no result. See `docs/cad.md`.
* **The project store is not the bottleneck it was assumed to be.** Opening a
  50 000-entity project costs 891 ms, of which 602 ms (68%) is JSON decoding of
  geometry and only 289 ms is SQLite and model rebuilding. The decision record,
  including why DuckDB is the wrong engine for this table and where it would
  earn its place, is `docs/storage.md`.

**OUTSTANDING - and the list as written is in the wrong order.** It names a
task graph, work stealing, TBB and SIMD. Rule 6 says profile before optimising,
and none of those four is where the time goes. What is measured:

`benchmarks/bench_terrain.cpp` has since been written and run - terrain was
the largest area of the codebase with no benchmark, which is why this list had
been written from the shape of the API instead. Full table in `docs/terrain.md`.
Release, 16 x 2496 MHz:

* **`compareSurfaces` was 12 150 ms, now 2 141 ms (5.7x). DELIVERED.**
  Comparing a 200 000-vertex surface with a 50 000-vertex one took 7.2x the
  worst `buildTin` measured, for the single number an earthworks job exists to
  produce. **This was the bottleneck, and nothing in the original list above
  would have touched it.** Instrumentation put 82% of it in one constrained
  triangulation of the merged overlay: 250 000 points, 749 928 constraints.
  That triangulation was unnecessary. Two triangles are convex, so their
  intersection is a convex polygon needing no new vertices, and the set of
  those intersections is the same partition the global overlay was building.
  Pairs come from `geometry::SpatialIndex`, the broad phase the codebase
  already had. The answer is unchanged - no expected value was touched, and the
  exactness `volume.hpp` promises is preserved rather than traded for a grid.
  Full record, including why the clipper's inside test must be `>= 0`, in
  `docs/terrain.md`.
* **`buildTin` is 1681 ms at 400 000 points**, 360 ms at 100 000. Second
  largest, and the one every user reaches: it runs on import and on every
  surface rebuild. Cost per point rises from 3.49 to 4.20 microseconds across
  that range, which is the n log n of incremental Delaunay insertion, not
  anything quadratic.
* A task graph solves a problem Katana does not have. There is no fan-in of
  dependent stages anywhere; the renderer's three stages are a straight line.
* Work stealing would change visit order, which is exactly what `TaskPool`'s
  contract forbids, and incremental Delaunay insertion is sequential by
  construction. It is not a missing feature; it is a rejected one, and this
  plan should have said so.
* SIMD is not where the 12 seconds is.
* `elevationAt` (499 ns) and `contours` (~45 ms fixed, 1.97x for 25x the bands)
  are already the right shape and need no work.

An earlier draft of this section claimed 657.7 ms for the 400 000-point build.
That number was never measured on this machine and is wrong by 2.6x; it is
recorded here as struck rather than silently replaced, because CLAUDE.md
section 3 forbids trusting a figure without a source and this plan was briefly
guilty of exactly that.

## The optimisation pass of 2026-09-23 — DELIVERED

Five hot paths, analysed and changed at once by parallel agents on disjoint
directories (`CLAUDE.md` §5.3), then re-measured centrally. Full tables, method
and rejected findings in `docs/performance.md`; the reasoning lives beside the
code it governs in `docs/terrain.md`, `docs/geometry.md`, `docs/storage.md` and
`docs/interop.md`. Release, interleaved A/B on an idle machine:

* **`combineSurfaces` was 30 984 ms, now 248 ms (125x).** A 12d super tin of a
  diagonal corridor over a 200 000-point base took **thirty-one seconds**. Every
  triangle of a lower member scanned every vertex of every higher member; the
  vertices now go into a bucket grid built once per member. The output is
  identical - 402 719 triangles either way - and a test asserts that equality
  rather than inferring it from the timing.
* **`TiledTerrain::buildAll` 184 ms to 34 ms (5.3x)** and **`elevationsAt` 17.9
  ms to 2.5 ms (5.9x)**, both over `TaskPool`, both asserted bit-identical to
  the serial path. `buildAll` now builds every tile even when one fails and
  returns the lowest-index failure; that is a behaviour change and it is stated
  in the header.
* **`isSimple` 3.19 ms to 0.80 ms at 512 vertices (3.9x)**, from a bounding-box
  reject whose margin is *derived* from `intersect()`'s own acceptance rule
  rather than chosen. **`Rectangle2::closestPoint` 2.8x**, from a stack array
  instead of a heap vector of four corners.
* **Opening a 50 000-entity project: 184 ms to 170 ms (-7.6%)**, and creating
  27 000 entities **2.0x** faster on 61% fewer allocations. Three separate
  copies of every entity were removed - `ChangeSetCommand` built its change set
  twice, kept an image of every created entity that `remove()` already returns,
  and `applyToModel` duplicated the whole project on its way into the model.
* **Reading a 62.8 MB 12da archive: ~1.96 s to ~0.42 s (4.7x)**, by not building
  strings to throw away. Proved byte-identical on 21 archives including nine
  malformed ones. **This is the one figure here that was not re-measured
  centrally** - the archives are synthetic and not in the repository - and it is
  recorded as the implementing agent's measurement.

**A benchmark that lied, recorded because it is the useful part.** The first
`BM_CombineSurfaces` used an axis-aligned band and showed *no difference at
all*. `overlaps()` rejects by bounding box, then returns on the first covered
sample point, and only then scans vertices - so an axis-aligned band never
reaches the code that was changed. A diagonal band has a bounding box covering
the whole site while covering only a strip, which is what a road corridor or a
watercourse actually looks like. Both shapes are kept. The agents' own report
claimed 5.9x for this work from a harness with the same flaw; the real figure
is twenty times larger.

**OUTSTANDING.** Five of these optimisations had **no benchmark in the
repository** when they were made - they were measured in scratch harnesses that
are now gone, which `CLAUDE.md` §4 exists to prevent. They have benchmarks now
(`BM_CombineSurfaces`, `BM_ElevationsAt`, `BM_TiledTerrainBuildAll`,
`BM_IsSimple`, `BM_ClipPolygon`, `BM_RectangleClosestPoint`). Still missing: a
benchmark for a 12da read, and one for scene building and the per-frame draw
path, which is why the viewport work of 2026-09-22 quotes no figure.
`Importer::tallyFor` (`domain_import.cpp`) is a linear scan per element, the
third instance of that shape, worth about 1% of an import and left alone.

## The language standard is a build variable

The project builds as **C++26** (`KATANA_CXX_STANDARD`, default 26; GCC 16.2
accepts `-std=c++26`, `__cplusplus 202400L`, and the whole suite passes on it).
A C++23 build is one cache variable away, and exists so that a claim about the
standard can be **measured** rather than argued about:

```sh
cmake -S . -B build/rel23 -G Ninja -DCMAKE_BUILD_TYPE=Release -DKATANA_CXX_STANDARD=23
```

**No figure is claimed for C++23 versus C++26.** Two attempts to produce one
were unsound - the first was Windows clock quantisation reported as a result,
the second compared two sequential runs eight minutes apart on a laptop and
would have attributed thermal drift to the language. The A/A control that would
settle it was not run. `docs/performance.md` records both attempts so that the
next person starts from the control rather than from the A/B.


Latency, throughput, memory bandwidth and cache behaviour are still not
benchmarked as section 32 requires.

The win `docs/storage.md` named has since landed: geometry is stored as a
versioned little-endian blob (schema 3), taking a 50 000-entity project open
from 891 ms to 195 ms (4.6x) and a save from 445 ms to 206 ms (2.2x). Doubles
are stored by bit pattern, so the round trip is exact. Projects written before
schema 3 keep their JSON and convert on the next save.

---

