# Performance: what was measured, and what it decided

The targets are `docs/architecture.md`, "Performance targets"; the rules are
Rules 5 and 6 there: measure in Release, record both numbers, and never claim
a speed-up that was not measured. This file is where the numbers live, and
how a comparison is made is `docs/testing.md`, "Benchmarks".

**Every figure below is the C++ build's.** They were measured in September
2026, when Katana was C++, on the owner's machine (Windows 11, an i7-1270P
with 16 logical cores, GCC 16.2 in MSYS2 UCRT64, `-O3 -DNDEBUG
-ffp-contract=off -fno-fast-math`), with Google Benchmark programs and a
script that alternated the builds. Both went with the C++ on 2026-11-15
(`docs/rust.md`, "The C++ is removed"). **The Rust has no benchmarks yet and
has not been measured against any figure here** ("Not done"). The figures are
kept because the Rust ports the same algorithms, culls, caches and kernels,
bit for bit where the tests say so: each records why the code is shaped as it
is and what a Rust benchmark should find again. None is a claim about the
Rust's speed. A benchmark name below (`BM_...`) is the C++ program's.

## How a figure was made

Two binaries built from the same tree, differing only in the code under
test, run in alternating rounds, with a byte-identical copy of one of them as
an A/A control, and the minimum and median of every cell reported; only a
difference outside the A/A spread is a result. The rule came from failures:
an attempt to compare two language standards gave a uniform one-sided shift
from two runs eight minutes apart on a laptop, and was not reported, and two
agents' own figures in the pass of 2026-09-23 were wrong in opposite
directions until they were measured again centrally.

## Terrain and geometry (2026-09-23)

| C++ benchmark | before | after | |
|---|---|---|---|
| `BM_CombineSurfaces/1` diagonal band over a 200k-point base | 30 984 / 31 002 ms | 248 / 254 ms | **125x** |
| `BM_CombineSurfaces/0` pad over 4% of the base | 217 / 224 ms | 211 / 217 ms | unchanged |
| `BM_ElevationsAt` 100 000 probes | 17.9 / 17.4 ms | 2.52 / 3.44 ms | **5.9x** |
| `BM_TiledTerrainBuildAll` 150k points, 64 tiles | 184 / 185 ms | 35.5 / 33.7 ms | **5.3x** |
| `BM_IsSimple/512` | 3.19 / 3.16 ms | 0.801 / 0.806 ms | **3.9x** |
| `BM_IsSimple/64` | 51.2 / 51.9 us | 11.0 / 11.2 us | **4.6x** |
| `BM_RectangleClosestPoint` 1024 probes | 79.5 / 81.6 us | 28.2 / 28.5 us | **2.8x** |
| `BM_ClipPolygon` 512-vertex subject | 10.25 / 10.31 us | 9.02 / 8.81 us | 1.15x |

`combine_surfaces` (`crates/katana_terrain/src/super_surface.rs`) is the
headline: a super surface of two real surfaces took thirty-one seconds and
then a quarter of one, with the same 402 719 triangles, an equality that
`super_surface::the_indexed_overlap_test_keeps_exactly_the_triangles_the_pairwise_scan_kept`
asserts rather than trusting the timing (Rule 5).

**Why the benchmark has two shapes.** Its first version, an axis-aligned
band, showed no difference at all: the overlap test rejects by the higher
member's bounding box, then accepts on a sample point, and only then scans
the member's vertices, and an axis-aligned band answers nearly every
triangle at the first two stages. A band running diagonally has a box
covering almost the whole site, so nearly every triangle reaches the scan -
the shape a road corridor or a watercourse has. The pad measures that the
cheap path did not regress; the band measures the path that changed.

## Per-frame drawing

Each entity drawn was resolved for display twice a frame (once more inside
`resolve_hatch_pattern`), its layer looked up twice, its dimension style
resolved though only a dimension uses one, and its dash pattern rebuilt.
Now the display and the layer are passed down, the dimension style is
resolved for dimensions only, and dash patterns are cached for one frame,
keyed by linetype and pen width and cleared at each frame's start because a
pattern depends on the view scale. **Kept on purpose:** the bounding-box test
in `draw_entities` that looks redundant against `for_each_candidate`'s: the
spatial index's extents are deliberately wider than the geometry (an arc
offers its centre for snapping), so the painter's own test is the tighter
one. No benchmark covered this path, so no figure is quoted.

## Storage and commands (2026-09-23)

| C++ measurement | before | after | |
|---|---|---|---|
| `BM_ProjectOpenAndLoad/50000` (SQLite, JSON, apply) | 184 / 185 / 184 ms | 170 / 170 / 177 ms | **-7.6%** |
| `CREATE_ENTITIES` execute, 27k entities (allocations) | 330 785 | 128 269 | **-61%** |
| the same, wall time | 56.5 ms | 27.7 ms | **2.0x** |
| open + load + apply, 50k entities (allocations) | 1 375 127 | 1 275 112 | -7.3% |

Allocation counts repeat exactly, so they are the honest measure on a
contended machine. Three copies of every entity were removed, and the Rust
keeps their removal: the change set a command's `validate` built is the one
its `execute` consumes (execute still validates against the live model, so a
stale set is refused, not applied); an undone creation keeps no image of its
entities, since removal returns them; and opening a project moves its
contents into the model rather than copying them (22.6% of the open's
allocations). One copy was measured and **kept**: a builder hands its
captured entities over as a copy, because a transaction validates its first
command twice and a builder that moved them out would give the second call
nothing. Values parsed and dropped within a row are read as borrowed views of
SQLite's buffer (`column_text_view`, `column_blob_span`,
`crates/katana_storage/src/sqlite_database.rs`), valid until the next step.

## The archive reader (2026-09-23)

| synthetic archive | before | after | |
|---|---|---|---|
| 62.8 MB, 100k elements, 400 models | 1916-2000 ms | 408-425 ms | 4.7x |
| 15.1 MB, 2401 elements, 233k-triangle TIN | 79.0 ms | 71.0 ms | -10% |

The agent that did the work measured these, on archives not in the
repository, so they are its figures, not checked centrally. What changed:
the keyword table is asked with the token as written, case folding stopped
allocating, and the model lookup became a case-folded hash map instead of a
linear scan of 20 million comparisons a read. Behaviour was proved
unchanged on every fixture, three synthetic archives, five text encodings
and nine malformed archives, byte for byte.

**Rejected, each by its measurement:** reserving the value vectors in
`read_number_block` (a fixed reserve made the TIN-heavy read slower, 72 to
96-105 ms, since the many small blocks each took the large allocation);
reserving the element vector (about 7% of a read, but the count is unknown
without a second pass and a guessed size wastes 30 MB on a 15 MB file); and
`Importer::tally_for`'s linear scan (`domain_import.rs`), about 1% of an
import, left alone.

## The software rasteriser

**Clipping (2026-09-23).** The clipping was rewritten to fix primitives that
crossed the eye plane being dropped whole; ground grids of 131k and 1.05M
triangles at 1920x1080, framed and with the eye inside, were within the
noise of the old code everywhere (A/A rounds, 18 samples a cell). The first
version tested each triangle's corners against five planes and showed in the
clipping medians, because a TIN vertex is a corner of about six triangles;
the kept version classifies each vertex once, in the parallel transform,
into a five-bit code, and a triangle needs two ORs and an AND.

**Tiles, rows and kernels (2026-09-24),** all in
`crates/katana_render/src/rasterizer.rs` and its kernels
(`crates/katana_render/src/simd/`). None changes a pixel:

- a triangle is left out of the 64 px tiles it cannot touch, tested at each
  tile's corners (a 1 px diagonal line of a 640x480 view: 160 bin entries
  to at most 34);
- a box of 64 pixels or more is cut row by row to the pixels whose centres
  can be inside before the fill;
- the fill shades eight pixels a step with AVX2 (four with NEON), in the
  scalar order, and skips the edge tests where the corner test shows the
  whole box inside;
- the transform takes four vertices a step, and projects each vertex once.

**Why skipping cannot change the frame.** Coverage is decided in float; the
skips are decided on the exact edge functions in double with a margin
M = 256 u (R^2 + 1), u = 2^-24 and R the largest vertex-to-pixel distance,
against a float error bounded at about 55 u R^2 (the derivation is at
"conservative coverage" in the source). So a pixel is skipped only where the
float test would reject it. `crates/katana_render/tests/rasterizer_simd.rs`
holds every frame, colour and depth, equal at each level and on 0, 1, 3 and
7 workers.

**The cutoff is measured.** With none, the framed 1.05M grid (0.27 pixels a
triangle) was 23% slower; a cutoff of 16 cost the 131k grid 4%; at 64 no
dense frame moved outside the A/A spread. Isolated and serial, the kernel
wins from about 8 px legs (33.9 against 76.3 ms) and is 5.8x faster at
256 px. Two regressions of the first version were found by toggling each
change in one binary and fixed: a one-tile triangle binned through a second
box flooring (four real `floorf` calls a triangle on baseline x86-64), and a
bounded fill inlined into the loop every small box takes.

| C++, ms, min / median | main | main (A/A) | AVX2 | scalar |
|---|---|---|---|---|
| a 229k-triangle survey archive | 19.16 / 21.02 | 18.77 / 20.56 | 17.30 / 18.78 | 18.14 / 19.58 |
| a plot archive of 8 surfaces | 19.15 / 19.74 | 18.59 / 20.23 | 15.26 / 16.64 | 15.81 / 16.16 |
| `RenderGroundWithin/256` | 9.09 / 9.38 | 8.84 / 9.44 | 3.89 / 4.12 | 7.86 / 8.17 |
| `RenderGroundWithin/724` | 15.35 / 16.19 | 15.59 / 16.36 | 10.21 / 10.55 | 13.83 / 14.25 |
| `RenderGroundFramed/724` | 20.96 / 21.75 | 21.57 / 22.66 | 21.56 / 22.62 | 20.93 / 22.63 |

The eye inside the ground is 2.3x faster at 131k triangles; the archive
frames 10% and 20%, most of the second at the scalar level too, since it
comes from binning long strings only where they are drawn; dense framed
grids are unchanged within the 2-5% A/A spread. What did not pay: row bounds
without the kernel (kept, as what the kernel runs on), the transform kernel
outside the survey archive (12% there), and anything for dense TINs, whose
frame is capped by setup and binning.

## The 3D scene build (2026-09-24)

`katana_cad::scene::SceneBuilder` rebuilds the 3D view's draw lists on every
document change. Profiled stage by stage on three of the owner's archives,
the TIN's vertex colour and hillshade, the edges and their reversal, the
meshes, two walks for the bounds and the drawing itself were the costs.
What changed, and the Rust keeps:

- **Kernels** (`crates/katana_cad/src/simd/`): the lift, the summed
  per-vertex normals (one triangle a step, the corner sums in the scalar
  order), the ramp colour and hillshade, per-face mesh shading and the edge
  fade. The three-argument `hypot` is written out lane by lane as the scalar
  length computes it. The fade works in 16-bit integers, which is exact
  because the scalar blend moves a channel by whole eighths; a test runs all
  65,536 channel pairs at every eighth.
- **Mesh corner order** written out as three statements, so the lists no
  longer depend on the order a compiler evaluates arguments in.
- **Allocation**: lists sized once, geometric growth in place of a reserve
  per surface, an edge's ink shaded once per vertex.
- **Bounds**: the terrain's box is kept (`SceneLayers::terrain_bounds`), so a
  rebuild of the drawing alone does not walk the terrain again.

Dispatch minimums, from a probe build with every minimum at 1: surfaces keep
the scalar loop below 16 vertices (4 to 25 were within the noise), meshes
take the kernel from 2 faces and the fade from 8 vertices.

| C++, ms, minimum of 12 | main | new, scalar | new, AVX2 | A/A | main / AVX2 |
|---|---|---|---|---|---|
| survey archive, whole first build | 22.82 | 17.57 | 16.77 | 15.71 | 1.36x |
| plot archive | 19.65 | 15.94 | 15.66 | 13.18 | 1.25x |
| mesh archive (266 meshes, 126k faces) | 14.01 | 11.65 | 5.21 | 5.39 | 2.7x |
| `TerrainShaded/512` (524k triangles) | 21.77 | 16.03 | 10.70 | 10.49 | 2.0x |
| `Meshes` (2,000 meshes, 120k faces) | 10.48 | 10.47 | 3.98 | 4.59 | 2.6x |
| `FadeEdges/128` (98,816 edge vertices) | - | 0.51 | 0.02 | 0.03 | 25x over scalar |

The A/A pair differed by up to 19%, so only the terrain, the meshes and the
fade are results. The lists were the same to the bit at both levels and as
main's (a digest over every list, equal in all three binaries).

## Point clouds in the plan view (2026-09-24)

The plan view draws a cloud by writing pixels into an image composited over
the drawing. It draws from a display copy
(`crates/katana_geometry/src/point_splat.rs`), never the stored cloud:

- **The display copy**: float offsets from a double origin in the middle of
  the cloud, x, y and colour as arrays, 12 bytes a point (240 MB more for 20
  million points), built in fixed chunks on the task pool and combined in
  chunk order, so it is the same at every thread count, and kept between
  frames.
- **Tiles** of about 1,024 points; a frame skips a tile whose projected
  bounds cannot reach the image, which is exact because the projection is
  monotone.
- **Bands**: one band of rows per pool thread, each writing only its rows.
- **The kernel** (`project_to_pixels`) projects four points a step, each
  offset widened to double exactly and then the scalar arithmetic in the
  scalar order, 512 points a call, from 4 points.
- **The layer image** is kept and reallocated only on a resize; only the
  rows the cloud reaches are cleared, drawn and composited.
- **A point budget**, `SPLAT_POINT_BUDGET`, 4 million (2.5 points a pixel at
  1600x1000): above it each tile draws the same share, its points ordered
  coarse to fine (the first point of each cell of a 1x1 grid, then 2x2, and
  so on), so the share is spread over the tile.

**Which point wins a pixel (Rule 7).** The later in the display copy: tile
by tile, finer levels over coarser, the file's order within a level. Every
band visits in that order, so the image is the same at every thread count
and SIMD level
(`splat_cloud::the_image_is_the_same_at_every_thread_count_and_simd_level`,
`plan_painter_cloud::a_frame_is_the_same_at_both_simd_levels`). Against the
old all-double splat, a float offset moves a point by at most half a float's
spacing (0.03 mm over 1.5 km), so a point on a pixel edge may move one
pixel: 3 of 200,639 lit pixels on 300,000 points, held to at most 1 in
10,000 by
`plan_painter_cloud::float_offsets_move_a_few_points_on_pixel_edges_by_one_pixel_and_none_further`.
Points whose x or y is not finite are left out of the copy; they could never
be drawn.

| C++, 1600x1000, 16 threads, minimum ms | main, A / A' | after, A / A' | main / after |
|---|---|---|---|
| 2M points, extents | 17.44 / 16.36 | 4.66 / 4.66 | 3.5-3.7x |
| 10M, extents (the budget draws 4M) | 58.47 / 66.41 | 7.30 / 7.22 | 8.0-9.2x |
| 20M, extents (the budget draws 4M) | 122.27 / 116.25 | 8.09 / 8.39 | 14-15x |
| 10M, zoomed in 8x | 126.72 / 109.05 | 3.20 / 3.38 | 32-40x |
| first frame, 20M | 918.21 / 960.40 | 766.30 / 794.79 | 1.2x |

The kernel alone, the level forced in one binary, gave only 1.06x on 10M:
once tiles, threads and the budget have done their work, the frame waits on
scattered writes, and the kernel's 2.5x on the projection is a small part of
it. The first frame builds the copy; built on one thread it cost 1.7-2.4x
main's first frame, and parallel chunks made it 1.2-1.4x faster.

## SIMD kernels

**The policy.** The shipped build is for baseline x86-64 and starts on any
64-bit PC: nothing is compiled for x86-64-v3 (measured in the C++ before
this work: no benchmark moved outside the A/A spread with the flag on the
whole build, because the hot code is structs, maps and branches). A few small
kernels are written by hand, each has a scalar reference that is the code as
it was before, and **every kernel is bit-identical to its reference**,
signed zeros and which values are NaN included (a NaN's payload is not
compared: IEEE leaves it to operand order). That rule is what lets a test, or
a person, switch the level at any moment.

**Dispatch** (`crates/katana_core/src/cpu_features.rs`). The detected level
is AVX2 where the processor reports AVX2 and FMA
(`is_x86_feature_detected!`, which also requires the operating system to
save the wide registers), NEON on every AArch64 target (it is that
architecture's baseline), else scalar; the levels belong one to each
architecture and are not ordered
(`simd_level::avx2_is_refused_on_a_neon_processor_although_it_comes_first_in_the_enumeration`).
`KATANA_SIMD=scalar|avx2|neon` is read once: `scalar` is how a machine with
AVX2 proves the path one without it takes, and a level the processor cannot
run, or a word that names none, is refused with one line on stderr and not
acted on. `set_simd_level` is the in-process form, for tests. Built without
katana_core's `simd-kernels` feature (on by default) only the references
are there. Each kernel is a `#[target_feature(enable = "avx2")]` function,
so AVX2 code exists inside the kernel and nowhere else: the C++ needed
naming rules, object libraries and three object-file checks against an
inline function's AVX2 copy reaching baseline code, and the Rust needs none
of them (`docs/rust.md`, "Porting rules"). No kernel fuses a multiply and an
add; Rust never does unasked.

| kernel | called from | from | scalar reference |
|---|---|---|---|
| UTF-16 to UTF-8, ASCII runs (`crates/katana_core/src/simd/`) | `decode_text`, both byte orders | any length | the unit loop, taking over at the first non-ASCII unit |
| UTF-8 validation | `is_valid_utf8` (`crates/katana_core/src/text_kernels.rs`) | 64 bytes | the byte loop |
| `transform_points` (`crates/katana_geometry/src/point_batch.rs`) | the batch API, no caller in the program yet | 8 points | `transform_point` on each point |
| `bounds_of` | polyline and mesh bounds | `BOUNDS_BATCH_MINIMUM`, 16 points | the `expand()` loop, inline below the minimum |
| `project_to_pixels` (`crates/katana_geometry/src/point_splat.rs`) | the cloud splat | 4 points | the splat's loop |
| the fill and the vertex transform (`crates/katana_render/src/simd/`) | the rasteriser | a 64-pixel box; 16 vertices | the scalar fill and transform |
| the scene kernels (`crates/katana_cad/src/simd/`) | the scene build | 16 vertices, 2 faces, 8 edge vertices | the scalar loops of `scene.rs` |
| `pack_draw_list`'s vertices (`crates/katana_gpu/src/simd/`) | every layer the 3D view hands the GPU | any length | the loop in `gpu_scene.rs` (`docs/gpu.md`, "Packing") |

Each minimum was measured on AVX2 in the C++ (the first guesses were wrong
in both places): the bounds kernel with its call and its zero-sign fix-up
breaks even with the loop at 12-16 points, and the validator needs 64 bytes
because it ends on a whole 32-byte block that begins inside the text (a
padded copy for shorter texts was slower up to about 24 bytes and at most
15 ns faster up to 63).

**Why validation is a whole validator.** A first version handed runs of
ASCII to a kernel and made text dense with accented, Cyrillic or CJK
characters 2-4.5x slower, paying a call for every short run; every placement
of an ASCII check tried cost 10-40% on one of those shapes, because the byte
loop is about a cycle a byte. The validator judges every byte against the
three before it, whatever the language, with the lookup algorithm of
J. Keiser and D. Lemire ("Validating UTF-8 in less than one instruction per
byte", Software: Practice and Experience, 2021), never branching on where a
character ends; the last block is judged again with the one before it, so
there is no padded partial block.

**Three details keep the bounds bit-identical.** MINPD and MAXPD return the
second operand unless the first is strictly less (greater), which with the
new value first is the loop's `min(acc, v)`, NaN passed over as the loop
passes it; when the extreme is zero the loop keeps the first zero it met and
the kernel folds lanes in another order, so the sign is taken from the first
zero in the array; and NaN payloads are not compared. On NEON, min, max and
clamp are a compare and a select in the reference's order, since FMIN, FMAX,
FMINNM and FMAXNM each differ from `b < a ? b : a`.

**The tests** hold every kernel to its reference through the public
function, at each level in one process
(`crates/katana_core/tests/simd_text.rs`,
`crates/katana_geometry/tests/cases/point_batch.rs`,
`crates/katana_render/tests/rasterizer_simd.rs`,
`crates/katana_cad/tests/scene/scene_simd.rs`), with the level switched
under one lock (`crates/katana_cad/tests/support/simd_levels.rs`). A
comparison on a processor with no kernels fails rather than skips: the
scalar path compared with itself does not count as having compared the
kernels. The inputs reach the kernels' edges: every position of a bad byte
across the first three 64-byte steps, all 65,536 byte pairs at each block
edge, 3000 generated UTF-16 inputs, arrays of every length up to 70 with
zeros, NaN and infinities, and hand-worked bounds cases long enough to reach
`BOUNDS_BATCH_MINIMUM`. In the C++, each test was shown to have teeth by
breaking a kernel (the zero fix-up disabled, a lane permute changed, five
rules of the validator dropped); each break failed tests.

| C++, ms, min / median | main | new, scalar | new, AVX2 | A/A, AVX2 |
|---|---|---|---|---|
| `DecodeUtf16Archive` (8 MB) | 13.95 / 20.92 | 3.32 / 4.52 | 1.43 / 2.24 | 1.41 / 2.30 |
| `IsValidUtf8Archive` (4 MB) | 1.99 / 3.21 | 1.08 / 2.55 | 0.13 / 0.25 | 0.16 / 0.24 |
| `IsValidUtf8Names` (10,000 short names) | 0.09 / 0.19 | 0.09 / 0.29 | 0.12 / 0.28 | 0.09 / 0.27 |
| `BoundsOfPoints2` (65,536 points) | 0.20 / 0.26 | 0.22 / 0.27 | 0.03 / 0.04 | 0.02 / 0.04 |
| `PolylineBoundingBoxes` (28,000 strings) | 0.61 / 0.88 | 0.65 / 0.95 | 0.68 / 0.99 | 0.62 / 1.55 |

Decoding gained about 4x from the scalar path alone (a whole run narrowed
into the buffer at once, not a character at a time) and AVX2 halved the
rest; on three of the owner's real archives decoding fell from 141-169 ms to
9-12 ms and the second pass, validation, 7-8x. Short names and a drawing's
short strings never reach a kernel and were unchanged.

**NEON.** Every kernel family has a NEON twin. The C++'s were cross-built
for Linux aarch64 and run under qemu (2026-09-26): every kernel equal to its
reference at both levels, and each test shown to reach its kernel by
breaking one lane. The Rust's NEON kernels are written and have not been
compiled, since nothing has been built for an ARM target
(`docs/rust.md`, "Not done"); the release workflow's ARM64 and macOS jobs
are where they first are. The NEON kernel of the scene keeps the
libstdc++ form of `hypot` only, where the C++ chose libc++'s on macOS. No
minimum was ever measured on ARM: each is the AVX2 one, on the reasoning that
the fixed cost it repays is the same.

## Not done

- **No Rust benchmarks.** None of the measurements above has a Rust twin,
  so no claim about the Rust's speed can be made yet, and the first change
  made for speed must first add the benchmark it is judged by, with the A/A
  method of `docs/testing.md`, "Benchmarks". The tables above say what each
  should find. The one Rust benchmark so far is new work, not a twin: the
  meshes' faces in plan (`crates/katana_window/benches/mesh_plan.rs`, a plain
  program with no benchmark library; figures in `docs/plan_view.md`, "Meshes
  in plan").
- **The hand-written SIMD kernels are to be removed.** On 2026-11-16 the
  owner decided that `unsafe` goes wherever it can, the kernels with it,
  their scalar references staying and the speed measured before and after
  (`docs/architecture.md`, "Layering"; `docs/rust.md`, "Not done"). That
  measurement needs the benchmarks above.
- **Nothing was measured on ARM**, and the Rust's NEON kernels have not been
  compiled.
- **Uncovered by any benchmark, then and now:** the per-frame drawing path,
  the scene's drawing stage (draping, dashing and style resolution, about
  10 ms of the survey archive's build, branches and small strings rather
  than arrays, the next thing to profile), and an archive read.
- **Not attempted:** surfaces and meshes of the scene on the task pool (a
  dense TIN cannot be split without changing the order its normals are
  summed in, and the several-surface archive would save at most about 4 ms
  of 16); classifying the archive lexer's input 64 bytes at a time, as
  simdjson does (a rewrite of the lexer, not a kernel added to it); points
  in the 3D view; a point budget chosen from the machine's speed; and saying
  which SIMD level is in force in a command-line or About line.
