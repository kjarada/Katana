// The Baseline and Continue Dimension tools (annotate_dimension_chain.rs),
// driven through ToolDriver and held to DIM BASELINE and DIM CONTINUE: the
// same points typed into a second drawing made the same way must make the
// same dimensions. The spacing and offsets are worked out by hand beside each
// check (tests/cad/tools/test_annotate_dimension_chain.cpp).

use katana_cad::Document;
use katana_entity::{DimensionGeometry, Entity, Geometry};

fn dimensions_of(document: &Document) -> Vec<Entity> {
    let mut out = Vec::new();
    document.model().entities.for_each(|entity| {
        if matches!(entity.geometry, Geometry::Dimension(_)) {
            out.push(entity.clone());
        }
    });
    out
}

#[track_caller]
fn geometry_of(entity: &Entity) -> &DimensionGeometry {
    match &entity.geometry {
        Geometry::Dimension(dimension) => dimension,
        other => panic!("not a dimension: {other:?}"),
    }
}

#[track_caller]
fn expect_same_dimensions(drawn: &Document, typed: &Document) {
    let made = dimensions_of(drawn);
    let expected = dimensions_of(typed);
    assert_eq!(made.len(), expected.len());
    for (i, (a, b)) in made.iter().zip(&expected).enumerate() {
        assert_eq!(a.geometry, b.geometry, "{i}");
        assert_eq!(a.layer, b.layer, "{i}");
        assert_eq!(a.style, b.style, "{i}");
    }
}

mod dimension_chain_tools {
    use katana_cad::Document;
    use katana_cad::interactive_tool::{ToolInput, tool_catalog};
    use katana_entity::{AnchorPoint, DimensionKind};

    use super::{dimensions_of, expect_same_dimensions, geometry_of};
    use crate::tool_driver::ToolDriver;
    use crate::{CONTINUE, DONE, REJECTED, anchor, contains, ok, pt, run};

    #[test]
    fn they_are_in_the_annotate_menu_under_their_command_names() {
        let catalog = tool_catalog();
        let expected: [(&str, &[&str]); 3] = [
            ("annotate.dimbaseline", &["DIMBASELINE", "DBA"]),
            ("annotate.dimcontinue", &["DIMCONTINUE", "DCO"]),
            ("annotate.balloon", &["BALLOON"]),
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

    #[test]
    fn baseline_goes_on_from_the_newest_dimension_as_dim_baseline_does() {
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "DIM LINEAR 0,0 10,0 at=5,5"); // 1
        driver.start("annotate.dimbaseline");
        assert_eq!(
            driver.expects(),
            ToolInput::Point,
            "the newest is taken as the base"
        );
        assert_eq!(
            driver.prompt(),
            "Specify next extension line origin (from dimension 1) or [Select/Spacing/Undo] \
             <done>"
        );
        driver.click(20.0, 0.0);
        driver.click(30.0, 0.0);
        let done = driver.enter();
        assert_eq!(done.outcome, DONE, "{}", done.message);
        assert_eq!(done.message, "2 baseline dimensions");
        assert_eq!(driver.executed(), 1, "the chain is one undo step");

        let mut typed = Document::default();
        run(&mut typed, "DIM LINEAR 0,0 10,0 at=5,5");
        run(&mut typed, "DIM BASELINE 1 20,0 30,0");
        expect_same_dimensions(driver.document(), &typed);
        // The Standard style's 2.5 text at 1 : 1000 is 2.5, so the lines are
        // 3.75 apart: the base's at 5, then 8.75 and 12.5, all from 0,0.
        let all = dimensions_of(driver.document());
        assert_eq!(all.len(), 3);
        assert_eq!(geometry_of(&all[1]).kind, DimensionKind::Linear);
        assert_eq!(geometry_of(&all[1]).start, pt(0.0, 0.0));
        assert_eq!(geometry_of(&all[1]).offset, 8.75);
        assert_eq!(geometry_of(&all[2]).offset, 12.5);
        assert_eq!(geometry_of(&all[2]).measurement(), 30.0);
        ok(driver.document_mut().undo());
        assert_eq!(dimensions_of(driver.document()).len(), 1);
    }

    #[test]
    fn spacing_is_baselines_and_continue_chains_end_to_end() {
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "DIM LINEAR 0,0 10,0 at=5,5");
        driver.start("annotate.dimbaseline");
        driver.r#type("SP");
        assert_eq!(driver.prompt(), "Specify baseline spacing <3.75>");
        assert_eq!(driver.r#type("0").outcome, REJECTED);
        driver.r#type("5");
        driver.click(20.0, 0.0);
        assert_eq!(driver.enter().outcome, DONE);
        let mut typed = Document::default();
        run(&mut typed, "DIM LINEAR 0,0 10,0 at=5,5");
        run(&mut typed, "DIM BASELINE 1 20,0 spacing=5");
        expect_same_dimensions(driver.document(), &typed);
        assert_eq!(
            geometry_of(&dimensions_of(driver.document())[1]).offset,
            10.0
        );

        // Continue from an aligned base: end to end, each keeping its offset.
        let mut chain = ToolDriver::new();
        run(chain.document_mut(), "DIM ALIGNED 0,0 10,0 at=5,5");
        chain.start("annotate.dimcontinue");
        assert!(
            contains(&chain.prompt(), "[Select/Undo] <done>"),
            "{}",
            chain.prompt()
        );
        assert_eq!(
            chain.r#type("SP").outcome,
            REJECTED,
            "Continue has no spacing"
        );
        chain.click(20.0, 0.0);
        chain.click(35.0, 0.0);
        let done = chain.enter();
        assert_eq!(done.message, "2 continued dimensions");
        let mut continued = Document::default();
        run(&mut continued, "DIM ALIGNED 0,0 10,0 at=5,5");
        run(&mut continued, "DIM CONTINUE 1 20,0 35,0");
        expect_same_dimensions(chain.document(), &continued);
        let all = dimensions_of(chain.document());
        assert_eq!(all.len(), 3);
        assert_eq!(geometry_of(&all[2]).start, pt(20.0, 0.0));
        assert_eq!(geometry_of(&all[2]).measurement(), 15.0);
    }

    #[test]
    fn select_takes_another_base_and_only_a_linear_or_aligned_one() {
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "DIM LINEAR 0,0 10,0 at=5,5"); // 1
        run(driver.document_mut(), "CIRCLE 50,50 5"); // 2
        run(driver.document_mut(), "DIM RADIUS 2"); // 3, not one to chain from
        run(driver.document_mut(), "DIM ALIGNED 0,20 10,20 at=5,25"); // 4
        driver.start("annotate.dimcontinue");
        assert!(
            contains(&driver.prompt(), "(from dimension 4)"),
            "{}",
            driver.prompt()
        );
        driver.r#type("S");
        assert_eq!(driver.prompt(), "Select base dimension");
        assert_eq!(driver.expects(), ToolInput::Entity);
        assert_eq!(driver.pick(2, 55.0, 50.0).outcome, REJECTED, "a circle");
        assert_eq!(driver.pick(3, 52.0, 52.0).outcome, REJECTED, "a radius");
        assert_eq!(driver.undo().outcome, CONTINUE, "back to the base it had");
        assert!(contains(&driver.prompt(), "(from dimension 4)"));
        driver.r#type("S");
        assert_eq!(driver.pick(1, 5.0, 5.0).outcome, CONTINUE);
        assert!(contains(&driver.prompt(), "(from dimension 1)"));
        driver.click(20.0, 0.0);
        assert_eq!(
            driver.r#type("S").outcome,
            REJECTED,
            "not with a point placed"
        );
        assert_eq!(driver.enter().outcome, DONE);
        let all = dimensions_of(driver.document());
        let Some(last) = all.last() else {
            panic!("no dimension");
        };
        assert_eq!(geometry_of(last).start, pt(10.0, 0.0));
    }

    #[test]
    fn enter_after_a_chain_goes_on_from_its_last_and_enter_with_none_placed_ends() {
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "DIM LINEAR 0,0 10,0 at=5,5"); // 1
        driver.start("annotate.dimcontinue");
        driver.click(20.0, 0.0);
        assert_eq!(driver.enter().outcome, DONE); // 2
        assert!(!driver.finished());
        assert!(
            contains(&driver.prompt(), "(from dimension 2)"),
            "{}",
            driver.prompt()
        );
        driver.click(30.0, 0.0);
        assert_eq!(driver.enter().outcome, DONE); // 3, from 20,0
        let all = dimensions_of(driver.document());
        let Some(last) = all.last() else {
            panic!("no dimension");
        };
        assert_eq!(geometry_of(last).start, pt(20.0, 0.0));
        assert_eq!(driver.enter().outcome, DONE);
        assert!(driver.finished(), "Enter, Enter ends it, as in AutoCAD");
        assert_eq!(driver.executed(), 2);
    }

    #[test]
    fn with_no_dimension_the_base_is_asked_for_and_enter_ends() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimbaseline");
        assert_eq!(driver.prompt(), "Select base dimension");
        assert_eq!(driver.click(1.0, 1.0).outcome, REJECTED);
        assert_eq!(driver.undo().outcome, REJECTED);
        assert_eq!(driver.enter().outcome, DONE);
        assert!(driver.finished());
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn undo_takes_back_a_point_and_esc_keeps_those_given() {
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "DIM LINEAR 0,0 10,0 at=5,5");
        driver.start("annotate.dimbaseline");
        assert_eq!(driver.undo().outcome, REJECTED, "no origin given yet");
        driver.click(20.0, 0.0);
        driver.click(30.0, 0.0);
        driver.click(40.0, 0.0);
        assert_eq!(driver.r#type("U").outcome, CONTINUE);
        assert_eq!(driver.last_point(), Some(pt(30.0, 0.0)));
        let kept = driver.cancel();
        assert_eq!(kept.outcome, DONE);
        assert_eq!(kept.message, "2 baseline dimensions");
        assert_eq!(dimensions_of(driver.document()).len(), 3);
        assert!(driver.finished());
    }

    #[test]
    fn an_origin_that_would_measure_nothing_is_refused_when_it_is_given() {
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "DIM LINEAR 0,0 10,0 at=5,5");
        driver.start("annotate.dimcontinue");
        // From the base's end, 10,0, to 10,7 measures nothing across.
        let refused = driver.click(10.0, 7.0);
        assert_eq!(refused.outcome, REJECTED);
        assert!(!refused.message.is_empty());
        driver.click(20.0, 0.0);
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(dimensions_of(driver.document()).len(), 2);
    }

    #[test]
    fn a_snapped_origin_follows_the_entity_it_was_snapped_to() {
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "DIM LINEAR 0,0 10,0 at=5,5"); // 1
        run(driver.document_mut(), "LINE 20,0 20,-5"); // 2
        driver.start("annotate.dimcontinue");
        driver.click_snapped(20.1, 0.1); // line 2's start
        assert_eq!(driver.enter().outcome, DONE);
        let all = dimensions_of(driver.document());
        let Some(last) = all.last() else {
            panic!("no dimension");
        };
        let made = geometry_of(last).clone();
        assert_eq!(made.end_ref, anchor(2, AnchorPoint::Start, 0));
        let mut typed = Document::default();
        run(&mut typed, "DIM LINEAR 0,0 10,0 at=5,5");
        run(&mut typed, "LINE 20,0 20,-5");
        run(&mut typed, "DIM CONTINUE 1 #2.start");
        expect_same_dimensions(driver.document(), &typed);
        // The line moves 4 right: the dimension now measures 10 to 24.
        run(driver.document_mut(), "SELECT 2");
        run(driver.document_mut(), "MOVE 4,0");
        let all = dimensions_of(driver.document());
        let Some(last) = all.last() else {
            panic!("no dimension");
        };
        assert_eq!(geometry_of(last).measurement(), 14.0);
    }
}
