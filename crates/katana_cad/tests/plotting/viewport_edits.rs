// Several viewports at once, and the paper geometry of the sheet editor
// (include/katana/cad/plotting/viewport_edits.hpp): the paper grid and its
// snapping, the rubber band, Tab's order, a plan's paper and world, and the
// group edits - move, remove, copy, paste, duplicate - each ONE undoable step;
// tests/cad/plotting/test_viewport_edits.cpp.

use std::f64::consts::PI;

use crate::{double_eq, near, rect};
use katana_cad::Document;
use katana_cad::plotting::viewport_edits::{
    DUPLICATE_VIEWPORTS, MOVE_VIEWPORTS, PASTE_STEP_MM, PASTE_VIEWPORTS, REMOVE_VIEWPORTS,
    copy_viewports, cycle_viewport, duplicate_viewports, move_viewports, paste_offset,
    paste_viewports, plan_paper_to_world, plan_world_to_paper, remove_viewports, snap_edge,
    snap_moving_rect, snap_to_grid, viewport_bounds, viewports_in_band,
};
use katana_cad::plotting::{BandMode, Sheet, SheetSet, Viewport, ViewportKind};
use katana_core::ErrorCode;
use katana_geometry::{Box2, Point2};

fn ok<T>(result: katana_core::Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn panel(id: &str, area: Box2, kind: ViewportKind) -> Viewport {
    Viewport {
        id: String::from(id),
        kind,
        rect: area,
        ..Viewport::default()
    }
}

// Two sheets: s1 with three panels, vp3 locked; s2 with one.
fn two_sheets() -> SheetSet {
    let mut first = Sheet {
        id: String::from("s1"),
        name: String::from("FIRST"),
        viewports: vec![
            panel("vp1", rect(100.0, 100.0, 200.0, 180.0), ViewportKind::Notes),
            panel(
                "vp2",
                rect(220.0, 100.0, 300.0, 180.0),
                ViewportKind::Legend,
            ),
            panel("vp3", rect(320.0, 100.0, 380.0, 180.0), ViewportKind::Image),
        ],
        ..Sheet::default()
    };
    first.viewports[2].locked = true;
    let second = Sheet {
        id: String::from("s2"),
        name: String::from("SECOND"),
        viewports: vec![panel(
            "vp4",
            rect(50.0, 50.0, 90.0, 90.0),
            ViewportKind::Notes,
        )],
        ..Sheet::default()
    };
    SheetSet {
        sheets: vec![first, second],
        ..SheetSet::default()
    }
}

fn by_id(document: &Document, sheet: usize, id: &str) -> Viewport {
    document.sheet_set().sheets[sheet]
        .viewports
        .iter()
        .find(|viewport| viewport.id == id)
        .cloned()
        .unwrap_or_else(|| panic!("no viewport {id}"))
}

fn ids_of(sheet: &Sheet) -> Vec<String> {
    sheet
        .viewports
        .iter()
        .map(|viewport| viewport.id.clone())
        .collect()
}

fn ids(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| String::from(*name)).collect()
}

fn document_with_two_sheets() -> Document {
    let mut document = Document::default();
    ok(document.set_sheet_set(&two_sheets(), Document::SET_SHEETS));
    document
}

mod viewport_edits {
    use super::*;

    // ---- the paper grid ----------------------------------------------------------------

    #[test]
    fn a_value_snaps_to_the_nearest_grid_line_through_the_papers_origin() {
        double_eq(snap_to_grid(12.4, 5.0), 10.0);
        double_eq(snap_to_grid(12.6, 5.0), 15.0);
        double_eq(snap_to_grid(12.5, 5.0), 15.0); // halfway goes up
        double_eq(snap_to_grid(-2.4, 5.0), 0.0);
        double_eq(snap_to_grid(-2.6, 5.0), -5.0);
        // No grid: the value as it is.
        double_eq(snap_to_grid(12.4, 0.0), 12.4);
        double_eq(snap_to_grid(12.4, -5.0), 12.4);
    }

    #[test]
    fn a_moving_rectangle_snaps_an_edge_to_the_grid_where_no_edge_is_in_reach() {
        let area = rect(23.0, 35.0, 410.0, 287.0);
        // Its left edge is 1.2 past a line, its right edge 2.4 past one; the
        // left is the smaller move.
        let left = snap_moving_rect(&rect(111.2, 121.9, 212.4, 171.9), &area, &[], 1.0, 5.0);
        double_eq(left.rect.min.x, 110.0);
        double_eq(left.rect.max.x, 211.2);
        // 121.9 is 1.9 from 120, and 171.9 is 1.9 from 170 too: the first edge.
        near(left.rect.min.y, 120.0, 1e-9);
        near(left.rect.height(), 50.0, 1e-9);
        // The grid draws no guide.
        assert!(left.guide_x.is_none());
        assert!(left.guide_y.is_none());

        // The right edge nearer a line: it goes there, and the width is kept.
        let right = snap_moving_rect(&rect(111.2, 120.0, 214.0, 170.0), &area, &[], 1.0, 5.0);
        near(right.rect.max.x, 215.0, 1e-9);
        near(right.rect.width(), 102.8, 1e-9);
    }

    #[test]
    fn an_edge_in_reach_wins_over_the_grid() {
        let area = rect(23.0, 35.0, 410.0, 287.0);
        // A neighbour whose right edge, 201.3, is off the grid; the moving
        // panel's left edge is 0.5 from it.
        let others = [rect(150.0, 40.0, 201.3, 60.0)];
        let snapped = snap_moving_rect(&rect(201.8, 121.0, 251.8, 171.0), &area, &others, 1.0, 5.0);
        double_eq(snapped.rect.min.x, 201.3);
        double_eq(snapped.guide_x.expect("a guide"), 201.3);
        // Nothing in reach vertically: the grid.
        assert!(snapped.guide_y.is_none());
        double_eq(snapped.rect.min.y, 120.0);

        // No grid: only the edge snapping, as snap_rect.
        let plain = snap_moving_rect(&rect(201.8, 121.0, 251.8, 171.0), &area, &others, 1.0, 0.0);
        double_eq(plain.rect.min.y, 121.0);
    }

    #[test]
    fn a_resized_edge_snaps_to_a_target_in_reach_else_to_the_grid() {
        let targets = [201.3, 250.0];
        let on_target = snap_edge(200.9, &targets, 1.0, 5.0);
        double_eq(on_target.value, 201.3);
        double_eq(on_target.guide.expect("a guide"), 201.3);

        let on_grid = snap_edge(207.4, &targets, 1.0, 5.0);
        double_eq(on_grid.value, 205.0);
        assert!(on_grid.guide.is_none());

        let free = snap_edge(207.4, &targets, 1.0, 0.0);
        double_eq(free.value, 207.4);
        assert!(free.guide.is_none());
    }

    // ---- picking -----------------------------------------------------------------------

    #[test]
    fn a_window_takes_what_it_encloses_and_a_crossing_what_it_touches() {
        let sheet = two_sheets().sheets.remove(0);
        // Around vp1 wholly and half of vp2.
        let band = rect(90.0, 90.0, 260.0, 190.0);
        assert_eq!(
            viewports_in_band(&sheet, &band, BandMode::Window),
            ids(&["vp1"])
        );
        assert_eq!(
            viewports_in_band(&sheet, &band, BandMode::Crossing),
            ids(&["vp1", "vp2"])
        );
        // An empty band picks nothing; an unplaced viewport is never picked.
        assert!(viewports_in_band(&sheet, &Box2::default(), BandMode::Crossing).is_empty());
        let mut with_unplaced = sheet.clone();
        with_unplaced
            .viewports
            .push(panel("vp9", Box2::default(), ViewportKind::Notes));
        assert_eq!(
            viewports_in_band(
                &with_unplaced,
                &rect(0.0, 0.0, 500.0, 500.0),
                BandMode::Window
            ),
            ids(&["vp1", "vp2", "vp3"])
        );
    }

    #[test]
    fn the_bounds_of_several_viewports_span_them_all() {
        let sheet = two_sheets().sheets.remove(0);
        assert_eq!(
            viewport_bounds(&sheet, &ids(&["vp1", "vp3"])),
            rect(100.0, 100.0, 380.0, 180.0)
        );
        assert!(viewport_bounds(&sheet, &ids(&["nothing"])).empty());
    }

    #[test]
    fn tab_steps_through_the_placed_viewports_and_wraps_round() {
        let mut sheet = two_sheets().sheets.remove(0);
        sheet
            .viewports
            .insert(1, panel("vp9", Box2::default(), ViewportKind::Notes));
        assert_eq!(cycle_viewport(&sheet, "", true), "vp1");
        assert_eq!(cycle_viewport(&sheet, "", false), "vp3");
        assert_eq!(cycle_viewport(&sheet, "vp1", true), "vp2"); // the unplaced vp9 is skipped
        assert_eq!(cycle_viewport(&sheet, "vp3", true), "vp1");
        assert_eq!(cycle_viewport(&sheet, "vp1", false), "vp3");
        assert_eq!(cycle_viewport(&sheet, "gone", true), "vp1");
        assert_eq!(cycle_viewport(&Sheet::default(), "", true), "");
    }

    // ---- a plan's paper and its world --------------------------------------------------

    #[test]
    fn a_paper_point_of_a_plan_is_the_world_point_drawn_there() {
        // A plan 200 x 150 mm centred at (200, 175) on the paper, at 1:500,
        // with (1000, 2000) at its centre.
        let mut plan = panel("vp1", rect(100.0, 100.0, 300.0, 250.0), ViewportKind::Plan);
        let centre = Point2::new(1000.0, 2000.0);
        assert_eq!(
            plan_paper_to_world(&plan, 500.0, centre, Point2::new(200.0, 175.0)),
            centre
        );
        // 20 mm right at 1:500 is 10 m east; 10 mm up is 5 m north.
        let east = plan_paper_to_world(&plan, 500.0, centre, Point2::new(220.0, 185.0));
        near(east.x, 1010.0, 1e-9);
        near(east.y, 2005.0, 1e-9);

        // Turned so north runs left to right across the paper: right is north,
        // up is west.
        plan.rotation = PI / 2.0;
        let north = plan_paper_to_world(&plan, 500.0, centre, Point2::new(220.0, 185.0));
        near(north.x, 995.0, 1e-9);
        near(north.y, 2010.0, 1e-9);
        // And back again.
        let paper = plan_world_to_paper(&plan, 500.0, centre, north);
        near(paper.x, 220.0, 1e-9);
        near(paper.y, 185.0, 1e-9);
    }

    // ---- the edits ---------------------------------------------------------------------

    #[test]
    fn several_viewports_move_in_one_step_and_a_locked_one_stays() {
        let mut document = document_with_two_sheets();
        let before = document.history().undo_count();
        ok(move_viewports(
            &mut document,
            0,
            &ids(&["vp1", "vp2", "vp3"]),
            Point2::new(10.0, -5.0),
            MOVE_VIEWPORTS,
        ));
        assert_eq!(document.history().undo_count(), before + 1);
        assert_eq!(
            by_id(&document, 0, "vp1").rect,
            rect(110.0, 95.0, 210.0, 175.0)
        );
        assert_eq!(
            by_id(&document, 0, "vp2").rect,
            rect(230.0, 95.0, 310.0, 175.0)
        );
        assert_eq!(
            by_id(&document, 0, "vp3").rect,
            rect(320.0, 100.0, 380.0, 180.0)
        ); // locked
        // The other sheet is untouched.
        assert_eq!(
            by_id(&document, 1, "vp4").rect,
            rect(50.0, 50.0, 90.0, 90.0)
        );

        ok(document.undo());
        assert!(*document.sheet_set() == two_sheets());
    }

    #[test]
    fn a_move_of_nothing_or_of_a_stranger_is_refused_and_changes_nothing() {
        let mut document = document_with_two_sheets();
        let before = document.history().undo_count();
        // vp4 is on the other sheet.
        let stranger = move_viewports(
            &mut document,
            0,
            &ids(&["vp1", "vp4"]),
            Point2::new(1.0, 0.0),
            MOVE_VIEWPORTS,
        )
        .expect_err("vp4 is not on the sheet");
        assert_eq!(stranger.code, ErrorCode::NotFound);
        assert!(stranger.context.contains("vp4"));
        assert_eq!(
            move_viewports(&mut document, 0, &[], Point2::new(1.0, 0.0), MOVE_VIEWPORTS)
                .expect_err("nothing to move")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            move_viewports(
                &mut document,
                5,
                &ids(&["vp1"]),
                Point2::new(1.0, 0.0),
                MOVE_VIEWPORTS
            )
            .expect_err("no sheet 5")
            .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            move_viewports(
                &mut document,
                0,
                &ids(&["vp1"]),
                Point2::new(f64::NAN, 0.0),
                MOVE_VIEWPORTS
            )
            .expect_err("not a distance")
            .code,
            ErrorCode::InvalidArgument
        );
        // Only the locked one: nothing moves, so no step.
        ok(move_viewports(
            &mut document,
            0,
            &ids(&["vp3"]),
            Point2::new(1.0, 0.0),
            MOVE_VIEWPORTS,
        ));
        assert_eq!(document.history().undo_count(), before);
        assert!(*document.sheet_set() == two_sheets());
    }

    #[test]
    fn several_viewports_go_in_one_step_locked_or_not() {
        let mut document = document_with_two_sheets();
        let before = document.history().undo_count();
        // A repeated id counts once.
        ok(remove_viewports(
            &mut document,
            0,
            &ids(&["vp1", "vp3", "vp1"]),
            REMOVE_VIEWPORTS,
        ));
        assert_eq!(document.history().undo_count(), before + 1);
        assert_eq!(ids_of(&document.sheet_set().sheets[0]), ids(&["vp2"]));
        ok(document.undo());
        assert_eq!(
            ids_of(&document.sheet_set().sheets[0]),
            ids(&["vp1", "vp2", "vp3"])
        );
        assert_eq!(
            remove_viewports(&mut document, 0, &ids(&["vp4"]), REMOVE_VIEWPORTS)
                .expect_err("vp4 is not on the sheet")
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_copy_is_in_the_sheets_order_whatever_the_order_asked_for() {
        let set = two_sheets();
        let copies = ok(copy_viewports(&set, 0, &ids(&["vp3", "vp1"])));
        assert_eq!(copies.len(), 2);
        assert_eq!(copies[0], set.sheets[0].viewports[0]);
        assert_eq!(copies[1], set.sheets[0].viewports[2]);
        assert_eq!(
            copy_viewports(&set, 0, &ids(&["vp4"]))
                .expect_err("vp4 is not on the sheet")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            copy_viewports(&set, 2, &ids(&["vp1"]))
                .expect_err("no sheet 2")
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_paste_steps_clear_of_the_rectangles_already_there() {
        let set = two_sheets();
        let clip = vec![set.sheets[0].viewports[0].clone()];
        // On its own sheet the original is where the copy was: one step on.
        assert_eq!(
            paste_offset(&set.sheets[0], &clip, PASTE_STEP_MM),
            Point2::new(5.0, -5.0)
        );
        // On another sheet: where it was.
        assert_eq!(
            paste_offset(&set.sheets[1], &clip, PASTE_STEP_MM),
            Point2::new(0.0, 0.0)
        );
        // With a copy one step on already, two steps.
        let mut crowded = set.sheets[0].clone();
        crowded.viewports.push(panel(
            "vp8",
            rect(105.0, 95.0, 205.0, 175.0),
            ViewportKind::Notes,
        ));
        assert_eq!(
            paste_offset(&crowded, &clip, PASTE_STEP_MM),
            Point2::new(10.0, -10.0)
        );
        assert_eq!(paste_offset(&crowded, &clip, 2.0), Point2::new(2.0, -2.0));
    }

    #[test]
    fn pasting_onto_another_sheet_is_one_step_with_new_ids() {
        let mut document = document_with_two_sheets();
        let clip = ok(copy_viewports(
            &document.sheet_set(),
            0,
            &ids(&["vp1", "vp2"]),
        ));
        let before = document.history().undo_count();
        let pasted = ok(paste_viewports(
            &mut document,
            1,
            clip.clone(),
            None,
            PASTE_VIEWPORTS,
        ));
        assert_eq!(document.history().undo_count(), before + 1);
        // New ids after the highest in the set, in the clipboard's order.
        assert_eq!(pasted, ids(&["vp5", "vp6"]));
        let set = document.sheet_set();
        let second = &set.sheets[1];
        assert_eq!(ids_of(second), ids(&["vp4", "vp5", "vp6"]));
        // Where they were, and otherwise the same.
        let mut expected = clip[1].clone();
        expected.id = String::from("vp6");
        assert_eq!(second.viewports[2], expected);
        assert_eq!(second.viewports[1].rect, rect(100.0, 100.0, 200.0, 180.0));
        // The first sheet is as it was.
        assert!(set.sheets[0] == two_sheets().sheets[0]);

        ok(document.undo());
        assert!(*document.sheet_set() == two_sheets());
    }

    #[test]
    fn repeated_pastes_onto_the_same_sheet_step_down_the_sheet() {
        let mut document = document_with_two_sheets();
        let clip = ok(copy_viewports(&document.sheet_set(), 0, &ids(&["vp1"])));
        let first = ok(paste_viewports(
            &mut document,
            0,
            clip.clone(),
            None,
            PASTE_VIEWPORTS,
        ));
        let second = ok(paste_viewports(
            &mut document,
            0,
            clip.clone(),
            None,
            PASTE_VIEWPORTS,
        ));
        assert_eq!(
            by_id(&document, 0, &first[0]).rect,
            rect(105.0, 95.0, 205.0, 175.0)
        );
        assert_eq!(
            by_id(&document, 0, &second[0]).rect,
            rect(110.0, 90.0, 210.0, 170.0)
        );
        // An offset given is used as it is.
        let placed = ok(paste_viewports(
            &mut document,
            0,
            clip,
            Some(Point2::new(0.0, 0.0)),
            PASTE_VIEWPORTS,
        ));
        assert_eq!(
            by_id(&document, 0, &placed[0]).rect,
            rect(100.0, 100.0, 200.0, 180.0)
        );
        assert_eq!(
            paste_viewports(&mut document, 0, Vec::new(), None, PASTE_VIEWPORTS)
                .expect_err("nothing to paste")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn duplicating_is_one_step_that_places_the_copies_one_step_on() {
        let mut document = document_with_two_sheets();
        let before = document.history().undo_count();
        let copies = ok(duplicate_viewports(
            &mut document,
            0,
            &ids(&["vp2", "vp1"]),
            DUPLICATE_VIEWPORTS,
        ));
        assert_eq!(document.history().undo_count(), before + 1);
        assert_eq!(copies, ids(&["vp5", "vp6"])); // vp1's copy first: the sheet's order
        assert_eq!(
            by_id(&document, 0, "vp5").rect,
            rect(105.0, 95.0, 205.0, 175.0)
        );
        assert_eq!(by_id(&document, 0, "vp6").kind, ViewportKind::Legend);
        assert_eq!(
            ids_of(&document.sheet_set().sheets[0]),
            ids(&["vp1", "vp2", "vp3", "vp5", "vp6"])
        );
        assert_eq!(
            duplicate_viewports(&mut document, 0, &ids(&["vp4"]), DUPLICATE_VIEWPORTS)
                .expect_err("vp4 is not on the sheet")
                .code,
            ErrorCode::NotFound
        );
    }
}
