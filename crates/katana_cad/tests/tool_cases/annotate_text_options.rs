// The Text tool's Style, Justify and Paper options, and the Multiline Text
// tool (annotate_text.rs, docs/annotation.md "Text"), driven through
// ToolDriver as the plan view drives them. What a tool makes is checked
// against what the TEXT and MTEXT verbs make of the same request, and every
// height against paper millimetres x scale / 1000 worked by hand
// (tests/cad/tools/test_annotate_text_options.cpp).

use katana_cad::{CommandInterpreter, Document};
use katana_commands::create_text_style;
use katana_entity::{Geometry, TextGeometry, TextStyle};

use crate::ok;

fn texts(document: &Document) -> Vec<TextGeometry> {
    let mut out = Vec::new();
    document.model().entities.for_each(|entity| {
        if let Geometry::Text(text) = &entity.geometry {
            out.push(text.clone());
        }
    });
    out
}

// A text style "Notes" 3.5 mm on paper, and the drawing at 1:500.
#[track_caller]
fn notes_at_500(document: &mut Document) {
    let notes = TextStyle {
        name: String::from("Notes"),
        paper_height: 3.5,
        ..TextStyle::default()
    };
    ok(document.execute(create_text_style(notes)));
    ok(document.set_annotation_scale(500.0));
}

// What a verb line makes in a drawing set up the same way.
#[track_caller]
fn made_by_verb(line: &str) -> TextGeometry {
    let mut document = Document::default();
    notes_at_500(&mut document);
    if let Err(error) = CommandInterpreter::new().run(&mut document, line) {
        panic!("{line}: {}", error.describe());
    }
    texts(&document).pop().unwrap_or_default()
}

mod annotate_text_options {
    use katana_cad::interactive_tool::ToolInput;
    use katana_entity::{TextGeometry, TextJustify};

    use super::{made_by_verb, notes_at_500, texts};
    use crate::tool_driver::ToolDriver;
    use crate::{CONTINUE, DONE, REJECTED, contains, double_eq, pt};

    #[test]
    fn a_style_and_a_justification_make_what_the_text_verb_makes() {
        let mut driver = ToolDriver::new();
        notes_at_500(driver.document_mut());
        driver.start("annotate.text");
        assert!(
            contains(&driver.prompt(), "[Style/Justify/Paper]"),
            "{}",
            driver.prompt()
        );
        assert_eq!(driver.r#type("S").outcome, CONTINUE);
        assert_eq!(driver.expects(), ToolInput::Value);
        assert!(
            contains(&driver.prompt(), "text style"),
            "{}",
            driver.prompt()
        );
        assert_eq!(driver.r#type("Notes").outcome, CONTINUE);
        assert_eq!(driver.r#type("j").outcome, CONTINUE);
        assert_eq!(driver.r#type("MC").outcome, CONTINUE);
        assert!(
            contains(&driver.prompt(), "style Notes, MC"),
            "{}",
            driver.prompt()
        );

        // A paper-sized style: no height is asked, straight to the rotation.
        assert_eq!(driver.click(10.0, 20.0).outcome, CONTINUE);
        assert!(
            contains(&driver.prompt(), "rotation"),
            "{}",
            driver.prompt()
        );
        driver.r#type("0");
        driver.r#type("Kerb");
        assert_eq!(driver.enter().outcome, DONE);

        let made = texts(driver.document());
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].style, "Notes");
        assert_eq!(made[0].justify, TextJustify::MiddleCentre);
        assert_eq!(
            made[0].paper_height, 0.0,
            "the style's paper height, not a copy of it"
        );
        // 3.5 mm at 1:500 is 3.5 x 500 / 1000 = 1.75 m.
        double_eq(made[0].height, 1.75);
        assert_eq!(
            made[0],
            made_by_verb("TEXT 10,20 \"Kerb\" style=Notes justify=MC")
        );
    }

    #[test]
    fn a_paper_height_skips_the_height_prompt_and_is_the_verbs_paper() {
        let mut driver = ToolDriver::new();
        notes_at_500(driver.document_mut());
        driver.start("annotate.text");
        driver.r#type("P");
        assert!(
            contains(&driver.prompt(), "height on paper"),
            "{}",
            driver.prompt()
        );
        assert_eq!(driver.r#type("5").outcome, CONTINUE);
        driver.click(0.0, 0.0);
        assert!(
            contains(&driver.prompt(), "rotation"),
            "{}",
            driver.prompt()
        );
        driver.r#type("90");
        driver.r#type("North");
        driver.enter();
        let made = texts(driver.document());
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].style, "", "no style was chosen");
        assert_eq!(made[0].paper_height, 5.0);
        // 5 mm at 1:500 is 2.5 m.
        double_eq(made[0].height, 2.5);
        assert_eq!(
            made[0],
            made_by_verb("TEXT 0,0 1 \"North\" paper=5 rotation=90")
        );
    }

    #[test]
    fn no_style_and_no_paper_height_ask_the_height_as_before() {
        let mut driver = ToolDriver::new();
        notes_at_500(driver.document_mut());
        driver.start("annotate.text");
        driver.r#type("S");
        driver.r#type("Notes");
        driver.r#type("S");
        assert!(contains(&driver.prompt(), "<Notes>"), "{}", driver.prompt());
        assert_eq!(driver.r#type(".").outcome, CONTINUE, "a dot is no style");
        driver.r#type("P");
        driver.r#type("0");
        driver.click(0.0, 0.0);
        assert!(
            contains(&driver.prompt(), "Specify height"),
            "{}",
            driver.prompt()
        );
        driver.r#type("2");
        driver.r#type("0");
        driver.r#type("plain");
        driver.enter();
        let made = texts(driver.document());
        assert_eq!(made.len(), 1);
        assert_eq!(made[0], TextGeometry::new(pt(0.0, 0.0), "plain", 2.0, 0.0));
    }

    #[test]
    fn an_option_it_cannot_take_is_refused_and_the_tool_waits_where_it_was() {
        let mut driver = ToolDriver::new();
        notes_at_500(driver.document_mut());
        driver.start("annotate.text");
        driver.r#type("S");
        let style_prompt = driver.prompt();
        let missing = driver.r#type("Nope");
        assert_eq!(missing.outcome, REJECTED);
        assert!(
            contains(&missing.message, "no text style 'Nope'"),
            "{}",
            missing.message
        );
        assert_eq!(driver.prompt(), style_prompt);
        assert_eq!(
            driver.enter().outcome,
            CONTINUE,
            "Enter keeps the style shown"
        );
        driver.r#type("J");
        assert_eq!(driver.r#type("XX").outcome, REJECTED);
        assert_eq!(driver.undo().outcome, CONTINUE, "back to the start point");
        driver.r#type("P");
        assert_eq!(driver.r#type("-1").outcome, REJECTED);
        assert_eq!(driver.r#type("tall").outcome, REJECTED);
        driver.enter();
        assert!(
            contains(&driver.prompt(), "start point"),
            "{}",
            driver.prompt()
        );
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn undo_from_a_paper_sized_texts_rotation_goes_back_to_its_start_point() {
        let mut driver = ToolDriver::new();
        notes_at_500(driver.document_mut());
        driver.start("annotate.text");
        driver.r#type("S");
        driver.r#type("Notes");
        driver.click(1.0, 1.0);
        assert_eq!(driver.undo().outcome, CONTINUE);
        assert!(
            contains(&driver.prompt(), "start point"),
            "it never asked for a height: {}",
            driver.prompt()
        );
    }

    #[test]
    fn the_next_text_is_offered_the_choices_the_last_was_made_with() {
        // As the height is (annotate_text.rs): read from the newest text, so
        // the tool that starts again after a text offers what that text has.
        let mut driver = ToolDriver::new();
        notes_at_500(driver.document_mut());
        driver.start("annotate.text");
        driver.r#type("S");
        driver.r#type("Notes");
        driver.r#type("J");
        driver.r#type("TR");
        driver.click(0.0, 0.0);
        driver.enter();
        driver.r#type("first");
        assert!(driver.enter().restart);
        assert!(
            contains(&driver.prompt(), "style Notes, TR"),
            "{}",
            driver.prompt()
        );
        // Enter continues below it in the same style: 1.75 x 5 / 3 below.
        driver.enter();
        driver.r#type("second");
        driver.enter();
        let made = texts(driver.document());
        assert_eq!(made.len(), 2);
        assert_eq!(made[1].style, "Notes");
        assert_eq!(made[1].justify, TextJustify::TopRight);
        double_eq(made[1].position.y, -1.75 * 5.0 / 3.0);
    }
}

mod annotate_multiline_text {
    use katana_cad::interactive_tool::tool_catalog;
    use katana_commands::{EntityAttributes, create_text};
    use katana_entity::{Geometry, TextGeometry, TextJustify};

    use super::{made_by_verb, notes_at_500, texts};
    use crate::tool_driver::ToolDriver;
    use crate::{CONTINUE, DONE, contains, double_eq, ok, pt};

    #[test]
    fn every_line_is_one_entity_as_mtext_makes_it() {
        let mut driver = ToolDriver::new();
        notes_at_500(driver.document_mut());
        driver.start("annotate.mtext");
        assert!(
            contains(&driver.prompt(), "insertion point"),
            "{}",
            driver.prompt()
        );
        assert!(
            contains(&driver.prompt(), "style Standard"),
            "paper-sized, as MTEXT is"
        );
        assert_eq!(driver.click(5.0, 5.0).outcome, CONTINUE);
        assert!(
            contains(&driver.prompt(), "rotation"),
            "{}",
            driver.prompt()
        );
        driver.enter();
        driver.r#type("PIT 12");
        driver.r#type("IL 10.50");
        assert_eq!(driver.executed(), 0);
        let done = driver.enter();
        assert_eq!(done.outcome, DONE);
        assert_eq!(done.message, "a text of 2 lines");
        assert_eq!(driver.executed(), 1, "one step");

        let made = texts(driver.document());
        assert_eq!(made.len(), 1, "one entity");
        assert_eq!(made[0].text, "PIT 12\nIL 10.50");
        assert_eq!(made[0].style, "Standard");
        // Standard is 2.5 mm on paper (annotation.rs): 2.5 x 500 / 1000 = 1.25.
        double_eq(made[0].height, 1.25);
        assert_eq!(made[0], made_by_verb("MTEXT 5,5 \"PIT 12\\nIL 10.50\""));

        ok(driver.document_mut().undo());
        assert!(texts(driver.document()).is_empty());
    }

    #[test]
    fn it_takes_the_options_and_previews_the_block_as_one_text() {
        let mut driver = ToolDriver::new();
        notes_at_500(driver.document_mut());
        driver.start("annotate.mtext");
        driver.r#type("S");
        driver.r#type("Notes");
        driver.r#type("J");
        driver.r#type("TL");
        driver.click(0.0, 10.0);
        driver.r#type("30");
        driver.r#type("one");
        driver.r#type("two");
        let preview = driver.preview(50.0, 50.0);
        assert_eq!(preview.shapes.len(), 1);
        let Geometry::Text(block) = &preview.shapes[0] else {
            panic!("not a text: {:?}", preview.shapes[0]);
        };
        assert_eq!(block.text, "one\ntwo");
        assert_eq!(block.justify, TextJustify::TopLeft);
        driver.enter();
        assert_eq!(
            texts(driver.document()).last(),
            Some(&made_by_verb(
                "MTEXT 0,10 \"one\\ntwo\" style=Notes justify=TL rotation=30"
            ))
        );
    }

    #[test]
    fn enter_at_its_first_prompt_ends_the_tool_and_its_aliases_start_it() {
        let mut driver = ToolDriver::new();
        // Even with a text in the drawing: Multiline Text does not continue one.
        driver.add(create_text(
            TextGeometry::new(pt(0.0, 0.0), "above", 3.0, 0.0),
            EntityAttributes::default(),
        ));
        driver.start("annotate.mtext");
        let step = driver.enter();
        assert_eq!(step.outcome, DONE);
        assert!(step.command.is_none());

        let catalog = tool_catalog();
        for alias in ["MTEXT", "MT"] {
            let Some(info) = catalog.find_by_alias(alias) else {
                panic!("{alias}");
            };
            assert_eq!(info.id, "annotate.mtext");
            assert_eq!(info.group, "Text");
        }
    }
}
