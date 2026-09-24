# DXF - native import and export

`katana_dxf` (`include/katana/dxf/`, `src/katana_dxf/`) reads and writes DXF
drawings itself, with no third-party library. Both front ends send every
`.dxf` to it: File > Import, File > Export Vector, a path given to
`katana.exe`, and the `IMPORT` and `EXPORT` verbs of the window's command line
and of `katana_cli`. GDAL still reads and writes the GIS formats; it no longer
sees a DXF.

## Why

DXF used to go through GDAL's vector driver, which treats a drawing as GIS
features. Measured on 2026-09-23 (the feature and performance maps):

- an arc of radius 1000 and 30 degrees came in as a 9-vertex polyline
  (length 523.505 against the true 523.599);
- a circle came in as an OPEN 91-vertex polyline;
- text came in as a point;
- every closed polyline and circle went OUT as a solid-filled HATCH, so a
  parcel arrived in the client's CAD program as a filled black shape;
- text and dimensions were skipped on export;
- exporting a real 28 000 entity survey drawing took 116 s, about 2 ms a
  feature inside the driver.

## Where it sits

```
core < math < geometry < entity < commands
                                     |
                                    dxf        (like archive12d: no GDAL)
                                     |
                           app (katana_cli), qt (katana)
```

`dxf` may see core, math, geometry, entity and commands
(`tools/check_layering.cmake`). It needs no third-party library, so it builds
with `-DKATANA_BUILD_IO=OFF` and a sanitizer build runs its parser of untrusted
text. `commands` is there for one thing: `dxf::importCommand`, the import as
the single undoable step both front ends execute, so they cannot differ about
it.

The public interface:

| Header | What |
| --- | --- |
| `dxf/reader.hpp` | `readDxf(bytes)`, `readDxfFile(path)` -> `DxfImport`: entities, layers, linetypes, bounds, the release, a tally per entity kind, warnings. `isDxfPath`. |
| `dxf/writer.hpp` | `writeDxf(model)`, `writeDxfFile(model, path)` -> `DxfExport`: the text (or the file), counts, warnings. `layerNameFor`. |
| `dxf/import_command.hpp` | `importCommand(import, model)`: linetypes, layers, entities and layer locks as one transaction. A layer the model already has locked (or whose parent is locked) is opened for the new entities and locked again in the same step, so a file imported twice, or two sheets that share a locked layer, both come in. |
| `dxf/codes.hpp` | The indexed colours, lineweights, and the text codes (`%%d`, `\U+XXXX`, MTEXT formatting), exposed so tests check them against hand-worked values. |

## Reading

Any ASCII DXF from R12 to R2018 (`$ACADVER` AC1009 to AC1032). Group codes are
read as pairs straight out of the file - a value is a view into it, a number is
parsed with `std::from_chars` only when an entity needs it - and every entity
of one import goes into the drawing in one `createEntities` transaction.

| DXF | Becomes |
| --- | --- |
| `LINE` | a Line. One of zero length is a Point: some programs draw a surveyed dot that way. |
| `ARC`, `CIRCLE` | an Arc, a Circle - true curves. An arc whose start and end angle coincide is the full circle it draws. |
| `LWPOLYLINE` | a Polyline, closed flag kept. |
| `POLYLINE` 2D and 3D | a Polyline, closed flag kept; spline frame points dropped, fitted vertices kept. |
| `POINT` | a Point. |
| `TEXT` | a Text: string, height, rotation. |
| `MTEXT` | a Text for each paragraph, formatting codes stripped. |
| `INSERT`, `MINSERT` | the block's entities, expanded, nested blocks too. |
| `ATTRIB` | a property of every entity its insert produced (tag -> value, as text); a visible one is also a Text where the file put it. |
| `DIMENSION` | its picture block, expanded: the lines, arrowheads and text the file shows. |
| `ELLIPSE` | a Polyline, chorded. |
| `ATTDEF`, `VIEWPORT` | nothing: a template the ATTRIB replaced, and paper space furniture. |
| `LAYER` table | layers: colour (true colour where given, else the indexed colour), on/off and frozen as visibility, lock, linetype, lineweight. |
| `LTYPE` table | linetypes, where Katana can draw the pattern. |

What is kept, and how:

- **Arcs inside polylines.** A Katana polyline has no arc segments yet, so a
  bulge is replaced by chords that stand off the true arc by at most
  `ImportOptions::curveTolerance`, 1 mm by default. The vertices of the file
  are kept exactly; only points between them are added. Every arc chorded is
  counted and the import says so ("N polyline arc segments chorded to within
  0.001").
- **Heights.** Through `entity::setHeights`, the one writer of the
  `elevation` / `elevations` properties the surface builder reads. A 3D
  polyline keeps every vertex's Z, zero included - zero is data there. A 2D
  polyline's elevation, a point's Z, a line's two Z values, a circle's, arc's,
  ellipse's and text's elevation are kept when they are not zero, since zero
  is what a plan drawing writes for "no height". A level of exactly 0 that
  this module's writer said beside the entity (see Writing) is kept as 0.
- **Blocks.** An insert's position, scale (per axis), rotation and array; a
  block's own base point. An entity on layer 0 inside a block takes the
  insert's layer, and a ByBlock colour the insert's colour, as the format
  defines. Each block is converted once and placed per insert, so a symbol
  inserted 14 000 times costs one conversion. The block a result came from is
  kept as `dxf.block` metadata ("SITE/TREE" for one nested in another). A
  block of attributes only, which draws nothing, leaves a Point carrying its
  attributes, so the data is not lost.
- **Heights through an insert.** A block entity with heights has them scaled
  by the insert's Z scale and raised to its level. One drawn at the block's
  Z 0 - the usual survey symbol, drawn at 0 and inserted at the point's
  level - takes the insert's level (block Z 0, scaled, plus the insert's Z);
  only where that comes to exactly 0 at the top does it stay in plan. An
  insert in a tilted plane gives no heights, and says so.
- **Scale that is not uniform.** A circle or arc in a block inserted at
  different X and Y scales is an ellipse; it is chorded to the same tolerance.
- **Object coordinate systems.** Entities whose extrusion direction is -Z
  (drawn mirrored) are mirrored into plan exactly, arcs reversed. Their
  elevation runs along the extrusion too, so an elevation of -10 facing down
  is a height of +10 - for CIRCLE, ARC, LWPOLYLINE, a 2D POLYLINE, TEXT and an
  INSERT's level alike. MTEXT is not an object coordinate system entity: its
  point and its direction are world coordinates whichever way it faces, and
  only an angle given without a direction is measured in its plane. An entity
  in a tilted plane is projected onto the plan, a circle or arc as chords, and
  counted.
- **Colours.** Group 420 (true colour) wins over group 62 (indexed); ByLayer
  is left ByLayer. Indexed colours 1-9 are the named colours, 250-255 the grey
  ramp, 10-249 computed from their hue, brightness and saturation (within a
  unit or two of the published table, which no screen shows).
- **Linetypes.** A pattern that starts with a gap, or holds two dashes
  together, is turned into Katana's form - dashes and gaps alternating from a
  dash - by merging runs and rotating the cycle, which draws the same line. A
  pattern of dashes only is continuous. Embedded shapes and text in a pattern
  keep their spacing and lose their glyph. A layer that names a linetype the
  table does not have is drawn continuous, and said.
- **An entity's own linetype and lineweight.** Katana has no per-entity field
  for either yet: they are kept as the `dxf.linetype` and `dxf.lineweight`
  metadata, and the writer puts them back.
- **Text.** `%%d`, `%%p`, `%%c` become the degree, plus-minus and diameter
  signs, `%%%` a percent sign, `\U+XXXX` its character. A TEXT justified by its
  second point is placed by the first, which the writing program computes as
  the left of the baseline; where it did not (the first point is missing or at
  the origin), the left end is estimated from the second without a font. MTEXT
  paragraphs are placed one five-thirds-of-a-height pitch apart from the
  attachment point. Width factor, obliquing and the font are not kept.
- **Layer names.** A DXF layer name that is already a valid Katana layer path
  is kept - a "/" in it nests, which brings back the tree of a file an older
  export wrote. Otherwise it is mended level by level (control characters to
  "_", blanks trimmed). Names are matched without regard to letter case, as
  the format compares them. A file this module wrote carries each layer's full
  path as extended data (see Writing), and that path is used.
- **Encoding.** UTF-8 is read as it is (R2007 and later write it; ASCII is
  UTF-8). Anything else is read as Windows-1252 and said, with the file's
  declared code page named when it is another.

What is not read, and is said - every kind the file held is in the tally
("HATCH: 1 read, 0 imported") and in a warning:

- `HATCH`, `SOLID`, `TRACE`, `3DFACE`, `SPLINE`, `LEADER`, `MLEADER`, `MLINE`,
  `IMAGE`, `WIPEOUT`, `REGION`, `3DSOLID`, tables and other objects;
- polyface and polygon meshes (`POLYLINE` flags 16 and 64);
- paper space: an entity with group 67 set, and layouts - a Katana drawing is
  the model;
- external references: the insert is reported, the other drawing not read;
- binary DXF (refused by name: save as ASCII DXF) and DWG.

### When the file is damaged

`readDxf` fails only when the text is not a DXF at all (no `SECTION`), is a
binary DXF, or a group code line is not a number - past that point the pairs
cannot be told apart, and the error names the line. Everything else is a
warning, and costs only the entity it is in: a coordinate that is not a
number, an insert of a block the file does not define, a file that ends
without `EOF` (what was read is imported, and the truncation said). Every
entity is checked with `entity::validate` before it is kept. A block that
inserts itself, nesting deeper than 16 blocks, an array of more than 100 000
copies and an import of more than 20 million entities are each stopped and
said. `tests/dxf/test_robustness.cpp` reads 3 000 damaged copies of the two
fixtures - bytes changed, lines cut out and repeated, the file cut short - and
checks that none crashes and that every success holds only valid geometry.

## Writing

R2000 (`AC1015`), ASCII, with the handles, owners and subclass markers that
release requires: the nine tables, `*Model_Space` and `*Paper_Space` blocks
and records, and the root dictionary, groups, layouts, plot style and
multiline style objects. An independent DXF library audits the files with no
errors and no fixes.

| Katana | Written as |
| --- | --- |
| Point | `POINT`, its elevation as Z. |
| Line | `LINE`, its heights as the two Z values - or, with a height at one end only, in plan (see Heights below). |
| Arc, Circle | `ARC`, `CIRCLE` - true curves, never chords. A clockwise arc is written counter-clockwise from its other end, the only direction the format has, its end heights in that order too. An arc with the same height at both ends has it as Z; one that climbs, as an imported survey arc on a grade does, is written in plan (see Heights below). |
| Polyline | `LWPOLYLINE` with the closed flag - **never a HATCH**. One height for every vertex is its elevation. Heights that differ make a 3D `POLYLINE` instead, since an `LWPOLYLINE` has one elevation. Heights at only some vertices: in plan (see Heights below). |
| Text | `TEXT`: the left of the baseline, height, rotation. A Text holding line breaks is a `TEXT` a line, a five-thirds pitch apart. |
| Dimension | exploded: two extension lines, the dimension line, a tick at each end, and the measurement as `TEXT` centred over it, by the layer's dimension style. |
| Layer | a `LAYER` record: the nearest indexed colour, visibility (a negative colour), lock, linetype, the nearest standard lineweight. Every layer of the drawing, used or not. |
| Linetype | an `LTYPE` record with its pattern, beside `ByBlock`, `ByLayer` and `Continuous`. |

- **Extended data.** What the format's own groups cannot hold goes beside the
  entity or layer as extended data under the registered application `KATANA`,
  each value after its word: `path`, `elevations`, `colour`. Other programs
  pass it by; this reader takes it back, so that a Katana drawing written and
  read again is the drawing it was.
- **Layer names.** A DXF layer name may not contain "/" (nor `< > \ " : ; ? *
  | = \``), so a path is written with "$" for "/" - "survey/kerb" becomes
  "survey$kerb", the separator bound references use - and the others as "_".
  Names that then collide without regard to case get "~2", "~3". The full path
  goes beside the name (`path`), and this reader restores the tree from it.
- **Heights.** A height at every vertex is written as Z. Heights known at only
  some vertices are not written as Z at all: the format has no "no height",
  and a vertex written at Z 0 would be a false level - a surface built from the
  file would dive to the datum there. Such an entity is written in plan, its
  heights beside it (`elevations`, the property's own text, nulls included, in
  pieces of at most 250 characters since R2000 holds an extended data string
  to 255), and the export says how many. An arc whose two ends differ in
  height is written the same way, since `ARC` has one Z. A level of exactly
  0 at every vertex is written as Z 0 and said beside the entity as well
  (the one height "0"), since Z 0 alone reads as "no height"; nothing is lost
  to another program there, so it is not counted in the warning.
- **Colours** are indexed: R2000 has no true colour. The nearest of the 255 is
  written, and every other program draws that; black, which no index is, is
  written as 7, the drawing's foreground. A colour no index is exactly goes
  beside it as well (`colour`, "#RRGGBB"), and comes back exact.
- **Angles** are degrees. A text's rotation is written as the degrees that read
  back as the same radians where any do; where none do (not every double is a
  multiple of pi/180 rounded), it comes back within a unit in the last place.
- **Strings** are ASCII: another character is `\U+XXXX`, the degree,
  plus-minus and diameter signs `%%d %%p %%c`, a percent sign that another
  follows `%%%`. A character beyond the basic multilingual plane has no R2000
  escape and is written "?".
- **Numbers** are the shortest decimal that reads back as the same double, so
  coordinates survive the round trip exactly.
- `$INSUNITS` is metres.
- **A file is replaced whole**: written beside the target and renamed over
  it, so a failed write leaves the previous file.

Not written: entity properties and survey attributes (DXF has nowhere for
them that other programs read), alignments, surfaces and meshes, styles and
symbols (an entity is written with its own colour and its layer's linetype),
hatch patterns, and blocks - what Katana draws, it writes as the entities
drawn.

## Measured

Measured on 2026-09-24, on a laptop shared with other work and on battery:
the claim is the ratios and the counts, and the seconds are what they were
that afternoon.

### The committed benchmark

`benchmarks/bench_dxf.cpp` (Release) writes and reads a generated survey
drawing - half points with heights, a quarter 3D strings of six vertices,
labels, arcs, circles and lines on twenty layers - through a file, with the
native module and with GDAL's driver. Run with
`tools/compare_benchmarks.py --alternate 3 Dxf A=<exe> B=<copy of exe>`: the
same binary twice, so that the gap between A and B is the noise the ratios
are read against.

| ms, min / median (9 samples each) | A | B |
| --- | ---: | ---: |
| BM_DxfWriteGdal/1000 | 921.98 / 1134.23 | 814.66 / 1016.33 |
| BM_DxfWriteNative/1000 | 4.91 / 5.86 | 4.96 / 5.79 |
| BM_DxfWriteNative/28000 | 56.36 / 70.37 | 61.80 / 70.95 |
| BM_DxfReadGdal/1000 | 32.34 / 38.80 | 27.36 / 33.41 |
| BM_DxfReadNative/1000 | 3.87 / 4.26 | 3.65 / 4.06 |
| BM_DxfReadNative/28000 | 95.21 / 106.27 | 80.75 / 93.82 |

The A/A gap is 1-5 % on the native benchmarks and 11-16 % on GDAL's. At 1 000
entities the native writer is 164-194 times as fast as GDAL's (min and
median, either copy) and the native reader 7.5-9.1 times. GDAL is not run at
28 000: one iteration would be a minute and a half.

### The owner's drawing

'Test 4 without tin' - 27 886 entities read from its .12da archive (14 089
points, 9 980 3D strings, 1 885 lines, 1 518 texts, 414 plan polylines on 13
layers) - through `katana_cli`: this branch's Release build for the native
module, main's Release build for GDAL, three rounds alternating between them,
twice. The export is `IMPORT` of the archive and `EXPORT` less the `IMPORT`
alone; the import is `IMPORT` of the file less the start-up.

| | native | GDAL |
| --- | --- | --- |
| export, run 1 / run 2 (medians) | 0.69 s / 0.99 s | 115.0 s / 96.8 s |
| what was written | 27 886 entities, 13 layers, 34.1 MB | 26 368 features: the 1 518 texts skipped |
| its own file read back, run 1 / run 2 | 1.37 s / 1.11 s: 27 886 entities | 2.97 s / 2.51 s: 26 368 |
| the other's file read, run 1 / run 2 | GDAL's file: 1.21 s / 0.99 s, 25 366 entities and 1 002 HATCH - its closed polylines | the native file: 3.61 s / 2.16 s, 27 886 |

The export subtraction is two noisy numbers of a second or two each; the
module alone, timed in-process on the same drawing, writes the 34 MB in
0.26-0.37 s and reads it in 0.29-0.46 s, and the rest of an `IMPORT` is the
command line's own work with 27 886 new entities, whatever the file.

Read back, the drawing is the drawing it was: 27 886 of 27 886 entities and
13 of 13 layers, every height, colour and layer exact, 27 747 geometries
bit-identical and the other 139 - texts - within 2.2e-16 rad of their
rotation. The exported file audits with no errors and no fixes in an
independent DXF library, and GDAL's driver reads all 27 886 of its entities.

## Tests

- `tests/dxf/data/r12_survey.dxf` and `r2000_site.dxf`: an R12 and an R2000
  drawing written by hand (the R12 one with CRLF line ends, as that release's
  writers made them). Every value the tests expect of them is worked out in
  the test beside the check.
- `tests/dxf/test_reader.cpp`, `test_writer.cpp`, `test_codes.cpp`,
  `test_import_command.cpp`, `test_robustness.cpp`: `katana_dxf_tests`.
- `cli.a_drawing_exported_to_dxf_and_imported_back_keeps_its_curves_text_and_layer`:
  the command line both ways.
- `benchmarks/bench_dxf.cpp`: native against GDAL, write and read.
