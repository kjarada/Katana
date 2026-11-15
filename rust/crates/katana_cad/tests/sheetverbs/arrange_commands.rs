// Arranging a project's sheets as undoable steps
// (include/katana/cad/plotting/arrange_commands.hpp): each command is ONE
// step, undone whole; a refused one records nothing; and what a view shows is
// taken from the drawing when the caller gives nothing;
// tests/cad/plotting/test_arrange_commands.cpp.
//
// The A3 tiling area is 24..409 x 36..286 mm, 385 x 250; with the painter's
// 4% to spare a view that fills it holds 370.19 x 240.38 mm of drawing.

use std::f64::consts::PI;

use crate::{expect_box, near, ok, refused};
use katana_cad::Document;
use katana_cad::plot::PaperSize;
use katana_cad::plotting::arrange::{AUTO_SCALE_SPARE, FIT_SPARE, viewport_content};
use katana_cad::plotting::arrange_commands::{
    align_viewports, auto_arrange_sheet, choose_paper_for_scale, distribute_viewports, drawn_scale,
    fit_viewport_to_content, main_plan_of, match_scale, rotate_to_best_fit,
};
use katana_cad::plotting::layout::{TILING_GUTTER_MM, preset_cells, tiling_area};
use katana_cad::plotting::sheet_commands::add_sheet;
use katana_cad::plotting::{
    AlignEdge, DistributeAxis, Sheet, TilingPreset, Viewport, ViewportKind,
};
use katana_commands::{EntityAttributes, create_line};
use katana_core::ErrorCode;
use katana_geometry::{Box2, Point2};

fn viewport_at(id: &str, kind: ViewportKind, rect: Box2) -> Viewport {
    Viewport {
        id: String::from(id),
        kind,
        rect,
        ..Viewport::default()
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Box2 {
    Box2::new(Point2::new(x0, y0), Point2::new(x1, y1))
}

// One sheet holding `viewports`, added as one step. The set is empty, so the
// ids are given out afresh from vp1 in order, as they are written here.
fn add_sheet_with(document: &mut Document, viewports: Vec<Viewport>) {
    let sheet = Sheet {
        id: String::from("s1"),
        name: String::from("TEST"),
        viewports,
        ..Sheet::default()
    };
    ok(add_sheet(document, sheet, None));
}

fn viewport_of(document: &Document, id: &str) -> Viewport {
    document
        .sheet_set()
        .sheets
        .iter()
        .flat_map(|sheet| sheet.viewports.iter())
        .find(|viewport| viewport.id == id)
        .cloned()
        .unwrap_or_else(|| panic!("no viewport {id}"))
}

fn add_line(document: &mut Document, a: Point2, b: Point2) {
    ok(document.execute(create_line(a, b, EntityAttributes::default())));
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|id| String::from(*id)).collect()
}

mod sheet_arrange_commands {
    use super::*;

    #[test]
    fn auto_arrange_is_one_step_and_undo_puts_the_views_back() {
        let mut document = Document::new(None);
        let blank = Sheet::default();
        let full = preset_cells(TilingPreset::Full, &tiling_area(&blank), TILING_GUTTER_MM)[0];
        let dropped = rect(100.0, 100.0, 180.0, 200.0);
        add_sheet_with(
            &mut document,
            vec![
                viewport_at("vp1", ViewportKind::Plan, full),
                viewport_at("vp2", ViewportKind::Legend, dropped),
            ],
        );
        let before = document.history().undo_count();

        let result = ok(auto_arrange_sheet(&mut document, 0));
        assert_eq!(document.history().undo_count(), before + 1);
        assert_eq!(result.moved, ids(&["vp1", "vp2"]));
        assert!(result.main_shrunk);
        assert!(result.overlapping.is_empty());

        // Arranged again, nothing changes and no step is recorded.
        let again = ok(auto_arrange_sheet(&mut document, 0));
        assert!(again.moved.is_empty());
        assert_eq!(document.history().undo_count(), before + 1);

        ok(document.undo());
        assert_eq!(viewport_of(&document, "vp1").rect, full);
        assert_eq!(viewport_of(&document, "vp2").rect, dropped);

        assert_eq!(
            refused(auto_arrange_sheet(&mut document, 3)).code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn aligning_and_spacing_are_one_step_each_and_a_refusal_records_nothing() {
        let mut document = Document::new(None);
        add_sheet_with(
            &mut document,
            vec![
                viewport_at("vp1", ViewportKind::Notes, rect(30.0, 40.0, 80.0, 80.0)),
                viewport_at("vp2", ViewportKind::Notes, rect(100.0, 50.0, 130.0, 120.0)),
                viewport_at("vp3", ViewportKind::Notes, rect(300.0, 45.0, 360.0, 60.0)),
            ],
        );
        let all = ids(&["vp1", "vp2", "vp3"]);
        let before = document.history().undo_count();

        let aligned = ok(align_viewports(&mut document, 0, &all, AlignEdge::Bottom));
        assert_eq!(aligned, ids(&["vp2", "vp3"]));
        assert_eq!(document.history().undo_count(), before + 1);
        assert_eq!(viewport_of(&document, "vp2").rect.min.y, 40.0);
        assert_eq!(viewport_of(&document, "vp3").rect.min.y, 40.0);

        // The first ends at 80 and the last starts at 300; vp2 is 30 wide:
        // gaps of (220 - 30) / 2 = 95, so vp2 starts at 175.
        let spaced = ok(distribute_viewports(
            &mut document,
            0,
            &all,
            DistributeAxis::Horizontal,
        ));
        assert_eq!(spaced, ids(&["vp2"]));
        assert_eq!(document.history().undo_count(), before + 2);
        near(viewport_of(&document, "vp2").rect.min.x, 175.0, 1e-9);

        let unknown = ids(&["vp1", "zz"]);
        assert_eq!(
            refused(align_viewports(&mut document, 0, &unknown, AlignEdge::Left)).code,
            ErrorCode::NotFound
        );
        let two = ids(&["vp1", "vp2"]);
        assert_eq!(
            refused(distribute_viewports(
                &mut document,
                0,
                &two,
                DistributeAxis::Vertical
            ))
            .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(align_viewports(&mut document, 1, &all, AlignEdge::Left)).code,
            ErrorCode::NotFound
        );
        assert_eq!(document.history().undo_count(), before + 2);

        ok(document.undo());
        ok(document.undo());
        assert_eq!(
            viewport_of(&document, "vp3").rect,
            rect(300.0, 45.0, 360.0, 60.0)
        );
    }

    #[test]
    fn matching_an_automatic_plan_takes_the_scale_it_is_drawn_at() {
        // A 300 x 100 m drawing in a plan filling the sheet, centred on it:
        // with 4% to spare it needs 1 : max(300000 / 370.19, 100000 /
        // 240.38) = 1 : 810.4, so it is drawn at 1 : 1000, whatever it last
        // stored.
        let mut document = Document::new(None);
        add_line(
            &mut document,
            Point2::new(0.0, 0.0),
            Point2::new(300.0, 100.0),
        );
        let mut plan = viewport_at("vp1", ViewportKind::Plan, tiling_area(&Sheet::default()));
        plan.auto_scale = true;
        plan.auto_centre = true;
        plan.scale = 500.0;
        let mut section = viewport_at(
            "vp2",
            ViewportKind::LongSection,
            rect(30.0, 40.0, 300.0, 120.0),
        );
        section.scale = 200.0;
        add_sheet_with(&mut document, vec![plan, section]);
        let content = viewport_content(document.model(), &viewport_of(&document, "vp1"));
        assert_eq!(
            drawn_scale(&viewport_of(&document, "vp1"), &content),
            1000.0
        );
        // Drawn round a centre of its own at (0, 0), the painter measures the
        // drawing about that point: 600 x 200 m, 1 : 1620.8, so 1 : 2000.
        let mut off_centre = viewport_of(&document, "vp1");
        off_centre.auto_centre = false;
        assert_eq!(drawn_scale(&off_centre, &content), 2000.0);
        // A view at a scale of its own is drawn at it.
        off_centre.auto_scale = false;
        assert_eq!(drawn_scale(&off_centre, &content), 500.0);
        // Turned, the painter measures the drawing's BOX, not its outline. A
        // diamond (50, 0), (100, 50), (50, 100), (0, 50) turned 45 degrees is
        // a 70.7 m square, but its 100 x 100 m box turned is 141.4 m across:
        // in 200 x 200 mm with 4% to spare that needs 1 : 735.4, so 1 : 750,
        // not the 1 : 367.7 (1 : 500) the diamond alone would need.
        let mut turned = viewport_at("vp9", ViewportKind::Plan, rect(30.0, 40.0, 230.0, 240.0));
        turned.auto_scale = true;
        turned.auto_centre = true;
        turned.rotation = PI / 4.0;
        let diamond = [
            Point2::new(50.0, 0.0),
            Point2::new(100.0, 50.0),
            Point2::new(50.0, 100.0),
            Point2::new(0.0, 50.0),
        ];
        assert_eq!(drawn_scale(&turned, &diamond), 750.0);
        let before = document.history().undo_count();

        let targets = ids(&["vp2"]);
        let changed = ok(match_scale(&mut document, &targets, "vp1", None, None));
        assert_eq!(changed, ids(&["vp2"]));
        assert_eq!(viewport_of(&document, "vp2").scale, 1000.0);
        assert_eq!(document.history().undo_count(), before + 1);
        // A scale given wins.
        ok(match_scale(
            &mut document,
            &targets,
            "vp1",
            Some(750.0),
            None,
        ));
        assert_eq!(viewport_of(&document, "vp2").scale, 750.0);
        assert_eq!(
            refused(match_scale(&mut document, &targets, "vp7", None, None)).code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn fitting_a_view_to_the_drawing_is_one_step() {
        // A 100 x 50 m line at 1 : 500 is 200 x 100 mm, and 5% more: 210 x 105
        // about the view's centre (150, 150).
        let mut document = Document::new(None);
        add_line(
            &mut document,
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 50.0),
        );
        let mut plan = viewport_at("vp1", ViewportKind::Plan, rect(100.0, 100.0, 200.0, 200.0));
        plan.scale = 500.0;
        let section = viewport_at(
            "vp2",
            ViewportKind::LongSection,
            rect(30.0, 220.0, 300.0, 280.0),
        );
        add_sheet_with(&mut document, vec![plan, section]);
        let before = document.history().undo_count();

        ok(fit_viewport_to_content(&mut document, "vp1", &[], None));
        assert_eq!(document.history().undo_count(), before + 1);
        expect_box(
            &viewport_of(&document, "vp1").rect,
            45.0,
            97.5,
            255.0,
            202.5,
            1e-9,
        );
        near(viewport_of(&document, "vp1").centre.x, 50.0, 1e-9);
        near(viewport_of(&document, "vp1").centre.y, 25.0, 1e-9);

        // At a scale given (an automatic view's, as drawn) 1 : 1000 halves it.
        ok(fit_viewport_to_content(
            &mut document,
            "vp1",
            &[],
            Some(1000.0),
        ));
        near(viewport_of(&document, "vp1").rect.width(), 105.0, 1e-9);
        assert_eq!(viewport_of(&document, "vp1").scale, 1000.0);

        // A section's ground is not in the drawing: its points must be given.
        assert_eq!(
            refused(fit_viewport_to_content(&mut document, "vp2", &[], None)).code,
            ErrorCode::InvalidArgument
        );
        let profile = [Point2::new(0.0, 10.0), Point2::new(100.0, 15.0)];
        ok(fit_viewport_to_content(
            &mut document,
            "vp2",
            &profile,
            None,
        ));
        near(
            viewport_of(&document, "vp2").rect.width(),
            100.0 * 1000.0 / 500.0 * FIT_SPARE,
            1e-9,
        );
        assert_eq!(
            refused(fit_viewport_to_content(&mut document, "vp9", &[], None)).code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_plan_of_a_diagonal_road_is_turned_just_far_enough_to_reach_the_next_scale() {
        // A 424.26 m line at 45 degrees. Square, the view needs 1 : 300000 /
        // 240.38 = 1248, so 1 : 1250; laid along the diagonal it would need
        // 1 : 961, so 1 : 1000 is to be had. The least turn that reaches it
        // leaves the line's height 240.38 m: t = 45 - asin(240.38 / 424.26) =
        // 10.49 degrees.
        let mut document = Document::new(None);
        add_line(
            &mut document,
            Point2::new(0.0, 0.0),
            Point2::new(300.0, 300.0),
        );
        let mut plan = viewport_at("vp1", ViewportKind::Plan, tiling_area(&Sheet::default()));
        plan.auto_scale = true;
        plan.auto_centre = true;
        add_sheet_with(&mut document, vec![plan]);
        let before = document.history().undo_count();

        let fit = ok(rotate_to_best_fit(&mut document, "vp1", &[]));
        assert_eq!(document.history().undo_count(), before + 1);
        let t = PI / 4.0 - (250.0 / AUTO_SCALE_SPARE / 300.0_f64.hypot(300.0)).asin();
        near(fit.rotation, t, 1e-9);
        assert_eq!(fit.standard_scale, 1000.0);
        let turned = viewport_of(&document, "vp1");
        near(turned.rotation, t, 1e-9);
        assert_eq!(turned.scale, 1000.0);
        assert!(!turned.auto_scale);
        near(turned.centre.x, 150.0, 1e-9);
        near(turned.centre.y, 150.0, 1e-9);

        ok(document.undo());
        assert_eq!(viewport_of(&document, "vp1").rotation, 0.0);
        assert!(viewport_of(&document, "vp1").auto_scale);
        assert_eq!(
            refused(rotate_to_best_fit(&mut document, "vp8", &[])).code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn choosing_paper_for_the_main_plan_is_one_step() {
        // A 500 x 300 m drawing in a plan at 1 : 1000 filling an A3 sheet
        // needs A2 landscape (520 x 312 mm with 4% to spare, in 545.27 x
        // 354.36).
        let mut document = Document::new(None);
        let corners = [
            Point2::new(0.0, 0.0),
            Point2::new(500.0, 0.0),
            Point2::new(500.0, 300.0),
            Point2::new(0.0, 300.0),
        ];
        for i in 0..corners.len() {
            add_line(&mut document, corners[i], corners[(i + 1) % corners.len()]);
        }
        let legend = viewport_at("vp1", ViewportKind::Legend, rect(300.0, 40.0, 400.0, 100.0));
        let mut plan = viewport_at("vp2", ViewportKind::Plan, tiling_area(&Sheet::default()));
        plan.scale = 1000.0;
        add_sheet_with(&mut document, vec![legend, plan]);
        assert_eq!(main_plan_of(&document.sheet_set(), 0), "vp2");
        assert_eq!(main_plan_of(&document.sheet_set(), 5), "");
        let before = document.history().undo_count();

        let change = ok(choose_paper_for_scale(&mut document, "vp2", &[], None));
        assert_eq!(change.advice.paper, PaperSize::A2);
        assert!(change.advice.landscape);
        assert_eq!(document.history().undo_count(), before + 1);
        let sheet = document.sheet_set().sheets[0].clone();
        assert_eq!(sheet.paper, PaperSize::A2);
        assert_eq!(viewport_of(&document, "vp2").rect, tiling_area(&sheet));
        assert_eq!(viewport_of(&document, "vp2").scale, 1000.0);

        // At a scale given: 1 : 2000 fits A4 (260 x 156 mm in 271.64 x
        // 176.18).
        ok(choose_paper_for_scale(
            &mut document,
            "vp2",
            &[],
            Some(2000.0),
        ));
        assert_eq!(document.sheet_set().sheets[0].paper, PaperSize::A4);
        assert_eq!(viewport_of(&document, "vp2").scale, 2000.0);

        ok(document.undo());
        ok(document.undo());
        assert_eq!(document.sheet_set().sheets[0].paper, PaperSize::A3);
        assert_eq!(
            refused(choose_paper_for_scale(&mut document, "vp1", &[], None)).code,
            ErrorCode::InvalidArgument
        );
    }
}
