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
| `kind` | `Plan`, `LongSection`, `CrossSections`, `Model3D` (a snapshot of the 3D view), `Legend`, `Notes`, `Image`, `KeyPlan`, `SheetIndex` (the drawing register), `Revisions` (the revision table) |
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
| `revisionLimit` | a Revisions table's newest so many revisions; 0 for every one |

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

| Kind | Plan | LongSection | CrossSections | Model3D | KeyPlan | Image | Legend | Notes | SheetIndex | Revisions |
|---|---|---|---|---|---|---|---|---|---|---|
| Rank | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
| Minimum (mm) | 35 x 30 | 70 x 45 | 70 x 45 | 30 x 24 | 30 x 26 | 8 x 8 | 16 x 8 | 16 x 8 | 70 x 30 | 50 x 20 |

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
- **Imagery under the plan.** Rasters (an aerial photo, a GeoTIFF), point
  clouds and mesh footprints print beneath the linework, as the plan view
  shows them. A raster is cropped to what the viewport shows and averaged
  down to at most `SheetPaintOptions::rasterDpiCap` (200 dpi in a PDF, 110 in
  the editor) and 16 megapixels, so a sheet over a 400-megapixel orthophoto
  embeds only its viewport's few megapixels; a cloud is splatted into an
  image of the viewport at the same cap; a footprint is a 0.13 mm dashed
  outline. `docs/plan_view.md`, "Imagery on paper", has the rule and the
  measurements.
- **Sections.** Cut once per drawing revision from the window's visible
  surfaces along the named alignment, the design profile added when the
  alignment has one. A long section gets a data band (design and ground
  levels at each grid chainage, then the chainage); a cross section gets
  its centreline and a "CH" caption. Crossings are dashed with their layer
  name and a circle at their level. The datum is written inside the plot.
- **3D snapshot.** The 3D view's renderer on a white background, capped at
  200 dpi and 8 megapixels. The dpi cap is the same option the plan's
  imagery is capped by.
- **Legend, notes, image.** A sample line per layer in use (in its paper
  colour and weight); wrapped notes; a project image fitted.

  200 dpi and 8 megapixels.
- **Legend, notes, image.** What the sheet's plans show, a sample each as
  it prints ("The smart legend", below); wrapped notes; a project image
  fitted.
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

- **Sheets** on the left, each with a picture of itself: add, duplicate,
  remove, reorder (drag), rename (double-click).
- **The canvas**: the painter's output, cached and painted again only when
  the document, the zoom or the pan changes. Click to select a viewport;
  drag to move it and the handles to resize it, snapping to the drawing area
  and the other viewports (Alt: no snap); Shift-drag pans the drawing inside
  a plan; arrows nudge (Shift: 10 mm); Delete removes; the wheel zooms and a
  middle or Space drag pans; an empty-paper drag is a rubber band;
  double-click the desk to fit the page. Several viewports at once, the
  clipboard, the paper grid and the rulers are in "Editing on the canvas".
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

## Sheets on the command line

Everything the editor does to sheets can be typed. The verbs are in
`include/katana/cad/plotting/sheet_verbs.hpp`: `runSheetVerb(document,
words)` runs one line and returns its reply, with no Qt and no window. The
`CommandInterpreter` hands them on, so they work in three places:
- the window's command line;
- `katana_cli` scripts and `-c`;
- `katana --command`.

A person, a script and an AI agent drive the sheets with the same words.
Only plotting needs the window, because it paints.

**The words.**
- Verbs, keys and words are case-insensitive. Separators in a key or field
  name do not matter: `project_line_1`, `ProjectLine1` and `project1` are one
  key.
- A sheet is its number in the set (`3`) or its id (`s3`). A view is its id
  (`vp7`), unique across the set.
- Options are `key=value`. A switch is `on` or `off` (`yes`/`no`,
  `true`/`false` and `1`/`0` also work).
- Double quotes group words (`""` is an empty value). `\n` in a text is a
  line break.
- A scale is `500`, `1:500` or `auto`. A rectangle is `x0,y0,x1,y1` in paper
  millimetres (either corner first).
- Words left over after the sheet are the name or the value, so
  `SHEET RENAME 2 LONG SECTION CH 0 TO 500` needs no quotes.

| Verb | What it does | Undo step |
|---|---|---|
| `SHEETS [LIST]` | every sheet: number, id, name, paper, orientation, frame, legend block, its own fields, and each view's id, kind, scale and rectangle | |
| `SHEETS JSON [path]` | the set's JSON (`sheet_json.hpp`), printed, or written to a file | |
| `SHEETS SAVE path` | the JSON written to a file | |
| `SHEETS LOAD path` | the whole set replaced from a JSON file | `LOAD_SHEETS` |
| `SHEETS CHECK [sheets=1,3-5] [json]` | the preflight checks ("Preflight checks"): `checked 2 sheets: 1 error, 3 warnings`, then a finding a line, `severity code sheet view message="..." fix="..." subject="..."` (`checkReplyLine`; `-` for the set or no view); `json` gives `findingsToJson` instead. In the window, with what the painter knows (`SheetVerbContext::check`) | |
| `SHEETS PAGESETUP [style=colour\|grey\|mono] [lineweight=f] [dpi=n] [pattern=text] [filepersheet=on\|off]` | the set's page setup ("Plot styles and output"), printed as `pagesetup style=... lineweight=... dpi=... pattern="..." filepersheet=...`, or changed; refused whole by `validatePageSetup` | `PAGE_SETUP` |
| `SHEET NEW [name] [paper=A1] [portrait] [frame=off] [legendblock=off] [at=n]` | a blank sheet, at the end or at position n; no name numbers it | `ADD_SHEET` |
| `SHEET REMOVE n` / `MOVE n to` / `COPY n` | `removeSheet`, `moveSheet`, `duplicateSheet` | `REMOVE_SHEET`, `MOVE_SHEET`, `DUPLICATE_SHEET` |
| `SHEET RENAME n name` | | `RENAME_SHEET` |
| `SHEET SET n [paper=] [orientation=] [frame=on\|off] [legendblock=on\|off] [name=]` | the sheet's paper and frame | `EDIT_SHEET` |
| `SHEET FIELD n field [value]` | the sheet's own value for a field its frame prints (`sheet_number`, `scale`...); `""` clears it; no value prints it, marked `automatic` when the sheet has none | `SET_SHEET_FIELD` |
| `SHEET SUGGESTPAPER n [scale=n\|auto] [apply=on]` | the smallest paper that holds what the sheet's main plan (`mainPlanOf`) shows at its drawn scale or `scale=` (`fitPaperToViewport`): `sheet 1 view=vp1 scale=500 paper=A0 orientation=landscape frame=a3_landscape fill=0.761`; `apply=on` puts the sheet on it (`choosePaperForScale`) | `CHOOSE_PAPER` with `apply=on` |
| `VIEW ADD n kind [option=value...]` | a view placed as the editor's Add View places one, then given the options | `ADD_VIEWPORT` |
| `VIEW SET id option=value...` | | `EDIT_VIEWPORT` |
| `VIEW REMOVE id` | | `REMOVE_VIEWPORT` |
| `VIEW LIST [n]` | every view, with every option it has | |
| `VIEW FIT id` | a plan's or key plan's rectangle fitted to what it shows (`fitViewportToContent`) | `FIT_VIEWPORT_TO_CONTENT` |
| `VIEW BESTROTATE id` | a plan turned to the rotation that shows most of it, and scaled to fit (`rotateToBestFit`): `rotated vp1 rotation=deg scale=n` | `ROTATE_TO_BEST_FIT` |
| `TILE n preset` | `tileViewports`, by the preset's id (`sectionR`) or its menu name (`Main and panel right`, no quotes needed) | `TILE_VIEWPORTS` |
| `ARRANGE n` | `autoArrange`: no two views overlapping, the main view first; `arranged sheet n moved=... overlapping=... unplaced=... mainshrunk=on\|off` and the sheet | `AUTO_ARRANGE` |
| `ARRANGE ALIGN left\|right\|top\|bottom\|hcentre\|vcentre id id ...` | `alignViewports` on the views' sheet (one sheet at a time) | `ALIGN_VIEWPORTS` |
| `ARRANGE DISTRIBUTE across\|up id id id ...` | `distributeViewports`: equal gaps, the first and last staying | `DISTRIBUTE_VIEWPORTS` |
| `ARRANGE MATCHSCALE from id id ...` | `matchScale`: the views take the scale `from` is drawn at (an automatic plan's as the painter resolves it) | `MATCH_SCALE` |
| `GENERATE kind [option=value...] [replace=on]` | a generator's sheets, appended, or in place of every sheet | `GENERATE_SHEETS` |
| `GENERATE register [paper=] [portrait] [frame=]` | the drawing register and the revision table on a cover put first (`addRegisterSheet`); refused when the set has one, and with `replace=on` | `ADD_REGISTER_SHEET` |
| `TITLEBLOCK [LIST]` | every shared title-block value, the logo and the revisions | |
| `TITLEBLOCK field [value]` | reads or sets one value | `EDIT_TITLE_BLOCK` |
| `TITLEBLOCK REVISION [LIST]` | the revisions, one a line | |
| `TITLEBLOCK REVISION ADD code date description [by]` | a revision; a code already used is refused | `ADD_REVISION` |
| `TITLEBLOCK REVISION REMOVE code` | | `REMOVE_REVISION` |
| `TITLEBLOCK LOGO path` | `importLogo`; `""` removes the logo | `SET_LOGO` |
| `HELP SHEETS` | every verb and option (`sheetVerbHelp`) | |
| `PLOTSHEETS [path] [format=pdf\|pdfs\|png\|tiff] [style=colour\|grey\|mono] [sheets=1,3-5] [dpi=n] [lineweight=f] [folder=path] [pattern=text]` | the window only: the sheets plotted as the Plot dialog and `--plot-sheets` plot them (`parsePlotSheets`, `plotRequestFor`), the page setup giving what is not said; one PDF unless `format=` says otherwise (`folder=` alone is a PDF a sheet); the summary, then `file="path"` a line for each file written | |

**The view kinds** are the stored names (`plan`, `key_plan`, `long_section`,
`cross_sections`, `model_3d`, `legend`, `notes`, `image`, `sheet_index`,
`revisions`), plus some common words: `profile`, `xs`, `sections`, `3d`,
`key`, `register`, `drawing_register`, `revision_table`. A new view is set up as Add
View sets one up (`defaultViewport`):
- On an empty sheet it fills the tiling area. Otherwise it gets its kind's
  own size, centred.
- A plan is automatic in scale and centre, with a north arrow and a scale
  bar.
- Sections run along the drawing's first alignment. Cross sections are cut
  half way along it, or half way along the one named by `alignment=`.
- A key plan is automatic in scale and centre and stores no outlines: it
  outlines the sheets' plans where they are when it is drawn, as the
  editor's Add View > Key Plan does ("The live key plan").
- An image view needs `file=path`, which is copied into the project's
  `assets/` as a logo is (`importImageAsset`, up to 32 MB), or
  `text=name` for a file already there.

| View option | Views | |
|---|---|---|
| `rect=x0,y0,x1,y1` | all | refused when it misses the paper |
| `scale=500\|1:500\|auto`, `centre=x,y\|auto` | plan, key plan, sections | `auto` sets `autoScale` / `autoCentre`; a section's automatic scale is `fitSection`'s ("The automatic section scale") |
| `rotation=deg` | plan, key plan, 3D | the world direction across the paper, counter-clockwise |
| `alignment=`, `from=`, `to=` | plan, key plan, sections | the alignment must exist |
| `stations=a,b,c`, `interval=`, `halfwidth=` | cross sections | |
| `ve=` | sections | vertical exaggeration |
| `tilt=deg` | 3D | 1 to 89 |
| `north=on\|off`, `scalebar=on\|off` | plan, key plan | |
| `grid=none\|ticks\|crosses\|lines`, `gridinterval=m\|auto` | plan, key plan | the coordinate grid ("The coordinate grid") |
| `legend=this_sheet\|whole_set\|whole_drawing` (or `scope=`) | legend | what the legend lists ("The smart legend") |
| `revisions=n\|all` | revisions | the newest n revisions, or every one |
| `locked=on\|off`, `title=text` | all | an empty title is the automatic one |
| `text=text` | notes, image | |
| `hide=layer`, `show=layer`, `hidden=a,b` | all | the view's hidden layers |
| `file=path` | image | |

An option a kind does not have is refused by name (`scale= is for plan,
key_plan, long_section and cross_sections views, not legend`). A view's kind
is fixed: remove it and add another.

**GENERATE** runs a generator and adds its sheets (`addSheets`), or with
`replace=on` puts them in place of every sheet, as one step:

| Kind | Generator | Options |
|---|---|---|
| `fit` | `smartLayout` of the drawing, `area=` or `alignment=` | `scale=auto\|n`, `model3d=on`, `legend=on`, `interval=`, `halfwidth=` |
| `grid` | `gridSheets` over the drawing or `area=` | `scale=500`, `overlap=m`, `keyplan=on\|off` |
| `strips` | `stripSheets` at a fixed scale; on `auto`, the whole alignment fitted on one sheet (`smartLayout`) | `alignment=`, `scale=`, `overlap=`, `from=`, `to=`, `keyplan=` |
| `profile` | `smartLayout`, plan and profile | `alignment=`, `scale=`, `interval=`, `halfwidth=`, `model3d=`, `legend=` |
| `sections` | `crossSectionSheets` | `alignment=`, `interval=` or `stations=`, `halfwidth=`, `rows=`, `columns=`, `scale=`, `ve=auto\|n` |
| `frames` | `sheetsFromPlotFrames`, and the frames skipped, with the reason | `frame=on\|off` |
| `register` | `addRegisterSheet`: a cover first in the set, the drawing register beside the revision table | `paper=`, `portrait`, `frame=` |

All kinds but `frames` also take `paper=`, `portrait` or `landscape`, and
`frame=`. `alignment=` may be left out when the drawing has only one. An
option the kind does not take is refused, and the reply lists the ones it
does. The document holds neither the surfaces a section samples nor
the imagery a plan draws, so the front end supplies them
(`SheetVerbContext`):
- The window passes everything its plan view draws, as the extent `fit` and
  `grid` cover, and its visible surfaces for `sections`.
- Headless, the extent is the drawing's entities, and each section is
  centred when drawn at an exaggeration of 1.
- The window also gives `SHEETS CHECK` its checker (`checkSheetsFor`: the
  imagery and meshes, the painter's rule for an automatic plan), and
  `ARRANGE MATCHSCALE`, `VIEW FIT`, `VIEW BESTROTATE` and
  `SHEET SUGGESTPAPER` what each view shows with its reference layers
  (`viewportContent` in `plotting/sheet_arrange.hpp`). Headless they check
  the document (`checkDocumentSheets`) and measure the drawing.
- The context is asked for only by the verbs that need it.

**A whole round trip, headless.** The `--command` lines run before
`--sheets-json` and `--plot-sheets`, so an agent can lay a set out, check it
and plot it in one run, reading each reply on stderr:

```sh
katana <copy-of-project> --command "GENERATE fit legend=on" \
    --command "VIEW SET vp1 grid=ticks" --command "GENERATE register" \
    --command "SHEETS CHECK" --plot-sheets out.pdf
```

The `qt_sheets_agent_headless` test (`tools/check_sheets_agent_headless.cmake`)
runs such a line with every verb above, and the verbs themselves are tested
in `tests/cad/plotting/test_sheet_agent_verbs.cpp`.

**Title-block fields** are the set's shared values (`titleBlockValue`,
`setTitleBlockValue`):
- `organisation`, `project1` to `project4`, `client`, `setnumber`;
- `numbering`, the sheet-number pattern, which must contain `{n}`;
- `coordsys`, `datum`, `model`, `notes`;
- `<role>name` and `<role>date` for the locator, surveyor, compiler, reviewer
  and approver.

The frame's own field names (`project_line_1`, `height_datum`) are accepted
too. A project line cleared at the end of the list is removed from it, so
clearing the lines returns the set to its default.

**Replies** are one fact per line, in the `key=value` form the options take,
so an option an agent reads can be typed back. Text values are in double
quotes, with a line break written as `\n` and a backslash as `\\` (and a
typed `\n` in a name, a title, a text or a title-block value is read as a
line break, `\\` as a backslash, and any other backslash as itself, so
`C:\data` needs no escaping). A text typed back as the reply wrote it is
the text stored. The command line has no way to type a double quote inside
a value, so a value holding one (only a loaded set can) is written `\"` and
cannot be typed back. What only describes (a
view's `id=`, `kind=` and `marks=`, a sheet's `views=`) is not an option. Numbers are rounded to a
millionth and written in their shortest form. For example:

```
2 sheets
sheet 1 id=s1 name="PLAN" paper=A3 orientation=landscape frame=a3_landscape legendblock=on views=2
  view id=vp1 kind=plan scale=500 rect=24,36,409,286
  view id=vp2 kind=legend rect=176.5,111,256.5,211
sheet 2 id=s2 name="KEY PLAN" paper=A4 orientation=portrait frame=a3_landscape legendblock=on views=1
  view id=vp3 kind=key_plan scale=auto rect=11,11,199,286
```

An edit replies with what it made: `added sheet 3 id=s5 ...`, `generated 7
sheets: 1 to 7` followed by each sheet, or `view id=vp2 ...` after a `VIEW
SET`. An error names what was refused and why, with a code an agent can test:
- `NotFound`: `no sheet 9: the set has 3 sheets`, a view or alignment that
  does not exist;
- `ParseFailure`: a number or list that cannot be read;
- `InvalidArgument`: a value out of range, or an option a kind does not
  take;
- `AlreadyExists`: a revision code in use;
- `Unsupported` (a newer version) or `ParseFailure` (text that is not a
  set) with "the project's sheets cannot be read": the stored sheets cannot
  be read. Every verb but `SHEETS LOAD` is then refused, so neither a list
  shows them as none nor an edit made on nothing replaces them. `SHEETS
  LOAD` replaces them on purpose, as one step `UNDO` takes back.

**One step each.** Every edit goes through `sheet_commands.hpp`, so it is ONE
undo step. An edit that is refused, or has one bad option among good ones,
changes nothing and records nothing: `VIEW SET vp1 scale=100 tilt=45` on a
plan keeps neither. `VIEW ADD` and `VIEW SET` try every option before an image
is copied into the project. The same lines give the same set every time.

**The window and the switches.**
- `PLOTSHEETS path.pdf [sheets=1,3-5] [dpi=300]` is read by the window with
  the interpreter's tokenizer (`CommandInterpreter::tokenize`) and
  `parsePlotSheets`. It calls the same `MainWindow::plotSheetsToPdf` as
  File > Plot Sheets to PDF.
  - `sheets=` takes numbers, ids, ranges (`2-4`, `s2-s4`) and `all`, in the
    order given.
  - `dpi=` is what 3D snapshots and images are rasterised at, 72 to 1200.
- `katana project --sheets-json out.json` writes the project's sheets and
  exits; `-` writes them to stdout.
- Without `--screenshot`, `--command` lines run before `--sheets-json` and
  `--plot-sheets`, and a refused one fails the run. So an agent can lay out,
  keep and plot in one headless run (`docs/headless.md`):
  `katana project --command "GENERATE grid scale=500" --command SAVE
  --sheets-json - --plot-sheets out.pdf`.

The tests are `tests/cad/plotting/test_sheet_verbs.cpp`, which types lines
through the interpreter:
- every verb, with its reply and its errors;
- each edit undone and redone exactly, one step at a time
  (`EveryEditIsOneUndoStepThatUndoesExactly`);
- the JSON saved, loaded and compared whole
  (`SheetsJsonSaveAndLoadRoundTripTheWholeSet`);
- the same lines giving the same set (`TheSameLinesMakeTheSameSet`).

`cli.sheet_verbs_lay_out_edit_and_list_the_sheets` runs the verbs in
`katana_cli`. `qt_sheets_headless` runs them, `PLOTSHEETS` and both switches
in the window.

## Preflight checks

A plot that comes out with a view under the title block, a plan over empty
ground or a blank "Surveyed by" wastes the paper and the time of whoever
reads it. `include/katana/cad/plotting/preflight.hpp` finds such things
before the plot. `checkSheets(set, model, context, options)` reads the sheet
set, the drawing and the project's field values and returns a list of
`Finding`s. It changes nothing and needs no Qt, and the same input gives the
same findings in the same order.

A finding has:
- a **severity**. `Error`: the sheet is wasted, because something is not
  drawn, is cut off or is covered. `Warning`: probably not what was meant.
  `Info`: worth knowing;
- a stable **code** (below). Codes are never renamed, so an agent can key on
  them;
- where it is: the sheet's position and id, and the viewport's id. A finding
  about the whole set has no sheet;
- a **subject**, what else it names: the field, the other viewport of an
  overlap, the alignment, the sheet a mark leads to;
- a **message** and a **fix**, in words.

The order is fixed. The set's own findings come first. Then each sheet in
turn: the sheet's own findings, then its viewports' from back to front, each
viewport's in the order `preflightChecks()` lists them.

| Code | Severity | Found when |
|---|---|---|
| `field.empty` | Warning (some Info) | a value the frame prints resolves to nothing: the organisation, a sign-off name, the height datum, the coordinate system... |
| `logo.missing` | Info | the logo slot is empty |
| `logo.unreadable` | Warning | the logo named is not in the project or cannot be read |
| `pagesetup.invalid` | Error | the set's page setup would be refused by a plot: a resolution or line weight scale out of range, or a file-name pattern that cannot be expanded (`validatePageSetup`) |
| `sheet.duplicate-id` | Error | a sheet has no id, or an earlier sheet's |
| `sheet.duplicate-name` | Warning | a sheet has an earlier sheet's name, ignoring case and spaces at the ends |
| `frame.unknown` | Error | the sheet names a frame this build does not have |
| `portrait.no-frame` | Warning | a portrait sheet asks for a frame; it prints without one |
| `sheet.empty` | Warning | a sheet has no views |
| `viewport.duplicate-id` | Error | a viewport has no id, or one used before in the set |
| `viewport.unplaced` | Warning | a viewport has no rectangle, so it does not print; nothing more is checked on it |
| `viewport.outside` | Error or Warning | off the paper or under the title block (Error); past the drawing area into the margin (Warning) |
| `viewport.overlap` | Warning or Info | a viewport covers part of an earlier one; a panel set wholly inside a view is an inset (Info) |
| `viewport.too-small` | Info | smaller than its kind's minimum (the tiling table above) |
| `scale.invalid` | Error | a fixed scale that is not a positive number; an automatic one with nothing to fit and no usable stored scale |
| `scale.non-standard` | Info | a fixed scale not on `kSheetScales`; the fix names the steps either side |
| `plan.alignment-missing` | Warning | a plan or key plan follows an alignment the drawing does not have |
| `plan.empty` | Warning | nothing a plan or key plan draws is in its window |
| `text.too-small` | Warning | drawing text in a plan prints under 1.8 mm at the plan's scale |
| `grid.invalid` | Error | a plan's coordinate grid cannot be drawn at the window it is drawn at: its lines would be closer than `kGridMinimumSpacingMm` (`planGrid`) |
| `grid.empty` | Info | a plan's grid interval is wider than its window, so no grid line prints |
| `section.alignment-missing` | Error | a section has no alignment, or names one the drawing does not have |
| `section.alignment-invalid` | Error | the alignment cannot be solved |
| `section.no-stations` | Error | cross sections with no chainage to cut at |
| `section.station-outside` | Error or Warning | a cross section's chainage is off the alignment (Error); a long section's range runs past its end (Warning), or lies wholly off it (Error) |
| `section.no-surface` | Error | no surface to cut, and for a long section no design profile either |
| `image.missing` | Error | an image panel names no file, or one the project does not have |
| `notes.empty` | Info | a notes panel has no text |
| `legend.empty` | Info | a legend lists nothing: the plans it reads, at its scope, show nothing (`computeLegend`) |
| `legend.overflow` | Warning | a legend has more entries than fit (`layoutLegend`, the labels measured by Arial's widths) and prints "+N more" |
| `revisions.empty` | Info | a revision table on a set with no revisions prints its headings alone |
| `table.overflow` | Warning | a drawing register or revision table has rows that do not fit, laid out as the painter lays it out (`layoutViewportTable`), and prints "+N more" or "+N earlier" |
| `matchline.dangling` | Warning | a match line or key-plan outline leads to a sheet that no longer exists |

How the harder checks decide:
- **Outside.** The limits are the paper, the frame's title-block box and the
  drawing area (`drawingArea`). They are allowed 0.05 mm, a hand-drawn
  rectangle's rounding. A viewport filling the drawing area exactly is
  inside. A frameless sheet has no title block.
- **Overlap.** Two rectangles overlap when they share more than 0.5 mm on
  both axes, so views snapped edge to edge do not. The finding goes on the
  viewport in front and names the one behind.
- **An empty plan.** The window is the viewport's rectangle at its scale,
  about its centre, turned by its rotation. So a 100 x 10 m window turned 45
  degrees sees a point at (30, 30), which the level window misses.
  - Lines and polylines are clipped against the window itself, not tested by
    their bounding box. A diagonal line whose box covers the window but
    which passes 49.5 m from it is not seen.
  - A window inside a HATCHED closed outline or circle looks at its fill,
    and counts as showing it. Inside a bare outline - a site boundary, a
    buffer circle - there is nothing to see, so the plan is empty unless
    the outline crosses the window.
  - A dimension is seen by everything it draws (its line, arrows and
    label, `queryExtents`), not only by the two points it measures.
  - Hidden layers (the view's own and the document's) are left out.
    Alignments count, and so do imagery, point clouds and meshes when the
    caller passes their boxes (`otherContent`).
  - An automatic plan is checked where the painter will draw it:
    `planWindow` applies the painter's rule to the model, and the editor
    passes the painter's own `resolvePlanViewport`. A test compares the two,
    turned, along an alignment, over a mesh alone and with a layer hidden.
  - A key plan is drawn as a plan is, so it is checked as one. Its sheet
    outlines and a plan's match lines count as what it shows. Its own small
    text is not reported: a key plan is a small-scale map of the sheets.
- **Small text.** A text prints `height x 1000 / scale` mm high. A dimension's
  text is its style's height. The finding counts every too-small text in
  the plan's window and gives the smallest. The fix names the largest
  standard scale at which that text reaches the minimum, and the height text
  needs at this scale. Example: at 1:500 a 0.5 m text prints 1.0 mm, so the
  fix is "Plot the view at 1:250 or larger, or make the text at least 0.9 m
  high". `minimumTextMm` changes the 1.8 mm.
- **Sections** list their chainages as the painter does: the stations given,
  else every interval over the range. A cross section is off the alignment
  when either end of its cut is: `pointAtStationOffset` at plus and minus
  its half width. The message names up to three such chainages and the
  alignment's own range.
- **Blank title-block values** are listed once for the set, not once per
  sheet, with how many sheets print them blank: "every sheet", or "1 of 2
  sheets" when one sheet fills the blank itself. A value a sheet sets to
  nothing is blank on purpose and is not counted. Project lines 3 and 4, the
  client and the notes may be blank. Project line 2, the set number, the
  model name and the revision are Info. The rest are Warnings. Only framed
  sheets count, since a frameless sheet prints no title block.
- **Match lines.** Marks leading to removed sheets are counted per viewport
  ("2 match lines and 1 key-plan outline"), and each missing id is named
  once.

`PreflightOptions` changes what is checked:
- `sheets`: check only these sheets. The set's own checks then count only
  these sheets too.
- `minimumTextMm`, `overlapToleranceMm`, `outsideToleranceMm`: the limits
  above.
- `skip`: codes not to report.
- What the model alone cannot know: `resolvePlan` and `otherContent` (above),
  `sectionSurfaces` (how many surfaces sections are cut from), `logoReadable`
  and `assets`. Each one left unset leaves its check out: no surface finding
  when the surfaces are unknown, no missing-image finding when there is no
  assets folder to look in.
- `index`: a spatial index in step with the model, so a plan's window is
  searched rather than the whole drawing.

For a caller with only a document, `checkDocumentSheets(document)` supplies
the document's drawing, spatial index, fields, assets folder and whether its
logo file is there. `findingsOnSheets(findings, indices)` keeps what
concerns some sheets: their own findings and the set's. `summarize` counts
each severity. `summaryText` writes the counts ("2 errors, 1 warning, 3
notes"). `findingLine` writes one finding as a log line:

```
ERROR viewport.outside [s2/vp3] PLAN 1:500 (vp3) runs 14.6 mm under the title block: the title block covers it. Fix: Drag it back inside the drawing area, or tile the sheet.
```

`findingsToJson` writes the findings for an agent. Empty members are left
out. `findingsFromJson` reads them back to the same findings and refuses
another format, another version, an unknown severity, or a sheet index that
is not a whole number of zero or more.

```json
{"format": "katana-sheet-checks", "version": 1,
 "summary": {"errors": 1, "info": 0, "warnings": 1},
 "findings": [{"severity": "warning", "code": "field.empty", "subject": "organisation",
               "message": "Organisation is blank in the title block of every sheet",
               "fix": "Fill it in on Title Block > Project"},
              {"severity": "error", "code": "viewport.outside", "sheet_index": 1, "sheet": "s2",
               "viewport": "vp3", "message": "...", "fix": "..."}]}
```

**In the editor** (`src/katana_qt/plotting/sheet_checks.hpp`):
- `preflightOptionsFor(source)` fills the options from the painter's
  `SheetSource`, and `checkSheetsFor(set, source, sheets)` runs the checks
  with them. The checks and the painter then agree about what prints empty:
  - an automatic plan is resolved by `resolvePlanViewport`;
  - visible imagery, point clouds and meshes are content;
  - only visible surfaces are cut, so a section over a hidden one is
    reported;
  - a logo that did not decode is unreadable.
- The **Checks** dock (`sheetChecksDock`) lists the last run's findings under
  the canvas: severity mark, sheet, view, problem and fix, with the code in
  the tooltip.
  - Errors come first, then warnings, then notes, each in the checker's
    order.
  - Its title and summary line (`sheetChecksSummary`) give the counts.
  - Double-click a row (or press Enter on it) to go to the finding: its
    sheet is made current, its viewport selected, and the fix is shown on
    the status bar (`SheetEditor::showFinding`).
- The toolbar's **Check Sheets** (`sheetCheck`, `SheetEditor::checkSheets`)
  checks every sheet at once and shows the dock.
- After every change to the document, undo and redo included, the checks
  run again once the edits have stopped for 400 ms
  (`SheetChecksDock::schedule`). A drag or a burst of edits costs one run.
  While the editor is closed, a change only marks the findings out of date;
  they are checked once it is shown again.
- **Plot Sheet** and **Plot All** check first. When the sheets being
  plotted have errors, the dock is brought up and the plot's message ends
  "The checks found 1 error on it: the Checks panel lists them". The plot
  goes ahead anyway, with no dialog in the way.
- File > Plot Sheets to PDF and `--plot-sheets` log a summary line and each
  error before plotting (`preflightLog`), and plot.

The tests are `tests/cad/plotting/test_preflight.cpp` (each check with the
smallest set that trips it and the nearest that does not, the order, the
options, the JSON) and `tests/qt_widgets/plotting/test_sheet_checks.cpp`.

## Arranging a sheet

Layout advice and tidying, headless first. `include/katana/cad/plotting/arrange.hpp`
holds pure functions on world points and on `Sheet` and `SheetSet` values.
`include/katana/cad/plotting/arrange_commands.hpp` makes each of them ONE
undoable step on a document, through `editSheet`, `editViewport` or
`editSheetSet`. The sheet editor's Arrange menu calls those steps, and an
agent or a command-line verb calls the same ones.

**Content** is a set of world points; only its convex hull matters.
`drawnOutline(model, layers)` is everything a plan with those layers hidden
draws:
- each entity by its vertices;
- an arc or circle by a polygon whose sides touch it at points 1/32 of a turn
  apart, so the hull holds the whole curve and not only its chords;
- texts and dimensions by their boxes;
- every alignment, chorded to 5 cm.

`viewportContent(model, viewport)` is what a plan or key plan shows: its
stretch of the alignment its source names (sampled every 0.5 m, with every
element change), else the drawing. Given the set, a key plan's content is
the live outlines of the sheets' plans, each automatic plan placed as the
painter places it, and the drawing only when there is no plan to outline;
the outlines it stored when it was made are stale and not counted. A
chainage range wholly off the alignment shows none of it, and then, as the
painter does, the drawing. The
editor adds the window's reference layers and meshes
(`src/katana_qt/plotting/sheet_arrange.hpp`, `drawingContent`).

### The best rotation

Rotations follow `Viewport::rotation`: radians counter-clockwise, the world
direction that runs across the paper. A drawing turned half a turn needs the
same room, so a rotation returned is in (-90, 90] degrees.

`bestFitRotation(content, rectangle)` finds the rotation at which the content
fills a rectangle of that shape at the largest scale, exactly:
- It walks the rotating calipers of the hull. Between two orientations at
  which a hull edge lies along or square to the paper, the same vertices are
  widest across and up the paper. There the needed scale is the larger of two
  concave curves, so its least is at an end of the stretch or where width and
  height ask the same scale. Both kinds are tried.
- Of rotations that tie, the one nearest 0 wins (the positive one of two
  equally near).
- A result within 1 degree of a multiple of 90 is snapped to it, at the scale
  that costs (`kRotationSnapDegrees`).

It returns a `RotationFit`: the rotation, the turned content's width, height
and middle, the exact scale, and the first of `kSheetScales` at or above it.

Examples, in the A3 tiling area (385 x 250 mm):
- A 300 x 100 m rectangle lying along 30 degrees is turned back 30 degrees,
  at 1 : 300000 / 385 = 779.2. Along 120 degrees the answer is -60, not 120.
- The thin triangle (0, 0), (100, 10), (100, -10) fits best where its width
  and height ask the same scale: tan t = 0.5875, t = 30.43 degrees, at
  1 : 237.1 against 1 : 259.7 square.
- A line fits along the rectangle's diagonal.

A drawing turned only to gain a scale no one can print is a drawing with a
north arrow askew for nothing. Two more functions work on the standard
ladder:
- `leastRotationToFit(content, rectangle, scale)` is the rotation nearest
  north-up at which the content fits at 1 : `scale`. It is 0 when that
  already fits. When no rotation fits, the message says what the best needs.
- `bestStandardRotation(content, rectangle)` is the least turn that reaches
  the largest STANDARD scale any rotation reaches. A 450 x 20 m strip along
  45 degrees needs 1 : 2000 square and 1 : 1168.8 along its length; it is
  turned only 3.62 degrees, the least turn that reaches 1 : 1250.

`rotateToBestFit(viewport, content)` turns a plan or key plan by
`bestStandardRotation` inside its rectangle, less the painter's 4% to spare
(`kAutoScaleSpare`). It centres the view on the content, gives it that scale
and turns automatic scale and centring off. The painter's automatic fit
measures the drawing's box, which a turned drawing overfills.

### Paper and scale

`suggestPaper(content, {scale, rotation, frame, shareAcross, shareUp})` is
the smallest ISO sheet whose tiling area holds the content at that scale:
A4 to A0, landscape before portrait at each size. The shares say how much of
the area the content may use, for a main view beside panels. A portrait sheet
has no frame, so its area is the paper less 10 mm, inset 1. When not even A0
holds it, the answer is `NotFound`, and the message names the scale at which
A0 would. Examples:

| Content | Scale | Sheet | Why |
|---|---|---|---|
| 100 x 60 m | 1:500 | A4 landscape | 200 x 120 mm in 271.64 x 176.18 |
| 200 x 100 m | 1:500 | A2 landscape | 400 mm is wider than A3 landscape (385) and A3 portrait (275) |
| 100 x 300 m | 1:1000 | A3 portrait, no frame | 300 mm is higher than A3 landscape (250); 275 x 398 holds it |
| the same, turned 90 degrees | 1:1000 | A3 landscape | |
| 272 x 100 m | 1:1000 | A3 with the frame, A4 without | A4's frame leaves 271.64 mm, its paper less 22 mm 275 |

`suggestScale(content, sheet, rotation)` is the largest standard scale at
which the content fits the sheet's tiling area.

`fitPaperToViewport(sheet, id, content, scale)` is "Choose paper for this
scale":
- The sheet goes on the smallest paper that holds what the view shows at
  that scale, with the painter's 4% to spare, in the share of the tiling area
  the view takes now.
- `applyPaper` then maps every placed view from the old tiling area onto the
  new one, so the layout keeps its proportions.
- The view keeps the scale and is centred on its content. Its automatic scale
  is turned off, so the larger paper does not choose another.

For example, 500 x 300 m at 1 : 1000 filling an A3 sheet is 520 x 312 mm
with room to spare. That needs A2 landscape, 545.27 x 354.36. The same plan
beside a legend has 251.1 of the 385 mm across, so it needs 797 mm of tiling
width and gets A0.

### Arranging the views

`autoArrange(sheet)` removes the overlaps between the unlocked views inside
the tiling area:
- Locked views stay where they are, and the others keep clear of them.
- The main view, the lowest tiling rank (ties in the sheet's order), keeps
  its place and size.
- Every other view that overlaps nothing kept before it, lies inside the
  tiling area less half a gutter and is no larger than the main view stays.
  One that has strayed outside that is brought back first.
- The rest are packed in rank order into the free space, one gutter from
  everything. Each goes to the highest, then the left-most, place it fits,
  the order a sheet is read in.
- An unplaced view is packed at the size of the cell tiling would give it.
  So a plan and a legend with no place yet land exactly in "Main and panel
  right".
- When they do not all fit, they shrink together in 5% steps, never below
  their kind's minimum. Next, every view but the main one is packed again.
  Only then is the main view made smaller, from its top-left corner, in 10%
  steps. A view that gives way is never made larger than the main view.
- A main view with no place yet, or in the way of a locked view, is packed
  first, with the rest, and nothing packed after it is made larger than it.
- A view with no room even at its minimum stays where it was, and the views
  placed after it keep clear of it.

It is deterministic, and a sheet arranged completely does not change when it
is arranged again. What it could not place is reported
(`ArrangeResult::overlapping`, `ArrangeResult::unplaced`) and left where it
was.

The other commands act on a list of ids, so a multiple selection plugs
straight in:

| Function | What it does |
|---|---|
| `alignViewports(sheet, ids, edge)` | lines the views up on the outermost of their `left`, `right`, `top` or `bottom` edges, or on the middle of the box round them (`hcentre`: one vertical line; `vcentre`: one horizontal line). One view alone aligns to the tiling area. A locked view counts where it is and does not move |
| `distributeViewports(sheet, ids, axis)` | equal gaps `horizontal`ly or `vertical`ly: the first and last stay and the others move between them; needs three that can move. When the views between are wider together than the room between the first and the last, equal gaps would lay them over each other: that is refused (`InvalidArgument`, saying both sizes) and nothing moves |
| `matchScale(set, ids, fromId)` | the views, on any sheet, take `fromId`'s scale and lose their automatic scale; a section matched to a section takes its exaggeration too; views with no scale are left out |
| `fitViewportToContent(viewport, content, area)` | the rectangle grows or shrinks about its centre until the content, with 5% round it, just fits at the view's scale (`kFitSpare`), never below the kind's minimum and kept inside the drawing area; the view is centred on its content. A section measures (chainage or offset, level) points, the level exaggerated |

`AlignEdge` and `DistributeAxis` read and write those names (`alignEdgeFrom`,
`distributeAxisFrom`), for a command line.

### One step each, for the editor and for agents

`arrange_commands.hpp` records each of these as one step. An edit that
changes nothing records none, and a refused one changes nothing. When the
caller gives no content, it is gathered from the document's drawing with
`viewportContent`.

| Call | Step |
|---|---|
| `autoArrangeSheet(document, sheetIndex)` | `AUTO_ARRANGE` |
| `alignViewports(document, sheetIndex, ids, edge)` | `ALIGN_VIEWPORTS` |
| `distributeViewports(document, sheetIndex, ids, axis)` | `DISTRIBUTE_VIEWPORTS` |
| `matchScale(document, ids, fromId, fromScale)` | `MATCH_SCALE` |
| `fitViewportToContent(document, id, content, scale)` | `FIT_VIEWPORT_TO_CONTENT` |
| `rotateToBestFit(document, id, content)` | `ROTATE_TO_BEST_FIT` |
| `choosePaperForScale(document, id, content, scale)` | `CHOOSE_PAPER` |

An automatic view is measured at the scale it is drawn at. `drawnScale`
applies the painter's rule to the content: the first standard scale that
holds it with 4% to spare, measured about the content's middle for an
automatic centre and about the view's own centre otherwise. A view along an
alignment is measured by the content's points, any other by the four
corners of the content's box, as the painter measures the drawing's box; so
a turned automatic plan is matched at the scale it prints at. The editor
passes the painter's exact answer instead. `mainPlanOf(set, sheetIndex)` is
the plan "Choose paper" acts on: the sheet's first placed plan, else its
first placed key plan.

### In the editor

The toolbar's **Arrange** button (`sheetArrangeButton`, its menu
`sheetArrangeMenu`) and an **Arrange** submenu at the top of the canvas's
context menu hold:

| Command | Object name | Acts on |
|---|---|---|
| Auto Arrange | `sheetArrangeAuto` | the sheet |
| Align Left, Right, Top, Bottom, Centres Horizontally, Centres Vertically | `sheetAlignLeft`, `sheetAlignRight`, `sheetAlignTop`, `sheetAlignBottom`, `sheetAlignHCentre`, `sheetAlignVCentre` | the selected views (one alone to the tiling area), or every view |
| Distribute Horizontally, Vertically | `sheetDistributeHorizontal`, `sheetDistributeVertical` | the selected views, three or more (fewer is refused, so views not chosen never move), or every view on the sheet |
| Match Scale To... | `sheetMatchScaleMenu`, each item `sheetMatchScale_<id>` (`sheetMatchScaleNone` when there is none) | the selected views, or every scaled view on the sheet, take the chosen view's scale as drawn - an automatic section's fitted scale and exaggeration |
| Fit View to Content | `sheetFitToContent` | the selected plan, or the sheet's main plan |
| Rotate to Best Fit | `sheetRotateToBestFit` | the same |

The Match Scale list is filled when it opens: every view in the set drawn to
a scale, but the selected one, with the scale it is drawn at. The ids come
from `arrangeTargets`, the one place the selection is read: the whole group
chosen on the canvas (`selectedIds`), whichever of it is the primary. Every command
reports what it did, or why it did nothing, in the status bar and the log.

With nothing selected, the sheet's properties have a **Choose paper for this
scale** button (`sheetChoosePaper`). It is disabled on a sheet with no plan.
The Generate dialog's "The drawing, fitted" has **Rotate the drawing to fill
the sheet** (`sheetGenerateRotate`), which calls `smartLayoutRotated`:
- On an automatic scale, the plan is turned when that buys a larger standard
  scale, and left square otherwise.
- At a fixed scale that would need tiles, or a sheet of its own for the 3D
  view and legend, the plan is tried turned on one sheet, beside its panels.
  The tiles stay when it does not fit.

The strip above, generated fitted, is one sheet at 1 : 2000 square and at
1 : 1250 turned 3.62 degrees.

The tests are `tests/cad/plotting/test_arrange.cpp`,
`tests/cad/plotting/test_arrange_commands.cpp` and
`tests/qt_widgets/plotting/test_sheet_arrange.cpp`. The last paints a turned
plan at 4 px a millimetre and finds each corner of the strip inked where the
view's rotation, centre and scale put it, so the rotation means to the
painter what it means to the model.

## The drawing register and the revision table

A set of drawings is handed over with a cover that lists its sheets and the
revisions they have been through. Two viewport kinds draw those lists as
ruled tables:

| Kind (stored as) | Columns | Rows |
|---|---|---|
| `SheetIndex` (`sheet_index`), the drawing register | SHEET No. \| TITLE \| SCALE \| PAPER \| REV | a row per sheet, in the set's order; the sheet it is drawn on shaded light grey |
| `Revisions` (`revisions`), the revision table | REV \| DATE \| DESCRIPTION \| BY | `SheetSet::revisions`, newest at the top - the newest is the last in the list, the order they were issued in, not the latest date or code; with `revisionLimit` N only the newest N (0: every one) |

The heading across the top is the viewport's title, else `DRAWING REGISTER`
or `REVISIONS` (`automaticTitle`). A table writes no title in the corner as
the views do. Both kinds tile (rank 8 and 9, after Notes, so the register
takes the big cell) and need at least 70 x 30 and 50 x 20 mm. Neither is
drawn to scale, so a sheet of tables is `N.T.S.`.

**The rows are the title blocks'.** `drawingRegister(set)` reads each sheet's
fields through `resolveFields`, so the register cannot disagree with the
sheets:
- the number is the one the numbering pattern gives the sheet where it is
  now (`formatSheetNumber`), or the one typed on it. Move a sheet and its row
  moves and is renumbered with it;
- the title is the sheet's name, or its typed `sheet_name`;
- the scale is what its title block reports (`sheetScaleText`). An automatic
  plan scale is decided first, as the painter decides it for the title block
  (`resolvedSheetSet` in `src/katana_qt/plotting/sheet_tables.hpp`), so a
  plan drawn at 1:2000 is not listed at the 1:500 it was stored with. An
  automatic key plan is fitted to the sheets' outlines, as the painter fits
  it, and an automatic section's scale and exaggeration are the fitted ones
  (`resolveSectionViewport`). Preflight lays a register out with the same
  scales (`PreflightOptions::drawnSet`), and `SHEET FIELD n scale` in the
  window prints them;
- the revision is the one typed on the sheet (its `revision` field), else
  the set's latest - the last in `SheetSet::revisions`.

**The layout is headless.** `include/katana/cad/plotting/tables.hpp` lays a
table out: every rule, every text and its anchor in paper millimetres, the
text size, the rows shown and the rows left out. The painter only draws what
it is given, shading first, then rules, then text. The tests check the
layout without pixels. Text is measured by a function the caller passes: the
painter passes its own font's, so what was measured is what is drawn. Without
one, `estimateTextWidth` uses Arial's published advance widths, the same on
every machine.

| Rule | Value |
|---|---|
| Text | 2.5 mm caps, stepping down by 0.1 mm to no less than 1.8 mm, condensed to 0.9 |
| Heading, header | the heading bold at 1.25 times the text; the column headers bold |
| Row | 2 cap heights; each further line of a wrapped description 1.5 more |
| Padding | half a cap height each side of a cell's text |
| Rules | 0.25 mm round the box, under the heading and the header, and between blocks; 0.13 mm between rows and columns, down to the foot of the box, so a short list is a ruled form with room below |
| Columns | each as wide as its widest text; TITLE and DESCRIPTION take what is left, and the description wraps at spaces and starts a new line at each line break |

**Fitting.** The layout takes the first of these that holds every row:
1. the largest text at which the rows fit one block of columns;
2. the register only: the largest at which they fit two blocks side by side,
   each with its own header. A revision table stays one block, so an older
   revision is never level with a newer one;
3. otherwise the smallest text in as many blocks as are wide enough, with as
   many rows as fit and a last line counting the rest, grey and right-aligned
   (`+57 more`, or `+12 earlier` for revisions). The painter reports the
   viewport (`vp3: 57 of 100 rows do not fit; make the view larger`), and the
   editor shows that under its properties.

A size that fits but presses any line below 0.8 of its width is tried only
after every size that does not, so a long title is set a size smaller rather
than squeezed to half its width. A line still too wide is squeezed to its
cell, as the frame's fields are. Example: 100 rows in 200 x 89.2 mm need the
1.8 mm text in two blocks of 22 rows. The last line of the second block is
the count, so 43 rows are shown and `+57 more`.

**The cover sheet.** `registerSheet(set, paper)` makes a sheet named
`DRAWING REGISTER` with the register in the big cell of "Main and panel
right" and the revision table in the panel beside it. It has no utility
legend, which explains symbols a cover does not draw. A set that already
has a register anywhere is refused (`AlreadyExists`, naming the sheet): two
registers would be two lists to keep in step. `addRegisterSheet(document,
paper)` puts it FIRST in the set with new ids (`prepareForAppend`), as ONE
undoable step, and returns its id. The other sheets are numbered one on, and
their marks follow them by id.

**In the editor.**
- Generate Sheets has the layout "Drawing register (cover sheet)"; scale and
  "Replace" do not apply to it.
- Add View has "Drawing register" and "Revision table". Every Add View action
  is named `sheetAddView_` and the kind's stored name, such as
  `sheetAddView_revisions`.
- A revision table's properties have "Newest revisions" (`sheetRevisionLimit`,
  All for 0), one step per value.
- An empty revision table says in the editor, never on paper, that revisions
  are added under Title Block.

**The revision in the title block.** A frame text showing the `revision`
field prints the current revision: typed on the sheet, else the set's
latest. The built-in frame has no such text, and it is left as it was
measured; its sheets show their revisions in a revision table. A frame
written for Katana names the field directly, `REV {revision}`, and
`parseFrame` reads that as a field text.

**Storage.** The kinds are stored by their names above. `revision_limit` is
written only when it is not 0, and a value that is not a whole number of at
least 0 is refused rather than read as some other count.

The tests are `tests/cad/plotting/test_tables.cpp`,
`tests/qt_widgets/plotting/test_sheet_tables_painter.cpp` and
`tests/qt_widgets/plotting/test_sheet_register_editor.cpp`.

## The smart legend

A legend that lists every layer of the drawing tells the reader about things
that are not on the sheet. A set strung along a road then names the whole
survey on every page. So a Legend viewport lists what the sheet's plans
actually SHOW, each entry with a sample drawn exactly as the plan prints it.

The logic is headless, in `include/katana/cad/plotting/legend.hpp`:
`computeLegend(model, set, sheetIndex, scope, options)` is a pure function of
the model, the set and the options. The painter, the editor, a test and an
agent all get the same list from it.

**What is shown.** An entity is listed when all of these hold:
- it is visible, and on a layer the document shows;
- the plan does not hide its layer (`hiddenLayers`, children with their
  parent);
- its SHAPE meets the plan's window on the ground. The window is the
  viewport's rectangle at its scale about its centre, turned by its
  rotation. A turned viewport shows its rotated rectangle, not the box
  around it.

Example: a strip 100 x 10 mm at 1:1000, turned 45 degrees about the origin,
is 100 m along the diagonal and 10 m across it. The point (40, 0) lies inside
the box around the strip, but 28 m from the strip's middle line, so it is not
listed. A line from (30, 0) to (38, 0) is not listed either: its own box lies
inside the strip's box, but the line never meets the strip.

The finer rules:
- A symbol reaches half its size past its point. That is the size its
  style gives it on the ground; a symbol sized on paper (a style with no
  size) is judged by its point alone, so one whose point lies just outside
  the window is not listed although a sliver of it prints at the edge.
- A hatched area that covers the whole window is shown, although none of its
  edges is in the window.
- Dimensions are not listed.
- Key plans are not counted: they show the drawing faded, as a map of the
  sheets.
- An entity that two plans show is counted once.

**Grouped by what prints.**
- An entity drawn in a style of the drawing (a library linestyle or symbol,
  or the style a survey code gave it) is grouped with that style's other
  entities, whatever their layers.
- Any other entity is grouped by its layer. A style name the drawing has no
  style for counts as no style.
- Within a group, each kind of mark is an entry of its own: a symbol, a plain
  point, a line (lines, arcs, polylines and circles), a filled or hatched
  area (a closed polyline) and a text. A point and a line of one layer print
  differently.

**Labels.** An entry's label is the description the survey code library
gives the code its entities carry, when every coded entity of the entry
agrees on it. Otherwise it is the style's or the layer's name. A point's
field code is looked up by its string name, so `SP1 ST` finds the rule
`SP*`. A style's own description is not used: in practice it holds a colour
name or the import's provenance, not words for a reader. The survey code
library is optional: without one, every label is a name.

**Look.** An entry is drawn with the look most of its entities resolve to
(`resolveDisplay`: colour, weight, linetype, symbol and its size, hatch). A
tie goes to the look of the entity with the lowest id.

**Order.** Symbols, then points, lines, areas and texts, each sorted by label
with letter case folded. The same input always gives the same list.

**Scope** (`Viewport::legendScope`, a new member at the end of `Viewport`):

| `LegendScope` | Stored as | Lists what is shown by |
|---|---|---|
| `ThisSheet` (the default) | left out | the plans of the legend's own sheet |
| `WholeSet` | `"legend_scope": "whole_set"` | every plan of the set |
| `WholeDrawing` | `"legend_scope": "whole_drawing"` | the whole drawing, every entity shown in the document |

A sheet with no plan lists what the set's plans show, and a set with no plan
lists the whole drawing. `Legend::scope` says where the fall-back led. An
unplaced plan (one with an empty rectangle) shows nothing. An unknown
`legend_scope` is refused when the set is read.

**Automatic plans.** A plan on automatic scale or centre shows the window the
painter chose. The painter passes its own rule (`resolvePlanViewport`), so
the legend lists what the plan beside it drew. Without it, `fittedPlanWindow`
applies the same rule to the model. It does not count imagery and meshes,
which the model does not hold.

**On the paper** (`src/katana_qt/plotting/legend_painter.hpp`):
- The heading reads LEGEND.
- Each entry has a sample cell of 10 x 3.6 mm and its label in capitals
  beside it, 1.8 mm high.
- `layoutLegend` flows the entries down columns and then across. Each column
  is as wide as its widest label needs, and the rows are shared evenly
  between as few columns as hold every entry.
- A label wider than its column is squeezed to it.
- When the entries do not all fit, the last place says how many are left
  out, in grey. Example: a panel 40 x 60 mm holds one column of ten rows, so
  thirty layers print nine samples and "+21 more".

Each sample goes through the same resolution the plan painter uses, so a
legend cannot show a mark the plan does not print. Colours go through the
paper colour rule (`paperColour`), so white prints black.

| Kind | Sample |
|---|---|
| Line | a library linestyle's own strokes through the shared style painter (`src/katana_qt/customisation/style_painter.hpp`), else a model linetype's dashes, else a plain line, in the entry's weight; a style's symbol at each end, as the plan puts one at every vertex |
| Symbol | the symbol at its plotted size: a size on the ground at the legend's scale, the library definition's own, or the plain mark's for a built-in shape. It is shrunk only when it would not fit its cell |
| Point | the plain point mark, 2 mm across, as on the plan |
| Area | a swatch filled or hatched with the entry's pattern, outlined as a line of the entry is |
| Text | "Abc" in the entry's colour |

A size on the ground (a symbol in metres, a world linestyle, a hatch's
spacing) is drawn at `Legend::scale`. That is the scale of the first plan on
the legend's sheet, else of the first plan the entries came from, else the
legend viewport's own.

**In the editor.** A Legend viewport's properties have a **Lists** choice
(objectName `sheetLegendScope`): what this sheet's plans show, what every
sheet's plans show, or everything in the drawing. A change is one undo step.
Under the choice, a line (`sheetLegendSummary`) says what the legend lists
now and where a fall-back led, for example "3 entries from 2 plans of the
set - this sheet has no plan".

**For an agent.** Every step is a function with parameters:

| Function | Does |
|---|---|
| `legendFor(document, viewportId)` | the legend a Legend viewport lists, at its own scope, with the document's spatial index and survey code library. `NotFound` for no such viewport; `InvalidArgument` for one that is not a legend |
| `setLegendScope(document, viewportId, scope)` | sets the scope as one undoable step (`LEGEND_SCOPE`). An unchanged scope records no step |
| `computeLegend(model, set, sheetIndex, scope, options)` | any sheet at any scope, headless |
| `legendJson(legend)` | the entries as JSON: kind, label, style, layer, code, colour, weight, linetype, symbol and its size, hatch and count, every member written |
| `toString` / `legendScopeFrom` | `this_sheet`, `whole_set`, `whole_drawing` |

The tests are `tests/cad/plotting/test_legend.cpp` (visibility by rectangle
and rotation, hidden layers, grouping, labels, order, scopes, storage, the
flow into columns), `tests/qt_widgets/plotting/test_legend_painter.cpp` (each
kind of sample, a symbol inked in its cell at its plotted size, the panel on
a sheet, "+N more") and `tests/qt_widgets/plotting/test_legend_properties.cpp`
(the scope choice in the editor).

## The coordinate grid and the live key plan

Two things a plan viewport works out for itself. The geometry of both is in
`katana_cad`, with no Qt: `include/katana/cad/plotting/plan_grid.hpp` and
`include/katana/cad/plotting/key_plan.hpp`. The painter only strokes and sets
what they return, so both are tested without pixels
(`tests/cad/plotting/test_plan_grid.cpp`, `test_key_plan.cpp`) and then once
more on paper (`tests/qt_widgets/plotting/test_plan_grid_painter.cpp`,
`test_plan_grid_editor.cpp`).

### The coordinate grid

A plan or key plan viewport has two members for it, at the end of `Viewport`:

| Member | JSON | |
|---|---|---|
| `gridStyle` | `grid_style`: `none`, `ticks`, `crosses`, `lines` | `GridStyle`; `None` by default |
| `gridInterval` | `grid_interval` | metres between lines; 0, the default, is automatic |

Both are written only when they are not the default, like every member, and
an unknown style name fails the parse (`toString`, `gridStyleFrom`).

**The interval.** An automatic grid (`automaticGridInterval`) is spaced at 1,
2 or 5 times a power of ten metres: of those, the one whose lines fall
nearest 50 mm apart on the paper, nearest by ratio. So 1:500 has 20 m (40 mm
apart), 1:1000 has 50 m (50 mm), 1:2000 has 100 m, and 1:750 has 50 m
(67 mm, which is nearer 50 than 27 mm is). Every step of `kSheetScales`
lands between 40 and 67 mm. A grid closer than 2 mm on the paper
(`kGridMinimumSpacingMm`) is refused rather than drawn as a grey smear: the
plan is still drawn, and the painter reports why (`vp1: a 0.5 m grid at
1:1000 is 0.5 mm apart on the paper; ...`).

**The styles.**
- `Lines`: light grid lines right across the viewport, 0.13 mm in grey.
- `Crosses`: a 3 mm cross at every intersection, 0.18 mm in ink. Only a
  whole cross is drawn; one the border would cut reads as a tick that is not
  there.
- `Ticks`: a 2.5 mm tick in from the border where each line meets it,
  0.18 mm in ink, and nothing inside.

**The ground's grid, not the paper's.** The lines are eastings and northings
of the world, so on a rotated viewport they run askew across the paper. Each
line is clipped to the viewport's rectangle, and its ends are on whichever
edges it meets.

**The labels.** One where each line meets an edge: `E 305 200`,
`N 6 250 400`.
- Thousands are grouped with a space (`groupedCoordinate`). Decimals appear
  only when the interval needs them: none for whole metres, one for 0.5 or
  2.5 m, two for 0.25 m (`gridDecimals`). Nothing prints as `-0`.
- 1.8 mm capitals, set along their edge (reading up the sides), 0.8 mm in
  from it, or past the tick for ticks.
- Each is knocked out on white, 0.4 mm all round.
- A label is kept only when it lies wholly inside the viewport, clear of the
  view's title, scale bar and north arrow, and at least 1.5 mm clear of the
  labels already kept (`PlanGridOptions::labelSpacingMm`): two closer than
  that read as one label run on into the next. The painter passes the exact
  boxes those three knock out (`PlanGridOptions::keepOut`), so a label never
  sits over the title or the scale-bar corner, and never half-under either. Labels are taken bottom,
  left, top, right, and along each edge in order, so the same grid always
  keeps the same ones.

`planGrid(viewport, placement, options)` gives all of it on the paper: the
lines, what is stroked (`PlanGrid::strokes`), the crosses, and the labels
with their anchor, angle, justification and knock-out box. The placement is
the viewport's scale and centre once "auto" is decided: `storedPlacement`
for a fixed viewport, or what the painter resolved. `planPaperToWorld`,
`planWorldToPaper` and `planFootprint` are the viewport's mapping both ways,
and the ground under its rectangle.

`setPlanGrid(document, viewportId, style, interval)` sets both members as
ONE undoable step. It refuses a viewport that is not a plan or a key plan
(`InvalidArgument`), a missing one (`NotFound`), and an interval that is
negative or not finite. Setting what the viewport already has records no
step. In the editor a plan's properties have a Grid list (`sheetGridStyle`)
and a Grid interval box (`sheetGridInterval`, which shows "Auto" at 0, and
what that is now in its tooltip). They are a front end to `setPlanGrid`
and make the edit once their signal has returned, since the edit rebuilds
the panel they are on.

### The live key plan

A key plan used to draw the outlines stored in its marks when it was made.
Those were a snapshot: pan a tile, rescale it, add, remove or reorder the
sheets, and the key plan went on showing the old outlines. Now the outlines
are worked out whenever the key plan is drawn.

`keyPlanOutlines(set, sheetIndex, place)` gives one `KeyPlanOutline` for
every placed plan viewport on every sheet. Key plans and sections are not
outlined. Each outline has:
- the ground under the plan's rectangle, from its rectangle, scale, centre
  and rotation (`planFootprint`);
- its sheet's id, index and viewport id;
- `label`, the number the sheet prints now: its typed `sheet_number` if it
  has one, else the set's numbering pattern (`printedSheetNumber`). So the
  numbers follow a reorder at once;
- `current`, set on the key plan's own sheet: "you are here";
- `labelled`, false for a plan whose centre lies inside a larger plan of the
  same sheet (an inset), whose number is written once already.

`place` decides where an automatic plan's ground is. The painter passes
`resolvePlanViewport`, so an outline is exactly where its sheet draws.
`fitPlanPlacement` is the same fit, headless, and a test holds the two
together.

**Without a window.** The painter fits an automatic plan over everything the
window shows, and that includes imagery, point clouds and meshes that only
the application holds. `placePlan(model, viewport)` is the same rule over the
model alone: the viewport's stretch of its alignment, else what the model
draws less the viewport's hidden layers, with the alignments included. It
also has a form that takes the set and the sheet index, which fits an
automatic key plan to every outline. `modelPlacer(model)` is the `place` a
command line hands `keyPlanOutlines`. For a drawing of entities and
alignments they give what the painter gives, and a test holds them to it.
So `planGrid(viewport, placePlan(model, set, sheet, viewport))` is the grid a
sheet plots, worked out without drawing it.

**On paper.**
- The other sheets are outlined in red (0.35 mm) over a faint red fill.
- The key plan's own sheet is drawn over them in blue (0.5 mm), with a
  stronger blue fill.
- The numbers go on last, so no fill covers one. Each is sized to its
  outline (1.2 to 3 mm capitals), so a small sheet's number stays inside it.

A key plan's stored `SheetOutline` marks are no longer drawn, since they are
the stale copies. Its other marks, match lines, still are. The editor's Add
View > Key Plan stores none, and neither does `VIEW ADD keyplan`. The tile
and strip generators still store them, and they still reserve their sheets'
ids (`newSheetIds`). Preflight and arrange ignore them too: preflight checks
an automatic key plan at the window it is fitted to, and counts its live
outlines as what it shows; arrange fits it to them.

**An automatic key plan** (`autoScale` or `autoCentre`) fits the union of
the outlines (`fitKeyPlan`), and the drawing only when the set has no plan to
outline. Before, it fitted the drawing alone, so a sheet over ground outside
the drawing's extent fell off it, and a stray entity far away shrank every
sheet to a speck. The painter and
the editor resolve it through the four-argument `resolvePlanViewport(viewport,
source, set, sheetIndex)`. For every other viewport that gives the same as
the two-argument form.

Example: a key-plan sheet and two plans 100 x 50 mm at 1:1000. Sheet 2 is over
E 950..1050, sheet 3 over E 1050..1150, both over N 1975..2025. A key plan
200 x 150 mm at 1:2000 over (1050, 2000) outlines sheet 2 at x 150..200 and
sheet 3 at x 200..250, y 162.5..187.5 on the paper, numbered 2 and 3. Pan sheet
3's plan 100 m north and its outline is 50 mm higher at the next paint. Move
sheet 3 ahead of sheet 2 and the same outline reads 2. Put a key plan on
sheet 2 and sheet 2's own outline is the blue one.

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

## Plot styles and output

A sheet set is plotted in a PLOT STYLE and to one of several OUTPUTS, all
chosen by plain data, so the Plot dialog, `katana --plot-sheets` and an agent
make the same call and get the same files.

**The colour mode** is `cad::PlotColourMode` in
`include/katana/cad/plot.hpp`, carried by `cad::PlotSettings::colourMode`
with `PlotSettings::lineWeightScale` beside it. The rule lives in the model,
in `cad::paperColour` (a pen or a letter) and `cad::paperFillColour` (an
area), so the plan painter, the sheet painter and the frame agree:

- **Colour** prints as drawn (white still prints black under
  `whiteToBlack`).
- **Greyscale** prints every colour as the grey of its `cad::luminance`, the
  Rec. 601 luma in integers, so a colour keeps its lightness.
- **Monochrome** prints every pen and letter black. A FILL is black when its
  luminance is below `cad::kMonochromeFillThreshold` (128) and paper white
  otherwise, as a one-ink plotter prints it: dark areas stay solid, light
  tints drop out and leave their (black) outline. The paper's own white -
  the page, a knock-out behind a label, the key plan's fade - is never a
  colour of the drawing and stays white in every mode; a fill's alpha is
  kept, so a faint tint stays faint.

In the sheet painter the mode is applied in ONE place: `paperPen`,
`dashedPen`, `TextSetter` and the `paperFill` helper call `plotInk` and
`plotFill` (`src/katana_qt/plotting/plot_style.hpp`), and every image placed
on a sheet - the logo, an image viewport, the 3D snapshot - passes through
`plotImage`, which greys it (a photograph is printed grey, not thresholded,
in monochrome). A plan viewport follows because the plan painter takes the
same `PlotSettings`. **The line weight scale** multiplies every pen width on
paper (0.1 to 5): a check plot at 0.7, a bold one at 1.4; text, dash lengths
and symbol sizes are unchanged.

**The page setup** is `plotting::PageSetup`
(`include/katana/cad/plotting/page_setup.hpp`), kept at the end of
`SheetSet` and in its JSON as `page_setup` (each member only when it is not
the default): the colour mode (by name), line weight scale, resolution (50 to
1200 dpi, 300 by default), the file-name pattern and whether a PDF plot is a
file per sheet. `plotting::setPageSetup` changes it as one undoable step
("PAGE_SETUP"). The pattern takes `formatSheetNumber`'s `{n}`, `{n:02}`,
`{N}` and `{set}`, plus `{name}`, `{number}` and `{id}`; the default
`{set}{n:02} {name}` names the third sheet of set C "C03 PLAN TILE 3".
`plotting::sheetFileNames` expands it, sanitises the result for every file
system (`plotting::sanitiseFileName`: forbidden characters to '-', device
names prefixed, 120 bytes at most) and tells repeats apart with " (2)".

**A selection** is text: `plotting::parseSheetSelection` reads "1,3-5"
(positions from 1, in the order given, each once), sheet ids ("s7") and
""/"all", and refuses a sheet that is not there or a backward range saying
which; `plotting::formatSheetSelection` writes the short form back.

**The outputs** are `src/katana_qt/plotting/plot_output.hpp`: a
`PlotRequest` (sheets, `PlotFormat`, style, dpi, destination, pattern, title)
given to `plotSheets` writes

- `pdf`: one vector PDF, a page a sheet, each page the size of its paper;
- `pdfs`: a PDF a sheet in the destination folder, named by the pattern;
- `png`, `tiff`: a raster a sheet at the dpi, with the resolution recorded
  in the file; one grey channel when the mode is not Colour. TIFF is written
  by `src/katana_qt/plotting/tiff_writer.hpp` (baseline, Deflate), because
  Qt's TIFF plugin is not everywhere;

and `printSheets` prints on a `QPrinter`, a page a sheet on the sheet's
paper, scaled down to fit (and reported) when the printer's paper is
smaller. `validatePlotRequest` and `plannedFiles` check a request and list
its files without writing anything; the `PlotReport` lists the files
written, the sheets, every problem and whether it was cancelled. Every file
goes through a `QSaveFile`, so a cancelled or failed single PDF leaves no
half a set and an earlier file as it was.

**Progress.** `plotSheets` calls a `PlotProgress` before each sheet;
returning false cancels before the next one. The Plot dialog
(`src/katana_qt/plotting/plot_dialog.hpp`, `PlotDialog`) only collects a
request; `plotWithProgress` then runs it on the GUI thread behind an
application-modal progress dialog (`plotProgress`) with Cancel, so the
sheets cannot be edited while they are painted, and the plot paints from a
copy of the set. The editor's Plot Sheet and Plot All and File > Plot
Sheets to PDF open the dialog; its "Keep these settings" box stores the
choice as the page setup. On the command line, `--plot-sheets` takes
`--sheets`, `--format pdf|pdfs|png|tiff`, `--plot-style colour|grey|mono`,
`--dpi` and `--line-weight-scale` (`docs/headless.md`).

The tests are `tests/cad/plotting/test_page_setup.cpp`,
`tests/qt_widgets/plotting/test_plot_style.cpp`, `test_plot_output.cpp` and
`test_plot_dialog.cpp`.

## Editing on the canvas

What makes the sheet editor a layout tool: several viewports picked and
moved at once, a clipboard, a paper grid, rulers, the world under the cursor,
and a list of sheets that shows each one. What an edit does is decided in
`include/katana/cad/plotting/viewport_edits.hpp`, with no Qt, so an agent
makes the same edits without the canvas and a test checks each without a
mouse. The editor's pieces are in `src/katana_qt/plotting/`: the rulers and
grid (`sheet_rulers.hpp`), the cursor readout (`sheet_readout.hpp`), the
pictures of the sheets (`sheet_thumbnails.hpp`), the list
(`sheet_list_widget.hpp`), the clipboard (`viewport_clipboard.hpp`), and the
Edit and View menus (`sheet_editor_editing.cpp`).

Nothing new is stored. The selection, the grid and the rulers belong to the
editor; each edit stores the set through `Document::setSheetSet`, as every
edit in `sheet_commands.hpp` does, as ONE undoable step.

### Several viewports at once

- **Picking.** A click selects one viewport. Ctrl-click or Shift-click adds
  one or takes it out. (Shift-drag on a plan still pans its drawing; a
  Shift-click on it without a drag adds it or takes it out.) The last one
  picked is the
  **primary**: it has the handles and the properties. `SheetCanvas::selected`
  returns it, and `SheetCanvas::selectedIds` returns all of them in the
  sheet's order, for anything that acts on several.
- **The rubber band.** A drag on empty paper draws a band. Dragged left to
  right it is a window and takes what it encloses (solid, blue); right to
  left it is a crossing and takes what it touches (dashed, green), as CAD
  programs pick (`viewportsInBand`). With Ctrl or Shift it adds to the
  selection. The paper is panned with the middle button, or with Space held
  and the left.
- **A group drag.** Dragging any selected viewport moves them all. Their
  bounds snap, and each moves by the same distance: ONE step
  (`moveViewports`). A locked viewport stays where it is. A click on one of
  a group without a drag, locked or not, selects it alone, and Escape
  abandons a drag - a rubber band too, the selection left as it was.
- **The keys.** The arrows move every selected viewport by 1 mm (Shift:
  10 mm), one step a press. Delete or Backspace removes them all, locked or
  not, in one step (`removeViewports`). Tab and Shift+Tab step the selection
  through the placed viewports in the sheet's order and wrap round
  (`cycleViewport`); on a sheet with none, Tab moves the focus on.

### Copy, paste and duplicate

Ctrl+C puts copies of the selected viewports on the system clipboard as the
sheet set's own JSON, one sheet holding them, under the MIME type
`application/x-katana-viewports`. So they paste onto another sheet, into
another project, or into another Katana running beside this one. A copy
records nothing.

Ctrl+V pastes them onto the current sheet as one step (`pasteViewports`),
with new ids (`newViewportIds`) in their order, and selects them. Where they
land is `pasteOffset`: the first of 0, 5, 10 ... mm right and down at which
no pasted rectangle lies exactly on a rectangle already on the sheet. So a
paste onto another sheet lands where the copies were, a paste onto their own
sheet lands 5 mm from them, and the next paste 5 mm further. Ctrl+X copies and
removes (one step); after it, a paste lands where the copies were. Ctrl+D
duplicates, copying and pasting on the same sheet as one step, without
touching the clipboard (`duplicateViewports`).

Example: `vp1` at 100..200 x 100..180 on the first sheet, in a set whose
highest id is `vp4`. Copied and pasted onto the second sheet, it becomes
`vp5` at 100..200 x 100..180. Pasted there again, it is `vp6` at
105..205 x 95..175.

### The paper grid

View > Snap to Grid (F9) turns on a 5 mm grid (`kPaperGridMm`) through the
paper's bottom-left corner, the corner the rulers count from. It is drawn
faintly, every tenth line a little stronger, and zoomed out only every
second, fifth or tenth line is drawn, so the lines are never closer than
4 pixels.

A drag snaps as before first: an edge or centre line of the moving
rectangle to one of the drawing area or another viewport within 8 pixels,
with a guide drawn. Then, on an axis where nothing was in reach, it takes
the smaller move that puts its left or right edge (its bottom or top edge)
on a grid line (`snapMovingRect`). A panel next to another lines up with it
before it lines up with the grid. A handle's edge snaps the same way
(`snapEdge`). Alt turns all snapping off. Example: a panel 103 mm wide
dragged to 107.3..210.3 lands at 107..210, because its right edge is 0.3 mm
from a line and its left edge 2.3 mm.

### Rulers and the cursor

View > Rulers (Ctrl+R) shows rulers along the canvas's top and left. They
are in paper millimetres from the paper's bottom-left corner, Y up, and
follow the zoom and the pan. The numbers are at the smallest of 1, 2, 5, 10,
20, 50 ... mm that stands 50 pixels apart (`rulerSteps`). At 2 pixels a
millimetre they are every 50 mm, with ticks every 5. On the rulers the
paper's span is white, the selection's is shaded, and the cursor is marked
in red on both.

The right end of the status bar reads the cursor. It gives the paper
position, the viewport under the cursor with its kind and scale, and, over
a plan or key plan, the world point drawn there. That point is the
painter's mapping turned round (`planPaperToWorld`), at the scale and centre
the plan is drawn at, with "auto" decided and kept until the next change.
Example: a plan at 1:500 in 100..300 x 100..250, centred on (1000, 2000) and
turned 30 degrees, reads
`X 222.3  Y 173.7 mm   vp1 Plan 1:500   E 1010.000  N 2005.000`. An agent asks
`SheetCanvas::readoutAt` for the same values.

### What a drag shows

While viewports are dragged, each one's own paint goes with its rectangle,
taken from the last paint of the sheet, and where it was is faded. A
resized viewport keeps what it showed centred. A plate under it gives its
position and size. Nothing is painted afresh, and nothing is recorded, until
the button is let go.

Double-clicking a viewport selects it and fills the window with it. View >
Zoom to Selection (Shift+Home) fills the window with the selection. A
double-click on empty paper or the desk, or Home, fits the page.

### The list of sheets

Each sheet in the list has a picture of itself (`SheetThumbnails`), painted
by `paintSheet`, the function that plots it, at 112 x 80 pixels. A picture
is kept until something it shows changes: the sheet, its position, the order
of the sheets (its number and the numbers its marks print, and the sheet
numbers the sheets its marks lead to override), the title-block
values the sheets share, the project's field values, and, only for a sheet
that shows the drawing (a plan, section, 3D snapshot, legend or key plan),
the drawing's revision. So an edit to one sheet of a hundred paints one
picture, and a line drawn in the model does not paint a sheet of notes.
Stale pictures are painted one at a time while the editor is idle and shown,
the old picture standing in until then.

A sheet dragged up or down the list is moved with `moveSheet` as one step,
and the list is built again from the document, so it never shows an order
the set does not have (`SheetListWidget::dropAt`); a drop in the spacing
between two rows goes between them. PgUp and PgDn show the previous and the
next sheet while the canvas or the list has the focus, and are left to a
spin box of the properties, which pages with them.

### The actions

Each is a QAction of the editor window with an object name, and each calls
a public function that an agent calls directly:

| Action | Key | Calls |
|---|---|---|
| `sheetCut` | Ctrl+X | `SheetEditor::cutSelection` |
| `sheetCopy` | Ctrl+C | `SheetEditor::copySelection` |
| `sheetPaste` | Ctrl+V | `SheetEditor::paste` |
| `sheetDuplicateViews` | Ctrl+D | `SheetEditor::duplicateSelection` |
| `sheetDeleteViews` | Delete | `SheetEditor::removeSelectedViewport` |
| `sheetSelectAll` | Ctrl+A | `SheetCanvas::selectAll` |
| `sheetSelectNextView`, `sheetSelectPreviousView` | Tab, Shift+Tab on the canvas | `SheetCanvas::cycleSelection` |
| `sheetZoomSelection` | Shift+Home | `SheetCanvas::zoomToSelection` |
| `sheetFitPage` | Home | `SheetCanvas::fitPage` |
| `sheetSnapGrid` | F9 | `SheetCanvas::setSnapToGrid` |
| `sheetShowRulers` | Ctrl+R | `SheetCanvas::setRulersShown` |
| `sheetPreviousSheet`, `sheetNextSheet` | PgUp, PgDn on the canvas or the list | `SheetEditor::setCurrentSheet` |

The arrows call `SheetEditor::nudgeSelection`, and a drop in the list
`SheetEditor::moveSheetTo`. Cut, Copy, Duplicate, Delete and Zoom to
Selection are enabled only while something is selected, and Paste only while
the clipboard holds viewports. A disabled action leaves its key to the text
box that has the focus.

The toolbar's and the context menu's actions have names too: `sheetUndo`,
`sheetRedo`, `sheetGenerate`, `sheetNewSheet`, `sheetTitleBlock`,
`sheetPlotSheet`, `sheetPlotAll`, `sheetAddView_<kind>` and `sheetTile_<n>`
(the menus' `sheetMenuAddView_<kind>` and `sheetMenuTile_<n>`), and
`sheetBringToFront`, `sheetSendToBack` and `sheetFillDrawingArea`.

The mouse, all in one place:

| On the canvas | Does |
|---|---|
| click a viewport | selects it alone (one of a group: that one alone, on release) |
| Ctrl-click or Shift-click a viewport | adds it to the selection or takes it out |
| drag a selected viewport | moves the whole selection, snapped; ONE step on release |
| drag a handle (one viewport selected) | resizes it, snapped; ONE step |
| Shift-drag inside a plan or key plan | pans its drawing (ONE step); without a drag it is the Shift-click above |
| drag on empty paper | a rubber band: left to right a window, right to left a crossing; Ctrl or Shift adds |
| middle drag, or Space held and a left drag | pans the paper |
| Alt while dragging | no snapping, to edges or the grid |
| double-click a viewport / empty paper | zooms to it / fits the page |
| wheel | zooms about the cursor |
| Escape | abandons a drag, else clears the selection |

The edits themselves, in `viewport_edits.hpp`, take the sheet by its
position and the viewports by id:

| Function | |
|---|---|
| `moveViewports(document, sheet, ids, delta)` | the unlocked, placed ones moved by `delta` mm |
| `removeViewports(document, sheet, ids)` | all of them, locked or not |
| `copyViewports(set, sheet, ids)` | copies in the sheet's order, ids and all: a clipboard |
| `pasteViewports(document, sheet, viewports, offset)` | onto the front of the sheet with new ids, moved by `offset` (`pasteOffset` when not given); returns the new ids |
| `duplicateViewports(document, sheet, ids)` | copies pasted on the same sheet; returns their ids |
| `viewportsInBand`, `viewportBounds`, `cycleViewport` | picking, the selection's bounds, Tab's order |
| `snapToGrid`, `snapMovingRect`, `snapEdge` | the grid and a drag's snapping |
| `planPaperToWorld`, `planWorldToPaper` | a plan's paper and its world |

An id that is not on the sheet is refused with `NotFound`, naming it; an
empty list is refused with `InvalidArgument`; a repeated id counts once. A
refused edit changes nothing, and an edit that would change nothing adds no
step.

The tests are `tests/cad/plotting/test_viewport_edits.cpp` and
`tests/qt_widgets/plotting/test_sheet_editor_editing.cpp`.

## Not yet

- **Change notifications.** A sheet edit notifies the document's listeners
  like any model change, so it still rebuilds the 3D view's scene.
- **Frames and furniture.** More frames, such as a plan-sheet frame with a
  north-arrow zone, and a reader for title-block files.
- **Background plotting.** A large set is plotted on the GUI thread with a
  wait cursor; the painter is reentrant, so moving it to a job is the next
  step.

  north-arrow zone, and a reader for title-block files. The revision table
  is stored and edited but not yet drawn by any frame.
- **Background plotting.** A large set is plotted on the GUI thread, a
  sheet at a time behind a progress dialog; the painter is reentrant, so
  moving it to a job is the next step.
