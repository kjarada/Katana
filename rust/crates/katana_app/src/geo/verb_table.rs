//! The geoprocessing verbs the executor runs (docs/geoprocessing.md, "The
//! verb table"), the port of src/katana_app/geo/verb_table.hpp/.cpp: each a
//! verb, an optional second word, the function that prepares its line and
//! its usage. handles(), prepare() and help_text() read this table alone, so
//! a verb is added by adding its row - HELP, the window's Command Reference
//! and katana_help then name it with no other change.
//!
//! One reserved block per package of the geoprocessing plan
//! (docs/geoprocessing.md, "Packages"): a package adds its rows in its own
//! block and nowhere else, so packages built side by side never edit the same
//! lines. HELP lists the rows in the table's order, so each block stands where
//! the C++'s does. Step 6's wave 1 (G1) wrote every block and its own rows
//! (GDAL, COPC, FORMATS); each other row is here as a comment in its block,
//! the C++'s usage word for word, for its owner (A, B, R, V) to take with the
//! module it writes. The C++'s I0 block is three sub-blocks, one per owner.
//!
//! The table is one static list: a row is plain data and a prepare function
//! is a function pointer, so the list is built once, at compile time, where
//! the C++ built a vector on first use.

use katana_core::Result;

use super::bindings::Tokens;
use super::geo_verbs::{Context, Prepared};

// The C++ declares these in verb_table.hpp: the F0 and I0 prepare functions
// (and gdalUsage), whose files are private to the executor.
pub use super::copc_verb::prepare_copc;
pub use super::gdal_verbs::{gdal_usage, prepare_gdal};

/// Prepares a line of the row's verb.
pub type PrepareVerb = fn(&mut Context<'_>, &Tokens, &str) -> Result<Prepared>;

/// Whether a line of the verb is the executor's at all: a verb shared with
/// the interpreter answers false for the lines that are the interpreter's.
pub type TakesLine = fn(&Tokens) -> bool;

#[derive(Clone, Copy)]
pub struct VerbEntry {
    /// Upper case.
    pub verb: &'static str,
    /// Upper case; "" takes any second word.
    pub subverb: &'static str,
    pub prepare: PrepareVerb,
    /// For help_text: "VERB ...  what it does".
    pub usage: &'static str,
    /// None takes every line of the verb. INFO's leaves INFO <id>, an
    /// entity, to the interpreter.
    pub takes: Option<TakesLine>,
}

/// Every row, in the table's order.
pub fn verb_table() -> &'static [VerbEntry] {
    VERB_TABLE
}

static VERB_TABLE: &[VerbEntry] = &[
    // ---- F0: GDAL algorithm bridge (G1) ----
    VerbEntry {
        verb: "GDAL",
        subverb: "",
        prepare: super::gdal_verbs::prepare_gdal,
        usage: concat!(
            "GDAL VERSION | LIST [<group>|<text>] [JSON] | HELP <algorithm> [JSON]\n",
            "          GDAL [RUN] <algorithm> [<gdal word>...] [FROM [<arg>] <source>]...\n",
            "          [TO [<arg>] <target>] [CONFIRM] [OVERWRITE] [PREVIEW]  any of GDAL's\n",
            "          algorithms in GDAL's own words; FROM binds SELECTION | DRAWING | VIEW |\n",
            "          AREA x0,y0,x1,y1 | LAYERS a,b [ONLY] [WHERE k=v ...], RASTER <id|name>,\n",
            "          SURFACE <name> [CELL <m>] or FILE <path> [LAYER <name>]; TO takes\n",
            "          LAYER <path>, REFERENCE [<name>], FILE <path> [FORMAT <driver>],\n",
            "          SURFACE <name>, SELECTION or REPORT",
        ),
        takes: None,
    },
    // ---- T0: Terrain session (A) ----
    // VerbEntry {
    //     verb: "SURFACE",
    //     subverb: "",
    //     prepare: super::surface_verbs::prepare_surface,
    //     usage: concat!(
    //         "SURFACE LIST [JSON] | INFO <name> | REMOVE <name>  the drawing's surfaces\n",
    //         "          SURFACE FROM RASTER <id|name> | FILE <path> [max=<points>]\n",
    //         "          [AREA x0,y0,x1,y1] | CLOUD <id|name> [classes=2,...] [max=<points>]\n",
    //         "          | <scope>  [NAME <n>] [PREVIEW]  triangulate a surface from a DEM's\n",
    //         "          true values, a cloud's ground or the drawing's levelled points and lines\n",
    //         "          SURFACE EXPORT <name> <file> [cell=<m>] [type=Float32|Float64] [cog]\n",
    //         "          [format=<driver>] [co=K=V]... [OVERWRITE] [PREVIEW]  write it as a DEM\n",
    //         "          (tiled, compressed)",
    //     ),
    //     takes: Some(super::surface_verbs::takes_surface),
    // },
    // ---- T1: CONTOUR (A) ----
    // VerbEntry {
    //     verb: "CONTOUR",
    //     subverb: "",
    //     prepare: super::contour_verbs::prepare_contour,
    //     usage: concat!(
    //         "CONTOUR SURFACE <name> | RASTER <id|name> | FILE <path> interval=<m> [major=5]\n",
    //         "          [base=0] [layer=terrain/contours] [smooth=3|5] [<scope>] [PREVIEW]\n",
    //         "          contour lines on <layer>/major and <layer>/minor at their levels: a\n",
    //         "          surface traced exactly, a raster by GDAL at full resolution; a scope\n",
    //         "          last keeps them inside its closed shapes",
    //     ),
    //     takes: None,
    // },
    // ---- T2: RASTER SHADE (R) ----
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "SHADE",
    //     prepare: super::shade_verbs::prepare_shade,
    //     usage: concat!(
    //         "RASTER SHADE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>\n",
    //         "          [style=hillshade|relief|relief+hillshade|slope|plain] [azimuth=315]\n",
    //         "          [altitude=45] [z=1] [variant=regular|combined|multidirectional|igor]\n",
    //         "          [ramp=terrain|earth|diverging|slope|grey|spectral|<file>]\n",
    //         "          [range=<min>,<max>] [NAME <n>] [save=<file.tif>] [OVERWRITE] [PREVIEW]\n",
    //         "          a picture of the terrain, kept as a derived reference raster, with its\n",
    //         "          legend",
    //     ),
    //     takes: None,
    // },
    // ---- T3: RASTER SLOPE, RASTER ASPECT (R) ----
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "SLOPE",
    //     prepare: super::slope_verbs::prepare_slope,
    //     usage: concat!(
    //         "RASTER SLOPE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>\n",
    //         "          [unit=percent|degree] [classes=<b1>,<b2>,...] [areas=terrain/slope]\n",
    //         "          [min_area=<m2>] [NAME <n>] [<scope>] [PREVIEW]  the slope as a\n",
    //         "          reference raster; classes drawn as areas on <areas>/<class>, with\n",
    //         "          each class's area",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "ASPECT",
    //     prepare: super::slope_verbs::prepare_aspect,
    //     usage: concat!(
    //         "RASTER ASPECT SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>\n",
    //         "          [NAME <n>] [<scope>] [PREVIEW]  the direction the ground faces,\n",
    //         "          degrees clockwise from north, as a reference raster",
    //     ),
    //     takes: None,
    // },
    // ---- T4: RASTER ZONAL, RASTER SAMPLE, DRAPE (B) ----
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "ZONAL",
    //     prepare: super::zonal_verbs::prepare_raster_zonal,
    //     usage: concat!(
    //         "RASTER ZONAL SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path> [<scope>]\n",
    //         "          [stats=mean,min,max,count,sum] [prefix=zone]\n",
    //         "          [pixels=fractional|centre|all-touched] [csv=<file>] [OVERWRITE] [PREVIEW]\n",
    //         "          a raster's statistics inside each closed shape the scope takes, written\n",
    //         "          on the shapes as <prefix>_<stat> properties (one undo step)",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "SAMPLE",
    //     prepare: super::drape_verbs::prepare_raster_sample,
    //     usage: concat!(
    //         "RASTER SAMPLE SURFACE <name> | RASTER <id|name> | FILE <path> [AT x,y]...\n",
    //         "          [<scope>] [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]  the\n",
    //         "          height of the ground at points; off it, ground=no, never 0",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "DRAPE",
    //     subverb: "",
    //     prepare: super::drape_verbs::prepare_drape,
    //     usage: concat!(
    //         "DRAPE SURFACE <name> | RASTER <id|name> | FILE <path> [<scope>]\n",
    //         "          [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]  the points,\n",
    //         "          lines and polylines the scope takes given the ground's height at every\n",
    //         "          vertex (one undo step); a vertex off the ground is left heightless",
    //     ),
    //     takes: None,
    // },
    // ---- T5: RASTER VIEWSHED, LOS (R) ----
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "VIEWSHED",
    //     prepare: super::viewshed_verbs::prepare_raster_viewshed,
    //     usage: concat!(
    //         "RASTER VIEWSHED SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>\n",
    //         "          (OBSERVER x,y)... | OBSERVERS [<scope>] [height=1.7] [target=0] [max=<m>]\n",
    //         "          [curvature=<k>|none] [areas=<layer>] [NAME <n>] [PREVIEW]  what the\n",
    //         "          observers see, unioned, as a tinted reference raster; areas= draws the\n",
    //         "          visible area and the reply measures it",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "LOS",
    //     subverb: "",
    //     prepare: super::viewshed_verbs::prepare_line_of_sight,
    //     usage: concat!(
    //         "LOS SURFACE <name> | RASTER <id|name> | FILE <path> OBSERVER x,y TARGET x,y\n",
    //         "          [height=1.7] [target=0] [curvature=<k>|none] [step=<m>]\n",
    //         "          [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]  whether the\n",
    //         "          target is seen, where the ground first hides it, and the least\n",
    //         "          clearance of the sight line",
    //     ),
    //     takes: None,
    // },
    // ---- T6: RASTER GRID (R) ----
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "GRID",
    //     prepare: super::grid_verbs::prepare_raster_grid,
    //     usage: concat!(
    //         "RASTER GRID [<scope>] [method=linear|invdist|invdistnn|nearest|average|...]\n",
    //         "          [cell=<m> | size=<columns>x<rows>] [z=geometry|<property>]\n",
    //         "          [extent=scope|x0,y0,x1,y1] [power=<p>] [radius=<m>] [NAME <name>]\n",
    //         "          [TO REFERENCE [<name>] | TO FILE <path> [FORMAT <driver>] |\n",
    //         "          TO SURFACE <name>] [OVERWRITE] [PREVIEW]  the points in scope (and line\n",
    //         "          vertices) gridded into a DEM;\n",
    //         "          heightless entities are left out and counted, never read as 0",
    //     ),
    //     takes: None,
    // },
    // ---- T7: RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT, DIFFERENCE (R) ----
    // The first row says what <raster> is and the words every DEM tool
    // takes, as docs/terrain.md's grammar does ("The DEM tools"): HELP
    // lists the rows in this order.
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "MOSAIC",
    //     prepare: super::dem_verbs::prepare_raster_mosaic,
    //     usage: concat!(
    //         "RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT, DIFFERENCE: <raster> is\n",
    //         "          RASTER <id|name> | SURFACE <name> [CELL <m>] | FILE <path>, and each\n",
    //         "          takes [NAME <name>] [TO REFERENCE [<name>] | TO FILE <path>\n",
    //         "          [FORMAT <driver>] | TO SURFACE <name>] [OVERWRITE] [PREVIEW]\n",
    //         "          RASTER MOSAIC (RASTER <id|name> | FILE <path|folder|pattern>)...\n",
    //         "          [resolution=same|highest|lowest|average|<x>,<y>] [SAVE <file>]  tiles\n",
    //         "          joined into one DEM, a VRT kept as a reference raster (SAVE writes it\n",
    //         "          out and keeps that file); a surface is no tile",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "CLIP",
    //     prepare: super::dem_verbs::prepare_raster_clip,
    //     usage: concat!(
    //         "RASTER CLIP <raster> AREA x0,y0,x1,y1 | <scope>  a DEM cut to a box, or to the\n",
    //         "          closed boundaries the scope takes",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "FILL",
    //     prepare: super::dem_verbs::prepare_raster_fill,
    //     usage: concat!(
    //         "RASTER FILL <raster> [distance=<cells>] [smoothing=<n>] [strategy=invdist|nearest]\n",
    //         "          a DEM's holes filled from their edges; the reply counts the cells",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "FOOTPRINT",
    //     prepare: super::dem_verbs::prepare_raster_footprint,
    //     usage: concat!(
    //         "RASTER FOOTPRINT <raster> [TO LAYER <path> | TO FILE <path>]  where a raster has\n",
    //         "          data, as closed polylines (gis/footprint)",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "REPROJECT",
    //     prepare: super::dem_verbs::prepare_raster_reproject,
    //     usage: concat!(
    //         "RASTER REPROJECT <raster> [crs=<crs> | like=<raster>] [from=<crs>]\n",
    //         "          [resampling=<method>] [cell=<m>]  a DEM into the project's CRS, another,\n",
    //         "          or another raster's grid",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "RASTER",
    //     subverb: "DIFFERENCE",
    //     prepare: super::dem_verbs::prepare_raster_difference,
    //     usage: concat!(
    //         "RASTER DIFFERENCE <raster> [MINUS] <raster> [<scope>] [resampling=<method>]\n",
    //         "          the first minus the second, aligned to the first; cut and fill volumes\n",
    //         "          (grid method) within the scope's closed boundaries",
    //     ),
    //     takes: None,
    // },
    // ---- V1: GIS BUFFER, GIS DISSOLVE (V) ----
    // VerbEntry {
    //     verb: "GIS",
    //     subverb: "BUFFER",
    //     prepare: super::buffer_verbs::prepare_gis_buffer,
    //     usage: concat!(
    //         "GIS BUFFER [<scope>] distance=<m>|distance=prop:<key> [side=both|left|right]\n",
    //         "          [caps=round|flat|square] [joins=round|mitre|bevel] [dissolve[=k1,k2]]\n",
    //         "          [TO LAYER <path>] [PREVIEW]  areas around what the scope takes (default\n",
    //         "          layer gis/buffer); a negative distance shrinks an area (a setback)",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "GIS",
    //     subverb: "DISSOLVE",
    //     prepare: super::buffer_verbs::prepare_gis_dissolve,
    //     usage: concat!(
    //         "GIS DISSOLVE [<scope>] [by=k1,k2] [keep=identical] [TO LAYER <path>] [REPLACE]\n",
    //         "          [PREVIEW]  areas merged by their properties (default layer gis/dissolve);\n",
    //         "          REPLACE deletes the areas merged, in the same step",
    //     ),
    //     takes: None,
    // },
    // ---- V2: GIS OVERLAY (V) ----
    // VerbEntry {
    //     verb: "GIS",
    //     subverb: "OVERLAY",
    //     prepare: super::overlay_verbs::prepare_gis_overlay,
    //     usage: concat!(
    //         "GIS OVERLAY intersection|difference|union|symdifference|identity|update|clip\n",
    //         "          <scope> WITH (<scope> | FILE <path> [LAYER <name>] [where=\"<sql>\"])\n",
    //         "          [keep=a,b|all|none] [keepwith=a,b|all|none] [TO LAYER <path>]\n",
    //         "          [csv=<file>] [OVERWRITE] [PREVIEW]  polygon booleans between two scopes\n",
    //         "          (default layer gis/overlay): a row per piece with its area or length",
    //     ),
    //     takes: None,
    // },
    // ---- V3: GIS HULL, GIS CLIP (V) ----
    // VerbEntry {
    //     verb: "GIS",
    //     subverb: "HULL",
    //     prepare: super::hull_clip_verbs::prepare_gis_hull,
    //     usage: concat!(
    //         "GIS HULL [<scope>] [convex | concave=<0..1>] [holes] [TO LAYER <path>] [PREVIEW]\n",
    //         "          the boundary around every point and vertex the scope takes (default\n",
    //         "          convex; layer gis/hull)",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "GIS",
    //     subverb: "CLIP",
    //     prepare: super::hull_clip_verbs::prepare_gis_clip,
    //     usage: concat!(
    //         "GIS CLIP [<scope>] BY (<scope> | FILE <path> [LAYER <name>] [where=\"<sql>\"])\n",
    //         "          [TO LAYER <path> | REPLACE] [PREVIEW]  what the scope takes cut to the\n",
    //         "          boundary's areas: the pieces on a layer (gis/clip), or in place",
    //     ),
    //     takes: None,
    // },
    // ---- V4: GIS CHECK, GIS REPAIR, GIS COVERAGE (V) ----
    // VerbEntry {
    //     verb: "GIS",
    //     subverb: "CHECK",
    //     prepare: super::check_verbs::prepare_gis_check,
    //     usage: concat!(
    //         "GIS CHECK [<scope>] [markers=<layer>] [PREVIEW]  invalid lines and areas: a\n",
    //         "          problem record at each defect (a self-intersection, ...); markers= draws\n",
    //         "          a point at each, replacing the last check's",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "GIS",
    //     subverb: "REPAIR",
    //     prepare: super::check_verbs::prepare_gis_repair,
    //     usage: concat!(
    //         "GIS REPAIR [<scope>] [method=linework|structure] [PREVIEW]  invalid areas made\n",
    //         "          valid in place, ids kept; a result of several parts keeps the id on\n",
    //         "          the largest and draws the rest",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "GIS",
    //     subverb: "COVERAGE",
    //     prepare: super::check_verbs::prepare_gis_coverage,
    //     usage: concat!(
    //         "GIS COVERAGE CHECK [<scope>] [gap=<m>] [markers=<layer>] [PREVIEW]  overlaps,\n",
    //         "          enclosed gaps narrower than gap= and edges that do not match, as\n",
    //         "          problem records; GIS COVERAGE CLEAN [<scope>] [gap=<m>] [snap=<m>]\n",
    //         "          [merge=longest-border|max-area|min-area|min-index] REPLACE [PREVIEW]\n",
    //         "          moves the boundaries so they match (enclosed gaps only)",
    //     ),
    //     takes: None,
    // },
    // ---- V5: GIS SQL (V) ----
    // VerbEntry {
    //     verb: "GIS",
    //     subverb: "SQL",
    //     prepare: super::sql_verbs::prepare_gis_sql,
    //     usage: concat!(
    //         "GIS SQL \"<select>\" [<scope>] [dialect=sqlite|ogrsql] [AS REPORT | AS SELECT |\n",
    //         "          AS LAYER <layer>] [csv=<file>] [OVERWRITE] [PREVIEW]  the scope queried\n",
    //         "          as tables points, lines, polygons (katana_id, layer, style, colour,\n",
    //         "          type, then the properties); SQLite with Spatialite by default",
    //     ),
    //     takes: None,
    // },
    // ---- I0: IMPORT, EXPORT, INFO, REFS, COPC ----
    // ---- I0 (B): IMPORT, EXPORT ----
    // VerbEntry {
    //     verb: "IMPORT",
    //     subverb: "",
    //     prepare: super::import_verb::prepare_import,
    //     usage: concat!(
    //         "IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]  a drawing (.dxf), vector data or\n",
    //         "          a .12da archive as entities, a raster or a point cloud as a reference\n",
    //         "          layer, by its extension; LOCAL puts its lower-left corner at 0,0,\n",
    //         "          ALONGSIDE on the drawing's, OFFSET moves it by dE east and dN north\n",
    //         "          vector data: [layers=a,b] [where=\"...\"] [sql=\"...\"] [dialect=ogrsql|sqlite]\n",
    //         "          [<scope> [clip]] [fields=a,b] [attributes=no] [target=<layer>] [max=N]\n",
    //         "          [oo=K=V]... ; every kind: [crs=project|adopt] [srs=<code>] [PREVIEW];\n",
    //         "          a raster: [band=N] [subdataset=N|name] [maxpixels=N] [name=<n>];\n",
    //         "          a point cloud: [budget=N] [class=N] [resolution=<m>] [name=<n>]",
    //     ),
    //     takes: None,
    // },
    // VerbEntry {
    //     verb: "EXPORT",
    //     subverb: "",
    //     prepare: super::export_verb::prepare_export,
    //     usage: concat!(
    //         "EXPORT <file> [<scope>]  the drawing, or what the scope takes of it, by\n",
    //         "          extension (.dxf, .12da with the surfaces, GeoPackage, GeoJSON,\n",
    //         "          shapefile ...); GDAL's formats also [layername=<n> | split=layer]\n",
    //         "          [append] [crs=project|native|<code>] [co=K=V]... [lco=K=V]...\n",
    //         "          [text=points|skip] [curve=<m>] [properties=yes|no]; [PREVIEW]\n",
    //         "          EXPORT <file.las|.laz> [CLOUD <id|name>] [PREVIEW]  a reference\n",
    //         "          point cloud as held (a budgeted import's sample, said so)",
    //     ),
    //     takes: None,
    // },
    // ---- I0 (A): INFO, REFS ----
    // VerbEntry {
    //     verb: "INFO",
    //     subverb: "",
    //     prepare: super::info_verb::prepare_info,
    //     usage: concat!(
    //         "INFO <file|folder|url> [JSON] [STATS] [CHECK] [LAYER <name>]  what a\n",
    //         "          GIS file, a folder or a point cloud holds, without importing it:\n",
    //         "          dataset, raster, band, layer and field records, or GDAL's own\n",
    //         "          JSON; STATS the bands' statistics, CHECK every value read\n",
    //         "          (INFO <id> describes an entity, when no file has that name)",
    //     ),
    //     takes: Some(super::info_verb::takes_info),
    // },
    // VerbEntry {
    //     verb: "REFS",
    //     subverb: "",
    //     prepare: super::refs_verb::prepare_refs,
    //     usage: concat!(
    //         "REFS [LIST] [JSON] | SHOW|HIDE|REMOVE|INFO <ref> | OPACITY <ref> <0..1>\n",
    //         "          | COLOR <ref> elevation|intensity|classification|rgb|flat\n",
    //         "          | RENAME <ref> <name> | OVERVIEWS <ref> [levels=2,4,8] CONFIRM | RESTORE\n",
    //         "          the reference layers - rasters and point clouds - by id or name; the\n",
    //         "          project records their sources and display, and RESTORE (on OPEN)\n",
    //         "          reads them again",
    //     ),
    //     takes: None,
    // },
    // ---- I0 (G1): COPC ----
    VerbEntry {
        verb: "COPC",
        subverb: "",
        prepare: super::copc_verb::prepare_copc,
        usage: concat!(
            "COPC <source> <destination.copc.laz>  a point cloud rewritten as COPC,\n",
            "          every point kept",
        ),
        takes: None,
    },
    // ---- I2: FORMATS (G1) ----
    VerbEntry {
        verb: "FORMATS",
        subverb: "",
        prepare: super::formats_verbs::prepare_formats,
        usage: concat!(
            "FORMATS [RASTER|VECTOR] [READ|WRITE] [<text>...] [JSON]  the formats this GDAL\n",
            "          reads and writes, from its registry: driver, kinds, extensions, /vsi\n",
            "          FORMATS OPTIONS <driver> [JSON]  its open, creation and layer options",
        ),
        takes: None,
    },
    // ---- I3: IMPORT options (B) ----
    // ---- I4: EXPORT options (B) ----
    // ---- D1: INFO as data, STATS, CHECK (A) ----
    // ---- D2: REFS management (A) ----
];
