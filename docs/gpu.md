# GPU rendering: the 3D view on wgpu (Vulkan, Direct3D 12, Metal)

The 3D view can draw on the CPU (`katana_render::Rasterizer`, docs/render.md) or on
the GPU. This document is the record of the GPU renderer: it takes the same
`katana_render::DrawList`s and `katana_render::Camera` and draws them through
wgpu - on Vulkan on Linux and in WSL (Mesa's lavapipe where there is no GPU),
on Direct3D 12 on Windows (WARP as its software device), on Metal on macOS.
It is the crate `katana_gpu` (`crates/katana_gpu/src/`), tested in
`crates/katana_gpu/tests/gpu/` (`cargo test -p katana_gpu`).

**Status (2026-11-15).** The C++ renderer this was ported from drew through
QRhi, Qt's rendering hardware interface, on Direct3D 11, Vulkan and Metal; it
went with the C++ tree on the owner's word of 2026-11-15, and the Rust is the
only renderer. Where a passage below records what QRhi did, it says so. What
changed with the move (`crates/katana_gpu/src/lib.rs`):

* **The shaders are WGSL modules** (`crates/katana_gpu/src/shaders/`,
  assembled by `shader_library.rs`), checked by naga - the compiler wgpu runs -
  once per process (`shader_compiler.rs`), so a mistake comes back as a
  `Status` carrying naga's own message before wgpu is asked for anything. One
  set of shaders serves every backend: the C++ wrote HLSL for Windows and the
  same stages again as GLSL baked by Qt's `qsb` for Vulkan and Metal.
* **Lines and points are widened by instancing.** wgpu has no geometry stage,
  so the C++'s default expansion, a geometry shader, has no twin
  ("Lines and points").
* **A device can be shared** (`offscreen_gpu.rs`, `GpuContext`): the tests make
  one context of each kind per process, and the window can hand its own to the
  renderer. The offscreen tests need no display at all, where the C++'s Linux
  tests needed an X server for Vulkan.
* **The backends' names** are wgpu's (`renderer_choice.rs`, `GpuBackend`):
  Direct3D 12 on Windows where QRhi used Direct3D 11.

Not done: the desktop window's 3D views draw with the software rasteriser for
now, because the window is made before eframe's wgpu device exists and handing
it that device afterwards is left (`crates/katana_window/src/bin/katana/main.rs`;
`docs/rust.md`, "Not done"). The GPU renderer runs in its tests, on a real GPU
or the software device, and in `gpu_scene_view.rs` where a host gives it a
device.

## Architecture

```
DrawList ──pack_draw_list──▶ GpuSceneData ──GpuRenderer──▶ wgpu (a texture or a pass)
Camera ─────────────── relative_view_projection (double) ─▶ one uniform block
                                                             │
                                  GpuSceneView (the window) ◀┤ on the desktop
                                  OffscreenGpu (a texture)  ◀┘ tests
```

| File | What it is |
| --- | --- |
| `scene_origin.rs` | The precision rule in code: the scene origin, the camera matrix built in double against it, the reversed-Z projection. Plain arithmetic, no device. |
| `gpu_scene.rs` | A DrawList packed the way the GPU reads it (float offsets from the origin, 32-bit indices, lines and points carrying their own ends). Unit-tested without a device. Point clouds packed to a budget. |
| `shader_library.rs`, `shaders/` | The WGSL modules (`frame.wgsl`, the constant block; `quad.wgsl`, the quad builders; a module per kind of mark), embedded in the program, and the programs made of them. |
| `shader_compiler.rs` | Every module parsed and validated by naga once per process. |
| `gpu_renderer.rs` | Draws a packed scene - one draw list, or layers each with its own depth rule - through a camera into whatever render target it is given. Owns buffers and pipelines, not the target. |
| `offscreen_gpu.rs` | A device (`GpuContext`) and a texture to render into, with read-back and GPU timestamps: what the tests draw with. |
| `crates/katana_window/src/gpu_scene_view.rs` | The window's view of it, with the 3D view's mouse and keys. |
| `renderer_choice.rs` | The fallback rule: GPU or software, and why. |

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
  (`gpu_render::a_line_on_the_ground_behind_a_wall_stays_hidden`). Two pixels of slope are enough
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
  (`gpu_render::marks_lying_on_a_sloped_surface_draw_whole_over_it`: 100% and 99.97%, orthographic
  100%). A 1 px line is not moved at all. The price is that a mark shows
  through a surface nearer to it than about its own size on screen; a wall
  twice a point's size in front still hides it
  (`gpu_render::a_point_well_behind_a_wall_stays_hidden`). The working is at `MARK_PULL` in
  `gpu_renderer.rs`.
* **Per-pixel lighting from face normals** (`LightingMode::PerPixel`), from the
  screen-space derivatives of the position because the draw list has no
  normals. The normal is turned to face the eye, so a face lit from behind gets
  ambient only - where the software path's baked `abs(n.l)` lights both sides
  alike. The scene builder bakes lighting into the colours, so this mode is for
  a list built with `SceneOptions::light_direction` set to zero (which the
  builder documents as "no shading"); the default, `Baked`, draws the colours as
  they are and matches the software path.
* **Vertical exaggeration as a uniform** (`FrameSettings::vertical_exaggeration`
  and its datum): changing it redraws and rebuilds nothing, where the software
  view, as of this writing, rebuilds its whole scene for it.

**Uploads happen only when the scene changes.** `set_draw_list` packs the list and
marks it dirty; the next frame uploads it once. Every other frame writes one
160-byte uniform block (the camera and the frame settings) and draws
(`gpu_render::a_frame_that_only_moves_the_camera_uploads_nothing`). A packed vertex is 16 bytes
against the draw list's 28.

## Packing

`pack_draw_list` runs on the CPU for every layer the 3D view rebuilds, so every
edit pays it before anything reaches the GPU, and on a dense TIN it was
several milliseconds. The C++ build's benchmark `BM_GpuPack` timed it on the
ground grid with a line along each triangle edge, as a TIN's edges are drawn
(no device needed; its `digest` counter was FNV-1a over every packed byte).
Three changes, in the order the profile ranked them (2026-09-25, in the C++;
the Rust packs the same way, and has no benchmark of its own yet):

1. **Bounds once per vertex.** `DrawList::bounds()` visited a vertex once for
   every primitive using it, and a TIN's vertex is a corner of about six
   triangles and an end of as many edges. Each layer was also bounded twice,
   once for the scene's origin (`set_layers`) and once inside `pack_draw_list`.
   That was 61% of the time. `DrawList::bounds()` now marks the vertices its
   primitives use and bounds each marked vertex once, in index order. This is
   the scene builder's walk (`docs/performance.md`, "The 3D scene build"),
   moved into `katana_render::DrawList` so that both callers share one copy. When an
   extreme is a zero, the one case where the visiting order decides the
   result (which zero's sign), it falls back to primitive order.
   `crates/katana_render/tests/draw_list.rs` holds it to the old walk, to the bit, on
   2000 generated lists with both zeros, NaN, infinities, unused vertices and
   indices past the end. `set_layers` bounds each layer once and passes the box
   in (`pack_draw_list(list, origin, bounds, out)`).
2. **Triangle indices written in place.** They were appended, three
   pushes a triangle, and that made the whole pack 14% slower than
   sizing the array once (plain integers: one memset), writing through a
   pointer and cutting it back to the triangles kept. The same done to the
   lines made the pack 24% slower instead: a `GpuLine`'s widths default to 1,
   so sizing the array writes every line once before the loop writes it
   again. The lines and points are still appended.
3. **The vertices by AVX2** (`crates/katana_gpu/src/simd/pack_avx2.rs`, a
   kernel under the rules of `docs/performance.md`, "SIMD", chosen at run time
   by `crates/katana_gpu/src/simd/pack_kernels.rs`). Each step packs four
   vertices: their twelve doubles fill three registers, the origin is laid out
   in the same pattern and subtracted lane by lane, and CVTPD2PS rounds each
   lane to float as `as f32` does. It is the smallest of the three
   changes, because once the first two were done the vertex pass was about
   3% of the profile. Its output is the loop's to the bit
   (`pack_draw_list::every_vertex_packs_to_the_same_bits_at_each_simd_level`: every length
   0-70, with both zeros, infinities, NaN, subnormals and doubles outside
   float's range). A wrong byte rotation, and the tail reading the wrong
   origin lane, each fail that test and the hand-worked one after it. The
   kernel has no minimum length: a probe build with the minimum at 1 packed
   four vertices as fast as the loop (minimums 64 against 67 ns; in the A/A
   copy 66 against 68). On 64-bit ARM the same step is
   `crates/katana_gpu/src/simd/pack_neon.rs`: LD3 splits two vertices'
   doubles into x, y and z registers, FCVTN rounds them to float, and ST4
   interleaves them with the colour words into four whole vertices. It is held
   to the loop by the same test; the Rust's NEON has not yet been compiled or
   run on an ARM host (`docs/rust.md`, "Not done"), and its any-length rule is
   the AVX2 kernel's, not measured on ARM. The owner has decided that the
   hand-written kernels go, for their `unsafe`, and the scalar loop, which
   gives the same bits, stays, with the speed measured before and after
   (`docs/rust.md`, "Removing `unsafe` is decided but not done").

Measured on the C++ build, with its `compare_benchmarks.py --alternate 7
BM_GpuPack before=... after=... after_again=...`, on the cloud container's
4-core Xeon at 2.10 GHz. `before`
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
space. The rule, written once in `scene_origin.rs`:

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
differ (`gpu_render::a_scene_at_mga_coordinates_draws_like_the_same_scene_at_the_origin`).

**The origin follows a deep zoom.** The centre of the scene bounds the error
only while the view is about as large as the scene. Zoomed in close to a point
far from it, the float offsets there round to more than a pixel, and one stray
entity at (0, 0) under a survey at MGA coordinates puts the centre of the
scene's box 3000 km from the survey, where a float steps in 0.25 m. So the host
packs every layer again against the camera's pivot once the bound on the error
there passes a twentieth of a pixel (`katana_gpu::scene_origin::origin_error_pixels`,
`ORIGIN_ERROR_PIXELS`; `render_view_widget::prepare_gpu_frame`, before the layers
are sent). The bound is 10 u |pivot - origin| over a pixel's footprint at the
pivot, u = 2^-24: a vertex's offset and the matrix's translation are rounded
once each, and the shader's four-term dot product rounds both again (Higham's
bound for a dot product, `scene_origin.rs` works the count). Measured with
the shader's float arithmetic, the error stays under the bound for origins
from 1 m to 3000 km from a point seen from 0.2 m, and with the origin at the
pivot it is under a thousandth of a pixel
(`relative_view_projection::at_the_origins_error_bound_the_pivot_is_drawn_within_it_and_its_own_origin_draws_it_exactly`).
On the device, a line 3100 km from the scene's centre and a millimetre a pixel
is drawn where it is only when packed against the pivot
(`gpu_layers::layers_packed_against_the_pivot_draw_a_survey_zoomed_in_close_where_it_is`,
both devices), and the 3D view repacks as it zooms
(`render_view_gpu::on_the_desktop_a_deep_zoom_far_from_the_scenes_centre_packs_the_layers_against_the_pivot`:
without the repack the origin stays 2950 km off and the line lands off the
cursor, a stub at the top edge).

A repack is a pass over every vertex, 1.8 ms for 66k vertices and 32 ms for
526k in the C++ (`BM_GpuPack`, above), so it happens when the bound is crossed and not
before: the new origin is the pivot itself, and the bound is crossed again
only once the pivot has drifted 0.05 / (10 x 2^-24) = 84 000 of the view's
pixels from it. Before, the zoom stopped instead, at a ten-thousandth of the
scene's box: 3.3 cm on the sample, but 830 m from the survey with the stray
entity, and on the software view too, which has no such error
(docs/render.md, "Zooming towards the cursor"). Rejected: packing each vertex
as two floats, high and low, and subtracting the eye's in the shader (as
globe viewers do), which ends the error everywhere but doubles every vertex
and changes every shader, for a case a repack in a few frames of a deep zoom
covers.

## Shaders

**WGSL, one set for every backend, checked by naga once per process**
(`shader_library.rs`, `shader_compiler.rs`). The modules are text the program
carries (`crates/katana_gpu/src/shaders/`): `frame.wgsl`, the constant block,
and `quad.wgsl`, the quad builders, are joined to the module of each kind of
mark, so each program holds exactly what it uses. wgpu compiles a module for
its backend when the module's pipeline is made; before that, every module is
parsed and validated by naga once in the process, so a mistake in a shader
comes back as a `Status` carrying naga's own message, and a frame never meets
it. One set serves Vulkan, Direct3D 12 and Metal alike, so no change to one
backend's shader has to be made again in another's.

Why that shape, briefly: the C++ renderer wrote its stages twice - HLSL
compiled at run time for Direct3D 11, and the same stages again as GLSL baked
at build time for Vulkan and Metal, because Qt's shader baker could not
translate a geometry shader - and kept the two sets in step by hand, each
held to the software rasteriser on its own platform. The compile once per
process and the error as a `Status` (where QRhi only logged it) are the
C++'s rules, kept; the second set and its round-trip test have nothing to
twin, since the WGSL is one library.

The clip space is WebGPU's on every backend - y up, depth 0 to 1, wgpu
flipping Vulkan's y itself - which is the camera's own convention, so the
correction `ClipSpace` describes is the identity in every build; the other
forms are kept so that a clip space that differed would be corrected in
`scene_origin.rs` and nowhere else. Every quad is symmetric about its line or
centre, so a flipped screen y covers the same pixels. OpenGL's window depth
adds 1 before halving, which throws away what reversed Z gains, so wgpu's
GLES backend is not built (`Cargo.toml`) and Linux draws on Vulkan.

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

**A line or a point is an instance** of six vertices, and the vertex shader
builds one corner per invocation - so each of the six transforms both ends
again. wgpu has no geometry stage, so this is the only expansion. The C++
renderer's default was a geometry shader (a line two vertices of a line list,
appending a four-vertex strip), with instancing its fallback where there was
no geometry stage, as on Metal. What the choice costs, measured on the C++ on
the owner's scene ("Measurements"): with the lines on screen the two cost the
same, 12.5 ms - the fragments decide, and what halved them was the narrower
fringe (22.7 ms with a whole-pixel fringe); with the camera where no line
reaches the screen, which is what zooming into a large TIN approaches, the
geometry shader took 2.0 ms against instancing's 5.1, its per-line vertex
work 2.5 times cheaper. The two drew the same pixels there (0 differing on the
UHD and on WARP), so dropping the geometry shader changes no picture, only
that cost of a deep zoom.

A line's 40-byte instance is its two 20-byte end vertices. Lines keep their
depth writes and discard empty fringe fragments: turning that off to let the
depth test run early was measured slower ("Measurements").

## Point clouds

The software path never draws clouds in 3D. The GPU renderer draws one cloud
after the scene, as round sprites of `FrameSettings::cloud_point_size` logical
pixels, from a buffer of 16-byte points (against the engine's 40) relative to
the cloud's own origin (`GpuRenderer::set_point_cloud`). `pack_point_cloud` thins a
cloud to a **point budget** by taking every k-th point through the source order,
k the smallest that fits - the right thinning for a file read in scan or tile
order - and has an overload that reads `katana_pointcloud::point_cloud_engine::PointCloud` in place, each
point in its own colour when the file has colour.

A budget has to come from the GPU: one million random-order points as 2 px
sprites took 17 ms of GPU time on the UHD, two million 26 ms (the C++
renderer's `BM_GpuCloud`; the wgpu renderer is not measured yet). A
16 ms frame with a scene under it wants the cloud nearer half a million, or a
file in scan or tile order, which is kinder to the GPU's caches than random.

Not done here, because it needs files this module does not own: carrying the
reference clouds to the 3D view (its `ViewContext`), choosing the budget per
frame (COPC resolution levels already give level of detail, `point_cloud_engine.rs`),
and eye-dome lighting, which would read the depth buffer in a second pass.

## Fallback: which renderer a 3D view uses

The software rasteriser is not going away. It is the only renderer of a
headless run, where every window test and every headless screenshot runs (the
headless runner gives a view no device, and the platform it names is
`offscreen`, rule 5 below); it is the renderer of the desktop window's 3D
views for now ("Status", above); it is the bit-exact
reference the GPU is tested against, and it is what a machine whose GPU or driver
misbehaves falls back to. WARP, Windows' software Direct3D device, and
lavapipe, Mesa's software Vulkan, are what the tests draw on where there is no
GPU; neither is offered as a fallback, because each runs the GPU's work on the
CPU, which the rasteriser was written to do well and they were not. A GPU
view that finds only such a device fails, and the host falls back to the
rasteriser (`GpuSceneView::set_software_device_allowed`).

`choose_renderer(RendererEnvironment)` is the rule, a pure function unit-tested
without a GPU; `current_renderer_environment()` is the one place that reads the
real platform and environment. First match wins:

1. The GPU renderer was not built - software. (Kept from the C++, whose
   `KATANA_GPU` option could leave it out; every Rust build has the GPU
   layer, so `current_renderer_environment` always says it is built.)
2. `KATANA_RENDERER=software` in the environment - software.
3. The user's setting asks for software - software.
4. A GPU view already failed in this session (a frame that failed, or
   pipelines that would not build) - software. A renderer that failed once is
   not retried behind the user's back.
5. The platform cannot show the build's view (`GpuBackend`): offscreen and
   minimal always; for the Direct3D 12 build anything but `windows`, for the
   Vulkan build anything but `xcb` and `wayland`, for the Metal build
   anything but `cocoa` - software. The window names the platform
   (`GpuSession` in `render_view_widget.rs`): `offscreen` for a headless run,
   and on the desktop the one its build's backend is shown on
   (`desktop_platform`), so in the Rust this rule refuses a headless run and
   nothing else; the C++ read the name from Qt.
6. Otherwise - the GPU.

`KATANA_RENDERER=gpu` cannot undo rules 1-5, and the GPU is already the
default; what it does add is permission to draw on a software device
(`RendererDecision::software_device_allowed`), for a machine where that is
wanted and for the desktop tests. Any other value is ignored and named in the
reason, so a typo shows in the log. Help > About Katana names the choice and
its reason, and the vector kernels in force.

### Hosting

`RenderView` (`crates/katana_window/src/render_view_widget.rs`) keeps what it
owns - the document, the scene building, the legend, the empty message, the
status line - and, when the rules choose the GPU, a child drawn in its place:
`make_gpu_scene_view_if_chosen` with the session's environment
(`GpuSession::environment`), given the session's device. The host is lent to
each of the child's calls as a `GpuSceneHost`, which hands out the camera -
the view's own `ViewState`'s - so switching renderers keeps the view. Each
frame is drawn into an offscreen target on the device and read back, the
picture the window shows as a texture and a test reads
(`GpuSceneView::grab_framebuffer`); drawing into egui-wgpu's own pass through a
paint callback was rejected, since it would name egui-wgpu in the window's
views and a test could not read it back.

* **The same layers, with the same depth rules.** The host builds
  `katana_cad::scene::SceneLayers` as it always did, and hands the GPU all six
  (`GpuRenderer::set_layers`): the grid, the edges and the selection's casing
  tested but not written, the terrain, the drawing and the selection written
  - `katana_cad::scene::render_layers`' rules, for the same defects (`gpu_layers` tests:
  the grid through a flat pad, a draped line dashed by TIN edges, a selected
  line drawn dark by its own casing - pulled nearer than the core by its
  width, below). One draw list, as the first design here had it, could not
  carry them.
* **Only what changed is sent.** A rebuilt terrain sends every layer; a
  rebuilt drawing sends the grid, the drawing and the selection (its casing
  and its core) against the origin the terrain set; a click sends the
  selection alone - as the software
  view rebuilds only its overlay (docs/render.md, "The scene in layers").
* **Each frame, what `render_layers` does first** (`render_view_widget::prepare_gpu_frame`):
  the scene rebuilt if it is dirty, the depth range fitted to the layers and
  the grid, and the edges faded for the frame's scale - and only when a fade
  step changed is the edge layer sent again.
* **Widths in logical pixels.** The software framebuffer is in device pixels,
  so its scene is built with `SceneOptions::pixel_scale` = the display's ratio;
  the GPU view scales logical widths itself (`FrameSettings::pixel_ratio`), so
  its scene is built with 1, and the scene is rebuilt when the renderer
  changes.
* **The camera.** Framing, panning by pixels and zooming about a pixel count
  against the viewport's own size, so the view does not depend on its pixel
  count: the GPU child sizes the camera to its logical pixels, the software
  view to device pixels before each paint. `set_camera_framed(framed)` - the
  software view's own test - keeps a camera the user has orbited through a
  renderer switch, a re-tile or a re-dock; the host's `zoom_extents` hook
  (`GpuSceneHost::zoom_extents`) has the host frame what it built, and the
  host writes `ViewState::camera_framed` as before. A resize reframes, until
  the user moves the camera, at the child's next frame and its new size.
* **The mouse and the keys** are the child's, the same as the software view's
  (below), and a press in it activates the view (`view_focus.rs`).
* **The legend and the empty message** are painted over the GPU view, as over
  the software one.
* **A failed frame** is queued by the child (`GpuSceneEvent::RenderFailed`)
  and heard by the host after the call returns: the session remembers it
  (rule 4), the child is dropped, the rasteriser draws from the next frame,
  and the reason goes to the status line.
* **The shaders** are validated once per process, at the first pipeline; the
  C++ compiled its HLSL on a worker thread at start-up for the same first
  view, a cost naga's validation does not have.

Not done: vertical exaggeration still rebuilds the scene - its datum decides
where undraped linework goes, which the scene builder owns - rather than going
to `FrameSettings`; and the view does not yet carry reference point clouds
(`GpuRenderer::set_point_cloud` is ready for them; the `ViewContext` has none).

The widget's mouse and keys are `RenderView`'s (left drag orbits, or pans
in an elevation view; middle or Shift+left pans; double-click and E frame; 1-5
and 0 standard views; P the projection), in logical pixels: a pan or a zoom
about the cursor moves the world the same distance whichever pixels it is
counted in. Only the frame is drawn at the display's real resolution. The
wheel is the host's: the child hands it over through
`GpuSceneHost::wheel_zoom`, as it hands framing over through
`GpuSceneHost::zoom_extents`, and the host zooms towards what
it built under the cursor, `Camera::ZOOM_PER_NOTCH` a notch
(`render_view_widget::zoom_at_pixel`; docs/render.md, "Zooming towards the
cursor"). The child holds only packed lists, so without a host it zooms about
the target's plane - which stalls over ground beyond that plane. The desktop
case that holds the hand-over puts the cursor on a point of its line 21.7 m
beyond the target's plane, where the child's own zoom magnifies by 1.124 a
notch instead of 1.15, so it fails without the hand-over; with the cursor on
the target itself, as it first was, both zooms agree and it could not
(`render_view_gpu::on_the_desktop_the_wheel_over_the_gpu_view_zooms_towards_what_is_under_the_cursor`;
the C++ twin failed at the first notch with 1.1245 when the wheel was not
handed over).

A floating view is a window inside the main window (`docs/desktop.md`, "The
workspace"), on the same device, so floating a 3D view makes no new device;
the C++'s floating dock was a top-level window, which released its GPU
resources and started again.

## Build

`katana_gpu` is a crate of the workspace and always built: wgpu with the
Vulkan, Direct3D 12 and Metal backends and the WGSL front end (`Cargo.toml`;
GLES, WebGPU and WebGL are off). The window and the GPU layer name the
workspace's one wgpu, the version egui-wgpu asks for. Nothing ships beside the
program for it: the Vulkan loader, `libvulkan.so.1`, comes with the graphics
driver on Linux, and Direct3D 12 is part of Windows. There is no option to
leave the GPU layer out, as the C++ build's `KATANA_GPU` was, and no
build-time shader tool.

**Layering.** `tools/check_layering.cmake` gives `gpu` its own layer, allowed
`core`, `math`, `geometry`, `render` and `pointcloud`: the software renderer's
rule plus the cloud's plain types, never a `Document`. The window may see
`gpu`: it hosts it.

## Testing

`cargo test -p katana_gpu` (`crates/katana_gpu/tests/gpu/`). The render
cases draw small DrawLists into a texture through `OffscreenGpu` twice - on
the machine's GPU and on its software device (lavapipe on Linux and in WSL,
WARP on Windows) - and **skip** when that device is missing, rather than fail;
the arithmetic, packing and fallback cases need no device and always run. They
need no display: wgpu renders into a texture with no window. In WSL the device
cases run on lavapipe (Debian and Ubuntu: `mesa-vulkan-drivers`).

The window's GPU cases (`crates/katana_window/tests/views/gpu_scene_view.rs`,
`render_view_gpu.rs` beside it) are device cases of the same kind: the view
draws into a target on a device the test gives it, so the `on_the_desktop_*`
cases - drawing through the host's camera, keeping a camera the host says is
framed, framing a list that arrives after the first frame, leaving the camera
in logical pixels for the software view, refusing a software device unless
allowed, the wheel's hand-over and the repack of a deep zoom - run on each
device there is and skip one that is not, in the ordinary `cargo test`. The
C++ suite needed an X server on Linux (Qt's offscreen platform could not make
a Vulkan instance) and a real window on Windows, run by hand; nothing of that
is left.

A GPU frame is never bit-identical to the CPU's (multisampled edges, the
antialiased fringe, fill rules rounding differently at exact pixel centres), nor
from one driver to the next, so GPU against CPU is compared by:

* **coverage** - a pixel is covered when it is at least half as bright as the
  primitive would make it; the images may disagree only along edges, and the
  bound is a count of edge pixels worked out from the shape's perimeter;
* **colour** - over pixels whose whole 3x3 neighbourhood is covered in both,
  where no edge reaches, the colours agree within 2 levels.

GPU against GPU (origin against survey coordinates) is compared pixel for
pixel.

* `KATANA_GPU_TEST_IMAGES=<dir>` saves every compared image as a PNG to look at.
* That the child hands the wheel to its host needs no device
  (`gpu_scene_view::the_wheel_is_handed_to_the_host_that_zooms_towards_what_is_under_the_cursor`).
* The camera-pixels case
  (`gpu_scene_view::on_the_desktop_a_frame_leaves_the_hosts_camera_ready_for_the_software_view`)
  draws at the default ratio of 1, where logical and device pixels are the
  same, so it cannot tell them apart; not done: the same case at a ratio
  above 1 (`GpuSceneView::set_device_ratio`). The C++ twin could tell them
  apart only on a display scaled above 100%.

## Measurements

The figures below are the C++ renderer's (QRhi on Direct3D 11), measured
with the C++ build's benchmarks, which went with the C++ tree and have no
Rust twins yet; the wgpu renderer has not been measured.

`katana_gpu_benchmarks` (built with `KATANA_BUILD_BENCHMARKS`), 1920x1080, 4x
multisampling, Intel UHD Graphics on the i7-1270P. GPU times are the GPU's own,
from QRhi timestamps (`UseManualTime`); `Wall` rows are from beginning the frame
to the GPU finishing it; `Cpu` rows are the software rasteriser on every core,
as the 3D view runs it, on the same list, camera and size. The scene cases need
`KATANA_BENCH_SCENE` to name a `.12da` (survey data, not in the repository) and
skip without it; `KATANA_BENCH_SCENE_IMAGES=<dir>` also saves that scene's frame
from each renderer, to look at what was timed. The laptop is shared and on battery, so every figure below is
from the C++ build's `compare_benchmarks.py --alternate` with an A/A control (the same
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

* wgpu moves quickly between versions: it is pinned in the workspace
  (`Cargo.toml`, the version egui-wgpu asks for) and named only by this
  crate and the window.
* Two renderers to keep in agreement. The GPU tests compare against the CPU with
  tolerances; the CPU stays the bit-exact reference (the determinism rule does
  not reach the GPU - its frames are not reproducible bit for bit across
  drivers).
* The integrated GPU shares system memory; large clouds need the budget, and a
  large scene's buffers sit beside the CPU copies until the software path is
  dropped for that view.
* The C++ renderer was verified on one machine (Intel UHD, driver
  32.0.101.7088) and WARP; the wgpu renderer on lavapipe in WSL. Other
  vendors' drivers are untested.
