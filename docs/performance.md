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
