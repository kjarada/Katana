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
(`crates/katana_cad/src/terrain/surface_style.rs`, `docs/terrain.md`). This
document is what is DONE with a style.

## The pieces

| Where | What |
|---|---|
| `crates/katana_terrain/src/colour_ramps.rs` | THE ramp table, plain data with the source of each ramp's colours: `colour_ramps`, `colour_ramp_names`, `find_colour_ramp` ("The ramps") |
| `crates/katana_cad/src/terrain/surface_style.rs`, `crates/katana_cad/src/terrain/surface_colouring.rs` | the surface styles' view of it, in the drawing's colours: `surface_ramp_names`, `surface_ramp_stops`, and the lookup arithmetic the 3D kernels are held to (`surface_ramp_colour`) |
| `crates/katana_cad/src/terrain/surface_colouring.rs` | the colouring a style gives a surface's faces, resolved against the values it spans (`SurfaceColouring`), aspect sectors, band pieces, the legend |
| `crates/katana_cad/src/terrain/surface_plan.rs` | a surface in plan for one view: what is drawn, culled and to a level of detail (`surface_plan`, `SurfacePlanCache`) |
| `crates/katana_cad/src/terrain/surface_display_verbs.rs` | `SURFACE LEGEND` and `SURFACE PLAN` |
| `crates/katana_cad/src/scene.rs` | the 3D scene's surfaces in their styles (`SceneSurface::display`, `apply_surface_display`, `SceneLayers::legends`) |
| `crates/katana_window/src/terrain/surface_plan_painter.rs` | the plan painter's surface layer, and the legend panel and the stack of them (`paint_surface_legend_stack`) both views draw |
| `crates/katana_window/src/plan_painter.rs` | `PlanSource::surfaces`, `PlanPaintOptions::surfaces`, the statistics |
| `crates/katana_window/src/terrain/surface_style_manager.rs` | Terrain > Surface Styles... |
| `crates/katana_window/src/terrain/main_window_surface_display.rs` | the window's wiring: the action, the dialog, the 3D views restyled |

## The ramps

Every ramp is five colours evenly spaced over 0 to 1 and blended linearly
between them. Five, because that is the form the 3D scene's vector kernels
read (`crates/katana_cad/src/simd/scene_kernels.rs`: four segments a channel): a
surface coloured smoothly by ANY ramp takes the kernel path, and the kernels'
parameters are filled from the ramp the style names
(`scene_kernels::a_surface_in_a_display_style_on_every_ramp_builds_the_same_at_every_level`).

| Ramp | Stops | Source |
|---|---|---|
| terrain (the default) | (40, 70, 140) (60, 160, 90) (235, 220, 130) (200, 130, 70) (245, 245, 245) | the 3D view's one elevation ramp since 2026-09-24, unchanged to the bit (`docs/render.md`, "Surfaces") |
| earth | #ffffd4 #fed98e #fe9929 #d95f0e #993404 | ColorBrewer YlOrBr, five classes (Cynthia Brewer, colorbrewer2.org) |
| diverging | #0571b0 #92c5de #f7f7f7 #f4a582 #ca0020 | ColorBrewer RdBu, five classes, reversed: cut blue, fill red |
| slope | (0, 255, 38) (128, 255, 38) (255, 255, 38) (255, 128, 38) (255, 0, 38) | the 3D view's slope colours since 2026-09-24 |
| grey | 32, 80, 128, 176, 224 | chosen here: equal steps, so order survives a monochrome plot |
| spectral | #2b83ba #abdda4 #ffffbf #fdae61 #d7191c | ColorBrewer Spectral, five classes, reversed |

**One table.** The ramps are rows of `katana_terrain::colour_ramps()`
(`crates/katana_terrain/src/colour_ramps.rs`):
plain data, three bytes a colour, each row with the source of its colours (the
Source column above is that field). It sits in the `terrain` layer because that
is the lowest layer both users may see (the crates' dependencies: `cad` may
not see `interop`, and `interop` may not see `render` or `cad`; `core` could
hold it, but a colour ramp is no concern of `core`). The surface styles read it through
`katana_cad::terrain::surface_colouring::surface_ramp_stops` (the drawing's `Color`, alpha 255) and `RASTER SHADE` /
`RASTER SLOPE` through `katana_interop::geo::colour_ramps::built_in_ramp` (which gives each stop its
place along the range, 0, 1/4, 1/2, 3/4 and 1, the form GDAL's colour map
reads); both find a ramp by name through the one lookup, `katana_terrain::colour_ramps::find_colour_ramp`
(any case), and the style form, the shading dialog and the shading verb's
refusals list the table's names, so a ramp added to the table is offered
everywhere without a second edit
(`colour_ramps::the_shadings_ramps_are_the_surface_styles_ramps_stop_for_stop`,
`surface_ramps::every_style_ramp_is_the_one_tables_ramp_at_full_alpha_and_its_names_are_the_tables`).

Until 2026-10-15 `RASTER SHADE` and `RASTER SLOPE` had a table of their own
(`crates/katana_interop/src/geo/colour_ramps.rs`) with four of the same names -
terrain, diverging, slope and grey - and different colours, so `ramp=terrain`
was two maps, and an earlier version of this file called the styles' table
"THE ramp table" while the other existed. Its terrain was the hypsometric green
to white, (38, 115, 0) to (255, 255, 255); its diverging and slope were
ColorBrewer's eleven-class RdBu and RdYlGn at five stops; its grey ran 0 to 255
in two stops; and it refused `ramp=spectral` and `earth`.

**Decided (the owner had not chosen; this was taken to remove the second way of
doing it, and is the one to reverse if the shading's look is wanted):** the
surface styles' colours are the table. They are what the person sees in the
window and in 3D, and the scene's terrain ramp is held to the bit by its
kernels (`surface_ramps::the_terrain_ramp_is_the_scenes_old_elevation_ramp_to_the_bit`).
`RASTER SHADE` and `RASTER SLOPE` adopt them. The old names stay accepted: the
four names the shading had are in the table, so `ramp=terrain`, `diverging`,
`slope` and `grey` still work and mean the table's ramps; there is no alias to
add. `earth` and `spectral` are accepted by the shading now, too. Rejected:

* *The shading's colours win.* The 3D view's elevation colours would change,
  and with them the scene kernels' constants and the tests that hold them to
  the bit, for every picture of the window.
* *Rename one set* (the shading's ramps under other names). It leaves two tables
  and the second way of doing it, which was the defect.
* *Keep both and say so,* which is what this section did before.

What changes for a person, plainly: **a raster that `RASTER SHADE` or `RASTER
SLOPE` wrote before keeps the colours it was written with (a derived reference
raster is pixels); running the same line now paints it differently.** Terrain is
deep blue, green, sand, brown and snow where it was green to white; diverging
is the five-class RdBu (#0571b0 to #ca0020, through #f7f7f7) where it was
(33, 102, 172) to (178, 24, 43); slope is green to red through yellow at full
saturation where it was RdYlGn's softer greens and reds, so a class of
`RASTER SLOPE classes=` takes the table's colour at its place among the classes;
and grey runs 32 to 224 in five steps, so the darkest cell of a `style=plain`
picture is a dark grey and not black. Nothing is migrated: a line run again
restyles. The blending between stops is still each consumer's own: the scene's
kernels' (`katana_cad::terrain::surface_colouring::surface_ramp_colour`, to the bit) and GDAL's colour map (which
rounds as GDAL does), so between two stops the two may differ by a level of 255;
the stops, and the legends that list them, are the same.

The scene's `elevation_ramp_color` is now the table's terrain ramp, operation
for operation (`surface_ramps::the_terrain_ramp_is_the_scenes_old_elevation_ramp_to_the_bit`
holds it at a thousand points), so every existing scene and kernel test holds.
The old slope colour was a line, red 2t and green 2 - 2t; as a five-stop ramp
its quarter stops are that line rounded (128), and between stops a colour may
now differ from the old by one level of 255. No test held the old values
between stops; the stops are held
(`surface_ramps::the_slope_ramp_keeps_the_scenes_old_slope_colours_at_its_stops`).

## How faces are coloured

A face is coloured by its ELEVATION (drawing units), its SLOPE IN PERCENT
(rise over run times 100: 100 is 45 degrees), or its ASPECT - in eight
sectors of 45 degrees centred on north, north-east ... north-west, coloured
round a hue wheel from red at north - or by one FLAT colour.
`katana_cad::terrain::surface_colouring::surface_colouring` resolves a style's rule against the values a surface
spans:

* **The range.** Each end the style gives (`range_min`, `range_max`), else the
  surface's own: its lowest and highest vertex for elevation, 0 to its
  steepest face for slope. In 3D an open end of an elevation range is the
  one range of every visible surface ("In 3D", below).
* **Smooth ramp.** The ramp at (value - low) / (high - low), clamped.
* **Banded ramp.** `bands` bands of equal width over the range; below it is
  the first band and above it the last. Band k is the ramp at k / (bands - 1),
  so the first and last bands are the ramp's two ends. **At most 100 bands**
  (`MAXIMUM_SURFACE_BANDS`, refused by `check_surface_style`, so the verb, the
  form and a project file hold to it alike). The 3D view cuts each triangle
  at every band edge it crosses, so its geometry grew by about 643 triangles
  a band on the 21 182-triangle sample terrain (28 078 triangles at 10 bands,
  3 235 844 at 5 000, 32 163 345 at 50 000; the verb allowed a million, which
  extrapolates to some 640 million triangles), and a legend has 16 rows however many bands there are. 100 is the
  form's own box and a 0.14 m band on that terrain's 13.9 m of relief; at
  most that many pieces a triangle
  (`surface_band_pieces::a_triangle_across_the_maximum_number_of_bands_is_at_most_that_many_pieces`).
  Rejected: a piece budget in the 3D build that falls back to colouring the
  vertices - a style that looks different on a big surface and on a small
  one - and caching the legend, which a count of 100 makes cheap.
* **A range that is a point.** A range narrower than the geometric tolerance
  (a level surface, or `min=40` over ground wholly below it, which collapses
  the range to 40) is one point, and a value keeps its side of it: below is
  the first band or the ramp's start, above the last band or its end, and at
  it the middle - as the scene's one ramp always was for a level surface - or,
  for slope, the start, because the point is a face with no steepness (a level
  pad is green, as the 3D view drew it before styles, not the yellow middle).
  The one middle for every value drew a terrain wholly below the minimum in
  its middle band (`surface_colouring::a_collapsed_range_is_a_point_and_a_value_keeps_the_side_of_it_it_is_on`,
  `surface_plan_faces::a_range_above_the_whole_surface_colours_every_face_the_first_band_not_the_middle_one`,
  `surface_plan_faces::a_level_surface_coloured_by_slope_is_the_ramps_start_not_its_middle`).
* **Stops.** They override the ramp and are absolute values; at most 100 of
  them, since banded a stop is a band. Smooth, a value is blended between the
  two stops either side; banded, each stop is the lower edge of a band in its
  colour, the first band open below and the last open above (one stop is open
  both ways: every value is in it).
* **Not finite** (a face vertical in plan has no slope): the flat colour, as
  is a level face coloured by aspect.

The legend (`katana_cad::terrain::surface_colouring::surface_legend`) says the same: one swatch a band or stop
labelled by its edges ("24.892 - 26.285", "< 2", "33 +"), the eight sectors
and "level" for aspect, or for a smooth ramp or smooth stops a gradient
labelled at its ends. A label has the fewest decimals, at most three, that
write every edge to a millionth of a unit (`katana_cad::terrain::surface_colouring::legend_decimals`).

* **An end band the data runs past is open.** With `min=30 max=35` over
  ground of 24.9 to 38.8, the first band is drawn on everything below 31 and
  the last on everything above 34, so they read "< 31" and "34 +" and
  `SURFACE LEGEND` leaves `from=` / `to=` out there, as a stop's open ends do;
  they read closed ("24.892 - 26.285") only while the range is the data's own
  or holds it (`SurfaceColouring::open_below` and `open_above`, set by
  `surface_colouring` where a given `min` is above the lowest value or a given
  `max` below the highest). Closed, they said a colour covered 30 to 31 that
  covered everything under it. The picture is not changed by what the legend
  says.
* **A range that is a point** (above) has three swatches, below, at and
  above it, in the colours those really are ("< 40", "40", "> 40"; for slope
  "0" and "> 0": slope is never below 0) - not N swatches of "40 - 40" in N
  colours that did not say which one was drawn.
* **A gradient places each stop at its value**, from the first stop's to the
  last's, so the built-in Cut Fill style (stops at -2, -1, -0.5, 0, 0.5, 1, 2)
  reads true against its labelled ends; placed by its place in the list the
  -1 stop sat at 1/6 and the +1 at 5/6 of a bar labelled -2 to 2, and a
  1 m fill read as 1.33 m. Every labelled stop is written beside its place
  where it clears the label above by a line, the two ends always. A ramp's
  nine samples are evenly spaced, so they look as they did
  (`surface_plan_painter::a_smooth_legend_places_each_stop_at_its_value_not_at_its_place_in_the_list`).

## In plan

`paint_plan` draws every visible surface of the drawing - `PlanSource::surfaces`
and `PlanSource::surface_styles`, which `plan_source_of` fills from the document -
in its style, in the order the surfaces were added, **beneath the entities**:
over the imagery, the point clouds and the mesh footprints, under every line
of the drawing. So the plan view, File > Plot to PDF and every sheet viewport
draw a surface alike, through one painter. A hidden surface
(`NamedSurface::visible`) is not drawn and does not count in what Zoom
Extents frames (`plan_drawn_bounds`); a visible one does, so a drawing of
nothing but a surface frames it.

What is drawn is decided in `katana_cad` by `katana_cad::terrain::surface_plan::surface_plan`, for one
surface through one view, and drawn by
`crates/katana_window/src/terrain/surface_plan_painter.rs`; in order:

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
`katana_cad` into a premultiplied RGBA image (the layout the window's canvas
paints and egui's textures take, `crates/katana_window/src/canvas/image.rs`), one image
pixel a device pixel on screen, on a grid lined up with the view's first
pixel (`katana_cad::terrain::surface_plan::plan_image_grid`), and laid down through the frame like a raster.
Its cost is the pixels it covers and the triangles in view, whatever the zoom.

* **Exact to the pixel.** Elevation is interpolated at each pixel's centre,
  so a band edge is exact to the pixel - a contour line, not a staircase of
  whole triangles - and a smooth ramp is smooth across a triangle.
  Slope and aspect, properties of a face, are a colour a triangle.
* **No pinholes, nothing twice.** A pixel centre exactly on an edge two
  triangles share goes to one of them: each edge function is worked out from
  its lower-numbered vertex to its higher, the same arithmetic in both
  triangles, so both see the same tie - and the tie goes by the edge's
  direction in the image, the top-left rule of the graphics APIs: it is the
  triangle's when the edge, run so that the triangle is on its positive side,
  goes up the image (a left edge) or along it with the triangle below (a top
  edge). That is the same as testing the centre moved by an infinitesimal
  (e, e squared), which lies on no edge, so a centre on an edge goes to one of
  the two triangles and a centre on a VERTEX to exactly one of those round it.
  On an eighth-of-a-unit grid 80 centres lie exactly on the square's shared
  diagonal; the image covers 6 400 pixels, and the test fails with 6 480 when
  a tie goes to both and 6 320 when to neither
  (`surface_plan_faces::every_pixel_of_the_surface_is_covered_once_even_on_the_shared_diagonal`,
  both mutations run). Rejected: deciding a tie by vertex numbers, as the
  first version did - it settled an edge but a vertex only for a fan whose
  centre was numbered between its neighbours: numbered below all of them, or
  above all (a `.12da` import numbers vertices by first use, so the first vertex
  of its first triangle is always 0), no
  triangle had the pixel, a pinhole, and with neighbours alternating above
  and below two had it. A fan of four, its centre exactly on a pixel centre,
  covers 49 for every numbering where the old rule gave 48, 48, 50 and 49
  (`surface_plan_faces::a_vertex_on_a_pixel_centre_is_one_triangles_whatever_the_vertex_numbers_are`).
* **The same at any thread count.** Rows are painted in bands of 32 across the
  task pool, each band writing only its rows and taking its triangles in
  ascending order (`surface_plan_faces::the_image_is_the_same_at_any_thread_count`).
* **On paper** the image is made at the imagery cap, as a raster's crop is
  (`PlanPaintOptions::raster_dpi_cap`, 200 dpi, and the 16-megapixel cap), and
  a greyscale or monochrome plot prints it through `plot_image`.

Rejected: a painter's polygon a triangle (a frame of 400 000 polygons), and
pieces of triangles cut at band edges drawn as polygons (exact, but more
polygons still, and smooth colouring needs a gradient a triangle, which a 2D
painter cannot draw). The 3D view does cut triangles at band edges
(`katana_cad::terrain::surface_colouring::band_pieces`), because a GPU draws triangles and not pixels.

### Level of detail

All in pixels of the view (a logical pixel on screen; on paper a 96th of an
inch of the page, `PLAN_PAPER_PIXEL_MILLIMETRES`, so a plot keeps the detail
the screen showed at its scale):

* **Triangles under four pixels are not stroked** (`PLAN_EDGE_PIXELS`), as the
  3D view's edges are gone below 4 px (`SceneBuilder::fade_edges`): a mesh of
  one-pixel lines that close is a sheet, not triangles. A triangle whose every
  edge is shorter is tinted into the image in the triangle colour at 35 %
  (`PLAN_DENSE_TRIANGLE_ALPHA`), so a dense surface reads as a tint rather than
  disappearing. Each edge is stroked once - from the lower-numbered of its
  two triangles unless the other is out of view.
* **Points closer than four pixels apart** on average (the square root of
  the plan area a vertex) are single pixels in the image, not marks.
* **Contours closer than three pixels** apart at the surface's mean slope
  (interval over the area-weighted mean slope, in pixels) are left out: the
  minors first, then the majors (`PLAN_CONTOUR_PIXELS`). A contour whose box
  is under a pixel is a speck and is left out.
* **Lines are thinned** to half a pixel (radial distance: a vertex is kept
  when it is half a pixel or more from the last kept; the ends always) and
  clipped to the view by whole segments (`katana_geometry::polygon::clip_polyline`), so a long
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
(`surface_plan_cull::the_grid_finds_exactly_the_triangles_testing_every_one_finds`: 304
boxes inside, across, outside, thin and empty; a grid that drops its last row
of cells fails 129 of them). A view wholly holding the surface takes every
triangle without the grid.

`katana_cad::terrain::surface_plan::SurfacePlanCache`, one per view inside the plan painter's cache
(`PlanPaintCache::surface_cache`), keeps per surface its grid, its mean and
steepest slopes, its rim and breaklines joined into lines, and its contours
for the settings last drawn - traced once by `katana_terrain::contours` and again
only when the interval, base or major spacing change. It is keyed on the
surface itself, which is shared and immutable (an edited surface is a new
one), keeps it alive while kept, and lets a surface go at the first paint
after the drawing has lost it. A copy of the cache starts empty. **There is no
cap on how many it keeps**: the caller bounds it to the drawing's surfaces
(`retain`, which a paint calls with all of them). A cap of 32, evicting the
least recently used, was wrong for a paint, which asks for every visible
surface in the same order each time: the 33rd evicted the very surface the
next paint wanted first, and every repaint rebuilt every grid and traced every
contour again - 61-69 ms a repaint with 33 surfaces against 3-7 ms with 32
(`surface_plan_cache::more_surfaces_in_view_than_any_cap_are_kept_not_rebuilt_on_every_paint`).
Rejected: evicting only entries an earlier paint used (a paint generation) -
a cache that still grows past its cap in the case it was for.

Contours whose tracing would make more than four million points
(`PLAN_MAXIMUM_CONTOUR_POINTS`, estimated from the levels crossing each
triangle) are refused for the plan, saying why (`SurfacePlan::problem`): an
interval a hundredth of what the relief wants would fill memory and the view
with ink. The refusal is one function, `katana_cad::terrain::surface_plan::contour_refusal`, which the paint
and the preflight check below both ask, so they cannot disagree. A paint
reports it as a `katana_cad::terrain::surface_plan::SurfaceProblem` - the surface, its style and the reason -
in `PlanPaintStats::surface_problems`, and the reason reaches a person or an
agent in every place a surface is drawn from:

* `SURFACE PLAN` prints `problem=`;
* the plan view tells its error hook - the command log - once, and again if
  it goes and returns (`viewport_widget::report_surface_problems`), and its
  headless report ends with ` problem="ground (style Fine): contours: an
  interval of ... not drawn - set a larger interval"` for each;
* a sheet viewport adds the same line, `vp1: ground (style Fine): contours:
  ...`, to the sheet's problems (`SheetPaintStats::problems`): the editor
  shows it in the viewport's panel, and every plot reports it
  (`PlotReport::problems`: the Plot dialogs, the editor's message, the lines
  `PLOTSHEETS` logs). The plan is drawn, only without the contours, and the
  plot is not refused for it;
* `PLOT` (File > Plot to PDF, `--plot`) logs a record for each, `warning
  surface="ground" style="Fine" reason="contours: ..."`, between its sentence
  and its `file=` record, and the dialog shows them (`plotDrawingWarnings`);
* and before any paint the preflight finds it: `plan.surface-style` of
  `SHEETS CHECK` (the editor's Checks dock and the check before Plot Sheet),
  fed by `katana_cad::terrain::surface_plan::surface_style_problems`, which needs no painter - so
  `katana_cli` and `katana_mcp`, which cannot plot, have the warning with the
  surface as `subject=` (`docs/plotting.md`, "Preflight checks").

The refusal used to be thrown away on the way from the painter to the view,
and then, on a plot or a sheet, which paint the same stats and read nothing
from them: the contours vanished with no word. Rejected: a structured list on
`SheetPaintStats` beside its text lines - the problems already reach three
consumers as text, and the structured form an agent reads is the preflight
finding; and refusing the plot, which wastes no paper that the warning does
not already save.

### The legend

On screen, over everything in the view's top-right corner, the legend of each
drawn surface whose style colours its faces - at most three, stacked
(`PlanPaintOptions::surface_legends`). Each is measured before it is drawn
(`measure_surface_legend`, the one layout `paint_surface_legend` draws, and
`paint_surface_legend_stack`, the stack the 3D view's legends go through too):
the first is drawn whatever its height, and a later one that would not fit above the
view's foot is left out and not counted - a key with its lowest rows cut off
by the edge reads as a key that ends there (`surface_plan_painter::a_legend_too_tall_for_the_view_below_the_ones_already_drawn_is_left_out_not_cut_off`;
three 10-band legends are about 500 px). It is drawn into the kept drawing,
so a mouse move costs nothing more. A plot carries none: a sheet's legend
belongs with its title block.

A drawing that holds only surfaces is not empty: the "Nothing drawn yet"
hint is not laid over it (`viewport_widget::drawing_is_empty` counts the
document's surfaces, hidden or not, as the 3D view's does;
`plan_view::a_drawing_of_nothing_but_a_surface_is_not_said_to_be_empty`).

The plan view's headless report says what it drew of the surfaces, after the
entities, ghosts and grips, when it drew any - `PlanView1: drawn=N ghosts=N
grips=N surfaces=N triangles=N edges=N tinted=N contours=N labels=N
covered=N legends=N`, the counts of `PlanPaintStats`, then a `problem="..."`
for each part of a style it could not draw - and reads as it always did when
it drew none (`docs/headless.md`).

## In 3D

`SceneSurface::display` carries a COPY of the surface's style (the drawing's
set is replaced by an edit, so a pointer into it would dangle);
`katana_cad::scene::apply_surface_display` sets it with the shading (`SurfaceDisplayStyle::shading`,
the 3D view's `SurfaceStyle`). A surface with no style is drawn as before
styles, by `SceneSurface::coloring`. With one, its `faces3d`:

* **Elevation, smooth, on a ramp** - the shared vertices coloured by the
  style's ramp, on the kernel path. Where the style leaves an end of its range
  open, the end is the one range over every visible surface coloured so,
  which keeps the scene's rule that a height is one colour on every surface
  (`docs/render.md`, "Surfaces"); a surface with both ends of its own does not
  stretch it (`scene_surface_style::a_surface_with_a_range_of_its_own_does_not_stretch_the_one_ramp`).
* **Elevation by stops** - each vertex its colour, on the scalar loop.
* **Elevation banded** - each triangle cut where a band edge crosses it
  (`katana_cad::terrain::surface_colouring::band_pieces`: convex pieces of three to five corners, a corner on a
  band edge at the edge's level exactly, covering the triangle), each piece
  its band's colour lit by the triangle's normal: a band stops at its contour.
* **Slope, Aspect** - a colour a face on copies of its own, lit by the face's
  normal, as the old slope colouring was.
* **Flat** - one colour.

A range that is a point (above) is coloured by the colouring, on the scalar
loop, not by the kernels: their ramp has only a middle for a range that
narrow, where a height below the point is the ramp's start
(`scene_surface_style::a_range_above_the_whole_surface_colours_its_vertices_the_ramps_start_not_its_middle`).

**A wireframe colours nothing.** Its lines are one grey whatever its style
says of `faces3d`, so it takes no part in the one range that open ends are
stretched over, brings up no legend bar when it is the only surface, and
`SURFACE LEGEND <name> 3D` says `faces=none kind=none entries=0` for it. A
shaded surface above a mesh (the design as the built-in Outline, the ground
shaded) is coloured over its own heights, not over the two surfaces' - the
mesh stretched the ramp to 200 and the ground's top sat a twentieth of the way
along it (`scene_surface_style::a_wireframe_colours_nothing_so_its_heights_stretch_no_ramp_and_alone_it_brings_no_bar`).

A style on the terrain ramp draws exactly what the scene drew before
(`scene_surface_style::a_style_on_the_terrain_ramp_colours_a_surface_as_the_scene_always_did`).
`SceneLayers::legends` are the legends of the drawable surfaces whose styles
colour their faces by value or aspect, in drawing order, each as drawn
(`SurfaceLegendCard`: the surface, its style, the legend). A hidden surface, a
wireframe and a surface with no style have none. `RenderView` stacks
them down the top-right corner with the plan view's own stack
(`paint_surface_legend_stack`, one function for both views, so they measure, fit
and leave out alike), each captioned "surface - style"; with none, the old
elevation bar is drawn for surfaces with no style. It used to draw the first
styled surface's legend only, so a second styled surface's colours could not
be read. The plan view keeps its three, over the drawing a person is editing;
the 3D view is limited by room alone: a later legend that would not fit above
the foot of the view is left out whole and ends the stack
(`surface_legend3_d::more_legends_than_the_view_holds_are_left_out_whole_not_cut_off_or_overlapped`),
and the scene keeps them all (`SceneLayers::legends`). Rejected: the plan
view's cap of three here too, which would leave a fourth styled surface
unreadable in a view with the room for it. The headless report of a 3D view
says what it stacked, after its camera: `legends=2 legend="ground - Elevation
Bands" legend="second - Slope Bands"`, nothing when it drew none
(`render_view_widget::drawn_legends`,
`qt_the_3d_view_stacks_a_legend_for_each_styled_surface_headless`).

The window styles its 3D surfaces from the drawing's styles
(`main_window::style_scene_surfaces`) when it rebuilds them
(`main_window::sync_scene_surfaces`), and again, queued and once for a burst, when
the styles are edited or another drawing is opened
(`main_window::restyle_scene_surfaces`).

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
| Apply | `SURFACESTYLE SET "chosen" key=value ...` - only the fields the form changed, in the verb's key order (`katana_window::terrain::surface_style_manager::surface_style_set_words`) |
| In This Style | `SURFACE STYLE "surface" "chosen"` |

The form is a buffer, previewed as it is; a colour field takes `#RRGGBB` or
a colour name the drawing knows (`katana_cad::colour_lookup::resolve_colour`), and the line always
carries `#RRGGBB`. A field that does not read (an interval that is no number,
a colour that is neither) stops Apply and the line label says why. A reload - a `SURFACESTYLE` typed, an undo - keeps
the form's unapplied edits while the style they were made to is unchanged.
The form changes nothing it was not asked to: a field a box cannot show
(a major every 2 000 000 intervals, past the box's 1 000 000; a project's
`faces3d=none`, which the list shows as flat) keeps the style's own value
until the box is moved, as the opacity already did, so the line names "only
the fields the form changed" truly
(`surface_style_manager::a_style_holding_more_than_the_forms_boxes_show_is_left_alone_until_the_box_is_moved`).
The bands box is 2 to `MAXIMUM_SURFACE_BANDS`, the same number the check holds.
A refused New, Copy or Rename (a name that is taken) leaves the style being
edited chosen with its unapplied edits: the row a line names is chosen only if
the line is accepted (`ARefusedNameLeavesTheStyleBeingEditedChosenWithItsUnappliedEdits`);
it used to jump to the style that holds the name and replace the form with it.
Every control's name is in the dialog's module documentation; the Surfaces panel's
Edit Styles... button triggers the same action.

## On the command line

Both read only, so neither is an undo step; both reach `katana_mcp` through
`katana_run_commands`, as every interpreter verb does.

```
SURFACE LEGEND <name> [3D] [JSON]
SURFACE PLAN <name> [AREA x0,y0,x1,y1 | VIEW [<id>]] [pixels=W,H]
```

`SURFACE LEGEND` is the legend of its faces in its style: in plan
(`plan_faces`) or in 3D (`faces3d`, over the surface's own heights - a 3D view
stretches an open range over every visible surface, which a verb has no view
to know). A head record, then one a band, stop or sector:

```
legend surface="ground" style="Elevation Bands" view=plan faces=elevation kind=bands entries=10
entry index=1 colour=#28468C from=24.892000198364258 to=26.284700202941895 label="24.892 - 26.285"
```

`from=` and `to=` are the edges exactly (`format_exact_real`): the sample terrain's
heights are Float32 (GDAL reads the text grid so), so the edges show their
stored values, 24.892000198364258 and not 24.892, and the label carries the
rounded form. Rejected: rounding the edges as `SURFACE`'s own record does
(`docs/terrain.md`, "One surface record"), which drops the storage noise - a
value an agent compares against one it holds should be that value, and the
label is the rounded one already
(`cli.surface_legend_lists_the_bands_of_a_surfaces_style` pins the digits). An
open end has no `from=` or `to=` (JSON: `null`); a style that draws a surface
as a mesh in 3D has `kind=none` (above).

`SURFACE PLAN` is what the plan view draws of it looking at AREA (default: the
whole surface) through a view of W x H pixels (default 1600 x 1000, the plan
benchmark's), or at what the window's plan view shows (VIEW, through the one
scope parser; headless it is refused naming AREA) - its own area at its own
size and scale (`ScopeView::frame`, the view's `ViewTransform`), so what is
stroked, tinted or hidden is what that view drew, to the triangle (a 327 x 488
view at extents drew 21 182 triangles tinted and none stroked; the verb, told
1600 x 1000 for it, said all stroked and none tinted); `pixels=W,H` asks for
another frame of the view's area; `VIEW EXTENTS` is refused, naming EXTENTS: the
triangles in view,
stroked and tinted, the image and the pixels covered, the contour levels and
runs and whether the minors and majors are drawn, labels, points, the
boundary and breakline runs, and the scope record. A surface hidden in the
drawing is still described (`visible=no`): hiding is the picture's. Refused:
no such surface (NotFound), a word neither takes, WHERE ("a surface is not
entities"), a scope other than AREA or VIEW, a bad AREA (not four numbers,
or one with no width and no height: a point has no frame, and the reply of a
plan that was never worked out - every count 0 - would read as a point the
surface does not cover) or pixels=
(`surface_display_verbs::plan_of_the_view_is_what_the_view_draws_at_its_own_size_and_scale`,
`surface_display_verbs::plan_of_an_area_that_is_a_point_is_refused_not_answered_with_zero_counts`).

No MCP tool of its own: the legend as JSON (`SURFACE LEGEND name JSON`) is the
structured form an agent wants, and it comes back through
`katana_run_commands` already. The JSON is written as the other JSON replies are, with a byte that is not
UTF-8 replaced (nlohmann's `replace` handler in the C++ build), so a surface
or style name with such a byte is U+FFFD in the reply and not a failure that
ends katana_cli (`SurfaceStore::check_name` and `check_surface_style` refuse such a
name where it enters; this is for one that got past; `docs/headless.md`, "Text
and names"). The guard is reached through a seam: the reply is made of a
plain-data `SurfaceLegendReply` and written by `katana_cad::terrain::surface_display_verbs::surface_legend_json`, so a
test hands it the name the store would refuse and reads back U+FFFD and no
exception (`surface_display_verbs::legend_json_writes_a_bad_byte_of_a_name_or_label_as_u_fffd_and_does_not_throw`).
A mutation check of 2026-10-15, in the C++ build, had put the strict `dump`
back and every test of the verb still passed, because no API puts such a name
into a Document; the strict `dump` threw on these bytes (nlohmann's
type_error 316), which the test would then report.

## Speed

Measured on 2026-10-15 on a 24-thread machine (Google Benchmark reports
2496 MHz), in a scratch Release build: this branch's sources at the main
build's flags (-O3), linked against the main checkout's libraries - not the
integration build, which had not been made, and not alternated against
another binary, so read the times as sizes. Three repetitions of at least
0.3 s, medians. The surface is 448 x 448 cells, 401 408 triangles over a
kilometre at map-grid coordinates; the view 1600 x 1000 at extents (the
kilometre over 950 px), and zoomed in five and twenty-five times.

`BM_SurfacePlan` (the C++ build's `bench_surface_plan` benchmark, removed with the C++ tree), `katana_cad::terrain::surface_plan::surface_plan`
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

`BM_PlanPaintSurface` (the C++ build's `bench_plan_surface` benchmark), a whole frame
through `paint_plan` onto a 1600 x 1000 image, the view panned a pixel each
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

The 3D terrain built in a style (`BM_SurfaceSceneStyled`, `SceneBuilder::build_terrain`
of the same surface): smooth elevation on the kernels 4.2 ms; elevation bands,
cut at their contours into 455 732 triangles, 27.0 ms; slope bands, a colour a
face, 50.0 ms (70.2 ms while the legend worked the surface's steepest face out
a second time; each surface's colouring is now worked out once a build) -
paid when the terrain is rebuilt (a style or surface edit, a change of
exaggeration), not each frame.

To measure again: the benchmarks went with the C++ tree and have no Rust
twins yet (`docs/performance.md`); these figures are the C++ build's.

## Tests

* `crates/katana_cad/tests/scene/surface_colouring.rs` - the ramps' published stops
  and the scene's old constants, the table's names and fallback, aspect
  sectors either side of each edge, bands and stops by value, smooth stops,
  the range, slope in percent, band pieces covering a triangle exactly, the
  legends and their decimals.
* `crates/katana_cad/tests/scene/surface_plan.rs` - the cull grid against testing
  every triangle, the four-pixel edge rule and its tint, the colour of a known
  face in a known band either side of a band edge, coverage on the shared
  diagonal, opacity, slope and aspect faces, the image at any thread count, a
  contour at its level and its label, minors hidden, contours traced once and
  the refusal of a too-fine interval, the rim and a breakline joined, points
  as marks and as pixels, nothing out of view, the image grid and its cap.
* `crates/katana_cad/tests/scene/scene_surface_style.rs` and
  `scene_kernels::a_surface_in_a_display_style_on_every_ramp_builds_the_same_at_every_level` -
  the 3D colouring by style (above).
* `crates/katana_cad/tests/terrain_utility_verbs/surface_display_verbs.rs` - both verbs' replies,
  worked out on a 10 x 10 square, and every refusal.
* `crates/katana_window/tests/plan_painting/surface_plan_painter.rs` - a surface inking
  exactly its own 80 x 80 pixels where nothing was, a known face its band's
  colour, each built style inking its own way against the same paint without
  the surface, hidden drawing nothing, the legend on screen and never plotted,
  contours traced once through the painter's cache.
* `crates/katana_window/tests/surfaces/surface_style_manager.rs` - each button's
  line word for word, Apply naming only what changed, a bad field stopping
  Apply, the preview drawing the form's style, and the lines changing the
  drawing through the interpreter, undone as one step, the dialog following.
* Added with the review of 2026-10-15 (what each proves is named where it is
  described above): the band maximum in `surface_style.rs` and
  `cli.surfacestyle_with_more_bands_than_the_maximum_is_refused_naming_bands`;
  the collapsed range, open end bands and one stop in
  `surface_colouring.rs`; vertex ownership and the uncapped cache in
  `surface_plan.rs`; VIEW at its own frame, a point AREA and a wireframe's
  legend in `surface_display_verbs.rs`
  (`cli.surface_plan_of_an_area_that_is_a_point_is_refused_not_answered_with_zeros`);
  the 3D wireframe, collapsed and level-slope colours in
  `scene_surface_style.rs`; the gradient legend by value, the stacking
  and the surfaced problem in `surface_plan_painter.rs`; the form's
  boxes and a refused name in `surface_style_manager.rs`; the empty hint
  and the reported problem in `plan_view.rs`.
* Added with the follow-up of the same day (what each proves is named where it
  is described above): `surface_problems` in `surface_plan.rs` (the
  refusal at either side of the limit, an interval that is no number, a level
  surface, the plan's and the check's one answer, the line and the record, the
  styles of a drawing's surfaces without painting); `sheet_painter` problems,
  `PlanPainter.ASinglePagePlotHandsBack...` and `PlotDrawingDialog.TheWarningRecords...`
  in the widget tests; `plan.surface-style` in `preflight.rs` and
  `sheet_agent_verbs.rs`; `cli.sheets_check_warns_of_a_surface_whose_contours_are_too_fine_to_plot`,
  `qt_a_plot_of_a_surface_with_contours_too_fine_says_so_headless`; the 3D
  legends: `scene_surface_style::every_visible_surface_in_a_colouring_style_has_a_legend_in_drawing_order_and_no_other_has_one`
  (two styled, one hidden, one unstyled, one wireframe), `surface_legend3_d`
  (stacked in order and not overlapping; none styled keeps the bar; more than fit
  are left out whole) and the headless report; the one ramp table:
  `colour_ramps` in `crates/katana_terrain/tests/colour_ramps.rs`,
  `colour_ramps::the_shadings_ramps_are_the_surface_styles_ramps_stop_for_stop` and the
  `ShadeVerb` tests that read the table's colours.
* `qt_a_surface_is_drawn_in_plan_in_its_style_headless` - the sample terrain
  in the real window in Standard, Triangles and Elevation Bands, each paint's
  report read against the first; `qt_the_surface_style_manager_runs_its_lines_headless`.
* `cli.surface_legend_lists_the_bands_of_a_surfaces_style`,
  `cli.surface_plan_says_what_the_plan_view_draws_of_a_surface`,
  `cli.surface_plan_of_no_such_surface_is_refused`.

## Decisions

* **Faces as an image in `katana_cad`, not painter polygons** - above. The
  rasteriser is a few dozen lines with an exact tie rule, testable without a
  window.
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
* **A style has at most 100 bands and 100 stops**, checked where every style
  passes (`check_surface_style`) rather than limited in the 3D build: one number
  for the line, the form and a project file, and a style that draws the same
  on a big surface as on a small one. Rejected: a piece budget with a
  fallback to vertex colours. A project saved with more (the verb took a
  million) opens with the built-in styles and the refusal as the styles'
  status, its saved set kept to be written back as it was
  (`surface_document.rs`: a set that is refused is not repaired by guessing).
* **The plan cache has no cap** - the caller's `retain` bounds it. Rejected: a
  count cap with least-recently-used eviction (a paint's cyclic access misses
  every time one over the cap), and a paint generation protecting this
  paint's entries.
* **The rasteriser's tie rule is the top-left rule**, not vertex numbers.
* **A range that is a point keeps a value's side of it** (and slope's point is
  the ramp's start), rather than the one middle colour for everything.
* **One ramp table, in the `terrain` layer, with the surface styles' colours**
  ("The ramps"). Rejected: letting the shading's colours win, renaming one set,
  and keeping two.

## Not done

* A legend on a plot or sheet (a sheet's title-block legend), and a View
  menu switch to hide the screen legend (today: a style without faces has
  none). The pieces exist - `katana_cad::terrain::surface_colouring::SurfaceLegendCard` is plain data, and
  `paint_surface_legend_stack` takes any painter - but where a sheet keeps one is
  undecided: a line of the smart legend (`compute_legend`), an option of the
  plan viewport (a change to the sheet set's schema, on every surface at once),
  or a block of the title block; a plot made by `PLOT` has no sheet to hold it.
* The plan view's faces are not shaded (no hillshade in plan); a hillshade
  face colouring would be a fifth `SurfaceFaces` value and a contract change.
* Smooth elevation in plan evaluates the ramp at every pixel; a lookup table
  would roughly halve the faces' cost at extents.
* Slope and aspect faces in 3D cost 50 ms on a 400 000-triangle surface
  (a colour a face on copies of its own, as the old slope colouring did); the
  slope of each face could be kept with the surface.
* The minors are hidden by the surface's MEAN slope, so on a site of steep
  batters and flat pads they go from the flats when the batters crowd them.
* `RASTER ASPECT` colours the eight compass directions with a constant of its
  own (`crates/katana_app/src/geo/slope_verbs.rs`), which is neither the aspect wheel
  a surface style uses (`aspect_sector_colour`) nor a row of the ramp table: two
  palettes for aspect.
* `PLOTSHEETS` reports a surface style it could not draw as one of the
  sheets' problem lines (`vp1: ground (style Fine): ...`), as every problem
  of a sheet is, not as a `warning surface= style= reason=` record as `PLOT`
  does; the record form of a sheet's problems is `SHEETS CHECK`'s finding.
* Labels are not kept off each other or off the drawing's own labels (the
  label placer, `docs/annotation.md`, places entity labels only).
