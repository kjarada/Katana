# Surface display: a surface seen in plan and in 3D, in its style

On 2026-10-15 the owner asked for terrain with "multiple styles, colors ...
make it visiable in 2d". Until then a TIN surface was not drawn in the plan
view at all - it showed only in the 3D and section views - and the 3D view
coloured every surface with one shared elevation ramp. This document is the
record of how a surface is now drawn in its display style: in the plan view
(and so in every plot and sheet viewport, which share its painter), in the
3D view, in a legend, and on the command line.

The style itself - the named `SurfaceDisplayStyle`, the set the drawing keeps,
which surface is drawn in which, the `SURFACESTYLE` and `SURFACE STYLE` verbs
that edit them, and how they are saved - is the drawing's surfaces' own
(`include/katana/cad/terrain/surface_style.hpp`, `docs/terrain.md`). This
document is what is DONE with a style.

## The pieces

| Where | What |
|---|---|
| `src/katana_cad/terrain/surface_ramps.cpp` | THE ramp table: `surfaceRampNames`, `surfaceRampColour`, `surfaceRampStops` |
| `include/katana/cad/terrain/surface_colouring.hpp` | the colouring a style gives a surface's faces, resolved against the values it spans (`SurfaceColouring`), aspect sectors, band pieces, the legend |
| `include/katana/cad/terrain/surface_plan.hpp` | a surface in plan for one view: what is drawn, culled and to a level of detail (`surfacePlan`, `SurfacePlanCache`) |
| `include/katana/cad/terrain/surface_display_verbs.hpp` | `SURFACE LEGEND` and `SURFACE PLAN` |
| `include/katana/cad/scene.hpp` | the 3D scene's surfaces in their styles (`SceneSurface::display`, `applySurfaceDisplay`, `SceneLayers::legend`) |
| `src/katana_qt/terrain/surface_plan_painter.hpp` | the plan painter's surface layer, and the legend panel both views draw |
| `src/katana_qt/plan_painter.hpp` | `PlanSource::surfaces`, `PlanPaintOptions::surfaces`, the statistics |
| `src/katana_qt/terrain/surface_style_manager.hpp` | Terrain > Surface Styles... |
| `src/katana_qt/terrain/main_window_surface_display.cpp` | the window's wiring: the action, the dialog, the 3D views restyled |

## The ramps

Every ramp is five colours evenly spaced over 0 to 1 and blended linearly
between them. Five, because that is the form the 3D scene's vector kernels
read (`src/katana_cad/simd/scene_kernels.hpp`: four segments a channel): a
surface coloured smoothly by ANY ramp takes the kernel path, and the kernels'
parameters are filled from the ramp the style names
(`SceneKernels.ASurfaceInADisplayStyleOnEveryRampBuildsTheSameAtEveryLevel`).

| Ramp | Stops | Source |
|---|---|---|
| terrain (the default) | (40, 70, 140) (60, 160, 90) (235, 220, 130) (200, 130, 70) (245, 245, 245) | the 3D view's one elevation ramp since 2026-09-24, unchanged to the bit (`docs/render.md`, "Surfaces") |
| earth | #ffffd4 #fed98e #fe9929 #d95f0e #993404 | ColorBrewer YlOrBr, five classes (Cynthia Brewer, colorbrewer2.org) |
| diverging | #0571b0 #92c5de #f7f7f7 #f4a582 #ca0020 | ColorBrewer RdBu, five classes, reversed: cut blue, fill red |
| slope | (0, 255, 38) (128, 255, 38) (255, 255, 38) (255, 128, 38) (255, 0, 38) | the 3D view's slope colours since 2026-09-24 |
| grey | 32, 80, 128, 176, 224 | chosen here: equal steps, so order survives a monochrome plot |
| spectral | #2b83ba #abdda4 #ffffbf #fdae61 #d7191c | ColorBrewer Spectral, five classes, reversed |

The scene's `elevationRampColor` is now the table's terrain ramp, operation
for operation (`SurfaceRamps.TheTerrainRampIsTheScenesOldElevationRampToTheBit`
holds it at a thousand points), so every existing scene and kernel test holds.
The old slope colour was a line, red 2t and green 2 - 2t; as a five-stop ramp
its quarter stops are that line rounded (128), and between stops a colour may
now differ from the old by one level of 255. No test held the old values
between stops; the stops are held
(`SurfaceRamps.TheSlopeRampKeepsTheScenesOldSlopeColoursAtItsStops`).

## How faces are coloured

A face is coloured by its ELEVATION (drawing units), its SLOPE IN PERCENT
(rise over run times 100: 100 is 45 degrees), or its ASPECT - in eight
sectors of 45 degrees centred on north, north-east ... north-west, coloured
round a hue wheel from red at north - or by one FLAT colour.
`cad::surfaceColouring` resolves a style's rule against the values a surface
spans:

* **The range.** Each end the style gives (`rangeMin`, `rangeMax`), else the
  surface's own: its lowest and highest vertex for elevation, 0 to its
  steepest face for slope. In 3D an open end of an elevation range is the
  one range of every visible surface ("In 3D", below).
* **Smooth ramp.** The ramp at (value - low) / (high - low), clamped; a range
  narrower than the geometric tolerance is the ramp's middle, as the scene's
  one ramp always was.
* **Banded ramp.** `bands` bands of equal width over the range; below it is
  the first band and above it the last. Band k is the ramp at k / (bands - 1),
  so the first and last bands are the ramp's two ends.
* **Stops.** They override the ramp and are absolute values. Smooth, a value
  is blended between the two stops either side; banded, each stop is the lower
  edge of a band in its colour, the first band open below and the last open
  above.
* **Not finite** (a face vertical in plan has no slope): the flat colour, as
  is a level face coloured by aspect.

The legend (`cad::surfaceLegend`) says the same: one swatch a band or stop
labelled by its edges ("24.892 - 26.285", "< 2", "33 +"), the eight sectors
and "level" for aspect, or for a smooth ramp a gradient labelled at its ends.
A label has the fewest decimals, at most three, that write every edge to a
millionth of a unit (`cad::legendDecimals`).

## In plan

`paintPlan` draws every visible surface of the drawing - `PlanSource::surfaces`
and `PlanSource::surfaceStyles`, which `planSourceOf` fills from the document -
in its style, in the order the surfaces were added, **beneath the entities**:
over the imagery, the point clouds and the mesh footprints, under every line
of the drawing. So the plan view, File > Plot to PDF and every sheet viewport
draw a surface alike, through one painter. A hidden surface
(`NamedSurface::visible`) is not drawn and does not count in what Zoom
Extents frames (`planDrawnBounds`); a visible one does, so a drawing of
nothing but a surface frames it.

What is drawn is decided in `katana_cad` by `cad::surfacePlan`, for one
surface through one view, and drawn by
`src/katana_qt/terrain/surface_plan_painter.cpp`; in order:

1. **Faces** - an image of the part of the surface in view, each pixel the
   colour of the surface under its centre, at the style's opacity.
2. **Triangles** - each edge once, where it is at least four pixels long.
3. **Contours** - the minors, then the majors over them.
4. **Breaklines** - the constrained edges inside the surface, joined into lines.
5. **Boundary** - the rim, outside and holes, as rings.
6. **Points** - the vertices, as marks.
7. **Labels** - the level along each major contour, upright, haloed in the
   ground.

### Faces are an image, not polygons

Zoomed out, a dense TIN has many triangles to a pixel, and a polygon each is
most of a frame for colour nobody can see. The faces are rasterised in
`katana_cad` into a premultiplied ARGB image (the layout of
`QImage::Format_ARGB32_Premultiplied`, wrapped without a copy), one image
pixel a device pixel on screen, on a grid lined up with the view's first
pixel (`cad::planImageGrid`), and laid down through the frame like a raster.
Its cost is the pixels it covers and the triangles in view, whatever the zoom.

* **Exact to the pixel.** Elevation is interpolated at each pixel's centre,
  so a band edge is exact to the pixel - a contour line, not a staircase of
  whole triangles - and a smooth ramp is smooth across a triangle.
  Slope and aspect, properties of a face, are a colour a triangle.
* **No pinholes, nothing twice.** A pixel centre exactly on an edge two
  triangles share goes to one of them: each edge function is worked out from
  its lower-numbered vertex to its higher, the same arithmetic in both
  triangles, and a tie goes to the triangle that runs the edge that way round.
  On an eighth-of-a-unit grid 80 centres lie exactly on the square's shared
  diagonal; the image covers 6 400 pixels, and the test fails with 6 480 when
  a tie goes to both and 6 320 when to neither
  (`SurfacePlanFaces.EveryPixelOfTheSurfaceIsCoveredOnceEvenOnTheSharedDiagonal`,
  both mutations run).
* **The same at any thread count.** Rows are painted in bands of 32 across the
  task pool, each band writing only its rows and taking its triangles in
  ascending order (`SurfacePlanFaces.TheImageIsTheSameAtAnyThreadCount`).
* **On paper** the image is made at the imagery cap, as a raster's crop is
  (`PlanPaintOptions::rasterDpiCap`, 200 dpi, and the 16-megapixel cap), and
  a greyscale or monochrome plot prints it through `plotImage`.

Rejected: a `QPainter` polygon a triangle (a frame of 400 000 polygons), and
pieces of triangles cut at band edges drawn as polygons (exact, but more
polygons still, and smooth colouring needs a gradient a triangle, which
QPainter cannot draw). The 3D view does cut triangles at band edges
(`cad::bandPieces`), because a GPU draws triangles and not pixels.

### Level of detail

All in pixels of the view (a logical pixel on screen; on paper a 96th of an
inch of the page, `kPlanPaperPixelMillimetres`, so a plot keeps the detail
the screen showed at its scale):

* **Triangles under four pixels are not stroked** (`kPlanEdgePixels`), as the
  3D view's edges are gone below 4 px (`SceneBuilder::fadeEdges`): a mesh of
  one-pixel lines that close is a sheet, not triangles. A triangle whose every
  edge is shorter is tinted into the image in the triangle colour at 35 %
  (`kPlanDenseTriangleAlpha`), so a dense surface reads as a tint rather than
  disappearing. Each edge is stroked once - from the lower-numbered of its
  two triangles unless the other is out of view.
* **Points closer than four pixels apart** on average (the square root of
  the plan area a vertex) are single pixels in the image, not marks.
* **Contours closer than three pixels** apart at the surface's mean slope
  (interval over the area-weighted mean slope, in pixels) are left out: the
  minors first, then the majors (`kPlanContourPixels`). A contour whose box
  is under a pixel is a speck and is left out.
* **Lines are thinned** to half a pixel (radial distance: a vertex is kept
  when it is half a pixel or more from the last kept; the ends always) and
  clipped to the view by whole segments (`geometry::clipPolyline`), so a long
  contour seen in part costs its visible part.
* **Labels** go one every 360 px along each major contour, measured along
  the WHOLE contour from its own start - so a pan does not slide them along
  the line - half a spacing in, none on a contour under 90 px, 12 px inside
  the view, at most 400 a surface. A label too near the halo's lightness to
  be read (a dark major on the screen's dark ground) is drawn lighter, or
  darker on a light ground.

### Culling, and what is kept

The triangles in view are found through a grid of the surface's triangles
(about four a cell, at most 1024 cells a side), each candidate tested against
the view by its box - exactly the triangles a test of every one finds
(`SurfacePlanCull.TheGridFindsExactlyTheTrianglesTestingEveryOneFinds`: 304
boxes inside, across, outside, thin and empty; a grid that drops its last row
of cells fails 129 of them). A view wholly holding the surface takes every
triangle without the grid.

`cad::SurfacePlanCache`, one per view inside the plan painter's cache
(`PlanPaintCache::surfaceCache`), keeps per surface its grid, its mean and
steepest slopes, its rim and breaklines joined into lines, and its contours
for the settings last drawn - traced once by `terrain::contours` and again
only when the interval, base or major spacing change. It is keyed on the
surface itself, which is shared and immutable (an edited surface is a new
one), keeps it alive while kept, and lets a surface go at the first paint
after the drawing has lost it. A copy of the cache starts empty.

Contours whose tracing would make more than four million points
(`kPlanMaximumContourPoints`, estimated from the levels crossing each
triangle) are refused for the plan, saying why (`SurfacePlan::problem`): an
interval a hundredth of what the relief wants would fill memory and the view
with ink.

### The legend

On screen, over everything in the view's top-right corner, the legend of each
drawn surface whose style colours its faces - at most three, stacked
(`PlanPaintOptions::surfaceLegends`). It is drawn into the kept drawing, so a
mouse move costs nothing more. A plot carries none: a sheet's legend belongs
with its title block.

The plan view's headless report says what it drew of the surfaces, after the
entities, ghosts and grips, when it drew any - `PlanView1: drawn=N ghosts=N
grips=N surfaces=N triangles=N edges=N tinted=N contours=N labels=N
covered=N legends=N`, the counts of `PlanPaintStats` - and reads as it always
did when it drew none (`docs/headless.md`).

## In 3D

`SceneSurface::display` carries a COPY of the surface's style (the drawing's
set is replaced by an edit, so a pointer into it would dangle);
`cad::applySurfaceDisplay` sets it with the shading (`SurfaceDisplayStyle::shading`,
the 3D view's `SurfaceStyle`). A surface with no style is drawn as before
styles, by `SceneSurface::coloring`. With one, its `faces3d`:

* **Elevation, smooth, on a ramp** - the shared vertices coloured by the
  style's ramp, on the kernel path. Where the style leaves an end of its range
  open, the end is the one range over every visible surface coloured so,
  which keeps the scene's rule that a height is one colour on every surface
  (`docs/render.md`, "Surfaces"); a surface with both ends of its own does not
  stretch it (`SceneSurfaceStyle.ASurfaceWithARangeOfItsOwnDoesNotStretchTheOneRamp`).
* **Elevation by stops** - each vertex its colour, on the scalar loop.
* **Elevation banded** - each triangle cut where a band edge crosses it
  (`cad::bandPieces`: convex pieces of three to five corners, a corner on a
  band edge at the edge's level exactly, covering the triangle), each piece
  its band's colour lit by the triangle's normal: a band stops at its contour.
* **Slope, Aspect** - a colour a face on copies of its own, lit by the face's
  normal, as the old slope colouring was.
* **Flat** - one colour.

A style on the terrain ramp draws exactly what the scene drew before
(`SceneSurfaceStyle.AStyleOnTheTerrainRampColoursASurfaceAsTheSceneAlwaysDid`).
`SceneLayers::legend` is the legend of the first drawable surface whose style
colours its faces by value or aspect, as drawn; `RenderViewWidget` draws it
with the plan view's legend panel, and the old elevation bar for surfaces with
no style.

The window styles its 3D surfaces from the drawing's styles
(`MainWindow::styleSceneSurfaces`) when it rebuilds them
(`MainWindow::syncSceneSurfaces`), and again, queued and once for a burst, when
the styles are edited or another drawing is opened
(`MainWindow::restyleSceneSurfaces`).

## In the window

**Terrain > Surface St&yles...** (action `terrainSurfaceStyles`, icon
`SurfaceStyles`, a Terrain toolbar button beside the Surface From items) opens
the style manager, `surfaceStyleManagerDialog`: the drawing's styles listed
(the default in bold, and named below the list), every field of the chosen
style in four tabs - Plan, Colours, Contours, 3D - and a live preview of a
sample surface (a hill, a hollow and a fall, with a breakline across it)
drawn in the form's style through the plan view's own painter, with its
legend. Nothing is changed by the dialog itself: each button builds the line
a person would type and hands it to the window's executor (`docs/desktop.md`,
"One executor: the command runner"):

| Button | Line |
|---|---|
| New | `SURFACESTYLE NEW "name"` |
| Copy | `SURFACESTYLE NEW "name" FROM "chosen"` |
| Rename | `SURFACESTYLE RENAME "chosen" "name"` |
| Delete | `SURFACESTYLE DELETE "chosen"` |
| Set Default | `SURFACESTYLE DEFAULT "chosen"` |
| Apply | `SURFACESTYLE SET "chosen" key=value ...` - only the fields the form changed, in the verb's key order (`qt::surfaceStyleSetWords`) |
| In This Style | `SURFACE STYLE "surface" "chosen"` |

The form is a buffer, previewed as it is; a field that does not read (an
interval that is no number, a colour that is not `#RRGGBB`) stops Apply and
the line label says why. A reload - a `SURFACESTYLE` typed, an undo - keeps
the form's unapplied edits while the style they were made to is unchanged.
Every widget's object name is in the dialog's header; the Surfaces panel's
Edit Styles... button triggers the same action.

## On the command line

Both read only, so neither is an undo step; both reach `katana_mcp` through
`katana_run_commands`, as every interpreter verb does.

```
SURFACE LEGEND <name> [3D] [JSON]
SURFACE PLAN <name> [AREA x0,y0,x1,y1 | VIEW [<id>]] [pixels=W,H]
```

`SURFACE LEGEND` is the legend of its faces in its style: in plan
(`planFaces`) or in 3D (`faces3d`, over the surface's own heights - a 3D view
stretches an open range over every visible surface, which a verb has no view
to know). A head record, then one a band, stop or sector:

```
legend surface="ground" style="Elevation Bands" view=plan faces=elevation kind=bands entries=10
entry index=1 colour=#28468C from=24.892 to=26.2847 label="24.892 - 26.285"
```

`SURFACE PLAN` is what the plan view draws of it looking at AREA (default: the
whole surface) through a view of W x H pixels (default 1600 x 1000, the plan
benchmark's), or at what the window's plan view shows (VIEW, through the one
scope parser; headless it is refused naming AREA): the triangles in view,
stroked and tinted, the image and the pixels covered, the contour levels and
runs and whether the minors and majors are drawn, labels, points, the
boundary and breakline runs, and the scope record. A surface hidden in the
drawing is still described (`visible=no`): hiding is the picture's. Refused:
no such surface (NotFound), a word neither takes, WHERE ("a surface is not
entities"), a scope other than AREA or VIEW, a bad AREA or pixels=.

No MCP tool of its own: the legend as JSON (`SURFACE LEGEND name JSON`) is the
structured form an agent wants, and it comes back through
`katana_run_commands` already.

## Speed

Measured on 2026-10-15 on a 24-thread machine (Google Benchmark reports
2496 MHz), in a scratch Release build: this branch's sources at the main
build's flags (-O3), linked against the main checkout's libraries - not the
integration build, which had not been made, and not alternated against
another binary, so read the times as sizes. Three repetitions of at least
0.3 s, medians. The surface is 448 x 448 cells, 401 408 triangles over a
kilometre at map-grid coordinates; the view 1600 x 1000 at extents (the
kilometre over 950 px), and zoomed in five and twenty-five times.

`BM_SurfacePlan` (`benchmarks/bench_surface_plan.cpp`), `cad::surfacePlan`
alone with the cache warm, milliseconds:

| Style | extents | x5 | x25 |
|---|---|---|---|
| boundary only | 0.41 | 0.18 | 0.02 |
| triangles (401 408 tinted at extents; 43 528 edges stroked x5) | 15.5 | 1.10 | 0.04 |
| contours, 1 m, labelled (73 545 vertices, 62 labels at extents) | 0.85 | 0.35 | 0.09 |
| elevation bands (902 500 pixels at extents, 1.6 M zoomed) | 12.8 | 7.5 | 6.5 |
| smooth elevation | 12.2 | 6.7 | 5.6 |
| slope bands | 12.4 | 3.2 | 2.0 |
| everything at once | 20.9 | 8.8 | 6.7 |

The first paint of a surface (`BM_SurfacePlanFirst`) also builds the cull
grid and traces the contours: 34 ms for the boundary alone, 42 ms with
contours, 62 ms for everything - once per surface per view.

`BM_PlanPaintSurface` (`benchmarks/qt/bench_plan_surface.cpp`), a whole frame
through `paintPlan` onto a 1600 x 1000 image, the view panned a pixel each
frame, milliseconds:

| Style | extents | x5 | x25 |
|---|---|---|---|
| boundary only | 1.2 | 0.37 | 0.18 |
| triangles | 18.7 | 12.8 | 1.8 |
| contours | 24.3 | 5.2 | 1.0 |
| elevation bands | 15.4 | 9.6 | 8.4 |
| everything at once | 46.0 | 30.4 | 11.8 |

So a 400 000-triangle surface pans at 20 frames a second or better in the
heaviest style and 40 or better in any one of them, and a mouse move over it
costs nothing (the kept drawing). The face image is most of a banded frame
(its pixels); stroking is most of a contour frame (73 545 vertices at
extents) and of the triangles zoomed in (49 003 edges).

The 3D terrain built in a style (`BM_SurfaceSceneStyled`, `SceneBuilder::buildTerrain`
of the same surface): smooth elevation on the kernels 4.5 ms; elevation bands,
cut at their contours into 455 732 triangles, 27.6 ms; slope bands, a colour a
face, 70.2 ms - paid when the terrain is rebuilt (a style or surface edit, a
change of exaggeration), not each frame.

To measure again: `katana_benchmarks --benchmark_filter=Surface` and
`katana_qt_benchmarks --benchmark_filter=PlanPaintSurface`, alternated against
another build with `tools/compare_benchmarks.py --alternate`.

## Tests

* `tests/cad/terrain/test_surface_colouring.cpp` - the ramps' published stops
  and the scene's old constants, the table's names and fallback, aspect
  sectors either side of each edge, bands and stops by value, smooth stops,
  the range, slope in percent, band pieces covering a triangle exactly, the
  legends and their decimals.
* `tests/cad/terrain/test_surface_plan.cpp` - the cull grid against testing
  every triangle, the four-pixel edge rule and its tint, the colour of a known
  face in a known band either side of a band edge, coverage on the shared
  diagonal, opacity, slope and aspect faces, the image at any thread count, a
  contour at its level and its label, minors hidden, contours traced once and
  the refusal of a too-fine interval, the rim and a breakline joined, points
  as marks and as pixels, nothing out of view, the image grid and its cap.
* `tests/cad/terrain/test_scene_surface_style.cpp` and
  `SceneKernels.ASurfaceInADisplayStyleOnEveryRampBuildsTheSameAtEveryLevel` -
  the 3D colouring by style (above).
* `tests/cad/terrain/test_surface_display_verbs.cpp` - both verbs' replies,
  worked out on a 10 x 10 square, and every refusal.
* `tests/qt_widgets/terrain/test_surface_plan_painter.cpp` - a surface inking
  exactly its own 80 x 80 pixels where nothing was, a known face its band's
  colour, each built style inking its own way against the same paint without
  the surface, hidden drawing nothing, the legend on screen and never plotted,
  contours traced once through the painter's cache.
* `tests/qt_widgets/terrain/test_surface_style_manager.cpp` - each button's
  line word for word, Apply naming only what changed, a bad field stopping
  Apply, the preview drawing the form's style, and the lines changing the
  drawing through the interpreter, undone as one step, the dialog following.
* `qt_a_surface_is_drawn_in_plan_in_its_style_headless` - the sample terrain
  in the real window in Standard, Triangles and Elevation Bands, each paint's
  report read against the first; `qt_the_surface_style_manager_runs_its_lines_headless`.
* `cli.surface_legend_lists_the_bands_of_a_surfaces_style`,
  `cli.surface_plan_says_what_the_plan_view_draws_of_a_surface`,
  `cli.surface_plan_of_no_such_surface_is_refused`.

## Decisions

* **Faces as an image in `katana_cad`, not QPainter polygons** - above. The
  rasteriser is a few dozen lines with an exact tie rule, testable without Qt.
* **One colouring rule for plan, 3D, legend and verb** (`SurfaceColouring`) -
  four places that each worked out a band would be four places to disagree.
* **The 3D scene keeps its old path for a surface with no style.** Every
  existing scene and kernel test holds unchanged, and a style on the terrain
  ramp is proven to draw the same.
* **Five-stop ramps.** Arbitrary stop counts would push every styled surface
  off the kernels; a style that wants more colours gives stops.
* **The level of detail follows the 3D view's 4 px** rather than a number of
  its own, so the two views agree on when a triangle is too small to show.
* **Labels placed along the whole contour**, not the visible run, so a pan
  does not slide them; rejected: a label a visible run, which jumped on every
  pan.
* **The legend is screen furniture**, three at most, over the drawing; a
  legend panel docked in the window was rejected as one more panel for what
  the view itself can say where the surface is.
* **No new MCP tool** - the verbs' JSON reaches an agent already.

## Not done

* A legend on a plot or sheet (a sheet's title-block legend), and a View
  menu switch to hide the screen legend (today: a style without faces has
  none).
* The plan view's faces are not shaded (no hillshade in plan); a hillshade
  face colouring would be a fifth `SurfaceFaces` value and a contract change.
* Smooth elevation in plan evaluates the ramp at every pixel; a lookup table
  would roughly halve the faces' cost at extents.
* Slope and aspect faces in 3D cost 70 ms on a 400 000-triangle surface
  (a colour a face on copies of its own, as the old slope colouring did); the
  slope of each face could be kept with the surface.
* The minors are hidden by the surface's MEAN slope, so on a site of steep
  batters and flat pads they go from the flats when the batters crowd them.
* The 3D view's legend shows the first styled surface only.
* Labels are not kept off each other or off the drawing's own labels (the
  label placer, `docs/annotation.md`, places entity labels only).
