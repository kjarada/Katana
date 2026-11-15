// The surface display styles' data (terrain/surface_style.rs;
// tests/cad/terrain/test_surface_style.cpp): the built-in set the brief names,
// the style a surface is drawn in, the checks, and the versioned JSON form.
//
// Three C++ cases build text that is not UTF-8 into a style (the bytes a
// katana_cli on Windows hands over); a Rust String cannot hold them, so those
// cases keep their UTF-8 half - the door's answer for valid text, and the
// writer's - as the port's rule for such cases has it (docs/rust.md, "Step 2").
// The case whose bad bytes come through the JSON text itself is twinned whole.

use crate::surface_test_support::flat_square;
use katana_cad::terrain::surface_style::{
    MAXIMUM_SURFACE_BANDS, built_in_surface_styles, check_surface_style, check_surface_style_set,
    parse_surface_faces, parse_surface_shading, style_of, surface_faces_word, surface_shading_word,
    surface_styles_from_json, surface_styles_to_json,
};
use katana_cad::{ColourStop, SurfaceDisplayStyle, SurfaceFaces, SurfaceStyle, SurfaceStyleSet};
use katana_core::ErrorCode;
use katana_core::text_encoding::is_valid_utf8;
use katana_entity::Color;
use katana_terrain::NamedSurface;

fn names_of(set: &SurfaceStyleSet) -> Vec<String> {
    set.styles.iter().map(|style| style.name.clone()).collect()
}

fn stop(value: f64, colour: Color) -> ColourStop {
    ColourStop { value, colour }
}

/// A style with every field other than its default, and doubles a text form
/// would lose if it were not written in full.
fn everything() -> SurfaceDisplayStyle {
    let mut style = SurfaceDisplayStyle {
        name: String::from("Every Field"),
        description: String::from("set, all of it"),
        show_triangles: true,
        triangle_colour: Color::new(1, 2, 3, 255),
        show_boundary: false,
        boundary_colour: Color::new(4, 5, 6, 128),
        show_points: true,
        point_colour: Color::new(7, 8, 9, 255),
        show_breaklines: true,
        breakline_colour: Color::new(10, 11, 12, 255),
        plan_faces: SurfaceFaces::Aspect,
        plan_opacity: 0.1 + 0.2, // 0.30000000000000004
        ramp: String::from("diverging"),
        banded: true,
        bands: 7,
        stops: vec![
            stop(-1e-9, Color::new(0, 0, 255, 255)),
            stop(12345.678901234567, Color::new(255, 0, 0, 255)),
        ],
        range_min: Some(-3.5),
        range_max: Some(1e300),
        flat_colour: Color::new(19, 20, 21, 255),
        shading: SurfaceStyle::ShadedWithEdges,
        faces3d: SurfaceFaces::Flat,
        ..SurfaceDisplayStyle::default()
    };
    style.contours.show = true;
    style.contours.interval = 1.0 / 3.0;
    style.contours.major_every = 4;
    style.contours.base = -0.125;
    style.contours.minor_colour = Color::new(13, 14, 15, 255);
    style.contours.major_colour = Color::new(16, 17, 18, 255);
    style.contours.label_major = true;
    style
}

fn surface_in(style: &str) -> NamedSurface {
    NamedSurface {
        name: String::from("ground"),
        surface: Some(flat_square(0.0)),
        style: String::from(style),
        ..NamedSurface::default()
    }
}

/// The field a style that cannot be drawn is refused for ("" when it is
/// drawn).
fn field_refused_for(change: impl FnOnce(&mut SurfaceDisplayStyle)) -> String {
    let mut style = SurfaceDisplayStyle {
        name: String::from("Probe"),
        ..SurfaceDisplayStyle::default()
    };
    change(&mut style);
    match check_surface_style(&style) {
        Ok(()) => String::new(),
        Err(error) => error.context,
    }
}

mod surface_style_sets {
    use super::*;

    #[test]
    fn the_built_in_set_is_the_seven_styles_in_order_with_standard_the_default() {
        let set = built_in_surface_styles();
        assert_eq!(
            names_of(&set),
            vec![
                "Standard",
                "Triangles",
                "Contours",
                "Elevation Bands",
                "Slope Bands",
                "Cut Fill",
                "Outline"
            ]
        );
        assert_eq!(set.default_style, "Standard");
        let checked = check_surface_style_set(&set);
        assert!(checked.is_ok(), "{checked:?}");
    }

    #[test]
    fn each_built_in_style_shows_what_it_is_named_for() {
        let set = built_in_surface_styles();
        let standard = set.find("Standard").expect("Standard");
        assert!(standard.show_boundary);
        assert!(!standard.show_triangles);
        assert_eq!(standard.faces3d, SurfaceFaces::Elevation);
        assert_ne!(standard.shading, SurfaceStyle::Wireframe);

        let triangles = set.find("Triangles").expect("Triangles");
        assert!(triangles.show_triangles);
        assert!(triangles.show_boundary);

        let contours = set.find("Contours").expect("Contours");
        assert!(contours.contours.show);
        assert_eq!(contours.contours.interval, 1.0);
        assert_eq!(contours.contours.major_every, 5);
        assert!(contours.contours.label_major);
        assert!(contours.show_boundary);

        let elevation = set.find("Elevation Bands").expect("Elevation Bands");
        assert_eq!(elevation.plan_faces, SurfaceFaces::Elevation);
        assert!(elevation.banded);
        assert_eq!(elevation.bands, 10);
        assert_eq!(elevation.ramp, "terrain");

        let slope = set.find("Slope Bands").expect("Slope Bands");
        assert_eq!(slope.plan_faces, SurfaceFaces::Slope);
        assert_eq!(slope.ramp, "slope");
        let stops: Vec<f64> = slope.stops.iter().map(|stop| stop.value).collect();
        assert_eq!(stops, vec![0.0, 2.0, 5.0, 10.0, 20.0, 33.0]);

        // Symmetric about 0: every stop has its negative, cut (below 0) bluer
        // than it is red and fill (above) redder than it is blue.
        let cut_fill = set.find("Cut Fill").expect("Cut Fill");
        assert_eq!(cut_fill.ramp, "diverging");
        assert!(!cut_fill.stops.is_empty());
        for stop in &cut_fill.stops {
            let mirrored = cut_fill
                .stops
                .iter()
                .any(|other| other.value == -stop.value);
            assert!(mirrored, "{}", stop.value);
            if stop.value < 0.0 {
                assert!(stop.colour.b > stop.colour.r, "{}", stop.value);
            } else if stop.value > 0.0 {
                assert!(stop.colour.r > stop.colour.b, "{}", stop.value);
            }
        }

        let outline = set.find("Outline").expect("Outline");
        assert!(outline.show_boundary);
        assert!(!outline.show_triangles);
        assert!(!outline.contours.show);
        assert_eq!(outline.shading, SurfaceStyle::Wireframe);
    }

    #[test]
    fn a_style_is_found_by_its_name_in_any_case() {
        let set = built_in_surface_styles();
        assert_eq!(set.find("cut fill").expect("in any case").name, "Cut Fill");
        assert!(set.find("Nothing").is_none());
    }

    #[test]
    fn a_surface_is_drawn_in_its_own_style_else_the_default_else_standard() {
        let mut set = built_in_surface_styles();
        assert_eq!(style_of(&set, &surface_in("contours")).name, "Contours");
        assert_eq!(style_of(&set, &surface_in("")).name, "Standard");
        set.default_style = String::from("Outline");
        assert_eq!(style_of(&set, &surface_in("")).name, "Outline");
        assert_eq!(style_of(&set, &surface_in("since deleted")).name, "Outline");
        // A set with neither: the built-in Standard, never nothing.
        let empty = SurfaceStyleSet::default();
        assert_eq!(style_of(&empty, &surface_in("Contours")).name, "Standard");
    }

    #[test]
    fn the_json_form_gives_back_every_field_exactly() {
        let mut set = built_in_surface_styles();
        set.styles.push(everything());
        set.default_style = String::from("Every Field");
        let text = surface_styles_to_json(&set);
        let back =
            surface_styles_from_json(&text).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(back == set);
        let read = back.find("Every Field").expect("Every Field");
        assert_eq!(read.plan_opacity, 0.1 + 0.2);
        assert_eq!(read.contours.interval, 1.0 / 3.0);
        assert_eq!(read.boundary_colour.a, 128);
        // The same set is the same text.
        assert_eq!(surface_styles_to_json(&back), text);
    }

    #[test]
    fn a_newer_version_is_unsupported_and_anything_else_a_parse_failure() {
        let code_of = |text: &str| surface_styles_from_json(text).expect_err("refused").code;
        assert_eq!(
            code_of(r#"{"format":"katana-surface-styles","version":2,"styles":[]}"#),
            ErrorCode::Unsupported
        );
        assert_eq!(code_of("not json"), ErrorCode::ParseFailure);
        assert_eq!(
            code_of(r#"{"format":"katana-sheets","version":1}"#),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            code_of(
                r#"{"format":"katana-surface-styles","version":1,"styles":[{"name":"A","faces":"stripes"}],"default":"A"}"#
            ),
            ErrorCode::ParseFailure
        );
        // Read, but not a set that can be drawn: the checks' refusal.
        assert_eq!(
            code_of(
                r#"{"format":"katana-surface-styles","version":1,"styles":[{"name":"A","opacity":2}],"default":"A"}"#
            ),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_style_that_cannot_be_drawn_is_refused_naming_the_field() {
        let nan = f64::NAN;
        let blank = Color::default();
        assert_eq!(field_refused_for(|s| s.name.clear()), "field=name");
        assert_eq!(
            field_refused_for(|s| s.name = String::from("say \"hi\"")),
            "field=name"
        );
        assert_eq!(
            field_refused_for(|s| s.contours.interval = 0.0),
            "field=interval"
        );
        assert_eq!(
            field_refused_for(|s| s.contours.interval = nan),
            "field=interval"
        );
        assert_eq!(
            field_refused_for(|s| s.contours.major_every = -1),
            "field=major"
        );
        assert_eq!(field_refused_for(|s| s.plan_opacity = 1.5), "field=opacity");
        assert_eq!(field_refused_for(|s| s.bands = 1), "field=bands");
        assert_eq!(
            field_refused_for(|s| s.stops = vec![stop(2.0, blank), stop(1.0, blank)]),
            "field=stops"
        );
        assert_eq!(
            field_refused_for(|s| s.stops = vec![stop(1.0, blank), stop(1.0, blank)]),
            "field=stops"
        );
        assert_eq!(
            field_refused_for(|s| s.stops = vec![stop(nan, blank)]),
            "field=stops"
        );
        assert_eq!(
            field_refused_for(|s| {
                s.range_min = Some(5.0);
                s.range_max = Some(5.0);
            }),
            "field=min"
        );
        assert_eq!(field_refused_for(|s| s.range_max = Some(nan)), "field=min");
        assert_eq!(
            field_refused_for(|s| s.ramp = String::from("no such ramp")),
            "field=ramp"
        );
        // The degenerate style that is fine: a single stop, an open range.
        let fine = SurfaceDisplayStyle {
            name: String::from("Fine"),
            stops: vec![stop(0.0, blank)],
            range_min: Some(0.0),
            ..SurfaceDisplayStyle::default()
        };
        assert!(check_surface_style(&fine).is_ok());
    }

    #[test]
    fn text_that_is_not_utf8_is_refused_and_the_message_does_not_carry_its_bytes() {
        // The C++ hands the door Windows-1252 bytes ("Pente" and 0xB0, "caf"
        // and 0xE9), which no String holds. The UTF-8 half: the same text as
        // UTF-8 - "Pente" + U+00B0, "caf" + U+00E9 - is fine, and so are
        // letters from other scripts.
        let fine = SurfaceDisplayStyle {
            name: String::from("Pente\u{B0} \u{6D4B}"),
            description: String::from("caf\u{E9}"),
            ..SurfaceDisplayStyle::default()
        };
        assert!(check_surface_style(&fine).is_ok());
    }

    #[test]
    fn a_description_a_command_line_cannot_carry_is_refused() {
        // SURFACESTYLE INFO prints the description for the person to type
        // back; the tokenizer ends a word at a quote and a line at a break.
        let refused_for = |description: &str| {
            let style = SurfaceDisplayStyle {
                name: String::from("Probe"),
                description: String::from(description),
                ..SurfaceDisplayStyle::default()
            };
            match check_surface_style(&style) {
                Ok(()) => String::from("accepted"),
                Err(error) => error.context,
            }
        };
        assert_eq!(refused_for("say \"hi\""), "field=description");
        assert_eq!(refused_for("two\nlines"), "field=description");
        assert_eq!(refused_for("one\rtwo"), "field=description");
        assert_eq!(refused_for(""), "accepted");
        assert_eq!(refused_for("C:\\dir 'quoted' = fine"), "accepted");
    }

    #[test]
    fn the_json_writer_never_throws_for_a_set_built_by_hand_with_bad_bytes() {
        // The C++ builds a set with Windows-1252 bytes and finds U+FFFD where
        // they were; no String holds such bytes. The UTF-8 half: a set built
        // by hand with letters past ASCII is written as UTF-8 with the
        // letters as they are (the writer escapes nothing it need not).
        let mut set = built_in_surface_styles();
        set.styles.push(SurfaceDisplayStyle {
            name: String::from("Pente\u{B0}"),
            description: String::from("caf\u{E9}"),
            ..SurfaceDisplayStyle::default()
        });
        let text = surface_styles_to_json(&set);
        assert!(is_valid_utf8(&text));
        assert!(text.contains("Pente\u{B0}"), "{text}");
        assert!(text.contains("caf\u{E9}"), "{text}");
    }

    #[test]
    fn a_format_that_is_not_a_string_is_a_parse_failure_not_an_exception() {
        // value("format", "") throws type_error.302 for a number, null, an
        // object or an array; the header promises ParseFailure for text that
        // is not the form.
        for text in [
            r#"{"format":5,"version":1,"styles":[]}"#,
            r#"{"format":null,"version":1,"styles":[]}"#,
            r#"{"format":{},"version":1,"styles":[]}"#,
            r#"{"format":[],"version":1,"styles":[]}"#,
            r#"{"format":true,"version":1,"styles":[]}"#,
            r#"{"version":1,"styles":[]}"#,
            r#"[]"#,
            r#"null"#,
            r#"7"#,
            r#""katana-surface-styles""#,
        ] {
            let error = surface_styles_from_json(text).expect_err(text);
            assert_eq!(error.code, ErrorCode::ParseFailure, "{text}");
        }
        // The right type with the right word still reads: the guard is not a
        // refusal of everything.
        assert!(
            surface_styles_from_json(surface_styles_to_json(&built_in_surface_styles())).is_ok()
        );
    }

    #[test]
    fn text_with_a_bad_byte_in_its_json_is_a_parse_failure_not_an_exception() {
        // A project whose style text holds Windows-1252 (0xE9) is no JSON: the
        // parser refuses the string, so the set never reaches the Document and
        // its save.
        let mut bad = Vec::from(
            &br#"{"format":"katana-surface-styles","version":1,"default":"A","styles":[{"name":"A","description":"caf"#[..],
        );
        bad.push(0xE9);
        bad.extend_from_slice(br#""}]}"#);
        let error = surface_styles_from_json(&bad).expect_err("not UTF-8");
        assert_eq!(error.code, ErrorCode::ParseFailure);
    }

    #[test]
    fn a_set_needs_a_style_no_two_of_one_name_and_a_default_among_them() {
        assert!(check_surface_style_set(&SurfaceStyleSet::default()).is_err());
        let mut twice = built_in_surface_styles();
        let first = twice.styles[0].clone();
        twice.styles.push(first);
        if let Some(last) = twice.styles.last_mut() {
            last.name = String::from("STANDARD");
        }
        assert_eq!(
            check_surface_style_set(&twice).expect_err("twice").code,
            ErrorCode::InvalidArgument
        );
        let mut lost = built_in_surface_styles();
        lost.default_style = String::from("Gone");
        assert_eq!(
            check_surface_style_set(&lost).expect_err("lost").code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn the_words_of_faces_and_shading_read_back_in_any_case() {
        for faces in [
            SurfaceFaces::None,
            SurfaceFaces::Flat,
            SurfaceFaces::Elevation,
            SurfaceFaces::Slope,
            SurfaceFaces::Aspect,
        ] {
            assert_eq!(parse_surface_faces(surface_faces_word(faces)), Some(faces));
        }
        for shading in [
            SurfaceStyle::Automatic,
            SurfaceStyle::Shaded,
            SurfaceStyle::ShadedWithEdges,
            SurfaceStyle::Wireframe,
            SurfaceStyle::Hidden,
        ] {
            assert_eq!(
                parse_surface_shading(surface_shading_word(shading)),
                Some(shading)
            );
        }
        assert_eq!(
            parse_surface_shading("EDGES"),
            Some(SurfaceStyle::ShadedWithEdges)
        );
        assert!(parse_surface_faces("stripes").is_none());
    }

    #[test]
    fn the_band_count_is_held_to_two_through_the_maximum_not_left_to_grow_without_bound() {
        // The 3D view cuts each triangle at every band edge it crosses, so its
        // geometry grew by about 643 triangles a band on the 21 182-triangle
        // sample terrain (28 078 at 10 bands, 3 235 844 at 5 000, 32 163 345
        // at 50 000): a count the verb allowed up to 1 000 000 built tens of
        // gigabytes. One number, 100, the form's own range, for the line, the
        // form and a project file.
        assert_eq!(MAXIMUM_SURFACE_BANDS, 100);
        assert_eq!(field_refused_for(|s| s.bands = 2), "");
        assert_eq!(field_refused_for(|s| s.bands = 100), "");
        assert_eq!(field_refused_for(|s| s.bands = 101), "field=bands");
        assert_eq!(field_refused_for(|s| s.bands = 1_000_000), "field=bands");
        assert_eq!(field_refused_for(|s| s.bands = i32::MAX), "field=bands");
        assert_eq!(field_refused_for(|s| s.bands = 1), "field=bands");
        assert_eq!(field_refused_for(|s| s.bands = -3), "field=bands");
        // The refusal says the range it holds.
        let many = SurfaceDisplayStyle {
            name: String::from("Many"),
            bands: 101,
            ..SurfaceDisplayStyle::default()
        };
        let error = check_surface_style(&many).expect_err("too many bands");
        assert!(error.message.contains("2 to 100"), "{}", error.describe());
    }

    #[test]
    fn a_list_of_stops_is_held_to_the_same_maximum_because_a_banded_one_is_a_band_a_stop() {
        let stops_of = |count: i32| -> Vec<ColourStop> {
            (0..count)
                .map(|i| stop(f64::from(i), Color::new(0, 0, 0, 255)))
                .collect()
        };
        assert_eq!(field_refused_for(|s| s.stops = stops_of(100)), "");
        assert_eq!(
            field_refused_for(|s| s.stops = stops_of(101)),
            "field=stops"
        );
    }

    #[test]
    fn a_project_file_with_a_million_bands_is_refused_when_it_is_read() {
        let set = built_in_surface_styles();
        let mut text = surface_styles_to_json(&set);
        let ten = "\"bands\":10";
        let at = text.find(ten).unwrap_or_else(|| panic!("{text}"));
        text.replace_range(at..at + ten.len(), "\"bands\":1000000");
        let error = surface_styles_from_json(&text).expect_err("a million bands");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.describe().contains("bands"), "{}", error.describe());
        // The same text with a count within the maximum reads.
        let mut fine = surface_styles_to_json(&set);
        let at = fine.find(ten).unwrap_or_else(|| panic!("{fine}"));
        fine.replace_range(at..at + ten.len(), "\"bands\":100");
        assert!(surface_styles_from_json(&fine).is_ok());
    }
}
