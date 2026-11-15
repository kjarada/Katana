// The Annotate family: Text, Linear and Aligned Dimension, and Leader, driven
// through ToolDriver as the plan view drives them
// (tests/cad/tools/test_annotate.cpp). Every expected value is worked out by
// hand in the comment beside it; inputs are chosen exact in binary wherever
// the geometry allows, and where it does not (a 3-4-5 direction, an angle
// through pi) the tolerance says why.
//
// The C++ also types bytes that are not UTF-8 at the text prompts ("\xff\xfe"
// at Text's, "\xc3" at Leader's, "\xff" at the aligned dimension's); typed
// text here is a &str, which cannot hold them, so those inputs are left out
// of their cases, and AnnotateAlignedDimension.TextThatIsNotUtf8IsRefusedAtTheTextPrompt,
// whose only subject they are, is not twinned here.

use katana_cad::dimension_draw::build_dimension;
use katana_cad::interactive_tool::{ToolInput, ToolStepOutcome};
use katana_commands::{
    EntityAttributes, create_circle, create_dimension_style, create_layer, create_line,
    create_polyline, create_text,
};
use katana_entity::annotation::DEFAULT_ANNOTATION_SCALE;
use katana_entity::{
    ArrowHead, CalloutShape, DimensionGeometry, DimensionKind, DimensionStyle, Geometry, Layer,
    LeaderContext, LeaderGeometry, Model, TextGeometry,
};
use katana_geometry::{Point2, Polyline2, Segment2};
use katana_math::numerics::{HALF_PI, PI};

use crate::tool_driver::ToolDriver;
use crate::{assert_contains, dimensions, drawing, drawn, leaders, near, ok, texts};

fn at(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn text(x: f64, y: f64, words: &str, height: f64, rotation: f64) -> TextGeometry {
    TextGeometry::new(at(x, y), words, height, rotation)
}

fn dimension(start: Point2, end: Point2, offset: f64) -> DimensionGeometry {
    DimensionGeometry::new(start, end, offset, "")
}

// ---- the catalogue -----------------------------------------------------------------

mod annotate_tools {
    use katana_cad::interactive_tool::tool_catalog;

    #[test]
    fn each_tool_is_in_the_annotate_menu_under_its_auto_cad_command_names() {
        let catalog = tool_catalog();
        let expected: [(&str, &[&str]); 8] = [
            ("annotate.text", &["TEXT", "DTEXT", "DT"]),
            ("annotate.dimlinear", &["DIMLINEAR", "DLI"]),
            ("annotate.dimaligned", &["DIMALIGNED", "DAL"]),
            ("annotate.dimangular", &["DIMANGULAR", "DAN"]),
            ("annotate.dimradius", &["DIMRADIUS", "DRA"]),
            ("annotate.dimdiameter", &["DIMDIAMETER", "DDI"]),
            ("annotate.dimordinate", &["DIMORDINATE", "DOR"]),
            ("annotate.leader", &["LEADER", "LEAD", "LE"]),
        ];
        for (id, aliases) in expected {
            let Some(info) = catalog.find(id) else {
                panic!("{id}");
            };
            assert_eq!(info.category, "Annotate", "{id}");
            assert!(!info.tip.is_empty(), "{id}");
            for &alias in aliases {
                let Some(by_alias) = catalog.find_by_alias(alias) else {
                    panic!("{alias}");
                };
                assert_eq!(by_alias.id, id, "{alias}");
            }
        }
    }
}

// ---- Text --------------------------------------------------------------------------

mod annotate_text {
    use super::*;

    #[test]
    fn a_typed_height_and_rotation_place_one_line_at_the_start_point() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        assert_eq!(driver.click(10.0, 20.0).outcome, ToolStepOutcome::Continue);
        assert_eq!(driver.r#type("3").outcome, ToolStepOutcome::Continue);
        assert_eq!(driver.r#type("0").outcome, ToolStepOutcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Value);
        assert_eq!(driver.r#type("Hello").outcome, ToolStepOutcome::Continue);
        assert_eq!(
            driver.executed(),
            0,
            "nothing is committed until the text is finished"
        );
        let done = driver.enter();
        assert_eq!(done.outcome, ToolStepOutcome::Done);
        assert_eq!(driver.executed(), 1);

        let made = texts(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0], text(10.0, 20.0, "Hello", 3.0, 0.0));
        assert_eq!(drawing(&driver)[0].layer, "0", "made on the current layer");
    }

    #[test]
    fn each_line_goes_below_the_last_by_five_thirds_of_the_height_and_one_undo_removes_them_all() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        driver.click(0.0, 20.0);
        driver.r#type("3");
        driver.r#type("0");
        driver.r#type("first");
        driver.r#type("second");
        driver.r#type("third");
        let done = driver.enter();
        assert_eq!(done.outcome, ToolStepOutcome::Done);
        assert_eq!(done.message, "3 lines of text");
        assert_eq!(driver.executed(), 1, "three lines, one command");

        // Spacing 3 * 5 / 3 = 5, straight down for unrotated text: y = 20,
        // 15, 10.
        let made = texts(&driver);
        assert_eq!(made.len(), 3);
        assert_eq!(made[0].position, at(0.0, 20.0));
        assert_eq!(made[1].position, at(0.0, 15.0));
        assert_eq!(made[2].position, at(0.0, 10.0));
        assert_eq!(made[2].text, "third");

        ok(driver.document_mut().undo());
        assert!(drawing(&driver).is_empty());
    }

    #[test]
    fn a_picked_height_is_the_distance_from_the_start_point() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        driver.click(0.0, 0.0);
        // (3, 4) is 5 from the origin.
        driver.click(3.0, 4.0);
        driver.enter(); // rotation: the default, 0
        driver.r#type("A");
        driver.enter();
        let made = texts(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].height, 5.0);
        assert_eq!(made[0].rotation, 0.0);
    }

    #[test]
    fn relative_and_polar_input_measure_from_the_start_point() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        driver.r#type("10,20");
        // @0,3 is (10, 23): a height of 3.
        driver.r#type("@0,3");
        // @4<90 is straight up from the start: a rotation of 90 degrees. The
        // polar point's x is 10 + 4 cos(pi/2), and 4 cos(pi/2) is 2.4e-16
        // rather than 0 - below half an ulp of 10 today, but the test should
        // not rest on that rounding, so the angle is compared to within
        // 1e-15.
        driver.r#type("@4<90");
        driver.r#type("north");
        driver.enter();
        let made = texts(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].position, at(10.0, 20.0));
        assert_eq!(made[0].height, 3.0);
        near(made[0].rotation, HALF_PI, 1e-15);
    }

    #[test]
    fn a_rotated_texts_next_line_is_below_it_as_it_reads() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        driver.click(0.0, 0.0);
        driver.r#type("3");
        // A picked point straight above: atan2(7, 0) is exactly the double
        // nearest pi/2.
        driver.click(0.0, 7.0);
        driver.r#type("one");
        driver.r#type("two");
        driver.enter();
        // Text reading up the page has "below" to its right: (0, -1) turned
        // 90 degrees is (1, 0), so the second line is 5 along +x. sin(pi/2)
        // is exactly 1 in binary; cos(pi/2) is 6e-17, hence the tolerance on
        // y.
        let made = texts(&driver);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].rotation, HALF_PI);
        assert_eq!(made[1].position.x, 5.0);
        near(made[1].position.y, 0.0, 1e-12);
    }

    #[test]
    fn a_typed_rotation_is_in_degrees_counter_clockwise_from_east() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        driver.click(0.0, 0.0);
        driver.r#type("3");
        driver.r#type("90");
        driver.r#type("up");
        driver.r#type("next");
        driver.enter();
        driver.click(20.0, 0.0);
        driver.r#type("3");
        driver.r#type("-90");
        driver.r#type("down");
        driver.r#type("next");
        driver.enter();
        // 90 degrees counter-clockwise is pi/2: the text reads up the page,
        // so "below" is +x and its second line is one spacing (3 x 5 / 3 = 5)
        // along +x, at (5, 0). -90 is -pi/2, clockwise: it reads down the
        // page, "below" is -x, and the second line is at 20 - 5 = 15. 90 x
        // (pi / 180) may land an ulp from the double nearest pi/2, hence
        // 1e-15 on the angles; cos of either is about 6e-17 rather than 0,
        // hence the tolerance on y.
        let made = texts(&driver);
        assert_eq!(made.len(), 4);
        near(made[0].rotation, HALF_PI, 1e-15);
        near(made[1].position.x, 5.0, 1e-12);
        near(made[1].position.y, 0.0, 1e-12);
        near(made[2].rotation, -HALF_PI, 1e-15);
        near(made[3].position.x, 15.0, 1e-12);
        near(made[3].position.y, 0.0, 1e-12);
    }

    #[test]
    fn the_height_offered_is2_point5_when_the_drawing_has_no_text() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        driver.click(1.0, 2.0);
        assert_contains(&driver.prompt(), "<2.5>");
        driver.enter();
        assert_contains(&driver.prompt(), "<0>");
        driver.enter();
        driver.r#type("x");
        driver.enter();
        let made = texts(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].height, 2.5);
        assert_eq!(made[0].rotation, 0.0);
    }

    #[test]
    fn the_height_and_rotation_offered_are_the_newest_texts_in_the_drawing() {
        let mut driver = ToolDriver::new();
        driver.add(create_text(
            text(0.0, 0.0, "old", 7.0, 0.0),
            EntityAttributes::default(),
        ));
        // 0.5 rad is 28.6479 degrees; the prompt shows it to nine decimals.
        driver.add(create_text(
            text(0.0, 50.0, "new", 4.0, 0.5),
            EntityAttributes::default(),
        ));
        driver.start("annotate.text");
        driver.click(100.0, 0.0);
        assert_contains(&driver.prompt(), "<4>");
        driver.enter();
        driver.enter();
        driver.r#type("same again");
        driver.enter();
        let made = texts(&driver);
        assert_eq!(made.len(), 3);
        assert_eq!(made[2].height, 4.0);
        assert_eq!(made[2].rotation, 0.5);
    }

    #[test]
    fn enter_at_the_first_prompt_continues_below_the_newest_text() {
        let mut driver = ToolDriver::new();
        driver.add(create_text(
            text(2.0, 10.0, "above", 3.0, 0.0),
            EntityAttributes::default(),
        ));
        driver.start("annotate.text");
        assert_eq!(driver.enter().outcome, ToolStepOutcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Value, "straight to the text");
        driver.r#type("below");
        driver.enter();
        // One line spacing under (2, 10) at height 3: y = 10 - 5 = 5.
        let made = texts(&driver);
        assert_eq!(made.len(), 2);
        assert_eq!(made[1], text(2.0, 5.0, "below", 3.0, 0.0));
    }

    #[test]
    fn enter_at_the_first_prompt_with_no_text_in_the_drawing_ends_the_tool() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        let step = driver.enter();
        assert_eq!(step.outcome, ToolStepOutcome::Done);
        assert!(step.command.is_none());
        assert!(driver.finished());
    }

    #[test]
    fn enter_with_no_text_typed_ends_the_tool_without_a_command() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        driver.click(0.0, 0.0);
        driver.enter();
        driver.enter();
        let step = driver.enter();
        assert_eq!(step.outcome, ToolStepOutcome::Done);
        assert_eq!(driver.executed(), 0);
        assert!(driver.finished());
    }

    #[test]
    fn the_tool_starts_again_after_each_text() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        driver.click(0.0, 0.0);
        driver.enter();
        driver.enter();
        driver.r#type("one");
        let done = driver.enter();
        assert!(done.restart);
        assert!(!driver.finished());
        assert_eq!(driver.expects(), ToolInput::Point);
        assert_contains(&driver.prompt(), "start point");
    }

    #[test]
    fn typed_text_keeps_its_commas_and_the_letter_u_is_text_not_undo() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        driver.click(0.0, 0.0);
        driver.enter();
        driver.enter();
        assert_eq!(
            driver.r#type("Road, north side").outcome,
            ToolStepOutcome::Continue
        );
        assert_eq!(driver.r#type("U").outcome, ToolStepOutcome::Continue);
        driver.enter();
        let made = texts(&driver);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].text, "Road, north side");
        assert_eq!(made[1].text, "U");
    }

    #[test]
    fn undo_steps_back_one_input_at_a_time() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        assert_eq!(
            driver.undo().outcome,
            ToolStepOutcome::Rejected,
            "nothing to undo yet"
        );
        driver.click(0.0, 0.0);
        driver.r#type("3");
        // Back to the height prompt, then a different height.
        assert_eq!(driver.undo().outcome, ToolStepOutcome::Continue);
        assert_contains(&driver.prompt(), "height");
        driver.r#type("4");
        // U typed at the rotation prompt is Undo as well: back to the height.
        assert_eq!(driver.r#type("u").outcome, ToolStepOutcome::Continue);
        assert_contains(&driver.prompt(), "height");
        driver.r#type("4");
        driver.r#type("0");
        driver.r#type("kept");
        driver.r#type("dropped");
        assert_eq!(driver.undo().outcome, ToolStepOutcome::Continue);
        driver.enter();
        let made = texts(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0], text(0.0, 0.0, "kept", 4.0, 0.0));
    }

    #[test]
    fn bad_input_is_refused_with_a_reason_and_the_tool_stays_where_it_was() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        assert_eq!(
            driver.r#type("5").outcome,
            ToolStepOutcome::Rejected,
            "a number is not a start point"
        );
        driver.click(1.0, 1.0);
        let height_prompt = driver.prompt();
        for bad in ["0", "-2", "abc"] {
            let step = driver.r#type(bad);
            assert_eq!(step.outcome, ToolStepOutcome::Rejected, "{bad}");
            assert!(!step.message.is_empty(), "{bad}");
            assert_eq!(driver.prompt(), height_prompt, "{bad}");
        }
        driver.r#type("2");
        let rotation_prompt = driver.prompt();
        assert_ne!(rotation_prompt, height_prompt);
        assert_eq!(
            driver.click(1.0, 1.0).outcome,
            ToolStepOutcome::Rejected,
            "no direction"
        );
        assert_eq!(driver.r#type("north").outcome, ToolStepOutcome::Rejected);
        assert_eq!(driver.prompt(), rotation_prompt);
        driver.enter();
        // The C++ types "\xff\xfe" here, refused as not UTF-8: a &str cannot
        // hold it (the file's header).
        assert_eq!(driver.executed(), 0);

        // And a picked height of zero, back at the height prompt.
        driver.undo();
        driver.undo();
        assert_eq!(driver.prompt(), height_prompt);
        assert_eq!(
            driver.click(1.0, 1.0).outcome,
            ToolStepOutcome::Rejected,
            "zero height"
        );
        assert_eq!(driver.prompt(), height_prompt);
    }

    #[test]
    fn the_preview_shows_the_rubber_band_then_the_lines_typed_and_where_the_next_goes() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.text");
        driver.click(0.0, 0.0);
        let height = driver.preview(0.0, 3.0);
        assert_eq!(height.shapes.len(), 1);
        assert_eq!(
            height.shapes[0],
            Geometry::Line(Segment2::new(at(0.0, 0.0), at(0.0, 3.0)))
        );
        driver.r#type("3");
        driver.r#type("0");
        driver.r#type("typed");
        let content = driver.preview(50.0, 50.0);
        assert_eq!(content.shapes.len(), 1);
        assert_eq!(
            content.shapes[0],
            Geometry::Text(text(0.0, 0.0, "typed", 3.0, 0.0))
        );
        assert_eq!(content.markers.len(), 1);
        assert_eq!(content.markers[0], at(0.0, -5.0), "the next line, 5 below");
    }
}

// ---- Aligned dimension ---------------------------------------------------------------

mod annotate_aligned_dimension {
    use super::*;

    #[test]
    fn it_measures_the_true_distance_with_the_offset_positive_to_the_left() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimaligned");
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        // The origins are 10 apart along (0.8, 0.6); left of that is (-0.6,
        // 0.8), and (-3, 4) . (-0.6, 0.8) = 1.8 + 3.2 = 5. 0.6 and 0.8 are
        // not exact in binary, so the offset is compared to within rounding.
        let done = driver.click(-3.0, 4.0);
        assert_eq!(done.outcome, ToolStepOutcome::Done);
        assert_eq!(done.message, "aligned dimension measuring 10");
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].start, at(0.0, 0.0));
        assert_eq!(made[0].end, at(8.0, 6.0));
        near(made[0].offset, 5.0, 1e-12);
        assert_eq!(made[0].measurement(), 10.0);
        assert!(made[0].text_override.is_empty());

        ok(driver.document_mut().undo());
        assert!(drawing(&driver).is_empty());
    }

    #[test]
    fn the_offset_is_negative_on_the_right_of_first_to_second() {
        // Heading west from (10, 0) to (0, 0), left is south. Unit direction
        // (-1, 0) and its left normal (0, -1) are exact.
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimaligned");
        driver.click(10.0, 0.0);
        driver.click(0.0, 0.0);
        driver.click(5.0, -3.0); // south: left, +3
        driver.click(10.0, 0.0);
        driver.click(0.0, 0.0);
        driver.click(5.0, 3.0); // north: right, -3
        let made = dimensions(&driver);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].offset, 3.0);
        assert_eq!(made[1].offset, -3.0);
    }

    #[test]
    fn typed_origins_and_a_typed_offset_place_it_exactly() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimaligned");
        driver.r#type("0,0");
        driver.r#type("@10,0");
        let done = driver.r#type("2.5");
        assert_eq!(done.outcome, ToolStepOutcome::Done);
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0], dimension(at(0.0, 0.0), at(10.0, 0.0), 2.5));
    }

    #[test]
    fn polar_and_relative_input_measure_from_the_last_origin() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimaligned");
        driver.r#type("2,2");
        // @5<0 is 5 east of (2, 2): cos 0 and sin 0 are exact, so (7, 2).
        driver.r#type("@5<0");
        // @0,3 from the second origin is (7, 5); heading east, left is north,
        // so the offset is 5 - 2 = 3.
        driver.r#type("@0,3");
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0], dimension(at(2.0, 2.0), at(7.0, 2.0), 3.0));
    }

    #[test]
    fn the_text_option_replaces_the_label_and_enter_restores_the_measurement() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimaligned");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        assert_eq!(driver.r#type("T").outcome, ToolStepOutcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Value);
        driver.r#type("12.50 m, approx");
        driver.click(2.0, 1.0);
        // The second dimension is given a text and then, at the text prompt
        // again, Enter - which must take that text away, not keep it.
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.r#type("T");
        driver.r#type("wrong");
        driver.r#type("text");
        assert_eq!(driver.enter().outcome, ToolStepOutcome::Continue);
        driver.click(2.0, 1.0);
        let made = dimensions(&driver);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].text_override, "12.50 m, approx");
        assert!(
            made[1].text_override.is_empty(),
            "{}",
            made[1].text_override
        );
    }

    #[test]
    fn undo_after_enter_cleared_the_text_brings_the_text_back() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimaligned");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.r#type("T");
        driver.r#type("kept");
        driver.r#type("T");
        driver.enter(); // back to the measured distance...
        assert_eq!(driver.undo().outcome, ToolStepOutcome::Continue); // ...and back to "kept"
        driver.click(2.0, 1.0);
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].text_override, "kept");
    }

    #[test]
    fn undo_takes_back_the_text_option_first_then_the_second_origin() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimaligned");
        assert_eq!(driver.undo().outcome, ToolStepOutcome::Rejected);
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.r#type("T");
        driver.r#type("wrong");
        assert_eq!(driver.undo().outcome, ToolStepOutcome::Continue); // the text
        assert_contains(&driver.prompt(), "dimension line");
        assert_eq!(driver.r#type("U").outcome, ToolStepOutcome::Continue); // the second origin
        assert_contains(&driver.prompt(), "second");
        driver.click(0.0, 6.0);
        driver.click(-2.0, 3.0);
        // (0,0)->(0,6) heads north; left is west, so x = -2 is offset +2.
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0], dimension(at(0.0, 0.0), at(0.0, 6.0), 2.0));
    }

    #[test]
    fn coincident_origins_and_missing_input_are_refused() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimaligned");
        driver.click(3.0, 3.0);
        let second = driver.prompt();
        let same = driver.click(3.0, 3.0);
        assert_eq!(same.outcome, ToolStepOutcome::Rejected);
        assert!(!same.message.is_empty());
        assert_eq!(driver.prompt(), second);
        assert_eq!(driver.enter().outcome, ToolStepOutcome::Rejected);
        assert_eq!(driver.r#type("far").outcome, ToolStepOutcome::Rejected);
        driver.click(5.0, 3.0);
        assert_eq!(driver.enter().outcome, ToolStepOutcome::Rejected);
        assert_eq!(driver.r#type("north").outcome, ToolStepOutcome::Rejected);
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn enter_picks_a_line_or_the_polyline_segment_nearest_the_pick() {
        let mut driver = ToolDriver::new();
        let line = driver.add(create_line(
            at(0.0, 0.0),
            at(6.0, 0.0),
            EntityAttributes::default(),
        ));
        let polyline = driver.add(create_polyline(
            Polyline2::new(vec![at(10.0, 0.0), at(14.0, 0.0), at(14.0, 3.0)], false),
            EntityAttributes::default(),
        ));
        let circle = driver.add(create_circle(
            at(30.0, 0.0),
            2.0,
            EntityAttributes::default(),
        ));
        driver.start("annotate.dimaligned");
        assert_eq!(driver.enter().outcome, ToolStepOutcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Entity);
        assert_eq!(
            driver.pick(circle, 32.0, 0.0).outcome,
            ToolStepOutcome::Rejected,
            "not a line"
        );
        driver.pick(line, 3.0, 0.0);
        driver.click(3.0, 2.0);
        // The polyline's second segment, (14,0)->(14,3), is nearest (14, 1.5).
        driver.enter();
        driver.pick(polyline, 14.0, 1.5);
        // Heading north, left is west: x = 15 is 1 to the right, offset -1.
        driver.click(15.0, 1.5);
        let made = dimensions(&driver);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0], dimension(at(0.0, 0.0), at(6.0, 0.0), 2.0));
        assert_eq!(made[1], dimension(at(14.0, 0.0), at(14.0, 3.0), -1.0));
    }

    #[test]
    fn enter_while_picking_goes_back_to_picking_origins() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimaligned");
        driver.enter();
        assert_eq!(driver.expects(), ToolInput::Entity);
        assert_eq!(
            driver.r#type("0,0").outcome,
            ToolStepOutcome::Rejected,
            "a pick is wanted"
        );
        assert_eq!(driver.enter().outcome, ToolStepOutcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Point);
        assert_contains(&driver.prompt(), "first extension line");
    }

    #[test]
    fn undo_after_picking_a_line_goes_back_to_picking() {
        let mut driver = ToolDriver::new();
        let line = driver.add(create_line(
            at(0.0, 0.0),
            at(6.0, 0.0),
            EntityAttributes::default(),
        ));
        driver.start("annotate.dimaligned");
        driver.enter();
        driver.pick(line, 3.0, 0.0);
        driver.undo();
        assert_eq!(driver.expects(), ToolInput::Entity);
    }

    #[test]
    fn the_preview_is_the_dimension_a_click_there_would_make() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimaligned");
        driver.click(0.0, 0.0);
        let band = driver.preview(4.0, 0.0);
        assert_eq!(band.shapes.len(), 1);
        assert_eq!(
            band.shapes[0],
            Geometry::Line(Segment2::new(at(0.0, 0.0), at(4.0, 0.0)))
        );
        driver.click(4.0, 0.0);
        let feedback = driver.preview(2.0, 1.5);
        assert_eq!(feedback.shapes.len(), 1);
        assert_eq!(
            feedback.shapes[0],
            Geometry::Dimension(dimension(at(0.0, 0.0), at(4.0, 0.0), 1.5))
        );
    }

    #[test]
    fn the_tool_starts_again_after_each_dimension() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimaligned");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        assert!(driver.click(2.0, 1.0).restart);
        assert!(!driver.finished());
        assert_contains(&driver.prompt(), "first extension line");
    }
}

// ---- Linear dimension ----------------------------------------------------------------

mod annotate_linear_dimension {
    use super::*;

    #[test]
    fn a_line_above_the_origins_measures_their_horizontal_distance() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        // (4, 10) is above both: horizontal. The origin farther from y = 10
        // is (0, 0), so both project onto y = 0: start (0, 0), end (8, 0),
        // which measures 8; heading east, left is north, and y = 10 is offset
        // +10.
        let done = driver.click(4.0, 10.0);
        assert_eq!(done.outcome, ToolStepOutcome::Done);
        assert_eq!(done.message, "linear dimension measuring 8");
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0], dimension(at(0.0, 0.0), at(8.0, 0.0), 10.0));
        assert_eq!(made[0].measurement(), 8.0);

        ok(driver.document_mut().undo());
        assert!(drawing(&driver).is_empty());
    }

    #[test]
    fn a_line_beside_the_origins_measures_their_vertical_distance() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        // (12, 3) is right of both: vertical. The origin farther from x = 12
        // is (0, 0), so both project onto x = 0: start (0, 0), end (0, 6),
        // which measures 6; heading north, left is west, and x = 12 is offset
        // -12.
        driver.click(12.0, 3.0);
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0], dimension(at(0.0, 0.0), at(0.0, 6.0), -12.0));
        assert_eq!(made[0].measurement(), 6.0);
    }

    #[test]
    fn a_picked_segment_and_a_typed_location_give_the_same_projection() {
        let mut driver = ToolDriver::new();
        let line = driver.add(create_line(
            at(0.0, 0.0),
            at(6.0, 4.0),
            EntityAttributes::default(),
        ));
        driver.start("annotate.dimlinear");
        driver.enter();
        driver.pick(line, 3.0, 2.0);
        // (3, 9) is above both ends: horizontal. (0, 0) is 9 from y = 9 and
        // (6, 4) only 5, so both project onto y = 0: (0, 0) to (6, 0), offset
        // 9.
        driver.r#type("3,9");
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0], dimension(at(0.0, 0.0), at(6.0, 0.0), 9.0));
        assert_eq!(made[0].measurement(), 6.0);
    }

    #[test]
    fn the_nearer_origin_lies_on_its_extension_line_when_the_levels_differ_by_more_than_dimexo() {
        // The dimension of the first test, drawn: the extension lines run
        // from DIMEXO above (0, 0) and (8, 0) up past y = 10. The levels
        // differ by 6, more than DIMEXO, so the second origin, (8, 6), is on
        // its line, which carries on below it for 6 - 0.625 = 5.375 - the
        // price of projecting both origins onto the farther one's level.
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        driver.click(4.0, 10.0);
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        let style = DimensionStyle::default();
        let built = build_dimension(&made[0], &style, DEFAULT_ANNOTATION_SCALE);
        assert_eq!(built.extension_lines.len(), 2);
        // Standard style: DIMEXO 0.625, DIMEXE 1.25.
        assert_eq!(
            built.extension_lines[0],
            Segment2::new(at(0.0, 0.625), at(0.0, 11.25))
        );
        assert_eq!(
            built.extension_lines[1],
            Segment2::new(at(8.0, 0.625), at(8.0, 11.25))
        );
        assert_eq!(built.extension_lines[1].distance_to(at(8.0, 6.0)), 0.0);
    }

    #[test]
    fn when_the_levels_differ_by_less_than_dimexo_the_nearer_origin_is_in_the_gap_below_its_line() {
        // (0, 0) and (8, 0.25) with the line at y = 10: (0, 0) is farther, so
        // both project onto y = 0 and the second extension line starts DIMEXO
        // up, at (8, 0.625). The origin (8, 0.25) is 0.625 - 0.25 = 0.375
        // short of it: in the gap every extension line leaves at its foot, as
        // the farther origin always is, not on the line. All of it exact in
        // binary.
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        driver.click(0.0, 0.0);
        driver.click(8.0, 0.25);
        driver.click(4.0, 10.0);
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0], dimension(at(0.0, 0.0), at(8.0, 0.0), 10.0));
        let built = build_dimension(
            &made[0],
            &DimensionStyle::default(),
            DEFAULT_ANNOTATION_SCALE,
        );
        assert_eq!(built.extension_lines.len(), 2);
        assert_eq!(
            built.extension_lines[1],
            Segment2::new(at(8.0, 0.625), at(8.0, 11.25))
        );
        assert_eq!(built.extension_lines[1].distance_to(at(8.0, 0.25)), 0.375);
    }

    #[test]
    fn h_and_v_choose_the_orientation_wherever_the_line_is_placed() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        // (14, 10) is off the corner, 6 out in x and 4 in y - vertical by
        // default - but H makes it horizontal: y = 10, projected onto the
        // farther level y = 0, measuring 8.
        assert_eq!(driver.r#type("h").outcome, ToolStepOutcome::Continue);
        driver.click(14.0, 10.0);
        // V with the line above and left, at (-2, 10): the farther origin in
        // x is (8, 6), so both project onto x = 8: start (8, 0), end (8, 6),
        // heading north with west on the left, and x = -2 is offset +10.
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        assert_eq!(driver.r#type("Vertical").outcome, ToolStepOutcome::Continue);
        driver.click(-2.0, 10.0);
        let made = dimensions(&driver);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0], dimension(at(0.0, 0.0), at(8.0, 0.0), 10.0));
        assert_eq!(made[1], dimension(at(8.0, 0.0), at(8.0, 6.0), 10.0));
        assert_eq!(made[1].measurement(), 6.0);
    }

    #[test]
    fn undo_takes_back_a_chosen_orientation() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        driver.r#type("H");
        driver.undo();
        // Back to choosing by position: (12, 3) is beside, so vertical.
        driver.click(12.0, 3.0);
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].measurement(), 6.0);
    }

    #[test]
    fn placements_that_say_no_orientation_are_refused_with_a_reason() {
        // A line between the two origins' levels was refused here too until
        // the tool could make the Linear kind, which draws it as AutoCAD's
        // DIMLINEAR does (test_annotate_leader_options.cpp,
        // ALineBetweenTheOriginsLevelsIsTheLinearKindDimHorizontalMakes).
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        let location = driver.prompt();
        // Inside the rectangle the origins span: horizontal or vertical?
        let inside = driver.click(4.0, 3.0);
        assert_eq!(inside.outcome, ToolStepOutcome::Rejected);
        assert_contains(&inside.message, "H or V");
        assert_eq!(driver.prompt(), location);
        driver.r#type("H");
        let horizontal = driver.prompt();
        assert_contains(&horizontal, "horizontal");
        assert_eq!(
            driver.r#type("2.5").outcome,
            ToolStepOutcome::Rejected,
            "no typed offset for linear"
        );
        assert_eq!(driver.prompt(), horizontal);
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn an_orientation_that_would_measure_nothing_is_refused() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        // One above the other: a horizontal dimension of them is zero.
        driver.click(0.0, 0.0);
        driver.click(0.0, 5.0);
        let zero = driver.click(0.0, 9.0);
        assert_eq!(zero.outcome, ToolStepOutcome::Rejected);
        assert_contains(&zero.message, "measure nothing");
        // Beside them it is vertical, 5, projected onto x = 0 (a tie keeps
        // the first); heading north, x = 3 is 3 to the right, offset -3.
        assert_eq!(driver.click(3.0, 2.0).outcome, ToolStepOutcome::Done);
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0], dimension(at(0.0, 0.0), at(0.0, 5.0), -3.0));
    }

    #[test]
    fn off_the_corner_of_level_origins_the_orientation_that_measures_something_is_chosen() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        // A level pair, as a horizontal edge gives: (15, 3) is 5 past its end
        // and 3 above it. Farther out in x would mean vertical, which
        // measures nothing here, but the line IS above the origins, so it is
        // horizontal: start (0, 0), end (10, 0) (a tie of levels keeps the
        // first), measuring 10; heading east, left is north, so y = 3 is
        // offset +3.
        driver.click(0.0, 0.0);
        driver.click(10.0, 0.0);
        let across = dimension(at(0.0, 0.0), at(10.0, 0.0), 3.0);
        let feedback = driver.preview(15.0, 3.0);
        assert_eq!(feedback.shapes.len(), 1);
        assert_eq!(feedback.shapes[0], Geometry::Dimension(across.clone()));
        // Level with the origins, past the end, neither orientation measures
        // anything from there: still refused.
        assert_eq!(driver.click(15.0, 0.0).outcome, ToolStepOutcome::Rejected);
        // A forced V is not overridden: it is what the user asked for.
        driver.r#type("V");
        assert_eq!(driver.click(15.0, 3.0).outcome, ToolStepOutcome::Rejected);
        driver.undo();
        let done = driver.click(15.0, 3.0);
        assert_eq!(done.outcome, ToolStepOutcome::Done, "{}", done.message);
        assert_eq!(done.message, "linear dimension measuring 10");

        // One above the other, the same the other way: (2, 9) is 4 above and
        // 2 beside, but horizontal would measure nothing, so vertical: start
        // (0, 0), end (0, 5); heading north, left is west, so x = 2 is -2.
        driver.click(0.0, 0.0);
        driver.click(0.0, 5.0);
        assert_eq!(driver.click(2.0, 9.0).outcome, ToolStepOutcome::Done);

        let made = dimensions(&driver);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0], across);
        assert_eq!(made[1], dimension(at(0.0, 0.0), at(0.0, 5.0), -2.0));
    }

    #[test]
    fn the_preview_follows_the_orientation_the_cursor_chooses() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        let above = driver.preview(4.0, 10.0);
        assert_eq!(above.shapes.len(), 1);
        assert_eq!(
            above.shapes[0],
            Geometry::Dimension(dimension(at(0.0, 0.0), at(8.0, 0.0), 10.0))
        );
        // Inside the rectangle there is no dimension to show, only the
        // origins.
        let inside = driver.preview(4.0, 3.0);
        assert!(inside.shapes.is_empty());
        assert_eq!(inside.markers.len(), 2);
    }
}

// ---- Angular, radius, diameter and ordinate ----------------------------------------------

mod annotate_angular_dimension {
    use super::*;

    #[test]
    fn two_picked_lines_make_one_angular_dimension_that_follows_them() {
        let mut driver = ToolDriver::new();
        let level = driver.add(create_line(
            at(0.0, 0.0),
            at(10.0, 0.0),
            EntityAttributes::default(),
        ));
        let upright = driver.add(create_line(
            at(0.0, 0.0),
            at(0.0, 10.0),
            EntityAttributes::default(),
        ));
        driver.start("annotate.dimangular");
        assert_eq!(driver.expects(), ToolInput::Entity);
        driver.pick(level, 5.0, 0.0);
        driver.pick(upright, 0.0, 5.0);
        assert_eq!(driver.expects(), ToolInput::Point);
        // The arc between them, on the side of (3, 3): the right angle.
        let done = driver.click(3.0, 3.0);
        assert_eq!(done.outcome, ToolStepOutcome::Done);
        assert_eq!(done.message, "angular dimension measuring 90 degrees");
        assert_eq!(driver.executed(), 1);
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].kind, DimensionKind::Angular);
        near(made[0].measurement(), 0.5 * PI, 1e-12);
        assert_eq!(
            made[0].start_ref.entity, level,
            "associated with the lines picked"
        );
        assert_eq!(made[0].end_ref.entity, upright);
    }

    #[test]
    fn enter_gives_the_vertex_and_two_points() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimangular");
        assert_eq!(driver.enter().outcome, ToolStepOutcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Point);
        driver.click(0.0, 0.0);
        assert_eq!(
            driver.click(0.0, 0.0).outcome,
            ToolStepOutcome::Rejected,
            "an arm needs a length"
        );
        driver.click(10.0, 0.0);
        driver.click(10.0, 10.0);
        // The preview is the dimension the click will make.
        let feedback = driver.preview(6.0, 2.0);
        assert_eq!(feedback.shapes.len(), 1);
        let Geometry::Dimension(previewed) = &feedback.shapes[0] else {
            panic!("{:?}", feedback.shapes[0]);
        };
        near(previewed.measurement(), 0.25 * PI, 1e-12);
        let done = driver.click(6.0, 2.0);
        assert_eq!(done.outcome, ToolStepOutcome::Done);
        assert_eq!(done.message, "angular dimension measuring 45 degrees");
        let made = dimensions(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].vertex, at(0.0, 0.0));
        assert!(!made[0].start_ref.associated(), "points, not entities");
    }

    #[test]
    fn only_lines_are_picked_and_undo_steps_back() {
        let mut driver = ToolDriver::new();
        let round = driver.add(create_circle(
            at(0.0, 0.0),
            2.0,
            EntityAttributes::default(),
        ));
        let line = driver.add(create_line(
            at(0.0, 0.0),
            at(10.0, 0.0),
            EntityAttributes::default(),
        ));
        driver.start("annotate.dimangular");
        assert_eq!(
            driver.pick(round, 2.0, 0.0).outcome,
            ToolStepOutcome::Rejected
        );
        driver.pick(line, 5.0, 0.0);
        assert_eq!(
            driver.pick(line, 6.0, 0.0).outcome,
            ToolStepOutcome::Rejected,
            "the same line twice"
        );
        assert_eq!(driver.r#type("U").outcome, ToolStepOutcome::Continue);
        assert_eq!(
            driver.prompt(),
            "Select the first line, or press Enter to specify the vertex"
        );
        assert_eq!(driver.executed(), 0);
    }
}

mod annotate_radial_dimension {
    use super::*;

    #[test]
    fn a_radius_and_a_diameter_of_a_picked_circle_follow_it() {
        let mut driver = ToolDriver::new();
        let round = driver.add(create_circle(
            at(10.0, 10.0),
            3.0,
            EntityAttributes::default(),
        ));
        driver.start("annotate.dimradius");
        assert_eq!(driver.expects(), ToolInput::Entity);
        driver.pick(round, 13.0, 10.0);
        let radius = driver.click(16.0, 10.0);
        assert_eq!(radius.outcome, ToolStepOutcome::Done);
        assert_eq!(radius.message, "radius dimension measuring 3");

        driver.start("annotate.dimdiameter");
        driver.pick(round, 13.0, 10.0);
        let diameter = driver.click(10.0, 16.0);
        assert_eq!(diameter.outcome, ToolStepOutcome::Done);
        assert_eq!(diameter.message, "diameter dimension measuring 6");

        let made = dimensions(&driver);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].kind, DimensionKind::Radius);
        assert_eq!(made[1].kind, DimensionKind::Diameter);
        assert_eq!(
            made[0].vertex_ref.entity, round,
            "follows the circle's centre"
        );
        assert_eq!(made[0].vertex, at(10.0, 10.0));
    }

    #[test]
    fn only_an_arc_or_a_circle_is_taken() {
        let mut driver = ToolDriver::new();
        let line = driver.add(create_line(
            at(0.0, 0.0),
            at(10.0, 0.0),
            EntityAttributes::default(),
        ));
        driver.start("annotate.dimradius");
        assert_eq!(
            driver.pick(line, 5.0, 0.0).outcome,
            ToolStepOutcome::Rejected
        );
        assert_eq!(
            driver.undo().outcome,
            ToolStepOutcome::Rejected,
            "nothing picked yet"
        );
        assert_eq!(driver.executed(), 0);
    }
}

mod annotate_ordinate_dimension {
    use super::*;

    #[test]
    fn the_leaders_direction_chooses_the_ordinate_unless_typed() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimordinate");
        driver.click(12.0, 7.0);
        // Straight up from the feature: the value written up the sheet is its
        // easting, the X ordinate.
        let up = driver.click(12.0, 15.0);
        assert_eq!(up.outcome, ToolStepOutcome::Done);
        assert_eq!(up.message, "X ordinate dimension measuring 12");
        // Across, with X typed: still the X ordinate.
        driver.click(12.0, 7.0);
        assert_eq!(driver.r#type("X").outcome, ToolStepOutcome::Continue);
        let typed = driver.click(20.0, 7.0);
        assert_eq!(typed.outcome, ToolStepOutcome::Done);
        assert_eq!(typed.message, "X ordinate dimension measuring 12");
        // Across without: the Y ordinate.
        driver.click(12.0, 7.0);
        let across = driver.click(20.0, 7.0);
        assert_eq!(across.outcome, ToolStepOutcome::Done);
        assert_eq!(across.message, "Y ordinate dimension measuring 7");
        assert_eq!(driver.executed(), 3);
    }
}

// ---- Leader --------------------------------------------------------------------------

// The one leader entity the tool made.
#[track_caller]
fn only_leader(driver: &ToolDriver) -> LeaderGeometry {
    let made = leaders(driver);
    assert_eq!(made.len(), 1);
    made.into_iter().next().unwrap_or_default()
}

// A leader from (0, 0) east to (10, 0) with the note "X", drawn on a layer
// whose dimension style has `head` 3 long.
fn leader_with_arrow_head(head: ArrowHead) -> LeaderGeometry {
    let mut driver = ToolDriver::new();
    let style = DimensionStyle {
        name: String::from("Heads"),
        arrow_head: head,
        arrow_size: 3.0,
        ..DimensionStyle::default()
    };
    ok(driver.document_mut().execute(create_dimension_style(style)));
    let layer = Layer {
        name: String::from("Notes"),
        dimension_style: String::from("Heads"),
        ..Layer::default()
    };
    ok(driver.document_mut().execute(create_layer(layer)));
    ok(driver.document_mut().set_current_layer("Notes"));
    driver.start("annotate.leader");
    driver.click(0.0, 0.0);
    driver.click(10.0, 0.0);
    driver.enter();
    driver.r#type("X");
    driver.enter();
    only_leader(&driver)
}

mod annotate_leader {
    use katana_cad::annotation::drawing::estimated_measure;
    use katana_cad::annotation::leader_draw::build_leader;

    use super::*;

    #[test]
    fn a_leader_is_one_entity_made_by_one_command() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        assert_eq!(driver.enter().outcome, ToolStepOutcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Value);
        driver.r#type("AB");
        let done = driver.enter();
        assert_eq!(done.outcome, ToolStepOutcome::Done);
        assert_eq!(done.message, "leader with 1 line of text");
        assert_eq!(driver.executed(), 1);

        assert_eq!(
            drawing(&driver).len(),
            1,
            "line, arrow and note are one entity"
        );
        let leader = only_leader(&driver);
        assert_eq!(leader.vertices, vec![at(0.0, 0.0), at(8.0, 6.0)]);
        assert_eq!(leader.text, "AB");
        assert_eq!(leader.arrow, ArrowHead::ClosedFilled);
        assert_eq!(leader.callout, CalloutShape::None);
        // The Standard style's 2.5 model units, on paper at the default
        // 1 : 1000: 2.5 mm, so it is drawn the size the exploded leader was.
        assert_eq!(leader.arrow_size, 2.5);
        assert_eq!(leader.paper_height, 2.5);
        // The last segment rises at atan(6/8) = 36.9 degrees, more than 15,
        // so a landing one arrowhead long.
        assert_eq!(leader.landing, 2.5);

        // Drawn: the line runs on along the landing, right from (8, 6) to
        // (10.5, 6); the closed head is filled at the tip; the note stands
        // half a text height (1.25) past the landing's end.
        let picture = drawn(&driver, &leader);
        assert!(!picture.strokes.is_empty());
        assert_eq!(
            picture.strokes[0],
            vec![at(0.0, 0.0), at(8.0, 6.0), at(10.5, 6.0)]
        );
        assert_eq!(picture.fills.len(), 1);
        assert_eq!(picture.fills[0][0], at(0.0, 0.0));
        assert_eq!(picture.texts.len(), 1);
        near(picture.texts[0].origin.x, 11.75, 1e-12);

        ok(driver.document_mut().undo());
        assert!(drawing(&driver).is_empty(), "one undo removes the leader");
    }

    #[test]
    fn a_note_left_of_the_end_is_right_aligned_to_it_line_by_line() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.click(10.0, 0.0);
        driver.click(4.0, 8.0);
        driver.click(0.0, 8.0);
        driver.enter();
        driver.r#type("AB");
        driver.r#type("C");
        let done = driver.enter();
        assert_eq!(done.message, "leader with 2 lines of text");
        let leader = only_leader(&driver);
        assert_eq!(leader.text, "AB\nC", "the lines typed, one note");
        assert_eq!(
            leader.landing, 0.0,
            "the last segment runs level: no landing"
        );
        // The note ends 1.25 left of (0, 8), each line right-aligned to it:
        // "AB" is 2 x 0.6 x 2.5 = 3 wide and starts at -4.25, "C" is 1.5 wide
        // and starts at -2.75.
        let picture = drawn(&driver, &leader);
        assert_eq!(picture.texts.len(), 2);
        near(picture.texts[0].origin.x, -4.25, 1e-12);
        near(picture.texts[1].origin.x, -2.75, 1e-12);
        assert!(
            picture.texts[0].origin.y > picture.texts[1].origin.y,
            "first line on top"
        );
    }

    #[test]
    fn a_steep_last_segment_heading_left_gets_its_landing_to_the_left() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.click(10.0, 0.0);
        driver.click(4.0, 8.0);
        driver.enter();
        driver.r#type("AB");
        driver.enter();
        // The last segment is (-6, 8): atan(8 / 6) = 53.1 degrees off level,
        // heading left, so the landing runs one arrowhead (2.5) LEFT to
        // (1.5, 8).
        let leader = only_leader(&driver);
        assert_eq!(leader.landing, 2.5);
        let picture = drawn(&driver, &leader);
        assert!(!picture.strokes.is_empty());
        assert_eq!(picture.strokes[0].last(), Some(&at(1.5, 8.0)));
    }

    #[test]
    fn a_last_segment_typed_due_south_as270_degrees_puts_the_note_on_the_right() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.r#type("0,0");
        driver.r#type("@10<0");
        // 270 degrees is due south, but cos(3 pi / 2) in binary is -1.8e-16,
        // not 0, so the point is 10 x that = 1.8e-15 left of x = 10: the
        // double just below 10. sin rounds to exactly -1, so y is -10. That
        // is straight down within rounding, which the rule puts on the right:
        // a landing 2.5 right to (12.5, -10). Compared to within 1e-12, far
        // below the 5 between the two sides.
        driver.r#type("@10<270");
        driver.enter();
        driver.r#type("NOTE");
        driver.enter();
        let picture = drawn(&driver, &only_leader(&driver));
        assert!(!picture.strokes.is_empty());
        assert_eq!(picture.strokes[0].len(), 4);
        near(picture.strokes[0][3].x, 12.5, 1e-12);
        assert_eq!(picture.strokes[0][3].y, -10.0);
    }

    #[test]
    fn a_last_segment_within_fifteen_degrees_of_level_gets_no_landing() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.click(0.0, 0.0);
        // atan(2 / 10) is 11.3 degrees.
        driver.click(10.0, 2.0);
        driver.enter();
        driver.r#type("N");
        driver.enter();
        let leader = only_leader(&driver);
        assert_eq!(leader.landing, 0.0);
        assert_eq!(drawn(&driver, &leader).strokes[0].len(), 2);
    }

    #[test]
    fn a_leader_without_a_note_has_no_landing() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.click(0.0, 0.0);
        driver.click(0.0, 10.0);
        driver.enter();
        let done = driver.enter();
        assert_eq!(done.message, "leader with no text");
        let leader = only_leader(&driver);
        assert!(leader.text.is_empty());
        // No note, so no landing, although the line is vertical.
        assert_eq!(leader.landing, 0.0);
    }

    #[test]
    fn its_arrow_and_note_take_the_current_layers_dimension_style() {
        let mut driver = ToolDriver::new();
        let style = DimensionStyle {
            name: String::from("Survey"),
            arrow_head: ArrowHead::Dot,
            arrow_size: 4.0,
            text_height: 3.0,
            text_gap: 1.0,
            ..DimensionStyle::default()
        };
        ok(driver.document_mut().execute(create_dimension_style(style)));
        let layer = Layer {
            name: String::from("Notes"),
            dimension_style: String::from("Survey"),
            ..Layer::default()
        };
        ok(driver.document_mut().execute(create_layer(layer)));
        ok(driver.document_mut().set_current_layer("Notes"));

        driver.start("annotate.leader");
        driver.click(0.0, 0.0);
        driver.click(10.0, 0.0);
        driver.enter();
        driver.r#type("X");
        driver.enter();
        let leader = only_leader(&driver);
        assert_eq!(leader.arrow, ArrowHead::Dot);
        // Model units at 1 : 1000 are millimetres one for one.
        assert_eq!(leader.arrow_size, 4.0);
        assert_eq!(leader.paper_height, 3.0);
        assert_eq!(drawing(&driver)[0].layer, "Notes");
    }

    #[test]
    fn a_model_unit_style_is_taken_at_the_annotation_scale_and_a_paper_one_as_it_is() {
        // At 1 : 500 the Standard style's 2.5 model units are 5 mm on paper:
        // the leader is made the size it would have been drawn, and then
        // keeps its size on paper.
        let mut driver = ToolDriver::new();
        ok(driver.document_mut().set_annotation_scale(500.0));
        driver.start("annotate.leader");
        driver.click(0.0, 0.0);
        driver.click(10.0, 0.0);
        driver.enter();
        driver.r#type("X");
        driver.enter();
        let at_scale = only_leader(&driver);
        assert_eq!(at_scale.arrow_size, 5.0);
        assert_eq!(at_scale.paper_height, 5.0);

        // A paper-sized style's millimetres are the leader's whatever the
        // scale.
        let mut paper = ToolDriver::new();
        let style = DimensionStyle {
            name: String::from("Sheet"),
            paper_sized: true,
            arrow_size: 2.0,
            text_height: 1.8,
            ..DimensionStyle::default()
        };
        ok(paper.document_mut().execute(create_dimension_style(style)));
        let layer = Layer {
            name: String::from("Notes"),
            dimension_style: String::from("Sheet"),
            ..Layer::default()
        };
        ok(paper.document_mut().execute(create_layer(layer)));
        ok(paper.document_mut().set_current_layer("Notes"));
        ok(paper.document_mut().set_annotation_scale(200.0));
        paper.start("annotate.leader");
        paper.click(0.0, 0.0);
        paper.click(10.0, 0.0);
        paper.enter();
        paper.r#type("X");
        paper.enter();
        let sized = only_leader(&paper);
        assert_eq!(sized.arrow_size, 2.0);
        assert_eq!(sized.paper_height, 1.8);
    }

    #[test]
    fn every_arrowhead_kind_is_the_leaders_and_draws_as_a_dimensions_does() {
        let empty = Model::default();
        let draw = |head: ArrowHead| {
            let leader = leader_with_arrow_head(head);
            assert_eq!(leader.arrow, head);
            assert_eq!(leader.arrow_size, 3.0);
            build_leader(
                &empty,
                &leader,
                1000.0,
                &estimated_measure(),
                &LeaderContext::default(),
            )
        };
        // Open: two strokes meeting at the tip, drawn beside the line; back 3
        // along (1, 0) and a third of 3 either side.
        let open = draw(ArrowHead::Open);
        assert_eq!(open.strokes.len(), 2);
        assert_eq!(open.strokes[1].len(), 3);
        assert_eq!(open.strokes[1][1], at(0.0, 0.0));
        near(open.strokes[1][0].x, 3.0, 1e-12);
        near(open.strokes[1][0].y.abs(), 1.0, 1e-12);
        assert!(open.fills.is_empty());
        // Tick: one stroke through the tip, leaning / at 45 degrees.
        let tick = draw(ArrowHead::Tick);
        assert_eq!(tick.strokes.len(), 2);
        assert_eq!(tick.strokes[1].len(), 2);
        near(tick.strokes[1][0].y, tick.strokes[1][0].x, 1e-12); // leans /
        near(tick.strokes[1][1].x, -tick.strokes[1][0].x, 1e-12); // centred on the tip
        // Closed and dot: filled.
        assert_eq!(draw(ArrowHead::ClosedFilled).fills.len(), 1);
        assert_eq!(draw(ArrowHead::Dot).fills.len(), 1);
        // None: the line and the note only.
        let none = draw(ArrowHead::None);
        assert_eq!(none.strokes.len(), 1);
        assert!(none.fills.is_empty());
    }

    #[test]
    fn typed_and_polar_points_and_the_annotation_option() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.r#type("0,0");
        // @10<0 is (10, 0): cos 0 and sin 0 are exact.
        driver.r#type("@10<0");
        driver.r#type("@0,-5");
        assert_eq!(driver.r#type("A").outcome, ToolStepOutcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Value);
        driver.enter();
        assert_eq!(
            only_leader(&driver).vertices,
            vec![at(0.0, 0.0), at(10.0, 0.0), at(10.0, -5.0)]
        );
    }

    #[test]
    fn undo_takes_back_the_last_point_or_line_of_text() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        assert_eq!(driver.undo().outcome, ToolStepOutcome::Rejected);
        driver.click(0.0, 0.0);
        driver.click(5.0, 0.0);
        driver.click(9.0, 9.0);
        assert_eq!(driver.r#type("U").outcome, ToolStepOutcome::Continue);
        driver.enter();
        driver.r#type("keep");
        driver.r#type("drop");
        driver.undo();
        driver.enter();
        let leader = only_leader(&driver);
        assert_eq!(leader.vertices, vec![at(0.0, 0.0), at(5.0, 0.0)]);
        assert_eq!(leader.text, "keep");
    }

    #[test]
    fn degenerate_input_is_refused_and_the_tool_stays_where_it_was() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.click(2.0, 2.0);
        let prompt = driver.prompt();
        assert_eq!(
            driver.click(2.0, 2.0).outcome,
            ToolStepOutcome::Rejected,
            "zero-length segment"
        );
        assert_eq!(
            driver.enter().outcome,
            ToolStepOutcome::Rejected,
            "only the tip"
        );
        assert_eq!(
            driver.r#type("A").outcome,
            ToolStepOutcome::Rejected,
            "no annotation yet"
        );
        assert_eq!(driver.r#type("7").outcome, ToolStepOutcome::Rejected);
        assert_eq!(driver.prompt(), prompt);
        driver.click(6.0, 2.0);
        driver.enter();
        // The C++ types "\xc3" here, a truncated UTF-8 sequence refused at
        // the note: a &str cannot hold it (the file's header).
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn the_preview_is_the_leader_with_the_cursor_as_its_next_point() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.click(0.0, 0.0);
        let feedback = driver.preview(10.0, 0.0);
        // The leader itself, as it will be made, drawn by the view like any
        // other: its arrow is seen at its size before the point is placed.
        assert_eq!(feedback.shapes.len(), 1);
        let Geometry::Leader(leader) = &feedback.shapes[0] else {
            panic!("{:?}", feedback.shapes[0]);
        };
        assert_eq!(leader.vertices, vec![at(0.0, 0.0), at(10.0, 0.0)]);
        assert_eq!(leader.arrow, ArrowHead::ClosedFilled);
        assert_eq!(leader.arrow_size, 2.5);
    }

    #[test]
    fn the_tool_starts_again_after_each_leader() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.click(0.0, 0.0);
        driver.click(5.0, 0.0);
        driver.enter();
        assert!(driver.enter().restart);
        assert!(!driver.finished());
        assert!(driver.last_point().is_none());
    }
}
