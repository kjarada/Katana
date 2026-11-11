//! The round-trip gate (docs/rust.md, "Step 3's round-trip gate"): every file
//! of the test data and the samples is read by the C++ katana_cli and by the
//! Rust katana_cli on the same lines, written by both where a writer exists,
//! and each program reads what the other wrote. Step 3 wrote it for the
//! formats it ported, with a stand-in for the Rust program; it now runs the
//! Rust katana_cli itself (this package's), and takes what steps 4 to 6 ported
//! as well: the GIS data, the zipped archive, the reference layers, SURVEY
//! IMPORT, the scripts and the files only a verb naming them reads.
//!
//! The corpus is the table CORPUS, one row per file, so that a new data file
//! is a decision rather than an accident: the first test fails for a file of
//! the test data or the samples that has no row, and a row says why a file is
//! left out. What each kind of row runs, in both programs, as katana_cli's -c
//! lines (a script as its argument):
//!
//! ```text
//! project   OPEN <copy>, SAVE, SAVE <new>        the copy (migrated, saved in place, its
//!                                                backups) and the new project compared
//! import    IMPORT <file>, SAVE <new>            the new project compared
//! survey    SURVEY READ <file>                   the replies compared
//!           SURVEY IMPORT <file>, SAVE <new>     the new project compared
//! script    <file>, SAVE <new>                   the new project compared
//! lines     the row's lines, SAVE <new>          the new project compared
//! ifc       EXPORT <file.ifc> UTILITIES ...      the files compared, then each program
//!                                                imports the other's
//! ```
//!
//! and after every row whose runs saved a project:
//!
//! ```text
//! the other's project   OPEN <the other's>, SAVE <new>    compared
//! each writer           OPEN <the C++'s project>, EXPORT <out.dxf | out.12da | out.ifc>
//!                                                         the files compared byte for byte
//! the other's file      IMPORT <the other's out>, SAVE <new>   compared
//! ```
//!
//! Every pair of runs is compared first by how each ended (both succeeding,
//! or both failing on the same error line, is an answer; anything else is a
//! finding) and then, where they ended alike, by what each wrote on standard
//! output and on standard error, word for word, with each run's own folders
//! and files (the project it saved, the file it wrote or read: two paths for
//! one thing) written by one name on both sides.
//!
//! Both programs export from the same project - the C++'s - because an IFC's
//! GlobalIds are made from the project's name and the time it was created
//! (ifc::guidNamespaceFor), which two saves never share. Projects are
//! compared by tools/project_dump.py, a dump made with Python's own sqlite3,
//! independent of both programs, which normalises the values two saves of
//! the same content cannot share (the clock's stamps, the writer's version,
//! the backups' names, the line ends of the stored SQL) and nothing else.
//! The files a writer writes are compared byte for byte - the writers are
//! ports, line for line - with the one value an IFC takes from the clock,
//! the time stamp in its header, set aside; and an IFC is judged by
//! tools/check_ifc.py too, where the Python can import IfcOpenShell, both
//! programs' files alike.
//!
//! The C++ katana_cli is the one tools/build_visible.sh built, named by
//! KATANA_CPP_CLI as the number-format oracle is by its variable; without it
//! the gate fails rather than skipping. The Rust katana_cli is cargo's build
//! of this package's program. Both run in ONE environment, which the gate
//! makes for every run (Gate::katana): the gate's scratch folder as the
//! working directory, a folder inside it as the temporary folder (TMPDIR, TMP
//! and TEMP, so that whatever either program leaves there goes with the
//! scratch), and neither KATANA_CUSTOMISATION nor KATANA_BUILTIN_CUSTOMISATION.
//! So a caller's kept file or choice of built-in reaches neither program, and
//! each starts as a person's katana_cli starts, with the built-in
//! customisation it was built with. Step 3 ran the C++ with the built-in
//! switched off (`none`) because its stand-in had no customisation; the Rust
//! has cad's now, embedded from the same file by the same declaration
//! (src/katana_cad/CMakeLists.txt, which katana_cad's build.rs reads), and
//! `none` would leave out what a customisation does to the corpus: the
//! customisations a SAVE records, an archive's colours resolved through the
//! customisation's palette, and SURVEY IMPORT's coding and linework by its
//! survey code rules. That the two start alike is checked before any row, by
//! a run of a note alone, so that a build configured with another built-in
//! (which reaches the C++ alone; docs/rust.md, "Not done") fails the gate in
//! one sentence rather than in every row. A difference is reported as the
//! file, the format, what was compared and the first difference.

use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The checkout's root, which the corpus paths are relative to.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../..");

// ---- the corpus -----------------------------------------------------------------------

/// What both programs do with a file.
#[derive(Clone, Copy)]
enum Read {
    /// A project's database: opened from a copy, saved in place and saved as
    /// a new project; then written by each writer.
    Project,
    /// A file IMPORT reads into a new drawing, saved as a project; then
    /// written by each writer.
    Import,
    /// A survey field file: read by SURVEY READ, whose replies are compared;
    /// then imported by SURVEY IMPORT (the reduction, and the coding and
    /// linework of the customisation the session starts with) into a new
    /// drawing, saved as a project and written by each writer.
    Survey,
    /// A katana_cli script, run as the program's argument into a new
    /// drawing, saved as a project; then written by each writer.
    Script,
    /// A file that only a line naming it reads (a customisation, a design, a
    /// file of reduction settings) and the lines that read it, each {path} a
    /// file of the corpus read by a row of its own, the row's own file among
    /// them: run into a new drawing, saved as a project, then written by each
    /// writer.
    Lines(&'static [&'static str]),
    /// An IFC export whose input the line's files are - a schedule, its
    /// delivery schema, a project's rules - from a new drawing or from a
    /// project of the corpus (`project`, a row of its own). `arguments` is
    /// the rest of the line after the file written, each {path} a file of
    /// the corpus.
    IfcExport {
        project: Option<&'static str>,
        arguments: &'static str,
    },
    /// Not in the gate, and why.
    Out(&'static str),
}

#[derive(Clone, Copy)]
struct Row {
    /// From the checkout's root; for an Out row, a folder ending in '/' to
    /// leave out everything in it.
    path: &'static str,
    /// For the report.
    format: &'static str,
    read: Read,
}

const fn project(path: &'static str) -> Row {
    Row {
        path,
        format: "project",
        read: Read::Project,
    }
}

const fn import(path: &'static str, format: &'static str) -> Row {
    Row {
        path,
        format,
        read: Read::Import,
    }
}

const fn survey(path: &'static str, format: &'static str) -> Row {
    Row {
        path,
        format,
        read: Read::Survey,
    }
}

const fn script(path: &'static str) -> Row {
    Row {
        path,
        format: "script",
        read: Read::Script,
    }
}

const fn read_by(path: &'static str, format: &'static str, lines: &'static [&'static str]) -> Row {
    Row {
        path,
        format,
        read: Read::Lines(lines),
    }
}

const fn ifc_export(
    path: &'static str,
    project: Option<&'static str>,
    arguments: &'static str,
) -> Row {
    Row {
        path,
        format: "ifc",
        read: Read::IfcExport { project, arguments },
    }
}

const fn out(path: &'static str, format: &'static str, why: &'static str) -> Row {
    Row {
        path,
        format,
        read: Read::Out(why),
    }
}

const LEGACY: &str = "a file of the retired style library and survey code file formats, whose legacy readers are not ported until the owner decides (docs/rust.md, \"Decisions waiting on the owner\")";
const GIT: &str = "git's settings for the folder, not data";
const SERVICE: &str = "a web service's capabilities document, which describes a server's data and holds none: IMPORT would hand it to GDAL, whose service drivers (WMS, WMTS, WFS) take such a document as the way to the server it names and read from there, and the gate reaches no host (CLAUDE.md section 4); the online discovery reads these files, in tests/interop's own cases";

const CORPUS: &[Row] = &[
    // ---- projects (storage) --------------------------------------------------------------
    // The sample project, at schema 12: opened, it is backed up and migrated.
    project("samples/site_plan/project.db"),
    // Its backups, each the sample as an earlier schema had it (4, 4, 6, 6, 7, 7
    // and 8), opened as a project: the migrations from each.
    project("samples/site_plan/backups/project-20260920T172742-596.db"),
    project("samples/site_plan/backups/project-20260920T185953-849.db"),
    project("samples/site_plan/backups/project-20260920T185953-896.db"),
    project("samples/site_plan/backups/project-20260921T012821-244.db"),
    project("samples/site_plan/backups/project-20260921T012821-262.db"),
    project("samples/site_plan/backups/project-20260921T014629-888.db"),
    project("samples/site_plan/backups/project-20260921T014629-905.db"),
    // A project main's katana_cli wrote before survey jobs, at schema 9.
    project("tests/storage/fixtures/schema9_main/project.db"),
    // ---- DXF -----------------------------------------------------------------------------
    import("tests/dxf/data/r12_survey.dxf", "dxf"),
    // Its DIMENSION is read as its picture block's lines and text, so nothing
    // in the drawing is the cad layer's to draw.
    import("tests/dxf/data/r2000_site.dxf", "dxf"),
    out("tests/dxf/data/.gitattributes", "git", GIT),
    // ---- the archive ---------------------------------------------------------------------
    // Its point cloud ("LiDAR Patch") is a reference layer of the session, as
    // las_point_cloud.12da's is below: SAVE records it in the project's
    // reference_layers and OPEN reads it again from the archive, so the key is
    // compared with the rest of the project.
    import("tests/archive12d/data/all_geometries.12da", "12da"),
    // The same archive in its zip container, which IMPORT opens through the
    // gis layer.
    import("tests/archive12d/data/all_geometries.12daz", "12daz"),
    out(
        "tests/archive12d/data/customisation/test_linestyles.4d",
        "style library",
        LEGACY,
    ),
    out(
        "tests/archive12d/data/customisation/test_survey.mapfile",
        "survey code file",
        LEGACY,
    ),
    out(
        "tests/archive12d/data/customisation/test_symbols.4d",
        "style library",
        LEGACY,
    ),
    // Its two drafting leaders are drawn into a DXF by the cad layer's export
    // annotation (cad/annotation/export_annotation.cpp).
    import("tests/archive12d/data/drafting_leaders.12da", "12da"),
    // The customisation whose colour table names that archive's own colours:
    // loaded, and the archive imported after it, as
    // tests/archive12d/check_drafting_import.cmake runs them for
    // cli.archive_custom_colours_and_native_leaders_are_imported, so that the
    // colours are resolved through this palette rather than the built-in's.
    read_by(
        "tests/archive12d/data/drafting_palette.customisation.json",
        "customisation",
        &[
            "CUSTOMISE {tests/archive12d/data/drafting_palette.customisation.json}",
            "IMPORT {tests/archive12d/data/drafting_leaders.12da} LOCAL",
        ],
    ),
    import("tests/archive12d/data/drainage_strings.12da", "12da"),
    // Its LAS cloud is a reference layer of the session, which a SAVE records
    // in the project's reference_layers, compared with the rest.
    import("tests/archive12d/data/las_point_cloud.12da", "12da"),
    import("tests/archive12d/data/multiple_tins.12da", "12da"),
    import("tests/archive12d/data/super_tin.12da", "12da"),
    import("tests/archive12d/data/symbols.12da", "12da"),
    // ---- IFC -----------------------------------------------------------------------------
    // The corpus holds no .ifc: every row above writes one and reads the
    // other program's back, and these write the files an IFC export takes.
    ifc_export(
        "samples/utilities/schedule.csv",
        None,
        "UTILITIES {samples/utilities/schedule.csv}",
    ),
    // The schedule written to a delivery schema, first by its own columns...
    ifc_export(
        "samples/utilities/schedule_tfnsw.csv",
        None,
        "UTILITIES {samples/utilities/schedule_tfnsw.csv}",
    ),
    // ... then with the schema, and nothing of the drawing.
    ifc_export(
        "tests/ifc/data/delivery_schema.csv",
        None,
        "UTILITIES {samples/utilities/schedule_tfnsw.csv} SCHEMA {tests/ifc/data/delivery_schema.csv} NODRAWING",
    ),
    // A project's own classification rules, tried before the defaults.
    ifc_export(
        "samples/ifc/classification_rules.csv",
        Some("samples/site_plan/project.db"),
        "RULES {samples/ifc/classification_rules.csv}",
    ),
    // ---- scripts -------------------------------------------------------------------------
    // The road centreline and the survey beside it that the IFC samples are
    // exported from (docs/ifc.md): its project coordinate system, alignment,
    // profile, kerb, fence, pit, point and text, then each writer.
    script("samples/ifc/scenario.txt"),
    // The sample site plan: layers, polylines, arcs, circles, text,
    // dimensions, hatches and an alignment with its design profile, ending in
    // LIST.
    script("samples/site_plan.kcs"),
    // The window's scripts (File > Run Script) are katana_cli's scripts too.
    // Three commands, three entities; an indented note is a note.
    script("tests/qt_widgets/data/scripts/good.kcs"),
    // Its second command is refused (a circle of negative radius), which ends
    // a katana_cli run: both programs must fail on that line alike.
    script("tests/qt_widgets/data/scripts/bad.kcs"),
    // Its VIEWS and ZOOM lines are a window's, which katana_cli has not: both
    // programs must refuse the first of them alike.
    script("tests/qt_widgets/data/scripts/open_and_zoom.kcs"),
    // ---- the subsurface utilities --------------------------------------------------------
    // The design UTILITY CLEARANCE checks the sample schedule's services
    // against, as cli.utility_clearance_shows_the_crossing_within_tolerance
    // runs it: a report, compared with the replies.
    read_by(
        "samples/utilities/design.csv",
        "csv",
        &[
            "UTILITY CLEARANCE {samples/utilities/schedule.csv} {samples/utilities/design.csv} WIDTH 0.375",
        ],
    ),
    // The sample investigation as a GIS delivers it (plan strings, points shot
    // on the ground, the attributes under the GIS's names), which
    // cli.utility_draw_takes_the_services_an_import_left_in_the_drawing
    // imports: vector data, read through GDAL.
    import("samples/utilities/located_services.geojson", "geojson"),
    // ---- GIS data (the gis and pointcloud layers) ----------------------------------------
    import("samples/gis/parcels.geojson", "geojson"),
    // A point cloud, read through PDAL: a reference layer of the session, not
    // the drawing's, which SAVE records in reference_layers and OPEN reads
    // again from the file.
    import("samples/gis/survey_scan.las", "las"),
    // A raster, read through GDAL: a reference layer, as the cloud is.
    import("samples/gis/terrain.asc", "asc"),
    out(
        "samples/gis/terrain.prj",
        "prj",
        "the raster's coordinate system, which GDAL reads with samples/gis/terrain.asc (its row is above) and never alone",
    ),
    // The geoprocessing suite's data: a lot with a hole, two lots, the plane
    // (a raster with no coordinate system) and points on it.
    import("tests/geo/data/lot_with_hole.geojson", "geojson"),
    import("tests/geo/data/lots.geojson", "geojson"),
    import("tests/geo/data/plane.asc", "asc"),
    import("tests/geo/data/plane_points.geojson", "geojson"),
    // ---- the replies of online services, as the online tests keep them -------------------
    // JSON that no reader of GDAL takes (an error, an image service's and a
    // catalogue's descriptions, a feature service's list of collections):
    // both programs must refuse IMPORT of it alike.
    import("tests/interop/data/online/arcgis_error.json", "json"),
    import("tests/interop/data/online/arcgis_imageserver.json", "json"),
    import("tests/interop/data/online/ckan_search.json", "json"),
    import("tests/interop/data/online/oapif_collections.json", "json"),
    // A feature layer's and a map service's descriptions, which GDAL's ESRI
    // JSON reader claims by the geometry type they name and which hold no
    // features: whatever it makes of them, both programs must make the same.
    import("tests/interop/data/online/arcgis_layer.json", "json"),
    import("tests/interop/data/online/arcgis_mapserver.json", "json"),
    // Pages of features as the services return them: GeoJSON.
    import(
        "tests/interop/data/online/arcgis_query_page1.geojson",
        "geojson",
    ),
    import(
        "tests/interop/data/online/arcgis_query_page2.geojson",
        "geojson",
    ),
    import(
        "tests/interop/data/online/arcgis_query_page3.geojson",
        "geojson",
    ),
    import(
        "tests/interop/data/online/oapif_items_page1.json",
        "geojson",
    ),
    // OpenStreetMap's XML as Overpass returns it, read by GDAL's OSM driver.
    import("tests/interop/data/online/overpass_buildings.osm", "osm"),
    // A STAC item and a page of a STAC search, read as the GeoJSON they are.
    // GDAL's STAC readers, which would open the remote assets they name, take
    // only a document that carries the projection extension's transform,
    // which these do not.
    import(
        "tests/interop/data/online/stac_item_sentinel2.json",
        "geojson",
    ),
    import("tests/interop/data/online/stac_search.json", "geojson"),
    out(
        "tests/interop/data/online/headless_catalogue.json.in",
        "template",
        "a template the build fills in (tests/interop/CMakeLists.txt) for the window's headless check of the online catalogue: not data",
    ),
    out(
        "tests/interop/data/online/wcs_capabilities_100.xml",
        "service",
        SERVICE,
    ),
    out(
        "tests/interop/data/online/wfs_capabilities_200.xml",
        "service",
        SERVICE,
    ),
    out(
        "tests/interop/data/online/wms_capabilities_111.xml",
        "service",
        SERVICE,
    ),
    out(
        "tests/interop/data/online/wms_capabilities_130.xml",
        "service",
        SERVICE,
    ),
    out(
        "tests/interop/data/online/wmts_capabilities.xml",
        "service",
        SERVICE,
    ),
    // ---- reduction settings for SURVEY IMPORT --------------------------------------------
    // CP1 held where the drawing has it, read by SETTINGS after the point is
    // placed, as cli.survey_import_takes_its_settings_from_a_file runs it
    // (its last key is one a later Katana might write, skipped and said).
    read_by(
        "tests/app/data/survey_settings/hold_cp1.txt",
        "settings",
        &[
            "FORWARD 500000,4999999,100 0 1 0 CP1",
            "SURVEY IMPORT {tests/surveyio/data/sdr/traverse_without_coordinates.sdr} SETTINGS {tests/app/data/survey_settings/hold_cp1.txt}",
        ],
    ),
    // Settings of a later version, refused whole: both programs must refuse
    // them alike (cli.survey_import_refuses_settings_from_a_newer_katana).
    read_by(
        "tests/app/data/survey_settings/newer_version.txt",
        "settings",
        &[
            "FORWARD 500000,4999999,100 0 1 0 CP1",
            "SURVEY IMPORT {tests/surveyio/data/sdr/traverse_without_coordinates.sdr} SETTINGS {tests/app/data/survey_settings/newer_version.txt}",
        ],
    ),
    // ---- survey field files (surveyio): read, then imported ------------------------------
    survey("samples/Survey_File/Example field file1.fld", "fld"),
    survey("samples/Survey_File/Example field file2.fld", "fld"),
    survey("samples/Survey_File/Example field file3.fld", "fld"),
    // Prose about the field file format: whatever detection makes of it, both
    // programs must make the same.
    survey(
        "samples/Survey_File/Structure of the 12dfield. or fld .txt",
        "txt",
    ),
    survey("samples/Survey_File/example sdr.sdr", "sdr"),
    // GSI-16 records under a .txt name.
    survey("samples/Survey_File/example.txt", "txt"),
    out("tests/surveyio/data/fld/.gitattributes", "git", GIT),
    survey("tests/surveyio/data/fld/free_station.fld", "fld"),
    survey("tests/surveyio/data/fld/gnss.fld", "fld"),
    survey("tests/surveyio/data/fld/resection.fld", "fld"),
    survey("tests/surveyio/data/fld/rtk_setup.fld", "fld"),
    survey("tests/surveyio/data/fld/setup.fld", "fld"),
    survey("tests/surveyio/data/fld/two_backsights.fld", "fld"),
    survey("tests/surveyio/data/gts/setup.gt7", "gt7"),
    survey("tests/surveyio/data/leica/codes_after_gsi8.gsi", "gsi"),
    survey("tests/surveyio/data/leica/codes_before_gsi8.gsi", "gsi"),
    survey("tests/surveyio/data/leica/coordinates_gsi8.gsi", "gsi"),
    survey("tests/surveyio/data/leica/damaged_gsi8.gsi", "gsi"),
    survey(
        "tests/surveyio/data/leica/derived_distances_gsi8.gsi",
        "gsi",
    ),
    survey("tests/surveyio/data/leica/feet_mil_gsi8.gsi", "gsi"),
    survey(
        "tests/surveyio/data/leica/negative_vertical_gsi8.gsi",
        "gsi",
    ),
    survey("tests/surveyio/data/leica/rounded_seconds_gsi16.gsi", "gsi"),
    survey("tests/surveyio/data/leica/tps_gsi16.gsi", "gsi"),
    survey("tests/surveyio/data/leica/tps_gsi8.gsi", "gsi"),
    // A DBX job is a folder of files that name each other: each read alone,
    // with its folder for the others.
    survey("tests/surveyio/data/leica/dbx/JOB1.X01", "dbx"),
    survey("tests/surveyio/data/leica/dbx/JOB1.XCF", "dbx"),
    // RINEX observations, compact and plain, with their navigation files
    // beside them, and the navigation files read alone.
    survey(
        "tests/surveyio/data/rinex/BASE00AUS_R_20242561200_01H_30S_MO.crx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/BASE00AUS_R_20242561200_01H_30S_MO.rnx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/BASE00AUS_R_20242561200_01H_MN.rnx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/ROVR00AUS_R_20242561100_01H_01S_GO.crx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/ROVR00AUS_R_20242561100_01H_01S_GO.rnx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/TEST00AUS_R_20242561000_01H_30S_MO.crx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/TEST00AUS_R_20242561000_01H_30S_MO.rnx",
        "rinex",
    ),
    survey("tests/surveyio/data/rinex/rinex305_example1.crx", "rinex"),
    survey("tests/surveyio/data/rinex/rinex305_example1.rnx", "rinex"),
    survey("tests/surveyio/data/rinex/test2560.24d", "rinex"),
    survey("tests/surveyio/data/rinex/test2560.24n", "rinex"),
    survey("tests/surveyio/data/rinex/test2560.24o", "rinex"),
    survey("tests/surveyio/data/rw5/feet_grads.rw5", "rw5"),
    survey("tests/surveyio/data/rw5/gnss.rw5", "rw5"),
    survey("tests/surveyio/data/rw5/journal.rw5", "rw5"),
    survey("tests/surveyio/data/rw5/setup_metres.rw5", "rw5"),
    survey("tests/surveyio/data/rw5/survce250_us_feet.rw5", "rw5"),
    survey("tests/surveyio/data/sdr/traverse.sdr", "sdr"),
    survey(
        "tests/surveyio/data/sdr/traverse_without_coordinates.sdr",
        "sdr",
    ),
    // Coordinate files: SURVEY READ refuses one - it is read through its
    // column layout - and both programs must refuse it alike, and import it
    // alike.
    survey("tests/surveyio/data/survey_points_headerless.txt", "points"),
    survey(
        "tests/surveyio/data/survey_points_headerless_penz.txt",
        "points",
    ),
    survey("tests/surveyio/data/survey_points_pnezd.csv", "points"),
    survey(
        "tests/surveyio/data/survey_points_pnezd_utm30s.csv",
        "points",
    ),
    survey("tests/surveyio/data/trimble_dc/synthetic_job.dc", "dc"),
    survey("tests/surveyio/data/trimble_jxl/one_setup.jxl", "jxl"),
    survey("tests/surveyio/data/trimble_jxl/tps_gnss_job.jxl", "jxl"),
];

/// The folders whose every file is the corpus's: the test data of each
/// suite, the samples and the storage fixtures.
fn corpus_folders(root: &Path) -> Vec<PathBuf> {
    let mut folders = vec![root.join("samples"), root.join("tests/storage/fixtures")];
    let suites = std::fs::read_dir(root.join("tests")).expect("the checkout's tests/ folder");
    for suite in suites {
        let data = suite.expect("an entry of tests/").path().join("data");
        if data.is_dir() {
            folders.push(data);
        }
    }
    folders
}

fn files_under(folder: &Path, into: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(folder)
        .unwrap_or_else(|error| panic!("{} cannot be listed: {error}", folder.display()));
    for entry in entries {
        let path = entry.expect("a folder's entry").path();
        if path.is_dir() {
            files_under(&path, into);
        } else {
            into.push(path);
        }
    }
}

/// A path under the checkout as the table spells it.
fn relative(root: &Path, path: &Path) -> String {
    let inside = path.strip_prefix(root).unwrap_or(path);
    inside
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// What a run of a program leaves beside the corpus, which is no corpus
/// file. git ignores samples/*/backups/ (.gitignore): running the program on
/// a sample writes a backup there, and the backups git tracks have rows of
/// their own. GDAL's persistent auxiliary metadata, `<file>.aux.xml` beside
/// a raster a row reads, is written by a run that asks GDAL for statistics:
/// a cache of the file, never data (.gitignore ignores the samples' own, and
/// docs/geoprocessing.md, "Not done", records one left beside
/// tests/geo/data/plane.asc). That a run leaves one is for `git status` after
/// the suite to show, not for this table.
fn is_written_by_a_run(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    if parts.len() == 4 && parts[0] == "samples" && parts[2] == "backups" {
        return true;
    }
    relative
        .strip_suffix(".aux.xml")
        .is_some_and(|data| CORPUS.iter().any(|row| row.path == data))
}

/// The {path}s a row's line names.
fn named_files(arguments: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = arguments;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        found.push(&rest[open + 1..open + close]);
        rest = &rest[open + close + 1..];
    }
    found
}

/// Whether a file of the corpus is read by a row of its own.
fn read_by_a_row(path: &str) -> bool {
    CORPUS
        .iter()
        .any(|other| other.path == path && !matches!(other.read, Read::Out(_)))
}

fn checkout_root() -> PathBuf {
    std::fs::canonicalize(ROOT).expect("the checkout's root")
}

#[test]
fn the_corpus_table_has_a_row_for_every_file_of_the_test_data_and_the_samples() {
    let root = checkout_root();
    let mut files = Vec::new();
    for folder in corpus_folders(&root) {
        files_under(&folder, &mut files);
    }
    let mut problems = Vec::new();
    for file in &files {
        let name = relative(&root, file);
        let listed = CORPUS
            .iter()
            .any(|row| row.path == name || (row.path.ends_with('/') && name.starts_with(row.path)));
        if !listed && !is_written_by_a_run(&name) {
            problems.push(format!(
                "{name} has no row: add one that reads it, or says why it is left out"
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for row in CORPUS {
        if !seen.insert(row.path) {
            problems.push(format!("{} has two rows", row.path));
        }
        if !root.join(row.path).exists() {
            problems.push(format!("{} has a row and is not there", row.path));
        }
        match row.read {
            Read::Out(why) if why.trim().is_empty() => {
                problems.push(format!("{} is left out without saying why", row.path));
            }
            Read::IfcExport { project, arguments } => {
                for named in named_files(arguments).into_iter().chain(project) {
                    if !read_by_a_row(named) {
                        problems.push(format!(
                            "{}'s export names {named}, which is no file of the corpus read by a row of its own",
                            row.path
                        ));
                    }
                }
            }
            Read::Lines(lines) => {
                let named: Vec<&str> = lines.iter().copied().flat_map(named_files).collect();
                if !named.contains(&row.path) {
                    problems.push(format!(
                        "{}'s lines do not name it: a row of lines reads its own file",
                        row.path
                    ));
                }
                for each in named {
                    if !read_by_a_row(each) {
                        problems.push(format!(
                            "{}'s lines name {each}, which is no file of the corpus read by a row of its own",
                            row.path
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    assert!(
        problems.is_empty(),
        "the corpus table is out of step with the corpus ({} files):\n{}",
        files.len(),
        problems.join("\n")
    );
}

// ---- the programs ---------------------------------------------------------------------

/// What a program did: whether it succeeded, and what it wrote.
struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Run {
    /// The line saying why it failed: the LAST error line. katana_cli stops
    /// at the first line that fails, so nothing is run after it; an error
    /// line before it is the start's (a customisation that could not be
    /// read), which the start check compares once for every row.
    fn error(&self) -> String {
        self.stderr
            .lines()
            .rev()
            .find(|line| line.starts_with("error: "))
            .map_or_else(
                || format!("(no error line; standard error: {:?})", self.stderr.trim()),
                String::from,
            )
    }

    /// How it ended, for a report.
    fn ending(&self) -> String {
        if self.ok {
            String::from("succeeds")
        } else {
            format!("fails: {}", self.error())
        }
    }
}

fn run(command: &mut Command) -> Run {
    match command.output() {
        Ok(output) => Run {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(error) => Run {
            ok: false,
            stdout: String::new(),
            stderr: format!("error: the program could not be started: {error}"),
        },
    }
}

/// A katana_cli line of a verb and a path, quoted.
fn line(verb: &str, path: &Path) -> String {
    format!("{verb} \"{}\"", path.display())
}

struct Gate {
    root: PathBuf,
    cpp: PathBuf,
    rust: PathBuf,
    python: OsString,
    ifcopenshell: bool,
    /// The working directory of every run, and the parent of every row's
    /// folder.
    scratch: PathBuf,
    /// The temporary folder of every run, inside `scratch`.
    temporary: PathBuf,
}

impl Gate {
    /// One run of a program, in the one environment both run in (the
    /// module's head says why): `script` as its argument where there is one,
    /// then each line as a -c, in order, as katana_cli runs them.
    fn katana(&self, program: &Path, script: Option<&Path>, lines: &[String]) -> Run {
        let mut command = Command::new(program);
        if let Some(script) = script {
            command.arg(script);
        }
        for each in lines {
            command.arg("-c").arg(each);
        }
        command
            .env_remove("KATANA_CUSTOMISATION")
            .env_remove("KATANA_BUILTIN_CUSTOMISATION")
            .env("TMPDIR", &self.temporary)
            .env("TMP", &self.temporary)
            .env("TEMP", &self.temporary)
            .current_dir(&self.scratch);
        run(&mut command)
    }

    fn cpp(&self, lines: &[String]) -> Run {
        self.katana(&self.cpp, None, lines)
    }

    fn rust(&self, lines: &[String]) -> Run {
        self.katana(&self.rust, None, lines)
    }

    fn tool(&self, name: &str) -> PathBuf {
        self.root.join("tools").join(name)
    }

    fn corpus(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    /// tools/project_dump.py's dump of a project.
    fn dump(&self, project: &Path) -> Result<String, String> {
        let ran = run(Command::new(&self.python)
            .arg(self.tool("project_dump.py"))
            .arg(project));
        if ran.ok {
            Ok(ran.stdout)
        } else {
            Err(format!(
                "tools/project_dump.py fails on {}: {}",
                project.display(),
                ran.stderr.trim()
            ))
        }
    }

    /// tools/check_ifc.py's judgement of a file: its status and its words,
    /// the file's own path taken out of them.
    fn judge_ifc(&self, file: &Path) -> (bool, String) {
        let ran = run(Command::new(&self.python)
            .arg(self.tool("check_ifc.py"))
            .arg(file));
        let named = format!("{}", file.display());
        (ran.ok, ran.stdout.replace(named.as_str(), "<file>"))
    }

    /// A row's line with each {path} the corpus file, quoted.
    fn with_corpus_paths(&self, arguments: &str) -> String {
        let mut text = String::from(arguments);
        for named in named_files(arguments) {
            text = text.replace(
                &format!("{{{named}}}"),
                &format!("\"{}\"", self.corpus(named).display()),
            );
        }
        text
    }
}

/// A copy of a project's database as the project.db of `directory`.
fn place_project(database: &Path, directory: &Path) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .and_then(|()| std::fs::copy(database, directory.join("project.db")))
        .map(|_| ())
        .map_err(|error| {
            format!(
                "{} cannot be copied to {}: {error}",
                database.display(),
                directory.display()
            )
        })
}

/// What tells the two programs apart before any row: a run of a note alone,
/// which writes nothing but what a session says as it starts (the
/// customisation it starts with, and anything it could not read). None when
/// they start alike.
fn start_difference(gate: &Gate) -> Option<String> {
    let note = [String::from(
        "# the round-trip gate's first run: a session's start, and nothing else",
    )];
    let (cpp, rust) = (gate.cpp(&note), gate.rust(&note));
    if !cpp.ok || !rust.ok {
        return Some(format!(
            "a run of a note alone: the C++ {}, the Rust {}",
            cpp.ending(),
            rust.ending()
        ));
    }
    first_difference(&cpp.stdout, &rust.stdout)
        .map(|difference| format!("on standard output, {difference}"))
        .or_else(|| {
            first_difference(&cpp.stderr, &rust.stderr)
                .map(|difference| format!("on standard error, {difference}"))
        })
}

// ---- comparing ------------------------------------------------------------------------

/// What one row found, each a line naming the file, its format, what was
/// compared and the first difference.
struct Findings<'a> {
    row: &'a Row,
    list: Vec<String>,
}

impl Findings<'_> {
    fn add(&mut self, what: &str, difference: &str) {
        self.list.push(format!(
            "{} ({}): {what}: {difference}",
            self.row.path, self.row.format
        ));
    }
}

fn shown(line: Option<&str>) -> String {
    match line {
        None => String::from("(the end)"),
        Some(line) => {
            let cut: String = line.chars().take(300).collect();
            format!("{cut:?}")
        }
    }
}

/// The first line two texts differ at; None when they are the same.
fn first_difference(cpp: &str, rust: &str) -> Option<String> {
    if cpp == rust {
        return None;
    }
    let mut cpp_lines = cpp.lines();
    let mut rust_lines = rust.lines();
    let mut number = 1;
    loop {
        match (cpp_lines.next(), rust_lines.next()) {
            (Some(a), Some(b)) if a == b => number += 1,
            (None, None) => {
                return Some(format!(
                    "the same lines, ended differently ({} and {} bytes)",
                    cpp.len(),
                    rust.len()
                ));
            }
            (a, b) => {
                return Some(format!(
                    "line {number}: the C++ has {}, the Rust {}",
                    shown(a),
                    shown(b)
                ));
            }
        }
    }
}

/// `text` with each of a run's own folders and files written by the one name
/// both runs' are given. A step's lines name two paths for one thing (the
/// project each program saves, the file each writes or reads), and a reply
/// says a path as its line gave it (`saved to <path>`, `opened <path>`,
/// `file=<path>`). The longest path first, so that none is cut out of a
/// longer one; and each also with '/' for '\', the form a record gives a path
/// on Windows.
fn named(text: &str, names: &[(&Path, &str)]) -> String {
    let mut paths: Vec<(String, &str)> = names
        .iter()
        .map(|&(path, name)| (format!("{}", path.display()), name))
        .collect();
    paths.sort_by_key(|(path, _)| Reverse(path.len()));
    let mut out = String::from(text);
    for (path, name) in &paths {
        out = out.replace(path.as_str(), name);
        if path.contains('\\') {
            out = out.replace(path.replace('\\', "/").as_str(), name);
        }
    }
    out
}

/// Two runs of one step compared. How each ended: both succeeding, or both
/// failing on the same error line, is an answer; anything else is a finding.
/// Then, where they ended alike, what each wrote on standard output and on
/// standard error, word for word. `names` gives each path the step's lines
/// name: the C++'s, the Rust's, and the one name both are written as before
/// anything is compared. True when both succeeded, for the comparisons after.
fn same_step(
    findings: &mut Findings<'_>,
    what: &str,
    cpp: &Run,
    rust: &Run,
    names: &[(&Path, &Path, &str)],
) -> bool {
    let cpp_names: Vec<(&Path, &str)> = names.iter().map(|&(path, _, name)| (path, name)).collect();
    let rust_names: Vec<(&Path, &str)> =
        names.iter().map(|&(_, path, name)| (path, name)).collect();
    let cpp_error = named(&cpp.error(), &cpp_names);
    let rust_error = named(&rust.error(), &rust_names);
    match (cpp.ok, rust.ok) {
        (true, false) => {
            findings.add(
                what,
                &format!("the C++ succeeds and the Rust fails: {rust_error}"),
            );
            return false;
        }
        (false, true) => {
            findings.add(
                what,
                &format!("the Rust succeeds and the C++ fails: {cpp_error}"),
            );
            return false;
        }
        (false, false) if cpp_error != rust_error => {
            findings.add(
                what,
                &format!("both fail, differently: the C++ {cpp_error:?}, the Rust {rust_error:?}"),
            );
            return false;
        }
        _ => {}
    }
    for (stream, cpp_text, rust_text) in [
        ("standard output", &cpp.stdout, &rust.stdout),
        ("standard error", &cpp.stderr, &rust.stderr),
    ] {
        if let Some(difference) =
            first_difference(&named(cpp_text, &cpp_names), &named(rust_text, &rust_names))
        {
            findings.add(&format!("{what}: what each wrote on {stream}"), &difference);
        }
    }
    cpp.ok
}

fn same_project(gate: &Gate, findings: &mut Findings<'_>, what: &str, cpp: &Path, rust: &Path) {
    match (gate.dump(cpp), gate.dump(rust)) {
        (Ok(a), Ok(b)) => {
            if let Some(difference) = first_difference(&a, &b) {
                findings.add(what, &difference);
            }
        }
        (Err(why), _) | (_, Err(why)) => findings.add(what, &why),
    }
}

/// An IFC header's time stamp - FILE_NAME's second value, which each program
/// takes from its own clock (ifc_verbs.cpp: headerTimestamp of now) - set to
/// zeros; any other file as it is.
fn without_header_time(bytes: &[u8]) -> Vec<u8> {
    const MARKER: &[u8] = b"FILE_NAME('";
    const STAMP: &[u8] = b"0000-00-00T00:00:00";
    let mut out = bytes.to_vec();
    let Some(start) = bytes
        .windows(MARKER.len())
        .position(|window| window == MARKER)
    else {
        return out;
    };
    // Past the file's name, a STEP string in which '' is one quote.
    let mut at = start + MARKER.len();
    while at < bytes.len() {
        if bytes[at] == b'\'' {
            if bytes.get(at + 1) == Some(&b'\'') {
                at += 2;
                continue;
            }
            break;
        }
        at += 1;
    }
    let stamp = at + 3;
    if bytes.get(at..stamp) == Some(b"','".as_slice())
        && bytes.get(stamp + STAMP.len()) == Some(&b'\'')
    {
        out[stamp..stamp + STAMP.len()].copy_from_slice(STAMP);
    }
    out
}

/// A file's text for the report: UTF-16 little-endian behind its byte order
/// mark (a .12da), else UTF-8.
fn text_of_file(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xff\xfe") {
        let (pairs, _) = rest.as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn same_file(findings: &mut Findings<'_>, what: &str, cpp: &Path, rust: &Path) {
    let (Ok(a), Ok(b)) = (std::fs::read(cpp), std::fs::read(rust)) else {
        findings.add(what, "a file to compare cannot be read");
        return;
    };
    let (a, b) = (without_header_time(&a), without_header_time(&b));
    if a != b {
        let difference = first_difference(&text_of_file(&a), &text_of_file(&b))
            .unwrap_or_else(|| String::from("the bytes differ where their text does not"));
        findings.add(what, &difference);
    }
}

/// tools/check_ifc.py on both files, where IfcOpenShell is importable: the
/// two must be judged alike - not necessarily valid, which is the C++ suite's
/// to hold, but the same.
fn same_ifc_judgement(gate: &Gate, findings: &mut Findings<'_>, cpp: &Path, rust: &Path) {
    if !gate.ifcopenshell {
        return;
    }
    let (cpp_ok, cpp_words) = gate.judge_ifc(cpp);
    let (rust_ok, rust_words) = gate.judge_ifc(rust);
    if cpp_ok != rust_ok || cpp_words != rust_words {
        let difference = first_difference(&cpp_words, &rust_words)
            .unwrap_or_else(|| format!("the C++'s passes: {cpp_ok}, the Rust's: {rust_ok}"));
        findings.add(
            "tools/check_ifc.py's judgement of the two .ifc",
            &difference,
        );
    }
}

// ---- the rows -------------------------------------------------------------------------

/// Each program imports a file into a new drawing and saves it as a project:
/// the C++ `cpp_reads`, the Rust `rust_reads`.
fn each_reads(
    gate: &Gate,
    findings: &mut Findings<'_>,
    what: &str,
    folder: &Path,
    cpp_reads: &Path,
    rust_reads: &Path,
) {
    let (cpp_saved, rust_saved) = (folder.join("cpp/read"), folder.join("rust/read"));
    let cpp = gate.cpp(&[line("IMPORT", cpp_reads), line("SAVE", &cpp_saved)]);
    let rust = gate.rust(&[line("IMPORT", rust_reads), line("SAVE", &rust_saved)]);
    let names = [
        (cpp_reads, rust_reads, "<file>"),
        (cpp_saved.as_path(), rust_saved.as_path(), "<project>"),
    ];
    if same_step(
        findings,
        &format!("IMPORT of {what} and SAVE <new>"),
        &cpp,
        &rust,
        &names,
    ) {
        same_project(
            gate,
            findings,
            &format!("the project each program saves of {what}"),
            &cpp_saved,
            &rust_saved,
        );
    }
}

/// One writer, from the C++'s project, which both programs open; then each
/// reads the other's file.
fn check_writer(
    gate: &Gate,
    findings: &mut Findings<'_>,
    work: &Path,
    extension: &str,
    project: &Path,
) {
    let folder = work.join(extension);
    let (cpp_source, rust_source) = (folder.join("cpp/source"), folder.join("rust/source"));
    let file = format!("out.{extension}");
    let (cpp_file, rust_file) = (
        folder.join("cpp").join(file.as_str()),
        folder.join("rust").join(file.as_str()),
    );
    let database = project.join("project.db");
    if let Err(why) =
        place_project(&database, &cpp_source).and_then(|()| place_project(&database, &rust_source))
    {
        findings.add("the copies", &why);
        return;
    }
    let cpp = gate.cpp(&[line("OPEN", &cpp_source), line("EXPORT", &cpp_file)]);
    let rust = gate.rust(&[line("OPEN", &rust_source), line("EXPORT", &rust_file)]);
    let names = [
        (cpp_source.as_path(), rust_source.as_path(), "<opened>"),
        (cpp_file.as_path(), rust_file.as_path(), "<file>"),
    ];
    if !same_step(
        findings,
        &format!("OPEN and EXPORT .{extension}"),
        &cpp,
        &rust,
        &names,
    ) {
        return;
    }
    same_file(
        findings,
        &format!("the .{extension} both write"),
        &cpp_file,
        &rust_file,
    );
    if extension == "ifc" {
        same_ifc_judgement(gate, findings, &cpp_file, &rust_file);
    }
    each_reads(
        gate,
        findings,
        &format!("the other's .{extension}"),
        &folder,
        &rust_file,
        &cpp_file,
    );
}

/// After a row whose runs saved a project: each program opens the other's
/// project and saves it as a new one, then each writer.
fn check_writers(
    gate: &Gate,
    findings: &mut Findings<'_>,
    work: &Path,
    cpp_project: &Path,
    rust_project: &Path,
) {
    let (cpp_reads, rust_reads) = (work.join("cpp/reads-rust"), work.join("rust/reads-cpp"));
    let (cpp_opened, rust_opened) = (cpp_reads.join("opened"), rust_reads.join("opened"));
    let (cpp_saved, rust_saved) = (cpp_reads.join("saved"), rust_reads.join("saved"));
    match place_project(&rust_project.join("project.db"), &cpp_opened)
        .and_then(|()| place_project(&cpp_project.join("project.db"), &rust_opened))
    {
        Err(why) => findings.add("the copies", &why),
        Ok(()) => {
            let cpp = gate.cpp(&[line("OPEN", &cpp_opened), line("SAVE", &cpp_saved)]);
            let rust = gate.rust(&[line("OPEN", &rust_opened), line("SAVE", &rust_saved)]);
            let names = [
                (cpp_opened.as_path(), rust_opened.as_path(), "<opened>"),
                (cpp_saved.as_path(), rust_saved.as_path(), "<project>"),
            ];
            if same_step(
                findings,
                "OPEN of the other's project and SAVE <new>",
                &cpp,
                &rust,
                &names,
            ) {
                same_project(
                    gate,
                    findings,
                    "the project each program saves of the other's",
                    &cpp_saved,
                    &rust_saved,
                );
            }
        }
    }
    for extension in ["dxf", "12da", "ifc"] {
        check_writer(gate, findings, work, extension, cpp_project);
    }
}

fn check_project(gate: &Gate, findings: &mut Findings<'_>, work: &Path) {
    let database = gate.corpus(findings.row.path);
    let (cpp_opened, rust_opened) = (work.join("cpp/opened"), work.join("rust/opened"));
    let (cpp_saved, rust_saved) = (work.join("cpp/saved"), work.join("rust/saved"));
    if let Err(why) =
        place_project(&database, &cpp_opened).and_then(|()| place_project(&database, &rust_opened))
    {
        findings.add("the copies", &why);
        return;
    }
    let cpp = gate.cpp(&[
        line("OPEN", &cpp_opened),
        String::from("SAVE"),
        line("SAVE", &cpp_saved),
    ]);
    let rust = gate.rust(&[
        line("OPEN", &rust_opened),
        String::from("SAVE"),
        line("SAVE", &rust_saved),
    ]);
    let names = [
        (cpp_opened.as_path(), rust_opened.as_path(), "<opened>"),
        (cpp_saved.as_path(), rust_saved.as_path(), "<project>"),
    ];
    if !same_step(findings, "OPEN, SAVE and SAVE <new>", &cpp, &rust, &names) {
        return;
    }
    same_project(
        gate,
        findings,
        "the project opened (migrated where it is older) and saved in place, with its backups",
        &cpp_opened,
        &rust_opened,
    );
    same_project(
        gate,
        findings,
        "the project saved as a new one",
        &cpp_saved,
        &rust_saved,
    );
    check_writers(gate, findings, work, &cpp_saved, &rust_saved);
}

/// A row's run into a new drawing (`lines` after `script`, where there is
/// one), saved as a project: the two runs compared, then the projects, then
/// each writer.
fn check_saved_run(
    gate: &Gate,
    findings: &mut Findings<'_>,
    work: &Path,
    what: &str,
    script: Option<&Path>,
    lines: &[String],
) {
    if let Err(error) = std::fs::create_dir_all(work) {
        findings.add("the work folder", &format!("{error}"));
        return;
    }
    let (cpp_saved, rust_saved) = (work.join("cpp/saved"), work.join("rust/saved"));
    let saving = |saved: &Path| -> Vec<String> {
        let mut all = lines.to_vec();
        all.push(line("SAVE", saved));
        all
    };
    let cpp = gate.katana(&gate.cpp, script, &saving(&cpp_saved));
    let rust = gate.katana(&gate.rust, script, &saving(&rust_saved));
    let names = [(cpp_saved.as_path(), rust_saved.as_path(), "<project>")];
    if !same_step(
        findings,
        &format!("{what} and SAVE <new>"),
        &cpp,
        &rust,
        &names,
    ) {
        return;
    }
    same_project(
        gate,
        findings,
        &format!("the drawing {what} made, saved as a project"),
        &cpp_saved,
        &rust_saved,
    );
    check_writers(gate, findings, work, &cpp_saved, &rust_saved);
}

fn check_import(gate: &Gate, findings: &mut Findings<'_>, work: &Path) {
    let file = gate.corpus(findings.row.path);
    check_saved_run(
        gate,
        findings,
        work,
        "IMPORT",
        None,
        &[line("IMPORT", &file)],
    );
}

fn check_survey(gate: &Gate, findings: &mut Findings<'_>, work: &Path) {
    let file = gate.corpus(findings.row.path);
    let read = [line("SURVEY READ", &file)];
    let (cpp, rust) = (gate.cpp(&read), gate.rust(&read));
    same_step(findings, "SURVEY READ", &cpp, &rust, &[]);
    check_saved_run(
        gate,
        findings,
        work,
        "SURVEY IMPORT",
        None,
        &[line("SURVEY IMPORT", &file)],
    );
}

fn check_script(gate: &Gate, findings: &mut Findings<'_>, work: &Path) {
    let file = gate.corpus(findings.row.path);
    check_saved_run(
        gate,
        findings,
        work,
        "the script",
        Some(file.as_path()),
        &[],
    );
}

fn check_lines(gate: &Gate, findings: &mut Findings<'_>, work: &Path, lines: &[&str]) {
    let lines: Vec<String> = lines
        .iter()
        .map(|each| gate.with_corpus_paths(each))
        .collect();
    check_saved_run(gate, findings, work, "the row's lines", None, &lines);
}

fn check_ifc_export(
    gate: &Gate,
    findings: &mut Findings<'_>,
    work: &Path,
    project: Option<&str>,
    arguments: &str,
) {
    let folder = work.join("ifc");
    let (cpp_file, rust_file) = (folder.join("cpp/out.ifc"), folder.join("rust/out.ifc"));
    let (cpp_source, rust_source) = (folder.join("cpp/source"), folder.join("rust/source"));
    let mut cpp_lines = Vec::new();
    let mut rust_lines = Vec::new();
    if let Some(project) = project {
        let database = gate.corpus(project);
        if let Err(why) = place_project(&database, &cpp_source)
            .and_then(|()| place_project(&database, &rust_source))
        {
            findings.add("the copies", &why);
            return;
        }
        cpp_lines.push(line("OPEN", &cpp_source));
        rust_lines.push(line("OPEN", &rust_source));
    }
    for side in ["cpp", "rust"] {
        if let Err(error) = std::fs::create_dir_all(folder.join(side)) {
            findings.add("the work folder", &format!("{error}"));
            return;
        }
    }
    let arguments = gate.with_corpus_paths(arguments);
    cpp_lines.push(format!("{} {arguments}", line("EXPORT", &cpp_file)));
    rust_lines.push(format!("{} {arguments}", line("EXPORT", &rust_file)));
    let (cpp, rust) = (gate.cpp(&cpp_lines), gate.rust(&rust_lines));
    let names = [
        (cpp_source.as_path(), rust_source.as_path(), "<opened>"),
        (cpp_file.as_path(), rust_file.as_path(), "<file>"),
    ];
    if !same_step(findings, "EXPORT .ifc", &cpp, &rust, &names) {
        return;
    }
    same_file(findings, "the .ifc both write", &cpp_file, &rust_file);
    same_ifc_judgement(gate, findings, &cpp_file, &rust_file);
    each_reads(
        gate,
        findings,
        "the other's .ifc",
        &folder,
        &rust_file,
        &cpp_file,
    );
}

/// One row through both programs.
fn check_row(gate: &Gate, index: usize, row: &Row) -> Vec<String> {
    let work = gate.scratch.join(format!("{index:03}"));
    let mut findings = Findings {
        row,
        list: Vec::new(),
    };
    match row.read {
        Read::Out(_) => {}
        Read::Project => check_project(gate, &mut findings, &work),
        Read::Import => check_import(gate, &mut findings, &work),
        Read::Survey => check_survey(gate, &mut findings, &work),
        Read::Script => check_script(gate, &mut findings, &work),
        Read::Lines(lines) => check_lines(gate, &mut findings, &work, lines),
        Read::IfcExport { project, arguments } => {
            check_ifc_export(gate, &mut findings, &work, project, arguments);
        }
    }
    findings.list
}

#[test]
fn every_file_of_the_corpus_is_read_and_written_alike_by_the_cpp_and_the_rust() {
    let cpp = std::env::var_os("KATANA_CPP_CLI").expect(
        "KATANA_CPP_CLI names the C++ katana_cli: run tools/build_visible.sh wsl, which builds it and sets it",
    );
    let root = checkout_root();
    let python = std::env::var_os("KATANA_PYTHON").unwrap_or_else(|| OsString::from("python3"));
    let ifcopenshell = run(Command::new(&python).args(["-c", "import ifcopenshell"])).ok;
    if !ifcopenshell {
        eprintln!(
            "note: {} cannot import ifcopenshell, so tools/check_ifc.py does not judge the .ifc files; \
             they are compared byte for byte (the C++ suite registers its own IFC checks only where it can)",
            python.to_string_lossy()
        );
    }
    let scratch =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("round_trip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let temporary = scratch.join("tmp");
    std::fs::create_dir_all(&temporary)
        .expect("the gate's scratch folder and the temporary folder in it");
    let gate = Gate {
        root,
        cpp: PathBuf::from(cpp),
        rust: PathBuf::from(env!("CARGO_BIN_EXE_katana_cli")),
        python,
        ifcopenshell,
        scratch: scratch.clone(),
        temporary,
    };

    if let Some(difference) = start_difference(&gate) {
        panic!(
            "the C++ and the Rust katana_cli do not start alike, so no row can be compared (do both have the same built-in customisation?): {difference}"
        );
    }

    // The rows are independent, each in its own folder, so they run side by
    // side; the findings are reported in the table's order.
    let next = AtomicUsize::new(0);
    let found: Mutex<Vec<(usize, Vec<String>)>> = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |count| count.get().min(8));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(row) = CORPUS.get(index) else {
                        break;
                    };
                    let findings = check_row(&gate, index, row);
                    found
                        .lock()
                        .expect("no worker panics holding the findings")
                        .push((index, findings));
                }
            });
        }
    });
    let mut found = found.into_inner().expect("the findings");
    found.sort_by_key(|(index, _)| *index);
    let differences: Vec<String> = found.into_iter().flat_map(|(_, list)| list).collect();
    let compared = CORPUS
        .iter()
        .filter(|row| !matches!(row.read, Read::Out(_)))
        .count();
    if differences.is_empty() {
        let _ = std::fs::remove_dir_all(&scratch);
        eprintln!(
            "round trip: {compared} files of the corpus read and written alike by both programs ({} left out, each saying why)",
            CORPUS.len() - compared
        );
    }
    assert!(
        differences.is_empty(),
        "{} differences between the C++ and the Rust over {compared} files of the corpus (the work is kept in {}):\n{}",
        differences.len(),
        scratch.display(),
        differences.join("\n")
    );
}
