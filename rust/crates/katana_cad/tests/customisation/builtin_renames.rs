//! The renames the drawing side knows (customisation_record) against the built-in customisation:
//! the names they give now must be the ones the built-in has, or a project or drawing saved before
//! the rename would find nothing (tests/cad/customisation/test_builtin_renames.cpp).
//!
//! Two built-ins are held to it. A small fixture handed over as a host hands one over - always,
//! whatever this build compiled in - and the NSW customisation compiled into the program, when
//! there is one: its library for the definition renames (the 29 definitions were its own), and its
//! name, which the file renames answer with in every build. The NSW one is third-party material,
//! tracked as resources/customisation/nsw.customisation.json, so the half of a test that needs it
//! runs in a build made from a clean checkout and SKIPS in a build that does not compile it in.

use katana_cad::Document;
use katana_cad::customisation_host::{
    BuiltInCustomisation, CustomisationHost, built_ins, compiled_in_customisation, find_built_in,
    start_customisation,
};
use katana_cad::customisation_record::{
    EARLIER_FILE_NAMES_BELONG_TO, builtin_definition_renames, builtin_renames,
    builtin_renames_default, definition_name_now,
};

use crate::customisation_support::built_in_from;

// A built-in as a host hands one over: two definitions and a rule, written for this test, none of
// them of the reference customisation.
const FIXTURE: &str = r#"{
  "format": "katana-customisation", "version": 1,
  "name": "TEST Built In",
  "linestyles": [
    {"name": "TEST Fence", "strokes": [["move", 0, 0], ["draw", 2, 0]]}
  ],
  "symbols": [
    {"name": "TEST Peg", "atVertices": true, "strokes": [["move", 0, 0], ["circle", 0.5]]}
  ],
  "codes": [
    {"key": "FE*", "sets": "feature", "layer": "TEST FENCES", "linestyle": "TEST Fence"}
  ]
}"#;

fn fixture_built_in() -> BuiltInCustomisation {
    built_in_from(FIXTURE)
}

mod built_in_renames {
    use super::*;

    // Eight earlier names in two sets of four (customisation_record), and the
    // table answers for every one of them with the NAME it is given - one
    // customisation now stands where four files did. Which name that is for a
    // session is not its host's built-in's: see the next test.
    #[test]
    fn the_file_renames_answer_with_the_name_the_table_is_given() {
        let mut document = Document::default();
        let host = CustomisationHost {
            built_in: fixture_built_in(),
            ..CustomisationHost::default()
        };
        assert!(host.built_in.customisation.is_some());
        let start = start_customisation(&mut document, &host);
        assert!(start.problems.is_empty());

        // The host's built-in is the built-in of the run (CustomisationState::built_in)...
        assert_eq!(document.customisation_state().built_in, "TEST Built In");
        // ...and a table made for any name answers with it.
        for name in ["TEST Built In", "Another"] {
            let renames = builtin_renames(name);
            assert_eq!(renames.len(), 8);
            for rename in &renames {
                assert_eq!(rename.now, name);
            }
        }
    }

    // The eight earlier names were the four files of the NSW customisation, so
    // they are answered by that one and by no other: the default of a program is
    // Katana Standard, which defines none of what they drew, and answering them
    // with it would say that a project drawn with NSW's linestyles has its
    // customisation loaded when it has not. The form with no name is the
    // Document's table (Document::customisation_renames), whatever this build
    // compiled in.
    #[test]
    fn the_file_renames_with_no_name_answer_with_nsw_whatever_is_compiled_in() {
        let renames = builtin_renames_default();
        assert_eq!(renames.len(), 8);
        for rename in &renames {
            assert_eq!(rename.now, "NSW");
        }
        assert_eq!(EARLIER_FILE_NAMES_BELONG_TO, "NSW");
        // And not with the default's name, which is what they were answered with
        // when the default was NSW.
        if let Some(compiled_in) = &compiled_in_customisation().customisation
            && compiled_in.name != "NSW"
        {
            for rename in &renames {
                assert_ne!(rename.now, compiled_in.name);
            }
        }
    }

    // Each definition rename gives a name the built-in library defines, and no
    // built-in definition is itself known to the table as an earlier name, so
    // the retry after a missed lookup can only ever land on a definition that is
    // there.
    #[test]
    fn no_definition_of_a_fixture_built_in_is_known_as_an_earlier_name() {
        let built_in = fixture_built_in();
        let customisation = built_in.customisation.as_ref().expect("the fixture");
        let mut known = 0usize;
        customisation.library.for_each(|style| {
            known += usize::from(!definition_name_now(&style.name).is_empty());
        });
        assert_eq!(known, 0);
        // And the table itself: a name it gives is never one it also renames, so
        // a retry cannot be sent on a second time.
        for rename in builtin_definition_renames() {
            assert!(definition_name_now(rename.now).is_empty(), "{}", rename.now);
        }
    }

    #[test]
    fn every_renamed_definition_is_in_the_nsw_built_in_library_under_the_name_it_has_now() {
        // The library of the NSW built-in, found by name among what was compiled
        // in: the default is Katana Standard, which holds none of the 29.
        let Some(nsw) = find_built_in(built_ins(), "NSW")
            .and_then(|entry| entry.built_in.customisation.as_ref())
        else {
            eprintln!("this build has no NSW customisation compiled in");
            return;
        };
        // 29 definitions carried the publisher's word, which the reference
        // customisation was given without (the census's `carried`).
        assert_eq!(builtin_definition_renames().len(), 29);
        let library = &nsw.library;
        for rename in builtin_definition_renames() {
            assert!(library.find(rename.now).is_some(), "{}", rename.now);
        }
        let mut known = 0usize;
        library.for_each(|style| {
            known += usize::from(!definition_name_now(&style.name).is_empty());
        });
        assert_eq!(known, 0);
    }
}
