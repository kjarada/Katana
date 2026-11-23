//! Not a twin of a C++ file: the plan view drawn through egui in the harness, to hold what the
//! twins take for granted - that `PlanView::ui` lays the view out in the rectangle it is given,
//! records it under its C++ object name, and sends the frame's pointer to the view at the view's
//! own pixels (the C++'s widget pixels), wherever the view is in the window.

use egui::{Modifiers, PointerButton, Rect, UiBuilder, pos2, vec2};
use katana_cad::Document;
use katana_cad::view_set::ViewState;
use katana_commands::{EntityAttributes, create_line};
use katana_geometry::Point2;
use katana_window::drive::{ControlKind, Harness};
use katana_window::viewport_widget::PlanView;

use crate::support::plan_state;

/// Where the view is drawn in the harness's window, and so the size of its picture at a pixel
/// a point.
const PLACE_WIDTH: usize = 400;
const PLACE_HEIGHT: usize = 300;

/// One frame of the view at (100, 50) of the window, and the pictures the frame handed to a
/// texture whole: those of the view's size, which no other texture of the harness has (the
/// only other is the font atlas).
fn frame_of(
    harness: &mut Harness,
    view: &mut PlanView,
    document: &mut Document,
    state: &mut ViewState,
) -> Vec<egui::ColorImage> {
    let place = Rect::from_min_size(
        pos2(100.0, 50.0),
        vec2(PLACE_WIDTH as f32, PLACE_HEIGHT as f32),
    );
    let output = harness.frame(|ui: &mut egui::Ui| {
        ui.scope_builder(UiBuilder::new().max_rect(place), |ui| {
            let _ = view.ui(ui, &mut *document, &mut *state);
        });
    });
    output
        .textures_delta
        .set
        .values()
        .flatten()
        .filter(|delta| delta.pos.is_none() && delta.image.size() == [PLACE_WIDTH, PLACE_HEIGHT])
        .map(|delta| match &delta.image {
            egui::ImageData::Color(image) => (**image).clone(),
        })
        .collect()
}

mod view_ui {
    use super::*;

    // docs/performance.md, "The plan view's frames": a frame that paints the picture the
    // texture already holds does not convert and upload it again, and whenever the picture
    // changes the texture is given exactly the new one - so the screen never shows a picture
    // the view does not have.
    #[test]
    fn the_texture_is_given_the_picture_when_it_changes_and_only_then() {
        let mut document = Document::new(None);
        document
            .execute(create_line(
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                EntityAttributes::default(),
            ))
            .expect("the line is made");
        let line = document.last_created_entities()[0];
        let mut state = plan_state();
        state.plan_framed = true;
        state.plan.center = Point2::new(0.0, 0.0);
        state.plan.scale = 10.0;
        let mut view = PlanView::new(&document, &mut state);
        let mut harness = Harness::new();

        // The first frame: the picture, whole.
        let first = frame_of(&mut harness, &mut view, &mut document, &mut state);
        assert_eq!(first.len(), 1);
        assert!(first[0] == view.picture().to_color_image());

        // Frames with nothing changed: painted again, the same, and not handed over.
        for _ in 0..3 {
            let before = view.picture().clone();
            let again = frame_of(&mut harness, &mut view, &mut document, &mut state);
            assert!(again.is_empty());
            assert!(*view.picture() == before);
        }

        // A pan moves the line: the new picture, whole, and not the old one.
        state.plan.center = Point2::new(3.0, 2.0);
        let panned = frame_of(&mut harness, &mut view, &mut document, &mut state);
        assert_eq!(panned.len(), 1);
        assert!(panned[0] == view.picture().to_color_image());
        assert!(panned[0] != first[0]);
        assert!(frame_of(&mut harness, &mut view, &mut document, &mut state).is_empty());

        // A selection draws the line selected, and its grips.
        document.selection_mut().set(vec![line]);
        document.notify_selection_changed();
        let selected = frame_of(&mut harness, &mut view, &mut document, &mut state);
        assert_eq!(selected.len(), 1);
        assert!(selected[0] == view.picture().to_color_image());
        assert!(selected[0] != panned[0]);
        assert!(frame_of(&mut harness, &mut view, &mut document, &mut state).is_empty());

        // Back where it began: the picture is handed over again, since the texture holds
        // another.
        document.selection_mut().clear();
        document.notify_selection_changed();
        state.plan.center = Point2::new(0.0, 0.0);
        let back = frame_of(&mut harness, &mut view, &mut document, &mut state);
        assert_eq!(back.len(), 1);
        assert!(back[0] == view.picture().to_color_image());
        assert!(back[0] != selected[0]);
    }

    // The picture is laid into the buffer of the one before the last (the view keeps two), so
    // a picture must never carry anything over from the buffer it was laid into: a view painted
    // alone, frame after frame, shows what a view painted once from new shows.
    #[test]
    fn a_picture_laid_into_a_kept_buffer_is_the_picture_a_new_view_paints() {
        let mut document = Document::new(None);
        document
            .execute(create_line(
                Point2::new(-15.0, -10.0),
                Point2::new(15.0, 10.0),
                EntityAttributes::default(),
            ))
            .expect("the line is made");
        let centres = [
            Point2::new(0.0, 0.0),
            Point2::new(6.0, -4.0),
            Point2::new(-9.0, 7.0),
            Point2::new(0.0, 0.0),
            Point2::new(40.0, 40.0), // the line out of sight: the bare ground
            Point2::new(-2.0, 1.0),
        ];
        let sizes = [(400, 300), (400, 300), (320, 200), (520, 410), (400, 300), (400, 300)];

        let mut kept_state = plan_state();
        kept_state.plan_framed = true;
        kept_state.plan.scale = 10.0;
        let mut kept = PlanView::new(&document, &mut kept_state);
        for (centre, (width, height)) in centres.into_iter().zip(sizes) {
            kept_state.plan.center = centre;
            kept.resize(&mut kept_state, width, height);
            let reused = kept.paint(&document, &mut kept_state).clone();

            let mut new_state = plan_state();
            new_state.plan_framed = true;
            new_state.plan.scale = 10.0;
            new_state.plan.center = centre;
            let mut new = PlanView::new(&document, &mut new_state);
            new.resize(&mut new_state, width, height);
            let fresh = new.paint(&document, &mut new_state);
            assert_eq!((fresh.width(), fresh.height()), (width as u32, height as u32));
            assert!(reused == *fresh, "at {centre:?}, {width} x {height}");
        }
    }

    #[test]
    fn a_click_in_the_window_reaches_the_view_at_the_views_own_pixel() {
        // The line (0,0)-(10,0) in a 400 x 300 view centred on the origin at 10 px a unit: pixel
        // (250, 150) of the view is model (5, 0), on the line. The view is drawn 100 right and
        // 50 down in the window, so the click there is at (350, 200) of the window.
        let mut document = Document::new(None);
        document
            .execute(create_line(
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                EntityAttributes::default(),
            ))
            .expect("the line is made");
        let line = document.last_created_entities()[0];
        let mut state = plan_state();
        state.plan_framed = true;
        state.plan.center = Point2::new(0.0, 0.0);
        state.plan.scale = 10.0;
        let mut view = PlanView::new(&document, &mut state);
        let place = Rect::from_min_size(pos2(100.0, 50.0), vec2(400.0, 300.0));
        let mut harness = Harness::new();
        let mut draw = |ui: &mut egui::Ui| {
            ui.scope_builder(UiBuilder::new().max_rect(place), |ui| {
                let _ = view.ui(ui, &mut document, &mut state);
            });
        };

        harness.show(&mut draw).expect("the view is drawn");
        let record = harness
            .control("PlanView1")
            .expect("the view is recorded under its object name");
        assert_eq!(record.kind, ControlKind::View);
        assert_eq!(record.rect, place);

        let at = pos2(350.0, 200.0);
        harness.pointer_move(at);
        harness.run(&mut draw).expect("the pointer moves");
        harness.pointer_button(at, PointerButton::Primary, true, Modifiers::NONE);
        harness.run(&mut draw).expect("the button goes down");
        harness.pointer_button(at, PointerButton::Primary, false, Modifiers::NONE);
        harness.run(&mut draw).expect("the button comes up");

        assert_eq!(view.size(), (400, 300));
        assert_eq!(document.selection().ids(), [line]);
    }

    // docs/performance.md, "The selection in the plan view's key": the kept drawing is keyed by
    // the selection itself, compared set against set at every paint. The selection is changed
    // here with no notification - a notification repaints by itself - so only the comparison
    // can tell: one id swapped for another, the count as it was, paints the drawing again, and
    // the same set put again, in another order, does not.
    #[test]
    fn the_drawing_is_painted_again_for_a_selection_one_id_different_and_not_for_the_same_set() {
        let mut document = Document::new(None);
        let mut lines = Vec::new();
        for row in [-5.0, 0.0, 5.0] {
            document
                .execute(create_line(
                    Point2::new(-10.0, row),
                    Point2::new(10.0, row),
                    EntityAttributes::default(),
                ))
                .expect("the line is made");
            lines.push(document.last_created_entities()[0]);
        }
        let mut state = plan_state();
        state.plan_framed = true;
        state.plan.center = Point2::new(0.0, 0.0);
        state.plan.scale = 10.0;
        let mut view = PlanView::new(&document, &mut state);
        view.resize(&mut state, 400, 300);

        document.selection_mut().set(vec![lines[0], lines[1]]);
        let first_two = view.paint(&document, &mut state).clone();
        let painted = view.drawing_paint_count();

        // The same two, put again the other way round: the kept drawing.
        document.selection_mut().set(vec![lines[1], lines[0]]);
        assert!(*view.paint(&document, &mut state) == first_two);
        assert_eq!(view.drawing_paint_count(), painted);

        // Two still, the second swapped for the third: a count of the ids would not tell.
        document.selection_mut().set(vec![lines[0], lines[2]]);
        let outer_two = view.paint(&document, &mut state).clone();
        assert_eq!(view.drawing_paint_count(), painted + 1);
        assert!(outer_two != first_two);
        let _ = view.paint(&document, &mut state);
        assert_eq!(view.drawing_paint_count(), painted + 1);

        // One id more, then that one taken out again: painted each time, and the two that are
        // left are drawn as they were when they were first selected.
        document.selection_mut().add(lines[1]);
        let all_three = view.paint(&document, &mut state).clone();
        assert_eq!(view.drawing_paint_count(), painted + 2);
        assert!(all_three != outer_two);
        document.selection_mut().remove(lines[1]);
        assert!(*view.paint(&document, &mut state) == outer_two);
        assert_eq!(view.drawing_paint_count(), painted + 3);

        // And none: the selection the view began with before it was first painted.
        document.selection_mut().clear();
        let none = view.paint(&document, &mut state).clone();
        assert_eq!(view.drawing_paint_count(), painted + 4);
        assert!(none != outer_two);
    }
}
