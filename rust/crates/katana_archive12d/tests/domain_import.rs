//! The twin of tests/archive12d/test_domain_import.cpp: layers, colours,
//! geometry and heights, attributes and metadata, which way a radius turns,
//! the three transition conventions, the other strings and the surfaces.

#![allow(
    clippy::module_inception,
    clippy::excessive_precision,
    reason = "Inner module names identify the C++ suites for the parity checker; decimal constants are copied verbatim from the C++ oracle."
)]

pub mod support;

use std::f64::consts::PI;

use katana_archive12d::domain::{nearest_standard_colour, standard_colour, standard_colour_names};
use katana_archive12d::{Archive, ImportOptions, layer_path_for_model, to_domain};
use katana_core::text_encoding::is_valid_utf8;
use katana_entity::layer_path::{MAXIMUM_LAYER_DEPTH, layer_depth, validate_layer_path};
use katana_entity::{Color, PointGeometry, TextGeometry};
use katana_geometry::{Arc2, Circle2, Point2, Polyline2, Segment2, Vec2};

use support::{
    all_warnings, any_warning_contains, holds, import, import_with, near, prop_integer, prop_real,
    prop_text, shape,
};

fn degrees_of(from: Point2, to: Point2) -> f64 {
    let value = (to.y - from.y).atan2(to.x - from.x) * 180.0 / PI;
    if value < 0.0 { value + 360.0 } else { value }
}

fn rgba(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b, 255)
}

// ---- layers ------------------------------------------------------------------

mod layer_path {
    use super::*;

    #[test]
    fn a_tree_name_is_already_a_layer_path() {
        // Manual 1.1: "Stage 1/Water/Drainage" is an object-tree model name.
        assert_eq!(
            layer_path_for_model("Stage 1/Water/Drainage", ""),
            "Stage 1/Water/Drainage"
        );
        assert_eq!(
            layer_path_for_model("Kerb", "survey/2024"),
            "survey/2024/Kerb"
        );
    }

    #[test]
    fn what_a_layer_path_cannot_hold_is_replaced_not_refused() {
        // The fallback is the name the format's own tests hold, kept as the
        // C++ writes it.
        assert_eq!(layer_path_for_model("", ""), "12d");
        assert_eq!(layer_path_for_model("   ", ""), "12d");
        assert_eq!(layer_path_for_model("/a//b/", ""), "a/b");
        assert_eq!(layer_path_for_model("  padded  / name ", ""), "padded/name");
        assert_eq!(layer_path_for_model("a\\b", ""), "a_b");
        assert_eq!(layer_path_for_model("../etc", ""), "_/etc");
        assert_eq!(layer_path_for_model("tab\there", ""), "tab_here");
    }

    #[test]
    fn every_result_is_a_valid_layer_path_however_hostile_the_name() {
        let mut deep = String::new();
        for i in 0..40 {
            deep.push_str(&format!("level{i}/"));
        }
        let long_name = "x".repeat(2000);
        // Two-byte characters, so that a cut at the length limit can land
        // inside one.
        let accented = "\u{e9}".repeat(400);
        let mut deeper_than_the_limit = 0;
        for name in [
            deep.clone(),
            long_name.clone(),
            accented,
            String::from("."),
            String::from("a/./b"),
            String::from("\u{1}\u{2}"),
            format!("x/{long_name}"),
        ] {
            let path = layer_path_for_model(&name, "imported");
            assert!(validate_layer_path(&path).is_ok(), "{path}");
            assert!(is_valid_utf8(path.as_bytes()));
            if name == deep {
                deeper_than_the_limit += 1;
            }
        }
        // The generator does reach the case it exists for: 40 levels against
        // a limit of MAXIMUM_LAYER_DEPTH.
        assert_eq!(deeper_than_the_limit, 1);
        assert!(deep.matches('/').count() > MAXIMUM_LAYER_DEPTH);
        assert_eq!(
            layer_depth(&layer_path_for_model(&deep, "")),
            MAXIMUM_LAYER_DEPTH
        );
    }
}

// ---- colours -------------------------------------------------------------------

mod standard_colour {
    use super::*;

    #[test]
    fn names_are_matched_without_regard_to_case_or_spelling() {
        assert_eq!(standard_colour("red"), Some(rgba(255, 0, 0)));
        assert_eq!(standard_colour("RED"), standard_colour("red"));
        assert_eq!(standard_colour("Dark Green"), standard_colour("dark_green"));
        assert_eq!(standard_colour("gray"), standard_colour("grey"));
        assert_eq!(standard_colour("light gray"), standard_colour("light grey"));
        assert!(standard_colour("pen 025").is_none());
        assert!(standard_colour("").is_none());
    }

    #[test]
    fn every_name_is_its_own_nearest_colour() {
        for name in [
            "red",
            "green",
            "blue",
            "yellow",
            "cyan",
            "magenta",
            "white",
            "black",
            "grey",
            "orange",
            "brown",
            "purple",
            "dark grey",
            "light blue",
        ] {
            let colour = standard_colour(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(nearest_standard_colour(colour), name);
        }
        assert_eq!(nearest_standard_colour(rgba(250, 5, 5)), "red");
    }

    #[test]
    fn the_listed_names_are_the_twenty_seven_the_table_knows_each_distinct_and_drawn() {
        // The code manager's colour field lists these, so each must be a name
        // standard_colour draws, and nothing it knows may be missing from
        // them: 14 plain names, 7 dark and 6 light, as the table behind these
        // names (katana_entity::colour_names: they are the entity layer's
        // functions, named here by re-exports).
        let names = standard_colour_names();
        assert_eq!(names.len(), 27);
        assert_eq!(names[0], "red");
        assert_eq!(names[names.len() - 1], "light pink");
        for name in &names {
            let colour = standard_colour(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(
                names.iter().filter(|other| *other == name).count(),
                1,
                "{name}"
            );
            assert_eq!(
                &nearest_standard_colour(colour),
                name,
                "two names for one RGB would make export's choice arbitrary"
            );
        }
    }
}

mod domain_import {
    use super::*;

    #[test]
    fn each_model_becomes_a_layer_in_first_use_order_with_its_first_colour() {
        let domain = import(
            "model \"Stage 1/Water\" string super { colour blue data_2d { 0 0 1 1 } }\n\
             string super { colour red data_2d { 0 0 1 1 } }\n\
             model \"Kerb\" string super { colour \"pen 025\" data_2d { 0 0 1 1 } }",
        );
        assert_eq!(domain.layers_needed.len(), 2);
        assert_eq!(domain.layers_needed[0].name, "Stage 1/Water");
        assert_eq!(domain.layers_needed[0].color, rgba(0, 0, 255));
        assert_eq!(domain.layers_needed[1].name, "Kerb");
        assert_eq!(domain.entities.len(), 3);
        assert_eq!(domain.entities[1].layer, "Stage 1/Water");
        assert_eq!(domain.entities[2].layer, "Kerb");
    }

    #[test]
    fn a_layer_prefix_puts_every_model_under_one_parent() {
        let options = ImportOptions {
            layer_prefix: String::from("survey"),
            ..ImportOptions::default()
        };
        let domain = import_with(
            "model \"Kerb\" string super { data_2d { 0 0 1 1 } }",
            &options,
        );
        assert_eq!(domain.entities[0].layer, "survey/Kerb");
    }

    #[test]
    fn a_standard_colour_colours_the_entity_and_any_other_is_left_to_the_layer_but_kept() {
        let domain = import(
            "string super { colour yellow data_2d { 0 0 1 1 } }\n\
             string super { colour \"vis concrete\" data_2d { 0 0 1 1 } }",
        );
        assert_eq!(domain.entities.len(), 2);
        assert_eq!(domain.entities[0].color, Some(rgba(255, 255, 0)));
        assert!(domain.entities[1].color.is_none());
        assert_eq!(
            prop_text(&domain.entities[1].metadata, "12d.colour"),
            "vis concrete"
        );
    }

    // ---- geometry and heights ---------------------------------------------------------

    #[test]
    fn custom_colour_names_reach_lines_layers_symbols_and_text_through_the_one_resolver() {
        let mut options = ImportOptions::default();
        options
            .colours
            .add("test utility amber", rgba(224, 128, 32))
            .expect("a colour of the customisation's own");
        options
            .colours
            .add("test utility green", rgba(32, 112, 80))
            .expect("a colour of the customisation's own");
        let domain = import_with(
            r#"
        model Lines
        string super { colour "TEST_UTILITY_AMBER" data_2d { 0 0 10 0 } }
        string super { colour "test utility amber" data_2d { 2 3 }
          symbol_value { style Mark colour "test utility green" size 1 } }
        string text { colour red x 4 y 5 text Note text_colour "test utility green" }
        string super { colour blue data_2d { 6 7 } }
        string super { colour "missing project colour" data_2d { 8 9 } }
    "#,
            &options,
        );
        assert_eq!(domain.entities.len(), 5);
        assert_eq!(domain.entities[0].color, Some(rgba(224, 128, 32)));
        assert_eq!(domain.layers_needed[0].color, rgba(224, 128, 32));
        assert_eq!(domain.entities[1].color, Some(rgba(32, 112, 80)));
        assert_eq!(domain.entities[2].color, Some(rgba(32, 112, 80)));
        assert_eq!(domain.entities[3].color, standard_colour("blue"));
        assert!(domain.entities[4].color.is_none());
        assert!(any_warning_contains(&domain, "missing project colour"));
        assert_eq!(
            prop_text(&domain.entities[0].metadata, "12d.colour"),
            "TEST_UTILITY_AMBER"
        );
    }

    #[test]
    fn one_vertex_is_a_point_two_a_line_more_a_polyline() {
        let domain = import(
            "string super { data_3d { 1 2 3 } }\n\
             string super { data_3d { 1 2 3  4 6 3 } }\n\
             string super { closed true data_3d { 0 0 1  4 0 1  4 3 1 } }",
        );
        assert_eq!(domain.entities.len(), 3);
        assert_eq!(
            shape::<PointGeometry>(&domain.entities[0].geometry).position,
            Point2::new(1.0, 2.0)
        );
        let line = shape::<Segment2>(&domain.entities[1].geometry);
        assert_eq!(line.end, Point2::new(4.0, 6.0));
        let polyline = shape::<Polyline2>(&domain.entities[2].geometry);
        assert!(polyline.closed);
        assert_eq!(polyline.vertices.len(), 3);
        // A 3-4-5 triangle: the bounds of everything imported.
        assert_eq!(domain.bounds.min, Point2::new(0.0, 0.0));
        assert_eq!(domain.bounds.max, Point2::new(4.0, 6.0));
    }

    #[test]
    fn vertices_all_in_one_place_are_a_point_not_a_failed_import() {
        // A zero-length polyline is invalid, and entities are created in ONE
        // atomic command: left alone, this string would cost the whole file.
        let domain = import(
            "string super { name \"dup\" data_3d { 5 5 1  5 5 1  5 5 1 } }\n\
             string super { data_3d { 0 0 0  1 1 0 } }",
        );
        assert_eq!(domain.entities.len(), 2);
        assert!(holds::<PointGeometry>(&domain.entities[0].geometry));
    }

    #[test]
    fn a_string_with_no_vertices_is_skipped_and_said_to_be() {
        let domain =
            import("string super { name \"empty\" }\nstring super { data_2d { 0 0 1 1 } }");
        assert_eq!(domain.entities.len(), 1);
        assert!(
            any_warning_contains(&domain, "\"empty\""),
            "{}",
            all_warnings(&domain)
        );
        assert_eq!(domain.tally.len(), 1);
        assert_eq!(domain.tally[0].read, 2);
        assert_eq!(domain.tally[0].imported, 1);
    }

    #[test]
    fn one_height_is_elevation_and_differing_heights_are_listed_with_their_nulls() {
        let domain = import(
            "string super { data_3d { 0 0 7.5  1 1 7.5 } }\n\
             string super { data_3d { 0 0 1.5  1 1 null  2 2 2.25 } }\n\
             string super { z 31.25 data_2d { 0 0  1 1 } }\n\
             string super { data_2d { 0 0  1 1 } }",
        );
        assert_eq!(domain.entities.len(), 4);
        assert_eq!(prop_real(&domain.entities[0].properties, "elevation"), 7.5);
        assert!(!domain.entities[0].properties.contains_key("elevations"));
        assert_eq!(
            prop_text(&domain.entities[1].properties, "elevations"),
            "1.5 null 2.25"
        );
        assert!(!domain.entities[1].properties.contains_key("elevation"));
        assert_eq!(
            prop_real(&domain.entities[2].properties, "elevation"),
            31.25
        );
        assert!(
            domain.entities[3].properties.is_empty(),
            "no height is not a height of zero"
        );
    }

    #[test]
    fn an_origin_shift_moves_plan_coordinates_and_leaves_heights_alone() {
        let options = ImportOptions {
            origin_shift: Some(Vec2::new(502000.0, 6960000.0)),
            ..ImportOptions::default()
        };
        let domain = import_with("string super { data_3d { 502010 6960020 32.5 } }", &options);
        assert_eq!(
            shape::<PointGeometry>(&domain.entities[0].geometry).position,
            Point2::new(10.0, 20.0)
        );
        assert_eq!(prop_real(&domain.entities[0].properties, "elevation"), 32.5);
    }

    // ---- attributes and metadata ------------------------------------------------------------

    #[test]
    fn attributes_become_typed_properties_and_groups_flatten_into_paths() {
        let domain = import(
            r#"string super { attributes {
  text "Owner" "Telstra"  integer "Ways" 4  real "Size" 0.125
  group { name "Asset" attributes { text "Type" "Conduit" group { name "Dim" attributes { real "Cover" 0.9 } } } }
  text "Owner" "second of the same name"
} data_2d { 0 0 1 1 } }"#,
        );
        let p = &domain.entities[0].properties;
        assert_eq!(prop_text(p, "Owner"), "Telstra");
        assert_eq!(prop_integer(p, "Ways"), 4);
        assert_eq!(prop_real(p, "Size"), 0.125);
        assert_eq!(prop_text(p, "Asset/Type"), "Conduit");
        assert_eq!(prop_real(p, "Asset/Dim/Cover"), 0.9);
        assert_eq!(prop_text(p, "Owner (2)"), "second of the same name");
    }

    #[test]
    fn what_only_twelve_d_knows_about_a_string_travels_in_its_metadata() {
        let options = ImportOptions {
            source_name: String::from("job.12da"),
            ..ImportOptions::default()
        };
        let domain = import_with(
            "model \"M\" string super { name \"Kerb BL-047\" style \"Kerb\" colour cyan \
             chainage 12.5 breakline line point_data { 101 \"A 7\" } data_3d { 0 0 1  1 1 1 } \
             pipe_value { diameter 0.3 } justify invert }",
            &options,
        );
        let m = &domain.entities[0].metadata;
        assert_eq!(prop_text(m, "12d.element"), "string super");
        assert_eq!(prop_text(m, "12d.name"), "Kerb BL-047");
        assert!(
            !m.contains_key("12d.style"),
            "the style is the entity's, not metadata"
        );
        assert_eq!(domain.entities[0].style, "Kerb");
        assert_eq!(domain.styles_needed.len(), 1);
        assert_eq!(domain.styles_needed[0].name, "Kerb");
        assert_eq!(domain.styles_needed[0].description, "library linestyle");
        // The LINESTYLE NAME, which is what the line should be drawn with. It
        // used to be left as "continuous", so the name reached nowhere a
        // renderer would look and a loaded linestyle library could never be
        // matched against it (PLAN.MD 20.3).
        assert_eq!(domain.styles_needed[0].linetype, "Kerb");
        assert_eq!(prop_text(m, "12d.colour"), "cyan");
        assert_eq!(prop_real(m, "12d.chainage"), 12.5);
        assert_eq!(prop_text(m, "12d.breakline"), "line");
        assert_eq!(prop_text(m, "12d.point_ids"), "101,A 7");
        assert_eq!(prop_real(m, "12d.diameter"), 0.3);
        assert_eq!(prop_text(m, "12d.justify"), "invert");
        assert_eq!(prop_text(m, "source"), "job.12da");
    }

    // ---- the other strings ------------------------------------------------------------------------------

    #[test]
    fn an_arc_strings_radius_says_which_way_round_it_goes() {
        // Centre (0,0), from (10,0) to (0,10). Counter-clockwise that is a
        // quarter turn; clockwise it is the other three quarters.
        let domain = import(
            "string arc { radius -10 xcentre 0 ycentre 0 zcentre 1 xstart 10 ystart 0 zstart 1 \
             xend 0 yend 10 zend 1 }\n\
             string arc { radius 10 xcentre 0 ycentre 0 zcentre 1 xstart 10 ystart 0 zstart 1 \
             xend 0 yend 10 zend 3 }",
        );
        assert_eq!(domain.entities.len(), 2);
        let quarter = shape::<Arc2>(&domain.entities[0].geometry);
        near(quarter.sweep, PI / 2.0, 1e-12);
        assert_eq!(quarter.radius, 10.0);
        assert_eq!(prop_real(&domain.entities[0].properties, "elevation"), 1.0);
        let rest = shape::<Arc2>(&domain.entities[1].geometry);
        near(rest.sweep, -1.5 * PI, 1e-12);
        assert_eq!(
            prop_text(&domain.entities[1].properties, "elevations"),
            "1 3"
        );
    }

    #[test]
    fn circles_features_and_text_arrive_as_themselves() {
        let domain = import(
            "string circle { radius 8 xcentre 650 ycentre 200 zcentre 32 }\n\
             string feature { radius 3 xcentre 700 ycentre 250 zcentre 31 }\n\
             string text { worldsize 2.7 angle 90 x 5 y 6 z null text \"SS112061\" }\n\
             string text { papersize 3 x 1 y 1 text \"paper\" }\n\
             string text { x 1 y 1 text \"\" }",
        );
        assert_eq!(domain.entities.len(), 4, "empty text is not an entity");
        assert_eq!(shape::<Circle2>(&domain.entities[0].geometry).radius, 8.0);
        assert_eq!(prop_real(&domain.entities[1].properties, "elevation"), 31.0);
        let text = shape::<TextGeometry>(&domain.entities[2].geometry);
        assert_eq!(text.text, "SS112061");
        assert_eq!(text.height, 2.7);
        near(text.rotation, PI / 2.0, 1e-15);
        // Millimetres on paper are not model units; the model's default stands
        // in.
        assert_eq!(
            shape::<TextGeometry>(&domain.entities[3].geometry).height,
            TextGeometry::default().height
        );
        assert!(
            any_warning_contains(&domain, "text is empty"),
            "{}",
            all_warnings(&domain)
        );
    }

    #[test]
    fn the_text_on_the_vertices_of_a_string_becomes_text_entities() {
        let domain = import(
            "string 4d { worldsize 1.5 data { 0 0 1 \"GUM-01\"  5 5 2 \"\"  9 9 3 \"GUM-03\" } }",
        );
        assert_eq!(domain.entities.len(), 3);
        assert!(holds::<Polyline2>(&domain.entities[0].geometry));
        let last = shape::<TextGeometry>(&domain.entities[2].geometry);
        assert_eq!(last.text, "GUM-03");
        assert_eq!(last.position, Point2::new(9.0, 9.0));
        assert_eq!(last.height, 1.5);
        assert_eq!(prop_real(&domain.entities[2].properties, "elevation"), 3.0);
    }

    #[test]
    fn a_plot_frame_is_its_sheet_on_the_ground() {
        // A3, 420 x 297 mm, at 1:500: 0.420 * 500 = 210 m by 0.297 * 500 =
        // 148.5 m.
        let domain = import(
            "string plot_frame { name \"Sheet 01\" width 420 height 297 scale 500 \
             rotation 0 xorigin 1000 yorigin 2000 }\n\
             string plot_frame { width 100 height 50 scale 1000 rotation 90 xorigin 0 yorigin 0 }",
        );
        assert_eq!(domain.entities.len(), 2);
        let sheet = shape::<Polyline2>(&domain.entities[0].geometry);
        assert!(sheet.closed);
        assert_eq!(sheet.vertices.len(), 4);
        assert_eq!(sheet.vertices[0], Point2::new(1000.0, 2000.0));
        assert_eq!(sheet.vertices[2], Point2::new(1210.0, 2148.5));
        assert_eq!(
            prop_integer(&domain.entities[0].properties, "plot_frame.scale"),
            500
        );
        // Turned a quarter counter-clockwise, its long side runs up the page.
        let turned = shape::<Polyline2>(&domain.entities[1].geometry);
        near(turned.vertices[1].x, 0.0, 1e-9);
        near(turned.vertices[1].y, 100.0, 1e-9);
        near(turned.vertices[2].x, -50.0, 1e-9);
    }

    #[test]
    fn a_drainage_string_is_a_line_carrying_its_pipes_and_a_point_for_each_pit() {
        let domain = import(
            r#"model "Drainage" string drainage { name "SW Line A" outfall 28.5 flow_direction 1
  data { 0 0 29 0 0   50 20 29.5 0 0 }
  pit { name "A1" type "Grated" diameter 1.05 x 0 y 0 z 31 }
  pit { name "A2" x 50 y 20 z 31.5 }
  pipe { name "A1-A2" diameter 0.375 us_level 29.5 }
  house_connection { name "HC1" x 25 y 10 z 29.2 }
}"#,
        );
        assert_eq!(domain.entities.len(), 4);
        let line = &domain.entities[0];
        assert!(holds::<Segment2>(&line.geometry));
        assert_eq!(prop_real(&line.properties, "outfall"), 28.5);
        assert_eq!(prop_text(&line.properties, "pipe.1.name"), "A1-A2");
        assert_eq!(prop_real(&line.properties, "pipe.1.diameter"), 0.375);
        let pit = &domain.entities[1];
        assert_eq!(
            shape::<PointGeometry>(&pit.geometry).position,
            Point2::new(0.0, 0.0)
        );
        assert_eq!(prop_text(&pit.properties, "pit.name"), "A1");
        assert_eq!(prop_real(&pit.properties, "pit.diameter"), 1.05);
        assert_eq!(prop_real(&pit.properties, "elevation"), 31.0);
        assert!(
            !pit.properties.contains_key("pit.x"),
            "where it is, is its geometry"
        );
        assert_eq!(
            prop_text(&domain.entities[3].properties, "house_connection.name"),
            "HC1"
        );
        assert_eq!(
            prop_text(&domain.entities[3].metadata, "12d.element"),
            "drainage house_connection"
        );
    }

    #[test]
    fn super_tins_clouds_and_trimeshes_are_each_accounted_for() {
        let domain = import(
            r#"super_tin { name "COMBINED" tins { "A" "B" } }
string las_cloud_data { name "Scan" data { format v10_p0 points_v10_p0 { p { x 1 y 2 z 3 i 900 cl 2 } } } }
string las_cloud_data { name "Ref" ref_data { file_name "site.las" } }
primitive_3d { name M trimesh_3d { vertices { 0 0 0 1 0 0 0 1 0 } faces { 1 2 3 } } }"#,
        );
        assert_eq!(domain.super_tins.len(), 1);
        assert_eq!(domain.super_tins[0].tins.len(), 2);
        assert_eq!(domain.clouds.len(), 2);
        assert_eq!(domain.clouds[0].points.len(), 1);
        assert_eq!(domain.clouds[0].points[0].z, 3.0);
        assert_eq!(domain.clouds[0].points[0].intensity, 900);
        assert_eq!(domain.clouds[0].points[0].classification, 2);
        assert_eq!(domain.clouds[1].reference_file, "site.las");
        // The trimesh becomes a mesh in the session (PLAN.MD 20.2 slice 4).
        assert_eq!(domain.meshes.len(), 1);
        assert_eq!(domain.meshes[0].name, "M");
        assert_eq!(domain.meshes[0].mesh.triangle_count(), 1);
        // The tally says the same thing in numbers.
        let mesh = domain
            .tally
            .iter()
            .find(|tally| tally.keyword == "primitive_3d")
            .expect("a tally of the trimeshes");
        assert_eq!(mesh.read, 1);
        assert_eq!(mesh.imported, 1);
    }

    #[test]
    fn what_the_reader_did_not_read_is_said_in_the_imports_warnings() {
        let domain = import("string super { data_2d { 0 0 1 1 } drawables { } }");
        assert!(
            any_warning_contains(&domain, "not read: string super/drawables (1)"),
            "{}",
            all_warnings(&domain)
        );
    }

    #[test]
    fn bad_options_are_refused() {
        let archive = Archive::default();
        let options = ImportOptions {
            curve_tolerance: 0.0,
            ..ImportOptions::default()
        };
        assert!(to_domain(&archive, &options).is_err());
        let options = ImportOptions {
            alignment_tolerance: -1.0,
            ..ImportOptions::default()
        };
        assert!(to_domain(&archive, &options).is_err());
        let options = ImportOptions {
            layer_prefix: String::from("bad//prefix"),
            ..ImportOptions::default()
        };
        assert!(to_domain(&archive, &options).is_err());
        let options = ImportOptions {
            origin_shift: Some(Vec2::new(f64::NAN, 0.0)),
            ..ImportOptions::default()
        };
        assert!(to_domain(&archive, &options).is_err());
        assert!(
            to_domain(&archive, &ImportOptions::default()).is_ok(),
            "an empty archive is an empty import"
        );
    }
}

// ---- arcs: which way a radius turns ------------------------------------------------------------

fn arc_from_origin_to_two_zero(radius: &str, major: &str) -> Polyline2 {
    let domain = import(&format!(
        "string super {{ data_2d {{ 0 0  2 0 }} radius_data {{ {radius} }} major_data {{ {major} }} }}"
    ));
    assert_eq!(domain.entities.len(), 1);
    match &domain.entities[0].geometry {
        katana_entity::Geometry::Polyline(polyline) => polyline.clone(),
        other => panic!("an arc chords into more than two vertices: {other:?}"),
    }
}

mod domain_import_arcs {
    use super::*;

    #[test]
    fn a_positive_radius_bulges_above_the_line_and_a_negative_one_below() {
        // Manual 1.5.8.2.2.2: "+ve radius puts the arc's bulge above the line
        // connecting the vertices". From (0,0) to (2,0) with radius 1 the arc
        // is the unit semicircle about (1,0): above for +1, below for -1.
        for sign in [1.0, -1.0] {
            let arc = arc_from_origin_to_two_zero(if sign > 0.0 { "1" } else { "-1" }, "0");
            assert!(arc.vertices.len() > 4);
            assert_eq!(arc.vertices[0], Point2::new(0.0, 0.0));
            assert_eq!(arc.vertices[arc.vertices.len() - 1], Point2::new(2.0, 0.0));
            // Each vertex strictly between the ends, with the one before it.
            for pair in arc.vertices[..arc.vertices.len() - 1].windows(2) {
                let (previous, vertex) = (pair[0], pair[1]);
                near(vertex.distance_to(Point2::new(1.0, 0.0)), 1.0, 1e-12);
                assert!(vertex.y * sign > 0.0);
                // and it runs from the first vertex to the second, not back
                // again
                assert!(vertex.x > previous.x);
            }
        }
    }

    #[test]
    fn major_takes_the_larger_of_the_two_arcs_on_the_same_side() {
        // Radius sqrt 2 over a chord of 2: the centre is 1 from the chord, so
        // the minor arc rises to sqrt 2 - 1 above it and the major one to
        // sqrt 2 + 1.
        let root2 = 2.0_f64.sqrt();
        let height = |arc: &Polyline2| {
            let mut top = 0.0;
            for vertex in &arc.vertices {
                if top < vertex.y {
                    top = vertex.y;
                }
            }
            top
        };
        // The default chord tolerance is 1 mm, so the highest VERTEX is within
        // that of the top of the arc.
        near(
            height(&arc_from_origin_to_two_zero("1.4142135623730951", "0")),
            root2 - 1.0,
            1e-3,
        );
        near(
            height(&arc_from_origin_to_two_zero("1.4142135623730951", "1")),
            root2 + 1.0,
            1e-3,
        );
    }

    #[test]
    fn chords_stay_within_the_stated_tolerance_of_the_arc() {
        let options = ImportOptions {
            curve_tolerance: 0.01,
            ..ImportOptions::default()
        };
        let domain = import_with(
            "string super { data_2d { 0 0  200 0 } radius_data { 100 } major_data { 0 } }",
            &options,
        );
        let arc = shape::<Polyline2>(&domain.entities[0].geometry);
        for pair in arc.vertices.windows(2) {
            // The middle of a chord is where it is furthest from the arc.
            let mid = pair[0] + (pair[1] - pair[0]) * 0.5;
            let sagitta = 100.0 - mid.distance_to(Point2::new(100.0, 0.0));
            assert!(sagitta >= 0.0);
            assert!(sagitta <= 0.01 + 1e-12);
        }
        assert!(
            any_warning_contains(&domain, "1 arcs and 0 transitions were chorded"),
            "{}",
            all_warnings(&domain)
        );
    }

    #[test]
    fn heights_are_interpolated_for_the_vertices_chording_adds() {
        let domain = import(
            "string super { data_3d { 0 0 10  2 0 20 } radius_data { 1 } major_data { 0 } }",
        );
        let arc = shape::<Polyline2>(&domain.entities[0].geometry);
        // Read as the C++ reads it, with a stream: numbers until one is not.
        let heights: Vec<f64> = prop_text(&domain.entities[0].properties, "elevations")
            .split_whitespace()
            .map_while(|item| item.parse::<f64>().ok())
            .collect();
        assert_eq!(heights.len(), arc.vertices.len());
        assert_eq!(heights[0], 10.0);
        assert_eq!(heights[heights.len() - 1], 20.0);
        for pair in heights.windows(2) {
            assert!(pair[1] > pair[0]);
        }
        // Equal chords of a semicircle: height rises linearly with vertex
        // number.
        let step = 10.0 / (heights.len() - 1) as f64;
        near(heights[1], 10.0 + step, 1e-9);
    }

    #[test]
    fn a_radius_that_cannot_span_its_chord_is_drawn_straight_and_counted() {
        let domain =
            import("string super { data_2d { 0 0  10 0 } radius_data { 2 } major_data { 0 } }");
        assert!(holds::<Segment2>(&domain.entities[0].geometry));
        assert!(
            any_warning_contains(&domain, "1 curved segments could not be reproduced"),
            "{}",
            all_warnings(&domain)
        );
    }
}

// ---- transitions: three conventions settled against the writing program's own output --------
//
// The numbers below are copied from `test Super Alignment.12da`, written by
// the writing program's version 15.0C1t: two consecutive vertices of a
// horizontal_data block and the `spiral` block between them. They are the
// external authority these tests rest on - the program computed both ends, so
// a reading of the parameters is right exactly when the curve it produces
// runs from one to the other.

mod domain_import_transitions {
    use super::*;

    #[test]
    fn a_leading_cubic_parabola_lands_on_the_vertex_twelve_d_recorded() {
        let options = ImportOptions {
            curve_tolerance: 1e-6,
            ..ImportOptions::default()
        };
        let domain = import_with(
            r#"string super { data_2d { 1342.85432099 1789.30813759  1419.06779208 1813.40212949 }
 geometry_data { spiral { type "cubic parabola" leading 1 l1 0 r1 0 a1 20.20620846 l2 80 r2 294.56299992 a2 12.2642972 } } }"#,
            &options,
        );
        let curve = shape::<Polyline2>(&domain.entities[0].geometry);
        assert!(curve.vertices.len() > 10);
        // It leaves along a1 and arrives along a2 - which a clothoid of the
        // same length and radius does not: that ends at 12.4258 degrees, 0.16
        // out. A chord differs from the tangent by half the angle it subtends:
        // at a sagitta of 1e-6 on R = 294 a chord is 0.1 m long and that is
        // 0.01 degrees.
        near(
            degrees_of(curve.vertices[0], curve.vertices[1]),
            20.20620846,
            0.02,
        );
        let n = curve.vertices.len();
        near(
            degrees_of(curve.vertices[n - 2], curve.vertices[n - 1]),
            12.2642972,
            0.02,
        );
        // Positive radius: it turns to the right, so the heading falls.
        assert!(
            !any_warning_contains(&domain, "moved by"),
            "{}",
            all_warnings(&domain)
        );
        assert!(
            !any_warning_contains(&domain, "could not be reproduced"),
            "{}",
            all_warnings(&domain)
        );
        near(curve.length(), 80.0, 1e-3); // l2 - l1 is the length ALONG the curve
    }

    #[test]
    fn a_trailing_transition_is_described_backwards_from_its_straight_end() {
        // leading 0, yet l1 = 0 and r1 = 0: the manual says a full trailing
        // transition has l2 = 0 and r2 = 0. a1 = 200.2 is the tangent at the
        // SECOND vertex pointing back; the string itself runs at 20.2 there.
        let options = ImportOptions {
            curve_tolerance: 1e-6,
            ..ImportOptions::default()
        };
        let domain = import_with(
            r#"string super { data_2d { 1202.63472783 1741.58956859  1278.83135325 1765.74439519 }
 geometry_data { spiral { type "cubic parabola" leading 0 l1 0 r1 0 a1 200.20620846 l2 80 r2 299.38949171 a2 192.39768555 } } }"#,
            &options,
        );
        let katana_entity::Geometry::Polyline(curve) = &domain.entities[0].geometry else {
            panic!(
                "read the manual's way it misses by 160 m and is drawn straight{}",
                all_warnings(&domain)
            );
        };
        let n = curve.vertices.len();
        near(
            degrees_of(curve.vertices[0], curve.vertices[1]),
            192.39768555 - 180.0,
            0.02,
        );
        near(
            degrees_of(curve.vertices[n - 2], curve.vertices[n - 1]),
            200.20620846 - 180.0,
            0.02,
        );
        assert!(
            !any_warning_contains(&domain, "moved by"),
            "{}",
            all_warnings(&domain)
        );
        assert!(
            !any_warning_contains(&domain, "could not be reproduced"),
            "{}",
            all_warnings(&domain)
        );
    }

    #[test]
    fn a_transition_type_with_no_definition_is_approximated_and_the_error_stated() {
        // The same leading transition, relabelled as a type the manual names
        // and does not define. Drawn as a clothoid it ends 0.097 m from the
        // recorded vertex (integrated independently, in Python, while
        // establishing the conventions above); it is moved to meet it and the
        // movement reported.
        let domain = import(
            r#"string super { data_2d { 1342.85432099 1789.30813759  1419.06779208 1813.40212949 }
 geometry_data { spiral { type "bloss" leading 1 l1 0 r1 0 a1 20.20620846 l2 80 r2 294.56299992 a2 12.2642972 } } }"#,
        );
        let curve = shape::<Polyline2>(&domain.entities[0].geometry);
        assert_eq!(
            curve.vertices[curve.vertices.len() - 1],
            Point2::new(1419.06779208, 1813.40212949)
        );
        assert!(
            any_warning_contains(
                &domain,
                "\"bloss\" are drawn as clothoids and moved by up to 0.097"
            ),
            "{}",
            all_warnings(&domain)
        );
    }

    #[test]
    fn a_transition_that_misses_its_end_by_metres_is_not_drawn_as_if_it_fitted() {
        // a1 points the wrong way entirely.
        let domain = import(
            r#"string super { data_2d { 0 0  80 5 }
 geometry_data { spiral { type clothoid leading 1 l1 0 r1 0 a1 180 l2 80 r2 300 a2 170 } } }"#,
        );
        assert!(holds::<Segment2>(&domain.entities[0].geometry));
        assert!(
            any_warning_contains(&domain, "1 curved segments could not be reproduced"),
            "{}",
            all_warnings(&domain)
        );

        // A length that cannot be a transition between these two vertices:
        // shorter than the chord, or ten times longer than it (a spiral
        // turning less than a half turn is under pi/2 chords long). Found by
        // fuzzing: a length of 1e300 passed the closure check because the
        // allowed miss grew with it.
        for length in ["5", "1e300", "200"] {
            let absurd = import(&format!(
                "string super {{ data_2d {{ 0 0  10 10 }} geometry_data {{ spiral {{ \
                 type clothoid leading 1 l1 0 r1 0 a1 45 l2 {length} r2 1e-300 a2 45 }} }} }}"
            ));
            assert!(holds::<Segment2>(&absurd.entities[0].geometry), "{length}");
            assert!(
                any_warning_contains(&absurd, "could not be reproduced"),
                "{length}{}",
                all_warnings(&absurd)
            );
        }
    }
}

// ---- surfaces --------------------------------------------------------------------------------------

mod domain_import_tin {
    use super::*;

    #[test]
    fn only_visible_non_construction_triangles_make_the_surface() {
        let domain = import(
            r#"full_tin { name "GROUND" colour green
  points { -100 -100 0  -100 100 0  100 100 0  100 -100 0   0 0 5  10 0 6  0 10 7  10 10 8 }
  triangles { 5 7 6   6 7 8   1 5 6 }
  neighbours { 0 2 3   1 0 0   0 1 0 }
  nulling { 2 1 2 }
}"#,
        );
        assert_eq!(domain.surfaces.len(), 1);
        let ground = &domain.surfaces[0];
        assert_eq!(ground.name, "GROUND");
        assert_eq!(ground.triangles_in_file, 3);
        assert_eq!(
            ground.triangles_nulled, 2,
            "one nulled, one touching a construction point"
        );
        assert_eq!(ground.surface.triangle_count(), 1);
        assert_eq!(
            ground.surface.vertex_count(),
            3,
            "the construction points are not carried along"
        );
        // The plane through (0,0,5), (10,0,6), (0,10,7) is z = 5 + 0.1x + 0.2y.
        let z = ground
            .surface
            .elevation_at_xy(2.0, 2.0)
            .expect("inside the visible triangle");
        near(z, 5.6, 1e-12);
        assert!(
            ground.surface.elevation_at_xy(8.0, 8.0).is_none(),
            "inside the nulled triangle"
        );
        assert!(domain.entities.is_empty());
        assert_eq!(domain.bounds.max, Point2::new(10.0, 10.0));
    }

    #[test]
    fn triangles_are_accepted_whichever_way_round_the_writer_listed_them() {
        // The manual wants clockwise. (0,0) (0,10) (10,0) is; (10,0) (10,10)
        // (0,10) is not - its doubled signed area is 0*10 - 10*(-10) = +100.
        let domain = import(
            "tin { name \"T\" points { 0 0 1  10 0 1  0 10 1  10 10 1 } triangles { 1 3 2   2 4 3 } }",
        );
        assert_eq!(domain.surfaces.len(), 1, "{}", all_warnings(&domain));
        assert_eq!(domain.surfaces[0].surface.triangle_count(), 2);
        near(domain.surfaces[0].surface.plan_area(), 100.0, 1e-9);
    }

    #[test]
    fn a_triangle_with_no_area_in_plan_has_no_surface_to_give_either() {
        // (0,0) (10,0) (20,0) are collinear: no ground under the triangle, and
        // no side for it to have been listed from. Left in, its edge 2-3 runs
        // the same way round as the neighbour's below the line, and one such
        // pair refuses the whole surface - which is how four surfaces of
        // plot_PW_example_data.12da were lost.
        let domain = import(
            "tin { name \"T\" points { 0 0 1  10 0 1  20 0 1  15 -5 2 } triangles { 1 2 3   3 2 4 } }",
        );
        assert_eq!(domain.surfaces.len(), 1, "{}", all_warnings(&domain));
        assert_eq!(domain.surfaces[0].surface.triangle_count(), 1);
        assert_eq!(domain.surfaces[0].triangles_nulled, 1);
    }

    #[test]
    fn a_triangle_with_a_null_height_has_no_surface_to_give() {
        let domain = import(
            "tin { name \"T\" points { 0 0 1  10 0 1  0 10 1  10 10 null } triangles { 1 3 2   2 3 4 } }",
        );
        assert_eq!(domain.surfaces.len(), 1);
        assert_eq!(domain.surfaces[0].surface.triangle_count(), 1);
        assert_eq!(domain.surfaces[0].triangles_nulled, 1);
    }

    #[test]
    fn a_tin_with_nothing_visible_is_skipped_not_imported_empty() {
        let domain = import(
            "full_tin { name \"T\" points { 0 0 0 0 9 0 9 9 0 9 0 0  1 1 1 2 1 1 1 2 1 } \
             triangles { 5 7 6 } neighbours { 0 0 0 } nulling { 1 } }",
        );
        assert!(domain.surfaces.is_empty());
        assert!(
            any_warning_contains(&domain, "no visible triangles"),
            "{}",
            all_warnings(&domain)
        );
        assert_eq!(domain.tally[0].imported, 0);
    }
}
