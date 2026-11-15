//! The twins of tests/qt_widgets/annotation/test_leader_manager_lines.cpp: the Leaders manager
//! hands verb lines to the window's one executor; it changes nothing itself. Every button builds
//! the LEADER or BALLOON line a person would type - only what changed, numbers written exactly -
//! and the dialog's tests drive those lines. A note a command line cannot carry is refused with a
//! sentence, and the shared Apply to / Only those that match controls choose which leaders the list
//! shows and which entities and leaders the For Selection and Arrange tabs act on.
//!
//! Expected lines and counts are worked out by hand beside each assertion.

use katana_cad::annotation::dimension_build::AnchoredPoint;
use katana_cad::annotation::leader_edit::{
    LeaderChange, LeaderNote, LeaderNoteKind, LeaderVertexMove,
};
use katana_entity::{AnchorPoint, AnchorRef, ArrowHead, CalloutShape, EntityId};
use katana_geometry::Point2;
use katana_window::annotation::leader_manager::{
    LeaderManagerContext, LeaderManagerDialog, Tab, leader_option_words, leader_set_line,
    leaders_for_line,
};
use katana_window::host::RecordingHost;

use crate::support::{Bench, interpreter_host, leader, recording_only_host, run};

/// The note-kind boxes' Template, the one chosen here.
const TEMPLATE: &str = "Template (read off the entity)";

fn manager(host: RecordingHost) -> Bench<LeaderManagerDialog> {
    Bench::new(host, |document| {
        LeaderManagerDialog::new(document, LeaderManagerContext::default())
    })
}

fn show(bench: &mut Bench<LeaderManagerDialog>, id: EntityId) -> bool {
    let shown = bench.dialog.show_leader(bench.host.document(), id);
    bench.draw();
    shown
}

fn words(change: &LeaderChange) -> String {
    leader_option_words(change).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn last_line(bench: &Bench<LeaderManagerDialog>) -> String {
    bench.lines().last().cloned().unwrap_or_default()
}

/// Two points with a level on layer plan, one on layer other; nothing selected.
fn levels(host: &mut RecordingHost) -> [EntityId; 3] {
    run(host, "LAYER NEW plan");
    run(host, "LAYER NEW other");
    run(host, "LAYER SET plan");
    let q1 = run(host, "POINT 0,0");
    let q2 = run(host, "POINT 10,0");
    run(host, "LAYER SET other");
    let q3 = run(host, "POINT 20,0");
    run(host, "LAYER SET 0");
    for point in [q1, q2, q3] {
        run(host, &format!("SELECT {point}"));
        run(host, "PROP SET elevation 25");
    }
    run(host, "SELECT NONE");
    [q1, q2, q3]
}

mod leader_manager_lines {
    use super::*;

    // ---- the lines ------------------------------------------------------------------------------

    #[test]
    fn only_what_changed_is_written_and_numbers_are_written_exactly() {
        let change = LeaderChange {
            paper_height: Some(2.3456),
            arrow_size: Some(0.125),
            landing: Some(1500.0),
            arrow: Some(ArrowHead::Dot),
            callout: Some(CalloutShape::Box),
            text_style: Some(String::from("My Style")),
            ..LeaderChange::default()
        };
        // Each number as core's format_exact_real writes it: the shortest text that reads back
        // as the same double, so 2.3456 is not cut to 2.346.
        let written = words(&change);
        assert_eq!(
            written,
            " arrow=Dot callout=box style=\"My Style\" paper=2.3456 arrowsize=0.125 landing=1500"
        );

        // A tip along an entity, written as the verb reads it: segment 2, a fraction written
        // exactly.
        let mut moved = LeaderChange {
            tip: Some(AnchoredPoint {
                point: Point2::new(1.0, 2.0),
                r#ref: AnchorRef {
                    entity: 7,
                    point: AnchorPoint::Along,
                    index: 2,
                    parameter: 0.123456,
                },
            }),
            ..LeaderChange::default()
        };
        assert_eq!(words(&moved), " tip=#7.along2:0.123456");
        moved.tip = Some(AnchoredPoint {
            point: Point2::new(3.25, 4.5),
            r#ref: AnchorRef::default(),
        });
        assert_eq!(words(&moved), " tip=3.25,4.5");
        let point = LeaderChange {
            vertex: Some(LeaderVertexMove {
                index: 1,
                position: Point2::new(7.125, 9.5),
            }),
            landing_angle: Some(None),
            ..LeaderChange::default()
        };
        assert_eq!(words(&point), " landingangle=auto vertex=1 at=7.125,9.5");

        // The ids, then the options; a change that sets nothing is no line.
        let line = leader_set_line(&[3, 5], &change).expect("the line is made");
        assert_eq!(line, format!("LEADER SET 3 5{written}"));
        let none = leader_set_line(&[3], &LeaderChange::default()).expect("no line is made");
        assert!(none.is_empty());
        assert!(
            leader_set_line(&[], &change).is_err(),
            "no leader to change"
        );
    }

    #[test]
    fn a_note_is_written_as_text_a_template_or_a_label_style_and_a_line_break_as_backslash_n() {
        let mut change = LeaderChange {
            note: Some(LeaderNote {
                kind: LeaderNoteKind::Text,
                text: String::from("IL 10\nsecond line"),
            }),
            ..LeaderChange::default()
        };
        // A blank makes it one quoted word; the break is the two characters the verb reads back
        // as one.
        assert_eq!(words(&change), " text=\"IL 10\\nsecond line\"");
        let note = |kind: LeaderNoteKind, text: &str| {
            Some(LeaderNote {
                kind,
                text: String::from(text),
            })
        };
        change.note = note(LeaderNoteKind::Template, "{rl:.3f}");
        assert_eq!(words(&change), " template={rl:.3f}");
        change.note = note(LeaderNoteKind::LabelStyle, "Lot Area");
        assert_eq!(words(&change), " labelstyle=\"Lot Area\"");
        // A bare leader: the empty note, which a word still says.
        change.note = note(LeaderNoteKind::Text, "");
        assert_eq!(words(&change), " text=\"\"");
        // A backslash is written twice, so one before an n is not a line break.
        change.note = note(LeaderNoteKind::Text, "C:\\new");
        assert_eq!(words(&change), " text=C:\\\\new");
    }

    #[test]
    fn a_double_quote_is_refused_with_a_sentence_that_names_the_field() {
        let change = LeaderChange {
            note: Some(LeaderNote {
                kind: LeaderNoteKind::Text,
                text: String::from("say \"hi\""),
            }),
            ..LeaderChange::default()
        };
        let refused = leader_option_words(&change).expect_err("a double quote");
        assert!(refused.message.contains("the note"), "{}", refused.message);
        assert!(refused.message.contains("double quote"));

        let style = LeaderChange {
            text_style: Some(String::from("My \"Style\"")),
            ..LeaderChange::default()
        };
        let refused = leader_option_words(&style).expect_err("a double quote");
        assert!(refused.message.contains("text style"));

        // The line for the entities of a scope: the options first, then the scope's words, which
        // a WHERE would otherwise swallow them into.
        let arrow = LeaderChange {
            arrow: Some(ArrowHead::Tick),
            ..LeaderChange::default()
        };
        let line = leaders_for_line(false, "LAYERS plan WHERE TYPE=point", &arrow, 45.0, 10.0)
            .expect("the line is made");
        assert_eq!(
            line,
            "LEADER FOR arrow=Tick angle=45 length=10 LAYERS plan WHERE TYPE=point"
        );
        let balloons = leaders_for_line(true, "SELECTION", &LeaderChange::default(), 30.5, 4.0)
            .expect("the line is made");
        assert_eq!(balloons, "BALLOON FOR angle=30.5 length=4 SELECTION");
        assert!(
            leaders_for_line(false, "", &LeaderChange::default(), 45.0, 10.0).is_err(),
            "which entities?"
        );
        let tip = LeaderChange {
            tip: Some(AnchoredPoint {
                point: Point2::new(0.0, 0.0),
                r#ref: AnchorRef::default(),
            }),
            ..LeaderChange::default()
        };
        assert!(leaders_for_line(false, "SELECTION", &tip, 45.0, 10.0).is_err());
    }

    // ---- the buttons ----------------------------------------------------------------------------

    #[test]
    fn apply_hands_over_one_line_of_what_changed_and_changes_nothing_itself() {
        let mut host = recording_only_host();
        let pit = run(&mut host, "POINT 0,0");
        let id = run(&mut host, &format!("LEADER #{pit} 5,5 text=A"));
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        let steps = bench.steps();
        bench.fill("leaderCallout", "box");
        bench.fill("leaderPaperHeight", "3.5");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        // The callout and the height, nothing else: the leader's note, arrow, sizes and landing
        // are what they were.
        let expected = format!("LEADER SET {id} callout=box paper=3.5");
        assert_eq!(bench.lines(), [expected.as_str()]);
        assert_eq!(bench.dialog.last_line(), last_line(&bench));
        bench.draw();
        assert_eq!(bench.value("leaderLine"), last_line(&bench));
        assert_eq!(bench.steps(), steps, "the dialog edited nothing");
        assert_eq!(leader(&bench.host, id).callout, CalloutShape::None);
    }

    #[test]
    fn an_untouched_form_runs_no_line_and_an_executor_with_nothing_to_run_on_is_said_so() {
        let mut host = interpreter_host();
        let id = run(&mut host, "LEADER 0,0 5,5 text=A");
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        assert!(bench.dialog.apply(&mut bench.host));
        assert!(bench.lines().is_empty(), "nothing changed: no line");
        let Bench { host, .. } = bench;

        // No executor at all: a refusal in the problem line, never a silent success.
        let mut bare = Bench::new(host, |document| {
            LeaderManagerDialog::new(document, LeaderManagerContext::default())
                .without_command_line()
        });
        assert!(show(&mut bare, id));
        bare.fill("leaderNote", "B");
        assert!(!bare.dialog.apply(&mut bare.host));
        assert!(
            bare.dialog.problem().contains("no command line"),
            "{}",
            bare.dialog.problem()
        );
        assert_eq!(leader(&bare.host, id).text, "A");
        assert!(bare.lines().is_empty());
    }

    #[test]
    fn a_note_with_a_double_quote_is_refused_and_nothing_runs() {
        let mut host = interpreter_host();
        let id = run(&mut host, "LEADER 0,0 5,5 text=A");
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        bench.fill("leaderNote", "say \"hi\"");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(
            bench.dialog.problem().contains("the note"),
            "{}",
            bench.dialog.problem()
        );
        assert!(bench.dialog.problem().contains("double quote"));
        assert!(bench.lines().is_empty());
        assert_eq!(leader(&bench.host, id).text, "A");
    }

    #[test]
    fn a_backslash_before_an_n_and_a_line_break_in_the_note_arrive_whole() {
        let mut host = interpreter_host();
        let id = run(&mut host, "LEADER 0,0 5,5 text=A");
        let mut bench = manager(host);
        assert!(show(&mut bench, id));
        // C:\new\temp: a backslash before an n, which the verb would read as a line break had it
        // not been written twice. Typed whole: a fill would read the backslash and n as a break.
        bench
            .dialog
            .type_note(bench.host.document(), "C:\\new\\temp");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            last_line(&bench),
            format!("LEADER SET {id} text=C:\\\\new\\\\temp")
        );
        assert_eq!(leader(&bench.host, id).text, "C:\\new\\temp");
        bench.draw();
        bench.fill("leaderNote", "one\\ntwo");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, id).text, "one\ntwo");
    }

    #[test]
    fn freeze_detach_attach_and_the_attribute_each_hand_over_their_line() {
        let mut host = recording_only_host();
        let line = run(&mut host, "LINE 0,0 40,0");
        // A plain leader whose tip is at (10, 3): the place of the line nearest it is (10, 0), a
        // quarter of the way along a line of length 40.
        let id = run(&mut host, "LEADER 10,3 20,10 text=A");
        let mut bench = manager(host);
        assert!(show(&mut bench, id));

        assert!(bench.dialog.freeze(&mut bench.host));
        assert!(bench.dialog.detach(&mut bench.host));
        assert_eq!(
            bench.host.take_lines(),
            [format!("LEADER FREEZE {id}"), format!("LEADER DETACH {id}")]
        );

        bench.run(&format!("SELECT {line} {id}"));
        assert!(
            bench.dialog.attach_to_selected(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            bench.host.take_lines(),
            [format!("LEADER ATTACH {id} #{line}.along0:0.25")]
        );

        bench.fill("leaderAttributeName", " invert ");
        bench.fill("leaderAttributeValue", "10.5");
        bench.fill("leaderAttributeType", "real");
        assert!(bench.dialog.set_attribute(&mut bench.host));
        assert!(bench.dialog.remove_attribute(&mut bench.host));
        assert_eq!(
            bench.host.take_lines(),
            [
                format!("LEADER PROP {id} SET invert 10.5 real"),
                format!("LEADER PROP {id} DELETE invert"),
            ]
        );
        // A value with a blank is one quoted word; one with a quote is refused.
        bench.fill("leaderAttributeValue", "two words");
        bench.fill("leaderAttributeType", "as typed");
        assert!(bench.dialog.set_attribute(&mut bench.host));
        assert_eq!(
            last_line(&bench),
            format!("LEADER PROP {id} SET invert \"two words\"")
        );
        bench.fill("leaderAttributeValue", "a\"b");
        assert!(!bench.dialog.set_attribute(&mut bench.host));
        assert!(
            bench.dialog.problem().contains("attribute value"),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.lines().len(), 1, "the refused one ran nothing");
    }

    #[test]
    fn several_selected_leaders_are_one_line_naming_each_of_them() {
        let mut host = recording_only_host();
        let a = run(&mut host, "LEADER 0,0 5,5 text=A");
        let b = run(&mut host, "LEADER 10,0 15,5 text=B");
        run(&mut host, &format!("SELECT {a} {b}"));
        let mut bench = manager(host);
        assert!(show(&mut bench, a));
        bench.fill("leaderCallout", "box");
        assert!(bench.dialog.apply(&mut bench.host));
        assert_eq!(bench.lines(), [format!("LEADER SET {a} {b} callout=box")]);
    }

    // ---- the list and its scope -----------------------------------------------------------------

    #[test]
    fn the_list_shows_every_leader_or_those_the_scope_takes() {
        let mut host = interpreter_host();
        run(&mut host, "LAYER NEW notes");
        run(&mut host, "LAYER NEW other");
        run(&mut host, "LAYER SET notes");
        let a = run(&mut host, "LEADER 0,0 5,5 text=A");
        run(&mut host, "LAYER SET other");
        let b = run(&mut host, "LEADER 10,0 15,5 text=B");
        run(&mut host, "LAYER SET notes");
        let c = run(&mut host, "LEADER 20,0 25,5 text=C");
        run(&mut host, "LAYER SET 0");
        let mut bench = manager(host);
        assert_eq!(
            bench.dialog.listed_leaders(bench.host.document()),
            [a, b, c]
        );
        assert_eq!(bench.rows("leaderList"), 3);

        // "Which leaders...": the whole drawing to begin with, so all three.
        bench.press("leaderListScopeToggle");
        assert_eq!(bench.rows("leaderList"), 3);
        // The checked layers: notes holds a and c.
        bench.press("leaderListScopeLayers");
        assert_eq!(bench.rows("leaderList"), 0, "no layer ticked yet");
        assert!(!bench.report("leaderListScopeProblem").is_empty());
        bench.dialog.list_scope_mut().tick_layer("notes", true);
        bench.draw();
        assert_eq!(bench.dialog.listed_leaders(bench.host.document()), [a, c]);
        assert_eq!(bench.rows("leaderList"), 2);
        assert!(bench.report("leaderListScopeProblem").is_empty());
        // Only those that match: a layer pattern over the whole drawing.
        bench.press("leaderListScopeDrawing");
        bench.fill("leaderListFilterLayer", "oth*");
        assert_eq!(bench.dialog.listed_leaders(bench.host.document()), [b]);
        // Off again: every leader.
        bench.press("leaderListScopeToggle");
        assert_eq!(bench.rows("leaderList"), 3);
    }

    // ---- For Selection --------------------------------------------------------------------------

    #[test]
    fn for_selection_makes_leaders_for_the_selected_entities_by_default() {
        let mut host = interpreter_host();
        let [q1, q2, _] = levels(&mut host);
        run(&mut host, &format!("SELECT {q1} {q2}"));
        let mut bench = manager(host);
        bench.dialog.show_tab(Tab::ForSelection);
        bench.draw();
        assert_eq!(bench.report("leaderForSelection"), "2 entities selected");
        bench.fill("leaderForNoteKind", TEMPLATE);
        bench.fill("leaderForNote", "RL {rl:.3f}");
        let steps = bench.steps();
        assert!(
            bench.dialog.make_for_selection(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        // The note, the direction and distance as they stand (45 degrees, 10 mm), and the
        // selection as the scope: all in one line.
        assert_eq!(
            bench.lines(),
            ["LEADER FOR template=\"RL {rl:.3f}\" angle=45 length=10 SELECTION"]
        );
        assert_eq!(bench.steps(), steps + 1);
        assert_eq!(bench.dialog.for_report(), "made=2 skipped=0");
        bench.draw();
        assert_eq!(
            bench.report("leaderForScopeReport"),
            "scope=selection matched=2"
        );
        // What was made is selected, as it always was.
        assert_eq!(bench.document().selection().size(), 2);
    }

    #[test]
    fn for_selection_takes_a_layer_or_a_filter_and_says_what_the_scope_took() {
        let mut host = interpreter_host();
        levels(&mut host);
        let mut bench = manager(host);
        bench.dialog.show_tab(Tab::ForSelection);
        bench.draw();
        // Nothing selected, and the controls say selected entities: refused in the verb's words
        // and nothing made.
        let steps = bench.steps();
        assert!(!bench.dialog.make_for_selection(&mut bench.host));
        assert!(
            bench.dialog.problem().contains("nothing is selected"),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), steps);

        // The points on layer plan: q1 and q2 (hand count; q3 is on other).
        bench.press("leaderForScopeLayers");
        bench.dialog.for_scope_mut().tick_layer("plan", true);
        bench.draw();
        assert_eq!(
            bench.report("leaderForSelection"),
            "2 entities taken by Apply to"
        );
        assert!(
            bench.dialog.make_for_selection(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            last_line(&bench),
            "LEADER FOR text=\"\" angle=45 length=10 LAYERS plan",
            "no note typed: a bare arrow, said so"
        );
        assert_eq!(bench.dialog.for_report(), "made=2 skipped=0");
        bench.draw();
        assert_eq!(
            bench.report("leaderForScopeReport"),
            "scope=layers layers=plan sublayers=yes matched=2"
        );

        // The whole drawing, only the points: the three, as balloons.
        bench.press("leaderForScopeDrawing");
        bench.fill("leaderForTypePoint", "on");
        bench.fill("leaderForBalloon", "on");
        assert!(
            bench.dialog.make_for_selection(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            last_line(&bench),
            "BALLOON FOR angle=45 length=10 DRAWING WHERE TYPE=point"
        );
        assert_eq!(bench.dialog.for_report(), "made=3 skipped=0");
    }

    #[test]
    fn for_selection_says_why_entities_were_skipped() {
        let mut host = interpreter_host();
        let [q1, _, _] = levels(&mut host);
        let dimension = run(&mut host, "DIM ALIGNED 0,50 10,50 at=0,55");
        run(&mut host, &format!("SELECT {q1} {dimension}"));
        let mut bench = manager(host);
        bench.dialog.show_tab(Tab::ForSelection);
        bench.draw();
        assert!(
            bench.dialog.make_for_selection(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        // The point has a place for a leader; the dimension has none.
        assert!(
            bench
                .dialog
                .for_report()
                .starts_with(&format!("made=1 skipped=1: id={dimension}")),
            "{}",
            bench.dialog.for_report()
        );
    }

    // ---- Arrange --------------------------------------------------------------------------------

    #[test]
    fn arrange_hands_over_align_and_renumber_lines_with_their_scope() {
        let mut host = recording_only_host();
        run(&mut host, "LAYER NEW notes");
        let mut bench = manager(host);
        bench.dialog.show_tab(Tab::Arrange);
        bench.draw();

        // The notes of the selected leaders, at the topmost's x.
        assert!(bench.dialog.align_selection(&mut bench.host));
        assert_eq!(last_line(&bench), "LEADER ALIGN SELECTION");
        bench.fill("leaderAlignUseX", "on");
        bench.fill("leaderAlignX", "22.5");
        bench.fill("leaderAlignUseSpacing", "on");
        bench.fill("leaderAlignSpacing", "8");
        assert!(bench.dialog.align_selection(&mut bench.host));
        assert_eq!(last_line(&bench), "LEADER ALIGN x=22.5 spacing=8 SELECTION");
        // The leaders of a layer.
        bench.press("leaderArrangeScopeLayers");
        bench.dialog.arrange_scope_mut().tick_layer("notes", true);
        bench.draw();
        assert!(bench.dialog.align_selection(&mut bench.host));
        assert_eq!(
            last_line(&bench),
            "LEADER ALIGN x=22.5 spacing=8 LAYERS notes"
        );

        // The balloons: every numbered one, as it always was, unless ticked.
        assert!(bench.dialog.renumber_balloons(&mut bench.host));
        assert_eq!(last_line(&bench), "BALLOON RENUMBER start=1 order=id");
        bench.fill("balloonRenumberStart", "5");
        bench.fill("balloonRenumberOrder", "Across, left to right");
        bench.fill("balloonRenumberScoped", "on");
        assert!(bench.dialog.renumber_balloons(&mut bench.host));
        assert_eq!(
            last_line(&bench),
            "BALLOON RENUMBER start=5 order=x LAYERS notes"
        );
    }

    #[test]
    fn align_and_renumber_report_what_the_verbs_replied() {
        let mut host = interpreter_host();
        run(&mut host, "LAYER NEW notes");
        run(&mut host, "LAYER SET notes");
        let one = run(&mut host, "BALLOON 0,0 5,5");
        let two = run(&mut host, "BALLOON 10,0 15,5");
        run(&mut host, "LAYER SET 0");
        let other = run(&mut host, "BALLOON 20,0 25,5");
        let mut bench = manager(host);
        bench.dialog.show_tab(Tab::Arrange);
        bench.draw();
        // The two on notes become 10 and 11; the one on layer 0 keeps its 3.
        bench.fill("balloonRenumberStart", "10");
        bench.fill("balloonRenumberScoped", "on");
        bench.press("leaderArrangeScopeLayers");
        bench.dialog.arrange_scope_mut().tick_layer("notes", true);
        bench.draw();
        assert!(
            bench.dialog.renumber_balloons(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, one).text, "10");
        assert_eq!(leader(&bench.host, two).text, "11");
        assert_eq!(leader(&bench.host, other).text, "3");
        assert_eq!(bench.dialog.arrange_report(), "balloons=2 renumbered=2");
    }
}
