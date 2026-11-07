//! The twin of tests/archive12d/test_symbols.cpp (PLAN.MD 20.2 slice 2): an
//! archive's vertex symbol becomes the symbol on a Katana style, on a point;
//! on a line it is kept and written back.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use katana_archive12d::domain::standard_colour;
use katana_archive12d::{
    Archive, DomainImport, ExportOptions, ImportOptions, VertexString, WriteOptions, from_domain,
    symbol_for_linestyle, to_domain, write_archive,
};
use katana_entity::tables::{built_in_symbol_for, is_built_in_symbol_name};
use katana_entity::{Color, Entity, Model, PointGeometry, Style};
use katana_geometry::Point2;

use support::{any_contains, get, import, joined, meta, read};

fn style_named<'d>(domain: &'d DomainImport, name: &str) -> Option<&'d Style> {
    domain.styles_needed.iter().find(|style| style.name == name)
}

// Model built from an import, the way the application does it.
fn model_of(domain: &DomainImport) -> Model {
    let mut model = Model::default();
    for layer in &domain.layers_needed {
        assert!(model.layers.add(layer.clone()).is_ok());
    }
    for style in &domain.styles_needed {
        assert!(model.styles.add(style.clone()).is_ok());
    }
    for entity in &domain.entities {
        assert!(model.entities.add(entity.clone()).is_ok());
    }
    model
}

fn export_of(model: &Model) -> Archive {
    let exported = from_domain(model, &[], &ExportOptions::default(), &[])
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    read(&write_archive(&exported.archive, &WriteOptions::default()))
}

// What version 15 of the format's program writes on a surveyed manhole: the
// string's own linestyle is "0", the symbol's is the feature code's, and both
// share the colour.
const MANHOLE: &str = r#"string super { name "PSMH" breakline point colour yellow style "0"
  data_3d { 100 200 5.5 }
  symbol_data { properties { style "SEWR Manhole Cover" colour yellow size 1 rotation 0 offset 0 raise 0 } } }"#;

mod symbol_import {
    use super::*;

    #[test]
    fn a_point_takes_its_symbols_linestyle_as_its_style_and_the_style_carries_the_shape() {
        let domain = import(MANHOLE);
        assert_eq!(domain.entities.len(), 1);
        let point = &domain.entities[0];
        assert_eq!(point.style, "SEWR Manhole Cover");
        let style = style_named(&domain, "SEWR Manhole Cover").expect("the symbol's style");
        // The REAL library name, kept so a loaded symbol library can be
        // matched against it. What it DRAWS as without one is the shape its
        // words suggest, which is asserted where that guess lives.
        assert_eq!(style.symbol, "SEWR Manhole Cover");
        assert_eq!(
            built_in_symbol_for(&style.symbol),
            "manhole",
            "the name still says what it is"
        );
        assert_eq!(style.symbol_size, 1.0);
        assert_eq!(style.description, "library symbol");
        // The string's own linestyle "0" is not the point's style, but it is
        // kept so that export writes the string as it was.
        assert_eq!(meta(point, "12d.string_style"), Some("0"));
        assert_eq!(point.color, standard_colour("yellow"));
        // Defaults are not metadata: the format's program writes them on every
        // one of thousands.
        assert_eq!(meta(point, "12d.symbol.rotation"), None);
        assert_eq!(meta(point, "12d.symbol.offset"), None);
        assert_eq!(meta(point, "12d.symbol.raise"), None);
        assert_eq!(
            meta(point, "12d.symbol.size"),
            None,
            "the size is on the style"
        );
        assert_eq!(
            meta(point, "12d.symbol.colour"),
            None,
            "the colour is on the entity"
        );
        assert!(
            !any_contains(&domain.warnings, "symbol"),
            "{}",
            joined(&domain.warnings)
        );
    }

    #[test]
    fn the_symbol_value_form_and_the_symbol_data_form_are_one_symbol() {
        let value = import(
            r#"string super { name p breakline point data_3d { 1 2 3 }
  symbol_value { style "ELEC Pole - Power" colour red size 2.5 rotation 0 offset 0 raise 0 } }"#,
        );
        let data = import(
            r#"string super { name p breakline point data_3d { 1 2 3 }
  symbol_data { properties { style "ELEC Pole - Power" colour red size 2.5 rotation 0 offset 0 raise 0 } } }"#,
        );
        assert_eq!(value.entities.len(), 1);
        assert_eq!(data.entities.len(), 1);
        assert_eq!(value.entities[0].style, "ELEC Pole - Power");
        assert_eq!(data.entities[0].style, "ELEC Pole - Power");
        assert_eq!(value.styles_needed, data.styles_needed);
        let style = style_named(&value, "ELEC Pole - Power").expect("the symbol's style");
        assert_eq!(style.symbol, "ELEC Pole - Power");
        assert_eq!(style.symbol_size, 2.5);
    }

    #[test]
    fn what_is_not_on_the_style_or_the_entity_stays_on_the_point_as_metadata() {
        // A rotated, offset symbol in a colour Katana has no RGB for, at a
        // size that is not the size the style was given first.
        let domain = import(
            r#"string super { name a breakline point data_3d { 0 0 0 }
  symbol_value { style "CULT Sign Post" colour red size 1 } }
string super { name b breakline point data_3d { 5 5 0 }
  symbol_value { style "CULT Sign Post" colour "off yellow" size 3 rotation 45 offset 0.5 raise 0 } }"#,
        );
        assert_eq!(domain.entities.len(), 2);
        let style = style_named(&domain, "CULT Sign Post").expect("the symbol's style");
        assert_eq!(
            style.symbol, "CULT Sign Post",
            "the name the fixture declares"
        );
        assert_eq!(
            built_in_symbol_for(&style.symbol),
            "pole",
            "a post is a pole, before a sign is a flag"
        );
        assert_eq!(style.symbol_size, 1.0, "the first size seen");

        let b = &domain.entities[1];
        assert_eq!(b.style, "CULT Sign Post");
        assert_eq!(meta(b, "12d.symbol.size"), Some("3"));
        assert_eq!(meta(b, "12d.symbol.rotation"), Some("45"));
        assert_eq!(meta(b, "12d.symbol.offset"), Some("0.5"));
        assert_eq!(meta(b, "12d.symbol.raise"), None);
        assert_eq!(
            meta(b, "12d.symbol.colour"),
            Some("off yellow"),
            "no RGB for 'off yellow': the name is kept"
        );

        // And every one of them goes back where it came from.
        let archive = export_of(&model_of(&domain));
        assert_eq!(archive.elements.len(), 2);
        let out_b = get::<VertexString>(&archive.elements[1]);
        let symbol = out_b.symbol.as_ref().expect("a symbol");
        assert_eq!(symbol.text("style", ""), "CULT Sign Post");
        assert_eq!(symbol.text("colour", ""), "off yellow");
        assert_eq!(symbol.real("size"), Some(3.0));
        assert_eq!(symbol.real("rotation"), Some(45.0));
        assert_eq!(symbol.real("offset"), Some(0.5));
        assert_eq!(symbol.real("raise"), Some(0.0));
        let out_a = get::<VertexString>(&archive.elements[0]);
        let symbol = out_a.symbol.as_ref().expect("a symbol");
        assert_eq!(symbol.real("size"), Some(1.0), "the style's size");
        assert_eq!(symbol.text("colour", ""), "red");
    }

    #[test]
    fn a_style_met_as_a_line_first_gains_its_symbol_when_a_point_names_it() {
        let domain = import(
            r#"string super { name l style "Tree" data_3d { 0 0 0  1 1 1 } }
string super { name p breakline point style "1" data_3d { 5 5 0 }
  symbol_value { style "Tree" colour green size 2 } }"#,
        );
        let style = style_named(&domain, "Tree").expect("the Tree style");
        assert_eq!(built_in_symbol_for(&style.symbol), "tree");
        assert_eq!(style.symbol_size, 2.0);
        assert_eq!(
            domain.styles_needed.len(),
            2,
            "Tree and 1: one style per name"
        );
    }

    #[test]
    fn a_lines_vertex_symbols_are_kept_as_one_list_per_key_and_written_back_whole() {
        let domain = import(
            r#"string super { name fence data_3d { 0 0 0  10 0 0  20 0 0 }
  symbol_data { properties { style "Post" colour red size 1 rotation 0 offset 0 raise 0 }
                properties { style "Post" colour red size 1 rotation 90 offset 0 raise 0 }
                properties { style "Gate Post" colour "dark green" size 1.5 } } }"#,
        );
        assert_eq!(domain.entities.len(), 1);
        let fence = &domain.entities[0];
        assert_eq!(
            fence.style, "1",
            "the string's own linestyle - the format's default (manual 1.4.3) - and not the vertex symbol's"
        );
        assert!(style_named(&domain, "Post").is_none());
        assert_eq!(
            meta(fence, "12d.symbol.style"),
            Some("Post Post \"Gate Post\"")
        );
        assert_eq!(
            meta(fence, "12d.symbol.rotation"),
            Some("0 90 \"\""),
            "a field the third block lacks is an empty item, so the columns stay aligned"
        );
        assert_eq!(
            meta(fence, "12d.symbol.colour"),
            Some("red red \"dark green\"")
        );
        assert!(
            any_contains(
                &domain.warnings,
                "1 strings carry a DIFFERENT symbol per vertex"
            ),
            "{}",
            joined(&domain.warnings)
        );

        let archive = export_of(&model_of(&domain));
        assert_eq!(archive.elements.len(), 1);
        let out = get::<VertexString>(&archive.elements[0]);
        assert!(out.symbol.is_none());
        assert_eq!(out.symbols.len(), 3);
        assert_eq!(out.symbols[0].text("style", ""), "Post");
        assert_eq!(out.symbols[1].real("rotation"), Some(90.0));
        assert_eq!(out.symbols[2].text("style", ""), "Gate Post");
        assert_eq!(out.symbols[2].text("colour", ""), "dark green");
        assert_eq!(out.symbols[2].real("size"), Some(1.5));
        assert!(
            !out.symbols[2].contains("rotation"),
            "an empty item is a field the block never had"
        );
    }

    #[test]
    fn a_line_with_one_symbol_for_every_vertex_is_written_back_in_the_value_form() {
        let domain = import(
            r#"string super { name l data_3d { 0 0 0  10 0 0 }
  symbol_value { style "Peg" colour red size 0.5 } }"#,
        );
        let archive = export_of(&model_of(&domain));
        let out = get::<VertexString>(&archive.elements[0]);
        let symbol = out.symbol.as_ref().expect("a symbol");
        assert!(out.symbols.is_empty());
        assert_eq!(symbol.text("style", ""), "Peg");
        assert_eq!(symbol.real("size"), Some(0.5));
    }

    #[test]
    fn a_string_colour_and_a_symbol_colour_that_differ_both_survive_the_round_trip() {
        // The format's program writes the same colour name on the string and
        // on its symbol in every sample archive, but the format allows two,
        // and a point then has an appearance (the symbol's) and a string
        // colour that is not it. Neither may be lost: the entity is drawn in
        // the symbol's colour, the string's own name is written back on the
        // string.
        let domain = import(
            r#"string super { name p breakline point colour red style "0"
  data_3d { 0 0 0 }
  symbol_value { style "WATR Stop Valve" colour blue size 1 } }"#,
        );
        assert_eq!(domain.entities.len(), 1);
        let point = &domain.entities[0];
        assert_eq!(
            point.color,
            standard_colour("blue"),
            "a point looks like its symbol"
        );
        assert_eq!(
            meta(point, "12d.colour"),
            Some("red"),
            "the string's own colour is still the string's"
        );
        assert_eq!(meta(point, "12d.symbol.colour"), Some("blue"));

        let archive = export_of(&model_of(&domain));
        let out = get::<VertexString>(&archive.elements[0]);
        assert_eq!(
            out.header.colour, "red",
            "the string goes back out the colour it came in"
        );
        let symbol = out.symbol.as_ref().expect("a symbol");
        assert_eq!(symbol.text("colour", ""), "blue");
    }
}

mod symbol_export {
    use super::*;

    #[test]
    fn a_point_drawn_in_a_style_with_a_symbol_is_written_with_the_block12d_draws_it_from() {
        // A point made in Katana, never near an archive: its style says
        // "manhole".
        let mut model = Model::default();
        let style = Style {
            name: String::from("Manhole"),
            symbol: String::from("manhole"),
            symbol_size: 1.2,
            ..Style::default()
        };
        assert!(model.styles.add(style).is_ok());
        let plain = Style {
            name: String::from("Kerb"),
            ..Style::default()
        };
        assert!(model.styles.add(plain).is_ok());
        for name in ["Manhole", "Kerb"] {
            let entity = Entity {
                geometry: PointGeometry {
                    position: Point2::new(1.0, 2.0),
                }
                .into(),
                layer: String::from("0"),
                style: String::from(name),
                color: Some(Color::new(0, 0, 255, 255)),
                ..Entity::default()
            };
            assert!(model.entities.add(entity).is_ok());
        }
        let archive = export_of(&model);
        assert_eq!(archive.elements.len(), 2);
        let manhole = get::<VertexString>(&archive.elements[0]);
        // What the archive draws is the style's linetype and symbol, never the
        // style's own name, which is Katana's (and may be renamed): a
        // continuous line is the format's default linestyle "1", and the
        // symbol is the name the style gives.
        assert_eq!(manhole.header.style, "1");
        let symbol = manhole.symbol.as_ref().expect("a symbol");
        assert_eq!(symbol.text("style", ""), "manhole");
        assert_eq!(
            symbol.text("colour", ""),
            "blue",
            "the point's colour is the symbol's"
        );
        assert_eq!(symbol.real("size"), Some(1.2));
        assert_eq!(symbol.real("rotation"), Some(0.0));
        assert_eq!(symbol.real("offset"), Some(0.0));
        assert_eq!(symbol.real("raise"), Some(0.0));
        let kerb = get::<VertexString>(&archive.elements[1]);
        assert!(kerb.symbol.is_none(), "a style with no symbol writes none");
    }

    #[test]
    fn the_manhole_comes_back_as_it_went_after_a_full_round_trip() {
        let first = import(MANHOLE);
        let archive = export_of(&model_of(&first));
        let out = get::<VertexString>(&archive.elements[0]);
        assert_eq!(
            out.header.style, "0",
            "the string's own linestyle, not the symbol's"
        );
        assert_eq!(out.header.colour, "yellow");
        let symbol = out.symbol.as_ref().expect("a symbol");
        assert_eq!(symbol.text("style", ""), "SEWR Manhole Cover");
        assert_eq!(symbol.text("colour", ""), "yellow");
        assert_eq!(symbol.real("size"), Some(1.0));
        // And a second pass through Katana finds nothing changed.
        let again = to_domain(&archive, &ImportOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(again.entities[0].style, "SEWR Manhole Cover");
        assert_eq!(again.entities[0].metadata, first.entities[0].metadata);
        assert_eq!(again.styles_needed, first.styles_needed);
    }
}

mod symbol_names {
    use super::*;

    #[test]
    fn the_shape_is_read_from_the_linestyle_name_most_specific_word_first() {
        // The forty commonest symbol linestyles in the real archives, and what
        // a survey plan draws for each.
        let cases = [
            ("TOPO Natural Surface Point", "cross"),
            ("STNS Default MX Survey Mark", "target"),
            ("CULT Fence Post - Guide Post", "pole"),
            ("MISC Unidentified Pole", "pole"),
            ("ELEC Pole - Light", "pole"),
            ("ELEC Suspended Light", "star"),
            ("DRAIN Gully Pit Point", "manhole"),
            ("SEWR Manhole Cover", "manhole"),
            ("DRAIN Invert - 375 Dia", "cross"),
            ("COMM Telephone Single Concrete Pit", "manhole"),
            ("FOTO Vertical Control Point", "target"),
            ("WATR Stop Valve", "diamond"),
            ("WATR Hydrant", "diamond"),
            ("WATR Meter", "diamond"),
            ("TCS Traffic Signal Junction Box", "flag"),
            ("VEG Tree - Deciduous", "tree"),
            ("BDGE Pier-Column Point", "pole"),
            ("BUIL Floor Level", "cross"),
            ("MISC Bollard", "dot"),
            ("ELEC Cable Junction Box", "circle"),
            ("", "circle"),
        ];
        for (name, shape) in cases {
            assert_eq!(symbol_for_linestyle(name), shape, "{name}");
            assert!(
                is_built_in_symbol_name(symbol_for_linestyle(name)),
                "{name}"
            );
        }
    }
}
