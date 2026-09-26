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
        rows.push_back(
            {"RASTER", "GRID", &prepareRasterGrid,
             "RASTER GRID [<scope>] [method=linear|invdist|invdistnn|nearest|average|...]\n"
             "          [cell=<m> | size=<columns>x<rows>] [z=geometry|<property>]\n"
             "          [extent=scope|x0,y0,x1,y1] [power=<p>] [radius=<m>] [NAME <name>]\n"
             "          [TO REFERENCE [<name>] | TO FILE <path> [FORMAT <driver>]] [OVERWRITE]\n"
             "          [PREVIEW]  the points in scope (and line vertices) gridded into a DEM;\n"
             "          heightless entities are left out and counted, never read as 0"});
        // ---- T7: RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT, DIFFERENCE ----
        // <raster> := RASTER <id|name> | SURFACE <name> [CELL <m>] | FILE <path>; each
        // also takes [NAME <name>] [TO ...] [OVERWRITE] [PREVIEW].
        rows.push_back(
            {"RASTER", "MOSAIC", &prepareRasterMosaic,
             "RASTER MOSAIC <raster|FILE folder|FILE pattern>... [resolution=<r>] [SAVE <file>]\n"
             "          tiles joined into one DEM, a VRT kept as a reference raster (SAVE writes\n"
             "          it out and keeps that file)"});
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
             "RASTER DIFFERENCE <raster> <raster> [<scope>] [resampling=<method>]  the first\n"
             "          minus the second, aligned to the first; cut and fill volumes (grid\n"
             "          method) within the scope's closed boundaries"});
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
