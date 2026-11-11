//! tests/geo/test_surface_verbs.cpp: the SURFACE verbs - the drawing's named
//! surfaces made from a raster, a point cloud or the drawing, listed,
//! described, removed and written out as a DEM, and TO SURFACE - through the
//! one executor as katana_cli, katana_mcp and the window run it, each change
//! of the surfaces one undo step on the drawing (docs/terrain.md, "Surfaces in
//! the drawing").

use std::path::Path;

use katana_core::json::Json;
use katana_entity::entity::set_heights;
use katana_entity::{Entity, Geometry, PointGeometry, PropertyMap};
use katana_geometry::Point2;
use katana_gis::processing::{self as gp, ArgValue, DatasetPath, DatasetValue, RunRequest};
use katana_interop::PointCloudLayer;
use katana_pointcloud::PointCloudPoint;

use crate::geo_fixture::GeoFixture;

/// Float32's step near 100 (2^6 <= z < 2^7): 2^(6 - 23) = 7.63e-6. plane.asc
/// is read as Float32 (GDAL's AAIGrid reads a grid of decimals so), so each
/// height is within half of it of the value written in the file.
const FLOAT32_ULP_NEAR_100: f64 = 7.62939453125e-6;

/// The C++ fixture's: its scratch folder named after the test.
fn fixture(name: &str) -> GeoFixture {
    GeoFixture::new("katana-surface-verbs-", name)
}

fn levelled(x: f64, y: f64, z: Option<f64>) -> Entity {
    let mut properties = PropertyMap::default();
    set_heights(&mut properties, &[z]);
    Entity {
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(x, y),
        }),
        properties,
        ..Entity::default()
    }
}

#[track_caller]
fn draw(fixture: &mut GeoFixture, entities: Vec<Entity>) {
    assert!(
        fixture
            .document
            .execute(katana_commands::create_entities(entities))
            .is_ok()
    );
}

fn point(x: f64, y: f64, z: f64, classification: u8) -> PointCloudPoint {
    PointCloudPoint {
        x,
        y,
        z,
        classification,
        ..PointCloudPoint::default()
    }
}

/// GDAL's own raster info of `path`, as JSON; None when GDAL cannot say or
/// says what is no JSON (nlohmann's discarded value).
fn raster_info(path: &str) -> Option<Json> {
    let info = RunRequest {
        path: vec![String::from("raster"), String::from("info")],
        values: vec![(
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Path(DatasetPath {
                path: String::from(path),
                ..DatasetPath::default()
            })),
        )],
        tokens: vec![String::from("--format=json")],
        ..RunRequest::default()
    };
    let ran = gp::run(&info, None, None).ok()?;
    Json::parse(ran.text?).ok()
}

/// The data type of band 1, as GDAL's own raster info reports it.
fn band_type(path: &str) -> String {
    raster_info(path)
        .filter(|json| !json.is_null())
        .map(|json| {
            json["bands"][0]["type"]
                .get::<String>()
                .expect("a band's type is text")
        })
        .unwrap_or_default()
}

mod surface_verbs {
    use std::sync::Arc;

    use katana_app::geo::{handles, help_text, run_now};
    use katana_core::ErrorCode;
    use katana_core::json::Json;
    use katana_gis::GdalDataset;
    use katana_interop::RasterImportOptions;
    use katana_interop::import::import_raster;
    use katana_math::cpp;

    use super::{
        FLOAT32_ULP_NEAR_100, Path, PointCloudLayer, band_type, draw, fixture, levelled, point,
        raster_info,
    };
    use crate::geo_fixture::{holds, lacks, must, near, refused, shared_data};
    use crate::surface_support::{code, get, plane, record_of};

    fn parse(text: &str) -> Json {
        Json::parse(text).unwrap_or_else(|error| panic!("{error}: {text}"))
    }

    #[test]
    fn the_executor_takes_the_surface_verb_and_help_names_it() {
        assert!(handles("SURFACE LIST"));
        assert!(handles("surface from drawing"));
        holds(&help_text(), "SURFACE FROM");
        holds(&help_text(), "SURFACE EXPORT");
    }

    #[test]
    fn surface_from_raster_of_a_plane_spans_the_planes_range() {
        // plane.asc: 40 x 30 cells of 1 m from (0,0), z = 100 + 0.05 x at the
        // cell centres. The extreme centres are x = 0.5 and 39.5, so by hand
        // zmin = 100.025 and zmax = 101.975, and every one of the 1200 cells
        // is a vertex (under the 400 000 cap). A grid of m x n points in
        // convex position triangulates into 2 (m - 1)(n - 1) = 2 x 39 x 29 =
        // 2262.
        let _shared = shared_data();
        let mut fixture = fixture("SurfaceFromRasterOfAPlaneSpansThePlanesRange");
        let reply = fixture.ok(&format!("SURFACE FROM FILE \"{}\"", plane()));
        let surface = record_of(&reply, "surface");
        assert_eq!(get(&surface, "name"), "plane");
        assert_eq!(get(&surface, "points"), "1200");
        assert_eq!(get(&surface, "triangles"), "2262");
        assert_eq!(get(&surface, "zmin"), "100.025");
        assert_eq!(get(&surface, "zmax"), "101.975");
        assert_eq!(get(&surface, "bounds"), "0.500,0.500,39.500,29.500");
        let sampled = record_of(&reply, "sampled");
        assert_eq!(get(&sampled, "stride"), "1");
        assert_eq!(get(&sampled, "nodata"), "0");

        let kept = fixture.surfaces().find("plane").expect("the surface kept");
        let tin = kept.surface.as_ref().expect("a kept surface's triangles");
        near(tin.min_elevation(), 100.025, FLOAT32_ULP_NEAR_100 / 2.0);
        near(tin.max_elevation(), 101.975, FLOAT32_ULP_NEAR_100 / 2.0);
    }

    #[test]
    fn from_raster_with_an_area_only_triangulates_the_area() {
        // The window 10,10,20,20 on the 1 m grid is ten columns and ten rows
        // of whole cells, centres x = 10.5 ... 19.5: 100 points, z from
        // 100 + 0.05 x 10.5 = 100.525 to 100 + 0.05 x 19.5 = 100.975.
        let _shared = shared_data();
        let mut fixture = fixture("FromRasterWithAnAreaOnlyTriangulatesTheArea");
        let reply = fixture.ok(&format!(
            "SURFACE FROM FILE \"{}\" AREA 10,10,20,20 NAME site",
            plane()
        ));
        let surface = record_of(&reply, "surface");
        assert_eq!(get(&surface, "points"), "100");
        assert_eq!(get(&surface, "bounds"), "10.500,10.500,19.500,19.500");
        assert_eq!(get(&surface, "zmin"), "100.525");
        assert_eq!(get(&surface, "zmax"), "100.975");
        record_of(&reply, "window");
    }

    #[test]
    fn max_spends_the_cap_on_a_stride_over_the_whole_extent() {
        // max=300 on 40 x 30: the smallest stride s with ceil(40/s) ceil(30/s)
        // <= 300 is 2 (20 x 15 = 300), and the samples still span the extent.
        let _shared = shared_data();
        let mut fixture = fixture("MaxSpendsTheCapOnAStrideOverTheWholeExtent");
        let reply = fixture.ok(&format!("SURFACE FROM FILE \"{}\" max=300", plane()));
        let sampled = record_of(&reply, "sampled");
        assert_eq!(get(&sampled, "stride"), "2");
        assert_eq!(get(&sampled, "points"), "300");
    }

    #[test]
    fn surface_from_drawing_of_four_levelled_points_has_two_triangles() {
        let mut fixture = fixture("SurfaceFromDrawingOfFourLevelledPointsHasTwoTriangles");
        draw(
            &mut fixture,
            vec![
                levelled(0.0, 0.0, Some(100.0)),
                levelled(10.0, 0.0, Some(101.0)),
                levelled(10.0, 10.0, Some(102.0)),
                levelled(0.0, 10.0, Some(101.0)),
            ],
        );
        let reply = fixture.ok("SURFACE FROM DRAWING");
        let surface = record_of(&reply, "surface");
        assert_eq!(get(&surface, "triangles"), "2");
        assert_eq!(get(&surface, "points"), "4");
        assert_eq!(get(&surface, "name"), "drawing");
        assert_eq!(get(&surface, "zmin"), "100.000");
        assert_eq!(get(&surface, "zmax"), "102.000");
        let scope = record_of(&reply, "scope");
        assert_eq!(get(&scope, "scope"), "drawing");
        assert_eq!(get(&scope, "matched"), "4");
        assert_eq!(get(&scope, "used"), "4");
    }

    #[test]
    fn a_heightless_point_is_left_out_and_counted() {
        let mut fixture = fixture("AHeightlessPointIsLeftOutAndCounted");
        draw(
            &mut fixture,
            vec![
                levelled(0.0, 0.0, Some(100.0)),
                levelled(10.0, 0.0, Some(101.0)),
                levelled(10.0, 10.0, Some(102.0)),
                levelled(0.0, 10.0, Some(101.0)),
                levelled(5.0, 5.0, None),
            ],
        );
        let reply = fixture.ok("SURFACE FROM DRAWING");
        let scope = record_of(&reply, "scope");
        assert_eq!(get(&scope, "matched"), "5");
        assert_eq!(get(&scope, "used"), "4");
        assert_eq!(get(&scope, "vertices.heightless"), "1");
        assert_eq!(get(&scope, "skipped.heightless"), "1");
        // Still two triangles: the heightless point is not a fifth vertex at 0.
        assert_eq!(get(&record_of(&reply, "surface"), "triangles"), "2");
        assert_eq!(get(&record_of(&reply, "surface"), "zmin"), "100.000");
        record_of(&reply, "warning");
    }

    #[test]
    fn the_scope_narrows_the_drawing_and_a_scope_with_too_few_points_is_refused() {
        let mut fixture = fixture("TheScopeNarrowsTheDrawingAndAScopeWithTooFewPointsIsRefused");
        draw(
            &mut fixture,
            vec![
                levelled(0.0, 0.0, Some(100.0)),
                levelled(10.0, 0.0, Some(101.0)),
                levelled(10.0, 10.0, Some(102.0)),
                levelled(0.0, 10.0, Some(101.0)),
            ],
        );
        // AREA 0,0,10,5 takes the two points along y = 0: too few, and said.
        let refusal = refused(
            fixture.run("SURFACE FROM DRAWING AREA -1,-1,11,5"),
            "a scope of two points",
        );
        assert_eq!(refusal.code, ErrorCode::InvalidArgument);
        holds(&refusal.message, "matched=2");
        assert!(fixture.surfaces().empty());
        // DRAWING WHERE TYPE=point: all four.
        record_of(
            &fixture.ok("SURFACE FROM DRAWING WHERE TYPE=point NAME points"),
            "surface",
        );
    }

    #[test]
    fn a_duplicate_name_is_refused() {
        let _shared = shared_data();
        let mut fixture = fixture("ADuplicateNameIsRefused");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME ground", plane()));
        let again = refused(
            fixture.run(&format!("SURFACE FROM FILE \"{}\" NAME Ground", plane())),
            "a name in use",
        );
        assert_eq!(again.code, ErrorCode::AlreadyExists);
        // Without NAME the source's name is made unique instead.
        fixture.ok(&format!("SURFACE FROM FILE \"{}\"", plane()));
        fixture.ok(&format!("SURFACE FROM FILE \"{}\"", plane()));
        assert!(fixture.surfaces().find("plane").is_some());
        assert!(fixture.surfaces().find("plane (2)").is_some());
        assert_eq!(fixture.surfaces().all().len(), 3);
    }

    #[test]
    fn a_reference_raster_is_read_at_full_precision_from_its_file() {
        // The raster as IMPORT keeps it: an 8-bit display copy beside its
        // source path. RASTER <id> reads the path, so the range is the
        // file's, not 256 grey steps.
        let _shared = shared_data();
        let mut fixture = fixture("AReferenceRasterIsReadAtFullPrecisionFromItsFile");
        let overlay = must(import_raster(
            Path::new(&plane()),
            &RasterImportOptions::default(),
        ));
        let id = fixture.reference.add(overlay);
        let reply = fixture.ok(&format!("SURFACE FROM RASTER {id}"));
        assert_eq!(get(&record_of(&reply, "surface"), "zmin"), "100.025");
        assert_eq!(get(&record_of(&reply, "input"), "source"), "raster");
        let missing = refused(fixture.run("SURFACE FROM RASTER 99"), "no raster 99");
        assert_eq!(missing.code, ErrorCode::NotFound);
    }

    #[test]
    fn a_clouds_ground_is_triangulated_and_classes_choose_others() {
        // Four ground returns on a 10 m square and one roof return (ASPRS 6)
        // in the middle, 8 m up: the ground surface leaves the roof out and
        // says so.
        let mut fixture = fixture("ACloudsGroundIsTriangulatedAndClassesChooseOthers");
        let cloud = PointCloudLayer {
            name: String::from("scan"),
            points: vec![
                point(0.0, 0.0, 10.0, 2),
                point(10.0, 0.0, 11.0, 2),
                point(10.0, 10.0, 12.0, 2),
                point(0.0, 10.0, 11.0, 2),
                point(5.0, 5.0, 18.0, 6),
                point(4.0, 4.0, 18.0, 6),
                point(6.0, 4.0, 18.0, 6),
            ],
            ..PointCloudLayer::default()
        };
        let id = fixture.reference.add(cloud);
        let ground = fixture.ok(&format!("SURFACE FROM CLOUD {id}"));
        let input = record_of(&ground, "input");
        assert_eq!(get(&input, "ground"), "yes");
        assert_eq!(get(&input, "points"), "4");
        assert_eq!(get(&input, "excluded"), "3");
        assert_eq!(get(&record_of(&ground, "surface"), "zmax"), "12.000");

        let roofs = fixture.ok("SURFACE FROM CLOUD scan classes=6 NAME roofs");
        assert_eq!(get(&record_of(&roofs, "input"), "points"), "3");
        assert_eq!(get(&record_of(&roofs, "surface"), "zmin"), "18.000");
    }

    #[test]
    fn list_info_and_remove_say_what_the_session_holds() {
        let _shared = shared_data();
        let mut fixture = fixture("ListInfoAndRemoveSayWhatTheSessionHolds");
        holds(&fixture.ok("SURFACE LIST"), "listed surfaces=0 rasters=0");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME ground", plane()));
        let listed = fixture.ok("SURFACE LIST");
        holds(&listed, "surface name=ground triangles=2262 points=1200");
        holds(&listed, "listed surfaces=1 rasters=0");

        let json = parse(&fixture.ok("SURFACE LIST JSON"));
        assert_eq!(json["surfaces"].size(), 1);
        assert_eq!(json["surfaces"][0]["name"], Json::from("ground"));
        assert_eq!(json["surfaces"][0]["triangles"], Json::from(2262));
        near(
            json["surfaces"][0]["zmin"]
                .get::<f64>()
                .expect("zmin is a number"),
            100.025,
            FLOAT32_ULP_NEAR_100 / 2.0,
        );
        assert!(json["rasters"].empty());

        // The plan area of the TIN is its hull, the rectangle of the extreme
        // centres: 39 x 29 = 1131 m2.
        let info = fixture.ok("SURFACE INFO ground");
        holds(&info, "area plan=1131.000");

        assert_eq!(
            code(fixture.run("SURFACE INFO nothing")),
            ErrorCode::NotFound
        );
        holds(
            &fixture.ok("SURFACE REMOVE GROUND"),
            "removed surface=ground surfaces=0",
        );
        assert!(fixture.surfaces().empty());
        assert_eq!(
            code(fixture.run("SURFACE REMOVE ground")),
            ErrorCode::NotFound
        );
    }

    #[test]
    fn preview_says_what_would_be_read_and_makes_nothing() {
        let mut fixture = fixture("PreviewSaysWhatWouldBeReadAndMakesNothing");
        let reply = fixture.ok(&format!("SURFACE FROM FILE \"{}\" PREVIEW", plane()));
        holds(&reply, "preview valid=yes changed=no");
        assert!(fixture.surfaces().empty());
    }

    #[test]
    fn what_the_verb_cannot_read_is_refused_naming_it() {
        let _shared = shared_data();
        let mut fixture = fixture("WhatTheVerbCannotReadIsRefusedNamingIt");
        let plane = plane();
        assert_eq!(code(fixture.run("SURFACE")), ErrorCode::InvalidArgument);
        assert_eq!(
            code(fixture.run("SURFACE BUILD")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run("SURFACE FROM")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run(&format!("SURFACE FROM FILE \"{plane}\" max=2"))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run(&format!("SURFACE FROM FILE \"{plane}\" classes=2"))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run(&format!("SURFACE FROM FILE \"{plane}\" AREA 1,1,1,5"))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run("SURFACE FROM CLOUD 7")),
            ErrorCode::NotFound
        );
        let none = fixture.scratch.file("none.tif");
        assert_eq!(
            code(fixture.run(&format!("SURFACE FROM FILE \"{none}\""))),
            ErrorCode::NotFound
        );
        assert!(fixture.surfaces().empty());
    }

    // ---- EXPORT ---------------------------------------------------------------------------------

    #[test]
    fn surface_export_float32_reads_back_the_surface_within_float32_precision() {
        let _shared = shared_data();
        let mut fixture = fixture("SurfaceExportFloat32ReadsBackTheSurfaceWithinFloat32Precision");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME ground", plane()));
        let dem = fixture.scratch.file("ground.tif");
        let reply = fixture.ok(&format!("SURFACE EXPORT ground \"{dem}\" cell=1"));
        let exported = record_of(&reply, "exported");
        assert_eq!(get(&exported, "driver"), "GTiff");
        assert_eq!(get(&exported, "type"), "Float32");
        // The surface spans 0.5 ... 39.5 by 0.5 ... 29.5: 39 x 29 cells of 1 m.
        assert_eq!(get(&exported, "raster"), "39x29");
        assert_eq!(band_type(&dem), "Float32");

        // Each cell is the TIN at its centre (x0 + 0.5 + c, y1 - 0.5 - r),
        // which on the plane is 100 + 0.05 x. Error: each vertex is within
        // half a Float32 step of the plane (the file's Float32 values); the
        // TIN's barycentric weights are non-negative and sum to one, so the
        // interpolated value is too; storing it as Float32 rounds once more
        // by at most half a step. Together at most one step, 7.63e-6, plus
        // the double arithmetic's 1e-12.
        let dataset = must(GdalDataset::open(Path::new(&dem)));
        let samples = dataset.read_band_sampled(1, 1);
        drop(dataset); // closed, so the file can be replaced below
        let samples = must(samples);
        assert_eq!(samples.columns, 39);
        assert_eq!(samples.rows, 29);
        let mut worst = 0.0;
        for row in 0..samples.rows {
            for column in 0..samples.columns {
                let x = 0.5 + 0.5 + f64::from(column);
                let value = samples.values[(row * 39 + column) as usize];
                worst = cpp::max(worst, (value - (100.0 + 0.05 * x)).abs());
            }
        }
        assert!(worst <= FLOAT32_ULP_NEAR_100 + 1e-12, "worst={worst:?}");

        // Float64 on request; and the file in the way needs OVERWRITE.
        let refusal = refused(
            fixture.run(&format!(
                "SURFACE EXPORT ground \"{dem}\" cell=1 type=Float64"
            )),
            "a file in the way",
        );
        assert_eq!(refusal.code, ErrorCode::AlreadyExists);
        fixture.ok(&format!(
            "SURFACE EXPORT ground \"{dem}\" cell=1 type=Float64 OVERWRITE"
        ));
        assert_eq!(band_type(&dem), "Float64");
    }

    #[test]
    fn a_cog_export_is_laid_out_as_a_cloud_optimised_geo_tiff() {
        // At 0.05 m the 39 x 29 m surface is 780 x 580 cells, more than one
        // of the COG driver's 512-cell tiles, so a COG has an overview and
        // 512 x 512 tiles; the plain GeoTIFF the export writes otherwise has
        // 256 x 256 tiles and no overview - which is what tells the two
        // apart here.
        let _shared = shared_data();
        let mut fixture = fixture("ACogExportIsLaidOutAsACloudOptimisedGeoTiff");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME ground", plane()));
        let cog = fixture.scratch.file("ground_cog.tif");
        let reply = fixture.ok(&format!("SURFACE EXPORT ground \"{cog}\" cell=0.05 COG"));
        assert_eq!(get(&record_of(&reply, "exported"), "driver"), "COG");
        assert_eq!(get(&record_of(&reply, "exported"), "raster"), "780x580");
        let info = raster_info(&cog).expect("GDAL's raster info of the COG");
        assert_eq!(
            info["metadata"]["IMAGE_STRUCTURE"]["LAYOUT"],
            Json::from("COG"),
            "{}",
            info.dump()
        );
        assert_eq!(
            info["metadata"]["IMAGE_STRUCTURE"]["COMPRESSION"],
            Json::from("DEFLATE")
        );
        assert_eq!(
            info["bands"][0]["block"],
            Json::array_of([Json::from(512), Json::from(512)])
        );
        assert!(
            !info["bands"][0]["overviews"].empty(),
            "{}",
            info["bands"][0].dump()
        );

        let plain = fixture.scratch.file("ground_plain.tif");
        fixture.ok(&format!("SURFACE EXPORT ground \"{plain}\" cell=0.05"));
        let tiff = raster_info(&plain).expect("GDAL's raster info of the GeoTIFF");
        assert_eq!(
            tiff["bands"][0]["block"],
            Json::array_of([Json::from(256), Json::from(256)])
        );
        assert!(!tiff["bands"][0].contains("overviews") || tiff["bands"][0]["overviews"].empty());
    }

    #[test]
    fn a_cog_export_passes_gdals_cog_validation() {
        let _shared = shared_data();
        let mut fixture = fixture("ACogExportPassesGdalsCogValidation");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME ground", plane()));
        let cog = fixture.scratch.file("ground_cog.tif");
        fixture.ok(&format!("SURFACE EXPORT ground \"{cog}\" cell=0.05 COG"));
        // GDAL's own validator, through the GDAL verb: return code 0 is a
        // valid Cloud Optimised GeoTIFF. It is a script of GDAL's Python
        // utilities, which a GDAL built without them cannot run.
        let validated = fixture.run(&format!("GDAL driver cog validate \"{cog}\""));
        if let Err(error) = &validated
            && error.message.contains("osgeo_utils")
        {
            eprintln!(
                "this GDAL has no Python utilities to validate a COG with: {}",
                error.message
            );
            return;
        }
        let validated = must(validated);
        // "driver cog validate" declares no return-code argument (GDAL 3.13's
        // own argument list; only "dataset check" and "raster compare" have
        // one), so its verdict is the sentence GDAL's
        // validate_cloud_optimized_geotiff prints: "<file> is a valid cloud
        // optimized GeoTIFF", and "is NOT a valid" when it is not.
        holds(&validated, "is a valid cloud optimized GeoTIFF");
        lacks(&validated, "is NOT a valid");
    }

    #[test]
    fn export_takes_creation_options_and_refuses_what_it_cannot_write() {
        let _shared = shared_data();
        let mut fixture = fixture("ExportTakesCreationOptionsAndRefusesWhatItCannotWrite");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME ground", plane()));
        let lzw = fixture.scratch.file("lzw.tif");
        fixture.ok(&format!("SURFACE EXPORT ground \"{lzw}\" co=COMPRESS=LZW"));
        assert!(fixture.scratch.path().join("lzw.tif").exists());
        let xyzzy = fixture.scratch.file("x.xyzzy");
        let tif = fixture.scratch.file("x.tif");
        assert_eq!(
            code(fixture.run(&format!("SURFACE EXPORT ground \"{xyzzy}\""))),
            ErrorCode::Unsupported
        );
        assert_eq!(
            code(fixture.run(&format!("SURFACE EXPORT ground \"{tif}\" cell=0"))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run(&format!("SURFACE EXPORT ground \"{tif}\" type=Int16"))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run(&format!("SURFACE EXPORT nothing \"{tif}\""))),
            ErrorCode::NotFound
        );
        assert!(!fixture.scratch.path().join("x.tif").exists());
        assert!(!fixture.scratch.path().join("x.xyzzy").exists());
    }

    // ---- TO SURFACE ----------------------------------------------------------------------------

    #[test]
    fn a_raster_result_to_a_surface_is_triangulated_and_kept() {
        // GDAL's clip of the plane to 0,0,10,10: 10 x 10 whole cells, their
        // centres 0.5 ... 9.5, z 100.025 ... 100.475.
        let _shared = shared_data();
        let mut fixture = fixture("ARasterResultToASurfaceIsTriangulatedAndKept");
        let line = format!(
            "GDAL raster clip --bbox=0,0,10,10 FROM FILE \"{}\" TO SURFACE corner",
            plane()
        );
        let reply = fixture.ok(&line);
        let output = record_of(&reply, "output");
        assert_eq!(get(&output, "target"), "surface");
        let surface = record_of(&reply, "surface");
        assert_eq!(get(&surface, "points"), "100");
        assert_eq!(get(&surface, "zmax"), "100.475");
        assert!(fixture.surfaces().find("corner").is_some());
        // The spilled raster is gone once read.
        let mut left = 0;
        for entry in std::fs::read_dir(fixture.scratch.path()).expect("the scratch folder") {
            let entry = entry.expect("an entry of the scratch folder");
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "tif")
            {
                left += 1;
            }
        }
        assert_eq!(left, 0);
        // A name in use is refused.
        assert_eq!(code(fixture.run(&line)), ErrorCode::AlreadyExists);
    }

    // ---- the drawing's surfaces, undone and taken by the right front end ------------------------

    #[test]
    fn surface_from_is_one_undo_step_and_undo_leaves_none() {
        let _shared = shared_data();
        let mut fixture = fixture("SurfaceFromIsOneUndoStepAndUndoLeavesNone");
        let steps = fixture.document.history().undo_count();
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME ground", plane()));
        assert_eq!(fixture.document.history().undo_count(), steps + 1);
        assert_eq!(fixture.document.history().undo_name(), "SURFACE FROM");
        must(fixture.document.undo());
        let listed = fixture.ok("SURFACE LIST");
        holds(&listed, "listed surfaces=0");
        must(fixture.document.redo());
        assert!(fixture.surfaces().find("ground").is_some());
    }

    #[test]
    fn remove_is_one_undo_step_and_undo_puts_the_same_surface_back() {
        let _shared = shared_data();
        let mut fixture = fixture("RemoveIsOneUndoStepAndUndoPutsTheSameSurfaceBack");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME ground", plane()));
        let ground = Arc::clone(
            fixture
                .surfaces()
                .find("ground")
                .and_then(|named| named.surface.as_ref())
                .expect("the surface made"),
        );
        let removed = fixture.ok("SURFACE REMOVE GROUND");
        holds(&removed, "removed surface=ground surfaces=0");
        assert!(fixture.surfaces().empty());
        assert_eq!(fixture.document.history().undo_name(), "SURFACE REMOVE");
        must(fixture.document.undo());
        let back = fixture
            .surfaces()
            .find("ground")
            .and_then(|named| named.surface.as_ref())
            .expect("the surface put back");
        assert!(
            Arc::ptr_eq(back, &ground),
            "the very surface, not a rebuilt one"
        );
    }

    #[test]
    fn a_refused_line_changes_no_surface_and_makes_no_step() {
        let _shared = shared_data();
        let mut fixture = fixture("ARefusedLineChangesNoSurfaceAndMakesNoStep");
        let line = format!("SURFACE FROM FILE \"{}\" NAME ground", plane());
        fixture.ok(&line);
        let steps = fixture.document.history().undo_count();
        assert!(fixture.run(&line).is_err());
        assert!(fixture.run("SURFACE REMOVE nothing").is_err());
        assert_eq!(fixture.document.history().undo_count(), steps);
        assert_eq!(fixture.surfaces().all().len(), 1);
    }

    // geo_verbs.rs: the executor's copy of the drawing's surfaces is "never a
    // second store: nothing keeps it between phases". It holds an Arc of every
    // surface of the drawing, in a Context that lives as long as the window
    // or the session (here: one lent for these lines), so a copy left behind
    // after a line kept the previous drawing's surfaces in memory after NEW
    // until the next geo line.
    #[test]
    fn the_staging_copy_is_dropped_when_the_line_ends_whatever_it_answered() {
        let _shared = shared_data();
        let mut fixture = fixture("TheStagingCopyIsDroppedWhenTheLineEndsWhateverItAnswered");
        let line = format!("SURFACE FROM FILE \"{}\" NAME ground", plane());
        {
            let mut context = fixture.context();
            must(run_now(&mut context, &line, None, None));
            assert!(
                context.surfaces.empty(),
                "a line that changed the surfaces left its copy"
            );
            must(run_now(&mut context, "SURFACE LIST", None, None));
            assert!(
                context.surfaces.empty(),
                "a line that changed nothing left its copy"
            );
            assert!(run_now(&mut context, &line, None, None).is_err());
            assert!(context.surfaces.empty(), "a refused line left its copy");
            assert!(run_now(&mut context, "SURFACE REMOVE nothing", None, None).is_err());
            assert!(context.surfaces.empty());
        }

        // So a new drawing is the only holder's end of the old one's surface.
        let old = Arc::downgrade(
            fixture
                .surfaces()
                .find("ground")
                .and_then(|named| named.surface.as_ref())
                .expect("the surface made"),
        );
        assert!(old.strong_count() > 0);
        fixture.document.new_document();
        assert_eq!(
            old.strong_count(),
            0,
            "the previous drawing's surface is still held by the executor"
        );
    }

    // A line is previewed to learn whether it would run: PREVIEW answered
    // "valid=yes" for a scope the run refused for having fewer than three
    // points with a height (and for a cloud's classes that match nothing).
    #[test]
    fn a_preview_of_a_scope_the_run_refuses_is_refused_as_the_run_is() {
        let mut fixture = fixture("APreviewOfAScopeTheRunRefusesIsRefusedAsTheRunIs");
        fixture.type_line("LAYER NEW empty");
        let empty_run = fixture.run("SURFACE FROM LAYERS empty ONLY");
        let empty_preview = fixture.run("SURFACE FROM LAYERS empty ONLY PREVIEW");
        let empty_run = refused(empty_run, "a scope of an empty layer");
        let empty_preview = refused(
            empty_preview,
            "a preview of a line the run refuses said it was valid",
        );
        assert_eq!(empty_preview.code, ErrorCode::InvalidArgument);
        assert_eq!(empty_preview.message, empty_run.message);
        holds(&empty_preview.message, "fewer than three points");

        // A lone point with no height: matched, but nothing to triangulate.
        draw(&mut fixture, vec![levelled(0.0, 0.0, None)]);
        let lone_preview = refused(
            fixture.run("SURFACE FROM DRAWING PREVIEW"),
            "a lone heightless point",
        );
        holds(&lone_preview.message, "matched=1 used=0");
        assert!(fixture.run("SURFACE FROM DRAWING").is_err());

        // Enough heighted points: the preview is valid, and so is the run.
        draw(
            &mut fixture,
            vec![
                levelled(0.0, 0.0, Some(100.0)),
                levelled(10.0, 0.0, Some(101.0)),
                levelled(10.0, 10.0, Some(102.0)),
            ],
        );
        let valid = fixture.ok("SURFACE FROM DRAWING PREVIEW");
        holds(&valid, "preview valid=yes changed=no");
        assert!(fixture.surfaces().empty(), "a preview makes nothing");
        assert!(fixture.run("SURFACE FROM DRAWING").is_ok());
    }

    #[test]
    fn a_preview_of_a_cloud_with_too_few_points_in_the_classes_is_refused_as_the_run_is() {
        // Four ground returns and two roof returns (ASPRS 6): classes=6
        // leaves two points, under the three a triangle needs.
        let mut fixture =
            fixture("APreviewOfACloudWithTooFewPointsInTheClassesIsRefusedAsTheRunIs");
        let cloud = PointCloudLayer {
            name: String::from("scan"),
            points: vec![
                point(0.0, 0.0, 10.0, 2),
                point(10.0, 0.0, 11.0, 2),
                point(10.0, 10.0, 12.0, 2),
                point(0.0, 10.0, 11.0, 2),
                point(5.0, 5.0, 18.0, 6),
                point(4.0, 4.0, 18.0, 6),
            ],
            ..PointCloudLayer::default()
        };
        fixture.reference.add(cloud);

        for classes in ["classes=6", "classes=99"] {
            let preview = refused(
                fixture.run(&format!("SURFACE FROM CLOUD scan {classes} PREVIEW")),
                &format!("{classes} previewed as valid"),
            );
            assert_eq!(preview.code, ErrorCode::InvalidArgument, "{classes}");
            assert!(
                preview.message.contains("fewer than three points"),
                "{classes}: {}",
                preview.message
            );
            let real = refused(
                fixture.run(&format!("SURFACE FROM CLOUD scan {classes}")),
                classes,
            );
            assert_eq!(real.code, ErrorCode::InvalidArgument);
        }
        let valid = fixture.ok("SURFACE FROM CLOUD scan classes=2 PREVIEW");
        holds(&valid, "preview valid=yes changed=no");
        assert!(fixture.surfaces().empty());
        assert!(fixture.run("SURFACE FROM CLOUD scan classes=2").is_ok());
        assert_eq!(fixture.surfaces().all().len(), 1);
    }

    #[test]
    fn the_list_says_each_surfaces_style_and_whether_it_is_shown() {
        let _shared = shared_data();
        let mut fixture = fixture("TheListSaysEachSurfacesStyleAndWhetherItIsShown");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME ground", plane()));
        let listed = fixture.ok("SURFACE LIST");
        holds(&listed, "style=Standard visible=yes");
        // The interpreter's words change them; the executor's LIST says so.
        fixture.type_line("SURFACE STYLE ground Contours");
        fixture.type_line("SURFACE HIDE ground");
        let listed = fixture.ok("SURFACE LIST");
        holds(&listed, "style=Contours visible=no");
        let json = parse(&fixture.ok("SURFACE LIST JSON"));
        assert_eq!(json["surfaces"].size(), 1);
        assert_eq!(json["surfaces"][0]["style"], Json::from("Contours"));
        assert_eq!(json["surfaces"][0]["visible"], Json::from(false));
    }

    #[test]
    fn the_executor_takes_only_its_own_surface_words() {
        for line in [
            "SURFACE LIST",
            "SURFACE INFO ground",
            "SURFACE REMOVE ground",
            "SURFACE FROM DRAWING",
            "SURFACE EXPORT ground x.tif",
            "SURFACE",
        ] {
            assert!(handles(line), "{line}");
        }
        // The drawing's own words, the edits' and the volumes' are the
        // interpreter's (cad's terrain::surface_verbs).
        for line in [
            "SURFACE RENAME a b",
            "SURFACE STYLE a Contours",
            "SURFACE SHOW ALL",
            "SURFACE HIDE a",
            "SURFACE COPY a b",
            "SURFACE EDIT a RAISE 1",
            "SURFACE DIFFERENCE a b",
            "SURFACESTYLE LIST",
        ] {
            assert!(!handles(line), "{line}");
        }
    }
}

// ---- through a Session, as katana_cli and katana_mcp run it -------------------------------------

mod surface_session {
    use katana_app::{Captured, Session};

    use crate::geo_fixture::{holds, shared_data};
    use crate::surface_support::plane;

    #[test]
    fn a_new_drawing_has_none_of_the_last_ones_surfaces() {
        let _shared = shared_data();
        let mut session = Session::new(None, &mut Captured::default());
        let mut captured = Captured::default();
        assert!(session.run(
            format!("SURFACE FROM FILE \"{}\" NAME ground", plane()),
            &mut captured
        ));
        assert!(session.run("NEW", &mut captured));
        assert!(session.run("SURFACE LIST", &mut captured));
        holds(&captured.out, "listed surfaces=0");
    }

    #[test]
    fn the_session_keeps_its_surfaces_between_lines() {
        let _shared = shared_data();
        let mut session = Session::new(None, &mut Captured::default());
        let mut captured = Captured::default();
        let made = session.run(
            format!("SURFACE FROM FILE \"{}\" NAME ground", plane()),
            &mut captured,
        );
        let listed = session.run("SURFACE LIST", &mut captured);
        assert!(made);
        assert!(listed);
        holds(&captured.out, "surface name=ground triangles=2262");
        holds(&captured.out, "listed surfaces=1");
    }
}
