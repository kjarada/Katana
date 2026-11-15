//! The twin of the three PlanView cases of tests/qt_widgets/test_plan_painter.cpp: the plan
//! view's kept drawing - what paints it again and what does not - and the plot fitted to what the
//! view draws. (That file's PlanPainter suite is twinned in tests/plan_painting.)

use katana_cad::plot::{PaperSize, PlotSettings};
use katana_cad::Document;
use katana_commands::{
    EntityAttributes, create_alignment, create_layer, create_line, delete_alignment,
    set_entity_layer,
};
use katana_entity::{Alignment, Layer};
use katana_geometry::{AlignmentPi, HorizontalAlignment, Point2};
use katana_window::canvas::PointF;
use katana_window::viewport_widget::{KeyModifiers, PlanView, PlanViewEvent};

use crate::support::plan_state;

fn line(document: &mut Document, start: Point2, end: Point2) {
    document
        .execute(create_line(start, end, EntityAttributes::default()))
        .expect("the line is made");
}

/// The last statistics line the view raised, taking what it raised.
fn last_stats(view: &mut PlanView) -> String {
    view.take_events()
        .into_iter()
        .filter_map(|event| match event {
            PlanViewEvent::FrameStats(text) => Some(text),
            _ => None,
        })
        .next_back()
        .unwrap_or_default()
}

mod plan_view {
    use super::*;

    #[test]
    fn a_mouse_move_over_an_unchanged_drawing_does_not_paint_the_drawing_again() {
        let mut document = Document::new(None);
        line(
            &mut document,
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 50.0),
        );
        let mut state = plan_state();
        let mut view = PlanView::new(&document, &mut state);
        view.resize(&mut state, 400, 300);
        let _ = view.paint(&document, &mut state);
        let stats = last_stats(&mut view);
        let first = view.drawing_paint_count();
        assert!(first >= 1);
        assert!(stats.starts_with("Plan  1 drawn"), "{stats}");

        // The cursor moves: only the overlay is painted.
        view.mouse_move(
            &mut document,
            &mut state,
            PointF::new(120.0, 80.0),
            KeyModifiers::NONE,
        );
        let _ = view.paint(&document, &mut state);
        let stats = last_stats(&mut view);
        assert_eq!(view.drawing_paint_count(), first);
        assert!(stats.contains("kept"), "{stats}");

        // A command changes the drawing; a pan changes the view; a layer hidden in this view
        // changes what it shows: each paints the drawing again.
        line(
            &mut document,
            Point2::new(0.0, 50.0),
            Point2::new(100.0, 0.0),
        );
        let _ = view.paint(&document, &mut state);
        assert_eq!(view.drawing_paint_count(), first + 1);
        // Zoomed as the user zooms, with the wheel: a view the user has moved keeps its own
        // centre and scale from then on.
        view.wheel(
            &mut document,
            &mut state,
            PointF::new(200.0, 150.0),
            120.0,
        );
        let _ = view.paint(&document, &mut state);
        assert_eq!(view.drawing_paint_count(), first + 2);
        state.layers.hide("0");
        let _ = view.paint(&document, &mut state);
        assert_eq!(view.drawing_paint_count(), first + 3);
        assert_eq!(view.last_drawn_entity_count(), 0);
    }

    #[test]
    fn a_selection_changed_without_a_notification_is_still_drawn() {
        // The selection is part of what the kept drawing shows: a caller that sets it and only
        // repaints must see it, notification or not.
        let mut document = Document::new(None);
        line(
            &mut document,
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 50.0),
        );
        let mut state = plan_state();
        let mut view = PlanView::new(&document, &mut state);
        view.resize(&mut state, 400, 300);
        let _ = view.paint(&document, &mut state);
        let first = view.drawing_paint_count();
        let made = document.last_created_entities();
        document.selection_mut().set(made);
        let _ = view.paint(&document, &mut state);
        assert_eq!(view.drawing_paint_count(), first + 1);
    }

    #[test]
    fn a_plot_fitted_to_the_drawing_fits_what_the_view_draws_and_not_every_entity() {
        // Audit QT-12 / GEO-01, worked by hand. Drawn: a line (0, 0)-(100, 50) and an alignment
        // (0, 0)-(300, 0), which is in no entity's bounds. Not drawn: a stray (5000, 5000)-
        // (5010, 5010) on a layer this view hides. What the view draws is x 0..300, y 0..50.
        //
        // A3 landscape less 10 mm margins is 400 x 277 mm. 300 m across needs
        // 1 : 300 000 / 400 = 1 : 750 and 50 m up 1 : 50 000 / 277 = 1 : 181, so the first
        // standard scale is 1 : 1000, about the middle (150, 25). The spatial index's bounds -
        // the hidden stray in, the alignment out - are 0..5010 both ways:
        // 1 : 5 010 000 / 277 = 1 : 18 087, so 1 : 20 000 about (2505, 2505), a sheet with the
        // drawing a dot in its corner.
        let mut document = Document::new(None);
        line(
            &mut document,
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 50.0),
        );
        let far = Layer {
            name: String::from("far"),
            ..Layer::default()
        };
        document
            .execute(create_layer(far))
            .expect("the layer is made");
        line(
            &mut document,
            Point2::new(5000.0, 5000.0),
            Point2::new(5010.0, 5010.0),
        );
        let stray = document.last_created_entities();
        document
            .execute(set_entity_layer(stray, "far"))
            .expect("the stray is put on its layer");
        let road = Alignment {
            name: String::from("road"),
            horizontal: HorizontalAlignment {
                pis: vec![
                    AlignmentPi {
                        point: Point2::new(0.0, 0.0),
                        ..AlignmentPi::default()
                    },
                    AlignmentPi {
                        point: Point2::new(300.0, 0.0),
                        ..AlignmentPi::default()
                    },
                ],
                ..HorizontalAlignment::default()
            },
            ..Alignment::default()
        };
        document
            .execute(create_alignment(road))
            .expect("the alignment is made");
        let mut state = plan_state();
        state.layers.hide("far");
        let view = PlanView::new(&document, &mut state);

        // A3 landscape, 10 mm margins, 300 dpi.
        let settings = PlotSettings {
            scale_denominator: 500.0,
            ..PlotSettings::default()
        };
        let fitted = view
            .fitted_plot(&document, &state, settings)
            .expect("the drawing is fitted");
        assert_eq!(fitted.scale_denominator, 1000.0);
        assert!((fitted.center.x - 150.0).abs() <= 1e-9);
        assert!((fitted.center.y - 25.0).abs() <= 1e-9);
        // The sheet is the one asked for.
        assert_eq!(fitted.paper, PaperSize::A3);
        assert!(fitted.landscape);
        assert_eq!(fitted.dpi, 300.0);

        // A view that draws nothing has nothing to fit, and says so.
        state.layers.hide("0");
        document
            .execute(delete_alignment("road"))
            .expect("the alignment is deleted");
        assert!(view.fitted_plot(&document, &state, settings).is_err());
    }
}
