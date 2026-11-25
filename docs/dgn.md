# DGN - MicroStation design files, read and written natively

`katana_dgn` (`crates/katana_dgn/src/`) reads and writes MicroStation V7
design files - the Intergraph Standard File Format, ISFF - itself, with no
third-party library. Every front end sends a `.dgn` to it: File > Import >
Import Any File, File > Export > Export Drawing, a path given to `katana.exe`,
and the `IMPORT` and `EXPORT` verbs of the window's command line, of
`katana_cli` and of `katana_mcp`. GDAL's own DGN driver no longer sees one.
A V8 design is refused by name, with what to do (below, "V8").

## The open software that reads and writes DGN

Surveyed on 2026-11-25. DGN is two formats: V7, ISFF, the element stream
MicroStation wrote until 2001, documented in the open by GDAL's dgnlib; and
V8, an OLE compound document of compressed element pages that Bentley has
not published.

| Software | V7 | V8 | Licence | Here |
|---|---|---|---|---|
| GDAL's OGR "DGN" driver, on dgnlib (`ogr/ogrsf_frmts/dgn/` in GDAL 3.13.2) | read; write 2D and 3D on a seed file | no | MIT | Linked already, and in both builds (MSYS2's GDAL 3.13.3 and conda-forge's): rejected as the route (below), kept as the reference for every offset |
| GDAL's "DGNv8" driver | - | read, write | GDAL's MIT, but built only against the Open Design Alliance's Teigha libraries | Not usable: Teigha is proprietary, and neither MSYS2's nor conda-forge's GDAL has the driver |
| dgnlib (Frank Warmerdam, maptools.org) | read, write | no | MIT/X | Its maintained copy is GDAL's |
| ODA Drawings SDK (Teigha), ODA File Converter | read, write | read, write | proprietary, membership | Excluded: nothing proprietary |
| cadkit-dgn (Rust, crates.io; github.com/TNYCL/cadkit) | read | read; seed-based write | MIT OR Apache-2.0 | The one open V8 reader as a crate. First released 2026-10-01, 17 releases in eight weeks, one author; its own notes say its V8 writing's "external application acceptance remain open". Not added: a new crate is the owner's decision ("Not done") |
| ezdgn (Rust core, Python package; github.com/monozukuri-ai/ezdgn) | read; write 2D | read | MIT | Not on crates.io: a Python package, not a library Katana can link |
| QGIS | through GDAL | through GDAL with Teigha | GPL | GDAL's driver, as above |
| LibreCAD, FreeCAD | none found | none found | GPL, LGPL | No DGN reader in either was found in their documentation |

Read for this: GDAL's driver pages (`doc/source/drivers/vector/dgn.rst` and
`dgnv8.rst` of GDAL 3.13.2), dgnlib's sources in GDAL 3.13.2 (`dgnopen.cpp`,
`dgnread.cpp`, `dgnwrite.cpp`, `dgnhelp.cpp`, `dgnstroke.cpp`,
`ogrdgnlayer.cpp`, `ogrdgndatasource.cpp`, `ogrdgndriver.cpp`) and GDAL's
`port/cpl_vax.cpp`; crates.io's records of cadkit-dgn and its 0.5.2 package
(its package description, its manifest and the project's provenance notes);
ezdgn's front page and its PyPI record.

## What a .dgn did before

Until this module, a `.dgn` was a GIS file:

- **IMPORT** of a V7 design went through GDAL's DGN driver as vector data
  (`docs/interop.md`, "Two kinds of imported data"): one OGR layer,
  `elements`, so every element landed on ONE layer named after it (the
  driver's level is a field, `Level`, and the importer reads a `layer`
  field); arcs and ellipses came in as line strings stroked every 5 degrees
  (`ogrdgnlayer.cpp`), curves and B-splines as polylines; a text as a point
  with its string in a `Text` property; a cell's components and a complex
  chain's members each as features of their own; the colour index, weight
  and style as properties, not as the entity's colour. Z was kept.
- **IMPORT** of a V8 design failed with GDAL's message that the DGNv8 driver
  is not in the build.
- **EXPORT** to a `.dgn` went to GDAL's DGN writer: on GDAL's
  `seed_2d.dgn`, metres at a centimetre (`ogrdgndatasource.cpp`), every
  element on level 0 (the driver reads its `Level` field, which Katana
  never wrote) in colour 0, every arc and circle chorded, text left out
  (`text=skip`, the default), polygons as shapes.

## The route, and what was rejected

**A reader and a writer of our own for V7, as the DXF module is**
(`docs/dxf.md`): the format is a published element stream, small enough to
read in safe Rust, and GDAL's driver loses exactly what made the DXF
module worth writing - true arcs, text, layers and colours. A design comes in
as a drawing does: entities on layers, one undoable step, a tally and
warnings.

- *Rejected: GDAL's DGN driver with a better mapping of its fields.* Its
  features have already lost the arcs (stroked), the complex chains
  (split) and the text placement by the time Katana sees them, and its
  writer chords every curve. The drawing would come in as GIS data.
- *Rejected: dgnlib's own functions.* They are internal to GDAL's library,
  not part of GDAL's C API, and reaching them would take a shim of our own,
  where the project writes none beyond the three that GDAL's and PDAL's
  C++-only interfaces need (`docs/rust.md`).
- *Rejected: a V7 header of our own.* The 768-word design header holds the
  views and every active setting MicroStation starts with, most of them
  described nowhere open. A file is written on a SEED design, as MicroStation
  and GDAL's writer make every new design: the header and the two control
  elements after it are the seed's.
- *Deferred, the owner's decision: V8 through cadkit-dgn* ("V8", below).

## Where it sits

```text
core < math < geometry < entity
                           |
                          dgn        (beside dxf: no GDAL)
                           |
                 app (katana_cli, katana_mcp), window (katana)
```

`dgn` may see core, math, geometry and entity. The import's undo step is
the DXF module's - `katana_dxf::import_command::import_parts`, which builds
the one transaction (layers before entities, locked layers opened and
locked again) for either reader - so the two imports cannot differ about it,
and `dgn` needs no commands. The default seeds are found by
`katana_gis::gdal_data_file`, GDAL's own file finder (`CPLFindFile`, the
way GDAL's DGN writer finds them), from the app layer.

| File | What |
|---|---|
| `crates/katana_dgn/src/codes.rs` | The format's numbers: the middle-endian integer, the VAX D-float, the range's binary offset, RADIX-50 cell names, the element kinds and the header's offsets, MicroStation's default colour table. |
| `crates/katana_dgn/src/plane.rs` | How a 2D rotation or a 3D quaternion lies on the plan, both ways. |
| `crates/katana_dgn/src/reader.rs` | `read_dgn`, `read_dgn_file` -> `DgnImport`: entities, layers, bounds, the working units, a tally per element kind, warnings. `identify`, `is_dgn_path`, the level's layer name. |
| `crates/katana_dgn/src/writer.rs` | `write_dgn`, `write_dgn_file` on a seed -> `DgnExport`: the bytes (or the file), counts, the levels and the layer each holds, warnings. `seed_info`, `has_heights`. |
| `crates/katana_app/src/dgn_verbs.rs` | The executor's read and apply, write and records, and which seed. |

## The format

Every offset in the code names the line of dgnlib it was read from. What
the code relies on:

- **An element** is a header word (level 0-63 in six bits, a "complex" bit
  for a component of a cell or chain; the type in seven bits, a "deleted"
  bit), its length in words after the first two, a range block of six
  corners (each an integer with its sign bit flipped), then the display
  header: graphic group, the index of its attribute data, properties
  (0x8000 a hole, 0x0800 attribute data present), line style and weight in
  one byte, colour index in the next. The element's own data starts at
  byte 36 (`dgnread.cpp:1274-1324`).
- **An integer** is two little-endian words, the high word first
  (`dgnlibp.h:70`). **A real** is a VAX D-float: (-1)^s x 0.1f x 2^(e-128),
  an 8-bit exponent and a 55-bit fraction, in four little-endian words, the
  most significant first (`port/cpl_vax.cpp`). Every double from 2^-128 to
  2^127 is one exactly (`numbers::every_double_the_format_holds_comes_back_bit_for_bit`).
- **The design header** (type 9, 768 words, `dgnread.cpp:1488-1585`): the
  sub-units per master unit at byte 1112, units of resolution per sub-unit
  at 1116, the units' two-character names at 1120 and 1122, the 3D flag
  (0x40 of byte 1214), and the global origin at 1240, three reals in units
  of resolution. A coordinate is an integer of units of resolution, and its
  value in master units is (u - origin) / (sub-units x units per sub-unit),
  read as ONE division so that a coordinate on a decimal step reads back as
  the nearest double to that decimal.
- **Angles** are integers of 1/360000 degree; an arc's sweep is a sign and a
  magnitude, 0 a whole turn (`dgnread.cpp:672-685`). **Text sizes** are
  integers of 6/1000 of a unit of resolution (`dgnread.cpp:749`). A 3D
  element's orientation is a quaternion of four integers of 1/2^31, the
  INVERSE of the element's turn (`dgnhelp.cpp:1084`), so the element's axes
  are the rows of its matrix (`plane.rs`).
- **Groups**: a cell header and a complex chain or shape header say their
  length in words after the header's first 38 bytes, members included
  (`dgnwrite.cpp:1920`, `:1672`); a chain also says how many members it has.
  Every member carries the complex bit.
- **The colour table** is group data (type 5) on level 1: the background's
  three bytes, then colours 0-254 (`dgnread.cpp:1330-1354`). dgnlib takes the
  file's LAST table, and so does this reader. A file without one uses
  MicroStation's default table, dgnlib's `abyDefaultPCT`, copied into
  `katana_dgn::codes::DEFAULT_COLOURS` (`THIRD_PARTY_NOTICES.md`).
- **The design ends** with the word FF FF.

## Reading

| V7 | Becomes |
|---|---|
| Line (3) | a Line; one of no length a Point - how dgnlib and MicroStation put a point. |
| Line String (4), Shape (6) | a Polyline; a shape closed, its repeated last vertex dropped. |
| Complex Chain (12), Complex Shape (14) | ONE polyline: a CurvePolyline when a member is a circular arc - the arc its start vertex's bulge - else a Polyline. Members meet within two units of resolution, or within 1e-7 of an arc's radius (its angles' step); where they meet, the line string's integer vertex is kept. An elliptical arc member is chords within 1 mm, counted. Chains nested in chains give their members in place. |
| Arc (16), Ellipse (15) | an Arc, or a Circle when it turns whole, when its axes are equal; else an Ellipse (the longer axis the major one). A 3D one whose plane is seen from below turns the other way on the plan; one in a tilted plane is not read, and said. |
| Text (17) | a Text at its origin - the left end of its baseline, whatever its justification - with its height and rotation; the font and the justification are kept as metadata (`dgn.font`, `dgn.justification`) for the writer. 8-bit text is read as UTF-8 where it is (GDAL's writer writes UTF-8), else Windows-1252; 16-bit text (after FF FD) as Latin-1, a double-byte code page's characters as U+FFFD, said. |
| Text Node (7) | its texts, each a Text. |
| Cell (2) | its components, each with the cell's name (`dgn.cell`, "OUTER/INNER" nested). A closed shape flagged a hole, and the closed shapes beside it, are tagged `source.ring` (`hole`, `exterior`) and `source.part` (the cell): MicroStation's grouped hole is an area with its hole, and the one ring rule (`katana_entity::ring_tags::is_tagged_hole`) reads it so. |
| Curve (11) | a Polyline through the points it passes through (its first two and last two set its end slopes, `dgnstroke.cpp`), counted: MicroStation smooths between them. |

- **Layers.** A level is a layer named "level N": a V7 design numbers its
  levels 1-63 and names none (names lived in MicroStation's level files, not
  in the design). One layer for each level an entity is on.
- **Colours.** Each entity's own colour, from the file's colour table, else
  the default one; the index beside it (`dgn.colour`). A V7 element has no
  "by level" colour.
- **Line style and weight** are kept as metadata (`dgn.style`, 0-7;
  `dgn.weight`, 0-31) where not 0, and written back: Katana has no
  per-entity field for either yet, as the DXF reader keeps its own.
- **Heights.** A 3D design's Z, unless every Z of the element is 0, which is
  what a plan element of a 3D design holds - the DXF reader's rule
  (`docs/dxf.md`, "Heights"). Through `katana_entity::entity::set_heights`.
- **Sub-unit corrections**: the 0xA9 0x51 attribute linkage dgnlib applies to
  a line's or line string's vertices (`dgnread.cpp:416-464`) is applied.
- **Deleted elements**, which MicroStation keeps until a design is
  compressed, are passed over and tallied as "Deleted".
- **Placement**: `LOCAL`, `ALONGSIDE` and `OFFSET=` move a design as one
  piece, read again with the shift, as every drawing import does
  (`docs/interop.md`, "Placing an import").

Not read, tallied and said: B-spline curves and surfaces, 3D surfaces and
solids, cones, point strings, shared cells, tag values, and reference file
attachments (said: import each of them as well). Control elements - the
header, digitizer setup, level symbology, application elements, colour
tables - are not drawing content and are not tallied.

### When the file is damaged

`read_dgn` fails only when the file is no V7 design at all, each by name:
empty; a V8 design; a cell library; a DXF or a DWG named .dgn (rename it);
anything else ("not a DGN file"); or a design whose header is cut short.
Past the header, damage costs the element it is in: an element that runs
past the end ends the read there, what came before imported and the cut
said; a file with no FF FF is read and said; an element shorter than its
kind, a number that is not one (the VAX reserved operand), no length, no
radius or no text is left out and counted. Every entity is checked with
`katana_entity::validate` before it is kept. Cells and chains nested deeper
than 16 and an import past 20 million entities are stopped and said.
`robustness::damaged_copies_never_crash_and_successes_hold_only_valid_geometry`
reads 3 000 damaged copies of a design of every kind, 2D and 3D.

## Writing

A drawing is written on a seed:

- **Given** (`seed=<file>`, a person's office seed): its header and its
  digitizer setup and level symbology, as `DGNCreate` copies them
  (`dgnwrite.cpp:290-403`), and its working units and global origin KEPT. A
  coordinate off its design plane (2^31 - 1 units either side) refuses the
  export, naming the coordinate - never clipped.
- **Not given**: GDAL's `seed_3d.dgn` when the drawing holds a height, else
  `seed_2d.dgn`, found where GDAL's writer finds them, with KATANA's units:
  master metres ("m"), sub-units millimetres, at the finest of 0.1 mm, 1 mm,
  1 cm, 1 dm and 1 m whose plane holds the drawing about its middle (at
  0.1 mm the plane is 429 km across), and the global origin at that middle,
  a whole number of metres, so that a coordinate of up to four decimals
  comes back as the double it was
  (`round_trip::a_drawing_written_and_read_back_is_the_drawing_it_was`). One
  model unit is one master unit: Katana works in metres.

| Katana | Written as |
|---|---|
| Point | a Line of no length. |
| Line | a Line. |
| Polyline | a Line String, a Shape when closed (its first vertex repeated last); past 101 vertices (`dgnwrite.cpp:844`) a Complex Chain or Shape of line strings that share their ends; past one header's 65 535 words, several chains, said. |
| Arc, Circle | an Arc element (start and sweep, sign kept), an Ellipse of equal axes - true curves. |
| Ellipse | an Ellipse, or an Arc element of its axes when partial. |
| CurvePolyline | a Complex Chain or Shape of line strings and arc elements. |
| Text | a Text element a line, each at the left end of its baseline as `katana_entity::text_block::line_origin` lays the lines out (the bottom left of a text is its LAST line's baseline); a justification read from a file goes back with its one line; Windows-1252, a character it lacks as "?", a line cut at 255 bytes - each said. |
| Spline | a Line String of chords within 1 mm, said: B-splines are not written. |
| Dimension, Label, Leader | what the front end drew of them for this file: the plan view's drawing at the annotation scale (cad's export annotation, aligned dimensions too); skipped and said when nothing was drawn. |

- **Levels.** A layer named "level N" (1-63) goes back to level N; every
  other layer takes the lowest free level in the order its entities are
  first met; past the 63rd, layers share level 63, said. The reply has a
  `level number=N layer=<name>` record for each, so the file's numbers can
  be read against the drawing's names.
- **Colours.** Every file gets a colour table: MicroStation's default one,
  each colour of the drawing that is not in it put in an unused slot from
  254 down to 16, so the colours come back exact and 0-15 keep their usual
  meaning; past 239 colours, the nearest in the table. An entity's colour is
  its own, else its layer's. **Black** is written as colour 0, the
  foreground (white on MicroStation's black screen), as the DXF writer
  writes it as 7, and said.
- **Heights** go into a 3D seed's Z where every vertex has one. Heights at
  only some vertices, or an arc whose ends differ, are written in plan
  (Z 0) and counted: the format has no "no height". A 2D seed holds none,
  said. A level of exactly 0 everywhere reads back as none (the reader's
  rule above).
- **Angles** are written to 1/360000 degree and text sizes to 6/1000 of a
  unit of resolution: an arc's end moves by at most r x 2.4e-8, a 2.5 m text
  at 0.1 mm comes back 2.5000002 m.
- **Style, weight, font** come from the metadata the reader keeps; 0, 0 and
  font 1 otherwise.
- **A file is replaced whole**: written beside the target and renamed over
  it.

Not written: properties and survey attributes (V7 has nowhere for them that
other programs read), alignments, surfaces, blocks, hatch patterns, and the
layers' names (only in the reply).

## V8

A V8 design (MicroStation from 2001 on, an OLE compound document) is refused
by name, `Unsupported`, with what to do: save it as V7 in MicroStation
(File > Save As, type V7), or export it there as DXF and import the DXF. No
library in the build reads V8: GDAL's DGNv8 driver needs the ODA's
proprietary Teigha libraries, which neither MSYS2's nor conda-forge's GDAL
has.

The one open V8 reader that is a Rust crate is cadkit-dgn (MIT OR
Apache-2.0, "Clean-room DGN V7 and V8 reader", crates.io). Its 0.5.2 of
2026-11-01 is the newest release more than two weeks old; it depends on
cadkit-core, flate2 (the miniz_oxide backend, which Katana's update layer
already carries), encoding_rs and serde. Reading V8 through it would be a
module beside the V7 reader mapping its document model onto entities. It is
not added: a new crate is added with the owner's knowledge, and this one is
eight weeks old with one author.

## On every surface

- **The window.** File > Import > Import Any File and GIS > Import Vector
  Data route a `.dgn` to the Placement step (`importPlacementDialog`), as a
  DXF's, which runs `IMPORT "<file>" <word>` through the window's executor;
  a path given to `katana.exe` is imported the same way
  (`crates/katana_window/src/main_window/window.rs`). File > Export > Export
  Drawing offers `.dgn` (GDAL's listing, or "MicroStation DGN (V7)" where a
  build of GDAL lists none) and opens the Export Drawing dialog
  (`crates/katana_window/src/gis_export_dialog.rs`): the shared scope, and
  for a `.dgn` the seed design (`vectorExportSeed`, off for any other file);
  GDAL's fields are off. Its line is what Run hands the executor.
- **The command line** (`katana_cli`, the window's): `IMPORT <file.dgn>
  [LOCAL | ALONGSIDE | OFFSET=dE,dN]`; `EXPORT <file.dgn> [<scope>]
  [seed=<file>] [PREVIEW]`. GDAL's options on a `.dgn` are refused by name,
  and `seed=` on any other file.
- **MCP**: `katana_import` and `katana_export` (its `seed` argument), which
  build those lines (`docs/mcp.md`).

The records:

```text
imported file=<path> kind=dgn format="DGN V7 2D" entities=3 layers=1 units=m uor_per_master=10000 bounds=...
tally element=Shape read=1 imported=1
exported file=<path> kind=dgn driver=DGN format="DGN V7 3D" entities= skipped= elements= levels= units=m uor_per_master= bytes=
level number=1 layer=Survey/Kerb
```

`uor_per_master` is the units of resolution a master unit holds: 10 000 is
a step of 0.1 mm. The placed and scope records are every import's and
export's (`docs/interop.md`).

## Tests

- `crates/katana_dgn/tests/codes.rs`: each number form against bytes worked
  out by hand (`numbers::vax_reals_of_hand_worked_values_read_and_write_exactly`),
  every double of the D-float's range bit for bit, RADIX-50, the default
  table, the quaternion both ways
  (`orientation::a_turn_written_as_dgnlib_writes_it_reads_back_as_the_turn`).
- `crates/katana_dgn/tests/reader.rs`: designs built byte by byte from
  dgnlib's offsets in the test (`crates/katana_dgn/tests/support/mod.rs`),
  every expected value worked out beside its check - MGA coordinates to the
  nearest double
  (`elements::lines_line_strings_and_shapes_come_in_on_their_levels_at_their_coordinates`),
  arcs and ellipses, text, chains, a cell's hole, colour tables, 3D heights,
  each refusal (`refusals::a_v8_design_is_refused_by_name_with_what_to_do`)
  and each damage.
- `crates/katana_dgn/tests/writer.rs`: a drawing of every kind written and
  read back, the header's bytes, the units chosen
  (`the_design::units_are_the_finest_step_whose_plane_holds_the_drawing_about_its_middle`),
  levels, colours, heights, long polylines, text.
- `crates/katana_dgn/tests/robustness.rs`: damaged and hostile designs.
- `crates/katana_app/tests/dgn_io/dgn_verbs.rs`: the executor's lines - the
  import as one undo step
  (`dgn_verbs::a_drawing_exported_and_imported_is_one_undo_step_on_its_level`),
  LOCAL, the scope, `seed=`, the refusals - and MCP.
- `dgn_export::a_dgn_takes_the_scope_and_its_seed_alone` and its neighbour:
  the Export Drawing dialog's line.
- `text_encoding::windows1252_encoding_gives_back_every_byte_its_decoding_read`:
  the writer's encoder, `katana_core::text_encoding::encode_windows1252`.
- `cli.a_drawing_exported_to_dgn_and_imported_back_keeps_its_curves_text_and_colour`
  and `cli.a_dxf_named_dgn_is_refused_by_name_with_what_to_do`: the command
  line; `qt_dgn_export_and_import_go_through_the_native_module_headless`:
  the window.

No test reads a DGN made by MicroStation: none is committed, since a file of
unknown origin is not, and none could be made here. The reader's tests are
held to dgnlib's published layout; a design MicroStation wrote, with its
licence, is the test to add ("Not done").

## Not done

- **V8.** Refused by name; cadkit-dgn is the candidate ("V8"), the owner's
  decision.
- **Checked against MicroStation.** Neither a design MicroStation wrote has
  been read nor a file written here opened in MicroStation: the layout is
  dgnlib's, which has written files MicroStation opens for twenty years, and
  the seeds are GDAL's. The colour table element and the complex chain's
  complex bit on its header are written as dgnlib writes them.
- **Level names.** V7 has none; the reply's `level` records are all there
  is. A level-name file or MicroStation's type 66 level table is not read
  or written.
- **Line styles and weights** are kept as metadata, not drawn, and a
  layer's linetype and lineweight are not written.
- **B-splines, surfaces, solids, cones, point strings, shared cells, tags and
  reference attachments** are not read.
- **Text width** (the length multiplier) and fonts are not drawn; a text
  is written with its width equal to its height.
- **The DXF writer** puts a bottom-left multi-line text's FIRST line at the
  text's position, where `katana_entity::text_block::line_origin` - the one
  layout of the plan view and of this writer - puts the LAST line there. One
  of the two disagrees with the plan view; the DXF writer's case wants
  checking (`docs/dxf.md`, "Writing").
- **INFO** of a `.dgn` is still GDAL's description of it as features.
