// Terrain > Edit Surface's tools (tools/terrain_edit.rs,
// docs/surface_editing.md; tests/cad/terrain/test_surface_edit_tools.cpp),
// driven as the plan view drives them: each click or pick becomes the SURFACE
// EDIT line a person would type, on the active surface, as one undo step.

mod surface_edit_tools {
    use std::cell::Cell;
    use std::rc::Rc;

    use katana_cad::interactive_tool::{
        ToolInput, ToolStep, ToolStepOutcome, tool_catalog, tool_catalog_problems,
    };
    use katana_cad::terrain::surface_edit_verbs::{
        add_surface_edit_target_observer, remove_surface_edit_target_observer,
        set_surface_edit_target, surface_edit_target,
    };
    use katana_cad::{CommandInterpreter, FeedbackRole};
    use katana_commands::{EntityAttributes, create_point, create_polyline};
    use katana_entity::EntityId;
    use katana_geometry::{Point2, Point3, Polyline2};

    use crate::surface_edit_support::{
        flat_grid_surface, make_surface, put_surface, pyramid_surface, square_surface, surface_of,
    };
    use crate::tool_driver::ToolDriver;
    use crate::{double_eq, level, near, ok};

    // The C++ fixture: the edit target cleared before and after each case.
    // The target is one for the thread (cad_plan.md, risk 4) and each case
    // runs on a thread of its own, so the clearing only keeps the C++'s
    // lines; it is harmless.
    struct SurfaceEditTools {
        driver: ToolDriver,
    }

    impl SurfaceEditTools {
        fn new() -> Self {
            set_surface_edit_target(String::new());
            Self {
                driver: ToolDriver::new(),
            }
        }

        fn polyline(&mut self, vertices: Vec<Point2>, closed: bool) -> EntityId {
            self.driver.add(create_polyline(
                Polyline2::new(vertices, closed),
                EntityAttributes::default(),
            ))
        }

        // Delete Triangles Inside's Outside choice is held by its catalogue
        // entry, which these tests share, so each says what it wants rather
        // than toggling.
        fn outside(&self) -> bool {
            self.driver.prompt().contains("OUTSIDE it")
        }

        #[track_caller]
        fn set_outside(&mut self, wanted: bool) {
            if self.outside() != wanted {
                self.driver.r#type("O");
            }
            assert_eq!(self.outside(), wanted);
        }
    }

    impl Drop for SurfaceEditTools {
        fn drop(&mut self) {
            set_surface_edit_target(String::new());
        }
    }

    fn rejected(step: &ToolStep) -> bool {
        step.outcome == ToolStepOutcome::Rejected
    }

    fn p(x: f64, y: f64) -> Point2 {
        Point2::new(x, y)
    }

    #[track_caller]
    fn last_message(driver: &ToolDriver) -> String {
        match driver.messages().last() {
            Some(message) => message.clone(),
            None => panic!("the tool reported nothing"),
        }
    }

    #[test]
    fn every_tool_is_in_the_terrain_category_of_the_catalogue() {
        let _fixture = SurfaceEditTools::new();
        for id in [
            "terrain.edit.deletetriangle",
            "terrain.edit.flip",
            "terrain.edit.addpoint",
            "terrain.edit.deletepoint",
            "terrain.edit.movepoint",
            "terrain.edit.deleteinside",
            "terrain.edit.breakline",
            "terrain.edit.boundary",
            "terrain.edit.hole",
        ] {
            let Some(info) = tool_catalog().find(id) else {
                panic!("{id}");
            };
            assert_eq!(info.category, "Terrain", "{id}");
            assert_eq!(info.group, "Surface Edit", "{id}");
            assert!(!info.aliases.is_empty(), "{id}");
        }
        assert!(tool_catalog_problems().is_empty());
        assert!(tool_catalog().find_by_alias("tinflip").is_some());
        assert!(tool_catalog().find_by_alias("tinmovpt").is_some());
    }

    #[test]
    fn delete_triangle_runs_the_verb_on_the_active_surface_as_one_undo_step() {
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        f.driver.start("terrain.edit.deletetriangle");
        let step = f.driver.click(0.6, 0.2);
        assert_eq!(step.outcome, ToolStepOutcome::Done, "{}", step.message);
        assert_eq!(f.driver.executed(), 1);
        assert_eq!(
            surface_of(f.driver.document(), "ground").triangle_count(),
            31
        );
        let message = last_message(&f.driver);
        assert!(
            message.starts_with("SURFACE EDIT ground DELETE TRIANGLE 0.6,0.2\n"),
            "{message}"
        );
        assert!(!f.driver.finished()); // it starts again for the next triangle
        ok(f.driver.document_mut().undo());
        assert_eq!(
            surface_of(f.driver.document(), "ground").triangle_count(),
            32
        );
    }

    #[test]
    fn flip_edge_flips_the_edge_nearest_the_click() {
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            square_surface(0.0, 1.0, 0.0, 1.0),
            true,
        );
        f.driver.start("terrain.edit.flip");
        f.driver.click(5.0, 5.0);
        near(
            level(surface_of(f.driver.document(), "ground"), 5.0, 5.0),
            1.0,
            1e-12,
        );
    }

    #[test]
    fn add_point_takes_the_typed_level_the_surfaces_on_enter_or_a_typed_xyz() {
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            square_surface(0.0, 0.0, 0.0, 0.0),
            true,
        );
        f.driver.start("terrain.edit.addpoint");
        assert_eq!(f.driver.click(5.0, 5.0).outcome, ToolStepOutcome::Continue);
        assert_eq!(f.driver.expects(), ToolInput::Value);
        f.driver.r#type("10");
        double_eq(
            level(surface_of(f.driver.document(), "ground"), 5.0, 5.0),
            10.0,
        );
        assert_eq!(surface_of(f.driver.document(), "ground").vertex_count(), 5);

        // Enter takes the surface's own level there: a vertex, no change of
        // shape. (2,8) lies on the spoke from (0,10,0) to the new apex
        // (5,5,10), 0.4 of the way up: level 4.
        let there = level(surface_of(f.driver.document(), "ground"), 2.0, 8.0);
        near(there, 4.0, 1e-12);
        f.driver.click(2.0, 8.0);
        f.driver.enter();
        assert_eq!(surface_of(f.driver.document(), "ground").vertex_count(), 6);
        near(
            level(surface_of(f.driver.document(), "ground"), 2.0, 8.0),
            there,
            1e-12,
        );

        f.driver.r#type("8,2,4");
        double_eq(
            level(surface_of(f.driver.document(), "ground"), 8.0, 2.0),
            4.0,
        );
        assert_eq!(f.driver.executed(), 3);
    }

    #[test]
    fn delete_point_takes_the_vertex_within_the_views_reach() {
        // A pick tolerance of 0.5 gives a vertex reach of 0.75 (12 px to 8
        // px).
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            pyramid_surface(10.0),
            true,
        );
        f.driver.set_pick_tolerance(0.5);
        f.driver.start("terrain.edit.deletepoint");
        assert!(rejected(&f.driver.click(6.0, 6.0))); // 1.41 from the apex
        f.driver.click(5.3, 5.3);
        assert_eq!(
            surface_of(f.driver.document(), "ground").triangle_count(),
            2
        );
    }

    #[test]
    fn hole_boundary_and_delete_inside_take_a_picked_closed_polyline() {
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            flat_grid_surface(10, 1.0, 4.0, 0.0, 0.0),
            true,
        );
        let hole = f.polyline(
            vec![p(3.0, 3.0), p(6.0, 3.0), p(6.0, 7.0), p(3.0, 7.0)],
            true,
        );
        f.driver.start("terrain.edit.hole");
        f.driver.pick(hole, 3.0, 5.0);
        near(
            surface_of(f.driver.document(), "ground").plan_area(),
            88.0,
            1e-9,
        );

        let clip = f.polyline(
            vec![p(1.0, 1.0), p(9.0, 1.0), p(9.0, 9.0), p(1.0, 9.0)],
            true,
        );
        f.driver.start("terrain.edit.boundary");
        f.driver.pick(clip, 1.0, 5.0);
        near(
            surface_of(f.driver.document(), "ground").plan_area(),
            64.0 - 12.0,
            1e-9,
        );

        // Outside [1,2] x [1,9]: only that strip's 8 x 2 triangles stay, area
        // 8.
        let strip = f.polyline(
            vec![p(1.0, 1.0), p(2.0, 1.0), p(2.0, 9.0), p(1.0, 9.0)],
            true,
        );
        f.driver.start("terrain.edit.deleteinside");
        f.set_outside(true);
        f.driver.pick(strip, 1.0, 5.0);
        near(
            surface_of(f.driver.document(), "ground").plan_area(),
            8.0,
            1e-9,
        );
        f.set_outside(false);
    }

    #[test]
    fn add_breakline_takes_a_picked_polyline_draped() {
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            pyramid_surface(10.0),
            true,
        );
        let line = f.polyline(vec![p(0.0, 2.5), p(10.0, 2.5)], false);
        f.driver.start("terrain.edit.breakline");
        f.driver.pick(line, 5.0, 2.5);
        let message = last_message(&f.driver);
        assert!(message.contains("crossings=2"), "{message}");
        near(
            level(surface_of(f.driver.document(), "ground"), 2.5, 2.5),
            5.0,
            1e-12,
        );
    }

    #[test]
    fn a_pick_of_the_wrong_kind_is_refused() {
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        let open = f.polyline(vec![p(1.0, 1.0), p(3.0, 1.0), p(3.0, 3.0)], false);
        f.driver.start("terrain.edit.hole");
        assert!(rejected(&f.driver.pick(open, 2.0, 1.0)));
        let point = f
            .driver
            .add(create_point(p(2.0, 2.0), EntityAttributes::default()));
        f.driver.start("terrain.edit.breakline");
        assert!(rejected(&f.driver.pick(point, 2.0, 2.0)));
        assert_eq!(f.driver.executed(), 0);
    }

    #[test]
    fn the_surface_option_chooses_the_surface_and_the_toolbar_keeps_it() {
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        put_surface(
            f.driver.document_mut(),
            "pad",
            flat_grid_surface(4, 1.0, 2.0, 0.0, 0.0),
            true,
        );
        f.driver.start("terrain.edit.deletetriangle");
        assert!(f.driver.prompt().contains("ground"));
        f.driver.r#type("S");
        assert!(rejected(&f.driver.r#type("nowhere")));
        f.driver.r#type("pad");
        assert_eq!(surface_edit_target(), "pad");
        f.driver.click(0.6, 0.2);
        assert_eq!(surface_of(f.driver.document(), "pad").triangle_count(), 31);
        assert_eq!(
            surface_of(f.driver.document(), "ground").triangle_count(),
            32
        );
    }

    #[test]
    fn with_no_surface_a_tool_says_so_and_refuses_every_click() {
        let mut f = SurfaceEditTools::new();
        f.driver.start("terrain.edit.flip");
        assert!(f.driver.prompt().contains("no surface"));
        assert!(rejected(&f.driver.click(1.0, 1.0)));
    }

    #[test]
    fn the_preview_marks_what_a_click_would_take_and_what_it_would_refuse() {
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        f.driver.start("terrain.edit.deletetriangle");
        let over = f.driver.preview(0.6, 0.2);
        assert_eq!(over.marks.len(), 1);
        assert_eq!(over.marks[0].role, FeedbackRole::Removed);
        assert!(f.driver.preview(9.0, 9.0).refused);

        f.driver.start("terrain.edit.flip");
        assert!(f.driver.preview(2.5, 0.01).refused); // nearest the rim along y = 0
        assert!(!f.driver.preview(2.5, 2.5).refused); // a diagonal inside
    }

    #[test]
    fn the_outside_choice_of_delete_triangles_inside_survives_the_restart_after_an_edit() {
        // A run of picks is a run of clicks: each edit ends the tool and the
        // host makes it again, and the Outside the person chose went with the
        // old one, so the second polygon's INSIDE was deleted instead.
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            flat_grid_surface(10, 1.0, 4.0, 0.0, 0.0),
            true,
        );
        let first = f.polyline(
            vec![p(1.0, 1.0), p(9.0, 1.0), p(9.0, 9.0), p(1.0, 9.0)],
            true,
        );
        let second = f.polyline(
            vec![p(2.0, 2.0), p(8.0, 2.0), p(8.0, 8.0), p(2.0, 8.0)],
            true,
        );
        f.driver.start("terrain.edit.deleteinside");
        f.set_outside(true);
        // Outside [1,9]^2 goes: 64 stays, in 128 triangles.
        f.driver.pick(first, 1.0, 5.0);
        near(
            surface_of(f.driver.document(), "ground").plan_area(),
            64.0,
            1e-9,
        );
        assert!(
            f.outside(),
            "the restarted tool still says OUTSIDE: {}",
            f.driver.prompt()
        );
        // And the second pick deletes outside [2,8]^2 of what is left: 36
        // stays.
        f.driver.pick(second, 2.0, 5.0);
        near(
            surface_of(f.driver.document(), "ground").plan_area(),
            36.0,
            1e-9,
        );
        let message = last_message(&f.driver);
        assert!(message.contains("DELETE TRIANGLES OUTSIDE #"), "{message}");
        f.set_outside(false);
        assert!(!f.outside());
    }

    #[test]
    fn delete_point_offers_no_vertex_on_a_breakline_or_boundary_edge_that_the_click_refuses() {
        // The square with its diagonal 0-2 a breakline (triangle 0 runs it as
        // edge 2, triangle 1 as edge 0). Vertex 0 at (0,0) is on it; vertex 1
        // at (10,0) is on the rim only, which Delete Point may take.
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            make_surface(
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(10.0, 0.0, 0.0),
                    Point3::new(10.0, 10.0, 0.0),
                    Point3::new(0.0, 10.0, 0.0),
                ],
                vec![[0, 1, 2], [0, 2, 3]],
                vec![4, 1],
            ),
            true,
        );
        f.driver.set_pick_tolerance(0.5);
        f.driver.start("terrain.edit.deletepoint");
        let kept = f.driver.preview(0.1, 0.1);
        assert!(kept.refused);
        assert!(kept.caption.contains("kept"), "{}", kept.caption);
        let free = f.driver.preview(9.9, 0.1);
        assert!(!free.refused, "{}", free.caption);
        assert_eq!(free.caption, "delete this vertex");
        // What the preview says is what the click does.
        assert!(rejected(&f.driver.click(0.1, 0.1)));
        assert!(!rejected(&f.driver.click(9.9, 0.1)));
    }

    #[test]
    fn a_curve_polyline_drawn_back_to_its_start_is_a_polygon_to_the_pick_as_to_the_verb() {
        // PLINE with an arc, ending where it began, is a curve polyline with
        // the closed flag off - which the verb takes as a polygon (the one
        // reader of the drawing's shapes) and the tool's own copy of the rule
        // refused.
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            flat_grid_surface(10, 10.0, 4.0, 0.0, 0.0),
            true,
        );
        let mut interpreter = CommandInterpreter::new();
        if let Err(error) = interpreter.run(
            f.driver.document_mut(),
            "PLINE 20,20 60,20 ARC 60,60 LINE 20,60 20,20",
        ) {
            panic!("{}", error.describe());
        }
        let curve = f.driver.document().last_created_entities()[0];
        f.driver.start("terrain.edit.hole");
        let step = f.driver.pick(curve, 20.0, 40.0);
        assert!(!rejected(&step), "{}", step.message);
        assert_eq!(f.driver.executed(), 1);
        // The square 20..60 is 1600 and the arc bulges by a little; the hole
        // is between that and the 3600 of the ground's cells it crosses.
        assert!(surface_of(f.driver.document(), "ground").plan_area() < 10000.0 - 1500.0);
        // A line is still no polygon.
        let open = f.polyline(vec![p(1.0, 1.0), p(3.0, 1.0), p(3.0, 3.0)], false);
        assert!(rejected(&f.driver.pick(open, 2.0, 1.0)));
    }

    #[test]
    fn the_surface_option_tells_the_toolbar_and_only_while_it_is_listening() {
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        put_surface(
            f.driver.document_mut(),
            "pad",
            flat_grid_surface(4, 1.0, 2.0, 0.0, 0.0),
            true,
        );
        // The C++ observer counts into a local it captures by reference; the
        // observers are 'static closures here (H5), so the count is shared.
        let told = Rc::new(Cell::new(0));
        let counter = Rc::clone(&told);
        let handle = add_surface_edit_target_observer(move || counter.set(counter.get() + 1));
        f.driver.start("terrain.edit.deletetriangle");
        f.driver.r#type("S");
        assert_eq!(
            told.get(),
            0,
            "choosing the option is not choosing the surface"
        );
        f.driver.r#type("pad");
        assert_eq!(told.get(), 1);
        assert_eq!(surface_edit_target(), "pad");
        remove_surface_edit_target_observer(handle);
        set_surface_edit_target(String::from("ground"));
        assert_eq!(told.get(), 1, "a removed observer is not called");
    }

    #[test]
    fn move_point_takes_a_vertex_then_a_target_and_runs_the_verb_as_one_undo_step() {
        // A pick tolerance of 0.5 gives a vertex reach of 0.75: the click at
        // (2.1, 2.1) is 0.14 from the vertex (2,2) of the 4 x 4 grid.
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        let before = surface_of(f.driver.document(), "ground").clone();
        f.driver.set_pick_tolerance(0.5);
        f.driver.start("terrain.edit.movepoint");
        assert!(
            f.driver.prompt().contains("to move"),
            "{}",
            f.driver.prompt()
        );
        assert!(
            rejected(&f.driver.click(10.0, 10.0)),
            "no vertex within reach of a point off the surface"
        );
        let first = f.driver.click(2.1, 2.1);
        assert_eq!(
            first.outcome,
            ToolStepOutcome::Continue,
            "{}",
            first.message
        );
        assert_eq!(f.driver.expects(), ToolInput::Point);
        assert!(f.driver.prompt().contains("goes"), "{}", f.driver.prompt());
        assert!(f.driver.last_point().is_some());
        assert_eq!(
            f.driver.last_point(),
            Some(p(2.0, 2.0)),
            "the vertex itself, not the click"
        );
        // A target the verb refuses leaves the vertex held for another click.
        assert!(rejected(&f.driver.click(9.0, 9.0)));
        assert_eq!(f.driver.executed(), 0);
        assert_eq!(f.driver.expects(), ToolInput::Point);

        let second = f.driver.click(3.5, 2.2);
        assert_eq!(second.outcome, ToolStepOutcome::Done, "{}", second.message);
        assert_eq!(f.driver.executed(), 1);
        let message = last_message(&f.driver);
        assert!(
            message.starts_with("SURFACE EDIT ground MOVE POINT 2,2 TO 3.5,2.2\n"),
            "{message}"
        );
        let moved = surface_of(f.driver.document(), "ground");
        near(moved.plan_area(), 16.0, 1e-9);
        assert_eq!(moved.vertex_count(), 25);
        ok(f.driver.document_mut().undo());
        assert!(*surface_of(f.driver.document(), "ground") == before);
        assert!(!f.driver.finished(), "it starts again for the next vertex");
    }

    #[test]
    fn move_point_refuses_a_held_vertex_at_the_first_click_and_the_undo_lets_go_of_one() {
        // The row y = 4 from x = 1 to 7 is a breakline; the corner (0,0) is on
        // the rim. Neither is picked up. A free vertex is, and Undo lets go of
        // it.
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            flat_grid_surface(10, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        let mut interpreter = CommandInterpreter::new();
        if let Err(error) = interpreter.run(
            f.driver.document_mut(),
            "SURFACE EDIT ground BREAKLINE 1,4 7,4",
        ) {
            panic!("{}", error.describe());
        }
        f.driver.set_pick_tolerance(0.5);
        f.driver.start("terrain.edit.movepoint");
        let on_line = f.driver.click(4.1, 4.1);
        assert!(rejected(&on_line));
        assert!(on_line.message.contains("kept"), "{}", on_line.message);
        assert!(rejected(&f.driver.click(0.1, 0.1)));
        assert_eq!(f.driver.expects(), ToolInput::Point);
        assert!(
            f.driver.prompt().contains("to move"),
            "still at the first click"
        );

        assert_eq!(f.driver.click(6.1, 6.1).outcome, ToolStepOutcome::Continue);
        assert!(f.driver.prompt().contains("goes"));
        assert_eq!(f.driver.undo().outcome, ToolStepOutcome::Continue);
        assert!(
            f.driver.prompt().contains("to move"),
            "{}",
            f.driver.prompt()
        );
        assert!(f.driver.last_point().is_none());
        assert_eq!(
            f.driver.executed(),
            0,
            "the breakline was added by the test, and no move was made"
        );
    }

    #[test]
    fn the_preview_of_move_point_marks_the_vertex_then_where_it_would_go_and_what_it_refuses() {
        let mut f = SurfaceEditTools::new();
        put_surface(
            f.driver.document_mut(),
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        f.driver.set_pick_tolerance(0.5);
        f.driver.start("terrain.edit.movepoint");
        // Over a free vertex: marked as the one to move. Over a rim vertex:
        // refused, saying it is kept.
        let free = f.driver.preview(2.05, 2.05);
        assert!(!free.refused, "{}", free.caption);
        assert_eq!(free.marks.len(), 1);
        assert_eq!(free.marks[0].role, FeedbackRole::Target);
        assert_eq!(free.caption, "move this vertex");
        let rim = f.driver.preview(0.05, 0.05);
        assert!(rim.refused);
        assert!(rim.caption.contains("kept"), "{}", rim.caption);
        assert!(f.driver.preview(9.0, 9.0).refused);

        // After the first click: the vertex where it is and where it would
        // go; off the surface, refused.
        assert_eq!(
            f.driver.click(2.05, 2.05).outcome,
            ToolStepOutcome::Continue
        );
        let target = f.driver.preview(3.5, 2.2);
        assert!(!target.refused, "{}", target.caption);
        assert_eq!(target.marks.len(), 2);
        assert_eq!(target.marks[0].role, FeedbackRole::Removed);
        assert_eq!(target.marks[1].role, FeedbackRole::Added);
        assert!(f.driver.preview(9.0, 9.0).refused);
    }
}
