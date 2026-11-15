//! tests/geo/test_import_options.cpp: IMPORT's options (docs/interop.md,
//! "Import options") - what of a file is read (layers, a WHERE filter, a
//! statement, the part in a scope, cut at its edge or not) and how (fields,
//! the project's coordinate system, a raster's band) - run through the one
//! executor as katana_cli, katana_mcp and the window's dialogs run it.
//!
//! The fixtures' facts, by hand:
//!
//! ```text
//! tests/geo/data/lots.geojson  EPSG:28356; three polygons, layer "lots":
//!   A  kind=lot       (0, 0) - (50, 40)    area 2000
//!   B  kind=lot       (50, 0) - (100, 40)  area 2000
//!   C  kind=corridor  (-10, 18) - (110, 22) area 480
//! samples/gis/parcels.geojson  8 features (docs above test_gis_verbs.cpp);
//!   two have surface=asphalt|gravel, one each.
//! ```

use katana_cad::Document;
use katana_core::ErrorCode;
use katana_core::text::format_exact_real;
use katana_entity::{Geometry, PropertyMap};

use crate::geo_fixture::{
    GeoFixture, generic, geo_data, gis_samples, holds, must, near, refused, shared_data,
};
use crate::quoted;

fn lots() -> String {
    quoted(&format!("{}/lots.geojson", geo_data()))
}

/// The C++ fixture's (vector_fixture.hpp): its scratch folder named after the
/// test.
fn fixture(name: &str) -> GeoFixture {
    GeoFixture::new("katana-geo-vector-", name)
}

/// The areas of the closed polylines on `layer`, smallest first.
fn areas(fixture: &GeoFixture, layer: &str) -> Vec<f64> {
    let mut found: Vec<f64> = fixture
        .on(layer)
        .iter()
        .filter_map(|entity| match &entity.geometry {
            Geometry::Polyline(ring) => Some(ring.area().abs()),
            _ => None,
        })
        .collect();
    found.sort_by(f64::total_cmp);
    found
}

/// A file written for one test, in its scratch folder: its path as a line
/// names it.
fn write(fixture: &GeoFixture, name: &str, text: impl AsRef<[u8]>) -> String {
    let path = fixture.scratch.path().join(name);
    std::fs::write(&path, text).expect("a scratch file is written");
    generic(&path)
}

/// The field `key` of the first record of `kind`, or what says it is not
/// there.
fn value(reply: &str, kind: &str, key: &str) -> String {
    match GeoFixture::record(reply, kind) {
        Some(found) => found
            .get(key)
            .map_or_else(|| format!("<no {key}>"), String::from),
        None => format!("<no {kind} record>"),
    }
}

/// A peg at 330000,6250000 in a GeoJSON that declares EPSG:28356.
const SITE: &str = concat!(
    r#"{"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::28356"}},"#,
    r#""features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[330000,6250000]}}]}"#
);

mod import_options {
    use super::*;

    // ---- vector: which features ----

    #[test]
    fn where_imports_only_matching_features() {
        let _shared = shared_data();
        let mut fixture = fixture("WhereImportsOnlyMatchingFeatures");
        // kind = 'lot' is A and B: two of the three.
        let reply = fixture.ok(&format!("IMPORT {} where=\"kind = 'lot'\"", lots()));
        assert_eq!(areas(&fixture, "lots"), vec![2000.0, 2000.0]);
        assert_eq!(value(&reply, "matched", "features"), "2");
        assert_eq!(value(&reply, "matched", "of"), "3");
    }

    #[test]
    fn a_where_that_matches_nothing_imports_nothing_and_says_so() {
        let _shared = shared_data();
        let mut fixture = fixture("AWhereThatMatchesNothingImportsNothingAndSaysSo");
        // No feature has kind = 'road': an answer, not a failure to read.
        let reply = fixture.ok(&format!("IMPORT {} where=\"kind = 'road'\"", lots()));
        assert_eq!(fixture.entity_count(), 0);
        assert_eq!(value(&reply, "imported", "entities"), "0");
        holds(&reply, "the filters took none of the 3 features");
    }

    #[test]
    fn a_filter_gdal_cannot_read_is_refused_with_its_reason() {
        let _shared = shared_data();
        let mut fixture = fixture("AFilterGdalCannotReadIsRefusedWithItsReason");
        let error = refused(
            fixture.run(&format!("IMPORT {} where=\"kind = = 'lot'\"", lots())),
            "a filter GDAL cannot read",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(fixture.entity_count(), 0);
    }

    #[test]
    fn layers_names_the_layers_read_and_an_unknown_one_is_refused_naming_those_there_are() {
        let _shared = shared_data();
        let mut fixture =
            fixture("LayersNamesTheLayersReadAndAnUnknownOneIsRefusedNamingThoseThereAre");
        fixture.ok(&format!("IMPORT {} layers=LOTS", lots()));
        assert_eq!(fixture.on("lots").len(), 3);
        let error = refused(
            fixture.run(&format!("IMPORT {} layers=parcels", lots())),
            "a layer the file does not have",
        );
        assert_eq!(error.code, ErrorCode::NotFound);
        holds(&error.message, "it has lots");
    }

    #[test]
    fn sql_select_imports_its_rows() {
        let _shared = shared_data();
        let mut fixture = fixture("SqlSelectImportsItsRows");
        // C alone: 120 x 4 = 480, on the layer named after the file.
        let reply = fixture.ok(&format!(
            "IMPORT {} sql=\"SELECT * FROM lots WHERE name = 'C'\" dialect=ogrsql",
            lots()
        ));
        assert_eq!(areas(&fixture, "lots"), vec![480.0]);
        assert_eq!(value(&reply, "matched", "features"), "1");
    }

    #[test]
    fn sql_that_would_change_the_file_is_refused() {
        let _shared = shared_data();
        let mut fixture = fixture("SqlThatWouldChangeTheFileIsRefused");
        let error = refused(
            fixture.run(&format!("IMPORT {} sql=\"DELETE FROM lots\"", lots())),
            "a statement that changes the file",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        holds(&error.message, "only a SELECT");
    }

    #[test]
    fn sql_with_layers_is_refused() {
        let _shared = shared_data();
        let mut fixture = fixture("SqlWithLayersIsRefused");
        let error = refused(
            fixture.run(&format!(
                "IMPORT {} layers=lots sql=\"SELECT * FROM lots\"",
                lots()
            )),
            "sql= with layers=",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn fields_keeps_only_those_properties() {
        let _shared = shared_data();
        let mut fixture = fixture("FieldsKeepsOnlyThoseProperties");
        fixture.ok(&format!("IMPORT {} fields=NAME", lots()));
        let imported = fixture.on("lots");
        assert_eq!(imported.len(), 3);
        for entity in &imported {
            assert!(entity.properties.contains_key("name"));
            assert!(!entity.properties.contains_key("kind"));
        }
        let error = refused(
            fixture.run(&format!("IMPORT {} fields=owner", lots())),
            "a field no layer has",
        );
        holds(&error.message, "'owner'");
    }

    #[test]
    fn attributes_no_keeps_no_properties() {
        let _shared = shared_data();
        let mut fixture = fixture("AttributesNoKeepsNoProperties");
        fixture.ok(&format!("IMPORT {} attributes=no", lots()));
        for entity in &fixture.on("lots") {
            assert!(!entity.properties.contains_key("name"));
            assert!(!entity.properties.contains_key("kind"));
        }
    }

    #[test]
    fn target_and_max_put_at_most_that_many_on_one_layer() {
        let _shared = shared_data();
        let mut fixture = fixture("TargetAndMaxPutAtMostThatManyOnOneLayer");
        fixture.ok(&format!("IMPORT {} target=parcels max=2", lots()));
        assert_eq!(fixture.on("parcels").len(), 2);
        assert!(fixture.on("lots").is_empty());
    }

    // ---- vector: where in the drawing ----

    #[test]
    fn an_area_filter_imports_only_features_touching_the_box() {
        let _shared = shared_data();
        let mut fixture = fixture("AnAreaFilterImportsOnlyFeaturesTouchingTheBox");
        // The box (60, 5) - (90, 15) lies inside B only: A ends at x = 50, and
        // C runs between y = 18 and 22.
        let reply = fixture.ok(&format!("IMPORT {} AREA 60,5,90,15", lots()));
        assert_eq!(areas(&fixture, "lots"), vec![2000.0]);
        assert_eq!(value(&reply, "matched", "features"), "1");
        assert_eq!(value(&reply, "scope", "scope"), "area");
        assert_eq!(value(&reply, "scope", "clip"), "no");
    }

    #[test]
    fn clip_cuts_at_the_boundary() {
        let _shared = shared_data();
        let mut fixture = fixture("ClipCutsAtTheBoundary");
        // (40, 10) - (60, 30) meets all three; cut at it, A keeps (40..50) x
        // (10..30) = 200, B (50..60) x (10..30) = 200, C (40..60) x (18..22) =
        // 80.
        fixture.ok(&format!("IMPORT {} AREA 40,10,60,30 clip", lots()));
        let cut = areas(&fixture, "lots");
        assert_eq!(cut.len(), 3);
        near(cut[0], 80.0, 1e-9);
        near(cut[1], 200.0, 1e-9);
        near(cut[2], 200.0, 1e-9);
    }

    #[test]
    fn clip_by_the_closed_shapes_a_layers_scope_takes() {
        let _shared = shared_data();
        let mut fixture = fixture("ClipByTheClosedShapesALayersScopeTakes");
        // The boundary (0, 0) - (25, 40): its box leaves B out (B starts at
        // x = 50); A is cut to 25 x 40 = 1000 and C to (0..25) x (18..22) =
        // 100.
        fixture.rect((0.0, 0.0), (25.0, 40.0), "boundary", PropertyMap::new());
        fixture.ok(&format!("IMPORT {} LAYERS boundary clip", lots()));
        let cut = areas(&fixture, "lots");
        assert_eq!(cut.len(), 2);
        near(cut[0], 100.0, 1e-9);
        near(cut[1], 1000.0, 1e-9);
    }

    #[test]
    fn a_scope_that_takes_nothing_is_reported_and_reads_nothing() {
        let _shared = shared_data();
        let mut fixture = fixture("AScopeThatTakesNothingIsReportedAndReadsNothing");
        // An empty drawing: DRAWING takes nothing, so there is no box to read.
        let reply = fixture.ok(&format!("IMPORT {} DRAWING", lots()));
        assert_eq!(fixture.entity_count(), 0);
        assert_eq!(value(&reply, "import", "ran"), "no");
        assert_eq!(value(&reply, "scope", "matched"), "0");
    }

    #[test]
    fn clip_without_closed_shapes_is_refused() {
        let _shared = shared_data();
        let mut fixture = fixture("ClipWithoutClosedShapesIsRefused");
        fixture.line((0.0, 0.0), (30.0, 30.0), "guide", PropertyMap::new());
        let error = refused(
            fixture.run(&format!("IMPORT {} LAYERS guide clip", lots())),
            "a clip by no closed shape",
        );
        holds(&error.message, "none of them closed");
    }

    #[test]
    fn a_scope_with_a_placement_is_refused() {
        let _shared = shared_data();
        let mut fixture = fixture("AScopeWithAPlacementIsRefused");
        let error = refused(
            fixture.run(&format!("IMPORT {} LOCAL AREA 0,0,10,10", lots())),
            "a scope with a placement",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn view_headless_is_refused_naming_area() {
        let _shared = shared_data();
        let mut fixture = fixture("ViewHeadlessIsRefusedNamingArea");
        let error = refused(
            fixture.run(&format!("IMPORT {} VIEW", lots())),
            "VIEW headless",
        );
        holds(&error.message, "AREA");
    }

    // ---- vector: open options ----

    #[test]
    fn an_unknown_open_option_is_refused_by_name() {
        let _shared = shared_data();
        let mut fixture = fixture("AnUnknownOpenOptionIsRefusedByName");
        let error = refused(
            fixture.run(&format!("IMPORT {} oo=NO_SUCH_OPTION=YES", lots())),
            "an open option the driver does not declare",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        holds(&error.message, "NO_SUCH_OPTION");
        assert_eq!(fixture.entity_count(), 0);
        // One GeoJSON declares (GDAL's GeoJSON driver page) reads as asked.
        fixture.ok(&format!(
            "IMPORT {} oo=FLATTEN_NESTED_ATTRIBUTES=YES",
            lots()
        ));
        assert_eq!(fixture.on("lots").len(), 3);
    }

    // ---- coordinate systems ----

    #[test]
    fn crs_project_reprojects_a_known_point() {
        let mut fixture = fixture("CrsProjectReprojectsAKnownPoint");
        // A point in Web Mercator (EPSG:3857) moved into a project in WGS 84
        // longitude and latitude. Expected by the spherical Mercator inverse
        // (EPSG guidance note 7-2, method 1024), independent of PROJ:
        //   lon = x / R,  lat = 2 atan(exp(y / R)) - pi / 2,  R = 6378137 m.
        let x: f64 = 1113194.9079327357;
        let y: f64 = 1118889.9748579594;
        let file = write(
            &fixture,
            "mercator.geojson",
            format!(
                "{}{}{},{}]}}}}]}}",
                r#"{"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::3857"}},"#,
                r#""features":[{"type":"Feature","properties":{"name":"P"},"geometry":{"type":"Point","coordinates":["#,
                format_exact_real(x),
                format_exact_real(y)
            ),
        );
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:4326", Document::SET_CRS),
        );
        fixture.ok(&format!("IMPORT {} crs=project", quoted(&file)));
        let points = fixture.on("mercator");
        assert_eq!(points.len(), 1);
        let Geometry::Point(point) = &points[0].geometry else {
            panic!("the feature is a point");
        };
        const RADIUS: f64 = 6_378_137.0;
        let lon = x / RADIUS * 180.0 / std::f64::consts::PI;
        let lat = (2.0 * (y / RADIUS).exp().atan() - std::f64::consts::PI / 2.0) * 180.0
            / std::f64::consts::PI;
        // 1e-9 degrees is about 0.1 mm on the ground: far below any survey's
        // concern, far above double rounding through PROJ's pipeline.
        near(point.position.x, lon, 1e-9);
        near(point.position.y, lat, 1e-9);
    }

    #[test]
    fn crs_project_without_a_project_crs_is_refused() {
        let _shared = shared_data();
        let mut fixture = fixture("CrsProjectWithoutAProjectCrsIsRefused");
        let error = refused(
            fixture.run(&format!("IMPORT {} crs=project", lots())),
            "crs=project with no project system",
        );
        assert_eq!(error.code, ErrorCode::InvalidCrs);
        holds(&error.message, "CRS SET");
    }

    #[test]
    fn srs_assumes_a_crs_for_a_file_without() {
        let mut fixture = fixture("SrsAssumesACrsForAFileWithout");
        // A CSV of WKT declares no coordinate system (GDAL's CSV driver).
        let file = write(
            &fixture,
            "pegs.csv",
            "WKT,name\n\"POINT (330000 6250000)\",P1\n",
        );
        let reply = fixture.ok(&format!("IMPORT {} srs=EPSG:28356", quoted(&file)));
        holds(&value(&reply, "imported", "crs"), "EPSG:28356");
    }

    #[test]
    fn srs_says_what_a_geo_json_without_a_crs_member_is_in() {
        let _shared = shared_data();
        let mut fixture = fixture("SrsSaysWhatAGeoJsonWithoutACrsMemberIsIn");
        // GDAL declares a GeoJSON without a crs member WGS 84 (RFC 7946), so
        // srs= - read only for a file that declares nothing - never applied,
        // and crs=project moved MGA metres as degrees. The point is in the
        // project's own system, so it stays exactly where it is.
        let file = write(
            &fixture,
            "pegs.geojson",
            concat!(
                r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"#,
                r#""geometry":{"type":"Point","coordinates":[330000,6250000]}}]}"#
            ),
        );
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:28356", Document::SET_CRS),
        );
        let reply = fixture.ok(&format!(
            "IMPORT {} srs=EPSG:28356 crs=project",
            quoted(&file)
        ));
        let points = fixture.on("pegs");
        assert_eq!(points.len(), 1, "{reply}");
        let Geometry::Point(point) = &points[0].geometry else {
            panic!("the feature is a point");
        };
        // No move at all: an identity, to a micrometre.
        near(point.position.x, 330000.0, 1e-6);
        near(point.position.y, 6250000.0, 1e-6);
        // Without crs=project, srs= is what the reply says the file is in.
        let kept = fixture.ok(&format!("IMPORT {} srs=EPSG:28356", quoted(&file)));
        holds(&value(&kept, "imported", "crs"), "EPSG:28356");
        // A file that declares another system is read as srs= says, and said.
        let said = fixture.ok(&format!("IMPORT {} srs=EPSG:7856", lots()));
        holds(
            &said,
            "declares GDA94 / MGA zone 56 (EPSG:28356); srs= says it is in GDA2020 / MGA zone 56 \
             (EPSG:7856)",
        );
    }

    #[test]
    fn a_file_in_another_crs_than_the_projects_is_said_to_be() {
        let mut fixture = fixture("AFileInAnotherCrsThanTheProjectsIsSaidToBe");
        // A peg in EPSG:28356 drawn unmoved into an EPSG:7856 project lies
        // about 1.5 m from where the project's system puts it (GDA94 to
        // GDA2020), and nothing said so.
        let file = write(&fixture, "site.geojson", SITE);
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:7856", Document::SET_CRS),
        );
        let unmoved = fixture.ok(&format!("IMPORT {}", quoted(&file)));
        holds(
            &unmoved,
            "warning text=\"the file is in GDA94 / MGA zone 56 (EPSG:28356), not the project's \
             GDA2020 / MGA zone 56 (EPSG:7856)",
        );
        // Moved into it, or in the project's own, nothing is said.
        let moved = fixture.ok(&format!(
            "IMPORT {} crs=project target=moved",
            quoted(&file)
        ));
        assert!(!moved.contains("not the project's"), "{moved}");
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:28356", Document::SET_CRS),
        );
        let same = fixture.ok(&format!("IMPORT {} target=same", quoted(&file)));
        assert!(!same.contains("not the project's"), "{same}");
    }

    #[test]
    fn crs_project_refuses_when_the_projects_crs_changed_while_the_import_ran() {
        let mut fixture = fixture("CrsProjectRefusesWhenTheProjectsCrsChangedWhileTheImportRan");
        let file = write(&fixture, "site.geojson", SITE);
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:7856", Document::SET_CRS),
        );
        let prepared = must(fixture.prepare(&format!("IMPORT {} crs=project", quoted(&file))));
        let work = prepared.work.expect("an import has work");
        let apply = must(work(None, None));
        // CRS SET while the worker read: the data is in the old system.
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:4326", Document::SET_CRS),
        );
        let error = refused(
            apply(&mut fixture.context()),
            "an apply after the project's system changed",
        );
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(fixture.entity_count(), 0);
    }

    #[test]
    fn adopt_sets_the_project_crs_only_when_it_has_none() {
        let _shared = shared_data();
        let mut fixture = fixture("AdoptSetsTheProjectCrsOnlyWhenItHasNone");
        // No project CRS: the file's (EPSG:28356) is adopted with the
        // entities, ONE undo step - undone, both go.
        let reply = fixture.ok(&format!("IMPORT {} crs=adopt", lots()));
        assert_eq!(fixture.document.metadata().coordinate_system, "EPSG:28356");
        assert_eq!(value(&reply, "crs", "adopted"), "yes");
        assert_eq!(fixture.on("lots").len(), 3);
        must(fixture.document.undo());
        assert_eq!(fixture.document.metadata().coordinate_system, "");
        assert_eq!(fixture.entity_count(), 0);

        // A project that has one keeps it.
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:7856", Document::SET_CRS),
        );
        let kept = fixture.ok(&format!("IMPORT {} crs=adopt", lots()));
        assert_eq!(fixture.document.metadata().coordinate_system, "EPSG:7856");
        assert_eq!(value(&kept, "crs", "adopted"), "no");
    }

    // ---- preview ----

    #[test]
    fn preview_imports_nothing_and_counts_the_match() {
        let _shared = shared_data();
        let mut fixture = fixture("PreviewImportsNothingAndCountsTheMatch");
        let reply = fixture.ok(&format!("IMPORT {} where=\"kind = 'lot'\" PREVIEW", lots()));
        assert_eq!(fixture.entity_count(), 0);
        assert_eq!(value(&reply, "import", "preview"), "yes");
        assert_eq!(value(&reply, "import", "features"), "2");
        assert_eq!(value(&reply, "import", "of"), "3");
        assert_eq!(value(&reply, "import", "entities"), "2");
        assert!(!fixture.document.history().can_undo());
    }

    // ---- rasters and point clouds ----

    #[test]
    fn a_subdataset_of_a_file_with_none_is_refused() {
        let _shared = shared_data();
        let mut fixture = fixture("ASubdatasetOfAFileWithNoneIsRefused");
        let error = refused(
            fixture.run(&format!(
                "IMPORT {} subdataset=1",
                quoted(&format!("{}/terrain.asc", gis_samples()))
            )),
            "a subdataset of a file with none",
        );
        holds(&error.message, "no subdatasets");
    }

    #[test]
    fn a_budget_with_a_class_is_sized_to_the_points_of_that_class() {
        let _shared = shared_data();
        let mut fixture = fixture("ABudgetWithAClassIsSizedToThePointsOfThatClass");
        // samples/gis/survey_scan.las: 40000 points, 29512 of class 2 (counted
        // from the file's point records, format 7, byte 16). The step was
        // sized to the whole file, 40000 / 1000 = 40, and applied after the
        // class filter: ceil(29512 / 40) = 738 of the 1000 asked. Sized to the
        // class as that first read measures it, 738 x 40 = 29520 points, the
        // step is ceil(29520 / 1000) = 30: ceil(29512 / 30) = 984, within the
        // budget.
        let reply = fixture.ok(&format!(
            "IMPORT {} budget=1000 class=2 name=ground",
            quoted(&format!("{}/survey_scan.las", gis_samples()))
        ));
        let clouds = fixture.reference.point_clouds();
        assert_eq!(clouds.len(), 1, "{reply}");
        assert_eq!(clouds[0].points.len(), 984, "{reply}");
        assert_eq!(clouds[0].decimation_step, 30);
    }

    #[test]
    fn an_option_of_another_kind_of_data_is_refused_by_name() {
        let _shared = shared_data();
        let mut fixture = fixture("AnOptionOfAnotherKindOfDataIsRefusedByName");
        let band = refused(
            fixture.run(&format!("IMPORT {} band=1", lots())),
            "band= on vector data",
        );
        holds(&band.message, "band= is not an option of a vector import");
        refused(
            fixture.run(&format!(
                "IMPORT {} where=\"a = 1\"",
                quoted(&format!("{}/terrain.asc", gis_samples()))
            )),
            "where= on a raster",
        );
        let classes = refused(
            fixture.run(&format!(
                "IMPORT {} class=2,6",
                quoted(&format!("{}/survey_scan.las", gis_samples()))
            )),
            "two classes",
        );
        assert_eq!(classes.code, ErrorCode::Unsupported);
    }

    #[test]
    fn a_plain_import_still_reads_as_it_always_did() {
        let _shared = shared_data();
        let mut fixture = fixture("APlainImportStillReadsAsItAlwaysDid");
        // No option: the whole file, as before the options came.
        let reply = fixture.ok(&format!("IMPORT {}", lots()));
        assert_eq!(fixture.on("lots").len(), 3);
        assert!(GeoFixture::record(&reply, "matched").is_none());
    }
}

mod raster_bands {
    use super::*;

    /// A 2 x 1 raster of three byte bands, band-sequential (GDAL's EHdr):
    /// band 1 = [0, 100], band 2 = [100, 0], band 3 = [50, 50].
    fn three_bands(fixture: &GeoFixture) -> String {
        write(
            fixture,
            "bands.hdr",
            "NROWS 1\nNCOLS 2\nNBANDS 3\nNBITS 8\nBYTEORDER I\nLAYOUT BSQ\nULXMAP 0.5\nULYMAP \
             0.5\nXDIM 1\nYDIM 1\n",
        );
        write(
            fixture,
            "bands.bil",
            [0x00_u8, 0x64, 0x64, 0x00, 0x32, 0x32],
        )
    }

    #[test]
    fn band_two_of_a_three_band_raster_is_read() {
        let mut fixture = fixture("BandTwoOfAThreeBandRasterIsRead");
        let file = three_bands(&fixture);
        fixture.ok(&format!("IMPORT {} band=2", quoted(&file)));
        assert_eq!(fixture.reference.rasters().len(), 1);
        let grey = &fixture.reference.rasters()[0].rgba;
        assert_eq!(grey.len(), 8);
        // Band 2 alone, stretched over its own range 0..100: the first pixel,
        // 100, is white; the second, 0, black.
        assert_eq!(grey[0], 255);
        assert_eq!(grey[1], 255);
        assert_eq!(grey[4], 0);

        // Without band=, the three bands are red, green and blue as they are.
        fixture.ok(&format!("IMPORT {} name=colour", quoted(&file)));
        assert_eq!(fixture.reference.rasters().len(), 2);
        let colour = &fixture.reference.rasters()[1];
        assert_eq!(colour.rgba[0], 0);
        assert_eq!(colour.rgba[1], 100);
        assert_eq!(colour.rgba[2], 50);
        assert_eq!(colour.name, "colour");
    }

    #[test]
    fn a_band_the_file_does_not_have_is_refused() {
        let mut fixture = fixture("ABandTheFileDoesNotHaveIsRefused");
        let file = three_bands(&fixture);
        let error = refused(
            fixture.run(&format!("IMPORT {} band=4", quoted(&file))),
            "a band the file does not have",
        );
        holds(&error.message, "3 bands");
        assert!(fixture.reference.rasters().is_empty());
    }
}
