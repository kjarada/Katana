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
lane by lane; the text narrowing writes into a `[u8]` or a String's spare
capacity through one slot trait instead of a raw pointer. What stays unsafe:
in each crate's dispatch module (`text_kernels.rs`, the geometry, render,
scene and packing `simd` entries) the call into a kernel, one line an entry
under the one proof that the level is one the processor runs; the baseline
facade's two entries into SSE2 or NEON, compiled only where the target has
them; and, in `append_ascii_utf16`, the String's spare capacity reached
through its `Vec` and its length set over what was written, since a safe
append would validate or copy every byte a second time. Rejected:
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
were measured together, not apart**: what each gives alone is not known, and
the pan's figure is the only evidence for the second.

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
database, where this is one part among the reading of the rows; no load
benchmark exists yet.

### Found and left alone

Hot spots read on 2026-11-21 and not changed, with what each would take:

- **The plan view's copy and furniture on an unchanged frame.** With the
  texture kept, an idle frame still copies the drawing, paints the furniture
  and compares. Skipping all three needs a key over everything the furniture
  reads (rejected above until someone lists it with a test for each input).
- **The sheet editor repaints its whole canvas every frame**
  (`crates/katana_window/src/sheet_editor.rs`, `paint_canvas`), and hands it
  to its texture every frame. It needs what the plan view has: a kept picture
  and a key that says when it is stale; the comparison used above would save
  only the upload, the smaller part there.
- **`for_each_candidate` works every candidate's extent out a second time**
  (`crates/katana_cad/src/spatial_query.rs`): the index has already compared
  the stored box, which is the same box unless the index is stale - and a
  dimension's box depends on its style, which a table edit changes without
  touching the entity. Dropping the second test changes answers in exactly
  that case, so it needs the index refreshed on a dimension style's change
  first.
- **`drawing_key` lists the whole selection every frame**
  (`crates/katana_window/src/viewport_widget.rs`): a vector of every selected
  id, to hash it. An iterator over the set would do; a fraction of a
  millisecond at 100 000 selected.
- **Other lists of every id followed by a lookup each**, as the labels' walk
  was: `crates/katana_cad/src/linework.rs` (three), `survey_coding.rs`,
  `utilities/utility_data.rs` and `code_table.rs` in the same folder, and the
  archive and IFC exports. Each is a verb's one pass, not a frame's, so each
  is worth a few milliseconds a run at 100 000 entities; `EntityDatabase::iter`
  is there for them.
- **Global Modify's match** (`global_modify::match_entities`) resolves the
  layer and matches the layer patterns for every entity; entities come in
  runs on one layer, so remembering the last layer's answers would do. Read
  as a few milliseconds at 100 000 entities and left: it runs once a command.

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
  `spatial_index`, `apply_contents`), measured there.
- **The plan view's two changes were measured together.** What the kept
  buffer gives without the kept texture, and the reverse, needs a build of
  each alone (`plan_frames`).
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
