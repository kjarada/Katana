//! The twin of tests/dxf/test_reader.cpp: the two hand-written fixtures, an
//! R12 and an R2000 drawing, and drawings written in the test, each value
//! worked out beside its check.

#![allow(
    clippy::excessive_precision,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]

pub mod support;

use std::path::Path;

use katana_dxf::{
    DxfImport, EntityTally, ImportOptions, META_BLOCK, META_LINEWEIGHT, is_dxf_path, read_dxf,
    read_dxf_file,
};
use katana_entity::entity::{
    ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, heights_of, property_to_string,
};
use katana_entity::{
    Color, Entity, EntityType, Geometry, Layer, PropertyValue, TextGeometry, validate,
};
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, CurveSegment, Point2, Polyline2, Segment2, Vec2,
};
use katana_math::numerics::PI;
use support::{data, near};

fn load(name: &str, options: &ImportOptions) -> DxfImport {
    match read_dxf_file(&data(name), options) {
        Ok(imported) => imported,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn read(text: &str) -> DxfImport {
    match read_dxf(text, &ImportOptions::default()) {
        Ok(imported) => imported,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn of_kind(imported: &DxfImport, kind: EntityType) -> Vec<&Entity> {
    imported
        .entities
        .iter()
        .filter(|entity| entity.r#type() == kind)
        .collect()
}

fn text_saying<'a>(imported: &'a DxfImport, text: &str) -> Option<&'a Entity> {
    imported
        .entities
        .iter()
        .find(|entity| matches!(&entity.geometry, Geometry::Text(shape) if shape.text == text))
}

fn layer_named<'a>(imported: &'a DxfImport, name: &str) -> Option<&'a Layer> {
    imported.layers.iter().find(|layer| layer.name == name)
}

fn tally_of<'a>(imported: &'a DxfImport, kind: &str) -> Option<&'a EntityTally> {
    imported.tally.iter().find(|tally| tally.kind == kind)
}

fn any_warning_contains(imported: &DxfImport, text: &str) -> bool {
    imported
        .warnings
        .iter()
        .any(|warning| warning.contains(text))
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b, 255)
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn arc_of(entity: &Entity) -> &Arc2 {
    match &entity.geometry {
        Geometry::Arc(arc) => arc,
        other => panic!("not an arc: {other:?}"),
    }
}

fn circle_of(entity: &Entity) -> &Circle2 {
    match &entity.geometry {
        Geometry::Circle(circle) => circle,
        other => panic!("not a circle: {other:?}"),
    }
}

fn segment_of(entity: &Entity) -> &Segment2 {
    match &entity.geometry {
        Geometry::Line(segment) => segment,
        other => panic!("not a line: {other:?}"),
    }
}

fn polyline_of(entity: &Entity) -> &Polyline2 {
    match &entity.geometry {
        Geometry::Polyline(polyline) => polyline,
        other => panic!("not a polyline: {other:?}"),
    }
}

fn curve_polyline_of(entity: &Entity) -> &CurvePolyline2 {
    match &entity.geometry {
        Geometry::CurvePolyline(polyline) => polyline,
        other => panic!("not a curve polyline: {other:?}"),
    }
}

fn text_of(entity: &Entity) -> &TextGeometry {
    match &entity.geometry {
        Geometry::Text(text) => text,
        other => panic!("not a text: {other:?}"),
    }
}

fn point_of(entity: &Entity) -> Point2 {
    match &entity.geometry {
        Geometry::Point(point) => point.position,
        other => panic!("not a point: {other:?}"),
    }
}

fn arc_segment(polyline: &CurvePolyline2, index: usize) -> Arc2 {
    match polyline.segment(index) {
        CurveSegment::Arc(arc) => arc,
        other => panic!("segment {index} is not an arc: {other:?}"),
    }
}

// The heights of an imported entity, one a vertex (two for a line or an arc).
fn heights(entity: &Entity, count: usize) -> Vec<Option<f64>> {
    heights_of(&entity.properties, count)
}

fn has_no_height(entity: &Entity) -> bool {
    !entity.properties.contains_key(ELEVATION_PROPERTY)
        && !entity.properties.contains_key(ELEVATIONS_PROPERTY)
}

// ---- R12 ---------------------------------------------------------------------------

mod dxf_reader_r12 {
    use super::*;

    fn survey() -> DxfImport {
        load("r12_survey.dxf", &ImportOptions::default())
    }

    #[test]
    fn the_file_is_read_with_every_entity_it_draws() {
        let imported = survey();
        assert_eq!(imported.version, "AC1009");
        assert_eq!(imported.release, "R12");
        // LINE 1, ARC 1, CIRCLE 1, POLYLINE 2, POINT 1, TEXT 2, and the INSERT's
        // circle, line and visible attribute: 11. The SOLID is not imported.
        assert_eq!(imported.entities.len(), 11);
    }

    #[test]
    fn an_arc_stays_an_arc_with_its_centre_radius_and_sweep() {
        let imported = survey();
        let arcs = of_kind(&imported, EntityType::Arc);
        assert_eq!(arcs.len(), 1);
        let arc = arc_of(arcs[0]);
        assert_eq!(arc.center, p(100.0, 200.0));
        assert_eq!(arc.radius, 1000.0);
        assert_eq!(arc.start_angle, 0.0);
        // 30 degrees; its length is 1000 * pi / 6 = 523.599, which GDAL's
        // nine chords made 523.505.
        near(arc.sweep, PI / 6.0, 1e-15);
        near(arc.length(), 523.5987755982988, 1e-9);
    }

    #[test]
    fn a_circle_is_a_circle_not_an_open_polyline() {
        let imported = survey();
        let circles = of_kind(&imported, EntityType::Circle);
        // The drawing's own circle, and the tree block's circle.
        assert_eq!(circles.len(), 2);
        let own = circles
            .iter()
            .find(|entity| circle_of(entity).center == p(5.0, 5.0))
            .expect("the drawing's own circle");
        assert_eq!(circle_of(own).radius, 0.6);
    }

    #[test]
    fn text_keeps_its_string_height_and_rotation() {
        let imported = survey();
        let lot = text_saying(&imported, "LOT 42").expect("LOT 42");
        let text = text_of(lot);
        assert_eq!(text.position, p(20.0, 30.0));
        assert_eq!(text.height, 2.5);
        near(text.rotation, PI / 2.0, 1e-15); // 90 degrees
    }

    #[test]
    fn the_percent_codes_of_text_become_the_signs_they_name() {
        let imported = survey();
        let bearing = text_saying(&imported, "45\u{00B0} BEARING").expect("the bearing");
        assert_eq!(text_of(bearing).height, 1.8);
    }

    #[test]
    fn a_bulged_polyline_keeps_its_arc_exactly() {
        // Chorded to 1 mm until the drawing system (docs/drawing.md) gave the
        // model a polyline that holds arcs: now the arc comes in as the file
        // has it, a bulge on its vertex, and nothing is approximated.
        let imported = survey();
        let curved = of_kind(&imported, EntityType::CurvePolyline);
        assert_eq!(curved.len(), 1);
        let polyline = curve_polyline_of(curved[0]);
        assert_eq!(polyline.vertices.len(), 4);
        assert!(polyline.closed);
        assert_eq!(polyline.vertices[0].position, p(0.0, 0.0));
        assert_eq!(polyline.vertices[1].position, p(2.0, 0.0));
        assert_eq!(polyline.vertices[2].position, p(2.0, 2.0));
        assert_eq!(polyline.vertices[3].position, p(0.0, 2.0));
        // Bulge 1 from (0,0) to (2,0) is a half turn counter-clockwise: centre
        // (1,0), radius 1, bulging below the chord.
        assert_eq!(polyline.vertices[0].bulge, 1.0);
        let arc = arc_segment(polyline, 0);
        near(arc.center.distance_to(p(1.0, 0.0)), 0.0, 1e-12);
        assert!(arc.midpoint().y < 0.0);
        // The square's 4 and the half disc below it, exactly.
        near(polyline.area(), 4.0 + 0.5 * PI, 1e-12);
        assert_eq!(imported.arcs_chorded, 0);
    }

    #[test]
    fn a_three_dimensional_polyline_keeps_every_vertex_height() {
        let imported = survey();
        let found = of_kind(&imported, EntityType::Polyline)
            .into_iter()
            .rev()
            .find(|entity| !polyline_of(entity).closed)
            .expect("the open 3D polyline");
        let polyline = polyline_of(found);
        assert_eq!(polyline.vertices.len(), 3);
        assert_eq!(polyline.vertices[2], p(10.0, 10.0));
        let levels = heights_of(&found.properties, 3);
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], Some(10.0));
        assert_eq!(levels[1], Some(11.0));
        assert_eq!(levels[2], Some(12.5));
        assert_eq!(found.layer, "KERB");
    }

    #[test]
    fn a_point_keeps_its_height_as_its_elevation() {
        let imported = survey();
        let points = of_kind(&imported, EntityType::Point);
        assert_eq!(points.len(), 1);
        assert_eq!(point_of(points[0]), p(3.0, 4.0));
        assert_eq!(heights_of(&points[0].properties, 1)[0], Some(55.5));
    }

    #[test]
    fn the_layer_table_gives_each_layer_its_colour_linetype_and_visibility() {
        let imported = survey();
        assert_eq!(imported.layers.len(), 3);
        let kerb = layer_named(&imported, "KERB").expect("KERB");
        assert_eq!(kerb.color, rgb(255, 0, 0));
        assert_eq!(kerb.linetype, "DASHED");
        assert!(kerb.visible);
        // Colour -3: green, switched off.
        let hidden = layer_named(&imported, "HIDDEN").expect("HIDDEN");
        assert_eq!(hidden.color, rgb(0, 255, 0));
        assert!(!hidden.visible);
        assert_eq!(imported.linetypes.len(), 1);
        assert_eq!(imported.linetypes[0].name, "DASHED");
        assert_eq!(imported.linetypes[0].pattern.len(), 2);
        assert_eq!(imported.linetypes[0].pattern[0].length, 0.5);
        assert_eq!(imported.linetypes[0].pattern[1].length, -0.25);
    }

    #[test]
    fn a_block_insert_is_expanded_scaled_rotated_and_moved() {
        let imported = survey();
        // The tree block's base is (1,1); the insert puts it at (50,50), twice
        // the size, turned 90 degrees: p -> (50,50) + R90 (2 (p - (1,1))).
        // Its circle at (1,1) r 0.5 -> (50,50) r 1. Its line (0,1)-(2,1):
        // (-1,0) -> (-2,0) -> (0,-2) -> (50,48), and (1,0) -> (2,0) -> (0,2) -> (50,52).
        let circle = of_kind(&imported, EntityType::Circle)
            .into_iter()
            .rev()
            .find(|entity| circle_of(entity).center == p(50.0, 50.0))
            .expect("the tree's circle");
        near(circle_of(circle).radius, 1.0, 1e-15);
        let line = of_kind(&imported, EntityType::Line)
            .into_iter()
            .rev()
            .find(|entity| entity.metadata.contains_key(META_BLOCK))
            .expect("the tree's line");
        let segment = segment_of(line);
        near(segment.start.x, 50.0, 1e-12);
        near(segment.start.y, 48.0, 1e-12);
        near(segment.end.x, 50.0, 1e-12);
        near(segment.end.y, 52.0, 1e-12);
        assert_eq!(property_to_string(&line.metadata[META_BLOCK]), "TREE");
        let tally = tally_of(&imported, "INSERT").expect("the INSERT tally");
        assert_eq!(tally.read, 1);
        assert_eq!(tally.imported, 3); // the circle, the line, the visible attribute
    }

    #[test]
    fn entities_on_layer_zero_in_a_block_take_the_inserts_layer_and_by_block_its_colour() {
        let imported = survey();
        for entity in of_kind(&imported, EntityType::Circle) {
            if circle_of(entity).center == p(50.0, 50.0) {
                assert_eq!(entity.layer, "KERB"); // on 0 in the block
                assert_eq!(entity.color, Some(rgb(0, 255, 0))); // ByBlock; the insert is 3
            }
        }
        for entity in of_kind(&imported, EntityType::Line) {
            if entity.metadata.contains_key(META_BLOCK) {
                assert_eq!(entity.layer, "KERB"); // its own layer
                assert!(entity.color.is_none()); // ByLayer
            }
        }
    }

    #[test]
    fn an_inserts_attributes_are_properties_of_what_it_produces() {
        let imported = survey();
        let mut carrying = 0;
        for entity in &imported.entities {
            if !entity.properties.contains_key("SPECIES") {
                continue;
            }
            carrying += 1;
            assert_eq!(property_to_string(&entity.properties["SPECIES"]), "Gum");
            assert_eq!(property_to_string(&entity.properties["HEIGHT"]), "12");
        }
        assert_eq!(carrying, 3);
        // The visible attribute is on the drawing where the file put it; the
        // invisible one is not.
        let height = text_saying(&imported, "12").expect("the visible attribute");
        assert_eq!(text_of(height).position, p(48.0, 48.0));
        assert!(text_saying(&imported, "Gum").is_none());
    }

    #[test]
    fn unsupported_entities_are_counted_not_silently_dropped() {
        let imported = survey();
        let solid = tally_of(&imported, "SOLID").expect("the SOLID tally");
        assert_eq!(solid.read, 1);
        assert_eq!(solid.imported, 0);
        assert!(any_warning_contains(
            &imported,
            "1 x SOLID entities not imported"
        ));
    }
}

// ---- R2000 -------------------------------------------------------------------------

mod dxf_reader_r2000 {
    use super::*;

    fn site() -> DxfImport {
        load("r2000_site.dxf", &ImportOptions::default())
    }

    #[test]
    fn the_file_is_read_with_every_entity_it_draws() {
        let imported = site();
        assert_eq!(imported.release, "R2000");
        // LWPOLYLINE 1, MTEXT 3 lines, LINE 1, CIRCLE 1, ARC 1, the dimension's
        // line and text 2: 9. The HATCH, the paper space line and the insert of
        // an undefined block make nothing.
        assert_eq!(imported.entities.len(), 9);
    }

    #[test]
    fn an_lw_polyline_keeps_its_closed_flag_elevation_and_arc() {
        let imported = site();
        let polylines = of_kind(&imported, EntityType::CurvePolyline);
        assert_eq!(polylines.len(), 1);
        let polyline = curve_polyline_of(polylines[0]);
        assert!(polyline.closed);
        assert_eq!(polyline.vertices.len(), 4);
        // Bulge tan(pi/8) from (10,0) to (20,10) is a quarter turn: chord
        // 10 sqrt 2, radius (10 sqrt 2 / 4)(1 + b^2)/b = 10, centre (10,10).
        let arc = arc_segment(polyline, 1);
        near(arc.radius, 10.0, 1e-9);
        near(arc.center.distance_to(p(10.0, 10.0)), 0.0, 1e-9);
        // The 10 x 10 square and the quarter disc beside it (what the fan of
        // chords approximated when the arc was chorded): 100 + 25 pi.
        near(polyline.area(), 100.0 + 25.0 * PI, 1e-9);
        // The elevation is every vertex's height, held by the geometry.
        for vertex in &polyline.vertices {
            assert_eq!(vertex.height, Some(7.25));
        }
        assert_eq!(polylines[0].layer, "Boundary");
    }

    #[test]
    fn each_m_text_paragraph_becomes_a_text_with_its_codes_stripped() {
        let imported = site();
        // Attached top left at (100,200), height 3: the first baseline is one
        // height below, and each next one five thirds of a height (5) lower.
        let site = text_saying(&imported, "SITE").expect("SITE");
        let plan = text_saying(&imported, "PLAN No.\u{00A0}3").expect("PLAN No. 3");
        let half = text_saying(&imported, "1/2").expect("1/2");
        assert_eq!(text_of(site).position, p(100.0, 197.0));
        assert_eq!(text_of(plan).position, p(100.0, 192.0));
        assert_eq!(text_of(half).position, p(100.0, 187.0));
        assert_eq!(text_of(site).height, 3.0);
        assert_eq!(text_of(site).rotation, 0.0);
        assert_eq!(site.layer, "Text Layer");
    }

    #[test]
    fn the_true_colour_wins_over_the_indexed_colour() {
        let imported = site();
        // Layer Boundary: index 1 (red) and true colour 0xFF8000.
        let boundary = layer_named(&imported, "Boundary").expect("Boundary");
        assert_eq!(boundary.color, rgb(255, 128, 0));
        assert!(boundary.locked);
        assert_eq!(boundary.line_weight, 0.5); // 370 = 50 hundredths
        assert_eq!(boundary.linetype, "CENTER2");
        // The line: index 1 and true colour 255 (blue), lineweight 35 kept.
        let lines = of_kind(&imported, EntityType::Line);
        let own = lines
            .iter()
            .find(|entity| !entity.metadata.contains_key(META_BLOCK))
            .expect("the drawing's own line");
        assert_eq!(own.color, Some(rgb(0, 0, 255)));
        assert_eq!(own.metadata[META_LINEWEIGHT], PropertyValue::Integer(35));
    }

    #[test]
    fn a_linetype_pattern_that_starts_with_a_gap_is_turned_to_start_with_a_dash() {
        let imported = site();
        assert_eq!(imported.linetypes.len(), 2);
        // CENTER2 is gap 0.1, dash 0.5, gap 0.1, dash 0.2 in the file: the same
        // cycle from its first dash is dash 0.5, gap 0.1, dash 0.2, gap 0.1.
        let centre = &imported.linetypes[0];
        assert_eq!(centre.name, "CENTER2");
        assert_eq!(centre.pattern.len(), 4);
        assert_eq!(centre.pattern[0].length, 0.5);
        assert_eq!(centre.pattern[1].length, -0.1);
        assert_eq!(centre.pattern[2].length, 0.2);
        assert_eq!(centre.pattern[3].length, -0.1);
        // DOTTY is a dot and a gap, which is already Katana's form.
        let dotty = &imported.linetypes[1];
        assert_eq!(dotty.pattern.len(), 2);
        assert!(dotty.pattern[0].is_dot());
    }

    #[test]
    fn an_object_coordinate_system_facing_down_mirrors_circles_and_arcs() {
        let imported = site();
        // Extrusion (0,0,-1): the object X axis is world -X, so (x, y) is (-x, y).
        let circles = of_kind(&imported, EntityType::Circle);
        assert_eq!(circles.len(), 1);
        assert_eq!(circle_of(circles[0]).center, p(-5.0, 3.0));
        // The arc: object centre (10,0), r 5, 0 to 90 degrees, i.e. from (15,0)
        // to (10,5). In the world, from (-15,0) to (-10,5) clockwise - which is
        // counter-clockwise from 90 degrees to 180 about (-10,0).
        let arcs = of_kind(&imported, EntityType::Arc);
        assert_eq!(arcs.len(), 1);
        let arc = arc_of(arcs[0]);
        assert_eq!(arc.center, p(-10.0, 0.0));
        near(arc.start_angle, PI / 2.0, 1e-15);
        near(arc.sweep, PI / 2.0, 1e-15);
        near(arc.end_point().x, -15.0, 1e-12);
        near(arc.end_point().y, 0.0, 1e-12);
    }

    #[test]
    fn a_mirrored_lw_polylines_straight_segments_keep_a_bulge_of_plus_zero() {
        // Extrusion (0,0,-1) mirrors the polyline, so each bulge changes sign:
        // the arc's 1 becomes -1, and a straight segment's 0 stays 0 - not -0,
        // which VERTEX LIST would print as "bulge=-0".
        let imported = read(concat!(
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\n0\n90\n3\n70\n0\n",
            "10\n0\n20\n0\n42\n1\n10\n10\n20\n0\n10\n10\n20\n10\n",
            "210\n0\n220\n0\n230\n-1\n0\nENDSEC\n0\nEOF\n"
        ));
        let polylines = of_kind(&imported, EntityType::CurvePolyline);
        assert_eq!(polylines.len(), 1);
        let polyline = curve_polyline_of(polylines[0]);
        assert_eq!(polyline.vertices.len(), 3);
        assert_eq!(polyline.vertices[0].bulge, -1.0);
        assert_eq!(polyline.vertices[1].bulge, 0.0);
        assert!(!polyline.vertices[1].bulge.is_sign_negative());
        assert!(!polyline.vertices[2].bulge.is_sign_negative());
    }

    #[test]
    fn a_dimension_is_its_picture_on_the_dimensions_layer() {
        let imported = site();
        let label = text_saying(&imported, "10.00").expect("the dimension's text");
        assert_eq!(label.layer, "Text Layer"); // on 0 in the picture block
        assert_eq!(text_of(label).position, p(3.5, -4.5));
        let tally = tally_of(&imported, "DIMENSION").expect("the DIMENSION tally");
        assert_eq!(tally.imported, 2);
    }

    #[test]
    fn paper_space_hatches_and_missing_blocks_are_reported_not_imported() {
        let imported = site();
        assert!(any_warning_contains(
            &imported,
            "1 x paper space entities not imported"
        ));
        assert!(any_warning_contains(
            &imported,
            "1 x HATCH entities not imported"
        ));
        assert!(any_warning_contains(
            &imported,
            "1 x INSERT of a block the file does not define"
        ));
        let hatch = tally_of(&imported, "HATCH").expect("the HATCH tally");
        assert_eq!(hatch.imported, 0);
    }

    #[test]
    fn the_origin_shift_is_subtracted_from_every_coordinate() {
        let options = ImportOptions {
            origin_shift: Some(Vec2::new(100.0, 200.0)),
            ..ImportOptions::default()
        };
        let imported = load("r2000_site.dxf", &options);
        let site = text_saying(&imported, "SITE").expect("SITE");
        assert_eq!(text_of(site).position, p(0.0, -3.0));
        let circles = of_kind(&imported, EntityType::Circle);
        assert_eq!(circle_of(circles[0]).center, p(-105.0, -197.0));
    }

    #[test]
    fn every_imported_entity_is_valid_geometry() {
        for name in ["r12_survey.dxf", "r2000_site.dxf"] {
            let imported = load(name, &ImportOptions::default());
            for entity in &imported.entities {
                assert!(validate(&entity.geometry).is_ok(), "{name}");
            }
            assert!(!imported.bounds.empty(), "{name}");
        }
    }
}

mod dxf_reader_paths {
    use super::*;

    #[test]
    fn the_dxf_extension_is_recognised_in_any_case() {
        assert!(is_dxf_path(Path::new("site.dxf")));
        assert!(is_dxf_path(Path::new("C:/work/SITE.DXF")));
        assert!(is_dxf_path(Path::new("plan.Dxf")));
        assert!(!is_dxf_path(Path::new("site.dwg")));
        assert!(!is_dxf_path(Path::new("site.dxfx")));
        assert!(!is_dxf_path(Path::new("dxf")));
    }
}

mod dxf_reader_layers {
    use super::*;

    #[test]
    fn two_layer_names_that_become_one_path_are_one_layer() {
        // "Kerb" with a control character in it cannot be a layer path, and
        // becomes "Kerb_" - which the file also names. One layer, two entities.
        let imported = read(concat!(
            "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n",
            "  0\nLAYER\n  2\nKerb\x01\n 62\n1\n",
            "  0\nLAYER\n  2\nKerb_\n 62\n5\n",
            "  0\nENDTAB\n  0\nENDSEC\n",
            "  0\nSECTION\n  2\nENTITIES\n",
            "  0\nPOINT\n  8\nKerb\x01\n 10\n1\n 20\n1\n",
            "  0\nPOINT\n  8\nKerb_\n 10\n2\n 20\n2\n",
            "  0\nENDSEC\n  0\nEOF\n"
        ));
        assert_eq!(imported.layers.len(), 1);
        assert_eq!(imported.layers[0].name, "Kerb_");
        assert_eq!(imported.layers[0].color, rgb(255, 0, 0)); // the first stands
        assert_eq!(imported.entities.len(), 2);
        assert_eq!(imported.entities[0].layer, "Kerb_");
        assert_eq!(imported.entities[1].layer, "Kerb_");
    }

    #[test]
    fn layer_names_are_matched_without_regard_to_letter_case() {
        // The table says "Kerb", the entity "KERB": the same layer, as the format
        // compares names.
        let imported = read(concat!(
            "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n",
            "  0\nLAYER\n  2\nKerb\n 62\n3\n",
            "  0\nENDTAB\n  0\nENDSEC\n",
            "  0\nSECTION\n  2\nENTITIES\n",
            "  0\nPOINT\n  8\nKERB\n 10\n1\n 20\n1\n",
            "  0\nENDSEC\n  0\nEOF\n"
        ));
        assert_eq!(imported.layers.len(), 1);
        assert_eq!(imported.entities[0].layer, "Kerb");
    }
}

// ---- blocks ------------------------------------------------------------------------

mod dxf_reader_blocks {
    use super::*;

    #[test]
    fn a_nested_block_is_placed_through_both_inserts_and_inherits_through_both() {
        // INNER: a line (0,0)-(1,0) on layer 0, colour ByBlock. OUTER: INNER
        // inserted at (10,0) turned 90 degrees, on layer 0, ByBlock. The drawing:
        // OUTER at (100,100), twice the size, on SITE in red.
        // INNER in OUTER: (0,0)-(0,1), moved to (10,0)-(10,1). OUTER placed:
        // (20,0)-(20,2), moved to (120,100)-(120,102). Layer 0 under layer 0
        // under SITE is SITE; ByBlock under ByBlock under red is red.
        let imported = read(concat!(
            "  0\nSECTION\n  2\nBLOCKS\n",
            "  0\nBLOCK\n  2\nINNER\n 10\n0\n 20\n0\n",
            "  0\nLINE\n  8\n0\n 62\n0\n 10\n0\n 20\n0\n 11\n1\n 21\n0\n",
            "  0\nENDBLK\n",
            "  0\nBLOCK\n  2\nOUTER\n 10\n0\n 20\n0\n",
            "  0\nINSERT\n  8\n0\n 62\n0\n  2\nINNER\n 10\n10\n 20\n0\n 50\n90\n",
            "  0\nENDBLK\n  0\nENDSEC\n",
            "  0\nSECTION\n  2\nENTITIES\n",
            "  0\nINSERT\n  8\nSITE\n 62\n1\n  2\nOUTER\n 10\n100\n 20\n100\n",
            " 41\n2\n 42\n2\n",
            "  0\nENDSEC\n  0\nEOF\n"
        ));
        assert_eq!(imported.entities.len(), 1);
        let line = &imported.entities[0];
        let segment = segment_of(line);
        near(segment.start.x, 120.0, 1e-12);
        near(segment.start.y, 100.0, 1e-12);
        near(segment.end.x, 120.0, 1e-12);
        near(segment.end.y, 102.0, 1e-12);
        assert_eq!(line.layer, "SITE");
        assert_eq!(line.color, Some(rgb(255, 0, 0)));
        assert_eq!(
            property_to_string(&line.metadata[META_BLOCK]),
            "OUTER/INNER"
        );
    }

    #[test]
    fn an_array_insert_places_every_copy() {
        // Two columns 10 apart, three rows 5 apart: points at x 0 and 10, y 0, 5, 10.
        let imported = read(concat!(
            "  0\nSECTION\n  2\nBLOCKS\n",
            "  0\nBLOCK\n  2\nDOT\n 10\n0\n 20\n0\n",
            "  0\nPOINT\n 10\n0\n 20\n0\n",
            "  0\nENDBLK\n  0\nENDSEC\n",
            "  0\nSECTION\n  2\nENTITIES\n",
            "  0\nINSERT\n  2\nDOT\n 10\n0\n 20\n0\n 70\n2\n 71\n3\n 44\n10\n 45\n5\n",
            "  0\nENDSEC\n  0\nEOF\n"
        ));
        assert_eq!(imported.entities.len(), 6);
        let points: Vec<Point2> = imported.entities.iter().map(point_of).collect();
        for expected in [
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(0.0, 5.0),
            p(10.0, 5.0),
            p(0.0, 10.0),
            p(10.0, 10.0),
        ] {
            assert!(points.contains(&expected), "{},{}", expected.x, expected.y);
        }
    }

    #[test]
    fn a_circle_in_a_block_scaled_unevenly_is_the_ellipse_it_draws() {
        // A unit circle inserted twice as wide as it is high: the ellipse
        // (x/2)^2 + y^2 = 1, chorded. The chords are worked in the block's own
        // units at the tolerance over the largest scale, 0.0005 on radius 1: a
        // step of 2 acos(0.9995) = 0.06325, so 2 pi needs ceil(99.3) = 100.
        let imported = read(concat!(
            "  0\nSECTION\n  2\nBLOCKS\n",
            "  0\nBLOCK\n  2\nRING\n 10\n0\n 20\n0\n",
            "  0\nCIRCLE\n 10\n0\n 20\n0\n 40\n1\n",
            "  0\nENDBLK\n  0\nENDSEC\n",
            "  0\nSECTION\n  2\nENTITIES\n",
            "  0\nINSERT\n  2\nRING\n 10\n0\n 20\n0\n 41\n2\n 42\n1\n",
            "  0\nENDSEC\n  0\nEOF\n"
        ));
        assert_eq!(imported.entities.len(), 1);
        let ellipse = polyline_of(&imported.entities[0]);
        assert!(ellipse.closed);
        assert_eq!(ellipse.vertices.len(), 100);
        for vertex in &ellipse.vertices {
            near(
                (vertex.x / 2.0) * (vertex.x / 2.0) + vertex.y * vertex.y,
                1.0,
                1e-12,
            );
        }
    }

    #[test]
    fn an_entity_drawn_at_the_blocks_zero_is_at_the_level_of_the_insert() {
        // The survey symbol: drawn at Z 0, inserted at the point's level, 45.3.
        // Block Z 0, times the insert's Z scale 1, plus 45.3 is 45.3 - for the
        // point and the circle alike; the point drawn at 0.001 is at 45.301.
        let imported = read(concat!(
            "  0\nSECTION\n  2\nBLOCKS\n",
            "  0\nBLOCK\n  2\nPT\n 10\n0\n 20\n0\n 30\n0\n",
            "  0\nPOINT\n 10\n0\n 20\n0\n 30\n0\n",
            "  0\nCIRCLE\n 10\n0\n 20\n0\n 30\n0\n 40\n0.5\n",
            "  0\nPOINT\n 10\n1\n 20\n0\n 30\n0.001\n",
            "  0\nENDBLK\n  0\nENDSEC\n",
            "  0\nSECTION\n  2\nENTITIES\n",
            "  0\nINSERT\n  2\nPT\n 10\n100\n 20\n200\n 30\n45.3\n",
            "  0\nENDSEC\n  0\nEOF\n"
        ));
        assert_eq!(imported.entities.len(), 3);
        let dot = &imported.entities[0];
        assert_eq!(point_of(dot), p(100.0, 200.0));
        assert_eq!(heights(dot, 1)[0], Some(45.3));
        let ring = &imported.entities[1];
        assert_eq!(circle_of(ring).center, p(100.0, 200.0));
        assert_eq!(heights(ring, 1)[0], Some(45.3));
        let raised = heights(&imported.entities[2], 1)[0].expect("a raised point");
        near(raised, 45.301, 1e-12);
    }

    #[test]
    fn a_zero_level_through_nested_inserts_is_their_levels_and_an_insert_at_zero_leaves_the_plan() {
        // INNER: a point and a line at Z 0. OUTER (base Z 2) inserts INNER at
        // Z 5, so they are at 5 in OUTER. The drawing inserts OUTER at Z 100,
        // twice the height: 5 x 2 + (100 - 2 x 2) = 106. INNER inserted straight
        // at Z 0 stays in plan, with no level at all.
        let imported = read(concat!(
            "  0\nSECTION\n  2\nBLOCKS\n",
            "  0\nBLOCK\n  2\nINNER\n 10\n0\n 20\n0\n 30\n0\n",
            "  0\nPOINT\n 10\n0\n 20\n0\n 30\n0\n",
            "  0\nLINE\n 10\n0\n 20\n0\n 30\n0\n 11\n1\n 21\n0\n 31\n0\n",
            "  0\nENDBLK\n",
            "  0\nBLOCK\n  2\nOUTER\n 10\n0\n 20\n0\n 30\n2\n",
            "  0\nINSERT\n  2\nINNER\n 10\n0\n 20\n0\n 30\n5\n",
            "  0\nENDBLK\n  0\nENDSEC\n",
            "  0\nSECTION\n  2\nENTITIES\n",
            "  0\nINSERT\n  2\nOUTER\n 10\n10\n 20\n10\n 30\n100\n 43\n2\n",
            "  0\nINSERT\n  2\nINNER\n 10\n50\n 20\n50\n",
            "  0\nENDSEC\n  0\nEOF\n"
        ));
        assert_eq!(imported.entities.len(), 4);
        assert_eq!(heights(&imported.entities[0], 1)[0], Some(106.0));
        let line = heights(&imported.entities[1], 2);
        assert_eq!(line[0], Some(106.0));
        assert_eq!(line[1], Some(106.0));
        assert!(has_no_height(&imported.entities[2]));
        assert!(has_no_height(&imported.entities[3]));
    }
}

mod dxf_reader_ocs {
    use super::*;

    #[test]
    fn an_object_system_facing_down_puts_its_elevation_above_the_datum_not_below() {
        // Extrusion (0,0,-1): object X is world -X, and object Z is world -Z, so
        // the writing program puts a level of +10 as elevation -10. Every entity
        // here is at height 10, and at the mirror in X of its object position.
        let down = "210\n0\n220\n0\n230\n-1\n";
        let text = [
            "  0\nSECTION\n  2\nBLOCKS\n",
            "  0\nBLOCK\n  2\nDOT\n 10\n0\n 20\n0\n 30\n0\n",
            "  0\nPOINT\n 10\n0\n 20\n0\n 30\n0\n",
            "  0\nPOINT\n 10\n1\n 20\n0\n 30\n1\n",
            "  0\nENDBLK\n  0\nENDSEC\n",
            "  0\nSECTION\n  2\nENTITIES\n",
            "  0\nCIRCLE\n  8\nC\n 10\n-100\n 20\n50\n 30\n-10\n 40\n1\n",
            down,
            "  0\nARC\n  8\nA\n 10\n-100\n 20\n60\n 30\n-10\n 40\n1\n 50\n0\n 51\n90\n",
            down,
            "  0\nLWPOLYLINE\n  8\nL\n 90\n2\n 70\n0\n 38\n-10\n 10\n1\n 20\n1\n 10\n2\n 20\n1\n",
            down,
            "  0\nPOLYLINE\n  8\nP\n 66\n1\n 10\n0\n 20\n0\n 30\n-10\n 70\n0\n",
            down,
            "  0\nVERTEX\n  8\nP\n 10\n3\n 20\n3\n",
            "  0\nVERTEX\n  8\nP\n 10\n4\n 20\n3\n",
            "  0\nSEQEND\n",
            "  0\nTEXT\n  8\nT\n 10\n10\n 20\n5\n 30\n-10\n 40\n1\n  1\nT\n",
            down,
            // The insert at object (-100,-50) elevation -7: world (100,-50) at
            // height 7. Block Z runs down with it: the point drawn at block Z 1
            // is at -(-7 + 1) = 6, one below the insert, and at world X 99.
            "  0\nINSERT\n  8\nI\n  2\nDOT\n 10\n-100\n 20\n-50\n 30\n-7\n",
            down,
            // The plain system, for comparison: elevation 10 is height 10.
            "  0\nCIRCLE\n  8\nU\n 10\n0\n 20\n0\n 30\n10\n 40\n1\n",
            "  0\nENDSEC\n  0\nEOF\n",
        ]
        .concat();
        let imported = read(&text);
        assert_eq!(imported.entities.len(), 8);
        let e = &imported.entities;
        assert_eq!(circle_of(&e[0]).center, p(100.0, 50.0));
        assert_eq!(heights(&e[0], 1)[0], Some(10.0));
        assert_eq!(arc_of(&e[1]).center, p(100.0, 60.0));
        assert_eq!(heights(&e[1], 2)[0], Some(10.0));
        assert_eq!(heights(&e[1], 2)[1], Some(10.0));
        let lw = polyline_of(&e[2]);
        assert_eq!(lw.vertices[0], p(-1.0, 1.0));
        assert_eq!(lw.vertices[1], p(-2.0, 1.0));
        assert_eq!(heights(&e[2], 2)[0], Some(10.0));
        assert_eq!(heights(&e[2], 2)[1], Some(10.0));
        let two = polyline_of(&e[3]);
        assert_eq!(two.vertices[0], p(-3.0, 3.0));
        assert_eq!(heights(&e[3], 2)[1], Some(10.0));
        assert_eq!(text_of(&e[4]).position, p(-10.0, 5.0));
        assert_eq!(heights(&e[4], 1)[0], Some(10.0));
        assert_eq!(point_of(&e[5]), p(100.0, -50.0));
        assert_eq!(heights(&e[5], 1)[0], Some(7.0));
        assert_eq!(point_of(&e[6]), p(99.0, -50.0));
        assert_eq!(heights(&e[6], 1)[0], Some(6.0));
        assert_eq!(heights(&e[7], 1)[0], Some(10.0));
    }
}

mod dxf_reader_m_text {
    use super::*;

    #[test]
    fn an_m_text_facing_down_stands_at_its_world_insertion_point() {
        // MTEXT is not an object system entity: its point (10/20/30) and its
        // direction (11/21/31) are world coordinates, whatever way it faces.
        let down = "210\n0\n220\n0\n230\n-1\n";
        let text = [
            "  0\nSECTION\n  2\nENTITIES\n",
            // Bottom left (7), one line: the first baseline is the point itself,
            // (10,5), at height 7 - not -7. With the angle only, 0 is measured
            // in the plane facing down, whose X is world -X: the text runs
            // toward -X, a rotation of half a turn.
            "  0\nMTEXT\n 10\n10\n 20\n5\n 30\n7\n 40\n2\n 71\n7\n  1\nABC\n",
            down,
            // With a direction vector, it is the world direction: (0,1), a
            // quarter turn, standing at (10,20).
            "  0\nMTEXT\n 10\n10\n 20\n20\n 40\n2\n 71\n7\n 11\n0\n 21\n1\n 31\n0\n  1\nDEF\n",
            down,
            // Top left (1), two lines of height 3: the first baseline one height
            // below the point, the second a pitch of 5/3 x 3 = 5 further. The
            // text's up is still world +Y facing down - only its X is mirrored -
            // so the baselines are at y -3 and -8.
            "  0\nMTEXT\n 10\n0\n 20\n0\n 40\n3\n 71\n1\n  1\nG\\PH\n",
            down,
            "  0\nENDSEC\n  0\nEOF\n",
        ]
        .concat();
        let imported = read(&text);
        assert_eq!(imported.entities.len(), 4);
        let abc = text_saying(&imported, "ABC").expect("ABC");
        let first = text_of(abc);
        assert_eq!(first.position, p(10.0, 5.0));
        assert_eq!(first.rotation.cos(), -1.0);
        assert_eq!(heights(abc, 1)[0], Some(7.0));
        let def = text_saying(&imported, "DEF").expect("DEF");
        let second = text_of(def);
        assert_eq!(second.position, p(10.0, 20.0));
        assert_eq!(second.rotation, PI / 2.0);
        let g = text_saying(&imported, "G").expect("G");
        let h = text_saying(&imported, "H").expect("H");
        assert_eq!(text_of(g).position, p(0.0, -3.0));
        assert_eq!(text_of(h).position, p(0.0, -8.0));
    }
}
