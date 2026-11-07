//! The twin of tests/archive12d/test_review_findings.cpp: regression tests
//! for the defects an adversarial review of the module found (recorded in
//! docs/interop.md). Each test failed against the code as first written and
//! passes with its fix; the case that exposed each one is kept as the review
//! reported it.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use katana_archive12d::{
    CircleString, DomainImport, DrainageString, Element, ExportOptions, ImportOptions, LasCloud,
    PlotFrame, TextString, Tin, VertexString, WriteOptions, from_domain, split_list, to_domain,
    write_archive,
};
use katana_entity::{Model, Style, TextGeometry};
use katana_geometry::{Point2, Polyline2};

use support::{
    any_contains, at, attribute_integer, attribute_real, attribute_text, get, import, joined,
    prop_integer, prop_real, prop_text, read, shape,
};

// The export of a model built from an import, as the application builds it,
// written and read again.
fn exported_again(domain: &DomainImport, with_styles: bool) -> katana_archive12d::Archive {
    let mut model = Model::default();
    for layer in &domain.layers_needed {
        assert!(model.layers.add(layer.clone()).is_ok());
    }
    if with_styles {
        for style in &domain.styles_needed {
            assert!(model.styles.add(style.clone()).is_ok());
        }
    }
    for entity in &domain.entities {
        assert!(model.entities.add(entity.clone()).is_ok());
    }
    let exported = from_domain(&model, &[], &ExportOptions::default(), &[])
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    read(&write_archive(&exported.archive, &WriteOptions::default()))
}

// ---- the null value ---------------------------------------------------------------

mod review_null {
    use super::*;

    #[test]
    fn a_real_height_equal_to_the_final_null_value_survives_being_written() {
        // Read under `null -999`, string a's -1 is a height; b's, read under
        // `null -1`, is not. Written with one null value, a real -1 must not
        // become a null on the way back.
        let first = read(
            "null -999\nstring super { name a data_3d { 0 0 -1  1 1 -999 } }\n\
             null -1\nstring super { name b data_3d { 0 0 -1  1 1 -999 } }",
        );
        assert_eq!(first.elements.len(), 2);
        let a = get::<VertexString>(&first.elements[0]);
        let b = get::<VertexString>(&first.elements[1]);
        assert_eq!(a.vertices[0].z, Some(-1.0));
        assert!(a.vertices[1].z.is_none());
        assert!(b.vertices[0].z.is_none());
        assert_eq!(b.vertices[1].z, Some(-999.0));

        let second = read(&write_archive(&first, &WriteOptions::default()));
        assert_eq!(second.elements.len(), 2);
        assert!(second.elements[0] == first.elements[0]);
        assert!(second.elements[1] == first.elements[1]);
        // Neither -1 nor -999 could serve; the writer went further down.
        assert!(second.null_value < -999.0);
    }

    #[test]
    fn a_strings_own_null_value_applies_wherever_it_stands_and_to_every_string_type() {
        // Manual 1.4.5 lets a string override the null value; 1.4.6 makes
        // string commands order-free.
        let archive = read(
            "null -1\n\
             string super { name after data_3d { 0 0 -5  1 1 -1 } null_value -5 }\n\
             string text { null_value -5 x 1 y 2 z -5 text \"hi\" }\n\
             string circle { null_value -5 radius 2 xcentre 1 ycentre 2 zcentre -5 }\n\
             string drainage { null_value -5 data { 0 0 -5 0 0  10 0 -1 0 0 } }\n\
             string super { name plain data_3d { 0 0 -5  1 1 -1 } }",
        );
        assert_eq!(archive.elements.len(), 5);
        let after = get::<VertexString>(&archive.elements[0]);
        assert!(after.vertices[0].z.is_none(), "-5 is this string's null");
        assert_eq!(
            after.vertices[1].z,
            Some(-1.0),
            "and -1 is a height here, whatever the file says"
        );
        assert!(get::<TextString>(&archive.elements[1]).position.z.is_none());
        assert!(get::<CircleString>(&archive.elements[2]).centre.z.is_none());
        let drainage = get::<DrainageString>(&archive.elements[3]);
        assert!(drainage.vertices[0].z.is_none());
        assert_eq!(drainage.vertices[1].z, Some(-1.0));
        let plain = get::<VertexString>(&archive.elements[4]);
        assert_eq!(plain.vertices[0].z, Some(-5.0));
        assert!(plain.vertices[1].z.is_none());

        // Written back, the per-string value is gone and every null is the
        // file's.
        let text = write_archive(&archive, &WriteOptions::default());
        assert!(!text.contains("null_value"), "{text}");
        let back = read(&text);
        assert_eq!(back.elements.len(), 5);
        for (i, (element, returned)) in archive.elements.iter().zip(&back.elements).enumerate() {
            let mut expected = element.clone();
            // The only intended difference: the extras no longer hold
            // null_value.
            match &mut expected {
                Element::VertexString(string) => {
                    string.header.extras.take("null_value");
                }
                Element::TextString(text) => {
                    text.header.extras.take("null_value");
                }
                Element::CircleString(circle) => {
                    circle.header.extras.take("null_value");
                }
                Element::DrainageString(drainage) => {
                    drainage.header.extras.take("null_value");
                }
                _ => {}
            }
            assert!(expected == *returned, "element {i}");
        }
    }
}

// ---- documented keys always take a value ---------------------------------------------

mod review_keywords {
    use super::*;

    #[test]
    fn a_bare_word_that_is_also_a_keyword_is_still_the_value_of_a_documented_key() {
        // Manual 1.1 needs quotes only around text that is not alphanumeric,
        // so `text PIT` and `title_1 Scale` are legal - and `PIT`, `Scale`,
        // `model` are all words the reader knows as keys.
        let archive = read(
            "string text { name lbl text PIT x 1 y 2 z 3 textstyle 1 }\n\
             string text { x 100 y 200 z 3 name t2 text X angle 45 }\n\
             string plot_frame { name pf title_1 Scale title_2 Drainage width 10 height 20 \
             plotter model colour dark grey }",
        );
        assert_eq!(archive.elements.len(), 3);
        let first = get::<TextString>(&archive.elements[0]);
        assert_eq!(first.text, "PIT");
        assert_eq!(first.position, at(1.0, 2.0, Some(3.0)));
        assert_eq!(first.annotation.text("textstyle", ""), "1");
        let second = get::<TextString>(&archive.elements[1]);
        assert_eq!(second.text, "X");
        assert_eq!(second.position.x, 100.0);
        assert_eq!(second.annotation.real("angle"), Some(45.0));
        let frame = get::<PlotFrame>(&archive.elements[2]);
        assert_eq!(frame.fields.text("title_1", ""), "Scale");
        assert_eq!(frame.fields.text("title_2", ""), "Drainage");
        assert_eq!(frame.fields.real("width"), Some(10.0));
        assert_eq!(frame.fields.text("plotter", ""), "model");
        assert_eq!(frame.header.colour, "dark grey");
        assert!(!frame.fields.contains("scale"), "no scale was written");
        assert!(archive.warnings.is_empty(), "{}", joined(&archive.warnings));
    }

    #[test]
    fn a_colour_called_dark_at_the_end_of_a_header_does_not_eat_the_next_element() {
        let archive = read(
            "string super { colour dark data_2d { 0 0 } }\n\
             string super { colour dark null_value -5 data_3d { 0 0 -5 } }\n\
             colour dark\nstring super { data_2d { 1 1 } }\ntin { name T points { } triangles { } }",
        );
        assert_eq!(archive.elements.len(), 4);
        assert_eq!(
            get::<VertexString>(&archive.elements[0]).header.colour,
            "dark"
        );
        assert!(
            get::<VertexString>(&archive.elements[1]).vertices[0]
                .z
                .is_none()
        );
        assert_eq!(
            get::<VertexString>(&archive.elements[2]).header.colour,
            "dark"
        );
        assert!(matches!(archive.elements[3], Element::Tin(_)));
    }

    #[test]
    fn the_default_linestyle_is_the_manuals_and_a_string_may_override_it() {
        // Manual 1.4.3: the current linestyle defaults to "1".
        let archive = read(
            "string super { data_2d { 0 0 } }\nstyle Kerb\n\
             string super { data_2d { 0 0 } }\nstring super { style \"Own\" data_2d { 0 0 } }\n\
             string super { data_2d { 0 0 } }",
        );
        assert_eq!(archive.elements.len(), 4);
        assert_eq!(get::<VertexString>(&archive.elements[0]).header.style, "1");
        assert_eq!(
            get::<VertexString>(&archive.elements[1]).header.style,
            "Kerb"
        );
        assert_eq!(
            get::<VertexString>(&archive.elements[2]).header.style,
            "Own"
        );
        assert_eq!(
            get::<VertexString>(&archive.elements[3]).header.style,
            "Kerb"
        );
    }
}

// ---- strings that lack what defines them ---------------------------------------------

mod review_incomplete {
    use super::*;

    #[test]
    fn an_arc_circle_or_text_without_its_position_is_ignored_and_said_to_be() {
        // Manual 1.4.6: "if there is not enough recognised information to
        // define the string, the string is ignored". Not placed at the origin.
        let archive = read(
            "string arc { name a1 radius 5 }\n\
             string circle { name c1 radius 2 }\n\
             string circle { name c2 xcentre 1 ycentre 2 }\n\
             string text { name t1 text \"lost\" }\n\
             string arc { name a2 radius 5 xcentre 500000 ycentre 6000000 xstart \"500005\" \
             ystart 6000000 xend 500000 yend 6000005 }",
        );
        assert_eq!(
            archive.elements.len(),
            1,
            "only the one with every coordinate"
        );
        let kept = get::<katana_archive12d::ArcString>(&archive.elements[0]);
        assert_eq!(kept.start.x, 500005.0, "a quoted number is a number");
        assert_eq!(archive.warnings.len(), 4, "{}", joined(&archive.warnings));
        assert!(
            any_contains(&archive.warnings, "\"a1\""),
            "{}",
            joined(&archive.warnings)
        );
        assert!(
            any_contains(&archive.warnings, "\"c1\""),
            "{}",
            joined(&archive.warnings)
        );
        assert!(any_contains(
            &archive.warnings,
            "\"c2\" in model \"data\" ignored: its radius"
        ));
        assert!(any_contains(
            &archive.warnings,
            "\"t1\" in model \"data\" ignored: its position"
        ));
    }

    #[test]
    fn mandatory_blocks_that_are_absent_are_reported() {
        let archive = read(
            "full_tin { name NoNb points { 0 0 0 0 9 0 9 9 0 9 0 0 1 1 1 2 1 1 1 2 1 } \
             triangles { 5 7 6 } }\n\
             tin { name Empty }\nsuper_tin { name ST }\nsuper_tin { tins { A } }\n\
             primitive_3d { name P trimesh_3d { } }\n\
             primitive_3d { name F trimesh_3d { vertices { 0 0 0 1 0 0 0 1 0 } faces { 1 2 3 } \
             face_infos { 0 0 red x } face_flags { 1 0 2 } } }",
        );
        for expected in [
            "full_tin \"NoNb\" has no neighbours block",
            "full_tin \"NoNb\" has no nulling block",
            "tin \"Empty\" has no points block",
            "tin \"Empty\" has no triangles block",
            "super_tin \"ST\" has no tins block",
            "super_tin has no name",
            "trimesh \"P\" has no vertices block",
            "trimesh \"P\" has no faces block",
            "trimesh \"F\": face_flags names an info that does not exist",
        ] {
            assert!(
                any_contains(&archive.warnings, expected),
                "{expected}{}",
                joined(&archive.warnings)
            );
        }
    }

    #[test]
    fn modes_and_directions_that_are_not_what_the_manual_allows_are_not_invented() {
        let archive = read(
            "string interface { name i1 data { 0 0 1 1e300  5 5 1 0.5  9 9 1 -1 } }\n\
             string drainage { name d1 flow_direction 1e300 data { 0 0 1 0 0 } }",
        );
        assert_eq!(archive.elements.len(), 2);
        assert_eq!(
            get::<VertexString>(&archive.elements[0]).interface_modes,
            vec![0, 0, -1]
        );
        assert!(
            get::<DrainageString>(&archive.elements[1])
                .flow_direction
                .is_none()
        );
        assert!(
            any_contains(&archive.warnings, "2 interface modes are not -1, 0 or 1"),
            "{}",
            joined(&archive.warnings)
        );
        assert!(
            any_contains(&archive.warnings, "flow_direction must be 0 or 1"),
            "{}",
            joined(&archive.warnings)
        );
    }
}

// ---- models ---------------------------------------------------------------------------

mod review_models {
    use super::*;

    #[test]
    fn one_model_has_one_spelling_and_is_registered_only_when_something_is_in_it() {
        // Manual 1.1: case is ignored and leading/trailing spaces are dropped.
        let archive = read(
            "model \" Fred \" string super { data_2d { 0 0 } }\n\
             model \"FRED\" string super { data_2d { 0 0 } }\n\
             string super { model Fred data_2d { 0 0 } }\n\
             string super { model \"Other\" data_2d { 0 0 } }",
        );
        assert_eq!(
            archive.model_names,
            vec!["Fred", "Other"],
            "no phantom \"data\": every string named its model"
        );
        for (i, element) in archive.elements.iter().take(3).enumerate() {
            assert_eq!(get::<VertexString>(element).header.model, "Fred", "{i}");
        }
        let domain = to_domain(&archive, &ImportOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(domain.layers_needed.len(), 2);
        let text = write_archive(&archive, &WriteOptions::default());
        assert!(!text.contains("\"data\""), "{text}");
    }
}

// ---- tins -----------------------------------------------------------------------------

mod review_tin {
    use super::*;

    #[test]
    fn per_triangle_colours_of_a_tin_are_written_back_and_of_a_demoted_full_tin_filtered() {
        // Manual 1.4.7.2 gives the tin form the same colours block.
        let tin = read(
            "tin { name T points { 0 0 1  10 0 2  10 10 3  0 10 4  5 20 5 } \
             triangles { 1 4 2  2 4 3  4 5 3 } colour red colours { blue -1 \"dark green\" } }",
        );
        let back = read(&write_archive(&tin, &WriteOptions::default()));
        assert!(back.elements[0] == tin.elements[0]);

        // A full_tin without neighbours is written as a tin of its surface
        // triangles; the colours must follow the triangles that were kept.
        let full = read(
            "full_tin { name F points { 0 0 0 0 9 0 9 9 0 9 0 0  1 1 1 2 1 1 1 2 1 2 2 1 } \
             triangles { 5 7 6  6 7 8  1 5 6 } nulling { 2 1 2 } colours { blue green red } }",
        );
        let demoted = read(&write_archive(&full, &WriteOptions::default()));
        let written = get::<Tin>(&demoted.elements[0]);
        assert_eq!(written.triangles.len(), 1);
        assert_eq!(written.colours, vec!["blue"]);
    }
}

// ---- LAS ------------------------------------------------------------------------------

mod review_las {
    use super::*;

    #[test]
    fn a_colour_is_sixty_four_unsigned_bits_and_bad_fields_are_counted() {
        // 2^63 = 9223372036854775808 does not fit a signed 64-bit integer; a
        // LAS colour packs three 16-bit channels and uses the top bit when red
        // is bright. Both spellings of the same bits must read as those bits.
        let archive = read(
            r#"string las_cloud_data { name neg data { format v12_p2 points_v12_p2 {
 p { x 1 y 2 z 3 i 4 rn 1 rc 1 sd 0 fe 0 cl 2 sr 0 ud 0 id 1 c -2 }
 p { x 1 y 2 z 3 c 9223372036854775808 }
 p { x 1 y 2 z 3 c 18446744073709551615 }
 p { x 1 y 2 z 3 i 70000 rn 1.5 c abc } } } }"#,
        );
        let cloud = get::<LasCloud>(&archive.elements[0]);
        assert_eq!(cloud.points.len(), 4);
        assert_eq!(cloud.points[0].colour, 18446744073709551614);
        assert_eq!(cloud.points[1].colour, 9223372036854775808);
        assert_eq!(cloud.points[2].colour, 18446744073709551615);
        assert_eq!(cloud.points[3].intensity, 65535, "clamped, and counted");
        assert!(
            any_contains(
                &archive.warnings,
                "\"neg\": 3 point fields were not numbers or were outside"
            ),
            "{}",
            joined(&archive.warnings)
        );
        // ... and they survive the writer, which is the promise writer.rs
        // makes.
        let back = read(&write_archive(&archive, &WriteOptions::default()));
        assert_eq!(get::<LasCloud>(&back.elements[0]).points, cloud.points);
    }
}

// ---- the domain mapping -----------------------------------------------------------------

mod review_import {
    use super::*;

    #[test]
    fn a_house_connections_height_is_its_level_not_its_internal_z() {
        // Manual 1.5.3 marks z "internal use only" for house connections and
        // property controls; the format's program writes 0 there, 30 m below
        // the job.
        let domain = import(
            "string drainage { data { 0 0 1 0 0  5 5 1 0 0 } \
             pit { name P x 0 y 0 z 31 } \
             house_connection { name H level 28.9 adopted_level 28.8 x 1 y 1 z 0 } \
             house_connection { name L level 28.9 x 2 y 2 z 0 } \
             property_control { name C x 3 y 3 z 0 } }",
        );
        assert_eq!(domain.entities.len(), 5);
        assert_eq!(prop_real(&domain.entities[1].properties, "elevation"), 31.0);
        assert_eq!(prop_real(&domain.entities[2].properties, "elevation"), 28.8);
        assert_eq!(prop_real(&domain.entities[3].properties, "elevation"), 28.9);
        assert!(!domain.entities[4].properties.contains_key("elevation"));
    }

    #[test]
    fn pipe_and_property_control_attributes_arrive_and_a_record_without_a_position_is_reported() {
        let domain = import(
            "string drainage { name D data { 0 0 1 0 0  5 5 1 0 0 } \
             pit { name A x 0 y 0 z 2 } pit { name B x 5 y 5 z 2 } \
             pipe { name P diameter 0.3 attributes { text \"pipe size\" \"375\" } } \
             property_control { name Lot attributes { integer lot 4 } \
             \x20 data { 1 1 1 0 0  2 2 1 0 0 } } \
             house_connection { name HC2 side right length 4 } }",
        );
        assert_eq!(domain.entities.len(), 4);
        assert_eq!(
            prop_text(
                &domain.entities[0].properties,
                "pipe.1.attributes/pipe size"
            ),
            "375"
        );
        assert_eq!(prop_integer(&domain.entities[3].properties, "lot"), 4);
        assert!(
            any_contains(
                &domain.warnings,
                "house_connection \"HC2\" of string drainage \"D\""
            ),
            "{}",
            joined(&domain.warnings)
        );
    }

    #[test]
    fn a_face_is_closed_and_an_interface_keeps_its_modes() {
        let domain = import(
            "string face { name f fill_mode 1 hatch_angle 45 data { 0 0 1  10 0 1  10 10 1 } }\n\
             string interface { name i data { 0 0 1 -1  5 0 2 0  9 0 3 1 } }",
        );
        assert_eq!(domain.entities.len(), 2);
        let face = shape::<Polyline2>(&domain.entities[0].geometry);
        assert!(face.closed);
        assert_eq!(
            prop_text(&domain.entities[0].metadata, "12d.x.hatch_angle"),
            "45"
        );
        assert_eq!(
            prop_text(&domain.entities[1].metadata, "12d.interface_modes"),
            "-1 0 1"
        );

        // And back out as the strings they were.
        let back = exported_again(&domain, false);
        assert_eq!(back.elements.len(), 2);
        let f = get::<VertexString>(&back.elements[0]);
        assert_eq!(f.kind, katana_archive12d::StringKind::Face);
        assert_eq!(f.header.extras.real("hatch_angle"), Some(45.0));
        assert_eq!(f.header.extras.integer("fill_mode"), Some(1));
        let i = get::<VertexString>(&back.elements[1]);
        assert_eq!(i.kind, katana_archive12d::StringKind::Interface);
        assert_eq!(i.interface_modes, vec![-1, 0, 1]);
    }

    #[test]
    fn per_segment_sizes_and_per_vertex_data_are_all_accounted_for() {
        let domain = import(
            r#"string super { name p data_3d { 0 0 0  10 0 0  20 0 0 }
  pipe_data { properties { diameter 0.225 } properties { diameter 0.225 } } justify bottom }
string super { name v data_3d { 0 0 0  10 0 0  20 0 0 }
  diameter_data { 0.3 0.45 } }
string super { name c data_3d { 0 0 0  10 0 0  20 0 0 }
  culvert_data { properties { width 1 height 2 } properties { width 3 height 4 } } }
string super { name full data_3d { 0 0 0  10 0 0  20 0 0 } closed 1
  segment_text_data { s1 "" "s 3" }
  segment_visible_data { 0 1 1 } vertex_tinable_value 0
  colour_data { red "dark green" -1 }
  vertex_attribute_data { attributes { integer n 1 } attributes { } attributes { text t "x y" } }
  segment_attribute_data { attributes { real grade 0.02 } attributes { } attributes { } }
  symbol_value { style Tree size 2 } }
string super { name one data_3d { 5 5 5 } vertex_attribute_data { attributes { text "QualityLevel" "B" } } }"#,
        );
        // p: a uniform per-segment pipe IS one diameter; v and c: lists.
        assert_eq!(
            prop_real(&domain.entities[0].metadata, "12d.diameter"),
            0.225
        );
        assert_eq!(
            prop_text(&domain.entities[1].metadata, "12d.diameters"),
            "0.3 0.45"
        );
        assert_eq!(
            prop_text(&domain.entities[2].metadata, "12d.culverts"),
            "1 2 3 4"
        );

        // full: the polyline, then its two non-empty segment texts.
        let full = &domain.entities[3];
        assert_eq!(prop_text(&full.metadata, "12d.segment_visible"), "0 1 1");
        assert_eq!(prop_text(&full.metadata, "12d.vertex_tinable"), "0");
        assert_eq!(
            prop_text(&full.metadata, "12d.segment_colours"),
            "red \"dark green\" -1"
        );
        assert_eq!(prop_integer(&full.properties, "vertex/1/n"), 1);
        assert_eq!(prop_text(&full.properties, "vertex/3/t"), "x y");
        assert_eq!(prop_real(&full.properties, "segment/1/grade"), 0.02);
        let text3 = shape::<TextGeometry>(&domain.entities[5].geometry);
        assert_eq!(text3.text, "s 3");
        assert_eq!(
            text3.position,
            Point2::new(10.0, 0.0),
            "the middle of the closing segment (20,0)-(0,0)"
        );
        // one: a single vertex's attributes are simply the point's.
        assert_eq!(
            prop_text(&domain.entities[6].properties, "QualityLevel"),
            "B"
        );
        assert!(
            any_contains(
                &domain.warnings,
                "1 strings have invisible vertices or segments"
            ),
            "{}",
            joined(&domain.warnings)
        );
        // full is a LINE with one vertex symbol, and that symbol is now what
        // it is drawn with: a vertex symbol goes on every vertex, so the style
        // carries it and there is nothing left over to warn about. It used to
        // be kept as metadata and drawn nowhere.
        assert!(
            !any_contains(&domain.warnings, "strings carry vertex symbols"),
            "{}",
            joined(&domain.warnings)
        );
        assert_eq!(full.style, "Tree");
        let tree: &Style = domain
            .styles_needed
            .iter()
            .find(|style| style.name == "Tree")
            .expect("the Tree style");
        assert_eq!(
            tree.symbol, "Tree",
            "the real library name, resolved when it is drawn"
        );
        assert_eq!(tree.symbol_size, 2.0);
        assert!(
            !full.metadata.contains_key("12d.symbol.style"),
            "the style carries it now, so it is not also loose in the metadata"
        );

        // The whole lot back out and in again: the same strings. The styles
        // too: an import returns styles_needed so a caller can add them, and
        // a string is written with the symbol ITS STYLE carries - a model
        // without them is a drawing that has forgotten what it looks like.
        let back = exported_again(&domain, true);
        let p = get::<VertexString>(&back.elements[0]);
        assert_eq!(p.diameter, Some(0.225));
        assert_eq!(p.justify, "bottom");
        assert_eq!(
            get::<VertexString>(&back.elements[1]).diameters,
            vec![0.3, 0.45]
        );
        let culverts = &get::<VertexString>(&back.elements[2]).culverts;
        assert_eq!(culverts.len(), 2);
        assert_eq!(culverts[1], [3.0, 4.0]);
        let f = get::<VertexString>(&back.elements[3]);
        assert_eq!(f.segment_visible, vec![false, true, true]);
        assert_eq!(f.vertex_tinable_value, Some(false));
        assert_eq!(f.segment_colours, vec!["red", "dark green", "-1"]);
        assert_eq!(f.vertex_attributes.len(), 3);
        assert_eq!(attribute_integer(&f.vertex_attributes[0][0]), 1);
        assert!(f.vertex_attributes[1].is_empty());
        assert_eq!(attribute_text(&f.vertex_attributes[2][0]), "x y");
        assert_eq!(f.segment_attributes.len(), 3);
        assert_eq!(attribute_real(&f.segment_attributes[0][0]), 0.02);
        let symbol = f
            .symbol
            .as_ref()
            .expect("one symbol for every vertex goes back as symbol_value");
        assert_eq!(symbol.text("style", ""), "Tree");
        assert_eq!(symbol.real("size"), Some(2.0));
    }

    #[test]
    fn split_list_is_the_inverse_of_what_import_joins() {
        assert_eq!(
            split_list("red \"dark green\" -1"),
            vec!["red", "dark green", "-1"]
        );
        assert_eq!(split_list("  a   b "), vec!["a", "b"]);
        assert_eq!(
            split_list(r#""say \"hi\"" "a\\b""#),
            vec!["say \"hi\"", "a\\b"]
        );
        assert!(split_list("").is_empty());
        assert_eq!(split_list("\"unterminated"), vec!["unterminated"]);
    }
}
