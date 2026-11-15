// Plot styles and the page setup (plot.hpp's colour modes, page_setup.hpp):
// what a colour prints as in each mode, which sheets a selection names, what
// the files of a plot are called, and the page setup's JSON and its one
// undoable step; tests/cad/plotting/test_page_setup.cpp.

use katana_cad::Document;
use katana_cad::plot::{luminance, paper_colour, paper_fill_colour, plot_colour_mode_from};
use katana_cad::plotting::page_setup::{
    DEFAULT_FILE_NAME_PATTERN, format_sheet_selection, parse_sheet_selection, plot_settings_for,
    sanitise_file_name, set_page_setup, sheet_file_name, sheet_file_names,
    validate_file_name_pattern, validate_page_setup,
};
use katana_cad::plotting::sheet_commands::add_sheet;
use katana_cad::plotting::sheet_json::{sheet_set_from_json, sheet_set_to_json};
use katana_cad::plotting::{PageSetup, Sheet, SheetSet};
use katana_cad::{PlotColourMode, PlotSettings};
use katana_core::ErrorCode;
use katana_entity::{Color, EnumName};

fn in_mode(mode: PlotColourMode) -> PlotSettings {
    PlotSettings {
        colour_mode: mode,
        ..PlotSettings::default()
    }
}

// A set of `count` sheets named "SHEET 1"..., with ids s1, s2 ...
fn set_of(count: usize) -> SheetSet {
    let mut set = SheetSet::default();
    for i in 0..count {
        set.sheets.push(Sheet {
            id: format!("s{}", i + 1),
            name: format!("SHEET {}", i + 1),
            ..Sheet::default()
        });
    }
    set
}

fn selected(text: &str, set: &SheetSet) -> Vec<usize> {
    parse_sheet_selection(text, set).unwrap_or_else(|error| panic!("{text}: {}", error.describe()))
}

fn json_of(set: &SheetSet) -> String {
    sheet_set_to_json(set).unwrap_or_else(|error| panic!("{}", error.describe()))
}

const fn colour(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::new(r, g, b, a)
}

// ---- colour modes ------------------------------------------------------------------------

mod plot_style {
    use super::*;

    #[test]
    fn luminance_is_the_rec601_luma_in_integers_and_a_grey_is_its_own() {
        assert_eq!(luminance(colour(255, 0, 0, 255)), 76); // 76.245
        assert_eq!(luminance(colour(0, 255, 0, 255)), 150); // 149.685
        assert_eq!(luminance(colour(0, 0, 255, 255)), 29); // 29.07
        assert_eq!(luminance(colour(255, 255, 255, 255)), 255);
        assert_eq!(luminance(colour(0, 0, 0, 255)), 0);
        for level in 0..=255_u8 {
            assert_eq!(
                luminance(colour(level, level, level, 255)),
                level,
                "{level}"
            );
        }
        // Alpha is not lightness.
        assert_eq!(luminance(colour(255, 0, 0, 10)), 76);
    }

    #[test]
    fn colour_keeps_every_colour_and_the_white_rule_as_it_was() {
        let settings = in_mode(PlotColourMode::Colour);
        assert_eq!(
            paper_colour(colour(255, 0, 0, 255), &settings),
            colour(255, 0, 0, 255)
        );
        assert_eq!(
            paper_colour(colour(255, 255, 255, 200), &settings),
            colour(0, 0, 0, 200)
        );
        assert_eq!(
            paper_fill_colour(colour(0, 90, 200, 40), &settings),
            colour(0, 90, 200, 40)
        );
        // The default plot is in colour, at the line weights drawn.
        assert_eq!(PlotSettings::default().colour_mode, PlotColourMode::Colour);
        assert_eq!(PlotSettings::default().line_weight_scale, 1.0);
    }

    #[test]
    fn greyscale_prints_each_colour_as_the_grey_of_its_luminance_and_keeps_alpha() {
        let grey = in_mode(PlotColourMode::Greyscale);
        assert_eq!(
            paper_colour(colour(255, 0, 0, 255), &grey),
            colour(76, 76, 76, 255)
        );
        assert_eq!(
            paper_colour(colour(0, 125, 50, 128), &grey),
            colour(79, 79, 79, 128)
        );
        // The white rule comes first: white prints black, not white.
        assert_eq!(
            paper_colour(colour(255, 255, 255, 255), &grey),
            colour(0, 0, 0, 255)
        );
        // A light colour that is not white stays light.
        assert_eq!(
            paper_colour(colour(255, 255, 224, 255), &grey),
            colour(251, 251, 251, 255)
        );
        // A fill keeps its lightness as a grey, as a pen does.
        assert_eq!(
            paper_fill_colour(colour(0, 90, 200, 40), &grey),
            colour(76, 76, 76, 40)
        );
        // Nothing printed in greyscale has any colour in it.
        for r in (0..=255_u8).step_by(15) {
            for g in (0..=255_u8).step_by(15) {
                for b in (0..=255_u8).step_by(15) {
                    let c = paper_colour(colour(r, g, b, 255), &grey);
                    assert!(c.r == c.g && c.g == c.b, "{r},{g},{b}");
                }
            }
        }
    }

    #[test]
    fn monochrome_prints_every_pen_black_and_a_fill_black_or_white_by_its_lightness() {
        let mono = in_mode(PlotColourMode::Monochrome);
        // Every pen, however light, is black; its alpha is its own.
        assert_eq!(
            paper_colour(colour(255, 255, 0, 255), &mono),
            colour(0, 0, 0, 255)
        );
        assert_eq!(
            paper_colour(colour(190, 190, 190, 255), &mono),
            colour(0, 0, 0, 255)
        );
        assert_eq!(
            paper_colour(colour(0, 90, 200, 99), &mono),
            colour(0, 0, 0, 99)
        );
        // A fill in the darker half prints black, in the lighter half drops to
        // the paper's white; the threshold is a luminance of 128.
        assert_eq!(
            paper_fill_colour(colour(127, 127, 127, 255), &mono),
            colour(0, 0, 0, 255)
        );
        assert_eq!(
            paper_fill_colour(colour(128, 128, 128, 255), &mono),
            colour(255, 255, 255, 255)
        );
        assert_eq!(
            paper_fill_colour(colour(200, 0, 30, 18), &mono),
            colour(0, 0, 0, 18)
        ); // 63
        assert_eq!(
            paper_fill_colour(colour(255, 255, 0, 90), &mono),
            colour(255, 255, 255, 90)
        ); // 226
        // A white fill takes the white rule first, as a white pen does.
        assert_eq!(
            paper_fill_colour(colour(255, 255, 255, 255), &mono),
            colour(0, 0, 0, 255)
        );
        let keep_white = PlotSettings {
            white_to_black: false,
            ..mono
        };
        assert_eq!(
            paper_fill_colour(colour(255, 255, 255, 255), &keep_white),
            colour(255, 255, 255, 255)
        );
    }

    #[test]
    fn colour_modes_are_named_and_read_with_their_common_spellings() {
        for mode in [
            PlotColourMode::Colour,
            PlotColourMode::Greyscale,
            PlotColourMode::Monochrome,
        ] {
            assert_eq!(plot_colour_mode_from(mode.name()), Some(mode));
        }
        assert_eq!(PlotColourMode::Greyscale.name(), "greyscale");
        assert_eq!(plot_colour_mode_from("COLOR"), Some(PlotColourMode::Colour));
        assert_eq!(
            plot_colour_mode_from("grey"),
            Some(PlotColourMode::Greyscale)
        );
        assert_eq!(
            plot_colour_mode_from("Gray"),
            Some(PlotColourMode::Greyscale)
        );
        assert_eq!(
            plot_colour_mode_from("grayscale"),
            Some(PlotColourMode::Greyscale)
        );
        assert_eq!(
            plot_colour_mode_from("mono"),
            Some(PlotColourMode::Monochrome)
        );
        assert!(plot_colour_mode_from("sepia").is_none());
        assert!(plot_colour_mode_from("").is_none());
    }
}

// ---- selections ----------------------------------------------------------------------------

mod sheet_selection {
    use super::*;

    #[test]
    fn positions_and_ranges_name_sheets_in_the_order_given_each_once() {
        let set = set_of(6);
        assert_eq!(selected("1,3-5", &set), vec![0, 2, 3, 4]);
        assert_eq!(selected(" 4 , 2 ", &set), vec![3, 1]);
        assert_eq!(selected("2-3,1-4", &set), vec![1, 2, 0, 3]);
        assert_eq!(selected("6", &set), vec![5]);
        assert_eq!(selected("3-3", &set), vec![2]);
        // A trailing or doubled comma is only punctuation.
        assert_eq!(selected("1,,2,", &set), vec![0, 1]);
    }

    #[test]
    fn nothing_or_all_is_every_sheet_and_an_id_names_its_sheet_wherever_it_is() {
        let mut set = set_of(3);
        assert_eq!(selected("", &set), vec![0, 1, 2]);
        assert_eq!(selected("all", &set), vec![0, 1, 2]);
        assert_eq!(selected(" ALL ", &set), vec![0, 1, 2]);
        // Ids follow a sheet that has moved.
        set.sheets.swap(0, 2);
        assert_eq!(selected("s1", &set), vec![2]);
        assert_eq!(selected("s3,2", &set), vec![0, 1]);
    }

    #[test]
    fn a_sheet_that_is_not_there_or_a_backward_range_is_refused_saying_which() {
        let set = set_of(5);
        let why = |text: &str| {
            let error = parse_sheet_selection(text, &set).expect_err("not a selection");
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{text}");
            error.message
        };
        assert_eq!(why("7"), "there is no sheet 7: the set has 5 sheets");
        assert_eq!(why("0"), "there is no sheet 0: sheets are numbered from 1");
        assert_eq!(why("2-9"), "there is no sheet 9: the set has 5 sheets");
        assert_eq!(why("5-3"), "the range 5-3 runs backwards: write 3-5");
        assert_eq!(
            why("s9"),
            "\"s9\" is neither a sheet number nor a sheet's id"
        );
        assert_eq!(
            why("1-"),
            "\"1-\" is neither a sheet number nor a sheet's id"
        );
        assert_eq!(
            why("99999999999999999999999"),
            "there is no sheet 99999999999999999999999: the set has 5 sheets"
        );
        assert_eq!(why(","), "the selection names no sheet");
        // A space inside a part is not a separator, and never joins two
        // numbers into another sheet ("1 3" is not sheet 13).
        assert_eq!(
            why("1 3"),
            "\"1 3\" is neither a sheet number nor a sheet's id"
        );
        assert_eq!(
            why("1 - 2 3"),
            "\"1-2 3\" is neither a sheet number nor a sheet's id"
        );
        assert_eq!(selected(" 1 - 3 ,\t5 ", &set), vec![0, 1, 2, 4]);
        assert_eq!(
            parse_sheet_selection("", &SheetSet::default())
                .expect_err("no sheets")
                .message,
            "there are no sheets to plot"
        );
    }

    #[test]
    fn a_selection_is_written_short_and_reads_back_the_same() {
        let some = [0, 2, 3, 4, 7, 8];
        assert_eq!(format_sheet_selection(&some), "1,3-5,8,9");
        assert_eq!(format_sheet_selection(&[4, 3, 2]), "5,4,3");
        assert_eq!(format_sheet_selection(&[]), "");
        let set = set_of(10);
        assert_eq!(
            selected(&format_sheet_selection(&some), &set),
            some.to_vec()
        );
    }
}

// ---- file names ------------------------------------------------------------------------------

mod plot_file_names {
    use super::*;

    #[test]
    fn a_name_is_made_safe_for_every_file_system() {
        assert_eq!(
            sanitise_file_name("PLAN: CH 0/100 <A>?"),
            "PLAN- CH 0-100 -A--"
        );
        assert_eq!(sanitise_file_name("  ROAD   PLAN  "), "ROAD PLAN");
        assert_eq!(sanitise_file_name("SHEET 1..."), "SHEET 1");
        assert_eq!(sanitise_file_name("A\tB\nC"), "A-B-C");
        assert_eq!(sanitise_file_name("con"), "_con");
        assert_eq!(sanitise_file_name("LPT1.pdf"), "_LPT1.pdf");
        assert_eq!(sanitise_file_name("COM10"), "COM10"); // not a device
        assert_eq!(sanitise_file_name(" .. "), "sheet");
        assert_eq!(sanitise_file_name(""), "sheet");
        // Non-ASCII text is kept, and a long name is cut between characters,
        // never inside one: 60 two-byte characters are 120 bytes, so a 61st
        // would not fit whole.
        let greek = "\u{3b1}".repeat(70); // alpha, CE B1
        let cut = sanitise_file_name(&greek);
        assert_eq!(cut.len(), 120);
        assert_eq!(&cut.as_bytes()[..2], b"\xCE\xB1");
        let mixed = format!("X{greek}"); // the cut now falls inside a character
        let cut_mixed = sanitise_file_name(&mixed);
        assert_eq!(cut_mixed.len(), 119);
        assert_eq!(cut_mixed.as_bytes()[cut_mixed.len() - 1], 0xB1);
    }

    #[test]
    fn the_pattern_uses_the_sheet_number_tokens_and_the_sheets_name_and_id() {
        let mut set = set_of(12);
        set.sheets[2].name = String::from("PLAN TILE 3");
        set.defaults.set_number = String::from("C");
        set.numbering = String::from("{set}-{n:03}");
        assert_eq!(
            sheet_file_name(&set, 2, DEFAULT_FILE_NAME_PATTERN),
            "C03 PLAN TILE 3"
        );
        assert_eq!(
            sheet_file_name(&set, 2, "{number} {name}"),
            "C-003 PLAN TILE 3"
        );
        assert_eq!(sheet_file_name(&set, 2, "{id}_{n}_of_{N}"), "s3_3_of_12");
        assert_eq!(sheet_file_name(&set, 11, "{n:02}"), "12");
        assert_eq!(sheet_file_name(&set, 12, "{n}"), ""); // past the end
        // Without a set number the default pattern starts at the number.
        set.defaults.set_number.clear();
        assert_eq!(
            sheet_file_name(&set, 0, DEFAULT_FILE_NAME_PATTERN),
            "01 SHEET 1"
        );
        // A name with a separator in it stays one file name.
        set.sheets[0].name = String::from("ROAD CH 0/250");
        assert_eq!(sheet_file_name(&set, 0, "{name}"), "ROAD CH 0-250");
    }

    #[test]
    fn two_sheets_that_would_share_a_name_are_told_apart_in_order() {
        let mut set = set_of(4);
        set.sheets[0].name = String::from("PLAN");
        set.sheets[1].name = String::from("plan");
        set.sheets[2].name = String::from("PLAN (2)");
        set.sheets[3].name = String::from("PLAN");
        let all = [0, 1, 2, 3];
        // Names that differ only in case are one file on Windows.
        assert_eq!(
            sheet_file_names(&set, &all, "{name}"),
            vec!["PLAN", "plan (2)", "PLAN (2) (2)", "PLAN (3)"]
        );
        // A pattern with no per-sheet token still gives every sheet its file.
        assert_eq!(
            sheet_file_names(&set, &[0, 1], "SET"),
            vec!["SET", "SET (2)"]
        );
    }

    #[test]
    fn a_pattern_with_an_unknown_token_or_an_open_brace_is_refused() {
        assert!(validate_file_name_pattern(DEFAULT_FILE_NAME_PATTERN).is_ok());
        assert!(validate_file_name_pattern("{number} - {name} ({id}) {N}").is_ok());
        assert!(validate_file_name_pattern("plan}").is_ok()); // a closing brace is only text
        assert_eq!(
            validate_file_name_pattern("{sheet}")
                .expect_err("no such token")
                .message,
            "the file-name pattern has no token {sheet}: use {n}, {n:02}, {N}, {set}, {name}, \
             {number} or {id}"
        );
        assert!(validate_file_name_pattern("{n:xx}").is_err());
        assert_eq!(
            validate_file_name_pattern("{name")
                .expect_err("a brace left open")
                .message,
            "a brace is left open in the file-name pattern \"{name\""
        );
        assert_eq!(
            validate_file_name_pattern("   ")
                .expect_err("an empty pattern")
                .message,
            "the file-name pattern is empty"
        );
    }
}

// ---- the page setup ----------------------------------------------------------------------------

mod page_setup {
    use super::*;

    #[test]
    fn a_resolution_or_line_weight_scale_out_of_range_is_refused() {
        let mut setup = PageSetup::default();
        assert!(validate_page_setup(&setup).is_ok());
        setup.dpi = 49.0;
        assert_eq!(
            validate_page_setup(&setup).expect_err("too coarse").message,
            "the resolution must be 50 to 1200 dpi"
        );
        setup.dpi = f64::NAN;
        assert!(validate_page_setup(&setup).is_err());
        setup.dpi = 1200.0;
        assert!(validate_page_setup(&setup).is_ok());
        setup.line_weight_scale = 0.0;
        assert_eq!(
            validate_page_setup(&setup)
                .expect_err("no line weight")
                .message,
            "the line weight scale must be 0.1 to 5"
        );
        setup.line_weight_scale = 5.0;
        assert!(validate_page_setup(&setup).is_ok());
        setup.file_name_pattern = String::from("{nope}");
        assert!(validate_page_setup(&setup).is_err());
    }

    #[test]
    fn the_plot_settings_carry_its_style_and_resolution() {
        let setup = PageSetup {
            colour_mode: PlotColourMode::Monochrome,
            line_weight_scale: 1.4,
            dpi: 600.0,
            ..PageSetup::default()
        };
        let settings = plot_settings_for(&setup);
        assert_eq!(settings.colour_mode, PlotColourMode::Monochrome);
        assert_eq!(settings.line_weight_scale, 1.4);
        assert_eq!(settings.dpi, 600.0);
        assert!(settings.white_to_black);
    }

    #[test]
    fn it_round_trips_through_json_and_is_left_out_at_its_defaults() {
        let mut set = set_of(1);
        let plain = json_of(&set);
        assert!(!plain.contains("page_setup"));

        set.page_setup.colour_mode = PlotColourMode::Greyscale;
        set.page_setup.line_weight_scale = 0.1 + 0.2; // written in full
        set.page_setup.dpi = 450.0;
        set.page_setup.file_name_pattern = String::from("{number} {name}");
        set.page_setup.file_per_sheet = true;
        let json = json_of(&set);
        assert!(
            json.contains(
                r#""page_setup":{"colour_mode":"greyscale","dpi":450.0,"file_name_pattern":"{number} {name}","file_per_sheet":true,"line_weight_scale":0.30000000000000004}"#
            ),
            "{json}"
        );
        let back =
            sheet_set_from_json(&json).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(back == set);
        assert_eq!(json_of(&back), json);

        // Only what differs is written.
        let mut one = set_of(1);
        one.page_setup.colour_mode = PlotColourMode::Monochrome;
        assert!(json_of(&one).contains(r#""page_setup":{"colour_mode":"monochrome"}"#));
    }

    #[test]
    fn an_unknown_colour_mode_is_a_parse_failure() {
        let set = sheet_set_from_json(
            r#"{"format":"katana-sheets","version":1,"page_setup":{"colour_mode":"sepia"}}"#,
        );
        assert_eq!(
            set.expect_err("no such colour mode").code,
            ErrorCode::ParseFailure
        );
        let read = sheet_set_from_json(
            r#"{"format":"katana-sheets","version":1,"page_setup":{"colour_mode":"monochrome","dpi":150}}"#,
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read.page_setup.colour_mode, PlotColourMode::Monochrome);
        assert_eq!(read.page_setup.dpi, 150.0);
        assert_eq!(read.page_setup.file_name_pattern, DEFAULT_FILE_NAME_PATTERN);
    }

    #[test]
    fn setting_it_is_one_undoable_step_and_no_step_when_nothing_changes() {
        let mut document = Document::default();
        add_sheet(&mut document, set_of(1).sheets.remove(0), None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let setup = PageSetup {
            colour_mode: PlotColourMode::Greyscale,
            dpi: 150.0,
            ..PageSetup::default()
        };
        set_page_setup(&mut document, &setup)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(document.sheet_set().page_setup, setup);
        assert_eq!(document.history().undo_name(), "PAGE_SETUP");
        // The same again records nothing.
        set_page_setup(&mut document, &setup)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        document
            .undo()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(document.sheet_set().page_setup, PageSetup::default());
        assert_eq!(document.sheet_set().sheets.len(), 1); // the sheet's step is still there
        document
            .redo()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(document.sheet_set().page_setup, setup);

        // Refused setups change nothing.
        let wrong = PageSetup {
            dpi: 5.0,
            ..setup.clone()
        };
        assert_eq!(
            set_page_setup(&mut document, &wrong)
                .expect_err("too coarse")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(document.sheet_set().page_setup, setup);
    }
}
