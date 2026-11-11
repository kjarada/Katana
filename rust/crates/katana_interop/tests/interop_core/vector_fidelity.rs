//! tests/geo/test_vector_fidelity.cpp, every case but WarningsReachTheReply
//! (which drives the app's session: step 6): what IMPORT and EXPORT used to
//! lose without a word - a lot's hole, a field's type, a KML's geometry, a
//! CSV's geometry, a MapInfo table's last centimetre, a curve, a TIN - and
//! each driver of the export table read back after writing
//! (docs/interop.md, "Fidelity").
//!
//! Every expected value is worked by hand or taken from outside Katana: the
//! lon/lat of an MGA point from PROJ's own cs2cs, areas from the shoelace
//! formula on the corners written into the test.

use std::collections::BTreeMap;
use std::path::Path;

use katana_core::ErrorCode;
use katana_entity::entity::{heights_of, set_heights, type_name};
use katana_entity::{Entity, Geometry, Layer, Model, PointGeometry, PropertyValue};
use katana_geometry::{Point2, Polyline2, Segment2};
use katana_gis::gdal_adapter::{
    GdalDataset, GeoPoint, GeometryKind, VectorExportOptions as GdalExportOptions, VectorFeature,
    VectorGeometry, VectorReadOptions,
};
use katana_gis::processing::{FeatureTable, FieldType, FieldValue};
use katana_gis::reproject::transform_point;
use katana_interop::VectorImportResult;
use katana_interop::export::{VectorExportOptions, export_vector};
use katana_interop::import::{VectorImportOptions, import_vector};

use crate::{TempDir, file_text, must, near, refused, warned, write_file};

/// The MGA zone 56 of GDA94: projected, metres, what a survey in Sydney is
/// in.
const MGA56: &str = "EPSG:28356";

fn temp_dir(name: &str) -> TempDir {
    TempDir::new("katana-fidelity-", name)
}

fn import(path: &Path) -> katana_core::Result<VectorImportResult> {
    import_vector(path, &VectorImportOptions::default())
}

fn export(model: &Model, path: &Path) -> katana_core::Result<katana_interop::VectorExportResult> {
    export_vector(model, path, &VectorExportOptions::default())
}

fn projected() -> VectorExportOptions {
    VectorExportOptions {
        projection_wkt: String::from(MGA56),
        ..VectorExportOptions::default()
    }
}

fn model_with(entities: Vec<Entity>) -> Model {
    let mut model = Model::default();
    for entity in entities {
        if !model.layers.contains(&entity.layer) {
            let layer = Layer {
                name: entity.layer.clone(),
                ..Layer::default()
            };
            assert!(model.layers.add(layer).is_ok(), "{}", entity.layer);
        }
        assert!(model.entities.add(entity).is_ok());
    }
    model
}

/// What an import gave, in a model, as the application puts it there.
fn model_of(imported: &VectorImportResult) -> Model {
    let mut model = Model::default();
    for name in &imported.layers_needed {
        let layer = Layer {
            name: name.clone(),
            ..Layer::default()
        };
        assert!(model.layers.add(layer).is_ok(), "{name}");
    }
    for entity in &imported.entities {
        assert!(model.entities.add(entity.clone()).is_ok());
    }
    model
}

fn square(x: f64, y: f64, side: f64) -> Entity {
    Entity {
        geometry: Geometry::Polyline(Polyline2::new(
            vec![
                Point2::new(x, y),
                Point2::new(x + side, y),
                Point2::new(x + side, y + side),
                Point2::new(x, y + side),
            ],
            true,
        )),
        ..Entity::default()
    }
}

fn point_entity(x: f64, y: f64) -> Entity {
    Entity {
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(x, y),
        }),
        ..Entity::default()
    }
}

fn segment_entity(start: (f64, f64), end: (f64, f64)) -> Entity {
    Entity {
        geometry: Geometry::Line(Segment2::new(
            Point2::new(start.0, start.1),
            Point2::new(end.0, end.1),
        )),
        ..Entity::default()
    }
}

fn ring_area(ring: &[GeoPoint]) -> f64 {
    let mut twice = 0.0;
    for (i, a) in ring.iter().enumerate() {
        let b = &ring[(i + 1) % ring.len()];
        twice += a.x * b.y - b.x * a.y;
    }
    f64::abs(twice) / 2.0
}

/// The file's first layer as the one read gives it.
fn read_back(path: &Path) -> FeatureTable {
    let dataset = must(GdalDataset::open(path));
    must(dataset.read_table(0, &VectorReadOptions::default(), None))
}

fn field_index(table: &FeatureTable, name: &str) -> usize {
    table
        .fields
        .iter()
        .position(|field| field.name == name)
        .unwrap_or_else(|| panic!("no field {name}"))
}

fn property<'a>(entity: &'a Entity, key: &str) -> Option<&'a PropertyValue> {
    entity.properties.get(key)
}

fn text(value: &str) -> PropertyValue {
    PropertyValue::Text(String::from(value))
}

/// Two lines with typed properties, the kerb a segment and the fence a
/// polyline, at survey coordinates.
fn kerb_and_fence() -> (Entity, Entity) {
    let mut kerb = segment_entity((330000.0, 6250000.0), (330030.0, 6250040.0));
    kerb.properties.insert(String::from("code"), text("KERB"));
    kerb.properties
        .insert(String::from("width"), PropertyValue::Real(0.15));
    kerb.properties
        .insert(String::from("count"), PropertyValue::Integer(3));
    let mut fence = Entity {
        geometry: Geometry::Polyline(Polyline2::new(
            vec![
                Point2::new(330000.0, 6250100.0),
                Point2::new(330010.0, 6250105.0),
                Point2::new(330020.0, 6250100.0),
            ],
            false,
        )),
        ..Entity::default()
    };
    fence.properties.insert(String::from("code"), text("FENCE"));
    fence
        .properties
        .insert(String::from("width"), PropertyValue::Real(0.05));
    fence
        .properties
        .insert(String::from("count"), PropertyValue::Integer(12));
    (kerb, fence)
}

mod vector_fidelity {
    use super::*;

    // ---- a hole is a hole -----------------------------------------------------------------

    #[test]
    fn a_lot_with_a_hole_round_trips_through_gpkg_with9600_square_metres() {
        // The defect: a 100 m lot with a 20 m hole, imported and exported,
        // came back as two polygons summing to 10400 m2. By hand: 100 x 100 -
        // 20 x 20 = 9600.
        let dir = temp_dir("hole");
        let ring = |corners: [(f64, f64); 5]| -> Vec<GeoPoint> {
            corners
                .iter()
                .map(|&(x, y)| GeoPoint { x, y, z: 0.0 })
                .collect()
        };
        let mut lot = VectorFeature {
            geometry: VectorGeometry {
                kind: GeometryKind::Polygon,
                parts: vec![
                    ring([
                        (330000.0, 6250000.0),
                        (330100.0, 6250000.0),
                        (330100.0, 6250100.0),
                        (330000.0, 6250100.0),
                        (330000.0, 6250000.0),
                    ]),
                    ring([
                        (330040.0, 6250040.0),
                        (330060.0, 6250040.0),
                        (330060.0, 6250060.0),
                        (330040.0, 6250060.0),
                        (330040.0, 6250040.0),
                    ]),
                ],
                ..VectorGeometry::default()
            },
            ..VectorFeature::default()
        };
        lot.attributes
            .insert(String::from("lot"), String::from("7"));
        let write = GdalExportOptions {
            projection_wkt: String::from(MGA56),
            ..GdalExportOptions::default()
        };
        must(GdalDataset::write_vector(
            &dir.file("lot.gpkg"),
            &[lot],
            &write,
        ));

        let imported = must(import(&dir.file("lot.gpkg")));
        assert_eq!(
            imported.entities.len(),
            2,
            "a ring each: the model has no polygon with holes"
        );
        let model = model_of(&imported);

        let exported = must(export_vector(&model, &dir.file("back.gpkg"), &projected()));
        assert_eq!(
            exported.features_written, 1,
            "one lot, not a lot and a solid hole"
        );

        let table = read_back(&dir.file("back.gpkg"));
        assert_eq!(table.features.len(), 1);
        let rings = &table.features[0].parts[0].parts;
        assert_eq!(rings.len(), 2);
        near(ring_area(&rings[0]) - ring_area(&rings[1]), 9600.0, 1e-6);
    }

    // ---- field types ----------------------------------------------------------------------

    #[test]
    fn real_and_integer_properties_export_typed() {
        // An area of 8400.5 went out as the text "8400.5", and SUM() over it
        // in any GIS was then no sum at all.
        let dir = temp_dir("typed-out");
        let mut parcel = square(0.0, 0.0, 10.0);
        parcel
            .properties
            .insert(String::from("area_m2"), PropertyValue::Real(8400.5));
        parcel
            .properties
            .insert(String::from("lot"), PropertyValue::Integer(7));
        parcel
            .properties
            .insert(String::from("sealed"), PropertyValue::Boolean(true));
        parcel
            .properties
            .insert(String::from("owner"), text("Smith"));
        let model = model_with(vec![parcel]);
        must(export(&model, &dir.file("typed.gpkg")));

        let table = read_back(&dir.file("typed.gpkg"));
        assert_eq!(table.features.len(), 1);
        let type_of = |name: &str| table.fields[field_index(&table, name)].r#type;
        assert_eq!(type_of("katana_id"), FieldType::Integer64);
        assert_eq!(type_of("layer"), FieldType::String);
        assert_eq!(type_of("area_m2"), FieldType::Real);
        assert_eq!(type_of("lot"), FieldType::Integer64);
        assert_eq!(type_of("sealed"), FieldType::Boolean);
        assert_eq!(type_of("owner"), FieldType::String);
        let values = &table.features[0].values;
        assert_eq!(
            values[field_index(&table, "area_m2")],
            FieldValue::Real(8400.5)
        );
        assert_eq!(values[field_index(&table, "lot")], FieldValue::Integer64(7));
        assert_eq!(
            values[field_index(&table, "sealed")],
            FieldValue::Boolean(true)
        );
    }

    #[test]
    fn typed_fields_import_typed() {
        let dir = temp_dir("typed-in");
        write_file(
            &dir.file("parcels.geojson"),
            r#"{"type": "FeatureCollection", "features": [
        {"type": "Feature",
         "properties": {"lot": 7, "area": 8400.5, "sealed": true, "owner": "Smith",
                        "surveyed": "2024-05-01", "missing": null},
         "geometry": {"type": "Point", "coordinates": [1, 2]}}]}"#,
        );
        let imported = must(import(&dir.file("parcels.geojson")));
        assert_eq!(imported.entities.len(), 1);
        let entity = &imported.entities[0];
        assert_eq!(property(entity, "lot"), Some(&PropertyValue::Integer(7)));
        assert_eq!(property(entity, "area"), Some(&PropertyValue::Real(8400.5)));
        assert_eq!(
            property(entity, "sealed"),
            Some(&PropertyValue::Boolean(true))
        );
        assert_eq!(property(entity, "owner"), Some(&text("Smith")));
        // A date is ISO 8601 text, whatever GDAL typed it as.
        assert_eq!(property(entity, "surveyed"), Some(&text("2024-05-01")));
        assert!(
            property(entity, "missing").is_none(),
            "absent is no property, not an empty one"
        );
    }

    #[test]
    fn dates_imported_come_back_from_export_as_dates() {
        // GDAL reads "2024-05-01" in a GeoJSON as a Date and the time as a
        // DateTime; a property holds them as text, and EXPORT wrote that text
        // as a String field, so a date filter on the exported file found
        // nothing.
        let dir = temp_dir("dates");
        write_file(
            &dir.file("dated.geojson"),
            r#"{"type": "FeatureCollection", "features": [
        {"type": "Feature",
         "properties": {"surveyed": "2024-05-01", "stamp": "2024-05-01T10:20:30"},
         "geometry": {"type": "Point", "coordinates": [1, 2]}}]}"#,
        );
        let imported = must(import(&dir.file("dated.geojson")));
        // The value stays the ISO 8601 text.
        assert_eq!(imported.entities.len(), 1);
        assert_eq!(
            property(&imported.entities[0], "surveyed"),
            Some(&text("2024-05-01"))
        );
        let model = model_of(&imported);
        must(export(&model, &dir.file("dated.gpkg")));
        let table = read_back(&dir.file("dated.gpkg"));
        assert_eq!(table.features.len(), 1);
        let type_of = |name: &str| table.fields[field_index(&table, name)].r#type;
        assert_eq!(type_of("surveyed"), FieldType::Date);
        assert_eq!(type_of("stamp"), FieldType::DateTime);
        let values = &table.features[0].values;
        assert_eq!(
            values[field_index(&table, "surveyed")],
            FieldValue::Text(String::from("2024-05-01"))
        );
        let FieldValue::Text(stamp) = &values[field_index(&table, "stamp")] else {
            panic!("the stamp is text");
        };
        assert_eq!(&stamp[..19], "2024-05-01T10:20:30");
        // Text typed as a date by no one stays text: a property set by hand
        // carries no tag.
        let mut typed = square(0.0, 0.0, 10.0);
        typed
            .properties
            .insert(String::from("surveyed"), text("2024-05-01"));
        must(export(&model_with(vec![typed]), &dir.file("typed.gpkg")));
        let plain = read_back(&dir.file("typed.gpkg"));
        assert_eq!(
            plain.fields[field_index(&plain, "surveyed")].r#type,
            FieldType::String
        );
    }

    // ---- KML holds longitude and latitude ---------------------------------------------------

    #[test]
    fn kml_of_a_projected_drawing_is_reprojected_to_lon_lat() {
        // GDAL's KML writer, handed MGA coordinates, wrote a placemark
        // without its geometry and succeeded. The expected lon/lat is PROJ's
        // own, from `echo 330000 6250000 | cs2cs -f %.12f EPSG:28356
        // EPSG:4326`: -33.876653622997 151.161906845856 (latitude first, as
        // EPSG orders it).
        let dir = temp_dir("kml");
        let mut peg = point_entity(330000.0, 6250000.0);
        peg.properties.insert(String::from("code"), text("PEG"));
        let model = model_with(vec![peg]);
        let exported = must(export_vector(&model, &dir.file("pegs.kml"), &projected()));
        assert!(
            warned(&exported.warnings, "longitude and latitude"),
            "the conversion is said, not done silently"
        );

        let table = read_back(&dir.file("pegs.kml"));
        assert_eq!(table.features.len(), 1);
        assert_eq!(
            table.features[0].parts.len(),
            1,
            "the placemark has its geometry"
        );
        let at = table.features[0].parts[0].parts[0][0];
        // 1e-8 degrees is about a millimetre; cs2cs printed twelve places.
        near(at.x, 151.161906845856, 1e-8);
        near(at.y, -33.876653622997, 1e-8);
    }

    #[test]
    fn kml_without_a_project_crs_is_refused_not_written_empty() {
        let dir = temp_dir("kml-no-crs");
        let model = model_with(vec![point_entity(330000.0, 6250000.0)]);
        let error = refused(
            export(&model, &dir.file("pegs.kml")),
            "a KML without a coordinate system",
        );
        assert_eq!(error.code, ErrorCode::InvalidCrs);
        assert!(
            error.message.contains("CRS SET"),
            "the refusal says what to do: {}",
            error.message
        );
        assert!(!dir.file("pegs.kml").exists(), "nothing is written");
    }

    #[test]
    fn a_kml_keeps_its_heights_and_invents_none() {
        // KML's default altitude mode puts a coordinate on the ground
        // whatever its altitude, so the export says "absolute" for a heighted
        // feature, and the import takes an altitude as a height only when it
        // says so. GDAL's KMZ writer (LIBKML) gives a 2D coordinate an
        // altitude of 0, which only the mode keeps from reading as a height.
        // A .kml is written by LIBKML too, GDAL's own choice for the name
        // (docs/interop.md, "Formats"); the older KML driver, named, is kept
        // to its word as well.
        let dir = temp_dir("kml-heights");
        let mut surveyed = point_entity(330000.0, 6250000.0);
        set_heights(&mut surveyed.properties, &[Some(32.5)]);
        let plan_only = point_entity(330010.0, 6250000.0);
        let model = model_with(vec![surveyed, plan_only]);
        for (file, driver) in [
            ("heights.kml", ""),
            ("heights.kmz", ""),
            ("heights-kml-driver.kml", "KML"),
        ] {
            let options = VectorExportOptions {
                driver: String::from(driver),
                ..projected()
            };
            must(export_vector(&model, &dir.file(file), &options));

            let imported = must(import(&dir.file(file)));
            assert_eq!(imported.entities.len(), 2, "{file}");
            let mut heighted = 0;
            for entity in &imported.entities {
                if let Some(z) = heights_of(&entity.properties, 1)[0] {
                    assert_eq!(z, 32.5, "{file}");
                    heighted += 1;
                }
            }
            assert_eq!(
                heighted, 1,
                "{file}: the plan-only point must not come back at 0"
            );
        }
    }

    // ---- CSV carries its geometry -----------------------------------------------------------

    #[test]
    fn csv_export_carries_wkt_geometry() {
        // CSV used to be written as katana_id,layer and nothing else -
        // reported as exported. A mix of kinds goes as WKT, typed by the .csvt
        // beside it.
        let dir = temp_dir("csv");
        let mut kerb = segment_entity((0.0, 0.0), (30.0, 40.0));
        kerb.properties
            .insert(String::from("width"), PropertyValue::Real(0.15));
        let lot = square(100.0, 100.0, 20.0);
        let peg = point_entity(5.0, 7.0);
        let model = model_with(vec![kerb, lot, peg]);
        must(export(&model, &dir.file("mixed.csv")));
        let written = file_text(&dir.file("mixed.csv"));
        assert!(written.contains("LINESTRING (0 0,30 40)"), "{written}");
        assert!(
            written.contains("POLYGON ((100 100,120 100,120 120,100 120,100 100))"),
            "{written}"
        );
        assert!(dir.file("mixed.csvt").exists());

        let imported = must(import(&dir.file("mixed.csv")));
        assert_eq!(imported.entities.len(), 3);
        for entity in &imported.entities {
            assert!(
                property(entity, "WKT").is_none(),
                "the geometry column is not a property"
            );
            match &entity.geometry {
                Geometry::Line(line) => {
                    assert_eq!(line.length(), 50.0); // 3-4-5, times 10
                    assert_eq!(property(entity, "width"), Some(&PropertyValue::Real(0.15)));
                }
                Geometry::Polyline(ring) => {
                    assert!(ring.closed);
                    assert_eq!(ring.area(), 400.0);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn a_csv_of_surveyed_points_has_xy_and_z_columns_and_keeps_its_heights() {
        let dir = temp_dir("csv-points");
        let mut a = point_entity(330000.125, 6250000.5);
        set_heights(&mut a.properties, &[Some(32.25)]);
        let mut b = point_entity(330001.125, 6250000.5);
        set_heights(&mut b.properties, &[Some(33.0)]);
        let model = model_with(vec![a, b]);
        must(export(&model, &dir.file("points.csv")));
        let written = file_text(&dir.file("points.csv"));
        assert!(written.starts_with("X,Y,Z,"), "{written}");

        let imported = must(import(&dir.file("points.csv")));
        assert_eq!(imported.entities.len(), 2);
        let Geometry::Point(first) = &imported.entities[0].geometry else {
            panic!("the first entity is a point");
        };
        assert_eq!(first.position, Point2::new(330000.125, 6250000.5));
        assert_eq!(
            heights_of(&imported.entities[0].properties, 1)[0],
            Some(32.25)
        );
        assert!(property(&imported.entities[0], "X").is_none());
        assert!(property(&imported.entities[0], "Z").is_none());
    }

    // ---- MapInfo keeps the millimetre -------------------------------------------------------

    #[test]
    fn tab_export_keeps_survey_coordinates_to_the_millimetre() {
        // The defect: LINE 330000,6250000 330100,6250100 to a .tab read back
        // as 330099.99 6250099.99. MapInfo keeps coordinates as integers
        // across its bounds, and the default bounds make that step a
        // centimetre; the data's own extent makes it far below 1e-4.
        let dir = temp_dir("tab");
        let line = segment_entity((330000.0, 6250000.0), (330100.0, 6250100.0));
        let other = segment_entity((330000.125, 6250000.0), (330100.456, 6250100.789));
        let model = model_with(vec![line, other]);
        must(export(&model, &dir.file("lines.tab")));

        let imported = must(import(&dir.file("lines.tab")));
        assert_eq!(imported.entities.len(), 2);
        let (Geometry::Line(back), Geometry::Line(back2)) = (
            &imported.entities[0].geometry,
            &imported.entities[1].geometry,
        ) else {
            panic!("both come back lines");
        };
        near(back.start.x, 330000.0, 1e-4);
        near(back.start.y, 6250000.0, 1e-4);
        near(back.end.x, 330100.0, 1e-4);
        near(back.end.y, 6250100.0, 1e-4);
        near(back2.start.x, 330000.125, 1e-4);
        near(back2.end.x, 330100.456, 1e-4);
        near(back2.end.y, 6250100.789, 1e-4);
    }

    // ---- curves and faces -------------------------------------------------------------------

    #[test]
    fn a_circular_string_imports_as_an_arc() {
        // Written as WKT in a CSV, which GDAL reads curves from as they are.
        // By hand: the circle through (0,0), (10,10) and (20,0) has its
        // centre at (10,0) and a radius of 10; ending where it starts through
        // (20,0) it is the whole circle with that centre and radius.
        let dir = temp_dir("arcs");
        write_file(
            &dir.file("curves.csv"),
            "WKT,what\n\
             \"CIRCULARSTRING (0 0,10 10,20 0)\",arc\n\
             \"CIRCULARSTRING (0 0,20 0,0 0)\",circle\n\
             \"CURVEPOLYGON (CIRCULARSTRING (0 0,20 0,0 0))\",disc\n\
             \"CIRCULARSTRING (0 0,10 10,20 0,30 -10,40 0)\",wave\n",
        );
        let imported = must(import(&dir.file("curves.csv")));
        let mut by_what: BTreeMap<String, &Entity> = BTreeMap::new();
        for entity in &imported.entities {
            let PropertyValue::Text(what) = &entity.properties["what"] else {
                panic!("what is text");
            };
            by_what.insert(what.clone(), entity);
        }
        assert_eq!(by_what.len(), 4);

        let Geometry::Arc(arc) = &by_what["arc"].geometry else {
            panic!("an arc, not three points of a polyline");
        };
        near(arc.center.x, 10.0, 1e-12);
        near(arc.center.y, 0.0, 1e-12);
        near(arc.radius, 10.0, 1e-12);
        near(arc.start_point().x, 0.0, 1e-9);
        near(arc.end_point().x, 20.0, 1e-9);
        // Through (10,10), the top, not the bottom.
        near(arc.midpoint().y, 10.0, 1e-9);

        for whole in ["circle", "disc"] {
            let Geometry::Circle(circle) = &by_what[whole].geometry else {
                panic!("{whole} is a circle");
            };
            assert_eq!(circle.center, Point2::new(10.0, 0.0), "{whole}");
            assert_eq!(circle.radius, 10.0, "{whole}");
        }

        // Two arcs in one line: chords within the import's 1 mm, every vertex
        // on one of the two circles (centres (10,0) and (30,0), radius 10).
        let Geometry::Polyline(wave) = &by_what["wave"].geometry else {
            panic!("the wave is a polyline");
        };
        assert!(wave.vertices.len() > 5);
        for vertex in &wave.vertices {
            let left = ((vertex.x - 10.0).hypot(vertex.y) - 10.0).abs();
            let right = ((vertex.x - 30.0).hypot(vertex.y) - 10.0).abs();
            assert!(
                katana_math::cpp::min(left, right) < 1e-9,
                "{},{}",
                vertex.x,
                vertex.y
            );
        }
        for pair in wave.vertices.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let middle = Point2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let centre = if middle.x < 20.0 { 10.0 } else { 30.0 };
            assert!(10.0 - (middle.x - centre).hypot(middle.y) <= 0.001 + 1e-12);
        }
        assert!(warned(&imported.warnings, "made chords"));
    }

    #[test]
    fn a_tin_is_reported_as_skipped() {
        let dir = temp_dir("tin");
        write_file(
            &dir.file("faces.csv"),
            "WKT,what\n\"TIN Z (((0 0 0,10 0 0,0 10 0,0 0 0)))\",tin\n\"POINT (1 2)\",peg\n",
        );
        let imported = must(import(&dir.file("faces.csv")));
        assert_eq!(imported.entities.len(), 1);
        assert_eq!(imported.features_read, 2);
        assert_eq!(imported.features_skipped, 1);
        assert_eq!(imported.skipped.get("tin"), Some(&1));
        assert!(
            warned(&imported.warnings, "1 TIN left out"),
            "said, never dropped silently"
        );
    }

    // ---- every driver of the export table -----------------------------------------------------

    #[test]
    fn round_trip_of_every_export_driver() {
        // Two lines with typed properties, written and read back by each
        // driver: the geometry to the micrometre (the lon/lat of KML to 1e-9
        // degrees, 0.1 mm), and the fields each format can hold with their
        // types.
        struct Case {
            file: &'static str,
            // DXF has only its own.
            fields: bool,
            // The types survive, not only the text.
            typed: bool,
        }
        let case = |file, fields, typed| Case {
            file,
            fields,
            typed,
        };
        let cases = [
            case("lines.shp", true, true),
            case("lines.geojson", true, true),
            case("lines.gpkg", true, true),
            case("lines.kml", true, true),
            case("lines.gml", true, true),
            case("lines.dxf", false, false),
            case("lines.csv", true, true),
            case("lines.sqlite", true, true),
            case("lines.tab", true, true),
        ];
        let (kerb, fence) = kerb_and_fence();
        let model = model_with(vec![kerb.clone(), fence.clone()]);

        for test in &cases {
            let dir = temp_dir(&format!("every-{}", test.file));
            let exported = must(export_vector(&model, &dir.file(test.file), &projected()));
            assert_eq!(exported.features_written, 2, "{}", test.file);

            let imported = must(import(&dir.file(test.file)));
            assert_eq!(imported.entities.len(), 2, "{}", test.file);
            let lon_lat = test.file.ends_with(".kml");
            for back in &imported.entities {
                let is_kerb = matches!(back.geometry, Geometry::Line(_));
                let original = if is_kerb { &kerb } else { &fence };
                let expected: Vec<Point2> = match &original.geometry {
                    Geometry::Line(segment) => vec![segment.start, segment.end],
                    Geometry::Polyline(line) => line.vertices.clone(),
                    _ => Vec::new(),
                };
                let got: Vec<Point2> = match &back.geometry {
                    Geometry::Line(segment) => vec![segment.start, segment.end],
                    Geometry::Polyline(line) => line.vertices.clone(),
                    _ => Vec::new(),
                };
                assert_eq!(got.len(), expected.len(), "{}", test.file);
                for (point, want) in got.iter().zip(&expected) {
                    let mut want = *want;
                    if lon_lat {
                        let moved = must(transform_point(want.x, want.y, MGA56, "EPSG:4326"));
                        want = Point2::new(moved[0], moved[1]);
                    }
                    let tolerance = if lon_lat { 1e-9 } else { 1e-6 };
                    near(point.x, want.x, tolerance);
                    near(point.y, want.y, tolerance);
                }
                if !test.fields {
                    continue;
                }
                assert_eq!(
                    property(back, "code"),
                    Some(&original.properties["code"]),
                    "{}",
                    test.file
                );
                let Some(width) = property(back, "width") else {
                    panic!("{}: no width", test.file);
                };
                let Some(count) = property(back, "count") else {
                    panic!("{}: no count", test.file);
                };
                if test.typed {
                    let PropertyValue::Real(width) = width else {
                        panic!("{}: the width is {}", test.file, type_name(width));
                    };
                    let PropertyValue::Real(original_width) = original.properties["width"] else {
                        panic!("the original width is a real");
                    };
                    near(*width, original_width, 1e-12);
                    assert_eq!(*count, original.properties["count"], "{}", test.file);
                }
            }
        }
    }
}
