// The Document's current style (the lead's D9;
// tests/cad/customisation/test_style_catalogue_current_style.cpp): what new
// work is drawn in, refused for a style that does not exist, and CLEARED -
// never left naming nothing - when the style goes by delete, merge, rename or
// undo.

use katana_cad::Document;
use katana_commands::{create_point, create_style, delete_style, merge_style, rename_style};
use katana_core::ErrorCode;
use katana_entity::Style;
use katana_geometry::Point2;

use crate::must;

fn add_style(document: &mut Document, name: &str) {
    let style = Style {
        name: String::from(name),
        ..Style::default()
    };
    must(document, create_style(style));
}

mod current_style {
    use super::*;

    #[test]
    fn new_work_wears_the_current_style_and_by_layer_is_the_default() {
        let mut document = Document::default();
        assert_eq!(document.current_style(), "");
        assert_eq!(
            document.current_attributes().style,
            "",
            "ByLayer until one is chosen"
        );

        add_style(&mut document, "Kerb");
        assert!(document.set_current_style("Kerb").is_ok());
        assert_eq!(document.current_attributes().style, "Kerb");
        assert_eq!(
            document.current_attributes().layer,
            "0",
            "the current layer is unchanged"
        );

        let attributes = document.current_attributes();
        must(
            &mut document,
            create_point(Point2::new(1.0, 1.0), attributes),
        );
        let created = document.last_created_entities();
        assert_eq!(created.len(), 1);
        assert_eq!(
            document
                .model()
                .entities
                .find(created[0])
                .map(|entity| entity.style.as_str()),
            Some("Kerb")
        );

        assert!(document.set_current_style("").is_ok());
        assert_eq!(document.current_attributes().style, "");
    }

    #[test]
    fn a_style_that_does_not_exist_is_refused_and_the_current_one_kept() {
        let mut document = Document::default();
        add_style(&mut document, "Kerb");
        assert!(document.set_current_style("Kerb").is_ok());
        let refused = document
            .set_current_style("nosuch")
            .expect_err("no such style");
        assert_eq!(refused.code, ErrorCode::NotFound);
        assert_eq!(document.current_style(), "Kerb");
        // Case-sensitive, as every name is.
        assert!(document.set_current_style("KERB").is_err());
    }

    #[test]
    fn deleting_merging_or_renaming_the_current_style_clears_it() {
        let mut document = Document::default();
        add_style(&mut document, "Kerb");
        add_style(&mut document, "Kerb 2");

        assert!(document.set_current_style("Kerb 2").is_ok());
        must(&mut document, delete_style("Kerb 2"));
        assert_eq!(document.current_style(), "", "deleted");
        assert!(document.undo().is_ok());
        assert_eq!(
            document.current_style(),
            "",
            "undo brings the style back, not the choice: nothing records it"
        );

        assert!(document.set_current_style("Kerb 2").is_ok());
        must(&mut document, merge_style("Kerb 2", "Kerb"));
        assert_eq!(document.current_style(), "", "merged away");

        assert!(document.set_current_style("Kerb").is_ok());
        must(&mut document, rename_style("Kerb", "Kerb line"));
        assert_eq!(
            document.current_style(),
            "",
            "cleared, not followed: no command event says what a rename went to"
        );

        // A style that stays is kept through other commands.
        assert!(document.set_current_style("Kerb line").is_ok());
        add_style(&mut document, "Other");
        assert_eq!(document.current_style(), "Kerb line");
    }

    #[test]
    fn undoing_the_styles_creation_clears_it_and_a_new_drawing_starts_by_layer() {
        let mut document = Document::default();
        add_style(&mut document, "Kerb");
        assert!(document.set_current_style("Kerb").is_ok());
        assert!(document.undo().is_ok());
        assert_eq!(document.current_style(), "");

        assert!(document.redo().is_ok());
        assert!(document.set_current_style("Kerb").is_ok());
        document.new_document();
        assert_eq!(document.current_style(), "");
    }
}
