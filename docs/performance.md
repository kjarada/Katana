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

### Still open

- No benchmark covers scene building or the per-frame draw path, so the
  viewport work earlier in this file still quotes no figure.
- No benchmark covers a 12da read.
- `Importer::tallyFor` (`domain_import.cpp`) is a linear scan per element, the
  third instance of that shape. About 1% of an import, left alone.
