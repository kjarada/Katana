//! The reader (docs/dgn.md, "Reading"): designs built byte by byte in the
//! test (support/mod.rs), every expected value worked out beside its check.

pub mod support;

use katana_core::ErrorCode;
use katana_dgn::codes::DEFAULT_COLOURS;
use katana_dgn::{
    Content, DgnImport, ImportOptions, META_CELL, META_FONT, META_JUSTIFICATION, META_STYLE,
    META_WEIGHT, identify, is_dgn_path, level_layer_name, level_of_layer_name, read_dgn,
};
use katana_entity::entity::heights_of;
use katana_entity::{Color, Entity, Geometry, PropertyValue};
use katana_geometry::{Point2, Vec2};
use katana_math::numerics::{DEG_TO_RAD, PI};
use support::{Design, Look, member, near};

fn read(bytes: &[u8]) -> DgnImport {
    match read_dgn(bytes, &ImportOptions::default()) {
        Ok(imported) => imported,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn refused(bytes: &[u8]) -> katana_core::Error {
    match read_dgn(bytes, &ImportOptions::default()) {
        Ok(imported) => panic!("read {} entities from what is no design", imported.entities.len()),
        Err(error) => error,
    }
}

fn colour(index: u8) -> Look {
    Look {
        colour: index,
        ..Look::default()
    }
}

fn tally(imported: &DgnImport, kind: &str) -> (usize, usize) {
    imported
        .tally
        .iter()
        .find(|tally| tally.kind == kind)
        .map_or((0, 0), |tally| (tally.read, tally.imported))
}

fn said(imported: &DgnImport, part: &str) -> bool {
    imported.warnings.iter().any(|warning| warning.contains(part))
}

fn metadata(entity: &Entity, key: &str) -> Option<PropertyValue> {
    entity.metadata.get(key).cloned()
}

/// The steps of an angle: 1/360000 degree.
fn steps(degrees: f64) -> i32 {
    (degrees * 360_000.0).round() as i32
}

mod elements {
    use super::*;

    #[test]
    fn lines_line_strings_and_shapes_come_in_on_their_levels_at_their_coordinates() {
        // The global origin of an MGA site: 330 000 m E and 6 250 000 m N at
        // 10 000 units of resolution a metre, so the plane's 0,0 is there and
        // an integer coordinate u is 330 000 + u / 10 000 metres - read as
        // one division, (u - origin) / 10 000, the nearest double to the
        // decimal it is.
        let mut design = Design::new(false, 1000, 10, [-3.3e9, -6.25e10, 0.0]);
        let line = design.line(7, colour(3), (1234, -5678, 0), (21234, 4322, 0));
        let string = design.points(4, 12, colour(1), &[(0, 0, 0), (100_000, 0, 0), (100_000, 50_000, 0)]);
        let shape = design.points(
            6,
            63,
            colour(2),
            &[(0, 0, 0), (10_000, 0, 0), (10_000, 10_000, 0), (0, 10_000, 0), (0, 0, 0)],
        );
        design.add(line).add(string).add(shape);
        let imported = read(&design.finish());
        assert_eq!(imported.format(), "DGN V7 2D");
        assert_eq!(imported.units.per_master(), 10_000.0);
        assert_eq!(imported.entities.len(), 3);

        let Geometry::Line(segment) = &imported.entities[0].geometry else {
            panic!("{:?}", imported.entities[0].geometry)
        };
        assert_eq!(segment.start, Point2::new(330_000.123_4, 6_249_999.432_2));
        assert_eq!(segment.end, Point2::new(330_002.123_4, 6_250_000.432_2));
        assert_eq!(imported.entities[0].layer, "level 7");
        // Colour 3 of MicroStation's table, the file having none of its own.
        assert_eq!(imported.entities[0].color, Some(Color::new(255, 0, 0, 255)));

        let Geometry::Polyline(polyline) = &imported.entities[1].geometry else {
            panic!("{:?}", imported.entities[1].geometry)
        };
        assert!(!polyline.closed);
        assert_eq!(
            polyline.vertices,
            vec![
                Point2::new(330_000.0, 6_250_000.0),
                Point2::new(330_010.0, 6_250_000.0),
                Point2::new(330_010.0, 6_250_005.0)
            ]
        );
        assert_eq!(imported.entities[1].layer, "level 12");

        // A shape repeats its first vertex last; the polyline closes instead.
        let Geometry::Polyline(square) = &imported.entities[2].geometry else {
            panic!("{:?}", imported.entities[2].geometry)
        };
        assert!(square.closed);
        assert_eq!(square.vertices.len(), 4);
        near(square.area(), 1.0, 1e-9);
        assert_eq!(imported.entities[2].layer, "level 63");

        let layers: Vec<&str> = imported.layers.iter().map(|layer| layer.name.as_str()).collect();
        assert_eq!(layers, ["level 7", "level 12", "level 63"]);
        assert_eq!(tally(&imported, "Line"), (1, 1));
        assert_eq!(tally(&imported, "Line String"), (1, 1));
        assert_eq!(tally(&imported, "Shape"), (1, 1));
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    }

    #[test]
    fn a_line_of_no_length_is_a_point_and_a_line_string_of_two_stays_a_polyline() {
        let mut design = Design::metric(false);
        let dot = design.line(1, colour(0), (50_000, 20_000, 0), (50_000, 20_000, 0));
        let pair = design.points(4, 1, colour(0), &[(0, 0, 0), (10_000, 0, 0)]);
        design.add(dot).add(pair);
        let imported = read(&design.finish());
        assert_eq!(
            imported.entities[0].geometry,
            Geometry::Point(katana_entity::PointGeometry {
                position: Point2::new(5.0, 2.0)
            })
        );
        let Geometry::Polyline(polyline) = &imported.entities[1].geometry else {
            panic!("{:?}", imported.entities[1].geometry)
        };
        assert_eq!(polyline.vertices, vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)]);
    }

    #[test]
    fn arcs_and_ellipses_come_in_as_true_curves() {
        // At 10 000 units a metre: a radius of 5 m is 50 000 units, the
        // centre (100 m, 200 m) is (1e6, 2e6).
        let mut design = Design::metric(false);
        let quarter = design.arc(1, colour(0), steps(30.0), steps(90.0), (50_000.0, 50_000.0), &[0], (1e6, 2e6, 0.0));
        let clockwise = design.arc(1, colour(0), steps(30.0), -steps(90.0), (50_000.0, 50_000.0), &[0], (1e6, 2e6, 0.0));
        // A sweep of 0 is a whole turn (dgnread.cpp:682).
        let whole = design.arc(1, colour(0), 0, 0, (50_000.0, 50_000.0), &[0], (1e6, 2e6, 0.0));
        // An ellipse 10 m by 4 m turned 30 degrees, and one whose secondary
        // axis is the longer, which is the major one a quarter turn on.
        let flat = design.ellipse(1, colour(0), (100_000.0, 40_000.0), &[steps(30.0)], (0.0, 0.0, 0.0));
        let tall = design.ellipse(1, colour(0), (40_000.0, 100_000.0), &[steps(30.0)], (0.0, 0.0, 0.0));
        let circle = design.ellipse(1, colour(0), (20_000.0, 20_000.0), &[0], (30_000.0, 0.0, 0.0));
        design.add(quarter).add(clockwise).add(whole).add(flat).add(tall).add(circle);
        let imported = read(&design.finish());
        assert_eq!(imported.entities.len(), 6, "{:?}", imported.warnings);

        let Geometry::Arc(arc) = &imported.entities[0].geometry else {
            panic!("{:?}", imported.entities[0].geometry)
        };
        assert_eq!(arc.center, Point2::new(100.0, 200.0));
        assert_eq!(arc.radius, 5.0);
        near(arc.start_angle, PI / 6.0, 1e-12);
        near(arc.sweep, PI / 2.0, 1e-12);
        let Geometry::Arc(back) = &imported.entities[1].geometry else {
            panic!("{:?}", imported.entities[1].geometry)
        };
        near(back.sweep, -PI / 2.0, 1e-12);
        assert!(matches!(imported.entities[2].geometry, Geometry::Circle(_)));

        let Geometry::Ellipse(flat) = &imported.entities[3].geometry else {
            panic!("{:?}", imported.entities[3].geometry)
        };
        near(flat.major_axis.x, 10.0 * (30.0 * DEG_TO_RAD).cos(), 1e-12);
        near(flat.major_axis.y, 10.0 * (30.0 * DEG_TO_RAD).sin(), 1e-12);
        near(flat.ratio, 0.4, 1e-15);
        let Geometry::Ellipse(tall) = &imported.entities[4].geometry else {
            panic!("{:?}", imported.entities[4].geometry)
        };
        // The secondary axis runs a quarter turn on from 30 degrees: 120.
        near(tall.major_axis.x, 10.0 * (120.0 * DEG_TO_RAD).cos(), 1e-12);
        near(tall.major_axis.y, 10.0 * (120.0 * DEG_TO_RAD).sin(), 1e-12);
        near(tall.ratio, 0.4, 1e-15);
        let Geometry::Circle(circle) = &imported.entities[5].geometry else {
            panic!("{:?}", imported.entities[5].geometry)
        };
        assert_eq!(circle.center, Point2::new(3.0, 0.0));
        assert_eq!(circle.radius, 2.0);
    }

    #[test]
    fn a_text_comes_in_at_its_origin_with_its_height_rotation_font_and_justification() {
        // At 1000 units a metre, a 3 m height is 3 x 1000 / (6 / 1000) =
        // 500 000 of the format's text steps; 45 degrees is 16 200 000 of its
        // angle steps. "Caf\xe9" is Windows-1252, MicroStation's for Western
        // text.
        let mut design = Design::new(false, 1000, 1, [0.0; 3]);
        let text = design.text(4, colour(0), 3, 7, 500_000, &[steps(45.0)], (12_000, 34_000, 0), b"Caf\xe9");
        design.add(text);
        let imported = read(&design.finish());
        let Geometry::Text(text) = &imported.entities[0].geometry else {
            panic!("{:?}", imported.entities[0].geometry)
        };
        assert_eq!(text.text, "Caf\u{e9}");
        assert_eq!(text.position, Point2::new(12.0, 34.0));
        assert_eq!(text.height, 3.0);
        near(text.rotation, PI / 4.0, 1e-15);
        assert_eq!(metadata(&imported.entities[0], META_FONT), Some(PropertyValue::Integer(3)));
        assert_eq!(
            metadata(&imported.entities[0], META_JUSTIFICATION),
            Some(PropertyValue::Integer(7))
        );
    }

    #[test]
    fn a_complex_chain_of_a_line_string_and_an_arc_is_one_curve_polyline() {
        // (0,0) to (10,0), then the half circle about (10,5) from -90 degrees
        // through 180 to (10,10): one polyline, the arc its middle vertex's
        // bulge, tan(180 / 4 degrees) = 1.
        let mut design = Design::new(false, 1, 1, [0.0; 3]);
        let straight = design.points(4, 2, member(colour(3)), &[(0, 0, 0), (10, 0, 0)]);
        let bend = design.arc(2, member(colour(3)), steps(-90.0), steps(180.0), (5.0, 5.0), &[0], (10.0, 5.0, 0.0));
        let header = design.complex_header(12, 2, colour(3), &[straight.clone(), bend.clone()]);
        design.add(header).add(straight).add(bend);
        let imported = read(&design.finish());
        assert_eq!(imported.entities.len(), 1, "{:?}", imported.warnings);
        let Geometry::CurvePolyline(chain) = &imported.entities[0].geometry else {
            panic!("{:?}", imported.entities[0].geometry)
        };
        assert!(!chain.closed);
        assert_eq!(chain.vertices.len(), 3);
        assert_eq!(chain.vertices[0].position, Point2::new(0.0, 0.0));
        assert_eq!(chain.vertices[1].position, Point2::new(10.0, 0.0));
        near(chain.vertices[1].bulge, 1.0, 1e-12);
        near(chain.vertices[2].position.x, 10.0, 1e-12);
        near(chain.vertices[2].position.y, 10.0, 1e-12);
        assert_eq!(tally(&imported, "Complex Chain"), (1, 1));
        assert_eq!(imported.entities[0].layer, "level 2");
    }

    #[test]
    fn a_complex_shape_of_line_strings_is_one_closed_polyline() {
        let mut design = Design::new(false, 1, 1, [0.0; 3]);
        let first = design.points(4, 1, member(colour(0)), &[(0, 0, 0), (20, 0, 0), (20, 10, 0)]);
        let second = design.points(4, 1, member(colour(0)), &[(20, 10, 0), (0, 10, 0), (0, 0, 0)]);
        let header = design.complex_header(14, 1, colour(0), &[first.clone(), second.clone()]);
        design.add(header).add(first).add(second);
        let imported = read(&design.finish());
        let Geometry::Polyline(ring) = &imported.entities[0].geometry else {
            panic!("{:?}", imported.entities[0].geometry)
        };
        assert!(ring.closed);
        assert_eq!(ring.vertices.len(), 4);
        near(ring.area(), 200.0, 1e-12);
    }

    #[test]
    fn a_cells_components_carry_its_name_and_its_hole_is_tagged_with_its_area() {
        // MicroStation's grouped hole: a cell holding an area and a shape
        // marked as a hole of it (property 0x8000). The cell is "TREE1":
        // "TRE" and "E1 " in RADIX-50, 32 725 and 9 240 (tests/codes.rs).
        let mut design = Design::new(false, 1, 1, [0.0; 3]);
        let area = design.points(6, 5, member(colour(2)), &[(0, 0, 0), (10, 0, 0), (10, 10, 0), (0, 10, 0), (0, 0, 0)]);
        let hole = design.points(
            6,
            5,
            member(Look { properties: 0x8000, ..colour(2) }),
            &[(4, 4, 0), (6, 4, 0), (6, 6, 0), (4, 6, 0), (4, 4, 0)],
        );
        let cell = design.cell_header(5, (32_725, 9_240), &[area.clone(), hole.clone()]);
        let after = design.line(6, colour(0), (0, 0, 0), (1, 1, 0));
        design.add(cell).add(area).add(hole).add(after);
        let imported = read(&design.finish());
        assert_eq!(imported.entities.len(), 3, "{:?}", imported.warnings);
        let ring = |entity: &Entity| metadata(entity, "source.ring");
        assert_eq!(metadata(&imported.entities[0], META_CELL), Some(PropertyValue::from("TREE1")));
        assert_eq!(ring(&imported.entities[0]), Some(PropertyValue::from("exterior")));
        assert_eq!(ring(&imported.entities[1]), Some(PropertyValue::from("hole")));
        let part = metadata(&imported.entities[0], "source.part");
        assert!(part.is_some());
        assert_eq!(metadata(&imported.entities[1], "source.part"), part);
        // The line after the cell is no component of it.
        assert_eq!(metadata(&imported.entities[2], META_CELL), None);
        assert_eq!(ring(&imported.entities[2]), None);
        assert_eq!(tally(&imported, "Cell"), (1, 1));
    }

    #[test]
    fn style_and_weight_are_kept_beside_the_entity() {
        let mut design = Design::metric(false);
        let dashed = design.line(1, Look { style: 3, weight: 5, ..colour(1) }, (0, 0, 0), (10, 0, 0));
        design.add(dashed);
        let imported = read(&design.finish());
        assert_eq!(metadata(&imported.entities[0], META_STYLE), Some(PropertyValue::Integer(3)));
        assert_eq!(metadata(&imported.entities[0], META_WEIGHT), Some(PropertyValue::Integer(5)));
    }
}

mod design {
    use super::*;

    #[test]
    fn the_files_colour_table_names_the_colours_and_its_last_one_wins() {
        let mut first = DEFAULT_COLOURS;
        first[3] = [10, 20, 30];
        let mut second = DEFAULT_COLOURS;
        second[3] = [40, 50, 60];
        let mut design = Design::metric(false);
        let one = design.colour_table(&first);
        let line = design.line(1, colour(3), (0, 0, 0), (10, 0, 0));
        design.add(one).add(line);
        let imported = read(&design.finish());
        assert!(imported.colour_table);
        assert_eq!(imported.entities[0].color, Some(Color::new(10, 20, 30, 255)));
        let two = design.colour_table(&second);
        design.add(two);
        let imported = read(&design.finish());
        assert_eq!(imported.entities[0].color, Some(Color::new(40, 50, 60, 255)));
    }

    #[test]
    fn a_3d_designs_heights_are_kept_and_an_element_all_at_zero_has_none() {
        // At 10 000 units a metre: 105 000 is 10.5 m, 112 500 is 11.25 m.
        let mut design = Design::metric(true);
        let climbing = design.points(4, 1, colour(0), &[(0, 0, 105_000), (10_000, 0, 112_500)]);
        let flat = design.line(1, colour(0), (0, 0, 0), (10_000, 10_000, 0));
        let arc = design.arc(1, colour(0), 0, steps(90.0), (10_000.0, 10_000.0), &[i32::MAX, 0, 0, 0], (0.0, 0.0, 30_000.0));
        design.add(climbing).add(flat).add(arc);
        let imported = read(&design.finish());
        assert_eq!(imported.format(), "DGN V7 3D");
        assert_eq!(
            heights_of(&imported.entities[0].properties, 2),
            vec![Some(10.5), Some(11.25)]
        );
        assert_eq!(heights_of(&imported.entities[1].properties, 2), vec![None, None]);
        // An arc in plan at 3 m: both its ends.
        assert_eq!(
            heights_of(&imported.entities[2].properties, 2),
            vec![Some(3.0), Some(3.0)]
        );
    }

    #[test]
    fn a_tilted_arc_is_not_read_and_is_said() {
        // An eighth of a turn about X tilts the arc's plane 45 degrees.
        let half = ((22.5 * DEG_TO_RAD).cos() * f64::from(i32::MAX)).round() as i32;
        let side = ((22.5 * DEG_TO_RAD).sin() * f64::from(i32::MAX)).round() as i32;
        let mut design = Design::metric(true);
        let arc = design.arc(1, colour(0), 0, steps(90.0), (10_000.0, 10_000.0), &[half, side, 0, 0], (0.0, 0.0, 0.0));
        design.add(arc);
        let imported = read(&design.finish());
        assert!(imported.entities.is_empty());
        assert!(said(&imported, "tilted"), "{:?}", imported.warnings);
    }

    #[test]
    fn deleted_elements_are_passed_over_and_counted() {
        let mut design = Design::metric(false);
        let gone = design.line(1, Look { deleted: true, ..colour(0) }, (0, 0, 0), (10, 0, 0));
        let kept = design.line(1, colour(0), (0, 0, 0), (0, 10, 0));
        design.add(gone).add(kept);
        let imported = read(&design.finish());
        assert_eq!(imported.entities.len(), 1);
        assert_eq!(tally(&imported, "Deleted"), (1, 0));
    }

    #[test]
    fn what_is_not_read_is_tallied_and_said() {
        // A B-spline curve header (type 27) and a reference attachment (group
        // data on level 9) - the bytes after the display header do not
        // matter, as neither is read.
        let mut design = Design::metric(false);
        design.add(support::element(27, 1, colour(0), &[0; 12]));
        design.add(support::element(5, 9, Look::default(), &[0; 12]));
        let imported = read(&design.finish());
        assert!(imported.entities.is_empty());
        assert_eq!(tally(&imported, "B-spline Curve"), (1, 0));
        assert!(said(&imported, "1 B-spline Curve elements are not read"), "{:?}", imported.warnings);
        assert!(said(&imported, "reference files"), "{:?}", imported.warnings);
    }

    #[test]
    fn the_origin_shift_moves_every_kind_alike() {
        let mut design = Design::new(false, 1, 1, [0.0; 3]);
        let line = design.line(1, colour(0), (100, 200, 0), (110, 200, 0));
        let arc = design.arc(1, colour(0), 0, steps(90.0), (5.0, 5.0), &[0], (100.0, 200.0, 0.0));
        let text = design.text(1, colour(0), 1, 2, 1000, &[0], (105, 205, 0), b"A");
        design.add(line).add(arc).add(text);
        let options = ImportOptions {
            origin_shift: Some(Vec2::new(100.0, 200.0)),
            ..ImportOptions::default()
        };
        let imported = read_dgn(&design.finish(), &options).expect("read");
        let Geometry::Line(segment) = &imported.entities[0].geometry else {
            panic!()
        };
        assert_eq!(segment.start, Point2::new(0.0, 0.0));
        let Geometry::Arc(arc) = &imported.entities[1].geometry else {
            panic!()
        };
        assert_eq!(arc.center, Point2::new(0.0, 0.0));
        let Geometry::Text(text) = &imported.entities[2].geometry else {
            panic!()
        };
        assert_eq!(text.position, Point2::new(5.0, 5.0));
    }

    #[test]
    fn a_design_with_no_working_units_is_read_in_units_of_resolution_and_says_so() {
        let mut design = Design::new(false, 0, 0, [0.0; 3]);
        let line = design.line(1, colour(0), (0, 0, 0), (7, 0, 0));
        design.add(line);
        let imported = read(&design.finish());
        let Geometry::Line(segment) = &imported.entities[0].geometry else {
            panic!()
        };
        assert_eq!(segment.end, Point2::new(7.0, 0.0));
        assert!(said(&imported, "no working units"), "{:?}", imported.warnings);
    }

    #[test]
    fn what_the_drawing_cannot_hold_is_left_out_and_counted() {
        // An arc of no radius, and a text of no characters.
        let mut design = Design::metric(false);
        let dot = design.arc(1, colour(0), 0, steps(90.0), (0.0, 0.0), &[0], (0.0, 0.0, 0.0));
        let empty = design.text(1, colour(0), 1, 2, 1000, &[0], (0, 0, 0), b"");
        let line = design.line(1, colour(0), (0, 0, 0), (10, 0, 0));
        design.add(dot).add(empty).add(line);
        let imported = read(&design.finish());
        assert_eq!(imported.entities.len(), 1);
        assert!(said(&imported, "2 elements had geometry a drawing cannot hold"), "{:?}", imported.warnings);
    }
}

mod damaged {
    use super::*;

    fn two_lines() -> Vec<u8> {
        let mut design = Design::metric(false);
        let first = design.line(1, colour(0), (0, 0, 0), (10_000, 0, 0));
        let second = design.line(1, colour(0), (0, 0, 0), (0, 10_000, 0));
        design.add(first).add(second);
        design.finish()
    }

    #[test]
    fn a_file_cut_inside_an_element_keeps_what_came_before_and_says_so() {
        let whole = two_lines();
        // The second line is the last 52 bytes before FF FF: cut inside it.
        let cut = &whole[..whole.len() - 2 - 20];
        let imported = read(cut);
        assert_eq!(imported.entities.len(), 1);
        assert!(said(&imported, "cut short"), "{:?}", imported.warnings);
    }

    #[test]
    fn a_file_without_its_end_word_is_read_and_says_so() {
        let whole = two_lines();
        let imported = read(&whole[..whole.len() - 2]);
        assert_eq!(imported.entities.len(), 2);
        assert!(said(&imported, "end-of-design"), "{:?}", imported.warnings);
    }

    #[test]
    fn a_header_cut_short_is_refused() {
        let whole = two_lines();
        let error = refused(&whole[..100]);
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(error.message.contains("design header"), "{}", error.message);
    }
}

mod refusals {
    use super::*;

    #[test]
    fn a_v8_design_is_refused_by_name_with_what_to_do() {
        // An OLE compound document's signature ([MS-CFB] 2.2), as every V8
        // design begins.
        let mut v8 = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        v8.resize(2048, 0);
        assert_eq!(identify(&v8), Content::CompoundDocument);
        let error = refused(&v8);
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error.message.contains("V8"), "{}", error.message);
        assert!(error.message.contains("save it as V7"), "{}", error.message);
        assert!(error.message.contains("DXF"), "{}", error.message);
    }

    #[test]
    fn an_empty_file_is_refused_as_empty() {
        let error = refused(&[]);
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(error.message.contains("empty"), "{}", error.message);
    }

    #[test]
    fn a_dxf_or_a_dwg_named_dgn_is_refused_by_name() {
        let dxf = b"  0\nSECTION\n  2\nENTITIES\n  0\nENDSEC\n  0\nEOF\n";
        assert_eq!(identify(dxf), Content::Dxf);
        let error = refused(dxf);
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error.message.contains("DXF file named .dgn"), "{}", error.message);
        let mut dwg = b"AC1032".to_vec();
        dwg.resize(1600, 0);
        assert_eq!(identify(&dwg), Content::Dwg);
        assert!(refused(&dwg).message.contains("DWG file named .dgn"));
    }

    #[test]
    fn a_cell_library_and_a_file_that_is_no_design_are_refused() {
        let mut library = vec![0x08, 0x05, 0x17, 0x00];
        library.resize(1600, 0);
        assert!(refused(&library).message.contains("cell library"));
        let error = refused(b"hello, world");
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(error.message.contains("not a DGN file"), "{}", error.message);
    }

    #[test]
    fn options_the_reader_cannot_work_with_are_refused() {
        let design = Design::metric(false).finish();
        for options in [
            ImportOptions {
                curve_tolerance: 0.0,
                ..ImportOptions::default()
            },
            ImportOptions {
                curve_tolerance: f64::NAN,
                ..ImportOptions::default()
            },
            ImportOptions {
                origin_shift: Some(Vec2::new(f64::INFINITY, 0.0)),
                ..ImportOptions::default()
            },
        ] {
            let error = read_dgn(&design, &options).expect_err("refused");
            assert_eq!(error.code, ErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn an_empty_design_reads_as_nothing_and_says_nothing() {
        let imported = read(&Design::metric(false).finish());
        assert!(imported.entities.is_empty());
        assert!(imported.layers.is_empty());
        assert!(imported.bounds.empty());
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    }
}

mod names {
    use super::*;

    #[test]
    fn a_dgn_path_is_known_by_its_extension_in_any_case() {
        assert!(is_dgn_path(std::path::Path::new("C:/jobs/site.dgn")));
        assert!(is_dgn_path(std::path::Path::new("SITE.DGN")));
        assert!(!is_dgn_path(std::path::Path::new("site.dxf")));
        assert!(!is_dgn_path(std::path::Path::new("dgn")));
    }

    #[test]
    fn a_level_is_a_layer_named_for_it_and_back() {
        assert_eq!(level_layer_name(7), "level 7");
        assert_eq!(level_of_layer_name("level 7"), Some(7));
        assert_eq!(level_of_layer_name("level 63"), Some(63));
        assert_eq!(level_of_layer_name("level 64"), None);
        assert_eq!(level_of_layer_name("level"), None);
        assert_eq!(level_of_layer_name("Level 7"), None);
        assert_eq!(level_of_layer_name("survey/level 7"), None);
    }
}
