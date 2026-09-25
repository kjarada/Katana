# Sheets: the plot frame, sheet sets and the sheet generators

A drawing is handed over on paper, or as PDF pages that stand in for paper.
Katana's single-page plot (`include/katana/cad/plot.hpp`, `docs/cad.md`) puts
one plan view on one page. This document describes what a whole SET of sheets
is made of: several sheets, each with a title block and several views, all
numbered and filled in automatically.

The owner asked for three things:

- the frame of their own cross-section app, with every piece of the original
  organisation's branding removed and a slot for the user's own logo;
- several sheets and several kinds of view per sheet, without the system
  becoming complicated;
- sheets laid out for the user: fitted, tiled, strung along an alignment,
  packed with cross sections.

This part is the **model**: the data, the frame, the generators and the
undoable edits. It lives in `katana_cad` and uses no Qt. The painter that
draws a sheet and the editor for sheets come next ("Not yet", at the end).

| Header (`include/katana/cad/plotting/`) | What it holds |
|---|---|
| `frame.hpp` | the frame: paper, margins, drawing area, lines, texts, cells, field slots, the logo slot; uniform scaling to other sizes |
| `sheet_set.hpp` | `SheetSet`, `Sheet`, `Viewport`; fields and their automatic values |
| `layout.hpp` | the eight tiling presets, the tiling rank of every viewport kind, snapping |
| `generators.hpp` | fit, grid, strips, cross sections, sheets from plot frames, the smart layout |
| `sheet_json.hpp` | the versioned JSON a project stores its sheets in |
| `sheet_commands.hpp` | the undoable edits, the logo import, the project's field values |

`include/katana/cad/document.hpp` gains `sheetSet()`, `sheetSetStatus()` and
`setSheetSet()`, and `plot.hpp` gains the finer sheet scale ladder
`kSheetScales` and `sheetScaleAtLeast`. The tests are in `tests/cad/plotting/`.

## Paper coordinates

Every position on a sheet (the frame, its cells and the viewports) is in
**millimetres from the bottom-left corner of the paper, Y up**. The frame was
measured in that convention, and model space uses it too. A renderer flips Y
once, at the device, so nothing in the model has two conventions.

## The model

A **SheetSet** is every sheet of a project. It holds the title-block values
the sheets share (`SheetDefaults`), the numbering pattern, the revisions and
the sheets. A **Sheet** is one piece of paper. A **Viewport** is a rectangle
on that paper showing either a view of the drawing or a panel of paper
furniture.

| `SheetDefaults` | |
|---|---|
| `organisation` | the organisation cell (a slot the user fills) |
| `projectLines[0..3]` | the project block's four lines; the first two default to the project's name and description |
| `client` | a field for a frame that has a client cell |
| `locator`, `surveyor`, `compiler`, `reviewer`, `approver` | a name and a date each (`SignOff`); a missing date prints the plot date |
| `notes` | the notes cell (the other slot the user fills) |
| `heightDatum`, `coordinateSystem`, `modelName` | text for the survey-details cells; an empty coordinate system is the project's |
| `setNumber` | the drawing-set number |
| `logoAsset` | the logo's file name in the project's `assets/` directory |

| `Sheet` | |
|---|---|
| `id` | `s1`, `s2`...: stable for the sheet's life, so a match line or key plan can refer to the sheet however the sheets are reordered or renamed; never given to a new sheet while a mark still names it |
| `name` | what the sheet shows (`PLAN TILE 3`, `ROAD CH 0.000 TO 172.500`) |
| `paper`, `landscape` | ISO A0 to A4 |
| `frame` | `a3_landscape` (the built-in frame, scaled to the paper) or empty for no frame |
| `frameLegend` | whether the frame's legend block is drawn |
| `fields` | the title-block values this sheet overrides, by field name; an override always wins |
| `viewports` | back to front |

| `Viewport` | |
|---|---|
| `id` | `vp1`, `vp2`...: unique across the set |
| `kind` | `Plan`, `LongSection`, `CrossSections`, `Model3D` (a snapshot of the 3D view), `Legend`, `Notes`, `Image`, `KeyPlan` |
| `rect` | the rectangle on the paper, mm; empty until the viewport is placed |
| `scale`, `autoScale` | 1 : `scale`; with `autoScale` the painter chooses the largest standard scale at which the content fits |
| `centre`, `autoCentre` | the world point at the rectangle's centre: for a plan (easting, northing); for a long section (chainage, level); for a cross section (offset, level), offset 0 being the centreline. `autoCentre` asks the painter to centre what it draws and keep the scale |
| `rotation` | radians counter-clockwise: the world direction that runs left to right across the paper |
| `verticalExaggeration` | sections only |
| `tiltDegrees` | a 3D snapshot's camera height angle |
| `source` | the alignment, the chainage range, and the section interval or stations |
| `hiddenLayers` | a `cad::LayerOverrides`, the same value a plan view uses to hide layers |
| `title` | empty: `automaticTitle` (`PLAN 1:500`, `LONG SECTION H 1:500 V 1:50`, `CROSS SECTION CH 120.000`) |
| `northArrow`, `scaleBar` | furniture flags for the painter |
| `locked` | tiling and snapping leave the viewport alone |
| `text` | a Notes panel's text; an Image panel's asset file name |
| `marks` | world lines drawn over the view: match lines and a key plan's sheet outlines (`WorldMark`) |

Everything is a plain value with `operator==`, so a test, an undo step and
the JSON round trip can each compare whole sets.

## The frame

### Where it comes from

The frame is the owner's cross-section app's A3 landscape frame. It was
measured item by item from the app's own parser and renderer, and the
renders were compared with the app's page. The result is committed as data:
`resources/plot_frames/a3_landscape.json`. The build compiles that file in
with C++26 `#embed` (`src/katana_cad/plotting/frame.cpp`, with
`--embed-dir` set in `src/katana_cad/CMakeLists.txt`). The dependency file
names the JSON, so editing it rebuilds the frame, and a missing file is a
build error rather than a blank title block.

The measured frame had 179 items (85 lines, 80 texts, 14 symbols) and 18
fields. The 34 items of the original organisation's branding were removed
before the data was committed: its logo, the header naming it, and the
disclaimer naming it. 145 items remain: 57 polylines, 74 texts and 14
legend symbols. There are also 16 named cells. The three cells the branding
occupied are now **empty slots the user fills**:

| Slot | Cell (mm, bottom-left origin) | Size | Filled with |
|---|---|---|---|
| logo | 167.265..220.229 x 22.625..34.625 | 52.964 x 12.0 | the user's own logo image (`fitImage`: the largest size that fits inside the cell less 1 mm of padding all round, aspect ratio kept, centred; a 4:1 logo prints 40 x 10 mm) |
| organisation | 297.117..401.843 x 28.865..34.625 | 104.726 x 5.76 | `{organisation}`, centred, at the removed header's 3.738 mm cap height |
| notes | 220.229..297.117 x 12.0..34.625 | 76.888 x 22.625 | `{notes}`, from the removed text's first-line anchor (220.918, 32.961), 0.85 mm caps |

### What was measured

- **Paper and margins.** A3 is 420 x 297 mm, with margins of 23 left, 10
  right, 10 top and 35 bottom. The **drawing area**, where viewports go, is
  23..410 x 35..287 mm (387 x 252).
- **Border and title block.** The drawn border is 21.825..411.309 x
  34.625..289.265 at 0.53 mm. The title-block strip under it is
  389.484 x 25.68 mm. The drawing area sits unevenly inside the border
  (1.2 mm on the left, 2.3 mm at the top); that is kept, because the goal is
  to plot exactly like the app.
- **Lines.** Rules are 0.53 mm and the legend grid 0.13 mm. The twelve field
  underlines are dashed, 2.2 mm on and 1.1 mm off, at 0.53 mm. Colours are
  exact hex values. Dashes are paper millimetres. A closed ring does not
  repeat its first point.
- **Construction guide.** Two rectangles at the paper's edge (orange,
  0.18 mm) are construction lines. The app plots them; here they are
  `FrameRole::Construction` with `plots == false`, drawn while editing and
  never on paper.
- **Texts.** Each text's `anchor` already includes its offset and raise; six
  texts carry one. For example, "No. of" is measured at (406.169, 32.970)
  with an offset of -1.349 and a raise of 0.063, so it is drawn at
  (404.820, 33.033).
  - The height is the **cap height**, not the em.
  - Nearly every label has an x factor of 0.82. A painter applies it as a
    horizontal painter scale, never as a font stretch: on Arial the painter
    scale gives exactly Arial Narrow's width, while `QFont::setStretch(82)`
    measures 89%.
  - A multi-line text anchors on its first line, and later lines step down by
    1.5 x the cap height whatever the justification.
  - Justification is vertical (bottom, middle, top) by horizontal (left,
    centre, right). The angle is in degrees counter-clockwise; the filename
    up the left margin is at 90.
- **Centred values.** The scale, the sheet number and the sheet count are
  centred in their ruled cells (`FrameText::centredIn`), as the app
  re-centres them.
- **Legend symbols.** The 14 legend symbols are kept by the name of their
  style (`FrameSymbol`). `Sheet::frameLegend` turns the legend block off.

The cells are `drawing_area`, `legend`, `scale`, `coord_system`,
`height_datum`, `file_model`, `logo`, `signoff`, `notes`, `manager`,
`organisation`, `project`, `sheet_count`, `sheet_number`, `left_margin` and
`stamp` (`Frame::cell(id)`).

### What changed from the measured frame

- **Field tokens.** The frame's numbered text tokens become named fields (the
  table below). A numbered token the build has no name for fails the parse,
  so a changed frame cannot print a raw token.
- **Dates.** The app fed one date to all six date texts, so a sheet reviewed
  a week after it was surveyed said otherwise. Here each sign-off row has its
  own date field, and the stamp has the plot date.
- **Regional text.** The coordinate-system and height-datum texts were
  hard-coded for one country's grid and datum. They are now
  `CO-ORD SYSTEM: {coordinate_system}` and `HEIGHT DATUM: {height_datum}`.
  The project block's first line lost its regional label ("LGA:") ahead of
  the user's text.
- **Stamp.** The stamp under the title block names the paper (`{paper}`),
  since a scaled frame is no longer A3.

### Other sizes: uniform scaling

Only the A3 frame exists, so the other A sizes use it **scaled uniformly**.
The factor is `min(paper width / 420, paper height / 297)`, the largest that
fits (`frameScaleFor`):

| Paper (landscape) | Factor | Why |
|---|---|---|
| A0 1189 x 841 | 2.8310 | 1189 / 420 (841 / 297 = 2.8316 is larger) |
| A1 841 x 594 | 2 exactly | 594 / 297 (841 / 420 = 2.0024) |
| A2 594 x 420 | 1.4141 | 420 / 297 |
| A3 420 x 297 | 1 | |
| A4 297 x 210 | 0.7071 | 210 / 297 |

**Everything** scales: positions, cells, text heights, line spacing, line
weights, dashes and symbol sizes. A1 is the A3 sheet doubled, with a 1.06 mm
border and 2 mm labels. The millimetre or two that the ISO roundings leave
goes to the right and top margins: A1's drawing area is 46..820 x 70..574, so
its right margin is 21 and its top 20. The construction guide follows the
paper's real edge, not the scaled frame's.

**A portrait sheet has no frame.** The title block is a strip 389 mm long,
laid out for landscape paper. Scaled to fit across a portrait page it would
print 0.7 mm labels on A3. A portrait sheet (and a sheet whose `frame` is
empty) gets the paper less 10 mm all round as its drawing area: the
single-page plot's default margin, so the two agree. `frameFor` refuses a
portrait sheet with `InvalidArgument` rather than hand back a frame that
does not fit.

## Fields and their automatic values

`resolveFields(set, sheetIndex, context)` gives every field of a sheet. The
order of precedence is: the sheet's own override, then the set's value, then
the automatic value. `expandTemplate` fills a frame text's `{field}`
placeholders, and `{{` is a literal brace. The `FieldContext` is what the
project contributes; `fieldContextFor(document, plotDate)` builds it, so
`resolveFields` stays a pure function.

| Field | Automatic value |
|---|---|
| `sheet_number` | the sheet's position, written by the set's numbering pattern: `{n}` the position from 1, `{n:02}` padded, `{N}` the count, `{set}` the set number (default `{n}`) |
| `sheet_count` | **the number of sheets in the set**. The app left this as typed, so a set that grew to twelve sheets still said "of 1" |
| `scale` | the main viewport's scale; see below |
| `sheet_name`, `paper` | the sheet's |
| `plot_date` | the date the sheet is plotted, as `dd/mm/yy` (`frameDate`) |
| `locator_name/_date` ... `approver_name/_date` | the set's sign-offs, each with its own date; a missing date is the plot date |
| `project_line_1`, `project_line_2` | the set's, else the project's name and description |
| `project_line_3`, `project_line_4`, `organisation`, `client`, `notes`, `height_datum`, `model_name`, `set_number` | the set's (the project records no height datum or organisation) |
| `coordinate_system` | the set's, else the project's coordinate system |
| `file_name` | the project directory's name (printed twice: up the left margin and in its cell) |
| `revision` | the code of the latest revision |

**The scale** is that of the main viewport: the lowest tiling rank among the
viewports drawn to scale (plans and sections).
- If those viewports disagree, it is `AS SHOWN`.
- A key plan inset beside a plan is at its own, smaller scale by design and
  does not make the sheet `AS SHOWN`. On a sheet with no plan or section (a
  key-plan sheet) the key plan's scale is the sheet's.
- If nothing on the sheet is drawn to scale, it is `N.T.S.`.
- A section always states both scales, `H 1:500 V 1:50`, even when they are
  equal. A lone 1:500 leaves the reader to assume the vertical, and that is
  the assumption that goes wrong.
- A denominator prints as a whole number, as a scale rule reads it: 1:250 at
  8 times is `V 1:31`.

**Match lines and key-plan outlines** name the sheet they lead to by its id
(`WorldMark::sheet`). The number is looked up when the mark is drawn
(`markLabel`: `MATCH LINE CH 250.000 - SEE SHEET 3`), so reordering the
sheets cannot make a mark lie.

Removing a sheet cannot make one lie either. The marks that led to it still
name its id, and `newSheetIds` counts every id a mark names as taken, so the
next sheet added gets a new one. A mark whose sheet is gone prints its label
alone (`MATCH LINE`), not `SEE SHEET` and the number of a stranger. Example:
a key plan and three tiles, `s1` to `s4`. Remove `s4` and add a legend: the
legend is sheet 4 but its id is `s5`, and the middle tile's match line reads
`MATCH LINE`, not `MATCH LINE - SEE SHEET 4`.

Ids are numbers after a prefix, and a stored set is read unchecked. New ids
follow the highest number in use. An id too near the largest 64-bit count to
have room after it gives the lowest free numbers instead. Nothing throws, and
no id is given out twice.

## The sheet scale ladder

The single-page plot's `kStandardScales` starts at 1:100. It is unchanged,
along with its tests. Sheets hold details and sections too: a cross section
at 1:100 on a 120 mm cell is only 12 m wide. So sheets use their own ladder,
`kSheetScales`:

1:1, 2, 5, 10, 20, 25, 50, 75, 100, 125, 150, 200, 250, 500, 750, 1000, 1250,
2000, 2500, 5000, 10 000, 20 000, 25 000, 50 000.

It contains every step of the plot ladder. `sheetScaleAtLeast(needed)` gives
the first step at or above `needed`: the largest standard scale at which
something needing 1:`needed` still fits. Beyond 1:50 000 it returns `needed`
itself, as `fitScale` does.

## Layout: the presets, the ranks, snapping

`tileViewports(sheet, preset)` lays a sheet's viewports into one of the
app's eight presets. It uses the app's 3 mm gutter, inside the drawing area
inset by 1 mm (`tilingArea`). Each cell is its share of that area less half
the gutter on every side:

| Preset | Id (stored, never renamed) | Cells |
|---|---|---|
| Full frame | `full` | one |
| Two columns | `cols2` | halves side by side |
| Two rows | `rows2` | halves one above the other |
| Quarters | `quad` | four |
| Main and panel right | `sectionR` | main 66% wide, a panel right |
| Main and panel below | `sectionB` | main 64% high, a panel below |
| Main and panel in the corner | `sectionBR` | main 66% high across the top, a panel bottom-right |
| Main and two panels right | `sectionMap3d` | main 64% wide, two panels stacked right |

On A3 the tiling area is 24..409 x 36..286 mm, so "Full frame" is
25.5..407.5 x 37.5..284.5. In "Two columns" the second column starts at 218,
exactly 3 mm after the first ends at 215.

**Every kind has a rank**, and the lowest rank takes the first (largest)
cell:

| Kind | Plan | LongSection | CrossSections | Model3D | KeyPlan | Image | Legend | Notes |
|---|---|---|---|---|---|---|---|---|
| Rank | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 |
| Minimum (mm) | 35 x 30 | 70 x 45 | 70 x 45 | 30 x 24 | 30 x 26 | 8 x 8 | 16 x 8 | 16 x 8 |

The app sorted its panels by a table with no entry for its map panels. The
comparator then returned NaN, and a map could land in the big cell. That was
measured: given map, section and 3D view in that order, "Section + map + 3D"
left the map in the main cell. Here a key plan, cross sections and a 3D view
added in that worst order still put the sections in the main cell (a test
says so).

The tiling rules:
- Ties keep the sheet's order.
- Locked viewports and Notes are left where they are; the app placed notes
  by hand too.
- Viewports beyond the cell count are not moved.
- A cell never makes a viewport smaller than its kind's minimum. It grows
  right and down from the cell's top-left corner, as the app's do.

`snapRect(moving, drawingArea, others, tolerance)` snaps a moving rectangle.
Its left edge, right edge and centre line are compared with every edge and
centre line of the drawing area and of the other viewports. The smallest move
within the tolerance wins, separately on each axis. The line snapped to is
returned for the editor to draw as a guide. A tolerance of 0 turns snapping
off.

## The generators

Each generator returns sheets. None touches a document. Each is
deterministic: the same request gives the same sheets, compared whole in a
test. `addSheets(document, sheets)` adds a generator's output as ONE undo
step. It gives every sheet and viewport a new id (`newSheetIds`,
`newViewportIds`) and moves the marks' sheet references with them
(`prepareForAppend`). The paper is A3 landscape with the built-in frame
unless a `SheetTemplate` says otherwise.

**Fit to one sheet: `fitToSheet(extent)`.** This gives one plan filling the
tiling area, centred on the extent, at the largest standard scale that holds
it. A 300 x 100 m extent in 385 x 250 mm needs 1 : max(300000 / 385 = 779,
100000 / 250 = 400), so the sheet is at 1:1000. A line along an axis fits by
its length. A single point has nothing to scale to and is refused.

**Tiles: `gridSheets({area, scale, overlapM, keyPlan})`.**
- Tiles cover the area at the chosen scale and are centred on it.
- They are numbered in reading order: rows from the top, left to right.
- Each tile's plan carries a match line down the middle of every overlap it
  shares, leading to the sheet across it.
- With `keyPlan`, sheet 1 shows every tile's outline, numbered, at the scale
  that holds them all.

Example: 400 x 200 m at 1:500 with 10 m of overlap.
- A tile is 192.5 x 125 m, stepping 182.5 m across and 115 m down. That gives
  3 columns and 2 rows: six tiles and a key plan at 1:2000.
- The top-left tile's match lines are at x = 108.75, leading to sheet 3, and
  at y = 100, leading to sheet 5.

**Strips along an alignment: `stripSheets(alignment, name, {scale, overlapM,
from/to chainage, keyPlan})`.**
- Each strip is a plan rotated so the alignment runs left to right across it.
- Strips advance by the strip's length less the overlap.
- A strip is shortened, a tenth at a time, where a curve would bend out of
  it. A test checks every metre of every strip lies inside its viewport.
- Match lines run square across the alignment at each change of sheet,
  labelled with their chainage.

Example: a straight 1000 m alignment at 1:500 with 20 m of overlap gives six
strips (0, 172.5, 345, 517.5, 690 and 862.5 to 1000), each rotated to the
alignment's bearing. The first strip's match line crosses at CH 172.500.

**Cross sections: `crossSectionSheets(alignment, name, {interval or stations,
halfWidth, rows, columns, scale, exaggeration, surfaces})`.**
- The stations follow the rule of `cad::sectionStations`: every interval
  from the start, and the end.
- Each section is cut square to the alignment itself at its true chainage
  (`pointAtStationOffset` either side), left to right looking along it, as
  `crossSectionLine` draws one. It is not cut at a distance along the
  alignment's chorded polyline. That polyline is shorter than the alignment
  on every curve, so the end chainage fell past its end and was refused.
- Example: east 300 m, a 100 m radius curve, then north 300 m. The road ends
  at CH 557.0796 (200 + 100 pi / 2 + 200); its chords end at 557.0791. The
  last of the sections every 100 m is at CH 557.0796, titled
  `CROSS SECTION CH 557.080`.
- A given station within 0.1 micrometre of an end is taken as the end. One
  further out is refused.
- They are packed rows x columns per sheet, in chainage order down each
  column and then across, and spill onto as many sheets as they need.
- All share one scale and one exaggeration.
- With `scale` 0, the scale is the largest standard scale at which the width
  fits 80% of a cell.
- With `exaggeration` 0, it is the largest of 1, 2, 2.5, 4, 5, 8 and 10 at
  which the deepest sampled section fits 80% of a cell's height. With no
  surfaces it is 1, and each section is centred when drawn.
- Given stations are sorted and duplicates dropped.

Example: 100 m cut every 20 m on 2 x 2 cells gives six sections, four on the
first sheet and two on the second. The scale is 1:500, because a 40 m
section in 80% of a 191 mm cell needs 1:262. Over ground falling 20 m across
the section, the exaggeration is 2: 20 m at 1:500 is 40 mm, and 80% of a
123.5 mm cell allows up to 2.47.

**From imported plot frames: `sheetsFromPlotFrames(model, template,
&skipped)`.** The archive importer keeps each plot frame as a closed polyline
with `plot_frame.*` properties: width, height, scale, rotation, xorigin,
yorigin and the four margins (`docs/interop.md`). Each frame becomes a sheet:
- on the ISO paper its size matches (within 2 mm, either way round);
- its plan viewport is the frame's own window on the paper, kept inside the
  sheet's drawing area;
- the viewport is centred where that window falls on the ground, at the
  frame's scale;
- the frame's own layer is hidden in the viewport.

Where the paper lies comes from the frame's OUTLINE, not from its xorigin,
yorigin and rotation properties:
- the outline's first corner is the paper's corner;
- its first edge runs along the paper's bottom and gives the rotation;
- its last corner is up the paper's left side.

The properties are the file's own. An import with an origin shift (the
default "Shift Alongside" import and the command line's LOCAL option) moves
the outline and leaves the properties as they were. So does a later move or
rotation of the frame. A sheet placed from the properties showed ground
millions of metres from the frame. Width, height, scale and margins still
come from the properties. A mirrored frame is placed inside its outline.

Frames that cannot be read are listed in `skipped` rather than dropped
silently: no size or scale, not an ISO size, margins leaving nothing, or an
outline that is no longer the frame's four corners.

Examples:
- An A3 frame at 1:1000, rotated 30 degrees, with its corner at (1000, 2000)
  and margins L23 R10 T10 B35. Its window's centre (216.5, 161) mm lands at
  (1106.9945, 2247.6801).
- An A3 frame at 1:500 whose file puts its corner at (300000, 6200000),
  imported with that shifted to (0, 0). Its window's centre lands at
  (108.25, 80.5), inside the outline, not near (300108, 6200081).

**The smart layout: `smartLayout(model, request)`.** A request says what to
show, and the layout decides how to lay it out:
- a plan area;
- a plan along an alignment;
- a long section;
- cross sections every N m;
- a 3D snapshot;
- a legend;
- the paper, and a scale or 0 for auto.

The rules:
- **Plan and profile.** A plan along an alignment with its long section
  becomes plan-and-profile sheets. The plan goes in the top cell of "Main and
  panel below", and the long section of the same chainages goes under it at
  the same horizontal scale, exaggerated 10 times (H 1:500 V 1:50).
- **Long section alone.** A long section without a plan takes as many
  chainages per sheet as the sheet's width holds.
- **Plan of an area.** On auto it is one sheet at the fitted scale. At a
  fixed scale it is one sheet when the area fits, and tiles with a key plan
  when it does not. An empty area is refused.
- **3D and legend.** These can go beside the plan, in "Main and panel right"
  (one of them) or "Main and two panels right" (both). That needs three
  things:
  - the plan is the only one: a plan of an area or along the alignment, not
    both;
  - there is no long section;
  - the plan, made for the narrower main cell, still shows everything asked
    of it.

  The plan is made for that cell from the start. On auto its scale is fitted
  to the cell, and the whole plan fits there on one sheet. At a fixed scale,
  an area must fit the cell at that scale, and an alignment must still be one
  strip. When the plan does not fit, it keeps its whole sheet and the 3D view
  and legend go on a sheet of their own after it. The plan is never squeezed
  after it is made: that cut the ends off a strip, and the edges off an area.
- **Cross sections** follow on sheets of their own.
- **Auto** fits everything on as few sheets as it can.

Examples:
- A 300 x 200 m plan with a 3D view and a legend is one sheet: the plan at
  1:1250 in the main cell, the 3D view and legend to its right.
- A 180 m straight road on auto with a 3D view is one sheet. The main cell of
  "Main and panel right" is 251.1 mm wide, so the strip is at 1:750, holding
  188 m, and the whole road is in it. At a fixed 1:500 the cell holds only
  125.55 m. So the strip keeps the whole sheet (191 m), and the 3D view is
  sheet 2.
- A 180 x 100 m area at 1:500 with a legend would need 1:717 beside the
  legend. So it keeps its whole sheet, and the legend is sheet 2. A
  100 x 80 m area needs 1:398 and shares its sheet with the legend.
- A 1000 m road at 1:500, plan and profile, with cross sections every
  100 m, is eight sheets: six plan-and-profile sheets (191 m each), then two
  of cross sections (eleven sections, eight to a sheet).
- The same road on auto is one plan-and-profile sheet at 1:5000.

## Storage and undo

**No schema change.** The set is stored as versioned JSON under the
project's metadata key `sheets`. The storage layer keeps any metadata key it
does not read and writes it back unchanged (`ProjectMetadata::unknownKeys`).
So sheets reach a project's file without a migration, and a build older than
this one keeps them intact.

```json
{"format": "katana-sheets", "version": 1,
 "defaults": {"organisation": "...", "surveyor": {"name": "...", "date": "..."}, "logo_asset": "logo.png"},
 "revisions": [{"code": "A", "date": "...", "description": "...", "by": "..."}],
 "sheets": [{"id": "s1", "name": "PLAN",
             "viewports": [{"id": "vp1", "kind": "plan", "rect": [24, 36, 409, 286], "scale": 1000,
                            "centre": [150, 50], "north_arrow": true, "scale_bar": true}]}]}
```

- **Defaults are left out.** Only the format, the version, and each sheet's
  and viewport's id (and a viewport's kind) are always written. Any other
  member is written only when it differs from the default of its type, and a
  member left out reads as that default. The example's sheet is A3
  landscape with the built-in frame, and its plan is not rotated, because
  those are the defaults. So a default never changes without a version
  increase, which lets the reader supply an old version's defaults to an old
  set.
- **Doubles** are written in the shortest form that reads back to the same
  bits, and compared bit for bit against their default, so `-0.0` survives. A
  save and a load give back exactly the set that was saved; a test compares
  it whole.
- **An unplaced viewport's** empty rectangle is the default, so it is left
  out. Any other number that is not finite is refused when the set is
  written, because JSON has no form for it that reads back.
- **A newer version** than this build's (`kSheetSetVersion` = 1) is refused
  (`Unsupported`), not read wrongly. The document then refuses to overwrite
  those sheets (`CommandRejected`) and keeps the text as it was. A new member
  that must survive being saved by an older build also needs a version
  increase.

**Undo.**
- `Document::setSheetSet(set, stepName)` is the one way in. It records the
  state before and after (the JSON and the set parsed from it) as a single
  command on the document's history, so undo and redo are exact and parse
  nothing.
- Undoing the first sheet ever added leaves the metadata with no `sheets` key
  at all.
- An edit that changes nothing records no step. A refused edit changes
  nothing.
- `Document::sheetSet()` parses the stored JSON once per change. It compares
  the text rather than a revision counter, because `setMetadata` replaces the
  whole metadata and must be seen.

The helpers in `sheet_commands.hpp` are each one step:

| Edit | |
|---|---|
| `addSheets` / `addSheet(sheet, at)` | append a generator's output, or insert one sheet, with fresh ids |
| `removeSheet` | the last sheet can go too: an empty set is valid |
| `moveSheet(from, to)` | reorder; marks follow their sheet by id |
| `duplicateSheet` | a deep copy after the original, `<name> (copy)`, new ids, without the original's typed sheet number |
| `editSheet`, `editViewport(id)`, `editSheetSet` | apply a function to a copy and store the result; a function that returns an error changes nothing |

**The logo.** `importLogo(document, image)` copies the image into the
project's `assets/` directory and sets `logoAsset`, as one step.
- The image must be PNG, JPEG, GIF or BMP, recognised by its first bytes,
  not by its extension.
- It may be at most 4 MiB (`kMaximumLogoBytes`). The logo prints
  53 x 12 mm, which is 1250 x 283 pixels at 600 dpi, so a bigger file is a
  photograph that would ride along in every PDF of the set.
- The stored name is the source's stem made safe for any file system.
- The same image imported again is found and reused. A different image of
  the same name gets a name of its own (`logo-2.png`) and never overwrites
  one an undo could bring back.
- After an undo the file stays in `assets/`; only the name is undone, so a
  redo finds it.
- A drawing not yet saved as a project has nowhere to keep a logo
  (`InvalidState`).

**Cost.** Each edit writes the whole set's JSON once. The undo step keeps the
state before and after, each as the text together with the set parsed from
it, so an undo or redo swaps them back without parsing anything. Only
members that differ from their type's defaults are written.

`benchmarks/bench_sheets.cpp` measures a large set: a 10 km road drawn
plan-and-profile at 1:500 with cross sections every 20 m, which is 116
sheets and 607 viewports. It was run with `tools/compare_benchmarks.py
--alternate 4` against the first version of this code, with a second copy of
that version as the A/A control. Medians of 12 samples each, release build:

| Benchmark | First version | A/A control | Now | Ratio |
|---|---|---|---|---|
| `BM_SheetEditViewportAndUndo` (edit one viewport, read, undo, read) | 103.1 ms | 97.8 ms | 7.65 ms | 13x faster |
| `BM_SheetSetToJson` | 24.6 ms | 24.3 ms | 6.3 ms | 3.9x |
| `BM_SheetSetFromJson` (a project being opened) | 39.3 ms | 36.8 ms | 18.7 ms | 2.1x |
| `BM_SheetSmartLayoutLongRoad` (generating the 116 sheets) | 2.16 ms | 1.94 ms | 1.87 ms | within noise |

The A/A control moved by 1-11%. The JSON went from 300,942 bytes to 125,652
(0.42x). What an edit still pays is one write of the whole set. A drag in the
editor should therefore commit once, on release; it should not commit on
every mouse move.

The review fixes changed how the generators append their output and cut
cross sections. The smart layout no longer copies the sheets made so far
into a set for each append, `prepareForAppend` no longer copies the set once
per sheet it numbers, and each cross section is built on the alignment. The
same benchmarks were run again, twice, the fixed build against two copies of
the build before it (`--alternate 4`, then `--alternate 6`). Ratios of
medians, fixed over before:

| Benchmark | Run 1 | Run 2 | A/A control (runs 1, 2) |
|---|---|---|---|
| `BM_SheetSmartLayoutLongRoad` | 0.75-0.79 | 0.65-0.67 | 1.05, 1.03 |
| `BM_SheetEditViewportAndUndo` | 1.06-1.11 | 1.05-1.08 | 0.95, 1.03 |
| `BM_SheetSetToJson` | 1.06-1.22 | 1.07-1.12 | 1.14, 0.96 |
| `BM_SheetSetFromJson` | 0.92-1.14 | 0.91-1.04 | 1.23, 0.88 |

The smart layout is 1.3 to 1.5 times as fast. The other three run code the
fixes did not touch, on the same set (125,652 bytes both times). Their ratios
are at the edge of the A/A spread, and no claim is made for them either way.

## The sheet painter

`src/katana_qt/sheet_painter.hpp` draws one sheet onto any QPainter: the
editor's canvas and every PDF page go through the same `paintSheet`, so the
editor shows exactly what plots. It reads no widget and keeps its caches
(frames, cut sections, 3D snapshots, images, the plan painter's) in a
`SheetPaintCache` the caller owns.

- **Order.** White paper, then the viewports back to front, then the frame
  over them, as the owner's app drew it.
- **The frame.** Lines at their measured weights, dashes in paper
  millimetres (flat caps, so a 2.2 mm dash is 2.2 mm). Text in Arial set by
  cap height at a 200 px reference size and scaled, with the 0.82 x factor
  as a painter scale. Field values come from `resolveFields`; `centredIn`
  values are centred in their cells. A single-line field wider than the room
  to its cell's right edge is squeezed, as the app did. The notes slot wraps
  to its cell. The 14 legend glyphs are the app's procedural glyphs, redrawn
  in a -1..1 box at a 0.18 mm stroke. The logo is `fitImage` in the logo
  slot. The construction guide and faint "YOUR LOGO" / "ORGANISATION" hints
  are drawn only when the options ask (the editor).
- **Plan and key plan.** `paintPlan` on paper through a `PlanFrame` sized to
  the viewport, rotated by minus the viewport's rotation (the world
  direction that runs across the paper), clipped. `autoScale` picks the
  first of `kSheetScales` at which the drawing - or the viewport's stretch
  of its alignment - fits with 4% to spare; the title block reports the
  scale drawn. Match lines are dash-dot with their label along them; a key
  plan fades the drawing and outlines the other sheets, numbered.
- **Sections.** Cut once per drawing revision from the window's visible
  surfaces along the named alignment, the design profile added when the
  alignment has one. A long section gets a data band (design and ground
  levels at each grid chainage, then the chainage); a cross section gets
  its centreline and a "CH" caption. Crossings are dashed with their layer
  name and a circle at their level. The datum is written inside the plot.
- **3D snapshot.** The 3D view's renderer on a white background, capped at
  200 dpi and 8 megapixels.
- **Legend, notes, image.** A sample line per layer in use (in its paper
  colour and weight); wrapped notes; a project image fitted.
- **Furniture.** North arrow (pointing where world +Y is on the paper),
  scale bar (1, 2 or 5 times a power of ten, at most 50 mm), and the view's
  title underlined in its bottom-left corner.
- **Problems.** A viewport that cannot be drawn - no such alignment, no
  surface to cut - is outlined and left empty on paper, reported in
  `SheetPaintStats::problems`, and explained inside it in the editor.

`plotSheetsToPdf` writes the whole set, or chosen sheets, to one vector PDF,
each page the size of its sheet's paper. File > Plot Sheets to PDF and
`katana --plot-sheets out.pdf` use it; a project with no sheets plots one
fitted to the drawing without adding it to the project.

## The sheet editor

File > Sheets (Ctrl+Shift+P) opens `src/katana_qt/sheet_editor.hpp`, a window
beside the drawing:

- **Sheets** on the left: add, duplicate, remove, reorder, rename
  (double-click).
- **The canvas**: the painter's output, cached and painted again only when
  the document, the zoom or the pan changes. Click to select a viewport;
  drag to move it and the handles to resize it, snapping to the drawing area
  and the other viewports (Alt: no snap); Shift-drag pans the drawing inside
  a plan; arrows nudge (Shift: 10 mm); Delete removes; the wheel zooms and a
  middle or empty-paper drag pans; double-click the desk to fit the page.
  A drag is ONE command, committed on release.
- **Properties** on the right: a viewport's title, scale (or Auto),
  rotation, centre, exaggeration, alignment and chainages, north arrow,
  scale bar, hidden layers, text, lock; or, with nothing selected, the
  sheet's paper, orientation, frame and legend block, and a table of every
  field the frame prints with this sheet's overrides.
- **Toolbar**: Generate Sheets (the drawing fitted, tiles with a key plan,
  strips along an alignment, plan and profile, cross sections, one sheet per
  imported plot frame; appended or replacing), New Sheet, Add View, Tile
  (the eight presets), Title Block (organisation, project lines, client, set
  number and numbering, coordinate system, datum, the five sign-offs, notes,
  revisions and the logo), Fit Page, Plot Sheet, Plot All.

The tests are `tests/qt_widgets/test_sheet_painter.cpp` and
`test_sheet_editor.cpp`.

## Section smarts

A long section or a cross section viewport fits itself to what it shows,
keeps its labels off one another, shades its cut and fill, and says what
each service it crosses is and how deep. Every decision made with numbers is
headless, in `katana_cad`, so an agent reads the same numbers the sheet
prints:

| Header (`include/katana/cad/plotting/`) | What it decides |
|---|---|
| `section_fit.hpp` | the automatic scale and exaggeration (`fitSection`, `fitExaggeration`, `kSectionExaggerations`), the label steps (`labelStep`, `roundStepAtLeast`, `stepText`, `gridValues`), the plot's layout (`sectionPlotLayout`), a viewport's area, stations and half width (`sectionDrawingArea`, `viewportStations`, `viewportHalfWidth`) |
| `section_annotation.hpp` | the design and ground series (`designSeries`, `groundSeries`), levels (`levelAt`, `levelRange`), cut and fill (`cutFillAt`, `cutFillText`, `earthworkRegions`, `outlineArea`), a crossing's note (`ownLevel`, `crossingNote`) and where labels go (`placeLabels`) |

The painter is `src/katana_qt/plotting/section_painter.hpp`
(`paintSectionViewport`, `fitSectionViewport`). It measures its text and
draws what those decide. It draws through the sheet painter's paper, pens
and text setter (`SectionCanvas`), so every colour and weight still passes
through `paperPen`, `dashedPen` and the `TextSetter`. The tests are
`tests/cad/plotting/test_section_fit.cpp` and `test_section_annotation.cpp`
without pixels, and `tests/qt_widgets/plotting/test_sheet_sections.cpp` and
`test_section_editor.cpp` on paper and in the editor.

### The automatic section scale

A section viewport with `autoScale` is drawn at a scale and an exaggeration
chosen for it (`fitSection`):
- **What must fit across.** A long section's chainage range
  (`source.chainageFrom` to `chainageTo`), or the whole alignment when the
  range is empty. A cross section's two half widths. With `autoCentre` off,
  the span is measured both ways from the viewport's centre.
- **What must fit up.** The levels shown: every series' samples over that
  range, the levels of the crossings drawn (hidden layers left out), and at
  a cross section the design profile's level at the centreline
  (`levelRange`). All the cross sections of one viewport share one scale and
  one exaggeration, the deepest setting it.
- **The plot** is what the viewport leaves once its title strip, its level
  labels and its data band or axis values are taken out
  (`sectionPlotLayout`). The level labels are measured, so the fit is for
  the plot that is drawn. The section fills 90% of it each way
  (`kSectionFitFill`).
- **The scale** is the largest of `kSheetScales` at which the span fits the
  plot's width, and at which the depth would still fit its height at true
  scale: a deep, narrow section is drawn smaller rather than out of its plot.
- **The exaggeration** is then the largest of `kSectionExaggerations` (1, 2,
  2.5, 4, 5, 8, 10, 20) at which the depth fits the height
  (`fitExaggeration`). One that gives the vertical scale a whole denominator
  is preferred: at 1:750, 2.5 (V 1:300) rather than 4 (V 1:187.5). A section
  with no depth (flat, or nothing sampled) takes the flat value: 10 for a
  long section (H 1:500 V 1:50, as long sections are drawn), 1 for a cross
  section. The cross-section generator uses the same ladder, so a generated
  section and a fitted one agree.

The painter decides it once per paint, in the same step that decides an
automatic plan's scale (`SheetPainter::run`). So the viewport's title
(`LONG SECTION H 1:750 V 1:75`) and the title block's scale cell report the
scale drawn. A cross-section viewport with one station keeps its title
`CROSS SECTION CH 120.000`, and its scale is in the title block.
`resolveSectionViewport` (`src/katana_qt/sheet_painter.hpp`) gives the same
answer without painting; the editor shows it as the tooltip of the scale
box.

Example: a 180 m road whose levels span 9 m, in a long section filling the
A3 drawing area. The band leaves a 363 x 210 mm plot. 180 m in 90% of
363 mm needs 1:551, so the scale is 1:750. At 1:750, 90% of 210 mm holds 9 m
up to 15.75 times, so the exaggeration is 10: `H 1:750 V 1:75`.

A stored scale that is not a positive number (the JSON is read unchecked)
is reported as a problem and the viewport is left empty.

### Labels that do not collide

- **Steps from measured widths.** The grid along the section is the
  smallest round step (1, 2 or 5 times a power of ten) at least 10 mm apart
  at which its widest label, as the painter measures it, still clears its
  neighbours by 2 mm (`labelStep`). A finer step writes more decimals
  (`stepText`: 0.5 gives one, 0.25 two), so the widest label is measured
  for each step tried. A data band's values are turned across their row, so
  there their height is what must clear. The level step up the side is at
  least 7 mm on the paper.
- **The plot is laid out around its labels.** The column left of the plot is
  as wide as the widest level label (or the band's row names), so no level
  is drawn outside the viewport. A band that would leave the plot less than
  30 mm high gives way to plain axis values.
- **Placing.** Every label inside the plot (the datum, the key of series,
  the centreline's levels, each crossing's note and offset) is placed by
  `placeLabels`: in priority order, each takes the first of its candidate
  places that lies inside the plot and clear of every label placed before.
  The labels nearest the middle go first. A label with no free place is
  dropped, never drawn over another; the notes dropped (a crossing's, the
  centreline's levels) are counted in `SheetPaintStats::sectionNotesDropped`.
  The level labels, axis values, band columns and caption are placed the
  same way inside the viewport.
- **Nothing outside the viewport.** Lines, shades and labels are clipped to
  the viewport, and a long section with a chainage range is cut to exactly
  that range.
- **What cannot be drawn is said.** A section with no room in its viewport,
  a range that lies off its plot, or a stored scale, chainage, centre or
  width that is not a finite number is reported in
  `SheetPaintStats::problems` and outlined empty, as any viewport that
  cannot be drawn is; it is not counted as drawn.

### Cut and fill

`earthworkRegions(design, ground)` gives the regions between the design and
the ground. The design is the first series whose name starts with "design",
in any case (the alignment's grade line, `design ROAD`, or a design surface);
the ground is the first other series (`designSeries`, `groundSeries`).
- Where the ground is above the design the region is CUT, shaded light red.
- Where it is below, it is FILL, shaded light green.
- Regions split where the two lines cross between samples, at the crossing
  point on the straight lines between them, and at a gap in either.
- They are drawn under the grid and the lines.

A long section's data band gains a **CUT/FILL** row when it has both: the
design less the ground at each column, signed to three decimals (`+2.500`
fill, `-2.500` cut, `0.000`; `cutFillText`). A cross section writes the
design and ground levels at its centreline beside it, and the cut or fill
between them (`DESIGN RL 25.000`, `GROUND RL 23.500`, `FILL 1.500`), with a
tick across the centreline at each level. Its design level is the design
series', else the alignment's profile's. The note goes clear of the
services' lines when it can. Among services close either side it goes
across one, masked, but only after the services' own notes have their
places.

The long section's design profile is shifted by the alignment's start
chainage, so the design lies over the ground however the chainages are
numbered.

### Services crossed

Each line the section crosses that has a level is marked with a ring at it
and noted (`crossingNote`):
- **The level** is the entity's own (its vertex heights, straight between
  the two either side of the crossing; `ownLevel`), else the level the
  section found under it.
- **The depth** is the ground less its own level at the crossing: `WATER RL
  22.00 D 1.70`. Above the ground (an overhead line) it is a height:
  `POWER RL 36.00 H 6.00`. A line draped on the ground has no depth.
- The note is stood up beside its line, above or below its marker, on
  either side, never across another crossing's line or the centreline.
  When the whole does not fit, the layer alone is written. Its offset
  (cross section) or chainage (long section) is written under the marker.
- A crossing on a layer the viewport hides is neither drawn nor noted.

### A chainage range

A long section whose source has a range shows exactly that range: the plot
ends at its two chainages, drawn heavier, and nothing of the section is
drawn past them. Each end is written in full just inside it (`30.000`,
`150.000`), in the chainage row with its levels above. Without a range the
range is the whole alignment, ends labelled the same way.

### In the editor

A section's scale box offers Auto, as a plan's does. With Auto the
exaggeration box is disabled, since it is chosen with the scale, and a fixed
scale chosen from Auto keeps the exaggeration Auto drew. The tooltip is
worked out through the canvas's own cache (`SheetCanvas::paintCache`), so a
selection cuts nothing again. New long
sections and cross sections are added with Auto on. Setting it is the usual
one-step `editViewport` (`viewport.autoScale = true`).

## Not yet

- **Change notifications.** A sheet edit notifies the document's listeners
  like any model change, so it still rebuilds the 3D view's scene.
- **Frames and furniture.** More frames, such as a plan-sheet frame with a
  north-arrow zone, and a reader for title-block files. The revision table
  is stored and edited but not yet drawn by any frame.
- **Background plotting.** A large set is plotted on the GUI thread with a
  wait cursor; the painter is reentrant, so moving it to a job is the next
  step.
