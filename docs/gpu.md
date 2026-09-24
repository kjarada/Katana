# GPU rendering: the Direct3D 11 renderer for the 3D view

The 3D view draws on the CPU today (`render::Rasterizer`, docs/render.md). This
document is the record of its GPU twin: a renderer that takes the same
`render::DrawList` and `render::Camera` and draws them with QRhi, Qt's rendering
hardware interface, on Direct3D 11. It lives in `src/katana_qt/gpu` (library
`katana_gpu`), is tested headlessly in `tests/gpu` and timed in
`benchmarks/gpu`.

**Status (2026-09-24).** Built by default on Windows, tested, measured, and not
yet hosted: `RenderViewWidget` still shows the software rasteriser. Hosting it
is the next step (see "Hosting it" below); everything the host needs - the
widget, the fallback rule, a factory - is here.

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
| `shader_library.hpp` | Where shaders come from: the HLSL sources, and the interface a library of precompiled `.qsb` blobs would plug into later. |
| `shader_compiler.hpp` | The default library: the HLSL compiled to bytecode once per process. |
| `gpu_renderer.hpp` | Draws a packed scene through a camera into whatever render target it is given. Owns buffers and pipelines, not the target. |
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

**Today: HLSL compiled at run time, once per process.** The shaders are HLSL
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

**Later: precompiled `.qsb`.** The owner has not approved installing
`qt6-shadertools`, which provides Qt's `qsb` tool. With it, a build step would
compile one Vulkan-style GLSL source per stage into a `.qsb` holding SPIR-V,
GLSL, HLSL/DXBC and MSL; the bytes would be embedded the way the customisation
is (`tools/embed_customisation.py`: no rcc, no moc) and handed to
`SerializedShaderLibrary`. The renderer would not change - it only ever asks a
`ShaderLibrary` for a program. That would add:

* the **Vulkan, OpenGL and Metal backends**, so Linux and macOS;
* **no compile on the user's machine at all** (the bytecode is built in);
* **shader errors at build time** instead of at the first frame.

One caveat: `qsb` cannot translate a **geometry** shader into HLSL or MSL. The
geometry stage would stay this file's hand-written HLSL (qsb takes it with
`--replace`), and Metal, which has no geometry stage, would draw lines and points
with `Expansion::Instanced`. `SerializedShaderLibrary::serialize` turns any
library into the blob table, and `SerializedShadersDrawWhatTheRuntimeShadersDraw`
proves the seam: the default library's bytecode, serialized and read back, draws
the same frame (0 differing pixels).

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
misbehaves falls back to. WARP, Windows' software Direct3D device, is what the
tests draw on where there is no GPU; it is not offered as a fallback, because it
runs the GPU's work on the CPU, which the rasteriser was written to do well and
WARP was not.

`chooseRenderer(RendererEnvironment)` is the rule, a pure function unit-tested
without a GPU; `currentRendererEnvironment()` is the one place that reads the
real platform and environment. First match wins:

1. The GPU renderer was not built (`KATANA_GPU` off) - software.
2. `KATANA_RENDERER=software` in the environment - software.
3. The user's setting asks for software - software.
4. A GPU view already failed in this session (`renderFailed`, or pipelines that
   would not build) - software. A renderer that failed once is not retried
   behind the user's back.
5. The platform cannot show a `QRhiWidget`: offscreen, minimal, and anything
   that is not the Windows platform while the shaders are Direct3D 11 only -
   software.
6. Otherwise - the GPU.

`KATANA_RENDERER=gpu` changes nothing (it cannot undo rules 1-5, and the GPU is
already the default); any other value is ignored and named in the reason, so a
typo shows in the log.

### Hosting it (the next step)

`RenderViewWidget` keeps what it owns - the document, the scene building, the
empty message, the status line - and gains a child: `makeGpuSceneViewIfChosen(camera,
currentRendererEnvironment(setting, failedBefore))` returns a `GpuSceneView` when
the rule says GPU and null (with the reason) otherwise. The view takes the host's
camera by reference, so switching renderers keeps the view. It leaves that
camera in its own logical pixels, as the software view keeps it, and draws each
frame through a copy sized to the device pixels it draws. Then:

* Before the first frame, `gpuView->setCameraFramed(state.cameraFramed &&
  state.cameraKind == state.kind)` - the software view's own test - so a view
  made over a camera the user has already orbited and zoomed (a renderer
  switch, a re-tile, a re-dock) keeps it; after a framing, write
  `gpuView->cameraFramed()` back into `ViewState::cameraFramed`. A frame of an
  empty list does not count as framed: the first list with something in it is
  framed in its turn, as the software view does.
* On a rebuilt scene, `gpuView->setDrawList(list)` - not per frame.
* `onRenderFailed` - remember the failure for the session (rule 4), delete the
  GPU child and paint with the rasteriser; log the reason. The camera is in the
  GPU child's logical pixels, which are the host's when the child fills it; a
  host whose child is smaller calls `camera.setViewportSize(width(), height())`
  before painting, or the rasteriser refuses the camera.
* `onZoomExtents` - the host frames as it does today (it knows what "the scene"
  is); `onActivated` - make the cell active; `onFrameStats` - the status line.
* Vertical exaggeration goes to `FrameSettings` instead of the scene rebuild.
* At application start-up, `precompileHlslShaders()` on a worker thread (for
  example a `TaskPool` job), so the first GPU view finds its bytecode ready.
* `setOrbitAllowed(false)` for an orthographic elevation view, as the software
  view pans instead of orbiting there.

The widget's mouse and keys are `RenderViewWidget`'s (left drag orbits, or pans
in an elevation view; middle or Shift+left pans; the wheel zooms about the
cursor by 1.15 a notch; double-click and E frame; 1-5 and 0 standard views; P
the projection), in logical pixels as there: the host's camera is in logical
pixels, and a pan or a zoom about the cursor moves the world the same distance
whichever pixels it is counted in. Only the frame is drawn at the display's real
resolution.

Before shipping it, check on the Windows platform what offscreen tests cannot:
floating a 3D dock creates a new top-level window, which releases GPU resources
and starts QRhi again, and every `QRhiWidget` in one window must use the same API.

## Build

`KATANA_GPU` (ON by default on Windows, OFF elsewhere) builds `katana_gpu`, which
defines `KATANA_HAS_GPU` for whoever links it. QRhi's headers are semi-public:
they reach the include path only through `Qt6::GuiPrivate`, with a narrower
compatibility promise than the rest of Qt, which is why everything QRhi-shaped
stays in this directory behind Katana's own types. If `GuiPrivate` is missing,
or the platform is not Windows, configure says why and the GPU module is skipped
- the 3D view keeps the software rasteriser. No new DLL ships: QRhi is inside
`Qt6Gui.dll`, and `d3dcompiler_47.dll` is part of Windows.

**Layering.** `tools/check_layering.cmake` gives `src/katana_qt/gpu` its own
layer, `gpu`, allowed `core`, `math`, `geometry`, `render` and `pointcloud`: the
software renderer's rule plus the cloud's plain types, never a `Document`,
although the directory sits under `katana_qt`.

## Testing

`katana_gpu_tests`, registered with ctest as `gpu.*`, runs offscreen like every
Katana Qt test. The render cases draw small DrawLists into a texture through
`OffscreenGpu` twice - on the machine's GPU and on WARP - and **skip** when that
device is missing (a runner without Direct3D 11), rather than fail; the
arithmetic, packing and fallback cases need no device and always run.

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
  where the cases that need a real `QRhiWidget` frame - the four
  `GpuSceneView.OnTheDesktop...` cases: drawing through the host's camera,
  keeping a camera the host says is framed, framing a list that arrives after
  the first frame, and leaving the camera in logical pixels for the software
  view - run instead of skipping. ctest never sets it, so run it by hand after
  changing `gpu_scene_view.*`; the last case can only tell the two pixel sizes
  apart on a display scaled above 100%.
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
