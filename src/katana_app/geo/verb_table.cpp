// The geoprocessing verb table (verb_table.hpp).
//
// One reserved block per package of the geoprocessing plan
// (docs/geoprocessing.md, "Packages"): a package adds its rows, and the
// declarations of its prepare functions, in its own block and nowhere else,
// so packages built side by side never edit the same lines.

#include "verb_table.hpp"

namespace katana::app::geo {

// ---- T0: Terrain session: SURFACE LIST, INFO, REMOVE, FROM, EXPORT ----
// ---- T1: CONTOUR ----
// ---- T2: RASTER SHADE ----
// ---- T3: RASTER SLOPE, RASTER ASPECT ----
// ---- T4: RASTER ZONAL, RASTER SAMPLE, DRAPE ----
// ---- T5: RASTER VIEWSHED, LOS ----
// ---- T6: RASTER GRID ----
// ---- T7: RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT, DIFFERENCE ----
// ---- V1: GIS BUFFER, GIS DISSOLVE ----
// ---- V2: GIS OVERLAY ----
// ---- V3: GIS HULL, GIS CLIP ----
// ---- V4: GIS CHECK, GIS REPAIR, GIS COVERAGE ----
// ---- V5: GIS SQL ----
// ---- I0: IMPORT, EXPORT, INFO, REFS, COPC ----
// ---- I1: vector fidelity (no verb of its own) ----
// ---- I2: FORMATS ----
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
             "          SURFACE <name> [CELL <m>] or FILE <path>; TO takes LAYER <path>,\n"
             "          REFERENCE [<name>] or FILE <path> [FORMAT <driver>]"});
        // ---- T0: Terrain session ----
        // ---- T1: CONTOUR ----
        // ---- T2: RASTER SHADE ----
        // ---- T3: RASTER SLOPE, RASTER ASPECT ----
        // ---- T4: RASTER ZONAL, RASTER SAMPLE, DRAPE ----
        // ---- T5: RASTER VIEWSHED, LOS ----
        // ---- T6: RASTER GRID ----
        // ---- T7: RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT, DIFFERENCE ----
        // ---- V1: GIS BUFFER, GIS DISSOLVE ----
        // ---- V2: GIS OVERLAY ----
        // ---- V3: GIS HULL, GIS CLIP ----
        // ---- V4: GIS CHECK, GIS REPAIR, GIS COVERAGE ----
        // ---- V5: GIS SQL ----
        // ---- I0: IMPORT, EXPORT, INFO, REFS, COPC ----
        rows.push_back(
            {"IMPORT", "", &prepareImport,
             "IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]  a drawing (.dxf), vector data or\n"
             "          a .12da archive as entities, a raster or a point cloud as a reference\n"
             "          layer, by its extension; LOCAL puts its lower-left corner at 0,0,\n"
             "          ALONGSIDE on the drawing's, OFFSET moves it by dE east and dN north"});
        rows.push_back({"EXPORT", "", &prepareExport,
                        "EXPORT <file>  the drawing, by extension (.dxf, .12da with the surfaces,\n"
                        "          GeoPackage, GeoJSON, shapefile ...)"});
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
        // ---- I3: IMPORT options ----
        // ---- I4: EXPORT options ----
        // ---- D1: INFO as data, STATS, CHECK ----
        // ---- D2: REFS management ----
        return rows;
    }();
    return table;
}

} // namespace katana::app::geo
