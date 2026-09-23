# Rendering — camera, draw lists, the software rasteriser and the scene

`katana_render` (PLAN.MD Phase 15) turns a list of primitives into pixels. It
may see only `core`, `math` and `geometry` - it cannot include an Entity or a
Document (Rule 3, enforced by the layering test) - and `katana::cad::SceneBuilder`
is the one place that knows both a Document and a DrawList. This document is the
record of the decisions in both; the code comments say the same things next to
the lines they govern. Until 2026-09-23 there was no such document and every
renderer decision lived only in headers and PLAN.MD section 20 (audit finding).

## What draws what

* The **plan viewport** (`katana_qt/viewport_widget.cpp`) paints with `QPainter`
  straight from the model. It is not on the DrawList path.
* The **3D and Elevation cells** of the tiled viewport go through
  `SceneBuilder` → `DrawList` → `Rasterizer`. A Vulkan backend, when there is
  one, replaces the rasteriser and nothing above it.
* Why a software rasteriser at all, when the plan names Vulkan: every pixel it
  writes can be asserted in a unit test on a build machine with no GPU, so the
  clipping, fill and depth rules are pinned down before a driver has to
  reproduce them; and it fixes the architecture - everything above it talks to
  `DrawList` and `Camera`.

## Conventions (`camera.hpp`)

World right-handed and Z-UP (survey data is Z-up; converting at the door would
mean converting every picked coordinate back). Eye space looks down -Z. Clip
space keeps x, y in [-w, w] and z in [0, w]: NDC depth is 0 at the near plane
and 1 at the far - the Vulkan range rather than OpenGL's [-1, 1], because the
eventual backend is Vulkan and a float depth buffer resolves far more of [0, 1]
than of [-1, 1]. Screen pixels have their origin top-left with +y down. The
camera is a value, so a viewport records its view before an orbit by copying it.

## The rasteriser, in three stages (`rasterizer.hpp`)

1. **Transform** every vertex once, in parallel, into clip space, and classify
   it into a five-bit clip code (below). A DrawList is indexed precisely so a
   vertex shared by twenty triangles is transformed once.
2. **Set up and bin**: clip what crosses a plane, project to pixels, widen lines
   into quads, and record each primitive in every 64×64 tile its box touches -
   in parallel over a fixed number of chunks.
3. **Rasterise** one task per tile. A tile owns its pixels, so there is no lock,
   no atomic and no false sharing, and its colour and depth stay in cache.

**Determinism (Rule 7).** The chunk count in stage 2 is a function of the
primitive count alone, never of the core count, so a tile visits its primitives
in the same order on every machine; with a strictly-less depth test the frame
is reproducible bit for bit. `ThreadCountDoesNotChangeASinglePixel` renders the
same scene with 0, 1, 3 and 7 workers and compares the buffers byte for byte.

### Clipping (rewritten 2026-09-23, audit REN-01)

Five planes, in clip space, before the divide:

* the **near plane**, clip z ≥ 0 - exactly w ≥ near for this projection, and
  `setDepthRange` refuses near ≤ 0, so every kept vertex has w > 0;
* a **guard band**, |x| ≤ 2w and |y| ≤ 2w, which bounds every projected
  coordinate to within half a viewport of the image.

Triangles are clipped Sutherland-Hodgman (near plane first, so every later cut
interpolates between vertices with positive w) and fanned; lines Liang-Barsky.
A primitive whose vertices' codes OR to zero passes straight through, and one
whose codes AND to non-zero cannot be seen and is dropped - two bit operations,
because the codes were computed once per vertex in stage 1.

Decisions, with what was rejected:

* **It used to clip at w > 1e-6.** That kept the divide finite and nothing
  else: a cut vertex projected to about 1e6 × its clip coordinate, beyond
  INT_MAX in any view taller than ~213 px, and the float-to-int conversion in
  the binning is undefined there (INT_MIN on x86-64) - so a triangle crossing
  the eye plane was dropped whole, visible part included. The ground under a
  perspective eye went missing. GCC's `-fsanitize=undefined` does not include
  `float-cast-overflow`, which is why no sanitizer run saw it;
  `cmake/KatanaSanitizers.cmake` now names it.
* **The cut is computed in double.** Plane distances from float inputs are
  exact in double; in float, 2 + (-3.1e8) loses the 2, the cut parameter comes
  out at exactly 0.5 and the vertex lands at the image centre. A cut vertex is
  small however large its endpoints, so rounding it back to float costs nothing.
* **A band of 2, not 1 or more.** Wider than the image so a point or a wide line
  centred just outside still draws the part that reaches in, and so a triangle
  that only overhangs takes the pass-through path. Wider costs edge-function
  precision: the error is about the triangle's extent in pixels × 2⁻²⁴.
* **Clip codes per vertex, not a test per triangle corner.** The first version
  tested three corners against five planes; a TIN vertex is a corner of about
  six triangles, so the test ran six times per vertex in the serial-per-chunk
  stage and showed in the medians. Measured in `docs/performance.md`.
* The far plane is not clipped: nothing numerical goes wrong beyond it and the
  per-pixel depth test rejects depth > 1.
* Every float-to-int conversion is clamped first (`pixelFloor`) - the last line
  of defence, and the only one for a point's half-size, which comes straight
  from the draw list.

### Lines, points, fill and depth

* **Lines are widened in screen space** into quads and drawn by the triangle
  path - one tested rasteriser - so a line keeps its pixel width at any depth.
  **Square caps** extend each end by half a width: without them a segment
  shorter than a pixel encloses no pixel centre and draws nothing, and a densely
  surveyed string, all sub-pixel segments when zoomed out, vanished entirely.
* **No top-left fill rule.** A pixel exactly on a shared edge is covered by both
  triangles; with an opaque, strictly-less depth test that costs a redundant
  write and changes no pixel. It must be added before any blended pass.
* **Perspective-correct colour**: c/w and 1/w are interpolated and divided.
* **Depth bias** lifts wireframe and linework off the surface they lie on.

## The scene (`cad/scene.hpp`)

* Lighting is **baked** by the scene builder: the rasteriser interpolates and
  nothing else.
* Curves are tessellated here, not in the renderer, because how finely an arc
  is chorded is a modelling decision (view scale, drawing tolerance).
* **What is drawn is what the plan draws**: an entity is in the scene only when
  `cad::isDrawn` says so - visible, on a layer that is shown once its ancestors
  are counted (PLAN.MD 5.1). Until 2026-09-23 the scene tested the entity's own
  flag only and drew hidden layers, and framed zoom-extents around them (audit
  REN-03/04); `sceneBounds` now covers drawn entities only.
* Vertical exaggeration scales about a datum and nothing else.

## Performance

`benchmarks/bench_render.cpp` (1920×1080, ground grids of 131k and 1.05M
triangles, framed and from within) is where rasteriser cost is measured; the
figures for the clipping rewrite are in `docs/performance.md`. Compare builds
with `tools/compare_benchmarks.py --alternate`, not single runs: runs on this
machine vary by up to 2×.

## Outstanding

The GPU (PLAN.MD Phase 15): no Vulkan device, GPU buffers, GPU picking,
instancing, frustum culling beyond the tile bin, LOD or streaming. The open
render defects of the audit are in `docs/audit/2026-09-23-defects.md`, section
REN.
