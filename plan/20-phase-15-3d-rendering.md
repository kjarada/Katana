<!-- Katana plan, section 20 of 47. Index: ../PLAN.MD. Previous: 19-phase-14-terrain-engine.md. Next: 21-phase-16-3d-cad.md -->

# 20. Phase 15 — 3D Rendering

**STATUS: PARTIALLY DELIVERED.**

A complete renderer exists and is in the application; it runs on the CPU rather
than on Vulkan. The abstraction the plan asks for is in place, so the Vulkan
backend is a second implementation rather than a rewrite.

Delivered, in `katana_render` (allowed to see only core, math and geometry — it
cannot include an Entity or a Document, which is Rule 3 made structural):

* `Camera` — orbit/pan/dolly, perspective and orthographic, the ten standard
  CAD views, framing by bounding SPHERE so orbiting after a zoom-extents cannot
  push a corner off screen, pick rays that are the exact inverse of projection.
  Z-up world, screen +y down, NDC depth 0 at the near plane (the Vulkan range,
  chosen because that is the eventual backend). CORRECTED 2026-09-23: the
  second reason given here - "a float depth buffer resolves far more of [0,1]
  than of [-1,1]" - holds only with REVERSED Z (near at 1). With near at 0,
  as built, the far scene lies just below 1.0 at a fixed 2^-24 spacing, and a
  framed 500 m site gets depth steps of 0.8-3.75 m (audit REN-02, open).
* `DrawList` — structure-of-arrays positions/colours with indexed triangles,
  lines and points. Lighting is BAKED by the scene builder, so the rasteriser
  interpolates and nothing else.
* `Rasterizer` — three stages in the shape a tile-based GPU uses: transform
  every vertex once in parallel and classify it against the clip planes; clip,
  project, widen lines into screen-space quads with square caps, and bin each
  primitive into every 64x64 tile it touches; then rasterise one task per
  tile, each tile owning its pixels exclusively so there is no lock and no
  false sharing. Perspective-correct colour, z-buffered, depth bias for
  wireframe over surfaces.
* **Clipping - CORRECTED 2026-09-23.** Clipping is against the true near plane
  (clip z >= 0, i.e. w >= near) and a guard band of twice the viewport, with
  the cut computed in double (Sutherland-Hodgman for triangles, Liang-Barsky
  for lines), and every float-to-int conversion is clamped first. It used to
  clip at w > 1e-6, which kept the divide finite and nothing else: the cut
  vertex projected beyond INT_MAX, the conversion in the binning was undefined
  (INT_MIN on x86-64), and a triangle or line crossing the eye plane was
  dropped whole, visible part included, in any view taller than about 213 px -
  the ground under a perspective eye went missing. Found by the audit (REN-01); the
  regression tests at 400x400 and 1920x1080 fail on the old code. The reasons
  for the band's width and for double are recorded at `kGuardBand` in
  `rasterizer.cpp`; the cost, none measurable, is in `docs/performance.md`.
  No sanitizer could have caught it: GCC's `-fsanitize=undefined` leaves out
  `float-cast-overflow`, which `cmake/KatanaSanitizers.cmake` now names - and
  a MinGW trap-mode UBSan build of the old code dies on it with an illegal
  instruction in the new tests.
* **Reproducible (Rule 7).** The binning chunk count is a function of the
  primitive count alone, never of the core count, so a tile visits its
  primitives in the same order on every machine. Asserted by rendering the same
  scene with 0, 1, 3 and 7 workers and comparing the buffers byte for byte.
* `katana::cad::SceneBuilder` — Document and TIN surfaces to a DrawList:
  elevation and slope ramps, flat shading, breakline edges drawn heavier,
  vertical exaggeration about a datum, sagitta-based arc chording.
* `katana::cad::ViewportLayout` and the Qt `ViewportContainer` — tiled
  viewports (1, 2, 3 or 4 cells), each with its own camera and kind (Plan, 3D,
  Section, Elevation). The tiling arithmetic is headless and unit tested: every
  point belongs to exactly one cell, and pixel rectangles tile any window size
  with no seam and no overlap. SUPERSEDED on 2026-09-23 by section 47: the
  views are now docks, their state is `cad::ViewSet`, and the tiles survive as
  arrangement presets (`cad::dockSplits`).

**OUTSTANDING: the GPU.** No Vulkan device, no GPU buffers, no GPU picking, no
instancing, no frustum culling beyond the per-tile bin, no LOD, no streaming.
The software renderer is interactive on the scenes it is given today but will
not reach section 32's 10M-point target; it exists so that the clipping, the
fill rule and the depth rules are pinned down by pixel-exact tests BEFORE a
driver has to reproduce them. The 3D and Elevation cells talk to `DrawList`
and `Camera` only, so a Vulkan backend does not change them. (This said
"everything above the renderer": the plan viewport and the section view paint
with QPainter and never touch a DrawList, so a GPU plan view is its own work.)

---

