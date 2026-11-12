//! The twins of tests/qt_widgets/annotation/test_leader_manager.cpp: the Leaders manager driven by
//! object name as a person clicks it - the form shows a leader and what the entity it is on
//! offers, a template is checked and previewed as it is typed, and every button is one undo step,
//! the same edit the LEADER verbs make.
//!
//! The suite's TheAnnotateMenuOpensItOnEachTabWithLettersOfItsOwn installs the annotation
//! workbench and is V3's.

use katana_cad::annotation::leader_edit::{LeaderNote, LeaderNoteKind};
use katana_commands::{create_entities, move_entities};
use katana_entity::anchor::resolve_anchor;
use katana_entity::leader_values::{is_smart, leader_note};
use katana_entity::{
    AnchorPoint, AnchorRef, ArrowHead, CalloutShape, Entity, EntityId, Geometry, LeaderContext,
    LeaderGeometry, PropertyValue,
};
use katana_geometry::{Point2, Vec2};
use katana_window::annotation::leader_manager::{LeaderManagerContext, LeaderManagerDialog, Tab};
use katana_window::host::RecordingHost;

use crate::support::{Bench, interpreter_host, leader, run};
use crate::window_support::Scratch;

/// What the note-kind boxes show for Text, Template and a label style's.
const TEXT: &str = "Text";
const TEMPLATE: &str = "Template (read off the entity)";
const LABEL_STYLE: &str = "Label style's template";
/// The text style boxes' first entry: the default text style (data "").
const DEFAULT_STYLE: &str = "(default: Standard)";

fn manager(host: RecordingHost) -> Bench<LeaderManagerDialog> {
    Bench::new(host, |document| {
        LeaderManagerDialog::new(document, LeaderManagerContext::default())
    })
}

/// What leader `id` says (entity's leader_note, as the C++ test reads it).
fn says(bench: &Bench<LeaderManagerDialog>, id: EntityId) -> String {
    leader_note(
        bench.document().model(),
        &leader(&bench.host, id),
        &[],
        &LeaderContext::default(),
    )
}

fn show(bench: &mut Bench<LeaderManagerDialog>, id: EntityId) -> bool {
    let shown = bench.dialog.show_leader(bench.host.document(), id);
    bench.draw();
    shown
}

fn number(bench: &Bench<LeaderManagerDialog>, name: &str) -> f64 {
    let shown = bench.value(name);
    shown
        .parse()
        .unwrap_or_else(|_| panic!("{name} shows {shown}"))
}

/// The leader's own change, empty or not.
fn untouched(bench: &Bench<LeaderManagerDialog>) -> bool {
    bench
        .dialog
        .form_change(bench.host.document())
        .expect("the form reads")
        .empty()
}

/// The drawing the look-lending cases share: a point with an invert, a text style, and label
/// styles lending all of a look (Invert), none of it (Plain) and a height finer than the form's 3
/// decimals (Fine). The point's id.
fn styled(host: &mut RecordingHost) -> EntityId {
    let pit = run(host, "POINT 0,0");
    run(host, &format!("SELECT {pit}"));
    run(host, "PROP SET invert 10.5");
    run(host, "TEXTSTYLE NEW Notes paper=3.5");
    run(
        host,
        "LABELSTYLE NEW Invert kind=point text=\"INV {prop.invert:.2f}\" textstyle=Notes paper=3",
    );
    run(
        host,
        "LABELSTYLE NEW Plain kind=point text=\"P {prop.invert:.1f}\"",
    );
    run(
        host,
        "LABELSTYLE NEW Fine kind=point text=\"F {id}\" paper=2.3456",
    );
    pit
}

/// A plain leader on the point, 2 mm high in the default text style.
fn plain_leader(bench: &mut Bench<LeaderManagerDialog>, pit: EntityId, hang: &str) -> EntityId {
    bench.run(&format!("LEADER #{pit} {hang} text=X paper=2"))
}

fn choose_style(bench: &mut Bench<LeaderManagerDialog>, name: &str) {
    bench.fill("leaderNoteKind", LABEL_STYLE);
    bench.fill("leaderLabelStyle", name);
}

mod leader_manager {
    use super::*;

    #[test]
    fn the_form_shows_the_leader_what_it_says_and_what_its_entity_offers() {
        let mut host = interpreter_host();
        let pit = run(&mut host, "POINT 10,10");
        run(&mut host, &format!("SELECT {pit}"));
        run(&mut host, "PROP SET invert 10.5");
        run(&mut host, "PROP SET elevation 12.25");
        let id = run(
            &mut host,
            &format!("LEADER #{pit} 20,20 template=\"IL {{prop.invert:.2f}}\""),
        );
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        assert_eq!(bench.dialog.current_leader(), id);
        assert_eq!(bench.value("leaderNoteKind"), TEMPLATE);
        assert_eq!(bench.value("leaderNote"), "IL {prop.invert:.2f}");
        assert!(
            bench.report("leaderTarget").starts_with("Point "),
            "{}",
            bench.report("leaderTarget")
        );
        assert_eq!(bench.dialog.preview(), "IL 10.50");
        let rows = bench.value_rows("leaderValues");
        assert!(rows.iter().any(|row| row == "rl=12.250"), "{rows:?}");
        assert!(
            rows.iter().any(|row| row == "prop.invert=10.500"),
            "{rows:?}"
        );
        // The list names every leader by what it says.
        assert_eq!(bench.rows("leaderList"), 1);
        assert!(
            bench.row("leaderList", 0)[0].ends_with("IL 10.50"),
            "{:?}",
            bench.row("leaderList", 0)
        );
        assert!(!show(&mut bench, pit), "a point is no leader");
    }

    #[test]
    fn a_template_is_checked_as_it_is_typed_and_apply_is_one_step() {
        let mut host = interpreter_host();
        let pit = run(&mut host, "POINT 0,0");
        run(&mut host, &format!("SELECT {pit}"));
        run(&mut host, "PROP SET invert 10.5");
        let id = run(&mut host, &format!("LEADER #{pit} 5,5 text=PIT"));
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        bench.fill("leaderNoteKind", TEMPLATE);
        bench.fill("leaderNote", "IL {invrt}");
        assert!(
            bench.dialog.preview().contains("no value 'invrt'"),
            "{}",
            bench.dialog.preview()
        );
        let before = bench.steps();
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(
            bench.dialog.problem().contains("invrt"),
            "said in the dialog"
        );
        assert_eq!(bench.steps(), before);

        bench.fill("leaderNote", "PIT {id}\\nIL {prop.invert:.3f}");
        assert_eq!(bench.dialog.preview(), format!("PIT {pit}\nIL 10.500"));
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 1);
        assert!(leader(&bench.host, id).fields);
        assert_eq!(says(&bench, id), format!("PIT {pit}\nIL 10.500"));
        bench.draw();
        assert!(bench.dialog.apply(&mut bench.host));
        assert_eq!(bench.steps(), before + 1, "an unchanged form is no step");

        // The look, one more step.
        bench.fill("leaderCallout", "box");
        bench.fill("leaderPaperHeight", "3.5");
        assert!(bench.dialog.apply(&mut bench.host));
        assert_eq!(leader(&bench.host, id).callout, CalloutShape::Box);
        assert_eq!(leader(&bench.host, id).paper_height, 3.5);
        assert_eq!(bench.steps(), before + 2);
    }

    #[test]
    fn a_double_clicked_value_makes_the_note_a_template() {
        let mut host = interpreter_host();
        let line = run(&mut host, "LINE 0,0 40,0");
        let id = run(&mut host, &format!("LEADER #{line}@10,1 20,10 text=\"L=\""));
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        let rows = bench.value_rows("leaderValues");
        assert!(
            rows.iter().any(|row| row.starts_with("length=")),
            "{rows:?}"
        );
        // The cursor left where the form put it: the value goes at the END of the note, after
        // what it already says. What the table's double-click calls:
        bench.dialog.insert_value(bench.host.document(), "length");
        bench.draw();
        assert_eq!(bench.value("leaderNoteKind"), TEMPLATE);
        assert_eq!(bench.value("leaderNote"), "L={length}");
        assert_eq!(bench.dialog.preview(), "L=40.000");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(says(&bench, id), "L=40.000");
    }

    #[test]
    fn an_untouched_form_is_no_step_and_what_its_widgets_cannot_hold_is_kept() {
        let mut host = interpreter_host();
        let line = run(&mut host, "LINE 0,0 40,0");
        // Values the form's widgets cannot hold exactly: a height and an arrow finer than their 3
        // decimals, a landing past their largest size, a place along finer than the percentage's
        // decimals, and a non-breaking space.
        let reference = AnchorRef {
            entity: line,
            point: AnchorPoint::Along,
            index: 0,
            parameter: 0.123456,
        };
        let tip = {
            let on = host
                .document()
                .model()
                .entities
                .find(line)
                .expect("the line is there");
            resolve_anchor(on, &reference).expect("the place resolves")
        };
        let entity = Entity {
            geometry: Geometry::Leader(LeaderGeometry {
                vertices: vec![tip, Point2::new(20.0, 10.0)],
                text: String::from("A\u{a0}B"),
                paper_height: 2.3456,
                arrow_size: 0.12345,
                landing: 1500.0,
                tip_ref: reference,
                ..LeaderGeometry::default()
            }),
            ..Entity::default()
        };
        host.document_mut()
            .execute(create_entities(vec![entity]))
            .expect("the leader is made");
        let id = host.document().last_created_entities()[0];
        let drawn = leader(&host, id);
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        assert!(untouched(&bench));
        let before = bench.steps();
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            bench.steps(),
            before,
            "an untouched form applied is no step"
        );
        assert_eq!(leader(&bench.host, id), drawn);

        // One field changed is the one sent; everything else stays exactly.
        bench.fill("leaderCallout", "box");
        let change = bench
            .dialog
            .form_change(bench.host.document())
            .expect("the form reads");
        assert!(change.callout.is_some());
        assert!(
            change.note.is_none()
                && change.arrow.is_none()
                && change.text_style.is_none()
                && change.paper_height.is_none()
                && change.arrow_size.is_none()
                && change.landing.is_none()
                && change.tip.is_none()
                && change.hang.is_none()
                && change.vertex.is_none()
                && change.landing_angle.is_none()
        );
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 1);
        let mut expected = drawn.clone();
        expected.callout = CalloutShape::Box;
        assert_eq!(leader(&bench.host, id), expected);

        // Typed on, the note keeps its non-breaking space.
        bench.draw();
        bench.fill("leaderNote", "A\u{a0}BC");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, id).text, "A\u{a0}BC");
        assert_eq!(leader(&bench.host, id).paper_height, 2.3456);
        assert_eq!(leader(&bench.host, id).landing, 1500.0);
    }

    #[test]
    fn a_note_made_a_styles_and_back_keeps_the_users_words_and_look() {
        let mut host = interpreter_host();
        let pit = run(&mut host, "POINT 0,0");
        run(&mut host, &format!("SELECT {pit}"));
        run(&mut host, "PROP SET invert 10.5");
        run(&mut host, "TEXTSTYLE NEW Notes paper=3.5");
        run(
            &mut host,
            "LABELSTYLE NEW Invert kind=point text=\"INV {prop.invert:.2f}\" textstyle=Notes \
             paper=3",
        );
        let id = run(&mut host, &format!("LEADER #{pit} 5,5 text=X paper=2"));
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        bench.fill("leaderNote", "MY WORDS");

        bench.fill("leaderNoteKind", LABEL_STYLE);
        assert_eq!(
            bench.value("leaderNote"),
            "INV {prop.invert:.2f}",
            "the style's, to read"
        );
        assert_eq!(bench.value("leaderTextStyle"), "Notes", "lent by the style");
        assert_eq!(number(&bench, "leaderPaperHeight"), 3.0);
        assert_eq!(bench.dialog.preview(), "INV 10.50");

        bench.fill("leaderNoteKind", TEXT);
        assert_eq!(
            bench.value("leaderNote"),
            "MY WORDS",
            "the user's words back"
        );
        assert_eq!(
            bench.value("leaderTextStyle"),
            DEFAULT_STYLE,
            "and the look before"
        );
        assert_eq!(number(&bench, "leaderPaperHeight"), 2.0);
        let change = bench
            .dialog
            .form_change(bench.host.document())
            .expect("the form reads");
        assert_eq!(
            change.note,
            Some(LeaderNote {
                kind: LeaderNoteKind::Text,
                text: String::from("MY WORDS"),
            })
        );
        assert!(
            change.text_style.is_none() && change.paper_height.is_none(),
            "the look is as it was"
        );
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, id).text, "MY WORDS");
        assert_eq!(leader(&bench.host, id).style, "");
        assert_eq!(leader(&bench.host, id).paper_height, 2.0);
    }

    #[test]
    fn a_look_set_back_over_a_styles_is_kept_and_another_style_lends_over_the_one_before() {
        let mut host = interpreter_host();
        let pit = styled(&mut host);
        let mut bench = manager(host);
        let kept = plain_leader(&mut bench, pit, "5,5");

        // The style's words, the leader's own look: set back over the lent one, it is what Apply
        // gives, though the style would lend its own.
        assert!(show(&mut bench, kept));
        choose_style(&mut bench, "Invert");
        assert_eq!(bench.value("leaderTextStyle"), "Notes");
        assert_eq!(number(&bench, "leaderPaperHeight"), 3.0);
        bench.fill("leaderTextStyle", DEFAULT_STYLE);
        bench.fill("leaderPaperHeight", "2");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, kept).label_style, "Invert");
        assert_eq!(leader(&bench.host, kept).style, "");
        assert_eq!(leader(&bench.host, kept).paper_height, 2.0);

        // A style lending nothing, chosen after one lending all: the look from before either, as
        // LEADER SET labelstyle=Plain gives.
        let other = plain_leader(&mut bench, pit, "5,-5");
        let typed = plain_leader(&mut bench, pit, "-5,-5");
        assert!(show(&mut bench, other));
        choose_style(&mut bench, "Invert");
        choose_style(&mut bench, "Plain");
        assert_eq!(bench.value("leaderTextStyle"), DEFAULT_STYLE);
        assert_eq!(number(&bench, "leaderPaperHeight"), 2.0);
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        bench.run(&format!("LEADER SET {typed} labelstyle=Plain"));
        assert_eq!(leader(&bench.host, other).label_style, "Plain");
        assert_eq!(
            leader(&bench.host, other).style,
            leader(&bench.host, typed).style
        );
        assert_eq!(
            leader(&bench.host, other).paper_height,
            leader(&bench.host, typed).paper_height
        );
        assert_eq!(leader(&bench.host, other).paper_height, 2.0);
    }

    #[test]
    fn a_look_typed_after_a_lend_is_kept_and_a_styles_height_is_lent_exactly() {
        let mut host = interpreter_host();
        let pit = styled(&mut host);
        let mut bench = manager(host);
        let typed = plain_leader(&mut bench, pit, "5,5");
        let fine = plain_leader(&mut bench, pit, "5,-5");

        // Back from the style, only what still shows the style's is put back.
        assert!(show(&mut bench, typed));
        choose_style(&mut bench, "Invert");
        bench.fill("leaderPaperHeight", "5");
        bench.fill("leaderNoteKind", TEXT);
        assert_eq!(
            bench.value("leaderTextStyle"),
            DEFAULT_STYLE,
            "lent, so put back"
        );
        assert_eq!(
            number(&bench, "leaderPaperHeight"),
            5.0,
            "the user's own since the lend"
        );
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, typed).text, "X");
        assert_eq!(leader(&bench.host, typed).style, "");
        assert_eq!(leader(&bench.host, typed).paper_height, 5.0);

        // A height the box shows to 3 decimals is lent whole, as labelstyle= does.
        assert!(show(&mut bench, fine));
        choose_style(&mut bench, "Fine");
        assert_eq!(number(&bench, "leaderPaperHeight"), 2.346);
        let change = bench
            .dialog
            .form_change(bench.host.document())
            .expect("the form reads");
        assert!(
            change.paper_height.is_none(),
            "the style's own: left to the style"
        );
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, fine).paper_height, 2.3456);
    }

    #[test]
    fn a_style_that_comes_or_goes_under_the_form_lends_its_look() {
        let mut host = interpreter_host();
        let pit = run(&mut host, "POINT 0,0");
        run(&mut host, &format!("SELECT {pit}"));
        run(&mut host, "PROP SET invert 10.5");
        run(&mut host, "TEXTSTYLE NEW Notes paper=3.5");
        let id = run(&mut host, &format!("LEADER #{pit} 5,5 text=X paper=2"));
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        bench.fill("leaderNoteKind", LABEL_STYLE);
        assert!(bench.control("leaderLabelStyle").items.is_empty());
        assert!(
            !bench.dialog.apply(&mut bench.host),
            "there is no style to be"
        );

        // The first style made while the form waits for one: named, and lending.
        bench.run(
            "LABELSTYLE NEW Zed kind=point text=\"Z {prop.invert:.1f}\" textstyle=Notes paper=3",
        );
        bench.draw();
        assert_eq!(bench.value("leaderLabelStyle"), "Zed");
        assert_eq!(bench.value("leaderTextStyle"), "Notes");
        assert_eq!(number(&bench, "leaderPaperHeight"), 3.0);
        assert_eq!(bench.dialog.preview(), "Z 10.5");

        // Another made leaves the choice; the chosen one deleted moves it, and what that one lent
        // goes with it.
        bench.run("LABELSTYLE NEW Alpha kind=point text=\"A {prop.invert:.1f}\"");
        bench.draw();
        assert_eq!(bench.value("leaderLabelStyle"), "Zed");
        bench.run("LABELSTYLE DELETE Zed");
        bench.draw();
        assert_eq!(bench.value("leaderLabelStyle"), "Alpha");
        assert_eq!(bench.value("leaderTextStyle"), DEFAULT_STYLE);
        assert_eq!(number(&bench, "leaderPaperHeight"), 2.0);
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, id).label_style, "Alpha");
        assert_eq!(leader(&bench.host, id).style, "");
        assert_eq!(leader(&bench.host, id).paper_height, 2.0);
    }

    #[test]
    fn for_selection_shows_the_look_a_style_lends_and_makes_what_it_shows() {
        let mut host = interpreter_host();
        let pit = styled(&mut host);
        let other = run(&mut host, "POINT 20,0");
        run(&mut host, &format!("SELECT {other}"));
        run(&mut host, "PROP SET invert 9.25");
        run(&mut host, &format!("SELECT {pit}"));
        let mut bench = manager(host);
        bench.dialog.show_tab(Tab::ForSelection);
        bench.draw();
        bench.fill("leaderForNoteKind", LABEL_STYLE);
        bench.fill("leaderForLabelStyle", "Fine");
        assert_eq!(
            number(&bench, "leaderForPaperHeight"),
            2.346,
            "Fine's height, to the box's decimals"
        );
        bench.fill("leaderForLabelStyle", "Invert");
        assert_eq!(bench.value("leaderForTextStyle"), "Notes");
        assert_eq!(number(&bench, "leaderForPaperHeight"), 3.0);
        assert_eq!(bench.dialog.for_preview(), "INV 10.50");
        assert!(
            bench.dialog.make_for_selection(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        let lent = bench.document().last_created_entities()[0];
        assert_eq!(leader(&bench.host, lent).style, "Notes");
        assert_eq!(leader(&bench.host, lent).paper_height, 3.0);

        // Set to the default face and the text style's height, that is what the leader gets -
        // not the style's, lent over what the boxes show.
        bench.fill("leaderForTextStyle", DEFAULT_STYLE);
        bench.fill("leaderForPaperHeight", "0");
        bench.run(&format!("SELECT {other}"));
        bench.draw();
        assert!(
            bench.dialog.make_for_selection(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        let own = bench.document().last_created_entities()[0];
        assert_eq!(leader(&bench.host, own).label_style, "Invert");
        assert_eq!(leader(&bench.host, own).style, "");
        assert_eq!(leader(&bench.host, own).paper_height, 0.0);

        // Lent again, then the note a template: the look before comes back.
        bench.fill("leaderForLabelStyle", "Plain");
        bench.fill("leaderForLabelStyle", "Invert");
        assert_eq!(bench.value("leaderForTextStyle"), "Notes");
        bench.fill("leaderForNoteKind", TEMPLATE);
        assert_eq!(bench.value("leaderForTextStyle"), DEFAULT_STYLE);
        assert_eq!(number(&bench, "leaderForPaperHeight"), 0.0);
    }

    #[test]
    fn a_numbered_balloon_must_stay_a_circle() {
        let mut host = interpreter_host();
        let a = run(&mut host, "POINT 0,0");
        run(&mut host, &format!("SELECT {a}"));
        let mut bench = manager(host);
        bench.dialog.show_tab(Tab::ForSelection);
        bench.draw();
        bench.fill("leaderForBalloon", "on");
        bench.fill("leaderForCallout", "box");
        // A box's number would not be counted, and so be given again.
        assert!(
            bench
                .dialog
                .for_preview()
                .starts_with("a numbered balloon is a circle"),
            "{}",
            bench.dialog.for_preview()
        );
        let before = bench.steps();
        assert!(!bench.dialog.make_for_selection(&mut bench.host));
        // In the verb's own words: the dialog only hands the line over.
        assert!(
            bench
                .dialog
                .problem()
                .contains("a numbered balloon is a circle"),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before);
        // With a note it is not numbered, and may be a box.
        bench.fill("leaderForNote", "A");
        assert!(
            bench.dialog.make_for_selection(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        let made = bench.document().last_created_entities()[0];
        assert_eq!(leader(&bench.host, made).callout, CalloutShape::Box);
    }

    #[test]
    fn a_styles_note_is_read_afresh_when_the_style_changes() {
        let mut host = interpreter_host();
        let pit = run(&mut host, "POINT 0,0");
        run(&mut host, &format!("SELECT {pit}"));
        run(&mut host, "PROP SET invert 10.5");
        run(
            &mut host,
            "LABELSTYLE NEW Invert kind=point text=\"INV {prop.invert:.2f}\"",
        );
        let id = run(&mut host, &format!("LEADER #{pit} 5,5 labelstyle=Invert"));
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        assert!(bench.dialog.note_is_read_only());
        assert_eq!(bench.value("leaderNote"), "INV {prop.invert:.2f}");
        assert_eq!(bench.dialog.preview(), "INV 10.50");
        // The leader is as it was; its style is not.
        bench.run("LABELSTYLE SET Invert text=\"IL {prop.invert:.3f}\"");
        bench.draw();
        assert_eq!(bench.value("leaderNote"), "IL {prop.invert:.3f}");
        assert_eq!(bench.dialog.preview(), "IL 10.500");
        assert!(untouched(&bench), "nothing of the leader's changed");
    }

    #[test]
    fn the_list_chooses_and_freeze_waits_for_the_form_to_be_applied() {
        let mut host = interpreter_host();
        let pit = run(&mut host, "POINT 0,0");
        let first = run(&mut host, "LEADER 20,0 25,5 text=ONE");
        let second = run(
            &mut host,
            &format!("LEADER #{pit} 5,5 template=\"PIT {{id}}\""),
        );
        let mut bench = manager(host);
        assert_eq!(
            bench.dialog.current_leader(),
            first,
            "the first there is, with nothing selected"
        );
        assert_eq!(bench.rows("leaderList"), 2);
        let row = bench.row("leaderList", 1)[0].clone();
        bench.fill("leaderList", &row);
        assert_eq!(bench.dialog.current_leader(), second);
        assert!(bench.enabled("leaderFreeze"));
        assert!(bench.enabled("leaderDetach"));

        // Freezing keeps what the leader says now, not the form's unapplied note: while there is
        // one, it waits.
        bench.fill("leaderNote", "PIT {id}!");
        assert!(!bench.enabled("leaderFreeze"));
        assert!(!bench.enabled("leaderDetach"));
        assert!(
            bench
                .dialog
                .button_tip("leaderFreeze")
                .is_some_and(|tip| tip.contains("not applied"))
        );
        let before = bench.steps();
        assert!(!bench.dialog.freeze(&mut bench.host));
        assert!(!bench.dialog.detach(&mut bench.host));
        assert!(
            bench.dialog.problem().contains("not applied"),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before);
        assert!(is_smart(&leader(&bench.host, second)));

        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        bench.draw();
        assert!(bench.enabled("leaderFreeze"));
        assert!(bench.dialog.freeze(&mut bench.host));
        assert_eq!(bench.steps(), before + 2);
        assert_eq!(leader(&bench.host, second).text, format!("PIT {pit}!"));
        assert!(!is_smart(&leader(&bench.host, second)));
    }

    #[test]
    fn several_selected_leaders_are_changed_together_in_one_step() {
        let mut host = interpreter_host();
        let pit = run(&mut host, "POINT 0,0");
        let a = run(
            &mut host,
            &format!("LEADER #{pit} 5,5 template=\"A {{id}}\""),
        );
        let b = run(
            &mut host,
            &format!("LEADER #{pit} 5,-5 template=\"B {{id}}\""),
        );
        let c = run(&mut host, "LEADER 30,0 35,5 text=C");
        run(&mut host, &format!("SELECT {a} {b}"));
        let mut bench = manager(host);
        assert_eq!(bench.dialog.current_leader(), a);
        assert_eq!(bench.dialog.targets(bench.host.document()), [a, b]);
        let scope = bench.control("leaderScope");
        assert!(scope.visible);
        assert!(scope.text.contains("2 selected leaders"), "{}", scope.text);

        bench.fill("leaderCallout", "box");
        let before = bench.steps();
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 1);
        assert_eq!(leader(&bench.host, a).callout, CalloutShape::Box);
        assert_eq!(leader(&bench.host, b).callout, CalloutShape::Box);
        assert_eq!(
            leader(&bench.host, c).callout,
            CalloutShape::None,
            "not selected"
        );
        assert_eq!(
            leader(&bench.host, b).text,
            "B {id}",
            "only what was changed is sent"
        );

        bench.draw();
        assert!(
            bench.dialog.freeze(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 2);
        assert_eq!(leader(&bench.host, a).text, format!("A {pit}"));
        assert_eq!(leader(&bench.host, b).text, format!("B {pit}"));
        bench.draw();
        assert!(
            bench.dialog.detach(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 3);
        assert!(!leader(&bench.host, a).tip_ref.associated());
        assert!(!leader(&bench.host, b).tip_ref.associated());

        // One selected: the form's leader alone.
        bench.run(&format!("SELECT {b}"));
        bench.draw();
        assert_eq!(bench.dialog.targets(bench.host.document()), [b]);
        assert!(!bench.control("leaderScope").visible);
    }

    #[test]
    fn attach_to_selected_puts_the_tip_on_the_entity_and_along_moves_it() {
        let mut host = interpreter_host();
        // Made first, so selected it comes before the line: it offers no place for a tip and is
        // passed over.
        let dim = run(&mut host, "DIM 0,20 10,20 2");
        let line = run(&mut host, "LINE 0,0 40,0");
        let id = run(&mut host, "LEADER 10,1 20,10 text=X");
        let mut bench = manager(host);
        assert!(
            !bench.enabled("leaderAttachSelected"),
            "nothing else is selected"
        );
        assert!(
            !bench.enabled("leaderAlong"),
            "a plain leader is along nothing"
        );
        bench.run(&format!("SELECT {dim} {id} {line}"));
        bench.draw();
        assert_eq!(bench.dialog.current_leader(), id);
        assert!(bench.enabled("leaderAttachSelected"));

        // Attaching acts on the leader as drawn: with the form changed, it waits.
        bench.fill("leaderNote", "X!");
        assert!(!bench.enabled("leaderAttachSelected"));
        assert!(
            bench
                .dialog
                .button_tip("leaderAttachSelected")
                .is_some_and(|tip| tip.contains("not applied"))
        );
        let before = bench.steps();
        assert!(!bench.dialog.attach_to_selected(&mut bench.host));
        assert!(
            bench.dialog.problem().contains("not applied"),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before);
        assert!(!leader(&bench.host, id).tip_ref.associated());
        bench.fill("leaderNote", "X");
        assert!(untouched(&bench), "back to what it showed");
        assert!(bench.enabled("leaderAttachSelected"));

        assert!(
            bench.dialog.attach_to_selected(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 1);
        // The tip (10,1) is over a quarter of the 40-long line: its foot (10,0).
        let attached = leader(&bench.host, id);
        assert_eq!(attached.tip_ref.entity, line);
        assert_eq!(attached.tip_ref.point, AnchorPoint::Along);
        assert_eq!(attached.tip_ref.parameter, 0.25);
        assert_eq!(attached.vertices[0], Point2::new(10.0, 0.0));

        bench.draw();
        assert!(
            bench.dialog.attach_to_selected(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 1, "on that place already: no step");
        bench.draw();
        assert!(bench.enabled("leaderAlong"));
        assert_eq!(number(&bench, "leaderAlong"), 25.0);
        assert!(
            bench.report("leaderTarget").ends_with("25.0% along it"),
            "{}",
            bench.report("leaderTarget")
        );
        bench.fill("leaderAlong", "75");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 2);
        assert_eq!(leader(&bench.host, id).tip_ref.parameter, 0.75);
        assert_eq!(leader(&bench.host, id).vertices[0], Point2::new(30.0, 0.0));
        assert_eq!(
            leader(&bench.host, id).vertices.last().copied(),
            Some(Point2::new(20.0, 10.0)),
            "the note hangs where it did"
        );

        // Along is one leader's: changed, then another leader selected with it, Apply refuses
        // rather than move both tips to one place.
        let other = bench.run("LEADER 50,0 55,5 text=Y");
        let made = bench.steps();
        bench.run(&format!("SELECT {id}"));
        bench.draw();
        bench.fill("leaderAlong", "50");
        bench.run(&format!("SELECT {id} {other}"));
        bench.draw();
        assert_eq!(bench.dialog.targets(bench.host.document()), [id, other]);
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(
            bench
                .dialog
                .problem()
                .starts_with("Along moves one leader's tip"),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), made);
        assert_eq!(
            leader(&bench.host, other).vertices[0],
            Point2::new(50.0, 0.0)
        );
        assert!(!leader(&bench.host, other).tip_ref.associated());
        // Set back, the rest of the form applies to both.
        assert!(bench.enabled("leaderAlong"));
        bench.fill("leaderAlong", "75");
        bench.fill("leaderCallout", "box");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, other).callout, CalloutShape::Box);
        assert_eq!(leader(&bench.host, id).tip_ref.parameter, 0.75);
    }

    #[test]
    fn attaching_a_tip_that_is_inside_a_lot_points_into_it() {
        let mut host = interpreter_host();
        // A 10 x 10 lot, and a plain leader whose tip (3,4) is inside it: it goes to the lot's
        // inside, its middle (5,5), as LEADER FOR points into a lot; one whose tip is outside
        // goes to the nearest place on it.
        let lot = run(&mut host, "PLINE 0,0 10,0 10,10 0,10 C");
        let inside = run(&mut host, "LEADER 3,4 20,20 text=IN");
        let outside = run(&mut host, "LEADER 14,5 20,5 text=OUT");
        let closed = matches!(
            host.document().model().entities.find(lot).map(|entity| &entity.geometry),
            Some(Geometry::Polyline(outline)) if outline.closed
        );
        assert!(closed, "the premise: a closed lot");
        let mut bench = manager(host);
        bench.run(&format!("SELECT {inside} {lot}"));
        bench.draw();
        assert_eq!(bench.dialog.current_leader(), inside);
        assert!(
            bench.dialog.attach_to_selected(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, inside).tip_ref.entity, lot);
        assert_eq!(
            leader(&bench.host, inside).tip_ref.point,
            AnchorPoint::Inside
        );
        assert_eq!(
            leader(&bench.host, inside).vertices[0],
            Point2::new(5.0, 5.0)
        );
        bench.draw();
        assert!(
            bench.report("leaderTarget").ends_with("inside it"),
            "{}",
            bench.report("leaderTarget")
        );

        bench.run(&format!("SELECT {outside} {lot}"));
        bench.draw();
        assert_eq!(bench.dialog.current_leader(), outside);
        assert!(
            bench.dialog.attach_to_selected(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            leader(&bench.host, outside).tip_ref.point,
            AnchorPoint::Along
        );
        assert_eq!(
            leader(&bench.host, outside).vertices[0],
            Point2::new(10.0, 5.0),
            "half way up the east side"
        );
    }

    #[test]
    fn an_attribute_is_set_on_the_entity_through_its_leader() {
        let mut host = interpreter_host();
        let pit = run(&mut host, "POINT 0,0");
        let id = run(
            &mut host,
            &format!("LEADER #{pit} 5,5 template=\"PIT {{id}}\\nIL {{prop.invert}}\""),
        );
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        bench.fill("leaderAttributeName", "invert");
        bench.fill("leaderAttributeValue", "9");
        bench.fill("leaderAttributeType", "real");
        let before = bench.steps();
        assert!(
            bench.dialog.set_attribute(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 1);
        let property = |bench: &Bench<LeaderManagerDialog>| {
            bench
                .document()
                .model()
                .entities
                .find(pit)
                .and_then(|entity| entity.properties.get("invert").cloned())
        };
        assert_eq!(
            property(&bench),
            Some(PropertyValue::Real(9.0)),
            "real, as stated"
        );
        bench.draw();
        assert!(
            bench.dialog.preview().ends_with("IL 9.000"),
            "{}",
            bench.dialog.preview()
        );
        assert!(
            bench
                .value_rows("leaderValues")
                .iter()
                .any(|row| row == "prop.invert=9.000")
        );
        assert!(bench.dialog.remove_attribute(&mut bench.host));
        assert_eq!(property(&bench), None);
        bench.fill("leaderAttributeValue", "nine");
        bench.fill("leaderAttributeType", "integer");
        assert!(!bench.dialog.set_attribute(&mut bench.host));
        assert!(!bench.dialog.problem().is_empty());
    }

    #[test]
    fn freeze_keeps_the_words_and_detach_lets_the_tip_go() {
        let mut host = interpreter_host();
        let pit = run(&mut host, "POINT 0,0");
        let id = run(
            &mut host,
            &format!("LEADER #{pit} 5,5 template=\"PIT {{id}}\""),
        );
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        assert!(bench.dialog.freeze(&mut bench.host));
        assert!(!is_smart(&leader(&bench.host, id)));
        assert_eq!(leader(&bench.host, id).text, format!("PIT {pit}"));
        assert!(leader(&bench.host, id).tip_ref.associated());
        bench.draw();
        assert_eq!(
            bench.value("leaderNoteKind"),
            TEXT,
            "the form shows the leader as it now is"
        );
        assert!(bench.dialog.detach(&mut bench.host));
        assert!(!leader(&bench.host, id).tip_ref.associated());
        bench.draw();
        assert!(bench.report("leaderTarget").starts_with("Nothing"));
    }

    #[test]
    fn the_form_follows_the_selection_and_an_undo_but_keeps_unapplied_edits() {
        let mut host = interpreter_host();
        let first = run(&mut host, "LEADER 0,0 5,5 text=ONE");
        let second = run(&mut host, "LEADER 10,0 15,5 text=TWO");
        let line = run(&mut host, "LINE 100,0 110,0");
        let mut bench = manager(host);
        bench.run(&format!("SELECT {second}"));
        bench.draw();
        assert_eq!(bench.dialog.current_leader(), second);

        // An edit elsewhere leaves unapplied edits in the form alone...
        bench.fill("leaderNote", "TWO, EDITED");
        bench
            .document_mut()
            .execute(move_entities(vec![line], Vec2::new(0.0, 1.0)))
            .expect("the line is moved");
        bench.draw();
        assert_eq!(bench.value("leaderNote"), "TWO, EDITED");
        // ...and an edit to the leader shows the leader as it now is.
        bench.run(&format!("LEADER SET {second} text=CHANGED"));
        bench.draw();
        assert_eq!(bench.value("leaderNote"), "CHANGED");
        bench.run("UNDO");
        bench.draw();
        assert_eq!(bench.value("leaderNote"), "TWO");

        // Erased, the form moves to a leader there still is.
        bench.run("ERASE");
        bench.draw();
        assert_eq!(bench.dialog.current_leader(), first);
        assert_eq!(bench.rows("leaderList"), 1);
    }

    #[test]
    fn a_label_style_note_lends_its_look_and_is_read_live() {
        let mut host = interpreter_host();
        let pit = run(&mut host, "POINT 0,0");
        run(&mut host, &format!("SELECT {pit}"));
        run(&mut host, "PROP SET invert 10.25");
        run(&mut host, "TEXTSTYLE NEW Notes paper=3.5");
        run(
            &mut host,
            "LABELSTYLE NEW Invert kind=point text=\"INV {prop.invert:.2f}\" textstyle=Notes \
             paper=3",
        );
        let id = run(&mut host, &format!("LEADER #{pit} 5,5 text=X"));
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        assert!(
            !bench.enabled("leaderLabelStyle"),
            "until the note is a style's"
        );
        bench.fill("leaderNoteKind", LABEL_STYLE);
        assert!(bench.enabled("leaderLabelStyle"));
        assert_eq!(bench.value("leaderLabelStyle"), "Invert");
        assert!(bench.dialog.note_is_read_only());
        assert_eq!(bench.value("leaderTextStyle"), "Notes");
        assert_eq!(number(&bench, "leaderPaperHeight"), 3.0);
        assert_eq!(bench.dialog.preview(), "INV 10.25");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, id).label_style, "Invert");
        assert_eq!(leader(&bench.host, id).style, "Notes");
        assert_eq!(leader(&bench.host, id).paper_height, 3.0);
    }

    #[test]
    fn for_selection_makes_a_leader_to_each_selected_entity_in_one_step() {
        let mut host = interpreter_host();
        // The dimension first: the first selected offers no place for a leader, so the preview
        // passes over it to a, and not on to b.
        let dim = run(&mut host, "DIM 0,20 10,20 2");
        let a = run(&mut host, "POINT 0,0");
        let b = run(&mut host, "POINT 10,0");
        run(&mut host, &format!("SELECT {dim} {a} {b}"));
        let mut bench = manager(host);
        bench.dialog.show_tab(Tab::ForSelection);
        bench.draw();
        assert_eq!(bench.report("leaderForSelection"), "3 entities selected");
        bench.fill("leaderForNoteKind", TEMPLATE);
        bench.fill("leaderForNote", "{type} {id}");
        assert_eq!(bench.dialog.for_preview(), format!("Point {a}"));
        assert_eq!(
            bench.report("leaderForCheck"),
            format!("For Point {a}, the first selected:")
        );
        let rows = bench.value_rows("leaderForValues");
        assert!(rows.iter().any(|row| *row == format!("id={a}")), "{rows:?}");
        let before = bench.steps();
        assert!(
            bench.dialog.make_for_selection(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 1);
        assert!(
            bench.dialog.for_report().starts_with("made=2 skipped=1"),
            "{}",
            bench.dialog.for_report()
        );
        let made = bench.document().last_created_entities();
        assert_eq!(made.len(), 2);
        assert_eq!(says(&bench, made[0]), format!("Point {a}"));

        // What was made is selected, and so shown on the Leader tab.
        assert_eq!(bench.document().selection().ids(), made);
        bench.draw();
        assert_eq!(bench.dialog.current_leader(), made[0]);

        // Balloons with no note are numbered, whatever the kind the box is at.
        for kind in [TEXT, TEMPLATE] {
            bench.fill("leaderForNote", "");
            bench.fill("leaderForNoteKind", kind);
            bench.fill("leaderForBalloon", "on");
            bench.run(&format!("SELECT {a} {b}"));
            bench.draw();
            let next = if kind == TEXT { 1 } else { 3 };
            assert_eq!(bench.dialog.for_preview(), format!("{next}"));
            assert!(
                bench.dialog.make_for_selection(&mut bench.host),
                "{kind}: {}",
                bench.dialog.problem()
            );
            let balloons = bench.document().last_created_entities();
            assert_eq!(balloons.len(), 2);
            assert_eq!(leader(&bench.host, balloons[0]).text, format!("{next}"));
            assert_eq!(
                leader(&bench.host, balloons[1]).text,
                format!("{}", next + 1)
            );
            assert_eq!(
                leader(&bench.host, balloons[0]).callout,
                CalloutShape::Circle
            );
        }

        bench.run("SELECT NONE");
        let steps = bench.steps();
        assert!(!bench.dialog.make_for_selection(&mut bench.host));
        assert!(
            bench.dialog.problem().contains("nothing is selected"),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), steps);
    }

    #[test]
    fn for_selection_is_previewed_for_the_first_and_placed_as_told() {
        let mut host = interpreter_host();
        run(&mut host, "ANNOSCALE 500");
        run(&mut host, "TEXTSTYLE NEW Notes paper=3.5");
        let pit = run(&mut host, "POINT 0,0");
        run(&mut host, &format!("SELECT {pit}"));
        run(&mut host, "PROP SET invert 10.5");
        let mut bench = manager(host);
        bench.dialog.show_tab(Tab::ForSelection);
        bench.draw();
        bench.fill("leaderForNoteKind", TEMPLATE);
        bench.fill("leaderForNote", "IL {prop.invert:.2f} ");
        assert_eq!(bench.dialog.for_preview(), "IL 10.50 ");
        assert_eq!(
            bench.report("leaderForCheck"),
            format!("For Point {pit}, the first selected:")
        );
        let rows = bench.value_rows("leaderForValues");
        assert!(
            rows.iter().any(|row| row == "prop.invert=10.500"),
            "{rows:?}"
        );
        // A double-clicked value goes where the user was typing.
        assert!(rows.iter().any(|row| row.starts_with("id=")), "{rows:?}");
        bench.dialog.insert_for_value(bench.host.document(), "id");
        bench.draw();
        assert_eq!(bench.value("leaderForNote"), "IL {prop.invert:.2f} {id}");
        assert_eq!(bench.dialog.for_preview(), format!("IL 10.50 {pit}"));
        bench.fill("leaderForNote", "IL {prop.invert:.2f} {id}{nothing}");
        assert!(
            bench.dialog.for_preview().contains("no value 'nothing'"),
            "{}",
            bench.dialog.for_preview()
        );
        bench.fill("leaderForNote", "IL {prop.invert:.2f} {id}");
        assert_eq!(bench.dialog.for_preview(), format!("IL 10.50 {pit}"));

        bench.fill("leaderForAngle", "90");
        bench.fill("leaderForLength", "10");
        bench.fill("leaderForCallout", "box");
        bench.fill("leaderForArrow", "Open");
        bench.fill("leaderForTextStyle", "Notes");
        // Not the text style's 3.5: the box's own is what is sent.
        bench.fill("leaderForPaperHeight", "2");
        let defaults = LeaderGeometry::default();
        assert_eq!(
            number(&bench, "leaderForArrowSize"),
            defaults.arrow_size,
            "a new leader's own, to start"
        );
        assert_eq!(number(&bench, "leaderForLanding"), defaults.landing);
        bench.fill("leaderForArrowSize", "4");
        bench.fill("leaderForLanding", "0");
        let before = bench.steps();
        assert!(
            bench.dialog.make_for_selection(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 1);
        assert_eq!(bench.dialog.for_report(), "made=1 skipped=0");
        let made = bench.document().last_created_entities();
        assert_eq!(made.len(), 1);
        let shape = leader(&bench.host, made[0]);
        // 10 mm on paper at 1:500 is 5 m, straight up (90 degrees) from the point.
        assert_eq!(shape.vertices[0], Point2::new(0.0, 0.0));
        let back = shape.vertices.last().copied().expect("a leader has an end");
        assert!(back.x.abs() <= 1e-12, "{}", back.x);
        assert!((back.y - 5.0).abs() <= 1e-12, "{}", back.y);
        assert_eq!(shape.callout, CalloutShape::Box);
        assert_eq!(shape.arrow, ArrowHead::Open);
        assert_eq!(shape.style, "Notes");
        assert_eq!(shape.paper_height, 2.0);
        assert_eq!(shape.arrow_size, 4.0);
        assert_eq!(shape.landing, 0.0, "none");
        assert_eq!(says(&bench, made[0]), format!("IL 10.50 {pit}"));
    }

    #[test]
    fn arrange_aligns_the_selected_notes_and_renumbers_the_balloons() {
        let mut host = interpreter_host();
        run(&mut host, "ANNOSCALE 500");
        let a = run(&mut host, "LEADER 0,0 10,30 text=A");
        let b = run(&mut host, "LEADER 5,0 22,40 text=B");
        run(&mut host, &format!("SELECT {a} {b}"));
        let mut bench = manager(host);
        bench.dialog.show_tab(Tab::Arrange);
        bench.draw();
        bench.fill("leaderAlignUseSpacing", "on");
        bench.fill("leaderAlignSpacing", "8");
        assert!(
            bench.dialog.align_selection(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        // B hangs highest and sets x = 22; 8 mm at 1:500 is 4 m below it.
        assert_eq!(
            leader(&bench.host, b).vertices.last().copied(),
            Some(Point2::new(22.0, 40.0))
        );
        assert_eq!(
            leader(&bench.host, a).vertices.last().copied(),
            Some(Point2::new(22.0, 36.0))
        );
        assert_eq!(bench.dialog.arrange_report(), "aligned=2");

        bench.run("SELECT NONE");
        assert!(!bench.dialog.align_selection(&mut bench.host));
        assert!(
            bench.dialog.problem().contains("no leader is selected"),
            "{}",
            bench.dialog.problem()
        );

        let seven = bench.run("BALLOON 30,0 35,5 n=7");
        let three = bench.run("BALLOON 10,20 15,25 n=3");
        bench.fill("balloonRenumberOrder", "Across, left to right");
        bench.fill("balloonRenumberStart", "5");
        let before = bench.steps();
        assert!(
            bench.dialog.renumber_balloons(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 1);
        // Across by tip: the one at x = 10 first, from 5.
        assert_eq!(leader(&bench.host, three).text, "5");
        assert_eq!(leader(&bench.host, seven).text, "6");
        assert_eq!(bench.dialog.arrange_report(), "balloons=2 renumbered=2");
        assert!(bench.dialog.renumber_balloons(&mut bench.host));
        assert_eq!(bench.steps(), before + 1, "numbered so already: no step");
        assert_eq!(bench.dialog.arrange_report(), "balloons=2 renumbered=0");
    }

    #[test]
    fn a_drawing_replaced_starts_the_form_afresh() {
        let scratch = Scratch::new("leader-manager-replaced");
        let directory = scratch.path("replaced.katana");
        let mut host = interpreter_host();
        let id = run(&mut host, "LEADER 0,0 5,5 text=ONE");
        host.document_mut()
            .save_as(&directory)
            .expect("the drawing is saved");
        let mut bench = manager(host);
        assert_eq!(bench.dialog.current_leader(), id);
        bench.fill("leaderNote", "EDITED");
        // The very same leader, by id and shape, in the drawing opened: the form's edits were
        // for the one before and must not be applied to it.
        bench
            .document_mut()
            .open(&directory)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(leader(&bench.host, id).text, "ONE");
        bench.draw();
        assert_eq!(bench.dialog.current_leader(), id);
        assert_eq!(bench.value("leaderNote"), "ONE");
        assert!(untouched(&bench));

        bench.document_mut().new_document();
        bench.draw();
        assert_eq!(bench.dialog.current_leader(), 0);
        assert_eq!(bench.rows("leaderList"), 0);
        assert!(!bench.enabled("leaderApply"));
        assert!(!bench.dialog.apply(&mut bench.host));
    }
}
