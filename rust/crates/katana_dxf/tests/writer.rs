//! The twin of tests/dxf/test_writer.cpp: what the writer puts in the file,
//! and that a drawing written and read back is the drawing it was.

pub mod support;

use std::collections::BTreeMap;

use katana_dxf::{
    DxfImport, ExportOptions, ImportOptions, layer_name_for, read_dxf, read_dxf_file, write_dxf,
    write_dxf_file,
};
use katana_entity::entity::{heights_of, set_heights};
use katana_entity::{
    AnchorPoint, AnchorRef, Color, DimensionGeometry, Entity, EntityId, EntityType, Geometry,
    LabelGeometry, LabelKind, LabelStyle, Layer, LeaderGeometry, Linetype, LinetypeElement, Model,
    PointGeometry, PropertyValue, TextGeometry,
};
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, Ellipse2, Point2, Polyline2, Segment2, Spline2, Vec2,
};
use katana_math::numerics::PI;
use support::{Scratch, double_eq, near};

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b, 255)
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn add(
    model: &mut Model,
    geometry: impl Into<Geometry>,
    layer: &str,
    heights: &[Option<f64>],
) -> EntityId {
    let mut entity = Entity {
        geometry: geometry.into(),
        layer: layer.to_owned(),
        ..Entity::default()
    };
    if !heights.is_empty() {
        set_heights(&mut entity.properties, heights);
    }
    match model.entities.add(entity) {
        Ok(id) => id,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn written(model: &Model, options: &ExportOptions<'_>) -> String {
    match write_dxf(model, options) {
        Ok(out) => out.text,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn read(text: &str) -> DxfImport {
    match read_dxf(text, &ImportOptions::default()) {
        Ok(imported) => imported,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn round_trip(model: &Model) -> DxfImport {
    let text = written(model, &ExportOptions::default());
    read(text.as_str())
}

/// How many times `record` (a "0" pair's value) starts a record.
fn records(text: &str, record: &str) -> usize {
    let needle = format!("  0\n{record}\n");
    let mut count = 0;
    let mut at = text.find(needle.as_str());
    while let Some(found) = at {
        count += 1;
        at = text[found + 1..]
            .find(needle.as_str())
            .map(|next| found + 1 + next);
    }
    count
}

fn of_kind(imported: &DxfImport, kind: EntityType) -> Vec<&Entity> {
    imported
        .entities
        .iter()
        .filter(|entity| entity.r#type() == kind)
        .collect()
}

fn layer_named<'a>(imported: &'a DxfImport, name: &str) -> Option<&'a Layer> {
    imported.layers.iter().find(|layer| layer.name == name)
}

fn polyline_of(entity: &Entity) -> &Polyline2 {
    match &entity.geometry {
        Geometry::Polyline(polyline) => polyline,
        other => panic!("not a polyline: {other:?}"),
    }
}

fn arc_of(entity: &Entity) -> &Arc2 {
    match &entity.geometry {
        Geometry::Arc(arc) => arc,
        other => panic!("not an arc: {other:?}"),
    }
}

fn text_of(entity: &Entity) -> &TextGeometry {
    match &entity.geometry {
        Geometry::Text(text) => text,
        other => panic!("not a text: {other:?}"),
    }
}

fn curve_polyline_of(entity: &Entity) -> &CurvePolyline2 {
    match &entity.geometry {
        Geometry::CurvePolyline(polyline) => polyline,
        other => panic!("not a curve polyline: {other:?}"),
    }
}

fn ellipse_of(entity: &Entity) -> &Ellipse2 {
    match &entity.geometry {
        Geometry::Ellipse(ellipse) => ellipse,
        other => panic!("not an ellipse: {other:?}"),
    }
}

fn spline_of(entity: &Entity) -> &Spline2 {
    match &entity.geometry {
        Geometry::Spline(spline) => spline,
        other => panic!("not a spline: {other:?}"),
    }
}

fn point_of(entity: &Entity) -> Point2 {
    match &entity.geometry {
        Geometry::Point(point) => point.position,
        other => panic!("not a point: {other:?}"),
    }
}

/// A model with a point and a label of it in a point-number style.
fn labelled_point() -> (Model, EntityId) {
    let mut model = Model::default();
    let style = LabelStyle {
        name: String::from("pt"),
        kind: LabelKind::Point,
        text: String::from("{point}"),
        ..LabelStyle::default()
    };
    assert!(model.label_styles.add(style).is_ok());
    let point = add(
        &mut model,
        PointGeometry {
            position: p(1.0, 1.0),
        },
        "0",
        &[],
    );
    let label = add(
        &mut model,
        LabelGeometry {
            target: point,
            style: String::from("pt"),
            anchor: p(1.0, 1.0),
            ..LabelGeometry::default()
        },
        "0",
        &[],
    );
    (model, label)
}

mod dxf_writer {
    use super::*;

    #[test]
    fn a_closed_polyline_is_written_as_a_closed_lw_polyline_never_a_hatch() {
        let mut model = Model::default();
        add(
            &mut model,
            Polyline2::new(
                vec![p(0.0, 0.0), p(30.0, 0.0), p(30.0, 20.0), p(0.0, 20.0)],
                true,
            ),
            "0",
            &[],
        );
        let text = written(&model, &ExportOptions::default());
        assert_eq!(records(&text, "LWPOLYLINE"), 1);
        assert_eq!(records(&text, "HATCH"), 0);
        // Flag 70 = 1: closed. The four vertices and nothing else.
        assert!(text.contains("AcDbPolyline\n 90\n4\n 70\n1\n"));
        let back = read(text.as_str());
        assert_eq!(back.entities.len(), 1);
        let polyline = polyline_of(&back.entities[0]);
        assert!(polyline.closed);
        assert_eq!(polyline.vertices.len(), 4);
        assert_eq!(polyline.area(), 600.0);
    }

    #[test]
    fn circles_and_arcs_are_written_as_true_curves() {
        let mut model = Model::default();
        add(&mut model, Circle2::new(p(15.0, 10.0), 5.0), "0", &[]);
        add(
            &mut model,
            Arc2::new(p(100.0, 200.0), 1000.0, 0.0, PI / 6.0),
            "0",
            &[],
        );
        let text = written(&model, &ExportOptions::default());
        assert_eq!(records(&text, "CIRCLE"), 1);
        assert_eq!(records(&text, "ARC"), 1);
        assert_eq!(records(&text, "LWPOLYLINE"), 0);
        // Read back: the same curves, not chords.
        let back = read(text.as_str());
        let circles = of_kind(&back, EntityType::Circle);
        let arcs = of_kind(&back, EntityType::Arc);
        assert_eq!(circles.len(), 1);
        assert_eq!(arcs.len(), 1);
        assert_eq!(
            circles[0].geometry,
            Geometry::Circle(Circle2::new(p(15.0, 10.0), 5.0))
        );
        let arc = arc_of(arcs[0]);
        assert_eq!(arc.start_angle, 0.0);
        // 30 degrees, through degrees and back: within a unit in the last place.
        near(arc.sweep, PI / 6.0, 1e-15);
    }

    #[test]
    fn a_clockwise_arc_is_written_counter_clockwise_from_its_other_end() {
        let mut model = Model::default();
        // From 90 degrees clockwise by 90: the curve from 0 to 90 counter-clockwise.
        add(
            &mut model,
            Arc2::new(p(0.0, 0.0), 2.0, PI / 2.0, -PI / 2.0),
            "0",
            &[],
        );
        let back = round_trip(&model);
        let arcs = of_kind(&back, EntityType::Arc);
        assert_eq!(arcs.len(), 1);
        let arc = arc_of(arcs[0]);
        near(arc.start_angle, 0.0, 1e-15);
        near(arc.sweep, PI / 2.0, 1e-15);
        assert_eq!(arc.radius, 2.0);
    }

    #[test]
    fn every_kind_comes_back_as_the_geometry_it_was() {
        let mut model = Model::default();
        assert!(
            model
                .layers
                .add(Layer {
                    name: String::from("Survey"),
                    color: rgb(255, 0, 0),
                    ..Layer::default()
                })
                .is_ok()
        );
        add(
            &mut model,
            PointGeometry {
                position: p(502000.125, 6250000.1),
            },
            "Survey",
            &[Some(31.245)],
        );
        add(
            &mut model,
            Segment2::new(p(0.1, 0.2), p(3.0, 4.0)),
            "0",
            &[],
        );
        add(
            &mut model,
            Arc2::new(p(650.0, 200.0), 8.0, 0.25, 1.5),
            "0",
            &[],
        );
        add(&mut model, Circle2::new(p(-7.5, 1.0 / 3.0), 0.6), "0", &[]);
        add(
            &mut model,
            Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0)], false),
            "0",
            &[],
        );
        add(
            &mut model,
            TextGeometry::new(p(5.0, 6.0), "RL 32.45° Böschung", 2.75, PI / 2.0),
            "0",
            &[],
        );
        let back = round_trip(&model);
        assert_eq!(back.entities.len(), 6);

        let point = &back.entities[0];
        assert_eq!(point_of(point), p(502000.125, 6250000.1));
        assert_eq!(heights_of(&point.properties, 1)[0], Some(31.245));
        assert_eq!(point.layer, "Survey");
        // Coordinates are written with the shortest digits that read back the
        // same double: nothing is lost to formatting.
        assert_eq!(
            back.entities[1].geometry,
            Geometry::Line(Segment2::new(p(0.1, 0.2), p(3.0, 4.0)))
        );
        let arc = arc_of(&back.entities[2]);
        assert_eq!(arc.center, p(650.0, 200.0));
        assert_eq!(arc.radius, 8.0);
        // Through degrees and back: equal to a few units in the last place.
        near(arc.start_angle, 0.25, 1e-14);
        near(arc.sweep, 1.5, 1e-14);
        assert_eq!(
            back.entities[3].geometry,
            Geometry::Circle(Circle2::new(p(-7.5, 1.0 / 3.0), 0.6))
        );
        let polyline = polyline_of(&back.entities[4]);
        assert!(!polyline.closed);
        assert_eq!(polyline.vertices.len(), 3);
        let text = text_of(&back.entities[5]);
        assert_eq!(text.text, "RL 32.45° Böschung");
        assert_eq!(text.position, p(5.0, 6.0));
        assert_eq!(text.height, 2.75);
        near(text.rotation, PI / 2.0, 1e-15);
    }

    #[test]
    fn a_polyline_whose_heights_differ_is_a_three_dimensional_polyline() {
        let mut model = Model::default();
        add(
            &mut model,
            Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)], true),
            "0",
            &[Some(10.0), Some(11.0), Some(12.5)],
        );
        add(
            &mut model,
            Polyline2::new(vec![p(0.0, 0.0), p(5.0, 5.0)], false),
            "0",
            &[Some(3.5), Some(3.5)],
        );
        let text = written(&model, &ExportOptions::default());
        assert_eq!(records(&text, "POLYLINE"), 1);
        assert_eq!(records(&text, "VERTEX"), 3);
        assert_eq!(records(&text, "SEQEND"), 1);
        assert_eq!(records(&text, "LWPOLYLINE"), 1); // one height is an elevation
        let back = read(text.as_str());
        assert_eq!(back.entities.len(), 2);
        let three_d = polyline_of(&back.entities[0]);
        assert!(three_d.closed);
        let heights = heights_of(&back.entities[0].properties, 3);
        assert_eq!(heights[0], Some(10.0));
        assert_eq!(heights[1], Some(11.0));
        assert_eq!(heights[2], Some(12.5));
        assert_eq!(heights_of(&back.entities[1].properties, 2)[1], Some(3.5));
    }

    #[test]
    fn layers_are_written_with_their_colours_visibility_lock_and_weight() {
        let mut model = Model::default();
        let kerb = Layer {
            name: String::from("Kerb"),
            color: rgb(0, 0, 255),
            visible: false,
            locked: true,
            line_weight: 0.5,
            ..Layer::default()
        };
        assert!(model.layers.add(kerb).is_ok());
        add(
            &mut model,
            Segment2::new(p(0.0, 0.0), p(1.0, 0.0)),
            "Kerb",
            &[],
        );
        let back = round_trip(&model);
        let found = layer_named(&back, "Kerb").expect("Kerb");
        assert_eq!(found.color, rgb(0, 0, 255));
        assert!(!found.visible);
        assert!(found.locked);
        assert_eq!(found.line_weight, 0.5);
        assert_eq!(back.entities[0].layer, "Kerb");
    }

    #[test]
    fn a_nested_layer_path_is_a_legal_name_and_comes_back_through_the_extended_data() {
        assert_eq!(layer_name_for("survey/kerb/top"), "survey$kerb$top");
        assert_eq!(layer_name_for("a<b>c:d"), "a_b_c_d");
        assert_eq!(layer_name_for(""), "_");
        let mut model = Model::default();
        assert!(
            model
                .layers
                .add(Layer {
                    name: String::from("survey/kerb"),
                    ..Layer::default()
                })
                .is_ok()
        );
        add(
            &mut model,
            Segment2::new(p(0.0, 0.0), p(1.0, 0.0)),
            "survey/kerb",
            &[],
        );
        let text = written(&model, &ExportOptions::default());
        assert!(text.contains("  2\nsurvey$kerb\n"));
        assert!(!text.contains("  2\nsurvey/kerb\n"));
        let back = read(text.as_str());
        assert_eq!(back.entities[0].layer, "survey/kerb");
    }

    #[test]
    fn linetypes_are_written_and_read_back() {
        let mut model = Model::default();
        let dashed = Linetype {
            name: String::from("kerb dash"),
            description: String::from("Kerb __ . __"),
            pattern: vec![
                LinetypeElement { length: 2.0 },
                LinetypeElement { length: -0.5 },
                LinetypeElement { length: 0.0 },
                LinetypeElement { length: -0.5 },
            ],
        };
        assert!(model.linetypes.add(dashed.clone()).is_ok());
        let kerb = Layer {
            name: String::from("Kerb"),
            linetype: String::from("kerb dash"),
            ..Layer::default()
        };
        assert!(model.layers.add(kerb).is_ok());
        add(
            &mut model,
            Segment2::new(p(0.0, 0.0), p(1.0, 0.0)),
            "Kerb",
            &[],
        );
        let back = round_trip(&model);
        assert_eq!(back.linetypes.len(), 1);
        assert_eq!(back.linetypes[0].name, "kerb dash");
        assert_eq!(back.linetypes[0].pattern, dashed.pattern);
        let found = layer_named(&back, "Kerb").expect("Kerb");
        assert_eq!(found.linetype, "kerb dash");
    }

    #[test]
    fn a_colour_no_index_is_goes_out_as_the_nearest_and_comes_back_exact() {
        // (250,5,5) is no indexed colour; the nearest is 1, red, which is what
        // every other program draws. The colour itself rides along as extended
        // data, so it comes back to Katana as it went out - for an entity and for
        // a layer alike.
        let mut model = Model::default();
        assert!(
            model
                .layers
                .add(Layer {
                    name: String::from("Kerb"),
                    color: rgb(10, 200, 30),
                    ..Layer::default()
                })
                .is_ok()
        );
        let entity = Entity {
            geometry: Geometry::Line(Segment2::new(p(0.0, 0.0), p(1.0, 0.0))),
            layer: String::from("Kerb"),
            color: Some(rgb(250, 5, 5)),
            ..Entity::default()
        };
        assert!(model.entities.add(entity).is_ok());
        let text = written(&model, &ExportOptions::default());
        assert!(text.contains(" 62\n1\n"));
        assert!(text.contains("1000\ncolour\n1000\n#FA0505\n"));
        let back = read(text.as_str());
        assert_eq!(back.entities[0].color, Some(rgb(250, 5, 5)));
        let kerb = layer_named(&back, "Kerb").expect("Kerb");
        assert_eq!(kerb.color, rgb(10, 200, 30));
    }

    #[test]
    fn an_indexed_colour_needs_no_extended_data() {
        let mut model = Model::default();
        let entity = Entity {
            geometry: Geometry::Line(Segment2::new(p(0.0, 0.0), p(1.0, 0.0))),
            color: Some(rgb(255, 0, 0)), // index 1 exactly
            ..Entity::default()
        };
        assert!(model.entities.add(entity).is_ok());
        let text = written(&model, &ExportOptions::default());
        assert!(!text.contains("1000\ncolour\n"));
        assert_eq!(read(text.as_str()).entities[0].color, Some(rgb(255, 0, 0)));
    }

    #[test]
    fn text_with_a_line_break_is_one_text_a_line() {
        let mut model = Model::default();
        add(
            &mut model,
            TextGeometry::new(p(0.0, 0.0), "LOT 42\nDP 1234", 3.0, 0.0),
            "0",
            &[],
        );
        let back = round_trip(&model);
        assert_eq!(back.entities.len(), 2);
        // The second line five thirds of a height (5) lower.
        assert_eq!(text_of(&back.entities[1]).text, "DP 1234");
        near(text_of(&back.entities[1]).position.y, -5.0, 1e-12);
    }

    #[test]
    fn a_dimension_is_written_as_the_lines_and_text_it_draws() {
        let mut model = Model::default();
        add(
            &mut model,
            DimensionGeometry::new(p(0.0, 0.0), p(10.0, 0.0), 5.0, ""),
            "0",
            &[],
        );
        let out = write_dxf(&model, &ExportOptions::default()).expect("the export");
        // Two extension lines, the dimension line, two ticks, and the text.
        assert_eq!(out.dxf_entities_written, 6);
        assert_eq!(records(&out.text, "LINE"), 5);
        assert_eq!(records(&out.text, "TEXT"), 1);
        // The standard style's three decimals.
        assert!(out.text.contains("  1\n10.000\n"));
    }

    #[test]
    fn the_origin_shift_is_added_back() {
        let mut model = Model::default();
        add(
            &mut model,
            PointGeometry {
                position: p(1.0, 2.0),
            },
            "0",
            &[],
        );
        let options = ExportOptions {
            origin_shift: Some(Vec2::new(500000.0, 6000000.0)),
            ..ExportOptions::default()
        };
        let out = write_dxf(&model, &options).expect("the export");
        let back = read(out.text.as_str());
        assert_eq!(point_of(&back.entities[0]), p(500001.0, 6000002.0));
    }

    #[test]
    fn only_the_chosen_entities_and_layers_are_written() {
        let mut model = Model::default();
        assert!(
            model
                .layers
                .add(Layer {
                    name: String::from("A"),
                    ..Layer::default()
                })
                .is_ok()
        );
        assert!(
            model
                .layers
                .add(Layer {
                    name: String::from("B"),
                    ..Layer::default()
                })
                .is_ok()
        );
        let first = add(
            &mut model,
            PointGeometry {
                position: p(1.0, 1.0),
            },
            "A",
            &[],
        );
        add(
            &mut model,
            PointGeometry {
                position: p(2.0, 2.0),
            },
            "A",
            &[],
        );
        add(
            &mut model,
            PointGeometry {
                position: p(3.0, 3.0),
            },
            "B",
            &[],
        );
        let by_id = ExportOptions {
            entities: vec![first],
            ..ExportOptions::default()
        };
        assert_eq!(
            write_dxf(&model, &by_id)
                .expect("the export")
                .entities_written,
            1
        );
        let by_layer = ExportOptions {
            layers: vec![String::from("B")],
            ..ExportOptions::default()
        };
        let out = write_dxf(&model, &by_layer).expect("the export");
        assert_eq!(out.entities_written, 1);
        assert_eq!(out.layers_written, 2); // B, and 0 which every file has
    }

    #[test]
    fn the_file_has_the_structure_of_an_r2000_drawing() {
        let mut model = Model::default();
        add(
            &mut model,
            Segment2::new(p(0.0, 0.0), p(1.0, 0.0)),
            "0",
            &[],
        );
        let text = written(&model, &ExportOptions::default());
        assert!(text.starts_with("  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1015\n"));
        assert!(text.ends_with("  0\nEOF\n"));
        for table in [
            "VPORT",
            "LTYPE",
            "LAYER",
            "STYLE",
            "VIEW",
            "UCS",
            "APPID",
            "DIMSTYLE",
            "BLOCK_RECORD",
        ] {
            assert!(
                text.contains(&format!("  0\nTABLE\n  2\n{table}\n")),
                "{table}"
            );
        }
        for section in [
            "HEADER", "CLASSES", "TABLES", "BLOCKS", "ENTITIES", "OBJECTS",
        ] {
            assert!(
                text.contains(&format!("  0\nSECTION\n  2\n{section}\n")),
                "{section}"
            );
        }
        // $HANDSEED is above every handle used: the line is handle 0x30 + 1
        // layer (0) = 0x31, so the seed is 0x32.
        assert!(text.contains("$HANDSEED\n  5\n32\n"));
    }

    #[test]
    fn writing_to_a_file_replaces_it_whole() {
        let mut model = Model::default();
        add(&mut model, Circle2::new(p(0.0, 0.0), 1.0), "0", &[]);
        let scratch = Scratch::new("writing_to_a_file_replaces_it_whole");
        let path = scratch.root.join("katana_dxf_writer_test.dxf");
        std::fs::write(path.as_path(), "x".repeat(100000)).expect("the stale file");
        let out = match write_dxf_file(&model, &path, &ExportOptions::default()) {
            Ok(out) => out,
            Err(error) => panic!("{}", error.describe()),
        };
        let size = std::fs::metadata(path.as_path())
            .expect("the written file")
            .len();
        assert_eq!(size, out.bytes_written as u64);
        let back = read_dxf_file(&path, &ImportOptions::default()).expect("the file read back");
        assert_eq!(back.entities.len(), 1);
    }

    #[test]
    fn a_closed_polyline_that_repeats_its_first_vertex_comes_back_as_it_was() {
        // The real drawing this module was measured on holds one: 19 vertices,
        // closed, the last the first again. Dropped as redundant, it was the only
        // one of 27 886 entities not to come back as it went out.
        let mut model = Model::default();
        let ring = Polyline2::new(
            vec![p(0.0, 0.0), p(4.0, 0.0), p(4.0, 3.0), p(0.0, 0.0)],
            true,
        );
        add(&mut model, ring.clone(), "0", &[]);
        let back = round_trip(&model);
        assert_eq!(back.entities.len(), 1);
        assert_eq!(back.entities[0].geometry, Geometry::Polyline(ring));
    }

    #[test]
    fn heights_known_at_only_some_vertices_go_out_in_plan_and_come_back_with_their_gaps() {
        // No height is not zero: written at Z 0, the gap would be a false level
        // the moment a surface is built from the file. So the polyline and the
        // line go out flat, their heights beside them, and come back as they were.
        let mut model = Model::default();
        add(
            &mut model,
            Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)], false),
            "0",
            &[Some(31.25), None, Some(32.5)],
        );
        add(
            &mut model,
            Segment2::new(p(0.0, 0.0), p(5.0, 0.0)),
            "0",
            &[None, Some(12.0)],
        );
        let out = write_dxf(&model, &ExportOptions::default()).expect("the export");
        assert_eq!(records(&out.text, "POLYLINE"), 0); // no 3D polyline: it would need every Z
        assert_eq!(records(&out.text, "LWPOLYLINE"), 1);
        assert!(out.text.contains("1000\n31.25 null 32.5\n"));
        assert!(!out.warnings.is_empty());
        let back = read(out.text.as_str());
        assert_eq!(back.entities.len(), 2);
        let polyline = heights_of(&back.entities[0].properties, 3);
        assert_eq!(polyline[0], Some(31.25));
        assert!(polyline[1].is_none());
        assert_eq!(polyline[2], Some(32.5));
        let line = heights_of(&back.entities[1].properties, 2);
        assert!(line[0].is_none());
        assert_eq!(line[1], Some(12.0));
    }

    #[test]
    fn an_arc_that_climbs_goes_out_with_both_end_heights_beside_it_and_comes_back_with_them() {
        // An arc on a grade keeps a height at each end; ARC has one Z. So it
        // goes out in plan with both beside it, and is counted in the warning.
        // The clockwise arc goes out from its other end (counter-clockwise from
        // 0 to 90 degrees about (50,0)), so its heights go out, and come back,
        // in that order: 12 at the start, 10 at the end. An arc at one height
        // needs nothing beside it.
        let mut model = Model::default();
        add(
            &mut model,
            Arc2::new(p(0.0, 0.0), 10.0, 0.0, PI / 2.0),
            "0",
            &[Some(31.5), Some(32.25)],
        );
        add(
            &mut model,
            Arc2::new(p(50.0, 0.0), 5.0, PI / 2.0, -PI / 2.0),
            "0",
            &[Some(10.0), Some(12.0)],
        );
        add(
            &mut model,
            Arc2::new(p(100.0, 0.0), 5.0, 0.0, PI / 2.0),
            "0",
            &[Some(5.0), Some(5.0)],
        );
        let out = write_dxf(&model, &ExportOptions::default()).expect("the export");
        assert_eq!(records(&out.text, "ARC"), 3);
        assert!(out.text.contains("1000\n31.5 32.25\n"));
        assert!(out.text.contains("1000\n12 10\n"));
        assert_eq!(out.warnings.len(), 1);
        assert!(
            out.warnings[0].starts_with("2 entities with heights the format cannot hold"),
            "{}",
            out.warnings[0]
        );
        let back = read(out.text.as_str());
        assert_eq!(back.entities.len(), 3);
        let climbing = heights_of(&back.entities[0].properties, 2);
        assert_eq!(climbing[0], Some(31.5));
        assert_eq!(climbing[1], Some(32.25));
        let turned = arc_of(&back.entities[1]);
        near(turned.start_angle, 0.0, 1e-15);
        let reversed = heights_of(&back.entities[1].properties, 2);
        assert_eq!(reversed[0], Some(12.0));
        assert_eq!(reversed[1], Some(10.0));
        let level = heights_of(&back.entities[2].properties, 2);
        assert_eq!(level[0], Some(5.0));
        assert_eq!(level[1], Some(5.0));
    }

    #[test]
    fn a_level_of_exactly_zero_comes_back_as_zero_not_as_no_level() {
        // Z 0 is how every program writes "in plan", so the reader takes it as
        // no level; a surveyed level of 0.000 must be said beside the entity or
        // it comes back unsurveyed. The one height "0" stands for every vertex.
        // Level 5 and no level at all are the controls.
        let mut model = Model::default();
        add(
            &mut model,
            PointGeometry {
                position: p(10.0, 10.0),
            },
            "0",
            &[Some(0.0)],
        );
        add(
            &mut model,
            PointGeometry {
                position: p(20.0, 20.0),
            },
            "0",
            &[Some(5.0)],
        );
        add(
            &mut model,
            PointGeometry {
                position: p(30.0, 30.0),
            },
            "0",
            &[],
        );
        add(
            &mut model,
            Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)], true),
            "0",
            &[Some(0.0), Some(0.0), Some(0.0)],
        );
        add(
            &mut model,
            Segment2::new(p(0.0, 0.0), p(5.0, 0.0)),
            "0",
            &[Some(0.0), Some(0.0)],
        );
        add(
            &mut model,
            Circle2::new(p(40.0, 0.0), 2.0),
            "0",
            &[Some(0.0)],
        );
        add(
            &mut model,
            Arc2::new(p(60.0, 0.0), 2.0, 0.0, PI / 2.0),
            "0",
            &[Some(0.0), Some(0.0)],
        );
        add(
            &mut model,
            TextGeometry::new(p(70.0, 0.0), "RL 0.000", 2.5, 0.0),
            "0",
            &[Some(0.0)],
        );
        let out = write_dxf(&model, &ExportOptions::default()).expect("the export");
        // Nothing is lost to another program - its Z 0 is the level - so there
        // is nothing to warn of.
        assert!(out.warnings.is_empty());
        let back = read(out.text.as_str());
        assert_eq!(back.entities.len(), 8);
        let e = &back.entities;
        assert_eq!(heights_of(&e[0].properties, 1)[0], Some(0.0));
        assert_eq!(heights_of(&e[1].properties, 1)[0], Some(5.0));
        assert!(heights_of(&e[2].properties, 1)[0].is_none());
        for height in heights_of(&e[3].properties, 3) {
            assert_eq!(height, Some(0.0));
        }
        for height in heights_of(&e[4].properties, 2) {
            assert_eq!(height, Some(0.0));
        }
        assert_eq!(heights_of(&e[5].properties, 1)[0], Some(0.0));
        for height in heights_of(&e[6].properties, 2) {
            assert_eq!(height, Some(0.0));
        }
        assert_eq!(heights_of(&e[7].properties, 1)[0], Some(0.0));
    }

    // ---- the drawing system's kinds (docs/drawing.md) ------------------------------

    #[test]
    fn a_curve_polyline_goes_out_with_its_bulges_and_comes_back_the_same() {
        let mut model = Model::default();
        let mut shape =
            CurvePolyline2::from_points(&[p(0.0, 0.0), p(10.0, 0.0), p(20.0, 10.0)], false);
        shape.vertices[1].bulge = (PI / 8.0).tan();
        add(&mut model, shape, "0", &[]);
        let mut closed =
            CurvePolyline2::from_points(&[p(0.0, 0.0), p(4.0, 0.0), p(4.0, 4.0)], false);
        closed.closed = true;
        closed.vertices[2].bulge = -0.5; // the closing segment
        add(&mut model, closed.clone(), "0", &[]);
        let text = written(&model, &ExportOptions::default());
        assert_eq!(records(&text, "LWPOLYLINE"), 2);
        let back = read(text.as_str());
        let curved = of_kind(&back, EntityType::CurvePolyline);
        assert_eq!(curved.len(), 2);
        let first = curve_polyline_of(curved[0]);
        assert_eq!(first.vertices.len(), 3);
        assert!(!first.closed);
        near(first.vertices[1].bulge, (PI / 8.0).tan(), 1e-12);
        double_eq(first.vertices[0].bulge, 0.0);
        let second = curve_polyline_of(curved[1]);
        assert!(second.closed);
        near(second.vertices[2].bulge, -0.5, 1e-12);
        near(second.length(), closed.length(), 1e-9);
    }

    #[test]
    fn an_ellipse_and_an_elliptical_arc_come_back_as_ellipses() {
        let mut model = Model::default();
        let full = Ellipse2 {
            center: p(100.0, 50.0),
            major_axis: Vec2::new(6.0, 8.0),
            ratio: 0.4,
            ..Ellipse2::default()
        };
        add(&mut model, full, "0", &[]);
        let arc = Ellipse2 {
            start_parameter: 0.5,
            sweep: 2.0,
            ..full
        };
        add(&mut model, arc, "0", &[]);
        let text = written(&model, &ExportOptions::default());
        assert_eq!(records(&text, "ELLIPSE"), 2);
        let back = read(text.as_str());
        let ellipses = of_kind(&back, EntityType::Ellipse);
        assert_eq!(ellipses.len(), 2);
        let a = ellipse_of(ellipses[0]);
        assert_eq!(a.center, full.center);
        near(a.major_axis.x, 6.0, 1e-12);
        near(a.major_axis.y, 8.0, 1e-12);
        near(a.ratio, 0.4, 1e-12);
        assert!(a.is_full());
        let b = ellipse_of(ellipses[1]);
        near(b.start_parameter, 0.5, 1e-12);
        near(b.sweep, 2.0, 1e-12);
    }

    #[test]
    fn a_spline_comes_back_with_its_control_points_knots_and_fit_points() {
        let mut model = Model::default();
        let spline = Spline2::through_points(
            vec![p(0.0, 0.0), p(5.0, 5.0), p(10.0, 0.0), p(15.0, 5.0)],
            3,
        )
        .expect("the spline");
        add(&mut model, spline.clone(), "0", &[]);
        let text = written(&model, &ExportOptions::default());
        assert_eq!(records(&text, "SPLINE"), 1);
        let back = read(text.as_str());
        let splines = of_kind(&back, EntityType::Spline);
        assert_eq!(splines.len(), 1);
        let read_back = spline_of(splines[0]);
        assert_eq!(read_back.degree, spline.degree);
        assert_eq!(read_back.control_points.len(), spline.control_points.len());
        for (got, expected) in read_back.control_points.iter().zip(&spline.control_points) {
            near(got.distance_to(*expected), 0.0, 1e-12);
        }
        assert_eq!(read_back.knots.len(), spline.knots.len());
        assert_eq!(read_back.fit_points.len(), 4);
        near(read_back.length(), spline.length(), 1e-9);
    }

    // ---- annotation (docs/annotation.md, "Exchange") -------------------------------

    #[test]
    fn paper_sized_text_is_written_at_the_annotation_scale() {
        let mut model = Model::default();
        let note = TextGeometry {
            paper_height: 3.5,
            ..TextGeometry::new(p(0.0, 0.0), "NOTE", 2.5, 0.0)
        };
        add(&mut model, note, "0", &[]);
        let options = ExportOptions {
            annotation_scale: 500.0,
            ..ExportOptions::default()
        };
        let back = read(written(&model, &options).as_str());
        assert_eq!(back.entities.len(), 1);
        // 3.5 mm at 1:500 is 1.75 m; the model height is not what the sheet shows.
        double_eq(text_of(&back.entities[0]).height, 1.75);
    }

    #[test]
    fn with_nothing_drawn_a_label_is_skipped_and_said_so() {
        let (model, _label) = labelled_point();
        let out = write_dxf(&model, &ExportOptions::default()).expect("the export");
        assert_eq!(
            out.entities_written, 1,
            "the point; the label is not counted written"
        );
        assert_eq!(out.entities_skipped, 1);
        assert!(!out.warnings.is_empty());
        assert!(out.warnings[0].contains("1 labels were not written"));
        assert_eq!(records(&out.text, "TEXT"), 0);
    }

    #[test]
    fn drawn_annotation_is_written_as_its_shapes_on_its_layer() {
        let (mut model, label) = labelled_point();
        assert!(
            model
                .layers
                .add(Layer {
                    name: String::from("Labels"),
                    ..Layer::default()
                })
                .is_ok()
        );
        let mut moved = model.entities.find(label).expect("the label").clone();
        moved.layer = String::from("Labels");
        assert!(model.entities.replace(moved).is_ok());
        // What the cad layer's drawAnnotationForExport would hand over: the
        // label's text and a leader stroke.
        let drawn: BTreeMap<EntityId, Vec<Geometry>> = BTreeMap::from([(
            label,
            vec![
                Geometry::Text(TextGeometry::new(p(2.0, 2.0), "P1", 1.25, 0.0)),
                Geometry::Line(Segment2::new(p(1.0, 1.0), p(2.0, 2.0))),
            ],
        )]);
        let options = ExportOptions {
            drawn: Some(&drawn),
            ..ExportOptions::default()
        };
        let out = write_dxf(&model, &options).expect("the export");
        assert_eq!(out.entities_written, 2);
        assert_eq!(out.entities_skipped, 0);
        assert!(out.warnings.is_empty());
        let back = read(out.text.as_str());
        let texts = of_kind(&back, EntityType::Text);
        assert_eq!(texts.len(), 1);
        assert_eq!(text_of(texts[0]).text, "P1");
        assert_eq!(texts[0].layer, "Labels");
        assert_eq!(of_kind(&back, EntityType::Line).len(), 1);
    }

    #[test]
    fn a_label_drawn_as_nothing_is_left_out_and_said_so() {
        let (model, label) = labelled_point();
        let drawn: BTreeMap<EntityId, Vec<Geometry>> = BTreeMap::from([(label, Vec::new())]);
        let options = ExportOptions {
            drawn: Some(&drawn),
            annotation_scale: 2000.0,
            ..ExportOptions::default()
        };
        let out = write_dxf(&model, &options).expect("the export");
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].contains("1 labels had no room at 1:2000"));
        assert_eq!(records(&out.text, "TEXT"), 0);
    }

    #[test]
    fn with_nothing_drawn_a_leader_is_its_line_landing_and_note() {
        let mut model = Model::default();
        let leader = LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(10.0, 10.0)],
            text: String::from("PIT 12"),
            ..LeaderGeometry::default()
        };
        add(&mut model, leader, "0", &[]);
        let out = write_dxf(&model, &ExportOptions::default()).expect("the export");
        // The segment and the landing, and the note.
        assert_eq!(records(&out.text, "LINE"), 2);
        assert_eq!(records(&out.text, "TEXT"), 1);
    }

    // A smart leader's note is read off the entity its tip is on, here too
    // (docs/annotation.md, "Smart leaders"): the file says what the drawing
    // says, not the template.
    #[test]
    fn with_nothing_drawn_a_smart_leader_writes_the_note_it_reads() {
        let mut model = Model::default();
        let mut pit = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: p(0.0, 0.0),
            }),
            ..Entity::default()
        };
        pit.properties
            .insert(String::from("invert"), PropertyValue::Real(10.5));
        assert!(model.entities.add(pit).is_ok());
        let leader = LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(10.0, 10.0)],
            text: String::from("IL {prop.invert:.2f}\nRL {rl:.3f}"),
            fields: true,
            tip_ref: AnchorRef {
                entity: 1,
                point: AnchorPoint::Position,
                ..AnchorRef::default()
            },
            ..LeaderGeometry::default()
        };
        add(&mut model, leader, "0", &[]);
        let out = write_dxf(&model, &ExportOptions::default()).expect("the export");
        assert!(out.text.contains("IL 10.50"));
        assert!(!out.text.contains("{prop.invert"), "never the template");
        assert_eq!(
            records(&out.text, "TEXT"),
            1,
            "no line for a level the pit has not got"
        );
    }
}

mod dxf_writer_drawing_kinds {
    use super::*;

    #[test]
    fn an_elliptical_arc_through_the_parameter_zero_is_written_in_one_turn() {
        // From 5.5 rad for 2 rad: the end, 7.5, is 7.5 - 2 pi = 1.2168... in the
        // turn a DXF reader expects, below the start - which is how the format
        // says the arc passes 0.
        let mut model = Model::default();
        let arc = Ellipse2 {
            center: p(0.0, 0.0),
            major_axis: Vec2::new(4.0, 0.0),
            ratio: 0.5,
            start_parameter: 5.5,
            sweep: 2.0,
        };
        add(&mut model, arc, "0", &[]);
        let text = written(&model, &ExportOptions::default());
        let from = text.find("AcDbEllipse").expect("an ELLIPSE");
        let at = from
            + text[from..]
                .find("\n 42\n")
                .unwrap_or_else(|| panic!("{text}"));
        let end: f64 = text[at + 5..]
            .lines()
            .next()
            .and_then(|value| value.parse().ok())
            .expect("the end parameter");
        near(end, 7.5 - 2.0 * PI, 1e-12);
        let back = read(text.as_str());
        let ellipses = of_kind(&back, EntityType::Ellipse);
        assert_eq!(ellipses.len(), 1);
        let read_back = ellipse_of(ellipses[0]);
        near(read_back.start_parameter, 5.5, 1e-12);
        near(read_back.sweep, 2.0, 1e-12);
    }
}
