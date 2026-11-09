// The Label Objects tool (annotate_label.rs), driven through ToolDriver as
// the plan view drives it, and held to the LABEL verb: the same answers typed
// as one LABEL line in another drawing must make the same label. The drawings
// are made with the verbs an agent would type, so the ids are the order of
// the lines below (tests/cad/tools/test_annotate_label_tool.cpp).

use katana_cad::Document;
use katana_entity::{Entity, EntityId, Geometry, LabelGeometry};

use crate::run;

#[track_caller]
fn newest(document: &Document) -> EntityId {
    let ids = document.last_created_entities();
    assert_eq!(ids.len(), 1);
    ids.first().copied().unwrap_or(0)
}

// Every label in the drawing, in id order, with its layer.
fn labels_of(document: &Document) -> Vec<Entity> {
    let mut out = Vec::new();
    document.model().entities.for_each(|entity| {
        if matches!(entity.geometry, Geometry::Label(_)) {
            out.push(entity.clone());
        }
    });
    out
}

#[track_caller]
fn label_of(entity: &Entity) -> &LabelGeometry {
    match &entity.geometry {
        Geometry::Label(label) => label,
        other => panic!("not a label: {other:?}"),
    }
}

// The drawing both sides of a comparison start from: the standard label
// styles, then a closed 30 x 40 lot (id 1) and a line 0,50 -> 10,50 (id 2).
#[track_caller]
fn standard_drawing(document: &mut Document) {
    run(document, "LABELSTYLE DEFAULTS");
    run(document, "PLINE 0,0 30,0 30,40 0,40 CLOSE");
    run(document, "LINE 0,50 10,50");
}

fn select(document: &mut Document, ids: Vec<EntityId>) {
    document.selection_mut().set(ids);
}

mod annotate_label_tool {
    use katana_cad::interactive_tool::{ToolInput, tool_catalog};
    use katana_cad::{CommandInterpreter, Document};

    use super::{label_of, labels_of, newest, select, standard_drawing};
    use crate::tool_driver::ToolDriver;
    use crate::{CONTINUE, DONE, REJECTED, contains, ok, pt, run};

    #[test]
    fn it_is_in_the_annotate_menu_under_labels_and_a_bare_label_starts_it() {
        let catalog = tool_catalog();
        let Some(info) = catalog.find("annotate.label") else {
            panic!("no annotate.label");
        };
        assert_eq!(info.name, "Label Objects");
        assert_eq!(info.category, "Annotate");
        assert_eq!(info.group, "Labels");
        for alias in ["LABELOBJECTS", "LBL", "LABEL"] {
            let Some(by_alias) = catalog.find_by_alias(alias) else {
                panic!("{alias}");
            };
            assert_eq!(by_alias.id, "annotate.label", "{alias}");
        }
    }

    #[test]
    fn a_preselected_lot_is_labelled_where_the_text_is_put_as_the_verb_labels_it() {
        let mut driver = ToolDriver::new();
        standard_drawing(driver.document_mut());
        select(driver.document_mut(), vec![1]);
        driver.start("annotate.label");
        assert_eq!(
            driver.expects(),
            ToolInput::Point,
            "the selection is taken as given"
        );
        assert_eq!(
            driver.prompt(),
            "Specify text location or [Style/Part/Text/Undo] <automatic>"
        );
        assert_eq!(driver.r#type("S").outcome, CONTINUE);
        assert_eq!(driver.expects(), ToolInput::Value);
        assert_eq!(driver.r#type("Lot Area").outcome, CONTINUE);
        let done = driver.click(10.0, 12.0);
        assert_eq!(done.outcome, DONE, "{}", done.message);
        assert_eq!(done.message, "1 label in style Lot Area");
        assert_eq!(driver.executed(), 1);

        let mut typed = Document::default();
        standard_drawing(&mut typed);
        run(&mut typed, "LABEL 1 style=\"Lot Area\" at=10,12");
        let made = labels_of(driver.document());
        let expected = labels_of(&typed);
        assert_eq!(made.len(), 1);
        assert_eq!(expected.len(), 1);
        assert_eq!(made[0].geometry, expected[0].geometry);
        assert_eq!(made[0].layer, expected[0].layer);
        let label = label_of(&made[0]);
        assert_eq!(label.target, 1);
        assert_eq!(label.position, Some(pt(10.0, 12.0)));

        // One undo takes it back.
        ok(driver.document_mut().undo());
        assert!(labels_of(driver.document()).is_empty());
    }

    #[test]
    fn with_none_used_yet_the_first_style_by_name_that_can_label_the_object_is_offered() {
        // The standard styles by name: Arc Data (arcs), then Bearing Distance
        // (segments) - the first that can label a line.
        let mut driver = ToolDriver::new();
        standard_drawing(driver.document_mut());
        select(driver.document_mut(), vec![2]);
        driver.start("annotate.label");
        driver.r#type("Style");
        assert_eq!(driver.prompt(), "Enter label style name <Bearing Distance>");
        assert_eq!(
            driver.enter().outcome,
            CONTINUE,
            "Enter keeps the one offered"
        );
        let done = driver.enter();
        assert_eq!(done.outcome, DONE, "{}", done.message);
        assert_eq!(done.message, "1 label in style Bearing Distance");
        let made = labels_of(driver.document());
        assert_eq!(made.len(), 1);
        let label = label_of(&made[0]);
        assert!(
            label.position.is_none(),
            "Enter leaves the text to the placer"
        );
        assert_eq!(label.style, "Bearing Distance");
    }

    #[test]
    fn the_style_offered_next_is_the_newest_hand_placed_labels_when_it_fits() {
        let mut driver = ToolDriver::new();
        standard_drawing(driver.document_mut());
        run(driver.document_mut(), "PLINE 40,0 60,0 60,20 CLOSE"); // id 3
        run(driver.document_mut(), "LABEL 1 style=\"Lot Area\"");
        select(driver.document_mut(), vec![3]);
        driver.start("annotate.label");
        driver.r#type("S");
        assert_eq!(driver.prompt(), "Enter label style name <Lot Area>");
        driver.enter();
        assert_eq!(driver.enter().outcome, DONE);

        // A line cannot wear an area style: the first that can label it instead.
        select(driver.document_mut(), vec![2]);
        driver.start("annotate.label");
        driver.r#type("S");
        assert_eq!(driver.prompt(), "Enter label style name <Bearing Distance>");
    }

    #[test]
    fn several_objects_are_one_undo_step_and_share_no_place() {
        let mut driver = ToolDriver::new();
        standard_drawing(driver.document_mut());
        run(driver.document_mut(), "LINE 0,60 20,60"); // id 3
        select(driver.document_mut(), Vec::new());
        driver.start("annotate.label");
        assert_eq!(driver.expects(), ToolInput::Selection);
        assert_eq!(
            driver.prompt(),
            "Select objects to label or [Style/Part/Text], then press Enter"
        );
        assert_eq!(
            driver.click(1.0, 1.0).outcome,
            REJECTED,
            "the objects come first"
        );
        driver.pick(2, 5.0, 50.0);
        assert_eq!(
            driver.pick(2, 5.0, 50.0).outcome,
            REJECTED,
            "the same line twice"
        );
        driver.pick(3, 5.0, 60.0);
        let chosen = driver.enter();
        assert_eq!(chosen.outcome, CONTINUE);
        assert_eq!(
            chosen.message,
            "2 objects to label in style Bearing Distance"
        );
        assert!(contains(&driver.prompt(), "label the 2 objects"));
        let shared = driver.click(5.0, 70.0);
        assert_eq!(shared.outcome, REJECTED);
        assert!(
            contains(&shared.message, "cannot share one place"),
            "{}",
            shared.message
        );
        let done = driver.enter();
        assert_eq!(done.outcome, DONE, "{}", done.message);
        assert_eq!(done.message, "2 labels in style Bearing Distance");
        assert_eq!(driver.executed(), 1);
        assert_eq!(labels_of(driver.document()).len(), 2);
        ok(driver.document_mut().undo());
        assert!(
            labels_of(driver.document()).is_empty(),
            "one step took back both"
        );
    }

    #[test]
    fn part_and_text_are_the_verbs_part_and_text() {
        let mut driver = ToolDriver::new();
        standard_drawing(driver.document_mut());
        select(driver.document_mut(), vec![1]);
        driver.start("annotate.label");
        driver.r#type("S");
        driver.r#type("Bearing Distance");
        driver.r#type("P");
        assert_eq!(
            driver.prompt(),
            "Enter the polyline segment to label, 0 for the first, or [All] <all>"
        );
        assert_eq!(driver.r#type("-1").outcome, REJECTED);
        assert_eq!(driver.r#type("1.5").outcome, REJECTED);
        assert_eq!(driver.r#type("1").outcome, CONTINUE);
        driver.r#type("T");
        assert_eq!(
            driver.r#type("KERB, 150").outcome,
            CONTINUE,
            "a comma is text here"
        );
        assert_eq!(driver.click(35.0, 20.0).outcome, DONE);

        let mut typed = Document::default();
        standard_drawing(&mut typed);
        run(
            &mut typed,
            "LABEL 1 style=\"Bearing Distance\" part=1 text=\"KERB, 150\" at=35,20",
        );
        let made = labels_of(driver.document());
        let expected = labels_of(&typed);
        assert_eq!(made.len(), 1);
        assert_eq!(expected.len(), 1);
        assert_eq!(made[0].geometry, expected[0].geometry);
        let label = label_of(&made[0]);
        assert_eq!(label.part, 1);
        assert_eq!(label.text_override, "KERB, 150");
    }

    #[test]
    fn a_style_that_cannot_label_the_object_is_refused_when_it_is_named() {
        let mut driver = ToolDriver::new();
        standard_drawing(driver.document_mut());
        select(driver.document_mut(), vec![2]);
        driver.start("annotate.label");
        driver.r#type("S");
        let wrong_kind = driver.r#type("Point Number");
        assert_eq!(wrong_kind.outcome, REJECTED);
        // In the words LABEL refuses it with, and naming the object as LABEL does.
        let typed = CommandInterpreter::new()
            .run(driver.document_mut(), "LABEL 2 style=\"Point Number\"")
            .expect_err("LABEL refuses a style that cannot label a line");
        assert_eq!(wrong_kind.message, format!("{} (id=2)", typed.message));
        assert!(contains(&typed.context, "id=2"), "{}", typed.context);
        let missing = driver.r#type("Nowhere");
        assert_eq!(missing.outcome, REJECTED);
        assert!(
            contains(
                &missing.message,
                "there is no label style \"Nowhere\"; the drawing has Arc Data, Bearing Distance"
            ),
            "{}",
            missing.message
        );
        // Another case of a name the drawing has is that style.
        assert_eq!(driver.r#type("bearing distance").outcome, CONTINUE);
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(
            label_of(&labels_of(driver.document())[0]).style,
            "Bearing Distance"
        );
    }

    #[test]
    fn undo_takes_back_the_options_then_the_choice_then_the_picks() {
        let mut driver = ToolDriver::new();
        standard_drawing(driver.document_mut());
        select(driver.document_mut(), Vec::new());
        driver.start("annotate.label");
        assert_eq!(driver.undo().outcome, REJECTED, "nothing picked yet");
        driver.pick(2, 5.0, 50.0);
        driver.enter();
        driver.r#type("T");
        driver.r#type("EDGE");
        assert_eq!(driver.undo().outcome, CONTINUE);
        assert_eq!(
            driver.expects(),
            ToolInput::Point,
            "the text is taken back first"
        );
        assert_eq!(driver.undo().outcome, CONTINUE);
        assert_eq!(driver.expects(), ToolInput::Selection, "then the choice");
        assert_eq!(driver.undo().outcome, CONTINUE, "then the pick");
        assert_eq!(driver.undo().outcome, REJECTED);
        // The text taken back is not on the label made after.
        driver.pick(2, 5.0, 50.0);
        driver.enter();
        assert_eq!(driver.enter().outcome, DONE);
        assert!(
            label_of(&labels_of(driver.document())[0])
                .text_override
                .is_empty()
        );
    }

    #[test]
    fn after_labelling_the_selection_is_cleared_and_enter_with_nothing_chosen_ends_it() {
        let mut driver = ToolDriver::new();
        standard_drawing(driver.document_mut());
        select(driver.document_mut(), vec![2]);
        driver.start("annotate.label");
        assert_eq!(driver.enter().outcome, DONE);
        assert!(driver.document().selection().ids().is_empty());
        assert!(!driver.finished(), "it starts again");
        assert_eq!(
            driver.expects(),
            ToolInput::Selection,
            "asking for the next objects, not labelling the same one again"
        );
        let end = driver.enter();
        assert_eq!(end.outcome, DONE);
        assert!(driver.finished());
        assert_eq!(labels_of(driver.document()).len(), 1);
    }

    #[test]
    fn a_label_that_could_not_be_made_is_refused_with_the_verbs_reason_and_the_tool_waits() {
        // No label styles at all.
        let mut bare = ToolDriver::new();
        run(bare.document_mut(), "LINE 0,0 10,0");
        select(bare.document_mut(), vec![1]);
        bare.start("annotate.label");
        let none = bare.enter();
        assert_eq!(none.outcome, REJECTED);
        assert!(
            contains(&none.message, "no label style that can label this"),
            "{}",
            none.message
        );

        // A point with no number: Point Number would say nothing, as LABEL says.
        let mut driver = ToolDriver::new();
        standard_drawing(driver.document_mut());
        run(driver.document_mut(), "POINT 5,5"); // id 3
        select(driver.document_mut(), vec![3]);
        driver.start("annotate.label");
        driver.r#type("S");
        driver.r#type("Point Number");
        let prompt = driver.prompt();
        let silent = driver.enter();
        assert_eq!(silent.outcome, REJECTED);
        assert!(
            contains(&silent.message, "the label would say nothing"),
            "{}",
            silent.message
        );
        assert_eq!(driver.prompt(), prompt);
        assert_eq!(driver.executed(), 0);
        assert_eq!(
            newest(driver.document()),
            3,
            "nothing was made after the point"
        );
    }
}
