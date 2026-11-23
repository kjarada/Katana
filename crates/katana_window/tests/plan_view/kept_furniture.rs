//! Not a twin of a C++ file: the plan view's kept picture (docs/performance.md, "The plan view's
//! furniture on an unchanged frame"). A paint whose drawing was kept and whose furniture would be
//! drawn from exactly what the last paint's was keeps the whole picture - no copy of the drawing,
//! no furniture, nothing compared - so the screen is only as right as the furniture's key
//! (`FurnitureKey`, viewport_widget.rs) is complete. One case an input of that key: a frame with
//! the input as it was is kept, a change of it paints the furniture again, and the picture kept
//! is, pixel for pixel, what a paint of everything gives.
//!
//! How a case knows which path a paint took: `PlanView::furniture_paint_count` counts the
//! paints that drew the furniture. How it gets a paint of everything for the same state: the
//! window's word that its reference data changed (`invalidate_reference_cache`) makes the kept
//! drawing stale, and a drawing with no reference data paints the same again.
//!
//! The fixtures are support.rs's: `Plan::over_polyline` (the polyline (0,0)-(10,0)-(20,0), model
//! (0, 0) at pixel (100, 200), ten pixels a unit) and `Plan::at_origin` (an empty drawing, the
//! model origin at pixel (200, 150), ten pixels a unit).

use egui::{Rect, UiBuilder, pos2, vec2};
use katana_cad::Document;
use katana_cad::snapping::{SnapMode, SnapModes};
use katana_cad::view_set::ViewState;
use katana_commands::{EntityAttributes, create_line};
use katana_geometry::Point2;
use katana_window::canvas::PointF;
use katana_window::drawing::feedback_painter::overlay;
use katana_window::drive::Harness;
use katana_window::theme::{TextSize, set_text_size};
use katana_window::viewport_widget::{
    KeyModifiers, MouseButton, PlanView, PlanViewEvent, Tool, ViewKey,
};

use crate::support::{Plan, ink_near, plan_state};

const NONE: KeyModifiers = KeyModifiers::NONE;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
const CTRL: KeyModifiers = KeyModifiers::CTRL;

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

/// A paint, and whether it drew the furniture again; false is a paint that kept the picture.
fn painted(f: &mut Plan) -> bool {
    let before = f.view.furniture_paint_count();
    f.paint();
    f.view.furniture_paint_count() != before
}

/// The view as it stands is kept by a paint with nothing changed, and the picture it keeps is
/// the one a paint of everything - the drawing and the furniture, nothing kept - then gives.
fn assert_kept(f: &mut Plan) {
    // Whatever the events before this changed is painted first.
    f.paint();
    assert!(
        !painted(f),
        "a paint with nothing changed keeps the picture"
    );
    let kept = f.view.picture().clone();
    let drawings = f.view.drawing_paint_count();
    f.view.invalidate_reference_cache();
    assert!(painted(f), "the paint of everything draws the furniture");
    assert_eq!(f.view.drawing_paint_count(), drawings + 1);
    assert!(
        *f.view.picture() == kept,
        "the kept picture is what a paint of everything gives"
    );
    assert!(!painted(f));
}

/// The tracking tests' view (plan_view_tracking.rs): the line (0,0)-(10,10), whose far end is at
/// pixel (300, 50), with the snaps on and Endpoint alone.
fn snapping() -> Plan {
    let mut plan = Plan::at_origin();
    let _ = plan.add_line(p(0.0, 0.0), p(10.0, 10.0));
    plan.document.drafting_mut().snap_enabled = true;
    plan.document.drafting_mut().snap_modes = SnapMode::Endpoint as SnapModes;
    plan.paint();
    plan
}

fn cursor_moves(f: &Plan) -> usize {
    f.log
        .iter()
        .filter(|event| matches!(event, PlanViewEvent::CursorMoved { .. }))
        .count()
}

mod kept_furniture {
    use super::*;

    // The key's first input is the kept drawing itself (the DrawingKey): the size here, as the
    // one part of it the furniture reads for itself.
    #[test]
    fn a_paint_with_nothing_changed_keeps_the_picture_a_paint_of_everything_gives() {
        let mut f = Plan::over_polyline();
        assert_kept(&mut f);
        // Selected: its grips are furniture, and are kept with the picture.
        f.select();
        assert_eq!(f.view.grip_count(), 5);
        assert_kept(&mut f);

        // The statistics are raised by a paint that kept the picture as by any other.
        let raised = f.frame_stats().len();
        assert!(!painted(&mut f));
        let stats = f.frame_stats();
        assert_eq!(stats.len(), raised + 1);
        assert!(stats[raised].contains("(kept, "), "{}", stats[raised]);

        // A view made wider paints everything again, at its new size.
        let before = f.view.picture().clone();
        f.resize(420, 300);
        assert!(painted(&mut f));
        assert_eq!(f.view.picture().width(), 420);
        assert!(*f.view.picture() != before);
        assert_kept(&mut f);
    }

    // The cursor is in the key only where the snap marker draws at it, so the commonest frame
    // of all - the pointer crossing a view with no tool running - is kept.
    #[test]
    fn a_pointer_crossing_the_view_with_nothing_to_mark_keeps_the_picture() {
        let mut f = Plan::over_polyline();
        f.select();
        assert_kept(&mut f);
        let shown = f.view.picture().clone();
        let moves = cursor_moves(&f);
        // Bare ground, each place well over a pick aperture (8 px) from the polyline's five
        // grips, which lie along y = 200 from x = 100 to x = 300.
        let places = [(40.0, 30.0), (41.0, 30.0), (350.0, 60.0), (220.0, 280.0)];
        for (x, y) in places {
            f.move_to(x, y);
            assert!(!painted(&mut f), "at {x}, {y}");
        }
        assert_eq!(cursor_moves(&f), moves + places.len(), "the cursor did move");
        assert_kept(&mut f);
        assert!(*f.view.picture() == shown);
    }

    #[test]
    fn a_grip_coming_under_the_pointer_and_leaving_it_each_paint_the_furniture_again() {
        let mut f = Plan::over_polyline();
        f.select();
        assert_kept(&mut f);
        let bare = f.view.picture().clone();

        // Onto the middle vertex: drawn as the one under the cursor, its hint along the bottom.
        let grip = f.pixel(p(10.0, 0.0));
        f.mouse_move(grip, NONE);
        assert!(f.view.grip_controller().hovered().is_some());
        assert!(painted(&mut f));
        let hovered = f.view.picture().clone();
        assert!(hovered != bare);

        // A pixel along, the same grip still under the pointer: nothing to draw again.
        f.mouse_move(PointF::new(grip.x + 1.0, grip.y), NONE);
        assert!(!painted(&mut f));
        assert_kept(&mut f);
        assert!(*f.view.picture() == hovered);

        // Off it: as it was before the pointer came.
        f.move_to(40.0, 30.0);
        assert!(f.view.grip_controller().hovered().is_none());
        assert!(painted(&mut f));
        assert!(*f.view.picture() == bare);
    }

    #[test]
    fn a_grip_made_hot_and_let_go_each_paint_the_furniture_again() {
        let mut f = Plan::over_polyline();
        f.select();
        assert_kept(&mut f);
        let cold = f.view.picture().clone();

        // Shift+click chooses the vertex without picking it up.
        f.click_at(p(10.0, 0.0), SHIFT);
        assert_eq!(f.view.grip_controller().hot().len(), 1);
        assert!(!f.view.grip_controller().active());
        assert!(painted(&mut f));
        let hot = f.view.picture().clone();
        assert!(hot != cold);
        assert_kept(&mut f);
        assert!(*f.view.picture() == hot);

        // Esc lets it go.
        f.plain_key(ViewKey::Escape);
        assert!(f.view.grip_controller().hot().is_empty());
        assert!(painted(&mut f));
        assert!(*f.view.picture() == cold);
    }

    #[test]
    fn a_selection_box_opened_stretched_and_abandoned_each_paint_the_furniture_again() {
        let mut f = Plan::over_polyline();
        assert_kept(&mut f);
        let bare = f.view.picture().clone();

        // A press on bare ground opens the box; a move stretches it.
        f.mouse_press(PointF::new(40.0, 30.0), MouseButton::Left, NONE);
        assert!(painted(&mut f));
        f.move_to(140.0, 110.0);
        assert!(painted(&mut f));
        let boxed = f.view.picture().clone();
        assert!(boxed != bare);

        // The pointer still and the box as it is: kept, box and all.
        f.move_to(140.0, 110.0);
        assert!(!painted(&mut f));
        assert_kept(&mut f);
        assert!(*f.view.picture() == boxed);

        // Stretched further.
        f.move_to(180.0, 120.0);
        assert!(painted(&mut f));
        assert!(*f.view.picture() != boxed);

        // Esc abandons the box with nothing selected and nothing announced: only the key's
        // own corners say it has gone.
        f.plain_key(ViewKey::Escape);
        assert!(f.document.selection().empty());
        assert!(painted(&mut f));
        assert!(*f.view.picture() == bare);
    }

    #[test]
    fn the_empty_drawings_hint_is_kept_with_the_picture_and_goes_when_something_is_drawn() {
        let mut f = Plan::at_origin();
        assert!(f.view.empty_hint_shown());
        assert_kept(&mut f);
        assert!(f.view.empty_hint_shown());
        let hinted = f.view.picture().clone();

        let _ = f.add_line(p(-5.0, -5.0), p(5.0, 5.0));
        assert!(painted(&mut f));
        assert!(!f.view.empty_hint_shown());
        assert!(*f.view.picture() != hinted);
        assert_kept(&mut f);
    }

    // View > Text Size changes the fonts of the hint and of the bands with no word to the view
    // or the document. The view is 800 x 600 so that the hint is laid out at either size (a
    // view too small for it shows none).
    #[test]
    fn a_change_of_the_text_size_paints_the_furniture_again() {
        let mut f = Plan::at_origin();
        f.resize(800, 600);
        assert_kept(&mut f);
        let standard = f.view.picture().clone();

        let ctx = egui::Context::default();
        set_text_size(&ctx, TextSize::Large);
        assert!(painted(&mut f));
        let large = f.view.picture().clone();
        assert!(large != standard, "the hint is laid out in the larger text");
        assert_kept(&mut f);
        assert!(*f.view.picture() == large);

        set_text_size(&ctx, TextSize::Standard);
        assert!(painted(&mut f));
        assert!(*f.view.picture() == standard);
    }

    // The snap marker outlives its tool until the pointer next moves: with no tool running it
    // is furniture with a key, and what it is drawn from is in the key.
    #[test]
    fn a_snap_marker_left_when_its_tool_ends_is_kept_until_the_pointer_moves() {
        let mut f = snapping();
        f.start_tool("draw.point").expect("the tool starts");
        // Onto the line's end at (10, 10).
        f.move_to(300.0, 50.0);
        assert!(painted(&mut f));
        f.set_tool(Tool::Select);
        assert!(!f.view.tool_active());

        // The first paint after a tool's has no key to match: painted, the marker in it.
        assert!(painted(&mut f));
        let marked = f.view.picture().clone();
        assert!(
            ink_near(&marked, PointF::new(300.0, 50.0), 9.0, overlay::snap()) > 0,
            "the Endpoint square is drawn round the line's end"
        );
        assert_kept(&mut f);
        assert!(*f.view.picture() == marked);

        // The pointer moves with no tool to snap for: the marker goes.
        f.move_to(120.0, 220.0);
        assert!(painted(&mut f));
        assert!(*f.view.picture() != marked);
        assert_kept(&mut f);
    }

    // Object snap tracking's switch is a drafting setting changed with no notification, and the
    // acquired points' crosses are drawn only while it is on.
    #[test]
    fn the_acquired_points_crosses_left_by_a_tool_follow_the_tracking_switch() {
        let mut f = snapping();
        f.document.drafting_mut().object_tracking = true;
        f.start_tool("draw.point").expect("the tool starts");
        f.move_to(300.0, 50.0);
        assert_eq!(f.view.tracking_points().len(), 1);
        f.set_tool(Tool::Select);
        assert!(painted(&mut f));
        let tracked = f.view.picture().clone();
        assert_kept(&mut f);

        f.document.drafting_mut().object_tracking = false;
        assert!(painted(&mut f));
        let untracked = f.view.picture().clone();
        assert!(untracked != tracked, "the acquired point's cross is gone");
        assert_kept(&mut f);

        f.document.drafting_mut().object_tracking = true;
        assert!(painted(&mut f));
        assert!(*f.view.picture() == tracked);

        // The pointer moves: the acquired point is forgotten, and the snap with it.
        f.move_to(120.0, 220.0);
        assert!(f.view.tracking_points().is_empty());
        assert!(painted(&mut f));
        assert!(*f.view.picture() != tracked);
        assert!(*f.view.picture() != untracked);
    }

    // A drafting aid's label is written beside the cursor: the one furniture drawn AT the
    // cursor with no tool running. Ortho holding a grip's drag leaves "Ortho" there when the
    // grip is put down (plan_view_grips' ortho case shows the drag is held).
    #[test]
    fn a_drafting_aids_label_left_by_a_grip_drag_is_kept_until_the_pointer_moves() {
        let mut f = Plan::over_polyline();
        f.document.drafting_mut().ortho = true;
        f.select();
        f.drag(p(10.0, 0.0), p(11.0, 8.0), NONE);
        assert_eq!(f.vertices()[1], p(10.0, 8.0));

        assert!(painted(&mut f));
        let labelled = f.view.picture().clone();
        assert_kept(&mut f);
        assert!(*f.view.picture() == labelled);

        // Away over bare ground: no grip is being dragged, so nothing constrains the cursor
        // and the label goes.
        f.move_to(40.0, 30.0);
        assert!(painted(&mut f));
        assert!(*f.view.picture() != labelled);
        assert_kept(&mut f);
    }

    // No key: a tool's preview and prompt are read from the tool itself.
    #[test]
    fn a_running_tools_frames_are_always_painted() {
        let mut f = Plan::at_origin();
        assert_kept(&mut f);
        f.start_tool("draw.line").expect("the tool starts");
        for _ in 0..3 {
            assert!(painted(&mut f));
        }
        assert!(!f.view.prompt_band_text().is_empty());

        // Ended: the first paint has no key to match, and the ones after are kept, with what
        // a kept paint leaves beside the picture as a painted one leaves it.
        f.set_tool(Tool::Select);
        assert!(painted(&mut f));
        assert_kept(&mut f);
        assert!(f.view.prompt_band_text().is_empty());
        assert_eq!(f.view.last_preview_count(), 0);
    }

    // No key: a grip pressed or picked up draws its drag from what the controller keeps to
    // itself.
    #[test]
    fn a_grip_in_hand_is_painted_every_frame() {
        let mut f = Plan::over_polyline();
        f.select();
        assert_kept(&mut f);

        f.press_at(p(20.0, 0.0), NONE);
        assert!(f.view.grip_controller().pressed());
        for _ in 0..2 {
            assert!(painted(&mut f));
        }
        // Let go without a drag: picked up.
        f.release_at(p(20.0, 0.0), NONE);
        assert!(f.view.grip_controller().active());
        for _ in 0..2 {
            assert!(painted(&mut f));
        }

        // Esc puts it down, still chosen.
        f.plain_key(ViewKey::Escape);
        assert!(!f.view.grip_controller().active());
        assert!(painted(&mut f));
        assert_kept(&mut f);
    }

    // No key: with Ctrl held over a segment middle the vertex a drag would add is drawn by the
    // plan painter.
    #[test]
    fn ctrl_held_over_a_segment_middle_is_painted_every_frame() {
        let mut f = Plan::over_polyline();
        f.select();
        f.hover(p(5.0, 0.0));
        assert!(painted(&mut f));
        // The middle under the pointer with no Ctrl has a key.
        assert_kept(&mut f);
        let plain = f.view.picture().clone();

        f.hover_with(p(5.0, 0.0), CTRL);
        assert!(f.view.grip_controller().ctrl());
        for _ in 0..3 {
            assert!(painted(&mut f));
        }
        assert!(
            *f.view.picture() != plain,
            "the vertex a Ctrl-drag would add is marked"
        );

        f.hover(p(5.0, 0.0));
        assert!(painted(&mut f));
        assert!(*f.view.picture() == plain);
        assert_kept(&mut f);
    }

    // Through egui, as the window draws the view: frames made by the pointer moving about the
    // window, outside the view and over its bare ground, draw no furniture and hand the texture
    // nothing.
    #[test]
    fn frames_of_the_pointer_moving_about_the_window_draw_no_furniture() {
        let mut document = Document::new(None);
        document
            .execute(create_line(
                p(0.0, 0.0),
                p(10.0, 0.0),
                EntityAttributes::default(),
            ))
            .expect("the line is made");
        let mut state = plan_state();
        state.plan_framed = true;
        state.plan.center = p(0.0, 0.0);
        state.plan.scale = 10.0;
        let mut view = PlanView::new(&document, &mut state);
        let mut harness = Harness::new();
        // The view at (100, 50) of the window, 400 x 300: its line runs from (300, 200) to
        // (400, 200) of the window.
        let place = Rect::from_min_size(pos2(100.0, 50.0), vec2(400.0, 300.0));
        let frame = |harness: &mut Harness,
                     view: &mut PlanView,
                     document: &mut Document,
                     state: &mut ViewState| {
            let output = harness.frame(|ui: &mut egui::Ui| {
                ui.scope_builder(UiBuilder::new().max_rect(place), |ui| {
                    let _ = view.ui(ui, &mut *document, &mut *state);
                });
            });
            // Whether the view's picture was handed to its texture: the only texture of its size.
            output
                .textures_delta
                .set
                .values()
                .flatten()
                .any(|delta| delta.pos.is_none() && delta.image.size() == [400, 300])
        };

        assert!(frame(&mut harness, &mut view, &mut document, &mut state));
        assert!(!frame(&mut harness, &mut view, &mut document, &mut state));
        let drawn = view.furniture_paint_count();
        let _ = view.take_events();

        // Outside the view, three places over its bare ground, and outside again.
        for at in [
            pos2(20.0, 20.0),
            pos2(150.0, 100.0),
            pos2(180.0, 120.0),
            pos2(420.0, 320.0),
            pos2(30.0, 400.0),
        ] {
            harness.pointer_move(at);
            assert!(
                !frame(&mut harness, &mut view, &mut document, &mut state),
                "at {at:?}"
            );
        }
        assert_eq!(view.furniture_paint_count(), drawn);
        let reached = view
            .take_events()
            .iter()
            .filter(|event| matches!(event, PlanViewEvent::CursorMoved { .. }))
            .count();
        assert!(reached >= 2, "the pointer's moves reached the view: {reached}");
    }
}
