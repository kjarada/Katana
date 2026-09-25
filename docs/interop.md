# Interoperability — GIS, rasters, point clouds and 12d archives

`katana_io` (the GDAL and PDAL adapters, exposed as `katana::gis` and
`katana::pointcloud`) and `katana_interop` (conversion to and from the domain
model): the point-cloud and file-interoperability work.

## Purpose

Import external survey and GIS data into the drawing, and export the drawing
back out. The interoperability plan stated the rule this layer exists to
enforce: *"Do not make
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
`-DKATANA_BUILD_IO=OFF` - which also needs `-DKATANA_BUILD_QT_APP=OFF`, since
the application links interop - and that is the configuration a sanitizer build
wants: neither library is sanitizer-instrumented, so their allocations produce
false positives that would drown real findings. (This said the sanitizer CI job
depends on it. There is no CI workflow in the repository - it was deleted, and
audit BLD-01 is open - so the `sanitize` preset is run by hand; see
`docs/building.md`.) `tools/check_layering.cmake` enforces the layering.

No GDAL or PDAL type appears in any public header: geometry crosses the boundary
as plain coordinate arrays (`GeoPoint`), rasters as 8-bit RGBA, and failures as
`Result<T>`. Both libraries signal errors by throwing; that is caught at the
adapter boundary and converted, so nothing throws across the interface.

## What is supported

| Direction | Formats |
|---|---|
| Vector in | Shapefile, GeoJSON, GeoPackage, KML, GML, DXF, MapInfo TAB, SQLite |
| Vector out | the same, driver inferred from the extension |
| Raster in | GeoTIFF, ASCII Grid, IMG, VRT, PNG, JPEG, JP2 — GDAL's readers |
| Point cloud | LAS, LAZ, COPC, BPF, PLY, PCD in; LAS/LAZ out, at 1 mm (audit IO-17); any of them to COPC |
| Raster out | a surface as a DEM: GeoTIFF, Esri ASCII grid, Erdas IMG (`exportSurfaceRaster`) |
| 12d Archive | .12da and .12daz in and out — every element of the format; see below |

Not supported: **DWG, IFC**, and **ECW and E57**: the extensions are routed
to GDAL and PDAL, but the MSYS2 toolchain has no ECW SDK and no PDAL E57
plugin, so such a file fails to open with the library's own error (this table
listed both until the audit of 2026-09-23 checked the toolchain). LandXML
SURVEY data - points and observations - is read by the survey data exchange
(`docs/survey.md`), not here.

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
  and reported in `warnings` — never silently dropped (`docs/architecture.md`,
  "Error handling").
* **Attributes** become string entity properties; entity properties become
  attributes, doubles formatted at `%.17g` so they round trip exactly.
* **Heights.** A file's Z becomes the `elevation` / `elevations` properties
  that the 12d archive, the survey import and Surface From Drawing all read
  (`entity.hpp`, one writer `setHeights` and one reader `heightsOf`); on export
  they become the geometry's Z again. A 2D feature gets no height property at
  all, and a real Z of 0 is kept as 0 - the adapter's `VectorGeometry::hasZ`
  is what tells them apart, because a 2D source used to read as z = 0
  everywhere. Until 2026-09-23 (audit IO-01) import dropped Z and export wrote
  Z = 0, so a 3D DXF became a drawing on the datum and Surface From Drawing
  built it flat, and nothing said so. What cannot survive is reported:
  * a vertex dropped for standing on the one before it in plan takes its
    height with it (a vertical step) - counted in `warnings`;
  * a 3D geometry needs a height at every vertex, so an entity heighted at
    only some is written in plan with its heights as attributes, and an arc
    whose two ends differ is too - a height between an arc's ends is not
    recorded anywhere, and interpolating one would be inventing it;
  * a **shapefile** layer is all 2D or all 3D and a 3D one has no "no height",
    so a drawing mixing heighted and heightless entities goes in plan with the
    heights as attributes and a warning (GeoPackage, GeoJSON, DXF and GML keep
    each geometry's own dimension and need none of this). Reading such a file
    back takes a 2D feature's numeric `elevation` attribute as its height, so
    the round trip still keeps them;
  * a source attribute named `elevation` or `elevations` is taken by the
    geometry's Z when there is one, and the replacement is counted: those two
    names are where Katana keeps a height.

  Each case has a round-trip test through the real driver (`InteropHeights`),
  because which of these a GDAL driver does was checked, not assumed.

### Precision at survey coordinates

`originShift` subtracts a local origin on import. The reason is NOT the
coordinates themselves - this said a double has "about 0.1 mm of resolution
left" at survey magnitudes, and a double at 1e7 resolves about 2e-9 m. It is
what is done WITH them: products and differences (a polygon's shoelace terms at
1e7 are 1e14, with ulps of about 0.02 m²), and the float the 3D path and any GPU
use, which has about a metre of resolution at 1e7. A drawing worked at a local
origin keeps all of that small.

It is not undone on export yet: the shift is not in `VectorImportResult`, and
neither the GUI nor the CLI passes one to an export, so a drawing imported
shifted exports shifted (audit QT-07, open). The header's "recorded in the
result" describes the intent, not the code.

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
  The one exception is data fetched from a web service, whose CRS nobody chose:
  GIS > Online Data moves it into the project's CRS through
  `VectorImportOptions::targetCrs` and a GDAL warp (`docs/gis_online.md`).

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

**What is not implemented**: the out-of-core half of the point-cloud work. There is no
spatial hierarchy, no level of detail and no streaming. A billion-point dataset
is opened as a decimated sample held in memory, not worked on in full.

## Failure modes

| Condition | Result |
|---|---|
| File does not exist | `NotFound` |
| Extension has no importer/driver | `Unsupported`, naming the extension |
| Mixed geometry into a Shapefile | `Unsupported`, nothing written |
| Nothing matched the export filter | `InvalidArgument`, no file created |
| Raster with no georeferencing | imported, placed at the origin, **warned**; refused by Surface From Raster (`InvalidArgument`), which would otherwise build ground in the wrong place |
| Vector export with a CRS GDAL cannot read | `InvalidCRS` before anything is written (it used to be dropped and the file written with none) |
| A write that fails at GDAL's setters or at close | `FileExportFailure`, and the partial file removed (audit IO-14; the close path is reviewed, not tested - no failure could be injected there) |
| DEM export over `maxCells` (25 million) | `InvalidArgument` naming the cell count; the dialog refuses first |
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

The original plan asked Katana to build a spatial hierarchy for point clouds.
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
the silent failure `docs/architecture.md` ("Error handling") forbids, so it
is `InvalidArgument`
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

**Asked by a person since 2026-09-23.** GIS > Convert Point Cloud to COPC
(and `COPC` on either command line) converts a file, and GIS > Import Point Cloud offers a
point spacing when - and only when - the file is COPC
(`PointCloudImportOptions::resolution`). **Not done yet:** nothing converts
on import by itself, and the viewport still holds the one sample it was given;
re-querying at a resolution derived from the view's `worldPerPixel` is the
next slice.

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

## The GIS menu: every GDAL and PDAL capability, reachable

Until 2026-09-23 the desktop application reached GDAL and PDAL through two
items - File > Import (any file, default options) and File > Export Vector -
and several things the libraries and `katana_interop` could already do had no
way in: writing a point cloud, converting to COPC and reading COPC at a level
of detail, choosing a GeoPackage's layer, a point budget or a class filter,
the export's curve tolerance, looking at a file before importing it. And two
commands that should have used GDAL and PDAL did not: Surface From Raster
rebuilt heights from 8-bit display greys (audit QT-23), and Surface From Point
Cloud triangulated every return, canopy and roofs included (QT-10).

The **GIS** menu and toolbar hold all of it, grouped by library and data:

| Section | Item | What it calls |
|---|---|---|
| Vector - GDAL | Import Vector Data... | `describeSource`, then `importVector` with the dialog's `VectorImportOptions` (source layer, target layer, attributes) |
| | Export Vector... | File's own action; now with a dialog for `VectorExportOptions` (selection, layer name, curve tolerance, properties) |
| Raster - GDAL | Import Raster... | `importRaster` with a display resolution |
| | Export Surface as DEM... | `exportSurfaceRaster` - GeoTIFF, Esri ASCII grid or IMG |
| Point Cloud - PDAL | Import Point Cloud... | `importPointCloud` with a budget, an ASPRS class, and a COPC resolution when the file is COPC |
| | Export Point Cloud... | `exportPointCloud` - LAS or LAZ |
| | Convert Point Cloud to COPC... | `PointCloudEngine::convertToCopc` - every point, then an offer to import it |
| | Dataset Information... | `describeSource` + `formatDescription`, the gdalinfo / pdal info a person needs first |
| Online - Web Services | Online Data... | `interop::fetchOnlineLayer`: imagery, elevation and features from public web services, warped or reprojected into the project's CRS and taken in through `importRaster` and `importVector`; the `ONLINE` verbs do the same (`docs/gis_online.md`) |

Decisions, and what was rejected:

* **File > Import stays as it was.** It is the quick way in - any file, the
  default options, no questions - and a command-line argument and the `IMPORT`
  verb take the same path. The GIS imports are the considered way: they
  describe the file and offer its options before anything is read. Putting a
  dialog on File > Import was rejected: a single-layer shapefile would then
  cost a click for nothing, every time.
* **A dialog is built from a description of the file, not from the kind of
  menu item.** `makeImportOptions` routes by what `describeSource` finds, so a
  `.las` picked through Import Vector's "All files" still gets the point-cloud
  dialog, and the dialog offers the layers THIS GeoPackage has and a COPC
  level of detail only when the file IS COPC - the engine refuses the question
  of any other file, so offering it would be offering a failure.
* **One wording.** `formatDescription` is shown by Dataset Information, above
  every import dialog's options, by the Reference Data panel's Info button,
  and by the `INFO` verb of both command lines, so a file cannot be described
  two ways.
* **A point cloud's export says when it is a sample.** A budgeted import holds
  one point in N; Export Point Cloud asks before writing a sample as though it
  were the survey, and points at Convert to COPC for the whole file.
* **Surfaces use the libraries.** Surface From Raster re-reads the band's
  true values through GDAL (`readRasterElevations`) on a stride that keeps the
  whole extent under the triangulation cap (QT-24: the old stride could pass
  it threefold). Surface From Point Cloud uses `surfacePoints`, the one policy:
  the ground returns (ASPRS class 2) when the cloud has any, otherwise every
  return - and the log says which, because a surface over trees presented as
  ground is the failure QT-10 found. Choosing which raster or cloud now takes
  the panel's selection, or the only one there is, before asking.
* **Reference data goes with its drawing.** File > New, Open, and `NEW` or
  `OPEN` typed on the command line clear the rasters and clouds (QT-17): an
  orthophoto of the last site no longer sits behind the next one.

The interoperability verbs now exist in BOTH command lines, as the CLI's own
comment always claimed: `IMPORT`, `EXPORT`, `INFO` and `REFS` in the desktop
application's, and `INFO` and `COPC <source> <destination.copc.laz>` added to
`katana_cli`'s. `IMPORT <raster or cloud> LOCAL` in the CLI is refused by name
(QT-13, QT-14): reference data is drawn at its own coordinates, and the old
code left " LOCAL" on the path and reported a missing file.

Since 2026-09-26 the two command lines read these verbs alike
(`docs/desktop.md`, "The session's verbs on the window's command line"):

- **`IMPORT <file> LOCAL`** moves a DXF, vector file or .12da archive as one
  piece so the lower-left corner of what it holds sits at 0,0 - read again with
  that `originShift`, so the one reader moves every kind of geometry alike -
  in the window too, where it asks no placement question. Both front ends read
  the argument with `CommandInterpreter::importArgument`: one pair of quotes
  off the path, and `LOCAL` only as an unquoted last word. The session had
  taken `LOCAL` off and left the quotes on, so `IMPORT "<path>" LOCAL` - what
  `katana_import` sends with `local: true` - looked for a file named with its
  quotes and failed for every GIS file; the window took `LOCAL` for part of
  the path.
- **`INFO <id>`** is the interpreter's entity description whenever the word
  is an id (`CommandInterpreter::isEntityId`) and no file of that name exists;
  both front ends had read every `INFO` as `INFO <file>`.
- **`COPC`** is on the window's command line, its paths read by the
  interpreter's `tokenize` in both front ends; GIS > Convert Point Cloud to
  COPC runs the `COPC` line it makes through the window's one executor.

Everything a menu item does can be driven headlessly: `--action <name>`
triggers the QAction by its object name, `--dataset-info` and
`--import-options` build and grab those windows, and a headless run echoes its
log to stderr, where `tools/check_screenshot.cmake`'s `EXPECT` reads it. The
ctest cases assert values worked out outside Katana: the sample DEM's range
read straight from the ASCII grid (24.892 to 38.819 - not the 25.054 to 38.633
of the approximate statistics GDAL had once written into a
`terrain.asc.aux.xml` that was then committed by accident; audit IO-13, and
the file is gone and `samples/**/*.aux.xml` ignored), and the sample scan's
29 512 ground points counted by `pdal translate` with `filters.range`.

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

The same module holds the CUSTOMISATION - the `.4d` linestyle and symbol
libraries and the survey code file (`.mapfile`): their readers, their writers (`writeStyleLibrary`,
`writeMapFile`), the loader that tells them apart by content, and
`mergeCustomisation`, which loads one on top of another. None of that is the
archive format, and it is recorded in `docs/survey_coding.md`.

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
  new style's LINETYPE is the 12d linestyle's own name, at the default weight
  and described as "library linestyle": a loaded style library draws it by
  that name, and with none loaded the name resolves to nothing and the line is
  solid (`docs/cad.md`, "What a style draws"). This bullet used to say a new
  style was "continuous", which is what the import once wrote - and why no
  imported line ever drew its linestyle (`docs/survey_coding.md`, "The import
  has to keep the real names"). An empty 12d linestyle name is ByLayer: the
  entity gets no style. **Rejected:** guessing a dash pattern from the name
  ("DRAIN Water Course" is probably dashed): wrong more often than right, and
  a wrong pattern is worse than a solid line the user knows to fix.
- **Symbols are on the style, and a point takes its symbol's style.** A
  12d vertex symbol (`symbol_value` for the string, `symbol_data` per
  vertex) is a linestyle drawn at a vertex with a colour, a size, a rotation,
  an offset and a raise; 12d writes one on every surveyed point - 27 075 of
  them across the sample archives, the string's own linestyle "0" beside
  the symbol's. A string with ONE symbol block - a one-vertex point, or a
  line of any length - takes that symbol as its whole appearance: its `style`
  becomes a style named after the symbol's linestyle, whose `symbol` is the
  REAL 12d name and whose `symbolSize` is the block's size (described "library
  symbol"), and the symbol's colour is the entity's colour. The name is kept,
  not a guess at a shape: a loaded symbol library draws it, and only when
  nothing defines the name does the viewport fall back to the built-in shape
  the name suggests (`entity::builtInSymbolFor`: "SEWR Manhole Cover" is a
  manhole, "ELEC Pole - Light" a pole, "TOPO Natural Surface Point" a cross,
  anything unreadable a circle) - the import used to store the guess and
  throw the name away, so a loaded library could never match it. On a line
  the symbol is drawn at every vertex (decision D8, `docs/cad.md`), and the
  style's linetype, being the symbol's own name, draws a plain line under
  it - so no list of missing names reports that linetype
  (`cad::NameStatus::OwnSymbol`; `docs/cad.md`, "What the managers stand
  on"), although it names a symbol and no linestyle. The string's own linestyle is kept as `12d.string_style` when it
  differs, and written back, so the string is the string it was. What 12d
  writes on tens of thousands of points as `rotation 0 offset 0 raise 0` is
  not kept: only a value that says something becomes `12d.symbol.<key>`
  metadata, and export writes the defaults. A string carrying a DIFFERENT
  symbol per vertex, which the format allows and 12d does not write, has no
  one symbol for its style: every block is kept as one list per key
  (`12d.symbol.style` = `Post Post "Gate Post"`), written back as
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
- **Export writes what the style draws.** A string's 12d linestyle is the
  LINETYPE of its Katana style, and a symbol block's `style` is the style's
  `symbol` - never the style's own name (`domain_export.cpp`,
  `linestyleOf`). Import makes the two the same, so this changed nothing
  until someone renamed a style in Katana, at which point export used to
  rename the linestyle 12d draws to one no loaded library defines. A ByLayer
  entity - no style, or a style whose linetype is `ByLayer` (decision D2) -
  writes its LAYER's linetype. Katana's `continuous` goes out as `1`, 12d's
  default solid linestyle (manual 1.4.3), since 12d has no linestyle called
  continuous; for every layer an import creates that is still `1`, as it
  always was. The style's name is written only for a style the model does
  not hold, where it is the only information there is.
- **Text is placed where 12d put it.** A 12d annotation anchors text by one
  of nine justifications ("top|middle|bottom" by "left|centre|right"); a
  Katana `TextGeometry` position IS the left end of the baseline, so the
  anchor is moved by the justification it was placed with. The width that
  needs is ESTIMATED at 0.6 of the height per character - there is no font
  here - which only has to beat ignoring justification, which is out by the
  whole width. Every annotation in the sample archives is "bottom-left",
  which is already Katana's meaning, so this shows only on files that use
  the format's freedom. The text's own colour (`text_colour`, or `colour`
  inside a vertex or segment annotation) is the entity's, with 12d's
  "no_colour" meaning "the string's"; its `textstyle` is a Katana style like
  any other. An `offset` is in the same units as the SIZE, so it is applied
  only with `worldsize` (model units) and kept as metadata with `papersize`
  (millimetres on a plot, meaningless without a plot scale - the rule the
  height already follows); its direction is not in the manual and is taken
  as perpendicular to the text, to the left. A `raise` is a LEVEL, so it is
  added to the text's elevation and moves nothing in plan.
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
  name is written instead - the old name would be a lie. The standard names
  are listed by `archive12d::standardColourNames()`, read from the same table
  `standardColour` draws with, which is what the Survey Code Manager's colour
  field offers; the dialog keeps no copy of the table.
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
  mandatory neighbours block wrong. Per-triangle colours survive the ARCHIVE
  round trip only: the domain import does not carry them onto the surface and
  the domain export writes none (a tin goes out green) - which this used to
  describe as "kept and written back".
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
- **Super tins are built.** A super tin is a ranked list of tins that
  behaves as one surface, and the 12da carries only the list (manual 1.4.8):
  `terrain::combineSurfaces` makes the surface, a later member overriding an
  earlier one wherever it covers it. The members remain surfaces of their
  own, because a tin and a super tin are separate objects in 12d. Which end
  of the list wins is NOT in the manual - this follows the sample archive's
  own comment, "base surface first, then the pads that substitute it over
  their footprints" - and a triangle of a lower member is dropped whole
  where a higher one covers any of it, so the seam is ragged by up to one
  triangle and no point ever has two answers. See docs/cad.md for why that
  trade was made in that direction.
- **Point clouds** become reference layers. A `ref_data` cloud names a LAS
  file relative to a 12d project the archive has left behind; the file is
  looked for BESIDE the archive - where a 12da and its scans travel together
  when a job is sent on - and read when it is there. Only the file name of
  the reference is used: following "..\..\scans\site.las" out of the
  directory the archive was found in would let a file choose what gets read.

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
hand afterwards; the test-quality review was not, and is still outstanding.

### Coverage

Every element of the 12d Model V15 manual, with what Katana does with it. The
handling column is asserted by `tests/archive12d/test_coverage.cpp`.

| Element | Manual | Handling | In Katana |
|---|---|---|---|
| `model` | 1.4.1 | import and export | a layer; a tree name such as Stage 1/Water arrives as that nested layer |
| `colour` | 1.4.2 | import and export | the entity's colour where the name is one of 12d's standard colours; the name is always kept |
| `style` | 1.4.3 | import and export | a Katana Style of that name, with the name as its linetype, in the entity's own style field; a vertex symbol's linestyle is the string's style, with the symbol on it. Export writes the linestyle the style DRAWS (its linetype), not the style's name - see "Export writes what the style draws" below |
| `breakline` | 1.4.4 | import and export | kept on the entity as 12d.breakline and written back |
| `null` | 1.4.5 | import and export | a height equal to the null value, or the null keyword, is no height at all |
| `attributes` | 1.3 | import and export | typed entity properties; a group flattens into Group/Name and is rebuilt on export |
| `project_attributes` | - | read | read into the archive; a Katana project has no attributes of its own |
| `tin` | 1.4.7.2 | import and export | a surface (terrain::TinSurface) |
| `full_tin` | 1.4.7.1 | import | a surface of its visible, non-construction triangles; written back as a tin |
| `super_tin` | 1.4.8 | read | BUILT as a combined surface, with its member tins imported as surfaces of their own (see "Super tins are built"; this row said "reported" after that was no longer true) |
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

Release, GCC 16.2, this machine (`docs/building.md`, "Toolchain"): the
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

### The round trip, measured

`tests/interop/test_round_trip.cpp` imports a fixture, puts everything it
gave into a model, exports it, and imports that - three times. The FIRST
export normalises, because the format has several spellings of one thing and
Katana writes one of them: an arc goes out as a two-vertex super string, a
drainage string as its line plus a point per pit, a `full_tin` as a `tin`.
So the property that must hold is not "the first reading equals the second"
but **"every reading after the first is identical"**, plus "the first pass
loses nothing" - every style with its symbol, every layer, every mesh with
its faces and face colours, every surface's triangles.

`katana_12da_probe <file.12da> --roundtrip` runs the same thing on the large
archives, which are real project data and are not in the repository. Run over
the fourteen on 2026-09-22:

| Archive | Result |
|---|---|
| Windsor Road (58 MB, 1 453 meshes) | stable: 2 409 entities, 82 styles, 1 453 meshes |
| Test 4 without tin / Test_V2 (33 MB) | stable: 27 177 entities, 212 styles |
| Test 4 with Tin (55 MB) | stable: 7 820 entities, 1 surface |
| PV tri Lat Long | stable: 311 entities, 116 meshes, 2 alignments |
| test trimishes complex | stable: 2 888 entities, 266 meshes |
| test super tin, test multiple tins | stable, surfaces included |
| test drainage strings, test Super Alignment, test las point cloud | stable |
| comprehensive-all-geometries, repro-2vertex-pipe-bucketing | stable |
| plot_PW_example_data | stable: 4469 entities, 8 surfaces (after the flat-triangle fix below) |

Two defects came out of this, which is what it was written for. The first is
fixed: exporting a drawing imported from a 12da wrote every alignment TWICE
- once as the alignment and once as the centreline polyline the import had
made to draw it with - and each round trip read the duplicate back and added
another. The exporter now skips a polyline that is the centreline of an
alignment it is writing, and the alignment carries the layer, colour and
style that polyline wore (they were hard-coded to model "Alignments",
colour red, style "1" before, so this is also the first time an alignment
keeps its appearance).

The second is fixed, and is the more interesting of the two. Four surfaces of
`plot_PW_example_data.12da` built on the first import and were refused on the
second with "two triangles run along an edge in the same direction". The
cause was not a duplicated triangle, which is what it looks like: it was a
**flat** one.

12d's tins contain triangles whose three points are collinear. Counted from
the file's own hexadecimal floats, outside Katana (`tools/sliver_census.py`),
seven of them across the
eight surfaces have a doubled plan area below 6e-14 m^2 - over coordinates of
6.2e6 m, where the arithmetic that computes that area cannot resolve better
than about 1e-14. So the SIGN of such a triangle's area is not information.
The import measured it anyway to decide which way round the triangle went,
and handed `TinSurface` an edge direction decided by the last bits of a
subtraction; one triangle facing the wrong way refuses the whole surface.

The import now nulls a triangle with no plan area, exactly as it already
nulls one whose heights are null - there is no ground under it to interpolate
over, and `geometry::Triangle2::isDegenerate` is the tolerance policy that
already says what "no plan area" means. The census (a triangle is flat when
its plan area's sign flips as the coordinates are rounded to eight places)
predicted precisely the four failures and the four survivors:

| Surface | Flat triangles | Read back |
|---|---|---|
| Design/2/BASIN, CHANNEL, SWALE, Existing/SURVEY | 0 | always did |
| Design/2/LOTS | 2 | was refused |
| Design/2/ROADS | 4 | was refused |
| Design/2/ROADS DETAIL | 1 | was refused |
| Super/FS | inherits its members' | was refused |

The second fault was ours: the writer put tin points out at eight decimal
places, which moves a point by up to 5e-9 and so can flatten a sliver that
was not flat. `WriteOptions::hexFloatTins` now defaults to true - 12d Model's
own default (`output_tin_hex_floats true`), and for its reason. Note the
arithmetic: 5e-9 is twenty times below `tolerance::kGeometric`, so rounding
can only ever flatten a triangle that already counts as flat. That is why
either fix alone makes the file stable, and both are kept: one says what a
degenerate triangle means, the other says whose bits those were to throw
away.

### Not done

A symbol on the vertices of a LINE (468 fence and kerb strings in Windsor
Road, 18 in `Test 4`) is now drawn at every vertex (decision D8) when the
string carries one symbol block, which is what 12d writes; a string carrying
a different symbol per vertex - which the format allows and 12d does not
write - is still kept and written back but not drawn. A point symbol's rotation, offset
and raise are kept and not drawn: a Katana style has no rotation (a schema
change, deferred), and offset and raise are paper-space quantities. A
mesh is session data, so it is drawn but not saved with the project - the
same open question as a surface. A mesh's vertex and edge infos, its edge
list and its `blend` are read and not modelled. A text's slant and width factor are kept and not
drawn: Katana text has neither, and inventing them in the renderer would be
a worse lie than leaving the text upright. The undocumented parts of a
super alignment (`computator`, `floating_arc_end_radius_length` and the rest)
are read as fields and not interpreted; such an alignment arrives through its
solved geometry, which is what the manual says a reader should use. A 12da
declares no coordinate system, so nothing is reprojected.

## Reading a large 12da archive

A 62.8 MB archive of 100 000 elements took two seconds to read and now takes
four tenths of one. Nothing about the parse changed; what changed is that it
stopped building strings to throw away:

- The keyword table is a case-folded hash set asked with the token **as
  written**. Both call sites used to lower-case the token first, allocating for
  every keyword in the file.
- `detail::CaseBuffer` folds into a 64-byte inline array and spills to the heap
  only for a longer token, replacing 2.4 million heap-allocating `lowered()`
  calls per read. Twenty-two sites use it; the ones that keep the folded string
  do not, because for those the `std::string` is the point.
- Model lookup is a case-folded hash map instead of a linear scan - 20.1 million
  case-insensitive comparisons per read before. Its key is **owned rather than a
  view**: `modelNames` grows, and a short name moved by that growth takes its
  bytes with it.
- `Scope` holds a `std::string_view`. `Scope(Reader&, std::string&&)` is
  **deleted** so that a temporary cannot be borrowed by the next contributor,
  with a `const char*` overload so keyword literals stay unambiguous.

Correctness was demonstrated rather than argued: a reference build of the
pre-change sources and the new one produce byte-identical probe reports and
byte-identical rewritten archives across all six repository fixtures, three
synthetic archives, all five text encodings and nine deliberately malformed
archives - and again with the inline buffer cut to one byte, which forces the
heap-spill branch that real keywords never reach.

Two changes were **measured and rejected**; `docs/performance.md` carries the
numbers. Reserving the value vectors made a TIN-heavy read slower, not faster,
because the same code runs for many small blocks. Reserving the element vector
saves 7% but cannot be sized without a second pass over the text, and `Element`
is 1040 bytes, so a guessed constant either falls far short or wastes 30 MB on a
15 MB file.
