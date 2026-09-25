# Rendering — camera, draw lists, the software rasteriser and the 3D scene

`katana_render` turns a list of primitives into pixels. It may see only
`core`, `math` and `geometry` - it cannot include an Entity or a Document
(Rule 3, enforced by the layering test) - and `katana::cad::SceneBuilder` is the
one place that knows both a Document and a DrawList. This document is the
record of the decisions in both and in the 3D view widget that drives them; the
code comments say the same things next to the lines they govern.

## What draws what

* The **plan view** (`katana_qt/viewport_widget.cpp`) paints with `QPainter`
  straight from the model. It is not on the DrawList path.
* The **3D and Elevation views** are docks of the view workspace
  (`katana_qt/view_workspace.cpp`), each a `RenderViewWidget`. The widget owns a
  camera (in the view's `cad::ViewState`) and a framebuffer and nothing else:
  `SceneBuilder` builds the scene in layers, `cad::renderLayers` draws them
  with the software `Rasterizer` into one depth buffer, and the framebuffer's
  bytes are blitted as a `QImage` without a copy. (The tiled viewport this page
  used to describe is gone; the views dock, float and tab.)
* The GPU renderer (`docs/gpu.md`) draws the same layers through the same
  camera wherever the renderer rules choose it - Direct3D 11 on Windows,
  Vulkan on Linux - and replaces the rasteriser and nothing above it. The
  software rasteriser stays: every pixel it writes can be asserted in a unit
  test on a machine with no GPU, it draws the headless screenshots and the CI
  runs, and it is what a view falls back to when its GPU fails.

## Why the 3D view "looked weird sometimes" (2026-09-24)

Measured on the owner's archives (`Test 4 with Tin`, `plot_PW_example_data`,
`test multiple tins`, `test trimishes complex`) through the real widget before
this work, and each fixed below:

| Symptom | Cause | Fix |
|---|---|---|
| Green and orange speckle where surfaces overlap | near = far x 1e-5 on standard depth: a framed scene had 289-802 distinct depths; a surface 1 m above another lost 62% of its pixels | depth range fitted to the scene every frame, reversed Z ([Depth](#depth)) |
| Linework and far TIN edges through buildings | a constant NDC bias 1-25 times the whole scene's depth span | bias in pixel footprints of view distance; slope-scaled offset on fills ([Bias](#bias)) |
| Blank view after zooming out | the depth range was fixed at the frame; 8 notches out left 3 fragments | fitted every frame |
| Zoomed-in orthographic views cutting the model's front away | a zoom moved the orthographic eye into the model and clipped what lay behind it | the eye is stood off behind the scene |
| Surfaces drawn as black slabs | every TIN edge in dark grey: 73% of the pixels of a 229k-triangle survey | edges fade by on-screen triangle size; dense TINs default to Shaded ([Surfaces](#surfaces)) |
| Linework floating under or through the terrain | everything drawn at z = 0 | own heights, else draped, else the datum ([Linework](#linework-in-3d)) |
| One elevation, two colours | a colour ramp per surface | one ramp over all visible surfaces, with a legend |
| Flat, plastic terrain; opposite walls alike | ambient 0.35 + 0.65 abs(n . l) | hillshade: NW sun, hemispheric ambient, no abs() ([Lighting](#lighting)) |
| A stray 800 m square; 70-95 ms a frame on it | fixed grid of 162 full-length lines | sized to the scene, on the datum, one line per cell ([Grid](#grid)) |
| The grid drawn across a flat pad or a pond floor (found in review) | the grid on the datum, drawn with depth, exactly in the plane of a surface flat at its lowest, which the slope-scaled offset pushes behind it | the grid is a backdrop that writes no depth ([Layers](#the-scene-in-layers)) |
| Draped linework dashed where TIN edges cross it (found in review) | edge and line each at one depth across their width; at a low angle a pixel of slope beats the half footprint between their biases | the edges write no depth ([Layers](#the-scene-in-layers)) |
| Survey points on a slope, or on the strings through them, drawn 5x3, notched or as ragged bars (found in review) | a point's square tested pixel by pixel at its centre's depth | decided once, at its centre, against the solids before it, and drawn whole ([Points](#lines-points-fill)) |
| Beaded 1-2 px lines on a 125% display | framebuffer in logical pixels, scaled up | device pixels ([HiDPI](#hidpi)) |
| A corridor as a sliver; sides cut off in a tall view | framed by the bounding sphere, before the view had a size | projected corners, re-framed at the first real size ([Framing](#framing)) |
| A click in plan cost 104-178 ms in an open 3D view | every notification rebuilt the whole scene | the scene in layers; a selection rebuilds only its overlay ([Layers](#the-scene-in-layers)) |

## Conventions (`camera.hpp`)

World right-handed and Z-UP (survey data is Z-up; converting at the door would
mean converting every picked coordinate back). Eye space looks down -Z. Screen
pixels have their origin top-left with +y down. The camera is a value, so a
view records its view before an orbit by copying it. The model-view-projection
is applied in DOUBLE, so survey coordinates at MGA northings (6.25e6 m, where a
float's step is 0.5 m) render exactly as at the origin; a GPU path must upload
positions relative to a local origin for the same reason.

### Depth

Clip space keeps x, y in [-w, w] and z in [0, w] with **reversed Z**: NDC depth
**1 at the near plane and 0 at the far**. The framebuffer clears depth to 0 and
the test is **strictly greater**, so of two equal depths the first drawn wins
(which, with the fixed binning order, keeps a frame reproducible - Rule 7).

* **Why reversed.** A float is dense near 0 and a perspective divide crowds
  distant depths towards one end of the range; reversing puts the dense end of
  the float where the divide crowds, so the step between depths stays about
  2⁻²³ of the distance from the eye at any distance and almost whatever the
  near plane. Worked out in float for an eye inside the scene (near = far x
  1e-6, far 2 km, a strip 5 cm above the ground 0.7-1.3 km ahead, seen
  0.03 rad below level): the strip and the ground behind it round to the same
  float depth at 93.9% of 20 001 distances on standard depth (39.1% with the
  old near = far x 1e-5), and at none reversed; the reversed case is rendered
  by `RenderDepth.WithTheEyeInsideTheSceneSurfacesAKilometreAwayStillSeparate`.
  The GPU convention is the same (D32F, compare Greater).
* **The near clip plane** is clip z ≤ w - exactly w ≥ near - and a vertex cut
  there gets depth exactly 1. The far plane is not clipped; the test rejects
  anything at or below the cleared 0.
* **The range is fitted to the scene every frame** (`Camera::fitDepthRange`,
  called by the widget's paint before it renders): near and far bracket the
  eight corners of the scene's box along the view direction, with a pad of
  1/1000 of the box's diagonal. With the eye inside the box the near plane
  falls back to far x 1e-6 (`kNearFarFloor`), which reversed Z can afford.
  Fitted to a framed scene, two planes 5 cm apart a kilometre away are
  hundreds of float steps apart (`RenderDepth.ASurfaceFiveCentimetres...`).
* **Orthographic standoff.** An orthographic view has no reason to cut away
  what lies behind an eye a zoom moved into the model, so `fitDepthRange` moves
  the eye back along the view direction until the whole box is in front of it
  (`orthographicStandoff()`). Only the depths change: an orthographic ray's
  screen position does not depend on where along the view direction it
  starts (`RenderDepth.TheOrthographicStandoffMovesOnlyTheDepthsNotThePicture`).

### Bias

Linework lies exactly in the plane of the surface it is draped on, and edges in
the plane of their triangles; without a bias they z-fight into a dashed mess.
A constant bias in NDC depth cannot work once the depth range is fitted - it
was 1 to 25 times the whole scene's depth span, and a line on the ground behind
a 25 m building showed through all of it. Two parts replace it:

* **Lines and points are pulled towards the eye by a number of pixel
  footprints** (`DrawLine::depthBias`, `DrawPoint::depthBias`; the unit changed
  from NDC depth - a GPU renderer must read it this way). A footprint is the
  world size of one pixel at the primitive's own depth
  (`Camera::worldPerPixelAt`). Under both reversed projections that pull is an
  affine map of the depth itself, so it is applied to each end and
  interpolates exactly (`Rasterizer::depthPullFor`):

  - perspective: d = n (f - z) / ((f - n) z), a footprint is z · 2 tan(fov/2) / H,
    and the pull is `pixels · (2 tan(fov/2) / H) · (d + n / (f - n))`;
  - orthographic: d = (f - z) / (f - n), a footprint is orthoHeight / H, and
    the pull is the constant `pixels · (orthoHeight / H) / (f - n)`.

  The scene uses 1.5 footprints for linework, 1.0 for surface edges and 2.5
  for the selection overlay (which must win the tie with the entity's own
  first drawing).
* **Filled triangles are pushed away by one pixel of their own depth slope**
  (the slope-scaled offset a GPU calls polygon offset): a 1-2 px line samples
  the surface up to a pixel off its centre line, and at a grazing angle the
  surface there is nearer by up to a pixel's worth of slope. No constant can
  cover that without also showing lines through buildings; the triangle's own
  slope covers exactly that and nothing more. It is per triangle and
  screen-constant, so the fill loop only adds it.

Pinned by `RenderDepth.ALineOnTheGroundBehindABuildingIsHidden` (0 pixels, both
projections, both orders) and
`RenderDepth.ALineLyingOnTheGroundIsDrawnWholeOverIt` (every pixel it draws
alone).

### Framing

`Camera::frame(box)` fits the box's eight **projected corners** from the
current direction (a box projects inside the hull of its corners), on the
tighter of the two screen axes, with a 6% margin. It used to fit the bounding
sphere, which is orientation-independent but fitted a 12 km corridor as if it
were 12 km tall as well: 1.5% of the pixels. An orthographic frame respects
the aspect (audit REN-08). A box with a half-diagonal under 1 m is grown to it
first (REN-07): one survey point is framed as a box 2 m across its diagonal. The widget frames
again on every resize until the user moves the camera, because the window zooms
a new view to extents before the dock has laid it out; a frame fitted to that
size cut the sides off a tall view.

## The rasteriser, in three stages (`rasterizer.hpp`)

1. **Transform** every vertex once, in parallel, into clip space, and classify
   it into a five-bit clip code (below). A DrawList is indexed precisely so a
   vertex shared by six triangles is transformed once.
2. **Set up and bin**: clip what crosses a plane, project to pixels, widen lines
   into quads, and record each primitive in every 64×64 tile its box touches -
   in parallel over a fixed number of chunks.
3. **Rasterise** one task per tile. A tile owns its pixels, so there is no lock,
   no atomic and no false sharing, and its colour and depth stay in cache.
   Points come after every triangle and line of the pass, whole or not at all
   ([below](#lines-points-fill)).

**Determinism (Rule 7).** Chunks are contiguous ranges of the primitive stream
and a tile visits them in index order, so a tile sees its primitives in
DrawList order whatever the chunk count. With a strict depth test the frame is
reproducible bit for bit; `ThreadCountDoesNotChangeASinglePixel` renders the
same scene with 0, 1, 3 and 7 workers and compares the buffers byte for byte.
`cad::renderLayers` draws the layers one after another into the same buffer
in a fixed order, so the same holds for a whole view.

### Clipping (rewritten 2026-09-23, audit REN-01)

Five planes, in clip space, before the divide:

* the **near plane**, clip z ≤ w under reversed Z - exactly w ≥ near, and
  `setDepthRange` refuses near ≤ 0, so every kept vertex has w > 0;
* a **guard band**, |x| ≤ 2w and |y| ≤ 2w, which bounds every projected
  coordinate to within half a viewport of the image.

Triangles are clipped Sutherland-Hodgman (near plane first) and fanned; lines
Liang-Barsky. A primitive whose vertices' codes OR to zero passes straight
through, and one whose codes AND to non-zero is dropped.

* **It used to clip at w > 1e-6.** That kept the divide finite and nothing
  else: a cut vertex projected beyond INT_MAX in any view taller than ~213 px,
  and the float-to-int conversion in the binning is undefined there - a
  triangle crossing the eye plane was dropped whole. GCC's
  `-fsanitize=undefined` does not include `float-cast-overflow`, which is why no
  sanitizer run saw it; `cmake/KatanaSanitizers.cmake` now names it.
* **The cut is computed in double**; a cut vertex is small however large its
  endpoints, so rounding it back to float costs nothing.
* **A band of 2**: wider than the image so a point or a wide line centred just
  outside still draws the part that reaches in.
* Every float-to-int conversion is clamped first (`pixelFloor`).

### Lines, points, fill

* **Lines are widened in screen space** into quads with square caps and drawn
  by the triangle path, so a line keeps its pixel width at any depth and a
  sub-pixel segment still leaves a mark.
* **Points** cover the pixels whose centres lie in [c - size/2, c + size/2):
  exactly size x size wherever the point falls. It drew floor(c - h) to
  floor(c + h), one pixel too many on each axis (audit REN-10).
* **A point is decided once, at its centre, and drawn whole**
  (`Rasterizer::decidePoints`). Its square has one depth, the centre's;
  tested pixel by pixel against the surface it is draped on it lost its lower
  rows, because at elevation e the surface under a row k pixels below the
  centre is about k / tan(e) footprints nearer, past the surface's one-pixel
  push and the point's 1.5-footprint pull once k > 1: 6.4% of the pixels of
  draped survey points at the iso view, 20% at 0.25 rad and 25% at 0.12
  (`RenderDepth.SurveyPointsLyingOnASlopedSurfaceAreDrawnWhole`). A bigger
  pull would show points through the walls in front of them instead.
  **Solids hide a point; the linework of its own pass does not.** A list with
  points is swept in two parts: its filled triangles first, then every point
  is decided at the pixel of its square nearest its centre against that depth
  - the earlier passes and the list's solids - and then one sweep draws the
  lines and the points, the visible points whole. Decided after its own
  pass's lines, a survey point tied with the string through its centre (same
  pass, same pull) and vanished: the kerb points of `plot_PW_example_data`
  at 12 notches in did, where before the review they drew as ragged bars
  (`RenderDepth.SurveyPointsOnTheVerticesOfDrapedStringsAreDrawnWholeOverThem`).
  Stream order is kept in every tile - filled triangles, line quads, points -
  so nothing else changes, and the frame is the same on any number of
  threads. The 3D view's drawing pass has no filled triangles and takes one
  sweep, as before; a single list with solids and points takes two.
  Between two points of one pass the nearer still wins pixel by pixel (a
  per-tile mask of the pixels a point has drawn), and a point behind a
  building stays hidden
  (`RenderDepth.APointOnTheGroundBehindABuildingIsHidden`). The price: a
  visible point draws over up to half its size of a nearer silhouette beside
  its centre. Up to 4 096 points the decisions are made on the calling
  thread: a pool dispatch wakes every worker and waits for each, and two
  extra dispatches made `BM_SceneFrame` 5-9% slower on the median for its
  400 points. The pixels are the same either way
  (`RenderDepth.PointsDrawTheSamePixelsOnTheCallingThreadAndAcrossThreads`).
* **A pass may write no depth** (`RenderOptions::depthWrite`): it draws where
  the test passes and leaves the depth buffer as it found it, so later passes
  are tested as if it were not there, and within it what is drawn later covers
  what was drawn earlier
  (`RenderDepth.APassThatWritesNoDepthIsCoveredByWhateverIsDrawnAfterIt`).
* **No top-left fill rule** and no blending: a pixel exactly on a shared edge
  is written twice to the same result. It must be added before any blended
  pass. No anti-aliasing yet.
* **Perspective-correct colour**: c/w and 1/w are interpolated and divided.

## The scene (`cad/scene.hpp`)

Curves are tessellated here, not in the renderer, because how finely an arc is
chorded is a modelling decision. **What is drawn is what the plan draws**: an
entity is in the scene only when `cad::isDrawn` says so, and `sceneBounds`
covers drawn entities only (audit REN-03/04). Vertical exaggeration scales z
about a datum as the scene is built, so framing, draping and depth all agree.

### The scene in layers

`SceneLayers` holds five draw lists, each rebuilt only when what it depends on
changes, drawn in this order into one depth buffer:

| Layer | Holds | Rebuilt when |
|---|---|---|
| `grid` | the navigation grid | the terrain or the drawing's bounds change |
| `terrain` | surfaces and meshes | surfaces, meshes, options or exaggeration change |
| `edges` | surface edges that fade | with the terrain; recoloured (not rebuilt) by `fadeEdges` |
| `entities` | the drawing | the document's `modelRevision` moves (any command, undo, redo, open) |
| `selection` | the selected entities again, in the selection style | any document notification |

A document notification that leaves `Document::modelRevision` where it was is a
selection or current-layer change, so the widget rebuilds the overlay alone
(`RenderView.ASelectionClickRebuildsTheOverlayAndNothingUnderIt`). A vertical
exaggeration change rebuilds every layer, because every one of them is built at
exaggerated heights. `SceneBuilder::build` still makes the whole scene as one
list for callers that want that (the benchmarks, headless tools); one list
cannot carry the depth rules below, so there a surface flat at the datum shows
the grid through it.

`cad::renderLayers` draws one frame of the layers: it fits the depth range to
them and the grid, fades the edges, then draws the five in the order above.
The widget calls it and nothing else, so the tests draw exactly what the view
does. Two of the passes **write no depth**:

* **The grid is a backdrop.** It stands on the datum, exactly in the plane of
  a surface that is flat at its lowest (a pad, a pond or basin floor). Drawn
  with depth it beat that surface, which the slope-scaled offset pushes back,
  and its whole pattern showed across a flat pad: grid lines on 7 894 of
  131 804 pad pixels at the perspective iso view and 12 331-13 392 of about
  320 355 from the top (`SceneFrame.AFlatSurfaceAtTheDatumCoversTheGridStandingUnderIt`).
  Drawn first and writing no depth, it is covered by the model everywhere.
  Seen from below the datum it is still behind the terrain: it is a reference,
  not an object.
* **The edges are tested against the terrain but not written.** An edge (1
  footprint) and a line draped across it (1.5) both lie in the surface, each
  at one depth across its width; off their centres those differ by up to a
  pixel of the surface's slope, which at a low angle is more than the half
  footprint between them. The edge, drawn first, broke every draped line it
  crossed: 2.2%, 9.2% and 17.2% of the line pixels at 0.61, 0.25 and 0.12 rad
  (`SceneFrame.DrawingLinesDrapedOnASurfaceCrossItsEdgesUnbroken`). Nearer
  terrain still hides the edges, and linework is tested against the surface
  alone, which it beats everywhere. Painted in order, a later surface's
  edges would cover an earlier one's where the two coincide (a design
  repeating the existing triangles outside the works), while in the terrain
  the surface drawn first wins that tie and is the one seen; so
  `buildTerrain` emits the edges surface by surface in reverse, and the
  edges on top are the seen surface's
  (`SceneFrame.OfTwoCoincidentSurfacesTheEdgesShownAreThoseOfTheSurfaceShown`;
  without it, `plot_PW_example_data`'s dark edges went pale under the design
  surface's weaker ones).

A GPU renderer must draw the layers in this order, with the same two passes'
depth writes off.

### Linework in 3D

`SceneOptions::linework` (`LineworkHeights`) puts each drawing vertex in z; the
default is **HeightsThenDrape**:

1. a vertex with a **height of its own** (`entity::heightsOf`: a 3D string's
   `elevations`, a point's or circle's `elevation`) is drawn at it;
2. else it is **draped** on the top visible surface under it, and a chord
   between two draped vertices on the same TIN is bent at every edge it
   crosses (a walk through the neighbour table, one step per edge), so it lies
   on the surface rather than cutting ridges or floating over valleys;
3. else it goes on the **datum**: the lowest visible surface or mesh; with
   neither, the lowest height any entity carries; with none of those,
   `SceneOptions::entityElevation`.

`Heights`, `Drape` and `Datum` restrict it to one rule. Dashes are laid along
the plan of the draped path and take their heights from it. `sceneBounds` reads
the same heights per geometry kind (one for a point, text or circle, one per
vertex for a line or polyline; arcs and dimensions drape), so zoom-extents sees
the z the build uses; the widget frames what it built.

### Surfaces

* **`SurfaceStyle::Automatic`** (the default) is ShadedWithEdges up to 50 000
  triangles and Shaded above: a dense TIN's edges are all the eye can see at
  any useful zoom, and a line per edge doubled the frame.
* **Edges fade by projected size** (`SceneBuilder::fadeEdges`, every frame): the
  typical edge of a surface is the side of an equilateral triangle of its mean
  plan area; below 4 px on screen the edges are gone, from 12 px they are at
  full strength, smoothstepped in eighths so an orbit does not recolour a
  million vertices for an invisible change. An edge is a darker shade (0.55) of
  the surface under it, so it fades into the surface, not into black; a
  breakline or boundary edge is inked a 2 px amber and fades with the rest.
* **One elevation ramp** spans the lowest to the highest of every visible
  surface coloured by elevation, so a height is one colour everywhere; the
  widget draws a small legend of it with its two heights
  (`elevationRampColor` is what both use).
* Surface vertices are **shared** (one per TIN vertex, one transform each);
  slope colouring, a property of the facet, keeps private copies.

### Lighting

Baked by the scene builder (per vertex on a surface, per face on a mesh); the
rasteriser only interpolates. A **hillshade**: the sun from the north-west, 45°
up (`lightDirection` (-0.5, 0.5, 0.707), the cartographic convention, so relief
reads as raised); a **hemispheric ambient** from 0.10 for a face turned straight
down to 0.30 for one facing up, blended by the normal's z; plus
0.80 · max(0, n · sun). Never the absolute value: `abs()` lit a face turned away
from the sun as brightly as one facing it and made faces steeper than about 52°
brighter as they steepened. TIN normals are turned to face up (a height field's
outside) and summed area-weighted per vertex for smooth shading; a mesh face is
lit by its winding, so the far side of a wall is darker
(`SceneMeshes.AWallTurnedAwayFromTheSunIsDarkerThanOneFacingIt`: 0.20 against
0.77). A zero `lightDirection` turns lighting off.

### Grid

Sized to the scene: a 1-2-5 spacing for about 16 cells over its larger side,
15% past it on every side, at most 200 cells a side; standing on the **datum**
(the lowest visible surface or mesh) rather than at z = 0, and drawn as a
backdrop ([Layers](#the-scene-in-layers)), so it is under the model even where
a surface lies on the datum; the x and y axes drawn only
where the real origin is in range (survey data is kilometres from it, and axes
through the scene's centre looked like axes and meant nothing); faded into the
background towards its rim. Each line is emitted **cell by cell**: a
full-length line's quad touches every tile its bounding box does, and the old
fixed grid's 162 long diagonals cost 70-95 ms a frame at 1600x1000-1920x1080,
95% of a small scene's time; cut into cells the same grid measured about 3 ms.
With no depth written, a later grid line covers an earlier one where they
cross, so the plain lines are emitted first, then every fifth, then the axes:
no plain line breaks an axis
(`CadScene.TheGridDrawsItsAxesLastSoNoOtherGridLineBreaksThem`).

### HiDPI

The framebuffer is the widget's size times `devicePixelRatioF()` and the
`QImage` carries that ratio, so Qt draws it one to one. Line widths and point
sizes scale by `SceneOptions::pixelScale`; pan and zoom-at-cursor count in
device pixels. The owner's display runs at 125%: the view rendered at 80% of
its resolution and was scaled up nearest-neighbour, beading 1 px lines into
1-2 px steps. At `QT_SCALE_FACTOR=1.25` a 400 x 300 view renders 500 x 375
(`qt_render_view_at_125_percent`); a 960 x 640 view drew 960 x 640 and was
scaled to 1200 x 800, and now draws 1200 x 800.

## Measurements

All on the i7-1270P laptop this project is built on, shared with other agents
and on battery: **ratios and counts, not absolute times**.

### Benchmarks (`benchmarks/bench_render.cpp`)

`BM_SceneBuild` and `BM_SceneFrame` run a synthetic survey (a rolling TIN of
N x N cells over 1 km at MGA-like coordinates, 400 3D strings with heights, 400
plan strings, 400 points) through `SceneBuilder::build` and one framed frame at
1600x1000; `BM_SceneSelectionBuild` builds the selection overlay alone. Run
with `tools/compare_benchmarks.py --alternate 5 "BM_Scene|BM_Render"` against
the benchmark binary built before this work, with a second copy of the new
binary as the A/A control:

CPU ms, minimum / median of 15 samples (5 alternating rounds x 3
repetitions), 2026-09-24:

| Benchmark | before | after | after, A/A copy | before / after |
|---|---|---|---|---|
| `BM_SceneFrame/256` (131k-triangle TIN + drawing) | 55.45 / 72.65 | 13.01 / 27.02 | 10.48 / 23.30 | 4.3x / 2.7x |
| `BM_SceneFrame/512` (524k) | 131.23 / 174.22 | 21.28 / 40.45 | 20.53 / 41.17 | 6.2x / 4.3x |
| `BM_SceneBuild/256` | 27.73 / 33.25 | 26.24 / 32.49 | 29.71 / 35.26 | no change (inside the A/A spread) |
| `BM_SceneBuild/512` | 171.46 / 292.64 | 91.14 / 113.13 | 81.74 / 93.31 | 1.9x / 2.6x |
| a selection click: whole scene before, `BM_SceneSelectionBuild` after (256 / 512) | 27.73 / 171.46 | 0.08 / 0.08 | 0.08 / 0.06 | 350x / 2100x |
| `BM_RenderGroundFramedSerial/256` | 43.49 / 53.02 | 58.58 / 78.93 | 58.22 / 75.61 | 0.74x - see below |

* The frame is 4-6x faster because a dense TIN no longer draws a line (two
  triangles) per edge, its vertices are shared, and the grid is cut into
  cells: `BM_SceneFrame/256` rasterises 227 496 triangles, down from
  599 236.
* The bare-grid benchmarks (`BM_RenderGround*`) run the rasteriser alone on a
  framed ground; the A/A copies differ by up to 30% there, so only the serial
  one is readable. It is 0.74x because the camera now frames the ground's box
  instead of its sphere: the ground fills 424 186 pixels, up from 280 048
  (1.51x), and the time per pixel did not rise. `BM_RenderGroundWithin`,
  which places its camera itself, is unchanged within the noise.

### The review fixes: grid backdrop, edges without depth, whole points

Measured the same way against the binary built before them (the last row of
the table above), with a second copy of the new one as the A/A control; CPU
ms, minimum / median:

| Benchmark (samples) | before | after | after, A/A copy |
|---|---|---|---|
| `BM_SceneFrame/256` (27) | 9.66 / 11.54 | 9.60 / 10.95 | 9.74 / 11.38 |
| `BM_SceneFrame/512` (27) | 17.09 / 25.21 | 17.52 / 26.12 | 17.42 / 19.96 |
| `BM_RenderGroundFramedSerial/256` (21) | 34.73 / 43.59 | 22.94 / 40.69 | 28.32 / 45.39 |
| `BM_RenderGroundFramedSerial/724` (21) | 146.77 / 244.35 | 113.59 / 211.54 | 110.61 / 195.15 |
| `BM_SceneLayersFrame/64` (21) | - | 14.63 / 24.12 | 11.68 / 28.45 |
| `BM_SceneLayersFrame/256` (21) | - | 13.62 / 25.29 | 15.47 / 25.77 |
| `BM_SceneLayersFrame/512` (21) | - | 20.73 / 40.76 | 26.93 / 43.14 |

No row is slower than before by more than its A/A spread: `BM_SceneFrame`
draws one list with solids and points, so it now sweeps twice, and its
minimums are within 3%; the serial fill (the depth-write test in the fill
loop) did not move. None is claimed faster either - this laptop's A/A
spread reached 1.6x on a minimum in a five-round run. While both point steps
were still dispatched to the pool, `BM_SceneFrame` measured 5-9% slower on
the median (10.88 / 11.48 and 19.26 / 21.46), which is why the decisions of
up to 4 096 points are made inline. `BM_SceneLayersFrame` is new with these
fixes: it draws the same survey as `BM_SceneFrame` through
`cad::renderLayers` - five passes, the edges drawn at 64 cells - which is
what a paint of the 3D view costs; it has no before.

What the fixes changed in the headless pictures of the four archives
(against the build before them, 1200x800): `plot_PW_example_data` about
16 000 pixels at 12 notches in, perspective and orthographic - its yellow
survey points along the kerbs whole 5 x 5 squares over the strings instead
of ragged bars, and its draped contours unbroken across the TIN edges -
3 169 from the top and 243-421 at the iso and low views; every other view of
every archive under 530 pixels.

### The real archives (headless widget, 1200x800)

A scratch harness drove `RenderViewWidget` offscreen over each archive and
saved what it painted - iso, top, 12 notches in, 8 notches out, orthographic
12 in, a low orbit - with the build before this work and after it, and timed
the paint that follows a selection click (single samples on a busy machine:
read them as sizes, not ratios). What changed, looking at the pictures:

* **Test 4 with Tin** (229 462-triangle TIN, 7 820 entities): the black slab
  is gone - the surface shows its elevation colours with the drawing draped on
  it; the 800 m grid patch floating beside the corridor is a grid under the
  whole corridor; triangles rasterised at extents 1 160 944 → 468 514 (no
  line per TIN edge); the paint after a selection click 155 ms → 62 ms.
* **plot_PW_example_data** (8 overlapping surfaces, 4 469 entities): no
  speckle where design and existing surfaces overlap; one ramp 23.05-63.68 m
  across all eight with its legend; linework on the surfaces instead of at
  z = 0 below them; 8 notches out still shows the surfaces (it drew
  linework over nothing); triangles at extents 483 186 → 186 309; the paint
  after a selection click 178 ms → 37 ms.
* **test multiple tins** (4 TINs, 10-18.4 m): each TIN was its own
  blue-to-white; now the four are four heights of one ramp, and the view is
  framed on them rather than on a grid three times their size.
* **test trimishes complex** (266 meshes): the 3D strings run along the
  meshes at their own heights instead of at z = 0 beside them; the paint
  after a selection click 104 ms → 16 ms.
* At `QT_SCALE_FACTOR=1.25` the four TINs draw at the display's pixels: the
  1 px grid and edges no longer bead into 1-2 px steps.

## Outstanding

* A vertical exaggeration change rebuilds every layer; making it a transform
  (and re-lighting only) would make it free.
* No anti-aliasing, no transparency, no text in 3D (text is a point marker),
  no point clouds in 3D, no picking in 3D.
* The rasteriser's fill loop is scalar (a SIMD rewrite is its own wave); a
  dense TIN at extents is still bound by triangle setup and binning.
* Zoom-extents centres the scene's box, so in perspective the near half of a
  large flat site fills more of the view than the far half.
* `draw_list.hpp` still describes `DrawLine::depthBias` as NDC depth; it is
  pixel footprints of view distance (above).
* The GPU renderer must match four rules of this path: the grid and edges
  passes write no depth; the edges come surface by surface in reverse (as
  `buildTerrain` emits them); a point sprite is decided at its centre against
  the solids before it, not its own pass's lines, and drawn whole (a larger
  pull instead shows points through thin walls); and the layers are drawn in
  `renderLayers`' order.
* A mesh styled ShadedWithEdges (meshes default to Shaded) keeps its edges in
  the terrain list, where they write depth: linework at its own heights lying
  exactly in a mesh face can still lose pixels where it crosses a mesh edge.
* The open render defects of the audit are in `docs/audit/2026-09-23-defects.md`,
  section REN; REN-02, 05, 06, 07, 08 and 10 are fixed here and need their
  status changed there.
