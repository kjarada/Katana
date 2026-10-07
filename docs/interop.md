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

GDAL's algorithm framework - the `gdal raster ...` / `gdal vector ...`
algorithms - is reached the same way, through `katana::gis::processing`
(`include/katana/gis/processing.hpp`, in `src/katana_io/geo/`). Drawing
data, surfaces and derived rasters are bound to it in `katana_interop`
(`include/katana/interop/geo/`). That, the GDAL verb and its executor are
`docs/geoprocessing.md`. Two pieces of this layer are shared with it:

- the chords an arc becomes (`src/katana_interop/curve_chords.hpp`), which
  EXPORT and the bindings use alike;
- the grid a surface is sampled on (`geo::surfaceGrid`), which
  `exportSurfaceRaster` now writes.

A reference raster says what it is to the drawing - imagery, elevation, or a
product derived by a geoprocessing run (`RasterOverlay::role`), and for the
last the line that made it (`RasterOverlay::derivation`).

The GIS analysis and check verbs - `GIS BUFFER`, `DISSOLVE`, `OVERLAY`, `HULL`,
`CLIP`, `CHECK`, `REPAIR`, `COVERAGE` and `SQL` - read the drawing through the
same conversion and apply what they make through `geo::resultCommand` as one
undo step; they are `docs/geoprocessing.md`'s "V1" onwards.

## What is supported

| Direction | Formats |
|---|---|
| Vector in | every vector format this build of GDAL reads - 77 drivers in GDAL 3.13.2: Shapefile (and a zipped one), GeoJSON, GeoPackage, KML and KMZ, GML, DXF, MapInfo TAB, SQLite, CSV with its geometry (WKT, or X, Y and Z), FlatGeobuf, GeoParquet, GPX, a file geodatabase ... (`FORMATS VECTOR READ`, "Formats", below) |
| Vector out | every one GDAL writes layers with - 44 drivers; the extension picks the writer, GDAL's own choice for the name, each written as the format needs ("Fidelity", below) |
| Raster in | every raster format GDAL reads - 143 drivers: GeoTIFF, ASCII Grid, IMG, VRT, PNG, JPEG, JP2, a GeoPackage's or an MBTiles' tiles ... |
| Point cloud | LAS, LAZ, COPC, BPF, PLY, PCD in; LAS/LAZ out, at 1 mm (audit IO-17); any of them to COPC |
| Raster out | a surface as a DEM: GeoTIFF, Esri ASCII grid, Erdas IMG (`exportSurfaceRaster`) |
| 12d Archive | .12da and .12daz in and out — every element of the format; see below |
| Where | a file, a folder GDAL reads (a `.gdb`), a `/vsi` path, a URL, a `.zip`, `.tar`, `.tgz` or `.gz` by what is inside it |

IFC is not here: `katana_ifc` reads and writes it natively, with no GDAL
(`docs/ifc.md`), and every front end routes a `.ifc` to it before any of
these. Not supported: **DWG** where GDAL has no CAD driver, and **ECW and
E57**. The MSYS2 toolchain has no ECW SDK, so GDAL has no ECW reader and
`.ecw` is not offered at all now that the extensions come from GDAL's
registry (it was offered until 2026-09-26); PDAL has no E57 plugin here, so
a `.e57` fails to open with PDAL's own error. LandXML SURVEY data - points
and observations - is read by the survey data exchange (`docs/survey.md`),
not here.

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
  (`source.ring`: `exterior` / `hole`) and the feature it came from
  (`source.part`) recorded in entity metadata. The entity model has no
  polygon-with-holes type, so the information is kept where it can be, and
  EXPORT puts the area together again: a lot with a hole goes out as one
  polygon with its hole ("Fidelity").
* **Multi-geometries** are flattened to one entity per part, each carrying a
  copy of the feature's attributes.
* The **closing vertex** of a ring is dropped; `Polyline2::closed` expresses it,
  and keeping it would create a zero-length final segment the model rejects.
* **Curves in a file** - a CircularString, a CompoundCurve, a CurvePolygon -
  become an `Arc` where the curve is one arc and a `Circle` where it is a
  whole circle; any other curve is chords by the rule below, and counted.
* **Arcs and circles** have no exact representation in most of these
  formats, so they are exported as polylines. `curveTolerance` is the sagitta — the greatest
  distance the polyline may deviate from the true curve — in model units,
  default 1 mm. The chord count follows `φ = 2·acos(1 − tolerance/r)`, so the
  result is the coarsest polyline meeting the tolerance and no finer.
* **The draw system's curves** (`docs/drawing.md`) go out by the same rule,
  through the one conversion (`docs/geoprocessing.md`, "Drawing data to
  features"): a **curve polyline** is chords within `curveTolerance`, a
  Polygon when closed, and 3D when every vertex has a height - its heights
  are its vertices' own, a chord point's linear by length along its
  segment, the rule main's EXPORT wrote them by before this branch's one
  conversion replaced its loop; an end not surveyed leaves that segment's
  chord points without one, and the string goes in plan with a warning as a
  part-heighted polyline does. An **ellipse** or a **spline** is chords, a
  Polygon when whole or closed, always in plan: neither holds a height
  (`DrawingCurves.ExportWritesEachCurveKindAndInfoReadsItBack`, a GeoPackage
  read back with INFO: Polygon, LineStringZ, Polygon, LineString). An
  IMPORT of the file brings them back as polylines; the arcs, the ellipse
  and the spline are not recovered from their chords.
* **Text and dimensions** have no counterpart at all. They are skipped, counted,
  and reported in `warnings` — never silently dropped (`docs/architecture.md`,
  "Error handling").
* **Attributes** become entity properties of their own type - integers,
  reals, booleans and text, a date as ISO 8601 text with its type kept in
  the metadata - and properties become fields of their type, a date a date;
  a key whose type differs between entities is text, and says so
  ("Fidelity").
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
* **KML, KMZ and GPX hold longitude and latitude on WGS 84 and nothing else.**
  An export to one is converted from the project's CRS and says so; with no
  project CRS it is refused (`InvalidCRS`), where GDAL's KML writer used to
  write placemarks without their geometry and report success.
* **Reprojection only when asked.** A file's declared CRS is read and
  reported, and by default not applied: an IMPORT keeps the file's
  coordinates. Moving vector data into the project's CRS is an explicit
  opt-in, `IMPORT <file> crs=project` ("Import options" below), and
  `crs=adopt` sets the project's CRS from the file when the project has none.
  Why an opt-in and not the default: a survey file is often in a CRS its
  header gets wrong or leaves out (a DXF, a CSV, a shapefile without its
  .prj), and a silent reprojection of a file whose CRS was guessed moves every
  point by up to hundreds of metres with nothing to show it happened; kept
  coordinates are at least the coordinates the surveyor wrote. The earlier
  rule - never reproject a file at all - is rejected too: it left an agent
  or a person with a GDA2020 project and a GDA94 file no way in but an
  outside tool. Data fetched from a web service, whose CRS nobody chose, is
  still always moved (GIS > Online Data, through
  `VectorImportOptions::targetCrs` and a GDAL warp, `docs/gis_online.md`),
  and so is an export to a format that holds only longitude and latitude,
  above.

## Fidelity: what IMPORT and EXPORT no longer lose

The GDAL investigation of 2026-09-26 reproduced a set of silent losses in
the vector path, each reported as a success. Each is fixed where the loss
was, and each has a test that fails without the fix
(`tests/geo/test_vector_fidelity.cpp`;
`cli.a_lot_with_a_hole_is_exported_as_one_polygon_with_the_projects_crs`,
`cli.export_of_a_projected_drawing_to_kml_is_in_longitude_and_latitude`,
`cli.export_to_kml_without_a_project_crs_is_refused`,
`qt_export_of_a_projected_drawing_to_kml_is_in_longitude_and_latitude_headless`).

**One conversion.** EXPORT reads the drawing through
`interop::geo::drawingDataset`, the conversion every geoprocessing
algorithm reads it through (`docs/geoprocessing.md`, "Bindings"), and
IMPORT makes its entities with `interop::geo::featurePieces`, the one a
result is made with. The adapter reads a layer as a typed table
(`GdalDataset::readTable`) and writes typed tables
(`GdalDataset::writeTables`); `readFeatures` and `writeVector` stay, as the
same read and write with every value as text. There was a second
entity-to-feature conversion in `export.cpp` beside the bindings', and the
losses below came about between the two.

- **A hole is a hole.** A 100 m lot with a 20 m hole, imported and exported
  to a GeoPackage, came back as two polygons summing to 10 400 m2 instead
  of 9 600: the export wrote every closed polyline as a polygon of its own.
  The conversion joins a ring IMPORT tagged as a hole to the area it lies
  in, the one of its own feature (`source.part`) when there is such, so the
  lot is one polygon again
  (`VectorFidelity.ALotWithAHoleRoundTripsThroughGpkgWith9600SquareMetres`).
  A building inside a lot is still not a hole: only a tagged ring is.
- **Types.** Every field was text both ways (`GetFieldAsString` in,
  `OFTString` out), so a sum over an imported area column was no sum.
  Fields keep their types both ways. A driver without a type gets the
  nearest one that holds every value: KML's writer has no Integer64, so an
  id that fits in 32 bits is its Integer, not its text.
- **Dates.** A property has no date type, so a Date or DateTime field is its
  ISO 8601 text, and EXPORT wrote that text as a String: the claim above
  was untrue of dates (review finding; a GPKG's `surveyed (Date)` came back
  `surveyed (String)`). IMPORT now keeps the type in the entity's metadata
  (`source.type.<key>` = `date` | `datetime`; a result keeps
  `gis.type.<key>`), and the one conversion to features gives a text
  property so tagged the field type again
  (`VectorFidelity.DatesImportedComeBackFromExportAsDates`,
  `DrawingDataset.ADateAResultMadeIsADateWhenReadAgain`). Text typed by
  hand stays text however much it looks like a date.
  - *Rejected:* recognising ISO 8601 text on export. A lot number or a
    code that happens to read `2024-05-01` would become a date without
    anyone having said it was one.
  - *Rejected:* a date type in `PropertyValue`. It is the stored form of
    every property, and a new alternative is a storage schema change for
    one field type.
  - *Not done:* a result that sets properties on existing entities
    (SetProperties) writes the text without the tag, so such a date goes
    out as text.
- **CRS.** EXPORT never wrote the project's coordinate system: every
  shapefile went without a `.prj`. Both command lines and the window now
  pass it (`document.metadata().coordinateSystem`), so the file declares
  it.
- **KML, KMZ and GPX** hold longitude and latitude only. Given MGA
  coordinates, GDAL's KML writer raised "Latitude 6250000 is invalid",
  wrote the placemark without its geometry, and the export said "exported
  1 features". The writer converts to EPSG:4326 by the one reprojection
  (`gis::reprojectFeatures`) and says it did; with no CRS it refuses. GDAL's
  KML writer converts by itself when given the CRS, and its GPX writer does
  not: one conversion of Katana's for all three was preferred to relying on
  one driver's habit. The point at 330000,6250000 in EPSG:28356 comes back
  at 151.161906846 E, 33.876653623 S, PROJ's own `cs2cs` answer.
- **KML heights.** KML's default altitude mode, clampToGround, puts a
  coordinate on the ground whatever its altitude says, and GDAL's KMZ
  writer gives a 2D coordinate an altitude of 0. So a heighted feature is
  written in the "absolute" mode, and an altitude is read as a height only
  in that mode: a KMZ's plan-only point no longer comes back at 0.
- **CSV** was written with its fields and no geometry at all. It is written
  with its geometry - WKT, or X and Y (and Z when every point has a height)
  for a table of points, which a spreadsheet reads - and the `.csvt` that
  keeps the field types. IMPORT reads a CSV's geometry columns as its
  geometry, not as properties as well, and a Z column as heights.
- **MapInfo TAB** keeps a coordinate as a 32-bit integer across the
  table's bounds, and GDAL's default bounds made that step a centimetre:
  330100 came back as 330099.99. The bounds are the data's extent, widened
  by a tenth and a unit, so the step across a 200 m site is 5e-8 m and
  across 2000 km 5e-4 m.
- **Curves and faces.** A CircularString, a TIN and a polyhedral surface
  were dropped without a word ("imported 1 entities" from a file of three).
  An arc is an `Arc`, a whole circle a `Circle`, and a line of several arcs
  chords within `VectorImportOptions::curveTolerance` (1 mm) by the chord
  rule EXPORT uses, counted in a warning; a TIN or a polyhedral surface is
  left out, counted in `VectorImportResult::skipped` and said by name. The
  adapter hands the arcs over as they are (`VectorGeometry::arcs`) rather
  than making chords itself: GDAL's `getLinearGeometry`, asked for the
  step that keeps a 1 mm sagitta on a 10 m radius, left 1.001 mm
  (measured), and katana_io cannot see the geometry layer, where the chord
  rule lives.
- **GDAL's warnings** went to the process-wide quiet handler and nowhere
  else. Each read and write collects the warnings GDAL raises on its own
  thread (`src/katana_io/cpl_error_collector.hpp`) and returns them, each
  once with a count, among the import's or export's warnings: a shapefile
  shortening `surveyed_by_party` to `surveyed_b` says so
  (`VectorFidelity.WarningsReachTheReply`). A failure GDAL raises while
  writing a feature fails the export and removes the file, even when GDAL
  itself carried on.

Every format of the export table is written and read back in one test,
geometry and fields (`VectorFidelity.RoundTripOfEveryExportDriver`): shp,
geojson, gpkg, kml, gml, dxf (its fixed fields hold the layer only), csv,
sqlite and tab.

**Not done.** The online import's reprojection moves an arc's three points
and not the arc (no web service sends arcs). A KML's own fields (`Name`,
`description`, `tessellate` ...) still arrive as properties, as they
always have. A GPX export writes points as waypoints and lines, closed
ones included, as routes; `.gpx` and `.kmz` are now written by their names
("Formats", below). `originShift` is still not
undone on export (QT-07, above). The bridge (`src/katana_io/geo/processing.cpp`)
keeps its own copy of the error collector and of the typed field reading;
both could move onto the adapter's.

## Formats: GDAL's registry, not tables kept by hand

Until 2026-09-26 three tables said what Katana read and wrote: the
extensions the import dialog offered (`vectorExtensions`,
`rasterExtensions`), the drivers EXPORT wrote with (`kVectorDrivers`, ten
extensions) and the formats the save dialog listed (`vectorExportFormats`,
six). They had drifted apart - a CSV could be written and not opened, TAB
and SQLite written from the command line and not offered by the window
(audit finding "Three parallel format tables") - they offered `.ecw`, which
this toolchain's GDAL cannot open, and they left out FlatGeobuf, GeoParquet,
KMZ, GPX and a zipped shapefile, all of which GDAL here reads and writes.
So what GDAL can do is read from GDAL (`include/katana/gis/formats.hpp`,
`src/katana_io/geo/formats.cpp`):

- **The registry.** `gis::formats()` is every driver of this build, read
  once from its driver manager: whether it holds rasters or vectors
  (`DCAP_RASTER`, `DCAP_VECTOR`), opens (`DCAP_OPEN`), writes - a vector
  writer creates layers or fields (`DCAP_CREATE` with `DCAP_CREATE_LAYER` or
  `DCAP_CREATE_FIELD`), which is what `writeTables` needs; a raster writer
  creates or copies, and a driver of both kinds writes rasters only when it
  declares the types it writes them in (a file geodatabase declares none: it
  reads a geodatabase's rasters and writes only its tables) - its extensions
  (`DMD_EXTENSIONS`, compound ones whole: `shp.zip`, `gpkg.zip`), whether it
  opens `/vsi` paths (`DCAP_VIRTUALIO`) and a connection prefix (`PG:`).
  `gis::formatOptions` is a driver's declared open, creation and
  layer-creation options, what `oo=` and `co=` will be checked against.
- **The overlay**, small and each entry with its reason in `formats.cpp`:
  drivers that are no format a person opens or saves are hidden (`MEM`,
  `DERIVED`, `HTTP`, `AIVector`, which sends the data to a remote service,
  and `GPSBabel`, which runs a program Katana does not ship); a `.xml` is
  written as GML where GDAL would pick NASA's PDS4; sidecars and generic
  extensions (`.dbf`, `.prj`, `.txt`, `.xml` ...) are not offered in a file
  dialog, since they are opened through their data file or would fill the
  filter with files that are not data; the save dialog lists the formats a
  survey or CAD office exchanges every day first, and offers KMZ beside KML
  and a zipped shapefile beside a shapefile.
- **The writer for a name is GDAL's own choice** (`gis::vectorWriterFor`,
  through `GDALGetOutputDriversForDatasetName`, the ranking GDAL's own
  tools pick an output's writer by), the first writer Katana offers taken. So a `.kml` is now written
  by LIBKML, where the hand-kept table named the older KML driver; both
  write the same KML 2.2 for a drawing, both are converted to longitude and
  latitude first ("Fidelity"), and a refusal says "KML", not the library's
  name. LIBKML declares 64-bit integers and writes them as text (KML 2.2's
  schema, section 9.5, has none), so for it an Integer64 that fits goes as
  an int. `rasterDriverForPath` stays a table on purpose: `writeRaster`
  writes a DEM's Float64 heights, and most raster writers GDAL has (PNG,
  JPEG, GIF) hold 8- or 16-bit integers.

**A file is routed by what it holds** (`interop::kindForPath`). A `.12da`
and a point cloud are known by their extensions - PDAL's, and GDAL claims
`.e57` for the images in one. Anything else that can be looked at - a local
file or folder, a `/vsi` path - is identified by GDAL
(`gis::identifyContent`, `GDALIdentifyDriverEx`, and for a driver of both
kinds the dataset opened to see which it holds): a GeoPackage of raster
tiles is a raster, a `.gdb` folder vector data, a GeoJSON named `lot.data`
vector data. Routing by extension through the registry alone was rejected:
a `.gpkg` or `.mbtiles` holds either kind, and the name cannot say which. A
path that cannot be looked at - a file not written yet, or a URL, whose
look would cost a round trip - is routed by its name: an extension only
raster readers claim is a raster, one both claim is vector data, except the
formats that are chiefly imagery (`mbtiles`, `pdf`, `jp2` ...).

**Archives and paths.**

- `GdalDataset::open` refused every `/vsi` path, URL and connection string
  as "file does not exist" (`std::filesystem::exists`). It checks only a
  local path now (`gis::isVirtualPath`); whether the rest exists is GDAL's
  to find out.
- An archive GDAL does not open as it is is opened by its inside
  (`src/katana_io/geo/formats_detail.hpp`, shared by the open and the
  routing): a `.zip`, `.kmz`, `.tar`, `.tgz` or `.tar.gz` as a folder -
  a shapefile's four files, or several shapefiles, open so as the layers of
  one dataset - else the one member that is a dataset; a `.gz` as the one
  file it compresses. An archive of several datasets is refused
  (`InvalidArgument`) naming them and the `/vsizip/{<archive>}/<member>`
  that opens one: guessing between them was rejected. `/vsigzip` takes no
  braces (measured: `/vsigzip/{<path>}` opens nothing), `/vsizip` and
  `/vsitar` do, which keeps a folder named `x.zip` from splitting the path.
- A GPX is read by GDAL as five layers, two of which (`route_points`,
  `track_points`) are the vertices of its routes and tracks again: a whole
  GPX import leaves them out and says so, and either is imported by its index.

**`FORMATS`** (`src/katana_app/geo/formats_verbs.cpp`, the verb table's I2
row) answers at prepare, changing nothing, in every front end:

```
FORMATS [RASTER|VECTOR] [READ|WRITE] [<text>...] [JSON]
FORMATS OPTIONS <driver> [JSON]
```

- one `format driver=... kind=raster,vector read=... write=... extensions=...
  vsi=yes|no description=...` record per driver, then `listed formats=<n>
  kind=... capability=... filter=... gdal=<version>`; the words keep a driver
  whose name, description or extensions hold every one (a keyword quoted is
  a word: `FORMATS "raster" tile`);
- `FORMATS OPTIONS GPKG` gives the format's record and one `option
  driver=GPKG list=open|creation|layer_creation name=... type=... default=...
  scope=... choices=a,b min= max= description=...` record per option;
- `JSON` gives the same as data. `katana_formats` (MCP) and the resource
  `katana://formats` are built from the same functions (`docs/mcp.md`).

**The window.** GIS > Processing - GDAL > Formats... (`gisFormats`) shows a
non-modal, read-only table (`gisFormatsDialog`: `gisFormatsKind`,
`gisFormatsCapability`, `gisFormatsFilter`, `gisFormatsTable`,
`gisFormatsCount`, `gisFormatsCommand`, `gisFormatsOptions`,
`gisFormatsRun`, `gisFormatsClose`; `src/katana_qt/geo/formats_dialog.hpp`).
Its choices make the FORMATS line it shows, and the table is that line's
records, prepared by the one geoprocessing executor. It does not log a
hundred records at every keystroke; Run in Command Line hands the line to
the window's executor (`MainWindow::runVerbLine`) for a person who wants it
in the log. Selecting a driver shows its options. The import dialogs'
filters are the registry's readable extensions, with a "Zipped GIS data"
filter for archives; the export dialog lists every writer, and a name typed
without an extension takes the chosen filter's.

Tests: `tests/geo/test_formats.cpp` (`Formats.*`, `FormatsVerb.*`: the
registry against GDAL's documented capabilities, the writer for each name,
FlatGeobuf, GeoParquet, KMZ, GPX and CSV written by their names and read
back to the micrometre or, in longitude and latitude, to 1e-9 degrees of
PROJ's conversion; a shapefile in a `.zip` and by `/vsizip`; a gzipped
GeoJSON and a tar built in the test to RFC 1952 and POSIX ustar; a raster
GeoPackage routed to raster import; an unknown extension routed by
content), `McpServer.FormatsReturnsStructuredDrivers`,
`qt_widgets.FormatsDialog.*`, `cli.formats_lists_writable_vector_drivers`,
`cli.import_of_a_zipped_shapefile_by_vsizip`,
`cli.import_of_a_zip_opens_the_one_dataset_inside`,
`qt_the_formats_dialog_shows_the_formats_line_it_runs_headless` and
`qt_import_of_a_zip_opens_the_one_dataset_inside_headless`.

**Not done.** `INFO` (`src/katana_interop/dataset_info.cpp`) still checks
that a file exists before GDAL looks, so it refuses a `/vsi` path that
IMPORT opens. A zipped shapefile EXPORT writes holds `katana.shp`, the
layer's name, whatever the archive is called. Routing opens a local file
once more before it is imported. A URL with no extension is identified over
the network. The options `formatOptions` lists are not yet checked when
given (that is IMPORT's and EXPORT's options work).

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
| File does not exist | `NotFound` (a local path; a `/vsi` path or URL GDAL cannot open is `FileImportFailure` with GDAL's message) |
| Extension has no writer | `Unsupported`, naming the extension |
| File no reader recognises, by content or name | `Unsupported`, naming the extension (the window names GIS > Formats) |
| Archive of several datasets | `InvalidArgument`, naming them and the `/vsizip/{...}/<member>` that opens one |
| Mixed geometry into a Shapefile | `Unsupported`, nothing written |
| Nothing matched the export filter | `InvalidArgument`, no file created |
| Raster with no georeferencing | imported, placed at the origin, **warned**; refused by Surface From Raster (`InvalidArgument`), which would otherwise build ground in the wrong place |
| Vector export with a CRS GDAL cannot read | `InvalidCRS` before anything is written (it used to be dropped and the file written with none) |
| KML, KMZ or GPX export with no project CRS | `InvalidCRS` before anything is written, naming `CRS SET` |
| GDAL says it could not write a feature, and carries on | `FileExportFailure` with GDAL's message, and the file removed (KML's "Export of geometry to KML failed") |
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
items - File > Import (any file, default options) and File > Export Vector,
as Export Drawing was then called -
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
| Vector - GDAL | Import Vector Data... | a file dialog, `describeSource` for the dialog it routes to, then the Import Vector Data dialog, whose layers, where, fields, scope, target and placement are the `IMPORT` line it runs through the window's one executor ("Import options") |
| | Export Drawing... | File's own action (`fileExportVector`, called Export Vector until it was said to write DXF, archives and IFC too): after the file dialog, the Export Drawing dialog (`vectorExportDialog`, titled as the item is), whose scope and options are the `EXPORT` line it runs ("Export options") |
| Raster - GDAL | Import Raster... | the same, to the Import Raster dialog: band, subdataset, resolution and name as the `IMPORT` line it runs |
| | Export Surface as DEM... | the `SURFACE EXPORT` line: `exportSurfaceRaster` through GDAL's `raster convert` - a tiled, compressed Float32 GeoTIFF, a COG, an Esri ASCII grid or IMG (`docs/terrain.md`) |
| Point Cloud - PDAL | Import Point Cloud... | the same, to the Import Point Cloud dialog: budget, class and a COPC resolution when the file is COPC, as the `IMPORT` line it runs |
| | Export Point Cloud... | the cloud (the Reference Data panel's selection, or asked), a question when it holds a sample, a file dialog, then the `EXPORT <file.las\|.laz> CLOUD <id>` line - `exportPointCloud`, LAS or LAZ, as a job |
| | Convert Point Cloud to COPC... | two file dialogs, then the `COPC` line - `PointCloudEngine::convertToCopc`, every point - and, when it converted, an offer to import it |
| | Dataset Information... | the `INFO` lines, through the window's one executor: `describeSource`'s records and GDAL's JSON, the gdalinfo / pdal info a person needs first ("Dataset information", below) |
| Online - Web Services | Online Data... | `interop::fetchOnlineLayer`: imagery, elevation and features from public web services, warped or reprojected into the project's CRS and taken in through `importRaster` and `importVector`; the `ONLINE` verbs do the same (`docs/gis_online.md`) |
| Processing - GDAL | Formats... | the `FORMATS` verb: every format this GDAL reads and writes, and a driver's options ("Formats", above) |

**File lists them too.** Every import and every export of this menu is also
under File > Import or File > Export, in a section named GIS - Import Vector
Data, Import Raster, Import Point Cloud and Online Data; Export Surface as
DEM and Export Point Cloud - beside File's own and Survey's
(`docs/desktop.md`, "The File menu"). They are the same `QAction` objects,
put there by object name, so the two menus cannot differ; this menu is as it
was, and Convert Point Cloud to COPC and Dataset Information, which neither
bring anything into the drawing nor take it out, are here alone. The three
imports and Dataset Information open a file dialog first: in a headless run
each is refused, naming the `IMPORT` or `INFO` line that does the same
(`qt_the_gis_items_that_ask_for_a_file_name_their_verbs_in_a_headless_run_headless`).

Decisions, and what was rejected:

* **File > Import > Import Any File stays the quick way in** for GIS files - any file, the
  default options, no questions - and a command-line argument and the `IMPORT`
  verb take the same path; it runs the `IMPORT "<file>"` line through the
  window's one executor, so the log shows it. The GIS imports are the
  considered way: they describe the file and offer its options before
  anything is read. Putting a dialog on Import Any File for every file was
  rejected: a single-layer shapefile would then cost a click for nothing,
  every time. A DXF or a .12da archive is the exception (below, "Placing an
  import"): where it lands is its one choice, and no GIS dialog offers it.
* **A dialog is built from a description of the file, not from the kind of
  menu item.** `makeImportOptions` routes by what `describeSource` finds, so a
  `.las` picked through Import Vector's "All files" still gets the point-cloud
  dialog, and the dialog offers the layers THIS GeoPackage has and a COPC
  level of detail only when the file IS COPC - the engine refuses the question
  of any other file, so offering it would be offering a failure.
* **One description.** `describeSource` is read by the import dialogs, whose
  `formatDescription` text stands above their options, and by the `INFO`
  verb of every front end, whose records Dataset Information and the
  Reference Data panel's Info button show ("Dataset information", below), so
  a file cannot be described two ways.
* **A point cloud's export says when it is a sample.** A budgeted import holds
  one point in N; Export Point Cloud asks before writing a sample as though it
  were the survey, and points at Convert to COPC for the whole file. The
  writing is the `EXPORT` verb's (`EXPORT <file.las|.laz> [CLOUD <id|name>]
  [PREVIEW]`, `src/katana_app/geo/export_verb.cpp`), so `katana_cli` and
  `katana_export`'s `cloud` write a cloud as the item does. A line has nobody
  to ask, so its reply says it instead: `sample=yes` and a `warning` naming
  COPC (`GisVerbs.ExportOfAPointCloudWritesTheCloudAsHeldAndSaysWhenItIsASample`,
  `cli.gis_export_writes_a_reference_cloud_and_says_when_it_is_a_sample`,
  `McpServer.ExportWritesAReferenceCloudByIdOrName`). A .las or .laz path is
  a cloud's, never the drawing's: no vector driver EXPORT picks writes one.
  Without `CLOUD` the one cloud there is is written, and with several the
  line is refused rather than one guessed. A `.copc.laz` is refused: COPC
  rewrites a file whole, and a sample written under that name would read as
  the survey. Headless, the item points at the line rather than open a file
  dialog nobody can close, as Convert to COPC does.
* **Surfaces use the libraries.** Surface From Raster re-reads the band's
  true values through GDAL (`readRasterElevations`) on a stride that keeps the
  whole extent under the triangulation cap (QT-24: the old stride could pass
  it threefold). Surface From Point Cloud uses `surfacePoints`, the one policy:
  the ground returns (ASPRS class 2) when the cloud has any, otherwise every
  return - and the log says which, because a surface over trees presented as
  ground is the failure QT-10 found. Both are the SURFACE FROM verb's now,
  the same on every front end (`docs/terrain.md`, "Surfaces on every front
  end"); the Surface From dialog lists the rasters and clouds and chooses the
  panel's selection first.
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
  in the window too, where it asks no placement question. `ALONGSIDE` and
  `OFFSET=dE,dN` joined it ("Placing an import", below). Both front ends read
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

## Placing an import

Survey data arrives at its own coordinates - (255440, 7410850) in MGA - and a
drawing is often somewhere else, near 0,0. Where imported data lands is one
choice with four answers, defined once in `include/katana/cad/import_placement.hpp`
(`cad::resolveImportShift`) and used by every importer and every front end:

| Placement | Typed | The move |
|---|---|---|
| Keep (the default) | `IMPORT <file>` | none; the window asks when the data lands far from the drawing |
| Local | `IMPORT <file> LOCAL` | its lower-left corner to 0,0 |
| Alongside | `IMPORT <file> ALONGSIDE` | its lower-left corner onto the drawing's; into an empty drawing, none, and it says so |
| Offset | `IMPORT <file> OFFSET=dE,dN` | by exactly dE east and dN north |

- **One reader applies one shift.** The file is read at its own coordinates,
  the move worked out from what it holds, and the file read again with that
  `originShift`, so every kind of geometry - entities, a .12da's surfaces and
  clouds - moves alike. Moving the entities afterwards was rejected: it would
  leave the surfaces behind.
- **The word is the line's last**, read by `CommandInterpreter::importArgument`
  in the window and in the session alike: never inside the quotes, never the
  only word (a file called `LOCAL` can be imported), any case. A mistyped
  `OFFSET=` is refused as `InvalidArgument` rather than taken for part of
  the path.
- **Every move is said**, in the same words everywhere
  (`ImportShift::said`): `LOCAL: moved as one piece by -180.000,0.000, so its
  lower-left corner sits at 0,0.` A raster or a point cloud refuses a
  placement by name: it is reference data drawn at its own coordinates.
- **A moved entity is in no known coordinate system, and says so.** IMPORT
  writes the shift on each entity it moved (metadata `source.shift` =
  `dE,dN`, `geo::kShiftKey`). The one conversion to features
  (`drawingDataset`) then gives no table the project's system, and warns:
  EXPORT wrote shifted coordinates into a GeoPackage labelled with the
  project's system, which put a lot imported LOCAL at 0,0 of MGA zone 56,
  off the coast of Africa (review finding). Now the file declares none, its
  `exported` record says `crs=`, and an export that would convert them (a
  KML, `crs=<code>`) is refused with `InvalidCRS`
  (`ExportOptions.EntitiesImportedMovedGoOutInNoCoordinateSystem`). A GDAL
  run given them from the drawing gets no system either.
  - *Rejected:* moving them back on export by the recorded shift. The
    entities may have been edited since, relative to where they were put;
    writing them somewhere they are not in the drawing is a second,
    silent move.
- **Alongside is the far-apart question's Shift Alongside**: the same move
  (`interop::advisePlacement`'s `suggestedShift`), which is now made by
  `resolveImportShift` in both places.

On every surface:

- **The window.** Typed, as above. GIS > Import Vector Data's dialog has a
  Placement group (`importPlacementKeep`, `importPlacementLocal`,
  `importPlacementAlongside`, `importPlacementOffset` with `importOffsetE` and
  `importOffsetN`; `importPlacementShift` says what the choice will do and the
  line that does the same), and the choice last accepted is offered next time
  (QSettings `import/placement`). File > Import of a DXF or a .12da asks the
  same group as a small step (`importPlacementDialog`) and then runs the
  `IMPORT "<file>" <word>` line it makes through the window's one executor;
  GIS > Import Vector Data given a DXF or a .12da does the same. The vector
  dialog imports itself - its layer and attribute choices have no `IMPORT`
  word - and logs the move it makes. An `IMPORT` line that keeps the data's
  coordinates asks the far-apart question through the executor's
  `Context::farApart` (below, "IMPORT, EXPORT, INFO, REFS and COPC on every
  front end"); the vector dialog's own import asks it through
  `decideImportPlacement` (`src/katana_qt/import_placement.hpp`). Both show
  the one question box, `askFarApart`. A headless window asks nothing and
  keeps the coordinates. A headless File > Import opens no file dialog: it
  names the verb.
- **katana_cli**: `IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]`, the move
  said by the `placed` record after the `imported` one; the DXF import too,
  in a build without GDAL. Giving katana_cli `LOCAL` alone and leaving the rest for later was
  considered and rejected, because a verb the window types and the CLI does
  not is the gap this work closes.
- **katana_mcp**: `katana_import` takes `placement` (`keep`, `local`,
  `alongside`, `offset`) with `offset_east` and `offset_north`; `local: true`
  still means `placement: local`, and the two disagreeing is refused.

Tests: `ImportPlacement.*` and `CadInterpreter.AnImportArgument*`
(`tests/cad/test_cad.cpp`), `Session.AlongsideAndAnOffset*`,
`McpServer.TheImportToolRefusesAPlacementItCannotSay`,
`cli.import_alongside_and_offset_place_the_data_and_say_the_move`,
`qt_widgets.ImportPlacementBox.*` and `qt_import_placement_typed_on_the_windows_command_line_headless`.

The GIS menu's vector import dialog carries the same Placement group, and
its line - with the placement word, and any options ("Import options") -
is what it runs.

## IMPORT, EXPORT, INFO, REFS and COPC on every front end

Until 2026-09-26 these verbs were written twice: once in `session.cpp`,
printing prose to stdout for `katana_cli` and `katana_mcp`, and again in
`MainWindow::dispatchLine`, logging other prose in the window. The two had
drifted - the window framed and asked, the CLI kept no surfaces - and neither
reply could be read by a program. They are now verbs of the one
geoprocessing executor (`src/katana_app/geo/`, `docs/geoprocessing.md`, "The
executor"), a file each: `import_verb.cpp`, `export_verb.cpp`,
`info_verb.cpp`, `refs_verb.cpp`, `copc_verb.cpp`. The session runs them
inline; the window's geo workbench runs them as background jobs, with
progress and Cancel in the status bar, and a headless window waits for them.

```
IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]
EXPORT <file>
INFO <file>                (INFO <id> stays the interpreter's: an entity)
REFS [LIST]
COPC <source> <destination.copc.laz>
```

### Replies

Records, one per line, as every geoprocessing verb replies
(`src/katana_app/import_records.hpp`, `src/katana_app/geo/gis_records.hpp`):

```
imported file=<path> kind=vector entities=9 layers=1 features=9 skipped=0 bounds=x0,y0,x1,y1 crs="..."
imported file=<path> kind=dxf format="DXF R2000" entities=9 layers=3 linetypes=2 bounds=...
imported file=<path> kind=archive entities= alignments= surfaces= meshes= clouds= layers= styles= encoding= version= member= bounds=
imported file=<path> kind=raster            (then its reference record)
imported file=<path> kind=pointcloud        (then its reference record)
placed placement=local|alongside|offset east=<dE> north=<dN> text="LOCAL: moved as one piece by ..."
tally element=<what> read=<n> imported=<n>
surface name= triangles= points= bounds= zmin= zmax= source=
reference id= kind=raster name= width= height= bounds= georeferenced= visible= opacity= role= file=
reference id= kind=pointcloud name= points= source_points= bounds= visible= color= file=
meshes count= triangles= held=yes|no
references rasters=<n> clouds=<n>
exported file=<path> kind=vector driver=GPKG features= skipped=
exported file=<path> kind=dxf driver=DXF format="DXF R2000" entities= skipped= layers= bytes=
exported file=<path> kind=archive driver=12da entities= skipped= alignments= surfaces= bytes=
converted source=<path> file=<path> format=copc
warning text="..."
```

- **`east` and `north` are the move**, what was added to every coordinate as
  `OFFSET=dE,dN` gives it, and empty when nothing moved (`ALONGSIDE` into an
  empty drawing). The readers subtract an origin shift; the record says the
  move, which is what a person reads the sentence for.
- **`features`** counts what the reader read, a multi-part feature once per
  part - the sample's spoil heaps, one MultiPolygon, are two.
- **The DXF records are the same in every build.** The DXF steps live in
  `src/katana_app/dxf_verbs.hpp`, split as the executor runs a line (a pure
  read or write, then the apply), and a build without GDAL runs them back to
  back through `runDxfVerb`. One DXF import, one DXF export, one reply.
- **This changed the replies.** `imported 9 entities from parcels.geojson`
  and `  extent 0,0 to 185,165` became the records above; the `cli.*`,
  `Session.*`, `GeoSession.*` and window checks that read the prose were
  rewritten for the records, and only for them - every number they expect
  is the one they expected before. Anyone scripting against the prose has to
  change; an agent reading `structuredContent` no longer parses text at all.
  `INFO`'s records are below ("Dataset information").

### Decisions

- **The window keeps what only it does, through the executor's context**
  (`geo::Context`), not a second implementation:
  - `farApart` is asked, on the GUI thread, when an `IMPORT` that keeps its
    coordinates lands far from the drawing: Shift Alongside, Keep or Cancel
    (`askFarApart`). Shift Alongside reads the file again with the shift, in
    the apply, as a typed `ALONGSIDE` would have. A session, and a headless
    window, have nobody to ask: the data keeps its coordinates and a
    `warning` record says what to type instead. The headless window used to
    log that as an error, which failed a `--command` import the CLI ran
    without complaint; it is the CLI's warning now.
  - `imported` shows what only a window can: a 12d archive's meshes, and the
    3D view for new surfaces or meshes. A session counts the meshes and says
    it holds none (`meshes ... held=no`).
  - `frame` frames what the import added; `changed` redraws the reference
    layers and surfaces.
- **A 12d archive's surfaces go into the session's store**
  (`terrain::SurfaceStore`), in the CLI and MCP too, where the session once
  said it held none. A geoprocessing line then finds them by name
  (`GDAL raster hillshade FROM SURFACE "TIN SOUTH WEST" ...`), and `EXPORT
  <file>.12da` writes them back, as the window's export always did. A name
  already taken is "name (2)".
- **`INFO <id>` is left to the interpreter by the executor itself.** The verb
  table's rows gained `takes` (`geo::takesInfo`): `INFO 12` is the entity
  unless a file of that name exists, `INFO #12` always is.
- **EXPORT and COPC write beside their target, and the apply moves the file
  into place** (`src/katana_app/geo/staged_files.hpp`). A job's work may not
  change what a person can see, and a cancelled job never applies
  (`src/katana_qt/jobs.hpp`), but the writers - GDAL's, the DXF and archive
  writers, PDAL's COPC - cannot be stopped part way. Written straight to the
  target, a cancel after the write would report a cancel that did not
  happen. So the work writes into a folder of its own beside the target, and
  the apply renames what it wrote into place; a job dropped after its write
  takes the folder with it. Rejected: a scratch folder elsewhere (the move
  would be a copy across volumes), and a scratch NAME beside the target (a
  shapefile is several files named by its stem, a writer takes its driver
  from the extension, and a `.12daz` names its member after the file).
- **EXPORT writes a copy of the drawing**, taken when the line is prepared,
  so the window stays live while a large drawing is written. The copy's
  entity observer is cleared: a copy must never report to the Document.
- **Cancel lands at the end of a read or a write.** The readers and writers
  take no stop token; a cancelled `IMPORT` imports nothing and a cancelled
  `EXPORT` or `COPC` writes nothing, but each stops only when its read or
  write ends. The status bar shows them as busy, with no measure.
- **Where the verbs are said.** HELP, the window's Command Reference and
  `katana_help` list them from the executor's table; the session's own
  "Interop" help block is gone, and so are the window's copies in
  `dispatchLine` and its archive and DXF importers. HELP (or `?`) alone,
  typed as a session line - `katana_cli -c HELP`, or `HELP` among
  `katana_run_commands` - is `Session::helpText`, the text `katana_help`
  and `--help` give. It used to reach the interpreter, whose HELP knows
  neither the executor's verbs nor IFC, which is the session's (and did not
  know CUSTOMISE, the session's too until it became the interpreter's own
  on 2026-10-06) (`cli.help_typed_as_a_line_is_the_whole_session_help`,
  `McpServer.HelpSentAsACommandIsTheWholeSessionHelp`). With a word,
  `HELP SHEETS` and `HELP UTILITY` stay the interpreter's.

Tests: `GisVerbs.*`, `GisSession.*` and `GisRecords.*`
(`tests/geo/test_gis_verbs.cpp`), `McpServer.ImportReturnsStructuredRecords`,
the `cli.gis_*` checks (`src/katana_app/geo/cli/gis_verbs.cmake`), the
window's `qt_gis_*_headless` (`tests/geo/headless/gis_verbs.cmake`) - of
which `qt_gis_import_line_gives_the_sessions_records_headless` expects the
very records `cli.gis_import_line_gives_the_records_the_window_gives` does -
and `qt_widgets.GisVerbsWindow.*`, where a cancelled import imports nothing
and a cancelled export leaves an empty folder.

Not done:

- A process that dies mid-write leaves its `.katana-staging-<pid>-<n>`
  folder beside the target.
- The window's Command Reference (`windowHelpText`,
  `src/katana_qt/command_reference_dialog.cpp`) keeps its own copies of the
  IFC and CUSTOMISE help blocks rather than reading `ifcHelpText` and the
  interpreter's, so an edit to one is not seen in the other. (The session
  has no CUSTOMISE block of its own any more: the interpreter's help has the
  family's.)

The import and export dialogs build `IMPORT` and `EXPORT` lines ("Import
options", "Export options"); neither writes through `interop` itself any
more.

## Import options

`IMPORT` grew words that say what of a file is read and how, read by one
parser in `src/katana_app/geo/import_verb.cpp` (the contract's grammar D):

```
IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]
       vector data:  [layers=a,b] [where="<OGR SQL WHERE>"] [sql="<SELECT>"] [dialect=ogrsql|sqlite]
                     [<scope> [clip]] [fields=a,b] [attributes=no] [target=<layer>] [max=N]
                     [oo=KEY=VALUE]...
       every kind:   [crs=project|adopt] [srs=<code>] [PREVIEW]
       a raster:     [band=N] [subdataset=N|name] [maxpixels=N] [name=<n>]
       a cloud:      [budget=N] [class=N] [resolution=<m>] [name=<n>]
```

- **`where=` and the scope go to the driver**, not through Katana after a
  full read: `GdalDataset::readTable` sets `OGRLayer::SetAttributeFilter` and
  `SetSpatialFilterRect` (`gis::VectorReadOptions::attributeFilter`,
  `spatialFilter`), so a GeoPackage answers from its SQLite and its R-tree,
  and a feature outside is never read. The filters are cleared after the
  read, so a dataset read twice is not narrowed the second time. A filter
  GDAL cannot parse is refused with GDAL's reason - never read as "nothing
  matches".
- **`sql=`** runs a SELECT on the dataset (`GdalDataset::readSql`,
  `GDALDataset::ExecuteSQL`) and imports its rows as one layer named after
  the file. Only `SELECT` and `WITH` are read: an import never changes its
  file. It is refused with `layers=`. The statement is one word of the line,
  so it holds no double quote; SQLite reads `[identifier]` alike, as GIS SQL
  takes it (`vector::sqlForLine`).
- **The scope is the shared one** (`cad::parseScopeWords`, resolved by
  `cad::matchScope`): AREA's box, a VIEW's visible area, else the extent of
  what SELECTION, DRAWING or LAYERS takes - in the drawing's coordinates,
  which are the file's, or the project's with `crs=project`, when the box
  is moved into each layer's CRS first (`gis::transformBox`, densified, so
  the box the driver filters by encloses the one asked for). The reply says
  what it took: `scope scope=area area=... matched=0 box=... clip=no`. A
  scope that takes nothing reads nothing and says so (`import ... ran=no`),
  not an error.
- **`clip` cuts** at the box (AREA, a VIEW's area: `vector clip --bbox`) or
  by the closed shapes the scope takes (`vector clip --like`, which clips to
  their geometry, not their bounds), through the one bridge, after any
  reprojection. A scope that takes no closed shape is refused for `clip`.
  Rejected: Katana's own clipping of rings, a second implementation of what
  GIS CLIP already runs through GDAL.
- **A scope with a placement is refused.** The scope is in the drawing's
  coordinates, which LOCAL, ALONGSIDE and OFFSET move the data away from;
  reading "what lies in this box" and then moving it elsewhere would say
  two things at once.
- **`oo=` is checked against the driver's own list** (`gis::checkOptions`,
  from `GDAL_DMD_OPENOPTIONLIST`): a key it does not declare, or a
  string-select value outside its choices, is refused naming the keys it
  has. GDAL itself only warns and reads on without the option, which looked
  as if the option had worked. The same check serves EXPORT's `co=` and
  `lco=`.
- **`crs=project`** moves vector data into the project's coordinate system
  (`VectorImportOptions::targetCrs`); refused, naming `CRS SET`, when the
  project has none, and for a raster or a point cloud, which are drawn at
  their own coordinates (RASTER REPROJECT makes a moved copy). **`srs=`** is
  the CRS a vector file's coordinates are in, whatever its layers declare,
  as ogr2ogr's `-s_srs`: used as the source for `crs=project`, and recorded
  as the file's otherwise; a layer that declares another system is said in
  a warning (`VectorImportOptions::sourceCrs`). It was the CRS of a layer
  that declares none, which a GeoJSON without a `crs` member never is:
  GDAL declares it WGS 84 (RFC 7946 section 4), so `srs=EPSG:28356
  crs=project` moved MGA metres as degrees and drew the lots millions of
  metres away (review finding;
  `ImportOptions.SrsSaysWhatAGeoJsonWithoutACrsMemberIsIn`). The online
  import keeps the fallback it needs (`assumedSourceCrs`, WGS 84 for a
  service's layer that declares none). For a raster, `srs=` still only
  records a system on one that declares none.
- **A file in another system than the project's is said.** Imported
  without `crs=project`, a vector file, a raster or a point cloud known to
  be in another system (`gis::sameCrs`) is drawn at its own coordinates,
  which do not register against the drawing's; the reply says so in a
  warning naming both (`ImportOptions.AFileInAnotherCrsThanTheProjectsIsSaidToBe`).
  `crs=adopt` that set the project's from the file's says nothing.
- **`crs=project` is checked again at the apply.** The data is moved on the
  worker into the project's system as it was at prepare; a `CRS SET` while
  the import ran would draw it in the wrong one, so the apply refuses with
  `InvalidState` and adds nothing
  (`ImportOptions.CrsProjectRefusesWhenTheProjectsCrsChangedWhileTheImportRan`). **`crs=adopt`**
  sets the project's CRS from the file's when the project has none - in the
  SAME undo step as the entities (`Document::coordinateSystemCommand`), so
  one Undo takes both away - and does nothing, saying why
  (`crs adopted=no reason=...`), when the project has one already: changing
  a project's CRS is `CRS SET`'s decision, not an import's.
- **A raster's `band=`** shows that band alone as grey, stretched over its
  own range (or through its colour table), whatever the file says its bands
  make (`GdalDataset::readImage` with a band); **`subdataset=`** opens a
  dataset inside a container by its number in INFO's list or by its name.
- **A point cloud's `class=`** is one ASPRS class: the reader filters by one
  (`PointCloudReadOptions::classification`); a list is refused rather than
  read as its first. With `budget=`, the step is sized to the class: the
  filter runs before the decimation, and a step sized to the whole file
  kept 738 of 1000 for class 2 of `samples/gis/survey_scan.las` (29512 of
  its 40000 points, counted from the point records). No LAS header counts
  points by class, so the first read measures it - at most kept x step
  points passed the filter - and a second read at the step that sizes gives
  984 (`ImportOptions.ABudgetWithAClassIsSizedToThePointsOfThatClass`).
  Reading the file once more to count the class exactly was rejected: the
  measured bound already keeps within the budget, and costs the same second
  read only when it helps.
- **An option of another kind of data is refused by name** - `band=` on a
  shapefile, `where=` on a raster - rather than ignored and seeming to have
  worked.
- **PREVIEW** reads with every filter and replies
  `import file=... kind=vector preview=yes features=N of=M entities=E layers=L`
  (after the scope record), importing nothing; for a raster or a cloud,
  which have no filters to count, it answers without reading. `of` is the
  features the layers read hold before any filter (`featuresInFile`).
- **What the filters took is said**: an import with `layers=`, `where=`,
  `sql=` or a scope adds `matched features=N of=M` after its `imported`
  record, and a filter that takes nothing imports nothing with a warning -
  an answer, where a file that yields nothing unfiltered is still refused.
- **The path** is the first word: quoted, or unquoted up to the first
  option, flag, placement or scope word, so a path with a blank still reads
  without quotes. With nothing after it but a placement, the line is read by
  `CommandInterpreter::importArgument` exactly as before.

**The window.** GIS > Import Vector Data, Import Raster and Import Point
Cloud (`src/katana_qt/gis_import_dialogs.hpp`) are built from the file's
description and show the `IMPORT` line their fields make (`<d>Command`);
Import hands that line to `MainWindow::runVerbLine`, so the dialog does
nothing an agent cannot. The vector dialog's scope is the shared "Apply to"
and "Only those that match" widget (`vectorImportScope`), off until
`vectorImportUseScope` is ticked; its Preview runs the line with PREVIEW and
shows the count in `vectorImportMatchCount`. The fields that were
`importSourceLayer`, `importTargetLayer` and `importAttributes` are
`vectorImportLayers` (every layer ticked is no `layers=` word),
`vectorImportTarget` and `vectorImportAttributes`, named as every GIS
dialog's are. The window's own importVectorFile, importRasterFile and
importPointCloudFile, the second path that imported through `interop`
directly, are gone.

**MCP.** `katana_import` takes the options as arguments and builds the line
(`docs/mcp.md`, "I3 and I4").

Tests: `ImportOptions.*` and `RasterBands.*`
(`tests/geo/test_import_options.cpp`), the `cli.import_*` checks of
`src/katana_app/geo/cli/import_options.cmake`,
`McpServer.ImportTakesItsFilterScopeAndPreviewArgumentsAsTheWordsAPersonTypes`,
and `qt_widgets.VectorImportDialog.TheFieldsBuildExactlyTheTypedLine` with
its neighbours (`tests/qt_widgets/geo/test_import_dialogs.cpp`).

Not done:

- A raster's `band=` and `subdataset=` are not recorded with the reference
  layer, so a project reopened (REFS RESTORE) shows the file as it is
  without them.
- `srs=` for a raster only records the CRS on the reference layer; nothing
  is moved.
- No test reads a real container's subdataset: no fixture holds one (a
  netCDF would); `ImportOptions.ASubdatasetOfAFileWithNoneIsRefused` covers
  the refusal only.
- The scope for `sql=` with `crs=project` is moved into the first layer's
  CRS: a statement over layers in different systems is filtered in that
  one.
- A point cloud takes no scope; `PointCloudImportOptions::clip` exists and
  is not yet given a word.

## Export options

`EXPORT` takes the shared scope and filter, and the options GDAL's formats
have, read by one parser in `src/katana_app/geo/export_verb.cpp`:

```
EXPORT <file> [<scope>] [layername=<n> | split=layer] [append]
       [crs=project|native|<code>] [co=KEY=VALUE]... [lco=KEY=VALUE]...
       [text=points|skip] [curve=<m>] [properties=yes|no] [PREVIEW]
```

- **The scope is the one grammar** (`cad::parseScopeWords`, resolved by
  `cad::matchScope`): no scope words is the whole drawing, as `EXPORT`
  always was. `WHERE` alone is the parser's own rule, `MODIFY`'s: the
  selection, filtered; `DRAWING WHERE ...` filters the whole drawing, and the
  `scope` record says which was taken
  (`cli.export_where_alone_filters_the_selection_and_drawing_where_the_drawing`).
  `katana_export`'s `where` without `scope` means the same and writes
  `SELECTION WHERE ...`; it was refused, while a typed line took it
  (`McpServer.WhereWithoutAScopeFiltersTheSelectionAsTheLineDoes`). Making
  `WHERE` alone the drawing for `EXPORT` was rejected: it is one parser's
  rule for every verb, and `MODIFY WHERE ...` acting on the drawing instead
  of the selection would change what an edit touches. Refusing `WHERE` alone
  everywhere was rejected for the same reason. The reply says what it took - a `scope` record after the
  `exported` one, which stays first, as a reader of the first record found
  it - and a scope that takes nothing writes nothing and says so
  (`export ... ran=no`), not an error.
- **The `exported` record's `crs=` is what the file is in**
  (`VectorExportResult::projectionWkt`): the project's, `crs=<code>`'s,
  WGS 84 for KML, KMZ and GPX, which the writer converts to whatever the
  drawing is in, and none for entities imported moved. It named the
  project's system for a KML whose coordinates were longitude and latitude
  (review finding;
  `ExportOptions.AKmlSaysItIsInLongitudeAndLatitudeNotTheProjects`).
- **A .dxf and a .12da take the scope too**, through a copy of the drawing
  that keeps only what the scope took: the native writers take a model, and
  the copy is already made for the worker. Rejected: threading the scope's
  entity list through `writeDxfExport` and the archive writer, a second
  selection mechanism beside the copy. An archive carries the session's
  surfaces with the whole drawing (no scope, or DRAWING with no filter) and
  not with a part of it, as the window's export always did. GDAL's options
  on a .dxf or a .12da are refused by name.
- **`layername=`, not `layer=`.** An option key is never a WHERE key:
  `WHERE ... LAYER=` would take `layer=<n>` for a filter
  (`src/katana_app/geo/vector_support.hpp`). The plan's grammar said
  `layer=`.
- **`split=layer`** writes one file layer per drawing layer, named after it,
  in the order the layers are first met; refused with `layername=`, and for
  GPX, whose layers are its own.
- **`append`** adds the layers to the file (a GeoPackage) rather than
  replacing it (`gis::VectorExportOptions::append`): a layer name the file
  has is refused before anything is written, a format that adds no layer to
  an existing file is refused, and a failure part-way deletes only the
  layers it made. The write is to a copy of the file in the staging folder
  (`StagedFiles`), which the apply puts in place of it, so a cancelled
  append leaves the file as it was. `co=` with `append` is refused: they are
  the options of a new file.
- **`co=` and `lco=` are checked** against the driver's creation and layer
  creation lists (`gis::checkOptions`), as IMPORT's `oo=` is: GDAL only
  warns of an unknown one.
- **`crs=`.** The project's coordinate system is written by default
  (`crs=project`), as it has been since the fidelity work. A code moves the
  coordinates into that system on the way out
  (`VectorExportOptions::targetCrs`, by the one table reprojection IMPORT's
  `crs=project` uses);
  refused without a project CRS to move from. **A GeoJSON is written in
  longitude and latitude by default**, converted from the project's, with a
  warning saying so: RFC 7946 section 4 fixes GeoJSON's coordinates to WGS 84
  longitude and latitude, and GDAL's writer, given projected coordinates,
  writes them with a `crs` member most readers ignore - so they were read as
  degrees. `crs=native` keeps the project's system where the format allows
  it (a GeoJSON with its `crs` member, JSON-FG). KML, KMZ and GPX were
  already converted ("Fidelity"). The default changed for GeoJSON only; the
  `interop::exportVector` API writes coordinates as they are unless asked.
- **`text=points`** writes a text entity as a point at its insertion point,
  with `text`, `text_height` (model units) and `text_rotation` (degrees
  anticlockwise) fields and an `OGR_STYLE` field whose `LABEL(t:"...",s:<h>g,
  a:<deg>)` the writer also sets as the feature's style string, which KML,
  DXF and MapInfo draw by. `text=skip`, the default, leaves text out and
  counts it, as before. The reply's `texts=` says how many went.
- **`curve=`** is the chords' largest stray from an arc, and
  **`properties=no`** writes no entity properties - the old dialog's two
  choices, as words.
- **PREVIEW** answers at once:
  `export file=... kind=vector driver=GPKG preview=yes entities=<n>` and the
  scope record, and writes nothing.
- The `exported` record of a GDAL format gained `layers=` (the file layers
  written), `crs=` (the system written), `texts=` with `text=points` and
  `append=yes` with `append`, after its old fields.

**The window.** File > Export > Export Drawing (`fileExportVector`; the GIS
menu shows the same action) asks for the file, then opens the Export Drawing
dialog
(`src/katana_qt/gis_export_dialog.hpp`), built on the GIS dialog frame: the
shared "Apply to" and "Only those that match" controls (`vectorExportScope`,
starting at the selection when there is one, else the whole drawing), the
option fields (`vectorExportLayerName`, `vectorExportSplit`,
`vectorExportAppend`, `vectorExportCrs`, `vectorExportCreationOptions`,
`vectorExportLayerOptions`, `vectorExportText`, `vectorExportCurve`,
`vectorExportProperties`), and `vectorExportCommand`, `vectorExportPreview`
and `vectorExportRun`, which hand the line to the one executor. The GDAL
fields are off for a .dxf or a .12da. The window's own exportDrawingTo and
its native DXF export (main_window_dxf.cpp), the second path that wrote
through `interop` directly and asked "selected only?" in a message box, are
gone. A headless run still refuses the item's file dialog and names the
`EXPORT` line, which is the non-interactive path.

**MCP.** `katana_export` takes the scope and the options as arguments
(`docs/mcp.md`, "I3 and I4").

Tests: `ExportOptions.*` (`tests/geo/test_export_options.cpp`), the
`cli.export_*` checks of `src/katana_app/geo/cli/export_options.cmake`,
`McpServer.ExportTakesTheSharedScopeAndItsOptionsAsTheWordsAPersonTypes`,
and `qt_widgets.VectorExportDialog.TheFieldsBuildExactlyTheTypedLineAndRunHandsItOver`
with `VectorExportLine.*` (`tests/qt_widgets/geo/test_export_dialog.cpp`).
The expected coordinates of the conversions are PROJ's `cs2cs` figures for
330000,6250000 in EPSG:28356, as `tests/geo/test_vector_fidelity.cpp`
records them.

Not done:

- JSON-FG is written only when the file's name picks its driver; EXPORT has
  no word for a driver other than the extension's, so `crs=native` is shown
  with a GeoJSON.
- `text=points` writes one point per text; a multi-line text's lines are one
  field, its line breaks blanks in the style's label.
- The archive writer and the DXF writer take no `co=`/`lco=`; they have no
  such options.

## Dataset information

```
INFO <file|folder|url> [JSON] [STATS] [CHECK] [LAYER <name>]
```

What a file holds, read without importing it, as records an agent can act
on - which field to filter on, which band, which layer - or as GDAL's own
JSON:

```
dataset file=<path> kind=raster|vector|raster,vector|pointcloud|folder driver=AAIGrid crs="WGS 84 / UTM zone 30N (EPSG:32630)"
raster width=120 height=90 bands=1 georeferenced=yes cell=1.5,1.5 bounds=-5,-5,175,130
band band=1 type=Float32 nodata=-9999 min= max= mean= stddev= color=Undefined overviews=0
overview band=1 index=1 width= height=
subdataset index=1 name="NETCDF:..." description="..."
layer name=lots features=3 geometry=Polygon crs="..." bounds=-10,0,110,40 fields=2
field layer=lots name=kind type=String subtype= width= precision=
pointcloud points=40000 bounds=... zmin= zmax= color=no copc=no
found file=<path> driver=GeoJSON has_crs=yes            (a folder: one per dataset)
check code=0 problems=0                                  (CHECK)
problem text="..."
```

- **One reading.** `interop::describeSource` describes a GDAL dataset from
  GDAL's own `raster info` and `vector info` JSON, run through the
  geoprocessing bridge, and keeps that JSON beside the description. The
  records are read from it and `INFO ... JSON` gives it back as it is
  (`{"path", "raster", "vector", "multidim"}`, GDAL's key order kept), so the
  two cannot disagree. The adapter's own reading of a dataset for this was
  replaced: two readers of one file would drift. The import dialogs and
  `formatDescription` read the same description.
- **Both readers are asked.** One file can hold rasters and vector layers (a
  GeoPackage), so `raster info` and `vector info` are both run; what
  neither reads is refused in the words of the reader its kind suggests.
  `mdim info` is asked, for JSON, of the multidimensional formats this GDAL
  has (netCDF, HDF4, HDF5, GRIB, BAG, S-102, S-104, S-111, Zarr, CPHD),
  since it fails for every classic raster.
- **Absent is not zero.** A band's statistics are empty until computed;
  `STATS` computes them from every pixel. GDAL's JSON prints them to three
  decimals, so the `STATISTICS_` metadata beside them, 14 significant
  digits, is read first (measured: `"mean":30.341` against
  `STATISTICS_MEAN 30.341263614231` for the sample terrain). The bridge puts
  back whatever `.aux.xml` the computation would have left beside the file
  (`docs/geoprocessing.md`, "Sidecars"), so `STATS` writes nothing there.
- **CHECK** is GDAL's `dataset check`: every value read, its return code and
  what failed, as `problem` records. A file GDAL cannot open at all fails
  the line instead.
- **A folder** is GDAL's `dataset identify`, recursive and detailed, and the
  point clouds in it, which GDAL does not read (PDAL describes them). An
  OpenFileGDB `.gdb` is a folder that GDAL opens as one dataset, and it is
  described as one.
- **/vsi paths and URLs** are GDAL's to find: `describeSource` no longer
  refuses `/vsizip/a.zip/b.geojson` as a missing file before GDAL is asked,
  and INFO reads any format GDAL reads (`DescribeOptions::anyFormat`), where
  an import dialog asks only of what Katana imports.
- **The path**, as INFO has always read it, is one quoted word or the
  unquoted words before the first keyword; a quoted path followed by a word
  INFO does not know is refused.

**GIS > Dataset Information** (`datasetInfoDialog`, and the Reference Data
panel's Info button) builds `INFO "<file>"` and `INFO "<file>" JSON` and runs
them through the window's one executor, then shows the records: the Summary
(`datasetInfoSummary`), the Fields (`datasetInfoFields`) and Bands
(`datasetInfoBands`) tables, and GDAL's JSON indented (`datasetInfoJson`,
`datasetInfoCopyJson`). Compute Statistics (`datasetInfoStats`) and Check
(`datasetInfoCheck`) run `INFO ... STATS` and `INFO ... CHECK`;
`datasetInfoCommand` shows the line last run and `datasetInfoReply` what it
said. A band's statistics, once computed, stay in the Bands table when a
later reply - Check's - gives none for the same file: Check blanked them
(`DatasetInfoDialog.StatisticsAndCheckAreTheirLines`). The reply and the
import dialogs' status lines are plain text: GDAL's words are shown as they
are, where a `<code>` in one was taken for markup and vanished
(`VectorImportDialog.AMessageIsShownAsTypedNotAsMarkup`). INFO runs as a background job in the window, so the dialog is told when
the job ends (`MainWindow::awaitJob`, which reads the `job id=<n> ...
state=started` record the line answered with). MCP: `katana_dataset_info`
(`docs/mcp.md`).

Tests: `InfoVerb.*` (`tests/geo/test_info_verb.cpp`) - the terrain's
statistics against the grid's own values read from the text, within the
half ulp of Float32 GDAL reads it as, and no sidecar; typed GeoJSON fields;
a folder; CHECK; a `/vsizip` path; a netCDF's multidimensional JSON;
`DatasetInfo.TheFieldsAndBandsAreReadFromGdalsOwnDescription` and
`DatasetInfo.AVsiPathIsGdalsToFindAndIsDescribed`;
`McpServer.DatasetInfoReturnsRecordsAndGdalsJson`; `cli.info_*`;
`qt_widgets.DatasetInfoDialog.*`;
`qt_info_typed_and_run_as_a_dialog_runs_it_gives_the_records_headless` and
`qt_dataset_info_headless`.

Not done: INFO says nothing of a raster's metadata domains or histogram
(GDAL's JSON has them, under `JSON`), and lists a folder's datasets without
describing each.

## Reference layers

```
REFS [LIST] [JSON]
REFS SHOW|HIDE|REMOVE|INFO <ref>
REFS OPACITY <ref> <0..1>                     (a raster's)
REFS COLOR <ref> elevation|intensity|classification|rgb|flat   (a point cloud's; COLOUR too)
REFS RENAME <ref> <name>
REFS OVERVIEWS <ref> [levels=2,4,8] CONFIRM   (a raster's)
REFS RESTORE
<ref> := a layer's id, or its name in any case
```

```
reference id=1 kind=raster name=terrain width=120 height=90 bounds=... georeferenced=yes visible=yes opacity=1 role=imagery display=plain file=...
reference id=2 kind=pointcloud name=scan points=40000 source_points=40000 bounds=... visible=yes color=elevation file=...
missing name=ortho kind=raster file=...
references rasters=1 clouds=1 missing=0
removed id=2 kind=pointcloud name=scan
source file=... url=... licence=... attribution=... crs=...        (INFO)
derivation line="GDAL raster hillshade ..."                        (INFO of a derived raster)
overviews id=1 file=.../terrain.asc.ovr count=2 levels=2,4         (then an overview record each)
restored layers=2 missing=0                                        (RESTORE)
```

- **The project keeps them.** The header of `reference_data.hpp` always said
  the project records each layer's source and display settings and reads
  the pixels again on opening; nothing did. Now a save records every layer
  (`interop::referenceRecords`: kind, name, source, visibility, a raster's
  opacity, role, display style, derivation, web source and licence and the
  display copy's size, a cloud's colouring, point size and the points it
  held), and opening the project runs `REFS RESTORE`, which reads each
  layer again from its source through the one executor - a job in the
  window - with its recorded name and settings. A cloud that came in with
  a 12d archive is read again from the archive. How the records are kept
  is `docs/model.md`'s ("Metadata a newer build wrote"): a key, not a
  schema change.
- **Taken at the save.** A layer imported, hidden or restyled does not by
  itself make the drawing ask to be saved: reference data was session data,
  and File > New after an import still simply drops it (audit QT-17). Both
  front ends record the layers where they record the customisation, before
  a save that has somewhere to go (`geo::recordReferences`). `NEW` and
  `OPEN` in `katana_cli` and `katana_mcp` now let the layers and surfaces go
  with their drawing, as the window's always did.
- **Where a source is.** A record keeps the source absolute, resolved when
  it is recorded: `IMPORT terrain.asc` typed in katana_cli's working folder
  was kept as `terrain.asc`, and an OPEN from any other folder found nothing
  (`RefsSession.ALayerImportedByARelativePathReopensFromAnotherFolder`). A
  source inside the project's folder is also kept relative to it
  (`in_project`, a key added to the record, not a new version), and an
  opening whose absolute source is gone looks for it there, so a project
  moved or copied with its data keeps its layers
  (`RefsSession.ALayerInsideTheProjectFollowsTheProjectWhenItMoves`). A
  relative source a record of an older build holds is looked for in the
  project, then in the working folder as before. Keeping only the
  project-relative path was rejected: File > Save As records before it
  knows the new folder, so a path relative to the old one could name
  nothing; the absolute path is read first and never depends on it.
- **A missing file is warned of, not fatal.** The drawing opens; the
  restore says which source is gone and keeps that layer's record, listed as
  `missing`, so the next save does not drop a layer because a drive was not
  connected that day. `REFS REMOVE <name>` lets it go.
- **Overviews only when told.** `REFS OVERVIEWS` runs GDAL's `raster
  overview add --external` on the raster's file, writing `<file>.ovr` beside
  it - which Katana otherwise never does to a person's file
  (`docs/geoprocessing.md`, "Safety") - so the line must say `CONFIRM`,
  `katana_references` must be given `confirm: true`, and the window asks
  (headless, nobody can, so its line goes without and is refused, saying
  what it would write). The overviews make GDAL's reads of a large raster
  faster; the display copy is not re-read at view resolution.
- **By id or by name.** A name several layers share is refused, naming
  their ids.

**The window.** The Reference Data panel builds REFS lines and runs them
through the window's one executor, each logged as typed: the list
(`referenceList`, its check boxes `REFS SHOW|HIDE`), Show (`referenceShow`),
Hide (`referenceHide`), Information (`referenceInfo`: `REFS INFO`, then the
Dataset Information dialog on the layer's file), Build Overviews
(`referenceOverviews`), Remove (`referenceRemove`), and below the list the
selected layer's Opacity (`referenceOpacity`) and Colour
(`referenceColour`). The per-row controls it had were replaced by these: a
control in a cell has no name a test or an agent can reach, and a line's
reply rebuilds the table the cell is in.

**MCP**: `katana_references` (`docs/mcp.md`).

Tests: `RefsVerbs.*` and `RefsSession.ASavedProjectReopensWithItsReferenceRasters`
(`tests/geo/test_refs_verbs.cpp`) - visibility round trip, opacity bounds,
colour, rename and remove by name, JSON and INFO, overviews built only with
CONFIRM (120 x 90 halved is 60 x 45, quartered 30 x 23 - GDAL rounds up),
every display setting through a record, a derived raster's line kept, a
missing source kept for the next save, an archive's clouds read again;
`ProjectStoreRoundTrip.TheReferenceLayersAProjectRecordsRoundTripAsTheyWere`
and `...FromBeforeReferenceLayersWereRecordedOpensWithNone`; `cli.refs_*`;
`McpServer.ReferencesActsOnALayerByIdOrNameAndListsThemAfter`;
`qt_reference_dock_builds_refs_lines_headless`.

Not done: a source outside the project is found only at the same absolute
place, so a project moved to another machine without it lists it as
missing (warned of and kept); the display copy is one decimated read, not
re-read at view resolution; a web layer whose cached file is gone is not
fetched again (`ONLINE IMPORT` does).

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

The same directory holds the readers of the older CUSTOMISATION formats - the
`.4d` linestyle and symbol libraries and the survey code file (`.mapfile`),
their writers and the loader that tells them apart by content - under
`src/katana_archive12d/legacy/`, as a library of its own,
`katana_legacy_customisation`, which only the converter and its tests link.
None of that is the archive format, and none of it is in the product:
`katana_archive12d` (what `katana_interop`, `katana_app` and `katana_qt` link)
holds the archive only, and a test fails if the legacy library gets into
`katana`, `katana_cli` or `katana_mcp`. The merge that loads one customisation
on top of another is `cad::mergeCustomisation`, over the Katana format. What
the older formats hold is recorded in `docs/survey_coding.md`, "The legacy
formats the converter reads".

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
