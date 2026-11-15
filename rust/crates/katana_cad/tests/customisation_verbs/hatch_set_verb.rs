//! HATCH SET (the Hatch Patterns tab's Save) and STYLE NEW name field value (its New Style Using
//! This) through CommandInterpreter::run, as the window's command line, katana_cli and katana_mcp
//! type them; and the rows the tab shows (hatch_pattern_rows)
//! (tests/cad/customisation/test_hatch_set_verb.cpp).
//!
//! Degrees are what is typed and radians what is kept: 45 degrees is pi / 4. A pattern's offsets
//! cannot be typed, so SET keeps them by position; one is made here through the command HATCH NEW
//! itself runs, commands' create_hatch_pattern.

use katana_cad::hatch_pattern_rows::{
    free_hatch_pattern_name, hatch_families_text, hatch_pattern_kind, hatch_pattern_rows,
};
use katana_cad::{CommandInterpreter, Document};
use katana_commands::create_hatch_pattern;
use katana_core::ErrorCode;
use katana_entity::{HatchLineFamily, HatchPattern, Style};
use katana_math::PI;

use crate::{must, near};

#[derive(Default)]
struct Session {
    document: Document,
    interpreter: CommandInterpreter,
}

impl Session {
    #[track_caller]
    fn ok(&mut self, line: &str) -> String {
        self.interpreter
            .run(&mut self.document, line)
            .unwrap_or_else(|error| panic!("{line} -> {}", error.describe()))
    }

    #[track_caller]
    fn fails(&mut self, line: &str) -> ErrorCode {
        match self.interpreter.run(&mut self.document, line) {
            Ok(reply) => panic!("{line} unexpectedly succeeded: {reply}"),
            Err(error) => error.code,
        }
    }

    #[track_caller]
    fn pattern(&self, name: &str) -> HatchPattern {
        self.document
            .model()
            .hatch_patterns
            .find(name)
            .unwrap_or_else(|| panic!("no hatch pattern {name}"))
            .clone()
    }

    fn undo_count(&self) -> usize {
        self.document.history().undo_count()
    }
}

mod hatch_set {
    use super::*;

    #[test]
    fn set_replaces_the_families_in_degrees_as_one_undo_step() {
        let mut s = Session::default();
        s.ok("HATCH NEW brick 45 0.25");
        let before = s.undo_count();
        assert_eq!(
            s.ok("HATCH SET brick 45 0.5 135 0.5"),
            "hatch pattern brick updated (2 families)"
        );
        assert_eq!(s.undo_count(), before + 1);
        let brick = s.pattern("brick");
        assert_eq!(brick.families.len(), 2);
        near(brick.families[0].angle, PI / 4.0, 1e-15);
        assert_eq!(brick.families[0].spacing, 0.5);
        near(brick.families[1].angle, 3.0 * PI / 4.0, 1e-15);
        assert!(!brick.solid);

        s.ok("UNDO");
        assert_eq!(s.pattern("brick").families.len(), 1);
        assert_eq!(s.pattern("brick").families[0].spacing, 0.25);
    }

    #[test]
    fn set_keeps_each_familys_offset_by_position_and_the_description() {
        let mut s = Session::default();
        let imported = HatchPattern {
            name: String::from("paving"),
            description: String::from("Pavers, from the council's file"),
            families: vec![
                HatchLineFamily {
                    angle: 0.0,
                    spacing: 1.0,
                    offset: 0.3,
                },
                HatchLineFamily {
                    angle: PI / 2.0,
                    spacing: 1.0,
                    offset: 0.7,
                },
            ],
            ..HatchPattern::default()
        };
        must(s.document.execute(create_hatch_pattern(imported)));
        s.ok("HATCH SET paving 0 2 90 2 45 3");
        let paving = s.pattern("paving");
        assert_eq!(paving.families.len(), 3);
        assert_eq!(paving.families[0].offset, 0.3);
        assert_eq!(paving.families[1].offset, 0.7);
        assert_eq!(paving.families[2].offset, 0.0); // a family added has none
        assert_eq!(paving.families[0].spacing, 2.0);
        assert_eq!(paving.description, "Pavers, from the council's file");
    }

    #[test]
    fn set_turns_a_pattern_solid_and_back_without_keeping_words_that_are_no_longer_true() {
        let mut s = Session::default();
        s.ok("HATCH NEW brick 45 0.25");
        assert_eq!(
            s.ok("HATCH SET brick solid"),
            "hatch pattern brick updated (solid)"
        );
        assert!(s.pattern("brick").solid);
        assert!(s.pattern("brick").families.is_empty());
        assert_eq!(s.pattern("brick").description, "Solid fill"); // as HATCH SOLID calls one

        s.ok("HATCH SOLID fill");
        assert_eq!(s.pattern("fill").description, "Solid fill");
        s.ok("HATCH SET fill 30 1");
        assert!(!s.pattern("fill").solid);
        assert!(s.pattern("fill").description.is_empty());
    }

    #[test]
    fn setting_what_is_already_there_is_no_undo_step() {
        let mut s = Session::default();
        s.ok("HATCH NEW brick 45 0.25 135 0.25");
        let before = s.undo_count();
        assert_eq!(
            s.ok("HATCH SET brick 45 0.25 135 0.25"),
            "hatch pattern brick unchanged"
        );
        assert_eq!(s.undo_count(), before);
    }

    #[test]
    fn set_refuses_what_cannot_be_a_pattern_and_keeps_the_one_there_was() {
        let mut s = Session::default();
        s.ok("HATCH NEW brick 45 0.25");
        assert_eq!(s.fails("HATCH SET nosuch 45 1"), ErrorCode::NotFound);
        assert_eq!(s.fails("HATCH SET brick"), ErrorCode::InvalidArgument);
        assert_eq!(s.fails("HATCH SET brick 45"), ErrorCode::InvalidArgument); // no spacing
        assert_eq!(
            s.fails("HATCH SET brick 45 1 90"),
            ErrorCode::InvalidArgument
        ); // half a family
        // SOLID with a number after it is read as families, and SOLID is no angle.
        assert_eq!(s.fails("HATCH SET brick SOLID 45"), ErrorCode::ParseFailure);
        assert_eq!(s.fails("HATCH SET brick 45 -1"), ErrorCode::InvalidArgument); // model refuses
        assert_eq!(s.fails("HATCH SET brick 45 x"), ErrorCode::ParseFailure);
        // "none" is what every unhatched layer draws; it cannot be given a fill,
        // which the command refuses as CommandRejected (the hatch pattern
        // table's policy, which keeps the built-in entry).
        assert_eq!(s.fails("HATCH SET none 45 1"), ErrorCode::CommandRejected);
        assert_eq!(s.pattern("brick").families.len(), 1);
        assert_eq!(s.pattern("brick").families[0].spacing, 0.25);
        assert!(s.pattern("none").draws_nothing());
    }

    #[test]
    fn style_new_with_a_field_is_a_style_made_with_it_set_in_one_step() {
        let mut s = Session::default();
        s.ok("HATCH NEW brick 45 0.25");
        let before = s.undo_count();
        assert_eq!(
            s.ok("STYLE NEW \"brick paving\" HATCH brick"),
            "style brick paving created"
        );
        assert_eq!(s.undo_count(), before + 1);
        let made = s
            .document
            .model()
            .styles
            .find("brick paving")
            .expect("the style brick paving");
        assert_eq!(made.hatch_pattern, "brick");
        // SET's own reading of a field: a pattern that does not exist is
        // refused, and nothing is made.
        assert_eq!(s.fails("STYLE NEW ghost HATCH nosuch"), ErrorCode::NotFound);
        assert!(s.document.model().styles.find("ghost").is_none());
        s.ok("STYLE NEW kerb COLOUR #FF0000");
        let kerb = s
            .document
            .model()
            .styles
            .find("kerb")
            .expect("the style kerb");
        assert!(kerb.color.is_some());
        // A second word that is no field is still a name that wanted quotes.
        assert_eq!(
            s.fails("STYLE NEW TOPO Natural Surface"),
            ErrorCode::InvalidArgument
        );
        assert!(s.document.model().styles.find("TOPO").is_none());
        assert_eq!(
            s.fails("STYLE NEW lonely HATCH"),
            ErrorCode::InvalidArgument
        ); // no value
    }

    // A field whose value is one word takes one word: a second field after it
    // was once dropped, the style made and nothing said. NEW and SET alike.
    #[test]
    fn a_word_after_a_one_word_fields_value_is_refused_rather_than_dropped() {
        let mut s = Session::default();
        let before = s.undo_count();
        assert_eq!(
            s.fails("STYLE NEW a colour #FF0000 weight 0.5"),
            ErrorCode::InvalidArgument
        );
        assert!(s.document.model().styles.find("a").is_none());
        assert_eq!(s.undo_count(), before);
        for line in [
            "STYLE NEW b weight 0.5 colour #FF0000",
            "STYLE NEW c hatch - x",
            "STYLE NEW d symbolsize 1.5 2",
        ] {
            assert_eq!(s.fails(line), ErrorCode::InvalidArgument, "{line}");
        }
        s.ok("STYLE NEW e colour #FF0000");
        assert_eq!(
            s.fails("STYLE SET e weight 0.5 colour #00FF00"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            s.document
                .model()
                .styles
                .find("e")
                .expect("the style e")
                .line_weight,
            Style::default().line_weight
        );
        // A name takes the rest of the line, as it always has.
        s.ok("STYLE SET e description kerb and channel");
        assert_eq!(
            s.document
                .model()
                .styles
                .find("e")
                .expect("the style e")
                .description,
            "kerb and channel"
        );
    }
}

mod hatch_pattern_rows {
    use super::*;

    #[test]
    fn every_pattern_is_a_row_with_its_kind_and_what_uses_it() {
        let mut s = Session::default();
        s.ok("HATCH NEW brick 45 0.25 135 0.25");
        s.ok("HATCH SOLID fill");
        s.ok("LAYER NEW site");
        s.ok("LAYER HATCH site brick");
        let rows = hatch_pattern_rows(s.document.model());
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].name, "brick");
        assert_eq!(hatch_pattern_kind(&rows[0]), "2 families");
        assert_eq!(rows[0].users.layers, ["site"]);
        assert!(!rows[0].built_in);
        assert_eq!(hatch_pattern_kind(&rows[1]), "solid");
        assert!(!rows[1].users.used());
        assert_eq!(rows[2].name, "none");
        assert_eq!(hatch_pattern_kind(&rows[2]), "draws nothing");
        assert!(rows[2].built_in);
    }

    #[test]
    fn the_families_read_back_as_the_degrees_typed() {
        let mut s = Session::default();
        // 30 degrees is kept as 0.5235987755982988 radians, which multiplies
        // back to 29.999999999999996; the text is the 30 that was typed.
        s.ok("HATCH NEW slope 30 0.125 120 0.5");
        assert_eq!(hatch_families_text(&s.pattern("slope")), "30 0.125 120 0.5");
        // Retyped, the text makes the same pattern.
        let line = format!(
            "HATCH NEW copy {}",
            hatch_families_text(&s.pattern("slope"))
        );
        s.ok(&line);
        assert_eq!(s.pattern("copy").families, s.pattern("slope").families);
        s.ok("HATCH SOLID fill");
        assert_eq!(hatch_families_text(&s.pattern("fill")), "");
    }

    #[test]
    fn a_free_name_counts_on_past_the_ones_taken() {
        let mut s = Session::default();
        assert_eq!(
            free_hatch_pattern_name(s.document.model(), "brick"),
            "brick"
        );
        s.ok("HATCH NEW brick 45 0.25");
        s.ok("HATCH NEW \"brick 2\" 45 0.5");
        assert_eq!(
            free_hatch_pattern_name(s.document.model(), "brick"),
            "brick 3"
        );
    }
}
