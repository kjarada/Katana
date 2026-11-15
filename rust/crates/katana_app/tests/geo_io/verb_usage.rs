//! tests/geo/test_verb_usage.cpp: the grammar HELP gives and the docs
//! document is the grammar the verbs accept (docs/terrain.md, "Surfaces",
//! "Gridding points to a DEM" and "The DEM tools"; docs/geoprocessing.md, "The
//! GDAL verb"). One line per documented form, every option of it given, run
//! through the executor as the window, katana_cli and katana_mcp run it: a
//! form the help names that the verb refuses fails here. The heavy forms are
//! previewed, which reads the whole line and binds its sources; the targets
//! the help once left out (TO SURFACE, TO SELECTION, TO REPORT) run.
//!
//! These read the whole verb table: SURFACE and the raster verbs are step 6's
//! agents A's and R's, so they pass when wave 2 is integrated as one.

mod geo_verb_usage {
    use katana_app::geo::{Context, help_text, run_now};

    use crate::geo_fixture::{GeoFixture, geo_data, gis_samples, shared_data};

    /// The C++ fixture: its scratch folder named after the test, derived
    /// rasters kept in its "derived" folder.
    fn fixture(name: &str) -> GeoFixture {
        GeoFixture::new("katana-verb-usage-", name)
    }

    /// The line run through the executor, its derived rasters in
    /// <scratch>/derived as the C++ fixture's context keeps them.
    fn run(fixture: &mut GeoFixture, line: &str) -> katana_core::Result<String> {
        let derived = fixture.scratch.path().join("derived");
        let mut context = Context::new(
            &mut fixture.document,
            &fixture.interpreter,
            &mut fixture.reference,
            &derived,
            &fixture.front_end,
        );
        run_now(&mut context, line, None, None)
    }

    fn at(fixture: &GeoFixture, name: &str) -> String {
        format!("\"{}\"", fixture.scratch.file(name))
    }

    #[track_caller]
    fn accepted(fixture: &mut GeoFixture, line: &str) {
        if let Err(error) = run(fixture, line) {
            panic!("{line}\n  {}", error.describe());
        }
    }

    #[test]
    fn every_form_the_help_gives_is_accepted() {
        let _shared = shared_data();
        let mut fixture = fixture("EveryFormTheHelpGivesIsAccepted");
        // What the forms read: plane.asc (40 x 30 cells of 1 m) as the
        // reference raster "plane" and the surface "ground", the sample scan as
        // the cloud "scan", levelled points, the lots, and one closed boundary
        // inside the plane on the layer "boundary".
        let plane = format!("\"{}/plane.asc\"", geo_data());
        let lots = format!("\"{}/lots.geojson\"", geo_data());
        for setup in [
            format!("IMPORT {plane}"),
            format!("IMPORT \"{}/survey_scan.las\" name=scan", gis_samples()),
            format!("IMPORT \"{}/plane_points.geojson\"", geo_data()),
            format!("IMPORT {lots}"),
            String::from("SURFACE FROM RASTER plane NAME ground"),
        ] {
            if let Err(error) = run(&mut fixture, &setup) {
                panic!("{setup}: {}", error.describe());
            }
        }
        for setup in ["LAYER NEW boundary", "LAYER SET boundary", "RECT 2,2 20,20"] {
            fixture.type_line(setup);
        }

        let lines = [
            // GDAL: every source kind, every target kind, every clause word.
            String::from("GDAL VERSION"),
            String::from("GDAL LIST raster JSON"),
            String::from("GDAL HELP raster hillshade JSON"),
            String::from(
                "GDAL RUN raster hillshade --zfactor=2 FROM input RASTER plane TO output \
                 REFERENCE shade CONFIRM OVERWRITE PREVIEW",
            ),
            format!(
                "GDAL raster hillshade FROM SURFACE ground CELL 2 TO FILE {} FORMAT GTiff PREVIEW",
                at(&fixture, "h.tif")
            ),
            format!("GDAL raster hillshade FROM FILE {plane} TO SURFACE shaded"),
            format!(
                "GDAL vector buffer --distance=1 FROM FILE {lots} LAYER lots TO LAYER gis/b PREVIEW"
            ),
            String::from(
                "GDAL vector buffer --distance=1 FROM LAYERS lots ONLY WHERE TYPE=polyline TO \
                 SELECTION",
            ),
            String::from("GDAL vector buffer --distance=1 FROM AREA 0,0,200,200 TO REPORT"),
            // SURFACE: each source with its options, and EXPORT with all of
            // its.
            String::from("SURFACE LIST JSON"),
            String::from("SURFACE INFO ground"),
            String::from("SURFACE FROM RASTER plane max=500 AREA 0,0,10,10 NAME a PREVIEW"),
            format!("SURFACE FROM FILE {plane} max=500 AREA 0,0,10,10 NAME b PREVIEW"),
            String::from("SURFACE FROM CLOUD scan classes=2 max=100 NAME c PREVIEW"),
            String::from("SURFACE FROM LAYERS plane_points NAME d PREVIEW"),
            format!(
                "SURFACE EXPORT ground {} cell=2 type=Float32 cog format=GTiff \
                 co=COMPRESS=DEFLATE OVERWRITE PREVIEW",
                at(&fixture, "g.tif")
            ),
            // RASTER GRID: its options and each target.
            String::from(
                "RASTER GRID LAYERS plane_points method=invdist cell=2 z=geometry extent=scope \
                 power=2 radius=10 NAME g PREVIEW",
            ),
            format!(
                "RASTER GRID LAYERS plane_points method=nearest size=10x10 extent=0,0,40,30 TO \
                 FILE {} FORMAT GTiff OVERWRITE PREVIEW",
                at(&fixture, "grid.tif")
            ),
            String::from(
                "RASTER GRID LAYERS plane_points method=nearest size=10x10 extent=0,0,40,30 TO \
                 SURFACE gridded",
            ),
            // The DEM tools: each <raster> kind, each tool's options, and the
            // words every tool takes.
            format!(
                "RASTER MOSAIC RASTER plane FILE {plane} resolution=highest SAVE {} NAME m \
                 OVERWRITE PREVIEW",
                at(&fixture, "m.tif")
            ),
            String::from("RASTER CLIP SURFACE ground CELL 1 AREA 0,0,10,10 NAME cl PREVIEW"),
            format!(
                "RASTER CLIP RASTER plane LAYERS boundary TO FILE {} FORMAT GTiff OVERWRITE \
                 PREVIEW",
                at(&fixture, "c.tif")
            ),
            String::from(
                "RASTER FILL RASTER plane distance=5 smoothing=1 strategy=nearest TO REFERENCE f \
                 PREVIEW",
            ),
            format!("RASTER FOOTPRINT FILE {plane} TO LAYER gis/fp PREVIEW"),
            format!(
                "RASTER FOOTPRINT RASTER plane TO FILE {} OVERWRITE PREVIEW",
                at(&fixture, "fp.geojson")
            ),
            String::from(
                "RASTER REPROJECT RASTER plane crs=EPSG:28356 from=EPSG:28355 \
                 resampling=bilinear cell=2 PREVIEW",
            ),
            String::from("RASTER REPROJECT RASTER plane like=plane from=EPSG:28356 PREVIEW"),
            String::from(
                "RASTER DIFFERENCE RASTER plane MINUS SURFACE ground CELL 1 LAYERS boundary \
                 resampling=bilinear TO SURFACE difference",
            ),
            // EXPORT of a reference cloud.
            format!("EXPORT {} CLOUD scan PREVIEW", at(&fixture, "scan.laz")),
        ];
        for line in &lines {
            accepted(&mut fixture, line);
        }
        // The targets run, not only previewed, made what they name.
        for name in ["shaded", "gridded", "difference"] {
            assert!(fixture.surfaces().find(name).is_some(), "{name}");
        }
    }

    // And HELP names the words those lines use that its rows once left out:
    // the GDAL verb's FILE LAYER and its SURFACE, SELECTION and REPORT
    // targets, SURFACE's format= and CLOUD's max=, and what a DEM tool's
    // <raster> is and the words each takes.
    #[test]
    fn the_help_names_every_word_the_forms_use() {
        let help = help_text();
        for words in [
            "FILE <path> [LAYER <name>]; TO takes",
            "SURFACE <name>, SELECTION or REPORT",
            "CLOUD <id|name> [classes=2,...] [max=<points>]",
            "[format=<driver>]",
            "<raster> is\n          RASTER <id|name> | SURFACE <name> [CELL <m>] | FILE <path>",
            "[FORMAT <driver>] | TO SURFACE <name>] [OVERWRITE] [PREVIEW]",
            "[resolution=same|highest|lowest|average|<x>,<y>] [SAVE <file>]",
        ] {
            assert!(help.contains(words), "{words}");
        }
    }
}
