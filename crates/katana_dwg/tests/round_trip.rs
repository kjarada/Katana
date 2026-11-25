//! A Katana drawing written to DWG and read back is the drawing it was, in every release
//! written (docs/dwg.md, "Writing"). Every expected value is the one the test put in: the
//! drawing is built by hand, so what comes back is checked against what went out, through
//! the same comparisons katana_dxf's writer tests make of a DXF.

pub mod support;

use katana_dwg::DwgRelease;
use katana_entity::entity::heights_of;
use katana_entity::{
    Entity, EntityType, Geometry, Layer, Linetype, LinetypeElement, Model, PointGeometry,
    TextGeometry,
};
use katana_geometry::{Arc2, Circle2, Polyline2, Segment2};
use katana_math::numerics::PI;
use support::{add, add_layer, dwg_of, layer_named, near, p, read, rgb, round_trip, warned};

fn of_kind(imported: &katana_dxf::DxfImport, kind: EntityType) -> Vec<&Entity> {
    imported
        .entities
        .iter()
        .filter(|entity| entity.r#type() == kind)
        .collect()
}

/// The drawing every release is written from, by hand:
///
/// ```text
/// layer Survey          red (255, 0, 0), an indexed colour
/// layer Survey/Kerb     (10, 200, 30), no indexed colour; hidden, locked, 0.5 mm,
///                       linetype "kerb dash": 2 on, 0.5 off, a dot, 0.5 off
/// on Survey:            a point at (502000.125, 6250000.1), height 31.245
/// on 0:                 a line (0.1, 0.2)-(3, 4); an arc about (650, 200), radius 8, from
///                       0.25 rad through 1.5 rad; a circle about (-7.5, 1/3), radius 0.6;
///                       an open polyline (0,0) (10,0) (10,5); the text "RL 32.45° Böschung"
///                       at (5, 6), 2.75 high, turned a quarter
/// on Survey/Kerb:       a closed polyline (0,0) (10,0) (10,10) at heights 10, 11, 12.5
/// ```
fn site() -> Model {
    let mut model = Model::default();
    add_layer(
        &mut model,
        Layer {
            name: String::from("Survey"),
            color: rgb(255, 0, 0),
            ..Layer::default()
        },
    );
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
    if let Err(error) = model.linetypes.add(dashed) {
        panic!("{}", error.describe());
    }
    add_layer(
        &mut model,
        Layer {
            name: String::from("Survey/Kerb"),
            color: rgb(10, 200, 30),
            visible: false,
            locked: true,
            line_weight: 0.5,
            linetype: String::from("kerb dash"),
            ..Layer::default()
        },
    );
    add(
        &mut model,
        PointGeometry {
            position: p(502000.125, 6250000.1),
        },
        "Survey",
        &[Some(31.245)],
    );
    add(&mut model, Segment2::new(p(0.1, 0.2), p(3.0, 4.0)), "0", &[]);
    add(&mut model, Arc2::new(p(650.0, 200.0), 8.0, 0.25, 1.5), "0", &[]);
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
    add(
        &mut model,
        Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)], true),
        "Survey/Kerb",
        &[Some(10.0), Some(11.0), Some(12.5)],
    );
    model
}

mod dwg_round_trip {
    use super::*;

    #[test]
    fn every_release_begins_with_its_own_version_string() {
        // A DWG's first six bytes are its version string, whatever the release: the format's
        // own definition, which acadrust's reader dispatches on.
        let model = site();
        for release in DwgRelease::ALL {
            let bytes = dwg_of(&model, release);
            assert_eq!(
                &bytes[..6],
                release.code().as_bytes(),
                "{}",
                release.name()
            );
        }
    }

    #[test]
    fn every_kind_comes_back_as_the_geometry_it_was_in_every_release() {
        let model = site();
        for release in DwgRelease::ALL {
            let name = release.name();
            let back = round_trip(&model, release);
            assert_eq!(back.entities.len(), 7, "{name}");
            assert_eq!(back.release, name);
            assert_eq!(back.version, release.code());

            let points = of_kind(&back, EntityType::Point);
            assert_eq!(points.len(), 1, "{name}");
            let Geometry::Point(point) = &points[0].geometry else {
                panic!("{name}: not a point");
            };
            // The shortest digits that read back as the same double go out, through acadrust's
            // DXF and the DWG's own doubles: nothing is lost to formatting.
            assert_eq!(point.position, p(502000.125, 6250000.1), "{name}");
            assert_eq!(heights_of(&points[0].properties, 1)[0], Some(31.245), "{name}");
            assert_eq!(points[0].layer, "Survey", "{name}");

            let lines = of_kind(&back, EntityType::Line);
            assert_eq!(lines.len(), 1, "{name}");
            assert_eq!(
                lines[0].geometry,
                Geometry::Line(Segment2::new(p(0.1, 0.2), p(3.0, 4.0))),
                "{name}"
            );

            let arcs = of_kind(&back, EntityType::Arc);
            assert_eq!(arcs.len(), 1, "{name}");
            let Geometry::Arc(arc) = &arcs[0].geometry else {
                panic!("{name}: not an arc");
            };
            assert_eq!(arc.center, p(650.0, 200.0), "{name}");
            assert_eq!(arc.radius, 8.0, "{name}");
            // Degrees in the DXF, radians in the DWG, degrees again: as the DXF round trip,
            // equal to a few units in the last place.
            near(arc.start_angle, 0.25, 1e-14);
            near(arc.sweep, 1.5, 1e-14);

            let circles = of_kind(&back, EntityType::Circle);
            assert_eq!(circles.len(), 1, "{name}");
            assert_eq!(
                circles[0].geometry,
                Geometry::Circle(Circle2::new(p(-7.5, 1.0 / 3.0), 0.6)),
                "{name}"
            );

            let texts = of_kind(&back, EntityType::Text);
            assert_eq!(texts.len(), 1, "{name}");
            let Geometry::Text(text) = &texts[0].geometry else {
                panic!("{name}: not a text");
            };
            assert_eq!(text.text, "RL 32.45° Böschung", "{name}");
            assert_eq!(text.position, p(5.0, 6.0), "{name}");
            assert_eq!(text.height, 2.75, "{name}");
            near(text.rotation, PI / 2.0, 1e-15);

            let polylines = of_kind(&back, EntityType::Polyline);
            assert_eq!(polylines.len(), 2, "{name}");
            let open = polylines
                .iter()
                .find(|entity| entity.layer == "0")
                .expect("the open polyline");
            let Geometry::Polyline(open_line) = &open.geometry else {
                panic!("{name}: not a polyline");
            };
            assert!(!open_line.closed, "{name}");
            assert_eq!(open_line.vertices, vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0)]);
            let kerb = polylines
                .iter()
                .find(|entity| entity.layer == "Survey/Kerb")
                .expect("the kerb");
            let Geometry::Polyline(kerb_line) = &kerb.geometry else {
                panic!("{name}: not a polyline");
            };
            assert!(kerb_line.closed, "{name}");
            assert_eq!(
                heights_of(&kerb.properties, 3),
                vec![Some(10.0), Some(11.0), Some(12.5)],
                "{name}"
            );
        }
    }

    #[test]
    fn layers_keep_their_paths_colours_visibility_lock_weight_and_linetype_in_every_release() {
        let model = site();
        for release in DwgRelease::ALL {
            let name = release.name();
            let back = round_trip(&model, release);
            let survey = layer_named(&back, "Survey").expect("Survey");
            assert_eq!(survey.color, rgb(255, 0, 0), "{name}");
            assert!(survey.visible, "{name}");
            // The path "Survey/Kerb" is "Survey$Kerb" in the drawing, its path beside it.
            let kerb = layer_named(&back, "Survey/Kerb").expect("Survey/Kerb");
            // No index is (10, 200, 30): from R2004 it is the layer's own colour, before that
            // the nearest index with the colour beside it - Katana reads it back either way.
            assert_eq!(kerb.color, rgb(10, 200, 30), "{name}");
            assert!(!kerb.visible, "{name}");
            assert!(kerb.locked, "{name}");
            assert_eq!(kerb.line_weight, 0.5, "{name}");
            assert_eq!(kerb.linetype, "kerb dash", "{name}");
            assert_eq!(back.linetypes.len(), 1, "{name}");
            assert_eq!(back.linetypes[0].name, "kerb dash", "{name}");
            assert_eq!(
                back.linetypes[0].pattern,
                vec![
                    LinetypeElement { length: 2.0 },
                    LinetypeElement { length: -0.5 },
                    LinetypeElement { length: 0.0 },
                    LinetypeElement { length: -0.5 },
                ],
                "{name}"
            );
        }
    }

    #[test]
    fn a_colour_no_index_is_comes_back_exact_on_an_entity_in_every_release() {
        // (250, 5, 5) is no indexed colour; the nearest is 1, red.
        let mut model = Model::default();
        let entity = Entity {
            geometry: Geometry::Line(Segment2::new(p(0.0, 0.0), p(1.0, 0.0))),
            color: Some(rgb(250, 5, 5)),
            ..Entity::default()
        };
        assert!(model.entities.add(entity).is_ok());
        for release in DwgRelease::ALL {
            let back = round_trip(&model, release);
            assert_eq!(back.entities[0].color, Some(rgb(250, 5, 5)), "{}", release.name());
        }
    }

    #[test]
    fn text_in_a_script_the_code_page_does_not_hold_comes_back_from_every_release() {
        // Before R2007 a DWG's text is in its code page, Windows-1252 here, which holds neither
        // the Greek nor the Japanese: acadrust writes those as \U+XXXX and reads them back.
        // From R2007 the text is UTF-16 and holds them as they are.
        let mut model = Model::default();
        add(
            &mut model,
            TextGeometry::new(p(1.0, 2.0), "Ω 測量 é", 1.5, 0.0),
            "0",
            &[],
        );
        for release in DwgRelease::ALL {
            let back = round_trip(&model, release);
            let Geometry::Text(text) = &back.entities[0].geometry else {
                panic!("{}: not a text", release.name());
            };
            assert_eq!(text.text, "Ω 測量 é", "{}", release.name());
        }
    }

    #[test]
    fn survey_size_and_enormous_coordinates_come_back_exactly() {
        // Finite numbers, however large, are numbers: 4e15 + 0.5 and -1e12 - 0.25 are each a
        // double exactly (their units in the last place are 0.5 and 2^-13), written in full
        // and read back bit for bit.
        let mut model = Model::default();
        add(
            &mut model,
            PointGeometry {
                position: p(4.0e15 + 0.5, -1.0e12 - 0.25),
            },
            "0",
            &[],
        );
        add(
            &mut model,
            Segment2::new(p(6_250_000.125, 302_000.062_5), p(6_250_010.5, 302_020.75)),
            "0",
            &[],
        );
        let back = round_trip(&model, DwgRelease::DEFAULT);
        let Geometry::Point(point) = &back.entities[0].geometry else {
            panic!("not a point");
        };
        assert_eq!(point.position, p(4.0e15 + 0.5, -1.0e12 - 0.25));
        assert_eq!(
            back.entities[1].geometry,
            Geometry::Line(Segment2::new(
                p(6_250_000.125, 302_000.062_5),
                p(6_250_010.5, 302_020.75)
            ))
        );
    }

    #[test]
    fn heights_known_at_only_some_vertices_come_back_with_their_gaps() {
        // The format has no "no height": such a polyline goes out in plan, its heights beside it
        // as extended data, which the DWG keeps and Katana reads back.
        let mut model = Model::default();
        add(
            &mut model,
            Polyline2::new(vec![p(0.0, 0.0), p(5.0, 0.0), p(5.0, 5.0)], false),
            "0",
            &[Some(3.25), None, Some(4.5)],
        );
        for release in DwgRelease::ALL {
            let back = round_trip(&model, release);
            assert_eq!(
                heights_of(&back.entities[0].properties, 3),
                vec![Some(3.25), None, Some(4.5)],
                "{}",
                release.name()
            );
        }
    }

    #[test]
    fn an_empty_drawing_is_written_and_reads_back_as_nothing_said_so() {
        let back = round_trip(&Model::default(), DwgRelease::DEFAULT);
        assert!(back.entities.is_empty());
        assert!(warned(&back, "nothing was imported"), "{:?}", back.warnings);
    }

    #[test]
    fn a_drawing_read_back_is_read_with_the_release_it_was_written_as() {
        let back = read(&dwg_of(&site(), DwgRelease::R2010));
        assert_eq!(back.version, "AC1024");
        assert_eq!(back.release, "R2010");
    }
}
