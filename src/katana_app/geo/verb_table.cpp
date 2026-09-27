// The geoprocessing verb table (verb_table.hpp).
//
// One reserved block per package of the geoprocessing plan
// (docs/geoprocessing.md, "Packages"): a package adds its rows, and the
// declarations of its prepare functions, in its own block and nowhere else,
// so packages built side by side never edit the same lines.

#include "verb_table.hpp"

namespace katana::app::geo {

// ---- T0: Terrain session: SURFACE LIST, INFO, REMOVE, FROM, EXPORT ----
// surface_verbs.cpp (terrain_verbs.hpp).
katana::core::Result<Prepared> prepareSurface(Context& context, const Tokens& tokens,
                                              std::string_view line);
// ---- T1: CONTOUR ----
// contour_verbs.cpp (terrain_verbs.hpp).
katana::core::Result<Prepared> prepareContour(Context& context, const Tokens& tokens,
                                              std::string_view line);
// ---- T2: RASTER SHADE ----
// shade_verbs.cpp (terrain_verbs.hpp).
katana::core::Result<Prepared> prepareShade(Context& context, const Tokens& tokens,
                                            std::string_view line);
// ---- T3: RASTER SLOPE, RASTER ASPECT ----
// slope_verbs.cpp (terrain_verbs.hpp).
katana::core::Result<Prepared> prepareSlope(Context& context, const Tokens& tokens,
                                            std::string_view line);
katana::core::Result<Prepared> prepareAspect(Context& context, const Tokens& tokens,
                                             std::string_view line);
// ---- T4: RASTER ZONAL, RASTER SAMPLE, DRAPE ----
// zonal_verbs.cpp, drape_verbs.cpp (analysis_support.hpp).
[[nodiscard]] katana::core::Result<Prepared> prepareRasterZonal(Context& context, const Tokens& tokens,
                                                                std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareRasterSample(Context& context,
                                                                 const Tokens& tokens,
                                                                 std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareDrape(Context& context, const Tokens& tokens,
                                                          std::string_view line);
// ---- T5: RASTER VIEWSHED, LOS ----
// viewshed_verbs.cpp (analysis_support.hpp, terrain/line_of_sight.hpp).
[[nodiscard]] katana::core::Result<Prepared>
prepareRasterViewshed(Context& context, const Tokens& tokens, std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareLineOfSight(Context& context,
                                                                const Tokens& tokens,
                                                                std::string_view line);
// ---- T6: RASTER GRID ----
// grid_verbs.cpp: surveyed points to a DEM.
[[nodiscard]] katana::core::Result<Prepared> prepareRasterGrid(Context& context, const Tokens& tokens,
                                                               std::string_view line);
// ---- T7: RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT, DIFFERENCE ----
// dem_verbs.cpp: the DEM tools.
[[nodiscard]] katana::core::Result<Prepared> prepareRasterMosaic(Context& context, const Tokens& tokens,
                                                                 std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareRasterClip(Context& context, const Tokens& tokens,
                                                               std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareRasterFill(Context& context, const Tokens& tokens,
                                                               std::string_view line);
[[nodiscard]] katana::core::Result<Prepared>
prepareRasterFootprint(Context& context, const Tokens& tokens, std::string_view line);
[[nodiscard]] katana::core::Result<Prepared>
prepareRasterReproject(Context& context, const Tokens& tokens, std::string_view line);
[[nodiscard]] katana::core::Result<Prepared>
prepareRasterDifference(Context& context, const Tokens& tokens, std::string_view line);
// ---- V1: GIS BUFFER, GIS DISSOLVE ----
[[nodiscard]] katana::core::Result<Prepared> prepareGisBuffer(Context& context, const Tokens& tokens,
                                                              std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareGisDissolve(Context& context,
                                                                const Tokens& tokens,
                                                                std::string_view line);
// ---- V2: GIS OVERLAY ----
[[nodiscard]] katana::core::Result<Prepared> prepareGisOverlay(Context& context, const Tokens& tokens,
                                                               std::string_view line);
// ---- V3: GIS HULL, GIS CLIP ----
[[nodiscard]] katana::core::Result<Prepared> prepareGisHull(Context& context, const Tokens& tokens,
                                                            std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareGisClip(Context& context, const Tokens& tokens,
                                                            std::string_view line);
// ---- V4: GIS CHECK, GIS REPAIR, GIS COVERAGE ----
[[nodiscard]] katana::core::Result<Prepared> prepareGisCheck(Context& context, const Tokens& tokens,
                                                             std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareGisRepair(Context& context, const Tokens& tokens,
                                                              std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareGisCoverage(Context& context,
                                                                const Tokens& tokens,
                                                                std::string_view line);
// ---- V5: GIS SQL ----
[[nodiscard]] katana::core::Result<Prepared> prepareGisSql(Context& context, const Tokens& tokens,
                                                           std::string_view line);
// ---- I0: IMPORT, EXPORT, INFO, REFS, COPC ----
// ---- I1: vector fidelity (no verb of its own) ----
// ---- I2: FORMATS ----
// formats_verbs.cpp, and formats_verbs.hpp for the MCP tool that shares it.
katana::core::Result<Prepared> prepareFormats(Context& context, const Tokens& tokens,
                                              std::string_view line);
// ---- I3: IMPORT options ----
// ---- I4: EXPORT options ----
// ---- D1: INFO as data, STATS, CHECK ----
// ---- D2: REFS management ----
// ---- X1: the toolbox window (no verb of its own) ----
// ---- X2: the toolbox's pipeline tab (no verb of its own) ----

const std::vector<VerbEntry>& verbTable()
{
    static const std::vector<VerbEntry> table = [] {
        std::vector<VerbEntry> rows;
        // ---- F0: GDAL algorithm bridge ----
        rows.push_back(
            {"GDAL", "", &prepareGdal,
             "GDAL VERSION | LIST [<group>|<text>] [JSON] | HELP <algorithm> [JSON]\n"
             "          GDAL [RUN] <algorithm> [<gdal word>...] [FROM [<arg>] <source>]...\n"
             "          [TO [<arg>] <target>] [CONFIRM] [OVERWRITE] [PREVIEW]  any of GDAL's\n"
             "          algorithms in GDAL's own words; FROM binds SELECTION | DRAWING | VIEW |\n"
             "          AREA x0,y0,x1,y1 | LAYERS a,b [ONLY] [WHERE k=v ...], RASTER <id|name>,\n"
             "          SURFACE <name> [CELL <m>] or FILE <path> [LAYER <name>]; TO takes\n"
             "          LAYER <path>, REFERENCE [<name>], FILE <path> [FORMAT <driver>],\n"
             "          SURFACE <name>, SELECTION or REPORT"});
        // ---- T0: Terrain session ----
        rows.push_back(
            {"SURFACE", "", &prepareSurface,
             "SURFACE LIST [JSON] | INFO <name> | REMOVE <name>  the session's surfaces\n"
             "          SURFACE FROM RASTER <id|name> | FILE <path> [max=<points>]\n"
             "          [AREA x0,y0,x1,y1] | CLOUD <id|name> [classes=2,...] [max=<points>]\n"
             "          | <scope>  [NAME <n>] [PREVIEW]  triangulate a surface from a DEM's\n"
             "          true values, a cloud's ground or the drawing's levelled points and lines\n"
             "          SURFACE EXPORT <name> <file> [cell=<m>] [type=Float32|Float64] [cog]\n"
             "          [format=<driver>] [co=K=V]... [OVERWRITE] [PREVIEW]  write it as a DEM\n"
             "          (tiled, compressed)"});
        // ---- T1: CONTOUR ----
        rows.push_back(
            {"CONTOUR", "", &prepareContour,
             "CONTOUR SURFACE <name> | RASTER <id|name> | FILE <path> interval=<m> [major=5]\n"
             "          [base=0] [layer=terrain/contours] [smooth=3|5] [<scope>] [PREVIEW]\n"
             "          contour lines on <layer>/major and <layer>/minor at their levels: a\n"
             "          surface traced exactly, a raster by GDAL at full resolution; a scope\n"
             "          last keeps them inside its closed shapes"});
        // ---- T2: RASTER SHADE ----
        rows.push_back(
            {"RASTER", "SHADE", &prepareShade,
             "RASTER SHADE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>\n"
             "          [style=hillshade|relief|relief+hillshade|slope|plain] [azimuth=315]\n"
             "          [altitude=45] [z=1] [variant=regular|combined|multidirectional|igor]\n"
             "          [ramp=terrain|diverging|slope|grey|<file>] [range=<min>,<max>] [NAME <n>]\n"
             "          [save=<file.tif>] [OVERWRITE] [PREVIEW]  a picture of the terrain,\n"
             "          kept as a derived reference raster, with its legend"});
        // ---- T3: RASTER SLOPE, RASTER ASPECT ----
        rows.push_back(
            {"RASTER", "SLOPE", &prepareSlope,
             "RASTER SLOPE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>\n"
             "          [unit=percent|degree] [classes=<b1>,<b2>,...] [areas=terrain/slope]\n"
             "          [min_area=<m2>] [NAME <n>] [<scope>] [PREVIEW]  the slope as a\n"
             "          reference raster; classes drawn as areas on <areas>/<class>, with\n"
             "          each class's area"});
        rows.push_back(
            {"RASTER", "ASPECT", &prepareAspect,
             "RASTER ASPECT SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>\n"
             "          [NAME <n>] [<scope>] [PREVIEW]  the direction the ground faces,\n"
             "          degrees clockwise from north, as a reference raster"});
        // ---- T4: RASTER ZONAL, RASTER SAMPLE, DRAPE ----
        rows.push_back(
            {"RASTER", "ZONAL", &prepareRasterZonal,
             "RASTER ZONAL SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path> [<scope>]\n"
             "          [stats=mean,min,max,count,sum] [prefix=zone]\n"
             "          [pixels=fractional|centre|all-touched] [csv=<file>] [OVERWRITE] [PREVIEW]\n"
             "          a raster's statistics inside each closed shape the scope takes, written\n"
             "          on the shapes as <prefix>_<stat> properties (one undo step)"});
        rows.push_back(
            {"RASTER", "SAMPLE", &prepareRasterSample,
             "RASTER SAMPLE SURFACE <name> | RASTER <id|name> | FILE <path> [AT x,y]...\n"
             "          [<scope>] [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]  the\n"
             "          height of the ground at points; off it, ground=no, never 0"});
        rows.push_back(
            {"DRAPE", "", &prepareDrape,
             "DRAPE SURFACE <name> | RASTER <id|name> | FILE <path> [<scope>]\n"
             "          [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]  the points,\n"
             "          lines and polylines the scope takes given the ground's height at every\n"
             "          vertex (one undo step); a vertex off the ground is left heightless"});
        // ---- T5: RASTER VIEWSHED, LOS ----
        rows.push_back(
            {"RASTER", "VIEWSHED", &prepareRasterViewshed,
             "RASTER VIEWSHED SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>\n"
             "          (OBSERVER x,y)... | OBSERVERS [<scope>] [height=1.7] [target=0] [max=<m>]\n"
             "          [curvature=<k>|none] [areas=<layer>] [NAME <n>] [PREVIEW]  what the\n"
             "          observers see, unioned, as a tinted reference raster; areas= draws the\n"
             "          visible area and the reply measures it"});
        rows.push_back(
            {"LOS", "", &prepareLineOfSight,
             "LOS SURFACE <name> | RASTER <id|name> | FILE <path> OBSERVER x,y TARGET x,y\n"
             "          [height=1.7] [target=0] [curvature=<k>|none] [step=<m>]\n"
             "          [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]  whether the\n"
             "          target is seen, where the ground first hides it, and the least\n"
             "          clearance of the sight line"});
        // ---- T6: RASTER GRID ----
        rows.push_back(
            {"RASTER", "GRID", &prepareRasterGrid,
             "RASTER GRID [<scope>] [method=linear|invdist|invdistnn|nearest|average|...]\n"
             "          [cell=<m> | size=<columns>x<rows>] [z=geometry|<property>]\n"
             "          [extent=scope|x0,y0,x1,y1] [power=<p>] [radius=<m>] [NAME <name>]\n"
             "          [TO REFERENCE [<name>] | TO FILE <path> [FORMAT <driver>] |\n"
             "          TO SURFACE <name>] [OVERWRITE] [PREVIEW]  the points in scope (and line\n"
             "          vertices) gridded into a DEM;\n"
             "          heightless entities are left out and counted, never read as 0"});
        // ---- T7: RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT, DIFFERENCE ----
        // The first row says what <raster> is and the words every DEM tool
        // takes, as docs/terrain.md's grammar does ("The DEM tools"): HELP
        // lists the rows in this order.
        rows.push_back(
            {"RASTER", "MOSAIC", &prepareRasterMosaic,
             "RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT, DIFFERENCE: <raster> is\n"
             "          RASTER <id|name> | SURFACE <name> [CELL <m>] | FILE <path>, and each\n"
             "          takes [NAME <name>] [TO REFERENCE [<name>] | TO FILE <path>\n"
             "          [FORMAT <driver>] | TO SURFACE <name>] [OVERWRITE] [PREVIEW]\n"
             "          RASTER MOSAIC (RASTER <id|name> | FILE <path|folder|pattern>)...\n"
             "          [resolution=same|highest|lowest|average|<x>,<y>] [SAVE <file>]  tiles\n"
             "          joined into one DEM, a VRT kept as a reference raster (SAVE writes it\n"
             "          out and keeps that file); a surface is no tile"});
        rows.push_back(
            {"RASTER", "CLIP", &prepareRasterClip,
             "RASTER CLIP <raster> AREA x0,y0,x1,y1 | <scope>  a DEM cut to a box, or to the\n"
             "          closed boundaries the scope takes"});
        rows.push_back(
            {"RASTER", "FILL", &prepareRasterFill,
             "RASTER FILL <raster> [distance=<cells>] [smoothing=<n>] [strategy=invdist|nearest]\n"
             "          a DEM's holes filled from their edges; the reply counts the cells"});
        rows.push_back(
            {"RASTER", "FOOTPRINT", &prepareRasterFootprint,
             "RASTER FOOTPRINT <raster> [TO LAYER <path> | TO FILE <path>]  where a raster has\n"
             "          data, as closed polylines (gis/footprint)"});
        rows.push_back(
            {"RASTER", "REPROJECT", &prepareRasterReproject,
             "RASTER REPROJECT <raster> [crs=<crs> | like=<raster>] [from=<crs>]\n"
             "          [resampling=<method>] [cell=<m>]  a DEM into the project's CRS, another,\n"
             "          or another raster's grid"});
        rows.push_back(
            {"RASTER", "DIFFERENCE", &prepareRasterDifference,
             "RASTER DIFFERENCE <raster> [MINUS] <raster> [<scope>] [resampling=<method>]\n"
             "          the first minus the second, aligned to the first; cut and fill volumes\n"
             "          (grid method) within the scope's closed boundaries"});
        // ---- V1: GIS BUFFER, GIS DISSOLVE ----
        rows.push_back(
            {"GIS", "BUFFER", &prepareGisBuffer,
             "GIS BUFFER [<scope>] distance=<m>|distance=prop:<key> [side=both|left|right]\n"
             "          [caps=round|flat|square] [joins=round|mitre|bevel] [dissolve[=k1,k2]]\n"
             "          [TO LAYER <path>] [PREVIEW]  areas around what the scope takes (default\n"
             "          layer gis/buffer); a negative distance shrinks an area (a setback)"});
        rows.push_back(
            {"GIS", "DISSOLVE", &prepareGisDissolve,
             "GIS DISSOLVE [<scope>] [by=k1,k2] [keep=identical] [TO LAYER <path>] [REPLACE]\n"
             "          [PREVIEW]  areas merged by their properties (default layer gis/dissolve);\n"
             "          REPLACE deletes the areas merged, in the same step"});
        // ---- V2: GIS OVERLAY ----
        rows.push_back(
            {"GIS", "OVERLAY", &prepareGisOverlay,
             "GIS OVERLAY intersection|difference|union|symdifference|identity|update|clip\n"
             "          <scope> WITH (<scope> | FILE <path> [LAYER <name>] [where=\"<sql>\"])\n"
             "          [keep=a,b|all|none] [keepwith=a,b|all|none] [TO LAYER <path>]\n"
             "          [csv=<file>] [OVERWRITE] [PREVIEW]  polygon booleans between two scopes\n"
             "          (default layer gis/overlay): a row per piece with its area or length"});
        // ---- V3: GIS HULL, GIS CLIP ----
        rows.push_back(
            {"GIS", "HULL", &prepareGisHull,
             "GIS HULL [<scope>] [convex | concave=<0..1>] [holes] [TO LAYER <path>] [PREVIEW]\n"
             "          the boundary around every point and vertex the scope takes (default\n"
             "          convex; layer gis/hull)"});
        rows.push_back(
            {"GIS", "CLIP", &prepareGisClip,
             "GIS CLIP [<scope>] BY (<scope> | FILE <path> [LAYER <name>] [where=\"<sql>\"])\n"
             "          [TO LAYER <path> | REPLACE] [PREVIEW]  what the scope takes cut to the\n"
             "          boundary's areas: the pieces on a layer (gis/clip), or in place"});
        // ---- V4: GIS CHECK, GIS REPAIR, GIS COVERAGE ----
        rows.push_back(
            {"GIS", "CHECK", &prepareGisCheck,
             "GIS CHECK [<scope>] [markers=<layer>] [PREVIEW]  invalid lines and areas: a\n"
             "          problem record at each defect (a self-intersection, ...); markers= draws\n"
             "          a point at each, replacing the last check's"});
        rows.push_back(
            {"GIS", "REPAIR", &prepareGisRepair,
             "GIS REPAIR [<scope>] [method=linework|structure] [PREVIEW]  invalid areas made\n"
             "          valid in place, ids kept; a result of several parts keeps the id on\n"
             "          the largest and draws the rest"});
        rows.push_back(
            {"GIS", "COVERAGE", &prepareGisCoverage,
             "GIS COVERAGE CHECK [<scope>] [gap=<m>] [markers=<layer>] [PREVIEW]  overlaps,\n"
             "          enclosed gaps narrower than gap= and edges that do not match, as\n"
             "          problem records; GIS COVERAGE CLEAN [<scope>] [gap=<m>] [snap=<m>]\n"
             "          [merge=longest-border|max-area|min-area|min-index] REPLACE [PREVIEW]\n"
             "          moves the boundaries so they match (enclosed gaps only)"});
        // ---- V5: GIS SQL ----
        rows.push_back(
            {"GIS", "SQL", &prepareGisSql,
             "GIS SQL \"<select>\" [<scope>] [dialect=sqlite|ogrsql] [AS REPORT | AS SELECT |\n"
             "          AS LAYER <layer>] [csv=<file>] [OVERWRITE] [PREVIEW]  the scope queried\n"
             "          as tables points, lines, polygons (katana_id, layer, style, colour,\n"
             "          type, then the properties); SQLite with Spatialite by default"});
        // ---- I0: IMPORT, EXPORT, INFO, REFS, COPC ----
        rows.push_back(
            {"IMPORT", "", &prepareImport,
             "IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]  a drawing (.dxf), vector data or\n"
             "          a .12da archive as entities, a raster or a point cloud as a reference\n"
             "          layer, by its extension; LOCAL puts its lower-left corner at 0,0,\n"
             "          ALONGSIDE on the drawing's, OFFSET moves it by dE east and dN north\n"
             "          vector data: [layers=a,b] [where=\"...\"] [sql=\"...\"] [dialect=ogrsql|sqlite]\n"
             "          [<scope> [clip]] [fields=a,b] [attributes=no] [target=<layer>] [max=N]\n"
             "          [oo=K=V]... ; every kind: [crs=project|adopt] [srs=<code>] [PREVIEW];\n"
             "          a raster: [band=N] [subdataset=N|name] [maxpixels=N] [name=<n>];\n"
             "          a point cloud: [budget=N] [class=N] [resolution=<m>] [name=<n>]"});
        rows.push_back(
            {"EXPORT", "", &prepareExport,
             "EXPORT <file> [<scope>]  the drawing, or what the scope takes of it, by\n"
             "          extension (.dxf, .12da with the surfaces, GeoPackage, GeoJSON,\n"
             "          shapefile ...); GDAL's formats also [layername=<n> | split=layer]\n"
             "          [append] [crs=project|native|<code>] [co=K=V]... [lco=K=V]...\n"
             "          [text=points|skip] [curve=<m>] [properties=yes|no]; [PREVIEW]\n"
             "          EXPORT <file.las|.laz> [CLOUD <id|name>] [PREVIEW]  a reference\n"
             "          point cloud as held (a budgeted import's sample, said so)"});
        rows.push_back({"INFO", "", &prepareInfo,
                        "INFO <file|folder|url> [JSON] [STATS] [CHECK] [LAYER <name>]  what a\n"
                        "          GIS file, a folder or a point cloud holds, without importing it:\n"
                        "          dataset, raster, band, layer and field records, or GDAL's own\n"
                        "          JSON; STATS the bands' statistics, CHECK every value read\n"
                        "          (INFO <id> describes an entity, when no file has that name)",
                        &takesInfo});
        rows.push_back(
            {"REFS", "", &prepareRefs,
             "REFS [LIST] [JSON] | SHOW|HIDE|REMOVE|INFO <ref> | OPACITY <ref> <0..1>\n"
             "          | COLOR <ref> elevation|intensity|classification|rgb|flat\n"
             "          | RENAME <ref> <name> | OVERVIEWS <ref> [levels=2,4,8] CONFIRM | RESTORE\n"
             "          the reference layers - rasters and point clouds - by id or name; the\n"
             "          project records their sources and display, and RESTORE (on OPEN)\n"
             "          reads them again"});
        rows.push_back({"COPC", "", &prepareCopc,
                        "COPC <source> <destination.copc.laz>  a point cloud rewritten as COPC,\n"
                        "          every point kept"});
        // ---- I2: FORMATS ----
        rows.push_back(
            {"FORMATS", "", &prepareFormats,
             "FORMATS [RASTER|VECTOR] [READ|WRITE] [<text>...] [JSON]  the formats this GDAL\n"
             "          reads and writes, from its registry: driver, kinds, extensions, /vsi\n"
             "          FORMATS OPTIONS <driver> [JSON]  its open, creation and layer options"});
        // ---- I3: IMPORT options ----
        // ---- I4: EXPORT options ----
        // ---- D1: INFO as data, STATS, CHECK ----
        // ---- D2: REFS management ----
        return rows;
    }();
    return table;
}

} // namespace katana::app::geo
