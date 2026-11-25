//! The writer (docs/dgn.md, "Writing"): a drawing with known coordinates,
//! layers and colours written on a seed built in the test and read back, and
//! the bytes the format fixes checked where it fixes them.

pub mod support;

use std::collections::BTreeMap;

use katana_core::ErrorCode;
use katana_dgn::codes::{int32, vax_double, word};
use katana_dgn::writer::{palette_for, units_for};
use katana_dgn::{
    DgnExport, DgnImport, ExportOptions, ImportOptions, META_JUSTIFICATION, META_STYLE,
    META_WEIGHT, read_dgn, write_dgn, write_dgn_file,
};
use katana_entity::entity::{heights_of, set_heights};
use katana_entity::{
    Color, DimensionGeometry, Entity, EntityId, Geometry, Layer, Model, PointGeometry,
    PropertyValue, TextGeometry,
};
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, CurveVertex, Ellipse2, Point2, Polyline2, Segment2, Vec2,
};
use katana_math::numerics::PI;
use support::{Design, Scratch, near};

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b, 255)
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn layer(model: &mut Model, name: &str, colour: Color) {
    let made = Layer {
        name: String::from(name),
        color: colour,
        ..Layer::default()
    };
    if let Err(error) = model.layers.add(made) {
        panic!("{}", error.describe());
    }
}

fn add(model: &mut Model, geometry: impl Into<Geometry>, layer: &str) -> EntityId {
    add_entity(
        model,
        Entity {
            geometry: geometry.into(),
            layer: String::from(layer),
            ..Entity::default()
        },
    )
}

fn add_entity(model: &mut Model, entity: Entity) -> EntityId {
    match model.entities.add(entity) {
        Ok(id) => id,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn seed_2d() -> Vec<u8> {
    Design::new(false, 12, 10, [7.0, 8.0, 0.0]).finish()
}

fn seed_3d() -> Vec<u8> {
    Design::new(true, 12, 10, [7.0, 8.0, 9.0]).finish()
}

fn write(model: &Model, seed: &[u8]) -> DgnExport {
    match write_dgn(model, seed, &ExportOptions::default()) {
        Ok(written) => written,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn read_back(written: &DgnExport) -> DgnImport {
    match read_dgn(&written.bytes, &ImportOptions::default()) {
        Ok(imported) => imported,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn said(warnings: &[String], part: &str) -> bool {
    warnings.iter().any(|warning| warning.contains(part))
}

/// The elements of a design: (type, level, offset), the header first.
fn elements(bytes: &[u8]) -> Vec<(u8, u8, usize)> {
    let mut found = Vec::new();
    let mut at = 0;
    while at + 4 <= bytes.len() && bytes[at..at + 2] != [0xFF, 0xFF] {
        found.push((bytes[at + 1] & 0x7F, bytes[at] & 0x3F, at));
        at += 4 + 2 * usize::from(word(bytes, at + 2).expect("a word"));
    }
    found
}

mod round_trip {
    use super::*;

    #[test]
    fn a_drawing_written_and_read_back_is_the_drawing_it_was() {
        // An MGA site. Every coordinate has at most four decimals, the step
        // of 10 000 units of resolution a metre the writer chooses for a
        // drawing this size, so each comes back as the double it was; an
        // angle comes back to its step, 1/360000 degree (4.85e-8 rad).
        let mut model = Model::default();
        layer(&mut model, "survey/points", rgb(12, 34, 56));
        layer(&mut model, "survey/kerb", rgb(0, 255, 0));
        layer(&mut model, "level 9", rgb(0, 0, 0));
        add(
            &mut model,
            PointGeometry {
                position: p(330_000.123_4, 6_250_000.567_8),
            },
            "survey/points",
        );
        add_entity(
            &mut model,
            Entity {
                geometry: Geometry::Line(Segment2::new(
                    p(330_000.0, 6_250_000.0),
                    p(330_010.5, 6_250_004.25),
                )),
                layer: String::from("survey/kerb"),
                color: Some(rgb(255, 0, 0)),
                ..Entity::default()
            },
        );
        add(
            &mut model,
            Polyline2::new(
                vec![
                    p(330_000.0, 6_250_010.0),
                    p(330_030.0, 6_250_010.0),
                    p(330_030.0, 6_250_030.0),
                    p(330_000.0, 6_250_030.0),
                ],
                true,
            ),
            "survey/kerb",
        );
        add(
            &mut model,
            Arc2::new(p(330_005.0, 6_250_005.0), 2.5, PI / 6.0, -2.0 * PI / 3.0),
            "survey/kerb",
        );
        add(&mut model, Circle2::new(p(330_020.0, 6_250_000.0), 1.25), "survey/kerb");
        add(
            &mut model,
            Ellipse2 {
                center: p(330_030.0, 6_250_000.0),
                major_axis: Vec2::new(3.0, 4.0),
                ratio: 0.5,
                start_parameter: 0.0,
                sweep: 2.0 * PI,
            },
            "survey/kerb",
        );
        let vertex = |x: f64, y: f64, bulge: f64| CurveVertex {
            position: p(x, y),
            bulge,
            height: None,
        };
        add(
            &mut model,
            CurvePolyline2 {
                vertices: vec![
                    vertex(330_040.0, 6_250_000.0, 1.0),
                    vertex(330_044.0, 6_250_000.0, 0.0),
                    vertex(330_044.0, 6_250_004.0, 0.0),
                ],
                closed: false,
            },
            "survey/kerb",
        );
        add(
            &mut model,
            TextGeometry::new(p(330_001.0, 6_250_001.0), "LOT 42", 2.5, 0.0),
            "level 9",
        );

        let written = write(&model, &seed_2d());
        assert_eq!(written.entities_written, 8);
        assert_eq!(written.entities_skipped, 0);
        assert_eq!(written.units.per_master(), 10_000.0);
        // Layers to levels in the order first met; "level 9" keeps its own.
        let levels: Vec<(u8, &str)> = written
            .levels
            .iter()
            .map(|level| (level.level, level.layer.as_str()))
            .collect();
        assert_eq!(levels, [(1, "survey/points"), (2, "survey/kerb"), (9, "level 9")]);
        // Black is MicroStation's background, and goes as the foreground.
        assert!(said(&written.warnings, "black"), "{:?}", written.warnings);

        let read = read_back(&written);
        assert_eq!(read.entities.len(), 8, "{:?}", read.warnings);
        let layers: Vec<&str> = read.layers.iter().map(|layer| layer.name.as_str()).collect();
        assert_eq!(layers, ["level 1", "level 2", "level 9"]);
        let entities = &read.entities;

        assert_eq!(
            entities[0].geometry,
            Geometry::Point(PointGeometry {
                position: p(330_000.123_4, 6_250_000.567_8)
            })
        );
        assert_eq!(entities[0].color, Some(rgb(12, 34, 56)));
        assert_eq!(
            entities[1].geometry,
            Geometry::Line(Segment2::new(p(330_000.0, 6_250_000.0), p(330_010.5, 6_250_004.25)))
        );
        assert_eq!(entities[1].color, Some(rgb(255, 0, 0)));
        let Geometry::Polyline(lot) = &entities[2].geometry else {
            panic!("{:?}", entities[2].geometry)
        };
        assert!(lot.closed);
        assert_eq!(lot.vertices.len(), 4);
        assert_eq!(lot.area(), 600.0);
        assert_eq!(entities[2].color, Some(rgb(0, 255, 0)));

        let Geometry::Arc(arc) = &entities[3].geometry else {
            panic!("{:?}", entities[3].geometry)
        };
        assert_eq!(arc.center, p(330_005.0, 6_250_005.0));
        assert_eq!(arc.radius, 2.5);
        near(arc.start_angle, PI / 6.0, 5e-8);
        near(arc.sweep, -2.0 * PI / 3.0, 5e-8);

        assert_eq!(
            entities[4].geometry,
            Geometry::Circle(Circle2::new(p(330_020.0, 6_250_000.0), 1.25))
        );
        let Geometry::Ellipse(ellipse) = &entities[5].geometry else {
            panic!("{:?}", entities[5].geometry)
        };
        assert_eq!(ellipse.center, p(330_030.0, 6_250_000.0));
        // The axis's angle, atan2(4, 3), to its step: 5 m x 4.85e-8 rad.
        near(ellipse.major_axis.x, 3.0, 3e-7);
        near(ellipse.major_axis.y, 4.0, 3e-7);
        near(ellipse.ratio, 0.5, 1e-15);

        let Geometry::CurvePolyline(bend) = &entities[6].geometry else {
            panic!("{:?}", entities[6].geometry)
        };
        assert_eq!(bend.vertices.len(), 3);
        assert_eq!(bend.vertices[0].position, p(330_040.0, 6_250_000.0));
        near(bend.vertices[0].bulge, 1.0, 1e-12);
        assert_eq!(bend.vertices[1].position, p(330_044.0, 6_250_000.0));
        assert_eq!(bend.vertices[1].bulge, 0.0);
        assert_eq!(bend.vertices[2].position, p(330_044.0, 6_250_004.0));

        let Geometry::Text(text) = &entities[7].geometry else {
            panic!("{:?}", entities[7].geometry)
        };
        assert_eq!(text.text, "LOT 42");
        assert_eq!(text.position, p(330_001.0, 6_250_001.0));
        // A height is held to 6/1000 of a unit of resolution: 6e-7 m here.
        near(text.height, 2.5, 3e-7);
        // Written as colour 0, which reads as its white.
        assert_eq!(entities[7].color, Some(rgb(255, 255, 255)));
    }

    #[test]
    fn style_weight_and_a_files_justification_go_back_as_they_came() {
        let mut model = Model::default();
        let mut dashed = Entity {
            geometry: Geometry::Line(Segment2::new(p(0.0, 0.0), p(10.0, 0.0))),
            ..Entity::default()
        };
        dashed
            .metadata
            .insert(String::from(META_STYLE), PropertyValue::Integer(3));
        dashed
            .metadata
            .insert(String::from(META_WEIGHT), PropertyValue::Integer(5));
        add_entity(&mut model, dashed);
        let mut centred = Entity {
            geometry: Geometry::Text(TextGeometry::new(p(1.0, 1.0), "A", 1.0, 0.0)),
            ..Entity::default()
        };
        centred
            .metadata
            .insert(String::from(META_JUSTIFICATION), PropertyValue::Integer(7));
        add_entity(&mut model, centred);
        let read = read_back(&write(&model, &seed_2d()));
        let meta = |index: usize, key: &str| read.entities[index].metadata.get(key).cloned();
        assert_eq!(meta(0, META_STYLE), Some(PropertyValue::Integer(3)));
        assert_eq!(meta(0, META_WEIGHT), Some(PropertyValue::Integer(5)));
        assert_eq!(meta(1, META_JUSTIFICATION), Some(PropertyValue::Integer(7)));
    }

    #[test]
    fn the_file_is_written_whole_and_replaces_one_there() {
        let scratch = Scratch::new("replace");
        let path = scratch.root.join("site.dgn");
        let mut model = Model::default();
        add(&mut model, Segment2::new(p(0.0, 0.0), p(1.0, 0.0)), "0");
        for _ in 0..2 {
            let written = write_dgn_file(&model, &seed_2d(), &path, &ExportOptions::default())
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            assert!(written.bytes.is_empty());
            assert!(written.bytes_written > 1536);
        }
        let bytes = std::fs::read(&path).expect("written");
        let read = read_dgn(&bytes, &ImportOptions::default()).expect("read");
        assert_eq!(read.entities.len(), 1);
        assert!(!scratch.root.join("site.dgn.partial").exists());
    }
}

mod the_design {
    use super::*;

    #[test]
    fn units_are_the_finest_step_whose_plane_holds_the_drawing_about_its_middle() {
        // The plane is 2^31 - 1 steps either side of the middle. 30 x 20 m
        // about (15, 10), reaching 15 m and a metre's margin: 16 x 10 000 is
        // well inside, so 0.1 mm, and the global origin is -(15, 10) x 10 000.
        let (units, origin) = units_for(p(0.0, 0.0), p(30.0, 20.0), false).expect("units");
        assert_eq!((units.subunits_per_master, units.units_per_subunit), (1000, 10));
        assert_eq!((units.master.as_str(), units.sub.as_str()), ("m", "mm"));
        assert_eq!((origin[0], origin[1]), (-150_000.0, -100_000.0));
        // An MGA site's middle, (330 100, 6 250 050).
        let (_, origin) = units_for(p(330_000.0, 6_250_000.0), p(330_200.0, 6_250_100.0), false)
            .expect("units");
        assert_eq!((origin[0], origin[1]), (-3_301_000_000.0, -62_500_500_000.0));
        // 300 km across reaches 150 001 m: 1.50001e9 steps of 0.1 mm fit.
        let (units, _) = units_for(p(0.0, 0.0), p(300_000.0, 0.0), false).expect("units");
        assert_eq!(units.per_master(), 10_000.0);
        // 500 km: 2.50001e9 steps of 0.1 mm do not, 2.5e8 of 1 mm do.
        let (units, _) = units_for(p(0.0, 0.0), p(500_000.0, 0.0), false).expect("units");
        assert_eq!(units.per_master(), 1000.0);
        // 5000 km: a centimetre.
        let (units, _) = units_for(p(0.0, 0.0), p(5_000_000.0, 0.0), false).expect("units");
        assert_eq!((units.per_master(), units.sub.as_str()), (100.0, "cm"));
        // Past what a metre's step holds, refused.
        let error = units_for(p(0.0, 0.0), p(1e10, 0.0), false).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn the_header_is_the_seeds_with_katanas_units_then_the_colour_table_and_the_end_word() {
        let seed = seed_2d();
        let mut model = Model::default();
        add(&mut model, Segment2::new(p(0.0, 0.0), p(30.0, 20.0)), "0");
        let written = write(&model, &seed);
        let bytes = &written.bytes;
        // The seed's own signature, and the rest of its header but the units
        // and the origin.
        assert_eq!(bytes[0..4], seed[0..4]);
        assert_eq!(bytes[1124..1214], seed[1124..1214]);
        assert_eq!(int32(bytes, 1112), Some(1000));
        assert_eq!(int32(bytes, 1116), Some(10));
        assert_eq!(&bytes[1120..1124], b"m\0mm");
        assert_eq!(vax_double(bytes, 1240), Some(-150_000.0));
        assert_eq!(vax_double(bytes, 1248), Some(-100_000.0));
        assert_eq!(bytes[bytes.len() - 2..], [0xFF, 0xFF]);
        // Then a colour table - group data on level 1, 806 bytes - and the
        // line, on level 1 (layer "0", the first met).
        let found = elements(bytes);
        assert_eq!(found[0], (9, 8, 0));
        assert_eq!(found[1], (5, 1, 1536));
        assert_eq!(found[2], (3, 1, 1536 + 806));
        assert_eq!(found.len(), 3);
        assert_eq!(written.elements_written, 3);
    }

    #[test]
    fn a_seeds_units_are_kept_when_asked_and_a_drawing_off_its_plane_is_refused() {
        // A seed at 1000 units a metre with its origin at 0,0: its plane
        // reaches 2 147 483.647 m either way.
        let seed = Design::new(false, 1000, 1, [0.0; 3]).finish();
        let keep = ExportOptions {
            keep_seed_units: true,
            ..ExportOptions::default()
        };
        let mut model = Model::default();
        add(&mut model, Segment2::new(p(1000.0, 0.0), p(1001.0, 0.0)), "0");
        let written = write_dgn(&model, &seed, &keep).expect("written");
        assert_eq!(written.units.per_master(), 1000.0);
        assert_eq!(vax_double(&written.bytes, 1240), Some(0.0));
        let read = read_back(&written);
        assert_eq!(
            read.entities[0].geometry,
            Geometry::Line(Segment2::new(p(1000.0, 0.0), p(1001.0, 0.0)))
        );
        // 3 000 km is 3e9 units: off the plane.
        add(&mut model, Segment2::new(p(3_000_000.0, 0.0), p(3_000_001.0, 0.0)), "0");
        let error = write_dgn(&model, &seed, &keep).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("design plane"), "{}", error.message);
    }

    #[test]
    fn a_seed_that_is_no_v7_design_is_refused() {
        let model = Model::default();
        let mut v8 = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        v8.resize(2048, 0);
        let error = write_dgn(&model, &v8, &ExportOptions::default()).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("V8"), "{}", error.message);
        let error = write_dgn(&model, &[], &ExportOptions::default()).expect_err("refused");
        assert!(error.message.contains("empty"), "{}", error.message);
        let short = &seed_2d()[..1000];
        assert!(write_dgn(&model, short, &ExportOptions::default()).is_err());
    }

    #[test]
    fn an_empty_drawing_is_a_design_of_its_header_and_colour_table() {
        let written = write(&Model::default(), &seed_2d());
        assert_eq!(written.entities_written, 0);
        assert_eq!(written.elements_written, 2);
        assert_eq!(written.bytes.len(), 1536 + 806 + 2);
        assert!(written.warnings.is_empty(), "{:?}", written.warnings);
        let read = read_back(&written);
        assert!(read.entities.is_empty());
        assert!(read.colour_table);
        assert!(read.warnings.is_empty(), "{:?}", read.warnings);
    }
}

mod elements_written {
    use super::*;

    #[test]
    fn a_polyline_of_more_than_101_vertices_is_a_complex_chain_of_line_strings() {
        // 250 vertices: line strings of 101, 101 and 50, each starting where
        // the last ended (0-100, 100-200, 200-249). Closed, 150 vertices and
        // the closing one make 151: two pieces under one complex shape.
        let zigzag: Vec<Point2> = (0..250).map(|index| p(f64::from(index), f64::from(index % 2))).collect();
        let ring: Vec<Point2> = (0..150)
            .map(|index| {
                let angle = 2.0 * PI * f64::from(index) / 150.0;
                p((100.0 * angle.cos()).round(), (100.0 * angle.sin()).round())
            })
            .collect();
        let mut model = Model::default();
        add(&mut model, Polyline2::new(zigzag.clone(), false), "0");
        add(&mut model, Polyline2::new(ring.clone(), true), "0");
        let written = write(&model, &seed_2d());
        let kinds: Vec<u8> = elements(&written.bytes).iter().map(|found| found.0).collect();
        assert_eq!(kinds, [9, 5, 12, 4, 4, 4, 14, 4, 4]);
        let counts: Vec<u16> = elements(&written.bytes)
            .iter()
            .filter(|found| found.0 == 4)
            .map(|found| word(&written.bytes, found.2 + 36).expect("a count"))
            .collect();
        assert_eq!(counts, [101, 101, 50, 101, 51]);
        let read = read_back(&written);
        assert_eq!(read.entities.len(), 2, "{:?}", read.warnings);
        assert_eq!(read.entities[0].geometry, Geometry::Polyline(Polyline2::new(zigzag, false)));
        assert_eq!(read.entities[1].geometry, Geometry::Polyline(Polyline2::new(ring, true)));
    }

    #[test]
    fn levels_go_to_layers_first_met_first_and_a_level_layer_keeps_its_number() {
        let mut model = Model::default();
        for name in ["b", "level 1", "a"] {
            add(&mut model, Segment2::new(p(0.0, 0.0), p(1.0, 0.0)), name);
        }
        let written = write(&model, &seed_2d());
        let levels: Vec<(u8, &str)> = written
            .levels
            .iter()
            .map(|level| (level.level, level.layer.as_str()))
            .collect();
        assert_eq!(levels, [(1, "level 1"), (2, "b"), (3, "a")]);
        let read = read_back(&written);
        let on: Vec<&str> = read.entities.iter().map(|entity| entity.layer.as_str()).collect();
        assert_eq!(on, ["level 2", "level 1", "level 3"]);
    }

    #[test]
    fn layers_past_the_63_levels_share_level_63_and_say_so() {
        let mut model = Model::default();
        for index in 0..70 {
            add(&mut model, Segment2::new(p(0.0, 0.0), p(1.0, 0.0)), &format!("layer {index:02}"));
        }
        let written = write(&model, &seed_2d());
        assert!(said(&written.warnings, "7 layers went beyond the 63 levels"), "{:?}", written.warnings);
        assert_eq!(written.levels.iter().filter(|level| level.level == 63).count(), 8);
    }

    #[test]
    fn colours_take_their_own_slot_from_254_down_and_keep_0_to_15() {
        // Red is colour 3 of the default table; a colour the table lacks
        // takes slot 254, and keeps it; black is 0, the foreground.
        let (indices, table) = palette_for(&[
            rgb(255, 0, 0),
            rgb(12, 34, 56),
            rgb(12, 34, 56),
            rgb(0, 0, 0),
        ]);
        assert_eq!(indices, [3, 254, 254, 0]);
        assert_eq!(table[254], [12, 34, 56]);
        // 256 colours none of the table has: slots 254 down to 16 hold the
        // first 239 exactly.
        let many: Vec<Color> = (0..=255).map(|red| rgb(red, 1, 2)).collect();
        let (indices, table) = palette_for(&many);
        assert_eq!(indices[0], 254);
        assert_eq!(indices[238], 16);
        assert_eq!(table[16], [238, 1, 2]);
    }

    #[test]
    fn heights_go_to_a_3d_seed_and_partial_ones_go_in_plan_and_are_counted() {
        let mut model = Model::default();
        let mut climbing = Entity {
            geometry: Geometry::Polyline(Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], false)),
            ..Entity::default()
        };
        set_heights(&mut climbing.properties, &[Some(10.5), Some(11.25)]);
        add_entity(&mut model, climbing);
        let mut partial = Entity {
            geometry: Geometry::Polyline(Polyline2::new(vec![p(0.0, 5.0), p(10.0, 5.0)], false)),
            ..Entity::default()
        };
        set_heights(&mut partial.properties, &[Some(1.0), None]);
        add_entity(&mut model, partial);
        let written = write(&model, &seed_3d());
        assert!(written.three_d);
        assert!(said(&written.warnings, "written in plan"), "{:?}", written.warnings);
        let read = read_back(&written);
        assert_eq!(read.format(), "DGN V7 3D");
        assert_eq!(
            heights_of(&read.entities[0].properties, 2),
            vec![Some(10.5), Some(11.25)]
        );
        assert_eq!(heights_of(&read.entities[1].properties, 2), vec![None, None]);
    }

    #[test]
    fn a_2d_seed_holds_no_heights_and_says_so() {
        let mut model = Model::default();
        let mut point = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: p(1.0, 2.0),
            }),
            ..Entity::default()
        };
        set_heights(&mut point.properties, &[Some(12.5)]);
        add_entity(&mut model, point);
        let written = write(&model, &seed_2d());
        assert!(said(&written.warnings, "2D design"), "{:?}", written.warnings);
        assert!(katana_dgn::has_heights(&model));
    }

    #[test]
    fn text_is_windows1252_a_line_to_an_element_cut_at_255_and_said() {
        let mut model = Model::default();
        add(
            &mut model,
            TextGeometry::new(p(0.0, 0.0), "90\u{b0} \u{4e2d}\nNEXT", 1.0, 0.0),
            "0",
        );
        add(&mut model, TextGeometry::new(p(0.0, 10.0), "A".repeat(300), 1.0, 0.0), "0");
        let written = write(&model, &seed_2d());
        assert!(said(&written.warnings, "1 characters have no place in Windows-1252"), "{:?}", written.warnings);
        assert!(said(&written.warnings, "255 characters"), "{:?}", written.warnings);
        let read = read_back(&written);
        let texts: Vec<String> = read
            .entities
            .iter()
            .filter_map(|entity| match &entity.geometry {
                Geometry::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["90\u{b0} ?", "NEXT", "A".repeat(255).as_str()]);
        // A text's bottom left is the baseline of its LAST line
        // (katana_entity's text_block): the first line sits a pitch - 5/3 of
        // the height - above the second, which is at the text's position.
        let Geometry::Text(first) = &read.entities[0].geometry else {
            panic!()
        };
        let Geometry::Text(second) = &read.entities[1].geometry else {
            panic!()
        };
        near(first.position.y, 5.0 / 3.0, 1e-4);
        near(second.position.y, 0.0, 1e-4);
        near(second.position.x, 0.0, 1e-4);
    }

    #[test]
    fn annotation_with_nothing_drawn_is_skipped_and_what_was_drawn_is_written() {
        let mut model = Model::default();
        let dimension = add(
            &mut model,
            DimensionGeometry::new(p(0.0, 0.0), p(10.0, 0.0), 2.0, ""),
            "0",
        );
        let written = write(&model, &seed_2d());
        assert_eq!(written.entities_skipped, 1);
        assert!(said(&written.warnings, "were not written"), "{:?}", written.warnings);
        let mut drawn = BTreeMap::new();
        drawn.insert(
            dimension,
            vec![Geometry::Line(Segment2::new(p(0.0, 2.0), p(10.0, 2.0)))],
        );
        let options = ExportOptions {
            drawn: Some(&drawn),
            ..ExportOptions::default()
        };
        let written = write_dgn(&model, &seed_2d(), &options).expect("written");
        assert_eq!(written.entities_written, 1);
        let read = read_back(&written);
        assert_eq!(
            read.entities[0].geometry,
            Geometry::Line(Segment2::new(p(0.0, 2.0), p(10.0, 2.0)))
        );
    }
}
