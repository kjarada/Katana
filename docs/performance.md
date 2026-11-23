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
(`docs/rust.md`, "The C++ is removed"). **The Rust has no twin of these
benchmarks and has not been measured against any figure here** ("Not done");
its own, written since, are listed there. The figures are
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

**Safe kernels (2026-11-20).** Every kernel is a safe function, and no
kernel module opts out of the workspace's `unsafe_code` denial. Since Rust
1.86 a `#[target_feature]` function may be safe and is a safe call from one
with the same features (implied ones counting: AVX2 implies AVX, SSE4.2 ...
SSE2), and since 1.87 every intrinsic of the standard library's arch module that takes no pointer is a
safe call inside such a function; both checked against the sources (rustc's
unsafety check of a call into a target-feature function, and stdarch's
declarations, where only the loads and stores - `_mm256_loadu_pd`,
`_mm256_maskstore_ps`, `vld1q_f64`, `vld2q_u8`, `vst4q_f32`, ... - are still
`unsafe fn`). So the kernels take slices and never a pointer: a register is
built from the elements with a `set` intrinsic and comes back as an array
lane by lane, through `crates/katana_core/src/simd/x86_lanes.rs` and
`crates/katana_core/src/simd/neon_lanes.rs`, shared by every crate's
kernels. Consecutive elements so built fold into the one
unaligned load the pointer form was, and lanes copied out whole into one
store; no lane is converted, so the bits are the bits. Where a kernel wrote
through a masked store, the fill on AVX2 now writes a block the tile's row
holds whole as a blend of the new pixels and the old (the other lanes given
back unchanged, as NEON's fill already did), and a block past the row's end
lane by lane; the text narrowing writes into a `[u8]` through one slot
trait instead of a raw pointer. What stays unsafe:
in each crate's dispatch module (`text_kernels.rs`, the geometry, render,
scene and packing `simd` entries) the call into a kernel, one line an entry
under the one proof that the level is one the processor runs; the baseline
facade's two entries into SSE2 or NEON, compiled only where the target has
them. Until 2026-11-22 `append_ascii_utf16` also reached the String's spare
capacity through its `Vec` and set its length over what was written, since
a safe append validates and copies every byte a second time; it now narrows
a block at a time into an array of its own and appends that as text
(`docs/rust.md`, the third pass), at the price measured below ("The safe
UTF-16 append"). Rejected:
`transmute` between an array and a register (unsafe), `std::simd` (not
stable), a crate that wraps the conversions (`safe_arch`, `bytemuck`: a new
dependency for a dozen lines), and removing the kernels for their scalar
references, which the 2026-11-16 decision proposed: the references give the
same bits but forfeit the speed in the table below (validation 8x,
decoding 2x), and the safe form keeps it. A single unsafe call a crate was
rejected too: a target-feature function implements no `Fn` trait and is no
safe fn pointer, so one generic entry would need a job enum a crate, more
code around the same one line of reasoning.

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

## Changes of 2026-11-21, measured

Seven changes made for speed, each found by reading the code and then measured.
Each is one change, with its own benchmark cells, its own test that the fast
path gives what the slow one gave, and its own hunks. Every benchmark is a
plain program in the house form (`crates/katana_render/benches/rasterise.rs`),
on generated data, using only members the base build has, so one source
measures both builds; `KATANA_BENCH_ROUNDS` sets the rounds.

**How they were measured** (2026-11-21, the owner's Windows machine, MSYS2
UCRT64, Release, on mains power with nothing else building): the base is the
commit before the changes with only the benchmark files added, built in a
tree of its own. By the method of `docs/testing.md`, "Benchmarks", each
program ran base, changed and base again (the A/A control), four times over
with the order reversed every time, 15 samples a cell a run. A table gives
the least of the four minimums and the median of the four medians, in
milliseconds, as min / median. The A/A column is the same base program under
a second name: where it differs from the base, that is the machine. All seven
were kept: each is outside its A/A spread.

**The changes of 2026-11-22** - every subsection from "The index and the
tables a box is drawn from" on - were measured the same way on 2026-11-23,
on the same machine: the base is the commit before them with the benchmark
files added, and the window's two programs were also built with one change
at a time on that base, so each change has a figure of its own. All were
kept. One of them is not a gain and was kept for another reason, with its
price ("The safe UTF-16 append").

### The plan view's frames

egui paints a frame for every pointer move anywhere in the window, and
`PlanView::ui` (`crates/katana_window/src/viewport_widget.rs`) answered each
one in full although the kept drawing (the `DrawingKey`) was still good: a
copy of the drawing, the furniture over it, **the picture converted pixel by
pixel into egui's colours, and the whole of it handed to the texture** - a
second pass over every pixel and an upload of the whole view to the graphics
card, for a picture the texture already held. Two changes, apart:

1. **The texture is given the picture only when the picture changed.** A
   paint compares the picture it made with the one before, whole, and counts
   a revision when they differ; egui's side converts and hands over only a
   revision it has not shown. **Rejected:** a key over what the furniture is
   drawn from (the tool's feedback, the grips, the snap, the typed line, the
   selection box, the hint), which would also save the copy and the furniture:
   one input missed and the screen shows a stale picture, where a comparison
   of the pixels cannot be wrong. The comparison costs a pass over the two
   buffers when they are equal and stops at the first difference when they
   are not, which for a pan is in the first rows; a frame that changes only
   the prompt band at the bottom pays nearly the whole pass for nothing.
   (The key was made afterwards, for the frames that can have one, with the
   comparison kept as the guard of every frame that is painted: "The plan
   view's furniture on an unchanged frame", below.)
   Held by `view_ui::the_texture_is_given_the_picture_when_it_changes_and_only_then`
   (every picture handed over is the view's picture, a changed picture is
   always handed over, an unchanged one never).
2. **The picture is laid into a kept buffer** (`Image::copy_from`,
   `crates/katana_window/src/canvas/image.rs`): the view keeps the picture
   before the last and paints the next into it, where `clone` allocated a
   whole frame each time and the system handed it over as untouched pages.
   The price is one more frame of memory a plan view (6.4 MB at 1600 x 1000,
   31 MB at 3840 x 2000). Held by
   `view_ui::a_picture_laid_into_a_kept_buffer_is_the_picture_a_new_view_paints`.

`cargo bench --locked -p katana_window --bench plan_frames`
(`crates/katana_window/benches/plan_frames.rs`): 100 000 lines, a 1600 x 1000
view through egui headless. Headless, a texture handed over is only queued,
so the upload itself is not in these times: the desktop gains more than the
table says.

| ms, min / median | base | base (A/A) | changes 1 and 2 | changed / base |
|---|---|---|---|---|
| `idle_frames` (30 frames, nothing changed) | 101.39 / 108.37 | 101.24 / 108.34 | 52.05 / 56.93 | 0.51 / 0.53 |
| `pan_frames` (10 frames, each repainted) | 108.64 / 115.10 | 109.39 / 116.22 | 99.91 / 107.89 | 0.92 / 0.94 |
| `uploads=` of `idle_frames` | 30 | 30 | 0 | |

An idle frame costs half what it did (3.4 ms to 1.7 ms a frame), and a pan,
which the comparison could only have slowed, is 6 to 8 % quicker: the kept
buffer's gain is larger than the comparison's cost there. **The two changes
were measured together, not apart** on that day. On 2026-11-23 each was
built without the other, and with neither, from the commit that has both
(the same `plan_frames`, the same method):

| ms, min / median | neither | kept texture alone | kept buffer alone | both |
|---|---|---|---|---|
| `idle_frames` | 102.73 / 111.13 | 78.80 / 91.95 | 77.56 / 84.43 | 55.32 / 61.43 |
| `pan_frames` | 105.39 / 118.24 | 105.92 / 116.45 | 94.58 / 107.36 | 102.41 / 112.04 |
| `uploads=` of `idle_frames` | 30 | 0 | 30 | 0 |

Each takes about a quarter off an idle frame by itself (0.77 / 0.83 and
0.76 / 0.76 of neither; A/A 0.99 / 0.94) and the two add up (0.54 / 0.55).
On a pan the kept texture gives nothing, as it should - every frame is a new
picture - and the kept buffer is the whole of the gain there (0.90 / 0.91).

### The labels' walk

A repaint of a drawing that has any label style looked for its labels by
listing every entity's id and looking each one up again (`draw_labels`,
`crates/katana_window/src/plan_painter.rs`): a tree lookup for every entity
of the drawing on every pan and zoom, whatever the view showed, where the
entities drawn come through the spatial index. It is now one pass over the
entities in the same order (`EntityDatabase::iter`,
`crates/katana_entity/src/entity_database.rs`; `for_each` cannot be used, a
label is kept past the visit). **Not done:** finding the labels without
walking the drawing at all - a label is placed where its target is, not where
it was made, so the index does not answer it; a list of the label ids kept
by the database would. Held by
`entity_walk::the_walk_hands_out_what_each_id_finds_in_ascending_id_order`
and the label cases of the plan painter's tests, which place the same labels.

`cargo bench --locked -p katana_window --bench plan_frames`: a hundredth of
the 100 000 lines' area in view, without and with one label style.

| ms, min / median | base | changed |
|---|---|---|
| `paint_zoomed` (no label style: the control, which must not move) | 10.05 / 10.51 | 9.91 / 10.44 |
| `paint_zoomed_label_style` | 15.19 / 17.17 | 11.30 / 12.08 |

Changed / base 0.74 on the minimum and 0.70 on the median with a label style
(A/A 0.98 / 0.99), and 0.99 / 0.99 without one (A/A 1.00 / 1.00): the control
did not move. Both builds drew the same 1 131 entities.

### Layer counts

`EntityDatabase::count_on_layer` walked every entity. The layers panel asks
it once for each layer row on every frame it is shown, and the layer manager
and LAYER LIST once a layer: a drawing of 100 000 entities on 200 layers was
20 million comparisons of layer names a frame. The database now keeps a count
a layer, moved by `add`, `insert`, `replace`, `remove`, `clear` and
`adopt_contents` - the only members that change the entities - so the answer
is one lookup; `ids_on_layer` still walks, and is what the test holds the
counts to after every step of 4 000 generated changes
(`layer_counts::every_layer_count_is_the_length_of_the_walk_after_every_kind_of_change`;
a refused change and a copy of the database have a case each). The price is
a hash lookup an entity added, removed or moved to another layer, which
`add_entities`, `replace_onto_another_layer` and `remove_entities` measure.
**Rejected:** caching the counts in the panel, keyed on the model's revision:
three callers would each keep one, and two of them are another agent's files.

`cargo bench --locked -p katana_entity --bench entity_database`
(`crates/katana_entity/benches/entity_database.rs`).

| ms, min / median | base | base (A/A) | changed |
|---|---|---|---|
| `count_every_layer` (200 layers, 100 000 entities) | 339.00 / 385.67 | 351.45 / 399.71 | 0.003 / 0.003 |
| `add_entities` (100 000) | 22.33 / 23.47 | 22.34 / 23.88 | 24.32 / 25.94 |
| `replace_onto_another_layer` (10 000) | 9.75 / 10.67 | 9.81 / 10.75 | 10.29 / 11.19 |
| `remove_entities` (10 000) | 9.24 / 9.98 | 9.57 / 10.20 | 9.61 / 10.36 |

The counts of every layer went from a third of a second to three
microseconds. **The price is real and was kept:** adding is 9 to 11 % slower
(A/A 1.00 / 1.02), which is 2.4 ms for 100 000 entities, once, at a load or
an import; moving to another layer is 5 % slower (A/A 1.01); removing is
inside the A/A spread (1.04 against 1.02 to 1.04). A frame of the layers
panel repays a load's whole price a hundred times over.

### The spatial index's hashing

`SpatialIndex` (`crates/katana_geometry/src/spatial_index.rs`) keeps two hash
maps keyed by a 64-bit number - a box by its id, a cell's ids by the cell -
and hashed both with the standard library's keyed SipHash. A rebuild hashes
every box once and every cell it covers; a query hashes every cell it covers
and then every id it finds, to fetch the box it compares. The keys are now
mixed by SplitMix64's finaliser (the source is cited at the hasher), a
bijection of the 64 bits, a few instructions where SipHash is some tens.
SipHash defends a table against chosen keys, which a drawing's ids and cells
are not. **Nothing the index answers depends on the hash:** a query's ids are
sorted before they are returned and a cell's are kept in the order filed, so
the answers are the same to the id - held by the existing cases against a
scan (`crates/katana_geometry/tests/cases/spatial_index.rs`) and, on keys a
weak hash would fold together (consecutive ids, ids differing only above bit
32, cells either side of zero), by
`spatial_index_keys::a_rebuilt_index_answers_as_a_scan_of_every_box_does` and
its two neighbours. **Rejected:** the identity as the hash (consecutive ids
land in consecutive buckets, but a cell key's low half is its north number
alone, and the table's seven tag bits would be the top of the east number:
long probe runs on any real site); keeping the box beside the id in each cell
to save the second lookup (five times the memory of a cell's entry, for the
lookup this makes cheap).

`cargo bench --locked -p katana_geometry --bench spatial_index`
(`crates/katana_geometry/benches/spatial_index.rs`); the `ids=` of a cell
must be the same in both builds.

| ms, min / median | base | base (A/A) | changed |
|---|---|---|---|
| `query_view` (200 windows of 1 % of the site) | 18.86 / 24.58 | 18.81 / 20.82 | 14.84 / 16.84 |
| `query_cursor` (20 000 windows at the cursor) | 5.45 / 7.23 | 5.39 / 6.25 | 4.50 / 5.34 |
| `rebuild` (100 000 boxes) | 16.99 / 19.20 | 17.11 / 17.89 | 14.64 / 15.55 |
| `move_boxes` (10 000 moved and moved back) | 8.00 / 8.83 | 7.97 / 8.64 | 6.72 / 7.42 |

**Read the minimums here.** The medians of this program were noisy on the
day - the base's own second copy is 0.85 of the first on `query_view`'s
median - so a median's ratio is no result; the minimums agree with
themselves (A/A 0.99 to 1.01 in every cell). On the minimums changed / base
is 0.79 for `query_view`, 0.83 for `query_cursor`, 0.86 for `rebuild` and
0.84 for `move_boxes`. The `ids=` of every cell is the same in both builds
(207 400 and 20 585).

### Checking a project's contents

`validate_contents` (`crates/katana_storage/src/project_store.rs`) runs before
a project's contents are applied to the model and before they are saved. For
every entity it built the text "id=N" that only a refusal prints - the same
waste the load's own comment says it avoids - and put the id into an ordered
set, a tree node for every few entities, to refuse a repeat. The text is now
built when a check fails, and the ids are a sorted vector while they ascend,
which is the order a project's entities always come in (the model walks them
so, the load reads them `ORDER BY id`); an id that does not ascend is told
apart exactly, through a set of the few that do not. What is refused, and the
words of the refusal, are the same in every order: held against an ordered
set filled one id at a time, over hand-picked and 600 generated lists
(`contents_checks::generated_orders_with_and_without_repeats_are_judged_as_a_set_judges_them`
and its neighbours, which also hold the relationship check and the two
refusals that print the id).

`cargo bench --locked -p katana_storage --bench apply_contents`
(`crates/katana_storage/benches/apply_contents.rs`): 100 000 entities, no
database opened. `refuse_last` is the check alone; the other two are the
check and the model.

| ms, min / median | base | base (A/A) | changed |
|---|---|---|---|
| `apply_consuming` (what an open calls) | 41.12 / 42.85 | 41.15 / 42.36 | 29.71 / 31.25 |
| `apply_copying` | 41.88 / 43.94 | 42.76 / 44.29 | 31.19 / 32.49 |
| `refuse_last` (the check alone) | 17.39 / 18.39 | 17.65 / 18.46 | 4.67 / 4.91 |

Changed / base 0.72 / 0.73 for what an open calls and 0.27 / 0.27 for the
check alone, against an A/A of 1.00 / 0.99 and 1.01 / 1.00.

### Empty property maps

Found on 2026-11-21 and left then; done the same day, after the rest. A
stored entity's two property maps are read from JSON on every load
(`properties_from_json`, `crates/katana_entity/src/serialization.rs`), and
nearly every one is the text `{}`, which was parsed into a tree to learn
that it has no members: 200 000 parses for 100 000 entities. That exact
text - what `properties_to_json` writes for an empty map - is now answered
before the parser; any other spelling of an empty object still goes through
it and gives the same map
(`the_empty_object_reads_as_no_properties_however_it_is_spelt`).

`cargo bench --locked -p katana_entity --bench entity_database`, measured as
the others were, the base being the commit before with the cell added.

| ms, min / median | base | base (A/A) | changed |
|---|---|---|---|
| `read_empty_property_maps` (200 000 reads) | 21.68 / 22.31 | 21.58 / 22.11 | 0.585 / 0.629 |

Changed / base 0.03 on both, A/A 1.00 / 0.99: 21 ms off the load of a
drawing of 100 000 entities. **Not measured:** a whole load from a real
database, where this is one part among the reading of the rows; the
benchmark for it was written afterwards ("A project saved and loaded",
below).

### The index and the tables a box is drawn from

Not a change for speed: what had to be right before the next one could be
made. The Document's spatial index stores a box an entity, when the entity
changes (`Document::apply_to_spatial_index`). One kind's box is not its
geometry's alone: a dimension's is what it draws, by the dimension style its
layer names (`query_extents`, `crates/katana_cad/src/spatial_query.rs`). An
edit of that style, or of the layer's choice of one, moves the box of every
such dimension and reports no entity, and the index went on holding the old
one: **a dimension restyled larger was culled from a view its label now
reached into, and could not be found there**, until something touched it or
the project was opened again. Every other kind was read for the same fault
and has none: a text's, a label's, a leader's and a symbol point's box is
made from the geometry alone (`bounding_box` of the entity layer takes
nothing else), whatever style draws them.

Two changes, both in `crates/katana_cad/src/document.rs`:

1. **A step that changed the layers or the dimension styles stores every
   dimension's box again**, after the entity changes it reported. The tables'
   revisions say that a table changed (`tables_changed`, which the
   notification already asked), not where, so it is every dimension: one pass
   over the entities and one drawing of each dimension, for a step that
   touches either table - a layer switched off included. Which kinds follow a
   table is said in one place beside the box itself (`extent_follows_tables`).
   **Rejected:** comparing the tables before and after to find the layers and
   styles that moved, which would save the pass on a layer switched off and
   needs a copy of both tables a step; storing no box for a dimension and
   drawing it at every query, which is what the second test of the broad
   phase did for every kind.
2. **A step that ran and failed has the index built again.** The index
   follows the changes a step publishes, and a failed step publishes none
   (`CommandStack::execute` drops them). A command's contract is that a
   failure leaves the model as it was, and one that broke it - or an undo
   that stopped half way - left the index describing a model that was no
   longer there. A refusal by `validate`, which cannot change the model and
   is what nearly every refused command is, costs nothing; a failure after
   that costs a rebuild. **Rejected:** trusting the contract, which every
   command in the tree keeps as far as was read, because the next change
   makes the broad phase believe the index outright.

Held by `index_tables::a_dimension_restyled_larger_is_found_where_its_label_now_reaches`
and its neighbours (the undo and the redo of a style edit, a layer given
another style and taken off it, a command that fails after moving a line),
whose windows are worked out by hand from the dimension's construction, and
by `index_tables::the_index_answers_as_a_scan_through_generated_edits_of_entities_and_tables`:
96 steps of entity and table edits over 60 dimensions and 120 lines, after
each of which the index, the broad phase through it and the broad phase's
scan are compared with a scan of every box as the model stands - for random
windows, and for the windows that meet a restyled box only as it was or only
as it is. Without the first change those tests fail; before it nothing
compared the index with the model after a table edit.

**What it costs** is not measured: a layer or dimension-style edit of a
drawing with many dimensions (the pass, and a drawing of each); no cell of
`drawing_passes` times it.

### The broad phase's second extent

Found on 2026-11-21 and left then, because it needed the section above
first. `for_each_candidate` (`crates/katana_cad/src/spatial_query.rs`) is
where a repaint, a pick, a snap, a box selection and the sheet checks narrow a
query to the entities that could answer it. Through the index it asked
`SpatialIndex::query_into`, which compares the query with each stored box,
and then **worked every candidate's box out a second time and compared
again**: a pass over the vertices of every polyline in view, and a whole
drawing of every dimension (`build_dimension`), on every frame. The stored
box is that box - the index is built from `query_extents` and nothing else -
so the second test could only disagree where the index was stale, and there
it hid half the fault (what had left the query's reach was dropped, what had
entered it was still missed). With the index now kept to the model through
table edits and failed steps, the candidates the index answers with are
visited as they are; an id the model no longer has is still passed over.

**Why it cannot change an answer.** The index's test and the dropped one are
the same call, `Box2::intersects`, on the same two boxes whenever the stored
box is what `query_extents` gives for the model as it stands. That holds
after every way the Document's model changes, each read: a step that
succeeds (every member of `EntityDatabase` that changes an entity reports it,
and a change of the two tables a box is drawn from is seen by their
revisions, which only a table's own members move); a step that fails (the
index is built again unless the failure was a refusal before the command
ran); a new and an opened drawing (built again; an open that fails has not
touched the model, which `apply_to_model_consuming` builds aside). Every
caller in the tree passes the Document's index with the Document's model, or
an index built from the model it is asked with. **Not covered, and no
different than before:** a query box or an entity box with an infinite
coordinate, which the index does not hold and the scan does compare - the
indexed and the scanned paths already differed there, with or without the
second test.

**Rejected:** keeping the second test for dimensions only, as the one kind
whose box is not its geometry's - it would keep a drawing of every dimension
in view on every frame to guard a case the section above removes.

Held by the tests of the section above, which compare the broad phase
through the index with a scan of every entity's box as the model stands:
`index_tables::undoing_and_redoing_a_dimension_style_edit_moves_the_boxes_with_it`
and the layer taken off its style are the cases a believed stale box would
fail (a dimension handed out where it no longer reaches), and the generated
steps ask at least 60 windows that only a box before a restyle meets, and
count the random windows answered through the index rather than by the scan.
The tests that were there already run every pick, snap and box selection
both ways (`indexed_queries::picking_gives_the_same_answer_with_and_without_the_index`
and its neighbours).

`cargo bench --locked -p katana_cad --bench drawing_passes`
(`crates/katana_cad/benches/drawing_passes.rs`): a Document of 100 000
entities - one in fifty a dimension, the rest points, lines and polylines of
12 vertices - asked through its index. The `count=` of a cell, the
candidates visited, must be the same in both builds.

| ms, min / median | base | base (A/A) | changed |
|---|---|---|---|
| `candidates_view` (200 windows of 1 % of the drawing) | 34.29 / 38.60 | 33.88 / 39.83 | 21.35 / 26.17 |
| `candidates_cursor` (20 000 windows at the cursor) | 15.00 / 18.20 | | 9.74 / 11.76 |

Changed / base 0.62 / 0.68 for a view's windows and 0.65 / 0.65 at the
cursor (A/A 0.99 / 1.03); the same 207 990 and 40 867 candidates in both
builds.

### The verbs' walks

Found on 2026-11-21 and left then. Eight passes over the whole drawing were
written as the labels' walk had been: a list of every id, then a lookup of
each - a tree lookup an entity where one pass over the map gives the same
entities in the same order. Each now walks `EntityDatabase::iter`:

- linework (`crates/katana_cad/src/linework.rs`), three: the points of a run
  over the whole drawing, every point by its number for the joins, and the
  coded lines the drawing already holds;
- survey coding (`survey_coding.rs` beside it): the entities of a run over
  the whole drawing, looked up once where they were looked up once for every
  candidate name of the code property and once more to read the code;
- the code census (`code_table.rs`): `code_census` of the drawing, which
  handed every id to the census of a list;
- the utility data (`utilities/utility_data.rs`): every point and run by its
  line;
- the archive export (`crates/katana_archive12d/src/domain_export.rs`) and
  the IFC export (`crates/katana_ifc/src/drawing.rs`): the entities written.

**None of them changes the drawing while it walks** - each holds the model
by shared reference, which is why the walk can be borrowed - and none was
left as a list for that reason. Where a verb takes either the whole drawing
or the entities named, only the whole drawing's side changed: the named side
still looks its ids up, in the order the verb always took them (sorted for
linework, as given for coding and the census). `find_code_property` keeps
its signature and stops at the first entity that carries a candidate, as
before; it and the walks share one search (`code_property_among`).
**Rejected:** `for_each`, which lends each entity for the visit only, where
all of these keep what they walk.

Held by the verbs' existing tests, and for the order of what they give by
`entity_walks::linework_of_the_whole_drawing_is_the_linework_of_every_id_named`
and its neighbours for coding and the census - 2 000 generated entities with
gaps in their ids, the whole drawing's answer compared record for record with
the unchanged named path's, and the drawings the two commands leave compared
entity for entity -
`entity_walks::a_line_the_drawing_holds_twice_is_reported_as_the_one_of_the_lower_id`,
`utility_data::of_two_points_whose_line_was_deleted_the_one_of_the_lower_id_is_refused`,
and a case of each export
(`domain_export::entities_are_written_in_the_order_of_their_ids_whatever_ids_are_missing`,
and the same name in `ifc_export_drawing`).

`cargo bench --locked -p katana_cad --bench drawing_passes`: the Document of
100 000 entities, of which 1 000 are coded survey points in 250 strings and
none carries utility data, so that a cell is mostly its walk: on a drawing
that is all survey the walk is a smaller share of the verb. The exports have
no cell: neither crate has a benchmark, and each walk is one of these.

| ms, min / median | base | changed |
|---|---|---|
| `linework` (three walks, 250 lines planned) | 15.19 / 17.81 | 3.46 / 4.25 |
| `survey_codes` (1 000 entities coded, not executed) | 5.62 / 6.79 | 2.12 / 2.52 |
| `code_census` | 4.90 / 6.00 | 1.39 / 1.77 |
| `utility_read` (the walk alone) | 4.72 / 5.77 | 1.19 / 1.48 |

Changed / base on the median 0.24, 0.37, 0.29 and 0.26 (A/A between 0.96 and
1.06 in these cells); the `count=` of each is the same in both builds.

### Global Modify's layers

Found on 2026-11-21 and left then. `global_modify::match_entities` is what
every scope and filter resolves through - Global Modify, and each verb that
takes `LAYERS`, `VIEW`, `DRAWING` or `WHERE` - and for every entity it asked
again what only the entity's layer decides: whether the layer is one of the
scope's, whether a pattern takes its path, whether the layer is shown and
whether the view hides it - a lookup of the layer in the table and a match of
every pattern, 100 000 times for the two hundred answers a drawing of two
hundred layers has. The match is now in two halves, the layer's and the
entity's, and **the layer's half is asked once for a run of entities on one
layer**: the name and the answer of the last layer asked about are kept for
the length of the call. A scope and filter that ask nothing of a layer (the
whole drawing or a selection, no pattern, not drawn-only) skip the half
altogether.

Nothing an entity is asked changed, only how often its layer is: the
layer's half is a function of the model, the scope, the filter and the
layer's name, none of which changes during the call, and the drawn rule is
put together from the same two pieces `is_drawn` is (`is_layer_drawn` and the
entity's own switch). The ids come out in the same order, sorted as before.
**Rejected:** a map of every layer seen in the call, which would also serve a
drawing whose layers alternate entity by entity - it costs a hash of the
layer's name an entity, more than the half itself costs for a scope of one
layer and no pattern, where the last layer costs one comparison of two names.
Such a drawing gains nothing from this and pays that comparison.

Held by `global_modify_layers::a_layers_scope_takes_what_the_rule_takes_entity_by_entity`
and its neighbours for patterns, drawn-only, a view with hidden layers and an
area, and a selection: 1 200 generated entities on nine nested layers whose
names share prefixes of text, half in runs of twenty on a layer and half
with no two neighbours on one, and the rule written out again in the test,
entity by entity with nothing remembered, from the layers as the test made
them. Every case must take some of the drawing and not all of it.

`cargo bench --locked -p katana_cad --bench drawing_passes`: the 100 000
entities in runs of 25 on 200 layers. The `count=` of a cell, the entities
matched, must be the same in both builds.

| ms, min / median | base | changed |
|---|---|---|
| `match_layers` (a branch with sublayers, two patterns, drawn only) | 7.07 / 7.71 | 2.63 / 2.92 |
| `match_view` (a view hiding two layers of its own) | 9.43 / 9.92 | 3.09 / 3.38 |
| `match_drawing` (no layer asked: the control, which must not move) | 2.30 / 2.86 | 1.36 / 1.95 |

Changed / base 0.37 / 0.38 and 0.33 / 0.34 where a layer is asked about
(A/A 1.00 to 1.03). **The control moved, and for the better** (0.59 / 0.68):
it asks nothing of layers, so the remembered answer is not what it gained
by - the walk is, which became one pass over the entities with the change
(the same gain as "The verbs' walks"). The counts matched are the same.

### A project saved and loaded

No change, a benchmark: until it there was none that opened a database, so
what "Checking a project's contents" and "Empty property maps" took off an
open could be given only as milliseconds, not as a share of the open.
`crates/katana_storage/benches/project_load.rs` generates a project of
100 000 entities on 200 layers - lines and polylines of 12 vertices by turns,
and one in fifty a surveyed point carrying three properties, so 98 % of the
property columns hold the empty object - saves it once to a folder of its own
under the system's temporary directory, removed when the run ends, and times
`ProjectStore::load` alone (with its A/A pair), `ProjectStore::open` with the
load and `apply_to_model_consuming` (what `Document::open` pays before it
builds its index), and `ProjectStore::save` over the project. It uses only
members the commit before "Changes of 2026-11-21" has, so it can be built
against that commit to say what the seven changes above gave a whole open.
**The times are the disk's as much as the code's**: a cell is a result only
outside the spread of `load` against `load_again`.

`cargo bench --locked -p katana_storage --bench project_load`, run on
2026-11-23; the project is 19.3 MB on disk.

| ms, min / median | this build | this build (A/A) |
|---|---|---|
| `load` (100 000 entities read) | 62.61 / 66.38 | 63.51 / 65.98 |
| `open_load_apply` (what an open pays before its index) | 186.92 / 193.31 | |
| `save` (the project rewritten) | 521.15 / 553.79 | |

The build before the changes of 2026-11-22 reads the same (`load` 62.83 /
65.96, `save` 525.28 / 557.60): none of them is on the path of an open or a
save, and this is where the next one that is will be judged. A save costs
eight times a load, which is the place to look first.

### The plan view's furniture on an unchanged frame

Written on 2026-11-22 and measured on 2026-11-23, as the two subsections
after it were; each of the three is built alone on the base for its table.

With the texture kept ("The plan view's frames"), a frame in which nothing
changed still copied the kept drawing, painted the view's furniture over the
copy and compared the picture with the last one, whole: three passes over
every pixel of the view to learn that the screen was right. A key over what
the furniture is painted from was rejected there until its inputs were
listed, with a test for each. `FurnitureKey`
(`crates/katana_window/src/viewport_widget.rs`) is that list. A paint whose
drawing was kept and whose key is the last paint's keeps the picture: no
copy, no furniture, nothing compared.

| drawn over the drawing | painted from | where it is keyed |
|---|---|---|
| a running tool's preview, caption and prompt band | the tool, through a trait object (`ToolHost::feedback`) with no revision | **no key**: every frame of a running tool is painted |
| the selection's grips | the list, the hot ones, the one under the cursor; the view's transform | the key; the drawing's key |
| a grip pressed, dragged or picked up | the controller's target and whether it inserts, which it does not tell | **no key** |
| the vertex a Ctrl-drag would add, over a segment middle | the plan painter | **no key** |
| the band of a hovered grip's hint | the hovered grip; its entity | the key; the drawing's key (notifications, model revision) |
| the empty drawing's hint | whether it shows, the view's size, the text size | the key |
| the selection box | its two corners | the key |
| the snap marker | the snap, the acquired points, the tracking path and its switch, a drafting aid's label, and the cursor where the marker draws at it | the key |
| the fonts of the hint and the bands | View > Text Size | the key |

The colours are constants of the build. **The cursor is in the key only
where it is drawn.** With no tool running the one thing drawn at the cursor
is the snap marker's tracking path and label; the marker now reads the
cursor through one accessor (`SnapMarks::cursor_drawn`), which answers only
then, and the key holds what the accessor answers. So the commonest frame
of all - the pointer crossing a view with no tool running and no grip under
it - is kept, as a frame made by a pointer elsewhere in the window is.

**Rejected:** a revision counted by the tool host and by the grip
controller, which would give a tool's and a drag's frames a key too. A
tool's preview is worked out from the document and the cursor behind a
trait, at every paint, and a count kept by its host would be a guess at
when that answer changes: those frames are painted, and compared, as
before. **Rejected:** dropping the whole-picture comparison now that there
is a key. It stays as the guard of every frame that is painted - one with no
key, and one whose key changed and whose picture did not.

Held by `crates/katana_window/tests/plan_view/kept_furniture.rs`, a case an
input of the key (fourteen). Each shows by `PlanView::furniture_paint_count`
that the kept path is reached before the input changes and left when it
does, and that the picture kept is, pixel for pixel, what a paint of
everything gives - the kept drawing made stale first, so that nothing is
kept. `kept_furniture::a_pointer_crossing_the_view_with_nothing_to_mark_keeps_the_picture`
is the cursor's case, and
`kept_furniture::a_running_tools_frames_are_always_painted` one of the three
with no key.

`cargo bench --locked -p katana_window --bench plan_frames`; the base is the
build before this change.

| ms, min / median | base | base (A/A) | changed | changed / base |
|---|---|---|---|---|
| `idle_frames` (30 frames, nothing changed) | 55.32 / 61.43 | 54.55 / 57.51 | 0.118 / 0.133 | 0.002 / 0.002 |
| `pan_frames` (the control: every frame is painted, and makes a key it has no use for) | 102.41 / 112.04 | 100.44 / 108.05 | 101.70 / 111.02 | 0.99 / 0.99 |
| `idle_frames_selected` (30 frames, 100 000 selected) | 79.63 / 93.30 | 82.22 / 90.86 | 26.74 / 27.34 | 0.34 / 0.29 |

Thirty idle frames of the view cost 0.12 ms where they cost 55: nothing of
the plan is done in a frame that changes nothing of it. The pan did not
move. What is left of an idle frame with a large selection is the listing
of the selection for the key, which the next subsection takes out; with
both, and the rest of 2026-11-22, the build as committed gives `idle_frames`
0.122 / 0.131, `idle_frames_selected` 5.51 / 5.76 and `pan_frames` 93.68 /
107.00 (0.91 / 0.95 of the base, A/A 0.98 / 0.96).

**Not measured by any cell:** frames of the pointer crossing the view, which
were furniture frames and are now kept; `plan_frames` moves no pointer.

### The selection in the plan view's key

Not measured yet (see the subsection above).

The kept drawing is keyed by the selection, because a caller may change the
selection and only ask for a repaint, with no notification
(`Document::notify_selection_changed`). To make that part of the key,
`drawing_key` listed every selected id into a vector and hashed the list a
byte at a time on every frame: at 100 000 selected, an allocation of
800 000 bytes and 800 000 multiplications, each waiting on the one before,
to learn that the selection was the one the last frame had. The view now
keeps the selection it last saw (`selection_seen`) and compares the
document's with it, set against set: two of one size id by id, stopping at
the first that differs, nothing allocated; two of different sizes at once.
The key holds a count of the changes found (`selection_revision`). **The
price** is a second copy of the selection a plan view - eight bytes an id
and the tree's nodes - copied once for each change of it.

**Rejected:** hashing the ids through an iterator over the set, with no
vector, which is what was first proposed. `SelectionSet`
(`crates/katana_cad/src/selection.rs`) hands its ids out only as a vector,
and by reading the hash, not the vector, is the larger part of the cost. A
comparison also cannot take two selections for one, as a 64-bit hash could.
**Nothing persists the key** or compares it across builds: it is private to
the view and lives in memory, so that its selection part is now a count and
not a hash changes nothing outside the view.

Held by
`view_ui::the_drawing_is_painted_again_for_a_selection_one_id_different_and_not_for_the_same_set`
(the selection changed with no notification, which would repaint by itself:
one id swapped for another with the count as it was paints the drawing
again, the same set put again in another order does not).

`cargo bench --locked -p katana_window --bench plan_frames`: a second view
of the same 100 000 lines with every one selected; the base is the build
before this change.

| ms, min / median | base | base (A/A) | changed | changed / base |
|---|---|---|---|---|
| `idle_frames_selected` (30 frames, 100 000 selected) | 79.63 / 93.30 | 82.22 / 90.86 | 61.85 / 70.59 | 0.78 / 0.76 |
| `idle_frames` (the control, nothing selected) | 55.32 / 61.43 | 54.55 / 57.51 | 53.43 / 61.50 | 0.97 / 1.00 |

### The sheet editor's canvas

Not measured yet (see two subsections above).

The sheet editor's canvas (`crates/katana_window/src/sheet_editor.rs`) keeps
the painted sheet as an image and paints it again only when the sheet, the
zoom or the size changes. But every frame it made a new picture - the desk,
that image laid down, the drawing area, the selection's outlines and
handles, the rulers - converted it pixel by pixel into egui's colours and
handed the whole of it to the texture, for a picture the texture already
held. Two changes, apart:

1. **The texture is given the picture only when the picture changed.** The
   canvas keeps its last picture and counts a revision (`picture_revision`)
   when a paint leaves it different; the editor converts and hands over only
   a revision it has not shown (`canvas_shown`). The comparison is of whole
   pictures, as the plan view's is, and saves the conversion and the upload,
   not the paint. Held by
   `kept_canvas::the_texture_is_given_the_canvas_when_it_changes_and_only_then`
   (every frame is asked what it did: a canvas that is not the last frame's
   is handed over once, whole, and is the canvas; one that is, is not handed
   over).
2. **The picture is kept when nothing it is painted from changed.** The
   canvas's paint (`paint_over`) reads the canvas and nothing else - no
   document - so what it is painted from can be listed: `CanvasKey`. The
   painted sheet, by a count of its paints; the canvas's size and device
   ratio, the zoom and where the paper is; the set and which sheet of it
   (the paper, the viewports' rectangles and locks); the selected viewports
   and the primary; the paper grid and its spacing; the rulers and the
   cursor's place on the paper that they mark. The fonts and colours are
   constants. A paint whose key is the last paint's paints nothing. **No
   key while a drag is under way**: a drag's marks - the viewports' paint
   going with the hand, the guides and the plate, the rubber band, a plan
   panned inside its viewport, the paper under the hand - are read from a
   dozen members the drag keeps changing, and those frames are painted and
   compared as before. The key is exactly as good as the kept sheet image
   the canvas already had: a change the editor does not see leaves both as
   they were. Held by the ten cases after the first in
   `crates/katana_window/tests/sheet_editor/kept_canvas.rs`, an input each,
   which show by `SheetCanvas::paints` that the kept path is reached before
   the input changes and left when it does, and that the picture kept is,
   pixel for pixel, what a paint of everything gives.

**Rejected:** a kept buffer for the picture, as the plan view has: the
canvas's paint begins by filling the whole desk, so a buffer used again
would be right, but it is a change of its own with its own measurement and
is not made (Not done). **Rejected:** a key for a drag's frames: a drag is
the one time the canvas changes at nearly every frame.

`cargo bench --locked -p katana_window --bench sheet_frames`
(`crates/katana_window/benches/sheet_frames.rs`): the whole editor through
egui headless on a 1400 x 900 screen, over one A3 sheet of two plans of
20 000 generated lines and a note. Headless, a texture handed over is only
queued, so the upload itself is not in these times. The base is the build
before both; change 1 alone is the build between them.

| ms, min / median | base | base (A/A) | change 1 | changes 1 and 2 | 1 and 2 / base |
|---|---|---|---|---|---|
| `idle_frames` (30 frames, nothing changed) | 213.67 / 268.36 | 222.75 / 273.60 | 196.03 / 263.35 | 11.65 / 32.64 | 0.05 / 0.12 |
| `select_frames` (10 frames, another viewport picked in each) | 83.42 / 91.71 | 87.97 / 92.95 | 83.43 / 94.47 | 88.27 / 93.81 | 1.06 / 1.02 |
| `pointer_frames` (10 frames, the pointer moved across the paper in each) | 81.30 / 90.78 | 86.74 / 92.19 | 84.07 / 92.36 | 86.53 / 92.45 | 1.06 / 1.02 |
| `uploads=` of `idle_frames` | 30 | 30 | 0 | 0 | |

**Read the medians of `idle_frames`**: its minimum and its A/A twin's
disagree in the changed build (11.65 and 31.87), so the minimum is no
result; the medians agree (32.64 and 32.57, of 268.36). An idle frame of the
editor costs an eighth of what it did. **Change 1 alone is not a result in
these times** (0.92 / 0.98 against an A/A of 1.04 / 1.02): headless, the
upload it saves is only queued, so what it saves here is the conversion,
which the comparison it adds costs back. It was kept for the 30 uploads a
second it saves the desktop and because change 2 stands on it. The frames
that change the canvas are inside the A/A spread (1.06 / 1.02 against 1.05
to 1.07 / 1.01 to 1.02): the price could not be told from the machine.

`select_frames` and `pointer_frames` change the canvas in every frame, so
both changes can only cost them: the comparison, and a key made and not
matched. They are there to show that price. The frames are the whole
editor's, its menus, list, properties and checks with the canvas, so the
canvas's part is less than the whole of a figure.

### The safe UTF-16 append

Not a change made for speed: `append_ascii_utf16`
(`crates/katana_core/src/text_kernels.rs`) lost its `unsafe` on 2026-11-22
(`docs/rust.md`, the third pass). It reached into the String's spare
capacity and set its length; it now narrows a block of 4 096 units into an
array of its own and appends that as text, so every ASCII byte is checked
and copied once more. The second pass had declined this form for want of a
benchmark; this is the benchmark, and the price.

`cargo bench --locked -p katana_core --bench decode_utf16`
(`crates/katana_core/benches/decode_utf16.rs`): 4 000 000 units (8 MB) of a
point file's lines through `decode_text`, at the AVX2 level.

| ms, min / median | base | base (A/A) | changed | changed / base |
|---|---|---|---|---|
| `ascii_le` (no other character) | 0.629 / 0.708 | 0.617 / 0.708 | 0.776 / 0.865 | 1.23 / 1.22 |
| `sparse_le` (a degree sign every 200 units) | 1.386 / 1.579 | 1.377 / 1.600 | 1.478 / 1.677 | 1.07 / 1.06 |
| `dense_le` (another character every 8 units) | 4.227 / 4.365 | 4.235 / 4.376 | 7.053 / 7.236 | 1.67 / 1.66 |
| `sparse_be` | 1.413 / 1.597 | | 1.465 / 1.635 | 1.04 / 1.02 |

**It is slower, and was kept.** A fifth more on plain ASCII and two thirds
more on a text with another character in every eight, against an A/A of
1.00: 0.15 ms and 2.8 ms on 8 MB, of a file whose reading after the decode
takes hundreds of times that. The owner's ask was the `unsafe` gone where
it can go, and the two places it stays (the task pool's workers, the
rasteriser's disjoint writes) stay for a tenth and two thirds of a frame,
every frame; this is a few milliseconds once a file. Rejected: keeping the
`unsafe` for the dense case alone (two paths to keep equal, for 3 ms).
Reverting the one commit restores the old form.

### Found and left alone

Hot spots read on 2026-11-21 and not changed, with what each would take:

- **Two more lists of every id followed by a lookup each**, read on
  2026-11-22 while the eight of "The verbs' walks" were changed and left,
  being outside that work's list: `label_entities`
  (`crates/katana_cad/src/annotation/label_layout.rs`, what LABEL LAYOUT lays
  out with no view) and `sheets_from_plot_frames`
  (`crates/katana_cad/src/plotting/generators.rs`). Each holds the model by
  shared reference and wants `EntityDatabase::iter` and nothing else; a
  verb's one pass each.
- **Every command walks every entity for the annotations that follow others**
  (`associative_changes`, `crates/katana_cad/src/annotation/associative.rs`,
  which `Document::execute` wraps every command in): one pass of the drawing
  a command, and the resolution of each dimension, leader and label, whatever
  the command changed - a click that draws one line in a drawing of 100 000
  entities pays the pass. It would take the ids of the entities that follow
  another kept beside the drawing, moved by the same change events the
  spatial index reads, and a test that the kept ids give what the pass gives
  after every kind of change; unmeasured.
- **A layer or dimension-style edit draws every dimension again** to store
  its box ("The index and the tables a box is drawn from"): switching a layer
  off in a drawing of 10 000 dimensions pays 10 000 drawings for boxes that
  did not move. Telling which layers changed their choice of style, and which
  styles changed, needs the two tables as they were before the step;
  unmeasured.
- **A command that fails after it has run costs a rebuild of the index** (the
  same section): right for a failure nothing is known about, and paid also by
  a transaction whose later part is refused and which rolled back exactly.
  Saving it needs the stack to hand over the changes of a failed step rather
  than drop them, which is `crates/katana_commands/src/command_stack.rs`'s to
  give.

## Not done

- **No Rust benchmarks.** None of the measurements above has a Rust twin,
  so no claim about the Rust's speed can be made yet, and the first change
  made for speed must first add the benchmark it is judged by, with the A/A
  method of `docs/testing.md`, "Benchmarks". The tables above say what each
  should find. The Rust benchmarks so far are new work, not twins,
  each a plain program with no benchmark library: the meshes' faces in plan
  (`crates/katana_window/benches/mesh_plan.rs`; figures in
  `docs/plan_view.md`, "Meshes in plan"), the rasteriser's frame and the task
  pool's dispatch (`crates/katana_render/benches/rasterise.rs`), and the four
  of "Changes of 2026-11-21" above (`plan_frames`, `entity_database`,
  `spatial_index`, `apply_contents`), measured there, with the four of
  2026-11-22 (`sheet_frames`, `drawing_passes`, `project_load`,
  `decode_utf16`).
- **What a table edit costs the index is not measured** ("The index and the
  tables a box is drawn from"): a cell that edits a dimension style of a
  drawing with many dimensions would say.
- **A running tool's frames, and a drag's, keep nothing.** The plan view
  paints, and compares, every frame of a running tool and of a grip in hand,
  and the sheet canvas every frame of a drag ("The plan view's furniture on
  an unchanged frame", "The sheet editor's canvas"). A tool's frames could
  be kept only if a tool said when its preview changes, which is every
  tool's to say.
- **`SelectionSet` has no iterator.** With one, the plan view could keep the
  selection it last saw as a sorted vector and compare the set with it in
  one walk of the tree, where it now walks two
  ("The selection in the plan view's key").
- **The sheet canvas makes a new picture for every frame it paints**, where
  the plan view lays its own into a kept buffer.
- **The safe kernels' speed is unmeasured.** The kernels were made safe on
  2026-11-20 rather than removed ("Safe kernels" above); that the folds into
  single loads and stores happened, and that no kernel slowed, needs the
  benchmarks above, run before and after. The changed kernels: the UTF-8
  validator and the UTF-16 narrowing (also the baseline facade's compare and
  select), the point transform, bounds and projection, the rasteriser's
  vertex transform and fill (the fill's masked stores became whole-block
  blends), the scene's lift, normals, vertices, faces and fade, and the
  GPU packing (whose per-vertex store is the likeliest to have changed).
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
