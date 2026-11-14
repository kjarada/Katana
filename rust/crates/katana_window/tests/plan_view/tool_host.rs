//! The twin of tests/qt_widgets/tools/test_tool_host.cpp: the tool host on its own
//! (tools/tool_host), without a view - what it reports through its hooks, and that a hook may
//! start another tool while the host is still dealing with the step that raised it, as the window
//! does when it chains one command into the next.
//!
//! The C++ host keeps the document; here every call is lent it, and a hook is handed the host and
//! the document it could not keep.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use katana_cad::drawing::grips::{Grip, GripKind};
use katana_cad::drawing::vertex_editing::edit_polyline;
use katana_cad::snapping::{SnapMode, SnapResult};
use katana_cad::tool_feedback::{FeedbackRole, ToolFeedback};
use katana_cad::Document;
use katana_commands::{EntityAttributes, create_circle, create_line, create_polyline};
use katana_entity::{AnchorPoint, EntityId, Geometry, LeaderGeometry};
use katana_geometry::polyline_vertices::move_vertex;
use katana_geometry::{CurvePolyline2, Point2, Polyline2};
use katana_window::tools::tool_host::{
    Outcome, ToolHost, is_transparent_command, text_hook, transparent_hook,
};

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn add_line(document: &mut Document, start: Point2, end: Point2) -> EntityId {
    document
        .execute(create_line(start, end, EntityAttributes::default()))
        .expect("the line is made");
    document.last_created_entities()[0]
}

fn add_polyline(document: &mut Document, vertices: Vec<Point2>) -> EntityId {
    document
        .execute(create_polyline(
            Polyline2::new(vertices, false),
            EntityAttributes::default(),
        ))
        .expect("the polyline is made");
    document.last_created_entities()[0]
}

/// A list a hook writes into and the test reads.
fn said() -> Rc<RefCell<Vec<String>>> {
    Rc::new(RefCell::new(Vec::new()))
}

mod tool_host {
    use super::*;

    #[test]
    fn a_tool_started_from_a_hook_of_the_step_that_finished_another_keeps_running() {
        // Move does not restart, so after its command the host ends it - unless a hook has
        // already started another tool, which must not be ended in its place.
        let mut document = Document::new(None);
        let line = add_line(&mut document, p(0.0, 0.0), p(10.0, 0.0));
        document.selection_mut().add(line);
        let mut host = ToolHost::new();
        let messages = said();
        let heard = Rc::clone(&messages);
        host.on_message = Some(text_hook(move |host, document, message| {
            heard.borrow_mut().push(String::from(message));
            if message.contains("moved") {
                host.start(document, "draw.circle")
                    .expect("the circle tool starts");
            }
        }));
        host.start(&mut document, "modify.move")
            .expect("the move tool starts");
        let _ = host.point(&mut document, p(0.0, 0.0), None);
        let _ = host.point(&mut document, p(0.0, 5.0), None);

        assert_eq!(host.active_id(), "draw.circle");
        assert!(!messages.borrow().is_empty());
        assert_eq!(
            document.history().undo_count(),
            2,
            "the line, then the move"
        );
    }

    #[test]
    fn input_with_no_tool_running_is_refused_aloud() {
        let mut document = Document::new(None);
        let mut host = ToolHost::new();
        let refusals = said();
        let heard = Rc::clone(&refusals);
        host.on_rejected = Some(text_hook(move |_host, _document, why| {
            heard.borrow_mut().push(String::from(why));
        }));
        assert_eq!(
            host.point(&mut document, p(1.0, 1.0), None),
            Outcome::Rejected
        );
        assert_eq!(host.enter(&mut document), Outcome::Rejected);
        assert_eq!(refusals.borrow().len(), 2);
    }

    #[test]
    fn a_restarting_tool_reports_its_prompt_again_and_ends_only_on_esc() {
        // Point restarts after every point: two points are two commands, and the tool is still
        // running until Esc.
        let mut document = Document::new(None);
        let mut host = ToolHost::new();
        let finished = said();
        let heard = Rc::clone(&finished);
        host.on_finished = Some(text_hook(move |_host, _document, id| {
            heard.borrow_mut().push(String::from(id));
        }));
        host.start(&mut document, "draw.point")
            .expect("the tool starts");
        assert_eq!(host.point(&mut document, p(1.0, 2.0), None), Outcome::Done);
        assert_eq!(host.point(&mut document, p(3.0, 4.0), None), Outcome::Done);
        assert!(host.active());
        assert_eq!(document.history().undo_count(), 2);
        host.cancel(&mut document);
        assert!(!host.active());
        assert_eq!(*finished.borrow(), ["draw.point"]);
    }

    #[test]
    fn abandon_ends_the_tool_without_committing_its_points() {
        // For a replaced document: the chain belongs to the drawing that went, and the tool
        // ends, as Esc ends it, so the toolbar hears of it.
        let mut document = Document::new(None);
        let mut host = ToolHost::new();
        let finished = said();
        let heard = Rc::clone(&finished);
        host.on_finished = Some(text_hook(move |_host, _document, id| {
            heard.borrow_mut().push(String::from(id));
        }));
        host.start(&mut document, "draw.line")
            .expect("the tool starts");
        let _ = host.point(&mut document, p(0.0, 0.0), None);
        let _ = host.point(&mut document, p(5.0, 0.0), None);
        host.abandon(&mut document);
        assert!(!host.active());
        assert_eq!(
            document.history().undo_count(),
            0,
            "Esc would have kept the segment"
        );
        assert_eq!(*finished.borrow(), ["draw.line"]);
    }

    // ---- Esc through the tool's own cancel(), transparent commands, selections -------------

    #[test]
    fn esc_in_copy_keeps_the_copies_already_placed_as_one_undo_step() {
        // Copy was left out of the host's old keep-on-Esc list, because its Enter with no copy
        // placed copies by the base point; the tool's own cancel() now keeps what was placed and
        // applies no default.
        let mut document = Document::new(None);
        let line = add_line(&mut document, p(0.0, 0.0), p(1.0, 0.0));
        document.selection_mut().add(line);
        let mut host = ToolHost::new();
        let messages = said();
        let heard = Rc::clone(&messages);
        host.on_message = Some(text_hook(move |_host, _document, message| {
            heard.borrow_mut().push(String::from(message));
        }));
        host.start(&mut document, "modify.copy")
            .expect("the tool starts");
        let _ = host.point(&mut document, p(0.0, 0.0), None);
        let _ = host.point(&mut document, p(0.0, 5.0), None);
        let _ = host.point(&mut document, p(0.0, 10.0), None);
        host.cancel(&mut document);
        assert!(!host.active());
        // The line, then ONE command for both copies: three lines in all.
        assert_eq!(document.history().undo_count(), 2);
        assert_eq!(document.model().entities.size(), 3);
        assert_eq!(
            messages.borrow().last().map(String::as_str),
            Some("2 copies of 1 entity")
        );
    }

    #[test]
    fn zoom_typed_while_a_tool_runs_goes_to_the_view_and_the_tool_keeps_its_step() {
        let mut document = Document::new(None);
        let mut host = ToolHost::new();
        let transparent = said();
        let heard = Rc::clone(&transparent);
        host.on_transparent = Some(transparent_hook(move |_host, _document, command| {
            heard.borrow_mut().push(String::from(command));
            true
        }));
        host.start(&mut document, "draw.line")
            .expect("the tool starts");
        let _ = host.point(&mut document, p(0.0, 0.0), None);
        assert_eq!(host.typed(&mut document, "Z"), Outcome::Continue);
        assert_eq!(host.typed(&mut document, "'zoom e"), Outcome::Continue);
        assert_eq!(host.typed(&mut document, "PAN"), Outcome::Continue);
        assert_eq!(*transparent.borrow(), ["Z", "'zoom e", "PAN"]);
        // Still at the second point of the same chain.
        assert_eq!(host.last_point(&document), Some(p(0.0, 0.0)));
        assert_eq!(host.typed(&mut document, "10,0"), Outcome::Continue);
        host.cancel(&mut document);
        assert_eq!(document.model().entities.size(), 1);
    }

    #[test]
    fn a_transparent_command_with_no_view_to_run_it_is_refused_not_sent_to_the_tool() {
        let mut document = Document::new(None);
        let mut host = ToolHost::new();
        let refusals = said();
        let heard = Rc::clone(&refusals);
        host.on_rejected = Some(text_hook(move |_host, _document, why| {
            heard.borrow_mut().push(String::from(why));
        }));
        host.start(&mut document, "draw.line")
            .expect("the tool starts");
        assert_eq!(host.typed(&mut document, "ZOOM"), Outcome::Rejected);
        assert_eq!(
            *refusals.borrow(),
            ["ZOOM cannot run inside Line; press Esc to end the tool first."]
        );
        assert!(host.active());
    }

    #[test]
    fn a_bare_p_is_an_option_for_the_tool_and_only_the_apostrophe_form_is_pan() {
        assert!(is_transparent_command("'P", false));
        assert!(is_transparent_command(" zoom ", false));
        // Rotate's and Scale's [Points].
        assert!(!is_transparent_command("P", false));
        assert!(!is_transparent_command("ZOOMY", false));
        assert!(!is_transparent_command("10,0", false));
        // At a prompt for a value a bare word is the answer; 'Z is still ZOOM.
        assert!(!is_transparent_command("Z", true));
        assert!(is_transparent_command("'Z", true));
    }

    #[test]
    fn a_word_typed_at_a_value_prompt_is_the_tools_answer_even_when_it_reads_zoom() {
        // Quick Select's layer prompt takes any name, "Z" included; only 'Z goes to the view
        // from there.
        let mut document = Document::new(None);
        let mut host = ToolHost::new();
        let transparent = said();
        let heard = Rc::clone(&transparent);
        host.on_transparent = Some(transparent_hook(move |_host, _document, command| {
            heard.borrow_mut().push(String::from(command));
            true
        }));
        host.start(&mut document, "select.quick")
            .expect("the tool starts");
        assert_eq!(host.typed(&mut document, "Any"), Outcome::Continue);
        assert_eq!(host.typed(&mut document, "'Z"), Outcome::Continue);
        assert_eq!(*transparent.borrow(), ["'Z"]);
        // The layer, "Z".
        assert_eq!(host.typed(&mut document, "Z"), Outcome::Continue);
        assert_eq!(transparent.borrow().len(), 1);
        let prompt = host.prompt(&document);
        assert!(prompt.contains("Condition"), "{prompt}");
    }

    #[test]
    fn a_selecting_tools_answer_is_left_in_the_documents_selection() {
        let mut document = Document::new(None);
        let mut lines = Vec::new();
        for y in [0.0, 1.0, 2.0] {
            lines.push(add_line(&mut document, p(0.0, y), p(1.0, y)));
        }
        document
            .execute(create_circle(p(5.0, 5.0), 1.0, EntityAttributes::default()))
            .expect("the circle is made");
        let circle = document.last_created_entities()[0];
        document.selection_mut().set(vec![lines[0]]);
        let notified = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&notified);
        let _listening = document.add_listener(move || {
            counter.fetch_add(1, Ordering::AcqRel);
        });
        let mut host = ToolHost::new();
        host.start(&mut document, "select.similar")
            .expect("the tool starts");
        assert_eq!(host.enter(&mut document), Outcome::Done);
        // The three lines on layer 0, not the circle.
        assert_eq!(document.selection().ids(), lines);
        assert!(!document.selection().contains(circle));
        assert!(notified.load(Ordering::Acquire) >= 1);
        assert_eq!(
            document.history().undo_count(),
            4,
            "no command for a selection"
        );
    }

    #[test]
    fn a_snap_on_an_entitys_point_reaches_the_tool_as_that_point() {
        // A leader's tip snapped to a line's end follows the line's End. One snapped to somewhere
        // along it (Nearest) names no point of the line, so the snap hands the tool a plain
        // point - and the Leader puts a tip on what it is clicked on (docs/annotation.md, "Smart
        // leaders"): it goes on the line at the place nearest, 4 of the line's 10 along, and
        // follows it there.
        let mut document = Document::new(None);
        let line = add_line(&mut document, p(0.0, 0.0), p(10.0, 0.0));
        let mut host = ToolHost::new();
        let mut leader = |snap: SnapResult| -> LeaderGeometry {
            host.start(&mut document, "annotate.leader")
                .expect("the leader tool starts");
            let _ = host.point(&mut document, snap.point, Some(&snap));
            let _ = host.point(&mut document, p(15.0, 5.0), None);
            let _ = host.enter(&mut document);
            let _ = host.enter(&mut document);
            host.cancel(&mut document);
            let made = document.last_created_entities();
            assert_eq!(made.len(), 1);
            match &document
                .model()
                .entities
                .find(made[0])
                .expect("the leader is there")
                .geometry
            {
                Geometry::Leader(leader) => leader.clone(),
                other => panic!("not a leader: {other:?}"),
            }
        };
        let snapped = leader(SnapResult {
            point: p(10.0, 0.0),
            mode: SnapMode::Endpoint,
            entity: line,
        });
        assert_eq!(snapped.tip_ref.entity, line);
        assert_eq!(snapped.tip_ref.point, AnchorPoint::End);
        assert_eq!(snapped.tip_ref.index, 0);
        assert_eq!(snapped.tip_ref.parameter, 0.0);
        let along = leader(SnapResult {
            point: p(4.0, 0.0),
            mode: SnapMode::Nearest,
            entity: line,
        });
        assert_eq!(along.tip_ref.entity, line);
        assert_eq!(along.tip_ref.point, AnchorPoint::Along);
        assert!(
            (along.tip_ref.parameter - 0.4).abs() <= 4.0 * f64::EPSILON * 0.4,
            "{}",
            along.tip_ref.parameter
        );
        assert_eq!(along.vertices[0], p(4.0, 0.0));
    }

    #[test]
    fn a_click_the_tool_took_is_not_answered_in_red_until_the_pointer_leaves_it() {
        // Insert Vertex restarts after each vertex with the cursor on the vertex it made, where a
        // second click WOULD be refused as too close - and the view said so in red straight
        // after every success. Until the cursor leaves the click's place, what the click did is
        // shown instead.
        let mut document = Document::new(None);
        let polyline = add_polyline(&mut document, vec![p(0.0, 0.0), p(10.0, 0.0)]);
        document.selection_mut().add(polyline);
        let mut host = ToolHost::new();
        host.set_pick_tolerance(0.5);
        host.start(&mut document, "draw.vertex.insert")
            .expect("the tool starts");
        host.set_cursor(p(4.0, 0.1));
        assert_eq!(host.point(&mut document, p(4.0, 0.1), None), Outcome::Done);
        let held = host.feedback(&document, p(4.0, 0.1));
        assert!(!held.refused);
        assert!(held.marks.is_empty());
        assert_eq!(
            held.caption,
            format!("vertex 1 added to polyline {polyline} on segment 0 (3 vertices)")
        );
        assert_eq!(held.focus, Some(polyline));
        // Away - 4 units, beyond the 0.5 aperture - and back: now the tool's refusal is what a
        // click there would get, and it is shown.
        host.set_cursor(p(8.0, 0.1));
        assert!(
            !host.feedback(&document, p(8.0, 0.1)).refused,
            "8 is clear of every vertex"
        );
        host.set_cursor(p(4.0, 0.1));
        let refused = host.feedback(&document, p(4.0, 0.1));
        assert!(refused.refused);
        assert!(
            refused.caption.starts_with("too close to vertex 1"),
            "{}",
            refused.caption
        );
        // A click the tool refuses is no success to spare: its refusal shows.
        assert_eq!(
            host.point(&mut document, p(4.0, 0.1), None),
            Outcome::Rejected
        );
        assert!(host.feedback(&document, p(4.0, 0.1)).refused);
    }

    #[test]
    fn straightens_first_pick_is_held_as_taken_and_its_refusal_shown_once_the_pointer_comes_back()
    {
        // A second click on Straighten's first vertex is refused, so the tool's preview there is
        // the refusal (R3). Straight after the pick, the hold shows the pick as taken - its
        // "keep" ring, not red - and once the pointer has left and come back, the refusal. The
        // hold used to keep nothing but the polyline, so the ring the click had just made went.
        let mut document = Document::new(None);
        let polyline = add_polyline(
            &mut document,
            vec![
                p(0.0, 0.0),
                p(1.0, 1.0),
                p(2.0, -1.0),
                p(3.0, 1.0),
                p(4.0, 0.0),
            ],
        );
        document.selection_mut().add(polyline);
        let mut host = ToolHost::new();
        host.set_pick_tolerance(0.5);
        host.start(&mut document, "draw.vertex.straighten")
            .expect("the tool starts");
        host.set_cursor(p(0.0, 0.1));
        assert_eq!(
            host.point(&mut document, p(0.0, 0.1), None),
            Outcome::Continue
        );
        let keep = |feedback: &ToolFeedback| {
            feedback
                .marks
                .iter()
                .filter(|mark| {
                    mark.role == FeedbackRole::Target
                        && mark.label == "keep"
                        && matches!(
                            &mark.geometry,
                            Geometry::Point(point) if point.position == p(0.0, 0.0)
                        )
                })
                .count()
        };
        let held = host.feedback(&document, p(0.0, 0.1));
        assert!(!held.refused);
        assert_eq!(keep(&held), 1);
        host.set_cursor(p(2.0, 3.0));
        host.set_cursor(p(0.0, 0.1));
        let back = host.feedback(&document, p(0.0, 0.1));
        assert!(back.refused);
        assert_eq!(
            back.caption,
            "vertex 0 is picked already; click the other one"
        );
        assert_eq!(keep(&back), 1, "the pick's mark, drawn as the refusal");
    }

    #[test]
    fn a_refusal_that_drops_a_stale_pick_says_the_new_prompt_and_no_other_repeats_it() {
        // Insert beside vertex 1, then another edit moves vertex 2: the next click is refused as
        // stale and drops the choice, so the tool asks afresh - and the window's command line,
        // which hears a prompt only through on_prompt, went on offering Enter for the vertex
        // dropped. A refusal that leaves the prompt as it was says nothing more: the status bar
        // keeps the refusal it shows.
        let mut document = Document::new(None);
        let polyline = add_polyline(
            &mut document,
            vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)],
        );
        document.selection_mut().add(polyline);
        let mut host = ToolHost::new();
        host.set_pick_tolerance(0.5);
        let prompts = said();
        let heard = Rc::clone(&prompts);
        host.on_prompt = Some(text_hook(move |_host, _document, prompt| {
            heard.borrow_mut().push(String::from(prompt));
        }));
        host.set_handles(vec![Grip {
            entity: polyline,
            kind: GripKind::Vertex,
            index: 1,
            position: p(10.0, 0.0),
        }]);
        host.start(&mut document, "draw.vertex.insert")
            .expect("the tool starts");
        let last = prompts.borrow().last().cloned().unwrap_or_default();
        assert!(
            last.starts_with("Press Enter for a vertex at the marked middle of segment 1"),
            "{last}"
        );
        document
            .execute(edit_polyline(
                polyline,
                "VERTEX_MOVE",
                |shape: &CurvePolyline2| move_vertex(shape, 2, p(20.0, 10.0)),
            ))
            .expect("vertex 2 is moved");
        assert_eq!(
            host.point(&mut document, p(10.2, 5.0), None),
            Outcome::Rejected
        );
        assert_eq!(
            prompts.borrow().last().cloned().unwrap_or_default(),
            format!("Click on polyline {polyline} where the new vertex goes")
        );
        let said_so_far = prompts.borrow().len();
        assert_eq!(
            host.point(&mut document, p(50.0, 50.0), None),
            Outcome::Rejected,
            "nothing there"
        );
        assert_eq!(prompts.borrow().len(), said_so_far);
    }
}
