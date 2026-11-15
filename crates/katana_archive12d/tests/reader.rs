//! The twin of tests/archive12d/test_reader.cpp: numbers, the syntax of manual
//! 1.1, the commands of 1.4, attributes, super strings, and what the manual
//! does not describe.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use katana_archive12d::{
    Breakline, ReadOptions, SegmentKind, VertexString, parse_real, read_archive, read_archive_bytes,
};
use katana_core::ErrorCode;

use support::{
    at, attribute_group, attribute_integer, attribute_real, attribute_text, get, only, read,
};

// ---- numbers ---------------------------------------------------------------

mod parse_real {
    use super::*;

    #[test]
    fn reads_decimals_exponents_and_signs() {
        assert_eq!(parse_real("502000.5"), Some(502000.5));
        assert_eq!(parse_real("-0.25"), Some(-0.25));
        assert_eq!(parse_real("+8"), Some(8.0));
        assert_eq!(parse_real("1.5e3"), Some(1500.0));
        assert_eq!(parse_real(".5"), Some(0.5));
    }

    #[test]
    fn reads_the_hexadecimal_floats_twelve_d_writes_its_tins_in() {
        // C99 hexadecimal floating constants: mantissa in hex, exponent a
        // power of two. 0x1.8p+1 = 1.5 * 2 = 3; 0x1p-4 = 1/16. Both from the C
        // standard's own definition, and both exact in binary.
        assert_eq!(parse_real("0x1.8p+1"), Some(3.0));
        assert_eq!(parse_real("-0x1.8p+1"), Some(-3.0));
        assert_eq!(parse_real("0x1p-4"), Some(0.0625));
        assert_eq!(parse_real("0x0.0p+0"), Some(0.0));
        // A value from a real full_tin: 0x1.6d4014ac08318p+17. Its leading
        // digits are 1.6d4 hex = 1 + 6/16 + 13/256 + 4/4096 = 1.4267578...,
        // times 2^17 = 131072, so it lies between 187008 and 187009.
        let easting = parse_real("0x1.6d4014ac08318p+17").expect("a hexadecimal float");
        assert!(easting > 187008.0);
        assert!(easting < 187009.0);
    }

    #[test]
    fn refuses_what_is_not_a_finite_number() {
        assert!(parse_real("").is_none());
        assert!(parse_real("null").is_none());
        assert!(parse_real("inf").is_none());
        assert!(parse_real("nan").is_none());
        assert!(parse_real("12abc").is_none());
        assert!(parse_real("0x").is_none());
        assert!(parse_real("1e999").is_none()); // overflows a double
        assert!(parse_real("-").is_none());
    }

    #[test]
    fn tolerates_the_comma_of_a_hand_made_row() {
        assert_eq!(parse_real("502000.5,"), Some(502000.5));
    }
}

// ---- syntax (manual 1.1) ---------------------------------------------------

mod reader_syntax {
    use super::*;

    #[test]
    fn comments_blank_lines_and_layout_do_not_matter() {
        let spread = read(
            "string super {\n  // a comment\n  name \"A\"   // trailing comment\n\n  data_3d {\n    1 2 3\n    4 5 6\n  }\n}\n",
        );
        let packed = read("string super{name \"A\" data_3d{1 2 3 4 5 6}}");
        let a = only::<VertexString>(&spread);
        let b = only::<VertexString>(&packed);
        assert_eq!(a.header.name, "A");
        assert_eq!(a.vertices.len(), 2);
        assert_eq!(a.vertices, b.vertices);
        assert_eq!(a.vertices[1], at(4.0, 5.0, Some(6.0)));
    }

    #[test]
    fn a_keyword_and_its_value_may_be_on_different_lines() {
        let archive =
            read("string super {\n name\n \"Kerb\"\n chainage\n 12.5\n data_2d { 0 0 }\n}");
        let string = only::<VertexString>(&archive);
        assert_eq!(string.header.name, "Kerb");
        assert_eq!(string.header.chainage, 12.5);
    }

    #[test]
    fn keywords_are_case_insensitive() {
        let archive = read("MODEL \"M\" STRING Super { NAME \"A\" Data_3D { 1 2 3 } }");
        let string = only::<VertexString>(&archive);
        assert_eq!(string.header.model, "M");
        assert_eq!(string.header.name, "A");
        assert_eq!(string.vertices.len(), 1);
    }

    #[test]
    fn quoted_text_keeps_its_spaces_comment_markers_and_escapes() {
        // Manual 1.1: inside a text, \" is a double quote and \\ a backslash.
        let archive = read(r#"string super { name "a // b  \"c\" \\ d" data_2d { 0 0 } }"#);
        assert_eq!(
            only::<VertexString>(&archive).header.name,
            "a // b  \"c\" \\ d"
        );
    }

    #[test]
    fn a_backslash_before_anything_else_is_itself() {
        let archive = read(r#"string super { name "C:\Temp\job" data_2d { 0 0 } }"#);
        assert_eq!(only::<VertexString>(&archive).header.name, r"C:\Temp\job");
    }

    #[test]
    fn a_text_never_closed_is_an_error_naming_its_line() {
        let error = read_archive(
            "model \"A\"\nstring super {\n name \"unclosed\n}",
            &ReadOptions::default(),
        )
        .expect_err("the text is never closed");
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(error.context.contains("line 3"), "{}", error.describe());
    }

    #[test]
    fn a_brace_never_closed_is_an_error() {
        let error = read_archive(
            "string super { name \"A\" data_3d { 1 2 3 }",
            &ReadOptions::default(),
        )
        .expect_err("the brace is never closed");
        assert_eq!(error.code, ErrorCode::ParseFailure);
    }

    #[test]
    fn a_brace_that_closes_nothing_is_an_error() {
        let error = read_archive("model \"A\"\n}\n", &ReadOptions::default())
            .expect_err("the brace closes nothing");
        assert!(error.context.contains("line 2"), "{}", error.describe());
    }

    #[test]
    fn nesting_beyond_the_limit_is_refused_rather_than_overflowing_the_stack() {
        let mut text = String::from("string super { attributes ");
        for _ in 0..200 {
            text.push_str("{ group ");
        }
        let error =
            read_archive(text.as_bytes(), &ReadOptions::default()).expect_err("nested too deep");
        assert!(error.message.contains("nested"), "{}", error.describe());
    }

    #[test]
    fn text_that_is_not_utf8_is_refused_before_it_can_reach_the_model() {
        let error = read_archive(
            b"string super { name \"\xFF\" data_2d { 0 0 } }",
            &ReadOptions::default(),
        )
        .expect_err("the text is not UTF-8");
        assert_eq!(error.code, ErrorCode::ParseFailure);
    }

    #[test]
    fn the_empty_file_is_an_empty_archive() {
        let archive = read("");
        assert!(archive.elements.is_empty());
        assert!(archive.model_names.is_empty());
        let comments_only = read("// nothing here\n\n// at all\n");
        assert!(comments_only.elements.is_empty());
    }

    #[test]
    fn reads_utf16_as_twelve_d_model_writes_it() {
        let text = "model \"M\"\nstring super { name \"A\" data_3d { 1 2 3 } }\n";
        let mut bytes = vec![0xFF_u8, 0xFE];
        for byte in text.bytes() {
            bytes.push(byte);
            bytes.push(0);
        }
        let archive = read_archive_bytes(bytes, &ReadOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(archive.elements.len(), 1);
        assert_eq!(get::<VertexString>(&archive.elements[0]).header.name, "A");
        assert!(archive.warnings.is_empty(), "a marked file is not a guess");
    }
}

// ---- commands (manual 1.4) ---------------------------------------------------

mod reader_commands {
    use super::*;

    #[test]
    fn the_defaults_are_the_ones_the_manual_gives() {
        // 1.4.1 "data", 1.4.2 "red", 1.4.4 "point", 1.4.5 -999.
        let archive = read("string super { data_3d { 1 2 -999 } }");
        let string = only::<VertexString>(&archive);
        assert_eq!(string.header.model, "data");
        assert_eq!(string.header.colour, "red");
        assert_eq!(string.header.breakline, Breakline::Point);
        assert!(string.vertices[0].z.is_none(), "-999 is the default null");
        assert_eq!(archive.null_value, -999.0);
    }

    #[test]
    fn a_command_holds_until_the_next_and_a_string_may_override_it() {
        let archive = read(
            "model \"A\" colour blue style \"Kerb\" breakline line\n\
             string super { data_2d { 0 0 } }\n\
             string super { model \"B\" colour green data_2d { 0 0 } }\n\
             string super { data_2d { 0 0 } }\n",
        );
        assert_eq!(archive.elements.len(), 3);
        let first = get::<VertexString>(&archive.elements[0]);
        let second = get::<VertexString>(&archive.elements[1]);
        let third = get::<VertexString>(&archive.elements[2]);
        assert_eq!(first.header.model, "A");
        assert_eq!(first.header.colour, "blue");
        assert_eq!(first.header.style, "Kerb");
        assert_eq!(first.header.breakline, Breakline::Line);
        assert_eq!(second.header.model, "B");
        assert_eq!(second.header.colour, "green");
        // The override was for that string only (manual 1.4.6).
        assert_eq!(third.header.model, "A");
        assert_eq!(third.header.colour, "blue");
        assert_eq!(archive.model_names, vec!["A", "B"]);
    }

    #[test]
    fn the_null_command_changes_which_height_means_none() {
        let archive = read("null -1\nstring super { data_3d { 0 0 -1  1 1 -999  2 2 null } }");
        let string = only::<VertexString>(&archive);
        assert_eq!(string.vertices.len(), 3);
        assert!(string.vertices[0].z.is_none());
        assert_eq!(
            string.vertices[1].z,
            Some(-999.0),
            "-999 is an ordinary height once null is -1"
        );
        assert!(
            string.vertices[2].z.is_none(),
            "the null keyword is always null"
        );
    }

    #[test]
    fn null_null_as_twelve_d_model_writes_it_leaves_the_value_alone() {
        let archive = read("null null\nstring super { data_3d { 0 0 -999 } }");
        assert_eq!(archive.null_value, -999.0);
        assert!(only::<VertexString>(&archive).vertices[0].z.is_none());
    }

    #[test]
    fn a_string_may_carry_a_null_value_of_its_own() {
        let archive = read(
            "string super { null_value -5 data_3d { 0 0 -5 } }\n\
             string super { data_3d { 0 0 -5 } }",
        );
        assert_eq!(archive.elements.len(), 2);
        assert!(
            get::<VertexString>(&archive.elements[0]).vertices[0]
                .z
                .is_none()
        );
        assert_eq!(
            get::<VertexString>(&archive.elements[1]).vertices[0].z,
            Some(-5.0)
        );
    }

    #[test]
    fn a_model_declared_with_a_block_carries_attributes_and_becomes_current() {
        let archive = read(
            "model { name \"telegraph poles\" attributes { integer \"wires\" 3 } }\n\
             string super { data_2d { 0 0 } }",
        );
        assert_eq!(archive.models.len(), 1);
        assert_eq!(archive.models[0].name, "telegraph poles");
        assert_eq!(archive.models[0].attributes.len(), 1);
        assert_eq!(attribute_integer(&archive.models[0].attributes[0]), 3);
        assert_eq!(
            only::<VertexString>(&archive).header.model,
            "telegraph poles"
        );
    }

    #[test]
    fn model_names_that_differ_only_in_case_are_one_model() {
        // Manual 1.1: "FRED" is considered the same model name as "Fred".
        let archive = read(
            "model \"Fred\" string super { data_2d { 0 0 } }\n\
             model \"FRED\" string super { data_2d { 0 0 } }",
        );
        assert_eq!(archive.model_names, vec!["Fred"]);
    }

    #[test]
    fn a_two_word_colour_is_read_whether_or_not_it_is_quoted() {
        let archive = read(
            "string super { colour dark blue style \"WS\" data_2d { 0 0 } }\n\
             string super { colour \"light grey\" data_2d { 0 0 } }\n\
             string super { colour dark name \"N\" data_2d { 0 0 } }",
        );
        assert_eq!(archive.elements.len(), 3);
        let bare = get::<VertexString>(&archive.elements[0]);
        assert_eq!(bare.header.colour, "dark blue");
        assert_eq!(bare.header.style, "WS");
        assert_eq!(
            get::<VertexString>(&archive.elements[1]).header.colour,
            "light grey"
        );
        // `dark` followed by a keyword is a colour called dark, not "dark name".
        let guarded = get::<VertexString>(&archive.elements[2]);
        assert_eq!(guarded.header.colour, "dark");
        assert_eq!(guarded.header.name, "N");
    }

    #[test]
    fn the_export_settings_twelve_d_writes_as_comments_are_kept() {
        let archive = read(
            "// archive_version \"15.01.08.60\"\n// decimal_places 8\n\
             // output_hex_floats false\n// just a remark 3\n\
             string super { data_2d { 0 0 } }\n// decimal_places 2\n",
        );
        assert_eq!(archive.header_settings.len(), 3);
        assert_eq!(archive.header_settings[0].key, "archive_version");
        assert_eq!(archive.header_settings[0].value, "15.01.08.60");
        assert_eq!(
            archive.header_settings[1].value, "8",
            "settings after the first element are not the header"
        );
    }
}

// ---- attributes (manual 1.3) -------------------------------------------------

mod reader_attributes {
    use super::*;

    #[test]
    fn each_type_is_read_as_its_own_type() {
        let archive = read(
            "string super { attributes {\n\
             \x20 text \"pole id\" \"QMR-37\"\n  text street \"477 Boundary St\"\n\
             \x20 real \"pole height\" 5.25\n  integer \"pole wires\" 3\n\
             } data_2d { 0 0 } }",
        );
        let attributes = &only::<VertexString>(&archive).header.attributes;
        assert_eq!(attributes.len(), 4);
        assert_eq!(attributes[0].name, "pole id");
        assert_eq!(attribute_text(&attributes[0]), "QMR-37");
        assert_eq!(attributes[1].name, "street");
        assert_eq!(attribute_real(&attributes[2]), 5.25);
        assert_eq!(attribute_integer(&attributes[3]), 3);
    }

    #[test]
    fn groups_nest() {
        let archive = read(
            "string super { attributes { group { name \"Asset\" attributes {\n\
             \x20 text \"Owner\" \"Telstra\"\n\
             \x20 group { name \"Size\" attributes { real \"Diameter\" 0.1 } }\n\
             } } text \"Date\" \"2024\" } data_2d { 0 0 } }",
        );
        let attributes = &only::<VertexString>(&archive).header.attributes;
        assert_eq!(attributes.len(), 2);
        assert_eq!(attributes[0].name, "Asset");
        let asset = attribute_group(&attributes[0]);
        assert_eq!(asset.len(), 2);
        let size = attribute_group(&asset[1]);
        assert_eq!(size.len(), 1);
        assert_eq!(attribute_real(&size[0]), 0.1);
        assert_eq!(attributes[1].name, "Date");
    }

    #[test]
    fn a_type_the_manual_does_not_list_is_kept_under_its_own_name() {
        // Real exports carry `uid "this id" 5292`.
        let archive = read("string super { attributes { uid \"this id\" 5292 } data_2d { 0 0 } }");
        let attribute = &only::<VertexString>(&archive).header.attributes[0];
        assert_eq!(attribute.declared_type, "uid");
        assert_eq!(attribute_integer(attribute), 5292);
    }

    #[test]
    fn an_attribute_with_a_null_value_is_counted_not_invented() {
        let archive = read(
            "string super { attributes { real \"depth\" null integer \"n\" 2 } data_2d { 0 0 } }",
        );
        let attributes = &only::<VertexString>(&archive).header.attributes;
        assert_eq!(attributes.len(), 1);
        assert_eq!(attributes[0].name, "n");
        let last = archive.warnings.last().expect("a warning");
        assert!(last.contains("1 attributes"), "{last}");
    }
}

// ---- super strings (manual 1.5.8) ----------------------------------------------

mod reader_super_string {
    use super::*;

    #[test]
    fn a_two_dimensional_string_takes_its_one_height_from_z() {
        let archive = read("string super { z 31.25 closed true data_2d { 0 0  10 0  10 5 } }");
        let string = only::<VertexString>(&archive);
        assert!(string.closed);
        assert_eq!(string.constant_z, Some(31.25));
        assert_eq!(string.vertices.len(), 3);
        assert!(string.vertices[0].z.is_none());
        assert_eq!(
            string.segment_count(),
            3,
            "a closed string of n vertices has n segments"
        );
    }

    #[test]
    fn closed_accepts_every_spelling_the_manual_lists() {
        // 1.5.8: true is 1, T, t, Y, y or a word starting so; false 0, F, f,
        // N, n.
        for yes in ["1", "T", "t", "Y", "y", "true", "Yes"] {
            let archive = read(&format!(
                "string super {{ closed {yes} data_2d {{ 0 0 }} }}"
            ));
            assert!(only::<VertexString>(&archive).closed, "{yes}");
        }
        for no in ["0", "F", "f", "N", "n", "false", "No"] {
            let archive = read(&format!("string super {{ closed {no} data_2d {{ 0 0 }} }}"));
            assert!(!only::<VertexString>(&archive).closed, "{no}");
        }
    }

    #[test]
    fn radius_and_major_data_become_arc_segments() {
        let archive = read(
            "string super { data_2d { 0 0  2 0  4 0  6 0 }\n\
             radius_data { 0 -5 7.5 } major_data { 0 1 f } }",
        );
        let string = only::<VertexString>(&archive);
        assert_eq!(string.segments.len(), 3);
        assert_eq!(string.segments[0].kind, SegmentKind::Straight);
        assert_eq!(string.segments[1].kind, SegmentKind::Arc);
        assert_eq!(string.segments[1].radius, -5.0);
        assert!(string.segments[1].major);
        assert_eq!(string.segments[2].radius, 7.5);
        assert!(!string.segments[2].major);
    }

    #[test]
    fn radii_that_are_all_zero_mean_there_is_no_curved_segment() {
        let archive =
            read("string super { data_2d { 0 0 2 0 4 0 } radius_data { 0 0 } major_data { 0 0 } }");
        assert!(only::<VertexString>(&archive).segments.is_empty());
    }

    #[test]
    fn geometry_data_reads_every_kind_of_segment() {
        let archive = read(
            "string super { data_2d { 0 0  1 0  2 0  3 0  4 0 } geometry_data {\n\
             \x20straight { }\n arc { radius 80 major 1 }\n\
             \x20spiral { type \"cubic parabola\" leading 0 l1 0 r1 0 a1 200.5 l2 80 r2 299 a2 192 }\n\
             \x20curve { type clothoid leading 1 xorigin 1 yorigin 2 radius 50 length 30 \
             start 5 end 25 angle 10 offset 1.5 mvalue 0 }\n} }",
        );
        let segments = &only::<VertexString>(&archive).segments;
        assert_eq!(segments.len(), 4);
        assert_eq!(segments[0].kind, SegmentKind::Straight);
        assert_eq!(segments[1].kind, SegmentKind::Arc);
        assert_eq!(segments[1].radius, 80.0);
        assert!(segments[1].major);
        assert_eq!(segments[2].kind, SegmentKind::Spiral);
        assert_eq!(segments[2].parameters.text("type", ""), "cubic parabola");
        assert_eq!(segments[2].parameters.boolean("leading"), Some(false));
        assert_eq!(segments[2].parameters.real("a1"), Some(200.5));
        assert_eq!(segments[3].kind, SegmentKind::Curve);
        assert_eq!(segments[3].parameters.real("offset"), Some(1.5));
    }

    #[test]
    fn a_segment_kind_nobody_knows_keeps_its_place_as_a_straight() {
        // Dropping it would move every later segment onto the wrong vertices.
        let archive = read(
            "string super { data_2d { 0 0 1 0 2 0 3 0 } geometry_data {\n\
             \x20straight { } wiggle { amplitude 3 } arc { radius 9 major 0 } } }",
        );
        let segments = &only::<VertexString>(&archive).segments;
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[1].kind, SegmentKind::Straight);
        assert_eq!(segments[2].radius, 9.0);
        assert!(!archive.warnings.is_empty());
        assert!(archive.warnings[0].contains("wiggle"));
    }

    #[test]
    fn the_superseded_data_block_carries_radius_and_bulge_on_each_vertex() {
        let archive = read("string super { data { 0 0 1 12 0   20 0 1 0 0   40 10 1 -8 1 } }");
        let string = only::<VertexString>(&archive);
        assert_eq!(string.vertices.len(), 3);
        assert_eq!(string.vertices[2], at(40.0, 10.0, Some(1.0)));
        assert_eq!(string.segments.len(), 3);
        assert_eq!(string.segments[0].radius, 12.0);
        assert_eq!(string.segments[1].kind, SegmentKind::Straight);
        assert_eq!(string.segments[2].radius, -8.0);
        assert!(string.segments[2].major);
    }

    #[test]
    fn every_block_of_extra_information_is_read() {
        let archive = read(
            r#"string super {
  data_3d { 0 0 1  10 0 2  10 10 3 }
  interval { chord_arc 0.01 distance 5 }
  colour_data { red "dark green" }
  point_data { 101 "A 7" 103 }
  diameter_data { 0.3 0.45 }
  justify invert
  vertex_tinable_data { 1 0 t }   segment_tinable_data { 1 0 }
  vertex_visible_data { 1 1 0 }   segment_visible_data { 0 1 }
  vertex_text_data { "a" "" "c" }
  vertex_annotate_value { angle 45 offset 1 raise 0.5 textstyle "Arial" worldsize 2 justify "top-left" colour blue }
  segment_text_value "seg"
  segment_annotate_data { properties { papersize 3 } properties { papersize 4 } }
  symbol_data { properties { style "Tree" size 2 rotation 0 } properties { } properties { style "Post" } }
  vertex_attribute_data { attributes { integer "n" 1 } attributes { } attributes { text "t" "x" } }
  segment_attribute_data { attributes { real "grade" 0.02 } attributes { } }
}"#,
        );
        let s = only::<VertexString>(&archive);
        assert_eq!(
            s.interval.as_ref().expect("an interval").real("chord_arc"),
            Some(0.01)
        );
        assert_eq!(s.segment_colours, vec!["red", "dark green"]);
        assert_eq!(s.point_ids, vec!["101", "A 7", "103"]);
        assert_eq!(s.diameters, vec![0.3, 0.45]);
        assert_eq!(s.justify, "invert");
        assert_eq!(s.vertex_tinable, vec![true, false, true]);
        assert_eq!(s.segment_tinable, vec![true, false]);
        assert_eq!(s.vertex_visible, vec![true, true, false]);
        assert_eq!(s.segment_visible, vec![false, true]);
        assert_eq!(s.vertex_text, vec!["a", "", "c"]);
        let annotation = s.vertex_annotation.as_ref().expect("an annotation");
        assert_eq!(annotation.real("angle"), Some(45.0));
        assert_eq!(annotation.text("justify", ""), "top-left");
        assert_eq!(annotation.text("colour", ""), "blue");
        assert_eq!(s.segment_text_value.as_deref(), Some("seg"));
        assert_eq!(s.segment_annotations.len(), 2);
        assert_eq!(s.segment_annotations[1].real("papersize"), Some(4.0));
        assert_eq!(s.symbols.len(), 3);
        assert_eq!(s.symbols[0].text("style", ""), "Tree");
        assert!(s.symbols[1].empty());
        assert_eq!(s.vertex_attributes.len(), 3);
        assert!(s.vertex_attributes[1].is_empty());
        assert_eq!(attribute_text(&s.vertex_attributes[2][0]), "x");
        assert_eq!(s.segment_attributes.len(), 2);
        assert_eq!(attribute_real(&s.segment_attributes[0][0]), 0.02);
        assert!(
            archive.warnings.is_empty(),
            "{:?}",
            archive.warnings.first()
        );
        assert!(archive.unrecognised.is_empty());
    }

    #[test]
    fn pipe_and_culvert_sizes_are_read_in_both_the_manuals_form_and_twelve_ds() {
        // The manual documents diameter_value / diameter_data; version 15 of
        // the writing program writes pipe_value { diameter } / pipe_data {
        // properties { diameter } }.
        let manual = read("string super { data_3d { 0 0 0 1 0 0 } diameter_value 0.375 }");
        assert_eq!(only::<VertexString>(&manual).diameter, Some(0.375));
        let real = read(
            "string super { data_3d { 0 0 0 1 0 0 2 0 0 }\n\
             pipe_data { properties { diameter 0.225 } properties { diameter 0.3 } } }",
        );
        assert_eq!(only::<VertexString>(&real).diameters, vec![0.225, 0.3]);
        let value = read(
            "string super { data_3d { 0 0 0 1 0 0 } pipe_value { diameter 0.45 thickness 0.02 } }",
        );
        let string = only::<VertexString>(&value);
        assert_eq!(string.diameter, Some(0.45));
        assert_eq!(
            string.header.extras.real("pipe_value.thickness"),
            Some(0.02),
            "understood, but not lost"
        );

        let culvert = read(
            "string super { data_3d { 0 0 0 1 0 0 2 0 0 } culvert_value { width 0.9 height 0.55 }\n\
             culvert_data { properties { width 1 height 2 } properties { width 3 height 4 } } }",
        );
        let c = only::<VertexString>(&culvert);
        assert_eq!(c.culvert, Some([0.9, 0.55]));
        assert_eq!(c.culverts.len(), 2);
        assert_eq!(c.culverts[1], [3.0, 4.0]);

        // Both on one string, as version 15 writes every culvert: kept, and
        // not complained about - the manual says it cannot happen, and it does.
        let both = read(
            "string super { name EU data_3d { 0 0 0 1 0 0 } pipe_data { properties { diameter 0.125 } } \
             culvert_data { properties { width 0.5 height 0.4 } } }",
        );
        assert_eq!(only::<VertexString>(&both).diameters, vec![0.125]);
        assert_eq!(only::<VertexString>(&both).culverts[0], [0.5, 0.4]);
        assert!(both.warnings.is_empty(), "{:?}", both.warnings.first());
    }

    #[test]
    fn a_list_of_the_wrong_length_is_reported_not_silently_accepted() {
        let archive =
            read("string super { name \"S\" data_3d { 0 0 0  1 0 0  2 0 0 } point_data { 1 2 } }");
        assert_eq!(archive.warnings.len(), 1);
        assert!(
            archive.warnings[0].contains("point_data has 2 entries for 3 vertices"),
            "{}",
            archive.warnings[0]
        );
    }

    #[test]
    fn values_that_do_not_divide_into_rows_are_an_error_not_a_guess() {
        let error = read_archive(
            "string super {\n data_3d {\n 0 0 0\n 1 1\n }\n}",
            &ReadOptions::default(),
        )
        .expect_err("five values are not rows of three");
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(error.message.contains("5 values"), "{}", error.describe());
    }

    #[test]
    fn a_word_where_a_number_belongs_is_an_error() {
        let error = read_archive(
            "string super { data_3d { 0 zero 0 } }",
            &ReadOptions::default(),
        )
        .expect_err("zero is not a number");
        assert!(error.message.contains("zero"), "{}", error.describe());
    }

    #[test]
    fn only_a_height_may_be_null() {
        assert!(
            read_archive(
                "string super { data_3d { null 0 0 } }",
                &ReadOptions::default()
            )
            .is_err()
        );
        assert!(
            read_archive(
                "string super { data_3d { 0 0 null } }",
                &ReadOptions::default()
            )
            .is_ok()
        );
    }
}

// ---- what the manual does not describe -------------------------------------------

mod reader_unknown {
    use super::*;

    #[test]
    fn an_unknown_block_is_skipped_whole_and_counted_by_path() {
        let archive = read(
            "string super { name \"A\" drawables { label { data_2d { 9 9 } } }\n\
             \x20data_2d { 1 2 } solid_fill { colour cyan } }\n\
             string super { data_2d { 3 4 } drawables { } }\n\
             drafting_element_table { table { rows 3 } }",
        );
        assert_eq!(archive.elements.len(), 2);
        let first = get::<VertexString>(&archive.elements[0]);
        assert_eq!(
            first.vertices.len(),
            1,
            "the data_2d inside the unknown block is not the string's"
        );
        assert_eq!(first.vertices[0], at(1.0, 2.0, None));
        assert_eq!(archive.unrecognised["string super/drawables"], 2);
        assert_eq!(archive.unrecognised["string super/solid_fill"], 1);
        assert_eq!(archive.unrecognised["drafting_element_table"], 1);
    }

    #[test]
    fn an_unknown_scalar_is_kept_in_order() {
        let archive = read(
            "string super { weight 2 time_created \"07-Mar-2009 03:53:17\" data_2d { 0 0 } frobnicate 7 }",
        );
        let extras = &only::<VertexString>(&archive).header.extras;
        assert_eq!(extras.size(), 3);
        assert_eq!(extras.fields()[0].key, "weight");
        assert_eq!(extras.real("weight"), Some(2.0));
        assert_eq!(extras.text("time_created", ""), "07-Mar-2009 03:53:17");
        assert!(extras.find("time_created").expect("a field").quoted);
        assert_eq!(extras.integer("frobnicate"), Some(7));
    }

    #[test]
    fn an_unknown_flag_does_not_swallow_the_keyword_after_it() {
        let archive = read("string super { mystery_flag name \"Kept\" data_2d { 0 0 } }");
        let string = only::<VertexString>(&archive);
        assert_eq!(string.header.name, "Kept");
        assert!(string.header.extras.contains("mystery_flag"));
    }

    #[test]
    fn an_unknown_string_type_is_jumped_over_as_the_manual_intends() {
        // Manual 1.4.6: a reader "has a chance of jumping over the string by
        // looking for the ending curly brace".
        let archive = read("string hologram { data { 1 2 3 } }\nstring super { data_2d { 0 0 } }");
        assert_eq!(archive.elements.len(), 1);
        assert_eq!(archive.unrecognised["string hologram"], 1);
    }
}

mod reader_limits {
    use super::*;

    #[test]
    fn a_file_with_more_elements_than_the_limit_is_refused() {
        let options = ReadOptions {
            max_elements: 2,
            ..ReadOptions::default()
        };
        let text = "string super { data_2d { 0 0 } }\n".repeat(4);
        let error = read_archive(text.as_bytes(), &options).expect_err("too many elements");
        assert!(error.message.contains("limit"), "{}", error.describe());
    }

    #[test]
    fn a_block_with_more_values_than_the_limit_is_refused() {
        let options = ReadOptions {
            max_values_per_block: 5,
            ..ReadOptions::default()
        };
        let error = read_archive("string super { data_3d { 1 2 3 4 5 6 7 8 9 } }", &options)
            .expect_err("too many values");
        assert!(error.message.contains("limit"), "{}", error.describe());
    }
}
