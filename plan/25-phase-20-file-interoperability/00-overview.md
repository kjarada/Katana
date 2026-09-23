<!-- Katana plan, section 25 of 47, lead text before its subsections. Index: ../../PLAN.MD. Previous: ../24-phase-19-performance-architecture.md. Next: 01-20-1-where-imported-data-lands.md -->

# 25. Phase 20 — File Interoperability

**STATUS: PARTIALLY DELIVERED.**

`katana_interop` converts external data to and from the domain model, and is the
only layer that may see both the external readers and the entity model - which
is what keeps GDAL and PDAL out of `katana_cad`, so the core still builds with
`-DKATANA_BUILD_IO=OFF`.

Working, in the desktop application (File > Import / Export Vector, the GIS
menu and toolbar, and a Reference Data panel) and in `katana_cli` (`IMPORT`,
`EXPORT`, `REFS`, `INFO`, `COPC`):

| Direction | Formats |
|---|---|
| Vector in | Shapefile, GeoJSON, GeoPackage, KML, GML, DXF, MapInfo |
| Vector out | the same, driver chosen by extension |
| Raster in | GeoTIFF, ASCII Grid, IMG, VRT, PNG/JPEG and the rest of GDAL's readers |
| Point cloud | LAS, LAZ in and out |
| 12d Archive | `.12da`, `.12daz` in and out - every element of the format (`.12dz` was withdrawn in 20.2 slice 1) |

Vector import produces plain `Entity` values which the caller wraps in
`createEntities`, so an import is one validated, atomic, undoable command like
any other edit - including the layers it needs, in a single `Transaction`.
Multi-geometries are flattened to one entity per part, polygon rings become
closed polylines, and a two-point LineString becomes a Line rather than a
polyline so it can still be filleted. A file's Z becomes the entity's heights
(the `elevation` / `elevations` properties) and goes back out as Z; a 2D
feature has no height, not a height of 0. CORRECTED 2026-09-23 (audit IO-01):
import dropped Z and export wrote Z = 0 until then; what a format cannot carry
is reported, and the rules are in `docs/interop.md`, "Conversion".

Raster and point cloud import produce REFERENCE DATA instead: backdrop that is
drawn, not drawn ON. It is deliberately outside the entity model and outside
undo - putting a 400-megapixel orthophoto through before-image undo for a
visibility toggle would be absurd - and it is held by the application layer, not
the Document.

Lossy conversions are stated rather than hidden. Arcs and circles have no exact
representation in these formats, so they are written as polylines at an explicit
`curveTolerance` (the sagitta, in model units). Text and dimension entities have
no counterpart at all: they are skipped, counted and reported, never silently
dropped. A Shapefile holds one geometry type per file, so a mixed selection is
refused BEFORE anything is written rather than failing halfway and leaving a
partial file. GeoJSON always declares WGS 84, so exporting projected coordinates
to it warns that a reader will take them for degrees.

**CORRECTED: this paragraph had DXF backwards.** It said "DXF is export only".
DXF IMPORT has worked all along, through the GDAL driver - a 51 000-feature
drawing was imported in testing. DXF EXPORT is what had never worked, for two
independent reasons found by running the bundled CLI rather than by any test:
GDAL could not find `header.dxf` (it has no usable compiled-in data path on
MSYS2, and only an MSYS2 login shell sets `GDAL_DATA`), and the exporter
treated "DXF does not support arbitrary fields" as fatal. Both are fixed -
GDAL's data is now located relative to the GDAL DLL, which is right in the
build tree and in a bundle alike - and DXF import now puts each entity on its
own CAD layer instead of all 51 000 on one layer called `entities`. Record in
`docs/interop.md`.

**The 12d Archive format is delivered** (`katana_archive12d`, a module of
its own beside `commands` so that it builds and sanitizes without GDAL;
`interop/archive12d.hpp` for the file and the zip container; File > Import /
Export and `IMPORT`/`EXPORT` in the CLI). Every element the 12d Model V15
manual defines is read - the current string types, the seven superseded ones,
tins in both forms, super tins, trimeshes, LAS clouds in all eleven point
formats - and `coverage.hpp` holds the list against the reader in a test, so
"all elements accounted for" is asserted rather than claimed. Strings become
entities with their heights, attributes and everything 12d knows about them
in metadata; super alignments become named Alignments where their geometry is
PI-definable and VERIFIED against 12d's own solved vertices; tins become
surfaces; clouds become reference layers; trimeshes become session meshes
(20.2 slice 4). Export writes entities, alignments and the
session's surfaces back, and what was imported and exported unchanged is the
string it was. Three conventions the manual gets wrong or leaves out - the
hand of a radius, the description of a trailing transition, the definition of
the cubic parabola - were measured against 12d Model's output and are
recorded in `src/katana_archive12d/plan_geometry.hpp`; the rest of the
reasoning is in `docs/interop.md`. An adversarial review found nineteen
defects, all fixed with regression tests; 185 hostile inputs crash nothing.
12d Model 15's own 58 MB exports read in a quarter of a second.

**The GIS menu (2026-09-23): every GDAL and PDAL capability reachable from
the application.** A GIS menu and toolbar group the libraries' work by kind of
data: Import Vector Data (choosing a source layer, a target layer and
attributes), Export Vector (with its curve tolerance, layer name and
selection), Import Raster, Export Surface as DEM (GeoTIFF, ASCII grid, IMG -
`interop::exportSurfaceRaster`, each cell the surface's elevation at its
centre), Import Point Cloud (budget, ASPRS class, COPC level of detail),
Export Point Cloud (LAS, LAZ), Convert Point Cloud to COPC, and Dataset
Information (`interop::describeSource`, which the `INFO` verb of both command
lines prints too). Each import dialog is built from what the file holds. Surface
From Raster now triangulates the DEM's TRUE values read through GDAL
(`interop::readRasterElevations`; audit QT-23, QT-24), and Surface From Point
Cloud the cloud's ground returns when it has any, saying so either way
(`interop::surfacePoints`; QT-10). Reference data is cleared with its drawing
(QT-17). The About box names the GDAL and PDAL versions. Driven by ctest
through the QActions themselves (`--action`), against values worked out
outside Katana. Record in `docs/interop.md`, "The GIS menu".

Two things this taught the application. Surface From Drawing now reads
per-vertex heights and leaves a null vertex out of the surface instead of
triangulating it at the datum. And a headless session (`--plot`,
`--screenshot`) no longer opens a question nobody can answer: the first
scripted 12da import hung on the "far from the current drawing" box.

**OUTSTANDING: DWG, LandXML and IFC**, and **text and dimensions are skipped on
DXF export** although DXF has both (the vector path models only points, lines
and polygons). **No coordinate transformation on import**: a file's declared
CRS is reported but coordinates are never reprojected, so mixing coordinate
systems is the user's responsibility. Reference layers are not yet persisted
into the project - the pixels are re-read from the source path each session.
For the 12d archive (this list was stale until 2026-09-23: meshes are
imported and drawn, super tins are built, point symbols are drawn - 20.2
slices 2, 4 and 8): a per-vertex symbol list that differs along a line is kept
and not drawn, and the module's tests were audited by a second reader on
2026-09-23 - its findings are in the register of section 46 (A12-nn).

---

