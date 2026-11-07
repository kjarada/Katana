//! The twin of tests/archive12d/customisation/test_colour_name_forwarding.cpp.
//!
//! The standard colour names are the entity layer's
//! (katana_entity::colour_names); this crate keeps the three names its import,
//! its export and the front ends have always called them by. What those names
//! promise is held here.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

use std::any::type_name_of_val;

use katana_archive12d::domain::{nearest_standard_colour, standard_colour, standard_colour_names};
use katana_entity::Color;

mod colour_name_forwarding {
    use super::*;

    // A re-export names the item itself: the type of a function item is the
    // item, so its name is the entity layer's path whichever path it was
    // reached by. A copy of the function here would have this crate's path.
    // (The C++ compared function pointers; comparing Rust's tells nothing
    // reliable, as the compiler may merge or split them.)
    #[test]
    fn the_archive_names_are_the_entity_functions_and_not_copies_of_them() {
        let colour: fn(&str) -> Option<Color> = standard_colour;
        let names: fn() -> Vec<String> = standard_colour_names;
        let nearest: fn(Color) -> String = nearest_standard_colour;
        assert_eq!(
            type_name_of_val(&standard_colour),
            type_name_of_val(&katana_entity::colour_names::standard_colour)
        );
        assert_eq!(
            type_name_of_val(&standard_colour_names),
            type_name_of_val(&katana_entity::colour_names::standard_colour_names)
        );
        assert_eq!(
            type_name_of_val(&nearest_standard_colour),
            type_name_of_val(&katana_entity::colour_names::nearest_standard_colour)
        );
        assert_eq!(colour("red"), Some(Color::new(255, 0, 0, 255)));
        assert_eq!(names().len(), 27);
        assert_eq!(nearest(Color::new(0, 0, 255, 255)), "blue");
    }

    // The C++ case is about argument-dependent lookup: a plain call inside the
    // archive namespace with an entity::Color finds entity's function as well
    // as the archive's name for it. Rust has no such lookup; the call through
    // this crate's path is the twin.
    #[test]
    fn a_plain_call_inside_the_archive_namespace_with_a_colour_is_not_ambiguous() {
        // (250, 5, 5) is 5, 5, 5 from red and 111 from dark red in the first
        // channel alone.
        assert_eq!(nearest_standard_colour(Color::new(250, 5, 5, 255)), "red");
        assert_eq!(nearest_standard_colour(Color::new(0, 0, 255, 255)), "blue");
    }

    // An archive names its colours, and the names are read by the one fold of
    // the entity layer: '_' and '-' as a blank, and no blank left at either
    // end. So a name with a separator at its end is the standard colour, where
    // it used to be no colour at all (trimmed first, "red_" became "red " and
    // matched nothing).
    #[test]
    fn an_archives_colour_name_with_a_separator_at_an_end_is_the_standard_colour() {
        assert_eq!(standard_colour("red_"), Some(Color::new(255, 0, 0, 255)));
        assert_eq!(
            standard_colour("_Dark-Green_"),
            Some(Color::new(0, 100, 0, 255))
        );
        assert!(standard_colour("_").is_none());
        assert!(standard_colour("pen_025").is_none());
    }
}
