//! The twin of tests/qt_widgets/test_plan_view.cpp: the plan view (viewport_widget), how it frames
//! the drawing and what it culls. Positions are checked through the view's state, which is what
//! the view draws through (ViewState::plan).

use std::sync::Arc;

use katana_cad::Document;
use katana_cad::terrain::surface_style::{SurfaceDisplayStyle, built_in_surface_styles};
use katana_cad::view_set::ViewState;
use katana_commands::{EntityAttributes, create_dimension, create_line};
use katana_entity::DimensionGeometry;
use katana_geometry::{Point2, Point3};
use katana_interop::reference_data::{RasterOverlay, ReferenceData};
use katana_terrain::surface_store::{NamedSurface, SurfaceStore};
use katana_terrain::tin_surface::TinSurface;
use katana_window::viewport_widget::{PlanView, PlanViewEvent, ViewParts};

use crate::support::plan_state;

/// The 10 x 10 square z = x as a surface named "ground", drawn in `style`.
fn ground_in(style: &str) -> SurfaceStore {
    let tin = TinSurface::create(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 10.0),
            Point3::new(10.0, 10.0, 10.0),
            Point3::new(0.0, 10.0, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    )
    .expect("the square is a surface");
    let mut store = SurfaceStore::new();
    store
        .add(NamedSurface {
            name: String::from("ground"),
            surface: Some(Arc::new(tin)),
            source: String::from("test"),
            style: String::from(style),
            visible: true,
        })
        .expect("the surface is added");
    store
}

/// True when `world` lands inside a `width` x `height` view.
fn on_screen(state: &ViewState, world: Point2, width: f64, height: f64) -> bool {
    let pixel = state.plan.world_to_screen(world);
    pixel.x >= 0.0 && pixel.x <= width && pixel.y >= 0.0 && pixel.y <= height
}

fn line_drawing() -> Document {
    let mut document = Document::new(None);
    document
        .execute(create_line(
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 50.0),
            EntityAttributes::default(),
        ))
        .expect("the line is made");
    document
}

mod plan_view {
    use super::*;

    #[test]
    fn a_view_still_as_framed_keeps_the_whole_drawing_in_view_when_it_shrinks() {
        // The line (0, 0)-(100, 50) framed in a 400 x 300 view. Shrunk to 200 x 300 - a 3D view
        // opened beside it - keeping the scale would leave 200 / 400 of the width it had, about
        // 46 of its 100 units about x = 50, with both ends off the edges. Framed again it shows
        // the whole line.
        let document = line_drawing();
        let mut state = plan_state();
        let mut view = PlanView::new(&document, &mut state);
        view.resize(&mut state, 400, 300);
        let _ = view.paint(&document, &mut state);
        assert!(state.plan_framed);
        assert!(on_screen(&state, Point2::new(0.0, 0.0), 400.0, 300.0));
        assert!(on_screen(&state, Point2::new(100.0, 50.0), 400.0, 300.0));

        view.resize(&mut state, 200, 300);
        let _ = view.paint(&document, &mut state);
        assert!(on_screen(&state, Point2::new(0.0, 0.0), 200.0, 300.0));
        assert!(on_screen(&state, Point2::new(100.0, 50.0), 200.0, 300.0));
    }

    #[test]
    fn a_view_the_user_zoomed_keeps_its_scale_when_resized() {
        let mut document = line_drawing();
        let mut state = plan_state();
        let mut view = PlanView::new(&document, &mut state);
        view.resize(&mut state, 400, 300);
        let _ = view.paint(&document, &mut state);
        // One notch of the wheel over the middle of the view.
        view.wheel(
            &mut document,
            &mut state,
            katana_window::canvas::PointF::new(200.0, 150.0),
            120.0,
        );
        let zoomed = state.plan.scale;

        view.resize(&mut state, 200, 300);
        let _ = view.paint(&document, &mut state);
        assert_eq!(state.plan.scale, zoomed);
    }

    #[test]
    fn a_dimension_whose_label_alone_is_on_screen_is_drawn() {
        // Audit QT-25. The dimension from (0, 0) to (10, 0) with its line 2 above: the Standard
        // style puts the label textGap 0.625 above that line and textHeight 2.5 tall, centred on
        // x = 5, so it spans y 2.625 to 5.125. A 400 x 300 view at 100 px a unit centred on
        // (5, 4.5) shows x 3..7, y 3..6: the label's middle, but not the dimension line (y = 2),
        // the extension lines (x = 0 and 10) or the arrows (2.5 long, so x <= 2.5 and x >= 7.5).
        // The geometry's own box, (0, 0)-(10, 2), is off screen, and culling on it left the
        // dimension out.
        let mut document = Document::new(None);
        let dimension = DimensionGeometry {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
            offset: 2.0,
            ..DimensionGeometry::default()
        };
        document
            .execute(create_dimension(dimension, EntityAttributes::default()))
            .expect("the dimension is made");
        let mut state = plan_state();
        // This test's own view, not a frame of the drawing.
        state.plan_framed = true;
        state.plan.center = Point2::new(5.0, 4.5);
        state.plan.scale = 100.0;
        let mut view = PlanView::new(&document, &mut state);
        view.resize(&mut state, 400, 300);
        let _ = view.paint(&document, &mut state);
        assert_eq!(view.last_drawn_entity_count(), 1);
    }

    #[test]
    fn an_image_hidden_in_the_view_is_left_out_of_its_extents() {
        // A 2 x 2 pixel image placed by the geotransform (1000, 1, 0, 2000, 0, -1): x = 1000 +
        // column, y = 2000 - row, so its corners span (1000, 1998)-(1002, 2000). With the line
        // (0, 0)-(100, 50) the view draws (0, 0)-(1002, 2000); with the image hidden in this view
        // only the line, (0, 0)-(100, 50).
        let mut document = line_drawing();
        let mut reference = ReferenceData::default();
        let image = RasterOverlay {
            name: String::from("ortho"),
            width: 2,
            height: 2,
            rgba: vec![255; 2 * 2 * 4],
            geotransform: [1000.0, 1.0, 0.0, 2000.0, 0.0, -1.0],
            has_geotransform: true,
            ..RasterOverlay::default()
        };
        let id = reference.add(image);
        let mut state = plan_state();
        let view = PlanView::new(&document, &mut state);
        let drawing = ViewParts {
            document: &mut document,
            reference: Some(&reference),
            meshes: &[],
        };

        let drawn = view.drawn_bounds(&drawing, &state);
        assert_eq!(drawn.min, Point2::new(0.0, 0.0));
        assert_eq!(drawn.max, Point2::new(1002.0, 2000.0));

        state.hidden_references.insert(id);
        let drawn = view.drawn_bounds(&drawing, &state);
        assert_eq!(drawn.min, Point2::new(0.0, 0.0));
        assert_eq!(drawn.max, Point2::new(100.0, 50.0));
    }

    #[test]
    fn a_drawing_of_nothing_but_a_surface_is_not_said_to_be_empty() {
        // The view draws the surface, frames it and counts it in its extents, and laid "Nothing
        // drawn yet ... Bring data in with File > Import" over it, nearly opaque: the 3D view's
        // own rule counts surfaces.
        let mut document = Document::new(None);
        let mut state = plan_state();
        let mut view = PlanView::new(&document, &mut state);
        view.resize(&mut state, 400, 300);
        let _ = view.paint(&document, &mut state);
        assert!(
            view.empty_hint_shown(),
            "an empty drawing shows the hint (the control)"
        );

        document
            .set_surfaces(ground_in(""), "SET_SURFACES")
            .expect("the surface is set");
        view.resize(&mut state, 401, 300);
        let _ = view.paint(&document, &mut state);
        assert!(!view.empty_hint_shown());
        assert_eq!(view.last_paint_stats().surfaces_drawn, 1);
        // A hidden surface is still in the drawing: not empty either.
        let mut hidden = ground_in("");
        hidden
            .set_visible("ground", false)
            .expect("the surface is hidden");
        document
            .set_surfaces(hidden, "SET_SURFACES")
            .expect("the hidden surface is set");
        view.resize(&mut state, 400, 300);
        let _ = view.paint(&document, &mut state);
        assert!(!view.empty_hint_shown());
    }

    #[test]
    fn contours_too_fine_to_draw_are_reported_once_per_change_not_lost_in_silence() {
        // An interval of a millionth over 10 units of relief is refused for the plan (more than
        // four million points): the view draws the outline alone, and told the person nothing.
        // It tells the error hook once - not on every repaint - and again when the problem comes
        // back after it went.
        let mut document = Document::new(None);
        let mut set = built_in_surface_styles();
        let mut fine = SurfaceDisplayStyle {
            name: String::from("Fine"),
            ..SurfaceDisplayStyle::default()
        };
        fine.contours.show = true;
        fine.contours.interval = 1.0e-6;
        set.styles.push(fine);
        document
            .set_surface_styles(set.clone(), "SET_SURFACE_STYLES")
            .expect("the styles are set");
        document
            .set_surfaces(ground_in("Fine"), "SET_SURFACES")
            .expect("the surface is set");

        let mut state = plan_state();
        let mut view = PlanView::new(&document, &mut state);
        let mut errors: Vec<String> = Vec::new();
        let mut width = 400;
        // A new size each time: the kept drawing is stale.
        let mut repaint =
            |view: &mut PlanView, document: &Document, state: &mut ViewState, errors: &mut Vec<String>| {
                width += 1;
                view.resize(state, width, 300);
                let _ = view.paint(document, state);
                for event in view.take_events() {
                    if let PlanViewEvent::Error(text) = event {
                        errors.push(text);
                    }
                }
            };
        repaint(&mut view, &document, &mut state, &mut errors);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].contains("ground (style Fine): contours"),
            "{}",
            errors[0]
        );
        assert!(errors[0].contains("interval"), "{}", errors[0]);
        assert_eq!(view.last_paint_stats().surface_problems.len(), 1);
        repaint(&mut view, &document, &mut state, &mut errors);
        repaint(&mut view, &document, &mut state, &mut errors);
        assert_eq!(errors.len(), 1, "the same problem is said once");

        // The interval mended: nothing to say. Broken again: said again.
        set.styles
            .last_mut()
            .expect("the Fine style is there")
            .contours
            .interval = 1.0;
        document
            .set_surface_styles(set.clone(), "SET_SURFACE_STYLES")
            .expect("the mended styles are set");
        repaint(&mut view, &document, &mut state, &mut errors);
        assert_eq!(errors.len(), 1);
        assert!(view.last_paint_stats().surface_problems.is_empty());
        set.styles
            .last_mut()
            .expect("the Fine style is there")
            .contours
            .interval = 1.0e-6;
        document
            .set_surface_styles(set, "SET_SURFACE_STYLES")
            .expect("the broken styles are set");
        repaint(&mut view, &document, &mut state, &mut errors);
        assert_eq!(errors.len(), 2);
    }
}
