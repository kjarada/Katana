// GENERATE fit rotate=on (tests/cad/plotting/test_generate_rotate.cpp;
// plotting/sheet_verbs.rs, docs/plotting.md "Sheets on the command line"): the
// plan of the drawing, or of area=, turned to fill the sheet as the sheet
// editor's "Rotate the drawing to fill the sheet" turns it
// (smart_layout_rotated), so the Generate dialog's choice has a verb an agent
// can type. One undoable step; a refusal changes nothing.

use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::Rc;

use katana_cad::plotting::arrange::{drawn_outline, smart_layout_rotated};
use katana_cad::plotting::generators::prepare_for_append;
use katana_cad::plotting::sheet_verbs::sheet_verb_help;
use katana_cad::plotting::{
    LayoutRequest, Sheet, SheetSet, SheetVerbContext, Viewport, ViewportKind,
};
use katana_cad::selection::drawn_extent;
use katana_cad::{CommandInterpreter, Document, NO_LAYER_OVERRIDES};
use katana_core::text::format_exact_real;
use katana_geometry::{Box2, Point2, Vec2};

use crate::{Session, contains, head, must, near};

const DEGREE: f64 = PI / 180.0;

// The corners of a `length` x `width` strip along `turn`, centred on `centre`.
fn strip_corners(centre: Point2, length: f64, width: f64, turn: f64) -> Vec<Point2> {
    let mut corners = Vec::new();
    for corner in [
        Vec2::new(-length / 2.0, -width / 2.0),
        Vec2::new(length / 2.0, -width / 2.0),
        Vec2::new(length / 2.0, width / 2.0),
        Vec2::new(-length / 2.0, width / 2.0),
    ] {
        corners.push(centre + corner.rotated(turn));
    }
    corners
}

fn point_text(point: Point2) -> String {
    format!(
        "{},{}",
        format_exact_real(point.x),
        format_exact_real(point.y)
    )
}

// The strip drawn as its four sides, typed.
#[track_caller]
fn draw_strip(session: &mut Session, corners: &[Point2]) {
    for (i, corner) in corners.iter().enumerate() {
        let next = corners[(i + 1) % corners.len()];
        session.ok(&format!(
            "LINE {} {}",
            point_text(*corner),
            point_text(next)
        ));
    }
}

#[track_caller]
fn plan(sheet: &Sheet) -> &Viewport {
    match sheet
        .viewports
        .iter()
        .find(|viewport| viewport.kind == ViewportKind::Plan)
    {
        Some(viewport) => viewport,
        None => panic!("the sheet has no plan"),
    }
}

// A 450 x 20 m strip along 45 degrees, alone on an A3 sheet, whose tiling
// area is 385 x 250 mm (tiling_area of the built-in frame). Square to the
// paper it spans 332 m each way, which needs 1 : 1328 of the 250 mm, so
// 1 : 2000 on the ladder. Turned, the ladder's 1 : 1250 holds 481 x 312.5 m;
// the least turn that brings the strip's far corner (225, 10) from its centre
// within 156.25 m of the middle line is 45 degrees less the corner's
// direction, atan2(225, 10), plus the angle whose cosine is 156.25 over the
// corner's distance - the calculation the editor's own test makes
// (tests/qt_widgets/plotting/test_sheet_arrange.cpp).
fn turn() -> f64 {
    45.0 * DEGREE - 225.0_f64.atan2(10.0) + (156.25 / 225.0_f64.hypot(10.0)).acos()
}

mod generate_rotate {
    use super::*;

    #[test]
    fn the_fitted_drawing_is_turned_to_fill_the_sheet_in_one_step() {
        let mut s = Session::default();
        draw_strip(
            &mut s,
            &strip_corners(Point2::new(0.0, 0.0), 450.0, 20.0, 45.0 * DEGREE),
        );
        let steps = s.steps();

        assert_eq!(
            head(&s.ok("GENERATE fit rotate=off"), 20),
            "generated 1 sheet: 1"
        );
        assert_eq!(s.set().sheets.len(), 1);
        assert_eq!(plan(&s.set().sheets[0]).scale, 2000.0);
        assert_eq!(plan(&s.set().sheets[0]).rotation, 0.0);

        let turned = s.ok("GENERATE fit rotate=on");
        assert_eq!(head(&turned, 20), "generated 1 sheet: 2");
        let set = s.set();
        assert_eq!(set.sheets.len(), 2);
        let view = plan(&set.sheets[1]);
        assert_eq!(view.scale, 1250.0);
        near(view.rotation, turn(), 1e-9);
        assert!(contains(&turned, "kind=plan scale=1250 rect="), "{turned}");
        assert_eq!(s.steps(), steps + 2);
        assert_eq!(s.document.history().undo_name(), "GENERATE_SHEETS");

        // What the dialog's "Rotate" does is what the editor always did: the
        // same function over the drawing's outline.
        let request = LayoutRequest {
            plan_area: Some(drawn_extent(s.document.model(), &NO_LAYER_OVERRIDES)),
            ..LayoutRequest::default()
        };
        let mut expected = must(smart_layout_rotated(
            s.document.model(),
            &request,
            &drawn_outline(s.document.model(), &NO_LAYER_OVERRIDES),
            None,
        ));
        let first_only = SheetSet {
            sheets: vec![set.sheets[0].clone()],
            ..SheetSet::default()
        };
        prepare_for_append(&first_only, &mut expected);
        assert_eq!(expected.as_slice(), std::slice::from_ref(&set.sheets[1]));

        assert!(s.document.undo().is_ok());
        assert_eq!(s.set().sheets.len(), 1);
    }

    #[test]
    fn an_area_is_turned_when_it_fits_the_paper_better_that_way() {
        // A 100 x 300 m area on the 385 x 250 mm tiling area. Square, its 300 m
        // go up the 250 mm: 1 : 1200 or more, 1 : 1250 on the ladder. Turned by
        // t its height is 100 sin t + 300 cos t = hypot(100, 300) cos(t - a),
        // a = atan2(100, 300); at 1 : 1000 that must be 250 m or less, which it
        // first is at t = a + acos(250 / hypot(100, 300)), 56.2 degrees, where
        // its width, 305 m, is well inside the 385. 1 : 750 cannot be reached:
        // the height is 187.5 m or less only from 72.1 degrees, where the width
        // is 316 m, past the 288.75 m 1 : 750 allows. So the turn is the least
        // one that reaches 1 : 1000, not a quarter.
        let mut s = Session::default();
        s.ok("LINE 0,0 1,1");
        s.ok("GENERATE fit area=0,0,100,300");
        s.ok("GENERATE fit area=0,0,100,300 rotate=on");
        let set = s.set();
        assert_eq!(set.sheets.len(), 2);
        let square = plan(&set.sheets[0]);
        let turned = plan(&set.sheets[1]);
        assert_eq!(square.rotation, 0.0);
        assert_eq!(square.scale, 1250.0);
        assert_eq!(turned.scale, 1000.0);
        near(
            turned.rotation.abs(),
            100.0_f64.atan2(300.0) + (250.0 / 100.0_f64.hypot(300.0)).acos(),
            1e-9,
        );
    }

    #[test]
    fn a_straight_line_is_turned_across_the_sheet() {
        // One 300 m line along the x axis. Square it goes across the 385 mm:
        // 1 : 780 or more, 1 : 1000 on the ladder. Turned by t it needs
        // 300 cos t across and 300 sin t up; at 1 : 750 those are 288.75 m and
        // 187.5 m, both met from t = acos(288.75 / 300), 15.7 degrees, to 38.7.
        let mut s = Session::default();
        s.ok("LINE 0,0 300,0");
        s.ok("GENERATE fit");
        s.ok("GENERATE fit rotate=on");
        let set = s.set();
        assert_eq!(set.sheets.len(), 2);
        assert_eq!(plan(&set.sheets[0]).scale, 1000.0);
        assert_eq!(plan(&set.sheets[1]).scale, 750.0);
        near(
            plan(&set.sheets[1]).rotation.abs(),
            (288.75_f64 / 300.0).acos(),
            1e-9,
        );
    }

    #[test]
    fn the_front_ends_content_is_what_is_turned() {
        // The window draws a strip of imagery the document knows nothing of; the
        // plan is turned to it, as the editor turns the plan to what it draws.
        let mut document = Document::default();
        let mut interpreter = CommandInterpreter::new();
        let corners = strip_corners(Point2::new(5000.0, 7000.0), 450.0, 20.0, 45.0 * DEGREE);
        let asked = Rc::new(Cell::new(0));
        let counter = Rc::clone(&asked);
        interpreter.set_sheet_context(Some(Box::new(
            move |_document: &Document| -> SheetVerbContext<'static> {
                counter.set(counter.get() + 1);
                let mut extent = Box2::default();
                for corner in &corners {
                    extent.expand(*corner);
                }
                let content = corners.clone();
                SheetVerbContext {
                    drawing_extent: Some(extent),
                    content: Some(Box::new(
                        move |_document: &Document, _viewport: &Viewport| content.clone(),
                    )),
                    ..SheetVerbContext::default()
                }
            },
        )));
        let reply = interpreter.run(&mut document, "GENERATE fit rotate=on");
        if let Err(error) = &reply {
            panic!("{}", error.describe());
        }
        let set = document.sheet_set();
        assert_eq!(set.sheets.len(), 1);
        let view = plan(&set.sheets[0]);
        assert_eq!(view.scale, 1250.0);
        near(view.rotation, turn(), 1e-9);
        near(view.centre.x, 5000.0, 1e-6);
        near(view.centre.y, 7000.0, 1e-6);
        assert!(asked.get() >= 1);
    }

    #[test]
    fn what_cannot_be_turned_is_refused_and_changes_nothing() {
        let mut s = Session::default();
        s.ok("ALIGN NEW ROAD 0,0 1000,0");
        let steps = s.steps();
        // A plan along an alignment already follows it.
        assert_eq!(
            s.refused("GENERATE fit alignment=ROAD rotate=on").message,
            "turning the drawing to fill the sheet needs a plan of an area"
        );
        // Nor are sections after the plan turned with it.
        assert_eq!(
            s.refused("GENERATE fit area=0,0,100,100 interval=20 rotate=on")
                .message,
            "only a plan of an area is turned to fill the sheet; a strip already follows its \
             alignment"
        );
        assert_eq!(
            s.refused("GENERATE fit rotate=sideways").message,
            "rotate= is on or off"
        );
        assert_eq!(
            s.refused("GENERATE grid scale=500 rotate=on").message,
            "GENERATE GRID takes no option rotate=; it takes paper orientation frame area scale \
             overlap keyplan empty replace"
        );
        assert!(s.set().sheets.is_empty());
        assert_eq!(s.steps(), steps);
        // An empty drawing has nothing to turn, or to lay out.
        let mut empty = Session::default();
        assert!(contains(
            &empty.refused("GENERATE fit rotate=on").message,
            "the drawing is empty"
        ));
    }

    #[test]
    fn content_that_is_one_point_is_left_square() {
        // What the front end says the plan shows is one point: it has no size
        // to turn, so the layout is the square one over the extent.
        let mut document = Document::default();
        let mut interpreter = CommandInterpreter::new();
        interpreter.set_sheet_context(Some(Box::new(
            |_document: &Document| -> SheetVerbContext<'static> {
                SheetVerbContext {
                    drawing_extent: Some(Box2::new(
                        Point2::new(0.0, 0.0),
                        Point2::new(300.0, 100.0),
                    )),
                    content: Some(Box::new(|_document: &Document, _viewport: &Viewport| {
                        vec![Point2::new(5.0, 5.0)]
                    })),
                    ..SheetVerbContext::default()
                }
            },
        )));
        assert!(
            interpreter
                .run(&mut document, "GENERATE fit rotate=on")
                .is_ok()
        );
        assert!(interpreter.run(&mut document, "GENERATE fit").is_ok());
        let set = document.sheet_set();
        assert_eq!(set.sheets.len(), 2);
        assert_eq!(plan(&set.sheets[0]).rotation, 0.0);
        assert_eq!(plan(&set.sheets[0]).scale, plan(&set.sheets[1]).scale);
        assert_eq!(plan(&set.sheets[0]).centre, plan(&set.sheets[1]).centre);
    }

    #[test]
    fn the_help_names_the_option() {
        assert!(contains(&sheet_verb_help(), "rotate=on"));
    }
}
