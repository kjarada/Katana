//! The customisations compiled into the program, when they are the ones these figures are of: the
//! reference one and Katana Standard (tests/cad/customisation/test_builtin_census.cpp).
//!
//! The reference customisation is third-party material under its author's own licence, tracked as
//! resources/customisation/nsw.customisation.json on the owner's decision (docs/customisation.md,
//! "Why the built-in is in the repository"), so a build from a clean checkout has it compiled in,
//! as an ALTERNATE built-in (Katana Standard is the default), and these tests run. A build whose
//! alternates name no such file has no such built-in and every test here SKIPS: the suite must
//! stay green without it. Where it is compiled in (built_ins() lists a customisation named
//! "NSW"), these are the figures it must hold.
//!
//! Where the figures come from: not from this program. They were counted by two scripts that use
//! none of its code - tools/customisation_census.py over the compiled-in file and
//! tools/reference_census.py over the four legacy files it was converted from - which agree figure
//! for figure. Nothing here spells a definition, a group, a layer, a code or a comment of it:
//! counts only, and a property that a test can find without naming anything.
//!
//! What a build has compiled in is asked of the build, never of the environment: the seam changes
//! what a program's front end starts with, and not what was compiled in. It is found by NAME in
//! the list of what was compiled in, because which one is the default is the build's choice and no
//! figure here depends on it.

use std::collections::BTreeMap;

use katana_cad::customisation_host::{BuiltInEntry, built_ins, find_built_in};
use katana_entity::Customisation;
use katana_entity::style_library::style_groups;

// The compiled-in built-in of this name, by its entry; None when this build did not compile one
// in.
fn compiled_in_entry(name: &str) -> Option<&'static BuiltInEntry> {
    find_built_in(built_ins(), name).filter(|entry| entry.built_in.customisation.is_some())
}

// The compiled-in reference customisation; None when this build has none.
fn reference_built_in() -> Option<&'static Customisation> {
    compiled_in_entry("NSW").and_then(|entry| entry.built_in.customisation.as_deref())
}

const SKIPPED: &str = "this build has no reference customisation compiled in";

mod built_in_census {
    use super::*;

    #[test]
    fn the_compiled_in_reference_customisation_holds_the_figures_of_its_census() {
        let Some(nsw) = reference_built_in() else {
            eprintln!("{SKIPPED}");
            return;
        };
        let mut symbols = 0usize;
        let mut linestyles = 0usize;
        let mut at_vertices = 0usize;
        let mut strokes = 0usize;
        nsw.library.for_each(|style| {
            if style.symbol {
                symbols += 1;
            } else {
                linestyles += 1;
            }
            at_vertices += usize::from(style.at_vertices);
            strokes += style.strokes.len();
        });
        // 792 definitions: 471 listed as symbols and 321 not. The census counts
        // the symbol library's 474 names and the linestyle library's 321 and says
        // three are in both; the library read last keeps them, so those three are
        // linestyles: 474 - 3 = 471.
        assert_eq!(nsw.library.size(), 792);
        assert_eq!(symbols, 471);
        assert_eq!(linestyles, 321);
        assert_eq!(symbols + linestyles, nsw.library.size());
        assert_eq!(at_vertices, 155);
        assert_eq!(style_groups(&nsw.library).len(), 71);
        // The census's `opsInLibrary`: 17,020 + 17,222 + 104 + 312 + 178 + 342 +
        // 514 strokes, text strokes included.
        assert_eq!(strokes, 35692);
        // 725 + 899 rules of the two survey code files, in 632 distinct keys.
        assert_eq!(nsw.map.size(), 1624);
        assert_eq!(nsw.map.keys().len(), 632);
        assert_eq!(nsw.colours.size(), 7);

        // Five of the names the rules use are defined by no library: the plain
        // continuous lines "0" and "1", and three the customisation expects to be
        // supplied elsewhere.
        let unresolved: Vec<String> = nsw
            .map
            .styles_referenced()
            .into_iter()
            .filter(|name| !nsw.library.contains(name))
            .collect();
        assert_eq!(unresolved.len(), 5);
        assert!(unresolved.iter().any(|name| name == "0"));
        assert!(unresolved.iter().any(|name| name == "1"));
    }

    // Each definition says which customisation it came from by that
    // customisation's NAME - never a file, never a path - so a project that
    // records it records "NSW".
    #[test]
    fn every_definition_is_from_the_customisation_itself_by_its_name() {
        let Some(nsw) = reference_built_in() else {
            eprintln!("{SKIPPED}");
            return;
        };
        let mut elsewhere = 0usize;
        nsw.library.for_each(|style| {
            elsewhere += usize::from(style.source != nsw.name);
        });
        assert_eq!(elsewhere, 0);
        // The digest of the entry for this very customisation, not of the default.
        let entry = compiled_in_entry("NSW").expect("the reference entry");
        assert!(!entry.built_in.digest.is_empty());
    }

    // A group path says what a definition is. A word put in front of most of
    // them says only whose the customisation was, and is not wanted: no first
    // word - up to the first blank - may begin the group path of more than half
    // the definitions. Nothing here names such a word; the test finds it if
    // there is one. (It was a test of the legacy built-in's four files, and the
    // property is the data's, so it is kept here.)
    #[test]
    fn no_word_begins_the_group_path_of_most_definitions() {
        let Some(nsw) = reference_built_in() else {
            eprintln!("{SKIPPED}");
            return;
        };
        let mut first_words: BTreeMap<String, usize> = BTreeMap::new();
        let mut grouped = 0usize;
        nsw.library.for_each(|style| {
            if style.group.is_empty() {
                return;
            }
            grouped += 1;
            let word = style.group.split(' ').next().unwrap_or("");
            *first_words.entry(String::from(word)).or_default() += 1;
        });
        assert!(grouped > 0, "the definitions are grouped");
        for count in first_words.values() {
            assert!(
                count * 2 <= grouped,
                "a word begins {count} of {grouped} group paths"
            );
        }
    }
}
