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
        rows.push_back(
            {"SURFACE", "", &prepareSurface,
             "SURFACE LIST [JSON] | INFO <name> | REMOVE <name>  the session's surfaces\n"
             "          SURFACE FROM RASTER <id|name> | FILE <path> [max=<points>]\n"
             "          [AREA x0,y0,x1,y1] | CLOUD <id|name> [classes=2,...] | <scope>\n"
             "          [NAME <n>]  triangulate a surface from a DEM's true values, a\n"
             "          cloud's ground or the drawing's levelled points and lines\n"
             "          SURFACE EXPORT <name> <file> [cell=<m>] [type=Float32|Float64] [cog]\n"
             "          [co=K=V]... [OVERWRITE]  write it as a DEM (tiled, compressed)"});
        // ---- T1: CONTOUR ----
        rows.push_back(
            {"CONTOUR", "", &prepareContour,
             "CONTOUR SURFACE <name> | RASTER <id|name> | FILE <path> interval=<m> [major=5]\n"
             "          [base=0] [layer=terrain/contours] [smooth=3|5] [<scope>] [PREVIEW]\n"
             "          contour lines on <layer>/major and <layer>/minor at their levels: a\n"
             "          surface traced exactly, a raster by GDAL at full resolution; a scope\n"
             "          last keeps them inside its closed shapes"});
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
