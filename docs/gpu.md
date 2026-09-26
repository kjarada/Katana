# GPU rendering: the 3D view on Direct3D 11, Vulkan and Metal

The 3D view can draw on the CPU (`render::Rasterizer`, docs/render.md) or on
the GPU. This document is the record of the GPU renderer: it takes the same
`render::DrawList`s and `render::Camera` and draws them with QRhi, Qt's
rendering hardware interface - on Direct3D 11 on Windows, on Vulkan on Linux,
on Metal on macOS.
It lives in `src/katana_qt/gpu` (library `katana_gpu`), is tested in
`tests/gpu` and timed in `benchmarks/gpu`.

**Status (2026-09-26).** Built by default on Windows and Linux, and HOSTED:
`RenderViewWidget` draws with it wherever the renderer rules choose it, and
with the software rasteriser everywhere else - every headless run and test,
and after a GPU view fails ("Hosting", below). The Direct3D 11 path is the
one measured on the owner's machine; the Vulkan path is tested on Mesa's
software Vulkan (lavapipe), not yet on a hardware GPU. The Metal path (macOS,
since 2026-09-26) is built by the release workflow, whose macOS job runs the
`gpu.` suite on the runner's Metal device before packaging; it has not been
measured.

## Architecture

```
DrawList ──packDrawList──▶ GpuSceneData ──GpuRenderer──▶ QRhi (Direct3D 11)
Camera ─────────────── relativeViewProjection (double) ─▶ one uniform block
                                                             │
                                  GpuSceneView (QRhiWidget) ◀┤ on the desktop
                                  OffscreenGpu (a texture)  ◀┘ tests, benchmark
```

| File | What it is |
| --- | --- |
| `scene_origin.hpp` | The precision rule in code: the scene origin, the camera matrix built in double against it, the reversed-Z projection. Plain arithmetic, no Qt. |
| `gpu_scene.hpp` | A DrawList packed the way the GPU reads it (float offsets from the origin, 32-bit indices, lines and points carrying their own ends). No Qt; unit-tested without a device. Point clouds packed to a budget. |
| `shader_library.hpp` | Where shaders come from: the HLSL sources, the library of serialized `.qsb` blobs, and `defaultShaders()`, the one this build draws with. |
| `shader_compiler.hpp` | Windows' library: the HLSL compiled to bytecode once per process. |
| `shaders/`, `baked_shaders.hpp` | Linux's library: the same stages as Vulkan-style GLSL, baked to SPIR-V by `qsb` at build time and embedded with `#embed`. |
| `gpu_renderer.hpp` | Draws a packed scene - one draw list, or layers each with its own depth rule - through a camera into whatever render target it is given. Owns buffers and pipelines, not the target. |
| `offscreen_gpu.hpp` | A QRhi of its own rendering into a texture, with read-back and GPU timestamps: what the tests and the benchmark draw with. |
| `gpu_scene_view.hpp` | The widget: a `QRhiWidget` subclass with the 3D view's mouse and keys. |
| `renderer_choice.hpp` | The fallback rule: GPU or software, and why. |

**What it does differently from the software path**, with the same DrawList:

* **Positions relative to a scene origin kept in double** (below, "Precision").
* **Reversed Z into a 32-bit float depth buffer**, with an infinite far plane
  for perspective. The near plane is at depth 1 and far at 0, cleared to 0 and
  tested "greater". A float's exponent then spends its precision where the
  perspective divide throws it away, and resolution becomes roughly constant
  relative to distance, where the software path's standard Z gives a framed TIN
  only a few hundred distinct depth values (audit REN-02). With the far plane at infinity nothing is clipped
  away by zooming out (audit REN-05).
* **4x multisampling.**
* **Lines and points widened on the GPU** into screen-space quads with an
  analytic antialiased edge ("Lines and points", below). The software path
  widens every line into two triangles on the CPU.
* **A slope-scaled polygon offset on filled triangles instead of the draw
  list's NDC depth bias.** The bias is sized for a standard-Z buffer; applied to
  reversed Z it would pull a line metres forward, through buildings (the x-ray
  defect). Pushing the fill back by two pixels of its own depth slope, plus 64
  units of the depth's last bit for a face seen straight on, lets an edge lying
  in a surface win without letting a line behind a wall show through it
  (`ALineOnTheGroundBehindAWallStaysHidden`). Two pixels of slope are enough
  for a mark that reaches one pixel from its centre - a 1 px line, the TIN's
  edges - and no more, because the same offset lets linework behind a ridge
  show through next to its silhouette.
* **Marks wider than that pulled towards the eye by their own size.** A point
  or line quad is flat in depth, so on a sloping surface the part of a size-5
  point (3 px from its centre, 3.6 px at 125%) uphill of its centre was behind
  the surface and cut off: 13% of the points' pixels were lost on a relief 24
  degrees steep seen 35 degrees down, 17.5% at 125%. Each mark now moves along
  its own ray - same pixels, nearer depth - by three pixels' worth of world at
  its depth for every pixel it reaches beyond the first, and draws whole
  (`MarksLyingOnASlopedSurfaceDrawWholeOverIt`: 100% and 99.97%, orthographic
  100%). A 1 px line is not moved at all. The price is that a mark shows
  through a surface nearer to it than about its own size on screen; a wall
  twice a point's size in front still hides it
  (`APointWellBehindAWallStaysHidden`). The working is at `kMarkPull` in
  `gpu_renderer.cpp`.
* **Per-pixel lighting from face normals** (`LightingMode::PerPixel`), from the
  screen-space derivatives of the position because the draw list has no
  normals. The normal is turned to face the eye, so a face lit from behind gets
  ambient only - where the software path's baked `abs(n.l)` lights both sides
  alike. The scene builder bakes lighting into the colours, so this mode is for
  a list built with `cad::SceneOptions::lightDirection` set to zero (which the
  builder documents as "no shading"); the default, `Baked`, draws the colours as
  they are and matches the software path.
* **Vertical exaggeration as a uniform** (`FrameSettings::verticalExaggeration`
  and its datum): changing it redraws and rebuilds nothing, where the software
  view, as of this writing, rebuilds its whole scene for it.

**Uploads happen only when the scene changes.** `setDrawList` packs the list and
marks it dirty; the next frame uploads it once. Every other frame writes one
160-byte uniform block (the camera and the frame settings) and draws
(`AFrameThatOnlyMovesTheCameraUploadsNothing`). A packed vertex is 16 bytes
against the draw list's 28.

## Packing

`packDrawList` runs on the CPU for every layer the 3D view rebuilds, so every
edit pays it before anything reaches the GPU, and on a dense TIN it was
several milliseconds. `BM_GpuPack` times it on the ground grid with a line
along each triangle edge, as a TIN's edges are drawn (no device needed; its
`digest` counter is FNV-1a over every packed byte). Three changes, in the
order the profile ranked them (2026-09-25):

1. **Bounds once per vertex.** `DrawList::bounds()` visited a vertex once for
   every primitive using it, and a TIN's vertex is a corner of about six
   triangles and an end of as many edges. Each layer was also bounded twice,
   once for the scene's origin (`setLayers`) and once inside `packDrawList`.
   That was 61% of the time. `DrawList::bounds()` now marks the vertices its
   primitives use and bounds each marked vertex once, in index order. This is
   the scene builder's walk (`docs/performance.md`, "The 3D scene build"),
   moved into `render::DrawList` so that both callers share one copy. When an
   extreme is a zero, the one case where the visiting order decides the
   result (which zero's sign), it falls back to primitive order.
   `tests/render/test_draw_list.cpp` holds it to the old walk, to the bit, on
   2000 generated lists with both zeros, NaN, infinities, unused vertices and
   indices past the end. `setLayers` bounds each layer once and passes the box
   in (`packDrawList(list, origin, bounds, out)`).
2. **Triangle indices written in place.** They were appended, three
   `push_back`s a triangle, and that made the whole pack 14% slower than
   sizing the array once (plain integers: one memset), writing through a
   pointer and cutting it back to the triangles kept. The same done to the
   lines made the pack 24% slower instead: a `GpuLine`'s widths default to 1,
   so sizing the array writes every line once before the loop writes it
   again. The lines and points are still appended.
3. **The vertices by AVX2** (`simd/pack_avx2.cpp`, a kernel file under the
   rules of `docs/performance.md`, "SIMD"). Each step packs four vertices:
   their twelve doubles fill three registers, the origin is laid out in the
   same pattern and subtracted lane by lane, and CVTPD2PS rounds each lane to
   float as `static_cast<float>` does. It is the smallest of the three
   changes, because once the first two were done the vertex pass was about
   3% of the profile. Its output is the loop's to the bit
   (`PackDrawList.EveryVertexPacksToTheSameBitsAtEachSimdLevel`: every length
   0-70, with both zeros, infinities, NaN, subnormals and doubles outside
   float's range). A wrong byte rotation, and the tail reading the wrong
   origin lane, each fail that test and the hand-worked one after it. The
   kernel has no minimum length: a probe build with the minimum at 1 packed
   four vertices as fast as the loop (minimums 64 against 67 ns; in the A/A
   copy 66 against 68).

`tools/compare_benchmarks.py --alternate 7 BM_GpuPack before=... after=...
after_again=...`, on the cloud container's 4-core Xeon at 2.10 GHz. `before`
is this tree with only the benchmark added; `after again` is a byte copy of
`after`, the A/A control. The digests are the same in all three binaries and
at both levels. Min / median of 21 samples:

| Case | before | after | after again |
| --- | --- | --- | --- |
| 66k vertices, 197k lines, AVX2 | 8.19 / 8.52 ms | 1.69 / 1.82 ms | 1.72 / 1.82 ms |
| the same, scalar | 8.25 / 8.56 ms | 1.82 / 1.87 ms | 1.82 / 1.90 ms |
| 526k vertices, 1.57M lines, AVX2 | 80.46 / 87.39 ms | 25.89 / 32.69 ms | 24.09 / 31.66 ms |
| the same, scalar | 82.82 / 87.00 ms | 23.50 / 32.40 ms | 27.30 / 33.44 ms |

The pack is 4.6-4.7x faster at 66k vertices and 2.7x at 526k, where its 63 MB
of lines no longer fit in any cache. `before`'s two rows both time the old
code, which had no kernel. The kernel takes 3-4% off the pack at 66k vertices,
where the A/A pair agrees to 2%. At 526k the A/A medians differ by 3% and the
minimums by up to 16%, so its share there is not resolved. On lists the size
of an entity or selection layer it takes 6-12% off (medians, AVX2 against
scalar, then the same in the A/A copy): 226 against 247 ns and 243 against
258 ns for 16 vertices, 4.61 against 5.10 us and 4.53 against 5.12 us for 256.

## Precision

A GPU takes positions and matrices as 32-bit floats, and at an MGA northing of
6 250 000 m a float steps in **0.5 m**. Survey data uploaded as it is would snap
to a half-metre lattice and shimmer as the camera moves. The software path never
meets this because it multiplies by the camera in double all the way to clip
space. The rule, written once in `scene_origin.hpp`:

1. Every position is uploaded as a float **offset from a scene origin** kept in
   double - the centre of the scene's box, so the largest offset is half the
   scene: 6 km on a 12 km corridor, where a float steps in 0.5 mm.
2. The camera matrix is built **in double against that origin**: the eye is
   subtracted from the origin before anything is rounded, and only the finished
   matrix, whose translation is now scene-sized rather than earth-sized, is
   rounded to float.
3. A point cloud has its own origin and its own copy of the uniform block.

The test that holds it: a site at (300 000, 6 250 000, 50) renders like the same
site at the origin, 0 differing pixels on the GPU and on WARP; packed against a
zero origin instead, as a GPU path without the rule would, 510-523 pixels
differ (`ASceneAtMgaCoordinatesDrawsLikeTheSameSceneAtTheOrigin`).

## Shaders

**Windows: HLSL compiled at run time, once per process.** The shaders are HLSL
text in `shader_library.cpp`, assembled from shared pieces (the constant buffer,
the quad vertex, the quad builders) so each stage compiles exactly what it uses.
They are compiled by `d3dcompiler_47.dll`, which every Windows 10 and 11 install
carries: no build-time tool and no package, at the price of Direct3D 11 only.

QRhi can take the HLSL source itself (`runtimeHlslShaders()`), but then every
new QRhi compiles it again - the first 3D view, a second one, a 3D dock floated
into a window of its own, each test target - and the eleven stages cost more
than the device, the target and the pipelines together. So the default,
`compiledHlslShaders()` (`shader_compiler.hpp`), calls the same compiler itself,
keeps each stage's bytecode for the life of the process (keyed by the source
text, so a changed source cannot be answered from a stale entry) and hands QRhi
bytecode. `precompileHlslShaders()` lets a host pay the compile on a worker
thread at start-up, so no view waits for it; and a compile error comes back as a
`Status` carrying the compiler's own message, where QRhi only logs it.

**Linux: precompiled `.qsb`, drawn on Vulkan.** The same thirteen stages are
written a second time as Vulkan-style GLSL in `src/katana_qt/gpu/shaders/`,
stage for stage and function for function, with the shared pieces
(`frame.glsl`, the constant block; `quad.glsl`, the quad builders) included
where the HLSL splices its strings. At build time Qt's `qsb`
(Qt6ShaderTools, part of the Linux toolchain, `docs/building.md`) bakes each to
a `.qsb` holding SPIR-V; `baked_shaders.cpp` embeds the bytes with `#embed`
and hands them to `SerializedShaderLibrary`. So the Linux program compiles no
shader at run time, and a shader error is a build error. The renderer did not
change for it - it only ever asks a `ShaderLibrary` for a program, and
`defaultShaders()` is the one the build carries.

Why the GLSL is written by hand rather than generated from the HLSL, or the
HLSL from it: `qsb` cannot translate a **geometry** shader into HLSL or MSL,
and the default expansion is one. The two sets must draw the same thing; a
change to one belongs in the other, and the tests hold each to the software
rasteriser on its own platform.

**macOS: the same `.qsb`, with Metal Shading Language, drawn on Metal.** The
GLSL above is baked with `qsb --msl 12` as well, so each `.qsb` carries MSL 1.2
beside the SPIR-V, translated by SPIRV-Cross inside `qsb`; QRhi's Metal
backend picks the MSL. Metal has no geometry stage, so the two geometry
shaders are baked as SPIR-V only (`qsb` cannot translate them), and the
renderer, finding `QRhi::GeometryShader` unsupported, draws lines and points
with `Expansion::Instanced`, whose stages are all translated. No shader was
written a third time. Checked on Linux before any Mac: `qsb --msl 12`
translates all eleven non-geometry stages. Metal, like Direct3D 11, renders
into a texture without a window, so the `gpu.` suite runs its device cases
under the offscreen platform there.

What the Vulkan conventions change, and why the shaders did not need to
change with them: clip-space y points down and depth runs 0 to 1, so
`reversedZProjection` flips y for Vulkan (`rhi.isYUpInNDC()`) and needs no
depth correction; every quad is symmetric about its line or centre, so the
flipped screen y covers the same pixels. Were the OpenGL backend used, its
depth correction z' = 2z - w keeps z' = w at the near plane, so the shaders'
near-plane test (w - z < 0) and the mark pull hold there too - but OpenGL's
window depth adds 1 before halving, which throws away what reversed Z gains,
so Linux draws on Vulkan and not on OpenGL. `SerializedShadersDrawWhatTheRuntimeShadersDraw`
round-trips whichever library the build carries (0 differing pixels on
lavapipe).

## Lines and points

A line is drawn as a quad in screen space: the two ends are projected, the quad
is built along and across the segment in device pixels, and the fragment shader
works out how much of the pixel the line's rectangle covers (the distance of the
pixel centre inside each edge plus half a pixel, clamped to [0, 1]) and blends
that as alpha. Square caps half a width past each end, as the software path
draws them; a line thinner than a pixel is drawn a pixel wide, as there. The
quad stops **half a pixel** past the line's edges and ends, where that coverage
reaches zero: nothing further out could be drawn, and every fringe fragment is
paid hundreds of times per pixel on a dense TIN. Draw-list points are squares
of their size; cloud points are round.

There are two ways to turn a line or a point into its quad (`Expansion`):

* **Geometry shader** (the default). A line is two vertices of a line list and a
  point one vertex of a point list. The vertex shader transforms each end once;
  a geometry shader appends a four-vertex strip.
* **Instanced** (the fallback where there is no geometry stage). Each line or
  point is an instance of six vertices, and the vertex shader builds one corner
  per invocation - so each of the six transforms both ends again.

What each is worth, measured on the owner's scene ("Measurements"): with the
lines on screen the two cost the same, 12.5 ms - the fragments decide, and what
halved them was the narrower fringe (22.7 ms with a whole-pixel fringe). With
the camera where no line reaches the screen, which is what zooming into a large
TIN approaches, the geometry shader takes 2.0 ms against instancing's 5.1: the
per-line vertex work is 2.5 times cheaper.

Both run the same HLSL functions and split the quad along the same diagonal, so
they draw the same pixels (`TheGeometryShaderAndInstancingDrawTheSameLinesPointsAndCloud`:
0 differing on the UHD and on WARP). A line's two ends are 20-byte vertices that
together make its 40-byte instance, so one buffer serves both. Lines keep their
depth writes and discard empty fringe fragments: turning that off to let the
depth test run early was measured slower ("Measurements").

## Point clouds

The software path never draws clouds in 3D. The GPU renderer draws one cloud
after the scene, as round sprites of `FrameSettings::cloudPointSize` logical
pixels, from a buffer of 16-byte points (against the engine's 40) relative to
the cloud's own origin (`GpuRenderer::setPointCloud`). `packPointCloud` thins a
cloud to a **point budget** by taking every k-th point through the source order,
k the smallest that fits - the right thinning for a file read in scan or tile
order - and has an overload that reads `pointcloud::PointCloud` in place, each
point in its own colour when the file has colour.

A budget has to come from the GPU: one million random-order points as 2 px
sprites take 17 ms of GPU time on the UHD, two million 26 ms (`BM_GpuCloud`). A
16 ms frame with a scene under it wants the cloud nearer half a million, or a
file in scan or tile order, which is kinder to the GPU's caches than random.

Not done here, because it needs files this module does not own: carrying the
reference clouds to the 3D view (its `ViewContext`), choosing the budget per
frame (COPC resolution levels already give level of detail, `point_cloud_engine.hpp`),
and eye-dome lighting, which would read the depth buffer in a second pass.

## Fallback: which renderer a 3D view uses

The software rasteriser is not going away. It is the only renderer that works
under Qt's offscreen platform, where every test and every headless screenshot
runs - `QRhiWidget` reports `renderFailed` there for every graphics API, while
raw QRhi on Direct3D 11 still renders into textures - it is the bit-exact
reference the GPU is tested against, and it is what a machine whose GPU or driver
misbehaves falls back to. WARP, Windows' software Direct3D device, and
lavapipe, Mesa's software Vulkan, are what the tests draw on where there is no
GPU; neither is offered as a fallback, because each runs the GPU's work on the
CPU, which the rasteriser was written to do well and they were not. A GPU
view that finds only such a device fails, and the host falls back to the
rasteriser (`GpuSceneView::setSoftwareDeviceAllowed`).

`chooseRenderer(RendererEnvironment)` is the rule, a pure function unit-tested
without a GPU; `currentRendererEnvironment()` is the one place that reads the
real platform and environment. First match wins:

1. The GPU renderer was not built (`KATANA_GPU` off) - software.
2. `KATANA_RENDERER=software` in the environment - software.
3. The user's setting asks for software - software.
4. A GPU view already failed in this session (`renderFailed`, or pipelines that
   would not build) - software. A renderer that failed once is not retried
   behind the user's back.
5. The platform cannot show the build's view (`GpuBackend`): offscreen and
   minimal always; for the Direct3D 11 build anything but `windows`, for the
   Vulkan build anything but `xcb` and `wayland`, for the Metal build
   anything but `cocoa` - software.
6. Otherwise - the GPU.

`KATANA_RENDERER=gpu` cannot undo rules 1-5, and the GPU is already the
default; what it does add is permission to draw on a software device
(`RendererDecision::softwareDeviceAllowed`), for a machine where that is
wanted and for the desktop tests. Any other value is ignored and named in the
reason, so a typo shows in the log. Help > About Katana names the choice and
its reason, and the vector kernels in force.

### Hosting

`RenderViewWidget` keeps what it owns - the document, the scene building, the
legend, the empty message, the status line - and, when the rules choose the
GPU, a child that covers it: `makeGpuSceneViewIfChosen(camera,
currentRendererEnvironment(false, failedBefore))`. The child takes the host's
camera by reference, so switching renderers keeps the view.

* **The same layers, with the same depth rules.** The host builds
  `cad::SceneLayers` as it always did, and hands the GPU all five
  (`GpuRenderer::setLayers`): the grid and the edges tested but not written,
  the terrain, the drawing and the selection written - `cad::renderLayers`'
  rules, for the same defects (`GpuLayers.*` tests: the grid through a flat
  pad, a draped line dashed by TIN edges). One draw list, as the first design
  here had it, could not carry them.
* **Only what changed is sent.** A rebuilt terrain sends every layer; a
  rebuilt drawing sends the grid, the drawing and the selection against the
  origin the terrain set; a click sends the selection alone - as the software
  view rebuilds only its overlay (docs/render.md, "The scene in layers").
* **Each frame, what `renderLayers` does first** (`GpuSceneView::onPrepareFrame`):
  the scene rebuilt if it is dirty, the depth range fitted to the layers and
  the grid, and the edges faded for the frame's scale - and only when a fade
  step changed is the edge layer sent again.
* **Widths in logical pixels.** The software framebuffer is in device pixels,
  so its scene is built with `SceneOptions::pixelScale` = the display's ratio;
  the GPU view scales logical widths itself (`FrameSettings::pixelRatio`), so
  its scene is built with 1, and the scene is rebuilt when the renderer
  changes.
* **The camera.** Framing, panning by pixels and zooming about a pixel count
  against the viewport's own size, so the view does not depend on its pixel
  count: the GPU child sizes the camera to its logical pixels, the software
  view to device pixels before each paint. `setCameraFramed(framed)` - the
  software view's own test - keeps a camera the user has orbited through a
  renderer switch, a re-tile or a re-dock; `onZoomExtents` has the host frame
  what it built, and the host writes `ViewState::cameraFramed` as before. A
  resize reframes, until the user moves the camera, at the child's next frame
  and its new size.
* **The mouse and the keys** are the child's, the same as the software view's
  (below); the host's focus proxy is the child, and focus reaching it
  activates the view (`view_focus.hpp`).
* **The legend and the empty message** are painted by a transparent child over
  the GPU view, which `QRhiWidget` composes like any other widget.
* **`onRenderFailed`** - remembered for the session (rule 4); the child is
  deleted after its own call returns, the rasteriser draws from the next paint,
  and the reason goes to the status line.
* **At application start-up** on Windows, `precompileHlslShaders()` runs on a
  worker thread, so the first GPU view finds its bytecode ready. The Vulkan
  build compiled its shaders when it was built.

Not done: vertical exaggeration still rebuilds the scene - its datum decides
where undraped linework goes, which the scene builder owns - rather than going
to `FrameSettings`; and the view does not yet carry reference point clouds
(`GpuRenderer::setPointCloud` is ready for them; the `ViewContext` has none).

The widget's mouse and keys are `RenderViewWidget`'s (left drag orbits, or pans
in an elevation view; middle or Shift+left pans; the wheel zooms about the
cursor by 1.15 a notch; double-click and E frame; 1-5 and 0 standard views; P
the projection), in logical pixels: a pan or a zoom about the cursor moves the
world the same distance whichever pixels it is counted in. Only the frame is
drawn at the display's real resolution.

Checked only on Linux (xcb under Xvfb, lavapipe), and to be checked on the
Windows platform by hand: floating a 3D dock creates a new top-level window,
which releases GPU resources and starts QRhi again, and every `QRhiWidget` in
one window must use the same API.

## Build

`KATANA_GPU` (ON by default on Windows, Linux and macOS, OFF elsewhere) builds
`katana_gpu`, which defines `KATANA_HAS_GPU` for whoever links it, and
`KATANA_GPU_D3D11`, `KATANA_GPU_VULKAN` or `KATANA_GPU_METAL` for the backend
it was built for;
`katana` and the widget tests link it when it is built. QRhi's headers are
semi-public: they reach the include path only through `Qt6::GuiPrivate`, with
a narrower compatibility promise than the rest of Qt, which is why everything
QRhi-shaped stays in this directory behind Katana's own types. If
`GuiPrivate` is missing - or, on Linux and macOS, Qt's shader baker
`Qt6::qsb` - or the platform is none of Windows, Linux and macOS, configure
says why and the GPU module
is skipped: the 3D view keeps the software rasteriser. No new DLL ships on
Windows: QRhi is inside `Qt6Gui.dll`, and `d3dcompiler_47.dll` is part of
Windows. On Linux the Vulkan loader, `libvulkan.so.1`, comes with the
graphics driver, and Qt loads it when a view first asks for Vulkan.

**Layering.** `tools/check_layering.cmake` gives `src/katana_qt/gpu` its own
layer, `gpu`, allowed `core`, `math`, `geometry`, `render` and `pointcloud`: the
software renderer's rule plus the cloud's plain types, never a `Document`,
although the directory sits under `katana_qt`. The `qt` layer may see `gpu`:
it hosts it.

## Testing

`katana_gpu_tests`, registered with ctest as `gpu.*`. The render cases draw
small DrawLists into a texture through `OffscreenGpu` twice - on the machine's
GPU and on its software device (WARP on Windows, lavapipe on Linux) - and
**skip** when that device is missing, rather than fail; the arithmetic,
packing and fallback cases need no device and always run.

* **Windows** runs the suite offscreen, like every Katana Qt test: raw QRhi on
  Direct3D 11 renders into textures there.
* **Linux** runs it on `xcb` under Xvfb (`xvfb-run`, found at configure time):
  Qt's offscreen platform cannot make the Vulkan instance every device case
  needs. There the `OnTheDesktop` cases below draw a real `QRhiWidget` too, in
  ctest, on lavapipe when there is no GPU (Debian and Ubuntu:
  `mesa-vulkan-drivers`, `xvfb`). The one offscreen case is registered again as
  `gpu_offscreen.*`. Without `xvfb-run` the suite runs offscreen and its device
  cases skip, saying why.

A GPU frame is never bit-identical to the CPU's (multisampled edges, the
antialiased fringe, fill rules rounding differently at exact pixel centres), nor
from one driver to the next, so GPU against CPU is compared by:

* **coverage** - a pixel is covered when it is at least half as bright as the
  primitive would make it; the images may disagree only along edges, and the
  bound is a count of edge pixels worked out from the shape's perimeter;
* **colour** - over pixels whose whole 3x3 neighbourhood is covered in both,
  where no edge reaches, the colours agree within 2 levels.

GPU against GPU (the two expansions, runtime against serialized shaders, origin
against survey coordinates) is compared pixel for pixel.

* `KATANA_GPU_TEST_IMAGES=<dir>` saves every compared image as a PNG to look at.
* `KATANA_GPU_TEST_PLATFORM=windows` runs the binary on the desktop platform,
  where the cases that need a real `QRhiWidget` frame - the
  `GpuSceneView.OnTheDesktop...` cases: drawing through the host's camera,
  keeping a camera the host says is framed, framing a list that arrives after
  the first frame, leaving the camera in logical pixels for the software
  view, and refusing a software device unless allowed - run instead of
  skipping. ctest does not set it on Windows, so run it by hand after changing
  `gpu_scene_view.*`; the camera-pixels case can only tell the two pixel sizes
  apart on a display scaled above 100%. On Linux ctest sets
  `KATANA_GPU_TEST_PLATFORM=xcb` itself.
* A `QRhiWidget` that was never shown can be grabbed ONCE. Qt gives each grab
  of such a widget a new QRhi without calling `initialize()` for it, so every
  grab after the first reads back nothing - all zeros, about 3 s a grab on this
  laptop - where a shown widget's grabs take 2-5 ms and read back the frame. A
  desktop test that draws twice shows its widget first
  (`OnTheDesktopADrawListArrivingAfterTheFirstFrameIsFramed`).

## Measurements

`katana_gpu_benchmarks` (built with `KATANA_BUILD_BENCHMARKS`), 1920x1080, 4x
multisampling, Intel UHD Graphics on the i7-1270P. GPU times are the GPU's own,
from QRhi timestamps (`UseManualTime`); `Wall` rows are from beginning the frame
to the GPU finishing it; `Cpu` rows are the software rasteriser on every core,
as the 3D view runs it, on the same list, camera and size. The scene cases need
`KATANA_BENCH_SCENE` to name a `.12da` (survey data, not in the repository) and
skip without it; `KATANA_BENCH_SCENE_IMAGES=<dir>` also saves that scene's frame
from each renderer, to look at what was timed. The laptop is shared and on battery, so every figure below is
from `tools/compare_benchmarks.py --alternate` with an A/A control (the same
binary twice), and ratios mean more than absolutes.

**GPU against the software rasteriser**, medians of two alternated binaries
(run 1 / run 2), 3 rounds x 3 repetitions each:

| Case | Software rasteriser | GPU time | GPU wall | Software / GPU wall |
| --- | --- | --- | --- | --- |
| Ground grid, 131k triangles | 5.76 / 6.46 ms | 2.35 / 2.34 ms | 2.96 / 2.91 ms | 1.9-2.2x |
| Ground grid, 1.05M triangles | 23.09 / 25.88 ms | 2.66 / 2.61 ms | 3.38 / 3.44 ms | 6.8-7.5x |
| 'Test 4 with Tin' (229,462 triangles, 465,741 lines) | 55.04 / 58.94 ms | 13.39 / 13.32 ms | 14.16 / 13.99 ms | 3.9-4.2x |

The archive's scene is what the application builds today (surfaces shaded with
edges, lighting baked). Its frame is nearly all lines: its triangles alone take
1.39 ms of GPU time (`BM_GpuSceneParts/1/4`), its 465,741 TIN edges the other
12. The edges are dark lines a fraction of a pixel long, hundreds over every
pixel the surface covers - the same edges that make it read as a black sheet in
the software view. Whatever the scene builder decides about edges on dense TINs
decides the GPU frame too.

**Lines and points** (the change of 2026-09-24): six-vertex instances with a
whole-pixel fringe ("before") against the geometry shader with a half-pixel
fringe ("after"), with the after binary run twice as the A/A control:

| Case, GPU ms min / median | before | after | after again |
| --- | --- | --- | --- |
| whole scene | 23.21 / 23.59 | 13.38 / 13.46 | 13.16 / 13.47 |
| its lines alone | 22.10 / 22.73 | 12.45 / 12.47 | 12.29 / 12.46 |
| its lines, none on screen | 5.46 / 5.75 | 2.15 / 2.25 | 2.14 / 2.21 |
| ground, 1.05M triangles | 2.54 / 2.69 | 2.62 / 2.68 | 2.66 / 2.77 |

Separated afterwards (`BM_GpuSceneLinesBy`, medians of the two runs): lines on
screen 12.47 / 12.39 ms by geometry shader against 12.48 / 12.49 ms instanced;
none on screen 2.01 / 2.01 ms against 5.10 / 5.24 ms. Tried and rejected: lines
without depth writes and without discarding empty fringe fragments, so the depth
test could run early - 16.83 ms against 12.44 (A/A 12.47): the test and the
discard are what spare the blender the overlapping fringes.

**Pulling marks towards the eye** (the second change of 2026-09-24; "Marks
wider than that", above) against the binary before it, with that binary run
twice as the A/A control, 5 rounds x 3 repetitions, GPU ms min / median:

| Case | before | after | before again |
| --- | --- | --- | --- |
| whole scene | 11.22 / 13.59 | 12.65 / 13.45 | 13.23 / 13.59 |
| its lines alone | 11.57 / 12.66 | 12.27 / 12.59 | 12.45 / 12.69 |
| its lines, none on screen | 1.66 / 2.00 | 1.69 / 1.96 | 1.85 / 1.99 |
| one million 2 px cloud points | 15.65 / 17.23 | 15.10 / 17.88 | 15.83 / 17.46 |

No change the control does not show as well: the scene's lines are 1 px, which
the shader leaves unpulled before doing any of the arithmetic. The cloud's
sprites reach 1.5 px and are pulled, 4% slower at the median against the
control's 1% - inside this laptop's noise, and noted in case it is not.

**Starting a view** (`BM_GpuStartUp`, `BM_GpuShaderCompile`, 5 iterations each,
min / median, run 1 then run 2):

| Case | run 1 | run 2 |
| --- | --- | --- |
| Compiling the eleven stages, cold | 153 / 173 ms | 111 / 126 ms |
| Device, target and pipelines, QRhi compiling the source | 161 / 230 ms | 131 / 148 ms |
| The same from bytecode compiled earlier in the process | 87 / 96 ms | 55 / 81 ms |

The two runs disagree by up to 1.5x (a shared laptop on battery), but within a
run the cached bytecode starts a view 1.8-2.4x sooner.

**Point clouds** (`BM_GpuCloud`, GPU ms median, run 1 / run 2): one million
points 17.23 / 17.27, two million 26.30 / 26.11.

## Risks

* QRhi is semi-public (`GuiPrivate`): a Qt update can change it. It is confined
  to this directory, and the Qt version is pinned by the toolchain.
* Two renderers to keep in agreement. The GPU tests compare against the CPU with
  tolerances; the CPU stays the bit-exact reference (the determinism rule does
  not reach the GPU - its frames are not reproducible bit for bit across
  drivers).
* The integrated GPU shares system memory; large clouds need the budget, and a
  large scene's buffers sit beside the CPU copies until the software path is
  dropped for that view.
* Everything is verified on one machine (Intel UHD, driver 32.0.101.7088) and
  WARP. Other vendors' drivers are untested.
