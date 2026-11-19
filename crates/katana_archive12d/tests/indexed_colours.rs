//! An archive's AutoCAD Colour Index names ("acad 001") draw in the index's
//! colour, and a name nothing defines ("vis ...") is said once, in one line.
//!
//! The fixture, tests/archive12d/data/indexed_colours.12da, is invented: two
//! meshes coloured "acad 001" and "acad 005" (the second with two coloured
//! faces, "acad 010" and "vis something"), and five strings coloured "ACAD
//! 5", "acad 010", "vis something", "acad 000" and "Vis_Something". The
//! expected RGB are the published table's (acadrgb.txt; index 1 red, 5 blue,
//! 10 the first hue at full brightness, red again), not this program's.

pub mod support;

use katana_archive12d::{ImportOptions, ReadOptions, read_archive_bytes, to_domain};
use katana_entity::Color;

use support::{fixture, joined, prop_text};

fn rgb(r: u8, g: u8, b: u8) -> Option<Color> {
    Some(Color::new(r, g, b, 255))
}

fn imported() -> katana_archive12d::DomainImport {
    let archive = read_archive_bytes(fixture("indexed_colours.12da"), &ReadOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    assert!(archive.unrecognised.is_empty());
    to_domain(&archive, &ImportOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod indexed_colour_names {
    use super::*;

    #[test]
    fn a_mesh_coloured_by_an_index_name_has_the_index_colour() {
        let domain = imported();
        assert_eq!(domain.meshes.len(), 2);
        assert_eq!(domain.meshes[0].name, "Red Slab");
        assert_eq!(domain.meshes[0].colour_name, "acad 001");
        assert_eq!(domain.meshes[0].color, rgb(255, 0, 0));
        assert_eq!(domain.meshes[1].name, "Blue Slab");
        assert_eq!(domain.meshes[1].color, rgb(0, 0, 255));
    }

    #[test]
    fn a_face_coloured_by_an_index_name_has_the_index_colour_and_an_unknown_one_none() {
        let domain = imported();
        let faces = &domain.meshes[1].face_colors;
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[0], rgb(255, 0, 0), "acad 010");
        assert_eq!(faces[1], None, "vis something: the mesh's own colour");
    }

    #[test]
    fn an_index_name_is_read_in_any_case_with_or_without_its_padding() {
        let domain = imported();
        assert_eq!(domain.entities.len(), 5, "{}", joined(&domain.warnings));
        assert_eq!(domain.entities[0].color, rgb(0, 0, 255), "ACAD 5");
        assert_eq!(domain.entities[1].color, rgb(255, 0, 0), "acad 010");
        // The name as written is kept, as every colour name is, for export.
        assert_eq!(
            prop_text(&domain.entities[0].metadata, "12d.colour"),
            "ACAD 5"
        );
    }

    #[test]
    fn by_block_is_the_inherited_colour_and_not_reported() {
        let domain = imported();
        assert_eq!(domain.entities[3].color, None, "acad 000");
        assert!(
            !domain
                .warnings
                .iter()
                .any(|warning| warning.contains("\"acad 000\"")),
            "{}",
            joined(&domain.warnings)
        );
    }

    #[test]
    fn a_name_nothing_defines_keeps_the_inherited_colour_and_is_said_once_in_one_line() {
        let domain = imported();
        assert_eq!(domain.entities[2].color, None);
        assert_eq!(domain.entities[4].color, None);
        assert_eq!(
            prop_text(&domain.entities[2].metadata, "12d.colour"),
            "vis something"
        );
        // One line for the colours, naming the one unknown name once (its
        // second spelling, "Vis_Something", folds to it) and no index name.
        let lines: Vec<&String> = domain
            .warnings
            .iter()
            .filter(|warning| warning.contains("colour name"))
            .collect();
        assert_eq!(lines.len(), 1, "{}", joined(&domain.warnings));
        let line = lines[0];
        assert!(
            line.starts_with("1 colour name is not a standard name"),
            "{line}"
        );
        assert_eq!(line.matches("\"vis something\"").count(), 1, "{line}");
        assert!(!line.contains("Vis_Something"), "{line}");
        for indexed in ["\"acad 001\"", "\"acad 005\"", "\"ACAD 5\"", "\"acad 010\""] {
            assert!(
                !domain
                    .warnings
                    .iter()
                    .any(|warning| warning.contains(indexed)),
                "{indexed}: {}",
                joined(&domain.warnings)
            );
        }
        // And it says where such a name can be given its colour.
        assert!(line.contains("customisation's \"colours\""), "{line}");
    }

    #[test]
    fn a_customisation_colour_resolves_a_name_the_archive_does_not_define() {
        let archive = read_archive_bytes(fixture("indexed_colours.12da"), &ReadOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut options = ImportOptions::default();
        options
            .colours
            .add("vis something", Color::new(200, 190, 170, 255))
            .expect("a name of the customisation's own");
        let domain =
            to_domain(&archive, &options).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(domain.entities[2].color, rgb(200, 190, 170));
        assert_eq!(domain.meshes[1].face_colors[1], rgb(200, 190, 170));
        assert!(
            !domain
                .warnings
                .iter()
                .any(|warning| warning.contains("colour name")),
            "{}",
            joined(&domain.warnings)
        );
    }

    #[test]
    fn twenty_one_unknown_names_list_twenty_and_count_the_rest() {
        // MAX_NAMED_PROBLEMS (20) names at most, and "and 1 more".
        let mut text = String::new();
        for i in 0..21 {
            text.push_str(&format!(
                "string super {{ colour \"vis shade {i}\" data_2d {{ 0 {i} 1 {i} }} }}\n"
            ));
        }
        let domain = support::import(&text);
        let line = domain
            .warnings
            .iter()
            .find(|warning| warning.contains("colour names"))
            .unwrap_or_else(|| panic!("{}", joined(&domain.warnings)));
        assert!(line.starts_with("21 colour names are not"), "{line}");
        assert!(line.contains("\"vis shade 19\""), "{line}");
        assert!(!line.contains("\"vis shade 20\""), "{line}");
        assert!(line.contains("and 1 more"), "{line}");
    }
}
