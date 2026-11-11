//! tests/geo/test_export_options.cpp: EXPORT on the shared scope and filter,
//! with its options (docs/interop.md, "Export options") - what of the drawing
//! is written, the coordinate system it is written in, one file layer or one
//! per drawing layer, adding to a GeoPackage, the driver's options checked,
//! and text as points - run through the one executor as katana_cli,
//! katana_mcp and the window's dialog run it, and checked by reading the file
//! back through GDAL.
//!
//! The drawing each test starts from, by hand (draw_site):
//!
//! ```text
//! lots   two closed rectangles, (0, 0) - (50, 40) and (50, 0) - (100, 40)
//! roads  one line, (0, 50) - (100, 50)
//! pegs   one point, (10, 60)
//! ```
//!
//! so 4 entities; LAYERS lots,pegs takes 3; WHERE TYPE=polyline takes the 2
//! rectangles.

use std::collections::BTreeMap;
use std::path::Path;

use katana_app::dxf_verbs::read_dxf_import;
use katana_app::geo::replies::parse_records;
use katana_cad::Document;
use katana_core::ErrorCode;
use katana_entity::{
    AnchorPoint, AnchorRef, EntityId, Geometry, LeaderGeometry, PointGeometry, PropertyMap,
    PropertyValue, TextGeometry,
};
use katana_geometry::Point2;
use katana_gis::processing::{Feature, FeatureTable, FieldValue};
use katana_gis::reproject::crs_epsg_code;
use katana_gis::{GdalDataset, GeometryKind, VectorReadOptions};

use crate::geo_fixture::{GeoFixture, holds, lacks, must, near, refused};
use crate::{double_eq, quoted};

/// The C++ fixture's (vector_fixture.hpp): its scratch folder named after the
/// test.
fn fixture(name: &str) -> GeoFixture {
    GeoFixture::new("katana-geo-vector-", name)
}

fn point(x: f64, y: f64) -> Geometry {
    Geometry::Point(PointGeometry {
        position: Point2::new(x, y),
    })
}

fn draw_site(fixture: &mut GeoFixture) {
    fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
    fixture.rect((50.0, 0.0), (100.0, 40.0), "lots", PropertyMap::new());
    fixture.line((0.0, 50.0), (100.0, 50.0), "roads", PropertyMap::new());
    fixture.add(point(10.0, 60.0), "pegs", PropertyMap::new());
}

/// The file's layers and how many features each holds, read by GDAL.
fn layers_of(path: &str) -> BTreeMap<String, u64> {
    let mut found = BTreeMap::new();
    let dataset = GdalDataset::open(Path::new(path));
    assert!(dataset.is_ok(), "{path}");
    let Ok(dataset) = dataset else {
        return found;
    };
    for layer in must(dataset.vector_layers()) {
        found.insert(layer.name, layer.feature_count);
    }
    found
}

fn layers(expected: &[(&str, u64)]) -> BTreeMap<String, u64> {
    expected
        .iter()
        .map(|(name, count)| (String::from(*name), *count))
        .collect()
}

fn table(path: &str, layer: i32) -> FeatureTable {
    let dataset = GdalDataset::open(Path::new(path));
    assert!(dataset.is_ok(), "{path}");
    let Ok(dataset) = dataset else {
        return FeatureTable::default();
    };
    let read = dataset.read_table(layer, &VectorReadOptions::default(), None);
    assert!(read.is_ok());
    read.unwrap_or_default()
}

fn field<'t>(table: &FeatureTable, feature: &'t Feature, name: &str) -> Option<&'t FieldValue> {
    table
        .fields
        .iter()
        .zip(&feature.values)
        .find(|(field, _)| field.name == name)
        .map(|(_, value)| value)
}

/// The C++'s std::get<double> of a field: it must be a real.
#[track_caller]
fn real(value: Option<&FieldValue>) -> f64 {
    match value {
        Some(FieldValue::Real(real)) => *real,
        other => panic!("not a real: {other:?}"),
    }
}

/// The C++'s std::get<std::string> of a field: it must be text.
#[track_caller]
fn text(value: Option<&FieldValue>) -> &str {
    match value {
        Some(FieldValue::Text(text)) => text,
        other => panic!("not text: {other:?}"),
    }
}

/// The field `key` of the first record of `kind`, when both are there.
fn field_of(reply: &str, kind: &str, key: &str) -> Option<String> {
    GeoFixture::record(reply, kind).and_then(|record| record.get(key).map(String::from))
}

/// The texts a DXF holds, read back through the importer.
fn texts_of(path: &str) -> Vec<String> {
    let read = read_dxf_import(Path::new(path), None);
    assert!(read.is_ok(), "{path}");
    let mut found = Vec::new();
    if let Ok(read) = read {
        for entity in &read.entities {
            if let Geometry::Text(text) = &entity.geometry {
                found.push(text.text.clone());
            }
        }
    }
    found
}

/// A smart leader from (10, 60) to (30, 70) whose tip is on `target` and
/// whose note reads its level.
fn smart_leader(target: EntityId) -> Geometry {
    Geometry::Leader(LeaderGeometry {
        vertices: vec![Point2::new(10.0, 60.0), Point2::new(30.0, 70.0)],
        tip_ref: AnchorRef {
            entity: target,
            point: AnchorPoint::Position,
            ..AnchorRef::default()
        },
        text: String::from("RL {rl:.3f}"),
        fields: true,
        ..LeaderGeometry::default()
    })
}

// PROJ's own cs2cs puts 330000,6250000 in EPSG:28356 at 151.161906845856,
// -33.876653622997 (`echo 330000 6250000 | cs2cs -f %.12f EPSG:28356
// EPSG:4326`, as tests/geo/test_vector_fidelity.cpp records it).
const LONGITUDE: f64 = 151.161906845856;
const LATITUDE: f64 = -33.876653622997;

mod export_options {
    use super::*;

    // ---- what is written ----

    #[test]
    fn export_writes_the_project_crs() {
        let mut fixture = fixture("ExportWritesTheProjectCrs");
        draw_site(&mut fixture);
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:7856", Document::SET_CRS),
        );
        let out = fixture.scratch.file("site.gpkg");
        fixture.ok(&format!("EXPORT {}", quoted(&out)));
        let dataset = must(GdalDataset::open(Path::new(&out)));
        let written = must(dataset.vector_layers());
        assert_eq!(written.len(), 1);
        assert_eq!(crs_epsg_code(&written[0].projection_wkt), Some(7856));
    }

    #[test]
    fn a_kml_says_it_is_in_longitude_and_latitude_not_the_projects() {
        // The writer converts a KML to WGS 84 whatever the drawing is in; the
        // reply named the project's system, which the file is not in.
        let mut fixture = fixture("AKmlSaysItIsInLongitudeAndLatitudeNotTheProjects");
        fixture.add(point(330000.0, 6250000.0), "pegs", PropertyMap::new());
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:28356", Document::SET_CRS),
        );
        let reply = fixture.ok(&format!(
            "EXPORT {}",
            quoted(&fixture.scratch.file("pegs.kml"))
        ));
        let crs = field_of(&reply, "exported", "crs");
        assert!(crs.is_some(), "{reply}");
        holds(&crs.unwrap_or_default(), "EPSG:4326");
    }

    #[test]
    fn a_writers_failure_names_the_file_asked_for_not_its_staging_folder() {
        // A KML of a drawing in no coordinate system is refused by the
        // writer, which wrote into the staging folder: the refusal named
        // ".katana-staging-<pid>-<n>/pegs.kml".
        let mut fixture = fixture("AWritersFailureNamesTheFileAskedForNotItsStagingFolder");
        fixture.add(point(1.0, 2.0), "pegs", PropertyMap::new());
        let out = fixture.scratch.file("pegs.kml");
        let error = refused(
            fixture.run(&format!("EXPORT {}", quoted(&out))),
            "a KML of a drawing in no system",
        );
        assert_eq!(error.code, ErrorCode::InvalidCrs);
        lacks(&error.describe(), ".katana-staging");
        holds(&error.context, "pegs.kml");
    }

    #[test]
    fn entities_imported_moved_go_out_in_no_coordinate_system() {
        // Imported LOCAL, the lot is moved to 0,0: its coordinates are in
        // neither its file's system nor the project's, and a GeoPackage that
        // claimed the project's put it off the coast of Africa.
        let mut fixture = fixture("EntitiesImportedMovedGoOutInNoCoordinateSystem");
        let lot = fixture.scratch.file("lot.geojson");
        std::fs::write(
            &lot,
            concat!(
                r#"{"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::28356"}},"#,
                r#""features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[330000,6250000]}}]}"#
            ),
        )
        .expect("the file is written");
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:28356", Document::SET_CRS),
        );
        fixture.ok(&format!("IMPORT {} LOCAL", quoted(&lot)));
        let out = fixture.scratch.file("moved.gpkg");
        let reply = fixture.ok(&format!("EXPORT {}", quoted(&out)));
        let exported = GeoFixture::record(&reply, "exported");
        assert!(exported.is_some(), "{reply}");
        assert_eq!(
            exported.as_ref().and_then(|record| record.get("crs")),
            Some(""),
            "{reply}"
        );
        holds(&reply, "imported moved from their file's coordinates");
        let dataset = must(GdalDataset::open(Path::new(&out)));
        let written = must(dataset.vector_layers());
        assert_eq!(written.len(), 1);
        assert!(written[0].projection_wkt.is_empty());
        // Nor can they be converted into another system.
        let error = refused(
            fixture.run(&format!(
                "EXPORT {}",
                quoted(&fixture.scratch.file("moved.kml"))
            )),
            "a KML of entities in no system",
        );
        assert_eq!(error.code, ErrorCode::InvalidCrs);
    }

    #[test]
    fn export_of_layers_only_writes_those_layers() {
        let mut fixture = fixture("ExportOfLayersOnlyWritesThoseLayers");
        draw_site(&mut fixture);
        let out = fixture.scratch.file("some.gpkg");
        let reply = fixture.ok(&format!("EXPORT {} LAYERS lots,pegs", quoted(&out)));
        assert_eq!(layers_of(&out), layers(&[("katana", 3)]));
        assert_eq!(
            field_of(&reply, "scope", "matched"),
            Some(String::from("3")),
            "{reply}"
        );
        // The exported record stays the first, as it always was.
        assert_eq!(parse_records(&reply)[0].kind, "exported");
    }

    #[test]
    fn where_filters_the_export() {
        let mut fixture = fixture("WhereFiltersTheExport");
        draw_site(&mut fixture);
        let out = fixture.scratch.file("areas.gpkg");
        fixture.ok(&format!(
            "EXPORT {} DRAWING WHERE TYPE=polyline layername=areas",
            quoted(&out)
        ));
        assert_eq!(layers_of(&out), layers(&[("areas", 2)]));
    }

    #[test]
    fn split_by_layer_writes_one_layer_each() {
        let mut fixture = fixture("SplitByLayerWritesOneLayerEach");
        draw_site(&mut fixture);
        let out = fixture.scratch.file("split.gpkg");
        fixture.ok(&format!("EXPORT {} split=layer", quoted(&out)));
        assert_eq!(
            layers_of(&out),
            layers(&[("lots", 2), ("pegs", 1), ("roads", 1)])
        );
    }

    #[test]
    fn append_adds_a_second_layer_to_a_gpkg() {
        let mut fixture = fixture("AppendAddsASecondLayerToAGpkg");
        draw_site(&mut fixture);
        let out = fixture.scratch.file("both.gpkg");
        fixture.ok(&format!(
            "EXPORT {} LAYERS lots layername=lots",
            quoted(&out)
        ));
        fixture.ok(&format!(
            "EXPORT {} LAYERS roads layername=roads append",
            quoted(&out)
        ));
        assert_eq!(layers_of(&out), layers(&[("lots", 2), ("roads", 1)]));
        // A layer name the file has already is refused, and the file kept.
        let error = refused(
            fixture.run(&format!(
                "EXPORT {} LAYERS pegs layername=roads append",
                quoted(&out)
            )),
            "a layer the file has already",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(layers_of(&out), layers(&[("lots", 2), ("roads", 1)]));
    }

    #[test]
    fn an_unknown_creation_option_is_refused() {
        let mut fixture = fixture("AnUnknownCreationOptionIsRefused");
        draw_site(&mut fixture);
        let out = fixture.scratch.file("refused.gpkg");
        let error = refused(
            fixture.run(&format!("EXPORT {} lco=NO_SUCH_OPTION=1", quoted(&out))),
            "a layer creation option the driver does not declare",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        holds(&error.message, "NO_SUCH_OPTION");
        assert!(!Path::new(&out).exists());
        // One the GeoPackage driver declares is written with.
        fixture.ok(&format!("EXPORT {} lco=SPATIAL_INDEX=NO", quoted(&out)));
        assert_eq!(layers_of(&out).get("katana").copied(), Some(4));
    }

    #[test]
    fn text_becomes_points_with_text_fields() {
        let mut fixture = fixture("TextBecomesPointsWithTextFields");
        let words = TextGeometry {
            position: Point2::new(10.0, 20.0),
            text: String::from("MH 12"),
            height: 2.5,
            rotation: std::f64::consts::PI / 6.0, // 30 degrees
            ..TextGeometry::default()
        };
        fixture.add(Geometry::Text(words), "notes", PropertyMap::new());
        let out = fixture.scratch.file("text.gpkg");
        // Left out, a text is all there is: nothing to export, said.
        assert!(fixture.run(&format!("EXPORT {}", quoted(&out))).is_err());

        let reply = fixture.ok(&format!("EXPORT {} text=points", quoted(&out)));
        assert_eq!(
            field_of(&reply, "exported", "texts"),
            Some(String::from("1"))
        );
        let written = table(&out, 0);
        assert_eq!(written.features.len(), 1);
        let feature = &written.features[0];
        assert_eq!(feature.parts.len(), 1);
        assert_eq!(feature.parts[0].kind, GeometryKind::Point);
        double_eq(feature.parts[0].parts[0][0].x, 10.0);
        double_eq(feature.parts[0].parts[0][0].y, 20.0);
        let note = field(&written, feature, "text");
        assert!(note.is_some());
        assert_eq!(text(note), "MH 12");
        double_eq(real(field(&written, feature, "text_height")), 2.5);
        // pi / 6 radians is 30 degrees; 1e-12 is far above the rounding of one
        // multiplication and one division.
        near(real(field(&written, feature, "text_rotation")), 30.0, 1e-12);
        let style = field(&written, feature, "OGR_STYLE");
        assert!(style.is_some());
        holds(text(style), "LABEL(t:\"MH 12\"");
    }

    #[test]
    fn properties_no_writes_none() {
        let mut fixture = fixture("PropertiesNoWritesNone");
        let owner = PropertyMap::from([(
            String::from("owner"),
            PropertyValue::Text(String::from("Smith")),
        )]);
        fixture.rect((0.0, 0.0), (10.0, 10.0), "lots", owner);
        let out = fixture.scratch.file("bare.gpkg");
        fixture.ok(&format!("EXPORT {} properties=no", quoted(&out)));
        for written in &table(&out, 0).fields {
            assert_ne!(written.name, "owner");
        }
    }

    // ---- coordinate systems ----

    #[test]
    fn geo_json_of_a_projected_drawing_is_lon_lat() {
        let mut fixture = fixture("GeoJsonOfAProjectedDrawingIsLonLat");
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:28356", Document::SET_CRS),
        );
        fixture.add(point(330000.0, 6250000.0), "pegs", PropertyMap::new());
        let out = fixture.scratch.file("peg.geojson");
        let reply = fixture.ok(&format!("EXPORT {}", quoted(&out)));
        holds(&reply, "RFC 7946");
        let written = table(&out, 0);
        assert_eq!(written.features.len(), 1);
        let at = &written.features[0].parts[0].parts[0][0];
        // 1e-9 degrees is about 0.1 mm: the cs2cs figures carry 12 decimals.
        near(at.x, LONGITUDE, 1e-9);
        near(at.y, LATITUDE, 1e-9);
    }

    #[test]
    fn crs_native_keeps_the_projected_crs_in_a_geo_json() {
        let mut fixture = fixture("CrsNativeKeepsTheProjectedCrsInAGeoJson");
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:28356", Document::SET_CRS),
        );
        fixture.add(point(330000.0, 6250000.0), "pegs", PropertyMap::new());
        let out = fixture.scratch.file("native.geojson");
        fixture.ok(&format!("EXPORT {} crs=native", quoted(&out)));
        let written = table(&out, 0);
        assert_eq!(written.features.len(), 1);
        let at = &written.features[0].parts[0].parts[0][0];
        double_eq(at.x, 330000.0);
        double_eq(at.y, 6250000.0);
        assert_eq!(crs_epsg_code(&written.crs_wkt), Some(28356));
    }

    #[test]
    fn crs_code_moves_the_coordinates_on_the_way_out() {
        let mut fixture = fixture("CrsCodeMovesTheCoordinatesOnTheWayOut");
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:28356", Document::SET_CRS),
        );
        fixture.add(point(330000.0, 6250000.0), "pegs", PropertyMap::new());
        let out = fixture.scratch.file("moved.gpkg");
        fixture.ok(&format!("EXPORT {} crs=EPSG:4326", quoted(&out)));
        let written = table(&out, 0);
        assert_eq!(written.features.len(), 1);
        let at = &written.features[0].parts[0].parts[0][0];
        near(at.x, LONGITUDE, 1e-9);
        near(at.y, LATITUDE, 1e-9);
        assert_eq!(crs_epsg_code(&written.crs_wkt), Some(4326));
    }

    #[test]
    fn crs_code_without_a_project_crs_is_refused() {
        let mut fixture = fixture("CrsCodeWithoutAProjectCrsIsRefused");
        draw_site(&mut fixture);
        let error = refused(
            fixture.run(&format!(
                "EXPORT {} crs=EPSG:4326",
                quoted(&fixture.scratch.file("x.gpkg"))
            )),
            "crs=<code> with no project system",
        );
        assert_eq!(error.code, ErrorCode::InvalidCrs);
    }

    // ---- the native formats, PREVIEW and nothing taken ----

    #[test]
    fn a_scoped_dxf_export_writes_only_the_scope() {
        let mut fixture = fixture("AScopedDxfExportWritesOnlyTheScope");
        draw_site(&mut fixture);
        let out = fixture.scratch.file("lots.dxf");
        fixture.ok(&format!("EXPORT {} LAYERS lots", quoted(&out)));
        let read = must(read_dxf_import(Path::new(&out), None));
        assert_eq!(read.entities.len(), 2);
    }

    #[test]
    fn a_scoped_dxf_export_keeps_the_note_of_a_leader_whose_target_the_scope_left_out() {
        // A point at RL 30 on layer pegs, and on layer notes a smart leader
        // whose tip is on it and whose note reads its level: "RL 30.000".
        // Exporting LAYERS notes takes the leader and not the point; the
        // writer was handed a copy of the drawing with the point already
        // removed, and the leader, reading nothing, was written as a bare
        // arrow - callout text silently missing, skipped=0, no warning. A note
        // is read off the whole drawing.
        let mut fixture =
            fixture("AScopedDxfExportKeepsTheNoteOfALeaderWhoseTargetTheScopeLeftOut");
        let level = PropertyMap::from([(String::from("elevation"), PropertyValue::Real(30.0))]);
        let peg = fixture.add(point(10.0, 60.0), "pegs", level);
        fixture.add(smart_leader(peg), "notes", PropertyMap::new());

        let whole = fixture.scratch.file("whole.dxf");
        fixture.ok(&format!("EXPORT {}", quoted(&whole)));
        let expected = vec![String::from("RL 30.000")];
        assert_eq!(
            texts_of(&whole),
            expected,
            "the control: the whole drawing says it"
        );

        let scoped = fixture.scratch.file("notes.dxf");
        let reply = fixture.ok(&format!("EXPORT {} LAYERS notes", quoted(&scoped)));
        holds(&reply, "entities=1");
        lacks(&reply, "without its note");
        assert_eq!(texts_of(&scoped), expected);

        // A selection of the leader alone is the same scope in another word.
        let note = fixture.on("notes")[0].id;
        fixture.type_line(&format!("SELECT {note}"));
        let selected = fixture.scratch.file("selection.dxf");
        fixture.ok(&format!("EXPORT {} SELECTION", quoted(&selected)));
        assert_eq!(texts_of(&selected), expected);
    }

    #[test]
    fn a_leader_whose_note_cannot_be_read_is_written_and_the_export_says_it_has_no_note() {
        // The entity its tip is on has no level, which the note reads: nothing
        // to read, a bare arrow in the file - and the reply says which leader,
        // rather than leave the callout silently blank. (A target that is not
        // in the drawing at all cannot be built here: a smart leader goes with
        // its target.)
        let mut fixture =
            fixture("ALeaderWhoseNoteCannotBeReadIsWrittenAndTheExportSaysItHasNoNote");
        let bare = fixture.add(point(10.0, 60.0), "pegs", PropertyMap::new());
        let note = fixture.add(smart_leader(bare), "notes", PropertyMap::new());
        let blank = fixture.scratch.file("blank.dxf");
        let reply = fixture.ok(&format!("EXPORT {}", quoted(&blank)));
        holds(
            &reply,
            &format!("leader #{note} is written without its note"),
        );
        assert!(texts_of(&blank).is_empty());
        // A plain leader's text is its own, and says nothing of the kind.
        let plain = LeaderGeometry {
            vertices: vec![Point2::new(10.0, 60.0), Point2::new(30.0, 70.0)],
            text: String::from("plain"),
            ..LeaderGeometry::default()
        };
        fixture.add(Geometry::Leader(plain), "plain", PropertyMap::new());
        let out = fixture.scratch.file("plain.dxf");
        let again = fixture.ok(&format!("EXPORT {} LAYERS plain", quoted(&out)));
        lacks(&again, "without its note");
        assert_eq!(texts_of(&out), vec![String::from("plain")]);
    }

    #[test]
    fn gdal_options_for_a_dxf_are_refused_by_name() {
        let mut fixture = fixture("GdalOptionsForADxfAreRefusedByName");
        draw_site(&mut fixture);
        let error = refused(
            fixture.run(&format!(
                "EXPORT {} split=layer",
                quoted(&fixture.scratch.file("x.dxf"))
            )),
            "a GDAL option for a DXF",
        );
        holds(&error.message, "split=");
    }

    #[test]
    fn preview_writes_nothing() {
        let mut fixture = fixture("PreviewWritesNothing");
        draw_site(&mut fixture);
        let out = fixture.scratch.file("preview.gpkg");
        let reply = fixture.ok(&format!("EXPORT {} LAYERS lots PREVIEW", quoted(&out)));
        assert!(!Path::new(&out).exists());
        assert!(GeoFixture::record(&reply, "export").is_some(), "{reply}");
        assert_eq!(
            field_of(&reply, "export", "preview"),
            Some(String::from("yes"))
        );
        assert_eq!(
            field_of(&reply, "export", "entities"),
            Some(String::from("2"))
        );
    }

    #[test]
    fn a_scope_that_takes_nothing_writes_nothing_and_says_so() {
        let mut fixture = fixture("AScopeThatTakesNothingWritesNothingAndSaysSo");
        draw_site(&mut fixture);
        let out = fixture.scratch.file("none.gpkg");
        let reply = fixture.ok(&format!(
            "EXPORT {} DRAWING WHERE TYPE=circle",
            quoted(&out)
        ));
        assert!(!Path::new(&out).exists());
        assert_eq!(field_of(&reply, "export", "ran"), Some(String::from("no")));
        assert_eq!(
            field_of(&reply, "scope", "matched"),
            Some(String::from("0"))
        );
    }

    #[test]
    fn a_plain_export_still_writes_the_whole_drawing() {
        let mut fixture = fixture("APlainExportStillWritesTheWholeDrawing");
        draw_site(&mut fixture);
        let out = fixture.scratch.file("all.gpkg");
        let reply = fixture.ok(&format!("EXPORT {}", quoted(&out)));
        assert_eq!(layers_of(&out), layers(&[("katana", 4)]));
        assert!(GeoFixture::record(&reply, "scope").is_none());
    }
}
