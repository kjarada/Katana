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
[[nodiscard]] katana::core::Result<Prepared> prepareGisBuffer(Context& context, const Tokens& tokens,
                                                              std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareGisDissolve(Context& context,
                                                                const Tokens& tokens,
                                                                std::string_view line);
// ---- V2: GIS OVERLAY ----
[[nodiscard]] katana::core::Result<Prepared> prepareGisOverlay(Context& context, const Tokens& tokens,
                                                               std::string_view line);
// ---- V3: GIS HULL, GIS CLIP ----
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
