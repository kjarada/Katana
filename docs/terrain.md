# Terrain

Decisions and measurements for `katana::terrain`. `PLAN.MD` sections 19 (Phase
14), 24 (Phase 19) and 32 govern this area.

## Where the time actually goes

Terrain was the largest area of the codebase with no benchmark, so Phase 19's
outstanding list had been written from the shape of the API rather than from
evidence: it named a task graph, work stealing, TBB and SIMD. `benchmarks/bench_terrain.cpp`
was added to settle it. Rule 6 — profile before optimising.

Release, GCC 15.2 UCRT64, 16 × 2496 MHz, L3 18 MiB, `--benchmark_min_time=0.3s`.
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
| **`compareSurfaces` 200k vs 50k** | **12 150 ms** | exact overlay |

### What that changes

**`compareSurfaces` is the bottleneck, not `buildTin`.** Twelve seconds, on a
site that is not large, for the single number an earthworks job exists to
produce. It is 7.2× the worst `buildTin` in the table and 32× the cost of
building the design surface it compares against. Nothing in Phase 19's original
outstanding list would have touched it.

The reason is in `volume.hpp`: the overlay merges both triangulations into one
constrained triangulation with *every* edge of *both* surfaces as a constraint,
plus every crossing point. Comparing a 200k-vertex surface with a 50k-vertex one
therefore means a constrained insert of roughly 250 000 vertices and 750 000
constraint edges — and the breakline rows above show constraint is what is
expensive: 100 breaklines of 200 vertices (20 000 constrained edges, 8% of the
point count) add 55% to a build.

That exactness is not negotiable and must not be traded for a sampled grid — it
is the property `volume.hpp` promises and the reason the answer can be signed
off. The work is to make the *same* answer cheaper: the surfaces overlap only
where their bounds do, and the overlay currently gains nothing from the fact
that both inputs are already triangulated. Any change here needs an equivalence
test against the current result, per CLAUDE.md section 4.

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
