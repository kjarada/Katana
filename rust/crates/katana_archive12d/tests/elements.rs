//! The twin of tests/archive12d/test_elements.cpp: every element of manual
//! 1.4 and 1.5 read into its typed form, and the fixtures read whole.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use std::collections::BTreeMap;

use katana_archive12d::{
    AlignmentSource, ArcString, CircleString, DrainageString, LasCloud, LasPoint, PlotFrame,
    ReadOptions, SegmentKind, StringKind, SuperAlignment, SuperTin, TextString, Tin, Trimesh,
    VertexString, element_keyword, read_archive, read_archive_bytes,
};

use support::{at, attribute_group, attribute_text, fixture, get, only, read};

// ---- simple strings (manual 1.5.1 - 1.5.7, 1.5.10) -----------------------------

mod reader_strings {
    use super::*;

    #[test]
    fn an_arc_is_its_centre_its_ends_and_its_radius() {
        let archive = read(
            "string arc { name \"A\" colour blue chainage 5 interval 1 radius -15\n\
             \x20xcentre 600 ycentre 100 zcentre 31 xstart 615 ystart 100 zstart 31\n\
             \x20xend 600 yend 115 zend 31.5 }",
        );
        let arc = only::<ArcString>(&archive);
        assert_eq!(arc.header.name, "A");
        assert_eq!(arc.header.chainage, 5.0);
        assert_eq!(arc.radius, -15.0);
        assert_eq!(arc.centre, at(600.0, 100.0, Some(31.0)));
        assert_eq!(arc.start, at(615.0, 100.0, Some(31.0)));
        assert_eq!(arc.end, at(600.0, 115.0, Some(31.5)));
        assert_eq!(arc.header.extras.real("interval"), Some(1.0));
        assert!(
            !arc.header.extras.contains("radius"),
            "a modelled field is not also an extra"
        );
    }

    #[test]
    fn a_circle_and_a_feature_differ_only_in_name() {
        let archive = read(
            "string circle { radius 8 zcentre 32 xcentre 650 ycentre 200 }\n\
             string feature { radius 3 zcentre null xcentre 700 ycentre 250 }",
        );
        assert_eq!(archive.elements.len(), 2);
        let circle = get::<CircleString>(&archive.elements[0]);
        let feature = get::<CircleString>(&archive.elements[1]);
        assert!(!circle.feature);
        assert_eq!(circle.radius, 8.0);
        assert_eq!(circle.centre, at(650.0, 200.0, Some(32.0)));
        assert!(feature.feature);
        assert!(feature.centre.z.is_none());
    }

    #[test]
    fn a_face_keeps_its_hatching_and_an_interface_its_cut_fill_modes() {
        let archive = read(
            "string face { hatch_angle 45 hatch_distance 2 hatch_colour red \
             edge_colour blue fill_mode 1 edge_mode 0 data { 0 0 1  4 0 1  4 3 1 } }\n\
             string interface { data { 0 0 1 -1   5 0 2 0   9 0 3 1 } }",
        );
        assert_eq!(archive.elements.len(), 2);
        let face = get::<VertexString>(&archive.elements[0]);
        assert_eq!(face.kind, StringKind::Face);
        assert_eq!(face.vertices.len(), 3);
        assert_eq!(face.header.extras.real("hatch_angle"), Some(45.0));
        assert_eq!(face.header.extras.text("hatch_colour", ""), "red");
        assert_eq!(face.header.extras.integer("fill_mode"), Some(1));

        let interface = get::<VertexString>(&archive.elements[1]);
        assert_eq!(interface.kind, StringKind::Interface);
        assert_eq!(interface.vertices.len(), 3);
        assert_eq!(interface.vertices[2], at(9.0, 0.0, Some(3.0)));
        assert_eq!(interface.interface_modes, vec![-1, 0, 1]);
    }

    #[test]
    fn a_text_string_keeps_how_it_is_drawn_apart_from_where_and_what() {
        let archive = read(
            "string text { name \"\" colour \"pen 025\" worldsize 2.7 textstyle \"Arial Narrow\"\n\
             \x20angle 30 offset 3 raise -2.5 justify \"bottom-left\" time_created \"08-Nov-2025 06:05:42\"\n\
             \x20x 307416.316 y 6270203.132 z null text \"SS112061\" }",
        );
        let text = only::<TextString>(&archive);
        assert_eq!(text.text, "SS112061");
        assert_eq!(text.position, at(307416.316, 6270203.132, None));
        assert_eq!(text.header.colour, "pen 025");
        assert_eq!(text.annotation.real("worldsize"), Some(2.7));
        assert_eq!(text.annotation.real("angle"), Some(30.0));
        assert_eq!(text.annotation.text("justify", ""), "bottom-left");
        assert!(!text.annotation.contains("x"));
        assert!(!text.annotation.contains("text"));
        assert_eq!(
            text.header.extras.text("time_created", ""),
            "08-Nov-2025 06:05:42"
        );
    }

    #[test]
    fn the_one_line_text_form_other_tools_write_is_read() {
        let archive = read(
            "model \"Labels\"\ntext \"PM001 RL=32.451\" 502000 6960000 32.5\n\
             text \"no height\" 1 2\nstring super { data_2d { 0 0 } }",
        );
        assert_eq!(archive.elements.len(), 3);
        let first = get::<TextString>(&archive.elements[0]);
        assert_eq!(first.text, "PM001 RL=32.451");
        assert_eq!(first.position, at(502000.0, 6960000.0, Some(32.5)));
        assert_eq!(first.header.model, "Labels");
        assert!(get::<TextString>(&archive.elements[1]).position.z.is_none());
    }

    #[test]
    fn a_plot_frame_is_its_fields() {
        let archive = read(
            "string plot_frame { name \"Sheet 01\" border 1 width 420 height 297 scale 500\n\
             \x20rotation 0 xorigin 503600 yorigin 6960000 colour white title_1 \"Plan\" }",
        );
        let frame = only::<PlotFrame>(&archive);
        assert_eq!(frame.header.name, "Sheet 01");
        assert_eq!(frame.header.colour, "white");
        assert_eq!(frame.fields.real("width"), Some(420.0));
        assert_eq!(frame.fields.real("scale"), Some(500.0));
        assert_eq!(frame.fields.text("title_1", ""), "Plan");
    }
}

// ---- drainage (manual 1.5.3) ------------------------------------------------------

mod reader_drainage {
    use super::*;

    #[test]
    fn pits_pipes_controls_and_connections_stay_out_of_the_lines_geometry() {
        let archive = read(
            r#"string drainage {
  name "SW Line A" outfall 28.5 flow_direction 1
  attributes { text Tin "DESIGN" integer "_floating" 0 }
  data { 0 0 29 0 0   50 20 29.5 0 0   100 40 30 0 0 }
  pit { name "A1" type "Grated" diameter 1.05 floating no x 0 y 0 z 31 }
  pit { name "A2" x 50 y 20 z 31.5 attributes { text "Lid" "Class D" } }
  pipe { name "A1-A2" type "RCP" diameter 0.375 us_level 29.5 ds_level 29 }
  property_control { name "Lot 4" grade 60 x 25 y 10 z 29.2 data { 25 10 29.2 0 0   30 40 30 0 0 } }
  house_connection { name "HC1" hcb 7 side left x 40 y 16 z 29.4 }
}"#,
        );
        let d = only::<DrainageString>(&archive);
        assert_eq!(d.outfall, Some(28.5));
        assert_eq!(d.flow_direction, Some(1));
        assert_eq!(
            d.vertices.len(),
            3,
            "coordinates inside pits must not leak into the line"
        );
        assert_eq!(d.vertices[1], at(50.0, 20.0, Some(29.5)));
        assert_eq!(attribute_text(&d.header.attributes[0]), "DESIGN");
        assert_eq!(d.pits.len(), 2);
        assert!(!d.pits_are_version2);
        assert_eq!(d.pits[0].fields.text("name", ""), "A1");
        assert_eq!(d.pits[0].fields.real("diameter"), Some(1.05));
        assert_eq!(d.pits[0].fields.boolean("floating"), Some(false));
        assert_eq!(d.pits[1].attributes.len(), 1);
        assert_eq!(d.pipes.len(), 1);
        assert_eq!(d.pipes[0].fields.real("us_level"), Some(29.5));
        assert_eq!(d.property_controls.len(), 1);
        assert_eq!(d.property_controls[0].fields.real("grade"), Some(60.0));
        assert_eq!(d.property_controls[0].vertices.len(), 2);
        assert_eq!(
            d.property_controls[0].vertices[1],
            at(30.0, 40.0, Some(30.0))
        );
        assert_eq!(d.house_connections.len(), 1);
        assert_eq!(d.house_connections[0].fields.integer("hcb"), Some(7));
    }

    #[test]
    fn version_two_pits_are_remembered_as_such() {
        let archive = read(
            "string drainage { data { 0 0 1 0 0 } pit_v2 { name \"P\" riser_enabled true \
             riser_colour dark green base_height 0.3 x 0 y 0 z 2 } }",
        );
        let d = only::<DrainageString>(&archive);
        assert!(d.pits_are_version2);
        assert_eq!(d.pits[0].fields.text("riser_colour", ""), "dark green");
        assert_eq!(d.pits[0].fields.real("base_height"), Some(0.3));
    }
}

// ---- alignments (manual 1.5.9, 1.5.16, 1.5.17) ------------------------------------------

mod reader_alignment {
    use super::*;

    #[test]
    fn the_manuals_own_ip_example_is_read_part_by_part() {
        // Manual 1.5.9.1.2 and 1.5.9.2.2, abbreviated to one of each kind.
        let archive = read(
            r#"string super_alignment {
  name "MC01" spiral_type clothoid valid_horizontal true valid_vertical false closed false
  horizontal_parts {
    ip { id 100 x 42606.66161172 y 37239.28824481 }
    spiral { id 300 r 50 l1 30 l2 40 x 43336.6595 y 37469.2563 }
    arc { id 400 r 75 x 43481.15324268 y 37331.6431906 }
  }
  vertical_parts {
    ip { id 600 x -50.8459652 y 159.79764161 }
    kvalue { id 700 k 1.25 x 38.4627 y 179.2126 }
    length { id 800 l 50 x 172.61694837 y 154.72967932 }
    asymmetric { id 900 l1 25 l2 75 x 270.0182 y 208.1493 }
    arc { id 1000 r 1000 x 424.2402 y 196.5637 }
    radius { id 1100 r 200 x 526.7263 y 201.5302 }
  }
}"#,
        );
        let a = only::<SuperAlignment>(&archive);
        assert_eq!(a.source, AlignmentSource::SuperAlignment);
        assert_eq!(a.spiral_type, "clothoid");
        assert_eq!(a.valid_horizontal, Some(true));
        assert_eq!(a.valid_vertical, Some(false));
        assert_eq!(a.horizontal_parts.len(), 3);
        assert_eq!(a.horizontal_parts[1].kind, "spiral");
        assert_eq!(a.horizontal_parts[1].fields.real("l2"), Some(40.0));
        assert_eq!(a.horizontal_parts[2].fields.real("x"), Some(43481.15324268));
        assert_eq!(a.vertical_parts.len(), 6);
        assert_eq!(a.vertical_parts[1].fields.real("k"), Some(1.25));
        assert_eq!(a.vertical_parts[3].kind, "asymmetric");
        assert!(a.horizontal_is_ip_only());
        assert!(a.vertical_is_ip_only());
    }

    #[test]
    fn solved_geometry_is_vertices_and_the_segments_between_them() {
        let archive = read(
            r#"string super_alignment { name "A"
  horizontal_data { name "A" closed 0 weight 1
    data_2d { 0 0  100 0  150 50 }
    geometry_data { straight { } arc { radius -50 major 0 } } }
  vertical_data { closed 0
    data_2d { 0 30  60 33  140 31 }
    geometry_data { straight { } parabola { chainage 100 height 35 } } }
}"#,
        );
        let a = only::<SuperAlignment>(&archive);
        let horizontal = a.horizontal_data.as_ref().expect("horizontal data");
        assert_eq!(horizontal.vertices.len(), 3);
        assert_eq!(horizontal.segments.len(), 2);
        assert_eq!(horizontal.segments[1].radius, -50.0);
        assert_eq!(horizontal.header.real("weight"), Some(1.0));
        let vertical = a.vertical_data.as_ref().expect("vertical data");
        assert_eq!(vertical.vertices[1], at(60.0, 33.0, None));
        assert_eq!(vertical.segments[1].kind, SegmentKind::Parabola);
        assert_eq!(vertical.segments[1].parameters.real("height"), Some(35.0));
        assert!(
            !a.horizontal_is_ip_only(),
            "no parts at all is not an IP definition"
        );
    }

    #[test]
    fn twelve_ds_undocumented_constructions_are_kept_and_said_not_to_be_ips() {
        let archive = read(
            "string super_alignment { horizontal_parts {\n\
             \x20ip { id 100 x 0 y 0 }\n\
             \x20floating_arc_end_radius_length { id 200 radius 50 length 30 attach_to previous_part }\n\
             } drawables { labels { } } geometry_modifiers { } }",
        );
        let a = only::<SuperAlignment>(&archive);
        assert_eq!(a.horizontal_parts.len(), 2);
        assert_eq!(a.horizontal_parts[1].kind, "floating_arc_end_radius_length");
        assert_eq!(a.horizontal_parts[1].fields.real("radius"), Some(50.0));
        assert!(!a.horizontal_is_ip_only());
        assert_eq!(archive.unrecognised["string super_alignment/drawables"], 1);
        assert_eq!(
            archive.unrecognised["string super_alignment/geometry_modifiers"],
            1
        );
    }

    #[test]
    fn an_old_alignment_strings_rows_become_the_same_ip_parts() {
        // Manual 1.5.16: "x y radius [spil1 L spil2 L]" and "ch z length
        // [parabola]" or "ch z radius circle".
        let archive = read(
            r#"string alignment { name "AL" spiral_type "clothoid" draw_mode 1
  hipdata { 0 0 0
            100 0 40
            150 100 60 spil1 20 spil2 25
            300 100 0 }
  vipdata { 0 30 0
            100 33 40 parabola
            160 32 35
            220 31 500 circle
            300 30 0 }
}"#,
        );
        let a = only::<SuperAlignment>(&archive);
        assert_eq!(a.source, AlignmentSource::Alignment);
        assert_eq!(a.horizontal_parts.len(), 4);
        assert_eq!(a.horizontal_parts[0].kind, "ip");
        assert_eq!(a.horizontal_parts[1].kind, "arc");
        assert_eq!(a.horizontal_parts[1].fields.real("r"), Some(40.0));
        assert_eq!(a.horizontal_parts[2].kind, "spiral");
        assert_eq!(a.horizontal_parts[2].fields.real("l1"), Some(20.0));
        assert_eq!(a.horizontal_parts[2].fields.real("l2"), Some(25.0));
        assert_eq!(a.horizontal_parts[2].fields.real("y"), Some(100.0));
        // "a number that is unique for each horizontal and vertical part ... a
        // multiple of 100" (manual 1.5.9.1.2).
        assert_eq!(a.horizontal_parts[3].fields.integer("id"), Some(400));
        assert_eq!(a.vertical_parts.len(), 5);
        assert_eq!(a.vertical_parts[0].fields.integer("id"), Some(500));
        assert_eq!(a.vertical_parts[1].kind, "length");
        assert_eq!(
            a.vertical_parts[2].kind, "length",
            "the word parabola is optional"
        );
        assert_eq!(a.vertical_parts[2].fields.real("l"), Some(35.0));
        assert_eq!(a.vertical_parts[3].kind, "arc");
        assert_eq!(a.vertical_parts[3].fields.real("r"), Some(500.0));
        assert_eq!(a.header.extras.integer("draw_mode"), Some(1));
    }

    #[test]
    fn a_pipeline_is_an_alignment_with_a_size() {
        let archive =
            read("string pipeline { diameter 0.375 length 6 hipdata { 0 0 0  100 50 0 } }");
        let a = only::<SuperAlignment>(&archive);
        assert_eq!(a.source, AlignmentSource::Pipeline);
        assert_eq!(a.diameter, Some(0.375));
        assert_eq!(a.pipe_length, Some(6.0));
        assert_eq!(a.horizontal_parts.len(), 2);
    }

    #[test]
    fn a_hip_row_that_is_not_three_numbers_is_an_error() {
        let options = ReadOptions::default();
        assert!(read_archive("string alignment { hipdata { 0 0 0  100 } }", &options).is_err());
        assert!(read_archive("string alignment { hipdata { 0 0 0 spil1 } }", &options).is_err());
    }
}

// ---- superseded strings (manual 1.5.11 - 1.5.15) --------------------------------------------

mod reader_superseded {
    use super::*;

    #[test]
    fn each_old_string_type_arrives_as_the_super_string_that_replaced_it() {
        let archive = read(
            r#"string 2d { z 12.5 data { 0 0  5 5 } }
string 3d { data { 0 0 1  5 5 2 } }
string 4d { angle 15 worldsize 2 textstyle "ISO" justify "middle-centre" data { 0 0 1 "T1"  5 5 null "" } }
string pipe { diameter 0.6 data { 0 0 1  5 5 2 } }
string polyline { closed 1 data { 0 0 1 12 0   20 0 1 0 0   20 20 1 0 0 } }"#,
        );
        assert_eq!(archive.elements.len(), 5);
        let two_d = get::<VertexString>(&archive.elements[0]);
        assert_eq!(two_d.kind, StringKind::TwoD);
        assert_eq!(two_d.constant_z, Some(12.5));
        assert_eq!(two_d.vertices.len(), 2);

        let three_d = get::<VertexString>(&archive.elements[1]);
        assert_eq!(three_d.vertices[1], at(5.0, 5.0, Some(2.0)));

        let four_d = get::<VertexString>(&archive.elements[2]);
        assert_eq!(four_d.vertex_text, vec!["T1", ""]);
        assert!(four_d.vertices[1].z.is_none());
        let annotation = four_d.vertex_annotation.as_ref().expect("an annotation");
        assert_eq!(annotation.real("angle"), Some(15.0));
        assert_eq!(annotation.text("justify", ""), "middle-centre");
        assert!(
            four_d.justify.is_empty(),
            "a 4d string's justify is its text's, not a pipe's"
        );

        assert_eq!(
            get::<VertexString>(&archive.elements[3]).diameter,
            Some(0.6)
        );

        let polyline = get::<VertexString>(&archive.elements[4]);
        assert!(polyline.closed);
        assert_eq!(polyline.segments.len(), 3);
        assert_eq!(polyline.segments[0].radius, 12.0);
    }
}

// ---- LAS clouds (manual 1.5.18) ----------------------------------------------------------------

mod reader_las {
    use super::*;

    #[test]
    fn tagged_and_compact_records_of_one_format_give_the_same_point() {
        // Format 3 is format 1 (format 0 plus gps time) plus colour: x y z i
        // rn rc sd fe cl sr ud id t c.
        let archive = read(
            r#"string las_cloud_data { name "C" data {
  categories { true false } format v12_p3 range { xmin 1 xmax 2 ymin 3 ymax 4 }
  points_v12_p3 { p { x 10.5 y 20.5 z 3.25 i 1200 rn 2 rc 3 sd 1 fe 0 cl 6 sr -12 ud 9 id 77 t 1234.5 c 281474976710655 } }
  compact_points_v12_p3 { p { 10.5 20.5 3.25 1200 2 3 1 0 6 -12 9 77 1234.5 281474976710655 } }
} }"#,
        );
        let cloud = only::<LasCloud>(&archive);
        assert_eq!(cloud.format, "v12_p3");
        assert_eq!(cloud.point_format, 3);
        assert_eq!(cloud.categories, vec![true, false]);
        assert_eq!(cloud.range.real("ymax"), Some(4.0));
        assert_eq!(cloud.points.len(), 2);
        assert_eq!(cloud.points[0], cloud.points[1]);
        let p: &LasPoint = &cloud.points[0];
        assert_eq!(p.x, 10.5);
        assert_eq!(p.z, 3.25);
        assert_eq!(p.intensity, 1200);
        assert_eq!(p.return_number, 2);
        assert_eq!(p.return_count, 3);
        assert_eq!(p.scan_direction, 1);
        assert_eq!(p.classification, 6);
        assert_eq!(p.scan_angle, -12);
        assert_eq!(p.user_data, 9);
        assert_eq!(p.point_source_id, 77);
        assert_eq!(p.gps_time, 1234.5);
        assert_eq!(
            p.colour, 281474976710655,
            "2^48 - 1: more than a double's 53 bits would survive, but only as an integer"
        );
    }

    #[test]
    fn the_las_one_point_four_records_put_user_data_before_scan_angle() {
        // Manual 1.5.18 point_p6: x y z i rn rc cf sc sd fe cl ud sr id t; p8
        // adds c and ir. In formats 0-5 the order is ... cl sr ud id.
        let archive = read(
            "string las_cloud_data { data { format v14_p8\n\
             \x20compact_points_v14_p8 { p { 1 2 3 100 5 6 7 2 1 0 4 200 -30 9 55.5 42 17 } } } }",
        );
        let p = &only::<LasCloud>(&archive).points[0];
        assert_eq!(p.return_number, 5);
        assert_eq!(p.return_count, 6);
        assert_eq!(p.classification_flags, 7);
        assert_eq!(p.scanner_channel, 2);
        assert_eq!(p.scan_direction, 1);
        assert_eq!(p.classification, 4);
        assert_eq!(p.user_data, 200);
        assert_eq!(p.scan_angle, -30);
        assert_eq!(p.point_source_id, 9);
        assert_eq!(p.gps_time, 55.5);
        assert_eq!(p.colour, 42);
        assert_eq!(p.near_infrared, 17);
    }

    #[test]
    fn a_cloud_may_name_a_las_file_instead_of_holding_points() {
        let archive = read(
            "string las_cloud_data { name \"R\" ref_data { categories { true }\n\
             \x20file_name \"scans/site.las\" range { xmin 0 xmax 9 ymin 0 ymax 9 } } }",
        );
        let cloud = only::<LasCloud>(&archive);
        assert_eq!(cloud.reference_file, "scans/site.las");
        assert!(cloud.points.is_empty());
        assert_eq!(cloud.range.real("xmax"), Some(9.0));
    }

    #[test]
    fn a_compact_record_without_even_a_position_is_an_error() {
        assert!(
            read_archive(
                "string las_cloud_data { data { format v10_p0\n compact_points_v10_p0 { p { 1 2 } } } }",
                &ReadOptions::default()
            )
            .is_err()
        );
    }
}

// ---- tins (manual 1.4.7, 1.4.8) --------------------------------------------------------------------

mod reader_tin {
    use super::*;

    #[test]
    fn point_numbers_start_at_one_and_become_zero_based() {
        let archive = read(
            "tin { name \"T\" colour green points { 0 0 1  10 0 2  0 10 3  10 10 null }\n\
             \x20triangles { 1 3 2   2 3 4 } colours { -1 \"vis grass\" } }",
        );
        let tin = only::<Tin>(&archive);
        assert!(!tin.full);
        assert_eq!(tin.name, "T");
        assert_eq!(tin.colour, "green");
        assert_eq!(tin.points.len(), 4);
        assert!(tin.points[3].z.is_none());
        assert_eq!(tin.triangles.len(), 2);
        assert_eq!(tin.triangles[0], [0, 2, 1]);
        assert_eq!(tin.triangles[1], [1, 2, 3]);
        assert_eq!(tin.colours, vec!["-1", "vis grass"]);
        assert!(
            tin.is_surface_triangle(0),
            "a tin's first four points are data, not construction"
        );
    }

    #[test]
    fn a_full_tin_says_which_triangles_are_visible_and_which_are_construction() {
        // Manual 1.4.7.1: the first four points are construction points;
        // nulling is 1 for null and 2 for visible; a neighbour of 0 is "none".
        let archive = read(
            r#"full_tin { name "F"
  points { -100 -100 0  -100 100 0  100 100 0  100 -100 0   0 0 5  10 0 6  0 10 7  10 10 8 }
  triangles { 5 7 6   6 7 8   1 5 6 }
  neighbours { 0 2 3   1 0 0   0 1 0 }
  nulling { 2 1 2 }
  input { preserve_strings true weed_tin false models { "SURVEY" "DESIGN" } }
}"#,
        );
        let tin = only::<Tin>(&archive);
        assert!(tin.full);
        assert_eq!(tin.triangles.len(), 3);
        assert_eq!(tin.visible, vec![true, false, true]);
        assert!(tin.is_surface_triangle(0));
        assert!(!tin.is_surface_triangle(1), "nulled");
        assert!(
            !tin.is_surface_triangle(2),
            "visible, but it touches construction point 1"
        );
        assert_eq!(tin.neighbours.len(), 3);
        assert_eq!(tin.neighbours[0], [Tin::NO_NEIGHBOUR, 1, 2]);
        assert_eq!(tin.input.boolean("preserve_strings"), Some(true));
        assert_eq!(tin.input_models, vec!["SURVEY", "DESIGN"]);
    }

    #[test]
    fn triangles_may_be_wrapped_in_a_vertices_block_as_some_writers_do() {
        let archive = read(
            "full_tin { name \"F\" points { 0 0 0 0 1 0 1 1 0 1 0 0  2 2 1 3 2 1 2 3 1 }\n\
             \x20triangles { vertices { 5 7 6 } } nulling { 2 } }",
        );
        assert_eq!(only::<Tin>(&archive).triangles[0], [4, 6, 5]);
    }

    #[test]
    fn a_triangle_naming_a_point_that_does_not_exist_is_an_error() {
        let error = read_archive(
            "tin { name \"T\" points { 0 0 0  1 0 0  0 1 0 } triangles { 1 2 4 } }",
            &ReadOptions::default(),
        )
        .expect_err("point 4 does not exist");
        assert!(error.message.contains("point 4"), "{}", error.describe());
    }

    #[test]
    fn point_numbers_that_do_not_make_whole_triangles_are_an_error() {
        assert!(
            read_archive(
                "tin { name \"T\" points { 0 0 0 1 0 0 0 1 0 } triangles { 1 2 3 1 } }",
                &ReadOptions::default()
            )
            .is_err()
        );
    }

    #[test]
    fn a_file_that_counts_from_zero_is_read_and_said_to_be_non_conforming() {
        let archive = read("tin { name \"T\" points { 0 0 0  1 0 0  0 1 0 } triangles { 0 2 1 } }");
        assert_eq!(only::<Tin>(&archive).triangles[0], [0, 2, 1]);
        assert!(!archive.warnings.is_empty());
        assert!(archive.warnings[0].contains("from 0"));
    }

    #[test]
    fn a_super_tin_is_an_ordered_list_of_tin_names() {
        let archive = read(
            "super_tin { name \"COMBINED\" colour green exact true \
             attributes { text \"Style\" \"1\" } tins { \"NATURAL\" \"PAD A\" PadB } }",
        );
        let super_tin = only::<SuperTin>(&archive);
        assert_eq!(super_tin.name, "COMBINED");
        assert_eq!(super_tin.tins, vec!["NATURAL", "PAD A", "PadB"]);
        assert_eq!(super_tin.extras.boolean("exact"), Some(true));
        assert_eq!(super_tin.attributes.len(), 1);
    }
}

// ---- trimeshes (manual 1.4.9) --------------------------------------------------------------------------

mod reader_trimesh {
    use super::*;

    #[test]
    fn vertices_faces_edges_and_their_presentation_are_all_read() {
        let archive = read(
            r#"model "Site Mesh"
primitive_3d { attributes { text "Material" "Rock" } name Stockpile colour brown
  trimesh_3d {
    info { flag 0 key 1 colour brown name Stockpile }
    vertices { 0 0 0  20 0 0  20 20 0  10 10 3.5 }
    faces { 1 2 4   2 3 4 }
    edges { 1 2  2 3 }
    blend 0.5
    face_infos { 0 0 green "no name"   0 1 blue "side" }
    face_flags { 2 0 }
  }
}"#,
        );
        let mesh = only::<Trimesh>(&archive);
        assert_eq!(mesh.name, "Stockpile");
        assert_eq!(mesh.model, "Site Mesh");
        assert_eq!(mesh.colour, "brown");
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.faces.len(), 2);
        assert_eq!(mesh.faces[1], [1, 2, 3]);
        assert_eq!(mesh.edges[1], [1, 2]);
        assert_eq!(mesh.info.integer("key"), Some(1));
        assert_eq!(mesh.blend, Some(0.5));
        assert_eq!(mesh.face_infos.len(), 2);
        assert_eq!(mesh.face_infos[1].colour, "blue");
        assert_eq!(mesh.face_infos[1].name, "side");
        assert_eq!(mesh.face_flags, vec![2, 0], "1-based; 0 is none");
        assert_eq!(mesh.attributes.len(), 1);
    }

    #[test]
    fn a_face_naming_a_vertex_that_does_not_exist_is_an_error() {
        assert!(
            read_archive(
                "primitive_3d { name M trimesh_3d { vertices { 0 0 0 1 0 0 0 1 0 } faces { 1 2 9 } } }",
                &ReadOptions::default()
            )
            .is_err()
        );
    }
}

// ---- the project --------------------------------------------------------------------------------------------

mod reader_project {
    use super::*;

    #[test]
    fn project_attributes_are_read() {
        let archive = read(
            "project_attributes { group { name \"Meta\" attributes { text \"Zone\" \"MGA 56\" } } }",
        );
        assert_eq!(archive.project_attributes.len(), 1);
        let group = attribute_group(&archive.project_attributes[0]);
        assert_eq!(attribute_text(&group[0]), "MGA 56");
    }
}

// ---- whole files ------------------------------------------------------------------------------------------------

mod reader_files {
    use super::*;

    #[test]
    fn the_all_geometries_fixture_holds_one_of_everything_and_nothing_is_left_unread() {
        let archive = read_archive_bytes(fixture("all_geometries.12da"), &ReadOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));

        // Counted by hand from the file: 8 super strings, 3 one-line texts,
        // and one each of the other sixteen kinds it was written to exercise.
        let mut counts: BTreeMap<String, i32> = BTreeMap::new();
        for element in &archive.elements {
            *counts.entry(element_keyword(element)).or_insert(0) += 1;
        }
        let count = |keyword: &str| counts.get(keyword).copied().unwrap_or(0);
        assert_eq!(count("string super"), 8);
        assert_eq!(count("string text"), 3);
        for keyword in [
            "string super_alignment",
            "full_tin",
            "super_tin",
            "primitive_3d",
            "string arc",
            "string circle",
            "string feature",
            "string interface",
            "string polyline",
            "string 4d",
            "string alignment",
            "string pipeline",
            "string drainage",
            "string plot_frame",
            "string las_cloud_data",
        ] {
            assert_eq!(count(keyword), 1, "{keyword}");
        }
        assert_eq!(archive.elements.len(), 26);
        assert_eq!(archive.model_names.len(), 15);
        assert_eq!(archive.model_names[0], "Survey Control");
        assert!(archive.unrecognised.is_empty());
        // The fixture's full_tin was written by hand without the neighbours
        // block that manual 1.4.7.1 makes mandatory - and that is the one
        // thing the reader has to say about the file.
        assert_eq!(archive.warnings.len(), 1, "{:?}", archive.warnings.first());
        assert!(
            archive.warnings[0].contains("no neighbours block"),
            "{}",
            archive.warnings[0]
        );
        assert!(!archive.header_settings.is_empty());
        assert_eq!(archive.header_settings[0].value, "15.01.08.60");
    }

    #[test]
    fn the_other_fixtures_read_without_complaint() {
        for name in [
            "drainage_strings.12da",
            "super_tin.12da",
            "multiple_tins.12da",
            "las_point_cloud.12da",
        ] {
            let archive = read_archive_bytes(fixture(name), &ReadOptions::default())
                .unwrap_or_else(|error| panic!("{name}: {}", error.describe()));
            assert!(!archive.elements.is_empty(), "{name}");
            assert!(archive.unrecognised.is_empty(), "{name}");
            assert!(
                archive.warnings.is_empty(),
                "{name}: {:?}",
                archive.warnings.first()
            );
        }
    }

    #[test]
    fn the_super_tin_fixture_ranks_its_three_tins() {
        let archive = read_archive_bytes(fixture("super_tin.12da"), &ReadOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        // The file's own header: one natural surface of 25 points and 32
        // triangles (a 5 x 5 grid is 4 x 4 cells of 2), two 3 x 3 pads of 8.
        assert_eq!(archive.elements.len(), 4);
        let natural = get::<Tin>(&archive.elements[0]);
        assert_eq!(natural.points.len(), 25);
        assert_eq!(natural.triangles.len(), 32);
        assert_eq!(get::<Tin>(&archive.elements[1]).triangles.len(), 8);
        let super_tin = get::<SuperTin>(&archive.elements[3]);
        assert_eq!(super_tin.tins, vec!["NATURAL SURFACE", "PAD A", "PAD B"]);
    }
}
