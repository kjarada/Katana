//! The twin of tests/entity/test_named_table.cpp: the shared named-table
//! behaviour (named_table.rs).
//!
//! Linetypes, dimension styles and styles were three hand-written copies of
//! one container until they were collapsed onto NamedTable. The two rules that
//! protect the built-in "continuous" entry had no test of any kind: they would
//! have gone on passing if the code had been deleted.

use katana_core::error::ErrorCode;
use katana_entity::tables::{CONTINUOUS_LINETYPE, DEFAULT_DIMENSION_STYLE_NAME};
use katana_entity::{
    DimensionStyle, DimensionStyleDatabase, Linetype, LinetypeDatabase, LinetypeElement, Style,
    StyleDatabase,
};

fn dashed(name: &str) -> Linetype {
    Linetype {
        name: String::from(name),
        pattern: vec![
            LinetypeElement { length: 0.5 },
            LinetypeElement { length: -0.25 },
        ],
        ..Linetype::default()
    }
}

mod named_table {
    use super::*;

    // ---- the rules that protect a built-in entry ---------------------------------

    #[test]
    fn the_continuous_linetype_cannot_be_removed() {
        // Every entity resolves to a linetype, and an unset ByLayer chain ends
        // at this one. Removing it would leave that chain pointing at nothing.
        let mut linetypes = LinetypeDatabase::new();
        let removed = linetypes.remove(CONTINUOUS_LINETYPE);
        assert_eq!(removed.unwrap_err().code, ErrorCode::InvalidArgument);
        assert!(linetypes.contains(CONTINUOUS_LINETYPE));
    }

    #[test]
    fn the_continuous_linetype_cannot_be_given_a_pattern() {
        // Giving it a pattern would silently change what every entity that has
        // not chosen a linetype draws as, across the whole document.
        let mut linetypes = LinetypeDatabase::new();
        let status = linetypes.update(&dashed(CONTINUOUS_LINETYPE));
        assert_eq!(status.unwrap_err().code, ErrorCode::InvalidArgument);
        assert!(linetypes.find(CONTINUOUS_LINETYPE).unwrap().is_continuous());
    }

    #[test]
    fn the_continuous_linetype_may_still_be_updated_in_every_other_respect() {
        // The rule is about the pattern, not about the entry being frozen - so
        // the test proves the guard is narrow rather than a blanket refusal.
        let mut linetypes = LinetypeDatabase::new();
        let mut continuous = linetypes.find(CONTINUOUS_LINETYPE).unwrap().clone();
        continuous.description = String::from("Unbroken");
        assert!(linetypes.update(&continuous).is_ok());
        assert_eq!(
            linetypes.find(CONTINUOUS_LINETYPE).unwrap().description,
            "Unbroken"
        );
    }

    #[test]
    fn the_default_dimension_style_cannot_be_removed() {
        let mut styles = DimensionStyleDatabase::new();
        let removed = styles.remove(DEFAULT_DIMENSION_STYLE_NAME);
        assert_eq!(removed.unwrap_err().code, ErrorCode::InvalidArgument);
        assert!(styles.contains(DEFAULT_DIMENSION_STYLE_NAME));
    }

    #[test]
    fn a_table_with_no_built_in_starts_empty_and_removes_freely() {
        // StylePolicy takes the default is_protected and seed. Asserting that
        // the defaults actually apply is what stops a future policy from
        // inheriting a protection it never asked for.
        let mut styles = StyleDatabase::new();
        assert_eq!(styles.size(), 0);
        let style = Style {
            name: String::from("kerb"),
            ..Style::default()
        };
        assert!(styles.add(style).is_ok());
        assert!(styles.remove("kerb").is_ok());
        assert_eq!(styles.size(), 0);
    }

    // ---- the rules every table shares --------------------------------------------

    #[test]
    fn adding_a_name_that_exists_is_refused_and_changes_nothing() {
        let mut linetypes = LinetypeDatabase::new();
        assert!(linetypes.add(dashed("fence")).is_ok());
        let mut different = dashed("fence");
        different.description = String::from("a different fence");

        let status = linetypes.add(different);
        assert_eq!(status.unwrap_err().code, ErrorCode::AlreadyExists);
        // The rejected add must not have overwritten the original.
        assert_eq!(linetypes.find("fence").unwrap().description, "");
    }

    #[test]
    fn updating_something_absent_is_not_found_rather_than_an_insert() {
        // An update that silently inserted would let a rename lose the
        // original and create a second entry instead.
        let mut linetypes = LinetypeDatabase::new();
        let status = linetypes.update(&dashed("never added"));
        assert_eq!(status.unwrap_err().code, ErrorCode::NotFound);
        assert!(!linetypes.contains("never added"));
    }

    #[test]
    fn removing_something_absent_is_not_found() {
        let mut linetypes = LinetypeDatabase::new();
        let removed = linetypes.remove("never added");
        assert_eq!(removed.unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn the_error_names_the_kind_of_table_so_the_message_reads_as_english() {
        // Each policy carries its own noun.
        let mut linetypes = LinetypeDatabase::new();
        assert!(linetypes.add(dashed("fence")).is_ok());
        let linetype_message = linetypes.add(dashed("fence")).unwrap_err().describe();
        assert!(linetype_message.contains("linetype"), "{linetype_message}");

        let mut styles = DimensionStyleDatabase::new();
        let style = DimensionStyle {
            name: String::from(DEFAULT_DIMENSION_STYLE_NAME),
            ..DimensionStyle::default()
        };
        let style_message = styles.add(style).unwrap_err().describe();
        assert!(style_message.contains("dimension style"), "{style_message}");
    }

    #[test]
    fn names_and_records_come_back_in_name_order_whatever_the_insertion_order() {
        // The project store writes tables in this order, and a stable order is
        // what makes a saved file diffable. Inserted deliberately out of order.
        let mut linetypes = LinetypeDatabase::new();
        for name in ["zigzag", "ditch", "kerb", "aerial"] {
            assert!(linetypes.add(dashed(name)).is_ok());
        }
        let expected = vec!["aerial", "continuous", "ditch", "kerb", "zigzag"];
        assert_eq!(linetypes.names(), expected);

        let from_records: Vec<String> = linetypes
            .all()
            .into_iter()
            .map(|linetype| linetype.name)
            .collect();
        assert_eq!(from_records, expected);
    }

    #[test]
    fn reset_restores_the_built_in_and_nothing_else() {
        let mut linetypes = LinetypeDatabase::new();
        assert!(linetypes.add(dashed("fence")).is_ok());
        assert!(linetypes.remove(CONTINUOUS_LINETYPE).is_err()); // still protected
        linetypes.reset();
        assert_eq!(linetypes.size(), 1);
        assert!(linetypes.contains(CONTINUOUS_LINETYPE));
        assert!(!linetypes.contains("fence"));
    }
}
