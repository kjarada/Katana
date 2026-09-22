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

**Still open.** 2 141 ms is not fast; it is 5.7x less slow. The remaining cost
is now spread across roughly two million pair clips rather than concentrated in
one call, so the next step - if the measurement justifies one - is the
`TaskPool`, which this loop suits: existing triangles are independent and the
only shared state is the compensated sums, which would need per-range
accumulators combined in index order to keep Rule 7.

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
right shape already.

### Why a fixed-seed LCG rather than `<random>`

The C++ standard fixes the *distribution* interfaces but not the output of
`std::mt19937`-fed distributions across implementations, so `<random>` would
give different points on a different standard library and the timings could not
be compared with the ones in this table. The LCG constants (Press et al.,
*Numerical Recipes* 3rd ed., section 7.1) are plain arithmetic on `uint32_t` and
produce an identical sequence everywhere.

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
