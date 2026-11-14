//! Not a twin of a C++ file: the plan view drawn through egui in the harness, to hold what the
//! twins take for granted - that `PlanView::ui` lays the view out in the rectangle it is given,
//! records it under its C++ object name, and sends the frame's pointer to the view at the view's
//! own pixels (the C++'s widget pixels), wherever the view is in the window.

use egui::{Modifiers, PointerButton, Rect, UiBuilder, pos2, vec2};
use katana_cad::Document;
use katana_commands::{EntityAttributes, create_line};
use katana_geometry::Point2;
use katana_window::drive::{ControlKind, Harness};
use katana_window::viewport_widget::PlanView;

use crate::support::plan_state;

mod view_ui {
    use super::*;

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
        drop(draw);

        assert_eq!(view.size(), (400, 300));
        assert_eq!(document.selection().ids(), [line]);
    }
}
