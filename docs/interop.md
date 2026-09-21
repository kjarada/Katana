# Interoperability — GIS, rasters and point clouds

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

Not supported: **DWG, LandXML, IFC**, and DXF *import* (export only, via GDAL).

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

