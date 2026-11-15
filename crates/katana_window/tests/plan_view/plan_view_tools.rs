//! The twin of tests/qt_widgets/tools/test_plan_view_tools.cpp: the catalogue's tools hosted in a
//! real plan view (viewport_widget with tools/tool_host) - clicks, typing, Enter, Esc and a
//! right-click sent to the view as its events, and the result read back from the Document, its
//! entities and its undo history.
//!
//! Every view here is 400 x 300 pixels, centred on the model origin at 10 pixels a unit, and never
//! framed, so a pixel's model point is worked by hand from ViewTransform::screen_to_world:
//!     x = (px - 200) / 10        y = -(py - 150) / 10
//! e.g. pixel (300, 50) is model (10, 10) and pixel (250, 100) is (5, 5).

use katana_cad::interactive_tool::tool_catalog;
use katana_cad::snapping::{SnapMode, SnapModes};
use katana_commands::{EntityAttributes, create_entities, create_layer, create_line};
use katana_core::ErrorCode;
use katana_entity::tables::DEFAULT_LAYER_NAME;
use katana_entity::{AnchorPoint, Entity, EntityId, Geometry, Layer};
use katana_geometry::{Point2, Segment2};
use katana_window::canvas::PointF;
use katana_window::viewport_widget::{KeyModifiers, MouseButton, Tool, ViewKey, tool_id};

use crate::support::Plan;

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

/// The tools tests' view: the window takes the view's errors and the text it has no use for.
fn fixture() -> Plan {
    let mut plan = Plan::at_origin();
    plan.view.hooks.text_typed = true;
    plan
}

#[track_caller]
fn expect_segment(entity: Option<&Entity>, start: Point2, end: Point2) {
    let entity = entity.expect("the entity is there");
    let Geometry::Line(segment) = &entity.geometry else {
        panic!("not a line: {:?}", entity.geometry);
    };
    assert!((segment.start.x - start.x).abs() <= 1e-9, "{segment:?}");
    assert!((segment.start.y - start.y).abs() <= 1e-9, "{segment:?}");
    assert!((segment.end.x - end.x).abs() <= 1e-9, "{segment:?}");
    assert!((segment.end.y - end.y).abs() <= 1e-9, "{segment:?}");
}

fn layer(plan: &mut Plan, name: &str) {
    plan.document
        .execute(create_layer(Layer {
            name: String::from(name),
            ..Layer::default()
        }))
        .expect("the layer is made");
}

mod plan_view_tools {
    use super::*;

    #[test]
    fn a_line_from_the_catalogue_is_two_clicks_and_enter_and_one_undo_step() {
        let mut plan = fixture();
        plan.start_tool("draw.line").expect("the tool starts");
        assert_eq!(plan.view.active_tool_id(), "draw.line");

        plan.press(200.0, 150.0); // model (0, 0)
        plan.press(300.0, 50.0); // model (10, 10)
        assert!(
            plan.entities().is_empty(),
            "the chain is the tool's until Enter"
        );
        plan.enter();

        let made = plan.entities();
        assert_eq!(made.len(), 1);
        expect_segment(made.first(), p(0.0, 0.0), p(10.0, 10.0));
        assert_eq!(plan.undo_steps(), 1);
        assert!(plan.errors().is_empty(), "{:?}", plan.errors());
    }

    #[test]
    fn a_typed_relative_point_is_measured_from_the_last_click() {
        let mut plan = fixture();
        plan.start_tool("draw.line").expect("the tool starts");
        plan.press(250.0, 100.0); // model (5, 5)
        plan.type_text("@10,0"); // (5 + 10, 5 + 0) = (15, 5)
        assert_eq!(
            plan.view.typed_input(),
            "@10,0",
            "shown after the prompt until Enter"
        );
        assert!(
            plan.typed_out().is_empty(),
            "a tool's input is not a command for the window"
        );
        plan.enter(); // sends the typed point
        plan.enter(); // finishes the chain

        let made = plan.entities();
        assert_eq!(made.len(), 1);
        expect_segment(made.first(), p(5.0, 5.0), p(15.0, 5.0));
        assert_eq!(plan.undo_steps(), 1);
    }

    #[test]
    fn a_polar_point_typed_after_a_click_goes_that_far_at_that_angle() {
        let mut plan = fixture();
        plan.start_tool("draw.line").expect("the tool starts");
        plan.press(200.0, 150.0); // model (0, 0)
        // @5<90: 5 units at 90 degrees counter-clockwise from east, due north.
        plan.type_text("@5<90");
        plan.enter();
        plan.enter();

        let made = plan.entities();
        assert_eq!(made.len(), 1);
        expect_segment(made.first(), p(0.0, 0.0), p(0.0, 5.0));
    }

    #[test]
    fn esc_after_one_click_creates_nothing_and_ends_the_tool() {
        let mut plan = fixture();
        plan.start_tool("draw.line").expect("the tool starts");
        plan.press(200.0, 150.0);
        plan.escape();

        assert!(plan.entities().is_empty());
        assert_eq!(plan.undo_steps(), 0);
        assert!(!plan.view.tool_active());
    }

    #[test]
    fn esc_after_a_segment_keeps_the_lines_drawn_as_auto_cad_does() {
        // A LINE's segments are finished work once both ends are given; Esc ends the tool, and
        // keeps them (ToolHost::cancel, the Line tool's own InteractiveTool::cancel).
        let mut plan = fixture();
        plan.start_tool("draw.line").expect("the tool starts");
        plan.press(200.0, 150.0); // (0, 0)
        plan.press(300.0, 150.0); // (10, 0)
        plan.escape();

        let made = plan.entities();
        assert_eq!(made.len(), 1);
        expect_segment(made.first(), p(0.0, 0.0), p(10.0, 0.0));
        assert_eq!(plan.undo_steps(), 1);
        assert!(!plan.view.tool_active());
    }

    #[test]
    fn the_first_esc_takes_back_what_was_typed_and_leaves_the_tool_running() {
        let mut plan = fixture();
        plan.start_tool("draw.line").expect("the tool starts");
        plan.press(200.0, 150.0);
        plan.type_text("@3");
        plan.escape();
        assert!(plan.view.typed_input().is_empty());
        assert!(plan.view.tool_active());
    }

    #[test]
    fn the_rubber_band_is_drawn_while_hovering_after_the_first_point() {
        let mut plan = fixture();
        plan.start_tool("draw.line").expect("the tool starts");
        plan.move_to(300.0, 150.0);
        plan.paint();
        // Before the first point Line previews nothing: no segment, no marker.
        assert_eq!(plan.view.last_preview_count(), 0);

        plan.press(200.0, 150.0); // (0, 0)
        plan.move_to(300.0, 150.0); // (10, 0)
        plan.paint();
        // The band from (0, 0) to the cursor, and a marker on (0, 0): 1 + 1.
        assert_eq!(plan.view.last_preview_count(), 2);
        let feedback = plan.view.tool_host().feedback(&plan.document, p(10.0, 0.0));
        assert_eq!(feedback.shapes.len(), 1);
        let Geometry::Line(band) = &feedback.shapes[0] else {
            panic!("the band is no line: {:?}", feedback.shapes[0]);
        };
        assert_eq!(band.start, p(0.0, 0.0));
        assert_eq!(band.end, p(10.0, 0.0));
    }

    #[test]
    fn no_preview_is_drawn_before_the_pointer_has_moved() {
        let mut plan = fixture();
        plan.start_tool("draw.line").expect("the tool starts");
        plan.type_text("5,5");
        plan.enter();
        plan.paint();
        // The pointer has never been over the view: the cursor is the model's origin, where
        // nobody pointed, and a band to it would lead nowhere.
        assert_eq!(plan.view.last_preview_count(), 0);
        plan.move_to(300.0, 150.0); // (10, 0)
        plan.paint();
        // The band from (5, 5) to the cursor and the marker on (5, 5): 1 + 1.
        assert_eq!(plan.view.last_preview_count(), 2);
    }

    #[test]
    fn no_preview_is_drawn_while_the_pointer_is_outside_the_view() {
        // Off to a menu or a toolbar, the pointer leaves the view where it last was: a tool
        // chosen there previewed at that spot - Move Vertex dragged a vertex to the view's edge,
        // Insert Vertex refused at a vertex nobody pointed at. Nothing is drawn until the pointer
        // is back.
        let mut plan = fixture();
        plan.start_tool("draw.line").expect("the tool starts");
        plan.type_text("5,5");
        plan.enter();
        plan.move_to(300.0, 150.0); // (10, 0)
        plan.paint();
        assert_eq!(
            plan.view.last_preview_count(),
            2,
            "the band and the marker on (5, 5)"
        );
        plan.view.leave();
        plan.paint();
        assert_eq!(plan.view.last_preview_count(), 0);
        plan.move_to(250.0, 50.0); // (5, 10)
        plan.paint();
        assert_eq!(
            plan.view.last_preview_count(),
            2,
            "back over the view, from where it is now"
        );
    }

    #[test]
    fn a_right_click_is_enter_while_a_tool_runs() {
        let mut plan = fixture();
        plan.view.hooks.context_menu = true;
        plan.start_tool("draw.line").expect("the tool starts");
        plan.press(200.0, 150.0); // (0, 0)
        plan.press(200.0, 50.0); // (0, 10)
        plan.press_with(200.0, 50.0, MouseButton::Right, KeyModifiers::NONE); // Enter

        let made = plan.entities();
        assert_eq!(made.len(), 1);
        expect_segment(made.first(), p(0.0, 0.0), p(0.0, 10.0));
        assert_eq!(
            plan.menus(),
            0,
            "the shortcut menu is for selecting, not for a tool"
        );
    }

    #[test]
    fn ctrl_z_inside_a_tool_steps_back_a_point_not_the_drawing() {
        let mut plan = fixture();
        let _ = plan.add_line(p(-50.0, -50.0), p(-40.0, -50.0));
        plan.start_tool("draw.line").expect("the tool starts");
        plan.press(200.0, 150.0); // (0, 0)
        plan.press(300.0, 150.0); // (10, 0)
        plan.press(300.0, 50.0); // (10, 10)
        plan.key(ViewKey::Z, KeyModifiers::CTRL, "");
        plan.enter();

        // The existing line is still there, and the chain lost (10, 10): (0, 0)-(10, 0) and the
        // line from before.
        let made = plan.entities();
        assert_eq!(made.len(), 2);
        expect_segment(made.last(), p(0.0, 0.0), p(10.0, 0.0));
        assert_eq!(plan.undo_steps(), 2);
    }

    #[test]
    fn ctrl_z_with_nothing_to_step_back_leaves_the_drawing_alone_and_says_so() {
        let mut plan = fixture();
        let _ = plan.add_line(p(-50.0, -50.0), p(-40.0, -50.0));
        plan.start_tool("draw.line").expect("the tool starts");
        plan.key(ViewKey::Z, KeyModifiers::CTRL, "");

        assert_eq!(plan.entities().len(), 1);
        assert_eq!(plan.undo_steps(), 1);
        assert_eq!(
            plan.errors().len(),
            1,
            "the tool's refusal, not silence"
        );
        assert!(plan.view.tool_active());
    }

    #[test]
    fn a_click_snaps_to_the_end_of_a_line_near_it() {
        let mut plan = fixture();
        let _ = plan.add_line(p(0.0, 0.0), p(10.0, 0.0));
        plan.set_snap_enabled(true);
        plan.view
            .set_snap_modes(&mut plan.document, SnapMode::Endpoint as SnapModes);
        plan.start_tool("draw.line").expect("the tool starts");
        // Pixel (302, 152) is model (10.2, -0.2), 0.28 from the end (10, 0) and inside the
        // 12-pixel aperture (1.2 units at this scale).
        plan.press(302.0, 152.0);
        plan.set_snap_enabled(false);
        plan.press(300.0, 50.0); // (10, 10)
        plan.enter();

        let made = plan.entities();
        assert_eq!(made.len(), 2);
        expect_segment(made.last(), p(10.0, 0.0), p(10.0, 10.0));
    }

    #[test]
    fn move_acts_on_the_selection_it_started_with() {
        let mut plan = fixture();
        let id = plan.add_line(p(0.0, 0.0), p(10.0, 0.0));
        plan.document.selection_mut().add(id);
        plan.start_tool("modify.move").expect("the tool starts");
        plan.press(200.0, 150.0); // base (0, 0)
        plan.press(250.0, 100.0); // to (5, 5): the displacement is (5, 5)

        expect_segment(plan.entity(id), p(5.0, 5.0), p(15.0, 5.0));
        assert_eq!(plan.undo_steps(), 2, "the line, then the move");
        assert!(
            !plan.view.tool_active(),
            "Move ends; its next use needs a new selection"
        );
    }

    #[test]
    fn a_tools_selection_step_gathers_clicks_until_enter() {
        // Erase always asks. Two plain clicks pick both lines, as AutoCAD's "Select objects"
        // gathers; the Select tool would keep only the last.
        let mut plan = fixture();
        let _ = plan.add_line(p(0.0, 0.0), p(10.0, 0.0)); // through pixel (250, 150)
        let _ = plan.add_line(p(0.0, 10.0), p(10.0, 10.0)); // through pixel (250, 50)
        plan.start_tool("modify.erase").expect("the tool starts");
        plan.press(250.0, 150.0);
        plan.press(250.0, 50.0);
        assert_eq!(plan.document.selection().ids().len(), 2);
        plan.enter();

        assert!(plan.entities().is_empty());
        assert_eq!(plan.undo_steps(), 3, "two lines, then one erase");
    }

    #[test]
    fn a_shift_window_at_a_tools_selection_step_takes_what_it_encloses_back_out() {
        // AutoCAD's "Select objects": Shift with a window removes. The line (0, 0)-(10, 0) lies
        // inside the window from pixel (180, 130) to (320, 170), model (-2, 2) to (12, -2),
        // dragged left to right.
        let mut plan = fixture();
        let id = plan.add_line(p(0.0, 0.0), p(10.0, 0.0));
        plan.start_tool("modify.erase").expect("the tool starts");
        plan.press(250.0, 150.0); // (5, 0), on the line
        assert!(plan.document.selection().contains(id));
        plan.drag_pixels(
            PointF::new(180.0, 130.0),
            PointF::new(320.0, 170.0),
            KeyModifiers::SHIFT,
        );

        assert!(!plan.document.selection().contains(id));
        plan.enter(); // nothing to erase: refused, and the line stays
        assert!(plan.entity(id).is_some());
        assert_eq!(plan.undo_steps(), 1);
    }

    #[test]
    fn a_shift_or_ctrl_click_at_a_tools_selection_step_takes_a_pick_back_out() {
        // Line a through pixel (250, 150), line b through (250, 50). Both are picked;
        // Shift-clicking a takes it out, and Ctrl-clicking a again does not put it back (Ctrl no
        // longer toggles here, it removes), so Enter erases b alone.
        let mut plan = fixture();
        let a = plan.add_line(p(0.0, 0.0), p(10.0, 0.0));
        let b = plan.add_line(p(0.0, 10.0), p(10.0, 10.0));
        plan.start_tool("modify.erase").expect("the tool starts");
        plan.press(250.0, 150.0);
        plan.press(250.0, 50.0);
        plan.press_with(250.0, 150.0, MouseButton::Left, KeyModifiers::SHIFT);
        assert!(!plan.document.selection().contains(a));
        assert!(plan.document.selection().contains(b));
        plan.press_with(250.0, 150.0, MouseButton::Left, KeyModifiers::CTRL);
        assert!(!plan.document.selection().contains(a));
        plan.enter();

        assert!(plan.entity(a).is_some());
        assert!(plan.entity(b).is_none());
    }

    #[test]
    fn a_click_at_a_tools_selection_step_sends_out_the_prompt_that_counts_it() {
        // Trim's edge prompt counts the edges from the document's selection. The command line
        // and status bar hear the prompt only through the Prompt event, so a click there must
        // send it again. Two lines picked, none by the tool itself: "Select cutting edges" +
        // " <use 2 edges>" (EdgeTool::prompt).
        let mut plan = fixture();
        let _ = plan.add_line(p(0.0, 0.0), p(10.0, 0.0)); // through pixel (250, 150)
        let _ = plan.add_line(p(0.0, 10.0), p(10.0, 10.0)); // through pixel (250, 50)
        plan.log.clear();
        plan.start_tool("modify.trim").expect("the tool starts");
        plan.press(250.0, 150.0);
        plan.press(250.0, 50.0);

        let prompts = plan.prompts();
        assert_eq!(
            prompts.last().map(String::as_str),
            Some("Trim: Select cutting edges <use 2 edges>")
        );
    }

    #[test]
    fn ctrl_z_at_a_tools_selection_step_takes_back_the_last_click() {
        // The clicks changed the document's selection, which the tool reads at Enter; the tool
        // never saw them, so its own undo would say nothing was picked while both edges stayed
        // picked.
        let mut plan = fixture();
        let a = plan.add_line(p(0.0, 0.0), p(10.0, 0.0));
        let b = plan.add_line(p(0.0, 10.0), p(10.0, 10.0));
        plan.start_tool("modify.trim").expect("the tool starts");
        plan.press(250.0, 150.0);
        plan.press(250.0, 50.0);

        plan.key(ViewKey::Z, KeyModifiers::CTRL, "");
        assert!(plan.document.selection().contains(a));
        assert!(!plan.document.selection().contains(b));
        plan.key(ViewKey::Z, KeyModifiers::CTRL, "");
        assert!(plan.document.selection().empty());
        assert!(plan.errors().is_empty(), "{:?}", plan.errors());

        plan.key(ViewKey::Z, KeyModifiers::CTRL, ""); // nothing left: the tool says so
        assert_eq!(plan.errors().len(), 1);
        assert!(plan.view.tool_active());
        assert_eq!(
            plan.undo_steps(),
            2,
            "the drawing is never undone under a tool"
        );
    }

    #[test]
    fn u_typed_at_a_tools_selection_step_takes_back_the_last_click_as_ctrl_z_does() {
        let mut plan = fixture();
        let a = plan.add_line(p(0.0, 0.0), p(10.0, 0.0));
        let b = plan.add_line(p(0.0, 10.0), p(10.0, 10.0));
        plan.start_tool("modify.erase").expect("the tool starts");
        plan.press(250.0, 150.0);
        plan.press(250.0, 50.0);
        plan.type_text("u");
        plan.enter();
        plan.enter(); // erases what is left picked: a

        assert!(plan.entity(a).is_none());
        assert!(plan.entity(b).is_some());
    }

    #[test]
    fn ctrl_z_after_a_typed_all_takes_back_the_all_before_the_click_before_it() {
        // Steps back in the order they were made: the click on a, then All (which the tool holds
        // itself), so Ctrl+Z takes back All and leaves a picked, and Enter erases a alone.
        let mut plan = fixture();
        let a = plan.add_line(p(0.0, 0.0), p(10.0, 0.0));
        let b = plan.add_line(p(0.0, 10.0), p(10.0, 10.0));
        plan.start_tool("modify.erase").expect("the tool starts");
        plan.press(250.0, 150.0);
        plan.type_text("ALL");
        plan.enter();
        plan.key(ViewKey::Z, KeyModifiers::CTRL, "");
        plan.enter();

        assert!(plan.entity(a).is_none());
        assert!(plan.entity(b).is_some());
    }

    #[test]
    fn text_typed_with_no_tool_running_still_goes_to_the_window() {
        let mut plan = fixture();
        plan.type_text("L");
        assert_eq!(plan.typed_out(), ["L"]);
    }

    #[test]
    fn the_legacy_tools_start_their_catalogue_tools() {
        let mut plan = fixture();
        for tool in [
            Tool::Point,
            Tool::Line,
            Tool::Polyline,
            Tool::Rectangle,
            Tool::Circle,
            Tool::Arc,
            Tool::Move,
            Tool::Copy,
        ] {
            assert!(
                tool_catalog().find(tool_id(tool)).is_some(),
                "{}",
                tool.name()
            );
            plan.set_tool(tool);
            assert_eq!(plan.view.active_tool_id(), tool_id(tool));
            assert_eq!(plan.view.tool(), tool);
        }
        plan.set_tool(Tool::Select);
        assert!(!plan.view.tool_active());
        assert_eq!(plan.view.tool(), Tool::Select);
    }

    #[test]
    fn an_unknown_tool_id_is_refused_and_the_running_tool_runs_on() {
        let mut plan = fixture();
        plan.start_tool("draw.line").expect("the tool starts");
        let refused = plan
            .start_tool("draw.nothing")
            .expect_err("there is no such tool");
        assert_eq!(refused.code, ErrorCode::NotFound);
        assert_eq!(plan.view.active_tool_id(), "draw.line");
    }

    #[test]
    fn the_layer_new_work_is_drawn_on_is_the_documents_current_layer() {
        let mut plan = fixture();
        layer(&mut plan, "Kerbs");
        plan.document
            .set_current_layer("Kerbs")
            .expect("Kerbs is made current");
        plan.start_tool("draw.line").expect("the tool starts");
        plan.press(200.0, 150.0);
        plan.press(300.0, 150.0);
        plan.enter();

        let made = plan.entities();
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].layer, "Kerbs");
    }

    #[test]
    fn a_tool_running_across_a_new_drawing_ends_and_its_next_start_draws_on_the_new_drawing() {
        // The main window's new document calls reset_interaction BEFORE it replaces the document.
        // A tool rebuilt at that moment read the old drawing's current layer, Kerbs, and its
        // first chain in the new drawing was refused (no such layer); Move kept ids from the
        // discarded drawing. The tool ends instead, so the order of the two calls no longer
        // matters.
        let mut plan = fixture();
        layer(&mut plan, "Kerbs");
        plan.document
            .set_current_layer("Kerbs")
            .expect("Kerbs is made current");
        plan.log.clear();
        plan.start_tool("draw.line").expect("the tool starts");
        plan.press(200.0, 150.0);

        // In the main window's order.
        plan.view.reset_interaction(&mut plan.document);
        plan.drain();
        plan.document.new_document();
        assert!(!plan.view.tool_active());
        assert_eq!(
            plan.active_tool_changes().last().map(String::as_str),
            Some(""),
            "the toolbar hears that the tool ended"
        );

        plan.start_tool("draw.line").expect("the tool starts");
        plan.press(200.0, 150.0); // (0, 0)
        plan.press(300.0, 150.0); // (10, 0)
        plan.enter();
        let made = plan.entities();
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].layer, DEFAULT_LAYER_NAME);
        expect_segment(made.first(), p(0.0, 0.0), p(10.0, 0.0));
        assert!(plan.errors().is_empty(), "{:?}", plan.errors());
    }

    #[test]
    fn enter_at_no_prompt_repeats_the_last_tool() {
        let mut plan = fixture();
        plan.start_tool("draw.line").expect("the tool starts");
        plan.press(200.0, 150.0);
        plan.press(300.0, 150.0);
        plan.enter(); // the line; Line starts again
        plan.enter(); // Enter at its first prompt ends it
        assert!(!plan.view.tool_active());

        plan.enter();
        assert_eq!(plan.view.active_tool_id(), "draw.line");
        assert_eq!(
            plan.entities().len(),
            1,
            "repeating a tool draws nothing by itself"
        );
    }

    #[test]
    fn esc_after_a_trim_keeps_the_cut_as_one_undo_step() {
        // The Modify Edit report's case: Trim holds its cuts in a session that only Enter
        // commits, so an Esc that dropped the tool would lose them. The horizontal line
        // (0, 0)-(20, 0) is crossed by the vertical one at x = 10; picking it at (15, 0) - pixel
        // (350, 150) - takes away the part beyond the crossing, leaving (0, 0)-(10, 0).
        let mut plan = fixture();
        let _ = plan.add_line(p(0.0, 0.0), p(20.0, 0.0));
        let _ = plan.add_line(p(10.0, -5.0), p(10.0, 5.0));
        plan.start_tool("modify.trim").expect("the tool starts");
        plan.enter(); // every object is a cutting edge
        plan.press(350.0, 150.0); // the part to take away
        assert_eq!(
            plan.undo_steps(),
            2,
            "the cut is the tool's until Enter or Esc"
        );
        plan.escape();

        assert!(!plan.view.tool_active());
        assert_eq!(plan.undo_steps(), 3);
        let entities = plan.entities();
        let kept = entities.iter().rfind(|entity| {
            matches!(
                &entity.geometry,
                Geometry::Line(segment) if segment.start.y == 0.0 && segment.end.y == 0.0
            )
        });
        expect_segment(kept, p(0.0, 0.0), p(10.0, 0.0));
    }

    #[test]
    fn esc_at_the_radius_prompt_keeps_the_corners_a_multiple_fillet_made() {
        // Enter at Fillet's radius prompt only returns to the lines, so an Esc that sent Enter
        // there and then dropped the tool lost the corners the Multiple session had made. Line a
        // (0, 0)-(12, 0) and line b (10, -2)-(10, 10) meet at (10, 0). Picking a at (5, 0) -
        // pixel (250, 150) - and b at (10, 5) - pixel (300, 100) - keeps the picked sides, and
        // radius 0 makes a sharp corner: a becomes (0, 0)-(10, 0), b (10, 0)-(10, 10).
        let mut plan = fixture();
        let a = plan.add_line(p(0.0, 0.0), p(12.0, 0.0));
        let b = plan.add_line(p(10.0, -2.0), p(10.0, 10.0));
        plan.start_tool("modify.fillet").expect("the tool starts");
        plan.type_text("R"); // the radius is remembered for the program: set it here
        plan.enter();
        plan.type_text("0");
        plan.enter();
        plan.type_text("M");
        plan.enter();
        plan.press(250.0, 150.0);
        plan.press(300.0, 100.0);
        assert_eq!(
            plan.undo_steps(),
            2,
            "the corner is the tool's until Enter or Esc"
        );
        plan.type_text("R");
        plan.enter();
        let prompt = plan.view.tool_host().prompt(&plan.document);
        assert!(prompt.contains("radius <"), "{prompt}");
        plan.escape();

        assert!(!plan.view.tool_active());
        assert_eq!(plan.undo_steps(), 3, "the two lines, then one FILLET");
        expect_segment(plan.entity(a), p(0.0, 0.0), p(10.0, 0.0));
        expect_segment(plan.entity(b), p(10.0, 0.0), p(10.0, 10.0));
    }

    #[test]
    fn esc_at_the_second_chamfer_distance_leaves_the_remembered_distances_alone() {
        // Enter at the second distance takes the first as the second and stores both for every
        // later Chamfer; Esc must never apply a default. The distances are remembered for the
        // program, so the test compares the prompt before with the prompt after rather than
        // assuming what it says.
        let mut plan = fixture();
        plan.start_tool("modify.chamfer").expect("the tool starts");
        let before = plan.view.tool_host().prompt(&plan.document);
        plan.type_text("D");
        plan.enter();
        plan.type_text("3.25");
        plan.enter();
        let prompt = plan.view.tool_host().prompt(&plan.document);
        assert!(
            prompt.contains("second chamfer distance <3.25>"),
            "{prompt}"
        );
        plan.escape();
        assert!(!plan.view.tool_active());

        plan.start_tool("modify.chamfer").expect("the tool starts");
        assert_eq!(plan.view.tool_host().prompt(&plan.document), before);
        assert_eq!(plan.undo_steps(), 0);
    }

    #[test]
    fn esc_from_a_move_at_its_second_point_moves_nothing() {
        // Move's Enter at the second point moves by the base point as a displacement (AutoCAD's
        // default); Esc must never apply it.
        let mut plan = fixture();
        let id = plan.add_line(p(0.0, 0.0), p(10.0, 0.0));
        plan.document.selection_mut().add(id);
        plan.start_tool("modify.move").expect("the tool starts");
        plan.press(250.0, 100.0); // base (5, 5)
        plan.escape();

        expect_segment(plan.entity(id), p(0.0, 0.0), p(10.0, 0.0));
        assert_eq!(plan.undo_steps(), 1);
    }

    #[test]
    fn select_is_reported_to_the_window_even_with_nothing_running() {
        // The window checks its Select button from the ToolChanged event at start-up, when no
        // tool has run yet.
        let mut plan = fixture();
        plan.log.clear();
        plan.set_tool(Tool::Select);
        assert_eq!(plan.tool_changes(), [Tool::Select]);

        plan.set_tool(Tool::Line);
        plan.set_tool(Tool::Select);
        assert_eq!(
            plan.tool_changes(),
            [Tool::Select, Tool::Line, Tool::Select]
        );
    }

    // ---- the everyday tools in a real view -------------------------------------------------

    #[test]
    fn esc_after_two_copies_keeps_both_as_one_undo_step() {
        // Copy's placed copies are work the user clicked for; Esc keeps them now, where the
        // host's old list dropped them.
        let mut plan = fixture();
        plan.document
            .execute(create_line(
                p(0.0, 0.0),
                p(1.0, 0.0),
                EntityAttributes::default(),
            ))
            .expect("the line is made");
        let line = plan.document.last_created_entities()[0];
        plan.document.selection_mut().add(line);
        plan.start_tool("modify.copy").expect("the tool starts");
        plan.press(200.0, 150.0); // base (0, 0)
        plan.press(250.0, 150.0); // (5, 0)
        plan.press(300.0, 150.0); // (10, 0)
        plan.escape();
        assert!(!plan.view.tool_active());
        assert_eq!(plan.entities().len(), 3);
        assert_eq!(
            plan.undo_steps(),
            2,
            "the line, then one step for both copies"
        );
    }

    #[test]
    fn distance_between_two_clicks_reaches_the_log_as_an_inverse() {
        let mut plan = fixture();
        plan.log.clear();
        plan.start_tool("inquiry.distance")
            .expect("the tool starts");
        plan.press(200.0, 150.0); // (0, 0)
        plan.press(230.0, 110.0); // (3, 4)
        let messages = plan.messages();
        assert_eq!(messages.len(), 1);
        // sqrt(3^2 + 4^2) = 5, and nothing drawn.
        assert!(
            messages[0].contains("Horizontal distance 5.000"),
            "{}",
            messages[0]
        );
        assert!(plan.entities().is_empty());
        assert!(
            plan.view.tool_active(),
            "ready for the next measurement"
        );
    }

    #[test]
    fn match_properties_takes_the_source_by_a_click_and_the_targets_by_selection() {
        let mut plan = fixture();
        layer(&mut plan, "ROAD");
        let source = Entity {
            geometry: Geometry::Line(Segment2::new(p(0.0, 0.0), p(10.0, 0.0))),
            layer: String::from("ROAD"),
            ..Entity::default()
        };
        plan.document
            .execute(create_entities(vec![source]))
            .expect("the source is made");
        let target = plan.add_line(p(0.0, 5.0), p(10.0, 5.0));
        plan.start_tool("modify.match_properties")
            .expect("the tool starts");
        plan.press(250.0, 150.0); // on the source, (5, 0)
        plan.press(250.0, 100.0); // on the target, (5, 5)
        plan.enter();
        assert!(!plan.view.tool_active());
        assert_eq!(
            plan.entity(target).expect("the target is there").layer,
            "ROAD"
        );
        assert!(plan.errors().is_empty(), "{:?}", plan.errors());
    }

    #[test]
    fn a_dimension_snapped_to_a_lines_ends_keeps_the_ends_it_was_snapped_to() {
        // The view hands the tool the entity point its snap found, not only the point: pixel
        // (202, 152) is model (0.2, -0.2), 0.28 from the line's start and inside the aperture,
        // and (298, 148) is (9.8, 0.2), as near its end. The dimension line goes through
        // (250, 100), model (5, 5).
        let mut plan = fixture();
        let line: EntityId = plan.add_line(p(0.0, 0.0), p(10.0, 0.0));
        plan.set_snap_enabled(true);
        plan.view
            .set_snap_modes(&mut plan.document, SnapMode::Endpoint as SnapModes);
        plan.start_tool("annotate.dimaligned")
            .expect("the tool starts");
        plan.press(202.0, 152.0);
        plan.press(298.0, 148.0);
        plan.set_snap_enabled(false);
        plan.press(250.0, 100.0);

        let made = plan.entities();
        assert_eq!(made.len(), 2);
        let Geometry::Dimension(dimension) = &made[1].geometry else {
            panic!("not a dimension: {:?}", made[1].geometry);
        };
        assert_eq!(dimension.start, p(0.0, 0.0));
        assert_eq!(dimension.end, p(10.0, 0.0));
        assert_eq!(dimension.start_ref.entity, line);
        assert_eq!(dimension.start_ref.point, AnchorPoint::Start);
        assert_eq!(dimension.start_ref.index, 0);
        assert_eq!(dimension.start_ref.parameter, 0.0);
        assert_eq!(dimension.end_ref.entity, line);
        assert_eq!(dimension.end_ref.point, AnchorPoint::End);
        assert_eq!(dimension.end_ref.index, 0);
        assert_eq!(dimension.end_ref.parameter, 0.0);
    }
}
