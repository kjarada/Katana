//! The twin of tests/entity/customisation/test_colour_names.cpp: the one fold,
//! the standard names, a customisation's own table and the resolver that asks
//! both (colour_names.rs).
//!
//! The standard RGB values asserted here are those of the CSS named colours
//! (CSS Color Module Level 4, section 6.1), written out by hand, and the two
//! departures from that list the table's own comment states.
//!
//! ColourTable.AnEmptyNameAndANameThatIsNotTextAreRefused also hands the table
//! "caf\xFF", which a String cannot hold: its UTF-8 half is the twin.

use katana_core::error::ErrorCode;
use katana_entity::colour_names::{
    fold_colour_name, nearest_standard_colour, resolve_colour, standard_colour,
    standard_colour_names,
};
use katana_entity::{Color, ColourTable};

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::new(r, g, b, a)
}

mod colour_names {
    use super::*;

    // ---- the fold ---------------------------------------------------------------------

    #[test]
    fn a_folded_name_is_lower_case_trimmed_and_reads_underscore_and_hyphen_as_a_blank() {
        assert_eq!(fold_colour_name("red"), "red");
        assert_eq!(fold_colour_name("RED"), "red");
        assert_eq!(fold_colour_name("  Dark Green\t"), "dark green");
        assert_eq!(fold_colour_name("dark_green"), "dark green");
        assert_eq!(fold_colour_name("Dark-Green"), "dark green");
        assert_eq!(fold_colour_name("SUI Test_Water"), "sui test water");
        assert_eq!(fold_colour_name(""), "");
        assert_eq!(fold_colour_name("   "), "");
        // Only ASCII letters change case: a name in another script is left as
        // it is, byte for byte (u-umlaut is C3 BC, its capital C3 9C).
        assert_eq!(fold_colour_name("GR\u{DC}N"), "gr\u{DC}n");
        // Blanks INSIDE a name are kept as they are, so two blanks are not one.
        assert_eq!(fold_colour_name("dark  red"), "dark  red");
        assert_eq!(fold_colour_name("dark__red"), "dark  red");
    }

    // '_' and '-' are read as a blank, and a folded name has no blank at either
    // end: so a separator at an end goes, as a blank there does. ("red_" used to
    // fold to "red " - trimmed first, the separator turned into a blank
    // afterwards - which is a name with a blank at its end and, to the standard
    // names, not red.)
    #[test]
    fn a_separator_at_either_end_is_folded_away_like_a_blank_there() {
        assert_eq!(fold_colour_name("red_"), "red");
        assert_eq!(fold_colour_name("_red"), "red");
        assert_eq!(fold_colour_name("-red-"), "red");
        assert_eq!(fold_colour_name(" _ Dark_Green - "), "dark green");
        assert_eq!(fold_colour_name("sui gas_"), "sui gas");
        assert_eq!(fold_colour_name("_"), "");
        assert_eq!(fold_colour_name(" -_- "), "");
        // "gray" is looked for in what is left: it is the last word once the
        // separator after it has gone.
        assert_eq!(fold_colour_name("Dark_Gray_"), "dark grey");
        assert_eq!(fold_colour_name("-gray-"), "grey");

        // So the standard names and a table read such a name as they read it
        // with a blank there.
        assert_eq!(standard_colour("red_"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(
            standard_colour("_Light-Grey_"),
            standard_colour("light grey")
        );
        let mut table = ColourTable::default();
        assert!(table.add("sui gas_", rgba(255, 255, 0, 255)).is_ok());
        assert_eq!(table.find("sui gas"), Some(rgba(255, 255, 0, 255)));
        assert_eq!(table.find("SUI-GAS "), Some(rgba(255, 255, 0, 255)));
        let alike = table.add("sui gas", rgba(1, 2, 3, 255));
        assert_eq!(
            alike
                .expect_err("\"sui gas_\" and \"sui gas\" are one name")
                .code,
            ErrorCode::InvalidArgument
        );
        let standard = table.add("red_", rgba(1, 2, 3, 255));
        assert_eq!(
            standard.expect_err("\"red_\" is the standard red").code,
            ErrorCode::InvalidArgument
        );
    }

    // The fold is the form a name is COMPARED in, so folding what is already
    // folded must change nothing: otherwise "the fold of a name" would depend on
    // how many times it had been taken, and a table keyed by one fold would not
    // be found by another. Every kind of name the fold does something to is
    // here, and each is checked to be one it does change.
    #[test]
    fn folding_a_folded_name_changes_nothing() {
        let mut changed = 0;
        for name in [
            "red",
            "RED",
            "  Dark Green\t",
            "dark_green",
            "Dark-Green",
            "red_",
            "_red",
            "-red-",
            " _ Dark_Green - ",
            "gray",
            "Dark_Gray_",
            "-gray-",
            "sui dark-gray",
            "grayish",
            "gray blue",
            "dark  red",
            "dark__red",
            "_",
            " -_- ",
            "",
            "GR\u{DC}N",
            "a_-_b",
            "\tsui_test water\n",
        ] {
            let once = fold_colour_name(name);
            assert_eq!(fold_colour_name(&once), once, "\"{name}\"");
            if once != name {
                changed += 1;
            }
        }
        // By hand: of the 23 names, "red", "grayish", "gray blue", "dark  red"
        // and "" are already in their folded form; the fold changes the other
        // 18.
        assert_eq!(changed, 18);
    }

    #[test]
    fn gray_is_grey_where_it_is_the_whole_name_or_its_last_word() {
        assert_eq!(fold_colour_name("gray"), "grey");
        assert_eq!(fold_colour_name("GRAY"), "grey");
        assert_eq!(fold_colour_name("Dark Gray"), "dark grey");
        assert_eq!(fold_colour_name("light_gray"), "light grey");
        assert_eq!(fold_colour_name("sui dark-gray"), "sui dark grey");
        // Not inside a word, and not a word that is not the last.
        assert_eq!(fold_colour_name("grayish"), "grayish");
        assert_eq!(fold_colour_name("gray blue"), "gray blue");
        assert_eq!(fold_colour_name("stingray"), "stingray");
    }

    // ---- the standard names -----------------------------------------------------------

    #[test]
    fn the_standard_names_are_twenty_seven_plain_then_dark_then_light() {
        // 14 plain names, 7 dark and 6 light, in that order.
        let expected = [
            "red",
            "green",
            "blue",
            "yellow",
            "cyan",
            "magenta",
            "white",
            "black",
            "grey",
            "orange",
            "brown",
            "purple",
            "pink",
            "violet",
            "dark red",
            "dark green",
            "dark blue",
            "dark cyan",
            "dark magenta",
            "dark orange",
            "dark grey",
            "light grey",
            "light blue",
            "light green",
            "light cyan",
            "light yellow",
            "light pink",
        ];
        assert_eq!(expected.len(), 27);
        assert_eq!(standard_colour_names(), expected);
        for name in expected {
            assert!(standard_colour(name).is_some(), "{name}");
            assert_eq!(
                fold_colour_name(name),
                name,
                "a listed name is already in its folded form"
            );
        }
    }

    #[test]
    fn a_standard_name_has_the_rgb_the_same_name_has_in_the_css_list() {
        // CSS: red #FF0000, blue #0000FF, yellow #FFFF00, cyan #00FFFF, magenta
        // #FF00FF, white #FFFFFF, black #000000, gray #808080, orange #FFA500,
        // brown #A52A2A, purple #800080, pink #FFC0CB, violet #EE82EE, darkred
        // #8B0000, darkgreen #006400, darkblue #00008B, darkcyan #008B8B,
        // darkmagenta #8B008B, darkorange #FF8C00, lightblue #ADD8E6,
        // lightgreen #90EE90, lightcyan #E0FFFF, lightyellow #FFFFE0, lightpink
        // #FFB6C1.
        for (name, colour) in [
            ("red", rgba(0xFF, 0x00, 0x00, 255)),
            ("blue", rgba(0x00, 0x00, 0xFF, 255)),
            ("yellow", rgba(0xFF, 0xFF, 0x00, 255)),
            ("cyan", rgba(0x00, 0xFF, 0xFF, 255)),
            ("magenta", rgba(0xFF, 0x00, 0xFF, 255)),
            ("white", rgba(0xFF, 0xFF, 0xFF, 255)),
            ("black", rgba(0x00, 0x00, 0x00, 255)),
            ("grey", rgba(0x80, 0x80, 0x80, 255)),
            ("orange", rgba(0xFF, 0xA5, 0x00, 255)),
            ("brown", rgba(0xA5, 0x2A, 0x2A, 255)),
            ("purple", rgba(0x80, 0x00, 0x80, 255)),
            ("pink", rgba(0xFF, 0xC0, 0xCB, 255)),
            ("violet", rgba(0xEE, 0x82, 0xEE, 255)),
            ("dark red", rgba(0x8B, 0x00, 0x00, 255)),
            ("dark green", rgba(0x00, 0x64, 0x00, 255)),
            ("dark blue", rgba(0x00, 0x00, 0x8B, 255)),
            ("dark cyan", rgba(0x00, 0x8B, 0x8B, 255)),
            ("dark magenta", rgba(0x8B, 0x00, 0x8B, 255)),
            ("dark orange", rgba(0xFF, 0x8C, 0x00, 255)),
            ("light blue", rgba(0xAD, 0xD8, 0xE6, 255)),
            ("light green", rgba(0x90, 0xEE, 0x90, 255)),
            ("light cyan", rgba(0xE0, 0xFF, 0xFF, 255)),
            ("light yellow", rgba(0xFF, 0xFF, 0xE0, 255)),
            ("light pink", rgba(0xFF, 0xB6, 0xC1, 255)),
        ] {
            assert_eq!(standard_colour(name), Some(colour), "{name}");
        }
        // The departures. CSS "green" is the half-bright #008000; a CAD palette
        // means the primary, CSS "lime". And CSS darkgray (#A9A9A9) is LIGHTER
        // than gray, so the greys are set either side of it instead: a quarter
        // and three quarters.
        assert_eq!(standard_colour("green"), Some(rgba(0, 255, 0, 255)));
        assert_eq!(standard_colour("dark grey"), Some(rgba(64, 64, 64, 255)));
        assert_eq!(
            standard_colour("light grey"),
            Some(rgba(192, 192, 192, 255))
        );
    }

    #[test]
    fn a_standard_name_is_found_however_it_is_spelled_and_no_other_name_is() {
        assert_eq!(standard_colour("RED"), standard_colour("red"));
        assert_eq!(
            standard_colour(" Dark_Green "),
            standard_colour("dark green")
        );
        assert_eq!(standard_colour("Light-Gray"), standard_colour("light grey"));
        assert_eq!(standard_colour("gray"), Some(rgba(128, 128, 128, 255)));
        for name in [
            "",
            "pen 025",
            "sui test water",
            "off lime",
            "dark",
            "reddish",
            "dark  red",
            "darkred",
        ] {
            assert!(standard_colour(name).is_none(), "\"{name}\"");
        }
    }

    #[test]
    fn the_nearest_standard_name_is_the_one_at_the_least_distance_in_rgb() {
        // Each name is its own nearest: no two share an RGB.
        for name in standard_colour_names() {
            assert_eq!(
                nearest_standard_colour(standard_colour(&name).unwrap()),
                name
            );
        }
        // (250, 5, 5) is 5, 5, 5 from red - 75 squared - and 111 at least from
        // dark red in the first channel alone.
        assert_eq!(nearest_standard_colour(rgba(250, 5, 5, 255)), "red");
        // (100, 100, 100): from grey (128) 3 x 28^2 = 2352, from dark grey (64)
        // 3 x 36^2 = 3888.
        assert_eq!(nearest_standard_colour(rgba(100, 100, 100, 255)), "grey");
        // (90, 90, 90): from grey 3 x 38^2 = 4332, from dark grey 3 x 26^2 =
        // 2028.
        assert_eq!(nearest_standard_colour(rgba(90, 90, 90, 255)), "dark grey");
        // Opacity is not part of the distance.
        assert_eq!(nearest_standard_colour(rgba(0, 0, 255, 0)), "blue");
    }

    // ---- the resolver -----------------------------------------------------------------

    #[test]
    fn a_name_is_resolved_from_the_table_then_from_the_standard_names() {
        let mut table = ColourTable::default();
        assert!(table.add("sui ember", rgba(255, 127, 0, 255)).is_ok());

        assert_eq!(
            resolve_colour(&table, "sui ember"),
            Some(rgba(255, 127, 0, 255))
        );
        assert_eq!(
            resolve_colour(&table, "Dark_Green"),
            Some(rgba(0, 100, 0, 255))
        );
        assert!(
            resolve_colour(&table, "pen 035").is_none(),
            "a name neither knows leaves the colour alone"
        );
        assert!(resolve_colour(&table, "").is_none());

        // With no table at all, the standard names are still there.
        assert_eq!(
            resolve_colour(&ColourTable::default(), "red"),
            Some(rgba(255, 0, 0, 255))
        );
        assert!(resolve_colour(&ColourTable::default(), "sui ember").is_none());
    }
}

mod colour_table {
    use super::*;

    #[test]
    fn a_name_is_found_by_its_fold_and_kept_as_it_was_written() {
        let mut table = ColourTable::default();
        assert!(table.empty());
        assert!(table.add("sui ember", rgba(255, 127, 0, 255)).is_ok());
        assert!(table.add("sui_gas", rgba(255, 255, 0, 128)).is_ok());
        assert_eq!(table.size(), 2);

        assert_eq!(table.find("sui ember"), Some(rgba(255, 127, 0, 255)));
        assert_eq!(table.find(" Sui-Ember "), Some(rgba(255, 127, 0, 255)));
        assert_eq!(
            table.find("SUI GAS"),
            Some(rgba(255, 255, 0, 128)),
            "opacity and all"
        );
        assert!(table.find("sui water").is_none());
        assert!(table.find("").is_none());

        // In the order of the folds - "sui ember" before "sui gas" - with each
        // name as its author wrote it.
        let entries = table.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "sui ember");
        assert_eq!(entries[1].name, "sui_gas");
        assert_eq!(entries[1].colour, rgba(255, 255, 0, 128));
    }

    #[test]
    fn the_entries_come_in_the_order_of_their_folds_not_of_their_bytes() {
        // By bytes "Zinc" (5A) comes before "apple" (61); folded, "apple" first.
        let mut table = ColourTable::default();
        assert!(table.add("Zinc", rgba(1, 1, 1, 255)).is_ok());
        assert!(table.add("apple", rgba(2, 2, 2, 255)).is_ok());
        assert!(table.add("Mid_tone", rgba(3, 3, 3, 255)).is_ok());
        let entries = table.entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "apple");
        assert_eq!(entries[1].name, "Mid_tone");
        assert_eq!(entries[2].name, "Zinc");
    }

    #[test]
    fn an_empty_name_and_a_name_that_is_not_text_are_refused() {
        let mut table = ColourTable::default();
        // Nothing, blanks, and nothing but the separators the fold reads as
        // blanks.
        for name in ["", "   ", "\t", "_", "-", " _- "] {
            let status = table.add(name, Color::default());
            assert_eq!(
                status.expect_err(&format!("\"{name}\"")).code,
                ErrorCode::InvalidArgument
            );
        }
        // (A name that is not UTF-8, the C++ case's other half, cannot be made
        // into a String.)
        assert!(table.empty(), "a refused name leaves the table as it was");
    }

    #[test]
    fn two_names_with_one_fold_are_refused_and_the_first_is_kept() {
        let mut table = ColourTable::default();
        assert!(table.add("SUI Gas", rgba(255, 255, 0, 255)).is_ok());
        for again in ["SUI Gas", "sui gas", "sui_gas", "Sui-Gas", " sui gas "] {
            let error = table.add(again, rgba(1, 2, 3, 255)).expect_err(again);
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            // Both spellings are in what it says.
            assert!(
                error.describe().contains("\"SUI Gas\""),
                "{}",
                error.describe()
            );
            assert!(
                error.describe().contains(&format!("\"{again}\"")),
                "{}",
                error.describe()
            );
        }
        assert_eq!(table.size(), 1);
        assert_eq!(table.find("sui gas"), Some(rgba(255, 255, 0, 255)));
    }

    #[test]
    fn a_standard_name_cannot_be_given_another_colour() {
        let mut table = ColourTable::default();
        // Every one of the 27, and spellings that fold to one of them.
        let mut names = standard_colour_names();
        names.extend(["Red", "DARK_GREEN", "light-gray", "Gray"].map(String::from));
        for name in &names {
            let status = table.add(name.as_str(), rgba(1, 2, 3, 255));
            assert_eq!(status.expect_err(name).code, ErrorCode::InvalidArgument);
        }
        assert!(table.empty());
        // A name that only CONTAINS one is another name.
        assert!(table.add("off red", rgba(200, 0, 0, 255)).is_ok());
        assert!(table.add("reddish", rgba(210, 0, 0, 255)).is_ok());
    }

    #[test]
    fn two_tables_are_equal_when_they_hold_the_same_names_as_written_and_the_same_colours() {
        let mut a = ColourTable::default();
        let mut b = ColourTable::default();
        assert_eq!(a, b);
        assert!(a.add("sui gas", rgba(255, 255, 0, 255)).is_ok());
        assert_ne!(a, b);
        assert!(b.add("sui gas", rgba(255, 255, 0, 255)).is_ok());
        assert_eq!(a, b);

        // The order of adding is not part of a table.
        assert!(a.add("sui water", rgba(0, 0, 255, 255)).is_ok());
        let mut c = ColourTable::default();
        assert!(c.add("sui water", rgba(0, 0, 255, 255)).is_ok());
        assert!(c.add("sui gas", rgba(255, 255, 0, 255)).is_ok());
        assert_eq!(a, c);

        // Another colour, or another spelling of the same name, is another
        // table: a file written from one would not be the file written from the
        // other.
        let mut recoloured = ColourTable::default();
        assert!(recoloured.add("sui gas", rgba(255, 255, 1, 255)).is_ok());
        assert_ne!(recoloured, b);
        let mut respelled = ColourTable::default();
        assert!(respelled.add("SUI Gas", rgba(255, 255, 0, 255)).is_ok());
        assert_ne!(respelled, b);
    }
}
