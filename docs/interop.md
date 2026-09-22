# Interoperability — GIS, rasters, point clouds and 12d archives

`katana_io` (the GDAL and PDAL adapters, exposed as `katana::gis` and
`katana::pointcloud`) and `katana_interop` (conversion to and from the domain
model). PLAN.MD Phases 17 and 20.

## Purpose

Import external survey and GIS data into the drawing, and export the drawing
back out. Phase 20 states the rule this layer exists to enforce: *"Do not make
the internal data model dependent on any external format. All importers should
convert external data into the internal domain model."*

## Layering

```
katana_io      GDAL + PDAL behind Katana interfaces. No domain knowledge:
  ↓            it does not know what an entity is.
katana_interop Converts what katana_io reads into entities and reference data.
  ↓            The ONLY layer allowed to see both.
katana_app     Own the reference data and offer Import/Export.
katana_qt
```

`katana_cad` deliberately may **not** see `katana_interop`. Keeping GDAL and
PDAL out of the core application layer is what lets it build with
`-DKATANA_BUILD_IO=OFF`, which the sanitizer CI job depends on — neither library
is sanitizer-instrumented, so their allocations produce false positives that
would drown real findings. `tools/check_layering.cmake` enforces this.

No GDAL or PDAL type appears in any public header: geometry crosses the boundary
as plain coordinate arrays (`GeoPoint`), rasters as 8-bit RGBA, and failures as
`Result<T>`. Both libraries signal errors by throwing; that is caught at the
adapter boundary and converted, so nothing throws across the interface.

## What is supported

| Direction | Formats |
|---|---|
| Vector in | Shapefile, GeoJSON, GeoPackage, KML, GML, DXF, MapInfo TAB, SQLite |
| Vector out | the same, driver inferred from the extension |
| Raster in | GeoTIFF, ASCII Grid, IMG, VRT, PNG, JPEG, JP2, ECW — GDAL's readers |
| Point cloud | LAS, LAZ, COPC, BPF, PLY, PCD, E57 in; LAS/LAZ out |
| 12d Archive | .12da and .12daz in and out — every element of the format; see below |

Not supported: **DWG, LandXML, IFC**.

## Two kinds of imported data

This is the distinction the whole design turns on.

**Vector data becomes entities.** An importer returns plain `Entity` values; the
caller wraps them in `commands::createEntities` and a `Transaction` alongside any
layers they need. So an import is one validated, atomic, undoable command like
every other edit, and a single Ctrl+Z removes the whole thing.

**Rasters and point clouds become reference data.** They are backdrop: material a
drawing is worked *on top of*, not made *of*. An entity is something you draw,
select, snap to, edit and undo; a 400-megapixel orthophoto is none of those. Making
it an entity would push a hundred megabytes of pixels through before-image undo
for a visibility toggle, and would give the user a "select all" that returns a
photograph. So reference data lives beside the model, is not undoable, and is
owned by the application layer.

Rule 3 still holds throughout: the viewport paints reference data, it does not
own it.

## Conversion, and what it costs

Every lossy step is stated rather than hidden.

* A **two-point LineString** becomes a `Line`, not a two-vertex polyline — a
  drafter expects to be able to fillet it. Longer ones become polylines.
* A **polygon** becomes closed polylines, one per ring, with the ring's role
  (`exterior` / `hole`) recorded in entity metadata. The entity model has no
  polygon-with-holes type, so the information is preserved where it can be
  rather than discarded.
* **Multi-geometries** are flattened to one entity per part, each carrying a
  copy of the feature's attributes.
* The **closing vertex** of a ring is dropped; `Polyline2::closed` expresses it,
  and keeping it would create a zero-length final segment the model rejects.
* **Arcs and circles** have no exact representation in these formats, so they
  are exported as polylines. `curveTolerance` is the sagitta — the greatest
  distance the polyline may deviate from the true curve — in model units,
  default 1 mm. The chord count follows `φ = 2·acos(1 − tolerance/r)`, so the
  result is the coarsest polyline meeting the tolerance and no finer.
* **Text and dimensions** have no counterpart at all. They are skipped, counted,
  and reported in `warnings` — never silently dropped (PLAN.MD §36).
* **Attributes** become string entity properties; entity properties become
  attributes, doubles formatted at `%.17g` so they round trip exactly.

### Precision at survey coordinates

`originShift` subtracts a local origin on import and adds it back on export.
Survey data often sits where a `double` has about 0.1 mm of resolution left; a
drawing worked at a local origin keeps full precision and exports back to the
true position unchanged.

## Format limitations that are refused rather than papered over

* A **Shapefile holds one geometry type per file.** GDAL discovers the mismatch
  only on the first feature of a different kind, by which point a partial
  `.shp`/`.shx`/`.dbf` set exists — and a half-written shapefile is worse than
  none, because it opens. So a mixed selection is refused *before* anything is
  created, with a message counting what is present and naming formats that can
  hold it.
* Every failure after the dataset is created unwinds through the driver's own
  `Delete`, which removes the sidecar files too.
* **GeoJSON always declares WGS 84** (RFC 7946). Projected coordinates written
  to it will be read back as degrees, so exporting without a CRS warns.
* **No reprojection.** A file's declared CRS is read and reported, never applied.
  Mixing coordinate systems is the user's responsibility and the UI says so.

## Scale

`readHeader()` reads only the LAS header, so the importer picks a decimation
step from the real point count before committing to a read — opening a
400-million-point file costs what opening a small one costs. Filtering, cropping
and decimation all happen *inside* the PDAL pipeline, so discarded points are
never materialised.

Rasters are decimated by GDAL during `RasterIO`, so a 2 GB GeoTIFF never becomes
resident; the geotransform is rescaled by the actual size ratio (not the
decimation step, which differs whenever the size is not an exact multiple).

Point clouds are drawn by splatting into an image buffer rather than one
`QPainter::drawPoint` per point — microseconds each would be seconds per frame
at two million points. Per-point colours are cached against the layer and its
colour mode, so only the projection is redone per frame.

**What is not implemented**: the out-of-core half of Phase 17. There is no
spatial hierarchy, no level of detail and no streaming. A billion-point dataset
is opened as a decimated sample held in memory, not worked on in full.

## Failure modes

| Condition | Result |
|---|---|
| File does not exist | `NotFound` |
| Extension has no importer/driver | `Unsupported`, naming the extension |
| Mixed geometry into a Shapefile | `Unsupported`, nothing written |
| Nothing matched the export filter | `InvalidArgument`, no file created |
| Raster with no georeferencing | imported, placed at the origin, **warned** |
| Band index out of range | `InvalidArgument` |
| No points survive the import filters | `InvalidArgument` |
| GDAL/PDAL internal failure | `FileImportFailure` / `FileExportFailure`, carrying the library's own message as context |

## Threading

`ensureRegistered()` registers GDAL's drivers once, under a `call_once`, and the
driver manager is deliberately never destroyed — it is process-global state
shared by every open dataset, and tearing it down when one dataset closes
invalidates all the others. Process exit frees it, which costs nothing.

A `GdalDataset` is not thread-safe; use one per thread.

## Point-cloud level of detail: COPC, not a bespoke octree

PLAN.MD Phase 17 asked Katana to build a spatial hierarchy for point clouds.
It should not build one. A Cloud Optimised Point Cloud is a LAZ file whose
chunks are already arranged as an octree, and the PDAL linked into
`katana_io` reads and writes it natively (`pdal --drivers` lists
`readers.copc` and `writers.copc`). `readers.copc` takes a `resolution` and
returns only the octree levels at or above that point spacing, so level of
detail is a *query parameter* rather than a data structure Katana owns,
maintains and keeps correct.

What the engine now offers (`include/katana/pointcloud/point_cloud_engine.hpp`):

* `PointCloudReadOptions::resolution` - the coarsest spacing acceptable.
* `convertToCopc(source, destination)` - any PDAL-readable cloud to COPC,
  done once so every later view can ask for just the resolution it needs.
* `isCopc(path)` - whether PDAL will read the file as COPC.

**A resolution asked of a non-COPC file is refused, not ignored.**
`readers.las` has no such option; passing one through would hand the whole
file to a caller who asked for a coarse sample of a billion points. That is
the silent failure PLAN.MD section 36 forbids, so it is `InvalidArgument`
naming `convertToCopc`.

**The destination must be named `.copc.laz`.** The extension is what makes
every later read infer `readers.copc`; a COPC file called `.laz` would be
read as plain LAZ and could never answer a resolution query.

**The test fixture's spacing is load-bearing.** A COPC node keeps at most
one point per cell of a 128-cell grid across its span, so on a 100 m root a
point every 0.78 m or coarser lands entirely in the root node and EVERY
resolution returns the whole file - a fixture that could never show a coarse
read being coarser. The tests use 0.25 m spacing (160 000 points) and assert
the properties of level of detail rather than the counts one PDAL version
happens to produce: coarser is never more, a 5 m query is under a quarter of
the file, and it still spans the whole extent - which is the difference
between a level of detail and the `maxPoints` truncation it replaces.

**Not done yet.** The engine can answer the query; nothing asks it. The
importer still decimates by a fixed step, and the viewport still holds that
one sample. Converting on import and re-querying at a resolution derived
from the view's `worldPerPixel` is the next slice.

## DXF export had never worked, and why nothing noticed

Found by running the *bundled* CLI in isolation, not by a test: `EXPORT x.dxf`
failed. Every export test used a format that accepts arbitrary fields and
needs no data files, so the one format a CAD program cannot do without was
never exercised. Two independent causes:

**GDAL could not find its own data.** GDAL builds every DXF from two template
files, `header.dxf` and `trailer.dxf`, found through `GDAL_DATA` or a path
compiled in when GDAL was built. The MSYS2 GDAL is relocatable, so the compiled
path is useless, and only an MSYS2 *login shell* sets `GDAL_DATA`. Started any
other way - Git Bash, an IDE, Explorer, ctest, a bundle on another machine -
GDAL found nothing. `locateGdalData` now sets `GDAL_DATA` to
`<directory of the GDAL DLL>/../share/gdal` when nothing else has, which is
correct for the toolchain and for a bundle alike, and is the mechanism PROJ
already uses for `proj.db`.

Two approaches were tried and rejected, and both are worth knowing:

* *Setting the variables from each program's `main()`*, relative to the
  executable. Built, tested, and removed within the hour: it rested on the
  belief that the compiled-in path works on the build machine, which is false,
  so the build tree stayed broken; and it was a second mechanism beside the
  one PROJ has.
* *Asking Windows which module holds `&GDALAllRegister`.* Under MinGW that
  address is the import THUNK inside the importing executable, so Windows
  truthfully names the caller and the data is looked for beside the wrong
  file. The module is found by name among the loaded modules instead
  (`libgdal*.dll`, or `gdal*.dll` from an MSVC build).

**A fixed-field format was treated as a failure.** A DXF layer has `Layer`,
`Linetype`, `Text` and a few more fields and refuses any other, and the writer
aborted on "could not create field `katana_id`". It now asks
`OLCCreateField`, writes the geometry, and keeps any attribute the format
already has a field for - OGR matches names case-insensitively, so the Katana
attribute `layer` lands in the DXF field `Layer` and every entity keeps its
CAD layer. What is dropped is reported as a warning (`driverHasFixedFields`),
not silently.

`InteropExport.ADrawingCanBeWrittenToDxfAndKeepsItsLayers` was watched to fail
for each cause separately before it passed.

## Imported entities keep their layers

A DXF is ONE layer to GDAL (`entities`), and each entity's CAD layer arrives as
its `Layer` attribute. The importer used to place everything on a layer named
after the source layer, so a 51 000-entity drawing arrived on a single layer
called `entities` and the layer tree was useless.
`VectorImportOptions::layerAttribute` (default `layer`, matched
case-insensitively) now names the attribute that carries each entity's layer;
Katana's own exports write the same information, so a round trip through
GeoJSON or GeoPackage keeps its layers too. An explicit `targetLayer` still
wins, and no empty `entities` layer is created when nothing ends up on it.

**Still open:** text and dimensions are skipped on DXF export, though DXF has
both, because the vector path models only points, lines and polygons.

## The 12d Archive format (.12da, .12daz)

12d Model is the civil design package most Australian survey and road work is
delivered in, and its interchange format - the 12d Archive, `.12da`, and its
zipped form `.12daz` - is what a surveyor hands over. (There is no `.12dz`;
Katana accepted the extension for a day by mistake.) Katana reads every element the format defines and writes back everything
it can represent. This section records what was decided and, more usefully,
what the manual does not say and had to be measured.

### Where it lives, and why it is not part of `katana_interop`

The format is a module of its own, `katana_archive12d`, beside `commands` in
the layering: it sees `core`, `math`, `geometry`, `terrain` and `entity` and
nothing else. Reading a 12da needs no third-party library - it is text - and
keeping it that way means the module builds with `-DKATANA_BUILD_IO=OFF`,
which is the configuration the sanitizer job runs. A hand-written parser of
untrusted text is exactly the code that most needs to run under
AddressSanitizer, and it would have been excluded from that had it lived
behind GDAL.

Only the ZIP container needs a library. That stays in `katana_io` behind
`gis/zip_container.hpp`, opened through GDAL's `/vsizip/`, and only bytes cross
the wall. **Rejected:** a small inflate of our own inside `katana_archive12d`,
which would have made `.12daz` available without GDAL. Nobody needs that - a
build without GDAL has no importer at all - and it would have been a second
implementation of something zlib already does, written by us, on untrusted
input.

The container itself was established from an archive written by 12d Model:
one member, DEFLATE, DOS platform header, named after the *project* rather
than the zip (`UT5527 Appin Rd V5.12daz` holds `Appin Rd V5.12da`). So the
importer does not predict the member's name; it takes the one member that is
a `.12da`, and refuses an archive with two rather than guess.

### Three layers: text, archive, domain

`readArchive` turns text into an `Archive` - typed values, every element the
manual defines, with its own vocabulary and no knowledge of entities.
`toDomain` maps that onto entities, alignments, surfaces and point clouds.
`writeArchive` and `fromDomain` are the inverses. The split is what makes
"every element accounted for" a checkable claim: `coverage.hpp` lists the
format's elements with the manual section each comes from, and a test writes
a minimal instance of every row, reads it, and fails if the reader skips any
block of it or the import does other than the row claims. The table in this
document is generated from those rows, not maintained beside them.

**12d Model writes far more than its manual documents.** A real export
carries `drawables`, `geometry_modifiers`, `equalities`, `extrude_value`,
`vertex_uid_data`, `time_created`, `weight`, `solid_fill` and dozens more, and
the construction methods of a super alignment other than IPs are declared
undocumented by the manual itself. So the reader does not pretend the
vocabulary is closed: a scalar it has no member for is kept, in order, in the
element's `extras`, and written back; a *block* it has no member for is
skipped whole and counted by path in `Archive::unrecognised`, and the import
lists those counts in its warnings. An import says exactly what it did not
take. Nothing scalar is lost between a file and the file exported from it -
the extras travel through an entity as `12d.x.<key>` metadata.

**The superseded string types are super strings.** The manual says of the 2d,
3d, 4d, pipe and polyline strings that each "has been superseded by the super
string", and a super string expresses every one of them; the alignment and
pipeline strings likewise by the super alignment. They are read into the same
structures, tagged with the kind they came from, and written back as the
current types - which is what 12d Model itself does.

### The encoding is not what the manual implies

The manual calls a 12da "a Unicode file" and says no more. Every archive
written by 12d Model 15 that this was developed against - nine files, 4 to
58 MB - is UTF-16 little-endian with a byte order mark; the hand-written ones
are UTF-8. A reader that assumed UTF-8 would fail on every file that came out
of the program the format belongs to. So the bytes are decoded first: by their
mark; failing that by where the NUL bytes fall (ASCII as UTF-16 has one in
every other byte); failing that as UTF-8 if the bytes validate and as
Windows-1252 if they do not, and the fallback is reported. Katana writes
UTF-16LE with a mark by default, as 12d does, and UTF-8 on request.

### What the manual does not say, measured against 12d's own output

Three conventions were settled against `test Super Alignment.12da`, written
by 12d Model 15.0C1t, which holds 32 arcs and 16 transitions whose end points
are all recorded. The reasoning and the numbers are in
`src/katana_archive12d/plan_geometry.hpp`; in short:

1. **A positive radius turns right.** The manual says "+ve is above the line
   connecting the vertices", which does not say which way is up. All 18 arcs
   that follow a straight agree: 10 positive, all clockwise; 8 negative, all
   counter-clockwise.
2. **A trailing transition is described backwards.** The manual defines
   `l1 r1 a1` as the values "at the start vertex" and says a full trailing
   transition has `r2 = 0, l2 = 0`. What 12d writes is `l1 = 0, r1 = 0` for
   trailing transitions too, with `a1` the tangent at the segment's *second*
   vertex pointing back along the string. Read that way all 16 transitions
   land on their recorded end points; read the manual's way every trailing one
   misses by 40 to 195 m.
3. **"Cubic parabola" is `y = m x³` in the frame of the tangent,** with `m`
   chosen so that the *true* curvature at the end is `1/R` and the *arc
   length* to the end is `L`. Under that definition all 16 close to under
   0.01 mm and reproduce `a2` to the four decimals written. The textbook
   `y = x³/(6RL)`, and a clothoid, both miss by up to 0.3 m on a railway
   transition of `L = 80, R = 210`.

Two more came from real files rather than the manual: trimesh flags are
one-based indices into their info table with 0 meaning none (the manual's own
example - two infos, flags `2 0 1 2 0` - reads no other way, and 12d's exports
agree); and 12d Model 15 writes both `pipe_data` and `culvert_data` on every
culvert string although the manual says a string "cannot have both", so that
is not a warning.

The other transition types the manual names and does not define (Westrail
cubic, cubic spiral, Bloss, sinusoidal, cosinusoidal) are drawn as the
clothoid with the same end radii and moved to meet the vertex 12d recorded,
and the size of that movement is reported - it *is* the error. A transition
that would have to move more than a metre, or more than its own chord, is not
an approximation of anything and is drawn straight and counted.

### Where each element goes

The mapping is documented in full at the head of
`include/katana/archive12d/domain.hpp`. The decisions worth defending:

- **Models are layers.** 12d model names are already `/`-separated tree names
  (`Stage 1/Water/Drainage`), which is exactly what a Katana layer path is.
  Model names are compared as the manual says - case ignored, blanks trimmed -
  so `" Fred "` and `"FRED"` are one model and land on one layer.
- **Heights.** Entities are 2D. A string whose vertices share one height
  carries it as the `elevation` property, which Surface From Drawing already
  read; one whose heights differ carries them all in `elevations`, in vertex
  order, with `null` where 12d had no height. Surface From Drawing now reads
  both, leaves a null vertex out and breaks the breakline there: a null is
  "not surveyed", and triangulating it at zero would dig a pit to the datum
  under every unlevelled point. `entityHeights` is the one reader of that
  list, used by export and the application alike.
- **Alignments.** Katana defines an alignment by its PIs; a 12d super
  alignment stores the solved elements and, separately, however the designer
  constructed them. Where the construction is the IP method the PIs are taken
  directly. Otherwise they are *reconstructed* from the solved elements - each
  run of spiral/arc/spiral between two straights is one PI at the intersection
  of those straights, with the arc's tangents taken exactly from the circle
  (a chord's direction is off by half the angle it subtends, and a PI built
  from chords drawn to a millionth of a unit still landed 9 mm out) - and the
  reconstruction is then **checked**: Katana solves it and every vertex 12d
  recorded must lie within 10 mm of the result. Where the check fails, or the
  geometry has no PI form (a compound curve, an alignment starting on an arc,
  a transition type Katana does not have), the alignment is imported as its
  polyline only, with the reason. An IP-only definition with a transition
  type Katana lacks is taken with clothoids of the same lengths and says so,
  since there is nothing to check it against and a centimetre-out alignment is
  worth far more than none. What can be drawn always is: the tangent polygon
  through the IPs when nothing else can be solved.
- **Linestyles are styles.** A 12d linestyle name becomes a Katana `Style`
  of that name, which the entity carries in its own `style` field - the field
  the style panel, the `STYLE` verb and the plotter read - rather than as
  metadata. The importer lists the styles it needs and the caller creates the
  missing ones in the same transaction as the layers, so an import is still
  one undo step. A 12da says nothing about what a linestyle looks like, so a
  new one is continuous at the default weight and described as "12d
  linestyle" for the user to finish in the style manager. **Rejected:**
  guessing a dash pattern from the name ("DRAIN Water Course" is probably
  dashed): wrong more often than right, and a wrong pattern is worse than a
  solid line the user knows to fix.
- **Symbols are on the style, and a point takes its symbol's style.** A
  12d vertex symbol (`symbol_value` for the string, `symbol_data` per
  vertex) is a linestyle drawn at a vertex with a colour, a size, a rotation,
  an offset and a raise; 12d writes one on every surveyed point - 27 075 of
  them across the sample archives, the string's own linestyle "0" beside
  the symbol's. On a one-vertex point string the symbol is the point's whole
  appearance, so the point's `style` becomes the symbol's linestyle, the
  `Style` row carries the shape and the size, and the symbol's colour is the
  entity's colour. The shape is read from the linestyle name
  (`symbolForLinestyle`: "SEWR Manhole Cover" is a manhole, "ELEC Pole -
  Light" a pole, "TOPO Natural Surface Point" a cross, anything unreadable a
  circle), because a 12da carries only the name and what it looks like lives
  in the 12d project; the style manager is where a wrong guess is put right,
  once per name rather than once per point. The string's own linestyle is
  kept as `12d.string_style` and written back, so the string is the string
  it was. What 12d writes on tens of thousands of points as `rotation 0
  offset 0 raise 0` is not kept: only a value that says something becomes
  `12d.symbol.<key>` metadata, and export writes the defaults. On a LINE the
  vertices have no style of their own; every block is kept as one list per
  key (`12d.symbol.style` = `Post Post "Gate Post"`), written back as
  `symbol_value` or `symbol_data` by the length of the lists, and counted in
  a warning because they are not drawn. **Rejected:** a symbol field on the
  entity - it would mean two places a point's appearance can come from, and
  12d's own answer is that a symbol IS a linestyle. **Rejected:** making the
  symbol's colour a second colour on the entity: a point has one colour on a
  Katana screen, and it is the symbol's, because the symbol is what you see.
  The string's own colour name is a different thing and is NOT overwritten by
  it: it stays in `12d.colour` and export writes it back unexamined when the
  entity's colour came from a symbol (the rule that rewrites a colour name
  the user has since changed would otherwise rename the string after its
  symbol). In every sample archive the two names are the same, so this only
  ever shows on a file that uses the format's freedom; it was found by asking
  what happens when they differ, and the answer was that the string's colour
  was lost. A symbol colour Katana has no RGB for is kept by name and
  written back.
- **Attributes** become typed properties; a group flattens into
  `Group/Sub/Name` and is rebuilt on export. Vertex and segment attributes
  become `vertex/3/Name` and `segment/2/Name`; a one-vertex string's vertex
  attributes are simply its own. Everything else 12d knows about a string -
  name, style, colour name, chainage, breakline, point ids, pipe sizes,
  visibility, tinability, segment colours, interface modes - travels in
  `12d.*` metadata and is written back, so a string imported and exported
  unchanged is the string it was.
- **Colours.** A 12da carries colour *names*; the RGB behind a name lives in
  the 12d project and can be redefined there, so no table can be "right".
  The standard names 12d ships with get the X11 values of the same names
  (with grey's shades set either side of grey, since X11's DarkGray is
  lighter than its Gray); an unknown name (`pen 025`, `vis concrete`) leaves
  the entity ByLayer and is kept as metadata. On export the name is written
  back unless the colour has been changed since, when the nearest standard
  name is written instead - the old name would be a lie.
- **Drainage.** The line becomes a polyline carrying its pipes as properties
  (pipe *i* joins pit *i* to *i+1* and has no position of its own), each pit
  a point at its top with its name, type and size, each house connection a
  point at its *adopted level* - the manual marks `z` "internal use only"
  there, and 12d writes 0, thirty metres below the job.
- **Tins.** A `full_tin` lists every triangle including the nulled ones and
  the ones touching its four construction points; the surface is the rest,
  and a triangle with a null-height vertex has no surface to give. Triangles
  are accepted either way round - the manual wants clockwise, other writers
  do not comply, and a surface built inside out fails as a whole. Katana
  writes the `tin` form (visible triangles only), which the manual
  recommends to "most software packages" and which cannot get the
  mandatory neighbours block wrong. Per-triangle colours are kept and written
  back but a surface here has no use for them, which is said.
- **Trimeshes become meshes in the session.** A `primitive_3d` is a
  `geometry::TriangleMesh` held beside the surfaces, drawn in 3D and as its
  footprint in plan (docs/cad.md, "A mesh is not a surface"). Per-face
  colours survive both ways: the one-based `face_flags` into `face_infos` on
  the way in, one info per DISTINCT colour on the way out, because an info
  is a colour and not a face. A face that repeats a vertex or uses one with
  no height is dropped and counted - it names no triangle in space - while
  a face naming a vertex that does not exist is refused by the READER,
  which can say which line. Vertex and edge infos, the edge list ("for
  checking only", manual 1.4.9) and `blend` are read and not modelled.
- **Point clouds** become reference layers. A `ref_data` cloud names a LAS
  file relative to a 12d project the archive has left behind; it is reported
  for the user to import, not chased.

### The null value

12d's null height is a *value* (`null -999`, the default) that a `null`
command can change part way through a file and a string's own `null_value`
can override; 12d Model 15 also writes the keyword `null`. The reader keeps
raw heights while an element is being read and applies the value in force
once the element is whole, so a `null_value` written after its data - string
commands are order-free - still applies. The writer writes one null value
for the whole file, and chooses one that equals no real height in the archive:
the archive's own where it is safe, else sentinels below the Mariana Trench.
Without that, a height read under one null value and written under another
came back as null - silently, from a plain read and write - which is the
first defect the review below found.

### What the reader will not guess

A file it cannot make structural sense of is a `ParseFailure` naming the
line: a brace never closed, a data block whose values do not divide into rows,
a word where a number belongs, a triangle naming a point that does not exist.
Guessing at any of those would shift every coordinate after the mistake. A
string that lacks what defines it - an arc without its centre, text without a
position - is ignored and named, as the manual says (1.4.6), rather than
placed at the origin, which in an MGA job is six thousand kilometres away.
Mandatory blocks that are absent are reported; a documented key that happens
to be followed by a bare word the reader knows (`text PIT`, `title_1 Scale`,
`plotter model` - all legal, the manual requires quotes only around text that
is not alphanumeric) is read as the value it is. Limits on element count,
block size and nesting depth make a hostile file a refusal rather than an
exhaustion; 185 hostile and degenerate inputs, from a lone `}` to 100 000
nested braces to 1e308 coordinates, produce a clean failure or a sensible
result and no crash.

### An adversarial review found nineteen defects before anyone else did

Once the module passed its own tests, seven independent reviewers audited it
by lens - one per chapter of the manual, real files, hostile input, geometry,
test quality - and every finding was handed to a second reviewer told to
refute it. Nineteen survived, all fixed, each with a regression test in
`tests/archive12d/test_review_findings.cpp`. The ones that would have hurt:
the null-value collision above; documented keys mis-read as flags; arcs and
text placed at the origin for want of a coordinate; house connections thirty
metres underground; a superseded alignment that could not be solved dropped
entirely while the message said "imported as a polyline"; vertical arcs
honouring `major`, which the manual says to ignore, and losing the whole
grade line; per-segment pipe sizes, vertex attributes, segment text,
visibility and tinability silently dropped; a LAS colour that is 64 unsigned
bits parsed as signed. Four of the seven reviewers did not complete, so the
real-file sweep, the hostile-input fuzzing and a geometry check were done by
hand afterwards; the test-quality review was not, and is recorded as
outstanding in `PLAN.MD`.

### Coverage

Every element of the 12d Model V15 manual, with what Katana does with it. The
handling column is asserted by `tests/archive12d/test_coverage.cpp`.

| Element | Manual | Handling | In Katana |
|---|---|---|---|
| `model` | 1.4.1 | import and export | a layer; a tree name such as Stage 1/Water arrives as that nested layer |
| `colour` | 1.4.2 | import and export | the entity's colour where the name is one of 12d's standard colours; the name is always kept |
| `style` | 1.4.3 | import and export | a Katana Style of that name in the entity's own style field; a vertex symbol's linestyle is the point's style, with the symbol on it |
| `breakline` | 1.4.4 | import and export | kept on the entity as 12d.breakline and written back |
| `null` | 1.4.5 | import and export | a height equal to the null value, or the null keyword, is no height at all |
| `attributes` | 1.3 | import and export | typed entity properties; a group flattens into Group/Name and is rebuilt on export |
| `project_attributes` | - | read | read into the archive; a Katana project has no attributes of its own |
| `tin` | 1.4.7.2 | import and export | a surface (terrain::TinSurface) |
| `full_tin` | 1.4.7.1 | import | a surface of its visible, non-construction triangles; written back as a tin |
| `super_tin` | 1.4.8 | read | reported; its member tins are what is imported |
| `primitive_3d` | 1.4.9 | import and export | a mesh in the session (geometry::TriangleMesh), drawn in 3D and as its footprint in plan; per-face colours kept and written back |
| `string arc` | 1.5.1 | import | an Arc; exported arcs are written as two-vertex super strings |
| `string circle` | 1.5.2 | import and export | a Circle |
| `string drainage` | 1.5.3 | import | the line as a Polyline carrying its pipes, each pit and house connection as a Point |
| `string face` | 1.5.4 | import and export | a closed Polyline, written back as a face |
| `string feature` | 1.5.5 | import | a Circle |
| `string interface` | 1.5.6 | import and export | a Polyline with its cut/fill modes, written back as an interface |
| `string plot_frame` | 1.5.7 | import | the sheet rectangle, paper size times plot scale, as a closed Polyline |
| `string super` | 1.5.8 | import and export | a Point, Line or Polyline; arcs and transitions chorded to a stated tolerance |
| `string super_alignment` | 1.5.9 | import and export | a named Alignment where its geometry is PI-definable and verifies, and always a Polyline of the centreline |
| `string text` | 1.5.10 | import and export | Text |
| `string 2d` | 1.5.11 | import | as string super |
| `string 3d` | 1.5.12 | import | as string super |
| `string 4d` | 1.5.13 | import | as string super, with its vertex text as Text |
| `string pipe` | 1.5.14 | import | as string super, with its diameter |
| `string polyline` | 1.5.15 | import | as string super |
| `string alignment` | 1.5.16 | import | as string super_alignment |
| `string pipeline` | 1.5.17 | import | as string super_alignment |
| `string las_cloud_data` | 1.5.18 | import | a point cloud reference layer; all eleven point record formats, tagged and compact |

### Measured

Release, GCC 16.2, this machine (see `CLAUDE.md` for the toolchain): the
58 MB Windsor Road export (2 364 super strings with per-vertex attributes,
1 453 trimeshes, UTF-16) decodes in 0.06 s, reads in 0.14 s and maps in
0.08 s; the 55 MB `Test 4 with Tin` (7 820 strings and a 233 946-triangle
full_tin written in hexadecimal floats) reads in 0.13 s and builds its
229 462-triangle surface in 0.13 s; the 33 MB `Test 4 without tin` (26 683
elements) reads in 0.12 s. Writing the 55 MB file back takes 0.5 s. All from
`katana_12da_probe`, which prints them.

`katana_12da_probe <file.12da> [--rewrite out.12da]` is the tool for the
question "did the importer take all of it?": it lists every kind of element
with how many were read and imported, every block the reader has no member
for, and every warning, and with `--rewrite` checks that the file written back
reads the same.

### Not done

A symbol on the vertices of a LINE (468 fence and kerb strings in Windsor
Road, 18 in `Test 4`) is kept and written back but not drawn, and a point
symbol's rotation, offset and raise are kept and not drawn either: a Katana
point has no rotation, and offset and raise are paper-space quantities. A
mesh is session data, so it is drawn but not saved with the project - the
same open question as a surface. A mesh's vertex and edge infos, its edge
list and its `blend` are read and not modelled. Per-vertex annotation settings
beyond text height and angle (offset, raise, justification, slant) are not
taken. Super tins are reported, not built - Katana has no notion of one
surface overriding another where they overlap. The undocumented parts of a
super alignment (`computator`, `floating_arc_end_radius_length` and the rest)
are read as fields and not interpreted; such an alignment arrives through its
solved geometry, which is what the manual says a reader should use. A 12da
declares no coordinate system, so nothing is reprojected.
