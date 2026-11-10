//! tests/geo/test_drawing_dataset.cpp: drawing data to and from GDAL's
//! feature tables (katana_interop::geo::drawing_dataset, docs/geoprocessing.md
//! "Bindings"): the one conversion, and a result as one undo step.

mod drawing_dataset {
    use katana_cad::Document;
    use katana_core::ErrorCode;
    use katana_entity::entity::set_heights;
    use katana_entity::{Entity, EntityId, Geometry, PointGeometry, PropertyValue, TextGeometry};
    use katana_geometry::{Circle2, Point2, Polyline2, Segment2};
    use katana_gis::processing::{
        Feature, FeatureSet, FeatureTable, FieldDef, FieldType, FieldValue,
    };
    use katana_gis::{GeoPoint, GeometryKind, VectorGeometry};
    use katana_interop::geo::drawing_dataset::{
        DrawingDatasetOptions, ResultMode, ResultOptions, ResultPlan, drawing_dataset,
        result_command,
    };

    use crate::{double_eq, draw, must, point};

    fn point_entity(x: f64, y: f64) -> Entity {
        Entity {
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(x, y),
            }),
            ..Entity::default()
        }
    }

    fn polyline(vertices: &[(f64, f64)], closed: bool) -> Entity {
        Entity {
            geometry: Geometry::Polyline(Polyline2::new(
                vertices.iter().map(|&(x, y)| Point2::new(x, y)).collect(),
                closed,
            )),
            ..Entity::default()
        }
    }

    fn square(x: f64, y: f64, side: f64) -> Entity {
        polyline(
            &[(x, y), (x + side, y), (x + side, y + side), (x, y + side)],
            true,
        )
    }

    fn table_named<'a>(set: &'a FeatureSet, name: &str) -> Option<&'a FeatureTable> {
        set.tables.iter().find(|table| table.name == name)
    }

    fn field_index(table: &FeatureTable, name: &str) -> usize {
        table
            .fields
            .iter()
            .position(|field| field.name == name)
            .unwrap_or(table.fields.len())
    }

    fn ring_area(ring: &[GeoPoint]) -> f64 {
        let mut twice = 0.0;
        for (i, a) in ring.iter().enumerate() {
            let b = &ring[(i + 1) % ring.len()];
            twice += a.x * b.y - b.x * a.y;
        }
        f64::abs(twice) / 2.0
    }

    fn text(value: &str) -> PropertyValue {
        PropertyValue::Text(String::from(value))
    }

    fn defaults() -> DrawingDatasetOptions {
        DrawingDatasetOptions::default()
    }

    /// The plan's command, executed on `document` as one step.
    #[track_caller]
    fn execute(document: &mut Document, plan: &mut ResultPlan) {
        let command = plan.command.take().expect("the plan has a command");
        let status = document.execute(command);
        assert!(status.is_ok(), "{status:?}");
    }

    #[test]
    fn katana_id_and_typed_properties_become_typed_fields() {
        let mut document = Document::default();
        let mut tree = point_entity(1.0, 2.0);
        tree.properties.insert(String::from("code"), text("TREE"));
        tree.properties
            .insert(String::from("girth"), PropertyValue::Real(1.5));
        tree.properties
            .insert(String::from("count"), PropertyValue::Integer(3));
        tree.properties
            .insert(String::from("alive"), PropertyValue::Boolean(true));
        let mut pit = point_entity(3.0, 4.0);
        pit.properties.insert(String::from("code"), text("PIT"));
        pit.properties
            .insert(String::from("count"), PropertyValue::Integer(1));
        let ids = draw(&mut document, vec![tree, pit]);

        let dataset = must(drawing_dataset(document.model(), &ids, &defaults()));
        assert_eq!(dataset.set.tables.len(), 1);
        let points = &dataset.set.tables[0];
        assert_eq!(points.name, "points");
        assert_eq!(points.kind, GeometryKind::Point);
        // katana_id first, the bookkeeping, then the properties sorted by key.
        let names: Vec<&str> = points
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "katana_id",
                "layer",
                "style",
                "colour",
                "type",
                "alive",
                "code",
                "count",
                "girth"
            ]
        );
        assert_eq!(points.fields[0].r#type, FieldType::Integer64);
        assert_eq!(
            points.fields[field_index(points, "alive")].r#type,
            FieldType::Boolean
        );
        assert_eq!(
            points.fields[field_index(points, "code")].r#type,
            FieldType::String
        );
        assert_eq!(
            points.fields[field_index(points, "count")].r#type,
            FieldType::Integer64
        );
        assert_eq!(
            points.fields[field_index(points, "girth")].r#type,
            FieldType::Real
        );
        assert_eq!(points.features.len(), 2);
        assert_eq!(
            points.features[0].values[0],
            FieldValue::Integer64(ids[0] as i64)
        );
        assert_eq!(
            points.features[0].values[field_index(points, "girth")],
            FieldValue::Real(1.5)
        );
        // Absent is not zero: the pit has no girth.
        assert_eq!(
            points.features[1].values[field_index(points, "girth")],
            FieldValue::Null
        );
        assert_eq!(dataset.stats.matched, 2);
        assert_eq!(dataset.stats.used, 2);
        assert_eq!(dataset.stats.points, 2);
    }

    #[test]
    fn a_key_of_several_types_becomes_text_and_says_so() {
        let mut document = Document::default();
        let mut a = point_entity(0.0, 0.0);
        a.properties
            .insert(String::from("tag"), PropertyValue::Integer(7));
        let mut b = point_entity(1.0, 1.0);
        b.properties.insert(String::from("tag"), text("seven"));
        let ids = draw(&mut document, vec![a, b]);
        let dataset = must(drawing_dataset(document.model(), &ids, &defaults()));
        let points = &dataset.set.tables[0];
        assert_eq!(
            points.fields[field_index(points, "tag")].r#type,
            FieldType::String
        );
        assert_eq!(
            points.features[0].values[field_index(points, "tag")],
            FieldValue::Text(String::from("7"))
        );
        assert_eq!(dataset.stats.warnings.len(), 1);
        assert!(dataset.stats.warnings[0].contains("tag"));
    }

    #[test]
    fn a_vertex_without_a_height_keeps_the_line_two_dimensional() {
        let mut document = Document::default();
        let mut partial = polyline(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)], false);
        set_heights(&mut partial.properties, &[Some(1.0), None, Some(3.0)]);
        let mut whole = polyline(&[(0.0, 5.0), (10.0, 5.0), (20.0, 5.0)], false);
        set_heights(&mut whole.properties, &[Some(1.0), Some(2.0), Some(3.0)]);
        let ids = draw(&mut document, vec![partial, whole]);
        let dataset = must(drawing_dataset(document.model(), &ids, &defaults()));
        let lines = table_named(&dataset.set, "lines").expect("a lines table");
        assert_eq!(lines.features.len(), 2);
        assert!(!lines.features[0].parts[0].has_z);
        assert!(lines.features[1].parts[0].has_z);
        assert_eq!(lines.features[1].parts[0].parts[0][1].z, 2.0);
    }

    #[test]
    fn heightless_entities_are_counted_when_heights_are_required() {
        let mut document = Document::default();
        let mut surveyed = point_entity(0.0, 0.0);
        set_heights(&mut surveyed.properties, &[Some(12.5)]);
        let ids = draw(&mut document, vec![surveyed, point_entity(5.0, 5.0)]);
        let options = DrawingDatasetOptions {
            require_heights: true,
            ..DrawingDatasetOptions::default()
        };
        let dataset = must(drawing_dataset(document.model(), &ids, &options));
        assert_eq!(dataset.stats.used, 1);
        assert_eq!(dataset.stats.skipped.get("heightless"), Some(&1));
        assert_eq!(dataset.set.tables.len(), 1);
        assert_eq!(
            dataset.set.tables[0].features[0].parts[0].parts[0][0].z,
            12.5
        );
    }

    #[test]
    fn text_is_skipped_and_counted() {
        let mut document = Document::default();
        let label = Entity {
            geometry: Geometry::Text(TextGeometry {
                text: String::from("LOT 7"),
                ..TextGeometry::default()
            }),
            ..Entity::default()
        };
        let ids = draw(&mut document, vec![label, point_entity(0.0, 0.0)]);
        let dataset = must(drawing_dataset(document.model(), &ids, &defaults()));
        assert_eq!(dataset.stats.matched, 2);
        assert_eq!(dataset.stats.used, 1);
        assert_eq!(dataset.stats.skipped.get("text"), Some(&1));
    }

    #[test]
    fn an_id_the_drawing_does_not_hold_is_counted_missing() {
        let document = Document::default();
        let dataset = must(drawing_dataset(document.model(), &[42], &defaults()));
        assert!(dataset.set.tables.is_empty());
        assert_eq!(dataset.stats.skipped.get("missing"), Some(&1));
    }

    #[test]
    fn a_tagged_hole_joins_its_exterior() {
        // A 100 m square lot with a 20 m square hole tagged as IMPORT tags
        // one: 10000 - 400 = 9600 m2.
        let mut document = Document::default();
        let mut hole = square(40.0, 40.0, 20.0);
        hole.metadata
            .insert(String::from("source.ring"), text("hole"));
        let ids = draw(&mut document, vec![square(0.0, 0.0, 100.0), hole]);
        let dataset = must(drawing_dataset(document.model(), &ids, &defaults()));
        let polygons = table_named(&dataset.set, "polygons").expect("a polygons table");
        assert_eq!(polygons.features.len(), 1);
        let rings = &polygons.features[0].parts[0].parts;
        assert_eq!(rings.len(), 2);
        double_eq(ring_area(&rings[0]) - ring_area(&rings[1]), 9600.0);
        assert_eq!(dataset.stats.used, 2);
        assert_eq!(dataset.stats.polygons, 1);
    }

    #[test]
    fn a_building_inside_a_lot_is_not_a_hole() {
        let mut document = Document::default();
        let ids = draw(
            &mut document,
            vec![square(0.0, 0.0, 100.0), square(40.0, 40.0, 20.0)],
        );
        let dataset = must(drawing_dataset(document.model(), &ids, &defaults()));
        let polygons = table_named(&dataset.set, "polygons").expect("a polygons table");
        assert_eq!(polygons.features.len(), 2);
        for feature in &polygons.features {
            assert_eq!(feature.parts[0].parts.len(), 1);
        }
    }

    #[test]
    fn an_arc_is_chords_within_the_tolerance() {
        // A circle of radius 10 at a 1 mm sagitta: every chord's middle is
        // within 1 mm of the circle, so no chord subtends more than
        // 2 acos(1 - 0.001 / 10).
        let mut document = Document::default();
        let circle = Entity {
            geometry: Geometry::Circle(Circle2::new(Point2::new(0.0, 0.0), 10.0)),
            ..Entity::default()
        };
        let ids = draw(&mut document, vec![circle]);
        let dataset = must(drawing_dataset(document.model(), &ids, &defaults()));
        let ring = &dataset.set.tables[0].features[0].parts[0].parts[0];
        let largest = 2.0 * (1.0 - 0.001 / 10.0_f64).acos();
        assert!(ring.len() as f64 >= (2.0 * std::f64::consts::PI / largest).ceil());
        for (i, a) in ring.iter().enumerate() {
            let b = &ring[(i + 1) % ring.len()];
            let middle = ((a.x + b.x) / 2.0).hypot((a.y + b.y) / 2.0);
            assert!(10.0 - middle <= 0.001 + 1e-12, "{middle}");
        }
    }

    #[test]
    fn drawing_to_dataset_to_entities_is_the_identity() {
        let mut document = Document::default();
        let mut marked = point_entity(1.0, 2.0);
        marked.properties.insert(String::from("code"), text("PEG"));
        let mut kerb = polyline(&[(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)], false);
        kerb.properties
            .insert(String::from("length.design"), PropertyValue::Real(15.25));
        let mut lot = square(0.0, 0.0, 30.0);
        lot.properties
            .insert(String::from("lot"), PropertyValue::Integer(7));
        let segment = Entity {
            geometry: Geometry::Line(Segment2::new(Point2::new(0.0, 0.0), Point2::new(3.0, 4.0))),
            ..Entity::default()
        };
        let ids = draw(&mut document, vec![marked, kerb, lot, segment]);

        let dataset = must(drawing_dataset(document.model(), &ids, &defaults()));
        let options = ResultOptions {
            target_layer: String::from("copy"),
            operation: String::from("identity"),
            ..ResultOptions::default()
        };
        let mut plan = must(result_command(document.model(), &dataset.set, &options));
        assert_eq!(plan.created, 4);
        // Three tables, so a layer each under the target.
        assert_eq!(plan.layers, ["copy/points", "copy/lines", "copy/polygons"]);
        execute(&mut document, &mut plan);
        let copies = document.last_created_entities();
        assert_eq!(copies.len(), 4);
        for copy_id in copies {
            let copy = document
                .model()
                .entities
                .find(copy_id)
                .expect("the copy is in the drawing");
            let Some(PropertyValue::Integer(source)) = copy.properties.get("gis.source") else {
                panic!("the copy names its source");
            };
            let original = document
                .model()
                .entities
                .find(*source as EntityId)
                .expect("the source is in the drawing");
            assert_eq!(copy.geometry, original.geometry);
            assert_eq!(copy.properties.get("gis.op"), Some(&text("identity")));
            let mut properties = copy.properties.clone();
            properties.remove("gis.source");
            properties.remove("gis.op");
            assert_eq!(properties, original.properties);
        }
    }

    #[test]
    fn all_results_are_one_undo_step() {
        let mut document = Document::default();
        let ids = draw(
            &mut document,
            vec![point_entity(0.0, 0.0), square(0.0, 0.0, 5.0)],
        );
        let dataset = must(drawing_dataset(document.model(), &ids, &defaults()));
        let options = ResultOptions {
            target_layer: String::from("gis/result"),
            ..ResultOptions::default()
        };
        let mut plan = must(result_command(document.model(), &dataset.set, &options));
        let steps = document.history().undo_count();
        execute(&mut document, &mut plan);
        assert_eq!(document.history().undo_count(), steps + 1);
        assert_eq!(document.model().entities.size(), 4);
        assert!(document.model().layers.contains("gis/result/points"));
        assert!(document.undo().is_ok());
        assert_eq!(document.model().entities.size(), 2);
        assert!(!document.model().layers.contains("gis/result/points"));
    }

    #[test]
    fn a_hole_written_back_is_tagged_and_joins_its_area_again() {
        let lot = VectorGeometry {
            kind: GeometryKind::Polygon,
            parts: vec![
                vec![
                    point(0.0, 0.0, 0.0),
                    point(100.0, 0.0, 0.0),
                    point(100.0, 100.0, 0.0),
                    point(0.0, 100.0, 0.0),
                    point(0.0, 0.0, 0.0),
                ],
                vec![
                    point(40.0, 40.0, 0.0),
                    point(60.0, 40.0, 0.0),
                    point(60.0, 60.0, 0.0),
                    point(40.0, 60.0, 0.0),
                    point(40.0, 40.0, 0.0),
                ],
            ],
            ..VectorGeometry::default()
        };
        let polygons = FeatureTable {
            name: String::from("polygons"),
            kind: GeometryKind::Polygon,
            features: vec![Feature {
                parts: vec![lot],
                values: Vec::new(),
            }],
            ..FeatureTable::default()
        };
        let mut document = Document::default();
        let options = ResultOptions {
            target_layer: String::from("gis/lot"),
            ..ResultOptions::default()
        };
        let mut plan = must(result_command(
            document.model(),
            &FeatureSet {
                tables: vec![polygons],
            },
            &options,
        ));
        execute(&mut document, &mut plan);
        let rings = document.last_created_entities();
        assert_eq!(rings.len(), 2);
        let exterior = document
            .model()
            .entities
            .find(rings[0])
            .expect("the exterior");
        let hole = document.model().entities.find(rings[1]).expect("the hole");
        assert_eq!(exterior.properties.get("gis.ring"), Some(&text("exterior")));
        assert_eq!(hole.properties.get("gis.ring"), Some(&text("hole")));
        assert_eq!(
            exterior.properties.get("gis.part"),
            hole.properties.get("gis.part")
        );
        // The part names the exterior by its id.
        assert_eq!(
            exterior.properties.get("gis.part"),
            Some(&PropertyValue::Integer(rings[0] as i64))
        );
        let again = must(drawing_dataset(document.model(), &rings, &defaults()));
        assert_eq!(again.set.tables[0].features.len(), 1);
        let parts = &again.set.tables[0].features[0].parts[0].parts;
        assert_eq!(parts.len(), 2);
        double_eq(ring_area(&parts[0]) - ring_area(&parts[1]), 9600.0);
    }

    #[test]
    fn a_date_a_result_made_is_a_date_when_read_again() {
        // A result's Date field becomes a text property tagged
        // gis.type.<key>, so the drawing read as features gives the field its
        // type again.
        let at = VectorGeometry {
            kind: GeometryKind::Point,
            parts: vec![vec![point(1.0, 2.0, 0.0)]],
            ..VectorGeometry::default()
        };
        let points = FeatureTable {
            name: String::from("points"),
            kind: GeometryKind::Point,
            fields: vec![
                FieldDef {
                    name: String::from("surveyed"),
                    r#type: FieldType::Date,
                },
                FieldDef {
                    name: String::from("note"),
                    r#type: FieldType::String,
                },
            ],
            features: vec![Feature {
                parts: vec![at],
                values: vec![
                    FieldValue::Text(String::from("2024-05-01")),
                    FieldValue::Text(String::from("x")),
                ],
            }],
            ..FeatureTable::default()
        };
        let mut document = Document::default();
        let options = ResultOptions {
            target_layer: String::from("gis/dated"),
            ..ResultOptions::default()
        };
        let mut plan = must(result_command(
            document.model(),
            &FeatureSet {
                tables: vec![points],
            },
            &options,
        ));
        execute(&mut document, &mut plan);
        let made = document.last_created_entities();
        assert_eq!(made.len(), 1);
        let again = must(drawing_dataset(document.model(), &made, &defaults()));
        let table = &again.set.tables[0];
        let type_of = |name: &str| {
            table
                .fields
                .iter()
                .find(|field| field.name == name)
                .map(|field| field.r#type)
                .unwrap_or_else(|| panic!("no field {name}"))
        };
        assert_eq!(type_of("surveyed"), FieldType::Date);
        assert_eq!(type_of("note"), FieldType::String);
    }

    #[test]
    fn update_geometry_keeps_the_entitys_id() {
        let mut document = Document::default();
        let ids = draw(
            &mut document,
            vec![polyline(&[(0.0, 0.0), (10.0, 0.0), (20.0, 5.0)], false)],
        );
        let mut dataset = must(drawing_dataset(document.model(), &ids, &defaults()));
        for vertex in &mut dataset.set.tables[0].features[0].parts[0].parts[0] {
            vertex.x += 5.0;
        }
        let options = ResultOptions {
            mode: ResultMode::UpdateGeometry,
            ..ResultOptions::default()
        };
        let mut plan = must(result_command(document.model(), &dataset.set, &options));
        assert_eq!(plan.updated, 1);
        execute(&mut document, &mut plan);
        let moved = document
            .model()
            .entities
            .find(ids[0])
            .expect("the entity kept its id");
        let Geometry::Polyline(line) = &moved.geometry else {
            panic!("the entity is a polyline still");
        };
        assert_eq!(line.vertices[0], Point2::new(5.0, 0.0));
        assert_eq!(document.model().entities.size(), 1);
    }

    #[test]
    fn set_properties_writes_the_fields_on_the_entities_they_came_from() {
        let mut document = Document::default();
        let ids = draw(&mut document, vec![square(0.0, 0.0, 10.0)]);
        let zones = FeatureTable {
            name: String::from("zones"),
            kind: GeometryKind::Polygon,
            fields: vec![
                FieldDef {
                    name: String::from("katana_id"),
                    r#type: FieldType::Integer64,
                },
                FieldDef {
                    name: String::from("mean"),
                    r#type: FieldType::Real,
                },
            ],
            features: vec![Feature {
                parts: Vec::new(),
                values: vec![FieldValue::Integer64(ids[0] as i64), FieldValue::Real(3.5)],
            }],
            ..FeatureTable::default()
        };
        let options = ResultOptions {
            mode: ResultMode::SetProperties,
            property_prefix: String::from("zone."),
            ..ResultOptions::default()
        };
        let mut plan = must(result_command(
            document.model(),
            &FeatureSet {
                tables: vec![zones],
            },
            &options,
        ));
        execute(&mut document, &mut plan);
        let zoned = document.model().entities.find(ids[0]).expect("the zone");
        assert_eq!(
            zoned.properties.get("zone.mean"),
            Some(&PropertyValue::Real(3.5))
        );
    }

    #[test]
    fn a_result_without_katana_id_cannot_change_entities_in_place() {
        let document = Document::default();
        let loose = FeatureTable {
            name: String::from("loose"),
            kind: GeometryKind::Point,
            features: vec![Feature::default()],
            ..FeatureTable::default()
        };
        let options = ResultOptions {
            mode: ResultMode::SetProperties,
            ..ResultOptions::default()
        };
        let Err(error) = result_command(
            document.model(),
            &FeatureSet {
                tables: vec![loose],
            },
            &options,
        ) else {
            panic!("a table without katana_id should have been refused");
        };
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_result_of_nothing_changes_nothing() {
        let document = Document::default();
        let options = ResultOptions {
            target_layer: String::from("gis/nothing"),
            ..ResultOptions::default()
        };
        let plan = must(result_command(
            document.model(),
            &FeatureSet::default(),
            &options,
        ));
        assert!(plan.command.is_none());
        assert_eq!(plan.created, 0);
    }

    #[test]
    fn a_target_that_is_no_layer_path_is_refused() {
        let document = Document::default();
        let options = ResultOptions {
            target_layer: String::from("gis//bad"),
            ..ResultOptions::default()
        };
        let Err(error) = result_command(document.model(), &FeatureSet::default(), &options) else {
            panic!("gis//bad should have been refused");
        };
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}
