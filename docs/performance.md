# Performance: the standard, the measurements, and what actually moved

`PLAN.MD` §32 sets the targets and `CLAUDE.md` §4 sets the rules: measure in
Release, record both numbers, and never claim a speed-up that was not
measured. This file is where the numbers live.

Machine for every figure below: Windows 11, 16 logical cores, GCC 16.2
(MSYS2 UCRT64), Ninja, `-O3 -DNDEBUG`, `-ffp-contract=off -fno-fast-math`.

## The C++ standard is a build variable

```sh
cmake -S . -B build/release -G Ninja -DCMAKE_BUILD_TYPE=Release   # C++26, the default
cmake -S . -B build/rel23  -G Ninja -DCMAKE_BUILD_TYPE=Release -DKATANA_CXX_STANDARD=23
```

`KATANA_CXX_STANDARD` exists so that a claim about the standard can be
**measured** rather than argued about. The project ships C++26; GCC 16.2
accepts `-std=c++26` (`__cplusplus 202400L`) and the whole suite passes on it.

### What the standard alone is worth: not established

An attempt was made to answer "is C++26 faster?" by building the 103
benchmarks both ways and comparing. **The comparison is not reported here,
because it was not sound**, and the reason is worth recording so the next
person does not repeat it:

- The first attempt used `--benchmark_min_time=0.05s` and one repetition.
  Timings came back quantised to values like 6250.000 µs and 4166.667 µs -
  Windows clock granularity, not the code - with a ±26 % spread and swings of
  both signs. Noise, reported as if it were a result.
- The second attempt used five repetitions at 0.5 s and compared medians. It
  showed the C++26 build slower on 83 of 103 benchmarks. That is a suspicious
  result with an obvious confounder: the two runs were sequential, eight
  minutes apart, on a laptop. Thermal and background drift alone can produce
  a uniform one-sided shift of that size.

The control that would settle it - running the SAME binary twice and
measuring the drift between those two runs - was not completed. **So there is
no defensible figure for C++23 versus C++26 here, and none is quoted.**
Anyone picking this up should run the A/A control first; without it an A/B on
this machine means nothing.

Both flag sets were verified identical (`-O3 -DNDEBUG`), so the standard was
the only intended difference.

## What was optimised, and why

The work below came from a read-only analysis of the hot paths, done by
parallel agents (`CLAUDE.md` §5.3), and each item was checked against the
code before it was acted on. Where the analysis was wrong, that is recorded
too - a rejected finding is as useful as an accepted one.

### Per-frame drawing (`viewport_widget.cpp`, `scene.cpp`)

Every entity drawn was resolved through `resolveDisplay` **twice** a frame:
once by the caller and once inside `resolveHatchPattern`. A resolution walks
the layer table and the style table and builds three `std::string`s. On top
of that the layer was looked up twice (once inside `isDrawn`, once for its
colour), the dimension style was resolved for every entity although only a
dimension can use one, and the dash pattern was rebuilt - with two container
allocations - for every entity carrying a linetype.

Changed:

- `resolveHatchPattern` and `colorOf` take a display the caller already has.
- `isDrawn` has an overload taking the layer the caller already found.
- The dimension style is resolved only for `DimensionGeometry`.
- Dash patterns are cached for the duration of one frame, keyed by linetype
  and pen width. The cache is cleared at the top of every `drawEntities`,
  because a pattern is a function of the **view scale** too and that changes
  between frames.

**Not changed, and deliberately:** the bounding-box test in `drawEntities`
looks redundant against the one `forEachCandidate` already did, and is not.
`queryExtents` is documented as deliberately wider than the geometry - an arc
offers its centre so it can be snapped to - so the viewport's own test is the
tighter, drawing-specific filter. Removing it would paint entities that are
off screen.

This path has **no benchmark**, so no figure is quoted for it. What is
provable without one is the work removed: one fewer full display resolution,
two fewer string-keyed map lookups and two fewer allocations per entity per
frame. A scene-building benchmark is the obvious next step.

## The optimisation pass of 2026-09-23

Five hot paths were analysed and changed at once, by parallel agents working on
disjoint directories (`CLAUDE.md` §5.3). **Every figure below was re-measured
centrally afterwards**, not taken from an agent's report, and two of the
agents' own numbers turned out to be wrong in opposite directions - one badly
understated, one unmeasurable. That is the reason for the rule.

Method for every A/B here: two binaries built from the same tree differing ONLY
in the code under test, run back to back in alternating rounds on an otherwise
idle machine, medians of 3-5 repetitions. The earlier attempts in this file
failed because their two runs were minutes apart with other work in between;
these were interleaved for exactly that reason.

### Terrain and geometry

| benchmark | before | after | |
|---|---|---|---|
| `BM_CombineSurfaces/1` diagonal band over a 200k-point base | 30 984 / 31 002 ms | 248 / 254 ms | **125x** |
| `BM_CombineSurfaces/0` pad over 4% of the base | 217 / 224 ms | 211 / 217 ms | unchanged |
| `BM_ElevationsAt` 100 000 probes | 17.9 / 17.4 ms | 2.52 / 3.44 ms | **5.9x** |
| `BM_TiledTerrainBuildAll` 150k points, 64 tiles | 184 / 185 ms | 35.5 / 33.7 ms | **5.3x** |
| `BM_IsSimple/512` | 3.19 / 3.16 ms | 0.801 / 0.806 ms | **3.9x** |
| `BM_IsSimple/64` | 51.2 / 51.9 us | 11.0 / 11.2 us | **4.6x** |
| `BM_RectangleClosestPoint` 1024 probes | 79.5 / 81.6 us | 28.2 / 28.5 us | **2.8x** |
| `BM_ClipPolygon` 512-vertex subject | 10.25 / 10.31 us | 9.02 / 8.81 us | 1.15x |

`combineSurfaces` is the headline: a 12d super tin of two real surfaces took
**thirty-one seconds** and now takes a quarter of one. The output is identical -
402 719 triangles before and after, and `SuperSurface.TheIndexedVertexTestKeeps
ExactlyTheTrianglesTheExhaustiveScanKept` asserts that equality rather than
trusting the timing.

### Why `BM_CombineSurfaces` takes two shapes, and the benchmark that lied

The first version of this benchmark used an axis-aligned band and showed **no
difference at all** between the exhaustive scan and the indexed one. The
benchmark was wrong, not the change.

`overlaps()` has three stages: reject by the higher member's bounding box, then
return true if any of the triangle's four sample points is covered, and only
then scan the member's vertices. The expensive stage is therefore reached ONLY
by a triangle that is inside the higher member's bounding box and is not under
its surface. An axis-aligned band culls nine triangles in ten at the first
stage and answers most of the rest at the second, so it never reaches the code
that was optimised. A band running **diagonally** has a bounding box covering
almost the whole site while covering only a narrow strip, so nearly every
triangle falls through to the scan - the shape a diagonal road corridor or a
watercourse actually has.

Both shapes are kept: the pad measures that the cheap path did not regress, and
the diagonal band measures the path that was changed. The agents' own report
claimed 5.9x for this work, from a harness whose shape partly culled; the real
figure on the shape that reaches the code is twenty times larger than that.

### Five of these had no benchmark at all

`isSimple`, `clipPolygon`, `Rectangle2::closestPoint`, `elevationsAt` and
`buildAll` were optimised against a scratch harness that lived in a temporary
directory and is now gone. `CLAUDE.md` §4 says a measurement lives in
`benchmarks/` so that it can be repeated; these five could not be. They now have
benchmarks in `bench_terrain.cpp` and `bench_geometry.cpp`, which is how the
numbers above were taken and how the next contributor will notice a regression.

### Storage and commands

| measurement | before | after | |
|---|---|---|---|
| `BM_ProjectOpenAndLoad/50000` (whole open: SQLite, JSON, apply) | 184 / 185 / 184 ms | 170 / 170 / 177 ms | **-7.6%** |
| `CREATE_ENTITIES` execute, 27k entities (allocations) | 330 785 | 128 269 | **-61%** |
| the same, wall time | 56.5 ms | 27.7 ms | **2.0x** |
| open + load + apply, 50k entities (allocations) | 1 375 127 | 1 275 112 | -7.3% |

The allocation counts are deterministic and repeat exactly; they are the honest
measurement on a contended machine, and they were taken by the agent that did
the work. The `BM_ProjectOpenAndLoad` figure is mine, interleaved, after the
machine went quiet - `after` was faster in all three rounds with about 1 ms of
spread within a round.

Three separate copies of every entity were removed:

- **`ChangeSetCommand` built its change set twice.** Every path that runs a
  command calls `validate()` and then `execute()` with nothing in between that
  can touch the model, and each built the set from scratch - so creating 27 000
  entities copied all 27 000 of them twice. `validate()` now keeps what it
  built and `execute()` consumes it. `validate()` still rebuilds on every call,
  so it remains a pure function of the model as it is now, and `execute()`
  re-validates against the live model regardless, so a set that somehow went
  stale is rejected rather than applied.
- **`execute()` kept an image of every created entity** so that `redo()` could
  put it back. It does not need to: `EntityDatabase::remove()` *returns* the
  entity it removes, and `undo()` always runs before `redo()`, so the image is
  free at undo time.
- **`applyToModel` copied the whole project into the model.** Opening a drawing
  loads a `ProjectContents`, applies it and throws it away, and every `Entity`
  in it - two `std::map`s and a geometry variant holding vectors - was
  duplicated on the way in. It was 22.6% of every allocation the open made.
  There is now a consuming overload, and `Document::open` uses it.

One copy was measured and **kept**: `createEntities` copies its captured
entities into the change set. A builder is documented as a pure function of the
model and `Transaction` really does call `validate()` twice on its first
command, so a builder that moved its entities out would hand the second call an
empty set and the command would be rejected as one that changes nothing. With
the cache above, that copy now happens once per command instead of once per
call. The reasoning is recorded at `entity_commands.cpp:233`.

`SqliteStatement` gained `columnTextView`/`columnBlobSpan`, borrowing views into
SQLite's own row buffer for values that are parsed and discarded within the row.
The owning `columnText`/`columnBlob` are now implemented as "the borrowing one
plus a copy", so one place knows how SQLite hands a value over. The lifetime
rule is on the declarations: a view dies at the next `step()`.

### The 12da reader

Measured by the agent that did the work, on synthetic archives generated in a
scratch directory. **These are the one set of figures here that could not be
re-measured centrally**, because the archives are not in the repository and a
58 MB fixture does not belong in it. They are recorded as its measurement, not
as a verified one; a 12da read benchmark is outstanding.

| archive | before | after | |
|---|---|---|---|
| 62.8 MB, 100k elements, 400 models | 1916-2000 ms | 408-425 ms | 4.7x |
| 15.1 MB, 2401 elements, 233k-triangle TIN | 79.0 ms | 71.0 ms | -10% |

What changed: the keyword table is asked with the token as written instead of a
lower-cased copy of it; a 64-byte inline case-folding buffer replaced 2.4
million heap-allocating `lowered()` calls per read; model lookup became a
case-folded hash map instead of a linear scan (20.1 million case-insensitive
comparisons per read before); and `Scope` holds a `std::string_view`, with the
`std::string&&` constructor **deleted** so a temporary cannot be borrowed.

Behaviour was proved unchanged rather than assumed: a reference build of the
pre-change sources and the new one produce byte-identical probe reports and
byte-identical rewritten archives for all six repository fixtures, three
synthetic archives, all five text encodings and nine deliberately malformed
archives. A build with the inline buffer cut to one byte - forcing the heap
spill on every token - produces the same output again.

### Rejected, with the measurement that rejected it

- **Reserving the value vectors in `readNumberBlock`.** Measured: reserving 1M
  values made the TIN-heavy read *slower*, 72 ms to 96-105 ms, because both
  callers also run for the many small blocks and each then takes an 8 MB
  allocation. With a growth factor of 2 the total copied is about 2n, so a
  fixed "sensible chunk" is amortised-irrelevant anyway.
- **Reserving the element vector.** Reserving at exactly the true count saves
  ~7% of a 500 ms read - but `Element` is 1040 bytes and the reader cannot know
  the count without a second pass, and a byte-based estimate either falls far
  short or wastes 30 MB on a 15 MB file. Recorded as a known cost rather than
  bought with a guessed constant.
- **Removing the bounding-box test in `drawEntities`.** It looks redundant
  against the one `forEachCandidate` already did, and is not: `queryExtents` is
  deliberately wider than the geometry, so the viewport's own test is the
  tighter filter and removing it would paint entities that are off screen.

### The rasteriser's clipping rewrite (2026-09-23)

The software rasteriser had pixel tests and no timing. Its clipping was
rewritten to fix a correctness defect (PLAN.MD Phase 15: primitives crossing
the eye plane were dropped whole), and the new clipping does work on every
triangle, so `bench_render.cpp` was written first and run on the old code.
Ground grids at 1920x1080: `Framed` has every vertex in view (the pass-through
path), `Within` puts the eye 2 m over the middle of the grid so triangles cross
the near plane and the guard band (the clipping path).

The machine was not quiet (the application was open), and single runs of the
same binary varied by up to 2x - one "after" run first reported the serial
131k-triangle case at 25.7 ms against 13.1 ms before, which re-running did not
reproduce. So the three builds were run ALTERNATELY, six rounds of three
repetitions each, and the table gives the minimum and the median of the 18
samples (`python tools/compare_benchmarks.py --alternate 6 BM_RenderGround
old=... new=...`):

| ms, min / median | old (w > 1e-6) | clip test per triangle | clip codes per vertex (shipped) |
|---|---|---|---|
| Framed, 131k tris | 3.74 / 4.16 | 3.37 / 4.39 | 3.05 / 4.39 |
| Framed, 1.05M tris | 14.82 / 17.35 | 14.93 / 18.12 | 14.64 / 17.94 |
| Framed serial, 131k | 13.31 / 15.34 | 14.34 / 15.89 | 14.50 / 15.25 |
| Framed serial, 1.05M | 59.67 / 65.42 | 60.89 / 70.81 | 57.83 / 64.21 |
| Within, 131k | 5.88 / 7.54 | 6.27 / 8.25 | 5.87 / 7.66 |
| Within, 1.05M | 11.62 / 13.86 | 13.05 / 16.96 | 10.39 / 13.02 |

Reading it: the shipped version is within the noise of the old code
everywhere. (In `Within` the triangles that cross the eye plane lie below the
bottom edge of the view, so both versions write the same 1.71M fragments: the
case measures the clipping arithmetic, not the defect, which the pixel tests
cover.) The middle column is the
first version of the fix, which tested each triangle's three corners against
five planes: a TIN vertex is a corner of about six triangles, so it did the
same test six times, in the serial-per-chunk stage, and it shows in the
medians of the clipping case. The shipped version classifies each vertex ONCE,
in the parallel transform, into a five-bit code; a triangle needs two ORs to
know it can pass through and an AND to know it cannot be seen.

### Still open

- No benchmark covers scene building or the Qt per-frame draw path, so the
  viewport work earlier in this file still quotes no figure. The software
  rasteriser has one now (`bench_render.cpp`, above).
- No benchmark covers a 12da read.
- `Importer::tallyFor` (`domain_import.cpp`) is a linear scan per element, the
  third instance of that shape. About 1% of an import, left alone.

## SIMD: kernels chosen at run time (2026-09-24)

The program ships for baseline x86-64 and has to start on any 64-bit PC. A few
small kernels now process four doubles, or 32 bytes, per instruction on a
processor with AVX2. Which path runs is decided once, at start-up, and every
kernel gives exactly the same bits as the scalar code it replaces. This
section covers the policy, the mechanism, the hazard that shapes both, and
what was measured.

### The policy

- **The shipped build stays at the baseline.** Nothing is compiled with
  `-march=x86-64-v3`. Putting that flag on the whole build was measured
  before this work, in an interleaved A/B/A run against the release binary.
  No benchmark moved outside the A/A spread. The hot code here is arrays of
  structs of doubles, maps and branches, and the auto-vectoriser does not
  help with those. SIMD has to be written by hand.
- **SIMD code lives in kernel files and is chosen at run time.** Each kernel
  has a scalar reference. That reference is the code as it was before, and
  it is what a processor without AVX2 runs.
- **Every kernel is bit-identical to its scalar reference.** This includes
  signed zeros and which values count as NaN. It does not include a NaN's
  payload (see below). This rule is what lets a test switch the level. It is
  also why each lane does the reference's operations in the reference's
  order and never fuses a multiply with an add. `-ffp-contract=off` stays on
  for kernel files. `-mfma` is only there so that the flag set matches
  x86-64-v3. The object check fails the build if a fused instruction ever
  appears.

### Dispatch

`include/katana/core/cpu_features.hpp`:

- `detectedSimdLevel()` uses `__builtin_cpu_supports("avx2")` and `("fma")`.
  libgcc sets these only when XGETBV shows that the operating system saves
  the YMM registers. So the answer means "this process may execute AVX2",
  not just "the chip has it".
- `activeSimdLevel()` is what every kernel family asks. After the first call
  it costs one relaxed atomic load.
- **`KATANA_SIMD=scalar|avx2`** is read once, on first use. `scalar` is how a
  machine that has AVX2 proves the path a machine without it would take.
  Asking for a level the processor cannot run, or giving a word that is not
  a level, is refused and not acted on. The refusal is written once to
  stderr and kept in `simdSelection().note`. The program does not fault, and
  it does not quietly run scalar under an AVX2 label.
- `setSimdLevel()` is the in-process form of the override, used by tests and
  benchmarks. Switching is legitimate at any time only because the levels
  are bit-identical.

Each caller dispatches in a baseline file with
`if (activeSimdLevel() == SimdLevel::Avx2)`. Short inputs skip the dispatch,
and the thresholds are measured, not guessed (see "Measured" below; the first
guesses were wrong in both places):

- `boundsOf` below `kBoundsBatchMinimum` (16 points) is the `expand()` loop,
  inline in `point_batch.hpp`, so a drawing's strings (about six vertices
  each) cost no call at all. From 16 points it calls the batch code, which
  takes the kernel. The kernel with its call and its zero-sign fix-up breaks
  even with the loop at about 12-16 points.
- `isValidUtf8` takes the kernel from 64 bytes (`kValidateMinimum` in
  `text_encoding.cpp`). The kernel ends on a whole 32-byte block that begins
  inside the text, so it needs 64. A padded copy for shorter texts was
  measured and does not pay: slower than the byte loop up to about 24
  bytes, and at most 15 ns faster up to 63. So names and labels take main's
  byte loop at every level, with no extra call.
- `transformPoints` takes the kernel from 8 points (two kernel steps). It has
  no caller in the program yet, so its break-even has not been measured.

Not used, and why:

- **`target_clones`** needs ifunc, which MinGW does not have. It does not
  compile.
- **Highway 1.4** is installed, builds with GCC 16, links statically and
  dispatched to AVX2 in a probe. It has not been adopted yet. It would be a
  new third-party dependency to vet, and six kernel entries, the largest
  about 150 lines, do not need it. It is worth revisiting if the kernels
  multiply or a second architecture matters. Its per-target namespaces are
  its own answer to the hazard below.

### The inline-copy hazard

An inline function can be used by two object files compiled with different
`-m` flags. Examples are a template member, anything defined in a header, or
`std::span::data`. Each object then carries its own out-of-line copy of the
function (a COMDAT), and the linker keeps only one of them for the whole
program: whichever it meets first. If it keeps the AVX2 object's copy,
baseline code calls AVX2 instructions. The program then faults on a processor
without AVX2, far from any kernel, and only in some link orders. This was
demonstrated with this toolchain. A shared header function was compiled into
both an x86-64-v3 object and a baseline one, and the baseline `main` ended up
calling a copy with 20 VEX instructions in it.

An unoptimised build makes it certain rather than possible. A 12-line bounds
kernel compiled at `-O0 -mavx2` behaved as follows:

- Written with `std::simd` (GCC 16, C++26), it left seven functions out of
  line with external linkage. Two of them were
  `std::span<double const>::data() const` and
  `std::integral_constant<int, 4>::operator()() const`.
- Written with `std::experimental::simd`, it left
  `simd<double, _VecBuiltin<32>>::size()`.

At `-O3` both versions inlined everything. That was the optimiser's choice,
not a guarantee.

So a kernel file follows these rules:

1. It is named `*_avx2.cpp` and added with
   `katana_add_simd_sources(<target> AVX2 <files>)` (`cmake/KatanaSimd.cmake`).
   The helper compiles it into an OBJECT library with `-mavx2 -mfma` added to
   the project's own flags, and links the objects into the target. The
   helper refuses any other name. The option `KATANA_SIMD_KERNELS=OFF`, or
   any architecture other than x86-64, leaves only the scalar references.
2. It includes only `<immintrin.h>`, `<cstddef>`, `<cstdint>` and its own
   `*_kernels.hpp`, which holds declarations only. GCC's intrinsics are
   `gnu_inline` and `always_inline`, so they are never emitted out of line.
3. It uses raw pointers at the boundary. Everything except the entry points
   sits in an anonymous namespace. The entry points have C linkage and a
   `katana_avx2_` prefix.
4. It has no namespace-scope object with a dynamic initialiser. Such an
   initialiser runs at start-up, on every machine.

### The guard

Every `ctest` run includes three checks, all in `tools/check_simd_kernels.cmake`:

- **`simd_kernel_sources`** reads source files, like `layering` does. It
  checks rule 2 for each `src/**/*_avx2.cpp` and for its declarations header,
  which may contain no braces other than `extern "C"` and namespace blocks.
  It also enforces the OpenMP rule below.
- **`simd_kernel_objects`** reads the objects this build actually produced,
  so it applies in Debug and Release alike. `nm` must find no externally
  visible code symbol other than `katana_avx2_*`, and at least one entry per
  object. `objdump -h` must find no `.ctors` or `.init_array` section.
  `objdump -d` must find no `vfmadd`, `vfmsub`, `vfnmadd` or `vfnmsub`.
- **`simd_kernel_objects_under_asan`** runs the same object check on the
  kernels compiled a second time with `-fsanitize=address`, as the
  `linux-sanitize` preset compiles them. They are only compiled, never
  linked, so it runs on MinGW too, which has no ASan runtime. It exists
  because that preset cannot be run on the owner's machine, and it was
  broken: ASan puts a module constructor into every object (`.ctors.65436`
  on MinGW, `.init_array.00099` on Linux), and `simd_kernel_objects` failed
  on it every time. `katana_simd_kernel_options` now compiles kernel objects
  with `-fno-sanitize=address`. UBSan adds no constructor and stays. The
  price is that ASan would not report a kernel reading past the end of its
  input. The kernels guard against that by their structure (whole blocks
  only while a whole block remains, then a scalar tail, or in the UTF-8
  validator a last block that ends exactly at the text's end), and the tests
  run lengths across every block edge. Without the flag this check fails on
  both kernel objects with "has a static initialiser (.ctors)".

The object check is the one that matters, because it checks what the compiler
did rather than what the source intended. It was run on four deliberately bad
objects and rejected each one, giving the reason:

- the `-O0` `std::simd` kernel above;
- the `-O0` TS kernel;
- a kernel built with `-ffp-contract=fast` (`vfmadd213pd`);
- a kernel with a namespace-scope `__m256d` constant (`.ctors`).

The source check was run on a tree built to break the rules. It rejected
`<vector>`, a `katana/` header, a function body in a declarations header,
`-fopenmp` in a CMakeLists, and `#pragma omp parallel for`. One ordinary test,
`SimdLevel.OrdinaryCodeIsCompiledForTheBaselineSoTheProgramStartsOnAnyX64Machine`,
fails if the files every target compiles are ever built with AVX enabled.

### The facade, `core/simd.hpp`

The facade is `std::simd` where libstdc++ sets `__glibcxx_simd` (GCC 16 in
C++26 mode; `__cpp_lib_simd` is deliberately undefined there), and
`std::experimental::simd` otherwise. So a `KATANA_CXX_STANDARD=23` build still
compiles.

It is for ordinary files, which are compiled for the baseline. There a
vector is 2 doubles, 4 floats or 16 bytes. It is portable, needs no dispatch
and carries no hazard. It is not for kernel files: the `-O0` probe above is
why, and the source check refuses it there.

`minOf` and `maxOf` are written as compare-and-select, which is `std::min` and
`std::max` lane by lane. The TS's own `min` and `max` are marked
`finite-math-only,no-signed-zeros`, which leaves the result for NaNs and
zeros up to the optimiser. No production code uses the facade yet. It exists
so that the next vectorised loop is written once for both standards. The
rasteriser's pixel loop is the obvious one: it was measured bit-identical and
2.2-2.5x faster on large triangles with 8-lane `std::simd`, and it would be
written as a kernel file.

### OpenMP: no threading, `-fopenmp-simd` only as a hint

`core::TaskPool` is the only thread pool. OpenMP would add nothing it lacks.
libgomp already ships, because GDAL and PDAL use it, and an OpenMP region run
straight after a TaskPool job showed no conflict. But OpenMP reductions
combine in no fixed order, which breaks the bit-identical rule. Running both
pools at once would also put 31 threads on 16 cores.

`-fopenmp-simd` needs no runtime and only gives the vectoriser permission, so
it is allowed, though nothing uses it now. `simd_kernel_sources` fails on
`-fopenmp`, `OpenMP::` or `find_package(OpenMP)` in any build file, and on
any `#pragma omp` other than `omp simd`.

### The kernels

| kernel | called from | one AVX2 step | scalar reference |
|---|---|---|---|
| UTF-16 to UTF-8, ASCII runs | `decodeText`, both byte orders | 32 units: two loads, one test, pack, permute, one store | the unit loop, which takes over at the first non-ASCII unit and reports errors at the same byte as before |
| UTF-8 validation | `isValidUtf8`, for texts of 64 bytes or more | 32 bytes: three nibble-table lookups (VPSHUFB) on each byte and the one before it, two saturating subtractions for leads two and three back, one XOR; a 64-byte step of pure ASCII is one OR and one movemask | main's byte loop, unchanged |
| `geometry::transformPoints` (Mat4 with Vec3, Mat3 with Point2, in place) | the batch API | 4 points: 3 loads, 6 permutes or blends, 9 multiplies and 9 adds, 6 permutes back (2D: no transpose) | `math::transformPoint` on each point |
| `geometry::boundsOf` (Point2, Vec3) | `Polyline2::boundingBox`, `TriangleMesh::bounds`, for 16 points or more | 4 points: one MINPD and one MAXPD per register, lanes folded at the end | the `expand()` loop |

Both text kernels are on the real import path. The interop archive import
decodes the file and then calls `readArchive`, which validates the whole
decoded text again. `decodeText` also validates the whole of a file that has
no byte order mark, to tell UTF-8 from Windows-1252.

**Why validation is a whole validator, not an ASCII fast path.** The first
version looked for runs of ASCII and handed them to a kernel. That made text
dense with accented, Cyrillic or CJK characters 2-4.5x slower than main,
because it paid a call for every short run between two characters. A 16-byte
byte-by-byte probe before the call only narrowed that: in one binary against
main's own loop (21 alternating rounds, minimum ms) it was still 1.44x main
on accented text, 1.15x on Cyrillic and 1.25x on CJK. Every other placement
of an ASCII check tried there (a run counter, an 8-byte SWAR skip, a check at
the start of each run, a check at aligned positions) cost 10-40% on one of
those shapes, because main's loop is already about one cycle a byte and any
work per run or per character is a large share of that. The validator judges
every byte against the three before it, whatever the language, using the
lookup algorithm of J. Keiser and D. Lemire ("Validating UTF-8 in less than
one instruction per byte", Software: Practice and Experience, 2021). It never
branches on where one character ends. The last 32 bytes of a text are judged
again with the 32 before them, so there is no padded partial block. Judging a
byte twice gives the same answer. The result is a yes or a no, and it is the
same yes or no as the byte loop's.

Keeping the results bit-identical required three details:

- **MINPD and MAXPD** return their second operand unless the first is
  strictly less (or greater). With the new value first, that is exactly
  `expand()`'s `std::min(acc, v)`, so a NaN is passed over just as the loop
  passes over it.
- **Which zero is kept.** When the extreme is zero, the loop keeps the first
  zero it met (`-0 < +0` is false). The kernel folds its lanes in a different
  order, so `boundsOf` takes the value from the kernel and the sign from the
  first zero in the array. One test places its zeros so that a kernel that
  merely folded its lanes would fail it. With the fix-up disabled, that test
  and the random property test both fail.
- **NaN payloads.** A result that is NaN must be NaN at every level, but
  which payload it carries is not compared. IEEE leaves the payload to
  operand order, and GCC may swap the operands of a commutative operation
  even in the scalar reference.

The tests hold every kernel to its reference through the public function,
running at each level in one process (`tests/core/test_simd_text.cpp`,
`tests/geometry/test_point_batch.cpp`). This covers 3000 generated UTF-16
inputs, every position of a bad byte across the first three 64-byte steps,
text dense with accented letters at every ASCII run length from 0 to 40 and
every cut near its end, and for the validator: sixteen kinds of error worked
by hand at block edges, all 65,536 byte pairs at six places (18,304 valid at
each, counted by hand), every lead C0-FF with sixteen kinds of follower, and
6000 generated texts with damage. A scratch run, not committed, compared the
validator with main's loop on 225 million texts at `-O3` and `-O0`, with no
difference. There are also 300 transforms and 600 bounds over arrays of
every length up to 70 with zeros, NaN and infinities. The hand-worked bounds
cases are 17-20 points long and assert that they reach `kBoundsBatchMinimum`,
so that at the AVX2 level they test the kernel and not the inline loop; with
the minimum set to 24 they fail on that assertion.
`PointBatch.AnArrayShorterThanTheBatchMinimumIsBoundedInlineWithoutACall`
bounds three points in `static_assert`s, which compile only while the short
path is inline. ctest also runs the whole core and geometry suites
a second time under `KATANA_SIMD=scalar` and under `KATANA_SIMD=avx2`
(`simd_scalar.*`, `simd_avx2.*`). Mutations were tried to confirm that the
tests have teeth: disabling the zero fix-up; changing the text kernel's lane
permute from `0xD8` to `0x00`; and five in the validator (the surrogate bit
dropped from a table, a character left open before a step of ASCII not
counted, the last block judged without the 32 bytes before it, a character
open at the text's end not counted, and the rule for a four-byte lead's
continuations dropped). Each one failed the tests; the validator's each
failed three to five of them.

### Measured

The machine was an i7-1270P on AC power, with other agents' builds running
throughout. Only the ratios and the A/A spreads below mean anything; the
absolute times do not.

**Benchmarks** (`benchmarks/bench_simd.cpp`). Every benchmark is run twice,
once per level: `BM_x/scalar` and `BM_x/avx2`. The comparison ran three
binaries in alternating rounds:

- `main`: the file compiled against main's sources, with no SIMD layer, so
  both members time the old code;
- `new`: this branch;
- `new again`: a byte-identical copy of `new`, as the A/A control.

The command was
`python tools/compare_benchmarks.py --alternate 6 "BM_(DecodeUtf16|IsValidUtf8|TransformPoints[23]Batch|BoundsOfPoints|PolylineBoundingBoxes)" main=... new=... new_again=...`,
giving 18 samples per cell. Each cell is min / median in ms.

| benchmark | main | new, scalar | new, avx2 | new again, avx2 (A/A) |
|---|---|---|---|---|
| `DecodeUtf16Archive` (8 MB of UTF-16 in an archive's shape) | 13.95 / 20.92 | 3.32 / 4.52 | 1.43 / 2.24 | 1.41 / 2.30 |
| `DecodeUtf16Mixed` (a non-ASCII character every 4th) | 6.72 / 10.77 | 4.28 / 7.79 | 4.27 / 9.41 | 4.96 / 8.62 |
| `IsValidUtf8Archive` (4 MB) | 1.99 / 3.21 | 1.08 / 2.55 | 0.13 / 0.25 | 0.16 / 0.24 |
| `IsValidUtf8Names` (10,000 short names) | 0.09 / 0.19 | 0.09 / 0.29 | 0.12 / 0.28 | 0.09 / 0.27 |
| `BoundsOfPoints2` (65,536 points) | 0.20 / 0.26 | 0.22 / 0.27 | 0.03 / 0.04 | 0.02 / 0.04 |
| `BoundsOfPoints3` (65,536 points) | 0.22 / 0.27 | 0.22 / 0.24 | 0.04 / 0.05 | 0.04 / 0.08 |
| `PolylineBoundingBoxes` (28,000 strings, 2-10 vertices) | 0.61 / 0.88 | 0.65 / 0.95 | 0.68 / 0.99 | 0.62 / 1.55 |
| `TransformPoints2Batch` (65,536 points, in place) | - | 0.08 / 0.12 | 0.04 / 0.06 | 0.02 / 0.07 |
| `TransformPoints3Batch` (65,536 points, in place) | - | 0.11 / 0.19 | 0.09 / 0.20 | 0.07 / 0.12 |

The main column comes from the `/scalar` member, since both members ran the
old code there. The two transform rows have no main figure. On main they
timed a copy from one array into another, while here they transform one
array in place. That is different memory traffic, so the rows are compared
only across levels.

How to read the table:

- **Decoding.** Most of the gain on decoding comes from the scalar path, not
  from AVX2. The old loop appended one `char` at a time to a `std::string`,
  checking capacity for every unit. The new structure narrows a whole run
  into the string's buffer through `resize_and_overwrite`. That alone is
  about 4x (13.95 to 3.32 ms at the minimum), and AVX2 halves what is left
  (3.32 to 1.43). Main to AVX2 is 9.8x at the minimum and 9.3x at the
  median. The A/A control agrees to within 3%.
- **The adversarial mix.** The mix enters and leaves the fast path every
  few units. It is not slower than main at either level (4.3 against 6.7 ms
  at the minimum). The AVX2 and scalar medians sit inside the A/A spread.
- **Validation.** Validating a long text with AVX2 is about 8x faster by
  the minimum and 10x by the median than the scalar loop, which is the same
  code as main's. Short names are unchanged within the noise, because they
  never reach the kernel.
- **Bounds.** Bounds of a large array are 5-7x faster. A drawing's strings,
  mostly under the 8-point minimum, are unchanged within the A/A spread,
  which is wide on that row (0.99 against 1.55 ms median for one binary run
  twice).
- **Transforms.** The 2D transform is about 2x faster. The 3D transform is
  1.2-1.6x faster by the minimum, and its medians are inside the noise. With
  its transpose it does 12 shuffles for 18 arithmetic instructions, over
  data that is 1.5 MB and does not fit in L2 cache. It is a batch API with
  no production caller today, and it is recorded here as a small win, not a
  large one.

**Real archives.** A scratch harness, not committed, timed three of the
owner's real archives. It compiled main's `text_encoding.cpp` into a renamed
namespace and linked it beside the new one. It then ran five arms in
alternating order, 7 rounds each: main's code, new scalar, new AVX2, and a
second new scalar and new AVX2 as A/A controls. All five arms produced
byte-identical text on all three archives, and every text validated.
Each cell is min / median in ms.

| archive (UTF-16LE) | main decode | new scalar decode | new AVX2 decode (A/A) | main validate | new AVX2 validate (A/A) |
|---|---|---|---|---|---|
| 55 MB, 229k-triangle TIN | 140.8 / 168.5 | 24.9 / 30.5 | 9.2 / 11.8 (10.6 / 12.4) | 12.0 / 18.7 | 1.5 / 2.5 (2.0 / 2.7) |
| 58 MB, meshes | 135.7 / 164.7 | 33.5 / 38.4 | 11.2 / 14.0 (11.4 / 14.0) | 19.4 / 23.9 | 2.0 / 3.0 (2.0 / 2.9) |
| 33 MB, 27,886-entity drawing | 87.3 / 112.8 | 16.4 / 24.2 | 7.3 / 8.3 (5.5 / 7.8) | 9.8 / 27.6 | 1.2 / 1.9 (0.9 / 1.6) |

Decoding the largest real archive fell from about 141-169 ms to 9-12 ms,
14-15x. That is close to the ~15 ms estimated before the work. Its second
full pass, the UTF-8 validation that `readArchive` runs on the decoded text,
fell 7-8x. Together that is about 150-190 ms of every large import down to
about 11-15 ms. A processor without AVX2 still gets the scalar column, which
is 4-6x faster than main.

### Not done, and where next

- **The archive lexer** (`lexer.cpp`) is the other flat loop on the import
  path. It is not a kernel candidate, because it works one token at a time
  and a token is a number or a short run of spaces. A kernel call per token
  cannot pay for itself. What would pay is classifying 64 bytes at a time
  into bitmaps of delimiters for the whole buffer, as simdjson does. That is
  a rewrite of the lexer, not a kernel added to it.
- **The rasteriser's pixel loop and the plan view's point-cloud splat** are
  measured candidates in files other work owns now. Both would be kernel
  files under the rules above.
- **Surfacing the level.** A command-line or About line saying which level
  is in force would let a person see it; today only `simdSelection()` and a
  refused override's stderr line do.

### The 3D scene build (`scene.cpp`, 2026-09-24)

`cad::SceneBuilder` rebuilds the 3D view's draw lists on every document
change, so its time is paid on every edit with a 3D view open. It was
profiled stage by stage (temporary timers, not committed) on the owner's three
archives, before any change:

| stage, ms per build | Test 4 with Tin (229k-triangle TIN, 7,820 entities) | plot_PW (8 surfaces) | trimeshes complex (266 meshes, 126k faces) |
|---|---|---|---|
| TIN normals | 1.43 | 0.65 | - |
| TIN vertex colour and hillshade | 3.66 | 2.03 | - |
| TIN triangles | 0.71 | 0.36 | - |
| TIN edges, and their reversal | - | 4.96 + 0.82 | - |
| meshes | - | - | 8.88 |
| bounds of terrain and edges | 2.84 | 2.68 | 1.52 |
| the drawing (draping, dashing, styles) | 9.64 | 6.30 | 1.00 |
| bounds again, with the drawing | 3.82 | 3.08 | 1.55 |

What changed:

- **Kernels** (`src/katana_cad/simd/scene_avx2.cpp`): the lift, the summed
  per-vertex TIN normals (one triangle per step, the three corner sums in
  order, so a repeated corner adds twice as the loop did), the per-vertex
  ramp colour and hillshade, per-face mesh shading, and the edge fade.
  `std::hypot` of three is libstdc++'s `__hypot3` (largest magnitude, three
  quotients, a square root), written out lane by lane. The ramp picks its
  stop with a permute of one register per table row; the first version's
  gathers were slower. The fade works in 16-bit integers: the scalar blend
  moves a channel by a whole number of eighths, so
  `lo + (hi - lo) * e/8 + 0.5` is exact in a double and truncating it is
  `(8 lo + (hi - lo) e + 4) >> 3`. A test runs all 65,536 channel pairs at
  every eighth against the scalar blend.
- **Mesh corner order.** The scalar code added a face's three vertices as
  the arguments of `addTriangle`, so their order was the compiler's: GCC
  evaluates them right to left, c first. That is now written out as three
  statements, so the lists are the same as before on GCC and no longer
  depend on the compiler, and the kernel writes c, b, a to match.
- **Allocation.** Lists are sized once and written in place; a per-surface
  `reserve` of exactly one surface's worth (which copies the list once per
  surface) is replaced by geometric growth; an ordinary edge's ink is shaded
  once per vertex, not once per edge end.
- **Bounds.** `SceneLayers::terrainBounds` keeps the terrain's box, so a
  rebuild of the drawing alone does not walk the terrain again. The walk
  visits each vertex once rather than once per primitive using it, and falls
  back to `DrawList::bounds()` when an extreme is a zero, the one case where
  the visiting order decides the result (which zero's sign).

Dispatch thresholds, from a probe build with every minimum at 1
(`BM_SceneKernelBreakEven*`, `BM_SceneFadeEdges/1-4`): a surface of 4 to 25
vertices was within the noise either way, so surfaces keep the scalar loop
below 16 vertices; a mesh of 2 faces (0.19 against 0.49-0.74 us) and a fade of
10 edge vertices (0.03-0.05 against 0.15-0.30 us) were already quicker by
kernel, so their minimums are 2 faces and 8 vertices.

Measured with `tools/compare_benchmarks.py --alternate 4
"BM_Scene(TerrainShaded|TerrainEdges|Meshes|Archive|FadeEdges/.*/128)"
main=... new=... new_again=...` (`benchmarks/bench_scene.cpp`; the
archives through `KATANA_BENCH_SCENE_12DA`). `main` is the benchmark file
built against main, where both members time the old code; `new again` is a
byte-copy of `new`, the A/A control. Other builds ran throughout, so the
medians wandered by up to 2x; the table gives the minimum of 12 samples, ms:

| benchmark | main | new, scalar | new, avx2 | new again, avx2 (A/A) | main / new avx2 |
|---|---|---|---|---|---|
| `ArchiveBuild/0` Test 4 with Tin, whole first build | 22.82 | 17.57 | 16.77 | 15.71 | 1.36x |
| `ArchiveBuild/1` plot_PW | 19.65 | 15.94 | 15.66 | 13.18 | 1.25x |
| `ArchiveBuild/2` trimeshes complex | 14.01 | 11.65 | 5.21 | 5.39 | 2.7x |
| `ArchiveTerrain/0` | 8.14 | 6.08 | 3.74 | 4.37 | 2.2x |
| `ArchiveTerrain/1` | 10.22 | 7.98 | 6.92 | 7.54 | 1.5x |
| `ArchiveTerrain/2` | 9.24 | 10.01 | 3.71 | 4.41 | 2.5x |
| `ArchiveEntities/0` (drawing only: terrain bounds now kept) | 12.95 | 9.52 | 10.64 | 12.11 | 1.2x, inside the A/A spread |
| `TerrainShaded/512` (524k triangles) | 21.77 | 16.03 | 10.70 | 10.49 | 2.0x |
| `TerrainEdges/128` (49k edges) | 2.54 | 2.20 | 1.78 | 1.87 | 1.4x |
| `Meshes` (2,000 meshes, 120k faces) | 10.48 | 10.47 | 3.98 | 4.59 | 2.6x |
| `FadeEdges/128` (98,816 edge vertices recoloured) | - | 0.51 | 0.02 | 0.03 | 25x over scalar |

The A/A pair differs by up to 19% (plot_PW's build), so only the ratios well
outside that are results: the terrain of every archive, meshes and the fade.
The draw lists are the same to the bit at both levels and as main's: each
benchmark's digest counter (FNV-1a over every list) is equal in all three
binaries, and the `SceneKernels` tests compare the lists element by element.

Not done:

- **The drawing** is now the largest stage on the survey archives (about
  10 ms of Test 4's build). It is draping, dashing and style resolution per
  entity: branches and small strings, not arrays, so no kernel. It is the
  next thing to profile.
- **Surfaces and meshes on the TaskPool.** Not attempted. After the kernels, a single
  dense TIN (Test 4) cannot be split without changing the order its normals
  are summed in, and the one archive with several surfaces (plot_PW) spends
  at most about 5 ms in them before bounds and edge reversal; spreading 8
  surfaces of unequal size over the pool would save at most about 4 ms of a
  16 ms build, for a size pass and slice writes. The terrain of 126k mesh
  faces is under 4 ms in all. Worth doing only with the drawing, which is the
  larger half.
