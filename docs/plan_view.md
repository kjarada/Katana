# The plan view: one painter for the screen, the plot and every sheet

The plan drawing - imagery, point clouds, mesh footprints, every entity
through its layer and style, library linestyles and symbols, hatches,
dimensions, text and the alignment overlay - is painted by ONE function,
`katana::qt::paintPlan` (`src/katana_qt/plan_painter.hpp`). The plan view
(`ViewportWidget`) paints through it, its tool previews go through
`paintPlanGeometry`, and a plot goes through `plotPlanToPdf`, which needs no
widget at all. A sheet with many viewports - plan viewports turned to an
alignment, each with its own scale and hidden layers - is `paintPlan` called
once per viewport into one page.

Until 2026-09-24 this code was `ViewportWidget`'s own members. A plot
borrowed the live widget: it swapped the view's transform for the sheet's,
set a paper flag on the widget, painted, and put everything back. That tied
every plot to one widget on the GUI thread, gave a sheet one unrotated
transform, and left the caches as widget members nobody else could own.

## The painter

```cpp
PlanPaintStats paintPlan(QPainter& painter, const PlanSource& source,
                         const PlanFrame& frame, const PlanPaintOptions& options,
                         PlanPaintCache& cache);
```

| Argument | What it is |
|---|---|
| `PlanSource` | What is drawn: the model, the style library and its generation, the spatial index, the selection, the window's reference data and meshes. `planSourceOf(document)` fills the document's part. Every pointer but the model may be null; each null leaves out only what it provides. |
| `PlanFrame` | How it is looked at: the `ViewTransform` sized to the viewport, a `rotation` (counter-clockwise, radians, about the viewport's centre), the viewport's `origin` on the device, whether to `clip` to it, and the view's hidden layers (`LayerOverrides`) and hidden reference layers. |
| `PlanPaintOptions` | The medium: `Screen` or `Paper`, pixels per millimetre, the plot settings on paper; what is drawn besides the entities (grid, rasters, point clouds, mesh footprints, alignments); and the screen-only speed switches (`thinLines`, `symbolSprites`, `clipLines`). |
| `PlanPaintCache` | What is kept between paints, owned by the CALLER (below). |

It returns `PlanPaintStats`: entities drawn, symbol stamps and how many came
from a sprite, displays resolved, lines clipped - for a view's statistics and
for the tests that cannot look at pixels.

Other entry points in the same header:

* `visibleBox(frame)` - the model box a frame shows; for a turned frame, the
  box around the turned rectangle.
* `planDrawnBounds(source, layers, hiddenReferences)` - everything a view
  with those hidden layers draws: its entities, its visible reference layers,
  the shown meshes and the alignments. Zoom Extents frames it, and a plot's
  Fit fits it.
* `sheetFrame(settings)` - the frame a plot draws through: the printable area
  (paper less margins) at the sheet's scale, clipped.
* `pdfResolutionFor(dpi)` - the whole-number resolution a `QPdfWriter` is
  given and the painter scale that makes the sheet's exact dpi land on it.
* `plotPlanToPdf(path, settings, source, layers, hiddenReferences, cache,
  title)` - one page, titled, `Katana` as its creator.

### Reentrant, and off the GUI thread

`paintPlan` reads its arguments and writes nothing but the painter and the
cache. Two threads, each with its own `PlanPaintCache`, may paint the same
drawing at once onto a `QImage` and a `QPdfWriter`; that is what plotting a
sheet set in the background needs. The model must stand still while it is
painted - the GUI thread paints between commands, and a background plot must
work on a snapshot or hold edits off.

### The caches

`PlanPaintCache` (one per view, one per plotting thread, never shared):

| Kept | Keyed on | Why |
|---|---|---|
| Flattened library definitions | the library generation | thousands of coded points would otherwise flatten their symbol every frame |
| Fonts | on screen one per whole pixel size; on paper one font, scaled | making a `QFont` by family name per label was a measurable part of a frame |
| Raster images, point-cloud colours | the reference layer's id (and colour mode) | converting tens of megabytes of RGBA per repaint |
| Symbol sprites | symbol, pen, size, quarter-pixel phase; all dropped when the scale, paper scale, device ratio or library generation changes | see below |

Per paint, not kept (the tables they read can change between paints):

* **Display resolution.** Everything an entity's look depends on besides its
  geometry - the layer walk, `resolveDisplay`'s table lookups and string
  copies, the linetype and hatch resolution, the pens, the flattened
  linestyle - is worked out once per distinct layer, style and own colour,
  and reused by every entity that shares it. The generated survey drawing
  (below) has 87 such combinations for 27 600 entities.
* **Dash patterns.** A model linetype's pixel pattern, per linetype and pen
  width. It is the linetype's contents at this scale, and those can change
  between two paints with nothing a cache could key on: the Style Manager's
  `updateLinetype`, its undo, a delete and re-create under the same name.
  Kept across paints by name, they drew the old dashes until the view was
  zoomed, and a second plot at the same scale printed them. They are worked
  out at display resolution, once per distinct display a paint, so keeping
  them longer saved a few dozen small vectors a frame at most.
* **Stamp extents.** A symbol's reach and whether it is under three pixels
  (then drawn as a dot) are the same wherever it is put, so they are worked
  out once per symbol and size, and a stamp that is culled or blitted from a
  sprite is never built.

The plan view keeps one more thing: **the drawing itself**, painted into an
image the size of the widget in device pixels. A paint whose view, drawing,
layers, library, selection, reference layers and meshes are all as they were
only lays that image down and draws the view's own furniture over it - tool
previews, the snap marker, the selection box, the prompt. The key
(`ViewportWidget::DrawingKey`) holds the transform and device ratio, the
number of document notifications (every command, undo, selection, library
and current-layer change), the model revision, the library generation, a
fingerprint of the selection's ids (a caller that changes the selection and
only repaints still sees it), fingerprints of the reference layers' and
meshes' visibility and style (the panels change those without a
notification), the view's hidden layers and references, the grid and the
line option. A mouse move, a snap marker or a rubber band then costs the
furniture, not the drawing.

## Screen and paper

The two media differ only where they must, and the difference is in one
place, the painter:

| | Screen | Paper |
|---|---|---|
| Line width | a hairline: 1.5 px, or 1 px with Thin screen lines | the line weight in millimetres |
| White pens | white | black (decision D7, `cad::paperColour`) |
| Selection, locked-layer fading | shown | not printed (audit QT-26) |
| Plain point cross | 4 px either side | 1 mm either side |
| Alignment overlay | 2 px line, 1 px ticks 6 px long, 11 px labels | 0.5 mm line, 0.25 mm ticks 1.5 mm long, 2.5 mm labels |
| Hatch lines | cosmetic hairline | 0.13 mm |
| Solid hatch | alpha 90, so the drawing under it shows | opaque |
| Text | a font per whole pixel size, hinted for the screen | one font scaled to the exact fractional height |
| Grid, rasters, point clouds, mesh footprints | drawn | not drawn (imagery at plot resolution needs a resolution cap first: an A1 page at 300 dpi is 70 megapixels) |
| Clipping lines to the view, symbol sprites | on | never: a plot stays vector |

Before 2026-09-24 the marks were device pixels on paper too, so their size on
the page followed the resolution: the point cross was 0.68 mm at 300 dpi and
0.34 mm at 600, the chainage labels 0.93 mm at 300 dpi.

### Rotated viewports

`PlanFrame::rotation` turns the drawing about the viewport's centre through
the painter. Culling uses `visibleBox`, the box around the turned rectangle
(for a 200 x 100 frame at 30 degrees, +-111.6 x +-93.3 where the unturned
frame shows +-100 x +-50), grown by the furthest symbol reach, so an entity
in a corner of a twisted viewport is drawn. Symbol sprites are not used under
a turn (a stamp would not land on its pixel grid); the point-cloud splat and
the grid are screen furniture and are not turned yet.

## The plot

`plotPlanToPdf` and the headless `--plot`:

* **Clipped to the printable area.** The frame is the paper less its margins,
  clipped; at a fixed scale the drawing stops at the margin instead of running
  to the paper's edge.
* **The dpi is a double** (audit QT-27): the writer gets the nearest whole
  resolution and the painter the scale to the exact one, so a sheet at
  300.9 dpi is drawn at 1 : 1000 and not 1 : 1003.
* **Fit fits what the view draws** (`ViewportWidget::fittedPlot`, over the
  same `planDrawnBounds` Zoom Extents frames): its layers, reference layers,
  meshes and alignments - not the spatial index's bounds, which never shrink,
  count every arc's whole circle and hidden layers, and miss alignments (audit
  QT-12, GEO-01). The site-plan sample's A3 plot is now centred with its
  alignment on the sheet.
* **`fitScale` takes a line with no height** (audit CAD-15): the zero
  dimension asks nothing of the sheet. A single point still has no scale.
* **Title and creator** are set: the project's name and Katana.

`tools/check_plot.cmake` (the `qt_plot_headless` test) checks, besides a PDF
with the drawing in it: one page, the title and creator (`pdfinfo`), and a
1 : 50 plot of the sample rendered at 10 dpi with no ink in the outer two
pixels all round (`pdftoppm`) - a check the build before the clip fails.

## Speed

The plan view repainted the whole drawing on every mouse move, and a frame of
a 28 000-entity survey drawing at zoom extents cost 270-900 ms. Measured
where the time went (a scratch harness on the owner's archive): QPainter
stroking every line with an antialiased 1.5 px pen, 14 000 symbols drawn from
scratch each frame, per-entity display resolution, and plain polylines handed
to QPainter whole when a sliver of them showed.

### The benchmark

`katana_qt_benchmarks` (`benchmarks/qt/`, built with `KATANA_BUILD_BENCHMARKS`
where the application is built) links the plan view's sources and paints a
generated survey drawing (`benchmarks/qt/survey_drawing.*`): 27 600 entities
in the make-up of the owner's 28k-entity archive - 16 000 coded points in
library symbols, 10 000 strings in library linestyles, 250 long contours,
labels, arcs, hatched lots and dimensions - at map-grid coordinates,
deterministic through a SplitMix64 stream, saved and reopened so its spatial
index is an opened project's. At 1600 x 1000:

* `BM_PlanPaintExtents`, `BM_PlanPaintZoomed` - a whole frame at extents and
  zoomed in five times, panned a pixel each iteration so nothing is reused,
  through the widget; `...ThickLines` the same at the 1.5 px look.
* `BM_PlanPaintCursorMove` - the mouse moving over an unchanged drawing.
* `BM_PlanPlotA1` - one A1 sheet at 1 : 2500, 300 dpi.
* `BM_PlanPainter/<zoom>_<measures>` - the painter alone with the thin pen,
  clipping and sprites switched on one at a time, with counters of what each
  paint did.

### Measured

Release, `tools/compare_benchmarks.py --alternate 3` (three rounds, order
reversed each round, three repetitions each: nine samples a cell), the build
before the painter (`main` plus the benchmark) run twice as an A/A control.
The laptop was on battery and shared with other agents' builds, so the ratios
are the result and the absolute times are not; the A/A pair shows the noise.
Milliseconds, minimum / median:

| Benchmark | before | before, again (A/A) | after | after / before |
|---|---|---|---|---|
| `BM_PlanPaintExtents` (27 600 drawn) | 1372 / 1419 | 1328 / 1488 | 372 / 410 (thin lines) | 3.7x / 3.5x faster |
| the same at the 1.5 px look | | | 880 / 955 (`...ThickLines`) | 1.56x / 1.49x faster |
| `BM_PlanPaintZoomed` (2 129 drawn) | 319 / 346 | 290 / 361 | 95 / 112 (thin lines) | 3.4x / 3.1x faster |
| the same at the 1.5 px look | | | 230 / 254 (`...ThickLines`) | 1.39x / 1.36x faster |
| `BM_PlanPaintCursorMove` | 1215 / 1472 | 1203 / 1409 | 2.6 / 3.2 | about 470x faster |
| `BM_PlanPlotA1` | 4330 / 5121 | 4540 / 4935 | 4499 / 5153 | unchanged (inside the A/A spread) |

The A/A pair spreads by up to 9% (minimum) and 5% (median); every change
above but the plot's is far outside it.

What each measure buys, from the painter-level captures of one binary (the
same binary run twice as its own control):

| `BM_PlanPainter/...` | run | run again (A/A) | against `..._none` |
|---|---|---|---|
| `extents_none` (1.5 px, every vertex, every stamp stroked) | 1266 / 1339 | 1161 / 1329 | |
| `extents_clip` | 1231 / 1279 | 1231 / 1271 | no change: nothing at extents runs off the view |
| `extents_sprites` (1 975 of 16 000 stamps; the rest are dots) | 968 / 1033 | 952 / 1015 | 1.3x faster |
| `extents_thin` | 463 / 487 | 438 / 483 | 2.7x faster |
| `extents_all` | 411 / 435 | 388 / 411 | 3.1x faster |
| `zoomed_none` (x5, 2 129 drawn) | 298 / 348 | 291 / 338 | |
| `zoomed_clip` (60 lines clipped) | 268 / 310 | 273 / 293 | 1.1x faster |
| `zoomed_all` | 101 / 114 | 96 / 110 | 3.0x faster |
| `deep_none` (x25) | 55.0 / 56.3 | 44.3 / 54.3 | |
| `deep_clip` | 45.5 / 53.4 | 45.5 / 46.4 | inside the A/A spread |

And on one of the owner's archives, 'Test 4 with Tin' (7 820 strings), with
no library - `KATANA_BENCH_ARCHIVE=<the .12da> katana_qt_benchmarks
--benchmark_filter=Archive`, which skips when the variable is unset - zoomed
to a fifth of the extent about its centre (74 entities drawn, 50 clipped):
`zoomed_none` 5.02 / 6.43 and again 4.98 / 5.66, `zoomed_clip` 3.22 / 3.77
and again 3.60 / 3.94: clipping is about 1.5x faster there.

Reading them:

* **Thin screen lines** is the largest single step, as the scratch
  measurement said (a cosmetic 1 px pen strokes 5-8x faster than the 1.5 px
  one, because Qt's fast antialiased-line path needs a width of at most one
  pixel).
* **The kept drawing layer** takes a mouse move from a whole frame to a
  couple of milliseconds, whatever the drawing's size.
* **Symbol sprites**: at extents most of the generated drawing's 16 000
  stamps are under three pixels and drawn as dots; the rest come from about
  two thousand sprite blits.
* **Clipping to the view** is worth little on the generated drawing (its
  400-vertex contours: 1.1x zoomed five times, nothing measurable at 25x)
  and about 1.5x on the owner's TIN archive zoomed five times, where a frame
  is already 5 ms. Qt's own stroker drops most of a path outside the device,
  so the scratch figure of 221 ms for 159 partly visible strings was not
  reproduced at this framing; where that case came from is still open.
* The plot's time is unchanged: it was never the drawing's bottleneck on the
  sheet (startup and import are), and a plot stays vector.

### What does not change the look

At the 1.5 px look, every one of these measures paints the same pixels as
before, proven with headless screenshots of the site-plan sample (a copy),
'Test 4 without tin' and 'plot_PW_example_data' against the build before:
0 pixels of the plan view differ (only the status bar's frame-time text).
Two measures are exact only to within antialiasing, and say so in their
tests:

* **Clipping** keeps each segment that reaches the view WHOLE
  (`geometry::clipPolyline` with `PolylineClip::WholeSegments`) and drops
  only those that miss the view grown by the pen's reach and a pixel. A
  segment cut at the view's edge is the same line in exact arithmetic but
  not in Qt's fixed-point rasteriser. Kept whole, a zig-zag leaving and
  re-entering the view ten times paints within 3 grey levels of the whole
  line at 1.5 px and within 8 along the view's edge at 1 px: Qt's
  rasterisers are not quite local to a segment. Only solid pens are
  clipped; a dash pattern starts at the start of a line.
* **Sprites** are rasterised at the stamp's quarter-pixel position and
  blitted onto whole device pixels, so a stamp is at most an eighth of a
  pixel from where a stroked one would be.

### Thin screen lines

View > **Thin screen lines (faster)**, on by default: every plan-view line a
cosmetic one-pixel pen. Off, the 1.5 px hairline the plan view drew before -
one click away. A process-wide choice (`ViewportWidget::setThinScreenLines`)
that every plan view reads at its next paint. Plots keep their
paper-millimetre weights either way. The headless `--trigger
ThinScreenLinesAction` turns it off for a screenshot.

### The frame statistic

The plan view reports each paint in the status bar, as the 3D view does:
`Plan  27886 drawn  192.9 ms` when the drawing was painted, and
`Plan  27886 drawn  192.9 ms (kept, 0.4 ms)` when a frame only laid the kept
drawing down. `lastFrameMilliseconds`, `lastDrawingMilliseconds` and
`drawingPaintCount` give the same to a test.

## Tests

* `tests/qt_widgets/test_plan_painter.cpp` - turned frames (culling box,
  entities in turned corners), the plot's frame and margin clip, the paper
  marks in millimetres at two resolutions each (the point cross, the hatch
  lines' 0.13 mm, and the alignment overlay's tick and its ink as a whole),
  the opaque solid hatch, fractional paper text from one font, the PDF
  resolution, Fit (a hidden layer's stray and an alignment beyond the
  entities, worked by hand), clipping against the whole line, one resolution
  per display, thin and hairline widths, sprites on screen and vector on
  paper, a linetype edited between two paints through one cache, and the
  kept drawing (a mouse move keeps it; a command, a zoom, a hidden layer and
  a silent selection change repaint it).
* `tests/geometry/test_polygon.cpp` `PolygonClipPolyline.*` - the clipper, in
  both modes, by hand and as a property over random lines.
* `tests/cad/test_plot.cpp` `Plot.ALineWithNoHeightFitsOnItsLengthAlone`.
* `qt_plot_headless` - above.

## Still open

* The case the scratch harness measured clipping on (221 ms in 159 partly
  visible strings of the TIN archive) was not reproduced: at a fifth of the
  extent about its centre the frame is 5 ms. Clipping dashed lines (a dash
  offset per run) is not done; if that case was dashed strings, it is where
  the time went.
* The point-cloud splat and the grid under a turned frame; rasters, clouds and
  mesh footprints on paper (a resolution cap, and footprints in paper
  millimetres).
* Sheets: several viewports on one page (`paintPlan` per viewport with its
  own `origin`, `clip` and `rotation`), a frame and title block, multi-page
  output, plotting on a worker thread with its own cache.
* A GPU linework layer under the QPainter overlay, for drawings where even
  the thin pen is too slow.

## Grips and the drafting aids

With a selection and no tool running the view draws the selection's grips
and forwards its mouse and keys to `drawing::GripController`
(`src/katana_qt/drawing/grip_controller.hpp`) before its own selection
handling; a grip drag snaps from the grip's old position and, with no
object snap, is constrained by the document's drafting settings (ortho,
polar, locks), whose label is drawn beside the cursor. The gesture and the
rules are `docs/drawing.md` ("Grips", "Precision input").
